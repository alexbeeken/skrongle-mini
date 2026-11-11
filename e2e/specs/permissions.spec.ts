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

// Saved presets (#140): a Custom set saved under a name is a preset for every
// account; deleting it leaves the accounts that had it their permissions, as
// Custom.
test("a custom set saved as a preset is offered to another account, then deleted", async ({ page, browser }) => {
  const emails: string[] = [];
  for (const prefix of ["preset-a", "preset-b"]) {
    const context = await browser.newContext();
    const account = newAccount(prefix);
    await signUp(await context.newPage(), account);
    await context.close();
    emails.push(account.email.toLowerCase());
  }
  // Its own name each run, so a retry does not meet the last run's preset.
  const name = `Helpers ${Math.random().toString(36).slice(2, 7)}`;

  await devLogin(page);
  await page.goto("/manage");
  const rowOf = (email: string) => page.getByTestId("manage-account-row").filter({ hasText: email });
  const [first, second] = emails.map(rowOf);
  const dialog = page.getByTestId("manage-permissions-dialog");
  const preset = (id: string) => dialog.locator(`[data-testid=manage-permissions-preset][data-preset="${id}"]`);
  const line = (id: string) => dialog.locator(`[data-testid=manage-permission][data-perm="${id}"]`);
  const toggle = (id: string) => line(id).getByTestId("manage-permission-toggle");
  const saveAs = dialog.getByTestId("manage-permissions-save-as");
  const saved = page.getByTestId("manage-preset-row").filter({ hasText: name });
  await expect(saved).toHaveCount(0);

  // The name box is there only while Custom is chosen.
  await first.getByTestId("manage-account-permissions").click();
  await expect(preset("2")).toBeChecked();
  await expect(saveAs).toBeHidden();
  await preset("custom").check();
  await expect(saveAs).toBeVisible();
  await expect(dialog).toContainText("Save as a preset (optional)");
  await expect(dialog).toContainText("Leave empty to save it for this account only.");
  // The AI tutor and Skip levels only: 4 + 16.
  await toggle("create_courses").uncheck();
  await toggle("privacy").uncheck();
  await saveAs.fill(name);
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${emails[0]}`);
  // The row shows the preset's name, not Custom; the list under the accounts has it.
  await expect(first.getByTestId("manage-account-preset")).toHaveText(name);
  await expect(first).toHaveAttribute("data-perms", "20");
  await expect(page.getByTestId("manage-presets")).toBeVisible();
  await expect(saved.getByTestId("manage-preset-name")).toHaveText(name);
  await expect(saved.getByTestId("manage-preset-perms")).toHaveText("Use the AI tutor, Skip levels");
  await expect(saved.getByTestId("manage-preset-count")).toHaveText("1");

  // Another account: the preset is among the radios; chosen, its permissions are listed without switches.
  await second.getByTestId("manage-account-permissions").click();
  await expect(dialog).toContainText(emails[1]);
  const radio = dialog.getByRole("radio", { name, exact: true });
  await expect(radio).toBeVisible();
  await expect(radio).not.toBeChecked();
  await radio.check();
  await expect(saveAs).toBeHidden();
  for (const id of ["create_courses", "ai_courses", "ai_tutor", "share_courses", "skip_levels", "privacy", "superuser"]) {
    await expect(toggle(id)).toBeHidden();
    await expect(line(id).getByTestId("manage-permission-mark")).toBeVisible();
    await expect(line(id)).toHaveAttribute("data-on", id === "ai_tutor" || id === "skip_levels" ? "true" : "false");
  }
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${emails[1]}`);
  await expect(second.getByTestId("manage-account-preset")).toHaveText(name);
  await expect(second).toHaveAttribute("data-perms", "20");
  await expect(saved.getByTestId("manage-preset-count")).toHaveText("2");

  // The same name again, in another case: refused, and the account is as it was.
  await second.getByTestId("manage-account-permissions").click();
  await expect(radio).toBeChecked();
  await preset("custom").check();
  await expect(saveAs).toHaveValue("");
  await saveAs.fill(name.toUpperCase());
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-error")).toHaveText("A preset with that name already exists.");
  await expect(second.getByTestId("manage-account-preset")).toHaveText(name);
  await expect(saved).toHaveCount(1);

  // Delete it: a dialog asks first, and both accounts say Custom with what they had.
  await saved.getByTestId("manage-preset-delete").click();
  const confirm = page.getByTestId("manage-preset-dialog");
  await expect(confirm).toBeVisible();
  await expect(confirm).toContainText(name);
  await expect(confirm).toContainText("Accounts that have this preset keep their permissions and show as Custom.");
  await confirm.getByTestId("manage-preset-confirm").click();
  await expect(page.getByTestId("manage-notice")).toContainText("Preset deleted.");
  await expect(saved).toHaveCount(0);
  for (const row of [first, second]) {
    await expect(row.getByTestId("manage-account-preset")).toHaveText("Custom");
    await expect(row).toHaveAttribute("data-preset", "custom");
    await expect(row).toHaveAttribute("data-perms", "20");
  }
  await first.getByTestId("manage-account-permissions").click();
  await expect(preset("custom")).toBeChecked();
  await expect(dialog.getByRole("radio", { name, exact: true })).toHaveCount(0);
});
