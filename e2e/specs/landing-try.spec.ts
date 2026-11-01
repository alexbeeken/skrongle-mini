// The landing page's "try it" stage (#1365): a visitor plays a short sample
// of each mode in each subject (language, maths, music), each with its own
// kind of question: listening and typing, shading, number lines, graphs,
// formulas, a staff, a keyboard. Then the "What can I do with it?" section,
// on the landing page and on the waitlist page (hosted only). No key, no network.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";

/** Turn the waitlist on or off from the Release page, if it isn't already. */
async function setWaitlist(page: Page, on: boolean): Promise<void> {
  await page.goto("/superuser/release");
  if ((await page.getByTestId("release-page").getAttribute("data-waitlist")) === String(on)) return;
  await page.getByTestId("release-switch").click();
  await page.getByTestId("release-switch-confirm").click();
  await expect(page.getByTestId("release-page")).toHaveAttribute("data-waitlist", String(on));
}

// Every other spec signs up the normal way: the waitlist never stays on.
test.afterAll(async ({ browser }) => {
  if (process.env.E2E_MINI === "1") return;
  const page = await browser.newPage();
  await devLogin(page, "next=/superuser");
  await setWaitlist(page, false);
  await page.close();
});

/** Click a drawing at a spot given as fractions of its width and height. */
async function clickAt(target: Locator, fx: number, fy: number): Promise<void> {
  const box = await target.boundingBox();
  if (!box) throw new Error("not drawn");
  await target.page().mouse.click(box.x + box.width * fx, box.y + box.height * fy);
}

/** Every answer drawn as a picture has a name a screen reader can say (#1395). */
async function picturesNamed(buttons: Locator, n: number): Promise<void> {
  await expect(buttons).toHaveCount(n);
  for (let i = 0; i < n; i++) await expect(buttons.nth(i)).toHaveAccessibleName(/.+/);
}

function helpers(page: Page) {
  const stage = page.getByTestId("try-stage");
  return {
    stage,
    done: stage.getByTestId("try-done"),
    opt: (name: string) => stage.getByTestId("try-opt").filter({ hasText: new RegExp(`^${name}$`) }),
    value: (v: string) => stage.locator(`[data-testid="try-opt"][data-value="${v}"]`),
    async open(subject: string, mode: string) {
      await page.getByTestId(`try-subject-${subject}`).click();
      await page.getByTestId(`try-tab-${mode}`).click();
    },
    /** Play's pairs: left item i goes with the right item of the same data-pair. */
    async matchAll(n: number) {
      for (let i = 0; i < n; i++) {
        await stage.locator(`[data-testid="try-match-term"][data-pair="${i}"]`).click();
        await stage.locator(`[data-testid="try-match-meaning"][data-pair="${i}"]`).click();
      }
      await expect(stage.getByTestId("try-done")).toContainText(/All pairs in \d+\.\d s/);
    },
  };
}

async function playLanguage(page: Page): Promise<void> {
  const { stage, done, opt, open, matchAll } = helpers(page);

  // Sprint: pick, listen (wrong on purpose: the right one still shows), build.
  await open("language", "sprint");
  await opt("thank you").click();
  await expect(opt("thank you")).toHaveClass(/right/);
  await expect(stage.getByTestId("try-speaker")).toBeVisible();
  await opt("Buenos días").click();
  await expect(opt("Buenas noches")).toHaveClass(/right/);
  await expect(opt("Buenos días")).toHaveClass(/wrong/);
  for (const w of ["¿Cómo", "estás?"]) await stage.getByTestId("try-tile").filter({ hasText: w }).click();
  await stage.getByTestId("try-check").click();
  await expect(done).toContainText("2 of 3 right");
  await stage.getByTestId("try-again").click();
  await expect(opt("thank you")).toBeVisible();

  // Study: read, a choice, then a typed word where a missing accent is a near miss.
  await open("language", "study");
  await stage.getByTestId("try-read").click();
  await opt("Buenos días").click();
  await stage.getByTestId("try-next").click();
  await stage.getByTestId("try-input").fill("estas");
  await stage.getByTestId("try-check").click();
  await expect(stage.getByTestId("try-input")).toHaveClass(/right/);
  await expect(stage).toContainText("Watch the accent: estás");
  await stage.getByTestId("try-next").click();
  await expect(done).toContainText("Lesson passed: 2 of 2");

  // Play: a wrong pair flashes, then all four.
  await open("language", "play");
  await stage.locator('[data-testid="try-match-term"][data-pair="0"]').click();
  await stage.locator('[data-testid="try-match-meaning"][data-pair="1"]').click();
  await expect(stage.locator('[data-testid="try-match-meaning"][data-pair="1"]')).toHaveClass(/wrong/);
  await matchAll(4);

  // Guide: a card, a true-or-false, a flashcard, stars.
  await open("language", "guide");
  await stage.getByTestId("try-next").click();
  await opt("True").click();
  await stage.getByTestId("try-flash").click();
  await expect(stage.getByTestId("try-flash")).toHaveText("well, fine");
  await stage.getByTestId("try-got-it").click();
  await expect(done).toContainText("★★★");
}

async function checkUses(page: Page): Promise<void> {
  const uses = page.getByTestId("landing-uses");
  await expect(uses.getByRole("heading", { level: 2 })).toHaveText("What can I do with it?");
  await expect(uses.locator("h3")).toHaveCount(9);
  await expect(uses).toContainText("Learn a niche topic");
}

test("a visitor plays the language sample of every mode", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("try-tab-sprint")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("try-subject-language")).toHaveAttribute("aria-pressed", "true");
  await playLanguage(page);
  await checkUses(page);
});

test("maths: shade, the number line, plot a point, a formula, graphs", async ({ page }) => {
  await page.goto("/");
  const { stage, done, open, value, matchAll } = helpers(page);

  await open("maths", "sprint");
  for (let i = 0; i < 3; i++) await stage.getByTestId("try-slice").nth(i).click();
  await stage.getByTestId("try-check").click();
  // The number line runs from x = 20 to 280 of 300: 0.75 is at 215.
  await clickAt(stage.locator(".try-nline"), 215 / 300, 0.5);
  await stage.getByTestId("try-check").click();
  await expect(stage.locator(".try-nline .marker")).toHaveClass(/right/);
  for (const w of ["1/3", "0.5", "3/4"]) await stage.getByTestId("try-tile").filter({ hasText: new RegExp(`^${w.replace("/", "\\/")}$`) }).click();
  await stage.getByTestId("try-check").click();
  await expect(done).toContainText("3 of 3 right");

  await open("maths", "study");
  await expect(stage.locator(".try-graph")).toBeVisible();
  await stage.getByTestId("try-read").click();
  // The grid spans x −1..5 and y −2..6 inside 18 px of padding on 220: (2, 3) is at (110, 87).
  await clickAt(stage.locator(".try-graph"), 110 / 220, 87 / 220);
  await stage.getByTestId("try-check").click();
  await expect(stage.locator(".try-graph .pick")).toHaveClass(/right/);
  await stage.getByTestId("try-next").click();
  await stage.getByTestId("try-input").fill("(x+1)(x+1)");
  await stage.getByTestId("try-check").click();
  await expect(stage.getByTestId("try-input")).toHaveClass(/right/);
  await stage.getByTestId("try-next").click();
  await expect(done).toContainText("Lesson passed: 2 of 2");

  await open("maths", "play");
  await expect(stage.locator('[data-testid="try-match-meaning"] .try-graph')).toHaveCount(4);
  await picturesNamed(stage.getByTestId("try-match-meaning"), 4);
  await matchAll(4);

  await open("maths", "guide");
  await stage.getByTestId("try-next").click();
  await picturesNamed(stage.getByTestId("try-opt"), 3);
  await expect(value("down")).toHaveAccessibleName("graph: a line going down");
  await value("down").click();
  await expect(value("down")).toHaveClass(/right/);
  await stage.getByTestId("try-flash").click();
  await expect(stage.getByTestId("try-flash")).toHaveText("y = x², a parabola");
  await stage.getByTestId("try-got-it").click();
  await expect(done).toContainText("★★★");
});

test("music: read the staff, hear it, play the keys, place a note", async ({ page }) => {
  await page.goto("/");
  const { stage, done, opt, open, value, matchAll } = helpers(page);

  await open("music", "sprint");
  await expect(stage.getByTestId("try-staff")).toBeVisible();
  await opt("B").click();
  await opt("a 5th").click();
  await stage.locator('[data-testid="try-key"][data-note="E4"]').click();
  await expect(done).toContainText("3 of 3 right");

  await open("music", "study");
  await stage.getByTestId("try-read").click();
  // A4 is the second space: y = 62 − 3 × 5 = 47 of 84.
  await clickAt(stage.locator(".try-staff"), 0.6, 47 / 84);
  await stage.getByTestId("try-check").click();
  await expect(stage.locator(".try-staff .note.placed")).toHaveClass(/right/);
  await stage.getByTestId("try-next").click();
  await picturesNamed(stage.getByTestId("try-opt"), 3);
  await expect(value("b")).toHaveAccessibleName("staff: C E G C");
  await value("b").click();
  await expect(value("b")).toHaveClass(/right/);
  await stage.getByTestId("try-next").click();
  await expect(done).toContainText("Lesson passed: 2 of 2");

  await open("music", "play");
  await expect(stage.locator('[data-testid="try-match-meaning"] .try-staff')).toHaveCount(4);
  await picturesNamed(stage.getByTestId("try-match-meaning"), 4);
  await matchAll(4);

  await open("music", "guide");
  await expect(stage.locator(".try-keys .lit")).toHaveCount(2);
  await stage.getByTestId("try-next").click();
  await opt("Minor").click();
  await stage.getByTestId("try-flash").click();
  await expect(stage.getByTestId("try-flash")).toHaveText("C major: C E G");
  await stage.getByTestId("try-got-it").click();
  await expect(done).toContainText("★★★");
});

test("the try-it stage fits a phone in every subject and mode", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 740 });
  await page.goto("/");
  for (const subject of ["language", "maths", "music"]) {
    await page.getByTestId(`try-subject-${subject}`).click();
    for (const mode of ["sprint", "study", "play", "guide"]) {
      await page.getByTestId(`try-tab-${mode}`).click();
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow, `${subject} ${mode}: horizontal overflow in px`).toBeLessThanOrEqual(0);
    }
  }
});

test("the waitlist page has the same samples and use cases", async ({ page, browser }) => {
  test.skip(process.env.E2E_MINI === "1", "the waitlist is not in Skrongle mini");
  await devLogin(page, "next=/superuser");
  await setWaitlist(page, true);
  const visitor = await browser.newPage();
  await visitor.goto("/");
  await expect(visitor.getByTestId("waitlist-page")).toBeVisible();
  await playLanguage(visitor);
  await checkUses(visitor);
  await visitor.setViewportSize({ width: 375, height: 740 });
  for (const subject of ["maths", "music"]) {
    await visitor.getByTestId(`try-subject-${subject}`).click();
    for (const mode of ["sprint", "study", "play", "guide"]) {
      await visitor.getByTestId(`try-tab-${mode}`).click();
      const overflow = await visitor.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow, `waitlist ${subject} ${mode}: horizontal overflow in px`).toBeLessThanOrEqual(0);
    }
  }
  await visitor.close();
});
