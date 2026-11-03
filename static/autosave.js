// Save as you edit: fields post to a JSON endpoint as they change, with no
// Save button. Used by the profile page (static/profile.js) and the course
// editor (static/courses.js).
//
//   var saver = Autosave({ url, delay, status, message, onSaved, onRefused, body });
//   saver.watch(input, { field, validate, value, immediate, delay, burst, mark, ignoreEmpty, error });
//   saver.track(field, { validate, value, delay })   // a field with no input of its own
//   saver.change(field, { now, delay })              // ...whose value changed
//   saver.problem(name, text)   // a problem the page found ('' clears it): red, and leaving asks
//   saver.saved(field)          // show the field (and the status) as just saved: a save made before a reload
//   saver.settle() -> Promise<bool>   // save what is waiting now; true once all is saved
//   saver.discard()             // drop what is waiting and every error (the page put the saved version back)
//   saver.pause()               // stop saving for good (the page is locked)
//
// `body(pending)`, when given, builds the request from the fields waiting to
// be sent (default: the fields themselves; null sends nothing).
//
// A text field saves `delay` ms after the last keystroke (the field's own
// `delay`, else the page's), or when it loses focus; typing `burst` or more
// characters at once (a paste) saves straight away. A checkbox or radio (or
// `immediate`) saves at once. Once a field's change is saved its `mark`
// element (default: the input's .field-wrap or label; a function for radio
// groups) gets data-saved, which draws a green check, until it is edited
// again; the check fades out FADE_MS after it appears. With `ignoreEmpty` an empty field is simply not sent. One request is
// in flight at a time; edits made meanwhile go in the next. The endpoint
// answers 2xx when saved, or 422 {"errors": {field: message}} and saves
// nothing (`onRefused(data)` gets the whole answer, for a page that places
// the errors itself; a refusal of a field edited since it was sent is not
// shown, and a field's error clears once its latest value saves), or 409 {"error": message} when it never will (the record changed
// elsewhere): that stays red, with the message, and is not tried again.
// When the tab is hidden or closed, what is waiting is sent at once (a
// keepalive request, so it survives the page).
//
// `status` shows the state in data-state: "saved" (green), "saving"
// (yellow) or "error" (red). It stays hidden until there is something to
// say (an edit, or a problem); FADE_MS after it turns green it fades out
// (data-faded), until the next edit. A save shows as saved SETTLE_MS (a
// random 200–500 ms) after the server says so, so the check is believed;
// the request itself and errors are not held back. `message` (an element with role=alert) says
// what has gone wrong with no field of its own to say it in (a problem the
// page found, a save that failed): a field that says it itself is not
// repeated there, so the page does not grow a banner over a typo. A field
// that does not validate is marked aria-invalid with its message in the
// .field-error its aria-describedby names, and is not sent; with `error`
// the field shows its own problem instead (the profile's phone number), and
// that function is handed the message ('' when there is none).
// While anything is unsaved or invalid, leaving the page asks first: a
// dialog for links and forms on the page, the browser's own prompt for the
// rest (reload, back, closing the tab).
(function () {
  "use strict";

  var RETRY_MS = 5000;
  var FADE_MS = 5000;
  function settleMs() { return 200 + Math.floor(Math.random() * 301); }

  window.Autosave = function (opts) {
    var delay = opts.delay == null ? 1000 : opts.delay;
    var fields = {};    // field -> { input, validate, value, error, delay, burst, mark, edits, marked }
    var pending = {};   // field -> value waiting to be sent
    var timer = null;
    var inFlight = false;
    var failure = "";   // why the last save failed (network, server), '' if it did not
    var retry = null;
    var leaving = false;
    var problems = {};  // name -> what the page found wrong ('' when fine)
    var stuck = false;  // a 409: saving cannot go on until the page is reloaded
    var paused = false;
    var waiters = [];   // settle() promises
    var settling = 0;   // saves answered but not shown as saved yet (SETTLE_MS)
    var shown = false;  // the status has had something to say since the page loaded
    var fade = null;    // timer: the green status fades out

    function invalid() {
      return Object.keys(fields).some(function (f) { return fields[f].error; }) || !!why();
    }
    // The first problem that has no field box of its own to say it in.
    function why() {
      var text = "";
      Object.keys(problems).some(function (k) { return (text = problems[k]); });
      if (!text) Object.keys(fields).some(function (f) { return !fields[f].input && (text = fields[f].error); });
      return text;
    }
    function busy() {
      return inFlight || timer !== null || Object.keys(pending).length > 0;
    }
    // What is true: "saved" as soon as the server said so.
    function state() {
      if (invalid() || failure) return "error";
      return busy() ? "saving" : "saved";
    }
    // What the page shows: still "saving" while a save settles.
    function shownState() {
      var s = state();
      return s === "saved" && settling ? "saving" : s;
    }

    function render() {
      var s = shownState();
      var label = { saved: t("common.js.all_saved"), saving: t("common.js.saving"), error: t("common.js.not_saved") }[s];
      if (s !== "saved") shown = true;
      if (opts.status) {
        var was = opts.status.dataset.state;
        opts.status.hidden = !shown;
        if (s !== "saved") {
          clearTimeout(fade);
          fade = null;
          opts.status.removeAttribute("data-faded");
        } else if (shown && (was !== "saved" || (fade === null && !opts.status.hasAttribute("data-faded")))) {
          clearTimeout(fade);
          fade = setTimeout(function () {
            fade = null;
            if (shownState() === "saved") opts.status.setAttribute("data-faded", "");
          }, FADE_MS);
        }
        opts.status.dataset.state = s;
        opts.status.setAttribute("aria-label", label);
        opts.status.title = label;
      }
      if (opts.message) {
        var text = why() || failure;
        opts.message.textContent = text;
        opts.message.hidden = !text;
      }
      var real = state();
      if (waiters.length && (real === "saved" || (real === "error" && !inFlight))) {
        var done = waiters;
        waiters = [];
        done.forEach(function (w) { w(real === "saved"); });
      }
    }

    // Where a field says what is wrong: the .field-error its aria-describedby
    // names. A field may describe itself with more than one element (the
    // phone number's +1 is one of its own), so the box is picked by class,
    // not by being named first.
    function errorBox(input) {
      var ids = (input.getAttribute("aria-describedby") || "").split(" ");
      var first = null;
      for (var i = 0; i < ids.length; i++) {
        var el = ids[i] && document.getElementById(ids[i]);
        if (!el) continue;
        if (el.classList.contains("field-error")) return el;
        if (!first) first = el;
      }
      return first;
    }

    function showError(f, text) {
      var d = fields[f];
      d.error = text || "";
      if (!d.input) return;
      // A field that shows its problem its own way is handed it and does the rest.
      if (d.show) return d.show(d.error);
      d.input.toggleAttribute("aria-invalid", !!text);
      var box = errorBox(d.input);
      if (box) {
        box.textContent = d.error;
        box.hidden = !d.error;
      }
    }

    // The field's green check: on, then faded out FADE_MS later; off when edited.
    function markSaved(f, on) {
      var d = fields[f];
      clearTimeout(d.fade);
      d.fade = null;
      if (d.marked) {
        d.marked.removeAttribute("data-saved");
        d.marked.removeAttribute("data-faded");
      }
      d.marked = on ? d.mark() : null;
      if (!d.marked) return;
      var el = d.marked;
      el.setAttribute("data-saved", "");
      d.fade = setTimeout(function () {
        d.fade = null;
        if (d.marked === el) el.setAttribute("data-faded", "");
      }, FADE_MS);
    }

    function send(keepalive) {
      clearTimeout(timer);
      timer = null;
      if (paused || stuck || inFlight || !Object.keys(pending).length) return render();
      var body = pending;
      var payload = opts.body ? opts.body(body) : body;
      pending = {};
      if (payload == null) return render();
      var edits = {};
      Object.keys(body).forEach(function (f) { edits[f] = fields[f] ? fields[f].edits : 0; });
      inFlight = true;
      render();
      var json = JSON.stringify(payload);
      fetch(opts.url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: json,
        credentials: "same-origin",
        // A keepalive request outlives the page, but only a small one is allowed.
        keepalive: !!keepalive && json.length < 60000,
      })
        .then(function (r) {
          return r.json().catch(function () { return {}; }).then(function (data) { return { r: r, data: data }; });
        })
        .then(function (res) {
          inFlight = false;
          if (res.r.ok) {
            failure = "";
            // A refusal of an older value no longer stands once the field's latest one is saved.
            Object.keys(body).forEach(function (f) {
              if (fields[f] && fields[f].edits === edits[f] && fields[f].error && !(fields[f].validate && fields[f].validate(body[f]))) showError(f, "");
            });
            // Saved now; shown as saved a moment later. A field edited again
            // meanwhile is not done yet.
            settling++;
            setTimeout(function () {
              settling--;
              Object.keys(body).forEach(function (f) {
                if (fields[f] && fields[f].edits === edits[f]) markSaved(f, true);
              });
              render();
            }, settleMs());
            if (opts.onSaved) opts.onSaved(res.data, body);
          } else if (res.r.status === 422 && res.data.errors) {
            // Nothing was stored: mark the refused fields, send the rest again.
            failure = "";
            if (opts.onRefused) opts.onRefused(res.data, body);
            Object.keys(body).forEach(function (f) {
              if (res.data.errors[f]) {
                // Edited since it was sent: the refusal is of a value that is gone, and the new one is queued.
                if (fields[f] && fields[f].edits === edits[f]) showError(f, res.data.errors[f]);
              } else if (!(f in pending)) {
                pending[f] = body[f];
              }
            });
          } else if (res.r.status === 409) {
            // It never will save (the record changed elsewhere): say why, keep
            // the edits on the page, and stop.
            stuck = true;
            fail(body, res.data.error || t("common.js.could_not_save_reload"));
            return;
          } else {
            fail(body, res.r.status === 401 ? t("common.js.signed_out") : t("common.js.server_error", { status: res.r.status }));
            return;
          }
          send();
        })
        .catch(function () {
          inFlight = false;
          fail(body, t("common.js.unreachable"));
        });
    }

    // Put what was not saved back in the queue (newer edits win) and try
    // again in a while, or as soon as the browser is back online.
    function fail(body, why) {
      failure = why;
      Object.keys(body).forEach(function (f) {
        if (!(f in pending)) pending[f] = body[f];
      });
      clearTimeout(retry);
      if (!stuck) retry = setTimeout(send, RETRY_MS);
      render();
    }
    window.addEventListener("online", function () { if (failure) send(); });
    // Hidden (another tab, the phone locked) or going away: send what waits now.
    function flushNow() {
      if (timer !== null || (Object.keys(pending).length && !inFlight)) send(true);
    }
    document.addEventListener("visibilitychange", function () { if (document.visibilityState === "hidden") flushNow(); });
    window.addEventListener("pagehide", flushNow);
    // Back from the browser's page cache: the checks were for the last
    // visit, so they do not come back with it.
    window.addEventListener("pageshow", function (e) {
      if (!e.persisted) return;
      Object.keys(fields).forEach(function (f) { markSaved(f, false); });
      if (state() === "saved") shown = false;
      render();
    });

    function change(f, now, wait) {
      var d = fields[f];
      if (paused) return;
      d.edits++;
      markSaved(f, false);
      var v = d.value ? d.value(d.input) : true;
      var problem = d.validate ? d.validate(v) : "";
      showError(f, problem);
      if (problem || (d.ignoreEmpty && v === "")) {
        delete pending[f];
      } else {
        pending[f] = v;
      }
      clearTimeout(timer);
      timer = null;
      if (now) send();
      else if (Object.keys(pending).length) timer = setTimeout(send, wait != null ? wait : d.delay == null ? delay : d.delay);
      render();
    }

    function track(f, o) {
      o = o || {};
      fields[f] = fields[f] || {
        input: null,
        validate: o.validate,
        value: o.value || null,
        error: "",
        show: o.error,
        delay: o.delay,
        mark: o.mark || function () { return null; },
        edits: 0,
        marked: null,
      };
    }

    function watch(input, o) {
      var f = o.field || input.name;
      var choice = input.type === "checkbox" || input.type === "radio";
      var mark = typeof o.mark === "function" ? o.mark : function () { return o.mark || input.closest(".field-wrap, label"); };
      fields[f] = fields[f] || {
        input: input,
        validate: o.validate,
        value: o.value || function (el) { return el.type === "checkbox" ? el.checked : el.value; },
        error: "",
        show: o.error,
        delay: o.delay,
        ignoreEmpty: !!o.ignoreEmpty,
        mark: mark,
        edits: 0,
        marked: null,
      };
      if (choice || o.immediate) {
        input.addEventListener("change", function () { change(f, true); });
      } else {
        var before = input.value.length;
        input.addEventListener("input", function () {
          var grew = input.value.length - before;
          before = input.value.length;
          change(f, o.burst != null && grew >= o.burst);
        });
        input.addEventListener("blur", function () { if (timer !== null) send(); });
      }
    }

    // ---- leaving with unsaved changes ----

    function unsavedReason() {
      if (invalid()) return why() ? t("common.js.leave_lose_these", { why: why() }) : t("common.js.leave_fields");
      if (busy() || failure) return t("common.js.leave_latest");
      return "";
    }

    window.addEventListener("beforeunload", function (e) {
      if (leaving || paused || !unsavedReason()) return;
      e.preventDefault();
      e.returnValue = "";
    });

    var dialog = null;
    function confirmLeave(reason, go) {
      if (!dialog) {
        dialog = document.createElement("dialog");
        dialog.className = "modal";
        dialog.dataset.testid = "autosave-leave-dialog";
        dialog.innerHTML = '<h2></h2><p data-reason></p><div class="modal-actions">' +
          '<button type="button" class="btn ghost" data-testid="autosave-leave"></button>' +
          '<button type="button" class="btn" data-testid="autosave-stay"></button></div>';
        dialog.querySelector("h2").textContent = t("common.js.leave_title");
        dialog.querySelector("[data-testid=autosave-leave]").textContent = t("common.js.leave_anyway");
        dialog.querySelector("[data-testid=autosave-stay]").textContent = t("common.js.stay");
        document.body.appendChild(dialog);
        dialog.querySelector("[data-testid=autosave-stay]").addEventListener("click", function () { dialog.close(); });
      }
      dialog.querySelector("[data-reason]").textContent = reason;
      dialog.querySelector("[data-testid=autosave-leave]").onclick = function () {
        leaving = true;
        dialog.close();
        go();
      };
      dialog.showModal();
      dialog.querySelector("[data-testid=autosave-stay]").focus();
    }

    // Links and forms that take the browser elsewhere ask in the page's own
    // dialog, which can say what is wrong (the browser's prompt cannot).
    document.addEventListener("click", function (e) {
      var a = e.target.closest && e.target.closest("a[href]");
      if (!a || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      if (a.target && a.target !== "_self") return;
      if (a.hasAttribute("download") || a.getAttribute("href").charAt(0) === "#") return;
      var reason = unsavedReason();
      if (!reason) return;
      e.preventDefault();
      // A menu the link sits in closes, so the question stands alone.
      var menu = a.closest("details[open]");
      if (menu) menu.open = false;
      confirmLeave(reason, function () { location.href = a.href; });
    });
    document.addEventListener("submit", function (e) {
      var form = e.target;
      var reason = unsavedReason();
      // A form that opens a new tab (the editor's open icon) leaves nothing behind.
      if (leaving || !reason || e.defaultPrevented || form.method === "dialog" || (form.target && form.target !== "_self")) return;
      e.preventDefault();
      var by = e.submitter;
      confirmLeave(reason, function () { form.requestSubmit(by || undefined); });
    });

    render();
    return {
      watch: watch,
      track: track,
      change: function (f, o) { o = o || {}; change(f, !!o.now, o.delay); },
      problem: function (name, text) {
        if ((problems[name] || "") === (text || "")) return;
        if (text) problems[name] = text;
        else delete problems[name];
        render();
      },
      state: state,
      // A save the page made before it was drawn again (a reload) is still
      // shown: the green status and the field's check, fading as usual.
      saved: function (f) {
        shown = true;
        if (fields[f]) markSaved(f, true);
        render();
      },
      flush: function () { send(); },
      settle: function () {
        return new Promise(function (resolve) {
          waiters.push(resolve);
          if (timer !== null) send();
          else render();
        });
      },
      discard: function () {
        clearTimeout(timer);
        clearTimeout(retry);
        timer = null;
        pending = {};
        problems = {};
        if (!stuck) failure = "";
        Object.keys(fields).forEach(function (f) { showError(f, ""); });
        render();
      },
      pause: function () {
        paused = true;
        clearTimeout(timer);
        clearTimeout(retry);
        timer = null;
      },
    };
  };
})();
