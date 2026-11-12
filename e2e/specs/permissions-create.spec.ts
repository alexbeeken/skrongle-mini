// The Create courses permission (#143): an account without it (a Student)
// is offered no way to make a course on its course list, the routes that
// make one refuse it, and it still learns from a built-in course. With the
// permission back, everything is offered again.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, register } from "../lib/learner";

/** The owner (`page`, on Manage Users) gives the account `email` the preset `id` (1 Student, 2 Teacher) through its Permissions dialog. */
async function givePreset(page: Page, email: string, id: "1" | "2"): Promise<void> {
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: email });
  await row.getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await expect(dialog).toBeVisible();
  await dialog.locator(`[data-testid=manage-permissions-preset][data-preset="${id}"]`).check();
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${email}`);
  await expect(row).toHaveAttribute("data-preset", id);
}

test("a Student is offered no way to make a course and still learns from a built-in one", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("perms-create");
  const email = a.email.toLowerCase();
  await register(them, a, "spanish");
  const spanish = them.locator('[data-testid="courses-card"][data-course-id="spanish"]');
  const gear = spanish.getByTestId("courses-settings");
  const settings = them.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');

  // A new account is a Teacher: Create a course, and Copy behind Spanish's gear.
  await them.goto("/courses");
  await expect(them.getByTestId("courses-create")).toBeVisible();
  await gear.click();
  await expect(settings.getByTestId("courses-copy")).toBeVisible();

  await devLogin(page);
  await givePreset(page, email, "1");

  // The Student's course list: nothing to make a course with, and nothing left where the block was.
  await them.goto("/courses");
  await expect(them.getByTestId("courses-lead")).toHaveText(/^Pick a course to learn from\./);
  await expect(them.getByTestId("courses-create")).toHaveCount(0);
  await expect(them.getByTestId("courses-make-section")).toHaveCount(0);
  await expect(them.getByTestId("courses-make")).toHaveCount(0);
  await expect(them.locator("main")).not.toContainText("Create a course");
  await expect(them.getByTestId("courses-copy")).toHaveCount(0);
  await expect(them.getByTestId("courses-translate")).toHaveCount(0);
  // The gear still opens the course's settings, without the icons.
  await gear.click();
  await expect(settings).toBeVisible();
  await expect(settings.getByTestId("courses-copy")).toHaveCount(0);
  // The routes refuse.
  expect((await them.request.post("/courses/new", { form: { title: "" }, maxRedirects: 0 })).status()).toBe(403);
  expect((await them.request.post("/courses/spanish/copy", { maxRedirects: 0 })).status()).toBe(403);
  expect((await them.request.post("/courses/spanish/translate", { form: { lang: "fr" }, maxRedirects: 0 })).status()).toBe(403);
  await them.goto("/courses");
  await expect(them.getByTestId("courses-projects")).toBeVisible();
  await expect(them.getByTestId("courses-mine")).toHaveCount(0);
  // The built-in course still opens.
  await spanish.getByTestId("courses-open").click();
  await expect(them).toHaveURL(/\/$/);
  await expect(them.getByTestId("home-mode").first()).toBeVisible();

  // A Teacher again: it is all back.
  await givePreset(page, email, "2");
  await them.goto("/courses");
  await expect(them.getByTestId("courses-create")).toBeVisible();
  await gear.click();
  await expect(settings.getByTestId("courses-copy")).toBeVisible();
  await them.context().close();
});
