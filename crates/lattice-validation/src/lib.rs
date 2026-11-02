//! Pure state execution. Inputs must come from authenticated state resolution;
//! success does not establish signatures, policies, ancestry or block validity.
#![forbid(unsafe_code)]
pub mod authenticated;
pub mod builder;
pub use builder::*;
pub mod block;
pub use block::*;
pub mod content;
pub mod policy;
pub mod transaction;
pub use content::*;
pub use policy::*;
pub mod growth;
pub mod header;
pub use authenticated::*;
pub use growth::*;
pub use header::*;
use lattice_primitives::*;
use num_bigint::BigUint;
use std::collections::{BTreeMap, BTreeSet};
pub use transaction::*;

pub const NONCE_PREFIX: &str = "_nonce_";

/// Fully resolved state; receipt keys use the consensus SHA-256 storage spelling.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolvedState {
    pub accounts: BTreeMap<String, u64>,
    pub general: BTreeMap<String, String>,
    pub deposits: BTreeMap<String, u64>,
    pub receipts: BTreeMap<String, String>,
}
#[derive(Debug, Default, Clone)]
pub struct TransitionActions {
    pub accounts: Vec<AccountAction>,
    pub general: Vec<Action>,
    pub deposits: Vec<DepositAction>,
    pub receipts: Vec<ReceiptAction>,
    pub withdrawals: Vec<WithdrawalAction>,
}
impl TransitionActions {
    pub fn from_bodies(bodies: &[TransactionBody]) -> Self {
        let mut result = Self::default();
        for body in bodies {
            result.accounts.extend_from_slice(&body.account_actions);
            result.general.extend_from_slice(&body.actions);
            result.deposits.extend_from_slice(&body.deposit_actions);
            result.receipts.extend_from_slice(&body.receipt_actions);
            result
                .withdrawals
                .extend_from_slice(&body.withdrawal_actions);
        }
        result
    }
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("invalid account action")]
    InvalidAccountAction,
    #[error("fee rule violated")]
    FeeRule,
    #[error("invalid coinbase recipient")]
    InvalidRecipient,
    #[error("coinbase does not fit an Int64 action")]
    CoinbaseOverflow,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("balance overflow")]
    BalanceOverflow,
    #[error("conflicting state actions or old value mismatch")]
    Conflict,
    #[error("missing matching parent receipt")]
    ParentReceipt,
    #[error("nonce sequence gap or overflow")]
    NonceGap,
    #[error("state growth overflow")]
    GrowthOverflow,
}
impl From<StateError> for TransitionError {
    fn from(value: StateError) -> Self {
        match value {
            StateError::BalanceOverflow => Self::BalanceOverflow,
            StateError::NonceGap => Self::NonceGap,
            StateError::ConflictingActions => Self::Conflict,
            StateError::StateDeltaOverflow => Self::GrowthOverflow,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionResult {
    pub state: ResolvedState,
    pub account_growth: i64,
}

/// Exact fee arithmetic, independent of address validation and balance execution.
pub fn coinbase_amount(
    actions: &TransitionActions,
    reward: u64,
) -> Result<BigUint, TransitionError> {
    let mut credits = BigUint::default();
    let mut debits = BigUint::default();
    for action in &actions.accounts {
        if !action.verify() {
            return Err(TransitionError::InvalidAccountAction);
        }
        if action.delta < 0 {
            debits += action.absolute_amount();
        } else {
            credits += action.absolute_amount();
        }
    }
    for action in &actions.deposits {
        credits += action.amount_deposited;
    }
    for action in &actions.withdrawals {
        debits += action.amount_withdrawn;
    }
    if credits > debits {
        return Err(TransitionError::FeeRule);
    }
    Ok(debits - credits + reward)
}
pub fn coinbase_credit(
    actions: &TransitionActions,
    reward: u64,
    recipient: Option<&str>,
) -> Result<Option<AccountAction>, TransitionError> {
    let amount = coinbase_amount(actions, reward)?;
    let Some(recipient) = recipient else {
        return Ok(None);
    };
    if !standard_cid(recipient) {
        return Err(TransitionError::InvalidRecipient);
    }
    if amount == BigUint::default() {
        return Ok(None);
    }
    let amount = i64::try_from(amount).map_err(|_| TransitionError::CoinbaseOverflow)?;
    Ok(Some(AccountAction {
        owner: recipient.into(),
        delta: amount,
    }))
}

/// Execute one non-genesis block against resolved state. All changes are local
/// until success, so a late failure cannot partially mutate the caller's state.
/// Signature/admission checks are deliberately a separate layer.
pub fn execute_block(
    previous: &ResolvedState,
    parent_receipts: &BTreeMap<String, String>,
    directory: &str,
    actions: &TransitionActions,
    transaction_bodies: &[TransactionBody],
    reward: u64,
    recipient: Option<&str>,
) -> Result<TransitionResult, TransitionError> {
    let mut account_actions = actions.accounts.clone();
    if let Some(credit) = coinbase_credit(actions, reward, recipient)? {
        account_actions.push(credit);
    }
    execute_state(
        previous,
        Some(parent_receipts),
        directory,
        actions,
        transaction_bodies,
        &account_actions,
    )
}

/// Genesis permits only its scheduled premine and cannot contain swap actions.
/// The caller separately authenticates the empty entering state/genesis shape.
pub fn execute_genesis(
    previous: &ResolvedState,
    actions: &TransitionActions,
    transaction_bodies: &[TransactionBody],
    spec: &ChainSpec,
) -> Result<TransitionResult, TransitionError> {
    if !actions.deposits.is_empty()
        || !actions.withdrawals.is_empty()
        || !actions.receipts.is_empty()
        || !valid_genesis_credits(spec, &actions.accounts)
    {
        return Err(TransitionError::Conflict);
    }
    execute_state(
        previous,
        Some(&BTreeMap::new()),
        "Nexus",
        actions,
        transaction_bodies,
        &actions.accounts,
    )
}

fn execute_state(
    previous: &ResolvedState,
    parent_receipts: Option<&BTreeMap<String, String>>,
    directory: &str,
    actions: &TransitionActions,
    transaction_bodies: &[TransactionBody],
    account_actions: &[AccountAction],
) -> Result<TransitionResult, TransitionError> {
    let mut result = previous.clone();
    let account_growth = execute_accounts(
        &mut result.accounts,
        account_actions,
        &actions.receipts,
        transaction_bodies,
    )?;
    let mut seen = BTreeSet::new();
    for action in &actions.general {
        if !action.verify()
            || !seen.insert(&action.key)
            || previous.general.get(&action.key) != action.old_value.as_ref()
        {
            return Err(TransitionError::Conflict);
        }
        match &action.new_value {
            Some(value) => {
                result.general.insert(action.key.clone(), value.clone());
            }
            None => {
                result.general.remove(&action.key);
            }
        }
    }
    let mut seen = BTreeSet::new();
    // A spent deposit retains its identity forever. Insertion after withdrawal
    // therefore cannot resurrect that deposit, even within the same block.
    for action in &actions.withdrawals {
        let key = DepositKey {
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .to_string();
        if action.amount_withdrawn == 0
            || action.amount_demanded == 0
            || !seen.insert(key.clone())
            || result.deposits.get(&key) != Some(&action.amount_withdrawn)
        {
            return Err(TransitionError::Conflict);
        }
        let receipt = ReceiptKey {
            directory: directory.into(),
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .storage_key();
        if parent_receipts
            .is_some_and(|receipts| receipts.get(&receipt) != Some(&action.withdrawer))
        {
            return Err(TransitionError::ParentReceipt);
        }
        result.deposits.insert(key, 0);
    }
    for action in &actions.deposits {
        let key = DepositKey {
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .to_string();
        if action.amount_deposited == 0
            || action.amount_demanded == 0
            || result.deposits.contains_key(&key)
        {
            return Err(TransitionError::Conflict);
        }
        result.deposits.insert(key, action.amount_deposited);
    }
    for action in &actions.receipts {
        let key = ReceiptKey {
            directory: action.directory.clone(),
            demander: action.demander.clone(),
            amount_demanded: action.amount_demanded,
            nonce: action.nonce,
        }
        .storage_key();
        if action.amount_demanded == 0 || result.receipts.contains_key(&key) {
            return Err(TransitionError::Conflict);
        }
        result.receipts.insert(key, action.withdrawer.clone());
    }
    Ok(TransitionResult {
        state: result,
        account_growth,
    })
}

fn execute_accounts(
    state: &mut BTreeMap<String, u64>,
    actions: &[AccountAction],
    receipts: &[ReceiptAction],
    bodies: &[TransactionBody],
) -> Result<i64, TransitionError> {
    let deltas = net_balance_deltas(actions, receipts)?;
    let mut nonces: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for body in bodies {
        if body.signers.iter().any(|s| s.starts_with(NONCE_PREFIX)) {
            return Err(TransitionError::Conflict);
        }
        if let Some(signer) = body.signers.first() {
            nonces.entry(signer).or_default().push(body.nonce);
        }
    }
    let mut growth = 0i64;
    for (owner, delta) in deltas {
        if owner.starts_with(NONCE_PREFIX) {
            return Err(TransitionError::Conflict);
        }
        let existing = state.get(&owner).copied();
        let current = existing.unwrap_or(0);
        let next = match delta {
            AccountBalanceDelta::Credit(amount) => current
                .checked_add(amount)
                .ok_or(TransitionError::BalanceOverflow)?,
            AccountBalanceDelta::Debit(amount) => current
                .checked_sub(amount)
                .ok_or(TransitionError::InsufficientBalance)?,
        };
        let size = i64::try_from(owner.len())
            .ok()
            .and_then(|n| n.checked_add(8))
            .ok_or(TransitionError::GrowthOverflow)?;
        if current == 0 && next > 0 {
            if existing.is_some() {
                return Err(TransitionError::Conflict);
            }
            growth = growth
                .checked_add(size)
                .ok_or(TransitionError::GrowthOverflow)?;
        } else if current > 0 && next == 0 {
            growth = growth
                .checked_sub(size)
                .ok_or(TransitionError::GrowthOverflow)?;
        }
        if next == 0 {
            state.remove(&owner);
        } else {
            state.insert(owner, next);
        }
    }
    for (signer, mut nonces) in nonces {
        nonces.sort_unstable();
        let key = format!("{NONCE_PREFIX}{signer}");
        let current = state.get(&key).copied();
        let expected = next_expected_nonce(current)?;
        if nonces[0] != expected || nonces.windows(2).any(|p| p[0].checked_add(1) != Some(p[1])) {
            return Err(TransitionError::NonceGap);
        }
        if current.is_none() {
            let size = i64::try_from(key.len())
                .ok()
                .and_then(|n| n.checked_add(8))
                .ok_or(TransitionError::GrowthOverflow)?;
            growth = growth
                .checked_add(size)
                .ok_or(TransitionError::GrowthOverflow)?;
        }
        state.insert(key, *nonces.last().expect("nonempty signer group"));
    }
    Ok(growth)
}

pub mod preflight;
pub use preflight::*;
