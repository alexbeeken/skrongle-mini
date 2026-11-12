// The AI workflows on a release build (#44). A release binary has no /dev routes
// and no built-in AI simulator, so scripts/release-e2e.sh starts a second process
// (the dev-feature build of the same code) that plays the providers from the
// fixtures, and points the release server's SKRONGLE_ANTHROPIC_BASE_URL at it.
// E2E_AI_SIM_URL is that process (e2e/lib/release-ai.ts). This spec drives the
// release server only through what a self-hoster sees; the simulator's own request
// log (read from the spec's node side, not through the browser) proves the release
// server made the calls. The OpenAI wire format and Ollama's row are
// ai-formats.spec.ts. Without E2E_AI_SIM_URL (no cargo, or the dev-mode run) the
// spec skips itself.
import { removeApiKey, saveApiKey } from "../../lib/learner.ts";
import { callsSince, GENERATION, generateFixtureCourse, lastCallId, NO_SIM, SIM } from "../../lib/release-ai.ts";
import { expect, newLearner, ownerPerson, test } from "../../lib/release.ts";

// The simulator's Learner key profile takes any sk-ant-dev-… key.
const KEY = "sk-ant-dev-release-ai-1234";

/** The purposes of the calls made since `lastId`, each an Anthropic-format 200. */
async function purposesSince(lastId: number): Promise<string[]> {
  return (await callsSince(lastId, "anthropic")).map((c) => c.purpose);
}

test("AI on a release server, through the simulator: generate, learn, edit with AI, tutor, key removed", async ({ browser }) => {
  test.skip(!SIM, NO_SIM);
  test.setTimeout(240_000);

  // The owner has the server's key: saved on Manage Users, as a self-hoster does. Only this one, so the
  // generate dialog and the tutor use it (ai-formats.spec.ts saves and removes the others).
  const owner = await ownerPerson(browser);
  for (const p of ["openai", "ollama"]) await removeApiKey(owner.page, p);
  await saveApiKey(owner.page, KEY, "anthropic");
  const start = await lastCallId();

  // A learner generates a course; the release server calls the simulator for the outline and each lesson.
  const learner = await newLearner(browser, "ai", "Teacher");
  const id = await generateFixtureCourse(learner.page, "Fractions for Friday");
  expect(await purposesSince(start)).toEqual(GENERATION);

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

  // The tutor answers: its greeting, then a second turn to what the learner types. The token in the message
  // picks the fixture's `second-turn` reply (docs/testing.md, "Fixtures"), so a repeated greeting cannot pass for it.
  const beforeTutor = await lastCallId();
  await lp.goto("/tutor");
  await expect(lp.getByTestId("tutor-thread")).toContainText("practise this lesson together");
  await lp.getByTestId("tutor-input").fill("Can we try one more? [fixture:second-turn]");
  await lp.getByTestId("tutor-send").click();
  await expect(lp.getByTestId("tutor-thread")).toContainText("This is the tutor's second turn");
  expect((await lp.getByTestId("tutor-thread").innerText()).match(/practise this lesson together/g)).toHaveLength(1);
  expect(await purposesSince(beforeTutor)).toEqual(["tutor-turn", "tutor-turn"]);

  // The owner makes a course of their own and uses Edit with AI on one lesson.
  const op = owner.page;
  await generateFixtureCourse(op, "Fractions for the owner");
  const beforeEdit = await lastCallId();
  await op.locator('[data-testid="course-lessons-row"]', { hasText: "Simplifying" }).getByTestId("course-lessons-ai-edit").click();
  await expect(op.getByTestId("course-lessons-ai-dialog")).toBeVisible();
  await op.getByTestId("course-lessons-ai-text").fill("More worked examples, please");
  await op.getByTestId("course-lessons-ai-submit").click();
  await expect(op.getByTestId("course-review-modal")).toBeVisible({ timeout: 60_000 });
  await op.getByTestId("course-review-close").click();
  await expect(op.getByTestId("course-undo-label")).toContainText("Edited with feedback");
  expect(await purposesSince(beforeEdit)).toEqual(["edit-lesson"]);

  // The owner removes the key: the learner is told to ask the owner, and no call is made.
  const beforeRemove = await lastCallId();
  await removeApiKey(op, "anthropic");
  await lp.goto("/tutor");
  await expect(lp.getByTestId("tutor-no-key")).toContainText("Ask the owner of this server");
  await expect(lp.getByTestId("tutor-add-key")).toHaveCount(0);
  expect(await lastCallId()).toBe(beforeRemove);
});
