use std::collections::BTreeSet;

use writ_core::{Error, Learning, ListFilter, Scope, ScopeKind, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, project_repo_name, scope_chip, with_store};
use crate::ui::pages::{health, inbox};

/// Sentinel for learnings that carry no `project:` scope.
const PROJECT_NONE: &str = "_none";

/// The element every Collection control swaps. `hx-push-url` keeps the
/// address bar honest, so a reload lands on the same sort and filter.
const SWAP: &str = r##" hx-target="#collection-body" hx-swap="outerHTML" hx-push-url="true""##;

struct Params<'a> {
    view: View,
    q: Option<&'a str>,
    sort: Option<&'a str>,
    dir: &'a str,
    projects: &'a [String],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    Active,
    Review,
    NeedsAttention,
    Archive,
    All,
}

impl View {
    fn from_query(view: Option<&str>, legacy_status: Option<&str>) -> Self {
        match view.or(legacy_status) {
            Some("review" | "proposed") => Self::Review,
            Some("needs-attention") => Self::NeedsAttention,
            Some("archive" | "archived") => Self::Archive,
            Some("all") => Self::All,
            _ => Self::Active,
        }
    }

    fn param(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Review => "review",
            Self::NeedsAttention => "needs-attention",
            Self::Archive => "archive",
            Self::All => "all",
        }
    }

    fn status(self) -> Option<Status> {
        match self {
            Self::Active => Some(Status::Active),
            Self::Archive => Some(Status::Archived),
            Self::All => None,
            Self::Review | Self::NeedsAttention => unreachable!("rendered as dedicated views"),
        }
    }
}

pub fn render(
    state: &AppState,
    view_filter: Option<&str>,
    q: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    legacy_status: Option<&str>,
    project_filter: &[String],
) -> Result<String, Error> {
    render_with_notice(
        state,
        view_filter,
        q,
        sort,
        dir,
        legacy_status,
        project_filter,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn render_with_notice(
    state: &AppState,
    view_filter: Option<&str>,
    q: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    legacy_status: Option<&str>,
    project_filter: &[String],
    notice: Option<&str>,
) -> Result<String, Error> {
    let view = View::from_query(view_filter, legacy_status);
    let attention_count = health::count(state)?;
    let content = match view {
        View::Review => inbox::render(state)?,
        View::NeedsAttention => health::render(state)?,
        View::Active | View::Archive | View::All => {
            render_table(state, view, q, sort, dir, project_filter)?
        }
    };

    let mut html = String::from(r#"<div id="collection-body">"#);
    // Review is a top-nav page, not a Collection filter — no mode chips there.
    if view != View::Review {
        html.push_str(&mode_chips(view, attention_count));
    }
    if let Some(notice) = notice {
        html.push_str(&format!(
            r#"<p class="flash" role="status" aria-live="polite">{}</p>"#,
            escape(notice)
        ));
    }
    html.push_str(&content);
    html.push_str("</div>");
    Ok(html)
}

fn render_table(
    state: &AppState,
    view: View,
    q: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    project_filter: &[String],
) -> Result<String, Error> {
    with_store(state, |store| {
        let filter = ListFilter {
            status: view.status(),
            search: q.filter(|s| !s.is_empty()).map(String::from),
            ..Default::default()
        };
        let mut rows = store.list(&filter)?;

        let (available_projects, has_no_project) = project_options(&rows);

        if !project_filter.is_empty() {
            let selected: BTreeSet<&str> = project_filter.iter().map(String::as_str).collect();
            rows.retain(|row| row_matches_projects(row, &selected));
        }

        let sort_field = sort
            .filter(|field| matches!(*field, "hit" | "last_used" | "status" | "project" | "title"))
            .unwrap_or("title");
        let sort_dir = if dir == Some("desc") { "desc" } else { "asc" };
        rows.sort_by(|left, right| {
            let ordering = match sort_field {
                "hit" => left.times_applied.cmp(&right.times_applied),
                "last_used" => left.last_selected_at.cmp(&right.last_selected_at),
                "status" => left.status.as_str().cmp(right.status.as_str()),
                "project" => cmp_projects(&left.scopes, &right.scopes),
                _ => left.title.cmp(&right.title),
            };
            if sort_dir == "desc" {
                ordering.reverse()
            } else {
                ordering
            }
        });

        let params = Params {
            view,
            q,
            sort: Some(sort_field),
            dir: sort_dir,
            projects: project_filter,
        };

        let result_count = rows.len();
        let mut html =
            String::from(r#"<section class="collection-ledger"><div class="collection-controls">"#);
        html.push_str(&search_form(&params));
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
                "Try a different search, mode, or project filter.",
            ));
            if !available_projects.is_empty() || has_no_project {
                html.push_str(r#"<div class="project-filter-empty">"#);
                html.push_str(&project_filter_form(
                    &params,
                    &available_projects,
                    has_no_project,
                ));
                html.push_str("</div>");
            }
            html.push_str("</section>");
            return Ok(html);
        }

        html.push_str(r#"<div class="table-shell"><table class="collection">"#);
        html.push_str(
            r#"<colgroup><col class="title"><col class="project"><col class="mode"><col class="hits"><col class="used"><col class="status"></colgroup>"#,
        );
        html.push_str("<thead><tr>");
        html.push_str(&sort_link("Title", "title", &params));
        html.push_str(&project_heading(
            &params,
            &available_projects,
            has_no_project,
        ));
        html.push_str(r#"<th scope="col">Mode</th>"#);
        html.push_str(&sort_link("Findings", "hit", &params));
        html.push_str(&sort_link("Last selected", "last_used", &params));
        html.push_str(&sort_link("Status", "status", &params));
        html.push_str("</tr></thead><tbody>");

        for row in rows {
            let last_used = row.last_selected_at.as_deref().unwrap_or("—");
            let mode = if row.blocking { "blocking" } else { "advisory" };

            html.push_str("<tr>");
            html.push_str(&title_cell(&row.id, &row.title, &row.scopes, view.param()));
            html.push_str(&project_cell(&row.scopes));
            html.push_str(&format!(
                "<td><span class=\"mode-badge {}\">{}</span></td>",
                mode, mode
            ));
            html.push_str(&format!("<td class=\"numeric\">{}</td>", row.times_applied));
            html.push_str(&format!("<td class=\"date\">{}</td>", escape(last_used)));
            html.push_str(&format!(
                "<td><span class=\"status-badge {}\">{}</span></td>",
                row.status.as_str(),
                escape(row.status.as_str())
            ));
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table></div></section>");
        Ok(html)
    })
}

fn project_options(rows: &[Learning]) -> (Vec<String>, bool) {
    let mut available = BTreeSet::new();
    let mut has_no_project = false;
    for row in rows {
        let mut any = false;
        for scope in &row.scopes {
            if scope.kind == ScopeKind::Project {
                available.insert(scope.value.clone());
                any = true;
            }
        }
        if !any {
            has_no_project = true;
        }
    }
    (available.into_iter().collect(), has_no_project)
}

fn row_matches_projects(row: &Learning, selected: &BTreeSet<&str>) -> bool {
    let mut has_project = false;
    for scope in &row.scopes {
        if scope.kind == ScopeKind::Project {
            has_project = true;
            if selected.contains(scope.value.as_str()) {
                return true;
            }
        }
    }
    !has_project && selected.contains(PROJECT_NONE)
}

fn cmp_projects(left: &[Scope], right: &[Scope]) -> std::cmp::Ordering {
    let left_keys = project_keys(left);
    let right_keys = project_keys(right);
    match (left_keys.is_empty(), right_keys.is_empty()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => left_keys.cmp(&right_keys),
    }
}

fn project_keys(scopes: &[Scope]) -> Vec<&str> {
    let mut keys: Vec<&str> = scopes
        .iter()
        .filter(|scope| scope.kind == ScopeKind::Project)
        .map(|scope| scope.value.as_str())
        .collect();
    keys.sort_unstable();
    keys
}

fn search_form(params: &Params<'_>) -> String {
    let value = params.q.map_or(String::new(), escape);
    let mut html = format!(
        r#"<form method="get" action="/collection" hx-get="/collection"{SWAP} class="search search-form" role="search">
             <input class="form-control search-input" type="search" name="q" value="{value}" placeholder="Search title, rule, and rationale…" aria-label="Search learnings">"#
    );
    html.push_str(&hidden_state_fields(params, true));
    html.push_str(r#"<button class="btn search-button" type="submit">Search</button></form>"#);
    html
}

fn mode_chips(selected: View, attention_count: usize) -> String {
    let mut html = String::from(
        r#"<nav class="ledger-modes" aria-label="Collection filters">"#,
    );
    for (label, view, count) in [
        ("Active", View::Active, None),
        (
            "Needs attention",
            View::NeedsAttention,
            Some(attention_count),
        ),
        ("Archive", View::Archive, None),
        ("All", View::All, None),
    ] {
        let current_attr = if selected == view {
            r#" aria-current="true""#
        } else {
            ""
        };
        let href = format!("/collection?view={}", view.param());
        let href = escape(&href);
        let count = count.map_or_else(String::new, |count| {
            format!(r#" <span class="mode-count">{count}</span>"#)
        });
        html.push_str(&format!(
            "<a href=\"{href}\" hx-get=\"{href}\"{SWAP} class=\"ledger-mode\"{current_attr}>{label}{count}</a>"
        ));
    }
    html.push_str("</nav>");
    html
}

fn sort_link(label: &str, field: &str, params: &Params<'_>) -> String {
    let active = params.sort == Some(field);
    let next_dir = if active && params.dir == "asc" {
        "desc"
    } else {
        "asc"
    };
    let mut href = format!("/collection?sort={field}&dir={next_dir}");
    append_q(&mut href, params.q);
    append_view(&mut href, params.view);
    append_projects(&mut href, params.projects);
    let href = escape(&href);
    let direction = if active {
        format!(r#" data-direction="{}""#, params.dir)
    } else {
        String::new()
    };
    let aria_sort = if active {
        if params.dir == "asc" {
            "ascending"
        } else {
            "descending"
        }
    } else {
        "none"
    };
    format!(
        "<th scope=\"col\" aria-sort=\"{aria_sort}\"><a class=\"sort-button\"{direction} href=\"{href}\" hx-get=\"{href}\"{SWAP}>{}{}</a></th>",
        escape(label),
        sort_caret_svg()
    )
}

fn project_heading(params: &Params<'_>, available: &[String], has_no_project: bool) -> String {
    let active = params.sort == Some("project");
    let next_dir = if active && params.dir == "asc" {
        "desc"
    } else {
        "asc"
    };
    let mut href = format!("/collection?sort=project&dir={next_dir}");
    append_q(&mut href, params.q);
    append_view(&mut href, params.view);
    append_projects(&mut href, params.projects);
    let href = escape(&href);
    let direction = if active {
        format!(r#" data-direction="{}""#, params.dir)
    } else {
        String::new()
    };
    let aria_sort = if active {
        if params.dir == "asc" {
            "ascending"
        } else {
            "descending"
        }
    } else {
        "none"
    };
    let filter_active = !params.projects.is_empty();

    let mut html = format!(
        "<th scope=\"col\" class=\"project-col\" aria-sort=\"{aria_sort}\"><div class=\"project-heading\">"
    );
    html.push_str(&format!(
        "<a class=\"sort-button\"{direction} href=\"{href}\" hx-get=\"{href}\"{SWAP}>Project{}</a>",
        sort_caret_svg()
    ));

    if available.is_empty() && !has_no_project {
        html.push_str("</div></th>");
        return html;
    }

    let summary_class = if filter_active {
        "project-filter__summary is-active"
    } else {
        "project-filter__summary"
    };
    html.push_str(&format!(
        r#"<details class="project-filter"><summary class="{summary_class}" aria-label="Filter by project">Filter</summary>"#
    ));
    html.push_str(&project_filter_form(params, available, has_no_project));
    html.push_str("</details></div></th>");
    html
}

fn project_filter_form(params: &Params<'_>, available: &[String], has_no_project: bool) -> String {
    let selected: BTreeSet<&str> = params.projects.iter().map(String::as_str).collect();
    let mut html = format!(
        r#"<form method="get" action="/collection" hx-get="/collection"{SWAP} class="project-filter__form">"#
    );
    if let Some(query) = params.q.filter(|query| !query.is_empty()) {
        html.push_str(&format!(
            r#"<input type="hidden" name="q" value="{}">"#,
            escape(query)
        ));
    }
    html.push_str(&hidden_state_fields(params, false));
    for project in available {
        let checked = if selected.contains(project.as_str()) {
            " checked"
        } else {
            ""
        };
        html.push_str(&format!(
            r#"<label class="project-filter__option"><input type="checkbox" name="project" value="{value}"{checked}><span title="{title}">{label}</span></label>"#,
            value = escape(project),
            title = escape(project),
            label = escape(project_repo_name(project)),
            checked = checked
        ));
    }
    if has_no_project {
        let checked = if selected.contains(PROJECT_NONE) {
            " checked"
        } else {
            ""
        };
        html.push_str(&format!(
            r#"<label class="project-filter__option"><input type="checkbox" name="project" value="{PROJECT_NONE}"{checked}><span>No project</span></label>"#
        ));
    }
    html.push_str(r#"<button class="btn" type="submit">Apply</button></form>"#);
    html
}

fn hidden_state_fields(params: &Params<'_>, include_projects: bool) -> String {
    let mut html = String::new();
    if let Some(sort) = params.sort {
        html.push_str(&format!(
            r#"<input type="hidden" name="sort" value="{}">"#,
            escape(sort)
        ));
    }
    html.push_str(&format!(
        r#"<input type="hidden" name="dir" value="{}">"#,
        escape(params.dir)
    ));
    html.push_str(&format!(
        r#"<input type="hidden" name="view" value="{}">"#,
        params.view.param()
    ));
    if include_projects {
        for project in params.projects {
            html.push_str(&format!(
                r#"<input type="hidden" name="project" value="{}">"#,
                escape(project)
            ));
        }
    }
    html
}

fn sort_caret_svg() -> &'static str {
    r#"<svg class="sort-caret" viewBox="0 0 12 12" fill="none" aria-hidden="true"><path d="m3 7 3-3 3 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"></path></svg>"#
}

fn append_q(href: &mut String, q: Option<&str>) {
    if let Some(query) = q.filter(|s| !s.is_empty()) {
        href.push_str(&format!("&q={}", urlencode(query)));
    }
}

fn append_view(href: &mut String, view: View) {
    href.push_str(&format!("&view={}", view.param()));
}

fn append_projects(href: &mut String, projects: &[String]) {
    for project in projects {
        href.push_str(&format!("&project={}", urlencode(project)));
    }
}

fn title_cell(id: &str, title: &str, scopes: &[Scope], origin: &str) -> String {
    let mut html = format!(
        "<td class=\"title-cell\"><a class=\"title-link\" href=\"/learnings/{}?from={}\">{}</a>",
        escape(id),
        escape(origin),
        escape(title)
    );
    let has_meta = scopes.iter().any(|scope| scope.kind != ScopeKind::Project);
    if has_meta {
        html.push_str(r#"<div class="title-meta">"#);
        for scope in scopes
            .iter()
            .filter(|scope| scope.kind != ScopeKind::Project)
        {
            html.push_str(&scope_chip(scope, "scope"));
        }
        html.push_str("</div>");
    }
    html.push_str("</td>");
    html
}

fn project_cell(scopes: &[Scope]) -> String {
    let has_projects = scopes.iter().any(|scope| scope.kind == ScopeKind::Project);
    let mut html = String::from("<td><div class=\"cell-chips\">");
    if has_projects {
        for scope in scopes
            .iter()
            .filter(|scope| scope.kind == ScopeKind::Project)
        {
            html.push_str(&scope_chip(scope, "project"));
        }
    } else {
        html.push_str(r#"<span class="muted empty-value">—</span>"#);
    }
    html.push_str("</div></td>");
    html
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
