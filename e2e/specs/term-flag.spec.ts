// The listed-term check (#1033): a made course with a word from
// data/moderation/terms.txt is flagged. Its owner's card has a red ! that
// names the lesson and the word, masked ("w******": holding it down shows
// it), Start and Share are refused by the server, and taking the word out
// brings everything back. More than five matches show as a count that opens
// the whole list, ten to a page. "negro" ("black" in
// Spanish) is not on the list. Sharees losing the course is checked over
// HTTP in course_flags::tests.
import { expect, test, type Page } from "@playwright/test";
import { createCourse } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";

test.skip(process.env.E2E_MINI === "1", "the content check is hosted only; Skrongle mini leaves it out");

async function save(page: Page, id: string, path: string, content: string) {
  const r = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path, content }] } });
  expect(r.ok()).toBe(true);
  expect((await r.json()).saved).toEqual([path]);
}

test("a course with a listed word can't start or be shared until the word is gone", async ({ page }) => {
  const a = newAccount("termflag");
  await signUp(page, a);
  await login(page, a);
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();

  const listing = (await (await page.request.get(`/api/courses/${id}/files`)).json()) as { files: { path: string }[] };
  const path = listing.files.map((f) => f.path).find((p) => p.startsWith("lessons/"))!;
  const original = ((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()) as { content: string }).content;
  const lesson = JSON.parse(original);
  lesson.title = "Insults";
  lesson.summary = "Never say wetback.";
  await save(page, id, path, JSON.stringify(lesson, null, 2));

  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-open")).toHaveCount(0);
  await card.getByTestId("courses-card-flagged").click();
  const dialog = card.getByTestId("courses-card-flag-dialog");
  await expect(dialog).toContainText("can't be started or shared yet");
  const match = dialog.getByTestId("courses-card-flag-match");
  await expect(match).toHaveText("w****** · Insults (/summary)");
  await expect(dialog).not.toContainText("wetback");
  const word = match.getByTestId("masked-word");
  await expect(word).toHaveAttribute("data-tip", "Click to reveal word");
  await expect(word).not.toHaveAttribute("title", /./);
  // Held down it shows the word; let go, it hides again. The same with Space.
  await word.hover();
  await page.mouse.down();
  await expect(word).toHaveText("wetback");
  await page.mouse.up();
  await expect(word).toHaveText("w******");
  await word.focus();
  await page.keyboard.down(" ");
  await expect(word).toHaveText("wetback");
  await page.keyboard.up(" ");
  await expect(word).toHaveText("w******");

  // Start and Share are refused on the server, not only hidden.
  const start = await page.request.post(`/courses/open/${id}`, { maxRedirects: 0 });
  expect(start.headers()["location"]).toContain(`/courses/${id}?notice=flagged`);
  const share = await page.request.post(`/courses/${id}/share`, { form: { emails: "friend@example.org" }, maxRedirects: 0 });
  expect(share.headers()["location"]).toContain("notice=share-flagged");

  // The dialog's button leads to the editor, which says what to fix.
  await dialog.getByTestId("courses-card-flag-edit").click();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}$`));
  await page.goto(`/courses/${id}?notice=flagged`);
  await expect(page.locator("body")).toContainText("w****** (in Insults)");

  // Fixed: the red ! goes and the course starts. "negro" is not on the list.
  lesson.summary = "El gato es negro.";
  await save(page, id, path, JSON.stringify(lesson, null, 2));
  await page.goto("/courses");
  await expect(card.getByTestId("courses-card-flagged")).toHaveCount(0);
  await expect(card.getByTestId("courses-open")).toBeVisible();
});

test("more than five matches show as a count that opens the list, ten to a page", async ({ page }) => {
  const a = newAccount("termflagmany");
  await signUp(page, a);
  await login(page, a);
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();

  const listing = (await (await page.request.get(`/api/courses/${id}/files`)).json()) as { files: { path: string }[] };
  const path = listing.files.map((f) => f.path).find((p) => p.startsWith("lessons/"))!;
  const lesson = JSON.parse(((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()) as { content: string }).content);
  lesson.title = "Insults";
  lesson.vocabulary.push(...Array.from({ length: 12 }, (_, i) => ({ term: `wetback ${i + 1}`, meaning: `insult ${i + 1}` })));
  await save(page, id, path, JSON.stringify(lesson, null, 2));

  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await card.getByTestId("courses-card-flagged").click();
  const dialog = card.getByTestId("courses-card-flag-dialog");
  // Only the count is in the dialog; the list opens from it.
  await expect(dialog.getByTestId("courses-card-flag-match").filter({ visible: true })).toHaveCount(0);
  await expect(dialog.getByTestId("flag-match-count")).toHaveText("12 matches");
  await dialog.getByTestId("flag-match-count").click();
  const list = dialog.getByTestId("flag-match-list");
  await expect(list.getByTestId("courses-card-flag-match").filter({ visible: true })).toHaveCount(10);
  await expect(list.getByTestId("flag-match-page")).toHaveText("Page 1 of 2");
  await expect(list.getByTestId("courses-card-flag-match").first()).toHaveText("w****** · Insults (/vocabulary/3/term)");
  await list.getByTestId("flag-match-next").click();
  await expect(list.getByTestId("courses-card-flag-match").filter({ visible: true })).toHaveCount(2);
  await expect(list.getByTestId("flag-match-page")).toHaveText("Page 2 of 2");
  await expect(list.getByTestId("flag-match-next")).toBeDisabled();
  await list.getByTestId("flag-match-prev").click();
  await expect(list.getByTestId("flag-match-page")).toHaveText("Page 1 of 2");
});
