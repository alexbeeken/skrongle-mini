// Permissions on Manage Users (#139): the owner opens the Permissions dialog
// on an account's row, sees its preset read-only (marks, no switches), picks
// Custom and gets the switches, with the permissions that need another one
// greyed out while that one is off. The server stores the set. An account
// given Superuser gets Manage Users in its menu and, there, the accounts
// only.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, signUp } from "../lib/learner";

test("the owner sets an account's permissions; a superuser sees only the accounts", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("perms");
  const email = a.email.toLowerCase();
  await signUp(them, a);
  // A new account is a Teacher: no Manage Users for it.
  await them.goto("/profile");
  await them.getByTestId("nav-account-toggle").click();
  await expect(them.getByTestId("nav-manage")).toHaveCount(0);

  await devLogin(page);
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: email });
  await expect(row).toHaveAttribute("data-preset", "2");
  await expect(row.getByTestId("manage-account-preset")).toHaveText("Teacher");
  // An owner's row (the dev user's own) says Owner and has no Permissions button.
  const own = page.getByTestId("manage-account-row").filter({ hasText: "dev@example.com" });
  await expect(own.getByTestId("manage-account-preset")).toHaveText("Owner");
  await expect(own.getByTestId("manage-account-permissions")).toHaveCount(0);

  // Teacher is chosen, shown as marks: no switches.
  await row.getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(email);
  const preset = (id: string) => dialog.locator(`[data-testid=manage-permissions-preset][data-preset="${id}"]`);
  const line = (id: string) => dialog.locator(`[data-testid=manage-permission][data-perm="${id}"]`);
  const toggle = (id: string) => line(id).getByTestId("manage-permission-toggle");
  await expect(preset("2")).toBeChecked();
  await expect(dialog.getByTestId("manage-permission")).toHaveCount(7);
  await expect(dialog.getByTestId("manage-permission").first()).toContainText("Create courses");
  for (const id of ["create_courses", "ai_courses", "ai_tutor", "share_courses", "skip_levels", "privacy", "superuser"]) {
    await expect(toggle(id)).toBeHidden();
    await expect(toggle(id)).toBeDisabled();
    await expect(line(id).getByTestId("manage-permission-mark")).toBeVisible();
    await expect(line(id)).toHaveAttribute("data-on", id === "superuser" ? "false" : "true");
  }
  // Student: nothing, still read-only.
  await preset("1").check();
  await expect(line("create_courses")).toHaveAttribute("data-on", "false");
  await expect(toggle("create_courses")).toBeHidden();
  await preset("2").check();

  // Custom: the switches, starting from what was shown.
  await preset("custom").check();
  await expect(toggle("create_courses")).toBeVisible();
  await expect(toggle("create_courses")).toBeChecked();
  await expect(toggle("ai_courses")).toBeChecked();
  await expect(toggle("superuser")).not.toBeChecked();
  await expect(line("create_courses").getByTestId("manage-permission-mark")).toBeHidden();

  // Create courses off: the two that need it are greyed out and off.
  await toggle("create_courses").uncheck();
  for (const id of ["ai_courses", "share_courses"]) {
    await expect(line(id)).toHaveAttribute("aria-disabled", "true");
    await expect(line(id)).toHaveClass(/is-off/);
    await expect(toggle(id)).toBeDisabled();
    await expect(toggle(id)).not.toBeChecked();
  }
  await expect(toggle("ai_tutor")).toBeEnabled();
  // Back on: changeable again, and still off until checked.
  await toggle("create_courses").check();
  await expect(line("ai_courses")).not.toHaveAttribute("aria-disabled", "true");
  await expect(toggle("ai_courses")).toBeEnabled();
  await expect(toggle("ai_courses")).not.toBeChecked();
  await toggle("create_courses").uncheck();

  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${email}`);
  await expect(row.getByTestId("manage-account-preset")).toHaveText("Custom");
  await expect(row).toHaveAttribute("data-preset", "custom");
  // ai_tutor 4 + skip_levels 16 + privacy 32.
  await expect(row).toHaveAttribute("data-perms", "52");

  // Reopened: Custom, with what was saved.
  await row.getByTestId("manage-account-permissions").click();
  await expect(preset("custom")).toBeChecked();
  await expect(toggle("create_courses")).not.toBeChecked();
  await expect(toggle("ai_tutor")).toBeChecked();
  await expect(toggle("share_courses")).toBeDisabled();
  // Superuser too.
  await toggle("superuser").check();
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toBeVisible();
  await expect(row).toHaveAttribute("data-perms", "116");

  // That account: Manage Users in the menu; the page has the accounts and nothing of the owner's.
  await them.goto("/profile");
  await them.getByTestId("nav-account-toggle").click();
  await expect(them.getByTestId("nav-manage")).toHaveText("Manage Users");
  await them.getByTestId("nav-manage").click();
  await expect(them).toHaveURL(/\/manage$/);
  await expect(them.getByTestId("manage-accounts-card")).toBeVisible();
  await expect(them.getByTestId("manage-ai-card")).toHaveCount(0);
  await expect(them.getByTestId("manage-invites-card")).toHaveCount(0);
  await expect(them.getByTestId("manage-account-temp-password")).toHaveCount(0);
  await expect(them.getByTestId("manage-account-make-owner")).toHaveCount(0);
  // No Permissions button on its own row or on the owner's.
  const mine = them.getByTestId("manage-account-row").filter({ hasText: email });
  await expect(mine.getByTestId("manage-account-preset")).toHaveText("Custom");
  await expect(mine.getByTestId("manage-account-permissions")).toHaveCount(0);
  await expect(them.getByTestId("manage-account-row").filter({ hasText: "dev@example.com" }).getByTestId("manage-account-permissions")).toHaveCount(0);
  // The owner's actions still answer 404 to it.
  expect((await them.request.post("/manage/temp-password", { form: { email }, maxRedirects: 0 })).status()).toBe(404);
  await them.context().close();
});

// One preset for several accounts (#141): the owner ticks two accounts, the
// bar above the table says how many and offers the presets, Apply to selected
// asks first, and both rows say Student afterwards. The header's checkbox
// ticks every row that has one; the owner's row has none.
test("the owner gives several accounts a preset at once", async ({ page, browser }) => {
  const accounts = [newAccount("bulk"), newAccount("bulk")];
  for (const a of accounts) {
    const context = await browser.newContext();
    await signUp(await context.newPage(), a);
    await context.close();
  }
  await devLogin(page);
  await page.goto("/manage");
  const rows = page.getByTestId("manage-account-row");
  const rowOf = (email: string) => rows.filter({ hasText: email.toLowerCase() });
  const bar = page.getByTestId("manage-bulk-bar");
  const count = page.getByTestId("manage-bulk-count");
  const all = page.getByTestId("manage-account-select-all");
  const boxes = page.getByTestId("manage-account-select");
  const ticked = page.locator("[data-testid=manage-account-select]:checked");
  const mixed = () => all.evaluate((box: HTMLInputElement) => box.indeterminate);

  // Nothing ticked: no bar. An owner's row cannot be ticked.
  await expect(bar).toBeHidden();
  await expect(rows.filter({ hasText: "dev@example.com" }).getByTestId("manage-account-select")).toHaveCount(0);
  await expect(all).not.toBeChecked();

  for (const a of accounts) {
    await expect(rowOf(a.email).getByTestId("manage-account-preset")).toHaveText("Teacher");
    await rowOf(a.email).getByTestId("manage-account-select").check();
  }
  await expect(bar).toBeVisible();
  await expect(count).toHaveText("2 selected");

  // The presets, no Custom; Apply asks before anything is sent.
  const options = await page.getByTestId("manage-bulk-preset").locator("option").allInnerTexts();
  expect(options.slice(0, 2)).toEqual(["Student", "Teacher"]);
  expect(options).not.toContain("Custom");
  await page.getByTestId("manage-bulk-preset").selectOption({ label: "Student" });
  await page.getByTestId("manage-bulk-apply").click();
  const dialog = page.getByTestId("manage-bulk-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("manage-bulk-question")).toHaveText('Give "Student" to 2 accounts?');
  await dialog.getByTestId("manage-bulk-confirm").click();

  await expect(page.getByTestId("manage-notice")).toContainText('"Student" given to 2 accounts.');
  await expect(page.getByTestId("manage-notice")).not.toContainText("left as");
  for (const a of accounts) {
    await expect(rowOf(a.email).getByTestId("manage-account-preset")).toHaveText("Student");
    await expect(rowOf(a.email)).toHaveAttribute("data-preset", "1");
    await expect(rowOf(a.email)).toHaveAttribute("data-perms", "0");
  }

  // The header's checkbox: every row that has a box, mixed while only some are ticked.
  await expect(bar).toBeHidden();
  const total = await boxes.count();
  expect(total).toBeGreaterThanOrEqual(2);
  await all.check();
  await expect(ticked).toHaveCount(total);
  await expect(count).toHaveText(`${total} selected`);
  expect(await mixed()).toBe(false);
  await rowOf(accounts[0].email).getByTestId("manage-account-select").uncheck();
  await expect(all).not.toBeChecked();
  expect(await mixed()).toBe(true);
  await expect(count).toHaveText(`${total - 1} selected`);
  await all.check();
  await expect(ticked).toHaveCount(total);
  await all.uncheck();
  await expect(ticked).toHaveCount(0);
  await expect(bar).toBeHidden();

  // A row's own action still goes to its own form, not the table's.
  await rowOf(accounts[1].email).getByTestId("manage-account-permissions").click();
  const permissions = page.getByTestId("manage-permissions-dialog");
  await expect(permissions).toBeVisible();
  await permissions.locator('[data-testid=manage-permissions-preset][data-preset="2"]').check();
  await permissions.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${accounts[1].email.toLowerCase()}`);
  await expect(rowOf(accounts[1].email).getByTestId("manage-account-preset")).toHaveText("Teacher");
  await expect(rowOf(accounts[0].email).getByTestId("manage-account-preset")).toHaveText("Student");
});
