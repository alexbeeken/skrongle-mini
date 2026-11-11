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
