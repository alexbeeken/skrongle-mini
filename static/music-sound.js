// Music sound (#1219): ABC text played with Web Audio oscillators. No
// samples, no network, so it works offline and on Skrongle mini. Loaded only
// on pages with something to play (a quiz item's `play`, a speaker tile in
// Sort it).
//
// Sound starts only after a tap; the first tap wakes the audio context. It
// follows the site's one audio switch (speech.js keeps it in localStorage:
// Sprint mode's speaker button, "Can't listen now" for an hour). A second
// after a tap with the audio clock standing still (an iPhone on silent, a
// blocked context) the speaker button says so.
//
// Server pages mark a speaker with <div class="music-play" data-play="ABC">
// inside a .question; this file builds it there. Sprint mode calls
// MusicSound.speaker() and MusicSound.chip() itself.
(function () {
  "use strict";
  var AUDIO = "speech:audio", UNTIL = "speech:audio-off-until"; // speech.js's switch
  var HOUR = 60 * 60 * 1000;

  function on() {
    try {
      if (localStorage.getItem(AUDIO) === "0") return false;
      var until = parseInt(localStorage.getItem(UNTIL) || "0", 10);
      return !(until && Date.now() < until);
    } catch (e) { return true; }
  }
  function turnOn() {
    if (window.LearnSpeech) return window.LearnSpeech.setAudio(true);
    try { localStorage.setItem(AUDIO, "1"); localStorage.removeItem(UNTIL); } catch (e) { /* private mode */ }
  }
  function muteHour() {
    if (window.LearnSpeech) return window.LearnSpeech.muteFor(HOUR);
    try { localStorage.setItem(UNTIL, String(Date.now() + HOUR)); } catch (e) { /* private mode */ }
  }

  var ctx = null;
  function audio() {
    try {
      if (!ctx) { var C = window.AudioContext || window.webkitAudioContext; if (!C) return null; ctx = new C(); }
      if (ctx.state !== "running") ctx.resume();
      return ctx;
    } catch (e) { return null; }
  }

  // ---- ABC to notes ----
  var STEP = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
  // Sharps (+) or flats (-) of a key, major and minor; other modes play as major.
  var KEYS = { C: 0, G: 1, D: 2, A: 3, E: 4, B: 5, "F#": 6, "C#": 7, F: -1, Bb: -2, Eb: -3, Ab: -4, Db: -5, Gb: -6, Cb: -7,
    Am: 0, Em: 1, Bm: 2, "F#m": 3, "C#m": 4, "G#m": 5, "D#m": 6, "A#m": 7, Dm: -1, Gm: -2, Cm: -3, Fm: -4, Bbm: -5, Ebm: -6, Abm: -7 };
  function signature(k) {
    var m = /^([A-G][#b]?)(m(?!aj)|min)?/i.exec(k.trim());
    var n = m ? KEYS[m[1].charAt(0).toUpperCase() + m[1].slice(1) + (m[2] ? "m" : "")] || 0 : 0;
    var sig = {};
    (n > 0 ? "FCGDAEB" : "BEADGCF").slice(0, Math.abs(n)).split("").forEach(function (l) { sig[l] = n > 0 ? 1 : -1; });
    return sig;
  }
  function fraction(s, dflt) {
    var m = /^\s*(\d+)\s*\/\s*(\d+)/.exec(s || "");
    return m ? parseInt(m[1], 10) / parseInt(m[2], 10) : dflt;
  }
  // A note length after a note: "", "2", "/", "//", "3/2", "/4".
  function length(s) {
    if (!s) return 1;
    var m = /^(\d*)(\/*)(\d*)$/.exec(s);
    if (!m) return 1;
    var num = m[1] ? parseInt(m[1], 10) : 1;
    if (!m[2]) return num;
    return num / (m[3] ? parseInt(m[3], 10) : Math.pow(2, m[2].length));
  }

  // ABC text as timed notes: { notes: [{ at, dur, midi: [..] }] (seconds), perc }.
  function parse(abc) {
    var unit = 1 / 8, beat = 1 / 4, bpm = 120, sig = {}, perc = false, body = [];
    String(abc).split(/\r?\n/).forEach(function (line) {
      line = line.replace(/%.*$/, "");
      var h = /^\s*([A-Za-z]):(.*)$/.exec(line);
      if (!h) { body.push(line); return; }
      var v = h[2].trim();
      if (h[1] === "L") unit = fraction(v, unit);
      else if (h[1] === "Q") { var q = /(?:(\d+\/\d+)\s*=\s*)?(\d+)\s*$/.exec(v); if (q) { beat = fraction(q[1], beat); bpm = parseInt(q[2], 10) || bpm; } }
      else if (h[1] === "K") { sig = signature(v); perc = /clef\s*=\s*perc/i.test(v); }
    });
    var whole = (60 / bpm) / beat; // seconds in a whole note
    var text = body.join(" ").replace(/"[^"]*"/g, "").replace(/![^!]*!/g, "").replace(/\+[^+]*\+/g, "");
    var re = /\[([^\]|]*)\](\d*\/*\d*)|([_^=]*)([A-Ga-gzx])([,']*)(\d*\/*\d*)|(\|)/g, tok, bar = {}, at = 0, notes = [];
    function pitch(acc, letter, oct) {
      var up = letter.toUpperCase(), m = 60 + STEP[up] + (letter === up ? 0 : 12);
      oct.split("").forEach(function (c) { m += c === "'" ? 12 : -12; });
      var key = up + m;
      if (acc) bar[key] = acc.charAt(0) === "=" ? 0 : (acc.charAt(0) === "^" ? 1 : -1) * acc.length;
      return m + (key in bar ? bar[key] : (sig[up] || 0));
    }
    while ((tok = re.exec(text))) {
      if (tok[7]) { bar = {}; continue; }
      var midi = [], len;
      if (tok[1] !== undefined) {
        var inner = /([_^=]*)([A-Ga-g])([,']*)(\d*\/*\d*)/g, n, first = null;
        while ((n = inner.exec(tok[1]))) { midi.push(pitch(n[1], n[2], n[3])); if (first === null) first = n[4]; }
        len = tok[2] ? length(tok[2]) : length(first);
      } else {
        if (tok[4] !== "z" && tok[4] !== "x") midi.push(pitch(tok[3], tok[4], tok[5]));
        len = length(tok[6]);
      }
      var dur = len * unit * whole;
      if (midi.length) notes.push({ at: at, dur: dur, midi: midi });
      at += dur;
    }
    // Two single notes (an interval): one after the other, then together.
    if (notes.length === 2 && notes[0].midi.length === 1 && notes[1].midi.length === 1) {
      notes.push({ at: at + 0.15, dur: Math.max(notes[0].dur, notes[1].dur) * 1.5, midi: [notes[0].midi[0], notes[1].midi[0]] });
    }
    return { notes: notes, perc: perc };
  }

  function tone(c, midi, at, dur, perc, voices) {
    var o = c.createOscillator(), g = c.createGain(), peak = (perc ? 0.2 : 0.25) / Math.sqrt(voices);
    o.type = perc ? "square" : "triangle";
    o.frequency.value = perc ? 1000 : 440 * Math.pow(2, (midi - 69) / 12);
    var end = at + (perc ? 0.05 : Math.max(0.1, dur * 0.95));
    g.gain.setValueAtTime(0.0001, at);
    g.gain.exponentialRampToValueAtTime(peak, at + 0.015);
    g.gain.exponentialRampToValueAtTime(0.0001, end);
    o.connect(g); g.connect(c.destination);
    o.start(at); o.stop(end + 0.05);
  }

  // Play `abc` now. Returns { tones, secs, start } or null with no Web Audio.
  function play(abc) {
    var c = audio();
    if (!c) return null;
    var tune = parse(abc), start = c.currentTime + 0.05, tones = 0, secs = 0;
    tune.notes.forEach(function (n) {
      n.midi.forEach(function (m) { tone(c, m, start + n.at, n.dur, tune.perc, n.midi.length); tones += 1; });
      secs = Math.max(secs, n.at + n.dur);
    });
    return { tones: tones, secs: secs, start: c.currentTime };
  }

  // ---- the speaker button ----
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text) e.textContent = text;
    return e;
  }
  // State for tests and screen readers: data-state is idle, playing, silent
  // (no sound after a second) or off (the audio switch); data-ctx is the
  // audio context's state and data-tones the tones of the last play.
  function button(abc, cls, onPlayed) {
    var b = el("button", cls, "🔊"), timer = null;
    b.type = "button";
    function set(state) {
      b.dataset.state = state;
      var label = state === "off" ? t("common.js.music_sound_off") : state === "silent" ? t("common.js.music_no_sound") : t("common.js.music_play");
      b.title = label; b.setAttribute("aria-label", label);
      b.textContent = state === "off" ? "🔇" : "🔊";
    }
    set(on() ? "idle" : "off");
    b.addEventListener("click", function (ev) {
      ev.stopPropagation();
      if (!on()) turnOn();
      var r = play(abc);
      b.dataset.ctx = ctx ? ctx.state : "none";
      if (!r) { set("silent"); if (onPlayed) onPlayed(false); return; }
      b.dataset.tones = r.tones;
      set("playing");
      clearTimeout(timer);
      var clock = r.start;
      timer = setTimeout(function () {
        b.dataset.ctx = ctx.state;
        if (ctx.state !== "running" || ctx.currentTime <= clock) { set("silent"); if (onPlayed) onPlayed(false); return; }
        if (onPlayed) onPlayed(true);
        timer = setTimeout(function () { if (b.dataset.state === "playing") set("idle"); }, Math.max(0, r.secs * 1000 - 950));
      }, 1000);
    });
    b.paint = function () { set(on() ? "idle" : "off"); };
    return b;
  }

  // A speaker for `abc` in `box`: the button, a line that shows when no
  // sound comes, and "Can't listen now" (sound off for an hour, then
  // opts.cant()). opts.watch: an element whose data-state turns right or
  // wrong once the question is answered; with opts.staffAfter the played
  // notes are then drawn on a staff (notation.js).
  function speaker(box, abc, opts) {
    opts = opts || {};
    box.classList.add("music-play");
    box.dataset.play = abc;
    var note = el("p", "music-no-sound", t("common.js.music_no_sound"));
    note.dataset.testid = "music-no-sound"; note.hidden = true; note.setAttribute("role", "status");
    var b = button(abc, "music-speaker", function (ok) { note.hidden = ok; });
    b.dataset.testid = "music-play";
    var cant = el("button", "music-cant", t("common.js.music_cant_listen"));
    cant.type = "button"; cant.dataset.testid = "music-cant-listen";
    cant.addEventListener("click", function () { muteHour(); b.paint(); note.hidden = true; if (opts.cant) opts.cant(); });
    box.appendChild(b); box.appendChild(note); box.appendChild(cant);
    var drawn = false;
    function staff() {
      if (drawn || !window.drawStaff) return;
      drawn = true;
      var wrap = el("div", "music-staff"); wrap.dataset.testid = "music-staff";
      box.parentNode.insertBefore(wrap, box);
      window.drawStaff(wrap.appendChild(el("div")), abc);
    }
    if (opts.watch && opts.staffAfter) {
      var seen = new MutationObserver(function () {
        if (!box.isConnected) return seen.disconnect();
        var s = opts.watch.dataset.state;
        if (s === "right" || s === "wrong") { seen.disconnect(); staff(); }
      });
      seen.observe(opts.watch, { attributes: true, attributeFilter: ["data-state"] });
    }
    return { button: b, staff: staff };
  }

  // A small replay button (a placed speaker tile in Sort it).
  function chip(abc) {
    var b = button(abc, "music-chip");
    b.dataset.testid = "music-chip";
    return b;
  }

  // Server pages: every marked speaker inside a question. With sound off
  // (or after "Can't listen now") the tune is shown as a staff to read
  // instead, unless the options are the staves to tell apart.
  function init() {
    document.querySelectorAll(".music-play[data-play]").forEach(function (box) {
      var q = box.closest(".question"), staffAfter = !!box.dataset.staffAfter;
      var reveal = staffAfter && !box.dataset.abcOptions;
      var s = speaker(box, box.dataset.play, { watch: q && q.querySelector(".feedback"), staffAfter: staffAfter, cant: reveal ? function () { s.staff(); } : null });
      if (!on() && reveal) whenVisible(box, s.staff);
    });
  }
  // abcjs measures as it draws, so a staff waits for its question to show.
  function whenVisible(node, f) {
    if (node.getClientRects().length || !window.IntersectionObserver) return f();
    var io = new IntersectionObserver(function () { if (node.getClientRects().length) { io.disconnect(); f(); } });
    io.observe(node.closest(".question") || node);
  }

  // Here, not in style.css: only these pages need it.
  var css = document.createElement("style");
  css.textContent = ".music-play{display:flex;flex-direction:column;align-items:center;gap:.4rem;margin:.25rem 0 1rem}" +
    ".music-speaker:not(.sprint-speaker){width:4.5rem;height:4.5rem;border-radius:50%;border:2px solid var(--line);background:var(--surface,#fff);font-size:2rem;cursor:pointer}" +
    ".music-speaker[data-state=playing]{outline:3px solid var(--accent);outline-offset:2px}" +
    ".music-no-sound{margin:0;font-size:.9rem;color:var(--muted)}" +
    ".music-cant{background:none;border:0;padding:.2rem;color:var(--muted);text-decoration:underline;font:inherit;font-size:.9rem;cursor:pointer}" +
    ".music-chip{border:1px solid var(--line);border-radius:999px;background:transparent;padding:.1rem .5rem;cursor:pointer}" +
    ".music-staff{margin:0 auto}";
  document.head.appendChild(css);

  window.MusicSound = { on: on, parse: parse, play: play, speaker: speaker, chip: chip };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
