// Flashcard study: one card at a time, flip, then self-grade. Each grade is
// reported to /api/vocabulary/review so the Vocabulary page can show how well
// each word is known.
(function () {
  "use strict";
  var root = document.getElementById("study");
  if (!root) return;
  var base = document.body.dataset.base || "";
  var all = Array.prototype.slice.call(root.querySelectorAll(".study-card"));
  var sideSel = document.getElementById("st-side");
  var actions = document.getElementById("st-actions");
  var queue = all.slice(), pos = 0, known = 0, missed = [], flipped = false;
  // A due round (?due=1, #1360): a missed card comes back once at the end of
  // the round; the tally counts first tries only.
  var dueRound = root.dataset.due === "1", pending = [];

  try { sideSel.value = localStorage.getItem("vocab-side") || "term"; } catch (e) { /* ignore */ }
  sideSel.addEventListener("change", function () {
    try { localStorage.setItem("vocab-side", sideSel.value); } catch (e) { /* ignore */ }
    showCard();
  });

  function first() { return sideSel.value === "meaning" ? "meaning" : "term"; }

  function showCard() {
    all.forEach(function (c) { c.hidden = true; });
    document.getElementById("st-done").hidden = true;
    var card = queue[pos];
    if (!card) return;
    flipped = false;
    card.hidden = false;
    card.querySelector(".face-term").hidden = first() !== "term";
    card.querySelector(".face-meaning").hidden = first() !== "meaning";
    card.querySelector(".hint").hidden = false;
    actions.hidden = true;
    document.getElementById("st-pos").textContent = pos + 1;
    document.getElementById("st-bar").style.width = (queue.length ? (pos / queue.length) * 100 : 0) + "%";
    root.dataset.total = queue.length;
    document.querySelector(".quiz-progress").innerHTML = '<span id="st-pos">' + (pos + 1) + "</span> / " + queue.length;
    card.focus();
  }

  function flip() {
    var card = queue[pos];
    if (!card || flipped) return;
    flipped = true;
    card.querySelector(".face-term").hidden = false;
    card.querySelector(".face-meaning").hidden = false;
    card.querySelector(".hint").hidden = true;
    actions.hidden = false;
    document.getElementById("st-yes").focus();
  }

  function grade(knew) {
    var card = queue[pos];
    if (!card || !flipped) return;
    var repeat = card.dataset.again === "1";
    if (!repeat) { if (knew) known += 1; else missed.push(card); }
    if (dueRound && !knew && !repeat) { card.dataset.again = "1"; queue.push(card); }
    pending.push(fetch(base + "/api/vocabulary/review", {
      method: "POST", credentials: "same-origin",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ lesson_id: card.dataset.lesson, word: card.dataset.word, known: knew })
    }).catch(function () { /* best effort */ }));
    pos += 1;
    if (pos >= queue.length) finish(); else showCard();
  }

  function finish() {
    all.forEach(function (c) { c.hidden = true; });
    actions.hidden = true;
    document.getElementById("st-bar").style.width = "100%";
    document.getElementById("st-known").textContent = known;
    document.getElementById("st-count").textContent = known + missed.length;
    var again = document.getElementById("st-missed");
    if (again) again.hidden = missed.length === 0;
    document.getElementById("st-done").hidden = false;
    if (dueRound) showNext();
  }

  // The due round's summary: how many come back tomorrow, and when the next
  // words are due, once every grade has reached the server.
  function showNext() {
    document.getElementById("st-tomorrow").textContent = missed.length;
    Promise.all(pending).then(function () {
      return fetch(base + "/api/vocabulary/due", { credentials: "same-origin" });
    }).then(function (r) { return r.json(); }).then(function (d) {
      if (!d.next) return;
      var el = document.getElementById("st-next");
      var when = new Date((d.next_at + 12 * 3600) * 1000);
      var lang = document.documentElement.lang || undefined;
      var far = d.next_at - Date.now() / 1000 > 6 * 86400;
      var day = when.toLocaleDateString(lang, far ? { month: "long", day: "numeric" } : { weekday: "long" });
      el.textContent = d.next.replace("{day}", day);
      el.hidden = false;
    }).catch(function () { /* the line stays hidden */ });
  }

  function restart(cards) {
    queue = cards; pos = 0; known = 0; missed = [];
    showCard();
  }

  all.forEach(function (card) {
    // The speaker button on the term face (say.js) reads the word; it must not flip the card.
    card.addEventListener("click", function (ev) { if (ev.target.closest(".say")) return; flip(); });
  });
  document.getElementById("st-yes").addEventListener("click", function () { grade(true); });
  document.getElementById("st-no").addEventListener("click", function () { grade(false); });
  if (!dueRound) {
    document.getElementById("st-again").addEventListener("click", function () { restart(all.slice()); });
    document.getElementById("st-missed").addEventListener("click", function () { restart(missed.slice()); });
  }
  document.addEventListener("keydown", function (ev) {
    if (ev.target.tagName === "SELECT") return;
    if (ev.key === " " || ev.key === "Enter") { ev.preventDefault(); if (!flipped) flip(); else grade(true); }
    else if (ev.key === "ArrowLeft" || ev.key === "n") { if (flipped) grade(false); }
    else if (ev.key === "ArrowRight" || ev.key === "y") { if (flipped) grade(true); }
  });

  showCard();
})();
