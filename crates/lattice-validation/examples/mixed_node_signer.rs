//! Public RFC 8032 identity, exclusively for disposable local integration chains.
use lattice_primitives::{AccountAction, TransactionBody, crypto};
use serde_json::json;
const PRIVATE: &str = "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
const PUBLIC: &str = "ed013d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let owner = crypto::create_address(PUBLIC).unwrap();
    if args.len() == 1 {
        println!("{}", json!({"address":owner,"publicKey":PUBLIC}));
        return;
    }
    if args[1] == "--mine" {
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&args[2]).unwrap()).unwrap();
        let block: lattice_primitives::Block =
            serde_json::from_value(value["block"].clone()).unwrap();
        let target: lattice_primitives::UInt256 =
            serde_json::from_value(value["searchTarget"].clone()).unwrap();
        let prefix = block.pow_prefix();
        for nonce in 0..1_000_000u64 {
            let hash = lattice_pow::hash(&prefix, nonce);
            if lattice_pow::validate(&hash, &target.0) {
                println!("{}", json!({"nonce":nonce,"hash":hash,"prefix":prefix}));
                return;
            }
        }
        panic!("fixture work exceeded bounded search");
    }
    let nonce: u64 = args[1].parse().unwrap();
    let amount: i64 = args[2].parse().unwrap();
    assert!(amount > 0);
    let body = TransactionBody {
        account_actions: vec![
            AccountAction {
                owner: owner.clone(),
                delta: -amount - 1,
            },
            AccountAction {
                owner: args[3].clone(),
                delta: amount,
            },
        ],
        actions: vec![],
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![owner],
        nonce,
        chain_path: args.get(4).map_or_else(
            || vec!["Nexus".into()],
            |path| path.split('/').map(str::to_owned).collect(),
        ),
    };
    let message = body.signing_preimage().unwrap();
    let signature = crypto::sign(&message, PRIVATE).unwrap();
    assert!(crypto::verify(&message, &signature, PUBLIC));
    let mut wire = serde_json::to_value(body).unwrap();
    wire["nonce"] = json!(nonce.to_string());
    for action in wire["accountActions"].as_array_mut().unwrap() {
        action["delta"] = json!(action["delta"].as_i64().unwrap().to_string());
    }
    println!(
        "{}",
        json!({"transaction":{"body":wire,"signatures":{PUBLIC:signature}}})
    );
}
