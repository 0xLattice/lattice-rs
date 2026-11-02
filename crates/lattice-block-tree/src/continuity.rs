//! Capabilities for parent executed-set continuity. A carrier proof alone can
//! never mint these: its parent-state bytes need not have been executed.
use crate::{ChainTree, ParentStateContinuityLink};
use lattice_primitives::{Block, ChainRuntimeContext, LatticeState, content_id};
#[derive(Debug)]
pub struct VerifiedParentContinuity {
    link: ParentStateContinuityLink,
    parent_context: ChainRuntimeContext,
}
impl VerifiedParentContinuity {
    pub fn link(&self) -> &ParentStateContinuityLink {
        &self.link
    }
    pub fn parent_context(&self) -> &ChainRuntimeContext {
        &self.parent_context
    }
}
#[derive(Debug)]
pub struct VerifiedChildParentAnchor {
    block_cid: String,
    child_context: ChainRuntimeContext,
    from_state_cid: String,
    to_state_cid: String,
}
impl VerifiedChildParentAnchor {
    pub fn block_cid(&self) -> &str {
        &self.block_cid
    }
    pub fn child_context(&self) -> &ChainRuntimeContext {
        &self.child_context
    }
    pub fn from_state_cid(&self) -> &str {
        &self.from_state_cid
    }
    pub fn to_state_cid(&self) -> &str {
        &self.to_state_cid
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ParentAnchorFailure {
    #[error("parent anchor operation requires a child-chain context")]
    WrongContext,
    #[error("same-chain predecessor header is unavailable")]
    PredecessorUnavailable,
    #[error("invalid parent anchor or unexpected parent evidence")]
    ProtocolInvalid,
    #[error("parent executed-set continuity has not yet been established")]
    Required(ParentStateContinuityLink),
}
impl ChainTree {
    /// A positive result is derived only from actual graph facts. Executed
    /// ancestry and verdicts are irreversible, so later fork-choice movement
    /// does not invalidate this evidence. Missing evidence is retryable.
    pub fn prove_parent_continuity(
        &self,
        link: &ParentStateContinuityLink,
    ) -> Option<VerifiedParentContinuity> {
        if !self.has_parent_continuity(link) {
            return None;
        }
        Some(VerifiedParentContinuity {
            link: link.clone(),
            parent_context: self.context()?.clone(),
        })
    }
    /// Bind a parent's evidence to this child's exact bytes and actual held
    /// predecessor, including the explicit empty-anchor protocol rules.
    pub fn verify_child_parent_anchor(
        &self,
        child: &Block,
        evidence: Option<&VerifiedParentContinuity>,
    ) -> Result<VerifiedChildParentAnchor, ParentAnchorFailure> {
        use ParentAnchorFailure::*;
        let context = self
            .context()
            .filter(|c| !c.is_root())
            .ok_or(WrongContext)?;
        let empty = LatticeState::empty()
            .and_then(lattice_cashew::Volume::new)
            .map_err(|_| ProtocolInvalid)?
            .0
            .raw_cid;
        let from = if let Some(parent) = &child.parent {
            self.header_snapshot(&parent.0.raw_cid)
                .ok_or(PredecessorUnavailable)?
                .parent_state_cid
                .clone()
        } else {
            empty.clone()
        };
        let to = child.parent_state.0.raw_cid.clone();
        if to == empty {
            if child.parent.is_none() || from != empty || evidence.is_some() {
                return Err(ProtocolInvalid);
            }
        } else {
            let expected = ParentStateContinuityLink::new(
                context.path()[..context.path().len() - 1].to_vec(),
                from.clone(),
                to.clone(),
            );
            match evidence {
                Some(e)
                    if e.link == expected && e.parent_context.path() == expected.parent_path => {}
                Some(_) => return Err(ProtocolInvalid),
                None => return Err(Required(expected)),
            }
        }
        Ok(VerifiedChildParentAnchor {
            block_cid: content_id(child).map_err(|_| ProtocolInvalid)?,
            child_context: context.clone(),
            from_state_cid: from,
            to_state_cid: to,
        })
    }
}
