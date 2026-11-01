// Reading check at the bottom of a lesson: two of its questions answered
// together and posted to /api/lesson-check. Each answer is graded and
// recorded like a practice answer; all right marks the lesson as read, which
// is what lets recommended practice draw from it.
(function () {
  "use strict";
  var root = document.getElementById("lesson-check");
  var form = document.getElementById("check-form");
  if (!root || !form) return;
  var base = document.body.dataset.base || "";
  var praise = root.dataset.praise || t('study.js.correct');
  var lang = root.dataset.lang || "";
  var questions = Array.prototype.slice.call(form.querySelectorAll(".question"));
  var result = document.getElementById("check-result");

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

  function answerOf(q) {
    var radio = q.querySelector("input[type=radio]:checked");
    if (radio) return radio.value;
    var fill = q.querySelector("input.fill");
    return fill ? fill.value : "";
  }

  function renderOne(q, res, given) {
    var fb = q.querySelector(".feedback");
    fb.className = "feedback " + (res.correct ? "ok" : "bad");
    fb.dataset.key = res.key || ""; // a keyboard question's right key (keyboard.js lights it up)
    fb.dataset.state = res.correct ? "right" : "wrong";
    var html = '<p class="verdict">' + (res.correct ? esc(praise) : esc(t('study.js.not_quite'))) + "</p>";
    if (res.note) html += "<p>" + esc(res.note) + "</p>";
    if (!res.correct) html += "<p class=\"answer-key\">" + t('study.js.correct_answer_html', { lang: esc(lang), answer: esc(res.correct_answer) }) + "</p>";
    html += "<p>" + esc(res.explanation) + "</p>";
    if (res.new_words && res.new_words.length) {
      html += '<p class="new-words">' + esc(tn('study.js.new_terms_collected', res.new_words.length)) +
        res.new_words.map(function (w) { return '<strong lang="' + esc(lang) + '">' + esc(w.term) + "</strong> — " + esc(w.meaning); }).join(", ") + "</p>";
    }
    fb.innerHTML = html;
    fb.hidden = false;
    if (!res.correct && window.answerStaff) window.answerStaff(fb.querySelector(".answer-key"), q, res.correct_answer);
    q.querySelectorAll(".option").forEach(function (o) {
      var v = o.querySelector("input").value;
      var st = v === res.correct_answer ? "correct" : v === given && !res.correct ? "wrong" : "";
      if (st) { o.classList.add(st); o.dataset.state = st; }
    });
  }

  function lock() {
    form.querySelectorAll("input, button").forEach(function (el) { el.disabled = true; });
  }

  function renderResult(res) {
    var last = res.results[res.results.length - 1];
    var html;
    if (res.passed) {
      html = '<p class="verdict">' + esc(t('study.js.lesson_read')) + "</p>" +
        "<p>" + t('study.js.read_passed_html', { mastery: esc(String(last.mastery)) }) + "</p>" +
        '<p class="actions"><a class="btn" href="' + base + '/practice">' + esc(t('study.js.recommended_practice')) + "</a> " +
        '<a class="btn ghost" href="' + base + '/lessons">' + esc(t('study.js.back_to_lessons')) + "</a></p>";
      result.className = "check-result ok";
      result.dataset.state = "passed";
    } else {
      html = '<p class="verdict">' + esc(t('study.js.not_read')) + "</p>" +
        "<p>" + esc(t('study.js.not_read_hint')) + "</p>" +
        '<p class="actions"><button type="button" class="btn" id="check-retry" data-testid="lesson-check-retry">' + esc(t('study.js.try_two_more')) + "</button></p>";
      result.className = "check-result bad";
      result.dataset.state = "failed";
    }
    // The worn motivator's line (#568), drawn by the server.
    if (res.reward) html = res.reward + html;
    result.innerHTML = html;
    result.hidden = false;
    var retry = document.getElementById("check-retry");
    if (retry) {
      retry.addEventListener("click", function () {
        window.location.href = window.location.pathname + window.location.search + "#lesson-check";
        window.location.reload();
      });
      retry.focus();
    } else {
      var go = result.querySelector("a.btn");
      if (go) go.focus();
    }
  }

  // Accent helper buttons.
  form.querySelectorAll(".accents button").forEach(function (b) {
    b.addEventListener("click", function () {
      var inp = b.closest(".question").querySelector("input.fill");
      if (!inp || inp.disabled) return;
      var s = inp.selectionStart || 0, e = inp.selectionEnd || 0;
      inp.value = inp.value.slice(0, s) + b.dataset.ch + inp.value.slice(e);
      inp.focus();
      inp.setSelectionRange(s + 1, s + 1);
    });
  });

  form.addEventListener("submit", function (ev) {
    ev.preventDefault();
    var answers = [];
    for (var i = 0; i < questions.length; i++) {
      var a = answerOf(questions[i]);
      if (a.trim() === "") {
        var inp = questions[i].querySelector("input.fill") || questions[i].querySelector("input[type=radio]");
        if (inp) inp.focus();
        return;
      }
      answers.push({ question_id: questions[i].dataset.qid, answer: a });
    }
    var submit = form.querySelector("button[type=submit]");
    submit.disabled = true;

    fetch(base + "/api/lesson-check", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify({ lesson: root.dataset.lesson, answers: answers })
    }).then(function (r) {
      if (r.status === 401) { window.location.href = base + "/login"; return null; }
      if (r.status === 410) { window.location.href = base + "/courses"; return null; }
      if (!r.ok) throw new Error("HTTP " + r.status);
      return r.json();
    }).then(function (res) {
      if (!res) return;
      lock();
      res.results.forEach(function (one, i) { renderOne(questions[i], one, answers[i].answer); });
      renderResult(res);
      result.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }).catch(function (err) {
      submit.disabled = false;
      result.className = "check-result bad";
      result.textContent = t('study.js.submit_failed', { error: err.message });
      result.hidden = false;
    });
  });
})();
