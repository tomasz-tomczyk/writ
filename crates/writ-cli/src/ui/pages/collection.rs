use std::collections::HashSet;

use writ_core::{Error, ListFilter, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, with_store};

const UNUSED_DAYS: u32 = 90;

pub fn render(
    state: &AppState,
    q: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    status_filter: Option<&str>,
) -> Result<String, Error> {
    with_store(state, |store| {
        let filter = ListFilter {
            status: status_filter
                .and_then(|s| if s == "all" { None } else { s.parse().ok() }),
            search: q.filter(|s| !s.is_empty()).map(String::from),
            ..Default::default()
        };
        let mut rows = store.list(&filter)?;

        if status_filter.is_none() {
            rows.retain(|r| r.status != Status::Archived);
        }

        let sort_dir = dir.unwrap_or("asc");
        rows.sort_by(|left, right| {
            let ordering = match sort.unwrap_or("title") {
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

        let mut html = String::new();
        html.push_str(r#"<div class="collection-controls">"#);
        html.push_str(&search_form(q));
        html.push_str(&status_chips(status_filter));
        html.push_str("</div>");

        if rows.is_empty() {
            html.push_str(r#"<div class="shell empty">No learnings match.</div>"#);
            return Ok(html);
        }

        html.push_str(r#"<table class="collection">"#);
        html.push_str("<thead><tr>");
        html.push_str(&sort_link("Title", "title", sort, sort_dir, q));
        html.push_str("<th>Scope</th>");
        html.push_str("<th>Matcher</th>");
        html.push_str(&sort_link("Hits", "hit", sort, sort_dir, q));
        html.push_str(&sort_link("Last used", "last_used", sort, sort_dir, q));
        html.push_str(&sort_link("Status", "status", sort, sort_dir, q));
        html.push_str("<th>Health</th>");
        html.push_str("</tr></thead><tbody>");

        for row in rows {
            let is_unused = unused.contains(&row.id);
            let is_never_applied = never_applied.contains(&row.id);
            let health_class = health_class(is_unused, is_never_applied);
            let last_used = row.last_selected_at.as_deref().unwrap_or("—");

            html.push_str("<tr>");
            html.push_str(&format!(
                "<td><a href=\"/learnings/{}\">{}</a></td>",
                escape(&row.id),
                escape(&row.title)
            ));
            html.push_str("<td>");
            for scope in &row.scopes {
                html.push_str(&format!(
                    "<span class=\"chip\">{}</span>",
                    escape(&scope.to_string())
                ));
            }
            html.push_str("</td>");
            html.push_str(&format!(
                "<td>{}</td>",
                matcher_badge(row.matcher_kind, row.matcher.as_deref())
            ));
            html.push_str(&format!("<td>{}</td>", row.times_applied));
            html.push_str(&format!("<td>{}</td>", escape(last_used)));
            html.push_str(&format!(
                "<td><span class=\"status-badge {}\">{}</span></td>",
                row.status.as_str(),
                escape(row.status.as_str())
            ));
            html.push_str(&format!(
                "<td><span class=\"health-dot {}\" title=\"{}\"></span></td>",
                health_class,
                health_title(is_unused, is_never_applied)
            ));
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table>");
        Ok(html)
    })
}

fn search_form(q: Option<&str>) -> String {
    let value = q.map_or(String::new(), escape);
    format!(
        r#"<form method="get" action="/collection" class="search">
             <input type="search" name="q" value="{}" placeholder="Search…">
             <button type="submit">Search</button>
           </form>"#,
        value
    )
}

fn status_chips(current: Option<&str>) -> String {
    let mut html = String::from(r#"<div class="chips">"#);
    for (label, param) in [
        ("Active", Some("active")),
        ("Proposed", Some("proposed")),
        ("Archived", Some("archived")),
        ("All", Some("all")),
    ] {
        let selected = current == param;
        let href = match param {
            Some(p) => format!("/collection?status={}", p),
            None => "/collection".into(),
        };
        let class = if selected { "chip active" } else { "chip" };
        html.push_str(&format!(
            "<a href=\"{}\" class=\"{}\">{}</a>",
            href, class, label
        ));
    }
    html.push_str("</div>");
    html
}

fn sort_link(label: &str, field: &str, sort: Option<&str>, dir: &str, q: Option<&str>) -> String {
    let active = sort == Some(field);
    let next_dir = if active && dir == "asc" { "desc" } else { "asc" };
    let mut href = format!("/collection?sort={field}&dir={next_dir}");
    if let Some(query) = q {
        href.push_str(&format!("&q={}", urlencode(query)));
    }
    let arrow = if active {
        if dir == "asc" { " ▲" } else { " ▼" }
    } else {
        ""
    };
    format!(
        "<th><a href=\"{}\">{}{}</a></th>",
        href,
        escape(label),
        arrow
    )
}

fn matcher_badge(kind: Option<writ_core::MatcherKind>, pattern: Option<&str>) -> String {
    match (kind, pattern) {
        (Some(k), Some(p)) => format!(
            "<span class=\"badge matcher\">{}: {}</span>",
            escape(k.as_str()),
            escape(p)
        ),
        _ => "<span class=\"muted\">—</span>".into(),
    }
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
    text.replace(' ', "%20")
}
