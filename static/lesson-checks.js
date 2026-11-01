// The lesson editor's checks (static/lesson-editor.js, split out to keep that
// file under the size limit). LessonChecks.make(helpers) returns itemChecks,
// built on the editor's own helpers, so both read a document the same way.
(function () {
  "use strict";
  window.LessonChecks = { make: function (d) {
    var T = d.T, QUIZ_TYPES = d.QUIZ_TYPES, ABC_NOTE = d.ABC_NOTE, languageOn = d.languageOn, dotted = d.dotted, get = d.get, words = d.words, norm = d.norm, isIcon = d.isIcon, headings = d.headings, hasIntro = d.hasIntro, unreadable = d.unreadable, itemPath = d.itemPath;
  // The course rules (src/content.rs validate_lesson) for one item, plus a
  // little advice. [{ path, level: "ok" | "warn" | "bad", text }]. A "bad"
  // one is what the server would refuse on save.
  function itemChecks(doc, man, sel) {
    var out = [];
    var at = function (path, level, text) { out.push({ path: dotted(path), level: level, text: text }); };
    var p = itemPath(sel);
    var it = get(doc, p);
    var sec = sel.sec, sub = sel.sub;
    if (sec === "quiz" && it) {
      var type = QUIZ_TYPES.indexOf(it.type) >= 0 ? it.type : null;
      if (!String(it.prompt || "").trim()) at(p.concat("prompt"), "bad", T("check_prompt_empty"));
      else if (type === "fill-in-blank") at(p.concat("prompt"), String(it.prompt).indexOf("___") >= 0 ? "ok" : "warn", String(it.prompt).indexOf("___") >= 0 ? T("check_blank_ok") : T("check_blank_missing"));
      if (!type) at(p.concat("type"), "bad", T("check_type"));
      var opts = Array.isArray(it.options) ? it.options : [];
      if (type === "multiple-choice") {
        var filled = opts.filter(function (o) { return String(o).trim(); });
        if (filled.length < 2) at(p.concat("options"), "bad", T("check_options_two"));
        else if (opts.indexOf(it.correct_answer) < 0 || !String(it.correct_answer || "").trim()) at(p.concat("options"), "bad", T("check_options_right"));
        else if (new Set(opts.map(norm)).size !== opts.length) at(p.concat("options"), "warn", T("check_options_same"));
        else at(p.concat("options"), "ok", T("check_options_ok", { n: opts.length }));
      }
      if (type === "fill-in-blank" && !String(it.correct_answer || "").trim()) at(p.concat("correct_answer"), "bad", T("check_answer_empty"));
      if (type === "fill-bar" && (Array.isArray(it.tiles) ? it.tiles : []).filter(function (x) { return String(x).trim(); }).length < 2) at(p.concat("tiles"), "bad", T("check_tiles_two"));
      if (type === "true-false" && !/^(true|false|yes|no)$/i.test(String(it.correct_answer || "").trim())) at(p.concat("correct_answer"), "bad", T("check_true_false"));
      if (type === "numeric" || type === "expression") {
        var accepted = Array.isArray(it.accepted_answers) ? it.accepted_answers : [];
        var blank = function (a) { return !String(a == null ? "" : a).trim(); };
        if (blank(it.correct_answer)) at(p.concat("correct_answer"), "bad", T("check_answer_empty"));
        else if (accepted.some(blank)) at(p.concat("accepted_answers"), "bad", T("check_answer_empty"));
        else {
          var odd = unreadable(it);
          if (odd) at(p.concat(odd === String(it.correct_answer) ? "correct_answer" : "accepted_answers"), "bad", T("check_" + type + "_read", { answer: odd }));
        }
      }
      if (type === "numeric") {
        if (it.tolerance != null && !(typeof it.tolerance === "number" && isFinite(it.tolerance) && it.tolerance >= 0)) at(p.concat("tolerance"), "bad", T("check_tolerance"));
        else if (typeof it.tolerance === "number" && it.tolerance > 0) at(p.concat("tolerance"), "ok", T("check_tolerance_ok", { n: it.tolerance }));
      }
      if (type === "chord-spell") {
        // A note's spelling without its octave, as the server compares them (src/notation/chord_spell.rs).
        var spell = function (n) { return String(n).replace(/[,']/g, "").replace(/^=/, "").toUpperCase(); };
        var chordNotes = String(it.correct_answer || "").split(/\s+/).filter(Boolean);
        var tileNotes = (Array.isArray(it.tiles) ? it.tiles : String(it.tiles || "").split(/\s+/)).filter(function (x) { return String(x).trim(); }).map(spell);
        if (chordNotes.length < 3 || chordNotes.length > 4) at(p.concat("correct_answer"), "bad", T("check_chord_notes"));
        else if (chordNotes.some(function (n) { return tileNotes.indexOf(spell(n)) < 0; })) at(p.concat("tiles"), "bad", T("check_chord_tiles"));
      }
      // One ABC note with its octave, as src/notation/staff_note.rs reads it.
      if (type === "algebra-tiles" && !String(it.correct_answer || "").trim()) at(p.concat("correct_answer"), "bad", T("check_answer_empty"));
      if (type === "clock" && !/^\s*(1[0-2]|[1-9]):[0-5]\d\s*$/.test(String(it.correct_answer || ""))) at(p.concat("correct_answer"), "bad", T("check_clock"));
      if (type === "staff-note" && !/^[\^_=]?[A-Ga-g][,']*$/.test(String(it.correct_answer || "").trim())) at(p.concat("correct_answer"), "bad", T("check_staff_note"));
      if (type === "keyboard" && !ABC_NOTE.test(String(it.correct_answer || "").trim())) at(p.concat("correct_answer"), "bad", T("check_key_note"));
      if (type && type !== "numeric" && type !== "expression" && (it.tolerance != null || (it.unit && String(it.unit).trim()))) at(p.concat("unit"), "bad", T("check_unit_numeric"));
      var terms = (Array.isArray(doc.vocabulary) ? doc.vocabulary : []).map(function (v) { return v && v.term; });
      (Array.isArray(it.teaches) ? it.teaches : []).forEach(function (term) {
        if (terms.indexOf(term) < 0) at(p.concat("teaches"), "bad", T("check_teaches", { term: term }));
      });
      if (!String(it.explanation || "").trim()) at(p.concat("explanation"), "warn", T("check_explanation"));
    } else if (sec === "vocabulary" && it) {
      if (!String(it.term || "").trim() || !String(it.meaning || "").trim()) at(p.concat(String(it.term || "").trim() ? "meaning" : "term"), "bad", T("check_vocab_both"));
      else {
        var dup = (doc.vocabulary || []).some(function (v, i) { return i !== sel.idx && v && v.term === it.term; });
        at(p.concat("term"), dup ? "bad" : "ok", dup ? T("check_vocab_dup") : T("check_vocab_unique"));
      }
      if (it.example && !it.example_meaning && languageOn(man)) at(p.concat("example_meaning"), "warn", T("check_example_meaning"));
    } else if (sec === "listening" && it) {
      var n = words(it.text).length;
      if (n < 1 || n > 30) at(p.concat("text"), "bad", T("check_listening_words", { n: n }));
      else {
        var twice = (doc.listening || []).some(function (l, i) { return i !== sel.idx && l && norm(l.text) === norm(it.text); });
        at(p.concat("text"), twice ? "bad" : "ok", twice ? T("check_listening_twice") : T("check_listening_ok", { n: n }));
      }
      if (it.note != null && !String(it.note).trim()) at(p.concat("note"), "bad", T("check_note_empty"));
      else if (/^New word: .+ = .+/.test(String(it.note || ""))) at(p.concat("note"), "ok", T("check_note_new_word"));
    } else if (sec === "conversations" && it) {
      var lines = Array.isArray(it.lines) ? it.lines : [];
      var a = lines.some(function (l) { return l && l.speaker === "a"; }), b = lines.some(function (l) { return l && l.speaker === "b"; });
      if (lines.length < 2) at(p.concat("lines"), "bad", T("check_conv_two"));
      else if (!a || !b) at(p.concat("lines"), "bad", T("check_conv_both"));
      else if (lines.some(function (l) { return !l || !String(l.text || "").trim(); })) at(p.concat("lines"), "bad", T("check_conv_empty"));
      else {
        var builds = lines.filter(function (l) { var w = words(l.text).length; return w >= 3 && w <= 9 && String(l.meaning || "").trim(); }).length;
        at(p.concat("lines"), "ok", T("check_conv_ok", { n: builds }));
      }
    } else if (sec === "conjugations" && it) {
      var para = man && man.paradigm;
      if (!String(it.item || "").trim() || !String(it.meaning || "").trim()) at(p.concat("item"), "bad", T("check_conj_both"));
      if (!para) at(p.concat("set"), "bad", T("check_conj_no_paradigm"));
      else {
        if (!para.sets.some(function (s) { return s.key === it.set; })) at(p.concat("set"), "bad", T("check_conj_set", { sets: para.sets.map(function (s) { return s.key; }).join(", ") }));
        else if (String(it.item || "").trim() && (doc.conjugations || []).some(function (k, i) { return i !== sel.idx && k && k.item === it.item && k.set === it.set; })) at(p.concat("item"), "bad", T("check_conj_dup", { item: it.item }));
        var forms = it.forms && typeof it.forms === "object" ? it.forms : {};
        var missing = para.slots.filter(function (s) { return !String(forms[s.key] || "").trim(); }).map(function (s) { return s.label; });
        if (missing.length) at(p.concat("forms"), "bad", T("check_conj_missing", { slots: missing.join(", ") }));
        else at(p.concat("forms"), "ok", T("check_conj_ok"));
        Object.keys(forms).forEach(function (k) { if (!para.slots.some(function (s) { return s.key === k; })) at(p.concat("forms"), "bad", T("check_conj_unknown", { slot: k })); });
      }
      var art = Array.isArray(it.art) ? it.art : [];
      if (art.length > 8 || art.some(function (l) { return Array.from(String(l)).length > 24; })) at(p.concat("art"), "bad", T("check_art"));
    } else if (sec === "sprint" && sub === "icon") {
      if (!isIcon(it)) at(p, "bad", T("check_icon"));
    } else if (sec === "sprint" && it) {
      if (sub === "bites") {
        if (!String(it.title || "").trim() || !String(it.rule || "").trim()) at(p.concat(String(it.title || "").trim() ? "rule" : "title"), "bad", T("check_bite"));
        if (it.emoji && !isIcon(it.emoji)) at(p.concat("emoji"), "bad", T("check_icon"));
      } else if (sub === "sorts") {
        var buckets = Array.isArray(it.buckets) ? it.buckets : [], items = Array.isArray(it.items) ? it.items : [];
        if (buckets.length < 2 || buckets.length > 4) at(p.concat("buckets"), "bad", T("check_sort_buckets"));
        if (items.length < 3 || items.length > 10) at(p.concat("items"), "bad", T("check_sort_items", { n: items.length }));
        else if (items.some(function (x) { return !x || (!String(x.text || "").trim() && !String(x.play || "").trim()); })) at(p.concat("items"), "bad", T("check_sort_empty"));
        else if (items.some(function (x) { return !(x.bucket >= 0 && x.bucket < buckets.length); })) at(p.concat("items"), "bad", T("check_sort_bucket"));
        else at(p.concat("items"), "ok", T("check_sort_ok", { n: items.length }));
      } else if (sub === "spots" && it.abc) {
        // A spot on a staff (#1227); the server counts the notes when it saves.
        if (!(Number(it.wrong) >= 1 && Number(it.wrong) % 1 === 0)) at(p.concat("wrong"), "bad", T("check_spot_note"));
        if (!/^[\^_=]*[A-Ga-g][,']*$/.test(String(it.fix || "").trim())) at(p.concat("fix"), "bad", T("check_spot_fix_note"));
      } else if (sub === "spots") {
        var hits = words(it.sentence).filter(function (w) { return norm(w) === norm(it.wrong); }).length;
        if (!norm(it.wrong) || hits !== 1) at(p.concat("wrong"), "bad", T("check_spot_once"));
        else at(p.concat("wrong"), "ok", T("check_spot_ok", { word: it.wrong }));
        if (!String(it.fix || "").trim()) at(p.concat("fix"), "bad", T("check_spot_fix"));
      } else if (sub === "scales") {
        // Build the scale (#1229): eight ABC notes; decoys are notes, none of them a note of the scale.
        var one = function (n) { var m = /^(\^{1,2}|_{1,2}|=)?([A-Ga-g])[,']*$/.exec(n); return m ? (m[1] || "") + m[2].toUpperCase() : null; };
        var ns = words(it.notes), ds = words(it.decoys), badNote = ns.concat(ds).filter(function (n) { return !one(n); })[0];
        var clash = ds.filter(function (d) { return one(d) && ns.some(function (n) { return one(n) === one(d); }); })[0];
        if (ns.length !== 8) at(p.concat("notes"), "bad", T("check_scale_notes", { n: ns.length }));
        else if (badNote && ns.indexOf(badNote) >= 0) at(p.concat("notes"), "bad", T("check_scale_note", { note: badNote }));
        else at(p.concat("notes"), "ok", T("check_scale_ok"));
        if (badNote && ns.indexOf(badNote) < 0) at(p.concat("decoys"), "bad", T("check_scale_note", { note: badNote }));
        else if (clash) at(p.concat("decoys"), "bad", T("check_scale_decoy", { note: clash }));
      } else if (sub === "builds") {
        var wn = words(it.text).length;
        if (wn < 2 || wn > 12 || !String(it.meaning || "").trim()) at(p.concat(wn < 2 || wn > 12 ? "text" : "meaning"), "bad", T("check_build", { n: wn }));
        else at(p.concat("text"), "ok", T("check_build_ok", { n: wn }));
      }
    } else if (sec === "guide") {
      var hs = headings(doc.content).map(function (x) { return x.toLowerCase(); });
      if (hasIntro(doc.content)) hs.push("introduction");
      var qids = (doc.quiz || []).map(function (q) { return q && q.question_id; });
      (Array.isArray(doc.guide) ? doc.guide : []).forEach(function (step, i) {
        if (!step || typeof step !== "object") { at(["guide", i], "bad", T("check_guide_step")); return; }
        if ("section" in step && !hs.some(function (x) { return x.indexOf(String(step.section).toLowerCase()) === 0; })) at(["guide", i], "bad", T("check_guide_section", { name: step.section }));
        if ("question" in step && qids.indexOf(step.question) < 0) at(["guide", i], "bad", T("check_guide_question", { id: step.question }));
        if ("vocab" in step && !(Number.isInteger(step.vocab) && step.vocab >= 1)) at(["guide", i], "bad", T("check_guide_vocab"));
        if ("tip" in step && !String(step.tip).trim()) at(["guide", i], "bad", T("check_guide_tip"));
        if ("concept" in step && !(step.concept && String(step.concept.title || "").trim() && String(step.concept.markdown || "").trim())) at(["guide", i], "bad", T("check_guide_concept"));
      });
    } else if (sec === "lesson") {
      if (!String(doc.title || "").trim()) at(["title"], "bad", T("check_title"));
    } else if (sec === "content") {
      if (!String(doc.content || "").trim()) at(["content"], "bad", T("check_content"));
      else at(["content"], "ok", tn("editor.js.cards", headings(doc.content).length + (hasIntro(doc.content) ? 1 : 0)));
    }
    return out;
  }
    return itemChecks;
  } };
})();
