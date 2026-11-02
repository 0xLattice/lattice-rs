use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Counts materialized references, preserving repeated edges and zero entries.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDiff {
    pub replaced: BTreeMap<String, i64>,
    pub created: BTreeMap<String, i64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("state diff count overflow")]
pub struct StateDiffOverflow;
impl StateDiff {
    pub fn is_empty(&self) -> bool {
        self.replaced.is_empty() && self.created.is_empty()
    }
    /// Swift traps on Int overflow. Rust returns an explicit error and leaves
    /// the original diff unchanged, avoiding a release-mode wrapping count.
    pub fn merging(&self, other: &Self) -> Result<Self, StateDiffOverflow> {
        let mut result = self.clone();
        for (target, source) in [
            (&mut result.replaced, &other.replaced),
            (&mut result.created, &other.created),
        ] {
            for (cid, count) in source {
                let current = target.entry(cid.clone()).or_default();
                *current = current.checked_add(*count).ok_or(StateDiffOverflow)?;
            }
        }
        Ok(result)
    }
    pub fn merge(&mut self, other: &Self) -> Result<(), StateDiffOverflow> {
        *self = self.merging(other)?;
        Ok(())
    }
}

pub type AccountState = lattice_cashew::VolumeMerkleDictionary<u64>;
pub type GeneralState = lattice_cashew::VolumeMerkleDictionary<String>;
pub type DepositState = lattice_cashew::VolumeMerkleDictionary<u64>;
pub type ReceiptState = lattice_cashew::VolumeMerkleDictionary<String>;
pub type LatticeStateHeader = lattice_cashew::Volume<LatticeState>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticeState {
    pub account_state: lattice_cashew::Volume<AccountState>,
    pub general_state: lattice_cashew::Volume<GeneralState>,
    pub deposit_state: lattice_cashew::Volume<DepositState>,
    pub receipt_state: lattice_cashew::Volume<ReceiptState>,
}
impl LatticeState {
    pub fn empty() -> Result<Self, lattice_cashew::Error> {
        use lattice_cashew::Volume;
        Ok(Self {
            account_state: Volume::new(AccountState::default())?,
            general_state: Volume::new(GeneralState::default())?,
            deposit_state: Volume::new(DepositState::default())?,
            receipt_state: Volume::new(ReceiptState::default())?,
        })
    }
}
impl lattice_cashew::DagNode for LatticeState {
    fn links(&self) -> Vec<&dyn lattice_cashew::DagHeader> {
        vec![
            &self.account_state,
            &self.general_state,
            &self.deposit_state,
            &self.receipt_state,
        ]
    }
}

/// Reference-count diff of materialized state trie nodes. Unchanged commitments
/// are skipped, and unresolved nodes contribute no materialized references.
pub fn diff_state_dictionary<V: Clone + Serialize>(
    old: &lattice_cashew::Volume<lattice_cashew::VolumeMerkleDictionary<V>>,
    new: &lattice_cashew::Volume<lattice_cashew::VolumeMerkleDictionary<V>>,
) -> Result<StateDiff, StateDiffOverflow> {
    if old.0.raw_cid == new.0.raw_cid {
        return Ok(StateDiff::default());
    }
    let mut diff = StateDiff::default();
    if old.0.node.is_some() {
        increment(&mut diff.replaced, &old.0.raw_cid)?;
    }
    if new.0.node.is_some() {
        increment(&mut diff.created, &new.0.raw_cid)?;
    }
    if let (Some(old), Some(new)) = (&old.0.node, &new.0.node) {
        diff_children(&old.children, &new.children, &mut diff)?;
    }
    Ok(diff)
}
fn increment(map: &mut BTreeMap<String, i64>, cid: &str) -> Result<(), StateDiffOverflow> {
    let value = map.entry(cid.into()).or_default();
    *value = value.checked_add(1).ok_or(StateDiffOverflow)?;
    Ok(())
}
type StateEdges<V> =
    [lattice_cashew::radix::Entry<lattice_cashew::Volume<lattice_cashew::VolumeRadixNode<V>>>];
fn collect_trie<V>(
    header: &lattice_cashew::Volume<lattice_cashew::VolumeRadixNode<V>>,
    counts: &mut BTreeMap<String, i64>,
) -> Result<(), StateDiffOverflow> {
    if let Some(node) = &header.0.node {
        increment(counts, &header.0.raw_cid)?;
        for child in &node.children {
            collect_trie(&child.value, counts)?;
        }
    }
    Ok(())
}
fn diff_children<V>(
    old: &StateEdges<V>,
    new: &StateEdges<V>,
    diff: &mut StateDiff,
) -> Result<(), StateDiffOverflow> {
    for entry in old {
        if let Some(next) = new.iter().find(|e| e.key == entry.key) {
            let old = &entry.value.0;
            let new = &next.value.0;
            if old.raw_cid == new.raw_cid {
                continue;
            }
            if old.node.is_some() {
                increment(&mut diff.replaced, &old.raw_cid)?;
            }
            if new.node.is_some() {
                increment(&mut diff.created, &new.raw_cid)?;
            }
            if let (Some(old), Some(new)) = (&old.node, &new.node) {
                diff_children(&old.children, &new.children, diff)?;
            }
        } else {
            collect_trie(&entry.value, &mut diff.replaced)?;
        }
    }
    for entry in new {
        if !old.iter().any(|e| e.key == entry.key) {
            collect_trie(&entry.value, &mut diff.created)?;
        }
    }
    Ok(())
}
