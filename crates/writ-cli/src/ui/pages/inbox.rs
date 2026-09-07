use writ_core::{Error, ExemplarKind, ListFilter, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, with_store};

/// The element every Inbox action swaps.
const SWAP: &str = r##" hx-target="#inbox-list" hx-swap="outerHTML""##;

/// Render the Inbox page body.
///
/// Merge-into is deferred: the UI offers Approve, Reject, and Edit only.
pub fn render(state: &AppState) -> Result<String, Error> {
    with_store(state, |store| {
        let proposed = store.list(&ListFilter {
            status: Some(Status::Proposed),
            ..Default::default()
        })?;

        let mut html = String::from(r#"<div class="inbox" id="inbox-list">"#);

        if proposed.is_empty() {
            html.push_str(
                r#"<div class="shell empty"><strong>Inbox is empty</strong><span>Every proposal is curated.</span></div>"#,
            );
            html.push_str("</div>");
            return Ok(html);
        }

        for learning in proposed {
            let exemplars = store.exemplars_of(&learning.id)?;

            html.push_str("<article class=\"proposal\">");
            html.push_str("<div class=\"proposal-header\"><div>");
            html.push_str(&format!("<h2>{}</h2>", escape(&learning.title)));
            html.push_str("<div class=\"proposal-meta\">");
            html.push_str(r#"<span class="status-badge proposed">proposed</span>"#);
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
            for scope in &learning.scopes {
                html.push_str(&format!(
                    "<span class=\"chip scope\">{}</span>",
                    escape(&scope.to_string())
                ));
            }
            html.push_str("</div></div></div>");
            html.push_str(&format!(
                "<p class=\"rule\"><strong>Rule</strong><span>{}</span></p>",
                escape(&learning.rule)
            ));
            html.push_str(&format!(
                "<p class=\"rationale\"><strong>Why</strong><span>{}</span></p>",
                escape(&learning.rationale)
            ));

            if let (Some(kind), Some(pattern)) = (learning.matcher_kind, learning.matcher.as_ref())
            {
                html.push_str(&format!(
                    "<p class=\"matcher-row\"><span class=\"badge matcher\">{}: {}</span></p>",
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

            html.push_str("<div class=\"actions\">");
            let approve = format!("/inbox/{}/approve", escape(&learning.id));
            html.push_str(&format!(
                "<form method=\"post\" action=\"{approve}\" hx-post=\"{approve}\"{SWAP}><button type=\"submit\" class=\"primary\">Approve</button></form>"
            ));
            let reject = format!("/inbox/{}/reject", escape(&learning.id));
            html.push_str(&format!(
                "<form method=\"post\" action=\"{reject}\" hx-post=\"{reject}\"{SWAP}><button type=\"submit\" class=\"danger\">Reject</button></form>"
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
