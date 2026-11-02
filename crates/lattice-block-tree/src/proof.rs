//! Content-bound sparse child proofs. Verified evidence has no public constructor
//! or deserializer: only verification of actual proof bytes can produce it.
use crate::VerifiedWorkContribution;
use lattice_cashew::{ContentSource, FlatDictionary, cbor, validate_entry};
use lattice_primitives::{
    Block, BlockHeader, UInt256, canonical_cid, child_directory, content_id, standard_cid,
};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChildProofFailure {
    #[error("malformed or nonminimal evidence")]
    MalformedEvidence,
    #[error("content proves an invalid protocol structure")]
    ProtocolInvalid,
    #[error("proof exceeds wire representation bounds")]
    ValueTooLarge,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildBlockProof {
    pub root_cid: String,
    pub directory_path: Vec<String>,
    pub entries: Vec<(String, Vec<u8>)>,
}
#[derive(Debug, Clone)]
pub struct VerifiedChildEvidence {
    chain_path: Vec<String>,
    grind_id: String,
    root_hash: UInt256,
    credited_ancestor_work: UInt256,
    child_cid: String,
    terminal_carrier_cid: String,
    contribution: Option<VerifiedWorkContribution>,
}
impl VerifiedChildEvidence {
    pub fn chain_path(&self) -> &[String] {
        &self.chain_path
    }
    pub fn grind_id(&self) -> &str {
        &self.grind_id
    }
    pub fn root_hash(&self) -> UInt256 {
        self.root_hash
    }
    pub fn credited_ancestor_work(&self) -> UInt256 {
        self.credited_ancestor_work
    }
    pub fn child_cid(&self) -> &str {
        &self.child_cid
    }
    pub fn terminal_carrier_cid(&self) -> &str {
        &self.terminal_carrier_cid
    }
    pub fn contribution(&self) -> Option<&VerifiedWorkContribution> {
        self.contribution.as_ref()
    }
}
#[derive(Debug, Clone)]
pub struct DirectChildHop {
    pub proof: ChildBlockProof,
    pub child_cid: String,
    pub parent_state_cid: String,
}
impl DirectChildHop {
    pub fn binds(&self, child: &Block) -> bool {
        content_id(child).ok().as_deref() == Some(&self.child_cid)
            && child.parent_state.0.raw_cid == self.parent_state_cid
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSchedulingTargets {
    pub search_target: UInt256,
    pub deployment_target: Option<UInt256>,
}
fn content_block(cid: &str, bytes: &[u8]) -> Result<Block, ChildProofFailure> {
    if !standard_cid(cid) || lattice_cashew::cid_for_bytes(0x71, bytes) != cid {
        return Err(ChildProofFailure::MalformedEvidence);
    }
    let block = cbor::decode::<Block>(bytes).map_err(|_| ChildProofFailure::ProtocolInvalid)?;
    if !block.has_well_formed_reward_recipient() {
        return Err(ChildProofFailure::ProtocolInvalid);
    }
    Ok(block)
}
fn well_formed(children: &FlatDictionary<BlockHeader>) -> bool {
    children.iter().all(|(name, h)| {
        child_directory(name) && standard_cid(&h.0.raw_cid) && h.0.encryption_info.is_none()
    })
}
impl ChildBlockProof {
    pub fn new(
        root_cid: String,
        directory_path: Vec<String>,
        entries: Vec<(String, Vec<u8>)>,
    ) -> Self {
        let mut entries: Vec<_> = entries
            .into_iter()
            .map(|(cid, bytes)| (canonical_cid(&cid).unwrap_or(cid), bytes))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Self {
            root_cid: canonical_cid(&root_cid).unwrap_or(root_cid),
            directory_path,
            entries,
        }
    }
    fn representable(&self) -> bool {
        self.directory_path.len() <= u16::MAX as usize
            && self
                .directory_path
                .iter()
                .all(|s| s.len() <= u16::MAX as usize)
    }
    fn sealed(&self) -> Result<BTreeMap<String, Vec<u8>>, ChildProofFailure> {
        let mut map = BTreeMap::new();
        for (cid, data) in &self.entries {
            if map.insert(cid.clone(), data.clone()).is_some() {
                return Err(ChildProofFailure::MalformedEvidence);
            }
        }
        Ok(map)
    }
    /// Generate exactly root, its complete flat child index and selected child.
    /// Unselected sibling bodies are never fetched.
    pub fn generate(
        root: &str,
        directory: &str,
        source: &dyn ContentSource,
    ) -> Result<Self, ChildProofFailure> {
        if directory.len() > u16::MAX as usize {
            return Err(ChildProofFailure::ValueTooLarge);
        }
        let bytes = source
            .fetch(root)
            .map_err(|_| ChildProofFailure::MalformedEvidence)?;
        let block = content_block(root, &bytes)?;
        let index = source
            .fetch(&block.children.raw_cid)
            .map_err(|_| ChildProofFailure::MalformedEvidence)?;
        validate_entry(&block.children.raw_cid, &index)
            .map_err(|_| ChildProofFailure::MalformedEvidence)?;
        let children: FlatDictionary<BlockHeader> =
            cbor::decode(&index).map_err(|_| ChildProofFailure::MalformedEvidence)?;
        let mut entries = vec![(root.into(), bytes), (block.children.raw_cid, index)];
        if let Some(child) = children.get(directory) {
            let data = source
                .fetch(&child.0.raw_cid)
                .map_err(|_| ChildProofFailure::MalformedEvidence)?;
            validate_entry(&child.0.raw_cid, &data)
                .map_err(|_| ChildProofFailure::MalformedEvidence)?;
            cbor::decode::<Block>(&data).map_err(|_| ChildProofFailure::MalformedEvidence)?;
            entries.push((child.0.raw_cid.clone(), data));
        }
        Ok(Self::new(root.into(), vec![directory.into()], entries))
    }
    pub fn composing(&self, hop: &Self) -> Self {
        let mut entries = self.entries.clone();
        let mut known: BTreeMap<_, _> = self.entries.iter().cloned().collect();
        for (cid, data) in &hop.entries {
            if let Some(old) = known.get(cid) {
                if old != data {
                    entries.push((cid.clone(), data.clone()));
                }
            } else {
                known.insert(cid.clone(), data.clone());
                entries.push((cid.clone(), data.clone()));
            }
        }
        let mut path = self.directory_path.clone();
        path.extend(hop.directory_path.clone());
        Self::new(self.root_cid.clone(), path, entries)
    }
    pub fn verify_securing_work(
        &self,
        child: &Block,
        chain_path: &[String],
    ) -> Result<VerifiedChildEvidence, ChildProofFailure> {
        if chain_path.first().map(String::as_str) != Some("Nexus")
            || !self.representable()
            || self.directory_path.is_empty()
            || self.directory_path != chain_path[1..]
            || self.entries.is_empty()
        {
            return Err(ChildProofFailure::MalformedEvidence);
        }
        let map = self.sealed()?;
        let root_data = map
            .get(&self.root_cid)
            .ok_or(ChildProofFailure::MalformedEvidence)?;
        let root = content_block(&self.root_cid, root_data)?;
        let root_hash = root.pow_hash();
        let child_cid = content_id(child).map_err(|_| ChildProofFailure::MalformedEvidence)?;
        let mut consumed = BTreeSet::from([self.root_cid.clone()]);
        let mut current = root;
        let mut carrier = self.root_cid.clone();
        let mut first_met = None;
        for (index, directory) in self.directory_path.iter().enumerate() {
            if first_met.is_none() && lattice_pow::validate(&root_hash, &current.target.0) {
                first_met = Some(lattice_pow::work_for_target(&current.target.0));
            }
            let index_cid = &current.children.raw_cid;
            let bytes = map
                .get(index_cid)
                .ok_or(ChildProofFailure::MalformedEvidence)?;
            validate_entry(index_cid, bytes).map_err(|_| ChildProofFailure::MalformedEvidence)?;
            consumed.insert(index_cid.clone());
            let children: FlatDictionary<BlockHeader> =
                cbor::decode(bytes).map_err(|_| ChildProofFailure::MalformedEvidence)?;
            let next = children
                .get(directory)
                .ok_or(ChildProofFailure::MalformedEvidence)?;
            if !well_formed(&children) {
                return Err(ChildProofFailure::ProtocolInvalid);
            }
            let next_cid = &next.0.raw_cid;
            let data = map
                .get(next_cid)
                .ok_or(ChildProofFailure::MalformedEvidence)?;
            // Targeted Cashew resolution consumes the selected terminal block too.
            validate_entry(next_cid, data).map_err(|_| ChildProofFailure::MalformedEvidence)?;
            let resolved =
                cbor::decode::<Block>(data).map_err(|_| ChildProofFailure::MalformedEvidence)?;
            consumed.insert(next_cid.clone());
            if index + 1 == self.directory_path.len() {
                if *next_cid != child_cid {
                    return Err(ChildProofFailure::MalformedEvidence);
                }
                if child.parent_state.0.raw_cid != current.prev_state.0.raw_cid {
                    return Err(ChildProofFailure::ProtocolInvalid);
                }
                if consumed != map.keys().cloned().collect() {
                    return Err(ChildProofFailure::MalformedEvidence);
                }
                let credited = first_met.unwrap_or([0; 32]);
                let contribution = lattice_pow::validate(&root_hash, &child.target.0).then(|| {
                    VerifiedWorkContribution::from_verified_import(
                        self.root_cid.clone(),
                        UInt256(credited.max(lattice_pow::work_for_target(&child.target.0))),
                    )
                });
                return Ok(VerifiedChildEvidence {
                    chain_path: chain_path.to_vec(),
                    grind_id: self.root_cid.clone(),
                    root_hash: UInt256(root_hash),
                    credited_ancestor_work: UInt256(credited),
                    child_cid,
                    terminal_carrier_cid: carrier,
                    contribution,
                });
            }
            let next_block = content_block(next_cid, data)?;
            if next_block.parent_state.0.raw_cid != current.prev_state.0.raw_cid {
                return Err(ChildProofFailure::ProtocolInvalid);
            }
            let _ = resolved;
            current = next_block;
            carrier = next_cid.clone();
        }
        Err(ChildProofFailure::MalformedEvidence)
    }
    pub fn serialize(&self) -> Result<Vec<u8>, ChildProofFailure> {
        if !self.representable()
            || self.root_cid.len() > u16::MAX as usize
            || self.entries.len() > u16::MAX as usize
        {
            return Err(ChildProofFailure::ValueTooLarge);
        }
        let mut out = vec![];
        put_string(&mut out, &self.root_cid)?;
        out.extend_from_slice(&(self.directory_path.len() as u16).to_le_bytes());
        for dir in &self.directory_path {
            put_string(&mut out, dir)?;
        }
        out.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        for (cid, bytes) in &self.entries {
            put_string(&mut out, cid)?;
            let len = u32::try_from(bytes.len()).map_err(|_| ChildProofFailure::ValueTooLarge)?;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(bytes);
        }
        Ok(out)
    }
    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader { bytes, pos: 0 };
        let root = reader.string()?;
        let n = reader.u16()?;
        let mut path = vec![];
        for _ in 0..n {
            path.push(reader.string()?);
        }
        let n = reader.u16()?;
        let mut entries = vec![];
        let mut previous = None;
        for _ in 0..n {
            let cid = reader.string()?;
            if previous.as_ref().is_some_and(|p| p >= &cid) {
                return None;
            }
            previous = Some(cid.clone());
            let n = reader.u32()? as usize;
            entries.push((cid, reader.take(n)?.to_vec()));
        }
        if reader.pos != bytes.len() {
            return None;
        }
        Some(Self::new(root, path, entries))
    }
}
fn put_string(out: &mut Vec<u8>, s: &str) -> Result<(), ChildProofFailure> {
    let n = u16::try_from(s.len()).map_err(|_| ChildProofFailure::ValueTooLarge)?;
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let value = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(value)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn string(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}
struct SealedSource(BTreeMap<String, Vec<u8>>);
impl ContentSource for SealedSource {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
        self.0
            .get(cid)
            .cloned()
            .ok_or_else(|| lattice_cashew::Error::MissingContent(cid.into()))
    }
}
impl ChildBlockProof {
    pub fn direct_hop(&self) -> Option<DirectChildHop> {
        let directory = self.directory_path.last()?;
        if !standard_cid(&self.root_cid) {
            return None;
        }
        let map = self.sealed().ok()?;
        if map
            .keys()
            .any(|cid| canonical_cid(cid).as_deref() != Some(cid))
        {
            return None;
        }
        let source = SealedSource(map);
        let mut carrier = self.root_cid.clone();
        for directory in self
            .directory_path
            .iter()
            .take(self.directory_path.len() - 1)
        {
            let block = content_block(&carrier, &source.fetch(&carrier).ok()?).ok()?;
            let bytes = source.fetch(&block.children.raw_cid).ok()?;
            validate_entry(&block.children.raw_cid, &bytes).ok()?;
            let children: FlatDictionary<BlockHeader> = cbor::decode(&bytes).ok()?;
            if !well_formed(&children) {
                return None;
            }
            carrier = children.get(directory)?.0.raw_cid.clone();
        }
        let proof = Self::generate(&carrier, directory, &source).ok()?;
        let block = content_block(&carrier, &source.fetch(&carrier).ok()?).ok()?;
        let children: FlatDictionary<BlockHeader> =
            cbor::decode(&source.fetch(&block.children.raw_cid).ok()?).ok()?;
        if !well_formed(&children) {
            return None;
        }
        Some(DirectChildHop {
            proof,
            child_cid: children.get(directory)?.0.raw_cid.clone(),
            parent_state_cid: block.prev_state.0.raw_cid,
        })
    }
    pub fn scheduling_targets(
        &self,
        root: &Block,
        terminal: &Block,
    ) -> Option<ChildSchedulingTargets> {
        if !self.representable()
            || self.directory_path.is_empty()
            || self.entries.is_empty()
            || content_id(root).ok()? != self.root_cid
        {
            return None;
        }
        let source = SealedSource(self.sealed().ok()?);
        if source
            .0
            .keys()
            .any(|cid| canonical_cid(cid).as_deref() != Some(cid))
        {
            return None;
        }
        content_block(&self.root_cid, source.0.get(&self.root_cid)?).ok()?;
        let terminal_cid = content_id(terminal).ok()?;
        let mut current = root.clone();
        let mut deployment = root.target;
        let mut canonical = BTreeMap::new();
        for (index, directory) in self.directory_path.iter().enumerate() {
            let current_cid = content_id(&current).ok()?;
            let hop = Self::generate(&current_cid, directory, &source).ok()?;
            for (cid, bytes) in hop.entries {
                if canonical
                    .insert(cid, bytes.clone())
                    .is_some_and(|old| old != bytes)
                {
                    return None;
                }
            }
            let children: FlatDictionary<BlockHeader> =
                cbor::decode(source.0.get(&current.children.raw_cid)?).ok()?;
            if !well_formed(&children) {
                return None;
            }
            let child = children.get(directory)?;
            if index + 1 == self.directory_path.len() {
                if child.0.raw_cid != terminal_cid
                    || terminal.parent_state.0.raw_cid != current.prev_state.0.raw_cid
                    || canonical != source.0
                {
                    return None;
                }
                return Some(ChildSchedulingTargets {
                    search_target: terminal.target,
                    deployment_target: terminal
                        .parent
                        .is_none()
                        .then_some(deployment.min(terminal.target)),
                });
            }
            let next = content_block(&child.0.raw_cid, source.0.get(&child.0.raw_cid)?).ok()?;
            if next.parent_state.0.raw_cid != current.prev_state.0.raw_cid {
                return None;
            }
            deployment = deployment.min(next.target);
            current = next;
        }
        None
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectChildEdge {
    #[serde(rename = "parentCarrierCID")]
    pub parent_carrier_cid: String,
    pub directory: String,
    #[serde(rename = "childCID")]
    pub child_cid: String,
    #[serde(with = "proof_bytes")]
    pub proof_bytes: Vec<u8>,
}
// Foundation Data's Codable representation is base64 text, not CBOR byte strings.
mod proof_bytes {
    use serde::{Deserialize, Serialize};
    pub fn serialize<S: serde::Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .encode(bytes)
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Vec<u8>, D::Error> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(String::deserialize(decoder)?)
            .map_err(serde::de::Error::custom)
    }
}
impl DirectChildEdge {
    pub fn edge_cid(&self) -> Option<String> {
        content_id(self).ok()
    }
    pub fn proof(&self) -> Option<ChildBlockProof> {
        ChildBlockProof::deserialize(&self.proof_bytes)
    }
    pub fn derive(proof: &ChildBlockProof) -> Option<Self> {
        let directory = proof.directory_path.last()?.clone();
        let hop = proof.direct_hop()?;
        let edge = Self {
            parent_carrier_cid: hop.proof.root_cid.clone(),
            directory,
            child_cid: hop.child_cid,
            proof_bytes: hop.proof.serialize().ok()?,
        };
        edge.canonical_hop()?;
        Some(edge)
    }
    fn canonical_hop(&self) -> Option<DirectChildHop> {
        if !standard_cid(&self.parent_carrier_cid)
            || !standard_cid(&self.child_cid)
            || !child_directory(&self.directory)
        {
            return None;
        }
        let proof = self.proof()?;
        if proof.root_cid != self.parent_carrier_cid
            || proof.directory_path != [self.directory.clone()]
            || proof.serialize().ok()? != self.proof_bytes
        {
            return None;
        }
        let hop = proof.direct_hop()?;
        if hop.child_cid != self.child_cid || hop.proof.serialize().ok()? != self.proof_bytes {
            return None;
        }
        self.edge_cid()?;
        Some(hop)
    }
    pub fn validated(&self) -> bool {
        self.canonical_hop().is_some()
    }
    pub fn validates(&self, child: &Block) -> bool {
        self.canonical_hop().is_some_and(|hop| hop.binds(child))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentStateContinuityLink {
    pub parent_path: Vec<String>,
    pub from_state_cid: String,
    pub to_state_cid: String,
}
impl ParentStateContinuityLink {
    pub fn new(parent_path: Vec<String>, from: String, to: String) -> Self {
        Self {
            parent_path,
            from_state_cid: canonical_cid(&from).unwrap_or(from),
            to_state_cid: canonical_cid(&to).unwrap_or(to),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrossChainEvidenceRequirement {
    ChildProof {
        chain_path: Vec<String>,
        child_cid: String,
    },
    ParentStateContinuity(ParentStateContinuityLink),
}
