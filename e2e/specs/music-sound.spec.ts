// Exact spec for #1219: a quiz item's `play` is a speaker button that plays
// it with Web Audio tones, only after a tap; with no `abc` the played notes
// appear on a staff once the question is answered; `option_kind: "abc"`
// draws the options as staves; a sort item with `play` and no text is a
// speaker tile in Sprint mode; "Can't listen now" turns sound off for an
// hour (Sprint mode skips what needs it, the other modes show the staff).
// A browser test can't hear, so it checks the button's states: data-state
// (idle, playing, silent, off), data-ctx (the AudioContext's state) and
// data-tones (how many tones the last tap scheduled).
import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

const BAR = "B2 BB B2 B2|"; // the right answer of tc-ear-q3
const PHONE = { width: 390, height: 844 };
const fitsPhone = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);

// Tap the speaker: it plays (the context runs) and schedules `tones` tones.
async function playIt(scope: Locator, tones: number) {
  const b = scope.getByTestId("music-play");
  await expect(b).toHaveAttribute("data-state", "idle");
  await b.click();
  await expect(b).toHaveAttribute("data-ctx", "running");
  await expect(b).toHaveAttribute("data-tones", String(tones));
  await expect(b).toHaveAttribute("data-state", /playing|idle/);
  await expect(scope.getByTestId("music-no-sound")).toBeHidden();
}

// Build the scale (#1229, scale-ladder.spec.ts) is in the same session: fill its seven rungs with any tiles and check.
async function playScale(page: Page) {
  const stage = page.getByTestId("sprint-play-stage");
  for (let k = 0; k < 7; k++) await stage.locator('[data-testid="sprint-ladder-tile"]:not(:disabled)').first().click();
  await page.getByTestId("sprint-play-check").click();
  await expect(page.getByTestId("sprint-play-result")).toBeVisible();
}

test("Practice: the speaker plays the interval, the staff comes after the answer, and staves are options", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/practice?lesson=ear");
  await expect(page.locator('script[src*="music-sound.js"]')).toHaveCount(1);
  for (let n = 0; n < 3; n++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = await q.getAttribute("data-qid");
    if (id === "tc-ear-q3") {
      // The rhythm bar (#1225) has its own test below.
      await q.locator(`label.option:has(input[value="${BAR}"])`).click();
      await q.getByTestId("quiz-check").click();
    } else if (id === "tc-ear-q1") {
      // Hear the interval: a speaker and the names, no staff yet.
      await expect(q.getByTestId("notation")).toHaveCount(0);
      // Two notes: one after the other, then together.
      await playIt(q, 4);
      await q.locator("label.option", { hasText: "perfect 5th" }).click();
      await q.getByTestId("quiz-check").click();
      await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");
      await expect(q.getByTestId("music-staff").locator("svg .abcjs-note")).toHaveCount(2);
    } else {
      // Which melody plays: three staves to choose from.
      expect(id).toBe("tc-ear-q2");
      await expect(q.getByTestId("quiz-option-abc")).toHaveCount(3);
      await expect(q.locator("label.option svg")).toHaveCount(3);
      await expect(q.locator('label.option:has(input[value="CDEC"]) svg .abcjs-note')).toHaveCount(4);
      await playIt(q, 4);
      await q.locator('label.option:has(input[value="CDEC"])').click();
      await q.getByTestId("quiz-check").click();
      await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");
    }
    await fitsPhone(page);
    await q.getByTestId("quiz-next").click();
  }
  await expect(page.getByTestId("quiz-summary")).toHaveAttribute("data-correct", "3");
});

test("no sound a second after the tap: the speaker says so", async ({ page }) => {
  // An audio clock that never moves, as on an iPhone on silent or a blocked context.
  await page.addInitScript(() => {
    class Stuck {
      state = "suspended"; currentTime = 0; destination = {};
      resume() { return Promise.resolve(); }
      createOscillator() { return { type: "", frequency: { value: 0 }, connect() {}, start() {}, stop() {} }; }
      createGain() { return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {} }; }
    }
    (window as unknown as { AudioContext: unknown }).AudioContext = Stuck;
  });
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/guide/ear");
  const step = page.locator('[data-testid="stage-step"][data-kind="question"]').first();
  const index = Number(await step.getAttribute("data-index"));
  for (let i = 0; i < index; i++) {
    const card = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
    await card.getByTestId("stage-continue").click();
    await expect(card).toBeHidden();
  }
  await step.getByTestId("music-play").click();
  await expect(step.getByTestId("music-play")).toHaveAttribute("data-state", "silent");
  await expect(step.getByTestId("music-play")).toHaveAttribute("data-ctx", "suspended");
  await expect(step.getByTestId("music-no-sound")).toBeVisible();
});

test("Can't listen now: off for an hour, the notes to read instead, and a tap turns it back on", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  // The reading check asks two of the lesson's three questions; reload until q1 is one.
  const q = page.locator('[data-testid="lesson-check-question"][data-qid="tc-ear-q1"]');
  for (let i = 0; i < 20; i++) {
    await page.goto("/lessons/ear");
    await expect(page.getByTestId("lesson-check-question").first()).toBeAttached();
    if (await q.count()) break;
  }
  await q.scrollIntoViewIfNeeded();
  await expect(q.getByTestId("music-staff")).toHaveCount(0);
  await q.getByTestId("music-cant-listen").click();
  await expect(q.getByTestId("music-play")).toHaveAttribute("data-state", "off");
  await expect(q.getByTestId("music-staff").locator("svg .abcjs-note")).toHaveCount(2);
  const until = Number(await page.evaluate(() => localStorage.getItem("speech:audio-off-until")));
  expect(until - Date.now()).toBeGreaterThan(59 * 60 * 1000);
  // Still off on the next page: Sprint mode skips the exercises that need sound.
  await page.goto("/sprint/play?lesson=ear");
  const stage = page.getByTestId("sprint-play-stage");
  const seen: string[] = [];
  for (let i = 0; i < 20 && !(await page.getByTestId("sprint-play-done").isVisible()); i++) {
    if (await page.getByTestId("sprint-play-result").isVisible()) { await page.getByTestId("sprint-play-next").click(); continue; }
    seen.push(`${await stage.getAttribute("data-kind")} ${await stage.getAttribute("data-exercise-id")}`);
    if (seen.at(-1)!.startsWith("scale")) await playScale(page);
    else await page.getByTestId("sprint-play-check").click();
  }
  expect(seen.length).toBeGreaterThan(0);
  // A scale stays: its notes play as they land, but it is answered by reading them.
  expect(seen.filter((s) => !s.startsWith("bite") && !s.startsWith("scale"))).toEqual([]);
  // A tap on a muted speaker turns sound back on and plays.
  await page.goto("/practice?lesson=ear");
  const first = page.locator('[data-testid="quiz-question"]:visible');
  const b = first.getByTestId("music-play");
  await expect(b).toHaveAttribute("data-state", "off");
  await b.click();
  // The rhythm bar (tc-ear-q3) is five hits; the interval and the melody are four tones.
  await expect(b).toHaveAttribute("data-tones", (await first.getAttribute("data-qid")) === "tc-ear-q3" ? "5" : "4");
  expect(await page.evaluate(() => localStorage.getItem("speech:audio"))).toBe("1");
});

test("Sprint mode: the speaker, staves as options and speaker tiles in Sort it", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/sprint/play?lesson=ear");
  const stage = page.getByTestId("sprint-play-stage");
  const bucketOf = new Map([["[CEG]2", 0], ["[A,CE]2", 1], ["[GBd]2", 0], ["[DFA]2", 1]]);
  const did = new Set<string>();
  for (let i = 0; i < 30 && !(await page.getByTestId("sprint-play-done").isVisible()); i++) {
    if (await page.getByTestId("sprint-play-result").isVisible()) { await page.getByTestId("sprint-play-next").click(); continue; }
    const kind = await stage.getAttribute("data-kind"), id = (await stage.getAttribute("data-exercise-id")) ?? "";
    if (kind === "choice" && id === "tc-ear-q1") {
      await playIt(stage, 4);
      await stage.locator('.sprint-opt[data-value="perfect 5th"]').click();
      await page.getByTestId("sprint-play-check").click();
      await expect(page.getByTestId("sprint-play-result")).toBeVisible();
      await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
      await expect(stage.getByTestId("music-staff").locator("svg .abcjs-note")).toHaveCount(2);
    } else if (kind === "choice" && id === "tc-ear-q2") {
      await expect(stage.locator(".sprint-opt-staff svg")).toHaveCount(3);
      await stage.locator('.sprint-opt[data-value="CDEC"]').click();
      await page.getByTestId("sprint-play-check").click();
      await expect(page.getByTestId("sprint-play-result")).toBeVisible();
      await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
    } else if (kind === "choice" && id === "tc-ear-q3") {
      // The rhythm bar (#1225) has its own test below; here it only needs answering.
      await stage.locator(`.sprint-opt[data-value="${BAR}"]`).click();
      await page.getByTestId("sprint-play-check").click();
      await expect(page.getByTestId("sprint-play-result")).toBeVisible();
      await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
      continue;
    } else if (kind === "sort") {
      for (let k = 0; k < 4; k++) {
        const card = stage.locator(".sprint-sort-item");
        const play = (await card.locator("[data-play]").getAttribute("data-play"))!;
        await playIt(card, 3);
        await stage.locator(".sprint-bucket").nth(bucketOf.get(play)!).click();
      }
      await expect(page.getByTestId("sprint-play-result")).toBeVisible();
      await expect(page.getByTestId("sprint-play-result")).toHaveAttribute("data-state", "right");
      await expect(stage.locator(".sprint-chip.music-chip")).toHaveCount(4);
      // A placed tile replays on a tap and stays green (its button state must not grey it).
      const chip = stage.locator(".sprint-chip.music-chip").first(), bg = (l: Locator) => l.evaluate((e) => getComputedStyle(e).backgroundColor);
      const green = await bg(stage.locator(".sprint-chip.music-chip").nth(1));
      await chip.click();
      await expect(chip).toHaveAttribute("data-tones", "3");
      await expect(chip).toHaveAttribute("data-state", /playing|idle|silent/);
      expect(await bg(chip)).toBe(green);
    } else {
      if (kind === "scale") await playScale(page);
      else await page.getByTestId("sprint-play-check").click();
      continue;
    }
    did.add(kind === "sort" ? "sort" : id);
    await fitsPhone(page);
  }
  expect([...did].sort()).toEqual(["sort", "tc-ear-q1", "tc-ear-q2"]);
});

// #1308: a wrong answer to "Which melody plays?" shows the right melody as a
// staff, not its ABC text ("CDEC"), in Practice, the lesson check and Sprint mode.
test("a wrong melody: the answer line shows the right one as a staff, not as ABC", async ({ page }) => {
  await page.setViewportSize(PHONE);
  // From no progress, so the lesson check is open (the tests above pass it).
  await devLogin(page, "open=" + TEST_COURSE);
  await devReset(page);
  await devLogin(page, "open=" + TEST_COURSE);
  const staffNotABC = async (line: Locator) => {
    await expect(line.getByTestId("answer-staff").locator("svg .abcjs-note")).toHaveCount(4);
    await expect(line).not.toContainText("CDEC");
  };
  // Practice: the melody wrong, the other two right, so the lesson check
  // (two of the three, fresh ones first: quiz::check_questions) is sure to
  // hold the melody question.
  await page.goto("/practice?lesson=ear");
  for (let n = 0; n < 3; n++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = await q.getAttribute("data-qid");
    const melody = id === "tc-ear-q2";
    const pick = melody ? "CEDC" : id === "tc-ear-q1" ? "perfect 5th" : BAR;
    await q.locator(`label.option:has(input[value="${pick}"])`).click();
    await q.getByTestId("quiz-check").click();
    await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", melody ? "wrong" : "right");
    if (melody) {
      await staffNotABC(q.getByTestId("quiz-feedback"));
      await fitsPhone(page);
    }
    await q.getByTestId("quiz-next").click();
  }
  // The check draws two of the lesson's three questions: load until the melody is one of them.
  const check = page.locator('[data-testid="lesson-check-question"][data-qid="tc-ear-q2"]');
  for (let draws = 0; draws < 8 && !(await check.count()); draws++) await page.goto("/lessons/ear");
  await page.locator('[data-testid="lesson-check-question"]:not([data-qid="tc-ear-q2"]) label.option').first().click();
  await check.locator('label.option:has(input[value="CEDC"])').click();
  await page.getByTestId("lesson-check-submit").click();
  await expect(check.getByTestId("lesson-check-feedback")).toHaveAttribute("data-state", "wrong");
  await staffNotABC(check.getByTestId("lesson-check-feedback"));
  await page.goto("/sprint/play?lesson=ear&reduce-motion=1");
  const stage = page.getByTestId("sprint-play-stage");
  const result = page.getByTestId("sprint-play-result");
  for (let i = 0; i < 30 && !(await page.getByTestId("sprint-play-done").isVisible()); i++) {
    if (await result.isVisible()) { await page.getByTestId("sprint-play-next").click(); continue; }
    const kind = await stage.getAttribute("data-kind");
    const exercise = await stage.getAttribute("data-exercise-id");
    if (kind === "sort") {
      for (let k = 0; k < 4; k++) {
        const play = (await stage.locator(".sprint-sort-item [data-play]").getAttribute("data-play"))!;
        await stage.locator(".sprint-bucket").nth(["[CEG]2", "[GBd]2"].includes(play) ? 0 : 1).click();
      }
      await expect(result).toBeVisible();
      continue;
    }
    if (exercise !== "tc-ear-q2") {
      if (kind === "choice") await stage.locator(".sprint-opt").first().click();
      await page.getByTestId("sprint-play-check").click();
      // Checked, or moved on (a bite has no result), before the next round.
      await expect
        .poll(async () => (await page.getByTestId("sprint-play-result").isVisible()) || (await page.getByTestId("sprint-play-done").isVisible()) || (await stage.getAttribute("data-exercise-id")) !== exercise)
        .toBe(true);
      continue;
    }
    await stage.locator('.sprint-opt[data-value="CEDC"]').click();
    await page.getByTestId("sprint-play-check").click();
    await expect(result).toHaveAttribute("data-state", "wrong");
    await staffNotABC(result.locator(".r-answer"));
    await fitsPhone(page);
    return;
  }
  throw new Error("Sprint mode never showed the melody question");
});

test("a lesson without play loads no sound code", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  for (const path of ["/lessons/notation", "/practice?lesson=notation", "/sprint/play?lesson=notation", "/guide/notation"]) {
    await page.goto(path);
    await expect(page.locator("main, .sprint-play-main").first()).toBeVisible();
    await expect(page.locator('script[src*="music-sound.js"]'), path).toHaveCount(0);
  }
});

test("the lesson editor has the Sound box, the options switch and a sort item's sound", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Ear training" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  const quiz = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
  if ((await quiz.getAttribute("aria-expanded")) !== "true") await quiz.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="0"]').click();
  await expect(form.locator('[data-testid="course-form-field"][data-path="quiz.0.play"]')).toHaveValue("X:1\nL:1/2\nK:C\nC G");
  await expect(form.getByTestId("course-preview-phone").getByTestId("music-play")).toBeVisible();
  await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="1"]').click();
  const kinds = form.getByTestId("course-form-option-kind");
  await expect(kinds.locator('[data-value="abc"]')).toHaveAttribute("aria-pressed", "true");
  await kinds.locator('[data-value="text"]').click();
  await expect(kinds.locator('[data-value="text"]')).toHaveAttribute("aria-pressed", "true");
  const sprint = form.locator('[data-testid="course-outline-section"][data-sec="sprint"]');
  if ((await sprint.getAttribute("aria-expanded")) !== "true") await sprint.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="sprint"][data-sub="sorts"][data-idx="0"]').click();
  await expect(form.locator('[data-testid="course-form-field"][data-path="sprint.sorts.0.items.0.play"]')).toHaveValue("[CEG]2");
  await devReset(page);
});

// #1224: "Which melody plays?" in the Guide. Three staves, each the notes of
// an option after the header of `play`; the one that plays grades right, a
// melody one or two notes off grades wrong.
test.describe("Which one did you hear? (#1224)", () => {
  test("Guide: the options are three staves, a wrong melody grades wrong and the right one is marked", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);
    // Start from no progress, so the Guide opens at its first card even when
    // this test runs alone or twice (the Guide resumes at the last step).
    await devReset(page);
    await devLogin(page, "open=" + TEST_COURSE);
    // Opening the lesson makes it the current stage, so the Guide opens it
    // even when the lessons before it are not passed.
    await page.goto("/lessons/ear");
    await page.goto("/guide/ear");
    const steps = page.getByTestId("stage-step");
    const total = await steps.count();
    let step: Locator | null = null;
    for (let i = 0; i < total && !step; i++) {
      const s = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
      await expect(s).toBeVisible();
      if ((await s.getAttribute("data-qid")) === "tc-ear-q2") { step = s; break; }
      if ((await s.getAttribute("data-kind")) === "question") {
        await s.getByTestId("stage-option").first().check();
        await s.getByTestId("stage-check").click();
        await s.getByTestId("stage-next-step").click();
      } else {
        await s.getByTestId("stage-continue").click();
      }
      await expect(s).toBeHidden();
    }
    expect(step, "the Guide has the Which melody plays? step").not.toBeNull();
    const q = step!;
    await expect(q.getByTestId("quiz-option-abc")).toHaveCount(3);
    for (const melody of ["CDEC", "CEDC", "CDCE"]) {
      // Four quarter notes each: the header (L:1/4, K:C) came from `play`.
      await expect(q.locator(`label.option:has(input[value="${melody}"]) svg .abcjs-note`)).toHaveCount(4);
    }
    await playIt(q, 4);
    await q.locator('label.option:has(input[value="CEDC"])').click();
    await q.getByTestId("stage-check").click();
    await expect(q.getByTestId("stage-feedback")).toHaveAttribute("data-state", "wrong");
    await expect(q.locator('label.option:has(input[value="CEDC"])')).toHaveAttribute("data-state", "wrong");
    await expect(q.locator('label.option:has(input[value="CDEC"])')).toHaveAttribute("data-state", "correct");
    await fitsPhone(page);
  });
});

// #1223 "Major or minor?": the sort the issue asks for is the ear lesson's
// (#1219 added it). Since #1160 each bucket tap is graded at once, so a tile
// never lands in the wrong bucket: a wrong tap shakes it and it stays on the
// pile, still a speaker to replay, and the sheet counts the wrong tap.
test.describe("#1223 Major or minor? in Sort it", () => {
  test("speaker tiles, a wrong tap keeps the chord on the pile to hear again, each placed chord replays", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);
    await page.goto("/sprint/play?lesson=ear");
    const stage = page.getByTestId("sprint-play-stage");
    const bucketOf = new Map([["[CEG]2", 0], ["[A,CE]2", 1], ["[GBd]2", 0], ["[DFA]2", 1]]);
    let sorted = false;
    for (let i = 0; i < 30 && !sorted && !(await page.getByTestId("sprint-play-done").isVisible()); i++) {
      if (await page.getByTestId("sprint-play-result").isVisible()) { await page.getByTestId("sprint-play-next").click(); continue; }
      const kind = await stage.getAttribute("data-kind"), id = (await stage.getAttribute("data-exercise-id")) ?? "";
      if (kind !== "sort") {
        if (id === "tc-ear-q1") await stage.locator('.sprint-opt[data-value="perfect 5th"]').click();
        if (id === "tc-ear-q2") await stage.locator('.sprint-opt[data-value="CDEC"]').click();
        await page.getByTestId("sprint-play-check").click();
        continue;
      }
      await expect(stage.getByRole("heading", { name: "Major or minor?" })).toBeVisible();
      await expect(page.getByTestId("sprint-sort-bucket")).toHaveText(["major", "minor"]);
      const card = stage.locator(".sprint-sort-item");
      // A speaker, no words: the chord is the item.
      await expect(card.getByTestId("music-play")).toBeVisible();
      await expect(card.locator(":scope > span")).toHaveCount(0);
      const first = (await card.locator("[data-play]").getAttribute("data-play"))!;
      await page.getByTestId("sprint-sort-bucket").nth(bucketOf.get(first) === 0 ? 1 : 0).click();
      await expect(card).toHaveClass(/shake/);
      await expect(page.getByTestId("sprint-sort-chip")).toHaveCount(0);
      await expect(card.locator("[data-play]")).toHaveAttribute("data-play", first);
      // Still there to hear again (a triad: three tones).
      await playIt(card, 3);
      for (let k = 0; k < bucketOf.size; k++) {
        const play = (await card.locator("[data-play]").getAttribute("data-play"))!;
        if (k > 0) await playIt(card, 3);
        await page.getByTestId("sprint-sort-bucket").nth(bucketOf.get(play)!).click();
      }
      const result = page.getByTestId("sprint-play-result");
      await expect(result).toBeVisible();
      await expect(result).toHaveAttribute("data-state", "wrong");
      await expect(result).toContainText("1 wrong tap");
      // Every chord sits in its own bucket and replays on a tap.
      const chips = stage.locator(".sprint-chip.music-chip");
      await expect(chips).toHaveCount(4);
      for (let k = 0; k < 4; k++) {
        await chips.nth(k).click();
        await expect(chips.nth(k)).toHaveAttribute("data-tones", "3");
      }
      await fitsPhone(page);
      sorted = true;
    }
    expect(sorted).toBe(true);
  });
});

test.describe("Which bar did you hear? (#1225)", () => {
  // Every oscillator the page makes, to read what the speaker played.
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      const made: OscillatorNode[] = [];
      (window as unknown as { oscs: OscillatorNode[] }).oscs = made;
      const orig = AudioContext.prototype.createOscillator;
      AudioContext.prototype.createOscillator = function (this: AudioContext) {
        const o = orig.call(this);
        made.push(o);
        return o;
      };
    });
  });

  test("a percussion bar plays one pitch, the options are one-bar percussion staves, the right bar grades right", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await devLogin(page, "open=" + TEST_COURSE);
    await page.goto("/practice?lesson=ear");
    let q = page.locator('[data-testid="quiz-question"]:visible');
    for (let n = 0; n < 3 && (await q.getAttribute("data-qid")) !== "tc-ear-q3"; n++) {
      await q.locator("label.option").first().click();
      await q.getByTestId("quiz-check").click();
      await q.getByTestId("quiz-next").click();
      q = page.locator('[data-testid="quiz-question"]:visible');
    }
    await expect(q).toHaveAttribute("data-qid", "tc-ear-q3");
    // Three staves, each one line with the percussion clef and the 4/4 metre, five notes a bar.
    const staves = q.locator("label.option svg");
    await expect(staves).toHaveCount(3);
    for (let i = 0; i < 3; i++) {
      const svg = staves.nth(i);
      await expect(svg.locator(".abcjs-staff")).toHaveCount(1);
      await expect(svg.locator('[data-name="clefs.perc"]')).toHaveCount(1);
      await expect(svg.locator(".abcjs-time-signature")).toHaveCount(1);
      await expect(svg.locator(".abcjs-note")).toHaveCount(5);
    }
    // Five hits, all the same short drum-like tone: the rhythm, not a tune.
    await playIt(q, 5);
    const tones = await page.evaluate(() => (window as unknown as { oscs: OscillatorNode[] }).oscs.map((o) => `${o.type} ${o.frequency.value}`));
    expect(tones).toHaveLength(5);
    expect(new Set(tones).size).toBe(1);
    expect(tones[0]).toMatch(/^square /);
    await q.locator(`label.option:has(input[value="${BAR}"])`).click();
    await q.getByTestId("quiz-check").click();
    await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");
    await fitsPhone(page);
    // Leave the dev user as the editor test left it: a Practice run here locks the next spec's Guide stage.
    await devReset(page);
  });
});
