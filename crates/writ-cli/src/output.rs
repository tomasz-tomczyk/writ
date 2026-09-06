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
