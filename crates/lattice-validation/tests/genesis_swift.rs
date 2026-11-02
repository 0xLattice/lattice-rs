//! Actual pinned Swift validation observations; regenerate with scripts/genesis-swift-fixtures.py.
use lattice_cashew::{Header, MerkleDictionary, Volume, VolumeMerkleDictionary};
use lattice_primitives::*;
use lattice_validation::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
struct Case {
    name: String,
    cid: String,
    entries: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    swift: serde_json::Value,
}
fn genesis() -> Block {
    let empty = Volume::new(LatticeState::empty().unwrap()).unwrap();
    Block {
        version: 1,
        parent: None,
        transactions: Header::new(MerkleDictionary::default()).unwrap(),
        target: UInt256([255; 32]),
        next_target: UInt256([255; 32]),
        spec: Volume::new(ChainSpec {
            max_number_of_transactions_per_block: 100,
            max_state_growth: 100000,
            max_block_size: 1000000,
            premine: 1,
            target_block_time: 1000,
            initial_reward: 1024,
            halving_interval: 210000,
            half_life: 120,
            wasm_policies: vec![],
        })
        .unwrap(),
        parent_state: empty.clone(),
        prev_state: empty.clone(),
        post_state: empty,
        children: Header::new(BTreeMap::new()).unwrap(),
        height: 0,
        timestamp: 1000000000000,
        reward_recipient: None,
        nonce: 0,
    }
}
fn body(signers: &[&str], nonce: u64) -> TransactionBody {
    TransactionBody {
        account_actions: vec![],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: signers.iter().map(|x| x.to_string()).collect(),
        nonce,
        chain_path: vec!["Nexus".into()],
    }
}
fn build(
    name: &str,
    bodies: Vec<TransactionBody>,
    accounts: &[(&str, u64)],
    general: &[(&str, &str)],
    modify: impl FnOnce(&mut Block),
) -> Case {
    let mut block = genesis();
    let mut tree = MerkleDictionary::default();
    for (i, b) in bodies.into_iter().enumerate() {
        let tx = Volume::new(Transaction {
            signatures: BTreeMap::new(),
            body: Header::new(b).unwrap(),
        })
        .unwrap();
        tree = tree.inserting(&format!("tx{i}"), tx).unwrap();
    }
    block.transactions = Header::new(tree).unwrap();
    let mut state = LatticeState::empty().unwrap();
    let mut a = VolumeMerkleDictionary::default();
    for (key, value) in accounts {
        a = a.inserting(key, *value).unwrap();
    }
    state.account_state = Volume::new(a).unwrap();
    let mut g = VolumeMerkleDictionary::default();
    for (key, value) in general {
        g = g.inserting(key, value.to_string()).unwrap();
    }
    state.general_state = Volume::new(g).unwrap();
    block.post_state = Volume::new(state).unwrap();
    modify(&mut block);
    let root = Volume::new(block).unwrap();
    let mut entries = BTreeMap::new();
    for volume in lattice_cashew::volume::serialize_recursive(&root, &()).unwrap() {
        entries.extend(volume.entries);
    }
    Case {
        name: name.into(),
        cid: root.0.raw_cid,
        entries,
        swift: serde_json::Value::Null,
    }
}
fn spec(block: &mut Block, change: impl FnOnce(&mut ChainSpec)) {
    let mut spec = block.spec.0.node.as_deref().unwrap().clone();
    change(&mut spec);
    block.spec = Volume::new(spec).unwrap();
}
fn cases() -> Vec<Case> {
    let mut cases = vec![
        build("empty", vec![], &[], &[], |_| {}),
        build(
            "unsigned_declared_signer_nonce_zero",
            vec![body(&["alice"], 0)],
            &[("_nonce_alice", 0)],
            &[],
            |_| {},
        ),
        build(
            "first_signer_only",
            vec![body(&["alice", "bob"], 0)],
            &[("_nonce_alice", 0)],
            &[],
            |_| {},
        ),
        build(
            "unsigned_nonce_one",
            vec![body(&["alice"], 1)],
            &[],
            &[],
            |_| {},
        ),
        build(
            "unsigned_empty_signers_max_nonce",
            vec![body(&[], u64::MAX)],
            &[],
            &[],
            |_| {},
        ),
        build(
            "unordered_contiguous_nonces",
            vec![body(&["alice"], 1), body(&["alice"], 0)],
            &[("_nonce_alice", 1)],
            &[],
            |_| {},
        ),
        build(
            "duplicate_nonce",
            vec![body(&["alice"], 0), body(&["alice"], 0)],
            &[],
            &[],
            |_| {},
        ),
        build(
            "nonce_gap",
            vec![body(&["alice"], 0), body(&["alice"], 2)],
            &[],
            &[],
            |_| {},
        ),
        build(
            "reserved_cosigner",
            vec![body(&["alice", "_nonce_bob"], 0)],
            &[],
            &[],
            |_| {},
        ),
    ];
    let mut credit = body(&[], 0);
    credit.account_actions.push(AccountAction {
        owner: "alice".into(),
        delta: 100,
    });
    for (name, limit, premine) in [
        ("premine_credit", 100000, 1),
        ("premine_zero_rejects", 100000, 0),
        ("growth_exact_13", 13, 1),
        ("growth_below_13", 12, 1),
    ] {
        cases.push(build(
            name,
            vec![credit.clone()],
            &[("alice", 100)],
            &[],
            |b| {
                spec(b, |s| {
                    s.max_state_growth = limit;
                    s.premine = premine;
                })
            },
        ));
    }
    let mut mixed = credit.clone();
    mixed.actions.push(Action {
        key: "g".into(),
        old_value: None,
        new_value: Some("v".into()),
    });
    cases.push(build(
        "shared_empty_subtree_diff",
        vec![mixed.clone()],
        &[("alice", 100)],
        &[("g", "v")],
        |_| {},
    ));
    cases.push(build(
        "wrong_post_state",
        vec![mixed.clone()],
        &[],
        &[],
        |_| {},
    ));
    mixed.actions[0].old_value = Some("missing".into());
    cases.push(build("general_missing_old", vec![mixed], &[], &[], |_| {}));
    cases.push(build(
        "nonce_growth_exact",
        vec![body(&["alice"], 0)],
        &[("_nonce_alice", 0)],
        &[],
        |b| spec(b, |s| s.max_state_growth = 20),
    ));
    cases.push(build(
        "nonce_growth_exceeded",
        vec![body(&["alice"], 0)],
        &[("_nonce_alice", 0)],
        &[],
        |b| spec(b, |s| s.max_state_growth = 19),
    ));
    let mut wide = body(&[], 0);
    wide.account_actions = vec![
        AccountAction {
            owner: "alice".into(),
            delta: i64::MAX
        };
        2
    ];
    cases.push(build(
        "u64_balance_near_max",
        vec![wide.clone()],
        &[("alice", u64::MAX - 1)],
        &[],
        |b| spec(b, |s| s.initial_reward = u64::MAX),
    ));
    wide.account_actions.push(AccountAction {
        owner: "alice".into(),
        delta: 2,
    });
    cases.push(build(
        "u64_total_credit_overflow",
        vec![wide],
        &[],
        &[],
        |b| spec(b, |s| s.initial_reward = u64::MAX),
    ));
    let mut net = body(&["alice"], 0);
    net.account_actions = vec![
        AccountAction {
            owner: "alice".into(),
            delta: 5,
        },
        AccountAction {
            owner: "alice".into(),
            delta: -4,
        },
    ];
    cases.push(build(
        "premine_counts_gross_credits",
        vec![net.clone()],
        &[("alice", 1), ("_nonce_alice", 0)],
        &[],
        |b| spec(b, |s| s.initial_reward = 4),
    ));
    cases.push(build(
        "simultaneous_credit_debit",
        vec![net],
        &[("alice", 1), ("_nonce_alice", 0)],
        &[],
        |_| {},
    ));
    for (name, delta) in [
        ("zero_account_delta", 0),
        ("minimum_account_delta", i64::MIN),
        ("unfunded_debit", -1),
    ] {
        let mut bad = body(&["alice"], 0);
        bad.account_actions.push(AccountAction {
            owner: "alice".into(),
            delta,
        });
        cases.push(build(name, vec![bad], &[], &[], |_| {}));
    }
    let mut path = body(&[], 0);
    path.chain_path = vec!["other".into()];
    cases.push(build("wrong_chain_path", vec![path], &[], &[], |_| {}));
    let mut duplicate = body(&[], 0);
    duplicate.actions = vec![
        Action {
            key: "g".into(),
            old_value: None,
            new_value: Some("v".into())
        };
        2
    ];
    cases.push(build(
        "duplicate_general_key",
        vec![duplicate],
        &[],
        &[],
        |_| {},
    ));
    // Independent module bytes must be explicitly placed into the CAS: policy refs carry raw CIDs.
    for (name, result, transactions, entry) in [
        ("policy_accept", 1, true, "check"),
        ("policy_reject", 0, true, "check"),
        ("policy_empty_no_execution", 0, false, "check"),
        ("policy_missing_export", 1, false, "missing"),
    ] {
        let wasm=wat::parse_str(format!("(module (memory (export \"memory\") 1) (func (export \"lattice_alloc\") (param i32) (result i32) i32.const 0) (func (export \"check\") (param i32 i32) (result i32) i32.const {result}))")).unwrap();
        let module = Header::new(WasmPolicyModule { bytes: wasm }).unwrap();
        let mut case = build(
            name,
            if transactions {
                vec![body(&[], 0)]
            } else {
                vec![]
            },
            &[],
            &[],
            |b| {
                spec(b, |s| {
                    s.wasm_policies = vec![WasmPolicyRef {
                        module_cid: module.raw_cid.clone(),
                        source_cid: None,
                        abi_version: 1,
                        scope: PolicyScope::Transaction,
                        entrypoint: entry.into(),
                    }]
                })
            },
        );
        case.entries
            .insert(module.raw_cid.clone(), module.bytes(None).unwrap());
        cases.push(case);
    }
    cases
}
fn observe(case: &Case) -> serde_json::Value {
    match validate_root_genesis(
        &Volume::from_cid(&case.cid).unwrap(),
        &case.entries,
        i64::MAX,
    ) {
        Ok(result) => {
            serde_json::json!({"valid":true,"post_state":result.post_state().0.raw_cid,"diff":result.state_diff()})
        }
        Err(_) => serde_json::json!({"valid":false}),
    }
}
#[test]
fn actual_swift_genesis_validation_matches() {
    if let Ok(path) = std::env::var("LATTICE_GENESIS_EXPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&cases()).unwrap()).unwrap();
        return;
    }
    let rows: Vec<Case> = serde_json::from_str(include_str!("genesis-swift.json")).unwrap();
    assert_eq!(rows.len(), 31);
    for case in rows {
        if [
            "empty",
            "unsigned_declared_signer_nonce_zero",
            "first_signer_only",
            "unsigned_empty_signers_max_nonce",
            "unordered_contiguous_nonces",
            "premine_credit",
            "growth_exact_13",
            "shared_empty_subtree_diff",
            "nonce_growth_exact",
            "u64_balance_near_max",
            "simultaneous_credit_debit",
            "policy_accept",
            "policy_empty_no_execution",
        ]
        .contains(&case.name.as_str())
        {
            assert_eq!(
                case.swift["valid"], true,
                "intended positive fixture {}",
                case.name
            );
        }
        assert_eq!(observe(&case), case.swift, "{}", case.name);
    }
}
