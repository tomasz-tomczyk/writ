use writ_core::{Error, ExemplarKind, ListFilter, ScopeKind, Status};

use crate::ui::AppState;
use crate::ui::pages::layout::{escape, ledger_empty, scope_chip, with_store};

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

        let mut html =
            String::from(r#"<div class="inbox" id="inbox-list"><section class="inbox-ledger">"#);

        if proposed.is_empty() {
            html.push_str(&ledger_empty(
                "Inbox is empty",
                "Every proposal is curated.",
            ));
            html.push_str("</section></div>");
            return Ok(html);
        }

        html.push_str(r#"<div class="table-shell proposal-list">"#);
        for learning in proposed {
            let exemplars = store.exemplars_of(&learning.id)?;

            html.push_str("<article class=\"proposal\">");
            html.push_str(&format!(
                "<a class=\"proposal-open\" href=\"/learnings/{}\" aria-label=\"Open {}\"></a>",
                escape(&learning.id),
                escape(&learning.title)
            ));
            html.push_str("<div class=\"proposal-header\"><div>");
            html.push_str(&format!(
                "<h2 class=\"proposal-title\">{}</h2>",
                escape(&learning.title)
            ));
            html.push_str("<div class=\"proposal-meta title-meta\">");
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
            for scope in learning
                .scopes
                .iter()
                .filter(|scope| scope.kind != ScopeKind::Project)
            {
                html.push_str(&scope_chip(scope, "scope"));
            }
            html.push_str("</div></div>");
            let projects: Vec<_> = learning
                .scopes
                .iter()
                .filter(|scope| scope.kind == ScopeKind::Project)
                .collect();
            if !projects.is_empty() {
                html.push_str(r#"<div class="proposal-project cell-chips">"#);
                for scope in projects {
                    html.push_str(&scope_chip(scope, "project"));
                }
                html.push_str("</div>");
            }
            html.push_str("</div>");
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
                    "<p class=\"matcher-row\"><span class=\"chip matcher\"><span class=\"scope-kind\">{}:</span>{}</span></p>",
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
                "<form method=\"post\" action=\"{approve}\" hx-post=\"{approve}\"{SWAP}><button type=\"submit\" class=\"btn btn--primary primary\">Approve</button></form>"
            ));
            let reject = format!("/inbox/{}/reject", escape(&learning.id));
            html.push_str(&format!(
                "<form method=\"post\" action=\"{reject}\" hx-post=\"{reject}\"{SWAP}><button type=\"submit\" class=\"btn danger\">Reject</button></form>"
            ));
            html.push_str(&format!(
                "<a href=\"/learnings/{}\" class=\"btn button\">Edit</a>",
                escape(&learning.id)
            ));
            html.push_str("</div>");

            html.push_str("</article>");
        }
        html.push_str("</div></section></div>");
        Ok(html)
    })
}
