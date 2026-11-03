// Lesson conversations (#1264): each line's meaning starts hidden. Tapping a
// bubble reads its line aloud in the course's language (speech.js, so the
// audio switch counts) and shows the meaning; tapping again hides it and
// reads the line again. Without this script every meaning stays on show.
(function () {
  "use strict";
  document.querySelectorAll(".conv-bubble").forEach(function (b) {
    var m = b.querySelector(".conv-meaning"), line = b.querySelector(".conv-text");
    if (m) { m.hidden = true; b.setAttribute("aria-expanded", "false"); }
    b.addEventListener("click", function () {
      if (m) { m.hidden = !m.hidden; b.setAttribute("aria-expanded", String(!m.hidden)); }
      var S = window.LearnSpeech;
      if (S && S.supported && line) S.say(line.textContent, line.getAttribute("lang") || "");
    });
  });
})();
