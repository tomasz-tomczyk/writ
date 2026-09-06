//! The local web interface server.

mod pages;
mod routes;
mod state;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::{Command, ExitCode};

use writ_core::{Config, Error, Paths, Store};

pub use routes::router;
pub use state::AppState;

/// Version of the protocol spoken by the UI server.
pub const UI_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Port to listen on. Overrides [ui].port
    #[arg(long)]
    pub port: Option<u16>,

    /// Do not open a browser
    #[arg(long)]
    pub no_open: bool,
}

pub fn run(args: &Args, paths: &Paths, config: &Config) -> Result<ExitCode, Error> {
    let store = Store::open(&paths.db)?;
    let state = AppState {
        db: paths.db.clone(),
        config: config.clone(),
        store: std::sync::Arc::new(std::sync::Mutex::new(store)),
    };
    let port = args.port.unwrap_or(config.ui.port);
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let url = format!("http://{address}");

    let runtime = tokio::runtime::Runtime::new().map_err(|error| Error::Command {
        program: "writ ui".to_string(),
        message: format!("cannot start the async runtime: {error}"),
    })?;

    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|error| Error::Command {
                program: "writ ui".to_string(),
                message: format!("cannot bind {address}: {error}"),
            })?;

        println!("writ ui listening on {url}");

        if !args.no_open
            && let Err(error) = open_browser(&url)
        {
            eprintln!("writ: warning: cannot open browser: {error}");
        }

        axum::serve(listener, routes::router(state))
            .await
            .map_err(|error| Error::Command {
                program: "writ ui".to_string(),
                message: format!("server failed: {error}"),
            })
    })?;

    Ok(ExitCode::SUCCESS)
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status()?;

    #[cfg(target_os = "linux")]
    let status = Command::new("xdg-open").arg(url).status()?;

    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()?;

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "this platform has no configured browser opener",
    ));

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "browser opener exited with {status}"
        )))
    }
}
