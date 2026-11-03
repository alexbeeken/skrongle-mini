// Skrongle Cash (#562): a motivator that pays play money (SKC) for progress,
// one balance per course, worked out from the shared tables
// (src/motivators/skc.rs). Its hero shows the balance and today's SKC
// against the daily goal in SKC; its reward line says what a run earned
// ("+12 SKC", bonuses called out). It keeps counting while another
// motivator is worn.
import type { Page } from "@playwright/test";
import { AnswerKey } from "../../lib/key.ts";
import { passReadingCheck } from "../../lib/answer.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, newAccount, pickMotivator, register, wearMotivator } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);

/** The SKC balance on the course home page. */
async function balance(page: Page): Promise<number> {
  await page.goto("/");
  await expect(page.getByTestId("home-today")).toHaveAttribute("data-motivator", "skc");
  return Number(await page.getByTestId("skc-balance").getAttribute("data-skc"));
}

test("Skrongle Cash draws the hero and reward line, and keeps counting while another motivator is worn", async ({ page }) => {
  await register(page, newAccount("skc"), COURSE);
  const [a, b] = (await apiProgress(page)).filter((l) => l.required_answers >= 2);

  // Picked in the course's settings, behind its gear on the course list.
  await pickMotivator(page, COURSE, "skc");

  // The hero: a new learner has nothing yet; the daily goal in SKC is 20.
  expect(await balance(page)).toBe(0);
  await expect(page.getByTestId("skc-balance").getByTestId("skc-symbol")).toBeVisible();
  // The number is drawn large; only the "SKC" unit after it is small (#906).
  const size = (sel: string) =>
    page.getByTestId("skc-balance").locator(sel).evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  expect(await size(".skc-amount")).toBeGreaterThan(await size("[data-testid=skc-unit]"));
  const today = page.getByTestId("skc-today");
  await expect(today).toHaveText("today: 0 / 20 SKC");
  await expect(page.getByTestId("home-continue")).toBeVisible();
  // How SKC is earned, and what the shop is for, behind the (i).
  await page.getByTestId("skc-info").click();
  const dialog = page.getByTestId("skc-info-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("skc-rates").locator("tbody tr")).toHaveCount(4);
  await expect(dialog.getByTestId("skc-info-shop")).toContainText("shop");
  await page.getByTestId("skc-info-close").click();
  await expect(dialog).toBeHidden();

  // A reading check earns SKC: the line says how much, the balance grows by it.
  await page.goto(`/lessons/${a.slug}`);
  await passReadingCheck(page, key);
  const line = page.getByTestId("lesson-check-result").getByTestId("motivator-reward");
  await expect(line).toHaveAttribute("data-motivator", "skc");
  await expect(line).toContainText(/\+\d+ SKC/);
  const earned = Number(await line.getByTestId("skc-reward").getAttribute("data-skc"));
  expect(earned).toBeGreaterThan(0);
  const first = await balance(page);
  expect(first).toBe(earned);
  await expect(today).toHaveAttribute("data-skc", String(first));

  // A Sprint session that passes the lesson calls the pass bonus out.
  await page.goto(`/sprint/play?lesson=${a.slug}&reduce-motion=1`);
  await playSprintRight(page, key);
  const done = page.getByTestId("sprint-play-done");
  await expect(done).toHaveAttribute("data-saved", "true");
  const sprintLine = done.getByTestId("motivator-reward");
  await expect(sprintLine).toHaveAttribute("data-motivator", "skc");
  await expect(sprintLine.getByTestId("skc-bonus").filter({ hasText: "+5 SKC lesson passed" })).toHaveCount(1);
  const second = await balance(page);
  expect(second).toBeGreaterThan(first + 5);

  // Wearing another motivator, the learner earns more; switching back shows it.
  await wearMotivator(page, COURSE, "stats");
  await page.goto(`/lessons/${b.slug}`);
  await passReadingCheck(page, key);
  await expect(page.getByTestId("lesson-check-result").getByTestId("motivator-reward")).toHaveAttribute("data-motivator", "stats");
  await wearMotivator(page, COURSE, "skc");
  expect(await balance(page)).toBeGreaterThan(second);
});
