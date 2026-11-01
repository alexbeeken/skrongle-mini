// Closing things by clicking outside them (#233, #226).
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, generationEnds } from "../lib/app";

// The dev user is shared with the other specs: leave it as it was found.
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a Sprint mode stop's sheet closes on a tap outside it and on Escape (#233)", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await devLogin(page);
  await page.goto("/sprint");
  const stop = page.getByTestId("sprint-lesson").first();
  const sheet = stop.getByTestId("sprint-sheet");
  const toggle = stop.getByTestId("sprint-lesson-toggle");

  await toggle.click();
  await expect(sheet).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  // A tap inside the sheet keeps it.
  await sheet.locator(".sprint-sheet-title").click();
  await expect(sheet).toBeVisible();
  // A tap on the unit's heading, outside the stop, puts it away.
  await page.getByTestId("sprint-unit").first().locator(".sprint-level-head").click();
  await expect(sheet).toBeHidden();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");

  // Escape too, and focus goes back to the stop.
  await toggle.click();
  await expect(sheet).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await expect(toggle).toBeFocused();

  // Tapping another stop still swaps one sheet for the other.
  await toggle.click();
  const second = page.getByTestId("sprint-lesson").nth(1);
  await second.getByTestId("sprint-lesson-toggle").click();
  await expect(sheet).toBeHidden();
  await expect(second.getByTestId("sprint-sheet")).toBeVisible();
});

// #226: every closable modal closes on a click on its backdrop (static/modal.js).
// Outside = the top-left corner of the window, which no modal covers.
async function clickOutside(page: Page): Promise<void> {
  await page.mouse.click(4, 4);
}

test("profile dialogs close on a click outside; one with something typed in its form stays (#226)", async ({ page }) => {
  await devLogin(page, "open=spanish&next=/profile");
  const opens: [string, string][] = [
    ["profile-privacy-info", "profile-privacy-note"],
    ["profile-password-open", "profile-password-dialog"],
    ["profile-sign-out-others-info", "profile-sign-out-others-info-dialog"],
  ];
  for (const [opener, testid] of opens) {
    const dialog = page.getByTestId(testid);
    await page.getByTestId(opener).click();
    await expect(dialog).toBeVisible();
    // A click inside keeps it.
    await dialog.locator("h2").click();
    await expect(dialog).toBeVisible();
    await clickOutside(page);
    await expect(dialog).toBeHidden();
  }

  // A drag that starts inside and ends outside (selecting text) is not a click outside.
  const privacy = page.getByTestId("profile-privacy-note");
  await page.getByTestId("profile-privacy-info").click();
  const box = (await privacy.locator("h2").boundingBox())!;
  await page.mouse.move(box.x + 4, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(4, 4, { steps: 4 });
  await page.mouse.up();
  await expect(privacy).toBeVisible();
  await privacy.getByTestId("profile-privacy-close").click();
  await expect(privacy).toBeHidden();

  // Opted out: a password typed and not sent keeps the dialog open; Cancel still closes it.
  const password = page.getByTestId("profile-password-dialog");
  await page.getByTestId("profile-password-open").click();
  await page.getByTestId("profile-password-current").fill("half-typed");
  await clickOutside(page);
  await expect(password).toBeVisible();
  await page.getByTestId("profile-password-cancel").click();
  await expect(password).toBeHidden();
});

test("the Generate dialog closes on a click outside until a wish is typed; a running job's box stays (#226)", async ({ page }) => {
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  const id = await createCourse(page);
  const dialog = page.getByTestId("generate");
  await clickOutside(page);
  await expect(dialog).toBeHidden();
  // Closed: the page forgets ?generate=, as with the close button.
  await expect(page).not.toHaveURL(/[?&]generate=/);

  await page.getByTestId("course-ai-generate").click();
  await expect(dialog).toBeVisible();
  await page.getByTestId("generate-wish").fill("Fractions to keep [fixture:slow]");
  await clickOutside(page);
  await expect(dialog).toBeVisible();

  // The job's progress box has no way out but Cancel: a click outside does nothing.
  await page.getByTestId("generate-submit").click();
  const job = page.getByTestId("course-ai-modal");
  await expect(job).toHaveAttribute("data-state", "running");
  await expect(page.getByTestId("course-ai-modal-backdrop")).toHaveAttribute("data-modal-static", "");
  const url = page.url();
  await clickOutside(page);
  await page.waitForTimeout(300);
  expect(page.url()).toBe(url);
  await expect(job).toBeVisible();
  expect(await generationEnds(page)).toBe("done");
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-course-id", id);

  // Review AI changes, drawn by the server: a click outside follows its Close link.
  await page.getByTestId("course-ai-text").fill("comparing two fractions");
  await page.getByTestId("course-ai-submit").click();
  const review = page.getByTestId("course-review-modal");
  await expect(review).toHaveAttribute("data-after-job", "true", { timeout: 30_000 });
  await clickOutside(page);
  await expect(review).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/courses/${id}#lessons$`));
});

test("the /dev/jobs (i) opens its dialog; OK and a click outside close it (#872)", async ({ page }) => {
  await page.goto("/dev/jobs");
  const dialog = page.getByTestId("dev-jobs-info-dialog");
  await page.getByTestId("dev-jobs-info").click();
  await expect(dialog).toBeVisible();
  await dialog.locator("[data-closes]").click();
  await expect(dialog).toBeHidden();
  await page.getByTestId("dev-jobs-info").click();
  await expect(dialog).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(dialog).toBeHidden();
});

test("/dev/jobs fits a desktop window: Run now is in view, in German too (#1110)", async ({ browser }) => {
  // German has the longest labels; a failed job fills the Last error column.
  const ctx = await browser.newContext({ locale: "de-DE", viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  await page.goto("/dev/jobs");
  await page.getByTestId("dev-jobs-queue-fail").click();
  // The runner picks the job up within a few seconds; the page shows its error after a reload.
  await expect(async () => {
    await page.goto("/dev/jobs");
    await expect(page.getByTestId("dev-jobs-error").filter({ hasText: "dev.fail" }).first()).toBeVisible({ timeout: 500 });
  }).toPass({ timeout: 15_000 });
  const frame = (await page.locator(".devjobs .devbox-frame").boundingBox())!;
  const runNow = (await page.getByTestId("dev-jobs-run-now").first().boundingBox())!;
  expect(runNow.x + runNow.width).toBeLessThanOrEqual(frame.x + frame.width);
  await ctx.close();
});
