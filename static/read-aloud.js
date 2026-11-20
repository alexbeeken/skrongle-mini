// Read aloud (#234): Listen to a whole lesson, or tap the speaker beside a
// question to hear it and its choices, in any course. The browser's own
// voices do the speaking (speech.js, window.LearnSpeech); nothing is hosted.
//
// Loaded by templates/read_aloud.html, whose <script> tag carries the course's
// languages: data-lang is the explaining language (`meanings.lang`),
// data-term-lang the course's own language in a language course (empty
// elsewhere, and then everything is read in the explaining language).
//
// Rules (docs/features.md "Read aloud"): no buttons while the audio switch is
// off or the browser has no voice for a language it would need; one sentence
// per utterance, the next queued from the last one's end; the sentence being
// read is wrapped in a span only while it is read; speech stops when the page
// goes or the learner answers. Music notation is skipped, tables are read row
// by row, and a question with something to listen to (`play`) or a listening
// exercise never gets a speaker: it would give the answer away.
window.ReadAloud = (function () {
  "use strict";
  var S = window.LearnSpeech;
  var me = document.currentScript;
  var ML = (me && me.dataset.lang || "").trim();
  var TL = (me && me.dataset.termLang || "").trim();
  if (TL === ML) TL = "";
  var none = { question: function () {}, sprint: function () {}, sentences: function () { return []; } };
  if (!S || !S.supported || !ML) return none;

  // Can this page speak now? Audio on, and a voice for every language it
  // would read in (both, in a language course). Voices arrive late in
  // Chrome, so this is asked again whenever the list changes.
  function ready() {
    return S.audioOn() && S.hasVoice(ML) && (!TL || S.hasVoice(TL));
  }
  // The language a piece of text is read in: the course's own language when
  // it is marked so in a language course, else the explaining language.
  function langOf(lang) { return TL && lang && lang.toLowerCase() === TL.toLowerCase() ? TL : ML; }

  // ---- speed (one choice per device, kept beside the voice) -------------
  function rate() { return S.rate ? S.rate() : 0.95; }

  // ---- sentences -------------------------------------------------------
  var seg = null;
  try { if (window.Intl && Intl.Segmenter) seg = new Intl.Segmenter(ML, { granularity: "sentence" }); } catch (e) { seg = null; }
  // [start, end) offsets of each sentence in `text`.
  function sentences(text) {
    var out = [];
    if (seg) {
      var it = seg.segment(text)[Symbol.iterator](), r;
      while (!(r = it.next()).done) out.push([r.value.index, r.value.index + r.value.segment.length]);
    } else {
      var re = /[^.!?…。！？]+(?:[.!?…。！？]+["'”’»)\]]*\s*|$)/g, m;
      while ((m = re.exec(text)) && m[0]) out.push([m.index, m.index + m[0].length]);
    }
    return out.filter(function (s) { return /\S/.test(text.slice(s[0], s[1])); });
  }
  var WORDY;
  try { WORDY = new RegExp("[\\p{L}\\p{N}]", "u"); } catch (e) { WORDY = /[A-Za-z0-9À-￿]/; }

  // ---- one queue of utterances ------------------------------------------
  // A run is {text, lang}. Each is said with LearnSpeech.say, the next one
  // from the end of the last. `token` makes a stopped queue's late callbacks
  // harmless (a cancelled utterance still calls onend).
  var token = 0, onStop = null;
  function stop() {
    token += 1;
    S.hush();
    var f = onStop; onStop = null;
    if (f) f();
  }
  function speakRuns(runs, done) {
    var my = token, i = 0;
    function step() {
      if (my !== token) return;
      while (i < runs.length && !WORDY.test(runs[i].text)) i += 1;
      if (i >= runs.length) { done(); return; }
      var r = runs[i++];
      var ok = S.say(r.text, r.lang, { rate: rate(), onend: function () { if (my === token) setTimeout(step, 0); } });
      if (!ok) stop();
    }
    step();
  }
  window.addEventListener("pagehide", stop);

  // ---- reading a block of the page ----------------------------------------
  var SKIP = "pre, svg, button, select, input, textarea, .abc-staff, .music-play, .katex-mathml, [aria-hidden=true], [data-read-skip]";
  // The text nodes of a block in reading order, with their offsets in the
  // block's text. A table row gets ", " between its cells (not in the page).
  function pieces(block) {
    var out = [], pos = 0, text = "";
    function add(t, node) { out.push({ node: node, start: pos, end: pos + t.length }); pos += t.length; text += t; }
    function walk(n) {
      if (n.nodeType === 3) { if (n.nodeValue) add(n.nodeValue, n); return; }
      if (n.nodeType !== 1 || n.matches(SKIP)) return;
      if (n.tagName === "BR") { add(" ", null); return; }
      var cell = n.tagName === "TD" || n.tagName === "TH";
      if (cell && pos > 0) add(", ", null);
      for (var c = n.firstChild; c; c = c.nextSibling) walk(c);
    }
    walk(block);
    return { list: out, text: text };
  }
  // The language of a text node: the nearest `lang` up to the block's page
  // area; in a lesson's text (whose lang is the course's language, but whose
  // words mostly explain) only the words say.js marks as the course's
  // (italics, table cells, known bold words) count as its language.
  function nodeLang(node) {
    for (var el = node.parentElement; el && el !== document.body; el = el.parentElement) {
      if (el.classList.contains("prose")) return ML;
      if (el.classList.contains("can-say") && el.closest(".prose")) return TL || ML;
      if (el.classList.contains("prompt") || el.hasAttribute("data-read-prompt")) return ML;
      var l = el.getAttribute("lang");
      if (l) return langOf(l);
    }
    return ML;
  }
  // Runs of one language for [s, e) of a block's text.
  function runsOf(p, s, e) {
    var runs = [], gap = "";
    p.list.forEach(function (x) {
      var a = Math.max(s, x.start), b = Math.min(e, x.end);
      if (a >= b) return;
      var t = p.text.slice(a, b);
      // A gap between cells is a comma inside one language, a new utterance between two.
      if (!x.node) { gap += t; return; }
      var lang = nodeLang(x.node), last = runs[runs.length - 1];
      if (last && last.lang === lang) last.text += gap + t;
      else runs.push({ text: t, lang: lang });
      gap = "";
    });
    runs.forEach(function (r) { r.text = r.text.replace(/\s+/g, " ").trim(); });
    return runs;
  }
  // A question's prompt: in a language course, words in quotes are the
  // course's ("What does 'rojo' mean?"), the rest explains.
  var QUOTED = /(^|[\s(¿¡])(['"“‘«])([^'"”’»]+)(['"”’»])(?=[\s.,;:!?)]|$)/g;
  function promptRuns(text) {
    text = text.replace(/\s+/g, " ").trim();
    if (!TL) return [{ text: text, lang: ML }];
    var runs = [], last = 0, m;
    QUOTED.lastIndex = 0;
    while ((m = QUOTED.exec(text))) {
      var q = m.index + m[1].length;
      if (q > last) runs.push({ text: text.slice(last, q), lang: ML });
      runs.push({ text: m[3], lang: TL });
      last = q + m[2].length + m[3].length + m[4].length;
    }
    if (last < text.length) runs.push({ text: text.slice(last), lang: ML });
    return runs;
  }

  // Mark [s, e) of a block: each text node slice in it wrapped in a span.
  // unmark() puts the text back as it was (say.js watches the page, and
  // adds nothing for a plain span or text node).
  function mark(block, s, e) {
    var spans = [];
    pieces(block).list.forEach(function (x) {
      if (!x.node) return;
      var a = Math.max(s, x.start), b = Math.min(e, x.end);
      if (a >= b || !/\S/.test(x.node.nodeValue.slice(a - x.start, b - x.start))) return;
      var n = x.node;
      if (b - x.start < n.nodeValue.length) n.splitText(b - x.start);
      if (a > x.start) n = n.splitText(a - x.start);
      var span = document.createElement("span");
      span.className = "ra-mark";
      span.dataset.testid = "read-aloud-mark";
      n.parentNode.replaceChild(span, n);
      span.appendChild(n);
      spans.push(span);
    });
    if (spans[0] && spans[0].scrollIntoView) {
      var r = spans[0].getBoundingClientRect();
      if (r.top < 60 || r.bottom > window.innerHeight - 40) spans[0].scrollIntoView({ block: "center", behavior: "smooth" });
    }
    return function unmark() {
      spans.forEach(function (span) {
        var parent = span.parentNode;
        if (!parent) return;
        while (span.firstChild) parent.insertBefore(span.firstChild, span);
        parent.removeChild(span);
        parent.normalize();
      });
    };
  }

  // ---- Listen: the whole lesson ----------------------------------------
  // The blocks read, in page order: the title and summary, the lesson's
  // text, its vocabulary and paradigm tables row by row, its conversations.
  // Not the menus, the reading check, the note or anything drawn as music.
  var BLOCKS = "h1, h2, h3, h4, h5, h6, p, li, blockquote, tr, dt, dd, figcaption, .conv-text";
  var NOT_READ = ".lesson-actions, .lesson-topline, .lesson-check, .lesson-note, .lesson-footer, .lesson-pager, .lesson-scales, dialog, nav, form, pre, .say-hint, p.small, [hidden], [data-read-skip]";
  function lessonBlocks(root) {
    var all = Array.prototype.slice.call(root.querySelectorAll(BLOCKS));
    return all.filter(function (b) {
      if (b.closest(NOT_READ)) return false;
      if (b.querySelector(BLOCKS)) return false; // only the innermost: a list item's own paragraphs
      return WORDY.test(pieces(b).text);
    });
  }

  function lessonPlayer(box) {
    var root = box.closest("[data-testid=lesson]") || document.querySelector("[data-testid=lesson]");
    if (!root) return;
    var btn = box.querySelector("[data-read-aloud-toggle]");
    var speed = box.querySelector("[data-read-aloud-speed]");
    var blocks = [], bi = 0, si = 0, state = "idle"; // idle | playing | paused
    var unmark = null;

    function label() {
      var key = state === "playing" ? "common.js.read_aloud_pause" : state === "paused" ? "common.js.read_aloud_resume" : "common.js.read_aloud_listen";
      btn.textContent = t(key);
      btn.setAttribute("aria-pressed", state === "playing" ? "true" : "false");
      btn.dataset.state = state;
      root.classList.toggle("ra-active", state !== "idle");
    }
    function clearMark() { if (unmark) { unmark(); unmark = null; } }
    function halt(next) {
      onStop = null;
      stop();
      clearMark();
      state = next;
      label();
    }
    function play() {
      if (!ready()) { halt("idle"); return; }
      state = "playing"; label();
      onStop = function () { clearMark(); if (state === "playing") { state = "paused"; label(); } };
      step();
    }
    function step() {
      if (state !== "playing") return;
      while (bi < blocks.length) {
        var p = pieces(blocks[bi]);
        // A table row is one piece; other blocks one sentence at a time.
        var ss = blocks[bi].tagName === "TR" ? [[0, p.text.length]] : sentences(p.text);
        if (si < ss.length) {
          var s = ss[si];
          clearMark();
          unmark = mark(blocks[bi], s[0], s[1]);
          speakRuns(runsOf(pieces(blocks[bi]), s[0], s[1]), function () {
            clearMark();
            si += 1;
            step();
          });
          return;
        }
        bi += 1; si = 0;
      }
      // The end: back to the start, ready to listen again.
      bi = 0; si = 0;
      onStop = null;
      clearMark();
      state = "idle"; label();
    }

    btn.addEventListener("click", function () {
      if (state === "playing") { halt("paused"); return; }
      blocks = lessonBlocks(root);
      if (bi >= blocks.length) { bi = 0; si = 0; }
      play();
    });
    if (speed) {
      speed.value = rate() < 0.9 ? "slow" : "normal";
      speed.addEventListener("change", function () { if (S.setRate) S.setRate(speed.value === "slow" ? 0.75 : 0.95); });
    }
    // While listening (or paused), a tap on a paragraph starts reading there.
    // Not a tap meant for a link, a control, a word to hear or a selection.
    root.addEventListener("click", function (ev) {
      if (state === "idle") return;
      if (ev.target.closest("a, button, input, select, textarea, label, summary, .can-say, [data-read-aloud]")) return;
      var sel = window.getSelection && window.getSelection();
      if (sel && String(sel).length) return;
      blocks = lessonBlocks(root);
      for (var i = 0; i < blocks.length; i++) {
        if (blocks[i].contains(ev.target)) {
          halt("paused");
          bi = i; si = 0;
          play();
          return;
        }
      }
    });
    return {
      show: function (on) { box.hidden = !on; if (!on && state !== "idle") halt("idle"); }
    };
  }

  // ---- the speaker beside a question --------------------------------------
  // `q` is the question's element, `prompt` its prompt, `options` the
  // elements of its choices. The speaker goes right after the prompt, not in
  // it, so the prompt's own text stays as it was.
  function saysFor(prompt) {
    var n = prompt.nextElementSibling;
    return n && n.classList.contains("ra-say") ? n : null;
  }
  function speaker(q, prompt, options, opts) {
    opts = opts || {};
    if (!prompt || saysFor(prompt)) return null;
    var b = document.createElement("button");
    b.type = "button";
    b.className = "ra-say" + (opts.cls ? " " + opts.cls : "");
    b.dataset.testid = "read-aloud-question";
    b.setAttribute("aria-label", t('common.js.read_aloud_question'));
    b.title = t('common.js.read_aloud_question');
    b.textContent = "🔊";
    prompt.insertAdjacentElement("afterend", b);
    function clear() { b.classList.remove("saying"); }
    b.addEventListener("click", function (ev) {
      ev.preventDefault(); ev.stopPropagation();
      if (b.classList.contains("saying")) { stop(); return; }
      stop();
      if (!ready()) return;
      var p = pieces(prompt);
      var runs = opts.promptLang ? runsOf(p, 0, p.text.length) : promptRuns(p.text);
      options.forEach(function (o) {
        if (o.querySelector("pre, svg, .abc-staff")) return; // music notation is not read
        var op = pieces(o);
        runs = runs.concat(runsOf(op, 0, op.text.length));
      });
      b.classList.add("saying");
      onStop = clear;
      speakRuns(runs, function () { onStop = null; clear(); });
    });
    // Answering stops the speech: a check, a choice, a tile, a typed key.
    function answered(ev) { if (b.isConnected && b.classList.contains("saying") && ev.target !== b) stop(); }
    q.addEventListener("submit", answered, true);
    q.addEventListener("change", answered, true);
    q.addEventListener("click", function (ev) { if (ev.target.closest("button, input, label, .sprint-opt")) answered(ev); }, true);
    q.addEventListener("keydown", answered, true);
    return b;
  }

  // Study quizzes, the lesson's reading check and the Guide: every
  // `.question` with a prompt, unless it has music to listen to.
  function studyQuestions() {
    var out = [];
    document.querySelectorAll(".question").forEach(function (q) {
      var prompt = q.querySelector(".prompt");
      if (!prompt || q.querySelector(".music-play, [data-play]")) return;
      out.push({ q: q, prompt: prompt, options: Array.prototype.slice.call(q.querySelectorAll(".options .option")) });
    });
    return out;
  }

  // Sprint mode: called by sprint.js after each exercise is drawn. A
  // listening exercise and one with music to hear get no speaker, nor one
  // whose prompt already has its own (the sentence in the course's language).
  function sprint(stage, e) {
    if (!ready() || !e || /^hear-/.test(e.kind) || e.kind === "bite" || /"play":"[^"]/.test(JSON.stringify(e))) return;
    var prompt = stage.querySelector(".sprint-prompt");
    if (!prompt || prompt.querySelector(".sprint-say")) return;
    var options = Array.prototype.slice.call(stage.querySelectorAll(".sprint-opt"));
    options = options.map(function (o) { return o.querySelector("[lang]") || o; });
    speaker(stage, prompt, options, { cls: "sprint-say", promptLang: e.kind !== "choice" });
  }

  // ---- wiring ------------------------------------------------------------
  var player = null;
  var box = document.querySelector("[data-read-aloud]");
  if (box) player = lessonPlayer(box);
  var quizzes = studyQuestions();
  function refresh() {
    var on = ready();
    if (player) player.show(on);
    quizzes.forEach(function (x) {
      var b = saysFor(x.prompt);
      if (on && !b) speaker(x.q, x.prompt, x.options);
      if (b) b.hidden = !on;
      if (!on && b && b.classList.contains("saying")) stop();
    });
    document.querySelectorAll(".sprint-prompt + .ra-say").forEach(function (b) { b.hidden = !on; });
  }
  S.onVoices(refresh);
  window.addEventListener("learnspeech:audio", refresh);

  return { question: speaker, sprint: sprint, sentences: sentences, stop: stop };
})();
