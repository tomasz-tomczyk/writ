-- Spec section 6. The schema is copied from the spec, which is the source
-- of truth. Do not edit an applied migration: add a new one.

CREATE TABLE learnings (
  id             TEXT PRIMARY KEY,          -- UUIDv7
  created_at     TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at     TEXT NOT NULL DEFAULT (datetime('now')),
  status         TEXT NOT NULL DEFAULT 'proposed'
                   CHECK (status IN ('proposed','active','archived')),
  title          TEXT NOT NULL,
  rule           TEXT NOT NULL,
  rationale      TEXT NOT NULL,
  blocking       INTEGER NOT NULL DEFAULT 1,
  matcher_kind   TEXT CHECK (matcher_kind IN ('ast_grep','regex')),
  matcher        TEXT,
  source_kind    TEXT NOT NULL CHECK (source_kind IN
                   ('manual','session','import')),
  source_adapter TEXT,
  source_ref     TEXT,
  author         TEXT,
  activated_at   TEXT,
  reinforced     INTEGER NOT NULL DEFAULT 0,
  times_applied  INTEGER NOT NULL DEFAULT 0,
  last_used_at   TEXT,
  last_verified  TEXT
);

CREATE TABLE learning_scopes (
  learning_id TEXT NOT NULL REFERENCES learnings(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN
                ('global','project','language','glob')),
  value       TEXT NOT NULL DEFAULT '',
  updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (learning_id, kind, value)
);

CREATE TABLE exemplars (
  id          TEXT PRIMARY KEY,             -- UUIDv7
  learning_id TEXT NOT NULL REFERENCES learnings(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN ('good','bad')),
  language    TEXT,
  snippet     TEXT NOT NULL,
  note        TEXT,
  updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE audits (
  id          TEXT PRIMARY KEY,             -- UUIDv7
  started_at  TEXT NOT NULL DEFAULT (datetime('now')),
  repo        TEXT,
  diff_range  TEXT,
  considered  INTEGER NOT NULL DEFAULT 0,
  sent        INTEGER NOT NULL DEFAULT 0,
  findings    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE findings (
  id          TEXT PRIMARY KEY,             -- UUIDv7
  audit_id    TEXT NOT NULL REFERENCES audits(id) ON DELETE CASCADE,
  learning_id TEXT NOT NULL REFERENCES learnings(id) ON DELETE CASCADE,
  path        TEXT,
  line        INTEGER,
  detail      TEXT,
  outcome     TEXT NOT NULL DEFAULT 'open'
                CHECK (outcome IN ('open','fixed','ignored','rejected'))
);

CREATE VIRTUAL TABLE learnings_fts USING fts5(
  title, rule, rationale, content='learnings', content_rowid='rowid'
);

-- An external content FTS5 table does not follow its content table. These
-- three triggers are the whole synchronization mechanism. Spec section 6.

CREATE TRIGGER learnings_fts_ai AFTER INSERT ON learnings BEGIN
  INSERT INTO learnings_fts (rowid, title, rule, rationale)
  VALUES (new.rowid, new.title, new.rule, new.rationale);
END;

CREATE TRIGGER learnings_fts_ad AFTER DELETE ON learnings BEGIN
  INSERT INTO learnings_fts (learnings_fts, rowid, title, rule, rationale)
  VALUES ('delete', old.rowid, old.title, old.rule, old.rationale);
END;

-- The WHEN clause matters. The `updated_at` trigger below writes back to
-- `learnings`, and SQLite does not define the order in which two AFTER
-- UPDATE triggers on one table run. Without the guard, that write-back
-- could delete a term set the index does not hold yet and corrupt it.
CREATE TRIGGER learnings_fts_au AFTER UPDATE ON learnings
WHEN old.title IS NOT new.title
  OR old.rule IS NOT new.rule
  OR old.rationale IS NOT new.rationale
BEGIN
  INSERT INTO learnings_fts (learnings_fts, rowid, title, rule, rationale)
  VALUES ('delete', old.rowid, old.title, old.rule, old.rationale);
  INSERT INTO learnings_fts (rowid, title, rule, rationale)
  VALUES (new.rowid, new.title, new.rule, new.rationale);
END;

-- `updated_at` triggers. `DEFAULT (datetime('now'))` fires on INSERT only,
-- so without these the column freezes at creation. Spec section 6.
--
-- Each trigger writes back to its own table, which is the only way to
-- change a row from an AFTER UPDATE trigger. Two things stop it
-- recursing: `Store` sets `PRAGMA recursive_triggers = OFF`, and the WHEN
-- clause fires only when the caller left `updated_at` alone. The guard
-- also lets an import keep its own timestamps.

CREATE TRIGGER learnings_set_updated_at AFTER UPDATE ON learnings
WHEN new.updated_at = old.updated_at
BEGIN
  UPDATE learnings SET updated_at = datetime('now') WHERE id = new.id;
END;

CREATE TRIGGER learning_scopes_set_updated_at AFTER UPDATE ON learning_scopes
WHEN new.updated_at = old.updated_at
BEGIN
  UPDATE learning_scopes SET updated_at = datetime('now')
  WHERE learning_id = new.learning_id
    AND kind = new.kind
    AND value = new.value;
END;

CREATE TRIGGER exemplars_set_updated_at AFTER UPDATE ON exemplars
WHEN new.updated_at = old.updated_at
BEGIN
  UPDATE exemplars SET updated_at = datetime('now') WHERE id = new.id;
END;
