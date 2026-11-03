// Edit with AI on a lesson's row (#1242, templates/lessons_ai_dialog.html):
// the row's ✦ opens the dialog for that lesson. Its form posts to Use AI's
// route with the lesson's file, after the table's edits are saved, and the
// editor page that comes back follows the job in its modal. A refused
// request comes back with the dialog's text and error filled in
// (data-reopen names the lesson), and the dialog opens again.
(function () {
  "use strict";
  var dialog = document.getElementById("lt-ai-dialog");
  if (!dialog || !dialog.showModal) return;
  var form = dialog.querySelector("form");
  var lesson = form.querySelector("input[name=lesson]");
  var text = form.querySelector("textarea[name=text]");
  var heading = document.getElementById("lt-ai-title");
  function open(row, refused) {
    var name = row.querySelector(".lt-title").textContent.trim();
    heading.textContent = t('courses.js.ai_edit_heading', { title: name });
    if (!refused && lesson.value !== row.dataset.path) {
      text.value = "";
      var err = form.querySelector("[role=alert]");
      if (err) err.remove();
    }
    lesson.value = row.dataset.path;
    dialog.showModal();
    text.focus();
  }
  document.addEventListener("click", function (ev) {
    var b = ev.target.closest && ev.target.closest(".lt-ai-edit");
    var row = b && b.closest(".lt-row");
    if (row && row.dataset.path) open(row, false);
  });
  var again = dialog.dataset.reopen;
  if (again) {
    var row = Array.prototype.slice.call(document.querySelectorAll(".lt-row")).filter(function (r) { return r.dataset.path === again; })[0];
    if (row) open(row, true);
    // The lesson is gone (deleted in another tab): the dialog still shows why.
    else if (form.querySelector("[role=alert]")) dialog.showModal();
  }
  // The job works on the course as saved: the table's edits go first.
  form.addEventListener("submit", function (ev) {
    var saver = window.CourseEditor && window.CourseEditor.saver;
    if (saver && saver.state() !== "saved" && !form.dataset.settled) {
      ev.preventDefault();
      saver.settle().then(function (ok) { if (!ok) return; form.dataset.settled = "1"; form.requestSubmit(); });
      return;
    }
    delete form.dataset.settled;
    var b = form.querySelector("button[type=submit]");
    if (b) { b.disabled = true; b.textContent = t('courses.js.starting'); }
  });
})();
