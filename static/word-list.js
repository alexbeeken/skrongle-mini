// "Paste a word list" in the course editor (#453). Lines like
// "term — meaning", "term, meaning" or a tab-separated export become a new
// lesson's vocabulary, with no model involved. The server reads the lines
// (POST /api/courses/{id}/words) as they are typed; the dialog shows the
// pairs and the lines it could not read. Add puts the lesson at the end of
// the last unit through the table's own + and rename (static/courses.js),
// with the lesson's JSON in the new row, so the table saves it like any
// other new lesson. Language courses only: the button follows the gear's
// Language features switch.
(function () {
  "use strict";
  var base = document.body.dataset.base || "";
  var panel = document.getElementById("lessons");
  var btn = document.getElementById("lt-paste-words");
  var dlg = document.getElementById("lt-words-dialog");
  if (!panel || !btn || !dlg) return;
  var course = panel.dataset.courseId;
  var list = document.getElementById("lt-list");
  var langSwitch = document.querySelector(".lt-switch[data-pref=language]");
  var showBtn = function () { btn.hidden = langSwitch ? langSwitch.getAttribute("aria-checked") !== "true" : btn.dataset.language !== "true"; };
  showBtn();
  if (langSwitch) new MutationObserver(showBtn).observe(langSwitch, { attributes: true, attributeFilter: ["aria-checked"] });

  var name = dlg.querySelector("[data-testid=course-words-name]");
  var text = dlg.querySelector("[data-testid=course-words-text]");
  var count = dlg.querySelector("[data-testid=course-words-count]");
  var wrap = dlg.querySelector("[data-testid=course-words-preview-wrap]");
  var body = dlg.querySelector("[data-testid=course-words-preview] tbody");
  var skipped = dlg.querySelector("[data-testid=course-words-skipped]");
  var addBtn = dlg.querySelector("[data-testid=course-words-add]");
  var timer = null, asked = 0, rows = 0;
  var SKIP = { no_split: "courses.js.words_skip_no_split", duplicate: "courses.js.words_skip_duplicate", too_many: "courses.js.words_skip_too_many" };

  function read(title) {
    return fetch(base + "/api/courses/" + course + "/words", { method: "POST", headers: { "Content-Type": "application/json" }, credentials: "same-origin", body: JSON.stringify({ text: text.value, title: title || null }) })
      .then(function (r) { return r.json().then(function (b) { if (!r.ok) throw new Error(b.error || t("courses.js.words_failed")); return b; }); });
  }
  function canAdd() { addBtn.disabled = !(rows > 0 && name.value.trim()); }
  function failed(e) { rows = 0; canAdd(); count.className = "small bad-text"; count.textContent = e.message; }
  function draw(b) {
    rows = b.rows.length;
    body.textContent = "";
    b.rows.forEach(function (p) {
      var tr = document.createElement("tr");
      tr.setAttribute("data-testid", "course-words-row");
      [p.term, p.meaning].forEach(function (x) { var td = document.createElement("td"); td.textContent = x; tr.appendChild(td); });
      body.appendChild(tr);
    });
    wrap.hidden = !rows;
    skipped.textContent = "";
    b.skipped.forEach(function (s) {
      var li = document.createElement("li");
      li.setAttribute("data-testid", "course-words-skip");
      li.textContent = t(SKIP[s.why] || SKIP.no_split, { line: s.line, text: s.text });
      skipped.appendChild(li);
    });
    skipped.hidden = !b.skipped.length;
    count.className = "small muted";
    count.textContent = text.value.trim() ? tn("courses.js.words_read", rows) : "";
    canAdd();
  }
  function preview() {
    var mine = ++asked;
    read(null).then(function (b) { if (mine === asked) draw(b); }).catch(function (e) { if (mine === asked) failed(e); });
  }

  // A new row named `title` at the end of the last unit, holding `lesson`:
  // the last unit's + adds it, its name box takes the title, and Enter
  // names it, which saves the table.
  function addRow(title, lesson) {
    var plus = list.querySelectorAll(".lt-unit .lt-add-here");
    if (!plus.length) return false;
    plus[plus.length - 1].click();
    var input = list.querySelector(".lt-row.is-new [data-testid=course-lessons-rename]");
    if (!input) return false;
    var row = input.closest(".lt-row");
    var ta = row.querySelector(".lt-json");
    ta.value = lesson;
    ta.dataset.loaded = "1";
    ta.dataset.edited = "1";
    row._edits = (row._edits || 0) + 1;
    input.value = title;
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    return true;
  }

  btn.addEventListener("click", function () {
    name.value = "";
    text.value = "";
    draw({ rows: [], skipped: [] });
    dlg.showModal();
    name.focus();
  });
  text.addEventListener("input", function () { clearTimeout(timer); timer = setTimeout(preview, 250); });
  name.addEventListener("input", canAdd);
  addBtn.addEventListener("click", function () {
    var title = name.value.trim();
    addBtn.disabled = true;
    read(title).then(function (b) {
      if (!b.lesson) { draw(b); return; }
      dlg.close();
      if (!addRow(title, b.lesson)) { failed(new Error(t("courses.js.words_failed"))); return; }
      var status = document.getElementById("lt-status");
      if (status) { status.textContent = tn("courses.js.words_added", b.rows.length); status.className = "small lt-status muted"; }
    }).catch(failed);
  });
})();
