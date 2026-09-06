use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use writ_core::Error;

use crate::ui::AppState;

pub fn render(_state: &AppState, _id: &str) -> Result<String, Error> {
    Ok(r#"<div class="shell">Detail</div>"#.into())
}

pub async fn get(Path(_id): Path<String>, State(_state): State<AppState>) -> Response {
    StatusCode::NOT_FOUND.into_response()
}

#[derive(Debug, serde::Deserialize)]
pub struct SaveForm {}

pub async fn post(
    Path(_id): Path<String>,
    State(_state): State<AppState>,
    Form(_form): Form<SaveForm>,
) -> Response {
    StatusCode::NOT_FOUND.into_response()
}

pub async fn archive(Path(_id): Path<String>, State(_state): State<AppState>) -> Response {
    StatusCode::NOT_FOUND.into_response()
}
