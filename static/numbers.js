// Numbers by ear. The browser's speech synthesis says a random number in the
// course's term language (data-lang on the root); the learner types the digits.
// Nothing is dealt by the server: it only records each finished round for the
// dashboard and history (POST /api/games/numbers/finish).
(function () {
  "use strict";
  var root = document.getElementById("numbers");
  if (!root) return;
  var base = document.body.dataset.base || "";
  var lang = (root.dataset.lang || "").trim();
  var praise = root.dataset.praise || "Correct!";
  var LIMIT = parseInt(root.dataset.limit, 10) || 1000000000;
  var S = window.LearnSpeech || null; // static/speech.js
  var supported = !!(S && S.supported);

  var $ = function (id) { return document.getElementById(id); };
  var setup = $("n-setup"), card = $("n-card"), counters = $("n-counters"), done = $("n-done");
  var input = $("n-answer"), fb = $("n-feedback"), status = $("n-status"), speaker = $("n-speaker");
  var form = $("n-form"), checkBtn = $("n-check");
  var rangeSel = $("n-range"), customBox = $("n-custom"), minIn = $("n-min"), maxIn = $("n-max");
  var voiceSel = $("n-voice"), rateSel = $("n-rate"), countSel = $("n-count");

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function store(k, v) { try { localStorage.setItem("numbers:" + k, v); } catch (e) { /* private mode */ } }
  function load(k) { try { return localStorage.getItem("numbers:" + k); } catch (e) { return null; } }
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
  function fmt(n) {
    try { return n.toLocaleString(lang || undefined); } catch (e) { return String(n); }
  }

  // ---- voices -----------------------------------------------------------
  // The voice list and the quirks of speaking live in speech.js; here we
  // only bind the <select> and the "no voice" warning.
  var currentVoice = function () { return null; };
  if (!supported) {
    $("n-unsupported").hidden = false;
    $("n-start").disabled = true;
  } else {
    currentVoice = S.bindSelect(voiceSel, lang, root.dataset.label);
    S.onVoices(function () { $("n-novoice").hidden = !(S.anyVoices() && !S.voicesFor(lang).length); });
  }

  // ---- settings ---------------------------------------------------------
  var savedRange = load("range");
  if (savedRange) {
    var found = false;
    for (var i = 0; i < rangeSel.options.length; i++) if (rangeSel.options[i].value === savedRange) { rangeSel.selectedIndex = i; found = true; }
    if (!found && /^\d+,\d+$/.test(savedRange)) {
      rangeSel.value = "custom";
      minIn.value = savedRange.split(",")[0]; maxIn.value = savedRange.split(",")[1];
    }
  }
  if (load("rate")) rateSel.value = load("rate");
  if (load("count") !== null) countSel.value = load("count");
  function syncCustom() { customBox.hidden = rangeSel.value !== "custom"; }
  rangeSel.addEventListener("change", syncCustom);
  syncCustom();

  function range() {
    var lo, hi;
    if (rangeSel.value === "custom") {
      lo = parseInt(minIn.value, 10); hi = parseInt(maxIn.value, 10);
    } else {
      lo = parseInt(rangeSel.value.split(",")[0], 10); hi = parseInt(rangeSel.value.split(",")[1], 10);
    }
    if (isNaN(lo) || lo < 0) lo = 0;
    if (isNaN(hi) || hi > LIMIT) hi = LIMIT;
    if (hi <= lo) hi = lo + 1;
    if (hi > LIMIT) { hi = LIMIT; lo = LIMIT - 1; }
    return { min: lo, max: hi };
  }

  // ---- random numbers ---------------------------------------------------
  function rnd(lo, hi) { return lo + Math.floor(Math.random() * (hi - lo + 1)); }
  // A plain uniform draw over "0 to a million" is almost always six digits.
  // For wide ranges pick the number of digits first so short numbers still
  // come up; narrow ranges are uniform.
  function draw(r) {
    if (r.max < 1000 * Math.max(1, r.min)) return rnd(r.min, r.max);
    var lowDigits = String(r.min).length, highDigits = String(r.max).length;
    var d = rnd(lowDigits, highDigits);
    var lo = Math.max(r.min, d === 1 ? 0 : Math.pow(10, d - 1));
    var hi = Math.min(r.max, Math.pow(10, d) - 1);
    return hi < lo ? rnd(r.min, r.max) : rnd(lo, hi);
  }

  // ---- answers ----------------------------------------------------------
  // "1,234" "1.234" "1 234" -> 1234; "12k" -> 12000; "1.5m" -> 1500000.
  function parseAnswer(s) {
    s = String(s).toLowerCase().replace(/[\s,]/g, "");
    if (!s) return null;
    var mult = 1;
    if (/[km]$/.test(s)) {
      mult = s.slice(-1) === "k" ? 1000 : 1000000;
      s = s.slice(0, -1);
      if (!/^\d+(\.\d+)?$/.test(s)) return null;
      var v = Math.round(parseFloat(s) * mult);
      return isFinite(v) ? v : null;
    }
    s = s.replace(/\./g, "");
    if (!/^\d+$/.test(s)) return null;
    return parseInt(s, 10);
  }

  // ---- speech -----------------------------------------------------------
  function speak(n) {
    if (!supported) return;
    S.speak(String(n), {
      voice: currentVoice(), lang: lang, rate: parseFloat(rateSel.value) || 1,
      onstart: function () { speaker.classList.add("speaking"); },
      onend: function () { speaker.classList.remove("speaking"); },
      onerror: function (code) { status.textContent = t("play.js.numbers.audio_error", { code: code }); }
    });
  }

  // ---- the round --------------------------------------------------------
  var r = null, count = 0, current = null, answered = false, finished = false;
  var total = 0, correct = 0, streak = 0, best = 0, misses = [], startedAt = 0, advanceTimer = null;

  function counts() {
    $("n-streak").textContent = streak;
    $("n-best").textContent = best;
    $("n-score").textContent = correct + " / " + total + (count ? t("play.js.numbers.of_count", { n: count }) : "");
  }

  function next() {
    if (advanceTimer) { clearTimeout(advanceTimer); advanceTimer = null; }
    if (finished) return;
    if (count && total >= count) { finish(); return; }
    current = draw(r);
    answered = false;
    input.value = ""; input.disabled = false;
    fb.hidden = true; fb.innerHTML = "";
    status.textContent = t("play.js.numbers.listen");
    checkBtn.textContent = t("play.js.numbers.answer");
    input.focus();
    speak(current);
  }

  function settle(ok, typed, revealed) {
    answered = true;
    total += 1;
    if (ok) { correct += 1; streak += 1; if (streak > best) best = streak; }
    else { streak = 0; misses.push({ n: current, typed: typed, revealed: revealed }); }
    counts();
    input.disabled = true;
    var html = '<p class="verdict">' + (ok ? esc(praise) : revealed ? esc(t("play.js.numbers.number_was")) : esc(t("play.js.not_quite"))) + "</p>" +
      '<p class="big-number" lang="' + esc(lang) + '">' + esc(fmt(current)) + "</p>";
    if (!ok && !revealed) html += "<p>" + t("play.js.numbers.you_wrote_full", { typed: esc(typed) }) + "</p>";
    fb.className = "feedback " + (ok ? "ok" : "bad");
    fb.dataset.state = ok ? "right" : "wrong";
    fb.innerHTML = html; fb.hidden = false;
    var last = count && total >= count;
    checkBtn.textContent = last ? t("play.js.finish") : t("play.js.next");
    status.textContent = ok ? "" : t("play.js.numbers.press_r");
    if (ok) advanceTimer = setTimeout(function () { advanceTimer = null; next(); }, last ? 1200 : 900);
    else checkBtn.focus();
  }

  function check() {
    var raw = input.value;
    var n = parseAnswer(raw);
    if (n === null) { status.textContent = t("play.js.numbers.digits_hint"); input.focus(); return; }
    settle(n === current, raw.trim(), false);
  }

  function reveal() {
    if (answered || current === null) return;
    settle(false, "", true);
    speak(current);
  }

  function finish() {
    if (finished) return;
    finished = true;
    if (advanceTimer) { clearTimeout(advanceTimer); advanceTimer = null; }
    if (supported) S.hush();
    card.hidden = true;
    if (total > 0) {
      post("/api/games/numbers/finish", {
        min: r.min, max: r.max, total: total, correct: correct, best_streak: best,
        duration_ms: Math.round(performance.now() - startedAt)
      }).then(function (res) {
        // The worn motivator's line (#568), drawn by the server; none if the round earned nothing.
        done.querySelectorAll('[data-testid="motivator-reward"]').forEach(function (r) { r.remove(); });
        if (res && res.reward) $("n-done-score").insertAdjacentHTML("afterend", res.reward);
      }).catch(function () { /* stats only; never block the summary */ });
    }
    $("n-done-score").textContent = correct + " / " + total;
    var secs = ((performance.now() - startedAt) / 1000).toFixed(0);
    $("n-done-detail").textContent = t("play.js.numbers.done_detail", { pct: Math.round((correct / Math.max(1, total)) * 100), secs: secs, best: best });
    var ul = $("n-misses");
    ul.innerHTML = "";
    misses.forEach(function (m) {
      ul.insertAdjacentHTML("beforeend", '<li><button type="button" class="link" data-say="' + m.n + '" lang="' + esc(lang) + '">🔊 <strong>' + esc(fmt(m.n)) +
        "</strong></button><span class=\"meta\">" + (m.revealed ? esc(t("play.js.numbers.revealed")) : t("play.js.numbers.you_wrote", { typed: esc(m.typed) })) + "</span></li>");
    });
    done.hidden = false;
    window.scrollTo(0, 0);
  }

  function start() {
    r = range();
    count = parseInt(countSel.value, 10) || 0;
    store("range", r.min + "," + r.max); store("rate", rateSel.value); store("count", countSel.value);
    total = 0; correct = 0; streak = 0; best = 0; misses = []; finished = false; current = null;
    counts();
    $("n-total-label").textContent = count ? t("play.js.numbers.of", { n: count }) : t("play.js.numbers.heard");
    setup.hidden = true; done.hidden = true;
    counters.hidden = false; card.hidden = false;
    startedAt = performance.now();
    next();
  }

  setup.addEventListener("submit", function (ev) { ev.preventDefault(); start(); });
  form.addEventListener("submit", function (ev) { ev.preventDefault(); if (answered) next(); else check(); });
  $("n-replay").addEventListener("click", function () { if (current !== null) { speak(current); input.focus(); } });
  $("n-reveal").addEventListener("click", reveal);
  $("n-stop").addEventListener("click", function () {
    if (total > 0) finish();
    else { finished = true; if (supported) S.hush(); card.hidden = true; counters.hidden = true; setup.hidden = false; }
  });
  $("n-again").addEventListener("click", start);
  $("n-settings").addEventListener("click", function () { done.hidden = true; counters.hidden = true; setup.hidden = false; });
  done.addEventListener("click", function (ev) {
    var b = ev.target.closest("[data-say]");
    if (b) speak(parseInt(b.dataset.say, 10));
  });
  document.addEventListener("keydown", function (ev) {
    if (card.hidden || ev.ctrlKey || ev.metaKey || ev.altKey) return;
    if (ev.key === "r" || ev.key === "R") { ev.preventDefault(); if (current !== null) speak(current); }
    else if (ev.key === "q" || ev.key === "Q") { ev.preventDefault(); reveal(); }
  });
})();
