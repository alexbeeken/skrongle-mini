// /login/code for a texted code: if the provider refuses the text because
// the number opted out of texts (a STOP reply), the status says so and the
// page asks for a new code, which the server e-mails instead (#969).
(function () {
  var page = document.querySelector("[data-testid=login-code][data-status]");
  var resend = document.querySelector("[data-testid=login-code-resend]");
  if (!page || !resend) return;
  var url = page.getAttribute("data-status");
  var tries = 0;
  function check() {
    if (++tries > 40) return;
    fetch(url, { credentials: "same-origin", headers: { Accept: "application/json" } })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (s) {
        if (s && s.texts_stopped) resend.form.submit();
        else if (s) setTimeout(check, 3000);
      })
      .catch(function () { setTimeout(check, 3000); });
  }
  setTimeout(check, 1000);
})();
