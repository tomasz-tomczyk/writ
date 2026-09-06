use std::path::Path;
use std::sync::{Arc, Mutex};

use writ_cli::ui::{AppState, router};
use writ_core::{Config, ExemplarKind, NewExemplar, NewLearning, Selected, Status, Store};

async fn start_app(db: &Path, config: Config) -> String {
    let store = Store::open(db).unwrap();
    let state = AppState {
        db: db.to_path_buf(),
        config,
        store: Arc::new(Mutex::new(store)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.unwrap();
    });
    format!("http://{}", addr)
}

fn config() -> Config {
    Config::default()
}

fn record_proposed(store: &mut Store, title: &str) {
    let mut learning = NewLearning::new(title, "rule", "rationale");
    learning.status = Some(Status::Proposed);
    store.record(&learning, 0).unwrap();
}

#[tokio::test]
async fn root_redirects_to_inbox_when_proposed_exist() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        record_proposed(&mut store, "a proposal");
    }

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client.get(format!("{}/", base)).send().await.unwrap();

    assert_eq!(response.status(), 302);
    assert_eq!(response.headers()["location"], "/inbox");
}

#[tokio::test]
async fn root_redirects_to_collection_when_inbox_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client.get(format!("{}/", base)).send().await.unwrap();

    assert_eq!(response.status(), 302);
    assert_eq!(response.headers()["location"], "/collection");
}

#[tokio::test]
async fn protocol_version_header_is_present() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/collection", base))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-writ-protocol-version"], "1");
}

#[tokio::test]
async fn layout_shows_nav_and_inbox_badge() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        record_proposed(&mut store, "first");
        record_proposed(&mut store, "second");
    }

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/inbox", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("Inbox"), "{body}");
    assert!(body.contains("Collection"), "{body}");
    assert!(body.contains("Health"), "{body}");
    assert!(body.contains(">2</span>"), "badge should be 2: {body}");
}

#[tokio::test]
async fn embedded_assets_are_served() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();

    let css = client
        .get(format!("{}/assets/app.css", base))
        .send()
        .await
        .unwrap();
    assert_eq!(css.status(), 200);
    let css_type = css.headers()["content-type"].to_str().unwrap();
    assert!(css_type.contains("text/css"), "{css_type}");

    let js = client
        .get(format!("{}/assets/htmx.min.js", base))
        .send()
        .await
        .unwrap();
    assert_eq!(js.status(), 200);
    let js_type = js.headers()["content-type"].to_str().unwrap();
    assert!(js_type.contains("javascript"), "{js_type}");
    let js_body = js.text().await.unwrap();
    assert!(js_body.contains("htmx"), "{js_body}");
}

fn proposed_with_exemplars(store: &mut Store, title: &str, snippet: &str) -> String {
    let mut learning = NewLearning::new(title, "rule", "rationale");
    learning.status = Some(Status::Proposed);
    learning.exemplars = vec![NewExemplar {
        kind: ExemplarKind::Good,
        language: Some("rust".into()),
        snippet: snippet.into(),
        note: None,
    }];
    store.record(&learning, 0).unwrap().id
}

fn active_learning(store: &mut Store, title: &str) -> String {
    let mut learning = NewLearning::new(title, "rule", "rationale");
    learning.status = Some(Status::Active);
    store.record(&learning, 0).unwrap().id
}

#[tokio::test]
async fn inbox_lists_proposed_with_exemplars_and_near_matches() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "prefer sd over sed", "let x = 1;");
        proposed_with_exemplars(&mut store, "prefer ripgrep over grep", "let y = 2;");
    }

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/inbox", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("prefer sd over sed"), "{body}");
    assert!(body.contains("prefer ripgrep over grep"), "{body}");
    assert!(body.contains("let x = 1;"), "{body}");
    assert!(body.contains("Near matches"), "{body}");
    assert!(body.contains("/learnings/"), "edit link should be present: {body}");
}

#[tokio::test]
async fn approve_activates_and_removes_from_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "activate me", "s")
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/inbox/{id}/approve", base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);

    let store = Store::open(&db).unwrap();
    let learning = store.get(&id).unwrap();
    assert_eq!(learning.status, Status::Active);

    let body = client
        .get(format!("{}/inbox", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!body.contains("activate me"), "{body}");
}

#[tokio::test]
async fn reject_archives_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "reject me", "s")
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/inbox/{id}/reject", base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);

    let store = Store::open(&db).unwrap();
    assert_eq!(store.get(&id).unwrap().status, Status::Archived);
}

#[tokio::test]
async fn merge_reinforces_target_and_archives_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let (target_id, proposed_id) = {
        let mut store = Store::open(&db).unwrap();
        let target = active_learning(&mut store, "target rule");
        let proposed = proposed_with_exemplars(&mut store, "similar target rule", "merged snippet");
        (target, proposed)
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/inbox/{proposed_id}/merge", base))
        .form(&[("target_id", &target_id)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);

    let store = Store::open(&db).unwrap();
    assert_eq!(store.get(&proposed_id).unwrap().status, Status::Archived);
    let target_exemplars = store.exemplars_of(&target_id).unwrap();
    assert!(
        target_exemplars.iter().any(|e| e.snippet == "merged snippet"),
        "target should receive the proposed exemplars"
    );
}

fn bump_applied(store: &mut Store, learning_id: &str, count: usize) {
    for _ in 0..count {
        let learning = store.get(learning_id).unwrap();
        let exemplars = store.exemplars_of(learning_id).unwrap();
        let selected = vec![Selected { learning, exemplars }];
        let audit_id = store
            .start_audit("repo", "HEAD", selected.len(), &selected)
            .unwrap();
        store
            .ingest(&writ_core::FindingsInput {
                audit_id,
                findings: vec![writ_core::IncomingFinding {
                    learning_id: learning_id.into(),
                    path: None,
                    line: None,
                    detail: None,
                    outcome: writ_core::Outcome::Fixed,
                }],
            })
            .unwrap();
    }
}

#[tokio::test]
async fn collection_search_filters_by_title() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        active_learning(&mut store, "alpha rule");
        active_learning(&mut store, "beta rule");
    }

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/collection?q=alpha", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("alpha rule"), "{body}");
    assert!(!body.contains("beta rule"), "{body}");
}

#[tokio::test]
async fn collection_hides_archived_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let active_id = {
        let mut store = Store::open(&db).unwrap();
        let active = active_learning(&mut store, "still active");
        let archived = active_learning(&mut store, "now archived");
        store.set_status(&archived, Status::Archived).unwrap();
        active
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/collection", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains(&active_id), "{body}");
    assert!(!body.contains("now archived"), "{body}");
}

#[tokio::test]
async fn collection_sorts_by_hits_descending() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let high = active_learning(&mut store, "high hits");
        let low = active_learning(&mut store, "low hits");
        bump_applied(&mut store, &high, 3);
        bump_applied(&mut store, &low, 1);
    }

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/collection?sort=hit&dir=desc", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let high_pos = body.find("high hits").unwrap();
    let low_pos = body.find("low hits").unwrap();
    assert!(high_pos < low_pos, "high hits should come first: {body}");
}
