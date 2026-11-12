// Connected apps (#167): the owner turns on "Let apps connect" on Manage
// Users; a teacher's profile then has a Connected apps section, makes a
// token with its password, sees it once, lists it, uses it on
// /api/v1/whoami (bearer only) and revokes it.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, signUp } from "../lib/learner";

async function setAppsOn(page: Page, on: boolean): Promise<void> {
  await page.goto("/manage#apps");
  const row = page.getByTestId("manage-apps");
  if ((await row.getAttribute("data-apps-on")) !== String(on)) await page.getByTestId("manage-apps-switch").click();
  await expect(page.getByTestId("manage-apps")).toHaveAttribute("data-apps-on", String(on));
}

test("switch on, make a token, see it listed, use it, revoke it", async ({ browser, page, playwright }) => {
  await devLogin(page, "");
  await setAppsOn(page, true);

  const ctx = await browser.newContext();
  const me = await ctx.newPage();
  const account = newAccount("apps");
  await signUp(me, account);
  await me.goto("/profile#apps");
  await expect(me.getByTestId("profile-apps")).toBeVisible();
  await expect(me.getByTestId("profile-apps-none")).toBeVisible();

  // A wrong password: the dialog again with its error, and no token.
  await me.getByTestId("profile-apps-make-open").click();
  await me.getByTestId("profile-apps-token-name").fill("Test");
  await me.getByTestId("profile-apps-token-password").fill("not-the-password");
  await me.getByTestId("profile-apps-token-submit").click();
  await expect(me.getByTestId("profile-apps-token-error")).toBeVisible();
  await expect(me.getByTestId("profile-apps-new-token")).toHaveCount(0);

  // The right one: the token once, starting with skr_.
  await me.getByTestId("profile-apps-token-name").fill("Test");
  await me.getByTestId("profile-apps-token-password").fill(account.password);
  await me.getByTestId("profile-apps-token-submit").click();
  const field = me.getByTestId("profile-apps-new-token");
  await expect(field).toBeVisible();
  const token = await field.inputValue();
  expect(token).toMatch(/^skr_[0-9a-f]{64}$/);
  await me.getByTestId("profile-apps-new-token-close").click();
  await expect(me.getByTestId("profile-app")).toHaveCount(1);
  await expect(me.getByTestId("profile-app-name")).toHaveText("Test");
  await expect(me.getByTestId("profile-app")).toHaveAttribute("data-used", "false");

  // Bearer only: a client with no cookies.
  const api = await playwright.request.newContext({ baseURL: test.info().project.use.baseURL });
  const who = await api.get("/api/v1/whoami", { headers: { authorization: `Bearer ${token}` } });
  expect(who.status()).toBe(200);
  const body = await who.json();
  expect(body.client_name).toBe("Test");
  expect(body.scopes).toEqual(["courses:read", "courses:write"]);
  expect((await me.request.get("/api/v1/whoami")).status()).toBe(401);
  expect((await api.get(`/api/v1/whoami?access_token=${token}`, { headers: { authorization: `Bearer ${token}` } })).status()).toBe(400);

  await me.goto("/profile#apps");
  await expect(me.getByTestId("profile-app")).toHaveAttribute("data-used", "true");
  await expect(me.locator("body")).not.toContainText(token);

  // The owner sees it on Manage Users.
  await page.goto("/manage#apps");
  await expect(page.getByTestId("manage-app").filter({ hasText: account.email })).toHaveCount(1);

  // Revoke on the profile: the token stops working.
  await me.getByTestId("profile-app-revoke").click();
  await expect(me.getByTestId("profile-apps-none")).toBeVisible();
  expect((await api.get("/api/v1/whoami", { headers: { authorization: `Bearer ${token}` } })).status()).toBe(401);

  await api.dispose();
  await ctx.close();
  await setAppsOn(page, false);
});
