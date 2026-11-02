//! Actual unchanged pinned Swift TransactionPreflight.classify observations.
use lattice_cashew::{Volume, cbor};
use lattice_primitives::*;
use lattice_validation::*;
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    tip: String,
    transaction: String,
    entries: BTreeMap<String, Vec<u8>>,
    checks_policies: bool,
    chain_path: Vec<String>,
    parent_state: Option<String>,
    swift: String,
}
#[test]
fn pinned_swift_preflight_dispositions() {
    let rows: Vec<Case> = serde_json::from_str(include_str!("preflight-swift.json")).unwrap();
    assert_eq!(rows.len(), 51);
    let intended_ready = [
        "ready",
        "insert_new",
        "timestamp_overflow_policy_disabled",
        "policy_accept",
        "policy_disabled_ready",
        "withdrawal_ready",
        "update_existing",
        "withdrawal_nonce_u64_max",
        "withdrawal_nonce_u64_max_minus_one",
    ];
    for row in rows {
        if intended_ready.contains(&row.name.as_str()) {
            assert_eq!(row.swift, "ready", "positive corpus witness {}", row.name);
        }
        if ["future_nonce", "policy_disabled_future"].contains(&row.name.as_str()) {
            assert_eq!(row.swift, "future");
        }
        let tx: Transaction = cbor::decode(&row.entries[&row.transaction]).unwrap();
        let tip = Volume::<Block>::from_cid(row.tip).unwrap();
        let parent = row
            .parent_state
            .map(|cid| Volume::<LatticeState>::from_cid(cid).unwrap());
        let result = TransactionPreflight {
            transaction: &tx,
            tip: &tip,
            chain_path: &row.chain_path,
            parent_state: parent.as_ref(),
            now_milliseconds: 0,
            checks_policies: row.checks_policies,
        }
        .classify(&row.entries)
        .unwrap();
        assert_eq!(
            format!("{:?}", result.disposition()).to_lowercase(),
            row.swift,
            "{}",
            row.name
        );
        if result.disposition() != PreflightDisposition::Ready {
            assert!(
                result.debit_balances().is_empty(),
                "{} retained spend authority",
                row.name
            );
        }
    }
}
#[test]
fn swift_u128_encoding_boundary_is_preserved() {
    let evidence: serde_json::Value =
        serde_json::from_str(include_str!("preflight-swift-evidence.json")).unwrap();
    for nonce in [u128::from(u64::MAX), 1u128 << 64, 1u128 << 80, u128::MAX] {
        let action = WithdrawalAction {
            withdrawer: "owner".into(),
            nonce,
            demander: "owner".into(),
            amount_demanded: 1,
            amount_withdrawn: 1,
        };
        let actual = if cbor::encode(&action).is_ok() {
            "encoded"
        } else {
            "rejected"
        };
        assert_eq!(
            evidence["encoding_boundaries"][format!("encoding-nonce-{nonce}")],
            actual
        );
    }
}
