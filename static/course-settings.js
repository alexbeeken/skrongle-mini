// A course's settings: two dialogs, one script.
//
// The gear's settings (templates/course_settings.html, #565, #576) are one
// sheet: level, motivator, theme, picture and background, the badge in the
// header as the preview. The dialog and the card behind it wear the theme as
// it is picked (a fade, `.theming`), the level's note and the picture's name
// follow the pick, Save posts the form in place, the dialog closes and the
// card nods (`.accepted`). Closing without Save puts the saved settings back.
//
// Before a course is first opened (templates/course_wizard.html, #574) it is
// a wizard, one step a page, the arrows on either side sliding between them,
// previewing the theme on itself, and "Get Started!" under every step: what
// is not picked yet keeps its default (#896). Behind a card's Start button (`data-start`,
// #587) the button opens it, "Get Started!" posts the form, which opens the
// course with these settings, and closing it opens nothing. Over the home
// page of a course opened another way it opens on load, "Get Started!" saves
// and reloads the page in the new settings, and closing it keeps the settings
// as they are and counts the course as set up.
//
// Either way the pictures and backgrounds are redrawn in the theme picked
// (`#course-looks`: each choice's, profile::look_json). Without a script the
// form posts and the wizard's steps stack.
(function () {
  "use strict";
  var looks = {};
  try { looks = JSON.parse((document.getElementById("course-looks") || {}).textContent || "{}"); } catch (e) { looks = {}; }
  var SVG = "http://www.w3.org/2000/svg";
  function cssImage(url) {
    return url && !/[\s"'()\;<>]/.test(url) ? "url(" + url + ")" : "none";
  }
  function reducedMotion() {
    return window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }
  // `el` (a card, or a dialog itself) fades into the theme `value`; '' is
  // Default, which the page around it wears outside the course (#577).
  function wearOn(el, value) {
    if (!el) return;
    el.classList.add("theming");
    if (!value) el.removeAttribute("data-card-theme");
    else el.setAttribute("data-card-theme", value);
    clearTimeout(el._theming);
    el._theming = setTimeout(function () { el.classList.remove("theming"); }, 500);
  }
  // The picture `id` as a theme draws it (`themed`: {url} or none), in an
  // .avatar of `size`.
  function pictureEl(id, themed, size) {
    var span = document.createElement("span");
    span.className = "avatar" + (size ? " " + size : "") + (themed && themed.url ? " themed" : "");
    if (themed && themed.url) {
      var img = document.createElement("img");
      img.src = themed.url;
      img.alt = "";
      span.appendChild(img);
    } else {
      var svg = document.createElementNS(SVG, "svg");
      var use = document.createElementNS(SVG, "use");
      use.setAttribute("href", "#av-" + id);
      svg.appendChild(use);
      span.appendChild(svg);
    }
    return span;
  }
  function swatchEl(s) {
    var label = document.createElement("label");
    label.className = "color-pick";
    label.title = s.label;
    var radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "avatar_color";
    radio.value = String(s.slot);
    radio.dataset.testid = "courses-settings-color";
    radio.dataset.color = s.color;
    radio.dataset.image = s.image || "";
    radio.setAttribute("aria-label", s.label);
    var sw = document.createElement("span");
    sw.className = "swatch";
    sw.style.setProperty("--sw", s.color);
    sw.style.setProperty("--sw-img", cssImage(s.image));
    label.appendChild(radio);
    label.appendChild(sw);
    return label;
  }

  // What both dialogs share: reading the form, the (i) and close buttons,
  // the preview badge, and redrawing the pictures and backgrounds in a theme.
  function setup(dlg) {
    var form = dlg.querySelector("form[data-testid=courses-settings-form]");
    if (!form) return null;
    var colors = dlg.querySelector("[data-testid=courses-settings-colors]");
    var preview = dlg.querySelector(".cs-preview .avatar");
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

    // The preview badge follows each pick: its disc colour transitions in
    // CSS, and a new picture fades in over the old one.
    function showPreview() {
      if (!preview) return;
      var look = looks[value("theme", "")] || {};
      var id = value("avatar", "");
      var swatch = form.querySelector("input[name=avatar_color]:checked");
      if (swatch) {
        preview.style.setProperty("--av", swatch.dataset.color);
        preview.style.setProperty("--av-img", cssImage(swatch.dataset.image));
      }
      var pic = (look.pictures || {})[id];
      var fresh = pictureEl(id, pic, "").firstElementChild;
      preview.classList.toggle("themed", !!(pic && pic.url));
      var shown = Array.prototype.filter.call(preview.children, function (c) { return !c.dataset.leaving; });
      if (shown.length && shown.every(function (c) { return c.isEqualNode(fresh); })) return;
      preview.appendChild(fresh);
      if (!fresh.animate || reducedMotion()) { shown.forEach(function (c) { c.remove(); }); return; }
      var fade = { duration: 300, easing: "ease", fill: "forwards" };
      fresh.animate([{ opacity: 0 }, { opacity: 1 }], fade);
      shown.forEach(function (c) {
        c.dataset.leaving = "1";
        c.animate([{ opacity: 1 }, { opacity: 0 }], fade).finished.then(function () { c.remove(); });
      });
    }
    // The pictures and backgrounds in the theme picked: each picture in its
    // version, the theme's palette (a background it has not got shows the
    // first, as profile::shown_slot does).
    function restyle(theme) {
      var look = looks[theme];
      if (!look) return;
      dlg.querySelectorAll(".avatar-pick, .cs-pic").forEach(function (pick) {
        var id = pick.querySelector("input").value;
        var pic = (look.pictures || {})[id];
        var el = pictureEl(id, pic, "lg");
        el.style.setProperty("--av", pic ? pic.color : "");
        pick.querySelector(".avatar").replaceWith(el);
        pick.querySelector("[data-avatar-label]").textContent = pic ? pic.label : pick.dataset.label;
        if (pic && pic.url) pick.removeAttribute("title"); else pick.title = pick.dataset.shows;
      });
      if (colors) {
        var slot = value("avatar_color", "1");
        colors.textContent = "";
        (look.palette || []).forEach(function (s) { colors.appendChild(swatchEl(s)); });
        if (!check("avatar_color", slot)) {
          var first = colors.querySelector("input");
          if (first) first.checked = true;
        }
      }
    }
    var state = function () { return { level: value("start_unit", null), motivator: value("motivator", ""), theme: value("theme", ""), avatar: value("avatar", ""), color: value("avatar_color", "1") }; };
    var same = function (a, b) { return a.level === b.level && a.motivator === b.motivator && a.theme === b.theme && a.avatar === b.avatar && a.color === b.color; };
    // Puts the settings `s` back on the form and in the preview.
    var restore = function (s) {
      if (s.level !== null) check("start_unit", s.level);
      check("motivator", s.motivator);
      check("theme", s.theme);
      check("avatar", s.avatar);
      restyle(s.theme);
      check("avatar_color", s.color);
      showPreview();
    };
    return { form: form, value: value, check: check, state: state, same: same, restore: restore, showPreview: showPreview, restyle: restyle };
  }

  // Posts the settings `now` in place; `then` runs once they are saved, and a
  // failed request falls back to the form's own post.
  function save(s, now, then) {
    var button = s.form.querySelector("button[type=submit]");
    if (button) button.disabled = true;
    var body = new URLSearchParams({ motivator: now.motivator, theme: now.theme, avatar: now.avatar, avatar_color: now.color });
    if (now.level !== null) body.set("start_unit", now.level);
    // No theme row while themes are off (#1138): leave the course's theme as it is.
    if (!s.form.querySelector("input[name=theme]")) body.delete("theme");
    // No picture or background rows while character art is off (#1153): keep both.
    if (!s.form.querySelector("input[name=avatar]")) body.delete("avatar");
    if (!s.form.querySelector("input[name=avatar_color]")) body.delete("avatar_color");
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
    var pictureName = dlg.querySelector("[data-testid=courses-settings-picture-name]");
    var saved = s.state();
    // The note under the level ladder and the name under the pictures say
    // what is picked.
    function annotate() {
      var rung = s.form.querySelector(".cs-rung:has(input:checked)");
      if (levelNote && rung) levelNote.textContent = rung.dataset.note || "";
      var pic = s.form.querySelector(".cs-pic:has(input:checked) [data-avatar-label]");
      if (pictureName && pic) pictureName.textContent = pic.textContent;
    }
    function wear(theme) {
      wearOn(card, theme);
      wearOn(dlg, theme);
    }
    s.form.addEventListener("change", function (ev) {
      if (ev.target.name === "theme") {
        wear(ev.target.value);
        s.restyle(ev.target.value);
      }
      s.showPreview();
      annotate();
    });
    dlg.addEventListener("close", function () {
      // Closed without Save: what was saved comes back.
      var now = s.state();
      if (s.same(now, saved)) return;
      s.restore(saved);
      annotate();
      if (now.theme !== saved.theme) wear(saved.theme);
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
    var themed = card || dlg;
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

    s.form.addEventListener("change", function (ev) {
      if (ev.target.name === "theme") {
        wearOn(themed, ev.target.value);
        s.restyle(ev.target.value);
        fit();
      }
      s.showPreview();
    });
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
      if (now.theme !== saved.theme) wearOn(themed, saved.theme);
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
        // The home page draws the motivator's hero and wears the theme:
        // drawn again in the new settings.
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
