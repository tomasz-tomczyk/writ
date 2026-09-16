// Findings list: outcome filters, and an arming step on Reject.
//
// Progressive enhancement only. Without this file the header strip reads as
// static counts, every finding is in the DOM, and every form submits on the
// first click — which is exactly what the server expects. Nothing here is
// required to read or to act on a finding.
//
// htmx replaces the whole `#findings` section on an outcome action, so the
// enhancement is re-applied on `htmx:load` as well as at startup, and the
// arming is delegated from the document so it survives a swap.
(function () {
  "use strict";

  // Spans become buttons only once a script is here to give them behaviour.
  // Rendering them as buttons server-side would ship a dead control.
  function enhanceLegend(root) {
    var legend = root.querySelector
      ? root.querySelector(".findings-legend")
      : null;
    if (root.classList && root.classList.contains("findings-legend")) {
      legend = root;
    }
    if (!legend || legend.hasAttribute("data-enhanced")) return;

    var section = legend.closest(".findings");
    if (!section) return;

    legend.setAttribute("data-enhanced", "");
    legend.setAttribute("role", "group");
    legend.setAttribute("aria-label", "Filter findings by outcome");

    legend.querySelectorAll("span.tally").forEach(function (span) {
      var button = document.createElement("button");
      button.type = "button";
      button.className = span.className;
      button.dataset.filter = span.dataset.filter;
      if (span.dataset.outcome) button.dataset.outcome = span.dataset.outcome;
      if (span.dataset.count !== undefined) {
        button.dataset.count = span.dataset.count;
      }
      button.innerHTML = span.innerHTML;
      button.setAttribute(
        "aria-pressed",
        span.dataset.filter === "all" ? "true" : "false",
      );
      span.replaceWith(button);
    });

    var filters = legend.querySelectorAll("button.tally");
    filters.forEach(function (button) {
      button.addEventListener("click", function () {
        var want = button.dataset.filter;
        filters.forEach(function (other) {
          other.setAttribute("aria-pressed", String(other === button));
        });
        section.querySelectorAll(".f-row").forEach(function (row) {
          row.hidden = want !== "all" && row.dataset.outcome !== want;
        });
      });
    });
  }

  function enhance(root) {
    if (!root || !root.querySelectorAll) return;
    enhanceLegend(root);
  }

  // Rejecting is the only human vote in the ranking and it demotes the rule
  // in every future selection, so it does not fire on a stray click. Undo is
  // restorative and is left to commit on the first click.
  var armed = null;

  function disarm() {
    if (!armed) return;
    armed.textContent = armed.dataset.label;
    armed.classList.remove("is-confirming");
    armed = null;
  }

  document.addEventListener(
    "click",
    function (event) {
      var button = event.target.closest && event.target.closest(".f-act-reject");
      if (!button) {
        disarm();
        return;
      }
      if (armed === button) {
        // Second click: let the form submit.
        disarm();
        return;
      }
      event.preventDefault();
      disarm();
      armed = button;
      if (!button.dataset.label) {
        button.dataset.label = button.textContent.trim();
      }
      button.textContent = button.dataset.confirm || "Confirm";
      button.classList.add("is-confirming");
    },
    true,
  );

  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape") disarm();
  });

  // A swap replaces the armed button's DOM node, so drop the reference.
  document.body.addEventListener("htmx:beforeSwap", function () {
    armed = null;
  });

  document.addEventListener("DOMContentLoaded", function () {
    enhance(document);
  });
  document.body.addEventListener("htmx:load", function (event) {
    enhance(event.target);
  });

  // The script is at the end of <body>, so the first paint may already be
  // past DOMContentLoaded.
  if (document.readyState !== "loading") enhance(document);
})();
