use super::*;
#[test]
fn rotations_splices_exact_sums_and_recovery() {
    let mut work = vec![BigUint::from(1u8); 1001];
    let mut index = EulerWorkIndex::default();
    assert!(index.insert_root(0, &work));
    for h in 1..1000 {
        assert!(index.insert_leaf(h, h - 1, &work).is_some());
        assert!(index.valid(&work));
    }
    work[999] = BigUint::from(1u8) << 400;
    index.changed(999, &work).unwrap();
    assert_eq!(
        index.weight(0, &work).unwrap(),
        (&work[999]) + BigUint::from(999u32)
    );
    let events = index.sequence();
    let restored = EulerWorkIndex::build(&events, &work);
    assert!(restored.valid(&work));
    for h in 0..1000 {
        assert_eq!(index.weight(h, &work), restored.weight(h, &work));
    }
    let before = index.clone();
    assert_eq!(
        index.splice(&[Event::Open(1000), Event::Close(999)], 0, &work),
        None
    );
    assert_eq!(index, before);
    assert!(
        index
            .splice(&[Event::Open(1000), Event::Close(1000)], 0, &work)
            .is_some()
    );
    assert!(index.valid(&work));
    assert_eq!(index.encloses(0, 1000), Some(true));
    assert_eq!(index.encloses(999, 1000), Some(false));
}
#[test]
fn counters_follow_sequence_height_not_chain_depth() {
    let mut measurements = vec![];
    for depth in [200, 400, 800] {
        let mut work = vec![BigUint::from(1u8); depth + 1];
        let mut index = EulerWorkIndex::default();
        index.insert_root(0, &work);
        for h in 1..=depth {
            index.insert_leaf(h, h - 1, &work).unwrap();
        }
        work[depth] += BigUint::from(7u8);
        let updates = index.changed(depth, &work).unwrap();
        let (_, visits) = index.weight_visiting(depth, &work).unwrap();
        assert!(visits <= 2 * index.height(index.root) as usize);
        measurements.push(updates);
    }
    assert!(measurements[2] <= measurements[0] * 2);
    assert!(measurements[2] <= measurements[0] + measurements[1]);
}

#[test]
fn unchanged_pinned_swift_differential() {
    use serde_json::{Value, json};
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/swift-euler.json")).unwrap();
    assert_eq!(
        fixture["sourceCommit"],
        "dda2d852429a27be6043599b63bde81530cd9514"
    );
    let mut work = vec![BigUint::default(); 512];
    let mut index = EulerWorkIndex::default();
    for (step, (row, want)) in fixture["rows"]
        .as_array()
        .unwrap()
        .iter()
        .zip(fixture["expected"].as_array().unwrap())
        .enumerate()
    {
        let h = |key: &str| row[key].as_u64().unwrap() as usize;
        let list = |key: &str| {
            row[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as usize)
                .collect::<Vec<_>>()
        };
        let result = match row["op"].as_str().unwrap() {
            "root" => json!(index.insert_root(h("h"), &work)),
            "leaf" => json!(index.insert_leaf(h("h"), h("parent"), &work)),
            "add" => {
                let at = h("h");
                work[at] +=
                    BigUint::parse_bytes(row["delta"].as_str().unwrap().as_bytes(), 16).unwrap();
                json!(index.changed(at, &work))
            }
            "splice" => {
                let events = row["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| {
                        if e[0] == 0 {
                            Event::Open(e[1].as_u64().unwrap() as usize)
                        } else {
                            Event::Close(e[1].as_u64().unwrap() as usize)
                        }
                    })
                    .collect::<Vec<_>>();
                json!(index.splice(&events, h("parent"), &work))
            }
            "build" => {
                index = EulerWorkIndex::build(&index.sequence(), &work);
                Value::Null
            }
            other => panic!("unknown op {other}"),
        };
        assert_eq!(result, want["result"], "result step {step}");
        assert!(index.valid(&work), "AVL step {step}");
        assert_eq!(want["valid"], true);
        let mut visits = vec![];
        for (q, expected) in list("query")
            .into_iter()
            .zip(want["weights"].as_array().unwrap())
        {
            let got = index.weight_visiting(q, &work);
            visits.push(got.as_ref().map(|(_, v)| *v));
            let expected = expected
                .as_str()
                .map(|v| BigUint::parse_bytes(v.as_bytes(), 16).unwrap());
            assert_eq!(got.map(|(sum, _)| sum), expected, "weight {q}, step {step}");
        }
        assert_eq!(json!(visits), want["visits"], "query counters step {step}");
        let encloses = row["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                index.encloses(
                    pair[0].as_u64().unwrap() as usize,
                    pair[1].as_u64().unwrap() as usize,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(encloses), want["encloses"], "enclosure step {step}");
        assert_eq!(
            json!(index.any_encloses(&list("ancestors"), &list("descendants"))),
            want["any"],
            "batch enclosure step {step}"
        );
        let sequence = index
            .sequence()
            .iter()
            .map(|e| match e {
                Event::Open(h) => [0, *h],
                Event::Close(h) => [1, *h],
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(sequence), want["sequence"], "sequence step {step}");
    }
}

#[test]
fn out_of_domain_handles_fail_without_partial_mutation() {
    let work = vec![BigUint::from(1u8); 2];
    let mut index = EulerWorkIndex::default();
    assert!(!index.insert_root(usize::MAX, &work));
    assert!(!index.insert_root(2, &work));
    assert!(index.insert_root(0, &work));
    let before = index.clone();
    for events in [
        vec![Event::Close(usize::MAX)],
        vec![Event::Open(1), Event::Close(usize::MAX)],
        vec![Event::Open(usize::MAX), Event::Close(usize::MAX)],
    ] {
        assert_eq!(index.splice(&events, 0, &work), None);
        assert_eq!(index, before);
    }
}
