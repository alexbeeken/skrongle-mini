// Sprint: a check that cannot reach the server (issue #411) shows one error
// line, replaced on every retry, and gone once a check gets through. A tap
// exercise is put first in the session (the page's session JSON is reordered
// on the way in); the answer comes from the course files (lib/key.ts).
import type { Page } from "@playwright/test";
import { AnswerKey } from "../../lib/key.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { newAccount, register } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);
const LESSON = "nouns-gender-articles-plurals";

/** Open `path` with the first exercise `want` accepts moved to the front of the session. */
async function openWithFirst(page: Page, path: string, want: (e: { id?: string; kind?: string }) => boolean) {
  await page.route(/\/sprint\/play\?/, async (route) => {
    const res = await route.fetch();
    const html = (await res.text()).replace(/(<script id="session" type="application\/json">)([\s\S]*?)(<\/script>)/, (_m, a: string, json: string, c: string) => {
      const s = JSON.parse(json);
      const i = s.exercises.findIndex(want);
      if (i < 0) throw new Error("no such exercise in the session");
      s.exercises.unshift(s.exercises.splice(i, 1)[0]);
      return a + JSON.stringify(s).replace(/</g, "\\u003c") + c;
    });
    await route.fulfill({ response: res, body: html });
  });
  await page.goto(path);
}

test("a failing check shows one error line, replaced on retry, cleared on success", async ({ page }) => {
  await register(page, newAccount("checkerr"), COURSE);
  await page.setViewportSize({ width: 390, height: 844 });
  await openWithFirst(page, `/sprint/play?lesson=${LESSON}&reduce-motion=1`, (e) => e.kind === "pick" || e.kind === "choice");
  const stage = page.getByTestId("sprint-play-stage");
  await expect(stage).toHaveAttribute("data-kind", /^(pick|choice)$/);
  const kind = (await stage.getAttribute("data-kind"))!;
  const id = (await stage.getAttribute("data-exercise-id"))!;
  const a = key.sprint(kind, id);
  if (a.kind !== "option") throw new Error(`${kind} ${id}: no option in the key`);
  await stage.locator(`.sprint-opt[data-value="${a.value.replace(/"/g, '\\"')}"]`).click();

  // The network is down: three taps on Check, one line.
  await page.route(/\/api\/(sprint\/check|answer)$/, (r) => r.abort("internetdisconnected"));
  const check = page.getByTestId("sprint-play-check");
  const error = page.getByTestId("sprint-play-error");
  for (let k = 0; k < 3; k++) {
    await check.click();
    await expect(error).toHaveCount(1);
    await expect(check).toBeEnabled();
  }
  await expect(error).toBeVisible();

  // Back online: the check goes through and the line is gone.
  await page.unroute(/\/api\/(sprint\/check|answer)$/);
  await check.click();
  await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
  await expect(error).toHaveCount(0);
});

test("a typed answer stays editable after a failed check (issue #672)", async ({ page }) => {
  await register(page, newAccount("checkerrtyped"), COURSE);
  await page.setViewportSize({ width: 390, height: 844 });
  // A word gap with tiles, so "Type it instead" is offered.
  type Gap = { kind?: string; tiles?: unknown[]; math?: boolean };
  await openWithFirst(page, `/sprint/play?lesson=${LESSON}&reduce-motion=1`, (e) => {
    const g = e as Gap;
    return g.kind === "tiles" && !!g.tiles?.length && !g.math;
  });
  const stage = page.getByTestId("sprint-play-stage");
  await expect(stage).toHaveAttribute("data-kind", "tiles");
  const id = (await stage.getAttribute("data-exercise-id"))!;
  const a = key.sprint("tiles", id);
  if (a.kind !== "tile") throw new Error(`tiles ${id}: no tile in the key`);
  await stage.locator(".sprint-link:not(.sprint-cant)").click();
  const input = stage.locator("input.sprint-input");
  await input.fill(`x${a.value}`);

  // The network is down: Check fails, and the box can still be edited.
  await page.route(/\/api\/(sprint\/check|answer)$/, (r) => r.abort("internetdisconnected"));
  const check = page.getByTestId("sprint-play-check");
  await check.click();
  await expect(page.getByTestId("sprint-play-error")).toHaveCount(1);
  await expect(input).toBeEnabled();
  await input.fill(a.value);
  await expect(input).toHaveValue(a.value);

  // Back online: the corrected answer is right.
  await page.unroute(/\/api\/(sprint\/check|answer)$/);
  await check.click();
  await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
});
