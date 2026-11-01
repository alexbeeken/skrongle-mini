// Modals close on a click outside them (#226), on every page built on
// base.html.
//
// - A modal <dialog> (opened with showModal) closes when a click starts and
//   ends on its backdrop, as Close or Cancel would.
// - A server-drawn overlay (<div class="modal-backdrop"> around a .modal:
//   the AI job box once its job has ended, Review AI changes) follows its
//   [data-modal-close] link when its backdrop is clicked. One without such a
//   link (a job still running) stays.
//
// A modal stays put when it, or its .modal-backdrop, has data-modal-static,
// or while a field of a form in it holds something typed or picked that is
// not sent yet (its value differs from the one it was drawn with): a stray
// click must not throw input away. Escape, Close and Cancel work as before.
//
// A form with data-confirm asks its text with confirm() before it is sent
// (#387: the text is an attribute, not JavaScript inside an inline handler).
//
// A masked word (templates/masked_word.html, #1033 review) shows its word
// while it is held down: mouse, finger, or Space/Enter on it. Letting go
// hides it again. [data-show-dialog=<id>] opens that <dialog>, and a list
// with data-pager="<n>" shows n items at a time behind its Previous/Next
// buttons (templates/flag_matches.html).
(function () {
  "use strict";

  function changed(root) {
    var fields = root.querySelectorAll("input, textarea, select");
    for (var i = 0; i < fields.length; i++) {
      var f = fields[i];
      // Only what a form would send: a field that saves itself (the
      // profile's picture) or one outside any form loses nothing.
      if (!f.form || (f.form.getAttribute("method") || "").toLowerCase() === "dialog") continue;
      if (f.disabled || f.readOnly) continue;
      var t = (f.type || "").toLowerCase();
      if (t === "hidden" || t === "submit" || t === "button" || t === "reset") continue;
      if (t === "checkbox" || t === "radio") { if (f.checked !== f.defaultChecked) return true; continue; }
      if (t === "file") { if (f.files && f.files.length) return true; continue; }
      if (f.tagName === "SELECT") {
        for (var j = 0; j < f.options.length; j++) if (f.options[j].selected !== f.options[j].defaultSelected) return true;
        continue;
      }
      if (f.value !== f.defaultValue) return true;
    }
    return false;
  }
  function stays(modal) {
    return modal.hasAttribute("data-modal-static") || changed(modal);
  }
  function isModal(d) {
    try { return d.matches(":modal"); } catch (e) { return d.open; }
  }
  // A click on a dialog's ::backdrop lands on the dialog itself, at a point
  // outside its box.
  function onBackdrop(d, ev) {
    if (ev.target !== d) return false;
    var r = d.getBoundingClientRect();
    return ev.clientX < r.left || ev.clientX > r.right || ev.clientY < r.top || ev.clientY > r.bottom;
  }
  function isOverlay(t) {
    return !!(t && t.classList && t.classList.contains("modal-backdrop"));
  }

  // Where the press started: a drag from inside the modal (selecting text)
  // let go outside it does not count.
  var downOn = null;
  document.addEventListener("pointerdown", function (ev) {
    var t = ev.target;
    if (t && t.tagName === "DIALOG") downOn = onBackdrop(t, ev) ? t : null;
    else downOn = isOverlay(t) ? t : null;
  }, true);

  document.addEventListener("click", function (ev) {
    var t = ev.target, was = downOn;
    downOn = null;
    if (!t || t !== was || ev.button) return;
    if (t.tagName === "DIALOG") {
      if (t.open && isModal(t) && onBackdrop(t, ev) && !stays(t)) t.close();
      return;
    }
    if (isOverlay(t) && !stays(t)) {
      var close = t.querySelector("[data-modal-close]");
      if (close) close.click();
    }
  });

  document.addEventListener("submit", function (ev) {
    var f = ev.target;
    if (!f || !f.hasAttribute || !f.hasAttribute("data-confirm")) return;
    if (!window.confirm(f.getAttribute("data-confirm"))) {
      ev.preventDefault();
      ev.stopImmediatePropagation();
    }
  }, true);

  var held = null;
  function show(el) {
    if (held && held !== el) hide();
    if (!el.hasAttribute("data-mask")) el.setAttribute("data-mask", el.textContent);
    el.textContent = el.getAttribute("data-word");
    el.classList.add("revealed");
    held = el;
  }
  function hide() {
    if (!held) return;
    held.textContent = held.getAttribute("data-mask");
    held.classList.remove("revealed");
    held = null;
  }
  function masked(ev) {
    return ev.target && ev.target.closest ? ev.target.closest(".masked-word") : null;
  }
  document.addEventListener("mousedown", function (ev) {
    var el = masked(ev);
    if (el && ev.button === 0) { ev.preventDefault(); show(el); }
  });
  document.addEventListener("touchstart", function (ev) {
    var el = masked(ev);
    if (el) show(el);
  }, { passive: true });
  document.addEventListener("keydown", function (ev) {
    var el = masked(ev);
    if (el && (ev.key === " " || ev.key === "Enter")) { ev.preventDefault(); show(el); }
  });
  ["mouseup", "touchend", "touchcancel", "keyup", "dragend"].forEach(function (name) {
    document.addEventListener(name, hide);
  });
  document.addEventListener("mouseout", function (ev) {
    if (held && masked(ev) === held && !held.contains(ev.relatedTarget)) hide();
  });
  document.addEventListener("focusout", hide);

  function page(list, to) {
    var size = parseInt(list.getAttribute("data-pager"), 10) || 10;
    var items = list.children, pages = Math.max(1, Math.ceil(items.length / size));
    to = Math.min(Math.max(0, to), pages - 1);
    list.setAttribute("data-page", to);
    for (var i = 0; i < items.length; i++) items[i].hidden = Math.floor(i / size) !== to;
    var box = list.parentNode, st = box.querySelector("[data-pager-status]");
    if (st) st.textContent = st.getAttribute("data-label").replace("{n}", to + 1).replace("{total}", pages);
    var prev = box.querySelector("[data-pager-prev]"), next = box.querySelector("[data-pager-next]");
    if (prev) prev.disabled = to === 0;
    if (next) next.disabled = to === pages - 1;
  }
  document.querySelectorAll("[data-pager]").forEach(function (list) { page(list, 0); });
  document.addEventListener("click", function (ev) {
    var t = ev.target && ev.target.closest ? ev.target : null;
    if (!t) return;
    var open = t.closest("[data-show-dialog]");
    if (open) {
      var d = document.getElementById(open.getAttribute("data-show-dialog"));
      if (d && !d.open) d.showModal();
      return;
    }
    var step = t.closest("[data-pager-prev]") ? -1 : t.closest("[data-pager-next]") ? 1 : 0;
    if (!step) return;
    var list = (t.closest("dialog") || document).querySelector("[data-pager]");
    if (list) page(list, (parseInt(list.getAttribute("data-page"), 10) || 0) + step);
  });
})();
