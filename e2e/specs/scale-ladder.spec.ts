// Exact spec for #1229, Build the scale: a lesson's `sprint.scales` is
// Sprint mode's ladder with notes. Eight rungs, the first filled; a bank of
// the scale's notes and two decoys in the course's note names (letters in
// the test course); a tap puts a note on the glowing rung, the staff above
// draws it and it plays; nothing is judged until Check ("6 of 8 notes
// right", wrong rungs show the right note). The lesson page shows the scale
// on a staff with a speaker, and the lesson editor has a Scale form.
// A browser test can't hear: the staff's data-played and data-tones say
// what the last tap played.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

const PHONE = { width: 390, height: 844 };
const fitsPhone = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
const SORT = new Map([["[CEG]2", 0], ["[A,CE]2", 1], ["[GBd]2", 0], ["[DFA]2", 1]]);

// Through the ear lesson's session to its scale: bites and questions move on
// (any option), the sort of speaker tiles is sorted right.
async function toScale(page: Page): Promise<Locator> {
  await page.goto("/sprint/play?lesson=ear&reduce-motion=1");
  const stage = page.getByTestId("sprint-play-stage");
  const check = page.getByTestId("sprint-play-check");
  for (let i = 0; i < 30; i++) {
    if (await page.getByTestId("sprint-play-result").isVisible()) { await page.getByTestId("sprint-play-next").click(); continue; }
    await expect(stage).toHaveAttribute("data-kind", /./);
    const kind = await stage.getAttribute("data-kind");
    if (kind === "scale") return stage;
    if (kind === "sort") {
      while (await stage.locator(".sprint-sort-item [data-play]").count()) {
        const play = (await stage.locator(".sprint-sort-item [data-play]").getAttribute("data-play"))!;
        await stage.locator(".sprint-bucket").nth(SORT.get(play)!).click();
      }
    } else if (kind === "choice") {
      await stage.locator(".sprint-opt").first().click();
      await check.click();
    } else {
      // A bite: Check moves on.
      const pos = (await stage.getAttribute("data-pos"))!;
      await check.click();
      await expect(stage).not.toHaveAttribute("data-pos", pos);
      continue;
    }
    await expect(page.getByTestId("sprint-play-result")).toBeVisible();
  }
  throw new Error("the ear session has no scale");
}

const tile = (stage: Locator, name: string) => stage.getByTestId("sprint-ladder-tile").filter({ hasText: new RegExp(`^${name}$`) });

test("Sprint mode: tap the notes of D major onto the ladder; graded at Check", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  let stage = await toScale(page);
  const check = page.getByTestId("sprint-play-check");
  const result = page.getByTestId("sprint-play-result");
  await expect(stage.locator(".sprint-kicker")).toHaveText("Build the scale");
  await expect(stage.locator(".sprint-title")).toHaveText("D major");
  // Eight rungs, the first filled with D; the second glows.
  const rungs = stage.getByTestId("sprint-ladder-rung");
  await expect(rungs).toHaveCount(8);
  await expect(rungs.nth(0).locator(".form")).toHaveText("D");
  await expect(rungs.nth(0)).toBeDisabled();
  await expect(rungs.nth(1)).toHaveClass(/next/);
  // The bank: the seven other notes and the decoys F and C, in letter names.
  const names = await stage.getByTestId("sprint-ladder-tile").allInnerTexts();
  expect(names.sort()).toEqual(["A", "B", "C", "C♯", "D", "E", "F", "F♯", "G"]);
  // The staff draws the notes on the rungs so far: D.
  const staff = stage.getByTestId("scale-staff");
  await expect(staff).toHaveAttribute("data-notes", "1");
  await expect(staff.locator("svg")).toHaveAttribute("aria-label", "Music notation");
  await expect(check).toBeDisabled();

  // A tap puts E on rung 2, draws it and plays it.
  await tile(stage, "E").click();
  await expect(rungs.nth(1).locator(".form")).toHaveText("E");
  await expect(staff).toHaveAttribute("data-notes", "2");
  await expect(staff).toHaveAttribute("data-played", "E");
  await expect(staff).toHaveAttribute("data-tones", "1");
  // Tapping the rung sends E back; the first rung can't be taken off.
  await rungs.nth(1).click();
  await expect(rungs.nth(1).locator(".form")).toHaveText("");
  await expect(tile(stage, "E")).toBeEnabled();
  await expect(staff).toHaveAttribute("data-notes", "1");

  // The decoys F and C on rungs 3 and 7: nothing is judged until Check.
  for (const n of ["E", "F", "G", "A", "B", "C", "D"]) await tile(stage, n).click();
  await expect(stage.getByTestId("sprint-ladder-rung").locator("[data-state]")).toHaveCount(0);
  await expect(staff).toHaveAttribute("data-notes", "8");
  await expect(staff).toHaveAttribute("data-played", "d");
  await check.click();
  await expect(result).toHaveAttribute("data-state", "wrong");
  await expect(result).toContainText("6 of 8 notes right");
  await expect(result).toContainText("D E F♯ G A B C♯ D");
  await expect(rungs.nth(0)).toHaveAttribute("data-state", "right");
  await expect(rungs.nth(2)).toHaveAttribute("data-state", "wrong");
  await expect(rungs.nth(2).locator(".fix")).toHaveText("F♯");
  await expect(rungs.nth(6).locator(".fix")).toHaveText("C♯");
  await expect(rungs.nth(7)).toHaveAttribute("data-state", "right");
  await fitsPhone(page);

  // Every note in its place: right.
  stage = await toScale(page);
  for (const n of ["E", "F♯", "G", "A", "B", "C♯", "D"]) await tile(stage, n).click();
  await check.click();
  await expect(result).toHaveAttribute("data-state", "right");
  await expect(result).toContainText("8 of 8 notes right");
});

test("Study: the lesson page shows the scale on a staff with a speaker", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/lessons/ear");
  const scale = page.getByTestId("lesson-scale");
  await expect(scale).toHaveCount(1);
  await expect(scale.locator("h3")).toHaveText("D major");
  await expect(scale.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(8);
  const speaker = scale.getByTestId("music-play");
  await speaker.click();
  await expect(speaker).toHaveAttribute("data-tones", "8");
  // A lesson without a scale has no Scales heading.
  await page.goto("/lessons/notation");
  await expect(page.getByTestId("lesson-scales")).toHaveCount(0);
});

test("the lesson editor's Scale form checks the notes and decoys and previews the scale", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Ear training" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  const sprint = form.locator('[data-testid="course-outline-section"][data-sec="sprint"]');
  if ((await sprint.getAttribute("aria-expanded")) !== "true") await sprint.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="sprint"][data-sub="scales"][data-idx="0"]').click();
  const field = (k: string) => form.locator(`[data-testid="course-form-field"][data-path="sprint.scales.0.${k}"]`);
  await expect(field("title")).toHaveValue("D major");
  await expect(field("notes")).toHaveValue("D E ^F G A B ^c d");
  await expect(field("decoys")).toHaveValue("F c");
  const phone = form.getByTestId("course-preview-phone");
  await expect(phone.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(8);
  await expect(phone).toContainText("taps the other 7 notes from 9 tiles");
  // A decoy that is a note of the scale, and seven notes, are flagged.
  await field("decoys").fill("F d");
  await expect(form.locator('.le-check[data-check="sprint.scales.0.decoys"]')).toHaveAttribute("data-level", "bad");
  await field("notes").fill("D E ^F G A B ^c");
  await expect(form.locator('.le-check[data-check="sprint.scales.0.notes"]')).toHaveAttribute("data-level", "bad");
  await devReset(page);
});
