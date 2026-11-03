// The starting level (#175): a course whose units are `levels` asks a
// learner where to start in the last step of the wizard behind its Start
// button (#587, #1106). Starting at A2 skips the A1 lessons: open to read, never
// recommended, practised or reviewed, and not passed. The level can be
// changed later (#333) in the course's settings (the gear on /courses), and
// only there; progress stays. A course without levels never asks. Each test
// registers its own account; no key, no network.
import { AnswerKey } from "../../lib/key.ts";
import { passReadingCheck } from "../../lib/answer.ts";
import { createCourse, openFromEditor, TEST_COURSE } from "../../lib/app.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { expectProgressAgrees, login, newAccount, openCourse, register, signUp } from "../../lib/learner.ts";
import type { Page } from "@playwright/test";

const key = new AnswerKey("spanish");

interface Lesson {
  slug: string;
  title: string;
  unit: string;
  seen: boolean;
  passed: boolean;
  skipped: boolean;
  quiz_questions: number;
}

async function progress(page: Page): Promise<{ start_unit: string; lessons: Lesson[] }> {
  const r = await page.request.get("/api/progress");
  expect(r.ok()).toBe(true);
  return r.json();
}

/** The Start wizard of `course`, open over the course list (#587). */
function wizard(page: Page, course: string) {
  return page.locator(`[data-testid="courses-settings-dialog"][data-course-id="${course}"][data-start]`);
}

/** Slide the open wizard `dlg` to its level step, the last one (#1106). */
async function toLevels(dlg: ReturnType<typeof wizard>) {
  const next = dlg.getByTestId("courses-settings-next");
  while ((await dlg.locator('.cs-step.here[data-step="level"]').count()) === 0) await next.click();
}

/** "Get Started!" in the open wizard `dlg`, from the step it is on (#896): the course opens on its home. */
async function getStarted(page: Page, dlg: ReturnType<typeof wizard>) {
  await dlg.getByTestId("courses-settings-save").click();
  await expect(page).toHaveURL(/\/$/);
}

/** Change the level of `course` behind its gear on /courses, and Save (#587). */
async function changeLevel(page: Page, course: string, unit: string, current?: string) {
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator(`[data-course-id="${course}"]`).getByTestId("courses-settings").click();
  const dlg = page.locator(`[data-testid="courses-settings-dialog"][data-course-id="${course}"]`);
  if (current !== undefined) await expect(dlg.locator(`[data-testid="courses-settings-level"][data-unit="${current}"] input`)).toBeChecked();
  await dlg.locator(`[data-testid="courses-settings-level"][data-unit="${unit}"]`).click();
  const saved = page.waitForResponse((r) => r.url().endsWith(`/courses/${course}/settings`) && r.request().method() === "POST");
  await dlg.getByTestId("courses-settings-save").click();
  expect((await saved).status()).toBe(303);
  await expect(dlg).toBeHidden();
}

/** The exercises of the Sprint session open in `page`, from its JSON. */
// The tag on a review-mix question from a lesson not read yet (#409).
const TASTE = "A taste of what's next · ";

/** The tags above a session's exercises, as the session JSON has them. */
async function sessionTags(page: Page, path: string): Promise<string[]> {
  await page.goto(path);
  const json = await page.locator("#session").textContent();
  const exercises: { lesson?: string }[] = JSON.parse(json ?? "{}").exercises ?? [];
  return exercises.map((e) => e.lesson).filter((l): l is string => !!l && l !== "Your words");
}

/** The lessons a session draws on, by title. */
async function sessionLessons(page: Page, path: string): Promise<string[]> {
  return (await sessionTags(page, path)).map((t) => (t.startsWith(TASTE) ? t.slice(TASTE.length) : t));
}

test("start Spanish at A2: the A1 lessons are skipped and never offered", async ({ page }) => {
  const acct = newAccount("start-a2");
  await signUp(page, acct);
  await login(page, acct);
  // Start asks where to start on its last step (#1106): A1 (the beginning),
  // A2, or any later level that has lessons.
  await openCourse(page, "spanish", { wizard: true });
  const dlg = wizard(page, "spanish");
  await expect(dlg).toHaveAttribute("data-step", "0");
  await toLevels(dlg);
  const levels = await dlg.getByTestId("courses-settings-level").count();
  expect(levels).toBeGreaterThanOrEqual(2);
  await expect(dlg.locator('[data-testid="courses-settings-level"][data-unit=""]')).toContainText("A1");
  await expect(dlg.locator('[data-testid="courses-settings-level"][data-unit=""] input')).toBeChecked();
  // The radio is visually hidden (#325): clicking anywhere on the card chooses it.
  await dlg.locator('[data-testid="courses-settings-level"][data-unit="A2"] .muted').first().click();
  await expect(dlg.locator('[data-testid="courses-settings-level"][data-unit="A2"] input')).toBeChecked();
  await expect(dlg.locator('[data-testid="courses-settings-level"][data-unit=""] input')).not.toBeChecked();
  await getStarted(page, dlg);
  expect(new Set((await progress(page)).lessons.map((l) => l.unit)).size).toBe(levels);

  const { start_unit, lessons } = await progress(page);
  expect(start_unit).toBe("A2");
  const a1 = lessons.filter((l) => l.unit === "A1");
  const a2 = lessons.filter((l) => l.unit === "A2");
  expect(a1.length).toBeGreaterThan(0);
  expect(a2.length).toBeGreaterThan(0);
  // Skipped, not faked: nothing seen or passed, zero points.
  expect(a1.every((l) => l.skipped && !l.passed && !l.seen)).toBe(true);
  expect(a2.some((l) => l.skipped)).toBe(false);
  await expect(page.getByTestId("home-skipped")).toHaveText(`${a1.length} skipped`);
  const a1Titles = new Set(a1.map((l) => l.title));
  const firstA2 = a2[0].slug;

  // The lessons list: A1 open but skipped, A2 not started.
  await page.goto("/lessons?unit=all");
  for (const l of a1) await expect(page.locator(`[data-testid="lessons-item"][data-lesson-id="${l.slug}"]`)).toHaveAttribute("data-status", "skipped");
  for (const l of a2) await expect(page.locator(`[data-testid="lessons-item"][data-lesson-id="${l.slug}"]`)).toHaveAttribute("data-status", "untested");
  const skippedRow = page.locator(`[data-testid="lessons-item"][data-lesson-id="${a1[0].slug}"]`);
  await expect(skippedRow).toContainText("Skipped");
  await expect(skippedRow.locator(".pill.unread")).toHaveCount(0);

  // Each mode starts at the first A2 lesson.
  await page.goto("/sprint?reduce-motion=1");
  await expect(page.getByTestId("sprint-continue").first()).toHaveAttribute("data-lesson-id", firstA2);
  await expect(page.locator(`[data-testid="sprint-lesson"][data-lesson-id="${a1[0].slug}"]`)).toHaveAttribute("data-state", "skipped");
  await expect(page.locator(`[data-testid="sprint-lesson"][data-lesson-id="${firstA2}"]`)).toHaveAttribute("data-state", "available");
  await page.goto("/guide");
  await expect(page.locator('[data-testid="guide-stage"][data-current="true"]')).toHaveAttribute("data-lesson-id", firstA2);
  await page.goto("/map");
  // One "You are here" on the whole map, at the first A2 lesson: none in A1 (skipped) or in the units after A2.
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveCount(1);
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveAttribute("data-lesson-id", firstA2);
  // The home's unit line is at A2, and the lesson list shows every A1 lesson skipped.
  await page.goto("/");
  await expect(page.getByTestId("home-map-unit")).toContainText("A2");
  await page.goto("/lessons?unit=all");
  await expect(page.locator('[data-testid="lessons-item"][data-status="skipped"]')).toHaveCount(a1.length);

  // Recommended practice: nothing read yet, so it points at the first A2 lesson.
  await page.goto("/practice");
  await expect(page.getByTestId("quiz-read-next")).toHaveAttribute("data-lesson-id", firstA2);
  // The review mix and the lightning round never draw a skipped A1 lesson
  // (they may reach past A2 when the course has later levels).
  for (const path of ["/sprint/play?mode=review", "/sprint/play?mode=lightning"]) {
    const met = await sessionLessons(page, path);
    expect(met.length, path).toBeGreaterThan(0);
    expect(met.filter((t) => a1Titles.has(t)), `${path}: skipped A1 lessons`).toEqual([]);
  }
  // Nothing read: every question in the review mix says it is a taste of what's next.
  const untagged = (await sessionTags(page, "/sprint/play?mode=review")).filter((t) => !t.startsWith(TASTE));
  expect(untagged, "review mix questions from lessons not read, untagged").toEqual([]);

  // Read an A2 lesson: recommended practice asks about it alone.
  const withQuiz = a2.find((l) => l.quiz_questions >= 2)!;
  await page.goto(`/lessons/${withQuiz.slug}`);
  await passReadingCheck(page, key);
  await page.goto("/practice");
  const asked = await page.getByTestId("quiz-question").evaluateAll((els) => els.map((e) => e.getAttribute("data-lesson-id")));
  expect(asked.length).toBeGreaterThan(0);
  expect(new Set(asked)).toEqual(new Set([withQuiz.slug]));
  // The review mix too: the read lesson untagged, anything else tagged as a taste.
  const tags = await sessionTags(page, "/sprint/play?mode=review");
  expect(tags, "review mix asks about the lesson read").toContain(withQuiz.title);
  expect(tags.filter((t) => t !== withQuiz.title && !t.startsWith(TASTE)), "review mix: lessons not read, untagged").toEqual([]);

  // Every page still agrees on what is passed (nothing) and nothing is locked ahead.
  await expectProgressAgrees(page);

  // Changed in the course's settings: back to the beginning, A1 is pending again.
  await changeLevel(page, "spanish", "", "A2");
  await page.goto("/lessons?unit=all");
  await expect(page.locator(`[data-testid="lessons-item"][data-lesson-id="${a1[0].slug}"]`)).toHaveAttribute("data-status", "untested");
  expect((await progress(page)).lessons.some((l) => l.skipped)).toBe(false);
  await page.goto("/sprint?reduce-motion=1");
  await expect(page.locator(`[data-testid="sprint-lesson"][data-lesson-id="${a1[0].slug}"]`)).toHaveAttribute("data-state", "available");
  // Changing level moves where every mode resumes: the first lesson of the level (#333).
  await expect(page.getByTestId("sprint-continue").first()).toHaveAttribute("data-lesson-id", a1[0].slug);
  // Opening the course again does not ask again.
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator('[data-testid="courses-card"][data-course-id="spanish"]').getByTestId("courses-open").click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.locator('[data-testid="courses-settings-dialog"][data-wizard]')).toHaveCount(0);
});

test("change level mid-course (#333): progress stays, every mode moves to the chosen level", async ({ page }) => {
  await register(page, newAccount("change-level"), "spanish"); // from the beginning
  const before = (await progress(page)).lessons;
  const a1 = before.filter((l) => l.unit === "A1");
  const firstA2 = before.find((l) => l.unit === "A2")!.slug;
  // Study the second A1 lesson: it is read (seen) and has answers.
  const studied = a1.filter((l) => l.quiz_questions >= 2)[1];
  await page.goto(`/lessons/${studied.slug}`);
  await passReadingCheck(page, key);
  const seenBefore = (await progress(page)).lessons.find((l) => l.slug === studied.slug)!;
  expect(seenBefore.seen).toBe(true);

  // The home's unit line shows the level.
  await page.goto("/");
  await expect(page.getByTestId("home-map-unit")).toContainText("A1");
  // Closing the gear's form without Save changes nothing.
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator('[data-course-id="spanish"]').getByTestId("courses-settings").click();
  const settings = page.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');
  await settings.locator('[data-testid="courses-settings-level"][data-unit="A2"]').click();
  await settings.getByTestId("courses-settings-close").click();
  expect((await progress(page)).start_unit).toBe("");
  await changeLevel(page, "spanish", "A2", "");
  await page.goto("/");
  await expect(page.getByTestId("home-map-unit")).toContainText("A2");

  // Nothing lost or faked: the studied lesson keeps its progress and is not
  // skipped; the A1 lessons never touched are skipped, none passed.
  const after = await progress(page);
  expect(after.start_unit).toBe("A2");
  const kept = after.lessons.find((l) => l.slug === studied.slug)!;
  expect({ seen: kept.seen, passed: kept.passed, skipped: kept.skipped }).toEqual({ seen: true, passed: seenBefore.passed, skipped: false });
  for (const l of after.lessons.filter((l) => l.unit === "A1" && l.slug !== studied.slug)) {
    expect(l.skipped && !l.passed, l.slug).toBe(true);
  }

  // The position moved to the first A2 lesson in every mode, past the begun A1 lesson.
  await page.goto("/sprint?reduce-motion=1");
  await expect(page.getByTestId("sprint-continue").first()).toHaveAttribute("data-lesson-id", firstA2);
  await expect(page.locator(`[data-testid="sprint-lesson"][data-lesson-id="${firstA2}"]`)).toHaveAttribute("data-state", "available");
  await page.goto("/guide");
  await expect(page.locator('[data-testid="guide-stage"][data-current="true"]')).toHaveAttribute("data-lesson-id", firstA2);
  await page.goto("/map");
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveAttribute("data-lesson-id", firstA2);
  await expectProgressAgrees(page);

  // From the course's settings, back down to A1: the studied lesson is still read.
  await changeLevel(page, "spanish", "", "A2");
  const back = await progress(page);
  expect(back.start_unit).toBe("");
  expect(back.lessons.some((l) => l.skipped)).toBe(false);
  expect(back.lessons.find((l) => l.slug === studied.slug)!.seen).toBe(true);
  await page.goto("/guide");
  await expect(page.locator('[data-testid="guide-stage"][data-current="true"]')).toHaveAttribute("data-lesson-id", a1[0].slug);
});

test("the test course offers each of its units as a level", async ({ page }) => {
  const tc = newAccount("start-tc");
  await signUp(page, tc);
  await login(page, tc);
  await openCourse(page, TEST_COURSE, { wizard: true });
  const dlg = wizard(page, TEST_COURSE);
  await toLevels(dlg);
  await expect(dlg.getByTestId("courses-settings-level")).toHaveCount(6);
  // Everything, the first level, has one lesson and counts it in the singular (#1112).
  await expect(dlg.getByTestId("courses-settings-level").first()).toContainText(/Everything[\s\S]*\b1 lesson\b/);
  await dlg.locator('[data-testid="courses-settings-level"][data-unit="edge"]').click();
  await getStarted(page, dlg);
  const { lessons } = await progress(page);
  const firstEdge = lessons.find((l) => l.unit === "Edge cases")!;
  expect(lessons.filter((l) => l.skipped).length).toBe(lessons.findIndex((l) => l.slug === firstEdge.slug));
  await page.goto("/guide");
  await expect(page.locator('[data-testid="guide-stage"][data-current="true"]')).toHaveAttribute("data-lesson-id", firstEdge.slug);
  await expect(page.getByTestId("guide-stage-skipped").first()).toBeVisible();
});

test("a course without levels never asks for one", async ({ page }) => {
  await register(page, newAccount("start-none"), "spanish");
  // A course made in the app has a single unit: no levels.
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const tab = await openFromEditor(page);
  await expect(tab).toHaveURL(/\/$/);
  await expect(tab.getByTestId("nav-course-switch")).toHaveAttribute("data-course-id", id);
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item").first()).toBeVisible();
  // Its settings have no level step.
  await page.goto("/courses");
  await expect(page.locator(`[data-testid="courses-settings-dialog"][data-course-id="${id}"]`).getByTestId("courses-settings-level")).toHaveCount(0);
  // There is no level page any more (#587).
  expect((await page.request.get("/start")).status()).toBe(404);
  expect((await progress(page)).lessons.some((l) => l.skipped)).toBe(false);
});
