//! Emit production work observations for the independent Lean work model.
use lattice_pow::{validate, work_for_hash, work_for_target, Target};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    target: String,
    hash: String,
    target_work: String,
    hash_work: String,
    meets: bool,
}
fn target(s: &str) -> Target {
    let n = BigUint::parse_bytes(s.as_bytes(), 10).expect("natural number");
    let bytes = n.to_bytes_be();
    assert!(bytes.len() <= 32, "outside UInt256");
    let mut out = [0; 32];
    out[32 - bytes.len()..].copy_from_slice(&bytes);
    out
}
fn main() {
    let input = std::fs::read(std::env::args().nth(1).expect("input path")).unwrap();
    let mut rows: Vec<Row> = serde_json::from_slice(&input).unwrap();
    for row in &mut rows {
        let t = target(&row.target);
        let h = target(&row.hash);
        row.target_work = BigUint::from_bytes_be(&work_for_target(&t)).to_string();
        row.hash_work = BigUint::from_bytes_be(&work_for_hash(&h)).to_string();
        row.meets = validate(&h, &t);
    }
    println!("{}", serde_json::to_string(&rows).unwrap());
}
