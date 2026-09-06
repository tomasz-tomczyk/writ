//! Everything a subcommand needs from the outside world.
//!
//! This crate is the only one that reads a file, runs a program, or looks
//! at a terminal. `writ-core` takes the answers as arguments. Invariant 1.

use std::path::Path;
use std::process::Command;

use writ_core::{Config, Error, Result};

/// Load `config.toml`.
///
/// A missing file is not an error: the defaults apply, and a first run has
/// no file. Anything else is reported, because a configuration writ cannot
/// read is a setting writ is silently ignoring. See P7.
pub fn load_config(path: &Path) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(text) => Config::parse(&text).map_err(|error| match error {
            Error::Config { message } => Error::Config {
                message: format!("{}: {message}", path.display()),
            },
            other => other,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(Error::Config {
            message: format!("{}: {error}", path.display()),
        }),
    }
}

/// Who to stamp on a write.
///
/// `[identity] author` wins. Otherwise `git config user.email`, which is
/// the answer outside as well as inside a repository, because git reads the
/// global file too. Outside git with no key it stays NULL. Spec section 6.
pub fn resolve_author(config: &Config) -> Option<String> {
    if let Some(author) = config.configured_author() {
        return Some(author.to_string());
    }
    git_user_email()
}

fn git_user_email() -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", "user.email"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let email = String::from_utf8(output.stdout).ok()?;
    let email = email.trim();
    (!email.is_empty()).then(|| email.to_string())
}

/// Read a snippet file for `--example`.
///
/// The text is copied in. The path is not stored anywhere: exemplars hold
/// snippet text and never a location. Invariant 3.
pub fn read_snippet(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| Error::Validation {
        message: format!("cannot read the example file {}: {error}", path.display()),
    })
}
