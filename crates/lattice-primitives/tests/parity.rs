use lattice_primitives::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SigningVector {
    #[serde(rename = "bodyCID")]
    body_cid: String,
    nonce: String,
    path: Vec<String>,
    preimage: String,
    private_key: String,
    public_key: String,
    signature: String,
}

#[test]
fn swift_signing_vectors_and_domain_separation() {
    let vectors: Vec<SigningVector> =
        serde_json::from_str(include_str!("swift-signing.json")).unwrap();
    assert_eq!(vectors.len(), 12);
    for v in vectors {
        let message = signing_preimage(&v.body_cid, &v.path, v.nonce.parse().unwrap());
        assert_eq!(message, v.preimage);
        let rust_signature = crypto::sign(&message, &v.private_key).unwrap();
        assert!(crypto::verify(&message, &rust_signature, &v.public_key));
        assert!(crypto::verify(&message, &v.signature, &v.public_key));
        assert!(!crypto::verify(
            &(message.clone() + "\n"),
            &v.signature,
            &v.public_key
        ));
        assert!(!crypto::verify(
            &message,
            &v.signature.to_uppercase(),
            &v.public_key
        ));
        assert!(!crypto::verify(
            &message,
            &v.signature,
            &v.public_key.to_uppercase()
        ));
        assert!(!crypto::verify(
            &message,
            &format!("0x{}", v.signature),
            &v.public_key
        ));
    }
}

#[test]
fn exact_netting_cancels_before_enforcing_uint64_limit() {
    let receipt = |withdrawer: &str, demander: &str| ReceiptAction {
        withdrawer: withdrawer.into(),
        demander: demander.into(),
        nonce: 0,
        amount_demanded: u64::MAX,
        directory: "child".into(),
    };
    let actions = vec![
        receipt("alice", "bob"),
        receipt("alice", "bob"),
        receipt("bob", "alice"),
    ];
    let deltas = net_balance_deltas(&[], &actions).unwrap();
    assert_eq!(deltas["alice"], AccountBalanceDelta::Debit(u64::MAX));
    assert_eq!(deltas["bob"], AccountBalanceDelta::Credit(u64::MAX));
    assert_eq!(
        net_balance_deltas(&[], &actions[..2]),
        Err(StateError::BalanceOverflow)
    );
    assert_eq!(
        net_balance_deltas(
            &[AccountAction {
                owner: "alice".into(),
                delta: i64::MIN
            }],
            &[]
        ),
        Err(StateError::BalanceOverflow)
    );
}

#[test]
fn nonce_overflow_and_atom_boundaries() {
    assert_eq!(next_expected_nonce(None), Ok(0));
    assert_eq!(next_expected_nonce(Some(0)), Ok(1));
    assert_eq!(
        next_expected_nonce(Some(u64::MAX)),
        Err(StateError::NonceGap)
    );
    assert!(account_atom("alice"));
    for atom in ["", "a/b", "a b", "a\nb", "é", "\u{7f}"] {
        assert!(!account_atom(atom));
    }
}

#[test]
fn address_normalization_uses_multikey_and_canonical_dag_cbor() {
    let (_, key) = crypto::generate_key_pair();
    let address = crypto::create_address(&key).unwrap();
    assert!(standard_cid(&address));
    assert_eq!(
        crypto::create_address(&key.to_uppercase()).unwrap(),
        address
    );
    assert!(crypto::is_address(&address, &key));
    assert!(!crypto::is_address("bafy-invalid", &key));
}

#[test]
fn transaction_signature_array_normalizes_and_rejects_alias_duplicates() {
    let (_, key) = crypto::generate_key_pair();
    let cid = content_id(&"body").unwrap();
    let raw = serde_json::json!({"body":{"rawCID":cid},"signatures":[{"key":key.to_uppercase(),"value":"0xABCD"}]});
    let tx: Transaction = serde_json::from_value(raw).unwrap();
    assert_eq!(tx.signatures[&key], "abcd");
    let encoded = serde_json::to_value(&tx).unwrap();
    assert_eq!(encoded["signatures"][0]["key"], key);
    let duplicate = serde_json::json!({"body":{"rawCID":cid},"signatures":[{"key":key,"value":"aa"},{"key":key.to_uppercase(),"value":"bb"}]});
    assert!(serde_json::from_value::<Transaction>(duplicate).is_err());
    let cbor = canonical_bytes(&tx).unwrap();
    let restored: Transaction = lattice_cashew::cbor::decode(&cbor).unwrap();
    assert_eq!(restored.signatures, tx.signatures);
}

#[test]
fn policy_wire_field_names_preserve_swift_acronyms() {
    let policy = WasmPolicyRef {
        module_cid: content_id(&"module").unwrap(),
        source_cid: None,
        abi_version: 1,
        scope: PolicyScope::Action,
        entrypoint: PolicyScope::Action.default_entrypoint().into(),
    };
    let value = serde_json::to_value(&policy).unwrap();
    assert_eq!(value["moduleCID"], policy.module_cid);
    assert!(value.get("moduleCid").is_none());
    assert!(value.get("sourceCID").is_none());
    assert_eq!(
        lattice_cashew::cbor::decode::<WasmPolicyRef>(&canonical_bytes(&policy).unwrap()).unwrap(),
        policy
    );
}

#[test]
fn swift_noncanonical_multikey_aliases_normalize_but_do_not_verify_directly() {
    let (private, key) = crypto::generate_key_pair();
    let message = "multikey boundary";
    let signature = crypto::sign(message, &private).unwrap();
    let address = crypto::create_address(&key).unwrap();
    let cid = content_id(&"body").unwrap();
    let aliases = [
        format!("ed+1{}", &key[4..]),
        format!("ed81{}02{}", "80".repeat(7), &key[4..]),
    ];
    assert!(crypto::verify(message, &signature, &key));
    for alias in aliases {
        assert_eq!(crypto::create_address(&alias).unwrap(), address);
        assert!(!crypto::verify(message, &signature, &alias));
        let raw = serde_json::json!({"body":{"rawCID":cid},"signatures":[{"key":alias,"value":signature}]});
        let tx: Transaction = serde_json::from_value(raw).unwrap();
        assert_eq!(tx.signatures.get(&key), Some(&signature));
        let duplicate = serde_json::json!({"body":{"rawCID":cid},"signatures":[{"key":key,"value":signature},{"key":alias,"value":signature}]});
        assert!(serde_json::from_value::<Transaction>(duplicate).is_err());
    }
}

#[test]
fn swift_hex_pair_normalization_preserves_canonical_signature_gate() {
    let message = "signed hex pairs";
    assert_eq!(
        crypto::sign(message, &"00".repeat(32)),
        crypto::sign(message, &"-0".repeat(32))
    );
    assert_eq!(
        crypto::sign(message, &"01".repeat(32)),
        crypto::sign(message, &"+1".repeat(32))
    );
    assert!(crypto::sign(message, &"-1".repeat(32)).is_none());
    let (_, key) = crypto::generate_key_pair();
    let raw = serde_json::json!({"body":{"rawCID":content_id(&"body").unwrap()},"signatures":[{"key":key,"value":"+1-0"}]});
    let tx: Transaction = serde_json::from_value(raw).unwrap();
    assert_eq!(tx.signatures[&key], "0100");
    assert!(!crypto::verify(message, "+1-0", &key));
}
