//! Composed validation capabilities. Successful values have no public constructor
//! or deserializer; immutable block bytes bind every result they expose.
use crate::*;
use lattice_cashew::ContentSource;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum BlockValidationFailure {
    #[error("invalid block: {0}")]
    Invalid(&'static str),
    #[error("block timestamp is later than the supplied clock")]
    NotYetValid,
    #[error("inherited difficulty anchor is unavailable")]
    AnchorUnavailable,
    #[error("content source unavailable: {0}")]
    Unavailable(lattice_cashew::Error),
    #[error(transparent)]
    Content(#[from] lattice_cashew::Error),
    #[error(transparent)]
    Size(#[from] BlockContentError),
    #[error(transparent)]
    Execution(#[from] ExecutionError),
    #[error(transparent)]
    Growth(#[from] TransitionError),
    #[error(transparent)]
    Policy(#[from] PolicyBatchError),
}
impl BlockValidationFailure {
    pub fn is_retriable(&self) -> bool {
        match self {
            Self::Unavailable(_) | Self::NotYetValid | Self::AnchorUnavailable => true,
            Self::Policy(error) => error.verdict() == PolicyFailureVerdict::Unavailable,
            _ => false,
        }
    }
}

/// Proof of complete root-genesis validation at a supplied wall-clock instant.
/// This does not prove child-chain continuity or validate non-genesis blocks.
#[derive(Debug)]
pub struct VerifiedRootGenesis {
    header: BlockHeader,
    post_state: LatticeStateHeader,
    spec: lattice_cashew::Volume<ChainSpec>,
    state_diff: StateDiff,
}
impl VerifiedRootGenesis {
    pub fn header(&self) -> &BlockHeader {
        &self.header
    }
    pub fn block_cid(&self) -> &str {
        &self.header.0.raw_cid
    }
    pub fn post_state(&self) -> &LatticeStateHeader {
        &self.post_state
    }
    pub fn spec(&self) -> &lattice_cashew::Volume<ChainSpec> {
        &self.spec
    }
    pub fn state_diff(&self) -> &StateDiff {
        &self.state_diff
    }
}

// A provider can return any Cashew error. Record its origin so a provider's
// "noncanonical"/"limit" error never becomes a permanent block verdict.
struct RecordingSource<'a, S> {
    source: &'a S,
    failure: Mutex<Option<lattice_cashew::Error>>,
}
impl<S: ContentSource> ContentSource for RecordingSource<'_, S> {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
        self.source.fetch(cid).inspect_err(|error| {
            let mut failure = self.failure.lock().expect("source failure lock poisoned");
            if failure.is_none() {
                *failure = Some(error.clone());
            }
        })
    }
}

pub fn validate_root_genesis(
    header: &BlockHeader,
    source: &impl ContentSource,
    now_milliseconds: i64,
) -> Result<VerifiedRootGenesis, BlockValidationFailure> {
    let recording = RecordingSource {
        source,
        failure: Mutex::new(None),
    };
    let result = genesis_transition(header, &recording, now_milliseconds, &["Nexus".into()]);
    if let Some(error) = recording
        .failure
        .into_inner()
        .expect("source failure lock poisoned")
    {
        return Err(BlockValidationFailure::Unavailable(error));
    }
    result
}

fn genesis_transition(
    header: &BlockHeader,
    source: &impl ContentSource,
    now: i64,
    path: &[String],
) -> Result<VerifiedRootGenesis, BlockValidationFailure> {
    use BlockValidationFailure::Invalid;
    if header.0.encryption_info.is_some() || !standard_cid(&header.0.raw_cid) {
        return Err(Invalid("root header link"));
    }
    let resolved = header.0.resolve(source, None)?;
    let block = resolved.node.as_deref().ok_or(Invalid("missing block"))?;
    if !block.has_standard_links() || !has_genesis_shape(block, path.len() == 1)? {
        return Err(Invalid("root genesis shape"));
    }
    if path.len() == 1 && !block.has_valid_pow() {
        return Err(Invalid("proof of work"));
    }
    if block.timestamp > now {
        return Err(BlockValidationFailure::NotYetValid);
    }
    // Resolve and check the spec before any unbounded transaction fetch.
    let spec_header = block.spec.0.resolve(source, None)?;
    let spec = spec_header.node.as_deref().ok_or(Invalid("missing spec"))?;
    if !spec.is_valid() {
        return Err(Invalid("chain spec"));
    }
    let mut content = resolve_transaction_content(block, source, spec.max_block_size)?;
    let mut bodies = Vec::with_capacity(content.transactions.len());
    for transaction in &content.transactions {
        bodies.push(
            validate_transaction(transaction, source, true)?
                .ok_or(Invalid("genesis transaction"))?,
        );
    }
    if path.first().map(String::as_str) != Some("Nexus") {
        return Err(Invalid("chain path"));
    }
    if !valid_chain_paths(&bodies, path) {
        return Err(Invalid("transaction chain path"));
    }
    if bodies.iter().any(|body| {
        body.actions.iter().any(|action| {
            action.new_value.is_some() && action.key.starts_with(CHILD_COMMITMENT_KEY_PREFIX)
        })
    }) {
        content.block.children = content.block.children.resolve(source, None)?;
    }
    if !valid_child_commitments(&content.block, &bodies)? {
        return Err(Invalid("child commitment"));
    }
    if !validate_configured_policy_modules(spec, source)? {
        return Err(Invalid("configured policy"));
    }
    if !(PolicyBatch {
        bodies: &bodies,
        spec,
        chain_path: path,
        height: block.height,
        timestamp: block.timestamp,
        scopes: None,
    })
    .verify(source)?
    {
        return Err(Invalid("policy rejection"));
    }
    if bodies.len() as u64 > spec.max_number_of_transactions_per_block {
        return Err(Invalid("transaction count"));
    }
    if state_growth_floor(&bodies)? > spec.max_state_growth {
        return Err(Invalid("growth floor"));
    }
    let mut whole = resolve_block_content(&content.block, source, spec.max_block_size)?.block;
    let actions = TransitionActions::from_bodies(&bodies);
    if !valid_genesis_credits(spec, &actions.accounts) {
        return Err(Invalid("premine"));
    }
    let execution = AuthenticatedBlockExecution {
        previous: &block.prev_state,
        parent: &block.parent_state,
        directory: path.last().expect("validated nonempty path"),
        actions: &actions,
        transaction_bodies: &bodies,
        reward: 0,
        recipient: None,
    }
    .execute_genesis(source, spec)?;
    if state_growth(&bodies, execution.account_growth)? > spec.max_state_growth {
        return Err(Invalid("exact state growth"));
    }
    if execution.state.0.raw_cid != block.post_state.0.raw_cid {
        return Err(Invalid("post-state commitment"));
    }
    if !whole.children_are_well_formed()? {
        return Err(Invalid("child index"));
    }
    whole.spec = lattice_cashew::Volume(spec_header.clone());
    whole.post_state = execution.state.clone();
    whole.prev_state = lattice_cashew::Volume::new(LatticeState::empty()?)?;
    if path.len() == 1 {
        whole.parent_state = whole.prev_state.clone();
    }
    let materialized = lattice_cashew::Volume::new(whole)?;
    if materialized.0.raw_cid != resolved.raw_cid {
        return Err(Invalid("materialization changed block commitment"));
    }
    Ok(VerifiedRootGenesis {
        header: materialized,
        post_state: execution.state,
        spec: lattice_cashew::Volume(spec_header),
        state_diff: execution.diff,
    })
}

/// Authenticated root-block execution relative to its immediate predecessor and
/// the supplied graph anchor. A graph must check its held/executed predecessor
/// and inherited anchor against these bindings before admitting the result.
#[derive(Debug)]
pub struct VerifiedRootTransition {
    header: BlockHeader,
    parent_cid: String,
    inherited_anchor: Option<DifficultyAnchor>,
    resolved_anchor: DifficultyAnchor,
    post_state: LatticeStateHeader,
    spec: lattice_cashew::Volume<ChainSpec>,
    state_diff: StateDiff,
}
impl VerifiedRootTransition {
    pub fn header(&self) -> &BlockHeader {
        &self.header
    }
    pub fn block_cid(&self) -> &str {
        &self.header.0.raw_cid
    }
    pub fn parent_cid(&self) -> &str {
        &self.parent_cid
    }
    pub fn inherited_anchor(&self) -> Option<DifficultyAnchor> {
        self.inherited_anchor
    }
    pub fn resolved_anchor(&self) -> DifficultyAnchor {
        self.resolved_anchor
    }
    pub fn post_state(&self) -> &LatticeStateHeader {
        &self.post_state
    }
    pub fn spec(&self) -> &lattice_cashew::Volume<ChainSpec> {
        &self.spec
    }
    pub fn state_diff(&self) -> &StateDiff {
        &self.state_diff
    }
}

/// Validate an already-weighed root block's execution. This does not turn an
/// arbitrary supplied anchor or predecessor into trusted graph state.
pub fn validate_root_transition(
    header: &BlockHeader,
    source: &impl ContentSource,
    now_milliseconds: i64,
    inherited_anchor: Option<DifficultyAnchor>,
) -> Result<VerifiedRootTransition, BlockValidationFailure> {
    let recording = RecordingSource {
        source,
        failure: Mutex::new(None),
    };
    let result = block_transition(
        header,
        &recording,
        now_milliseconds,
        inherited_anchor,
        &["Nexus".into()],
    );
    if let Some(error) = recording
        .failure
        .into_inner()
        .expect("source failure lock poisoned")
    {
        return Err(BlockValidationFailure::Unavailable(error));
    }
    result
}

fn block_transition(
    header: &BlockHeader,
    source: &impl ContentSource,
    now: i64,
    inherited_anchor: Option<DifficultyAnchor>,
    path: &[String],
) -> Result<VerifiedRootTransition, BlockValidationFailure> {
    use BlockValidationFailure::Invalid;
    if header.0.encryption_info.is_some() || !standard_cid(&header.0.raw_cid) {
        return Err(Invalid("root header link"));
    }
    let resolved = header.0.resolve(source, None)?;
    let block = resolved.node.as_deref().ok_or(Invalid("missing block"))?;
    if !block.has_standard_links()
        || block.version != Block::CURRENT_VERSION
        || !block.has_well_formed_reward_recipient()
    {
        return Err(Invalid("block structure"));
    }
    if path.len() == 1 && !block.has_valid_pow() {
        return Err(Invalid("proof of work"));
    }
    let parent_link = block
        .parent
        .as_ref()
        .ok_or(Invalid("missing predecessor"))?;
    let parent = parent_link.0.resolve(source, None)?;
    let parent_node = parent
        .node
        .as_deref()
        .ok_or(Invalid("missing predecessor bytes"))?;
    if block.spec.0.raw_cid != parent_node.spec.0.raw_cid
        || block.prev_state.0.raw_cid != parent_node.post_state.0.raw_cid
        || parent_node.height.checked_add(1) != Some(block.height)
    {
        return Err(Invalid("predecessor linkage"));
    }
    let spec_header = block.spec.0.resolve(source, None)?;
    let spec = spec_header.node.as_deref().ok_or(Invalid("missing spec"))?;
    if !spec.is_valid() {
        return Err(Invalid("chain spec"));
    }
    if block.timestamp > now {
        return Err(BlockValidationFailure::NotYetValid);
    }
    if block.timestamp <= parent_node.timestamp {
        return Err(Invalid("timestamp order"));
    }
    let linkage = HeaderLinkageParent::from(parent_node);
    let anchor = immediate_difficulty_anchor(block, &linkage)
        .or(inherited_anchor)
        .ok_or(BlockValidationFailure::AnchorUnavailable)?;
    if !valid_next_target(block, &linkage, spec, anchor) {
        return Err(Invalid("difficulty schedule"));
    }
    let content = resolve_transaction_content(block, source, spec.max_block_size)?;
    let mut bodies = Vec::with_capacity(content.transactions.len());
    for tx in &content.transactions {
        bodies.push(validate_transaction(tx, source, false)?.ok_or(Invalid("transaction"))?);
    }
    if bodies.len() as u64 > spec.max_number_of_transactions_per_block {
        return Err(Invalid("transaction count"));
    }
    if state_growth_floor(&bodies)? > spec.max_state_growth {
        return Err(Invalid("growth floor"));
    }
    let mut whole = resolve_block_content(&content.block, source, spec.max_block_size)?.block;
    if path.first().map(String::as_str) != Some("Nexus") {
        return Err(Invalid("chain path"));
    }
    if !valid_chain_paths(&bodies, path) {
        return Err(Invalid("transaction chain path"));
    }
    if !valid_root_actions(&bodies, path) {
        return Err(Invalid("root swap actions"));
    }
    if !valid_child_commitments(&whole, &bodies)? {
        return Err(Invalid("child commitment"));
    }
    if !(PolicyBatch {
        bodies: &bodies,
        spec,
        chain_path: path,
        height: block.height,
        timestamp: block.timestamp,
        scopes: None,
    })
    .verify(source)?
    {
        return Err(Invalid("policy rejection"));
    }
    let actions = TransitionActions::from_bodies(&bodies);
    validate_authenticated_withdrawals(
        &block.prev_state,
        &block.parent_state,
        path.last().expect("validated nonempty path"),
        &actions.withdrawals,
        source,
    )?;
    // Swift decides fee validity and coinbase overflow before resolving state.
    coinbase_credit(
        &actions,
        spec.reward_at_block(block.height),
        block.reward_recipient.as_deref(),
    )?;
    let execution = AuthenticatedBlockExecution {
        previous: &block.prev_state,
        parent: &block.parent_state,
        directory: path.last().expect("validated nonempty path"),
        actions: &actions,
        transaction_bodies: &bodies,
        reward: spec.reward_at_block(block.height),
        recipient: block.reward_recipient.as_deref(),
    }
    .execute(source)?;
    if state_growth(&bodies, execution.account_growth)? > spec.max_state_growth {
        return Err(Invalid("exact state growth"));
    }
    if execution.state.0.raw_cid != block.post_state.0.raw_cid {
        return Err(Invalid("post-state commitment"));
    }
    if !whole.children_are_well_formed()? {
        return Err(Invalid("child index"));
    }
    whole.spec = lattice_cashew::Volume(spec_header.clone());
    whole.post_state = execution.state.clone();
    whole.prev_state = lattice_cashew::Volume(block.prev_state.0.resolve(source, None)?);
    // Predecessor content is already durable; don't recursively copy its history.
    whole.parent = Some(lattice_cashew::Volume(parent_link.0.removing_node()));
    let materialized = lattice_cashew::Volume::new(whole)?;
    if materialized.0.raw_cid != resolved.raw_cid {
        return Err(Invalid("materialization changed block commitment"));
    }
    Ok(VerifiedRootTransition {
        header: materialized,
        parent_cid: parent_link.0.raw_cid.clone(),
        inherited_anchor,
        resolved_anchor: anchor,
        post_state: execution.state,
        spec: lattice_cashew::Volume(spec_header),
        state_diff: execution.diff,
    })
}

/// Child-genesis execution evidence, deliberately distinct from root admission.
/// The importer must also verify securing work and parent-chain continuity from
/// the empty state to `parent_state_cid` on the bound parent path. Neither the
/// carrier's bytes nor equality with an unexecuted predecessor proves continuity.
#[derive(Debug)]
pub struct VerifiedChildGenesis {
    execution: VerifiedRootGenesis,
    chain_path: Vec<String>,
    parent_state_cid: String,
}
impl VerifiedChildGenesis {
    pub fn header(&self) -> &BlockHeader {
        self.execution.header()
    }
    pub fn block_cid(&self) -> &str {
        self.execution.block_cid()
    }
    pub fn post_state(&self) -> &LatticeStateHeader {
        self.execution.post_state()
    }
    pub fn spec(&self) -> &lattice_cashew::Volume<ChainSpec> {
        self.execution.spec()
    }
    pub fn state_diff(&self) -> &StateDiff {
        self.execution.state_diff()
    }
    pub fn chain_path(&self) -> &[String] {
        &self.chain_path
    }
    pub fn parent_path(&self) -> &[String] {
        &self.chain_path[..self.chain_path.len() - 1]
    }
    pub fn parent_state_cid(&self) -> &str {
        &self.parent_state_cid
    }
}

/// Execute child genesis under an explicit positional path. This cannot mint a
/// root capability and does not claim that the child's own hash secures work.
pub fn validate_child_genesis(
    header: &BlockHeader,
    source: &impl ContentSource,
    now_milliseconds: i64,
    chain_path: &[String],
) -> Result<VerifiedChildGenesis, BlockValidationFailure> {
    if chain_path.len() < 2 || chain_path.first().map(String::as_str) != Some("Nexus") {
        return Err(BlockValidationFailure::Invalid("child chain path"));
    }
    let recording = RecordingSource {
        source,
        failure: Mutex::new(None),
    };
    let result = genesis_transition(header, &recording, now_milliseconds, chain_path);
    if let Some(error) = recording
        .failure
        .into_inner()
        .expect("source failure lock poisoned")
    {
        return Err(BlockValidationFailure::Unavailable(error));
    }
    let execution = result?;
    let parent_state_cid = execution
        .header()
        .0
        .node
        .as_deref()
        .expect("validated materialized block")
        .parent_state
        .0
        .raw_cid
        .clone();
    Ok(VerifiedChildGenesis {
        execution,
        chain_path: chain_path.to_vec(),
        parent_state_cid,
    })
}

/// Child execution conditional on graph-derived securing work, inherited
/// difficulty anchor, executed predecessor, and forward parent-state continuity.
#[derive(Debug)]
pub struct VerifiedChildTransition {
    execution: VerifiedRootTransition,
    chain_path: Vec<String>,
    predecessor_parent_state_cid: String,
    parent_state_cid: String,
}
impl VerifiedChildTransition {
    pub fn header(&self) -> &BlockHeader {
        self.execution.header()
    }
    pub fn block_cid(&self) -> &str {
        self.execution.block_cid()
    }
    pub fn parent_cid(&self) -> &str {
        self.execution.parent_cid()
    }
    pub fn inherited_anchor(&self) -> Option<DifficultyAnchor> {
        self.execution.inherited_anchor()
    }
    pub fn resolved_anchor(&self) -> DifficultyAnchor {
        self.execution.resolved_anchor()
    }
    pub fn post_state(&self) -> &LatticeStateHeader {
        self.execution.post_state()
    }
    pub fn spec(&self) -> &lattice_cashew::Volume<ChainSpec> {
        self.execution.spec()
    }
    pub fn state_diff(&self) -> &StateDiff {
        self.execution.state_diff()
    }
    pub fn chain_path(&self) -> &[String] {
        &self.chain_path
    }
    pub fn parent_path(&self) -> &[String] {
        &self.chain_path[..self.chain_path.len() - 1]
    }
    pub fn predecessor_parent_state_cid(&self) -> &str {
        &self.predecessor_parent_state_cid
    }
    pub fn parent_state_cid(&self) -> &str {
        &self.parent_state_cid
    }
}

pub fn validate_child_transition(
    header: &BlockHeader,
    source: &impl ContentSource,
    now_milliseconds: i64,
    inherited_anchor: Option<DifficultyAnchor>,
    chain_path: &[String],
) -> Result<VerifiedChildTransition, BlockValidationFailure> {
    if chain_path.len() < 2 || chain_path.first().map(String::as_str) != Some("Nexus") {
        return Err(BlockValidationFailure::Invalid("child chain path"));
    }
    let recording = RecordingSource {
        source,
        failure: Mutex::new(None),
    };
    let result = (|| {
        let execution = block_transition(
            header,
            &recording,
            now_milliseconds,
            inherited_anchor,
            chain_path,
        )?;
        let block = execution
            .header()
            .0
            .node
            .as_deref()
            .expect("validated block");
        let parent_state_cid = block.parent_state.0.raw_cid.clone();
        // Resolve from the original header so materialized predecessors do not
        // become artificial missing-content failures after output pruning.
        let original = header.0.resolve(&recording, None)?;
        let predecessor = original
            .node
            .as_deref()
            .expect("validated block")
            .parent
            .as_ref()
            .expect("validated predecessor")
            .0
            .resolve(&recording, None)?;
        let predecessor_parent_state_cid = predecessor
            .node
            .as_deref()
            .expect("resolved predecessor")
            .parent_state
            .0
            .raw_cid
            .clone();
        Ok(VerifiedChildTransition {
            execution,
            chain_path: chain_path.to_vec(),
            predecessor_parent_state_cid,
            parent_state_cid,
        })
    })();
    if let Some(error) = recording
        .failure
        .into_inner()
        .expect("source failure lock poisoned")
    {
        return Err(BlockValidationFailure::Unavailable(error));
    }
    result
}
