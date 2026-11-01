// The estimate under "How much to write" on the Generate dialog (#443):
// the sums of ai::job_tokens and estimate_line in
// src/routes/courses/generate.rs, redone as the depth (its expected lesson
// count, #1119), the material or the provider changes. Listens on the document, so a dialog drawn again
// after a save (courses.js refreshAi) keeps it.
(function () {
  "use strict";
  function estimate(form) {
    var est = form.querySelector("#gen-estimate");
    var depth = form.querySelector("input[name=depth]:checked");
    if (!est || !depth) return;
    var d = est.dataset, n = parseInt(depth.dataset.lessons, 10) || 0;
    var notesBox = form.querySelector("#gen-notes");
    var chars = notesBox ? notesBox.value.trim().length : 0;
    var num = function (v) { return parseFloat(v) || 0; };
    var secs = n * num(d.lessonSecs) + num(d.outlineSecs);
    var parts = [tn('courses.js.estimate_lessons', n), tn('courses.js.estimate_minutes', Math.max(1, Math.ceil(secs / 60)))];
    var p = form.querySelector("input[name=provider]:checked") || form.querySelector("input[type=hidden][name=provider]");
    if (p && p.dataset.priceIn) {
      var perRequest = Math.ceil((num(d.formatChars) + chars) / 4) + num(d.promptTokens);
      var input = (n + 1) * perRequest, output = n * num(d.lessonOut) + num(d.outlineOut);
      var cost = Math.ceil((input * num(p.dataset.priceIn) + output * num(p.dataset.priceOut)) / 1e6);
      if (p.dataset.month) {
        var total = num(p.dataset.month);
        parts.push(t('courses.js.estimate_share', { n: Math.min(100, Math.max(1, Math.ceil(cost * 100 / total))), hosted: d.hostedLabel || "" }));
      } else {
        var cad = Math.ceil(cost * num(d.cadCents) / 100); // to_cad
        parts.push(t('courses.js.estimate_cost', { cost: (Math.max(cad, 10000) / 1e6).toFixed(2) }));
      }
    }
    est.textContent = parts.join(t('courses.js.estimate_sep'));
  }
  function on(ev) {
    var form = ev.target && ev.target.closest && ev.target.closest("#gen-form");
    if (form) estimate(form);
  }
  document.addEventListener("change", on);
  document.addEventListener("input", function (ev) { if (ev.target && ev.target.id === "gen-notes") on(ev); });
})();
