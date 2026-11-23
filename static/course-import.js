// Courses: "Add a course from a file". The chosen file's text goes into the
// box the form posts; the form works without this script by pasting into
// the box (the file is never uploaded as a file).
(function () {
  "use strict";
  var form = document.querySelector('[data-testid="courses-import-form"]');
  if (!form) return;
  var pick = form.querySelector("[data-import-pick]");
  var file = pick.querySelector("input");
  var text = form.querySelector("textarea");
  pick.hidden = false;
  file.addEventListener("change", function () {
    var f = file.files[0];
    if (f) f.text().then(function (t) { text.value = t; });
  });
})();
