// The drawing and sound kit behind the landing page's "try it" samples
// (#1365): graphs, a fraction pie, a number line, a treble staff, a piano
// keyboard, a speaker button, a small synth (Web Audio) and the browser's
// own voices, plus an expression check. Plain SVG and DOM, no library:
// abcjs is too heavy for a marketing page. landing.js uses it as TryKit.
(function () {
  var NS = 'http://www.w3.org/2000/svg';
  function svg(tag, attrs, parent) {
    var e = document.createElementNS(NS, tag);
    for (var k in attrs) e.setAttribute(k, attrs[k]);
    if (parent) parent.appendChild(e);
    return e;
  }
  function root(w, h, cls) {
    return svg('svg', { viewBox: '0 0 ' + w + ' ' + h, class: cls, 'aria-hidden': 'true' });
  }
  // Where a pointer event lands in an SVG's own units.
  function local(s, ev) {
    var r = s.getBoundingClientRect(), vb = s.viewBox.baseVal;
    return { x: (ev.clientX - r.left) * vb.width / r.width, y: (ev.clientY - r.top) * vb.height / r.height };
  }

  // ---- sound ----
  var ctx = null;
  function audio() {
    if (!ctx) {
      var A = window.AudioContext || window.webkitAudioContext;
      if (!A) return null;
      ctx = new A();
    }
    if (ctx.state === 'suspended') ctx.resume();
    return ctx;
  }
  function freq(midi) { return 440 * Math.pow(2, (midi - 69) / 12); }
  // A soft piano-ish note: a triangle with a quiet sine an octave up, struck and fading.
  function tone(c, midi, at, dur, vol) {
    var g = c.createGain();
    g.gain.setValueAtTime(0.0001, at);
    g.gain.exponentialRampToValueAtTime(vol, at + 0.012);
    g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
    g.connect(c.destination);
    [['triangle', 1, 1], ['sine', 2, 0.25]].forEach(function (o) {
      var osc = c.createOscillator(), og = c.createGain();
      osc.type = o[0];
      osc.frequency.value = freq(midi) * o[1];
      og.gain.value = o[2];
      osc.connect(og); og.connect(g);
      osc.start(at); osc.stop(at + dur + 0.05);
    });
  }
  // Play a list of steps, each a midi number or an array (a chord), one after another.
  function play(steps, gap) {
    var c = audio();
    if (!c) return;
    gap = gap || 0.55;
    var t0 = c.currentTime + 0.05;
    steps.forEach(function (s, i) {
      var notes = [].concat(s);
      notes.forEach(function (m) { tone(c, m, t0 + i * gap, 1.2, 0.22 / notes.length + 0.08); });
    });
  }
  // The browser's own voice for a language, as in the app's tap to hear.
  function say(text, lang) {
    if (!window.speechSynthesis) return;
    var u = new SpeechSynthesisUtterance(text);
    u.lang = lang;
    var v = speechSynthesis.getVoices().filter(function (v) { return v.lang && v.lang.indexOf(lang.slice(0, 2)) === 0; })[0];
    if (v) u.voice = v;
    u.rate = 0.9;
    speechSynthesis.cancel();
    speechSynthesis.speak(u);
  }
  var SPEAKER = 'M3 9h4l5-4v14l-5-4H3z M15.5 8.5a5 5 0 0 1 0 7 M18 6a8.5 8.5 0 0 1 0 12';
  function speaker(label, onPlay, testid) {
    var b = document.createElement('button');
    b.type = 'button';
    b.className = 'try-speaker';
    b.setAttribute('aria-label', label);
    b.dataset.testid = testid || 'try-speaker';
    var s = svg('svg', { viewBox: '0 0 24 24', width: 26, height: 26, 'aria-hidden': 'true' }, b);
    svg('path', { d: SPEAKER, fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, s);
    b.addEventListener('click', function () {
      b.classList.remove('playing'); void b.offsetWidth; b.classList.add('playing');
      onPlay();
    });
    return b;
  }

  // ---- graphs ----
  // opts: x, y ([min, max]), size, fns [{f, cls}], points [[x, y]], tri ({x, y, run, rise}),
  // labels (axis numbers), pick (a callback: tap the grid to put a point on a whole-number spot).
  function graph(o) {
    var size = o.size || 240, pad = o.labels ? 18 : 6;
    var xs = o.x, ys = o.y, w = size, h = size;
    var sx = (w - 2 * pad) / (xs[1] - xs[0]), sy = (h - 2 * pad) / (ys[1] - ys[0]);
    var X = function (x) { return pad + (x - xs[0]) * sx; };
    var Y = function (y) { return h - pad - (y - ys[0]) * sy; };
    var s = root(w, h, 'try-graph' + (o.mini ? ' mini' : ''));
    for (var gx = Math.ceil(xs[0]); gx <= xs[1]; gx++) svg('line', { x1: X(gx), x2: X(gx), y1: Y(ys[0]), y2: Y(ys[1]), class: gx === 0 ? 'axis' : 'grid' }, s);
    for (var gy = Math.ceil(ys[0]); gy <= ys[1]; gy++) svg('line', { x1: X(xs[0]), x2: X(xs[1]), y1: Y(gy), y2: Y(gy), class: gy === 0 ? 'axis' : 'grid' }, s);
    if (o.labels) {
      for (var lx = Math.ceil(xs[0]); lx <= xs[1]; lx++) if (lx) svg('text', { x: X(lx), y: Y(0) + 12, class: 'num', 'text-anchor': 'middle' }, s).textContent = lx;
      for (var ly = Math.ceil(ys[0]); ly <= ys[1]; ly++) if (ly) svg('text', { x: X(0) - 5, y: Y(ly) + 3.5, class: 'num', 'text-anchor': 'end' }, s).textContent = ly;
    }
    (o.fns || []).forEach(function (fn) {
      var d = '', on = false;
      for (var i = 0; i <= 120; i++) {
        var x = xs[0] + (xs[1] - xs[0]) * i / 120, y = fn.f(x);
        var inside = y >= ys[0] - 0.01 && y <= ys[1] + 0.01;
        if (inside) { d += (on ? 'L' : 'M') + X(x).toFixed(1) + ' ' + Y(y).toFixed(1); }
        on = inside;
      }
      svg('path', { d: d, class: 'curve ' + (fn.cls || '') }, s);
    });
    if (o.tri) {
      var t = o.tri;
      svg('path', { d: 'M' + X(t.x) + ' ' + Y(t.y) + 'H' + X(t.x + t.run) + 'V' + Y(t.y + t.rise), class: 'tri' }, s);
      svg('text', { x: (X(t.x) + X(t.x + t.run)) / 2, y: Y(t.y) + 14, class: 'tri-label', 'text-anchor': 'middle' }, s).textContent = t.runLabel;
      svg('text', { x: X(t.x + t.run) + 6, y: (Y(t.y) + Y(t.y + t.rise)) / 2 + 4, class: 'tri-label' }, s).textContent = t.riseLabel;
    }
    (o.points || []).forEach(function (p) { svg('circle', { cx: X(p[0]), cy: Y(p[1]), r: 5, class: 'pt' }, s); });
    if (!o.pick) return s;
    var dot = null, value = null;
    s.classList.add('pickable');
    s.addEventListener('pointerdown', function (ev) {
      if (s.dataset.locked) return;
      var p = local(s, ev);
      var x = Math.round(xs[0] + (p.x - pad) / sx), y = Math.round(ys[0] + (h - pad - p.y) / sy);
      x = Math.max(Math.ceil(xs[0]), Math.min(Math.floor(xs[1]), x));
      y = Math.max(Math.ceil(ys[0]), Math.min(Math.floor(ys[1]), y));
      value = [x, y];
      if (!dot) dot = svg('circle', { r: 7, class: 'pick' }, s);
      dot.setAttribute('cx', X(x)); dot.setAttribute('cy', Y(y));
      o.pick(value);
    });
    s.mark = function (ok) { s.dataset.locked = '1'; if (dot) dot.classList.add(ok ? 'right' : 'wrong'); };
    return s;
  }

  // ---- a fraction pie: tap slices to shade them ----
  function pie(n, onChange) {
    var s = root(160, 160, 'try-pie'), shaded = 0;
    for (var i = 0; i < n; i++) {
      var a0 = (i / n) * 2 * Math.PI - Math.PI / 2, a1 = ((i + 1) / n) * 2 * Math.PI - Math.PI / 2;
      var d = 'M80 80L' + (80 + 70 * Math.cos(a0)).toFixed(2) + ' ' + (80 + 70 * Math.sin(a0)).toFixed(2) +
        'A70 70 0 0 1 ' + (80 + 70 * Math.cos(a1)).toFixed(2) + ' ' + (80 + 70 * Math.sin(a1)).toFixed(2) + 'Z';
      var p = svg('path', { d: d, class: 'slice', 'data-testid': 'try-slice' }, s);
      p.addEventListener('click', function () {
        if (s.dataset.locked) return;
        this.classList.toggle('on');
        shaded = s.querySelectorAll('.slice.on').length;
        onChange(shaded);
      });
    }
    return s;
  }

  // ---- a number line from 0 to 1 in tenths: tap or drag to place the marker ----
  function numberLine(onChange) {
    var s = root(300, 70, 'try-nline'), L = 20, R = 280, value = null, mark = null;
    svg('line', { x1: L, x2: R, y1: 36, y2: 36, class: 'axis' }, s);
    for (var i = 0; i <= 10; i++) {
      var x = L + (R - L) * i / 10;
      svg('line', { x1: x, x2: x, y1: i % 5 ? 30 : 26, y2: i % 5 ? 42 : 46, class: 'tick' }, s);
      if (i === 0 || i === 10) svg('text', { x: x, y: 62, class: 'num', 'text-anchor': 'middle' }, s).textContent = i / 10;
    }
    function set(ev) {
      if (s.dataset.locked) return;
      var p = local(s, ev);
      value = Math.max(0, Math.min(1, Math.round(((p.x - L) / (R - L)) * 20) / 20));
      if (!mark) mark = svg('path', { class: 'marker' }, s);
      var x = L + (R - L) * value;
      mark.setAttribute('d', 'M' + x + ' 34l-7 -14h14z');
      onChange(value);
    }
    s.addEventListener('pointerdown', function (ev) { set(ev); s.setPointerCapture(ev.pointerId); });
    s.addEventListener('pointermove', function (ev) { if (ev.buttons) set(ev); });
    s.mark = function (ok) { s.dataset.locked = '1'; if (mark) mark.classList.add(ok ? 'right' : 'wrong'); };
    return s;
  }

  // ---- a treble staff ----
  // Notes are names with an octave ('C4' to 'G5'); a step is a line or space above E4, the bottom line.
  var LETTERS = 'CDEFGAB';
  function step(n) { return LETTERS.indexOf(n[0]) + 7 * (+n[n.length - 1] - 4) - 2; }
  function midi(n) { return [0, 2, 4, 5, 7, 9, 11][LETTERS.indexOf(n[0])] + 12 * (+n[n.length - 1] + 1); }
  function noteName(st) { var k = st + 2; return LETTERS[((k % 7) + 7) % 7] + (4 + Math.floor(k / 7)); }
  // groups: a list of notes or chords (arrays) drawn left to right as whole notes.
  // opts.place(name): tap the staff to put one note on it.
  function staff(groups, opts) {
    opts = opts || {};
    var gap = opts.gap || 30, w = opts.place ? 210 : 64 + Math.max(groups.length, 1) * gap;
    var s = root(w, 84, 'try-staff' + (opts.mini ? ' mini' : ''));
    s.dataset.testid = 'try-staff';
    var Y = function (st) { return 62 - st * 5; };
    for (var i = 0; i < 5; i++) svg('line', { x1: 2, x2: w - 2, y1: Y(i * 2), y2: Y(i * 2), class: 'line' }, s);
    svg('text', { x: 4, y: 66, class: 'clef' }, s).textContent = '\u{1D11E}';
    function head(x, st, cls) {
      var g = svg('g', { class: 'note ' + (cls || '') }, s);
      if (st <= -2) svg('line', { x1: x - 10, x2: x + 10, y1: Y(-2), y2: Y(-2), class: 'line' }, g);
      if (st >= 10) svg('line', { x1: x - 10, x2: x + 10, y1: Y(10), y2: Y(10), class: 'line' }, g);
      svg('ellipse', { cx: x, cy: Y(st), rx: 6.4, ry: 4.5, transform: 'rotate(-20 ' + x + ' ' + Y(st) + ')', class: 'head' }, g);
      return g;
    }
    groups.forEach(function (gr, i) { [].concat(gr).forEach(function (n) { head(60 + i * gap, step(n)); }); });
    if (!opts.place) return s;
    var placed = null, value = null, x = 130;
    s.classList.add('pickable');
    s.addEventListener('pointerdown', function (ev) {
      if (s.dataset.locked) return;
      var st = Math.max(-2, Math.min(9, Math.round((62 - local(s, ev).y) / 5)));
      if (placed) placed.remove();
      placed = head(x, st, 'placed');
      value = noteName(st);
      opts.place(value);
    });
    s.mark = function (ok) { s.dataset.locked = '1'; if (placed) placed.classList.add(ok ? 'right' : 'wrong'); };
    return s;
  }

  // ---- a piano keyboard, C4 to E5 ----
  // opts.lit: note names to colour; opts.tap(name): a key was pressed (it also sounds).
  function keyboard(opts) {
    opts = opts || {};
    var whites = ['C4', 'D4', 'E4', 'F4', 'G4', 'A4', 'B4', 'C5', 'D5', 'E5'], blacks = { C: 1, D: 1, F: 1, G: 1, A: 1 };
    var kw = 30, s = root(whites.length * kw + 2, 110, 'try-keys');
    var lit = opts.lit || [];
    function key(name, x, black) {
      var r = svg('rect', black ? { x: x, y: 1, width: 18, height: 64, rx: 3 } : { x: x, y: 1, width: kw, height: 106, rx: 4 }, s);
      r.setAttribute('class', (black ? 'black' : 'white') + (lit.indexOf(name) >= 0 ? ' lit' : ''));
      r.dataset.note = name;
      r.dataset.testid = 'try-key';
      r.addEventListener('pointerdown', function (ev) {
        ev.preventDefault();
        play([keyMidi(name)]);
        r.classList.add('down');
        setTimeout(function () { r.classList.remove('down'); }, 180);
        if (opts.tap && !s.dataset.locked) opts.tap(name, r);
      });
    }
    whites.forEach(function (n, i) { key(n, 1 + i * kw, false); });
    whites.forEach(function (n, i) {
      if (blacks[n[0]] && i < whites.length - 1) key(n[0] + '#' + n[1], 1 + (i + 1) * kw - 9, true);
    });
    if (lit.length) whites.forEach(function (n, i) {
      if (lit.indexOf(n) >= 0) svg('text', { x: 1 + i * kw + kw / 2, y: 98, class: 'key-label', 'text-anchor': 'middle' }, s).textContent = n[0];
    });
    return s;
  }
  function blackMidi(name) { return midi(name[0] + name[2]) + 1; }
  function keyMidi(name) { return name.length === 3 ? blackMidi(name) : midi(name); }

  // ---- algebra: is the typed expression the same as the answer for every x? ----
  // A small parser for + - * / ^ ², brackets and implicit multiplication (2x, x(x+1)).
  function parse(src) {
    var s = src.replace(/\s+/g, '').replace(/²/g, '^2').replace(/³/g, '^3').replace(/[×·]/g, '*').replace(/−/g, '-').toLowerCase();
    var i = 0;
    function peek() { return s[i]; }
    function expr() {
      var v = term();
      while (peek() === '+' || peek() === '-') { var op = s[i++], r = term(); v = op === '+' ? add(v, r) : sub(v, r); }
      return v;
    }
    function add(a, b) { return function (x) { return a(x) + b(x); }; }
    function sub(a, b) { return function (x) { return a(x) - b(x); }; }
    function term() {
      var v = unary();
      for (;;) {
        var c = peek();
        if (c === '*' || c === '/') { i++; var r = unary(); v = (function (a, b, op) { return op === '*' ? function (x) { return a(x) * b(x); } : function (x) { return a(x) / b(x); }; })(v, r, c); }
        else if (c === 'x' || c === '(' || (c >= '0' && c <= '9')) { var r2 = unary(); v = (function (a, b) { return function (x) { return a(x) * b(x); }; })(v, r2); }
        else return v;
      }
    }
    function unary() {
      if (peek() === '-') { i++; var u = unary(); return function (x) { return -u(x); }; }
      if (peek() === '+') { i++; return unary(); }
      return power();
    }
    function power() {
      var b = atom();
      if (peek() === '^') { i++; var e = unary(); return function (x) { return Math.pow(b(x), e(x)); }; }
      return b;
    }
    function atom() {
      var c = peek();
      if (c === '(') { i++; var v = expr(); if (s[i++] !== ')') throw 0; return v; }
      if (c === 'x') { i++; return function (x) { return x; }; }
      var m = s.slice(i).match(/^\d*\.?\d+/);
      if (!m) throw 0;
      i += m[0].length;
      var n = +m[0];
      return function () { return n; };
    }
    var f = expr();
    if (i !== s.length) throw 0;
    return f;
  }
  function sameExpression(a, b) {
    try {
      var f = parse(a), g = parse(b);
      return [-2.3, -0.7, 0.5, 1.9, 3.1].every(function (x) { var u = f(x), v = g(x); return isFinite(u) && Math.abs(u - v) < 1e-7 * (1 + Math.abs(v)); });
    } catch (e) { return false; }
  }

  window.TryKit = {
    svg: svg, graph: graph, pie: pie, numberLine: numberLine, staff: staff, keyboard: keyboard,
    play: play, say: say, speaker: speaker, midi: midi, keyMidi: keyMidi, sameExpression: sameExpression,
  };
})();
