// The course home page (and the new-password gate, /password/change, and the shop, /shop): each
// (i) opens a modal with what it explains. The buttons carry commandfor/command, which browsers with
// invoker commands follow on their own (no script needed); this fills in
// for the ones without. A modal drawn with data-open-on-load (the shop's
// refusal, #569) opens as the page does; the server may draw it `open` so it
// shows without script too (#926), and this turns it into a real modal.
(function () {
  "use strict";
  document.querySelectorAll("dialog.modal[data-open-on-load]").forEach(function (d) {
    if (!d.showModal) return;
    if (d.open) d.removeAttribute("open");
    d.showModal();
  });
  if ("commandForElement" in HTMLButtonElement.prototype) return;
  document.querySelectorAll("[data-opens]").forEach(function (b) {
    var dialog = document.getElementById(b.dataset.opens);
    if (dialog) b.addEventListener("click", function () { if (!dialog.open) dialog.showModal(); });
  });
  document.querySelectorAll("dialog.modal [data-closes]").forEach(function (b) {
    b.addEventListener("click", function () { b.closest("dialog").close(); });
  });
})();
