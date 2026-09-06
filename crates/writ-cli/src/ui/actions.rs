use axum::Form;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use writ_core::{Error, Status};

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

pub async fn reject_finding(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    let result = with_store(&state, |store| {
        let finding = store.finding(&id)?;
        let learning_id = finding.learning_id;
        store.reject_finding(&id)?;
        Ok(learning_id)
    });
    match result {
        Ok(learning_id) => redirect(&format!("/learnings/{learning_id}")),
        Err(error) => error_response(error),
    }
}

pub async fn open_editor(Path(id): Path<String>, State(state): State<AppState>) -> Response {
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
        Ok(learning_id) => redirect(&format!("/learnings/{learning_id}")),
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
