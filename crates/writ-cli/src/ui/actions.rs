use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use writ_core::{
    CounterMetric, Error, FindingOutcomeMetric, HealthActionMetric, Outcome, Status, SurfaceMetric,
    TelemetryBatch,
};

use crate::ui::AppState;
use crate::ui::pages::layout::{error_response, with_store};
use crate::ui::pages::{collection, detail, layout};

/// Answer an Inbox action.
///
/// htmx gets the list fragment plus an out-of-band badge, so the row and
/// the navigation count move together. Anything else gets the redirect a
/// plain form POST expects, so the page still works without htmx.
fn inbox_reply(state: &AppState, headers: &HeaderMap, notice: &str) -> Response {
    if !layout::is_htmx(headers) {
        return redirect("/collection?view=review");
    }
    let body = match collection::render_with_notice(
        state,
        Some("review"),
        None,
        None,
        None,
        None,
        &[],
        Some(notice),
    ) {
        Ok(body) => body,
        Err(error) => return error_response(error),
    };
    let badge = match layout::review_badge_oob(state) {
        Ok(badge) => badge,
        Err(error) => return error_response(error),
    };
    layout::fragment(&format!("{body}{badge}"))
}

/// Answer a finding rejection without replacing the surrounding Detail form.
fn finding_reply(
    state: &AppState,
    headers: &HeaderMap,
    finding_id: &str,
    learning_id: &str,
    message: &str,
) -> Response {
    if !layout::is_htmx(headers) {
        return redirect(&format!("/learnings/{learning_id}"));
    }
    match detail::render_finding(state, finding_id) {
        Ok(body) => layout::fragment(&format!("{body}{}", layout::action_feedback_oob(message))),
        Err(error) => error_response(error),
    }
}

/// Answer a Health action without regressing the htmx fragment contract.
fn health_reply(state: &AppState, headers: &HeaderMap) -> Response {
    if !layout::is_htmx(headers) {
        return redirect("/collection?view=needs-attention");
    }
    match collection::render(state, Some("needs-attention"), None, None, None, None, &[]) {
        Ok(body) => layout::fragment(body.as_str()),
        Err(error) => error_response(error),
    }
}

pub async fn approve(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let result = with_store(&state, |store| {
        let title = store.get(&id)?.title;
        store.set_status(&id, Status::Active)?;
        Ok(title)
    });
    match result {
        Ok(title) => inbox_reply(&state, &headers, &format!("Activated “{title}”")),
        Err(error) => error_response(error),
    }
}

pub async fn reject_proposal(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let result = with_store(&state, |store| {
        let title = store.get(&id)?.title;
        store.set_status(&id, Status::Archived)?;
        Ok(title)
    });
    match result {
        Ok(title) => inbox_reply(
            &state,
            &headers,
            &format!("Archived proposal “{title}”; history preserved"),
        ),
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
        let changed = finding.outcome != Outcome::Rejected;
        if changed {
            store.reject_finding(&id)?;
        }
        Ok((learning_id, changed))
    });
    match result {
        Ok((learning_id, changed)) => {
            let message = if changed {
                observe_ui(
                    &state,
                    CounterMetric::FindingOutcome(FindingOutcomeMetric::Rejected),
                );
                "Marked as not a violation."
            } else {
                "Already marked as not a violation."
            };
            finding_reply(&state, &headers, &id, &learning_id, message)
        }
        Err(error) => error_response(error),
    }
}

pub async fn health_archive(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    match with_store(&state, |store| store.set_status(&id, Status::Archived)) {
        Ok(()) => {
            observe_ui(
                &state,
                CounterMetric::HealthAction(HealthActionMetric::Archive),
            );
            health_reply(&state, &headers)
        }
        Err(error) => error_response(error),
    }
}

pub async fn health_edit(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    match with_store(&state, |store| store.get(&id).map(|_| ())) {
        Ok(()) => {
            observe_ui(
                &state,
                CounterMetric::HealthAction(HealthActionMetric::Edit),
            );
            redirect(&format!("/learnings/{id}?from=needs-attention"))
        }
        Err(error) => error_response(error),
    }
}

pub async fn health_keep(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    match with_store(&state, |store| store.get(&id).map(|_| ())) {
        Ok(()) => {
            observe_ui(
                &state,
                CounterMetric::HealthAction(HealthActionMetric::Keep),
            );
            health_reply(&state, &headers)
        }
        Err(error) => error_response(error),
    }
}

fn observe_ui(state: &AppState, metric: CounterMetric) {
    if !state.config.telemetry.enabled {
        return;
    }
    let batch = TelemetryBatch {
        counters: vec![CounterMetric::Surface(SurfaceMetric::Ui), metric],
        buckets: Vec::new(),
    };
    crate::telemetry::record_best_effort(&state.telemetry_db, &batch);
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
            return Err(Error::Validation {
                message: "Cannot open this finding: both path and line are required".into(),
            });
        };
        let command = state
            .config
            .ui
            .editor_cmd
            .replace("{path}", &path)
            .replace("{line}", &line.to_string());
        spawn_editor(&command)?;
        Ok((learning_id, path, line))
    });
    match result {
        Ok((learning_id, _, _)) if !layout::is_htmx(&headers) => {
            redirect(&format!("/learnings/{learning_id}"))
        }
        Ok((_, path, line)) => layout::fragment(&layout::action_feedback_oob(&format!(
            "Sent {path}:{line} to the editor."
        ))),
        Err(Error::Validation { message }) => {
            layout::action_error(StatusCode::BAD_REQUEST, &message)
        }
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use writ_core::{
        AuditScope, Config, Diff, FindingsInput, IncomingFinding, NewLearning, Outcome,
        RepoIdentity, Selected, Store, TelemetryStore,
    };

    use super::*;

    fn state(db: &std::path::Path, telemetry_enabled: bool) -> AppState {
        let mut config = Config::default();
        config.telemetry.enabled = telemetry_enabled;
        let telemetry_db = db.with_file_name("telemetry.db");
        if telemetry_enabled {
            let mut telemetry = TelemetryStore::open(&telemetry_db).unwrap();
            telemetry.enable().unwrap();
        }
        AppState {
            db: db.to_path_buf(),
            telemetry_db,
            config,
            store: Arc::new(Mutex::new(Store::open(db).unwrap())),
        }
    }

    fn active(store: &mut Store, title: &str) -> String {
        let mut learning = NewLearning::new(title, "rule", "rationale");
        learning.status = Some(Status::Active);
        store.record(&learning).unwrap().id
    }

    #[tokio::test]
    async fn health_actions_record_fixed_labels_and_ui_surface() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("learnings.db");
        let state = state(&db, true);
        let (archive_id, edit_id, keep_id) = {
            let mut store = state.store.lock().unwrap();
            (
                active(&mut store, "archive"),
                active(&mut store, "edit"),
                active(&mut store, "keep"),
            )
        };

        assert_eq!(
            health_archive(Path(archive_id), State(state.clone()), HeaderMap::new())
                .await
                .status(),
            StatusCode::SEE_OTHER
        );
        assert_eq!(
            health_edit(Path(edit_id), State(state.clone()))
                .await
                .status(),
            StatusCode::SEE_OTHER
        );
        assert_eq!(
            health_keep(Path(keep_id), State(state.clone()), HeaderMap::new())
                .await
                .status(),
            StatusCode::SEE_OTHER
        );

        let telemetry = TelemetryStore::open(&state.telemetry_db).unwrap();
        let rows = telemetry.counters().unwrap();
        for action in ["archive", "edit", "keep"] {
            assert!(rows.iter().any(|row| {
                row.metric == "health_action" && row.label == action && row.count == 1
            }));
        }
        assert!(
            rows.iter()
                .any(|row| row.metric == "surface" && row.label == "ui" && row.count == 3)
        );
    }

    #[tokio::test]
    async fn developer_rejection_records_no_finding_content() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("learnings.db");
        let state = state(&db, true);
        let finding_id = {
            let mut store = state.store.lock().unwrap();
            let learning_id = active(&mut store, "private title");
            let learning = store.get(&learning_id).unwrap();
            let audit_id = store
                .start_audit(
                    &AuditScope {
                        identity: RepoIdentity::Remote("private repository".into()),
                        diff: Diff::parse("--- a/private.rs\n+++ b/private.rs\n+private line\n"),
                        diff_range: "private branch".into(),
                        diff_digest: "private digest".into(),
                    },
                    1,
                    &[Selected {
                        learning,
                        exemplars: Vec::new(),
                    }],
                )
                .unwrap();
            store
                .ingest(&FindingsInput {
                    audit_id,
                    findings: vec![IncomingFinding {
                        learning_id: learning_id.clone(),
                        path: Some("private/path.rs".into()),
                        line: Some(17),
                        detail: Some("private detail".into()),
                        outcome: Outcome::Open,
                    }],
                })
                .unwrap();
            store.findings_of(&learning_id).unwrap()[0].id.clone()
        };

        assert_eq!(
            reject_finding(
                Path(finding_id.clone()),
                State(state.clone()),
                HeaderMap::new(),
            )
            .await
            .status(),
            StatusCode::SEE_OTHER
        );
        let rows = TelemetryStore::open(&state.telemetry_db)
            .unwrap()
            .counters()
            .unwrap();
        assert!(rows.iter().any(|row| {
            row.metric == "finding_outcome" && row.label == "rejected" && row.count == 1
        }));
        let json = serde_json::to_string(&rows).unwrap();
        for sentinel in [
            "private title",
            "private repository",
            "private branch",
            "private/path.rs",
            "private detail",
            &finding_id,
        ] {
            assert!(!json.contains(sentinel), "leaked {sentinel}: {json}");
        }
    }

    #[tokio::test]
    async fn broken_ui_telemetry_does_not_change_the_action() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("learnings.db");
        let state = state(&db, true);
        let id = active(
            &mut state.store.lock().unwrap(),
            "archive despite telemetry",
        );
        std::fs::remove_file(&state.telemetry_db).unwrap();
        std::fs::create_dir(&state.telemetry_db).unwrap();

        let response =
            health_archive(Path(id.clone()), State(state.clone()), HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            state.store.lock().unwrap().get(&id).unwrap().status,
            Status::Archived
        );
    }
}
