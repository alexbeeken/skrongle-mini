// The lesson editor's live preview (#841): the item picked in the editor,
// drawn as the learner sees it in Study, Sprint or the Guide, on a phone-
// sized screen beside the form. It draws again as the item is typed, and
// can be played: a quiz question is graded by the server's own checker
// (POST /api/courses/{id}/preview-answer, nothing recorded), a Sprint
// exercise by the same rules the session uses.
//
//   LessonPreview.modes(sel, doc) -> ["study", "sprint", "guide"] (those that show this kind)
//   LessonPreview.render(el, { doc, sel, mode, man, api, termLang, step })
(function () {
  "use strict";
  // Every text of the editor is under editor.js in i18n/<code>.yaml (the keys are built, so cargo test i18n cannot scan them; en.yaml is checked by hand).
  var PREFIX = "editor.js.";
  var T = function (key, args) { return t(PREFIX + key, args); };
  var LE = function () { return window.LessonEditor; };
  var SRC = (document.currentScript && document.currentScript.src) || "";

  function h(tag, attrs, kids) {
    var el = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (a) {
      if (a === "text") el.textContent = attrs[a];
      else if (a === "html") el.innerHTML = attrs[a];
      else if (attrs[a] !== null && attrs[a] !== false && attrs[a] !== undefined) el.setAttribute(a, attrs[a] === true ? "" : attrs[a]);
    });
    (kids || []).forEach(function (k) { if (k) el.appendChild(typeof k === "string" ? document.createTextNode(k) : k); });
    return el;
  }
  function esc(s) { return String(s == null ? "" : s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); }
  var words = function (s) { return String(s == null ? "" : s).split(/\s+/).filter(Boolean); };
  function norm(s) { return LE() ? LE().norm(s) : String(s || "").trim().toLowerCase(); }
  // A fixed shuffle (the same order for the same words), so the preview holds still while typing.
  function shuffle(arr, seedText) {
    var seed = 7;
    String(seedText || "").split("").forEach(function (c) { seed = (seed * 31 + c.charCodeAt(0)) % 100003; });
    var out = arr.slice();
    for (var i = out.length - 1; i > 0; i--) {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      var j = seed % (i + 1);
      var tmp = out[i]; out[i] = out[j]; out[j] = tmp;
    }
    return out;
  }
  function pick(list, one) { Array.prototype.forEach.call(list, function (x) { x.setAttribute("aria-pressed", x === one ? "true" : "false"); x.classList.toggle("sel", x === one); }); }
  function mark(el, state) { el.classList.remove("right", "wrong"); el.classList.add(state); el.dataset.state = state; }
  function praise(man, ok) {
    var s = man && man.strings;
    var list = ok ? (s && s.praise && s.praise.length ? s.praise : [T("praise")]) : (s && s.almost && s.almost.length ? s.almost : [T("almost")]);
    return list[0];
  }
  // A little Markdown, for cards: headings, paragraphs, bold, italics,
  // code, lists, quotes, tables and fences. Enough to read a lesson by.
  function md(text) {
    var inline = function (s) {
      return esc(s).replace(/`([^`]+)`/g, "<code>$1</code>").replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>").replace(/(^|[^*])\*([^*]+)\*/g, "$1<em>$2</em>").replace(/\[([^\]]+)\]\(([^)]+)\)/g, "$1");
    };
    var out = [], para = [], list = null, fence = null, table = null, quote = [];
    var flush = function () {
      if (para.length) { out.push("<p>" + inline(para.join(" ")) + "</p>"); para = []; }
      if (list) { out.push("<" + list.tag + ">" + list.items.map(function (x) { return "<li>" + inline(x) + "</li>"; }).join("") + "</" + list.tag + ">"); list = null; }
      if (table) { out.push("<table><tbody>" + table.map(function (r, i) { return "<tr>" + r.map(function (c) { return (i ? "<td>" : "<th>") + inline(c) + (i ? "</td>" : "</th>"); }).join("") + "</tr>"; }).join("") + "</tbody></table>"); table = null; }
      if (quote.length) { out.push("<blockquote>" + inline(quote.join(" ")) + "</blockquote>"); quote = []; }
    };
    String(text || "").split("\n").forEach(function (line) {
      if (fence !== null) { if (line.trim().indexOf("```") === 0) { out.push("<pre><code>" + esc(fence.join("\n")) + "</code></pre>"); fence = null; } else fence.push(line); return; }
      if (line.trim().indexOf("```") === 0) { flush(); fence = []; return; }
      var m;
      if ((m = /^(#{1,6})\s+(.*)$/.exec(line))) { flush(); out.push("<h" + (m[1].length + 1) + ">" + inline(m[2]) + "</h" + (m[1].length + 1) + ">"); return; }
      if (/^\s*\|.*\|\s*$/.test(line)) { if (/^\s*\|[\s:|-]+\|\s*$/.test(line)) return; if (!table) { flush(); table = []; } table.push(line.trim().replace(/^\||\|$/g, "").split("|").map(function (c) { return c.trim(); })); return; }
      if ((m = /^\s*[-*]\s+(.*)$/.exec(line))) { if (!list || list.tag !== "ul") { flush(); list = { tag: "ul", items: [] }; } list.items.push(m[1]); return; }
      if ((m = /^\s*\d+[.)]\s+(.*)$/.exec(line))) { if (!list || list.tag !== "ol") { flush(); list = { tag: "ol", items: [] }; } list.items.push(m[1]); return; }
      if ((m = /^\s*>\s?(.*)$/.exec(line))) { if (para.length || list || table) flush(); quote.push(m[1]); return; }
      if (!line.trim()) { flush(); return; }
      if (list || table || quote.length) flush();
      para.push(line.trim());
    });
    flush();
    return out.join("");
  }

  // ---------- which modes show a kind ----------
  function modes(sel, doc) {
    var sec = sel.sec, sub = sel.sub;
    if (sec === "quiz") return ["study", "sprint", "guide"];
    if (sec === "vocabulary") return ["guide", "sprint"];
    if (sec === "listening") return ["study"];
    if (sec === "conversations") return ["sprint"];
    if (sec === "conjugations") return ["study", "sprint"];
    if (sec === "sprint") return ["sprint"];
    if (sec === "guide") return ["guide"];
    if (sec === "lesson" || sec === "content") return ["study"];
    return [];
  }

  // ---------- pieces ----------
  function phone(mode, kids, bar) {
    return h("div", { class: "le-phone " + mode, "data-testid": "course-preview-phone", "data-mode": mode }, [
      bar ? h("div", { class: "lp-bar" }, [h("span", { text: bar.left || "" }), h("span", { text: bar.right || "" })]) : null,
      h("div", { class: "lp-screen" }, kids)
    ]);
  }
  function note(text) { return h("p", { class: "le-preview-note", text: text }); }
  function tryBtn(label) { return h("button", { type: "submit", class: "btn", "data-testid": "course-preview-check", text: label || T("check") }); }

  // A question's music: `abc` (#1218) drawn by notation.js, `play` (#1219) a
  // speaker from music-sound.js, a keyboard (#1231) from keyboard.js, a fill-bar
  // question (#1230) built by fill-bar.js and a chord-spell question (#1228) built
  // by chord-spell.js. They and abcjs load the first time a previewed question has music.
  var notationWaiting = null;
  function whenNotation(f) {
    if (window.drawStaff && window.MusicSound && window.PianoKeys && window.FillBar && window.ChordSpell) { setTimeout(f, 0); return; }
    if (!notationWaiting) {
      notationWaiting = [];
      var names = [["vendor/abcjs-basic-min.js", "ABCJS"], ["notation.js", "drawStaff"], ["music-sound.js", "MusicSound"], ["keyboard.js", "PianoKeys"], ["fill-bar.js", "FillBar"], ["chord-spell.js", "ChordSpell"]].filter(function (n) { return !window[n[1]]; });
      var add = function () {
        var n = names.shift();
        if (!n) { notationWaiting.forEach(function (w) { w(); }); return; }
        var sc = document.createElement("script"); sc.src = SRC.replace("lesson-preview.js", n[0]); sc.onload = add; document.head.appendChild(sc);
      };
      add();
    }
    notationWaiting.push(f);
  }
  function staffBox(q) {
    if ((!q.abc && !q.play) || q.type === "fill-bar") return null;
    var box = h("div", { class: "lp-staff" });
    whenNotation(function () {
      if (!box.isConnected) return;
      if (q.abc) window.drawStaff(box.appendChild(h("div")), String(q.abc));
      if (q.play) window.MusicSound.speaker(box.appendChild(h("div")), String(q.play));
    });
    return box;
  }
  // A fill-bar question's bar and tiles; onChange(answer) once the gap is full.
  function fillBar(q, onChange) {
    var box = h("div", { class: "fill-bar", "data-testid": "course-preview-fill-bar" });
    var w = { lock: function () {}, reveal: function () {} };
    whenNotation(function () { w = window.FillBar.mount(box, String(q.abc || ""), (q.tiles || []).map(String), onChange); });
    return { el: box, lock: function () { w.lock(); }, reveal: function (c) { w.reveal(c); } };
  }
  // A chord-spell question's staff, slots and tiles (shown in ABC: the
  // course's note names are applied by the server on the real pages);
  // onChange(answer) once every slot is full.
  function chordSpell(q, onChange) {
    var box = h("div", { class: "chord-spell", "data-testid": "course-preview-chord-spell" });
    var tiles = (Array.isArray(q.tiles) ? q.tiles : words(q.tiles)).map(function (x) { return { abc: String(x), name: String(x) }; });
    var w = { lock: function () {} };
    whenNotation(function () { w = window.ChordSpell.mount(box, tiles, words(q.correct_answer).length, onChange); });
    return { el: box, lock: function () { w.lock(); } };
  }

  // A staff-note question (#1226), built by staff-note.js, which loads the
  // first time a previewed question is one. Notes are named in ABC here (the
  // course's note names are applied by the server on the real pages);
  // onChange(answer) once a note is placed.
  var staffNoteWaiting = null;
  function staffNote(q, onChange) {
    var box = h("div", { class: "staff-note", "data-testid": "course-preview-staff-note" });
    var w = { lock: function () {}, reveal: function () {} };
    var go = function () { w = window.StaffNote.mount(box, q.clef || "treble", {}, onChange); };
    if (window.StaffNote) setTimeout(go, 0);
    else if (staffNoteWaiting) staffNoteWaiting.push(go);
    else {
      staffNoteWaiting = [go];
      var sc = document.createElement("script"); sc.src = SRC.replace("lesson-preview.js", "staff-note.js");
      sc.onload = function () { staffNoteWaiting.forEach(function (f) { f(); }); };
      document.head.appendChild(sc);
    }
    return { el: box, done: function (res, fb) { w.lock(res.correct ? "right" : "wrong"); w.reveal(fb); } };
  }
  // A keyboard question's keys; onKey(abc) after each tap. after(res) lights up the right key.
  function keyboardBox(q, onKey) {
    var box = h("div", { class: "keyboard", "data-testid": "course-preview-keyboard" }), w = null;
    whenNotation(function () { w = window.PianoKeys.mount(box, { octaves: Number(q.octaves) === 2 ? 2 : 1, onKey: onKey }); });
    return { el: box, after: function (res) { if (w) { w.lock(); w.reveal(res.key || String(q.correct_answer || ""), !!q.octave); } } };
  }

  // A clock question (#1340, static/clock.js): what Clock.make needs, with
  // `hands` when it is turned around (read it, pick from the options).
  function clockOf(q) {
    if (q.type !== "clock" || !window.Clock) return null;
    return (q.options || []).length ? { hands: Clock.hands(q.correct_answer) || [0, 0] } : { step: q.step_minutes || 5 };
  }

  // A quiz question in the Study / Guide skin: the real .question markup.
  function studyQuestion(o, q, guide) {
    var math = q.type === "numeric" || q.type === "expression" || q.type === "algebra-tiles";
    var form = h("form", { class: "question", "data-testid": "course-preview-question", "data-kind": q.type }, [
      guide ? h("p", { class: "step-label", text: T("quick_check") }) : null,
      staffBox(q),
      h("p", { class: "prompt", lang: o.termLang || null, text: q.prompt || "…" })
    ]);
    var answer = function () { return null; };
    var clock = clockOf(q);
    if (clock && clock.hands) Clock.make(form.appendChild(h("div", { class: "clock-face", "data-testid": "course-preview-clock" })), clock);
    if (clock && !clock.hands) {
      var ck = Clock.make(form.appendChild(h("div", { class: "clock-face", "data-testid": "course-preview-clock" })), clock);
      answer = function () { return ck.time(); };
    } else if (q.type === "multiple-choice" || q.type === "true-false" || clock) {
      var opts = q.type === "true-false" ? ["true", "false"] : (q.options || []);
      var labels = q.type === "true-false" ? [T("true"), T("false")] : opts;
      var box = h("div", { class: "options" }, opts.map(function (opt, i) {
        return h("label", { class: "option" }, [h("input", { type: "radio", name: "lp-answer", value: opt, "data-testid": "course-preview-option" }), h("span", { lang: o.termLang || null, text: labels[i] })]);
      }));
      form.appendChild(box);
      answer = function () { var r = form.querySelector("input[type=radio]:checked"); return r ? r.value : null; };
    } else if (q.type === "fill-bar") {
      var filled = null, bar = fillBar(q, function (a) { filled = a; });
      form.appendChild(bar.el);
      answer = function () { return filled; };
    } else if (q.type === "chord-spell") {
      var spelled = null, chord = chordSpell(q, function (a) { spelled = a; });
      form.appendChild(chord.el);
      answer = function () { return spelled; };
    } else if (q.type === "staff-note") {
      var placed = null, staff = staffNote(q, function (a) { placed = a; });
      form.appendChild(staff.el);
      answer = function () { return placed; };
    } else if (q.type === "keyboard") {
      var tapped = null, keys = keyboardBox(q, function (a) { tapped = a; });
      form.appendChild(keys.el);
      answer = function () { return tapped; };
    } else {
      var inp = h("input", { class: "fill", type: "text", "data-testid": "course-preview-answer", maxlength: "200", autocomplete: "off", autocapitalize: "off", spellcheck: "false", lang: math ? null : (o.termLang || null), placeholder: q.type === "numeric" ? T("ph_number") : q.type === "expression" ? T("ph_expression") : T("ph_text") });
      form.appendChild(inp);
      if (q.unit) form.appendChild(h("span", { class: "unit", text: " " + q.unit }));
      var chars = !math && o.man && Array.isArray(o.man.chars) ? o.man.chars : [];
      if (chars.length) form.appendChild(h("div", { class: "accents" }, chars.map(function (ch) {
        var b = h("button", { type: "button", "data-ch": ch, text: ch });
        b.addEventListener("click", function () { var s = inp.selectionStart || 0; inp.value = inp.value.slice(0, s) + ch + inp.value.slice(inp.selectionEnd || 0); inp.focus(); inp.setSelectionRange(s + 1, s + 1); });
        return b;
      })));
      answer = function () { return inp.value; };
    }
    if (q.hints && q.hints.length) form.appendChild(h("details", { class: "hints" }, [h("summary", { text: T("hint") }), h("ul", {}, q.hints.map(function (x) { return h("li", { text: x }); }))]));
    var check = tryBtn();
    form.appendChild(h("div", { class: "actions" }, [check]));
    var fb = h("div", { class: "feedback", role: "status", "data-testid": "course-preview-feedback", hidden: true });
    form.appendChild(fb);
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      var a = answer();
      if (a == null || !String(a).trim()) return;
      check.disabled = true;
      o.api("preview-answer", { question: q, answer: String(a) }).then(function (res) {
        fb.className = "feedback " + (res.correct ? "ok" : "bad");
        fb.dataset.state = res.correct ? "right" : "wrong";
        fb.textContent = "";
        fb.appendChild(h("p", { class: "verdict", text: praise(o.man, res.correct) }));
        if (res.note) fb.appendChild(h("p", { text: res.note }));
        if (!res.correct) fb.appendChild(h("p", { html: T("answer_html", { answer: esc(res.correct_answer) }) }));
        if (res.explanation) fb.appendChild(h("p", { text: res.explanation }));
        form.querySelectorAll(".option").forEach(function (lab) { var r = lab.querySelector("input"); if (r.value === res.correct_answer) lab.classList.add("correct"); else if (r.checked && !res.correct) lab.classList.add("wrong"); });
        if (bar) { bar.lock(); bar.reveal(fb); }
        if (chord) chord.lock();
        if (staff) staff.done(res, fb);
        if (keys) keys.after(res);
        var again = h("button", { type: "button", class: "btn ghost small", "data-testid": "course-preview-again", text: T("try_again") });
        again.addEventListener("click", function () { o.redraw(); });
        fb.appendChild(h("p", { class: "actions" }, [again]));
        fb.hidden = false;
      }).catch(function (e) { fb.className = "feedback bad"; fb.textContent = e.message; fb.hidden = false; check.disabled = false; });
    });
    return form;
  }

  // The Sprint skin: the session's markup (static/sprint.js), scoped to the phone.
  function sprintFrame(o, body, label, submit) {
    var foot = h("div", { class: "sprint-foot" });
    var check = h("button", { type: "button", class: "sprint-btn big", "data-testid": "course-preview-check", disabled: true, text: label || T("check") });
    foot.appendChild(check);
    var result = h("div", { class: "sprint-result", "data-testid": "course-preview-feedback", hidden: true });
    var api = {
      ready: function (ok) { check.disabled = !ok; },
      label: function (txt) { check.textContent = txt; },
      show: function (res, extra) {
        result.className = "sprint-result " + (res.correct ? "ok" : "bad");
        result.dataset.state = res.correct ? "right" : "wrong";
        result.textContent = "";
        result.appendChild(h("p", { class: "sprint-verdict", text: praise(o.man, res.correct) }));
        if (res.note) result.appendChild(h("p", { text: res.note }));
        if (!res.correct && res.correct_answer && !(extra && extra.noKey)) result.appendChild(h("p", { html: T("answer_html", { answer: esc(res.correct_answer) }) }));
        if (extra && extra.slips) result.appendChild(h("p", { text: extra.slips }));
        if (res.explanation) result.appendChild(h("p", { text: res.explanation }));
        var again = h("button", { type: "button", class: "sprint-btn", "data-testid": "course-preview-again", text: T("try_again") });
        again.addEventListener("click", function () { o.redraw(); });
        result.appendChild(again);
        result.hidden = false;
        foot.hidden = true;
      }
    };
    check.addEventListener("click", function () { check.disabled = true; submit(api); });
    var stage = h("div", { class: "sprint-stage", "data-testid": "course-preview-stage" }, body);
    return { el: [stage, result, foot], api: api, stage: stage };
  }
  function sprintQuestion(o, q) {
    var body = [], api;
    var chosen = null;
    var grade = function () { o.api("preview-answer", { question: q, answer: String(chosen) }).then(function (res) { after(res); api.show(res); }).catch(function (e) { api.show({ correct: false, note: e.message }, { noKey: true }); }); };
    var after = function () {};
    var clock = clockOf(q);
    if (clock && !clock.hands) {
      body.push(h("p", { class: "sprint-kicker", text: T("set_clock") }));
      body.push(h("p", { class: "sprint-prompt", lang: o.termLang || null, text: q.prompt || "…" }));
      var box = h("div", { class: "sprint-clock", "data-testid": "course-preview-clock" });
      body.push(box);
      Clock.make(box, { step: clock.step, onChange: function (time) { chosen = time; } });
      setTimeout(function () { api.ready(true); }, 0);
    } else if (q.type === "multiple-choice" || q.type === "true-false" || clock) {
      var opts = q.type === "true-false" ? ["true", "false"] : (q.options || []);
      var labels = q.type === "true-false" ? [T("true"), T("false")] : opts;
      body.push(h("p", { class: "sprint-kicker", text: T("pick_right") }));
      body.push(staffBox(q));
      body.push(h("p", { class: "sprint-prompt", lang: o.termLang || null, text: q.prompt || "…" }));
      if (clock) { var face = h("div", { class: "sprint-clock", "data-testid": "course-preview-clock" }); body.push(face); Clock.make(face, clock); }
      var list = h("div", { class: "sprint-options" }, opts.map(function (opt, i) {
        var b = h("button", { type: "button", class: "sprint-opt", "data-value": opt, "data-testid": "course-preview-option" }, [h("span", { class: "key", text: String(i + 1) }), h("span", { lang: o.termLang || null, text: labels[i] })]);
        b.addEventListener("click", function () { pick(list.querySelectorAll(".sprint-opt"), b); chosen = opt; api.ready(true); });
        return b;
      }));
      body.push(list);
      after = function (res) { list.querySelectorAll(".sprint-opt").forEach(function (x) { x.disabled = true; if (x.dataset.value === res.correct_answer) mark(x, "right"); else if (x.dataset.value === chosen && !res.correct) mark(x, "wrong"); }); };
    } else if (q.type === "fill-bar") {
      body.push(h("p", { class: "sprint-kicker", text: T("fill_gap") }));
      body.push(h("p", { class: "sprint-prompt", text: q.prompt || "…" }));
      var bar = fillBar(q, function (a) { chosen = a; api.ready(!!a); });
      body.push(bar.el);
      after = function () { bar.lock(); };
    } else if (q.type === "chord-spell") {
      body.push(h("p", { class: "sprint-kicker", text: T("spell_chord") }));
      body.push(h("p", { class: "sprint-prompt", text: q.prompt || "…" }));
      var chord = chordSpell(q, function (a) { chosen = a; api.ready(!!a); });
      body.push(chord.el);
      after = function () { chord.lock(); };
    } else if (q.type === "staff-note") {
      body.push(h("p", { class: "sprint-kicker", text: T("place_note") }));
      body.push(h("p", { class: "sprint-prompt", text: q.prompt || "…" }));
      var staff = staffNote(q, function (a) { chosen = a; api.ready(!!a); });
      body.push(staff.el);
      after = function (res) { staff.done(res, null); };
    } else if (q.type === "keyboard") {
      body.push(h("p", { class: "sprint-kicker", text: T("find_key") }));
      body.push(staffBox(q));
      body.push(h("p", { class: "sprint-prompt", text: q.prompt || "…" }));
      var kb = keyboardBox(q, function (a) { chosen = a; api.ready(true); });
      body.push(kb.el);
      after = kb.after;
    } else {
      var math = q.type === "numeric" || q.type === "expression";
      body.push(h("p", { class: "sprint-kicker", text: math ? T("write_answer") : T("fill_gap") }));
      body.push(staffBox(q));
      var prompt = h("p", { class: "sprint-prompt", lang: math ? null : (o.termLang || null) });
      var slot = h("span", { class: "sprint-slot", html: "&nbsp;" });
      var text = q.prompt || "…";
      if (text.indexOf("___") >= 0) { var parts = text.split("___"); prompt.appendChild(document.createTextNode(parts[0])); prompt.appendChild(slot); prompt.appendChild(document.createTextNode(parts.slice(1).join("___"))); }
      else { prompt.appendChild(document.createTextNode(text)); prompt.appendChild(h("br")); prompt.appendChild(slot); }
      if (q.unit) prompt.appendChild(h("span", { class: "sprint-unit", text: " " + q.unit }));
      body.push(prompt);
      // Tiles for a fill-in: the answer among words of the lesson (the session draws them the same way).
      var tiles = [];
      if (q.type === "fill-in-blank" && String(q.correct_answer || "").trim()) {
        var pool = (o.doc.vocabulary || []).map(function (v) { return v.term; }).concat((o.doc.quiz || []).filter(function (x) { return x !== q && x.type === "fill-in-blank"; }).map(function (x) { return x.correct_answer; })).filter(function (w) { return w && norm(w) !== norm(q.correct_answer); });
        tiles = [q.correct_answer].concat(shuffle(pool, q.prompt).slice(0, 3));
        tiles = shuffle(tiles, q.correct_answer + q.prompt);
      }
      var bank = h("div", { class: "sprint-bank" }, tiles.map(function (w) {
        var b = h("button", { type: "button", class: "sprint-tile", lang: o.termLang || null, text: w, "data-testid": "course-preview-tile" });
        b.addEventListener("click", function () { pick(bank.querySelectorAll(".sprint-tile"), b); chosen = w; slot.textContent = w; slot.classList.add("filled"); api.ready(true); });
        return b;
      }));
      if (tiles.length) body.push(bank);
      var inp = h("input", { class: "sprint-input", type: "text", autocomplete: "off", autocapitalize: "off", spellcheck: "false", placeholder: T("type_it"), "data-testid": "course-preview-answer", lang: math ? null : (o.termLang || null) });
      inp.addEventListener("input", function () { chosen = inp.value; slot.textContent = inp.value || " "; slot.classList.toggle("filled", !!inp.value); api.ready(!!inp.value.trim()); });
      var wrap = h("div", { class: "sprint-type", hidden: tiles.length > 0 }, [inp]);
      body.push(wrap);
      if (tiles.length) {
        var toggle = h("button", { type: "button", class: "sprint-link", text: T("type_instead") });
        toggle.addEventListener("click", function () { wrap.hidden = !wrap.hidden; bank.hidden = !wrap.hidden; chosen = null; slot.textContent = " "; slot.classList.remove("filled"); api.ready(false); if (!wrap.hidden) inp.focus(); });
        body.push(toggle);
      }
      after = function (res) { bank.querySelectorAll(".sprint-tile").forEach(function (x) { x.disabled = true; if (x.textContent === res.correct_answer) mark(x, "right"); else if (x.textContent === chosen && !res.correct) mark(x, "wrong"); }); mark(slot, res.correct ? "right" : "wrong"); };
    }
    if (q.hints && q.hints.length) body.push(h("details", { class: "sprint-hint" }, [h("summary", { text: "💡 " + T("hint") }), h("p", { text: q.hints[0] })]));
    var f = sprintFrame(o, body, null, function (a) { api = a; if (chosen == null || !String(chosen).trim()) return; grade(); });
    api = f.api;
    return f.el;
  }

  // ---------- kinds ----------
  function viewLesson(o) {
    var d = o.doc;
    var pre = Array.isArray(d.prerequisites) ? d.prerequisites : [];
    return phone("study", [
      h("p", { class: "crumbs", text: T("crumb_lessons") + " › " + (d.unit || "") }),
      h("h1", { class: "lp-h1", text: d.title || T("untitled") }),
      d.summary ? h("p", { class: "lead", text: d.summary }) : null,
      h("div", { class: "pills-row" }, [h("span", { class: "pill untested", text: T("pill_new") })]),
      pre.length ? h("p", { class: "small" }, [h("strong", { text: T("before_this") + " "}), pre.map(function (id) { var l = (o.lessons || []).filter(function (x) { return x.id === id; })[0]; return l ? l.title : id; }).join(", ")]) : null,
      h("p", { class: "muted small", text: (d.focus ? T("focus_line", { focus: d.focus }) + " · " : "") + tn("editor.js.quiz_questions", (d.quiz || []).length) })
    ], { left: d.title || "", right: "" });
  }
  function viewContent(o) {
    var d = o.doc;
    return phone("study", [
      h("h1", { class: "lp-h1", text: d.title || T("untitled") }),
      h("div", { class: "prose", lang: o.termLang || null, html: md(d.content) })
    ], { left: T("mode_study"), right: tn("editor.js.cards", LE() ? LE().headings(d.content).length + (/\S/.test(String(d.content || "").split(/^## /m)[0] || "") ? 1 : 0) : 0) });
  }
  function viewVocab(o, v) {
    if (o.mode === "sprint") {
      var others = shuffle((o.doc.vocabulary || []).filter(function (x) { return x !== v && x.term; }).map(function (x) { return x.term; }), v.meaning).slice(0, 3);
      var opts = shuffle([v.term].concat(others), v.term);
      var chosen = null, api;
      var list = h("div", { class: "sprint-options" }, opts.map(function (opt, i) {
        var b = h("button", { type: "button", class: "sprint-opt", "data-value": opt, "data-testid": "course-preview-option" }, [h("span", { class: "key", text: String(i + 1) }), h("span", { lang: o.termLang || null, text: opt })]);
        b.addEventListener("click", function () { pick(list.querySelectorAll(".sprint-opt"), b); chosen = opt; api.ready(true); });
        return b;
      }));
      var f = sprintFrame(o, [h("p", { class: "sprint-kicker", text: T("in_lang_this_is", { lang: (o.man && o.man.terms && o.man.terms.label) || T("course_language") }) }), h("p", { class: "sprint-prompt", text: v.meaning || "…" }), list], null, function (a) {
        api = a; if (chosen == null) return;
        var ok = chosen === v.term;
        list.querySelectorAll(".sprint-opt").forEach(function (x) { x.disabled = true; if (x.dataset.value === v.term) mark(x, "right"); else if (x.dataset.value === chosen && !ok) mark(x, "wrong"); });
        api.show({ correct: ok, correct_answer: v.term });
      });
      api = f.api;
      return phone("sprint", f.el, { left: T("mode_sprint"), right: "" });
    }
    var card = h("li", { class: "flashcard", tabindex: "0", "data-testid": "course-preview-flashcard" }, [
      h("span", { class: "front", lang: o.termLang || null, text: v.term || "…" }),
      h("span", { class: "back" }, [v.meaning || "…", v.example ? h("em", { lang: o.termLang || null, title: v.example_meaning || null, text: v.example }) : null])
    ]);
    card.addEventListener("click", function () { card.classList.toggle("flipped"); });
    return phone("guide", [
      h("p", { class: "step-label", text: T("terms") }),
      h("h2", { class: "lp-h2", text: T("flashcards_title") }),
      h("p", { class: "muted", text: T("tap_reveal") }),
      h("ul", { class: "flashcards" }, [card]),
      h("div", { class: "actions" }, [h("button", { type: "button", class: "btn big", disabled: true, text: T("next") })])
    ], { left: T("mode_guide"), right: "" });
  }
  function viewQuiz(o, q) {
    if (o.mode === "sprint") return phone("sprint", sprintQuestion(o, q), { left: "🎭 " + T("mode_sprint"), right: "" });
    var n = (o.doc.quiz || []).indexOf(q) + 1;
    return phone(o.mode, [studyQuestion(o, q, o.mode === "guide")], { left: o.mode === "guide" ? T("mode_guide") : (o.doc.title || ""), right: n + " / " + (o.doc.quiz || []).length });
  }
  function viewListening(o, item) {
    var inp = h("input", { class: "fill", type: "text", autocomplete: "off", autocapitalize: "off", spellcheck: "false", placeholder: T("ph_heard"), "data-testid": "course-preview-answer", lang: o.termLang || null });
    var speaker = h("div", { class: "num-speaker lp-speaker", role: "button", tabindex: "0", title: T("hear_it"), text: "🔊" });
    var sayIt = function () { if (!window.speechSynthesis || !item.text) return; var u = new SpeechSynthesisUtterance(item.text); if (o.termLang) u.lang = o.termLang; speechSynthesis.cancel(); speechSynthesis.speak(u); };
    speaker.addEventListener("click", sayIt);
    var fb = h("div", { class: "feedback", role: "status", "data-testid": "course-preview-feedback", hidden: true });
    var check = tryBtn();
    var form = h("form", { class: "question" }, [inp, h("div", { class: "actions" }, [check, h("button", { type: "button", class: "btn ghost", text: T("replay") })]), fb]);
    form.querySelector(".btn.ghost").addEventListener("click", sayIt);
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      var want = words(item.text), got = words(inp.value), all = true;
      var line = h("p", { class: "dictation" });
      want.forEach(function (w, i) {
        var g = got[i], state = g == null ? "missing" : norm(g) === norm(w) ? "ok" : (LE() && stripAccents(norm(g)) === stripAccents(norm(w)) ? "accent" : "wrong");
        if (state !== "ok") all = false;
        line.appendChild(h("span", { class: "w " + state, text: w })); line.appendChild(document.createTextNode(" "));
      });
      got.slice(want.length).forEach(function (g) { all = false; line.appendChild(h("span", { class: "w extra", text: g })); });
      fb.className = "feedback " + (all ? "ok" : "bad"); fb.dataset.state = all ? "right" : "wrong"; fb.textContent = "";
      fb.appendChild(h("p", { class: "verdict", text: praise(o.man, all) }));
      fb.appendChild(line);
      if (item.meaning) fb.appendChild(h("p", { class: "meaning", text: item.meaning }));
      if (item.note) { var m = /^New word: (.+?) = (.+)$/.exec(item.note); fb.appendChild(h("p", { class: "note" }, m ? ["New word: ", h("mark", { text: m[1] }), " = " + m[2]] : [item.note])); }
      fb.hidden = false; check.disabled = true;
    });
    return phone("study", [speaker, h("p", { class: "muted small", text: T("listen_status") }), form], { left: T("sec_listening"), right: ((o.doc.listening || []).indexOf(item) + 1) + " / " + (o.doc.listening || []).length });
  }
  function stripAccents(s) { return String(s).replace(/[áàäâ]/g, "a").replace(/[éèëê]/g, "e").replace(/[íìïî]/g, "i").replace(/[óòöô]/g, "o").replace(/[úùüû]/g, "u").replace(/ñ/g, "n").replace(/ç/g, "c"); }
  function viewConversation(o, c) {
    var lines = (c.lines || []).filter(function (l) { var n = words(l.text).length; return n >= 3 && n <= 9 && String(l.meaning || "").trim(); });
    if (!lines.length) return phone("sprint", [h("p", { class: "sprint-kicker", text: T("build_sentence") }), h("p", { class: "sprint-sub", text: T("conv_no_build") })], { left: "🎭 " + T("mode_sprint"), right: "" });
    var line = lines[0];
    return phone("sprint", buildExercise(o, { text: line.text, meaning: line.meaning, distractors: [] }), { left: "🎭 " + T("mode_sprint"), right: c.title || "" });
  }
  function viewConjugation(o, k) {
    var para = o.man && o.man.paradigm;
    var slots = para ? para.slots : Object.keys(k.forms || {}).map(function (x) { return { key: x, label: x }; });
    if (o.mode === "sprint") {
      var api, inputs = [];
      var ladder = h("ol", { class: "sprint-ladder" }, slots.map(function (s) {
        var inp = h("input", { class: "sprint-input", type: "text", autocomplete: "off", autocapitalize: "off", spellcheck: "false", lang: o.termLang || null, "aria-label": s.label, placeholder: "…" });
        inp.addEventListener("input", function () { api.ready(inputs.some(function (x) { return x.value.trim(); })); });
        inputs.push(inp);
        return h("li", { class: "sprint-rung" }, [h("span", { class: "sprint-rung-label", text: s.label }), inp]);
      }));
      var f = sprintFrame(o, [h("p", { class: "sprint-kicker", text: T("ladder") }), h("h2", { class: "sprint-title" }, [h("span", { lang: o.termLang || null, text: k.item || "…" }), h("small", { text: [k.meaning, k.set].filter(Boolean).join(" · ") })]), ladder], null, function (a) {
        api = a;
        var wrong = 0;
        slots.forEach(function (s, i) { var ok = norm(inputs[i].value) === norm((k.forms || {})[s.key]); inputs[i].disabled = true; mark(inputs[i], ok ? "right" : "wrong"); if (!ok) { wrong += 1; inputs[i].value = (k.forms || {})[s.key] || ""; } });
        api.show({ correct: wrong === 0 }, { noKey: true, slips: wrong ? tn("editor.js.wrong_rungs", wrong) : "" });
      });
      api = f.api;
      return phone("sprint", f.el, { left: "🎭 " + T("mode_sprint"), right: "" });
    }
    return phone("study", [
      h("h2", { class: "lp-h2", text: para ? para.label : T("sec_conjugations") }),
      k.art && k.art.length ? h("pre", { class: "ascii", text: k.art.join("\n") }) : null,
      h("table", { class: "vocab conj" }, [
        h("thead", {}, [h("tr", {}, [h("th", { text: para ? para.item_label : T("item") }), h("th", { text: T("f_set") })].concat(slots.map(function (s) { return h("th", { text: s.label }); })))]),
        h("tbody", {}, [h("tr", { lang: o.termLang || null }, [h("td", {}, [h("strong", { text: k.item || "…" }), h("br"), h("span", { class: "muted small", text: k.meaning || "" })]), h("td", { text: (para && (para.sets.filter(function (s) { return s.key === k.set; })[0] || {}).label) || k.set || "" })].concat(slots.map(function (s) { return h("td", { text: (k.forms || {})[s.key] || "" }); })))])
      ])
    ], { left: o.doc.title || "", right: "" });
  }
  function viewIcon(o) {
    return phone("sprint", [
      h("p", { class: "sprint-kicker", text: T("path_stop") }),
      h("div", { class: "lp-stop" }, [h("span", { class: "lp-stop-icon", text: (o.doc.sprint && o.doc.sprint.icon) || "⭐" }), h("span", { class: "lp-stop-title", text: o.doc.title || T("untitled") })]),
      note(T("path_stop_note"))
    ], { left: "🎭 " + T("mode_sprint"), right: "" });
  }
  function viewBite(o, b) {
    var f = sprintFrame(o, [h("article", { class: "sprint-bite" }, [
      h("p", { class: "sprint-kicker", text: T("quick_bite") }),
      h("div", { class: "sprint-bite-emoji", text: b.emoji || "💡" }),
      h("h2", { text: b.title || "…" }),
      h("p", { class: "sprint-bite-rule", text: b.rule || "" }),
      b.example ? h("p", { class: "sprint-bite-ex" }, [h("span", { lang: o.termLang || null, text: b.example }), b.example_meaning ? h("small", { text: b.example_meaning }) : null]) : null
    ])], T("got_it"), function (a) { a.show({ correct: true }, { noKey: true }); });
    f.api.ready(true);
    return phone("sprint", f.el, { left: "🎭 " + T("mode_sprint"), right: "" });
  }
  // The sort as Sprint plays it (#1148): each tap is judged at once, a wrong
  // bucket shakes the word and counts as a wrong tap, and the sort is graded
  // as soon as the pile is empty.
  function viewSort(o, s) {
    var buckets = Array.isArray(s.buckets) ? s.buckets : [], items = Array.isArray(s.items) ? s.items : [];
    var queue = shuffle(items.filter(function (x) { return x && String(x.text || "").trim(); }), s.title);
    var api, graded = false, mistakes = 0;
    var card = h("div", { class: "sprint-sort-item" });
    var left = h("p", { class: "sprint-sort-left" });
    var btns = [];
    var grade = function () {
      graded = true;
      btns.forEach(function (x) { x.disabled = true; });
      api.show({ correct: mistakes === 0, explanation: s.why || "" }, { noKey: true, slips: mistakes ? tn("editor.js.sort_slips", mistakes) : "" });
    };
    var showNext = function () {
      if (!queue.length) { card.innerHTML = '<span class="done">' + esc(T("all_sorted")) + "</span>"; left.textContent = ""; api.ready(false); grade(); return; }
      card.textContent = queue[0].text; left.textContent = T("to_go", { n: queue.length }); api.ready(false);
    };
    var cols = h("div", { class: "sprint-buckets b" + buckets.length }, buckets.map(function (name, bi) {
      var b = h("button", { type: "button", class: "sprint-bucket", "data-testid": "course-preview-bucket" }, [h("strong", { text: name || "…" })]);
      var wordsEl = h("div", { class: "sprint-bucket-words" });
      b.addEventListener("click", function () {
        if (graded || !queue.length) return;
        if (queue[0].bucket === bi) {
          var it = queue.shift();
          btns.forEach(function (x) { x.classList.remove("miss"); });
          wordsEl.appendChild(h("span", { class: "sprint-chip", "data-state": "right", "data-testid": "course-preview-chip", text: it.text }));
          showNext();
        } else {
          mistakes += 1;
          card.classList.remove("shake"); void card.offsetWidth; card.classList.add("shake");
          b.classList.remove("miss"); void b.offsetWidth; b.classList.add("miss");
        }
      });
      btns.push(b);
      return h("div", { class: "sprint-bucket-col" }, [b, wordsEl]);
    }));
    var f = sprintFrame(o, [h("p", { class: "sprint-kicker", text: T("sort_it") }), h("h2", { class: "sprint-title", text: s.title || "…" }), h("div", { class: "sprint-sort-card" }, [card]), cols, left], null, function () {
      if (!queue.length && !graded) grade();
    });
    api = f.api;
    if (queue.length) showNext();
    else { card.innerHTML = '<span class="done">' + esc(T("all_sorted")) + "</span>"; }
    return phone("sprint", f.el, { left: "🎭 " + T("mode_sprint"), right: "" });
  }
  function viewSpot(o, s) {
    // A spot on a staff (#1227) is played in Sprint mode only: the preview draws it and says the answer.
    if (s.abc) return phone("sprint", sprintFrame(o, [h("p", { class: "sprint-kicker", text: T("spot_mistake") }), h("p", { class: "sprint-sub", text: T("one_note_wrong") }), staffBox(s), note(T("spot_note_preview", { n: s.wrong, fix: s.fix }))], null, function () {}).el, { left: "🎭 " + T("mode_sprint"), right: "" });
    var ws = words(s.sentence), chosen = null, api;
    var sent = h("p", { class: "sprint-sentence", lang: o.termLang || null });
    ws.forEach(function (w, i) {
      var b = h("button", { type: "button", class: "sprint-word", text: w, "data-testid": "course-preview-word" });
      b.addEventListener("click", function () { pick(sent.querySelectorAll(".sprint-word"), b); chosen = i; api.ready(true); });
      sent.appendChild(b); sent.appendChild(document.createTextNode(" "));
    });
    var f = sprintFrame(o, [h("p", { class: "sprint-kicker", text: T("spot_mistake") }), h("p", { class: "sprint-sub", text: T("one_word_wrong") }), sent], null, function (a) {
      api = a; if (chosen == null) return;
      var wordsEl = sent.querySelectorAll(".sprint-word");
      var ok = norm(ws[chosen]) === norm(s.wrong);
      wordsEl.forEach(function (x, i) {
        x.disabled = true;
        if (norm(x.textContent) === norm(s.wrong)) { mark(x, "right"); if (s.fix) x.innerHTML = "<s>" + esc(x.textContent) + "</s> " + esc(s.fix); }
        else if (i === chosen && !ok) mark(x, "wrong");
      });
      api.show({ correct: ok, correct_answer: s.wrong + " → " + s.fix, explanation: s.why || "" });
    });
    api = f.api;
    return phone("sprint", f.el, { left: "🎭 " + T("mode_sprint"), right: "" });
  }
  function buildExercise(o, b) {
    var tiles = shuffle(words(b.text).concat((b.distractors || []).filter(Boolean)), b.text);
    var chosen = [], api;
    var answer = h("div", { class: "sprint-answer-line empty", lang: o.termLang || null, "data-empty": T("tap_tiles_in_order") });
    var bank = h("div", { class: "sprint-bank", lang: o.termLang || null }, tiles.map(function (w) {
      var tile = h("button", { type: "button", class: "sprint-tile", text: w, "data-testid": "course-preview-tile" });
      tile.addEventListener("click", function () {
        if (tile.parentNode === bank) { chosen.push(tile); answer.appendChild(tile); } else { chosen.splice(chosen.indexOf(tile), 1); bank.appendChild(tile); }
        answer.classList.toggle("empty", !chosen.length); api.ready(chosen.length > 0);
      });
      return tile;
    }));
    var f = sprintFrame(o, [h("p", { class: "sprint-kicker", text: T("build_sentence") }), h("p", { class: "sprint-prompt en", text: b.meaning || "…" }), answer, bank], null, function (a) {
      api = a; if (!chosen.length) return;
      var text = chosen.map(function (x) { return x.textContent; }).join(" ");
      var ok = norm(text) === norm(b.text);
      bank.querySelectorAll(".sprint-tile").forEach(function (x) { x.disabled = true; }); answer.querySelectorAll(".sprint-tile").forEach(function (x) { x.disabled = true; });
      mark(answer, ok ? "right" : "wrong");
      api.show({ correct: ok, correct_answer: b.text });
    });
    api = f.api;
    return f.el;
  }
  // Build the scale (#1229): the scale on a staff with a speaker, and how Sprint plays it.
  function viewScale(o, s) {
    var notes = words(s.notes), tune = "X:1\nL:1/4\nK:C\n" + notes.join(" ") + "|";
    return phone("sprint", sprintFrame(o, [h("p", { class: "sprint-kicker", text: T("scale_kicker") }), h("h2", { class: "sprint-title", text: s.title || "…" }), notes.length ? staffBox({ abc: tune, play: tune }) : null, note(T("scale_preview", { n: Math.max(0, notes.length - 1) + words(s.decoys).length }))], null, function () {}).el, { left: "🎭 " + T("mode_sprint"), right: "" });
  }
  function viewBuild(o, b) { return phone("sprint", buildExercise(o, b), { left: "🎭 " + T("mode_sprint"), right: "" }); }
  function viewGuide(o) {
    var steps = Array.isArray(o.doc.guide) ? o.doc.guide : [];
    var i = Math.min(o.step || 0, Math.max(0, steps.length - 1));
    var s = steps[i];
    var strip = h("div", { class: "lp-strip" }, steps.map(function (_, j) { return h("span", { class: j <= i ? "done" : "" }); }));
    var body;
    if (!s) body = [h("p", { class: "step-label", text: T("mode_guide") }), h("p", { class: "muted", text: T("guide_derived_preview") })];
    else if ("section" in s) {
      var hs = LE() ? LE().headings(o.doc.content) : [];
      var name = hs.filter(function (x) { return x.toLowerCase().indexOf(String(s.section).toLowerCase()) === 0; })[0] || s.section;
      var parts = String(o.doc.content || "").split(/^## /m);
      var text = parts.filter(function (p) { return p.split("\n")[0].trim().replace(/[*_`]/g, "") === name; })[0];
      body = [h("p", { class: "step-label", text: T("concept") }), h("h2", { class: "lp-h2", text: name }), h("div", { class: "prose", lang: o.termLang || null, html: md(text ? text.split("\n").slice(1).join("\n") : (String(s.section).toLowerCase() === "introduction" ? parts[0] : "")) }), h("div", { class: "actions" }, [h("button", { type: "button", class: "btn big", disabled: true, text: T("got_it") })])];
    } else if ("tip" in s) body = [h("p", { class: "step-label", text: T("tip") }), h("p", { class: "lead", text: s.tip }), h("div", { class: "actions" }, [h("button", { type: "button", class: "btn big", disabled: true, text: T("next") })])];
    else if ("concept" in s) body = [h("p", { class: "step-label", text: T("concept") }), h("h2", { class: "lp-h2", text: (s.concept && s.concept.title) || "…" }), h("div", { class: "prose", html: md(s.concept && s.concept.markdown) }), h("div", { class: "actions" }, [h("button", { type: "button", class: "btn big", disabled: true, text: T("got_it") })])];
    else if ("vocab" in s) {
      var before = steps.slice(0, i).reduce(function (n, x) { return n + (x && typeof x.vocab === "number" ? x.vocab : 0); }, 0);
      var hand = (o.doc.vocabulary || []).slice(before, before + (s.vocab || 0));
      body = [h("p", { class: "step-label", text: T("terms") }), h("h2", { class: "lp-h2", text: T("flashcards_title") }), h("ul", { class: "flashcards" }, hand.map(function (v) {
        var c = h("li", { class: "flashcard", tabindex: "0" }, [h("span", { class: "front", lang: o.termLang || null, text: v.term }), h("span", { class: "back", text: v.meaning })]);
        c.addEventListener("click", function () { c.classList.toggle("flipped"); });
        return c;
      })), hand.length ? null : h("p", { class: "muted small", text: T("hand_empty") })];
    } else if ("question" in s) {
      var q = (o.doc.quiz || []).filter(function (x) { return x.question_id === s.question; })[0];
      body = q ? [studyQuestion(o, q, true)] : [h("p", { class: "step-label", text: T("quick_check") }), h("p", { class: "muted", text: T("question_missing", { id: s.question }) })];
    } else body = [h("p", { class: "muted", text: T("guide_step_unknown") })];
    return phone("guide", [strip].concat(body), { left: T("mode_guide"), right: steps.length ? (i + 1) + " / " + steps.length : "" });
  }

  // ---------- render ----------
  function render(el, o) {
    var sel = o.sel, sec = sel.sec, sub = sel.sub;
    var path = LE() ? LE().sections : null;
    var item = sel.idx != null ? (sec === "sprint" ? (o.doc.sprint || {})[sub][sel.idx] : (o.doc[sec] || [])[sel.idx]) : null;
    // Drawn again only when what it shows changed, so a game in progress survives a keystroke elsewhere.
    var key = JSON.stringify([sec, sub, sel.idx, o.mode, o.step, item || (sec === "guide" ? o.doc.guide : sec === "content" ? [o.doc.content, o.doc.title] : sec === "lesson" ? [o.doc.title, o.doc.summary, o.doc.focus, o.doc.prerequisites, o.doc.unit] : sec === "sprint" ? (o.doc.sprint || {}).icon : null), sec === "quiz" && item ? o.doc.vocabulary : null]);
    if (el.dataset.key === key) return;
    el.dataset.key = key;
    o.redraw = function () { delete el.dataset.key; render(el, o); };
    el.textContent = "";
    var out;
    if (!o.mode) { el.appendChild(h("p", { class: "le-preview-note", "data-testid": "course-preview-none", text: T("no_preview") })); return; }
    try {
      if (sec === "lesson") out = viewLesson(o);
      else if (sec === "content") out = viewContent(o);
      else if (sec === "guide") out = viewGuide(o);
      else if (sec === "sprint" && sub === "icon") out = viewIcon(o);
      else if (!item) out = h("p", { class: "le-preview-note", text: T("no_preview") });
      else if (sec === "vocabulary") out = viewVocab(o, item);
      else if (sec === "quiz") out = viewQuiz(o, item);
      else if (sec === "listening") out = viewListening(o, item);
      else if (sec === "conversations") out = viewConversation(o, item);
      else if (sec === "conjugations") out = viewConjugation(o, item);
      else if (sub === "bites") out = viewBite(o, item);
      else if (sub === "sorts") out = viewSort(o, item);
      else if (sub === "spots") out = viewSpot(o, item);
      else if (sub === "builds") out = viewBuild(o, item);
      else if (sub === "scales") out = viewScale(o, item);
      else out = h("p", { class: "le-preview-note", text: T("no_preview") });
    } catch (e) {
      out = h("p", { class: "le-preview-note", text: T("preview_failed", { error: e.message }) });
    }
    el.appendChild(out);
    var notes = { quiz: o.mode === "sprint" ? T("note_quiz_sprint") : T("note_quiz"), vocabulary: o.mode === "sprint" ? T("note_vocab_sprint") : T("note_vocab"), listening: T("note_listening"), conversations: T("note_conversation"), conjugations: o.mode === "sprint" ? T("note_ladder") : T("note_conjugation"), sprint: sub === "sorts" ? T("note_sort") : sub === "spots" ? T("note_spot") : sub === "builds" ? T("note_build") : sub === "scales" ? T("note_scale") : sub === "bites" ? T("note_bite") : null, guide: T("note_guide"), content: T("note_content") };
    if (notes[sec]) el.appendChild(note(notes[sec]));
  }

  window.LessonPreview = { modes: modes, render: render, md: md };
})();
