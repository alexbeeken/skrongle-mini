// Algebra tiles (#1355): a pile of x², x and 1 tiles for an `algebra-tiles`
// question, to lay out as a rectangle before typing its two sides. The
// board is a scratch area: the typed answer is what the server grades, and
// the page never knows the right layout.
//
// Drag a tile with the mouse or a finger; it snaps to the grid when dropped.
// With the keyboard: Tab to a tile, the arrow keys move it one step, R (or
// the Turn button) turns a strip, Enter checks the answer.
(function () {
  "use strict";
  var NS = "http://www.w3.org/2000/svg";
  var STEP = 10;            // SVG units per grid step
  var ONE = 2, X = 7;       // a 1 tile is 2 steps wide, an x tile 7 (not a multiple of 1 on purpose)
  var boards = 0;

  function el(name, attrs) {
    var e = document.createElementNS(NS, name);
    Object.keys(attrs || {}).forEach(function (k) { e.setAttribute(k, attrs[k]); });
    return e;
  }

  function size(t) {
    if (t.kind === "square") return [X, X];
    if (t.kind === "strip") return t.turned ? [X, ONE] : [ONE, X];
    return [ONE, ONE];
  }

  function setup(box) {
    var v = box.dataset.var || "x";
    var n = { square: +box.dataset.squares || 0, strip: +box.dataset.strips || 0, unit: +box.dataset.units || 0 };
    var label = { square: v + "²", strip: v, unit: "1" };
    var W = Math.max(24, X * n.square + ONE * n.strip + 2);
    var tiles = [];
    ["square", "strip", "unit"].forEach(function (kind) {
      for (var i = 0; i < n[kind]; i++) tiles.push({ kind: kind, turned: false });
    });

    // The pile: tiles in rows along the top, upright strips side by side.
    var home = [], cx = 0, cy = 0, rowH = 0;
    tiles.forEach(function (t) {
      var s = size(t);
      if (cx > 0 && cx + s[0] > W) { cx = 0; cy += rowH + 1; rowH = 0; }
      home.push([cx, cy]);
      cx += s[0] + 1;
      rowH = Math.max(rowH, s[1]);
    });
    var pileH = cy + rowH + 1;
    var H = pileH + Math.max(12, X * n.square + ONE * n.strip);

    var board = box.querySelector(".atiles-board");
    var keys = board.querySelector(".atiles-keys");
    keys.id = "atiles-keys-" + (++boards);
    var svg = el("svg", { viewBox: "0 0 " + W * STEP + " " + H * STEP, class: "atiles-svg" });
    svg.style.maxWidth = W * 16 + "px";
    var defs = el("defs");
    var pat = el("pattern", { id: "atiles-grid-" + boards, width: STEP, height: STEP, patternUnits: "userSpaceOnUse" });
    pat.appendChild(el("path", { d: "M " + STEP + " 0 L 0 0 0 " + STEP, class: "atiles-gridline" }));
    defs.appendChild(pat);
    svg.appendChild(defs);
    svg.appendChild(el("rect", { x: 0, y: 0, width: W * STEP, height: H * STEP, fill: "url(#atiles-grid-" + boards + ")" }));
    svg.appendChild(el("line", { x1: 0, y1: pileH * STEP - STEP / 2, x2: W * STEP, y2: pileH * STEP - STEP / 2, class: "atiles-divider" }));
    board.appendChild(svg);

    var turnBtn = box.querySelector("[data-turn]");
    var selected = null;

    function place(t) {
      var s = size(t);
      t.x = Math.max(0, Math.min(W - s[0], t.x));
      t.y = Math.max(0, Math.min(H - s[1], t.y));
      t.g.setAttribute("transform", "translate(" + t.x * STEP + " " + t.y * STEP + ")");
      t.rect.setAttribute("width", s[0] * STEP);
      t.rect.setAttribute("height", s[1] * STEP);
      t.text.setAttribute("x", s[0] * STEP / 2);
      t.text.setAttribute("y", s[1] * STEP / 2);
      t.g.dataset.x = t.x;
      t.g.dataset.y = t.y;
      t.g.dataset.turned = t.turned ? "1" : "0";
    }

    function select(t) {
      selected = t;
      tiles.forEach(function (o) { o.g.classList.toggle("selected", o === t); });
      turnBtn.disabled = !(t && t.kind === "strip");
    }

    function turn(t) {
      if (!t || t.kind !== "strip") return;
      t.turned = !t.turned;
      place(t);
    }

    function reset() {
      tiles.forEach(function (t, i) { t.turned = false; t.x = home[i][0]; t.y = home[i][1]; place(t); });
      select(null);
    }

    // A pointer position in grid steps.
    function at(ev) {
      var m = svg.getScreenCTM();
      if (!m) return [0, 0];
      var p = svg.createSVGPoint();
      p.x = ev.clientX; p.y = ev.clientY;
      p = p.matrixTransform(m.inverse());
      return [p.x / STEP, p.y / STEP];
    }

    tiles.forEach(function (t) {
      t.g = el("g", { class: "atiles-tile atiles-" + t.kind, tabindex: "0", role: "button", "aria-label": label[t.kind], "aria-describedby": keys.id });
      t.g.dataset.testid = "tile";
      t.g.dataset.kind = t.kind;
      t.rect = el("rect", { rx: 3 });
      t.text = el("text", { "text-anchor": "middle", "dominant-baseline": "central" });
      t.text.textContent = label[t.kind];
      t.g.appendChild(t.rect);
      t.g.appendChild(t.text);
      svg.appendChild(t.g);

      var drag = null;
      t.g.addEventListener("pointerdown", function (ev) {
        ev.preventDefault();
        svg.appendChild(t.g); // on top while it moves
        t.g.focus();
        var p = at(ev);
        drag = { dx: p[0] - t.x, dy: p[1] - t.y, sx: ev.clientX, sy: ev.clientY };
        t.g.setPointerCapture(ev.pointerId);
      });
      t.g.addEventListener("pointermove", function (ev) {
        if (!drag) return;
        var p = at(ev);
        t.x = p[0] - drag.dx; t.y = p[1] - drag.dy;
        place(t);
      });
      function drop() {
        if (!drag) return;
        drag = null;
        t.x = Math.round(t.x); t.y = Math.round(t.y);
        place(t);
      }
      t.g.addEventListener("pointerup", drop);
      t.g.addEventListener("pointercancel", drop);
      t.g.addEventListener("focus", function () { select(t); });
      t.g.addEventListener("keydown", function (ev) {
        var d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[ev.key];
        if (d) {
          ev.preventDefault();
          t.x += d[0]; t.y += d[1];
          place(t);
        } else if (ev.key === "r" || ev.key === "R") {
          ev.preventDefault();
          turn(t);
        } else if (ev.key === "Enter") {
          ev.preventDefault();
          var form = box.closest("form");
          if (form) form.requestSubmit();
        }
      });
    });

    turnBtn.addEventListener("click", function () {
      turn(selected);
      if (selected) selected.g.focus();
    });
    box.querySelector("[data-reset]").addEventListener("click", reset);
    reset();
  }

  document.querySelectorAll(".atiles[data-squares]").forEach(setup);
})();
