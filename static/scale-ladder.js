// Build the scale (#1229): the staff above Sprint mode's ladder of notes.
// sprint.js runs the ladder (rungs, bank, Check); this draws the notes on
// the rungs so far as one staff (an empty rung is an invisible rest, so a
// note keeps its place) and plays each note as it lands, through
// music-sound.js, when sound is on. Notes are ABC, in C, so every sharp
// and flat is written on its note (src/sprint/scale.rs `tune`).
(function () {
  "use strict";
  function tune(notes) { return "X:1\nL:1/4\nK:C\n" + notes + "|"; }

  window.ScaleLadder = {
    // `box` is on the stage already; returns { draw(notes), play(note) }.
    mount: function (box) {
      box.className = "sprint-staff scale-staff";
      box.dataset.testid = "scale-staff";
      return {
        // `notes`: one ABC note per rung, "" for an empty one.
        draw: function (notes) {
          box.dataset.notes = notes.filter(Boolean).length;
          if (!window.drawStaff) return;
          box.textContent = "";
          window.drawStaff(box.appendChild(document.createElement("div")), tune(notes.map(function (n) { return n || "x"; }).join(" ")));
        },
        play: function (note) {
          var M = window.MusicSound;
          if (!note || !M || !M.on()) return;
          var r = M.play(tune(note));
          box.dataset.played = note;
          box.dataset.tones = r ? r.tones : 0;
        }
      };
    }
  };
})();
