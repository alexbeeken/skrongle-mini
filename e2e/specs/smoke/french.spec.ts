// Smoke spec for the French course (issue #464): the course is on /courses
// with its four levels, a new learner passes its first lesson in Study,
// Sprint and Play, numeric questions grade,
// material comes first, and the games that read course data (the verb
// drill with j'ai, Date Dash) play. Answers come from the
// course files (lib/key.ts); no key, no network. Each test is its own
// account and fails on any HTTP error, console error or overflow.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { Page, TestInfo } from "@playwright/test";
import { courseDir } from "../../lib/app.ts";
import { passReadingCheck, playPractice, playStage } from "../../lib/answer.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { expectHistory, playDates, playDrill, watchBugs } from "../../lib/games.ts";
import { AnswerKey } from "../../lib/key.ts";
import { apiProgress, expectProgressAgrees, login, newAccount, openCourse, register, signUp } from "../../lib/learner.ts";

const COURSE = "french";
const FIRST = "greetings";
const key = new AnswerKey(COURSE);
const course = JSON.parse(readFileSync(join(courseDir(COURSE), "course.json"), "utf8"));

async function newLearner(page: Page, info: TestInfo) {
  const bugs = watchBugs(page, info);
  await register(page, newAccount("fr"), COURSE);
  return bugs;
}

/** Pass a lesson the quick way (the agents' API), for a game it unlocks. */
async function pass(page: Page, slug: string) {
  const r = await page.request.post("/api/progress/pass", { data: { lesson: slug, answered: 5, correct: 5, evidence: "smoke spec: unlocks a game" } });
  expect((await r.json()).passed, `${slug} passed`).toBe(true);
}

/** `slug` is passed in /api/progress, every page agrees, and `after` is open in Sprint and Play. */
async function expectPassedEverywhere(page: Page, slug: string, after: string) {
  const truth = await expectProgressAgrees(page);
  expect(truth.find((l) => l.slug === slug)?.passed, `${slug} passed in /api/progress`).toBe(true);
  for (const [path, testid] of [["/lessons?unit=all", "lessons-item"], ["/map", "map-lesson"], ["/sprint?reduce-motion=1", "sprint-lesson"], ["/guide", "guide-stage"]]) {
    await page.goto(path);
    await expect(page.locator(`[data-testid="${testid}"][data-lesson-id="${slug}"]`), `${slug} on ${path}`).toHaveAttribute("data-passed", "true");
  }
  for (const [path, testid] of [["/sprint?reduce-motion=1", "sprint-lesson"], ["/guide", "guide-stage"]]) {
    await page.goto(path);
    await expect(page.locator(`[data-testid="${testid}"][data-lesson-id="${after}"]`), `${after} on ${path}`).not.toHaveAttribute("data-state", "locked");
  }
}

test("the course is on /courses, offers its four levels, and starts at greetings", async ({ page }, info) => {
  const bugs = watchBugs(page, info);
  const a = newAccount("fr");
  await signUp(page, a);
  await login(page, a);
  // A levels course asks a new learner where to start, first in its Start wizard (#587); A1 is the beginning.
  await openCourse(page, COURSE, { wizard: true });
  const wizard = page.locator(`[data-testid="courses-settings-dialog"][data-course-id="${COURSE}"]`);
  await expect(wizard.getByTestId("courses-settings-level")).toHaveCount(course.units.length);
  // The first level is "the beginning" (an empty unit value) and is picked by default.
  await expect(wizard.locator('[data-testid="courses-settings-level"][data-unit=""]').getByTestId("courses-settings-level-radio"), "A1 is the default").toBeChecked();
  while (await wizard.getByTestId("courses-settings-next").isVisible()) await wizard.getByTestId("courses-settings-next").click();
  await wizard.getByTestId("courses-settings-save").click();
  await expect(page).toHaveURL(/\/$/);
  const { lessons } = (await (await page.request.get("/api/progress")).json()) as { lessons: { slug: string; unit: string; skipped: boolean }[] };
  expect(lessons[0].slug).toBe(FIRST);
  expect(lessons.filter((l) => l.skipped)).toEqual([]);
  expect(new Set(lessons.map((l) => l.unit)).size, "every unit has lessons").toBe(course.units.length);
  await bugs.go("/sprint?reduce-motion=1");
  await expect(page.getByTestId("sprint-continue").first()).toHaveAttribute("data-lesson-id", FIRST);
  await bugs.go("/courses");
  await expect(page.locator(`[data-testid="courses-card"][data-course-id="${COURSE}"]`).first()).toBeVisible();
  bugs.none();
});

test("Study: read greetings, pass its check, practise it; numbers' numeric questions grade", async ({ page }, info) => {
  const bugs = await newLearner(page, info);
  const [first, second] = (await apiProgress(page)).map((l) => l.slug);
  expect(first).toBe(FIRST);
  await bugs.go(`/lessons/${first}`);
  await passReadingCheck(page, key);
  await page.reload();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-read", "true");
  await bugs.go(`/practice?lesson=${first}`);
  const [right, total] = await playPractice(page, key);
  expect(right, "every practice answer right").toBe(total);
  await expectPassedEverywhere(page, first, second);
  // Numbers, time and dates: typed numbers are graded by value.
  await bugs.go("/practice?lesson=numbers-time-dates");
  const [nr, nt] = await playPractice(page, key);
  expect(nt).toBeGreaterThan(0);
  expect(nr, "every numbers answer right").toBe(nt);
  bugs.none();
});

test("Sprint: play greetings' session right at phone width", async ({ page }, info) => {
  test.setTimeout(120_000);
  const bugs = await newLearner(page, info);
  const [first, second] = (await apiProgress(page)).map((l) => l.slug);
  await page.setViewportSize({ width: 390, height: 844 });
  await bugs.go(`/sprint/play?lesson=${first}&reduce-motion=1`);
  const run = await playSprintRight(page, key);
  expect(run.wrong, "answers the app marked wrong").toEqual([]);
  await expect(page.getByTestId("sprint-play-done")).toHaveAttribute("data-saved", "true");
  await page.setViewportSize({ width: 1280, height: 900 });
  await expectPassedEverywhere(page, first, second);
  bugs.none();
});

test("Play: play the first stage", async ({ page }, info) => {
  const bugs = await newLearner(page, info);
  const [first, second] = (await apiProgress(page)).map((l) => l.slug);
  await bugs.go(`/guide/${first}`);
  const [right, total] = await playStage(page, key);
  expect(right, "every quick check right").toBe(total);
  await expectPassedEverywhere(page, first, second);
  bugs.none();
});

test("games: the verb drill (être, avoir) and Date Dash", async ({ page }, info) => {
  test.setTimeout(120_000);
  const bugs = await newLearner(page, info);
  await bugs.go("/play/conjugation");
  await expect(page.getByTestId("drill-setup-locked")).toBeVisible();
  await pass(page, "etre-avoir");
  await bugs.go("/play/conjugation");
  await expect(page.locator('[data-testid="drill-setup-item"][data-unlocked="true"]')).not.toHaveCount(0);
  await page.getByTestId("drill-setup-start").click();
  const [dr, dt] = await playDrill(page, key);
  expect(dt).toBeGreaterThan(0);
  expect(dr, "every form right").toBe(dt);

  await bugs.go("/play/dates");
  for (const mode of ["days", "months", "dates"]) {
    await playDates(page, course.calendar, mode, 4);
    await page.getByTestId("dates-settings").click();
  }

  await expectHistory(page, [`${course.paradigm.item_label} Drill`, "Date Dash"]);
  bugs.none();
});
