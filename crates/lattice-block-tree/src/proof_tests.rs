use crate::*;
use lattice_cashew::{ContentSource, Header, MerkleDictionary, Volume, cbor};
use lattice_primitives::*;
use std::collections::BTreeMap;
fn block(nonce: u64) -> Block {
    let empty = Volume::new(LatticeState::empty().unwrap()).unwrap();
    Block {
        version: 1,
        parent: None,
        transactions: Header::new(MerkleDictionary::default()).unwrap(),
        target: UInt256([255; 32]),
        next_target: UInt256([255; 32]),
        spec: Volume::new(ChainSpec {
            max_number_of_transactions_per_block: 100,
            max_state_growth: 100_000,
            max_block_size: 1_000_000,
            premine: 0,
            target_block_time: 1000,
            initial_reward: 1024,
            halving_interval: 210_000,
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
        nonce,
    }
}
struct Source(BTreeMap<String, Vec<u8>>);
impl ContentSource for Source {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
        self.0
            .get(cid)
            .cloned()
            .ok_or_else(|| lattice_cashew::Error::MissingContent(cid.into()))
    }
}
fn source(blocks: &[&Block]) -> Source {
    let mut map = BTreeMap::new();
    for b in blocks {
        map.insert(content_id(*b).unwrap(), cbor::encode(*b).unwrap());
        map.insert(
            b.children.raw_cid.clone(),
            cbor::encode(b.children.node.as_deref().unwrap()).unwrap(),
        );
    }
    Source(map)
}
#[test]
fn exact_single_hop_verifies_and_rejects_padding_or_missing_leaf() {
    let child = block(1);
    let mut root = block(2);
    root.children = Header::new(BTreeMap::from([(
        "Alpha".into(),
        Volume::new(child.clone()).unwrap(),
    )]))
    .unwrap();
    let src = source(&[&root, &child]);
    let proof = ChildBlockProof::generate(&content_id(&root).unwrap(), "Alpha", &src).unwrap();
    assert_eq!(proof.entries.len(), 3);
    let evidence = proof
        .verify_securing_work(&child, &["Nexus".into(), "Alpha".into()])
        .unwrap();
    assert!(evidence.contribution().is_some());
    assert_eq!(evidence.child_cid(), content_id(&child).unwrap());
    let mut bad = proof.clone();
    bad.entries.push((
        content_id(&block(99)).unwrap(),
        cbor::encode(&block(99)).unwrap(),
    ));
    assert_eq!(
        bad.verify_securing_work(&child, &["Nexus".into(), "Alpha".into()])
            .unwrap_err(),
        ChildProofFailure::MalformedEvidence
    );
    let mut bad = proof.clone();
    bad.entries.retain(|(cid, _)| cid != evidence.child_cid());
    assert!(
        bad.verify_securing_work(&child, &["Nexus".into(), "Alpha".into()])
            .is_err()
    );
    assert!(DirectChildEdge::derive(&proof).unwrap().validates(&child));
    assert_eq!(
        ChildBlockProof::deserialize(&proof.serialize().unwrap()),
        Some(proof)
    );
}
#[test]
fn composed_root_most_work_pricing_and_exact_terminal_hop() {
    let child = block(1);
    let mut middle = block(2);
    middle.target = UInt256([127; 32]);
    middle.children = Header::new(BTreeMap::from([(
        "Beta".into(),
        Volume::new(child.clone()).unwrap(),
    )]))
    .unwrap();
    let mut root = block(3);
    root.children = Header::new(BTreeMap::from([(
        "Alpha".into(),
        Volume::new(middle.clone()).unwrap(),
    )]))
    .unwrap();
    while !lattice_pow::validate(&root.pow_hash(), &middle.target.0) {
        root.nonce += 1;
    }
    let src = source(&[&root, &middle, &child]);
    let upstream = ChildBlockProof::generate(&content_id(&root).unwrap(), "Alpha", &src).unwrap();
    let hop = ChildBlockProof::generate(&content_id(&middle).unwrap(), "Beta", &src).unwrap();
    let proof = upstream.composing(&hop);
    assert_eq!(proof.entries.len(), 5);
    let evidence = proof
        .verify_securing_work(&child, &["Nexus".into(), "Alpha".into(), "Beta".into()])
        .unwrap();
    assert_eq!(
        evidence.credited_ancestor_work(),
        UInt256(lattice_pow::work_for_target(&root.target.0))
    );
    assert!(
        evidence.credited_ancestor_work() < UInt256(lattice_pow::work_for_target(&middle.target.0))
    );
    assert_eq!(proof.direct_hop().unwrap().proof, hop);
    assert_eq!(
        proof
            .scheduling_targets(&root, &child)
            .unwrap()
            .deployment_target,
        Some(middle.target)
    );
}
#[test]
fn wire_rejects_unsorted_duplicate_trailing_and_truncated_entries() {
    let p = ChildBlockProof::new(
        "root".into(),
        vec!["Alpha".into()],
        vec![("a".into(), vec![1]), ("b".into(), vec![2])],
    );
    let bytes = p.serialize().unwrap();
    for end in 0..bytes.len() {
        assert!(ChildBlockProof::deserialize(&bytes[..end]).is_none());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(ChildBlockProof::deserialize(&trailing).is_none());
    let mut unsorted = p.clone();
    unsorted.entries.reverse();
    assert!(ChildBlockProof::deserialize(&unsorted.serialize().unwrap()).is_none());
    let mut duplicate = p.clone();
    duplicate.entries[1] = duplicate.entries[0].clone();
    assert!(ChildBlockProof::deserialize(&duplicate.serialize().unwrap()).is_none());
}
#[test]
fn structural_parent_state_failure_is_distinct_from_bad_bytes() {
    let mut child = block(1);
    child.parent_state =
        Volume(Header::from_cid(lattice_cashew::cid_for_bytes(0x71, b"different state")).unwrap());
    let mut root = block(2);
    root.children = Header::new(BTreeMap::from([(
        "Alpha".into(),
        Volume::new(child.clone()).unwrap(),
    )]))
    .unwrap();
    root.prev_state = Volume::new(LatticeState::empty().unwrap()).unwrap();
    let src = source(&[&root, &child]);
    let mut proof = ChildBlockProof::generate(&content_id(&root).unwrap(), "Alpha", &src).unwrap();
    assert_eq!(
        proof
            .verify_securing_work(&child, &["Nexus".into(), "Alpha".into()])
            .unwrap_err(),
        ChildProofFailure::ProtocolInvalid
    );
    proof
        .entries
        .iter_mut()
        .find(|(cid, _)| cid == &proof.root_cid)
        .unwrap()
        .1
        .push(0);
    assert_eq!(
        proof
            .verify_securing_work(&child, &["Nexus".into(), "Alpha".into()])
            .unwrap_err(),
        ChildProofFailure::MalformedEvidence
    );
}
fn restored_root(root: &Block) -> ChainTree {
    let hash = content_id(root).unwrap();
    let fact = ChainBlockFact {
        block_hash: hash.clone(),
        parent_block_hash: None,
        block_height: 0,
        post_state_cid: root.post_state.0.raw_cid.clone(),
        prev_state_cid: root.prev_state.0.raw_cid.clone(),
        parent_state_cid: Some(root.parent_state.0.raw_cid.clone()),
        spec_cid: root.spec.0.raw_cid.clone(),
        target: target_hex(&root.target.0),
        next_target: target_hex(&root.next_target.0),
        timestamp: root.timestamp,
        state_diff: StateDiff::default(),
        child_commitments: Some(BTreeMap::new()),
    };
    let batch = BlockImportBatch::from_verified_import(vec![
        ChainFact::Block(Box::new(fact)),
        ChainFact::Work(ChainWorkFact {
            block_hash: hash.clone(),
            contribution: VerifiedWorkContribution::from_verified_import(
                hash.clone(),
                UInt256(lattice_pow::work_for_target(&root.target.0)),
            ),
        }),
        ChainFact::Validation(ChainValidationFact {
            block_hash: hash.clone(),
        }),
    ]);
    let mut tree = ChainTree::restore(
        &[batch],
        1,
        ChainRuntimeContext::new(vec!["Nexus".into()], Some(&hash)).unwrap(),
    )
    .unwrap();
    assert!(tree.hold_spec(
        root.spec.0.node.as_deref().unwrap().clone(),
        &root.spec.0.raw_cid
    ));
    tree
}
#[test]
fn authenticated_header_preparation_does_not_mutate_and_rejects_stale_base() {
    let root = block(0);
    let mut candidate = block(1);
    candidate.parent = Some(Volume::new(root.clone()).unwrap());
    candidate.height = 1;
    candidate.timestamp = root.timestamp + 1000;
    let mut tree = restored_root(&root);
    let HeaderPreparation::Prepared(prepared) = tree
        .prepare_root_header(
            &candidate,
            candidate.children.node.as_deref().unwrap(),
            candidate.timestamp,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(tree.block_count(), 1);
    assert_eq!(prepared.batches().len(), 1);
    let update = tree.apply_prepared(*prepared).unwrap();
    assert_eq!(update.block_hash, content_id(&candidate).unwrap());
    assert_eq!(tree.block_count(), 2);
    assert!(matches!(
        tree.prepare_root_header(
            &candidate,
            candidate.children.node.as_deref().unwrap(),
            candidate.timestamp
        )
        .unwrap(),
        HeaderPreparation::Duplicate
    ));
    let mut left = restored_root(&root);
    let HeaderPreparation::Prepared(prepared) = left
        .prepare_root_header(
            &candidate,
            candidate.children.node.as_deref().unwrap(),
            candidate.timestamp,
        )
        .unwrap()
    else {
        panic!()
    };
    left.serve_runs("new directory");
    assert_eq!(
        left.apply_prepared(*prepared).unwrap_err(),
        AdmissionError::StalePreparation
    );
    assert_eq!(left.block_count(), 1);
}
#[test]
fn header_admission_keeps_future_schedule_and_exclusion_distinct() {
    let root = block(0);
    let mut candidate = block(1);
    candidate.parent = Some(Volume::new(root.clone()).unwrap());
    candidate.height = 1;
    candidate.timestamp = root.timestamp + 1000;
    let tree = restored_root(&root);
    assert!(matches!(
        tree.prepare_root_header(
            &candidate,
            candidate.children.node.as_deref().unwrap(),
            root.timestamp
        ),
        Err(AdmissionError::NotYetValid)
    ));
    let mut wrong_time = candidate.clone();
    wrong_time.timestamp = root.timestamp;
    assert!(matches!(
        tree.prepare_root_header(
            &wrong_time,
            wrong_time.children.node.as_deref().unwrap(),
            candidate.timestamp
        ),
        Err(AdmissionError::ProofOfWorkInvalid)
    ));
    candidate.prev_state =
        Volume(Header::from_cid(lattice_cashew::cid_for_bytes(0x71, b"wrong prev-state")).unwrap());
    let HeaderPreparation::Prepared(prepared) = tree
        .prepare_root_header(
            &candidate,
            candidate.children.node.as_deref().unwrap(),
            candidate.timestamp,
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(prepared.excluded());
    assert_eq!(prepared.batches().len(), 2);
}
#[test]
fn original_swift_wire_vectors_match_byte_for_byte() {
    fn unhex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .chunks(2)
            .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
            .collect()
    }
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../tests/fixtures/swift-proof-wire.json")).unwrap();
    assert_eq!(rows.len(), 100);
    for row in rows {
        let path = row["path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().into())
            .collect();
        let entries = row["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e[0].as_str().unwrap().into(), unhex(e[1].as_str().unwrap())))
            .collect();
        let proof = ChildBlockProof::new(row["root"].as_str().unwrap().into(), path, entries);
        let bytes = unhex(row["wire"].as_str().unwrap());
        assert_eq!(proof.serialize().unwrap(), bytes);
        assert_eq!(ChildBlockProof::deserialize(&bytes), Some(proof));
    }
}
