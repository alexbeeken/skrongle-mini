// Exact spec for "Paste a word list" in the course editor (#453): pasted
// lines become a new lesson's vocabulary with no model involved. The dialog
// previews the pairs under the course's labels and flags the lines it can't
// read; Add puts the lesson in the table, which saves it; the lesson opens
// with the words. Language courses only: the button follows the gear's
// Language features switch. Fixtures only (no key).
import { expect, test } from "@playwright/test";
import { copyCourse, devLogin, devReset, editorSaved, fastAutosave, openFromEditor, TEST_COURSE } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
  await fastAutosave(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a pasted word list previews, flags what it can't read, and becomes a lesson that opens with the words", async ({ page }) => {
  const id = await copyCourse(page, TEST_COURSE);
  const before = await page.getByTestId("course-lessons-row").count();

  await page.getByTestId("course-words-open").click();
  const dlg = page.getByTestId("course-words-dialog");
  await expect(dlg).toBeVisible();
  // The preview's columns carry the course's own labels (test course: Español / English).
  await expect(dlg.getByTestId("course-words-preview").locator("th")).toHaveText(["Español", "English"]);
  await expect(dlg.getByTestId("course-words-add")).toBeDisabled();

  await dlg.getByTestId("course-words-name").fill("Pasted words");
  await dlg.getByTestId("course-words-text").fill(["Español\tEnglish", "el gato — cat", "el perro, dog", "la casa\thouse", "just one word", "el gato — the cat"].join("\n"));
  const rows = dlg.getByTestId("course-words-row");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0).locator("td")).toHaveText(["el gato", "cat"]);
  await expect(rows.nth(1).locator("td")).toHaveText(["el perro", "dog"]);
  await expect(rows.nth(2).locator("td")).toHaveText(["la casa", "house"]);
  await expect(dlg.getByTestId("course-words-count")).toHaveText("3 words read");
  const skipped = dlg.getByTestId("course-words-skip");
  await expect(skipped).toHaveCount(2);
  await expect(skipped.nth(0)).toContainText("Line 5");
  await expect(skipped.nth(0)).toContainText("just one word");
  await expect(skipped.nth(1)).toContainText("Line 6");
  await expect(skipped.nth(1)).toContainText("already in the list");

  await dlg.getByTestId("course-words-add").click();
  await expect(dlg).toBeHidden();
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(before + 1);
  const row = page.getByTestId("course-lessons-row").last();
  await expect(row.locator(".lt-title")).toHaveText("Pasted words");
  await editorSaved(page);
  await expect(row).toHaveAttribute("data-lesson-id", "pasted-words");

  // Stored as a lesson file with the words as its vocabulary.
  const path = (await row.getAttribute("data-path"))!;
  const file = await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json();
  const lesson = JSON.parse(file.content);
  expect(lesson.vocabulary).toEqual([
    { term: "el gato", meaning: "cat" },
    { term: "el perro", meaning: "dog" },
    { term: "la casa", meaning: "house" },
  ]);

  // The lesson opens for learners with the words in its vocabulary table.
  const tab = await openFromEditor(page);
  await tab.goto("/lessons/pasted-words");
  const vocab = tab.getByTestId("lesson-vocab-item");
  await expect(vocab).toHaveCount(3);
  await expect(vocab.nth(0)).toContainText("el gato");
  await expect(vocab.nth(0)).toContainText("cat");
  await tab.close();
});

test("a paste past the cap is refused in words, not with a body-limit error", async ({ page }) => {
  // 6,000 lines with an em dash: about 60,000 characters (the box allows 65,536)
  // but over 64 KB once encoded.
  const id = await copyCourse(page, TEST_COURSE);
  await page.getByTestId("course-words-open").click();
  const dlg = page.getByTestId("course-words-dialog");
  await dlg.getByTestId("course-words-text").evaluate((el, v) => {
    (el as HTMLTextAreaElement).value = v;
    el.dispatchEvent(new Event("input"));
  }, "abc — def\n".repeat(6000));
  await expect(dlg.getByTestId("course-words-count")).toHaveText("Paste at most 64 KB of words at a time.");
  await expect(dlg.getByTestId("course-words-add")).toBeDisabled();
  const r = await page.request.post(`/api/courses/${id}/words`, { data: { text: "abc — def\n".repeat(6000) } });
  expect(r.status()).toBe(400);
});

test("the button follows the Language features switch", async ({ page }) => {
  // The switch is in the gear with JSON off (Forms mode, remembered in the browser).
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const open = page.getByTestId("course-words-open");
  await expect(open).toBeVisible();
  await page.getByTestId("course-lessons-gear").click();
  const sw = page.getByTestId("course-lessons-language-switch");
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect(open).toBeHidden();
  await page.getByTestId("course-lessons-gear").click();
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect(open).toBeVisible();
});
