use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PolicyScope {
    Transaction,
    Action,
}
impl PolicyScope {
    pub fn default_entrypoint(self) -> &'static str {
        match self {
            Self::Transaction => "lattice_validate_transaction",
            Self::Action => "lattice_validate_action",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmPolicyRef {
    #[serde(rename = "moduleCID")]
    pub module_cid: String,
    #[serde(rename = "sourceCID", skip_serializing_if = "Option::is_none")]
    pub source_cid: Option<String>,
    pub abi_version: u16,
    pub scope: PolicyScope,
    pub entrypoint: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainSpec {
    pub max_number_of_transactions_per_block: u64,
    pub max_state_growth: i64,
    pub max_block_size: i64,
    pub premine: u64,
    pub target_block_time: u64,
    pub initial_reward: u64,
    pub halving_interval: u64,
    pub half_life: u64,
    pub wasm_policies: Vec<WasmPolicyRef>,
}
impl<'de> Deserialize<'de> for ChainSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Fields {
            max_number_of_transactions_per_block: u64,
            max_state_growth: i64,
            max_block_size: i64,
            premine: u64,
            target_block_time: u64,
            initial_reward: u64,
            halving_interval: u64,
            half_life: u64,
            wasm_policies: Vec<WasmPolicyRef>,
        }
        let value = serde_json::Value::deserialize(d)?;
        if value.get("transactionFilters").is_some() || value.get("actionFilters").is_some() {
            return Err(serde::de::Error::custom(
                "legacy JavaScript filters are not supported",
            ));
        }
        let f: Fields = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self {
            max_number_of_transactions_per_block: f.max_number_of_transactions_per_block,
            max_state_growth: f.max_state_growth,
            max_block_size: f.max_block_size,
            premine: f.premine,
            target_block_time: f.target_block_time,
            initial_reward: f.initial_reward,
            halving_interval: f.halving_interval,
            half_life: f.half_life,
            wasm_policies: f.wasm_policies,
        })
    }
}
impl ChainSpec {
    pub fn is_valid(&self) -> bool {
        self.max_number_of_transactions_per_block > 0
            && self.max_state_growth > 0
            && self.max_block_size > 0
            && self.target_block_time > 0
            && self.initial_reward > 0
            && self.halving_interval > 0
            && self.half_life > 0
    }
    pub fn reward_at_block(&self, height: u64) -> u64 {
        let Some(offset) = height.checked_add(self.premine) else {
            return 0;
        };
        if self.halving_interval == 0 {
            return 0;
        }
        let halves = offset / self.halving_interval;
        if halves >= 64 {
            0
        } else {
            self.initial_reward >> halves
        }
    }
    fn rewards_interval(&self, start: u64, count: u64) -> u64 {
        if self.halving_interval == 0 {
            return 0;
        }
        let mut processed = 0u64;
        let mut total = 0u64;
        while processed < count {
            let Some(offset) = start.checked_add(processed) else {
                break;
            };
            let halves = offset / self.halving_interval;
            if halves >= 64 {
                break;
            }
            let reward = self.initial_reward >> halves;
            if reward == 0 {
                break;
            }
            let boundary = (halves as u128 + 1) * self.halving_interval as u128;
            let length = if boundary > u64::MAX as u128 {
                count - processed
            } else {
                (boundary as u64 - offset).min(count - processed)
            };
            if length == 0 {
                break;
            }
            total = total.saturating_add(reward.saturating_mul(length));
            processed += length;
        }
        total
    }
    pub fn total_rewards(&self, count: u64) -> u64 {
        self.rewards_interval(self.premine, count)
    }
    pub fn premine_amount(&self) -> u64 {
        self.rewards_interval(0, self.premine)
    }
    pub fn total_halvings(&self) -> u64 {
        (64 - self.initial_reward.leading_zeros()) as u64
    }
    pub fn asert_target(
        &self,
        anchor: crate::UInt256,
        anchor_timestamp: i64,
        anchor_height: u64,
        timestamp: i64,
        height: u64,
    ) -> crate::UInt256 {
        crate::UInt256(lattice_pow::asert_target(
            anchor.0,
            anchor_timestamp,
            anchor_height,
            timestamp,
            height,
            self.target_block_time,
            self.half_life,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmPolicyModule {
    #[serde(with = "lattice_cashew::cbor::base64_bytes")]
    pub bytes: Vec<u8>,
}
impl lattice_cashew::DagNode for WasmPolicyModule {}
