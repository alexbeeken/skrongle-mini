// Words due (#1360): collected words come back on a schedule. When some
// are due, the course home shows a "Words due" card; its button opens a
// flashcard round of the due words. A missed card comes back once at the
// end, the summary says what comes back tomorrow, and the card goes away.
// The words are collected through POST /dev/vocab, backdated so they are due.
import { expect, test } from "@playwright/test";
import { newAccount, register } from "../lib/learner.ts";

test("due words show on the course home, a due round reviews them, and the card goes (#1360)", async ({ page }) => {
  await register(page, newAccount("words-due"), "spanish");

  // Nothing collected: the home page has no card.
  await page.goto("/");
  await expect(page.getByTestId("home-mode").first()).toBeVisible();
  await expect(page.getByTestId("home-due")).toHaveCount(0);

  // Collected today: due tomorrow, still no card.
  let r = await page.request.post("/dev/vocab", { data: { lesson: "estar-gerund-present-progressive", days_ago: 0 } });
  expect(r.ok()).toBe(true);
  await page.goto("/");
  await expect(page.getByTestId("home-due")).toHaveCount(0);

  // Collected three days ago: all 12 words of the lesson are due.
  r = await page.request.post("/dev/vocab", { data: { lesson: "asking-questions", days_ago: 3 } });
  expect((await r.json()).collected).toBe(12);
  await page.goto("/");
  const card = page.getByTestId("home-due");
  await expect(card).toHaveAttribute("data-due", "12");
  await expect(card.getByTestId("home-due-count")).toHaveText("12 words due");
  await expect(card.getByTestId("home-due-word")).toHaveCount(3);
  await expect(card.getByTestId("home-due-review")).toHaveText("Review 12 words");

  // The (i) explains the ladder.
  await card.getByTestId("home-info-due").click();
  const info = page.getByTestId("home-info-due-dialog");
  await expect(info).toBeVisible();
  await expect(info).toContainText("1, 3, 7, 16, 35, 75");
  await info.getByTestId("home-info-close").click();

  await card.getByTestId("home-due-review").click();
  await expect(page).toHaveURL(/\/vocabulary\/study\?due=1$/);
  const study = page.getByTestId("flashcards");
  await expect(study).toHaveAttribute("data-due", "1");
  await expect(page.getByTestId("flashcards-card")).toHaveCount(12);

  // Miss the first card, know the rest; the missed one comes back once at the end.
  const missed = await page.locator('[data-testid="flashcards-card"]:visible').getAttribute("data-word");
  const seen: string[] = [];
  for (let i = 0; i < 13; i++) {
    const c = page.locator('[data-testid="flashcards-card"]:visible');
    await expect(c).toHaveCount(1);
    seen.push((await c.getAttribute("data-word"))!);
    await c.click();
    await page.getByTestId(i === 0 ? "flashcards-no" : "flashcards-yes").click();
  }
  expect(seen[12]).toBe(missed);
  const done = page.getByTestId("flashcards-done");
  await expect(done).toBeVisible();
  await expect(done.locator("#st-known")).toHaveText("11");
  await expect(done.locator("#st-count")).toHaveText("12");
  await expect(done.getByTestId("flashcards-due-tomorrow")).toHaveText("Back tomorrow: 1");
  // Tomorrow: the missed word and the 11 collected today.
  await expect(done.getByTestId("flashcards-due-next")).toHaveText("Next: 12 words tomorrow");

  // Nothing due any more: the card is gone.
  await done.getByTestId("flashcards-due-home").click();
  await expect(page.getByTestId("home-mode").first()).toBeVisible();
  await expect(page.getByTestId("home-due")).toHaveCount(0);
  // And the due round has nothing to deal.
  await page.goto("/vocabulary/study?due=1");
  await expect(page).toHaveURL(/\/vocabulary$/);
});

test("on a phone the card stacks: the button runs the full width under the words (#1360)", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 800 });
  await register(page, newAccount("words-due-phone"), "spanish");
  await page.request.post("/dev/vocab", { data: { lesson: "asking-questions", days_ago: 3 } });
  await page.goto("/");
  const card = page.getByTestId("home-due");
  const box = (await card.boundingBox())!;
  const btn = (await card.getByTestId("home-due-review").boundingBox())!;
  const count = (await card.getByTestId("home-due-count").boundingBox())!;
  expect(btn.y).toBeGreaterThan(count.y + count.height - 1);
  expect(btn.width).toBeGreaterThan(box.width * 0.8);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(375);
});
