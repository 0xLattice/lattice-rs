//! Materialize validated genesis boundaries for isolated integration fixtures.
use lattice_cashew::{Header, Volume};
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).expect("genesis corpus path");
    let rows: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let row = &rows[0];
    let entries: BTreeMap<String, Vec<u8>> =
        serde_json::from_value(row["entries"].clone()).unwrap();
    let header = Volume(Header::from_cid(row["cid"].as_str().unwrap()).unwrap());
    let validated = lattice_validation::validate_root_genesis(&header, &entries, i64::MAX).unwrap();
    let volumes = lattice_cashew::volume::serialize_recursive(validated.header(), &()).unwrap();
    println!("{}", serde_json::to_string(&volumes).unwrap());
}
