//! Consensus data and exact signing encodings. Validation and state execution
//! remain separate: constructing a value is not a validity verdict.
#![forbid(unsafe_code)]
pub mod actions;
pub mod block;
pub mod crypto;
pub mod identity;
pub mod spec;
pub mod state;
pub mod transaction;
pub use actions::*;
pub use block::*;
pub use identity::*;
pub use spec::*;
pub use state::*;
pub use transaction::*;

pub fn canonical_bytes<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, lattice_cashew::Error> {
    lattice_cashew::cbor::encode(value)
}
pub fn content_id<T: serde::Serialize>(value: &T) -> Result<String, lattice_cashew::Error> {
    Ok(lattice_cashew::cid_for_bytes(
        0x71,
        &canonical_bytes(value)?,
    ))
}
