// The "try it" stage on the landing and waitlist pages (#1365): a
// twenty-second sample of each mode in three subjects, each showing off what
// that subject's questions can do: a language you hear and type, maths you
// shade, place and plot, music you hear, read and play. The samples are
// course data, like the hero's quiz card, so they are not translated; the
// app's own words come from I18N (landing.js.*). The drawing and the sound
// are in landing-kit.js (TryKit).
(function () {
  var root = document.getElementById('land-try');
  if (!root || !window.TryKit) return;
  var K = window.TryKit;
  var stage = root.querySelector('.try-stage');
  var tabs = root.querySelectorAll('.try-tab');
  var subjects = root.querySelectorAll('.try-subject');

  // ---- small helpers ----
  var timers = [];
  function later(fn, ms) { timers.push(setTimeout(fn, ms)); }
  function clearTimers() { timers.forEach(clearTimeout); timers = []; }
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function button(cls, text, onClick, testid) {
    var b = el('button', 'btn ' + cls, text);
    b.type = 'button';
    if (testid) b.dataset.testid = testid;
    b.addEventListener('click', onClick);
    return b;
  }
  // **word** in a sample line becomes bold, the rest stays text.
  function rich(cls, text) {
    var p = el('p', cls);
    text.split(/\*\*(.+?)\*\*/).forEach(function (part, i) {
      p.appendChild(i % 2 ? el('strong', null, part) : document.createTextNode(part));
    });
    return p;
  }
  function row(cls) { var d = el('div', cls); d.append.apply(d, [].slice.call(arguments, 1)); return d; }
  function hear(text) { return K.speaker(t('landing.js.listen'), function () { K.say(text, 'es-ES'); }); }
  function sound(steps, gap) { return K.speaker(t('landing.js.listen'), function () { K.play(steps, gap); }); }
  function fold(s) { return s.normalize('NFD').replace(/[̀-ͯ]/g, ''); }
  function clean(s) { return s.trim().toLowerCase().replace(/^[¿¡"'.,!?]+|[¿¡"'.,!?]+$/g, '').replace(/\s+/g, ' '); }

  // ---- the samples ----
  // An exercise: label (an I18N key), media() (a picture or a speaker above
  // it), prompt, answer (shown when it was missed), and one kind:
  //   choice  options (text, or {value, draw(), alt} for pictures as options;
  //           alt names the picture for a screen reader)
  //   build   tiles to tap into order
  //   typed   grade(text) -> 'right' | 'near' | 'wrong'
  //   widget  make(set) draws something to tap; grade(value); instant: no Check button
  var M = function (n) { return K.midi(n); };
  var SAMPLES = {
    language: {
      sprint: [
        { label: 'landing.js.sprint_pick', prompt: 'gracias', kind: 'choice', options: ['please', 'thank you', 'sorry'], answer: 'thank you' },
        { label: 'landing.js.sprint_listen', media: function () { return hear('Buenas noches'); }, autoplay: true,
          kind: 'choice', options: ['Buenos días', 'Buenas noches', 'Buenas tardes'], answer: 'Buenas noches' },
        { label: 'landing.js.sprint_build', prompt: 'How are you?', kind: 'build', tiles: ['estás?', 'tú', '¿Cómo'], answer: '¿Cómo estás?' },
      ],
      study: {
        crumb: 'A1 · Greetings', title: 'Saying hello',
        text: 'Use **buenos días** until lunch, **buenas tardes** until dark and **buenas noches** after that. **¿Cómo estás?** asks how someone is.',
        media: function () { return row('try-media-row', hear('Buenos días. ¿Cómo estás?'), el('span', 'muted small', t('landing.js.tap_to_hear'))); },
        questions: [
          { prompt: 'At 9 in the morning you say:', kind: 'choice', options: ['Buenas noches', 'Buenos días', 'Buenas tardes'], answer: 'Buenos días', why: 'Días = days; until lunch.' },
          { label: 'landing.js.type_it', prompt: '¿Cómo ___? (How are you?)', kind: 'typed', answer: 'estás',
            grade: function (v) { v = clean(v); return v === 'estás' ? 'right' : fold(v) === 'estas' ? 'near' : 'wrong'; },
            why: 'Typed answers forgive a missing accent and point it out.' },
        ],
      },
      play: { pairs: [[{ text: 'perro', say: 'perro' }, 'dog'], [{ text: 'gato', say: 'gato' }, 'cat'], [{ text: 'casa', say: 'casa' }, 'house'], [{ text: 'agua', say: 'agua' }, 'water']], order: [3, 2, 0, 1] },
      guide: {
        tip: '**¿Qué tal?** is a friendly "how\'s it going?". Answer with **bien, gracias**.',
        media: function () { return hear('¿Qué tal? Bien, gracias.'); },
        check: { label: 'landing.js.guide_check', prompt: 'You say «¿Qué tal?» to a good friend.', kind: 'choice', truth: true },
        front: function () { return el('span', null, 'bien'); }, back: 'well, fine', onFlip: function () { K.say('bien', 'es-ES'); },
      },
    },
    maths: {
      sprint: [
        { label: 'landing.js.shade', prompt: 'Shade 3/8 of the circle.', kind: 'widget', answer: '3 of the 8 slices',
          make: function (set) { return K.pie(8, set); }, grade: function (v) { return v === 3; } },
        { label: 'landing.js.number_line', prompt: 'Put 0.75 on the line.', kind: 'widget', answer: '0.75, three quarters of the way',
          make: function (set) { return K.numberLine(set); }, grade: function (v) { return Math.abs(v - 0.75) < 0.026; } },
        { label: 'landing.js.sprint_order', prompt: 'Smallest to largest', kind: 'build', tiles: ['0.5', '3/4', '1/3'], answer: '1/3 0.5 3/4' },
      ],
      study: {
        crumb: 'Unit 3 · Straight-line graphs', title: 'y = mx + c',
        text: 'In **y = 2x − 1**, the **2** is the slope: up 2 for every 1 across. The **−1** is where the line crosses the y-axis.',
        media: function () { return K.graph({ x: [-1, 5], y: [-2, 6], size: 220, labels: true, fns: [{ f: function (x) { return 2 * x - 1; } }] }); },
        questions: [
          { label: 'landing.js.plot', prompt: 'On y = 2x − 1, plot the point where x = 2.', kind: 'widget', answer: '(2, 3)',
            make: function (set) { return K.graph({ x: [-1, 5], y: [-2, 6], size: 220, labels: true, fns: [{ f: function (x) { return 2 * x - 1; }, cls: 'faint' }], pick: set }); },
            grade: function (v) { return v[0] === 2 && v[1] === 3; }, why: '2 × 2 − 1 = 3, so the point is (2, 3).' },
          { label: 'landing.js.type_formula', prompt: 'Expand (x + 1)²', kind: 'typed', answer: 'x² + 2x + 1', math: true,
            grade: function (v) { return K.sameExpression(v, 'x^2+2x+1') ? 'right' : 'wrong'; },
            why: 'Graded by equivalence: x^2 + 2x + 1, 1 + 2x + x² and (x+1)(x+1) all count.' },
        ],
      },
      play: {
        pairs: [
          ['y = x', { draw: mini(function (x) { return x; }), alt: 'graph: a straight line rising through the middle' }],
          ['y = x²', { draw: mini(function (x) { return x * x; }), alt: 'graph: a U-shaped curve' }],
          ['y = −x + 2', { draw: mini(function (x) { return 2 - x; }), alt: 'graph: a straight line going down' }],
          ['y = 2', { draw: mini(function () { return 2; }), alt: 'graph: a flat line above the middle' }],
        ],
        order: [2, 0, 3, 1],
      },
      guide: {
        tip: 'The **slope** is rise over run: up 2 for every 1 across is a slope of 2.',
        media: function () { return K.graph({ x: [-1, 3], y: [-1, 5], size: 170, fns: [{ f: function (x) { return 2 * x; } }], tri: { x: 1, y: 2, run: 1, rise: 2, runLabel: '1', riseLabel: '2' } }); },
        check: { label: 'landing.js.match_graph', prompt: 'Which line has a negative slope?', kind: 'choice', answer: 'the line going down',
          options: [{ value: 'up', draw: mini(function (x) { return x + 1; }), alt: 'graph: a line going up' }, { value: 'down', draw: mini(function (x) { return 1 - x; }), alt: 'graph: a line going down' }, { value: 'flat', draw: mini(function () { return 1; }), alt: 'graph: a flat line' }], correct: 'down' },
        front: function () { return K.graph({ x: [-3, 3], y: [-1, 5], size: 120, fns: [{ f: function (x) { return x * x; } }] }); }, back: 'y = x², a parabola',
      },
    },
    music: {
      sprint: [
        { label: 'landing.js.sprint_note', media: function () { return K.staff(['B4']); }, kind: 'choice', options: ['G', 'B', 'D'], answer: 'B' },
        { label: 'landing.js.hear_interval', media: function () { return sound([M('C4'), M('G4')]); }, autoplay: true, prompt: 'C up to…',
          kind: 'choice', options: ['a 3rd', 'a 5th', 'an octave'], answer: 'a 5th' },
        { label: 'landing.js.find_key', prompt: 'Tap the E, a major 3rd above C.', kind: 'widget', instant: true, answer: 'E',
          make: function (set) { return K.keyboard({ tap: set }); }, grade: function (v) { return v[0] === 'E' && v.length === 2; } },
      ],
      study: {
        crumb: 'Unit 1 · Scales', title: 'The C major scale',
        text: 'Eight notes, **C D E F G A B C**, and no sharps or flats. The lines of the staff are **E G B D F**; the spaces spell **F A C E**.',
        media: function () { var steps = ['C4', 'D4', 'E4', 'F4', 'G4', 'A4', 'B4', 'C5']; return row('try-media-row', K.staff(steps, { gap: 22 }), sound(steps.map(M), 0.32)); },
        questions: [
          { label: 'landing.js.place_note', prompt: 'Place an A on the staff.', kind: 'widget', answer: 'A, the second space',
            make: function (set) { return K.staff([], { place: function (n) { K.play([M(n)]); set(n); } }); },
            grade: function (v) { return v === 'A4'; }, why: 'The spaces spell F A C E: A is the second space.' },
          { label: 'landing.js.which_heard', media: function () { return sound(['C4', 'E4', 'G4', 'C5'].map(M), 0.4); }, autoplay: true, kind: 'choice',
            options: [{ value: 'a', alt: 'staff: C D E C', draw: function () { return K.staff(['C4', 'D4', 'E4', 'C4'], { mini: true, gap: 20 }); } },
              { value: 'b', alt: 'staff: C E G C', draw: function () { return K.staff(['C4', 'E4', 'G4', 'C5'], { mini: true, gap: 20 }); } },
              { value: 'c', alt: 'staff: G E C G', draw: function () { return K.staff(['G4', 'E4', 'C4', 'G4'], { mini: true, gap: 20 }); } }],
            correct: 'b', answer: 'C E G C', why: 'It climbs C, E, G, then the high C.' },
        ],
      },
      play: {
        pairs: [
          ['C major', chord(['C4', 'E4', 'G4'])], ['F major', chord(['F4', 'A4', 'C5'])],
          ['G major', chord(['G4', 'B4', 'D5'])], ['A minor', chord(['A4', 'C5', 'E5'])],
        ],
        order: [1, 3, 0, 2],
      },
      guide: {
        tip: 'A **perfect fifth** is 7 semitones: C up to G. Tap the keys to hear it.',
        media: function () { return row('try-media-row', K.keyboard({ lit: ['C4', 'G4'] }), sound([M('C4'), M('G4')])); },
        check: { label: 'landing.js.major_minor', media: function () { return sound([['A4', 'C5', 'E5'].map(M)]); }, autoplay: true, kind: 'choice', options: ['Major', 'Minor'], answer: 'Minor' },
        front: function () { return K.staff([['C4', 'E4', 'G4']], { mini: true }); }, back: 'C major: C E G',
        onFlip: function () { K.play([['C4', 'E4', 'G4'].map(M)]); },
      },
    },
  };
  function mini(f) { return function () { return K.graph({ x: [-3, 3], y: [-3, 3], size: 84, mini: true, fns: [{ f: f }] }); }; }
  function chord(notes) { return { draw: function () { return K.staff([notes], { mini: true }); }, sound: [notes.map(M)], alt: 'staff: ' + notes.map(function (n) { return n[0]; }).join(' ') }; }

  // ---- one exercise ----
  // Draws ex into body and calls done(ok, note) once it is answered.
  function ask(ex, body, done) {
    if (ex.label) body.appendChild(el('p', 'try-label', t(ex.label)));
    if (ex.media) {
      var m = ex.media();
      body.appendChild(m);
      if (ex.autoplay) { var sp = m.matches('.try-speaker') ? m : m.querySelector('.try-speaker'); if (sp) later(function () { sp.click(); }, 250); }
    }
    if (ex.prompt) body.appendChild(el('p', 'try-prompt', ex.prompt));
    var kinds = { choice: choice, build: build, typed: typed, widget: widget };
    kinds[ex.kind](ex, body, done);
  }
  function choice(ex, body, done) {
    var opts = ex.options;
    if (ex.truth != null) opts = [t('landing.js.answer_true'), t('landing.js.answer_false')];
    var correct = ex.correct || (ex.truth != null ? opts[ex.truth ? 0 : 1] : ex.answer);
    if (ex.truth != null) ex.answer = correct;
    var box = el('div', 'try-options' + (typeof opts[0] === 'object' ? ' pictures' : ''));
    opts.forEach(function (o) {
      var value = typeof o === 'object' ? o.value : o;
      var b = button('try-opt', typeof o === 'object' ? '' : o, function () {
        if (box.dataset.done) return;
        box.dataset.done = '1';
        box.querySelectorAll('button').forEach(function (x) {
          if (x.dataset.value === correct) x.classList.add('right');
          else if (x === b) x.classList.add('wrong');
          x.disabled = true;
        });
        done(value === correct);
      }, 'try-opt');
      b.dataset.value = value;
      if (typeof o === 'object') { b.appendChild(o.draw()); b.setAttribute('aria-label', o.alt); }
      box.appendChild(b);
    });
    body.appendChild(box);
  }
  function build(ex, body, done) {
    var line = el('div', 'try-line');
    line.dataset.testid = 'try-line';
    var bank = el('div', 'try-bank');
    var check = button('', t('landing.js.check'), function () {
      var ok = Array.prototype.map.call(line.children, function (b) { return b.textContent; }).join(' ') === ex.answer;
      line.classList.add(ok ? 'right' : 'wrong');
      body.querySelectorAll('.try-tile').forEach(function (b) { b.disabled = true; });
      check.disabled = true;
      done(ok);
    }, 'try-check');
    check.disabled = true;
    ex.tiles.forEach(function (w) {
      var tile = button('try-tile', w, function () {
        (tile.parentNode === bank ? line : bank).appendChild(tile);
        check.disabled = line.children.length === 0;
      }, 'try-tile');
      bank.appendChild(tile);
    });
    body.append(line, bank, check);
  }
  function typed(ex, body, done) {
    var form = el('form', 'try-typed');
    var input = el('input');
    input.type = 'text'; input.autocomplete = 'off'; input.spellcheck = false;
    input.setAttribute('autocapitalize', 'off');
    input.placeholder = t(ex.math ? 'landing.js.type_formula_hint' : 'landing.js.type_answer');
    input.setAttribute('aria-label', t('landing.js.type_answer'));
    input.dataset.testid = 'try-input';
    var check = button('', t('landing.js.check'), function () {}, 'try-check');
    check.type = 'submit';
    form.append(input, check);
    form.addEventListener('submit', function (e) {
      e.preventDefault();
      if (!input.value.trim() || form.dataset.done) return;
      form.dataset.done = '1';
      var g = ex.grade(input.value);
      input.classList.add(g === 'wrong' ? 'wrong' : 'right');
      input.readOnly = true; check.disabled = true;
      done(g !== 'wrong', g === 'near' ? t('landing.js.near_miss', { answer: ex.answer }) : null);
    });
    body.appendChild(form);
  }
  function widget(ex, body, done) {
    var value = null, check = null, over = false, pic;
    function finish() {
      if (over) return;
      over = true;
      var ok = ex.grade(value);
      if (pic.mark) pic.mark(ok);
      if (check) check.disabled = true;
      done(ok);
    }
    pic = ex.make(function (v) {
      value = v;
      if (ex.instant) finish();
      else check.disabled = false;
    });
    body.appendChild(row('try-widget', pic));
    if (ex.instant) return;
    check = button('', t('landing.js.check'), finish, 'try-check');
    check.disabled = true;
    body.appendChild(check);
  }

  // The end card of every sample: what you got, and Again.
  function finishCard(head, result, line, again) {
    head.replaceChildren();
    var card = el('div', 'try-done');
    card.dataset.testid = 'try-done';
    card.appendChild(el('p', 'try-done-big', result));
    if (line) card.appendChild(el('p', 'muted small', line));
    card.appendChild(button('ghost', t('landing.js.again'), again, 'try-again'));
    return card;
  }
  function missed(body, ex) { body.appendChild(el('p', 'try-why', t('landing.js.wrong', { answer: ex.answer }))); }

  // ---- Sprint: three quick exercises against the session ring ----
  function sprint(head, body, data) {
    var list = data.sprint, right = 0;
    var ring = el('span', 'try-ring');
    var count = el('span', 'muted small');
    function show(i) {
      ring.style.setProperty('--p', (i / list.length) * 100);
      count.textContent = t('landing.js.count', { n: Math.min(i + 1, list.length), total: list.length });
      if (i === list.length) {
        body.replaceChildren(finishCard(head, t('landing.js.sprint_done', { right: right, total: list.length }), t('landing.js.sprint_done_line'), run));
        return;
      }
      body.replaceChildren();
      ask(list[i], body, function (ok, note) {
        if (ok) right++; else missed(body, list[i]);
        if (note) body.appendChild(el('p', 'try-why', note));
        later(function () { show(i + 1); }, ok && !note ? 900 : 1900);
      });
    }
    function run() { clearTimers(); right = 0; head.replaceChildren(ring, count); show(0); }
    run();
  }

  // ---- Study: read a short lesson, then a two-question check ----
  function study(head, body, data) {
    var lesson = data.study, right = 0;
    var crumb = el('span', 'muted small', lesson.crumb);
    function read() {
      right = 0;
      head.replaceChildren(crumb);
      body.replaceChildren(el('p', 'try-title', lesson.title), rich('try-text', lesson.text));
      if (lesson.media) body.appendChild(lesson.media());
      body.appendChild(button('', t('landing.js.read_quiz'), function () { quiz(0); }, 'try-read'));
    }
    function quiz(i) {
      var qs = lesson.questions;
      if (i === qs.length) {
        var passed = right / qs.length >= 0.7;
        body.replaceChildren(finishCard(head, t(passed ? 'landing.js.study_passed' : 'landing.js.study_not', { right: right, total: qs.length }),
          t(passed ? 'landing.js.study_passed_line' : 'landing.js.study_not_line'), read));
        return;
      }
      var q = qs[i];
      head.replaceChildren(crumb, el('span', 'muted small try-count', t('landing.js.count', { n: i + 1, total: qs.length })));
      body.replaceChildren();
      ask(q, body, function (ok, note) {
        if (ok) right++; else missed(body, q);
        if (note) body.appendChild(el('p', 'try-why', note));
        body.append(el('p', 'try-why', q.why), button('', t('landing.js.next'), function () { quiz(i + 1); }, 'try-next'));
      });
    }
    read();
  }

  // ---- Play: match the pairs against the clock ----
  function play(head, body, data) {
    var pairs = data.play.pairs;
    var clock = el('span', 'try-clock', '0.0 s');
    clock.dataset.testid = 'try-clock';
    var start = 0, tick = 0, picked = null, left = 0;
    function item(spec, side, i) {
      var b = button('try-opt', typeof spec === 'string' ? spec : spec.text || '', function () { tap(b, spec); }, side === 'term' ? 'try-match-term' : 'try-match-meaning');
      b.dataset.side = side; b.dataset.pair = i;
      if (spec.draw) { b.appendChild(spec.draw()); b.classList.add('picture'); b.setAttribute('aria-label', spec.alt); }
      return b;
    }
    function tap(b, spec) {
      if (spec.say) K.say(spec.say, 'es-ES');
      if (spec.sound) K.play(spec.sound);
      if (!start) { start = Date.now(); tick = setInterval(function () { clock.textContent = ((Date.now() - start) / 1000).toFixed(1) + ' s'; }, 100); }
      if (!picked || picked.dataset.side === b.dataset.side) {
        if (picked) picked.classList.remove('picked');
        picked = b; b.classList.add('picked');
        return;
      }
      var a = picked; picked = null; a.classList.remove('picked');
      if (a.dataset.pair === b.dataset.pair) {
        [a, b].forEach(function (x) { x.classList.add('right'); x.disabled = true; });
        if (--left === 0) {
          clearInterval(tick);
          var secs = ((Date.now() - start) / 1000).toFixed(1);
          later(function () { body.replaceChildren(finishCard(head, t('landing.js.play_done', { secs: secs }), t('landing.js.play_done_line'), setup)); }, 500);
        }
      } else {
        [a, b].forEach(function (x) { x.classList.add('wrong'); });
        later(function () { a.classList.remove('wrong'); b.classList.remove('wrong'); }, 450);
      }
    }
    function setup() {
      clearInterval(tick); start = 0; picked = null; left = pairs.length;
      clock.textContent = '0.0 s';
      head.replaceChildren(clock);
      var cols = [el('div', 'try-col'), el('div', 'try-col')];
      pairs.forEach(function (p, i) { cols[0].appendChild(item(p[0], 'term', i)); });
      data.play.order.forEach(function (i) { cols[1].appendChild(item(pairs[i][1], 'meaning', i)); });
      body.replaceChildren(el('p', 'try-label', t('landing.js.play_hint')), row('try-match', cols[0], cols[1]));
    }
    setup();
    return function () { clearInterval(tick); };
  }

  // ---- Guide: a card, a quick check, a flashcard, stars ----
  function guide(head, body, data) {
    var g = data.guide, right = 0;
    var dots = el('span', 'try-dots');
    function step(i) {
      head.replaceChildren(dots);
      dots.replaceChildren();
      for (var k = 0; k < 3; k++) dots.appendChild(el('i', k <= i ? 'on' : ''));
      body.replaceChildren();
      if (i === 0) {
        right = 0;
        body.append(el('p', 'try-label', t('landing.js.guide_card')), rich('try-text', g.tip));
        if (g.media) body.appendChild(g.media());
        body.appendChild(button('', t('landing.js.next'), function () { step(1); }, 'try-next'));
      } else if (i === 1) {
        ask(g.check, body, function (ok) {
          if (ok) right++;
          later(function () { step(2); }, ok ? 900 : 1600);
        });
      } else if (i === 2) {
        var card = button('try-flash', '', function () {
          card.classList.add('flipped');
          card.replaceChildren(document.createTextNode(g.back));
          card.disabled = true;
          if (g.onFlip) g.onFlip();
          body.appendChild(button('', t('landing.js.got_it'), function () { step(3); }, 'try-got-it'));
        }, 'try-flash');
        card.appendChild(g.front());
        body.append(el('p', 'try-label', t('landing.js.guide_flip')), card);
      } else {
        head.replaceChildren();
        body.appendChild(finishCard(head, right ? '★★★' : '★★☆', t('landing.js.guide_done'), function () { step(0); }));
      }
    }
    step(0);
  }

  var MODES = { sprint: sprint, study: study, play: play, guide: guide };
  var mode = 'sprint', subject = 'language', stop = null;
  function open() {
    clearTimers();
    if (stop) { stop(); stop = null; }
    if (window.speechSynthesis) speechSynthesis.cancel();
    tabs.forEach(function (b) { b.setAttribute('aria-selected', b.dataset.mode === mode ? 'true' : 'false'); });
    subjects.forEach(function (b) { b.setAttribute('aria-pressed', b.dataset.subject === subject ? 'true' : 'false'); });
    stage.dataset.mode = mode;
    stage.dataset.subject = subject;
    var head = el('div', 'try-head');
    var body = el('div', 'try-body');
    stage.replaceChildren(head, body);
    stop = MODES[mode](head, body, SAMPLES[subject]) || null;
  }
  tabs.forEach(function (b) { b.addEventListener('click', function () { mode = b.dataset.mode; open(); }); });
  subjects.forEach(function (b) { b.addEventListener('click', function () { subject = b.dataset.subject; open(); }); });
  open();
})();
