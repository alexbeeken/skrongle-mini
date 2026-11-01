// Date Dash: weekday and month names against the clock. The course's names
// come from the manifest's `calendar` (data-calendar on the root, JSON); the
// learner's own names come from the browser (Intl, in the meanings' language),
// so nothing here is specific to one language. Prompts are dealt here; the
// server only records each finished round (POST /api/games/dates/finish).
(function () {
  "use strict";
  var root = document.getElementById("dates");
  if (!root) return;
  var base = document.body.dataset.base || "";
  var cal;
  try { cal = JSON.parse(root.dataset.calendar); } catch (e) { return; }
  if (!cal || !cal.days || cal.days.length !== 7 || !cal.months || cal.months.length !== 12) return;
  var lang = (root.dataset.lang || "").trim();
  var meaningLang = (root.dataset.meaningLang || "").trim() || "en";
  var praise = root.dataset.praise || "Correct!";
  var bestEver = parseInt(root.dataset.best, 10) || 0;
  var S = window.LearnSpeech || null; // static/speech.js
  var speechOk = !!(S && S.supported && lang);

  var $ = function (id) { return document.getElementById(id); };
  var setup = $("dt-setup"), hud = $("dt-hud"), card = $("dt-card"), done = $("dt-done");
  var promptEl = $("dt-prompt"), tilesEl = $("dt-tiles"), taskEl = $("dt-task"), replayBtn = $("dt-replay");
  var earBox = $("dt-ear");

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function store(k, v) { try { localStorage.setItem("dates:" + k, v); } catch (e) { /* private mode */ } }
  function load(k) { try { return localStorage.getItem("dates:" + k); } catch (e) { return null; } }
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
  function rnd(n) { return Math.floor(Math.random() * n); }
  function shuffle(a) { for (var i = a.length - 1; i > 0; i--) { var j = rnd(i + 1); var t = a[i]; a[i] = a[j]; a[j] = t; } return a; }
  function cap(s) { return s ? s.charAt(0).toUpperCase() + s.slice(1) : s; }
  function pad(n) { return (n < 10 ? "0" : "") + n; }

  // ---- names --------------------------------------------------------------
  // Monday-first weekday index of a Date; cal.days is Monday first too.
  function wd(date) { return (date.getDay() + 6) % 7; }
  function intl(opts) {
    try { return new Intl.DateTimeFormat(meaningLang, opts); } catch (e) { return new Intl.DateTimeFormat("en", opts); }
  }
  var fmtWeekday = intl({ weekday: "long" }), fmtMonth = intl({ month: "long" }), fmtFull = intl({ weekday: "long", year: "numeric", month: "long", day: "numeric" });
  var fmtMonthYear = intl({ month: "long", year: "numeric" });
  var meaning = { day: [], month: [] };
  for (var i = 0; i < 7; i++) meaning.day.push(cap(fmtWeekday.format(new Date(2024, 0, 1 + i))));   // 1 Jan 2024 was a Monday
  for (var m = 0; m < 12; m++) meaning.month.push(cap(fmtMonth.format(new Date(2024, m, 15))));
  var term = { day: cal.days.slice(), month: cal.months.slice() };
  var format = cal.format || "{day}, {d} {month} {year}";
  function writeDate(d) {
    return format.replace("{day}", term.day[wd(d)]).replace("{month}", term.month[d.getMonth()])
      .replace("{d}", String(d.getDate())).replace("{year}", String(d.getFullYear()));
  }
  // The sentence frame with blanks: "___, 9 de ___ de 2026"; filled parts are shown.
  function frame(d, dayDone, monthDone) {
    var slot = function (ok, text, cls) { return ok ? '<b class="filled">' + esc(text) + "</b>" : '<span class="blank ' + cls + '">___</span>'; };
    return esc(format).replace("{day}", slot(dayDone, term.day[wd(d)], "day")).replace("{month}", slot(monthDone, term.month[d.getMonth()], "month"))
      .replace("{d}", "<b>" + d.getDate() + "</b>").replace("{year}", "<b>" + d.getFullYear() + "</b>");
  }
  function calCard(d) {
    return '<div class="cal-page" data-date="' + d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) + '"><div class="cal-top">' + esc(cap(fmtMonthYear.format(d))) + '</div><div class="cal-num">' + d.getDate() +
      '</div><div class="cal-wd">' + esc(cap(fmtWeekday.format(d))) + "</div></div>";
  }

  // ---- today ----------------------------------------------------------------
  var today = new Date();
  $("dt-today-text").textContent = writeDate(today);
  $("dt-today").hidden = false;

  // ---- speech ---------------------------------------------------------------
  // Voices and the quirks of speaking live in speech.js; the best voice for
  // the course's language is used as is (no picker on this page).
  function matching() { return speechOk ? S.voicesFor(lang) : []; }
  var hasVoice = false;
  function syncVoices() {
    hasVoice = matching().length > 0;
    earBox.disabled = !hasVoice;
    $("dt-ear-note").textContent = hasVoice ? "" : speechOk ? t("play.js.dates.no_voice", { lang: root.dataset.label || lang }) : t("play.js.dates.cannot_speak");
    $("dt-today-say").hidden = !hasVoice;
    if (!hasVoice) earBox.checked = false;
  }
  if (speechOk) S.onVoices(syncVoices); else syncVoices();
  var lastSpoken = "";
  function speak(text) {
    if (!hasVoice) return;
    lastSpoken = text;
    S.speak(text, { voice: matching()[0], lang: lang });
  }
  function hush() { if (speechOk) S.hush(); }
  $("dt-today-say").addEventListener("click", function () { speak(writeDate(today)); });

  // ---- settings -------------------------------------------------------------
  function radio(name) { var el = setup.querySelector('input[name="' + name + '"]:checked'); return el ? el.value : ""; }
  function setRadio(name, value) { var el = setup.querySelector('input[name="' + name + '"][value="' + value + '"]'); if (el) el.checked = true; }
  if (load("mode")) setRadio("mode", load("mode"));
  if (load("len") !== null) setRadio("len", load("len"));
  if (load("ear") === "1") earBox.checked = true;

  // ---- adaptive weights ---------------------------------------------------
  // Each day and month has a weight; misses raise it, quick right answers
  // lower it, so the round keeps coming back to what is not yet automatic.
  var weight = { day: [], month: [] };
  var record = { day: [], month: [] };   // { wrong, rt: [ms] }
  function resetWeights() {
    ["day", "month"].forEach(function (k) {
      weight[k] = term[k].map(function () { return 1; });
      record[k] = term[k].map(function () { return { wrong: 0, rt: [] }; });
    });
  }
  var lastPick = { day: -1, month: -1 };
  function pick(kind) {
    var w = weight[kind], sum = 0, i;
    for (i = 0; i < w.length; i++) if (i !== lastPick[kind]) sum += w[i];
    var r = Math.random() * sum;
    for (i = 0; i < w.length; i++) {
      if (i === lastPick[kind]) continue;
      r -= w[i];
      if (r <= 0) break;
    }
    if (i >= w.length) i = w.length - 1;
    lastPick[kind] = i;
    return i;
  }
  function learn(kind, i, ok, rt) {
    if (kind !== "day" && kind !== "month") return;
    var rec = record[kind][i];
    if (ok) { rec.rt.push(rt); weight[kind][i] = Math.max(0.25, weight[kind][i] * (rt > 2500 ? 1.3 : 0.6)); }
    else { rec.wrong += 1; weight[kind][i] = Math.min(8, weight[kind][i] * 2.5); }
  }

  // ---- prompts --------------------------------------------------------------
  // A prompt: what to show (html / spoken text), the tiles, which tile is
  // right, and the item it trains. Full dates are two steps (day, then month).
  function randomDate() {
    var d = new Date(today.getFullYear() - 2 + rnd(5), rnd(12), 1 + rnd(28));
    return d;
  }
  function tilesFor(kind, side) {
    // side: "term" (answer in the course language) or "meaning"
    var names = side === "term" ? term[kind] : meaning[kind];
    return names.map(function (n, i) { return { i: i, text: n, lang: side === "term" ? lang : meaningLang }; });
  }
  function wordPrompt(kind, dir) {
    // dir: "to-term" (meaning shown, term tapped), "to-meaning", "ear" (term spoken, meaning tapped)
    var i = pick(kind);
    var what = kind === "day" ? t("play.js.dates.day") : t("play.js.dates.month");
    if (dir === "to-term") {
      return { kind: kind, item: i, task: t("play.js.dates.tap_the", { what: what, lang: root.dataset.label || t("play.js.course_language") }),
        html: '<p class="dash-word" lang="' + esc(meaningLang) + '">' + esc(meaning[kind][i]) + "</p>", tiles: shuffle(tilesFor(kind, "term")), answer: i };
    }
    if (dir === "ear") {
      return { kind: kind, item: i, task: t("play.js.dates.listen_tap", { what: what }), speak: term[kind][i],
        html: '<p class="dash-word dash-ear" aria-label="' + esc(t("play.js.dates.spoken")) + '">🔊</p>', tiles: shuffle(tilesFor(kind, "meaning")), answer: i };
    }
    return { kind: kind, item: i, task: t("play.js.dates.what_is", { what: what }),
      html: '<p class="dash-word" lang="' + esc(lang) + '">' + esc(term[kind][i]) + "</p>", tiles: shuffle(tilesFor(kind, "meaning")), answer: i };
  }
  function neighbourPrompt(kind) {
    var i = pick(kind), n = term[kind].length;
    var after = Math.random() < 0.5;
    var answer = (i + (after ? 1 : n - 1)) % n;
    var what = kind === "day" ? t("play.js.dates.day") : t("play.js.dates.month");
    return { kind: kind, item: answer, task: kind === "day" ? t("play.js.dates.in_order_day") : t("play.js.dates.in_order_month"),
      html: '<p class="dash-word small"><span class="muted">' + esc(after ? t("play.js.dates.after", { what: what }) : t("play.js.dates.before", { what: what })) + '</span><br><span lang="' + esc(lang) + '">' + esc(term[kind][i]) + "</span></p>",
      tiles: shuffle(tilesFor(kind, "term")), answer: answer };
  }
  function datePrompt(d) {
    // Step 1 asks for the weekday, step 2 for the month; the frame fills as you go.
    return { kind: "date", date: d, step: 0, task: t("play.js.dates.build_date"),
      html: calCard(d) + '<p class="dash-frame" lang="' + esc(lang) + '">' + frame(d, false, false) + "</p>",
      tiles: shuffle(tilesFor("day", "term")), answer: wd(d) };
  }
  function dateStep2(p) {
    var d = p.date;
    return { kind: "date", date: d, step: 1, task: t("play.js.dates.now_month"),
      html: calCard(d) + '<p class="dash-frame" lang="' + esc(lang) + '">' + frame(d, true, false) + "</p>",
      tiles: shuffle(tilesFor("month", "term")), answer: d.getMonth() };
  }
  function dateEarPrompt() {
    // Hear the whole date; tap the matching calendar page among four. The
    // decoys differ in exactly one part: weekday, month, day number or year.
    var d = randomDate();
    var decoys = [], seen = {};
    seen[+d] = true;
    var tries = 0;
    while (decoys.length < 3 && tries++ < 50) {
      var c = new Date(d.getTime()), k = decoys.length;
      if (k === 0) c.setDate(c.getDate() + 1 + rnd(5));                       // another weekday
      else if (k === 1) c.setMonth((c.getMonth() + 1 + rnd(11)) % 12);         // another month
      else c.setFullYear(c.getFullYear() + (Math.random() < 0.5 ? -10 : 10) + rnd(3) - 1);   // another year
      if (c.getDate() !== d.getDate() && k !== 0) c.setDate(d.getDate());
      if (!seen[+c]) { seen[+c] = true; decoys.push(c); }
    }
    var all = shuffle([d].concat(decoys));
    return { kind: "date-ear", date: d, task: t("play.js.dates.listen_tap_page"), speak: writeDate(d),
      html: '<p class="dash-word dash-ear" aria-label="' + esc(t("play.js.dates.spoken")) + '">🔊</p>',
      tiles: all.map(function (c) { return { i: +c, html: calCard(c), cls: "cal" }; }), answer: +d };
  }
  function nextPrompt() {
    var kinds = [];
    var ear = earBox.checked && hasVoice;
    function words(kind) {
      kinds.push([kind, "to-term", 4], [kind, "to-meaning", 2], [kind, "next", 1.5]);
      if (ear) kinds.push([kind, "ear", 2]);
    }
    if (mode === "days" || mode === "both" || mode === "all") words("day");
    if (mode === "months" || mode === "both" || mode === "all") words("month");
    if (mode === "dates" || mode === "all") { kinds.push(["date", "", mode === "dates" ? 6 : 3]); if (ear) kinds.push(["date-ear", "", mode === "dates" ? 3 : 1.5]); }
    var sum = kinds.reduce(function (a, k) { return a + k[2]; }, 0), r = Math.random() * sum, pickK = kinds[0];
    for (var i = 0; i < kinds.length; i++) { r -= kinds[i][2]; if (r <= 0) { pickK = kinds[i]; break; } }
    if (pickK[0] === "date") {
      // The first full date of a round is today's: "what's the date today?"
      var d = todayDone ? randomDate() : today; todayDone = true;
      return datePrompt(d);
    }
    if (pickK[0] === "date-ear") return dateEarPrompt();
    if (pickK[1] === "next") return neighbourPrompt(pickK[0]);
    return wordPrompt(pickK[0], pickK[1]);
  }

  // ---- the round --------------------------------------------------------------
  var mode = "both", seconds = 60, FIXED = 20;
  var score = 0, total = 0, correct = 0, streak = 0, bestCombo = 0, startedAt = 0, finished = false, todayDone = false;
  var current = null, shownAt = 0, locked = false, timerId = null, advanceTimer = null;
  var misses = [], rts = [];
  var KEYS = "1234567890-=";

  function multiplier() { return 1 + Math.min(3, Math.floor(streak / 4)); }
  function hudUpdate() {
    $("dt-score").textContent = score;
    $("dt-combo").textContent = "×" + multiplier();
    $("dt-streak").textContent = streak;
    $("dt-combo").parentNode.classList.toggle("hot", multiplier() > 1);
  }
  function tick() {
    var elapsed = (performance.now() - startedAt) / 1000;
    if (seconds > 0) {
      var left = Math.max(0, seconds - elapsed);
      $("dt-time").textContent = Math.ceil(left);
      $("dt-timer-fill").style.width = (100 * left / seconds) + "%";
      $("dt-timer-fill").classList.toggle("low", left < 10);
      if (left <= 0) finish();
    } else {
      $("dt-time").textContent = total + " / " + FIXED;
      $("dt-timer-fill").style.width = (100 * total / FIXED) + "%";
    }
  }

  function render(p) {
    current = p;
    locked = false;
    taskEl.textContent = p.task;
    promptEl.innerHTML = p.html;
    var html = "";
    p.tiles.forEach(function (t, idx) {
      html += '<button type="button" class="dash-tile' + (t.cls ? " " + t.cls : "") + '" data-i="' + esc(String(t.i)) + '"' + (t.lang ? ' lang="' + esc(t.lang) + '"' : "") + ">" +
        (idx < KEYS.length ? '<kbd class="key">' + KEYS[idx] + "</kbd>" : "") + (t.html || esc(t.text)) + "</button>";
    });
    tilesEl.innerHTML = html;
    tilesEl.className = "dash-tiles " + (p.kind === "date-ear" ? "cols-2" : p.tiles.length > 7 ? "cols-3" : "cols-days");
    replayBtn.hidden = !p.speak;
    if (p.speak) speak(p.speak);
    shownAt = performance.now();
  }

  function next() {
    if (advanceTimer) { clearTimeout(advanceTimer); advanceTimer = null; }
    if (finished) return;
    if (seconds === 0 && total >= FIXED) { finish(); return; }
    render(nextPrompt());
  }

  function answer(value, tile) {
    if (locked || !current || finished) return;
    locked = true;
    var rt = performance.now() - shownAt;
    var ok = String(value) === String(current.answer);
    total += 1;
    var pts = 0;
    if (ok) {
      correct += 1; streak += 1; if (streak > bestCombo) bestCombo = streak;
      var bonus = Math.max(0, Math.min(100, Math.round(100 * (1 - (rt - 400) / 3000))));
      pts = (100 + bonus) * multiplier();
      score += pts;
      rts.push(rt);
    } else {
      streak = 0;
    }
    var itemKind = current.kind === "date" ? (current.step === 0 ? "day" : "month") : current.kind;
    var itemIdx = current.kind === "date" ? current.answer : current.item;
    learn(itemKind, itemIdx, ok, rt);
    if (!ok) misses.push({ kind: itemKind, i: itemIdx, date: current.date });
    hudUpdate();
    // Paint the verdict on the tiles, then move on without waiting for a click.
    var tiles = tilesEl.querySelectorAll(".dash-tile");
    for (var i = 0; i < tiles.length; i++) {
      var isAnswer = tiles[i].dataset.i === String(current.answer);
      var st = isAnswer ? "right" : tiles[i] === tile ? "wrong" : "dim";
      tiles[i].classList.add(st); tiles[i].dataset.state = st;
    }
    if (ok && tile) {
      var pop = document.createElement("span");
      pop.className = "dash-pop"; pop.textContent = "+" + pts + (multiplier() > 1 ? " ×" + multiplier() : "");
      tile.appendChild(pop);
    }
    var flash = document.createElement("p");
    flash.className = "dash-flash " + (ok ? "ok" : "bad");
    if (ok) flash.textContent = praise;
    else if (current.kind === "date-ear") flash.innerHTML = t("play.js.dates.it_was_html", { lang: esc(lang), date: esc(writeDate(current.date)) });
    else flash.innerHTML = '<b lang="' + esc(lang) + '">' + esc(term[itemKind][itemIdx]) + "</b> = " + esc(meaning[itemKind][itemIdx]);
    promptEl.appendChild(flash);
    if (current.kind === "date" && current.step === 0) {
      var p2 = dateStep2(current);
      advanceTimer = setTimeout(function () { advanceTimer = null; if (!finished) render(p2); }, ok ? 350 : 1300);
    } else {
      advanceTimer = setTimeout(function () { advanceTimer = null; next(); }, ok ? 350 : 1300);
    }
    if (seconds === 0) tick();
  }

  function finish() {
    if (finished) return;
    finished = true;
    if (timerId) { clearInterval(timerId); timerId = null; }
    if (advanceTimer) { clearTimeout(advanceTimer); advanceTimer = null; }
    hush();
    card.hidden = true; hud.hidden = true;
    var duration = Math.round(performance.now() - startedAt);
    var avg = rts.length ? Math.round(rts.reduce(function (a, b) { return a + b; }, 0) / rts.length / 100) / 10 : 0;
    $("dt-done-score").textContent = score;
    $("dt-done-title").textContent = score > bestEver && bestEver > 0 ? t("play.js.new_best") : t("play.js.round_complete");
    $("dt-done-detail").textContent = t("play.js.done_detail", { correct: correct, total: total, combo: bestCombo }) + (avg ? t("play.js.per_answer", { secs: avg }) : "");
    if (score > bestEver) bestEver = score;
    var ul = $("dt-review");
    ul.innerHTML = "";
    var seen = {};
    misses.forEach(function (mi) {
      var key = mi.kind + ":" + mi.i;
      if (seen[key]) return; seen[key] = true;
      var word = term[mi.kind] ? term[mi.kind][mi.i] : "", mn = meaning[mi.kind] ? meaning[mi.kind][mi.i] : "";
      if (mi.kind === "date-ear") { word = writeDate(mi.date); mn = cap(fmtFull.format(mi.date)); }
      ul.insertAdjacentHTML("beforeend", '<li><span class="miss"><strong lang="' + esc(lang) + '">' + esc(word) + "</strong> · " + esc(mn) +
        '</span><span class="meta">' + esc(t("play.js.dates.missed")) + "</span>" + (hasVoice ? '<button type="button" class="link" data-say="' + esc(word) + '">🔊</button>' : "") + "</li>");
    });
    // The three slowest items that were not missed outright: still worth a look.
    var slow = [];
    ["day", "month"].forEach(function (k) {
      record[k].forEach(function (r, i) {
        if (!r.rt.length || seen[k + ":" + i]) return;
        slow.push({ k: k, i: i, rt: r.rt.reduce(function (a, b) { return a + b; }, 0) / r.rt.length });
      });
    });
    slow.sort(function (a, b) { return b.rt - a.rt; }).slice(0, 3).forEach(function (sl) {
      if (sl.rt < 2000) return;
      ul.insertAdjacentHTML("beforeend", '<li><span class="miss"><strong lang="' + esc(lang) + '">' + esc(term[sl.k][sl.i]) + "</strong> · " + esc(meaning[sl.k][sl.i]) +
        '</span><span class="meta">' + esc(t("play.js.dates.slow", { secs: Math.round(sl.rt / 100) / 10 })) + "</span></li>");
    });
    if (!ul.children.length && total > 0) ul.innerHTML = '<li><span class="miss">' + esc(t("play.js.dates.nothing_missed", { praise: praise })) + "</span></li>";
    done.hidden = false;
    window.scrollTo(0, 0);
    if (total > 0) {
      post("/api/games/dates/finish", { mode: mode, seconds: seconds, total: total, correct: correct, best_combo: bestCombo, score: score, duration_ms: duration })
        .then(function (res) {
          // The worn motivator's line (#568), drawn by the server; none if the round earned nothing.
          done.querySelectorAll('[data-testid="motivator-reward"]').forEach(function (r) { r.remove(); });
          if (res && res.reward) $("dt-done-score").insertAdjacentHTML("afterend", res.reward);
        }).catch(function () { /* stats only; never block the summary */ });
    }
  }

  function start() {
    mode = radio("mode") || "both";
    seconds = parseInt(radio("len"), 10); if (isNaN(seconds)) seconds = 60;
    store("mode", mode); store("len", String(seconds)); store("ear", earBox.checked ? "1" : "0");
    score = 0; total = 0; correct = 0; streak = 0; bestCombo = 0; finished = false; todayDone = false;
    misses = []; rts = []; current = null;
    resetWeights();
    lastPick = { day: -1, month: -1 };
    hudUpdate();
    $("dt-time-lbl").textContent = seconds > 0 ? t("play.js.dates.seconds") : t("play.js.dates.prompts");
    setup.hidden = true; done.hidden = true;
    hud.hidden = false; card.hidden = false;
    startedAt = performance.now();
    tick();
    if (timerId) clearInterval(timerId);
    timerId = setInterval(tick, 100);
    next();
  }

  setup.addEventListener("submit", function (ev) { ev.preventDefault(); start(); });
  tilesEl.addEventListener("click", function (ev) {
    var t = ev.target.closest(".dash-tile");
    if (t) answer(t.dataset.i, t);
  });
  replayBtn.addEventListener("click", function () { if (current && current.speak) speak(current.speak); });
  $("dt-stop").addEventListener("click", function () {
    if (total > 0) finish();
    else { finished = true; if (timerId) clearInterval(timerId); hush(); card.hidden = true; hud.hidden = true; setup.hidden = false; }
  });
  $("dt-again").addEventListener("click", start);
  $("dt-settings").addEventListener("click", function () { done.hidden = true; setup.hidden = false; });
  done.addEventListener("click", function (ev) {
    var b = ev.target.closest("[data-say]");
    if (b) speak(b.dataset.say);
  });
  document.addEventListener("keydown", function (ev) {
    if (card.hidden || ev.ctrlKey || ev.metaKey || ev.altKey) return;
    if (ev.key === "r" || ev.key === "R") { ev.preventDefault(); if (current && current.speak) speak(current.speak); return; }
    var k = KEYS.indexOf(ev.key);
    if (k < 0) return;
    var tiles = tilesEl.querySelectorAll(".dash-tile");
    if (k < tiles.length) { ev.preventDefault(); answer(tiles[k].dataset.i, tiles[k]); }
  });
  window.addEventListener("pagehide", hush);
})();
