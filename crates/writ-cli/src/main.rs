//! The `writ` binary. This crate is the only one that does I/O.

mod audit;
mod context;
mod git;
mod inspect;
mod list;
mod matcher;
mod output;
mod record;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use writ_core::{Env, Error, Paths, resolve_paths};

/// A local-first ledger of the steering you give coding agents.
#[derive(Debug, Parser)]
#[command(name = "writ", version, about, long_about = None)]
struct Cli {
    /// Path to the database. Overrides $XDG_DATA_HOME/writ/learnings.db
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Path to config.toml. Overrides $XDG_CONFIG_HOME/writ/config.toml
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Write a learning. The only way into the database
    Record(record::Args),
    /// Read learnings back
    List(list::Args),
    /// Check a diff against the learnings that apply to it
    Audit(audit::Args),
    /// Read one learning in full
    Show(inspect::ShowArgs),
    /// Prune a learning. It stops being selected and stays in the database
    Archive(inspect::ArchiveArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Every subcommand must take its paths from this one call, so `--db`
    // and `--config` cannot be honored by one command and ignored by the
    // next. See spec section 4.1 and crit #763.
    let paths: Paths =
        match resolve_paths(&Env::from_os(), cli.db.as_deref(), cli.config.as_deref()) {
            Ok(paths) => paths,
            // No database path is resolved yet, so there is none to name.
            Err(error) => return fail(&error, None),
        };

    let Some(command) = cli.command else {
        eprintln!("writ: no command given. Run `writ --help`.");
        return ExitCode::from(2);
    };

    let result = match command {
        Command::Record(args) => match context::load_config(&paths.config) {
            Ok(config) => record::run(&args, &paths.db, &config).map(|()| ExitCode::SUCCESS),
            Err(error) => Err(error),
        },
        Command::List(args) => list::run(&args, &paths.db).map(|()| ExitCode::SUCCESS),
        Command::Audit(args) => match context::load_config(&paths.config) {
            Ok(config) => audit::run(&args, &paths.db, &config),
            Err(error) => Err(error),
        },
        Command::Show(args) => inspect::show(&args, &paths.db).map(|()| ExitCode::SUCCESS),
        Command::Archive(args) => inspect::archive(&args, &paths.db).map(|()| ExitCode::SUCCESS),
    };

    match result {
        Ok(code) => code,
        Err(error) => fail(&error, Some(&paths.db)),
    }
}

/// Report the error and pick its exit code.
///
/// A storage failure names the database, because "database error: unable to
/// open database file" without a path leaves the user guessing which file.
/// SQLite already puts the path in some of its messages and not others, so
/// the path is appended only when it is missing. Printing it twice reads as
/// two different failures, which is exactly what P7 forbids.
fn fail(error: &Error, db: Option<&Path>) -> ExitCode {
    let message = error.to_string();
    match (error, db) {
        (Error::Sqlite(_), Some(db)) if !message.contains(&*db.to_string_lossy()) => {
            eprintln!("writ: {message}: {}", db.display());
        }
        _ => eprintln!("writ: {message}"),
    }
    ExitCode::from(exit_code(error))
}

/// Spec section 5.7. One table for every command, so a cause keeps one code
/// wherever it happens.
///
/// `CreateDirectory` shares code `8` with SQLite. The only directory writ
/// creates is the database's parent, so the failure is the database being
/// unwritable. Reporting it as `2` would tell the user to check their
/// flags, which P7 forbids.
fn exit_code(error: &Error) -> u8 {
    match error {
        Error::BadJson { .. } => 4,
        Error::NotFound { .. } => 5,
        Error::NotAGitRepository { .. } => 6,
        Error::EmptyDiff { .. } => 7,
        Error::Sqlite(_) | Error::CreateDirectory { .. } => 8,
        Error::NoBaseDirectory { .. }
        | Error::Config { .. }
        | Error::Validation { .. }
        | Error::Command { .. } => 2,
    }
}
