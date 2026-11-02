use crate::*;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionBody {
    pub account_actions: Vec<AccountAction>,
    pub actions: Vec<Action>,
    pub deposit_actions: Vec<DepositAction>,
    pub receipt_actions: Vec<ReceiptAction>,
    pub withdrawal_actions: Vec<WithdrawalAction>,
    pub signers: Vec<String>,
    pub nonce: u64,
    pub chain_path: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountBalanceDelta {
    Credit(u64),
    Debit(u64),
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    #[error("balance overflow")]
    BalanceOverflow,
    #[error("nonce gap")]
    NonceGap,
    #[error("conflicting actions")]
    ConflictingActions,
    #[error("state delta overflow")]
    StateDeltaOverflow,
}
pub fn next_expected_nonce(stored: Option<u64>) -> Result<u64, StateError> {
    stored.map_or(Ok(0), |v| v.checked_add(1).ok_or(StateError::NonceGap))
}
pub fn signing_preimage(body_cid: &str, chain_path: &[String], nonce: u64) -> String {
    let mut lines = vec![
        "domain:13:lattice-tx-v1".to_string(),
        format!("chainPath.count:{}", chain_path.len()),
    ];
    for component in chain_path {
        lines.push(format!(
            "chainPath.component:{}:{}",
            component.len(),
            component
        ));
    }
    lines.push(format!("nonce:{nonce}"));
    lines.push(format!("bodyCID:{}:{body_cid}", body_cid.len()));
    lines.join("\n")
}
impl TransactionBody {
    pub fn signing_preimage(&self) -> Result<String, lattice_cashew::Error> {
        Ok(signing_preimage(
            &content_id(self)?,
            &self.chain_path,
            self.nonce,
        ))
    }
    pub fn state_atoms_are_valid(&self) -> bool {
        self.signers.iter().all(|s| account_atom(s))
            && self.chain_path.iter().all(|s| directory_atom(s))
            && self.account_actions.iter().all(|a| account_atom(&a.owner))
            && self.actions.iter().all(|a| deterministic_atom(&a.key))
            && self
                .deposit_actions
                .iter()
                .all(|a| account_atom(&a.demander))
            && self.receipt_actions.iter().all(|a| {
                account_atom(&a.demander)
                    && account_atom(&a.withdrawer)
                    && directory_atom(&a.directory)
            })
            && self
                .withdrawal_actions
                .iter()
                .all(|a| account_atom(&a.demander) && account_atom(&a.withdrawer))
    }
    pub fn account_actions_are_valid(&self) -> bool {
        self.account_actions
            .iter()
            .all(|a| a.verify() && (a.delta > 0 || self.signers.contains(&a.owner)))
    }
    pub fn deposit_actions_are_valid(&self) -> bool {
        self.deposit_actions.iter().all(|a| {
            a.amount_deposited > 0 && a.amount_demanded > 0 && self.signers.contains(&a.demander)
        })
    }
    pub fn receipt_actions_are_valid(&self) -> bool {
        self.receipt_actions
            .iter()
            .all(|a| a.amount_demanded > 0 && self.signers.contains(&a.withdrawer))
    }
    pub fn withdrawal_actions_are_valid(&self) -> bool {
        self.withdrawal_actions.iter().all(|a| {
            a.amount_withdrawn > 0 && a.amount_demanded > 0 && self.signers.contains(&a.withdrawer)
        })
    }
    pub fn miner_surplus(&self) -> Option<lattice_pow::WorkSum> {
        let mut destroyed = BigUint::default();
        let mut created = BigUint::default();
        for a in &self.account_actions {
            if !a.verify() {
                return None;
            }
            if a.delta < 0 {
                destroyed += a.absolute_amount()
            } else {
                created += a.absolute_amount()
            }
        }
        for a in &self.withdrawal_actions {
            destroyed += a.amount_withdrawn
        }
        for a in &self.deposit_actions {
            created += a.amount_deposited
        }
        if destroyed < created {
            None
        } else {
            lattice_pow::WorkSum::from_hex(&(destroyed - created).to_str_radix(16))
        }
    }
    pub fn net_balance_deltas(&self) -> Result<BTreeMap<String, AccountBalanceDelta>, StateError> {
        net_balance_deltas(&self.account_actions, &self.receipt_actions)
    }
    pub fn withdrawals_have_unique_receipt_keys(bodies: &[Self], directory: &str) -> bool {
        let mut keys = BTreeSet::new();
        bodies.iter().flat_map(|b| &b.withdrawal_actions).all(|a| {
            keys.insert(
                ReceiptKey {
                    directory: directory.into(),
                    demander: a.demander.clone(),
                    amount_demanded: a.amount_demanded,
                    nonce: a.nonce,
                }
                .to_string(),
            )
        })
    }
}
pub fn net_balance_deltas(
    accounts: &[AccountAction],
    receipts: &[ReceiptAction],
) -> Result<BTreeMap<String, AccountBalanceDelta>, StateError> {
    let mut totals: BTreeMap<String, (BigUint, BigUint)> = BTreeMap::new();
    let mut add = |owner: &str, amount: u64, credit: bool| {
        let entry = totals.entry(owner.into()).or_default();
        if credit {
            entry.0 += amount
        } else {
            entry.1 += amount
        }
    };
    for a in accounts {
        if !a.verify() {
            return Err(StateError::BalanceOverflow);
        }
        add(&a.owner, a.absolute_amount(), a.delta > 0);
    }
    for a in receipts {
        if a.amount_demanded == 0 {
            return Err(StateError::BalanceOverflow);
        }
        add(&a.withdrawer, a.amount_demanded, false);
        add(&a.demander, a.amount_demanded, true);
    }
    let mut result = BTreeMap::new();
    for (owner, (credits, debits)) in totals {
        if credits == debits {
            continue;
        }
        let credit = credits > debits;
        let value = if credit {
            credits - debits
        } else {
            debits - credits
        };
        let amount = u64::try_from(value).map_err(|_| StateError::BalanceOverflow)?;
        result.insert(
            owner,
            if credit {
                AccountBalanceDelta::Credit(amount)
            } else {
                AccountBalanceDelta::Debit(amount)
            },
        );
    }
    Ok(result)
}

/// Swift stores signatures as a sorted array of key/value entries on the wire.
/// Decoding rejects collisions after Multikey normalization.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub signatures: BTreeMap<String, String>,
    pub body: lattice_cashew::Header<TransactionBody>,
}
#[derive(Serialize, Deserialize)]
struct SignatureEntry {
    key: String,
    value: String,
}
fn normalized_key(key: &str) -> String {
    lattice_multikey::Multikey::from_hex(key)
        .map(|key| key.hex_encoded())
        .unwrap_or_else(|_| key.into())
}
fn normalized_signature(value: &str) -> String {
    crate::crypto::decode_hex(value)
        .map(hex::encode)
        .unwrap_or_else(|| value.into())
}
impl Transaction {
    pub fn new(
        signatures: BTreeMap<String, String>,
        body: lattice_cashew::Header<TransactionBody>,
    ) -> Self {
        let mut normalized = BTreeMap::new();
        for (key, value) in &signatures {
            if normalized
                .insert(normalized_key(key), normalized_signature(value))
                .is_some()
            {
                // Match Swift's nonthrowing constructor: preserve original input
                // if normalization would discard one of its keys.
                return Self { signatures, body };
            }
        }
        Self {
            signatures: normalized,
            body,
        }
    }
}
impl Serialize for Transaction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let entries: Vec<_> = self
            .signatures
            .iter()
            .map(|(key, value)| SignatureEntry {
                key: key.clone(),
                value: value.clone(),
            })
            .collect();
        let mut state = serializer.serialize_struct("Transaction", 2)?;
        state.serialize_field("signatures", &entries)?;
        state.serialize_field("body", &self.body)?;
        state.end()
    }
}
impl<'de> Deserialize<'de> for Transaction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            signatures: Vec<SignatureEntry>,
            body: lattice_cashew::Header<TransactionBody>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let mut signatures = BTreeMap::new();
        for entry in wire.signatures {
            if signatures
                .insert(
                    normalized_key(&entry.key),
                    normalized_signature(&entry.value),
                )
                .is_some()
            {
                return Err(serde::de::Error::custom("duplicate signature key"));
            }
        }
        Ok(Self {
            signatures,
            body: wire.body,
        })
    }
}

impl Transaction {
    /// Normalize without silently discarding aliases of the same public key.
    pub fn normalized_signatures(&self) -> Option<BTreeMap<String, String>> {
        let mut result = BTreeMap::new();
        for (key, value) in &self.signatures {
            if result
                .insert(normalized_key(key), normalized_signature(value))
                .is_some()
            {
                return None;
            }
        }
        Some(result)
    }
}
