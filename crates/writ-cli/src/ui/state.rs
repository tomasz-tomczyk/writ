use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use writ_core::{Config, Store};

/// Shared server state.
#[derive(Clone)]
pub struct AppState {
    pub db: PathBuf,
    pub config: Config,
    pub store: Arc<Mutex<Store>>,
}

impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState")
            .field("db", &self.db)
            .field("config", &self.config)
            .field("store", &"<Store>")
            .finish()
    }
}
