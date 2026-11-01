// "Measure the angle" (#1352, src/measure.rs). Each [data-testid=measure]
// box draws its question's angle and a see-through protractor with JSXGraph
// (vendor/jsxgraphcore.js, loaded only when a page has such a box). The
// learner drags the protractor, turns it by its handle or with two fingers,
// reads the angle and types the number in the answer box below; the server
// grades it. Keys: arrows move it, Shift with ← or → turns it, Enter goes to
// the answer box.
(function () {
  "use strict";
  var boxes = Array.prototype.slice.call(document.querySelectorAll('[data-testid="measure"]'));
  if (!boxes.length) return;

  var R = 0.95; // protractor radius, in board units
  var RAY = 1.2; // length of the angle's arms: longer than R, so they cross the scale
  var BOX = [-1.75, 1.35, 1.75, -1.35];
  var BLUE = "#1f5fbf";
  var STEP = 0.02, TURN = Math.PI / 180;

  var css = document.createElement("style"); // here, not in style.css: only these pages need it
  css.textContent = ".measure{width:100%;max-width:30rem;aspect-ratio:35/27;margin:.5rem 0 .75rem;border:1px solid currentColor;border-radius:8px;position:relative;overflow:hidden;touch-action:none}" +
    ".measure:focus-visible{outline:3px solid " + BLUE + ";outline-offset:2px}";
  document.head.appendChild(css);

  // The protractor as an SVG picture, in units of R = 100: centre at (0, 0),
  // the outer scale counting from the left, the inner one from the right.
  function protractorSvg() {
    var p = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="-104 -104 208 108">',
      '<path d="M -100 0 A 100 100 0 0 1 100 0 Z" fill="rgba(232,241,255,0.55)" stroke="' + BLUE + '" stroke-width="0.8"/>',
      '<path d="M -40 0 A 40 40 0 0 1 40 0" fill="none" stroke="' + BLUE + '" stroke-width="0.4"/>'];
    for (var d = 0; d <= 180; d++) {
      var a = d * Math.PI / 180, c = Math.cos(a), s = -Math.sin(a);
      var len = d % 10 === 0 ? 12 : d % 5 === 0 ? 8 : 4;
      p.push('<line x1="' + (100 * c).toFixed(2) + '" y1="' + (100 * s).toFixed(2) + '" x2="' + ((100 - len) * c).toFixed(2) + '" y2="' + ((100 - len) * s).toFixed(2) +
        '" stroke="' + BLUE + '" stroke-width="' + (d % 10 === 0 ? 0.6 : 0.35) + '"/>');
      if (d % 10 === 0) {
        p.push('<line x1="' + (40 * c).toFixed(2) + '" y1="' + (40 * s).toFixed(2) + '" x2="' + (34 * c).toFixed(2) + '" y2="' + (34 * s).toFixed(2) + '" stroke="' + BLUE + '" stroke-width="0.4"/>');
        var turn = (90 - d).toFixed(0);
        p.push(label(82, a, 180 - d, 6.5, turn, "#0d3b80", "600")); // outer scale: 0 on the left
        p.push(label(71, a, d, 5, turn, BLUE, "400")); // inner scale: 0 on the right
      }
    }
    p.push('<line x1="-100" y1="0" x2="100" y2="0" stroke="' + BLUE + '" stroke-width="0.8"/>');
    p.push('<line x1="0" y1="0" x2="0" y2="-8" stroke="' + BLUE + '" stroke-width="0.6"/><circle cx="0" cy="0" r="1.6" fill="' + BLUE + '"/>');
    p.push("</svg>");
    return "data:image/svg+xml;charset=utf-8," + encodeURIComponent(p.join(""));
  }
  function label(r, a, n, size, turn, fill, weight) {
    var x = (r * Math.cos(a)).toFixed(2), y = (-r * Math.sin(a)).toFixed(2);
    return '<text x="' + x + '" y="' + y + '" font-family="sans-serif" font-size="' + size + '" font-weight="' + weight + '" fill="' + fill +
      '" text-anchor="middle" dominant-baseline="central" transform="rotate(' + turn + " " + x + " " + y + ')">' + n + "</text>";
  }

  function draw(box) {
    var JXG = window.JXG;
    var angle = Number(box.dataset.angle) * Math.PI / 180;
    if (!JXG || !(angle > 0)) return;
    if (!box.id) box.id = "measure-" + Math.random().toString(36).slice(2);
    var ink = getComputedStyle(box).color;
    var label = box.getAttribute("aria-label");
    var board = JXG.JSXGraph.initBoard(box.id, {
      title: label,
      boundingbox: BOX, keepAspectRatio: true, axis: false, grid: false,
      showNavigation: false, showCopyright: false, showFullscreen: false, showScreenshot: false, showInfobox: false,
      pan: { enabled: false }, zoom: { enabled: false }, keyboard: { enabled: false }
    });
    box.setAttribute("role", "application"); // JSXGraph sets its own
    box.setAttribute("aria-label", label);
    var still = { fixed: true, highlight: false, withLabel: false };

    // The angle: its vertex at the middle, turned any way, so the
    // protractor has to be moved and turned onto it.
    var base = Math.random() * 2 * Math.PI;
    [base, base + angle].forEach(function (t) {
      board.create("segment", [[0, 0], [RAY * Math.cos(t), RAY * Math.sin(t)]], Object.assign({ strokeColor: ink, strokeWidth: 2.5 }, still));
    });
    var arc = [];
    for (var i = 0; i <= 24; i++) arc.push(base + angle * i / 24);
    board.create("curve", [arc.map(function (t) { return 0.2 * Math.cos(t); }), arc.map(function (t) { return 0.2 * Math.sin(t); })],
      Object.assign({ strokeColor: ink, strokeWidth: 1.5 }, still));

    // The protractor: where its centre is, and how far it is turned.
    var at = { x: 0.55, y: -1.25, turn: 0 };
    var img = board.create("image", [protractorSvg(), [-1.04 * R, -0.04 * R], [2.08 * R, 1.08 * R]], Object.assign({ opacity: 0.92 }, still));
    img.addTransform([
      board.create("transform", [function () { return at.turn; }, 0, 0], { type: "rotate" }),
      board.create("transform", [function () { return at.x; }, function () { return at.y; }], { type: "translate" })
    ]);
    var H = R + 0.14; // the turning handle, past the 0 of the inner scale
    var knob = board.create("point", [function () { return at.x + H * Math.cos(at.turn); }, function () { return at.y + H * Math.sin(at.turn); }],
      Object.assign({ name: "", size: 7, fillColor: BLUE, strokeColor: "#fff", strokeWidth: 2, showInfobox: false }, still));
    knob.rendNode && knob.rendNode.setAttribute("data-testid", "measure-handle");

    function place(x, y, turn) {
      at.x = Math.max(BOX[0], Math.min(BOX[2], x));
      at.y = Math.max(BOX[3], Math.min(BOX[1], y));
      at.turn = turn;
      box.dataset.at = [at.x.toFixed(3), at.y.toFixed(3), (turn * 180 / Math.PI).toFixed(1)].join(",");
      board.update();
    }
    place(at.x, at.y, at.turn);

    // Pointers: on the handle one finger turns it; on the protractor one
    // finger moves it and two move and turn it.
    var down = {}; // pointerId -> [x, y] in board units
    var grab = null;
    function usr(ev) { return board.getUsrCoordsOfMouse(ev); }
    function local(p) {
      var dx = p[0] - at.x, dy = p[1] - at.y, c = Math.cos(-at.turn), s = Math.sin(-at.turn);
      return [dx * c - dy * s, dx * s + dy * c];
    }
    function startGrab() {
      var ids = Object.keys(down);
      if (ids.length >= 2) {
        var a = down[ids[0]], b = down[ids[1]];
        grab = { kind: "two", a: ids[0], b: ids[1], from: Math.atan2(b[1] - a[1], b[0] - a[0]), mid: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], at: Object.assign({}, at) };
      }
    }
    box.addEventListener("pointerdown", function (ev) {
      var p = usr(ev);
      var q = local(p);
      var onKnob = Math.hypot(p[0] - (at.x + H * Math.cos(at.turn)), p[1] - (at.y + H * Math.sin(at.turn))) < 0.16;
      var onBody = q[1] > -0.08 && Math.hypot(q[0], q[1]) < R + 0.04;
      if (!onKnob && !onBody && !Object.keys(down).length) return;
      ev.preventDefault();
      box.setPointerCapture && box.setPointerCapture(ev.pointerId);
      down[ev.pointerId] = p;
      if (Object.keys(down).length >= 2) startGrab();
      else grab = onKnob ? { kind: "turn" } : { kind: "move", from: p, at: Object.assign({}, at) };
    });
    box.addEventListener("pointermove", function (ev) {
      if (!(ev.pointerId in down) || !grab) return;
      var p = usr(ev);
      down[ev.pointerId] = p;
      if (grab.kind === "turn") place(at.x, at.y, Math.atan2(p[1] - at.y, p[0] - at.x));
      else if (grab.kind === "move") place(grab.at.x + p[0] - grab.from[0], grab.at.y + p[1] - grab.from[1], at.turn);
      else if (down[grab.a] && down[grab.b]) {
        var a = down[grab.a], b = down[grab.b];
        var mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
        var t = Math.atan2(b[1] - a[1], b[0] - a[0]) - grab.from;
        // Turn about the fingers' midpoint, then follow it.
        var dx = grab.at.x - grab.mid[0], dy = grab.at.y - grab.mid[1];
        place(mid[0] + dx * Math.cos(t) - dy * Math.sin(t), mid[1] + dx * Math.sin(t) + dy * Math.cos(t), grab.at.turn + t);
      }
    });
    function up(ev) {
      if (!(ev.pointerId in down)) return;
      delete down[ev.pointerId];
      var left = Object.keys(down);
      grab = left.length ? { kind: "move", from: down[left[0]], at: Object.assign({}, at) } : null;
    }
    box.addEventListener("pointerup", up);
    box.addEventListener("pointercancel", up);

    box.addEventListener("keydown", function (ev) {
      if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
      var k = ev.key;
      if (k === "Enter") {
        var q = box.closest(".question");
        var inp = q && q.querySelector("input.fill");
        if (inp) { ev.preventDefault(); inp.focus(); }
        return;
      }
      var move = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] }[k];
      if (!move) return;
      ev.preventDefault();
      if (ev.shiftKey && move[0]) place(at.x, at.y, at.turn - move[0] * TURN); // Shift+→ turns clockwise
      else place(at.x + move[0] * STEP, at.y + move[1] * STEP, at.turn);
    });
    box.dataset.ready = "true";
  }

  // A box inside a hidden question has no size yet: draw it when it shows.
  function start() {
    var seen = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting && !e.target.dataset.ready) { seen.unobserve(e.target); draw(e.target); }
      });
    });
    boxes.forEach(function (b) { seen.observe(b); });
  }
  if (window.JXG) return start();
  var s = document.createElement("script");
  s.src = boxes[0].dataset.lib;
  s.onload = start;
  document.head.appendChild(s);
})();
