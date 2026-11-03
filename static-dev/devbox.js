// The AI section (templates/dev_ai.html): the call log, each key profile's
// usage and the Manual replies section are re-read from /dev/ai/rows every
// few seconds, so calls and held requests show up as the app makes them; a
// click on a row opens the call (or the fixture). The settings forms post
// and reload the page; the Manual replies pill and the reply forms post in
// place (data-live).
(function () {
  var root = document.querySelector('[data-testid="dev-ai"]');
  if (!root) return;
  var rowsUrl = root.dataset.rowsUrl;
  var POLL_MS = 2000;
  var seq = 0;

  function refresh() {
    var mine = ++seq;
    return fetch(rowsUrl, { headers: { accept: 'application/json' }, credentials: 'same-origin' })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (data) {
        if (!data || mine !== seq) return;
        var body = root.querySelector('[data-rows="log"]');
        if (body && body.dataset.html !== data.log) {
          body.innerHTML = data.log;
          body.dataset.html = data.log;
        }
        if (typeof data.holding === 'boolean') setHolding(data.holding);
        if (data.held) mergeHeld(data.held.html, data.held.pending);
        Object.keys(data.profiles || {}).forEach(function (who) {
          var p = data.profiles[who];
          var card = root.querySelector('[data-testid="dev-ai-profile"][data-who="' + who + '"]');
          if (!card) return;
          card.dataset.over = p.over ? 'true' : 'false';
          var usage = card.querySelector('[data-usage]');
          if (usage) usage.textContent = p.usage;
          var detail = card.querySelector('[data-detail]');
          if (detail) detail.textContent = p.detail;
          var badge = card.querySelector('[data-over-badge]');
          if (badge) badge.hidden = !p.over;
          var row = card.querySelector('[data-inject-row]');
          var text = card.querySelector('[data-inject]');
          if (row) row.hidden = !p.inject;
          if (text) text.textContent = p.inject || '';
        });
      })
      .catch(function () {});
  }

  // Manual replies: the pill, its count and the section at the bottom follow
  // the server's state, so the pill flips (data-live) without a reload.
  var hold = root.querySelector('[data-testid="dev-ai-hold-form"]');
  var held = root.querySelector('[data-testid="dev-ai-held"]');
  function setHolding(on) {
    if (!hold) return;
    hold.dataset.holding = on ? 'true' : 'false';
    var toggle = hold.querySelector('[role="switch"]');
    if (toggle) toggle.setAttribute('aria-checked', on ? 'true' : 'false');
    var state = hold.querySelector('[data-testid="dev-ai-hold-state"]');
    if (state) state.textContent = on ? 'on' : 'off';
    var act = hold.querySelector('[data-hold-do]');
    if (act) act.value = on ? 'off' : 'on';
    var link = hold.querySelector('[data-testid="dev-ai-held-link"]');
    if (link) link.hidden = !on;
    if (held) held.hidden = !on;
  }

  // Held requests are merged, not redrawn: a request you have open, or a
  // reply half typed, stays as it is while new requests land above it and
  // an answered one is swapped for its answered row.
  function mergeHeld(html, pending) {
    root.querySelectorAll('[data-held-count]').forEach(function (n) { n.textContent = pending + ' waiting'; });
    var box = root.querySelector('[data-rows="held"]');
    if (!box || box.dataset.html === html) return;
    box.dataset.html = html;
    var fresh = document.createElement('div');
    fresh.innerHTML = html;
    var items = Array.prototype.slice.call(fresh.querySelectorAll('[data-stem]'));
    if (!items.length) { box.innerHTML = html; return; }
    var stems = {};
    items.forEach(function (i) { stems[i.dataset.stem] = true; });
    Array.prototype.slice.call(box.children).forEach(function (n) { if (!n.dataset.stem || !stems[n.dataset.stem]) n.remove(); });
    var prev = null;
    items.forEach(function (item) {
      var mine = box.querySelector('[data-stem="' + item.dataset.stem + '"]');
      if (!mine) {
        box.insertBefore(item, prev ? prev.nextSibling : box.firstChild);
        mine = item;
      } else if (mine.dataset.pending !== item.dataset.pending) {
        item.open = mine.open;
        mine.replaceWith(item);
        mine = item;
      }
      prev = mine;
    });
  }

  // A key button copies its key and says so for a moment.
  root.addEventListener('click', function (e) {
    var key = e.target.closest('[data-copy-key]');
    if (!key || !navigator.clipboard) return;
    navigator.clipboard.writeText(key.dataset.copyKey).then(function () {
      var label = key.querySelector('[data-key-label]');
      var was = key.dataset.label || (key.dataset.label = label.textContent);
      label.textContent = 'Copied';
      key.dataset.copied = '';
      clearTimeout(key.copiedTimer);
      key.copiedTimer = setTimeout(function () { label.textContent = was; delete key.dataset.copied; }, 1500);
    }, function () {});
  });

  root.addEventListener('click', function (e) {
    var row = e.target.closest('tr[data-href]');
    if (row && !e.target.closest('a, button, form, input, select, textarea')) location.href = row.dataset.href;
  });

  root.addEventListener('submit', function (e) {
    var form = e.target;
    if (!form.hasAttribute('data-live')) return;
    e.preventDefault();
    var error = form.querySelector('[data-error]');
    if (error) error.hidden = true;
    fetch(form.action, {
      method: 'POST',
      headers: { 'x-dev-box': '1', 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams(new FormData(form)).toString(),
      credentials: 'same-origin',
    })
      .then(function (r) {
        if (r.ok) return;
        return r.text().then(function (t) { if (error) { error.textContent = t || r.statusText; error.hidden = false; } });
      })
      .catch(function (err) { if (error) { error.textContent = String(err); error.hidden = false; } })
      .then(refresh);
  });

  document.querySelectorAll('[data-opens]').forEach(function (b) {
    var dialog = document.getElementById(b.dataset.opens);
    if (dialog) b.addEventListener('click', function () { dialog.showModal(); });
  });
  document.querySelectorAll('dialog [data-closes]').forEach(function (b) {
    b.addEventListener('click', function () { b.closest('dialog').close(); });
  });

  setInterval(function () { if (!document.hidden) refresh(); }, POLL_MS);
  document.addEventListener('visibilitychange', function () { if (!document.hidden) refresh(); });
})();

// The dev authenticator (templates/dev_authenticator.html): codes come from
// the server (worked out from each account's secret); the page counts each
// one down against the server's clock, fetches the list again when a code
// runs out (and every few seconds, for accounts added elsewhere), and adds
// or removes without a reload. Copy puts the code on the clipboard.
(function () {
  var root = document.querySelector('[data-testid="dev-authenticator"]');
  if (!root) return;
  var rowsUrl = root.dataset.rowsUrl;
  var list = root.querySelector('[data-rows="auth"]');
  var error = root.querySelector('[data-testid="dev-authenticator-error"]');
  var POLL_MS = 3000;
  // Server time minus this browser's, in seconds.
  var offset = Number(root.dataset.now) - Date.now() / 1000;

  // With no accounts left and nothing typed, the box offers the example
  // account again, so removing the last one is one click from undone.
  var input = root.querySelector('[data-testid="dev-authenticator-uri"]');
  function offerExample() {
    if (!input.value && !list.querySelector('[data-testid="dev-authenticator-item"]')) input.value = input.dataset.example;
  }

  var seq = 0;
  function refresh() {
    var mine = ++seq;
    return fetch(rowsUrl, { headers: { accept: 'application/json' }, credentials: 'same-origin' })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (data) {
        if (!data || mine !== seq) return;
        offset = data.now - Date.now() / 1000;
        if (list.dataset.html !== data.html) {
          list.innerHTML = data.html;
          list.dataset.html = data.html;
          offerExample();
        }
        tick();
      })
      .catch(function () {});
  }

  var waiting = false;
  function tick() {
    var now = Date.now() / 1000 + offset;
    var stale = false;
    list.querySelectorAll('[data-expires]').forEach(function (row) {
      var left = Math.ceil(Number(row.dataset.expires) - now);
      if (left <= 0) { stale = true; left = 0; }
      var timer = row.querySelector('.devauth-timer');
      if (timer) { timer.textContent = String(left); timer.style.setProperty('--left', String(left)); }
    });
    if (stale && !waiting) {
      waiting = true;
      refresh().then(function () { waiting = false; });
    }
  }

  root.addEventListener('click', function (e) {
    var copy = e.target.closest('[data-copy]');
    if (!copy) return;
    var code = copy.dataset.copy;
    var shown = function () {
      copy.classList.add('devauth-copied');
      copy.title = 'Copied';
      setTimeout(function () { copy.classList.remove('devauth-copied'); copy.title = 'Copy the code'; }, 1500);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(code).then(shown, function () {});
  });

  root.addEventListener('submit', function (e) {
    var form = e.target;
    if (!form.hasAttribute('data-live')) return;
    e.preventDefault();
    var button = form.querySelector('button');
    if (button) button.disabled = true;
    var adding = form.classList.contains('devauth-add');
    if (adding) error.hidden = true;
    fetch(form.action, {
      method: 'POST',
      headers: { 'x-dev-box': '1', 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams(new FormData(form)).toString(),
      credentials: 'same-origin',
    })
      .then(function (r) {
        if (r.ok) {
          if (adding) {
            form.reset();
            form.querySelector('input').value = '';
            // A scanned link is added: drop it from the address.
            if (location.search) history.replaceState(null, '', location.pathname);
          }
          return;
        }
        return r.text().then(function (t) {
          if (adding) { error.textContent = t || r.statusText; error.hidden = false; } else alert(t || r.statusText);
        });
      })
      .catch(function (err) { error.textContent = String(err); error.hidden = false; })
      .then(refresh)
      .then(function () { if (button && button.isConnected) button.disabled = false; });
  });

  document.querySelectorAll('[data-opens]').forEach(function (b) {
    var dialog = document.getElementById(b.dataset.opens);
    if (dialog) b.addEventListener('click', function () { dialog.showModal(); });
  });
  document.querySelectorAll('dialog [data-closes]').forEach(function (b) {
    b.addEventListener('click', function () { b.closest('dialog').close(); });
  });

  tick();
  setInterval(function () { if (!document.hidden) tick(); }, 1000);
  setInterval(function () { if (!document.hidden) refresh(); }, POLL_MS);
  document.addEventListener('visibilitychange', function () { if (!document.hidden) refresh(); });
})();

// The dev device framed at the bottom of /login/code
// (templates/login_code_device.html): the rows are re-read from the page's
// own URL every couple of seconds (a text or mail lands moments after the
// password), an authenticator code counts down against the server's clock
// and is re-read when it runs out, and **Use** types the code into the
// sign-in form above (same origin, so the frame may reach its parent).
(function () {
  var root = document.querySelector('[data-testid="login-code-device"]');
  if (!root) return;
  var list = root.querySelector('[data-rows]');
  var POLL_MS = 2000;
  var offset = Number(root.dataset.now) - Date.now() / 1000;

  var seq = 0;
  function refresh() {
    var mine = ++seq;
    return fetch(location.href, { headers: { accept: 'text/html' }, credentials: 'same-origin' })
      .then(function (r) { return r.ok ? r.text() : null; })
      .then(function (html) {
        if (!html || mine !== seq || !list) return;
        var fresh = new DOMParser().parseFromString(html, 'text/html');
        var next = fresh.querySelector('[data-rows]');
        var now = fresh.querySelector('[data-testid="login-code-device"]');
        if (now) offset = Number(now.dataset.now) - Date.now() / 1000;
        if (next && list.innerHTML !== next.innerHTML) list.innerHTML = next.innerHTML;
        tick();
      })
      .catch(function () {});
  }

  var waiting = false;
  function tick() {
    if (!list) return;
    var now = Date.now() / 1000 + offset;
    var stale = false;
    list.querySelectorAll('[data-expires]').forEach(function (row) {
      var left = Math.ceil(Number(row.dataset.expires) - now);
      if (left <= 0) { stale = true; left = 0; }
      var timer = row.querySelector('.devauth-timer');
      if (timer) { timer.textContent = String(left); timer.style.setProperty('--left', String(left)); }
    });
    if (stale && !waiting) {
      waiting = true;
      refresh().then(function () { waiting = false; });
    }
  }

  root.addEventListener('click', function (e) {
    var use = e.target.closest('[data-use]');
    if (!use) return;
    var input;
    try { input = window.parent && window.parent.document.querySelector('[data-testid="login-code-input"]'); } catch (err) { input = null; }
    if (!input) return;
    input.value = use.dataset.use;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.focus();
  });

  tick();
  setInterval(function () { if (!document.hidden) tick(); }, 1000);
  setInterval(function () { if (!document.hidden) refresh(); }, POLL_MS);
  document.addEventListener('visibilitychange', function () { if (!document.hidden) refresh(); });
})();

// /dev/jobs (templates/dev_jobs.html, #872): the (i) opens its dialog. The
// page has no live parts; its buttons are plain forms.
(function () {
  var root = document.querySelector('[data-testid="dev-jobs"]');
  if (!root) return;
  document.querySelectorAll('[data-opens]').forEach(function (b) {
    var dialog = document.getElementById(b.dataset.opens);
    if (dialog) b.addEventListener('click', function () { dialog.showModal(); });
  });
  document.querySelectorAll('dialog [data-closes]').forEach(function (b) {
    b.addEventListener('click', function () { b.closest('dialog').close(); });
  });
})();
