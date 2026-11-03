// The course editor's advice badge (#1235, proposal C of #1198). Each lesson
// row whose items have advice (LessonEditor.checks at level "warn": no
// explanation, two options that read the same, a blank missing, an example
// without its meaning) gets a yellow ! after its title. Any of them opens one
// "Things to check" modal (#lt-advice-dialog) listing the advice of every
// lesson, grouped by lesson and scrolled to the one clicked; a row of it opens
// that lesson's drawer in Forms mode at the item's field. Red checks (what a
// save would refuse) are not advice and are not listed here.
//
// A row's advice comes from its JSON box once the drawer has loaded it, else
// from the saved file (fetched once, four at a time). courses.js calls
// Advice.row(row) whenever it draws a row's status (typed, loaded, saved) and
// lends LessonTable.open(row) and LessonTable.manifest().
//
//   Advice.list(doc, man) -> [{ sel, label, path, text }]   one lesson's advice
(function () {
  "use strict";
  var panel = document.getElementById("lessons"), table = document.getElementById("lt-list"), dialog = document.getElementById("lt-advice-dialog");
  // Texts under editor.js in i18n/<code>.yaml (built keys, as in lesson-editor.js).
  var PREFIX = "editor.js.";
  var T = function (key, args) { return t(PREFIX + key, args); };
  var ready = !!(panel && table && dialog && window.LessonEditor && window.LessonTable);
  var base = document.body.dataset.base || "", course = panel && panel.dataset.courseId;
  var man = {}, saved = {}, advice = new WeakMap(), queue = [], running = 0;

  // One lesson's advice: every item of every section, the "warn" checks only.
  function list(doc, m) {
    var out = [];
    if (!doc || typeof doc !== "object" || Array.isArray(doc)) return out;
    LessonEditor.sections(doc, m).forEach(function (s) {
      var sels = s.single ? [{ sec: s.key }] : (s.items || []).map(function (it) { return it.sel; });
      sels.forEach(function (sel) {
        LessonEditor.checks(doc, m, sel).forEach(function (c) {
          if (c.level === "warn") out.push({ sel: sel, label: s.label + (sel.idx != null ? " " + (sel.idx + 1) : ""), path: c.path, text: c.text });
        });
      });
    });
    return out;
  }

  function docOf(row) {
    var ta = row.querySelector(".lt-json");
    if (ta && ta.dataset.loaded) { try { return JSON.parse(ta.value); } catch (e) { return undefined; } }
    return row.dataset.path ? saved[row.dataset.path] : null;
  }
  function draw(row) {
    var doc = docOf(row);
    if (doc === undefined) return; // JSON mid-typing that does not parse: keep what it had
    var items = doc ? list(doc, man) : [];
    advice.set(row, items);
    var b = row.querySelector(".lt-advice");
    if (b) {
      b.hidden = !items.length;
      var say = items.length ? tn(PREFIX + "advice_count", items.length) : "";
      b.setAttribute("aria-label", say);
      b.dataset.tip = say;
    }
    if (dialog.open) fill();
  }
  // The saved files of rows whose drawers never opened, four at a time.
  function fetchSaved(row) {
    if (!row.dataset.path || saved[row.dataset.path] !== undefined) { draw(row); return; }
    queue.push(row);
    pump();
  }
  function pump() {
    while (running < 4 && queue.length) {
      var row = queue.shift(), path = row.dataset.path;
      running++;
      fetch(base + "/api/courses/" + course + "/file?path=" + encodeURIComponent(path), { credentials: "same-origin" })
        .then(function (r) { return r.ok ? r.json() : null; })
        .then(function (row, path) { return function (f) { try { saved[path] = f ? JSON.parse(f.content) : null; } catch (e) { saved[path] = null; } draw(row); }; }(row, path))
        .catch(function () {})
        .then(function () { running--; pump(); });
    }
  }
  var rows = function () { return Array.prototype.slice.call(table.querySelectorAll(".lt-row")); };

  // ----- the modal -----
  function el(tag, attrs, kids) {
    var n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) { if (k === "text") n.textContent = attrs[k]; else n.setAttribute(k, attrs[k]); });
    (kids || []).forEach(function (k) { if (k) n.appendChild(k); });
    return n;
  }
  function fill() {
    var box = dialog.querySelector(".advice-list");
    box.textContent = "";
    var with_ = rows().filter(function (r) { return (advice.get(r) || []).length; });
    if (!with_.length) { box.appendChild(el("p", { class: "muted", "data-testid": "course-advice-none", text: T("advice_none") })); return; }
    with_.forEach(function (r) {
      var title = r.querySelector(".lt-title").textContent.trim(), num = r.querySelector(".lt-num").textContent.trim();
      var group = el("section", { class: "advice-lesson", "data-testid": "course-advice-lesson", "data-lesson-id": r.dataset.lessonId || "" }, [
        el("h3", { text: (num ? num + " · " : "") + title }),
        el("ul", {}, advice.get(r).map(function (it) {
          var b = el("button", { type: "button", class: "advice-item", "data-testid": "course-advice-item", "data-path": it.path }, [
            el("span", { class: "field-check advice", "aria-hidden": "true", text: "!" }),
            el("span", {}, [el("strong", { text: it.label }), document.createTextNode(" · " + it.text)])
          ]);
          b._go = function () { go(r, it); };
          return el("li", {}, [b]);
        }))
      ]);
      group._row = r;
      box.appendChild(group);
    });
  }
  function show(row) {
    fill();
    if (!dialog.open) dialog.showModal();
    var group = Array.prototype.filter.call(dialog.querySelectorAll(".advice-lesson"), function (g) { return g._row === row; })[0];
    if (group) group.scrollIntoView({ block: "start" });
  }
  // The item's lesson open in Forms mode, the item picked and its field focused.
  function go(row, it) {
    dialog.close();
    LessonTable.open(row);
    var box = row.querySelector(".lt-form"), drawer = row.querySelector(".lt-drawer"), tries = 0;
    (function wait() {
      if (drawer.dataset.mode === "forms" && box._le && !box.hidden) { LessonEditor.select(box, it.sel, it.path); return; }
      if (++tries < 100) setTimeout(wait, 50);
    })();
  }

  if (ready) {
    table.addEventListener("click", function (ev) {
      var b = ev.target.closest && ev.target.closest(".lt-advice");
      if (b) show(b.closest(".lt-row"));
    });
    dialog.addEventListener("click", function (ev) {
      var b = ev.target.closest && ev.target.closest(".advice-item");
      if (b && b._go) b._go();
    });
    // Rows the table takes in (Undo and Redo bring fresh ones) are read again.
    new MutationObserver(function (muts) {
      muts.forEach(function (m) {
        Array.prototype.forEach.call(m.addedNodes, function (n) {
          if (n.nodeType !== 1 || !n.classList.contains("lt-row") || advice.has(n)) return;
          if (n.dataset.path) delete saved[n.dataset.path];
          fetchSaved(n);
        });
      });
    }).observe(table, { childList: true });
    LessonTable.manifest().catch(function () { return {}; }).then(function (m) { man = m || {}; rows().forEach(fetchSaved); });
  }

  window.Advice = {
    list: list,
    // A row's JSON was drawn again; the settings' JSON changes every row's.
    row: function (row) {
      if (!ready) return;
      if (row.dataset.path === "course.json") { LessonTable.manifest().then(function (m) { man = m || {}; rows().forEach(draw); }, function () {}); return; }
      if (row.classList.contains("lt-row")) draw(row);
    }
  };
})();
