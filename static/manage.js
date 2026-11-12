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
// The Permissions dialog (#139) is filled from the row too: its preset
// ([data-preset]) picks the radio and its set ([data-perms], the bits) the
// switches. A preset's radio shows that preset's set (its own [data-perms])
// read-only, as marks; Custom shows the switches, starting from what was
// shown. A permission whose needed one ([data-needs]) is off is greyed out
// and off. The server decides what is stored; this only shows it.
//
// Several accounts get one preset at once (#141): a checkbox per row the
// viewer may edit, the header's box for all of them, a bar above the table
// while any is ticked, and a confirm dialog before the table's form is sent.
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

  var permsDialog = document.getElementById("manage-permissions-dialog");
  var permsForm = permsDialog && permsDialog.querySelector("form");
  var permRadios = permsForm ? Array.prototype.slice.call(permsForm.querySelectorAll("[data-testid=manage-permissions-preset]")) : [];
  var permLines = permsForm ? Array.prototype.slice.call(permsForm.querySelectorAll("[data-testid=manage-permission]")) : [];
  function permBox(line) { return line.querySelector("[data-testid=manage-permission-toggle]"); }
  // The switches show the set `bits`.
  function showPerms(bits) {
    permLines.forEach(function (line) { permBox(line).checked = (bits & Number(line.dataset.bit)) !== 0; });
  }
  // After a radio or a switch changed: read-only for a preset, and the
  // lines whose needed permission is off greyed out (and off).
  function refreshPerms() {
    var custom = permRadios.some(function (r) { return r.checked && r.dataset.preset === "custom"; });
    permsForm.classList.toggle("is-readonly", !custom);
    permLines.forEach(function (line) {
      var box = permBox(line);
      var needed = line.dataset.needs && permsForm.querySelector('[data-perm="' + line.dataset.needs + '"]');
      var off = !!needed && !permBox(needed).checked;
      if (off) box.checked = false;
      box.disabled = !custom || off;
      line.classList.toggle("is-off", custom && off);
      if (custom && off) line.setAttribute("aria-disabled", "true");
      else line.removeAttribute("aria-disabled");
      line.dataset.on = String(box.checked);
      line.querySelector("[data-testid=manage-permission-mark]").setAttribute("aria-label", t(box.checked ? "manage.js.perm_on" : "manage.js.perm_off"));
    });
  }
  // Opened from a row: its preset's radio (Custom when it has none) and its set.
  function fillPermissions(row) {
    if (!row) return;
    var radio = permRadios.filter(function (r) { return r.dataset.preset === row.dataset.preset; })[0];
    permRadios.forEach(function (r) { r.checked = radio ? r === radio : r.dataset.preset === "custom"; });
    showPerms(Number(row.dataset.perms) || 0);
    refreshPerms();
  }
  permRadios.forEach(function (r) {
    r.addEventListener("change", function () {
      if (r.dataset.perms !== undefined) showPerms(Number(r.dataset.perms) || 0);
      refreshPerms();
    });
  });
  permLines.forEach(function (line) { permBox(line).addEventListener("change", refreshPerms); });

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
      if (dialog === permsDialog) fillPermissions(b.closest("[data-testid=manage-account-row]"));
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

  // One preset for several accounts (#141). The table is a form; the bar
  // above it shows while a row is ticked, with how many. The header's box
  // ticks or unticks every row and is mixed while only some are ticked.
  // "Apply to selected" asks first, in a dialog whose button sends the form.
  // The server decides who is skipped; this only counts what is ticked.
  var bulkForm = document.querySelector("[data-testid=manage-bulk-form]");
  var bulkDialog = document.getElementById("manage-bulk-dialog");
  if (bulkForm && bulkDialog) {
    var bulkBar = bulkForm.querySelector("[data-testid=manage-bulk-bar]");
    var bulkCount = bulkForm.querySelector("[data-testid=manage-bulk-count]");
    var bulkAll = bulkForm.querySelector("[data-testid=manage-account-select-all]");
    var bulkPreset = bulkForm.querySelector("[data-testid=manage-bulk-preset]");
    var bulkBoxes = Array.prototype.slice.call(bulkForm.querySelectorAll("[data-testid=manage-account-select]"));
    var bulkTicked = function () { return bulkBoxes.filter(function (b) { return b.checked; }).length; };
    var bulkRefresh = function () {
      var n = bulkTicked();
      bulkBar.hidden = n === 0;
      bulkCount.textContent = tn("manage.js.bulk_selected", n);
      bulkAll.checked = n > 0 && n === bulkBoxes.length;
      bulkAll.indeterminate = n > 0 && n < bulkBoxes.length;
    };
    bulkAll.hidden = bulkBoxes.length === 0;
    bulkAll.addEventListener("change", function () {
      bulkBoxes.forEach(function (b) { b.checked = bulkAll.checked; });
      bulkRefresh();
    });
    bulkBoxes.forEach(function (b) { b.addEventListener("change", bulkRefresh); });
    bulkForm.querySelector("[data-testid=manage-bulk-apply]").addEventListener("click", function (e) {
      e.preventDefault();
      var n = bulkTicked();
      var chosen = bulkPreset.options[bulkPreset.selectedIndex];
      if (!n || !chosen) return;
      bulkDialog.querySelector("[data-testid=manage-bulk-question]").textContent = tn("manage.js.bulk_confirm", n).replace("{preset}", function () { return chosen.textContent; });
      if (!bulkDialog.open) bulkDialog.showModal();
    });
    // A page come back to (Back, a reload) keeps its ticks.
    window.addEventListener("pageshow", bulkRefresh);
    bulkRefresh();
  }

  // Saved presets (#140). The Permissions dialog's "Save as a preset" box
  // starts empty each time the dialog opens (style.css shows it only while
  // Custom is chosen). A saved preset's Delete fills the confirm dialog with
  // its id and name; the [data-opens] handler above opens it.
  var saveAs = permsForm && permsForm.querySelector("[data-testid=manage-permissions-save-as]");
  document.querySelectorAll("[data-opens=manage-permissions-dialog]").forEach(function (b) {
    b.addEventListener("click", function () { if (saveAs) saveAs.value = ""; });
  });
  document.querySelectorAll("[data-testid=manage-preset-delete]").forEach(function (b) {
    b.addEventListener("click", function () {
      var dialog = document.getElementById(b.dataset.opens);
      if (!dialog) return;
      dialog.querySelector("[data-preset-field]").value = b.dataset.presetId;
      dialog.querySelector("[data-preset-name]").textContent = b.dataset.presetName;
    });
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
        if (data && data.ollama_model) {
          var shown = document.querySelector("[data-testid=manage-ollama-model-shown]");
          if (shown) shown.textContent = data.ollama_model;
        }
      },
    });
    inputs.forEach(function (input) {
      saver.watch(input, { field: input.name, delay: 3000, burst: 6, ignoreEmpty: true });
    });
    var model = document.querySelector("[data-testid=manage-ollama-model]");
    if (model) saver.watch(model, { field: "ollama_model", delay: 1500, ignoreEmpty: true });
  }
  // A saved key: the row says "Set" with its last four characters (an
  // Ollama row its host and model) and the box empties: a key is never shown again.
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
    var model = row.querySelector("[data-testid=manage-ollama-model]");
    if (model) {
      code.textContent = hint;
      var name = document.createElement("code");
      name.dataset.testid = "manage-ollama-model-shown";
      name.textContent = model.value.trim() || model.placeholder;
      status.append(ok, " · ", code, " · ", name);
      return;
    }
    code.textContent = "\u2026" + hint;
    status.append(ok, " · " + t("manage.js.ends_in") + " ", code);
    row.querySelector("[data-key-prompt]").textContent = t("manage.js.paste_new_key");
  }
})();
