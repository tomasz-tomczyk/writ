use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header::HeaderValue;
use axum::response::{Html, IntoResponse, Response};
use writ_core::{Error, ListFilter, Status, Store};

use crate::ui::AppState;
use crate::ui::UI_PROTOCOL_VERSION;

/// Render a full HTML page inside the shared layout.
pub fn render(state: &AppState, title: &str, body: &str) -> Response {
    let badge = match inbox_badge(state) {
        Ok(badge) => badge,
        Err(error) => return error_response(error),
    };
    let store_path = escape(&state.db.display().to_string());
    let inbox_current = current_page(title, "Inbox");
    let collection_current = current_page(title, "Collection");
    let health_current = current_page(title, "Health");
    let lede = page_lede(title);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="light">
  <meta name="theme-color" content="{theme_color}">
  <title>{title} · writ</title>
  <link rel="stylesheet" href="/assets/app.css">
  <script src="/assets/htmx.min.js"></script>
</head>
<body>
  <header class="app-header">
    <nav class="app-header__inner" aria-label="Primary">
      <a href="/" class="brand" aria-label="writ home">
        <span class="brand-mark" aria-hidden="true"></span>
        <span>writ</span>
      </a>
      <div class="nav-links">
        <a href="/inbox" class="nav-link"{inbox_current}>Inbox{badge}</a>
        <a href="/collection" class="nav-link"{collection_current}>Collection</a>
        <a href="/health" class="nav-link"{health_current}>Health</a>
      </div>
    </nav>
  </header>
  <main class="page">
    <div class="page-header">
      <h1>{title}</h1>
      <p>{lede}</p>
    </div>
    {body}
  </main>
  <footer class="store">
    <span class="label">Store</span>
    <code>{store_path}</code>
  </footer>
</body>
</html>"#,
        theme_color = "#ffffff",
        title = title,
        inbox_current = inbox_current,
        collection_current = collection_current,
        health_current = health_current,
        badge = badge,
        lede = lede,
        body = body,
        store_path = store_path,
    );

    with_version(Html(html).into_response())
}

fn current_page(title: &str, page: &str) -> &'static str {
    if title == page {
        r#" aria-current="page""#
    } else {
        ""
    }
}

fn page_lede(title: &str) -> &'static str {
    match title {
        "Inbox" => "Review proposed learnings before they enter an audit.",
        "Collection" => "Browse the rules your agents have learned and where they apply.",
        "Health" => "Find active learnings that may need maintenance.",
        "Detail" => "Edit this learning, its scope, and its supporting evidence.",
        _ => "",
    }
}

/// Render one fragment, for an htmx swap.
///
/// A fragment carries no layout: htmx replaces the target element with
/// exactly this markup. Spec section 9.4.
pub fn fragment(body: &str) -> Response {
    with_version(Html(body.to_string()).into_response())
}

/// Whether htmx sent this request. Every action still answers a plain
/// form POST, so the page is never dead without htmx.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

/// The navigation Inbox count.
///
/// The element is always present, so an out-of-band swap has a target
/// even when the count reaches zero. Empty content hides it in CSS.
pub fn inbox_badge(state: &AppState) -> Result<String, Error> {
    let count = count_proposed(state)?;
    let text = if count > 0 {
        count.to_string()
    } else {
        String::new()
    };
    Ok(format!(
        r#"<span class="badge" id="inbox-badge">{text}</span>"#
    ))
}

/// The same badge, marked for an out-of-band swap.
///
/// A row that moves out of the Inbox must move the count with it. A stale
/// count is a small lie.
pub fn inbox_badge_oob(state: &AppState) -> Result<String, Error> {
    let count = count_proposed(state)?;
    let text = if count > 0 {
        count.to_string()
    } else {
        String::new()
    };
    Ok(format!(
        r#"<span class="badge" id="inbox-badge" hx-swap-oob="true">{text}</span>"#
    ))
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
    with_version((status, escape(&error.to_string())).into_response())
}

fn with_version(mut response: Response) -> Response {
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
