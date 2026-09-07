use std::collections::HashSet;

use writ_core::{Error, ListFilter, Scope, ScopeKind, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, scope_chip, with_store};

const UNUSED_DAYS: u32 = 90;

/// The element every Collection control swaps. `hx-push-url` keeps the
/// address bar honest, so a reload lands on the same sort and filter.
const SWAP: &str = r##" hx-target="#collection-body" hx-swap="outerHTML" hx-push-url="true""##;

pub fn render(
    state: &AppState,
    q: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    status_filter: Option<&str>,
) -> Result<String, Error> {
    with_store(state, |store| {
        let filter = ListFilter {
            status: status_filter.and_then(|s| if s == "all" { None } else { s.parse().ok() }),
            search: q.filter(|s| !s.is_empty()).map(String::from),
            ..Default::default()
        };
        let mut rows = store.list(&filter)?;

        let sort_field = sort
            .filter(|field| matches!(*field, "hit" | "last_used" | "status"))
            .unwrap_or("title");
        let sort_dir = if dir == Some("desc") { "desc" } else { "asc" };
        rows.sort_by(|left, right| {
            let ordering = match sort_field {
                "hit" => left.times_applied.cmp(&right.times_applied),
                "last_used" => left.last_selected_at.cmp(&right.last_selected_at),
                "status" => left.status.as_str().cmp(right.status.as_str()),
                _ => left.title.cmp(&right.title),
            };
            if sort_dir == "desc" {
                ordering.reverse()
            } else {
                ordering
            }
        });

        let unused: HashSet<String> = store
            .list(&ListFilter {
                status: Some(Status::Active),
                unused_days: Some(UNUSED_DAYS),
                ..Default::default()
            })?
            .into_iter()
            .map(|l| l.id)
            .collect();
        let never_applied: HashSet<String> = store
            .list(&ListFilter {
                status: Some(Status::Active),
                never_applied: true,
                ..Default::default()
            })?
            .into_iter()
            .map(|l| l.id)
            .collect();

        let result_count = rows.len();
        let mut html = String::from(
            r#"<div id="collection-body"><section class="collection-ledger"><div class="collection-controls">"#,
        );
        html.push_str(&search_form(q));
        html.push_str(&status_chips(status_filter));
        html.push_str(&format!(
            r#"<span class="result-count">{} {}</span></div>"#,
            result_count,
            if result_count == 1 {
                "learning"
            } else {
                "learnings"
            }
        ));

        if rows.is_empty() {
            html.push_str(&crate::ui::pages::layout::ledger_empty(
                "No learnings match",
                "Try a different search or status filter.",
            ));
            html.push_str("</section></div>");
            return Ok(html);
        }

        html.push_str(r#"<div class="table-shell"><table class="collection">"#);
        html.push_str(
            r#"<colgroup><col class="title"><col class="project"><col class="mode"><col class="hits"><col class="used"><col class="status"><col class="health"></colgroup>"#,
        );
        html.push_str("<thead><tr>");
        html.push_str(&sort_link("Title", "title", Some(sort_field), sort_dir, q));
        html.push_str(r#"<th scope="col">Project</th>"#);
        html.push_str(r#"<th scope="col">Mode</th>"#);
        html.push_str(&sort_link("Hits", "hit", Some(sort_field), sort_dir, q));
        html.push_str(&sort_link(
            "Last used",
            "last_used",
            Some(sort_field),
            sort_dir,
            q,
        ));
        html.push_str(&sort_link(
            "Status",
            "status",
            Some(sort_field),
            sort_dir,
            q,
        ));
        html.push_str(r#"<th scope="col">Health</th>"#);
        html.push_str("</tr></thead><tbody>");

        for row in rows {
            let is_unused = unused.contains(&row.id);
            let is_never_applied = never_applied.contains(&row.id);
            let health_class = health_class(is_unused, is_never_applied);
            let last_used = row.last_selected_at.as_deref().unwrap_or("—");

            html.push_str("<tr>");
            html.push_str(&title_cell(&row.id, &row.title, &row.scopes));
            html.push_str(&project_cell(&row.scopes));
            html.push_str(&format!(
                "<td><span class=\"mode-badge {}\">{}</span></td>",
                if row.blocking { "blocking" } else { "advisory" },
                if row.blocking { "blocking" } else { "advisory" }
            ));
            html.push_str(&format!("<td class=\"numeric\">{}</td>", row.times_applied));
            html.push_str(&format!("<td class=\"date\">{}</td>", escape(last_used)));
            html.push_str(&format!(
                "<td><span class=\"status-badge {}\">{}</span></td>",
                row.status.as_str(),
                escape(row.status.as_str())
            ));
            html.push_str(&format!(
                "<td class=\"health-cell\"><span class=\"health-dot {}\" title=\"{}\" role=\"img\" aria-label=\"{}\"></span></td>",
                health_class,
                health_title(is_unused, is_never_applied),
                health_title(is_unused, is_never_applied)
            ));
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table></div></section></div>");
        Ok(html)
    })
}

fn search_form(q: Option<&str>) -> String {
    let value = q.map_or(String::new(), escape);
    format!(
        r#"<form method="get" action="/collection" hx-get="/collection"{SWAP} class="search search-form" role="search">
             <input class="form-control search-input" type="search" name="q" value="{}" placeholder="Search learnings, rationales, scopes…" aria-label="Search learnings">
             <button class="btn search-button" type="submit">Search</button>
           </form>"#,
        value
    )
}

fn status_chips(current: Option<&str>) -> String {
    let mut html = String::from(
        r#"<div class="filters filter-chips" role="group" aria-label="Filter by status">"#,
    );
    let selected = current.unwrap_or("all");
    for (label, param) in [
        ("Active", "active"),
        ("Proposed", "proposed"),
        ("Archived", "archived"),
        ("All", "all"),
    ] {
        let current_attr = if selected == param {
            r#" aria-current="true""#
        } else {
            ""
        };
        let href = format!("/collection?status={param}");
        html.push_str(&format!(
            "<a href=\"{href}\" hx-get=\"{href}\"{SWAP} class=\"filter filter-chip\"{current_attr}>{label}</a>"
        ));
    }
    html.push_str("</div>");
    html
}

fn sort_link(label: &str, field: &str, sort: Option<&str>, dir: &str, q: Option<&str>) -> String {
    let active = sort == Some(field);
    let next_dir = if active && dir == "asc" {
        "desc"
    } else {
        "asc"
    };
    let mut href = format!("/collection?sort={field}&dir={next_dir}");
    if let Some(query) = q {
        href.push_str(&format!("&q={}", urlencode(query)));
    }
    let href = escape(&href);
    let direction = if active {
        format!(r#" data-direction="{dir}""#)
    } else {
        String::new()
    };
    let aria_sort = if active {
        if dir == "asc" {
            "ascending"
        } else {
            "descending"
        }
    } else {
        "none"
    };
    format!(
        "<th scope=\"col\" aria-sort=\"{aria_sort}\"><a class=\"sort-button\"{direction} href=\"{}\" hx-get=\"{}\"{SWAP}>{}<svg class=\"sort-caret\" viewBox=\"0 0 12 12\" fill=\"none\" aria-hidden=\"true\"><path d=\"m3 7 3-3 3 3\" stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"></path></svg></a></th>",
        href,
        href,
        escape(label)
    )
}

fn title_cell(id: &str, title: &str, scopes: &[Scope]) -> String {
    let mut html = format!(
        "<td class=\"title-cell\"><a class=\"title-link\" href=\"/learnings/{}\">{}</a>",
        escape(id),
        escape(title)
    );
    let meta: Vec<&Scope> = scopes
        .iter()
        .filter(|scope| scope.kind != ScopeKind::Project)
        .collect();
    if !meta.is_empty() {
        html.push_str(r#"<div class="title-meta">"#);
        for scope in meta {
            html.push_str(&scope_chip(scope, "scope"));
        }
        html.push_str("</div>");
    }
    html.push_str("</td>");
    html
}

fn project_cell(scopes: &[Scope]) -> String {
    let matching: Vec<&Scope> = scopes
        .iter()
        .filter(|scope| scope.kind == ScopeKind::Project)
        .collect();
    let mut html = String::from("<td><div class=\"cell-chips\">");
    if matching.is_empty() {
        html.push_str(r#"<span class="muted empty-value">—</span>"#);
    } else {
        for scope in matching {
            html.push_str(&scope_chip(scope, "project"));
        }
    }
    html.push_str("</div></td>");
    html
}

fn health_class(unused: bool, never_applied: bool) -> &'static str {
    match (unused, never_applied) {
        (true, true) => "red",
        (true, false) | (false, true) => "amber",
        (false, false) => "green",
    }
}

fn health_title(unused: bool, never_applied: bool) -> String {
    match (unused, never_applied) {
        (true, true) => "unused 90d and never applied".into(),
        (true, false) => "unused 90d".into(),
        (false, true) => "selected but never applied".into(),
        (false, false) => "healthy".into(),
    }
}

fn urlencode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    encoded
}
