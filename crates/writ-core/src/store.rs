use std::path::Path;

use rusqlite::{Connection, Row, Transaction, params};

use crate::audit::{
    AuditScope, Budget, Candidate, FindingsInput, Ingested, Outcome, Outcomes, Selected, rule_block,
};
use crate::error::{Error, Result};
use crate::fts;
use crate::id::new_id;
use crate::migrate;
use crate::model::{
    Exemplar, ExemplarKind, Finding, Learning, LearningUpdate, ListFilter, MatcherKind,
    NewExemplar, NewLearning, Recorded, Scope, ScopeKind, SourceKind, Status,
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
    pub fn record(&mut self, learning: &NewLearning) -> Result<Recorded> {
        Ok(self.record_many(std::slice::from_ref(learning))?.remove(0))
    }

    /// Write many learnings in one transaction.
    ///
    /// One transaction, so a stream either lands whole or not at all and a
    /// re-run cannot duplicate half of it. See `jsonl`.
    pub fn record_many(&mut self, learnings: &[NewLearning]) -> Result<Vec<Recorded>> {
        for learning in learnings {
            learning.validate()?;
        }
        let tx = self.conn.transaction()?;
        let mut written = Vec::with_capacity(learnings.len());
        for learning in learnings {
            written.push(insert_learning(&tx, learning)?);
        }
        tx.commit()?;
        Ok(written)
    }

    /// Replace the fields owned by the Detail editor.
    ///
    /// Status, provenance, counters and activation history are not part of
    /// [`LearningUpdate`], so this write cannot change them. `updated_at` is
    /// deliberately omitted from the UPDATE and left to the trigger.
    pub fn update_learning(&mut self, id: &str, update: &LearningUpdate) -> Result<()> {
        update.validate()?;
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
            "UPDATE learnings
                SET title = ?1,
                    rule = ?2,
                    rationale = ?3,
                    blocking = ?4,
                    matcher_kind = ?5,
                    matcher = ?6
              WHERE id = ?7",
            params![
                &update.title,
                &update.rule,
                &update.rationale,
                update.blocking,
                update.matcher_kind.map(MatcherKind::as_str),
                &update.matcher,
                id,
            ],
        )?;

        tx.execute("DELETE FROM learning_scopes WHERE learning_id = ?1", [id])?;
        for scope in update.effective_scopes() {
            tx.execute(
                "INSERT INTO learning_scopes (learning_id, kind, value)
                 VALUES (?1, ?2, ?3)",
                params![id, scope.kind.as_str(), &scope.value],
            )?;
        }

        tx.execute("DELETE FROM exemplars WHERE learning_id = ?1", [id])?;
        for exemplar in &update.exemplars {
            insert_exemplar(&tx, id, exemplar, None)?;
        }

        tx.commit()?;
        Ok(())
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

    /// Read learnings back, filtered. This is `writ list`.
    pub fn list(&self, filter: &ListFilter) -> Result<Vec<Learning>> {
        self.list_where(filter, None)
    }

    fn list_where(&self, filter: &ListFilter, id: Option<&str>) -> Result<Vec<Learning>> {
        let mut sql = String::from(
            "SELECT id, created_at, updated_at, status, title, rule, rationale, blocking,
                    matcher_kind, matcher, source_kind, source_adapter, source_ref,
                    author, activated_at, reinforced, times_selected, last_selected_at,
                    times_applied, last_applied_at, last_verified
             FROM learnings WHERE 1 = 1",
        );
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(id) = id {
            sql.push_str(" AND id = ?");
            values.push(Box::new(id.to_string()));
        }
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
        // Two filters, one per axis. Reach is whether audits put the rule
        // in front of a reviewer. Usefulness is whether it ever caught
        // anything. They combine freely, and a rule in both is the
        // clearest delete candidate in the collection.
        //
        // Both imply `status = 'active'`. Only an active learning can be
        // selected, so a proposed one would otherwise sit in both forever
        // and the Inbox would leak into Health. An explicit status wins,
        // because asking for it is asking for it.
        if (filter.never_applied || filter.unused_days.is_some()) && filter.status.is_none() {
            sql.push_str(" AND status = 'active'");
        }
        if filter.never_applied {
            // `times_selected > 0` keeps a rule no audit ever reached out
            // of this bucket. It has caught nothing trivially, and the fix
            // it needs is the other filter's, not this one's.
            sql.push_str(" AND times_selected > 0 AND times_applied = 0");
        }
        if let Some(days) = filter.unused_days {
            // The boundary is inclusive: exactly N days of silence counts.
            // A rule no audit has reached yet falls back to when it was
            // written, so one recorded this morning is not unused, and
            // `times_selected` on the row says which case a match is.
            sql.push_str(" AND COALESCE(last_selected_at, created_at) <= datetime('now', ?)");
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

    /// One learning by id, or [`Error::NotFound`]. This is `writ show`.
    pub fn get(&self, id: &str) -> Result<Learning> {
        let filter = ListFilter::default();
        let mut found = self.list_where(&filter, Some(id))?;
        if found.is_empty() {
            return Err(Error::NotFound { id: id.to_string() });
        }
        Ok(found.remove(0))
    }

    /// Active learnings this diff could be about. Spec section 7.1 step 2.
    ///
    /// The `global`, `project:` and `language:` tests are SQL. `glob:` is
    /// matched in Rust afterwards, because SQLite has no glob dialect with
    /// `**` in it. A learning is kept when any one of its scopes matches.
    ///
    /// A matcher is not evaluated here. Running `ast-grep` is I/O, so the
    /// caller does that and drops what missed. Invariant 1.
    pub fn candidates(&self, scope: &AuditScope) -> Result<Vec<Candidate>> {
        // Every kind the learning carries must match, and the rows inside
        // one kind are alternatives. This SQL can only over-select: it
        // drops a learning whose `project` or `language` kind cannot
        // match, and lets `glob` through because SQLite has no dialect
        // with `**` in it. [`scope_hits`] is the authoritative test.
        let mut sql = String::from(
            "SELECT id, created_at, updated_at, status, title, rule, rationale, blocking,
                    matcher_kind, matcher, source_kind, source_adapter, source_ref,
                    author, activated_at, reinforced, times_selected, last_selected_at,
                    times_applied, last_applied_at, last_verified
             FROM learnings
             WHERE status = 'active'
               AND EXISTS (SELECT 1 FROM learning_scopes s
                            WHERE s.learning_id = learnings.id)
               AND NOT EXISTS (
                     SELECT 1 FROM learning_scopes s
                      WHERE s.learning_id = learnings.id
                      GROUP BY s.kind
                     HAVING MAX(CASE
                              WHEN s.kind IN ('global', 'glob') THEN 1
                              WHEN s.kind = 'project' AND s.value = ? THEN 1",
        );
        let mut values: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(scope.identity.value().to_string())];

        let languages = scope.diff.languages();
        if !languages.is_empty() {
            sql.push_str(" WHEN s.kind = 'language' AND s.value IN (");
            for (index, language) in languages.iter().enumerate() {
                if index > 0 {
                    sql.push_str(", ");
                }
                sql.push('?');
                values.push(Box::new(language.clone()));
            }
            sql.push_str(") THEN 1");
        }
        sql.push_str(" ELSE 0 END) = 0) ORDER BY id");

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|value| value.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), read_learning)?;

        let mut candidates = Vec::new();
        for row in rows {
            let mut learning = row?;
            learning.scopes = self.scopes_of(&learning.id)?;
            if !scope_hits(&learning, scope, &languages) {
                continue;
            }
            let outcomes = self.outcomes_of(&learning.id)?;
            candidates.push(Candidate { learning, outcomes });
        }
        Ok(candidates)
    }

    /// Take rules off the ranked list until a cap stops it. Section 7.1
    /// step 3, and the P3 contract section 11 turns into a benchmark.
    ///
    /// Exemplars are loaded one learning at a time, so a collection of a
    /// thousand costs the same reads as a collection of fifty. A rule that
    /// does not fit in the remaining characters stops the loop rather than
    /// being skipped over: the ranking says this rule matters more than
    /// every rule behind it, so filling the gap with a lesser one would
    /// spend the budget against its own order.
    pub fn take_budget(&self, ranked: &[Candidate], budget: &Budget) -> Result<Vec<Selected>> {
        let mut selected: Vec<Selected> = Vec::new();
        let mut chars = 0usize;
        for candidate in ranked {
            if selected.len() as u32 >= budget.max_rules {
                break;
            }
            let one = Selected {
                learning: candidate.learning.clone(),
                exemplars: self.exemplars_of(&candidate.learning.id)?,
            };
            let cost = rule_block(selected.len() + 1, &one).chars().count();
            if chars + cost > budget.max_chars as usize {
                break;
            }
            chars += cost;
            selected.push(one);
        }
        Ok(selected)
    }

    /// Open an `audits` row, stamp the rules it sent, and return its id.
    /// Spec section 7.1 step 4.
    ///
    /// The row is written when the prompt is emitted, not when findings
    /// come back, because `considered` and `sent` are only known here,
    /// `findings.audit_id` is `NOT NULL`, and `started_at` is what the
    /// recurrence query in section 7.5 compares against `activated_at`.
    ///
    /// `times_selected` and `last_selected_at` move here and nowhere else.
    /// They measure reach: this rule was put in front of a reviewer.
    /// Whether it caught anything is the other pair, moved by
    /// [`Store::ingest`]. `updated_at` is never named. Invariant 7.
    pub fn start_audit(
        &mut self,
        repo: &str,
        diff_range: &str,
        considered: usize,
        selected: &[Selected],
    ) -> Result<String> {
        let id = new_id();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO audits (id, repo, diff_range, considered, sent)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &id,
                repo,
                diff_range,
                considered as i64,
                selected.len() as i64
            ],
        )?;
        for one in selected {
            tx.execute(
                "UPDATE learnings
                    SET times_selected = times_selected + 1,
                        last_selected_at = datetime('now')
                  WHERE id = ?1",
                [&one.learning.id],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Write the findings a host reported. Spec section 7.1 step 5.
    ///
    /// `times_applied` and `last_applied_at` move for the learnings the
    /// findings name. They measure usefulness: this rule caught something.
    /// `updated_at` is never named: the trigger owns it. Invariant 7.
    ///
    /// An `outcome` of `rejected` is refused. Section 7.5 says the
    /// developer sets it and section 7.1 step 5 says why the audited agent
    /// must not: one call would escape the gate and demote the rule that
    /// caught it, in the same write.
    pub fn ingest(&mut self, input: &FindingsInput) -> Result<Ingested> {
        let tx = self.conn.transaction()?;
        let known: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM audits WHERE id = ?1)",
            [&input.audit_id],
            |row| row.get(0),
        )?;
        if !known {
            return Err(Error::NotFound {
                id: input.audit_id.clone(),
            });
        }

        let mut blocking = 0usize;
        let mut unfixed_blocking = 0usize;
        for finding in &input.findings {
            if finding.outcome == Outcome::Rejected {
                return Err(Error::validation(format!(
                    "--ingest cannot set outcome rejected on {}. \
                     Only a developer rejects a finding",
                    finding.learning_id
                )));
            }
            let is_blocking: Option<bool> = tx
                .query_row(
                    "SELECT blocking FROM learnings WHERE id = ?1",
                    [&finding.learning_id],
                    |row| row.get(0),
                )
                .optional_row()?;
            let Some(is_blocking) = is_blocking else {
                return Err(Error::NotFound {
                    id: finding.learning_id.clone(),
                });
            };
            if is_blocking {
                blocking += 1;
                if finding.outcome != Outcome::Fixed {
                    unfixed_blocking += 1;
                }
            }
            tx.execute(
                "INSERT INTO findings (id, audit_id, learning_id, path, line, detail, outcome)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    new_id(),
                    &input.audit_id,
                    &finding.learning_id,
                    &finding.path,
                    &finding.line,
                    &finding.detail,
                    finding.outcome.as_str(),
                ],
            )?;
            tx.execute(
                "UPDATE learnings
                    SET times_applied = times_applied + 1,
                        last_applied_at = datetime('now')
                  WHERE id = ?1",
                [&finding.learning_id],
            )?;
        }
        tx.execute(
            "UPDATE audits SET findings = ?1 WHERE id = ?2",
            params![input.findings.len() as i64, &input.audit_id],
        )?;
        tx.commit()?;

        Ok(Ingested {
            audit_id: input.audit_id.clone(),
            findings: input.findings.len(),
            blocking,
            unfixed_blocking,
        })
    }

    /// One finding by id, or [`Error::NotFound`].
    pub fn finding(&self, id: &str) -> Result<Finding> {
        self.conn
            .query_row(
                "SELECT id, audit_id, learning_id, path, line, detail, outcome
                   FROM findings
                  WHERE id = ?1",
                [id],
                |row| {
                    let outcome: String = row.get(6)?;
                    Ok(Finding {
                        id: row.get(0)?,
                        audit_id: row.get(1)?,
                        learning_id: row.get(2)?,
                        path: row.get(3)?,
                        line: row.get(4)?,
                        detail: row.get(5)?,
                        outcome: parse_outcome(&outcome).map_err(to_sqlite_error)?,
                    })
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Error::NotFound { id: id.into() },
                _ => error.into(),
            })
    }

    /// Read the findings for one learning, oldest first.
    pub fn findings_of(&self, learning_id: &str) -> Result<Vec<Finding>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, audit_id, learning_id, path, line, detail, outcome
               FROM findings
              WHERE learning_id = ?1
              ORDER BY id",
        )?;
        let rows = stmt.query_map([learning_id], |row| {
            let outcome: String = row.get(6)?;
            Ok(Finding {
                id: row.get(0)?,
                audit_id: row.get(1)?,
                learning_id: row.get(2)?,
                path: row.get(3)?,
                line: row.get(4)?,
                detail: row.get(5)?,
                outcome: parse_outcome(&outcome).map_err(to_sqlite_error)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Record that the developer rejected a finding.
    ///
    /// This is the only core path that writes `rejected`; audited agents
    /// cannot set it through [`Store::ingest`]. Repeating the rejection is
    /// harmless.
    pub fn reject_finding(&mut self, finding_id: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE findings SET outcome = 'rejected' WHERE id = ?1",
            [finding_id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound {
                id: finding_id.to_string(),
            });
        }
        Ok(())
    }

    /// How past findings settled for one learning. Spec section 7.5.
    fn outcomes_of(&self, id: &str) -> Result<Outcomes> {
        let mut outcomes = Outcomes::default();
        let mut stmt = self.conn.prepare(
            "SELECT outcome, COUNT(*) FROM findings
              WHERE learning_id = ?1 GROUP BY outcome",
        )?;
        let rows = stmt.query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (outcome, count) = row?;
            match outcome.as_str() {
                "fixed" => outcomes.fixed = count,
                "ignored" => outcomes.ignored = count,
                "rejected" => outcomes.rejected = count,
                // `open` is not a judgement yet, so it weighs nothing.
                _ => {}
            }
        }
        Ok(outcomes)
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

/// Whether this learning's scopes match this diff. Spec section 7.1 step 2.
///
/// **Every kind the learning carries must match. Within one kind the rows
/// are alternatives.** `language:elixir` plus `project:X` means Elixir
/// files in X, not either. Pure OR across kinds would make a second scope
/// *widen* a rule, so the only way to write a narrow one would be to give
/// it a single scope — and then a `project:` rule fires on markdown edits
/// in that repository.
///
/// `global` short-circuits. A write cannot combine it with another kind,
/// so there is nothing to intersect it with.
fn scope_hits(learning: &Learning, scope: &AuditScope, languages: &[String]) -> bool {
    if learning.scopes.is_empty() {
        return false;
    }
    if learning
        .scopes
        .iter()
        .any(|one| one.kind == ScopeKind::Global)
    {
        return true;
    }
    for kind in [ScopeKind::Project, ScopeKind::Language, ScopeKind::Glob] {
        let mut rows = learning
            .scopes
            .iter()
            .filter(|one| one.kind == kind)
            .peekable();
        if rows.peek().is_none() {
            continue;
        }
        let matched = rows.any(|one| match kind {
            ScopeKind::Project => one.value == scope.identity.value(),
            ScopeKind::Language => languages.iter().any(|language| language == &one.value),
            ScopeKind::Glob => scope.diff.matches_glob(&one.value),
            ScopeKind::Global => true,
        });
        if !matched {
            return false;
        }
    }
    true
}

/// `QueryRow` returning `None` for no row instead of an error.
trait OptionalRow<T> {
    fn optional_row(self) -> Result<Option<T>>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional_row(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

fn insert_learning(tx: &Transaction<'_>, learning: &NewLearning) -> Result<Recorded> {
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
        times_selected: row.get(16)?,
        last_selected_at: row.get(17)?,
        times_applied: row.get(18)?,
        last_applied_at: row.get(19)?,
        last_verified: row.get(20)?,
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

fn parse_outcome(outcome: &str) -> Result<Outcome> {
    match outcome {
        "open" => Ok(Outcome::Open),
        "fixed" => Ok(Outcome::Fixed),
        "ignored" => Ok(Outcome::Ignored),
        "rejected" => Ok(Outcome::Rejected),
        other => Err(Error::validation(format!(
            "unknown finding outcome in database: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{FindingsInput, IncomingFinding, Outcome};
    use crate::diff::Diff;
    use crate::id::new_id;
    use crate::migrate::SCHEMA_VERSION;
    use crate::repo::RepoIdentity;

    const DIFF: &str = "diff --git a/api/lib/a.ex b/api/lib/a.ex\n\
        --- a/api/lib/a.ex\n\
        +++ b/api/lib/a.ex\n\
        @@ -1 +1 @@\n\
        +IO.inspect(x)\n";

    fn scope_for(diff: &str) -> AuditScope {
        AuditScope {
            identity: RepoIdentity::Remote("github.com/owner/repo".into()),
            diff: Diff::parse(diff),
            diff_range: "HEAD".into(),
        }
    }

    /// A global, active learning. The Health filters imply `active`, so a
    /// proposed row would drop out of every one of those tests.
    fn active_row(store: &mut Store, title: &str) -> String {
        let mut learning = NewLearning::new(title, "r", "why");
        learning.status = Some(Status::Active);
        record(store, &learning)
    }

    fn active(store: &mut Store, title: &str, scopes: &[&str]) -> String {
        let mut learning = NewLearning::new(title, "r", "why");
        learning.scopes = scopes.iter().map(|s| s.parse().unwrap()).collect();
        learning.status = Some(Status::Active);
        store.record(&learning).unwrap().id
    }

    fn selected_titles(store: &Store, scope: &AuditScope) -> Vec<String> {
        let mut candidates = store.candidates(scope).unwrap();
        crate::audit::rank(&mut candidates);
        candidates
            .into_iter()
            .map(|candidate| candidate.learning.title)
            .collect()
    }

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
        store.record(learning).unwrap().id
    }

    fn learning_update() -> crate::model::LearningUpdate {
        crate::model::LearningUpdate {
            title: "updated title".into(),
            rule: "updated rule".into(),
            rationale: "updated rationale".into(),
            blocking: false,
            matcher_kind: Some(MatcherKind::Regex),
            matcher: Some("updated.*".into()),
            scopes: vec!["language:rust".parse().unwrap()],
            exemplars: vec![NewExemplar {
                kind: ExemplarKind::Good,
                language: Some("rust".into()),
                snippet: "let updated = true;".into(),
                note: Some("new exemplar".into()),
            }],
        }
    }

    #[test]
    fn update_learning_replaces_fields_scopes_and_exemplars() {
        let mut store = Store::open_in_memory().unwrap();
        let mut original = NewLearning::new("old title", "old rule", "old rationale");
        original.status = Some(Status::Active);
        original.source_kind = Some(SourceKind::Session);
        original.source_adapter = Some("claude-code".into());
        original.source_ref = Some("session-1".into());
        original.author = Some("author@example.com".into());
        original.activated_at = Some(BACKDATED.into());
        original.scopes = vec!["project:github.com/example/repo".parse().unwrap()];
        original.exemplars = vec![NewExemplar {
            kind: ExemplarKind::Bad,
            language: None,
            snippet: "old exemplar".into(),
            note: None,
        }];
        let id = record(&mut store, &original);
        store
            .conn
            .execute(
                "UPDATE learnings
                    SET reinforced = 2,
                        times_selected = 3,
                        last_selected_at = ?1,
                        times_applied = 4,
                        last_applied_at = ?1,
                        last_verified = ?1,
                        updated_at = ?1
                  WHERE id = ?2",
                (BACKDATED, &id),
            )
            .unwrap();

        store.update_learning(&id, &learning_update()).unwrap();

        let updated = store.get(&id).unwrap();
        assert_eq!(updated.title, "updated title");
        assert_eq!(updated.rule, "updated rule");
        assert_eq!(updated.rationale, "updated rationale");
        assert!(!updated.blocking);
        assert_eq!(updated.matcher_kind, Some(MatcherKind::Regex));
        assert_eq!(updated.matcher.as_deref(), Some("updated.*"));
        assert_eq!(
            updated.scopes,
            vec!["language:rust".parse::<Scope>().unwrap()]
        );
        assert_eq!(updated.status, Status::Active);
        assert_eq!(updated.source_kind, SourceKind::Session);
        assert_eq!(updated.source_adapter.as_deref(), Some("claude-code"));
        assert_eq!(updated.source_ref.as_deref(), Some("session-1"));
        assert_eq!(updated.author.as_deref(), Some("author@example.com"));
        assert_eq!(updated.activated_at.as_deref(), Some(BACKDATED));
        assert_eq!(updated.reinforced, 2);
        assert_eq!(updated.times_selected, 3);
        assert_eq!(updated.last_selected_at.as_deref(), Some(BACKDATED));
        assert_eq!(updated.times_applied, 4);
        assert_eq!(updated.last_applied_at.as_deref(), Some(BACKDATED));
        assert_eq!(updated.last_verified.as_deref(), Some(BACKDATED));

        let exemplars = store.exemplars_of(&id).unwrap();
        assert_eq!(exemplars.len(), 1);
        assert_eq!(exemplars[0].kind, ExemplarKind::Good);
        assert_eq!(exemplars[0].language.as_deref(), Some("rust"));
        assert_eq!(exemplars[0].snippet, "let updated = true;");
        assert_eq!(exemplars[0].note.as_deref(), Some("new exemplar"));
    }

    #[test]
    fn update_learning_does_not_set_updated_at_in_sql() {
        let mut store = Store::open_in_memory().unwrap();
        let id = record(&mut store, &NewLearning::new("t", "r", "why"));
        store
            .conn
            .execute(
                "UPDATE learnings SET updated_at = ?1 WHERE id = ?2",
                (BACKDATED, &id),
            )
            .unwrap();

        store.update_learning(&id, &learning_update()).unwrap();

        let updated = store.get(&id).unwrap().updated_at;
        assert!(
            updated.as_str() > BACKDATED,
            "updated_at stayed at {updated}"
        );
    }

    #[test]
    fn update_learning_rejects_invalid_editable_fields() {
        let mut store = Store::open_in_memory().unwrap();
        let id = record(&mut store, &NewLearning::new("t", "r", "why"));

        for field in ["title", "rule", "rationale"] {
            let mut update = learning_update();
            match field {
                "title" => update.title = "   ".into(),
                "rule" => update.rule = "   ".into(),
                "rationale" => update.rationale = "   ".into(),
                _ => unreachable!(),
            }
            let error = store.update_learning(&id, &update).unwrap_err();
            assert!(matches!(error, Error::Validation { .. }), "{error}");
            assert!(error.to_string().contains(field), "{error}");
        }

        let mut update = learning_update();
        update.exemplars[0].snippet.clear();
        let error = store.update_learning(&id, &update).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");

        let mut update = learning_update();
        update.matcher_kind = None;
        let error = store.update_learning(&id, &update).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");

        let mut update = learning_update();
        update.matcher = None;
        let error = store.update_learning(&id, &update).unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
    }

    #[test]
    fn update_learning_unknown_id_is_not_found() {
        let mut store = Store::open_in_memory().unwrap();
        let error = store
            .update_learning("nope", &learning_update())
            .unwrap_err();
        assert!(matches!(error, Error::NotFound { .. }), "{error}");
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
        let error = store.record_many(&[good, bad]).unwrap_err();
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
    fn unused_days_counts_from_the_last_selection_and_includes_the_boundary() {
        let mut store = Store::open_in_memory().unwrap();
        let id = active_row(&mut store, "t");
        store
            .conn
            .execute(
                "UPDATE learnings
                    SET times_selected = 1,
                        last_selected_at = datetime('now', '-90 days')
                  WHERE id = ?1",
                [&id],
            )
            .unwrap();

        let unused = |days: u32| {
            store
                .list(&ListFilter {
                    unused_days: Some(days),
                    ..ListFilter::default()
                })
                .unwrap()
                .len()
        };
        assert_eq!(unused(91), 0, "90 days of silence is not 91 days");
        assert_eq!(unused(90), 1, "exactly N days counts");
        assert_eq!(unused(89), 1);
    }

    /// Section 5 gives `writ list` two filters, one per axis. Reach is
    /// whether audits put the rule in front of a reviewer. Usefulness is
    /// whether it ever caught anything.
    #[test]
    fn the_two_health_filters_measure_reach_and_usefulness() {
        let mut store = Store::open_in_memory().unwrap();
        let misscoped = active_row(&mut store, "never reached");
        let dead = active_row(&mut store, "reached, then stopped");
        let noisy = active_row(&mut store, "reached daily, catches nothing");
        let working = active_row(&mut store, "reached and useful");

        // Never selected, and written long ago.
        store
            .conn
            .execute(
                "UPDATE learnings SET created_at = '2000-01-01 00:00:00' WHERE id = ?1",
                [&misscoped],
            )
            .unwrap();
        // Selected a year ago and not since.
        store
            .conn
            .execute(
                "UPDATE learnings
                    SET times_selected = 5,
                        last_selected_at = datetime('now', '-365 days'),
                        times_applied = 2
                  WHERE id = ?1",
                [&dead],
            )
            .unwrap();
        // Selected today, has never caught anything.
        store
            .conn
            .execute(
                "UPDATE learnings SET times_selected = 9, last_selected_at = datetime('now')
                 WHERE id = ?1",
                [&noisy],
            )
            .unwrap();
        // Selected today and useful.
        store
            .conn
            .execute(
                "UPDATE learnings
                    SET times_selected = 9,
                        last_selected_at = datetime('now'),
                        times_applied = 4
                  WHERE id = ?1",
                [&working],
            )
            .unwrap();

        let titles = |filter: ListFilter| {
            let mut found: Vec<String> = store
                .list(&filter)
                .unwrap()
                .into_iter()
                .map(|learning| learning.title)
                .collect();
            found.sort();
            found
        };

        // Reach. Both the misscoped rule and the dead one are here, and
        // `times_selected` on the row is what tells them apart: 0 means
        // reword it, 5 means it stopped being reached.
        assert_eq!(
            titles(ListFilter {
                unused_days: Some(90),
                ..ListFilter::default()
            }),
            ["never reached", "reached, then stopped"]
        );
        // Usefulness. The misscoped rule is deliberately absent: it caught
        // nothing trivially, and "make it advisory" is the wrong advice.
        assert_eq!(
            titles(ListFilter {
                never_applied: true,
                ..ListFilter::default()
            }),
            ["reached daily, catches nothing"]
        );
        // They combine, and nothing here answers badly to both.
        assert!(
            titles(ListFilter {
                unused_days: Some(90),
                never_applied: true,
                ..ListFilter::default()
            })
            .is_empty()
        );
    }

    /// A rule written this morning that no audit has reached yet is not
    /// unused. Without the fallback to `created_at`, the Health screen
    /// would open on the rules the user just wrote.
    #[test]
    fn a_learning_no_audit_has_reached_falls_back_to_when_it_was_written() {
        let mut store = Store::open_in_memory().unwrap();
        active_row(&mut store, "written today");
        assert!(
            store
                .list(&ListFilter {
                    unused_days: Some(1),
                    ..ListFilter::default()
                })
                .unwrap()
                .is_empty()
        );
    }

    /// Both imply `status = 'active'`. A proposed learning can never be
    /// selected, so it would sit in both buckets forever.
    #[test]
    fn the_health_filters_imply_active_unless_a_status_is_given() {
        let mut store = Store::open_in_memory().unwrap();
        let id = record(&mut store, &NewLearning::new("proposed", "r", "why"));
        store
            .conn
            .execute(
                "UPDATE learnings
                    SET created_at = '2000-01-01 00:00:00', times_selected = 3
                  WHERE id = ?1",
                [&id],
            )
            .unwrap();

        for filter in [
            ListFilter {
                unused_days: Some(90),
                ..ListFilter::default()
            },
            ListFilter {
                never_applied: true,
                ..ListFilter::default()
            },
        ] {
            assert!(store.list(&filter).unwrap().is_empty(), "{filter:?}");
        }

        assert_eq!(
            store
                .list(&ListFilter {
                    status: Some(Status::Proposed),
                    unused_days: Some(90),
                    ..ListFilter::default()
                })
                .unwrap()
                .len(),
            1,
            "an explicit status wins"
        );
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
                ..ListFilter::default()
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

    // --- selection, section 7.1 step 2 ---------------------------------

    #[test]
    fn selection_takes_global_this_project_and_this_language() {
        let mut store = Store::open_in_memory().unwrap();
        active(&mut store, "everywhere", &["global"]);
        active(&mut store, "this repo", &["project:github.com/owner/repo"]);
        active(&mut store, "elixir", &["language:elixir"]);
        active(
            &mut store,
            "another repo",
            &["project:github.com/other/thing"],
        );
        active(&mut store, "rust", &["language:rust"]);

        let mut titles = selected_titles(&store, &scope_for(DIFF));
        titles.sort();
        assert_eq!(titles, ["elixir", "everywhere", "this repo"]);
    }

    /// A proposed or archived learning is never selected. Invariant 2, P4.
    #[test]
    fn selection_takes_active_learnings_only() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .record(&NewLearning::new("proposed", "r", "why"))
            .unwrap();
        let archived = active(&mut store, "archived", &["global"]);
        store.set_status(&archived, Status::Archived).unwrap();
        assert!(selected_titles(&store, &scope_for(DIFF)).is_empty());
    }

    /// The glob rows all pass the SQL, so the Rust pass has to drop the
    /// ones that missed. Without it a `web/**` rule fires on an `api/`
    /// diff and nothing says so.
    #[test]
    fn a_glob_scope_is_matched_against_the_changed_paths() {
        let mut store = Store::open_in_memory().unwrap();
        active(&mut store, "api", &["glob:api/**"]);
        active(&mut store, "web", &["glob:web/**"]);
        assert_eq!(selected_titles(&store, &scope_for(DIFF)), ["api"]);
    }

    /// Section 7.1 step 2. Every kind the learning carries must match, so
    /// a second scope narrows a rule rather than widening it. Under the
    /// old OR this rule fired on markdown edits in the right repository,
    /// which is the defect the seed exposed.
    #[test]
    fn every_scope_kind_a_learning_carries_must_match() {
        let mut store = Store::open_in_memory().unwrap();
        active(
            &mut store,
            "elixir in this repo",
            &["language:elixir", "project:github.com/owner/repo"],
        );

        assert_eq!(
            selected_titles(&store, &scope_for(DIFF)),
            ["elixir in this repo"],
            "an Elixir file in the right repo matches both kinds"
        );

        let elsewhere = AuditScope {
            identity: RepoIdentity::Remote("github.com/other/thing".into()),
            diff: Diff::parse(DIFF),
            diff_range: "HEAD".into(),
        };
        assert!(
            selected_titles(&store, &elsewhere).is_empty(),
            "the same Elixir file in another repo fails the project kind"
        );

        let markdown = "--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n+text\n";
        assert!(
            selected_titles(&store, &scope_for(markdown)).is_empty(),
            "a markdown edit in the right repo fails the language kind"
        );
    }

    /// Within one kind the rows are alternatives, so two globs mean either
    /// path rather than both at once.
    #[test]
    fn rows_of_one_kind_are_alternatives() {
        let mut store = Store::open_in_memory().unwrap();
        active(&mut store, "either tree", &["glob:api/**", "glob:web/**"]);
        assert_eq!(selected_titles(&store, &scope_for(DIFF)), ["either tree"]);

        let web = "--- a/web/src/app.ex\n+++ b/web/src/app.ex\n@@ -1 +1 @@\n+x\n";
        assert_eq!(selected_titles(&store, &scope_for(web)), ["either tree"]);

        let neither = "--- a/docs/a.ex\n+++ b/docs/a.ex\n@@ -1 +1 @@\n+x\n";
        assert!(selected_titles(&store, &scope_for(neither)).is_empty());
    }

    /// A glob and a language still AND, so the narrow rule an author meant
    /// is the rule they get.
    #[test]
    fn a_glob_and_a_language_narrow_each_other() {
        let mut store = Store::open_in_memory().unwrap();
        active(
            &mut store,
            "elixir under api",
            &["glob:api/**", "language:elixir"],
        );
        assert_eq!(
            selected_titles(&store, &scope_for(DIFF)),
            ["elixir under api"]
        );

        let wrong_tree = "--- a/web/a.ex\n+++ b/web/a.ex\n@@ -1 +1 @@\n+x\n";
        assert!(selected_titles(&store, &scope_for(wrong_tree)).is_empty());

        let wrong_language = "--- a/api/a.md\n+++ b/api/a.md\n@@ -1 +1 @@\n+x\n";
        assert!(selected_titles(&store, &scope_for(wrong_language)).is_empty());
    }

    #[test]
    fn a_global_scope_matches_every_diff() {
        let mut store = Store::open_in_memory().unwrap();
        active(&mut store, "everywhere", &["global"]);
        for diff in [
            DIFF,
            "--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n+text\n",
        ] {
            assert_eq!(selected_titles(&store, &scope_for(diff)), ["everywhere"]);
        }
    }

    #[test]
    fn a_diff_of_files_with_no_known_language_still_selects_global_rules() {
        let mut store = Store::open_in_memory().unwrap();
        active(&mut store, "everywhere", &["global"]);
        active(&mut store, "elixir", &["language:elixir"]);
        let diff = "--- a/Makefile\n+++ b/Makefile\n@@ -1 +1 @@\n+all:\n";
        assert_eq!(selected_titles(&store, &scope_for(diff)), ["everywhere"]);
    }

    // --- the budget, section 7.1 step 3 --------------------------------

    #[test]
    fn the_budget_stops_at_max_rules() {
        let mut store = Store::open_in_memory().unwrap();
        for index in 0..10 {
            active(&mut store, &format!("rule {index}"), &["global"]);
        }
        let scope = scope_for(DIFF);
        let mut candidates = store.candidates(&scope).unwrap();
        crate::audit::rank(&mut candidates);
        let budget = Budget {
            max_rules: 3,
            max_chars: 100_000,
        };
        assert_eq!(store.take_budget(&candidates, &budget).unwrap().len(), 3);
    }

    #[test]
    fn the_budget_stops_at_max_chars() {
        let mut store = Store::open_in_memory().unwrap();
        for index in 0..10 {
            active(&mut store, &format!("rule {index}"), &["global"]);
        }
        let scope = scope_for(DIFF);
        let mut candidates = store.candidates(&scope).unwrap();
        crate::audit::rank(&mut candidates);
        let one = store
            .take_budget(
                &candidates,
                &Budget {
                    max_rules: 1,
                    max_chars: 100_000,
                },
            )
            .unwrap();
        let width = rule_block(1, &one[0]).chars().count() as u32;

        let two = store
            .take_budget(
                &candidates,
                &Budget {
                    max_rules: 40,
                    max_chars: width * 2 + 1,
                },
            )
            .unwrap();
        assert_eq!(two.len(), 2, "two rules fit and a third does not");
        let none = store
            .take_budget(
                &candidates,
                &Budget {
                    max_rules: 40,
                    max_chars: 1,
                },
            )
            .unwrap();
        assert!(none.is_empty(), "a budget of one character sends nothing");
    }

    // --- ingest, section 7.1 step 5 ------------------------------------

    #[test]
    fn ingesting_writes_a_finding_and_moves_the_counters() {
        let mut store = Store::open_in_memory().unwrap();
        let id = active(&mut store, "rule", &["global"]);
        let audit = store
            .start_audit("github.com/owner/repo", "HEAD", 1, &[])
            .unwrap();
        let before = store.get(&id).unwrap();
        assert_eq!(before.times_applied, 0);
        assert_eq!(before.last_applied_at, None);

        let written = store
            .ingest(&FindingsInput {
                audit_id: audit.clone(),
                findings: vec![IncomingFinding {
                    learning_id: id.clone(),
                    path: Some("api/lib/a.ex".into()),
                    line: Some(1),
                    detail: Some("here".into()),
                    outcome: Outcome::Open,
                }],
            })
            .unwrap();
        assert_eq!(written.findings, 1);
        assert_eq!(written.blocking, 1);

        let after = store.get(&id).unwrap();
        assert_eq!(after.times_applied, 1);
        assert!(after.last_applied_at.is_some());
        // Ingest moves usefulness only. Reach moved at emit, and this
        // audit row was opened with no selection.
        assert_eq!(after.times_selected, 0);
        // Invariant 7: the trigger owns updated_at, and no write named it.
        assert_eq!(after.created_at, before.created_at);
    }

    #[test]
    fn an_advisory_finding_is_not_a_blocking_one() {
        let mut store = Store::open_in_memory().unwrap();
        let mut learning = NewLearning::new("advisory", "r", "why");
        learning.blocking = false;
        learning.status = Some(Status::Active);
        let id = store.record(&learning).unwrap().id;
        let audit = store.start_audit("repo", "HEAD", 1, &[]).unwrap();
        let written = store
            .ingest(&FindingsInput {
                audit_id: audit,
                findings: vec![IncomingFinding {
                    learning_id: id,
                    path: None,
                    line: None,
                    detail: None,
                    outcome: Outcome::Open,
                }],
            })
            .unwrap();
        assert_eq!(written.blocking, 0);
    }

    #[test]
    fn ingesting_against_an_unknown_audit_is_not_found() {
        let mut store = Store::open_in_memory().unwrap();
        let error = store
            .ingest(&FindingsInput {
                audit_id: "nope".into(),
                findings: Vec::new(),
            })
            .unwrap_err();
        assert!(matches!(error, Error::NotFound { .. }), "{error}");
    }

    /// Section 7.5 reads outcomes back as the ranking signal, so an
    /// outcome the agent may set has to survive the write.
    #[test]
    fn an_ingested_outcome_is_stored_and_ranks_the_learning_down() {
        let mut store = Store::open_in_memory().unwrap();
        let id = active(&mut store, "rule", &["global"]);
        let audit = store
            .start_audit("github.com/owner/repo", "HEAD", 1, &[])
            .unwrap();
        store
            .ingest(&FindingsInput {
                audit_id: audit,
                findings: vec![IncomingFinding {
                    learning_id: id.clone(),
                    path: None,
                    line: None,
                    detail: None,
                    outcome: Outcome::Ignored,
                }],
            })
            .unwrap();
        let candidates = store.candidates(&scope_for(DIFF)).unwrap();
        assert_eq!(candidates[0].outcomes.ignored, 1);
        assert!(candidates[0].outcomes.acceptance() < 1.0);
    }

    /// Section 7.1 step 5. The developer owns `rejected`, and it is the
    /// heaviest negative in the ranking. If the audited agent could write
    /// it, one call would escape the gate and demote the rule that caught
    /// it.
    #[test]
    fn ingest_refuses_to_set_rejected() {
        let mut store = Store::open_in_memory().unwrap();
        let id = active(&mut store, "rule", &["global"]);
        let audit = store.start_audit("repo", "HEAD", 1, &[]).unwrap();
        let error = store
            .ingest(&FindingsInput {
                audit_id: audit,
                findings: vec![IncomingFinding {
                    learning_id: id.clone(),
                    path: None,
                    line: None,
                    detail: None,
                    outcome: Outcome::Rejected,
                }],
            })
            .unwrap_err();
        assert!(matches!(error, Error::Validation { .. }), "{error}");
        // The whole call is one transaction, so nothing landed.
        assert_eq!(store.get(&id).unwrap().times_applied, 0);
    }

    #[test]
    fn findings_of_returns_rows_for_a_learning() {
        let mut store = Store::open_in_memory().unwrap();
        let id = active(&mut store, "t", &["global"]);
        let audit = store.start_audit("repo", "HEAD", 1, &[]).unwrap();
        store
            .ingest(&FindingsInput {
                audit_id: audit.clone(),
                findings: vec![IncomingFinding {
                    learning_id: id.clone(),
                    path: Some("a.rs".into()),
                    line: Some(3),
                    detail: Some("bad".into()),
                    outcome: Outcome::Open,
                }],
            })
            .unwrap();

        let rows = store.findings_of(&id).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].audit_id, audit);
        assert_eq!(rows[0].learning_id, id);
        assert_eq!(rows[0].path.as_deref(), Some("a.rs"));
        assert_eq!(rows[0].line, Some(3));
        assert_eq!(rows[0].detail.as_deref(), Some("bad"));
        assert_eq!(rows[0].outcome, Outcome::Open);
    }

    #[test]
    fn reject_finding_sets_outcome_rejected() {
        let mut store = Store::open_in_memory().unwrap();
        let id = active(&mut store, "t", &["global"]);
        let audit = store.start_audit("repo", "HEAD", 1, &[]).unwrap();
        store
            .ingest(&FindingsInput {
                audit_id: audit,
                findings: vec![IncomingFinding {
                    learning_id: id.clone(),
                    path: None,
                    line: None,
                    detail: None,
                    outcome: Outcome::Open,
                }],
            })
            .unwrap();
        let finding_id = store.findings_of(&id).unwrap()[0].id.clone();

        store.reject_finding(&finding_id).unwrap();
        store.reject_finding(&finding_id).unwrap();

        assert_eq!(
            store.findings_of(&id).unwrap()[0].outcome,
            Outcome::Rejected
        );
    }

    #[test]
    fn reject_finding_unknown_id_is_not_found() {
        let mut store = Store::open_in_memory().unwrap();
        let error = store.reject_finding("missing").unwrap_err();
        assert!(matches!(error, Error::NotFound { .. }), "{error}");
    }

    /// A rejection still has to rank, once a developer path writes one.
    #[test]
    fn a_rejected_finding_ranks_the_learning_to_the_bottom() {
        let mut store = Store::open_in_memory().unwrap();
        let id = active(&mut store, "rule", &["global"]);
        let audit = store
            .start_audit("github.com/owner/repo", "HEAD", 1, &[])
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO findings (id, audit_id, learning_id, outcome)
                 VALUES (?1, ?2, ?3, 'rejected')",
                params![new_id(), &audit, &id],
            )
            .unwrap();
        let candidates = store.candidates(&scope_for(DIFF)).unwrap();
        assert_eq!(candidates[0].outcomes.rejected, 1);
        assert_eq!(candidates[0].outcomes.acceptance(), 0.0);
    }

    /// Reach moves at emit and nowhere else. Section 6.
    #[test]
    fn emitting_stamps_the_rules_it_sent_and_no_others() {
        let mut store = Store::open_in_memory().unwrap();
        let sent = active(&mut store, "sent", &["global"]);
        let held = active(&mut store, "held back", &["global"]);
        let scope = scope_for(DIFF);
        let mut candidates = store.candidates(&scope).unwrap();
        crate::audit::rank(&mut candidates);
        let selected = store
            .take_budget(
                &candidates,
                &Budget {
                    max_rules: 1,
                    max_chars: 100_000,
                },
            )
            .unwrap();
        assert_eq!(selected.len(), 1);
        let chosen = selected[0].learning.id.clone();
        store
            .start_audit("github.com/owner/repo", "HEAD", 2, &selected)
            .unwrap();

        let other = if chosen == sent { held } else { sent };
        let stamped = store.get(&chosen).unwrap();
        assert_eq!(stamped.times_selected, 1);
        assert!(stamped.last_selected_at.is_some());
        // Selection is not application. The other pair must not move.
        assert_eq!(stamped.times_applied, 0);
        assert_eq!(stamped.last_applied_at, None);

        let untouched = store.get(&other).unwrap();
        assert_eq!(untouched.times_selected, 0);
        assert_eq!(untouched.last_selected_at, None);
    }

    #[test]
    fn getting_an_unknown_id_is_not_found() {
        let store = Store::open_in_memory().unwrap();
        let error = store.get(&new_id()).unwrap_err();
        assert!(matches!(error, Error::NotFound { .. }), "{error}");
    }
}
