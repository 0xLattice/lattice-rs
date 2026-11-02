use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountAction {
    pub owner: String,
    pub delta: i64,
}
impl AccountAction {
    pub fn verify(&self) -> bool {
        self.delta != 0 && self.delta != i64::MIN
    }
    pub fn absolute_amount(&self) -> u64 {
        self.delta.unsigned_abs()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_value: Option<String>,
}
impl Action {
    pub fn verify(&self) -> bool {
        !self.key.is_empty() && (self.old_value.is_some() || self.new_value.is_some())
    }
    pub fn state_delta(&self) -> Option<i64> {
        if !self.verify() {
            return None;
        }
        let key = i64::try_from(self.key.len()).ok()?;
        let old = i64::try_from(self.old_value.as_ref().map_or(0, String::len)).ok()?;
        let new = i64::try_from(self.new_value.as_ref().map_or(0, String::len)).ok()?;
        match (&self.old_value, &self.new_value) {
            (None, Some(_)) => new.checked_add(key),
            (Some(_), None) => old.checked_add(key)?.checked_neg(),
            _ => new.checked_sub(old),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositAction {
    pub nonce: u128,
    pub demander: String,
    pub amount_demanded: u64,
    pub amount_deposited: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptAction {
    pub withdrawer: String,
    pub nonce: u128,
    pub demander: String,
    pub amount_demanded: u64,
    pub directory: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawalAction {
    pub withdrawer: String,
    pub nonce: u128,
    pub demander: String,
    pub amount_demanded: u64,
    pub amount_withdrawn: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositKey {
    pub demander: String,
    pub amount_demanded: u64,
    pub nonce: u128,
}
impl std::fmt::Display for DepositKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}/{}",
            self.demander, self.amount_demanded, self.nonce
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptKey {
    pub directory: String,
    pub demander: String,
    pub amount_demanded: u64,
    pub nonce: u128,
}
impl std::fmt::Display for ReceiptKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}/{}/{}",
            self.directory, self.demander, self.amount_demanded, self.nonce
        )
    }
}
impl ReceiptKey {
    pub fn storage_key(&self) -> String {
        crate::crypto::sha256(&format!("lattice/receipt-state/v1\0{self}"))
    }
}
