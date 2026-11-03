// A course's settings: two dialogs, one script.
//
// The gear's settings (templates/course_settings.html, #565, #576) are one
// sheet: level and motivator. The level's note follows the pick, Save posts
// the form in place, the dialog closes and the card nods (`.accepted`).
// Closing without Save puts the saved settings back.
//
// Before a course is first opened (templates/course_wizard.html, #574) it is
// a wizard, one step a page, the arrows on either side sliding between them,
// and "Get Started!" under every step: what is not picked yet keeps its
// default (#896). Behind a card's Start button (`data-start`, #587) the
// button opens it, "Get Started!" posts the form, which opens the course with
// these settings, and closing it opens nothing. Over the home page of a
// course opened another way it opens on load, "Get Started!" saves and
// reloads the page in the new settings, and closing it keeps the settings as
// they are and counts the course as set up.
//
// Without a script the form posts and the wizard's steps stack.
(function () {
  "use strict";

  // What both dialogs share: reading the form, the (i) and close buttons.
  function setup(dlg) {
    var form = dlg.querySelector("form[data-testid=courses-settings-form]");
    if (!form) return null;
    var value = function (name, fallback) { var r = form.querySelector("input[name=" + name + "]:checked"); return r ? r.value : fallback; };
    var check = function (name, v) { var r = form.querySelector("input[name=" + name + '][value="' + CSS.escape(v) + '"]'); if (r) r.checked = true; return !!r; };

    // The (i) buttons open their dialogs where the browser has no invoker
    // commands, and their OK closes them.
    if (!("commandForElement" in HTMLButtonElement.prototype)) {
      dlg.querySelectorAll("[data-opens]").forEach(function (b) {
        var info = document.getElementById(b.dataset.opens);
        if (info) b.addEventListener("click", function () { if (!info.open) info.showModal(); });
      });
    }
    dlg.querySelectorAll("dialog [data-closes]").forEach(function (b) {
      b.addEventListener("click", function () { b.closest("dialog").close(); });
    });
    var x = dlg.querySelector("[data-testid=courses-settings-close]");
    if (x) x.addEventListener("click", function (ev) { ev.preventDefault(); dlg.close(); });

    var state = function () { return { level: value("start_unit", null), motivator: value("motivator", "") }; };
    var same = function (a, b) { return a.level === b.level && a.motivator === b.motivator; };
    // Puts the settings `s` back on the form.
    var restore = function (s) {
      if (s.level !== null) check("start_unit", s.level);
      check("motivator", s.motivator);
    };
    return { form: form, value: value, check: check, state: state, same: same, restore: restore };
  }

  // Posts the settings `now` in place; `then` runs once they are saved, and a
  // failed request falls back to the form's own post.
  function save(s, now, then) {
    var button = s.form.querySelector("button[type=submit]");
    if (button) button.disabled = true;
    var body = new URLSearchParams({ motivator: now.motivator });
    if (now.level !== null) body.set("start_unit", now.level);
    return fetch(s.form.action, { method: "POST", body: body, credentials: "same-origin" })
      .then(function (r) { if (!r.ok) throw new Error(r.status); then(); })
      .catch(function () { s.form.submit(); })
      .then(function () { if (button) button.disabled = false; });
  }

  // The gear's settings: one sheet.
  function initForm(dlg) {
    var s = setup(dlg);
    if (!s) return;
    var card = dlg.closest(".course-card");
    var levelNote = dlg.querySelector("[data-testid=courses-settings-level-note]");
    var saved = s.state();
    // The note under the level ladder says what is picked.
    function annotate() {
      var rung = s.form.querySelector(".cs-rung:has(input:checked)");
      if (levelNote && rung) levelNote.textContent = rung.dataset.note || "";
    }
    s.form.addEventListener("change", annotate);
    dlg.addEventListener("close", function () {
      // Closed without Save: what was saved comes back.
      var now = s.state();
      if (s.same(now, saved)) return;
      s.restore(saved);
      annotate();
    });
    s.form.addEventListener("submit", function (ev) {
      if (!window.fetch) return;
      ev.preventDefault();
      var now = s.state();
      save(s, now, function () {
        saved = now;
        dlg.close();
        if (!card) return;
        card.classList.remove("accepted");
        void card.offsetWidth; // restart the nod on a second save
        card.classList.add("accepted");
      });
    });
  }

  // The first visit's wizard: the steps side by side, one showing.
  function initWizard(dlg) {
    var s = setup(dlg);
    if (!s) return;
    var start = dlg.hasAttribute("data-start");
    var card = dlg.closest(".course-card");
    var viewport = dlg.querySelector(".cs-viewport");
    var track = dlg.querySelector(".cs-track");
    var steps = dlg.querySelectorAll(".cs-step");
    var prev = dlg.querySelector(".cs-arrow.prev");
    var next = dlg.querySelector(".cs-arrow.next");
    var dots = dlg.querySelectorAll(".cs-dots span");
    if (!track || !steps.length) return;
    var saved = s.state();
    var submitted = false;
    var at = 0;
    dlg.classList.add("cs-on");
    dlg.querySelector(".cs-dots").hidden = false;
    var fit = function () { viewport.style.height = steps[at].offsetHeight + "px"; };
    var go = function (i) {
      at = Math.max(0, Math.min(steps.length - 1, i));
      track.style.setProperty("--cs-step", at);
      steps.forEach(function (st, j) { st.classList.toggle("here", j === at); });
      dots.forEach(function (d, j) { d.classList.toggle("here", j === at); });
      prev.hidden = at === 0;
      next.hidden = at === steps.length - 1;
      fit();
      dlg.dataset.step = String(at);
    };
    // A step that changes height (the placement test, static/placement.js) asks to be fitted again.
    dlg.addEventListener("cs-fit", fit);
    prev.addEventListener("click", function () { go(at - 1); });
    next.addEventListener("click", function () { go(at + 1); });
    if (start) {
      // The card's Start button opens the wizard instead of the course.
      var open = card && card.querySelector("[data-testid=courses-open]");
      if (open) open.addEventListener("click", function (ev) {
        if (!dlg.showModal) return;
        ev.preventDefault();
        if (!dlg.open) dlg.showModal();
        go(0);
      });
    } else if (dlg.hasAttribute("data-open-on-load") && dlg.showModal && !dlg.open) {
      dlg.showModal();
    }
    go(0);

    dlg.addEventListener("dialog-open", function () { go(0); });
    dlg.addEventListener("close", function () {
      // The wizard closed without "Get Started!": the course keeps the
      // settings it started with and is set up, so it does not ask again.
      if (!start && !submitted) {
        var body = new URLSearchParams({ next: "home" });
        fetch(s.form.action, { method: "POST", body: body, credentials: "same-origin", keepalive: true });
      }
      var now = s.state();
      if (s.same(now, saved)) return;
      s.restore(saved);
    });
    s.form.addEventListener("submit", function (ev) {
      // From a Start button the form itself opens the course.
      if (start) { submitted = true; return; }
      if (!window.fetch) return;
      ev.preventDefault();
      var now = s.state();
      submitted = true; // saved in place, or posted by the form itself
      save(s, now, function () {
        saved = now;
        // The home page draws the motivator's hero: drawn again in the new settings.
        window.location.reload();
      });
    });
  }

  document.querySelectorAll("[data-testid=courses-settings-dialog]").forEach(function (dlg) {
    if (dlg.hasAttribute("data-wizard")) initWizard(dlg); else initForm(dlg);
  });
  document.addEventListener("animationend", function (ev) {
    if (ev.animationName === "card-accepted") ev.target.classList.remove("accepted");
  });
})();
