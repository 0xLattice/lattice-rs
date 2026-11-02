use lattice_cashew::{Header, MerkleDictionary, Volume};
use lattice_primitives::*;
use std::collections::BTreeMap;
fn golden_genesis() -> Block {
    let spec = ChainSpec {
        max_number_of_transactions_per_block: 100,
        max_state_growth: 100_000,
        max_block_size: 1_000_000,
        premine: 0,
        target_block_time: 1000,
        initial_reward: 1024,
        halving_interval: 210_000,
        half_life: 120,
        wasm_policies: vec![],
    };
    let empty = Volume::new(LatticeState::empty().unwrap()).unwrap();
    Block {
        version: 1,
        parent: None,
        transactions: Header::new(MerkleDictionary::default()).unwrap(),
        target: UInt256([255; 32]),
        next_target: UInt256([255; 32]),
        spec: Volume::new(spec).unwrap(),
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
#[test]
fn matches_pinned_swift_genesis_cid_and_pow_goldens() {
    // 0xLattice/Lattice@dda2d852 Tests/LatticeTests/SerializationPinningTests.swift
    let block = golden_genesis();
    assert_eq!(
        content_id(&block).unwrap(),
        "bafyreieq7pwonjmp3azvp3mkgqmaknqhtzvt3aw634zgulbvhhvfcvu3ia"
    );
    assert_eq!(
        hex::encode(block.pow_hash()),
        "ae6610161723772d77741acd0c8bfcc495c1980856ec234dc1a9bf972c10f69b"
    );
    let bytes = canonical_bytes(&block).unwrap();
    let decoded: Block = lattice_cashew::cbor::decode(&bytes).unwrap();
    assert_eq!(canonical_bytes(&decoded).unwrap(), bytes);
    assert!(block.has_valid_pow());
}
#[test]
fn block_decode_rejects_nonstandard_and_encrypted_links() {
    let block = golden_genesis();
    let mut wire = serde_json::to_value(&block).unwrap();
    wire["spec"]["rawCID"] = lattice_cashew::cid_for_bytes(0x55, b"raw").into();
    assert!(serde_json::from_value::<Block>(wire).is_err());
    let mut wire = serde_json::to_value(&block).unwrap();
    wire["prevState"]["encryptionInfo"] = serde_json::json!({"keyHash":"key","iv":"iv"});
    assert!(serde_json::from_value::<Block>(wire).is_err());
}
#[test]
fn recipient_and_child_validation_are_explicit_admission_checks() {
    let mut block = golden_genesis();
    block.reward_recipient = Some("bad".into());
    assert!(!block.has_well_formed_reward_recipient());
    assert!(block.children_are_well_formed().unwrap());
    let child = Volume::new(golden_genesis()).unwrap();
    block.children = Header::new(BTreeMap::from([("bad/name".into(), child)])).unwrap();
    assert!(!block.children_are_well_formed().unwrap());
    block.children = block.children.removing_node();
    assert!(block.children_are_well_formed().is_err());
}
