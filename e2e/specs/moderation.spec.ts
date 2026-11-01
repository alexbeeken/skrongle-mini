// Superuser moderation (#967): from /superuser, take a made course down and
// put it back, suspend an account and lift it, delete an account, block an
// address range. The dev user is a superuser. Shares and the 403 for a
// blocked address are checked over HTTP in moderation::tests (blocking the
// browser's own address here would lock out every spec after this one).
import { expect, test } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";

test("a course taken down shows its owner why, and comes back when put back", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("takedown");
  await signUp(them, a);
  await login(them, a);
  const id = await createCourse(them);
  await them.getByTestId("generate-close").click();

  await devLogin(page);
  await page.goto(`/superuser?course_q=${id}`);
  const row = page.locator(`[data-testid="superuser-course-row"][data-course-id="${id}"]`);
  await row.getByTestId("superuser-course-open").click();
  await page.getByTestId("superuser-course-take-down").click();
  const dialog = page.getByTestId("superuser-take-down-dialog");
  await expect(dialog).toContainText(id);
  await dialog.getByTestId("superuser-take-down-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`Course ${id} is taken down.`);
  await page.goto(`/superuser?course_q=${id}`);
  await expect(row).toContainText(a.email.toLowerCase());
  await expect(row).toHaveAttribute("data-state", "down");

  // The owner: a red ! on the card, its dialog says why; no Open; the editor is a 403.
  await them.goto("/courses");
  const card = them.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-open")).toHaveCount(0);
  await card.getByTestId("courses-card-removed").click();
  await expect(card.getByTestId("courses-card-error-dialog")).toContainText("taken down for breaking the Terms");
  expect((await them.request.get(`/courses/${id}`)).status()).toBe(403);

  await row.getByTestId("superuser-course-open").click();
  await page.getByTestId("superuser-course-put-back").click();
  await page.getByTestId("superuser-put-back-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`Course ${id} is back up.`);
  await them.goto("/courses");
  await expect(card.getByTestId("courses-card-removed")).toHaveCount(0);
  expect((await them.request.get(`/courses/${id}`)).status()).toBe(200);
  await them.context().close();
});

test("a suspended account is signed out and can't sign in until the suspension is lifted", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("suspend");
  await signUp(them, a);
  await login(them, a);

  await devLogin(page);
  await page.goto("/superuser");
  const row = page.getByTestId("superuser-account-row").filter({ hasText: a.email.toLowerCase() });
  await row.getByTestId("superuser-account-suspend").click();
  const dialog = page.getByTestId("superuser-suspend-dialog");
  await expect(dialog).toContainText(a.email.toLowerCase());
  await dialog.getByTestId("superuser-suspend-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`${a.email.toLowerCase()} is suspended.`);
  await expect(row).toHaveAttribute("data-suspended", "true");

  await them.goto("/profile");
  await expect(them).toHaveURL(/\/login/);
  await them.getByTestId("login-email").fill(a.email);
  await them.getByTestId("login-password").fill(a.password);
  await them.getByTestId("login-submit").click();
  await expect(them.getByTestId("login-error")).toContainText("suspended for breaking the Terms");

  await row.getByTestId("superuser-account-unsuspend").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`${a.email.toLowerCase()} can sign in again.`);
  await login(them, a);
  await them.context().close();
});

test("a superuser deletes an account after a warning; the address can register again", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("delete");
  await signUp(them, a);

  await devLogin(page);
  await page.goto("/superuser");
  const row = page.getByTestId("superuser-account-row").filter({ hasText: a.email.toLowerCase() });
  // Every action of the row is on screen (they wrap rather than run off the
  // table's right edge), and the superuser's own row has no Suspend or Delete.
  const width = page.viewportSize()!.width;
  for (const b of await row.locator(".account-actions button").all()) {
    const box = (await b.boundingBox())!;
    expect(box.x + box.width, await b.innerText()).toBeLessThanOrEqual(width);
  }
  const own = page.getByTestId("superuser-account-row").filter({ hasText: "dev@example.com" });
  await expect(own.getByTestId("superuser-account-temp-password")).toBeVisible();
  await expect(own.getByTestId("superuser-account-suspend")).toHaveCount(0);
  await expect(own.getByTestId("superuser-account-delete")).toHaveCount(0);
  await row.getByTestId("superuser-account-delete").click();
  const dialog = page.getByTestId("superuser-delete-dialog");
  await expect(dialog).toContainText(a.email.toLowerCase());
  await expect(dialog.getByTestId("superuser-delete-warning")).toContainText("can't be undone");
  await dialog.getByTestId("superuser-delete-reason").fill("Terms breach, case 1");
  await dialog.getByTestId("superuser-delete-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`${a.email.toLowerCase()} and its data are deleted.`);
  await expect(row).toHaveCount(0);

  expect(await signUp(them, a)).toBe(false); // "check your mail": a fresh account
  await them.context().close();
});

test("a superuser blocks an address range and removes the block; others get 404", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("block");
  await signUp(them, a);
  await login(them, a);
  for (const path of ["/superuser/courses/take-down", "/superuser/suspend", "/superuser/delete-account", "/superuser/block-ip"]) {
    expect((await them.request.post(path, { form: { email: "dev@example.com", course: "x", ip: "203.0.113.1" }, maxRedirects: 0 })).status()).toBe(404);
  }
  await them.context().close();

  await devLogin(page);
  await page.goto("/superuser");
  await page.getByTestId("superuser-block").click();
  const dialog = page.getByTestId("superuser-block-dialog");
  await dialog.getByTestId("superuser-block-ip").fill("example.org");
  await dialog.getByTestId("superuser-block-confirm").click();
  await expect(page.getByTestId("superuser-error")).toContainText("not an address or range");

  await page.getByTestId("superuser-block").click();
  await dialog.getByTestId("superuser-block-ip").fill("203.0.113.9/24");
  await dialog.getByTestId("superuser-block-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText("203.0.113.0/24 is blocked.");
  const row = page.locator('[data-testid="superuser-blocked-row"][data-ip="203.0.113.0/24"]');
  await expect(row).toBeVisible();
  await row.getByTestId("superuser-unblock").click();
  await expect(page.getByTestId("superuser-notice")).toContainText("203.0.113.0/24 is no longer blocked.");
  await expect(row).toHaveCount(0);
});
