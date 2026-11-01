// Skrongle AI (#299): the server's own key, shared by every account. The e2e
// server runs with SKRONGLE_AI_KEY=sk-ant-dev-platform (scripts/e2e.sh), the
// dev simulator's platform key, which dev mode's default Anthropic base URL
// sends to /dev/ai: the app only knows "the server's key". A learner with
// no key of their own uses the tutor and the generator on it; the
// simulator's platform limit reaches them as a plain sentence. Everyone's
// spend today against the daily cap shows to superusers only, on
// /superuser (#623). Exact: no real key, no network.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, generationEnds } from "../lib/app";
import { newAccount, register } from "../lib/learner";
import { devRequest } from "../lib/mail";

interface AiCall { id: number; who: string; purpose: string; status: number }

/** The simulator's log, newest first (read as the dev user, a developer). */
async function simulatorCalls(): Promise<AiCall[]> {
  const r = await (await devRequest()).get("/dev/ai?format=json");
  expect(r.status(), "GET /dev/ai").toBe(200);
  return (await r.json()).calls;
}

/** The platform key's card on /dev/ai: `do=exhaust|lift|reset|…`. */
async function platform(form: Record<string, string>): Promise<void> {
  const r = await (await devRequest()).post("/dev/ai/profiles/platform", { form, maxRedirects: 0 });
  expect(r.status(), `platform ${form.do}`).toBe(303);
}

/** A new learner with no key of their own. */
async function learner(page: Page): Promise<void> {
  await register(page, newAccount("ai"), "spanish");
}

test.describe.configure({ mode: "serial" });

test.afterEach(async () => {
  await platform({ do: "lift" }).catch(() => {});
  await platform({ do: "clear-inject" }).catch(() => {});
});

 test("a learner with no key tutors and generates on Skrongle AI", async ({ page }) => {
  test.setTimeout(120_000);
  await learner(page);
  // The profile has no usage card and no plan section: everyone has Skrongle AI.
  await page.goto("/profile");
  await expect(page.getByTestId("profile-ai-usage")).toHaveCount(0);
  await expect(page.getByTestId("profile-subscription")).toHaveCount(0);

  // The tutor: the home card goes straight there (no red !, no modal),
  // and Skrongle AI is the only model.
  await page.goto("/");
  await expect(page.getByTestId("home-ai-blocked")).toHaveCount(0);
  await expect(page.getByTestId("home-info-ai")).toBeVisible();
  await page.getByTestId("home-start-tutor").click();
  await expect(page).toHaveURL(/\/tutor$/);
  await expect(page.locator("body")).toHaveAttribute("data-provider", "hosted");
  await expect(page.getByTestId("ai-disclosure")).toContainText("the AI provider this server uses");
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible();

  // The generator: Skrongle AI with no choice shown, and no dollar amounts.
  await page.goto("/courses");
  await createCourse(page);
  await expect(page.getByTestId("generate-provider-only")).toHaveValue("hosted");
  await expect(page.getByTestId("generate-estimate")).toHaveText(/^6 lessons · about 5 minutes$/);
  const before = (await simulatorCalls())[0]?.id ?? 0;
  await page.getByTestId("generate-wish").fill("Fractions on Skrongle AI, for the hosted spec");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");

  // The simulator served them on the platform key.
  const calls = (await simulatorCalls()).filter((c) => c.id > before && c.who === "platform");
  expect(calls.map((c) => c.purpose)).toEqual(expect.arrayContaining(["generate-outline", "generate-lesson"]));
  expect(calls.every((c) => c.status === 200)).toBe(true);
});

test("the platform key's limit reaches the learner as a plain sentence", async ({ page }) => {
  test.setTimeout(90_000);
  await learner(page);
  await platform({ do: "exhaust" });

  await page.goto("/tutor");
  await expect(page.getByTestId("tutor-thread")).toContainText("Skrongle AI is not available right now");

  await page.goto("/courses");
  await createCourse(page);
  await page.getByTestId("generate-wish").fill("Fractions while the platform key is used up");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("failed");
  await expect(page.getByTestId("generate-error")).toContainText("Skrongle AI is not available right now");
  // Not the own-key wording: the learner has no provider account to check.
  await expect(page.getByTestId("generate-error")).not.toContainText("Check your plan and billing with the provider");
  const refused = (await simulatorCalls()).find((c) => c.who === "platform");
  expect(refused?.status).toBe(400);
});

test("only a superuser sees everyone's Skrongle AI spend today", async ({ page, browser }) => {
  test.setTimeout(90_000);
  // A learner spends some: the tutor's opening turn is on the server's key.
  await learner(page);
  await page.goto("/tutor");
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible();
  expect((await page.request.get("/superuser")).status()).toBe(404);

  // The dev user is a superuser: the day's figure, in dollars, against the cap.
  const su = await (await browser.newContext()).newPage();
  await devLogin(su);
  await su.goto("/superuser");
  const spent = su.getByTestId("superuser-ai-today-spent");
  await expect(spent).toHaveText(/^\$\d+\.\d+ of \$10\.00 · resets at midnight UTC$/);
  await expect(spent).not.toHaveText(/^\$0\.00 /);
  const used = Number(await su.getByTestId("superuser-ai-today-bar").getAttribute("data-used"));
  expect(used).toBeGreaterThanOrEqual(0);
  expect(used).toBeLessThanOrEqual(100);
  await su.getByTestId("superuser-ai-today-info").click();
  await expect(su.getByTestId("superuser-ai-today-dialog")).toContainText("Only superusers see this");
  await su.getByTestId("superuser-ai-today-close").click();
  await expect(su.getByTestId("superuser-ai-today-dialog")).toBeHidden();
});
