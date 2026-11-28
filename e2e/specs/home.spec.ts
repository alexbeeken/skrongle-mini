// The course home, the learner path (#318): Up next is the lowest unfinished
// activity; the eye hides one, "Hidden (1)" brings it back. A Play is listed
// only once the day's goal is met. The top bar names the course's places,
// and on a phone they are a bar at the bottom.
import { expect, test, type Page } from "@playwright/test";
import { correctAnswers, devLogin, devReset } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devReset(page);
});

/** How far the page scrolls sideways, in CSS pixels. */
async function sidewaysScroll(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

async function firstLesson(page: Page): Promise<string> {
  await page.goto("/lessons?unit=all");
  return ((await page.getByTestId("lessons-item-link").first().textContent()) ?? "").trim();
}

test("Up next is the first lesson's Study; hide it, then bring it back", async ({ page }) => {
  await devLogin(page, "open=spanish");
  const first = await firstLesson(page);
  await page.goto("/");
  const next = page.getByTestId("path-next");
  await expect(next).toBeVisible();
  await expect(next).toHaveAttribute("data-kind", "study");
  await expect(next.getByRole("heading")).toHaveText(first);
  await expect(page.getByTestId("path-start")).toHaveAttribute("href", /\/study\//);
  const lesson = await next.getAttribute("data-lesson");
  await expect(page.getByTestId("path-then").getByTestId("path-item")).toHaveCount(3);
  await expect(page.locator('[data-testid="path-item"][data-kind="play"]')).toHaveCount(0);
  await expect(page.getByTestId("home-today")).toBeVisible();
  await expect(page.getByTestId("path-whole")).toHaveAttribute("href", /\/study$/);
  await expect(page.getByTestId("path-hidden-toggle")).toHaveCount(0);
  // One filled button on the page: Start.
  await expect(page.locator("main .btn:not(.ghost)").filter({ visible: true })).toHaveCount(1);

  // The eye hides that one activity: its Review moves up.
  await next.getByTestId("path-hide").click();
  await expect(next).toHaveAttribute("data-kind", "review");
  await expect(next).toHaveAttribute("data-lesson", lesson!);
  const toggle = page.getByTestId("path-hidden-toggle");
  await expect(toggle).toHaveText("Hidden (1)");
  await expect(page.getByTestId("path-hidden-item")).toBeHidden();
  await toggle.click();
  const hidden = page.getByTestId("path-hidden-item");
  await expect(hidden).toHaveCount(1);
  await expect(hidden).toHaveAttribute("data-kind", "study");

  // Bring back puts it first again.
  await hidden.getByTestId("path-unhide").click();
  await expect(next).toHaveAttribute("data-kind", "study");
  await expect(next).toHaveAttribute("data-lesson", lesson!);
  await expect(page.getByTestId("path-hidden-toggle")).toHaveCount(0);
});

test("a Play is listed once the day's goal is met", async ({ page }) => {
  await devLogin(page, "open=spanish");
  await page.goto("/");
  const goal = Number(await page.getByTestId("home-goal-ring").getAttribute("data-goal"));
  await expect(page.getByTestId("home-goal-ring")).toHaveAttribute("data-points", "0");
  await expect(page.locator('[data-kind="play"]')).toHaveCount(0);
  // Each right answer is one point.
  const answers = [...correctAnswers("spanish")].slice(0, goal);
  for (const [question_id, answer] of answers) {
    const r = await page.request.post("/api/answer", { data: { question_id, answer } });
    expect(r.ok()).toBe(true);
  }
  await page.goto("/");
  await expect(page.getByTestId("home-goal-ring")).toHaveAttribute("data-points", String(goal));
  const lesson = await page.getByTestId("path-next").getAttribute("data-lesson");
  await expect(page.getByTestId("path-then").locator(`[data-testid="path-item"][data-kind="play"][data-lesson="${lesson}"]`)).toHaveCount(1);
});

test("fits a phone, at 200 % text too", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await devLogin(page, "open=spanish");
  await expect(page.getByTestId("path-home")).toBeVisible();
  expect(await sidewaysScroll(page)).toBeLessThanOrEqual(1);
  await page.addStyleTag({ content: "html { font-size: 200% !important; }" });
  expect(await sidewaysScroll(page), "scrolls sideways at 200 % text").toBeLessThanOrEqual(1);
});

test("the unit row and the daily goal's (i)", async ({ page }) => {
  await devLogin(page);
  await page.goto("/");
  await expect(page.getByTestId("home-goal-ring")).toHaveAttribute("data-goal", "20");
  await expect(page.getByTestId("home-stat-streak")).toContainText("day streak");
  // The unit row names the current unit and how far through it you are.
  const map = page.getByTestId("home-map");
  await expect(map.getByTestId("home-map-unit")).toContainText(/Unit \d+ of \d+/);
  const progress = map.getByTestId("home-map-progress");
  const passed = Number(await progress.getAttribute("data-passed"));
  const total = Number(await progress.getAttribute("data-total"));
  expect(total).toBeGreaterThan(0);
  expect(passed).toBeLessThanOrEqual(total);
  const dialog = page.getByTestId("home-info-goal-dialog");
  await expect(dialog).toBeHidden();
  await page.getByTestId("home-info-goal").click();
  await expect(dialog).toBeVisible();
  await dialog.getByTestId("home-info-close").click();
  await expect(dialog).toBeHidden();
});

test.describe("without JavaScript", () => {
  test.use({ javaScriptEnabled: false });
  test("Start is a plain link", async ({ page }) => {
    await devLogin(page);
    await page.goto("/");
    await expect(page.getByTestId("path-home")).toBeVisible();
    await expect(page.getByTestId("path-start")).toHaveAttribute("href", /\/study\//);
  });
});

test("the top bar names the course's places and marks the one you are in", async ({ page }) => {
  await devLogin(page);
  await page.goto("/");
  const nav = page.getByTestId("nav-main");
  await expect(nav.locator("a")).toHaveText(["Today", "Study", "Review", "Play", "Tutor"]);
  await expect(page.getByTestId("topnav-today")).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("nav-course-switch")).toHaveAttribute("href", /\/courses$/);
  // A page under the Study bar is in Study, whatever its address; the bar has no "Switch" row.
  await page.goto("/vocabulary");
  await expect(page.getByTestId("topnav-study")).toHaveAttribute("aria-current", "true");
  await expect(page.getByTestId("topnav-today")).not.toHaveAttribute("aria-current");
  await expect(page.getByTestId("nav-study-course")).toHaveAttribute("href", /\/study$/);
  await expect(page.locator(".mode-others")).toHaveCount(0);
  await page.goto("/play/matching");
  await expect(page.getByTestId("topnav-play")).toHaveAttribute("aria-current", "true");
  // Study's list of lessons sits under the Study bar, on its first tab.
  await page.getByTestId("topnav-study").click();
  await expect(page).toHaveURL(/\/study$/);
  await expect(page.getByTestId("nav-study-course")).toHaveAttribute("aria-current", "page");
});

test("on a phone the places are a bar at the bottom of the screen", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await devLogin(page);
  await page.goto("/");
  const box = (await page.getByTestId("nav-main").boundingBox())!;
  expect(box.y + box.height, "the bar ends at the bottom edge").toBeGreaterThanOrEqual(843);
  expect(box.width).toBeGreaterThanOrEqual(389);
  for (const id of ["topnav-today", "topnav-study", "topnav-review", "topnav-play"]) {
    const b = (await page.getByTestId(id).boundingBox())!;
    expect(b.height, `${id} is tall enough to tap`).toBeGreaterThanOrEqual(44);
  }
  // The last row of the page is not under the bar.
  await page.getByTestId("path-whole").scrollIntoViewIfNeeded();
  const link = (await page.getByTestId("path-whole").boundingBox())!;
  const bar = (await page.getByTestId("nav-main").boundingBox())!;
  expect(link.y + link.height).toBeLessThanOrEqual(bar.y);
});

test("the old addresses of Study and Review redirect", async ({ page }) => {
  await devLogin(page);
  await page.goto("/guide");
  await expect(page).toHaveURL(/\/study$/);
  await page.goto("/sprint");
  await expect(page).toHaveURL(/\/review$/);
});
