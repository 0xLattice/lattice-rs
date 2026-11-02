//! Production observations for the unchanged Lean Keys and ChildCommit models.
use lattice_cashew::{Header, Volume};
use lattice_primitives::*;
use lattice_validation::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn body(actions: Vec<Action>) -> TransactionBody {
    TransactionBody {
        account_actions: vec![],
        actions,
        deposit_actions: vec![],
        receipt_actions: vec![],
        withdrawal_actions: vec![],
        signers: vec![],
        nonce: 0,
        chain_path: vec!["Nexus".into()],
    }
}
fn block(cid: &str) -> Block {
    Block {
        version: 1,
        parent: None,
        transactions: Header::from_cid(cid).unwrap(),
        target: UInt256([255; 32]),
        next_target: UInt256([0; 32]),
        spec: Volume(Header::from_cid(cid).unwrap()),
        parent_state: Volume(Header::from_cid(cid).unwrap()),
        prev_state: Volume(Header::from_cid(cid).unwrap()),
        post_state: Volume(Header::from_cid(cid).unwrap()),
        children: Header::new(BTreeMap::new()).unwrap(),
        height: 0,
        timestamp: 0,
        reward_recipient: None,
        nonce: 0,
    }
}
fn main() {
    let cid = content_id(&0u64).unwrap();
    let other = content_id(&1u64).unwrap();
    let mut grammar:Vec<Value>=(0..=127u8).map(|c| {let text=String::from_utf8(vec![c]).unwrap();json!({"text":text,"visible":deterministic_atom(&text),"directory":directory_atom(&text),"account":account_atom(&text)})}).collect();
    for text in ["", "A/B", "child/X", "é", "e\u{301}", "a b", "~!@", "🙂"] {
        grammar.push(json!({"text":text,"visible":deterministic_atom(text),"directory":directory_atom(text),"account":account_atom(text)}));
    }
    let mut keys = vec![];
    for demander in ["alice", "a/b", "", "é", "!~@"] {
        for amount in [0, 1, 10, u64::MAX] {
            for nonce in [0, 1, u64::MAX as u128 + 1, u128::MAX] {
                for directory in ["X", "A/B", "", "é"] {
                    let d = DepositKey {
                        demander: demander.into(),
                        amount_demanded: amount,
                        nonce,
                    };
                    let r = ReceiptKey {
                        directory: directory.into(),
                        demander: demander.into(),
                        amount_demanded: amount,
                        nonce,
                    };
                    keys.push(json!({"demander":demander,"directory":directory,"amount":amount.to_string(),"nonce":nonce.to_string(),"deposit":d.to_string(),"receipt":r.to_string(),"path":r.storage_key()}));
                }
            }
        }
    }
    let mut cids = vec![];
    for i in 0..24u64 {
        let c = content_id(&i).unwrap();
        let mut variants = vec![
            c.clone(),
            c.to_uppercase(),
            format!(" {c}"),
            format!("{c}="),
            c[1..].to_string(),
            format!("{}b", &c[..c.len() - 1]),
        ];
        variants.push(lattice_cashew::cid_for_bytes(
            0x55,
            &canonical_bytes(&i).unwrap(),
        ));
        for text in variants {
            cids.push(json!({"text":text,"standard":standard_cid(&text)}));
        }
    }
    let mut prefixes = vec![];
    for version in [0, u16::MAX] {
        for height in [0, u64::MAX] {
            for timestamp in [i64::MIN, -1, 0, i64::MAX] {
                for recipient in [None, Some(""), Some(cid.as_str())] {
                    for parent in [false, true] {
                        let mut b = block(&cid);
                        b.version = version;
                        b.height = height;
                        b.timestamp = timestamp;
                        b.reward_recipient = recipient.map(str::to_owned);
                        if parent {
                            b.parent = Some(Volume(Header::from_cid(&other).unwrap()));
                        }
                        prefixes.push(json!({"block":b,"bytes":b.pow_prefix()}));
                    }
                }
            }
        }
    }
    let mut carriers = vec![];
    for key in ["ordinary", "child/X", "child/", "child/X/Y", "child/é"] {
        for value in [
            None,
            Some(format!("{other}@{cid}")),
            Some(format!("{other}@{other}")),
            Some(other.clone()),
            Some(format!("@{cid}")),
            Some(format!("{other}@")),
            Some(format!("{other}@{cid}@extra")),
        ] {
            for mode in 0..5 {
                for duplicate in [false, true] {
                    let mut b = block(&cid);
                    let index = match mode {
                        0 => BTreeMap::new(),
                        1 | 4 => BTreeMap::from([(
                            "X".into(),
                            Volume(Header::from_cid(&other).unwrap()),
                        )]),
                        2 => {
                            BTreeMap::from([("X".into(), Volume(Header::from_cid(&cid).unwrap()))])
                        }
                        _ => BTreeMap::from([(
                            "X/Y".into(),
                            Volume(Header::from_cid(&other).unwrap()),
                        )]),
                    };
                    b.children = Header::new(index.clone()).unwrap();
                    let well_formed = b.children_are_well_formed().unwrap();
                    if mode == 4 {
                        b.children = b.children.removing_node();
                    }
                    let action = Action {
                        key: key.into(),
                        old_value: Some("old".into()),
                        new_value: value.clone(),
                    };
                    let bodies = if duplicate {
                        vec![body(vec![action.clone()]), body(vec![action.clone()])]
                    } else {
                        vec![body(vec![action.clone()])]
                    };
                    let actions = TransitionActions::from_bodies(&bodies);
                    let previous = ResolvedState {
                        general: BTreeMap::from([(key.into(), "old".into())]),
                        ..Default::default()
                    };
                    let state = execute_block(
                        &previous,
                        &BTreeMap::new(),
                        "Nexus",
                        &actions,
                        &bodies,
                        0,
                        None,
                    );
                    let commitment = valid_child_commitments(&b, &bodies);
                    carriers.push(json!({"key":key,"value":value,"actions":actions.general,"children":index.iter().map(|(d,h)|(d,h.0.raw_cid.clone())).collect::<Vec<_>>(),"previous":cid,"loaded":mode!=4,"wellFormed":well_formed,"commitment":commitment.as_ref().ok(),"error":commitment.err().map(|e|e.to_string()),"stateSucceeded":state.is_ok(),"stateError":state.err().map(|e|e.to_string())}));
                }
            }
        }
    }
    println!(
        "{}",
        json!({"grammar":grammar,"keys":keys,"cids":cids,"prefixes":prefixes,"carriers":carriers})
    );
}
