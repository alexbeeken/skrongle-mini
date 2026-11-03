// Use AI's "What can the AI do?" (#1236, templates/ai_examples.html). A click
// on an example adds its text to the box: on a new line when the box already
// has text, else as the text. Then the dialog closes and the cursor goes to
// the end of the box. The (i) opens the dialog through courses.js's
// [data-open-dialog]. The form is drawn again after a save, so the click is
// caught on the document.
(function () {
  "use strict";
  document.addEventListener("click", function (ev) {
    var card = ev.target.closest && ev.target.closest("[data-ai-example]");
    var box = card && document.getElementById("ai-text");
    if (!box) return;
    var said = box.value.replace(/\s+$/, "");
    box.value = (said ? said + "\n" : "") + card.textContent.trim();
    box.dispatchEvent(new Event("input", { bubbles: true }));
    var dialog = card.closest("dialog");
    if (dialog && dialog.close) dialog.close();
    box.focus();
    box.setSelectionRange(box.value.length, box.value.length);
  });
})();
