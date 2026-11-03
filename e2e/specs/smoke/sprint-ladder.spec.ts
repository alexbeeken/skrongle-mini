// Sprint ladder (issue #640): every tap on a form fills the glowing rung,
// right or wrong, however fast; a filled rung can be tapped to take its form
// back; nothing is judged until Check. History reads the run as a score whose
// words agree with its ✓ or ✗. The ladder is put first in the session (the
// page's session JSON is reordered on the way in); the answers come from the
// test course's files (lib/key.ts).
import type { Page } from "@playwright/test";
import { TEST_COURSE } from "../../lib/app.ts";
import { AnswerKey } from "../../lib/key.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { newAccount, register } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);
const LESSON = "conjugations";

/** Open `path` with the first exercise whose id matches `id` moved to the front of the session. */
async function openWithFirst(page: Page, path: string, id: RegExp) {
  await page.route(/\/sprint\/play\?/, async (route) => {
    const res = await route.fetch();
    const html = (await res.text()).replace(/(<script id="session" type="application\/json">)([\s\S]*?)(<\/script>)/, (_m, a: string, json: string, c: string) => {
      const s = JSON.parse(json);
      const i = s.exercises.findIndex((e: { id?: string }) => id.test(e.id ?? ""));
      if (i < 0) throw new Error(`no exercise ${id} in the session`);
      s.exercises.unshift(s.exercises.splice(i, 1)[0]);
      return a + JSON.stringify(s).replace(/</g, "\\u003c") + c;
    });
    await route.fulfill({ response: res, body: html });
  });
  await page.goto(path);
}

/** Open the lesson's ladder first; its forms in rung order, from the course files. */
async function openLadder(page: Page): Promise<string[]> {
  await page.setViewportSize({ width: 390, height: 844 });
  const lesson = key.lesson(LESSON);
  await openWithFirst(page, `/sprint/play?lesson=${LESSON}&reduce-motion=1`, new RegExp(`^ladder:${lesson.id}:`));
  const stage = page.getByTestId("sprint-play-stage");
  await expect(stage).toHaveAttribute("data-kind", "ladder");
  const a = key.sprint("ladder", (await stage.getAttribute("data-exercise-id"))!);
  if (a.kind !== "ladder") throw new Error("not a ladder");
  expect(new Set(a.forms).size, "the ladder's forms differ, so one can sit on a wrong rung").toBe(a.forms.length);
  return a.forms;
}

const tile = (page: Page, form: string) => page.getByTestId("sprint-ladder-tile").filter({ hasText: new RegExp(`^${form}$`) });

test.beforeEach(async ({ page }) => {
  await register(page, newAccount("ladder"), TEST_COURSE);
});

test("fast taps all land on the rungs, a filled rung gives its form back, all right grades right", async ({ page }) => {
  const forms = await openLadder(page);
  const rungs = page.getByTestId("sprint-ladder-rung");
  const check = page.getByTestId("sprint-play-check");
  await expect(rungs).toHaveCount(forms.length);

  // The last form first: it lands on the first rung, and nothing is judged yet.
  const last = forms[forms.length - 1];
  await tile(page, last).click({ delay: 0 });
  await expect(rungs.first()).toContainText(last);
  await expect(rungs.first()).not.toHaveAttribute("data-state");
  await expect(check).toBeDisabled();
  // The rest as fast as Playwright taps: every one lands.
  for (const f of forms.slice(0, -1)) await tile(page, f).click({ delay: 0 });
  for (const [i, f] of [last, ...forms.slice(0, -1)].entries()) await expect(rungs.nth(i)).toContainText(f);
  await expect(page.locator('[data-testid="sprint-ladder-tile"]:not(:disabled)')).toHaveCount(0);
  await expect(check).toBeEnabled();

  // Take every form back off its rung (the first rung's first), then tap them in order.
  for (let i = 0; i < forms.length; i++) {
    await rungs.nth(i).click();
    await expect(check, "an empty rung closes Check").toBeDisabled();
  }
  await expect(page.locator('[data-testid="sprint-ladder-tile"]:not(:disabled)')).toHaveCount(forms.length);
  for (const f of forms) await tile(page, f).click({ delay: 0 });
  for (const [i, f] of forms.entries()) await expect(rungs.nth(i)).toContainText(f);

  await check.click();
  await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
  await expect(page.locator('[data-testid="sprint-ladder-rung"][data-state="right"]')).toHaveCount(forms.length);
  await expect(page.getByTestId("sprint-play-result")).toContainText(`${forms.length} of ${forms.length} forms right`);

  await page.goto("/history");
  const row = page.locator(`[data-testid="history-answer"][data-lesson-id="${LESSON}"]`).filter({ hasText: /Ladder:/ });
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-correct", "true");
  await expect(row).toContainText(`${forms.length} of ${forms.length} forms right`);
  await expect(row).not.toContainText("wrong tap");
});

test("one form on a wrong rung grades wrong, shows the key, and History agrees with its ✗", async ({ page }) => {
  const forms = await openLadder(page);
  const n = forms.length;
  const rungs = page.getByTestId("sprint-ladder-rung");
  // The first two forms swapped. The bank holds just the ladder's forms, so one
  // form on a wrong rung always puts another on a wrong rung too.
  const order = [forms[1], forms[0], ...forms.slice(2)];
  for (const f of order) await tile(page, f).click();
  await page.getByTestId("sprint-play-check").click();
  const result = page.getByTestId("sprint-play-result");
  await expect(result).toHaveAttribute("data-state", "wrong");
  await expect(result).toContainText(`${n - 2} of ${n} forms right`);
  for (const f of forms) await expect(result, "the key names every form").toContainText(f);
  await expect(page.locator('[data-testid="sprint-ladder-rung"][data-state="wrong"]')).toHaveCount(2);
  await expect(page.locator('[data-testid="sprint-ladder-rung"][data-state="right"]')).toHaveCount(n - 2);
  // A wrong rung shows its right form beside the struck-through one.
  await expect(rungs.first().locator(".fix")).toHaveText(forms[0]);
  await expect(rungs.first().locator(".form")).toHaveText(forms[1]);

  await page.goto("/history");
  const row = page.locator(`[data-testid="history-answer"][data-lesson-id="${LESSON}"]`).filter({ hasText: /Ladder:/ });
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-correct", "false");
  await expect(row).toContainText(`${n - 2} of ${n} forms right`);
  await expect(row.locator("td:last-child")).toHaveText("✗");
  await expect(row).not.toContainText(`${n} of ${n} forms right`);
  await expect(row).not.toContainText("wrong tap");
});
