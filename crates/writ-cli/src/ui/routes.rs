use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
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
        .route("/inbox/{id}/merge", post(actions::merge))
        .route("/collection", get(collection))
        .route("/health", get(health))
        .route("/learnings/{id}", get(detail::get).post(detail::post))
        .route("/learnings/{id}/archive", post(detail::archive))
        .route("/findings/{id}/reject", post(actions::reject_finding))
        .route("/findings/{id}/open", post(actions::open_editor))
        .route("/assets/{*path}", get(assets))
        .with_state(state)
}

async fn root(State(state): State<AppState>) -> Response {
    let proposed = layout::count_proposed(&state);
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

async fn inbox(State(state): State<AppState>) -> Response {
    match inbox::render(&state) {
        Ok(body) => layout::render(&state, "Inbox", &body),
        Err(error) => layout::error_response(error),
    }
}

async fn collection(State(state): State<AppState>) -> Response {
    match collection::render(&state, None, None, None, None) {
        Ok(body) => layout::render(&state, "Collection", &body),
        Err(error) => layout::error_response(error),
    }
}

async fn health(State(state): State<AppState>) -> Response {
    match health::render(&state) {
        Ok(body) => layout::render(&state, "Health", &body),
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
