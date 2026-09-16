//! The `--format` flag. Spec section 5: json is available on every command.

use std::fmt;
use std::str::FromStr;

/// How a command prints its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// For a person.
    Text,
    /// For a caller.
    Json,
}

impl FromStr for Format {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => Err(format!("unknown format {other}. Use text or json")),
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Text => "text",
            Self::Json => "json",
        })
    }
}

/// How `writ audit` prints its result.
///
/// `prompt` is audit's alone, so it lives in its own enum. Adding it to
/// [`Format`] would make `writ list --format prompt` parse and then mean
/// nothing, which is the kind of silent no-op P7 forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditFormat {
    /// For the host agent. Asserted byte for byte by a golden test.
    Prompt,
    /// For a caller.
    Json,
    /// For a person.
    Text,
}

/// What `--resolve` may settle a finding to.
///
/// `rejected` is absent on purpose: section 7.5 gives it to the developer
/// alone, and the UI is their path. `open` is absent because settling is
/// what this does — a finding that stays open needs no command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// The finding was corrected.
    Fixed,
    /// The finding was left as it stands.
    Ignored,
}

impl ResolveOutcome {
    /// The core value this settles to.
    pub fn outcome(self) -> writ_core::Outcome {
        match self {
            Self::Fixed => writ_core::Outcome::Fixed,
            Self::Ignored => writ_core::Outcome::Ignored,
        }
    }
}

impl FromStr for ResolveOutcome {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "fixed" => Ok(Self::Fixed),
            "ignored" => Ok(Self::Ignored),
            "rejected" => {
                Err("only a developer rejects a finding. Use the collection UI".to_string())
            }
            other => Err(format!("unknown outcome {other}. Use fixed or ignored")),
        }
    }
}

impl fmt::Display for ResolveOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Fixed => "fixed",
            Self::Ignored => "ignored",
        })
    }
}

impl FromStr for AuditFormat {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "prompt" => Ok(Self::Prompt),
            "json" => Ok(Self::Json),
            "text" => Ok(Self::Text),
            other => Err(format!("unknown format {other}. Use prompt, json or text")),
        }
    }
}

impl fmt::Display for AuditFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Prompt => "prompt",
            Self::Json => "json",
            Self::Text => "text",
        })
    }
}
