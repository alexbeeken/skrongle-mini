// The AI workflows on a release build (#44). A release binary has no /dev routes
// and no built-in AI simulator, so scripts/release-e2e.sh starts a second process
// (the dev-feature build of the same code) that plays the providers from the
// fixtures, and points the release server's SKRONGLE_ANTHROPIC_BASE_URL at it.
// E2E_AI_SIM_URL is that process. This spec drives the release server only through
// what a self-hoster sees; the simulator's own request log (read from the spec's
// node side, not through the browser) proves the release server made the calls.
// Without E2E_AI_SIM_URL (no cargo, or the dev-mode run) the spec skips itself.
import { generationEnds, createCourse } from "../../lib/app.ts";
import { removeApiKey, saveApiKey } from "../../lib/learner.ts";
import { expect, newLearner, ownerPerson, test } from "../../lib/release.ts";
import { request, type Page } from "@playwright/test";

const SIM = process.env.E2E_AI_SIM_URL?.replace(/\/$/, "");
// The simulator's Learner key profile takes any sk-ant-dev-… key.
const KEY = "sk-ant-dev-release-ai-1234";

interface Call {
  id: number;
  format: string;
  purpose: string;
  status: number;
}

/** The simulator's call log, oldest first, read straight from the simulator process. */
async function simCalls(): Promise<Call[]> {
  // A request context of its own: this is the test talking to the simulator, not a person using the release server.
  const ctx = await request.newContext();
  let calls: Call[];
  try {
    const r = await ctx.get(`${SIM}/dev/ai?format=json`);
    expect(r.status(), "the simulator's /dev/ai").toBe(200);
    calls = (await r.json()).calls;
  } finally {
    await ctx.dispose();
  }
  return [...calls].sort((a, b) => a.id - b.id);
}

/** The purposes of the calls made since `lastId`, and that each one was an Anthropic-format 200. */
async function callsSince(lastId: number): Promise<string[]> {
  const calls = (await simCalls()).filter((c) => c.id > lastId);
  for (const c of calls) {
    expect(c.format, `call #${c.id} ${c.purpose}`).toBe("anthropic");
    expect(c.status, `call #${c.id} ${c.purpose}`).toBe(200);
  }
  return calls.map((c) => c.purpose);
}

async function lastCallId(): Promise<number> {
  const calls = await simCalls();
  return calls.length ? calls[calls.length - 1].id : 0;
}

/** Generate the fixture course from the dialog of a fresh draft; stays on its editor. Returns the course id. */
async function generate(page: Page, wish: string): Promise<string> {
  const id = await createCourse(page);
  await page.getByTestId("generate-wish").fill(wish);
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");
  await expect(page.getByTestId("course-lessons-title")).toHaveText(["Simplifying", "What a fraction is", "Adding fractions"]);
  return id;
}

test("AI on a release server, through the simulator: generate, learn, edit with AI, tutor, key removed", async ({ browser }) => {
  test.skip(!SIM, "no AI simulator: scripts/release-e2e.sh needs cargo (or SIM_BIN) to start one");
  test.setTimeout(240_000);

  // The owner has the server's key: saved on Manage Users, as a self-hoster does.
  const owner = await ownerPerson(browser);
  await saveApiKey(owner.page, KEY, "anthropic");
  const start = await lastCallId();

  // A learner generates a course; the release server calls the simulator for the outline and each lesson.
  const learner = await newLearner(browser, "ai", "Teacher");
  const id = await generate(learner.page, "Fractions for Friday");
  expect(await callsSince(start)).toEqual(["generate-outline", "generate-lesson", "generate-lesson", "generate-lesson"]);

  // They open it and pass a lesson's reading check; the answers come from the lesson file the editor serves.
  const lp = learner.page;
  const file = await lp.request.get(`/api/courses/${id}/file?path=${encodeURIComponent("lessons/adding.json")}`);
  expect(file.ok()).toBe(true);
  const quiz: { question_id: string; correct_answer: string }[] = JSON.parse((await file.json()).content).quiz;
  expect(quiz.length).toBeGreaterThan(0);
  expect((await lp.request.post(`/courses/open/${id}`, { maxRedirects: 0 })).status()).toBe(303);
  await lp.goto("/lessons/adding");
  await expect(lp.getByTestId("lesson-check")).toBeVisible();
  for (const q of await lp.getByTestId("lesson-check-question").all()) {
    const qid = (await q.getAttribute("data-qid"))!;
    const answer = quiz.find((x) => x.question_id === qid)!.correct_answer;
    const radios = q.locator("input[type=radio]");
    if (await radios.count()) {
      const values = await radios.evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value));
      // A true-false question shows "True"/"False" for the file's "true"/"false".
      const at = values.findIndex((v) => v.toLowerCase() === answer.toLowerCase());
      expect(at, `${qid}: ${answer} among ${values}`).toBeGreaterThanOrEqual(0);
      await radios.nth(at).check();
    } else {
      await q.locator("input.fill").fill(answer);
    }
  }
  await lp.getByTestId("lesson-check-submit").click();
  await expect(lp.getByTestId("lesson-check-result")).toHaveAttribute("data-state", "passed");
  await lp.reload();
  await expect(lp.getByTestId("lesson")).toHaveAttribute("data-read", "true");

  // The tutor answers: its greeting, then one turn to what the learner types.
  const beforeTutor = await lastCallId();
  await lp.goto("/tutor");
  await expect(lp.getByTestId("tutor-thread")).toContainText("practise this lesson together");
  await lp.getByTestId("tutor-input").fill("Can we try one more?");
  await lp.getByTestId("tutor-send").click();
  // The fixture's default tutor reply is the same greeting, so a second turn shows it twice.
  await expect
    .poll(async () => ((await lp.getByTestId("tutor-thread").innerText()).match(/practise this lesson together/g) ?? []).length)
    .toBe(2);
  expect(await callsSince(beforeTutor)).toEqual(["tutor-turn", "tutor-turn"]);

  // The owner makes a course of their own and uses Edit with AI on one lesson.
  const op = owner.page;
  await generate(op, "Fractions for the owner");
  const beforeEdit = await lastCallId();
  await op.locator('[data-testid="course-lessons-row"]', { hasText: "Simplifying" }).getByTestId("course-lessons-ai-edit").click();
  await expect(op.getByTestId("course-lessons-ai-dialog")).toBeVisible();
  await op.getByTestId("course-lessons-ai-text").fill("More worked examples, please");
  await op.getByTestId("course-lessons-ai-submit").click();
  await expect(op.getByTestId("course-review-modal")).toBeVisible({ timeout: 60_000 });
  await op.getByTestId("course-review-close").click();
  await expect(op.getByTestId("course-undo-label")).toContainText("Edited with feedback");
  expect(await callsSince(beforeEdit)).toEqual(["edit-lesson"]);

  // The owner removes the key: the learner is told to ask the owner, and no call is made.
  const beforeRemove = await lastCallId();
  await removeApiKey(op, "anthropic");
  await lp.goto("/tutor");
  await expect(lp.getByTestId("tutor-no-key")).toContainText("Ask the owner of this server");
  await expect(lp.getByTestId("tutor-add-key")).toHaveCount(0);
  expect(await lastCallId()).toBe(beforeRemove);
});
