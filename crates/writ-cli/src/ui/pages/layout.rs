use std::sync::MutexGuard;

use axum::http::StatusCode;
use axum::http::header::HeaderValue;
use axum::response::{Html, IntoResponse, Response};
use writ_core::{Error, ListFilter, Status, Store};

use crate::ui::AppState;
use crate::ui::UI_PROTOCOL_VERSION;

/// Render a full HTML page inside the shared layout.
pub fn render(state: &AppState, title: &str, body: &str) -> Response {
    let proposed_count = count_proposed(state);
    let inbox_badge = if proposed_count > 0 {
        format!(r#"<span class="badge">{proposed_count}</span>"#)
    } else {
        String::new()
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} · writ</title>
  <link rel="stylesheet" href="/assets/app.css">
  <script src="/assets/htmx.min.js"></script>
</head>
<body>
  <header>
    <nav>
      <a href="/" class="brand">writ</a>
      <a href="/inbox">Inbox{inbox_badge}</a>
      <a href="/collection">Collection</a>
      <a href="/health">Health</a>
    </nav>
  </header>
  <main>
    <h1>{title}</h1>
    {body}
  </main>
</body>
</html>"#
    );

    let mut response = Html(html).into_response();
    response.headers_mut().insert(
        "x-writ-protocol-version",
        HeaderValue::from(UI_PROTOCOL_VERSION),
    );
    response
}

pub fn count_proposed(state: &AppState) -> usize {
    match state.store.lock() {
        Ok(store) => proposed_len(&store),
        Err(poisoned) => proposed_len(&poisoned.into_inner()),
    }
}

fn proposed_len(store: &MutexGuard<'_, Store>) -> usize {
    store
        .list(&ListFilter {
            status: Some(Status::Proposed),
            ..Default::default()
        })
        .map(|rows| rows.len())
        .unwrap_or(0)
}

/// Escape text for insertion into HTML.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Turn a `writ-core` error into an HTTP response.
pub fn error_response(error: Error) -> Response {
    let status = match error {
        Error::NotFound { .. } => StatusCode::NOT_FOUND,
        Error::Validation { .. } => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let mut response = (status, escape(&error.to_string())).into_response();
    response.headers_mut().insert(
        "x-writ-protocol-version",
        HeaderValue::from(UI_PROTOCOL_VERSION),
    );
    response
}

/// Lock the shared store, recovering from poisoning.
pub fn with_store<T, F>(state: &AppState, f: F) -> Result<T, Error>
where
    F: FnOnce(&mut Store) -> Result<T, Error>,
{
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut store)
}
