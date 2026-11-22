// The Class box page (#227): the dev user (an owner) opens **Class box**
// from the top bar; How students join shows the address in use; the poster
// is that card alone and prints with no top bar. A student who signs in is
// counted under Right now. **Download a backup** refuses a wrong password,
// then sends a .tar.gz, and the card says the last backup was today.
import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { devLogin } from "../lib/app";

test("the owner's Class box: join card, poster, Right now and a backup", async ({ page, browser, baseURL }) => {
  await devLogin(page);
  await page.getByTestId("nav-class-box").click();
  await expect(page.getByTestId("class-box-page")).toBeVisible();
  await expect(page.getByTestId("class-box-address")).toHaveText(new URL(baseURL ?? page.url()).origin);

  await page.getByTestId("class-box-poster").click();
  await expect(page).toHaveURL(/\/class-box\/poster$/);
  await expect(page.getByTestId("class-box-join")).toBeVisible();
  await expect(page.getByTestId("class-box-now")).toHaveCount(0);
  await page.emulateMedia({ media: "print" });
  await expect(page.locator(".topbar")).toBeHidden();
  await expect(page.getByTestId("class-box-print")).toBeHidden();
  await page.emulateMedia({ media: "screen" });
  await expect(page.getByTestId("class-box-print")).toBeVisible();

  // A student signs in: Right now counts at least that one.
  const res = await page.request.get("/dev/student?format=json");
  expect(res.status()).toBe(200);
  const kid: { username: string; password: string } = await res.json();
  const context = await browser.newContext({ javaScriptEnabled: false });
  const student = await context.newPage();
  await student.goto("/login");
  await student.getByTestId("login-email").fill(kid.username);
  await student.getByTestId("login-password").fill(kid.password);
  await student.getByTestId("login-submit").click();
  expect((await student.request.get("/class-box")).status()).toBe(404);
  await context.close();
  await page.goto("/class-box");
  const onNow = Number(await page.getByTestId("class-box-on-now").getAttribute("data-n"));
  expect(onNow).toBeGreaterThanOrEqual(1);

  // A wrong password: the page says so and nothing downloads.
  await page.getByTestId("class-box-backup-password").fill("not-the-password");
  await page.getByTestId("class-box-backup-download").click();
  await expect(page.getByTestId("class-box-backup-error")).toBeVisible();

  // The right one: a .tar.gz, and the card says today.
  await page.getByTestId("class-box-backup-password").fill("dev-password");
  const [download] = await Promise.all([page.waitForEvent("download"), page.getByTestId("class-box-backup-download").click()]);
  expect(download.suggestedFilename()).toMatch(/^skrongle-backup-\d{4}-\d{2}-\d{2}\.tar\.gz$/);
  const tar = gunzipSync(readFileSync(await download.path()));
  expect(tar.subarray(257, 262).toString()).toBe("ustar");
  await expect(async () => {
    await page.goto("/class-box");
    await expect(page.getByTestId("class-box-last-backup")).toHaveText("Last backup: today.", { timeout: 1000 });
  }).toPass();
});
