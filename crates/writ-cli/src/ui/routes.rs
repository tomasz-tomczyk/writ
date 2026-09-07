use axum::Router;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rust_embed::RustEmbed;

use crate::ui::AppState;
use crate::ui::actions;
use crate::ui::pages::{collection, detail, health, inbox, layout};

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/inbox", get(inbox))
        .route("/inbox/{id}/approve", post(actions::approve))
        .route("/inbox/{id}/reject", post(actions::reject_proposal))
        .route("/collection", get(collection))
        .route("/health", get(health))
        .route("/health/{id}/archive", post(actions::health_archive))
        .route("/health/{id}/edit", get(actions::health_edit))
        .route("/health/{id}/keep", get(actions::health_keep))
        .route("/learnings/{id}", get(detail::get).post(detail::post))
        .route("/learnings/{id}/archive", post(detail::archive))
        .route("/findings/{id}/reject", post(actions::reject_finding))
        .route("/findings/{id}/open", post(actions::open_editor))
        .route("/assets/{*path}", get(assets))
        .with_state(state)
}

async fn root(State(state): State<AppState>) -> Response {
    let proposed = match layout::count_proposed(&state) {
        Ok(count) => count,
        Err(error) => return layout::error_response(error),
    };
    let target = if proposed > 0 {
        "/inbox"
    } else {
        "/collection"
    };
    with_protocol_header(
        Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, HeaderValue::from_static(target))
            .body(axum::body::Body::empty())
            .unwrap(),
    )
}

async fn inbox(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match inbox::render(&state) {
        Ok(body) if layout::is_htmx(&headers) => layout::fragment(body.as_str()),
        Ok(body) => layout::render(&state, "Inbox", body.as_str()),
        Err(error) => layout::error_response(error),
    }
}

#[derive(Debug, serde::Deserialize)]
struct CollectionQuery {
    q: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
    status: Option<String>,
}

async fn collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CollectionQuery>,
    RawQuery(raw): RawQuery,
) -> Response {
    let projects = projects_from_raw(raw.as_deref());
    match collection::render(
        &state,
        query.q.as_deref(),
        query.sort.as_deref(),
        query.dir.as_deref(),
        query.status.as_deref(),
        &projects,
    ) {
        Ok(body) if layout::is_htmx(&headers) => layout::fragment(body.as_str()),
        Ok(body) => layout::render(&state, "Collection", body.as_str()),
        Err(error) => layout::error_response(error),
    }
}

/// Repeated `project=` keys. `serde_urlencoded` rejects duplicates, so
/// Collection reads them from the raw query string instead.
fn projects_from_raw(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    raw.split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            if key != "project" {
                return None;
            }
            let decoded = urldecode(value);
            if decoded.is_empty() {
                None
            } else {
                Some(decoded)
            }
        })
        .collect()
}

fn urldecode(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

async fn health(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match health::render(&state) {
        Ok(body) if layout::is_htmx(&headers) => layout::fragment(body.as_str()),
        Ok(body) => layout::render(&state, "Health", body.as_str()),
        Err(error) => layout::error_response(error),
    }
}

async fn assets(Path(path): Path<String>) -> Response {
    match Assets::get(&path) {
        Some(content) => {
            let content_type = content_type_for_path(&path);
            let mut response = content.data.into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            response
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn content_type_for_path(path: &str) -> &'static str {
    if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".js") {
        "application/javascript"
    } else {
        "application/octet-stream"
    }
}

fn with_protocol_header(mut response: Response) -> Response {
    response.headers_mut().insert(
        "x-writ-protocol-version",
        HeaderValue::from(crate::ui::UI_PROTOCOL_VERSION),
    );
    response
}
