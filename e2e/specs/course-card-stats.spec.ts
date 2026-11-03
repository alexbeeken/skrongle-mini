// The progress chips on a course card and the modal each opens (#684).
import { expect, test } from "@playwright/test";
import { devLogin, devReset } from "../lib/app";

const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// The dev user is shared with the other specs: leave it as it was found.
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a course card shows passed and mastered as a bar and chips; a chip opens its stat in a modal and its name fits a phone (#684)", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await devReset(page);
  await devLogin(page, "open=spanish");
  // Two lessons passed, as the dev seed does.
  const before = await (await page.request.get("/api/progress")).json();
  for (const l of before.lessons.slice(0, 2)) {
    const r = await page.request.post("/api/progress/pass", { data: { lesson: l.id, answered: 20, correct: 20, evidence: "course-card-stats spec" } });
    expect(r.ok()).toBe(true);
  }
  const overview = await (await page.request.get("/api/progress")).json();
  expect(overview.passed).toBe(2);
  const total = overview.total as number;

  await page.goto("/courses");
  const card = page.locator('[data-testid="courses-card"][data-course-id="spanish"]');
  const passed = card.locator('[data-testid="courses-card-stat"][data-metric="passed"]');
  const mastered = card.locator('[data-testid="courses-card-stat"][data-metric="mastered"]');
  await expect(passed).toHaveText(`2/${total}`);
  await expect(passed).toHaveAttribute("data-tip", `Lessons passed: 2 of ${total}`);
  await expect(mastered).toHaveText(`0/${total}`);
  await expect(mastered).toHaveAttribute("data-tip", `Lessons mastered: 0 of ${total}`);

  // The bar under the title (#901): passed and mastered as a share of the course.
  const bar = card.getByTestId("courses-card-bar");
  await expect(bar.locator('[data-metric="passed"]')).toHaveAttribute("style", `width: ${Math.floor(200 / total)}%`);
  await expect(bar.locator('[data-metric="mastered"]')).toHaveAttribute("style", "width: 0%");
  // When it was last opened sits in the button row, its full wording on hover.
  await expect(card.getByTestId("courses-card-opened")).toHaveAttribute("title", /^opened /);

  // Hovered, the name shows and the page still fits a 390 px phone.
  await passed.hover();
  const tip = await passed.evaluate((el) => getComputedStyle(el, "::after").display);
  expect(tip).toBe("block");
  const fits = await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
  expect(fits, "no horizontal scroll with a chip hovered").toBe(true);

  // A tap opens the stat in full: its name and number.
  await passed.click();
  const dialog = card.locator('[data-testid="courses-stat-dialog"][data-metric="passed"]');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("courses-stat-name")).toHaveText("Lessons passed");
  await expect(dialog.getByTestId("courses-stat-value")).toHaveText(`2 of ${total}`);
  await dialog.getByTestId("courses-stat-close").click();
  await expect(dialog).toBeHidden();

  await mastered.click();
  const m = card.locator('[data-testid="courses-stat-dialog"][data-metric="mastered"]');
  await expect(m.getByTestId("courses-stat-name")).toHaveText("Lessons mastered");
  await expect(m.getByTestId("courses-stat-value")).toHaveText(`0 of ${total}`);
});

test("a chip's modal fetches what is next as it opens: the next lesson to pass and each unit, the lessons to practise, and opens the course at one (#900)", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await devReset(page);
  await devLogin(page, "open=spanish");
  const before = await (await page.request.get("/api/progress")).json();
  const [first, second, third] = before.lessons;
  for (const l of [first, second]) {
    const r = await page.request.post("/api/progress/pass", { data: { lesson: l.id, answered: 20, correct: 20, evidence: "course-card-stats spec" } });
    expect(r.ok()).toBe(true);
  }
  // One right answer in the third lesson: practised, 20 %, not mastered.
  const a = await page.request.post("/api/progress/answers", { data: { lesson: third.id, prompt: "course-card-stats spec", answer: "x", correct: true } });
  expect(a.ok()).toBe(true);
  expect((await a.json()).mastery).toBe(20);

  await page.goto("/courses");
  const card = page.locator('[data-testid="courses-card"][data-course-id="spanish"]');
  // Nothing is fetched until a modal opens.
  await expect(card.getByTestId("courses-stat-detail").first()).toBeEmpty();

  const fetched = page.waitForResponse((r) => r.url().endsWith("/courses/spanish/stat/passed") && r.ok());
  await card.locator('[data-testid="courses-card-stat"][data-metric="passed"]').click();
  await fetched;
  const passed = card.locator('[data-testid="courses-stat-dialog"][data-metric="passed"]');
  await expect(passed.getByTestId("courses-stat-lesson")).toHaveText(third.title);
  await expect(passed.getByTestId("courses-stat-lesson")).toHaveAttribute("data-slug", third.slug);
  await expect(passed.getByTestId("courses-stat-last")).toHaveText(new RegExp(`^Last passed: (${esc(first.title)}|${esc(second.title)}), `));
  await expect(passed.getByTestId("courses-stat-unit").first()).toContainText("2/");
  const fits = await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
  expect(fits, "no horizontal scroll with the modal open").toBe(true);

  // What counts as passed is behind the (i).
  await passed.getByTestId("courses-stat-info").click();
  const info = page.locator('[data-testid="courses-stat-info-dialog"][data-metric="passed"]');
  await expect(info).toBeVisible();
  await expect(info).toContainText("70% or more right");
  await info.getByRole("button").click();
  await expect(info).toBeHidden();
  await passed.getByTestId("courses-stat-close").click();

  await card.locator('[data-testid="courses-card-stat"][data-metric="mastered"]').click();
  const mastered = card.locator('[data-testid="courses-stat-dialog"][data-metric="mastered"]');
  await expect(mastered.getByTestId("courses-stat-lesson")).toHaveText(third.title);
  await expect(mastered.getByTestId("courses-stat-score")).toHaveText("20%");

  // A lesson in the list opens the course at that lesson.
  await mastered.getByTestId("courses-stat-lesson").click();
  await expect(page).toHaveURL(new RegExp(`/lessons/${third.slug}$`));
});

test("with the app in French, when a course was last opened fits its card at 1280 and 390 px, without an ellipsis (#1182)", async ({ page, context }) => {
  await devReset(page);
  await devLogin(page, "open=spanish");
  // The cookie, not POST /lang: that stores the language on the shared dev user, and it would beat the cookie other specs set.
  await context.addCookies([{ name: "lang", value: "fr", url: page.url() }]);
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/courses");
    const when = page.locator('[data-testid="courses-card"][data-course-id="spanish"]').getByTestId("courses-card-opened");
    await expect(when).toHaveAttribute("title", /^ouvert /);
    // The label as rendered, then the widest values French gives it (the QA sweep saw "il y a 13 minut…").
    for (const text of [null, "il y a 59 minutes", "il y a 23 heures", "il y a 29 jours"]) {
      const fit = await when.evaluate((el, t) => {
        if (t) el.textContent = t;
        const row = el.parentElement!;
        return { scroll: el.scrollWidth, client: el.clientWidth, row: row.scrollWidth <= row.clientWidth, text: el.textContent };
      }, text);
      expect(fit.scroll, `"${fit.text}" at ${width} px is not cut`).toBeLessThanOrEqual(fit.client);
      expect(fit.row, `the button row holds "${fit.text}" at ${width} px`).toBe(true);
    }
  }
});
