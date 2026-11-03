// Problems in a course file, worded as the server words them (moved out of
// courses.js, which uses them for the lesson table and the file editor).
(function () {
  "use strict";
  // ---------- problems, worded as the server words them ----------
  // One sentence per problem: its kind, where (a lesson's name, the course
  // settings or a path; the field; the line), what is wrong and how to fix
  // it. The server sends its problems ready-made (src/problems.rs, `text`);
  // a parse error the page finds itself is written here the same way, so
  // the drawer and the line under the title say the same thing.
  var KIND = { json: t('courses.js.kind_json'), rules: t('courses.js.kind_rules'), refused: t('courses.js.kind_refused') };
  var JSON_HINT = t('courses.js.json_hint');
  function problemText(p) {
    var at = [];
    if (p.place) at.push(p.place);
    if (p.field) at.push(t('courses.js.problem_field', { field: p.field }));
    if (p.line) at.push(p.kind === "json" && p.column ? t('courses.js.problem_line_col', { line: p.line, column: p.column }) : t('courses.js.problem_line', { line: p.line }));
    return t('courses.js.problem_text', {
      kind: KIND[p.kind],
      where: at.length ? t('courses.js.problem_in', { where: at.join(t('courses.js.problem_sep')) }) : "",
      message: p.message,
      fix: p.hint ? t('courses.js.problem_fix', { hint: p.hint }) : ""
    });
  }
  // A JSON text that does not parse: { line, column, message } in plain
  // words (those of src/problems.rs `plain_syntax`); null when it parses.
  function jsonError(text) {
    try { JSON.parse(text); return null; } catch (e) {
      if (!text.trim()) return { line: 0, column: 0, message: t('courses.js.json_empty') };
      var f = locate(text);
      if (!f) return { line: 0, column: 0, message: e.message.charAt(0).toUpperCase() + e.message.slice(1).replace(/\.?$/, ".") };
      var before = text.slice(0, f.at);
      var nl = before.lastIndexOf("\n");
      return { line: before.split("\n").length, column: f.at - nl, message: f.message + "." };
    }
  }
  // Where a JSON text first goes wrong, and why: { at, message }, or null.
  function locate(s) {
    var i = 0, n = s.length;
    var END = t('courses.js.json_end');
    function stop(msg) { throw { at: Math.min(i, n), message: msg }; }
    function ws() { while (i < n && " \t\n\r".indexOf(s[i]) >= 0) i++; }
    function str() {
      i++;
      for (;;) {
        if (i >= n) stop(END);
        var c = s[i];
        if (c === '"') { i++; return; }
        if (c === "\\") {
          var e = s[i + 1];
          if (e === "u" && /^[0-9a-fA-F]{4}$/.test(s.slice(i + 2, i + 6))) { i += 6; continue; }
          if (e && "\"\\/bfnrt".indexOf(e) >= 0) { i += 2; continue; }
          stop(t('courses.js.json_escape'));
        }
        if (c < " ") stop(t('courses.js.json_control'));
        i++;
      }
    }
    function value() {
      ws();
      if (i >= n) stop(END);
      var c = s[i];
      if (c === "{") return obj();
      if (c === "[") return arr();
      if (c === '"') return str();
      if (c === "-" || (c >= "0" && c <= "9")) {
        var m = /^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?/.exec(s.slice(i));
        if (!m || m[0] === "-" || /^[0-9.eE+-]/.test(s.charAt(i + m[0].length))) stop(t('courses.js.json_number'));
        i += m[0].length;
        return;
      }
      var w = /^(true|false|null)/.exec(s.slice(i, i + 5));
      if (w) { i += w[1].length; return; }
      stop(c === "'" ? t('courses.js.json_single_quotes') : t('courses.js.json_value'));
    }
    function obj() {
      i++;
      ws();
      if (s[i] === "}") { i++; return; }
      for (;;) {
        ws();
        if (i >= n) stop(END);
        if (s[i] !== '"') stop(s[i] === "'" ? t('courses.js.json_single_quotes') : t('courses.js.json_key'));
        str();
        ws();
        if (i >= n) stop(END);
        if (s[i] !== ":") stop(t('courses.js.json_colon'));
        i++;
        value();
        ws();
        if (i >= n) stop(END);
        if (s[i] === "}") { i++; return; }
        if (s[i] !== ",") stop(t('courses.js.json_comma_brace'));
        i++;
        ws();
        if (s[i] === "}") stop(t('courses.js.json_trailing_comma'));
      }
    }
    function arr() {
      i++;
      ws();
      if (s[i] === "]") { i++; return; }
      for (;;) {
        value();
        ws();
        if (i >= n) stop(END);
        if (s[i] === "]") { i++; return; }
        if (s[i] !== ",") stop(t('courses.js.json_comma_bracket'));
        i++;
        ws();
        if (s[i] === "]") stop(t('courses.js.json_trailing_comma'));
      }
    }
    try {
      value();
      ws();
      if (i < n) stop(t('courses.js.json_more'));
    } catch (e) {
      if (e && typeof e.at === "number") return e;
      throw e;
    }
    return null;
  }

  window.JsonProblems = { jsonError: jsonError, problemText: problemText, JSON_HINT: JSON_HINT };
})();
