use lattice_pow::{
    asert_target, hash, validate, work_for_hash, work_for_target, PreimageFields, Target, WorkSum,
};
use serde::Deserialize;

fn target(hex: &str) -> Target {
    WorkSum::from_hex(hex).unwrap().as_target().unwrap()
}

#[derive(Deserialize)]
struct Vector {
    anchor: String,
    anchor_timestamp: i64,
    anchor_height: u64,
    timestamp: i64,
    height: u64,
    target_block_time: u64,
    half_life: u64,
    asert: String,
    work_for_target: String,
    work_for_hash: String,
}
#[derive(Deserialize)]
struct Corpus {
    vectors: Vec<Vector>,
}

#[test]
fn matches_pinned_swift_arithmetic() {
    let corpus: Corpus = serde_json::from_str(include_str!("swift-vectors.json")).unwrap();
    assert!(corpus.vectors.len() >= 300);
    for (i, v) in corpus.vectors.iter().enumerate() {
        let anchor = target(&v.anchor);
        assert_eq!(
            asert_target(
                anchor,
                v.anchor_timestamp,
                v.anchor_height,
                v.timestamp,
                v.height,
                v.target_block_time,
                v.half_life
            ),
            target(&v.asert),
            "ASERT vector {i}"
        );
        assert_eq!(
            work_for_target(&anchor),
            target(&v.work_for_target),
            "target work {i}"
        );
        assert_eq!(
            work_for_hash(&anchor),
            target(&v.work_for_hash),
            "hash work {i}"
        );
    }
}

#[test]
fn exact_work_survives_256_and_320_bits() {
    let max = WorkSum::from([255; 32]);
    let one = WorkSum::from_hex("1").unwrap();
    let sum = &max + &one;
    assert_eq!(sum.to_hex(), format!("1{}", "0".repeat(64)));
    assert!(sum > max);
    assert_eq!(sum.as_target(), None);
    assert_eq!(sum.checked_sub(&one), Some(max));
    let large = WorkSum::from_hex(&"f".repeat(80)).unwrap();
    let grown = &large + &one;
    assert_eq!(grown.to_hex(), format!("1{}", "0".repeat(80)));
    assert_eq!(grown.checked_sub(&large), Some(one));
    assert_eq!(WorkSum::default().checked_sub(&grown), None);
}

#[test]
fn work_serializes_as_swift_hex_string() {
    let one = WorkSum::from_hex("0001").unwrap();
    let json = serde_json::to_string(&one).unwrap();
    assert_eq!(json, format!("\"{}1\"", "0".repeat(63)));
    assert_eq!(serde_json::from_str::<WorkSum>(&json).unwrap(), one);
    for invalid in ["", "-1", "+1", "0x01", "gg", " 1", "１"] {
        assert!(WorkSum::from_hex(invalid).is_none());
    }
    assert!(serde_json::from_str::<WorkSum>("1").is_err());
}

#[test]
fn inclusive_work_and_zero_edges() {
    let zero = [0; 32];
    let one = target("1");
    assert!(!validate(&zero, &zero));
    assert!(validate(&one, &one));
    assert!(!validate(&target("2"), &one));
    assert_eq!(work_for_target(&zero), zero);
    assert_eq!(work_for_hash(&zero), [255; 32]);
    assert_eq!(work_for_target(&[255; 32]), one);
    assert_eq!(
        work_for_target(&one),
        target(&format!("8{}", "0".repeat(63)))
    );
}

#[test]
fn reward_presence_and_all_fields_are_bound() {
    let mut f = PreimageFields {
        version: 1,
        parent: None,
        transactions: "tx",
        target: [255; 32],
        next_target: [255; 32],
        spec: "spec",
        parent_state: "parent",
        prev_state: "prev",
        post_state: "post",
        children: "children",
        height: 1,
        timestamp: -1,
        reward_recipient: None,
    };
    let absent = f.prefix();
    let expected = format!(
        "1\0\0tx\0{}\0{}\0spec\0parent\0prev\0post\0children\0\x31\0-1\0\0",
        "f".repeat(64),
        "f".repeat(64)
    );
    assert_eq!(absent, expected.as_bytes());
    f.reward_recipient = Some("");
    let empty = f.prefix();
    assert_ne!(empty, absent);
    assert!(empty.ends_with(&[0, 1, 0]));
    f.reward_recipient = Some("ed01abcd");
    assert!(f.prefix().ends_with(b"\x01ed01abcd\0"));
    f.version = 2;
    assert!(f.prefix().starts_with(b"2\0"));
}

#[test]
fn nonce_is_exactly_eight_big_endian_bytes() {
    // Python hashlib.sha256(b"\xde\xad\xbe\xef" + (12345).to_bytes(8,"big")).
    assert_eq!(
        hash(&[0xde, 0xad, 0xbe, 0xef], 12345),
        target("97287fdce12fb701286532d9809d7028a1e551ae1e8f0b98e1a5d8fc24c203e7")
    );
}

#[test]
fn prefixes_match_unmodified_swift_function() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("swift-vectors.json")).unwrap();
    let expected = corpus["prefixes"].as_array().unwrap();
    for (recipient, encoded) in [None, Some(""), Some("ed01abcd"), Some("é/支付")]
        .into_iter()
        .zip(expected)
    {
        let fields = PreimageFields {
            version: u16::MAX,
            parent: None,
            transactions: "tx",
            target: [255; 32],
            next_target: target("1"),
            spec: "spec",
            parent_state: "parent",
            prev_state: "prev",
            post_state: "post",
            children: "children",
            height: u64::MAX,
            timestamp: i64::MIN,
            reward_recipient: recipient,
        };
        let actual: String = fields.prefix().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(actual, encoded.as_str().unwrap());
    }
    assert_eq!(expected.len(), 4);
}
