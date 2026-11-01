// Spec 7, the games half (issue #48): a new learner plays every game, the
// tutor and a theme on the test course (data/test-courses/), every
// answer right, from the course files (lib/key.ts) or, for Numbers by ear,
// from what the voice said. No key, no network. Each test is its own account,
// fails on any HTTP error, console error or overflow, and checks History.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { Page, TestInfo } from "@playwright/test";
import { courseDir, TEST_COURSE } from "../../lib/app.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { expectHistory, playDates, playDrill, playFlashcards, playListening, playMatching, playNumbers, recordSpeech, watchBugs } from "../../lib/games.ts";
import { AnswerKey } from "../../lib/key.ts";
import { newAccount, register, saveApiKey, wearTheme } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);
const lessonFile = (path: string) => JSON.parse(readFileSync(join(courseDir(TEST_COURSE), "lessons", path), "utf8"));
const course = JSON.parse(readFileSync(join(courseDir(TEST_COURSE), "course.json"), "utf8"));

async function newLearner(page: Page, info: TestInfo) {
  const bugs = watchBugs(page, info);
  await register(page, newAccount("tc-games"), TEST_COURSE);
  return bugs;
}

/** Pass a lesson the quick way (the agents' API), for a game it unlocks. */
async function pass(page: Page, slug: string) {
  const r = await page.request.post("/api/progress/pass", { data: { lesson: slug, answered: 5, correct: 5, evidence: "smoke spec: unlocks a game" } });
  expect((await r.json()).passed, `${slug} passed`).toBe(true);
}

/** The games of the /play hub this file plays (its `games-item[data-game]`). */
const PLAYED = ["matching", "conjugation", "numbers", "dates", "study", "guide"];

test("every game on /play is played here", async ({ page }, info) => {
  // The test course has the data for every game, so the hub offers them all. A game added to the
  // hub fails here until this file plays it (and the test course has what it needs).
  const bugs = await newLearner(page, info);
  await bugs.go("/play");
  const offered = await page.getByTestId("games-item").evaluateAll((els) => els.map((e) => e.getAttribute("data-game")!));
  expect(offered.filter((g) => !PLAYED.includes(g)), "games on /play that no test here plays").toEqual([]);
  expect(PLAYED.filter((g) => !offered.includes(g)), "games played here that the test course doesn't offer").toEqual([]);
  bugs.none();
});

test("K Listening: a lesson's round, then a mixed round", async ({ page }, info) => {
  const bugs = await newLearner(page, info);
  await bugs.go("/listening");
  await bugs.go("/listening/play?lesson=listening");
  const [r1, t1] = await playListening(page, key);
  expect(t1).toBeGreaterThan(0);
  expect(r1, "every sentence right").toBe(t1);
  await bugs.go("/listening/play");
  const [r2, t2] = await playListening(page, key);
  expect(t2).toBeGreaterThan(0);
  expect(r2, "every sentence right in the mixed round").toBe(t2);
  await expectHistory(page, ["Listening"]);
  bugs.none();
});

test("Vocabulary: collect words in Sprint mode, then the vocabulary page and the flashcards", async ({ page }, info) => {
  const bugs = await newLearner(page, info);
  // Nothing collected yet: the banner, and every lesson folded behind one toggle (#404).
  await bugs.go("/vocabulary");
  await expect(page.getByTestId("vocabulary-empty")).toBeVisible();
  await expect(page.locator('[data-testid="vocabulary-lesson"]:visible')).toHaveCount(0);
  await page.getByTestId("vocabulary-empty-lessons-toggle").click();
  await expect(page.locator('[data-testid="vocabulary-lesson"]:visible')).not.toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await bugs.go("/sprint/play?lesson=vocabulary&reduce-motion=1");
  expect((await playSprintRight(page, key)).wrong).toEqual([]);
  await page.setViewportSize({ width: 1280, height: 900 });
  await bugs.go("/vocabulary");
  const lesson = page.locator('[data-testid="vocabulary-lesson"][data-lesson-id="vocabulary"]');
  await expect(lesson).toBeVisible();
  expect(Number(await lesson.getAttribute("data-learned"))).toBeGreaterThan(0);
  // A lesson with terms is listed; those still empty stay folded.
  await expect(page.locator('[data-testid="vocabulary-empty-lessons"] [data-testid="vocabulary-lesson"][data-learned="0"]').first()).toBeHidden();
  await expect(page.locator('[data-testid="vocabulary-lesson"]:visible[data-learned="0"]')).toHaveCount(0);
  await bugs.go("/vocabulary/study");
  expect(await playFlashcards(page)).toBeGreaterThan(0);
  bugs.none();
});

test("Word Match: the lesson board and the all-lessons board", async ({ page }, info) => {
  const bugs = await newLearner(page, info);
  await bugs.go("/play/matching");
  await bugs.go("/play/matching/play?lesson=vocabulary");
  expect(await playMatching(page, key), "wrong tries").toBe(0);
  await bugs.go("/play/matching/play?lesson=all");
  expect(await playMatching(page, key), "wrong tries").toBe(0);
  await expectHistory(page, ["Word Match"]);
  bugs.none();
});

test("the paradigm drill, Numbers by ear, Date Dash (each drill)", async ({ page }, info) => {
  test.setTimeout(90_000);
  await recordSpeech(page);
  const bugs = await newLearner(page, info);
  // The drill offers what the learner has passed: hablar and comer come with the conjugations lesson.
  await bugs.go("/play/conjugation");
  await expect(page.getByTestId("drill-setup-locked")).toBeVisible();
  await pass(page, "conjugations");
  await bugs.go("/play/conjugation");
  const unlocked = page.locator('[data-testid="drill-setup-item"][data-unlocked="true"]');
  await expect(unlocked).not.toHaveCount(0);
  await page.getByTestId("drill-setup-start").click();
  const [dr, dt] = await playDrill(page, key);
  expect(dt).toBeGreaterThan(0);
  expect(dr, "every form right").toBe(dt);

  await bugs.go("/play/numbers");
  const [nr, nt] = await playNumbers(page);
  expect(nr, "every number right").toBe(nt);

  await bugs.go("/play/dates");
  for (const mode of ["days", "months", "both", "dates", "all"]) {
    await playDates(page, course.calendar, mode, 4);
    await page.getByTestId("dates-settings").click();
  }
  await expectHistory(page, [`${course.paradigm.item_label} Drill`, "Numbers by ear", "Date Dash"]);
  bugs.none();
});

test("Tutor: a tutored lesson over the fixtures, one tapped and one typed answer", async ({ page }, info) => {
  const bugs = await newLearner(page, info);
  // A key (any: the dev server answers model calls from data/test-fixtures/llm/).
  await bugs.go("/profile");
  await saveApiKey(page, "sk-ant-dev-test-fixtures");

  await bugs.go("/tutor?lesson=everything");
  const done = { tapped: 0, typed: 0 };
  for (let i = 0; i < 10 && !(done.tapped && done.typed); i++) {
    const ex = page.locator('[data-testid="tutor-exercise"]:not([data-state=done])');
    await expect(ex).toHaveCount(1, { timeout: 15_000 });
    const qid = (await ex.getAttribute("data-qid"))!;
    const chips = ex.getByTestId("tutor-choice");
    const options = await chips.allInnerTexts();
    const answer = key.answer(qid, options);
    if (options.length && !done.tapped) {
      await chips.nth(options.indexOf(answer)).click();
      done.tapped++;
    } else {
      await page.getByTestId("tutor-input").fill(answer);
      await page.getByTestId("tutor-send").click();
      done.typed++;
    }
    await expect(page.locator(`[data-testid="tutor-verdict"][data-qid="${qid}"]`)).toHaveAttribute("data-state", "right");
  }
  expect(done.tapped, "a tapped answer").toBeGreaterThan(0);
  expect(done.typed, "a typed answer").toBeGreaterThan(0);
  bugs.none();
});

test("Theme: the worn theme's material comes first, and nothing is missing without it", async ({ page }, info) => {
  test.setTimeout(90_000);
  const bugs = await newLearner(page, info);
  const plain = lessonFile("5-edge/themes.json");
  const realm = plain.themes.realm;

  // Plain: the lesson's own material, nothing themed.
  await bugs.go("/lessons/themes");
  await expect(page.getByTestId("lesson-vocab-item")).toHaveCount(plain.vocabulary.length);
  await expect(page.locator('[data-testid="lesson-vocab-item"][data-themed="true"]')).toHaveCount(0);
  await expect(page.getByTestId("lesson-check")).toHaveAttribute("data-praise", course.strings.praise[0]);

  // Wear realm: a setting of the course (#577).
  await wearTheme(page, TEST_COURSE, "realm");
  await bugs.go("/");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "realm");
  await expect(page.locator(".theme-tag")).toBeVisible();

  // The lesson page: the theme's examples and new term are there, the plain ones too.
  await bugs.go("/lessons/themes");
  const newTerms = realm.vocabulary.filter((v: { meaning?: string }) => v.meaning).length;
  await expect(page.getByTestId("lesson-vocab-item")).toHaveCount(plain.vocabulary.length + newTerms);
  await expect(page.locator('[data-testid="lesson-vocab-item"][data-themed="true"]')).toHaveCount(realm.vocabulary.length);
  await expect(page.getByTestId("lesson-content")).toContainText(realm.content.split("\n")[0].replace(/^## /, ""));
  // Praise: the course's own lines for the theme (its language), not the theme file's English ones.
  await expect(page.getByTestId("lesson-check")).toHaveAttribute("data-praise", course.themes.realm.strings.praise[0]);

  // A Sprint session: the themed sort, spot and build (index 0, they go first) are played.
  const ids: string[] = [];
  page.on("request", (r) => {
    if (/\/api\/(sprint\/check|answer)$/.test(r.url())) ids.push(JSON.parse(r.postData() ?? "{}").id ?? JSON.parse(r.postData() ?? "{}").question_id);
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await bugs.go("/sprint/play?lesson=themes&reduce-motion=1");
  expect((await playSprintRight(page, new AnswerKey(TEST_COURSE, "realm"))).wrong).toEqual([]);
  for (const id of ["sort:tc-themes:0", "spot:tc-themes:0", "build:tc-themes:0"]) expect(ids, "the themed exercise is played").toContain(id);
  await page.setViewportSize({ width: 1280, height: 900 });

  // The Guide stage: the themed question comes before the plain ones, which are all still there.
  await bugs.go("/guide/themes");
  const qids = await page.locator('[data-testid="stage-step"][data-qid]').evaluateAll((els) => els.map((e) => e.getAttribute("data-qid")));
  expect(qids[0], "the themed question first").toBe(realm.quiz[0].question_id);
  for (const q of plain.quiz) expect(qids).toContain(q.question_id);
  bugs.none();
});
