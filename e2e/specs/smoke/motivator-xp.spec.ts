// The XP motivator (#561): everything done in a course earns XP (effort
// points ×10, +50 per lesson passed, +100 per daily goal, +250 per unit),
// and XP raises the learner's level (level n → n+1 costs 100·n XP). It is
// worked out from the shared tables, so XP earned while Statistics was worn
// is there on switching, and switching back and forth loses nothing.
import type { Page } from "@playwright/test";
import { AnswerKey } from "../../lib/key.ts";
import { passReadingCheck } from "../../lib/answer.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, newAccount, pickMotivator, register } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);

/** Pick a motivator in the course's settings (#574). */
const wear = (page: Page, id: string) => pickMotivator(page, COURSE, id);

/** The XP hero's total XP, after checking it is the one drawn. */
async function heroXp(page: Page): Promise<number> {
  await page.goto("/");
  const hero = page.getByTestId("home-today");
  await expect(hero).toHaveAttribute("data-motivator", "xp");
  return Number(await hero.getAttribute("data-xp"));
}

test("XP counts what was earned under any motivator and levels the learner up", async ({ page }) => {
  await register(page, newAccount("xp"), COURSE);
  const [a] = (await apiProgress(page)).filter((l) => l.required_answers >= 2);

  // Earn something while Statistics (the default) is worn.
  await page.goto(`/lessons/${a.slug}`);
  await passReadingCheck(page, key);
  await page.goto("/");
  await expect(page.getByTestId("home-today")).toHaveAttribute("data-motivator", "stats");
  const ring = await page.getByTestId("home-goal-ring").getAttribute("data-points");
  expect(Number(ring)).toBeGreaterThan(0);

  // Switching to XP shows it already: today's goal in XP, a level and its bar.
  await wear(page, "xp");
  const before = await heroXp(page);
  expect(before).toBeGreaterThanOrEqual(Number(ring) * 10);
  const today = page.getByTestId("xp-today");
  await expect(today).toHaveAttribute("data-xp", String(Number(ring) * 10));
  await expect(today).toHaveAttribute("data-goal", "200");
  await expect(today).toContainText(`today: ${Number(ring) * 10} / 200 XP`);
  const level = page.getByTestId("xp-level");
  const next = page.getByTestId("xp-next");
  const lv = Number(await level.getAttribute("data-level"));
  const into = Number(await next.getAttribute("data-into"));
  const need = Number(await next.getAttribute("data-need"));
  // Level n starts at 50·n·(n−1) XP and the step to the next costs 100·n.
  expect(50 * lv * (lv - 1) + into).toBe(before);
  expect(need).toBe(100 * lv);
  await expect(next).toContainText(`${into} / ${need} XP`);
  await expect(page.getByTestId("home-continue")).toBeVisible();
  await expect(page.getByTestId("home-goal-ring")).toHaveCount(0);

  // How XP is earned is behind the (i).
  await page.getByTestId("xp-info").click();
  const dialog = page.getByTestId("xp-info-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("xp-info-table")).toContainText("+50");
  await page.getByTestId("xp-info-close").click();
  await expect(dialog).toBeHidden();

  // A Sprint session that passes the lesson: "+N XP" (passing alone is +50),
  // and the hero grows by exactly that much. A run's window is whole seconds
  // (and a second of slack), so let the reading check fall out of it first.
  await page.waitForTimeout(2500);
  await page.goto(`/sprint/play?lesson=${a.slug}&reduce-motion=1`);
  await playSprintRight(page, key);
  const done = page.getByTestId("sprint-play-done");
  await expect(done).toHaveAttribute("data-saved", "true");
  const line = done.getByTestId("motivator-reward");
  await expect(line).toHaveAttribute("data-motivator", "xp");
  const gained = Number(await line.getByTestId("xp-reward-gained").getAttribute("data-xp"));
  expect(gained).toBeGreaterThanOrEqual(50);
  await expect(line).toContainText(`+${gained} XP`);
  // "Level up! Lv N" exactly when the run crossed into a new level.
  const levelOf = (xp: number) => {
    let n = 1;
    while (50 * (n + 1) * n <= xp) n++;
    return n;
  };
  const upTo = levelOf(before + gained);
  if (upTo > lv) await expect(line.getByTestId("xp-reward-level-up")).toHaveAttribute("data-level", String(upTo));
  else await expect(line.getByTestId("xp-reward-level-up")).toHaveCount(0);
  const after = await heroXp(page);
  expect(after).toBe(before + gained);
  await expect(level).toHaveAttribute("data-level", String(upTo));

  // Stats → XP → Stats → XP loses nothing.
  await wear(page, "stats");
  await page.goto("/");
  await expect(page.getByTestId("home-today")).toHaveAttribute("data-motivator", "stats");
  const ringAfter = await page.getByTestId("home-goal-ring").getAttribute("data-points");
  await wear(page, "xp");
  expect(await heroXp(page)).toBe(after);
  await wear(page, "stats");
  await page.goto("/");
  await expect(page.getByTestId("home-goal-ring")).toHaveAttribute("data-points", ringAfter ?? "0");
});
