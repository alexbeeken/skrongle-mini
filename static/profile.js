// Profile page. The names, the AI API keys and the Dev options save as they
// change (static/autosave.js, POST /api/profile); the circle by the heading
// says whether everything is saved, and a green check on a field that its
// change is. (The picture and the theme are a course's settings, #565 and
// #577, picked in the course's settings.)
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

  // The plans modal: pick a plan card, then "Pay with Stripe · price". The
  // form posts (a real navigation) to /billing/checkout, which sends the
  // browser to Stripe's Checkout page (dev mode's stand-in, #314, in dev);
  // only Stripe's signed webhook sets the plan (#963). A learner already
  // paying is sent to the Billing Portal instead.
  var plansForm = document.querySelector("[data-testid=plans-form]");
  if (plansForm) (function () {
    var purchase = plansForm.querySelector("[data-testid=plans-purchase]");
    function picked() { return plansForm.querySelector("input[name=plan]:checked"); }
    function sync() {
      var p = picked();
      var price = p ? " · " + p.closest("[data-plan]").dataset.price : "";
      purchase.textContent = t("profile.js.pay_with_stripe") + price;
      plansForm.dataset.picked = p ? p.value : "";
    }
    plansForm.querySelectorAll("input[name=plan]").forEach(function (r) { r.addEventListener("change", sync); });
    sync();
    plansForm.addEventListener("submit", function (e) {
      if (!picked() || purchase.disabled) { e.preventDefault(); return; }
      purchase.disabled = true;
    });
  })();

  // Plus: "use my keys when my Skrongle AI limit is hit". The pill posts in
  // place (POST /profile/own-keys, JSON) and shows or hides the key rows.
  var ownForm = document.querySelector("[data-testid=profile-own-keys-form]");
  if (ownForm) (function () {
    var sw = ownForm.querySelector("[data-testid=profile-own-keys-switch]");
    var body = document.querySelector("[data-testid=profile-own-keys-body]");
    var card = document.querySelector("[data-testid=profile-api-keys]");
    var busy = false;
    ownForm.addEventListener("submit", function (e) {
      e.preventDefault();
      if (busy) return;
      busy = true;
      sw.setAttribute("aria-busy", "true");
      var on = ownForm.elements.on.value === "1";
      fetch(base + "/profile/own-keys", {
        method: "POST",
        credentials: "same-origin",
        headers: { accept: "application/json", "content-type": "application/x-www-form-urlencoded" },
        body: "on=" + (on ? "1" : "0")
      }).then(function (r) {
        if (!r.ok) throw new Error("HTTP " + r.status);
        return r.json();
      }).then(function (d) {
        var now = !!d.own_keys_fallback;
        sw.setAttribute("aria-checked", now ? "true" : "false");
        ownForm.elements.on.value = now ? "0" : "1";
        if (card) card.dataset.ownKeys = now ? "true" : "false";
        if (body) body.hidden = !now;
      }).catch(function () {
        location.href = base + "/profile#api-keys"; // the server-rendered state
      }).then(function () {
        busy = false;
        sw.removeAttribute("aria-busy");
      });
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

  // The phone number (#292) autosaves like the names; its saved check sits on
  // the label, since the field's own corner shows whether it is verified. A
  // new number is unverified: the red ! (its dialog sends the code) shows,
  // and text-message sign-in can't be chosen until it is verified.
  //
  // US and Canada only for now (#496): the field wears the +1 in its left
  // edge, takes the ten digits and punctuates them as they are typed, so
  // (555) 555 - 5555 appears without anyone typing a bracket. The digits go
  // to the server as E.164 (+15555555555), the one form it stores, and come
  // back punctuated again (`sms::format_us_number`). A number pasted with
  // another country's code is left as it is and marked, never reshaped into
  // a +1 number. The field takes more characters than a formatted number
  // needs, so a pasted one is not cut short before it is formatted.
  var phone = document.querySelector("[data-testid=profile-phone]");
  var phoneLabel = document.querySelector("[data-testid=profile-phone-label]");
  // Another country's number, pasted with its code: left exactly as typed
  // and marked (below), since bending it into a +1 number would make a
  // number nobody can be texted on out of a real one.
  function phoneForeign(v) {
    var s = String(v).trim();
    return s.charAt(0) === "+" && s.charAt(1) !== "1";
  }
  // What the field holds once the +1 it already shows is taken off, typed
  // again or pasted along with the number.
  function phoneBody(v) {
    return String(v).replace(/^\s*\+\s*1/, "");
  }
  // Its digits, at most ten. A leading 1 with ten digits behind it is the
  // country code too (pasted, or autofilled from a full number): no area
  // code starts with 1, so it can be nothing else.
  function phoneDigits(v) {
    var d = phoneBody(v).replace(/[^0-9]/g, "");
    if (d.length > 10 && d.charAt(0) === "1") d = d.slice(1);
    return d.slice(0, 10);
  }
  // How they are shown, as far as they go: 555 → "(555", 5555 → "(555) 5".
  function phoneShown(d) {
    if (!d) return "";
    if (d.length <= 3) return "(" + d;
    if (d.length <= 6) return "(" + d.slice(0, 3) + ") " + d.slice(3);
    return "(" + d.slice(0, 3) + ") " + d.slice(3, 6) + " - " + d.slice(6);
  }
  // Where the caret belongs: after the same digit it was after before, so
  // typing or deleting in the middle of a number does not jump to the end.
  function phoneCaret(shown, digitsBefore) {
    if (digitsBefore < 1) return Math.min(1, shown.length);
    var seen = 0;
    for (var i = 0; i < shown.length; i++) {
      var c = shown.charAt(i);
      if (c >= "0" && c <= "9" && ++seen === digitsBefore) return i + 1;
    }
    return shown.length;
  }
  function formatPhone() {
    var typed = String(phone.value);
    if (phoneForeign(typed)) return;
    var shown = phoneShown(phoneDigits(typed));
    if (shown === typed) return;
    // The digits the caret sat behind, to put it behind the same ones again.
    // Neither the country code taken off the front nor a stray 1 before ten
    // digits is one of them.
    var body = phoneBody(typed);
    var raw = body.replace(/[^0-9]/g, "");
    var dropped = raw.length > 10 && raw.charAt(0) === "1" ? 1 : 0;
    var at = phone.selectionStart;
    var into = at == null ? null : Math.max(0, at - (typed.length - body.length));
    var before = into == null ? null : Math.max(0, body.slice(0, into).replace(/[^0-9]/g, "").length - dropped);
    phone.value = shown;
    if (before != null && document.activeElement === phone) {
      var i = phoneCaret(shown, before);
      try { phone.setSelectionRange(i, i); } catch (e) { /* a field that keeps no caret */ }
    }
  }
  // What the number the field holds is not, if it is not a number yet.
  function phoneProblem(v) {
    if (!v) return "";
    if (v.slice(0, 2) !== "+1") return t("profile.js.phone_us_only");
    var d = v.slice(2);
    if (d.length !== 10) return t("profile.js.phone_digits");
    if (d.charAt(0) < "2" || d.charAt(3) < "2") return t("profile.js.phone_area");
    return "";
  }

  // A number is wrong on the way to being right: every number is too short
  // before its last digit. So the field says nothing while it is being
  // typed. The problem waits for a break — QUIET_MS without a keystroke, or
  // the field losing focus, whichever comes first — and then shows as a red
  // ! in the field's corner, faded in (style.css), never as text that moves
  // the page or covers what was typed. It goes the moment the number is
  // right again. The ! carries the message as its tip and opens a dialog
  // with it, the page's way with something that needs attention.
  var QUIET_MS = 2000;
  var phoneBad = document.querySelector("[data-testid=profile-phone-invalid]");
  var phoneWhy = document.querySelector("[data-testid=phone-problem-why]");
  var phoneErrorBox = document.getElementById("phone-error");
  var phoneWrap = phone && phone.closest(".field-with-icon");
  var phoneWait = null;   // the break being waited for
  var phonePending = "";  // what is wrong now
  var phoneSaid = "";     // what the ! is saying

  function phoneSay() {
    clearTimeout(phoneWait);
    phoneWait = null;
    if (phonePending === phoneSaid) return;
    phoneSaid = phonePending;
    phone.toggleAttribute("aria-invalid", !!phoneSaid);
    if (phoneWrap) phoneWrap.toggleAttribute("data-invalid", !!phoneSaid);
    if (phoneErrorBox) {
      phoneErrorBox.textContent = phoneSaid;
      phoneErrorBox.hidden = !phoneSaid;
    }
    if (phoneWhy) phoneWhy.textContent = phoneSaid;
    if (phoneBad) {
      if (phoneSaid) {
        phoneBad.setAttribute("aria-label", phoneSaid);
        phoneBad.setAttribute("data-tip", phoneSaid);
      }
      phoneBad.hidden = !phoneSaid;
    }
  }
  // The saver hands the field its problem ('' when there is none) on every
  // edit and on a refusal from the server.
  function phoneTrouble(text) {
    phonePending = text || "";
    clearTimeout(phoneWait);
    phoneWait = null;
    if (!phonePending || phonePending === phoneSaid) return phoneSay();
    phoneWait = setTimeout(phoneSay, QUIET_MS);
  }

  if (phone) {
    // Before the saver's own listener, so what it sends and validates is the
    // formatted number, not the half-punctuated one the keystroke left. The
    // stored number arrives punctuated, and a number from before #496 is
    // left alone until it is edited, so nothing is reformatted on load.
    phone.addEventListener("input", formatPhone);
    // Leaving the field is the break the ! was waiting for.
    phone.addEventListener("blur", phoneSay);
    saver.watch(phone, {
      field: "phone",
      mark: function () { return phoneLabel; },
      value: function (el) {
        if (phoneForeign(el.value)) return String(el.value).trim();
        var d = phoneDigits(el.value);
        return d ? "+1" + d : "";
      },
      validate: phoneProblem,
      error: phoneTrouble,
    });
  }
  // The language (#462) saves as soon as one is picked; the page then
  // reloads in it (onSaved above). Its check sits on the label, like the phone's.
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
  function phoneSaved(p) {
    var ok = document.querySelector("[data-testid=phone-verified]");
    var bad = document.querySelector("[data-testid=phone-unverified]");
    var stopped = document.querySelector("[data-testid=phone-opted-out]");
    if (ok) ok.hidden = !p.number || !p.verified || p.opted_out;
    if (bad) bad.hidden = !p.number || p.verified || p.opted_out;
    if (stopped) stopped.hidden = !p.number || !p.opted_out;
    var sms = document.querySelector("[data-testid=profile-two-factor-method][data-method=sms] input");
    if (sms && !p.verified && !sms.checked) sms.disabled = true;
  }
  document.addEventListener("profile:saved", function (e) { if (e.detail && e.detail.phone) phoneSaved(e.detail.phone); });

  // Two-factor sign-in: choosing a method posts the form (the page comes
  // back saying what changed, or with the authenticator's setup dialog).
  // Every change (a chip, or the app's Remove; turning it on too, #374)
  // opens the "Confirm it's you" dialog first, which posts the same change
  // with the password (or a Google-only account's code, or after "Confirm
  // with Google"); closing it puts the chips back. The server checks it
  // either way.
  var tfCard = document.querySelector("[data-testid=profile-two-factor]");
  var tfForm = document.querySelector("[data-testid=profile-two-factor-form]");
  var confirmDlg = document.getElementById("two-factor-confirm-dialog");
  var confirmForm = document.getElementById("two-factor-confirm-form");
  var confirmSend = document.getElementById("two-factor-confirm-send-form");
  var confirmGoogle = document.getElementById("two-factor-confirm-google-form");
  var current = tfCard ? tfCard.dataset.method : "";
  function askToConfirm(method, remove) {
    confirmForm.action = remove ? confirmForm.dataset.removeAction : confirmForm.dataset.methodAction;
    confirmForm.elements.method.value = method;
    if (confirmSend) {
      confirmSend.elements.method.value = method;
      confirmSend.elements.remove.value = remove ? "1" : "";
    }
    if (confirmGoogle) confirmGoogle.elements.method.value = method;
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
