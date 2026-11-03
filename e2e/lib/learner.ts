// A new learner and what the app says about their progress. Shared by the
// playthrough (e2e/playthrough/core.ts) and the smoke specs: a fresh account
// through /register, /api/progress, and the check that every page listing
// lessons agrees with it (the "one passed" rule, docs/progress.md).
import { expect, request, type Page } from "@playwright/test";

export interface Account {
  email: string;
  password: string;
}

/** The app's record of a lesson (/api/progress). */
export interface ApiLesson {
  slug: string;
  title: string;
  seen: boolean;
  passed: boolean;
  best_pct: number | null;
  status: string;
  required_answers: number;
}

let made = 0;
/** An address nobody has yet: `<prefix>-<time>-w<worker>-<n>-<random>@example.com`.
 * Each Playwright worker is its own process with its own counter, so the
 * worker index keeps two workers apart (#867). Outside a test run (the Jev
 * harness) there is no TEST_WORKER_INDEX and the process id stands in.
 * `worker` is for the unit test, which simulates several workers. */
export function newAccount(prefix: string, worker: string | number = process.env.TEST_WORKER_INDEX ?? `p${process.pid}`): Account {
  const stamp = new Date().toISOString().replace(/\D/g, "").slice(4, 14);
  const rand = Math.random().toString(36).slice(2, 8);
  return { email: `${prefix}-${stamp}-w${worker}-${++made}-${rand}@example.com`, password: `pw-${Math.random().toString(36).slice(2, 12)}` };
}

/** Register `account` through /register, sign in (where registering did
 * not), and open `course` on /courses. */
export async function register(page: Page, account: Account, course: string): Promise<void> {
  if (!(await signUp(page, account))) await login(page, account);
  await openCourse(page, course);
}

/** Fill in and send /register (ticking both boxes).
 * With mail the answer is "check your mail" and nobody is signed in (#379):
 * `false`. In the mini lane (no mail) registering signs in: `true`. */
export async function signUp(page: Page, account: Account): Promise<boolean> {
  await page.goto("/register");
  await page.getByTestId("register-email").fill(account.email);
  await page.getByTestId("register-password").fill(account.password);
  await page.getByTestId("register-age").check();
  await page.getByTestId("register-submit").click();
  await page.waitForLoadState("domcontentloaded");
  if (await page.getByTestId("register-error").isVisible()) throw new Error(`registration failed: ${await page.getByTestId("register-error").innerText()}`);
  if (process.env.E2E_MINI === "1") {
    await expect(page.getByTestId("nav-account-toggle")).toBeVisible();
    return true;
  }
  await expect(page.getByTestId("register-sent")).toBeVisible();
  return false;
}

/**
 * Open `course` on /courses. On a course not opened yet, its button opens the
 * settings wizard over the list (#587): with `wizard` set it is left open
 * there; otherwise the course is opened as "Get Started!" would, with the
 * settings it starts with, and the page goes to its home. A course already
 * opened goes straight home, past a first-visit wizard (#574) unless `wizard`.
 */
export async function openCourse(page: Page, course: string, { wizard = false } = {}): Promise<void> {
  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${course}"]`);
  if (!(await card.count())) throw new Error(`no course "${course}" on /courses`);
  const startWizard = card.locator('[data-testid="courses-settings-dialog"][data-start]');
  if (await startWizard.count()) {
    await card.getByTestId("courses-open").click();
    await expect(startWizard).toBeVisible();
    if (wizard) return;
    const r = await page.request.post(`/courses/open/${course}`, { form: { wizard: "1" }, maxRedirects: 0 });
    if (r.status() !== 303) throw new Error(`starting ${course}: HTTP ${r.status()}`);
    await page.goto("/");
    return;
  }
  await card.getByTestId("courses-open").click();
  await page.waitForLoadState("domcontentloaded");
  if (wizard) return;
  await skipWizard(page);
}

/**
 * The page is a course home showing the course's first-visit settings
 * wizard (#574): count the course as set up, as closing the wizard does,
 * and load the page again without it. Any other page is left alone.
 */
export async function skipWizard(page: Page): Promise<void> {
  const wizard = page.locator('[data-testid="courses-settings-dialog"][data-wizard]');
  if (!(await wizard.count())) return;
  const course = await wizard.getAttribute("data-course-id");
  const r = await page.request.post(`/courses/${course}/settings`, { form: { next: "home" }, maxRedirects: 0 });
  if (r.status() !== 303) throw new Error(`setting up ${course}: HTTP ${r.status()}`);
  await page.reload();
}

/** Wear motivator `id` in `course` (#574): its settings, posted as the gear's form does. */
export async function wearMotivator(page: Page, course: string, id: string): Promise<void> {
  const r = await page.request.post(`/courses/${course}/settings`, { form: { motivator: id }, maxRedirects: 0 });
  expect(r.status(), `wear ${id} in ${course}`).toBe(303);
}

/** Pick motivator `id` behind `course`'s gear on the course list, and Save. */
export async function pickMotivator(page: Page, course: string, id: string): Promise<void> {
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator(`[data-course-id="${course}"]`).getByTestId("courses-settings").click();
  const dialog = page.locator(`[data-testid="courses-settings-dialog"][data-course-id="${course}"]`);
  await dialog.locator(`[data-testid="courses-settings-motivator"][data-motivator-id="${id}"]`).click();
  const saved = page.waitForResponse((r) => r.url().endsWith(`/courses/${course}/settings`) && r.request().method() === "POST");
  await dialog.getByTestId("courses-settings-save").click();
  expect((await saved).status()).toBeLessThan(400);
  await expect(dialog).toBeHidden();
}

export async function login(page: Page, account: Account): Promise<void> {
  await page.goto("/login");
  await page.getByTestId("login-email").fill(account.email);
  await page.getByTestId("login-password").fill(account.password);
  await page.getByTestId("login-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", account.email);
}

export async function logout(page: Page): Promise<void> {
  await page.goto("/");
  // Wait for the redirect to /login itself: the page already shown counts as
  // loaded, and a goto racing the redirect is interrupted by it.
  const atLogin = page.waitForURL(/\/login(\?|$)/);
  await page.getByTestId("nav-logout-form").evaluate((f: HTMLFormElement) => f.requestSubmit());
  await atLogin;
}

export async function apiProgress(page: Page): Promise<ApiLesson[]> {
  const r = await page.request.get("/api/progress");
  if (!r.ok()) throw new Error(`GET /api/progress: HTTP ${r.status()}`);
  return (await r.json()).lessons;
}

/** Lesson `slug` as /api/progress reports it (it must be there). */
export async function progressOf(page: Page, slug: string): Promise<ApiLesson> {
  const l = (await apiProgress(page)).find((x) => x.slug === slug);
  expect(l, `${slug} in /api/progress`).toBeDefined();
  return l!;
}

/** Pages that list lessons with data-lesson-id and data-passed. */
export const PROGRESS_PAGES = ["/sprint", "/guide", "/lessons?unit=all", "/map"];

/**
 * Where a page disagrees with /api/progress (`truth`) on what is passed: one
 * message per lesson mark that differs, and one per page that won't open
 * (`status`). `extra` pages are checked too (e.g. the last lesson's page).
 */
export async function progressMismatches(page: Page, truth: ApiLesson[], extra: string[] = []): Promise<{ path: string; status: number; what: string }[]> {
  const passed = new Map(truth.map((l) => [l.slug, l.passed]));
  const out: { path: string; status: number; what: string }[] = [];
  for (const path of [...PROGRESS_PAGES, ...extra]) {
    const r = await page.goto(path);
    if (!r || r.status() >= 400) {
      out.push({ path, status: r?.status() ?? 0, what: `${path} returned HTTP ${r?.status() ?? "nothing"}` });
      continue;
    }
    const marks = await page.locator("[data-lesson-id][data-passed]").evaluateAll((els) =>
      els.map((e) => ({ slug: e.getAttribute("data-lesson-id")!, passed: e.getAttribute("data-passed") === "true", testid: e.getAttribute("data-testid") ?? "" })),
    );
    for (const m of marks) {
      const want = passed.get(m.slug);
      if (want !== undefined && want !== m.passed)
        out.push({ path, status: 200, what: `${path} shows ${m.slug} as ${m.passed ? "passed" : "not passed"} [${m.testid}], /api/progress says ${want ? "passed" : "not passed"}` });
    }
  }
  return out;
}

/** In the mode's lesson list (`index`, items `item`), the first lesson not passed must not be locked. */
export async function lockedNext(page: Page, index: string, item: string, truth: ApiLesson[]): Promise<string | undefined> {
  const passed = new Map(truth.map((l) => [l.slug, l.passed]));
  await page.goto(index);
  const items = await page.getByTestId(item).evaluateAll((els) => els.map((e) => ({ slug: e.getAttribute("data-lesson-id")!, locked: e.getAttribute("data-state") === "locked" })));
  const i = items.findIndex((x) => !passed.get(x.slug));
  if (i >= 0 && items[i].locked) return `${items[i].slug} is still locked on ${index} after ${items[i - 1]?.slug ?? "the lesson before"} passed`;
  return undefined;
}

/** Every page agrees with /api/progress, and the next lesson is open in each mode; fails the test with the list otherwise. */
export async function expectProgressAgrees(page: Page): Promise<ApiLesson[]> {
  const truth = await apiProgress(page);
  const problems = (await progressMismatches(page, truth)).map((p) => p.what);
  for (const [index, item] of [["/sprint?reduce-motion=1", "sprint-lesson"], ["/guide", "guide-stage"], ["/lessons?unit=all", "lessons-item"]]) {
    const p = await lockedNext(page, index, item, truth);
    if (p) problems.push(p);
  }
  expect(problems, "pages that disagree with /api/progress").toEqual([]);
  return truth;
}

/** Save one of the server's AI API keys (#25) on Manage Users: `page` must
 *  be signed in as an owner (the dev user is one). Opens the provider's
 *  folded row and pastes the key, which saves it straight away; every
 *  account then uses it. */
export async function saveApiKey(page: Page, key: string, provider = "anthropic"): Promise<void> {
  await page.goto("/manage#ai");
  const row = page.locator(`[data-testid="manage-api-key"][data-provider="${provider}"]`);
  await row.getByTestId("manage-api-key-toggle").click();
  await row.getByTestId("manage-api-key-input").fill(key);
  await expect(row).toHaveAttribute("data-key-set", "true");
  await expect(row.getByTestId("manage-api-key-status")).toContainText(key.slice(-4));
}

/** Remove one of the server's AI API keys on Manage Users (an owner's `page`). */
export async function removeApiKey(page: Page, provider = "anthropic"): Promise<void> {
  await page.goto("/manage#ai");
  const row = page.locator(`[data-testid="manage-api-key"][data-provider="${provider}"]`);
  if ((await row.getAttribute("data-key-set")) !== "true") return;
  await row.getByTestId("manage-api-key-toggle").click();
  await row.getByTestId("manage-api-key-remove").click();
  await expect(page.locator(`[data-testid="manage-api-key"][data-provider="${provider}"]`)).toHaveAttribute("data-key-set", "false");
}

/** The simulated provider's key for every provider, as the server's
 *  (`/dev/login?keys=fake`), from a request context of its own, so
 *  `page`'s sign-in stays as it is (`page` must have a page open): what a
 *  spec that changed the server's keys puts back at its end. */
export async function resetServerKeys(page: Page): Promise<void> {
  const ctx = await request.newContext({ baseURL: new URL(page.url()).origin });
  try {
    expect((await ctx.get("/dev/login?keys=fake&next=/manage")).ok()).toBe(true);
  } finally {
    await ctx.dispose();
  }
}

/** Hide (`true`) or show the AI features for every account (#25) with the
 *  owner's switch on Manage Users (an owner's `page`). */
export async function setAiHidden(page: Page, hidden: boolean): Promise<void> {
  await page.goto("/manage#ai");
  const row = page.getByTestId("manage-ai");
  if ((await row.getAttribute("data-ai-off")) !== String(hidden)) await page.getByTestId("manage-ai-switch").click();
  await expect(page.getByTestId("manage-ai")).toHaveAttribute("data-ai-off", String(hidden));
}
