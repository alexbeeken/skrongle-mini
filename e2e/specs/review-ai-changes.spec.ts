// Exact specs for "Review AI changes" on the course page (#163, #193, #189,
// #187): a modal listing the lessons the latest AI run touched like the
// course page's lesson table, each with what the AI did in words, opening to
// a side-by-side diff (removed lines red on the left, the AI's lines green on
// the right; a lesson the AI added as a whole is one column). A "−" leaves a
// line (drag: several) or a whole lesson out, and Accept writes only the rest.
// "Don't show this automatically next time" keeps it from opening by itself.
// Fixtures only.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, editorSaved, generateCourse, generationEnds, aiMode } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const jobModal = (page: Page) => page.getByTestId("course-ai-modal");
const review = (page: Page) => page.getByTestId("course-review-modal");
const reviewFile = (page: Page, kind: string) => review(page).locator(`[data-testid="course-review-file"][data-kind="${kind}"]`);
const fileTitled = (page: Page, title: string) =>
  review(page).getByTestId("course-review-file").filter({ has: page.getByTestId("course-review-file-title").getByText(title, { exact: true }) });

/** Open a lesson in the review (its ▸). */
async function expand(file: Locator) {
  const details = file.getByTestId("course-review-details");
  if ((await details.getAttribute("open")) === null) await file.getByTestId("course-review-file-link").locator(".rv-title").click();
  await expect(details).toHaveAttribute("open", "");
}

/** Generate the fixture course (3 lessons). Returns its id. */
async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  // A first generation opens no review: every file is new, the editor is the result.
  await expect(review(page)).toHaveCount(0);
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

/** Run Use AI from the course page and wait for the job to end (`opens`: the review comes up by itself). */
async function useAi(page: Page, id: string, mode: "add" | "change", text: string, opens = true) {
  await page.goto(`/courses/${id}`);
  if (mode === "change") await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill(text);
  await page.getByTestId("course-ai-submit").click();
  if (opens) {
    await expect(review(page)).toHaveAttribute("data-after-job", "true", { timeout: 30_000 });
  } else {
    await expect(page.getByTestId("course-edit-notice")).toContainText("The AI has finished", { timeout: 30_000 });
    await expect(review(page)).toHaveCount(0);
  }
  await expect(jobModal(page)).toHaveCount(0);
}

async function openReview(page: Page, id: string) {
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-ai-review").click();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?review=1`));
  await expect(review(page)).toBeVisible();
}

async function fileContent(page: Page, id: string, path: string): Promise<string | null> {
  const r = await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`);
  return r.ok() ? ((await r.json()).content as string) : null;
}

test("a generated course reviews as new lessons, listed like the lesson table", async ({ page }) => {
  const id = await makeCourse(page, "Fractions to review, step by step");
  await openReview(page, id);
  await expect(page.getByTestId("course-review-label")).toContainText("Generated with AI: “Fractions to review, step by step”");
  await expect(page.getByTestId("course-review-overview")).toContainText("The AI added 3 lessons and changed the course settings.");
  const files = review(page).getByTestId("course-review-file");
  // course.json and three lessons, every one new: no split view, no red or green.
  await expect(files).toHaveCount(4);
  await expect(reviewFile(page, "added")).toHaveCount(4);
  await expect(review(page).getByTestId("course-review-whole")).toHaveCount(4);
  await expect(review(page).getByTestId("course-review-split")).toHaveCount(0);
  await expect(review(page).getByTestId("course-review-add")).toHaveCount(0);
  await expect(review(page).getByTestId("course-review-del")).toHaveCount(0);
  // Course settings first, then the lessons numbered as the course page numbers them.
  await expect(files.first().getByTestId("course-review-file-title")).toHaveText("Course settings");
  await expect(review(page).getByTestId("course-review-position")).toHaveText(["⚙", "1", "2", "3"]);
  // Lessons by the titles the course page shows, never by file name; a light tag says new, changed or removed.
  await expect(review(page).getByTestId("course-review-list")).not.toContainText(".json");
  const tag = files.nth(1).getByTestId("course-review-tag");
  await expect(tag).toHaveText("New");
  expect(await tag.evaluate((el) => getComputedStyle(el).backgroundColor)).not.toBe("rgba(0, 0, 0, 0)");
  // In words what the AI wrote, and the lesson folded until it is opened.
  await expect(files.nth(1).getByTestId("course-review-summary")).toContainText(/^New, with a reading, \d+ quiz questions?/);
  await expect(files.nth(1).getByTestId("course-review-whole")).toBeHidden();
  await expand(files.nth(1));
  // JSON reads as labelled text, not code: no quotes or braces around the title.
  const body = files.nth(1).getByTestId("course-review-whole");
  await expect(body).toBeVisible();
  await expect(body.locator(".rv-k").getByText("Title", { exact: true })).toBeVisible();
  expect(await body.innerText()).not.toContain('"title":');
  // Leaving out the line that opens the quiz leaves out the whole quiz with it; its − brings it all back.
  const quiz = body.getByTestId("course-review-row").filter({ has: page.locator(".rv-k", { hasText: /^Quiz$/ }) });
  await quiz.getByTestId("course-review-exclude-line").check();
  expect(await body.locator("input[name=lines]:checked").count()).toBeGreaterThan(5);
  await expect(page.getByTestId("course-review-left-out")).toHaveText("1 change left out");
  await quiz.getByTestId("course-review-exclude-line").uncheck();
  await expect(body.locator("input[name=lines]:checked")).toHaveCount(0);
  // "Show the files as JSON" shows the file as it is written.
  await page.getByTestId("course-review-raw").click();
  await expect(page.getByTestId("course-review-raw")).toHaveAttribute("aria-checked", "true");
  expect(await body.innerText()).toContain('"title":');

  // Escape closes it, back on the course page with nothing changed.
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(new RegExp(`/courses/${id}#ai$`));
  await expect(review(page)).toHaveCount(0);
  await expect(page.getByTestId("course-edit-notice")).toHaveCount(0);
});

test("Only add shows the new lesson as one column; what was there keeps no red", async ({ page }) => {
  const id = await makeCourse(page, "Fractions plus one, step by step");
  await useAi(page, id, "add", "comparing two fractions");
  // Opened by the job: Close goes on to the lesson table, where the new lesson is.
  await expect(page.getByTestId("course-review-label")).toContainText("Added with AI:");
  await page.getByTestId("course-review-close").click();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}#lessons$`));
  await expect(review(page)).toHaveCount(0);
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(4);
  // The button opens it again.
  await openReview(page, id);
  await expect(review(page)).toHaveAttribute("data-after-job", "false");
  await expect(page.getByTestId("course-review-label")).toContainText("Added with AI:");
  await expect(reviewFile(page, "added")).toHaveCount(1);
  await expect(reviewFile(page, "removed")).toHaveCount(0);
  await expect(reviewFile(page, "added").getByTestId("course-review-whole")).toHaveCount(1);
  // Adding never takes a line away.
  await expect(review(page).getByTestId("course-review-del")).toHaveCount(0);
});

test("Use AI runs on the course page: the modal comes up in place, holds the page, then the review opens", async ({ page }) => {
  const id = await makeCourse(page, "Fractions in place, step by step");
  await page.goto(`/courses/${id}`);
  // [fixture:slow]: the reply takes a few seconds.
  await page.getByTestId("course-ai-text").fill("A lesson on comparing fractions, slowly [fixture:slow]");
  await page.getByTestId("course-ai-submit").click();
  await expect(jobModal(page)).toHaveAttribute("data-state", "running");
  // No page load: the same workspace, inert under the modal.
  await expect(page).toHaveURL(new RegExp(`/courses/${id}$`));
  const locked = page.getByTestId("course-edit-locked");
  await expect(locked).toHaveJSProperty("inert", true);
  await expect(locked.getByTestId("course-lessons")).toBeVisible();
  await expect(locked.getByTestId("course-ai-text")).toHaveValue(/comparing fractions/);
  // Coming back while it runs: the server draws the same workspace, locked, with no busy banner or second Cancel.
  await page.reload();
  await expect(jobModal(page)).toHaveAttribute("data-state", "running");
  await expect(locked).toHaveJSProperty("inert", true);
  await expect(locked.getByTestId("course-lessons")).toBeVisible();
  await expect(locked.getByTestId("course-ai")).toBeVisible();
  for (const gone of ["course-edit-revising", "course-edit-cancel", "course-edit-files"]) {
    await expect(page.getByTestId(gone), gone).toHaveCount(0);
  }
  await expect(page.getByTestId("course-ai-modal-close")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(jobModal(page)).toBeVisible();
  // Done: the review of what it added, and the page is usable again.
  await expect(review(page)).toHaveAttribute("data-after-job", "true", { timeout: 30_000 });
  await expect(jobModal(page)).toHaveCount(0);
  await expect(page.getByTestId("course-review-label")).toContainText("Added with AI:");
  await page.getByTestId("course-review-close").click();
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(4);
});

test("a refused Use AI request comes back on the same page, text kept", async ({ page }) => {
  const id = await makeCourse(page, "Fractions refused in place");
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-ai-text").fill("ab");
  // The browser's own check would stop two letters (minlength is not set): the server says why.
  await page.getByTestId("course-ai-submit").click();
  await expect(page.getByTestId("course-ai-error")).toHaveText("Say what the new lesson should teach, in at least 20 characters.");
  await expect(page).toHaveURL(new RegExp(`/courses/${id}$`));
  await expect(page.getByTestId("course-ai-text")).toHaveValue("ab");
  // The swapped-in form still submits in place.
  await page.getByTestId("course-ai-text").fill("A lesson on comparing fractions");
  await page.getByTestId("course-ai-submit").click();
  await expect(review(page)).toHaveAttribute("data-after-job", "true", { timeout: 30_000 });
});

test("a revision shows red and green side by side, and later hand edits do not change it", async ({ page }) => {
  const id = await makeCourse(page, "Fractions revised for review");
  await useAi(page, id, "change", "More worked examples, and add word problems.");
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback: “More worked examples");
  const changed = fileTitled(page, "Simplifying");
  await expect(changed).toHaveAttribute("data-kind", "changed");
  // In words: the reading rewritten.
  await expect(changed.getByTestId("course-review-summary")).toContainText("ewrote the reading");
  await expand(changed);
  await expect(changed.getByTestId("course-review-split")).toBeVisible();
  await expect(changed.getByTestId("course-review-del").first()).toBeVisible();
  await expect(changed.getByTestId("course-review-add").first()).toBeVisible();
  // Red on the left, green on the right.
  const del = await changed.getByTestId("course-review-del").first().evaluate((el) => getComputedStyle(el).backgroundColor);
  const add = await changed.getByTestId("course-review-add").first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(del).not.toBe(add);
  const path = (await changed.getAttribute("data-path"))!;
  const before = await changed.innerText();

  // A hand edit of that file afterwards: the review still shows what the AI wrote.
  // (Not a rename: the lesson table's keys do not block the review, #280.)
  const edited = JSON.parse((await fileContent(page, id, path))!);
  edited.summary = "Renamed by hand";
  const saved = await page.request.post(`/courses/${id}/file`, { form: { path, content: JSON.stringify(edited, null, 2) } });
  expect(saved.ok()).toBeTruthy();
  await openReview(page, id);
  const again = review(page).locator(`[data-testid="course-review-file"][data-path="${path}"]`);
  await expand(again);
  expect(await again.innerText()).toBe(before);
  await expect(review(page)).not.toContainText("Renamed by hand");
  // Leaving out part of a lesson edited since is refused, naming it; nothing changes.
  await again.locator('[data-testid="course-review-row"]').first().getByTestId("course-review-exclude-line").check();
  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-review-error")).toContainText("has been changed since the AI run");
  expect(JSON.parse((await fileContent(page, id, path))!).summary).toBe("Renamed by hand");
});

test("leaving out a line and a lesson: Accept writes only the rest, and the review remembers", async ({ page }) => {
  const id = await makeCourse(page, "Fractions kept in part");
  // The lesson as it was, before the AI rewrites it.
  await page.goto(`/courses/${id}`);
  const rows = page.getByTestId("course-lessons-row");
  await expect(rows).toHaveCount(3);
  const simplifyPath = (await rows.filter({ hasText: "Simplifying" }).getAttribute("data-path"))!;
  const original = JSON.parse((await fileContent(page, id, simplifyPath))!);

  await useAi(page, id, "change", "More worked examples, and add word problems.");
  const changed = fileTitled(page, "Simplifying");
  const added = reviewFile(page, "added");
  await expect(added).toHaveCount(1);
  const addedPath = (await added.getAttribute("data-path"))!;
  const aiVersion = JSON.parse((await fileContent(page, id, simplifyPath))!);
  expect(aiVersion.content).not.toBe(original.content);
  await expect(page.getByTestId("course-review-accept")).toHaveText("Accept");

  // The reading's line: the red − leaves it out; the row turns red and the AI's text is struck.
  await expand(changed);
  const reading = changed.locator('[data-testid="course-review-row"]').filter({ has: page.locator(".rv-k", { hasText: /^Reading$/ }) }).first();
  const box = reading.getByTestId("course-review-exclude-line");
  await box.check();
  await expect(box).toBeChecked();
  expect(await reading.locator("[data-testid=course-review-add] > span").first().evaluate((el) => getComputedStyle(el).textDecorationLine)).toBe("line-through");
  // The left-out row is tinted red on both sides.
  const bg = (testid: string) => reading.getByTestId(testid).evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await bg("course-review-del")).toBe(await bg("course-review-add"));
  await expect(page.getByTestId("course-review-left-out")).toHaveText("1 change left out");
  await expect(page.getByTestId("course-review-accept")).toHaveText("Accept the rest");

  // In the new lesson, a drag from one red − over the next rows leaves them
  // out as one group with one yellow −, on its first row, that brings them
  // all back; the rows are tinted red.
  await expand(added);
  const lines = added.locator('[data-testid="course-review-row"]');
  expect(await lines.count()).toBeGreaterThan(6);
  // Every − is one size, the thin bracket-only rows' too (#290).
  expect(await added.locator("tr.rv-bare .rv-minus").count()).toBeGreaterThan(0);
  const sizes = await added.locator(".rv-minus").evaluateAll((els) =>
    els.map((el) => { const r = el.getBoundingClientRect(); return `${Math.round(r.width)}x${Math.round(r.height)}`; }));
  expect(new Set(sizes)).toEqual(new Set([sizes[0]]));
  // − signs showing on the rows (hover, a fading trail, a left-out group's head).
  const shownMinus = () => added.locator("tr.rv-row").evaluateAll((trs) =>
    trs.flatMap((tr, i) => {
      const m = tr.querySelector(".rv-minus");
      if (!m) return [];
      const cs = getComputedStyle(m);
      return cs.visibility !== "hidden" && parseFloat(cs.opacity) > 0 ? [i] : [];
    }));
  const drag = async (from: number, to: number, during?: () => Promise<void>) => {
    await lines.nth(Math.max(from, to)).scrollIntoViewIfNeeded();
    await lines.nth(Math.min(from, to)).scrollIntoViewIfNeeded();
    const a = (await lines.nth(from).getByTestId("course-review-exclude-line").boundingBox())!;
    const b = (await lines.nth(to).boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(a.x + a.width / 2, b.y + b.height / 2, { steps: 8 });
    if (during) await during();
    await page.mouse.up();
  };
  // While dragging only the band shows: no − on any row, none fading out behind the pointer (#290).
  await drag(1, 3, async () => {
    await expect(lines.nth(3)).toHaveClass(/rv-sel/);
    expect(await shownMinus()).toEqual([]);
  });
  const ticked = added.locator("input[name=lines]:checked");
  await expect(ticked).toHaveCount(3);
  const group = (await lines.nth(1).getByTestId("course-review-exclude-line").getAttribute("data-group"))!;
  expect(group).not.toBe("0");
  await expect(added.locator(`input[name=lines][data-group="${group}"]`)).toHaveCount(3);
  await expect(page.getByTestId("course-review-left-out")).toHaveText("2 changes left out");
  await expect(lines.nth(1)).toHaveClass(/rv-head/);
  await expect(lines.nth(2).getByTestId("course-review-exclude-line")).toBeHidden();
  await expect(lines.nth(3)).toHaveClass(/rv-tail/);
  // Then the one − on the group's first row (and the hovered row's, if a free one).
  await page.mouse.move(0, 0);
  await expect.poll(shownMinus).toEqual([1]);
  // The yellow − takes the group back with no red − fading on its other rows
  // (#290): read in the same tick as the click, before any transition ends.
  const fading = await added.evaluate((f) => {
    const head = f.querySelector<HTMLInputElement>("tr.rv-head input[name=lines]")!;
    const headRow = head.closest("tr");
    head.click();
    return Array.from(f.querySelectorAll("tr.rv-row")).flatMap((tr, i) => {
      const m = tr.querySelector(".rv-minus");
      return tr !== headRow && m && parseFloat(getComputedStyle(m).opacity) > 0 ? [i] : [];
    });
  });
  expect(fading).toEqual([]);
  await expect(ticked).toHaveCount(0);
  await expect(box).toBeChecked();
  await expect(page.getByTestId("course-review-left-out")).toHaveText("1 change left out");
  // Upwards works too, and a drag takes exactly the rows it passed over,
  // even a line that opens a list (a click on one takes the list with it).
  await drag(5, 4);
  await expect(ticked).toHaveCount(2);
  // A drag into a group eats what it covers; the rest of that group comes back in.
  const out = async () => {
    const n = await lines.count(), res: string[] = [];
    for (let i = 0; i < n; i++) {
      const b = lines.nth(i).getByTestId("course-review-exclude-line");
      if (await b.isChecked()) res.push(`${i}:${await b.getAttribute("data-group")}`);
    }
    return res;
  };
  await drag(2, 4);
  let left = await out();
  expect(left.map((r) => r.split(":")[0])).toEqual(["2", "3", "4"]);
  expect(new Set(left.map((r) => r.split(":")[1])).size).toBe(1);
  // One that covers a whole group takes it in: one larger group.
  await drag(1, 4);
  left = await out();
  expect(left.map((r) => r.split(":")[0])).toEqual(["1", "2", "3", "4"]);
  expect(new Set(left.map((r) => r.split(":")[1])).size).toBe(1);
  await expect(page.getByTestId("course-review-left-out")).toHaveText("2 changes left out");
  const kept = await ticked.count();

  // The new lesson's − leaves it all out and folds it to its name; taken
  // back, it opens again with its group still left out.
  const whole = added.getByTestId("course-review-exclude-file");
  await whole.check();
  await expect(added.getByTestId("course-review-details")).not.toHaveAttribute("open", "");
  await expect(page.getByTestId("course-review-left-out")).toHaveText("1 change and 1 lesson left out");
  await whole.uncheck();
  await expect(added.getByTestId("course-review-details")).toHaveAttribute("open", "");
  await expect(ticked).toHaveCount(kept);
  await expect(page.getByTestId("course-review-left-out")).toHaveText("2 changes left out");
  await whole.check();
  // The same for Simplifying: its line stays left out.
  await changed.getByTestId("course-review-exclude-file").check();
  await expect(changed.getByTestId("course-review-details")).not.toHaveAttribute("open", "");
  await changed.getByTestId("course-review-exclude-file").uncheck();
  await expect(changed.getByTestId("course-review-details")).toHaveAttribute("open", "");
  await expect(box).toBeChecked();
  await expect(page.getByTestId("course-review-left-out")).toHaveText("1 change and 1 lesson left out");

  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("without the parts you left out");
  await expect(review(page)).toHaveCount(0);
  // Only the rest went in: the old reading with the AI's other changes, and no new lesson.
  const now = JSON.parse((await fileContent(page, id, simplifyPath))!);
  expect(now.content).toBe(original.content);
  expect(now).toEqual({ ...aiVersion, content: original.content });
  expect(await fileContent(page, id, addedPath)).toBeNull();
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(3);
  await expect(page.getByTestId("course-undo-label")).toContainText("Left out part of the AI's changes");

  // The review shows what was left out, and taking it all back in restores the AI's version.
  await openReview(page, id);
  await expect(fileTitled(page, "Simplifying").locator("input[name=lines]:checked")).toHaveCount(1);
  await expect(review(page).locator(`[data-testid="course-review-file"][data-path="${addedPath}"]`).getByTestId("course-review-exclude-file")).toBeChecked();
  await expect(page.getByTestId("course-review-left-out")).toHaveText("1 change and 1 lesson left out");
  const addedAgain = review(page).locator(`[data-testid="course-review-file"][data-path="${addedPath}"]`);
  await addedAgain.getByTestId("course-review-exclude-file").uncheck();
  // Its group of lines was kept with it, and comes back in with one click.
  await expect(addedAgain.locator("input[name=lines]:checked")).toHaveCount(kept);
  await addedAgain.locator("input[name=lines]:checked").first().uncheck();
  await expect(addedAgain.locator("input[name=lines]:checked")).toHaveCount(0);
  await expand(fileTitled(page, "Simplifying"));
  await fileTitled(page, "Simplifying").locator("input[name=lines]:checked").uncheck();
  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("All of the AI's changes are in again");
  expect(JSON.parse((await fileContent(page, id, simplifyPath))!)).toEqual(aiVersion);
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(4);
});

test("editing a line in place as text: it turns blue and stays in, Undo/Redo step through edits, Accept writes them", async ({ page }) => {
  const id = await makeCourse(page, "Fractions edited in review");
  await page.goto(`/courses/${id}`);
  const simplifyPath = (await page.getByTestId("course-lessons-row").filter({ hasText: "Simplifying" }).getAttribute("data-path"))!;
  await useAi(page, id, "change", "More worked examples, and add word problems.");
  const aiVersion = JSON.parse((await fileContent(page, id, simplifyPath))!);
  const changed = fileTitled(page, "Simplifying");
  const added = reviewFile(page, "added");
  const addedPath = (await added.getAttribute("data-path"))!;
  const aiTitle = JSON.parse((await fileContent(page, id, addedPath))!).title as string;
  const rowOf = (file: Locator, label: string) =>
    file.locator("tr.rv-row").filter({ has: page.locator(".rv-k", { hasText: new RegExp(`^${label}$`) }) }).first();
  const rx = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const footer = page.getByTestId("course-review-left-out");
  const history = page.getByTestId("course-review-history");
  const [undo, redo] = [page.getByTestId("course-review-undo"), page.getByTestId("course-review-redo")];
  // No edit yet: no Undo / Redo in the modal.
  await expect(history).toBeHidden();

  // The reading: a click on the AI's text opens a box holding the raw JSON line, full width.
  await expand(changed);
  const reading = rowOf(changed, "Reading");
  const readingCell = reading.getByTestId("course-review-add");
  const readingBox = reading.getByTestId("course-review-exclude-line");
  const lessonBox = changed.getByTestId("course-review-exclude-file");
  const green = await readingCell.evaluate((el) => getComputedStyle(el).backgroundColor);
  await readingCell.locator(".rv-nice").click();
  const editor = readingCell.getByTestId("course-review-edit-input");
  await expect(editor).toBeFocused();
  expect(await editor.evaluate((el) => el.tagName)).toBe("TEXTAREA");
  await expect(editor).toHaveValue(new RegExp(`^  "content": ${rx(JSON.stringify(aiVersion.content))},?$`));
  const [cellBox, editorBox] = [(await readingCell.boundingBox())!, (await editor.boundingBox())!];
  expect(editorBox.width).toBeGreaterThan(cellBox.width - 30);
  // Escape drops what was typed, and leaves the review open.
  await editor.fill("Thrown away");
  await editor.press("Escape");
  await expect(editor).toHaveCount(0);
  await expect(review(page)).toBeVisible();
  await expect(readingCell).not.toHaveClass(/rv-is-edited/);
  await expect(history).toBeHidden();
  // Typed and kept (⌘/Ctrl+Enter): the line turns blue (no "edited" badge, no ✎), loses its −,
  // the lesson's − is disabled saying why, the footer counts it, Undo / Redo appear.
  // Opened from the keyboard: the cell has the focus after an edit, Enter opens it.
  await expect(readingCell).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(editor).toBeFocused();
  await editor.fill('  "content": "My own reading.\\nIn two lines.",');
  await editor.press("Control+Enter");
  await expect(editor).toHaveCount(0);
  // Redrawn with the same plain label as the server's (#282).
  await expect(readingCell.locator(".rv-k")).toHaveText("Reading");
  await expect(readingCell.locator(".rv-nice")).toContainText("My own reading.");
  await expect(readingCell).toHaveClass(/rv-is-edited/);
  await expect(readingCell).not.toContainText("edited");
  await expect(readingCell.locator("button")).toHaveCount(0);
  const blue = await readingCell.evaluate((el) => [getComputedStyle(el).backgroundColor, getComputedStyle(el).color]);
  expect(blue).toEqual(["rgb(227, 238, 255)", "rgb(26, 95, 196)"]);
  expect(blue[0]).not.toBe(green);
  await expect(readingBox).toBeDisabled();
  await expect(readingBox).toBeHidden();
  await expect(lessonBox).toBeDisabled();
  expect(await lessonBox.locator("xpath=..").getAttribute("title")).toContain("you edited");
  await expect(footer).toHaveText("1 line edited");
  await expect(page.getByTestId("course-review-accept")).toHaveText("Accept");
  await expect(history).toBeVisible();
  await expect(undo).toBeEnabled();
  await expect(redo).toBeDisabled();
  // The JSON view shows the line as Accept will write it.
  await page.getByTestId("course-review-raw").click();
  await expect(readingCell.locator(".rv-raw")).toHaveText('"content": "My own reading.\\nIn two lines.",');
  await page.getByTestId("course-review-raw").click();

  // Undo: the AI's text, green again, with its − and the lesson's back.
  await undo.click();
  await expect(readingCell).not.toHaveClass(/rv-is-edited/);
  await expect(readingCell.locator(".rv-nice")).not.toContainText("My own reading.");
  await expect(readingBox).toBeEnabled();
  await expect(lessonBox).toBeEnabled();
  expect(await readingCell.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(green);
  await expect(footer).toHaveText("");
  await expect(history).toBeVisible();
  await expect(undo).toBeDisabled();
  await expect(redo).toBeEnabled();
  // Redo puts the edit back.
  await redo.click();
  await expect(readingCell).toHaveClass(/rv-is-edited/);
  await expect(readingCell.locator(".rv-nice")).toContainText("My own reading.");
  await expect(readingBox).toBeDisabled();
  // The keys, with no field focused: Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y.
  await page.getByTestId("course-review-overview").click();
  await page.keyboard.press("Control+z");
  await expect(readingCell).not.toHaveClass(/rv-is-edited/);
  await page.keyboard.press("Control+Shift+z");
  await expect(readingCell).toHaveClass(/rv-is-edited/);
  await page.keyboard.press("Control+z");
  await expect(readingCell).not.toHaveClass(/rv-is-edited/);
  await page.keyboard.press("Control+y");
  await expect(readingCell).toHaveClass(/rv-is-edited/);
  await expect(footer).toHaveText("1 line edited");

  // An unchanged line is editable too (the whole line: here its key as well).
  const groupRow = rowOf(changed, "Group");
  await expect(groupRow).toHaveClass(/same/);
  const sameCell = groupRow.locator("td[data-edit]");
  await expect(sameCell).toHaveCount(1);
  await sameCell.locator(".rv-nice").click();
  const sameEditor = sameCell.getByTestId("course-review-edit-input");
  await expect(sameEditor).toHaveValue(`  "group": ${JSON.stringify(aiVersion.group)},`);
  await sameEditor.fill('  "set": "by-hand",');
  await sameEditor.press("Control+Enter");
  await expect(sameCell).toHaveClass(/rv-is-edited/);
  await expect(sameCell.locator(".rv-nice")).toHaveText("Set by-hand");
  await expect(footer).toHaveText("2 lines edited");

  // The new lesson's title: Ctrl+Enter keeps it; emptied, nothing changes.
  await expand(added);
  const lines = added.getByTestId("course-review-row");
  const titleCell = rowOf(added, "Title").getByTestId("course-review-line");
  await titleCell.locator(".rv-nice").click();
  const titleEditor = titleCell.getByTestId("course-review-edit-input");
  await expect(titleEditor).toHaveValue(new RegExp(`^  "title": ${rx(JSON.stringify(aiTitle))},?$`));
  await titleEditor.fill("");
  await titleEditor.press("Control+Enter");
  await expect(titleCell).not.toHaveClass(/rv-is-edited/);
  await expect(footer).toHaveText("2 lines edited");
  await titleCell.locator(".rv-nice").click();
  await titleEditor.fill('  "title": "Problems in words",');
  await titleEditor.press("Control+Enter");
  await expect(footer).toHaveText("3 lines edited");
  await expect(titleCell).toHaveClass(/rv-is-edited/);
  // An edit may grow into several lines (Enter is a new line): a fourth option, "12", after "10".
  const last = added.locator("tr.rv-row").filter({ has: page.locator(".rv-v", { hasText: /^10$/ }) }).first();
  const lastCell = last.locator("td[data-edit]");
  await lastCell.locator(".rv-nice").click();
  const lastEditor = lastCell.getByTestId("course-review-edit-input");
  await expect(lastEditor).toHaveValue(/^ +"10"$/);
  const ten = await lastEditor.inputValue();
  await lastEditor.press("End");
  await lastEditor.press(",");
  await lastEditor.press("Enter");
  await lastEditor.pressSequentially(ten.replace('"10"', '"12"'));
  await expect(lastEditor).toHaveValue(`${ten},\n${ten.replace('"10"', '"12"')}`);
  await lastEditor.press("Control+Enter");
  await expect(lastCell).toHaveClass(/rv-is-edited/);
  await expect(lastCell.locator(".rv-nice")).toHaveCount(2);
  await expect(lastCell.locator(".rv-nice").nth(1)).toHaveText("12");
  await expect(footer).toHaveText("4 lines edited");
  // While typing, Ctrl+Z is the field's own: the modal's history is not touched.
  await titleCell.locator(".rv-nice").click();
  await titleEditor.press("Control+z");
  await titleEditor.press("Escape");
  await expect(titleCell.locator(".rv-nice")).toContainText("Problems in words");

  // A drag over the edited title skips it: the rows around it are left out, it stays in.
  const i = await lines.evaluateAll((trs) => trs.findIndex((tr) => /^Title$/.test(tr.querySelector(".rv-k")?.textContent ?? "")));
  expect(i).toBeGreaterThanOrEqual(0);
  const lineBox = (n: number) => lines.nth(n).getByTestId("course-review-exclude-line");
  await lines.nth(i + 2).scrollIntoViewIfNeeded();
  const a = (await lineBox(i + 2).boundingBox())!;
  const b = (await lines.nth(Math.max(i - 1, 0)).boundingBox())!;
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(a.x + a.width / 2, b.y + b.height / 2, { steps: 8 });
  await page.mouse.up();
  await expect(lineBox(i + 1)).toBeChecked();
  await expect(lineBox(i + 2)).toBeChecked();
  await expect(lineBox(i)).not.toBeChecked();
  await expect(lineBox(i)).toBeDisabled();
  await expect(footer).toHaveText("1 change left out, 4 lines edited");
  // Taken back in with the group's −.
  const head = added.locator("tr.rv-head").getByTestId("course-review-exclude-line");
  await head.uncheck();
  await expect(added.locator("input[name=lines]:checked")).toHaveCount(0);
  await expect(footer).toHaveText("4 lines edited");

  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("with your edits");
  const mine = JSON.parse((await fileContent(page, id, simplifyPath))!);
  const { group: _aiGroup, ...aiRest } = aiVersion;
  expect(mine).toEqual({ ...aiRest, content: "My own reading.\nIn two lines.", set: "by-hand" });
  const addedNow = JSON.parse((await fileContent(page, id, addedPath))!);
  expect(addedNow.title).toBe("Problems in words");
  expect(addedNow.quiz.find((q: { options?: string[] }) => q.options)?.options).toEqual(["3", "7", "10", "12"]);
  await expect(page.getByTestId("course-lessons-row").filter({ hasText: "Problems in words" })).toHaveCount(1);
  await expect(page.getByTestId("course-undo-label")).toContainText("Edited the AI's changes");

  // Reopened: the edits are there, blue, with no − and no badge; the modal's Undo starts empty.
  await openReview(page, id);
  await expect(review(page).locator("td.rv-is-edited")).toHaveCount(4);
  await expect(review(page).locator("td.rv-is-edited .rv-nice")).toHaveCount(5);
  await expect(review(page).locator("td.rv-is-edited").filter({ hasText: /\bedited\b/ })).toHaveCount(0);
  await expect(footer).toHaveText("4 lines edited");
  await expect(history).toBeHidden();
  const again = review(page).locator(`[data-testid="course-review-file"][data-path="${addedPath}"]`);
  await expand(again);
  const titleAgain = rowOf(again, "Title");
  await expect(titleAgain.getByTestId("course-review-line").locator(".rv-nice")).toContainText("Problems in words");
  await expect(titleAgain.getByTestId("course-review-exclude-line")).toBeDisabled();
  await expect(again.getByTestId("course-review-exclude-file")).toBeDisabled();
  // Still editable: typing the AI's line back makes it green and includable, Undo takes that back.
  const aiTitleLine = (await titleAgain.getByTestId("course-review-line").getAttribute("data-ai"))!;
  await titleAgain.getByTestId("course-review-line").locator(".rv-nice").click();
  await titleAgain.getByTestId("course-review-edit-input").fill(aiTitleLine);
  await titleAgain.getByTestId("course-review-edit-input").press("Control+Enter");
  await expect(titleAgain.getByTestId("course-review-line")).not.toHaveClass(/rv-is-edited/);
  await expect(titleAgain.getByTestId("course-review-exclude-line")).toBeEnabled();
  await expect(again.getByTestId("course-review-exclude-file")).toBeDisabled(); // its options edit is still there
  await expect(footer).toHaveText("3 lines edited");
  await undo.click();
  await expect(titleAgain.getByTestId("course-review-line")).toHaveClass(/rv-is-edited/);
  await expect(footer).toHaveText("4 lines edited");

  // A line that breaks the JSON is refused at Accept, by lesson name; the review stays open with it.
  await titleAgain.getByTestId("course-review-line").locator(".rv-nice").click();
  await titleAgain.getByTestId("course-review-edit-input").fill("garbage");
  await titleAgain.getByTestId("course-review-edit-input").press("Control+Enter");
  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-review-error")).toContainText(`Your edits to “${aiTitle}” leave it unreadable`);
  const broken = review(page).locator(`[data-testid="course-review-file"][data-path="${addedPath}"]`);
  await expand(broken);
  const brokenCell = broken.locator("td.rv-is-edited").filter({ hasText: "garbage" });
  await expect(brokenCell).toHaveCount(1);
  await brokenCell.locator(".rv-nice").click();
  await brokenCell.getByTestId("course-review-edit-input").fill(aiTitleLine);
  await brokenCell.getByTestId("course-review-edit-input").press("Control+Enter");
  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("with your edits");
  expect(JSON.parse((await fileContent(page, id, addedPath))!).title).toBe(aiTitle);
});

test("after a lesson-table edit, leaving out part of a lesson still goes in (#280)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions tabled after review");
  await page.goto(`/courses/${id}`);
  const rows = page.getByTestId("course-lessons-row");
  const simplifyPath = (await rows.filter({ hasText: "Simplifying" }).getAttribute("data-path"))!;
  const original = JSON.parse((await fileContent(page, id, simplifyPath))!);
  await useAi(page, id, "change", "More worked examples, and add word problems.");
  const aiVersion = JSON.parse((await fileContent(page, id, simplifyPath))!);
  await page.getByTestId("course-review-close").click();
  await expect(review(page)).toHaveCount(0);

  // Rename another lesson in the table: its save writes "order" into every lesson.
  const other = rows.filter({ hasText: "What a fraction is" });
  const otherPath = (await other.getAttribute("data-path"))!;
  await other.getByTestId("course-lessons-title").click();
  await page.getByTestId("course-lessons-rename").fill("Fractions, first look");
  const tableSaved = page.waitForResponse((r) => r.url().endsWith(`/api/courses/${id}/lessons`) && r.request().method() === "POST" && r.ok());
  await page.keyboard.press("Enter");
  await tableSaved;
  await editorSaved(page);
  expect(JSON.parse((await fileContent(page, id, simplifyPath))!).order).toBeDefined();

  // Leave out Simplifying's reading and Accept the rest: it goes in.
  await openReview(page, id);
  const changed = fileTitled(page, "Simplifying");
  await expand(changed);
  await changed.locator('[data-testid="course-review-row"]').filter({ has: page.locator(".rv-k", { hasText: /^Reading$/ }) }).first().getByTestId("course-review-exclude-line").check();
  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-review-error")).toHaveCount(0);
  await expect(page.getByTestId("course-edit-notice")).toContainText("without the parts you left out");
  const now = JSON.parse((await fileContent(page, id, simplifyPath))!);
  expect(now.content).toBe(original.content);
  expect(now.quiz).toEqual(aiVersion.quiz);
  expect(now.order).toBeDefined();
  // The rename stays.
  expect(JSON.parse((await fileContent(page, id, otherPath))!).title).toBe("Fractions, first look");
});

test("after Undo of the AI run the review says so and offers no Accept (#281)", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const id = await makeCourse(page, "Fractions undone after review");
  await page.goto(`/courses/${id}`);
  const simplifyPath = (await page.getByTestId("course-lessons-row").filter({ hasText: "Simplifying" }).getAttribute("data-path"))!;
  const original = JSON.parse((await fileContent(page, id, simplifyPath))!);
  await useAi(page, id, "change", "More worked examples, and add word problems.");
  await expect(page.getByTestId("course-review-accept")).toBeVisible();
  await expect(page.getByTestId("course-review-undone")).toHaveCount(0);
  await page.getByTestId("course-review-close").click();
  await page.getByTestId("course-undo").click();
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(3);

  // The review shows the run read-only: "Undone" with an (i), no −, no Accept, no editable lines.
  await openReview(page, id);
  await expect(page.getByTestId("course-review-undone")).toHaveText("Undone");
  await expect(page.getByTestId("course-review-undone-text")).toBeHidden();
  await page.getByTestId("course-review-undone-info").locator("summary").click();
  await expect(page.getByTestId("course-review-undone-text")).toContainText("Redo on the course page puts them back");
  // The card spans the modal's head, not the width of the (i).
  expect((await page.getByTestId("course-review-undone-text").boundingBox())!.width).toBeGreaterThan(300);
  await expect(page.getByTestId("course-review-accept")).toHaveCount(0);
  await expect(review(page).locator("input[name=lines], input[name=file]")).toHaveCount(0);
  await expect(review(page).locator("[data-edit]")).toHaveCount(0);
  // The page's script copes with a review that has no − boxes.
  expect(errors).toEqual([]);
  // Accept sent anyway (an old page) is refused; nothing changes.
  const sent = await page.request.post(`/courses/${id}/review`, { form: {} });
  expect(await sent.text()).toContain("This AI run was undone");
  expect(JSON.parse((await fileContent(page, id, simplifyPath))!)).toEqual(original);

  // Redo: the review can take it again.
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-redo").click();
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(4);
  await openReview(page, id);
  await expect(page.getByTestId("course-review-undone")).toHaveCount(0);
  await expect(page.getByTestId("course-review-accept")).toBeVisible();
});

test("the review labels lines in plain words, not JSON keys (#282)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions in plain words");
  await useAi(page, id, "change", "More worked examples, and add word problems.");
  const added = reviewFile(page, "added");
  await expand(added);
  const labels = await added.locator(".rv-k").allInnerTexts();
  expect(labels).toContain("Question");
  expect(labels).toContain("Answer");
  for (const raw of ["Prompt", "Question id", "Correct answer", "Slug", "Content"]) expect(labels).not.toContain(raw);
  // Id-only lines are dimmed.
  const idCell = added.locator("td.code").filter({ has: page.locator(".rv-k", { hasText: /^Id$/ }) }).first();
  await expect(idCell).toHaveClass(/rv-quiet/);
  expect(Number(await idCell.evaluate((el) => getComputedStyle(el).opacity))).toBeLessThan(1);
});

test("Don't show this automatically next time: the review waits for its button", async ({ page }) => {
  const id = await makeCourse(page, "Fractions reviewed quietly");
  await useAi(page, id, "add", "comparing two fractions");
  const quiet = page.getByTestId("course-review-quiet");
  await expect(quiet).not.toBeChecked();
  // Saved as soon as it is ticked, without Accept.
  const saved = page.waitForResponse((r) => r.url().endsWith("/api/review-quiet") && r.ok());
  await quiet.check();
  await saved;
  await page.getByTestId("course-review-close").click();

  // The next job ends on the course page, saying where the review is.
  await useAi(page, id, "add", "ordering fractions by size", false);
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(5);
  await page.getByTestId("course-ai-review").click();
  await expect(review(page)).toBeVisible();
  await expect(page.getByTestId("course-review-quiet")).toBeChecked();
  // Unticked (and sent with Accept): the next job opens it again.
  await page.getByTestId("course-review-quiet").uncheck();
  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("The AI's changes are in.");
  await useAi(page, id, "add", "fractions on a number line");
});

test("Review changes is a button next to Submit", async ({ page }) => {
  const id = await makeCourse(page, "Fractions with a button");
  await page.goto(`/courses/${id}`);
  const submit = page.getByTestId("course-ai-submit");
  const button = page.getByTestId("course-ai-review");
  await expect(button).toHaveText("Review changes");
  await expect(button).toHaveClass(/\bbtn\b/);
  // On the same line as Submit, to its right, below the text box.
  const s = (await submit.boundingBox())!;
  const b = (await button.boundingBox())!;
  expect(Math.abs(s.y + s.height / 2 - (b.y + b.height / 2))).toBeLessThan(4);
  expect(b.x).toBeGreaterThan(s.x + s.width);
  const text = (await page.getByTestId("course-ai-text").boundingBox())!;
  expect(b.y).toBeGreaterThan(text.y + text.height);
  await button.click();
  await expect(review(page)).toBeVisible();
});

test("a course nobody used AI on has no review button", async ({ page }) => {
  await devLogin(page, "keys=fake&next=/courses");
  await createCourse(page);
  await page.getByTestId("generate-close").click();
  await expect(page.getByTestId("course-edit")).toBeVisible();
  await expect(page.getByTestId("course-ai")).toBeVisible();
  await expect(page.getByTestId("course-ai-review")).toHaveCount(0);
  // Asking for it anyway shows the page without a modal.
  await page.goto(page.url().replace(/[?#].*$/, "") + "?review=1");
  await expect(review(page)).toHaveCount(0);
});

test("without JavaScript the review is a page whose boxes Accept sends", async ({ browser }) => {
  const js = await browser.newPage();
  await devLogin(js);
  const id = await makeCourse(js, "Fractions without scripts");
  await useAi(js, id, "add", "comparing two fractions");
  const addedPath = (await reviewFile(js, "added").getAttribute("data-path"))!;
  const state = await js.context().storageState();
  await js.close();
  const ctx = await browser.newContext({ javaScriptEnabled: false, storageState: state });
  const page = await ctx.newPage();
  await page.goto(`/courses/${id}?review=1`);
  await expect(review(page)).toBeVisible();
  const added = reviewFile(page, "added");
  await added.getByTestId("course-review-exclude-file").check();
  await page.getByTestId("course-review-accept").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("without the parts you left out");
  expect((await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(addedPath)}`)).ok()).toBeFalsy();
  await ctx.close();
});
