// Paradigm drill ("Conjugation Drill" for a language course). The server deals
// prompts (item picture + slot picture) for the chosen set/items and checks each
// typed form; this file runs the round. Slot pictures come from the course
// manifest (data-art on the legend).
(function () {
  "use strict";
  var base = document.body.dataset.base || "";

  var SLOT_ART = {}, SLOT_WHO = {}, PEOPLE = { me: null, others: [] };
  function art(lines) { return (lines || []).join("\n"); }

  // A slot with a "who" (#879) is drawn with profile pictures: the learner's
  // own for "me", others picked at random (never the learner's) for the rest.
  var CAST = { me: [1, 0], you: [0, 1], other: [0, 1], us: [1, 2], you_all: [0, 3], them: [0, 3] };
  function portrait(p, person) {
    var span = document.createElement("span");
    span.className = "avatar" + (p.url ? " themed" : "");
    span.setAttribute("style", p.style || "");
    span.dataset.testid = "drill-portrait";
    span.dataset.person = person;
    span.dataset.avatar = p.id;
    if (p.url) {
      var img = document.createElement("img"); img.src = p.url; img.alt = ""; span.appendChild(img);
    } else {
      var ns = "http://www.w3.org/2000/svg", svg = document.createElementNS(ns, "svg"), use = document.createElementNS(ns, "use");
      use.setAttribute("href", "#av-" + p.id); svg.appendChild(use); span.appendChild(svg);
    }
    return span;
  }
  function drawPeople(el, who) {
    el.textContent = "";
    var cast = CAST[who];
    if (!cast || !PEOPLE.me) return false;
    el.className = "people who-" + who;
    el.dataset.who = who;
    el.setAttribute("role", "img");
    el.setAttribute("aria-label", ({
      me: t("play.js.drill.people.me"), you: t("play.js.drill.people.you"), other: t("play.js.drill.people.other"),
      us: t("play.js.drill.people.us"), you_all: t("play.js.drill.people.you_all"), them: t("play.js.drill.people.them")
    })[who]);
    if (cast[0]) el.appendChild(portrait(PEOPLE.me, "me"));
    var pool = PEOPLE.others.slice();
    for (var i = 0; i < cast[1] && pool.length; i++) {
      el.appendChild(portrait(pool.splice(Math.floor(Math.random() * pool.length), 1)[0], "other"));
    }
    return true;
  }
  function readLegend(sel) {
    var ul = document.querySelector(sel);
    if (ul && ul.dataset.people) { try { PEOPLE = JSON.parse(ul.dataset.people); } catch (e) { /* ASCII only */ } }
    document.querySelectorAll(sel + " pre.subject").forEach(function (p) {
      try { SLOT_ART[p.dataset.slot] = JSON.parse(p.dataset.art || "[]"); } catch (e) { SLOT_ART[p.dataset.slot] = []; }
      SLOT_WHO[p.dataset.slot] = p.dataset.who || "";
      var people = document.createElement("div");
      if (drawPeople(people, SLOT_WHO[p.dataset.slot])) {
        people.dataset.testid = "drill-legend-people";
        people.dataset.slot = p.dataset.slot;
        p.replaceWith(people);
      } else {
        p.textContent = art(SLOT_ART[p.dataset.slot]);
      }
    });
  }

  // Setup page: render the legend and keep the hidden "items" field in sync.
  var legend = document.getElementById("subject-legend");
  if (legend) {
    // On a phone the tabs scroll sideways: bring the open one into view.
    document.querySelectorAll(".pick-tabs").forEach(function (nav) {
      var cur = nav.querySelector("[aria-current]");
      if (cur) nav.scrollLeft = cur.offsetLeft - (nav.clientWidth - cur.offsetWidth) / 2;
    });
    readLegend("#subject-legend");
    var form = document.getElementById("conj-setup");
    function syncItems() {
      var set = (form.querySelector('input[name="set"]') || {}).value;
      var picked = [];
      form.querySelectorAll('.verb-picks[data-set="' + set + '"] input[type=checkbox]:checked').forEach(function (c) { picked.push(c.value); });
      document.getElementById("items-field").value = picked.join(",");
      var submit = form.querySelector('button[type=submit]');
      if (!submit.hasAttribute("data-locked")) submit.disabled = picked.length === 0;
    }
    form.addEventListener("change", syncItems);
    form.querySelectorAll("[data-select]").forEach(function (b) {
      b.addEventListener("click", function () {
        form.querySelectorAll('.verb-picks[data-set="' + b.dataset.set + '"] input[type=checkbox]:not(:disabled)').forEach(function (c) {
          c.checked = b.dataset.select === "all";
        });
        syncItems();
      });
    });
    form.addEventListener("submit", function () {
      // The items travel in the hidden "items" field; drop the checkboxes from the URL.
      form.querySelectorAll('input[type=checkbox][name^="item-"]').forEach(function (c) { c.disabled = true; });
    });
    if (form.querySelector('button[type=submit]').disabled) form.querySelector('button[type=submit]').setAttribute("data-locked", "1");
    syncItems();
    return;
  }

  // Play page.
  var root = document.getElementById("conj");
  if (!root) return;
  readLegend("#subject-legend-inline");
  var set = root.dataset.set;
  var items = root.dataset.items.split(",").filter(Boolean);
  var count = parseInt(root.dataset.count, 10) || 0;
  var hideInfinitive = root.dataset.hide === "1";
  var praise = root.dataset.praise || "Correct!";
  var lang = root.dataset.lang || "";

  var prompts = [], pos = 0, correct = 0, misses = [], answered = false, startedAt = 0;
  var card = document.getElementById("c-card"), status = document.getElementById("c-status");
  var input = document.getElementById("c-answer"), fb = document.getElementById("c-feedback");
  var form = document.getElementById("c-form"), checkBtn = document.getElementById("c-check");

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
      if (!r.ok) return r.text().then(function (t) { var m = t; try { m = JSON.parse(t).error || t; } catch (e) { /* html */ } throw new Error(m); });
      return r.json();
    });
  }

  function show() {
    var p = prompts[pos];
    // Which form is asked, for tests (docs/testing.md).
    card.dataset.item = p.item; card.dataset.slot = p.slot;
    document.getElementById("c-pos").textContent = pos + 1;
    document.getElementById("c-total").textContent = prompts.length;
    document.getElementById("c-bar").style.width = (prompts.length ? (pos / prompts.length) * 100 : 0) + "%";
    document.getElementById("c-verb-art").textContent = art(p.art);
    document.getElementById("c-infinitive").textContent = hideInfinitive ? "" : p.item;
    document.getElementById("c-meaning").textContent = hideInfinitive ? "" : "· " + p.meaning;
    document.getElementById("c-caption").hidden = hideInfinitive;
    var subjectArt = document.getElementById("c-subject-art"), people = document.getElementById("c-subject-people");
    var drawn = drawPeople(people, SLOT_WHO[p.slot]);
    people.hidden = !drawn; subjectArt.hidden = drawn;
    subjectArt.textContent = drawn ? "" : art(SLOT_ART[p.slot] || []);
    document.getElementById("c-subject-caption").textContent = t("play.js.drill.which_slot");
    input.value = ""; input.disabled = false;
    fb.hidden = true; fb.innerHTML = "";
    checkBtn.textContent = t("play.js.check"); checkBtn.disabled = false;
    answered = false;
    card.hidden = false;
    input.focus();
  }

  function finish() {
    card.hidden = true;
    post("/api/games/conjugation/finish", {
      set: set, items: items, total: prompts.length, correct: correct,
      duration_ms: Math.round(performance.now() - startedAt)
    }).then(function (res) {
      // The worn motivator's line (#568), drawn by the server; none if the round earned nothing.
      document.getElementById("c-done").querySelectorAll('[data-testid="motivator-reward"]').forEach(function (r) { r.remove(); });
      if (res && res.reward) document.getElementById("c-score").insertAdjacentHTML("afterend", res.reward);
    }).catch(function () { /* stats only; never block the summary */ });
    document.getElementById("c-bar").style.width = "100%";
    document.getElementById("c-pos").textContent = prompts.length;
    document.getElementById("c-score").textContent = correct + " / " + prompts.length;
    var secs = ((performance.now() - startedAt) / 1000).toFixed(0);
    document.getElementById("c-detail").textContent = t("play.js.drill.right_in", { pct: Math.round((correct / Math.max(1, prompts.length)) * 100), secs: secs });
    var ul = document.getElementById("c-misses");
    ul.innerHTML = "";
    misses.forEach(function (m) {
      ul.insertAdjacentHTML("beforeend", '<li><span lang="' + esc(lang) + '"><strong>' + esc(m.expected) + "</strong></span><span class=\"meta\">" +
        esc(m.item) + " · " + esc(m.slot_label) + " · " + t("play.js.drill.you_wrote", { answer: esc(m.answer) }) + "</span></li>");
    });
    document.getElementById("c-again-misses").hidden = misses.length === 0;
    document.getElementById("c-done").hidden = false;
    window.scrollTo(0, 0);
  }

  function next() {
    pos += 1;
    if (pos >= prompts.length) finish(); else show();
  }

  function check() {
    var p = prompts[pos];
    var answer = input.value;
    if (!answer.trim()) return;
    checkBtn.disabled = true;
    post("/api/games/conjugation/check", { set: set, item: p.item, slot: p.slot, answer: answer }).then(function (res) {
      answered = true;
      input.disabled = true;
      if (res.correct) correct += 1;
      else misses.push({ item: p.item, slot: p.slot, expected: res.expected, slot_label: res.slot_label, answer: answer, prompt: p });
      document.getElementById("c-subject-caption").textContent = res.slot_label + (res.slot_hint ? " · " + res.slot_hint : "");
      var html = '<p class="verdict">' + (res.correct ? esc(praise) : esc(t("play.js.not_quite"))) + "</p>";
      if (res.note) html += "<p>" + esc(res.note) + "</p>";
      if (!res.correct) html += "<p>" + t("play.js.drill.correct_form_html", { lang: esc(lang), form: esc(res.expected), item: esc(p.item), slot: esc(res.slot_label) }) + "</p>";
      fb.className = "feedback " + (res.correct ? "ok" : "bad");
      fb.dataset.state = res.correct ? "right" : "wrong";
      fb.innerHTML = html; fb.hidden = false;
      checkBtn.textContent = pos + 1 >= prompts.length ? t("play.js.finish") : t("play.js.next");
      checkBtn.disabled = false;
      checkBtn.focus();
    }).catch(function (err) {
      checkBtn.disabled = false;
      fb.className = "feedback bad"; fb.textContent = t("play.js.drill.could_not_check", { error: err.message }); fb.hidden = false;
    });
  }

  form.addEventListener("submit", function (ev) { ev.preventDefault(); if (answered) next(); else check(); });
  form.querySelectorAll(".accents button").forEach(function (b) {
    b.addEventListener("click", function () {
      if (input.disabled) return;
      var s = input.selectionStart || 0, e = input.selectionEnd || 0;
      input.value = input.value.slice(0, s) + b.dataset.ch + input.value.slice(e);
      input.focus(); input.setSelectionRange(s + 1, s + 1);
    });
  });

  function run(list) {
    prompts = list; pos = 0; correct = 0; misses = [];
    document.getElementById("c-done").hidden = true;
    status.hidden = true;
    startedAt = performance.now();
    show();
  }
  function deal() {
    status.hidden = false; status.textContent = t("play.js.drill.dealing");
    post("/api/games/conjugation/deal", { set: set, items: items, count: count }).then(function (res) {
      if (!res.prompts.length) { status.textContent = t("play.js.drill.nothing"); return; }
      run(res.prompts);
    }).catch(function (err) { status.textContent = t("play.js.could_not_start", { error: err.message }); });
  }
  document.getElementById("c-again").addEventListener("click", deal);
  document.getElementById("c-again-misses").addEventListener("click", function () {
    run(misses.map(function (m) { return m.prompt; }));
  });
  deal();
})();
