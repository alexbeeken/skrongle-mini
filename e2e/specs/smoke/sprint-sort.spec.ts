// Sprint "Sort it" (issue #1148): each tap on a bucket is judged at once. The
// right bucket takes the word, a wrong one shakes it and it stays on the pile;
// the sort checks itself once the pile is empty, wrong if there was a wrong tap.
// The sort is put first in the session (the page's session JSON is reordered
// on the way in); the answers come from the course files (lib/key.ts).
import type { Page } from "@playwright/test";
import { AnswerKey } from "../../lib/key.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { newAccount, register } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);
const LESSON = "nouns-gender-articles-plurals";

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

test.beforeEach(async ({ page }) => {
  await register(page, newAccount("sort"), COURSE);
});

test("a wrong bucket shakes the word and keeps it; the sort checks itself, wrong, and History says why", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  // Whichever of the lesson's sorts the session drew.
  const lesson = key.lesson(LESSON);
  await openWithFirst(page, `/sprint/play?lesson=${LESSON}&reduce-motion=1`, new RegExp(`^sort:${lesson.id}:\\d+$`));
  const stage = page.getByTestId("sprint-play-stage");
  await expect(stage).toHaveAttribute("data-kind", "sort");
  const sid = (await stage.getAttribute("data-exercise-id"))!;
  const sort = lesson.sorts[Number(sid.split(":")[2])];
  const bucketOf = new Map(sort.items.map((i) => [i.text, i.bucket]));
  const buckets = stage.getByTestId("sprint-sort-bucket");
  const chips = stage.getByTestId("sprint-sort-chip");
  const card = stage.locator(".sprint-sort-item");
  const result = page.getByTestId("sprint-play-result");

  // The first word into a wrong bucket: nothing lands, the word is still on top.
  const first = (await card.innerText()).trim();
  const right = bucketOf.get(first)!;
  await buckets.nth(right === 0 ? 1 : 0).click();
  await expect(buckets.nth(right === 0 ? 1 : 0)).toHaveClass(/miss/);
  await expect(chips).toHaveCount(0);
  await expect(card).toHaveText(first);
  // The right bucket takes it, marked right at once.
  await buckets.nth(right).click();
  await expect(chips).toHaveCount(1);
  await expect(stage.locator(".sprint-bucket-col").nth(right).getByTestId("sprint-sort-chip")).toHaveText(first);
  await expect(chips.first()).toHaveAttribute("data-state", "right");
  // The wrong bucket's red was a flash: the right tap clears it.
  await expect(stage.locator(".sprint-bucket.miss")).toHaveCount(0);

  // The rest, each into its bucket: the last one checks the sort, no Check tap.
  for (let k = 1; k < sort.items.length; k++) {
    await expect(result).toBeHidden();
    const text = (await card.innerText()).trim();
    await buckets.nth(bucketOf.get(text)!).click();
  }
  await expect(result).toHaveAttribute("data-state", "wrong");
  await expect(result).toContainText("All sorted, but with 1 wrong tap.");
  await expect(stage.locator('[data-testid="sprint-sort-chip"][data-state="right"]')).toHaveCount(sort.items.length);

  // History agrees with the ✗: the wrong tap comes first.
  await page.goto("/history");
  const row = page.locator('[data-testid="history-answer"]').filter({ hasText: /Sort it:/ }).first();
  await expect(row).toHaveAttribute("data-correct", "false");
  await expect(row).toContainText(`1 wrong tap, then all ${sort.items.length} sorted correctly`);
});

test("a clean sort, by the keyboard: Enter on the right bucket drops the word, and the sort checks itself right", async ({ page }) => {
  const lesson = key.lesson(LESSON);
  await openWithFirst(page, `/sprint/play?lesson=${LESSON}&reduce-motion=1`, new RegExp(`^sort:${lesson.id}:\\d+$`));
  const stage = page.getByTestId("sprint-play-stage");
  await expect(stage).toHaveAttribute("data-kind", "sort");
  const sort = lesson.sorts[Number((await stage.getAttribute("data-exercise-id"))!.split(":")[2])];
  const bucketOf = new Map(sort.items.map((i) => [i.text, i.bucket]));
  const buckets = stage.getByTestId("sprint-sort-bucket");
  const card = stage.locator(".sprint-sort-item");
  for (let k = 0; k < sort.items.length; k++) {
    const text = (await card.innerText()).trim();
    await buckets.nth(bucketOf.get(text)!).focus();
    await page.keyboard.press("Enter");
  }
  await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
  await expect(stage.getByTestId("sprint-sort-chip")).toHaveCount(sort.items.length);

  await page.goto("/history");
  const row = page.locator('[data-testid="history-answer"]').filter({ hasText: /Sort it:/ }).first();
  await expect(row).toHaveAttribute("data-correct", "true");
  await expect(row).toContainText(`Sorted ${sort.items.length} of ${sort.items.length} correctly`);
});
