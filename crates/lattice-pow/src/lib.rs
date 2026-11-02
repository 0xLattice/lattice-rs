//! Initial LatticePoW port. Does not validate blocks or implement fork choice.
#![forbid(unsafe_code)]
use num_bigint::BigUint;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

/// Targets and hashes compare in network (big-endian) byte order.
pub type Target = [u8; 32];

fn fixed(value: &BigUint) -> Option<Target> {
    let bytes = value.to_bytes_be();
    if bytes.len() > 32 {
        return None;
    }
    let mut result = [0; 32];
    result[32 - bytes.len()..].copy_from_slice(&bytes);
    Some(result)
}

pub fn hash(prefix: &[u8], nonce: u64) -> Target {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(nonce.to_be_bytes());
    hasher.finalize().into()
}

/// A zero target expresses no valid work, including for an all-zero hash.
pub fn validate(hash: &Target, target: &Target) -> bool {
    *target != [0; 32] && hash <= target
}

pub fn work_for_target(target: &Target) -> Target {
    if *target == [0; 32] {
        return [0; 32];
    }
    work_for_hash(target)
}

pub fn work_for_hash(hash: &Target) -> Target {
    if *hash == [0; 32] {
        return [255; 32];
    }
    let work = (BigUint::from(1u8) << 256usize) / (BigUint::from_bytes_be(hash) + 1u8);
    fixed(&work).expect("nonzero hash implies work fits in 256 bits")
}

/// Exact cumulative work. Never saturates: saturation would change fork choice.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkSum(BigUint);
impl WorkSum {
    pub fn from_hex(text: &str) -> Option<Self> {
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        BigUint::parse_bytes(text.as_bytes(), 16).map(Self)
    }
    pub fn to_hex(&self) -> String {
        format!("{:0>64}", self.0.to_str_radix(16))
    }
    pub fn as_target(&self) -> Option<Target> {
        fixed(&self.0)
    }
    pub fn checked_sub(&self, other: &Self) -> Option<Self> {
        (self >= other).then(|| Self(&self.0 - &other.0))
    }
}
impl From<Target> for WorkSum {
    fn from(value: Target) -> Self {
        Self(BigUint::from_bytes_be(&value))
    }
}
impl std::ops::Add<&WorkSum> for &WorkSum {
    type Output = WorkSum;
    fn add(self, other: &WorkSum) -> WorkSum {
        WorkSum(&self.0 + &other.0)
    }
}
impl Serialize for WorkSum {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}
impl<'de> Deserialize<'de> for WorkSum {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text)
            .ok_or_else(|| serde::de::Error::custom("invalid hexadecimal work sum"))
    }
}

/// Inputs to the consensus preimage. CID strings must already be the exact
/// consensus spelling; this low-level function does not normalize or validate.
pub struct PreimageFields<'a> {
    pub version: u16,
    pub parent: Option<&'a str>,
    pub transactions: &'a str,
    pub target: Target,
    pub next_target: Target,
    pub spec: &'a str,
    pub parent_state: &'a str,
    pub prev_state: &'a str,
    pub post_state: &'a str,
    pub children: &'a str,
    pub height: u64,
    pub timestamp: i64,
    pub reward_recipient: Option<&'a str>,
}
impl PreimageFields<'_> {
    pub fn prefix(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let target = WorkSum::from(self.target).to_hex();
        let next_target = WorkSum::from(self.next_target).to_hex();
        let version = self.version.to_string();
        let height = self.height.to_string();
        let timestamp = self.timestamp.to_string();
        for field in [
            version.as_str(),
            self.parent.unwrap_or(""),
            self.transactions,
            &target,
            &next_target,
            self.spec,
            self.parent_state,
            self.prev_state,
            self.post_state,
            self.children,
            &height,
            &timestamp,
        ] {
            out.extend_from_slice(field.as_bytes());
            out.push(0);
        }
        if let Some(recipient) = self.reward_recipient {
            out.push(1);
            out.extend_from_slice(recipient.as_bytes());
        }
        out.push(0);
        out
    }
}

/// Integer ASERT schedule, including Swift's drift clamps and extreme hardening
/// behavior. Uses wide intermediates so multiplication cannot wrap.
pub fn asert_target(
    anchor: Target,
    anchor_timestamp: i64,
    anchor_height: u64,
    timestamp: i64,
    height: u64,
    target_block_time: u64,
    half_life: u64,
) -> Target {
    if height <= anchor_height {
        return anchor;
    }
    let half_ms = half_life
        .saturating_mul(target_block_time)
        .min(i64::MAX as u64) as i64;
    if half_ms == 0 {
        return anchor;
    }
    let one = || {
        let mut value = [0; 32];
        value[31] = 1;
        value
    };
    if anchor == [0; 32] {
        return one();
    }
    let scheduled = (height - anchor_height)
        .saturating_mul(target_block_time)
        .min(i64::MAX as u64) as i64;
    let elapsed = (i128::from(timestamp) - i128::from(anchor_timestamp))
        .clamp(0, i128::from(i64::MAX)) as i64;
    let drift = elapsed.saturating_sub(scheduled);
    let bound = i64::MAX / 65536;
    let exponent = (drift.clamp(-bound, bound) * 65536) / half_ms;
    let doublings = exponent >> 16;
    let fraction = (exponent & 65535) as u64;
    let cubic = 195_766_423_245_049u64 * fraction
        + 971_821_376u64 * fraction * fraction
        + 5_127u64 * fraction * fraction * fraction
        + (1u64 << 47);
    let factor = 65536 + (cubic >> 48);
    if doublings < 0 && -doublings >= 240 {
        return one();
    }
    let value = BigUint::from_bytes_be(&anchor);
    let maximum = BigUint::from_bytes_be(&[255; 32]);
    // Match the Swift helper's saturation of both products before summing.
    let scale = |shift: usize| {
        let divisor = BigUint::from(1u8) << shift;
        let quotient = ((&value / &divisor) * factor).min(maximum.clone());
        let remainder = ((&value % &divisor) * factor).min(maximum.clone()) / divisor;
        (quotient + remainder).min(maximum.clone())
    };
    let scaled = if doublings < 0 {
        scale((16 - doublings) as usize)
    } else {
        if doublings >= 256 {
            return [255; 32];
        }
        scale(16) << doublings as usize
    };
    if scaled == BigUint::default() {
        one()
    } else {
        fixed(&scaled).unwrap_or([255; 32])
    }
}
