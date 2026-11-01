// Spell the chord (#1228). A chord-spell question has note tiles (the note
// in ABC in data-tile, its name in the course's note names as the text) and
// as many empty slots as the chord has notes. A tap on a tile puts it in the
// first empty slot, a tap on a filled slot takes it out again, and the staff
// above draws the notes in the slots as one chord (window.drawStaff from
// notation.js). Once every slot is full the answer is the notes, separated by
// spaces, which the server grades as a set (src/notation/chord_spell.rs).
//
// In Practice, the reading check and the Guide the page has a .chord-spell
// box with the tiles as buttons and a hidden radio (.chord-spell-answer) that
// holds the answer once the slots are full, so quiz.js, lesson-check.js and
// guide.js post it like a picked option. Sprint mode and the lesson editor's
// preview call ChordSpell.mount themselves.
(function () {
  "use strict";
  var SRC = (document.currentScript && document.currentScript.src) || "";
  if (SRC) {
    var css = document.createElement("link");
    css.rel = "stylesheet";
    css.href = SRC.replace("chord-spell.js", "chord-spell.css");
    document.head.appendChild(css);
  }

  function el(tag, cls) { var e = document.createElement(tag); if (cls) e.className = cls; return e; }

  // Build the question in `root`: the staff, the slots and the tiles
  // ([{ abc, name }]). onChange(answer) after every tap: the notes once
  // every slot is full, else null. Returns { lock(), reveal(container) }.
  function mount(root, tiles, slots, onChange) {
    var filled = [], locked = false;
    for (var k = 0; k < slots; k++) filled.push(null);
    root.innerHTML = "";
    if (!root.dataset.testid) root.dataset.testid = "chord-spell";
    var staff = root.appendChild(el("div", "chord-spell-staff"));
    staff.dataset.testid = "chord-spell-staff";
    var slotRow = root.appendChild(el("div", "chord-spell-slots"));
    var slotEls = filled.map(function (_, i) {
      var s = slotRow.appendChild(el("button", "chord-spell-slot"));
      s.type = "button";
      s.dataset.testid = "chord-spell-slot";
      s.setAttribute("aria-label", t("common.js.chord_spell_slot", { n: i + 1 }));
      s.addEventListener("click", function () {
        if (locked || filled[i] === null) return;
        filled[i] = null;
        update();
      });
      return s;
    });
    var row = root.appendChild(el("div", "chord-spell-tiles"));
    var buttons = tiles.map(function (tile, i) {
      var b = row.appendChild(el("button", "chord-spell-tile"));
      b.type = "button";
      b.dataset.tile = tile.abc;
      b.dataset.testid = "chord-spell-tile";
      b.textContent = tile.name;
      b.addEventListener("click", function () {
        var free = filled.indexOf(null);
        if (locked || free < 0 || filled.indexOf(i) >= 0) return;
        filled[free] = i;
        update();
      });
      return b;
    });

    function notes() { return filled.filter(function (i) { return i !== null; }).map(function (i) { return tiles[i].abc; }); }
    function chordAbc(list) { return "X:1\nL:1/1\nK:C\n" + (list.length ? "[" + list.join("") + "]" : "x") + "|"; }
    // abcjs measures text as it draws, so the staff is drawn only while it shows.
    function paint() {
      var abc = chordAbc(notes());
      staff.dataset.abc = abc;
      if (!window.drawStaff || !root.getClientRects().length) return;
      staff.innerHTML = "";
      window.drawStaff(staff.appendChild(el("div")), abc, { emptyOk: true });
    }
    function update(quiet) {
      var full = filled.indexOf(null) < 0;
      slotEls.forEach(function (s, i) {
        var tile = filled[i] === null ? null : tiles[filled[i]];
        s.textContent = tile ? tile.name : "";
        s.dataset.tile = tile ? tile.abc : "";
        s.disabled = locked || !tile;
      });
      buttons.forEach(function (b, i) { b.disabled = locked || full || filled.indexOf(i) >= 0; });
      root.dataset.full = full ? "true" : "false";
      paint();
      if (!quiet) onChange(full ? notes().join(" ") : null);
    }
    if (window.IntersectionObserver) {
      new IntersectionObserver(function (es) { es.forEach(function (e) { if (e.isIntersecting || root.getClientRects().length) paint(); }); }).observe(root);
    }
    update();

    // The names of `text` when it is notes of this question's tiles, else null.
    function named(text) {
      var parts = String(text).trim().split(/\s+/), out = [];
      for (var i = 0; i < parts.length; i++) {
        var tile = tiles.filter(function (x) { return x.abc === parts[i]; })[0];
        if (!tile) return null;
        out.push(tile.name);
      }
      return parts[0] ? out.join(" ") : null;
    }
    return {
      lock: function () { locked = true; update(true); },
      // The right answer, shown in ABC in a graded answer's <strong>, in the course's note names.
      reveal: function (container) {
        if (!container) return;
        container.querySelectorAll("strong").forEach(function (s) {
          var n = !s.dataset.testid && named(s.textContent);
          if (n) { s.textContent = n; s.dataset.testid = "chord-spell-answer"; }
        });
      }
    };
  }

  window.ChordSpell = { mount: mount };

  // Practice, the reading check and the Guide.
  function auto() {
    document.querySelectorAll(".chord-spell[data-slots]").forEach(function (root) {
      var q = root.closest(".question");
      var radio = q && q.querySelector("input.chord-spell-answer");
      var fb = q && q.querySelector(".feedback");
      var tiles = Array.prototype.map.call(root.querySelectorAll(".chord-spell-tile"), function (b) { return { abc: b.dataset.tile, name: b.textContent }; });
      var w = mount(root, tiles, Number(root.dataset.slots), function (answer) {
        if (!radio) return;
        radio.value = answer || "";
        radio.checked = !!answer;
        radio.dispatchEvent(new Event("change", { bubbles: true }));
      });
      if (fb) new MutationObserver(function () {
        if (fb.dataset.state !== "right" && fb.dataset.state !== "wrong") return;
        w.lock();
        w.reveal(fb);
      }).observe(fb, { attributes: true, childList: true });
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", auto);
  else auto();
})();
