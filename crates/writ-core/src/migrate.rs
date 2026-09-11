use rusqlite::Connection;

use crate::error::Result;

/// The schema version this build writes.
pub const SCHEMA_VERSION: i64 = 3;

/// One forward-only step. writ never rolls a migration back, so a step is
/// only ever added, never edited.
struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("migrations/0002_learning_sides.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("migrations/0003_audit_prompt.sql"),
    },
];

/// Bring the database up to [`SCHEMA_VERSION`].
///
/// The function is idempotent. It applies only the steps the database has
/// not recorded, and each step runs inside its own transaction, so a
/// failure leaves the recorded version and the schema in step.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS writ_migrations (
           version    INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )?;

    for migration in MIGRATIONS {
        let applied: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM writ_migrations WHERE version = ?1)",
            [migration.version],
            |row| row.get(0),
        )?;
        if applied {
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO writ_migrations (version) VALUES (?1)",
            [migration.version],
        )?;
        tx.commit()?;
    }

    Ok(())
}

/// Return the highest migration version the database records.
pub(crate) fn current_version(conn: &Connection) -> Result<i64> {
    let version: Option<i64> =
        conn.query_row("SELECT MAX(version) FROM writ_migrations", [], |row| {
            row.get(0)
        })?;
    Ok(version.unwrap_or(0))
}
