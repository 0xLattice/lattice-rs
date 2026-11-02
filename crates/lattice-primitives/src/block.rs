use crate::*;
use lattice_cashew::{DagHeader, DagNode, FlatDictionary, Header, MerkleDictionary, Volume};
use serde::{Deserialize, Serialize};
pub type BlockHeader = Volume<Block>;
pub const CHILD_COMMITMENT_KEY_PREFIX: &str = "child/";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(try_from = "BlockWire")]
pub struct Block {
    pub version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<Volume<Block>>,
    pub transactions: Header<MerkleDictionary<Volume<Transaction>>>,
    pub target: UInt256,
    pub next_target: UInt256,
    pub spec: Volume<ChainSpec>,
    pub parent_state: LatticeStateHeader,
    pub prev_state: LatticeStateHeader,
    pub post_state: LatticeStateHeader,
    pub children: Header<FlatDictionary<BlockHeader>>,
    pub height: u64,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_recipient: Option<String>,
    pub nonce: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockWire {
    pub version: u16,
    pub parent: Option<Volume<Block>>,
    pub transactions: Header<MerkleDictionary<Volume<Transaction>>>,
    pub target: UInt256,
    pub next_target: UInt256,
    pub spec: Volume<ChainSpec>,
    pub parent_state: LatticeStateHeader,
    pub prev_state: LatticeStateHeader,
    pub post_state: LatticeStateHeader,
    pub children: Header<FlatDictionary<BlockHeader>>,
    pub height: u64,
    pub timestamp: i64,
    pub reward_recipient: Option<String>,
    pub nonce: u64,
}
impl TryFrom<BlockWire> for Block {
    type Error = &'static str;
    fn try_from(wire: BlockWire) -> Result<Self, Self::Error> {
        let block = Self {
            version: wire.version,
            parent: wire.parent,
            transactions: wire.transactions,
            target: wire.target,
            next_target: wire.next_target,
            spec: wire.spec,
            parent_state: wire.parent_state,
            prev_state: wire.prev_state,
            post_state: wire.post_state,
            children: wire.children,
            height: wire.height,
            timestamp: wire.timestamp,
            reward_recipient: wire.reward_recipient,
            nonce: wire.nonce,
        };
        if !block.has_standard_links() {
            return Err("a header link is not a standard plain CID");
        }
        Ok(block)
    }
}
fn plain<T>(header: &Header<T>) -> bool {
    header.encryption_info.is_none() && standard_cid(&header.raw_cid)
}
impl Block {
    pub const CURRENT_VERSION: u16 = 1;
    pub fn has_standard_links(&self) -> bool {
        self.parent.as_ref().is_none_or(|h| plain(&h.0))
            && plain(&self.transactions)
            && plain(&self.spec.0)
            && plain(&self.parent_state.0)
            && plain(&self.prev_state.0)
            && plain(&self.post_state.0)
            && plain(&self.children)
    }
    pub fn has_well_formed_reward_recipient(&self) -> bool {
        self.reward_recipient.as_deref().is_none_or(standard_cid)
    }
    /// Requires the child index to be materialized; missing data is an error.
    pub fn children_are_well_formed(&self) -> Result<bool, lattice_cashew::Error> {
        let children =
            self.children.node.as_deref().ok_or_else(|| {
                lattice_cashew::Error::MissingContent(self.children.raw_cid.clone())
            })?;
        Ok(children
            .iter()
            .all(|(directory, child)| child_directory(directory) && plain(&child.0)))
    }
    pub fn pow_prefix(&self) -> Vec<u8> {
        lattice_pow::PreimageFields {
            version: self.version,
            parent: self.parent.as_ref().map(|h| h.0.raw_cid.as_str()),
            transactions: &self.transactions.raw_cid,
            target: self.target.0,
            next_target: self.next_target.0,
            spec: &self.spec.0.raw_cid,
            parent_state: &self.parent_state.0.raw_cid,
            prev_state: &self.prev_state.0.raw_cid,
            post_state: &self.post_state.0.raw_cid,
            children: &self.children.raw_cid,
            height: self.height,
            timestamp: self.timestamp,
            reward_recipient: self.reward_recipient.as_deref(),
        }
        .prefix()
    }
    pub fn pow_hash(&self) -> [u8; 32] {
        lattice_pow::hash(&self.pow_prefix(), self.nonce)
    }
    pub fn has_valid_pow(&self) -> bool {
        lattice_pow::validate(&self.pow_hash(), &self.target.0)
    }
}
impl DagNode for Block {
    fn links(&self) -> Vec<&dyn DagHeader> {
        let mut links: Vec<&dyn DagHeader> = vec![
            &self.transactions,
            &self.spec,
            &self.parent_state,
            &self.prev_state,
            &self.post_state,
            &self.children,
        ];
        if let Some(parent) = &self.parent {
            links.push(parent);
        }
        links
    }
}
impl DagNode for TransactionBody {}
impl DagNode for ChainSpec {}
impl DagNode for Transaction {
    fn links(&self) -> Vec<&dyn DagHeader> {
        vec![&self.body]
    }
}
