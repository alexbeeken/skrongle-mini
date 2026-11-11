// AI per account (#145): two permissions on Manage Users' Permissions dialog.
// "Use the AI tutor" off takes the tutor card, "Tutor me" and /tutor from
// that account and leaves AI course creation; "Use AI in courses" off takes
// the Generate dialog, Use AI and Copy and translate and leaves the tutor.
// The server-wide "Hide AI features" switch is ai-off.spec.ts.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, register } from "../lib/learner";

const LESSON = "/lessons/adjectives-agreement-position";
const NOT_ALLOWED = "Your account is not allowed to do this";

/** The owner (`page`) sets the account's permissions through the Permissions
 * dialog on its row: Custom, each named switch on or off, Save. */
async function setPermissions(page: Page, email: string, set: Record<string, boolean>) {
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: email });
  await row.getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await expect(dialog).toBeVisible();
  await dialog.locator('[data-testid=manage-permissions-preset][data-preset="custom"]').check();
  for (const [id, on] of Object.entries(set)) {
    await dialog.locator(`[data-testid=manage-permission][data-perm="${id}"]`).getByTestId("manage-permission-toggle").setChecked(on);
  }
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${email}`);
  await expect(row.getByTestId("manage-account-preset")).toHaveText("Custom");
}

const tutorCard = (page: Page) => page.locator('[data-testid="home-mode"][data-mode="ai"]');

test("the tutor and AI in courses are allowed per account, each apart from the other", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const account = newAccount("perms-ai");
  const email = account.email.toLowerCase();
  await register(them, account, "spanish");
  await devLogin(page, "keys=fake&next=/manage");

  // "Use the AI tutor" off: no tutor anywhere, and the route refuses.
  await setPermissions(page, email, { ai_tutor: false });
  await them.goto("/");
  await expect(them.getByTestId("home-mode")).toHaveCount(4);
  await expect(tutorCard(them)).toHaveCount(0);
  await expect(them.getByTestId("home-start-tutor")).toHaveCount(0);
  await them.goto(LESSON);
  await expect(them.getByTestId("lesson")).toBeVisible();
  await expect(them.getByTestId("lesson-tutor")).toHaveCount(0);
  await expect(them.locator('a[href*="/tutor"]')).toHaveCount(0);
  const refused = await them.goto("/tutor");
  expect(refused?.status()).toBe(403);
  await expect(them.locator("main")).toContainText(NOT_ALLOWED);
  await expect(them.locator("main")).not.toContainText("turned off on this server");
  expect((await them.request.get("/api/tutor/session?id=x")).status()).toBe(403);
  // AI in courses is still theirs: the wording, the translate control, the Generate dialog.
  await them.goto("/courses");
  await expect(them.locator("main")).toContainText("Have AI write it");
  await expect(them.getByTestId("courses-translate")).not.toHaveCount(0);
  await them.getByTestId("courses-create").click();
  await expect(them.getByTestId("generate")).toBeVisible();
  await expect(them.getByTestId("generate-form")).toBeVisible();
  expect((await them.request.get("/api/generate/nope")).status()).not.toBe(403);

  // The tutor back and "Use AI in courses" off: the reverse.
  await setPermissions(page, email, { ai_tutor: true, ai_courses: false });
  await them.goto("/");
  await expect(them.getByTestId("home-mode")).toHaveCount(5);
  await expect(tutorCard(them)).toBeVisible();
  await them.goto(LESSON);
  await them.getByTestId("lesson-more-toggle").click();
  await expect(them.getByTestId("lesson-tutor")).toBeVisible();
  expect((await them.goto("/tutor"))?.status()).toBe(200);
  await them.goto("/courses");
  await expect(them.locator("main")).not.toContainText(/let an AI write|Have AI write it/);
  await expect(them.getByTestId("courses-translate")).toHaveCount(0);
  await expect(them.getByTestId("courses-translate-dialog")).toHaveCount(0);
  // Create a course: the editor, without the Generate dialog or Use AI.
  await them.getByTestId("courses-create").click();
  await expect(them).toHaveURL(/\/courses\/[^/?]+\?generate=1$/);
  await expect(them.getByTestId("course-edit")).toBeVisible();
  await expect(them.getByTestId("generate")).toHaveCount(0);
  await expect(them.getByTestId("course-ai")).toHaveCount(0);
  const courseId = await them.getByTestId("course-edit").getAttribute("data-course-id");
  const gen = await them.request.post(`/courses/${courseId}/generate`, { form: { provider: "anthropic", wish: "x" }, maxRedirects: 0 });
  expect(gen.status()).toBe(403);
  expect(await gen.text()).toContain(NOT_ALLOWED);
  expect((await them.request.get("/api/generate/nope")).status()).toBe(403);

  // Both on again: everything is back.
  await setPermissions(page, email, { ai_courses: true });
  await them.goto(`/courses/${courseId}?generate=1`);
  await expect(them.getByTestId("generate")).toBeVisible();
  await them.goto("/");
  await expect(tutorCard(them)).toBeVisible();
  await them.context().close();
});
