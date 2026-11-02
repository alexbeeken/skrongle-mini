// Forms mode: one course file (course.json or a lesson) as a form built
// from docs/course.schema.json, so a field added to the schema shows up
// here without code. The form edits the parsed document in place: fields
// it does not know stay as they were, and only what is typed changes.
// Hand-written on top of the schema: the correct-answer selector of a
// multiple-choice question, true/false as a select, ids (never shown: a
// new item's id is made up, the rest stay as they are), and fields whose description starts "<type>:" or "<type> only:" (e.g.
// "numeric only: …") shown only for that type of the object's enum.
//
//   SchemaForm.load(url) -> Promise<schema>
//   SchemaForm.render(el, schema, "lesson" | "manifest", doc, onChange)
(function () {
  "use strict";
  var loading = null;
  function load(url) {
    if (!loading) {
      loading = fetch(url, { credentials: "same-origin" }).then(function (r) {
        if (!r.ok) throw new Error(t('courses.js.form_load_failed'));
        return r.json();
      });
      loading.catch(function () { loading = null; });
    }
    return loading;
  }

  // ---------- schema ----------
  function resolve(root, s) {
    while (s && s.$ref) {
      var t = root;
      s.$ref.replace(/^#\//, "").split("/").forEach(function (p) { t = t && t[p]; });
      var own = Object.assign({}, s);
      delete own.$ref;
      s = Object.assign({}, t, own);
    }
    return s || {};
  }
  function kind(s) {
    if (s.oneOf) return "oneOf";
    if (s.enum) return "enum";
    var t = Array.isArray(s.type) ? s.type.filter(function (x) { return x !== "null"; })[0] : s.type;
    if (!t && s.properties) t = "object";
    if (t === "object" && !s.properties && s.additionalProperties) return "map";
    return t || "string";
  }
  var nullable = function (s) { return Array.isArray(s.type) && s.type.indexOf("null") >= 0; };
  // Ids, slugs and references to them are left to the JSON: the lesson's
  // own id, slug and unit (the lesson table moves it), a unit's, a
  // question's. A lesson's order too: the table's rows set it.
  function isId(key, s) { return /(^|_)id$/.test(key) || key === "slug" || /^A [a-z]+ id\b/.test(s.description || ""); }
  // blank(ctx, s, key, list): list is the path of the list the new value
  // goes into, which names a made-up id (units -> unit-2).
  function blank(ctx, s, key, list) {
    s = resolve(ctx.root, s);
    var k = kind(s);
    if (s.default !== undefined) return s.default;
    if (k === "oneOf") return blank(ctx, s.oneOf[0], key, list);
    if (k === "enum") return s.enum[0];
    if (k === "object") {
      var o = {};
      (s.required || []).forEach(function (p) { o[p] = blank(ctx, s.properties[p] || {}, p, list); });
      return o;
    }
    if (k === "map") return {};
    if (k === "array") return [];
    if (k === "integer" || k === "number") return s.minimum || 0;
    if (k === "boolean") return false;
    if (k === "string" && key && isId(key, s)) return made(ctx, key, list);
    return "";
  }
  // Question and scene ids are unique across the course, so they start with
  // the lesson id ("<lesson>-q1"); units and groups are only in course.json.
  function made(ctx, key, list) {
    var lesson = typeof ctx.doc.id === "string" && ctx.doc.id && ctx.doc.slug !== undefined ? ctx.doc.id + "-" : "";
    var name = key === "question_id" ? "q" : String((list || []).filter(function (x) { return typeof x === "string"; }).pop() || "item").replace(/s$/, "");
    var n = 1, taken = JSON.stringify(ctx.doc);
    while (taken.indexOf('"' + lesson + name + n + '"') >= 0 || taken.indexOf('"' + lesson + name + "-" + n + '"') >= 0) n++;
    return lesson + name + (key === "question_id" ? "" : "-") + n;
  }

  // ---------- the document ----------
  function get(path, doc) { var v = doc; for (var i = 0; i < path.length && v != null; i++) v = v[path[i]]; return v; }
  function put(ctx, path, value) {
    var v = ctx.doc;
    for (var i = 0; i < path.length - 1; i++) {
      if (v[path[i]] == null || typeof v[path[i]] !== "object") v[path[i]] = typeof path[i + 1] === "number" ? [] : {};
      v = v[path[i]];
    }
    v[path[path.length - 1]] = value;
  }
  function drop(ctx, path) {
    var parent = get(path.slice(0, -1), ctx.doc);
    if (parent && typeof parent === "object" && !Array.isArray(parent)) delete parent[path[path.length - 1]];
  }

  // ---------- markup ----------
  function h(tag, attrs, kids) {
    var el = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (a) {
      if (a === "text") el.textContent = attrs[a];
      else if (attrs[a] !== null && attrs[a] !== false && attrs[a] !== undefined) el.setAttribute(a, attrs[a] === true ? "" : attrs[a]);
    });
    (kids || []).forEach(function (k) { if (k) el.appendChild(typeof k === "string" ? document.createTextNode(k) : k); });
    return el;
  }
  var dotted = function (path) { return path.join("."); };
  function human(key) { var s = String(key).replace(/_/g, " "); return s.charAt(0).toUpperCase() + s.slice(1); }
  function btn(test, path, text, label, cls) {
    return h("button", { type: "button", class: "sf-btn " + (cls || ""), "data-testid": test, "data-path": dotted(path), "aria-label": label, title: label, text: text });
  }
  function hint(s) { return s.description ? h("small", { class: "sf-hint muted", text: s.description }) : null; }
  // A short line to tell list items apart: the first two required strings
  // that are not ids, or any string.
  function summary(s, v) {
    if (!v || typeof v !== "object") return String(v == null ? "" : v);
    var props = Object.keys(s.properties || {});
    var req = (s.required || []).filter(function (p) { return !/id$/.test(p) && typeof v[p] === "string" && v[p]; });
    var picked = req.slice(0, 2).map(function (p) { return v[p]; });
    if (!picked.length) {
      var any = props.concat(Object.keys(v)).filter(function (p) { return typeof v[p] === "string" && v[p]; })[0];
      if (any) picked.push(v[any]);
    }
    var t = picked.join(" — ");
    return t.length > 70 ? t.slice(0, 69) + "…" : t;
  }
  // Fields that apply to some values of a sibling enum, from their description.
  function condition(root, s, key, req) {
    if (req) return null;
    var d = resolve(root, (s.properties || {})[key] || {}).description || "";
    var m = /^([a-z-]+(?:, [a-z-]+)*)(?: only)?:/.exec(d);
    if (!m) return null;
    var vals = m[1].split(", ");
    var on = Object.keys(s.properties).filter(function (p) {
      var e = s.properties[p].enum;
      return e && vals.every(function (x) { return e.indexOf(x) >= 0; });
    })[0];
    return on ? { key: on, values: vals } : null;
  }

  // ---------- fields ----------
  function field(ctx, s, path, key, required, label) {
    s = resolve(ctx.root, s);
    var k = kind(s);
    if (k === "object" || k === "array" || k === "map" || k === "oneOf") return section(ctx, s, path, key, k, label);
    var value = get(path, ctx.doc);
    var wrap = h("label", { class: "sf-field" }, [label === false ? null : h("span", { class: "sf-label", text: human(key) })]);
    wrap.appendChild(scalar(ctx, s, path, key, required, value, k));
    var hn = hint(s);
    if (hn && label !== false) wrap.appendChild(hn);
    return wrap;
  }
  function scalar(ctx, s, path, key, required, value, k) {
    var el;
    var attrs = { "data-testid": "course-form-field", "data-path": dotted(path), required: required || null };
    // A true-false question's answer is one of two words.
    var parentV = get(path.slice(0, -1), ctx.doc);
    var options = k === "enum" ? s.enum : (key === "correct_answer" && parentV && parentV.type === "true-false" ? ["true", "false"] : null);
    if (options) {
      el = h("select", attrs, [required && value !== undefined ? null : h("option", { value: "", text: "—" })].concat(options.map(function (o) {
        return h("option", { value: String(o), text: String(o) });
      })));
      if (value !== undefined && options.indexOf(value) < 0) el.appendChild(h("option", { value: String(value), text: String(value) }));
      el.value = value === undefined || value === null ? "" : String(value);
      el.addEventListener("change", function () {
        var raw = el.value;
        var pick = options.filter(function (o) { return String(o) === raw; })[0];
        if (raw === "" && !required) drop(ctx, path); else put(ctx, path, pick !== undefined ? pick : raw);
        ctx.changed(true);
      });
      return el;
    }
    if (k === "boolean") {
      el = h("input", Object.assign(attrs, { type: "checkbox" }));
      el.checked = value === true;
      el.addEventListener("change", function () {
        if (!el.checked && !required && value === undefined) drop(ctx, path); else put(ctx, path, el.checked);
        ctx.changed();
      });
      return el;
    }
    if (k === "integer" || k === "number") {
      el = h("input", Object.assign(attrs, { type: "number", inputmode: k === "integer" ? "numeric" : "decimal", step: k === "integer" ? "1" : "any", min: s.minimum, max: s.maximum }));
      el.value = typeof value === "number" ? String(value) : "";
      el.addEventListener("input", function () {
        if (el.value === "") { if (!required) drop(ctx, path); ctx.changed(); return; }
        var n = Number(el.value);
        if (isFinite(n)) { put(ctx, path, k === "integer" ? Math.round(n) : n); ctx.changed(); }
      });
      return el;
    }
    var long = /markdown/i.test(s.description || "") || (typeof value === "string" && value.length > 80);
    var text = value === undefined || value === null ? "" : String(value);
    el = long ? h("textarea", Object.assign(attrs, { rows: Math.min(16, Math.max(3, text.split("\n").length + 1)), spellcheck: "true" }))
      : h("input", Object.assign(attrs, { type: "text", autocapitalize: "off" }));
    el.value = text;
    el.addEventListener("input", function () {
      var inList = typeof path[path.length - 1] === "number";
      if (el.value === "" && !required && !inList) { if (value === null || nullable(s) && value !== undefined) put(ctx, path, null); else drop(ctx, path); }
      else put(ctx, path, el.value);
      ctx.typed(path, el.value);
      ctx.changed();
    });
    return el;
  }

  // Lists, objects, maps and choices fold into a section; the open ones
  // stay open when the form is drawn again.
  function section(ctx, s, path, key, k, label) {
    var value = get(path, ctx.doc);
    var p = dotted(path);
    var count = Array.isArray(value) ? " (" + value.length + ")" : (k === "map" && value ? " (" + Object.keys(value).length + ")" : "");
    var d = h("details", { class: "sf-section", "data-testid": "course-form-section", "data-path": p, open: opened(ctx, path) || null }, [
      label === false ? null : h("summary", { text: human(key) + count })
    ]);
    d.addEventListener("toggle", function () { ctx.open[p] = d.open; });
    var hn = hint(s);
    if (hn) d.appendChild(hn);
    if (k === "array") d.appendChild(list(ctx, s, path));
    else if (k === "map") d.appendChild(map(ctx, s, path));
    else if (k === "oneOf") d.appendChild(choice(ctx, s, path));
    else d.appendChild(object(ctx, s, path));
    return d;
  }
  // The file's own lists start folded; inside a list item they start open.
  function opened(ctx, path) {
    var o = ctx.open[dotted(path)];
    return o === undefined ? !path.length || path.some(function (x) { return typeof x === "number"; }) : o;
  }
  function object(ctx, s, path) {
    var value = get(path, ctx.doc) || {};
    var box = h("div", { class: "sf-object" });
    var req = s.required || [];
    var props = Object.keys(s.properties || {});
    props.forEach(function (key) {
      if (isId(key, resolve(ctx.root, s.properties[key])) || (!path.length && key === "order")) return;
      var c = condition(ctx.root, s, key, req.indexOf(key) >= 0);
      if (c && value[key] === undefined && c.values.indexOf(value[c.key]) < 0) return;
      box.appendChild(field(ctx, s.properties[key], path.concat(key), key, req.indexOf(key) >= 0));
    });
    var other = Object.keys(value).filter(function (key) { return props.indexOf(key) < 0; });
    if (other.length && s.properties) box.appendChild(h("p", { class: "small muted sf-kept", "data-testid": "course-form-kept", "data-path": dotted(path), text: t('courses.js.form_kept', { keys: other.join(", ") }) }));
    return box;
  }
  function list(ctx, s, path) {
    var value = get(path, ctx.doc);
    var items = Array.isArray(value) ? value : [];
    var is = resolve(ctx.root, s.items || {});
    var ik = kind(is);
    var flat = ik !== "object" && ik !== "array" && ik !== "map" && ik !== "oneOf";
    var parent = get(path.slice(0, -1), ctx.doc);
    // A multiple-choice question: pick the right option.
    var correct = path[path.length - 1] === "options" && parent && parent.type === "multiple-choice";
    var ul = h("ol", { class: "sf-list" + (flat ? " flat" : ""), "data-testid": "course-form-list", "data-path": dotted(path) });
    items.forEach(function (item, i) {
      var ip = path.concat(i);
      var tools = h("span", { class: "sf-tools" }, [
        btn("course-form-up", ip, "↑", t('courses.js.form_move_up'), null),
        btn("course-form-down", ip, "↓", t('courses.js.form_move_down'), null),
        btn("course-form-remove", ip, "✕", t('courses.js.form_remove'), "sf-remove")
      ]);
      tools.children[0].disabled = i === 0;
      tools.children[1].disabled = i === items.length - 1;
      var li;
      if (flat) {
        li = h("li", { class: "sf-item", "data-testid": "course-form-item", "data-path": dotted(ip) }, [
          correct ? h("input", { type: "radio", name: "sf-correct-" + ctx.id + "-" + dotted(path), class: "sf-correct", "data-testid": "course-form-correct", "data-path": dotted(ip), "aria-label": t('courses.js.form_right_answer'), title: t('courses.js.form_right_answer'), checked: parent.correct_answer === item && item !== "" ? true : null }) : null,
          scalar(ctx, is, ip, i, true, item, ik), tools
        ]);
      } else {
        var open = ctx.openItems.has(item);
        var d = h("details", { open: open || null }, [h("summary", { class: "sf-item-head" }, [h("span", { class: "sf-item-title", text: (i + 1) + ". " + summary(is, item) }), tools])]);
        d.addEventListener("toggle", function () { if (d.open) ctx.openItems.add(item); else ctx.openItems.delete(item); });
        if (ik === "oneOf") d.appendChild(choice(ctx, is, ip));
        else if (ik === "object") d.appendChild(object(ctx, is, ip));
        else d.appendChild(field(ctx, is, ip, i, true, false));
        li = h("li", { class: "sf-item", "data-testid": "course-form-item", "data-path": dotted(ip) }, [d]);
      }
      ul.appendChild(li);
    });
    var add = btn("course-form-add", path, t('courses.js.form_add'), t('courses.js.form_add_to', { list: human(path[path.length - 1]).toLowerCase() }), "btn small ghost");
    if (s.maxItems && items.length >= s.maxItems) add.disabled = true;
    return h("div", {}, [ul, add]);
  }
  function map(ctx, s, path) {
    var value = get(path, ctx.doc) || {};
    var vs = resolve(ctx.root, s.additionalProperties);
    var box = h("div", { class: "sf-map" });
    Object.keys(value).forEach(function (key) {
      var kp = path.concat(key);
      var f = field(ctx, vs, kp, key, true);
      var rm = btn("course-form-remove", kp, "✕", t('courses.js.form_remove_key', { key: key }), "sf-remove");
      box.appendChild(h("div", { class: "sf-entry", "data-testid": "course-form-item", "data-path": dotted(kp) }, [f, rm]));
    });
    var name = h("input", { type: "text", class: "sf-key", placeholder: t('courses.js.form_key'), "aria-label": t('courses.js.form_key_to_add'), autocapitalize: "off", "data-testid": "course-form-map-key", "data-path": dotted(path) });
    box.appendChild(h("div", { class: "sf-map-add" }, [name, btn("course-form-add", path, t('courses.js.form_add'), t('courses.js.form_add_key'), "btn small ghost")]));
    return box;
  }
  // One of several shapes (a Guide step): which one, then its fields.
  function choice(ctx, s, path) {
    var value = get(path, ctx.doc);
    var variants = s.oneOf.map(function (v) { return resolve(ctx.root, v); });
    var at = variants.findIndex(function (v) { return value && typeof value === "object" && (v.required || []).every(function (r) { return r in value; }); });
    var sel = h("select", { "data-testid": "course-form-kind", "data-path": dotted(path), "aria-label": t('courses.js.form_kind') }, variants.map(function (v, i) {
      return h("option", { value: String(i), text: human((v.required || [])[0] || t('courses.js.form_option', { n: i + 1 })) });
    }));
    if (at < 0) sel.insertBefore(h("option", { value: "", text: "—" }), sel.firstChild);
    sel.value = at < 0 ? "" : String(at);
    sel.addEventListener("change", function () {
      if (sel.value === "") return;
      put(ctx, path, blank(ctx, variants[+sel.value], null, path.filter(function (x) { return typeof x === "string"; })));
      ctx.changed(true);
    });
    return h("div", { class: "sf-choice" }, [sel, at < 0 ? null : object(ctx, variants[at], path)]);
  }

  // ---------- the whole form ----------
  var forms = 0;
  // Drawing again (another document in the same place) keeps the listeners.
  function render(el, schema, def, doc, onChange) {
    var ctx = { root: schema, doc: doc, open: {}, openItems: new WeakSet(), id: ++forms };
    var top = schema.$defs[def];
    var first = !el.sfCtx;
    el.sfCtx = ctx;
    ctx.top = top;
    function draw() {
      var focus = document.activeElement && el.contains(document.activeElement) ? document.activeElement : null;
      var fp = focus && focus.dataset.path, ft = focus && focus.dataset.testid;
      el.textContent = "";
      el.appendChild(object(ctx, top, []));
      if (fp !== null && fp !== undefined) {
        var again = el.querySelector('[data-testid="' + ft + '"][data-path="' + fp.replace(/"/g, '\\"') + '"]');
        if (again) again.focus();
      }
    }
    ctx.changed = function (redraw) { onChange(doc); if (redraw) draw(); };
    // An option typed while it is the right answer keeps being the answer.
    ctx.typed = function (path, text) {
      var parent = get(path.slice(0, -2), doc);
      if (path[path.length - 2] !== "options" || !parent || parent.type !== "multiple-choice") return;
      var radio = el.querySelector('.sf-correct[data-path="' + dotted(path) + '"]');
      if (radio && radio.checked) {
        parent.correct_answer = text;
        var ca = el.querySelector('[data-testid="course-form-field"][data-path="' + dotted(path.slice(0, -2).concat("correct_answer")) + '"]');
        if (ca) ca.value = text;
      }
    };
    if (first) listen(el);
    draw();
  }
  function listen(el) {
    el.addEventListener("click", function (ev) {
      var ctx = el.sfCtx, doc = ctx.doc, schema = ctx.root, top = ctx.top;
      var b = ev.target.closest(".sf-btn");
      if (!b || !el.contains(b)) return;
      ev.preventDefault();
      var path = b.dataset.path === "" ? [] : b.dataset.path.split(".").map(function (x) { return /^\d+$/.test(x) ? +x : x; });
      var t = b.dataset.testid;
      if (t === "course-form-add") {
        var s = top;
        path.forEach(function (p) { s = resolve(schema, s); s = typeof p === "number" ? s.items : (s.properties && s.properties[p]) || s.additionalProperties; });
        s = resolve(schema, s);
        if (kind(s) === "map") {
          var keyIn = b.parentNode.querySelector(".sf-key");
          var key = keyIn.value.trim();
          if (!key) { keyIn.focus(); return; }
          var m = get(path, doc);
          if (!m || typeof m !== "object") { put(ctx, path, {}); m = get(path, doc); }
          if (!(key in m)) m[key] = blank(ctx, s.additionalProperties, null, path);
          ctx.open[dotted(path.concat(key))] = true;
        } else {
          var arr = get(path, doc);
          if (!Array.isArray(arr)) { put(ctx, path, []); arr = get(path, doc); }
          var item = blank(ctx, s.items || {}, null, path);
          arr.push(item);
          if (item && typeof item === "object") ctx.openItems.add(item);
          ctx.open[dotted(path)] = true;
        }
        ctx.changed(true);
        var fresh = el.querySelectorAll('[data-testid="course-form-list"][data-path="' + dotted(path) + '"] > li');
        var last = fresh[fresh.length - 1];
        var first = last && last.querySelector("input:not([type=radio]), textarea, select");
        if (first) first.focus();
        return;
      }
      var at = path[path.length - 1], parentPath = path.slice(0, -1), parent = get(parentPath, doc);
      if (t === "course-form-remove") {
        if (Array.isArray(parent)) parent.splice(at, 1); else drop(ctx, path);
      } else if (Array.isArray(parent)) {
        var to = t === "course-form-up" ? at - 1 : at + 1;
        if (to < 0 || to >= parent.length) return;
        parent.splice(to, 0, parent.splice(at, 1)[0]);
      }
      ctx.changed(true);
      var same = el.querySelector('[data-testid="' + t + '"][data-path="' + dotted(parentPath.concat(t === "course-form-remove" ? at : (t === "course-form-up" ? at - 1 : at + 1))) + '"]');
      if (same && !same.disabled) same.focus();
    });
    el.addEventListener("change", function (ev) {
      var r = ev.target;
      if (!r.classList.contains("sf-correct")) return;
      var path = r.dataset.path.split(".").map(function (x) { return /^\d+$/.test(x) ? +x : x; });
      var ctx = el.sfCtx;
      var q = get(path.slice(0, -2), ctx.doc);
      q.correct_answer = get(path, ctx.doc);
      ctx.changed(true);
    });
  }

  window.SchemaForm = { load: load, render: render };
})();
