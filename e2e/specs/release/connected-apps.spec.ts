// Connected apps on a release server (#170): the owner turns on "Let apps
// connect" on Manage Users, makes a token on the profile (password again,
// shown once), sees it listed there and on Manage Users with "1 token issued
// in the last day", uses it on /mcp with a client that has no cookies, then
// revokes it and turns the switch off again, as a self-hoster would.
import { baseURL, expect, ownerPerson, test } from "../../lib/release.ts";
import type { Page } from "@playwright/test";

async function setAppsOn(page: Page, on: boolean): Promise<void> {
  await page.goto("/manage#apps");
  if ((await page.getByTestId("manage-apps").getAttribute("data-apps-on")) !== String(on)) await page.getByTestId("manage-apps-switch").click();
  await expect(page.getByTestId("manage-apps")).toHaveAttribute("data-apps-on", String(on));
}

test("the owner lets apps connect, makes a token, sees it listed and counted, uses it and revokes it", async ({ browser, playwright }) => {
  const { page, owner } = await ownerPerson(browser);
  await setAppsOn(page, true);
  // A retry runs against the same server: count from what is there now.
  const before = Number(await page.getByTestId("manage-apps-counts").getAttribute("data-issued"));
  const name = `Release check ${Date.now()}`;

  await page.goto("/profile#apps");
  await page.getByTestId("profile-apps-make-open").click();
  await page.getByTestId("profile-apps-token-name").fill(name);
  await page.getByTestId("profile-apps-token-password").fill(owner.password);
  await page.getByTestId("profile-apps-token-submit").click();
  const field = page.getByTestId("profile-apps-new-token");
  await expect(field).toBeVisible();
  const token = await field.inputValue();
  expect(token).toMatch(/^skr_[0-9a-f]{64}$/);
  await page.getByTestId("profile-apps-new-token-close").click();
  await expect(page.getByTestId("profile-app-name").filter({ hasText: name })).toHaveCount(1);
  await page.goto("/profile#apps");
  await expect(page.locator("body")).not.toContainText(token);

  await page.goto("/manage#apps");
  await expect(page.getByTestId("manage-app").filter({ hasText: name })).toHaveCount(1);
  await expect(page.getByTestId("manage-apps-counts")).toHaveAttribute("data-issued", String(before + 1));
  if (before === 0) await expect(page.getByTestId("manage-apps-counts")).toContainText("1 token issued in the last day");

  // A client with no cookies: the token is its only sign-in.
  const api = await playwright.request.newContext();
  const mcp = `${baseURL()}/mcp`;
  const call = () =>
    api.post(mcp, {
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      data: { jsonrpc: "2.0", id: 1, method: "tools/call", params: { name: "list_courses" } },
    });
  const listed = await call();
  expect(listed.status()).toBe(200);
  expect((await listed.json()).result.isError).toBe(false);
  expect((await api.post(mcp, { headers: { "content-type": "application/json" }, data: {} })).status()).toBe(401);

  await page.getByTestId("manage-app").filter({ hasText: name }).getByTestId("manage-app-revoke").click();
  await expect(page.getByTestId("manage-app").filter({ hasText: name })).toHaveCount(0);
  expect((await call()).status()).toBe(401);
  await setAppsOn(page, false);
  expect((await call()).status()).toBe(404);
  await api.dispose();
});
