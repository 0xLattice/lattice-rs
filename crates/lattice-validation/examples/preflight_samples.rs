use lattice_cashew::{Header, MerkleDictionary, Volume, VolumeMerkleDictionary};
use lattice_primitives::*;
use lattice_validation::*;
use std::collections::BTreeMap;
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
            premine: 0,
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
        timestamp: 1_000_000_000_000,
        reward_recipient: None,
        nonce: 0,
    }
}
fn body(owner: &str, nonce: u64) -> TransactionBody {
    TransactionBody {
        account_actions: vec![AccountAction {
            owner: owner.into(),
            delta: -3,
        }],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![owner.into()],
        nonce,
        chain_path: vec!["Nexus".into()],
    }
}
fn signed(body: TransactionBody, private: &str, public: &str) -> Transaction {
    let signature = crypto::sign(&body.signing_preimage().unwrap(), private).unwrap();
    Transaction {
        body: Header::new(body).unwrap(),
        signatures: BTreeMap::from([(public.into(), signature)]),
    }
}
fn funded(owner: &str, balance: u64, nonce: Option<u64>) -> Block {
    let mut block = genesis();
    let mut accounts = VolumeMerkleDictionary::default()
        .inserting(owner, balance)
        .unwrap();
    if let Some(n) = nonce {
        accounts = accounts.inserting(&format!("_nonce_{owner}"), n).unwrap();
    }
    block.post_state = Volume::new(LatticeState {
        account_state: Volume::new(accounts).unwrap(),
        ..LatticeState::empty().unwrap()
    })
    .unwrap();
    block
}
fn main() {
    let private = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    let public = "ed01d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    let owner = crypto::create_address(public).unwrap();
    let mut rows = vec![];
    for index in 0..51 {
        let mut block = funded(&owner, 10, None);
        let mut body = body(&owner, 0);
        let mut checks = true;
        let mut parent: Option<Volume<LatticeState>> = None;
        let mut extra = BTreeMap::new();
        let mut remove = vec![];
        match index {
            1 => body.nonce = 1,
            2 => block = funded(&owner, 1, None),
            3 => block = funded(&owner, 10, Some(0)),
            4 => block = funded(&owner, 10, Some(u64::MAX)),
            5 => body.account_actions[0].delta = 1,
            6 => body.chain_path.push("wrong".into()),
            7 => body.actions.push(Action {
                key: "key".into(),
                old_value: Some("missing".into()),
                new_value: Some("new".into()),
            }),
            8 => body.actions.push(Action {
                key: "key".into(),
                old_value: None,
                new_value: Some("new".into()),
            }),
            9 => block.timestamp = i64::MAX,
            10 => {
                block.timestamp = i64::MAX;
                checks = false;
            }
            11 => {
                let mut spec = block.spec.0.node.as_deref().unwrap().clone();
                spec.max_state_growth = 0;
                block.spec = Volume::new(spec).unwrap();
                body.actions.push(Action {
                    key: "key".into(),
                    old_value: None,
                    new_value: Some("long".into()),
                });
            }
            _ => {}
        }
        if (18..=27).contains(&index) || (30..=32).contains(&index) {
            let accepts = index == 18;
            let module=Header::new(WasmPolicyModule{bytes:wat::parse_str(format!(r#"(module (memory (export "memory") 1) (func (export "lattice_alloc") (param i32) (result i32) i32.const 0) (func (export "check") (param i32 i32) (result i32) i32.const {}))"#,if accepts {1}else{0})).unwrap()}).unwrap();
            let mut spec = block.spec.0.node.as_deref().unwrap().clone();
            spec.wasm_policies = vec![WasmPolicyRef {
                module_cid: module.raw_cid.clone(),
                source_cid: None,
                scope: PolicyScope::Transaction,
                abi_version: 1,
                entrypoint: "check".into(),
            }];
            block.spec = Volume::new(spec).unwrap();
            if !(23..=25).contains(&index) {
                extra.insert(module.raw_cid.clone(), module.bytes(None).unwrap());
            }
            if index == 20 || index == 22 {
                body.nonce = 1;
            }
            if index == 21 || index == 22 || index == 31 {
                checks = false;
            }
            if index == 24 {
                let mut state = block.post_state.0.node.as_deref().unwrap().clone();
                state.account_state = Volume::new(
                    state
                        .account_state
                        .0
                        .node
                        .as_deref()
                        .unwrap()
                        .inserting(&format!("_nonce_{owner}"), 0)
                        .unwrap(),
                )
                .unwrap();
                block.post_state = Volume::new(state).unwrap();
            }
            if index == 25 || index == 27 {
                body.account_actions[0].delta = -100;
            }
            if index == 26 {
                remove.push(block.post_state.0.raw_cid.clone());
            }
        }
        if (28..=39).contains(&index) || (48..=49).contains(&index) {
            body.chain_path.push("Child".into());
            let nonce = if index == 48 {
                u128::from(u64::MAX)
            } else if index == 49 {
                u128::from(u64::MAX - 1)
            } else {
                0
            };
            body.withdrawal_actions.push(WithdrawalAction {
                withdrawer: owner.clone(),
                demander: owner.clone(),
                amount_demanded: 1,
                amount_withdrawn: 1,
                nonce,
            });
            let deposit = DepositKey {
                demander: owner.clone(),
                amount_demanded: 1,
                nonce,
            }
            .to_string();
            let receipt = ReceiptKey {
                directory: "Child".into(),
                demander: owner.clone(),
                amount_demanded: 1,
                nonce,
            }
            .storage_key();
            let mut state = block.post_state.0.node.as_deref().unwrap().clone();
            if index != 34 {
                state.deposit_state = Volume::new(
                    VolumeMerkleDictionary::default()
                        .inserting(
                            &deposit,
                            if index == 35 {
                                0
                            } else if index == 36 {
                                2
                            } else {
                                1
                            },
                        )
                        .unwrap(),
                )
                .unwrap();
            }
            if index == 38 || index == 39 {
                remove.push(state.deposit_state.0.raw_cid.clone());
            }
            block.post_state = Volume::new(state).unwrap();
            let mut p = LatticeState::empty().unwrap();
            if index != 32 && index != 39 {
                p.receipt_state = Volume::new(
                    VolumeMerkleDictionary::default()
                        .inserting(
                            &receipt,
                            if index == 33 || index == 38 {
                                "contradiction".into()
                            } else {
                                owner.clone()
                            },
                        )
                        .unwrap(),
                )
                .unwrap();
            }
            parent = Some(Volume::new(p).unwrap());
            if (29..=31).contains(&index) {
                parent = None;
            }
            if index == 37 {
                remove.push(parent.as_ref().unwrap().0.raw_cid.clone());
            }
        }
        if (40..=43).contains(&index) {
            let mut state = block.post_state.0.node.as_deref().unwrap().clone();
            state.general_state = Volume::new(
                VolumeMerkleDictionary::default()
                    .inserting("key", "old".to_string())
                    .unwrap(),
            )
            .unwrap();
            block.post_state = Volume::new(state).unwrap();
            body.actions.push(Action {
                key: "key".into(),
                old_value: if index == 41 {
                    None
                } else {
                    Some(if index == 42 { "wrong" } else { "old" }.into())
                },
                new_value: Some("new".into()),
            });
            if index == 40 {
                body.actions.push(body.actions[0].clone());
            }
        }
        if index == 44 {
            body.account_actions[0].owner = format!("_nonce_{owner}");
        }
        if index == 45 || index == 46 {
            let mut spec = block.spec.0.node.as_deref().unwrap().clone();
            spec.half_life = 0;
            block.spec = Volume::new(spec).unwrap();
            if index == 46 {
                remove.push(block.post_state.0.raw_cid.clone());
            }
        }
        if index == 47 {
            let mut state = block.post_state.0.node.as_deref().unwrap().clone();
            let wrong = Volume::new(
                VolumeMerkleDictionary::default()
                    .inserting(&owner, "wrong".to_string())
                    .unwrap(),
            )
            .unwrap();
            extra.extend(
                lattice_cashew::volume::serialize_recursive(&wrong, &())
                    .unwrap()
                    .into_iter()
                    .flat_map(|v| v.entries),
            );
            state.account_state = Volume::from_cid(wrong.0.raw_cid).unwrap();
            block.post_state = Volume::new(state).unwrap();
        }
        let path = if index == 6 {
            vec!["Nexus".into()]
        } else {
            body.chain_path.clone()
        };
        let mut tx = signed(body, private, public);
        if index == 12 {
            tx.signatures.clear();
        }
        let tip = Volume::new(block.clone()).unwrap();
        let tx_header = Header::new(tx.clone()).unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> =
            lattice_cashew::volume::serialize_recursive(&tip, &())
                .unwrap()
                .into_iter()
                .chain(lattice_cashew::volume::serialize_recursive(&tx_header, &()).unwrap())
                .flat_map(|v| v.entries)
                .collect();
        entries.extend(extra);
        if let Some(p) = &parent {
            entries.extend(
                lattice_cashew::volume::serialize_recursive(p, &())
                    .unwrap()
                    .into_iter()
                    .flat_map(|v| v.entries),
            );
        }
        for cid in remove {
            entries.remove(&cid);
        }
        let missing = match index {
            13 => Some(block.spec.0.raw_cid.clone()),
            14 => Some(block.post_state.0.raw_cid.clone()),
            15 => Some(tx.body.raw_cid.clone()),
            16 => Some(
                block
                    .post_state
                    .0
                    .node
                    .as_deref()
                    .unwrap()
                    .account_state
                    .0
                    .raw_cid
                    .clone(),
            ),
            _ => None,
        };
        if let Some(cid) = missing {
            entries.remove(&cid);
        }
        if index == 17 {
            tx.signatures.clear();
            entries.remove(&block.spec.0.raw_cid);
            let changed = Header::new(tx.clone()).unwrap();
            entries.insert(changed.raw_cid.clone(), changed.bytes(None).unwrap());
        }
        let tx_cid = content_id(&tx).unwrap();
        // Resolve from serialized evidence, avoiding retained materialized nodes.
        let resolved_tx: Transaction = lattice_cashew::cbor::decode(&entries[&tx_cid]).unwrap();
        let tip = Volume::<Block>::from_cid(tip.0.raw_cid).unwrap();
        let unresolved_parent = parent
            .as_ref()
            .map(|p| Volume::<LatticeState>::from_cid(p.0.raw_cid.clone()).unwrap());
        let result = TransactionPreflight {
            transaction: &resolved_tx,
            tip: &tip,
            chain_path: &path,
            parent_state: unresolved_parent.as_ref(),
            now_milliseconds: 0,
            checks_policies: checks,
        }
        .classify(&entries)
        .unwrap();
        rows.push(serde_json::json!({"name":(["ready", "future_nonce", "insufficient_balance", "stale_nonce", "exhausted_nonce", "negative_surplus", "wrong_path", "update_absent", "insert_new", "timestamp_overflow", "timestamp_overflow_policy_disabled", "growth_floor", "missing_signature", "missing_spec", "missing_state", "missing_body", "missing_accounts", "signature_before_missing_spec", "policy_accept", "policy_reject_ready", "policy_reject_future", "policy_disabled_ready", "policy_disabled_future", "missing_policy_ready", "stale_nonce_before_missing_policy", "bad_balance_before_missing_policy", "missing_state_before_reject_policy", "bad_balance_before_reject_policy", "withdrawal_ready", "withdrawal_missing_parent", "reject_policy_missing_parent", "disabled_policy_missing_parent", "missing_receipt_before_reject_policy", "withdrawal_wrong_receipt", "withdrawal_missing_deposit", "withdrawal_spent_deposit", "withdrawal_wrong_amount", "withdrawal_missing_parent_cas", "contradicting_receipt_missing_deposit_cas", "missing_receipt_missing_deposit_cas", "duplicate_state_actions", "insert_existing", "update_wrong_old", "update_existing", "reserved_account", "invalid_spec", "invalid_spec_before_missing_state", "wrong_account_value_type", "withdrawal_nonce_u64_max", "withdrawal_nonce_u64_max_minus_one", "duplicate_cbor_state_key"][index]),"tip":tip.0.raw_cid,"transaction":tx_cid,"entries":entries,"checksPolicies":checks,"chainPath":if index==6 {vec!["Nexus".to_string()]}else{path},"parentState":parent.map(|p|p.0.raw_cid),"expected":format!("{:?}",result.disposition()).to_lowercase()}));
    }
    println!("{}", serde_json::to_string_pretty(&rows).unwrap());
}
