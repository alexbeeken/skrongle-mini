// Smoke specs 1–6 (issue #48): a new learner reads, practises and passes the
// Spanish course's first lesson in each mode, and every page agrees about it.
// Each test registers its own account, answers from the course files
// (lib/key.ts), never from a recording, and needs no key and no network.
import { AnswerKey } from "../../lib/key.ts";
import { passReadingCheck, playPractice, playStage } from "../../lib/answer.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, expectProgressAgrees, login, logout, newAccount, register, type Account } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);

/** The course's lessons in the path's order: the first two. */
async function firstTwo(page: import("@playwright/test").Page): Promise<[string, string]> {
  const ls = await apiProgress(page);
  return [ls[0].slug, ls[1].slug];
}

async function passedIn(page: import("@playwright/test").Page, slug: string) {
  const l = (await apiProgress(page)).find((x) => x.slug === slug);
  expect(l, `${slug} in /api/progress`).toBeDefined();
  return l!;
}

/** Lesson `slug` shows as passed in every mode's list, and `after` is open in Sprint and Play. */
async function expectPassedEverywhere(page: import("@playwright/test").Page, slug: string, after: string) {
  expect((await passedIn(page, slug)).passed, `${slug} passed in /api/progress`).toBe(true);
  await expectProgressAgrees(page);
  for (const [path, testid] of [["/lessons?unit=all", "lessons-item"], ["/map", "map-lesson"], ["/sprint?reduce-motion=1", "sprint-lesson"], ["/guide", "guide-stage"]]) {
    await page.goto(path);
    await expect(page.locator(`[data-testid="${testid}"][data-lesson-id="${slug}"]`), `${slug} on ${path}`).toHaveAttribute("data-passed", "true");
  }
  // Passed never sits beside "Not started" (#399): the mastery pill only shows once there are answers.
  await page.goto("/lessons?unit=all");
  await expect(page.locator(`[data-testid="lessons-item"][data-lesson-id="${slug}"] .pill.untested`)).toHaveCount(0);
  for (const [path, testid] of [["/sprint?reduce-motion=1", "sprint-lesson"], ["/guide", "guide-stage"]]) {
    await page.goto(path);
    await expect(page.locator(`[data-testid="${testid}"][data-lesson-id="${after}"]`), `${after} on ${path}`).not.toHaveAttribute("data-state", "locked");
  }
  // The map's one "You are here" moves on to the next lesson, as Sprint's path does (#401).
  await page.goto("/map");
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveCount(1);
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveAttribute("data-lesson-id", after);
  // The home's unit line and the course card count the pass too, not only mastery (#400).
  await page.goto("/");
  await expect(page.getByTestId("home-map-progress")).toHaveAttribute("data-passed", "1");
  await page.goto("/courses");
  await expect(page.locator(`[data-testid="courses-card"][data-course-id="${COURSE}"]`).locator('[data-testid="courses-card-stat"][data-metric="passed"]')).toHaveAttribute("data-n", "1");
}

let account: Account;
test.beforeEach(async ({ page }) => {
  account = newAccount("smoke");
  await register(page, account, COURSE);
});

test("1. a new learner sees lesson 1 as next", async ({ page }) => {
  const [first] = await firstTwo(page);
  await page.goto("/sprint?reduce-motion=1");
  await expect(page.getByTestId("sprint-continue").first()).toHaveAttribute("data-lesson-id", first);
  await page.goto("/map");
  // One "You are here" on the whole map: the course's next lesson.
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveCount(1);
  await expect(page.locator('[data-testid="map-lesson"][data-here="true"]')).toHaveAttribute("data-lesson-id", first);
  await page.goto("/guide");
  await expect(page.locator('[data-testid="guide-stage"][data-current="true"]')).toHaveAttribute("data-lesson-id", first);
  expect((await apiProgress(page)).filter((l) => l.passed)).toEqual([]);
  // An untouched lesson says one thing, "Unread", not "Not started · Unread" (#399).
  await page.goto("/lessons?unit=all");
  const row = page.locator(`[data-testid="lessons-item"][data-lesson-id="${first}"]`);
  await expect(row.locator(".pill.unread")).toHaveCount(1);
  await expect(row.getByTestId("lessons-item-mastery")).toHaveCount(0);
});

test("1b. a stage read in Study but never played says Start in the Guide, not Try again (#417)", async ({ page }) => {
  const [first] = await firstTwo(page);
  const r = await page.request.post("/api/progress/seen", { data: { lesson: first } });
  expect(r.ok(), await r.text()).toBe(true);
  await page.goto("/guide");
  const stage = page.locator(`[data-testid="guide-stage"][data-lesson-id="${first}"]`);
  await expect(stage).toHaveAttribute("data-state", "progress");
  await expect(stage.getByTestId("guide-stage-play")).toHaveText("Start");
});

test("1c. a Guide stage played to the end under 70 % says Try again, not Continue past its last step (#417)", async ({ page }) => {
  const [first] = await firstTwo(page);
  await page.goto(`/guide/${first}`);
  const steps = await page.locator('[data-testid="stage-step"]').count();
  const [right, total] = await playStage(page, key, { wrong: () => true });
  expect(right, `0 of ${total} checks right`).toBe(0);
  await page.goto("/guide");
  const stage = page.locator(`[data-testid="guide-stage"][data-lesson-id="${first}"]`);
  await expect(stage).toHaveAttribute("data-state", "progress");
  await expect(stage.getByTestId("guide-stage-play"), `a finished run of ${steps} steps is not "Continue (step ${steps + 1} of ${steps})"`).toHaveText("Try again");
});

test("2. Study: read lesson 1, pass its check, practise it: passed everywhere", async ({ page }) => {
  const [first, second] = await firstTwo(page);
  await page.goto(`/lessons/${first}`);
  await passReadingCheck(page, key);
  await page.reload();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-read", "true");
  await page.goto(`/practice?lesson=${first}`);
  const [right, total] = await playPractice(page, key);
  expect(right, "every practice answer right").toBe(total);
  await expectPassedEverywhere(page, first, second);
});

test("3. Sprint: play lesson 1's session right: passed everywhere", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 390, height: 844 });
  const [first, second] = await firstTwo(page);
  await page.goto(`/sprint/play?lesson=${first}&reduce-motion=1`);
  const run = await playSprintRight(page, key);
  expect(run.wrong, "answers the app marked wrong").toEqual([]);
  await expect(page.getByTestId("sprint-play-done")).toHaveAttribute("data-saved", "true");
  await expectPassedEverywhere(page, first, second);
});

test("4. Play: play stage 1: passed everywhere", async ({ page }) => {
  const [first, second] = await firstTwo(page);
  await page.goto(`/guide/${first}`);
  const [right, total] = await playStage(page, key);
  expect(right, "every quick check right").toBe(total);
  await expectPassedEverywhere(page, first, second);
});

test("5. a lesson passed in one mode is passed in the other two", async ({ page }) => {
  const [first] = await firstTwo(page);
  await page.goto(`/guide/${first}`);
  await playStage(page, key);
  // Passed in Play: Study's lesson page and Sprint's path say so too, and Sprint offers it to play again.
  await page.goto(`/lessons/${first}`);
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-passed", "true");
  await page.goto("/sprint?reduce-motion=1");
  await expect(page.locator(`[data-testid="sprint-lesson"][data-lesson-id="${first}"]`)).toHaveAttribute("data-state", "complete");
  await page.goto("/lessons?unit=all");
  await expect(page.locator(`[data-testid="lessons-item"][data-lesson-id="${first}"]`)).toHaveAttribute("data-passed", "true");
});

test("6. progress survives logging out and back in", async ({ page }) => {
  const [first] = await firstTwo(page);
  await page.goto(`/guide/${first}`);
  await playStage(page, key);
  const before = await apiProgress(page);
  await logout(page);
  await login(page, account);
  expect(await apiProgress(page)).toEqual(before);
  await expectProgressAgrees(page);
});

test("7. a lesson whose every answer was right is not listed as needing practice (#681)", async ({ page }) => {
  const [first] = await firstTwo(page);
  await page.goto(`/lessons/${first}`);
  await passReadingCheck(page, key);
  await page.goto(`/lessons/${first}`);
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-status", "progress");
  for (const [path, testid] of [["/lessons?unit=all", "lessons-item"], ["/map", "map-lesson"]]) {
    await page.goto(path);
    await expect(page.locator(`[data-testid="${testid}"][data-lesson-id="${first}"]`), `${first} on ${path}`).toHaveAttribute("data-status", "progress");
    await expect(page.locator(`[data-testid="${testid}"][data-status="weak"]`), `nothing weak on ${path}`).toHaveCount(0);
  }
});

test("8. a Guide stage's quick checks mix their options, so the right one is not always first (#962)", async ({ page }) => {
  const [first] = await firstTwo(page);
  await page.goto(`/guide/${first}`);
  // Every step is in the page (hidden until reached): read each multiple-choice quick check's options in order.
  const checks = page.locator('[data-testid="stage-step"][data-qkind="multiple-choice"]');
  const n = await checks.count();
  expect(n, `${first}: multiple-choice quick checks`).toBeGreaterThanOrEqual(3);
  let firstIsRight = 0;
  for (const step of await checks.all()) {
    const id = (await step.getAttribute("data-qid"))!;
    const values = await step.getByTestId("stage-option").evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value));
    const answer = key.answer(id, values);
    expect(values, `question ${id}: the answer is among the options`).toContain(answer);
    if (values[0] === answer) firstIsRight++;
  }
  // The course files list the right answer first almost every time; shown in that order, all n would be first.
  expect(firstIsRight, `right answer first in ${firstIsRight} of ${n} quick checks`).toBeLessThan(n);
});

test("a learner registers, signs in again and passes a lesson", async ({ page }) => {
  await logout(page);
  await login(page, account);
  const [first] = await apiProgress(page);
  await page.goto(`/guide/${first.slug}`);
  const [right, total] = await playStage(page, key);
  expect(right, "every quick check right").toBe(total);
  expect((await apiProgress(page)).find((l) => l.slug === first.slug)?.passed, `${first.slug} passed`).toBe(true);
  await expectProgressAgrees(page);
});

test("the server says it is a Skrongle mini", async ({ request }) => {
  const res = await request.get("/healthz");
  expect(res.status()).toBe(200);
  expect(await res.text(), "/healthz names the edition (#641)").toMatch(/^ok skrongle mini \d+\.\d+\.\d+/);
});

test("the profile asks nobody to confirm their e-mail", async ({ page }) => {
  await page.goto("/profile");
  await expect(page.getByTestId("profile-form")).toBeVisible();
  // #10: nothing confirms an address, so neither a red ! nor an (i) about it.
  await expect(page.getByTestId("email-unverified")).toHaveCount(0);
  await expect(page.getByTestId("email-no-mail-info")).toHaveCount(0);
});

test("sharing shows the link once, and a reload goes back to the dialog without inviting again", async ({ page }) => {
  await page.goto("/courses");
  await page.getByTestId("courses-create").click();
  const id = await page.getByTestId("course-edit").getAttribute("data-course-id");
  expect(id, "the new course's id").toBeTruthy();
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-edit-share").click();
  await page.getByTestId("course-share-emails").fill("friend@example.org");
  await page.getByTestId("course-share-send").click();
  // #633: no mail, so the dialog shows the link to pass on.
  await expect(page.getByTestId("course-share-link")).toHaveValue(/\/courses\/shared\//);
  // #667: the address bar holds the course page, not the form's POST target.
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?share=1$`));
  await page.reload();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?share=1$`));
  await expect(page.getByTestId("course-share")).toBeVisible();
  await expect(page.getByTestId("course-share-link")).toHaveCount(0);
  await expect(page.getByTestId("course-share-row")).toHaveCount(1);
  await expect(page.locator('[data-testid="course-share-row"][data-email="friend@example.org"]')).toHaveAttribute("data-opened", "false");
  // The POST target opened by hand is a redirect to the dialog, not a 405.
  const res = await page.request.get(`/courses/${id}/share`, { maxRedirects: 0 });
  expect(res.status()).toBe(303);
  expect(res.headers()["location"]).toMatch(new RegExp(`/courses/${id}\\?share=1$`));
});
