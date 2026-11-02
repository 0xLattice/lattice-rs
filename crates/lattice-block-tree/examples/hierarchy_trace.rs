//! Production reducer traces. Input facts/verdicts are trusted fixture inputs;
//! this is not a cryptographic/network admission test. No expected outputs are
//! copied from a model: all snapshots come from ChainTree and anchor capabilities.
use lattice_block_tree::*;
use lattice_cashew::{Header, MerkleDictionary, Volume};
use lattice_primitives::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct B {
    ch: usize,
    name: &'static str,
    parent: Option<&'static str>,
    anchor: Option<&'static str>,
    child: Option<&'static str>,
    valid: bool,
}
fn cid(s: &str) -> String {
    lattice_cashew::cid_for_bytes(0x71, s.as_bytes())
}
fn state(s: Option<&str>) -> String {
    s.map(|s| cid(&format!("state/{s}")))
        .unwrap_or_else(|| content_id(&LatticeState::empty().unwrap()).unwrap())
}
fn path(ch: usize) -> Vec<String> {
    ["Nexus", "Alpha", "Beta"][..=ch]
        .iter()
        .map(|x| x.to_string())
        .collect()
}
fn fixture() -> Vec<B> {
    vec![
        B {
            ch: 0,
            name: "p0",
            parent: None,
            anchor: None,
            child: None,
            valid: true,
        },
        B {
            ch: 0,
            name: "p1",
            parent: Some("p0"),
            anchor: None,
            child: Some("c0"),
            valid: true,
        },
        B {
            ch: 0,
            name: "p2",
            parent: Some("p1"),
            anchor: None,
            child: None,
            valid: true,
        },
        B {
            ch: 0,
            name: "p3",
            parent: Some("p2"),
            anchor: None,
            child: Some("c1"),
            valid: true,
        },
        B {
            ch: 0,
            name: "p4",
            parent: Some("p3"),
            anchor: None,
            child: None,
            valid: true,
        },
        B {
            ch: 0,
            name: "px",
            parent: Some("p0"),
            anchor: None,
            child: None,
            valid: false,
        },
        B {
            ch: 1,
            name: "c0",
            parent: None,
            anchor: Some("p0"),
            child: None,
            valid: true,
        },
        B {
            ch: 1,
            name: "c1",
            parent: Some("c0"),
            anchor: Some("p2"),
            child: Some("g0"),
            valid: true,
        },
        B {
            ch: 1,
            name: "c2",
            parent: Some("c1"),
            anchor: Some("p2"),
            child: None,
            valid: true,
        },
        B {
            ch: 1,
            name: "cr",
            parent: None,
            anchor: None,
            child: None,
            valid: false,
        },
        B {
            ch: 1,
            name: "cx",
            parent: Some("c0"),
            anchor: Some("px"),
            child: None,
            valid: false,
        },
        B {
            ch: 2,
            name: "g0",
            parent: None,
            anchor: Some("c0"),
            child: None,
            valid: true,
        },
        B {
            ch: 2,
            name: "g1",
            parent: Some("g0"),
            anchor: Some("c1"),
            child: None,
            valid: true,
        },
    ]
}
fn block(b: &B, all: &[B]) -> Block {
    let height = height(b, all);
    Block {
        version: 1,
        parent: b.parent.map(|p| Volume(Header::from_cid(cid(p)).unwrap())),
        transactions: Header::new(MerkleDictionary::default()).unwrap(),
        target: UInt256([255; 32]),
        next_target: UInt256([255; 32]),
        spec: Volume(Header::from_cid(cid("spec")).unwrap()),
        parent_state: Volume(Header::from_cid(state(b.anchor)).unwrap()),
        prev_state: Volume(Header::from_cid(state(b.parent)).unwrap()),
        post_state: Volume(Header::from_cid(state(Some(b.name))).unwrap()),
        children: Header::new(BTreeMap::new()).unwrap(),
        height,
        timestamp: height as i64,
        reward_recipient: None,
        nonce: 0,
    }
}
fn height(b: &B, all: &[B]) -> u64 {
    b.parent
        .map(|p| 1 + height(all.iter().find(|b| b.name == p).unwrap(), all))
        .unwrap_or(0)
}
fn fact(b: &B, all: &[B], kind: &str, grind: &str, qty: u64) -> BlockImportBatch {
    let mut facts = vec![];
    if kind == "admit" {
        let children: BTreeMap<_, _> = b
            .child
            .map(|c| (["Alpha", "Beta"][b.ch].to_string(), cid(c)))
            .into_iter()
            .collect();
        facts.push(json!({"block":{"_0":{"blockHash":cid(b.name),"parentBlockHash":b.parent.map(cid),"blockHeight":height(b,all),"postStateCID":state(Some(b.name)),"prevStateCID":state(b.parent),"parentStateCID":state(b.anchor),"specCID":cid("spec"),"target":"ff","nextTarget":"ff","timestamp":height(b,all),"stateDiff":{"created":{},"replaced":{}},"childCommitments":children}}}));
    }
    match kind {
        "admit"|"observe"=>facts.push(json!({"work":{"_0":{"blockHash":cid(b.name),"contribution":{"id":cid(grind),"work":format!("0x{qty:x}")}}}})),
        "validate"=>facts.push(json!({"validation":{"_0":{"blockHash":cid(b.name)}}})),
        "exclude"=>facts.push(json!({"exclusion":{"_0":{"blockHash":cid(b.name)}}})),
        _=>panic!("fixture event")
    }
    serde_json::from_value(json!({"facts":facts})).unwrap()
}
fn continuity(trees: &[ChainTree], b: &B, all: &[B]) -> &'static str {
    let blk = block(b, all);
    let child = &trees[b.ch];
    let result = match child.verify_child_parent_anchor(&blk, None) {
        Err(ParentAnchorFailure::Required(link)) => {
            match trees[b.ch - 1].prove_parent_continuity(&link) {
                Some(e) => child.verify_child_parent_anchor(&blk, Some(&e)),
                None => return "held",
            }
        }
        other => other,
    };
    match result {
        Ok(_) => "proven",
        Err(ParentAnchorFailure::ProtocolInvalid) => "invalid",
        other => panic!("unexpected anchor result {other:?}"),
    }
}
fn snapshot(trees: &[ChainTree], all: &[B]) -> Value {
    let label = |cid: &str| {
        all.iter()
            .find(|b| self::cid(b.name) == cid)
            .map(|b| b.name)
    };
    let mut out = serde_json::Map::new();
    for (i, t) in trees.iter().enumerate() {
        let held: Vec<_> = all
            .iter()
            .filter(|b| b.ch == i && t.contains(&cid(b.name)))
            .collect();
        let weights: BTreeMap<_, _> = held
            .iter()
            .map(|b| {
                (
                    b.name,
                    num_bigint::BigUint::parse_bytes(
                        t.subtree_work(&cid(b.name)).unwrap().to_hex().as_bytes(),
                        16,
                    )
                    .unwrap()
                    .to_string(),
                )
            })
            .collect();
        let anchors: BTreeMap<_, _> = held
            .iter()
            .filter(|_| i > 0)
            .map(|b| (b.name, continuity(trees, b, all)))
            .collect();
        out.insert(["P","C","G"][i].into(),json!({"canonicalTip":label(t.canonical_tip()),"validatedTip":label(&t.heaviest_executed_tip(&BTreeSet::new())),"weights":weights,"continuity":anchors}));
    }
    Value::Object(out)
}
fn trace(mode: &str) -> Value {
    let all = fixture();
    let mut trees: Vec<_> = (0..3)
        .map(|i| {
            ChainTree::empty(
                ChainRuntimeContext::new(
                    path(i),
                    if i == 0 { Some(cid("p0")) } else { None }.as_deref(),
                )
                .unwrap(),
            )
        })
        .collect();
    trees[0].serve_runs("Alpha");
    trees[1].serve_runs("Beta");
    let order = if mode == "child-first" {
        vec![
            "c0", "c1", "c2", "cx", "cr", "g0", "g1", "p0", "p1", "p2", "p3", "p4", "px",
        ]
    } else {
        all.iter().map(|b| b.name).collect()
    };
    let mut schedule: Vec<(&str, &str, &str, u64)> =
        order.into_iter().map(|b| ("admit", b, b, 1)).collect();
    schedule.extend([
        ("exclude", "cr", "", 0),
        ("observe", "p2", "p4", 5),
        ("validate", "c0", "", 0),
        ("validate", "g0", "", 0),
        ("observe", "p4", "p4", 20),
        ("observe", "p4", "p4", 20),
        ("observe", "px", "px", 50),
        ("observe", "p3", "merged", 4),
        ("observe", "c1", "merged", 3),
        ("observe", "g0", "merged", 2),
        ("validate", "p0", "", 0),
        ("validate", "p1", "", 0),
        ("validate", "p2", "", 0),
        ("validate", "c0", "", 0),
        ("exclude", "cr", "", 0),
        ("exclude", "cx", "", 0),
        ("validate", "c1", "", 0),
        ("validate", "g0", "", 0),
        ("validate", "g1", "", 0),
        ("exclude", "px", "", 0),
        ("validate", "p3", "", 0),
        ("validate", "p4", "", 0),
        ("validate", "c2", "", 0),
    ]);
    let mut events = vec![];
    let mut grinds = vec![];
    for (kind, name, grind, qty) in schedule {
        let b = all.iter().find(|b| b.name == name).unwrap();
        let mut reason = None;
        let accepted = if kind == "validate" && b.ch > 0 && continuity(&trees, b, &all) == "held" {
            reason = Some("continuity");
            false
        } else {
            let before = trees[b.ch]
                .consensus_block(&cid(name))
                .and_then(|m| m.work_contributions.get(&cid(grind)).map(|w| *w.work()));
            let before_snapshot = snapshot(&trees, &all);
            let result = trees[b.ch].replay(&fact(b, &all, kind, grind, qty));
            if let Err(error) = &result {
                assert_eq!(*error, RestoreError::CorruptConsensusGraph);
                assert_eq!(
                    snapshot(&trees, &all),
                    before_snapshot,
                    "refused replay changed observable graph"
                );
                if kind == "observe" {
                    reason = Some("grindElsewhere");
                } else if kind == "exclude" && b.parent.is_none() {
                    reason = Some("rootExclusion");
                } else {
                    panic!("unexpected replay refusal {result:?}");
                }
                false
            } else if kind == "observe" {
                let after = trees[b.ch]
                    .consensus_block(&cid(name))
                    .and_then(|m| m.work_contributions.get(&cid(grind)).map(|w| *w.work()));
                before != after
            } else {
                true
            }
        };
        for i in 1..3 {
            let (up, down) = trees.split_at_mut(i);
            down[0]
                .apply_parent_run(&up[i - 1], ["Alpha", "Beta"][i - 1], None, &BTreeSet::new())
                .unwrap();
        }
        if kind == "admit" || kind == "observe" {
            grinds.push(json!({"label":grind,"ch":(["P","C","G"][b.ch]),"block":name,"qty":qty}));
        }
        events.push(json!({"kind":kind,"ch":(["P","C","G"][b.ch]),"block":name,"grind":grind,"qty":qty,"accepted":accepted,"reason":reason,"cause":if kind=="exclude" {Some("transition")}else{None},"headerExcluded":false,"after":snapshot(&trees,&all)}));
    }
    let mut sorted = all.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|b| cid(b.name).parse::<cid::Cid>().unwrap().to_bytes());
    let ranks: BTreeMap<_, _> = sorted
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name, i))
        .collect();
    let chains:BTreeMap<_,_>=(0..3).map(|i|(["P","C","G"][i],json!({"blocks":all.iter().filter(|b|b.ch==i).map(|b|json!({"label":b.name,"parent":b.parent,"cidRank":ranks[b.name],"valid":b.valid,"available":true})).collect::<Vec<_>>()}))).collect();
    let anchors: BTreeMap<_, _> = all.iter().map(|b| (b.name, b.anchor)).collect();
    let commits: Vec<_> = all
        .iter()
        .filter_map(|b| b.child.map(|c| (b.name, c)))
        .collect();
    json!({"chains":chains,"anchor":anchors,"commits":commits,"grinds":grinds,"events":events})
}
fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "parent-first".into());
    assert!(matches!(mode.as_str(), "parent-first" | "child-first"));
    println!("{}", trace(&mode));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedules_reach_same_executed_hierarchy_and_exercise_refusals() {
        let a = trace("parent-first");
        let b = trace("child-first");
        assert_eq!(
            a["events"].as_array().unwrap().last().unwrap()["after"],
            b["events"].as_array().unwrap().last().unwrap()["after"]
        );
        for t in [a, b] {
            let es = t["events"].as_array().unwrap();
            for reason in ["continuity", "rootExclusion", "grindElsewhere"] {
                assert!(
                    es.iter()
                        .any(|e| e["reason"] == reason && e["accepted"] == false)
                );
            }
            let end = &es.last().unwrap()["after"];
            assert_eq!(end["P"]["validatedTip"], "p4");
            assert_eq!(end["C"]["validatedTip"], "c2");
            assert_eq!(end["G"]["validatedTip"], "g1");
            assert!(es.iter().any(|e| e["after"]["P"]["canonicalTip"] == "px"
                && e["after"]["C"]["validatedTip"] == "c1"));
        }
    }
}
