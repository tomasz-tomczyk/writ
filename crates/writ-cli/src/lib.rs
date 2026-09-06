//! The `writ` command, as a library.
//!
//! `writ-cli` is the only crate that does I/O: terminal, git, HTTP, and
//! MCP all live here. The binary in `src/main.rs` is a one-line shell over
//! [`run`], so an integration test can reach the same code the binary runs
//! without spawning a process.

pub mod audit;
pub mod context;
pub mod git;
pub mod hook;
pub mod inspect;
pub mod install;
pub mod list;
pub mod matcher;
pub mod mcp;
pub mod output;
pub mod record;
pub mod telemetry;
pub mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use writ_core::{
    BucketMetric, CommandMetric, CounterMetric, Env, Error, Paths, SurfaceMetric, TelemetryBatch,
    resolve_paths,
};

/// A local-first ledger of the steering you give coding agents.
#[derive(Debug, Parser)]
#[command(name = "writ", version, about, long_about = None)]
pub struct Cli {
    /// Path to the database. Overrides $XDG_DATA_HOME/writ/learnings.db
    #[arg(long, global = true, value_name = "PATH")]
    pub db: Option<PathBuf>,

    /// Path to config.toml. Overrides $XDG_CONFIG_HOME/writ/config.toml
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Every subcommand in spec section 5.
#[derive(Debug, Subcommand)]
pub enum Command {
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
    /// Open the local web interface
    Ui(ui::Args),
    /// Run the MCP server on stdio
    Mcp(mcp::Args),
    /// Write the MCP registration and the gate hook into a host's config
    Install(install::Args),
    /// Inspect or control opt-in local-only aggregate telemetry
    Telemetry(telemetry::Args),
}

impl Command {
    fn telemetry_metric(&self) -> Option<CommandMetric> {
        Some(match self {
            Self::Record(_) => CommandMetric::Record,
            Self::List(_) => CommandMetric::List,
            Self::Audit(_) => CommandMetric::Audit,
            Self::Show(_) => CommandMetric::Show,
            Self::Archive(_) => CommandMetric::Archive,
            Self::Ui(_) => CommandMetric::Ui,
            Self::Mcp(_) => CommandMetric::Mcp,
            // Installing is a one-off setup step, not a use of the
            // collection, and it never opens the learnings database.
            Self::Install(_) => return None,
            // Administration does not observe itself: dump/show must be a
            // stable disclosure, and purge must not recreate what it removed.
            Self::Telemetry(_) => return None,
        })
    }
}

/// Parse the command line and run it. The binary returns what this returns.
pub fn run() -> ExitCode {
    dispatch(Cli::parse())
}

/// Run an already-parsed command line.
pub fn dispatch(cli: Cli) -> ExitCode {
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

    // The configuration is resolved once, here, for the same reason the
    // paths are: a subcommand that parses it late, or not at all, runs
    // against a file the user never approved. `list`, `show` and `archive`
    // each ignored `--config` entirely until this moved out of the arms.
    // Spec section 10 and crit #763.
    let config = match context::load_config(&paths.config) {
        Ok(config) => config,
        Err(error) => return fail(&error, Some(&paths.db)),
    };

    let command_metric = command.telemetry_metric();
    let failure_db = if command_metric.is_some() {
        &paths.db
    } else {
        &paths.telemetry_db
    };
    let telemetry_enabled = command_metric.is_some() && config.telemetry.enabled;
    let started = Instant::now();

    let result = match command {
        Command::Record(args) => {
            record::run(&args, &paths.db, &config).map(|batch| (ExitCode::SUCCESS, batch))
        }
        Command::List(args) => {
            list::run(&args, &paths.db).map(|()| (ExitCode::SUCCESS, TelemetryBatch::default()))
        }
        Command::Audit(args) => audit::run(&args, &paths.db, &config),
        Command::Show(args) => {
            inspect::show(&args, &paths.db).map(|()| (ExitCode::SUCCESS, TelemetryBatch::default()))
        }
        Command::Archive(args) => inspect::archive(&args, &paths.db)
            .map(|()| (ExitCode::SUCCESS, TelemetryBatch::default())),
        Command::Ui(args) => {
            ui::run(&args, &paths, &config).map(|code| (code, TelemetryBatch::default()))
        }
        Command::Mcp(args) => {
            mcp::run(&args, &paths, &config).map(|code| (code, TelemetryBatch::default()))
        }
        Command::Install(args) => {
            // `--project` needs the repository, and the default scope
            // needs $HOME. Neither is fatal here: `install::plan` says
            // which one is missing for the scope the caller asked for.
            let roots = install::Roots {
                home: Env::from_os().home,
                project: std::env::current_dir()
                    .ok()
                    .and_then(|cwd| git::discover(&cwd).ok())
                    .map(|repo| repo.root),
            };
            install::run(&args, &roots).map(|()| (ExitCode::SUCCESS, TelemetryBatch::default()))
        }
        Command::Telemetry(args) => {
            telemetry::run(&args, &paths, &config).map(|code| (code, TelemetryBatch::default()))
        }
    };

    match result {
        Ok((code, mut batch)) => {
            if telemetry_enabled {
                observe_command(
                    &paths,
                    &mut batch,
                    command_metric.expect("enabled commands have a metric"),
                    process_exit_code(code),
                    started.elapsed().as_millis() as u64,
                );
            }
            code
        }
        Err(error) => {
            if telemetry_enabled {
                let mut batch = TelemetryBatch::default();
                observe_command(
                    &paths,
                    &mut batch,
                    command_metric.expect("enabled commands have a metric"),
                    exit_code(&error),
                    started.elapsed().as_millis() as u64,
                );
            }
            fail(&error, Some(failure_db))
        }
    }
}

fn observe_command(
    paths: &Paths,
    batch: &mut TelemetryBatch,
    command: CommandMetric,
    code: u8,
    elapsed_ms: u64,
) {
    batch.counters.extend([
        CounterMetric::Command(command),
        CounterMetric::Surface(SurfaceMetric::Cli),
        CounterMetric::ExitCode(code),
    ]);
    batch.buckets.push(BucketMetric::CommandMs(elapsed_ms));
    telemetry::add_collection_size_best_effort(&paths.db, batch);
    telemetry::record_best_effort(&paths.telemetry_db, batch);
}

fn process_exit_code(code: ExitCode) -> u8 {
    (0..=8)
        .find(|value| code == ExitCode::from(*value))
        .unwrap_or(8)
}

/// Report the error and pick its exit code.
///
/// A storage failure names the database, because "database error: unable to
/// open database file" without a path leaves the user guessing which file.
/// SQLite already puts the path in some of its messages and not others, so
/// the path is appended only when it is missing. Printing it twice reads as
/// two different failures, which is exactly what P7 forbids.
pub fn fail(error: &Error, db: Option<&Path>) -> ExitCode {
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
pub fn exit_code(error: &Error) -> u8 {
    match error {
        Error::BadJson { .. } => 4,
        Error::NotFound { .. } => 5,
        Error::NotAGitRepository { .. } => 6,
        Error::EmptyDiff { .. } => 7,
        Error::Sqlite(_) | Error::CreateDirectory { .. } | Error::Storage { .. } => 8,
        Error::NoBaseDirectory { .. }
        | Error::Config { .. }
        | Error::Validation { .. }
        | Error::Command { .. } => 2,
    }
}
