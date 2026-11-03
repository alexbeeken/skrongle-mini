// The lessons list filter: every word typed must appear in a lesson's title,
// group or summary (accents and case ignored). The page shows one unit (#837),
// so the first word typed fetches every unit once and the filter runs over
// all of them; clearing the box puts the unit back. Units with no match are
// hidden, and a search that finds nothing says so. A link under the box
// looks the same words up in the course's vocabulary (/lookup, #348).
(function () {
  "use strict";
  var input = document.getElementById("lessons-filter");
  var status = document.getElementById("lessons-filter-status");
  var shown = document.getElementById("lessons-shown");
  var pager = document.getElementById("lessons-pager");
  var lookup = document.getElementById("lessons-lookup");
  // On a phone the tabs scroll sideways: bring the open one into view.
  document.querySelectorAll(".pick-tabs").forEach(function (nav) {
    var cur = nav.querySelector("[aria-current]");
    if (cur) nav.scrollLeft = cur.offsetLeft - (nav.clientWidth - cur.offsetWidth) / 2;
  });
  if (!input || !shown) return;
  var base = document.body.dataset.base || "";
  var oneUnit = shown.dataset.search !== "true";
  var unitHtml = shown.innerHTML, allHtml = null, loading = null, items = [], units = [];
  function fold(s) { return s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase(); }
  function collect() {
    items = Array.prototype.map.call(shown.querySelectorAll("[data-testid=lessons-item]"), function (li) {
      return { li: li, text: fold(li.dataset.search || li.textContent) };
    });
    units = shown.querySelectorAll("[data-testid=lessons-unit]");
  }
  // Every unit's lessons, from /lessons?unit=all (once).
  function loadAll() {
    if (!loading) {
      loading = fetch(base + "/lessons?unit=all", { credentials: "same-origin" })
        .then(function (r) { if (!r.ok) throw new Error(r.status); return r.text(); })
        .then(function (html) {
          var box = new DOMParser().parseFromString(html, "text/html").getElementById("lessons-shown");
          allHtml = box ? box.innerHTML : unitHtml;
        })
        .catch(function () { allHtml = unitHtml; });
    }
    return loading;
  }
  function show(html) {
    if (shown.innerHTML === html) return;
    shown.innerHTML = html;
    collect();
  }
  function apply() {
    var words = fold(input.value).split(/\s+/).filter(Boolean);
    if (oneUnit) {
      if (words.length && allHtml === null) { loadAll().then(apply); return; }
      show(words.length ? allHtml : unitHtml);
      if (pager) pager.hidden = words.length > 0;
    }
    var count = 0;
    items.forEach(function (it) {
      var hit = words.every(function (w) { return it.text.indexOf(w) >= 0; });
      it.li.hidden = !hit;
      if (hit) count += 1;
    });
    units.forEach(function (u) { u.hidden = !u.querySelector("[data-testid=lessons-item]:not([hidden])"); });
    // The same words looked up in the course's vocabulary (#348).
    if (lookup) {
      var query = input.value.trim();
      lookup.hidden = !words.length;
      lookup.href = base + "/lookup?q=" + encodeURIComponent(query);
      lookup.textContent = t('study.js.lookup_link', { query: query });
    }
    status.dataset.matches = String(count);
    status.textContent = !words.length ? "" : count ? t('study.js.filter_matches', { shown: count, total: items.length }) : t('study.js.filter_none', { query: input.value.trim() });
  }
  collect();
  input.addEventListener("input", apply);
  // Enter filters in place; the form's ?q= is for pages without script.
  input.form.addEventListener("submit", function (e) { e.preventDefault(); apply(); });
  if (input.value.trim()) apply();
  // "Back to top" shows once the page has scrolled past the first screen.
  var top = document.querySelector("[data-testid=lessons-to-top]");
  if (top) {
    var onScroll = function () { top.hidden = window.scrollY < window.innerHeight; };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
  }
})();
