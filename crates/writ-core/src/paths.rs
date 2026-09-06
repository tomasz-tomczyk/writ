use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The base directories writ reads its defaults from.
///
/// The struct is explicit so a test can describe an environment without
/// touching the process environment, which is global and shared between
/// parallel tests.
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// `$HOME`.
    pub home: Option<PathBuf>,
    /// `$XDG_DATA_HOME`.
    pub xdg_data_home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`.
    pub xdg_config_home: Option<PathBuf>,
}

impl Env {
    /// Read the base directories from the process environment.
    pub fn from_os() -> Self {
        Self {
            home: non_empty_var("HOME"),
            xdg_data_home: non_empty_var("XDG_DATA_HOME"),
            xdg_config_home: non_empty_var("XDG_CONFIG_HOME"),
        }
    }
}

fn non_empty_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The paths every subcommand needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The learnings SQLite database file.
    pub db: PathBuf,
    /// The physically separate aggregate telemetry database file.
    pub telemetry_db: PathBuf,
    /// The `config.toml` file. It does not have to exist.
    pub config: PathBuf,
}

/// Resolve the database and config paths.
///
/// This is the one place that answers the question. Every subcommand must
/// call it, so `--db` and `--config` cannot be honored by one command and
/// ignored by the next. See spec section 4.1 and crit #763.
///
/// An override wins over the XDG variable, which wins over the `$HOME`
/// default. writ follows the XDG base directory specification on every
/// platform, macOS included.
pub fn resolve_paths(env: &Env, db: Option<&Path>, config: Option<&Path>) -> Result<Paths> {
    let data_directory = data_home(env).ok().map(|home| home.join("writ"));
    let db = match db {
        Some(path) => path.to_path_buf(),
        None => data_directory
            .as_ref()
            .ok_or(Error::NoBaseDirectory {
                what: "data",
                var: "XDG_DATA_HOME",
            })?
            .join("learnings.db"),
    };
    let telemetry_db = match data_directory {
        Some(directory) => directory.join("telemetry.db"),
        None => {
            // Preserve the explicit-override contract when the process has no
            // base-directory environment at all, while never aliasing the two
            // databases even if the override itself is named telemetry.db.
            let sibling = db.with_file_name("telemetry.db");
            if sibling == db {
                db.with_file_name("writ-telemetry.db")
            } else {
                sibling
            }
        }
    };
    let config = match config {
        Some(path) => path.to_path_buf(),
        None => config_home(env)?.join("writ").join("config.toml"),
    };
    Ok(Paths {
        db,
        telemetry_db,
        config,
    })
}

fn data_home(env: &Env) -> Result<PathBuf> {
    if let Some(dir) = &env.xdg_data_home {
        return Ok(dir.clone());
    }
    let home = env.home.as_ref().ok_or(Error::NoBaseDirectory {
        what: "data",
        var: "XDG_DATA_HOME",
    })?;
    Ok(home.join(".local").join("share"))
}

fn config_home(env: &Env) -> Result<PathBuf> {
    if let Some(dir) = &env.xdg_config_home {
        return Ok(dir.clone());
    }
    let home = env.home.as_ref().ok_or(Error::NoBaseDirectory {
        what: "config",
        var: "XDG_CONFIG_HOME",
    })?;
    Ok(home.join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            home: Some(PathBuf::from("/home/dev")),
            ..Env::default()
        }
    }

    #[test]
    fn home_defaults_follow_xdg() {
        let paths = resolve_paths(&env(), None, None).unwrap();
        assert_eq!(
            paths.db,
            PathBuf::from("/home/dev/.local/share/writ/learnings.db")
        );
        assert_eq!(
            paths.config,
            PathBuf::from("/home/dev/.config/writ/config.toml")
        );
        assert_eq!(
            paths.telemetry_db,
            PathBuf::from("/home/dev/.local/share/writ/telemetry.db")
        );
    }

    #[test]
    fn xdg_variables_beat_the_home_default() {
        let env = Env {
            home: Some(PathBuf::from("/home/dev")),
            xdg_data_home: Some(PathBuf::from("/data")),
            xdg_config_home: Some(PathBuf::from("/conf")),
        };
        let paths = resolve_paths(&env, None, None).unwrap();
        assert_eq!(paths.db, PathBuf::from("/data/writ/learnings.db"));
        assert_eq!(paths.telemetry_db, PathBuf::from("/data/writ/telemetry.db"));
        assert_eq!(paths.config, PathBuf::from("/conf/writ/config.toml"));
    }

    #[test]
    fn overrides_beat_everything() {
        let env = Env {
            home: Some(PathBuf::from("/home/dev")),
            xdg_data_home: Some(PathBuf::from("/data")),
            xdg_config_home: Some(PathBuf::from("/conf")),
        };
        let paths = resolve_paths(
            &env,
            Some(Path::new("/tmp/other.db")),
            Some(Path::new("/tmp/other.toml")),
        )
        .unwrap();
        assert_eq!(paths.db, PathBuf::from("/tmp/other.db"));
        assert_eq!(paths.telemetry_db, PathBuf::from("/data/writ/telemetry.db"));
        assert_eq!(paths.config, PathBuf::from("/tmp/other.toml"));
    }

    #[test]
    fn one_override_leaves_the_other_default() {
        let paths = resolve_paths(&env(), Some(Path::new("/tmp/only.db")), None).unwrap();
        assert_eq!(paths.db, PathBuf::from("/tmp/only.db"));
        assert_eq!(
            paths.config,
            PathBuf::from("/home/dev/.config/writ/config.toml")
        );
    }

    #[test]
    fn no_home_and_no_xdg_names_the_real_cause() {
        let error = resolve_paths(&Env::default(), None, None).unwrap_err();
        assert!(matches!(error, Error::NoBaseDirectory { what: "data", .. }));
        assert!(error.to_string().contains("XDG_DATA_HOME"));
    }

    #[test]
    fn an_override_works_without_any_base_directory() {
        let paths = resolve_paths(
            &Env::default(),
            Some(Path::new("/tmp/a.db")),
            Some(Path::new("/tmp/a.toml")),
        )
        .unwrap();
        assert_eq!(paths.db, PathBuf::from("/tmp/a.db"));
        assert_eq!(paths.telemetry_db, PathBuf::from("/tmp/telemetry.db"));
    }

    #[test]
    fn an_override_can_never_alias_the_telemetry_store() {
        let paths = resolve_paths(
            &Env::default(),
            Some(Path::new("/tmp/telemetry.db")),
            Some(Path::new("/tmp/a.toml")),
        )
        .unwrap();
        assert_eq!(paths.db, PathBuf::from("/tmp/telemetry.db"));
        assert_eq!(paths.telemetry_db, PathBuf::from("/tmp/writ-telemetry.db"));
        assert_ne!(paths.db, paths.telemetry_db);
    }
}
