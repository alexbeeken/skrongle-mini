// Quiz runner: shows one question at a time, posts answers to /api/answer,
// renders the explanation inline and tracks a per-lesson summary.
(function () {
  "use strict";
  var root = document.getElementById("quiz");
  if (!root) return;
  var base = document.body.dataset.base || "";
  var forms = Array.prototype.slice.call(root.querySelectorAll("form.question"));
  var total = forms.length;
  var pos = 0, correct = 0;
  var started = Date.now();
  var lessons = {}; // slug -> {title, first, last, asked, right}
  var praise = root.dataset.praise || t('quiz.js.correct');
  var lang = root.dataset.lang || "";
  var posEl = document.getElementById("q-pos");
  var barEl = document.getElementById("q-bar");

  function show(i) {
    forms.forEach(function (f, j) { f.hidden = j !== i; });
    posEl.textContent = Math.min(i + 1, total);
    barEl.style.width = (total ? (i / total) * 100 : 0) + "%";
    var f = forms[i];
    if (!f) return;
    // Focus a text box or a clock to set, never a radio: a focused radio looks chosen (#405).
    var inp = f.querySelector("input.fill:not([type=hidden])") || f.querySelector("[data-clock] [tabindex]");
    if (inp) inp.focus();
  }

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

  function finish() {
    forms.forEach(function (f) { f.hidden = true; });
    barEl.style.width = "100%";
    posEl.textContent = total;
    document.getElementById("s-correct").textContent = correct;
    document.getElementById("s-total").textContent = total;
    var summary = document.getElementById("summary");
    summary.dataset.correct = correct; summary.dataset.total = total; // for tests: the score sent to the server
    var ul = document.getElementById("s-lessons");
    ul.innerHTML = "";
    Object.keys(lessons).forEach(function (slug) {
      var l = lessons[slug];
      var d = l.last - l.first;
      // The change in mastery, "(+17)" or "(-5)", and nothing when it did not move (#408).
      var delta = d ? ' <span class="delta ' + (d > 0 ? "up" : "down") + '" data-delta="' + d + '">(' + (d > 0 ? "+" : "") + d + ")</span>" : "";
      ul.insertAdjacentHTML("beforeend",
        '<li data-testid="quiz-summary-lesson" data-lesson-id="' + esc(slug) + '"><a href="' + base + '/lessons/' + esc(slug) + '">' + esc(l.title) + "</a>" +
        '<span class="meta">' + esc(t('quiz.js.lesson_result', { right: l.right, asked: l.asked, mastery: l.last })) + delta +
        ' <span class="pass-line" data-pass="' + esc(slug) + '"></span></span>' +
        '<a class="btn small ghost" href="' + base + '/practice?lesson=' + esc(slug) + '">' + esc(t('quiz.js.practise')) + "</a></li>");
    });
    document.getElementById("summary").hidden = false;
    window.scrollTo(0, 0);
    report();
  }

  // One run per lesson of the round; the server decides what passes.
  function report() {
    var runs = Object.keys(lessons).map(function (slug) {
      return { lesson: slug, answered: lessons[slug].asked, correct: lessons[slug].right };
    });
    if (!runs.length) return;
    fetch(base + "/api/practice/finish", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify({ runs: runs, duration_ms: Date.now() - started })
    }).then(function (r) { return r.ok ? r.json() : { runs: [] }; }).then(function (res) {
      // The worn motivator's line (#568), drawn by the server; none if the round earned nothing.
      if (res.reward) document.getElementById("s-lessons").insertAdjacentHTML("beforebegin", res.reward);
      res.runs.forEach(function (o, i) {
        var el = root.querySelector('[data-pass="' + runs[i].lesson + '"]');
        if (!el) return;
        if (o.passed_now) el.innerHTML = '<span class="pill mastered">' + esc(t('quiz.js.lesson_passed_now')) + "</span>";
        else if (o.passed) el.innerHTML = '<span class="pill mastered">' + esc(t('quiz.js.passed')) + "</span>";
        else if (o.run_counted && o.run_pct >= 70 && !o.seen) el.textContent = t('quiz.js.read_to_pass');
        else if (o.run_counted) el.textContent = t('quiz.js.seventy_passes');
        else el.textContent = t('quiz.js.answers_needed', { n: o.required_answers });
      });
    }).catch(function () {});
  }

  function next() {
    pos += 1;
    if (pos >= total) finish(); else show(pos);
  }

  function render(form, res, given) {
    var fb = form.querySelector(".feedback");
    fb.className = "feedback " + (res.correct ? "ok" : "bad");
    fb.dataset.key = res.key || ""; // a keyboard question's right key (keyboard.js lights it up)
    fb.dataset.state = res.correct ? "right" : "wrong";
    var html = '<p class="verdict">' + (res.correct ? esc(praise) : esc(t('quiz.js.not_quite'))) + "</p>";
    if (res.note) html += "<p>" + esc(res.note) + "</p>";
    if (!res.correct) html += "<p class=\"answer-key\">" + esc(t('quiz.js.correct_answer')) + ' <strong lang="' + esc(lang) + '">' + esc(res.correct_answer) + "</strong></p>";
    html += "<p>" + esc(res.explanation) + "</p>";
    if (res.new_words && res.new_words.length) {
      html += '<p class="new-words">' + esc(tn('quiz.js.new_terms', res.new_words.length)) + " " +
        res.new_words.map(function (w) { return '<strong lang="' + esc(lang) + '">' + esc(w.term) + "</strong> — " + esc(w.meaning); }).join(", ") + "</p>";
    }
    html += '<p class="mastery">' + t('quiz.js.mastery_html', { lesson: esc(res.lesson_title), mastery: res.mastery }) +
      (res.streak > 1 ? esc(t('quiz.js.streak', { n: res.streak })) : "") + "</p>";
    fb.innerHTML = html;
    fb.hidden = false;
    if (!res.correct && window.answerStaff) window.answerStaff(fb.querySelector(".answer-key"), form, res.correct_answer);

    // Mark options.
    form.querySelectorAll(".option").forEach(function (o) {
      var v = o.querySelector("input").value;
      var st = v === res.correct_answer ? "correct" : v === given && !res.correct ? "wrong" : "";
      if (st) { o.classList.add(st); o.dataset.state = st; }
      o.querySelector("input").disabled = true;
    });
    var fill = form.querySelector("input.fill");
    if (fill) fill.disabled = true;
    // The answer is graded: the hint has nothing left to help with (#406).
    var hints = form.querySelector(".hints");
    if (hints) hints.hidden = true;

    var actions = form.querySelector(".actions");
    actions.innerHTML = "";
    var btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn";
    btn.textContent = pos + 1 >= total ? t('quiz.js.see_results') : t('quiz.js.next');
    btn.dataset.testid = "quiz-next";
    btn.addEventListener("click", next);
    actions.appendChild(btn);
    btn.focus();
  }

  // Check with nothing chosen or typed: say so in the feedback area, in the
  // page's own style, instead of the browser's validation bubble (#405).
  function askFirst(form) {
    var fb = form.querySelector(".feedback");
    fb.className = "feedback ask";
    fb.dataset.state = "empty";
    fb.textContent = form.querySelector("input.fill") ? t('quiz.js.type_first') : t('quiz.js.pick_first');
    fb.hidden = false;
    var fill = form.querySelector("input.fill");
    if (fill) fill.focus();
  }

  // The "pick an answer first" line goes as soon as there is an answer.
  function clearAsk(form) {
    var fb = form.querySelector(".feedback");
    if (fb.dataset.state !== "empty") return;
    fb.hidden = true;
    fb.className = "feedback";
    fb.textContent = "";
    delete fb.dataset.state;
  }

  forms.forEach(function (form) {
    form.addEventListener("input", function () { clearAsk(form); });
    form.addEventListener("change", function () { clearAsk(form); });

    // Accent helper buttons.
    form.querySelectorAll(".accents button").forEach(function (b) {
      b.addEventListener("click", function () {
        var inp = form.querySelector("input.fill");
        if (!inp || inp.disabled) return;
        var s = inp.selectionStart || 0, e = inp.selectionEnd || 0;
        inp.value = inp.value.slice(0, s) + b.dataset.ch + inp.value.slice(e);
        inp.focus();
        inp.setSelectionRange(s + 1, s + 1);
      });
    });

    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      var answer;
      var radio = form.querySelector("input[type=radio]:checked");
      var fill = form.querySelector("input.fill");
      if (radio) answer = radio.value;
      else if (fill) answer = fill.value;
      if (answer === undefined || answer.trim() === "") { askFirst(form); return; }
      var submit = form.querySelector("button[type=submit]");
      submit.disabled = true;

      fetch(base + "/api/answer", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "same-origin",
        body: JSON.stringify({ question_id: form.dataset.qid, answer: answer })
      }).then(function (r) {
        if (r.status === 401) { window.location.href = base + "/login"; return null; }
        if (r.status === 410) { window.location.href = base + "/courses"; return null; }
        if (!r.ok) throw new Error("HTTP " + r.status);
        return r.json();
      }).then(function (res) {
        if (!res) return;
        if (res.correct) correct += 1;
        var l = lessons[res.lesson_slug] || (lessons[res.lesson_slug] = { title: res.lesson_title, first: null, last: 0, asked: 0, right: 0 });
        l.asked += 1;
        if (res.correct) l.right += 1;
        l.last = res.mastery;
        if (l.first === null) l.first = res.mastery_before;
        render(form, res, answer);
      }).catch(function (err) {
        submit.disabled = false;
        var fb = form.querySelector(".feedback");
        fb.className = "feedback bad";
        fb.textContent = t('quiz.js.submit_failed', { error: err.message });
        fb.hidden = false;
      });
    });
  });

  // A text box, select or editable area keeps its keys: digits are typed there.
  function typing(el) {
    if (!el || !el.tagName) return false;
    if (el.isContentEditable || el.tagName === "TEXTAREA" || el.tagName === "SELECT") return true;
    return el.tagName === "INPUT" && !/^(radio|checkbox|button|submit|reset)$/.test(el.type);
  }

  // Keys: 1–9 pick the n-th option (#407); Enter checks a picked answer and,
  // after feedback, advances. Enter on the next-button is handled natively too.
  // "Advances" only after a graded answer: not while "pick an answer first" shows (#405).
  document.addEventListener("keydown", function (ev) {
    if (ev.ctrlKey || ev.metaKey || ev.altKey || ev.isComposing) return;
    var f = forms[pos];
    if (!f || f.hidden) return;
    var fb = f.querySelector(".feedback");
    var graded = !!fb && !fb.hidden && (fb.dataset.state === "right" || fb.dataset.state === "wrong");
    if (ev.key === "Enter") {
      if (graded) { ev.preventDefault(); next(); return; }
      if (typing(ev.target)) return; // the text box submits the form itself
      if (ev.target.closest && ev.target.closest("a, summary")) return; // a link or the Hint toggle keeps Enter
      var submit = f.querySelector("button[type=submit]");
      if (f.querySelector("input[type=radio]:checked") && submit && !submit.disabled) {
        ev.preventDefault();
        submit.click();
      }
      return;
    }
    if (!/^[1-9]$/.test(ev.key) || graded || typing(ev.target)) return;
    var radio = f.querySelectorAll("input[type=radio]")[Number(ev.key) - 1];
    if (!radio || radio.disabled) return;
    ev.preventDefault();
    radio.checked = true;
    radio.dispatchEvent(new Event("change", { bubbles: true }));
    radio.focus();
  });

  if (total > 0) show(0);
})();
