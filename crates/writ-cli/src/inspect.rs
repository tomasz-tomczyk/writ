//! `writ show` and `writ archive`. Spec section 5.
//!
//! Both take an id, and both exit `5` when no learning carries it.
//! Section 5.7 gives "not found" its own code so that a typo in an id can
//! never read as a usage error or a storage failure.

use std::io::Write;
use std::path::Path;

use writ_core::{Exemplar, Learning, Result, Status, Store};

use crate::output::Format;

/// `writ show ID`.
#[derive(Debug, clap::Args)]
pub struct ShowArgs {
    /// The learning to read
    #[arg(value_name = "ID")]
    id: String,

    /// text or json
    #[arg(long, default_value_t = Format::Text, value_name = "FORMAT")]
    format: Format,
}

/// `writ archive ID`.
#[derive(Debug, clap::Args)]
pub struct ArchiveArgs {
    /// The learning to archive
    #[arg(value_name = "ID")]
    id: String,

    /// text or json
    #[arg(long, default_value_t = Format::Text, value_name = "FORMAT")]
    format: Format,
}

/// Read one learning, with its exemplars.
pub fn show(args: &ShowArgs, db: &Path) -> Result<()> {
    let store = Store::open(db)?;
    let learning = store.get(&args.id)?;
    let exemplars = store.exemplars_of(&learning.id)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match args.format {
        Format::Json => {
            let text = serde_json::to_string(&serde_json::json!({
                "learning": learning,
                "exemplars": exemplars,
            }))
            .expect("Learning serializes");
            let _ = writeln!(out, "{text}");
        }
        Format::Text => {
            let _ = write!(out, "{}", detail(&learning, &exemplars));
        }
    }
    Ok(())
}

/// Prune a learning. P4: nothing is destroyed, an archived rule stops
/// being selected and stays in the database.
pub fn archive(args: &ArchiveArgs, db: &Path) -> Result<()> {
    let mut store = Store::open(db)?;
    store.set_status(&args.id, Status::Archived)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match args.format {
        Format::Json => {
            let text = serde_json::to_string(&serde_json::json!({
                "id": args.id,
                "status": "archived",
            }))
            .expect("the report serializes");
            let _ = writeln!(out, "{text}");
        }
        // crit #446: every write says what it wrote.
        Format::Text => {
            let _ = writeln!(out, "archived {}", args.id);
        }
    }
    Ok(())
}

/// One learning, in full.
fn detail(learning: &Learning, exemplars: &[Exemplar]) -> String {
    let scopes = learning
        .scopes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut text = format!("{}  {}\n", learning.id, learning.title);
    text.push_str(&format!("status: {}\n", learning.status.as_str()));
    text.push_str(&format!(
        "enforcement: {}\n",
        if learning.blocking {
            "blocking"
        } else {
            "advisory"
        }
    ));
    if learning.sides != writ_core::Sides::Both {
        text.push_str(&format!("sides: {}\n", learning.sides));
    }
    text.push_str(&format!("scopes: {scopes}\n"));
    text.push_str(&format!("rule: {}\n", learning.rule));
    text.push_str(&format!("why: {}\n", learning.rationale));
    if let (Some(matcher), Some(kind)) = (&learning.matcher, learning.matcher_kind) {
        text.push_str(&format!("matcher ({}): {matcher}\n", kind.as_str()));
    }
    text.push_str(&format!(
        "reinforced: {}  selected: {}  applied: {}\n",
        learning.reinforced, learning.times_selected, learning.times_applied
    ));
    text.push_str(&format!(
        "created: {}  updated: {}\n",
        learning.created_at, learning.updated_at
    ));
    for exemplar in exemplars {
        text.push_str(&format!("{}:\n", exemplar.kind.as_str()));
        for line in exemplar.snippet.lines() {
            text.push_str(&format!("  {line}\n"));
        }
    }
    text
}
