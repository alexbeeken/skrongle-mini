// Exact spec for #1352, "Measure the angle": a `measure` question draws an
// angle and a protractor the learner drags and turns (mouse or keys), then
// reads and types the number. 180 minus the angle gets the other-scale note,
// a reading within the tolerance counts, and Sprint mode leaves it out.
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "@playwright/test";
import { devLogin, TEST_COURSE } from "../lib/app";

const PHONE = { width: 390, height: 844 };
const BOARD = { left: -1.75, top: 1.35, width: 3.5 }; // static/measure.js BOX
const HANDLE = 0.95 + 0.14; // R + the handle's distance past it

/** Where the protractor is: [x, y, turn in degrees], from the box's data-at. */
async function at(box: Locator): Promise<number[]> {
  return ((await box.getAttribute("data-at")) ?? "").split(",").map(Number);
}

/** A board point as page pixels. */
async function px(box: Locator, x: number, y: number): Promise<[number, number]> {
  const b = (await box.boundingBox())!;
  const u = b.width / BOARD.width;
  return [b.x + (x - BOARD.left) * u, b.y + (BOARD.top - y) * u];
}

async function drag(page: Page, from: [number, number], dx: number, dy: number): Promise<void> {
  await page.mouse.move(...from);
  await page.mouse.down();
  await page.mouse.move(from[0] + dx, from[1] + dy, { steps: 6 });
  await page.mouse.up();
}

test("drag and turn the protractor, read the angle, type it", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/practice?lesson=measure");
  const question = page.locator('[data-testid="quiz-question"]:visible');
  const box = question.getByTestId("measure");
  await expect(box).toHaveAttribute("data-ready", "true");
  await expect(box).toHaveAttribute("data-tool", "protractor");
  await expect(box).toHaveAttribute("aria-label", /Protractor/);
  await expect(box.locator("svg image")).toHaveCount(1);
  const angle = Number(await box.getAttribute("data-angle"));
  expect([125, 40, 72]).toContain(angle);

  // Drag the protractor by its body: it moves and does not turn.
  const [x0, y0, t0] = await at(box);
  await drag(page, await px(box, x0, y0 + 0.4), -60, -80);
  const [x1, y1, t1] = await at(box);
  expect(x1).toBeLessThan(x0 - 0.3);
  expect(y1).toBeGreaterThan(y0 + 0.3);
  expect(t1).toBe(t0);

  // Turn it by its handle: it turns about its centre.
  await drag(page, await px(box, x1 + HANDLE, y1), -30, -60);
  const [x2, y2, t2] = await at(box);
  expect([x2, y2]).toEqual([x1, y1]);
  expect(t2).toBeGreaterThan(10);

  // Keys: an arrow moves it a step, Shift with an arrow turns it a degree.
  await box.focus();
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Shift+ArrowRight");
  const [x3, , t3] = await at(box);
  expect(x3).toBeCloseTo(x2 - 0.02, 3);
  expect(t3).toBeCloseTo(t2 - 1, 1);
  // Enter goes to the answer box.
  await page.keyboard.press("Enter");
  await expect(question.getByTestId("quiz-answer")).toBeFocused();

  // Reading the other scale is wrong, with a note saying so.
  await page.keyboard.type(String(180 - angle));
  await page.keyboard.press("Enter");
  const feedback = question.getByTestId("quiz-feedback");
  await expect(feedback).toHaveAttribute("data-state", "wrong");
  await expect(feedback).toContainText("You read the other scale");
  await question.getByTestId("quiz-next").click();

  // The second question: a reading one degree off counts (tolerance 2).
  const second = page.locator('[data-testid="quiz-question"]:visible');
  const box2 = second.getByTestId("measure");
  await expect(box2).toHaveAttribute("data-ready", "true");
  const angle2 = Number(await box2.getAttribute("data-angle"));
  expect(angle2).not.toBe(angle);
  await second.getByTestId("quiz-answer").fill(`${angle2 + 1}°`);
  await second.getByTestId("quiz-check").click();
  await expect(second.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");

  // The board fits the phone.
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
});

test("the reading check and the Guide draw it; Sprint mode leaves it out", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/lessons/measure");
  await expect(page.getByTestId("lesson-check").getByTestId("measure")).toHaveCount(2);

  await page.goto("/guide/measure");
  await expect(page.getByTestId("stage-step").getByTestId("measure")).toHaveCount(3);

  // Sprint mode asks in text: none of the lesson's questions are in its session.
  await page.goto("/sprint/play?lesson=measure&reduce-motion=1");
  const session = await page.locator("script#session").textContent();
  expect(JSON.parse(session ?? "").exercises.length).toBeGreaterThan(0);
  expect(session).not.toContain("tc-measure-0");

  // A page without such a question never loads JSXGraph.
  await page.goto("/practice?lesson=numeric");
  await expect(page.getByTestId("quiz-question").first()).toBeAttached();
  await expect(page.locator('script[src*="jsxgraph"]')).toHaveCount(0);
});
