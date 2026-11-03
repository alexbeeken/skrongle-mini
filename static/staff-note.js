// Place the note (#1226). A staff-note question shows an empty staff with a
// clef; a tap anywhere on it puts the note on the nearest line or space, a
// drag moves it, and the flat and sharp buttons add an accidental. With the
// keyboard, the up and down arrows move a ghost note and Enter places it.
// The answer is the note placed, in ABC with its octave (G, ^F, c'), which
// the server grades (src/notation/staff_note.rs). abcjs draws but takes no
// input, so this staff is drawn here.
//
// In Practice, the reading check and the Guide the page has a .staff-note
// box (data-clef, data-names: every note spelling in the course's note
// names) and a hidden radio (.staff-note-answer) that holds the answer once
// a note is placed, so quiz.js, lesson-check.js and guide.js post it like a
// picked option. Sprint mode and the lesson editor's preview call
// StaffNote.mount themselves.
(function () {
  "use strict";
  var SRC = (document.currentScript && document.currentScript.src) || "";
  if (SRC) {
    var css = document.createElement("link");
    css.rel = "stylesheet";
    css.href = SRC.replace("staff-note.js", "staff-note.css");
    document.head.appendChild(css);
  }
  var NS = "http://www.w3.org/2000/svg";
  // Steps (a line or a space) from the bottom line, as in staff_note.rs.
  var LOW = -5, HIGH = 13, STEP = 5, TOP = 14, W = 180, H = TOP * 2 + (HIGH - LOW) * STEP, NX = 112;
  var BOTTOM = { treble: 30, bass: 18 }; // E4 and G2, one step per letter, middle C = 28
  // Each clef's ink spans from step `top` to step `bottom`; the glyph is sized to fit (fitClef).
  var CLEF = { treble: { glyph: "𝄞", top: 11, bottom: -3 }, bass: { glyph: "𝄢", top: 8.3, bottom: 1.6 } };
  var FONT = '"Bravura", "Noto Music", "Apple Symbols", "Segoe UI Symbol", "Noto Sans Symbols 2", serif';
  var LETTERS = "CDEFGAB";

  function y(s) { return TOP + (HIGH - s) * STEP; }
  function svgEl(tag, attrs, parent) {
    var e = document.createElementNS(NS, tag);
    for (var k in attrs) e.setAttribute(k, attrs[k]);
    return parent ? parent.appendChild(e) : e;
  }
  function abcOf(step) {
    var oct = Math.floor(step / 7), l = LETTERS.charAt(step - oct * 7);
    return oct >= 5 ? l.toLowerCase() + new Array(oct - 4).join("'") : l + new Array(5 - oct).join(",");
  }
  // An ABC note: { step, acc } or null.
  function parse(abc) {
    var m = /^(\^|_|=)?([A-Ga-g])([,']*)$/.exec(String(abc).trim());
    if (!m) return null;
    var oct = m[2] === m[2].toLowerCase() ? 5 : 4;
    for (var i = 0; i < m[3].length; i++) oct += m[3].charAt(i) === "'" ? 1 : -1;
    return { step: oct * 7 + LETTERS.indexOf(m[2].toUpperCase()), acc: m[1] === "=" ? "" : m[1] || "" };
  }
  // Where step `s` is, for a screen reader.
  function where(s) {
    if (s >= 0 && s <= 8) return t(s % 2 ? "common.js.staff_note_space" : "common.js.staff_note_line", { n: s % 2 ? (s + 1) / 2 : s / 2 + 1 });
    var below = s < 0, d = below ? -s : s - 8;
    if (d === 1) return t(below ? "common.js.staff_note_just_below" : "common.js.staff_note_just_above");
    if (d % 2) return t(below ? "common.js.staff_note_under_ledger" : "common.js.staff_note_over_ledger", { n: (d - 1) / 2 });
    return t(below ? "common.js.staff_note_ledger_below" : "common.js.staff_note_ledger_above", { n: d / 2 });
  }

  // Clef glyphs differ from font to font, so the glyph's ink is measured on
  // a canvas and the text sized and placed to span its steps.
  function fitClef(text, c) {
    text.textContent = c.glyph;
    text.style.fontFamily = FONT;
    var size = 40, asc = 30, desc = 10;
    try {
      var ctx = document.createElement("canvas").getContext("2d");
      ctx.font = "100px " + FONT;
      var m = ctx.measureText(c.glyph);
      if (m.actualBoundingBoxAscent + m.actualBoundingBoxDescent > 0) { asc = m.actualBoundingBoxAscent; desc = m.actualBoundingBoxDescent; size = 100; }
    } catch (e) { /* no canvas: the rough size above */ }
    var k = (y(c.bottom) - y(c.top)) / (asc + desc);
    text.setAttribute("font-size", size * k);
    text.setAttribute("y", y(c.top) + asc * k);
  }

  // A note head with its ledger lines and accidental, at step `s` and x `x`.
  function head(g, s, acc, x, cls, testid) {
    while (g.firstChild) g.removeChild(g.firstChild);
    if (s === null) return;
    for (var l = s < 0 ? (s % 2 ? s + 1 : s) : 10; s < 0 ? l <= -2 : l <= s; l += 2) {
      svgEl("line", { x1: x - 11, x2: x + 11, y1: y(l), y2: y(l), "class": "staff-note-ledger", "data-testid": testid + "-ledger" }, g);
    }
    if (acc) svgEl("text", { x: x - 18, y: y(s) + 4, "font-size": 14, "text-anchor": "middle", "class": "staff-note-acc" }, g).textContent = acc === "^" ? "♯" : "♭";
    svgEl("ellipse", { cx: x, cy: y(s), rx: 6.2, ry: 4.4, transform: "rotate(-20 " + x + " " + y(s) + ")", "class": cls, "data-testid": testid }, g);
  }

  // Build the question in `root` (clef: "treble" or "bass"; names: ABC
  // spelling -> name in the course's note names). onChange(answer) after
  // every change: the note in ABC, or null. Returns { lock(state), reveal(container) }.
  function mount(root, clef, names, onChange) {
    var bottom = BOTTOM[clef] || BOTTOM.treble, placed = null, cursor = 4, acc = "", locked = false, drag = false;
    root.innerHTML = "";
    if (!root.dataset.testid) root.dataset.testid = "staff-note";
    var svg = svgEl("svg", { viewBox: "0 0 " + W + " " + H, "class": "staff-note-staff", "data-testid": "staff-note-staff", "data-clef": clef, tabindex: 0, role: "slider",
      "aria-label": t("common.js.staff_note_label"), "aria-valuemin": LOW, "aria-valuemax": HIGH }, root);
    for (var s = 0; s <= 8; s += 2) svgEl("line", { x1: 4, x2: W - 4, y1: y(s), y2: y(s), "class": "staff-note-line" }, svg);
    var c = CLEF[clef] || CLEF.treble;
    fitClef(svgEl("text", { x: 6, "class": "staff-note-clef", "aria-hidden": "true" }, svg), c);
    var ghost = svgEl("g", { "class": "staff-note-ghost-g" }, svg), right = svgEl("g", { "class": "staff-note-right-g" }, svg), note = svgEl("g", {}, svg);
    var accs = root.appendChild(document.createElement("div"));
    accs.className = "staff-note-accs";
    var accBtns = [["_", "♭", "flat", t("common.js.staff_note_flat")], ["^", "♯", "sharp", t("common.js.staff_note_sharp")]].map(function (a) {
      var b = accs.appendChild(document.createElement("button"));
      b.type = "button"; b.className = "staff-note-accbtn"; b.textContent = a[1];
      b.dataset.acc = a[0]; b.dataset.testid = "staff-note-" + a[2];
      b.setAttribute("aria-label", a[3]);
      b.addEventListener("click", function () { if (locked) return; acc = acc === a[0] ? "" : a[0]; update(); });
      return b;
    });

    function answer() { return placed === null ? null : acc + abcOf(bottom + placed); }
    function update(quiet) {
      head(note, placed, acc, NX, "staff-note-head", "staff-note-head");
      head(ghost, locked ? null : cursor, "", NX, "staff-note-ghost", "staff-note-ghost");
      accBtns.forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.acc === acc)); b.disabled = locked; });
      svg.setAttribute("aria-valuenow", cursor);
      svg.setAttribute("aria-valuetext", where(cursor));
      root.dataset.answer = answer() || "";
      if (!quiet) onChange(answer());
    }
    function stepAt(ev) {
      var r = svg.getBoundingClientRect();
      var s = Math.round(HIGH - ((ev.clientY - r.top) * H / r.height - TOP) / STEP);
      return Math.max(LOW, Math.min(HIGH, s));
    }
    function place(s) { placed = cursor = s; update(); }
    svg.addEventListener("pointerdown", function (ev) {
      if (locked) return;
      ev.preventDefault();
      drag = true;
      try { svg.setPointerCapture(ev.pointerId); } catch (e) { /* a synthetic event has no pointer to capture */ }
      place(stepAt(ev));
    });
    svg.addEventListener("pointermove", function (ev) { if (drag && !locked && stepAt(ev) !== placed) place(stepAt(ev)); });
    ["pointerup", "pointercancel"].forEach(function (n) { svg.addEventListener(n, function () { drag = false; }); });
    svg.addEventListener("keydown", function (ev) {
      if (locked) return;
      var k = ev.key;
      if (k === "ArrowUp" || k === "ArrowRight" || k === "ArrowDown" || k === "ArrowLeft") {
        ev.preventDefault();
        cursor = Math.max(LOW, Math.min(HIGH, cursor + (k === "ArrowUp" || k === "ArrowRight" ? 1 : -1)));
        update(true);
      } else if ((k === "Enter" || k === " ") && (placed !== cursor || k === " ")) {
        // Enter places the note; on a note already placed it goes on to the page (Check).
        ev.preventDefault();
        ev.stopPropagation();
        place(cursor);
      }
    });
    update(true);

    return {
      lock: function (state) { locked = true; if (state) root.dataset.state = state; update(true); },
      // The right answer, shown in ABC in a graded answer's <strong>: drawn
      // on the staff beside the learner's note, and named in the course's note names.
      reveal: function (container) {
        if (!container) return;
        container.querySelectorAll("strong").forEach(function (el) {
          var p = !el.dataset.testid && parse(el.textContent);
          if (!p || p.step - bottom < LOW || p.step - bottom > HIGH) return;
          head(right, p.step - bottom, p.acc, NX + 34, "staff-note-right", "staff-note-right");
          el.textContent = (names && names[p.acc + LETTERS.charAt(((p.step % 7) + 7) % 7)]) || el.textContent;
          el.dataset.testid = "staff-note-answer";
        });
      }
    };
  }

  window.StaffNote = { mount: mount };

  // Practice, the reading check and the Guide.
  function auto() {
    document.querySelectorAll(".staff-note[data-clef]").forEach(function (root) {
      var q = root.closest(".question");
      var radio = q && q.querySelector("input.staff-note-answer");
      var fb = q && q.querySelector(".feedback");
      var names = {};
      try { names = JSON.parse(root.dataset.names || "{}"); } catch (e) { /* names stay ABC */ }
      var w = mount(root, root.dataset.clef, names, function (answer) {
        if (!radio) return;
        radio.value = answer || "";
        radio.checked = !!answer;
        radio.dispatchEvent(new Event("change", { bubbles: true }));
      });
      if (fb) new MutationObserver(function () {
        if (fb.dataset.state !== "right" && fb.dataset.state !== "wrong") return;
        w.lock(fb.dataset.state);
        w.reveal(fb);
      }).observe(fb, { attributes: true, childList: true });
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", auto);
  else auto();
})();
