// Piano keyboard (#1231). One or two octaves of keys from middle C, built
// from buttons. Each key sounds when tapped (music-sound.js, when the site's
// audio switch is on), and the last key tapped is the answer: an ABC note
// spelled with sharps (C, ^C, c), which the server grades by pitch
// (src/notation/keyboard.rs), so every spelling of a pitch is the same key.
// PianoKeys.mount() is the component; Read at tempo (#1234) uses it too.
//
// In Practice, the reading check and the Guide the page has an empty
// .keyboard box and a hidden radio (.keyboard-answer) that holds the key
// tapped, so quiz.js, lesson-check.js and guide.js post it like a picked
// option. Once graded, the feedback's data-key names the right key and this
// file lights it up. Sprint mode calls PianoKeys.mount itself.
(function () {
  "use strict";
  var SRC = (document.currentScript && document.currentScript.src) || "";
  if (SRC) {
    var css = document.createElement("link");
    css.rel = "stylesheet";
    css.href = SRC.replace("keyboard.js", "keyboard.css");
    document.head.appendChild(css);
  }

  var MIDDLE_C = 60;
  var SHARPS = ["C", "^C", "D", "^D", "E", "F", "^F", "G", "^G", "A", "^A", "B"];
  var STEP = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
  var ACC = { "": 0, "=": 0, "^": 1, "^^": 2, "_": -1, "__": -2 };

  // A MIDI number as an ABC note with sharps: 60 C, 61 ^C, 72 c, 84 c', 48 C,.
  function abcOf(m) {
    var pc = ((m % 12) + 12) % 12, oct = Math.floor((m - MIDDLE_C) / 12), n = SHARPS[pc];
    if (oct >= 1) { n = n.replace(/[A-G]/, function (c) { return c.toLowerCase(); }); oct -= 1; while (oct-- > 0) n += "'"; }
    else while (oct++ < 0) n += ",";
    return n;
  }
  // An ABC note as a MIDI number; null when it is not one note.
  function midiOf(abc) {
    var m = /^(\^\^|\^|__|_|=)?([A-Ga-g])([,']*)$/.exec(String(abc || "").trim());
    if (!m) return null;
    var n = MIDDLE_C + STEP[m[2].toUpperCase()] + ACC[m[1] || ""] + (m[2] === m[2].toLowerCase() ? 12 : 0);
    m[3].split("").forEach(function (c) { n += c === "'" ? 12 : -12; });
    return n;
  }
  // Sound one key, following the site's audio switch. Did it play?
  function sound(m) {
    var M = window.MusicSound;
    return !!(M && M.on() && M.play("X:1\nL:1/4\nK:C\n" + abcOf(m) + "2"));
  }

  function el(tag, cls) { var e = document.createElement(tag); if (cls) e.className = cls; return e; }

  // Build the keys in `root`. opts: octaves (1 or 2), low (the lowest key,
  // MIDI, a C; middle C by default), onKey(abc, midi) after each tap,
  // silent (no sound). Returns { keys, lock(), reveal(key, exact), clear() }.
  function mount(root, opts) {
    opts = opts || {};
    var octaves = opts.octaves === 2 ? 2 : 1, low = opts.low || MIDDLE_C, locked = false, chosen = null;
    root.innerHTML = "";
    root.classList.add("keyboard");
    if (!root.dataset.testid) root.dataset.testid = "keyboard";
    root.setAttribute("role", "group");
    root.setAttribute("aria-label", t("common.js.keyboard_label"));
    root.dataset.octaves = octaves;
    var keys = [], white = 0, black = 0;
    // One block per octave: on a narrow screen two octaves wrap, the higher one on top.
    for (var o = 0; o < octaves; o++) {
      var block = root.appendChild(el("div", "kb-octave")), whites = 0;
      for (var i = 0; i < 12; i++) {
        var m = low + 12 * o + i, isBlack = SHARPS[i].charAt(0) === "^";
        var b = block.appendChild(el("button", "kb-key " + (isBlack ? "kb-black" : "kb-white")));
        b.type = "button";
        b.dataset.testid = "keyboard-key";
        b.dataset.note = abcOf(m);
        b.dataset.midi = m;
        b.setAttribute("aria-pressed", "false");
        b.setAttribute("aria-label", isBlack ? t("common.js.keyboard_black", { n: ++black }) : t("common.js.keyboard_white", { n: ++white }));
        // A black key sits on the line between two white keys, 7 white keys to the octave.
        if (isBlack) b.style.left = "calc(" + (whites * 100 / 7) + "% - var(--kb-black) / 2)";
        else whites++;
        b.addEventListener("click", tap.bind(null, b, m));
        keys.push(b);
      }
    }
    function tap(b, m) {
      // data-sounded: the last key that sounded (a test can't hear).
      if (!opts.silent && sound(m)) root.dataset.sounded = b.dataset.note;
      if (locked) return;
      if (chosen) { chosen.classList.remove("kb-on"); chosen.setAttribute("aria-pressed", "false"); }
      chosen = b;
      b.classList.add("kb-on");
      b.setAttribute("aria-pressed", "true");
      root.dataset.chosen = b.dataset.note;
      if (opts.onKey) opts.onKey(b.dataset.note, m);
    }
    return {
      keys: keys,
      // The answer is in: keys still sound, but no longer change it.
      lock: function () { locked = true; root.dataset.locked = "true"; },
      // Light up the right key (`key`, an ABC note; every key of its pitch
      // class unless `exact`), and the key tapped when it is wrong.
      reveal: function (key, exact) {
        var want = midiOf(key);
        if (want === null) return;
        keys.forEach(function (k) {
          var m = Number(k.dataset.midi), right = exact ? m === want : (m - want) % 12 === 0;
          if (right) k.dataset.state = "right";
          else if (k === chosen) k.dataset.state = "wrong";
        });
      },
      clear: function () {
        locked = false; chosen = null; delete root.dataset.chosen; delete root.dataset.locked;
        keys.forEach(function (k) { k.classList.remove("kb-on"); k.setAttribute("aria-pressed", "false"); delete k.dataset.state; });
      }
    };
  }

  window.PianoKeys = { mount: mount, abcOf: abcOf, midiOf: midiOf };

  // Practice, the reading check and the Guide.
  function auto() {
    document.querySelectorAll(".question .keyboard[data-octaves]").forEach(function (root) {
      var q = root.closest(".question");
      var radio = q.querySelector("input.keyboard-answer");
      var fb = q.querySelector(".feedback");
      var exact = !!root.dataset.exact;
      var w = mount(root, {
        octaves: Number(root.dataset.octaves),
        onKey: function (abc) {
          if (!radio) return;
          radio.value = abc;
          radio.checked = true;
          radio.dispatchEvent(new Event("change", { bubbles: true }));
        }
      });
      if (fb) new MutationObserver(function () {
        if (fb.dataset.state !== "right" && fb.dataset.state !== "wrong") return;
        w.lock();
        w.reveal(fb.dataset.key, exact);
      }).observe(fb, { attributes: true, attributeFilter: ["data-state"] });
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", auto);
  else auto();
})();
