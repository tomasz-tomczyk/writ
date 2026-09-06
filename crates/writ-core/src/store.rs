use std::path::Path;

use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::migrate;

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
