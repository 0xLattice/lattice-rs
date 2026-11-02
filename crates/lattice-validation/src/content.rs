//! Bounded logical block-content resolution. Independent state/spec/child block
//! Volumes are excluded; the child index and transaction bodies are included.
use crate::*;
use lattice_cashew::{ContentSource, Header, Volume};
use std::cell::{Cell, RefCell};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentSizeError {
    #[error("logical block content exceeds its byte limit")]
    ExceedsLimit,
    #[error("logical block byte count overflow")]
    Overflow,
    #[error("conflicting bytes for CID {0}")]
    ConflictingCid(String),
}
#[derive(Debug, thiserror::Error)]
pub enum BlockContentError {
    #[error(transparent)]
    Size(#[from] ContentSizeError),
    #[error(transparent)]
    Content(#[from] lattice_cashew::Error),
}
pub struct ResolvedBlockContent {
    pub block: Block,
    pub transactions: Vec<Transaction>,
    pub byte_count: i64,
}
struct Counter<'a, S> {
    source: &'a S,
    limit: i64,
    entries: RefCell<BTreeMap<String, Vec<u8>>>,
    total: Cell<i64>,
    failure: RefCell<Option<ContentSizeError>>,
}
impl<S: ContentSource> Counter<'_, S> {
    fn add(&self, cid: &str, bytes: Vec<u8>) -> Result<(), lattice_cashew::Error> {
        if self.failure.borrow().is_some() {
            return Err(lattice_cashew::Error::Limit);
        }
        let mut entries = self.entries.borrow_mut();
        if let Some(previous) = entries.get(cid) {
            if previous == &bytes {
                return Ok(());
            }
            *self.failure.borrow_mut() = Some(ContentSizeError::ConflictingCid(cid.into()));
            return Err(lattice_cashew::Error::Limit);
        }
        let Some(total) = i64::try_from(bytes.len())
            .ok()
            .and_then(|n| self.total.get().checked_add(n))
        else {
            *self.failure.borrow_mut() = Some(ContentSizeError::Overflow);
            return Err(lattice_cashew::Error::Limit);
        };
        entries.insert(cid.into(), bytes);
        self.total.set(total);
        if total > self.limit {
            *self.failure.borrow_mut() = Some(ContentSizeError::ExceedsLimit);
            return Err(lattice_cashew::Error::Limit);
        }
        Ok(())
    }
    fn load<T: Clone + serde::Serialize + serde::de::DeserializeOwned>(
        &self,
        header: &Header<T>,
    ) -> Result<Header<T>, lattice_cashew::Error> {
        let resolved = header.resolve(self, None)?;
        self.add(&resolved.raw_cid, resolved.bytes(None)?)?;
        Ok(resolved)
    }
}
impl<S: ContentSource> ContentSource for Counter<'_, S> {
    fn fetch(&self, cid: &str) -> Result<Vec<u8>, lattice_cashew::Error> {
        if let Some(bytes) = self.entries.borrow().get(cid) {
            return Ok(bytes.clone());
        }
        let bytes = self.source.fetch(cid)?;
        lattice_cashew::verify_content_address(cid, &bytes)?;
        self.add(cid, bytes.clone())?;
        Ok(bytes)
    }
}
/// Fetches stop as soon as authenticated, unique owned bytes exceed the limit.
/// A fetcher error is never reclassified as a size violation unless this
/// counter itself recorded that violation.
pub fn resolve_block_content(
    block: &Block,
    source: &impl ContentSource,
    limit: i64,
) -> Result<ResolvedBlockContent, BlockContentError> {
    resolve_content(block, source, limit, true)
}

/// Bounded transaction-only resolution used before policy checks. Whole-block
/// measurement occurs later so missing child content cannot mask invalid txs.
pub fn resolve_transaction_content(
    block: &Block,
    source: &impl ContentSource,
    limit: i64,
) -> Result<ResolvedBlockContent, BlockContentError> {
    resolve_content(block, source, limit, false)
}

fn resolve_content(
    block: &Block,
    source: &impl ContentSource,
    limit: i64,
    whole: bool,
) -> Result<ResolvedBlockContent, BlockContentError> {
    let counter = Counter {
        source,
        limit,
        entries: RefCell::new(BTreeMap::new()),
        total: Cell::new(0),
        failure: RefCell::new(None),
    };
    let resolve = || -> Result<ResolvedBlockContent, lattice_cashew::Error> {
        let mut resolved = block.clone();
        if whole {
            let root = Header::new(block.clone())?;
            counter.add(&root.raw_cid, root.bytes(None)?)?;
            resolved.children = counter.load(&block.children)?;
        }
        let dictionary = counter.load(&block.transactions)?;
        let mut tree = dictionary
            .node
            .as_deref()
            .ok_or_else(|| lattice_cashew::Error::MissingContent(dictionary.raw_cid.clone()))?
            .resolve_all(&counter)?;
        // Materialized trie nodes need counting too: they may not have fetched.
        let mut materialized_dictionary = dictionary;
        materialized_dictionary.node = Some(std::sync::Arc::new(tree.clone()));
        let index = Volume(materialized_dictionary.clone());
        let boundary = lattice_cashew::volume::serialize_boundary(&index, &())?;
        for (cid, bytes) in boundary.entries {
            counter.add(&cid, bytes)?;
        }
        let mut transactions = Vec::new();
        for (key, volume) in tree.entries()? {
            let mut tx = counter.load(&volume.0)?;
            let mut transaction = tx
                .node
                .as_deref()
                .ok_or_else(|| lattice_cashew::Error::MissingContent(tx.raw_cid.clone()))?
                .clone();
            transaction.body = counter.load(&transaction.body)?;
            tx.node = Some(std::sync::Arc::new(transaction.clone()));
            tree = tree.updating(&key, Volume(tx))?;
            transactions.push(transaction);
        }
        materialized_dictionary.node = Some(std::sync::Arc::new(tree));
        resolved.transactions = materialized_dictionary;
        Ok(ResolvedBlockContent {
            block: resolved,
            transactions,
            byte_count: counter.total.get(),
        })
    };
    match resolve() {
        Ok(result) => Ok(result),
        Err(error) => match counter.failure.into_inner() {
            Some(size) => Err(size.into()),
            None => Err(error.into()),
        },
    }
}

pub fn valid_child_commitments(
    block: &Block,
    bodies: &[TransactionBody],
) -> Result<bool, lattice_cashew::Error> {
    let mut commitments = Vec::new();
    for body in bodies {
        for action in &body.actions {
            if let (Some(directory), Some(value)) = (
                action.key.strip_prefix(CHILD_COMMITMENT_KEY_PREFIX),
                action.new_value.as_deref(),
            ) {
                commitments.push((directory, value));
            }
        }
    }
    if commitments.is_empty() {
        return Ok(true);
    }
    if !block.children_are_well_formed()? {
        return Ok(false);
    }
    let children = block
        .children
        .node
        .as_deref()
        .expect("checked loaded child index");
    Ok(commitments.into_iter().all(|(directory, value)| {
        let mut parts = value.split('@');
        let (Some(child), Some(parent), None) = (parts.next(), parts.next(), parts.next()) else {
            return false;
        };
        !child.is_empty()
            && !parent.is_empty()
            && parent == block.prev_state.0.raw_cid
            && children
                .get(directory)
                .is_some_and(|h| h.0.raw_cid == child)
    }))
}
