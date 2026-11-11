// The Share courses permission (#144): an account makes an invitation link;
// the owner turns its Share courses off in the Permissions dialog on Manage
// Users (Custom); the account's course page still has Share, and the dialog
// lists the link with Remove, but has no form to make another; the link
// still opens for someone else; with the last link removed, Share is gone.
// The route's side (403, an owner, a Student opening a link) is a Rust
// test in src/routes/share/tests.rs.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, makeShareLink, shareRow } from "../lib/app";
import { newAccount, signUp } from "../lib/learner";

/** As the owner on `page`: switch one permission of the account `email` on or off in the Permissions dialog (Custom) and save. */
async function setPermission(page: Page, email: string, perm: string, on: boolean): Promise<void> {
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: email });
  await row.getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await expect(dialog).toBeVisible();
  await dialog.locator('[data-testid=manage-permissions-preset][data-preset="custom"]').check();
  const toggle = dialog.locator(`[data-testid=manage-permission][data-perm="${perm}"]`).getByTestId("manage-permission-toggle");
  await toggle.setChecked(on);
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${email}`);
  await expect(row).toHaveAttribute("data-preset", "custom");
}

test("without Share courses an account keeps and removes its links but makes no new one", async ({ page, browser }) => {
  // A new account (a Teacher) makes a course and a link for it.
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("perms-share");
  const email = a.email.toLowerCase();
  await signUp(them, a);
  await them.goto("/courses");
  const id = await createCourse(them);
  await them.goto(`/courses/${id}?share=1`);
  const link = await makeShareLink(them, "Friend");
  await expect(shareRow(them, "Friend")).toBeVisible();

  // The owner turns Share courses off for it: Teacher (63) minus 8.
  await devLogin(page);
  await setPermission(page, email, "share_courses", false);
  await expect(page.getByTestId("manage-account-row").filter({ hasText: email })).toHaveAttribute("data-perms", "55");

  // Its course page still has Share; the dialog lists the link with Remove, and no form.
  await them.goto(`/courses/${id}`);
  await expect(them.getByTestId("course-edit-share")).toHaveText("Share (1)");
  await them.getByTestId("course-edit-share").click();
  const dialog = them.getByTestId("course-share");
  await expect(dialog).toBeVisible();
  await expect(shareRow(them, "Friend").getByTestId("course-share-remove")).toBeVisible();
  await expect(dialog.getByTestId("course-share-send")).toHaveCount(0);
  await expect(dialog.getByTestId("course-share-label")).toHaveCount(0);
  await expect(dialog.getByTestId("course-share-not-allowed")).toContainText("Your account cannot make new invitation links.");
  // The request itself is refused.
  expect((await them.request.post(`/courses/${id}/share`, { form: { label: "Another" }, maxRedirects: 0 })).status()).toBe(403);
  await dialog.getByTestId("course-share-close").click();
  await expect(dialog).toBeHidden();

  // A third account still opens the link: the course is on its list, active.
  const friend = await (await browser.newContext()).newPage();
  await signUp(friend, newAccount("perms-share-friend"));
  await friend.goto(link);
  await expect(friend).not.toHaveURL(/\/courses(\?|$)/);
  await friend.goto("/courses");
  const card = friend.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card).toHaveAttribute("data-active", "true");

  // The account removes its link: Share goes with it, and the friend's course.
  await them.goto(`/courses/${id}?share=1`);
  await shareRow(them, "Friend").getByTestId("course-share-remove").click();
  await expect(them).toHaveURL(new RegExp(`/courses/${id}\\?share=1&notice=share-removed$`));
  await expect(them.getByTestId("course-edit")).toBeVisible();
  await expect(them.getByTestId("course-edit-share")).toHaveCount(0);
  await expect(them.getByTestId("course-share")).toHaveCount(0);
  await friend.goto("/courses");
  await expect(card).toHaveCount(0);

  // Share courses back on: the button and the form are back.
  await setPermission(page, email, "share_courses", true);
  await them.goto(`/courses/${id}`);
  await them.getByTestId("course-edit-share").click();
  await expect(them.getByTestId("course-share").getByTestId("course-share-send")).toBeVisible();

  await friend.context().close();
  await them.context().close();
});
