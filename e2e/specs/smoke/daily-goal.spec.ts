// One daily goal and one "where you left off" for every mode (#558): the
// home page's ring counts effort points from any way of studying (src/activity.rs),
// XP is gone, and the home page, Sprint mode, Study mode, the Guide and the
// map all resume at the lesson last worked on, opened in the mode last used.
import type { Page } from "@playwright/test";
import { AnswerKey } from "../../lib/key.ts";
import { passReadingCheck } from "../../lib/answer.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, newAccount, register } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);

/** Today's points on the home page's ring. */
async function ring(page: Page): Promise<number> {
  await page.goto("/");
  return Number(await page.getByTestId("home-goal-ring").getAttribute("data-points"));
}

/** Every mode resumes at `slug`, and the home page opens it at `href`. */
async function expectResumeAt(page: Page, slug: string, href: RegExp) {
  await page.goto("/");
  await expect(page.getByTestId("home-continue")).toHaveAttribute("data-lesson-id", slug);
  await expect(page.getByTestId("home-continue")).toHaveAttribute("href", href);
  await page.goto("/sprint?reduce-motion=1");
  await expect(page.getByTestId("sprint-continue").first()).toHaveAttribute("data-lesson-id", slug);
  await page.goto("/guide");
  await expect(page.locator('[data-testid="guide-stage"][data-current="true"]')).toHaveAttribute("data-lesson-id", slug);
  await page.goto("/map");
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveAttribute("data-lesson-id", slug);
}

test("every mode fills one daily goal and resumes where the learner left off", async ({ page }) => {
  await register(page, newAccount("goal"), COURSE);
  const lessons = await apiProgress(page);
  // A lesson further on, with a reading check of two questions, and the one after it.
  const at = lessons.findIndex((l, i) => i >= 3 && l.required_answers >= 2);
  const [later, next] = [lessons[at], lessons[at + 1]];

  // A new learner: an empty ring, and the first lesson.
  expect(await ring(page)).toBe(0);
  await expectResumeAt(page, lessons[0].slug, new RegExp(`/lessons/${lessons[0].slug}$`));

  // Study mode: reading the later lesson. Its two check answers (1 point each)
  // and the reading (4) make 6, and every mode now resumes there.
  await page.goto(`/lessons/${later.slug}`);
  await passReadingCheck(page, key);
  expect(await ring(page)).toBe(6);
  await expectResumeAt(page, later.slug, new RegExp(`/lessons/${later.slug}$`));

  // Sprint mode: its session adds to the same ring and passes the lesson;
  // everything moves on to the next lesson, opened in Sprint mode.
  await page.goto(`/sprint/play?lesson=${later.slug}&reduce-motion=1`);
  await playSprintRight(page, key);
  const done = page.getByTestId("sprint-play-done");
  await expect(done).toHaveAttribute("data-saved", "true");
  await expect(done).not.toContainText("XP");
  const shown = Number(await page.locator("#done-goal").getAttribute("data-points"));
  expect(shown).toBeGreaterThan(6);
  expect(await ring(page)).toBe(shown);
  expect((await apiProgress(page)).find((l) => l.slug === later.slug)?.passed).toBe(true);
  await expectResumeAt(page, next.slug, new RegExp(`/sprint/play\\?lesson=${next.slug}$`));

  // The Guide: opening its stage makes the Guide the mode to continue in.
  await page.goto(`/guide/${next.slug}`);
  await expect(page.getByTestId("stage")).toBeVisible();
  await page.goto("/");
  await expect(page.getByTestId("home-continue")).toHaveAttribute("href", new RegExp(`/guide/${next.slug}$`));

  // A review mix draws on many lessons: it does not move the resume point.
  await page.goto("/sprint/play?mode=review&reduce-motion=1");
  await page.goto("/");
  await expect(page.getByTestId("home-continue")).toHaveAttribute("data-lesson-id", next.slug);

  // The (i) says what earns how many points.
  await page.getByTestId("home-info-goal").click();
  await expect(page.getByTestId("home-goal-points").locator("tbody tr")).toHaveCount(8);

  // No XP left anywhere.
  for (const path of ["/", "/sprint?reduce-motion=1", "/guide", `/guide/${next.slug}`]) {
    await page.goto(path);
    await expect(page.locator("body"), path).not.toContainText(/\bXP\b/);
  }
});
