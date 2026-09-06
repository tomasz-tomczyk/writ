use std::path::PathBuf;

/// The result type every fallible `writ-core` function returns.
pub type Result<T> = std::result::Result<T, Error>;

/// Every error names its real cause. See spec P7.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Neither `XDG_DATA_HOME`/`XDG_CONFIG_HOME` nor `HOME` is set, so no
    /// default path can be built.
    #[error("cannot find the {what} directory: neither {var} nor HOME is set")]
    NoBaseDirectory {
        /// `data` or `config`.
        what: &'static str,
        /// The XDG variable that would have answered the question.
        var: &'static str,
    },

    /// The parent directory of the database could not be created.
    #[error("cannot create the directory {path}: {source}")]
    CreateDirectory {
        /// The directory writ tried to create.
        path: PathBuf,
        /// The underlying filesystem error.
        source: std::io::Error,
    },

    /// SQLite refused a statement or a connection.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// `config.toml` is not valid TOML, or a value in it is not usable.
    ///
    /// A malformed configuration is never ignored. A silently dropped
    /// setting is the invisible failure P7 exists to prevent.
    #[error("cannot read the configuration: {message}")]
    Config {
        /// What is wrong, in the author's terms.
        message: String,
    },

    /// A caller asked for something the rules do not allow.
    #[error("{message}")]
    Validation {
        /// What is wrong, in the author's terms.
        message: String,
    },

    /// A JSONL line could not be parsed. The line number is 1-based, so it
    /// matches what an editor shows.
    #[error("line {line} is not valid JSON: {message}")]
    BadJson {
        /// The 1-based line number in the stream.
        line: usize,
        /// The parser's complaint.
        message: String,
    },

    /// No learning carries this id.
    #[error("no learning has id {id}")]
    NotFound {
        /// The id the caller asked for.
        id: String,
    },
}

impl Error {
    /// Build a [`Error::Validation`] from anything printable.
    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }
}
