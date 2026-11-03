// Exact spec for #1176, #1218, #1232 and #1227: a ```abc block in a lesson's Markdown is
// drawn as a music staff (an SVG) on the lesson page, a plain code block stays
// code, a quiz item's `abc` is drawn as a staff above its prompt in every mode
// and in the lesson editor, a music course plays its builds in Sprint mode,
// a lesson without notation doesn't load the notation scripts, and a Sprint
// spot with `abc` is a staff with one wrong note to tap and fix. #1187: a block
// that isn't music, or is empty, stays as its ABC text; #1311: a question's text wraps in its card.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

const PHONE = { width: 390, height: 844 };

test("an abc block in the lesson text is drawn as a staff", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/lessons/notation");
  const content = page.getByTestId("lesson-content");
  const staff = content.getByTestId("notation");
  await expect(staff).toHaveCount(1);
  const svg = staff.locator("svg");
  await expect(svg).toBeVisible();
  await expect(svg).toHaveAttribute("role", "img");
  await expect(svg).toHaveAttribute("aria-label", "Music notation");
  // Eight notes on the staff: C D E F G A B c.
  await expect(svg.locator(".abcjs-note")).toHaveCount(8);
  // The ABC text itself is hidden once drawn; the plain code block is still shown.
  await expect(content.locator("pre:has(code.language-abc)", { hasText: "C major scale" })).toBeHidden();
  await expect(content.locator("pre:not(:has(code.language-abc))")).toHaveText("C D E F");
  // The staff fits the phone.
  const width = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(width).toBeLessThanOrEqual(PHONE.width);
  // The Guide's card for that section draws it too.
  await page.goto("/guide/notation");
  const card = page.locator('[data-testid="stage-step"]:visible');
  await expect(card.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(8);
});

test("on a wide page a staff is as tall as its drawing, and the lesson's staff stays wider than a question's", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await devLogin(page, "open=" + TEST_COURSE);
  // abcjs sizes its box with a padding-bottom percentage of the parent's width;
  // on a wide page that left a tall blank gap under a capped staff.
  const fits = async (staff: Locator) => {
    await expect(staff.locator("svg")).toBeVisible();
    const [box, svg] = [await staff.boundingBox(), await staff.locator("svg").boundingBox()];
    expect(box!.height).toBeLessThanOrEqual(svg!.height + 8);
    return box!.width;
  };
  await page.goto("/lessons/notation");
  const lessonWidth = await fits(page.getByTestId("lesson-content").getByTestId("notation"));
  const questionWidth = await fits(page.getByTestId("lesson-check-question").first().getByTestId("notation"));
  expect(lessonWidth).toBeGreaterThan(questionWidth);
  await page.goto("/practice?lesson=notation");
  await fits(page.locator('[data-testid="quiz-question"]:visible').getByTestId("notation"));
});

test("a lesson without notation loads no notation script", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  for (const path of ["/lessons/read-only", "/practice?lesson=multiple-choice", "/sprint/play?lesson=multiple-choice", "/guide/multiple-choice"]) {
    await page.goto(path);
    await expect(page.locator("main, .sprint-play-main").first()).toBeVisible();
    await expect(page.locator('script[src*="abcjs"], script[src*="notation.js"]'), path).toHaveCount(0);
    await expect(page.getByTestId("notation")).toHaveCount(0);
    await expect(page.getByTestId("quiz-abc")).toHaveCount(0);
  }
});

// A question's staff above its prompt; `notes` on it ("Name the note" has one A).
async function expectStaffAbove(scope: Locator, prompt: Locator, notes = 1) {
  const staff = scope.getByTestId("notation");
  await expect(staff).toHaveCount(1);
  await expect(staff.locator("svg")).toHaveAttribute("aria-label", "Music notation");
  if (notes) await expect(staff.locator("svg .abcjs-note")).toHaveCount(notes);
  else expect(await staff.locator("svg .abcjs-note").count()).toBeGreaterThan(0);
  const [s, p] = [await staff.boundingBox(), await prompt.boundingBox()];
  expect(s!.y + s!.height).toBeLessThanOrEqual(p!.y + 1);
}
// A note on the staff is tapped with force: the staff lines cross the note's
// box, and abcjs finds the note nearest the tap itself.
// Sprint mode: through the session to the first exercise of `kind`. Bites
// move on with Check; a question gets its first option; the staff spot (when
// it isn't the one wanted) is answered right.
async function sprintTo(page: Page, kind: string) {
  const stage = page.getByTestId("sprint-play-stage");
  for (let i = 0; i < 12; i++) {
    await expect(stage).toHaveAttribute("data-kind", /./);
    const now = await stage.getAttribute("data-kind");
    if (now === kind) break;
    const id = await stage.getAttribute("data-exercise-id");
    if (now === "build") {
      await playFifths(page);
    } else if (now === "spot") {
      await stage.getByTestId("wrong-note-note").nth(3).click({ force: true });
      await stage.getByTestId("wrong-note-tile").filter({ hasText: /^F$/ }).click();
      await page.getByTestId("sprint-play-check").click();
      await page.getByTestId("sprint-play-next").click();
    } else if (now === "choice") {
      await stage.locator(".sprint-opt").first().click();
      await page.getByTestId("sprint-play-check").click();
      await page.getByTestId("sprint-play-next").click();
    } else {
      await page.getByTestId("sprint-play-check").click();
    }
    await expect(stage).not.toHaveAttribute("data-exercise-id", id!);
  }
  await expect(stage).toHaveAttribute("data-kind", kind);
  return stage;
}
const fitsPhone = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);

// The notation lesson's Sprint build (#1232): the keys in order of sharps.
const FIFTHS = ["C", "G", "D", "A", "E", "B"];
async function playFifths(page: Page) {
  const stage = page.getByTestId("sprint-play-stage");
  for (const key of FIFTHS) await stage.locator(".sprint-bank .sprint-tile", { hasText: new RegExp(`^${key}$`) }).click();
  await expect(stage.locator(".sprint-answer-line .sprint-tile")).toHaveText(FIFTHS);
  await page.getByTestId("sprint-play-check").click();
  await expect(stage.locator(".sprint-answer-line")).toHaveAttribute("data-state", "right");
  await page.getByTestId("sprint-play-next").click();
}

test("a music course's Sprint session has the circle-of-fifths build, graded exactly (#1232)", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/sprint/play?lesson=notation");
  const stage = await sprintTo(page, "build");
  await expect(stage.locator(".sprint-prompt")).toHaveText("Each key adds one sharp.");
  // Six shuffled key tiles and an empty row.
  await expect(stage.locator(".sprint-bank .sprint-tile")).toHaveCount(6);
  expect((await stage.locator(".sprint-bank .sprint-tile").allInnerTexts()).sort()).toEqual([...FIFTHS].sort());
  await expect(stage.locator(".sprint-answer-line .sprint-tile")).toHaveCount(0);
  // Two keys swapped is wrong, and the result shows the right order.
  for (const key of ["C", "D", "G", "A", "E", "B"]) await stage.locator(".sprint-bank .sprint-tile", { hasText: new RegExp(`^${key}$`) }).click();
  await page.getByTestId("sprint-play-check").click();
  await expect(stage.locator(".sprint-answer-line")).toHaveAttribute("data-state", "wrong");
  await expect(page.getByTestId("sprint-play-result")).toContainText("C G D A E B");
  await fitsPhone(page);
});

// Practice on the notation lesson asks its questions in a random order:
// answer the others (first option) until `qid` shows.
async function practiceUntil(page: Page, qid: string): Promise<Locator> {
  const q = page.locator('[data-testid="quiz-question"]:visible');
  for (let i = 0; i < 10 && (await q.getAttribute("data-qid")) !== qid; i++) {
    await q.getByTestId("quiz-option").first().click();
    await q.getByTestId("quiz-check").click();
    await q.getByTestId("quiz-next").click();
  }
  await expect(q).toHaveAttribute("data-qid", qid);
  return q;
}

test("a question's abc is drawn as a staff above its prompt in Practice, the reading check, Sprint and the Guide", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);

  // Practice: the staff, the prompt, the four letters; a wrong answer explains.
  await page.goto("/practice?lesson=notation");
  const q = await practiceUntil(page, "tc-notation-q1");
  await expectStaffAbove(q, q.locator(".prompt"));
  await expect(q.getByTestId("quiz-abc")).toBeHidden();
  await expect(q.locator(".prompt")).toHaveText("Which note is this?");
  await expect(q.getByTestId("quiz-option")).toHaveCount(4);
  await q.locator("label.option", { hasText: "G" }).click();
  await q.getByTestId("quiz-check").click();
  await expect(q.getByTestId("quiz-feedback")).toContainText("Count up from G on the second line");
  await fitsPhone(page);

  // The reading check on the lesson page.
  await page.goto("/lessons/notation");
  const check = page.getByTestId("lesson-check-question").first();
  await expectStaffAbove(check, check.locator(".prompt"), 0);

  // Sprint mode: past the bites, the circle-of-fifths build (#1232) and the staff spot to the question.
  await page.goto("/sprint/play?lesson=notation");
  const stage = await sprintTo(page, "choice");
  await expectStaffAbove(stage, stage.locator(".sprint-prompt"), 0);
  await expect(stage.locator(".sprint-opt")).toHaveCount(4);
  await stage.locator(".sprint-opt").first().click();
  await page.getByTestId("sprint-play-check").click();
  await expect(page.getByTestId("sprint-play-result")).toBeVisible();
  await fitsPhone(page);

  // The Guide's quick check.
  await page.goto("/guide/notation");
  const step = page.locator('[data-testid="stage-step"][data-qid="tc-notation-q1"]');
  await expect(step.getByTestId("quiz-abc")).toHaveCount(1);
  // Through the cards before it, one at a time (each slides out before the next shows).
  const index = Number(await step.getAttribute("data-index"));
  for (let i = 0; i < index; i++) {
    const card = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
    await expect(card).toBeVisible();
    await card.getByTestId("stage-continue").click();
    await expect(card).toBeHidden();
  }
  await expect(step).toBeVisible();
  await expectStaffAbove(step, step.locator(".prompt"));
});

test("the lesson editor has the question's Music box and previews the staff", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Music notation" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  const quiz = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
  if ((await quiz.getAttribute("aria-expanded")) !== "true") await quiz.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="0"]').click();
  const box = form.locator('[data-testid="course-form-field"][data-path="quiz.0.abc"]');
  await expect(box).toHaveValue("X:1\nL:1/4\nK:C\nA");
  const phone = form.getByTestId("course-preview-phone");
  await expect(phone.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(1);
  // Two notes typed: two on the staff.
  await box.fill("X:1\nL:1/4\nK:C\nA c");
  await expect(phone.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(2);
  await devReset(page);
});

test.describe("abc that isn't music stays as text (#1187)", () => {
  test("an empty and a non-ABC block in the lesson text keep their text", async ({ page }) => {
    await devLogin(page, "open=" + TEST_COURSE);
    await page.goto("/lessons/notation");
    const content = page.getByTestId("lesson-content");
    // Only the scale is drawn.
    await expect(content.getByTestId("notation")).toHaveCount(1);
    await expect(content.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(8);
    const junk = content.locator("pre:has(code.language-abc)", { hasText: "this is not music at all" });
    await expect(junk).toBeVisible();
    const empty = content.locator("pre:has(code.language-abc)").filter({ hasNotText: /\S/ });
    await expect(empty).toBeVisible();
  });

  test("a question's abc that isn't music shows as text in the editor's preview", async ({ page }) => {
    await devLogin(page, "open=" + TEST_COURSE);
    await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
    await copyCourse(page, TEST_COURSE);
    const row = page.getByTestId("course-lessons-row").filter({ hasText: "Music notation" }).first();
    await row.getByTestId("course-lessons-expand").click();
    const form = row.getByTestId("course-form");
    const quiz = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
    if ((await quiz.getAttribute("aria-expanded")) !== "true") await quiz.click();
    await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="0"]').click();
    const box = form.locator('[data-testid="course-form-field"][data-path="quiz.0.abc"]');
    const phone = form.getByTestId("course-preview-phone");
    await expect(phone.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(1);
    await box.fill("this is not music at all ~~~");
    const text = phone.locator("pre", { hasText: "this is not music at all ~~~" });
    await expect(text).toBeVisible();
    await expect(phone.getByTestId("notation")).toHaveCount(0);
    // The text wraps inside the phone frame instead of running past its edge (#1311).
    const [t, f] = [await text.boundingBox(), await phone.boundingBox()];
    expect(t!.x + t!.width).toBeLessThanOrEqual(f!.x + f!.width);
    expect(await text.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    await devReset(page);
  });
});

// #1220 "What is the interval?": two notes on the staff, one after the other
// (C E) or stacked ([DF]); the options are interval names, graded exactly,
// and the explanation counts the half steps.
test.describe("Pick the interval", () => {
  test("shows two notes on a staff and grades the interval", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);
    await page.goto("/practice?lesson=notation");

    // Both interval questions, in whichever order Practice asks them.
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const seen = new Set<string>();
    for (let i = 0; i < 10 && seen.size < 2; i++) {
      const qid = (await q.getAttribute("data-qid"))!;
      const feedback = q.getByTestId("quiz-feedback");
      if (qid === "tc-notation-q2") {
        // One after the other: C then E.
        await expectStaffAbove(q, q.locator(".prompt"), 2);
        await expect(q.locator(".prompt")).toHaveText("What is the interval?");
        await expect(q.getByTestId("quiz-option")).toHaveCount(4);
        await q.locator("label.option", { hasText: "minor 3rd" }).click();
        await q.getByTestId("quiz-check").click();
        await expect(feedback).toHaveAttribute("data-state", "wrong");
        await expect(feedback).toContainText("major 3rd");
        await expect(feedback).toContainText("C to E is four half steps");
      } else if (qid === "tc-notation-q3") {
        // Stacked: one chord, two note heads.
        await expectStaffAbove(q, q.locator(".prompt"), 0);
        await expect(q.getByTestId("notation").locator("svg .abcjs-notehead")).toHaveCount(2);
        await q.locator("label.option", { hasText: "minor 3rd" }).click();
        await q.getByTestId("quiz-check").click();
        await expect(feedback).toHaveAttribute("data-state", "right");
        await expect(feedback).toContainText("three half steps");
      } else {
        await q.getByTestId("quiz-option").first().click();
        await q.getByTestId("quiz-check").click();
      }
      if (qid === "tc-notation-q2" || qid === "tc-notation-q3") seen.add(qid);
      await fitsPhone(page);
      if (seen.size < 2) await q.getByTestId("quiz-next").click();
    }
    expect([...seen].sort()).toEqual(["tc-notation-q2", "tc-notation-q3"]);
  });
});

test.describe("Fix the wrong note: a Sprint spot on a staff (#1227)", () => {
  test("the learner taps the wrong note, then picks its fix; both must be right", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);
    const check = page.getByTestId("sprint-play-check");
    const result = page.getByTestId("sprint-play-result");

    // The staff: the C major scale with a G where the F should be, and its caption.
    await page.goto("/sprint/play?lesson=notation");
    let stage = await sprintTo(page, "spot");
    await expect(stage.locator(".sprint-sub")).toHaveText("One note is wrong. Tap it, then pick its fix.");
    const staff = stage.getByTestId("notation");
    await expect(staff.locator("svg")).toHaveAttribute("aria-label", "Music notation");
    await expect(staff.locator("svg")).toContainText("C major scale");
    const notes = stage.getByTestId("wrong-note-note");
    await expect(notes).toHaveCount(8);
    // The fix tiles show once a note is tapped; Check waits for both.
    const tiles = stage.getByTestId("wrong-note-tiles");
    await expect(tiles).toBeHidden();
    await expect(check).toBeDisabled();
    await notes.nth(3).click({ force: true });
    await expect(tiles).toBeVisible();
    expect((await stage.getByTestId("wrong-note-tile").allInnerTexts()).sort()).toEqual(["A", "E", "F"]);
    await expect(check).toBeDisabled();
    // The right note with the wrong fix: wrong, and the sheet says the note was right.
    await stage.getByTestId("wrong-note-tile").filter({ hasText: /^E$/ }).click();
    await expect(check).toBeEnabled();
    await check.click();
    await expect(result).toHaveAttribute("data-state", "wrong");
    await expect(result).toContainText("You found the note, but not its fix.");
    await expect(result).toContainText("Note 4 should be F");
    await expect(result).toContainText("after E comes F");
    await expect(stage.getByTestId("wrong-note-tile").filter({ hasText: /^E$/ })).toHaveAttribute("data-state", "wrong");
    await expect(stage.locator(".wrong-note-staff")).toHaveAttribute("data-state", "right");
    await fitsPhone(page);

    // Another note with the right fix: wrong too.
    await page.goto("/sprint/play?lesson=notation");
    stage = await sprintTo(page, "spot");
    await stage.getByTestId("wrong-note-note").nth(2).click({ force: true });
    await stage.getByTestId("wrong-note-tile").filter({ hasText: /^F$/ }).click();
    await check.click();
    await expect(result).toHaveAttribute("data-state", "wrong");
    await expect(result).not.toContainText("You found the note");
    await expect(stage.locator(".wrong-note-staff")).toHaveAttribute("data-state", "wrong");

    // The fourth note, fixed to F: right.
    await page.goto("/sprint/play?lesson=notation");
    stage = await sprintTo(page, "spot");
    await stage.getByTestId("wrong-note-note").nth(3).click({ force: true });
    await stage.getByTestId("wrong-note-tile").filter({ hasText: /^F$/ }).click();
    await check.click();
    await expect(result).toHaveAttribute("data-state", "right");
    await expect(stage.getByTestId("wrong-note-tile").filter({ hasText: /^F$/ })).toHaveAttribute("data-state", "right");
  });

  test("dragging the wrong note down a step (or the down arrow) picks the tile of the note it lands on", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);
    await page.goto("/sprint/play?lesson=notation");
    const stage = await sprintTo(page, "spot");
    // The stage slides in: measure the note once it has stopped.
    await stage.evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)));
    const head = (await stage.getByTestId("wrong-note-note").nth(3).locator(".abcjs-notehead").boundingBox())!;
    // Down by about a step: half the gap between two staff lines.
    const x = head.x + head.width / 2, y = head.y + head.height / 2;
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x, y + 3, { steps: 3 });
    await page.mouse.up();
    await expect(stage.getByTestId("wrong-note-tile").filter({ hasText: /^F$/ })).toHaveAttribute("aria-pressed", "true");
    await page.getByTestId("sprint-play-check").click();
    await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");

    // The same from the keyboard: the note takes focus, the down arrow moves it a step, Enter drops it.
    await page.goto("/sprint/play?lesson=notation");
    const again = await sprintTo(page, "spot");
    await again.getByTestId("wrong-note-note").nth(3).focus();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(again.getByTestId("wrong-note-tile").filter({ hasText: /^F$/ })).toHaveAttribute("aria-pressed", "true");
  });

  test("the editor's spot form has the Music box, the note number and the fix, and previews the staff", async ({ page }) => {
    await devLogin(page, "open=" + TEST_COURSE);
    await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
    await copyCourse(page, TEST_COURSE);
    const row = page.getByTestId("course-lessons-row").filter({ hasText: "Music notation" }).first();
    await row.getByTestId("course-lessons-expand").click();
    const form = row.getByTestId("course-form");
    const sprint = form.locator('[data-testid="course-outline-section"][data-sec="sprint"]');
    if ((await sprint.getAttribute("aria-expanded")) !== "true") await sprint.click();
    await form.locator('[data-testid="course-outline-item"][data-sec="sprint"][data-sub="spots"][data-idx="0"]').click();
    await expect(form.locator('[data-testid="course-form-field"][data-path="sprint.spots.0.abc"]')).toHaveValue(/CDEG GABc/);
    const wrong = form.locator('[data-testid="course-form-field"][data-path="sprint.spots.0.wrong"]');
    await expect(wrong).toHaveValue("4");
    await expect(form.locator('[data-testid="course-form-field"][data-path="sprint.spots.0.fix"]')).toHaveValue("F");
    await expect(form.locator('[data-testid="course-form-field"][data-path="sprint.spots.0.sentence"]')).toHaveCount(0);
    const phone = form.getByTestId("course-preview-phone");
    await expect(phone.getByTestId("notation").locator("svg .abcjs-note")).toHaveCount(8);
    await expect(phone).toContainText("taps note 4 and picks F");
    // A note number below 1 is flagged.
    await wrong.fill("0");
    await expect(form.locator('.le-check[data-check="sprint.spots.0.wrong"]')).toHaveAttribute("data-level", "bad");
    await devReset(page);
  });
});

// #1222 "Name this chord.": a stacked chord on the staff ([DFA], [FAc]); the
// options are chord names, graded exactly, and the explanation names the notes.
test.describe("Name the chord", () => {
  test("shows a stacked chord on a staff and grades its name", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);

    // D minor, answered wrong: the explanation names D, F and A.
    await page.goto("/practice?lesson=notation");
    let q = await practiceUntil(page, "tc-notation-q4");
    await expectStaffAbove(q, q.locator(".prompt"), 0);
    await expect(q.getByTestId("notation").locator("svg .abcjs-notehead")).toHaveCount(3);
    await expect(q.locator(".prompt")).toHaveText("Name this chord.");
    await expect(q.getByTestId("quiz-option")).toHaveCount(4);
    await q.locator("label.option", { hasText: "D major" }).click();
    await q.getByTestId("quiz-check").click();
    let feedback = q.getByTestId("quiz-feedback");
    await expect(feedback).toHaveAttribute("data-state", "wrong");
    await expect(feedback).toContainText("D minor");
    await expect(feedback).toContainText("D, F and A");
    await fitsPhone(page);

    // F major, answered right.
    await page.goto("/practice?lesson=notation");
    q = await practiceUntil(page, "tc-notation-q5");
    await expect(q.getByTestId("notation").locator("svg .abcjs-notehead")).toHaveCount(3);
    await q.locator("label.option", { hasText: "F major" }).click();
    await q.getByTestId("quiz-check").click();
    feedback = q.getByTestId("quiz-feedback");
    await expect(feedback).toHaveAttribute("data-state", "right");
    await fitsPhone(page);
  });
});

// #1221 "Name the key": a key signature on the staff (two sharps, with one
// whole note B, which is in every key on offer, so the staff is drawn), and a
// short tune in A minor, which has the same signature as C major and ends on
// A. The options are key names, graded exactly.
test.describe("Name the key", () => {
  test("shows a key signature or a tune and grades the key", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);

    // Two sharps, answered wrong: the explanation names F♯ and C♯.
    await page.goto("/practice?lesson=notation");
    let q = await practiceUntil(page, "tc-notation-q6");
    await expectStaffAbove(q, q.locator(".prompt"), 1);
    await expect(q.getByTestId("notation").locator("svg .abcjs-key-signature")).toHaveCount(1);
    await expect(q.locator(".prompt")).toHaveText("Which major key has this signature?");
    await expect(q.getByTestId("quiz-option")).toHaveCount(4);
    await q.locator("label.option", { hasText: "G major" }).click();
    await q.getByTestId("quiz-check").click();
    let feedback = q.getByTestId("quiz-feedback");
    await expect(feedback).toHaveAttribute("data-state", "wrong");
    await expect(feedback).toContainText("D major");
    await expect(feedback).toContainText("F♯ and C♯");
    await fitsPhone(page);

    // The tune in A minor, answered right.
    await page.goto("/practice?lesson=notation");
    q = await practiceUntil(page, "tc-notation-q7");
    await expectStaffAbove(q, q.locator(".prompt"), 7);
    await expect(q.getByTestId("notation").locator("svg .abcjs-key-signature")).toHaveCount(0);
    await expect(q.getByTestId("quiz-option")).toHaveCount(4);
    await q.locator("label.option", { hasText: "A minor" }).click();
    await q.getByTestId("quiz-check").click();
    feedback = q.getByTestId("quiz-feedback");
    await expect(feedback).toHaveAttribute("data-state", "right");
    await fitsPhone(page);
  });
});
