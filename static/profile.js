// Profile page. The names, the AI API keys and the Dev options save as they
// change (static/autosave.js, POST /api/profile); the circle by the heading
// says whether everything is saved, and a green check on a field that its
// change is.
//
// Dialogs (password, terms, privacy): a [data-opens] button or link opens the
// <dialog> it names; [data-closes] or Escape closes it. The password dialog is
// a plain form: closing it puts the form back as it was, and one the server
// re-rendered with an error ([data-open-on-load]) opens straight away.
(function () {
  "use strict";
  var base = document.body.dataset.base || "";

  // "Do Not Show Me AI Features", the AI row of the Privacy and consent
  // card (#974): the switch posts in place (POST /profile/ai, JSON) and
  // slides the AI-only sections ([data-ai-only]: the API keys card and its
  // nav link) closed or open, instead of reloading the page. On hides them;
  // hiding deletes the saved keys, so with a key on file it asks first in
  // the confirm dialog; the rows then show as empty. Without JS the switch
  // is a form that posts and reloads.
  var ai = document.querySelector("[data-testid=consent-ai]");
  var aiSwitch = ai && ai.querySelector("[data-testid=consent-ai-switch]");
  if (aiSwitch) (function () {
    var confirmDialog = document.getElementById("ai-off-dialog");
    var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    var busy = false;
    aiSwitch.removeAttribute("data-opens"); // handled here, not by the generic opener below

    function keysOnFile() { return !!document.querySelector("[data-testid=profile-api-key][data-key-set=true]"); }
    function hidden() { return ai.dataset.aiOff === "true"; }

    function edges(el, cs, open) {
      var h = open ? el.offsetHeight + "px" : "0px";
      return {
        height: h, opacity: open ? 1 : 0,
        marginTop: open ? cs.marginTop : "0px", marginBottom: open ? cs.marginBottom : "0px",
        paddingTop: open ? cs.paddingTop : "0px", paddingBottom: open ? cs.paddingBottom : "0px",
        borderTopWidth: open ? cs.borderTopWidth : "0px", borderBottomWidth: open ? cs.borderBottomWidth : "0px"
      };
    }
    function slide(el, show) {
      if (show === !el.hidden) return;
      if (reduce || !el.animate) { el.hidden = !show; return; }
      if (show) el.hidden = false;
      var cs = getComputedStyle(el);
      var open = edges(el, cs, true), shut = edges(el, cs, false);
      el.style.overflow = "hidden";
      var a = el.animate(show ? [shut, open] : [open, shut], { duration: 280, easing: "ease" });
      a.onfinish = function () { if (!show) el.hidden = true; el.style.overflow = ""; };
    }

    function emptyKeyRows() {
      document.querySelectorAll("[data-testid=profile-api-key]").forEach(function (row) {
        row.dataset.keySet = "false";
        row.dataset.unreadable = "false";
        row.open = false;
        var status = row.querySelector("[data-testid=profile-api-key-status]");
        if (status) { status.textContent = ""; var none = document.createElement("span"); none.className = "muted"; none.textContent = t("profile.js.no_key"); status.appendChild(none); }
        var prompt = row.querySelector("[data-key-prompt]");
        if (prompt) prompt.textContent = t("profile.js.paste_key");
        var remove = row.querySelector(".key-remove");
        if (remove) remove.remove();
        var input = row.querySelector("input");
        if (input) input.value = "";
      });
    }

    function render(off, consentText) {
      ai.dataset.aiOff = off ? "true" : "false";
      aiSwitch.setAttribute("aria-checked", off ? "true" : "false");
      aiSwitch.lastChild.nodeValue = off ? t("profile.js.on") : t("profile.js.off");
      var state = ai.querySelector("[data-testid=consent-ai-state]");
      if (state && consentText) state.textContent = consentText;
      var form = ai.querySelector("[data-testid=consent-ai-form] input[name=off]");
      if (form) form.value = off ? "0" : "1";
      if (off) { var key = ai.querySelector("[data-testid=consent-ai-key]"); if (key) key.remove(); }
      document.querySelectorAll("[data-ai-only]").forEach(function (el) { slide(el, !off); });
      if (off) emptyKeyRows();
    }

    function apply(off) {
      if (busy) return;
      busy = true;
      aiSwitch.setAttribute("aria-busy", "true");
      fetch(base + "/profile/ai", {
        method: "POST",
        credentials: "same-origin",
        headers: { accept: "application/json", "content-type": "application/x-www-form-urlencoded" },
        body: "off=" + (off ? "1" : "0")
      }).then(function (r) {
        if (!r.ok) throw new Error("HTTP " + r.status);
        return r.json();
      }).then(function (d) {
        render(!!d.ai_off, d.consent);
      }).catch(function () {
        location.href = base + "/profile#consent"; // the server-rendered state
      }).then(function () {
        busy = false;
        aiSwitch.removeAttribute("aria-busy");
      });
    }

    function toggle(e) {
      e.preventDefault();
      if (busy) return;
      if (hidden()) apply(false);
      else if (confirmDialog && keysOnFile()) confirmDialog.showModal();
      else apply(true);
    }
    aiSwitch.addEventListener("click", toggle);
    if (confirmDialog) confirmDialog.querySelector("form").addEventListener("submit", function (e) {
      e.preventDefault();
      confirmDialog.close();
      apply(true);
    });
  })();

  function openDialog(dialog) { dialog.showModal(); }
  function closeDialog(dialog) { dialog.close(); }

  document.querySelectorAll("[data-opens]").forEach(function (b) {
    var dialog = document.getElementById(b.dataset.opens);
    if (dialog) b.addEventListener("click", function (e) {
      // A link still goes to its page in a new tab (ctrl/cmd/middle click) or without JS.
      if (e.ctrlKey || e.metaKey || e.shiftKey || e.button) return;
      e.preventDefault();
      openDialog(dialog);
    });
  });
  document.querySelectorAll("dialog.modal").forEach(function (dialog) {
    var form = dialog.querySelector("form");
    dialog.querySelectorAll("[data-closes]").forEach(function (b) {
      b.addEventListener("click", function () { closeDialog(dialog); });
    });
    dialog.addEventListener("close", function () {
      if (form) form.reset();
      var error = dialog.querySelector("[role=alert]");
      if (error) error.remove();
    });
    // One rendered [open] (so it shows without JS) is made modal instead.
    if (dialog.hasAttribute("data-open-on-load")) { dialog.removeAttribute("open"); dialog.showModal(); }
  });

  // The privacy dialog shows the name other learners see, as it is typed:
  // the same rule as profile::public_name ("Ana L.", "Ana", or "Learner").
  var shown = document.querySelector("[data-testid=profile-public-name]");
  var first = document.querySelector("[data-testid=profile-first-name]");
  var last = document.querySelector("[data-testid=profile-last-name]");
  if (shown && first && last) {
    var update = function () {
      var f = first.value.trim(), l = last.value.trim();
      shown.textContent = !f ? shown.dataset.noName : l ? f + " " + l.charAt(0).toUpperCase() + "." : f;
    };
    first.addEventListener("input", update);
    last.addEventListener("input", update);
  }

  // The two new passwords must match before the form is sent.
  var pw = document.querySelector("[data-testid=profile-password-form]");
  if (pw) {
    var fresh = pw.elements["new"], again = pw.elements.confirm;
    var check = function () {
      again.setCustomValidity(again.value && again.value !== fresh.value ? t("profile.js.password_mismatch") : "");
    };
    fresh.addEventListener("input", check);
    again.addEventListener("input", check);
  }

  // ---- save as you edit ----

  var heading = document.querySelector("[data-testid=profile-display-name]");
  var toggle = document.querySelector("[data-testid=nav-account-toggle]");
  var menuName = document.querySelector(".account-menu .menu-head strong");

  var saver = Autosave({
    url: base + "/api/profile",
    delay: 1000,
    status: document.querySelector("[data-testid=profile-save-status]"),
    message: document.querySelector("[data-testid=profile-save-error]"),
    onSaved: function (data, sent) {
      document.dispatchEvent(new CustomEvent("profile:saved", { detail: data }));
      // A new language: the page is drawn again in it, once the pick has
      // shown as saved (the yellow circle, then the green check) like any
      // other change, so the reload does not swallow them.
      if (sent && "lang" in sent) { reloadInLanguage(); return; }
      if (data.keys) Object.keys(data.keys).forEach(keySaved.bind(null, data.keys));
      if (!data.display_name) return;
      if (heading) heading.textContent = data.display_name;
      if (menuName) menuName.textContent = data.display_name;
      if (toggle) {
        toggle.title = data.display_name;
        toggle.setAttribute("aria-label", t("profile.js.account_menu_for", { name: data.display_name }));
      }
    },
  });

  function name(label) {
    return function (v) {
      if (v.trim().length > 60) return t("profile.js.name_too_long", { label: label, max: 60 });
      if (/[\u0000-\u001f\u007f]/.test(v)) return t("profile.js.name_bad_chars", { label: label });
      return "";
    };
  }
  var first = document.querySelector("[data-testid=profile-first-name]");
  var last = document.querySelector("[data-testid=profile-last-name]");
  if (first) saver.watch(first, { field: "first_name", validate: name(t("profile.js.first_name")) });
  if (last) saver.watch(last, { field: "last_name", validate: name(t("profile.js.last_name")) });

  // AI API keys: a pasted key saves at once, a typed one 3 s after the last
  // keystroke. Saved, the box empties (a key is never shown again) and the
  // row says which key is on file.
  document.querySelectorAll("[data-testid=profile-api-key-input]").forEach(function (input) {
    saver.watch(input, { field: input.name, delay: 3000, burst: 6, ignoreEmpty: true });
  });
  function keySaved(keys, provider) {
    var row = document.querySelector('[data-testid=profile-api-key][data-provider="' + provider + '"]');
    if (!row) return;
    row.dataset.keySet = "true";
    row.dataset.unreadable = "false";
    var input = row.querySelector("[data-testid=profile-api-key-input]");
    input.value = "";
    var status = row.querySelector("[data-testid=profile-api-key-status]");
    status.textContent = "";
    var ok = document.createElement("span");
    ok.className = "ok-text";
    ok.textContent = t("profile.js.key_on_file");
    var code = document.createElement("code");
    code.textContent = "…" + keys[provider];
    status.append(ok, " · " + t("profile.js.ends_in") + " ", code);
    row.querySelector("[data-key-prompt]").textContent = t("profile.js.paste_new_key");
  }

  // The language (#462) saves as soon as one is picked; the page then
  // reloads in it (onSaved above). Its check sits on the label.
  var lang = document.querySelector("[data-testid=profile-lang]");
  var langLabel = document.querySelector("[data-testid=profile-lang-label]");
  if (lang) saver.watch(lang, { field: "lang", immediate: true, mark: function () { return langLabel; } });
  // Reload when everything is saved and the green has been on screen for a
  // moment (SEEN_MS: longer than the save takes to settle); another edit made
  // meanwhile is saved first, so the reload never asks about unsaved changes.
  // The page drawn again shows the pick as saved still (LANG_SAVED, below),
  // so the circle and the check carry over the reload.
  var SEEN_MS = 1500;
  var LANG_SAVED = "profile:lang-saved";
  var reloading = false;
  function reloadInLanguage() {
    if (reloading) return;
    reloading = true;
    (function again() {
      saver.settle().then(function (ok) {
        if (!ok) { reloading = false; return; }
        setTimeout(function () {
          if (saver.state() !== "saved") return again();
          try { sessionStorage.setItem(LANG_SAVED, "1"); } catch (e) { /* no storage: the checks are just not carried over */ }
          location.reload();
        }, SEEN_MS);
      });
    })();
  }
  try {
    if (lang && sessionStorage.getItem(LANG_SAVED)) {
      sessionStorage.removeItem(LANG_SAVED);
      saver.saved("lang");
    }
  } catch (e) { /* no storage */ }
  // Two-factor sign-in: choosing a method posts the form (the page comes
  // back saying what changed, or with the authenticator's setup dialog).
  // Every change (a chip, or the app's Remove; turning it on too, #374)
  // opens the "Confirm it's you" dialog first, which posts the same change
  // with the password (or the code of an account without one);
  // closing it puts the chips back. The server checks it
  // either way.
  var tfCard = document.querySelector("[data-testid=profile-two-factor]");
  var tfForm = document.querySelector("[data-testid=profile-two-factor-form]");
  var confirmDlg = document.getElementById("two-factor-confirm-dialog");
  var confirmForm = document.getElementById("two-factor-confirm-form");
  var confirmSend = document.getElementById("two-factor-confirm-send-form");
  var current = tfCard ? tfCard.dataset.method : "";
  function askToConfirm(method, remove) {
    confirmForm.action = remove ? confirmForm.dataset.removeAction : confirmForm.dataset.methodAction;
    confirmForm.elements.method.value = method;
    if (confirmSend) {
      confirmSend.elements.method.value = method;
      confirmSend.elements.remove.value = remove ? "1" : "";
    }
    var what = confirmDlg.querySelector("[data-testid=two-factor-confirm-what]");
    var chip = document.querySelector('[data-testid=profile-two-factor-method][data-method="' + method + '"]');
    var label = chip ? chip.textContent.trim().toLowerCase() : "";
    what.textContent = remove || !method ? t("profile.js.turn_off") : t(current ? "profile.js.switch_to" : "profile.js.turn_on_by", { method: label });
    confirmDlg.showModal();
    var field = confirmForm.querySelector("input[name=password], input[name=code]");
    if (field) field.focus();
  }
  if (confirmDlg) confirmDlg.addEventListener("close", function () {
    var mine = tfForm && tfForm.querySelector('input[name=method][value="' + current + '"]');
    if (mine) mine.checked = true;
  });
  if (tfForm) tfForm.addEventListener("change", function (e) {
    if (e.target.name !== "method") return;
    if (confirmDlg && e.target.value !== current) askToConfirm(e.target.value, false);
    else tfForm.submit();
  });
  var tfRemove = document.querySelector("[data-testid=profile-two-factor-totp-remove][data-confirm-remove]");
  if (tfRemove && confirmDlg) tfRemove.addEventListener("click", function (e) {
    e.preventDefault();
    askToConfirm("", true);
  });
  // The side list marks the section in view (#420): aria-current="location"
  // on its link, and the .current style. The section counts as in view once
  // its top passes a third of the way down the window; at the very bottom of
  // the page the last section wins (it may be too short to get there).
  var navLinks = Array.prototype.slice.call(document.querySelectorAll("[data-testid=profile-nav-link]"));
  if (navLinks.length) {
    var marked = null;
    var mark = function (link) {
      if (link === marked) return;
      navLinks.forEach(function (a) {
        a.classList.toggle("current", a === link);
        if (a === link) a.setAttribute("aria-current", "location");
        else a.removeAttribute("aria-current");
      });
      marked = link;
    };
    var inView = function () {
      var line = window.innerHeight / 3, best = null;
      var shown = navLinks.filter(function (a) { return a.offsetParent !== null; });
      shown.forEach(function (a) {
        var s = document.getElementById(a.dataset.section);
        if (s && s.getBoundingClientRect().top <= line) best = a;
      });
      var atBottom = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2;
      if (atBottom && shown.length) best = shown[shown.length - 1];
      return best || shown[0] || null;
    };
    var pending = false, pinnedAt = null;
    var update = function () {
      if (pending) return;
      pending = true;
      requestAnimationFrame(function () {
        pending = false;
        // Just after a jump the jumped-to link stays marked until the reader scrolls on.
        if (pinnedAt !== null && Math.abs(window.scrollY - pinnedAt) < 40) return;
        pinnedAt = null;
        mark(inView());
      });
    };
    window.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    // A jump marks the link it came from, even when the section is too low to reach the line.
    // The browser has already scrolled to the section when hashchange fires, so the pin is
    // taken at once: pinning on the next frame could catch a later scroll instead (#674).
    var fromHash = function () {
      var id = location.hash.slice(1);
      var link = navLinks.filter(function (a) { return a.dataset.section === id; })[0];
      if (!link) return update();
      mark(link);
      pinnedAt = window.scrollY;
    };
    window.addEventListener("hashchange", fromHash);
    if (location.hash) {
      fromHash();
      // On page load the browser scrolls to the section after this script runs.
      requestAnimationFrame(function () { if (pinnedAt !== null) pinnedAt = window.scrollY; });
    } else update();
  }
})();
