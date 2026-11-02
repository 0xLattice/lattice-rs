use crate::euler::{EulerWorkIndex, Event};
use crate::*;
use lattice_pow::{Target, WorkSum};
use lattice_primitives::{ChainRuntimeContext, UInt256};
use num_bigint::BigUint;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RestoreError {
    #[error("corrupt consensus graph")]
    CorruptConsensusGraph,
    #[error("missing prerequisite block fact")]
    MissingBlockFact,
    #[error("block both executed and excluded")]
    ExecutedVerdictContradiction,
    #[error("root genesis differs from configured pin")]
    UnpinnedRootGenesis,
    #[error("non-standard consensus link")]
    NonStandardLink,
    #[error("revision exhausted")]
    RevisionExhausted,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainCommit {
    pub revision: u64,
    pub tip_hash: String,
    pub canonical_blocks_added: BTreeMap<String, u64>,
    pub canonical_blocks_removed: BTreeSet<String>,
}
impl ChainCommit {
    pub fn canonical_changed(&self) -> bool {
        !self.canonical_blocks_added.is_empty() || !self.canonical_blocks_removed.is_empty()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionResult {
    pub added_block: bool,
    pub added_contribution: bool,
    pub extends_canonical: bool,
    pub commit: Option<ChainCommit>,
    pub weighed: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifficultyAnchor {
    pub block_height: u64,
    pub timestamp: i64,
    pub target: Target,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipBlockSnapshot {
    pub post_state_cid: String,
    pub prev_state_cid: String,
    pub parent_state_cid: String,
    pub spec_cid: String,
    pub target: Target,
    pub next_target: Target,
    pub tip_height: u64,
    pub timestamp: i64,
}
#[derive(Debug, Clone)]
pub struct BlockMeta {
    pub block_hash: String,
    pub parent_block_hash: Option<String>,
    pub block_height: u64,
    pub difficulty_anchor: Option<DifficultyAnchor>,
    pub work: WorkSum,
    pub child_hashes: Vec<String>,
    pub work_contributions: BTreeMap<String, VerifiedWorkContribution>,
    pub attributed_runs: BTreeSet<String>,
    pub child_commitments: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkChoiceSnapshot {
    pub starting_hash: String,
    pub subtree_work: WorkSum,
    pub tip_hash: String,
    pub canonical_path: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    fact: ChainBlockFact,
    snapshot: TipBlockSnapshot,
    linked: bool,
    validated: bool,
    excluded: bool,
    handle: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Grind {
    block: String,
    work: BigUint,
    attributed: bool,
}
/// One source of truth for held facts, work locations and execution state.
/// Weights are exact arbitrary-precision values; exclusion affects selection only.
#[derive(Debug, Clone)]
pub struct ChainTree {
    context: Option<ChainRuntimeContext>,
    blocks: BTreeMap<String, Node>,
    children: BTreeMap<String, BTreeSet<String>>,
    grinds: BTreeMap<String, Grind>,
    direct_work: Vec<BigUint>,
    weights: EulerWorkIndex,
    canonical: Vec<String>,
    revision: u64,
    reserved: u64,
    replaying: bool,
    served: BTreeSet<String>,
    specs: BTreeMap<String, lattice_primitives::ChainSpec>,
}
#[derive(Clone)]
struct Trusted {
    block: Option<(ChainBlockFact, TipBlockSnapshot)>,
    work_block: String,
    contribution: VerifiedWorkContribution,
    validates: bool,
}
fn standard(s: &str) -> Option<String> {
    lattice_primitives::canonical_cid(s).filter(|s| lattice_primitives::standard_cid(s))
}
fn exact(value: &BigUint) -> WorkSum {
    WorkSum::from_hex(&value.to_str_radix(16)).unwrap()
}
fn fixed(value: &BigUint) -> Option<Target> {
    exact(value).as_target()
}
fn empty_state() -> String {
    lattice_primitives::content_id(
        &lattice_primitives::LatticeState::empty().expect("empty state serializes"),
    )
    .expect("empty state cid")
}
pub fn fork_choice_prefers_block(candidate: &str, current: &str) -> bool {
    match (candidate.parse::<cid::Cid>(), current.parse::<cid::Cid>()) {
        (Ok(a), Ok(b)) => a.to_bytes() < b.to_bytes(),
        (Ok(_), Err(_)) => true,
        (Err(_), Ok(_)) => false,
        _ => candidate < current,
    }
}
fn normalized(f: &ChainBlockFact) -> Result<(ChainBlockFact, TipBlockSnapshot), RestoreError> {
    let mut f = f.clone();
    f.block_hash = standard(&f.block_hash).ok_or(RestoreError::CorruptConsensusGraph)?;
    f.post_state_cid = standard(&f.post_state_cid).ok_or(RestoreError::NonStandardLink)?;
    f.prev_state_cid = standard(&f.prev_state_cid).ok_or(RestoreError::NonStandardLink)?;
    f.spec_cid = standard(&f.spec_cid).ok_or(RestoreError::NonStandardLink)?;
    let parent_state = standard(f.parent_state_cid.as_deref().unwrap_or(&empty_state()))
        .ok_or(RestoreError::NonStandardLink)?;
    f.parent_state_cid = Some(parent_state.clone());
    f.parent_block_hash = f
        .parent_block_hash
        .as_ref()
        .map(|p| standard(p).ok_or(RestoreError::NonStandardLink))
        .transpose()?;
    if f.child_commitments
        .as_ref()
        .is_some_and(|m| m.values().any(|s| !lattice_primitives::standard_cid(s)))
    {
        return Err(RestoreError::NonStandardLink);
    }
    let target = parse_target(&f.target)
        .filter(|v| *v != [0; 32])
        .ok_or(RestoreError::CorruptConsensusGraph)?;
    let next_target = parse_target(&f.next_target)
        .filter(|v| *v != [0; 32])
        .ok_or(RestoreError::CorruptConsensusGraph)?;
    if f.parent_block_hash.is_none() != (f.block_height == 0) {
        return Err(RestoreError::CorruptConsensusGraph);
    }
    let snap = TipBlockSnapshot {
        post_state_cid: f.post_state_cid.clone(),
        prev_state_cid: f.prev_state_cid.clone(),
        parent_state_cid: parent_state,
        spec_cid: f.spec_cid.clone(),
        target,
        next_target,
        tip_height: f.block_height,
        timestamp: f.timestamp,
    };
    Ok((f, snap))
}
fn trusted(batch: &BlockImportBatch) -> Result<Trusted, RestoreError> {
    let mut ids = std::collections::HashSet::new();
    let mut block = None;
    let mut work = None;
    let mut validation = None;
    for fact in batch.facts() {
        if !ids.insert(fact.id()) {
            return Err(RestoreError::CorruptConsensusGraph);
        }
        match fact {
            ChainFact::Block(f) => {
                if block.is_some() {
                    return Err(RestoreError::CorruptConsensusGraph);
                }
                block = Some(normalized(f)?);
            }
            ChainFact::Work(f) => {
                if work.is_some() {
                    return Err(RestoreError::CorruptConsensusGraph);
                }
                work = Some(f)
            }
            ChainFact::Validation(f) => {
                if validation.is_some() {
                    return Err(RestoreError::CorruptConsensusGraph);
                }
                validation =
                    Some(standard(&f.block_hash).ok_or(RestoreError::CorruptConsensusGraph)?);
            }
            _ => return Err(RestoreError::CorruptConsensusGraph),
        }
    }
    let w = work.ok_or(RestoreError::CorruptConsensusGraph)?;
    let work_block = standard(&w.block_hash).ok_or(RestoreError::CorruptConsensusGraph)?;
    let id = standard(w.contribution.id()).ok_or(RestoreError::CorruptConsensusGraph)?;
    if w.contribution.work() == &[0; 32]
        || block
            .as_ref()
            .is_some_and(|(b, _)| b.block_hash != work_block)
        || validation.as_ref().is_some_and(|v| *v != work_block)
    {
        return Err(RestoreError::CorruptConsensusGraph);
    }
    Ok(Trusted {
        block,
        work_block,
        contribution: VerifiedWorkContribution::from_verified_import(
            id,
            UInt256(*w.contribution.work()),
        ),
        validates: validation.is_some(),
    })
}
impl ChainTree {
    pub fn empty(context: ChainRuntimeContext) -> Self {
        Self::new(Some(context))
    }
    fn new(context: Option<ChainRuntimeContext>) -> Self {
        Self {
            context,
            blocks: BTreeMap::new(),
            children: BTreeMap::new(),
            grinds: BTreeMap::new(),
            direct_work: vec![],
            weights: EulerWorkIndex::default(),
            canonical: vec![],
            revision: 0,
            reserved: 0,
            replaying: false,
            served: BTreeSet::new(),
            specs: BTreeMap::new(),
        }
    }
    pub fn context(&self) -> Option<&ChainRuntimeContext> {
        self.context.as_ref()
    }
    pub fn current_revision(&self) -> u64 {
        self.revision
    }
    pub fn canonical_tip(&self) -> &str {
        self.canonical.last().map(String::as_str).unwrap_or("")
    }
    pub fn canonical_path(&self) -> &[String] {
        &self.canonical
    }
    pub fn contains(&self, hash: &str) -> bool {
        self.blocks.contains_key(hash)
    }
    pub fn parent(&self, hash: &str) -> Option<&str> {
        self.blocks.get(hash)?.fact.parent_block_hash.as_deref()
    }
    pub fn height(&self, hash: &str) -> Option<u64> {
        self.blocks.get(hash).map(|n| n.fact.block_height)
    }
    pub fn children(&self, hash: &str) -> Vec<String> {
        self.children
            .get(hash)
            .into_iter()
            .flatten()
            .filter(|c| self.blocks.get(*c).is_some_and(|n| n.linked))
            .cloned()
            .collect()
    }
    pub fn genesis_roots(&self) -> Vec<String> {
        self.blocks
            .iter()
            .filter(|(_, n)| n.fact.parent_block_hash.is_none())
            .map(|(h, _)| h.clone())
            .collect()
    }
    pub fn is_executed(&self, hash: &str) -> bool {
        self.blocks.get(hash).is_some_and(|n| n.validated)
    }
    pub fn is_excluded_root(&self, hash: &str) -> bool {
        self.blocks.get(hash).is_some_and(|n| n.excluded)
    }
    pub fn header_snapshot(&self, hash: &str) -> Option<&TipBlockSnapshot> {
        self.blocks.get(hash).map(|n| &n.snapshot)
    }
    pub fn highest_block_height(&self) -> u64 {
        self.blocks
            .values()
            .map(|n| n.fact.block_height)
            .max()
            .unwrap_or(0)
    }
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
    fn capacity(&self) -> bool {
        self.revision
            .checked_add(self.reserved)
            .is_some_and(|v| v < u64::MAX)
    }
    pub fn reserve_import_revision(&mut self) -> bool {
        if !self.capacity() {
            return false;
        }
        self.reserved += 1;
        true
    }
    pub fn release_import_revision(&mut self) {
        assert!(self.reserved > 0);
        self.reserved -= 1;
    }
    fn routed(&self, hash: &str) -> bool {
        self.blocks
            .get(hash)
            .is_some_and(|n| self.weights.contains(n.handle))
    }
    /// Tour only the newly connected component; never walk unrelated history.
    fn tour(&self, root: &str, unrouted_only: bool) -> Vec<Event> {
        let mut events = vec![];
        let mut stack = vec![(root.to_owned(), false)];
        let mut seen = BTreeSet::new();
        while let Some((hash, close)) = stack.pop() {
            let handle = self.blocks[&hash].handle;
            if close {
                events.push(Event::Close(handle));
                continue;
            }
            if (unrouted_only && self.weights.contains(handle)) || !seen.insert(hash.clone()) {
                continue;
            }
            events.push(Event::Open(handle));
            stack.push((hash.clone(), true));
            stack.extend(self.children(&hash).into_iter().rev().map(|c| (c, false)));
        }
        events
    }
    fn route_component(&mut self, hash: &str) {
        let node = &self.blocks[hash];
        let handle = node.handle;
        if self.weights.contains(handle) {
            self.weights.changed(handle, &self.direct_work);
            return;
        }
        let parent = match node.fact.parent_block_hash.as_deref() {
            Some(p) if node.linked && self.routed(p) => Some(self.blocks[p].handle),
            None if node.fact.block_height == 0 => None,
            _ => return,
        };
        if self.children(hash).is_empty() {
            if let Some(parent) = parent {
                assert!(
                    self.weights
                        .insert_leaf(handle, parent, &self.direct_work)
                        .is_some()
                );
            } else {
                assert!(self.weights.insert_root(handle, &self.direct_work));
            }
            return;
        }
        let events = self.tour(hash, true);
        if let Some(parent) = parent {
            assert!(
                self.weights
                    .splice(&events, parent, &self.direct_work)
                    .is_some()
            );
        } else {
            assert!(self.weights.insert_root(handle, &self.direct_work));
            assert!(
                self.weights
                    .splice(&events[1..events.len() - 1], handle, &self.direct_work)
                    .is_some()
            );
        }
    }
    fn rebuild_weights(&mut self) {
        let events: Vec<_> = self
            .genesis_roots()
            .into_iter()
            .flat_map(|h| self.tour(&h, false))
            .collect();
        self.weights = EulerWorkIndex::build(&events, &self.direct_work);
    }
    fn subtree(&self, root: &str) -> Vec<String> {
        let mut result = vec![];
        let mut stack = vec![root.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(h) = stack.pop() {
            if !seen.insert(h.clone()) {
                continue;
            }
            stack.extend(self.children(&h));
            result.push(h);
        }
        result
    }
    fn weight(&self, root: &str) -> Option<BigUint> {
        self.weights
            .weight(self.blocks.get(root)?.handle, &self.direct_work)
    }
    fn preferred(&self, candidates: impl IntoIterator<Item = String>) -> Option<String> {
        let mut best: Option<(String, BigUint)> = None;
        for h in candidates {
            let Some(w) = self.weight(&h) else { continue };
            if best
                .as_ref()
                .is_none_or(|(b, bw)| w > *bw || (w == *bw && fork_choice_prefers_block(&h, b)))
            {
                best = Some((h, w));
            }
        }
        best.map(|v| v.0)
    }
    fn descent(&self, start: &str) -> Vec<String> {
        let mut path = vec![start.to_string()];
        if self.is_excluded_root(start) {
            return path;
        }
        loop {
            let children: Vec<_> = self
                .children(path.last().unwrap())
                .into_iter()
                .filter(|h| !self.is_excluded_root(h))
                .collect();
            let next = if children.len() == 1 {
                children.into_iter().next()
            } else {
                self.preferred(children)
            };
            let Some(next) = next else {
                break;
            };
            // Routed graph edges form a tree. A cycle is an internal invariant
            // failure, not a reason to silently choose a partial canonical path.
            assert!(path.len() < self.blocks.len(), "cycle in routed graph");
            path.push(next);
        }
        path
    }
    fn project(&mut self) -> ChainCommit {
        let previous: BTreeSet<_> = self.canonical.iter().cloned().collect();
        self.canonical = self
            .preferred(
                self.genesis_roots()
                    .into_iter()
                    .filter(|h| !self.is_excluded_root(h)),
            )
            .map(|h| self.descent(&h))
            .unwrap_or_default();
        let next: BTreeSet<_> = self.canonical.iter().cloned().collect();
        ChainCommit {
            revision: self.revision,
            tip_hash: self.canonical_tip().into(),
            canonical_blocks_added: next
                .difference(&previous)
                .map(|h| (h.clone(), self.blocks[h].fact.block_height))
                .collect(),
            canonical_blocks_removed: previous.difference(&next).cloned().collect(),
        }
    }
    fn after_mutation(
        &mut self,
        added_block: bool,
        added_contribution: bool,
        hash: &str,
        old_tip: &str,
        weighed: Vec<String>,
    ) -> SubmissionResult {
        self.revision += 1;
        let commit = if self.replaying {
            ChainCommit {
                revision: self.revision,
                tip_hash: self.canonical_tip().into(),
                canonical_blocks_added: BTreeMap::new(),
                canonical_blocks_removed: BTreeSet::new(),
            }
        } else {
            self.project()
        };
        SubmissionResult {
            added_block,
            added_contribution,
            extends_canonical: added_block
                && !old_tip.is_empty()
                && self.parent(hash) == Some(old_tip)
                && self.canonical.iter().any(|h| h == hash),
            commit: Some(commit),
            weighed,
        }
    }
    /// Replay only facts loaded from a locally authenticated durable journal.
    /// This does not verify PoW, signatures, or content and is never a network admission API.
    pub fn replay(
        &mut self,
        batch: &BlockImportBatch,
    ) -> Result<Option<ChainCommit>, RestoreError> {
        Ok(self.apply_staged(batch)?.and_then(|r| r.commit))
    }
    pub(crate) fn apply_staged(
        &mut self,
        batch: &BlockImportBatch,
    ) -> Result<Option<SubmissionResult>, RestoreError> {
        if let [ChainFact::Exclusion(f)] = batch.facts() {
            let hash = standard(&f.block_hash).ok_or(RestoreError::CorruptConsensusGraph)?;
            let n = self
                .blocks
                .get(&hash)
                .ok_or(RestoreError::MissingBlockFact)?;
            if n.excluded {
                return Ok(None);
            }
            if n.validated {
                return Err(RestoreError::ExecutedVerdictContradiction);
            }
            if n.fact.parent_block_hash.is_none()
                && !self
                    .genesis_roots()
                    .iter()
                    .any(|r| r != &hash && self.is_executed(r) && !self.is_excluded_root(r))
            {
                return Err(if self.replaying {
                    RestoreError::MissingBlockFact
                } else {
                    RestoreError::CorruptConsensusGraph
                });
            }
            if !self.capacity() {
                return Err(RestoreError::RevisionExhausted);
            }
            self.blocks.get_mut(&hash).unwrap().excluded = true;
            return Ok(Some(self.after_mutation(false, false, &hash, "", vec![])));
        }
        for f in batch.facts() {
            if let ChainFact::Validation(v) = f
                && standard(&v.block_hash).is_some_and(|h| self.is_excluded_root(&h))
            {
                return Err(RestoreError::ExecutedVerdictContradiction);
            }
        }
        if let [ChainFact::Validation(f)] = batch.facts() {
            let hash = standard(&f.block_hash).ok_or(RestoreError::CorruptConsensusGraph)?;
            self.blocks
                .get_mut(&hash)
                .ok_or(RestoreError::MissingBlockFact)?
                .validated = true;
            return Ok(None);
        }
        let trusted = trusted(batch)?;
        let hash = trusted.work_block.clone();
        let id = trusted.contribution.id().to_string();
        let work = BigUint::from_bytes_be(trusted.contribution.work());
        if self.grinds.get(&id).is_some_and(|g| g.block != hash) {
            return Err(RestoreError::CorruptConsensusGraph);
        }
        let old_tip = self.canonical_tip().to_string();
        let mut added = false;
        if let Some((fact, snapshot)) = &trusted.block {
            if fact.parent_block_hash.is_none()
                && self
                    .context
                    .as_ref()
                    .is_some_and(|c| !c.admits_genesis(&hash))
            {
                return Err(RestoreError::UnpinnedRootGenesis);
            }
            if let Some(old) = self.blocks.get(&hash) {
                if old.fact.parent_block_hash != fact.parent_block_hash
                    || old.fact.block_height != fact.block_height
                    || old.snapshot != *snapshot
                {
                    return Err(RestoreError::CorruptConsensusGraph);
                }
            } else {
                if !self.capacity() {
                    return Err(RestoreError::RevisionExhausted);
                }
                let linked = fact
                    .parent_block_hash
                    .as_ref()
                    .is_none_or(|p| self.blocks.contains_key(p));
                let handle = self.direct_work.len();
                self.direct_work.push(BigUint::default());
                self.blocks.insert(
                    hash.clone(),
                    Node {
                        fact: fact.clone(),
                        snapshot: snapshot.clone(),
                        linked,
                        validated: false,
                        excluded: false,
                        handle,
                    },
                );
                if let Some(parent) = &fact.parent_block_hash {
                    self.children
                        .entry(parent.clone())
                        .or_default()
                        .insert(hash.clone());
                }
                if let Some(children) = self.children.get(&hash) {
                    for c in children {
                        let node = self.blocks.get_mut(c).unwrap();
                        node.linked =
                            fact.block_height.checked_add(1) == Some(node.fact.block_height);
                    }
                }
                added = true;
            }
        }
        if !self.blocks.contains_key(&hash) {
            return Err(RestoreError::MissingBlockFact);
        }
        if self.grinds.get(&id).is_some_and(|g| g.work >= work) {
            if trusted.validates {
                self.blocks.get_mut(&hash).unwrap().validated = true;
            }
            return Ok(None);
        }
        if !self.capacity() {
            return Err(RestoreError::RevisionExhausted);
        }
        let old = self
            .grinds
            .get(&id)
            .map(|g| g.work.clone())
            .unwrap_or_default();
        self.direct_work[self.blocks[&hash].handle] += &work - &old;
        if !self.replaying {
            self.route_component(&hash);
        }
        self.grinds.insert(
            id,
            Grind {
                block: hash.clone(),
                work,
                attributed: false,
            },
        );
        if trusted.validates {
            self.blocks.get_mut(&hash).unwrap().validated = true;
        }
        let weighed = if added && self.routed(&hash) {
            self.subtree(&hash)
        } else {
            vec![hash.clone()]
        };
        Ok(Some(
            self.after_mutation(added, true, &hash, &old_tip, weighed),
        ))
    }
    pub fn restore(
        batches: &[BlockImportBatch],
        revision_floor: u64,
        context: ChainRuntimeContext,
    ) -> Result<Self, RestoreError> {
        Self::restore_inner(batches, revision_floor, Some(context))
    }
    fn restore_inner(
        batches: &[BlockImportBatch],
        revision_floor: u64,
        context: Option<ChainRuntimeContext>,
    ) -> Result<Self, RestoreError> {
        if !batches.iter().any(|b| {
            trusted(b)
                .ok()
                .and_then(|t| t.block)
                .is_some_and(|(b, _)| b.parent_block_hash.is_none())
        }) {
            return Err(RestoreError::CorruptConsensusGraph);
        }
        let mut tree = Self::new(context);
        tree.replaying = true;
        let mut pending: Vec<_> = batches.iter().collect();
        pending.sort_by_key(|b| replay_key(b));
        while !pending.is_empty() {
            let mut deferred = vec![];
            let mut completed = false;
            for batch in pending {
                match tree.apply_staged(batch) {
                    Ok(_) => completed = true,
                    Err(RestoreError::MissingBlockFact) => deferred.push(batch),
                    Err(RestoreError::NonStandardLink) => {
                        return Err(RestoreError::CorruptConsensusGraph);
                    }
                    Err(e) => return Err(e),
                }
            }
            if !completed {
                return Err(RestoreError::CorruptConsensusGraph);
            }
            pending = deferred;
        }
        tree.replaying = false;
        tree.rebuild_weights();
        tree.project();
        tree.revision = tree.revision.max(revision_floor);
        Ok(tree)
    }
    pub fn fork_choice_snapshot(&self, hash: &str) -> Option<ForkChoiceSnapshot> {
        Some(ForkChoiceSnapshot {
            starting_hash: hash.into(),
            subtree_work: exact(&self.weight(hash)?),
            tip_hash: self.descent(hash).last()?.clone(),
            canonical_path: self.descent(hash).into_iter().collect(),
        })
    }
    pub fn subtree_work(&self, hash: &str) -> Option<WorkSum> {
        self.weight(hash).map(|w| exact(&w))
    }
    pub fn difficulty_anchor(&self, hash: &str) -> Option<DifficultyAnchor> {
        let mut at = hash;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(at) {
                return None;
            }
            let n = self.blocks.get(at)?;
            if n.fact.block_height == 1 {
                return Some(DifficultyAnchor {
                    block_height: 1,
                    timestamp: n.snapshot.timestamp,
                    target: n.snapshot.target,
                });
            }
            if n.fact.block_height == 0 {
                return None;
            }
            at = n.fact.parent_block_hash.as_deref()?;
        }
    }
    pub fn consensus_block(&self, hash: &str) -> Option<BlockMeta> {
        let n = self.blocks.get(hash)?;
        let mut work_contributions = BTreeMap::new();
        let mut attributed_runs = BTreeSet::new();
        for (id, g) in &self.grinds {
            if g.block == hash {
                if g.attributed {
                    attributed_runs.insert(id.clone());
                }
                if let Some(work) = fixed(&g.work) {
                    work_contributions.insert(
                        id.clone(),
                        VerifiedWorkContribution::from_verified_import(id.clone(), UInt256(work)),
                    );
                }
            }
        }
        Some(BlockMeta {
            block_hash: hash.into(),
            parent_block_hash: n.fact.parent_block_hash.clone(),
            block_height: n.fact.block_height,
            difficulty_anchor: self.difficulty_anchor(hash),
            work: exact(&self.direct_work[n.handle]),
            child_hashes: self.children(hash),
            work_contributions,
            attributed_runs,
            child_commitments: n.fact.child_commitments.clone().unwrap_or_default(),
        })
    }
    fn anchored(&self, hash: &str) -> bool {
        let mut at = hash;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(at) {
                return false;
            }
            let Some(n) = self.blocks.get(at) else {
                return false;
            };
            if !n.validated || n.excluded {
                return false;
            }
            match n.fact.parent_block_hash.as_deref() {
                None => return true,
                Some(p) => {
                    if !n.linked {
                        return false;
                    }
                    at = p;
                }
            }
        }
    }
    pub fn executed_set_produced(&self, state: &str) -> bool {
        let Some(state) = standard(state) else {
            return false;
        };
        self.blocks.iter().any(|(h, n)| {
            n.snapshot.post_state_cid == state
                && n.snapshot.post_state_cid != n.snapshot.prev_state_cid
                && self.anchored(h)
        })
    }
    pub fn executed_set_advanced(&self, from: &str, to: &str) -> bool {
        let (Some(from), Some(to)) = (standard(from), standard(to)) else {
            return false;
        };
        if from == to || from == empty_state() {
            return self.executed_set_produced(&to);
        }
        // Producer lookup/anchoring still scan the graph; the enclosure test
        // itself follows Swift's disjoint outermost Euler ranges.
        let producers = |state: &str| {
            self.blocks
                .iter()
                .filter(|(h, n)| {
                    n.snapshot.post_state_cid == state
                        && n.snapshot.post_state_cid != n.snapshot.prev_state_cid
                        && self.anchored(h)
                })
                .map(|(_, n)| n.handle)
                .collect::<Vec<_>>()
        };
        self.weights
            .any_encloses(&producers(&from), &producers(&to))
    }
}
fn replay_key(batch: &BlockImportBatch) -> (u8, u64, Target, String, String) {
    if let Ok(t) = trusted(batch) {
        (
            if t.block.is_some() { 0 } else { 1 },
            t.block.map(|(b, _)| b.block_height).unwrap_or(0),
            *t.contribution.work(),
            t.work_block,
            t.contribution.id().into(),
        )
    } else {
        match batch.facts() {
            [ChainFact::Validation(v)] => (2, 0, [0; 32], v.block_hash.clone(), String::new()),
            [ChainFact::Exclusion(v)] => (3, 0, [0; 32], v.block_hash.clone(), String::new()),
            _ => (4, 0, [0; 32], String::new(), String::new()),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AttributedRunIdentity {
    #[serde(rename = "committerBlockHash")]
    pub carrier_block_hash: String,
    pub directory: String,
}
impl AttributedRunIdentity {
    pub fn contribution_id(&self) -> Result<String, lattice_cashew::Error> {
        lattice_primitives::content_id(self)
    }
}
impl ChainTree {
    pub fn serve_runs(&mut self, directory: impl Into<String>) {
        self.served.insert(directory.into());
    }
    pub fn nearest_carrier(&self, hash: &str, directory: &str) -> Option<String> {
        if !self.served.contains(directory) || !self.routed(hash) {
            return None;
        }
        let mut at = Some(hash);
        while let Some(h) = at {
            let n = self.blocks.get(h)?;
            if n.fact
                .child_commitments
                .as_ref()
                .is_some_and(|m| m.contains_key(directory))
            {
                return Some(h.into());
            }
            at = n.fact.parent_block_hash.as_deref();
        }
        None
    }
    pub fn committers(&self, child: &str, directory: &str) -> Vec<String> {
        let Some(child) = standard(child) else {
            return vec![];
        };
        if !self.served.contains(directory) {
            return vec![];
        }
        self.blocks
            .iter()
            .filter(|(h, n)| {
                self.routed(h)
                    && n.fact
                        .child_commitments
                        .as_ref()
                        .and_then(|m| m.get(directory))
                        == Some(&child)
            })
            .map(|(h, _)| h.clone())
            .collect()
    }
    pub fn recorded_child_commitments(&self, hash: &str) -> Option<BTreeMap<String, String>> {
        self.blocks
            .get(&standard(hash)?)
            .map(|n| n.fact.child_commitments.clone().unwrap_or_default())
    }
    pub fn grind_work(&self, hash: &str) -> Option<WorkSum> {
        self.blocks.get(hash)?;
        Some(exact(
            &self
                .grinds
                .values()
                .filter(|g| g.block == hash && !g.attributed)
                .map(|g| g.work.clone())
                .sum(),
        ))
    }
    pub fn run_work(&self, committer: &str, directory: &str) -> Option<WorkSum> {
        if !self.served.contains(directory)
            || !self.routed(committer)
            || self.blocks[committer]
                .fact
                .child_commitments
                .as_ref()
                .and_then(|m| m.get(directory))
                .is_none()
        {
            return None;
        }
        Some(exact(
            &self
                .blocks
                .iter()
                .filter(|(h, _)| self.nearest_carrier(h, directory).as_deref() == Some(committer))
                .map(|(_, n)| self.direct_work[n.handle].clone())
                .sum(),
        ))
    }
    pub fn apply_parent_run(
        &mut self,
        parent: &Self,
        directory: &str,
        parent_blocks: Option<&BTreeSet<String>>,
        held: &BTreeSet<String>,
    ) -> Result<(Vec<String>, Option<ChainCommit>), RestoreError> {
        if !parent.served.contains(directory) {
            return Ok((vec![], None));
        }
        let mut scope: BTreeSet<String> = parent
            .blocks
            .iter()
            .filter(|(h, n)| {
                parent.routed(h)
                    && n.fact
                        .child_commitments
                        .as_ref()
                        .is_some_and(|m| m.contains_key(directory))
            })
            .map(|(h, _)| h.clone())
            .collect();
        if let Some(changed) = parent_blocks {
            let mut candidates: BTreeSet<_> = changed
                .iter()
                .filter_map(|h| parent.nearest_carrier(h, directory))
                .collect();
            for h in held {
                candidates.extend(parent.committers(h, directory));
            }
            scope = scope.intersection(&candidates).cloned().collect();
        }
        let mut raised = vec![];
        for committer in scope {
            let p = &parent.blocks[&committer];
            let child = p
                .fact
                .child_commitments
                .as_ref()
                .unwrap()
                .get(directory)
                .unwrap();
            let Some(n) = self.blocks.get(child) else {
                continue;
            };
            if n.snapshot.parent_state_cid != p.snapshot.prev_state_cid {
                continue;
            }
            let run = parent.run_work(&committer, directory).unwrap();
            let grind = parent.grind_work(&committer).unwrap();
            let Some(value) = run.checked_sub(&grind) else {
                continue;
            };
            let work = BigUint::parse_bytes(value.to_hex().as_bytes(), 16).unwrap();
            let id = AttributedRunIdentity {
                carrier_block_hash: committer,
                directory: directory.into(),
            }
            .contribution_id()
            .map_err(|_| RestoreError::CorruptConsensusGraph)?;
            let old = self
                .grinds
                .get(&id)
                .map(|g| g.work.clone())
                .unwrap_or_default();
            if work <= old {
                continue;
            }
            if self.grinds.get(&id).is_some_and(|g| g.block != *child) {
                return Err(RestoreError::CorruptConsensusGraph);
            }
            if !self.capacity() {
                return Err(RestoreError::RevisionExhausted);
            }
            self.direct_work[self.blocks[child].handle] += &work - &old;
            self.weights
                .changed(self.blocks[child].handle, &self.direct_work);
            self.grinds.insert(
                id,
                Grind {
                    block: child.clone(),
                    work,
                    attributed: true,
                },
            );
            self.revision += 1;
            raised.push(child.clone());
        }
        let commit = if raised.is_empty() || self.replaying {
            None
        } else {
            let commit = self.project();
            commit.canonical_changed().then_some(commit)
        };
        Ok((raised, commit))
    }
    pub fn heaviest_executed_tip(&self, avoided: &BTreeSet<String>) -> String {
        let root = |skip: &BTreeSet<String>| {
            self.preferred(
                self.genesis_roots()
                    .into_iter()
                    .filter(|h| self.anchored(h) && !skip.contains(h)),
            )
        };
        let descend = |start: String, skip: &BTreeSet<String>| {
            let mut tip = start;
            while let Some(next) = self.preferred(
                self.children(&tip)
                    .into_iter()
                    .filter(|h| self.is_executed(h) && !skip.contains(h)),
            ) {
                tip = next;
            }
            tip
        };
        let start = self
            .canonical
            .iter()
            .take_while(|h| self.anchored(h))
            .last()
            .cloned()
            .or_else(|| root(&BTreeSet::new()));
        let Some(start) = start else {
            return String::new();
        };
        let tip = descend(start, &BTreeSet::new());
        let mut first_avoided = None;
        let mut at = Some(tip.as_str());
        while let Some(h) = at {
            if avoided.contains(h) {
                first_avoided = Some(h.to_string())
            }
            at = self.parent(h);
        }
        let Some(block) = first_avoided else {
            return tip;
        };
        match self.parent(&block) {
            Some(parent) => descend(parent.into(), avoided),
            None => root(avoided).map(|r| descend(r, avoided)).unwrap_or(tip),
        }
    }
    pub fn unresolved_same_chain_predecessors(&self) -> Vec<(String, String)> {
        self.blocks
            .iter()
            .filter_map(|(h, n)| {
                n.fact
                    .parent_block_hash
                    .as_ref()
                    .filter(|p| !self.blocks.contains_key(*p))
                    .map(|p| (h.clone(), p.clone()))
            })
            .collect()
    }
    pub fn strongest_grind(&self, hash: &str) -> Option<VerifiedWorkContribution> {
        let (id, g) = self
            .grinds
            .iter()
            .filter(|(_, g)| g.block == hash && !g.attributed)
            .max_by(|(a, x), (b, y)| x.work.cmp(&y.work).then_with(|| b.cmp(a)))?;
        Some(VerifiedWorkContribution::from_verified_import(
            id.clone(),
            UInt256(fixed(&g.work)?),
        ))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TreeHeaderDeferred {
    #[error("parent header unavailable")]
    ParentUnavailable,
    #[error("root schedule spec unavailable")]
    SpecUnavailable,
    #[error(transparent)]
    Schedule(#[from] lattice_validation::HeaderDeferred),
}
impl ChainTree {
    /// Bind schedule content to its immutable volume identity before holding it.
    pub fn hold_spec(&mut self, spec: lattice_primitives::ChainSpec, cid: &str) -> bool {
        if lattice_primitives::content_id(&spec).ok().as_deref() != Some(cid) {
            return false;
        }
        self.specs.insert(cid.into(), spec);
        true
    }
    pub fn root_spec_cid(&self, hash: &str) -> Option<&str> {
        let mut at = hash;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(at) {
                return None;
            }
            let n = self.blocks.get(at)?;
            match n.fact.parent_block_hash.as_deref() {
                None => return Some(&n.snapshot.spec_cid),
                Some(parent) => at = parent,
            }
        }
    }
    /// Header linkage only. PoW, content, child evidence, policy and execution
    /// remain separate requirements; this result cannot manufacture a verified fact.
    pub fn header_linkage(
        &self,
        block: &lattice_primitives::Block,
        now_milliseconds: i64,
    ) -> Result<lattice_validation::HeaderAdmission, TreeHeaderDeferred> {
        let hash = block
            .parent
            .as_ref()
            .map(|v| v.0.raw_cid.as_str())
            .ok_or(TreeHeaderDeferred::ParentUnavailable)?;
        let parent = self
            .header_snapshot(hash)
            .ok_or(TreeHeaderDeferred::ParentUnavailable)?;
        let spec = self
            .root_spec_cid(hash)
            .and_then(|cid| self.specs.get(cid))
            .ok_or(TreeHeaderDeferred::SpecUnavailable)?;
        let parent = lattice_validation::HeaderLinkageParent {
            height: parent.tip_height,
            timestamp: parent.timestamp,
            target: UInt256(parent.target),
            next_target: UInt256(parent.next_target),
            post_state_cid: parent.post_state_cid.clone(),
            spec_cid: parent.spec_cid.clone(),
        };
        lattice_validation::header_admission(
            block,
            &parent,
            spec,
            || {
                self.difficulty_anchor(hash)
                    .map(|a| lattice_validation::DifficultyAnchor {
                        block_height: a.block_height,
                        timestamp: a.timestamp,
                        target: UInt256(a.target),
                    })
            },
            now_milliseconds,
        )
        .map_err(Into::into)
    }
}
impl ChainTree {
    /// Restore parent levels first. Run work is derived again and never journaled.
    /// Seal the durable floor only after derivation, so restart creates no revision.
    pub fn restore_with_parent(
        batches: &[BlockImportBatch],
        revision_floor: u64,
        context: ChainRuntimeContext,
        parent: Option<&ChainTree>,
        specs: Vec<lattice_primitives::ChainSpec>,
    ) -> Result<Self, RestoreError> {
        let mut tree = Self::restore_inner(batches, 0, Some(context.clone()))?;
        if !context.is_root()
            && let Some(parent) = parent
        {
            tree.replaying = true;
            tree.apply_parent_run(
                parent,
                context.path().last().unwrap(),
                None,
                &BTreeSet::new(),
            )?;
            tree.replaying = false;
            tree.project();
        }
        tree.revision = tree.revision.max(revision_floor);
        for spec in specs {
            let cid = lattice_primitives::content_id(&spec)
                .map_err(|_| RestoreError::CorruptConsensusGraph)?;
            tree.specs.insert(cid, spec);
        }
        Ok(tree)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdmissionError {
    #[error("proof of work invalid")]
    ProofOfWorkInvalid,
    #[error("protocol structure invalid")]
    ProtocolInvalid,
    #[error("required evidence unavailable")]
    UnavailableEvidence,
    #[error("not accepted on this chain")]
    NotAcceptedAtCurrentChain,
    #[error("header is from the future")]
    NotYetValid,
    #[error("grind already located elsewhere")]
    ConflictingWorkLocation,
    #[error("revision capacity exhausted")]
    RevisionExhausted,
    #[error("prepared admission's graph changed")]
    StalePreparation,
    #[error("local reducer rejected authenticated input")]
    LocalVerificationFailure,
}
#[derive(Debug)]
pub struct PreparedAdmission {
    base: ChainTree,
    next: ChainTree,
    batches: Vec<BlockImportBatch>,
    block_hash: String,
    excluded: bool,
    weighed: Vec<String>,
    commit: ChainCommit,
}
impl PreparedAdmission {
    pub fn batches(&self) -> &[BlockImportBatch] {
        &self.batches
    }
    pub fn block_hash(&self) -> &str {
        &self.block_hash
    }
    pub fn excluded(&self) -> bool {
        self.excluded
    }
}
#[derive(Debug)]
pub enum HeaderPreparation {
    Prepared(Box<PreparedAdmission>),
    Duplicate,
}
#[derive(Debug)]
pub struct ChainUpdate {
    pub block_hash: String,
    pub batches: Vec<BlockImportBatch>,
    pub excluded: bool,
    pub commit: ChainCommit,
    pub weighed: Vec<String>,
}
impl ChainTree {
    pub fn prepare_root_header(
        &self,
        block: &lattice_primitives::Block,
        children: &lattice_cashew::FlatDictionary<lattice_primitives::BlockHeader>,
        now: i64,
    ) -> Result<HeaderPreparation, AdmissionError> {
        if !self.context.as_ref().is_some_and(|c| c.is_root()) {
            return Err(AdmissionError::NotAcceptedAtCurrentChain);
        }
        if !block.has_valid_pow() {
            return Err(AdmissionError::ProofOfWorkInvalid);
        }
        let hash = lattice_primitives::content_id(block)
            .map_err(|_| AdmissionError::ProofOfWorkInvalid)?;
        let contribution = VerifiedWorkContribution::from_verified_import(
            hash.clone(),
            UInt256(lattice_pow::work_for_target(&block.target.0)),
        );
        self.prepare_header(block, children, hash, contribution, now)
    }
    pub fn prepare_child_header(
        &self,
        block: &lattice_primitives::Block,
        children: &lattice_cashew::FlatDictionary<lattice_primitives::BlockHeader>,
        evidence: &VerifiedChildEvidence,
        now: i64,
    ) -> Result<HeaderPreparation, AdmissionError> {
        let context = self
            .context
            .as_ref()
            .filter(|c| !c.is_root())
            .ok_or(AdmissionError::NotAcceptedAtCurrentChain)?;
        if context.path() != evidence.chain_path() {
            return Err(AdmissionError::NotAcceptedAtCurrentChain);
        }
        let hash = lattice_primitives::content_id(block)
            .map_err(|_| AdmissionError::ProofOfWorkInvalid)?;
        if hash != evidence.child_cid() {
            return Err(AdmissionError::ProofOfWorkInvalid);
        }
        let work = evidence
            .contribution()
            .cloned()
            .ok_or(AdmissionError::ProofOfWorkInvalid)?;
        self.prepare_header(block, children, hash, work, now)
    }
    fn prepare_header(
        &self,
        block: &lattice_primitives::Block,
        children: &lattice_cashew::FlatDictionary<lattice_primitives::BlockHeader>,
        hash: String,
        contribution: VerifiedWorkContribution,
        now: i64,
    ) -> Result<HeaderPreparation, AdmissionError> {
        if contribution.work() == &[0; 32] {
            return Err(AdmissionError::ProofOfWorkInvalid);
        }
        if !block.has_well_formed_reward_recipient() {
            return Err(AdmissionError::ProtocolInvalid);
        }
        if self
            .grinds
            .get(contribution.id())
            .is_some_and(|g| g.block != hash)
        {
            return Err(AdmissionError::ConflictingWorkLocation);
        }
        let mut excluded = false;
        let work = ChainFact::Work(ChainWorkFact {
            block_hash: hash.clone(),
            contribution: contribution.clone(),
        });
        let batches = if self.contains(&hash) {
            if self
                .grinds
                .get(contribution.id())
                .is_some_and(|g| g.work >= BigUint::from_bytes_be(contribution.work()))
            {
                return Ok(HeaderPreparation::Duplicate);
            }
            vec![BlockImportBatch::from_verified_import(vec![work])]
        } else {
            if block.parent.is_none() {
                return Err(AdmissionError::ProtocolInvalid);
            }
            if children.iter().any(|(directory, child)| {
                !lattice_primitives::child_directory(directory)
                    || !lattice_primitives::standard_cid(&child.0.raw_cid)
                    || child.0.encryption_info.is_some()
            }) || lattice_primitives::content_id(children).ok().as_deref()
                != Some(&block.children.raw_cid)
            {
                return Err(AdmissionError::ProtocolInvalid);
            }
            match self.header_linkage(block, now) {
                Ok(lattice_validation::HeaderAdmission::Linked) => {}
                Ok(lattice_validation::HeaderAdmission::Excluded) => excluded = true,
                Ok(lattice_validation::HeaderAdmission::Malformed) => {
                    return Err(AdmissionError::ProtocolInvalid);
                }
                Ok(lattice_validation::HeaderAdmission::OffSchedule) => {
                    return Err(AdmissionError::ProofOfWorkInvalid);
                }
                Err(TreeHeaderDeferred::SpecUnavailable) => {
                    return Err(AdmissionError::NotAcceptedAtCurrentChain);
                }
                Err(TreeHeaderDeferred::Schedule(
                    lattice_validation::HeaderDeferred::NotYetValid,
                )) => return Err(AdmissionError::NotYetValid),
                Err(_) => return Err(AdmissionError::UnavailableEvidence),
            }
            let fact = ChainBlockFact {
                block_hash: hash.clone(),
                parent_block_hash: block.parent.as_ref().map(|p| p.0.raw_cid.clone()),
                block_height: block.height,
                post_state_cid: block.post_state.0.raw_cid.clone(),
                prev_state_cid: block.prev_state.0.raw_cid.clone(),
                parent_state_cid: Some(block.parent_state.0.raw_cid.clone()),
                spec_cid: block.spec.0.raw_cid.clone(),
                target: target_hex(&block.target.0),
                next_target: target_hex(&block.next_target.0),
                timestamp: block.timestamp,
                state_diff: Default::default(),
                child_commitments: Some(
                    children
                        .iter()
                        .map(|(name, h)| (name.clone(), h.0.raw_cid.clone()))
                        .collect(),
                ),
            };
            let mut batches = vec![BlockImportBatch::from_verified_import(vec![
                ChainFact::Block(Box::new(fact)),
                work,
            ])];
            if excluded {
                batches.push(BlockImportBatch::from_verified_import(vec![
                    ChainFact::Exclusion(ChainExclusionFact {
                        block_hash: hash.clone(),
                    }),
                ]));
            }
            batches
        };
        self.prepare_batches(hash, batches, excluded)
    }
    /// Consume a capability produced by the composed root-genesis validator.
    /// The content-bound child index is checked again before deriving journal facts.
    pub fn prepare_root_genesis(
        &self,
        verified: &lattice_validation::VerifiedRootGenesis,
        children: &lattice_cashew::FlatDictionary<lattice_primitives::BlockHeader>,
    ) -> Result<HeaderPreparation, AdmissionError> {
        self.context
            .as_ref()
            .filter(|c| c.is_root() && c.admits_genesis(verified.block_cid()))
            .ok_or(AdmissionError::NotAcceptedAtCurrentChain)?;
        let block = verified
            .header()
            .0
            .node
            .as_deref()
            .ok_or(AdmissionError::LocalVerificationFailure)?;
        if lattice_primitives::content_id(children).ok().as_deref() != Some(&block.children.raw_cid)
        {
            return Err(AdmissionError::ProtocolInvalid);
        }
        let hash = verified.block_cid().to_owned();
        if self.is_executed(&hash) {
            return Ok(HeaderPreparation::Duplicate);
        }
        let contribution = VerifiedWorkContribution::from_verified_import(
            hash.clone(),
            UInt256(lattice_pow::work_for_target(&block.target.0)),
        );
        let fact = ChainBlockFact {
            block_hash: hash.clone(),
            parent_block_hash: None,
            block_height: block.height,
            post_state_cid: block.post_state.0.raw_cid.clone(),
            prev_state_cid: block.prev_state.0.raw_cid.clone(),
            parent_state_cid: Some(block.parent_state.0.raw_cid.clone()),
            spec_cid: block.spec.0.raw_cid.clone(),
            target: target_hex(&block.target.0),
            next_target: target_hex(&block.next_target.0),
            timestamp: block.timestamp,
            state_diff: verified.state_diff().clone(),
            child_commitments: Some(
                children
                    .iter()
                    .map(|(n, h)| (n.clone(), h.0.raw_cid.clone()))
                    .collect(),
            ),
        };
        let batch = BlockImportBatch::from_verified_import(vec![
            ChainFact::Block(Box::new(fact)),
            ChainFact::Work(ChainWorkFact {
                block_hash: hash.clone(),
                contribution,
            }),
            ChainFact::Validation(ChainValidationFact {
                block_hash: hash.clone(),
            }),
        ]);
        let mut prepared = self.prepare_batches(hash, vec![batch], false)?;
        if let HeaderPreparation::Prepared(p) = &mut prepared {
            let spec = verified
                .spec()
                .0
                .node
                .as_deref()
                .ok_or(AdmissionError::LocalVerificationFailure)?;
            if !p.next.hold_spec(spec.clone(), &verified.spec().0.raw_cid) {
                return Err(AdmissionError::LocalVerificationFailure);
            }
        }
        Ok(prepared)
    }
    /// Finalize root execution only when its validated predecessor and anchor
    /// are the ones already held by this graph, rather than caller claims.
    pub fn prepare_root_transition(
        &self,
        verified: &lattice_validation::VerifiedRootTransition,
    ) -> Result<HeaderPreparation, AdmissionError> {
        if !self.context.as_ref().is_some_and(|c| c.is_root()) {
            return Err(AdmissionError::NotAcceptedAtCurrentChain);
        }
        let hash = verified.block_cid();
        let node = self
            .blocks
            .get(hash)
            .ok_or(AdmissionError::UnavailableEvidence)?;
        if node.excluded {
            return Err(AdmissionError::ProtocolInvalid);
        }
        if node.validated {
            return Ok(HeaderPreparation::Duplicate);
        }
        if node.fact.parent_block_hash.as_deref() != Some(verified.parent_cid())
            || node.fact.spec_cid != verified.spec().0.raw_cid
            || node.fact.post_state_cid != verified.post_state().0.raw_cid
            || !self.anchored(verified.parent_cid())
        {
            return Err(AdmissionError::UnavailableEvidence);
        }
        let inherited = self.difficulty_anchor(verified.parent_cid()).map(|a| {
            lattice_validation::DifficultyAnchor {
                block_height: a.block_height,
                timestamp: a.timestamp,
                target: UInt256(a.target),
            }
        });
        if inherited != verified.inherited_anchor() {
            return Err(AdmissionError::StalePreparation);
        }
        let mut fact = node.fact.clone();
        fact.state_diff = verified.state_diff().clone();
        let contribution = self
            .strongest_grind(hash)
            .ok_or(AdmissionError::UnavailableEvidence)?;
        let batch = BlockImportBatch::from_verified_import(vec![
            ChainFact::Block(Box::new(fact)),
            ChainFact::Work(ChainWorkFact {
                block_hash: hash.into(),
                contribution,
            }),
            ChainFact::Validation(ChainValidationFact {
                block_hash: hash.into(),
            }),
        ]);
        self.prepare_batches(hash.into(), vec![batch], false)
    }
    /// Weigh a child genesis before body execution. Spec and child-index bytes
    /// are content-bound, but no transition or parent-continuity fact is asserted.
    pub fn prepare_child_genesis_header(
        &self,
        block: &lattice_primitives::Block,
        children: &lattice_cashew::FlatDictionary<lattice_primitives::BlockHeader>,
        spec: &lattice_primitives::ChainSpec,
        evidence: &VerifiedChildEvidence,
    ) -> Result<HeaderPreparation, AdmissionError> {
        let context = self
            .context
            .as_ref()
            .filter(|c| !c.is_root())
            .ok_or(AdmissionError::NotAcceptedAtCurrentChain)?;
        let hash =
            lattice_primitives::content_id(block).map_err(|_| AdmissionError::ProtocolInvalid)?;
        if evidence.chain_path() != context.path() || evidence.child_cid() != hash {
            return Err(AdmissionError::NotAcceptedAtCurrentChain);
        }
        let work = evidence
            .contribution()
            .cloned()
            .ok_or(AdmissionError::ProofOfWorkInvalid)?;
        if self.contains(&hash) {
            return self.prepare_header(block, children, hash, work, i64::MAX);
        }
        if block.parent.is_some()
            || block.height != 0
            || block.next_target.0 == [0; 32]
            || !block.has_well_formed_reward_recipient()
            || block.parent_state.0.raw_cid == empty_state()
            || lattice_primitives::content_id(spec).ok().as_deref() != Some(&block.spec.0.raw_cid)
            || lattice_primitives::content_id(children).ok().as_deref()
                != Some(&block.children.raw_cid)
            || children.iter().any(|(d, h)| {
                !lattice_primitives::child_directory(d)
                    || !lattice_primitives::standard_cid(&h.0.raw_cid)
                    || h.0.encryption_info.is_some()
            })
        {
            return Err(AdmissionError::ProtocolInvalid);
        }
        let fact = ChainBlockFact {
            block_hash: hash.clone(),
            parent_block_hash: None,
            block_height: 0,
            post_state_cid: block.post_state.0.raw_cid.clone(),
            prev_state_cid: block.prev_state.0.raw_cid.clone(),
            parent_state_cid: Some(block.parent_state.0.raw_cid.clone()),
            spec_cid: block.spec.0.raw_cid.clone(),
            target: target_hex(&block.target.0),
            next_target: target_hex(&block.next_target.0),
            timestamp: block.timestamp,
            state_diff: Default::default(),
            child_commitments: Some(
                children
                    .iter()
                    .map(|(d, h)| (d.clone(), h.0.raw_cid.clone()))
                    .collect(),
            ),
        };
        let batch = BlockImportBatch::from_verified_import(vec![
            ChainFact::Block(Box::new(fact)),
            ChainFact::Work(ChainWorkFact {
                block_hash: hash.clone(),
                contribution: work,
            }),
        ]);
        let mut prepared = self.prepare_batches(hash, vec![batch], false)?;
        if let HeaderPreparation::Prepared(p) = &mut prepared {
            p.next
                .specs
                .insert(block.spec.0.raw_cid.clone(), spec.clone());
        }
        Ok(prepared)
    }
    pub fn prepare_child_genesis(
        &self,
        verified: &lattice_validation::VerifiedChildGenesis,
        evidence: &VerifiedChildEvidence,
        anchor: &crate::VerifiedChildParentAnchor,
    ) -> Result<HeaderPreparation, AdmissionError> {
        self.check_child_bindings(
            verified.block_cid(),
            verified.chain_path(),
            evidence,
            anchor,
        )?;
        if anchor.from_state_cid() != empty_state()
            || anchor.to_state_cid() != verified.parent_state_cid()
        {
            return Err(AdmissionError::ProtocolInvalid);
        }
        if self.is_executed(verified.block_cid()) {
            return Ok(HeaderPreparation::Duplicate);
        }
        let block = verified
            .header()
            .0
            .node
            .as_deref()
            .ok_or(AdmissionError::LocalVerificationFailure)?;
        let children = block
            .children
            .node
            .as_deref()
            .ok_or(AdmissionError::LocalVerificationFailure)?;
        let hash = verified.block_cid().to_owned();
        let fact = ChainBlockFact {
            block_hash: hash.clone(),
            parent_block_hash: None,
            block_height: block.height,
            post_state_cid: block.post_state.0.raw_cid.clone(),
            prev_state_cid: block.prev_state.0.raw_cid.clone(),
            parent_state_cid: Some(block.parent_state.0.raw_cid.clone()),
            spec_cid: block.spec.0.raw_cid.clone(),
            target: target_hex(&block.target.0),
            next_target: target_hex(&block.next_target.0),
            timestamp: block.timestamp,
            state_diff: verified.state_diff().clone(),
            child_commitments: Some(
                children
                    .iter()
                    .map(|(n, h)| (n.clone(), h.0.raw_cid.clone()))
                    .collect(),
            ),
        };
        let contribution = evidence
            .contribution()
            .cloned()
            .ok_or(AdmissionError::ProofOfWorkInvalid)?;
        let batch = BlockImportBatch::from_verified_import(vec![
            ChainFact::Block(Box::new(fact)),
            ChainFact::Work(ChainWorkFact {
                block_hash: hash.clone(),
                contribution,
            }),
            ChainFact::Validation(ChainValidationFact {
                block_hash: hash.clone(),
            }),
        ]);
        let mut prepared = self.prepare_batches(hash, vec![batch], false)?;
        if let HeaderPreparation::Prepared(p) = &mut prepared {
            let spec = verified
                .spec()
                .0
                .node
                .as_deref()
                .ok_or(AdmissionError::LocalVerificationFailure)?;
            if !p.next.hold_spec(spec.clone(), &verified.spec().0.raw_cid) {
                return Err(AdmissionError::LocalVerificationFailure);
            }
        }
        Ok(prepared)
    }
    fn check_child_bindings(
        &self,
        hash: &str,
        path: &[String],
        evidence: &VerifiedChildEvidence,
        anchor: &crate::VerifiedChildParentAnchor,
    ) -> Result<(), AdmissionError> {
        let context = self
            .context
            .as_ref()
            .filter(|c| !c.is_root())
            .ok_or(AdmissionError::NotAcceptedAtCurrentChain)?;
        if context.path() != path
            || context != anchor.child_context()
            || anchor.block_cid() != hash
            || evidence.chain_path() != path
            || evidence.child_cid() != hash
        {
            return Err(AdmissionError::NotAcceptedAtCurrentChain);
        }
        if evidence.contribution().is_none() {
            return Err(AdmissionError::ProofOfWorkInvalid);
        }
        Ok(())
    }
    pub fn prepare_child_transition(
        &self,
        verified: &lattice_validation::VerifiedChildTransition,
        evidence: &VerifiedChildEvidence,
        anchor: &crate::VerifiedChildParentAnchor,
    ) -> Result<HeaderPreparation, AdmissionError> {
        let hash = verified.block_cid();
        self.check_child_bindings(hash, verified.chain_path(), evidence, anchor)?;
        let node = self
            .blocks
            .get(hash)
            .ok_or(AdmissionError::UnavailableEvidence)?;
        if node.excluded {
            return Err(AdmissionError::ProtocolInvalid);
        }
        if node.validated {
            return Ok(HeaderPreparation::Duplicate);
        }
        let parent = self
            .blocks
            .get(verified.parent_cid())
            .ok_or(AdmissionError::UnavailableEvidence)?;
        if node.fact.parent_block_hash.as_deref() != Some(verified.parent_cid())
            || node.fact.spec_cid != verified.spec().0.raw_cid
            || node.fact.post_state_cid != verified.post_state().0.raw_cid
            || !self.anchored(verified.parent_cid())
            || anchor.from_state_cid() != verified.predecessor_parent_state_cid()
            || anchor.to_state_cid() != verified.parent_state_cid()
            || parent.snapshot.parent_state_cid != anchor.from_state_cid()
        {
            return Err(AdmissionError::UnavailableEvidence);
        }
        let inherited = self.difficulty_anchor(verified.parent_cid()).map(|a| {
            lattice_validation::DifficultyAnchor {
                block_height: a.block_height,
                timestamp: a.timestamp,
                target: UInt256(a.target),
            }
        });
        if inherited != verified.inherited_anchor() {
            return Err(AdmissionError::StalePreparation);
        }
        let mut fact = node.fact.clone();
        fact.state_diff = verified.state_diff().clone();
        let contribution = evidence
            .contribution()
            .cloned()
            .ok_or(AdmissionError::ProofOfWorkInvalid)?;
        let batch = BlockImportBatch::from_verified_import(vec![
            ChainFact::Block(Box::new(fact)),
            ChainFact::Work(ChainWorkFact {
                block_hash: hash.into(),
                contribution,
            }),
            ChainFact::Validation(ChainValidationFact {
                block_hash: hash.into(),
            }),
        ]);
        self.prepare_batches(hash.into(), vec![batch], false)
    }
    fn prepare_batches(
        &self,
        hash: String,
        batches: Vec<BlockImportBatch>,
        excluded: bool,
    ) -> Result<HeaderPreparation, AdmissionError> {
        if self
            .revision
            .checked_add(self.reserved)
            .and_then(|v| v.checked_add(batches.len() as u64))
            .is_none()
        {
            return Err(AdmissionError::RevisionExhausted);
        }
        let mut next = self.clone();
        let mut weighed = vec![];
        for batch in &batches {
            if let Some(result) = next
                .apply_staged(batch)
                .map_err(|_| AdmissionError::LocalVerificationFailure)?
            {
                weighed.extend(result.weighed);
            }
        }
        let before: BTreeSet<_> = self.canonical.iter().cloned().collect();
        let after: BTreeSet<_> = next.canonical.iter().cloned().collect();
        let commit = ChainCommit {
            revision: next.revision,
            tip_hash: next.canonical_tip().into(),
            canonical_blocks_added: after
                .difference(&before)
                .map(|h| (h.clone(), next.blocks[h].fact.block_height))
                .collect(),
            canonical_blocks_removed: before.difference(&after).cloned().collect(),
        };
        Ok(HeaderPreparation::Prepared(Box::new(PreparedAdmission {
            base: self.clone(),
            next,
            batches,
            block_hash: hash,
            excluded,
            weighed,
            commit,
        })))
    }
    /// Consume the exact authenticated preparation after its batches are durable.
    /// No bytes are re-resolved after staging. A changed base refuses the token.
    pub fn apply_prepared(
        &mut self,
        prepared: PreparedAdmission,
    ) -> Result<ChainUpdate, AdmissionError> {
        let b = &prepared.base;
        if self.revision != b.revision
            || self.reserved != b.reserved
            || self.context != b.context
            || self.blocks != b.blocks
            || self.direct_work != b.direct_work
            || self.grinds != b.grinds
            || self.canonical != b.canonical
            || self.served != b.served
            || self.specs.keys().ne(b.specs.keys())
        {
            return Err(AdmissionError::StalePreparation);
        }
        *self = prepared.next;
        Ok(ChainUpdate {
            block_hash: prepared.block_hash,
            batches: prepared.batches,
            excluded: prepared.excluded,
            commit: prepared.commit,
            weighed: prepared.weighed,
        })
    }
}
#[derive(Debug, Clone)]
pub struct ConnectJob {
    block_hash: String,
    context: ChainRuntimeContext,
    contribution: VerifiedWorkContribution,
    recorded_child_commitments: BTreeMap<String, String>,
    anchors: BTreeMap<String, DifficultyAnchor>,
}
impl ConnectJob {
    pub fn block_hash(&self) -> &str {
        &self.block_hash
    }
    pub fn context(&self) -> &ChainRuntimeContext {
        &self.context
    }
    pub fn contribution(&self) -> &VerifiedWorkContribution {
        &self.contribution
    }
    pub fn recorded_child_commitments(&self) -> &BTreeMap<String, String> {
        &self.recorded_child_commitments
    }
    pub fn anchors(&self) -> &BTreeMap<String, DifficultyAnchor> {
        &self.anchors
    }
}
impl ChainTree {
    pub fn has_executed_ancestry(&self, hash: &str) -> bool {
        self.anchored(hash)
    }
    pub fn executed_prefix(&self) -> Option<&str> {
        self.canonical
            .iter()
            .take_while(|h| self.anchored(h))
            .last()
            .map(String::as_str)
    }
    pub fn connect_job(&self, hash: &str, grind: Option<&str>) -> Option<ConnectJob> {
        let context = self.context.clone()?;
        let hash = standard(hash)?;
        self.blocks.get(&hash)?;
        let contribution = if let Some(grind) = grind {
            let g = self.grinds.get(grind)?;
            if g.block != hash {
                return None;
            }
            VerifiedWorkContribution::from_verified_import(grind.into(), UInt256(fixed(&g.work)?))
        } else {
            self.strongest_grind(&hash)?
        };
        let mut anchors = BTreeMap::new();
        if let Some(parent) = self.parent(&hash)
            && let Some(anchor) = self.difficulty_anchor(parent)
        {
            anchors.insert(parent.into(), anchor);
        }
        Some(ConnectJob {
            block_hash: hash.clone(),
            context,
            contribution,
            recorded_child_commitments: self.recorded_child_commitments(&hash)?,
            anchors,
        })
    }
    pub fn has_parent_continuity(&self, link: &ParentStateContinuityLink) -> bool {
        self.context
            .as_ref()
            .is_some_and(|context| context.path() == link.parent_path)
            && self.executed_set_advanced(&link.from_state_cid, &link.to_state_cid)
    }
    pub fn held_spec(&self, cid: &str) -> Option<&lattice_primitives::ChainSpec> {
        self.specs.get(cid)
    }
}

/// A root body refusal derived by running composed validation against this
/// graph's executed parent and difficulty anchor. No public error-to-fact API.
#[derive(Debug)]
pub struct VerifiedRootRejection {
    prepared: Box<PreparedAdmission>,
}
impl VerifiedRootRejection {
    pub fn block_hash(&self) -> &str {
        self.prepared.block_hash()
    }
    pub fn into_prepared(self) -> PreparedAdmission {
        *self.prepared
    }
}
#[derive(Debug)]
pub enum RootConnectionExecution {
    Accepted(Box<lattice_validation::VerifiedRootTransition>),
    Rejected(VerifiedRootRejection),
    Unavailable,
}
/// A local root validation basis; it carries no graph mutation authority.
/// Only a tree with an executed parent can prepare one. No graph is cloned.
#[derive(Debug)]
pub struct RootConnectionTask {
    context: ChainRuntimeContext,
    block: String,
    parent: String,
    anchor: Option<lattice_validation::DifficultyAnchor>,
}
#[derive(Debug)]
enum RootValidationResult {
    Accepted(Box<lattice_validation::VerifiedRootTransition>),
    Rejected,
    Unavailable,
}
/// Sealed worker result. Invalidity cannot be supplied as a public boolean.
#[derive(Debug)]
pub struct RootConnectionCompletion {
    basis: RootConnectionTask,
    result: RootValidationResult,
}
impl RootConnectionTask {
    pub fn block_hash(&self) -> &str {
        &self.block
    }
    pub fn run(
        self,
        source: &impl lattice_cashew::ContentSource,
        now: i64,
    ) -> Result<RootConnectionCompletion, AdmissionError> {
        let header = lattice_cashew::Volume(
            lattice_cashew::Header::from_cid(&self.block)
                .map_err(|_| AdmissionError::ProtocolInvalid)?,
        );
        let result =
            match lattice_validation::validate_root_transition(&header, source, now, self.anchor) {
                Ok(verified) => RootValidationResult::Accepted(Box::new(verified)),
                Err(error) if error.is_retriable() => RootValidationResult::Unavailable,
                Err(_) => RootValidationResult::Rejected,
            };
        Ok(RootConnectionCompletion {
            basis: self,
            result,
        })
    }
}
impl RootConnectionCompletion {
    pub fn accepted_header(&self) -> Option<&lattice_primitives::BlockHeader> {
        match &self.result {
            RootValidationResult::Accepted(verified) => Some(verified.header()),
            _ => None,
        }
    }
    pub fn block_hash(&self) -> &str {
        &self.basis.block
    }
}
impl ChainTree {
    fn root_connection_basis(
        &self,
        context: &ChainRuntimeContext,
        hash: &str,
    ) -> Result<RootConnectionTask, AdmissionError> {
        if !self
            .context
            .as_ref()
            .is_some_and(|c| c.is_root() && c == context)
        {
            return Err(AdmissionError::NotAcceptedAtCurrentChain);
        }
        let node = self
            .blocks
            .get(hash)
            .ok_or(AdmissionError::UnavailableEvidence)?;
        let parent = node
            .fact
            .parent_block_hash
            .as_deref()
            .ok_or(AdmissionError::UnavailableEvidence)?;
        if node.validated || node.excluded || !self.anchored(parent) {
            return Err(AdmissionError::StalePreparation);
        }
        let anchor = self
            .difficulty_anchor(parent)
            .map(|a| lattice_validation::DifficultyAnchor {
                block_height: a.block_height,
                timestamp: a.timestamp,
                target: UInt256(a.target),
            });
        Ok(RootConnectionTask {
            context: context.clone(),
            block: hash.into(),
            parent: parent.into(),
            anchor,
        })
    }
    pub fn prepare_root_connection(
        &self,
        job: &ConnectJob,
    ) -> Result<RootConnectionTask, AdmissionError> {
        self.root_connection_basis(job.context(), job.block_hash())
    }
    /// Recheck facts on the actor. Tip/revision movement alone does not revoke
    /// content validation: a valid side-fork block still joins the executed set.
    /// None is cancelled/duplicate/changed evidence, never an exclusion.
    pub fn complete_root_connection(
        &self,
        completion: RootConnectionCompletion,
    ) -> Result<Option<RootConnectionExecution>, AdmissionError> {
        let basis = &completion.basis;
        let current = match self.root_connection_basis(&basis.context, &basis.block) {
            Ok(current) => current,
            Err(
                AdmissionError::StalePreparation
                | AdmissionError::UnavailableEvidence
                | AdmissionError::NotAcceptedAtCurrentChain,
            ) => return Ok(None),
            Err(error) => return Err(error),
        };
        if current.parent != basis.parent || current.anchor != basis.anchor {
            return Ok(None);
        }
        let result = match completion.result {
            RootValidationResult::Accepted(verified) => {
                self.prepare_root_transition(&verified)?;
                RootConnectionExecution::Accepted(verified)
            }
            RootValidationResult::Unavailable => RootConnectionExecution::Unavailable,
            RootValidationResult::Rejected => {
                let batch = BlockImportBatch::from_verified_import(vec![ChainFact::Exclusion(
                    ChainExclusionFact {
                        block_hash: basis.block.clone(),
                    },
                )]);
                let HeaderPreparation::Prepared(prepared) =
                    self.prepare_batches(basis.block.clone(), vec![batch], true)?
                else {
                    return Err(AdmissionError::LocalVerificationFailure);
                };
                RootConnectionExecution::Rejected(VerifiedRootRejection { prepared })
            }
        };
        Ok(Some(result))
    }
    pub fn execute_root_connection(
        &self,
        job: &ConnectJob,
        source: &impl lattice_cashew::ContentSource,
        now: i64,
    ) -> Result<RootConnectionExecution, AdmissionError> {
        let completion = self.prepare_root_connection(job)?.run(source, now)?;
        self.complete_root_connection(completion)?
            .ok_or(AdmissionError::StalePreparation)
    }
}

/// Invalid child execution derived from this graph and an executed same-chain
/// predecessor. Unavailable parent facts and invalid carrier proofs never mint it.
#[derive(Debug)]
pub struct VerifiedChildRejection {
    prepared: Box<PreparedAdmission>,
}
impl VerifiedChildRejection {
    pub fn block_hash(&self) -> &str {
        self.prepared.block_hash()
    }
    pub fn into_prepared(self) -> PreparedAdmission {
        *self.prepared
    }
}
#[derive(Debug)]
pub enum ChildConnectionExecution {
    Genesis(Box<lattice_validation::VerifiedChildGenesis>),
    Transition(Box<lattice_validation::VerifiedChildTransition>),
    Rejected(VerifiedChildRejection),
    AwaitingParent(crate::ParentStateContinuityLink),
    Unavailable,
}
impl ChainTree {
    /// Rebase only a sealed invalid-execution verdict onto the live graph.
    /// Tip changes do not invalidate it; changed ancestry or context does.
    pub fn refresh_child_rejection(
        &self,
        rejection: VerifiedChildRejection,
    ) -> Result<Option<VerifiedChildRejection>, AdmissionError> {
        let base = &rejection.prepared.base;
        let cid = rejection.block_hash();
        let Some(node) = self.blocks.get(cid) else {
            return Ok(None);
        };
        if self.context != base.context
            || node.validated
            || node.excluded
            || base.blocks.get(cid).is_none_or(|old| old.fact != node.fact)
            || node
                .fact
                .parent_block_hash
                .as_deref()
                .is_some_and(|parent| !self.anchored(parent))
        {
            return Ok(None);
        }
        match self.child_execution_rejection(cid)? {
            ChildConnectionExecution::Rejected(rejection) => Ok(Some(rejection)),
            _ => unreachable!(),
        }
    }
    fn child_execution_rejection(
        &self,
        hash: &str,
    ) -> Result<ChildConnectionExecution, AdmissionError> {
        let batch = BlockImportBatch::from_verified_import(vec![ChainFact::Exclusion(
            ChainExclusionFact {
                block_hash: hash.into(),
            },
        )]);
        let HeaderPreparation::Prepared(prepared) =
            self.prepare_batches(hash.into(), vec![batch], true)?
        else {
            return Err(AdmissionError::LocalVerificationFailure);
        };
        Ok(ChildConnectionExecution::Rejected(VerifiedChildRejection {
            prepared,
        }))
    }
    pub fn execute_child_connection(
        &self,
        job: &ConnectJob,
        parent: &ChainTree,
        proof: &crate::ChildBlockProof,
        source: &impl lattice_cashew::ContentSource,
        now: i64,
    ) -> Result<ChildConnectionExecution, AdmissionError> {
        let context = self
            .context
            .as_ref()
            .filter(|c| !c.is_root() && *c == job.context())
            .ok_or(AdmissionError::NotAcceptedAtCurrentChain)?;
        if parent
            .context()
            .is_none_or(|p| p.path() != &context.path()[..context.path().len() - 1])
        {
            return Err(AdmissionError::NotAcceptedAtCurrentChain);
        }
        let node = self
            .blocks
            .get(job.block_hash())
            .ok_or(AdmissionError::UnavailableEvidence)?;
        if node.validated
            || node.excluded
            || node
                .fact
                .parent_block_hash
                .as_deref()
                .is_some_and(|p| !self.anchored(p))
        {
            return Err(AdmissionError::StalePreparation);
        }
        let header: lattice_primitives::BlockHeader = lattice_cashew::Volume(
            lattice_cashew::Header::from_cid(job.block_hash())
                .map_err(|_| AdmissionError::ProtocolInvalid)?,
        );
        let Ok(resolved) = header.0.resolve(source, None) else {
            return Ok(ChildConnectionExecution::Unavailable);
        };
        let block = resolved
            .node
            .as_deref()
            .ok_or(AdmissionError::LocalVerificationFailure)?;
        let evidence = proof
            .verify_securing_work(block, context.path())
            .map_err(|_| AdmissionError::ProofOfWorkInvalid)?;
        if evidence.contribution().is_none() {
            return Err(AdmissionError::ProofOfWorkInvalid);
        }
        let anchor = match self.verify_child_parent_anchor(block, None) {
            Ok(anchor) => anchor,
            Err(crate::ParentAnchorFailure::Required(link)) => {
                let Some(parent_evidence) = parent.prove_parent_continuity(&link) else {
                    return Ok(ChildConnectionExecution::AwaitingParent(link));
                };
                self.verify_child_parent_anchor(block, Some(&parent_evidence))
                    .map_err(|_| AdmissionError::LocalVerificationFailure)?
            }
            Err(crate::ParentAnchorFailure::ProtocolInvalid) => {
                return self.child_execution_rejection(job.block_hash());
            }
            Err(_) => return Err(AdmissionError::UnavailableEvidence),
        };
        if block.parent.is_none() {
            match lattice_validation::validate_child_genesis(&header, source, now, context.path()) {
                Ok(verified) => {
                    self.prepare_child_genesis(&verified, &evidence, &anchor)?;
                    Ok(ChildConnectionExecution::Genesis(Box::new(verified)))
                }
                Err(error) if error.is_retriable() => Ok(ChildConnectionExecution::Unavailable),
                Err(_) => self.child_execution_rejection(job.block_hash()),
            }
        } else {
            let inherited = block
                .parent
                .as_ref()
                .and_then(|p| self.difficulty_anchor(&p.0.raw_cid))
                .map(|a| lattice_validation::DifficultyAnchor {
                    block_height: a.block_height,
                    timestamp: a.timestamp,
                    target: UInt256(a.target),
                });
            match lattice_validation::validate_child_transition(
                &header,
                source,
                now,
                inherited,
                context.path(),
            ) {
                Ok(verified) => {
                    self.prepare_child_transition(&verified, &evidence, &anchor)?;
                    Ok(ChildConnectionExecution::Transition(Box::new(verified)))
                }
                Err(error) if error.is_retriable() => Ok(ChildConnectionExecution::Unavailable),
                Err(_) => self.child_execution_rejection(job.block_hash()),
            }
        }
    }
}
