// The superuser's course table (#1034): every made course on /superuser,
// searchable and filtered, flagged ones first. A row's title opens its
// detail dialog (what matched, in which lesson) with the actions: override
// the flag with a note and clear the override, take down and put back,
// delete the course, suspend the owner. The dev user is a superuser.
// Paging and the 404s for anyone else are checked over HTTP in
// course_flags::tests.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";
import { login, newAccount, signUp, type Account } from "../lib/learner";

test.skip(process.env.E2E_MINI === "1", "moderation is hosted only; Skrongle mini leaves it out");

/** A new account with one course; with `word` in a lesson's summary. Returns its id. */
async function owner(page: Page, a: Account, word = "", more = 0): Promise<string> {
  await signUp(page, a);
  await login(page, a);
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  if (word) {
    const listing = (await (await page.request.get(`/api/courses/${id}/files`)).json()) as { files: { path: string }[] };
    const path = listing.files.map((f) => f.path).find((p) => p.startsWith("lessons/"))!;
    const lesson = JSON.parse(((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()) as { content: string }).content);
    lesson.title = "Insults";
    lesson.summary = `Never say ${word}.`;
    lesson.vocabulary.push(...Array.from({ length: more }, (_, i) => ({ term: `${word} ${i + 1}`, meaning: `insult ${i + 1}` })));
    const r = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path, content: JSON.stringify(lesson, null, 2) }] } });
    expect(r.ok()).toBe(true);
  }
  return id;
}

/** /superuser with the course table searched for `q`. */
async function search(page: Page, q: string, show = "all") {
  await page.goto("/superuser");
  await page.getByTestId("superuser-courses-q").fill(q);
  await page.getByTestId("superuser-courses-show").selectOption(show);
  await page.getByTestId("superuser-courses-find").click();
}

function row(page: Page, id: string) {
  return page.locator(`[data-testid="superuser-course-row"][data-course-id="${id}"]`);
}

test("a flagged course: search, the reason, override with a note, clear the override", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("coursetable");
  const bad = await owner(them, a, "wetback");
  const good = await createCourse(them);
  await them.getByTestId("generate-close").click();
  await them.goto("/courses"); // the check runs when the course list loads it

  await devLogin(page);
  await search(page, a.email.toLowerCase());
  const rows = page.getByTestId("superuser-course-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toHaveAttribute("data-course-id", bad); // flagged first
  await expect(row(page, bad)).toHaveAttribute("data-state", "flagged");
  await expect(row(page, good)).toHaveAttribute("data-state", "clear");
  // The term list version each was checked under (#1039): the current one, not marked.
  const terms = row(page, good).getByTestId("superuser-course-terms-version");
  await expect(terms).toHaveText(/^Terms v\d+$/);
  await expect(terms).toHaveAttribute("data-behind", "false");

  await search(page, good);
  await expect(rows).toHaveCount(1);
  await search(page, a.email.toLowerCase(), "flagged");
  await expect(rows).toHaveCount(1);
  // "Behind" (#1040): courses whose AI check is old enough for a nightly recheck, with how many wait. These are new.
  await expect(page.getByTestId("superuser-courses-show").locator('option[value="behind"]')).toHaveText(/^AI check \d+ or more versions behind \(\d+\)$/);
  await search(page, a.email.toLowerCase(), "behind");
  await expect(page.getByTestId("superuser-courses-empty")).toBeVisible();
  await search(page, a.email.toLowerCase(), "flagged");

  await row(page, bad).getByTestId("superuser-course-open").click();
  const detail = page.locator(`[data-testid="superuser-course-dialog"][data-course-id="${bad}"]`);
  await expect(detail).toBeVisible();
  // The word and the text around it are masked; holding the word down shows it.
  const match = detail.getByTestId("superuser-course-match");
  await expect(match).toContainText("w****** · Insults");
  await expect(detail).not.toContainText("wetback");
  const [word, quote] = [match.getByTestId("masked-word").first(), match.getByTestId("masked-word").last()];
  await expect(word).toHaveAttribute("data-tip", "Click to reveal word");
  await expect(quote).toHaveAttribute("data-tip", "Click to reveal text");
  await word.hover();
  await page.mouse.down();
  await expect(word).toHaveText("wetback");
  await page.mouse.up();
  await expect(word).toHaveText("w******");

  await detail.getByTestId("superuser-course-override").click();
  const confirm = page.getByTestId("superuser-override-dialog");
  await expect(confirm).toContainText(bad);
  await confirm.getByTestId("superuser-override-note").fill("A quote in a lesson about slurs; fine");
  await confirm.getByTestId("superuser-override-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`The flag on ${bad} is overridden.`);
  await expect(row(page, bad)).toHaveAttribute("data-state", "overridden");
  await row(page, bad).getByTestId("superuser-course-open").click();
  await expect(detail.getByTestId("superuser-course-override-note")).toHaveText("A quote in a lesson about slurs; fine");
  await expect(detail.getByTestId("superuser-course-overridden-by")).toContainText("dev@example.com");

  // The owner can start it again.
  await them.goto("/courses");
  await expect(them.locator(`[data-testid="courses-card"][data-course-id="${bad}"]`).getByTestId("courses-open")).toBeVisible();

  await detail.getByTestId("superuser-course-clear-override").click();
  await page.getByTestId("superuser-clear-override-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`The override on ${bad} is cleared.`);
  await expect(row(page, bad)).toHaveAttribute("data-state", "flagged");
  await them.goto("/courses");
  await expect(them.locator(`[data-testid="courses-card"][data-course-id="${bad}"]`).getByTestId("courses-card-flagged")).toBeVisible();
  await them.context().close();
});

test("more than five matches show as a count that opens them all", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("coursetablemany");
  const bad = await owner(them, a, "wetback", 6);
  await them.goto("/courses");

  await devLogin(page);
  await search(page, bad);
  await row(page, bad).getByTestId("superuser-course-open").click();
  const detail = page.locator(`[data-testid="superuser-course-dialog"][data-course-id="${bad}"]`);
  await expect(detail.getByTestId("superuser-course-match").filter({ visible: true })).toHaveCount(0);
  await detail.getByTestId("flag-match-count").click();
  const list = detail.getByTestId("flag-match-list");
  await expect(list.getByTestId("superuser-course-match").filter({ visible: true })).toHaveCount(7);
  await expect(list.getByTestId("flag-match-page")).toHaveText("Page 1 of 1");
  await expect(list).not.toContainText("wetback");
});

test("take down and put back from the course's dialog", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("tabledown");
  const id = await owner(them, a);

  await devLogin(page);
  await search(page, id);
  await row(page, id).getByTestId("superuser-course-open").click();
  await page.getByTestId("superuser-course-take-down").click();
  await page.getByTestId("superuser-take-down-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`Course ${id} is taken down.`);
  await search(page, a.email.toLowerCase(), "down");
  await expect(row(page, id)).toHaveAttribute("data-state", "down");
  await them.goto("/courses");
  await expect(them.locator(`[data-testid="courses-card"][data-course-id="${id}"]`).getByTestId("courses-card-removed")).toBeVisible();

  await row(page, id).getByTestId("superuser-course-open").click();
  await page.getByTestId("superuser-course-put-back").click();
  await page.getByTestId("superuser-put-back-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`Course ${id} is back up.`);
  await them.goto("/courses");
  await expect(them.locator(`[data-testid="courses-card"][data-course-id="${id}"]`).getByTestId("courses-open")).toBeVisible();
  await them.context().close();
});

test("delete a course after a warning: gone from the table and the owner's list", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const id = await owner(them, newAccount("tabledelete"));

  await devLogin(page);
  await search(page, id);
  await row(page, id).getByTestId("superuser-course-open").click();
  await page.getByTestId("superuser-course-delete").click();
  const confirm = page.getByTestId("superuser-course-delete-dialog");
  await expect(confirm.getByTestId("superuser-course-delete-warning")).toContainText("can't be undone");
  await confirm.getByTestId("superuser-course-delete-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`Course ${id} is deleted.`);
  await expect(row(page, id)).toHaveCount(0);
  await them.goto("/courses");
  await expect(them.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveCount(0);
  await them.context().close();
});

test("suspend a course's owner from its dialog: their session ends", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("tablesuspend");
  const id = await owner(them, a);

  await devLogin(page);
  await search(page, id);
  await row(page, id).getByTestId("superuser-course-open").click();
  await page.getByTestId("superuser-course-suspend").click();
  const confirm = page.getByTestId("superuser-suspend-dialog");
  await expect(confirm).toContainText(a.email.toLowerCase());
  await confirm.getByTestId("superuser-suspend-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`${a.email.toLowerCase()} is suspended.`);
  await them.goto("/profile");
  await expect(them).toHaveURL(/\/login/);
  await them.context().close();
});
