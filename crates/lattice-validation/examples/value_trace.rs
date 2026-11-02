//! Replay formal/Swift inputs through the production state executor.
use lattice_primitives::*;
use lattice_validation::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn deposit_key(v: &Value) -> Result<DepositKey, Box<dyn std::error::Error>> {
    Ok(DepositKey {
        demander: string(v, "demander")?,
        amount_demanded: number(v, "amountDemanded")?,
        nonce: number(v, "nonce")? as u128,
    })
}
fn receipt_key(v: &Value) -> Result<ReceiptKey, Box<dyn std::error::Error>> {
    Ok(ReceiptKey {
        directory: string(v, "directory")?,
        demander: string(v, "demander")?,
        amount_demanded: number(v, "amountDemanded")?,
        nonce: number(v, "nonce")? as u128,
    })
}
fn string(v: &Value, key: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(v[key]
        .as_str()
        .ok_or_else(|| format!("missing string {key}"))?
        .into())
}
fn number(v: &Value, key: &str) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(v[key]
        .as_u64()
        .ok_or_else(|| format!("invalid u64 {key}"))?)
}
fn array(v: &Value) -> Result<&Vec<Value>, Box<dyn std::error::Error>> {
    Ok(v.as_array().ok_or("expected array")?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("expected trace input path")?;
    let mut rows: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    for row in &mut rows {
        let mut previous = ResolvedState::default();
        let mut deposit_names = BTreeMap::new();
        let mut receipt_names = BTreeMap::new();
        for entry in array(&row["prev"]["balances"])? {
            let key = entry[0].as_str().ok_or("balance key")?.to_string();
            if previous
                .accounts
                .insert(key, entry[1].as_u64().ok_or("balance value")?)
                .is_some()
            {
                return Err("duplicate balance".into());
            }
        }
        for entry in array(&row["prev"]["deposits"])? {
            let key = deposit_key(&entry[0])?.to_string();
            deposit_names.insert(key.clone(), entry[0].clone());
            if previous
                .deposits
                .insert(key, entry[1].as_u64().ok_or("deposit value")?)
                .is_some()
            {
                return Err("duplicate deposit".into());
            }
        }
        for entry in array(&row["prev"]["receipts"])? {
            let key = receipt_key(&entry[0])?.storage_key();
            receipt_names.insert(key.clone(), entry[0].clone());
            if previous
                .receipts
                .insert(key, entry[1].as_str().ok_or("receipt value")?.into())
                .is_some()
            {
                return Err("duplicate receipt".into());
            }
        }
        let mut parents = BTreeMap::new();
        for entry in array(&row["parentReceipts"])? {
            if parents
                .insert(
                    receipt_key(&entry[0])?.storage_key(),
                    entry[1].as_str().ok_or("parent receipt value")?.into(),
                )
                .is_some()
            {
                return Err("duplicate parent receipt".into());
            }
        }
        let mut actions = TransitionActions::default();
        for v in array(&row["block"]["actions"])? {
            match v["kind"].as_str().ok_or("action kind")? {
                "account" => actions.accounts.push(serde_json::from_value(v.clone())?),
                "deposit" => {
                    let key = deposit_key(v)?;
                    deposit_names.insert(key.to_string(), json!({"demander":key.demander,"amountDemanded":key.amount_demanded,"nonce":key.nonce}));
                    actions.deposits.push(serde_json::from_value(v.clone())?);
                }
                "withdrawal" => actions.withdrawals.push(serde_json::from_value(v.clone())?),
                "receipt" => {
                    let key = receipt_key(v)?;
                    receipt_names.insert(key.storage_key(), json!({"directory":key.directory,"demander":key.demander,"amountDemanded":key.amount_demanded,"nonce":key.nonce}));
                    actions.receipts.push(serde_json::from_value(v.clone())?);
                }
                _ => return Err("unknown action".into()),
            }
        }
        let result = execute_block(
            &previous,
            &parents,
            &string(row, "dir")?,
            &actions,
            &[],
            number(&row["block"], "reward")?,
            row["block"]["recipient"].as_str(),
        );
        row["result"] = match result {
            Err(_) => Value::Null,
            Ok(result) => {
                let balances: Vec<_> = result.state.accounts.into_iter().collect();
                let deposits: Vec<_> = result
                    .state
                    .deposits
                    .into_iter()
                    .map(|(k, v)| (deposit_names[&k].clone(), v))
                    .collect();
                let receipts: Vec<_> = result
                    .state
                    .receipts
                    .into_iter()
                    .map(|(k, v)| (receipt_names[&k].clone(), v))
                    .collect();
                json!({"balances":balances,"deposits":deposits,"receipts":receipts})
            }
        };
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
