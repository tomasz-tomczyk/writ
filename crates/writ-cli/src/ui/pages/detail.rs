use axum::Form;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use writ_core::{Error, ExemplarKind, LearningUpdate, NewExemplar, Scope, Status};

use crate::ui::AppState;
use crate::ui::pages::health;
use crate::ui::pages::layout;
use crate::ui::pages::layout::{escape, with_store};

/// The element every Detail action swaps.
const SWAP: &str = r##" hx-target="#detail-body" hx-swap="outerHTML""##;

pub fn render(state: &AppState, id: &str) -> Result<String, Error> {
    with_store(state, |store| {
        let learning = store.get(id)?;
        let exemplars = store.exemplars_of(id)?;
        let findings = store.findings_of(id)?;

        let good = exemplars.iter().find(|e| e.kind == ExemplarKind::Good);
        let bad = exemplars.iter().find(|e| e.kind == ExemplarKind::Bad);
        let good_snippet = good.map(|e| e.snippet.as_str()).unwrap_or("");
        let bad_snippet = bad.map(|e| e.snippet.as_str()).unwrap_or("");

        let mut html = String::from(r#"<div id="detail-body">"#);
        html.push_str("<div class=\"detail-meta\">");
        html.push_str(&format!(
            "<span class=\"status-badge {}\">{}</span>",
            learning.status.as_str(),
            escape(learning.status.as_str())
        ));
        html.push_str(&format!(
            "<span class=\"mode-badge {}\">{}</span>",
            if learning.blocking {
                "blocking"
            } else {
                "advisory"
            },
            if learning.blocking {
                "blocking"
            } else {
                "advisory"
            }
        ));
        html.push_str("</div>");
        html.push_str(&format!(
            r#"<form method="post" action="/learnings/{id}" hx-post="/learnings/{id}"{SWAP} class="detail shell">"#,
            id = escape(id)
        ));
        html.push_str("<div class=\"field\">");
        html.push_str("<label>Title</label>");
        html.push_str(&format!(
            "<input type=\"text\" name=\"title\" value=\"{}\" required>",
            escape(&learning.title)
        ));
        html.push_str("</div>");

        html.push_str("<div class=\"field\">");
        html.push_str("<label>Rule</label>");
        html.push_str(&format!(
            "<textarea name=\"rule\" required>{}</textarea>",
            escape(&learning.rule)
        ));
        html.push_str("</div>");

        html.push_str("<div class=\"field\">");
        html.push_str("<label>Rationale</label>");
        html.push_str(&format!(
            "<textarea name=\"rationale\" required>{}</textarea>",
            escape(&learning.rationale)
        ));
        html.push_str("</div>");

        let checked = if learning.blocking { " checked" } else { "" };
        html.push_str("<div class=\"field inline mode-field\">");
        html.push_str(&format!(
            "<label><input type=\"checkbox\" name=\"blocking\" value=\"1\"{}> <span><strong>Blocking</strong><small>Stops handoff when a reported violation is not fixed.</small></span></label>",
            checked
        ));
        html.push_str("</div>");

        html.push_str("<div class=\"field\">");
        html.push_str("<label>Matcher kind</label>");
        html.push_str(r#"<select name="matcher_kind"><option value="">—</option>"#);
        for kind in [
            writ_core::MatcherKind::AstGrep,
            writ_core::MatcherKind::Regex,
        ] {
            let selected = learning.matcher_kind == Some(kind);
            html.push_str(&format!(
                "<option value=\"{}\"{}>{}</option>",
                escape(kind.as_str()),
                if selected { " selected" } else { "" },
                escape(kind.as_str())
            ));
        }
        html.push_str("</select>");
        html.push_str("</div>");

        html.push_str("<div class=\"field\">");
        html.push_str("<label>Matcher</label>");
        html.push_str(&format!(
            "<input type=\"text\" name=\"matcher\" value=\"{}\">",
            escape(learning.matcher.as_deref().unwrap_or(""))
        ));
        html.push_str("</div>");

        html.push_str("<div class=\"field\">");
        html.push_str("<label>Scopes</label>");
        for scope in &learning.scopes {
            html.push_str(&format!(
                "<input type=\"text\" name=\"scope[]\" value=\"{}\">",
                escape(&scope.to_string())
            ));
        }
        html.push_str("<input type=\"text\" name=\"scope[]\" value=\"\" placeholder=\"global or language:rust\">");
        html.push_str("</div>");

        html.push_str("<div class=\"field\">");
        html.push_str("<label>Good exemplar</label>");
        html.push_str(&format!(
            "<textarea name=\"good_snippet\">{}</textarea>",
            escape(good_snippet)
        ));
        html.push_str("</div>");

        html.push_str("<div class=\"field\">");
        html.push_str("<label>Bad exemplar</label>");
        html.push_str(&format!(
            "<textarea name=\"bad_snippet\">{}</textarea>",
            escape(bad_snippet)
        ));
        html.push_str("</div>");

        html.push_str("<div class=\"actions\">");
        html.push_str("<button type=\"submit\" name=\"action\" value=\"save\" class=\"primary\">Save</button>");
        html.push_str("<button type=\"submit\" name=\"action\" value=\"save_activate\">Save &amp; activate</button>");
        html.push_str("</div>");
        html.push_str("</form>");

        let archive = format!("/learnings/{}/archive", escape(id));
        html.push_str(&format!(
            "<form method=\"post\" action=\"{archive}\" hx-post=\"{archive}\"{SWAP} class=\"danger-form shell\"><div><strong>Archive learning</strong><span>Remove it from selection while preserving its history.</span></div>"
        ));
        html.push_str("<button type=\"submit\" class=\"danger\">Archive</button>");
        html.push_str("</form>");

        if !findings.is_empty() {
            html.push_str("<div class=\"section-heading\"><h2>Findings</h2><p>Violations reported for this learning.</p></div>");
            html.push_str("<div class=\"table-shell\">");
            html.push_str(r#"<table class="findings">"#);
            html.push_str("<thead><tr><th>Path</th><th>Line</th><th>Detail</th><th>Outcome</th><th></th></tr></thead><tbody>");
            for finding in findings {
                html.push_str("<tr>");
                html.push_str(&format!(
                    "<td>{}</td>",
                    escape(finding.path.as_deref().unwrap_or("—"))
                ));
                html.push_str(&format!(
                    "<td>{}</td>",
                    finding.line.map_or_else(|| "—".into(), |l| l.to_string())
                ));
                html.push_str(&format!(
                    "<td>{}</td>",
                    escape(finding.detail.as_deref().unwrap_or(""))
                ));
                html.push_str(&format!(
                    "<td><span class=\"outcome {}\">{}</span></td>",
                    finding.outcome.as_str(),
                    escape(finding.outcome.as_str())
                ));
                html.push_str("<td class=\"actions\">");
                let reject = format!("/findings/{}/reject", escape(&finding.id));
                html.push_str(&format!(
                    "<form method=\"post\" action=\"{reject}\" hx-post=\"{reject}\"{SWAP}><button type=\"submit\">Reject</button></form>"
                ));
                if finding.path.is_some() {
                    let open = format!("/findings/{}/open", escape(&finding.id));
                    html.push_str(&format!(
                        "<form method=\"post\" action=\"{open}\" hx-post=\"{open}\"{SWAP}><button type=\"submit\">Open</button></form>"
                    ));
                }
                html.push_str("</td></tr>");
            }
            html.push_str("</tbody></table></div>");
        }

        html.push_str("</div>");
        Ok(html)
    })
}

#[derive(Debug, serde::Deserialize)]
pub struct SaveForm {
    title: String,
    rule: String,
    rationale: String,
    blocking: Option<String>,
    matcher_kind: Option<String>,
    matcher: Option<String>,
    #[serde(default)]
    scope: Vec<String>,
    good_snippet: Option<String>,
    bad_snippet: Option<String>,
    action: Option<String>,
}

pub async fn get(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    match render(&state, &id) {
        Ok(body) if layout::is_htmx(&headers) => layout::fragment(body.as_str()),
        Ok(body) => layout::render(&state, "Detail", body.as_str()),
        Err(error) => layout::error_response(error),
    }
}

pub async fn post(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SaveForm>,
) -> Response {
    let result = with_store(&state, |store| {
        let update = build_update(&form)?;
        store.update_learning(&id, &update)?;
        if form.action.as_deref() == Some("save_activate") {
            store.set_status(&id, Status::Active)?;
        }
        Ok(())
    });
    match result {
        Ok(()) if layout::is_htmx(&headers) => match render(&state, &id) {
            Ok(body) => layout::fragment(body.as_str()),
            Err(error) => layout::error_response(error),
        },
        Ok(()) => redirect(&format!("/learnings/{id}")),
        Err(error) => layout::error_response(error),
    }
}

/// Archive, from either Health or Detail.
///
/// One route serves both screens, so the fragment to send back is the one
/// htmx names in `HX-Target`. Without htmx the redirect is unchanged.
pub async fn archive(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = with_store(&state, |store| store.set_status(&id, Status::Archived)) {
        return layout::error_response(error);
    }
    if !layout::is_htmx(&headers) {
        return redirect("/collection");
    }
    let target = headers
        .get("hx-target")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let body = match target {
        "detail-body" => render(&state, &id),
        _ => health::render(&state),
    };
    match body {
        Ok(body) => layout::fragment(body.as_str()),
        Err(error) => layout::error_response(error),
    }
}

fn build_update(form: &SaveForm) -> Result<LearningUpdate, Error> {
    let matcher_kind = form
        .matcher_kind
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .transpose()?;
    let matcher = form
        .matcher
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let scopes: Result<Vec<Scope>, Error> = form
        .scope
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .collect();

    let mut exemplars = Vec::new();
    if let Some(snippet) = form
        .good_snippet
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        exemplars.push(NewExemplar {
            kind: ExemplarKind::Good,
            language: None,
            snippet: snippet.into(),
            note: None,
        });
    }
    if let Some(snippet) = form.bad_snippet.as_deref().filter(|s| !s.trim().is_empty()) {
        exemplars.push(NewExemplar {
            kind: ExemplarKind::Bad,
            language: None,
            snippet: snippet.into(),
            note: None,
        });
    }

    Ok(LearningUpdate {
        title: form.title.trim().into(),
        rule: form.rule.trim().into(),
        rationale: form.rationale.trim().into(),
        blocking: form.blocking.is_some(),
        matcher_kind,
        matcher,
        scopes: scopes?,
        exemplars,
    })
}

fn redirect(path: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(header::LOCATION, path.parse().unwrap());
    response
}
