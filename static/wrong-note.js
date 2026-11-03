// Fix the wrong note (#1227): a Sprint spot with `abc` is a staff with one
// wrong note. The learner taps the note (abcjs's click listener says which;
// notes are counted in the order the staff draws them, as
// src/notation.rs abc_notes counts them on the server), then picks its fix
// from three tiles. Dragging a note up or down on the staff picks the tile
// for the note it lands on (from the keyboard: focus a note, arrow up or
// down, Enter). sprint.js mounts it and posts
// { note: index from 0, fix: the tile's name } to /api/sprint/check.
(function () {
  "use strict";
  var LETTERS = "CDEFGAB";
  // The tune's notes (rests left out) in order: abcjs elements.
  function notesOf(tune) {
    var out = [];
    ((tune && tune.lines) || []).forEach(function (line) {
      (line.staff || []).forEach(function (st) {
        (st.voices || []).forEach(function (v) {
          v.forEach(function (el) { if (el.el_type === "note" && !el.rest) out.push(el); });
        });
      });
    });
    return out;
  }
  function paint(el, color) {
    ((el && el.abselem && el.abselem.elemset) || []).forEach(function (g) {
      g.setAttribute("fill", color);
      g.querySelectorAll("path").forEach(function (p) { p.setAttribute("fill", color); });
    });
  }

  // `tiles` are { name, letter }; onChange gets { note, fix } once both are
  // picked, else null. Returns { reveal(res) } to show the grade.
  window.WrongNote = {
    mount: function (box, abc, tiles, onChange) {
      var note = null, fix = null, notes = [], btns = [];
      var staff = box.appendChild(document.createElement("div"));
      staff.className = "sprint-staff wrong-note-staff";
      var bank = box.appendChild(document.createElement("div"));
      bank.className = "sprint-bank wrong-note-tiles";
      bank.dataset.testid = "wrong-note-tiles";
      bank.hidden = true;
      function changed() { onChange(note !== null && fix !== null ? { note: note, fix: fix } : null); }
      function choose(b) {
        btns.forEach(function (x) { x.classList.toggle("sel", x === b); x.setAttribute("aria-pressed", x === b ? "true" : "false"); });
        fix = b ? b.dataset.value : null;
        changed();
      }
      tiles.forEach(function (tile) {
        var b = document.createElement("button");
        b.type = "button";
        b.className = "sprint-tile";
        b.dataset.testid = "wrong-note-tile";
        b.dataset.value = tile.name;
        b.dataset.letter = tile.letter;
        b.textContent = tile.name;
        b.addEventListener("click", function () { choose(b); });
        btns.push(b);
        bank.appendChild(b);
      });
      var css = getComputedStyle(document.body);
      var sky = css.getPropertyValue("--sky").trim() || "#3b82f6";
      // Drawn at its own size (abcjs drags in its own units, so a staff scaled
      // to fit would move a dragged note further than the finger), as wide as
      // the page allows up to 24rem; every note is a tap target.
      var width = Math.min(box.clientWidth || 300, 384);
      window.drawStaff(staff, abc, {
        staffwidth: width - 12,
        selectionColor: sky,
        dragging: true,
        // Only notes take taps; abcjs makes each focusable: the arrow keys move it, Enter picks it.
        selectTypes: ["note"],
        clickListener: function (el, tuneNumber, classes, analysis, drag) {
          var i = notes.indexOf(el);
          if (i < 0) return;
          note = i;
          staff.dataset.note = i;
          bank.hidden = false;
          // Dragged up or down: the tile for the note it lands on.
          var p = el.pitches && el.pitches[0];
          if (drag && drag.step && p) {
            var letter = LETTERS.charAt((((p.pitch - drag.step) % 7) + 7) % 7);
            var b = btns.filter(function (x) { return x.dataset.letter === letter; })[0];
            if (b) { choose(b); return; }
          }
          changed();
        }
      });
      staff.style.maxWidth = width + "px";
      notes = notesOf(staff.abcTune);
      staff.querySelectorAll(".abcjs-note").forEach(function (n) { n.dataset.testid = "wrong-note-note"; });
      return {
        lock: function () { btns.forEach(function (x) { x.disabled = true; }); staff.style.pointerEvents = "none"; },
        // `found`: the tapped note was the wrong one (the fix may still be wrong).
        reveal: function (correct, found) {
          paint(notes[note], css.getPropertyValue(found ? "--ok" : "--bad").trim() || (found ? "green" : "red"));
          staff.dataset.state = found ? "right" : "wrong";
          var b = btns.filter(function (x) { return x.dataset.value === fix; })[0];
          if (b && (correct || found)) { b.classList.add(correct ? "right" : "wrong"); b.dataset.state = correct ? "right" : "wrong"; }
        }
      };
    }
  };
})();
