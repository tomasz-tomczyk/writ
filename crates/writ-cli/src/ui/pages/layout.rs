use axum::http::StatusCode;
use axum::http::header::HeaderValue;
use axum::response::{Html, IntoResponse, Response};
use writ_core::{Error, ListFilter, Status, Store};

use crate::ui::AppState;
use crate::ui::UI_PROTOCOL_VERSION;

/// Render a full HTML page inside the shared layout.
pub fn render(state: &AppState, title: &str, body: &str) -> Response {
    let proposed_count = match count_proposed(state) {
        Ok(count) => count,
        Err(error) => return error_response(error),
    };
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

/// Count proposed learnings, or surface a real store/lock failure (P7).
pub fn count_proposed(state: &AppState) -> Result<usize, Error> {
    with_store(state, |store| {
        let rows = store.list(&ListFilter {
            status: Some(Status::Proposed),
            ..Default::default()
        })?;
        Ok(rows.len())
    })
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

/// Lock the shared store. A poisoned lock is reported, not papered over (P7).
pub fn with_store<T, F>(state: &AppState, f: F) -> Result<T, Error>
where
    F: FnOnce(&mut Store) -> Result<T, Error>,
{
    let mut store = state.store.lock().map_err(|_| Error::Command {
        program: "writ ui".to_string(),
        message: "store lock was poisoned".to_string(),
    })?;
    f(&mut store)
}
