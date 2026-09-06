//! The local web interface server.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::{Command, ExitCode};

use axum::Router;
use axum::http::{HeaderMap, HeaderValue};
use axum::routing::get;
use writ_core::{Config, Error, Paths, Store};

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
    let _store = Store::open(&paths.db)?;
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

        axum::serve(listener, Router::new().route("/", get(root)))
            .await
            .map_err(|error| Error::Command {
                program: "writ ui".to_string(),
                message: format!("server failed: {error}"),
            })
    })?;

    Ok(ExitCode::SUCCESS)
}

async fn root() -> (HeaderMap, &'static str) {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-writ-protocol-version",
        HeaderValue::from(UI_PROTOCOL_VERSION),
    );
    (headers, "writ ui")
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
