use lattice_primitives::*;
use lattice_validation::*;
use std::collections::BTreeMap;

fn body(signer: &str, nonce: u64) -> TransactionBody {
    TransactionBody {
        account_actions: vec![],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![signer.into()],
        nonce,
        chain_path: vec!["Nexus".into()],
    }
}
fn execute(
    state: &ResolvedState,
    actions: &TransitionActions,
    bodies: &[TransactionBody],
) -> Result<TransitionResult, TransitionError> {
    execute_block(state, &BTreeMap::new(), "Child", actions, bodies, 0, None)
}
#[test]
fn contiguous_nonces_are_order_independent_and_only_first_signer_advances() {
    let mut co_signed = body("alice", 1);
    co_signed.signers.push("bob".into());
    let result = execute(
        &ResolvedState::default(),
        &TransitionActions::default(),
        &[co_signed, body("alice", 0)],
    )
    .unwrap();
    assert_eq!(
        result.state.accounts,
        BTreeMap::from([("_nonce_alice".into(), 1)])
    );
    assert_eq!(result.account_growth, 20);
    assert_eq!(
        execute(
            &result.state,
            &TransitionActions::default(),
            &[body("alice", 1)]
        ),
        Err(TransitionError::NonceGap)
    );
    assert!(
        execute(
            &result.state,
            &TransitionActions::default(),
            &[body("alice", 2)]
        )
        .is_ok()
    );
    assert_eq!(
        execute(
            &ResolvedState::default(),
            &TransitionActions::default(),
            &[body("alice", 0), body("alice", 0)]
        ),
        Err(TransitionError::NonceGap)
    );
}
#[test]
fn late_failure_is_atomic_and_old_values_compare_utf8_bytes() {
    let state = ResolvedState {
        accounts: BTreeMap::from([("alice".into(), 10)]),
        general: BTreeMap::from([("key".into(), "é".into())]),
        ..Default::default()
    };
    let snapshot = state.clone();
    let actions = TransitionActions {
        accounts: vec![AccountAction {
            owner: "alice".into(),
            delta: -5,
        }],
        general: vec![Action {
            key: "key".into(),
            old_value: Some("e\u{301}".into()),
            new_value: Some("x".into()),
        }],
        ..Default::default()
    };
    assert_eq!(
        execute(&state, &actions, &[body("alice", 0)]),
        Err(TransitionError::Conflict)
    );
    assert_eq!(state, snapshot);
}
#[test]
fn withdrawal_keeps_permanent_marker_and_requires_matching_parent_receipt() {
    let key = DepositKey {
        demander: "alice".into(),
        amount_demanded: 7,
        nonce: 1,
    };
    let receipt = ReceiptKey {
        directory: "Child".into(),
        demander: "alice".into(),
        amount_demanded: 7,
        nonce: 1,
    };
    let state = ResolvedState {
        deposits: BTreeMap::from([(key.to_string(), 9)]),
        ..Default::default()
    };
    let mut actions = TransitionActions {
        withdrawals: vec![WithdrawalAction {
            withdrawer: "bob".into(),
            nonce: 1,
            demander: "alice".into(),
            amount_demanded: 7,
            amount_withdrawn: 9,
        }],
        ..Default::default()
    };
    assert_eq!(
        execute(&state, &actions, &[]),
        Err(TransitionError::ParentReceipt)
    );
    let parents = BTreeMap::from([(receipt.storage_key(), "bob".into())]);
    let result = execute_block(&state, &parents, "Child", &actions, &[], 0, None).unwrap();
    assert_eq!(result.state.deposits[&key.to_string()], 0);
    assert_eq!(
        execute_block(&result.state, &parents, "Child", &actions, &[], 0, None),
        Err(TransitionError::Conflict)
    );
    actions.deposits.push(DepositAction {
        demander: "alice".into(),
        amount_demanded: 7,
        nonce: 1,
        amount_deposited: 9,
    });
    assert_eq!(
        execute_block(&state, &parents, "Child", &actions, &[], 0, None),
        Err(TransitionError::Conflict)
    );
}
#[test]
fn account_growth_counts_insert_delete_and_nonce_entries() {
    let state = ResolvedState {
        accounts: BTreeMap::from([("alice".into(), 10)]),
        ..Default::default()
    };
    let actions = TransitionActions {
        accounts: vec![
            AccountAction {
                owner: "alice".into(),
                delta: -10,
            },
            AccountAction {
                owner: "bob".into(),
                delta: 10,
            },
        ],
        ..Default::default()
    };
    let result = execute(&state, &actions, &[body("alice", 0)]).unwrap();
    assert!(!result.state.accounts.contains_key("alice"));
    assert_eq!(result.state.accounts["bob"], 10);
    assert_eq!(result.account_growth, -13 + 11 + 20);
}
#[test]
fn coinbase_burn_has_no_int64_bound_but_present_recipient_does() {
    let recipient = crypto::create_address("test").unwrap();
    let actions = TransitionActions::default();
    assert_eq!(coinbase_credit(&actions, u64::MAX, None), Ok(None));
    assert_eq!(
        coinbase_credit(&actions, u64::MAX, Some(&recipient)),
        Err(TransitionError::CoinbaseOverflow)
    );
    assert_eq!(
        coinbase_credit(&actions, 0, Some("bad")),
        Err(TransitionError::InvalidRecipient)
    );
    assert_eq!(coinbase_credit(&actions, 0, Some(&recipient)), Ok(None));
    let creation = TransitionActions {
        accounts: vec![AccountAction {
            owner: "alice".into(),
            delta: 1,
        }],
        ..Default::default()
    };
    assert_eq!(
        coinbase_credit(&creation, 100, None),
        Err(TransitionError::FeeRule)
    );
}
#[test]
fn reserved_nonce_keys_cannot_be_spent_or_used_as_cosigners() {
    let state = ResolvedState {
        accounts: BTreeMap::from([("_nonce_alice".into(), 3)]),
        ..Default::default()
    };
    let actions = TransitionActions {
        accounts: vec![AccountAction {
            owner: "_nonce_alice".into(),
            delta: -1,
        }],
        ..Default::default()
    };
    assert_eq!(
        execute(&state, &actions, &[]),
        Err(TransitionError::Conflict)
    );
    let mut tx = body("alice", 4);
    tx.signers.push("_nonce_bob".into());
    assert_eq!(
        execute(&state, &TransitionActions::default(), &[tx]),
        Err(TransitionError::Conflict)
    );
}

#[test]
fn authenticated_transition_matches_full_root_and_preserves_untouched_subtrees() {
    use lattice_cashew::{Header, Volume, VolumeMerkleDictionary};
    let accounts = VolumeMerkleDictionary::default()
        .inserting("alice", 10u64)
        .unwrap()
        .inserting("alex", 9)
        .unwrap()
        .inserting("zed", 99)
        .unwrap();
    let mut state = LatticeState::empty().unwrap();
    state.account_state = Volume::new(accounts).unwrap();
    let before = Volume::new(state.clone()).unwrap();
    let volumes = lattice_cashew::volume::serialize_recursive(&before, &()).unwrap();
    let source: BTreeMap<_, _> = volumes.into_iter().flat_map(|v| v.entries).collect();
    let root = Volume(Header::<LatticeState>::from_cid(&before.0.raw_cid).unwrap());
    let actions = TransitionActions {
        accounts: vec![
            AccountAction {
                owner: "alice".into(),
                delta: -10,
            },
            AccountAction {
                owner: "bob".into(),
                delta: 10,
            },
        ],
        ..Default::default()
    };
    let txs = [body("alice", 0)];
    let executed = AuthenticatedBlockExecution {
        previous: &root,
        parent: &root,
        directory: "Child",
        actions: &actions,
        transaction_bodies: &txs,
        reward: 0,
        recipient: None,
    }
    .execute(&source)
    .unwrap();
    let expected_accounts = state
        .account_state
        .0
        .node
        .as_ref()
        .unwrap()
        .deleting("alice")
        .unwrap()
        .inserting("bob", 10)
        .unwrap()
        .inserting("_nonce_alice", 0)
        .unwrap();
    state.account_state = Volume::new(expected_accounts).unwrap();
    assert_eq!(
        executed.state.0.raw_cid,
        Volume::new(state).unwrap().0.raw_cid
    );
    assert_eq!(executed.account_growth, 18);
    let mut corrupt = source.clone();
    corrupt.insert(root.0.raw_cid.clone(), vec![0]);
    assert!(
        AuthenticatedBlockExecution {
            previous: &root,
            parent: &root,
            directory: "Child",
            actions: &actions,
            transaction_bodies: &txs,
            reward: 0,
            recipient: None
        }
        .execute(&corrupt)
        .is_err()
    );
}
#[test]
fn no_op_does_not_fetch_untouched_substate_volumes() {
    use lattice_cashew::{Header, Volume};
    let state = Volume::new(LatticeState::empty().unwrap()).unwrap();
    let source = lattice_cashew::volume::serialize_boundary(&state, &())
        .unwrap()
        .entries;
    assert_eq!(source.len(), 1);
    let root = Volume(Header::<LatticeState>::from_cid(&state.0.raw_cid).unwrap());
    let result = AuthenticatedBlockExecution {
        previous: &root,
        parent: &root,
        directory: "Child",
        actions: &TransitionActions::default(),
        transaction_bodies: &[],
        reward: 0,
        recipient: None,
    }
    .execute(&source)
    .unwrap();
    assert_eq!(result.state.0.raw_cid, root.0.raw_cid);
}

#[test]
fn transaction_admission_binds_body_chain_path_nonce_and_exact_signer_coverage() {
    use lattice_cashew::Header;
    let (private, key) = crypto::generate_key_pair();
    let address = crypto::create_address(&key).unwrap();
    let mut signed_body = body(&address, 0);
    let header = Header::new(signed_body.clone()).unwrap();
    let signature = crypto::sign(&signed_body.signing_preimage().unwrap(), &private).unwrap();
    let mut tx = Transaction::new(BTreeMap::from([(key.clone(), signature)]), header);
    let source = BTreeMap::<String, Vec<u8>>::new();
    assert!(validate_transaction(&tx, &source, false).unwrap().is_some());
    signed_body.nonce = 1;
    tx.body = Header::new(signed_body.clone()).unwrap();
    assert!(validate_transaction(&tx, &source, false).unwrap().is_none());
    let signature = crypto::sign(&signed_body.signing_preimage().unwrap(), &private).unwrap();
    tx.signatures.insert(key.clone(), signature);
    assert!(validate_transaction(&tx, &source, false).unwrap().is_some());
    signed_body.signers.push("extra".into());
    tx.body = Header::new(signed_body.clone()).unwrap();
    tx.signatures.insert(
        key,
        crypto::sign(&signed_body.signing_preimage().unwrap(), &private).unwrap(),
    );
    assert!(validate_transaction(&tx, &source, false).unwrap().is_none());
    // Genesis intentionally has no signature requirement, but forbids swaps.
    tx.signatures.clear();
    assert!(validate_transaction(&tx, &source, true).unwrap().is_some());
    signed_body.deposit_actions.push(DepositAction {
        nonce: 0,
        demander: address,
        amount_demanded: 1,
        amount_deposited: 1,
    });
    tx.body = Header::new(signed_body).unwrap();
    assert!(validate_transaction(&tx, &source, true).unwrap().is_none());
}

#[test]
fn genesis_can_mint_premine_without_non_genesis_fee_rule() {
    use lattice_cashew::Volume;
    let previous = Volume::new(LatticeState::empty().unwrap()).unwrap();
    let spec = ChainSpec {
        max_number_of_transactions_per_block: 10,
        max_state_growth: 10000,
        max_block_size: 100000,
        premine: 2,
        target_block_time: 1000,
        initial_reward: 100,
        halving_interval: 100,
        half_life: 10,
        wasm_policies: vec![],
    };
    let mut tx = body("alice", 0);
    tx.account_actions = vec![AccountAction {
        owner: "alice".into(),
        delta: 200,
    }];
    let bodies = [tx];
    let actions = TransitionActions::from_bodies(&bodies);
    let source = BTreeMap::<String, Vec<u8>>::new();
    let execution = AuthenticatedBlockExecution {
        previous: &previous,
        parent: &previous,
        directory: "Nexus",
        actions: &actions,
        transaction_bodies: &bodies,
        reward: 0,
        recipient: None,
    };
    assert!(execution.execute(&source).is_err());
    let result = execution.execute_genesis(&source, &spec).unwrap();
    assert_eq!(
        result
            .state
            .0
            .node
            .as_ref()
            .unwrap()
            .account_state
            .0
            .node
            .as_ref()
            .unwrap()
            .get("alice")
            .unwrap(),
        Some(&200)
    );
    assert_eq!(result.diff.replaced[&previous.0.raw_cid], 1);
    assert_eq!(result.diff.created[&result.state.0.raw_cid], 1);
    assert!(result.account_growth > 0);
}
