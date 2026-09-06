use std::collections::HashMap;

use writ_core::{Error, Learning, ListFilter, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, with_store};

const UNUSED_DAYS: u32 = 90;

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

        if rows.is_empty() {
            return Ok(r#"<div class="shell empty">Health is clear. Every active rule has been reached and applied within the last 90 days.</div>"#.into());
        }

        let mut ordered: Vec<HealthRow> = rows.into_values().collect();
        ordered.sort_by(|left, right| left.learning.title.cmp(&right.learning.title));

        let mut html = String::from(r#"<table class="health">"#);
        html.push_str("<thead><tr><th>Learning</th><th>Bucket</th><th>Selected</th><th>Applied</th><th></th></tr></thead><tbody>");
        for row in ordered {
            let class = if row.unused && row.never_applied {
                "in-both"
            } else {
                ""
            };
            let bucket_label = bucket_label(row.unused, row.never_applied);
            let learning = &row.learning;
            html.push_str(&format!("<tr class=\"{}\">", class));
            html.push_str(&format!(
                "<td><a href=\"/learnings/{}\">{}</a></td>",
                escape(&learning.id),
                escape(&learning.title)
            ));
            html.push_str(&format!("<td>{}</td>", escape(bucket_label)));
            html.push_str(&format!(
                "<td>{}</td>",
                learning.last_selected_at.as_deref().unwrap_or("—")
            ));
            html.push_str(&format!("<td>{}</td>", learning.times_applied));
            html.push_str("<td class=\"actions\">");
            html.push_str(&format!(
                "<form method=\"post\" action=\"/learnings/{}/archive\"><button type=\"submit\" class=\"danger\">Archive</button></form>",
                escape(&learning.id)
            ));
            html.push_str(&format!(
                "<a href=\"/learnings/{}\" class=\"button\">Edit</a>",
                escape(&learning.id)
            ));
            html.push_str("<a href=\"/health\" class=\"button keep\">Keep</a>");
            html.push_str("</td></tr>");
        }
        html.push_str("</tbody></table>");
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
