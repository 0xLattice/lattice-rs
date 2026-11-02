//! Deterministic header linkage and explicit retriable admission outcomes.
use crate::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DifficultyAnchor {
    pub block_height: u64,
    pub timestamp: i64,
    pub target: UInt256,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderLinkageParent {
    pub height: u64,
    pub timestamp: i64,
    pub target: UInt256,
    pub next_target: UInt256,
    pub post_state_cid: String,
    pub spec_cid: String,
}
impl From<&Block> for HeaderLinkageParent {
    fn from(b: &Block) -> Self {
        Self {
            height: b.height,
            timestamp: b.timestamp,
            target: b.target,
            next_target: b.next_target,
            post_state_cid: b.post_state.0.raw_cid.clone(),
            spec_cid: b.spec.0.raw_cid.clone(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderAdmission {
    Linked,
    Excluded,
    Malformed,
    OffSchedule,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HeaderDeferred {
    #[error("block timestamp is in the node's future")]
    NotYetValid,
    #[error("difficulty anchor unavailable locally")]
    AnchorUnavailable,
}
pub fn immediate_difficulty_anchor(
    block: &Block,
    parent: &HeaderLinkageParent,
) -> Option<DifficultyAnchor> {
    match parent.height {
        0 => Some(DifficultyAnchor {
            block_height: 1,
            timestamp: block.timestamp,
            target: block.target,
        }),
        1 => Some(DifficultyAnchor {
            block_height: 1,
            timestamp: parent.timestamp,
            target: parent.target,
        }),
        _ => None,
    }
}
pub fn valid_next_target(
    block: &Block,
    parent: &HeaderLinkageParent,
    spec: &ChainSpec,
    anchor: DifficultyAnchor,
) -> bool {
    let Some(height) = parent.height.checked_add(1) else {
        return false;
    };
    block.target.0 <= parent.next_target.0
        && block.next_target
            == spec.asert_target(
                anchor.target,
                anchor.timestamp,
                anchor.block_height,
                block.timestamp,
                height,
            )
}
/// Called only after PoW and structural content authentication. An excluded
/// header still carries work; a schedule failure does not. Future time and
/// missing local anchors are retryable evidence states, never invalidity facts.
pub fn header_admission(
    block: &Block,
    parent: &HeaderLinkageParent,
    spec: &ChainSpec,
    inherited_anchor: impl FnOnce() -> Option<DifficultyAnchor>,
    now_milliseconds: i64,
) -> Result<HeaderAdmission, HeaderDeferred> {
    if block.version != Block::CURRENT_VERSION || parent.height.checked_add(1) != Some(block.height)
    {
        return Ok(HeaderAdmission::Malformed);
    }
    if block.timestamp <= parent.timestamp {
        return Ok(HeaderAdmission::OffSchedule);
    }
    let anchor = immediate_difficulty_anchor(block, parent)
        .or_else(inherited_anchor)
        .ok_or(HeaderDeferred::AnchorUnavailable)?;
    if !valid_next_target(block, parent, spec, anchor) {
        return Ok(HeaderAdmission::OffSchedule);
    }
    if block.timestamp > now_milliseconds {
        return Err(HeaderDeferred::NotYetValid);
    }
    if block.spec.0.raw_cid != parent.spec_cid
        || block.prev_state.0.raw_cid != parent.post_state_cid
    {
        return Ok(HeaderAdmission::Excluded);
    }
    Ok(HeaderAdmission::Linked)
}
pub fn has_genesis_shape(block: &Block, is_root: bool) -> Result<bool, lattice_cashew::Error> {
    let empty = content_id(&LatticeState::empty()?)?;
    Ok(block.version == Block::CURRENT_VERSION
        && block.parent.is_none()
        && block.height == 0
        && block.prev_state.0.raw_cid == empty
        && (block.parent_state.0.raw_cid == empty) == is_root
        && block.target == block.next_target
        && block.reward_recipient.is_none())
}
pub fn valid_genesis_credits(spec: &ChainSpec, actions: &[AccountAction]) -> bool {
    let mut credits = BigUint::default();
    for action in actions {
        if !action.verify() {
            return false;
        }
        if action.delta > 0 {
            credits += action.absolute_amount();
        }
    }
    credits <= BigUint::from(spec.premine_amount())
}
pub fn valid_chain_paths(bodies: &[TransactionBody], path: &[String]) -> bool {
    bodies
        .iter()
        .all(|b| !b.chain_path.is_empty() && b.chain_path == path)
}
pub fn valid_root_actions(bodies: &[TransactionBody], path: &[String]) -> bool {
    path.len() != 1
        || bodies
            .iter()
            .all(|b| b.deposit_actions.is_empty() && b.withdrawal_actions.is_empty())
}
