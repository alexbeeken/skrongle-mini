// A copy of a lesson for the course editor's ⧉ (#454, static/courses.js
// `duplicate`): the parsed lesson file `doc` as a new lesson titled
// `title`, or null when `doc` is not a lesson object. `taken` lists the
// table's lesson ids. The copy's id gets a "-copy" ending ("-copy-2" and so
// on when taken), and so do its slug, question ids and conversation ids
// (themed ones too), which must be unique in the course; the guide's
// question steps follow them. Conjugations are left out, since one item and
// set may be taught by one lesson only. The rest, prerequisites included,
// stays as it was.
(function () {
  "use strict";
  function copyOf(doc, taken, title) {
    if (!doc || typeof doc !== "object" || Array.isArray(doc)) return null;
    var from = typeof doc.id === "string" && doc.id ? doc.id : "lesson";
    var end = "-copy";
    for (var n = 2; taken.indexOf(from + end) >= 0; n++) end = "-copy-" + n;
    doc.id = from + end;
    if (typeof doc.slug === "string" && doc.slug) doc.slug += end;
    doc.title = title;
    delete doc.conjugations;
    var renamed = {};
    var endIds = function (list, key) {
      if (!Array.isArray(list)) return;
      list.forEach(function (x) {
        if (x && typeof x[key] === "string" && x[key]) { renamed[x[key]] = x[key] + end; x[key] += end; }
      });
    };
    endIds(doc.quiz, "question_id");
    endIds(doc.conversations, "id");
    if (doc.themes && typeof doc.themes === "object") {
      Object.keys(doc.themes).forEach(function (k) {
        var th = doc.themes[k];
        if (th && typeof th === "object") { endIds(th.quiz, "question_id"); endIds(th.conversations, "id"); }
      });
    }
    if (Array.isArray(doc.guide)) {
      doc.guide.forEach(function (step) { if (step && renamed[step.question]) step.question = renamed[step.question]; });
    }
    return doc;
  }
  window.LessonDuplicate = { copyOf: copyOf };
})();
