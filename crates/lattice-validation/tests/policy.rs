use lattice_primitives::*;
use lattice_validation::*;
#[test]
fn canonical_context_matches_swift_host_determinism_golden() {
    // Pinned Lattice Sources/DeterminismGoldens/DeterminismGoldens.swift.
    let policy = WasmPolicyRef {
        module_cid: "tre127-policy-module".into(),
        source_cid: None,
        abi_version: 1,
        scope: PolicyScope::Action,
        entrypoint: PolicyScope::Action.default_entrypoint().into(),
    };
    let spec = ChainSpec {
        max_number_of_transactions_per_block: 100,
        max_state_growth: 100000,
        max_block_size: 1000000,
        premine: 0,
        target_block_time: 1000,
        initial_reward: 1024,
        halving_interval: 10000,
        half_life: 10,
        wasm_policies: vec![policy],
    };
    let action = Action {
        key: "tre127/policy-sentinel".into(),
        old_value: None,
        new_value: Some("large-int-9223372036854775807".into()),
    };
    let path = vec!["Nexus".into()];
    let body = TransactionBody {
        account_actions: vec![],
        actions: vec![action.clone()],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec!["tre127-signer".into()],
        nonce: 42,
        chain_path: path.clone(),
    };
    let context = WasmPolicyContext {
        abi_version: 1,
        scope: PolicyScope::Action,
        height: 0x0102030405060708,
        timestamp: 1700000000000,
        chain_spec: &spec,
        chain_path: &path,
        transaction: Some(&body),
        action: Some(&action),
        action_index: Some(0),
    };
    let actual: String = context
        .canonical_bytes()
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(actual, include_str!("policy-context.hex").trim());
}

fn spec_with(policies: Vec<WasmPolicyRef>) -> ChainSpec {
    ChainSpec {
        max_number_of_transactions_per_block: 100,
        max_state_growth: 10000,
        max_block_size: 100000,
        premine: 0,
        target_block_time: 1000,
        initial_reward: 1024,
        halving_interval: 10000,
        half_life: 10,
        wasm_policies: policies,
    }
}
fn body_with(actions: Vec<Action>) -> TransactionBody {
    TransactionBody {
        account_actions: vec![],
        actions,
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![],
        nonce: 0,
        chain_path: vec!["Nexus".into()],
    }
}
fn policy_source(
    code: &str,
    scope: PolicyScope,
) -> (WasmPolicyRef, std::collections::BTreeMap<String, Vec<u8>>) {
    let bytes = wat::parse_str(format!(
        r#"(module (memory (export "memory") 1)
        (func (export "lattice_alloc") (param i32) (result i32) i32.const 0)
        (func (export "check") (param i32 i32) (result i32) {code}))"#
    ))
    .unwrap();
    let header = lattice_cashew::Header::new(WasmPolicyModule { bytes }).unwrap();
    let policy = WasmPolicyRef {
        module_cid: header.raw_cid.clone(),
        source_cid: None,
        abi_version: 1,
        scope,
        entrypoint: "check".into(),
    };
    (
        policy,
        [(header.raw_cid.clone(), header.bytes(None).unwrap())].into(),
    )
}

#[test]
fn policy_context_scope_height_and_action_index_are_executed() {
    // scope at byte10; height's low byte at18; action index's low byte at len-1.
    let code = "local.get 0 i32.load8_u offset=10 i32.const 1 i32.eq
        local.get 0 i32.load8_u offset=18 i32.const 42 i32.eq i32.and
        local.get 0 local.get 1 i32.add i32.const 1 i32.sub i32.load8_u i32.eqz i32.and";
    let (policy, source) = policy_source(code, PolicyScope::Action);
    let spec = spec_with(vec![policy]);
    let path = vec!["Nexus".into()];
    let action = Action {
        key: "a".into(),
        old_value: None,
        new_value: Some("b".into()),
    };
    let one = [body_with(vec![action.clone()])];
    let two = [body_with(vec![action.clone(), action])];
    let batch = |bodies| PolicyBatch {
        bodies,
        spec: &spec,
        chain_path: &path,
        height: 42,
        timestamp: 0,
        scopes: None,
    };
    assert!(batch(&one).verify(&source).unwrap());
    assert!(!batch(&two).verify(&source).unwrap());
}

#[test]
fn genesis_validates_unused_programs_but_empty_batch_does_not_execute() {
    let (mut policy, source) = policy_source("i32.const 1", PolicyScope::Transaction);
    policy.entrypoint = "absent".into();
    let spec = spec_with(vec![policy]);
    assert!(!validate_configured_policy_modules(&spec, &source).unwrap());
    let path = vec!["Nexus".into()];
    let batch = PolicyBatch {
        bodies: &[],
        spec: &spec,
        chain_path: &path,
        height: 1,
        timestamp: 0,
        scopes: None,
    };
    assert!(batch.verify(&source).unwrap());
    assert_eq!(
        batch
            .verify(&std::collections::BTreeMap::new())
            .unwrap_err()
            .verdict(),
        PolicyFailureVerdict::Unavailable
    );
    assert!(
        PolicyBatch {
            scopes: Some(&[]),
            ..batch
        }
        .verify(&std::collections::BTreeMap::new())
        .unwrap()
    );
}

#[test]
fn module_authentication_and_fetch_failures_are_distinct() {
    let (policy, mut source) = policy_source("i32.const 1", PolicyScope::Transaction);
    let spec = spec_with(vec![policy.clone()]);
    source.get_mut(&policy.module_cid).unwrap().push(0);
    assert_eq!(
        validate_configured_policy_modules(&spec, &source)
            .unwrap_err()
            .verdict(),
        PolicyFailureVerdict::Invalid
    );
    struct BrokenSource;
    impl lattice_cashew::ContentSource for BrokenSource {
        fn fetch(&self, _cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
            Err(lattice_cashew::Error::NonCanonical)
        }
    }
    assert_eq!(
        validate_configured_policy_modules(&spec, &BrokenSource)
            .unwrap_err()
            .verdict(),
        PolicyFailureVerdict::Unavailable
    );
}

#[test]
fn selected_policies_deduplicate_fetches_and_abi_precedes_fetch() {
    let (policy, entries) = policy_source("i32.const 1", PolicyScope::Transaction);
    struct CountingSource {
        entries: std::collections::BTreeMap<String, Vec<u8>>,
        count: std::sync::atomic::AtomicUsize,
    }
    impl lattice_cashew::ContentSource for CountingSource {
        fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
            self.count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.entries
                .get(cid)
                .cloned()
                .ok_or_else(|| lattice_cashew::Error::MissingContent(cid.into()))
        }
    }
    let source = CountingSource {
        entries,
        count: 0.into(),
    };
    let mut spec = spec_with(vec![policy.clone(), policy]);
    assert!(validate_configured_policy_modules(&spec, &source).unwrap());
    assert_eq!(source.count.load(std::sync::atomic::Ordering::Relaxed), 1);
    spec.wasm_policies[0].abi_version = 2;
    assert!(!validate_configured_policy_modules(&spec, &source).unwrap());
    assert_eq!(source.count.load(std::sync::atomic::Ordering::Relaxed), 1);
}
