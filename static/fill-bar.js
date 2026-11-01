// Fill the bar (#1230). A fill-bar question has a bar of music in ABC with
// one invisible rest (x2) marking a gap, and tiles of note values. A tap on a
// tile puts it in the gap (a tile may be used again) and the bar is drawn
// again with abcjs (vendor/abcjs-basic-min.js), what is left of the gap still
// invisible; Undo takes the last tile out. Once the gap is full the answer is
// the tiles, separated by spaces, which the server grades by their summed
// length (src/notation/fill_bar.rs reads lengths the same way).
//
// In Practice, the reading check and the Guide the page has a .fill-bar box
// with the tiles as buttons and a hidden radio (.fill-bar-answer) that holds
// the answer once the gap is full, so quiz.js, lesson-check.js and guide.js
// post it like a picked option. Sprint mode calls FillBar.mount itself.
(function () {
  "use strict";
  var SRC = (document.currentScript && document.currentScript.src) || "";
  if (SRC) {
    var css = document.createElement("link");
    css.rel = "stylesheet";
    css.href = SRC.replace("fill-bar.js", "fill-bar.css");
    document.head.appendChild(css);
  }

  // Lengths are fractions [n, d] of the tune's unit note length (L:).
  function gcd(a, b) { return b ? gcd(b, a % b) : Math.abs(a) || 1; }
  function fr(n, d) { var g = gcd(n, d); return [n / g, d / g]; }
  function add(a, b) { return fr(a[0] * b[1] + b[0] * a[1], a[1] * b[1]); }
  function sub(a, b) { return add(a, [-b[0], b[1]]); }
  function mul(a, b) { return fr(a[0] * b[0], a[1] * b[1]); }

  // The length written at s[i] (2, /, //, 3/2): [length, index after it].
  function suffix(s, i) {
    var m = /^(\d{0,4})((?:\/\d{0,4})*)/.exec(s.slice(i));
    var d = 1;
    (m[2].match(/\/\d*/g) || []).forEach(function (p) { d *= p.length > 1 ? Number(p.slice(1)) : 2; });
    return [fr(m[1] ? Number(m[1]) : 1, d), i + m[0].length];
  }

  var PITCH = /^[\^_=]*[A-Ga-gz][,']*/;
  // The summed length of ABC notes (a tile); null when it holds anything else.
  function length(s) {
    var i = 0, total = [0, 1], tup = null, m, r;
    function note(len) {
      if (tup) { len = mul(len, tup.f); if (--tup.left === 0) tup = null; }
      total = add(total, len);
    }
    while (i < s.length) {
      var c = s[i];
      if (" \t-><).~".indexOf(c) >= 0) { i++; continue; }
      if (c === "(") {
        var p = s[i + 1];
        if (/[234]/.test(p)) { p = Number(p); tup = { left: p, f: fr(p === 3 ? 2 : 3, p) }; i += 2; }
        else if (/\d/.test(p)) return null;
        else i++;
        continue;
      }
      if (c === "{" || c === "\"" || c === "!") {
        var end = s.indexOf(c === "{" ? "}" : c, i + 1);
        if (end < 0) return null;
        i = end + 1;
        continue;
      }
      if (c === "[") {
        // A chord lasts as long as its first note, times the length after ].
        var j = i + 1, first = null;
        while ((m = PITCH.exec(s.slice(j)))) { r = suffix(s, j + m[0].length); if (!first) first = r[0]; j = r[1]; }
        if (!first || s[j] !== "]") return null;
        r = suffix(s, j + 1);
        note(mul(first, r[0]));
        i = r[1];
        continue;
      }
      if (!(m = PITCH.exec(s.slice(i)))) return null;
      r = suffix(s, i + m[0].length);
      note(r[0]);
      i = r[1];
    }
    return total;
  }

  // The bar: the header (to K:), the music before and after the gap, the
  // gap's length and how many beats one unit is (L: times M:'s denominator).
  function parseBar(abc) {
    var lines = String(abc).split("\n"), k = -1, i;
    for (i = 0; i < lines.length && k < 0; i++) if (/^\s*K:/.test(lines[i])) k = i;
    if (k < 0) return null;
    var field = function (name) {
      for (var j = 0; j < k; j++) { var l = lines[j].trim(); if (l.indexOf(name) === 0) return l.slice(name.length).trim(); }
      return null;
    };
    var frac = function (x) { var f = /^(\d+)\s*\/\s*(\d+)$/.exec(x || ""); return f && +f[1] > 0 && +f[2] > 0 ? [+f[1], +f[2]] : null; };
    var M = field("M:"), meter = M === "C" ? [4, 4] : M === "C|" ? [2, 2] : frac(M);
    var L = frac(field("L:"));
    var unit = L ? fr(L[0], L[1]) : meter && meter[0] * 4 < meter[1] * 3 ? [1, 16] : [1, 8];
    var body = lines.slice(k + 1).join("\n"), at = -1, count = 0, gap = null, gapEnd = 0;
    for (i = 0; i < body.length; i++) {
      var c = body[i];
      var end = c === "\"" || c === "!" ? c : c === "%" ? "\n" : c === "[" && body[i + 2] === ":" ? "]" : null;
      if (end) { var e = body.indexOf(end, i + 1); i = e < 0 ? body.length : e; continue; }
      if (c === "x") { count++; at = i; var r = suffix(body, i + 1); gap = r[0]; gapEnd = r[1]; }
    }
    if (count !== 1 || gap[0] <= 0) return null;
    return { head: lines.slice(0, k + 1).join("\n"), before: body.slice(0, at), after: body.slice(gapEnd), gap: gap, beat: mul(unit, [meter ? meter[1] : 4, 1]) };
  }

  // A length in beats as a number: 2, 1.5, 0.333.
  function beats(len, beat) { var b = mul(len, beat); return Math.round(b[0] / b[1] * 1000) / 1000; }
  function abcLen(f) { return f[1] === 1 ? String(f[0]) : f[0] + "/" + f[1]; }

  function el(tag, cls) { var e = document.createElement(tag); if (cls) e.className = cls; return e; }

  // Draw `abc` into `box`; the music as text when abcjs can't.
  function draw(box, abc, opts) {
    box.innerHTML = "";
    if (window.ABCJS) {
      try {
        // A resizing drawing keeps its aspect ratio with a padding-bottom
        // percentage, which CSS takes from the parent's width: draw into an
        // inner box so that parent is the capped `box`, or a wide page gets a
        // blank gap below the bar and an answer as wide as the card (as in
        // notation.js).
        var target = opts.responsive ? box.appendChild(el("div")) : box;
        var tunes = window.ABCJS.renderAbc(target, abc, Object.assign({ add_classes: true }, opts));
        var svg = box.querySelector("svg");
        if (tunes && tunes.length && svg) {
          svg.setAttribute("role", "img");
          var title = svg.querySelector("title");
          if (title) title.textContent = t("common.js.notation_label");
          return;
        }
      } catch (e) { box.innerHTML = ""; }
    }
    box.textContent = abc.split("\n").slice(-1)[0];
  }

  // Build the question in `root`: the bar, what is left to fill, the tiles
  // and Undo. onChange(answer) after every tap: the tiles once the gap is
  // full, else null. Returns { lock(), reveal(container) }.
  function mount(root, abc, tiles, onChange) {
    var bar = parseBar(abc);
    var lens = tiles.map(length);
    var placed = [], locked = false, tilesDrawn = false;
    root.innerHTML = "";
    if (!root.dataset.testid) root.dataset.testid = "fill-bar";
    var staff = root.appendChild(el("div", "fill-bar-staff"));
    staff.dataset.testid = "fill-bar-staff";
    var left = root.appendChild(el("p", "fill-bar-left"));
    left.dataset.testid = "fill-bar-left";
    left.setAttribute("aria-live", "polite");
    var row = root.appendChild(el("div", "fill-bar-tiles"));
    var buttons = tiles.map(function (tile, i) {
      var b = row.appendChild(el("button", "fill-bar-tile"));
      b.type = "button";
      b.dataset.tile = tile;
      b.dataset.testid = "fill-bar-tile";
      if (bar && lens[i]) {
        b.dataset.beats = beats(lens[i], bar.beat);
        b.setAttribute("aria-label", tn("common.js.fill_bar_beats", beats(lens[i], bar.beat)));
      }
      b.textContent = tile;
      b.addEventListener("click", function () {
        if (locked || full()) return;
        placed.push(i);
        update();
      });
      return b;
    });
    var undo = root.appendChild(el("button", "btn ghost small fill-bar-undo"));
    undo.type = "button";
    undo.dataset.testid = "fill-bar-undo";
    undo.textContent = t("common.js.fill_bar_undo");
    undo.addEventListener("click", function () { if (locked) return; placed.pop(); update(); });

    function sum(list) { return list.reduce(function (a, i) { return lens[i] ? add(a, lens[i]) : a; }, [0, 1]); }
    function full() { return bar && sub(bar.gap, sum(placed))[0] <= 0; }
    // The bar with `notes` (of length `len`) in the gap and the rest of it invisible.
    function filled(notes, len) {
      var rest = sub(bar.gap, len);
      return bar.head + "\n" + bar.before + notes + (rest[0] > 0 ? " x" + abcLen(rest) + " " : " ") + bar.after;
    }
    // abcjs measures text as it draws, so it draws only while the box shows.
    function paint() {
      if (!bar || !root.getClientRects().length) return;
      draw(staff, filled(placed.map(function (i) { return tiles[i]; }).join(" "), sum(placed)), { responsive: "resize", staffwidth: 260 });
      if (tilesDrawn) return;
      tilesDrawn = true;
      // A tile alone: no clef, no time signature.
      var head = bar.head.split("\n").filter(function (l) { return !/^\s*[MT]:/.test(l); }).join("\n") + " clef=none";
      buttons.forEach(function (b) {
        var box = el("span", "fill-bar-tile-staff");
        b.textContent = "";
        b.appendChild(box);
        draw(box, head + "\n" + b.dataset.tile, { staffwidth: 70, scale: 0.75, paddingleft: 0, paddingright: 0, paddingtop: 0, paddingbottom: 0 });
      });
    }
    function update() {
      if (!bar) { staff.textContent = abc; return; }
      var rest = sub(bar.gap, sum(placed));
      paint();
      left.textContent = rest[0] > 0 ? tn("common.js.fill_bar_left", beats(rest, bar.beat)) : t("common.js.fill_bar_full");
      root.dataset.full = rest[0] > 0 ? "false" : "true";
      root.dataset.left = beats(rest, bar.beat);
      buttons.forEach(function (b) { b.disabled = locked || rest[0] <= 0; });
      undo.disabled = locked || !placed.length;
      onChange(rest[0] > 0 ? null : placed.map(function (i) { return tiles[i]; }).join(" "));
    }
    if (window.IntersectionObserver) {
      var seen = new IntersectionObserver(function (es) { es.forEach(function (e) { if (e.isIntersecting || root.getClientRects().length) paint(); }); });
      seen.observe(root);
    }
    update();

    // Is `text` an answer of this question (its tiles, separated by spaces)?
    function answerOf(text) {
      var parts = String(text).trim().split(/\s+/);
      var idx = parts.map(function (p) { return tiles.indexOf(p); });
      return parts[0] && idx.indexOf(-1) < 0 ? idx : null;
    }
    return {
      lock: function () { locked = true; buttons.forEach(function (b) { b.disabled = true; }); undo.disabled = true; },
      // The right answer, shown as text in a graded answer's <strong>, drawn as the bar it fills.
      reveal: function (container) {
        if (!bar) return;
        container.querySelectorAll("strong").forEach(function (s) {
          var idx = answerOf(s.textContent);
          if (!idx || s.dataset.drawn) return;
          s.dataset.drawn = "1";
          s.textContent = "";
          var box = s.appendChild(el("span", "fill-bar-answer"));
          box.dataset.testid = "fill-bar-answer";
          draw(box, filled(idx.map(function (i) { return tiles[i]; }).join(" "), sum(idx)), { responsive: "resize", staffwidth: 220 });
        });
      }
    };
  }

  window.FillBar = { mount: mount };

  // Practice, the reading check and the Guide.
  function auto() {
    document.querySelectorAll(".fill-bar[data-abc]").forEach(function (root) {
      var q = root.closest(".question");
      var radio = q && q.querySelector("input.fill-bar-answer");
      var fb = q && q.querySelector(".feedback");
      var tiles = Array.prototype.map.call(root.querySelectorAll(".fill-bar-tile"), function (b) { return b.dataset.tile; });
      var w = mount(root, root.dataset.abc, tiles, function (answer) {
        if (!radio) return;
        radio.value = answer || "";
        radio.checked = !!answer;
        radio.dispatchEvent(new Event("change", { bubbles: true }));
      });
      if (fb) new MutationObserver(function () {
        if (fb.dataset.state !== "right" && fb.dataset.state !== "wrong") return;
        w.lock();
        w.reveal(fb);
      }).observe(fb, { attributes: true, childList: true });
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", auto);
  else auto();
})();
