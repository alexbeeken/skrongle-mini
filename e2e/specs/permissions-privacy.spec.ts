// Learners' progress on Manage Users, and Privacy (#147): the owner opens
// Progress on a Student's row and sees the course it studies with a date;
// given Privacy, the row says Private, the progress address is a 404 and the
// account's profile says its progress is private.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { passReadingCheck } from "../lib/answer";
import { devLogin } from "../lib/app";
import { AnswerKey } from "../lib/key";
import { apiProgress, newAccount, openCourse, signUp } from "../lib/learner";

const COURSE = "spanish";

/** Open the Permissions dialog on `row`, let `choose` set it and save. */
async function setPermissions(page: Page, row: Locator, email: string, choose: (dialog: Locator) => Promise<void>): Promise<void> {
  await row.getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await expect(dialog).toBeVisible();
  await choose(dialog);
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${email}`);
}

/** What the profile's "Who sees what" dialog says about the account's progress. */
async function progressVisibility(page: Page): Promise<Locator> {
  await page.goto("/profile");
  await page.getByTestId("profile-privacy-info").click();
  const line = page.getByTestId("profile-progress-visibility");
  await expect(line).toBeVisible();
  return line;
}

test("the owner sees a Student's progress; Privacy hides it and the profile says so", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("privacy");
  const email = a.email.toLowerCase();
  await signUp(them, a);
  // A new account is a Teacher, who has Privacy.
  await expect(await progressVisibility(them)).toContainText("Your study progress is private");

  await devLogin(page);
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: email });
  await expect(row.getByTestId("manage-account-private")).toHaveText("Private");
  await expect(row.getByTestId("manage-account-progress")).toHaveCount(0);
  // An owner's row (the dev user's own) is private too.
  const own = page.getByTestId("manage-account-row").filter({ hasText: "dev@example.com" });
  await expect(own.getByTestId("manage-account-private")).toBeVisible();
  await expect(own.getByTestId("manage-account-progress")).toHaveCount(0);

  // A Student has no Privacy: the row gets the Progress link.
  await setPermissions(page, row, email, (dialog) => dialog.locator('[data-testid=manage-permissions-preset][data-preset="1"]').check());
  await expect(row.getByTestId("manage-account-preset")).toHaveText("Student");
  await expect(row.getByTestId("manage-account-private")).toHaveCount(0);
  await expect(row.getByTestId("manage-account-progress")).toHaveText("Progress");
  await expect(await progressVisibility(them)).toContainText("can see which courses you study");

  // Nothing opened yet.
  await row.getByTestId("manage-account-progress").click();
  await expect(page).toHaveURL(/\/manage\/progress\?email=/);
  await expect(page.getByTestId("manage-progress-email")).toHaveText(email);
  await expect(page.getByTestId("manage-progress-empty")).toHaveText("This account has not opened a course yet.");
  await expect(page.getByTestId("manage-progress-course")).toHaveCount(0);

  // The Student opens Spanish and reads its first lesson.
  await openCourse(them, COURSE);
  const first = (await apiProgress(them))[0];
  await them.goto(`/lessons/${first.slug}`);
  await passReadingCheck(them, new AnswerKey(COURSE));

  // The owner: Spanish, nothing passed yet of its lessons, studied on a date.
  await page.reload();
  const course = page.getByTestId("manage-progress-course");
  await expect(course).toHaveCount(1);
  await expect(course).toHaveAttribute("data-course", COURSE);
  await expect(course.getByTestId("manage-progress-course-title")).toHaveText("Spanish");
  await expect(course.getByTestId("manage-progress-passed")).toHaveText(/^0 of \d+$/);
  await expect(course.getByTestId("manage-progress-last-studied")).toHaveText(/\d{4}/);
  await expect(course.getByTestId("manage-progress-streak")).toHaveText(/^\d+ days?$/);
  const address = new URL(page.url());
  await page.getByTestId("manage-progress-back").click();
  await expect(page).toHaveURL(/\/manage$/);

  // Privacy (Custom): the row says Private and the page is gone.
  await setPermissions(page, row, email, async (dialog) => {
    await dialog.locator('[data-testid=manage-permissions-preset][data-preset="custom"]').check();
    await dialog.locator('[data-testid=manage-permission][data-perm="privacy"]').getByTestId("manage-permission-toggle").check();
  });
  await expect(row.getByTestId("manage-account-preset")).toHaveText("Custom");
  await expect(row).toHaveAttribute("data-perms", "32");
  await expect(row.getByTestId("manage-account-private")).toHaveText("Private");
  await expect(row.getByTestId("manage-account-progress")).toHaveCount(0);
  expect((await page.request.get(address.pathname + address.search)).status()).toBe(404);
  // The account is told.
  await expect(await progressVisibility(them)).toContainText("Your study progress is private");
  await them.context().close();
});
