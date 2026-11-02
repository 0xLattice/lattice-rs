//! Full-source pinned Swift ordinary-root transition observations.
use lattice_cashew::{Header, MerkleDictionary, Volume, VolumeMerkleDictionary};
use lattice_primitives::*;
use lattice_validation::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const PRIVATE: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const PUBLIC: &str = "ed01d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
#[derive(Serialize, Deserialize)]
struct Case {
    name: String,
    cid: String,
    parent: String,
    entries: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    swift: serde_json::Value,
}
fn body(nonce: u64) -> TransactionBody {
    TransactionBody {
        account_actions: vec![],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![crypto::create_address(PUBLIC).unwrap()],
        nonce,
        chain_path: vec!["Nexus".into()],
    }
}
fn state(accounts: &[(String, u64)], general: &[(&str, &str)]) -> LatticeState {
    let mut s = LatticeState::empty().unwrap();
    let mut a = VolumeMerkleDictionary::default();
    let mut g = VolumeMerkleDictionary::default();
    for (k, v) in accounts {
        a = a.inserting(k, *v).unwrap();
    }
    for (k, v) in general {
        g = g.inserting(k, v.to_string()).unwrap();
    }
    s.account_state = Volume::new(a).unwrap();
    s.general_state = Volume::new(g).unwrap();
    s
}
fn transactions(
    bodies: Vec<TransactionBody>,
    signed: bool,
) -> Header<MerkleDictionary<Volume<Transaction>>> {
    let mut tree = MerkleDictionary::default();
    for (i, b) in bodies.into_iter().enumerate() {
        let sig = if signed {
            BTreeMap::from([(
                PUBLIC.into(),
                crypto::sign(&b.signing_preimage().unwrap(), PRIVATE).unwrap(),
            )])
        } else {
            BTreeMap::new()
        };
        tree = tree
            .inserting(
                &format!("tx{i}"),
                Volume::new(Transaction {
                    signatures: sig,
                    body: Header::new(b).unwrap(),
                })
                .unwrap(),
            )
            .unwrap();
    }
    Header::new(tree).unwrap()
}
#[allow(clippy::too_many_arguments)]
fn build(
    name: &str,
    bodies: Vec<TransactionBody>,
    initial: &[(String, u64)],
    old_general: &[(&str, &str)],
    final_accounts: &[(String, u64)],
    final_general: &[(&str, &str)],
    spec_change: impl FnOnce(&mut ChainSpec),
    change: impl FnOnce(&mut Block),
) -> Case {
    let empty = Volume::new(LatticeState::empty().unwrap()).unwrap();
    let mut spec = ChainSpec {
        max_number_of_transactions_per_block: 100,
        max_state_growth: 100000,
        max_block_size: 1000000,
        premine: 1,
        target_block_time: 1000,
        initial_reward: 1024,
        halving_interval: 210000,
        half_life: 120,
        wasm_policies: vec![],
    };
    spec_change(&mut spec);
    let mut funding = body(0);
    funding.signers.clear();
    funding.account_actions = initial
        .iter()
        .map(|(owner, amount)| AccountAction {
            owner: owner.clone(),
            delta: *amount as i64,
        })
        .collect();
    funding.actions = old_general
        .iter()
        .map(|(k, v)| Action {
            key: k.to_string(),
            old_value: None,
            new_value: Some(v.to_string()),
        })
        .collect();
    let parent = Volume::new(Block {
        version: 1,
        parent: None,
        transactions: transactions(
            if initial.is_empty() && old_general.is_empty() {
                vec![]
            } else {
                vec![funding]
            },
            false,
        ),
        target: UInt256([255; 32]),
        next_target: UInt256([255; 32]),
        spec: Volume::new(spec).unwrap(),
        parent_state: empty.clone(),
        prev_state: empty.clone(),
        post_state: Volume::new(state(initial, old_general)).unwrap(),
        children: Header::new(BTreeMap::new()).unwrap(),
        height: 0,
        timestamp: 1000000000000,
        reward_recipient: None,
        nonce: 0,
    })
    .unwrap();
    let p = parent.0.node.as_deref().unwrap();
    let mut block = Block {
        version: 1,
        parent: Some(parent.clone()),
        transactions: transactions(bodies, true),
        target: p.target,
        next_target: p.next_target,
        spec: p.spec.clone(),
        parent_state: empty,
        prev_state: p.post_state.clone(),
        post_state: Volume::new(state(final_accounts, final_general)).unwrap(),
        children: Header::new(BTreeMap::new()).unwrap(),
        height: 1,
        timestamp: p.timestamp + 1000,
        reward_recipient: None,
        nonce: 0,
    };
    change(&mut block);
    let root = Volume::new(block).unwrap();
    let mut entries = BTreeMap::new();
    for v in lattice_cashew::volume::serialize_recursive(&root, &()).unwrap() {
        entries.extend(v.entries);
    }
    Case {
        name: name.into(),
        cid: root.0.raw_cid,
        parent: parent.0.raw_cid,
        entries,
        swift: serde_json::Value::Null,
    }
}
fn cases() -> Vec<Case> {
    let address = crypto::create_address(PUBLIC).unwrap();
    let nonce = format!("_nonce_{address}");
    let miner = content_id(&"miner").unwrap();
    let mut rows = vec![
        build(
            "empty_burn_reward",
            vec![],
            &[],
            &[],
            &[],
            &[],
            |_| {},
            |_| {},
        ),
        build(
            "coinbase_without_transactions",
            vec![],
            &[],
            &[],
            &[(miner.clone(), 1024)],
            &[],
            |_| {},
            |b| b.reward_recipient = Some(miner.clone()),
        ),
        build(
            "signed_nonce_zero",
            vec![body(0)],
            &[],
            &[],
            &[(nonce.clone(), 0)],
            &[],
            |_| {},
            |_| {},
        ),
        build(
            "signed_nonce_one_rejected",
            vec![body(1)],
            &[],
            &[],
            &[],
            &[],
            |_| {},
            |_| {},
        ),
        build(
            "contiguous_reordered_nonces",
            vec![body(1), body(0)],
            &[],
            &[],
            &[(nonce.clone(), 1)],
            &[],
            |_| {},
            |_| {},
        ),
        build(
            "nonce_gap",
            vec![body(2), body(0)],
            &[],
            &[],
            &[],
            &[],
            |_| {},
            |_| {},
        ),
        build(
            "nonce_duplicate",
            vec![body(0), body(0)],
            &[],
            &[],
            &[],
            &[],
            |_| {},
            |_| {},
        ),
    ];
    let mut transfer = body(0);
    transfer.account_actions = vec![
        AccountAction {
            owner: address.clone(),
            delta: -10,
        },
        AccountAction {
            owner: "bob".into(),
            delta: 9,
        },
    ];
    let initial = vec![(address.clone(), 1000)];
    let paid = vec![
        (address.clone(), 990),
        ("bob".into(), 9),
        (nonce.clone(), 0),
        (miner.clone(), 1025),
    ];
    rows.push(build(
        "fee_and_reward",
        vec![transfer.clone()],
        &initial,
        &[],
        &paid,
        &[],
        |_| {},
        |b| b.reward_recipient = Some(miner.clone()),
    ));
    rows.push(build(
        "fee_and_reward_burn",
        vec![transfer.clone()],
        &initial,
        &[],
        &paid[..3],
        &[],
        |_| {},
        |_| {},
    ));
    rows.push(build(
        "wrong_post_state",
        vec![transfer.clone()],
        &initial,
        &[],
        &initial,
        &[],
        |_| {},
        |_| {},
    ));
    let mut mint = body(0);
    mint.account_actions.push(AccountAction {
        owner: "bob".into(),
        delta: 1,
    });
    rows.push(build(
        "unfunded_mint",
        vec![mint],
        &[],
        &[],
        &[],
        &[],
        |_| {},
        |_| {},
    ));
    rows.push(build(
        "insufficient_balance",
        vec![transfer.clone()],
        &[],
        &[],
        &[],
        &[],
        |_| {},
        |_| {},
    ));
    let mut general = body(0);
    general.actions.push(Action {
        key: "g".into(),
        old_value: None,
        new_value: Some("v".into()),
    });
    rows.push(build(
        "general_insert",
        vec![general.clone()],
        &[],
        &[],
        &[(nonce.clone(), 0)],
        &[("g", "v")],
        |_| {},
        |_| {},
    ));
    general.actions[0].old_value = Some("old".into());
    rows.push(build(
        "general_update",
        vec![general.clone()],
        &[],
        &[("g", "old")],
        &[(nonce.clone(), 0)],
        &[("g", "v")],
        |_| {},
        |_| {},
    ));
    rows.push(build(
        "general_wrong_old",
        vec![general.clone()],
        &[],
        &[("g", "other")],
        &[],
        &[],
        |_| {},
        |_| {},
    ));
    general.actions[0].new_value = None;
    rows.push(build(
        "general_delete",
        vec![general],
        &[],
        &[("g", "old")],
        &[(nonce.clone(), 0)],
        &[],
        |_| {},
        |_| {},
    ));
    for (name, limit) in [
        ("nonce_growth_exact", nonce.len() as i64 + 8),
        ("nonce_growth_exceeded", nonce.len() as i64 + 7),
    ] {
        rows.push(build(
            name,
            vec![body(0)],
            &[],
            &[],
            &[(nonce.clone(), 0)],
            &[],
            |s| s.max_state_growth = limit,
            |_| {},
        ));
    }
    rows.push(build(
        "transaction_limit",
        vec![body(0), body(1)],
        &[],
        &[],
        &[(nonce.clone(), 1)],
        &[],
        |s| s.max_number_of_transactions_per_block = 1,
        |_| {},
    ));
    rows.push(build(
        "coinbase_i64_overflow",
        vec![],
        &[],
        &[],
        &[],
        &[],
        |s| s.initial_reward = u64::MAX,
        |b| b.reward_recipient = Some(miner.clone()),
    ));
    rows.push(build(
        "huge_reward_burn",
        vec![],
        &[],
        &[],
        &[],
        &[],
        |s| s.initial_reward = u64::MAX,
        |_| {},
    ));
    for (name, mutation) in [
        ("unsigned_transaction", 0),
        ("bad_signature", 1),
        ("wrong_chain_path", 2),
        ("wrong_height", 3),
        ("nonincreasing_time", 4),
        ("wrong_next_target", 5),
    ] {
        rows.push(build(
            name,
            vec![body(0)],
            &[],
            &[],
            &[(nonce.clone(), 0)],
            &[],
            |_| {},
            |b| match mutation {
                0 => b.transactions = transactions(vec![body(0)], false),
                1 => {
                    let mut tx = Transaction {
                        signatures: BTreeMap::from([(PUBLIC.into(), "00".repeat(64))]),
                        body: Header::new(body(0)).unwrap(),
                    };
                    tx.signatures.insert(PUBLIC.into(), "00".repeat(64));
                    b.transactions = Header::new(
                        MerkleDictionary::default()
                            .inserting("tx0", Volume::new(tx).unwrap())
                            .unwrap(),
                    )
                    .unwrap();
                }
                2 => {
                    let mut t = body(0);
                    t.chain_path = vec!["Other".into()];
                    b.transactions = transactions(vec![t], true)
                }
                3 => b.height = 2,
                4 => b.timestamp -= 1000,
                _ => b.next_target = UInt256([0; 32]),
            },
        ));
    }
    for (name, result, has_tx) in [
        ("policy_accept", 1, true),
        ("policy_reject", 0, true),
        ("policy_empty_skips_execution", 0, false),
    ] {
        let bytes=wat::parse_str(format!("(module (memory (export \"memory\") 1) (func (export \"lattice_alloc\") (param i32) (result i32) i32.const 0) (func (export \"check\") (param i32 i32) (result i32) i32.const {result}))")).unwrap();
        let module = Header::new(WasmPolicyModule { bytes }).unwrap();
        let final_accounts = if has_tx {
            vec![(nonce.clone(), 0)]
        } else {
            vec![]
        };
        let mut row = build(
            name,
            if has_tx { vec![body(0)] } else { vec![] },
            &[],
            &[],
            &final_accounts,
            &[],
            |s| {
                s.wasm_policies = vec![WasmPolicyRef {
                    module_cid: module.raw_cid.clone(),
                    source_cid: None,
                    abi_version: 1,
                    scope: PolicyScope::Transaction,
                    entrypoint: "check".into(),
                }]
            },
            |_| {},
        );
        row.entries
            .insert(module.raw_cid.clone(), module.bytes(None).unwrap());
        rows.push(row);
    }
    let mut mint = body(0);
    mint.account_actions.push(AccountAction {
        owner: "bob".into(),
        delta: 1,
    });
    let mut row = build(
        "fee_invalid_before_missing_state",
        vec![mint],
        &initial,
        &[],
        &initial,
        &[],
        |_| {},
        |_| {},
    );
    let parent: Block = lattice_cashew::cbor::decode(&row.entries[&row.parent]).unwrap();
    row.entries.remove(&parent.post_state.0.raw_cid);
    rows.push(row);
    rows
}
#[test]
fn actual_swift_root_transition_matches() {
    if let Ok(path) = std::env::var("LATTICE_TRANSITION_EXPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&cases()).unwrap()).unwrap();
        return;
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("transition-swift.json")).unwrap();
    assert_eq!(cases.len(), 31);
    for case in cases {
        if [
            "empty_burn_reward",
            "coinbase_without_transactions",
            "signed_nonce_zero",
            "contiguous_reordered_nonces",
            "fee_and_reward",
            "fee_and_reward_burn",
            "general_insert",
            "general_update",
            "general_delete",
            "nonce_growth_exact",
            "huge_reward_burn",
            "policy_accept",
            "policy_empty_skips_execution",
        ]
        .contains(&case.name.as_str())
        {
            assert_eq!(
                case.swift["valid"], true,
                "intended positive fixture {}",
                case.name
            );
        }
        if case.name == "fee_invalid_before_missing_state" {
            let error = validate_root_transition(
                &Volume::from_cid(&case.cid).unwrap(),
                &case.entries,
                i64::MAX,
                None,
            )
            .unwrap_err();
            assert!(
                !error.is_retriable(),
                "fee failure must precede missing state: {error}"
            );
        }
        let actual = match validate_root_transition(
            &Volume::from_cid(&case.cid).unwrap(),
            &case.entries,
            i64::MAX,
            None,
        ) {
            Ok(v) => {
                serde_json::json!({"valid":true,"post_state":v.post_state().0.raw_cid,"diff":v.state_diff()})
            }
            Err(_) => serde_json::json!({"valid":false}),
        };
        assert_eq!(actual, case.swift, "{}", case.name);
    }
}
