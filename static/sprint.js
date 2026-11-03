// Sprint mode: the phone-first path (home) and the session player.
//
// Home: tapping a stop on the path opens its sheet (start / read).
// Player: exercises come as JSON from the page. Quiz questions (choice, tiles)
// are graded by POST /api/answer like everywhere else; Sprint mode exercises
// (sort, spot, build, ladder, scale, pick, match) and the listening ones (hear-build,
// hear-gap, hear-pick, hear-which, hear-match) by POST /api/sprint/check.
// Finishing posts to /api/sprint/finish, which completes the Guide stage for
// lesson runs. Speech is the browser's own, through speech.js: the speaker
// button in the top bar switches it off (listening exercises are then
// skipped, as with "Can't listen now"), and words in the course's language
// can be tapped to hear them.
(function () {
  "use strict";
  var base = document.body.dataset.base || "";
  var Sp = window.LearnSpeech || null; // static/speech.js

  // Remember the phone's time zone so streaks and the daily ring use local days.
  try {
    document.cookie = "tz=" + (-new Date().getTimezoneOffset()) + "; Path=" + (base || "/") + "; Max-Age=31536000; SameSite=Lax" +
      (location.protocol === "https:" ? "; Secure" : "");
  } catch (e) { /* cookies off */ }

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function el(tag, cls, html) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html !== undefined) e.innerHTML = html;
    return e;
  }
  // State is said in attributes as well as classes, so tests and screen
  // readers see it: aria-pressed on the tile or option picked, data-state on
  // one used up (used, done) or graded (right, wrong).
  function pick(group, b) {
    group.forEach(function (x) { x.classList.remove("sel"); x.setAttribute("aria-pressed", "false"); });
    if (b) { b.classList.add("sel"); b.setAttribute("aria-pressed", "true"); }
  }
  function mark(x, state) { x.classList.add(state); x.dataset.state = state; }
  function post(path, body) {
    return fetch(base + path, {
      method: "POST", credentials: "same-origin",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (r.status === 401) { window.location.href = base + "/login"; throw new Error("login"); }
      if (r.status === 410) { window.location.href = base + "/courses"; throw new Error("course"); }
      if (!r.ok) throw new Error("HTTP " + r.status);
      return r.json();
    });
  }
  function shuffle(a) {
    for (var i = a.length - 1; i > 0; i--) { var j = Math.floor(Math.random() * (i + 1)); var t = a[i]; a[i] = a[j]; a[j] = t; }
    return a;
  }
  function buzz(pattern) {
    try { if (navigator.vibrate) navigator.vibrate(pattern); } catch (e) { /* ignore */ }
  }

  // ---------- sound effects: a few WebAudio blips, off by default ----------
  var Sound = {
    on: false, ctx: null,
    init: function () {
      try { this.on = localStorage.getItem("sprint-sound") === "1"; } catch (e) { /* ignore */ }
      var btn = document.getElementById("sound");
      if (!btn) return;
      var self = this;
      var paint = function () { btn.textContent = self.on ? "🎶" : "🎵"; btn.classList.toggle("on", self.on); btn.title = self.on ? t('sprint.js.sound_on') : t('sprint.js.sound_off'); };
      paint();
      btn.addEventListener("click", function () {
        self.on = !self.on;
        try { localStorage.setItem("sprint-sound", self.on ? "1" : "0"); } catch (e) { /* ignore */ }
        paint();
        if (self.on) self.play("tap");
      });
    },
    tone: function (freq, start, dur, type, gain) {
      var c = this.ctx, o = c.createOscillator(), g = c.createGain();
      o.type = type || "sine"; o.frequency.value = freq;
      g.gain.setValueAtTime(0.0001, c.currentTime + start);
      g.gain.exponentialRampToValueAtTime(gain || 0.18, c.currentTime + start + 0.01);
      g.gain.exponentialRampToValueAtTime(0.0001, c.currentTime + start + dur);
      o.connect(g); g.connect(c.destination);
      o.start(c.currentTime + start); o.stop(c.currentTime + start + dur + 0.02);
    },
    play: function (kind) {
      if (!this.on) return;
      try {
        if (!this.ctx) this.ctx = new (window.AudioContext || window.webkitAudioContext)();
        if (this.ctx.state === "suspended") this.ctx.resume();
        if (kind === "ok") { this.tone(660, 0, 0.09); this.tone(990, 0.09, 0.14); }
        else if (kind === "bad") { this.tone(220, 0, 0.18, "square", 0.08); }
        else if (kind === "tap") { this.tone(520, 0, 0.04, "triangle", 0.08); }
        else if (kind === "win") { [523, 659, 784, 1047].forEach(function (f, i) { Sound.tone(f, i * 0.11, 0.2); }); }
        else if (kind === "tick") { this.tone(880, 0, 0.03, "square", 0.05); }
      } catch (e) { /* no audio */ }
    }
  };
  Sound.init();

  // ---------- audio: the voice that reads words and listening exercises ----------
  // One switch for the whole site (speech.js keeps it): off, and tapped
  // words stay silent and listening exercises are skipped.
  var Audio = {
    btn: document.getElementById("audio"),
    on: function () { return !!(Sp && Sp.supported && Sp.audioOn()); },
    paint: function () {
      if (!this.btn) return;
      var on = this.on();
      this.btn.textContent = on ? "🔊" : "🔇";
      this.btn.classList.toggle("on", on);
      this.btn.title = !Sp || !Sp.supported ? t('sprint.js.no_speech') : on ? t('sprint.js.audio_on') : t('sprint.js.audio_off');
    },
    init: function () {
      if (!this.btn) return;
      var self = this;
      if (!Sp || !Sp.supported) { this.btn.disabled = true; this.paint(); return; }
      this.paint();
      this.btn.addEventListener("click", function () {
        Sp.setAudio(!self.on());
        self.paint();
      });
    }
  };
  Audio.init();

  // ---------- home: the path ----------
  var paths = document.querySelectorAll(".sprint-path");
  if (paths.length) {
    var nodes = Array.prototype.slice.call(document.querySelectorAll(".sprint-node"));
    // A sheet slides shut the way the profile's AI-only sections do
    // (static/profile.js): height, spacing and opacity to zero, 280 ms.
    var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    var edges = function (el, cs, open) {
      return {
        height: open ? el.offsetHeight + "px" : "0px", opacity: open ? 1 : 0,
        marginTop: open ? cs.marginTop : "0px", marginBottom: open ? cs.marginBottom : "0px",
        paddingTop: open ? cs.paddingTop : "0px", paddingBottom: open ? cs.paddingBottom : "0px",
        borderTopWidth: open ? cs.borderTopWidth : "0px", borderBottomWidth: open ? cs.borderBottomWidth : "0px"
      };
    };
    var shut = function (el) {
      if (el.slide) { el.slide.cancel(); el.slide = null; el.style.overflow = ""; }
      if (el.hidden) return;
      if (reduce || !el.animate) { el.hidden = true; return; }
      var cs = getComputedStyle(el);
      el.style.overflow = "hidden";
      el.slide = el.animate([edges(el, cs, true), edges(el, cs, false)], { duration: 280, easing: "ease" });
      el.slide.onfinish = function () { el.hidden = true; el.style.overflow = ""; el.slide = null; };
    };
    var closeSheets = function () {
      nodes.forEach(function (n) { shut(n.querySelector(".sprint-sheet")); n.querySelector(".sprint-node-btn").setAttribute("aria-expanded", "false"); n.classList.remove("open"); });
    };
    nodes.forEach(function (node) {
      var btn = node.querySelector(".sprint-node-btn");
      var sheet = node.querySelector(".sprint-sheet");
      btn.addEventListener("click", function () {
        var open = node.classList.contains("open");
        closeSheets();
        if (!open) {
          if (sheet.slide) { sheet.slide.cancel(); sheet.slide = null; sheet.style.overflow = ""; }
          sheet.hidden = false; btn.setAttribute("aria-expanded", "true"); node.classList.add("open");
          Sound.play("tap");
          setTimeout(function () { sheet.scrollIntoView({ block: "nearest", behavior: "smooth" }); }, 50);
        }
      });
    });
    // A tap or click anywhere outside the open stop (its button and its
    // sheet) puts the sheet away, and so does Escape (#233).
    document.addEventListener("click", function (ev) {
      var openNode = document.querySelector(".sprint-node.open");
      if (openNode && !openNode.contains(ev.target)) closeSheets();
    });
    document.addEventListener("keydown", function (ev) {
      if (ev.key !== "Escape") return;
      var openNode = document.querySelector(".sprint-node.open");
      if (!openNode) return;
      closeSheets();
      var b = openNode.querySelector(".sprint-node-btn");
      if (b) b.focus();
    });
    var cur = document.querySelector(".sprint-node.current");
    if (cur && !location.hash) setTimeout(function () {
      var r = cur.getBoundingClientRect();
      if (r.top > window.innerHeight * 0.8) cur.scrollIntoView({ block: "center", behavior: "smooth" });
    }, 250);
    return;
  }

  // ---------- player ----------
  var sessionEl = document.getElementById("session");
  if (!sessionEl) return;
  var S = JSON.parse(sessionEl.textContent);
  var exercises = S.exercises || [];
  var C = S.course || {};
  var ICON = C.icons || {};
  function icon(slot, fallback) { return ICON[slot] || fallback; }
  var TL = C.term_lang || "", ML = C.meaning_lang || "";
  // Only a language course talks: elsewhere (maths, general) nothing is read
  // out and no speaker buttons appear.
  var SAY = !!C.say_lang;
  var PRAISE = (C.praise && C.praise.length) ? C.praise : [t('sprint.js.praise_1'), t('sprint.js.praise_2'), t('sprint.js.praise_3')];
  var ALMOST = (C.almost && C.almost.length) ? C.almost : [t('sprint.js.almost_1'), t('sprint.js.almost_2')];
  var CHARS = C.chars || [];
  var stage = document.getElementById("stage");

  // Say `text` in the course's language (nothing when audio is off).
  function say(text, opts) {
    if (!Sp || !text || !SAY) return false;
    return Sp.say(text, TL, opts);
  }
  // A small speaker button that says `text` when tapped (nothing, an empty
  // text node, in a course that does not talk).
  function sayBtn(text, cls) {
    if (!SAY) return document.createTextNode("");
    var b = el("button", "sprint-say" + (cls ? " " + cls : ""), "🔊");
    b.type = "button"; b.setAttribute("aria-label", t('sprint.js.hear_it')); b.title = t('sprint.js.hear_it');
    b.addEventListener("click", function (ev) {
      ev.stopPropagation();
      say(text, { force: true, onstart: function () { b.classList.add("saying"); }, onend: function () { b.classList.remove("saying"); } });
    });
    return b;
  }
  // Can a listening exercise be played right now? Audio on, and a voice for
  // the language once the browser has handed out its list.
  function listenable() {
    if (!Audio.on()) return false;
    if (Sp.anyVoices() && !Sp.hasVoice(TL)) return false;
    return true;
  }
  function isListening(e) { return e.kind.indexOf("hear-") === 0; }
  // A word in the course's language that was just tapped (a tile, an option,
  // a word of a sentence) is read out, unless audio is off.
  function sayTap(text) { say(text); }
  var foot = document.getElementById("foot");
  var btnCheck = document.getElementById("btn-check");
  var result = document.getElementById("result");
  var btnNext = document.getElementById("btn-next");
  var pFill = document.getElementById("p-fill");
  var pBar = document.getElementById("p-bar");
  var comboEl = document.getElementById("combo");
  var timerEl = document.getElementById("timer");

  var pos = 0, answered = 0, correct = 0, combo = 0, bestCombo = 0;
  var started = Date.now();
  var finished = false;
  var current = null; // the active exercise controller
  var timeLeft = S.timed_secs || 0, timerHandle = null;

  function setProgress() {
    var pct = exercises.length ? Math.round((pos / exercises.length) * 100) : 0;
    pFill.style.width = pct + "%";
    pBar.setAttribute("aria-valuenow", pct);
  }
  function setReady(ok) { btnCheck.disabled = !ok; }
  function setCheckLabel(t) { btnCheck.textContent = t; }

  function bumpCombo(ok) {
    if (ok) { combo += 1; if (combo > bestCombo) bestCombo = combo; } else combo = 0;
    comboEl.hidden = combo < 2;
    comboEl.querySelector("b").textContent = combo;
    if (combo >= 2) { comboEl.classList.remove("pop"); void comboEl.offsetWidth; comboEl.classList.add("pop"); }
  }

  // Result sheet shared by every graded exercise.
  function showResult(res, extra) {
    clearError();
    var ok = !!res.correct;
    answered += 1;
    if (ok) correct += 1;
    bumpCombo(ok);
    result.className = "sprint-result " + (ok ? "ok" : "bad");
    result.dataset.state = ok ? "right" : "wrong";
    var verdicts = ok ? PRAISE : ALMOST;
    document.getElementById("r-verdict").textContent = verdicts[Math.floor(Math.random() * verdicts.length)] + (ok && combo >= 3 ? "  " + icon("streak", "🔥") + "×" + combo : "");
    var html = "";
    if (res.note) html += "<p class=\"r-note\">" + esc(res.note) + "</p>";
    // What was (or should have been) said gets a speaker: on the answer line
    // when that is it, else on a line of its own (the sentence behind a meaning).
    var sayable = extra && extra.say;
    var sayIsAnswer = sayable && sayable === res.correct_answer;
    // A tune (`extra.staff`, options drawn as staves) is drawn on the line once the sheet shows (#1308).
    var tune = extra && extra.staff;
    if (!ok && res.correct_answer && !(extra && extra.noKey)) html += "<p class=\"r-answer\">" + t('sprint.js.answer_html', { lang: esc(sayIsAnswer ? TL : ML), answer: tune ? "" : esc(res.correct_answer) }) + (sayIsAnswer ? " <span class=\"r-say\"></span>" : "") + "</p>";
    if (sayable && (!sayIsAnswer || (ok && extra.sayShow))) html += "<p class=\"r-answer\"><strong lang=\"" + esc(TL) + "\">" + esc(sayable) + "</strong> <span class=\"r-say\"></span></p>";
    // Exercises with no one answer (sort, ladder, match) say what went wrong on the way.
    if (extra && extra.slips) html += "<p class=\"r-answer\">" + esc(extra.slips) + "</p>";
    if (res.explanation) html += "<p class=\"r-why\">" + esc(res.explanation) + "</p>";
    if (typeof extra === "string") html += extra;
    if (res.new_words && res.new_words.length) {
      html += "<p class=\"r-words\">" + t('sprint.js.collected_html', { words: res.new_words.map(function (w) { return "<strong lang=\"" + esc(TL) + "\" data-say=\"" + esc(w.term) + "\">" + esc(w.term) + "</strong> · " + esc(w.meaning); }).join(", ") }) + "</p>";
    }
    if (typeof res.mastery === "number" && res.lesson_title) {
      html += "<p class=\"r-mastery\">" + t('sprint.js.mastery_of', { title: esc(res.lesson_title), pct: res.mastery }) + "</p>";
    } else if (typeof res.mastery === "number") {
      html += "<p class=\"r-mastery\">" + t('sprint.js.mastery_now', { pct: res.mastery }) + "</p>";
    }
    var rt = document.getElementById("r-text");
    rt.innerHTML = html;
    rt.querySelectorAll(".r-say").forEach(function (slot) { slot.replaceWith(sayBtn(sayable)); });
    if (SAY) rt.querySelectorAll("[data-say]").forEach(function (w) {
      w.classList.add("can-say");
      w.addEventListener("click", function () { say(w.dataset.say, { force: true }); });
    });
    result.hidden = false;
    foot.hidden = true;
    var line = rt.querySelector(".r-answer");
    if (!ok && tune && line && window.drawStaff) { line.querySelector("strong").remove(); window.drawStaff(line.appendChild(el("div")), tune).dataset.testid = "answer-staff"; }
    btnNext.textContent = pos + 1 >= exercises.length ? t('sprint.js.finish') : t('sprint.js.continue');
    btnNext.focus();
    if (ok) { Sound.play("ok"); buzz(30); } else { Sound.play("bad"); buzz([60, 40, 60]); }
    // The sentence is read out once it is checked, so the learner hears it right.
    if (sayable) setTimeout(function () { if (!result.hidden) say(sayable); }, ok ? 350 : 250);
  }

  // One error line per exercise: a retry that fails again replaces its text
  // (and flashes it, so the tap shows), a check that gets through removes it.
  var errorLine = null;
  function fail(err) {
    if (err && err.message === "course") return; // on the way to the course list (#1083)
    setReady(true);
    if (!errorLine || !stage.contains(errorLine)) {
      errorLine = el("p", "sprint-error");
      errorLine.dataset.testid = "sprint-play-error";
      errorLine.setAttribute("role", "alert");
      stage.appendChild(errorLine);
    }
    if (window.console && console.warn) console.warn("sprint check failed:", err);
    errorLine.textContent = t('sprint.js.check_failed');
    errorLine.classList.remove("again"); void errorLine.offsetWidth; errorLine.classList.add("again");
  }
  function clearError() {
    if (errorLine) errorLine.remove();
    errorLine = null;
  }

  // Submit through /api/answer (quiz questions).
  function gradeQuiz(id, answer) {
    return post("/api/answer", { question_id: id, answer: answer });
  }
  // Submit through /api/sprint/check (Sprint mode exercises).
  function gradeSprint(id, answer, mistakes) {
    return post("/api/sprint/check", { id: id, answer: answer, mistakes: mistakes || 0 });
  }

  function lessonTag(e) {
    return S.mode === "lesson" ? "" : "<p class=\"sprint-tag\">" + esc(e.lesson) + "</p>";
  }
  function hintBlock(e) {
    if (!e.hint) return null;
    var d = el("details", "sprint-hint", "<summary>" + esc(icon("hint", "💡")) + " " + esc(t('sprint.js.hint')) + "</summary><p>" + esc(e.hint) + "</p>");
    return d;
  }
  function accentBar(input) {
    var bar = el("div", "sprint-accents");
    if (!CHARS.length) { bar.hidden = true; return bar; }
    CHARS.forEach(function (ch) {
      var b = el("button", "", esc(ch)); b.type = "button"; b.setAttribute("aria-label", t('sprint.js.type_char', { ch: ch }));
      b.addEventListener("click", function () {
        var s = input.selectionStart || 0, en = input.selectionEnd || 0;
        input.value = input.value.slice(0, s) + ch + input.value.slice(en);
        input.focus(); input.setSelectionRange(s + 1, s + 1);
        input.dispatchEvent(new Event("input"));
      });
      bar.appendChild(b);
    });
    return bar;
  }

  // ----- renderers: each returns a controller { submit() } -----

  function renderBite(e) {
    stage.innerHTML = "";
    var card = el("article", "sprint-bite",
      "<p class=\"sprint-kicker\">" + esc(t('sprint.js.quick_bite')) + "</p>" +
      "<div class=\"sprint-bite-emoji\">" + esc(e.emoji || icon("bite", "💡")) + "</div>" +
      "<h2>" + esc(e.title) + "</h2>" +
      "<p class=\"sprint-bite-rule\">" + esc(e.rule) + "</p>" +
      (e.example ? "<p class=\"sprint-bite-ex\"><span lang=\"" + esc(TL) + "\">" + esc(e.example) + "</span>" + (e.example_meaning ? "<small>" + esc(e.example_meaning) + "</small>" : "") + "</p>" : ""));
    stage.appendChild(card);
    if (e.example) {
      var ex = card.querySelector(".sprint-bite-ex");
      ex.appendChild(sayBtn(e.example, "inline"));
      ex.querySelector("span").addEventListener("click", function () { say(e.example, { force: true }); });
    }
    setCheckLabel(t('sprint.js.got_it'));
    setReady(true);
    return { submit: function () { Sound.play("tap"); next(); } };
  }

  // A quiz item's music: `abc` drawn as a staff above its prompt (notation.js,
  // #1218); `play` a speaker that plays it (music-sound.js, #1219), and with no
  // `abc` the played notes are drawn once the answer is graded.
  var Music = window.MusicSound || null;
  function staffFor(e) {
    if (e.abc && window.drawStaff) window.drawStaff(stage.appendChild(el("div", "sprint-staff")), e.abc);
    if (e.play && Music) Music.speaker(stage.appendChild(el("div", "sprint-music")), e.play, { watch: result, staffAfter: !e.abc, cant: cantListen }).button.classList.add("sprint-speaker");
  }
  function cantListen() { Audio.paint(); next(); }
  // An exercise that can't be done without sound: skipped while sound is off.
  function isEar(e) { return !!(e.play && !e.abc) || (e.kind === "sort" && e.items.some(function (i) { return i.play && !i.text; })); }

  function renderOptions(e, prompt, options, grade, label, sayPrompt, sayOptions) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", label));
    staffFor(e);
    var promptEl = el("p", "sprint-prompt", "<span lang=\"" + esc(TL) + "\">" + esc(prompt) + "</span>");
    stage.appendChild(promptEl);
    if (sayPrompt) {
      promptEl.appendChild(document.createTextNode(" "));
      promptEl.appendChild(sayBtn(prompt, "inline"));
      promptEl.querySelector("span").addEventListener("click", function () { say(prompt, { force: true }); });
      setTimeout(function () { say(prompt); }, 300);
    }
    var list = el("div", "sprint-options");
    var chosen = null;
    options.forEach(function (o, i) {
      var b = el("button", "sprint-opt", "<span class=\"key\">" + (i + 1) + "</span><span lang=\"" + esc(TL) + "\">" + esc(o) + "</span>");
      b.type = "button"; b.dataset.value = o;
      b.addEventListener("click", function () {
        pick(list.querySelectorAll(".sprint-opt"), b); chosen = o; Sound.play("tap"); setReady(true);
        if (sayOptions && !e.staves) sayTap(o);
      });
      list.appendChild(b);
    });
    stage.appendChild(list);
    // Options drawn as staves (`option_kind: "abc"`): each tune in place of its text.
    if (e.staves && window.drawStaff) list.querySelectorAll(".sprint-opt").forEach(function (b, i) {
      b.classList.add("sprint-opt-staff"); b.lastChild.textContent = ""; window.drawStaff(b.lastChild, e.staves[i]);
    });
    var h = hintBlock(e); if (h) stage.appendChild(h);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      key: function (n) { var b = list.querySelectorAll(".sprint-opt")[n - 1]; if (b) b.click(); },
      submit: function () {
        if (chosen === null) return;
        setReady(false);
        grade(chosen).then(function (res) {
          list.querySelectorAll(".sprint-opt").forEach(function (x) {
            x.disabled = true;
            if (x.dataset.value === res.correct_answer) mark(x, "right");
            else if (x.dataset.value === chosen && !res.correct) mark(x, "wrong");
          });
          showResult(res, e.staves ? { staff: e.staves[options.indexOf(res.correct_answer)] } : undefined);
        }).catch(fail);
      }
    };
  }

  function renderChoice(e) {
    return renderOptions(e, e.prompt, e.options, function (a) { return gradeQuiz(e.id, a); }, esc(t('sprint.js.pick_right')), false, true);
  }

  // Spell the chord (#1228): note tiles into slots, drawn as a chord (chord-spell.js).
  function renderChordSpell(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.spell_chord'))));
    stage.appendChild(el("p", "sprint-prompt", esc(e.prompt)));
    var answer = null;
    var w = window.ChordSpell.mount(stage.appendChild(el("div", "chord-spell")), e.tiles, e.slots, function (a) { answer = a; setReady(!!a); });
    var h = hintBlock(e); if (h) stage.appendChild(h);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      submit: function () {
        if (!answer) return;
        setReady(false);
        gradeQuiz(e.id, answer).then(function (res) {
          w.lock();
          showResult(res);
          w.reveal(document.getElementById("r-text"));
        }).catch(fail);
      }
    };
  }

  // Place the note (#1226): tap the staff, drag, then Check (staff-note.js).
  function renderStaffNote(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.place_note'))));
    stage.appendChild(el("p", "sprint-prompt", esc(e.prompt)));
    var answer = null;
    var w = window.StaffNote.mount(stage.appendChild(el("div", "staff-note")), e.clef, e.names, function (a) { answer = a; setReady(!!a); });
    var h = hintBlock(e); if (h) stage.appendChild(h);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      submit: function () {
        if (!answer) return;
        setReady(false);
        gradeQuiz(e.id, answer).then(function (res) {
          w.lock(res.correct ? "right" : "wrong");
          showResult(res);
          w.reveal(document.getElementById("r-text"));
        }).catch(fail);
      }
    };
  }

  // Find it on the keys (#1231): a piano keyboard (keyboard.js) whose keys
  // sound on a tap; the last key tapped is the answer, graded by /api/answer.
  function renderKeyboard(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.find_key'))));
    staffFor(e);
    stage.appendChild(el("p", "sprint-prompt", esc(e.prompt)));
    var answer = null;
    var kb = window.PianoKeys.mount(stage.appendChild(el("div", "keyboard")), { octaves: e.octaves, onKey: function (a) { answer = a; setReady(true); } });
    var h = hintBlock(e); if (h) stage.appendChild(h);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      submit: function () {
        if (!answer) return;
        setReady(false);
        gradeQuiz(e.id, answer).then(function (res) { kb.lock(); kb.reveal(res.key, e.octave); showResult(res); }).catch(fail);
      }
    };
  }

  // Set the clock (#1340): drag the hands (static/clock.js), or, turned
  // around, read the clock and pick its time from the options.
  function renderClock(e) {
    var hands = e.hands ? e.hands.split(" ").map(Number) : null;
    var ctl = hands ? renderChoice(e) : null;
    if (!ctl) {
      stage.innerHTML = lessonTag(e);
      stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.set_clock'))));
      stage.appendChild(el("p", "sprint-prompt", "<span lang=\"" + esc(TL) + "\">" + esc(e.prompt) + "</span>"));
    }
    var box = el("div", "sprint-clock");
    box.dataset.testid = "clock";
    var prompt = stage.querySelector(".sprint-prompt");
    prompt.parentNode.insertBefore(box, prompt.nextSibling);
    var done = false;
    var clock = Clock.make(box, { hands: hands, step: e.step, locked: function () { return done; } });
    if (ctl) return ctl;
    var h = hintBlock(e); if (h) stage.appendChild(h);
    setCheckLabel(t('sprint.js.check'));
    setReady(true);
    setTimeout(function () { clock.el.focus(); }, 50);
    return {
      submit: function () {
        if (done) return;
        done = true; setReady(false);
        gradeQuiz(e.id, clock.time()).then(showResult).catch(function (err) { done = false; fail(err); });
      }
    };
  }

  function renderPick(e) {
    // A term shown in the course's language is read out and can be tapped.
    return renderOptions(e, e.prompt, e.options, function (a) { return gradeSprint(e.id, a, 0); },
      e.to_term ? t('sprint.js.in_lang_this_is', { lang: esc(C.term_label || t('sprint.js.course_language')) }) : esc(t('sprint.js.this_means')), !e.to_term, e.to_term);
  }

  function renderTiles(e) {
    // Maths questions (e.math): numbers and formulas are not the course's
    // language, so no speech and no accent bar; with no tiles (an
    // expression) the answer is typed straight away.
    var math = !!e.math, lang = math ? "" : TL;
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(math ? (e.tiles.length ? t('sprint.js.work_it_out') : t('sprint.js.write_answer')) : t('sprint.js.fill_gap'))));
    staffFor(e);
    var hasBlank = e.prompt.indexOf("___") >= 0;
    var promptEl = el("p", "sprint-prompt");
    var slot = el("span", "sprint-slot", "&nbsp;");
    if (hasBlank) {
      var parts = e.prompt.split("___");
      promptEl.appendChild(document.createTextNode(parts[0]));
      promptEl.appendChild(slot);
      promptEl.appendChild(document.createTextNode(parts.slice(1).join("___")));
    } else {
      promptEl.innerHTML = lang ? "<span lang=\"" + esc(lang) + "\">" + esc(e.prompt) + "</span>" : esc(e.prompt);
      promptEl.appendChild(document.createElement("br"));
      promptEl.appendChild(slot);
    }
    if (e.unit) promptEl.appendChild(el("span", "sprint-unit", " " + esc(e.unit)));
    if (lang) promptEl.lang = lang;
    stage.appendChild(promptEl);
    var bank = el("div", "sprint-bank");
    var chosen = null, typing = !e.tiles.length;
    e.tiles.forEach(function (t) {
      var b = el("button", "sprint-tile", esc(t)); b.type = "button"; if (lang) b.lang = lang;
      b.addEventListener("click", function () {
        pick(bank.querySelectorAll(".sprint-tile"), b); chosen = t; slot.textContent = t; slot.classList.add("filled"); Sound.play("tap"); setReady(true);
        if (!math) sayTap(t);
      });
      bank.appendChild(b);
    });
    bank.hidden = typing;
    stage.appendChild(bank);
    var typeWrap = el("div", "sprint-type"); typeWrap.hidden = !typing;
    var input = el("input", "sprint-input"); input.type = "text"; if (lang) input.lang = lang; input.autocomplete = "off"; input.autocapitalize = "off"; input.spellcheck = false;
    input.placeholder = e.placeholder || t('sprint.js.type_it');
    if (math && e.tiles.length) input.inputMode = "decimal";
    if (math) input.dataset.math = "1"; // as in Study and Play: numbers and formulas, not the course's language
    input.addEventListener("input", function () { chosen = input.value; slot.textContent = input.value || " "; slot.classList.toggle("filled", !!input.value); setReady(!!input.value.trim()); });
    typeWrap.appendChild(input);
    if (!math) typeWrap.appendChild(accentBar(input));
    stage.appendChild(typeWrap);
    if (e.tiles.length) {
      var label = esc(math ? t('sprint.js.type_instead') : t('sprint.js.type_instead_honest'));
      var toggle = el("button", "sprint-link", label); toggle.type = "button";
      toggle.addEventListener("click", function () {
        typing = !typing; typeWrap.hidden = !typing; bank.hidden = typing; chosen = null; slot.textContent = " "; slot.classList.remove("filled"); setReady(false);
        pick(bank.querySelectorAll(".sprint-tile"));
        toggle.textContent = typing ? t('sprint.js.back_to_tiles') : t(math ? 'sprint.js.type_instead' : 'sprint.js.type_instead_honest');
        if (typing) input.focus();
      });
      stage.appendChild(toggle);
    } else {
      setTimeout(function () { input.focus(); }, 50);
    }
    var h = hintBlock(e); if (h) stage.appendChild(h);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      submit: function () {
        if (chosen === null || !String(chosen).trim()) return;
        setReady(false); input.disabled = true;
        gradeQuiz(e.id, chosen).then(function (res) {
          bank.querySelectorAll(".sprint-tile").forEach(function (x) {
            x.disabled = true;
            if (x.textContent === res.correct_answer) mark(x, "right");
            else if (x.textContent === chosen && !res.correct) mark(x, "wrong");
          });
          mark(slot, res.correct ? "right" : "wrong");
          showResult(res);
        }).catch(function (err) {
          // A check that never got through leaves the typed answer editable (#672).
          input.disabled = false;
          fail(err);
        });
      }
    };
  }

  // Fill the bar (#1230): note-value tiles into the gap of a bar (fill-bar.js).
  function renderFillBar(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.fill_gap'))));
    stage.appendChild(el("p", "sprint-prompt", esc(e.prompt)));
    var answer = null;
    var bar = window.FillBar.mount(stage.appendChild(el("div", "fill-bar")), e.abc, e.tiles, function (a) { answer = a; setReady(!!a); });
    var h = hintBlock(e); if (h) stage.appendChild(h);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      submit: function () {
        if (!answer) return;
        setReady(false);
        gradeQuiz(e.id, answer).then(function (res) {
          bar.lock();
          showResult(res);
          bar.reveal(document.getElementById("r-text"));
        }).catch(fail);
      }
    };
  }

  // Sort it: each tap on a bucket is judged at once (#1148, as before #412). The
  // right bucket takes the word on top of the pile, marked right; a wrong one
  // shakes the word, which stays on the pile, and counts as a wrong tap. The
  // sort is checked as soon as the pile is empty: all sorted without a wrong tap
  // is right, any wrong tap makes it wrong ("All sorted, but with 1 wrong tap").
  function renderSort(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.sort_it'))));
    stage.appendChild(el("h2", "sprint-title", esc(e.title)));
    var queue = e.items.slice();
    var placed = [], mistakes = 0;
    var graded = false;
    var cardWrap = el("div", "sprint-sort-card");
    var card = el("div", "sprint-sort-item");
    cardWrap.appendChild(card);
    var left = el("p", "sprint-sort-left");
    stage.appendChild(cardWrap);
    var buckets = el("div", "sprint-buckets b" + e.buckets.length);
    var lists = [], btns = [];
    function tap(bi) {
      if (graded || !queue.length) return;
      var it = queue[0];
      var b = btns[bi];
      if (it.bucket === bi) {
        queue.shift();
        placed.push({ text: it.text || it.play, bucket: bi });
        var chip = it.text || !Music ? el("span", "sprint-chip", esc(it.text)) : Music.chip(it.play); chip.lang = TL;
        chip.classList.add("sprint-chip");
        chip.dataset.testid = "sprint-sort-chip";
        chip.dataset.state = "right";
        lists[bi].appendChild(chip);
        // A wrong bucket's red is a flash: the next right tap clears it.
        btns.forEach(function (x) { x.classList.remove("miss"); });
        b.classList.remove("hit"); void b.offsetWidth; b.classList.add("hit");
        Sound.play("tap"); buzz(15);
        showNext();
      } else {
        mistakes += 1;
        card.classList.remove("shake"); void card.offsetWidth; card.classList.add("shake");
        b.classList.remove("miss"); void b.offsetWidth; b.classList.add("miss");
        Sound.play("bad"); buzz(60);
      }
    }
    e.buckets.forEach(function (name, bi) {
      var col = el("div", "sprint-bucket-col");
      var b = el("button", "sprint-bucket", "<strong>" + esc(name) + "</strong>"); b.type = "button";
      b.dataset.testid = "sprint-sort-bucket";
      b.addEventListener("click", function () { tap(bi); });
      var words = el("div", "sprint-bucket-words");
      col.appendChild(b); col.appendChild(words);
      btns.push(b); lists.push(words);
      buckets.appendChild(col);
    });
    stage.appendChild(buckets);
    stage.appendChild(left);
    var ctl = {
      submit: function () {
        if (queue.length || graded) return;
        setReady(false);
        graded = true;
        // The server marks it wrong when there were wrong taps and says how many (its note).
        gradeSprint(e.id, placed, mistakes).then(function (res) {
          btns.forEach(function (x) { x.disabled = true; });
          showResult(res, { noKey: true });
        }).catch(function (err) { graded = false; fail(err); });
      }
    };
    function showNext() {
      if (!queue.length) {
        card.innerHTML = "<span class=\"done\">" + esc(t('sprint.js.all_sorted')) + "</span>";
        left.textContent = "";
        setReady(true);
        ctl.submit();
        return;
      }
      card.innerHTML = queue[0].text || !Music ? "<span lang=\"" + esc(TL) + "\">" + esc(queue[0].text) + "</span>" : "";
      // A speaker tile (#1219): play it, then tap its bucket.
      if (!queue[0].text && Music) Music.speaker(card.appendChild(el("div")), queue[0].play, { cant: cantListen }).button.classList.add("sprint-speaker");
      card.classList.remove("in"); void card.offsetWidth; card.classList.add("in");
      left.textContent = t('sprint.js.to_go', { n: queue.length });
      setReady(false);
    }
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    showNext();
    return ctl;
  }

  // A spot on a staff (#1227): tap the wrong note, then its fix (wrong-note.js).
  function renderWrongNote(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.spot_mistake'))));
    stage.appendChild(el("p", "sprint-sub", esc(t('sprint.js.one_note_wrong'))));
    var answer = null;
    var spot = window.WrongNote.mount(stage.appendChild(el("div", "wrong-note")), e.abc, e.tiles, function (a) { answer = a; setReady(!!a); });
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      submit: function () {
        if (!answer) return;
        setReady(false);
        gradeSprint(e.id, answer, 0).then(function (res) {
          spot.lock();
          spot.reveal(!!res.correct, !!res.correct || !!res.note);
          showResult(res);
        }).catch(fail);
      }
    };
  }

  function renderSpot(e) {
    if (e.abc && window.WrongNote) return renderWrongNote(e);
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.spot_mistake'))));
    stage.appendChild(el("p", "sprint-sub", esc(t('sprint.js.one_word_wrong'))));
    var sent = el("p", "sprint-sentence"); sent.lang = TL;
    var chosen = null;
    e.words.forEach(function (w, i) {
      var b = el("button", "sprint-word", esc(w)); b.type = "button";
      b.addEventListener("click", function () {
        pick(sent.querySelectorAll(".sprint-word"), b); chosen = i; Sound.play("tap"); setReady(true);
        sayTap(w);
      });
      sent.appendChild(b);
      sent.appendChild(document.createTextNode(" "));
    });
    stage.appendChild(sent);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      submit: function () {
        if (chosen === null) return;
        setReady(false);
        gradeSprint(e.id, chosen, 0).then(function (res) {
          var words = sent.querySelectorAll(".sprint-word");
          words.forEach(function (x) { x.disabled = true; });
          var wrongWord = (res.correct_answer || "").split(" → ")[0];
          var fix = (res.correct_answer || "").split(" → ")[1] || "";
          words.forEach(function (x, i) {
            var plain = x.textContent.replace(/^[¿¡"'«(]+|[.!?,;:"'»)]+$/g, "").toLowerCase();
            if (plain === wrongWord.toLowerCase()) { mark(x, "right"); if (fix) x.innerHTML = "<s>" + esc(x.textContent) + "</s> " + esc(fix); }
            else if (i === chosen && !res.correct) mark(x, "wrong");
          });
          showResult(res);
        }).catch(fail);
      }
    };
  }

  function renderBuild(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.build_sentence'))));
    stage.appendChild(el("p", "sprint-prompt en", esc(e.meaning)));
    var answer = el("div", "sprint-answer-line"); answer.lang = TL; answer.dataset.empty = t("sprint.js.tap_tiles_in_order");
    var bank = el("div", "sprint-bank"); bank.lang = TL;
    stage.appendChild(answer);
    stage.appendChild(bank);
    var chosen = []; // tile elements in order
    function sync() { setReady(chosen.length > 0); answer.classList.toggle("empty", chosen.length === 0); }
    e.tiles.forEach(function (t) {
      var b = el("button", "sprint-tile", esc(t)); b.type = "button";
      b.addEventListener("click", function () {
        Sound.play("tap");
        if (b.parentNode === bank) { chosen.push(b); answer.appendChild(b); sayTap(t); }
        else { chosen.splice(chosen.indexOf(b), 1); bank.appendChild(b); }
        sync();
      });
      bank.appendChild(b);
    });
    sync();
    setCheckLabel(t('sprint.js.check'));
    return {
      submit: function () {
        if (!chosen.length) return;
        setReady(false);
        var text = chosen.map(function (b) { return b.textContent; }).join(" ");
        gradeSprint(e.id, text, 0).then(function (res) {
          stage.querySelectorAll(".sprint-tile").forEach(function (x) { x.disabled = true; });
          mark(answer, res.correct ? "right" : "wrong");
          showResult(res, { say: res.correct_answer });
        }).catch(fail);
      }
    };
  }

  // Ladder: every tap on a form puts it on the glowing rung (the first empty
  // one), right or wrong, at once. A filled rung is a button: tap it and its
  // form goes back to the bank, so the next tap moves it. Nothing is judged
  // until Check, which opens once every rung is filled (like Sort it, #640).
  // A scale (#1229) is the same ladder with notes: rungs are the degrees, the
  // first comes filled, and scale-ladder.js draws and plays each note as it lands.
  function renderLadder(e) {
    var scale = e.kind === "scale", lang = scale ? "" : TL;
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t(scale ? 'sprint.js.scale' : 'sprint.js.ladder'))));
    if (scale) stage.appendChild(el("h2", "sprint-title", esc(e.title)));
    else stage.appendChild(el("h2", "sprint-title", "<span lang=\"" + esc(TL) + "\">" + esc(e.item) + "</span> <small>" + esc(e.meaning) + " · " + esc(e.set).toLowerCase() + "</small>"));
    var music = scale && window.ScaleLadder ? window.ScaleLadder.mount(stage.appendChild(el("div", "")), e) : null;
    var ladder = el("ol", "sprint-ladder");
    var rungs = []; // { btn, form, tile, abc }
    var graded = false;
    e.slots.forEach(function (s, i) {
      var li = el("li", "");
      var btn = el("button", "sprint-rung", "<span class=\"who\">" + esc(s) + "</span><span class=\"form\"" + (lang ? " lang=\"" + esc(lang) + "\"" : "") + "></span>");
      btn.type = "button"; btn.disabled = true;
      btn.dataset.testid = "sprint-ladder-rung";
      var r = { btn: btn, form: btn.querySelector(".form"), tile: null };
      // A scale's first rung comes filled and stays.
      if (scale && i === 0) { r.tile = true; r.fixed = true; r.form.textContent = e.forms[0]; r.abc = e.notes[0]; btn.classList.add("given"); }
      btn.addEventListener("click", function () { lift(r); });
      li.appendChild(btn); ladder.appendChild(li); rungs.push(r);
    });
    stage.appendChild(ladder);
    var bank = el("div", "sprint-bank ladder"); if (lang) bank.lang = lang;
    function firstEmpty() { for (var i = 0; i < rungs.length; i++) if (!rungs[i].tile) return i; return -1; }
    function update() {
      var next = firstEmpty();
      rungs.forEach(function (r, i) { r.btn.classList.toggle("next", i === next); });
      setReady(next < 0 && !graded);
      if (music) music.draw(rungs.map(function (r) { return r.abc || ""; }));
    }
    function lift(r) {
      if (graded || !r.tile || r.fixed) return;
      var hadFocus = document.activeElement === r.btn;
      var tile = r.tile;
      r.tile = null; r.abc = ""; r.form.textContent = "";
      r.btn.disabled = true; r.btn.removeAttribute("aria-label");
      tile.disabled = false; tile.classList.remove("used"); delete tile.dataset.state;
      Sound.play("tap"); buzz(10);
      update();
      // A keyboard user keeps their place: focus goes to the form that came back.
      if (hadFocus) tile.focus();
    }
    function place(tile, text) {
      var i = firstEmpty();
      if (graded || i < 0) return;
      var r = rungs[i];
      var hadFocus = document.activeElement === tile;
      if (music) music.play(tile.dataset.abc); else sayTap(text);
      r.tile = tile; r.abc = tile.dataset.abc; r.form.textContent = text;
      r.btn.disabled = false;
      r.btn.setAttribute("aria-label", t('sprint.js.ladder_move', { form: text }));
      tile.disabled = true; mark(tile, "used");
      Sound.play("tap"); buzz(15);
      update();
      // The placed form is disabled now: a keyboard user goes on from the next free one.
      if (hadFocus) { var free = bank.querySelector(".sprint-tile:not(:disabled)"); if (free) free.focus(); }
    }
    var ctl = {
      submit: function () {
        if (graded || firstEmpty() >= 0) return;
        setReady(false);
        // Frozen while the answer is on its way, so the forms shown are the ones graded.
        graded = true;
        var given = rungs.map(function (r) { return r.form.textContent; });
        gradeSprint(e.id, scale ? given.slice(1) : given, 0).then(function (res) {
          stage.querySelectorAll(".sprint-tile").forEach(function (x) { x.disabled = true; });
          rungs.forEach(function (r, i) {
            r.btn.disabled = true;
            r.btn.classList.remove("next");
            var ok = given[i] === e.forms[i];
            r.btn.dataset.state = ok ? "right" : "wrong";
            if (!ok) r.btn.appendChild(el("span", "fix", esc(e.forms[i])));
          });
          showResult(res);
        }).catch(function (err) { graded = false; fail(err); });
      }
    };
    e.tiles.forEach(function (text, ti) {
      var b = el("button", "sprint-tile", esc(text)); b.type = "button";
      b.dataset.testid = "sprint-ladder-tile";
      if (scale) b.dataset.abc = e.tile_notes[ti];
      b.addEventListener("click", function () { place(b, text); });
      bank.appendChild(b);
    });
    stage.appendChild(bank);
    stage.appendChild(el("p", "sprint-sub", esc(t(scale ? 'sprint.js.tap_note_rung' : 'sprint.js.tap_rung'))));
    setCheckLabel(t('sprint.js.check'));
    update();
    return ctl;
  }

  function renderMatch(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.match_pairs'))));
    var board = el("div", "sprint-match");
    var left = el("div", "col"), right = el("div", "col");
    board.appendChild(left); board.appendChild(right);
    stage.appendChild(board);
    var pairs = e.pairs.map(function (p, i) { return { id: i, term: p.term, meaning: p.meaning }; });
    var L = shuffle(pairs.slice()), R = shuffle(pairs.slice());
    var sel = { term: null, meaning: null };
    var known = [], missed = {}, done = 0;
    var ctl = {
      submit: function () {
        setReady(false);
        gradeSprint(e.id, { known: known.filter(function (w) { return !missed[w]; }), missed: Object.keys(missed) }, 0)
          .then(function (res) {
            var m = Object.keys(missed);
            showResult(res, { noKey: true, slips: m.length ? t('sprint.js.mixed_up', { list: m.join(", ") }) : "" });
          }).catch(fail);
      }
    };
    function tile(p, side) {
      var b = el("button", "sprint-tile match", esc(side === "term" ? p.term : p.meaning)); b.type = "button"; b.lang = side === "term" ? TL : ML;
      b.addEventListener("click", function () {
        if (b.classList.contains("done")) return;
        if (side === "term") sayTap(p.term);
        var col = side === "term" ? left : right;
        pick(col.querySelectorAll(".sprint-tile"), b); sel[side] = { p: p, b: b };
        Sound.play("tap");
        if (sel.term && sel.meaning) {
          var a = sel.term, c = sel.meaning; sel = { term: null, meaning: null };
          if (a.p.id === c.p.id) {
            pick([a.b, c.b]);
            mark(a.b, "done"); mark(c.b, "done");
            known.push(a.p.term); done += 1; buzz(15); Sound.play("ok");
            if (done === pairs.length) { setReady(true); ctl.submit(); }
          } else {
            missed[a.p.term] = true;
            pick([a.b, c.b]);
            [a.b, c.b].forEach(function (x) { x.classList.remove("shake"); void x.offsetWidth; x.classList.add("shake"); });
            Sound.play("bad"); buzz(60);
          }
        }
      });
      return b;
    }
    L.forEach(function (p) { left.appendChild(tile(p, "term")); });
    R.forEach(function (p) { right.appendChild(tile(p, "meaning")); });
    stage.appendChild(el("p", "sprint-sub", esc(t('sprint.js.tap_term_meaning'))));
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return ctl;
  }

  // ----- listening exercises -----

  // The speaker card every listening exercise starts with: a big play
  // button, a slow one, auto-play on arrival, and a way out ("Can't listen now").
  function audioCard(e, text, sub) {
    var card = el("div", "sprint-audio");
    var big = el("button", "sprint-speaker", "🔊"); big.type = "button"; big.setAttribute("aria-label", t('sprint.js.play')); big.title = t('sprint.js.play');
    var slow = el("button", "sprint-speaker slow", "🐢"); slow.type = "button"; slow.setAttribute("aria-label", t('sprint.js.play_slowly')); slow.title = t('sprint.js.slowly');
    function play(isSlow) {
      say(text, { slow: isSlow, force: true,
        onstart: function () { card.classList.add("speaking"); },
        onend: function () { card.classList.remove("speaking"); } });
    }
    big.addEventListener("click", function () { play(false); });
    slow.addEventListener("click", function () { play(true); });
    card.appendChild(big); card.appendChild(slow);
    stage.appendChild(card);
    if (sub) stage.appendChild(el("p", "sprint-sub", sub));
    var cant = el("button", "sprint-link sprint-cant", esc(t('sprint.js.cant_listen'))); cant.type = "button";
    cant.addEventListener("click", function () {
      // Off for an hour; the speaker button turns it back on.
      Sp.muteFor(60 * 60 * 1000); Audio.paint();
      next();
    });
    stage.appendChild(cant);
    setTimeout(function () { if (stage.contains(card) && result.hidden) play(false); }, 350);
    return { play: play, card: card, cant: cant };
  }

  // Tap what you hear: the sentence from word tiles.
  function renderHearBuild(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.tap_what_you_hear'))));
    var audio = audioCard(e, e.text);
    var answer = el("div", "sprint-answer-line"); answer.lang = TL; answer.dataset.empty = t("sprint.js.tap_tiles_in_order");
    var bank = el("div", "sprint-bank"); bank.lang = TL;
    stage.insertBefore(answer, audio.cant); stage.insertBefore(bank, audio.cant);
    var chosen = [];
    function sync() { setReady(chosen.length > 0); answer.classList.toggle("empty", chosen.length === 0); }
    e.tiles.forEach(function (t) {
      var b = el("button", "sprint-tile", esc(t)); b.type = "button";
      b.addEventListener("click", function () {
        Sound.play("tap");
        if (b.parentNode === bank) { chosen.push(b); answer.appendChild(b); sayTap(t); }
        else { chosen.splice(chosen.indexOf(b), 1); bank.appendChild(b); }
        sync();
      });
      bank.appendChild(b);
    });
    sync();
    setCheckLabel(t('sprint.js.check'));
    return {
      submit: function () {
        if (!chosen.length) return;
        setReady(false);
        var text = chosen.map(function (b) { return b.textContent; }).join(" ");
        gradeSprint(e.id, text, 0).then(function (res) {
          stage.querySelectorAll(".sprint-tile").forEach(function (x) { x.disabled = true; });
          mark(answer, res.correct ? "right" : "wrong");
          showResult(res, { say: res.correct_answer, sayShow: true });
        }).catch(fail);
      }
    };
  }

  // Fill the gap from what you hear: the sentence with one word missing, tiles to fill it.
  function renderHearGap(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.fill_what_you_hear'))));
    var audio = audioCard(e, e.text);
    var sent = el("p", "sprint-prompt"); sent.lang = TL;
    var slot = el("span", "sprint-slot", "&nbsp;");
    e.text.split(/\s+/).forEach(function (w, i) {
      if (i > 0) sent.appendChild(document.createTextNode(" "));
      if (i === e.gap) {
        // Keep the punctuation around the missing word.
        var m = /^([^\wÀ-ÿ]*)(.*?)([^\wÀ-ÿ]*)$/.exec(w) || [w, "", w, ""];
        if (m[1]) sent.appendChild(document.createTextNode(m[1]));
        sent.appendChild(slot);
        if (m[3]) sent.appendChild(document.createTextNode(m[3]));
      } else sent.appendChild(document.createTextNode(w));
    });
    stage.insertBefore(sent, audio.cant);
    var bank = el("div", "sprint-bank"); bank.lang = TL;
    var chosen = null;
    e.tiles.forEach(function (t) {
      var b = el("button", "sprint-tile", esc(t)); b.type = "button";
      b.addEventListener("click", function () {
        pick(bank.querySelectorAll(".sprint-tile"), b); chosen = t; slot.textContent = t; slot.classList.add("filled"); Sound.play("tap"); setReady(true);
        sayTap(t);
      });
      bank.appendChild(b);
    });
    stage.insertBefore(bank, audio.cant);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      key: function (n) { var b = bank.querySelectorAll(".sprint-tile")[n - 1]; if (b) b.click(); },
      submit: function () {
        if (chosen === null) return;
        setReady(false);
        gradeSprint(e.id, chosen, 0).then(function (res) {
          bank.querySelectorAll(".sprint-tile").forEach(function (x) {
            x.disabled = true;
            if (x.textContent === res.correct_answer) mark(x, "right");
            else if (x.textContent === chosen && !res.correct) mark(x, "wrong");
          });
          slot.textContent = res.correct_answer;
          mark(slot, res.correct ? "right" : "wrong");
          showResult(res, { say: e.text });
        }).catch(fail);
      }
    };
  }

  // What do you hear? / What does it mean?: the text is spoken, pick among written options.
  function renderHearPick(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(e.to_meaning ? t('sprint.js.what_does_it_mean') : t('sprint.js.what_do_you_hear'))));
    var audio = audioCard(e, e.text);
    var list = el("div", "sprint-options");
    var chosen = null;
    e.options.forEach(function (o, i) {
      var b = el("button", "sprint-opt", "<span class=\"key\">" + (i + 1) + "</span><span lang=\"" + esc(e.to_meaning ? ML : TL) + "\">" + esc(o) + "</span>");
      b.type = "button"; b.dataset.value = o;
      b.addEventListener("click", function () {
        pick(list.querySelectorAll(".sprint-opt"), b); chosen = o; Sound.play("tap"); setReady(true);
        if (!e.to_meaning) sayTap(o);
      });
      list.appendChild(b);
    });
    stage.insertBefore(list, audio.cant);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      key: function (n) { var b = list.querySelectorAll(".sprint-opt")[n - 1]; if (b) b.click(); },
      submit: function () {
        if (chosen === null) return;
        setReady(false);
        gradeSprint(e.id, chosen, 0).then(function (res) {
          list.querySelectorAll(".sprint-opt").forEach(function (x) {
            x.disabled = true;
            if (x.dataset.value === res.correct_answer) mark(x, "right");
            else if (x.dataset.value === chosen && !res.correct) mark(x, "wrong");
          });
          showResult(res, e.to_meaning ? { say: e.text, sayShow: true } : { say: res.correct_answer });
        }).catch(fail);
      }
    };
  }

  // Select the matching audio: a meaning is shown; the options are spoken, not written.
  function renderHearWhich(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.select_audio'))));
    stage.appendChild(el("p", "sprint-prompt en", esc(e.prompt)));
    stage.appendChild(el("p", "sprint-sub", esc(t('sprint.js.tap_each_speaker'))));
    var list = el("div", "sprint-hear-opts");
    var chosen = null;
    var buttons = e.options.map(function (o, i) {
      var b = el("button", "sprint-hear-opt", "<span class=\"key\">" + (i + 1) + "</span><span class=\"spk\">🔊</span>");
      b.type = "button"; b.dataset.value = o;
      b.addEventListener("click", function () {
        pick(list.querySelectorAll(".sprint-hear-opt"), b); chosen = o; setReady(true);
        say(o, { force: true, onstart: function () { b.classList.add("saying"); }, onend: function () { b.classList.remove("saying"); } });
      });
      list.appendChild(b);
      return b;
    });
    stage.appendChild(list);
    var cant = el("button", "sprint-link sprint-cant", esc(t('sprint.js.cant_listen'))); cant.type = "button";
    cant.addEventListener("click", function () { Sp.muteFor(60 * 60 * 1000); Audio.paint(); next(); });
    stage.appendChild(cant);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return {
      key: function (n) { var b = buttons[n - 1]; if (b) b.click(); },
      submit: function () {
        if (chosen === null) return;
        setReady(false);
        buttons.forEach(function (x) { x.disabled = true; });
        gradeSprint(e.id, chosen, 0).then(function (res) {
          // Once graded, the right speaker (and a wrong pick) show their word.
          buttons.forEach(function (x) {
            var reveal = x.dataset.value === res.correct_answer ? "right" : (x.dataset.value === chosen && !res.correct) ? "wrong" : "";
            if (!reveal) return;
            mark(x, reveal);
            x.querySelector(".spk").insertAdjacentHTML("afterend", "<span class=\"word\" lang=\"" + esc(TL) + "\">" + esc(x.dataset.value) + "</span>");
          });
          showResult(res, { say: res.correct_answer });
        }).catch(fail);
      }
    };
  }

  // Listen and match: speakers on the left, the written terms on the right.
  function renderHearMatch(e) {
    stage.innerHTML = lessonTag(e);
    stage.appendChild(el("p", "sprint-kicker", esc(t('sprint.js.listen_match'))));
    var board = el("div", "sprint-match");
    var left = el("div", "col"), right = el("div", "col");
    board.appendChild(left); board.appendChild(right);
    stage.appendChild(board);
    var pairs = e.terms.map(function (t, i) { return { id: i, term: t }; });
    var L = shuffle(pairs.slice()), R = shuffle(pairs.slice());
    var sel = { audio: null, word: null };
    var known = [], missed = {}, done = 0;
    var ctl = {
      submit: function () {
        setReady(false);
        gradeSprint(e.id, { known: known.filter(function (w) { return !missed[w]; }), missed: Object.keys(missed) }, 0)
          .then(function (res) {
            var m = Object.keys(missed);
            showResult(res, { noKey: true, slips: m.length ? t('sprint.js.mixed_up', { list: m.join(", ") }) : "" });
          }).catch(fail);
      }
    };
    function tile(p, side) {
      var b = el("button", "sprint-tile match" + (side === "audio" ? " audio" : ""), side === "audio" ? "🔊" : esc(p.term));
      b.type = "button"; if (side === "word") b.lang = TL; else b.setAttribute("aria-label", t('sprint.js.play_n', { n: L.indexOf(p) + 1 }));
      b.dataset.value = p.term; // what it says, like the speakers of "Select the matching audio"
      b.addEventListener("click", function () {
        if (b.classList.contains("done")) return;
        if (side === "audio") say(p.term, { force: true, onstart: function () { b.classList.add("saying"); }, onend: function () { b.classList.remove("saying"); } });
        var col = side === "audio" ? left : right;
        pick(col.querySelectorAll(".sprint-tile"), b); sel[side] = { p: p, b: b };
        Sound.play("tap");
        if (sel.audio && sel.word) {
          var a = sel.audio, c = sel.word; sel = { audio: null, word: null };
          if (a.p.id === c.p.id) {
            pick([a.b, c.b]);
            mark(a.b, "done"); mark(c.b, "done");
            a.b.innerHTML = "<span lang=\"" + esc(TL) + "\">" + esc(a.p.term) + "</span>";
            known.push(a.p.term); done += 1; buzz(15); Sound.play("ok");
            if (done === pairs.length) { setReady(true); ctl.submit(); }
          } else {
            missed[a.p.term] = true;
            pick([a.b, c.b]);
            [a.b, c.b].forEach(function (x) { x.classList.remove("shake"); void x.offsetWidth; x.classList.add("shake"); });
            Sound.play("bad"); buzz(60);
          }
        }
      });
      return b;
    }
    L.forEach(function (p) { left.appendChild(tile(p, "audio")); });
    R.forEach(function (p) { right.appendChild(tile(p, "word")); });
    stage.appendChild(el("p", "sprint-sub", esc(t('sprint.js.tap_speaker_word'))));
    var cant = el("button", "sprint-link sprint-cant", esc(t('sprint.js.cant_listen'))); cant.type = "button";
    cant.addEventListener("click", function () { Sp.muteFor(60 * 60 * 1000); Audio.paint(); next(); });
    stage.appendChild(cant);
    setCheckLabel(t('sprint.js.check'));
    setReady(false);
    return ctl;
  }

  var renderers = { bite: renderBite, choice: renderChoice, tiles: renderTiles, "fill-bar": renderFillBar, "chord-spell": renderChordSpell, "staff-note": renderStaffNote, keyboard: renderKeyboard, sort: renderSort, spot: renderSpot, build: renderBuild, ladder: renderLadder, scale: renderLadder, pick: renderPick, match: renderMatch, clock: renderClock,
    "hear-build": renderHearBuild, "hear-gap": renderHearGap, "hear-pick": renderHearPick, "hear-which": renderHearWhich, "hear-match": renderHearMatch };

  function render(i) {
    var e = exercises[i];
    result.hidden = true;
    foot.hidden = false;
    setProgress();
    stage.scrollTop = 0;
    window.scrollTo(0, 0);
    if (Sp) Sp.hush();
    var r = renderers[e.kind];
    if (!r) { next(); return; }
    stage.classList.remove("in"); void stage.offsetWidth; stage.classList.add("in");
    // Which exercise is up, for tests (docs/testing.md): its kind and its id,
    // which names where it came from in the lesson (sort:<lesson>:<n>, a quiz question id, …).
    stage.dataset.kind = e.kind;
    stage.dataset.exerciseId = e.id || "";
    stage.dataset.pos = i;
    current = r(e);
  }

  // Listening exercises are skipped while audio is off (or the browser has
  // no voice for the language), as they are after "Can't listen now"; so
  // are music exercises that need sound (isEar).
  function next() {
    pos += 1;
    while (pos < exercises.length && ((isListening(exercises[pos]) && !listenable()) || (isEar(exercises[pos]) && !(Music && Music.on())))) pos += 1;
    if (pos >= exercises.length) finish(); else render(pos);
  }

  function fmtTime(ms) {
    var s = Math.round(ms / 1000);
    return Math.floor(s / 60) + ":" + ("0" + (s % 60)).slice(-2);
  }

  function confetti() {
    var box = document.getElementById("confetti");
    var colors = ["#c0392b", "#e3a21a", "#2e8b57", "#3b82f6", "#a855f7"];
    for (var i = 0; i < 60; i++) {
      var p = el("i");
      p.style.left = Math.random() * 100 + "%";
      p.style.background = colors[i % colors.length];
      p.style.animationDelay = (Math.random() * 1.2) + "s";
      p.style.animationDuration = (2.2 + Math.random() * 1.6) + "s";
      p.style.transform = "rotate(" + Math.random() * 360 + "deg)";
      box.appendChild(p);
    }
  }


  function finish() {
    if (finished) return;
    finished = true;
    if (timerHandle) clearInterval(timerHandle);
    var duration = Date.now() - started;
    // Emptied as well as hidden: innerText of a hidden element still returns its text.
    stage.hidden = true; stage.innerHTML = ""; foot.hidden = true; result.hidden = true;
    stage.dataset.kind = "done"; stage.dataset.exerciseId = "";
    pFill.style.width = "100%";
    var done = document.getElementById("done");
    done.hidden = false;
    done.dataset.correct = correct; done.dataset.answered = answered; // for tests: the score sent to the server
    var perfect = answered > 0 && correct === answered;
    var acc = answered ? Math.round((correct / answered) * 100) : 0;
    document.getElementById("done-acc").textContent = acc + "%";
    document.getElementById("done-time").textContent = fmtTime(duration);
    var title = document.getElementById("done-title"), emoji = document.getElementById("done-emoji");
    if (S.mode === "lightning") { title.textContent = t('sprint.js.n_in_seconds', { n: correct, secs: S.timed_secs || 60 }); emoji.textContent = correct >= 10 ? icon("xp", "⚡") : icon("effort", "🏃"); }
    else if (perfect) { title.textContent = t('sprint.js.flawless_title'); emoji.textContent = icon("flawless", "🌟"); }
    else if (acc >= 70) { title.textContent = t('sprint.js.session_complete'); emoji.textContent = icon("goal", "🎉"); }
    else { title.textContent = t('sprint.js.done_tomorrow'); emoji.textContent = icon("effort", "💪"); }
    var note = document.getElementById("done-note");
    note.textContent = bestCombo >= 3 ? t('sprint.js.best_streak', { n: bestCombo }) : "";
    post("/api/sprint/finish", { mode: S.mode, lesson_id: S.lesson_id, answered: answered, correct: correct, duration_ms: duration }).then(function (res) {
      done.dataset.saved = "true"; // for tests: the session is recorded
      document.getElementById("done-streak").textContent = res.streak;
      var goal = document.getElementById("done-goal");
      goal.dataset.points = res.today; // for tests: today's points as the server counts them
      goal.textContent = res.today >= res.goal ? t('sprint.js.goal_done', { n: res.today, goal: res.goal }) : t('sprint.js.daily_ring', { n: res.today, goal: res.goal });
      // The worn motivator's line (#568), drawn by the server; none if the session earned nothing.
      if (res.reward) note.insertAdjacentHTML("afterend", res.reward);
      if (S.mode === "lesson") {
        var st = document.getElementById("done-stars");
        st.hidden = false;
        st.textContent = "★★★".slice(0, res.stars) + "☆☆☆".slice(0, 3 - res.stars);
        var ps = res.pass;
        if (ps && ps.passed_now) note.textContent = t('sprint.js.lesson_passed') + " " + note.textContent;
        else if (ps && !ps.passed) note.textContent = t('sprint.js.score_to_pass', { pct: 70 }) + " " + note.textContent;
      }
      if (perfect || acc >= 90) confetti();
      Sound.play("win");
      buzz([40, 60, 40, 60, 120]);
    }).catch(function (err) {
      if (err.message === "course") return;
      note.textContent = t('sprint.js.save_failed', { error: err.message });
    });
  }

  btnCheck.addEventListener("click", function () { if (current && !btnCheck.disabled) current.submit(); });
  btnNext.addEventListener("click", function () { next(); });
  document.getElementById("done-again").addEventListener("click", function () { window.location.reload(); });

  document.addEventListener("keydown", function (ev) {
    if (finished) return;
    // An Enter that confirms an IME composition (Chinese, Japanese, Korean input) is not a submit.
    if (ev.isComposing || ev.keyCode === 229) return;
    if (ev.key === "Enter") {
      if (!result.hidden) { ev.preventDefault(); next(); }
      // Enter on a filled rung takes its form back (its own click), it does not check the ladder.
      else if (document.activeElement.classList.contains("sprint-rung")) return;
      else if (!btnCheck.disabled && document.activeElement.tagName !== "INPUT") { ev.preventDefault(); current.submit(); }
      else if (!btnCheck.disabled) { ev.preventDefault(); current.submit(); }
    } else if (/^[1-9]$/.test(ev.key) && result.hidden && current && current.key && document.activeElement.tagName !== "INPUT") {
      current.key(parseInt(ev.key, 10));
    }
  });

  // Lightning: a shrinking clock; when it runs out the session ends on the spot.
  if (S.timed_secs) {
    timerEl.hidden = false;
    timerEl.textContent = timeLeft;
    var overlay = el("div", "sprint-countdown", "<b>3</b>");
    document.querySelector(".phone").appendChild(overlay);
    var n = 3;
    var pre = setInterval(function () {
      n -= 1;
      if (n <= 0) { clearInterval(pre); overlay.remove(); startTimer(); }
      else { overlay.querySelector("b").textContent = n; Sound.play("tick"); }
    }, 700);
  }
  function startTimer() {
    timerHandle = setInterval(function () {
      timeLeft -= 1;
      timerEl.textContent = timeLeft;
      timerEl.classList.toggle("urgent", timeLeft <= 10);
      if (timeLeft <= 10 && timeLeft > 0) Sound.play("tick");
      if (timeLeft <= 0) { finish(); }
    }, 1000);
  }

  if (!exercises.length) {
    stage.innerHTML = "<p class=\"sprint-empty\">" + esc(t('sprint.js.nothing_yet')) + "</p>";
    foot.hidden = true;
  } else {
    pos = -1;
    next();
  }
})();
