// A course card's stat modal on /courses (#900): the body says what is next,
// fetched from GET /courses/{id}/stat/{metric} each time the modal opens
// (courses.js's [data-open-dialog] handler fires "dialog-open"). Its own file
// so courses.js does not grow past its line budget.
(function () {
  "use strict";
  // /courses itself only counts; fetched again on each open: it is small.
  document.addEventListener("dialog-open", function (ev) {
    var d = ev.target;
    var box = d && d.dataset && d.dataset.detail && d.querySelector(".stat-detail");
    if (!box) return;
    box.setAttribute("aria-busy", "true");
    fetch(d.dataset.detail, { credentials: "same-origin" })
      .then(function (r) { if (!r.ok) throw new Error("HTTP " + r.status); return r.text(); })
      .then(function (html) { box.innerHTML = html; })
      .catch(function () { box.innerHTML = ""; var p = document.createElement("p"); p.className = "small bad-text"; p.textContent = t('courses.js.stat_failed'); box.appendChild(p); })
      .then(function () { box.removeAttribute("aria-busy"); });
  }, true);
})();
