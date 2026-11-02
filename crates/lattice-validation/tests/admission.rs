use lattice_cashew::{Header, MerkleDictionary, Volume};
use lattice_primitives::*;
use lattice_validation::*;
use std::collections::BTreeMap;
fn genesis() -> Block {
    let empty = Volume::new(LatticeState::empty().unwrap()).unwrap();
    let spec = ChainSpec {
        max_number_of_transactions_per_block: 10,
        max_state_growth: 10000,
        max_block_size: 100000,
        premine: 0,
        target_block_time: 1000,
        initial_reward: 100,
        halving_interval: 100,
        half_life: 120,
        wasm_policies: vec![],
    };
    Block {
        version: 1,
        parent: None,
        transactions: Header::new(MerkleDictionary::default()).unwrap(),
        target: UInt256([127; 32]),
        next_target: UInt256([127; 32]),
        spec: Volume::new(spec).unwrap(),
        parent_state: empty.clone(),
        prev_state: empty.clone(),
        post_state: empty,
        children: Header::new(BTreeMap::new()).unwrap(),
        height: 0,
        timestamp: 1000,
        reward_recipient: None,
        nonce: 0,
    }
}
#[test]
fn header_classification_checks_schedule_before_clock_and_exclusion() {
    let genesis = genesis();
    let parent = HeaderLinkageParent::from(&genesis);
    let spec = genesis.spec.0.node.as_deref().unwrap();
    let mut block = genesis.clone();
    block.height = 1;
    block.timestamp = 2000;
    block.parent = Some(Volume::new(genesis.clone()).unwrap());
    assert_eq!(
        header_admission(
            &block,
            &parent,
            spec,
            || panic!("immediate anchor must avoid lookup"),
            2000
        ),
        Ok(HeaderAdmission::Linked)
    );
    assert_eq!(
        header_admission(&block, &parent, spec, || None, 1999),
        Err(HeaderDeferred::NotYetValid)
    );
    block.next_target = UInt256([1; 32]);
    assert_eq!(
        header_admission(&block, &parent, spec, || None, 1999),
        Ok(HeaderAdmission::OffSchedule)
    );
    block.next_target = block.target;
    block.spec = Volume::new(ChainSpec {
        initial_reward: 200,
        ..spec.clone()
    })
    .unwrap();
    assert_eq!(
        header_admission(&block, &parent, spec, || None, 2000),
        Ok(HeaderAdmission::Excluded)
    );
    block.height = 2;
    assert_eq!(
        header_admission(&block, &parent, spec, || None, 2000),
        Ok(HeaderAdmission::Malformed)
    );
}
#[test]
fn missing_anchor_is_retriable_and_height_overflow_is_malformed() {
    let mut block = genesis();
    let spec = block.spec.0.node.as_deref().unwrap().clone();
    let mut parent = HeaderLinkageParent::from(&block);
    parent.height = 5;
    block.height = 6;
    block.timestamp = 2000;
    assert_eq!(
        header_admission(&block, &parent, &spec, || None, 2000),
        Err(HeaderDeferred::AnchorUnavailable)
    );
    parent.height = u64::MAX;
    block.height = 0;
    assert_eq!(
        header_admission(&block, &parent, &spec, || None, 2000),
        Ok(HeaderAdmission::Malformed)
    );
}
#[test]
fn genesis_distinguishes_root_from_child_and_premine_bound() {
    let mut block = genesis();
    assert!(has_genesis_shape(&block, true).unwrap());
    assert!(!has_genesis_shape(&block, false).unwrap());
    let mut state = LatticeState::empty().unwrap();
    state.general_state =
        Volume::new(GeneralState::default().inserting("x", "y".into()).unwrap()).unwrap();
    block.parent_state = Volume::new(state).unwrap();
    assert!(has_genesis_shape(&block, false).unwrap());
    assert!(!has_genesis_shape(&block, true).unwrap());
    let mut spec = block.spec.0.node.as_deref().unwrap().clone();
    spec.premine = 2;
    assert!(valid_genesis_credits(
        &spec,
        &[AccountAction {
            owner: "a".into(),
            delta: 200
        }]
    ));
    assert!(!valid_genesis_credits(
        &spec,
        &[AccountAction {
            owner: "a".into(),
            delta: 201
        }]
    ));
}
fn body() -> TransactionBody {
    TransactionBody {
        account_actions: vec![
            AccountAction {
                owner: "alice".into(),
                delta: -10,
            },
            AccountAction {
                owner: "bob".into(),
                delta: 10,
            },
        ],
        actions: vec![Action {
            key: "x".into(),
            old_value: None,
            new_value: Some("é".into()),
        }],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec!["alice".into()],
        nonce: 0,
        chain_path: vec!["Nexus".into()],
    }
}
#[test]
fn state_growth_floor_exact_and_ceiling_bracket_transitions() {
    let body = body();
    let bodies = vec![body.clone()];
    assert_eq!(action_state_delta(&body), Ok(3));
    assert_eq!(state_growth_floor(&bodies), Ok(-10));
    assert_eq!(state_growth_ceiling(&bodies, None), Ok(34));
    assert_eq!(state_growth_ceiling(&bodies, Some("miner")), Ok(47));
    for alice in [10, 20] {
        for bob in [0, 10] {
            let mut previous = ResolvedState::default();
            previous.accounts.insert("alice".into(), alice);
            if bob > 0 {
                previous.accounts.insert("bob".into(), bob);
            }
            let result = execute_block(
                &previous,
                &BTreeMap::new(),
                "Nexus",
                &TransitionActions::from_bodies(&bodies),
                &bodies,
                0,
                None,
            )
            .unwrap();
            let exact = state_growth(&bodies, result.account_growth).unwrap();
            assert!(
                state_growth_floor(&bodies).unwrap() <= exact
                    && exact <= state_growth_ceiling(&bodies, None).unwrap()
            );
        }
    }
    assert_eq!(
        state_growth(&bodies, i64::MAX),
        Err(TransitionError::GrowthOverflow)
    );
}

#[test]
fn logical_size_counts_unique_owned_bytes_and_stops_fetching_at_limit() {
    use lattice_cashew::ContentSource;
    use std::cell::Cell;
    let mut block = genesis();
    let transaction = Transaction::new(BTreeMap::new(), Header::new(body()).unwrap());
    let volume = Volume::new(transaction.clone()).unwrap();
    block.transactions = Header::new(
        MerkleDictionary::default()
            .inserting("0", volume.clone())
            .unwrap()
            .inserting("1", volume.clone())
            .unwrap(),
    )
    .unwrap();
    let root = Volume::new(block.clone()).unwrap();
    let mut owned = lattice_cashew::volume::serialize_boundary(&root, &())
        .unwrap()
        .entries;
    owned.extend(
        lattice_cashew::volume::serialize_boundary(&volume, &())
            .unwrap()
            .entries,
    );
    let expected: i64 = owned.values().map(|b| b.len() as i64).sum();
    // No spec or state content is present in this source.
    let mut unloaded: Block =
        lattice_cashew::cbor::decode(&canonical_bytes(&block).unwrap()).unwrap();
    let result = resolve_block_content(&unloaded, &owned, expected).unwrap();
    assert_eq!(result.byte_count, expected);
    assert_eq!(result.transactions.len(), 2);
    assert!(matches!(
        resolve_block_content(&unloaded, &owned, expected - 1),
        Err(BlockContentError::Size(ContentSizeError::ExceedsLimit))
    ));
    struct Counting<'a> {
        inner: &'a BTreeMap<String, Vec<u8>>,
        calls: Cell<usize>,
    }
    impl ContentSource for Counting<'_> {
        fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
            self.calls.set(self.calls.get() + 1);
            self.inner.fetch(cid)
        }
    }
    let source = Counting {
        inner: &owned,
        calls: Cell::new(0),
    };
    assert!(matches!(
        resolve_block_content(&unloaded, &source, 0),
        Err(BlockContentError::Size(ContentSizeError::ExceedsLimit))
    ));
    assert_eq!(source.calls.get(), 0);
    // Source errors named Limit must not become size verdicts.
    struct Failing;
    impl ContentSource for Failing {
        fn fetch(&self, _: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
            Err(lattice_cashew::Error::Limit)
        }
    }
    unloaded.children = unloaded.children.removing_node();
    assert!(matches!(
        resolve_block_content(&unloaded, &Failing, i64::MAX),
        Err(BlockContentError::Content(lattice_cashew::Error::Limit))
    ));
}
#[test]
fn child_commitment_binds_directory_child_and_entering_state() {
    let mut block = genesis();
    let child = Volume::new(genesis()).unwrap();
    block.children = Header::new(BTreeMap::from([("Child".into(), child.clone())])).unwrap();
    let mut tx = body();
    tx.actions = vec![Action {
        key: "child/Child".into(),
        old_value: None,
        new_value: Some(format!(
            "{}@{}",
            child.0.raw_cid, block.prev_state.0.raw_cid
        )),
    }];
    assert!(valid_child_commitments(&block, &[tx.clone()]).unwrap());
    tx.actions[0].new_value = Some(format!("{}@wrong", child.0.raw_cid));
    assert!(!valid_child_commitments(&block, &[tx.clone()]).unwrap());
    tx.actions[0].new_value = None;
    assert!(valid_child_commitments(&block, &[tx]).unwrap());
}
