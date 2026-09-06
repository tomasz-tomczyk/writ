//! Storage and decisions for `writ`.
//!
//! This crate has no I/O surface. It does not print, serve HTTP, or speak
//! MCP. When a function needs to report something, it returns it. See
//! invariant 1 in `AGENTS.md`.

mod error;
mod id;
mod migrate;
mod paths;
mod store;

pub use error::{Error, Result};
pub use id::new_id;
pub use migrate::SCHEMA_VERSION;
pub use paths::{Env, Paths, resolve_paths};
pub use store::Store;
