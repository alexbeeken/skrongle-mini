// The student frame (#219): with JavaScript off, a student signs in, opens
// Change my password from the top bar, changes it, signs out and signs in
// with the new one. Each page in the frame loads one document and one
// stylesheet, fits 800 by 600 and 320 wide without sideways scroll, and
// keeps text at 16px or more and buttons at least 44px high. The dev user
// (an owner) makes the student with /dev/student.
import { expect, test, type Browser, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";

/** A new student of the dev user: its username and password. */
async function newStudent(page: Page): Promise<{ username: string; password: string }> {
  await devLogin(page);
  const res = await page.request.get("/dev/student?format=json");
  expect(res.status()).toBe(200);
  return res.json();
}

/** A browser with JavaScript off, at `width` by 600. */
async function oldBrowser(browser: Browser, width: number): Promise<Page> {
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width, height: 600 } });
  return context.newPage();
}

/** Opens `path` and checks the frame's rules on what loaded and how it lays out. */
async function expectLight(page: Page, path: string, width: number): Promise<void> {
  const loaded: string[] = [];
  const onRequest = (r: import("@playwright/test").Request) => loaded.push(`${r.resourceType()} ${new URL(r.url()).pathname}`);
  page.on("request", onRequest);
  const res = await page.goto(path);
  await page.waitForLoadState("load");
  page.off("request", onRequest);
  expect(res?.status(), path).toBe(200);
  expect(loaded.map((l) => l.split(" ")[0]).sort(), `${path} loaded ${loaded.join(", ")}`).toEqual(["document", "stylesheet"]);
  expect(loaded, path).toContain("stylesheet /static/student.css");
  await expect(page.locator("script")).toHaveCount(0);
  await expect(page.getByTestId("student-bar")).toBeVisible();
  const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(scrollWidth, `${path} at ${width} px is ${scrollWidth} px wide`).toBeLessThanOrEqual(width);
  const bodySize = await page.evaluate(() => parseFloat(getComputedStyle(document.body).fontSize));
  expect(bodySize, `${path}: body text`).toBeGreaterThanOrEqual(16);
  for (const button of await page.locator("button.btn").all()) {
    const box = await button.boundingBox();
    expect(box?.height ?? 0, `${path}: a button's height`).toBeGreaterThanOrEqual(44);
  }
}

test("with JavaScript off a student signs in, changes its password and signs in with the new one", async ({ page, browser }) => {
  const { username, password } = await newStudent(page);
  const kid = await oldBrowser(browser, 800);

  await expectLight(kid, "/login", 800);
  await kid.getByTestId("login-email").fill(username);
  await kid.getByTestId("login-password").fill(password);
  await kid.getByTestId("login-submit").click();
  await expect(kid).not.toHaveURL(/\/login/);

  await expectLight(kid, "/password", 800);
  await expect(kid.getByTestId("student-bar-name")).toHaveText(username);
  await expect(kid.getByTestId("student-my-subjects")).toHaveText("My subjects");
  await expect(kid.getByTestId("student-change-password")).toHaveText("Change my password");
  await expect(kid.getByTestId("student-sign-out")).toHaveText("Sign out");

  // A wrong current password: the form again, with the reason.
  await kid.getByTestId("student-password-current").fill("not-the-password");
  await kid.getByTestId("student-password-new").fill("a-new-password");
  await kid.getByTestId("student-password-confirm").fill("a-new-password");
  await kid.getByTestId("student-password-submit").click();
  await expect(kid.getByTestId("student-password-error")).toHaveText("The current password is not right.");

  await kid.getByTestId("student-password-current").fill(password);
  await kid.getByTestId("student-password-new").fill("a-new-password");
  await kid.getByTestId("student-password-confirm").fill("a-new-password");
  await kid.getByTestId("student-password-submit").click();
  await expect(kid).toHaveURL(/\/password\?saved=1$/);
  await expect(kid.getByTestId("student-password-saved")).toBeVisible();

  await kid.getByTestId("student-sign-out").click();
  await expect(kid).toHaveURL(/\/login$/);
  await kid.getByTestId("login-email").fill(username);
  await kid.getByTestId("login-password").fill(password);
  await kid.getByTestId("login-submit").click();
  await expect(kid.getByTestId("login-error")).toBeVisible();
  await kid.getByTestId("login-password").fill("a-new-password");
  await kid.getByTestId("login-submit").click();
  await expect(kid).not.toHaveURL(/\/login/);
  await kid.context().close();
});

test("the frame's pages fit 320 wide with JavaScript off", async ({ page, browser }) => {
  const { username, password } = await newStudent(page);
  const kid = await oldBrowser(browser, 320);
  await expectLight(kid, "/login", 320);
  await kid.getByTestId("login-email").fill(username);
  await kid.getByTestId("login-password").fill(password);
  await kid.getByTestId("login-submit").click();
  await expect(kid).not.toHaveURL(/\/login/);
  await expectLight(kid, "/password", 320);
  await expectLight(kid, "/courses?pick=1", 320);
  await kid.context().close();
});
