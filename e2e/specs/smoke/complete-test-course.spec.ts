// Smoke spec 7 (issue #48), the modes: a new learner finishes the whole test
// course (data/test-courses/test-course, one lesson per feature) in Study, in Sprint
// and in Play, each as its own account, answering from the course files. Every
// Sprint exercise kind must turn up, and every page must agree at the end. The
// games and the tutor are in test-course-games.spec.ts.
import type { Page } from "@playwright/test";
import { passReadingCheck, playPractice, playStage } from "../../lib/answer.ts";
import { ALL_KINDS, playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { AnswerKey } from "../../lib/key.ts";
import { apiProgress, expectProgressAgrees, newAccount, register } from "../../lib/learner.ts";
import { Findings } from "../../playthrough/watch.ts";

const COURSE = "test-course";
const key = new AnswerKey(COURSE);

test.use({ reducedMotion: "reduce" });

/** Exact bug signals (HTTP and console errors, overflow) on `page` for the whole test; `done()` fails on any. */
function watch(page: Page) {
  const findings = new Findings("smoke", test.info().outputDir);
  findings.watch(page);
  const seen = new Set<string>();
  page.on("load", () => {
    const path = new URL(page.url()).pathname;
    if (seen.has(path)) return;
    seen.add(path);
    void findings.checkPage(page);
  });
  return {
    findings,
    done: () => expect(findings.list.filter((f) => f.kind === "bug").map((f) => `${f.what} (${f.url})`), "bugs seen on the way").toEqual([]),
  };
}

/** Lessons in the order the course lists them. */
async function lessons(page: Page): Promise<string[]> {
  return (await apiProgress(page)).map((l) => l.slug);
}

/** The end of every mode: every lesson passed, every page agrees, every unit complete on the map. */
async function expectCourseComplete(page: Page) {
  const truth = await expectProgressAgrees(page);
  expect(truth.filter((l) => !l.passed).map((l) => `${l.slug}: ${l.status}`), "lessons not passed").toEqual([]);
  await page.goto("/map");
  for (const unit of await page.getByTestId("map-unit").all())
    await expect(unit.locator('[data-testid="map-lesson"][data-passed="false"]'), "a unit with a lesson not passed").toHaveCount(0);
}

test.describe("7. complete the test course", () => {
  test.describe.configure({ timeout: 240_000 });

  test("K: read and practise every lesson", async ({ page }) => {
    const w = watch(page);
    await register(page, newAccount("smoke-study"), COURSE);
    for (const slug of await lessons(page)) {
      await page.goto(`/lessons/${slug}`);
      await passReadingCheck(page, key);
      await page.goto(`/practice?lesson=${slug}`);
      if (await page.getByTestId("quiz-empty").count()) continue; // a lesson with no questions passes on reading
      const [right, total] = await playPractice(page, key);
      expect(right, `${slug}: every practice answer right`).toBe(total);
    }
    await expectCourseComplete(page);
    // The lesson list and the history saw it all.
    await page.goto("/lessons?unit=all");
    await expect(page.locator('[data-testid="lessons-item"][data-passed="false"]')).toHaveCount(0);
    await page.goto("/history");
    await expect(page.locator('[data-testid="history-answer"][data-correct="false"]')).toHaveCount(0);
    await expect(page.getByTestId("history-answer")).not.toHaveCount(0);
    w.done();
  });

  test("D: every lesson's session, every exercise kind, the review mix and lightning", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.clock.install(); // runs as usual; lightning's minute is run through below
    const w = watch(page);
    await register(page, newAccount("smoke-sprint"), COURSE);
    const met = new Set<string>();
    const wrong: string[] = [];
    let first = true;
    for (const slug of await lessons(page)) {
      await page.goto(`/sprint/play?lesson=${slug}&reduce-motion=1`);
      const run = await playSprintRight(page, key, { typeOnce: first });
      first = false;
      run.kinds.forEach((k) => met.add(k));
      wrong.push(...run.wrong.map((x) => `${slug}: ${x}`));
      await expect(page.getByTestId("sprint-play-done"), `${slug}: the session is saved`).toHaveAttribute("data-saved", "true");
    }
    expect(wrong, "answers the app marked wrong").toEqual([]);
    // Sprint mode skips questions that need a board (algebra tiles, #1355), so
    // a lesson made only of them is finished in Study.
    for (const slug of ["algebra-tiles"]) {
      await page.goto(`/practice?lesson=${slug}`);
      const [right, total] = await playPractice(page, key);
      expect(right, `${slug}: every practice answer right`).toBe(total);
    }
    await expectCourseComplete(page);

    // The review mix draws from every lesson; lightning is quiz questions and word picks against
    // the clock. Only these two ask a term's meaning (at random), so a review round more while a
    // kind is missing, up to three.
    for (const [n, mode] of [[0, "lightning"], [0, "review"], [1, "review"], [2, "review"]] as const) {
      if (n > 0 && ALL_KINDS.every((k) => met.has(k))) break;
      await page.goto(`/sprint/play?mode=${mode}&reduce-motion=1`);
      if (mode === "lightning") await page.clock.runFor(3_000); // the 3-2-1
      const run = await playSprintRight(page, key);
      expect(run.wrong, `${mode} answers marked wrong`).toEqual([]);
      run.kinds.forEach((k) => met.add(k));
      await expect(page.getByTestId("sprint-play-done"), `${mode}: the session is saved`).toHaveAttribute("data-saved", "true");
    }
    // A listening kind the draws left out (the seed's draws shift whenever a lesson is added): the
    // listening lesson's session again, up to three times.
    for (let n = 0; n < 3 && ALL_KINDS.some((k) => k.startsWith("hear-") && !met.has(k)); n++) {
      await page.goto("/sprint/play?lesson=listening&reduce-motion=1");
      const run = await playSprintRight(page, key);
      expect(run.wrong, "listening answers marked wrong").toEqual([]);
      run.kinds.forEach((k) => met.add(k));
      await expect(page.getByTestId("sprint-play-done")).toHaveAttribute("data-saved", "true");
    }
    // A lightning round also ends when its minute is up.
    await page.goto("/sprint/play?mode=lightning&reduce-motion=1");
    await page.clock.runFor(3_000);
    await playSprintRight(page, key, { answers: 2 });
    await page.clock.runFor(61_000); // every tick of the countdown (fastForward would fire it once)
    await expect(page.getByTestId("sprint-play-done")).toHaveAttribute("data-saved", "true");

    console.log(`   Sprint kinds met: ${[...met].sort().join(", ")}`);
    expect(ALL_KINDS.filter((k) => !met.has(k)), "D exercise kinds never met").toEqual([]);
    expect(met.has("type-instead"), '"Type it instead" used once').toBe(true);
    w.done();
  });

  test("G: every Guide stage, in order", async ({ page }) => {
    const w = watch(page);
    await register(page, newAccount("smoke-play"), COURSE);
    for (let n = 0; n < 50; n++) {
      await page.goto("/guide");
      const next = page.locator('[data-testid="guide-stage"]:not([data-passed="true"])').first();
      if (!(await next.count())) break;
      await expect(next, "the next stage is open").not.toHaveAttribute("data-state", "locked");
      const slug = (await next.getAttribute("data-lesson-id"))!;
      await page.goto(`/guide/${slug}`);
      const [right, total] = await playStage(page, key);
      expect(right, `${slug}: every quick check right`).toBe(total);
    }
    await expectCourseComplete(page);
    w.done();
  });
});
