// Skip levels (#146): an account without the permission starts a course with
// levels at the beginning. Its start wizard has no "Where do you want to
// start?" step and the gear on /courses has no level row. Given Skip levels,
// the level row is there and a level saves. Taken away again, the level
// chosen stays, the row is gone and a posted level change is refused.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, signUp } from "../lib/learner";

/** As the owner on Manage Users: open `email`'s Permissions dialog, let
 * `choose` set it up, and Save. */
async function setPermissions(owner: Page, email: string, choose: (dialog: Locator) => Promise<void>): Promise<Locator> {
  await owner.goto("/manage");
  const row = owner.getByTestId("manage-account-row").filter({ hasText: email });
  await row.getByTestId("manage-account-permissions").click();
  const dialog = owner.getByTestId("manage-permissions-dialog");
  await expect(dialog).toBeVisible();
  await choose(dialog);
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(owner.getByTestId("manage-notice")).toContainText(`Permissions saved for ${email}`);
  return row;
}

const preset = (dialog: Locator, id: string) => dialog.locator(`[data-testid=manage-permissions-preset][data-preset="${id}"]`);
const student = (dialog: Locator) => preset(dialog, "1").check();

test("without Skip levels a course starts at the beginning; with it the level can be changed", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("levels");
  const email = a.email.toLowerCase();
  await signUp(them, a);

  // The owner makes the new account a Student: no permission.
  await devLogin(page);
  const row = await setPermissions(page, email, student);
  await expect(row.getByTestId("manage-account-preset")).toHaveText("Student");
  await expect(row).toHaveAttribute("data-perms", "0");

  // Spanish has levels, but its start wizard is the motivator step alone.
  await them.goto("/courses");
  const card = them.locator('[data-testid="courses-card"][data-course-id="spanish"]');
  const dialog = them.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');
  const levels = dialog.getByTestId("courses-settings-level");
  const save = dialog.getByTestId("courses-settings-save");
  const gear = them.getByTestId("courses-projects").locator('[data-course-id="spanish"]').getByTestId("courses-settings");
  await card.getByTestId("courses-open").click();
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute("data-wizard", "");
  await expect(dialog.getByTestId("courses-settings-step")).toHaveCount(1);
  await expect(dialog.getByTestId("courses-settings-motivator").first()).toBeVisible();
  await expect(dialog).not.toContainText("Where do you want to start?");
  await expect(levels).toHaveCount(0);
  await expect(dialog.getByTestId("placement")).toHaveCount(0);
  await expect(dialog.getByTestId("courses-settings-next")).toBeHidden();
  // "Get Started!" opens the course, at the beginning.
  await save.click();
  await expect(them).toHaveURL(/\/$/);
  await expect(dialog).toHaveCount(0);
  expect((await (await them.request.get("/api/progress")).json()).start_unit).toBe("");

  // The gear: the motivator row only.
  await them.goto("/courses");
  await gear.click();
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("courses-settings-row")).toHaveCount(1);
  await expect(dialog.locator('[data-testid="courses-settings-row"][data-row="level"]')).toHaveCount(0);
  await expect(levels).toHaveCount(0);

  // Custom with Skip levels only.
  await setPermissions(page, email, async (d) => {
    await preset(d, "custom").check();
    await d.locator('[data-testid=manage-permission][data-perm="skip_levels"]').getByTestId("manage-permission-toggle").check();
  });
  await expect(row.getByTestId("manage-account-preset")).toHaveText("Custom");
  await expect(row).toHaveAttribute("data-perms", "16");

  // The level row is there, the beginning chosen; A2 saves.
  await them.goto("/courses");
  await gear.click();
  await expect(dialog.getByTestId("courses-settings-row")).toHaveCount(2);
  await expect(dialog.locator('[data-testid="courses-settings-level"][data-unit=""] input')).toBeChecked();
  await dialog.locator('[data-testid="courses-settings-level"][data-unit="A2"]').click();
  await save.click();
  await expect(dialog).toBeHidden();
  await them.goto("/courses");
  await gear.click();
  await expect(dialog.locator('[data-testid="courses-settings-level"][data-unit="A2"] input')).toBeChecked();
  expect((await (await them.request.get("/api/progress")).json()).start_unit).toBe("A2");

  // A Student again: the level stays, the row is gone, a posted level is refused and the motivator still saves.
  await setPermissions(page, email, student);
  await them.goto("/courses");
  await gear.click();
  await expect(dialog).toBeVisible();
  await expect(levels).toHaveCount(0);
  expect((await them.request.post("/courses/spanish/settings", { form: { start_unit: "B1" }, maxRedirects: 0 })).status()).toBe(403);
  await dialog.locator('[data-testid="courses-settings-motivator"][data-motivator-id="xp"]').click();
  await save.click();
  await expect(dialog).toBeHidden();
  await them.goto("/");
  await expect(them.locator('[data-testid="home-today"]')).toHaveAttribute("data-motivator", "xp");
  expect((await (await them.request.get("/api/progress")).json()).start_unit).toBe("A2");
  await them.context().close();
});
