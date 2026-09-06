//! `writ telemetry`: explicit consent and complete local disclosure.

use std::io::Write;
use std::process::ExitCode;

use writ_core::{
    BucketMetric, Config, Error, Paths, Result, Store, TelemetryBatch, TelemetryStore,
};

use crate::context::set_telemetry_enabled;

/// Telemetry design section 5, verbatim. README carries the same block.
pub const PRIVACY_TEXT: &str = r#"**Captured:**

- A random install id, generated when telemetry is enabled.
- The writ version and the operating system family (`macos`, `linux`,
  `windows`).
- The date, to the day.
- The counters and buckets in section 4.

**Never captured:**

- Rule text, titles, rationale, exemplar snippets, notes.
- File paths, directory names, glob patterns, repository names, remote
  URLs, branch names.
- Scope *values*. Only the scope kind, and language from a fixed
  allowlist.
- Search queries, matcher patterns, finding details.
- The author field, any email, any username, any hostname.
- Learning ids, audit ids, finding ids.
- Timestamps finer than one day.
- Anything from the diff being audited.

If a future metric cannot be added without touching that second list, it
does not get added.
"#;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Opt in after displaying and confirming the privacy disclosure
    On(OnArgs),
    /// Stop collection while retaining the local aggregate store
    Off,
    /// Display enabled state and everything held as text
    Show,
    /// Write the complete versioned JSON payload to stdout
    Dump,
    /// Delete telemetry.db entirely
    Purge,
}

#[derive(Debug, clap::Args)]
struct OnArgs {
    /// Confirm without reading stdin
    #[arg(long)]
    yes: bool,
}

pub fn run(args: &Args, paths: &Paths, config: &Config) -> Result<ExitCode> {
    match args.command.as_ref().unwrap_or(&Command::Show) {
        Command::On(args) => on(args, paths),
        Command::Off => off(paths),
        Command::Show => show(paths, config),
        Command::Dump => dump(paths),
        Command::Purge => purge(paths),
    }
}

fn on(args: &OnArgs, paths: &Paths) -> Result<ExitCode> {
    // Validate before asking for consent. A malformed existing file cannot be
    // safely edited, and prompting for an operation that will fail is hostile.
    print!("{PRIVACY_TEXT}");
    if !args.yes && !confirmed()? {
        println!("telemetry remains disabled");
        return Ok(ExitCode::SUCCESS);
    }

    let mut store = TelemetryStore::open(&paths.telemetry_db)?;
    store.enable()?;
    set_telemetry_enabled(&paths.config, true)?;
    println!("telemetry enabled");
    Ok(ExitCode::SUCCESS)
}

fn confirmed() -> Result<bool> {
    print!("Enable local telemetry? [y/N] ");
    std::io::stdout().flush().map_err(|error| Error::Command {
        program: "stdout".to_string(),
        message: error.to_string(),
    })?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|error| Error::Command {
            program: "stdin".to_string(),
            message: error.to_string(),
        })?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn off(paths: &Paths) -> Result<ExitCode> {
    set_telemetry_enabled(&paths.config, false)?;
    println!("telemetry disabled; existing data retained");
    Ok(ExitCode::SUCCESS)
}

fn show(paths: &Paths, config: &Config) -> Result<ExitCode> {
    println!(
        "telemetry: {}",
        if config.telemetry.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("\n{PRIVACY_TEXT}");
    if !paths.telemetry_db.exists() {
        println!("held data: none");
        return Ok(ExitCode::SUCCESS);
    }

    let store = TelemetryStore::open(&paths.telemetry_db)?;
    println!("held metadata:");
    for row in store.meta()? {
        println!("{}  {}", row.key, row.value);
    }
    println!("held counters:");
    for row in store.counters()? {
        println!("{}  {}  {}  {}", row.day, row.metric, row.label, row.count);
    }
    println!("held buckets:");
    for row in store.buckets()? {
        println!("{}  {}  {}  {}", row.day, row.metric, row.bucket, row.count);
    }
    Ok(ExitCode::SUCCESS)
}

fn dump(paths: &Paths) -> Result<ExitCode> {
    if !paths.telemetry_db.is_file() {
        return Err(Error::Storage {
            message: format!(
                "no telemetry database exists at {}",
                paths.telemetry_db.display()
            ),
        });
    }
    let store = TelemetryStore::open(&paths.telemetry_db)?;
    let dump = store.dump(env!("CARGO_PKG_VERSION"), os_family())?;
    let json = serde_json::to_string_pretty(&dump).expect("TelemetryDump serializes");
    println!("{json}");
    Ok(ExitCode::SUCCESS)
}

fn purge(paths: &Paths) -> Result<ExitCode> {
    match std::fs::remove_file(&paths.telemetry_db) {
        Ok(()) => println!("telemetry data purged"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("telemetry data already absent");
        }
        Err(error) => {
            return Err(Error::Storage {
                message: format!("cannot remove {}: {error}", paths.telemetry_db.display()),
            });
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn os_family() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        _ => "other",
    }
}

/// Record an incidental observation without allowing telemetry to influence
/// the command that produced it. T5 applies to opening and writing the store.
pub fn record_best_effort(path: &std::path::Path, batch: &TelemetryBatch) {
    if !path.is_file() {
        return;
    }
    let _ = TelemetryStore::open(path).and_then(|mut store| {
        if store.is_enabled()? {
            store.record(batch)?;
        }
        Ok(())
    });
}

/// Add collection size when the learning store remains readable. This is also
/// best-effort: observing a command cannot introduce a new failure path.
pub fn add_collection_size_best_effort(db: &std::path::Path, batch: &mut TelemetryBatch) {
    if let Ok(store) = Store::open(db)
        && let Ok(size) = store.collection_size()
    {
        batch.buckets.push(BucketMetric::CollectionSize(size));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_carries_the_exact_runtime_privacy_disclosure() {
        assert!(include_str!("../../../README.md").contains(PRIVACY_TEXT));
    }
}
