// "Hide all AI features" (#147): a switch at sign-up (a pill with an info
// dot, over a checkbox) and "Do Not Show Me AI Features" on the profile's
// Privacy and consent card (#974). On, nothing on the site offers the AI tutor, AI course creation
// or AI theme creation, their routes refuse, and any saved AI API keys are
// deleted (the switch asks first when there is one). The profile turns it
// back off.
import { expect, test, type Page } from "@playwright/test";
import { login, newAccount, openCourse, register } from "../lib/learner";

const LESSON = "/lessons/adjectives-agreement-position";

async function expectAiHidden(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("home-mode")).toHaveCount(4);
  await expect(page.locator('[data-testid="home-mode"][data-mode="ai"]')).toHaveCount(0);
  await expect(page.getByTestId("home-start-tutor")).toHaveCount(0);
  await page.goto(LESSON);
  await expect(page.getByTestId("lesson")).toBeVisible();
  await expect(page.getByTestId("lesson-tutor")).toHaveCount(0);
  await expect(page.locator('a[href*="/tutor"]')).toHaveCount(0);
  await page.goto("/courses");
  await expect(page.locator("main")).not.toContainText(/let an AI write|Have AI write it/);
  await expect(page.getByTestId("courses-create")).toBeVisible();
  // Create a course: the editor, without the "Generate the course" dialog or Use AI.
  await page.getByTestId("courses-create").click();
  await expect(page).toHaveURL(/\/courses\/[^/?]+\?generate=1$/);
  await expect(page.getByTestId("course-edit")).toBeVisible();
  await expect(page.getByTestId("generate")).toHaveCount(0);
  await expect(page.getByTestId("course-ai")).toHaveCount(0);
  const courseId = await page.getByTestId("course-edit").getAttribute("data-course-id");
  // The routes refuse: a page goes to the profile with a notice, JSON is a 403.
  const gen = await page.request.post(`/courses/${courseId}/generate`, { form: { provider: "anthropic", wish: "x" }, maxRedirects: 0 });
  expect(gen.status()).toBe(303);
  expect(gen.headers()["location"]).toMatch(/\/profile\?ai=off#consent$/);
  await page.goto("/tutor");
  await expect(page).toHaveURL(/\/profile\?ai=off#consent$/);
  await expect(page.getByTestId("profile-notice")).toContainText("uses AI, which you have hidden");
  expect((await page.request.get("/api/tutor/session?id=x")).status()).toBe(403);
  expect((await page.request.get("/api/generate/nope")).status()).toBe(403);
  await page.goto("/profile");
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "true");
  await expect(page.getByTestId("consent-ai-switch")).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("profile-api-keys")).toBeHidden();
  await expect(page.locator('[data-testid="profile-nav-link"][data-section="api-keys"]')).toBeHidden();
}

async function expectAiShown(page: Page) {
  await page.goto("/");
  await expect(page.locator('[data-testid="home-mode"][data-mode="ai"]')).toBeVisible();
  await page.goto(LESSON);
  await page.getByTestId("lesson-more-toggle").click();
  await expect(page.getByTestId("lesson-tutor")).toBeVisible();
  await page.goto("/courses");
  await expect(page.locator("main")).toContainText("Have AI write it");
  await page.getByTestId("courses-create").click();
  await expect(page.getByTestId("generate")).toBeVisible();
  await expect(page.getByTestId("generate-form")).toBeVisible();
  await page.goto("/profile");
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "false");
  await expect(page.getByTestId("consent-ai-switch")).toHaveAttribute("aria-checked", "false");
  await expect(page.getByTestId("profile-api-keys")).toBeVisible();
}

test("the sign-up box hides every AI feature, and the profile turns them back on", async ({ page }) => {
  const a = newAccount("no-ai");
  await page.goto("/register");
  await expect(page.getByTestId("register-no-ai")).not.toBeChecked();
  await expect(page.getByTestId("register-no-ai-tip")).toBeHidden();
  await page.getByTestId("register-no-ai-info").hover();
  await expect(page.getByTestId("register-no-ai-tip")).toContainText("AI tutor");
  await page.mouse.move(0, 0);
  await expect(page.getByTestId("register-no-ai-tip")).toBeHidden();
  await page.getByTestId("register-email").fill(a.email);
  await page.getByTestId("register-password").fill(a.password);
  await page.getByTestId("register-age").check();
  await page.getByTestId("register-terms").check();
  await page.getByTestId("register-no-ai").check();
  await page.getByTestId("register-submit").click();
  await expect(page.getByTestId("register-sent")).toBeVisible();
  await login(page, a);
  await page.goto("/courses");
  await openCourse(page, "spanish");

  await expectAiHidden(page);

  // The switch works in place: no reload, the API keys card slides in.
  await page.goto("/profile");
  await page.getByTestId("consent-ai-switch").click();
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "false");
  await expect(page.getByTestId("profile-api-keys")).toBeVisible();
  await expect(page).toHaveURL(/\/profile$/);
  await expectAiShown(page);
});

test("the profile's switch is a negative, hides the AI features at once, and asks first when a key would be deleted", async ({ page }) => {
  await register(page, newAccount("ai-off"), "spanish");
  await expectAiShown(page);

  await page.goto("/profile");
  // One switch, on the Privacy and consent card, said as a negative; the old card is gone.
  await expect(page.locator("#consent-ai-label")).toHaveText("Do Not Show Me AI Features");
  await expect(page.locator("#ai")).toHaveCount(0);
  await expect(page.locator('[data-testid="profile-nav-link"][data-section="ai"]')).toHaveCount(0);
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "false");

  // No key on file: the switch hides everything straight away, no dialog; the keys card slides out.
  const dialog = page.getByTestId("profile-ai-off-dialog");
  await page.getByTestId("consent-ai-switch").click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "true");
  await expect(page.getByTestId("profile-api-keys")).toBeHidden();
  await expect(page).toHaveURL(/\/profile$/);
  await expectAiHidden(page);

  // Back off, save a key: now the switch warns that the key would be deleted.
  await page.goto("/profile");
  await page.getByTestId("consent-ai-switch").click();
  await expect(page.getByTestId("profile-api-keys")).toBeVisible();
  const row = page.locator('[data-testid="profile-api-key"][data-provider="anthropic"]');
  await row.getByTestId("profile-api-key-toggle").click();
  await row.getByTestId("profile-api-key-input").fill("sk-ant-test-paste-ABCD");
  await expect(row).toHaveAttribute("data-key-set", "true");
  await page.goto("/profile");
  await expect(dialog).not.toBeVisible();
  await page.getByTestId("consent-ai-switch").click();
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("delete all AI API keys saved in your profile");
  // Cancel changes nothing.
  await page.getByTestId("profile-ai-off-cancel").click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "false");
  await expect(row).toHaveAttribute("data-key-set", "true");

  await page.getByTestId("consent-ai-switch").click();
  await page.getByTestId("profile-ai-off-confirm").click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "true");
  await expect(row).toHaveAttribute("data-key-set", "false");
  await expectAiHidden(page);
  // The key is gone on the server too: shown again, the row is empty.
  await page.goto("/profile");
  await page.getByTestId("consent-ai-switch").click();
  await expect(page.getByTestId("profile-api-keys")).toBeVisible();
  await expect(row).toHaveAttribute("data-key-set", "false");
  await page.goto("/profile");
  await expect(row).toHaveAttribute("data-key-set", "false");
});
