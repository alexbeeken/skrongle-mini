// Exact spec for #1355: an algebra-tiles question shows a board of x², x and
// 1 tiles in Practice, the reading check and the Guide. Tiles drag and snap,
// move with the arrow keys, turn with R, and Enter checks. The typed answer is
// graded by the server: two sides multiplied, whose product is the tiles.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { courseDir, devLogin, TEST_COURSE } from "../lib/app";
import { AnswerKey } from "../lib/key";

const key = new AnswerKey(TEST_COURSE);
const PHONE = { width: 390, height: 844 };
const LESSON: { quiz: { question_id: string; tiles: string }[] } = JSON.parse(
  readFileSync(join(courseDir(TEST_COURSE), "lessons/1-questions/algebra-tiles.json"), "utf8"),
);
const tilesOf = (qid: string) => LESSON.quiz.find((x) => x.question_id === qid)!.tiles;

/** The pile a question's `tiles` makes, read from the course file. */
function pile(qid: string): { squares: number; strips: number; units: number } {
  const q = { tiles: tilesOf(qid) };
  const m = q.tiles.replace(/\s/g, "").match(/^(\d*)x\^2(?:\+(\d*)x)?(?:\+(\d+))?$/);
  if (!m) throw new Error(`unexpected tiles ${q.tiles}`);
  const n = (s: string | undefined, dflt: number) => (s === undefined ? dflt : s === "" ? 1 : Number(s));
  return { squares: n(m[1], 1), strips: n(m[2], 0), units: n(m[3], 0) };
}

async function pos(tile: Pick<Locator, "getAttribute">): Promise<[number, number]> {
  return [Number(await tile.getAttribute("data-x")), Number(await tile.getAttribute("data-y"))];
}

async function visibleQuestion(page: Page): Promise<Locator> {
  return page.locator('[data-testid="quiz-question"]:visible');
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
});

test("practice draws the pile, tiles drag, snap, move with keys and turn", async ({ page }) => {
  await page.goto("/practice?lesson=algebra-tiles");
  const q = await visibleQuestion(page);
  const qid = (await q.getAttribute("data-qid"))!;
  const want = pile(qid);
  const board = q.getByTestId("tiles");
  await expect(board).toBeVisible();
  const tiles = board.getByTestId("tile");
  await expect(tiles.and(page.locator('[data-kind="square"]'))).toHaveCount(want.squares);
  await expect(tiles.and(page.locator('[data-kind="strip"]'))).toHaveCount(want.strips);
  await expect(tiles.and(page.locator('[data-kind="unit"]'))).toHaveCount(want.units);
  // The board fits the phone.
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);

  // Drag the big square down into the open part of the board: it lands on the grid.
  // A tile picked up moves to the end of the board's markup (drawn on top), so
  // with two squares (2x² + …) `.first()` would be the other one after the drag:
  // hold this element itself (#189).
  const square = (await tiles.and(page.locator('[data-kind="square"]')).first().elementHandle())!;
  const before = await pos(square);
  const box = (await square.boundingBox())!;
  const boardBox = (await board.getByTestId("tiles-board").boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 7, boardBox.y + boardBox.height - box.height, { steps: 8 });
  await page.mouse.up();
  const after = await pos(square);
  expect(after[1]).toBeGreaterThan(before[1]);
  expect(Number.isInteger(after[0]) && Number.isInteger(after[1])).toBe(true);

  // The keyboard: a strip moves one step per arrow and R turns it.
  const strip = tiles.and(page.locator('[data-kind="strip"]')).first();
  await strip.focus();
  const [x0, y0] = await pos(strip);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  expect(await pos(strip)).toEqual([x0 + 1, y0 + 1]);
  await expect(strip).toHaveAttribute("data-turned", "0");
  await page.keyboard.press("r");
  await expect(strip).toHaveAttribute("data-turned", "1");
  // Turn turns the selected strip back; Start again puts every tile home.
  await board.getByTestId("tiles-turn").click();
  await expect(strip).toHaveAttribute("data-turned", "0");
  await board.getByTestId("tiles-reset").click();
  expect(await pos(square)).toEqual(before);

  // The page holds no answer: the tiles are all it knows.
  const html = await page.content();
  expect(html).not.toContain(key.answer(qid));
});

test("the typed sides are graded: two sides right, the expanded form gets a note", async ({ page }) => {
  await page.goto("/practice?lesson=algebra-tiles");
  // First question: the expanded pile is equal but not two sides.
  let q = await visibleQuestion(page);
  await q.getByTestId("quiz-answer").fill(tilesOf((await q.getAttribute("data-qid"))!));
  await q.getByTestId("quiz-check").click();
  const fb = q.getByTestId("quiz-feedback");
  await expect(fb).toHaveAttribute("data-state", "wrong");
  await expect(fb).toContainText("Write it as its two sides multiplied");
  await q.getByTestId("quiz-next").click();

  // Next question: the right sides, in the other order, checked with Enter on a tile.
  q = await visibleQuestion(page);
  const right = key.answer((await q.getAttribute("data-qid"))!);
  const swapped = right.match(/^(.*?\))\s*(\(.*\))$/);
  await q.getByTestId("quiz-answer").fill(swapped ? swapped[2] + swapped[1] : right);
  await q.getByTestId("tile").first().focus();
  await page.keyboard.press("Enter");
  await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");
});

test("the reading check and the Guide show the board too", async ({ page }) => {
  await page.goto("/lessons/algebra-tiles");
  const check = page.getByTestId("lesson-check-question");
  await expect(check.first().getByTestId("tiles")).toBeVisible();
  await expect(check.first().getByTestId("tile").first()).toBeVisible();

  await page.goto("/guide/algebra-tiles");
  let step = page.locator('[data-testid="stage-step"]:visible');
  for (let i = 0; i < 10; i++) {
    await expect(page.locator('[data-testid="stage-step"]:visible')).toHaveCount(1);
    const index = await page.locator('[data-testid="stage-step"]:visible').getAttribute("data-index");
    step = page.locator(`[data-testid="stage-step"][data-index="${index}"]`);
    if ((await step.getAttribute("data-kind")) === "question") break;
    await step.getByTestId("stage-continue").click();
    await expect(step).toBeHidden();
  }
  await expect(step).toHaveAttribute("data-qkind", "algebra-tiles");
  await expect(step.getByTestId("tiles")).toBeVisible();
  await expect(step.getByTestId("tile").first()).toBeVisible();
});

test("a lesson without tiles loads no board", async ({ page }) => {
  await page.goto("/practice?lesson=expression");
  await expect(page.locator('[data-testid="quiz-question"]:visible')).toBeVisible();
  await expect(page.locator('script[src*="algebra-tiles"], link[href*="algebra-tiles"]')).toHaveCount(0);
  await expect(page.getByTestId("tiles")).toHaveCount(0);
});
