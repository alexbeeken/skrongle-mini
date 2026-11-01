// The dev mailbox page (templates/dev_box.html): tabs that switch boxes in
// place, rows that open on a click, buttons that post without leaving the
// page, and a poll of /dev/inbox/rows every few seconds so new mail and
// state changes show without a reload. Dev mode only.
(function () {
  var root = document.querySelector('[data-testid="dev-box"]');
  if (!root) return;
  var rowsUrl = root.dataset.rowsUrl;
  var POLL_MS = 3000;

  function showTab(tab, link) {
    root.dataset.tab = tab;
    root.querySelectorAll('[role="tab"]').forEach(function (t) { t.setAttribute('aria-selected', t.dataset.tab === tab ? 'true' : 'false'); });
    root.querySelectorAll('[data-panel]').forEach(function (p) { p.hidden = p.dataset.panel !== tab; });
    var filter = root.querySelector('.devbox-filter');
    if (filter && link) filter.action = link.pathname;
    if (link) history.replaceState(null, '', link.href);
  }

  // Only the newest request's answer is drawn, so a poll that started
  // before a button's post can't put the old rows back.
  var seq = 0;
  function refresh() {
    var mine = ++seq;
    return fetch(rowsUrl, { headers: { accept: 'application/json' }, credentials: 'same-origin' })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (data) {
        if (!data || mine !== seq) return;
        Object.keys(data).forEach(function (key) {
          var body = root.querySelector('[data-rows="' + key + '"]');
          if (body && body.dataset.html !== data[key].html) {
            body.innerHTML = data[key].html;
            body.dataset.html = data[key].html;
          }
          var count = root.querySelector('[data-count="' + key + '"]');
          if (count) count.textContent = data[key].unread > 0 ? String(data[key].unread) : '';
        });
      })
      .catch(function () {});
  }

  root.addEventListener('click', function (e) {
    var tab = e.target.closest('[role="tab"]');
    if (tab && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      showTab(tab.dataset.tab, tab);
      return;
    }
    // A click anywhere on a row but its links and buttons opens the message.
    var row = e.target.closest('tr[data-href]');
    if (row && !e.target.closest('a, button, form, input')) location.href = row.dataset.href;
  });

  root.addEventListener('submit', function (e) {
    var form = e.target;
    if (!form.hasAttribute('data-live')) return;
    e.preventDefault();
    var button = form.querySelector('button');
    if (button) button.disabled = true;
    fetch(form.action, {
      method: 'POST',
      headers: { 'x-dev-box': '1', 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams(new FormData(form)).toString(),
      credentials: 'same-origin',
    })
      .then(function (r) { if (!r.ok) return r.text().then(function (t) { alert(t || r.statusText); }); })
      .catch(function (err) { alert(String(err)); })
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

  setInterval(function () { if (!document.hidden) refresh(); }, POLL_MS);
  document.addEventListener('visibilitychange', function () { if (!document.hidden) refresh(); });
})();

// A text's page (templates/dev_text_message.html): the compose bar posts
// the text without leaving, and the conversation is re-read from the page's
// own URL (every few seconds, and right after a send) so the new bubble, and
// an outbox row's state, show without a reload. **In the outbox** / **On the
// phone** swap the page in place (fetch the other side, push its URL) inside
// a view transition, so the two names trade places and each bubble slides
// across; without view transitions, or under reduced motion, it just swaps.
(function () {
  var page = document.querySelector('[data-testid="dev-text-message"]');
  if (!page) return;
  var POLL_MS = 3000;
  var SIDE = /\/dev\/(phone|sms)\/\d+$/;

  // Bumped by every poll and every swap: only the newest answer is drawn,
  // so a poll of the old side can't land on the new one. A swap is only
  // outrun by a newer swap (its own counter): a poll that starts while the
  // swap's page is on its way must not cancel it, or Back would keep the old
  // side's attributes while the poll draws the new side's bubbles.
  var seq = 0, swaps = 0;
  function refresh() {
    var mine = ++seq;
    return fetch(location.href, { headers: { accept: 'text/html' }, credentials: 'same-origin' })
      .then(function (r) { return r.ok ? r.text() : null; })
      .then(function (html) {
        if (!html || mine !== seq) return;
        var fresh = new DOMParser().parseFromString(html, 'text/html');
        ['.devbox-bubbles', '.devbox-meta'].forEach(function (sel) {
          var now = page.querySelector(sel), next = fresh.querySelector(sel);
          if (now && next && now.innerHTML !== next.innerHTML) now.innerHTML = next.innerHTML;
        });
      })
      .catch(function () {});
  }

  // The parts that move get view-transition names only while switching:
  // the two names and the avatar by who they are, each bubble by its key.
  function name(on) {
    page.querySelectorAll('[data-vt]').forEach(function (el) { el.style.viewTransitionName = on ? 'devbox-' + el.dataset.vt : ''; });
    page.querySelectorAll('.devbox-bubble[data-key]').forEach(function (el) { el.style.viewTransitionName = on ? 'devbox-bubble-' + el.dataset.key : ''; });
  }

  function animate() {
    return typeof document.startViewTransition === 'function' && !matchMedia('(prefers-reduced-motion: reduce)').matches;
  }

  function show(url, push) {
    var mine = ++swaps;
    ++seq;
    fetch(url, { headers: { accept: 'text/html' }, credentials: 'same-origin' })
      .then(function (r) { return r.ok ? r.text() : null; })
      .then(function (html) {
        if (mine !== swaps) return;
        ++seq;
        var fresh = html && new DOMParser().parseFromString(html, 'text/html');
        var next = fresh && fresh.querySelector('[data-testid="dev-text-message"]');
        if (!next) { location.href = url; return; }
        function swap() {
          Array.prototype.slice.call(page.attributes).forEach(function (a) { page.removeAttribute(a.name); });
          Array.prototype.slice.call(next.attributes).forEach(function (a) { page.setAttribute(a.name, a.value); });
          page.innerHTML = next.innerHTML;
          document.title = fresh.title;
          if (push) history.pushState({ devText: true }, '', url);
        }
        if (!animate()) { swap(); return; }
        name(true);
        var t = document.startViewTransition(function () { swap(); name(true); });
        t.finished.then(function () { name(false); }, function () { name(false); });
      })
      .catch(function () { location.href = url; });
  }

  page.addEventListener('click', function (e) {
    var link = e.target.closest('[data-testid="dev-text-other"]');
    if (!link || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    show(link.href, true);
  });

  // Back and forward between the two sides show the side the URL names.
  history.replaceState({ devText: true }, '', location.href);
  window.addEventListener('popstate', function () {
    if (SIDE.test(location.pathname)) show(location.href, false);
    else location.reload();
  });

  page.addEventListener('submit', function (e) {
    var form = e.target.closest('.devbox-compose');
    if (!form) return;
    e.preventDefault();
    var input = form.querySelector('input[name="text"]');
    var button = form.querySelector('button');
    var error = page.querySelector('.devbox-compose-error');
    if (!input.value.trim()) return;
    button.disabled = true;
    error.hidden = true;
    fetch(form.action, {
      method: 'POST',
      headers: { 'x-dev-box': '1', 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams(new FormData(form)).toString(),
      credentials: 'same-origin',
    })
      .then(function (r) {
        if (r.ok) { input.value = ''; return; }
        return r.text().then(function (t) { error.textContent = t || r.statusText; error.hidden = false; });
      })
      .catch(function (err) { error.textContent = String(err); error.hidden = false; })
      .then(function () {
        button.disabled = false;
        if (input.isConnected) input.focus();
        // The texter thread delivers it a moment later: look a few times.
        refresh();
        [600, 1500, 3000].forEach(function (ms) { setTimeout(refresh, ms); });
      });
  });

  setInterval(function () { if (!document.hidden) refresh(); }, POLL_MS);
  document.addEventListener('visibilitychange', function () { if (!document.hidden) refresh(); });
})();

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

// The Payments section (templates/dev_payments.html): what the simulated
// Stripe holds and the log of API calls and webhooks are re-read from
// /dev/payments/rows every couple of seconds, so a checkout paid in another
// tab shows up here; the row actions post in place (data-live) and the page
// re-reads at once; a click on a log row opens the entry.
(function () {
  var root = document.querySelector('[data-testid="dev-payments"]');
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
        ['state', 'log'].forEach(function (key) {
          var box = root.querySelector('[data-rows="' + key + '"]');
          if (box && typeof data[key] === 'string' && box.dataset.html !== data[key]) {
            box.innerHTML = data[key];
            box.dataset.html = data[key];
          }
        });
      })
      .catch(function () {});
  }

  root.addEventListener('click', function (e) {
    var row = e.target.closest('tr[data-href]');
    if (row && !e.target.closest('a, button, form, input')) location.href = row.dataset.href;
  });

  root.addEventListener('submit', function (e) {
    var form = e.target;
    if (!form.hasAttribute('data-live')) return;
    e.preventDefault();
    var button = form.querySelector('button');
    if (button) button.disabled = true;
    fetch(form.action, {
      method: 'POST',
      headers: { 'x-dev-box': '1', 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams(new FormData(form)).toString(),
      credentials: 'same-origin',
    })
      .then(function (r) { if (!r.ok) return r.text().then(function (t) { alert(t || r.statusText); }); })
      .catch(function (err) { alert(String(err)); })
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
