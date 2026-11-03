// Music notation (#1176, #1218). A ```abc fenced block in a lesson's Markdown
// arrives as <pre><code class="language-abc">; a quiz item's `abc` field as
// <pre class="abc-staff"> above its prompt. With abcjs loaded
// (vendor/abcjs-basic-min.js, only on pages with music to draw), each is
// drawn as an SVG staff in its place. If drawing fails, or abcjs finds
// something it can't read or no notes at all (#1187), the ABC text stays on
// the page as it was. Pages that build questions in script (Sprint mode,
// the lesson editor's preview) call window.drawStaff(box, abc); a staff
// the learner taps (wrong-note.js) passes abcjs options and gets the drawn
// tune back as box.abcTune.
(function () {
  "use strict";
  // Draw `abc` into `box` (already on the page); false if it can't be drawn.
  function render(box, abc, question, opts) {
    if (!window.ABCJS) return false;
    box.classList.add("notation");
    box.setAttribute("data-testid", "notation");
    // Here, not in style.css: only these pages need it. A question's staff
    // holds a few notes, so it is narrower than a lesson's.
    box.style.cssText = question ? "margin: 0 auto .75em; max-width: 18rem" : "margin: 1em 0; max-width: 32rem";
    try {
      // abcjs keeps the aspect ratio with a padding-bottom percentage, which
      // CSS takes from the parent's width: an inner box makes that parent the
      // capped staff box, so a wide page gets no blank gap below the staff.
      var inner = box.appendChild(document.createElement("div"));
      var params = { responsive: "resize", staffwidth: question ? 220 : 400, add_classes: true, ariaLabel: t("common.js.notation_label") };
      var emptyOk = opts && opts.emptyOk;
      Object.keys(opts || {}).forEach(function (k) { if (k !== "emptyOk") params[k] = opts[k]; });
      var tunes = window.ABCJS.renderAbc(inner, abc, params);
      var svg = box.querySelector("svg");
      // abcjs draws whatever it can parse: stray notes for text that isn't
      // ABC, an empty staff for an empty block. Either way, keep the text
      // (unless the caller wants the empty staff: chord-spell before any tap).
      var warned = tunes && tunes.some(function (tune) { return tune.warnings && tune.warnings.length; });
      if (!tunes || !tunes.length || !svg || warned || (!emptyOk && !box.querySelector(".abcjs-note"))) throw new Error("no tune");
      svg.setAttribute("role", "img");
      var title = svg.querySelector("title");
      if (title) title.textContent = t("common.js.notation_label");
      box.abcTune = tunes[0];
      return true;
    } catch (e) {
      box.innerHTML = "";
      box.classList.remove("notation");
      box.removeAttribute("data-testid");
      box.style.cssText = "";
      return false;
    }
  }
  // A question's ABC text that can't be drawn wraps in its card instead of
  // running past its edge (#1311).
  var WRAP = "white-space: pre-wrap; overflow-wrap: anywhere";
  // A staff for a question built in script: drawn into `box`, or the ABC
  // text when it can't be drawn.
  window.drawStaff = function (box, abc, opts) {
    if (!render(box, abc, true, opts)) {
      var pre = document.createElement("pre");
      pre.textContent = abc;
      pre.style.cssText = WRAP;
      box.appendChild(pre);
    }
    return box;
  };
  // The right answer to a question whose options are staves, drawn as a
  // staff at the end of the feedback line `line` instead of its ABC text
  // (#1308). `form` holds the options; `answer` is the right option's value.
  // False when that option is not a staff (the line keeps its text).
  window.answerStaff = function (line, form, answer) {
    var pre = null;
    form.querySelectorAll(".option").forEach(function (o) {
      var input = o.querySelector("input");
      if (input && input.value === answer) pre = o.querySelector("pre.abc-option");
    });
    if (!pre || !line) return false;
    var strong = line.querySelector("strong");
    if (strong) strong.remove();
    var box = window.drawStaff(line.appendChild(document.createElement("div")), pre.textContent);
    box.dataset.testid = "answer-staff";
    return true;
  };
  function drawBlock(node) {
    var question = node.tagName === "PRE";
    var pre = question ? node : node.parentNode;
    var box = document.createElement("div");
    pre.parentNode.insertBefore(box, pre);
    if (render(box, node.textContent, question)) pre.hidden = true;
    else {
      box.remove();
      if (question) pre.style.cssText = WRAP;
    }
  }
  // abcjs measures text as it draws, which needs a visible page element, so
  // a block inside a hidden question or card is drawn once it shows.
  function draw() {
    var nodes = document.querySelectorAll(".prose pre > code.language-abc, pre.abc-staff");
    if (!window.IntersectionObserver) return nodes.forEach(drawBlock);
    var seen = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting && !e.target.getClientRects().length) return;
        seen.unobserve(e.target);
        drawBlock(e.target.classList.contains("abc-staff") ? e.target : e.target.querySelector("code"));
      });
    });
    nodes.forEach(function (node) { seen.observe(node.tagName === "PRE" ? node : node.parentNode); });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", draw);
  else draw();
})();
