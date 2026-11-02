use cid::{Cid, Version};
use serde::{Deserialize, Serialize};

pub fn deterministic_atom(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}
pub fn account_atom(value: &str) -> bool {
    deterministic_atom(value) && !value.contains('/')
}
pub fn directory_atom(value: &str) -> bool {
    account_atom(value)
}
pub fn child_directory(value: &str) -> bool {
    directory_atom(value) && value.len() <= u16::MAX as usize
}
pub fn canonical_cid(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > u16::MAX as usize {
        return None;
    }
    lattice_cashew::canonical_cid(value).ok()
}
pub fn standard_cid(value: &str) -> bool {
    let Ok(cid) = value.parse::<Cid>() else {
        return false;
    };
    cid.version() == Version::V1
        && cid.codec() == 0x71
        && cid.hash().code() == 0x12
        && cid.hash().size() == 32
        && cid.to_string() == value
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContextError {
    #[error("empty path")]
    EmptyPath,
    #[error("empty directory")]
    EmptyDirectory,
    #[error("root must be Nexus")]
    RootMustBeNexus,
    #[error("directory contains separator")]
    DirectoryContainsSeparator,
    #[error("invalid directory")]
    InvalidDirectory,
    #[error("directory too long")]
    DirectoryTooLong,
    #[error("path too deep")]
    PathTooDeep,
    #[error("root genesis required")]
    RootGenesisRequired,
    #[error("child genesis cannot be pinned")]
    ChildGenesisPinned,
    #[error("invalid genesis CID")]
    InvalidGenesisCid,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainRuntimeContext {
    path: Vec<String>,
    genesis_cid: Option<String>,
}
impl ChainRuntimeContext {
    pub fn new(path: Vec<String>, genesis_cid: Option<&str>) -> Result<Self, ContextError> {
        use ContextError::*;
        if path.is_empty() {
            return Err(EmptyPath);
        }
        if path.iter().any(String::is_empty) {
            return Err(EmptyDirectory);
        }
        if path[0] != "Nexus" {
            return Err(RootMustBeNexus);
        }
        if path.len() - 1 > u16::MAX as usize {
            return Err(PathTooDeep);
        }
        if path.iter().any(|x| x.len() > u16::MAX as usize) {
            return Err(DirectoryTooLong);
        }
        if path.iter().any(|x| x.contains('/')) {
            return Err(DirectoryContainsSeparator);
        }
        if !path.iter().all(|x| directory_atom(x)) {
            return Err(InvalidDirectory);
        }
        let genesis_cid = if path.len() == 1 {
            Some(canonical_cid(genesis_cid.ok_or(RootGenesisRequired)?).ok_or(InvalidGenesisCid)?)
        } else {
            if genesis_cid.is_some() {
                return Err(ChildGenesisPinned);
            }
            None
        };
        Ok(Self { path, genesis_cid })
    }
    pub fn path(&self) -> &[String] {
        &self.path
    }
    pub fn genesis_cid(&self) -> Option<&str> {
        self.genesis_cid.as_deref()
    }
    pub fn is_root(&self) -> bool {
        self.path.len() == 1
    }
    pub fn proof_path(&self) -> &[String] {
        &self.path[1..]
    }
    pub fn admits_genesis(&self, cid: &str) -> bool {
        self.genesis_cid
            .as_deref()
            .is_none_or(|expected| expected == cid)
    }
}

/// Swift UInt256's Codable spelling is unpadded `0x` hex; PoW uses padded hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UInt256(pub [u8; 32]);
impl Serialize for UInt256 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let digits = hex::encode(self.0);
        let digits = digits.trim_start_matches('0');
        s.serialize_str(&format!(
            "0x{}",
            if digits.is_empty() { "0" } else { digits }
        ))
    }
}
impl<'de> Deserialize<'de> for UInt256 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        let digits = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(&value);
        let digits = digits.strip_prefix('+').unwrap_or(digits);
        lattice_pow::WorkSum::from_hex(digits)
            .and_then(|sum| sum.as_target())
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("invalid UInt256 hex"))
    }
}
