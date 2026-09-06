//! Storage and decisions for `writ`.
//!
//! This crate has no I/O surface. It does not print, serve HTTP, or speak
//! MCP. When a function needs to report something, it returns it. See
//! invariant 1 in `AGENTS.md`.

mod config;
mod error;
mod fts;
mod id;
mod jsonl;
mod migrate;
mod model;
mod paths;
mod store;

pub use config::{Audit, BlockAbove, Config, Dedupe, Identity, Ui};
pub use error::{Error, Result};
pub use id::new_id;
pub use jsonl::parse_jsonl;
pub use migrate::SCHEMA_VERSION;
pub use model::{
    Exemplar, ExemplarKind, Learning, ListFilter, MatcherKind, NearMatch, NewExemplar, NewLearning,
    Recorded, Scope, ScopeKind, SourceKind, Status,
};
pub use paths::{Env, Paths, resolve_paths};
pub use store::Store;
