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
    store.record(&learning).unwrap();
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
    // The vendored copy shipped truncated once, and a truncated htmx is a
    // syntax error, so every swap silently falls back to a full page load.
    // The byte count is htmx 2.0.4, sha384
    // HGfztofotfshcF7+8n44JQL2oJmowVChPTg48S+jvZoztPfvwD79OC/LTtG6dMp+.
    assert_eq!(js_body.len(), 50917, "vendored htmx is not the whole file");
    assert!(
        js_body.contains(r#"version:"2.0.4""#),
        "unexpected htmx version"
    );
    assert!(
        js_body.trim_end().ends_with("return Q}();"),
        "htmx is cut short"
    );
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
    store.record(&learning).unwrap().id
}

fn active_learning(store: &mut Store, title: &str) -> String {
    let mut learning = NewLearning::new(title, "rule", "rationale");
    learning.status = Some(Status::Active);
    store.record(&learning).unwrap().id
}

fn select_learning(store: &mut Store, learning_id: &str) {
    let learning = store.get(learning_id).unwrap();
    let exemplars = store.exemplars_of(learning_id).unwrap();
    let selected = vec![Selected {
        learning,
        exemplars,
    }];
    let audit_id = store
        .start_audit("repo", "HEAD", selected.len(), &selected)
        .unwrap();
    store
        .ingest(&writ_core::FindingsInput {
            audit_id,
            findings: Vec::new(),
        })
        .unwrap();
}

fn backdate_last_selected(db: &std::path::Path, learning_id: &str) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute(
        "UPDATE learnings SET last_selected_at = '2000-01-01 00:00:00' WHERE id = ?1",
        [learning_id],
    )
    .unwrap();
}

#[tokio::test]
async fn inbox_lists_proposed_with_exemplars() {
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
    assert!(
        body.contains("/learnings/"),
        "edit link should be present: {body}"
    );
}

#[tokio::test]
async fn the_inbox_never_shows_near_matches() {
    // Spec section 7.3: nothing detects duplicates. Two proposals that
    // share every word must still produce no similarity claim.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "prefer sd over sed", "let x = 1;");
        proposed_with_exemplars(&mut store, "prefer sd over sed everywhere", "let y = 2;");
    }

    let base = start_app(&db, config()).await;
    let body = reqwest::get(format!("{}/inbox", base))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(!body.contains("Near matches"), "{body}");
    assert!(!body.contains("near-match"), "{body}");
    assert!(!body.contains("bm25"), "{body}");
}

// --- the footer names the store, spec section 9.4 ----------------------

#[tokio::test]
async fn every_page_footer_names_the_database_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();

    for page in ["/inbox", "/collection", "/health"] {
        let body = client
            .get(format!("{}{}", base, page))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            body.contains(db.to_str().unwrap()),
            "{page} footer should name the store: {body}"
        );
    }
}

#[tokio::test]
async fn detail_footer_names_the_database_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        active_learning(&mut store, "footed")
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/learnings/{id}", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains(db.to_str().unwrap()), "{body}");
}

// --- htmx: no interaction reloads the page, spec section 9.4 -----------

fn htmx_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

fn is_fragment(body: &str) -> bool {
    !body.contains("<!DOCTYPE html>") && !body.contains("<body>")
}

#[tokio::test]
async fn inbox_rows_carry_htmx_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "swap me", "s")
    };

    let base = start_app(&db, config()).await;
    let body = reqwest::get(format!("{}/inbox", base))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains(r#"id="inbox-list""#), "{body}");
    assert!(
        body.contains(&format!(r#"hx-post="/inbox/{id}/approve""#)),
        "{body}"
    );
    assert!(body.contains(r##"hx-target="#inbox-list""##), "{body}");
    // The plain form POST must survive htmx being unavailable.
    assert!(
        body.contains(&format!(r#"action="/inbox/{id}/approve""#)),
        "{body}"
    );
}

#[tokio::test]
async fn approve_over_htmx_swaps_the_list_and_the_badge() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        let id = proposed_with_exemplars(&mut store, "approve me", "s");
        proposed_with_exemplars(&mut store, "stay behind", "s2");
        id
    };

    let base = start_app(&db, config()).await;
    let response = htmx_client()
        .post(format!("{}/inbox/{id}/approve", base))
        .header("HX-Request", "true")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();

    assert!(
        is_fragment(&body),
        "a swap is a fragment, not a page: {body}"
    );
    assert!(body.contains(r#"id="inbox-list""#), "{body}");
    assert!(
        body.contains("stay behind"),
        "the other proposal stays: {body}"
    );
    // The navigation count moves with the row, out of band.
    assert!(body.contains(r#"id="inbox-badge""#), "{body}");
    assert!(body.contains("hx-swap-oob"), "{body}");
    assert!(
        body.contains(">1</span>"),
        "badge should now read 1: {body}"
    );
}

#[tokio::test]
async fn approve_without_htmx_still_redirects() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "no js here", "s")
    };

    let base = start_app(&db, config()).await;
    let response = htmx_client()
        .post(format!("{}/inbox/{id}/approve", base))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 303);
    assert_eq!(response.headers()["location"], "/inbox");
    let store = Store::open(&db).unwrap();
    assert_eq!(store.get(&id).unwrap().status, Status::Active);
}

#[tokio::test]
async fn merge_over_htmx_swaps_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let (target_id, proposed_id) = {
        let mut store = Store::open(&db).unwrap();
        let target = active_learning(&mut store, "target rule");
        let proposed = proposed_with_exemplars(&mut store, "similar target rule", "merged snippet");
        (target, proposed)
    };

    let base = start_app(&db, config()).await;
    let response = htmx_client()
        .post(format!("{}/inbox/{proposed_id}/merge", base))
        .header("HX-Request", "true")
        .form(&[("target_id", &target_id)])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert!(is_fragment(&body), "{body}");
    assert!(body.contains(r#"id="inbox-list""#), "{body}");
}

#[tokio::test]
async fn health_archive_over_htmx_swaps_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        let mut learning = NewLearning::new("archive me over htmx", "rule", "rationale");
        learning.status = Some(Status::Active);
        learning.created_at = Some("2000-01-01 00:00:00".into());
        store.record(&learning).unwrap().id
    };

    let base = start_app(&db, config()).await;
    let page = reqwest::get(format!("{}/health", base))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains(r#"id="health-body""#), "{page}");
    assert!(
        page.contains(&format!(r#"hx-post="/learnings/{id}/archive""#)),
        "{page}"
    );
    assert!(
        page.contains(r#"hx-get="/health""#),
        "keep is a swap too: {page}"
    );

    let response = htmx_client()
        .post(format!("{}/learnings/{id}/archive", base))
        .header("HX-Request", "true")
        .header("HX-Target", "health-body")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert!(is_fragment(&body), "{body}");
    assert!(body.contains(r#"id="health-body""#), "{body}");
    assert!(!body.contains("archive me over htmx"), "{body}");
}

#[tokio::test]
async fn collection_sort_and_search_over_htmx_return_a_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        active_learning(&mut store, "alpha rule");
        active_learning(&mut store, "beta rule");
    }

    let base = start_app(&db, config()).await;
    let page = reqwest::get(format!("{}/collection", base))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains(r#"id="collection-body""#), "{page}");
    assert!(page.contains(r#"hx-get="/collection"#), "{page}");
    assert!(page.contains(r#"hx-push-url="true""#), "{page}");

    let body = htmx_client()
        .get(format!("{}/collection?sort=hit&dir=desc", base))
        .header("HX-Request", "true")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(is_fragment(&body), "{body}");
    assert!(body.contains(r#"id="collection-body""#), "{body}");
    assert!(body.contains("alpha rule"), "{body}");

    let searched = htmx_client()
        .get(format!("{}/collection?q=alpha", base))
        .header("HX-Request", "true")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(is_fragment(&searched), "{searched}");
    assert!(searched.contains("alpha rule"), "{searched}");
    assert!(!searched.contains("beta rule"), "{searched}");
}

#[tokio::test]
async fn detail_save_over_htmx_returns_the_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        active_with_exemplar(&mut store, "editable over htmx")
    };

    let base = start_app(&db, config()).await;
    let page = reqwest::get(format!("{}/learnings/{id}", base))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains(r#"id="detail-body""#), "{page}");
    assert!(
        page.contains(&format!(r#"hx-post="/learnings/{id}""#)),
        "{page}"
    );

    let response = htmx_client()
        .post(format!("{}/learnings/{id}", base))
        .header("HX-Request", "true")
        .form(&[
            ("title", "editable over htmx"),
            ("rule", "swapped rule"),
            ("rationale", "swapped rationale"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert!(is_fragment(&body), "{body}");
    assert!(body.contains(r#"id="detail-body""#), "{body}");
    assert!(body.contains("swapped rule"), "{body}");

    let store = Store::open(&db).unwrap();
    assert_eq!(store.get(&id).unwrap().rule, "swapped rule");
}

#[tokio::test]
async fn finding_reject_over_htmx_returns_the_detail_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let (learning_id, finding_id) = {
        let mut store = Store::open(&db).unwrap();
        let id = active_with_exemplar(&mut store, "finding owner");
        let finding_id = finding_with_path(&mut store, &id, Some("a.rs"), Some(3));
        (id, finding_id)
    };

    let base = start_app(&db, config()).await;
    let page = reqwest::get(format!("{}/learnings/{learning_id}", base))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains(&format!(r#"hx-post="/findings/{finding_id}/reject""#)),
        "{page}"
    );

    let response = htmx_client()
        .post(format!("{}/findings/{finding_id}/reject", base))
        .header("HX-Request", "true")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert!(is_fragment(&body), "{body}");
    assert!(body.contains(r#"id="detail-body""#), "{body}");
    assert!(body.contains("rejected"), "{body}");
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
        target_exemplars
            .iter()
            .any(|e| e.snippet == "merged snippet"),
        "target should receive the proposed exemplars"
    );
}

fn bump_applied(store: &mut Store, learning_id: &str, count: usize) {
    for _ in 0..count {
        finding_with_path(store, learning_id, None, None);
    }
}

fn finding_with_path(
    store: &mut Store,
    learning_id: &str,
    path: Option<&str>,
    line: Option<i64>,
) -> String {
    let learning = store.get(learning_id).unwrap();
    let exemplars = store.exemplars_of(learning_id).unwrap();
    let selected = vec![Selected {
        learning,
        exemplars,
    }];
    let audit_id = store
        .start_audit("repo", "HEAD", selected.len(), &selected)
        .unwrap();
    store
        .ingest(&writ_core::FindingsInput {
            audit_id,
            findings: vec![writ_core::IncomingFinding {
                learning_id: learning_id.into(),
                path: path.map(Into::into),
                line,
                detail: None,
                outcome: writ_core::Outcome::Open,
            }],
        })
        .unwrap();
    let findings = store.findings_of(learning_id).unwrap();
    findings.last().unwrap().id.clone()
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

fn active_with_exemplar(store: &mut Store, title: &str) -> String {
    let mut learning = NewLearning::new(title, "rule", "rationale");
    learning.status = Some(Status::Active);
    learning.exemplars = vec![NewExemplar {
        kind: ExemplarKind::Good,
        language: Some("rust".into()),
        snippet: "let good = true;".into(),
        note: None,
    }];
    store.record(&learning).unwrap().id
}

#[tokio::test]
async fn detail_save_updates_rule_text() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        active_with_exemplar(&mut store, "editable")
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/learnings/{id}", base))
        .form(&[
            ("title", "editable"),
            ("rule", "updated rule"),
            ("rationale", "updated rationale"),
            ("good_snippet", "let good = true;"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);
    assert_eq!(response.headers()["location"], format!("/learnings/{id}"));

    let store = Store::open(&db).unwrap();
    let learning = store.get(&id).unwrap();
    assert_eq!(learning.rule, "updated rule");
    assert_eq!(learning.rationale, "updated rationale");
}

#[tokio::test]
async fn reject_finding_from_detail_sets_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let (learning_id, finding_id) = {
        let mut store = Store::open(&db).unwrap();
        let id = active_with_exemplar(&mut store, "finding owner");
        let finding_id = finding_with_path(&mut store, &id, Some("a.rs"), Some(3));
        (id, finding_id)
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/findings/{finding_id}/reject", base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);
    assert_eq!(
        response.headers()["location"],
        format!("/learnings/{learning_id}")
    );

    let store = Store::open(&db).unwrap();
    let finding = store.findings_of(&learning_id).unwrap().pop().unwrap();
    assert_eq!(finding.outcome, writ_core::Outcome::Rejected);
}

#[tokio::test]
async fn open_editor_runs_configured_command() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let target_file = dir.path().join("source.rs");
    std::fs::write(&target_file, "fn main() {}").unwrap();
    let marker = dir.path().join("source.rs.opened");
    let config = {
        let mut c = Config::default();
        c.ui.editor_cmd = format!("touch {}", marker.display());
        c
    };

    let (learning_id, finding_id) = {
        let mut store = Store::open(&db).unwrap();
        let id = active_with_exemplar(&mut store, "open editor");
        let finding_id = finding_with_path(
            &mut store,
            &id,
            Some(target_file.to_str().unwrap()),
            Some(12),
        );
        (id, finding_id)
    };

    let base = start_app(&db, config).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/findings/{finding_id}/open", base))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        303,
        "body: {}",
        response.text().await.unwrap()
    );
    assert_eq!(
        response.headers()["location"],
        format!("/learnings/{learning_id}")
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        marker.exists(),
        "editor command should create marker file at {}",
        marker.display()
    );
}

#[tokio::test]
async fn health_lists_unused_rules() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut learning = NewLearning::new("old and unused", "rule", "rationale");
        learning.status = Some(Status::Active);
        learning.created_at = Some("2000-01-01 00:00:00".into());
        store.record(&learning).unwrap();
    }

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/health", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("old and unused"), "{body}");
    assert!(body.contains("Not selected in 90 days"), "{body}");
}

#[tokio::test]
async fn health_lists_never_applied_rules() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let id = active_learning(&mut store, "selected but silent");
        select_learning(&mut store, &id);
    }

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/health", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("selected but silent"), "{body}");
    assert!(body.contains("Selected but never applied"), "{body}");
}

#[tokio::test]
async fn health_marks_rows_in_both_buckets() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        let id = active_learning(&mut store, "in both buckets");
        select_learning(&mut store, &id);
        id
    };
    backdate_last_selected(&db, &id);

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/health", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("in both buckets"), "{body}");
    assert!(body.contains("in-both"), "{body}");
}

#[tokio::test]
async fn inbox_empty_state_shows_curation_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

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

    assert!(body.contains("Inbox is empty"), "{body}");
    assert!(body.contains("Every proposal is curated"), "{body}");
}

#[tokio::test]
async fn health_empty_state_shows_clear_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::new();
    let body = client
        .get(format!("{}/health", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("Health is clear"), "{body}");
}

#[tokio::test]
async fn health_archive_removes_from_health() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        let mut learning = NewLearning::new("archive me", "rule", "rationale");
        learning.status = Some(Status::Active);
        learning.created_at = Some("2000-01-01 00:00:00".into());
        store.record(&learning).unwrap().id
    };

    let base = start_app(&db, config()).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/learnings/{id}/archive", base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);

    let body = client
        .get(format!("{}/health", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!body.contains("archive me"), "{body}");
}
