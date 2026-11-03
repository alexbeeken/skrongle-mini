// AI tutoring: a one-to-one chat. The model teaches and decides when to set
// an exercise; the app grades every answer through the same path as Practice.
// POST /api/tutor/start opens a session; every later action is one POST
// /api/tutor/turn with kind "say" | "answer" | "skip". Typed text always goes
// as "say": the server works out whether it answers the open exercise.
(function () {
  "use strict";
  var base = document.body.dataset.base || "";
  var provider = document.body.dataset.provider || "";
  var lesson = document.body.dataset.lesson || "";
  var praise = document.body.dataset.praise || t('tutor.js.correct');
  var thread = document.getElementById("thread");
  if (!thread || !provider) return;
  var say = document.getElementById("say");
  var sendBtn = document.getElementById("send");
  var composer = document.getElementById("composer");
  var charsRow = document.getElementById("chars");
  var STORE = "tutor-session";
  var PLACEHOLDER = say.placeholder;
  var session = null;
  var pending = null;      // the open exercise entry, if any
  var busy = false;
  var chars = [];
  try { chars = JSON.parse(document.body.dataset.chars || "[]"); } catch (e) { chars = []; }

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }
  // Just enough Markdown for a tutor: paragraphs, **bold**, *em*, `code`, "- " lists.
  function md(text) {
    var blocks = String(text).trim().split(/\n{2,}/);
    return blocks.map(function (b) {
      var lines = b.split("\n");
      var isList = lines.length > 0 && lines.every(function (l) { return /^\s*[-•]\s+/.test(l); });
      var inline = function (s) {
        return esc(s)
          .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
          .replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<i>$2</i>")
          .replace(/`([^`]+)`/g, "<code>$1</code>");
      };
      if (isList) return "<ul>" + lines.map(function (l) { return "<li>" + inline(l.replace(/^\s*[-•]\s+/, "")) + "</li>"; }).join("") + "</ul>";
      return "<p>" + lines.map(inline).join("<br>") + "</p>";
    }).join("");
  }
  function el(tag, cls, html) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html !== undefined) e.innerHTML = html;
    return e;
  }
  function scrollDown() {
    window.requestAnimationFrame(function () { window.scrollTo(0, document.body.scrollHeight); });
  }
  function post(path, body) {
    return fetch(base + path, {
      method: "POST", credentials: "same-origin",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (r.status === 401) { window.location.href = base + "/login"; throw new Error("login"); }
      if (r.status === 410) { window.location.href = base + "/courses"; throw new Error("course"); }
      return r.json().then(function (j) {
        if (!r.ok) { var err = new Error(j.error || ("HTTP " + r.status)); err.status = r.status; throw err; }
        return j;
      }, function () { throw new Error("HTTP " + r.status); });
    });
  }

  // ---------- the composer follows the open exercise ----------
  function setPending(q) {
    pending = q;
    var fill = q && !(q.options && q.options.length);
    var math = q && (q.type === "numeric" || q.type === "expression");
    say.placeholder = !q ? PLACEHOLDER : math ? (q.type === "numeric" ? t('tutor.js.ph_number') : t('tutor.js.ph_expression'))
      : fill ? t('tutor.js.ph_answer') : t('tutor.js.ph_choice');
    charsRow.hidden = !(fill && !math && chars.length);
  }
  chars.forEach(function (c) {
    var b = el("button", "", esc(c)); b.type = "button"; b.setAttribute("aria-label", t('tutor.js.type_char', { ch: c }));
    b.addEventListener("click", function () {
      var s = say.selectionStart || say.value.length;
      say.value = say.value.slice(0, s) + c + say.value.slice(s);
      say.focus(); say.setSelectionRange(s + c.length, s + c.length);
    });
    charsRow.appendChild(b);
  });

  // ---------- rendering entries ----------
  function addSystem(text, isError) {
    thread.appendChild(el("div", "msg system" + (isError ? " error" : ""), esc(text)));
    scrollDown();
  }
  function render(entry) {
    if (entry.kind === "tutor") thread.appendChild(el("div", "msg tutor", md(entry.text)));
    else if (entry.kind === "learner") thread.appendChild(el("div", "msg learner", esc(entry.text)));
    else if (entry.kind === "question") renderExercise(entry);
    else if (entry.kind === "answer") renderAnswer(entry);
    else if (entry.kind === "skipped") settle(entry.question_id, null);
    scrollDown();
  }
  // An exercise reads as part of the tutor's turn: same side, its own tint.
  function renderExercise(q) {
    var box = el("section", "msg tutor exercise");
    box.dataset.qid = q.question_id;
    box.dataset.testid = "tutor-exercise";
    box.appendChild(el("p", "ex-label", t('tutor.js.exercise', { title: esc(q.lesson_title) })));
    box.appendChild(el("p", "ex-prompt", esc(q.prompt)));
    // A clock to read (#1340): draw it, and its times are chips like a multiple choice.
    if (q.clock_hands && window.Clock) {
      var face = el("div", "clock-face");
      face.dataset.testid = "clock";
      box.appendChild(face);
      Clock.make(face, { hands: q.clock_hands.split(" ").map(Number) });
    }
    if (q.options && q.options.length) {
      var chips = el("div", "chips");
      q.options.forEach(function (o) {
        var b = el("button", "chip", esc(o));
        b.type = "button"; b.dataset.testid = "tutor-choice";
        b.addEventListener("click", function () { answer(q, o); });
        chips.appendChild(b);
      });
      box.appendChild(chips);
    }
    var tools = el("p", "ex-tools");
    var hint = el("span", "ex-hint", q.hints && q.hints.length ? "💡 " + esc(q.hints[0]) : "");
    hint.hidden = true;
    if (q.hints && q.hints.length) {
      var hb = el("button", "", esc(t('tutor.js.hint'))); hb.type = "button";
      hb.addEventListener("click", function () { hint.hidden = false; hb.hidden = true; scrollDown(); });
      tools.appendChild(hb);
      tools.appendChild(document.createTextNode(" · "));
    }
    var sk = el("button", "", esc(t('tutor.js.skip'))); sk.type = "button"; sk.dataset.testid = "tutor-skip";
    sk.addEventListener("click", function () { if (!busy) turn({ kind: "skip" }); });
    tools.appendChild(sk);
    box.appendChild(hint);
    box.appendChild(tools);
    thread.appendChild(box);
    setPending(q);
    if (window.innerWidth > 700) say.focus();
  }
  // Freeze an exercise once it is answered or skipped.
  function settle(qid, outcome) {
    var box = thread.querySelector('.exercise[data-qid="' + qid + '"]');
    if (box) {
      box.classList.add("done"); box.dataset.state = "done";
      box.querySelectorAll("button").forEach(function (b) { b.disabled = true; });
      var tools = box.querySelector(".ex-tools");
      if (tools) tools.remove();
      if (outcome) {
        box.querySelectorAll(".chip").forEach(function (b) {
          var t = b.textContent;
          var st = t === outcome.correct_answer ? "right" : t === outcome.answer && !outcome.correct ? "wrong" : "";
          if (st) { b.classList.add(st); b.dataset.state = st; }
        });
      }
    }
    if (pending && pending.question_id === qid) setPending(null);
  }
  function renderAnswer(o) {
    settle(o.question_id, o);
    var v = el("div", "verdict " + (o.correct ? "ok" : "bad"));
    v.dataset.testid = "tutor-verdict"; v.dataset.qid = o.question_id; v.dataset.state = o.correct ? "right" : "wrong";
    v.appendChild(el("b", "", o.correct ? "✓ " + esc(praise) : t('tutor.js.answer_is', { answer: esc(o.correct_answer) })));
    if (o.note) v.appendChild(el("span", "m", esc(o.note)));
    v.appendChild(el("span", "m", t('tutor.js.mastery', { title: esc(o.lesson_title), before: o.mastery_before, after: o.mastery })));
    if (o.new_words && o.new_words.length) {
      v.appendChild(el("span", "m", "📚 " + o.new_words.map(function (w) { return esc(w.term); }).join(", ")));
    }
    if (o.passed_lesson) v.appendChild(el("span", "m", t('tutor.js.lesson_passed', { title: esc(o.lesson_title) })));
    // The worn motivator's line (#568), drawn by the server.
    if (o.reward) v.insertAdjacentHTML("beforeend", o.reward);
    thread.appendChild(v);
  }
  function setScore(right, total) {
    var s = document.getElementById("score");
    if (!s) return;
    s.hidden = total === 0;
    document.getElementById("score-right").textContent = right;
    document.getElementById("score-total").textContent = total;
  }

  // ---------- turns ----------
  var typing = null, typingTimer = null;
  // A slow reply can be cancelled after this long (#890).
  var CANCEL_AFTER_MS = 10000;
  var cancelling = false;
  function setBusy(b) {
    busy = b;
    sendBtn.disabled = b;
    thread.querySelectorAll(".exercise:not(.done) button").forEach(function (x) { x.disabled = b; });
    clearTimeout(typingTimer);
    if (b) {
      typing = el("div", "typing", "<i></i><i></i><i></i>"); thread.appendChild(typing); scrollDown();
      var dots = typing;
      typingTimer = setTimeout(function () { dots.appendChild(cancelButton(function () { return true; })); scrollDown(); }, CANCEL_AFTER_MS);
    }
    else if (typing) { typing.remove(); typing = null; }
  }
  // Stop waiting for the reply (#890). The model's call runs on and its reply
  // is thrown away; a turn waiting for it comes back with an error at once.
  // `then` runs once the server let go (true: a call was cancelled).
  function cancelButton(then) {
    var b = el("button", "cancel", "✕");
    b.type = "button"; b.dataset.testid = "tutor-cancel";
    b.setAttribute("aria-label", t('tutor.js.cancel')); b.title = t('tutor.js.cancel');
    b.addEventListener("click", function () {
      b.disabled = true;
      cancelling = busy;
      post("/api/tutor/cancel", {}).then(function (r) {
        b.remove();
        then(r.cancelled);
      }, function (e) {
        cancelling = false;
        b.disabled = false;
        if (e.message !== "login") addSystem(e.message, true);
      });
    });
    return b;
  }
  function apply(resp) {
    session = resp.session;
    try { sessionStorage.setItem(STORE, session); } catch (e) { /* ignore */ }
    // The learner's own words were drawn when they were sent.
    resp.entries.filter(function (e) { return e.kind !== "learner"; }).forEach(render);
    setScore(resp.correct, resp.answered);
    if (resp.error && cancelling) addSystem(t('tutor.js.cancelled'), false);
    else if (resp.error) addSystem(t('tutor.js.could_not_reply', { error: resp.error }), true);
    cancelling = false;
    if (!resp.pending && pending) setPending(null);
    var resolved = resp.entries.some(function (e) { return e.kind === "answer" || e.kind === "skipped"; });
    if (resp.done && resolved) addSystem(t('tutor.js.last_exercise'));
  }
  function turn(body) {
    if (busy || !session) return;
    body.session = session;
    setBusy(true);
    post("/api/tutor/turn", body).then(function (r) {
      setBusy(false);
      apply(r);
    }, function (e) {
      setBusy(false);
      if (e.message === "login" || e.message === "course") return;
      addSystem(e.message, true);
      if (/no such session/.test(e.message)) start();
    });
  }
  // A tapped choice is the learner's message, too.
  function answer(q, text) {
    if (busy || !pending || q.question_id !== pending.question_id) return;
    render({ kind: "learner", text: text });
    turn({ kind: "answer", text: text });
  }
  // A turn of this learner's (in another tab, or one they left mid-reply)
  // still with the model refuses a new session (429, #382, #848): say so
  // once, with a Cancel that lets go of it (#890), then try again until it
  // is done. A 429 here is only ever that refusal; its text is in the
  // learner's language, so the status alone decides.
  var startTries = 0, startTimer = null, stillMsg = null;
  function start() {
    clearTimeout(startTimer);
    setBusy(true);
    post("/api/tutor/start", { provider: provider, lesson: lesson }).then(function (r) {
      setBusy(false);
      if (stillMsg) { stillMsg.remove(); stillMsg = null; }
      apply(r);
    }, function (e) {
      setBusy(false);
      if (e.message === "login" || e.message === "course") return;
      if (e.status === 429 && startTries++ < 40) {
        if (!stillMsg) {
          stillMsg = el("div", "msg system error", esc(e.message) + " ");
          stillMsg.dataset.testid = "tutor-still-answering";
          stillMsg.appendChild(cancelButton(function () { startTries = 0; start(); }));
          thread.appendChild(stillMsg);
          scrollDown();
        }
        startTimer = setTimeout(start, 3000);
        return;
      }
      addSystem(e.message, true);
    });
  }
  function resume(id) {
    return fetch(base + "/api/tutor/session?id=" + encodeURIComponent(id), { credentials: "same-origin" })
      .then(function (r) { if (!r.ok) throw new Error("gone"); return r.json(); })
      .then(function (s) {
        if (s.provider !== provider || (s.lesson || "") !== lesson) throw new Error("other");
        // A session whose first turn failed has nothing from the tutor: start again (#932).
        if (!s.entries.some(function (e) { return e.kind !== "learner"; })) throw new Error("empty");
        session = s.id;
        s.entries.forEach(render);
        setScore(s.correct, s.answered);
        thread.querySelectorAll(".exercise:not(.done)").forEach(function (c) { if (c.dataset.qid !== s.pending) settle(c.dataset.qid, null); });
        if (!s.pending) setPending(null);
      });
  }

  composer.addEventListener("submit", function (e) {
    e.preventDefault();
    var text = say.value.trim();
    if (!text || busy) return;
    say.value = "";
    say.style.height = "";
    render({ kind: "learner", text: text });
    turn({ kind: "say", text: text });
  });
  say.addEventListener("keydown", function (e) {
    if (e.isComposing || e.keyCode === 229) return; // Enter confirming an IME composition
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); composer.requestSubmit(); }
  });
  say.addEventListener("input", function () {
    say.style.height = "";
    say.style.height = Math.min(say.scrollHeight, 112) + "px";
  });

  var stored = null;
  try { stored = sessionStorage.getItem(STORE); } catch (e) { /* ignore */ }
  (stored ? resume(stored) : Promise.reject()).then(null, function () {
    thread.innerHTML = "";
    start();
  });
})();
