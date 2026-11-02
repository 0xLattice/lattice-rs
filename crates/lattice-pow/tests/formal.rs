use lattice_pow::{asert_target, WorkSum};

#[test]
fn all_formal_asert_vectors_match() {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("formal/asert-vectors.json")).unwrap();
    assert_eq!(rows.len(), 2703);
    for row in rows {
        let field = |key: &str| row[key].as_str().unwrap();
        let target = WorkSum::from_hex(field("anchorTarget"))
            .unwrap()
            .as_target()
            .unwrap();
        let actual = asert_target(
            target,
            field("anchorTimestamp").parse().unwrap(),
            field("anchorHeight").parse().unwrap(),
            field("blockTimestamp").parse().unwrap(),
            field("blockHeight").parse().unwrap(),
            field("targetBlockTime").parse().unwrap(),
            field("halfLife").parse().unwrap(),
        );
        assert_eq!(
            WorkSum::from(actual).to_hex(),
            field("nextTarget"),
            "{}",
            field("name")
        );
    }
}
