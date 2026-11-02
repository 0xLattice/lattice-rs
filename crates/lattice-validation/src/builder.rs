//! Block templates matching the Swift builder. Construction is not admission:
//! signatures, policies, securing work and chain continuity are validated later.
use crate::*;
use lattice_cashew::{ContentSource, Header, MerkleDictionary, Volume};

#[derive(Debug, thiserror::Error)]
pub enum BlockBuilderError {
    #[error("block height overflow")]
    HeightOverflow,
    #[error("difficulty anchor does not exist in the supplied ancestry")]
    MissingDifficultyAnchor,
    #[error("invalid transaction body")]
    InvalidTransactionContent,
    #[error("invalid child directory or commitment: {0}")]
    ChildCommitment(String),
    #[error("state growth exceeds the chain limit")]
    StateGrowth,
    #[error(transparent)]
    Content(#[from] lattice_cashew::Error),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Execution(#[from] ExecutionError),
}
#[derive(Debug)]
pub struct BlockBuildResult {
    pub block: Block,
    pub state_diff: StateDiff,
    pub materialized_post_state: LatticeStateHeader,
}

/// Resolve the height-one origin, checking each height/spec link. Missing bytes
/// remain errors; structurally invalid ancestry returns None.
pub fn resolve_difficulty_anchor(
    block: &Block,
    source: &impl ContentSource,
) -> Result<Option<DifficultyAnchor>, lattice_cashew::Error> {
    let mut current = block.clone();
    if current.height == 0 {
        return Ok(None);
    }
    while current.height > 1 {
        let Some(parent) = &current.parent else {
            return Ok(None);
        };
        let resolved = parent.0.resolve(source, None)?;
        let next = resolved
            .node
            .as_deref()
            .ok_or_else(|| lattice_cashew::Error::MissingContent(resolved.raw_cid.clone()))?;
        if next.height != current.height - 1 || next.spec.0.raw_cid != current.spec.0.raw_cid {
            return Ok(None);
        }
        current = next.clone();
    }
    Ok(Some(DifficultyAnchor {
        block_height: 1,
        timestamp: current.timestamp,
        target: current.target,
    }))
}

pub fn build_transactions_dictionary(
    transactions: &[Transaction],
) -> Result<Header<MerkleDictionary<Volume<Transaction>>>, lattice_cashew::Error> {
    let mut dictionary = MerkleDictionary::default();
    for (index, transaction) in transactions.iter().enumerate() {
        dictionary = dictionary.inserting(&index.to_string(), Volume::new(transaction.clone())?)?;
    }
    Header::new(dictionary)
}
fn bodies(
    transactions: &[Transaction],
    source: &impl ContentSource,
) -> Result<Vec<TransactionBody>, BlockBuilderError> {
    transactions
        .iter()
        .map(|tx| {
            let header = tx.body.resolve(source, None)?;
            let body = header
                .node
                .as_deref()
                .ok_or(BlockBuilderError::InvalidTransactionContent)?;
            if !body.state_atoms_are_valid() {
                return Err(BlockBuilderError::InvalidTransactionContent);
            }
            Ok(body.clone())
        })
        .collect()
}
fn committed_children(
    bodies: &[TransactionBody],
    children: &BTreeMap<String, Block>,
    entering: &LatticeStateHeader,
) -> Result<BTreeMap<String, BlockHeader>, BlockBuilderError> {
    let mut headers: BTreeMap<String, BlockHeader> = BTreeMap::new();
    for action in bodies.iter().flat_map(|b| &b.actions) {
        let (Some(directory), Some(value)) = (
            action.key.strip_prefix(CHILD_COMMITMENT_KEY_PREFIX),
            &action.new_value,
        ) else {
            continue;
        };
        let parts: Vec<_> = value.split('@').collect();
        if !child_directory(directory)
            || parts.len() != 2
            || parts[1] != entering.0.raw_cid
            || !standard_cid(parts[0])
        {
            return Err(BlockBuilderError::ChildCommitment(directory.into()));
        }
        let carried = children
            .get(directory)
            .map(content_id)
            .transpose()?
            .or_else(|| headers.get(directory).map(|h| h.0.raw_cid.clone()));
        if let Some(child) = carried {
            if child != parts[0] {
                return Err(BlockBuilderError::ChildCommitment(directory.into()));
            }
        } else {
            headers.insert(directory.into(), Volume(Header::from_cid(parts[0])?));
        }
    }
    Ok(headers)
}
fn build_children(
    children: &BTreeMap<String, Block>,
    mut committed: BTreeMap<String, BlockHeader>,
) -> Result<Header<BTreeMap<String, BlockHeader>>, BlockBuilderError> {
    for (directory, child) in children {
        committed.insert(directory.clone(), Volume::new(child.clone())?);
    }
    if let Some(invalid) = committed.keys().find(|key| !child_directory(key)) {
        return Err(BlockBuilderError::ChildCommitment(invalid.clone()));
    }
    Ok(Header::new(committed)?)
}
fn compute_state(
    previous: &LatticeStateHeader,
    bodies: &[TransactionBody],
    coinbase: Option<&AccountAction>,
    max_growth: i64,
    source: &impl ContentSource,
) -> Result<MaterializedTransition, BlockBuilderError> {
    if bodies.is_empty() && coinbase.is_none() {
        return Ok(MaterializedTransition {
            state: previous.clone(),
            account_growth: 0,
            diff: StateDiff::default(),
        });
    }
    let actions = TransitionActions::from_bodies(bodies);
    let result = AuthenticatedBlockExecution {
        previous,
        parent: previous,
        directory: "",
        actions: &actions,
        transaction_bodies: bodies,
        reward: 0,
        recipient: None,
    }
    .execute_template(source, coinbase)?;
    if state_growth(bodies, result.account_growth)? > max_growth {
        return Err(BlockBuilderError::StateGrowth);
    }
    Ok(result)
}

pub struct GenesisTemplate<'a> {
    pub spec: &'a ChainSpec,
    pub transactions: &'a [Transaction],
    pub children: &'a BTreeMap<String, Block>,
    /// None selects the root chain; Some commits a child carrier's entering state.
    pub parent_state: Option<&'a LatticeStateHeader>,
    pub timestamp: i64,
    pub target: UInt256,
    pub nonce: u64,
    pub version: u16,
}
impl GenesisTemplate<'_> {
    pub fn build(
        &self,
        source: &impl ContentSource,
    ) -> Result<BlockBuildResult, BlockBuilderError> {
        let previous = Volume::new(LatticeState::empty()?)?;
        let bodies = bodies(self.transactions, source)?;
        let committed = committed_children(&bodies, self.children, &previous)?;
        let execution =
            compute_state(&previous, &bodies, None, self.spec.max_state_growth, source)?;
        let children = build_children(self.children, committed)?;
        let block = Block {
            version: self.version,
            parent: None,
            transactions: build_transactions_dictionary(self.transactions)?,
            target: self.target,
            next_target: self.target,
            spec: Volume::new(self.spec.clone())?,
            parent_state: Volume(self.parent_state.unwrap_or(&previous).0.removing_node()),
            prev_state: Volume(previous.0.removing_node()),
            post_state: execution.state.clone(),
            children,
            height: 0,
            timestamp: self.timestamp,
            reward_recipient: None,
            nonce: self.nonce,
        };
        Ok(BlockBuildResult {
            block,
            state_diff: execution.diff,
            materialized_post_state: execution.state,
        })
    }
}

pub struct BlockTemplate<'a> {
    pub previous: &'a Block,
    pub transactions: &'a [Transaction],
    pub children: &'a BTreeMap<String, Block>,
    pub parent_chain_block: Option<&'a Block>,
    pub timestamp: i64,
    pub target: Option<UInt256>,
    pub next_target: Option<UInt256>,
    pub nonce: u64,
    pub difficulty_anchor: Option<DifficultyAnchor>,
    pub reward_recipient: Option<&'a str>,
}
impl BlockTemplate<'_> {
    pub fn build(
        &self,
        source: &impl ContentSource,
    ) -> Result<BlockBuildResult, BlockBuilderError> {
        let previous = self.previous;
        let height = previous
            .height
            .checked_add(1)
            .ok_or(BlockBuilderError::HeightOverflow)?;
        let spec_header = previous.spec.0.resolve(source, None)?;
        let spec = spec_header
            .node
            .as_deref()
            .ok_or_else(|| lattice_cashew::Error::MissingContent(spec_header.raw_cid.clone()))?;
        let target = self.target.unwrap_or(previous.next_target);
        let next_target = if let Some(next) = self.next_target {
            next
        } else {
            let anchor = if height == 1 {
                Some(DifficultyAnchor {
                    block_height: 1,
                    timestamp: self.timestamp,
                    target,
                })
            } else if self.difficulty_anchor.is_some() {
                self.difficulty_anchor
            } else {
                resolve_difficulty_anchor(previous, source)?
            };
            let anchor = anchor.ok_or(BlockBuilderError::MissingDifficultyAnchor)?;
            spec.asert_target(
                anchor.target,
                anchor.timestamp,
                anchor.block_height,
                self.timestamp,
                height,
            )
        };
        let bodies = bodies(self.transactions, source)?;
        let committed = committed_children(&bodies, self.children, &previous.post_state)?;
        let actions = TransitionActions::from_bodies(&bodies);
        let coinbase = coinbase_credit(
            &actions,
            spec.reward_at_block(height),
            self.reward_recipient,
        )?;
        let execution = compute_state(
            &previous.post_state,
            &bodies,
            coinbase.as_ref(),
            spec.max_state_growth,
            source,
        )?;
        let children = build_children(self.children, committed)?;
        let block = Block {
            version: previous.version,
            parent: Some(Volume(Volume::new(previous.clone())?.0.removing_node())),
            transactions: build_transactions_dictionary(self.transactions)?,
            target,
            next_target,
            spec: previous.spec.clone(),
            parent_state: self
                .parent_chain_block
                .map(|b| Volume(b.prev_state.0.removing_node()))
                .unwrap_or_else(|| previous.parent_state.clone()),
            prev_state: Volume(previous.post_state.0.removing_node()),
            post_state: execution.state.clone(),
            children,
            height,
            timestamp: self.timestamp,
            reward_recipient: self.reward_recipient.map(str::to_owned),
            nonce: self.nonce,
        };
        Ok(BlockBuildResult {
            block,
            state_diff: execution.diff,
            materialized_post_state: execution.state,
        })
    }
}

/// Reference CPU nonce search. Templates remain subject to full admission.
pub fn mine_block(block: &Block, target: UInt256, max_attempts: u64) -> Option<Block> {
    let prefix = block.pow_prefix();
    for nonce in 0..max_attempts {
        if lattice_pow::hash(&prefix, nonce) <= target.0 {
            let mut mined = block.clone();
            mined.nonce = nonce;
            return Some(mined);
        }
    }
    None
}
