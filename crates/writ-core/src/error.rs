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
}
