// "Hide AI features for everyone" (#25): one switch, the owner's, on Manage
// Users. On, nothing on the site offers the AI tutor or AI course creation
// to any account, and their routes refuse; the keys stay. Off shows them
// again. Nobody but an owner sees the switch or the keys.
import { expect, test, type Browser, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, register, setAiHidden } from "../lib/learner";

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
  // The routes refuse: a page says the AI features are off on this server, JSON is a 403.
  const gen = await page.request.post(`/courses/${courseId}/generate`, { form: { provider: "anthropic", wish: "x" }, maxRedirects: 0 });
  expect(gen.status()).toBe(403);
  const tutor = await page.goto("/tutor");
  expect(tutor?.status()).toBe(403);
  await expect(page.locator("main")).toContainText("turned off on this server");
  expect((await page.request.get("/api/tutor/session?id=x")).status()).toBe(403);
  expect((await page.request.get("/api/generate/nope")).status()).toBe(403);
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
}

/** A second account, not an owner, in a browser of its own. */
async function learner(browser: Browser): Promise<Page> {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await register(page, newAccount("ai-off"), "spanish");
  return page;
}

test.afterEach(async ({ page }) => {
  // Whatever happened, the next spec finds the AI features shown.
  await devLogin(page, "keys=fake&next=/manage");
  await setAiHidden(page, false);
});

test("the owner's switch hides every AI feature from every account, and shows them again", async ({ page, browser }) => {
  // The owner's home is checked too, so open a course: a spec before this one
  // may have left the dev user with none (`/dev/reset`), and then `/` is the
  // course list, with no mode cards (#189).
  await devLogin(page, "keys=fake&open=spanish&next=/manage");
  const other = await learner(browser);
  await expectAiShown(other);

  // The switch is a positive on Manage Users' AI section, off by default.
  await page.goto("/manage#ai");
  await expect(page.locator("#manage-ai-label")).toHaveText("Hide AI features for everyone");
  await expect(page.getByTestId("manage-ai-switch")).toHaveAttribute("aria-checked", "false");
  await setAiHidden(page, true);
  await expect(page.getByTestId("manage-notice")).toContainText("hidden for every account");
  await expect(page.getByTestId("manage-ai-switch")).toHaveAttribute("aria-checked", "true");
  // The keys stay.
  await expect(page.locator('[data-testid="manage-api-key"][data-key-set="true"]')).toHaveCount(3);

  await expectAiHidden(other);
  await expectAiHidden(page);

  await setAiHidden(page, false);
  await expect(page.getByTestId("manage-notice")).toContainText("shown again");
  await expectAiShown(other);
  await other.context().close();
});

test("only the owner sees the keys and the switch; nobody has them on the profile", async ({ page, browser }) => {
  const other = await learner(browser);
  expect((await other.goto("/manage"))?.status()).toBe(404);
  expect((await other.request.post("/manage/ai", { form: { off: "1" }, maxRedirects: 0 })).status()).toBe(404);
  expect((await other.request.post("/api/manage/keys", { data: { key_anthropic: "sk-ant-dev-not-yours" } })).status()).toBe(404);
  await other.goto("/profile");
  await expect(other.getByTestId("profile-two-factor")).toBeVisible();
  await expect(other.locator('[data-testid^="manage-api-key"], [data-testid^="profile-api-key"], [data-testid^="consent-ai"]')).toHaveCount(0);
  await expect(other.locator('[data-testid="profile-nav-link"][data-section="api-keys"]')).toHaveCount(0);
  await other.context().close();

  await devLogin(page, "keys=fake&next=/profile");
  await expect(page.locator('[data-testid^="profile-api-key"], [data-testid^="consent-ai"]')).toHaveCount(0);
  await page.goto("/manage#ai");
  await expect(page.getByTestId("manage-ai-card")).toBeVisible();
  // One folded row per provider (#24); keys=fake sets the first three.
  const rows = page.getByTestId("manage-api-key");
  await expect(rows).toHaveCount(8);
  await expect(page.locator('[data-testid="manage-api-key"][data-key-set="true"]')).toHaveCount(3);
  for (const row of await rows.all()) {
    await expect(row).not.toHaveAttribute("open", "");
    await expect(row.getByTestId("manage-api-key-input")).toBeHidden();
  }
});
