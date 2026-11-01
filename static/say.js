// Tap a word to hear it. On any page of a language course (the mode bar
// carries data-say-lang), the words that matter can be tapped and are read
// aloud in the course's language by speech.js: vocabulary table cells, the
// bold words, italic examples and table cells of a lesson's text, flashcards,
// the correct answer in quiz feedback, and anything with a data-say
// attribute (the text to say, or its own text when empty). Honours the audio
// switch shared with Sprint mode (speech:audio in localStorage). A tapped word
// whose meaning the page knows also shows it in a small bubble, audio or not,
// since a phone never shows a title on hover.
(function () {
  "use strict";
  var S = window.LearnSpeech;
  if (!S || !S.supported) return;
  var bar = document.querySelector("[data-say-lang]");
  var lang = bar ? (bar.dataset.sayLang || "").trim() : "";
  if (!lang) return;
  var L = '[lang="' + lang + '"]';
  var SEL = [
    "[data-say]",
    ".vocab td" + L, ".vocab tr" + L + " td",
    ".prose" + L + " strong", ".prose" + L + " em", ".prose" + L + " td",
    ".flashcard .front", ".flashcard .back em",
    ".study-card .face-term .word", ".study-card .example",
    ".feedback strong" + L, ".feedback .dictation .w"
  ].join(", ");

  // The element's own words: its text without any speaker button or other
  // control nested in it (a flashcard front is "la mesa" plus a 🔊 button).
  function words(el) {
    var out = "";
    el.childNodes.forEach(function (n) {
      if (n.nodeType === 3) out += n.nodeValue;
      else if (n.nodeType === 1 && !n.matches("button, [data-say], svg")) out += words(n);
    });
    return out;
  }

  // What to say for an element: its data-say, else its text up to a
  // meaning separator ("rojo — red", "ser = to be", "alto (tall)").
  function textOf(el) {
    if (el.dataset.say) return el.dataset.say;
    var t = words(el).replace(/\s+/g, " ").trim();
    t = t.split(/\s[—–=→]\s|\s\(/)[0];
    return t.replace(/^[*_"'«(]+|[*_"'»)]+$/g, "").trim();
  }

  // Bold words in a lesson's text are a mix: Spanish key words ("muy") and
  // English grammar terms ("Description"). Only the ones the page knows as
  // the course's language are tappable: every word in a vocabulary or
  // paradigm table cell of the page, or with a letter outside plain ASCII.
  var known = null;
  function knownWords() {
    if (known) return known;
    known = {};
    document.querySelectorAll(".vocab td" + L + ", .vocab tr" + L + " td, .prose" + L + " td").forEach(function (c) {
      textOf(c).toLowerCase().split(/[\s\/,;()]+/).forEach(function (w) { w = w.replace(/^[^a-zà-ÿ]+|[^a-zà-ÿ]+$/g, ""); if (w.length > 1) known[w] = true; });
    });
    return known;
  }
  function eligible(el) {
    if (!el.matches(".prose" + L + " strong")) return true;
    var words = textOf(el).toLowerCase().split(/\s+/);
    var k = knownWords();
    return words.length > 0 && words.every(function (w) {
      w = w.replace(/^[^a-zà-ÿ]+|[^a-zà-ÿ]+$/g, "");
      return !w || k[w] || /[^\x00-\x7f]/.test(w);
    });
  }

  // The meaning of a tapped word, if the page has one: the element's own
  // data-meaning or title (not the "Tap to hear it" hint), else the meaning
  // column of its vocabulary row, else (a bold word in a lesson's text) the
  // meaning of the same term in the page's vocabulary table.
  var glossary = null;
  function meaningCell(row) {
    var cells = row.querySelectorAll("td[lang]");
    for (var i = 0; i < cells.length; i++) if (cells[i].getAttribute("lang") !== lang) return cells[i];
    return null;
  }
  function lookup(term) {
    if (!glossary) {
      glossary = {};
      document.querySelectorAll(".vocab tr").forEach(function (row) {
        var t = row.querySelector("td" + L), m = meaningCell(row);
        if (!t || !m) return;
        var k = textOf(t).toLowerCase(), v = words(m).replace(/\s+/g, " ").trim();
        if (k && v && !glossary[k]) glossary[k] = v;
      });
    }
    return glossary[term.toLowerCase()] || "";
  }
  function meaningOf(el) {
    if (el.dataset.meaning) return el.dataset.meaning;
    if (el.title && !el.hasAttribute("data-say-hint")) return el.title;
    var row = el.closest(".vocab tr");
    if (row) {
      var m = meaningCell(row);
      return m && m !== el ? words(m).replace(/\s+/g, " ").trim() : "";
    }
    if (el.matches(".prose" + L + " strong")) return lookup(textOf(el));
    return "";
  }

  // One bubble for the page, under the tapped word (above it near the bottom),
  // kept inside the screen; it goes after a few seconds, on scroll, or on the next tap.
  var bubble = null, bubbleTimer = 0;
  function hideMeaning() {
    clearTimeout(bubbleTimer);
    if (bubble) bubble.hidden = true;
  }
  function showMeaning(el) {
    var text = meaningOf(el);
    if (!text) { hideMeaning(); return; }
    if (!bubble) {
      bubble = document.createElement("div");
      bubble.className = "say-meaning";
      bubble.setAttribute("role", "status");
      bubble.setAttribute("aria-label", t('common.js.meaning'));
      bubble.dataset.testid = "say-meaning";
      document.body.appendChild(bubble);
    }
    bubble.textContent = text;
    bubble.hidden = false;
    bubble.style.left = "0px"; bubble.style.top = "0px";
    var r = el.getBoundingClientRect(), b = bubble.getBoundingClientRect();
    var vw = document.documentElement.clientWidth, vh = window.innerHeight;
    var left = Math.max(8, Math.min(r.left + r.width / 2 - b.width / 2, vw - b.width - 8));
    var top = r.bottom + 6;
    if (top + b.height > vh - 8 && r.top - b.height - 6 > 8) top = r.top - b.height - 6;
    bubble.style.left = left + "px"; bubble.style.top = top + "px";
    clearTimeout(bubbleTimer);
    bubbleTimer = setTimeout(hideMeaning, 4000);
  }
  window.addEventListener("scroll", hideMeaning, { passive: true });

  var speaking = null;
  function sayEl(el) {
    var text = textOf(el);
    if (!text) return;
    if (speaking) speaking.classList.remove("saying");
    speaking = el;
    S.say(text, lang, {
      force: el.hasAttribute("data-say"), // an explicit speaker button always speaks
      onstart: function () { el.classList.add("saying"); },
      onend: function () { el.classList.remove("saying"); if (speaking === el) speaking = null; }
    });
  }

  document.addEventListener("click", function (ev) {
    var el = ev.target.closest(SEL);
    if (!el) { hideMeaning(); return; }
    // Explicit speaker buttons speak and nothing else (a flashcard must not flip).
    if (el.hasAttribute("data-say")) { ev.preventDefault(); ev.stopPropagation(); sayEl(el); return; }
    // Plain words: not when the tap was really for a link, a control or a selection.
    if (ev.target.closest("a, button, input, select, textarea, label, summary")) return;
    var sel = window.getSelection && window.getSelection();
    if (sel && String(sel).length) return;
    if (!eligible(el)) return;
    showMeaning(el);
    sayEl(el);
  }, true);

  // A keyboard reaches the words too: Enter or Space on a focused word says it.
  document.addEventListener("keydown", function (ev) {
    if (ev.key !== "Enter" && ev.key !== " ") return;
    var el = ev.target;
    if (!el.classList || !el.classList.contains("can-say") || el.hasAttribute("data-say")) return;
    ev.preventDefault();
    showMeaning(el);
    sayEl(el);
  });

  // Mark what can be tapped (underline, cursor, hint, a tab stop), now and as pages add content.
  function mark(root) {
    (root.querySelectorAll ? root.querySelectorAll(SEL) : []).forEach(function (el) {
      if (el.classList.contains("can-say") || !eligible(el)) return;
      el.classList.add("can-say");
      if (el.hasAttribute("data-say")) return;
      if (!el.title) { el.title = t('common.js.tap_to_hear'); el.setAttribute("data-say-hint", ""); }
      if (!el.hasAttribute("tabindex")) el.tabIndex = 0;
    });
  }
  mark(document);
  if (window.MutationObserver) {
    new MutationObserver(function (muts) {
      muts.forEach(function (m) { m.addedNodes.forEach(function (n) { if (n.nodeType === 1) mark(n); }); });
    }).observe(document.body, { childList: true, subtree: true });
  }
})();
