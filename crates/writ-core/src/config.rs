//! `config.toml`, as spec section 10 defines it.
//!
//! Parsing a string is not I/O, so the whole file format lives here where
//! it is tested without a filesystem. Reading the file is `writ-cli`'s job.
//! See invariant 1.
//!
//! Every documented key is parsed, including the ones later slices read.
//! An undocumented key is an error: a misspelled `max_rules` that is
//! silently ignored is the invisible failure P7 exists to prevent.

use serde::Deserialize;

use crate::error::{Error, Result};

/// The whole configuration file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// `[audit]`. Read by `writ audit`.
    pub audit: Audit,
    /// `[dedupe]`. Read by `writ record`.
    pub dedupe: Dedupe,
    /// `[identity]`. Read by `writ record` and `writ export`.
    pub identity: Identity,
    /// `[ui]`. Read by `writ ui`.
    pub ui: Ui,
}

/// `[audit]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Audit {
    /// The most learnings one audit prompt may carry.
    pub max_rules: u32,
    /// The most characters of rule text one audit prompt may carry.
    pub max_chars: u32,
}

/// `[dedupe]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Dedupe {
    /// How many near matches a write reports.
    pub warn_top_n: u32,
    /// The bm25 cutoff above which a write is refused.
    pub block_above: BlockAbove,
}

/// `[identity]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Identity {
    /// The author to stamp on a write. Empty means "ask git".
    pub author: String,
    /// Whether `writ export` carries the author.
    pub share_author: bool,
}

/// `[ui]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ui {
    /// The port `writ ui` listens on.
    pub port: u16,
    /// The command that opens a file at a line.
    pub editor_cmd: String,
}

/// `block_above` is either off or a bm25 cutoff.
///
/// bm25 in SQLite is negative, and more negative is a better match, so the
/// cutoff is a negative number and the comparison reads backwards from the
/// intuition. Spec section 7.3. MVP ships `false`, and there is no
/// calibrated number to default to.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum BlockAbove {
    /// `block_above = false`. No write is ever refused.
    Off(bool),
    /// `block_above = -8.5`. A bm25 score.
    Score(f64),
}

impl Default for Audit {
    fn default() -> Self {
        Self {
            max_rules: 40,
            max_chars: 20_000,
        }
    }
}

impl Default for Dedupe {
    fn default() -> Self {
        Self {
            warn_top_n: 3,
            block_above: BlockAbove::Off(false),
        }
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self {
            author: String::new(),
            share_author: true,
        }
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            port: 7749,
            editor_cmd: "cursor -g {path}:{line}".to_string(),
        }
    }
}

impl Config {
    /// Parse a `config.toml`. An absent file is the caller's business: it
    /// passes [`Config::default`] instead of calling this.
    pub fn parse(text: &str) -> Result<Self> {
        let config: Self = toml::from_str(text).map_err(|error| Error::Config {
            message: error.message().to_string(),
        })?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.dedupe.block_above == BlockAbove::Off(true) {
            return Err(Error::Config {
                message: "block_above = true names no cutoff. Give a bm25 score, or false"
                    .to_string(),
            });
        }
        if self.audit.max_rules == 0 || self.audit.max_chars == 0 {
            return Err(Error::Config {
                message: "max_rules and max_chars must be greater than zero".to_string(),
            });
        }
        Ok(())
    }

    /// The author to stamp on a write, or `None` when the file is silent.
    ///
    /// The git fallback is I/O and belongs to `writ-cli`, so this returns
    /// `None` rather than reaching for it. See invariant 1.
    pub fn configured_author(&self) -> Option<&str> {
        let author = self.identity.author.trim();
        (!author.is_empty()).then_some(author)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact block from spec section 10.
    const DOCUMENTED: &str = r#"
[audit]
max_rules = 40
max_chars = 20000

[dedupe]
warn_top_n  = 3
block_above = false

[identity]
author       = ""
share_author = true

[ui]
port = 7749
editor_cmd = "cursor -g {path}:{line}"
"#;

    #[test]
    fn the_documented_file_parses_to_the_defaults() {
        // P8: the file in the spec and the defaults in the code agree.
        assert_eq!(Config::parse(DOCUMENTED).unwrap(), Config::default());
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let config = Config::parse("[dedupe]\nwarn_top_n = 7\n").unwrap();
        assert_eq!(config.dedupe.warn_top_n, 7);
        assert_eq!(config.audit.max_rules, 40);
        assert!(config.identity.share_author);
    }

    #[test]
    fn malformed_toml_names_the_real_cause() {
        let error = Config::parse("[dedupe\nwarn_top_n = 3").unwrap_err();
        assert!(matches!(error, Error::Config { .. }), "{error}");
    }

    #[test]
    fn a_misspelled_key_is_refused_rather_than_ignored() {
        let error = Config::parse("[audit]\nmax_rulez = 10\n").unwrap_err();
        assert!(matches!(error, Error::Config { .. }), "{error}");
    }

    #[test]
    fn a_wrong_type_is_refused() {
        let error = Config::parse("[dedupe]\nwarn_top_n = \"three\"\n").unwrap_err();
        assert!(matches!(error, Error::Config { .. }), "{error}");
    }

    #[test]
    fn block_above_takes_false_or_a_score() {
        assert_eq!(
            Config::parse("[dedupe]\nblock_above = false\n")
                .unwrap()
                .dedupe
                .block_above,
            BlockAbove::Off(false)
        );
        assert_eq!(
            Config::parse("[dedupe]\nblock_above = -8.5\n")
                .unwrap()
                .dedupe
                .block_above,
            BlockAbove::Score(-8.5)
        );
    }

    #[test]
    fn block_above_true_names_no_cutoff() {
        let error = Config::parse("[dedupe]\nblock_above = true\n").unwrap_err();
        assert!(error.to_string().contains("bm25"), "{error}");
    }

    #[test]
    fn the_author_falls_through_when_the_file_is_silent() {
        assert_eq!(Config::default().configured_author(), None);
        let config = Config::parse("[identity]\nauthor = \"  \"\n").unwrap();
        assert_eq!(config.configured_author(), None);
    }

    #[test]
    fn a_configured_author_is_returned_trimmed() {
        let config = Config::parse("[identity]\nauthor = \" dev@example.com \"\n").unwrap();
        assert_eq!(config.configured_author(), Some("dev@example.com"));
    }
}
