use lattice_cashew::{ContentSource, Header, MerkleDictionary, Volume, VolumeMerkleDictionary};
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
fn classify(
    tx: &Transaction,
    block: &Block,
    path: &[String],
    parent: Option<&LatticeStateHeader>,
    policies: bool,
    source: &impl ContentSource,
) -> TransactionPreflightResult {
    struct Overlay<'a, S> {
        entries: BTreeMap<String, Vec<u8>>,
        source: &'a S,
    }
    impl<S: ContentSource> ContentSource for Overlay<'_, S> {
        fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
            self.entries
                .get(cid)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| self.source.fetch(cid))
        }
    }
    let header = Volume::new(block.clone()).unwrap();
    let entries = lattice_cashew::volume::serialize_recursive(&header, &())
        .unwrap()
        .into_iter()
        .flat_map(|v| v.entries)
        .collect();
    let source = Overlay { entries, source };
    TransactionPreflight {
        transaction: tx,
        tip: &Volume::new(block.clone()).unwrap(),
        chain_path: path,
        parent_state: parent,
        now_milliseconds: block.timestamp + 1,
        checks_policies: policies,
    }
    .classify(&source)
    .unwrap()
}
#[test]
fn classification_binds_transaction_tip_and_authenticated_debit_balances() {
    let (private, public) = crypto::generate_key_pair();
    let owner = crypto::create_address(&public).unwrap();
    let source = BTreeMap::new();
    let path = vec!["Nexus".into()];
    let block = funded(&owner, 10, None);
    let tx = signed(body(&owner, 0), &private, &public);
    let ready = classify(&tx, &block, &path, None, true, &source);
    assert_eq!(ready.disposition(), PreflightDisposition::Ready);
    assert_eq!(
        ready.debit_balances(),
        &BTreeMap::from([(owner.clone(), 10)])
    );
    assert_eq!(ready.transaction_cid(), content_id(&tx).unwrap());
    assert_eq!(ready.tip_cid(), content_id(&block).unwrap());
    assert_eq!(ready.chain_path(), path);
    assert!(ready.checks_policies());
    let future = signed(body(&owner, 1), &private, &public);
    assert_eq!(
        classify(
            &future,
            &funded(&owner, 0, None),
            &path,
            None,
            true,
            &source
        )
        .disposition(),
        PreflightDisposition::Future
    );
    for block in [
        funded(&owner, 1, None),
        funded(&owner, 10, Some(0)),
        funded(&owner, 10, Some(u64::MAX)),
    ] {
        assert_eq!(
            classify(&tx, &block, &path, None, true, &source).disposition(),
            PreflightDisposition::Invalid
        );
    }
    let mut bad = tx.clone();
    bad.signatures.clear();
    assert_eq!(
        classify(&bad, &block, &path, None, true, &source).disposition(),
        PreflightDisposition::Invalid
    );
    assert_eq!(
        classify(
            &tx,
            &block,
            &["Nexus".into(), "Child".into()],
            None,
            true,
            &source
        )
        .disposition(),
        PreflightDisposition::Invalid
    );
}
#[test]
fn source_failures_never_become_permanent_refusals() {
    struct Failed;
    impl ContentSource for Failed {
        fn fetch(&self, _: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
            Err(lattice_cashew::Error::NonCanonical)
        }
    }
    let (private, public) = crypto::generate_key_pair();
    let owner = crypto::create_address(&public).unwrap();
    let tx = signed(body(&owner, 0), &private, &public);
    let mut block = funded(&owner, 10, None);
    block.spec = Volume(block.spec.0.removing_node());
    assert_eq!(
        classify(&tx, &block, &["Nexus".into()], None, true, &Failed).disposition(),
        PreflightDisposition::Unavailable
    );
    let mut bad = tx;
    bad.signatures.clear();
    assert_eq!(
        classify(&bad, &block, &["Nexus".into()], None, true, &Failed).disposition(),
        PreflightDisposition::Invalid
    );
}
#[test]
fn policies_gate_ready_future_and_missing_parent_after_cheap_refusals() {
    let (private, public) = crypto::generate_key_pair();
    let owner = crypto::create_address(&public).unwrap();
    let mut block = funded(&owner, 10, None);
    let module=Header::new(WasmPolicyModule{bytes:wat::parse_str(r#"(module (memory (export "memory") 1) (func (export "lattice_alloc") (param i32) (result i32) i32.const 0) (func (export "check") (param i32 i32) (result i32) i32.const 0))"#).unwrap()}).unwrap();
    let source = BTreeMap::from([(module.raw_cid.clone(), module.bytes(None).unwrap())]);
    let mut spec = block.spec.0.node.as_deref().unwrap().clone();
    spec.wasm_policies = vec![WasmPolicyRef {
        module_cid: module.raw_cid.clone(),
        source_cid: None,
        scope: PolicyScope::Transaction,
        abi_version: 1,
        entrypoint: "check".into(),
    }];
    block.spec = Volume::new(spec).unwrap();
    for nonce in [0, 1] {
        let tx = signed(body(&owner, nonce), &private, &public);
        assert_eq!(
            classify(&tx, &block, &["Nexus".into()], None, true, &source).disposition(),
            PreflightDisposition::Invalid
        );
        assert_eq!(
            classify(&tx, &block, &["Nexus".into()], None, false, &source).disposition(),
            if nonce == 0 {
                PreflightDisposition::Ready
            } else {
                PreflightDisposition::Future
            }
        );
    }
    let mut withdrawal = body(&owner, 0);
    withdrawal.chain_path.push("Child".into());
    withdrawal.account_actions.clear();
    withdrawal.withdrawal_actions.push(WithdrawalAction {
        withdrawer: owner.clone(),
        demander: owner,
        amount_demanded: 1,
        amount_withdrawn: 1,
        nonce: 0,
    });
    let tx = signed(withdrawal, &private, &public);
    let path = vec!["Nexus".into(), "Child".into()];
    assert_eq!(
        classify(&tx, &block, &path, None, true, &source).disposition(),
        PreflightDisposition::Invalid
    );
    assert_eq!(
        classify(&tx, &block, &path, None, false, &source).disposition(),
        PreflightDisposition::Unavailable
    );
    // A missing receipt is retriable before policy evaluation, as in Swift.
    let parent = Volume::new(LatticeState::empty().unwrap()).unwrap();
    assert_eq!(
        classify(&tx, &block, &path, Some(&parent), true, &source).disposition(),
        PreflightDisposition::Unavailable
    );
}
