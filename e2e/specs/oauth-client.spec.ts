// OAuth sign-in for apps (#169), end to end with the dev test client
// (/dev/oauth-client): discovery names the endpoints; signed out, Connect
// goes through the sign-in, which names the waiting app (#190), to the
// consent page; Deny sends access_denied
// back; Allow with one course trades a code for tokens that work; a refresh
// token rotates and the old one used again revokes the app (#184); the code
// replayed revokes the app; a request edited by
// hand gets the error page and is never sent back; a Student is told to ask
// the owner. scripts/e2e.sh sets SKRONGLE_PUBLIC_URL to the server's address.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { login, logout, newAccount, signUp } from "../lib/learner";

async function setAppsOn(page: Page, on: boolean): Promise<void> {
  await page.goto("/manage#apps");
  const row = page.getByTestId("manage-apps");
  if ((await row.getAttribute("data-apps-on")) !== String(on)) await page.getByTestId("manage-apps-switch").click();
  await expect(page.getByTestId("manage-apps")).toHaveAttribute("data-apps-on", String(on));
}

/** The owner (`page`) makes the account `email` a Student (preset 1). */
async function makeStudent(page: Page, email: string): Promise<void> {
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: email });
  await row.getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await dialog.locator('[data-testid=manage-permissions-preset][data-preset="1"]').check();
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(row).toHaveAttribute("data-preset", "1");
}

test("the dev client signs in, is denied, allowed, refreshes and is revoked by a replayed code", async ({ browser, page, request }) => {
  await devLogin(page, "");
  await setAppsOn(page, true);
  const base = test.info().project.use.baseURL!.replace(/\/$/, "");

  // Discovery: every address starts with the public address.
  const meta = await (await request.get("/.well-known/oauth-authorization-server")).json();
  expect(meta.issuer).toBe(base);
  for (const k of ["authorization_endpoint", "token_endpoint", "registration_endpoint", "revocation_endpoint"]) expect(meta[k]).toMatch(new RegExp(`^${base}/oauth/`));
  expect(meta.code_challenge_methods_supported).toEqual(["S256"]);
  expect(meta.scopes_supported).toEqual(["courses:read", "courses:write"]);
  const mcp = await request.post("/mcp", { headers: { "content-type": "application/json" }, data: "{}" });
  expect(mcp.status()).toBe(401);
  expect(mcp.headers()["www-authenticate"]).toBe(`Bearer resource_metadata="${base}/.well-known/oauth-protected-resource/mcp"`);
  const evil = await request.post("/oauth/register", { data: { client_name: "x", redirect_uris: ["http://evil.example/cb"] } });
  expect(evil.status()).toBe(400);

  const ctx = await browser.newContext();
  const me = await ctx.newPage();
  const account = newAccount("oauth");
  await signUp(me, account);
  const made = await me.request.post("/courses/new", { form: { title: "OAuth course" } });
  const courseId = new URL(made.url()).pathname.split("/").pop()!;

  // Signed out: Connect goes through the sign-in, then to the consent page.
  await ctx.clearCookies();
  await me.goto("/dev/oauth-client");
  await me.getByTestId("dev-oauth-connect").click();
  await me.waitForURL(/\/login(\?|$)/);
  // The sign-in page names the waiting app and its host (#190), and keeps
  // saying so after a wrong password.
  const notice = me.getByTestId("login-oauth-notice");
  await expect(notice).toContainText("Dev test client");
  await expect(me.getByTestId("login-oauth-host")).toHaveText(new URL(base).hostname);
  await me.getByTestId("login-email").fill(account.email);
  await me.getByTestId("login-password").fill("not-the-password");
  await me.getByTestId("login-submit").click();
  await expect(me.getByTestId("login-error")).toBeVisible();
  await expect(notice).toContainText("Dev test client");
  await login(me, account);
  await expect(me.getByTestId("oauth-consent")).toBeVisible();
  await expect(me.getByTestId("oauth-consent-app")).toHaveText("Dev test client");
  await expect(me.getByTestId("oauth-consent-host")).toHaveText(new URL(base).hostname);
  await expect(me.getByTestId("oauth-consent-scope")).toHaveCount(2);
  await expect(me.getByTestId("oauth-consent-course")).toHaveCount(1);
  await expect(me.getByTestId("oauth-consent-lifetime")).toBeVisible();

  // Deny: access_denied comes back, nothing is connected.
  await me.getByTestId("oauth-consent-deny").click();
  await expect(me.getByTestId("dev-oauth-answer")).toHaveText("access_denied");
  await me.goto("/profile#apps");
  await expect(me.getByTestId("profile-apps-none")).toBeVisible();

  // Signed out again, an ordinary sign-in page says nothing of an app.
  await logout(me);
  await expect(me.getByTestId("login-form")).toBeVisible();
  await expect(me.getByTestId("login-oauth-notice")).toHaveCount(0);
  await login(me, account);

  // Allow with one course: the code is traded and whoami answers.
  await me.goto("/dev/oauth-client");
  await me.getByTestId("dev-oauth-connect").click();
  await me.getByTestId("oauth-consent-course").check();
  await me.getByTestId("oauth-consent-allow").click();
  await expect(me.getByTestId("dev-oauth-token")).toHaveText("ok: courses:read courses:write");
  await expect(me.getByTestId("dev-oauth-whoami")).toContainText("200");
  await expect(me.getByTestId("dev-oauth-whoami")).toContainText(courseId);
  const authorizeUrl = (await me.getByTestId("dev-oauth-authorize-url").textContent())!;
  await me.goto("/profile#apps");
  await expect(me.getByTestId("profile-app")).toHaveCount(1);
  await expect(me.getByTestId("profile-app-name")).toHaveText("Dev test client");
  await expect(me.getByTestId("profile-app-courses")).toHaveText("OAuth course");

  // Refresh: a new refresh token, and whoami still answers.
  await me.goto("/dev/oauth-client");
  const before = (await me.getByTestId("dev-oauth-refresh-token").textContent())!;
  await me.getByTestId("dev-oauth-refresh").click();
  await expect(me.getByTestId("dev-oauth-refresh-token")).not.toHaveText(before);
  await me.getByTestId("dev-oauth-whoami-btn").click();
  await expect(me.getByTestId("dev-oauth-whoami")).toContainText("200");

  // The old refresh token again (#184): refused, and the app is disconnected.
  await me.getByTestId("dev-oauth-reuse").click();
  await expect(me.getByTestId("dev-oauth-reuse-result")).toHaveText("400 invalid_grant");
  await expect(me.getByTestId("dev-oauth-whoami")).toContainText("401");
  await me.goto("/profile#apps");
  await expect(me.getByTestId("profile-apps-none")).toBeVisible();

  // Connect again, then the code again: refused, and the app is disconnected.
  await me.goto("/dev/oauth-client");
  await me.getByTestId("dev-oauth-connect").click();
  await me.getByTestId("oauth-consent-allow").click();
  await expect(me.getByTestId("dev-oauth-token")).toHaveText("ok: courses:read courses:write");
  await expect(me.getByTestId("dev-oauth-whoami")).toContainText("200");
  await me.getByTestId("dev-oauth-replay").click();
  await expect(me.getByTestId("dev-oauth-replay-result")).toHaveText("400 invalid_grant");
  await expect(me.getByTestId("dev-oauth-whoami")).toContainText("401");
  await me.goto("/profile#apps");
  await expect(me.getByTestId("profile-apps-none")).toBeVisible();

  // A request edited by hand gets the error page here and goes nowhere.
  const edits: Array<[string, (u: URL) => void]> = [
    ["code_challenge", (u) => u.searchParams.delete("code_challenge")],
    ["plain", (u) => u.searchParams.set("code_challenge_method", "plain")],
    ["state", (u) => u.searchParams.delete("state")],
    ["redirect_uri", (u) => u.searchParams.set("redirect_uri", "https://evil.example/cb")],
  ];
  for (const [name, edit] of edits) {
    const bad = new URL(authorizeUrl);
    edit(bad);
    await me.goto(bad.toString());
    await expect(me.getByTestId("oauth-error"), name).toBeVisible();
    await expect(me.getByTestId("oauth-consent"), name).toHaveCount(0);
    expect(new URL(me.url()).pathname, name).toMatch(/\/oauth\/authorize$/);
  }

  // A Student is told to ask the owner.
  const kidCtx = await browser.newContext();
  const kid = await kidCtx.newPage();
  const student = newAccount("oauth-kid");
  await signUp(kid, student);
  await makeStudent(page, student.email.toLowerCase());
  await kid.goto("/dev/oauth-client");
  await kid.getByTestId("dev-oauth-connect").click();
  await expect(kid.getByTestId("oauth-no-permission")).toBeVisible();
  await expect(kid.getByTestId("oauth-consent")).toHaveCount(0);

  await kidCtx.close();
  await ctx.close();
  await setAppsOn(page, false);
});
