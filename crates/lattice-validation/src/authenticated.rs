//! Authenticated storage adapter for the pure state executor. It resolves only
//! touched paths, then loads any sibling witness required for canonical collapse.
use crate::*;
use lattice_cashew::{ContentSource, Header, Transform, Volume, VolumeMerkleDictionary};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error(transparent)]
    State(#[from] TransitionError),
    #[error(transparent)]
    Content(#[from] lattice_cashew::Error),
    #[error(transparent)]
    Diff(#[from] StateDiffOverflow),
}
#[derive(Debug)]
pub struct MaterializedTransition {
    pub state: LatticeStateHeader,
    pub account_growth: i64,
    pub diff: StateDiff,
}

pub(crate) fn load<T: Clone + Serialize + DeserializeOwned>(
    header: &Header<T>,
    source: &impl ContentSource,
) -> Result<T, lattice_cashew::Error> {
    // Even a materialized public Header is checked against its commitment.
    if header.encryption_info.is_some() {
        return Err(lattice_cashew::Error::Unsupported);
    }
    if let Some(node) = header.node.as_deref() {
        lattice_cashew::verify_content_address(
            &header.raw_cid,
            &lattice_cashew::cbor::encode(node)?,
        )?;
        return Ok(node.clone());
    }
    let bytes = source.fetch(&header.raw_cid)?;
    lattice_cashew::verify_content_address(&header.raw_cid, &bytes)?;
    lattice_cashew::cbor::decode(&bytes)
}
type TargetedState<V> = (Option<VolumeMerkleDictionary<V>>, BTreeMap<String, V>);

pub(crate) fn targeted<V: Clone + Serialize + DeserializeOwned>(
    header: &Volume<VolumeMerkleDictionary<V>>,
    keys: &BTreeSet<String>,
    source: &impl ContentSource,
) -> Result<TargetedState<V>, lattice_cashew::Error> {
    if keys.is_empty() {
        return Ok((None, BTreeMap::new()));
    }
    let mut tree = load(&header.0, source)?;
    let mut values = BTreeMap::new();
    for key in keys {
        tree = tree.resolve_key(key, source)?;
        if let Some(value) = tree.get(key)? {
            values.insert(key.clone(), value.clone());
        }
    }
    Ok((Some(tree), values))
}
fn update<V: Clone + Serialize + DeserializeOwned + PartialEq>(
    header: &Volume<VolumeMerkleDictionary<V>>,
    tree: Option<VolumeMerkleDictionary<V>>,
    before: &BTreeMap<String, V>,
    after: &BTreeMap<String, V>,
    source: &impl ContentSource,
) -> Result<(Volume<VolumeMerkleDictionary<V>>, StateDiff), ExecutionError> {
    let mut operations = BTreeMap::new();
    for (key, value) in before {
        match after.get(key) {
            None => {
                operations.insert(key.clone(), Transform::Delete);
            }
            Some(new) if new != value => {
                operations.insert(key.clone(), Transform::Update(new.clone()));
            }
            _ => {}
        }
    }
    for (key, value) in after {
        if !before.contains_key(key) {
            operations.insert(key.clone(), Transform::Insert(value.clone()));
        }
    }
    if operations.is_empty() {
        return Ok((header.clone(), StateDiff::default()));
    }
    let tree =
        tree.ok_or_else(|| lattice_cashew::Error::MissingContent(header.0.raw_cid.clone()))?;
    let witness = tree.prove_transforms(&operations, source)?;
    let old = Volume::new(witness.clone())?;
    let new = Volume::new(witness.transform(&operations)?)?;
    let diff = diff_state_dictionary(&old, &new)?;
    Ok((new, diff))
}

enum ExecutionMode<'a> {
    Block,
    Genesis(&'a ChainSpec),
    Template(Option<&'a AccountAction>),
}

pub struct AuthenticatedBlockExecution<'a> {
    pub previous: &'a LatticeStateHeader,
    pub parent: &'a LatticeStateHeader,
    pub directory: &'a str,
    pub actions: &'a TransitionActions,
    pub transaction_bodies: &'a [TransactionBody],
    pub reward: u64,
    pub recipient: Option<&'a str>,
}
impl AuthenticatedBlockExecution<'_> {
    pub fn execute(
        &self,
        source: &impl ContentSource,
    ) -> Result<MaterializedTransition, ExecutionError> {
        self.execute_mode(source, ExecutionMode::Block)
    }
    pub fn execute_genesis(
        &self,
        source: &impl ContentSource,
        spec: &ChainSpec,
    ) -> Result<MaterializedTransition, ExecutionError> {
        if self.recipient.is_some() || self.reward != 0 {
            return Err(TransitionError::Conflict.into());
        }
        self.execute_mode(source, ExecutionMode::Genesis(spec))
    }
    /// Construct a template's state, without claiming signature, fee, policy or
    /// parent-receipt admission. The builder supplies its already-derived coinbase.
    pub(crate) fn execute_template(
        &self,
        source: &impl ContentSource,
        coinbase: Option<&AccountAction>,
    ) -> Result<MaterializedTransition, ExecutionError> {
        self.execute_mode(source, ExecutionMode::Template(coinbase))
    }
    fn execute_mode(
        &self,
        source: &impl ContentSource,
        mode: ExecutionMode<'_>,
    ) -> Result<MaterializedTransition, ExecutionError> {
        let previous = load(&self.previous.0, source)?;
        let mut account_keys = BTreeSet::new();
        for action in &self.actions.accounts {
            account_keys.insert(action.owner.clone());
        }
        for action in &self.actions.receipts {
            account_keys.insert(action.withdrawer.clone());
            account_keys.insert(action.demander.clone());
        }
        for body in self.transaction_bodies {
            if let Some(signer) = body.signers.first() {
                account_keys.insert(format!("{NONCE_PREFIX}{signer}"));
            }
        }
        if let Some(recipient) = self.recipient {
            account_keys.insert(recipient.into());
        }
        if let ExecutionMode::Template(Some(credit)) = &mode {
            account_keys.insert(credit.owner.clone());
        }
        let general_keys = self.actions.general.iter().map(|a| a.key.clone()).collect();
        let mut deposit_keys = BTreeSet::new();
        for action in &self.actions.deposits {
            deposit_keys.insert(
                DepositKey {
                    demander: action.demander.clone(),
                    amount_demanded: action.amount_demanded,
                    nonce: action.nonce,
                }
                .to_string(),
            );
        }
        let mut parent_keys = BTreeSet::new();
        for action in &self.actions.withdrawals {
            deposit_keys.insert(
                DepositKey {
                    demander: action.demander.clone(),
                    amount_demanded: action.amount_demanded,
                    nonce: action.nonce,
                }
                .to_string(),
            );
            parent_keys.insert(
                ReceiptKey {
                    directory: self.directory.into(),
                    demander: action.demander.clone(),
                    amount_demanded: action.amount_demanded,
                    nonce: action.nonce,
                }
                .storage_key(),
            );
        }
        let receipt_keys = self
            .actions
            .receipts
            .iter()
            .map(|a| {
                ReceiptKey {
                    directory: a.directory.clone(),
                    demander: a.demander.clone(),
                    amount_demanded: a.amount_demanded,
                    nonce: a.nonce,
                }
                .storage_key()
            })
            .collect();
        let (accounts, account_values) = targeted(&previous.account_state, &account_keys, source)?;
        let (general, general_values) = targeted(&previous.general_state, &general_keys, source)?;
        let (deposits, deposit_values) = targeted(&previous.deposit_state, &deposit_keys, source)?;
        let (receipts, receipt_values) = targeted(&previous.receipt_state, &receipt_keys, source)?;
        let before = ResolvedState {
            accounts: account_values,
            general: general_values,
            deposits: deposit_values,
            receipts: receipt_values,
        };
        let parents = if parent_keys.is_empty() || matches!(mode, ExecutionMode::Template(_)) {
            BTreeMap::new()
        } else {
            let parent = load(&self.parent.0, source)?;
            targeted(&parent.receipt_state, &parent_keys, source)?.1
        };
        let result = match mode {
            ExecutionMode::Genesis(spec) => {
                execute_genesis(&before, self.actions, self.transaction_bodies, spec)?
            }
            ExecutionMode::Block => execute_block(
                &before,
                &parents,
                self.directory,
                self.actions,
                self.transaction_bodies,
                self.reward,
                self.recipient,
            )?,
            ExecutionMode::Template(coinbase) => {
                let mut accounts = self.actions.accounts.clone();
                if let Some(credit) = coinbase {
                    accounts.push(credit.clone());
                }
                execute_state(
                    &before,
                    None,
                    self.directory,
                    self.actions,
                    self.transaction_bodies,
                    &accounts,
                )?
            }
        };
        let (account_state, mut diff) = update(
            &previous.account_state,
            accounts,
            &before.accounts,
            &result.state.accounts,
            source,
        )?;
        let (general_state, change) = update(
            &previous.general_state,
            general,
            &before.general,
            &result.state.general,
            source,
        )?;
        diff.merge(&change)?;
        // Swift emits two deposit diffs: spend first, insert second. Preserve
        // intermediate reference counts as well as the final commitment.
        let mut spent = before.deposits.clone();
        for withdrawal in &self.actions.withdrawals {
            let key = DepositKey {
                demander: withdrawal.demander.clone(),
                amount_demanded: withdrawal.amount_demanded,
                nonce: withdrawal.nonce,
            }
            .to_string();
            spent.insert(key, 0);
        }
        let (after_spending, change) = update(
            &previous.deposit_state,
            deposits,
            &before.deposits,
            &spent,
            source,
        )?;
        diff.merge(&change)?;
        let (spent_tree, _) = targeted(&after_spending, &deposit_keys, source)?;
        let (deposit_state, change) = update(
            &after_spending,
            spent_tree,
            &spent,
            &result.state.deposits,
            source,
        )?;
        diff.merge(&change)?;
        let (receipt_state, change) = update(
            &previous.receipt_state,
            receipts,
            &before.receipts,
            &result.state.receipts,
            source,
        )?;
        diff.merge(&change)?;
        let state = Volume::new(LatticeState {
            account_state,
            general_state,
            deposit_state,
            receipt_state,
        })?;
        if self.previous.0.raw_cid != state.0.raw_cid {
            let roots = StateDiff {
                replaced: BTreeMap::from([(self.previous.0.raw_cid.clone(), 1)]),
                created: BTreeMap::from([(state.0.raw_cid.clone(), 1)]),
            };
            diff.merge(&roots)?;
        }
        Ok(MaterializedTransition {
            state,
            account_growth: result.account_growth,
            diff,
        })
    }
}

/// Authenticate withdrawal authorization before fee validation or unrelated
/// state reads, matching the composed block validator's consensus ordering.
pub fn validate_authenticated_withdrawals(
    previous: &LatticeStateHeader,
    parent: &LatticeStateHeader,
    directory: &str,
    withdrawals: &[WithdrawalAction],
    source: &impl ContentSource,
) -> Result<(), ExecutionError> {
    if withdrawals.is_empty() {
        return Ok(());
    }
    let mut deposit_keys = BTreeSet::new();
    let mut receipt_keys = BTreeSet::new();
    for action in withdrawals {
        let key = DepositKey {
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .to_string();
        let receipt = ReceiptKey {
            directory: directory.into(),
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .storage_key();
        if !receipt_keys.insert(receipt) {
            return Err(TransitionError::Conflict.into());
        }
        deposit_keys.insert(key);
    }
    let previous = load(&previous.0, source)?;
    let parent = load(&parent.0, source)?;
    let deposits = targeted(&previous.deposit_state, &deposit_keys, source)?.1;
    let receipts = targeted(&parent.receipt_state, &receipt_keys, source)?.1;
    for action in withdrawals {
        let key = DepositKey {
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .to_string();
        if action.amount_withdrawn == 0
            || action.amount_demanded == 0
            || deposits.get(&key) != Some(&action.amount_withdrawn)
        {
            return Err(TransitionError::Conflict.into());
        }
        let receipt = ReceiptKey {
            directory: directory.into(),
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .storage_key();
        if receipts.get(&receipt) != Some(&action.withdrawer) {
            return Err(TransitionError::ParentReceipt.into());
        }
    }
    Ok(())
}
