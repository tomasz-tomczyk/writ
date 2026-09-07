//! Aggregate-only, local telemetry storage.
//!
//! The write surface is typed and finite. Callers can increment the metrics in
//! telemetry spec section 4, but cannot attach an arbitrary payload, path, rule
//! or event. The connection is always separate from the learnings [`Store`].

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::diff::telemetry_language;
use crate::error::{Error, Result};
use crate::model::{MatcherKind, ScopeKind};

/// Version of the JSON disclosure format, not the learnings schema.
pub const TELEMETRY_FORMAT_VERSION: u32 = 1;

/// The fixed distribution edges stored in every dump.
pub const BUCKET_EDGES: [&str; 5] = ["0", "1-5", "6-20", "21-50", "51+"];

/// A command name from the binary's finite command set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandMetric {
    Record,
    List,
    Audit,
    Show,
    Archive,
    Edit,
    Ui,
    Mcp,
}

impl CommandMetric {
    fn as_str(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::List => "list",
            Self::Audit => "audit",
            Self::Show => "show",
            Self::Archive => "archive",
            Self::Edit => "edit",
            Self::Ui => "ui",
            Self::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceMetric {
    Cli,
    Mcp,
    Ui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordSourceMetric {
    Manual,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatusMetric {
    Proposed,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherResultMetric {
    Hit,
    Miss,
    Unevaluable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookHostMetric {
    ClaudeCode,
    Codex,
    Cursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateResultMetric {
    Pass,
    Block,
    RetryCapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingOutcomeMetric {
    Fixed,
    Ignored,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthActionMetric {
    Archive,
    Edit,
    Keep,
}

/// A language label that can only be constructed through the fixed table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageMetric(&'static str);

impl LanguageMetric {
    pub fn from_path(path: &str) -> Self {
        Self(crate::diff::language_of(path).unwrap_or("other"))
    }

    pub fn from_label(label: &str) -> Self {
        Self(telemetry_language(label))
    }
}

/// One allowed counter increment. There is deliberately no `(String, String)`
/// variant: aggregate-by-construction is stronger than filtering content later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterMetric {
    Command(CommandMetric),
    Surface(SurfaceMetric),
    ExitCode(u8),
    RecordSource(RecordSourceMetric),
    RecordStatus(RecordStatusMetric),
    ScopeKind(ScopeKind),
    Language(LanguageMetric),
    MatcherKind(Option<MatcherKind>),
    MatcherResult(MatcherResultMetric),
    HookHost(HookHostMetric),
    GateResult(GateResultMetric),
    FindingOutcome(FindingOutcomeMetric),
    HealthAction(HealthActionMetric),
}

impl CounterMetric {
    fn parts(&self) -> (&'static str, String) {
        let (metric, label) = match self {
            Self::Command(value) => ("command", value.as_str()),
            Self::Surface(SurfaceMetric::Cli) => ("surface", "cli"),
            Self::Surface(SurfaceMetric::Mcp) => ("surface", "mcp"),
            Self::Surface(SurfaceMetric::Ui) => ("surface", "ui"),
            Self::ExitCode(value) => ("exit_code", exit_code_label(*value)),
            Self::RecordSource(RecordSourceMetric::Manual) => ("record_source", "manual"),
            Self::RecordSource(RecordSourceMetric::Json) => ("record_source", "json"),
            Self::RecordStatus(RecordStatusMetric::Proposed) => ("record_status", "proposed"),
            Self::RecordStatus(RecordStatusMetric::Active) => ("record_status", "active"),
            Self::ScopeKind(value) => ("scope_kind", value.as_str()),
            Self::Language(value) => ("language", value.0),
            Self::MatcherKind(Some(value)) => ("matcher_kind", value.as_str()),
            Self::MatcherKind(None) => ("matcher_kind", "none"),
            Self::MatcherResult(MatcherResultMetric::Hit) => ("matcher_result", "hit"),
            Self::MatcherResult(MatcherResultMetric::Miss) => ("matcher_result", "miss"),
            Self::MatcherResult(MatcherResultMetric::Unevaluable) => {
                ("matcher_result", "unevaluable")
            }
            Self::HookHost(HookHostMetric::ClaudeCode) => ("hook_host", "claude-code"),
            Self::HookHost(HookHostMetric::Codex) => ("hook_host", "codex"),
            Self::HookHost(HookHostMetric::Cursor) => ("hook_host", "cursor"),
            Self::GateResult(GateResultMetric::Pass) => ("gate_result", "pass"),
            Self::GateResult(GateResultMetric::Block) => ("gate_result", "block"),
            Self::GateResult(GateResultMetric::RetryCapped) => ("gate_result", "retry_capped"),
            Self::FindingOutcome(FindingOutcomeMetric::Fixed) => ("finding_outcome", "fixed"),
            Self::FindingOutcome(FindingOutcomeMetric::Ignored) => ("finding_outcome", "ignored"),
            Self::FindingOutcome(FindingOutcomeMetric::Rejected) => ("finding_outcome", "rejected"),
            Self::HealthAction(HealthActionMetric::Archive) => ("health_action", "archive"),
            Self::HealthAction(HealthActionMetric::Edit) => ("health_action", "edit"),
            Self::HealthAction(HealthActionMetric::Keep) => ("health_action", "keep"),
        };
        (metric, label.to_string())
    }
}

fn exit_code_label(code: u8) -> &'static str {
    match code {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "6",
        7 => "7",
        _ => "8",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketMetric {
    CollectionSize(u64),
    AuditConsidered(u64),
    AuditSent(u64),
    AuditFindings(u64),
    DiffFiles(u64),
    PromptChars(u64),
    CommandMs(u64),
}

impl BucketMetric {
    fn parts(self) -> (&'static str, &'static str) {
        let (metric, value) = match self {
            Self::CollectionSize(value) => ("collection_size", value),
            Self::AuditConsidered(value) => ("audit_considered", value),
            Self::AuditSent(value) => ("audit_sent", value),
            Self::AuditFindings(value) => ("audit_findings", value),
            Self::DiffFiles(value) => ("diff_files", value),
            Self::PromptChars(value) => ("prompt_chars", value),
            Self::CommandMs(value) => ("command_ms", value),
        };
        (metric, bucket_for(value))
    }
}

fn bucket_for(value: u64) -> &'static str {
    match value {
        0 => "0",
        1..=5 => "1-5",
        6..=20 => "6-20",
        21..=50 => "21-50",
        _ => "51+",
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TelemetryBatch {
    pub counters: Vec<CounterMetric>,
    pub buckets: Vec<BucketMetric>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaRow {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterRow {
    pub day: String,
    pub metric: String,
    pub label: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketRow {
    pub day: String,
    pub metric: String,
    pub bucket: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryDump {
    pub writ_telemetry: u32,
    pub install_id: String,
    pub writ_version: String,
    pub os: String,
    pub enabled_at: String,
    pub generated_at: String,
    pub bucket_edges: BTreeMap<String, Vec<String>>,
    pub counters: Vec<CounterRow>,
    pub buckets: Vec<BucketRow>,
}

/// A connection to `telemetry.db`, never to `learnings.db`.
pub struct TelemetryStore {
    conn: Connection,
}

impl TelemetryStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| Error::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch(include_str!("telemetry_schema.sql"))?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [TELEMETRY_FORMAT_VERSION.to_string()],
        )?;
        Ok(Self { conn })
    }

    /// Establish consent metadata. Re-enabling retains the random installation
    /// identity and moves only the day on which collection resumed.
    pub fn enable(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('install_id', ?1)",
            [Uuid::new_v4().to_string()],
        )?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('enabled_at', date('now','localtime'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Consent exists only after `writ telemetry on` has written both pieces
    /// of enable metadata. A hand-edited config value cannot manufacture it.
    pub fn is_enabled(&self) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT
                   EXISTS (SELECT 1 FROM meta WHERE key = 'install_id')
                   AND EXISTS (SELECT 1 FROM meta WHERE key = 'enabled_at')",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn record(&mut self, batch: &TelemetryBatch) -> Result<()> {
        if !self.is_enabled()? {
            return Err(Error::Storage {
                message: "telemetry has not been enabled by `writ telemetry on`".to_string(),
            });
        }
        let tx = self.conn.transaction()?;
        for counter in &batch.counters {
            let (metric, label) = counter.parts();
            tx.execute(
                "INSERT INTO counters (day, metric, label, count)
                 VALUES (date('now','localtime'), ?1, ?2, 1)
                 ON CONFLICT(day, metric, label)
                 DO UPDATE SET count = count + 1",
                params![metric, label],
            )?;
        }
        for bucket in &batch.buckets {
            let (metric, bucket) = bucket.parts();
            tx.execute(
                "INSERT INTO buckets (day, metric, bucket, count)
                 VALUES (date('now','localtime'), ?1, ?2, 1)
                 ON CONFLICT(day, metric, bucket)
                 DO UPDATE SET count = count + 1",
                params![metric, bucket],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn meta(&self) -> Result<Vec<MetaRow>> {
        let mut statement = self
            .conn
            .prepare("SELECT key, value FROM meta ORDER BY key")?;
        let rows = statement.query_map([], |row| {
            Ok(MetaRow {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn counters(&self) -> Result<Vec<CounterRow>> {
        let mut statement = self.conn.prepare(
            "SELECT day, metric, label, count FROM counters
             ORDER BY day, metric, label",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(CounterRow {
                day: row.get(0)?,
                metric: row.get(1)?,
                label: row.get(2)?,
                count: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn buckets(&self) -> Result<Vec<BucketRow>> {
        let mut statement = self.conn.prepare(
            "SELECT day, metric, bucket, count FROM buckets
             ORDER BY day, metric, bucket",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(BucketRow {
                day: row.get(0)?,
                metric: row.get(1)?,
                bucket: row.get(2)?,
                count: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn dump(&self, writ_version: &str, os: &str) -> Result<TelemetryDump> {
        let value = |key: &str| {
            self.conn
                .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                    row.get(0)
                })
        };
        let generated_at = self
            .conn
            .query_row("SELECT date('now','localtime')", [], |row| row.get(0))?;
        Ok(TelemetryDump {
            writ_telemetry: TELEMETRY_FORMAT_VERSION,
            install_id: value("install_id")?,
            writ_version: writ_version.to_string(),
            os: os.to_string(),
            enabled_at: value("enabled_at")?,
            generated_at,
            bucket_edges: bucket_edges(),
            counters: self.counters()?,
            buckets: self.buckets()?,
        })
    }
}

fn bucket_edges() -> BTreeMap<String, Vec<String>> {
    [
        "collection_size",
        "audit_considered",
        "audit_sent",
        "audit_findings",
        "diff_files",
        "prompt_chars",
        "command_ms",
    ]
    .into_iter()
    .map(|metric| {
        (
            metric.to_string(),
            BUCKET_EDGES
                .iter()
                .map(|edge| (*edge).to_string())
                .collect(),
        )
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_aggregate_only_and_has_exactly_the_three_public_tables() {
        let store = TelemetryStore::open_in_memory().unwrap();
        let mut statement = store
            .conn
            .prepare(
                "SELECT name FROM sqlite_schema
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        let names: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(names, ["buckets", "counters", "meta"]);
    }

    #[test]
    fn enabling_generates_a_uuid_v4_and_day_resolution_metadata() {
        let mut store = TelemetryStore::open_in_memory().unwrap();
        store.enable().unwrap();
        let meta: BTreeMap<_, _> = store
            .meta()
            .unwrap()
            .into_iter()
            .map(|row| (row.key, row.value))
            .collect();
        let id = Uuid::parse_str(&meta["install_id"]).unwrap();
        assert_eq!(id.get_version_num(), 4);
        assert_eq!(meta["enabled_at"].len(), 10);
        assert_eq!(meta["schema_version"], "1");
    }

    #[test]
    fn counters_and_named_buckets_upsert_without_raw_values() {
        let mut store = TelemetryStore::open_in_memory().unwrap();
        store.enable().unwrap();
        let batch = TelemetryBatch {
            counters: vec![
                CounterMetric::Command(CommandMetric::Audit),
                CounterMetric::Command(CommandMetric::Audit),
                CounterMetric::Language(LanguageMetric::from_label("secret-lang")),
            ],
            buckets: vec![BucketMetric::AuditSent(37), BucketMetric::AuditSent(37)],
        };
        store.record(&batch).unwrap();

        let counters = store.counters().unwrap();
        assert_eq!(counters.len(), 2);
        assert_eq!(counters[0].metric, "command");
        assert_eq!(counters[0].count, 2);
        assert_eq!(counters[1].label, "other");
        let buckets = store.buckets().unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].bucket, "21-50");
        assert_eq!(buckets[0].count, 2);
    }

    #[test]
    fn dump_round_trips_with_every_versioned_edge() {
        let mut store = TelemetryStore::open_in_memory().unwrap();
        store.enable().unwrap();
        store
            .record(&TelemetryBatch {
                counters: vec![CounterMetric::Surface(SurfaceMetric::Cli)],
                buckets: vec![BucketMetric::CommandMs(6)],
            })
            .unwrap();

        let dump = store.dump("0.1.0", "linux").unwrap();
        assert_eq!(dump.writ_telemetry, 1);
        assert_eq!(dump.bucket_edges.len(), 7);
        assert!(
            dump.bucket_edges
                .values()
                .all(|edges| edges == &BUCKET_EDGES)
        );
        let json = serde_json::to_string(&dump).unwrap();
        let decoded: TelemetryDump = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, dump);
    }

    #[test]
    fn section_four_has_a_typed_path_for_every_counter_and_bucket() {
        let mut store = TelemetryStore::open_in_memory().unwrap();
        store.enable().unwrap();
        store
            .record(&TelemetryBatch {
                counters: vec![
                    CounterMetric::Command(CommandMetric::Record),
                    CounterMetric::Surface(SurfaceMetric::Mcp),
                    CounterMetric::ExitCode(8),
                    CounterMetric::RecordSource(RecordSourceMetric::Json),
                    CounterMetric::RecordStatus(RecordStatusMetric::Active),
                    CounterMetric::ScopeKind(ScopeKind::Glob),
                    CounterMetric::Language(LanguageMetric::from_label("rust")),
                    CounterMetric::MatcherKind(Some(MatcherKind::AstGrep)),
                    CounterMetric::MatcherResult(MatcherResultMetric::Unevaluable),
                    CounterMetric::HookHost(HookHostMetric::ClaudeCode),
                    CounterMetric::GateResult(GateResultMetric::RetryCapped),
                    CounterMetric::FindingOutcome(FindingOutcomeMetric::Rejected),
                    CounterMetric::HealthAction(HealthActionMetric::Keep),
                ],
                buckets: vec![
                    BucketMetric::CollectionSize(0),
                    BucketMetric::AuditConsidered(1),
                    BucketMetric::AuditSent(6),
                    BucketMetric::AuditFindings(21),
                    BucketMetric::DiffFiles(51),
                    BucketMetric::PromptChars(999),
                    BucketMetric::CommandMs(5),
                ],
            })
            .unwrap();

        let counter_names: std::collections::BTreeSet<_> = store
            .counters()
            .unwrap()
            .into_iter()
            .map(|row| row.metric)
            .collect();
        assert_eq!(
            counter_names,
            [
                "command",
                "exit_code",
                "finding_outcome",
                "gate_result",
                "health_action",
                "hook_host",
                "language",
                "matcher_kind",
                "matcher_result",
                "record_source",
                "record_status",
                "scope_kind",
                "surface",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        );
        let bucket_names: std::collections::BTreeSet<_> = store
            .buckets()
            .unwrap()
            .into_iter()
            .map(|row| row.metric)
            .collect();
        assert_eq!(bucket_names, bucket_edges().into_keys().collect());
    }

    #[test]
    fn observations_require_enable_metadata_not_just_an_open_database() {
        let mut store = TelemetryStore::open_in_memory().unwrap();
        let error = store
            .record(&TelemetryBatch {
                counters: vec![CounterMetric::Surface(SurfaceMetric::Cli)],
                buckets: Vec::new(),
            })
            .unwrap_err();
        assert!(error.to_string().contains("telemetry on"), "{error}");
        assert!(store.counters().unwrap().is_empty());
    }
}
