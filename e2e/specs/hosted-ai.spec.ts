// Skrongle AI (#299): the server's own key, metered per learner. The e2e
// server runs with SKRONGLE_AI_KEY=sk-ant-dev-platform (scripts/e2e.sh), the
// dev simulator's platform key, which dev mode's default Anthropic base URL
// sends to /dev/ai: the app only knows "the server's key". A paid learner
// with no key of their own uses the tutor and the generator on it and sees
// the month's bar go down (never a dollar amount, #315); the simulator's
// platform limit reaches them as a plain sentence; Free and Basic (only
// Plus has an allowance; credits work on any plan, #931) are not offered it
// without credits, and the tutor card's red ! and "Start a
// conversation" modal say why (#283); and credit packs are not sold without
// Stripe. Everyone's spend today against the daily cap shows to superusers
// only, on /superuser (#623). Exact: no real key, no network.
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

/** A new learner with no key of their own, on the paid plan (the dev grant) unless `free`. */
async function learner(page: Page, free = false): Promise<void> {
  await register(page, newAccount(free ? "free-ai" : "paid-ai"), "spanish");
  if (!free) {
    expect((await page.request.post("/dev/premium", { form: { premium: "1" }, maxRedirects: 0 })).status()).toBe(303);
  }
}

/** "83 %" (what is left this month) anywhere in the text, as a number. Dollars never appear. */
function percent(text: string | null): number {
  expect(text ?? "").not.toContain("$");
  const m = (text ?? "").match(/(\d+) %/);
  expect(m, `a percentage in ${JSON.stringify(text)}`).not.toBeNull();
  return Number(m![1]);
}

/** The profile's bar (Plus only): what is left this month, 0–100. */
async function profileLeft(page: Page): Promise<number> {
  await page.goto("/profile#ai-usage");
  await expect(page.getByTestId("profile-ai-usage")).toHaveAttribute("data-offered", "true");
  const bar = page.getByTestId("profile-ai-bar");
  const left = Number(await bar.getAttribute("data-left"));
  expect(Number(await bar.getAttribute("aria-valuenow"))).toBe(100 - left);
  expect(percent(await page.getByTestId("profile-ai-left").textContent())).toBe(left);
  return left;
}

test.describe.configure({ mode: "serial" });

test.afterEach(async () => {
  await platform({ do: "lift" }).catch(() => {});
  await platform({ do: "clear-inject" }).catch(() => {});
});

test("a Plus learner with no key tutors and generates on Skrongle AI; the month's bar goes down", async ({ page }) => {
  test.setTimeout(120_000);
  await learner(page);
  const start = await profileLeft(page);
  expect(start).toBeGreaterThan(0);
  // Plus: the own-keys pill, off, and the key rows folded away behind it.
  await expect(page.getByTestId("profile-own-keys-switch")).toHaveAttribute("aria-checked", "false");
  await expect(page.getByTestId("profile-own-keys-body")).toBeHidden();

  // The tutor: the home card goes straight there (no red !, no modal),
  // Skrongle AI is the only model, and the header counts down.
  await page.goto("/");
  await expect(page.getByTestId("home-ai-blocked")).toHaveCount(0);
  await expect(page.getByTestId("home-info-ai")).toBeVisible();
  await page.getByTestId("home-start-tutor").click();
  await expect(page).toHaveURL(/\/tutor$/);
  await expect(page.locator("body")).toHaveAttribute("data-provider", "hosted");
  await expect(page.getByTestId("ai-disclosure")).toContainText("the AI provider this server uses");
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible();
  await expect(async () => expect(percent(await page.getByTestId("tutor-ai-left").textContent())).toBeLessThan(start)).toPass();

  // The generator: Skrongle AI with no choice shown, what is left and about what a lesson takes.
  await page.goto("/courses");
  await createCourse(page);
  await expect(page.getByTestId("generate-provider-only")).toHaveValue("hosted");
  await expect(page.getByTestId("generate-ai-left")).toContainText(/Skrongle AI left this month: \d+ % · about \d+ % a lesson/);
  await expect(page.getByTestId("generate-ai-left")).not.toContainText("$");
  // The estimate is a share of the month, never dollars (#443).
  await expect(page.getByTestId("generate-estimate")).toHaveText(/^6 lessons · about 5 minutes · about \d+ % of this month's Skrongle AI$/);
  const before = (await simulatorCalls())[0]?.id ?? 0;
  await page.getByTestId("generate-wish").fill("Fractions on Skrongle AI, for the hosted spec");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");

  // The simulator served them on the platform key.
  const calls = (await simulatorCalls()).filter((c) => c.id > before && c.who === "platform");
  expect(calls.map((c) => c.purpose)).toEqual(expect.arrayContaining(["generate-outline", "generate-lesson"]));
  expect(calls.every((c) => c.status === 200)).toBe(true);

  const after = await profileLeft(page);
  expect(after).toBeLessThan(start);
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

/** Why there is no Skrongle AI (#283): the plan includes none, so an own key or Plus. */
async function noneIncluded(scope: ReturnType<Page["getByTestId"]>): Promise<void> {
  await expect(scope.getByTestId("hosted-gap-reason")).toHaveText("Your plan includes no Skrongle AI.");
  await expect(scope.getByTestId("hosted-gap-reason")).toHaveAttribute("data-none", "true");
  await expect(scope.getByTestId("hosted-gap-instead")).toContainText("upgrade to Plus");
  await expect(scope.getByTestId("hosted-gap-upgrade")).toHaveAttribute("href", "/profile?plans=1#subscription");
}

test("a plan with no Skrongle AI (Free) is not offered it, and the tutor card says why", async ({ page }) => {
  await learner(page, true);
  // The home page's tutor card: a red ! whose tip says so, and "Start a
  // conversation" opens the tutor's "add an API key" page as a modal.
  await page.goto("/");
  await expect(page.getByTestId("home-ai-blocked")).toHaveAttribute("data-tip", "No AI keys set");
  await expect(page.getByTestId("home-info-ai")).toHaveCount(0);
  await page.getByTestId("home-start-tutor").click();
  const gate = page.getByTestId("home-tutor-gate");
  await expect(gate).toBeVisible();
  await expect(page).toHaveURL(/\/$/);
  await noneIncluded(gate);
  await expect(gate.getByTestId("home-tutor-gate-add-key")).toHaveAttribute("href", "/profile#api-keys");
  await gate.getByTestId("home-tutor-gate-close").click();
  await expect(gate).toBeHidden();
  // The red ! opens the same modal.
  await page.getByTestId("home-ai-blocked").click();
  await expect(gate).toBeVisible();

  await page.goto("/tutor");
  await expect(page.getByTestId("tutor-no-key")).toBeVisible();
  await noneIncluded(page.getByTestId("tutor-no-key"));
  expect((await page.request.post("/api/tutor/start", { data: { provider: "hosted" } })).status()).toBe(400);

  await page.goto("/courses");
  await createCourse(page);
  await expect(page.getByTestId("generate-no-key")).toBeVisible();
  await expect(page.locator('[data-testid="generate-provider"][value="hosted"], [data-testid="generate-provider-only"]')).toHaveCount(0);
  await noneIncluded(page.getByTestId("generate-no-key"));

  // The usage card is for Plus only; the API keys card has no own-keys pill.
  await page.goto("/profile#api-keys");
  await expect(page.getByTestId("profile-ai-usage")).toHaveCount(0);
  await expect(page.getByTestId("profile-own-keys-switch")).toHaveCount(0);
});

test("Basic, the CA$3 plan, has no Skrongle AI either: the tutor and the generator want an own key (#931)", async ({ page }) => {
  await learner(page, true);
  // Buy Basic the way a learner does: the plans chooser, then the dev Checkout.
  await page.goto("/profile#subscription");
  await page.getByTestId("profile-plan-choose").click();
  const dialog = page.getByTestId("plans-dialog");
  await dialog.locator('[data-testid="plan-card"][data-plan="basic"]').click();
  await dialog.getByTestId("plans-purchase").click();
  await expect(page).toHaveURL(/\/dev\/payments\/checkout\/cs_test_dev\w+$/);
  await page.getByTestId("dev-checkout-pay").click();
  await expect(page.getByTestId("profile-subscription")).toHaveAttribute("data-plan", "basic");

  await page.goto("/tutor");
  await expect(page.getByTestId("tutor-no-key")).toBeVisible();
  await noneIncluded(page.getByTestId("tutor-no-key"));
  expect((await page.request.post("/api/tutor/start", { data: { provider: "hosted" } })).status()).toBe(400);

  await page.goto("/courses");
  await createCourse(page);
  await expect(page.getByTestId("generate-no-key")).toBeVisible();
  await expect(page.locator('[data-testid="generate-provider"][value="hosted"], [data-testid="generate-provider-only"]')).toHaveCount(0);

  await page.goto("/profile#subscription");
  await expect(page.getByTestId("profile-ai-usage")).toHaveCount(0);
  await expect(page.getByTestId("profile-own-keys-switch")).toHaveCount(0);
});

test("credit packs are not sold without Stripe: the checkout answers 400", async ({ page }) => {
  await learner(page);
  await page.goto("/profile#ai-usage");
  await expect(page.getByTestId("profile-ai-buy")).toHaveCount(0);
  const r = await page.request.post("/billing/credits", { form: { pack: "price_anything" }, maxRedirects: 0 });
  expect(r.status()).toBe(400);
});

test("only a superuser sees everyone's Skrongle AI spend today; a learner sees their own bar", async ({ page, browser }) => {
  test.setTimeout(90_000);
  // A Plus learner spends some: the tutor's opening turn is on the server's key.
  await learner(page);
  await page.goto("/tutor");
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible();
  await page.goto("/profile");
  await expect(page.getByTestId("profile-ai-bar")).toBeVisible();
  await expect(page.getByTestId("profile-ai-usage")).not.toContainText("$");
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
