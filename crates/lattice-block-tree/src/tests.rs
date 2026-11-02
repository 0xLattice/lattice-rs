use crate::*;
use lattice_primitives::{ChainRuntimeContext, StateDiff, UInt256};
use std::collections::{BTreeMap, BTreeSet};
fn cid(s: &str) -> String {
    lattice_cashew::cid_for_bytes(0x71, s.as_bytes())
}
fn context(root: &str) -> ChainRuntimeContext {
    ChainRuntimeContext::new(vec!["Nexus".into()], Some(&cid(root))).unwrap()
}
fn batch(
    name: &str,
    parent: Option<&str>,
    height: u64,
    work: u64,
    valid: bool,
) -> BlockImportBatch {
    let mut bytes = [0; 32];
    bytes[24..].copy_from_slice(&work.to_be_bytes());
    let hash = cid(name);
    let mut facts = vec![
        ChainFact::Block(Box::new(ChainBlockFact {
            block_hash: hash.clone(),
            parent_block_hash: parent.map(cid),
            block_height: height,
            post_state_cid: cid(&format!("post/{name}")),
            prev_state_cid: parent
                .map(|p| cid(&format!("post/{p}")))
                .unwrap_or_else(|| cid("empty")),
            parent_state_cid: Some(cid("parent-state")),
            spec_cid: cid("spec"),
            target: "ff".into(),
            next_target: "ff".into(),
            timestamp: height as i64,
            state_diff: StateDiff::default(),
            child_commitments: None,
        })),
        ChainFact::Work(ChainWorkFact {
            block_hash: hash.clone(),
            contribution: VerifiedWorkContribution::from_verified_import(
                cid(&format!("grind/{name}")),
                UInt256(bytes),
            ),
        }),
    ];
    if valid {
        facts.push(ChainFact::Validation(ChainValidationFact {
            block_hash: hash,
        }));
    }
    BlockImportBatch::from_verified_import(facts)
}
fn exclude(name: &str) -> BlockImportBatch {
    BlockImportBatch::from_verified_import(vec![ChainFact::Exclusion(ChainExclusionFact {
        block_hash: cid(name),
    })])
}
#[test]
fn replay_order_floor_and_work_ratchet() {
    let g = batch("g", None, 0, 1, true);
    let a = batch("a", Some("g"), 1, 5, true);
    let b = batch("b", Some("g"), 1, 10, false);
    let rise = batch("a", Some("g"), 1, 20, true);
    let batches = [g, a, b, rise];
    let tree = ChainTree::restore(&batches, 100, context("g")).unwrap();
    assert_eq!(tree.canonical_tip(), cid("a"));
    assert_eq!(
        tree.subtree_work(&cid("g")).unwrap().to_hex(),
        format!("{:064x}", 31)
    );
    assert_eq!(tree.current_revision(), 100);
    for shift in 0..4 {
        let mut perm = batches.to_vec();
        perm.rotate_left(shift);
        perm.reverse();
        let other = ChainTree::restore(&perm, 100, context("g")).unwrap();
        assert_eq!(tree.canonical_path(), other.canonical_path());
        assert_eq!(tree.subtree_work(&cid("g")), other.subtree_work(&cid("g")));
    }
}
#[test]
fn exclusion_keeps_weight_and_changes_selection() {
    let tree = ChainTree::restore(
        &[
            batch("g", None, 0, 1, true),
            batch("a", Some("g"), 1, 10, false),
            batch("a2", Some("a"), 2, 100, false),
            batch("b", Some("g"), 1, 5, true),
            exclude("a"),
        ],
        0,
        context("g"),
    )
    .unwrap();
    assert_eq!(tree.canonical_tip(), cid("b"));
    assert_eq!(
        tree.subtree_work(&cid("g")).unwrap().to_hex(),
        format!("{:064x}", 116)
    );
    assert_eq!(tree.heaviest_executed_tip(&BTreeSet::new()), cid("b"));
}
#[test]
fn validation_and_exclusion_contradiction_fails() {
    assert_eq!(
        ChainTree::restore(
            &[
                batch("g", None, 0, 1, true),
                batch("a", Some("g"), 1, 2, true),
                exclude("a")
            ],
            0,
            context("g")
        )
        .unwrap_err(),
        RestoreError::ExecutedVerdictContradiction
    );
}
#[test]
fn orphan_grafts_when_missing_parent_arrives() {
    let mut tree = ChainTree::empty(context("g"));
    tree.replay(&batch("c", Some("a"), 2, 8, false)).unwrap();
    assert!(tree.subtree_work(&cid("c")).is_none());
    tree.replay(&batch("g", None, 0, 1, true)).unwrap();
    assert_eq!(tree.canonical_tip(), cid("g"));
    tree.replay(&batch("a", Some("g"), 1, 2, true)).unwrap();
    assert_eq!(tree.canonical_tip(), cid("c"));
    assert_eq!(
        tree.subtree_work(&cid("g")).unwrap().to_hex(),
        format!("{:064x}", 11)
    );
}
#[test]
fn one_grind_cannot_move_and_failed_apply_is_atomic() {
    let mut tree = ChainTree::restore(&[batch("g", None, 0, 1, true)], 0, context("g")).unwrap();
    let mut a = batch("a", Some("g"), 1, 5, false).facts().to_vec();
    if let ChainFact::Work(w) = &mut a[1] {
        w.contribution =
            VerifiedWorkContribution::from_verified_import(cid("grind/g"), UInt256([255; 32]));
    }
    assert!(
        tree.replay(&BlockImportBatch::from_verified_import(a))
            .is_err()
    );
    assert_eq!(tree.block_count(), 1);
    assert_eq!(tree.current_revision(), 1);
}
#[test]
fn production_requires_executed_ancestry_and_non_noop() {
    let mut tree = ChainTree::restore(
        &[
            batch("g", None, 0, 1, true),
            batch("a", Some("g"), 1, 2, false),
            batch("b", Some("a"), 2, 3, true),
        ],
        0,
        context("g"),
    )
    .unwrap();
    assert!(!tree.executed_set_produced(&cid("post/b")));
    tree.replay(&BlockImportBatch::validation(cid("a")))
        .unwrap();
    assert!(tree.executed_set_advanced(&cid("post/a"), &cid("post/b")));
    assert!(!tree.executed_set_advanced(&cid("forged"), &cid("forged")));
    assert_eq!(
        tree.heaviest_executed_tip(&BTreeSet::from([cid("b")])),
        cid("a")
    );
}
#[test]
fn derived_parent_run_excludes_carriers_own_work_and_is_idempotent() {
    let mut g = batch("g", None, 0, 3, true).facts().to_vec();
    if let ChainFact::Block(b) = &mut g[0] {
        b.child_commitments = Some(BTreeMap::from([("Alpha".into(), cid("child"))]));
        b.prev_state_cid = cid("parent-state");
    }
    let mut parent = ChainTree::restore(
        &[
            BlockImportBatch::from_verified_import(g),
            batch("a", Some("g"), 1, 7, false),
        ],
        0,
        context("g"),
    )
    .unwrap();
    parent.serve_runs("Alpha");
    let child_context =
        ChainRuntimeContext::new(vec!["Nexus".into(), "Alpha".into()], None).unwrap();
    let mut child =
        ChainTree::restore(&[batch("child", None, 0, 2, true)], 0, child_context).unwrap();
    assert_eq!(
        child
            .apply_parent_run(&parent, "Alpha", None, &BTreeSet::new())
            .unwrap()
            .0,
        vec![cid("child")]
    );
    assert_eq!(
        child.subtree_work(&cid("child")).unwrap().to_hex(),
        format!("{:064x}", 9)
    );
    let rev = child.current_revision();
    assert!(
        child
            .apply_parent_run(&parent, "Alpha", None, &BTreeSet::new())
            .unwrap()
            .0
            .is_empty()
    );
    assert_eq!(child.current_revision(), rev);
}
#[test]
fn revision_capacity_checks_reserved_slots() {
    let mut tree =
        ChainTree::restore(&[batch("g", None, 0, 1, true)], u64::MAX - 1, context("g")).unwrap();
    assert!(tree.reserve_import_revision());
    assert!(!tree.reserve_import_revision());
    assert!(tree.replay(&batch("a", Some("g"), 1, 2, false)).is_err());
    tree.release_import_revision();
    tree.replay(&batch("a", Some("g"), 1, 2, false)).unwrap();
    assert_eq!(tree.current_revision(), u64::MAX);
    assert!(tree.replay(&batch("b", Some("g"), 1, 2, false)).is_err());
    assert!(!tree.contains(&cid("b")));
}
#[test]
fn derived_work_preserves_more_than_256_bits() {
    let max_batch = |name: &str, parent: Option<&str>, height: u64| {
        let mut facts = batch(name, parent, height, 1, false).facts().to_vec();
        if let ChainFact::Work(w) = &mut facts[1] {
            w.contribution = VerifiedWorkContribution::from_verified_import(
                cid(&format!("grind/{name}")),
                UInt256([255; 32]),
            );
        }
        BlockImportBatch::from_verified_import(facts)
    };
    let mut facts = batch("g", None, 0, 1, true).facts().to_vec();
    if let ChainFact::Block(b) = &mut facts[0] {
        b.child_commitments = Some(BTreeMap::from([("Alpha".into(), cid("child"))]));
        b.prev_state_cid = cid("parent-state");
    }
    let mut parent = ChainTree::restore(
        &[
            BlockImportBatch::from_verified_import(facts),
            max_batch("a", Some("g"), 1),
            max_batch("b", Some("a"), 2),
        ],
        0,
        context("g"),
    )
    .unwrap();
    parent.serve_runs("Alpha");
    let child_context =
        ChainRuntimeContext::new(vec!["Nexus".into(), "Alpha".into()], None).unwrap();
    let mut child = ChainTree::restore(
        &[batch("child", None, 0, 2, true)],
        0,
        child_context.clone(),
    )
    .unwrap();
    child
        .apply_parent_run(&parent, "Alpha", None, &BTreeSet::new())
        .unwrap();
    assert_eq!(
        child.subtree_work(&cid("child")).unwrap().to_hex(),
        format!("2{}", "0".repeat(64))
    );
    assert_eq!(
        child
            .consensus_block(&cid("child"))
            .unwrap()
            .attributed_runs
            .len(),
        1
    );
    let restored = ChainTree::restore_with_parent(
        &[batch("child", None, 0, 2, true)],
        50,
        child_context,
        Some(&parent),
        vec![],
    )
    .unwrap();
    assert_eq!(
        restored.subtree_work(&cid("child")),
        child.subtree_work(&cid("child"))
    );
    assert_eq!(restored.current_revision(), 50);
}
#[test]
fn contribution_decoding_normalizes_base_alias() {
    let id = cid("grind");
    let parsed = id.parse::<cid::Cid>().unwrap();
    let alias = parsed
        .to_string_of_base(cid::multibase::Base::Base58Btc)
        .unwrap();
    let work: VerifiedWorkContribution =
        serde_json::from_value(serde_json::json!({"id":alias,"work":"0xf"})).unwrap();
    assert_eq!(work.id(), id);
}

#[test]
fn parent_continuity_capability_uses_executed_side_branches_and_rejects_sideways() {
    let tree = ChainTree::restore(
        &[
            batch("g", None, 0, 1, true),
            batch("a", Some("g"), 1, 2, true),
            batch("aa", Some("a"), 2, 2, true),
            batch("b", Some("g"), 1, 20, true),
            batch("pending", Some("b"), 2, 50, false),
        ],
        0,
        context("g"),
    )
    .unwrap();
    let link = |from: &str, to: &str| {
        ParentStateContinuityLink::new(
            vec!["Nexus".into()],
            cid(&format!("post/{from}")),
            cid(&format!("post/{to}")),
        )
    };
    assert_ne!(tree.canonical_tip(), cid("aa"));
    assert!(tree.prove_parent_continuity(&link("a", "aa")).is_some());
    assert!(tree.prove_parent_continuity(&link("a", "a")).is_some());
    assert!(tree.prove_parent_continuity(&link("aa", "a")).is_none());
    assert!(tree.prove_parent_continuity(&link("a", "b")).is_none());
    assert!(
        tree.prove_parent_continuity(&link("pending", "pending"))
            .is_none()
    );
    let mut wrong = link("a", "aa");
    wrong.parent_path.push("child".into());
    assert!(tree.prove_parent_continuity(&wrong).is_none());
}
fn anchor_block(parent: Option<&str>, state: String) -> lattice_primitives::Block {
    use lattice_cashew::{Header, MerkleDictionary, Volume};
    use lattice_primitives::*;
    let empty = Volume::new(LatticeState::empty().unwrap()).unwrap();
    Block {
        version: 1,
        parent: parent.map(|p| Volume(Header::from_cid(cid(p)).unwrap())),
        transactions: Header::new(MerkleDictionary::default()).unwrap(),
        target: UInt256([255; 32]),
        next_target: UInt256([255; 32]),
        spec: Volume(Header::from_cid(cid("spec")).unwrap()),
        parent_state: Volume(Header::from_cid(state).unwrap()),
        prev_state: empty.clone(),
        post_state: empty,
        children: Header::new(BTreeMap::new()).unwrap(),
        height: if parent.is_some() { 1 } else { 0 },
        timestamp: 1,
        reward_recipient: None,
        nonce: 0,
    }
}
#[test]
fn child_parent_anchor_cannot_launder_equal_unproduced_state_or_empty_genesis() {
    use lattice_primitives::LatticeState;
    let parent = ChainTree::restore(&[batch("g", None, 0, 1, true)], 0, context("g")).unwrap();
    let child = ChainTree::empty(
        ChainRuntimeContext::new(vec!["Nexus".into(), "child".into()], None).unwrap(),
    );
    let block = anchor_block(None, cid("post/g"));
    let Err(ParentAnchorFailure::Required(link)) = child.verify_child_parent_anchor(&block, None)
    else {
        panic!()
    };
    let evidence = parent.prove_parent_continuity(&link).unwrap();
    let verified = child
        .verify_child_parent_anchor(&block, Some(&evidence))
        .unwrap();
    assert_eq!(
        verified.block_cid(),
        lattice_primitives::content_id(&block).unwrap()
    );
    let empty = lattice_cashew::Volume::new(LatticeState::empty().unwrap())
        .unwrap()
        .0
        .raw_cid;
    assert!(matches!(
        child.verify_child_parent_anchor(&anchor_block(None, empty.clone()), None),
        Err(ParentAnchorFailure::ProtocolInvalid)
    ));
    let held = ChainTree::restore(
        &[batch("cg", None, 0, 1, false)],
        0,
        ChainRuntimeContext::new(vec!["Nexus".into(), "child".into()], None).unwrap(),
    )
    .unwrap();
    let forged = anchor_block(Some("cg"), cid("parent-state"));
    let Err(ParentAnchorFailure::Required(link)) = held.verify_child_parent_anchor(&forged, None)
    else {
        panic!()
    };
    assert_eq!(link.from_state_cid, link.to_state_cid);
    assert!(parent.prove_parent_continuity(&link).is_none());
    assert!(matches!(
        held.verify_child_parent_anchor(&anchor_block(Some("cg"), empty), None),
        Err(ParentAnchorFailure::ProtocolInvalid)
    ));
}

#[test]
fn child_template_tip_uses_parent_side_branches_and_executed_fallback() {
    let parent = ChainTree::restore(
        &[
            batch("pg", None, 0, 1, true),
            batch("pa", Some("pg"), 1, 100, true),
            batch("pb", Some("pg"), 1, 2, true),
            batch("pbb", Some("pb"), 2, 2, true),
            batch("pending", Some("pb"), 2, 1, false),
        ],
        0,
        context("pg"),
    )
    .unwrap();
    let anchored = |name, predecessor, height, weight, valid, anchor: &str| {
        let original = batch(name, predecessor, height, weight, valid);
        let mut facts = original.facts().to_vec();
        for fact in &mut facts {
            if let ChainFact::Block(block) = fact {
                block.parent_state_cid = Some(cid(&format!("post/{anchor}")));
            }
        }
        BlockImportBatch::from_verified_import(facts)
    };
    let child = ChainTree::restore(
        &[
            anchored("cg", None, 0, 1, true, "pg"),
            anchored("ca", Some("cg"), 1, 20, true, "pa"),
            anchored("cb", Some("cg"), 1, 10, true, "pb"),
            anchored("unexecuted", Some("ca"), 2, 1, false, "pb"),
        ],
        0,
        ChainRuntimeContext::new(vec!["Nexus".into(), "child".into()], None).unwrap(),
    )
    .unwrap();
    assert_eq!(parent.canonical_tip(), cid("pa"));
    let own = child
        .select_child_template_tip(&cid("ca"), &parent, &cid("post/pa"))
        .unwrap();
    assert_eq!(own.cid, cid("ca"));
    assert!(!own.below_act_on_tip);
    let fallback = child
        .select_child_template_tip(&cid("ca"), &parent, &cid("post/pbb"))
        .unwrap();
    assert_eq!(fallback.cid, cid("cb"));
    assert!(fallback.below_act_on_tip);
    assert_eq!(
        fallback.difficulty_anchor,
        child.difficulty_anchor(&cid("cb"))
    );
    assert!(
        child
            .select_child_template_tip(&cid("ca"), &parent, &cid("post/pending"))
            .is_none()
    );
    assert!(
        child
            .select_child_template_tip(&cid("ca"), &parent, &cid("forged"))
            .is_none()
    );
    assert!(
        child
            .select_child_template_tip(&cid("unexecuted"), &parent, &cid("post/pb"))
            .is_none()
    );
    assert!(
        child
            .select_child_template_tip(&cid("missing"), &parent, &cid("post/pb"))
            .is_none()
    );
    assert!(
        child
            .select_child_template_tip(&cid("ca"), &child, &cid("post/pb"))
            .is_none()
    );
}

/// Independent adjacency oracle retains the pre-index semantics, including
/// unrouted orphans, exact weights, exclusions, and CID-byte tie breaks.
#[test]
fn indexed_live_grafts_and_recovery_match_adjacency() {
    use num_bigint::BigUint;
    fn routed(tree: &ChainTree, hash: &str) -> bool {
        let mut at = hash.to_owned();
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(at.clone()) {
                return false;
            }
            let Some(meta) = tree.consensus_block(&at) else {
                return false;
            };
            match meta.parent_block_hash {
                None => return meta.block_height == 0,
                Some(parent) => {
                    if !tree.children(&parent).contains(&at) {
                        return false;
                    }
                    at = parent;
                }
            }
        }
    }
    fn weight(tree: &ChainTree, hash: &str) -> Option<BigUint> {
        if !routed(tree, hash) {
            return None;
        }
        let mut stack = vec![hash.to_owned()];
        let mut total = BigUint::default();
        while let Some(h) = stack.pop() {
            total += BigUint::parse_bytes(
                tree.consensus_block(&h).unwrap().work.to_hex().as_bytes(),
                16,
            )
            .unwrap();
            stack.extend(tree.children(&h));
        }
        Some(total)
    }
    fn preferred(tree: &ChainTree, candidates: Vec<String>) -> Option<String> {
        let mut best: Option<(String, BigUint)> = None;
        for h in candidates {
            if tree.is_excluded_root(&h) {
                continue;
            }
            let Some(w) = weight(tree, &h) else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|(b, bw)| w > *bw || (w == *bw && fork_choice_prefers_block(&h, b)))
            {
                best = Some((h, w));
            }
        }
        best.map(|(h, _)| h)
    }
    fn check(tree: &ChainTree, names: &[String]) {
        for h in names {
            assert_eq!(
                tree.subtree_work(h)
                    .map(|w| BigUint::parse_bytes(w.to_hex().as_bytes(), 16).unwrap()),
                weight(tree, h),
                "{h}"
            );
        }
        let mut path = vec![];
        let mut next = preferred(tree, tree.genesis_roots());
        while let Some(h) = next {
            next = preferred(tree, tree.children(&h));
            path.push(h);
        }
        assert_eq!(tree.canonical_path(), path);
    }
    let mut tree = ChainTree::empty(context("g"));
    let mut batches = vec![batch("g", None, 0, 1, true)];
    tree.apply_staged(&batches[0]).unwrap();
    let names: Vec<String> = (0..96).map(|h| format!("indexed-{h}")).collect();
    let mut parents = vec![];
    let mut heights = vec![];
    for h in 0..96 {
        let parent = if h == 0 {
            None
        } else {
            Some(if h < 32 { h - 1 } else { (h * 37 + 11) % h })
        };
        heights.push(parent.map_or(1, |p| heights[p] + 1));
        parents.push(parent);
    }
    let hashes: Vec<_> = std::iter::once(cid("g"))
        .chain(names.iter().map(|n| cid(n)))
        .collect();
    // Descendants arrive first, then several disconnected components graft.
    let order: Vec<_> = (0..96)
        .filter(|h| h % 3 == 0)
        .rev()
        .chain((0..96).filter(|h| h % 3 != 0).rev())
        .collect();
    for h in order {
        let parent = parents[h].map(|p| names[p].as_str()).unwrap_or("g");
        let b = batch(
            &names[h],
            Some(parent),
            heights[h],
            (h % 7 + 1) as u64,
            false,
        );
        tree.apply_staged(&b).unwrap();
        batches.push(b);
        check(&tree, &hashes);
    }
    for h in (0..96).step_by(7) {
        let parent = parents[h].map(|p| names[p].as_str()).unwrap_or("g");
        let b = batch(&names[h], Some(parent), heights[h], 100 + h as u64, false);
        tree.apply_staged(&b).unwrap();
        batches.push(b);
        check(&tree, &hashes);
    }
    for h in [10, 40, 65] {
        let b = exclude(&names[h]);
        tree.apply_staged(&b).unwrap();
        batches.push(b);
        check(&tree, &hashes);
    }
    for shift in [0, 1, 27, 83] {
        let mut reordered = batches.clone();
        reordered.rotate_left(shift);
        reordered.reverse();
        let recovered =
            ChainTree::restore(&reordered, tree.current_revision(), context("g")).unwrap();
        check(&recovered, &hashes);
        assert_eq!(tree.canonical_path(), recovered.canonical_path());
        for h in &hashes {
            assert_eq!(tree.subtree_work(h), recovered.subtree_work(h));
        }
    }
}
