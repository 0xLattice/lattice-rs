//! Actual authenticated receipt/value execution and canonical trie observations.
use lattice_cashew::{
    ContentSource, Header, MerkleDictionary, RadixNode, Transform, Volume, VolumeMerkleDictionary,
    verify_content_address,
};
use lattice_primitives::*;
use lattice_validation::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
type Tree = MerkleDictionary<String>;
fn chars(s: &str) -> Vec<u8> {
    assert!(s.bytes().all(|b| (0x21..=0x7e).contains(&b)));
    s.bytes().collect()
}
fn child_key(s: &str) -> u8 {
    let c = chars(s);
    assert_eq!(c.len(), 1);
    c[0]
}
fn record(cid: &str, bytes: Vec<u8>, enc: Value, table: &mut BTreeMap<String, Value>) {
    verify_content_address(cid, &bytes).unwrap();
    let id = enc.to_string();
    let row = json!({"enc":enc,"cid":cid,"bytes":bytes});
    if let Some(old) = table.insert(id, row.clone()) {
        assert_eq!(old, row);
    }
}
fn node(h: &Header<RadixNode<String>>, table: &mut BTreeMap<String, Value>) -> Value {
    if let Some(n) = &h.node {
        let kids: Vec<_> = n
            .children
            .iter()
            .map(|e| json!([child_key(&e.key), node(&e.value, table)]))
            .collect();
        record(
            &h.raw_cid,
            h.bytes(None).unwrap(),
            json!({"node":[chars(&n.prefix),n.value,n.children.iter().map(|e|json!([child_key(&e.key),e.value.raw_cid])).collect::<Vec<_>>()]}),
            table,
        );
        json!({"node":[chars(&n.prefix),n.value,kids]})
    } else {
        json!({"stub":h.raw_cid})
    }
}
fn root(t: &Tree, table: &mut BTreeMap<String, Value>) -> Value {
    let kids: Vec<_> = t
        .children
        .iter()
        .map(|e| json!([child_key(&e.key), node(&e.value, table)]))
        .collect();
    let h = Header::new(t.clone()).unwrap();
    record(
        &h.raw_cid,
        h.bytes(None).unwrap(),
        json!({"root":[t.count,t.children.iter().map(|e|json!([child_key(&e.key),e.value.raw_cid])).collect::<Vec<_>>()]}),
        table,
    );
    json!({"count":t.count,"kids":kids})
}

fn key(a: &ReceiptAction) -> ReceiptKey {
    ReceiptKey {
        directory: a.directory.clone(),
        demander: a.demander.clone(),
        amount_demanded: a.amount_demanded,
        nonce: a.nonce,
    }
}
fn action(a: &ReceiptAction) -> Value {
    json!({"withdrawer":a.withdrawer,"directory":a.directory,"demander":a.demander,"amount":a.amount_demanded,"nonce":a.nonce.to_string(),"path":key(a).storage_key(),"text":key(a).to_string()})
}
struct Recording {
    entries: BTreeMap<String, Vec<u8>>,
    reads: Mutex<BTreeSet<String>>,
}
impl ContentSource for Recording {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
        self.reads.lock().unwrap().insert(cid.into());
        self.entries.fetch(cid)
    }
}
fn main() {
    let mut table = BTreeMap::new();
    let mut rows = vec![];
    let mut boundaries = vec![];
    let universe: Vec<_> = [0, 1, u128::MAX, 2]
        .into_iter()
        .enumerate()
        .map(|(i, nonce)| ReceiptAction {
            withdrawer: "withdrawer".into(),
            demander: "demander".into(),
            directory: if i == 3 { "Beta" } else { "Alpha" }.into(),
            amount_demanded: (i + 1) as u64,
            nonce,
        })
        .collect();
    for mask in 0..8 {
        for balance in [0, 100] {
            let mut state = LatticeState::empty().unwrap();
            let mut before = Tree::default();
            let mut resolved = ResolvedState::default();
            if balance > 0 {
                resolved.accounts.insert("withdrawer".into(), balance);
            }
            resolved.accounts.insert("demander".into(), 10);
            let mut accounts = VolumeMerkleDictionary::default();
            for (k, v) in &resolved.accounts {
                accounts = accounts.inserting(k, *v).unwrap();
            }
            state.account_state = Volume::new(accounts).unwrap();
            let mut existing = vec![];
            for (i, a) in universe.iter().take(3).enumerate() {
                if mask & (1 << i) != 0 {
                    let k = key(a).storage_key();
                    before = before.inserting(&k, "old-owner".into()).unwrap();
                    resolved.receipts.insert(k, "old-owner".into());
                    existing.push(json!({"action":action(a),"value":"old-owner"}));
                }
            }
            state.receipt_state =
                Volume::new(VolumeMerkleDictionary::from_ordinary(&before)).unwrap();
            let previous = Volume::new(state).unwrap();
            let source: BTreeMap<_, _> =
                lattice_cashew::volume::serialize_recursive(&previous, &())
                    .unwrap()
                    .into_iter()
                    .flat_map(|v| v.entries)
                    .collect();
            let unresolved = previous.removing_node();
            let mut zero = universe[3].clone();
            zero.amount_demanded = 0;
            let batches = vec![
                vec![],
                vec![universe[0].clone()],
                vec![universe[3].clone()],
                vec![universe[0].clone(), universe[3].clone()],
                vec![universe[3].clone(), universe[3].clone()],
                vec![zero],
                vec![universe[1].clone()],
                vec![universe[2].clone()],
            ];
            for (batch_id, receipts) in batches.into_iter().enumerate() {
                let actions = TransitionActions {
                    receipts: receipts.clone(),
                    ..Default::default()
                };
                let execution = AuthenticatedBlockExecution {
                    previous: &unresolved,
                    parent: &unresolved,
                    directory: "Alpha",
                    actions: &actions,
                    transaction_bodies: &[],
                    reward: 0,
                    recipient: None,
                };
                let recording = Recording {
                    entries: source.clone(),
                    reads: Mutex::default(),
                };
                let actual = execution.execute(&recording);
                let pure =
                    execute_block(&resolved, &BTreeMap::new(), "Alpha", &actions, &[], 0, None);
                assert_eq!(actual.is_ok(), pure.is_ok());
                let mut operations = BTreeMap::new();
                for a in &receipts {
                    operations.insert(
                        key(a).storage_key(),
                        Transform::Insert(a.withdrawer.clone()),
                    );
                }
                let proof = if actual.is_ok() {
                    Some(before.prove_transforms(&operations, &source).unwrap())
                } else {
                    None
                };
                let result=actual.as_ref().ok().map(|out|{
    let state=out.state.node.as_ref().unwrap();let rt=state.receipt_state.resolve(&source,None).unwrap().node.as_ref().unwrap().resolve_all(&source).unwrap().to_ordinary();
    let values=state.account_state.resolve(&source,None).unwrap().node.as_ref().unwrap().resolve_all(&source).unwrap();
    let expected=pure.as_ref().unwrap();let observed_accounts=values.to_ordinary().entries().unwrap();let observed_receipts=rt.entries().unwrap();assert_eq!(observed_accounts,expected.state.accounts);assert_eq!(observed_receipts,expected.state.receipts);
    let mut rebuilt=Tree::default();for(k,v)in &expected.state.receipts{rebuilt=rebuilt.inserting(k,v.clone()).unwrap();}assert_eq!(Header::new(rebuilt).unwrap().raw_cid,state.receipt_state.raw_cid);
    json!({"tree":root(&rt,&mut table),"cid":state.receipt_state.raw_cid,"accounts":observed_accounts,"receipts":observed_receipts,"stateCID":out.state.raw_cid})
   });
                if mask == 0 && balance == 100 && batch_id == 2 {
                    for cid in recording.reads.lock().unwrap().iter() {
                        for corrupt in [false, true] {
                            let mut altered = source.clone();
                            if corrupt {
                                altered.get_mut(cid).unwrap()[0] ^= 1;
                            } else {
                                altered.remove(cid);
                            }
                            let outcome = execution.execute(&altered);
                            assert!(outcome.is_err());
                            boundaries.push(json!({"kind":if corrupt{"corrupt"}else{"missing"},"cid":cid,"accepted":outcome.is_ok(),"error":outcome.err().map(|e|e.to_string())}));
                        }
                    }
                    let mut wrong = previous.clone();
                    let mut content = wrong.node.as_deref().unwrap().clone();
                    content.receipt_state = Volume::new(VolumeMerkleDictionary::from_ordinary(
                        &before.inserting("forged", "owner".into()).unwrap(),
                    ))
                    .unwrap();
                    wrong.0.node = Some(std::sync::Arc::new(content));
                    let forged = AuthenticatedBlockExecution {
                        previous: &wrong,
                        ..execution
                    };
                    let outcome = forged.execute(&source);
                    assert!(outcome.is_err());
                    boundaries.push(json!({"kind":"materialized-mismatch","accepted":outcome.is_ok(),"error":outcome.err().map(|e|e.to_string())}));
                }
                rows.push(json!({"name":format!("mask{mask}-balance{balance}-batch{batch_id}"),"before":root(&before,&mut table),"beforeCID":previous.node.as_ref().unwrap().receipt_state.raw_cid,"accounts":resolved.accounts,"existing":existing,"actions":receipts.iter().map(action).collect::<Vec<_>>(),"proof":proof.as_ref().map(|p|root(p,&mut table)),"result":result}));
            }
        }
    }
    println!(
        "{}",
        json!({"cases":rows,"encodings":table.into_values().collect::<Vec<_>>(),"boundaries":boundaries})
    );
}
