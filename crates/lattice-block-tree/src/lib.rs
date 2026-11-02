//! Consensus fact reducer and exact-work block graph.
//! Durable replay accepts locally authenticated persisted facts, never untrusted wire claims.
mod euler;
mod facts;
pub use facts::*;

mod tree;
pub use tree::*;

mod proof;
#[cfg(test)]
mod tests;
pub use proof::*;
mod continuity;
#[cfg(test)]
mod proof_tests;
pub use continuity::*;

mod template_selection;
pub use template_selection::*;
