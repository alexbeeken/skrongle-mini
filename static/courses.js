// Course building pages: the editor's lesson table, a live JSON check in
// the file editor, the editor's "Generate the course" dialog and "Add
// content with AI" modal, and the
// polling status page for AI generation.
(function () {
  "use strict";
  var base = document.body.dataset.base || "";

  // ---------- the Share dialog's invitation links (no mail, #633) ----------
  // Copy puts the link on the clipboard; without the clipboard API (http on
  // a LAN address) the link is selected for the owner to copy.
  document.addEventListener("click", function (e) {
    var b = e.target.closest && e.target.closest("[data-copy-link]");
    if (!b) return;
    var input = b.parentNode.querySelector("input");
    if (!input) return;
    var done = function () { b.textContent = t('courses.js.link_copied'); setTimeout(function () { b.textContent = t('courses.js.link_copy'); }, 1600); };
    var select = function () { input.focus(); input.select(); b.textContent = t('courses.js.link_selected'); };
    if (navigator.clipboard && window.isSecureContext) navigator.clipboard.writeText(input.value).then(done, select);
    else select();
  });

  // Problems worded as the server words them: static/json-problems.js.
  var jsonError = JsonProblems.jsonError, problemText = JsonProblems.problemText, JSON_HINT = JsonProblems.JSON_HINT;

  // ---------- editor: the lesson table ----------
  // Rows carry their lesson id (empty for one added here), file path and
  // unit. Units are header rows (a fold arrow, no icon), whose + adds a lesson
  // there; a lesson belongs to the unit row above it. The course settings
  // row stays first; its JSON, while valid, reshapes the units as it is typed.
  // The table saves itself as it is edited (static/autosave.js, POST
  // /api/courses/{id}/lessons with the whole table): a move, rename, delete
  // or new lesson a second after it is made, typing in a lesson's JSON or
  // form 3 s after the last keystroke or when the drawer loses focus. The
  // circle by the title says how it stands. One page's saves share one Undo
  // step (for ten minutes). Drag the grip (mouse or finger), or focus it
  // and use the arrow keys.
  var panel = document.getElementById("lessons");
  var list = document.getElementById("lt-list");
  var editor = null;
  if (panel && list) {
    var course = panel.dataset.courseId;
    var started = panel.dataset.started === "true";
    var ltStatus = document.getElementById("lt-status");
    var workspace = document.querySelector(".workspace");
    // The row to clone for a new lesson comes from the panel's <template>, so a
    // course with no lessons yet (an outline whose lessons are still being written) adds one too.
    var blank = document.getElementById("lt-blank").content.querySelector(".lt-row");
    // Rows an Undo or Redo is fading out (data-leaving, applyPage) are not the table's.
    var q = function (sel) { return Array.prototype.slice.call(list.querySelectorAll(sel)).filter(function (el) { return !el.closest("[data-leaving]"); }); };
    var lessonRows = function () { return q(".lt-row"); };
    var units = function () { return q(".lt-unit"); };
    var unitOf = function (row) {
      for (var el = row.previousElementSibling; el; el = el.previousElementSibling) if (el.classList.contains("lt-unit")) return el.dataset.unit;
      return row.dataset.unit;
    };
    var settings = list.querySelector(".lt-settings");
    var courseTitle = document.getElementById("course-title");
    // The course title at the top renames like a lesson and saves with the table.
    var courseRename = document.getElementById("course-rename");
    var savedTitle = courseRename ? courseRename.textContent.trim() : "";
    var unitRow = function (id) { return units().filter(function (u) { return u.dataset.unit === id; })[0]; };
    var lastOfUnit = function (u) { var el = u; while (el.nextElementSibling && el.nextElementSibling.classList.contains("lt-row")) el = el.nextElementSibling; return el; };

    q(".lt-grip, .lt-expand, .lt-ai-edit, .lt-duplicate, .lt-delete, .lt-add-here, .lt-gear").forEach(function (b) { b.hidden = false; });
    var addUnitBtn = document.getElementById("lt-add-unit");
    if (addUnitBtn) addUnitBtn.hidden = false;
    // The gear's dialog (Editor settings): the JSON switch (with Forms mode
    // only) and the line numbers switch, both kept in the browser, and the
    // language features switch, which is about the course and lives in
    // course.json. Line numbers show with JSON on, language features with
    // JSON off (`showPrefs`).
    var prefs = document.getElementById("lt-prefs-dialog");
    var jsonSwitch = prefs.querySelector(".lt-switch[data-pref=mode]");
    var linesSwitch = prefs.querySelector(".lt-switch[data-pref=lines]");
    var langSwitch = prefs.querySelector(".lt-switch[data-pref=language]");
    var levelsSwitch = prefs.querySelector(".lt-switch[data-pref=levels]");
    var placementSwitch = prefs.querySelector(".lt-switch[data-pref=placement]");
    var placementRow = list.querySelector(".lt-placement");
    if (window.SchemaForm) { jsonSwitch.closest(".lt-pref").hidden = false; jsonSwitch.setAttribute("aria-checked", preferred() === "json" ? "true" : "false"); }
    setLines(linesOn());
    showPrefs(preferred());
    prefs.addEventListener("click", function (ev) {
      var b = ev.target.closest(".lt-switch");
      if (!b) return;
      var on = b.getAttribute("aria-checked") !== "true";
      if (b === jsonSwitch) setAll(on ? "json" : "forms");
      else if (b === langSwitch) withSettings(function (v) { v.language_features = on; });
      else if (b === levelsSwitch) withSettings(function (v) { if (on) v.levels = true; else delete v.levels; });
      else if (b === placementSwitch) setPlacement(on);
      else setLines(on);
    });
    function showPrefs(mode) {
      linesSwitch.closest(".lt-pref").hidden = mode !== "json";
      langSwitch.closest(".lt-pref").hidden = mode !== "forms";
    }
    // The language features as the settings say (`Manifest::is_language`):
    // the field when set, else on when the terms have a language of their own.
    function languageOn(v) {
      if (v && typeof v.language_features === "boolean") return v.language_features;
      var code = function (side) { return v && v[side] && v[side].lang ? String(v[side].lang).trim().toLowerCase() : ""; };
      var t = code("terms");
      return !!t && t !== code("meanings");
    }
    var languageNow = langSwitch.getAttribute("aria-checked") === "true";
    // Levels (course.json "levels") holds the Placement test switch, which
    // adds or takes out placement.json (#1106): its row under the settings
    // shows while it is on, a new one starting from a blank test.
    function syncLevels(v) {
      var on = !!(v && v.levels);
      levelsSwitch.setAttribute("aria-checked", on ? "true" : "false");
      placementSwitch.closest(".lt-pref").hidden = !on;
      if (!on && !placementRow.hidden) setPlacement(false);
    }
    function setPlacement(on) {
      placementSwitch.setAttribute("aria-checked", on ? "true" : "false");
      placementRow.hidden = !on;
      var ta = placementRow.querySelector(".lt-json");
      if (on && !placementRow.dataset.saved && !ta.dataset.loaded) {
        ta.value = JSON.stringify({ random_per_unit: 3, questions: [] }, null, 2) + "\n";
        ta.dataset.loaded = "1";
        contentEdited(placementRow);
        checkJson(placementRow);
      }
      problems();
      changed();
    }
    // The switch follows the settings, however they changed (the switch
    // itself, or typing in the settings JSON), and every open form is drawn
    // again with the new settings so the hidden parts come and go.
    function syncLanguage(v) {
      var on = languageOn(v);
      if (on === languageNow) return;
      languageNow = on;
      langSwitch.setAttribute("aria-checked", on ? "true" : "false");
      q(".lt-row").forEach(function (r) { if (!r.querySelector(".lt-drawer").hidden && r.dataset.target === "forms") setMode(r, "forms"); });
    }

    function say(msg, bad) { ltStatus.textContent = msg; ltStatus.className = "small lt-status " + (bad ? "bad-text" : "muted"); }

    // ----- saving -----
    // Specs shorten both waits with window.AUTOSAVE_TEST_MS (e2e: addInitScript).
    var CONTENT_MS = 3000, TABLE_MS = 1000;
    var hook = Number(window.AUTOSAVE_TEST_MS);
    if (hook > 0) CONTENT_MS = TABLE_MS = hook;
    var version = panel.dataset.version;
    var session = Math.random().toString(36).slice(2) + Date.now().toString(36);
    var sent = null; // what the request in flight carried, to settle the rows when it answers
    // Under a running job's modal the workspace is inert: nothing is saved.
    var locked = function () { return !workspace || workspace.inert; };
    var saver = window.Autosave && !locked() ? Autosave({
      url: base + "/api/courses/" + course + "/lessons",
      delay: TABLE_MS,
      status: document.querySelector("[data-testid=course-edit-save-status]"),
      message: document.querySelector("[data-testid=course-edit-save-error]"),
      body: tableBody,
      onSaved: saved,
      onRefused: refusedBy,
    }) : null;
    if (saver) saver.track("table");
    editor = window.CourseEditor = { saver: saver };

    function changed(wait) {
      renumber();
      // What the server refused is checked again with this change.
      if (refused) { refused = null; redraw(); }
      // A save in flight now carries an older table: its refusal will not count.
      if (sent) sent.stale = true;
      if (saver && !locked()) saver.change("table", { delay: wait });
    }
    function renumber() {
      lessonRows().forEach(function (r, i) {
        r.querySelector(".lt-num").textContent = i + 1;
        r.dataset.unit = unitOf(r);
      });
      units().forEach(function (u) {
        var n = 0;
        for (var el = u.nextElementSibling; el && el.classList.contains("lt-row"); el = el.nextElementSibling) n++;
        u.querySelector(".lt-count").textContent = n;
        // Every unit can go but the last one (one with lessons asks first).
        var del = u.querySelector(".lt-unit-delete");
        if (del) del.hidden = units().length < 2;
      });
    }
    renumber();
    var nameOf = function (row) { return row.querySelector(".lt-title, .lt-name").textContent.trim(); };
    var placeOf = function (row) { return row === settings ? t('courses.js.settings_place') : row === placementRow ? t('courses.js.placement_place') : t('courses.js.quoted', { text: nameOf(row) }); };
    // ----- problems -----
    // One source of truth: a drawer's problem is its JSON's parse error (found
    // here, as it is typed), else what the server last refused about it
    // (`refused`, until the next edit or save). The drawer's status, the row's
    // data-problem and the line under the title (Autosave's message: a parse
    // error through saver.problem, a refusal as the server's errors.table)
    // all say that same sentence.
    var refused = null; // { p: the server's problem, row }
    function rowProblem(row) {
      var ta = row.querySelector(".lt-json");
      var err = ta.dataset.loaded ? jsonError(ta.value) : null;
      if (err) {
        var p = { kind: "json", place: placeOf(row), line: err.line, column: err.column, message: err.message, hint: JSON_HINT };
        p.text = problemText(p);
        return p;
      }
      return refused && refused.row === row ? refused.p : null;
    }
    // A drawer whose JSON does not parse is not sent; the circle stays red
    // and says which, and the text stays in the box to be fixed.
    function problems() {
      if (!saver) return;
      var bad = null;
      q(".lt-row, .lt-settings").some(function (r) {
        var ta = r.querySelector(".lt-json");
        var p = ta.dataset.edited && ta.value.trim() ? rowProblem(r) : null;
        if (p && p.kind === "json") bad = p;
        return !!bad;
      });
      saver.problem("json", bad ? bad.text : "");
      drawUndo();
    }
    function drawStatus(row) {
      var ta = row.querySelector(".lt-json"), st = row.querySelector(".lt-json-status"), go = row.querySelector(".lt-goto");
      var p = rowProblem(row);
      if (p) row.dataset.problem = p.kind; else delete row.dataset.problem;
      ta.toggleAttribute("aria-invalid", !!p);
      markField(row, p && p.field);
      drawLines(row, p);
      if (go) { go.hidden = !(p && p.line); go.textContent = p && p.line ? t('courses.js.goto_line', { n: p.line }) : ""; }
      if (window.Advice) Advice.row(row); if (!ta.dataset.loaded) return; // the yellow ! of static/advice.js follows the JSON
      // Its JSON parses, but its edit went with a save the server refused
      // about another drawer: not saved either, and it says where to look.
      var held = !p && refused && refused.row !== row && ta.dataset.edited;
      st.textContent = p ? p.text : held ? t('courses.js.not_saved_refused', { place: placeOf(refused.row) }) : t('courses.js.valid_json_table');
      st.className = "lt-json-status " + (p || held ? "bad-text" : "ok-text");
    }
    function redraw() { q(".lt-row, .lt-settings").forEach(drawStatus); drawUndo(); }
    // In Forms mode, the field the problem is about.
    function markField(row, field) {
      Array.prototype.forEach.call(row.querySelectorAll(".sf-problem"), function (el) { el.classList.remove("sf-problem"); });
      var el = field && row.querySelector('.lt-form [data-path="' + field.replace(/"/g, '\\"') + '"]');
      if (el) el.classList.add("sf-problem");
    }
    // The lesson editor opens the section a problem names (the first item
    // the rules refuse): when a refusal lands, unless the author is typing
    // in the drawer, and on a click on the line under the title.
    function revealField(row, p, force) {
      var box = row.querySelector(".lt-form");
      if (!p || !p.field || !box || !box._le || !window.LessonEditor || box.hidden) return;
      if (!force && box.contains(document.activeElement)) return;
      LessonEditor.reveal(box, p.field);
      markField(row, p.field);
    }
    // ----- line numbers and the broken line -----
    // With line numbers on, a gutter beside the JSON box (.lt-lines) numbers
    // its lines, scrolling with it; the problem's line is marked there (a
    // <mark>) and tinted behind the text (.lt-badline, the colour of a
    // removed line in Review AI changes). Both follow every edit and scroll.
    function linesOn() {
      try { return localStorage.getItem("lt-lines") === "on"; } catch (e) { return false; }
    }
    function setLines(on) {
      try { localStorage.setItem("lt-lines", on ? "on" : "off"); } catch (e) { /* only a convenience */ }
      linesSwitch.setAttribute("aria-checked", on ? "true" : "false");
      list.classList.toggle("lines", on);
      q(".lt-row, .lt-settings").forEach(function (r) { drawLines(r, rowProblem(r)); });
    }
    function drawLines(row, p) {
      var ta = row.querySelector(".lt-json"), g = row.querySelector(".lt-lines");
      var bad = p && p.line > 0 ? p.line : 0;
      row.dataset.badLine = bad ? String(bad) : "";
      if (list.classList.contains("lines") && ta.dataset.loaded) {
        var n = (ta.value.match(/\n/g) || []).length + 1, html = "";
        for (var i = 1; i <= n; i++) html += (i === bad ? '<mark data-testid="course-lessons-bad-line">' + i + "</mark>" : i) + "\n";
        g.innerHTML = html;
      } else {
        g.textContent = "";
      }
      syncLines(row);
    }
    function syncLines(row) {
      var ta = row.querySelector(".lt-json"), g = row.querySelector(".lt-lines"), hl = row.querySelector(".lt-badline");
      g.scrollTop = ta.scrollTop;
      var bad = Number(row.dataset.badLine);
      hl.hidden = !bad;
      if (!bad) return;
      var cs = getComputedStyle(ta), lh = parseFloat(cs.lineHeight) || 18;
      hl.style.height = lh + "px";
      hl.style.top = (parseFloat(cs.borderTopWidth) + parseFloat(cs.paddingTop) + (bad - 1) * lh - ta.scrollTop) + "px";
    }
    list.addEventListener("scroll", function (ev) {
      var ta = ev.target;
      if (ta.classList && ta.classList.contains("lt-json")) syncLines(ta.closest(".lt-row, .lt-settings"));
    }, true);
    // The server refused the table: the row it is about opens, says why and is marked.
    function refusedBy(data) {
      var was = sent;
      sent = null;
      // Edited since that save was sent: the refusal is of a table that is gone.
      if (was && was.stale) return;
      refused = null;
      var p = data.problems && data.problems[0];
      if (p) {
        var row = (p.row != null && was && was.rows[p.row]) || (p.file === "course.json" ? settings : p.file === "placement.json" ? placementRow : null) ||
          q(".lt-row").filter(function (r) { return (p.file && r.dataset.path === p.file) || (p.lesson_id && r.dataset.lessonId === p.lesson_id); })[0];
        if (row && row.isConnected) { refused = { p: p, row: row }; reveal(row); }
      }
      redraw();
      if (refused) revealField(refused.row, refused.p, false);
    }
    // A row's drawer, open and in sight (its folder unfolded).
    function reveal(row) {
      var u = row === settings ? null : unitRow(unitOf(row));
      if (u && !isOpen(u)) fold(u, true);
      if (row.querySelector(".lt-drawer").hidden) toggleJson(row);
    }
    // The problem's line selected in the JSON box (its field, in Forms mode).
    function goTo(row) {
      var p = rowProblem(row);
      reveal(row);
      if (!p) return;
      var drawer = row.querySelector(".lt-drawer"), ta = row.querySelector(".lt-json");
      if (drawer.dataset.mode === "forms") revealField(row, p, true);
      var field = drawer.dataset.mode === "forms" && row.querySelector(".lt-form .sf-problem");
      if (field) { field.scrollIntoView({ block: "center" }); var input = field.matches("input, textarea, select") ? field : field.querySelector("input, textarea, select, summary"); if (input) input.focus(); return; }
      if (!p.line || ta.disabled) { ta.focus(); return; }
      var lines = ta.value.split("\n");
      var start = lines.slice(0, p.line - 1).join("\n").length + (p.line > 1 ? 1 : 0);
      var end = start + (lines[p.line - 1] || "").length;
      ta.focus();
      ta.setSelectionRange(start, end);
      var lh = parseFloat(getComputedStyle(ta).lineHeight) || 18;
      ta.scrollTop = Math.max(0, (p.line - 3) * lh);
    }
    // The line under the title points at its drawer: a click goes there.
    var why = document.querySelector("[data-testid=course-edit-save-error]");
    if (why) why.addEventListener("click", function () {
      var row = refused ? refused.row : q(".lt-row, .lt-settings").filter(function (r) { return r.querySelector(".lt-json").dataset.edited && rowProblem(r); })[0];
      if (row && row.isConnected) goTo(row);
    });
    function parse(text) { try { return JSON.parse(text); } catch (e) { return undefined; } }

    // The table as it stands, for the next save. A new lesson still being
    // named waits for its name (its id comes from it).
    function tableBody() {
      var sta = settings.querySelector(".lt-json");
      var manifest = null;
      if (sta.dataset.edited && parse(sta.value) !== undefined) manifest = sta.value;
      var rows = lessonRows().filter(function (r) { return r.dataset.lessonId || !r.dataset.naming; });
      // A drawer whose JSON does not parse sends none of it, and stays edited.
      var held = [];
      var lessons = rows.map(function (r) {
        var ta = r.querySelector(".lt-json");
        var content = null;
        if (ta.dataset.edited && ta.value.trim() && parse(ta.value) !== undefined) content = ta.value;
        held.push(!!ta.dataset.edited && content === null);
        return { key: r.dataset.lessonId || null, unit: unitOf(r), title: r.querySelector(".lt-title").textContent.trim(), content: content };
      });
      var title = courseRename && courseRename.textContent.trim() !== savedTitle ? courseRename.textContent.trim() : null;
      // The placement test: its JSON when edited, "" once switched off.
      var pta = placementRow.querySelector(".lt-json"), placement = null;
      if (placementRow.hidden) { if (placementRow.dataset.saved) placement = ""; }
      else if (pta.dataset.edited && parse(pta.value) !== undefined) placement = pta.value;
      sent = {
        placement: placement,
        placementEdits: placementRow._edits || 0,
        rows: rows,
        edits: rows.map(function (r) { return r._edits || 0; }),
        held: held,
        manifest: manifest !== null,
        manifestEdits: settings._edits || 0,
        title: title,
      };
      return { lessons: lessons, manifest: manifest, placement: placement, title: title, session: session, base: version };
    }

    // A save went in: rows learn their ids and files, and drawers not typed
    // in since take the files as saved (the server fills in a new lesson's
    // unit and group, and the order of every lesson that moved).
    function saved(res) {
      var was = sent;
      sent = null;
      if (refused) { refused = null; redraw(); }
      if (res.version) version = res.version;
      if (!res.saved || !was) return;
      was.rows.forEach(function (r, i) {
        var out = res.rows && res.rows[i];
        if (!out) return;
        r.dataset.lessonId = out.id;
        r.dataset.path = out.path;
        r.classList.remove("is-new");
        var ta = r.querySelector(".lt-json");
        if ((r._edits || 0) === was.edits[i] && !was.held[i]) delete ta.dataset.edited;
      });
      if (was.manifest && (settings._edits || 0) === was.manifestEdits) delete settings.querySelector(".lt-json").dataset.edited;
      if (was.title !== null) savedTitle = was.title;
      if (was.placement === "") { delete placementRow.dataset.saved; placementRow.querySelector(".lt-json").value = ""; delete placementRow.querySelector(".lt-json").dataset.loaded; }
      else if (was.placement !== null) { placementRow.dataset.saved = "1"; if ((placementRow._edits || 0) === was.placementEdits) delete placementRow.querySelector(".lt-json").dataset.edited; }
      var changedFiles = res.changed || {};
      q(".lt-row, .lt-settings").forEach(function (r) {
        var ta = r.querySelector(".lt-json");
        var text = changedFiles[r === settings ? "course.json" : r.dataset.path];
        if (text != null && ta.dataset.loaded && !ta.dataset.edited) setContent(r, text);
      });
      say(res.warning ? t('courses.js.saved_broken', { warning: res.warning }) : "", !!res.warning);
      var head = document.querySelector("[data-testid=course-edit]");
      if (head) head.dataset.loads = res.loads ? "true" : "false";
      // The Undo and Redo icons, as a reload would draw them (a save leaves nothing to redo).
      var history = document.querySelector("[data-testid=course-history]");
      if (res.undo_html) {
        var box = document.createElement("div");
        box.innerHTML = res.undo_html.trim();
        var icons = box.firstElementChild;
        if (history) history.replaceWith(icons);
        else { var slot = document.getElementById("course-history-row"); if (slot) slot.appendChild(icons); }
      } else if (history) history.remove();
      drawUndo();
      // A draft's Generate names its lessons and title, and goes once a
      // lesson has content: draw it again.
      // The feature report is drawn from the files as saved: draw it again too.
      if (document.getElementById("gen-dialog") || document.querySelector("[data-testid=course-ai-generate]") || document.getElementById("course-report")) refreshAi();
    }
    // JSON with every object's keys sorted, to compare documents regardless of key order.
    function canon(x) {
      if (Array.isArray(x)) return "[" + x.map(canon).join(",") + "]";
      if (x && typeof x === "object") return "{" + Object.keys(x).sort().map(function (k) { return JSON.stringify(k) + ":" + canon(x[k]); }).join(",") + "}";
      return JSON.stringify(x) === undefined ? "null" : JSON.stringify(x);
    }
    // The saved file into a drawer: the JSON box, and the open form's
    // document in place (the fields the server sets are not in the form).
    function setContent(row, text) {
      var ta = row.querySelector(".lt-json");
      ta.value = text;
      checkJson(row);
      var doc = row._doc, v = parse(text);
      if (!doc || !v || typeof v !== "object" || Array.isArray(v)) return;
      var redraw = false;
      Object.keys(Object.assign({}, doc, v)).forEach(function (k) {
        // The server writes keys in sorted order; only a change in substance counts.
        if (canon(doc[k]) === canon(v[k])) return;
        if ((doc[k] && typeof doc[k] === "object") || (v[k] && typeof v[k] === "object")) redraw = true;
        if (k in v) doc[k] = v[k]; else delete doc[k];
      });
      var drawer = row.querySelector(".lt-drawer");
      if (redraw && drawer.dataset.mode === "forms" && !drawer.contains(document.activeElement)) setMode(row, "forms");
    }

    // ----- unit rows -----
    function fold(u, open) {
      u.querySelector(".lt-fold").setAttribute("aria-expanded", open ? "true" : "false");
      u.querySelector(".lt-fold").textContent = open ? "▾" : "▸";
      u.classList.toggle("folded", !open);
      for (var el = u.nextElementSibling; el && el.classList.contains("lt-row"); el = el.nextElementSibling) el.hidden = !open;
    }
    var isOpen = function (u) { return !u.classList.contains("folded"); };

    // ----- moving -----
    // A lesson never sits above the first unit row.
    function keepInside(row) {
      var first = units()[0];
      if (first && !row.previousElementSibling.closest(".lt-row, .lt-unit")) list.insertBefore(row, first.nextElementSibling);
    }
    // Dropped into a folded unit, a lesson folds away with the rest.
    function settle(row) {
      var u = unitRow(unitOf(row));
      if (u && !isOpen(u)) row.hidden = true;
      changed();
    }
    // One step up or down; past a unit row the lesson changes unit (to the
    // end of the one above, or the start of the one below), opening it.
    function moveBy(row, dir) {
      var other = dir < 0 ? row.previousElementSibling : row.nextElementSibling;
      if (!other || (dir < 0 && other === units()[0])) return;
      list.insertBefore(row, dir < 0 ? other : other.nextElementSibling);
      var u = unitRow(unitOf(row));
      if (u && !isOpen(u)) fold(u, true);
      changed();
    }
    list.addEventListener("keydown", function (ev) {
      var g = ev.target.closest(".lt-grip");
      if (!g || (ev.key !== "ArrowUp" && ev.key !== "ArrowDown")) return;
      ev.preventDefault();
      moveBy(g.closest(".lt-row"), ev.key === "ArrowUp" ? -1 : 1);
      g.focus();
    });

    // Drag with pointer events, so a finger works as well as a mouse. The row
    // itself moves as the pointer passes the middle of another row; over a
    // unit row the lesson drops into that unit, at its end.
    var drag = null;
    list.addEventListener("pointerdown", function (ev) {
      var g = ev.target.closest(".lt-grip");
      if (!g || ev.button > 0) return;
      ev.preventDefault();
      var row = g.closest(".lt-row");
      drag = { row: row, start: lessonRows().indexOf(row) + ":" + unitOf(row), into: null, timer: null, y: ev.clientY };
      g.setPointerCapture(ev.pointerId);
      row.classList.add("dragging");
    });
    function over(y) {
      var hit = null;
      q(".lt-row:not([hidden]), .lt-unit").forEach(function (el) {
        if (el === drag.row) return;
        var r = el.getBoundingClientRect();
        if (y >= r.top && y < r.bottom) hit = { el: el, upper: y < r.top + r.height / 2 };
      });
      return hit;
    }
    function dragTo(y) {
      if (drag.into) { drag.into.classList.remove("drop-into"); drag.into = null; }
      var hit = over(y);
      if (!hit) return;
      if (hit.el.classList.contains("lt-unit")) {
        hit.el.classList.add("drop-into");
        drag.into = hit.el;
        return;
      }
      list.insertBefore(drag.row, hit.upper ? hit.el : hit.el.nextElementSibling);
      keepInside(drag.row);
    }
    function autoscroll() {
      if (!drag) return;
      var edge = 56, dy = 0;
      if (drag.y < edge) dy = -12; else if (drag.y > window.innerHeight - edge) dy = 12;
      if (dy) { window.scrollBy(0, dy); dragTo(drag.y); }
      drag.timer = requestAnimationFrame(autoscroll);
    }
    list.addEventListener("pointermove", function (ev) {
      if (!drag) return;
      drag.y = ev.clientY;
      if (!drag.timer) drag.timer = requestAnimationFrame(autoscroll);
      dragTo(ev.clientY);
    });
    function endDrag() {
      if (!drag) return;
      var d = drag;
      drag = null;
      cancelAnimationFrame(d.timer);
      d.row.classList.remove("dragging");
      if (d.into) {
        d.into.classList.remove("drop-into");
        list.insertBefore(d.row, lastOfUnit(d.into).nextElementSibling);
      }
      if (lessonRows().indexOf(d.row) + ":" + unitOf(d.row) !== d.start) settle(d.row);
    }
    list.addEventListener("pointerup", endDrag);
    list.addEventListener("pointercancel", endDrag);

    // ----- rename -----
    function rename(row, fresh) {
      renameIn(row.querySelector(".lt-title"), fresh, t('courses.js.rename_lesson'), "course-lessons-rename");
    }
    // A fresh row (just added) is saved once it is named, whatever the name.
    function renameIn(t, fresh, label, testid) {
      var input = document.createElement("input");
      input.type = "text";
      input.className = "lt-rename";
      input.value = t.textContent.trim();
      input.maxLength = t === courseRename ? 80 : 200;
      input.setAttribute("aria-label", label);
      input.setAttribute("data-testid", testid);
      t.hidden = true;
      t.parentNode.insertBefore(input, t);
      input.focus();
      input.select();
      var done = false;
      // Enter and Escape hand focus back to the title; a click elsewhere
      // (blur) leaves it where the click put it (#275).
      function finish(keep, refocus) {
        if (done) return;
        done = true;
        var name = input.value.trim();
        var row = t.closest(".lt-row");
        var named = keep && name && name !== t.textContent.trim();
        if (named) t.textContent = name;
        input.remove();
        t.hidden = false;
        if (fresh && row) delete row.dataset.naming;
        if (named || fresh) changed();
        if (!fresh && refocus) t.focus();
      }
      input.addEventListener("keydown", function (ev) {
        if (ev.isComposing || ev.keyCode === 229) return; // Enter confirming an IME composition
        if (ev.key === "Enter") { ev.preventDefault(); finish(true, true); }
        else if (ev.key === "Escape") { ev.preventDefault(); finish(false, true); }
      });
      input.addEventListener("blur", function () { finish(true, false); });
    }
    if (courseRename) courseRename.addEventListener("click", function () { renameIn(courseRename, false, t('courses.js.rename_course'), "course-edit-rename-input"); });
    // The pencil after the title (#422) renames the same way.
    var renamePencil = document.querySelector("[data-testid=course-edit-rename-pencil]");
    if (renamePencil && courseRename) renamePencil.addEventListener("click", function () { if (!courseRename.hidden) courseRename.click(); });

    // ----- add -----
    // A row for a lesson not saved yet, called `title`, not in the table yet.
    function newRow(title) {
      var row = blank.cloneNode(true);
      row.dataset.lessonId = "";
      row.dataset.path = "";
      row.querySelector(".lt-title").textContent = title;
      row.querySelector(".lt-drawer").hidden = true;
      row.querySelector(".lt-json").value = "";
      row.querySelectorAll(".lt-grip, .lt-expand, .lt-ai-edit, .lt-duplicate, .lt-delete").forEach(function (b) { b.hidden = false; });
      row.querySelector(".lt-expand").setAttribute("aria-expanded", "false");
      row.querySelector(".lt-expand").textContent = "▸";
      row.querySelector(".lt-expand").setAttribute("aria-label", t('courses.js.edit_new_lesson'));
      row.querySelector(".lt-grip").setAttribute("aria-label", t('courses.js.move_new_lesson'));
      row.querySelector(".lt-delete").setAttribute("aria-label", t('courses.js.delete_new_lesson'));
      row.querySelector(".lt-duplicate").setAttribute("aria-label", t('courses.js.duplicate_new_lesson'));
      row.classList.add("is-new");
      return row;
    }
    function add(unit) {
      var row = newRow(t('courses.js.new_lesson'));
      row.dataset.naming = "1";
      var u = unitRow(unit);
      if (u) { if (!isOpen(u)) fold(u, true); list.insertBefore(row, lastOfUnit(u).nextElementSibling); }
      else list.appendChild(row);
      row.hidden = false;
      renumber();
      row.scrollIntoView({ block: "nearest" });
      rename(row, true);
    }
    // ⧉ copies a lesson as it stands in the table (its JSON as typed when
    // the drawer has it, else its file) right below it, saved with the table
    // like a new lesson with its JSON (#454; the copy: static/lesson-duplicate.js).
    function duplicate(row) {
      var ta = row.querySelector(".lt-json");
      var source = ta.dataset.loaded ? Promise.resolve(ta.value) : !row.dataset.path ? Promise.resolve(blankLesson(row))
        : fetch(base + "/api/courses/" + course + "/file?path=" + encodeURIComponent(row.dataset.path), { credentials: "same-origin" })
          .then(function (r) { if (!r.ok) throw new Error(t('courses.js.load_file_failed')); return r.json(); }).then(function (f) { return f.content; });
      source.then(function (text) {
        var doc = LessonDuplicate.copyOf(parse(text), takenIds(null), t('courses.js.copy_of', { title: nameOf(row) }));
        if (!doc) { say(t('courses.js.duplicate_bad_json', { name: placeOf(row) }), true); return; }
        var copy = newRow(doc.title), cta = copy.querySelector(".lt-json");
        cta.value = JSON.stringify(doc, null, 2) + "\n";
        cta.dataset.loaded = "1";
        list.insertBefore(copy, row.nextElementSibling);
        copy.hidden = false;
        contentEdited(copy);
        checkJson(copy);
        changed();
        copy.scrollIntoView({ block: "nearest" });
        copy.querySelector(".lt-duplicate").focus();
      }).catch(function (e) { say(e.message, true); });
    }

    // ----- units -----
    // Units live in the course settings (course.json "units"), so adding or
    // deleting one edits the settings row's JSON and saves it with the table,
    // as typing there would. A new unit is named in place, like a new lesson
    // ("New unit"); Escape drops it. An empty unit is a header with no rows.
    function withSettings(fn) {
      var ta = settings.querySelector(".lt-json");
      var go = function () {
        var v = parse(ta.value);
        if (!v || typeof v !== "object" || !Array.isArray(v.units)) { say(t('courses.js.settings_need_units'), true); return; }
        if (fn(v) === false) return;
        setContent(settings, JSON.stringify(v, null, 2) + "\n");
        contentEdited(settings);
        applySettings(v);
        problems();
        changed();
      };
      if (ta.dataset.loaded) { go(); return; }
      fetch(base + "/api/courses/" + course + "/file?path=course.json", { credentials: "same-origin" })
        .then(function (r) { if (!r.ok) throw new Error(t('courses.js.load_settings_failed')); return r.json(); })
        .then(function (f) { if (!ta.dataset.loaded) { ta.value = f.content; ta.dataset.loaded = "1"; checkJson(settings); } go(); })
        .catch(function (e) { say(e.message, true); });
    }
    // A unit id: the next number when every id is a number (the usual "1",
    // "2"...), else a slug of its name; free either way.
    function unitId(title, taken) {
      var id;
      if (taken.length && taken.every(function (t) { return /^\d+$/.test(t); })) id = String(Math.max.apply(null, taken.map(Number)) + 1);
      else id = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40) || "unit";
      var free = id;
      for (var n = 2; taken.indexOf(free) >= 0; n++) free = id + "-" + n;
      return free;
    }
    function addUnit() {
      var last = units()[units().length - 1];
      var u = last.cloneNode(true);
      u.dataset.unit = "";
      u.classList.remove("folded", "missing", "drop-into");
      u.querySelector(".lt-fold").setAttribute("aria-expanded", "true");
      u.querySelector(".lt-fold").textContent = "▾";
      u.querySelector(".lt-count").textContent = "0";
      u.querySelector(".lt-unit-delete").hidden = true;
      var t = u.querySelector(".lt-unit-title");
      t.textContent = window.t('courses.js.new_unit');
      list.appendChild(u);
      u.scrollIntoView({ block: "nearest" });
      var input = document.createElement("input");
      input.type = "text";
      input.className = "lt-rename";
      input.value = window.t('courses.js.new_unit');
      input.maxLength = 120;
      input.setAttribute("aria-label", window.t('courses.js.rename_unit'));
      input.setAttribute("data-testid", "course-lessons-unit-rename");
      t.hidden = true;
      t.parentNode.insertBefore(input, t);
      input.focus();
      input.select();
      var done = false;
      // Enter and Escape hand focus back to the title; a click elsewhere
      // (blur) leaves it where the click put it (#275).
      function finish(keep, refocus) {
        if (done) return;
        done = true;
        var name = input.value.trim();
        u.remove();
        if (keep && name) {
          withSettings(function (v) {
            var taken = v.units.map(function (x) { return x && String(x.id); });
            v.units.push({ id: unitId(name, taken), title: name });
          });
        }
        if (refocus) addUnitBtn.focus();
      }
      input.addEventListener("keydown", function (ev) {
        if (ev.isComposing || ev.keyCode === 229) return; // Enter confirming an IME composition
        if (ev.key === "Enter") { ev.preventDefault(); finish(true, true); }
        else if (ev.key === "Escape") { ev.preventDefault(); finish(false, true); }
      });
      input.addEventListener("blur", function () { finish(true, false); });
    }
    // A unit goes out of the settings; its lessons, if any, go with it
    // (asked first, in the panel's dialog), in the one save.
    function deleteUnit(u) {
      var id = u.dataset.unit;
      var rows = [];
      for (var el = u.nextElementSibling; el && el.classList.contains("lt-row"); el = el.nextElementSibling) rows.push(el);
      if (rows.length && rows.length === lessonRows().length) { say(t('courses.js.need_one_lesson'), true); return; }
      var go = function () {
        rows.forEach(function (r) { r.remove(); });
        var next = u.nextElementSibling || u.previousElementSibling;
        withSettings(function (v) {
          if (v.units.length < 2) { say(t('courses.js.need_one_unit'), true); return false; }
          v.units = v.units.filter(function (x) { return !x || x.id !== id; });
        });
        var focus = next && next.querySelector(".lt-fold, .lt-expand");
        if (focus) focus.focus();
      };
      var dlg = document.getElementById("lt-unit-dialog");
      if (!rows.length || !dlg || !dlg.showModal) { go(); return; }
      dlg.returnValue = "";
      dlg.onclose = function () { if (dlg.returnValue === "confirm") go(); else u.querySelector(".lt-unit-delete").focus(); };
      dlg.showModal();
    }
    if (addUnitBtn) addUnitBtn.addEventListener("click", addUnit);

    // ----- the JSON drawer -----
    // The drawer's status as it stands; its JSON parsed (undefined when it does not parse).
    function checkJson(row) {
      drawStatus(row);
      return parse(row.querySelector(".lt-json").value);
    }
    // The ids of the table's lessons but `row`'s, saved or typed.
    function takenIds(row) {
      return lessonRows().filter(function (r) { return r !== row; }).map(function (r) {
        var v = r.dataset.lessonId;
        try { v = v || JSON.parse(r.querySelector(".lt-json").value).id; } catch (e) { /* not typed yet */ }
        return v;
      });
    }
    // A new lesson, editable before it is saved. Its id comes from its name,
    // free among the table's lessons; the server fills in unit and group
    // (and an id or slug left empty).
    function blankLesson(row) {
      var title = row.querySelector(".lt-title").textContent.trim();
      var id = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48) || "lesson";
      var taken = takenIds(row);
      var free = id;
      for (var n = 2; taken.indexOf(free) >= 0; n++) free = id + "-" + n;
      return JSON.stringify({ id: free, slug: free, title: title, summary: "", content: "## " + title + "\n\nWrite the lesson here. Each `## ` heading becomes one card." }, null, 2) + "\n";
    }
    function toggleJson(row) {
      var drawer = row.querySelector(".lt-drawer"), btn = row.querySelector(".lt-expand"), ta = row.querySelector(".lt-json");
      var open = drawer.hidden;
      drawer.hidden = !open;
      btn.setAttribute("aria-expanded", open ? "true" : "false");
      btn.textContent = open ? "▾" : "▸";
      if (!open) return;
      // Reopened: the switch may have moved while it was shut.
      if (ta.dataset.loaded) { setMode(row, preferred()); return; }
      if (!row.dataset.path) {
        ta.value = blankLesson(row);
        ta.placeholder = t('courses.js.whole_lesson_placeholder');
        ta.dataset.loaded = "1";
        checkJson(row);
        if (preferred() === "forms") setMode(row, "forms");
        return;
      }
      if (preferred() === "forms") row.dataset.want = "forms";
      ta.value = t('courses.js.loading');
      ta.disabled = true;
      fetch(base + "/api/courses/" + course + "/file?path=" + encodeURIComponent(row.dataset.path), { credentials: "same-origin" })
        .then(function (r) { if (!r.ok) throw new Error(t('courses.js.load_file_failed')); return r.json(); })
        .then(function (f) {
          ta.value = f.content; ta.dataset.loaded = "1"; ta.disabled = false; checkJson(row);
          if (row.dataset.want) { delete row.dataset.want; setMode(row, "forms"); }
        })
        .catch(function (e) { ta.value = ""; ta.disabled = false; row.querySelector(".lt-json-status").textContent = e.message; });
    }
    // ----- Forms mode -----
    // The drawer's file as a form (schema-form.js, from the course format's
    // JSON Schema). The form edits the parsed file and writes it back into
    // the JSON box on every change, so both modes hold the same document and
    // save the same way. One switch (JSON on or off) sets the mode of every
    // drawer, open now or later, and is remembered; a drawer whose JSON
    // can't be a form stays JSON on its own.
    function preferred() {
      if (!window.SchemaForm) return "json";
      try { return localStorage.getItem("lt-mode") || "json"; } catch (e) { return "json"; }
    }
    function setAll(mode) {
      try { localStorage.setItem("lt-mode", mode); } catch (e) { /* only a convenience */ }
      jsonSwitch.setAttribute("aria-checked", mode === "json" ? "true" : "false");
      showPrefs(mode);
      q(".lt-row, .lt-settings").forEach(function (r) { if (!r.querySelector(".lt-drawer").hidden) setMode(r, mode); });
    }
    function setMode(row, mode) {
      var drawer = row.querySelector(".lt-drawer"), ta = row.querySelector(".lt-json"), box = row.querySelector(".lt-form");
      var st = row.querySelector(".lt-json-status");
      function show(m) {
        drawer.dataset.mode = m;
        box.hidden = m !== "forms";
      }
      // The last mode asked for wins, even over a form still loading.
      row.dataset.target = mode;
      if (mode === "json" || row === placementRow) { delete row.dataset.want; row._doc = null; show("json"); return; }
      if (!ta.dataset.loaded) { row.dataset.want = "forms"; return; }
      if (!ta.value.trim() && !row.dataset.path) ta.value = blankLesson(row);
      var v = checkJson(row);
      if (!v || typeof v !== "object" || Array.isArray(v)) {
        if (v !== undefined) { st.textContent = t('courses.js.forms_need_object'); st.className = "lt-json-status bad-text"; }
        show("json");
        return;
      }
      st.textContent = t('courses.js.loading_form');
      var write = function (doc) {
        ta.value = JSON.stringify(doc, null, 2) + "\n";
        ta.dispatchEvent(new Event("input", { bubbles: true }));
      };
      // A lesson gets the lesson editor (static/lesson-editor.js: outline,
      // one form per kind, a live preview); the course settings keep the
      // schema form.
      if (row !== settings && window.LessonEditor) {
        row._doc = v;
        // The course settings first (the paradigm's slots, the language), so
        // the editor is drawn once, not again when they land mid-edit.
        manifestDoc().catch(function () { return {}; }).then(function (man) {
          if (row.dataset.target !== "forms" || row._doc !== v) return;
          LessonEditor.render(box, v, {
          onChange: write,
          manifest: man,
          schema: function () { return SchemaForm.load(base + "/docs/course.schema.json"); },
          api: function (name, body) {
            return fetch(base + "/api/courses/" + course + "/" + name, { method: "POST", headers: { "Content-Type": "application/json" }, credentials: "same-origin", body: JSON.stringify(body) })
              .then(function (r) { if (!r.ok) return r.json().then(function (j) { throw new Error(j.error || ("HTTP " + r.status)); }, function () { throw new Error("HTTP " + r.status); }); return r.json(); });
          },
          lessons: function () { return lessonRows().map(function (r) { return { id: r.dataset.lessonId, title: r.querySelector(".lt-title").textContent.trim() }; }); },
          toJson: function () { setAll("json"); },
          confirmDelete: function () { return confirm(t('courses.js.delete_item_confirm')); },
          confirmDerive: function () { return confirm(t('courses.js.guide_derive_confirm')); }
          });
          checkJson(row);
          show("forms");
        });
        return;
      }
      SchemaForm.load(base + "/docs/course.schema.json").then(function (schema) {
        if (row.dataset.target !== "forms") return;
        // The form edits `v` in place; a save puts the server's changes into it too.
        row._doc = v;
        SchemaForm.render(box, schema, row === settings ? "manifest" : "lesson", v, write);
        checkJson(row);
        show("forms");
      }).catch(function (e) { st.textContent = e.message; st.className = "lt-json-status bad-text"; });
    }
    // The course settings as they stand: the settings drawer's JSON when it
    // parses, else the saved file (fetched once).
    var manifestFetched = null; window.LessonTable = { manifest: function () { return manifestDoc(); }, open: function (row) { if (preferred() !== "forms") setAll("forms"); reveal(row); } }; // for static/advice.js
    function manifestDoc() {
      var v = parse(settings.querySelector(".lt-json").value);
      if (v && typeof v === "object") return Promise.resolve(v);
      if (!manifestFetched) {
        manifestFetched = fetch(base + "/api/courses/" + course + "/file?path=course.json", { credentials: "same-origin" })
          .then(function (r) { if (!r.ok) throw new Error(t('courses.js.load_settings_failed')); return r.json(); })
          .then(function (f) { return parse(f.content) || {}; });
        manifestFetched.catch(function () { manifestFetched = null; });
      }
      return manifestFetched;
    }

    list.addEventListener("input", function (ev) {
      if (!ev.target.classList.contains("lt-json")) return;
      var row = ev.target.closest(".lt-row, .lt-settings");
      contentEdited(row);
      var v = checkJson(row);
      if (row === settings) { if (v) applySettings(v); }
      // The name inside the JSON is the lesson's name.
      else if (v && typeof v.title === "string" && v.title.trim()) row.querySelector(".lt-title").textContent = v.title.trim();
      problems();
      changed(CONTENT_MS);
    });
    function contentEdited(row) {
      row.querySelector(".lt-json").dataset.edited = "1";
      row._edits = (row._edits || 0) + 1;
    }
    // Leaving a drawer (to another row, the page, another tab) saves what was typed in it.
    list.addEventListener("focusout", function (ev) {
      var d = ev.target.closest && ev.target.closest(".lt-drawer");
      if (!d || !saver || (ev.relatedTarget && d.contains(ev.relatedTarget))) return;
      saver.flush();
    });

    // The settings as typed: the course's title, and its units as unit rows in
    // their order, each taking its lessons along. A unit that is gone but
    // still holds lessons stays, marked, until they move (saving says so too).
    function applySettings(v) {
      if (courseTitle && typeof v.title === "string" && v.title.trim()) (courseRename || courseTitle).textContent = v.title.trim();
      syncLanguage(v);
      syncLevels(v);
      if (!Array.isArray(v.units)) return;
      var wanted = v.units.filter(function (u) { return u && typeof u.id === "string" && u.id; });
      if (!wanted.length) return;
      var groups = units().map(function (u) {
        var rows = [];
        for (var el = u.nextElementSibling; el && el.classList.contains("lt-row"); el = el.nextElementSibling) rows.push(el);
        return { unit: u, rows: rows };
      });
      var byId = {};
      groups.forEach(function (g) { byId[g.unit.dataset.unit] = g; });
      var seen = {};
      var order = [];
      wanted.forEach(function (w) {
        if (seen[w.id]) return;
        seen[w.id] = true;
        var g = byId[w.id];
        if (!g) {
          var u = groups[0].unit.cloneNode(true);
          u.dataset.unit = w.id;
          u.classList.remove("folded", "missing", "drop-into");
          u.querySelector(".lt-fold").setAttribute("aria-expanded", "true");
          u.querySelector(".lt-fold").textContent = "▾";
          g = { unit: u, rows: [] };
        }
        var title = typeof w.title === "string" && w.title.trim() ? w.title.trim() : w.id;
        g.unit.querySelector(".lt-unit-title").textContent = title;
        g.unit.querySelector(".lt-fold").setAttribute("aria-label", t('courses.js.fold_unit', { title: title }));
        g.unit.querySelector(".lt-add-here").setAttribute("aria-label", t('courses.js.add_lesson_to', { title: title }));
        var ud = g.unit.querySelector(".lt-unit-delete");
        if (ud) ud.setAttribute("aria-label", t('courses.js.delete_unit', { title: title }));
        g.unit.classList.remove("missing");
        order.push(g);
      });
      groups.forEach(function (g) {
        if (seen[g.unit.dataset.unit]) return;
        if (g.rows.length) { g.unit.classList.add("missing"); order.push(g); }
        else g.unit.remove();
      });
      order.forEach(function (g) {
        list.appendChild(g.unit);
        g.rows.forEach(function (r) { list.appendChild(r); });
      });
      list.classList.toggle("folders", order.length > 0);
      renumber();
    }
    list.addEventListener("keydown", function (ev) {
      var ta = ev.target;
      if (ev.key !== "Tab" || !ta.classList.contains("lt-json") || ev.shiftKey) return;
      ev.preventDefault();
      var s0 = ta.selectionStart, e0 = ta.selectionEnd;
      ta.value = ta.value.slice(0, s0) + "  " + ta.value.slice(e0);
      ta.selectionStart = ta.selectionEnd = s0 + 2;
      ta.dispatchEvent(new Event("input", { bubbles: true }));
    });

    // ----- clicks -----
    list.addEventListener("click", function (ev) {
      var b = ev.target.closest("button, a");
      if (!b) return;
      var row = b.closest(".lt-row, .lt-settings");
      if (b.classList.contains("lt-fold")) { var u = b.closest(".lt-unit"); fold(u, !isOpen(u)); }
      else if (b.classList.contains("lt-unit-delete")) deleteUnit(b.closest(".lt-unit"));
      else if (b.classList.contains("lt-add-here")) add(b.closest(".lt-unit").dataset.unit);
      else if (b.classList.contains("lt-duplicate")) duplicate(row);
      else if (b.classList.contains("lt-title")) rename(row, false);
      else if (b.classList.contains("lt-expand")) toggleJson(row);
      else if (b.classList.contains("lt-goto")) goTo(row);
      else if (b.classList.contains("lt-format")) {
        var v = checkJson(row);
        if (v !== undefined) {
          row.querySelector(".lt-json").value = JSON.stringify(v, null, 2) + "\n";
          contentEdited(row);
          changed(CONTENT_MS);
        }
      } else if (b.classList.contains("lt-delete")) {
        if (lessonRows().length === 1) { say(t('courses.js.need_one_lesson'), true); return; }
        // Saved at once, so a course with progress asks first.
        if (row.dataset.lessonId && started && !confirm(t('courses.js.delete_lesson_confirm', { name: nameOf(row) }))) return;
        var next = row.nextElementSibling || row.previousElementSibling;
        if (refused && refused.row === row) refused = null;
        row.remove();
        problems();
        changed();
        var focus = next && (next.querySelector(".lt-delete") || next.querySelector(".lt-fold"));
        if (focus) focus.focus();
      }
    });

    // ----- Undo and Redo, in place -----
    // The icons' forms post as they would without scripts. Here the answer
    // (the editor page after the step, course_edit.html) is read instead of
    // shown: the table takes its new rows, the title, the icons, the notice
    // and Use AI are drawn again, and nothing reloads. Rows that stay keep
    // still (one that changes place slides up or down to it), rows the step
    // adds fade in and rows it takes away fade out (#251).
    // Edits still waiting to be saved go first, so the step is theirs to undo too.
    var historyRow = document.getElementById("course-history-row");
    function applyPage(doc) {
      var fresh = doc.getElementById("lessons"), freshList = doc.getElementById("lt-list");
      var step = null;
      if (fresh && freshList) {
        step = stepStart();
        panel.dataset.version = version = fresh.dataset.version;
        panel.dataset.started = fresh.dataset.started;
        started = fresh.dataset.started === "true";
        panel.dataset.folders = fresh.dataset.folders;
        list.classList.toggle("folders", freshList.classList.contains("folders"));
        // What is open stays open: a lesson's drawer (by its id, or its file
        // for one not saved yet) opens again on the row that comes back for
        // it, and a folded unit stays folded.
        var keyOf = function (r) { return r.dataset.lessonId || r.dataset.path; };
        var open = {}, folded = {};
        lessonRows().forEach(function (r) { if (!r.querySelector(".lt-drawer").hidden) open[keyOf(r)] = true; });
        units().forEach(function (u) { if (!isOpen(u)) folded[u.dataset.unit] = true; });
        q(".lt-unit, .lt-row").forEach(function (el) { el.remove(); });
        Array.prototype.slice.call(freshList.querySelectorAll(".lt-unit, .lt-row")).forEach(function (el) { list.appendChild(document.adoptNode(el)); });
        q(".lt-grip, .lt-expand, .lt-ai-edit, .lt-duplicate, .lt-delete, .lt-add-here").forEach(function (b) { b.hidden = false; });
        lessonRows().forEach(function (r) { if (open[keyOf(r)]) toggleJson(r); });
        units().forEach(function (u) { if (folded[u.dataset.unit]) fold(u, false); });
        renumber();
        // The settings row stays (its switch, its listeners); its drawer shows the file as it now is.
        var sta = settings.querySelector(".lt-json");
        delete sta.dataset.edited;
        settings._edits = 0;
        if (sta.dataset.loaded) {
          fetch(base + "/api/courses/" + course + "/file?path=course.json", { credentials: "same-origin" })
            .then(function (r) { return r.ok ? r.json() : null; })
            .then(function (f) { if (f) setContent(settings, f.content); })
            .catch(function () {});
        }
        // The placement test and the gear's Levels as the page now has them, its drawer shut.
        var freshPlacement = freshList.querySelector(".lt-placement"), freshLevels = doc.querySelector(".lt-switch[data-pref=levels]");
        if (freshPlacement) {
          var pt = placementRow.querySelector(".lt-json");
          if (!placementRow.querySelector(".lt-drawer").hidden) toggleJson(placementRow);
          delete pt.dataset.edited; delete pt.dataset.loaded; pt.value = ""; placementRow._edits = 0;
          placementRow.hidden = freshPlacement.hidden;
          if (freshPlacement.dataset.saved) placementRow.dataset.saved = "1"; else delete placementRow.dataset.saved;
          placementSwitch.setAttribute("aria-checked", freshPlacement.hidden ? "false" : "true");
        }
        if (freshLevels) syncLevels({ levels: freshLevels.getAttribute("aria-checked") === "true" });
        problems();
      }
      var freshName = doc.getElementById("course-rename");
      if (freshName && courseRename) { courseRename.textContent = freshName.textContent; savedTitle = courseRename.textContent.trim(); }
      var freshHistory = doc.getElementById("course-history-row");
      if (freshHistory && historyRow) historyRow.innerHTML = freshHistory.innerHTML;
      // The notice ("Undone (…)"), and the alert when the course no longer loads.
      ["course-edit-notice", "course-edit-error"].forEach(function (id) {
        var here = document.querySelector("[data-testid=" + id + "]"), there = doc.querySelector("[data-testid=" + id + "]");
        if (there) there = document.adoptNode(there);
        if (here && there) here.replaceWith(there);
        else if (here) here.remove();
        else if (there) {
          var after = id === "course-edit-notice" && document.querySelector("[data-testid=course-edit-save-error]");
          if (after) after.after(there); else panel.before(there);
        }
      });
      if (step) stepEnd(step);
      drawUndo();
      var head = document.querySelector("[data-testid=course-edit]"), freshHead = doc.querySelector("[data-testid=course-edit]");
      if (head && freshHead) { head.dataset.loads = freshHead.dataset.loads; head.dataset.courseStatus = freshHead.dataset.courseStatus; }
      refreshAi(doc);
    }
    // The step's transition, by row: a lesson by its id (or file), a unit by
    // its id. Places are measured from the top of the table, so a notice
    // appearing above it moves nothing. Only vertical movement and opacity;
    // nothing under reduced motion. Specs lengthen it with window.UNDO_TEST_MS.
    var STEP_MS = Number(window.UNDO_TEST_MS) > 0 ? Number(window.UNDO_TEST_MS) : 220;
    var stepping = [];
    var stepKey = function (el) { return el.classList.contains("lt-unit") ? "u:" + el.dataset.unit : "l:" + (el.dataset.lessonId || el.dataset.path); };
    function stepPlaces() {
      var top = list.getBoundingClientRect().top + list.clientTop, at = {};
      q(".lt-unit, .lt-row").forEach(function (el) {
        if (!el.getClientRects().length) return; // folded away, or units in a table without folders
        var r = el.getBoundingClientRect();
        at[stepKey(el)] = { el: el, top: r.top - top, height: r.height };
      });
      return at;
    }
    function stepStart() {
      // A step still moving ends where it was going first.
      stepping.forEach(function (a) { a.finish(); });
      stepping = [];
      if (!list.animate || (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches)) return null;
      return { at: stepPlaces(), height: list.offsetHeight };
    }
    function stepEnd(step) {
      var now = stepPlaces(), height = list.offsetHeight;
      var timing = { duration: STEP_MS, easing: "ease-out" };
      var anims = [];
      Object.keys(now).forEach(function (k) {
        var was = step.at[k], el = now[k].el;
        if (!was) anims.push(el.animate([{ opacity: 0, transform: "translateY(-4px)" }, { opacity: 1, transform: "none" }], timing));
        else if (Math.abs(was.top - now[k].top) > 0.5) anims.push(el.animate([{ transform: "translateY(" + (was.top - now[k].top) + "px)" }, { transform: "none" }], timing));
      });
      // A row taken away lies where it was, out of the flow, and fades.
      var ghosts = [];
      Object.keys(step.at).forEach(function (k) {
        if (now[k]) return;
        var was = step.at[k], g = was.el;
        g.setAttribute("data-leaving", "");
        g.setAttribute("aria-hidden", "true");
        g.inert = true;
        [g].concat(Array.prototype.slice.call(g.querySelectorAll("[data-testid]"))).forEach(function (el) { el.removeAttribute("data-testid"); });
        g.style.cssText = "position:absolute;left:0;right:0;top:" + was.top + "px;height:" + was.height + "px;pointer-events:none";
        list.insertBefore(g, list.firstChild);
        ghosts.push(g);
        anims.push(g.animate([{ opacity: 1 }, { opacity: 0 }], timing));
      });
      if (height !== step.height) anims.push(list.animate([{ height: step.height + "px" }, { height: height + "px" }], timing));
      if (!anims.length) return;
      list.setAttribute("data-stepping", "");
      stepping = anims;
      Promise.all(anims.map(function (a) { return a.finished.catch(function () {}); })).then(function () {
        ghosts.forEach(function (g) { g.remove(); });
        if (stepping === anims) { stepping = []; list.removeAttribute("data-stepping"); }
      });
    }
    // ----- Undo while something cannot be saved -----
    // JSON that does not parse, or a table the server refused, is not saved,
    // so no Undo step holds it. Meanwhile the Undo icon (enabled even when
    // nothing saved can be undone) takes those changes back: what is valid
    // and waiting is saved first, then the table and its drawers go back to
    // the course as saved. Then Undo undoes saved steps again.
    function unsaved() {
      if (!saver || locked()) return false;
      return !!refused || q(".lt-row, .lt-settings").some(function (r) { return r.querySelector(".lt-json").dataset.edited && rowProblem(r); });
    }
    function drawUndo() {
      var btn = historyRow && historyRow.querySelector("[data-testid=course-undo]");
      if (!btn) return;
      var on = unsaved();
      if (on === !!btn.dataset.discard) return;
      if (on) {
        btn.dataset.label = btn.getAttribute("aria-label");
        btn.dataset.discard = "1";
        btn.disabled = false;
        btn.setAttribute("aria-label", t('courses.js.undo_unsaved'));
      } else {
        btn.setAttribute("aria-label", btn.dataset.label);
        delete btn.dataset.discard;
        delete btn.dataset.label;
        btn.disabled = !btn.closest("form");
      }
    }
    function discard(btn) {
      btn.disabled = true;
      saver.settle().then(function (ok) {
        if (ok || !unsaved()) { drawUndo(); btn.disabled = btn.dataset.discard ? false : !btn.closest("form"); return; }
        saver.discard();
        refused = null;
        return fetch(base + "/courses/" + course, { credentials: "same-origin" })
          .then(function (r) { if (!r.ok) throw new Error("reload failed"); return r.text(); })
          .then(function (html) {
            applyPage(new DOMParser().parseFromString(html, "text/html"));
            redraw();
            say(t('courses.js.unsaved_undone'));
          })
          .catch(function () { window.location.reload(); });
      });
    }
    if (historyRow) historyRow.addEventListener("click", function (ev) {
      var btn = ev.target.closest && ev.target.closest("[data-testid=course-undo][data-discard]");
      if (!btn || !window.DOMParser || !window.fetch) return;
      ev.preventDefault();
      discard(btn);
    });
    if (historyRow) historyRow.addEventListener("submit", function (ev) {
      var form = ev.target.closest("form");
      if (!form || !window.DOMParser || !window.fetch || locked()) return;
      ev.preventDefault();
      var btn = form.querySelector("button");
      btn.disabled = true;
      (saver ? saver.settle() : Promise.resolve(true)).then(function (ok) {
        // A save the server refused stays on screen; its circle says why.
        if (!ok) { btn.disabled = false; return; }
        return fetch(form.action, { method: "POST", credentials: "same-origin", body: new URLSearchParams(new FormData(form)) })
          .then(function (r) { if (!r.ok) throw new Error("undo failed"); return r.text(); })
          .then(function (html) { applyPage(new DOMParser().parseFromString(html, "text/html")); })
          .catch(function () { form.submit(); });
      });
    });
  }

  // ---------- file editor: live JSON check ----------
  var ta = document.getElementById("file-content");
  if (ta) {
    var st = document.getElementById("json-status");
    var pathBox = document.querySelector("[data-testid=course-file-path]");
    var timer = null;
    function check() {
      var path = pathBox ? pathBox.value.trim() : "";
      if (/\.svg$/.test(path)) return;
      var err = jsonError(ta.value);
      if (!err) { st.textContent = t('courses.js.valid_json_file'); st.className = "small ok-text"; return; }
      st.textContent = problemText({ kind: "json", place: path === "course.json" ? t('courses.js.settings_place') : path, line: err.line, column: err.column, message: err.message, hint: JSON_HINT });
      st.className = "small bad-text";
    }
    ta.addEventListener("input", function () { clearTimeout(timer); timer = setTimeout(check, 400); });
    ta.addEventListener("keydown", function (ev) {
      if (ev.key === "Tab") { ev.preventDefault(); var s = ta.selectionStart, e = ta.selectionEnd; ta.value = ta.value.slice(0, s) + "  " + ta.value.slice(e); ta.selectionStart = ta.selectionEnd = s + 2; }
    });
  }

  // ---------- prompt boxes: a live character count ----------
  // Under each box that forms a prompt (the Generate dialog's wish and
  // notes, Use AI's text): "45 / 2,000 characters". Red below data-min and
  // above the maximum (data-max, else maxlength), with the limit it breaks;
  // normal text in between. A data-optional box may also be left empty.
  // Characters are counted as the server counts them: trimmed, by code point.
  function grouped(n) { return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ","); }
  function promptLength(box) { return Array.from(box.value.trim()).length; }
  function boxMax(box) { return parseInt(box.dataset.max || box.getAttribute("maxlength"), 10) || Infinity; }
  // What is wrong with the box's length: "at least 20", "at most 2,000" or "".
  function lengthProblem(box) {
    var n = promptLength(box), min = parseInt(box.dataset.min, 10) || 0, max = boxMax(box);
    if (n > max) return t('courses.js.at_most', { max: grouped(max) });
    if (n < min && !(n === 0 && box.hasAttribute("data-optional"))) return t('courses.js.at_least', { min: grouped(min) });
    return "";
  }
  function charCount(box, out) {
    if (!box || !out) return function () {};
    var render = function () {
      var problem = lengthProblem(box);
      var count = t('courses.js.char_count', { n: grouped(promptLength(box)), max: grouped(boxMax(box)) });
      out.textContent = problem ? t('courses.js.char_count_problem', { count: count, problem: problem }) : count;
      out.dataset.state = problem ? "bad" : "ok";
    };
    box.addEventListener("input", render);
    render();
    return render;
  }
  // Smooth unless the learner asked for less motion (OS or the app's switch).
  function scrollBehavior() {
    var reduce = document.documentElement.classList.contains("reduce-motion") ||
      (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
    return reduce ? "auto" : "smooth";
  }

  // ---------- editor: the "Generate the course" dialog ----------
  // generate_form.html, on a draft or a stopped generation. Use AI's
  // "Generate the course" and Resume open it; it opens by itself when the
  // course was just created (?generate=1) or the form came back refused.
  function wireGenDialog(genDialog) {
    if (!genDialog || !genDialog.showModal) return;
    // The server rendered it open (for pages without scripts); make it modal
    // without a close event.
    if (genDialog.hasAttribute("data-open-on-load")) { genDialog.removeAttribute("open"); genDialog.showModal(); }
    // Closed: a reload should not open it again.
    genDialog.addEventListener("close", function () {
      if (!/[?&]generate=/.test(window.location.search)) return;
      var params = new URLSearchParams(window.location.search);
      params.delete("generate");
      var q = params.toString();
      window.history.replaceState(null, "", window.location.pathname + (q ? "?" + q : "") + window.location.hash);
    });
  }
  wireGenDialog(document.getElementById("gen-dialog"));
  // A <dialog data-open-on-load> (the Share dialog after sending or
  // removing an invitation, `?share=1`) opens as the page does.
  var onLoad = document.querySelector("dialog[data-open-on-load]");
  if (onLoad && onLoad.showModal && !onLoad.open) onLoad.showModal();
  // No mail (#633): the invitation links are shown once, on the page the
  // form's POST rendered (/courses/<id>/share). Put the course page in the
  // address bar so a reload opens the dialog again without resending the
  // form (#667).
  if (document.querySelector('[data-testid="course-share-links"]') && /\/share$/.test(window.location.pathname)) {
    window.history.replaceState(null, "", window.location.pathname.replace(/\/share$/, "") + "?share=1");
  }
  // [data-open-dialog=<id>] opens that <dialog>: Use AI's "Generate the
  // course" and Resume (the dialog may be drawn again after a save), and the
  // red ! on a stopped course's card (the reason it stopped).
  document.addEventListener("click", function (ev) {
    var a = ev.target.closest && ev.target.closest("[data-open-dialog]");
    var d = a && document.getElementById(a.dataset.openDialog);
    if (!a || !d || !d.showModal) return;
    ev.preventDefault();
    d.showModal();
    d.dispatchEvent(new Event("dialog-open"));
  });
  // [data-dialog-close] inside a <dialog> closes it (without JavaScript it is
  // a link away from the :target that showed it, as in the feature report).
  document.addEventListener("click", function (ev) {
    var c = ev.target.closest && ev.target.closest("[data-dialog-close]");
    var d = c && c.closest("dialog");
    if (!d || !d.close) return;
    ev.preventDefault();
    d.close();
  });

  // ---------- editor: Use AI and Generate, drawn again after a save ----------
  // A draft's Generate dialog names the title and lessons the AI keeps, and
  // goes once a lesson has content; the table saves in place, so after a
  // save both are fetched from the page as it now is.
  var refreshing = false;
  function refreshAi(doc) {
    if (!window.DOMParser) return;
    var open = document.getElementById("gen-dialog");
    if (open && open.open) return;
    if (doc) { apply(doc); return; }
    if (refreshing) return;
    refreshing = true;
    fetch(window.location.pathname, { credentials: "same-origin" }).then(function (r) { return r.ok ? r.text() : null; }).then(function (html) {
      refreshing = false;
      if (!html) return;
      apply(new DOMParser().parseFromString(html, "text/html"));
    }).catch(function () { refreshing = false; });
    function apply(doc) {
      var report = document.getElementById("course-report"), freshReport = doc.getElementById("course-report"), btn = document.querySelector("[data-testid=course-report-open]"), freshBtn = doc.querySelector("[data-testid=course-report-open]");
      if (report && freshReport && !report.open) { report.replaceWith(document.adoptNode(freshReport)); if (btn && freshBtn) btn.replaceWith(document.adoptNode(freshBtn)); } // and its button's lint count (#445)
      if (window.LessonEditor && LessonEditor.refresh) LessonEditor.refresh(); // the lints as amber advice in every open lesson form (#1199)
      var here = document.getElementById("ai"), fresh = doc.getElementById("ai");
      if (here && fresh) {
        // Keep what is typed in Use AI's box.
        var text = here.querySelector("textarea"), freshText = fresh.querySelector("textarea");
        if (text && freshText) freshText.value = text.value;
        fresh = document.adoptNode(fresh);
        here.replaceWith(fresh);
        wireAiForm(fresh.querySelector("form"));
      }
      var dlg = document.getElementById("gen-dialog"), freshDlg = doc.getElementById("gen-dialog");
      if (dlg && dlg.open) return;
      if (freshDlg) {
        freshDlg.removeAttribute("open");
        freshDlg.removeAttribute("data-open-on-load");
        freshDlg = document.adoptNode(freshDlg);
        if (dlg) dlg.replaceWith(freshDlg); else document.querySelector(".workspace").after(freshDlg);
        wireGenDialog(freshDlg);
        wireGenForm(freshDlg.querySelector("#gen-form"));
      } else if (dlg) dlg.remove();
    }
  }

  // ---------- editor: the AI job modal ----------
  // Rendered by the server for a job of this course (ai_job_modal.html): a
  // generation, or Add content with AI. While it runs the page polls the job;
  // when it ends the page reloads with ?job=, so the server shows the review
  // of what an Add content job changed, or the error with Close (a plain link
  // back to the editor; Escape follows it). A generation that succeeded shows
  // neither.
  function jobModal(modal) {
    document.body.classList.add("has-modal");
    // Focus Close once the job has failed; while it runs, the box itself,
    // so Enter cannot cancel it.
    (modal.querySelector("[data-modal-close]") || modal).focus();
    document.addEventListener("keydown", function (ev) {
      var close = modal.querySelector("[data-modal-close]");
      if (ev.key === "Escape" && close) { ev.preventDefault(); close.click(); }
    });
    if (modal.dataset.follow !== "true") return;
    var tickModal = function (wait) {
      setTimeout(function () {
        fetch(modal.dataset.pollUrl, { credentials: "same-origin" }).then(function (r) { return r.ok ? r.json() : null; }).then(function (s) {
          // The job is gone (server restarted): the page as it is now.
          if (!s) { window.location.replace(modal.dataset.doneUrl.replace(/\?.*$/, "")); return; }
          if (s.done) { window.location.replace(modal.dataset.doneUrl); return; }
          modal.dataset.phase = s.phase;
          modal.dataset.lessonsDone = s.lessons_done;
          modal.dataset.lessonsTotal = s.lessons_total;
          document.getElementById("ai-modal-phase").textContent = s.phase;
          document.getElementById("ai-modal-bar").style.width = (s.lessons_total ? Math.round(s.lessons_done * 100 / s.lessons_total) : 0) + "%";
          var n = { done: s.lessons_done, total: s.lessons_total };
          document.getElementById("ai-modal-count").textContent = s.editing ? t('courses.js.modal_count_edited', n) : s.revision && !s.adding ? t('courses.js.modal_count_rewritten', n) : t('courses.js.modal_count_written', n);
          tickModal(2000);
        }).catch(function () { tickModal(5000); });
      }, wait);
    };
    tickModal(700);
  }
  var modal = document.getElementById("ai-modal");
  if (modal) jobModal(modal);

  // ---------- editor: Use AI's form ----------
  // Sent in the background, so the course page stays: the answer is the
  // editor page, either with the job modal (shown over this page, which
  // is locked until the job ends and the review opens) or with the form
  // refused (its section swapped in). Without fetch it is a plain form post.
  function wireAiForm(form) {
    if (!form) return;
    // The box follows the mode (Add, Edit, Add and edit): label, placeholder, limit. Edit writes full lessons: no Outline only.
    var text = form.querySelector("textarea[name=text]"), outline = document.querySelector("[data-testid=course-ai-outline]");
    var count = charCount(text, document.getElementById("ai-text-count"));
    document.querySelectorAll("[data-testid=course-ai-mode]").forEach(function (r) {
      r.addEventListener("change", function () {
        var label = document.querySelector("[data-testid=course-ai-text-label]"); if (label) label.textContent = r.dataset.ask;
        if (text) { text.maxLength = r.dataset.max; text.placeholder = r.dataset.placeholder; count(); }
        if (outline) outline.disabled = r.value === "edit";
      });
    });
    if (!window.fetch || !window.DOMParser) return;
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      var b = form.querySelector("button[type=submit]");
      var label = b ? b.textContent : "";
      if (b) { b.disabled = true; b.textContent = t('courses.js.starting'); }
      var plain = function () { HTMLFormElement.prototype.submit.call(form); };
      // The job works on the course as saved: save the table's edits first.
      var saver = editor && editor.saver;
      (saver ? saver.settle() : Promise.resolve(true)).then(function (ok) {
        if (ok) return start();
        if (b) { b.disabled = false; b.textContent = label; }
        var why = document.querySelector("[data-testid=course-edit-save-error]");
        if (why) why.scrollIntoView({ block: "nearest" });
      });
      function start() {
      fetch(form.action, { method: "POST", credentials: "same-origin", body: new URLSearchParams(new FormData(form)) }).then(function (r) {
        return r.text().then(function (html) {
          if (!r.ok) { plain(); return; }
          var doc = new DOMParser().parseFromString(html, "text/html");
          var backdrop = doc.querySelector("[data-testid=course-ai-modal-backdrop]");
          if (backdrop) {
            // The workspace stays as it is, inert under the modal, as the
            // server draws it while a job runs (course_edit.html, `locked`).
            var workspace = document.querySelector(".workspace");
            if (!workspace) { plain(); return; }
            workspace.inert = true;
            workspace.setAttribute("data-testid", "course-edit-locked");
            if (saver) saver.pause();
            workspace.after(document.adoptNode(backdrop));
            jobModal(document.getElementById("ai-modal"));
            return;
          }
          // A job quick enough to be done already: its review (or, with the
          // review not opening by itself, the page saying it is done).
          if (doc.getElementById("review-modal") || (r.redirected && !doc.querySelector("[data-testid=course-ai-error]"))) { window.location.replace(r.url); return; }
          var fresh = doc.getElementById("ai");
          var here = document.getElementById("ai");
          if (!fresh || !here) { plain(); return; }
          here.replaceWith(document.adoptNode(fresh));
          wireAiForm(fresh.querySelector("form"));
          var err = fresh.querySelector("[role=alert]");
          var text = fresh.querySelector("textarea");
          if (text) text.focus({ preventScroll: true });
          if (err) err.scrollIntoView({ block: "nearest", behavior: scrollBehavior() });
        });
      }).catch(plain);
      }
    });
  }
  wireAiForm(document.querySelector("[data-testid=course-ai-form]"));
  // Info tips (<details class="info-tip">): Escape or a click elsewhere closes an open one.
  function closeTips(except) {
    Array.prototype.forEach.call(document.querySelectorAll("details.info-tip[open]"), function (d) { if (d !== except) d.open = false; });
  }
  document.addEventListener("click", function (ev) { closeTips(ev.target.closest && ev.target.closest("details.info-tip")); });
  document.addEventListener("keydown", function (ev) { if (ev.key === "Escape") closeTips(null); });

  // ---------- editor: "Review AI changes" ----------
  // Rendered by the server with ?review=1 or after a job
  // (course_review_modal.html); Close is a plain link back to the editor,
  // and Escape follows it. Every − is a checkbox of the Accept form. Here a
  // line's − leaves out a group: one row, or every changed row a drag from
  // it passes over; the group's one yellow −, on its first row, brings it all
  // back. A lesson's −
  // folds it away (its lines keep their state for when it comes back).
  function reviewModal(modal) {
    document.body.classList.add("has-modal");
    var close = modal.querySelector("[data-modal-close]");
    var form = document.getElementById("review-form");
    close.focus();
    var lineBoxes = function () { return modal.querySelectorAll("input[name=lines]"); };
    var state = function () {
      return Array.prototype.map.call(modal.querySelectorAll("input[form=review-form]"), function (i) {
        return i.type === "hidden" ? i.name + "=" + i.value : i.checked ? "1" : "0";
      }).join("\n");
    };
    var initial = state();
    var leave = function () { return state() === initial || window.confirm(t('courses.js.review_leave_confirm')); };
    close.addEventListener("click", function (ev) { if (!leave()) ev.preventDefault(); });
    document.addEventListener("keydown", function (ev) {
      if (ev.key === "Escape" && !document.querySelector("details.info-tip[open]") && !modal.querySelector(".rv-editor")) { ev.preventDefault(); close.click(); }
    });

    // Groups: data-group on each line's box, numbered per lesson.
    var nextGroup = 1;
    Array.prototype.forEach.call(lineBoxes(), function (b) { nextGroup = Math.max(nextGroup, (parseInt(b.dataset.group, 10) || 0) + 1); });
    var fileOf = function (el) { return el.closest("[data-testid=course-review-file]"); };
    var groupBoxes = function (box) {
      var g = box.dataset.group;
      if (!g || g === "0") return [box];
      return Array.prototype.filter.call(fileOf(box).querySelectorAll("input[name=lines]"), function (b) { return b.dataset.group === g; });
    };
    // A click on a line that opens a list or an object ("Quiz", a question)
    // takes the lines up to its closing bracket with it, so what is left still
    // reads. A drag leaves out exactly the rows it passed over.
    var rawOf = function (tr) { var c = tr.querySelector("td.add .rv-raw"); return c ? c.textContent : null; };
    var depth = function (s) { return s.length - s.replace(/^\s+/, "").length; };
    // The changed rows after an opening row up to its closing bracket.
    var blockOf = function (tr) {
      var raw = rawOf(tr), rows = [];
      if (raw === null || !/[\[{]$/.test(raw.replace(/\s+$/, ""))) return rows;
      for (var n = tr.nextElementSibling; n; n = n.nextElementSibling) {
        var r = rawOf(n);
        if (!n.querySelector("input[name=lines]") || r === null) break; // an unchanged line inside: the rest stays as it is
        rows.push(n);
        if (depth(r) === depth(raw) && /^\s*[\]}]/.test(r)) break;
      }
      return rows;
    };
    var withBlocks = function (boxes) {
      var out = boxes.slice();
      boxes.forEach(function (b) {
        blockOf(b.closest("tr")).forEach(function (n) {
          var box = n.querySelector("input[name=lines]");
          if (!box.disabled && out.indexOf(box) < 0) out.push(box);
        });
      });
      return out;
    };
    // The new group takes rows from groups it overlaps: a group it covers
    // joins it, the rest of one it cuts through comes back in.
    var leaveOut = function (boxes, whole) {
      if (whole) boxes = withBlocks(boxes);
      boxes.forEach(function (b) {
        if (!b.checked || b.dataset.group === "0" || !b.dataset.group) return;
        groupBoxes(b).forEach(function (o) { if (boxes.indexOf(o) < 0) { o.checked = false; o.dataset.group = "0"; } });
      });
      var g = String(nextGroup++);
      boxes.forEach(function (b) { b.checked = true; b.dataset.group = g; });
      update();
    };
    var takeBack = function (box) {
      groupBoxes(box).forEach(function (b) { b.checked = false; b.dataset.group = "0"; });
      update();
    };

    // How much is left out, in words, as the server says it.
    var count = document.getElementById("review-left-out");
    var accept = modal.querySelector("[data-testid=course-review-accept]");
    // An edited line is always in (#287): its − goes (disabled, hidden), and
    // so does that of a line opening a block it is in; its lesson's − is
    // disabled, saying why. Editing a line left out takes it back in.
    var editedTitle = t('courses.js.review_edited_file_tip');
    var immune = function (box, off, title) {
      // An undone run's review has no − boxes (#281).
      if (!box) return;
      var label = box.closest("label");
      if (label.dataset.title === undefined) label.dataset.title = label.title;
      if (off && box.checked) { box.checked = false; box.dataset.group = "0"; }
      box.disabled = off;
      label.title = off ? title : label.dataset.title;
    };
    function update() {
      var lessons = 0, groups = 0, edited = 0;
      Array.prototype.forEach.call(modal.querySelectorAll("[data-testid=course-review-file]"), function (f) {
        var n = f.querySelectorAll("td.rv-is-edited").length;
        edited += n;
        var fileBox = f.querySelector("input[name=file]");
        if (n && fileBox && fileBox.checked) { var d = fileBox.closest("details"); if (d) d.open = true; }
        immune(fileBox, n > 0, editedTitle);
        Array.prototype.forEach.call(f.querySelectorAll("input[name=lines]"), function (b) {
          var tr = b.closest("tr"), mine = !!tr.querySelector("td.rv-is-edited");
          tr.classList.toggle("rv-edited-row", mine);
          var inside = !mine && blockOf(tr).some(function (n) { return n.querySelector("td.rv-is-edited"); });
          immune(b, mine || inside, mine ? "" : t('courses.js.review_edited_line_tip'));
        });
        if (fileBox && fileBox.checked) { lessons++; return; }
        // A left-out group shows one − on its first row (it takes the whole
        // group back); the rest hide theirs.
        var seen = {};
        Array.prototype.forEach.call(f.querySelectorAll("input[name=lines]"), function (b) {
          var tr = b.closest("tr"), key = b.dataset.group && b.dataset.group !== "0" ? b.dataset.group : b.value;
          tr.classList.remove("rv-head", "rv-tail");
          if (!b.checked) return;
          tr.classList.add(seen[key] ? "rv-tail" : "rv-head");
          seen[key] = true;
        });
        groups += Object.keys(seen).length;
      });
      var parts = [];
      if (groups) parts.push(tn('courses.js.review_changes', groups));
      if (lessons) parts.push(tn('courses.js.review_lessons', lessons));
      var text = parts.length ? t('courses.js.review_left_out', { parts: parts.length === 1 ? parts[0] : t('courses.js.review_and', { a: parts[0], b: parts[1] }) }) : "";
      if (edited) text += (text ? t('courses.js.review_sep') : "") + tn('courses.js.review_lines_edited', edited);
      count.textContent = text;
      if (accept) accept.textContent = parts.length ? t('courses.js.accept_rest') : t('courses.js.accept');
    }

    // Editing a line in place (#287), like in a text editor: a click on a
    // line (Enter when it has the focus) swaps it for a box holding the raw JSON line,
    // which may be edited into several lines. ⌘/Ctrl+Enter or leaving the
    // box keeps it, Escape drops it. The edit rides with Accept in a hidden
    // `edit:<path>:<key>` field; typing the line back as it was, or Undo,
    // removes it. Each kept edit is a step of the modal's own Undo / Redo (in
    // the footer once there is one; Ctrl/⌘+Z, Ctrl/⌘+Shift+Z or Ctrl+Y when
    // no field has the focus). It lives in the page until Accept.
    var cellText = function (td) {
      var h = td.querySelector("input[type=hidden]");
      return h ? h.value : td.dataset.ai;
    };
    // The text as Accept writes it (review.rs `edited_lines`): lines without
    // trailing blanks, blank ones dropped.
    var normal = function (v) {
      return v.replace(/\r\n?/g, "\n").split("\n").map(function (l) { return l.replace(/\s+$/, ""); }).filter(function (l) { return l; }).join("\n");
    };
    // A line read as the server reads them (review.rs `readable`): its
    // indent, a label for its key and its value.
    var plainLabels = {};
    try { plainLabels = JSON.parse(modal.getAttribute("data-labels") || "{}"); } catch (e) { /* keys in words */ }
    var niceOf = function (line) {
      var t = line.replace(/^\s+/, ""), indent = Math.min(8, Math.floor((line.length - t.length) / 2));
      t = t.replace(/,$/, "");
      if (/^(\{|\}|\[|\]|\{\}|\[\])$/.test(t)) return { indent: indent, label: "", value: "" };
      var label = "", rest = t, m = /^"((?:[^"\\]|\\.)*)": (.*)$/.exec(t);
      if (m) {
        try {
          var key = JSON.parse('"' + m[1] + '"');
          // The plain names the server uses (review.rs LABELS, #282), else the key in words.
          label = Object.prototype.hasOwnProperty.call(plainLabels, key) ? plainLabels[key] : key.replace(/[_-]/g, " ");
          label = label.charAt(0).toUpperCase() + label.slice(1);
          rest = m[2];
        } catch (e) { /* not a key */ }
      }
      var value = rest;
      if (rest === "{" || rest === "[") value = "";
      else if (rest === "{}" || rest === "[]") value = window.t('courses.js.none');
      else if (rest.charAt(0) === '"') { try { value = JSON.parse(rest); } catch (e) { value = rest; } }
      return { indent: indent, label: label, value: value };
    };
    var niceSpans = function (text) {
      return text.split("\n").map(function (line) {
        var n = niceOf(line), span = document.createElement("span");
        span.className = "rv-nice";
        span.style.setProperty("--i", n.indent);
        if (n.label) {
          var k = document.createElement("span");
          k.className = "rv-k";
          k.textContent = n.label;
          span.appendChild(k);
          span.appendChild(document.createTextNode(" "));
        }
        var v = document.createElement("span");
        v.className = "rv-v";
        v.textContent = n.value;
        span.appendChild(v);
        return span;
      });
    };
    // The AI's line as the server drew it (with the changed part marked), kept
    // the first time a line not yet edited is shown.
    var aiNice = new WeakMap();
    var show = function (td, v) {
      v = normal(v);
      var raw = td.querySelector(".rv-raw"), h = td.querySelector("input[type=hidden]");
      var nices = Array.prototype.slice.call(td.querySelectorAll(".rv-nice"));
      if (!aiNice.has(td) && !td.classList.contains("rv-is-edited")) aiNice.set(td, nices.map(function (n) { return n.cloneNode(true); }));
      var edited = v !== td.dataset.ai;
      td.classList.toggle("rv-is-edited", edited);
      td.closest("tr").classList.toggle("rv-edited-row", edited);
      var fresh = !edited && aiNice.get(td) ? aiNice.get(td).map(function (n) { return n.cloneNode(true); }) : niceSpans(edited ? v : td.dataset.ai);
      fresh.forEach(function (n) { td.insertBefore(n, nices[0]); });
      nices.forEach(function (n) { n.remove(); });
      raw.textContent = edited ? v : td.dataset.ai;
      if (edited) {
        if (!h) {
          h = document.createElement("input");
          h.type = "hidden";
          h.name = td.dataset.name;
          h.setAttribute("form", "review-form");
          h.dataset.testid = "course-review-edit-value";
          td.appendChild(h);
        }
        h.value = v;
      } else if (h) {
        h.remove();
      }
      update();
    };
    var steps = [], at = 0;
    var historyBox = modal.querySelector("[data-testid=course-review-history]");
    var undoBtn = modal.querySelector("[data-testid=course-review-undo]");
    var redoBtn = modal.querySelector("[data-testid=course-review-redo]");
    var syncHistory = function () {
      historyBox.hidden = steps.length === 0;
      undoBtn.disabled = at === 0;
      redoBtn.disabled = at === steps.length;
    };
    var record = function (td, from, to) {
      steps.length = at;
      steps.push({ td: td, from: from, to: to });
      at = steps.length;
      syncHistory();
    };
    // Undo / Redo show the line they change: its lesson opens, it scrolls in.
    var step = function (back) {
      if (back ? at === 0 : at === steps.length) return false;
      var s = back ? steps[--at] : steps[at++];
      var d = s.td.closest("details");
      if (d) d.open = true;
      show(s.td, back ? s.from : s.to);
      if (s.td.scrollIntoView) s.td.scrollIntoView({ block: "nearest" });
      syncHistory();
      return true;
    };
    undoBtn.addEventListener("click", function () { step(true); });
    redoBtn.addEventListener("click", function () { step(false); });
    document.addEventListener("keydown", function (ev) {
      if (!(ev.ctrlKey || ev.metaKey) || ev.altKey) return;
      var a = document.activeElement;
      if (a && (a.isContentEditable || a.tagName === "TEXTAREA" || a.tagName === "SELECT" || (a.tagName === "INPUT" && !/^(checkbox|radio|button|submit)$/.test(a.type)))) return;
      var k = ev.key.toLowerCase();
      var back = k === "z" && !ev.shiftKey;
      var fwd = (k === "z" && ev.shiftKey) || (k === "y" && ev.ctrlKey && !ev.shiftKey);
      if ((back || fwd) && step(back)) ev.preventDefault();
    });
    var openEditor = function (td) {
      if (td.querySelector(".rv-editor")) return;
      var field = document.createElement("textarea");
      field.className = "rv-editor";
      field.dataset.testid = "course-review-edit-input";
      field.setAttribute("aria-label", t('courses.js.edit_line_label'));
      field.spellcheck = false;
      field.rows = 1;
      field.value = cellText(td);
      var grow = function () { field.style.height = "auto"; field.style.height = field.scrollHeight + "px"; };
      td.classList.add("rv-editing");
      td.appendChild(field);
      grow();
      field.focus();
      var done = false;
      var finish = function (keep) {
        if (done) return;
        done = true;
        var v = normal(field.value), was = cellText(td);
        field.remove();
        td.classList.remove("rv-editing");
        // Emptied: nothing changes (the − leaves a line out).
        if (keep && v && v !== normal(was)) { record(td, was, v); show(td, v); }
        if (document.activeElement === document.body) td.focus();
      };
      field.addEventListener("input", grow);
      field.addEventListener("keydown", function (ev) {
        if (ev.isComposing || ev.keyCode === 229) return; // Enter confirming an IME composition
        if (ev.key === "Escape") { ev.preventDefault(); ev.stopPropagation(); finish(false); }
        else if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) { ev.preventDefault(); finish(true); }
      });
      field.addEventListener("blur", function () { finish(true); });
    };
    modal.addEventListener("click", function (ev) {
      var t = ev.target;
      var td = t.closest && t.closest("td[data-edit]");
      if (!td || t.closest(".rv-editor")) return;
      // A click on the line (not the end of selecting some of it).
      var sel = window.getSelection && window.getSelection();
      if (!sel || sel.isCollapsed) openEditor(td);
    });
    modal.addEventListener("keydown", function (ev) {
      var td = ev.target.closest && ev.target.closest("td[data-edit]");
      if (ev.isComposing || ev.keyCode === 229) return;
      if (ev.key === "Enter" && td && ev.target === td) { ev.preventDefault(); openEditor(td); }
    });

    // A click on a line's −: leave it out alone, or take its group back.
    modal.addEventListener("change", function (ev) {
      var b = ev.target;
      if (b.name === "lines") {
        if (b.checked) leaveOut([b], true); else takeBack(b);
      } else if (b.name === "file") {
        var d = b.closest("details");
        // Left out: folded to its name; back in: open again, lines as they were.
        if (d) d.open = !b.checked;
        update();
      }
    });

    // A drag from a red −: every changed row it passes over, left out together.
    var drag = null;
    var rowAt = function (x, y) {
      var el = document.elementFromPoint(x, y);
      var tr = el && el.closest("tr.rv-row");
      return tr && drag && tr.parentNode === drag.body ? tr : null;
    };
    var span = function (a, b) {
      var rows = Array.prototype.slice.call(a.parentNode.children);
      var i = rows.indexOf(a), j = rows.indexOf(b);
      return rows.slice(Math.min(i, j), Math.max(i, j) + 1);
    };
    var mark = function (to) {
      Array.prototype.forEach.call(drag.body.querySelectorAll("tr.rv-sel"), function (tr) { tr.classList.remove("rv-sel"); });
      span(drag.from, to).forEach(function (tr) { var b = tr.querySelector("input[name=lines]"); if (b && !b.disabled) tr.classList.add("rv-sel"); });
      drag.to = to;
      // Only the band shows until the pointer goes up (style.css).
      modal.classList.add("rv-dragging");
    };
    modal.addEventListener("pointerdown", function (ev) {
      var b = ev.target.closest && ev.target.closest("input[name=lines]");
      if (!b || b.checked || b.disabled || ev.button !== 0) return;
      var tr = b.closest("tr");
      drag = { from: tr, to: tr, body: tr.parentNode, box: b };
      try { b.releasePointerCapture(ev.pointerId); } catch (e) { /* not captured */ }
    });
    document.addEventListener("pointermove", function (ev) {
      if (!drag) return;
      var tr = rowAt(ev.clientX, ev.clientY);
      if (tr && tr !== drag.to) { ev.preventDefault(); mark(tr); }
    });
    document.addEventListener("pointerup", function () {
      if (!drag) return;
      var d = drag;
      drag = null;
      if (d.to === d.from) { modal.classList.remove("rv-dragging"); return; } // a click: the change event does it
      // Unchanged rows in the span are not part of the group; left-out ones
      // are (see leaveOut).
      var boxes = [];
      span(d.from, d.to).forEach(function (tr) {
        var b = tr.querySelector("input[name=lines]");
        if (b && !b.disabled) boxes.push(b); // an edited line is skipped: it stays in
      });
      // (The click that ends a drag lands on no box, so nothing else toggles.)
      // The group's head and tails are set before the band and the
      // rv-dragging class go, so no row of − shows in between.
      leaveOut(boxes);
      Array.prototype.forEach.call(d.body.querySelectorAll("tr.rv-sel"), function (tr) { tr.classList.remove("rv-sel"); });
      modal.classList.remove("rv-dragging");
    });
    // A drag the browser cancels (a scroll took over) leaves nothing out.
    document.addEventListener("pointercancel", function () {
      if (!drag) return;
      Array.prototype.forEach.call(drag.body.querySelectorAll("tr.rv-sel"), function (tr) { tr.classList.remove("rv-sel"); });
      modal.classList.remove("rv-dragging");
      drag = null;
    });
    modal.addEventListener("dragstart", function (ev) { if (drag) ev.preventDefault(); });

    // Open / close all, and the files as JSON.
    Array.prototype.forEach.call(modal.querySelectorAll("[data-review-all]"), function (btn) {
      btn.hidden = false;
      btn.addEventListener("click", function () {
        var open = btn.dataset.reviewAll === "open";
        Array.prototype.forEach.call(modal.querySelectorAll("[data-testid=course-review-file] details"), function (d) { d.open = open; });
      });
    });
    var raw = modal.querySelector("[data-testid=course-review-raw]");
    if (raw) {
      raw.parentNode.hidden = false;
      raw.addEventListener("click", function () {
        var on = raw.getAttribute("aria-checked") !== "true";
        raw.setAttribute("aria-checked", String(on));
        modal.classList.toggle("show-raw", on);
      });
    }

    // "Don't show this automatically next time": saved as soon as it is ticked.
    var quiet = modal.querySelector("[data-testid=course-review-quiet]");
    quiet.addEventListener("change", function () {
      quiet.dataset.saved = "";
      fetch(base + "/api/review-quiet", { method: "POST", credentials: "same-origin", body: new URLSearchParams({ quiet: quiet.checked ? "1" : "0" }) })
        .then(function (r) { if (r.ok) quiet.dataset.saved = String(quiet.checked); })
        .catch(function () { /* sent again with Accept */ });
    });

    // Accept: one `lines` value per group ("path:key,key"), not one per box.
    form.addEventListener("submit", function () {
      var groups = {};
      Array.prototype.forEach.call(modal.querySelectorAll("input[name=lines]:checked"), function (b) {
        var at = b.value.lastIndexOf(":");
        var id = b.value.slice(0, at) + "\n" + (b.dataset.group || b.value);
        (groups[id] = groups[id] || { path: b.value.slice(0, at), keys: [] }).keys.push(b.value.slice(at + 1));
        b.disabled = true;
      });
      Object.keys(groups).forEach(function (id) {
        var input = document.createElement("input");
        input.type = "hidden";
        input.name = "lines";
        input.value = groups[id].path + ":" + groups[id].keys.join(",");
        form.appendChild(input);
      });
      var b = form.querySelector("button[type=submit]");
      if (b) { b.disabled = true; b.textContent = t('courses.js.saving'); }
    });
    update();
  }
  var review = document.getElementById("review-modal");
  if (review) reviewModal(review);

  // ---------- course list: courses being generated ----------
  // Each greyed-out card follows its job; when one finishes the page reloads
  // and the course shows as a normal one.
  Array.prototype.forEach.call(document.querySelectorAll(".course-card[data-job]"), function (card) {
    var job = card.dataset.job;
    var phase = card.querySelector(".gen-phase");
    var fill = card.querySelector(".bar .fill");
    function tick() {
      fetch(base + "/api/generate/" + job, { credentials: "same-origin" }).then(function (r) { return r.ok ? r.json() : null; }).then(function (s) {
        if (!s) return; // the job is gone (server restarted): the reload after a restart shows it stopped
        // Reload once, when the card is still drawn as generating; a card the
        // server already shows stopped or ready has nothing to catch up on.
        if (s.done) { if (card.dataset.courseStatus === "generating") window.location.reload(); return; }
        if (phase) phase.textContent = t('courses.js.card_progress', { phase: s.phase, done: s.lessons_done, total: s.lessons_total });
        if (fill) fill.style.width = (s.lessons_total ? Math.round(s.lessons_done * 100 / s.lessons_total) : 0) + "%";
        var h = card.querySelector("h3");
        if (s.course_title && h && h.firstChild && h.firstChild.nodeType === 3) h.firstChild.textContent = s.course_title + " ";
        setTimeout(tick, 4000);
      }).catch(function () { setTimeout(tick, 8000); });
    }
    setTimeout(tick, 2000);
  });

  // ---------- generate form ----------
  wireGenForm(document.getElementById("gen-form"));
  function wireGenForm(form) {
    if (!form) return;
    // Provider (two or more saved keys): the disclosure names the one chosen.
    function provider() {
      var r = form.querySelector("input[name=provider]:checked");
      var label = document.getElementById("gen-disclosure-provider");
      if (!r || !label) return;
      label.textContent = r.dataset.label || t('courses.js.your_provider');
    }
    Array.prototype.forEach.call(form.querySelectorAll("input[name=provider]"), function (r) { r.addEventListener("change", provider); });
    provider();

    var wish = document.getElementById("gen-wish");
    charCount(wish, document.getElementById("gen-wish-count"));

    // Notes: count, and text files read into the box (nothing is uploaded until submit).
    var notes = document.getElementById("gen-notes");
    var count = document.getElementById("gen-notes-count");
    if (notes) {
      var max = boxMax(notes);
      charCount(notes, count);
      var drop = document.getElementById("gen-drop");
      var files = document.getElementById("gen-files");
      var addFiles = function (list) {
        var chosen = Array.prototype.slice.call(list).filter(function (f) { return f.size < 2 * 1024 * 1024; });
        if (!chosen.length) { count.textContent = t('courses.js.files_too_large'); count.dataset.state = "bad"; return; }
        Promise.all(chosen.map(function (f) {
          return f.text().then(function (t) { return "## " + f.name + "\n\n" + t.trim(); });
        })).then(function (parts) {
          var text = (notes.value.trim() ? notes.value.trim() + "\n\n" : "") + parts.join("\n\n");
          if (/[\u0000-\u0008\u000E-\u001F]/.test(text)) { count.textContent = t('courses.js.not_text_file'); count.dataset.state = "bad"; return; }
          notes.value = text.slice(0, max);
          notes.dispatchEvent(new Event("input", { bubbles: true })); // the count, and the estimate (gen-estimate.js)
        });
      };
      document.getElementById("gen-pick").addEventListener("click", function () { files.click(); });
      files.addEventListener("change", function () { addFiles(files.files); files.value = ""; });
      ["dragenter", "dragover"].forEach(function (t) { drop.addEventListener(t, function (ev) { ev.preventDefault(); drop.classList.add("over"); }); });
      ["dragleave", "drop"].forEach(function (t) { drop.addEventListener(t, function (ev) { ev.preventDefault(); drop.classList.remove("over"); }); });
      drop.addEventListener("drop", function (ev) { if (ev.dataTransfer && ev.dataTransfer.files.length) addFiles(ev.dataTransfer.files); });
    }

    // Instructions: mark edits, reset to the default.
    var instr = document.getElementById("gen-instructions");
    var def = document.getElementById("gen-default");
    var edited = document.getElementById("gen-edited");
    if (instr && def) {
      var original = def.content ? def.content.textContent : def.textContent;
      var mark = function () { edited.hidden = instr.value.trim() === original.trim(); };
      instr.addEventListener("input", mark);
      document.getElementById("gen-reset").addEventListener("click", function () { instr.value = original; mark(); });
      mark();
    }
    // Checked here before posting, with the server's rules and words
    // (generate_in), so a refusal opens in place instead of reloading the
    // page: the error slides open at the top, the dialog scrolls up to it
    // smoothly and the box to fix is marked, as on the profile page.
    var errSlot = document.getElementById("gen-error-slot");
    var err = document.getElementById("gen-error");
    var checked = false; // the error on show is this script's, not the server's
    function problem() {
      if (!wish && !notes) return null; // resuming: nothing to type
      var w = wish ? promptLength(wish) : 0, n = notes ? promptLength(notes) : 0;
      var min = parseInt((wish || notes).dataset.min, 10) || 0;
      var short = function (len) { return len > 0 && len < min; };
      if (wish && short(w)) return { box: wish, text: form.dataset.tooShort };
      if (notes && short(n)) return { box: notes, text: form.dataset.tooShort };
      if (w === 0 && n === 0 && !form.hasAttribute("data-named-title")) return { box: wish || notes, text: form.dataset.tooShort };
      if (wish && w > boxMax(wish)) return { box: wish, text: form.dataset.wishTooLong };
      if (notes && n > boxMax(notes)) return { box: notes, text: form.dataset.notesTooLong };
      return null;
    }
    function markInvalid(box) {
      [wish, notes].forEach(function (b) { if (b) b.toggleAttribute("aria-invalid", b === box); });
    }
    function showError(p) {
      checked = true;
      markInvalid(p.box);
      if (!errSlot || !err) return;
      err.textContent = p.text;
      err.hidden = false;
      errSlot.setAttribute("data-shown", "");
      p.box.focus({ preventScroll: true });
      // The slot opens over a few frames; scroll once its height is settled
      // enough that the error lands fully in view.
      requestAnimationFrame(function () {
        errSlot.scrollIntoView({ block: "start", behavior: scrollBehavior() });
      });
    }
    function clearError() {
      if (!checked) return;
      checked = false;
      markInvalid(null);
      if (!errSlot) return;
      errSlot.removeAttribute("data-shown");
      // Hidden once it has closed, so screen readers and tests no longer find it.
      setTimeout(function () { if (!errSlot.hasAttribute("data-shown")) err.hidden = true; }, 400);
    }
    [wish, notes].forEach(function (b) {
      if (b) b.addEventListener("input", function () { if (checked && !problem()) clearError(); else if (checked) markInvalid(problem().box); });
    });
    form.addEventListener("submit", function (ev) {
      var p = problem();
      if (p) { ev.preventDefault(); showError(p); return; }
      // The editor's edits are saved first; then it goes (asking, if they could not be).
      var saver = editor && editor.saver;
      if (saver && saver.state() !== "saved" && !form.dataset.settled) {
        ev.preventDefault();
        saver.settle().then(function () { form.dataset.settled = "1"; form.requestSubmit(ev.submitter || undefined); });
        return;
      }
      delete form.dataset.settled;
      var b = form.querySelector("button[type=submit]");
      if (b) { b.disabled = true; b.textContent = t('courses.js.starting'); }
    });
  }
})();
