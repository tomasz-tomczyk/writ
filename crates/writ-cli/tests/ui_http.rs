use std::path::Path;
use std::sync::{Arc, Mutex};

use writ_cli::ui::{AppState, router};
use writ_core::{Config, NewLearning, Status, Store};

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
