//! The `writ` binary. This crate is the only one that does I/O.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use writ_core::{Env, Paths, resolve_paths};

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
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Every subcommand must take its paths from this one call, so `--db`
    // and `--config` cannot be honored by one command and ignored by the
    // next. See spec section 4.1 and crit #763.
    let paths: Paths =
        match resolve_paths(&Env::from_os(), cli.db.as_deref(), cli.config.as_deref()) {
            Ok(paths) => paths,
            Err(error) => {
                eprintln!("writ: {error}");
                return ExitCode::from(2);
            }
        };

    // No subcommand exists yet, so nothing reads the paths. The resolution
    // still runs, because a bad path must fail here and not somewhere else.
    let _ = paths;

    eprintln!("writ: no command given. Run `writ --help`.");
    ExitCode::from(2)
}
