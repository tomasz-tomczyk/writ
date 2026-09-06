use axum::Form;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use writ_core::Status;

use crate::ui::AppState;
use crate::ui::pages::layout::{error_response, with_store};

pub async fn approve(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    match with_store(&state, |store| store.set_status(&id, Status::Active)) {
        Ok(()) => redirect("/inbox"),
        Err(error) => error_response(error),
    }
}

pub async fn reject_proposal(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    match with_store(&state, |store| store.set_status(&id, Status::Archived)) {
        Ok(()) => redirect("/inbox"),
        Err(error) => error_response(error),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct MergeForm {
    target_id: String,
}

pub async fn merge(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Form(form): Form<MergeForm>,
) -> Response {
    let result = with_store(&state, |store| {
        let exemplars: Vec<writ_core::NewExemplar> = store
            .exemplars_of(&id)?
            .into_iter()
            .map(|e| writ_core::NewExemplar {
                kind: e.kind,
                language: e.language,
                snippet: e.snippet,
                note: e.note,
            })
            .collect();
        store.reinforce(&form.target_id, &exemplars, None)?;
        store.set_status(&id, Status::Archived)
    });
    match result {
        Ok(()) => redirect("/inbox"),
        Err(error) => error_response(error),
    }
}

pub async fn reject_finding(Path(_id): Path<String>, State(_state): State<AppState>) -> Response {
    StatusCode::NOT_FOUND.into_response()
}

pub async fn open_editor(Path(_id): Path<String>, State(_state): State<AppState>) -> Response {
    StatusCode::NOT_FOUND.into_response()
}

fn redirect(path: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(header::LOCATION, path.parse().unwrap());
    response
}
