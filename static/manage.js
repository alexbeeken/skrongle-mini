// The Manage Users page (/manage). A row's action button fills its confirm
// dialog with the row's address (or a course's id and title, #1034) and
// opens it, on top of the course's detail dialog when it comes from there;
// [data-closes] or Escape closes a dialog; one the server rendered open ([data-open-on-load]: a temporary
// password just set) is made modal. Copy puts a new invitation link or
// temporary password on the clipboard (https or localhost only); elsewhere it
// selects the text to copy by hand. A new link or password is the answer to
// a POST, rendered in place; the address bar is set back to the page so a
// reload asks for the page again instead of making another one (#665).
//
// The server's AI API keys (#25) save as they are pasted or typed
// (static/autosave.js, POST /api/manage/keys): a pasted key at once, a typed
// one 3 s after the last keystroke. Saved, the box empties (a key is never
// shown again) and the row says which key is on file.
(function () {
  "use strict";

  var page = document.querySelector("[data-page-url]");
  if (page && location.pathname !== page.dataset.pageUrl) history.replaceState(null, "", page.dataset.pageUrl);

  document.querySelectorAll("[data-copy]").forEach(function (b) {
    b.addEventListener("click", function () {
      var field = document.getElementById(b.dataset.copy);
      if (!field) return;
      var done = function () { b.textContent = t("manage.js.copied"); };
      var select = function () { field.focus(); field.select(); };
      if (navigator.clipboard && window.isSecureContext) navigator.clipboard.writeText(field.value).then(done, select);
      else select();
    });
  });

  document.querySelectorAll("[data-opens]").forEach(function (b) {
    var dialog = document.getElementById(b.dataset.opens);
    if (!dialog) return;
    b.addEventListener("click", function (e) {
      e.preventDefault();
      if (b.dataset.accountEmail) {
        var field = dialog.querySelector("[data-account-field]");
        var name = dialog.querySelector("[data-account-name]");
        if (field) field.value = b.dataset.accountEmail;
        if (name) name.textContent = b.dataset.accountEmail;
      }
      if (b.dataset.courseId) {
        var course = dialog.querySelector("[data-course-field]");
        var title = dialog.querySelector("[data-course-name]");
        if (course) course.value = b.dataset.courseId;
        if (title) title.textContent = (b.dataset.courseName || "") + " (" + b.dataset.courseId + ")";
      }
      if (!dialog.open) dialog.showModal();
    });
  });

  document.querySelectorAll("dialog.modal").forEach(function (dialog) {
    dialog.querySelectorAll("[data-closes]").forEach(function (b) {
      b.addEventListener("click", function () { dialog.close(); });
    });
    if (dialog.hasAttribute("data-open-on-load")) { dialog.removeAttribute("open"); dialog.showModal(); }
  });

  var inputs = document.querySelectorAll("[data-testid=manage-api-key-input]");
  if (inputs.length && window.Autosave) {
    var base = document.body.dataset.base || "";
    var saver = Autosave({
      url: base + "/api/manage/keys",
      delay: 3000,
      status: document.querySelector("[data-testid=manage-keys-save-status]"),
      message: document.querySelector("[data-testid=manage-keys-save-error]"),
      onSaved: function (data) {
        if (data && data.keys) Object.keys(data.keys).forEach(function (p) { keySaved(p, data.keys[p]); });
      },
    });
    inputs.forEach(function (input) {
      saver.watch(input, { field: input.name, delay: 3000, burst: 6, ignoreEmpty: true });
    });
  }
  function keySaved(provider, hint) {
    var row = document.querySelector('[data-testid=manage-api-key][data-provider="' + provider + '"]');
    if (!row) return;
    row.dataset.keySet = "true";
    row.dataset.unreadable = "false";
    row.querySelector("[data-testid=manage-api-key-input]").value = "";
    var status = row.querySelector("[data-testid=manage-api-key-status]");
    status.textContent = "";
    var ok = document.createElement("span");
    ok.className = "ok-text";
    ok.textContent = t("manage.js.key_on_file");
    var code = document.createElement("code");
    code.textContent = "\u2026" + hint;
    status.append(ok, " · " + t("manage.js.ends_in") + " ", code);
    row.querySelector("[data-key-prompt]").textContent = t("manage.js.paste_new_key");
  }
})();
