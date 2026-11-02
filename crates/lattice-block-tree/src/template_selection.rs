//! Template selection over executed graph facts. Selection is a snapshot, not
//! block admission evidence; callers must fence work against subsequent changes.
use crate::ChainTree;
use crate::DifficultyAnchor;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildTemplateTip {
    pub cid: String,
    /// The selected block can be on a sibling branch, not only an ancestor.
    /// This flag has Swift's `tip != tipCID` meaning: omit tip pool/descendants.
    pub below_act_on_tip: bool,
    pub difficulty_anchor: Option<DifficultyAnchor>,
}
impl ChainTree {
    /// Swift ChainCore.actOnTip(from:accepting:) with the predicate supplied by
    /// NodeRuntime.childTip: use the parent's executed set on every branch.
    /// A state claim in a merely weighed header cannot establish continuity.
    /// The starting tip must itself have complete executed ancestry.
    pub fn select_child_template_tip(
        &self,
        from_tip: &str,
        parent: &ChainTree,
        entering_state: &str,
    ) -> Option<ChildTemplateTip> {
        let context = self.context()?;
        if context.is_root()
            || parent.context()?.path() != &context.path()[..context.path().len() - 1]
            || !self.has_executed_ancestry(from_tip)
        {
            return None;
        }
        let mut avoided = BTreeSet::new();
        let mut tip = from_tip.to_owned();
        loop {
            if !self.has_executed_ancestry(&tip) {
                return None;
            }
            let snapshot = self.header_snapshot(&tip)?;
            if parent.executed_set_advanced(&snapshot.parent_state_cid, entering_state) {
                return Some(ChildTemplateTip {
                    below_act_on_tip: tip != from_tip,
                    difficulty_anchor: self.difficulty_anchor(&tip),
                    cid: tip,
                });
            }
            // Even the fallback when all roots are avoided may repeat a tip.
            // Each failed pass consumes one finite graph vertex.
            if !avoided.insert(tip) {
                return None;
            }
            tip = self.heaviest_executed_tip(&avoided);
        }
    }
}
