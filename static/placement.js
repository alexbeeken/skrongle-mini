// The placement test in a course's start wizard (#1106,
// templates/placement_test.html). "Take the placement test" fetches one
// sitting (GET …/placement: questions without answers), asks them one at a
// time in place of the level cards, posts the picks to be marked, then
// shows where to start and picks that level card, so Get Started! saves it.
// The report card lists each level's tally, green (all right), yellow
// (some) or red (none). Nothing is stored until the wizard is saved.
(function () {
  "use strict";
  function fill(text, vars) {
    return String(text).replace(/\{(\w+)\}/g, function (m, k) { return k in vars ? vars[k] : m; });
  }
  // The wizard's viewport follows the step's height (course-settings.js).
  function refit(el) {
    var dlg = el.closest("dialog");
    if (dlg) dlg.dispatchEvent(new Event("cs-fit"));
  }

  document.querySelectorAll("[data-testid=placement]").forEach(function (box) {
    var step = box.closest(".cs-step") || box.parentNode;
    var quiz = box.querySelector(".pt-quiz"), result = box.querySelector(".pt-result");
    var offer = box.querySelector(".pt-offer");
    var count = box.querySelector("[data-pt-count]"), prompt = box.querySelector("[data-pt-prompt]");
    var options = box.querySelector("[data-pt-options]");
    var suggest = box.querySelector("[data-pt-suggest]");
    var toggle = box.querySelector("[data-pt-card]"), card = box.querySelector(".pt-card");
    var questions = [], answers = [], at = 0;

    function show(part) {
      offer.hidden = part !== "offer";
      quiz.hidden = part !== "quiz";
      result.hidden = part !== "result";
      step.classList.toggle("pt-testing", part === "quiz");
      // Get Started! waits for the answers (it sits under every step, #896).
      var dlg = box.closest("dialog");
      if (dlg) dlg.classList.toggle("pt-testing", part === "quiz");
      refit(box);
    }
    function ask() {
      var q = questions[at];
      quiz.dataset.questionId = q.id;
      count.textContent = fill(box.dataset.count, { n: at + 1, total: questions.length });
      prompt.textContent = q.prompt;
      options.textContent = "";
      q.options.forEach(function (o) {
        var b = document.createElement("button");
        b.type = "button";
        b.className = "btn ghost pt-option";
        b.textContent = o;
        b.dataset.testid = "placement-option";
        b.addEventListener("click", function () { pick(o); });
        options.appendChild(b);
      });
      refit(box);
      var first = options.querySelector("button");
      if (first) first.focus();
    }
    function pick(option) {
      answers.push({ id: questions[at].id, answer: option });
      at += 1;
      if (at < questions.length) ask(); else finish();
    }
    function failed() {
      offer.hidden = false;
      quiz.hidden = true;
      step.classList.remove("pt-testing");
      var p = offer.querySelector(".pt-error") || offer.appendChild(document.createElement("span"));
      p.className = "pt-error small bad-text";
      p.setAttribute("role", "alert");
      p.textContent = " " + box.dataset.failed;
      refit(box);
    }
    function start() {
      fetch(box.dataset.url, { credentials: "same-origin" })
        .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
        .then(function (data) {
          questions = data.questions || [];
          answers = [];
          at = 0;
          if (!questions.length) throw new Error("empty");
          show("quiz");
          ask();
        })
        .catch(failed);
    }
    function finish() {
      fetch(box.dataset.url, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ answers: answers }) })
        .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
        .then(report)
        .catch(failed);
    }
    // The level card of unit `id` (the first card's value is '', the beginning).
    function cardOf(id) {
      var cards = step.querySelectorAll("[data-testid=courses-settings-level]");
      for (var i = 0; i < cards.length; i++) if (cards[i].dataset.unit === id) return cards[i];
      return cards[0];
    }
    function report(res) {
      var chosen = cardOf(res.start_unit);
      var title = chosen ? chosen.querySelector("strong").textContent : res.start_unit;
      suggest.textContent = fill(box.dataset.suggest, { unit: title });
      if (chosen) {
        var radio = chosen.querySelector("input");
        radio.checked = true;
        radio.dispatchEvent(new Event("change", { bubbles: true }));
        step.querySelectorAll(".pt-suggested").forEach(function (c) { c.classList.remove("pt-suggested"); });
        chosen.classList.add("pt-suggested");
      }
      var body = card.querySelector("tbody");
      body.textContent = "";
      (res.units || []).forEach(function (u) {
        var tr = document.createElement("tr");
        tr.className = "pt-" + u.status;
        tr.dataset.testid = "placement-card-row";
        tr.dataset.unit = u.unit;
        tr.dataset.status = u.status;
        var name = document.createElement("td");
        name.textContent = u.title;
        var tally = document.createElement("td");
        tally.textContent = u.correct + " / " + u.total + " ";
        var said = document.createElement("span");
        said.className = "sr-only";
        said.textContent = box.dataset[u.status] || "";
        tally.appendChild(said);
        tr.appendChild(name);
        tr.appendChild(tally);
        body.appendChild(tr);
      });
      setCard(false);
      show("result");
    }
    function setCard(open) {
      card.hidden = !open;
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
      toggle.textContent = open ? box.dataset.hide : box.dataset.show;
      refit(box);
    }
    box.querySelectorAll("[data-pt-start]").forEach(function (b) { b.addEventListener("click", start); });
    toggle.addEventListener("click", function () { setCard(card.hidden); });
    // Reopening the wizard starts over at the offer.
    var dlg = box.closest("dialog");
    if (dlg) dlg.addEventListener("close", function () { show("offer"); });
  });
})();
