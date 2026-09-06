use std::path::Path;

use rusqlite::{Connection, Row, Transaction, params};

use crate::error::{Error, Result};
use crate::fts;
use crate::id::new_id;
use crate::migrate;
use crate::model::{
    Exemplar, ExemplarKind, Learning, ListFilter, MatcherKind, NearMatch, NewExemplar, NewLearning,
    Recorded, Scope, ScopeKind, SourceKind, Status,
};

/// An open writ database.
///
/// The connection stays private. `writ-core` owns every statement, so no
/// caller can reach past the API and write a row the rules do not allow.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open the database at `path` and migrate it.
    ///
    /// The parent directory is created when it is missing, because the XDG
    /// data directory does not exist on a first run.
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
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::from_connection(conn)
    }

    /// Open a private in-memory database and migrate it. Tests use this.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        // OFF is the SQLite default. Setting it here makes the guarantee
        // the `updated_at` triggers rely on explicit, so a later change
        // cannot turn recursion on without touching this line.
        conn.pragma_update(None, "recursive_triggers", false)?;
        migrate::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// The schema version this database records.
    pub fn schema_version(&self) -> Result<i64> {
        migrate::current_version(&self.conn)
    }

    /// Write one learning.
    ///
    /// `warn_top_n` comes from `[dedupe] warn_top_n`. MVP warns and always
    /// writes, so the near matches are reported, never obeyed. Spec 7.3.
    pub fn record(&mut self, learning: &NewLearning, warn_top_n: u32) -> Result<Recorded> {
        Ok(self
            .record_many(std::slice::from_ref(learning), warn_top_n)?
            .remove(0))
    }

    /// Write many learnings in one transaction.
    ///
    /// One transaction, so a stream either lands whole or not at all and a
    /// re-run cannot duplicate half of it. See `jsonl`.
    pub fn record_many(
        &mut self,
        learnings: &[NewLearning],
        warn_top_n: u32,
    ) -> Result<Vec<Recorded>> {
        for learning in learnings {
            learning.validate()?;
        }
        let tx = self.conn.transaction()?;
        let mut written = Vec::with_capacity(learnings.len());
        for learning in learnings {
            written.push(insert_learning(&tx, learning, warn_top_n)?);
        }
        tx.commit()?;
        Ok(written)
    }

    /// Attach to an existing learning instead of creating one.
    ///
    /// It increments `reinforced` and appends the exemplars. `updated_at`
    /// is left to the trigger, per invariant 7.
    pub fn reinforce(
        &mut self,
        id: &str,
        exemplars: &[NewExemplar],
        status: Option<Status>,
    ) -> Result<Recorded> {
        for exemplar in exemplars {
            if exemplar.snippet.is_empty() {
                return Err(Error::validation("an exemplar snippet is empty"));
            }
        }
        let tx = self.conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM learnings WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(Error::NotFound { id: id.to_string() });
        }
        tx.execute(
            "UPDATE learnings SET reinforced = reinforced + 1 WHERE id = ?1",
            [id],
        )?;
        for exemplar in exemplars {
            insert_exemplar(&tx, id, exemplar, None)?;
        }
        if let Some(status) = status {
            set_status(&tx, id, status)?;
        }
        tx.commit()?;
        Ok(Recorded {
            id: id.to_string(),
            reinforced: true,
            near_matches: Vec::new(),
        })
    }

    /// Move a learning to another status.
    ///
    /// `activated_at` is stamped the first time a learning becomes active
    /// and never again: it is the clock recurrence is measured against, so
    /// a second activation must not move it. Spec section 6.
    pub fn set_status(&mut self, id: &str, status: Status) -> Result<()> {
        let tx = self.conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM learnings WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(Error::NotFound { id: id.to_string() });
        }
        set_status(&tx, id, status)?;
        tx.commit()?;
        Ok(())
    }

    /// The learnings closest to this text, best match first.
    ///
    /// bm25 is negative in SQLite and more negative is a better match, so
    /// the ordering is ascending. Spec section 7.3 names this trap.
    pub fn near_matches(&self, text: &str, limit: u32) -> Result<Vec<NearMatch>> {
        near_matches(&self.conn, text, limit)
    }

    /// Read learnings back, filtered. This is `writ list`.
    pub fn list(&self, filter: &ListFilter) -> Result<Vec<Learning>> {
        let mut sql = String::from(
            "SELECT id, created_at, updated_at, status, title, rule, rationale, blocking,
                    matcher_kind, matcher, source_kind, source_adapter, source_ref,
                    author, activated_at, reinforced, times_applied, last_used_at,
                    last_verified
             FROM learnings WHERE 1 = 1",
        );
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(status) = filter.status {
            sql.push_str(" AND status = ?");
            values.push(Box::new(status.as_str().to_string()));
        }
        if let Some(scope) = &filter.scope {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM learning_scopes s
                              WHERE s.learning_id = learnings.id
                                AND s.kind = ? AND s.value = ?)",
            );
            values.push(Box::new(scope.kind.as_str().to_string()));
            values.push(Box::new(scope.value.clone()));
        }
        if filter.never_used && filter.stale_days.is_some() {
            // The two buckets are disjoint, so the pair can only ever
            // return nothing. Say that rather than print an empty list.
            return Err(Error::validation(
                "--never-used and --stale-days are disjoint. Ask for one of them",
            ));
        }
        if filter.never_used {
            sql.push_str(" AND last_used_at IS NULL");
        }
        if let Some(days) = filter.stale_days {
            // The boundary is inclusive: exactly N days of silence is
            // stale. A learning no audit ever selected is not stale, it is
            // never used, which is the other bucket.
            sql.push_str(" AND last_used_at IS NOT NULL AND last_used_at <= datetime('now', ?)");
            values.push(Box::new(format!("-{days} days")));
        }
        if let Some(search) = &filter.search {
            let Some(query) = fts::match_all(search) else {
                // The search holds no searchable term, so nothing matches.
                return Ok(Vec::new());
            };
            sql.push_str(
                " AND rowid IN (SELECT rowid FROM learnings_fts
                                WHERE learnings_fts MATCH ?)",
            );
            values.push(Box::new(query));
        }
        // UUIDv7 sorts by creation time, so this is oldest first and stable.
        sql.push_str(" ORDER BY id");

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|value| value.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), read_learning)?;
        let mut learnings = Vec::new();
        for row in rows {
            let mut learning = row?;
            learning.scopes = self.scopes_of(&learning.id)?;
            learnings.push(learning);
        }
        Ok(learnings)
    }

    /// The exemplars attached to one learning, oldest first.
    pub fn exemplars_of(&self, id: &str) -> Result<Vec<Exemplar>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, language, snippet, note FROM exemplars
             WHERE learning_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([id], |row| {
            Ok(Exemplar {
                id: row.get(0)?,
                kind: exemplar_kind(row, 1)?,
                language: row.get(2)?,
                snippet: row.get(3)?,
                note: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn scopes_of(&self, id: &str) -> Result<Vec<Scope>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, value FROM learning_scopes
             WHERE learning_id = ?1 ORDER BY kind, value",
        )?;
        let rows = stmt.query_map([id], |row| {
            let kind: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((kind, value))
        })?;
        let mut scopes = Vec::new();
        for row in rows {
            let (kind, value) = row?;
            scopes.push(Scope {
                kind: scope_kind(&kind)?,
                value,
            });
        }
        Ok(scopes)
    }
}

fn insert_learning(
    tx: &Transaction<'_>,
    learning: &NewLearning,
    warn_top_n: u32,
) -> Result<Recorded> {
    let near_matches = if warn_top_n == 0 {
        Vec::new()
    } else {
        near_matches(
            tx,
            &format!("{} {}", learning.title, learning.rule),
            warn_top_n,
        )?
    };

    let id = new_id();
    let status = learning.effective_status();
    tx.execute(
        "INSERT INTO learnings
           (id, created_at, updated_at, status, title, rule, rationale, blocking,
            matcher_kind, matcher, source_kind, source_adapter, source_ref,
            author, activated_at)
         VALUES
           (?1,
            COALESCE(?2, datetime('now')),
            COALESCE(?3, datetime('now')),
            ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            COALESCE(?15, CASE WHEN ?4 = 'active' THEN datetime('now') END))",
        params![
            &id,
            &learning.created_at,
            &learning.updated_at,
            status.as_str(),
            &learning.title,
            &learning.rule,
            &learning.rationale,
            learning.blocking,
            learning.matcher_kind.map(MatcherKind::as_str),
            &learning.matcher,
            learning.source_kind.unwrap_or(SourceKind::Manual).as_str(),
            &learning.source_adapter,
            &learning.source_ref,
            &learning.author,
            &learning.activated_at,
        ],
    )?;

    for scope in learning.effective_scopes() {
        tx.execute(
            "INSERT INTO learning_scopes (learning_id, kind, value, updated_at)
             VALUES (?1, ?2, ?3, COALESCE(?4, datetime('now')))",
            params![&id, scope.kind.as_str(), &scope.value, &learning.updated_at],
        )?;
    }
    for exemplar in &learning.exemplars {
        insert_exemplar(tx, &id, exemplar, learning.updated_at.as_deref())?;
    }

    Ok(Recorded {
        id,
        reinforced: false,
        near_matches,
    })
}

fn insert_exemplar(
    tx: &Transaction<'_>,
    learning_id: &str,
    exemplar: &NewExemplar,
    updated_at: Option<&str>,
) -> Result<()> {
    tx.execute(
        "INSERT INTO exemplars (id, learning_id, kind, language, snippet, note, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE(?7, datetime('now')))",
        params![
            new_id(),
            learning_id,
            exemplar.kind.as_str(),
            &exemplar.language,
            &exemplar.snippet,
            &exemplar.note,
            updated_at,
        ],
    )?;
    Ok(())
}

/// `updated_at` is never named here. The trigger owns it. Invariant 7.
fn set_status(tx: &Transaction<'_>, id: &str, status: Status) -> Result<()> {
    tx.execute(
        "UPDATE learnings
            SET status = ?1,
                activated_at = CASE
                  WHEN ?1 = 'active' AND activated_at IS NULL THEN datetime('now')
                  ELSE activated_at
                END
          WHERE id = ?2",
        params![status.as_str(), id],
    )?;
    Ok(())
}

fn near_matches(conn: &Connection, text: &str, limit: u32) -> Result<Vec<NearMatch>> {
    let Some(query) = fts::match_any(text) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT l.id, l.title, l.status, bm25(learnings_fts) AS score
           FROM learnings_fts
           JOIN learnings l ON l.rowid = learnings_fts.rowid
          WHERE learnings_fts MATCH ?1
          ORDER BY score ASC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query, limit], |row| {
        let status: String = row.get(2)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            status,
            row.get::<_, f64>(3)?,
        ))
    })?;
    let mut matches = Vec::new();
    for row in rows {
        let (id, title, status, score) = row?;
        matches.push(NearMatch {
            id,
            title,
            status: parse_status(&status)?,
            score,
        });
    }
    Ok(matches)
}

fn read_learning(row: &Row<'_>) -> rusqlite::Result<Learning> {
    let status: String = row.get(3)?;
    let source_kind: String = row.get(10)?;
    let matcher_kind: Option<String> = row.get(8)?;
    Ok(Learning {
        id: row.get(0)?,
        created_at: row.get(1)?,
        updated_at: row.get(2)?,
        status: parse_status(&status).map_err(to_sqlite_error)?,
        title: row.get(4)?,
        rule: row.get(5)?,
        rationale: row.get(6)?,
        blocking: row.get(7)?,
        matcher_kind: matcher_kind
            .map(|kind| kind.parse::<MatcherKind>())
            .transpose()
            .map_err(to_sqlite_error)?,
        matcher: row.get(9)?,
        source_kind: parse_source_kind(&source_kind).map_err(to_sqlite_error)?,
        source_adapter: row.get(11)?,
        source_ref: row.get(12)?,
        author: row.get(13)?,
        activated_at: row.get(14)?,
        reinforced: row.get(15)?,
        times_applied: row.get(16)?,
        last_used_at: row.get(17)?,
        last_verified: row.get(18)?,
        scopes: Vec::new(),
    })
}

fn exemplar_kind(row: &Row<'_>, index: usize) -> rusqlite::Result<ExemplarKind> {
    let kind: String = row.get(index)?;
    kind.parse().map_err(to_sqlite_error)
}

/// A CHECK constraint guarantees these values, so an unknown one means the
/// database was written by something other than writ. Say that, do not
/// guess. See P7.
fn parse_status(text: &str) -> Result<Status> {
    text.parse()
}

fn parse_source_kind(text: &str) -> Result<SourceKind> {
    match text {
        "manual" => Ok(SourceKind::Manual),
        "session" => Ok(SourceKind::Session),
        "import" => Ok(SourceKind::Import),
        other => Err(Error::validation(format!(
            "the database holds an unknown source_kind {other}"
        ))),
    }
}

fn scope_kind(text: &str) -> Result<ScopeKind> {
    match text {
        "global" => Ok(ScopeKind::Global),
        "project" => Ok(ScopeKind::Project),
        "language" => Ok(ScopeKind::Language),
        "glob" => Ok(ScopeKind::Glob),
        other => Err(Error::validation(format!(
            "the database holds an unknown scope kind {other}"
        ))),
    }
}

fn to_sqlite_error(error: Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::new_id;
    use crate::migrate::SCHEMA_VERSION;

    fn table_names(store: &Store) -> Vec<String> {
        let mut stmt = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect()
    }

    fn insert_learning(store: &Store, id: &str, title: &str, rule: &str) {
        store
            .conn
            .execute(
                "INSERT INTO learnings (id, title, rule, rationale, source_kind)
                 VALUES (?1, ?2, ?3, 'because', 'manual')",
                (id, title, rule),
            )
            .unwrap();
    }

    fn fts_hits(store: &Store, query: &str) -> i64 {
        store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM learnings_fts WHERE learnings_fts MATCH ?1",
                [query],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn migrating_an_empty_database_creates_every_table() {
        let store = Store::open_in_memory().unwrap();
        let names = table_names(&store);
        for expected in [
            "audits",
            "exemplars",
            "findings",
            "learning_scopes",
            "learnings",
            "learnings_fts",
            "writ_migrations",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("learnings.db");

        let first = Store::open(&path).unwrap();
        insert_learning(&first, &new_id(), "keep me", "do the thing");
        let tables = table_names(&first);
        drop(first);

        let second = Store::open(&path).unwrap();
        assert_eq!(table_names(&second), tables);
        assert_eq!(second.schema_version().unwrap(), SCHEMA_VERSION);
        let rows: i64 = second
            .conn
            .query_row("SELECT COUNT(*) FROM learnings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1, "a second migration must not touch the data");
        let applied: i64 = second
            .conn
            .query_row("SELECT COUNT(*) FROM writ_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(applied, SCHEMA_VERSION);
    }

    #[test]
    fn inserting_a_learning_fills_the_fts_index() {
        let store = Store::open_in_memory().unwrap();
        insert_learning(&store, &new_id(), "prefer sd", "use sd instead of sed");
        assert_eq!(fts_hits(&store, "sed"), 1);
    }

    #[test]
    fn updating_a_rule_updates_the_fts_row() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "prefer sd", "use sd instead of sed");

        store
            .conn
            .execute(
                "UPDATE learnings SET rule = 'use ripgrep instead of grep' WHERE id = ?1",
                [&id],
            )
            .unwrap();

        assert_eq!(fts_hits(&store, "sed"), 0, "the stale term must be gone");
        assert_eq!(fts_hits(&store, "ripgrep"), 1);
    }

    #[test]
    fn deleting_a_learning_removes_the_fts_row() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "prefer sd", "use sd instead of sed");

        store
            .conn
            .execute("DELETE FROM learnings WHERE id = ?1", [&id])
            .unwrap();

        assert_eq!(fts_hits(&store, "sed"), 0);
    }

    #[test]
    fn the_fts_index_stays_consistent_with_its_content_table() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "one", "alpha");
        insert_learning(&store, &new_id(), "two", "beta");
        store
            .conn
            .execute("UPDATE learnings SET rule = 'gamma' WHERE id = ?1", [&id])
            .unwrap();
        store
            .conn
            .execute("DELETE FROM learnings WHERE id = ?1", [&id])
            .unwrap();

        // An external content table cannot repair itself. This check fails
        // loudly when a trigger is missing or wrong.
        store
            .conn
            .execute_batch(
                "INSERT INTO learnings_fts (learnings_fts, rank)
                 VALUES ('integrity-check', 1)",
            )
            .unwrap();
    }

    #[test]
    fn scopes_reject_a_duplicate_kind_and_value() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");

        let insert = "INSERT INTO learning_scopes (learning_id, kind, value) VALUES (?1, ?2, ?3)";
        store
            .conn
            .execute(insert, (&id, "language", "rust"))
            .unwrap();
        let error = store
            .conn
            .execute(insert, (&id, "language", "rust"))
            .unwrap_err();
        assert!(error.to_string().contains("UNIQUE"), "{error}");
    }

    #[test]
    fn a_global_scope_cannot_be_added_twice() {
        // `value` is NOT NULL DEFAULT '' precisely for this. SQLite allows
        // NULL in a primary key column, so a nullable value would let two
        // `global` rows through. Spec section 6.
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");

        let insert = "INSERT INTO learning_scopes (learning_id, kind) VALUES (?1, 'global')";
        store.conn.execute(insert, [&id]).unwrap();
        let error = store.conn.execute(insert, [&id]).unwrap_err();
        assert!(error.to_string().contains("UNIQUE"), "{error}");

        let value: String = store
            .conn
            .query_row("SELECT value FROM learning_scopes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "");
    }

    #[test]
    fn deleting_a_learning_cascades_to_scopes_and_exemplars() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");
        store
            .conn
            .execute(
                "INSERT INTO learning_scopes (learning_id, kind, value) VALUES (?1, 'project', 'x')",
                [&id],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO exemplars (id, learning_id, kind, snippet)
                 VALUES (?1, ?2, 'good', 'let x = 1;')",
                (new_id(), &id),
            )
            .unwrap();

        store
            .conn
            .execute("DELETE FROM learnings WHERE id = ?1", [&id])
            .unwrap();

        for table in ["learning_scopes", "exemplars"] {
            let rows: i64 = store
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(rows, 0, "{table} still holds an orphan");
        }
    }

    /// A timestamp far enough in the past that any real clock beats it.
    ///
    /// `datetime('now')` has one-second resolution, so a test that updates
    /// a row in the same second it was inserted can read back the value it
    /// started with. Backdating the row first removes the race without a
    /// sleep, and it costs nothing: the guard on the trigger lets an
    /// explicit `updated_at` through untouched, which is what makes the
    /// backdate stick.
    const BACKDATED: &str = "2000-01-01 00:00:00";

    fn read_one(store: &Store, sql: &str) -> String {
        store.conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn updating_a_learning_moves_updated_at() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");
        store
            .conn
            .execute(
                "UPDATE learnings SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
                (BACKDATED, &id),
            )
            .unwrap();

        store
            .conn
            .execute("UPDATE learnings SET rule = 'moved' WHERE id = ?1", [&id])
            .unwrap();

        let updated = read_one(&store, "SELECT updated_at FROM learnings");
        assert!(
            updated.as_str() > BACKDATED,
            "updated_at stayed at {updated}"
        );
        assert_eq!(
            read_one(&store, "SELECT created_at FROM learnings"),
            BACKDATED,
            "created_at must not move"
        );
    }

    #[test]
    fn updating_a_scope_moves_updated_at() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");
        store
            .conn
            .execute(
                "INSERT INTO learning_scopes (learning_id, kind, value, updated_at)
                 VALUES (?1, 'language', 'rust', ?2)",
                (&id, BACKDATED),
            )
            .unwrap();

        store
            .conn
            .execute(
                "UPDATE learning_scopes SET value = 'elixir' WHERE learning_id = ?1",
                [&id],
            )
            .unwrap();

        let updated = read_one(&store, "SELECT updated_at FROM learning_scopes");
        assert!(
            updated.as_str() > BACKDATED,
            "updated_at stayed at {updated}"
        );
    }

    #[test]
    fn updating_an_exemplar_moves_updated_at() {
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");
        let exemplar = new_id();
        store
            .conn
            .execute(
                "INSERT INTO exemplars (id, learning_id, kind, snippet, updated_at)
                 VALUES (?1, ?2, 'good', 'let x = 1;', ?3)",
                (&exemplar, &id, BACKDATED),
            )
            .unwrap();

        store
            .conn
            .execute(
                "UPDATE exemplars SET note = 'read this one' WHERE id = ?1",
                [&exemplar],
            )
            .unwrap();

        let updated = read_one(&store, "SELECT updated_at FROM exemplars");
        assert!(
            updated.as_str() > BACKDATED,
            "updated_at stayed at {updated}"
        );
    }

    #[test]
    fn an_explicit_updated_at_wins() {
        // The trigger guards on `new.updated_at = old.updated_at`. A caller
        // that sets the column means it, and an import keeps its own
        // timestamps. The guard is also what stops the trigger recursing.
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");

        store
            .conn
            .execute(
                "UPDATE learnings SET rule = 'x', updated_at = ?1 WHERE id = ?2",
                (BACKDATED, &id),
            )
            .unwrap();

        assert_eq!(
            read_one(&store, "SELECT updated_at FROM learnings"),
            BACKDATED
        );
    }

    #[test]
    fn touching_updated_at_leaves_the_fts_index_consistent() {
        // The updated_at trigger writes back to `learnings`, which could
        // fire the FTS trigger a second time with the wrong `old` values.
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "prefer sd", "use sd instead of sed");
        store
            .conn
            .execute(
                "UPDATE learnings SET rule = 'use ripgrep instead of grep' WHERE id = ?1",
                [&id],
            )
            .unwrap();

        assert_eq!(fts_hits(&store, "sed"), 0);
        assert_eq!(fts_hits(&store, "ripgrep"), 1);
        store
            .conn
            .execute_batch(
                "INSERT INTO learnings_fts (learnings_fts, rank)
                 VALUES ('integrity-check', 1)",
            )
            .unwrap();
    }

    fn record(store: &mut Store, learning: &NewLearning) -> String {
        store.record(learning, 0).unwrap().id
    }

    #[test]
    fn recording_writes_the_row_and_a_global_scope() {
        let mut store = Store::open_in_memory().unwrap();
        let id = record(
            &mut store,
            &NewLearning::new("prefer sd", "use sd", "sed is terse"),
        );

        let listed = store.list(&ListFilter::default()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].status, Status::Proposed);
        assert!(listed[0].blocking, "blocking is the default");
        assert_eq!(listed[0].source_kind, SourceKind::Manual);
        assert_eq!(listed[0].scopes, vec![Scope::global()]);
        assert_eq!(listed[0].activated_at, None);
    }

    #[test]
    fn recording_active_stamps_activated_at() {
        let mut store = Store::open_in_memory().unwrap();
        let mut learning = NewLearning::new("t", "r", "why");
        learning.status = Some(Status::Active);
        let id = record(&mut store, &learning);

        let listed = store.list(&ListFilter::default()).unwrap();
        assert_eq!(listed[0].status, Status::Active);
        assert!(listed[0].activated_at.is_some(), "activated_at is unset");
        assert_eq!(listed[0].id, id);
    }

    #[test]
    fn activating_twice_does_not_move_activated_at() {
        // activated_at is the clock recurrence is measured against. Spec 6.
        let mut store = Store::open_in_memory().unwrap();
        let mut learning = NewLearning::new("t", "r", "why");
        learning.status = Some(Status::Active);
        learning.activated_at = Some(BACKDATED.to_string());
        let id = record(&mut store, &learning);

        store.set_status(&id, Status::Active).unwrap();

        let listed = store.list(&ListFilter::default()).unwrap();
        assert_eq!(listed[0].activated_at.as_deref(), Some(BACKDATED));
    }

    #[test]
    fn activating_a_proposed_learning_stamps_activated_at_once() {
        let mut store = Store::open_in_memory().unwrap();
        let id = record(&mut store, &NewLearning::new("t", "r", "why"));
        assert_eq!(
            store.list(&ListFilter::default()).unwrap()[0].activated_at,
            None
        );

        store.set_status(&id, Status::Active).unwrap();
        let first = store.list(&ListFilter::default()).unwrap()[0]
            .activated_at
            .clone()
            .unwrap();

        store.set_status(&id, Status::Archived).unwrap();
        store.set_status(&id, Status::Active).unwrap();
        assert_eq!(
            store.list(&ListFilter::default()).unwrap()[0].activated_at,
            Some(first),
            "the first activation is the one that counts"
        );
    }

    #[test]
    fn a_status_change_on_an_unknown_id_says_so() {
        let mut store = Store::open_in_memory().unwrap();
        let error = store.set_status("nope", Status::Active).unwrap_err();
        assert!(matches!(error, Error::NotFound { .. }), "{error}");
    }

    #[test]
    fn an_imported_learning_keeps_the_timestamps_it_arrived_with() {
        // The one exception to invariant 7. This is an INSERT, where the
        // trigger does not run at all.
        let mut store = Store::open_in_memory().unwrap();
        let mut learning = NewLearning::new("t", "r", "why");
        learning.created_at = Some(BACKDATED.to_string());
        learning.updated_at = Some(BACKDATED.to_string());
        record(&mut store, &learning);

        let listed = store.list(&ListFilter::default()).unwrap();
        assert_eq!(listed[0].created_at, BACKDATED);
        assert_eq!(listed[0].updated_at, BACKDATED);
    }

    #[test]
    fn an_exemplar_survives_the_round_trip_byte_for_byte() {
        let mut store = Store::open_in_memory().unwrap();
        let snippet = "fn main() {\n    let x = 1;\n}\n";
        let mut learning = NewLearning::new("t", "r", "why");
        learning.exemplars = vec![NewExemplar {
            kind: ExemplarKind::Good,
            language: Some("rust".into()),
            snippet: snippet.to_string(),
            note: None,
        }];
        let id = record(&mut store, &learning);

        let exemplars = store.exemplars_of(&id).unwrap();
        assert_eq!(exemplars.len(), 1);
        assert_eq!(exemplars[0].snippet, snippet);
        assert_eq!(exemplars[0].kind, ExemplarKind::Good);
    }

    #[test]
    fn a_bad_learning_writes_nothing_from_the_batch() {
        let mut store = Store::open_in_memory().unwrap();
        let good = NewLearning::new("t", "r", "why");
        let bad = NewLearning::new("t", "r", "");
        let error = store.record_many(&[good, bad], 0).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
        assert!(store.list(&ListFilter::default()).unwrap().is_empty());
    }

    #[test]
    fn reinforcing_bumps_the_counter_and_appends_the_exemplar() {
        let mut store = Store::open_in_memory().unwrap();
        let id = record(&mut store, &NewLearning::new("t", "r", "why"));
        store
            .conn
            .execute(
                "UPDATE learnings SET updated_at = ?1 WHERE id = ?2",
                (BACKDATED, &id),
            )
            .unwrap();

        let written = store
            .reinforce(
                &id,
                &[NewExemplar {
                    kind: ExemplarKind::Bad,
                    language: None,
                    snippet: "sed -i".into(),
                    note: None,
                }],
                None,
            )
            .unwrap();

        assert!(written.reinforced);
        assert_eq!(written.id, id);
        let listed = store.list(&ListFilter::default()).unwrap();
        assert_eq!(listed.len(), 1, "reinforce must not create a row");
        assert_eq!(listed[0].reinforced, 1);
        assert!(
            listed[0].updated_at.as_str() > BACKDATED,
            "the trigger must move updated_at"
        );
        assert_eq!(store.exemplars_of(&id).unwrap().len(), 1);
    }

    #[test]
    fn reinforcing_an_unknown_id_says_so() {
        let mut store = Store::open_in_memory().unwrap();
        let error = store.reinforce("nope", &[], None).unwrap_err();
        assert!(matches!(error, Error::NotFound { .. }), "{error}");
    }

    #[test]
    fn a_near_match_is_reported_and_the_write_still_happens() {
        // MVP warns and always writes. Spec section 7.3.
        let mut store = Store::open_in_memory().unwrap();
        record(
            &mut store,
            &NewLearning::new("prefer sd", "use sd not sed", "why"),
        );

        let written = store
            .record(&NewLearning::new("prefer sd", "use sd not sed", "why"), 3)
            .unwrap();
        assert_eq!(written.near_matches.len(), 1);
        assert!(
            written.near_matches[0].score < 0.0,
            "bm25 is negative: {:?}",
            written.near_matches[0]
        );
        assert_eq!(store.list(&ListFilter::default()).unwrap().len(), 2);
    }

    #[test]
    fn a_near_match_query_survives_fts5_operator_characters() {
        // Raw, this title is an FTS5 syntax error. Spec section 7.3.
        let mut store = Store::open_in_memory().unwrap();
        let title = r#"no non-empty "x" OR NEAR sd*"#;
        record(&mut store, &NewLearning::new(title, "r", "why"));
        let written = store
            .record(&NewLearning::new(title, "r", "why"), 3)
            .unwrap();
        assert_eq!(written.near_matches.len(), 1);
    }

    #[test]
    fn near_match_ranking_puts_the_closest_first() {
        let mut store = Store::open_in_memory().unwrap();
        record(
            &mut store,
            &NewLearning::new("unrelated", "sd appears once", "why"),
        );
        record(
            &mut store,
            &NewLearning::new("prefer sd", "use sd instead of sed", "why"),
        );

        let found = store.near_matches("prefer sd instead of sed", 5).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "prefer sd");
        assert!(found[0].score <= found[1].score, "{found:?}");
    }

    #[test]
    fn listing_filters_by_status() {
        let mut store = Store::open_in_memory().unwrap();
        record(&mut store, &NewLearning::new("proposed one", "r", "why"));
        let mut active = NewLearning::new("active one", "r", "why");
        active.status = Some(Status::Active);
        record(&mut store, &active);

        let filter = ListFilter {
            status: Some(Status::Active),
            ..ListFilter::default()
        };
        let listed = store.list(&filter).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "active one");
    }

    #[test]
    fn listing_filters_by_scope() {
        let mut store = Store::open_in_memory().unwrap();
        let mut rust = NewLearning::new("rust one", "r", "why");
        rust.scopes = vec!["language:rust".parse().unwrap()];
        record(&mut store, &rust);
        record(&mut store, &NewLearning::new("global one", "r", "why"));

        let filter = ListFilter {
            scope: Some("language:rust".parse().unwrap()),
            ..ListFilter::default()
        };
        assert_eq!(store.list(&filter).unwrap().len(), 1);

        let filter = ListFilter {
            scope: Some(Scope::global()),
            ..ListFilter::default()
        };
        assert_eq!(store.list(&filter).unwrap()[0].title, "global one");
    }

    #[test]
    fn listing_searches_title_rule_and_rationale() {
        let mut store = Store::open_in_memory().unwrap();
        record(
            &mut store,
            &NewLearning::new("prefer sd", "use sd", "sed is terse"),
        );
        record(
            &mut store,
            &NewLearning::new("prefer rg", "use ripgrep", "grep is slow"),
        );

        let search = |query: &str| {
            store
                .list(&ListFilter {
                    search: Some(query.to_string()),
                    ..ListFilter::default()
                })
                .unwrap()
                .len()
        };
        assert_eq!(search("sd"), 1);
        assert_eq!(search("terse"), 1);
        assert_eq!(search("prefer"), 2);
        assert_eq!(search("prefer sd"), 1, "every term must match");
    }

    #[test]
    fn a_search_holding_fts5_operators_finds_the_words() {
        let mut store = Store::open_in_memory().unwrap();
        record(
            &mut store,
            &NewLearning::new("non empty", "keep OR and NEAR", "star and quote"),
        );
        let search = |query: &str| {
            store
                .list(&ListFilter {
                    search: Some(query.to_string()),
                    ..ListFilter::default()
                })
                .unwrap()
                .len()
        };
        assert_eq!(search("non-empty"), 1, "the hyphen must not negate");
        assert_eq!(search("\"empty\""), 1, "the quote must not open a phrase");
        assert_eq!(search("empt*"), 0, "the star is not a prefix operator");
        assert_eq!(search("OR"), 1, "OR is a word here");
        assert_eq!(search("NEAR"), 1, "NEAR is a word here");
        assert_eq!(search("***"), 0, "no term means no match, not an error");
    }

    #[test]
    fn stale_days_counts_from_the_last_use_and_includes_the_boundary() {
        let mut store = Store::open_in_memory().unwrap();
        let id = record(&mut store, &NewLearning::new("t", "r", "why"));
        store
            .conn
            .execute(
                "UPDATE learnings SET last_used_at = datetime('now', '-90 days')
                 WHERE id = ?1",
                [&id],
            )
            .unwrap();

        let stale = |days: u32| {
            store
                .list(&ListFilter {
                    stale_days: Some(days),
                    ..ListFilter::default()
                })
                .unwrap()
                .len()
        };
        assert_eq!(stale(91), 0, "90 days of silence is not 91 days");
        assert_eq!(stale(90), 1, "exactly N days counts as stale");
        assert_eq!(stale(89), 1);
    }

    #[test]
    fn a_learning_never_used_is_never_stale() {
        // Two Health buckets, not one. A rule that has never fired and a
        // rule that fired last year need different actions. Section 9.3.
        let mut store = Store::open_in_memory().unwrap();
        let mut old = NewLearning::new("old and unused", "r", "why");
        old.created_at = Some(BACKDATED.to_string());
        old.updated_at = Some(BACKDATED.to_string());
        record(&mut store, &old);

        let stale = store
            .list(&ListFilter {
                stale_days: Some(90),
                ..ListFilter::default()
            })
            .unwrap();
        assert!(stale.is_empty(), "age is not use");

        let unused = store
            .list(&ListFilter {
                never_used: true,
                ..ListFilter::default()
            })
            .unwrap();
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].title, "old and unused");
    }

    #[test]
    fn a_used_learning_is_not_in_the_never_used_bucket() {
        let mut store = Store::open_in_memory().unwrap();
        let used = record(&mut store, &NewLearning::new("used", "r", "why"));
        record(&mut store, &NewLearning::new("unused", "r", "why"));
        store
            .conn
            .execute(
                "UPDATE learnings SET last_used_at = datetime('now') WHERE id = ?1",
                [&used],
            )
            .unwrap();

        let unused = store
            .list(&ListFilter {
                never_used: true,
                ..ListFilter::default()
            })
            .unwrap();
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].title, "unused");
    }

    #[test]
    fn the_two_health_buckets_cannot_be_asked_for_together() {
        let store = Store::open_in_memory().unwrap();
        let error = store
            .list(&ListFilter {
                never_used: true,
                stale_days: Some(90),
                ..ListFilter::default()
            })
            .unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
    }

    #[test]
    fn filters_combine() {
        let mut store = Store::open_in_memory().unwrap();
        let mut one = NewLearning::new("prefer sd", "use sd", "why");
        one.status = Some(Status::Active);
        one.scopes = vec!["language:rust".parse().unwrap()];
        record(&mut store, &one);
        let mut two = NewLearning::new("prefer sd", "use sd", "why");
        two.scopes = vec!["language:rust".parse().unwrap()];
        record(&mut store, &two);

        let listed = store
            .list(&ListFilter {
                status: Some(Status::Active),
                scope: Some("language:rust".parse().unwrap()),
                search: Some("sd".into()),
                stale_days: None,
                never_used: false,
            })
            .unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn a_learning_defaults_to_proposed() {
        // Invariant 2: a caller that forgets `--activate` fails safe.
        let store = Store::open_in_memory().unwrap();
        let id = new_id();
        insert_learning(&store, &id, "t", "r");
        let status: String = store
            .conn
            .query_row("SELECT status FROM learnings WHERE id = ?1", [&id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "proposed");
    }
}
