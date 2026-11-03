// Set the clock (#1340): a clock face for a `clock` question. To set it,
// drag the minute hand; the hour hand follows it round, and can be dragged
// too (press near the middle, on the short hand). Keys: arrows move one
// step, Page Up / Page Down an hour, Enter checks.
// The time it shows goes into the hidden answer box as H:MM, and the server
// grades it. A clock to read (data-hands, "hour minute" in degrees) only
// shows its hands. Study, the reading check and the Guide use the
// [data-clock] boxes of the page; Sprint mode calls Clock.make itself.
(function () {
  "use strict";
  var NS = "http://www.w3.org/2000/svg";
  var DAY = 12 * 60;

  function svg(tag, attrs, parent) {
    var e = document.createElementNS(NS, tag);
    Object.keys(attrs).forEach(function (k) { e.setAttribute(k, attrs[k]); });
    if (parent) parent.appendChild(e);
    return e;
  }

  // Minutes since 12:00 as H:MM on the 12-hour face.
  function fmt(total) {
    var h = Math.floor(total / 60) % 12;
    return (h === 0 ? 12 : h) + ":" + ("0" + (total % 60)).slice(-2);
  }

  function hand(parent, len, width, color, testid) {
    var g = svg("g", { "data-testid": testid }, parent);
    svg("line", { x1: 0, y1: 10, x2: 0, y2: -len, stroke: color, "stroke-width": width, "stroke-linecap": "round" }, g);
    return g;
  }

  // Draw a clock in `box`. o.step (minutes) makes it settable; o.hands
  // ([hour, minute] degrees) draws it fixed. o.onChange(time) and
  // o.onEnter() are called as it is set; o.locked() says it is answered.
  function make(box, o) {
    var face = svg("svg", { viewBox: "-110 -110 220 220", width: "220", height: "220", "data-testid": "clock-svg" });
    face.style.cssText = "display:block;margin:0.75rem auto;max-width:100%;height:auto;touch-action:none;color:var(--text)";
    svg("circle", { r: 100, fill: "var(--surface)", stroke: "currentColor", "stroke-width": 4 }, face);
    for (var i = 0; i < 60; i++) {
      var a = i * Math.PI / 30, big = i % 5 === 0, r0 = big ? 84 : 91;
      svg("line", { x1: r0 * Math.sin(a), y1: -r0 * Math.cos(a), x2: 96 * Math.sin(a), y2: -96 * Math.cos(a), stroke: "currentColor", "stroke-width": big ? 3 : 1 }, face);
      if (big) {
        var n = svg("text", { x: 70 * Math.sin(a), y: -70 * Math.cos(a) + 6, "text-anchor": "middle", "font-size": 17, fill: "currentColor" }, face);
        n.textContent = String(i / 5 || 12);
      }
    }
    var hourHand = hand(face, 50, 7, "currentColor", "clock-hour-hand");
    var minHand = hand(face, 80, 4, "var(--accent)", "clock-minute-hand");
    svg("circle", { r: 6, fill: "currentColor" }, face);
    box.appendChild(face);

    function turn(h, m) {
      hourHand.setAttribute("transform", "rotate(" + h + ")");
      minHand.setAttribute("transform", "rotate(" + m + ")");
    }
    if (o.hands) {
      turn(o.hands[0], o.hands[1]);
      face.setAttribute("role", "img");
      face.setAttribute("aria-label", t("clock.js.read_label"));
      return { el: face };
    }

    var step = Math.max(1, o.step || 5), total = 0;
    face.setAttribute("role", "slider");
    face.setAttribute("tabindex", "0");
    face.setAttribute("aria-label", t("clock.js.set_label"));
    face.setAttribute("aria-valuemin", "0");
    face.setAttribute("aria-valuemax", String(DAY - 1));
    face.setAttribute("data-step", String(step));
    function set(v) {
      total = ((v % DAY) + DAY) % DAY;
      turn((total / 2) % 360, (total % 60) * 6);
      face.setAttribute("aria-valuenow", String(total));
      face.setAttribute("aria-valuetext", fmt(total));
      if (o.onChange) o.onChange(fmt(total));
    }
    function locked() { return !!(o.locked && o.locked()); }

    // The angle of a pointer from 12, clockwise, in degrees.
    function angleOf(ev) {
      var r = face.getBoundingClientRect();
      var x = ev.clientX - (r.left + r.width / 2), y = ev.clientY - (r.top + r.height / 2);
      return (Math.atan2(x, -y) * 180 / Math.PI + 360) % 360;
    }
    // A press near the hour hand, inside its length, takes the hour hand
    // (the minute hand covers it at 12:00); anywhere else, the minute hand.
    function grabsHour(ev) {
      var r = face.getBoundingClientRect();
      var d = Math.hypot(ev.clientX - (r.left + r.width / 2), ev.clientY - (r.top + r.height / 2)) * 220 / r.width;
      var off = Math.abs((angleOf(ev) - total / 2 + 540) % 360 - 180);
      return d > 8 && d < 56 && off < 25;
    }
    var dragging = null;
    function move(ev, jump) {
      var deg = angleOf(ev), m = total % 60, h = Math.floor(total / 60);
      if (dragging === "hour") {
        set(Math.round((deg - m / 2) / 30 + 12) % 12 * 60 + m);
        return;
      }
      var nm = Math.round(deg / 6 / step) * step % 60;
      // Past 12 going forwards is the next hour; going back, the one before.
      // A tap that jumps the hand keeps the hour.
      if (!jump && m >= 45 && nm < 15) h += 1;
      else if (!jump && m < 15 && nm >= 45) h -= 1;
      set(h * 60 + nm);
    }
    face.addEventListener("pointerdown", function (ev) {
      if (locked()) return;
      dragging = grabsHour(ev) ? "hour" : "minute";
      if (face.setPointerCapture) face.setPointerCapture(ev.pointerId);
      face.focus();
      move(ev, true);
      ev.preventDefault();
    });
    face.addEventListener("pointermove", function (ev) { if (dragging && !locked()) move(ev); });
    function stop() { dragging = null; }
    face.addEventListener("pointerup", stop);
    face.addEventListener("pointercancel", stop);
    face.addEventListener("keydown", function (ev) {
      if (locked()) return;
      var by = { ArrowUp: step, ArrowRight: step, ArrowDown: -step, ArrowLeft: -step, PageUp: 60, PageDown: -60 }[ev.key];
      if (by) { ev.preventDefault(); set(total + by); }
      else if (ev.key === "Enter" && o.onEnter) { ev.preventDefault(); o.onEnter(); }
    });
    set(0);
    return { el: face, time: function () { return fmt(total); } };
  }

  // The hands' angles for a time "H:MM" ([hour, minute] degrees), or null.
  // Only the lesson editor's preview needs it: a page gets angles from the server.
  function hands(time) {
    var m = /^\s*(\d{1,2}):(\d\d)\s*$/.exec(String(time || ""));
    if (!m) return null;
    var h = Number(m[1]) % 12, min = Number(m[2]);
    return [h * 30 + min / 2, min * 6];
  }

  window.Clock = { make: make, hands: hands };

  // The clocks in the page: the answer box after a settable one holds its time.
  document.querySelectorAll("[data-clock]").forEach(function (box) {
    if (box.dataset.hands) {
      make(box, { hands: box.dataset.hands.split(" ").map(Number) });
      return;
    }
    var input = box.nextElementSibling;
    if (!input || input.tagName !== "INPUT") return;
    var form = box.closest("form");
    make(box, {
      step: Number(box.dataset.step),
      locked: function () { return input.disabled; },
      onChange: function (time) { input.value = time; input.dispatchEvent(new Event("input", { bubbles: true })); },
      onEnter: function () { if (form && form.requestSubmit) form.requestSubmit(); }
    });
  });
})();
