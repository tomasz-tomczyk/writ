//! Storage and decisions for `writ`.
//!
//! This crate has no I/O surface. It does not print, serve HTTP, or speak
//! MCP. When a function needs to report something, it returns it. See
//! invariant 1 in `AGENTS.md`.

mod audit;
mod config;
mod diff;
mod error;
mod fts;
mod glob;
mod id;
mod jsonl;
mod migrate;
mod model;
mod paths;
mod repo;
mod store;
mod telemetry;

pub use audit::{
    AuditScope, Budget, Candidate, FindingsInput, IncomingFinding, Ingested, Outcome, Outcomes,
    Selected, parse_findings, rank, render_prompt, rule_block,
};
pub use config::{Audit, Config, Identity, Telemetry, Ui};
pub use diff::{Diff, language_of, telemetry_language};
pub use error::{Error, Result};
pub use glob::glob_match;
pub use id::new_id;
pub use jsonl::parse_jsonl;
pub use migrate::SCHEMA_VERSION;
pub use model::{
    Exemplar, ExemplarKind, Finding, Learning, LearningUpdate, ListFilter, MatcherKind,
    NewExemplar, NewLearning, Recorded, Scope, ScopeKind, Sides, SourceKind, Status,
};
pub use paths::{Env, Paths, resolve_paths};
pub use repo::{RepoIdentity, normalize_remote};
pub use store::Store;
pub use telemetry::{
    BUCKET_EDGES, BucketMetric, BucketRow, CommandMetric, CounterMetric, CounterRow,
    FindingOutcomeMetric, GateResultMetric, HealthActionMetric, HookHostMetric, LanguageMetric,
    MatcherResultMetric, MetaRow, RecordSourceMetric, RecordStatusMetric, SurfaceMetric,
    TELEMETRY_FORMAT_VERSION, TelemetryBatch, TelemetryDump, TelemetryStore,
};
