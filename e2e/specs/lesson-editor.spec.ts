// Exact specs for the lesson editor (#841): the drawer's Forms mode for a
// lesson is an outline of the file, one hand-written form per item kind and
// a live preview, editing the same document as the JSON box. Against a copy
// of the test course, whose first lesson has every kind at once.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyCourse, devLogin, devReset, editorSaved, fastAutosave, TEST_COURSE } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await fastAutosave(page);
  // The drawer opens as a form (the gear's JSON switch, remembered in the browser).
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await devLogin(page, `open=${TEST_COURSE}`);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const at = (scope: Page | Locator, testid: string, path: string) => scope.locator(`[data-testid="${testid}"][data-path="${path}"]`);

/** A copy of the test course, its editor open on the "Everything at once" lesson's form. */
async function openEverything(page: Page) {
  const id = await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Everything at once" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  await expect(row.getByTestId("course-lessons-drawer")).toHaveAttribute("data-mode", "forms");
  await expect(form.getByTestId("course-form-layout")).toBeVisible();
  const path = (await row.getAttribute("data-path"))!;
  const doc = async () => JSON.parse(await row.locator(".lt-json").evaluate((el: HTMLTextAreaElement) => el.value));
  const saved = async () => JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()).content);
  return { id, row, form, path, doc, saved };
}
const section = (form: Locator, sec: string) => form.locator(`[data-testid="course-outline-section"][data-sec="${sec}"]`);
const item = (form: Locator, sec: string, idx: number, sub?: string) =>
  form.locator(`[data-testid="course-outline-item"][data-sec="${sec}"][data-idx="${idx}"]${sub ? `[data-sub="${sub}"]` : ""}`);
async function openSection(form: Locator, sec: string) {
  const head = section(form, sec);
  if ((await head.getAttribute("aria-expanded")) !== "true") await head.click();
  await expect(head).toHaveAttribute("aria-expanded", "true");
}

test("the outline lists every section of the lesson, opens its items, and the preview knows which modes show them", async ({ page }) => {
  const { form } = await openEverything(page);
  // Every section, with counts; the lesson's own fields are the first.
  for (const sec of ["lesson", "content", "vocabulary", "quiz", "listening", "sprint", "conversations", "conjugations", "guide"]) {
    await expect(section(form, sec)).toBeVisible();
  }
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-sec", "lesson");
  await expect(at(form, "course-form-field", "title")).toHaveValue("Everything at once");
  // No ids, no slug, no unit, no order: the table sets them.
  await expect(form.locator('[data-testid="course-form-field"][data-path="id"], [data-path="slug"], [data-path="unit"], [data-path="order"]')).toHaveCount(0);
  await expect(form.getByTestId("course-preview")).toHaveAttribute("data-mode", "study");

  // The quiz: six questions, each with its type's badge.
  await openSection(form, "quiz");
  const items = form.locator('[data-testid="course-outline-item"][data-sec="quiz"]');
  await expect(items).toHaveCount(6);
  await expect(items.nth(0).locator(".le-badge")).toHaveText("A·B·C");
  await expect(items.nth(1).locator(".le-badge")).toHaveText("T/F");
  await expect(items.nth(2).locator(".le-badge")).toHaveText("___");
  await expect(items.nth(3).locator(".le-badge")).toHaveText("123");
  await expect(items.nth(4).locator(".le-badge")).toHaveText("ƒ(x)");
  await items.nth(2).click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "2");
  await expect(items.nth(2)).toHaveAttribute("aria-pressed", "true");
  // A quiz question shows in Study, Sprint and the Guide; a sort only in Sprint.
  await expect(form.getByTestId("course-form-mode").locator(".le-seg-btn")).toHaveText(["Study", "Sprint", "Guide"]);
  await openSection(form, "sprint");
  await item(form, "sprint", 0, "sorts").click();
  await expect(form.getByTestId("course-form-mode").locator(".le-seg-btn")).toHaveText(["Sprint"]);
  await expect(form.getByTestId("course-preview-phone")).toHaveAttribute("data-mode", "sprint");
  // Previous / Next walk a section.
  await openSection(form, "quiz");
  await items.nth(0).click();
  await form.getByTestId("course-form-next").click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "1");
  await form.getByTestId("course-form-prev").click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "0");
});

test("a multiple-choice question: the right answer is picked, a new option grows from the dashed box, the type switch reshapes the form, and it all lands in the file", async ({ page }) => {
  const { form, doc, saved } = await openEverything(page);
  await openSection(form, "quiz");
  await item(form, "quiz", 0).click();
  const q = "quiz.0";
  await expect(at(form, "course-form-field", `${q}.prompt`)).toHaveValue("How do you say 'hello'?");
  await expect(at(form, "course-form-correct", `${q}.options.0`)).toBeChecked();
  // Typing goes straight into the document (the JSON box) and the outline's label.
  await at(form, "course-form-field", `${q}.prompt`).fill("How do you greet someone?");
  expect((await doc()).quiz[0].prompt).toBe("How do you greet someone?");
  await expect(item(form, "quiz", 0)).toContainText("How do you greet someone?");
  // The dashed box under the options makes a new one from the first letter typed, and keeps the focus.
  await form.getByTestId("course-form-new-option").pressSequentially("nada");
  await expect(at(form, "course-form-field", `${q}.options.3`)).toHaveValue("nada");
  await expect(at(form, "course-form-field", `${q}.options.3`)).toBeFocused();
  expect((await doc()).quiz[0].options).toEqual(["hola", "adiós", "gracias", "nada"]);
  // Pick another option as the answer; retyping it keeps it the answer.
  await at(form, "course-form-correct", `${q}.options.1`).check();
  expect((await doc()).quiz[0].correct_answer).toBe("adiós");
  await at(form, "course-form-field", `${q}.options.1`).fill("adiós, Ana");
  expect((await doc()).quiz[0].correct_answer).toBe("adiós, Ana");
  await at(form, "course-form-remove", `${q}.options.3`).click();
  expect((await doc()).quiz[0].options).toEqual(["hola", "adiós, Ana", "gracias"]);
  await expect(at(form, "course-form-field", `${q}.options`).locator("..").getByTestId("course-form-list")).toHaveCount(0);
  // No "Correct answer" box for a multiple-choice question: the radio is the answer.
  await expect(at(form, "course-form-field", `${q}.correct_answer`)).toHaveCount(0);
  await expect(at(form, "course-form-field", `${q}.tolerance`)).toHaveCount(0);

  // The type switch: true/false drops the options and shows two buttons.
  await form.getByTestId("course-form-type").locator('[data-value="true-false"]').click();
  await expect(at(form, "course-form-field", `${q}.correct_answer`)).toBeVisible();
  await expect(form.locator('[data-testid="course-form-list"][data-path="quiz.0.options"]')).toHaveCount(0);
  await form.locator('.le-tf-btn[data-value="false"]').click();
  expect((await doc()).quiz[0]).toMatchObject({ type: "true-false", correct_answer: "false" });
  // The options with text in them stay in the file, for a switch back.
  expect((await doc()).quiz[0].options).toEqual(["hola", "adiós, Ana", "gracias"]);
  // Numeric: answer, tolerance and unit in a row.
  await form.getByTestId("course-form-type").locator('[data-value="numeric"]').click();
  await at(form, "course-form-field", `${q}.correct_answer`).fill("12.57");
  await at(form, "course-form-field", `${q}.tolerance`).fill("0.01");
  await at(form, "course-form-field", `${q}.unit`).fill("cm²");
  await expect(at(form, "course-form-field", `${q}.tolerance`).locator("..").locator(".le-check")).toContainText("±0.01");
  expect((await doc()).quiz[0]).toMatchObject({ type: "numeric", correct_answer: "12.57", tolerance: 0.01, unit: "cm²" });
  // Back to multiple choice: the unit and tolerance go, the answer becomes the first option.
  await form.getByTestId("course-form-type").locator('[data-value="multiple-choice"]').click();
  await expect(at(form, "course-form-field", `${q}.options.0`)).toHaveValue("12.57");
  const d = await doc();
  expect(d.quiz[0].tolerance).toBeUndefined();
  expect(d.quiz[0].unit).toBeUndefined();
  await at(form, "course-form-field", `${q}.options.1`).fill("13");
  // ↓ in the footer moves the question down the list; the selection follows it.
  await form.getByTestId("course-form-down").click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "1");
  await expect(item(form, "quiz", 1)).toContainText("How do you greet someone?");
  await form.getByTestId("course-form-up").click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "0");
  await editorSaved(page);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  const file = await saved();
  expect(file.quiz[0]).toMatchObject({ question_id: "tc-everything-01", prompt: "How do you greet someone?", type: "multiple-choice", correct_answer: "12.57", options: ["12.57", "13", "gracias"] });
  // Everything else in the file is untouched.
  expect(file.quiz[1].prompt).toBe("'Adiós' means 'goodbye'.");
  expect(file.sprint.icon).toBe("🧪");
});

test("checks appear on the field as you type, in the outline, and from the server", async ({ page }) => {
  const { form, doc } = await openEverything(page);
  await openSection(form, "quiz");
  await item(form, "quiz", 2).click();
  const q = "quiz.2";
  // A fill-in-blank: the blank is noted, and the answer must not be empty.
  await expect(at(form, "course-form-field", `${q}.prompt`).locator("..").locator(".le-check")).toHaveAttribute("data-level", "ok");
  await at(form, "course-form-field", `${q}.prompt`).fill("Muchas");
  await expect(at(form, "course-form-field", `${q}.prompt`).locator("..").locator(".le-check")).toHaveAttribute("data-level", "warn");
  await at(form, "course-form-field", `${q}.correct_answer`).fill("");
  const answerCheck = at(form, "course-form-field", `${q}.correct_answer`).locator("..").locator(".le-check");
  await expect(answerCheck).toHaveAttribute("data-level", "bad");
  await expect(answerCheck).toContainText("empty");
  await expect(item(form, "quiz", 2).locator(".le-warn")).toHaveCount(1);
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-problem", "true");
  await at(form, "course-form-field", `${q}.correct_answer`).fill("gracias");
  await expect(answerCheck).toBeHidden();
  await expect(item(form, "quiz", 2).locator(".le-warn")).toHaveCount(0);
  // A word the question teaches is picked from the vocabulary, and follows the word when it is renamed (#1200).
  await at(form, "course-form-field", `${q}.prompt`).fill("Muchas ___.");
  await form.getByTestId("course-form-add-teaches").selectOption("hola");
  expect((await doc()).quiz[2].teaches).toEqual(["hola"]);
  await openSection(form, "vocabulary");
  await item(form, "vocabulary", 0).click();
  await at(form, "course-form-field", "vocabulary.0.term").fill("holá");
  expect((await doc()).quiz[2].teaches).toEqual(["holá"]);
  expect((await doc()).quiz[0].teaches).toEqual(["holá"]);
  await at(form, "course-form-field", "vocabulary.0.term").fill("hola");
  // A heading renamed in the content: the Guide step that names it turns bad, and the outline flags the Guide from another section.
  await section(form, "content").click();
  const content = at(form, "course-form-field", "content");
  const text = await content.inputValue();
  await content.fill(text.replace("## Greetings", "## Hello there"));
  await expect(section(form, "guide").locator(".le-warn")).toHaveCount(1);
  // The server refuses the table too (the course rules); the line under the title opens the Guide on the step the rules refuse, which says what to do.
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "error");
  await expect(page.getByTestId("course-edit-save-error")).toContainText("Greetings");
  await page.getByTestId("course-edit-save-error").click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-sec", "guide");
  await expect(form.locator('[data-testid="course-form-step"][data-path="guide.2"]')).toHaveAttribute("aria-pressed", "true");
  await expect(form.locator('[data-testid="course-form-item"][data-path="guide.2"] .le-warn')).toHaveCount(1);
  await expect(form.locator('.le-check[data-check="guide.2"]')).toContainText("Pick a heading or remove the step");
  // The heading back: saved.
  await section(form, "content").click();
  await content.fill(text);
  await editorSaved(page);
  await expect(section(form, "guide").locator(".le-warn")).toHaveCount(0);
});

test("a field's own check is a mark in its corner that opens a modal, so typing moves nothing below it (#1320)", async ({ page }) => {
  const { form } = await openEverything(page);
  await openSection(form, "quiz");
  await item(form, "quiz", 0).click();
  const explanation = at(form, "course-form-field", "quiz.0.explanation");
  const check = form.locator('.le-check[data-check="quiz.0.explanation"]');
  const hints = form.locator('.le-f[data-field="quiz.0.hints"]');
  await expect(check).toBeHidden();
  // Hints' place below the Explanation box (the page may scroll as the box is typed in).
  const gap = async () => (await hints.boundingBox())!.y - (await explanation.boundingBox())!.y;
  const before = await gap();
  // No explanation: an amber ! appears in the field's corner, and Hints stays where it was.
  await explanation.fill("");
  await expect(check).toBeVisible();
  await expect(check).toHaveAttribute("data-level", "warn");
  await expect(check.locator(".le-check-mark")).toHaveText("!");
  expect(await gap()).toBe(before);
  // The ! opens the message in a modal.
  await check.click();
  const dialog = page.getByTestId("course-form-check-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("course-form-check-list")).toContainText("explanation");
  await dialog.getByTestId("course-form-check-close").click();
  await expect(dialog).toBeHidden();
  // A letter typed: the ! goes, and still nothing moved.
  await explanation.fill("H");
  await expect(check).toBeHidden();
  expect(await gap()).toBe(before);
});

test("a field's mark sits on its label's row in a two-column row too (#1320)", async ({ page }) => {
  const { form } = await openEverything(page);
  await openSection(form, "quiz");
  await item(form, "quiz", 2).click();
  // Fill in the blank: Correct answer sits beside Also accept in a two-column row.
  await at(form, "course-form-field", "quiz.2.correct_answer").fill("");
  const mark = form.locator('.le-check[data-check="quiz.2.correct_answer"]');
  await expect(mark).toHaveAttribute("data-level", "bad");
  const m = (await mark.boundingBox())!;
  const label = (await form.locator('.le-f[data-field="quiz.2.correct_answer"] > .le-label').boundingBox())!;
  const mid = m.y + m.height / 2;
  expect(mid).toBeGreaterThanOrEqual(label.y);
  expect(mid).toBeLessThanOrEqual(label.y + label.height);
});

test("Try it: the preview grades a question with the course's own checker, in Study and in Sprint", async ({ page }) => {
  const { form } = await openEverything(page);
  await openSection(form, "quiz");
  await item(form, "quiz", 0).click();
  const preview = form.getByTestId("course-preview");
  await expect(preview).toHaveAttribute("data-mode", "study");
  await preview.getByTestId("course-preview-option").nth(1).check();
  await preview.getByTestId("course-preview-check").click();
  const fb = preview.getByTestId("course-preview-feedback");
  await expect(fb).toHaveAttribute("data-state", "wrong");
  await expect(fb).toContainText("hola");
  await expect(fb).toContainText("Hola is hello; adiós is goodbye.");
  await preview.getByTestId("course-preview-again").click();
  await preview.getByTestId("course-preview-option").nth(0).check();
  await preview.getByTestId("course-preview-check").click();
  await expect(fb).toHaveAttribute("data-state", "right");
  // A numeric question in the Guide skin: the tolerance lets 4π through.
  await item(form, "quiz", 3).click();
  await at(form, "course-form-field", "quiz.3.correct_answer").fill("12.57");
  await at(form, "course-form-field", "quiz.3.tolerance").fill("0.01");
  await form.getByTestId("course-form-mode").locator('[data-value="guide"]').click();
  // The preview is drawn again shortly after the last keystroke (its data-key holds what it shows): let it settle before answering.
  await expect(form.getByTestId("course-preview-body")).toHaveAttribute("data-key", /"tolerance":0\.01/);
  await preview.getByTestId("course-preview-answer").fill("4π");
  await preview.getByTestId("course-preview-check").click();
  await expect(fb).toHaveAttribute("data-state", "right");
  // A fill-in-blank in Sprint: tiles, the answer among them; the slot fills on a tap.
  await item(form, "quiz", 2).click();
  await form.getByTestId("course-form-mode").locator('[data-value="sprint"]').click();
  const tiles = preview.getByTestId("course-preview-tile");
  await expect(tiles).toHaveCount(4);
  await expect(tiles.filter({ hasText: /^gracias$/ })).toHaveCount(1);
  await tiles.filter({ hasText: /^gracias$/ }).click();
  await preview.getByTestId("course-preview-check").click();
  await expect(preview.getByTestId("course-preview-feedback")).toHaveAttribute("data-state", "right");
  // Nothing was recorded for the learner.
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "saved");
});

test("Sprint exercises: buckets and items, the wrong word by a click, tiles and distractors, played in the preview", async ({ page }) => {
  const { form, doc } = await openEverything(page);
  await openSection(form, "sprint");
  // The path icon, then the exercises in order, each with its badge.
  const sprintItems = form.locator('[data-testid="course-outline-item"][data-sec="sprint"]');
  await expect(sprintItems.locator(".le-badge")).toHaveText(["ICON", "BITE", "SORT", "SPOT", "BUILD"]);
  // Sort: a third bucket, an item moved into it with ▸, the check counts.
  await item(form, "sprint", 0, "sorts").click();
  await form.getByTestId("course-form-add-bucket").click();
  await at(form, "course-form-field", "sprint.sorts.0.buckets.2").fill("neither");
  expect((await doc()).sprint.sorts[0].items[0].bucket).toBe(0);
  await at(form, "course-form-bucket-next", "sprint.sorts.0.items.0").click();
  expect((await doc()).sprint.sorts[0]).toMatchObject({ buckets: ["arriving", "leaving", "neither"] });
  expect((await doc()).sprint.sorts[0].items[0].bucket).toBe(1);
  await at(form, "course-form-bucket-next", "sprint.sorts.0.items.0").click();
  expect((await doc()).sprint.sorts[0].items[0].bucket).toBe(2);
  await expect(at(form, "course-form-bucket-next", "sprint.sorts.0.items.0")).toBeDisabled();
  await expect(form.locator('.le-check[data-check="sprint.sorts.0.items"]')).toContainText("4 items");
  // Played in the preview as Sprint plays it (#1148): each tap judged at once, one
  // wrong tap first, and the sort checks itself once the pile is empty.
  const preview = form.getByTestId("course-preview");
  await expect(form.getByTestId("course-preview-body")).toHaveAttribute("data-key", /neither/);
  const bucketOf = new Map(((await doc()).sprint.sorts[0].items as { text: string; bucket: number }[]).map((i) => [i.text, i.bucket]));
  const previewBuckets = preview.getByTestId("course-preview-bucket");
  const card = preview.locator(".sprint-sort-item");
  for (let i = 0; i < bucketOf.size; i++) {
    const b = bucketOf.get((await card.innerText()).trim())!;
    if (i === 0) {
      await previewBuckets.nth(b === 0 ? 1 : 0).click();
      await expect(preview.getByTestId("course-preview-chip")).toHaveCount(0);
    }
    await previewBuckets.nth(b).click();
    if (i === 0) await expect(preview.locator(".sprint-bucket.miss")).toHaveCount(0);
  }
  await expect(preview.getByTestId("course-preview-feedback")).toHaveAttribute("data-state", "wrong");
  await expect(preview.getByTestId("course-preview-feedback")).toContainText("All sorted, but with 1 wrong tap");
  // Spot: the wrong word is a click on the sentence's words; the check wants it exactly once.
  await item(form, "sprint", 0, "spots").click();
  await expect(form.locator('.le-word[aria-pressed="true"]')).toHaveText("grasias.");
  await form.locator('.le-word[data-value="Muchas"]').click();
  expect((await doc()).sprint.spots[0].wrong).toBe("Muchas");
  await expect(form.locator('.le-check[data-check="sprint.spots.0.wrong"]')).toHaveAttribute("data-level", "ok");
  await at(form, "course-form-field", "sprint.spots.0.sentence").fill("Muchas Muchas gracias.");
  await expect(form.locator('.le-check[data-check="sprint.spots.0.wrong"]')).toHaveAttribute("data-level", "bad");
  await at(form, "course-form-field", "sprint.spots.0.sentence").fill("Adiós, Ana. Muchas grasias.");
  await form.locator('.le-word[data-value="grasias."]').click();
  expect((await doc()).sprint.spots[0].wrong).toBe("grasias");
  // Build: the words of the text are shown as tiles; a distractor is added to the list.
  await item(form, "sprint", 0, "builds").click();
  await expect(form.getByTestId("course-form-words").locator(".le-word")).toHaveText(["Muy", "bien,", "gracias."]);
  await at(form, "course-form-add", "sprint.builds.0.distractors").click();
  await at(form, "course-form-field", "sprint.builds.0.distractors.1").fill("mal");
  expect((await doc()).sprint.builds[0].distractors).toEqual(["adiós", "mal"]);
  await expect(form.getByTestId("course-preview-body")).toHaveAttribute("data-key", /mal/);
  await expect(preview.getByTestId("course-preview-tile")).toHaveCount(5);
  // Bite: title, rule, example.
  await item(form, "sprint", 0, "bites").click();
  await at(form, "course-form-field", "sprint.bites.0.rule").fill("Hola to arrive, adiós to leave.");
  await expect(preview.locator(".sprint-bite-rule")).toHaveText("Hola to arrive, adiós to leave.");
  await editorSaved(page);
});

test("words, sentences, conversations and tables have their own forms", async ({ page }) => {
  const { form, doc } = await openEverything(page);
  await openSection(form, "vocabulary");
  await item(form, "vocabulary", 0).click();
  await expect(at(form, "course-form-field", "vocabulary.0.term")).toHaveValue("hola");
  await expect(form.getByTestId("course-form-uses")).toContainText("Questions 1");
  await item(form, "vocabulary", 1).click();
  await expect(at(form, "course-form-field", "vocabulary.1.term")).toHaveValue("adiós");
  await expect(form.getByTestId("course-form-uses")).toContainText("No question teaches it");
  await openSection(form, "listening");
  await item(form, "listening", 2).click();
  await expect(at(form, "course-form-field", "listening.2.note")).toHaveValue("New word: con = with.");
  await expect(form.locator('.le-check[data-check="listening.2.note"]')).toHaveAttribute("data-level", "ok");
  await at(form, "course-form-field", "listening.2.text").fill("Hola, ¿qué tal?");
  await expect(form.locator('.le-check[data-check="listening.2.text"]')).toContainText("Another sentence");
  await at(form, "course-form-field", "listening.2.text").fill("Hablo con Ana y Luis.");
  await expect(form.locator('.le-check[data-check="listening.2.text"]')).toContainText("5 words");
  // A conversation: lines as a grid with a speaker toggle; a new line takes the other speaker.
  await openSection(form, "conversations");
  await item(form, "conversations", 0).click();
  await expect(at(form, "course-form-field", "conversations.0.role_a")).toHaveValue("Ana");
  await at(form, "course-form-add", "conversations.0.lines").click();
  await at(form, "course-form-field", "conversations.0.lines.3.text").fill("¡Hasta luego, Ana!");
  await at(form, "course-form-field", "conversations.0.lines.3.meaning").fill("See you later, Ana!");
  await form.locator('[data-act="speaker"][data-path="conversations.0.lines.3.speaker"][data-value="a"]').click();
  expect((await doc()).conversations[0].lines[3]).toEqual({ speaker: "a", text: "¡Hasta luego, Ana!", meaning: "See you later, Ana!" });
  await expect(form.locator('.le-check[data-check="conversations.0.lines"]')).toContainText("3 lines of 3–9 words");
  // A conjugation table: one box per slot of the course's paradigm, the set from its sets.
  await openSection(form, "conjugations");
  await item(form, "conjugations", 0).click();
  await expect(form.locator(".le-forms input")).toHaveCount(7);
  await expect(at(form, "course-form-field", "conjugations.0.set")).toHaveValue("preterite");
  await at(form, "course-form-field", "conjugations.0.forms.tu").fill("");
  await expect(form.locator('.le-check[data-check="conjugations.0.forms"]')).toContainText("Missing: tú");
  await at(form, "course-form-field", "conjugations.0.forms.tu").fill("hablaste");
  await expect(form.locator('.le-check[data-check="conjugations.0.forms"]')).toHaveAttribute("data-level", "ok");
  await expect(form.getByTestId("course-preview").locator("table.conj td").nth(2)).toHaveText("hablé");
  await editorSaved(page);
});

test("items are added and deleted from the outline, the Guide's steps are ordered by hand, and the lesson's prerequisites are picked", async ({ page }) => {
  const { form, doc, saved } = await openEverything(page);
  page.on("dialog", (d) => d.accept());
  await openSection(form, "quiz");
  await form.locator('[data-testid="course-outline-add"][data-path="quiz"]').click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "6");
  await expect(at(form, "course-form-field", "quiz.6.prompt")).toBeFocused();
  await at(form, "course-form-field", "quiz.6.prompt").fill("Which word is a greeting?");
  await at(form, "course-form-field", "quiz.6.options.0").fill("hola");
  await at(form, "course-form-field", "quiz.6.options.1").fill("dos");
  await at(form, "course-form-correct", "quiz.6.options.0").check();
  expect((await doc()).quiz[6]).toEqual({ question_id: "tc-everything-q1", prompt: "Which word is a greeting?", type: "multiple-choice", options: ["hola", "dos"], correct_answer: "hola" });
  await form.getByTestId("course-form-duplicate").click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "7");
  expect((await doc()).quiz[7].question_id).toBe("tc-everything-q2");
  await form.getByTestId("course-form-remove").last().click();
  expect((await doc()).quiz).toHaveLength(7);
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-idx", "6");
  // A Sprint exercise from the section's own add links.
  await openSection(form, "sprint");
  await form.locator('[data-testid="course-outline-add"][data-path="sprint.spots"]').click();
  await expect(form.getByTestId("course-item")).toHaveAttribute("data-sub", "spots");
  await at(form, "course-form-field", "sprint.spots.1.sentence").fill("Hola, Ana. Muchas grasias.");
  await form.locator('.le-word[data-value="grasias."]').click();
  await at(form, "course-form-field", "sprint.spots.1.fix").fill("gracias");
  expect((await doc()).sprint.spots[1]).toMatchObject({ sentence: "Hola, Ana. Muchas grasias.", wrong: "grasias", fix: "gracias" });
  // The Guide: a tip step added at the end, moved up, then the steps dropped for derived ones.
  await section(form, "guide").click();
  await expect(form.locator('[data-testid="course-form-item"][data-path^="guide."]')).toHaveCount(13);
  await form.locator('[data-testid="course-form-add-step"][data-value="tip"]').click();
  await at(form, "course-form-field", "guide.13.tip").fill("Say it out loud.");
  await at(form, "course-form-up", "guide.13").click();
  expect((await doc()).guide[12]).toEqual({ tip: "Say it out loud." });
  await editorSaved(page);
  expect((await saved()).guide[12]).toEqual({ tip: "Say it out loud." });
  await form.getByTestId("course-form-guide-derive").click();
  expect((await doc()).guide).toBeUndefined();
  await expect(form.getByTestId("course-form-guide-by-hand")).toBeVisible();
  await form.getByTestId("course-form-guide-by-hand").click();
  expect((await doc()).guide[0]).toEqual({ section: "Introduction" });
  // The lesson's prerequisites are other lessons of the course, by title.
  await section(form, "lesson").click();
  await expect(at(form, "course-form-item", "prerequisites.0")).toContainText("Multiple choice");
  await form.getByTestId("course-form-add-prereq").selectOption({ label: "True or false" });
  expect((await doc()).prerequisites).toEqual(["tc-multiple-choice", "tc-true-false"]);
  await editorSaved(page);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
});

test("on a phone the three columns are tabs, and nothing overflows", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { form } = await openEverything(page);
  const tabs = form.locator(".le-tabs");
  await expect(tabs).toBeVisible();
  await expect(form.getByTestId("course-form-tab-edit")).toHaveAttribute("aria-selected", "true");
  await expect(form.getByTestId("course-outline")).toBeHidden();
  await form.getByTestId("course-form-tab-outline").click();
  await expect(form.getByTestId("course-outline")).toBeVisible();
  await openSection(form, "quiz");
  await item(form, "quiz", 0).click();
  // Picking an item in the outline goes to its form.
  await expect(form.getByTestId("course-form-tab-edit")).toHaveAttribute("aria-selected", "true");
  await expect(at(form, "course-form-field", "quiz.0.prompt")).toBeVisible();
  await form.getByTestId("course-form-tab-preview").click();
  await expect(form.getByTestId("course-preview-phone")).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test("the gear's Language features switch hides what only a language course fills, and is kept in the course settings", async ({ page }) => {
  const id = await copyCourse(page, TEST_COURSE);
  const manifest = async () => JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=course.json`)).json()).content);
  // With JSON off the dialog has the language switch and no Line numbers; the test course is a language course.
  await page.getByTestId("course-lessons-gear").click();
  const sw = page.getByTestId("course-lessons-language-switch");
  await expect(sw).toBeVisible();
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("course-lessons-lines-switch")).toBeHidden();
  // The (i) explains it in a dialog of its own.
  await page.getByTestId("course-lessons-language-info").click();
  await expect(page.getByTestId("course-lessons-language-dialog")).toBeVisible();
  await page.getByTestId("course-lessons-language-close").click();
  await expect(page.getByTestId("course-lessons-language-dialog")).toBeHidden();
  // Off: written into course.json and saved with the table.
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect.poll(async () => (await manifest()).language_features).toBe(false);

  // A lesson with no listening or conversations loses those sections; a new word is not asked for an example meaning.
  // + Build stays: the test course is a music course (`music` in course.json, #1232). The next test takes `music` out.
  // (By the row's title: an open drawer's form says "Multiple choice" too.)
  const row = page.getByTestId("course-lessons-row").filter({ has: page.locator(".lt-title", { hasText: "Multiple choice" }) }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  await expect(form.getByTestId("course-form-layout")).toBeVisible();
  await expect(section(form, "quiz")).toBeVisible();
  await expect(section(form, "listening")).toHaveCount(0);
  await expect(section(form, "conversations")).toHaveCount(0);
  await openSection(form, "sprint");
  await expect(form.locator('[data-testid="course-outline-add"][data-path="sprint.spots"]')).toBeVisible();
  await expect(form.locator('[data-testid="course-outline-add"][data-path="sprint.builds"]')).toBeVisible();
  await openSection(form, "vocabulary");
  await item(form, "vocabulary", 0).click();
  await expect(at(form, "course-form-field", "vocabulary.0.example_meaning")).toBeVisible(); // it has text, so it stays
  await form.locator('[data-testid="course-outline-add"][data-path="vocabulary"]').click();
  await expect(at(form, "course-form-field", "vocabulary.3.example")).toBeVisible();
  await expect(at(form, "course-form-field", "vocabulary.3.example_meaning")).toHaveCount(0);
  // (A word needs a term and a meaning, or the save is refused along with the settings.)
  await at(form, "course-form-field", "vocabulary.3.term").fill("la casa");
  await at(form, "course-form-field", "vocabulary.3.meaning").fill("house");

  // A lesson that has listening sentences and conversations keeps their sections.
  const all = page.getByTestId("course-lessons-row").filter({ has: page.locator(".lt-title", { hasText: "Everything at once" }) }).first();
  await all.getByTestId("course-lessons-expand").click();
  const allForm = all.getByTestId("course-form");
  await expect(allForm.getByTestId("course-form-layout")).toBeVisible();
  await expect(section(allForm, "listening")).toBeVisible();
  await expect(section(allForm, "conversations")).toBeVisible();

  // On again: the open forms are drawn again with everything. With JSON on the switch is gone and Line numbers are back.
  await page.getByTestId("course-lessons-gear").click();
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await expect(section(form, "listening")).toBeVisible();
  await expect(at(form, "course-form-field", "vocabulary.3.example_meaning")).toBeVisible();
  await page.getByTestId("course-lessons-json-switch").click();
  await expect(sw).toBeHidden();
  await expect(page.getByTestId("course-lessons-lines-switch")).toBeVisible();
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect.poll(async () => (await manifest()).language_features).toBe(true);
});

test("with the language features off, + Build is offered in a music course only (#1232)", async ({ page }) => {
  const id = await copyCourse(page, TEST_COURSE);
  const manifest = JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=course.json`)).json()).content);
  const addBuild = async (music: boolean) => {
    const m = { ...manifest, language_features: false };
    if (!music) delete m.music;
    const r = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path: "course.json", content: JSON.stringify(m, null, 2) }] } });
    expect(r.ok()).toBe(true);
    await page.goto(`/courses/${id}`);
    const row = page.getByTestId("course-lessons-row").filter({ has: page.locator(".lt-title", { hasText: "Multiple choice" }) }).first();
    await row.getByTestId("course-lessons-expand").click();
    const form = row.getByTestId("course-form");
    await expect(form.getByTestId("course-form-layout")).toBeVisible();
    await openSection(form, "sprint");
    await expect(form.locator('[data-testid="course-outline-add"][data-path="sprint.spots"]')).toBeVisible();
    return form.locator('[data-testid="course-outline-add"][data-path="sprint.builds"]');
  };
  await expect(await addBuild(true)).toBeVisible();
  await expect(await addBuild(false)).toHaveCount(0);
});

test("the gear's Levels switch holds the Placement test switch, which adds or takes out placement.json under the course settings (#1106)", async ({ page }) => {
  const id = await copyCourse(page, TEST_COURSE);
  const file = async (path: string) => {
    const r = await page.request.get(`/api/courses/${id}/file?path=${path}`);
    return r.ok() ? ((await r.json()).content as string) : null;
  };
  const row = page.getByTestId("course-lessons-placement");
  const levels = page.getByTestId("course-lessons-levels-switch");
  const placement = page.getByTestId("course-lessons-placement-switch");
  // The copy has levels and the test course's placement test: its row sits right under Course settings.
  await expect(row).toBeVisible();
  await expect(page.locator("#lt-list > li").nth(1)).toHaveAttribute("data-testid", "course-lessons-placement");
  await page.getByTestId("course-lessons-gear").click();
  await expect(page.getByTestId("course-lessons-pref-levels-group")).toBeVisible();
  // Two boxes alike: the editor's own settings first, Levels last.
  await expect(page.locator("#lt-prefs-dialog .lt-pref-group").first()).toHaveAttribute("data-testid", "course-lessons-pref-general-group");
  await expect(page.locator("#lt-prefs-dialog .lt-pref-group").last()).toHaveAttribute("data-testid", "course-lessons-pref-levels-group");
  await expect(levels).toHaveAttribute("aria-checked", "true");
  await expect(placement).toHaveAttribute("aria-checked", "true");
  // Placement test is about the course, not its language: there in Forms mode and with JSON on.
  await page.getByTestId("course-lessons-json-switch").click();
  await expect(placement).toBeVisible();
  await page.getByTestId("course-lessons-json-switch").click();
  // The (i) explains it.
  await page.getByTestId("course-lessons-placement-pref-info").click();
  await expect(page.getByTestId("course-lessons-placement-dialog")).toBeVisible();
  await page.getByTestId("course-lessons-placement-close").click();

  // Levels off: the Placement test switch goes with it, and so does placement.json.
  await levels.click();
  await expect(placement).toBeHidden();
  await expect(row).toBeHidden();
  await editorSaved(page);
  await expect.poll(() => file("placement.json")).toBeNull();
  expect(JSON.parse((await file("course.json"))!).levels).toBeUndefined();

  // Levels on again: Placement test shows, off until turned on.
  await levels.click();
  await expect(placement).toBeVisible();
  await expect(placement).toHaveAttribute("aria-checked", "false");
  await editorSaved(page);
  await expect.poll(async () => JSON.parse((await file("course.json"))!).levels).toBe(true);
  // On: a blank test, saved as placement.json, its row back under the settings.
  await placement.click();
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect(row).toBeVisible();
  await editorSaved(page);
  await expect.poll(async () => JSON.parse((await file("placement.json")) ?? "null")).toEqual({ random_per_unit: 3, questions: [] });

  // Its drawer edits the file as JSON; a written question that breaks the rules is refused at its row.
  await row.getByTestId("course-lessons-placement-expand").click();
  const json = row.getByTestId("course-lessons-placement-json");
  await expect(json).toBeVisible();
  await json.fill(JSON.stringify({ questions: [{ unit: "all", prompt: "Pick b", options: ["a", "b"], answer: "c" }] }, null, 2));
  await expect(row).toHaveAttribute("data-problem", "rules");
  await expect(row.getByTestId("course-lessons-placement-status")).toContainText("answer must be one of the options");
  await json.fill(JSON.stringify({ questions: [{ unit: "all", prompt: "Pick b", options: ["a", "b"], answer: "b" }] }, null, 2));
  await editorSaved(page);
  await expect.poll(async () => JSON.parse((await file("placement.json")) ?? "null")?.questions?.[0]?.answer).toBe("b");
  await expect(row).not.toHaveAttribute("data-problem");
});
