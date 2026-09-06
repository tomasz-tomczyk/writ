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

/// Persist the one setting only the explicit telemetry commands may change.
///
/// Existing configuration remains human-owned: sections, comments and spacing
/// outside the `enabled` line are preserved. The caller loads and validates the
/// file first, so this function never papers over malformed TOML.
pub fn set_telemetry_enabled(path: &Path, enabled: bool) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(Error::Config {
                message: format!("{}: {error}", path.display()),
            });
        }
    };
    let updated = replace_telemetry_setting(&text, enabled);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| Error::Config {
            message: format!("{}: {error}", parent.display()),
        })?;
    }
    std::fs::write(path, updated).map_err(|error| Error::Config {
        message: format!("{}: {error}", path.display()),
    })
}

fn replace_telemetry_setting(text: &str, enabled: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = lines.iter().position(|line| line.trim() == "[telemetry]");
    let setting = format!("enabled = {enabled}");

    match header {
        Some(header) => {
            let end = lines[header + 1..]
                .iter()
                .position(|line| {
                    let line = line.trim();
                    line.starts_with('[') && line.ends_with(']')
                })
                .map_or(lines.len(), |offset| header + 1 + offset);
            if let Some(index) = (header + 1..end).find(|index| {
                lines[*index]
                    .split_once('=')
                    .is_some_and(|(key, _)| key.trim() == "enabled")
            }) {
                lines[index] = setting;
            } else {
                lines.insert(header + 1, setting);
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
                lines.push(String::new());
            }
            lines.push("[telemetry]".to_string());
            lines.push(setting);
        }
    }
    let mut output = lines.join("\n");
    output.push('\n');
    output
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_setting_is_added_without_rewriting_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# mine\n[audit]\nmax_rules = 7\n").unwrap();

        set_telemetry_enabled(&path, true).unwrap();

        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.starts_with("# mine\n[audit]\nmax_rules = 7\n"));
        assert!(text.contains("[telemetry]\nenabled = true\n"));
        assert!(Config::parse(&text).unwrap().telemetry.enabled);
    }

    #[test]
    fn telemetry_setting_is_replaced_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[telemetry]\n# local only\nenabled = true\n\n[ui]\nport = 8123\n",
        )
        .unwrap();

        set_telemetry_enabled(&path, false).unwrap();

        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(
            text,
            "[telemetry]\n# local only\nenabled = false\n\n[ui]\nport = 8123\n"
        );
        assert!(!Config::parse(&text).unwrap().telemetry.enabled);
    }
}
