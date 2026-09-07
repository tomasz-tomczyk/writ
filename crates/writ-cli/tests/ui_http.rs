use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use tower::ServiceExt;
use writ_cli::ui::{AppState, router};
use writ_core::{Config, ExemplarKind, NewExemplar, NewLearning, Selected, Status, Store};

const TEST_HOST: &str = "writ.local:4173";
const TEST_ORIGIN: &str = "http://writ.local:4173";

fn start_app(db: &Path, config: Config) -> Router {
    let store = Store::open(db).unwrap();
    let state = AppState {
        db: db.to_path_buf(),
        telemetry_db: db.with_file_name("telemetry.db"),
        config,
        store: Arc::new(Mutex::new(store)),
    };
    router(state)
}

struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

async fn request_kind(
    app: &Router,
    method: Method,
    uri: &str,
    form: Option<&[(&str, &str)]>,
    htmx: bool,
) -> TestResponse {
    request_kind_with_origin(app, method, uri, form, htmx, Some(TEST_ORIGIN)).await
}

async fn request_kind_with_origin(
    app: &Router,
    method: Method,
    uri: &str,
    form: Option<&[(&str, &str)]>,
    htmx: bool,
    origin: Option<&str>,
) -> TestResponse {
    let body = form
        .map(|fields| serde_urlencoded::to_string(fields).unwrap())
        .unwrap_or_default();
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, TEST_HOST);
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    if form.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    if htmx {
        builder = builder.header("HX-Request", "true");
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    TestResponse {
        status,
        headers,
        body,
    }
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    form: Option<&[(&str, &str)]>,
) -> TestResponse {
    request_kind(app, method, uri, form, false).await
}

async fn htmx(
    app: &Router,
    method: Method,
    uri: &str,
    form: Option<&[(&str, &str)]>,
) -> TestResponse {
    request_kind(app, method, uri, form, true).await
}

async fn get(app: &Router, uri: &str) -> TestResponse {
    request(app, Method::GET, uri, None).await
}

async fn post(app: &Router, uri: &str) -> TestResponse {
    request(app, Method::POST, uri, None).await
}

async fn post_form(app: &Router, uri: &str, form: &[(&str, &str)]) -> TestResponse {
    request(app, Method::POST, uri, Some(form)).await
}

async fn foreign_post(app: &Router, uri: &str, form: Option<&[(&str, &str)]>) -> TestResponse {
    request_kind_with_origin(
        app,
        Method::POST,
        uri,
        form,
        false,
        Some("https://attacker.example"),
    )
    .await
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

    let app = start_app(&db, config());
    let response = get(&app, "/").await;

    assert_eq!(response.status, 302);
    assert_eq!(response.headers["location"], "/inbox");
}

#[tokio::test]
async fn root_redirects_to_collection_when_inbox_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let app = start_app(&db, config());
    let response = get(&app, "/").await;

    assert_eq!(response.status, 302);
    assert_eq!(response.headers["location"], "/collection");
}

#[tokio::test]
async fn protocol_version_header_is_present() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let app = start_app(&db, config());
    let response = get(&app, "/collection").await;

    assert_eq!(response.status, 200);
    assert_eq!(response.headers["x-writ-protocol-version"], "1");
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

    let app = start_app(&db, config());
    let body = get(&app, "/inbox").await.body;

    assert!(body.contains("Inbox"), "{body}");
    assert!(body.contains("Collection"), "{body}");
    assert!(body.contains("Health"), "{body}");
    assert!(body.contains(">2</span>"), "badge should be 2: {body}");
}

#[tokio::test]
async fn embedded_assets_are_served() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let app = start_app(&db, config());

    let css = get(&app, "/assets/app.css").await;
    assert_eq!(css.status, 200);
    let css_type = css.headers["content-type"].to_str().unwrap();
    assert!(css_type.contains("text/css"), "{css_type}");

    let js = get(&app, "/assets/htmx.min.js").await;
    assert_eq!(js.status, 200);
    let js_type = js.headers["content-type"].to_str().unwrap();
    assert!(js_type.contains("javascript"), "{js_type}");
    assert!(js.body.contains("htmx"), "{}", js.body);
    assert_eq!(js.body.len(), 50917, "vendored htmx is not the whole file");
    assert!(js.body.contains(r#"version:"2.0.4""#));
    assert!(js.body.trim_end().ends_with("return Q}();"));
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
async fn inbox_lists_proposed_without_near_matches() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "prefer sd over sed", "let x = 1;");
        proposed_with_exemplars(&mut store, "prefer sd over sed everywhere", "let y = 2;");
    }

    let app = start_app(&db, config());
    let body = get(&app, "/inbox").await.body;

    assert!(body.contains("prefer sd over sed"), "{body}");
    assert!(body.contains("prefer sd over sed everywhere"), "{body}");
    assert!(body.contains("let x = 1;"), "{body}");
    assert!(!body.contains("Near matches"), "{body}");
    assert!(!body.contains("near-match"), "{body}");
    assert!(!body.contains("bm25"), "{body}");
    assert!(!body.contains("Merge into"), "{body}");
    assert!(
        body.contains("/learnings/"),
        "edit link should be present: {body}"
    );
}

fn is_fragment(body: &str) -> bool {
    !body.contains("<!DOCTYPE html>") && !body.contains("<body>")
}

#[tokio::test]
async fn main_htmx_inbox_and_health_contracts_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let (approve_id, archive_id) = {
        let mut store = Store::open(&db).unwrap();
        let approve_id = proposed_with_exemplars(&mut store, "approve over htmx", "s");
        proposed_with_exemplars(&mut store, "stay behind", "s2");
        let mut old = NewLearning::new("archive over htmx", "rule", "rationale");
        old.status = Some(Status::Active);
        old.created_at = Some("2000-01-01 00:00:00".into());
        let archive_id = store.record(&old).unwrap().id;
        (approve_id, archive_id)
    };

    let app = start_app(&db, config());
    let inbox = get(&app, "/inbox").await.body;
    assert!(
        inbox.contains(&format!(r#"hx-post="/inbox/{approve_id}/approve""#)),
        "{inbox}"
    );
    let approved = htmx(
        &app,
        Method::POST,
        &format!("/inbox/{approve_id}/approve"),
        None,
    )
    .await;
    assert_eq!(approved.status, 200);
    assert!(is_fragment(&approved.body), "{}", approved.body);
    assert!(approved.body.contains(r#"id="inbox-list""#));
    assert!(approved.body.contains("hx-swap-oob"));

    let health = get(&app, "/health").await.body;
    assert!(
        health.contains(&format!(r#"hx-post="/health/{archive_id}/archive""#)),
        "{health}"
    );
    assert!(
        health.contains(&format!(r#"hx-get="/health/{archive_id}/keep""#)),
        "{health}"
    );
    let archived = htmx(
        &app,
        Method::POST,
        &format!("/health/{archive_id}/archive"),
        None,
    )
    .await;
    assert_eq!(archived.status, 200);
    assert!(is_fragment(&archived.body), "{}", archived.body);
    assert!(archived.body.contains(r#"id="health-body""#));
    assert!(!archived.body.contains("archive over htmx"));
}

#[tokio::test]
async fn main_htmx_detail_collection_contracts_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let (learning_id, finding_id) = {
        let mut store = Store::open(&db).unwrap();
        let learning_id = active_with_exemplar(&mut store, "editable over htmx");
        let finding_id = finding_with_path(&mut store, &learning_id, Some("a.rs"), Some(3));
        (learning_id, finding_id)
    };
    let app = start_app(&db, config());

    let collection = htmx(
        &app,
        Method::GET,
        "/collection?sort=hit&dir=desc&q=editable",
        None,
    )
    .await;
    assert_eq!(collection.status, 200);
    assert!(is_fragment(&collection.body), "{}", collection.body);
    assert!(collection.body.contains(r#"id="collection-body""#));

    let saved = htmx(
        &app,
        Method::POST,
        &format!("/learnings/{learning_id}"),
        Some(&[
            ("title", "editable over htmx"),
            ("rule", "swapped rule"),
            ("rationale", "swapped rationale"),
        ]),
    )
    .await;
    assert_eq!(saved.status, 200);
    assert!(is_fragment(&saved.body), "{}", saved.body);
    assert!(saved.body.contains(r#"id="detail-body""#));
    assert!(saved.body.contains("swapped rule"));

    let rejected = htmx(
        &app,
        Method::POST,
        &format!("/findings/{finding_id}/reject"),
        None,
    )
    .await;
    assert_eq!(rejected.status, 200);
    assert!(is_fragment(&rejected.body), "{}", rejected.body);
    assert!(rejected.body.contains(r#"id="detail-body""#));
    assert!(rejected.body.contains("rejected"));
}

#[tokio::test]
async fn approve_activates_and_removes_from_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "activate me", "s")
    };

    let app = start_app(&db, config());
    let response = post(&app, &format!("/inbox/{id}/approve")).await;
    assert_eq!(response.status, 303);

    let store = Store::open(&db).unwrap();
    let learning = store.get(&id).unwrap();
    assert_eq!(learning.status, Status::Active);

    let body = get(&app, "/inbox").await.body;
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

    let app = start_app(&db, config());
    let response = post(&app, &format!("/inbox/{id}/reject")).await;
    assert_eq!(response.status, 303);

    let store = Store::open(&db).unwrap();
    assert_eq!(store.get(&id).unwrap().status, Status::Archived);
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

    let app = start_app(&db, config());
    let body = get(&app, "/collection?q=alpha").await.body;

    assert!(body.contains("alpha rule"), "{body}");
    assert!(!body.contains("beta rule"), "{body}");
}

#[tokio::test]
async fn collection_uses_project_scope_and_mode_columns() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();

        let mut blocking = NewLearning::new("project rule", "rule", "rationale");
        blocking.status = Some(Status::Active);
        blocking.scopes = vec![
            "project:github.com/acme/writ".parse().unwrap(),
            "language:rust".parse().unwrap(),
            "glob:crates/**/*.rs".parse().unwrap(),
        ];
        blocking.matcher_kind = Some(writ_core::MatcherKind::Regex);
        blocking.matcher = Some("collection-only-secret".into());
        store.record(&blocking).unwrap();

        let mut advisory = NewLearning::new("global rule", "rule", "rationale");
        advisory.status = Some(Status::Active);
        advisory.blocking = false;
        advisory.scopes = vec!["global".parse().unwrap()];
        store.record(&advisory).unwrap();
    }

    let app = start_app(&db, config());
    let body = get(&app, "/collection").await.body;

    assert!(body.contains("Project"), "{body}");
    assert!(body.contains(r#"class="project-filter""#), "{body}");
    assert!(!body.contains(">Scope</th>"), "{body}");
    assert!(body.contains(">Mode</th>"), "{body}");
    assert!(!body.contains(">Matcher</th>"), "{body}");
    assert!(!body.contains("collection-only-secret"), "{body}");
    assert!(
        body.contains(
            r#"<th scope="col" aria-sort="ascending"><a class="sort-button" data-direction="asc""#
        ),
        "the default title order should be announced: {body}"
    );
    assert!(
        body.contains(r#">writ</span>"#),
        "project chips should show only the repo leaf: {body}"
    );
    assert!(
        !body.contains(r#"<span class="scope-kind">project:</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"title="project:github.com/acme/writ""#),
        "full project identity stays on the title attribute: {body}"
    );
    assert!(
        body.contains(r#"class="title-meta""#),
        "non-project scopes belong under the title: {body}"
    );
    assert!(
        body.contains(r#"<span class="scope-kind">language:</span>rust"#),
        "{body}"
    );
    assert!(
        body.contains(r#"<span class="scope-kind">glob:</span>crates/**/*.rs"#),
        "{body}"
    );
    assert!(
        body.contains(r#"class="mode-badge blocking">blocking</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"class="mode-badge advisory">advisory</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"class="muted empty-value">—</span>"#),
        "{body}"
    );
}

#[tokio::test]
async fn collection_uses_attached_ledger_table_chrome() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        active_learning(&mut store, "ledger row");
    }

    let app = start_app(&db, config());
    let body = get(&app, "/collection?status=all&sort=title&dir=asc")
        .await
        .body;

    assert!(
        body.contains(r#"<section class="collection-ledger"><div class="collection-controls">"#),
        "controls should share a ledger wrapper with the table: {body}"
    );
    assert!(
        body.contains(r#"class="filter filter-chip" aria-current="true""#),
        "{body}"
    );
    assert!(
        body.contains(r#"class="result-count">1 learning"#),
        "{body}"
    );
    assert!(
        body.contains(
            r#"<colgroup><col class="title"><col class="project"><col class="mode"><col class="hits"><col class="used"><col class="status"></colgroup>"#
        ),
        "{body}"
    );
    assert!(!body.contains(r#"col class="health""#), "{body}");
    assert!(!body.contains(">Health</th>"), "{body}");
    assert!(!body.contains("health-cell"), "{body}");
    assert!(!body.contains("health-dot"), "{body}");
    assert!(
        body.contains(
            r##"<form method="get" action="/collection" hx-get="/collection" hx-target="#collection-body" hx-swap="outerHTML" hx-push-url="true" class="search search-form""##
        ),
        "{body}"
    );
    assert!(
        body.contains(
            r##"hx-get="/collection?status=all&amp;sort=title&amp;dir=asc" hx-target="#collection-body" hx-swap="outerHTML" hx-push-url="true" class="filter filter-chip" aria-current="true""##
        ),
        "{body}"
    );
    assert!(
        body.contains(r#"class="sort-button" data-direction="asc""#),
        "{body}"
    );
    assert!(body.contains(r#"aria-sort="ascending""#), "{body}");
    assert!(
        body.contains(
            r##"hx-get="/collection?sort=title&amp;dir=desc&amp;status=all" hx-target="#collection-body" hx-swap="outerHTML" hx-push-url="true""##
        ),
        "{body}"
    );
    assert!(body.contains(r#"class="sort-caret""#), "{body}");
    assert!(body.contains(r#"class="table-shell""#), "{body}");
}

#[tokio::test]
async fn collection_sort_controls_escape_query_values() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        active_learning(&mut store, r#"" onfocus="alert(2)"#);
    }

    let app = start_app(&db, config());
    let body = get(
        &app,
        "/collection?sort=title&dir=%22%20onmouseover%3D%22alert(1)&q=%22%20onfocus%3D%22alert(2)",
    )
    .await
    .body;

    assert!(
        body.contains(r#"class="sort-button" data-direction="asc""#),
        "invalid directions should normalize to ascending: {body}"
    );
    assert!(
        body.contains("q=%22+onfocus%3D%22alert%282%29"),
        "search query should remain URL-encoded in sort links: {body}"
    );
    assert!(
        !body.contains(r#"data-direction="" onmouseover="#),
        "{body}"
    );
    assert!(!body.contains(r#"&q=" onfocus="#), "{body}");
}

#[tokio::test]
async fn empty_collection_has_no_record_or_new_learning_cta() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let app = start_app(&db, config());
    let body = get(&app, "/collection").await.body;

    assert!(body.contains("No learnings match"), "{body}");
    assert!(!body.contains("Record"), "{body}");
    assert!(!body.contains("record your"), "{body}");
    assert!(!body.contains("New learning"), "{body}");
    assert!(!body.contains("new learning"), "{body}");
}

#[tokio::test]
async fn collection_defaults_to_all_including_archived() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let active_id = {
        let mut store = Store::open(&db).unwrap();
        let active = active_learning(&mut store, "still active");
        let archived = active_learning(&mut store, "now archived");
        store.set_status(&archived, Status::Archived).unwrap();
        active
    };

    let app = start_app(&db, config());
    let body = get(&app, "/collection").await.body;

    assert!(body.contains(&active_id), "{body}");
    assert!(body.contains("now archived"), "{body}");
    assert!(
        body.contains(
            r##"hx-get="/collection?status=all&amp;sort=title&amp;dir=asc" hx-target="#collection-body" hx-swap="outerHTML" hx-push-url="true" class="filter filter-chip" aria-current="true""##
        ),
        "page load should highlight All like the github mockup: {body}"
    );
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

    let app = start_app(&db, config());
    let body = get(&app, "/collection?sort=hit&dir=desc").await.body;

    let high_pos = body.find("high hits").unwrap();
    let low_pos = body.find("low hits").unwrap();
    assert!(high_pos < low_pos, "high hits should come first: {body}");
}

#[tokio::test]
async fn collection_sorts_by_project() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut zebra = NewLearning::new("zebra title", "rule", "rationale");
        zebra.status = Some(Status::Active);
        zebra.scopes = vec!["project:github.com/acme/zebra".parse().unwrap()];
        store.record(&zebra).unwrap();

        let mut apple = NewLearning::new("apple title", "rule", "rationale");
        apple.status = Some(Status::Active);
        apple.scopes = vec!["project:github.com/acme/apple".parse().unwrap()];
        store.record(&apple).unwrap();
    }

    let app = start_app(&db, config());
    let body = get(&app, "/collection?sort=project&dir=asc").await.body;

    assert!(
        body.contains(r#"sort=project&amp;dir=desc"#) || body.contains("sort=project&dir=desc"),
        "project column should be sortable: {body}"
    );
    let apple_pos = body.find("apple title").unwrap();
    let zebra_pos = body.find("zebra title").unwrap();
    assert!(
        apple_pos < zebra_pos,
        "project asc should order by project id: {body}"
    );
}

#[tokio::test]
async fn collection_filters_by_selected_projects() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut writ = NewLearning::new("writ only", "rule", "rationale");
        writ.status = Some(Status::Active);
        writ.scopes = vec!["project:github.com/acme/writ".parse().unwrap()];
        store.record(&writ).unwrap();

        let mut other = NewLearning::new("other only", "rule", "rationale");
        other.status = Some(Status::Active);
        other.scopes = vec!["project:github.com/acme/other".parse().unwrap()];
        store.record(&other).unwrap();

        let mut none = NewLearning::new("no project", "rule", "rationale");
        none.status = Some(Status::Active);
        none.scopes = vec!["global".parse().unwrap()];
        store.record(&none).unwrap();
    }

    let app = start_app(&db, config());
    let unfiltered = get(&app, "/collection").await.body;
    assert!(
        unfiltered.contains(r#"class="project-filter""#),
        "project header should expose a multi-select filter: {unfiltered}"
    );
    assert!(
        unfiltered.contains(r#"name="project" value="github.com/acme/writ""#),
        "{unfiltered}"
    );
    assert!(
        unfiltered.contains(r#"name="project" value="_none""#),
        "no-project rows need a checkbox: {unfiltered}"
    );

    let body = get(
        &app,
        "/collection?project=github.com/acme/writ&project=_none",
    )
    .await
    .body;
    assert!(body.contains("writ only"), "{body}");
    assert!(body.contains("no project"), "{body}");
    assert!(!body.contains("other only"), "{body}");
    assert!(
        body.contains(r#"name="project" value="github.com/acme/writ" checked"#)
            || body.contains(r#"value="github.com/acme/writ" checked"#),
        "selected projects stay checked: {body}"
    );
}

#[tokio::test]
async fn collection_filters_by_single_project_param() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut writ = NewLearning::new("writ only", "rule", "rationale");
        writ.status = Some(Status::Active);
        writ.scopes = vec!["project:github.com/acme/writ".parse().unwrap()];
        store.record(&writ).unwrap();

        let mut other = NewLearning::new("other only", "rule", "rationale");
        other.status = Some(Status::Active);
        other.scopes = vec!["project:github.com/acme/other".parse().unwrap()];
        store.record(&other).unwrap();
    }

    let app = start_app(&db, config());
    let body = get(&app, "/collection?project=github.com/acme/writ")
        .await
        .body;
    assert!(body.contains("writ only"), "{body}");
    assert!(!body.contains("other only"), "{body}");
}

#[tokio::test]
async fn collection_project_filter_preserves_search_query() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut writ = NewLearning::new("writ alpha", "rule", "rationale");
        writ.status = Some(Status::Active);
        writ.scopes = vec!["project:github.com/acme/writ".parse().unwrap()];
        store.record(&writ).unwrap();
    }

    let app = start_app(&db, config());
    let body = get(&app, "/collection?q=alpha&project=github.com/acme/writ")
        .await
        .body;
    assert!(body.contains(r#"class="project-filter__form""#), "{body}");
    assert!(
        body.contains(r#"type="hidden" name="q" value="alpha""#),
        "project filter Apply must keep the active search: {body}"
    );
}

#[tokio::test]
async fn collection_empty_project_filter_keeps_clear_controls() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut writ = NewLearning::new("writ only", "rule", "rationale");
        writ.status = Some(Status::Active);
        writ.scopes = vec!["project:github.com/acme/writ".parse().unwrap()];
        store.record(&writ).unwrap();
    }

    let app = start_app(&db, config());
    let body = get(&app, "/collection?project=github.com/acme/missing")
        .await
        .body;
    assert!(body.contains("No learnings match"), "{body}");
    assert!(
        body.contains(r#"class="project-filter-empty""#),
        "empty filter results must keep a clearable project form: {body}"
    );
    assert!(
        body.contains(r#"name="project" value="github.com/acme/writ""#),
        "{body}"
    );
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

    let app = start_app(&db, config());
    let response = post_form(
        &app,
        &format!("/learnings/{id}"),
        &[
            ("title", "editable"),
            ("rule", "updated rule"),
            ("rationale", "updated rationale"),
            ("good_snippet", "let good = true;"),
        ],
    )
    .await;
    assert_eq!(response.status, 303);
    assert_eq!(response.headers["location"], format!("/learnings/{id}"));

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

    let app = start_app(&db, config());
    let response = post(&app, &format!("/findings/{finding_id}/reject")).await;
    assert_eq!(response.status, 303);
    assert_eq!(
        response.headers["location"],
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

    let app = start_app(&db, config);
    let response = post(&app, &format!("/findings/{finding_id}/open")).await;
    assert_eq!(response.status, 303, "body: {}", response.body);
    assert_eq!(
        response.headers["location"],
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

    let app = start_app(&db, config());
    let body = get(&app, "/health").await.body;

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

    let app = start_app(&db, config());
    let body = get(&app, "/health").await.body;

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

    let app = start_app(&db, config());
    let body = get(&app, "/health").await.body;

    assert!(body.contains("in both buckets"), "{body}");
    assert!(body.contains("in-both"), "{body}");
}

#[tokio::test]
async fn inbox_empty_state_shows_curation_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let app = start_app(&db, config());
    let body = get(&app, "/inbox").await.body;

    assert!(body.contains("Inbox is empty"), "{body}");
    assert!(body.contains("Every proposal is curated"), "{body}");
    assert!(
        body.contains(r#"class="table-shell ledger-empty empty""#),
        "empty inbox should use the shared ledger empty panel: {body}"
    );
}

#[tokio::test]
async fn health_empty_state_shows_clear_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");

    let app = start_app(&db, config());
    let body = get(&app, "/health").await.body;

    assert!(body.contains("Health is clear"), "{body}");
    assert!(
        body.contains(r#"class="table-shell ledger-empty empty""#),
        "empty health should use the shared ledger empty panel: {body}"
    );
}

#[tokio::test]
async fn inbox_opens_learning_detail_from_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        let mut learning = NewLearning::new("open me", "rule only", "why only");
        learning.status = Some(Status::Proposed);
        store.record(&learning).unwrap().id
    };

    let app = start_app(&db, config());
    let body = get(&app, "/inbox").await.body;

    assert!(
        body.contains(&format!(
            r#"class="proposal-open" href="/learnings/{id}" aria-label="Open open me""#
        )),
        "inbox proposals should open the learning detail: {body}"
    );
    assert!(
        body.contains(r#"class="proposal-title""#),
        "title stays visible while the card opens detail: {body}"
    );
}

#[tokio::test]
async fn inbox_uses_collection_style_chrome() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut learning = NewLearning::new("inbox chrome", "rule", "rationale");
        learning.scopes = vec![
            "project:github.com/acme/writ".parse().unwrap(),
            "language:rust".parse().unwrap(),
        ];
        store.record(&learning).unwrap();
    }

    let app = start_app(&db, config());
    let body = get(&app, "/inbox").await.body;

    assert!(
        body.contains(r#"class="inbox-ledger""#),
        "inbox should sit in a ledger section: {body}"
    );
    assert!(
        body.contains(r#"class="table-shell proposal-list""#),
        "{body}"
    );
    assert!(body.contains(r#"class="proposal-open""#), "{body}");
    assert!(body.contains(r#"class="proposal-title""#), "{body}");
    assert!(
        body.contains(r#"<span class="scope-kind">language:</span>rust"#),
        "{body}"
    );
    assert!(
        body.contains(r#">writ</span>"#),
        "project chips should show only the repo leaf: {body}"
    );
    assert!(
        !body.contains(r#"<span class="scope-kind">project:</span>"#),
        "{body}"
    );
    assert!(
        body.contains(r#"class="btn btn--primary primary""#),
        "{body}"
    );
}

#[tokio::test]
async fn health_uses_collection_style_chrome() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    {
        let mut store = Store::open(&db).unwrap();
        let mut learning = NewLearning::new("health chrome", "rule", "rationale");
        learning.status = Some(Status::Active);
        learning.created_at = Some("2000-01-01 00:00:00".into());
        learning.scopes = vec!["language:elixir".parse().unwrap()];
        store.record(&learning).unwrap();
    }

    let app = start_app(&db, config());
    let body = get(&app, "/health").await.body;

    assert!(
        body.contains(r#"class="health-ledger""#),
        "health should sit in a ledger section: {body}"
    );
    assert!(
        body.contains(
            r#"<colgroup><col class="title"><col class="bucket"><col class="used"><col class="hits"><col class="actions"></colgroup>"#
        ),
        "{body}"
    );
    assert!(body.contains(r#"class="title-link""#), "{body}");
    assert!(
        body.contains(r#"<span class="scope-kind">language:</span>elixir"#),
        "{body}"
    );
    assert!(body.contains(r#"class="btn danger""#), "{body}");
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

    let app = start_app(&db, config());
    let response = post(&app, &format!("/health/{id}/archive")).await;
    assert_eq!(response.status, 303);

    let body = get(&app, "/health").await.body;
    assert!(!body.contains("archive me"), "{body}");
}

#[tokio::test]
async fn foreign_origin_is_rejected_before_every_post_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let marker = dir.path().join("editor-opened");
    let (
        approve_id,
        reject_id,
        health_archive_id,
        detail_save_id,
        detail_archive_id,
        finding_owner_id,
        finding_id,
    ) = {
        let mut store = Store::open(&db).unwrap();
        let approve_id = proposed_with_exemplars(&mut store, "approve target", "a");
        let reject_id = proposed_with_exemplars(&mut store, "reject target", "b");
        let health_archive_id = active_with_exemplar(&mut store, "health archive target");
        let detail_save_id = active_with_exemplar(&mut store, "detail save target");
        let detail_archive_id = active_with_exemplar(&mut store, "detail archive target");
        let finding_owner_id = active_with_exemplar(&mut store, "finding target");
        let finding_id =
            finding_with_path(&mut store, &finding_owner_id, Some("source.rs"), Some(12));
        (
            approve_id,
            reject_id,
            health_archive_id,
            detail_save_id,
            detail_archive_id,
            finding_owner_id,
            finding_id,
        )
    };
    let mut app_config = config();
    app_config.ui.editor_cmd = format!("touch {}", marker.display());
    let app = start_app(&db, app_config);

    let detail_form = [
        ("title", "attacker changed title"),
        ("rule", "attacker changed rule"),
        ("rationale", "attacker changed rationale"),
    ];
    let requests = [
        foreign_post(&app, &format!("/inbox/{approve_id}/approve"), None).await,
        foreign_post(&app, &format!("/inbox/{reject_id}/reject"), None).await,
        foreign_post(&app, &format!("/health/{health_archive_id}/archive"), None).await,
        foreign_post(
            &app,
            &format!("/learnings/{detail_save_id}"),
            Some(&detail_form),
        )
        .await,
        foreign_post(
            &app,
            &format!("/learnings/{detail_archive_id}/archive"),
            None,
        )
        .await,
        foreign_post(&app, &format!("/findings/{finding_id}/reject"), None).await,
        foreign_post(&app, &format!("/findings/{finding_id}/open"), None).await,
    ];

    for response in requests {
        assert_eq!(response.status, StatusCode::FORBIDDEN, "{}", response.body);
        assert!(response.body.contains("Origin"), "{}", response.body);
        assert_eq!(response.headers["x-writ-protocol-version"], "1");
    }

    let store = Store::open(&db).unwrap();
    assert_eq!(store.get(&approve_id).unwrap().status, Status::Proposed);
    assert_eq!(store.get(&reject_id).unwrap().status, Status::Proposed);
    assert_eq!(
        store.get(&health_archive_id).unwrap().status,
        Status::Active
    );
    let detail_save = store.get(&detail_save_id).unwrap();
    assert_eq!(detail_save.title, "detail save target");
    assert_eq!(detail_save.rule, "rule");
    assert_eq!(detail_save.rationale, "rationale");
    assert_eq!(
        store.get(&detail_archive_id).unwrap().status,
        Status::Active
    );
    assert_eq!(
        store.finding(&finding_id).unwrap().outcome,
        writ_core::Outcome::Open
    );
    assert_eq!(store.get(&finding_owner_id).unwrap().status, Status::Active);
    assert!(
        !marker.exists(),
        "foreign Origin must not launch the editor"
    );
}

#[tokio::test]
async fn posts_without_an_origin_are_rejected_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let id = {
        let mut store = Store::open(&db).unwrap();
        proposed_with_exemplars(&mut store, "missing origin", "a")
    };
    let app = start_app(&db, config());

    let response = request_kind_with_origin(
        &app,
        Method::POST,
        &format!("/inbox/{id}/approve"),
        None,
        false,
        None,
    )
    .await;

    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert!(
        response.body.contains("missing Origin"),
        "{}",
        response.body
    );
    assert_eq!(
        Store::open(&db).unwrap().get(&id).unwrap().status,
        Status::Proposed
    );
}

#[tokio::test]
async fn same_origin_post_succeeds_and_get_cannot_mutate() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("learnings.db");
    let (same_origin_id, get_id) = {
        let mut store = Store::open(&db).unwrap();
        (
            proposed_with_exemplars(&mut store, "same origin", "a"),
            proposed_with_exemplars(&mut store, "get target", "b"),
        )
    };
    let app = start_app(&db, config());

    let accepted = post(&app, &format!("/inbox/{same_origin_id}/approve")).await;
    assert_eq!(accepted.status, StatusCode::SEE_OTHER, "{}", accepted.body);

    let get_response = request_kind_with_origin(
        &app,
        Method::GET,
        &format!("/inbox/{get_id}/approve"),
        None,
        false,
        Some("https://attacker.example"),
    )
    .await;
    assert_eq!(get_response.status, StatusCode::METHOD_NOT_ALLOWED);

    let store = Store::open(&db).unwrap();
    assert_eq!(store.get(&same_origin_id).unwrap().status, Status::Active);
    assert_eq!(store.get(&get_id).unwrap().status, Status::Proposed);
}
