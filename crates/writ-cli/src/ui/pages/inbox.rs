use writ_core::{Error, ExemplarKind, ListFilter, NearMatch, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, with_store};

/// Render the Inbox page body.
pub fn render(state: &AppState) -> Result<String, Error> {
    with_store(state, |store| {
        let proposed = store.list(&ListFilter {
            status: Some(Status::Proposed),
            ..Default::default()
        })?;

        if proposed.is_empty() {
            return Ok(
                r#"<div class="shell empty">Inbox is empty. Every proposal is curated.</div>"#
                    .into(),
            );
        }

        let mut html = String::from(r#"<div class="inbox">"#);
        for learning in proposed {
            let exemplars = store.exemplars_of(&learning.id)?;
            let near = store.near_matches(
                &format!("{} {}", learning.title, learning.rule),
                state.config.dedupe.warn_top_n,
            )?;
            let near: Vec<NearMatch> = near.into_iter().filter(|m| m.id != learning.id).collect();

            html.push_str("<article class=\"proposal\">");
            html.push_str(&format!("<h2>{}</h2>", escape(&learning.title)));
            html.push_str(&format!(
                "<p class=\"rule\"><strong>Rule:</strong> {}</p>",
                escape(&learning.rule)
            ));
            html.push_str(&format!(
                "<p class=\"rationale\"><strong>Why:</strong> {}</p>",
                escape(&learning.rationale)
            ));

            if let (Some(kind), Some(pattern)) = (learning.matcher_kind, learning.matcher.as_ref())
            {
                html.push_str(&format!(
                    "<p><span class=\"badge matcher\">{}: {}</span></p>",
                    escape(kind.as_str()),
                    escape(pattern)
                ));
            }

            if !exemplars.is_empty() {
                html.push_str("<div class=\"exemplars\">");
                for exemplar in exemplars {
                    let label = match exemplar.kind {
                        ExemplarKind::Good => "good",
                        ExemplarKind::Bad => "bad",
                    };
                    html.push_str(&format!(
                        "<div class=\"exemplar {}\"><h4>{}</h4><pre><code>{}</code></pre></div>",
                        label,
                        label,
                        escape(&exemplar.snippet)
                    ));
                }
                html.push_str("</div>");
            }

            if !near.is_empty() {
                html.push_str(&format!(
                    "<form method=\"post\" action=\"/inbox/{}/merge\" class=\"merge\">",
                    escape(&learning.id)
                ));
                html.push_str("<fieldset><legend>Near matches</legend>");
                for m in near {
                    html.push_str(&format!(
                        "<label class=\"near-match\"><input type=\"radio\" name=\"target_id\" value=\"{}\" required> {} <span class=\"chip\">{}</span></label>",
                        escape(&m.id),
                        escape(&m.title),
                        escape(m.status.as_str())
                    ));
                }
                html.push_str("</fieldset>");
                html.push_str("<button type=\"submit\">Merge into selected</button>");
                html.push_str("</form>");
            }

            html.push_str("<div class=\"actions\">");
            html.push_str(&format!(
                "<form method=\"post\" action=\"/inbox/{}/approve\"><button type=\"submit\" class=\"primary\">Approve</button></form>",
                escape(&learning.id)
            ));
            html.push_str(&format!(
                "<form method=\"post\" action=\"/inbox/{}/reject\"><button type=\"submit\" class=\"danger\">Reject</button></form>",
                escape(&learning.id)
            ));
            html.push_str(&format!(
                "<a href=\"/learnings/{}\" class=\"button\">Edit</a>",
                escape(&learning.id)
            ));
            html.push_str("</div>");

            html.push_str("</article>");
        }
        html.push_str("</div>");
        Ok(html)
    })
}
