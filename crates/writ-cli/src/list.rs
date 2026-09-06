//! `writ list`. Spec section 5.
//!
//! `writ list --unused-days 90 --format json` and
//! `writ list --never-applied --format json` are the Health screen as two
//! queries, which keeps the UI free of logic the CLI lacks.

use std::io::Write;
use std::path::Path;

use writ_core::{Learning, ListFilter, Result, Status, Store};

use crate::output::Format;

/// Every flag in the `writ list` row of spec section 5.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// proposed, active or archived
    #[arg(long, value_name = "STATUS")]
    status: Option<String>,

    /// global, project:ID, language:LANG or glob:PAT
    #[arg(long, value_name = "KIND:VALUE")]
    scope: Option<String>,

    /// Not selected by an audit for this many days. Reach. The row's
    /// times_selected says whether it is misscoped or dead
    #[arg(long = "unused-days", value_name = "N")]
    unused_days: Option<u32>,

    /// Selected at least once and never caught anything. Usefulness
    #[arg(long = "never-applied")]
    never_applied: bool,

    /// Full-text search over title, rule and rationale
    #[arg(long, value_name = "QUERY")]
    search: Option<String>,

    /// text or json
    #[arg(long, default_value_t = Format::Text, value_name = "FORMAT")]
    format: Format,
}

/// Run the command.
pub fn run(args: &Args, db: &Path) -> Result<()> {
    let filter = ListFilter {
        status: args
            .status
            .as_deref()
            .map(str::parse::<Status>)
            .transpose()?,
        scope: args.scope.as_deref().map(str::parse).transpose()?,
        unused_days: args.unused_days,
        never_applied: args.never_applied,
        search: args.search.clone(),
    };
    let store = Store::open(db)?;
    let learnings = store.list(&filter)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match args.format {
        Format::Json => {
            let text = serde_json::to_string(&learnings).expect("Learning serializes");
            let _ = writeln!(out, "{text}");
        }
        Format::Text => {
            for learning in &learnings {
                let _ = writeln!(out, "{}", line(learning));
            }
        }
    }
    Ok(())
}

/// One learning on one line: id, status, scopes, title.
fn line(learning: &Learning) -> String {
    let scopes = learning
        .scopes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{}  {:<9}  [{scopes}]  {}",
        learning.id,
        learning.status.as_str(),
        learning.title
    )
}
