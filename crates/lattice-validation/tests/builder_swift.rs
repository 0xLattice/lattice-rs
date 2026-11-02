//! Full-source pinned Swift BlockBuilder output observations (templates, not admission).
use lattice_cashew::{Header, Volume, VolumeMerkleDictionary};
use lattice_primitives::*;
use lattice_validation::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Serialize, Deserialize, Clone)]
struct Anchor {
    height: u64,
    timestamp: i64,
    target: UInt256,
}
#[derive(Serialize, Deserialize, Clone)]
struct Case {
    name: String,
    expected_success: bool,
    mode: String,
    spec: Vec<u8>,
    previous: Option<Vec<u8>>,
    parent_state: Option<String>,
    parent_chain: Option<Vec<u8>>,
    transactions: Vec<Vec<u8>>,
    children: BTreeMap<String, Vec<u8>>,
    timestamp: i64,
    target: Option<UInt256>,
    next_target: Option<UInt256>,
    nonce: u64,
    version: u16,
    anchor: Option<Anchor>,
    recipient: Option<String>,
    entries: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    swift: serde_json::Value,
}
fn body() -> TransactionBody {
    TransactionBody {
        account_actions: vec![],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![],
        nonce: 0,
        chain_path: vec!["Nexus".into()],
    }
}
fn add_tx(c: &mut Case, b: TransactionBody) {
    let h = Header::new(b).unwrap();
    c.entries.insert(h.raw_cid.clone(), h.bytes(None).unwrap());
    c.transactions.push(
        lattice_cashew::cbor::encode(&Transaction {
            signatures: BTreeMap::new(),
            body: h,
        })
        .unwrap(),
    );
}
fn store(c: &mut Case, b: &Block) {
    for v in lattice_cashew::volume::serialize_recursive(&b.post_state, &()).unwrap() {
        c.entries.extend(v.entries);
    }
}
fn change_spec(c: &mut Case, f: impl FnOnce(&mut ChainSpec)) {
    let mut s: ChainSpec = lattice_cashew::cbor::decode(&c.spec).unwrap();
    f(&mut s);
    c.spec = lattice_cashew::cbor::encode(&s).unwrap();
    if let Some(bytes) = &c.previous {
        let mut b: Block = lattice_cashew::cbor::decode(bytes).unwrap();
        b.spec = Volume::new(s).unwrap();
        c.entries
            .insert(b.spec.0.raw_cid.clone(), b.spec.0.bytes(None).unwrap());
        c.previous = Some(lattice_cashew::cbor::encode(&b).unwrap());
    }
}
fn cases() -> Vec<Case> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("genesis-swift.json")).unwrap();
    let base = &rows[0];
    let entries: BTreeMap<String, Vec<u8>> =
        serde_json::from_value(base["entries"].clone()).unwrap();
    let prev_bytes = entries[base["cid"].as_str().unwrap()].clone();
    let prev: Block = lattice_cashew::cbor::decode(&prev_bytes).unwrap();
    let empty = prev.prev_state.0.raw_cid.clone();
    let basic = Case {
        name: String::new(),
        expected_success: true,
        mode: "genesis".into(),
        spec: entries[&prev.spec.0.raw_cid].clone(),
        previous: None,
        parent_state: None,
        parent_chain: None,
        transactions: vec![],
        children: BTreeMap::new(),
        timestamp: 1000000002000,
        target: Some(UInt256([255; 32])),
        next_target: None,
        nonce: 7,
        version: 1,
        anchor: None,
        recipient: None,
        entries,
        swift: serde_json::Value::Null,
    };
    let mut out = vec![];
    let mut push = |name: &str, good: bool, modify: &dyn Fn(&mut Case)| {
        let mut c = basic.clone();
        c.name = name.into();
        c.expected_success = good;
        modify(&mut c);
        out.push(c);
    };
    push("genesis_empty", true, &|_| {});
    push("child_genesis_unavailable_parent_state", true, &|c| {
        c.mode = "child".into();
        c.parent_state = Some(content_id(&"unavailable vertical state").unwrap());
    });
    push("genesis_custom_header", true, &|c| {
        c.version = 9;
        c.target = Some(UInt256([0; 32]));
        c.timestamp = -123;
        c.nonce = u64::MAX;
    });
    push("genesis_unsigned_nonce", true, &|c| {
        let mut b = body();
        b.signers = vec!["alice".into()];
        add_tx(c, b);
    });
    push("genesis_nonce_gap", false, &|c| {
        let mut b = body();
        b.signers = vec!["alice".into()];
        b.nonce = 1;
        add_tx(c, b);
    });
    push("genesis_credits_beyond_premine_template", true, &|c| {
        change_spec(c, |s| s.premine = 0);
        let mut b = body();
        b.account_actions.push(AccountAction {
            owner: "alice".into(),
            delta: 100,
        });
        add_tx(c, b);
    });
    push("genesis_growth_exact", true, &|c| {
        change_spec(c, |s| s.max_state_growth = 13);
        let mut b = body();
        b.account_actions.push(AccountAction {
            owner: "alice".into(),
            delta: 100,
        });
        add_tx(c, b);
    });
    push("genesis_growth_exceeded", false, &|c| {
        change_spec(c, |s| s.max_state_growth = 12);
        let mut b = body();
        b.account_actions.push(AccountAction {
            owner: "alice".into(),
            delta: 100,
        });
        add_tx(c, b);
    });
    push("invalid_state_atom", false, &|c| {
        let mut b = body();
        b.signers = vec!["é".into()];
        add_tx(c, b);
    });
    let child = content_id(&"unavailable child block").unwrap();
    push("cid_only_child_commitment", true, &|c| {
        let mut b = body();
        b.actions.push(Action {
            key: "child/A".into(),
            old_value: None,
            new_value: Some(format!("{child}@{empty}")),
        });
        add_tx(c, b);
    });
    push("wrong_commitment_entering_state", false, &|c| {
        let mut b = body();
        b.actions.push(Action {
            key: "child/A".into(),
            old_value: None,
            new_value: Some(format!("{child}@wrong")),
        });
        add_tx(c, b);
    });
    push("explicit_child", true, &|c| {
        c.children.insert("A".into(), prev_bytes.clone());
    });
    push("invalid_explicit_directory", false, &|c| {
        c.children.insert("A/B".into(), prev_bytes.clone());
    });
    push("conflicting_child_commitment", false, &|c| {
        c.children.insert("A".into(), prev_bytes.clone());
        let mut b = body();
        b.actions.push(Action {
            key: "child/A".into(),
            old_value: None,
            new_value: Some(format!("{child}@{empty}")),
        });
        add_tx(c, b);
    });
    for name in [
        "next_empty",
        "next_no_state_fetch",
        "next_unsigned_nonce",
        "next_explicit_targets",
        "next_coinbase",
        "next_policy_not_loaded",
        "next_parent_carrier",
        "next_bad_child_missing_state",
    ] {
        push(name, true, &|c| {
            c.mode = "next".into();
            c.previous = Some(prev_bytes.clone());
            c.target = None;
            match name {
                "next_no_state_fetch" => {
                    c.entries.remove(&empty);
                }
                "next_unsigned_nonce" => {
                    let mut b = body();
                    b.signers = vec!["alice".into()];
                    add_tx(c, b);
                }
                "next_explicit_targets" => {
                    c.target = Some(UInt256([1; 32]));
                    c.next_target = Some(UInt256([2; 32]));
                }
                "next_coinbase" => c.recipient = Some(content_id(&"miner").unwrap()),
                "next_policy_not_loaded" => change_spec(c, |s| {
                    s.wasm_policies = vec![WasmPolicyRef {
                        module_cid: content_id(&"absent module").unwrap(),
                        source_cid: None,
                        abi_version: 99,
                        scope: PolicyScope::Transaction,
                        entrypoint: "missing".into(),
                    }]
                }),
                "next_parent_carrier" => {
                    let mut p = prev.clone();
                    p.prev_state =
                        Volume::from_cid(content_id(&"carrier entering").unwrap()).unwrap();
                    c.parent_chain = Some(lattice_cashew::cbor::encode(&p).unwrap());
                }
                "next_bad_child_missing_state" => {
                    c.expected_success = false;
                    c.entries.remove(&empty);
                    c.children.insert("A/B".into(), prev_bytes.clone());
                    let mut b = body();
                    b.signers = vec!["alice".into()];
                    add_tx(c, b);
                }
                _ => {}
            }
        });
    }
    for name in [
        "height_two_anchor_in_parent",
        "height_three_supplied_anchor",
        "height_three_walk_anchor",
        "height_three_missing_anchor",
        "height_overflow",
    ] {
        push(name, true, &|c| {
            c.mode = "next".into();
            c.target = None;
            let mut p = prev.clone();
            p.height = 1;
            p.timestamp += 1000;
            p.parent = Some(Volume::from_cid(base["cid"].as_str().unwrap()).unwrap());
            if name == "height_overflow" {
                p.height = u64::MAX;
                c.expected_success = false;
            }
            if name.starts_with("height_three") {
                let link = Volume::new(p.clone()).unwrap();
                c.entries
                    .insert(link.0.raw_cid.clone(), link.0.bytes(None).unwrap());
                p.height = 2;
                p.timestamp += 1000;
                p.parent = Some(link.removing_node());
                c.timestamp += 1000;
                if name == "height_three_supplied_anchor" {
                    c.anchor = Some(Anchor {
                        height: 1,
                        timestamp: 1000000001000,
                        target: UInt256([127; 32]),
                    });
                }
                if name == "height_three_missing_anchor" {
                    p.parent = None;
                    c.expected_success = false;
                }
            }
            c.previous = Some(lattice_cashew::cbor::encode(&p).unwrap());
        });
    }
    push("child_genesis_premine", true, &|c| {
        c.mode = "child".into();
        c.parent_state = Some(content_id(&"unavailable vertical state").unwrap());
        let mut b = body();
        b.account_actions.push(AccountAction {
            owner: "alice".into(),
            delta: 100,
        });
        add_tx(c, b);
    });
    push("explicit_next_target_skips_missing_anchor", true, &|c| {
        c.mode = "next".into();
        let mut p = prev.clone();
        p.height = 2;
        p.parent = None;
        c.previous = Some(lattice_cashew::cbor::encode(&p).unwrap());
        c.next_target = Some(UInt256([3; 32]));
    });
    for name in ["unsigned_transfer_fee_template", "template_invalid_fee"] {
        push(name, name == "unsigned_transfer_fee_template", &|c| {
            let funded = rows.iter().find(|r| r["name"] == "premine_credit").unwrap();
            let source: BTreeMap<String, Vec<u8>> =
                serde_json::from_value(funded["entries"].clone()).unwrap();
            c.previous = Some(source[funded["cid"].as_str().unwrap()].clone());
            c.entries.extend(source);
            c.mode = "next".into();
            c.target = None;
            c.recipient = Some(content_id(&"miner").unwrap());
            let mut b = body();
            b.signers = vec!["alice".into()];
            b.account_actions = vec![
                AccountAction {
                    owner: "alice".into(),
                    delta: -10,
                },
                AccountAction {
                    owner: "bob".into(),
                    delta: if name == "template_invalid_fee" {
                        11
                    } else {
                        9
                    },
                },
            ];
            add_tx(c, b);
        });
    }
    // Spending a stored deposit is template construction; no parent receipt is fetched.
    push("withdrawal_template_without_parent_receipt", true, &|c| {
        c.mode = "next".into();
        c.target = None;
        let mut p = prev.clone();
        let mut s = LatticeState::empty().unwrap();
        s.deposit_state = Volume::new(
            VolumeMerkleDictionary::default()
                .inserting("alice/5/1", 10u64)
                .unwrap(),
        )
        .unwrap();
        p.post_state = Volume::new(s).unwrap();
        store(c, &p);
        c.previous = Some(lattice_cashew::cbor::encode(&p).unwrap());
        let mut b = body();
        b.withdrawal_actions.push(WithdrawalAction {
            withdrawer: "bob".into(),
            nonce: 1,
            demander: "alice".into(),
            amount_demanded: 5,
            amount_withdrawn: 10,
        });
        add_tx(c, b);
    });
    out
}
fn observe(c: &Case) -> serde_json::Value {
    let spec: ChainSpec = lattice_cashew::cbor::decode(&c.spec).unwrap();
    let txs: Vec<Transaction> = c
        .transactions
        .iter()
        .map(|b| lattice_cashew::cbor::decode(b).unwrap())
        .collect();
    let children: BTreeMap<String, Block> = c
        .children
        .iter()
        .map(|(k, b)| (k.clone(), lattice_cashew::cbor::decode(b).unwrap()))
        .collect();
    let result = if c.mode == "next" {
        let prev: Block = lattice_cashew::cbor::decode(c.previous.as_ref().unwrap()).unwrap();
        let carrier: Option<Block> = c
            .parent_chain
            .as_ref()
            .map(|b| lattice_cashew::cbor::decode(b).unwrap());
        BlockTemplate {
            previous: &prev,
            transactions: &txs,
            children: &children,
            parent_chain_block: carrier.as_ref(),
            timestamp: c.timestamp,
            target: c.target,
            next_target: c.next_target,
            nonce: c.nonce,
            difficulty_anchor: c.anchor.as_ref().map(|a| DifficultyAnchor {
                block_height: a.height,
                timestamp: a.timestamp,
                target: a.target,
            }),
            reward_recipient: c.recipient.as_deref(),
        }
        .build(&c.entries)
    } else {
        let parent = c
            .parent_state
            .as_ref()
            .map(|cid| Volume::from_cid(cid).unwrap());
        GenesisTemplate {
            spec: &spec,
            transactions: &txs,
            children: &children,
            parent_state: parent.as_ref(),
            timestamp: c.timestamp,
            target: c.target.unwrap(),
            nonce: c.nonce,
            version: c.version,
        }
        .build(&c.entries)
    };
    match result {
        Ok(v) => {
            serde_json::json!({"built":true,"block":content_id(&v.block).unwrap(),"post_state":v.block.post_state.0.raw_cid,"materialized":v.materialized_post_state.0.node.is_some(),"diff":v.state_diff})
        }
        Err(e) => {
            if c.name == "next_bad_child_missing_state" {
                assert!(matches!(
                    e,
                    BlockBuilderError::Execution(ExecutionError::Content(_))
                ));
            }
            serde_json::json!({"built":false})
        }
    }
}
#[test]
fn actual_swift_builder_matches() {
    if let Ok(path) = std::env::var("LATTICE_BUILDER_EXPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&cases()).unwrap()).unwrap();
        return;
    }
    let rows: Vec<Case> = serde_json::from_str(include_str!("builder-swift.json")).unwrap();
    assert_eq!(rows.len(), 32);
    for c in rows {
        assert_eq!(
            c.swift["built"], c.expected_success,
            "intended outcome {}",
            c.name
        );
        assert_eq!(observe(&c), c.swift, "{}", c.name);
    }
}
