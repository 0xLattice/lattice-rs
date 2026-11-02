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
fn no_content() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::new()
}
#[test]
fn authentic_swift_genesis_produces_bound_materialized_capability() {
    let header = Volume::new(genesis()).unwrap();
    let verified = validate_root_genesis(&header, &no_content(), i64::MAX).unwrap();
    assert_eq!(
        verified.block_cid(),
        "bafyreieq7pwonjmp3azvp3mkgqmaknqhtzvt3aw634zgulbvhhvfcvu3ia"
    );
    assert_eq!(
        verified.post_state().0.raw_cid,
        header.0.node.as_ref().unwrap().post_state.0.raw_cid
    );
    assert!(verified.post_state().0.node.is_some());
    let mut forged = header;
    forged.0.raw_cid = content_id(&genesis().spec).unwrap();
    assert!(validate_root_genesis(&forged, &no_content(), i64::MAX).is_err());
}
#[test]
fn rejection_and_retriable_conditions_are_distinct() {
    let block = genesis();
    let header = Volume::new(block.clone()).unwrap();
    assert!(matches!(
        validate_root_genesis(&header, &no_content(), block.timestamp - 1),
        Err(BlockValidationFailure::NotYetValid)
    ));
    let missing = Volume(header.0.removing_node());
    assert!(
        validate_root_genesis(&missing, &no_content(), i64::MAX)
            .unwrap_err()
            .is_retriable()
    );
    for mutation in 0..5 {
        let mut bad = block.clone();
        match mutation {
            0 => bad.target = UInt256([0; 32]),
            1 => bad.height = 1,
            2 => bad.reward_recipient = Some(content_id(&"miner").unwrap()),
            3 => {
                bad.parent_state = Volume::new(LatticeState {
                    account_state: Volume::new(
                        VolumeMerkleDictionary::default()
                            .inserting("x", 1u64)
                            .unwrap(),
                    )
                    .unwrap(),
                    ..LatticeState::empty().unwrap()
                })
                .unwrap()
            }
            _ => {
                bad.post_state = Volume::new(LatticeState {
                    account_state: Volume::new(
                        VolumeMerkleDictionary::default()
                            .inserting("x", 1u64)
                            .unwrap(),
                    )
                    .unwrap(),
                    ..LatticeState::empty().unwrap()
                })
                .unwrap()
            }
        }
        let error =
            validate_root_genesis(&Volume::new(bad).unwrap(), &no_content(), i64::MAX).unwrap_err();
        assert!(!error.is_retriable(), "mutation {mutation}: {error}");
    }
}
#[test]
fn premine_transition_is_recomputed_and_growth_limit_enforced() {
    let mut block = genesis();
    let mut spec = block.spec.0.node.as_ref().unwrap().as_ref().clone();
    spec.premine = 1;
    block.spec = Volume::new(spec.clone()).unwrap();
    let body = TransactionBody {
        account_actions: vec![AccountAction {
            owner: "alice".into(),
            delta: 100,
        }],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![],
        nonce: 0,
        chain_path: vec!["Nexus".into()],
    };
    let transaction = Volume::new(Transaction {
        signatures: BTreeMap::new(),
        body: Header::new(body).unwrap(),
    })
    .unwrap();
    block.transactions = Header::new(
        MerkleDictionary::default()
            .inserting("tx", transaction)
            .unwrap(),
    )
    .unwrap();
    let mut state = LatticeState::empty().unwrap();
    state.account_state = Volume::new(
        VolumeMerkleDictionary::default()
            .inserting("alice", 100u64)
            .unwrap(),
    )
    .unwrap();
    block.post_state = Volume::new(state).unwrap();
    assert!(
        validate_root_genesis(
            &Volume::new(block.clone()).unwrap(),
            &no_content(),
            i64::MAX
        )
        .is_ok()
    );
    spec.max_state_growth = 12; // alice's five UTF-8 bytes plus an eight-byte balance.
    block.spec = Volume::new(spec).unwrap();
    assert!(matches!(
        validate_root_genesis(&Volume::new(block).unwrap(), &no_content(), i64::MAX),
        Err(BlockValidationFailure::Invalid("exact state growth"))
    ));
}
#[test]
fn source_errors_cannot_forge_invalidity_and_invalid_spec_precedes_tx_fetch() {
    struct Provider;
    impl lattice_cashew::ContentSource for Provider {
        fn fetch(&self, _: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
            Err(lattice_cashew::Error::NonCanonical)
        }
    }
    let mut block = genesis();
    block.transactions = block.transactions.removing_node();
    assert!(
        validate_root_genesis(&Volume::new(block.clone()).unwrap(), &Provider, i64::MAX)
            .unwrap_err()
            .is_retriable()
    );
    let mut spec = block.spec.0.node.as_ref().unwrap().as_ref().clone();
    spec.max_block_size = 0;
    block.spec = Volume::new(spec).unwrap();
    assert!(matches!(
        validate_root_genesis(&Volume::new(block).unwrap(), &Provider, i64::MAX),
        Err(BlockValidationFailure::Invalid("chain spec"))
    ));
}

#[test]
fn genesis_refuses_committed_invalid_program_even_without_transactions() {
    let mut block = genesis();
    let module = Header::new(WasmPolicyModule {
        bytes: b"invalid wasm".to_vec(),
    })
    .unwrap();
    let source = BTreeMap::from([(module.raw_cid.clone(), module.bytes(None).unwrap())]);
    let mut spec = block.spec.0.node.as_ref().unwrap().as_ref().clone();
    spec.wasm_policies.push(WasmPolicyRef {
        module_cid: module.raw_cid,
        source_cid: None,
        abi_version: 1,
        scope: PolicyScope::Action,
        entrypoint: "check".into(),
    });
    block.spec = Volume::new(spec).unwrap();
    assert!(matches!(
        validate_root_genesis(&Volume::new(block).unwrap(), &source, i64::MAX),
        Err(BlockValidationFailure::Invalid("configured policy"))
    ));
}

#[test]
fn content_helper_preserves_transaction_header_identity() {
    // Full admission rejects this codec; the lower-level content helper should
    // still preserve authenticated metadata instead of minting a new root CID.
    let mut block = genesis();
    block.transactions = Header::with_codec(MerkleDictionary::default(), 0x55).unwrap();
    let before = content_id(&block).unwrap();
    let content = resolve_block_content(&block, &no_content(), i64::MAX).unwrap();
    assert_eq!(content_id(&content.block).unwrap(), before);
}

#[test]
fn genesis_preserves_invalid_vs_unavailable_check_order() {
    let mut block = genesis();
    let body = TransactionBody {
        account_actions: vec![],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![],
        nonce: 0,
        chain_path: vec!["Wrong".into()],
    };
    let tx = Volume::new(Transaction {
        signatures: BTreeMap::new(),
        body: Header::new(body).unwrap(),
    })
    .unwrap();
    block.transactions =
        Header::new(MerkleDictionary::default().inserting("tx", tx).unwrap()).unwrap();
    block.children = block.children.removing_node();
    assert!(matches!(
        validate_root_genesis(&Volume::new(block).unwrap(), &no_content(), i64::MAX),
        Err(BlockValidationFailure::Invalid("transaction chain path"))
    ));

    let mut block = genesis();
    let mut spec = block.spec.0.node.as_ref().unwrap().as_ref().clone();
    spec.max_block_size = 100; // transaction-only index fits; whole block does not.
    spec.wasm_policies.push(WasmPolicyRef {
        module_cid: content_id(&WasmPolicyModule { bytes: vec![] }).unwrap(),
        source_cid: None,
        abi_version: 1,
        scope: PolicyScope::Action,
        entrypoint: "check".into(),
    });
    block.spec = Volume::new(spec).unwrap();
    let failure =
        validate_root_genesis(&Volume::new(block).unwrap(), &no_content(), i64::MAX).unwrap_err();
    assert!(
        failure.is_retriable(),
        "missing policy must precede whole-block size: {failure}"
    );
}

fn successor(parent: &BlockHeader, anchor: Option<DifficultyAnchor>) -> BlockHeader {
    let previous = parent.0.node.as_deref().unwrap();
    let mut block = previous.clone();
    block.parent = Some(parent.clone());
    block.height += 1;
    block.timestamp += 1000;
    block.prev_state = previous.post_state.clone();
    block.target = previous.next_target;
    let anchor = immediate_difficulty_anchor(&block, &HeaderLinkageParent::from(previous))
        .or(anchor)
        .unwrap();
    block.next_target = block.spec.0.node.as_deref().unwrap().asert_target(
        anchor.target,
        anchor.timestamp,
        anchor.block_height,
        block.timestamp,
        block.height,
    );
    Volume::new(block).unwrap()
}

#[test]
fn root_successors_bind_predecessor_and_anchor_without_copying_history() {
    let genesis = Volume::new(genesis()).unwrap();
    let first = successor(&genesis, None);
    let verified = validate_root_transition(&first, &no_content(), i64::MAX, None).unwrap();
    assert_eq!(verified.parent_cid(), genesis.0.raw_cid);
    assert_eq!(verified.block_cid(), first.0.raw_cid);
    assert_eq!(verified.inherited_anchor(), None);
    assert!(
        verified
            .header()
            .0
            .node
            .as_ref()
            .unwrap()
            .parent
            .as_ref()
            .unwrap()
            .0
            .node
            .is_none()
    );
    assert_eq!(
        verified.post_state().0.raw_cid,
        genesis.0.node.as_ref().unwrap().post_state.0.raw_cid
    );
    let anchor = verified.resolved_anchor();
    let second = successor(&first, None);
    assert_eq!(
        validate_root_transition(&second, &no_content(), i64::MAX, None)
            .unwrap()
            .resolved_anchor(),
        anchor
    );
    let third = successor(&second, Some(anchor));
    assert!(matches!(
        validate_root_transition(&third, &no_content(), i64::MAX, None),
        Err(BlockValidationFailure::AnchorUnavailable)
    ));
    let verified = validate_root_transition(&third, &no_content(), i64::MAX, Some(anchor)).unwrap();
    assert_eq!(verified.inherited_anchor(), Some(anchor));
}

#[test]
fn root_successors_reject_linkage_and_commitment_mutations() {
    let parent = Volume::new(genesis()).unwrap();
    let first = successor(&parent, None);
    for mutation in 0..6 {
        let mut bad = first.0.node.as_deref().unwrap().clone();
        match mutation {
            0 => bad.height += 1,
            1 => bad.timestamp = parent.0.node.as_ref().unwrap().timestamp,
            2 => bad.version += 1,
            3 => bad.next_target = UInt256([0; 32]),
            4 => bad.parent = None,
            _ => {
                bad.post_state = Volume::new(LatticeState {
                    account_state: Volume::new(
                        VolumeMerkleDictionary::default()
                            .inserting("unearned", 1u64)
                            .unwrap(),
                    )
                    .unwrap(),
                    ..LatticeState::empty().unwrap()
                })
                .unwrap()
            }
        }
        let error =
            validate_root_transition(&Volume::new(bad).unwrap(), &no_content(), i64::MAX, None)
                .unwrap_err();
        assert!(!error.is_retriable(), "mutation {mutation}: {error}");
    }
    let timestamp = first.0.node.as_ref().unwrap().timestamp;
    assert!(matches!(
        validate_root_transition(&first, &no_content(), timestamp - 1, None),
        Err(BlockValidationFailure::NotYetValid)
    ));
    let missing = Volume(first.0.removing_node());
    assert!(
        validate_root_transition(&missing, &no_content(), i64::MAX, None)
            .unwrap_err()
            .is_retriable()
    );
}

#[test]
fn root_successor_recomputes_coinbase_instead_of_trusting_post_state() {
    let parent = Volume::new(genesis()).unwrap();
    let mut block = successor(&parent, None).0.node.as_deref().unwrap().clone();
    let recipient = content_id(&"miner").unwrap();
    block.reward_recipient = Some(recipient.clone());
    assert!(matches!(
        validate_root_transition(
            &Volume::new(block.clone()).unwrap(),
            &no_content(),
            i64::MAX,
            None
        ),
        Err(BlockValidationFailure::Invalid("post-state commitment"))
    ));
    let reward = block
        .spec
        .0
        .node
        .as_ref()
        .unwrap()
        .reward_at_block(block.height);
    block.post_state = Volume::new(LatticeState {
        account_state: Volume::new(
            VolumeMerkleDictionary::default()
                .inserting(&recipient, reward)
                .unwrap(),
        )
        .unwrap(),
        ..LatticeState::empty().unwrap()
    })
    .unwrap();
    let verified = validate_root_transition(
        &Volume::new(block.clone()).unwrap(),
        &no_content(),
        i64::MAX,
        None,
    )
    .unwrap();
    assert_eq!(verified.post_state().0.raw_cid, block.post_state.0.raw_cid);
}

#[test]
fn child_genesis_binds_path_and_real_parent_state_without_claiming_own_work() {
    let mut child = genesis();
    let parent_state = Volume::new(LatticeState {
        account_state: Volume::new(
            VolumeMerkleDictionary::default()
                .inserting("parent", 1u64)
                .unwrap(),
        )
        .unwrap(),
        ..LatticeState::empty().unwrap()
    })
    .unwrap();
    child.parent_state = parent_state.clone();
    // A child is secured by a carrier proof, not by its own block hash.
    let mut target = [0u8; 32];
    target[31] = 1;
    child.target = UInt256(target);
    child.next_target = child.target;
    assert!(!child.has_valid_pow());
    let header = Volume::new(child.clone()).unwrap();
    let path = vec!["Nexus".into(), "Alpha".into()];
    let result = validate_child_genesis(&header, &no_content(), i64::MAX, &path).unwrap();
    assert_eq!(result.block_cid(), header.0.raw_cid);
    assert_eq!(result.chain_path(), path);
    assert_eq!(result.parent_path(), &["Nexus"]);
    assert_eq!(result.parent_state_cid(), parent_state.0.raw_cid);
    assert_eq!(
        result
            .header()
            .0
            .node
            .as_ref()
            .unwrap()
            .parent_state
            .0
            .raw_cid,
        parent_state.0.raw_cid
    );
    assert!(validate_root_genesis(&header, &no_content(), i64::MAX).is_err());
    for bad_path in [
        vec![],
        vec!["Nexus".into()],
        vec!["Other".into(), "Alpha".into()],
    ] {
        assert!(validate_child_genesis(&header, &no_content(), i64::MAX, &bad_path).is_err());
    }
    child.parent_state = Volume::new(LatticeState::empty().unwrap()).unwrap();
    assert!(
        validate_child_genesis(&Volume::new(child).unwrap(), &no_content(), i64::MAX, &path)
            .is_err()
    );
}

#[test]
fn child_successor_carries_both_parent_state_bindings() {
    let mut child = genesis();
    child.parent_state = Volume::new(LatticeState {
        account_state: Volume::new(
            VolumeMerkleDictionary::default()
                .inserting("parent", 1u64)
                .unwrap(),
        )
        .unwrap(),
        ..LatticeState::empty().unwrap()
    })
    .unwrap();
    let parent_state_cid = child.parent_state.0.raw_cid.clone();
    let mut target = [0; 32];
    target[31] = 1;
    child.target = UInt256(target);
    child.next_target = child.target;
    let parent = Volume::new(child).unwrap();
    let first = successor(&parent, None);
    assert!(!first.0.node.as_ref().unwrap().has_valid_pow());
    let path = vec!["Nexus".into(), "Alpha".into()];
    let result = validate_child_transition(&first, &no_content(), i64::MAX, None, &path).unwrap();
    assert_eq!(result.chain_path(), path);
    assert_eq!(result.parent_cid(), parent.0.raw_cid);
    assert_eq!(result.parent_state_cid(), parent_state_cid);
    assert_eq!(result.predecessor_parent_state_cid(), parent_state_cid);
    assert!(validate_root_transition(&first, &no_content(), i64::MAX, None).is_err());
}

#[test]
fn withdrawal_preflight_authenticates_deposit_receipt_and_directory() {
    let action = WithdrawalAction {
        withdrawer: "bob".into(),
        demander: "alice".into(),
        amount_demanded: 7,
        amount_withdrawn: 9,
        nonce: 1,
    };
    let deposit = DepositKey {
        demander: "alice".into(),
        amount_demanded: 7,
        nonce: 1,
    }
    .to_string();
    let receipt = ReceiptKey {
        directory: "Alpha".into(),
        demander: "alice".into(),
        amount_demanded: 7,
        nonce: 1,
    }
    .storage_key();
    let previous = Volume::new(LatticeState {
        deposit_state: Volume::new(
            VolumeMerkleDictionary::default()
                .inserting(&deposit, 9u64)
                .unwrap(),
        )
        .unwrap(),
        ..LatticeState::empty().unwrap()
    })
    .unwrap();
    let parent = Volume::new(LatticeState {
        receipt_state: Volume::new(
            VolumeMerkleDictionary::default()
                .inserting(&receipt, "bob".to_owned())
                .unwrap(),
        )
        .unwrap(),
        ..LatticeState::empty().unwrap()
    })
    .unwrap();
    validate_authenticated_withdrawals(
        &previous,
        &parent,
        "Alpha",
        std::slice::from_ref(&action),
        &no_content(),
    )
    .unwrap();
    assert!(
        validate_authenticated_withdrawals(
            &previous,
            &parent,
            "Beta",
            std::slice::from_ref(&action),
            &no_content()
        )
        .is_err()
    );
    assert!(
        validate_authenticated_withdrawals(
            &previous,
            &parent,
            "Alpha",
            &[action.clone(), action.clone()],
            &no_content()
        )
        .is_err()
    );
    let missing = Volume(previous.0.removing_node());
    assert!(
        validate_authenticated_withdrawals(&missing, &parent, "Alpha", &[action], &no_content())
            .is_err()
    );
}

#[test]
fn builders_reproduce_genesis_and_build_valid_rewarded_successor() {
    let reference = genesis();
    let spec = reference.spec.0.node.as_deref().unwrap();
    let children = BTreeMap::new();
    let built = GenesisTemplate {
        spec,
        transactions: &[],
        children: &children,
        parent_state: None,
        timestamp: reference.timestamp,
        target: reference.target,
        nonce: 0,
        version: 1,
    }
    .build(&no_content())
    .unwrap();
    assert_eq!(
        content_id(&built.block).unwrap(),
        content_id(&reference).unwrap()
    );
    let recipient = content_id(&"miner").unwrap();
    let successor = BlockTemplate {
        previous: &reference,
        transactions: &[],
        children: &children,
        parent_chain_block: None,
        timestamp: reference.timestamp + 1000,
        target: None,
        next_target: None,
        nonce: 9,
        difficulty_anchor: None,
        reward_recipient: Some(&recipient),
    }
    .build(&no_content())
    .unwrap();
    let mut block = successor.block;
    // Restore attached nodes stripped from the wire template, retaining its CID.
    let cid = content_id(&block).unwrap();
    block.parent = Some(Volume::new(reference.clone()).unwrap());
    block.prev_state = reference.post_state.clone();
    assert_eq!(content_id(&block).unwrap(), cid);
    let verified = validate_root_transition(
        &Volume::new(block.clone()).unwrap(),
        &no_content(),
        i64::MAX,
        None,
    )
    .unwrap();
    assert_eq!(verified.state_diff(), &successor.state_diff);
    assert_eq!(
        verified.post_state().0.raw_cid,
        successor.materialized_post_state.0.raw_cid
    );
    assert!(mine_block(&block, block.target, 0).is_none());
    let mined = mine_block(&block, block.target, 1).unwrap();
    assert_eq!(mined.nonce, 0);
    assert!(mined.has_valid_pow());
}

#[test]
fn builder_anchor_walk_distinguishes_missing_content_from_bad_ancestry() {
    let parent = Volume::new(genesis()).unwrap();
    let first = successor(&parent, None);
    let second = successor(&first, None);
    let expected = DifficultyAnchor {
        block_height: 1,
        timestamp: first.0.node.as_ref().unwrap().timestamp,
        target: first.0.node.as_ref().unwrap().target,
    };
    assert_eq!(
        resolve_difficulty_anchor(second.0.node.as_deref().unwrap(), &no_content()).unwrap(),
        Some(expected)
    );
    let mut missing = second.0.node.as_deref().unwrap().clone();
    missing.parent = Some(Volume(first.0.removing_node()));
    assert!(resolve_difficulty_anchor(&missing, &no_content()).is_err());
    missing.parent = Some(parent);
    assert_eq!(
        resolve_difficulty_anchor(&missing, &no_content()).unwrap(),
        None
    );
    missing.height = u64::MAX;
    assert!(matches!(
        BlockTemplate {
            previous: &missing,
            transactions: &[],
            children: &BTreeMap::new(),
            parent_chain_block: None,
            timestamp: i64::MAX,
            target: None,
            next_target: None,
            nonce: 0,
            difficulty_anchor: None,
            reward_recipient: None,
        }
        .build(&no_content()),
        Err(BlockBuilderError::HeightOverflow)
    ));
}
