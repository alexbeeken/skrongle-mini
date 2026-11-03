// Listening practice: the browser reads a sentence aloud (speech.js), the
// learner types it, and the server checks it word by word
// (POST /api/listening/check). A finished round is recorded for the
// overview and history (POST /api/listening/finish).
(function () {
  "use strict";
  var root = document.getElementById("listening");
  if (!root || !window.LearnSpeech) return;
  var S = window.LearnSpeech;
  var base = document.body.dataset.base || "";
  var lang = (root.dataset.lang || "").trim();
  var praise = root.dataset.praise || t('listening.js.correct');
  var board = root.dataset.board || "all";
  var items = [];
  try { items = JSON.parse(document.getElementById("l-items").textContent || "[]"); } catch (e) { items = []; }

  var $ = function (id) { return document.getElementById(id); };
  var setup = $("l-setup"), card = $("l-card"), done = $("l-done");
  var input = $("l-answer"), fb = $("l-feedback"), status = $("l-status"), speaker = $("l-speaker");
  var form = $("l-form"), checkBtn = $("l-check"), crumbs = $("l-crumbs");
  var voiceSel = $("l-voice"), rateSel = $("l-rate");
  var posEl = $("l-pos"), barEl = $("l-bar");

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function store(k, v) { try { localStorage.setItem("listening:" + k, v); } catch (e) { /* private mode */ } }
  function load(k) { try { return localStorage.getItem("listening:" + k); } catch (e) { return null; } }
  function post(path, body) {
    return fetch(base + path, {
      method: "POST", credentials: "same-origin",
      headers: { "Content-Type": "application/json" }, body: JSON.stringify(body)
    }).then(function (r) {
      if (r.status === 401) { window.location.href = base + "/login"; throw new Error("login"); }
      if (r.status === 410) { window.location.href = base + "/courses"; throw new Error("course"); }
      if (!r.ok) return r.text().then(function (t) { var m = t; try { m = JSON.parse(t).error || t; } catch (e) { /* html */ } throw new Error(m); });
      return r.json();
    });
  }

  // ---- voices -----------------------------------------------------------
  var currentVoice = function () { return null; };
  if (!S.supported) {
    $("l-unsupported").hidden = false;
    $("l-start").disabled = true;
  } else {
    currentVoice = S.bindSelect(voiceSel, lang, root.dataset.label);
    S.onVoices(function () { $("l-novoice").hidden = !(S.anyVoices() && !S.voicesFor(lang).length); });
  }
  if (load("rate")) rateSel.value = load("rate");
  rateSel.addEventListener("change", function () { store("rate", rateSel.value); });

  // The speed setting: a plain rate, or "phrases" / "words" for chunked
  // playback with pauses (see speech.js for why a rate alone is not enough).
  function speed(value) {
    if (value === "phrases") return { rate: 0.6, chunk: 3 };
    if (value === "words") return { rate: 0.6, chunk: 1, gap: 300 };
    return { rate: parseFloat(value) || 1 };
  }
  var SLOWER = { rate: 0.5, chunk: 2, gap: 400 };
  function speak(text, how) {
    if (!S.supported) return;
    var v = currentVoice();
    var h = how || speed(rateSel.value);
    S.speak(text, {
      voice: v, lang: lang, rate: h.rate, chunk: h.chunk, gap: h.gap,
      onstart: function () { speaker.classList.add("speaking"); },
      onend: function () { speaker.classList.remove("speaking"); },
      onerror: function (code) { status.textContent = t('listening.js.audio_failed', { code: code }); }
    });
  }

  // ---- the round --------------------------------------------------------
  var pos = -1, current = null, answered = false, finished = false;
  var correct = 0, misses = [], startedAt = 0, advanceTimer = null;
  var total = items.length;

  function progress() {
    posEl.textContent = Math.min(pos + 1, total);
    barEl.style.width = (total ? (Math.max(pos, 0) / total) * 100 : 0) + "%";
  }

  function next() {
    if (advanceTimer) { clearTimeout(advanceTimer); advanceTimer = null; }
    if (finished) return;
    pos += 1;
    if (pos >= total) { finish(); return; }
    current = items[pos];
    answered = false;
    // Which sentence is up, for tests: its lesson id and index (docs/testing.md).
    card.dataset.lesson = current.lesson; card.dataset.index = current.index;
    progress();
    crumbs.innerHTML = '<a href="' + base + "/lessons/" + esc(current.slug) + '" target="_blank" rel="noopener">' + esc(current.title) + "</a>";
    input.value = ""; input.disabled = false;
    fb.hidden = true; fb.innerHTML = "";
    status.textContent = t('listening.js.listen_status');
    checkBtn.textContent = t('listening.js.check');
    input.focus();
    speak(current.text);
  }

  function words(list, side) {
    return list.map(function (w) {
      return '<span class="w ' + esc(w.mark) + '">' + esc(w.text) + "</span>";
    }).join(" ");
  }

  function render(res, revealed) {
    var ok = res.correct;
    var html = '<p class="verdict">' + (ok ? esc(praise) : revealed ? esc(t('listening.js.sentence_was')) : res.accents_only ? esc(t('listening.js.almost_accents')) : esc(t('listening.js.not_quite'))) + "</p>";
    html += '<p class="dictation" lang="' + esc(lang) + '">' + words(res.expected) + "</p>";
    if (!ok && !revealed && res.typed.length) html += '<p class="dictation typed" lang="' + esc(lang) + '"><span class="muted small">' + esc(t('listening.js.you_wrote')) + "</span> " + words(res.typed) + "</p>";
    if (res.meaning) html += '<p class="meaning">' + esc(res.meaning) + "</p>";
    if (res.note) html += '<p class="note">' + (res.note.indexOf("New word:") === 0 ? "✦ " : "") + esc(res.note) + "</p>";
    if (!ok) html += '<p class="muted small">' + esc(t('listening.js.words_right', { right: res.words_right, total: res.expected.length })) + "</p>";
    fb.className = "feedback " + (ok ? "ok" : "bad");
    fb.dataset.state = ok ? "right" : "wrong";
    fb.innerHTML = html; fb.hidden = false;
  }

  function settle(res, revealed) {
    answered = true;
    if (res.correct) correct += 1;
    else misses.push({ text: res.text, meaning: res.meaning, revealed: revealed, typed: input.value.trim() });
    input.disabled = true;
    render(res, revealed);
    var last = pos + 1 >= total;
    checkBtn.textContent = last ? t('listening.js.finish') : t('listening.js.next');
    status.textContent = res.correct ? "" : t('listening.js.press_r_s');
    if (res.correct) advanceTimer = setTimeout(function () { advanceTimer = null; next(); }, last ? 1500 : 1300);
    else checkBtn.focus();
  }

  function check(revealed) {
    if (answered || !current) return;
    var answer = revealed ? "" : input.value;
    if (!revealed && answer.trim() === "") { status.textContent = t('listening.js.type_or_reveal'); input.focus(); return; }
    checkBtn.disabled = true;
    post("/api/listening/check", { lesson: current.lesson, index: current.index, answer: answer })
      .then(function (res) { checkBtn.disabled = false; settle(res, revealed); if (revealed) speak(res.text); })
      .catch(function (err) {
        checkBtn.disabled = false;
        fb.className = "feedback bad";
        fb.textContent = t('listening.js.check_failed', { error: err.message });
        fb.hidden = false;
      });
  }

  function finish() {
    if (finished) return;
    finished = true;
    if (advanceTimer) { clearTimeout(advanceTimer); advanceTimer = null; }
    S.hush();
    card.hidden = true;
    barEl.style.width = "100%";
    posEl.textContent = total;
    var answeredCount = correct + misses.length;
    if (answeredCount > 0) {
      post("/api/listening/finish", {
        board: board, total: answeredCount, correct: correct, duration_ms: Math.round(performance.now() - startedAt)
      }).then(function (res) {
        // The worn motivator's line (#568), drawn by the server.
        if (res && res.reward) $("l-done-score").insertAdjacentHTML("afterend", res.reward);
      }).catch(function () { /* stats only; never block the summary */ });
    }
    $("l-done-score").textContent = correct + " / " + answeredCount;
    var secs = ((performance.now() - startedAt) / 1000).toFixed(0);
    $("l-done-detail").textContent = t('listening.js.done_detail', { pct: Math.round((correct / Math.max(1, answeredCount)) * 100), secs: secs });
    var ul = $("l-misses");
    ul.innerHTML = "";
    misses.forEach(function (m) {
      ul.insertAdjacentHTML("beforeend", '<li><button type="button" class="link" data-say="' + esc(m.text) + '" lang="' + esc(lang) + '">🔊 <strong>' + esc(m.text) +
        "</strong></button><span class=\"meta\">" + esc(m.meaning || "") + (m.revealed ? esc(t('listening.js.revealed')) : m.typed ? esc(t('listening.js.you_wrote_quote', { typed: m.typed })) : "") + "</span></li>");
    });
    done.hidden = false;
    window.scrollTo(0, 0);
  }

  function start() {
    store("rate", rateSel.value);
    pos = -1; correct = 0; misses = []; finished = false; current = null;
    setup.hidden = true; done.hidden = true; card.hidden = false;
    done.querySelectorAll('[data-testid="motivator-reward"]').forEach(function (r) { r.remove(); });
    startedAt = performance.now();
    next();
  }

  // Accent helper buttons.
  root.querySelectorAll(".accents button").forEach(function (b) {
    b.addEventListener("click", function () {
      if (input.disabled) return;
      var s = input.selectionStart || 0, e = input.selectionEnd || 0;
      input.value = input.value.slice(0, s) + b.dataset.ch + input.value.slice(e);
      input.focus();
      input.setSelectionRange(s + 1, s + 1);
    });
  });

  setup.addEventListener("submit", function (ev) { ev.preventDefault(); start(); });
  form.addEventListener("submit", function (ev) { ev.preventDefault(); if (answered) next(); else check(false); });
  $("l-replay").addEventListener("click", function () { if (current) { speak(current.text); if (!answered) input.focus(); } });
  $("l-slow").addEventListener("click", function () { if (current) { speak(current.text, SLOWER); if (!answered) input.focus(); } });
  $("l-reveal").addEventListener("click", function () { check(true); });
  $("l-stop").addEventListener("click", function () {
    if (correct + misses.length > 0) finish();
    else { finished = true; S.hush(); card.hidden = true; setup.hidden = false; }
  });
  done.addEventListener("click", function (ev) {
    var b = ev.target.closest("[data-say]");
    if (b) speak(b.dataset.say);
  });
  // Letters go into the answer box, so the shortcuts need Alt while typing
  // (plain R / S / Q work once the answer is checked or the box is not focused).
  document.addEventListener("keydown", function (ev) {
    if (card.hidden || ev.ctrlKey || ev.metaKey) return;
    var typing = document.activeElement === input && !input.disabled;
    if (typing && !ev.altKey) return;
    var code = ev.code || "";
    if (code === "KeyR") { ev.preventDefault(); if (current) speak(current.text); }
    else if (code === "KeyS") { ev.preventDefault(); if (current) speak(current.text, SLOWER); }
    else if (code === "KeyQ") { ev.preventDefault(); check(true); }
  });
})();
