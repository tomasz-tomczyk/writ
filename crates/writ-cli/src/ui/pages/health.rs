use std::collections::HashMap;

use writ_core::{Error, Learning, ListFilter, ScopeKind, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, ledger_empty, scope_chip, with_store};

const UNUSED_DAYS: u32 = 90;

/// The element every Health action swaps.
const SWAP: &str = r##" hx-target="#health-body" hx-swap="outerHTML""##;

struct HealthRow {
    learning: Learning,
    unused: bool,
    never_applied: bool,
}

pub fn render(state: &AppState) -> Result<String, Error> {
    with_store(state, |store| {
        let unused: Vec<Learning> = store.list(&ListFilter {
            status: Some(Status::Active),
            unused_days: Some(UNUSED_DAYS),
            ..Default::default()
        })?;
        let never_applied: Vec<Learning> = store.list(&ListFilter {
            status: Some(Status::Active),
            never_applied: true,
            ..Default::default()
        })?;

        let mut rows: HashMap<String, HealthRow> = HashMap::new();
        for learning in unused {
            rows.entry(learning.id.clone())
                .or_insert_with(|| HealthRow {
                    learning,
                    unused: false,
                    never_applied: false,
                })
                .unused = true;
        }
        for learning in never_applied {
            rows.entry(learning.id.clone())
                .or_insert_with(|| HealthRow {
                    learning,
                    unused: false,
                    never_applied: false,
                })
                .never_applied = true;
        }

        let mut html = String::from(r#"<div id="health-body"><section class="health-ledger">"#);

        if rows.is_empty() {
            html.push_str(&ledger_empty(
                "Health is clear",
                "Every active rule has been reached and applied within the last 90 days.",
            ));
            html.push_str("</section></div>");
            return Ok(html);
        }

        let mut ordered: Vec<HealthRow> = rows.into_values().collect();
        ordered.sort_by(|left, right| left.learning.title.cmp(&right.learning.title));

        html.push_str(r#"<div class="table-shell"><table class="health collection">"#);
        html.push_str(
            r#"<colgroup><col class="title"><col class="bucket"><col class="used"><col class="hits"><col class="actions"></colgroup>"#,
        );
        html.push_str(
            "<thead><tr><th scope=\"col\">Title</th><th scope=\"col\">Bucket</th><th scope=\"col\">Last selected</th><th scope=\"col\">Hits</th><th scope=\"col\"><span class=\"sr-only\">Actions</span></th></tr></thead><tbody>",
        );
        for row in ordered {
            let class = if row.unused && row.never_applied {
                "in-both"
            } else {
                ""
            };
            let bucket_label = bucket_label(row.unused, row.never_applied);
            let bucket_class = bucket_class(row.unused, row.never_applied);
            let learning = &row.learning;
            html.push_str(&format!("<tr class=\"{}\">", class));
            html.push_str(&format!(
                "<td class=\"title-cell\"><a class=\"title-link\" href=\"/learnings/{}\">{}</a>",
                escape(&learning.id),
                escape(&learning.title)
            ));
            let meta: Vec<_> = learning
                .scopes
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
            html.push_str(&format!(
                "<td><span class=\"status-badge {bucket_class}\">{}</span></td>",
                escape(bucket_label)
            ));
            html.push_str(&format!(
                "<td class=\"date\">{}</td>",
                escape(learning.last_selected_at.as_deref().unwrap_or("—"))
            ));
            html.push_str(&format!(
                "<td class=\"numeric\">{}</td>",
                learning.times_applied
            ));
            html.push_str("<td class=\"actions\">");
            let archive = format!("/health/{}/archive", escape(&learning.id));
            html.push_str(&format!(
                "<form method=\"post\" action=\"{archive}\" hx-post=\"{archive}\"{SWAP}><button type=\"submit\" class=\"btn danger\">Archive</button></form>"
            ));
            html.push_str(&format!(
                "<a href=\"/health/{}/edit\" class=\"btn button\">Edit</a>",
                escape(&learning.id)
            ));
            html.push_str(&format!(
                "<a href=\"/health/{}/keep\" hx-get=\"/health/{}/keep\"{SWAP} class=\"btn button keep\">Keep</a>",
                escape(&learning.id),
                escape(&learning.id)
            ));
            html.push_str("</td></tr>");
        }
        html.push_str("</tbody></table></div></section></div>");
        Ok(html)
    })
}

fn bucket_label(unused: bool, never_applied: bool) -> &'static str {
    match (unused, never_applied) {
        (true, true) => "Not selected in 90 days + selected but never applied",
        (true, false) => "Not selected in 90 days",
        (false, true) => "Selected but never applied",
        (false, false) => "",
    }
}

fn bucket_class(unused: bool, never_applied: bool) -> &'static str {
    match (unused, never_applied) {
        (true, true) => "archived",
        (true, false) | (false, true) => "proposed",
        (false, false) => "active",
    }
}
