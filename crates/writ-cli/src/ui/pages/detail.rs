use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use writ_core::{
    Error, Exemplar, ExemplarKind, Finding, LearningUpdate, NewExemplar, Outcome, Scope, Status,
};

use crate::ui::AppState;
use crate::ui::pages::health;
use crate::ui::pages::layout;
use crate::ui::pages::layout::{escape, with_store};

/// The element every Detail action swaps.
const SWAP: &str = r##" hx-target="#detail-body" hx-swap="outerHTML""##;

fn render_with_origin(state: &AppState, id: &str, origin: Option<&str>) -> Result<String, Error> {
    with_store(state, |store| {
        let learning = store.get(id)?;
        let exemplars = store.exemplars_of(id)?;
        let findings = store.findings_of(id)?;

        let good = exemplars.iter().find(|e| e.kind == ExemplarKind::Good);
        let bad = exemplars.iter().find(|e| e.kind == ExemplarKind::Bad);
        let good_snippet = good.map(|e| e.snippet.as_str()).unwrap_or("");
        let bad_snippet = bad.map(|e| e.snippet.as_str()).unwrap_or("");

        let (back_href, back_label, origin_param) = origin_context(origin);
        let mut html = String::from(r#"<div id="detail-body">"#);
        html.push_str(&format!(
            r#"<nav class="back-nav" aria-label="Breadcrumb"><a class="back-link" href="{back_href}"><span class="back-link__arrow" aria-hidden="true">←</span> {back_label}</a></nav>"#
        ));
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
            r#"<form method="post" action="/learnings/{id}{origin_param}" hx-post="/learnings/{id}{origin_param}"{SWAP} class="detail shell">"#,
            id = escape(id),
            origin_param = origin_param,
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
        html.push_str("<label>Sides</label>");
        html.push_str(r#"<select name="sides">"#);
        for side in [
            writ_core::Sides::Both,
            writ_core::Sides::Added,
            writ_core::Sides::Removed,
        ] {
            let selected = learning.sides == side;
            html.push_str(&format!(
                "<option value=\"{}\"{}>{}</option>",
                escape(side.as_str()),
                if selected { " selected" } else { "" },
                escape(side.as_str())
            ));
        }
        html.push_str("</select>");
        html.push_str("<small>Which half of the diff this rule cares about.</small>");
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
                "<input type=\"text\" name=\"scope\" value=\"{}\">",
                escape(&scope.to_string())
            ));
        }
        html.push_str("<input type=\"text\" name=\"scope\" value=\"\" placeholder=\"global or language:rust\">");
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
            "<form method=\"post\" action=\"{archive}{origin_param}\" hx-post=\"{archive}{origin_param}\"{SWAP} class=\"danger-form shell\"><div><strong>Archive learning</strong><span>Remove it from selection while preserving its history.</span></div>"
        ));
        html.push_str("<button type=\"submit\" class=\"danger\">Archive</button>");
        html.push_str("</form>");

        if !findings.is_empty() {
            html.push_str("<div class=\"section-heading\"><h2>Findings</h2><p>Violations reported for this learning.</p></div>");
            html.push_str(&render_findings(&findings));
        }

        html.push_str("</div>");
        Ok(html)
    })
}

/// Render the whole Findings section an htmx outcome action replaces.
///
/// The swap is the section and not the one row it changed, because the
/// header strip carries per-outcome counts. Swapping a row alone would
/// move a finding out of `open` and leave the tally claiming it is still
/// there.
pub(crate) fn render_finding(state: &AppState, id: &str) -> Result<String, Error> {
    with_store(state, |store| {
        let finding = store.finding(id)?;
        let findings = store.findings_of(&finding.learning_id)?;
        Ok(render_findings(&findings))
    })
}

/// How many findings carry each outcome. Rendered as static text, which a
/// script upgrades into filters; the counts read without it.
fn tally(findings: &[Finding]) -> [(Outcome, usize); 4] {
    let count = |want: Outcome| findings.iter().filter(|f| f.outcome == want).count();
    [
        (Outcome::Open, count(Outcome::Open)),
        (Outcome::Fixed, count(Outcome::Fixed)),
        (Outcome::Ignored, count(Outcome::Ignored)),
        (Outcome::Rejected, count(Outcome::Rejected)),
    ]
}

/// The glyph one outcome is drawn with. Four shapes for four values, so
/// the state survives a reader who cannot separate the four hues.
fn outcome_glyph(outcome: Outcome, size: u32) -> String {
    let body = match outcome {
        Outcome::Open => r#"<circle cx="8" cy="8" r="6.4" fill="none" stroke="currentColor" stroke-width="1.7"/>"#.to_string(),
        // `r##` because the check stroke is "#fff" and `"#` would close `r#`.
        Outcome::Fixed => r##"<circle cx="8" cy="8" r="6.6" fill="currentColor"/><path d="M5.2 8.2l2 2 3.6-4.1" fill="none" stroke="#fff" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"##.to_string(),
        Outcome::Ignored => r#"<circle cx="8" cy="8" r="6.4" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M5.2 8h5.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#.to_string(),
        Outcome::Rejected => r#"<circle cx="8" cy="8" r="6.4" fill="none" stroke="currentColor" stroke-width="1.7"/><path d="M6 6l4 4M10 6l-4 4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#.to_string(),
    };
    format!(
        r#"<svg viewBox="0 0 16 16" width="{size}" height="{size}" aria-hidden="true">{body}</svg>"#
    )
}

/// Split a path so only the directory is allowed to give way when the row
/// runs out of room. The file name and the line are what the developer
/// reads, so they are their own elements and never shrink.
fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(cut) => path.split_at(cut + 1),
        None => ("", path),
    }
}

fn render_findings(findings: &[Finding]) -> String {
    let mut html =
        String::from(r#"<section class="findings" id="findings" aria-label="Findings">"#);
    html.push_str(r#"<div class="findings-head">"#);
    html.push_str(&format!(
        r#"<span class="findings-count">{} finding{}</span>"#,
        findings.len(),
        if findings.len() == 1 { "" } else { "s" }
    ));
    html.push_str(r#"<div class="findings-legend" id="findings-legend">"#);
    html.push_str(r#"<span class="tally" data-filter="all">All</span>"#);
    for (outcome, count) in tally(findings) {
        html.push_str(&format!(
            r#"<span class="tally" data-filter="{key}" data-count="{count}" data-outcome="{key}">{glyph}<span class="tally-n">{count}</span> {key}</span>"#,
            key = outcome.as_str(),
            glyph = outcome_glyph(outcome, 12),
        ));
    }
    html.push_str("</div></div>");

    html.push_str(r#"<ul class="findings-list">"#);
    for finding in findings {
        html.push_str(&render_finding_row(finding));
    }
    html.push_str("</ul></section>");
    html
}

fn render_finding_row(finding: &Finding) -> String {
    let id = escape(&finding.id);
    let outcome = finding.outcome;
    let mut html = format!(
        r#"<li class="f-row" id="finding-{id}" data-outcome="{key}">"#,
        key = outcome.as_str()
    );

    // Badge: glyph plus the word. Colour lives only in these two, so a
    // findings list never becomes a wall of filled pills.
    html.push_str(&format!(
        r#"<span class="f-badge">{glyph}{word}</span>"#,
        glyph = outcome_glyph(outcome, 13),
        word = escape(outcome.as_str()),
    ));

    // Body: the detail is what the developer reads, so it is the row, and
    // the path is demoted beneath it.
    html.push_str(r#"<div class="f-body">"#);
    match finding.detail.as_deref().filter(|text| !text.is_empty()) {
        Some(detail) => html.push_str(&format!(r#"<p class="f-detail">{}</p>"#, escape(detail))),
        // A blank cell reads as a rendering fault. Say what is missing.
        None => html.push_str(r#"<p class="f-detail is-empty">No detail reported</p>"#),
    }
    html.push_str(r#"<div class="f-meta">"#);
    match (finding.path.as_deref(), finding.line) {
        // The longest string on the row is also the thing the developer
        // clicks, so it is the control and there is no separate button.
        (Some(path), Some(line)) => {
            let open = format!("/findings/{id}/open");
            let (dir, base) = split_path(path);
            html.push_str(&format!(
                r#"<form class="f-path-form" method="post" action="{open}" hx-post="{open}" hx-swap="none"><button class="f-path" type="submit" title="Open {loc} in your editor"><span class="f-dir">{dir}</span><span class="f-base">{base}</span><span class="f-line">:{line}</span></button></form>"#,
                loc = escape(&format!("{path}:{line}")),
                dir = escape(dir),
                base = escape(base),
            ));
        }
        // A path with no line cannot be opened, so it is text, not a
        // control that would do nothing.
        (Some(path), None) => {
            let (dir, base) = split_path(path);
            html.push_str(&format!(
                r#"<span class="f-path"><span class="f-dir">{dir}</span><span class="f-base">{base}</span></span>"#,
                dir = escape(dir),
                base = escape(base),
            ));
        }
        (None, _) => html.push_str(r#"<span class="f-nopath">No location reported</span>"#),
    }
    html.push_str("</div></div>");

    // Actions. Rejecting is the only human vote in the ranking, so the
    // script arms it and a second click commits; without the script the
    // first click submits, which is what the server expects either way.
    html.push_str(r#"<div class="f-actions">"#);
    if outcome == Outcome::Rejected {
        let unreject = format!("/findings/{id}/unreject");
        html.push_str(&format!(
            r##"<form method="post" action="{unreject}" hx-post="{unreject}" hx-target="#findings" hx-swap="outerHTML"><button class="f-act f-act-undo" type="submit" title="Undo the rejection. The finding returns to open, not to what it carried before.">Undo rejection</button></form>"##
        ));
    } else {
        let reject = format!("/findings/{id}/reject");
        html.push_str(&format!(
            r##"<form method="post" action="{reject}" hx-post="{reject}" hx-target="#findings" hx-swap="outerHTML"><button class="f-act f-act-reject" type="submit" data-confirm="Confirm reject" title="This finding was wrong. Rejecting it demotes this rule in every future selection.">Reject</button></form>"##
        ));
    }
    html.push_str("</div></li>");
    html
}

fn origin_context(origin: Option<&str>) -> (&'static str, &'static str, String) {
    // Label is the destination in the ledger, not "Back to …".
    let (href, label, value) = match origin {
        Some("review") => ("/collection?view=review", "Review", Some("review")),
        Some("needs-attention") => (
            "/collection?view=needs-attention",
            "Needs attention",
            Some("needs-attention"),
        ),
        Some("archive") => ("/collection?view=archive", "Archive", Some("archive")),
        Some("all") => ("/collection?view=all", "All", Some("all")),
        _ => ("/collection?view=active", "Collection", Some("active")),
    };
    let param = value.map_or_else(String::new, |value| format!("?from={value}"));
    (href, label, param)
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct DetailQuery {
    from: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct SaveForm {
    title: String,
    rule: String,
    rationale: String,
    blocking: Option<String>,
    sides: Option<String>,
    matcher_kind: Option<String>,
    matcher: Option<String>,
    scope: Vec<String>,
    good_snippet: Option<String>,
    bad_snippet: Option<String>,
    action: Option<String>,
}

impl SaveForm {
    /// `serde_urlencoded` cannot deserialize repeated keys into a `Vec`, so
    /// keep the form as its native ordered key/value pairs and collect scopes
    /// explicitly. This is also the shape browsers actually submit.
    fn from_fields(fields: Vec<(String, String)>) -> Self {
        let mut form = Self::default();
        for (name, value) in fields {
            match name.as_str() {
                "title" => form.title = value,
                "rule" => form.rule = value,
                "rationale" => form.rationale = value,
                "blocking" => form.blocking = Some(value),
                "sides" => form.sides = Some(value),
                "matcher_kind" => form.matcher_kind = Some(value),
                "matcher" => form.matcher = Some(value),
                "scope" => form.scope.push(value),
                "good_snippet" => form.good_snippet = Some(value),
                "bad_snippet" => form.bad_snippet = Some(value),
                "action" => form.action = Some(value),
                _ => {}
            }
        }
        form
    }
}

pub async fn get(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DetailQuery>,
) -> Response {
    let title = match with_store(&state, |store| Ok(store.get(&id)?.title)) {
        Ok(title) => title,
        Err(error) => return layout::error_response(error),
    };
    match render_with_origin(&state, &id, query.from.as_deref()) {
        Ok(body) if layout::is_htmx(&headers) => layout::fragment(body.as_str()),
        Ok(body) => layout::render(&state, &title, body.as_str()),
        Err(error) => layout::error_response(error),
    }
}

pub async fn post(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DetailQuery>,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let form = SaveForm::from_fields(fields);
    let result = with_store(&state, |store| {
        let exemplars = store.exemplars_of(&id)?;
        let update = build_update(&form, &exemplars)?;
        let status = (form.action.as_deref() == Some("save_activate")).then_some(Status::Active);
        store.update_learning_and_set_status(&id, &update, status)
    });
    match result {
        Ok(()) if layout::is_htmx(&headers) => {
            match render_with_origin(&state, &id, query.from.as_deref()) {
                Ok(body) => layout::fragment(body.as_str()),
                Err(error) => layout::error_response(error),
            }
        }
        Ok(()) => redirect(&format!(
            "/learnings/{id}{}",
            origin_context(query.from.as_deref()).2
        )),
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
    Query(query): Query<DetailQuery>,
) -> Response {
    if let Err(error) = with_store(&state, |store| store.set_status(&id, Status::Archived)) {
        return layout::error_response(error);
    }
    if !layout::is_htmx(&headers) {
        return redirect(origin_context(query.from.as_deref()).0);
    }
    let target = headers
        .get("hx-target")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let body = match target {
        "detail-body" => render_with_origin(&state, &id, query.from.as_deref()),
        _ => health::render(&state),
    };
    match body {
        Ok(body) => layout::fragment(body.as_str()),
        Err(error) => layout::error_response(error),
    }
}

fn build_update(form: &SaveForm, current_exemplars: &[Exemplar]) -> Result<LearningUpdate, Error> {
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

    let exemplars = merge_visible_exemplars(form, current_exemplars);

    let sides = match form
        .sides
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(text) => text.parse()?,
        None => writ_core::Sides::Both,
    };

    Ok(LearningUpdate {
        title: form.title.trim().into(),
        rule: form.rule.trim().into(),
        rationale: form.rationale.trim().into(),
        blocking: form.blocking.is_some(),
        sides,
        matcher_kind,
        matcher,
        scopes: scopes?,
        exemplars,
    })
}

/// Apply the two visible snippet fields without discarding hidden exemplars or
/// their language/note metadata. Only the first exemplar of each kind is the
/// visible member of the pair; later children round-trip unchanged.
fn merge_visible_exemplars(form: &SaveForm, current: &[Exemplar]) -> Vec<NewExemplar> {
    let mut saw_good = false;
    let mut saw_bad = false;
    let mut exemplars = Vec::with_capacity(current.len() + 2);

    for exemplar in current {
        let (submitted, first) = match exemplar.kind {
            ExemplarKind::Good => (&form.good_snippet, !std::mem::replace(&mut saw_good, true)),
            ExemplarKind::Bad => (&form.bad_snippet, !std::mem::replace(&mut saw_bad, true)),
        };
        if !first || submitted.is_none() {
            exemplars.push(as_new_exemplar(exemplar));
            continue;
        }
        if let Some(snippet) = submitted
            .as_deref()
            .filter(|snippet| !snippet.trim().is_empty())
        {
            let mut edited = as_new_exemplar(exemplar);
            edited.snippet = snippet.to_string();
            exemplars.push(edited);
        }
    }

    if !saw_good
        && let Some(snippet) = form
            .good_snippet
            .as_deref()
            .filter(|snippet| !snippet.trim().is_empty())
    {
        exemplars.push(NewExemplar {
            kind: ExemplarKind::Good,
            language: None,
            snippet: snippet.to_string(),
            note: None,
        });
    }
    if !saw_bad
        && let Some(snippet) = form
            .bad_snippet
            .as_deref()
            .filter(|snippet| !snippet.trim().is_empty())
    {
        exemplars.push(NewExemplar {
            kind: ExemplarKind::Bad,
            language: None,
            snippet: snippet.to_string(),
            note: None,
        });
    }

    exemplars
}

fn as_new_exemplar(exemplar: &Exemplar) -> NewExemplar {
    NewExemplar {
        kind: exemplar.kind,
        language: exemplar.language.clone(),
        snippet: exemplar.snippet.clone(),
        note: exemplar.note.clone(),
    }
}

fn redirect(path: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(header::LOCATION, path.parse().unwrap());
    response
}
