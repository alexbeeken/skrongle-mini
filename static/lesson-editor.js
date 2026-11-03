// The lesson editor (#841): the drawer's Forms mode for a lesson file.
// Three columns: an outline of the file's sections and items (left), a
// hand-written form for the one item picked (middle) and a live preview of
// it as the learner sees it (right, static/lesson-preview.js). On a narrow
// drawer the columns become Outline / Edit / Preview tabs.
//
// The form edits the parsed document in place and tells the drawer
// (`onChange`) after every change, so the JSON box, autosave and Undo work
// as they do for the schema form. Fields it does not know are left as they
// are. Ids, the slug, the unit and the order are never shown: the table
// sets them, and a new item's id is made up (`<lesson>-q3`).
//
//   LessonEditor.render(el, doc, ctx)   ctx: { onChange(doc), manifest (the course settings; or course() -> Promise of them), api(path, body) -> Promise, lessons() -> [{id, title}], schema() -> Promise, toJson(), confirmDelete(), confirmDerive() }
//   LessonEditor.reveal(el, field)      pick the section a server problem names
//   LessonEditor.refresh([el])          draw the checks again (every open form, or one): the course lints of
//                                       the lesson, read from the page's report (#course-report, drawn again
//                                       after each save; or ctx.lints(id) -> [{ kind, path, text }]), show as
//                                       an amber ! in the corner of their field or of the form's title, which
//                                       opens them in a modal, so nothing on the page moves (#1199)
//   LessonEditor.select(el, sel, path)  pick one item and focus its field (static/advice.js)
(function () {
  "use strict";
  // Every text of the editor is under editor.js in i18n/<code>.yaml (the keys are built, so cargo test i18n cannot scan them; en.yaml is checked by hand).
  var PREFIX = "editor.js.";
  var T = function (key, args) { return t(PREFIX + key, args); };
  var QUIZ_TYPES = ["multiple-choice", "fill-in-blank", "true-false", "numeric", "expression", "fill-bar", "chord-spell", "staff-note", "keyboard", "algebra-tiles", "clock"];
  var BADGE = { "multiple-choice": "A·B·C", "fill-in-blank": "___", "true-false": "T/F", numeric: "123", expression: "ƒ(x)", clock: "3:45", "algebra-tiles": "▦", "fill-bar": "♩♩", "chord-spell": "♯♭", "staff-note": "♩", keyboard: "▯▮▯" };
  // Quiz types about music: offered in a course with a "music" block, or once a question is one.
  var MUSIC_TYPES = ["fill-bar", "chord-spell", "staff-note", "keyboard"];
  var ABC_NOTE = /^(\^\^|\^|__|_|=)?[A-Ga-g][,']*$/; // one note of a keyboard question (src/notation/keyboard.rs)
  var SPRINT = ["bites", "sorts", "spots", "builds", "scales"];
  var SPRINT_BADGE = { bites: "BITE", sorts: "SORT", spots: "SPOT", builds: "BUILD", scales: "SCALE", icon: "ICON" };
  var GUIDE_KINDS = ["section", "question", "vocab", "tip", "concept"];
  // The course's language features (course.json "language_features", set by
  // the gear; else on when the terms have a language of their own, as
  // `Manifest::is_language`). Off, the parts only a language course fills
  // are hidden while empty: Listening, Conversations, + Build (kept in a
  // music course, course.json "music"), an example's or a line's meaning.
  // Until the settings are known, nothing is hidden.
  function languageOn(man) {
    if (!man) return true;
    if (typeof man.language_features === "boolean") return man.language_features;
    var code = function (side) { return man[side] && man[side].lang ? String(man[side].lang).trim().toLowerCase() : ""; };
    var t = code("terms");
    return !!t && t !== code("meanings");
  }

  // ---------- helpers ----------
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
  var dotted = function (p) { return p.join("."); };
  function get(doc, path) { var v = doc; for (var i = 0; i < path.length && v != null; i++) v = v[path[i]]; return v; }
  function put(doc, path, value) {
    var v = doc;
    for (var i = 0; i < path.length - 1; i++) {
      if (v[path[i]] == null || typeof v[path[i]] !== "object") v[path[i]] = typeof path[i + 1] === "number" ? [] : {};
      v = v[path[i]];
    }
    v[path[path.length - 1]] = value;
  }
  function drop(doc, path) {
    var parent = get(doc, path.slice(0, -1));
    if (!parent || typeof parent !== "object") return;
    if (Array.isArray(parent)) parent.splice(path[path.length - 1], 1); else delete parent[path[path.length - 1]];
  }
  function parsePath(s) { return s === "" ? [] : s.split(".").map(function (x) { return /^\d+$/.test(x) ? +x : x; }); }
  var words = function (s) { return String(s == null ? "" : s).split(/\s+/).filter(Boolean); };
  // quiz::normalize: trimmed, edge punctuation off, one space between words, lower case.
  function norm(s) {
    return String(s == null ? "" : s).trim().replace(/^[.!¡?¿,;:"'’‘«»]+|[.!¡?¿,;:"'’‘«»]+$/g, "").replace(/[’‘]/g, "'").replace(/\s+/g, " ").toLowerCase();
  }
  function short(s, n) { s = String(s == null ? "" : s).replace(/\s+/g, " ").trim(); return s.length > n ? s.slice(0, n - 1) + "…" : s; }
  function isIcon(s) { s = String(s || "").trim(); var n = Array.from(s).length; return n >= 1 && n <= 4 && !/[\s<>&"]/.test(s); }
  // The `## ` headings of the content, as the loader splits them (a fenced code block is skipped).
  function headings(md) {
    var out = [], fence = false;
    String(md || "").split("\n").forEach(function (line) {
      if (line.trim().indexOf("```") === 0) { fence = !fence; return; }
      if (!fence && line.indexOf("## ") === 0) out.push(line.slice(3).trim().replace(/[*_`]/g, ""));
    });
    return out;
  }
  function hasIntro(md) {
    var first = String(md || "").split("\n").findIndex(function (l) { return l.indexOf("## ") === 0; });
    var before = first < 0 ? String(md || "") : String(md || "").split("\n").slice(0, first).join("\n");
    return before.trim() !== "";
  }
  // A made-up id, free in the document: "<lesson>-q3", "<lesson>-conv-2".
  function madeId(doc, kind) {
    var lesson = typeof doc.id === "string" && doc.id ? doc.id + "-" : "";
    var taken = JSON.stringify(doc);
    for (var n = 1; ; n++) {
      var id = lesson + (kind === "question_id" ? "q" + n : kind + "-" + n);
      if (taken.indexOf('"' + id + '"') < 0) return id;
    }
  }
  function debounce(fn, ms) { var tm; return function () { clearTimeout(tm); tm = setTimeout(fn, ms); }; }
  // A numeric or expression question's answers as the checker reads them
  // (POST answer-key, the rule a save refuses): the first it cannot read,
  // "" when it reads them all, undefined until the server has said.
  var math = { seen: {}, want: {}, api: null };
  var mathSoon = debounce(function () {
    var want = math.want;
    math.want = {};
    Object.keys(want).forEach(function (key) {
      if (key in math.seen || !math.api) return;
      math.seen[key] = undefined;
      math.api("answer-key", { question: want[key] }).then(function (r) { math.seen[key] = r && r.answer != null ? String(r.answer) : ""; }, function () { math.seen[key] = ""; }).then(function () {
        Array.prototype.forEach.call(document.querySelectorAll(".le-host"), function (host) { if (host._leRefresh) host._leRefresh(); });
      });
    });
  }, 250);
  function unreadable(q) {
    var str = function (a) { return String(a == null ? "" : a); };
    var accepted = (Array.isArray(q.accepted_answers) ? q.accepted_answers : []).map(str);
    var key = JSON.stringify([q.type, str(q.unit), str(q.correct_answer), accepted]);
    if (key in math.seen) return math.seen[key];
    math.want[key] = { question_id: "draft", prompt: "?", type: q.type, correct_answer: str(q.correct_answer), accepted_answers: accepted, unit: str(q.unit) };
    mathSoon();
  }

  // ---------- the outline ----------
  // Every section of the file, with the items a learner-facing kind has.
  // `sel` picks one: { sec, sub?, idx? }; a single-item section has no idx.
  function sections(doc, man) {
    var paradigm = man && man.paradigm, lang = languageOn(man);
    var has = function (key) { return Array.isArray(doc[key]) && doc[key].length > 0; };
    var list = function (key, kind, label) {
      var items = Array.isArray(doc[key]) ? doc[key] : [];
      return { key: key, label: label, count: items.length, items: items.map(function (it, i) { return itemRow(doc, man, { sec: key, idx: i }, it, kind); }), add: [{ sub: null, label: T("add_" + kind) }] };
    };
    var sprintItems = [];
    var sp = doc.sprint && typeof doc.sprint === "object" ? doc.sprint : {};
    if (typeof sp.icon === "string" && sp.icon.trim()) sprintItems.push(itemRow(doc, man, { sec: "sprint", sub: "icon" }, sp.icon, "icon"));
    SPRINT.forEach(function (sub) {
      (Array.isArray(sp[sub]) ? sp[sub] : []).forEach(function (it, i) { sprintItems.push(itemRow(doc, man, { sec: "sprint", sub: sub, idx: i }, it, sub)); });
    });
    var out = [
      { key: "lesson", label: T("sec_lesson"), single: true },
      { key: "content", label: T("sec_content"), single: true, count: headings(doc.content).length + (hasIntro(doc.content) ? 1 : 0), countLabel: "cards" },
      list("vocabulary", "vocab", T("sec_vocabulary")),
      list("quiz", "quiz", T("sec_quiz")),
      lang || has("listening") ? list("listening", "listening", T("sec_listening")) : null,
      { key: "sprint", label: T("sec_sprint"), count: sprintItems.length, items: sprintItems,
        add: [{ sub: "icon", label: T("add_icon"), hidden: !!(typeof sp.icon === "string" && sp.icon.trim()) }].concat(SPRINT.map(function (sub) { return { sub: sub, label: T("add_" + sub), hidden: (sub === "builds" && !lang && !(man && man.music)) || (sub === "scales" && !(man && man.music) && !(sp.scales || []).length) }; })) },
      lang || has("conversations") ? list("conversations", "conversation", T("sec_conversations")) : null,
    ].filter(Boolean);
    if (paradigm || (Array.isArray(doc.conjugations) && doc.conjugations.length)) out.push(list("conjugations", "conjugation", T("sec_conjugations")));
    var guide = Array.isArray(doc.guide) ? doc.guide : [];
    out.push({ key: "guide", label: T("sec_guide"), single: true, note: guide.length ? tn("editor.js.steps", guide.length) : T("guide_auto") });
    return out;
  }
  function itemRow(doc, man, sel, it, kind) {
    var badge = "", label = "";
    if (kind === "quiz") { badge = BADGE[it && it.type] || "?"; label = it && it.prompt; }
    else if (kind === "vocab") label = it ? [it.term, it.meaning].filter(Boolean).join(" · ") : "";
    else if (kind === "listening") label = it && it.text;
    else if (kind === "conversation") label = it && (it.title || it.id);
    else if (kind === "conjugation") label = it ? [it.item, it.set].filter(Boolean).join(" · ") : "";
    else if (kind === "icon") { badge = SPRINT_BADGE.icon; label = it; }
    else { badge = SPRINT_BADGE[kind]; label = it && (kind === "bites" || kind === "sorts" || kind === "scales" ? it.title : kind === "spots" ? it.sentence || it.abc : it.text); }
    var bad = itemChecks(doc, man, sel).some(function (c) { return c.level === "bad"; });
    return { sel: sel, badge: badge, label: short(label || T("untitled"), 60), bad: bad };
  }
  var same = function (a, b) { return !!a && !!b && a.sec === b.sec && (a.sub || null) === (b.sub || null) && (a.idx == null ? b.idx == null : a.idx === b.idx); };
  function itemPath(sel) {
    if (sel.sec === "sprint") return sel.sub === "icon" ? ["sprint", "icon"] : ["sprint", sel.sub, sel.idx];
    if (sel.idx == null) return [];
    return [sel.sec, sel.idx];
  }

  // ---------- checks ----------
  // The course rules for one item: static/lesson-checks.js.
  var itemChecks = window.LessonChecks.make({ T: T, QUIZ_TYPES: QUIZ_TYPES, ABC_NOTE: ABC_NOTE, languageOn: languageOn, dotted: dotted, get: get, words: words, norm: norm, isIcon: isIcon, headings: headings, hasIntro: hasIntro, unreadable: unreadable, itemPath: itemPath });

  // ---------- form pieces ----------
  // An input bound to a path in the document: `kind` says how its text is
  // stored (text, a number, an optional text that is dropped when empty).
  function input(ctx, path, opts) {
    opts = opts || {};
    var value = get(ctx.doc, path);
    var attrs = { "data-testid": opts.testid || "course-form-field", "data-path": dotted(path), "data-kind": opts.kind || "text", "aria-label": opts.label || null, placeholder: opts.placeholder || null, autocapitalize: opts.caps ? null : "off", spellcheck: opts.spell === false ? "false" : null, id: opts.id || null };
    var el;
    if (opts.rows) { el = h("textarea", Object.assign(attrs, { rows: opts.rows })); el.value = value == null ? "" : String(value); }
    else if (opts.kind === "number") { el = h("input", Object.assign(attrs, { type: "number", inputmode: opts.step ? "numeric" : "decimal", step: opts.step || "any", min: opts.min })); el.value = typeof value === "number" ? String(value) : ""; }
    else { el = h("input", Object.assign(attrs, { type: "text" })); el.value = value == null ? "" : String(value); }
    if (opts.lang) el.lang = opts.lang;
    if (opts.mono) el.classList.add("le-mono");
    if (opts.cls) el.classList.add(opts.cls);
    return el;
  }
  function field(ctx, path, label, opts) {
    opts = opts || {};
    var id = "le-" + ctx.id + "-" + dotted(path).replace(/\./g, "-");
    var lab = h("label", { class: "le-label", for: id }, [label, opts.aside ? h("i", { text: " · " + opts.aside }) : null]);
    var box = h("div", { class: "le-f", "data-field": dotted(path) }, [lab, input(ctx, path, Object.assign({ id: id }, opts)), opts.hint ? h("p", { class: "le-hint", text: opts.hint }) : null, checkLine(ctx, path)]);
    return box;
  }
  function group(title, kids, aside) {
    return h("section", { class: "le-group" }, [h("h4", { class: "le-group-title" }, [title, aside ? h("i", { text: " · " + aside }) : null])].concat(kids));
  }
  function row(kids, cls) { return h("div", { class: "le-row " + (cls || "two") }, kids); }
  function btn(ctx, act, path, text, label, cls, extra) {
    var b = h("button", Object.assign({ type: "button", class: cls || "le-btn", "data-act": act, "data-path": dotted(path), "data-testid": "course-form-" + act, "aria-label": label || null, title: label || null, text: text }, extra || {}));
    return b;
  }
  // A list of short texts (hints, accepted answers, distractors, buckets):
  // one box per entry, ✕, and + another.
  function stringList(ctx, path, label, opts) {
    opts = opts || {};
    var items = get(ctx.doc, path);
    items = Array.isArray(items) ? items : [];
    var ol = h("ol", { class: "le-list", "data-testid": "course-form-list", "data-path": dotted(path) }, items.map(function (_, i) {
      return h("li", { class: "le-list-item", "data-testid": "course-form-item", "data-path": dotted(path.concat(i)) }, [
        input(ctx, path.concat(i), { kind: "text", label: label, lang: opts.lang, placeholder: opts.placeholder }),
        btn(ctx, "remove", path.concat(i), "✕", T("remove"), "le-icon")
      ]);
    }));
    var add = btn(ctx, "add", path, opts.addText || T("another"), T("add_to", { list: label }), "le-link");
    return h("div", { class: "le-f", "data-field": dotted(path) }, [h("span", { class: "le-label", text: label }), opts.hint ? h("p", { class: "le-hint", text: opts.hint }) : null, ol, add, checkLine(ctx, path)]);
  }
  // A field's own checks (#1320): a ✓, amber ! or red ! in the field's corner
  // (refresh() fills it) that opens them in a modal. It takes no room, so a
  // check coming and going as you type moves nothing below it.
  function checkLine(ctx, path) {
    return h("button", { type: "button", class: "le-check", "data-check": dotted(path), "data-act": "check", "data-testid": "course-form-check", hidden: true }, [h("span", { class: "le-check-mark", "aria-hidden": "true" }), h("span", { class: "sr-only le-check-text" })]);
  }
  function seg(ctx, act, path, choices, current) {
    return h("span", { class: "le-seg", role: "group", "data-testid": "course-form-" + act }, choices.map(function (c) {
      return h("button", { type: "button", class: "le-seg-btn", "data-act": act, "data-path": dotted(path), "data-value": c.value, "aria-pressed": c.value === current ? "true" : "false", title: c.title || null, text: c.text });
    }));
  }
  var foot = function (ctx, sel) {
    var p = itemPath(sel);
    if (sel.idx == null) return null;
    var list = get(ctx.doc, p.slice(0, -1)), n = Array.isArray(list) ? list.length : 0;
    return h("div", { class: "le-foot" }, [
      btn(ctx, "prev", p, T("previous"), null, "le-link", { disabled: sel.idx === 0 || null }),
      btn(ctx, "next", p, T("next"), null, "le-link", { disabled: sel.idx >= n - 1 || null }),
      btn(ctx, "up", p, "↑", T("move_up"), "le-icon", { disabled: sel.idx === 0 || null }),
      btn(ctx, "down", p, "↓", T("move_down"), "le-icon", { disabled: sel.idx >= n - 1 || null }),
      h("span", { class: "le-sp" }),
      btn(ctx, "duplicate", p, T("duplicate"), null, "le-link"),
      btn(ctx, "remove", p, T("delete"), null, "le-link le-danger")
    ]);
  };

  // ---------- the forms, one per kind ----------
  function formLesson(ctx) {
    var doc = ctx.doc;
    var pre = Array.isArray(doc.prerequisites) ? doc.prerequisites : [];
    var others = (ctx.lessons ? ctx.lessons() : []).filter(function (l) { return l.id && l.id !== doc.id && pre.indexOf(l.id) < 0; });
    var sel = h("select", { class: "le-select", "data-act": "add-prereq", "data-testid": "course-form-add-prereq", "aria-label": T("add_prereq") }, [h("option", { value: "", text: T("add_prereq") })].concat(others.map(function (l) { return h("option", { value: l.id, text: l.title }); })));
    return [
      h("h3", { class: "le-title" }, [T("lesson_head")]),
      group(T("grp_about"), [
        field(ctx, ["title"], T("f_title"), { caps: true }),
        field(ctx, ["summary"], T("f_summary"), { hint: T("h_summary"), caps: true }),
        field(ctx, ["focus"], T("f_focus"), { hint: T("h_focus"), caps: true, kind: "opt" }),
        field(ctx, ["group"], T("f_group"), { hint: T("h_group"), kind: "opt" })
      ]),
      group(T("grp_prereq"), [
        h("div", { class: "le-chips", "data-testid": "course-form-list", "data-path": "prerequisites" }, pre.map(function (id, i) {
          var l = (ctx.lessons ? ctx.lessons() : []).filter(function (x) { return x.id === id; })[0];
          return h("span", { class: "le-chip", "data-testid": "course-form-item", "data-path": "prerequisites." + i }, [l ? l.title : id, btn(ctx, "remove", ["prerequisites", i], "✕", T("remove"), "le-chip-x")]);
        }).concat([sel])),
        h("p", { class: "le-hint", text: T("h_prereq") }), checkLine(ctx, ["prerequisites"])
      ]),
      h("p", { class: "le-hint le-ids", text: T("ids_note") })
    ];
  }
  function formContent(ctx) {
    var hs = headings(ctx.doc.content);
    var cards = (hasIntro(ctx.doc.content) ? [T("intro_card")] : []).concat(hs);
    return [
      h("h3", { class: "le-title" }, [T("content_head"), h("span", { class: "le-sub", text: T("content_sub") })]),
      field(ctx, ["content"], T("f_content"), { rows: 18, hint: T("h_content"), caps: true, spell: true }),
      h("div", { class: "le-cards", "data-testid": "course-form-cards" }, [h("span", { class: "le-label", text: T("cards_label") })].concat(cards.length ? cards.map(function (c, i) { return h("span", { class: "le-chip", text: (i + 1) + ". " + c }); }) : [h("span", { class: "le-hint", text: T("cards_none") })]))
    ];
  }
  // An example and its meaning (a word's, a bite's). With the language
  // features off the meaning box is left out unless it has text.
  function exampleRow(ctx, p, it) {
    var example = field(ctx, p.concat("example"), T("f_example"), { kind: "opt", lang: ctx.termLang, caps: true });
    if (!ctx.lang && !String(it && it.example_meaning || "").trim()) return example;
    return row([example, field(ctx, p.concat("example_meaning"), T("f_example_meaning"), { kind: "opt", caps: true })]);
  }
  function formVocab(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p);
    var uses = (ctx.doc.quiz || []).map(function (q, i) {
      var teaches = (Array.isArray(q.teaches) ? q.teaches : []).indexOf(it.term) >= 0;
      var answers = [q.correct_answer].concat(q.accepted_answers || []).some(function (a) { return norm(a) === norm(it.term); });
      return teaches || answers ? (i + 1) : null;
    }).filter(function (x) { return x; });
    return [
      h("h3", { class: "le-title" }, [T("word_n_of", { n: sel.idx + 1, total: ctx.doc.vocabulary.length })]),
      group(T("grp_word"), [
        row([field(ctx, p.concat("term"), T("f_term"), { lang: ctx.termLang }), field(ctx, p.concat("meaning"), T("f_meaning"), { caps: true })])
      ]),
      group(T("grp_example"), [exampleRow(ctx, p, it)], ctx.lang ? T("grp_example_aside") : T("optional")),
      group(T("grp_used"), [h("p", { class: "le-hint", "data-testid": "course-form-uses", text: uses.length ? T("used_by_questions", { list: uses.join(", ") }) : T("used_by_none") })]),
      foot(ctx, sel)
    ];
  }
  function formQuiz(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p), type = it.type;
    var termLang = ctx.termLang;
    var answer = [];
    if (type === "multiple-choice") {
      var opts = Array.isArray(it.options) ? it.options : [];
      var name = "le-correct-" + ctx.id + "-" + dotted(p);
      // Options drawn as staves (`option_kind: "abc"`, #1219): in a music course or once a question has it.
      if ((ctx.man && ctx.man.music) || it.option_kind) answer.push(h("div", { class: "le-f", "data-field": dotted(p.concat("option_kind")) }, [
        h("span", { class: "le-label", text: T("f_option_kind") }),
        seg(ctx, "option-kind", p.concat("option_kind"), [{ value: "text", text: T("option_kind_text") }, { value: "abc", text: T("option_kind_abc") }], it.option_kind === "abc" ? "abc" : "text"),
        checkLine(ctx, p.concat("option_kind"))
      ]));
      answer.push(h("div", { class: "le-f", "data-field": dotted(p.concat("options")) }, [
        h("span", { class: "le-label" }, [T("f_options"), h("i", { text: " · " + T("f_options_aside") })]),
        h("ol", { class: "le-list le-options", "data-testid": "course-form-list", "data-path": dotted(p.concat("options")) }, opts.map(function (o, i) {
          return h("li", { class: "le-list-item", "data-testid": "course-form-item", "data-path": dotted(p.concat("options", i)) }, [
            h("input", { type: "radio", class: "le-correct", name: name, "data-testid": "course-form-correct", "data-path": dotted(p.concat("options", i)), "aria-label": T("right_answer"), title: T("right_answer"), checked: it.correct_answer === o && String(o) !== "" ? true : null }),
            input(ctx, p.concat("options", i), { label: T("option_n", { n: i + 1 }), lang: termLang }),
            btn(ctx, "remove", p.concat("options", i), "✕", T("remove"), "le-icon", { disabled: opts.length <= 2 || null })
          ]);
        }).concat([h("li", { class: "le-list-item le-new" }, [
          h("span", { class: "le-radio-ghost", "aria-hidden": "true" }),
          h("input", { type: "text", class: "le-new-option", "data-act": "new-option", "data-path": dotted(p.concat("options")), "data-testid": "course-form-new-option", placeholder: T("another_option"), "aria-label": T("another_option"), autocapitalize: "off", lang: termLang })
        ])])),
        checkLine(ctx, p.concat("options"))
      ]));
    } else if (type === "true-false") {
      answer.push(h("div", { class: "le-f", "data-field": dotted(p.concat("correct_answer")) }, [
        h("span", { class: "le-label", text: T("f_which_right") }),
        h("div", { class: "le-tf", "data-testid": "course-form-field", "data-path": dotted(p.concat("correct_answer")) }, [
          h("button", { type: "button", class: "le-tf-btn", "data-act": "tf", "data-path": dotted(p.concat("correct_answer")), "data-value": "true", "aria-pressed": it.correct_answer === "true" ? "true" : "false", text: T("true") }),
          h("button", { type: "button", class: "le-tf-btn", "data-act": "tf", "data-path": dotted(p.concat("correct_answer")), "data-value": "false", "aria-pressed": it.correct_answer === "false" ? "true" : "false", text: T("false") })
        ]),
        checkLine(ctx, p.concat("correct_answer"))
      ]));
    } else if (type === "numeric") {
      answer.push(row([
        field(ctx, p.concat("correct_answer"), T("f_correct"), { hint: T("h_numeric"), mono: true }),
        field(ctx, p.concat("tolerance"), T("f_tolerance"), { kind: "number", min: 0, aside: "±", hint: T("h_tolerance") }),
        field(ctx, p.concat("unit"), T("f_unit"), { kind: "opt", aside: T("optional"), hint: T("h_unit") })
      ], "three"));
      answer.push(stringList(ctx, p.concat("accepted_answers"), T("f_accepted"), { hint: T("h_accepted") }));
    } else if (type === "clock") {
      answer.push(row([
        field(ctx, p.concat("correct_answer"), T("f_correct"), { hint: T("h_clock"), mono: true }),
        field(ctx, p.concat("step_minutes"), T("f_step"), { kind: "number", min: 1, hint: T("h_step") })
      ]));
      answer.push(stringList(ctx, p.concat("options"), T("f_clock_options"), { hint: T("h_clock_options") }));
    } else if (type === "expression") {
      answer.push(field(ctx, p.concat("correct_answer"), T("f_correct"), { aside: T("f_expression_aside"), hint: T("h_expression"), mono: true }));
      answer.push(h("p", { class: "le-check warn", text: T("h_expression_form") }));
      answer.push(stringList(ctx, p.concat("accepted_answers"), T("f_accepted"), { hint: T("h_accepted") }));
    } else if (type === "fill-bar") {
      // Fill the bar (#1230): note-value tiles, and whether any that fill the gap count.
      answer.push(stringList(ctx, p.concat("tiles"), T("f_tiles"), { hint: T("h_tiles") }));
      answer.push(h("div", { class: "le-f", "data-field": dotted(p.concat("exact")) }, [h("span", { class: "le-label", text: T("f_exact") }),
        seg(ctx, "exact", p.concat("exact"), [{ value: "any", text: T("exact_any") }, { value: "only", text: T("exact_only") }], it.exact ? "only" : "any")]));
      answer.push(field(ctx, p.concat("correct_answer"), T("f_correct"), { kind: it.exact ? "text" : "opt", aside: it.exact ? null : T("optional"), hint: T("h_bar_answer"), mono: true }));
    } else if (type === "chord-spell") {
      // Spell the chord (#1228): the chord's notes and the tiles, both in ABC; tiles written as a list stay a list.
      answer.push(field(ctx, p.concat("correct_answer"), T("f_correct"), { hint: T("h_chord_answer"), mono: true }));
      answer.push(Array.isArray(it.tiles) ? stringList(ctx, p.concat("tiles"), T("f_chord_tiles"), { hint: T("h_chord_tiles") })
        : field(ctx, p.concat("tiles"), T("f_chord_tiles"), { hint: T("h_chord_tiles"), mono: true }));
    } else if (type === "staff-note") {
      // Place the note (#1226): the note in ABC with its octave, the clef, and whether any octave counts.
      answer.push(field(ctx, p.concat("correct_answer"), T("f_correct"), { hint: T("h_staff_answer"), mono: true }));
      answer.push(h("div", { class: "le-f", "data-field": dotted(p.concat("clef")) }, [
        h("span", { class: "le-label", text: T("f_clef") }),
        h("div", { "data-testid": "course-form-field", "data-path": dotted(p.concat("clef")) }, [seg(ctx, "tf", p.concat("clef"), [{ value: "treble", text: T("clef_treble") }, { value: "bass", text: T("clef_bass") }], it.clef || "treble")]),
        h("button", { type: "button", class: "le-tf-btn", "data-act": "any-octave", "data-path": dotted(p.concat("any_octave")), "data-testid": "course-form-any-octave", "aria-pressed": it.any_octave ? "true" : "false", text: T("f_any_octave") }),
        checkLine(ctx, p.concat("clef"))
      ]));
    } else if (type === "keyboard") {
      // Find it on the keys (#1231): the key in ABC, how many octaves, and whether any key of the note counts.
      answer.push(field(ctx, p.concat("correct_answer"), T("f_correct"), { hint: T("h_key"), mono: true }));
      answer.push(row([
        h("div", { class: "le-f", "data-field": dotted(p.concat("octaves")) }, [h("span", { class: "le-label", text: T("f_octaves") }),
          seg(ctx, "octaves", p.concat("octaves"), [{ value: "1", text: T("octaves_one") }, { value: "2", text: T("octaves_two") }], it.octaves === 2 ? "2" : "1")]),
        h("div", { class: "le-f", "data-field": dotted(p.concat("octave")) }, [h("span", { class: "le-label", text: T("f_key_counts") }),
          seg(ctx, "octave", p.concat("octave"), [{ value: "any", text: T("key_any") }, { value: "exact", text: T("key_exact") }], it.octave ? "exact" : "any")])
      ]));
      answer.push(stringList(ctx, p.concat("accepted_answers"), T("f_accepted"), { hint: T("h_key_accepted") }));
    } else if (type === "algebra-tiles") {
      answer.push(field(ctx, p.concat("tiles"), T("f_pile"), { hint: T("h_pile"), mono: true }));
      answer.push(field(ctx, p.concat("correct_answer"), T("f_correct"), { aside: T("f_sides_aside"), hint: T("h_sides"), mono: true }));
    } else {
      answer.push(row([
        field(ctx, p.concat("correct_answer"), T("f_correct"), { hint: T("h_fill"), lang: termLang }),
        stringList(ctx, p.concat("accepted_answers"), T("f_accepted"), { lang: termLang })
      ]));
    }
    var terms = (Array.isArray(ctx.doc.vocabulary) ? ctx.doc.vocabulary : []).map(function (v) { return v && v.term; }).filter(Boolean);
    var teaches = Array.isArray(it.teaches) ? it.teaches : [];
    var free = terms.filter(function (x) { return teaches.indexOf(x) < 0; });
    var teachSel = h("select", { class: "le-select", "data-act": "add-teaches", "data-path": dotted(p.concat("teaches")), "data-testid": "course-form-add-teaches", "aria-label": T("add_word") }, [h("option", { value: "", text: free.length ? T("add_word") : T("no_words") })].concat(free.map(function (x) { return h("option", { value: x, text: x }); })));
    return [
      h("h3", { class: "le-title" }, [
        T("question_n_of", { n: sel.idx + 1, total: ctx.doc.quiz.length }),
        seg(ctx, "type", p.concat("type"), QUIZ_TYPES.filter(function (x) { return MUSIC_TYPES.indexOf(x) < 0 || (ctx.man && ctx.man.music) || type === x; }).map(function (x) { return { value: x, text: BADGE[x], title: T("type_" + x) }; }), type),
        h("span", { class: "le-sub", text: T("type_" + (QUIZ_TYPES.indexOf(type) >= 0 ? type : "unknown")) })
      ]),
      group(T("grp_ask"), [field(ctx, p.concat("prompt"), type === "true-false" ? T("f_statement") : T("f_prompt"), { rows: 2, hint: type === "true-false" ? T("h_statement") : type === "fill-in-blank" ? T("h_prompt_blank") : (type === "numeric" || type === "expression" || type === "algebra-tiles") ? T("h_prompt_math") : null, lang: termLang, caps: true, spell: true }),
        // Music drawn above the prompt (#1218): offered in a music course (course.json "music") or once an item has it.
        type === "fill-bar" ? field(ctx, p.concat("abc"), T("f_abc"), { rows: 3, mono: true, hint: T("h_abc_bar") })
          : type !== "chord-spell" && type !== "staff-note" && ((ctx.man && ctx.man.music) || it.abc) ? field(ctx, p.concat("abc"), T("f_abc"), { rows: 3, mono: true, kind: "opt", aside: T("optional"), hint: T("h_abc") }) : null,
        // Music a speaker plays (#1219), offered the same way.
        (ctx.man && ctx.man.music) || it.play ? field(ctx, p.concat("play"), T("f_play"), { rows: 3, mono: true, kind: "opt", aside: T("optional"), hint: T("h_play") }) : null]),
      group(T("grp_answer"), answer),
      group(T("grp_after"), [
        field(ctx, p.concat("explanation"), T("f_explanation"), { rows: 2, caps: true, spell: true, kind: "opt" }),
        row([
          stringList(ctx, p.concat("hints"), T("f_hints"), { caps: true }),
          h("div", { class: "le-f", "data-field": dotted(p.concat("teaches")) }, [
            h("span", { class: "le-label" }, [T("f_teaches"), h("i", { text: " · " + T("f_teaches_aside") })]),
            h("div", { class: "le-chips", "data-testid": "course-form-list", "data-path": dotted(p.concat("teaches")) }, teaches.map(function (term, i) {
              return h("span", { class: "le-chip", "data-testid": "course-form-item", "data-path": dotted(p.concat("teaches", i)), lang: termLang }, [term, btn(ctx, "remove", p.concat("teaches", i), "✕", T("remove"), "le-chip-x")]);
            }).concat([teachSel])),
            h("p", { class: "le-hint", text: T("h_teaches") }), checkLine(ctx, p.concat("teaches"))
          ])
        ])
      ]),
      foot(ctx, sel)
    ];
  }
  function formListening(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p);
    return [
      h("h3", { class: "le-title" }, [T("listening_n_of", { n: sel.idx + 1, total: ctx.doc.listening.length })]),
      group(T("grp_hear"), [
        h("div", { class: "le-row two wide-first" }, [
          field(ctx, p.concat("text"), T("f_text_exact"), { hint: T("h_listening_text"), lang: ctx.termLang, caps: true, spell: true }),
          h("div", { class: "le-f le-f-btn" }, [btn(ctx, "say", p.concat("text"), "🔊 " + T("hear_it"), T("hear_it"), "le-btn")])
        ])
      ]),
      group(T("grp_after"), [row([
        field(ctx, p.concat("meaning"), T("f_meaning"), { caps: true }),
        field(ctx, p.concat("note"), T("f_note"), { kind: "opt", aside: T("optional"), hint: T("h_note"), caps: true })
      ])]),
      foot(ctx, sel)
    ];
  }
  function formConversation(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p);
    var lines = Array.isArray(it.lines) ? it.lines : [];
    var grid = h("div", { class: "le-lines", "data-testid": "course-form-list", "data-path": dotted(p.concat("lines")) }, lines.map(function (l, i) {
      var lp = p.concat("lines", i);
      var meaning = ctx.lang || !!String(l.meaning || "").trim();
      return h("div", { class: "le-line" + (meaning ? "" : " no-meaning"), "data-testid": "course-form-item", "data-path": dotted(lp) }, [
        h("span", { class: "le-seg le-speaker", role: "group", "aria-label": T("f_speaker") }, [
          h("button", { type: "button", class: "le-seg-btn", "data-act": "speaker", "data-path": dotted(lp.concat("speaker")), "data-value": "a", "aria-pressed": l.speaker === "a" ? "true" : "false", title: it.role_a || "A", text: "A" }),
          h("button", { type: "button", class: "le-seg-btn", "data-act": "speaker", "data-path": dotted(lp.concat("speaker")), "data-value": "b", "aria-pressed": l.speaker === "b" ? "true" : "false", title: it.role_b || "B", text: "B" })
        ]),
        input(ctx, lp.concat("text"), { label: T("f_line_text"), placeholder: T("f_line_text"), lang: ctx.termLang, caps: true }),
        meaning ? input(ctx, lp.concat("meaning"), { label: T("f_line_meaning"), placeholder: T("f_line_meaning"), caps: true }) : null,
        btn(ctx, "remove", lp, "✕", T("remove"), "le-icon", { disabled: lines.length <= 2 || null }),
        input(ctx, lp.concat("note"), { label: T("f_line_note"), placeholder: T("f_line_note"), kind: "opt", cls: "le-line-note", caps: true })
      ]);
    }));
    return [
      h("h3", { class: "le-title" }, [T("conversation_head"), h("span", { class: "le-sub", text: it.title || "" })]),
      group(T("grp_scene"), [
        field(ctx, p.concat("title"), T("f_title"), { caps: true }),
        field(ctx, p.concat("scenario"), T("f_scenario"), { hint: T("h_scenario"), caps: true, kind: "opt" }),
        row([field(ctx, p.concat("role_a"), T("f_role_a"), { caps: true }), field(ctx, p.concat("role_b"), T("f_role_b"), { caps: true })])
      ]),
      group(T("grp_lines"), [
        h("div", { class: "le-f", "data-field": dotted(p.concat("lines")) }, [grid, btn(ctx, "add", p.concat("lines"), T("add_line"), T("add_line"), "le-link"), checkLine(ctx, p.concat("lines"))])
      ], T("grp_lines_aside")),
      foot(ctx, sel)
    ];
  }
  function formConjugation(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p), para = ctx.man && ctx.man.paradigm;
    var forms = it.forms && typeof it.forms === "object" ? it.forms : {};
    var slots = para ? para.slots : Object.keys(forms).map(function (k) { return { key: k, label: k, hint: "" }; });
    var setField;
    if (para) {
      var opts = para.sets.map(function (s) { return h("option", { value: s.key, text: s.label }); });
      if (!para.sets.some(function (s) { return s.key === it.set; })) opts.unshift(h("option", { value: it.set || "", text: it.set || "—" }));
      var selEl = h("select", { class: "le-select", "data-testid": "course-form-field", "data-path": dotted(p.concat("set")), "data-kind": "text", "aria-label": T("f_set") }, opts);
      selEl.value = it.set || "";
      setField = h("div", { class: "le-f", "data-field": dotted(p.concat("set")) }, [h("span", { class: "le-label", text: T("f_set") }), selEl, checkLine(ctx, p.concat("set"))]);
    } else {
      setField = field(ctx, p.concat("set"), T("f_set"));
    }
    var art = h("textarea", { class: "le-mono le-art", rows: 5, "data-testid": "course-form-field", "data-path": dotted(p.concat("art")), "data-kind": "lines", spellcheck: "false", "aria-label": T("f_art") });
    art.value = (Array.isArray(it.art) ? it.art : []).join("\n");
    return [
      h("h3", { class: "le-title" }, [T("conjugation_head", { item: para ? para.item_label : T("item") }), h("span", { class: "le-sub", text: [it.item, it.set].filter(Boolean).join(" · ") })]),
      group(para ? para.item_label : T("grp_item"), [
        row([field(ctx, p.concat("item"), para ? para.item_label : T("f_item"), { lang: ctx.termLang }), field(ctx, p.concat("meaning"), T("f_meaning"), { caps: true }), setField], "three"),
        checkLine(ctx, p.concat("item"))
      ]),
      group(T("grp_forms"), [
        h("div", { class: "le-f", "data-field": dotted(p.concat("forms")) }, [
          h("div", { class: "le-forms" }, slots.map(function (s) {
            return [h("span", { class: "le-slot" }, [h("b", { text: s.label }), s.hint ? " " + s.hint : ""]), input(ctx, p.concat("forms", s.key), { label: s.label, lang: ctx.termLang })];
          }).concat(para ? Object.keys(forms).filter(function (k) { return !para.slots.some(function (s) { return s.key === k; }); }).map(function (k) {
            return [h("span", { class: "le-slot" }, [h("b", { text: k }), " ", btn(ctx, "remove", p.concat("forms", k), "✕", T("remove"), "le-icon")]), input(ctx, p.concat("forms", k), { label: k, lang: ctx.termLang })];
          }) : []).reduce(function (a, b) { return a.concat(b); }, [])),
          checkLine(ctx, p.concat("forms"))
        ])
      ], para ? T("grp_forms_aside") : T("grp_forms_free")),
      group(T("grp_art"), [h("div", { class: "le-f", "data-field": dotted(p.concat("art")) }, [art, checkLine(ctx, p.concat("art"))])], T("grp_art_aside")),
      foot(ctx, sel)
    ];
  }
  function formIcon(ctx, sel) {
    return [
      h("h3", { class: "le-title" }, [T("icon_head"), h("span", { class: "le-sub", text: T("icon_sub") })]),
      group(T("grp_icon"), [field(ctx, ["sprint", "icon"], T("f_icon"), { hint: T("h_icon"), cls: "le-short", kind: "opt" })]),
      h("div", { class: "le-foot" }, [h("span", { class: "le-sp" }), btn(ctx, "remove", ["sprint", "icon"], T("delete"), null, "le-link le-danger")])
    ];
  }
  function formBite(ctx, sel) {
    var p = itemPath(sel);
    return [
      h("h3", { class: "le-title" }, [T("bite_head"), h("span", { class: "le-sub", text: T("bite_sub") })]),
      group(T("grp_say"), [
        row([field(ctx, p.concat("title"), T("f_title"), { caps: true }), field(ctx, p.concat("emoji"), T("f_emoji"), { kind: "opt", cls: "le-short", hint: T("h_emoji") })], "two wide-first"),
        field(ctx, p.concat("rule"), T("f_rule"), { hint: T("h_rule"), caps: true })
      ]),
      group(T("grp_show"), [exampleRow(ctx, p, get(ctx.doc, p))]),
      foot(ctx, sel)
    ];
  }
  function formSort(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p);
    var buckets = Array.isArray(it.buckets) ? it.buckets : [], items = Array.isArray(it.items) ? it.items : [];
    var cols = h("div", { class: "le-buckets b" + buckets.length, "data-testid": "course-form-list", "data-path": dotted(p.concat("buckets")) }, buckets.map(function (name, bi) {
      var rows = [];
      items.forEach(function (item, ii) {
        if (item.bucket !== bi) return;
        var ip = p.concat("items", ii);
        rows.push(h("div", { class: "le-sort-item", "data-testid": "course-form-item", "data-path": dotted(ip), draggable: "true" }, [
          h("span", { class: "le-grip", "aria-hidden": "true", text: "⋮" }),
          input(ctx, ip.concat("text"), { label: T("f_sort_item"), lang: ctx.termLang, caps: true }),
          // A speaker tile (#1219): music in place of text, in a music course or once an item has it.
          (ctx.man && ctx.man.music) || item.play ? input(ctx, ip.concat("play"), { label: T("f_sort_play"), placeholder: T("f_sort_play"), mono: true, kind: "opt" }) : null,
          btn(ctx, "bucket-prev", ip, "◂", T("move_prev_bucket"), "le-icon", { disabled: bi === 0 || null }),
          btn(ctx, "bucket-next", ip, "▸", T("move_next_bucket"), "le-icon", { disabled: bi === buckets.length - 1 || null }),
          btn(ctx, "remove", ip, "✕", T("remove"), "le-icon", { disabled: items.length <= 3 || null })
        ]));
      });
      return h("div", { class: "le-bucket", "data-testid": "course-form-item", "data-path": dotted(p.concat("buckets", bi)), "data-bucket": bi }, [
        h("div", { class: "le-bucket-head" }, [input(ctx, p.concat("buckets", bi), { label: T("f_bucket"), caps: true }), btn(ctx, "remove-bucket", p.concat("buckets", bi), "✕", T("remove_bucket"), "le-icon", { disabled: buckets.length <= 2 || null })]),
        h("div", { class: "le-bucket-items" }, rows),
        btn(ctx, "add-sort-item", p.concat("items"), T("add_item"), T("add_item"), "le-link", { "data-bucket": bi, disabled: items.length >= 10 || null })
      ]);
    }));
    return [
      h("h3", { class: "le-title" }, [T("sort_head"), h("span", { class: "le-sub", text: T("sort_sub") })]),
      group(T("grp_ask"), [field(ctx, p.concat("title"), T("f_title"), { caps: true })]),
      group(T("grp_buckets"), [
        h("div", { class: "le-f", "data-field": dotted(p.concat("items")) }, [cols, h("p", { class: "le-row-links" }, [btn(ctx, "add-bucket", p.concat("buckets"), T("add_bucket"), T("add_bucket"), "le-link", { disabled: buckets.length >= 4 || null })]), checkLine(ctx, p.concat("buckets")), checkLine(ctx, p.concat("items"))])
      ], T("grp_buckets_aside")),
      group(T("grp_after"), [field(ctx, p.concat("why"), T("f_why"), { rows: 2, kind: "opt", caps: true, spell: true })]),
      foot(ctx, sel)
    ];
  }
  // The sentence's words as buttons, the wrong word pressed. A word the
  // sentence has twice (or only punctuation) cannot be the wrong word.
  function spotWords(ctx, p, spot) {
    var ws = words(spot.sentence), n = {};
    ws.forEach(function (w) { n[norm(w)] = (n[norm(w)] || 0) + 1; });
    return ws.map(function (w) {
      var twice = n[norm(w)] > 1;
      return h("button", { type: "button", class: "le-word", "data-act": "spot", "data-path": dotted(p.concat("wrong")), "data-value": w, "aria-pressed": norm(w) === norm(spot.wrong) && norm(w) ? "true" : "false", disabled: twice || !norm(w) || null, title: twice ? T("spot_word_twice") : null, lang: ctx.termLang, text: w });
    });
  }
  function formSpot(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p);
    var ws = spotWords(ctx, p, it);
    // A spot on a staff (#1227): offered in a music course or once a spot has `abc`.
    var music = (ctx.man && ctx.man.music) || it.abc ? field(ctx, p.concat("abc"), T("f_abc"), { rows: 3, mono: true, kind: "opt", aside: T("optional"), hint: T("h_spot_abc") }) : null;
    if (it.abc) return [
      h("h3", { class: "le-title", "data-staff-spot": "" }, [T("spot_head"), h("span", { class: "le-sub", text: T("one_note_wrong") })]),
      group(T("grp_ask"), [music, field(ctx, p.concat("wrong"), T("f_wrong_note"), { kind: "number", min: 1, cls: "le-short", aside: T("f_wrong_note_aside") })]),
      group(T("grp_answer"), [field(ctx, p.concat("fix"), T("f_fix"), { hint: T("h_fix_note"), cls: "le-short", mono: true })]),
      group(T("grp_after"), [field(ctx, p.concat("why"), T("f_why"), { rows: 2, kind: "opt", caps: true, spell: true })]),
      foot(ctx, sel)
    ];
    return [
      h("h3", { class: "le-title" }, [T("spot_head"), h("span", { class: "le-sub", text: T("spot_sub") })]),
      group(T("grp_ask"), [
        field(ctx, p.concat("sentence"), T("f_sentence"), { aside: T("f_sentence_aside"), lang: ctx.termLang, caps: true }),
        music,
        h("div", { class: "le-f", "data-field": dotted(p.concat("wrong")) }, [
          h("span", { class: "le-label" }, [T("f_wrong"), h("i", { text: " · " + T("f_wrong_aside") })]),
          h("div", { class: "le-words", "data-testid": "course-form-words" }, ws.length ? ws : [h("span", { class: "le-hint", text: T("spot_type_first") })]),
          checkLine(ctx, p.concat("wrong"))
        ])
      ]),
      group(T("grp_answer"), [field(ctx, p.concat("fix"), T("f_fix"), { hint: T("h_fix"), cls: "le-short", lang: ctx.termLang })]),
      group(T("grp_after"), [field(ctx, p.concat("why"), T("f_why"), { rows: 2, kind: "opt", caps: true, spell: true })]),
      foot(ctx, sel)
    ];
  }
  function formBuild(ctx, sel) {
    var p = itemPath(sel), it = get(ctx.doc, p);
    return [
      h("h3", { class: "le-title" }, [T("build_head"), h("span", { class: "le-sub", text: T("build_sub") })]),
      group(T("grp_ask"), [row([field(ctx, p.concat("text"), T("f_text"), { lang: ctx.termLang, caps: true }), field(ctx, p.concat("meaning"), T("f_meaning"), { caps: true })])]),
      group(T("grp_tiles"), [
        h("div", { class: "le-f" }, [h("span", { class: "le-label", text: T("f_from_text") }), h("div", { class: "le-words", "data-testid": "course-form-words" }, words(it.text).map(function (w) { return h("span", { class: "le-word static", lang: ctx.termLang, text: w }); }))]),
        stringList(ctx, p.concat("distractors"), T("f_distractors"), { hint: T("h_distractors"), lang: ctx.termLang, addText: T("add_distractor") })
      ]),
      foot(ctx, sel)
    ];
  }
  function formScale(ctx, sel) {
    var p = itemPath(sel);
    return [
      h("h3", { class: "le-title" }, [T("scale_head"), h("span", { class: "le-sub", text: T("scale_sub") })]),
      group(T("grp_ask"), [field(ctx, p.concat("title"), T("f_title"), { caps: true })]),
      group(T("grp_answer"), [field(ctx, p.concat("notes"), T("f_notes"), { mono: true, aside: T("f_notes_aside"), hint: T("h_notes") })]),
      group(T("grp_tiles"), [field(ctx, p.concat("decoys"), T("f_decoys"), { mono: true, kind: "opt", aside: T("optional"), hint: T("h_decoys") })]),
      foot(ctx, sel)
    ];
  }
  function formGuide(ctx, st) {
    var doc = ctx.doc, steps = Array.isArray(doc.guide) ? doc.guide : [];
    if (!steps.length) {
      return [
        h("h3", { class: "le-title" }, [T("guide_head"), h("span", { class: "le-sub", text: T("guide_sub") })]),
        h("p", { class: "le-hint", text: T("guide_derived") }),
        h("p", {}, [btn(ctx, "guide-by-hand", ["guide"], T("guide_order_by_hand"), null, "btn small ghost")])
      ];
    }
    var cur = st.step == null ? 0 : Math.min(st.step, steps.length - 1);
    var kindOf = function (s) { return GUIDE_KINDS.filter(function (k) { return s && k in s; })[0] || "?"; };
    var summary = function (s) {
      var k = kindOf(s);
      if (k === "section") return s.section;
      if (k === "question") { var q = (doc.quiz || []).filter(function (x) { return x.question_id === s.question; })[0]; return q ? q.prompt : s.question; }
      if (k === "vocab") return T("guide_vocab_n", { n: s.vocab });
      if (k === "tip") return s.tip;
      if (k === "concept") return s.concept && s.concept.title;
      return "";
    };
    var bad = itemChecks(doc, ctx.man, { sec: "guide" }).filter(function (c) { return c.level === "bad"; }).map(function (c) { return c.path; });
    var list = h("ol", { class: "le-steps", "data-testid": "course-form-list", "data-path": "guide" }, steps.map(function (s, i) {
      var k = kindOf(s);
      return h("li", { class: "le-step" + (i === cur ? " on" : ""), "data-testid": "course-form-item", "data-path": "guide." + i, "data-kind": k }, [
        h("button", { type: "button", class: "le-step-pick", "data-act": "step", "data-path": "guide." + i, "data-testid": "course-form-step", "aria-pressed": i === cur ? "true" : "false" }, [
          h("span", { class: "le-badge", text: T("guide_kind_" + (k === "?" ? "unknown" : k)) }), h("span", { class: "le-step-text", text: short(summary(s) || T("untitled"), 70) }),
          bad.indexOf("guide." + i) >= 0 ? h("span", { class: "le-warn", title: T("has_problem"), text: "⚠" }) : null
        ]),
        btn(ctx, "up", ["guide", i], "↑", T("move_up"), "le-icon", { disabled: i === 0 || null }),
        btn(ctx, "down", ["guide", i], "↓", T("move_down"), "le-icon", { disabled: i === steps.length - 1 || null }),
        btn(ctx, "remove", ["guide", i], "✕", T("remove"), "le-icon")
      ]);
    }));
    var adds = h("p", { class: "le-row-links" }, GUIDE_KINDS.map(function (k) { return btn(ctx, "add-step", ["guide"], T("guide_add_" + k), T("guide_add_" + k), "le-link", { "data-value": k }); }));
    var s = steps[cur], k = kindOf(s), sp = ["guide", cur], fields = [];
    if (k === "section") {
      var hs = headings(doc.content); if (hasIntro(doc.content)) hs.unshift("Introduction");
      var selEl = h("select", { class: "le-select", "data-testid": "course-form-field", "data-path": dotted(sp.concat("section")), "data-kind": "text", "aria-label": T("f_section") }, (hs.some(function (x) { return x.toLowerCase().indexOf(String(s.section).toLowerCase()) === 0; }) ? [] : [h("option", { value: s.section, text: s.section })]).concat(hs.map(function (x) { return h("option", { value: x, text: x }); })));
      selEl.value = hs.filter(function (x) { return x.toLowerCase().indexOf(String(s.section).toLowerCase()) === 0; })[0] || s.section;
      fields.push(h("div", { class: "le-f" }, [h("span", { class: "le-label", text: T("f_section") }), selEl, h("p", { class: "le-hint", text: T("h_section") })]));
    } else if (k === "question") {
      var qs = doc.quiz || [];
      var qsel = h("select", { class: "le-select", "data-testid": "course-form-field", "data-path": dotted(sp.concat("question")), "data-kind": "text", "aria-label": T("f_question") }, (qs.some(function (q) { return q.question_id === s.question; }) ? [] : [h("option", { value: s.question, text: s.question })]).concat(qs.map(function (q, i) { return h("option", { value: q.question_id, text: (i + 1) + ". " + short(q.prompt, 60) }); })));
      qsel.value = s.question;
      fields.push(h("div", { class: "le-f" }, [h("span", { class: "le-label", text: T("f_question") }), qsel]));
    } else if (k === "vocab") {
      fields.push(field(ctx, sp.concat("vocab"), T("f_vocab_n"), { kind: "number", min: 1, step: 1, hint: T("h_vocab_n", { n: (doc.vocabulary || []).length }) }));
    } else if (k === "tip") {
      fields.push(field(ctx, sp.concat("tip"), T("f_tip"), { rows: 2, caps: true, spell: true }));
    } else if (k === "concept") {
      fields.push(field(ctx, sp.concat("concept", "title"), T("f_title"), { caps: true }));
      fields.push(field(ctx, sp.concat("concept", "markdown"), T("f_markdown"), { rows: 6, caps: true, spell: true }));
    }
    return [
      h("h3", { class: "le-title" }, [T("guide_head"), h("span", { class: "le-sub", text: T("guide_sub") })]),
      group(T("grp_steps"), [h("div", { class: "le-f", "data-field": "guide" }, [list, adds, checkLine(ctx, ["guide"])])], T("grp_steps_aside")),
      group(T("step_n_kind", { n: cur + 1, kind: T("guide_kind_" + (k === "?" ? "unknown" : k)) }), (fields.length ? fields : [h("p", { class: "le-hint", text: T("guide_step_unknown") })]).concat([checkLine(ctx, sp)])),
      h("div", { class: "le-foot" }, [h("span", { class: "le-sp" }), btn(ctx, "guide-derive", ["guide"], T("guide_back_to_derived"), null, "le-link le-danger")])
    ];
  }
  function form(ctx, st) {
    var sel = st.sel, sec = sel.sec;
    if (sec === "lesson") return formLesson(ctx);
    if (sec === "content") return formContent(ctx);
    if (sec === "guide") return formGuide(ctx, st);
    if (get(ctx.doc, itemPath(sel)) == null) return [h("p", { class: "le-hint", text: T("pick_item") })];
    if (sec === "vocabulary") return formVocab(ctx, sel);
    if (sec === "quiz") return formQuiz(ctx, sel);
    if (sec === "listening") return formListening(ctx, sel);
    if (sec === "conversations") return formConversation(ctx, sel);
    if (sec === "conjugations") return formConjugation(ctx, sel);
    if (sec === "sprint") {
      if (sel.sub === "icon") return formIcon(ctx, sel);
      if (sel.sub === "bites") return formBite(ctx, sel);
      if (sel.sub === "sorts") return formSort(ctx, sel);
      if (sel.sub === "spots") return formSpot(ctx, sel);
      if (sel.sub === "builds") return formBuild(ctx, sel);
      if (sel.sub === "scales") return formScale(ctx, sel);
    }
    return [h("p", { class: "le-hint", text: T("pick_item") })];
  }

  // ---------- blanks ----------
  function blank(doc, sec, sub, man) {
    if (sec === "vocabulary") return { term: "", meaning: "" };
    if (sec === "quiz") return { question_id: madeId(doc, "question_id"), prompt: "", type: "multiple-choice", options: ["", ""], correct_answer: "" };
    if (sec === "listening") return { text: "", meaning: "" };
    if (sec === "conversations") return { id: madeId(doc, "conv"), title: "", scenario: "", role_a: "A", role_b: "B", lines: [{ speaker: "a", text: "", meaning: "" }, { speaker: "b", text: "", meaning: "" }] };
    if (sec === "conjugations") { var sets = man && man.paradigm && Array.isArray(man.paradigm.sets) ? man.paradigm.sets : []; return { item: "", meaning: "", set: sets.length ? sets[0].key : "", forms: {} }; }
    if (sub === "bites") return { title: "", rule: "", example: "", example_meaning: "" };
    if (sub === "sorts") return { title: "", buckets: ["", ""], items: [{ text: "", bucket: 0 }, { text: "", bucket: 0 }, { text: "", bucket: 1 }], why: "" };
    if (sub === "spots") return { sentence: "", wrong: "", fix: "", why: "" };
    if (sub === "builds") return { text: "", meaning: "", distractors: [] };
    if (sub === "scales") return { title: "", notes: "", decoys: "" };
    return {};
  }
  function blankStep(doc, kind) {
    if (kind === "section") { var hs = headings(doc.content); return { section: hs[0] || "" }; }
    if (kind === "question") { var q = (doc.quiz || [])[0]; return { question: q ? q.question_id : "" }; }
    if (kind === "vocab") return { vocab: Math.min(5, (doc.vocabulary || []).length || 5) };
    if (kind === "tip") return { tip: "" };
    return { concept: { title: "", markdown: "" } };
  }
  function derivedSteps(doc) {
    var out = [];
    if (hasIntro(doc.content)) out.push({ section: "Introduction" });
    headings(doc.content).forEach(function (hd) { out.push({ section: hd }); });
    var n = (doc.vocabulary || []).length;
    if (n) out.splice(1, 0, { vocab: Math.min(5, n) });
    (doc.quiz || []).forEach(function (q) { out.push({ question: q.question_id }); });
    return out;
  }

  // ---------- drawing ----------
  var forms = 0;
  function render(el, doc, ctx) {
    var st = el._le;
    var first = !st;
    if (first) {
      st = el._le = { sel: { sec: "lesson" }, tab: "edit", open: {}, mode: null, step: 0 };
      el.classList.add("le-host");
    }
    ctx = ctx || {};
    ctx.doc = doc;
    ctx.id = st.id || (st.id = ++forms);
    if (ctx.manifest) st.man = ctx.manifest;
    ctx.man = st.man || null;
    ctx.termLang = st.man && st.man.terms && st.man.terms.lang ? st.man.terms.lang : null;
    ctx.lang = languageOn(st.man);
    ctx.changed = function (redraw) { ctx.onChange(doc); if (redraw) draw(); else refresh(); };
    el._lectx = ctx;
    if (ctx.api) math.api = ctx.api;
    el._leRefresh = refresh;
    st.refresh = refresh;
    // The course settings (for the paradigm's slots, the language, the
    // praise): fetched once per drawer; the form is drawn again when they land.
    if (!st.man && !st.manWait && ctx.course) {
      st.manWait = true;
      ctx.course().then(function (m) { st.man = m || {}; st.manWait = false; if (el._lectx === ctx) { ctx.man = st.man; ctx.termLang = st.man.terms && st.man.terms.lang ? st.man.terms.lang : null; ctx.lang = languageOn(st.man); draw(); } }).catch(function () { st.man = {}; st.manWait = false; });
    }
    // Keep a selection that still points at something.
    function clamp() {
      var s = st.sel;
      if (s.idx != null) {
        var arr = get(doc, s.sec === "sprint" ? ["sprint", s.sub] : [s.sec]);
        if (!Array.isArray(arr) || !arr.length) st.sel = { sec: s.sec === "sprint" ? "sprint" : s.sec, single: true };
        else if (s.idx >= arr.length) st.sel = Object.assign({}, s, { idx: arr.length - 1 });
      } else if (s.sec === "sprint" && s.sub === "icon" && !(doc.sprint && typeof doc.sprint.icon === "string" && doc.sprint.icon.trim())) st.sel = { sec: "sprint", single: true };
      if (st.sel.single) {
        // A section with nothing in it: its first item if it has one, else the lesson.
        var secs = sections(doc, st.man);
        var sc = secs.filter(function (x) { return x.key === st.sel.sec; })[0];
        if (sc && sc.items && sc.items.length) st.sel = sc.items[0].sel;
        else if (!sc || !sc.single) st.sel = { sec: "lesson" };
        else st.sel = { sec: sc.key };
      }
    }
    function draw() {
      clamp();
      var focus = document.activeElement && el.contains(document.activeElement) ? document.activeElement : null;
      var fp = focus && focus.dataset.path, ft = focus && focus.dataset.testid, fa = focus && focus.dataset.act, fv = focus && focus.dataset.value;
      var caret = focus && typeof focus.selectionStart === "number" ? focus.selectionStart : null;
      var scroll = el.querySelector(".le-main") ? el.querySelector(".le-main").scrollTop : 0;
      var secs = sections(doc, st.man);
      var active = secs.filter(function (s) { return s.key === st.sel.sec; })[0];
      st.open[st.sel.sec] = true;
      var outline = h("nav", { class: "le-outline", "data-testid": "course-outline", "aria-label": T("outline") }, secs.map(function (s) {
        var isOn = st.sel.sec === s.key;
        var open = !s.single && st.open[s.key] !== false && (st.open[s.key] || isOn);
        var head = h("button", { type: "button", class: "le-sec" + (s.single && isOn ? " on" : ""), "data-act": s.single ? "select" : "fold", "data-sec": s.key, "data-path": s.key, "data-testid": "course-outline-section", "aria-expanded": s.single ? null : (open ? "true" : "false"), "aria-pressed": s.single ? (isOn ? "true" : "false") : null }, [
          h("span", { class: "le-sec-arrow", "aria-hidden": "true", text: s.single ? "" : (open ? "▾" : "▸") }),
          h("span", { class: "le-sec-name", text: s.label }),
          h("span", { class: "le-sec-count", text: s.note != null ? s.note : (s.count != null ? String(s.count) : "") }),
          s.single && itemChecks(doc, st.man, { sec: s.key }).some(function (c) { return c.level === "bad"; }) ? h("span", { class: "le-warn", title: T("has_problem"), text: "⚠" }) : null
        ]);
        var kids = [head];
        if (open) {
          (s.items || []).forEach(function (it) {
            kids.push(h("button", { type: "button", class: "le-item" + (same(it.sel, st.sel) ? " on" : ""), "data-act": "select", "data-sec": it.sel.sec, "data-sub": it.sel.sub || null, "data-idx": it.sel.idx == null ? null : it.sel.idx, "data-path": dotted(itemPath(it.sel)), "data-testid": "course-outline-item", "aria-pressed": same(it.sel, st.sel) ? "true" : "false" }, [
              it.badge ? h("span", { class: "le-badge", text: it.badge }) : null,
              h("span", { class: "le-item-text", text: it.label }),
              it.bad ? h("span", { class: "le-warn", title: T("has_problem"), text: "⚠" }) : null
            ]));
          });
          (s.add || []).forEach(function (a) {
            if (a.hidden) return;
            kids.push(h("button", { type: "button", class: "le-add", "data-act": "add-item", "data-sec": s.key, "data-sub": a.sub || null, "data-testid": "course-outline-add", "data-path": s.key === "sprint" ? "sprint." + a.sub : s.key, text: a.label }));
          });
        }
        return h("div", { class: "le-sec-wrap", "data-sec": s.key }, kids);
      }).concat([h("button", { type: "button", class: "le-json-link", "data-act": "json", "data-testid": "course-outline-json", text: T("edit_as_json") })]));
      var main = h("div", { class: "le-main", "data-testid": "course-item", "data-sec": st.sel.sec, "data-sub": st.sel.sub || null, "data-idx": st.sel.idx == null ? null : st.sel.idx }, form(ctx, st));
      var modes = window.LessonPreview ? LessonPreview.modes(st.sel, doc) : [];
      if (modes.indexOf(st.mode) < 0) st.mode = modes[0] || null;
      var preview = h("aside", { class: "le-preview", "data-testid": "course-preview", "data-mode": st.mode || "" }, [
        h("div", { class: "le-preview-top" }, [h("span", { class: "le-preview-title", text: T("live_preview") }), modes.length ? seg(ctx, "mode", [], modes.map(function (m) { return { value: m, text: T("mode_" + m) }; }), st.mode) : null]),
        h("div", { class: "le-preview-body", "data-testid": "course-preview-body" })
      ]);
      var tabs = h("div", { class: "le-tabs", role: "tablist" }, ["outline", "edit", "preview"].map(function (tb) {
        return h("button", { type: "button", class: "le-tab", role: "tab", "data-act": "tab", "data-value": tb, "data-testid": "course-form-tab-" + tb, "aria-selected": st.tab === tb ? "true" : "false", text: T("tab_" + tb) });
      }));
      var root = h("div", { class: "le", "data-tab": st.tab, "data-testid": "course-form-layout" }, [tabs, outline, main, preview]);
      el.textContent = "";
      el.appendChild(root);
      el.querySelector(".le-main").scrollTop = scroll;
      refresh();
      if (fp != null || fa) {
        var selector = fa ? '[data-act="' + fa + '"][data-path="' + (fp || "") + '"]' + (fv ? '[data-value="' + fv + '"]' : "") : '[data-testid="' + ft + '"][data-path="' + fp.replace(/"/g, '\\"') + '"]';
        var again = el.querySelector(selector);
        if (again) { again.focus(); if (caret != null && typeof again.setSelectionRange === "function") { try { again.setSelectionRange(caret, caret); } catch (e) { /* not a text box */ } } }
      }
    }
    // After a keystroke: the checks, the outline's labels and the preview, with the form left alone.
    var previewSoon = debounce(function () { drawPreview(); }, 120);
    function refresh() {
      var checks = itemChecks(doc, st.man, st.sel);
      var byPath = {};
      checks.forEach(function (c) { (byPath[c.path] = byPath[c.path] || []).push(c); });
      // The course lints (as of the last save): an amber ! in the corner of
      // the field they name (or of the form's title, for what no field of
      // this form holds) opens them in a modal. It takes no room of its own,
      // so drawing the lints again after a save moves nothing (#1199).
      Array.prototype.forEach.call(el.querySelectorAll(".le-lint"), function (b) { b.remove(); });
      var title = el.querySelector(".le-main .le-title"), spots = [];
      (ctx.lints ? ctx.lints(doc.id) : reportLints(doc.id)).forEach(function (f) {
        if (!f.text) return;
        var line = f.path ? el.querySelector('.le-main .le-check[data-check="' + f.path.replace(/"/g, "") + '"]') : null;
        var box = line && line.closest(".le-f, .le-group");
        var spot = spots.filter(function (x) { return x.box === (box || title); })[0];
        if (!spot) spots.push(spot = { box: box || title, path: box ? f.path : null, list: [] });
        spot.list.push(f);
      });
      spots.forEach(function (sp) {
        if (!sp.box) return;
        var b = h("button", { type: "button", class: "le-lint" + (sp.path ? "" : " le-lint-top"), "data-act": "lint", "data-testid": sp.path ? "course-form-lint" : "course-form-advice", "data-path": sp.path, "data-kinds": sp.list.map(function (f) { return f.kind; }).join(" "), "aria-label": T("lint_title"), title: T("lint_title"), text: "!" });
        b._lints = sp.list.map(function (f) { return f.text; });
        sp.box.appendChild(b);
      });
      Array.prototype.forEach.call(el.querySelectorAll(".le-check[data-check]"), function (line) {
        var cs = byPath[line.dataset.check];
        var mark = line.querySelector(".le-check-mark"), text = line.querySelector(".le-check-text");
        if (!cs) { line.hidden = true; text.textContent = ""; line.title = ""; line._checks = []; line.className = "le-check"; delete line.dataset.level; return; }
        var worst = cs.some(function (c) { return c.level === "bad"; }) ? "bad" : cs.some(function (c) { return c.level === "warn"; }) ? "warn" : "ok";
        line.hidden = false;
        line.className = "le-check " + worst;
        // Worst first (#1235), so advice never hides a refusal; the mark opens them all (#1320).
        var rank = { bad: 0, warn: 1, ok: 2 };
        line._checks = cs.slice().sort(function (a, b) { return rank[a.level] - rank[b.level]; }).map(function (c) { return (c.level === "bad" ? "✕ " : c.level === "warn" ? "⚠ " : "✓ ") + c.text; });
        mark.textContent = worst === "ok" ? "✓" : "!";
        text.textContent = line.title = line._checks.join(" · ");
        line.dataset.level = worst;
      });
      // Problems the server would refuse are flagged in the outline too.
      var main = el.querySelector(".le-main");
      if (main) main.dataset.problem = checks.some(function (c) { return c.level === "bad"; }) ? "true" : "false";
      // Every outline row follows the document: labels, counts and the ⚠ of
      // any item (a word renamed changes a question's standing).
      var flag = function (node, bad) {
        if (!node) return;
        var warn = node.querySelector(".le-warn");
        if (bad && !warn) node.appendChild(h("span", { class: "le-warn", title: T("has_problem"), text: "⚠" }));
        if (!bad && warn) warn.remove();
      };
      sections(doc, st.man).forEach(function (sc) {
        var head = el.querySelector('.le-sec[data-sec="' + sc.key + '"]');
        if (head) {
          var cnt = head.querySelector(".le-sec-count");
          if (cnt) cnt.textContent = sc.note != null ? sc.note : (sc.count != null ? String(sc.count) : "");
          if (sc.single) flag(head, itemChecks(doc, st.man, { sec: sc.key }).some(function (c) { return c.level === "bad"; }));
        }
        (sc.items || []).forEach(function (it) {
          var node = el.querySelector('.le-item[data-path="' + dotted(itemPath(it.sel)) + '"]');
          if (!node) return;
          var text = node.querySelector(".le-item-text");
          if (text) text.textContent = it.label;
          flag(node, it.bad);
        });
      });
      var sp = el.querySelector(".le-words");
      if (sp && st.sel.sec === "sprint" && st.sel.sub === "builds") {
        var b = get(doc, itemPath(st.sel));
        sp.textContent = "";
        words(b && b.text).forEach(function (w) { sp.appendChild(h("span", { class: "le-word static", lang: ctx.termLang, text: w })); });
      }
      previewSoon();
    }
    function drawPreview() {
      var body = el.querySelector(".le-preview-body");
      if (!body || !window.LessonPreview) return;
      LessonPreview.render(body, { doc: doc, sel: st.sel, mode: st.mode, man: st.man || {}, api: ctx.api, termLang: ctx.termLang, step: st.step, lessons: ctx.lessons ? ctx.lessons() : [] });
    }
    if (first) listen(el);
    draw();
  }

  // The drawer's events: typing writes into the document; buttons add,
  // remove, move, pick and switch.
  function listen(el) {
    el.addEventListener("input", function (ev) {
      var ctx = el._lectx, st = el._le, doc = ctx.doc;
      var t0 = ev.target;
      if (t0.dataset.act === "new-option") {
        // The dashed box under the options: the first letter makes a new option.
        var path = parsePath(t0.dataset.path);
        var arr = get(doc, path);
        if (!Array.isArray(arr)) { put(doc, path, []); arr = get(doc, path); }
        arr.push(t0.value);
        var caret = t0.value.length;
        ctx.changed(true);
        var fresh = el.querySelector('[data-testid="course-form-field"][data-path="' + dotted(path.concat(arr.length - 1)) + '"]');
        if (fresh) { fresh.focus(); fresh.setSelectionRange(caret, caret); }
        return;
      }
      if (!t0.dataset.path || !t0.dataset.kind) return;
      var p = parsePath(t0.dataset.path), kind = t0.dataset.kind;
      if (kind === "number") {
        if (t0.value === "") drop(doc, p);
        else { var n = Number(t0.value); if (isFinite(n)) put(doc, p, n); }
      } else if (kind === "lines") {
        var lines = t0.value.split("\n");
        if (lines.length === 1 && !lines[0]) drop(doc, p); else put(doc, p, lines);
      } else if (kind === "opt" && !t0.value.trim()) {
        drop(doc, p);
      } else {
        var was = get(doc, p);
        put(doc, p, t0.value);
        if (p.length === 3 && p[0] === "vocabulary" && p[2] === "term") followTerm(doc, p[1], was, t0.value);
      }
      // An option typed while it is the right answer keeps being the answer.
      if (p[p.length - 2] === "options") {
        var q = get(doc, p.slice(0, -2));
        var radio = el.querySelector('.le-correct[data-path="' + dotted(p) + '"]');
        if (q && radio && radio.checked) q.correct_answer = t0.value;
      }
      // The sentence of a spot keeps its wrong word while the word is still in it.
      if (p[p.length - 1] === "sentence") {
        var spot = get(doc, p.slice(0, -1));
        var wordsEl = el.querySelector(".le-words");
        if (spot && wordsEl) {
          wordsEl.textContent = "";
          spotWords(ctx, p.slice(0, -1), spot).forEach(function (b) { wordsEl.appendChild(b); });
        }
      }
      ctx.changed();
    });
    el.addEventListener("change", function (ev) {
      var ctx = el._lectx, doc = ctx.doc, t0 = ev.target;
      // A spot's music typed in or cleared (#1227): its form switches between sentence and staff.
      var tp0 = t0.dataset && t0.dataset.path ? parsePath(t0.dataset.path) : [];
      if (tp0[0] === "sprint" && tp0[1] === "spots" && tp0[tp0.length - 1] === "abc" && !!el.querySelector("[data-staff-spot]") !== !!get(doc, tp0)) { ctx.changed(true); return; }
      if (t0.classList.contains("le-correct")) {
        var p = parsePath(t0.dataset.path);
        var q = get(doc, p.slice(0, -2));
        if (q) q.correct_answer = get(doc, p);
        ctx.changed();
        return;
      }
      if (t0.tagName === "SELECT" && t0.dataset.act === "add-teaches" && t0.value) {
        var tp = parsePath(t0.dataset.path);
        var arr = get(doc, tp); if (!Array.isArray(arr)) { put(doc, tp, []); arr = get(doc, tp); }
        arr.push(t0.value); ctx.changed(true); return;
      }
      if (t0.tagName === "SELECT" && t0.dataset.act === "add-prereq" && t0.value) {
        var pre = doc.prerequisites; if (!Array.isArray(pre)) pre = doc.prerequisites = [];
        pre.push(t0.value); ctx.changed(true); return;
      }
      if (t0.tagName === "SELECT" && t0.dataset.path && t0.dataset.kind) {
        put(doc, parsePath(t0.dataset.path), t0.value);
        ctx.changed(true);
      }
    });
    el.addEventListener("click", function (ev) {
      var b = ev.target.closest("[data-act]");
      if (!b || !el.contains(b) || b.tagName === "SELECT" || b.tagName === "INPUT") return;
      var ctx = el._lectx, st = el._le, doc = ctx.doc;
      var act = b.dataset.act, path = b.dataset.path != null ? parsePath(b.dataset.path) : [];
      ev.preventDefault();
      var redraw = true;
      if (act === "lint") { showLints(b._lints || []); return; }
      if (act === "check") { showChecks(b._checks || []); return; }
      if (act === "fold") { st.open[b.dataset.sec] = b.getAttribute("aria-expanded") !== "true"; draw(); return; }
      if (act === "select") {
        // Another section opens in the mode that uses its kind most; within a section the mode sticks.
        if (b.dataset.sec !== st.sel.sec) st.mode = null;
        st.sel = b.dataset.idx != null ? { sec: b.dataset.sec, sub: b.dataset.sub || null, idx: +b.dataset.idx } : (b.dataset.sub ? { sec: b.dataset.sec, sub: b.dataset.sub } : { sec: b.dataset.sec });
        if (st.tab === "outline") st.tab = "edit";
        draw(); return;
      }
      if (act === "tab") { st.tab = b.dataset.value; draw(); return; }
      if (act === "mode") { st.mode = b.dataset.value; draw(); return; }
      if (act === "json") { if (ctx.toJson) ctx.toJson(); return; }
      if (act === "say") { var text = get(doc, path); if (text && window.speechSynthesis) { var u = new SpeechSynthesisUtterance(String(text)); if (ctx.termLang) u.lang = ctx.termLang; speechSynthesis.cancel(); speechSynthesis.speak(u); } return; }
      if (act === "add-item") {
        var sec = b.dataset.sec, sub = b.dataset.sub || null;
        if (sec === "sprint") {
          if (!doc.sprint || typeof doc.sprint !== "object") doc.sprint = {};
          if (sub === "icon") { doc.sprint.icon = doc.sprint.icon || "⭐"; st.sel = { sec: "sprint", sub: "icon" }; }
          else { if (!Array.isArray(doc.sprint[sub])) doc.sprint[sub] = []; doc.sprint[sub].push(blank(doc, sec, sub, st.man)); st.sel = { sec: "sprint", sub: sub, idx: doc.sprint[sub].length - 1 }; }
        } else {
          if (!Array.isArray(doc[sec])) doc[sec] = [];
          doc[sec].push(blank(doc, sec, null, st.man));
          st.sel = { sec: sec, idx: doc[sec].length - 1 };
        }
        st.open[sec] = true;
        if (st.tab === "outline") st.tab = "edit";
        ctx.changed(true);
        var firstBox = el.querySelector(".le-main input:not([type=radio]), .le-main textarea");
        if (firstBox) firstBox.focus();
        return;
      }
      if (act === "add") {
        var arr = get(doc, path);
        if (!Array.isArray(arr)) { put(doc, path, []); arr = get(doc, path); }
        arr.push(path[path.length - 1] === "lines" ? { speaker: arr.length % 2 ? "b" : "a", text: "", meaning: "" } : "");
        ctx.changed(true);
        var last = el.querySelectorAll('[data-testid="course-form-list"][data-path="' + dotted(path) + '"] [data-testid="course-form-item"]');
        var box = last.length && last[last.length - 1].querySelector("input:not([type=radio]), textarea");
        if (box) box.focus();
        return;
      }
      if (act === "remove" || act === "delete") {
        var isItem = dotted(path) === dotted(itemPath(st.sel)) && st.sel.idx != null;
        var isIcon = st.sel.sec === "sprint" && st.sel.sub === "icon" && dotted(path) === "sprint.icon";
        if (isItem || isIcon) {
          if (ctx.confirmDelete && !ctx.confirmDelete()) return;
          var gone = get(doc, path);
          drop(doc, path);
          unlink(doc, st.sel.sec, gone);
          var parentArr = get(doc, path.slice(0, -1));
          if (Array.isArray(parentArr) && !parentArr.length) drop(doc, path.slice(0, -1));
          if (isIcon) delete doc.sprint.icon;
          // The next item, or the one before, stays in view.
          ctx.changed(true);
          return;
        }
        var removed = get(doc, path);
        drop(doc, path);
        // The option that was the answer: no answer until another is picked.
        var owner = path[path.length - 2] === "options" ? get(doc, path.slice(0, -2)) : null;
        if (owner && owner.correct_answer === removed && (owner.options || []).indexOf(removed) < 0) owner.correct_answer = "";
        ctx.changed(true);
        var sib = el.querySelector('[data-act="remove"][data-path="' + dotted(path) + '"]') || el.querySelector('[data-act="remove"][data-path="' + dotted(path.slice(0, -1).concat(Math.max(0, path[path.length - 1] - 1))) + '"]');
        if (sib) sib.focus();
        return;
      }
      if (act === "up" || act === "down" || act === "prev" || act === "next") {
        var at = path[path.length - 1], parent = get(doc, path.slice(0, -1));
        if (act === "prev" || act === "next") {
          var to = act === "prev" ? st.sel.idx - 1 : st.sel.idx + 1;
          if (Array.isArray(parent) && to >= 0 && to < parent.length) st.sel = Object.assign({}, st.sel, { idx: to });
          draw(); return;
        }
        if (!Array.isArray(parent)) return;
        var dest = act === "up" ? at - 1 : at + 1;
        if (dest < 0 || dest >= parent.length) return;
        parent.splice(dest, 0, parent.splice(at, 1)[0]);
        if (path[0] === "guide" && path.length === 2) st.step = dest;
        else if (dotted(path.slice(0, -1)) === dotted(itemPath(st.sel).slice(0, -1)) && st.sel.idx === at) st.sel = Object.assign({}, st.sel, { idx: dest });
        ctx.changed(true);
        var again = el.querySelector('[data-act="' + act + '"][data-path="' + dotted(path.slice(0, -1).concat(dest)) + '"]');
        if (again && !again.disabled) again.focus();
        return;
      }
      if (act === "duplicate") {
        var src = get(doc, path), list = get(doc, path.slice(0, -1));
        if (!Array.isArray(list)) return;
        var copy = JSON.parse(JSON.stringify(src));
        if (copy && typeof copy === "object") {
          if ("question_id" in copy) copy.question_id = madeId(doc, "question_id");
          if (st.sel.sec === "conversations") copy.id = madeId(doc, "conv");
        }
        list.splice(st.sel.idx + 1, 0, copy);
        st.sel = Object.assign({}, st.sel, { idx: st.sel.idx + 1 });
        ctx.changed(true); return;
      }
      if (act === "type") {
        var q = get(doc, path.slice(0, -1)), was = q.type, now = b.dataset.value;
        if (was === now) return;
        q.type = now;
        // Options with text in them are kept for a switch back; empty ones go.
        if (now === "multiple-choice") { if (!Array.isArray(q.options) || q.options.length < 2) q.options = (Array.isArray(q.options) ? q.options : []).concat(["", ""]).slice(0, Math.max(2, (q.options || []).length)); if (q.options.indexOf(q.correct_answer) < 0 && q.correct_answer) q.options[0] = q.correct_answer; }
        else if (Array.isArray(q.options) && !q.options.some(function (o) { return String(o).trim(); })) delete q.options;
        if (now === "true-false" && q.correct_answer !== "true" && q.correct_answer !== "false") q.correct_answer = "true";
        if (now !== "numeric") { delete q.tolerance; delete q.unit; }
        if (now !== "fill-bar") delete q.exact;
        if (now === "staff-note") { if (!q.clef) q.clef = "treble"; } else { delete q.clef; delete q.any_octave; }
        if (now !== "keyboard") { delete q.octave; delete q.octaves; }
        if (now !== "clock") delete q.step_minutes;
        if ((now === "multiple-choice" || now === "true-false") && Array.isArray(q.accepted_answers) && !q.accepted_answers.some(function (o) { return String(o).trim(); })) delete q.accepted_answers;
        ctx.changed(true); return;
      }
      if (act === "tf") { put(doc, path, b.dataset.value); ctx.changed(true); return; }
      if (act === "octaves") { if (b.dataset.value === "2") put(doc, path, 2); else drop(doc, path); ctx.changed(true); return; }
      if (act === "octave") { if (b.dataset.value === "exact") put(doc, path, true); else drop(doc, path); ctx.changed(true); return; }
      if (act === "option-kind") { var oq = get(doc, path.slice(0, -1)); if (b.dataset.value === "abc") oq.option_kind = "abc"; else delete oq.option_kind; ctx.changed(true); return; }
      if (act === "exact") { if (b.dataset.value === "only") put(doc, path, true); else drop(doc, path); ctx.changed(true); return; }
      if (act === "any-octave") { if (get(doc, path)) drop(doc, path); else put(doc, path, true); ctx.changed(true); return; }
      if (act === "speaker") { put(doc, path, b.dataset.value); ctx.changed(true); return; }
      if (act === "spot") { put(doc, path, b.dataset.value.replace(/^[.!¡?¿,;:"'’‘«»]+|[.!¡?¿,;:"'’‘«»]+$/g, "")); ctx.changed(true); return; }
      if (act === "bucket-prev" || act === "bucket-next") {
        var item = get(doc, path);
        if (item) { item.bucket = Math.max(0, (item.bucket || 0) + (act === "bucket-prev" ? -1 : 1)); ctx.changed(true); }
        return;
      }
      if (act === "add-sort-item") {
        var items = get(doc, path); if (!Array.isArray(items)) { put(doc, path, []); items = get(doc, path); }
        items.push({ text: "", bucket: +b.dataset.bucket || 0 });
        ctx.changed(true);
        var newBox = el.querySelector('[data-testid="course-form-field"][data-path="' + dotted(path.concat(items.length - 1, "text")) + '"]');
        if (newBox) newBox.focus();
        return;
      }
      if (act === "add-bucket") {
        var bs = get(doc, path); if (!Array.isArray(bs)) { put(doc, path, []); bs = get(doc, path); }
        if (bs.length >= 4) return;
        bs.push(""); ctx.changed(true);
        var nb = el.querySelector('[data-testid="course-form-field"][data-path="' + dotted(path.concat(bs.length - 1)) + '"]');
        if (nb) nb.focus();
        return;
      }
      if (act === "remove-bucket") {
        var bi = path[path.length - 1], sort = get(doc, path.slice(0, -2));
        if (!sort || !Array.isArray(sort.buckets)) return;
        sort.buckets.splice(bi, 1);
        (sort.items || []).forEach(function (x) { if (x.bucket > bi) x.bucket -= 1; else if (x.bucket === bi) x.bucket = Math.max(0, bi - 1); });
        ctx.changed(true); return;
      }
      if (act === "step") { st.step = path[1]; draw(); return; }
      if (act === "add-step") {
        if (!Array.isArray(doc.guide)) doc.guide = [];
        doc.guide.push(blankStep(doc, b.dataset.value));
        st.step = doc.guide.length - 1;
        ctx.changed(true);
        var sb = el.querySelector(".le-main .le-group:last-of-type input, .le-main .le-group:last-of-type textarea, .le-main .le-group:last-of-type select");
        if (sb) sb.focus();
        return;
      }
      if (act === "guide-by-hand") { doc.guide = derivedSteps(doc); st.step = 0; ctx.changed(true); return; }
      if (act === "guide-derive") { if (ctx.confirmDerive && !ctx.confirmDerive()) return; delete doc.guide; ctx.changed(true); return; }
      if (redraw) draw();
    });
    // Drag a sort item between buckets.
    el.addEventListener("dragstart", function (ev) {
      var it = ev.target.closest && ev.target.closest(".le-sort-item");
      if (!it) return;
      ev.dataTransfer.setData("text/plain", it.dataset.path);
      ev.dataTransfer.effectAllowed = "move";
      it.classList.add("dragging");
    });
    el.addEventListener("dragend", function (ev) { var it = ev.target.closest && ev.target.closest(".le-sort-item"); if (it) it.classList.remove("dragging"); });
    el.addEventListener("dragover", function (ev) { var bk = ev.target.closest && ev.target.closest(".le-bucket"); if (bk) { ev.preventDefault(); bk.classList.add("over"); } });
    el.addEventListener("dragleave", function (ev) { var bk = ev.target.closest && ev.target.closest(".le-bucket"); if (bk) bk.classList.remove("over"); });
    el.addEventListener("drop", function (ev) {
      var bk = ev.target.closest && ev.target.closest(".le-bucket");
      if (!bk) return;
      ev.preventDefault();
      bk.classList.remove("over");
      var ctx = el._lectx, doc = ctx.doc;
      var p = parsePath(ev.dataTransfer.getData("text/plain"));
      var item = get(doc, p);
      if (item && typeof item === "object") { item.bucket = +bk.dataset.bucket; ctx.changed(true); }
    });
    function draw() { var ctx = el._lectx; render(el, ctx.doc, ctx); }
  }

  // A word whose term was typed over: what names it follows, unless another
  // entry has the old term or the new one. While the new term is another
  // word's (a half-typed "habla" on the way back to "hablar"), the questions
  // keep the name they had, and follow again once the term is its own.
  var follows = new WeakMap();
  function followTerm(doc, idx, was, now) {
    var word = doc.vocabulary[idx];
    if (!word || typeof word !== "object") return;
    var from = follows.has(word) ? follows.get(word) : was;
    var words = doc.vocabulary || [];
    var heldBy = function (t, not) { return words.some(function (v, i) { return i !== not && v && v.term === t; }); };
    if (heldBy(now, idx)) follows.set(word, from);
    else {
      follows.delete(word);
      if (from !== now && !heldBy(from, idx)) renameTerm(doc, from, now);
    }
    // A word left on another's term, whose clash this edit ended (the other
    // word was renamed instead): its questions follow it now.
    words.forEach(function (v, i) {
      if (i === idx || !v || !follows.has(v) || heldBy(v.term, i)) return;
      var f = follows.get(v);
      follows.delete(v);
      if (f !== v.term && !heldBy(f, i)) renameTerm(doc, f, v.term);
    });
  }
  // A word renamed: the questions that teach it follow.
  function renameTerm(doc, was, now) {
    (Array.isArray(doc.quiz) ? doc.quiz : []).forEach(function (q) { if (q && Array.isArray(q.teaches)) q.teaches = q.teaches.map(function (x) { return x === was ? now : x; }).filter(function (x, i, all) { return all.indexOf(x) === i; }); });
  }
  // What pointed at a deleted item goes with it: a word's place in the
  // questions that teach it, a question's steps in a hand-ordered Guide.
  function unlink(doc, sec, gone) {
    if (!gone || typeof gone !== "object") return;
    if (sec === "vocabulary" && !(doc.vocabulary || []).some(function (v) { return v && v.term === gone.term; })) {
      var untaught = function (qs) {
        (Array.isArray(qs) ? qs : []).forEach(function (q) {
          if (!q || !Array.isArray(q.teaches)) return;
          q.teaches = q.teaches.filter(function (x) { return x !== gone.term; });
          if (!q.teaches.length) delete q.teaches;
        });
      };
      untaught(doc.quiz);
    }
    if (sec === "quiz" && Array.isArray(doc.guide) && !(doc.quiz || []).some(function (q) { return q && q.question_id === gone.question_id; })) {
      doc.guide = doc.guide.filter(function (s) { return !(s && s.question === gone.question_id); });
      if (!doc.guide.length) delete doc.guide;
    }
  }

  // A server problem names a top-level field: open that section.
  function reveal(el, field) {
    var st = el._le, ctx = el._lectx;
    if (!st || !ctx || !field) return false;
    var sec = field === "sprint" || field === "guide" || field === "content" ? field : ["vocabulary", "quiz", "listening", "conversations", "conjugations"].indexOf(field) >= 0 ? field : "lesson";
    var secs = sections(ctx.doc, st.man);
    var s = secs.filter(function (x) { return x.key === sec; })[0];
    if (!s) return false;
    // The open item when the rules refuse it (a copy just made), else the first item they refuse, else the section's first item.
    var bad = (s.items || []).filter(function (x) { return x.bad && same(x.sel, st.sel); })[0] || (s.items || []).filter(function (x) { return x.bad; })[0];
    var want = bad ? bad.sel : (s.items && s.items.length ? s.items[0].sel : { sec: s.key });
    // The Guide opens on its first step the rules refuse.
    var badStep = sec === "guide" ? itemChecks(ctx.doc, st.man, { sec: "guide" }).filter(function (c) { return c.level === "bad"; })[0] : null;
    var step = badStep ? +badStep.path.split(".")[1] : st.step;
    // Already there: nothing to draw again.
    if (same(want, st.sel) && st.tab === "edit" && step === st.step) return true;
    st.sel = want;
    st.step = step;
    st.open[sec] = true;
    if (st.tab === "outline" || st.tab === "preview") st.tab = "edit";
    render(el, ctx.doc, ctx);
    return true;
  }

  // The course lints of lesson `id` as the page's report lists them (src/lint.rs, templates/course_report.html).
  function reportLints(id) {
    var rows = document.querySelectorAll('#course-report [data-testid=course-lint][data-lesson="' + String(id || "").replace(/"/g, "") + '"]');
    return Array.prototype.map.call(rows, function (li) {
      var text = li.querySelector(".report-lint-text");
      return { kind: li.dataset.kind, path: li.dataset.path || "", text: text ? text.textContent.trim() : "" };
    });
  }
  // A field's course lints in a modal (the amber ! opens it): one shared <dialog>.
  function showLints(texts) {
    var d = document.getElementById("le-lint-dialog");
    if (!d) {
      d = h("dialog", { class: "modal", id: "le-lint-dialog", "aria-labelledby": "le-lint-dialog-title", "data-testid": "course-form-lint-dialog" }, [
        h("h2", { id: "le-lint-dialog-title", text: T("lint_title") }),
        h("ul", { class: "le-lint-list", "data-testid": "course-form-lint-list" }),
        h("div", { class: "modal-actions" }, [h("button", { type: "button", class: "btn ghost", "data-testid": "course-form-lint-close", text: T("lint_close") })])
      ]);
      d.querySelector("button").addEventListener("click", function () { d.close(); });
      document.body.appendChild(d);
    }
    var ul = d.querySelector("ul");
    ul.textContent = "";
    texts.forEach(function (t) { ul.appendChild(h("li", { text: t })); });
    if (d.showModal && !d.open) d.showModal();
  }
  // The checks again, for one form or every open one (the page calls it when the report is drawn again).
  function refreshAll(el) {
    var hosts = el ? [el] : Array.prototype.slice.call(document.querySelectorAll(".le-host"));
    hosts.forEach(function (host) { if (host._le && host._le.refresh) host._le.refresh(); });
  }

  // An item picked from the course's "Things to check" (static/advice.js):
  // its section open, the item in the Edit column and the field named by
  // `path` ("quiz.2.explanation") in sight and focused.
  function select(el, sel, path) {
    var st = el._le, ctx = el._lectx;
    if (!st || !ctx || !sel) return false;
    st.sel = sel;
    st.open[sel.sec] = true;
    if (st.tab === "outline" || st.tab === "preview") st.tab = "edit";
    render(el, ctx.doc, ctx);
    var q = String(path || "").replace(/"/g, '\\"');
    var f = el.querySelector('.le-f[data-field="' + q + '"]') || el.querySelector('[data-path="' + q + '"]');
    if (!f) return true;
    f.scrollIntoView({ block: "center" });
    var input = f.matches("input, textarea, select") ? f : f.querySelector("input, textarea, select, button");
    if (input) input.focus();
    return true;
  }

  // A field's checks in a modal (its ✓ or ! opens it): one shared <dialog>.
  function showChecks(texts) {
    var d = document.getElementById("le-check-dialog");
    if (!d) {
      d = h("dialog", { class: "modal", id: "le-check-dialog", "aria-labelledby": "le-check-dialog-title", "data-testid": "course-form-check-dialog" }, [
        h("h2", { id: "le-check-dialog-title", text: T("field_checks_title") }),
        h("ul", { class: "le-check-list", "data-testid": "course-form-check-list" }),
        h("div", { class: "modal-actions" }, [h("button", { type: "button", class: "btn ghost", "data-testid": "course-form-check-close", text: T("field_checks_close") })])
      ]);
      d.querySelector("button").addEventListener("click", function () { d.close(); });
      document.body.appendChild(d);
    }
    var ul = d.querySelector("ul");
    ul.textContent = "";
    texts.forEach(function (x) { ul.appendChild(h("li", { text: x })); });
    if (d.showModal && !d.open) d.showModal();
  }

  window.LessonEditor = { render: render, reveal: reveal, refresh: refreshAll, select: select, checks: itemChecks, sections: sections, headings: headings, words: words, norm: norm };
})();
