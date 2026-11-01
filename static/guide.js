// Guide stage player: one step at a time. Concept/tip/vocab steps just need
// "next"; question steps are graded by /api/answer (so mastery updates like a
// normal quiz) and then reported to /api/guide/step, which tracks the stage.
(function () {
  "use strict";
  var root = document.getElementById("stage");
  if (!root) return;
  var base = document.body.dataset.base || "";
  var lessonId = root.dataset.lesson;
  var total = parseInt(root.dataset.total, 10);
  var pos = parseInt(root.dataset.start, 10) || 0;
  var praise = root.dataset.praise || t('guide.js.correct');
  var lang = root.dataset.lang || "";
  var steps = Array.prototype.slice.call(root.querySelectorAll(".step"));
  var busy = false;
  var started = Date.now();

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

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

  function show(i) {
    steps.forEach(function (s, j) { s.hidden = j !== i; });
    document.getElementById("hud-pos").textContent = Math.min(i + 1, total);
    document.getElementById("stage-bar").style.width = (total ? (i / total) * 100 : 0) + "%";
    var s = steps[i];
    if (!s) return;
    var focus = s.querySelector("input.fill:not([type=hidden])") || s.querySelector("[data-clock] [tabindex]") || s.querySelector("input[type=radio]") || s.querySelector("[data-continue]");
    if (focus) focus.focus();
    window.scrollTo(0, 0);
  }

  function finish(res) {
    steps.forEach(function (s) { s.hidden = true; });
    document.getElementById("stage-bar").style.width = "100%";
    document.getElementById("hud-pos").textContent = total;
    var stars = res.stars || 0;
    document.getElementById("done-stars").textContent = "★★★".slice(0, stars) + "☆☆☆".slice(0, 3 - stars);
    document.getElementById("done-correct").textContent = res.run_correct;
    document.getElementById("done-answered").textContent = res.run_answered;
    var ps = res.pass, pn = document.getElementById("done-pass"), nx = document.getElementById("btn-next-stage");
    if (pn && ps) {
      pn.textContent = ps.passed_now ? t('guide.js.passed_now')
        : ps.passed ? t('guide.js.already_passed')
        : t('guide.js.not_passed');
      if (nx) nx.hidden = !ps.passed;
    }
    var doneEl = document.getElementById("stage-done");
    // For tests: the run as the server counted it, and whether the lesson is passed.
    doneEl.dataset.correct = res.run_correct; doneEl.dataset.answered = res.run_answered; doneEl.dataset.passed = !!(ps && ps.passed);
    // The worn motivator's line (#568), drawn by the server; none if the stage earned nothing.
    doneEl.querySelectorAll('[data-testid="motivator-reward"]').forEach(function (r) { r.remove(); });
    if (res.reward) document.getElementById("done-stars").insertAdjacentHTML("afterend", res.reward);
    doneEl.hidden = false;
    window.scrollTo(0, 0);
  }

  // Report a completed step; advance on success.
  function complete(index, correct) {
    if (busy) return;
    busy = true;
    post("/api/guide/step", { lesson_id: lessonId, step: index, correct: !!correct, duration_ms: Date.now() - started }).then(function (res) {
      busy = false;
      if (res.finished) { finish(res); return; }
      pos = res.step;
      show(pos);
    }).catch(function () { busy = false; });
  }

  steps.forEach(function (step, i) {
    var cont = step.querySelector("[data-continue]");
    if (cont) cont.addEventListener("click", function () { complete(i, false); });

    step.querySelectorAll(".flashcard").forEach(function (card) {
      var flip = function () { card.classList.toggle("flipped"); };
      // The speaker button beside the term (say.js) reads it; it must not flip the card.
      card.addEventListener("click", function (ev) { if (ev.target.closest(".say")) return; flip(); });
      card.addEventListener("keydown", function (ev) { if (ev.key === " " || ev.key === "Enter") { ev.preventDefault(); flip(); } });
    });

    step.querySelectorAll(".accents button").forEach(function (b) {
      b.addEventListener("click", function () {
        var inp = step.querySelector("input.fill");
        if (!inp || inp.disabled) return;
        var s = inp.selectionStart || 0, e = inp.selectionEnd || 0;
        inp.value = inp.value.slice(0, s) + b.dataset.ch + inp.value.slice(e);
        inp.focus(); inp.setSelectionRange(s + 1, s + 1);
      });
    });

    var form = step.querySelector("form.question");
    if (!form) return;
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      var radio = form.querySelector("input[type=radio]:checked");
      var fill = form.querySelector("input.fill");
      var answer = radio ? radio.value : fill ? fill.value : "";
      if (!answer.trim()) return;
      var submit = form.querySelector("button[type=submit]");
      submit.disabled = true;
      post("/api/answer", { question_id: step.dataset.qid, answer: answer }).then(function (res) {
        var fb = form.querySelector(".feedback");
        fb.className = "feedback " + (res.correct ? "ok" : "bad");
        fb.dataset.key = res.key || ""; // a keyboard question's right key (keyboard.js lights it up)
        fb.dataset.state = res.correct ? "right" : "wrong";
        var html = '<p class="verdict">' + (res.correct ? esc(praise) : esc(t('guide.js.not_quite'))) + "</p>";
        if (res.note) html += "<p>" + esc(res.note) + "</p>";
        if (!res.correct) html += "<p class=\"answer-key\">" + t('guide.js.correct_answer_html', { lang: esc(lang), answer: esc(res.correct_answer) }) + "</p>";
        html += "<p>" + esc(res.explanation) + "</p>";
        if (res.new_words && res.new_words.length) {
          html += '<p class="new-words">' + esc(tn('guide.js.new_terms', res.new_words.length)) + " " +
            res.new_words.map(function (w) { return '<strong lang="' + esc(lang) + '">' + esc(w.term) + "</strong> — " + esc(w.meaning); }).join(", ") + "</p>";
        }
        fb.innerHTML = html;
        fb.hidden = false;
        if (!res.correct && window.answerStaff) window.answerStaff(fb.querySelector(".answer-key"), form, res.correct_answer);
        form.querySelectorAll(".option").forEach(function (o) {
          var v = o.querySelector("input").value;
          var st = v === res.correct_answer ? "correct" : v === answer && !res.correct ? "wrong" : "";
          if (st) { o.classList.add(st); o.dataset.state = st; }
          o.querySelector("input").disabled = true;
        });
        if (fill) fill.disabled = true;
        var actions = form.querySelector(".actions");
        actions.innerHTML = "";
        var btn = document.createElement("button");
        btn.type = "button"; btn.className = "btn";
        btn.textContent = i + 1 >= total ? t('guide.js.finish_stage') : t('guide.js.next');
        btn.dataset.testid = "stage-next-step";
        btn.addEventListener("click", function () { complete(i, res.correct); });
        actions.appendChild(btn);
        btn.focus();
      }).catch(function (err) {
        submit.disabled = false;
        var fb = form.querySelector(".feedback");
        fb.className = "feedback bad";
        fb.textContent = t('guide.js.check_failed', { error: err.message });
        fb.hidden = false;
      });
    });
  });

  document.getElementById("btn-replay").addEventListener("click", function () {
    post("/api/guide/restart", { lesson_id: lessonId }).then(function () { window.location.reload(); });
  });

  if (pos >= total) pos = 0;
  show(pos);
})();
