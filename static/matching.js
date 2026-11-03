// Word Match: the server deals the rounds and keeps the official clock
// (start -> finish); this file runs the board and reports every attempt so the
// server can replay it. Interaction: drag a word onto its match in the other
// column (mouse or touch, via Pointer Events), or tap one then the other.
(function () {
  "use strict";
  var root = document.getElementById("match");
  if (!root) return;
  var base = document.body.dataset.base || "";
  var board = root.dataset.board;
  var penaltySecs = parseInt(root.dataset.penalty, 10) || 1;
  var termLabel = root.dataset.termLabel || "Term", meaningLabel = root.dataset.meaningLabel || "Meaning";
  var termLang = root.dataset.termLang || "", meaningLang = root.dataset.meaningLang || "";

  var session = null, rounds = [], roundIdx = 0, attempts = [], wrong = 0;
  var startedAt = 0, timerHandle = null, locked = false;
  var selected = null;            // tap-to-select: the first tapped tile
  var drag = null;                // {tile, ghost, startX, startY, moved, over}
  var timerEl = document.getElementById("timer");
  var statusEl = document.getElementById("status");
  var boardEl = document.getElementById("board");

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  function post(path, body) {
    return fetch(base + path, {
      method: "POST", credentials: "same-origin",
      headers: { "Content-Type": "application/json" }, body: JSON.stringify(body)
    }).then(function (r) {
      if (r.status === 401) { window.location.href = base + "/login"; throw new Error("login"); }
      if (r.status === 410) { window.location.href = base + "/courses"; throw new Error("course"); }
      if (!r.ok) return r.text().then(function (t) {
        var msg = t; try { msg = JSON.parse(t).error || t; } catch (e) { msg = t.replace(/<[^>]+>/g, " ").trim().slice(0, 120); }
        throw new Error(msg);
      });
      return r.json();
    });
  }
  function tick() {
    var secs = (performance.now() - startedAt) / 1000 + wrong * penaltySecs;
    timerEl.textContent = secs.toFixed(1);
  }
  function setStatus(text) { statusEl.textContent = text; }

  // ---------- board ----------
  function renderRound() {
    var r = rounds[roundIdx];
    // Randomise which side the terms go on this round.
    var termsLeft = Math.random() < 0.5;
    document.getElementById("round-label").textContent = t("play.js.matching.round", { n: roundIdx + 1, total: rounds.length });
    document.getElementById("label-left").textContent = termsLeft ? termLabel : meaningLabel;
    document.getElementById("label-right").textContent = termsLeft ? meaningLabel : termLabel;
    var left = document.getElementById("col-left"), right = document.getElementById("col-right");
    left.innerHTML = ""; right.innerHTML = "";
    var leftItems = termsLeft ? r.left : r.right, rightItems = termsLeft ? r.right : r.left;
    leftItems.forEach(function (s) {
      left.insertAdjacentHTML("beforeend", '<li><div class="tile" role="button" tabindex="0" lang="' + esc(termsLeft ? termLang : meaningLang) + '" ' +
        'data-side="l" data-id="' + s.id + '">' + esc(s.text) + "</div></li>");
    });
    rightItems.forEach(function (s) {
      right.insertAdjacentHTML("beforeend", '<li><div class="tile" role="button" tabindex="0" lang="' + esc(termsLeft ? meaningLang : termLang) + '" ' +
        'data-side="r" data-id="' + s.id + '">' + esc(s.text) + "</div></li>");
    });
    selected = null;
    setStatus(wrong ? t("play.js.matching.wrong_so_far", { n: wrong, secs: wrong * penaltySecs }) : t("play.js.matching.hint"));
  }

  function clearSelection() {
    boardEl.querySelectorAll(".tile.selected").forEach(function (t) { t.classList.remove("selected"); t.setAttribute("aria-pressed", "false"); });
    selected = null;
  }

  function attempt(a, b) {
    if (locked || a === b || a.dataset.side === b.dataset.side) return;
    if (a.classList.contains("matched") || b.classList.contains("matched")) return;
    var l = a.dataset.side === "l" ? a : b, r = a.dataset.side === "l" ? b : a;
    attempts.push([parseInt(l.dataset.id, 10), parseInt(r.dataset.id, 10)]);
    clearSelection();
    if (l.dataset.id === r.dataset.id) {
      l.classList.add("matched"); r.classList.add("matched"); l.dataset.state = r.dataset.state = "matched";
      l.setAttribute("aria-disabled", "true"); r.setAttribute("aria-disabled", "true");
      if (!boardEl.querySelector(".tile:not(.matched)")) {
        if (roundIdx + 1 < rounds.length) {
          locked = true;
          setTimeout(function () { roundIdx += 1; locked = false; renderRound(); }, 350);
        } else {
          finish();
        }
      }
    } else {
      wrong += 1;
      locked = true;
      l.classList.add("wrong"); r.classList.add("wrong"); l.dataset.state = r.dataset.state = "wrong";
      setStatus(t("play.js.matching.wrong_so_far", { n: wrong, secs: wrong * penaltySecs }));
      setTimeout(function () { l.classList.remove("wrong"); r.classList.remove("wrong"); delete l.dataset.state; delete r.dataset.state; locked = false; }, 450);
    }
  }

  function tileAt(x, y) {
    var el = document.elementFromPoint(x, y);
    return el && el.closest ? el.closest(".tile") : null;
  }

  // ---------- pointer events: drag & drop with tap fallback ----------
  function onDown(ev) {
    var tile = ev.target.closest(".tile");
    if (!tile || locked || tile.classList.contains("matched")) return;
    if (ev.button !== undefined && ev.button !== 0) return;
    ev.preventDefault();
    drag = { tile: tile, ghost: null, startX: ev.clientX, startY: ev.clientY, moved: false, over: null, pointerId: ev.pointerId };
    try { tile.setPointerCapture(ev.pointerId); } catch (e) { /* unsupported */ }
  }

  function onMove(ev) {
    if (!drag || ev.pointerId !== drag.pointerId) return;
    var dx = ev.clientX - drag.startX, dy = ev.clientY - drag.startY;
    if (!drag.moved) {
      if (Math.abs(dx) < 6 && Math.abs(dy) < 6) return;
      drag.moved = true;
      var g = drag.tile.cloneNode(true);
      g.classList.add("ghost");
      g.classList.remove("selected");
      var rect = drag.tile.getBoundingClientRect();
      g.style.width = rect.width + "px";
      g.style.left = rect.left + "px";
      g.style.top = rect.top + "px";
      document.body.appendChild(g);
      drag.ghost = g;
      drag.tile.classList.add("dragging");
      clearSelection();
    }
    ev.preventDefault();
    drag.ghost.style.transform = "translate(" + dx + "px," + dy + "px)";
    var over = tileAt(ev.clientX, ev.clientY);
    if (over && (over === drag.tile || over.dataset.side === drag.tile.dataset.side || over.classList.contains("matched"))) over = null;
    if (over !== drag.over) {
      if (drag.over) drag.over.classList.remove("drop-target");
      if (over) over.classList.add("drop-target");
      drag.over = over;
    }
  }

  function endDrag(ev, cancelled) {
    if (!drag || ev.pointerId !== drag.pointerId) return;
    var d = drag; drag = null;
    if (d.ghost) d.ghost.remove();
    d.tile.classList.remove("dragging");
    if (d.over) d.over.classList.remove("drop-target");
    if (cancelled) return;
    if (d.moved) {
      var target = tileAt(ev.clientX, ev.clientY);
      if (target && target !== d.tile && target.dataset.side !== d.tile.dataset.side) attempt(d.tile, target);
      return;
    }
    // A tap: select, or complete a pair with the earlier selection.
    if (selected && selected !== d.tile && selected.dataset.side !== d.tile.dataset.side) {
      attempt(selected, d.tile);
    } else {
      clearSelection();
      selected = d.tile;
      d.tile.classList.add("selected"); d.tile.setAttribute("aria-pressed", "true");
    }
  }

  boardEl.addEventListener("pointerdown", onDown);
  boardEl.addEventListener("pointermove", onMove);
  boardEl.addEventListener("pointerup", function (ev) { endDrag(ev, false); });
  boardEl.addEventListener("pointercancel", function (ev) { endDrag(ev, true); });
  boardEl.addEventListener("keydown", function (ev) {
    var tile = ev.target.closest && ev.target.closest(".tile");
    if (!tile || (ev.key !== " " && ev.key !== "Enter")) return;
    ev.preventDefault();
    if (selected && selected !== tile && selected.dataset.side !== tile.dataset.side) attempt(selected, tile);
    else { clearSelection(); selected = tile; tile.classList.add("selected"); tile.setAttribute("aria-pressed", "true"); }
  });
  // Old browsers without Pointer Events: plain clicks still select pairs.
  if (!window.PointerEvent) {
    boardEl.addEventListener("click", function (ev) {
      var tile = ev.target.closest(".tile");
      if (!tile) return;
      if (selected && selected !== tile && selected.dataset.side !== tile.dataset.side) attempt(selected, tile);
      else { clearSelection(); selected = tile; tile.classList.add("selected"); tile.setAttribute("aria-pressed", "true"); }
    });
  }

  // ---------- game flow ----------
  function start() {
    var btn = document.getElementById("btn-start");
    btn.disabled = true;
    post("/api/games/matching/start", { board: board }).then(function (res) {
      session = res.session; rounds = res.rounds; roundIdx = 0; attempts = []; wrong = 0; locked = false;
      document.getElementById("intro").hidden = true;
      document.getElementById("result").hidden = true;
      boardEl.hidden = false;
      statusEl.hidden = false;
      renderRound();
      startedAt = performance.now();
      timerHandle = setInterval(tick, 100);
    }).catch(function (err) {
      btn.disabled = false;
      statusEl.hidden = false; setStatus(t("play.js.could_not_start", { error: err.message }));
    });
  }

  function finish() {
    clearInterval(timerHandle);
    locked = true;
    post("/api/games/matching/finish", { session: session, attempts: attempts }).then(function (res) {
      timerEl.textContent = (res.score_ms / 1000).toFixed(1);
      boardEl.hidden = true;
      statusEl.hidden = true;
      document.getElementById("result-time").textContent = res.score;
      var d = t("play.js.matching.on_clock", { secs: (res.elapsed_ms / 1000).toFixed(2) });
      if (res.wrong) d += t("play.js.matching.wrong_penalty", { n: res.wrong, secs: penaltySecs });
      d += t("play.js.matching.rank", { rank: res.rank });
      if (res.personal_best) d += t("play.js.matching.personal_best");
      document.getElementById("result-detail").textContent = d;
      // The worn motivator's line (#568), drawn by the server; none if the round earned nothing.
      document.getElementById("result").querySelectorAll('[data-testid="motivator-reward"]').forEach(function (r) { r.remove(); });
      if (res && res.reward) document.getElementById("result-detail").insertAdjacentHTML("afterend", res.reward);
      document.getElementById("result").hidden = false;
      var tb = document.getElementById("lb-body");
      tb.innerHTML = "";
      res.leaderboard.forEach(function (r) {
        tb.insertAdjacentHTML("beforeend", "<tr" + (r.is_you ? ' class="you"' : "") + "><td>" + r.rank + "</td><td>" + esc(r.name) +
          (r.is_you ? esc(t("play.js.matching.you")) : "") + '</td><td class="nowrap"><strong>' + esc(r.time) + "</strong></td><td>" + r.wrong +
          '</td><td class="nowrap muted">' + esc(r.when) + "</td></tr>");
      });
      window.scrollTo(0, 0);
    }).catch(function (err) {
      statusEl.hidden = false; setStatus(t("play.js.matching.could_not_save", { error: err.message }));
      locked = false;
    });
  }

  document.getElementById("btn-start").addEventListener("click", start);
  document.getElementById("btn-again").addEventListener("click", function () {
    document.getElementById("result").hidden = true;
    document.getElementById("intro").hidden = false;
    document.getElementById("btn-start").disabled = false;
    timerEl.textContent = "0.0";
    document.getElementById("btn-start").focus();
  });
})();
