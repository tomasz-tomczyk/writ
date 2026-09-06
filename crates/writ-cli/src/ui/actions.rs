use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use writ_core::{Error, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{error_response, with_store};
use crate::ui::pages::{detail, inbox, layout};

/// Answer an Inbox action.
///
/// htmx gets the list fragment plus an out-of-band badge, so the row and
/// the navigation count move together. Anything else gets the redirect a
/// plain form POST expects, so the page still works without htmx.
fn inbox_reply(state: &AppState, headers: &HeaderMap) -> Response {
    if !layout::is_htmx(headers) {
        return redirect("/inbox");
    }
    let body = match inbox::render(state) {
        Ok(body) => body,
        Err(error) => return error_response(error),
    };
    let badge = match layout::inbox_badge_oob(state) {
        Ok(badge) => badge,
        Err(error) => return error_response(error),
    };
    layout::fragment(&format!("{body}{badge}"))
}

/// Answer a Detail action, the same way.
fn detail_reply(state: &AppState, headers: &HeaderMap, learning_id: &str) -> Response {
    if !layout::is_htmx(headers) {
        return redirect(&format!("/learnings/{learning_id}"));
    }
    match detail::render(state, learning_id) {
        Ok(body) => layout::fragment(body.as_str()),
        Err(error) => error_response(error),
    }
}

pub async fn approve(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    match with_store(&state, |store| store.set_status(&id, Status::Active)) {
        Ok(()) => inbox_reply(&state, &headers),
        Err(error) => error_response(error),
    }
}

pub async fn reject_proposal(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    match with_store(&state, |store| store.set_status(&id, Status::Archived)) {
        Ok(()) => inbox_reply(&state, &headers),
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
    headers: HeaderMap,
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
        Ok(()) => inbox_reply(&state, &headers),
        Err(error) => error_response(error),
    }
}

pub async fn reject_finding(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let result = with_store(&state, |store| {
        let finding = store.finding(&id)?;
        let learning_id = finding.learning_id;
        store.reject_finding(&id)?;
        Ok(learning_id)
    });
    match result {
        Ok(learning_id) => detail_reply(&state, &headers, &learning_id),
        Err(error) => error_response(error),
    }
}

pub async fn open_editor(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let result = with_store(&state, |store| {
        let finding = store.finding(&id)?;
        let learning_id = finding.learning_id;
        let (Some(path), Some(line)) = (finding.path, finding.line) else {
            return Ok(learning_id);
        };
        let command = state
            .config
            .ui
            .editor_cmd
            .replace("{path}", &path)
            .replace("{line}", &line.to_string());
        spawn_editor(&command)?;
        Ok(learning_id)
    });
    match result {
        Ok(learning_id) => detail_reply(&state, &headers, &learning_id),
        Err(error) => error_response(error),
    }
}

fn spawn_editor(command: &str) -> Result<(), Error> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return Err(Error::Command {
            program: "editor".into(),
            message: "editor_cmd is empty".into(),
        });
    }
    let (program, args) = (parts[0], &parts[1..]);
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map_err(|source| Error::Command {
            program: program.into(),
            message: source.to_string(),
        })?;
    Ok(())
}

fn redirect(path: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(header::LOCATION, path.parse().unwrap());
    response
}
