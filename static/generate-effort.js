// Effort beside Generate (#918, generate_form.html): the button names the
// level picked in its dialog. Listens on the document, so a Generate dialog
// drawn again after a save (courses.js refreshAi) is covered too.
(function () {
  document.addEventListener("change", function (ev) {
    var r = ev.target;
    if (!r || r.name !== "effort" || !r.checked || !r.closest("#gen-form")) return;
    var open = document.getElementById("gen-effort-open");
    if (open && r.dataset.button) open.textContent = r.dataset.button;
  });
})();
