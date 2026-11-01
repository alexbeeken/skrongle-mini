// The Social motivator (#563): anonymous comparisons with the course's other
// learners on the course home and in the reward line (src/motivators/social.rs).
// Nobody is named. A comparison needs at least five learners active in its
// window (MIN_CROWD), so the spec seeds five other learners who study and
// pass a lesson through the agent API. Other specs share the server and add
// to the crowd, so counts are checked as "at least". scripts/e2e.sh turns
// the five-minute cache off (SKRONGLE_SOCIAL_CACHE_SECS=0). The lesson line
// (#501) counts who opened or worked on the lesson to continue at in the
// last hour; other specs' learners may be on whichever lesson that is, so
// the checks after the pass leave it out.
import type { Browser, Page } from "@playwright/test";
import { AnswerKey } from "../../lib/key.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, newAccount, pickMotivator, register, wearMotivator } from "../../lib/learner.ts";

const COURSE = "spanish";
const SEEDED = 5;
const key = new AnswerKey(COURSE);

/** Another learner, in their own session: a few graded answers on `slug` (effort points today), then a pass of it. */
async function seedLearner(browser: Browser, slug: string, answers: number): Promise<void> {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await register(page, newAccount("social-seed"), COURSE);
  for (let i = 0; i < answers; i++) {
    const r = await page.request.post("/api/progress/answers", { data: { lesson: slug, prompt: `social seed ${i}`, answer: "x", correct: true } });
    expect(r.ok(), await r.text()).toBe(true);
  }
  const r = await page.request.post("/api/progress/pass", { data: { lesson: slug, answered: 5, correct: 5, evidence: "social spec: a seeded learner" } });
  expect(r.ok(), await r.text()).toBe(true);
  await ctx.close();
}

async function lineKinds(page: Page, withLesson = false): Promise<string[]> {
  const kinds = await page.getByTestId("home-social-line").evaluateAll((els) => els.map((e) => e.getAttribute("data-kind") ?? ""));
  return withLesson ? kinds : kinds.filter((k) => k !== "lesson-now");
}

test("Social compares the learner with the course's other learners, anonymously", async ({ page, browser }) => {
  await register(page, newAccount("social"), COURSE);
  const [a] = (await apiProgress(page)).filter((l) => l.required_answers >= 2);

  // Pick Social in the course's settings.
  await pickMotivator(page, COURSE, "social");

  // Five other learners study today and pass lesson a.
  for (let i = 0; i < SEEDED; i++) await seedLearner(browser, a.slug, 2 + i);

  // Opening lesson a makes it the one to continue at (opening earns no points).
  await page.goto(`/lessons/${a.slug}`);

  // The hero: how many studied this week and today, how many were on lesson a in the
  // last hour, the daily goal, Continue. The learner has done nothing yet, so nothing
  // compares their effort.
  await page.goto("/");
  const hero = page.getByTestId("home-today");
  await expect(hero).toHaveAttribute("data-motivator", "social");
  await expect(page.getByTestId("home-goal-ring")).toHaveCount(0);
  expect(await lineKinds(page, true)).toEqual(["week-count", "today-count", "lesson-now"]);
  await expect(page.getByTestId("home-continue")).toHaveAttribute("data-lesson-id", a.slug);
  const onLesson = page.locator('[data-testid="home-social-line"][data-kind="lesson-now"]');
  await expect(onLesson).toContainText(a.title);
  expect(Number((await onLesson.innerText()).match(/\d+/)?.[0])).toBeGreaterThanOrEqual(SEEDED + 1);
  const today = page.locator('[data-testid="home-social-line"][data-kind="today-count"]');
  expect(Number((await today.innerText()).match(/\d+/)?.[0])).toBeGreaterThanOrEqual(SEEDED);
  await expect(page.getByTestId("home-social-goal")).toHaveAttribute("data-points", "0");
  await expect(page.getByTestId("home-social-ahead")).toHaveCount(0);
  await expect(page.getByTestId("home-continue")).toBeVisible();

  // The (i) says what is compared; the page names nobody.
  await page.getByTestId("home-social-info").click();
  await expect(page.getByTestId("home-social-dialog")).toBeVisible();
  await page.getByTestId("home-social-close").click();
  await expect(page.getByTestId("home-social-dialog")).toBeHidden();
  await expect(hero).not.toContainText("@example.com");

  // Passing lesson a in Sprint mode: one of at least six who passed it this week.
  await page.goto(`/sprint/play?lesson=${a.slug}&reduce-motion=1`);
  await playSprintRight(page, key);
  const done = page.getByTestId("sprint-play-done");
  await expect(done).toHaveAttribute("data-saved", "true");
  const line = done.getByTestId("motivator-reward");
  await expect(line).toHaveAttribute("data-motivator", "social");
  const passed = (await line.innerText()).match(/one of (\d+) who passed this lesson this week/);
  expect(passed, await line.innerText()).not.toBeNull();
  expect(Number(passed![1])).toBeGreaterThanOrEqual(SEEDED + 1);

  // Back home the learner's effort is compared, this week and today.
  await page.goto("/");
  expect(await lineKinds(page)).toEqual(["week-effort", "today-count"]);
  await expect(page.locator('[data-testid="home-social-line"][data-kind="week-effort"]')).toContainText(/more effort than \d+ %/);
  await expect(page.getByTestId("home-social-ahead")).toHaveAttribute("data-pct", /^\d+$/);
  const points = await page.getByTestId("home-social-goal").getAttribute("data-points");
  expect(Number(points)).toBeGreaterThan(0);

  // Switching away and back loses nothing.
  await wearMotivator(page, COURSE, "stats");
  await page.goto("/");
  await expect(page.getByTestId("home-goal-ring")).toHaveAttribute("data-points", points!);
  await wearMotivator(page, COURSE, "social");
  await page.goto("/");
  await expect(hero).toHaveAttribute("data-motivator", "social");
  await expect(page.getByTestId("home-social-goal")).toHaveAttribute("data-points", points!);
  expect(await lineKinds(page)).toEqual(["week-effort", "today-count"]);
});
