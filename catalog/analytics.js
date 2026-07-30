/* Hosted, privacy-first analytics (Plausible) -- decision-log entry 45
   (registry#133). No cookies, no PII, no consent banner needed.

   Requires this site to actually be added in a Plausible account for
   registry.traverse-framework.com -- this repo doesn't manage that account
   (creating third-party accounts isn't something this codebase can do for
   you). Until that's done, this script sends events to an unregistered
   domain and Plausible silently drops them; nothing breaks either way.

   Shared by catalog/index.html (the SPA) and every static page
   scripts/ci/generate_catalog_pages.py generates, so both surfaces report
   consistently under one script instead of drifting apart. */
(function () {
  var script = document.createElement("script");
  script.defer = true;
  script.setAttribute("data-domain", "registry.traverse-framework.com");
  script.src = "https://plausible.io/js/script.js";
  document.head.appendChild(script);

  window.plausible = window.plausible || function () {
    (window.plausible.q = window.plausible.q || []).push(arguments);
  };
})();

/* Manually attributes a pageview to `url` (a full URL, per Plausible's `u`
   override) instead of the browser's actual location -- used so SPA
   hash-navigation to a capability's detail view reports under the same
   real permalink URL its static page uses, rather than fragmenting the
   same capability's views across two different "pages" in the dashboard. */
function trackPageview(url) {
  window.plausible("pageview", url ? { u: url } : undefined);
}

/* Custom event capturing what was searched and how many results it
   returned. Zero-result searches are the strongest signal of unmet demand
   (decision-log entry 45) -- deliberately not filtered out or specially
   flagged here beyond the numeric `results` prop, since Plausible's own
   dashboard can filter/segment by a custom event property directly. */
function trackSearch(query, resultCount) {
  window.plausible("Search", { props: { query: query, results: resultCount } });
}
