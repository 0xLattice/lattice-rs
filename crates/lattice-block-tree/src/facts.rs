use lattice_pow::Target;
use lattice_primitives::{StateDiff, UInt256};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainBlockFact {
    pub block_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_block_hash: Option<String>,
    pub block_height: u64,
    #[serde(rename = "postStateCID")]
    pub post_state_cid: String,
    #[serde(rename = "prevStateCID")]
    pub prev_state_cid: String,
    #[serde(
        rename = "parentStateCID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_state_cid: Option<String>,
    #[serde(rename = "specCID")]
    pub spec_cid: String,
    pub target: String,
    pub next_target: String,
    pub timestamp: i64,
    pub state_diff: StateDiff,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_commitments: Option<BTreeMap<String, String>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainValidationFact {
    pub block_hash: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainExclusionFact {
    pub block_hash: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifiedWorkContribution {
    id: String,
    work: UInt256,
}
impl VerifiedWorkContribution {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn work(&self) -> &Target {
        &self.work.0
    }
    /// The caller is the authenticated import boundary. Recovery should deserialize durable facts.
    #[doc(hidden)]
    pub(crate) fn from_verified_import(id: String, work: UInt256) -> Self {
        Self {
            id: lattice_primitives::canonical_cid(&id).unwrap_or(id),
            work,
        }
    }
}
pub fn parse_target(text: &str) -> Option<Target> {
    if text.is_empty() || text.len() > 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let padded = format!("{text:0>64}");
    let mut result = [0; 32];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(result)
}
pub fn target_hex(value: &Target) -> String {
    value.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainWorkFact {
    pub block_hash: String,
    pub contribution: VerifiedWorkContribution,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainFact {
    Block(Box<ChainBlockFact>),
    Work(ChainWorkFact),
    Exclusion(ChainExclusionFact),
    Validation(ChainValidationFact),
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChainFactId {
    Block(String),
    Work {
        block_hash: String,
        grind_id: String,
        work: String,
    },
    Exclusion(String),
    Validation(String),
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum WireFact {
    Block {
        #[serde(rename = "_0")]
        value: Box<ChainBlockFact>,
    },
    Work {
        #[serde(rename = "_0")]
        value: ChainWorkFact,
    },
    Exclusion {
        #[serde(rename = "_0")]
        value: ChainExclusionFact,
    },
    Validation {
        #[serde(rename = "_0")]
        value: ChainValidationFact,
    },
}
impl Serialize for ChainFact {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Block(v) => WireFact::Block { value: v.clone() },
            Self::Work(v) => WireFact::Work { value: v.clone() },
            Self::Exclusion(v) => WireFact::Exclusion { value: v.clone() },
            Self::Validation(v) => WireFact::Validation { value: v.clone() },
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for ChainFact {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match WireFact::deserialize(d)? {
            WireFact::Block { value } => Self::Block(value),
            WireFact::Work { value } => Self::Work(value),
            WireFact::Exclusion { value } => Self::Exclusion(value),
            WireFact::Validation { value } => Self::Validation(value),
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum WireId {
    Block {
        #[serde(rename = "_0")]
        value: String,
    },
    Work {
        #[serde(rename = "blockHash")]
        block_hash: String,
        #[serde(rename = "grindID")]
        grind_id: String,
        work: String,
    },
    Exclusion {
        #[serde(rename = "_0")]
        value: String,
    },
    Validation {
        #[serde(rename = "_0")]
        value: String,
    },
}
impl Serialize for ChainFactId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Block(v) => WireId::Block { value: v.clone() },
            Self::Work {
                block_hash,
                grind_id,
                work,
            } => WireId::Work {
                block_hash: block_hash.clone(),
                grind_id: grind_id.clone(),
                work: work.clone(),
            },
            Self::Exclusion(v) => WireId::Exclusion { value: v.clone() },
            Self::Validation(v) => WireId::Validation { value: v.clone() },
        }
        .serialize(s)
    }
}
impl ChainFact {
    pub fn id(&self) -> ChainFactId {
        match self {
            Self::Block(v) => ChainFactId::Block(v.block_hash.clone()),
            Self::Work(v) => ChainFactId::Work {
                block_hash: v.block_hash.clone(),
                grind_id: v.contribution.id.clone(),
                work: target_hex(&v.contribution.work.0),
            },
            Self::Exclusion(v) => ChainFactId::Exclusion(v.block_hash.clone()),
            Self::Validation(v) => ChainFactId::Validation(v.block_hash.clone()),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockImportBatch {
    facts: Vec<ChainFact>,
}
impl BlockImportBatch {
    pub fn facts(&self) -> &[ChainFact] {
        &self.facts
    }
    pub fn validation(block_hash: impl Into<String>) -> Self {
        Self {
            facts: vec![ChainFact::Validation(ChainValidationFact {
                block_hash: block_hash.into(),
            })],
        }
    }
    /// Package boundary for the authenticated import tier, not a wire admission API.
    pub(crate) fn from_verified_import(facts: Vec<ChainFact>) -> Self {
        Self { facts }
    }
}

impl<'de> Deserialize<'de> for VerifiedWorkContribution {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            work: UInt256,
        }
        let wire = Wire::deserialize(decoder)?;
        Ok(Self::from_verified_import(wire.id, wire.work))
    }
}
impl<'de> Deserialize<'de> for ChainFactId {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Ok(match WireId::deserialize(decoder)? {
            WireId::Block { value } => Self::Block(value),
            WireId::Work {
                block_hash,
                grind_id,
                work,
            } => Self::Work {
                block_hash,
                grind_id,
                work,
            },
            WireId::Exclusion { value } => Self::Exclusion(value),
            WireId::Validation { value } => Self::Validation(value),
        })
    }
}
