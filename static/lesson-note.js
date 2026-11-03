// "My note" on the lesson page (#336): the learner's own note on the
// lesson, saved as they type (static/autosave.js) to
// POST /api/lessons/{slug}/note. An emptied note is deleted.
(function () {
  "use strict";
  var box = document.querySelector("[data-testid=lesson-note]");
  if (!box || !window.Autosave) return;
  var text = box.querySelector("[data-testid=lesson-note-text]");
  var saver = Autosave({
    url: box.getAttribute("data-url"),
    delay: 1000,
    status: box.querySelector("[data-testid=lesson-note-status]"),
    message: box.querySelector("[data-testid=lesson-note-save-error]"),
  });
  saver.watch(text, { field: "note" });
})();
