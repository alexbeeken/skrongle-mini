// Exact specs for #1200: the lesson editor's forms cannot put a lesson into
// a state the course rules refuse (src/content.rs validate_lesson). Each
// test walks one breaking path on a copy of the test course's "Everything at
// once" lesson and ends on a save the server accepts. Where the form cannot
// prevent a refusal (a field still being typed), a red check says what to do.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyCourse, devLogin, devReset, editorSaved, fastAutosave, TEST_COURSE } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await fastAutosave(page);
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  page.on("dialog", (d) => d.accept());
  await devLogin(page, `open=${TEST_COURSE}`);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const at = (scope: Page | Locator, testid: string, path: string) => scope.locator(`[data-testid="${testid}"][data-path="${path}"]`);
const check = (form: Locator, path: string) => form.locator(`.le-check[data-check="${path}"]`).first();
const item = (form: Locator, sec: string, idx: number, sub?: string) =>
  form.locator(`[data-testid="course-outline-item"][data-sec="${sec}"][data-idx="${idx}"]${sub ? `[data-sub="${sub}"]` : ""}`);

async function open(page: Page, sec: string) {
  const id = await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Everything at once" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  await expect(form.getByTestId("course-form-layout")).toBeVisible();
  const path = (await row.getAttribute("data-path"))!;
  const doc = async () => JSON.parse(await row.locator(".lt-json").evaluate((el: HTMLTextAreaElement) => el.value));
  const saved = async () => JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()).content);
  const head = form.locator(`[data-testid="course-outline-section"][data-sec="${sec}"]`);
  if ((await head.getAttribute("aria-expanded")) !== "true") await head.click();
  return { form, doc, saved };
}
/** Saved, and the editor says the course loads. */
async function loads(page: Page) {
  await editorSaved(page);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
}

test("a type switch to numeric or expression says when the checker cannot read the answer", async ({ page }) => {
  const { form, doc } = await open(page, "quiz");
  await item(form, "quiz", 2).click();
  const q = "quiz.2";
  const type = (t: string) => form.getByTestId("course-form-type").locator(`[data-value="${t}"]`).click();
  // A fill-in-blank answered "gracias", switched to numeric: the answer is no number.
  await type("numeric");
  await expect(check(form, `${q}.correct_answer`)).toHaveAttribute("data-level", "bad");
  await expect(check(form, `${q}.correct_answer`)).toContainText("can't read “gracias” as a number");
  await expect(item(form, "quiz", 2).locator(".le-warn")).toHaveCount(1);
  await at(form, "course-form-field", `${q}.correct_answer`).fill("4");
  await expect(check(form, `${q}.correct_answer`)).toBeHidden();
  // An accepted answer the checker cannot read is named too.
  await at(form, "course-form-add", `${q}.accepted_answers`).click();
  await expect(check(form, `${q}.accepted_answers`)).toContainText("empty");
  await at(form, "course-form-field", `${q}.accepted_answers.0`).fill("four");
  await expect(check(form, `${q}.accepted_answers`)).toContainText("can't read “four” as a number");
  await at(form, "course-form-field", `${q}.accepted_answers.0`).fill("2+2");
  await expect(check(form, `${q}.accepted_answers`)).toBeHidden();
  await loads(page);
  // Expression: "2x +" is no expression; "2x + 1" is.
  await type("expression");
  await at(form, "course-form-field", `${q}.correct_answer`).fill("2x +");
  await expect(check(form, `${q}.correct_answer`)).toContainText("can't read “2x +” as an expression");
  await at(form, "course-form-field", `${q}.correct_answer`).fill("2x + 1");
  await expect(check(form, `${q}.correct_answer`)).toBeHidden();
  await loads(page);
  // And back to fill-in-blank: no unit or tolerance left behind.
  await type("fill-in-blank");
  expect((await doc()).quiz[2]).toMatchObject({ type: "fill-in-blank", correct_answer: "2x + 1" });
  await loads(page);
});

test("deleting the option that is the answer leaves no answer to pick, and a question keeps two options", async ({ page }) => {
  const { form, doc } = await open(page, "quiz");
  await item(form, "quiz", 0).click();
  await at(form, "course-form-remove", "quiz.0.options.0").click();
  expect((await doc()).quiz[0]).toMatchObject({ options: ["adiós", "gracias"], correct_answer: "" });
  await expect(check(form, "quiz.0.options")).toContainText("Pick which option is right");
  // Two options left: neither ✕ works.
  await expect(at(form, "course-form-remove", "quiz.0.options.0")).toBeDisabled();
  await expect(at(form, "course-form-remove", "quiz.0.options.1")).toBeDisabled();
  await at(form, "course-form-correct", "quiz.0.options.0").check();
  expect((await doc()).quiz[0].correct_answer).toBe("adiós");
  await loads(page);
});

test("a deleted word leaves the questions and theme that named it; a deleted question leaves the Guide", async ({ page }) => {
  const { form, doc, saved } = await open(page, "vocabulary");
  // "hola" is taught by question 1 and re-exampled by the realm theme.
  await item(form, "vocabulary", 0).click();
  await at(form, "course-form-remove", "vocabulary.0").click();
  const d = await doc();
  expect(d.vocabulary.map((v: { term: string }) => v.term)).not.toContain("hola");
  expect(d.quiz[0].teaches).toBeUndefined();
  expect(d.themes.realm.vocabulary).toEqual([]);
  await loads(page);
  // Question 1 is a step of the hand-ordered Guide; deleting it takes the step out.
  const head = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
  if ((await head.getAttribute("aria-expanded")) !== "true") await head.click();
  await item(form, "quiz", 0).click();
  await at(form, "course-form-remove", "quiz.0").click();
  expect((await doc()).guide.some((s: { question?: string }) => s.question === "tc-everything-01")).toBe(false);
  await expect(form.locator('[data-testid="course-outline-section"][data-sec="guide"] .le-warn')).toHaveCount(0);
  await loads(page);
  expect((await saved()).guide).toContainEqual({ question: "tc-everything-02" });
});

test("a word typed through another word's term keeps its questions (#1248)", async ({ page }) => {
  const { form, doc, saved } = await open(page, "vocabulary");
  await form.locator('[data-testid="course-outline-add"][data-path="vocabulary"]').click();
  await at(form, "course-form-field", "vocabulary.6.term").fill("habla");
  await at(form, "course-form-field", "vocabulary.6.meaning").fill("speaks");
  await loads(page);
  // "hablar" (question 6 teaches it) passes through "habla" on a Backspace.
  await item(form, "vocabulary", 4).click();
  const term = at(form, "course-form-field", "vocabulary.4.term");
  await term.click();
  await term.press("End");
  await term.press("Backspace");
  await term.pressSequentially("r");
  await expect(term).toHaveValue("hablar");
  expect((await doc()).quiz[5].teaches).toEqual(["hablar"]);
  // Two Backspaces: "habl" is its own, "habla" clashes, "hablar" follows from "habl".
  await term.press("Backspace");
  await term.press("Backspace");
  expect((await doc()).quiz[5].teaches).toEqual(["habl"]);
  await term.pressSequentially("ar");
  expect((await doc()).quiz[5].teaches).toEqual(["hablar"]);
  // A rename typed over another word's name and back: "hola" -> "dos" -> "hola".
  await item(form, "vocabulary", 0).click();
  await at(form, "course-form-field", "vocabulary.0.term").fill("dos");
  await at(form, "course-form-field", "vocabulary.0.term").fill("hola");
  expect((await doc()).quiz[0].teaches).toEqual(["hola"]);
  await loads(page);
  const s = await saved();
  expect(s.quiz[0].teaches).toEqual(["hola"]);
  expect(s.quiz[5].teaches).toEqual(["hablar"]);
});

test("a word left on another word's term gets its questions back when the other word is renamed (#1248)", async ({ page }) => {
  const { form, doc } = await open(page, "vocabulary");
  await form.locator('[data-testid="course-outline-add"][data-path="vocabulary"]').click();
  await at(form, "course-form-field", "vocabulary.6.term").fill("habla");
  await at(form, "course-form-field", "vocabulary.6.meaning").fill("speaks");
  await loads(page);
  // "hablar" stops on "habla": question 6 keeps "hablar" while the terms clash.
  await item(form, "vocabulary", 4).click();
  const term = at(form, "course-form-field", "vocabulary.4.term");
  await term.click();
  await term.press("End");
  await term.press("Backspace");
  expect((await doc()).quiz[5].teaches).toEqual(["hablar"]);
  // The added word is renamed instead: the clash is over, question 6 follows "habla".
  await item(form, "vocabulary", 6).click();
  const other = at(form, "course-form-field", "vocabulary.6.term");
  await other.click();
  await other.press("End");
  await other.press("Backspace");
  await other.pressSequentially("o");
  expect((await doc()).quiz[5].teaches).toEqual(["habla"]);
  await loads(page);
});

test("a duplicated word, sentence or conjugation says to change one; a duplicated question gets its own id", async ({ page }) => {
  const { form, doc } = await open(page, "vocabulary");
  await item(form, "vocabulary", 1).click();
  await form.getByTestId("course-form-duplicate").click();
  await expect(check(form, "vocabulary.2.term")).toContainText("Another word has this term. Change one of them");
  await at(form, "course-form-field", "vocabulary.2.term").fill("hasta luego");
  await at(form, "course-form-field", "vocabulary.2.meaning").fill("see you later");
  await expect(check(form, "vocabulary.2.term")).toHaveAttribute("data-level", "ok");
  // A question: the copy gets a new id, nothing to fix.
  await form.locator('[data-testid="course-outline-section"][data-sec="quiz"]').click();
  await item(form, "quiz", 1).click();
  await form.getByTestId("course-form-duplicate").click();
  const ids = (await doc()).quiz.map((q: { question_id: string }) => q.question_id);
  expect(new Set(ids).size).toBe(ids.length);
  // A conjugation: the same verb in the same set clashes until one changes.
  // (Not "present": the course's "Verb forms" lesson has hablar there, a
  // clash the save refusal names, since the form sees only this lesson.)
  await form.locator('[data-testid="course-outline-section"][data-sec="conjugations"]').click();
  await item(form, "conjugations", 0).click();
  await form.getByTestId("course-form-duplicate").click();
  await expect(check(form, "conjugations.1.item")).toContainText("“hablar” is already conjugated in this set");
  await at(form, "course-form-field", "conjugations.1.item").fill("charlar");
  await expect(item(form, "conjugations", 1).locator(".le-warn")).toHaveCount(0);
  await loads(page);
  // A new conjugation starts in the paradigm's first set, not in none.
  await form.locator('[data-testid="course-outline-add"][data-path="conjugations"]').click();
  expect((await doc()).conjugations[2].set).toBe("present");
});

test("a sort keeps 2 to 4 buckets and 3 to 10 items; a deleted bucket's items move to the one before", async ({ page }) => {
  const { form, doc } = await open(page, "sprint");
  await item(form, "sprint", 0, "sorts").click();
  const s = "sprint.sorts.0";
  await expect(at(form, "course-form-remove-bucket", `${s}.buckets.0`)).toBeDisabled();
  await form.getByTestId("course-form-add-bucket").click();
  await at(form, "course-form-field", `${s}.buckets.2`).fill("neither");
  await at(form, "course-form-bucket-next", `${s}.items.0`).click();
  await at(form, "course-form-bucket-next", `${s}.items.0`).click();
  expect((await doc()).sprint.sorts[0].items[0].bucket).toBe(2);
  await at(form, "course-form-remove-bucket", `${s}.buckets.2`).click();
  expect((await doc()).sprint.sorts[0]).toMatchObject({ buckets: ["arriving", "leaving"] });
  expect((await doc()).sprint.sorts[0].items[0].bucket).toBe(1);
  await expect(at(form, "course-form-remove-bucket", `${s}.buckets.1`)).toBeDisabled();
  // Four items: one can go, then no ✕ until there are more.
  await at(form, "course-form-remove", `${s}.items.3`).click();
  await expect(at(form, "course-form-remove", `${s}.items.0`)).toBeDisabled();
  // Up to ten.
  for (let i = 3; i < 10; i++) {
    await form.locator(`[data-testid="course-form-add-sort-item"][data-bucket="0"]`).click();
    await at(form, "course-form-field", `${s}.items.${i}.text`).fill(`palabra ${i}`);
  }
  await expect(form.getByTestId("course-form-add-sort-item").first()).toBeDisabled();
  await expect(check(form, `${s}.items`)).toHaveAttribute("data-level", "ok");
  await loads(page);
});

test("a spot's wrong word: one the sentence has twice cannot be picked, and punctuation round a word does not stop it", async ({ page }) => {
  const { form, doc } = await open(page, "sprint");
  await item(form, "sprint", 0, "spots").click();
  const s = "sprint.spots.0";
  await at(form, "course-form-field", `${s}.sentence`).fill("Muchas muchas grasias (Ana).");
  const words = form.getByTestId("course-form-words");
  await expect(words.locator('[data-value="Muchas"]')).toBeDisabled();
  await expect(words.locator('[data-value="muchas"]')).toBeDisabled();
  const faded = await words.locator('[data-value="Muchas"]').evaluate((b) => Number(getComputedStyle(b).opacity));
  expect(faded).toBeLessThan(1);
  expect(await words.locator('[data-value="grasias"]').evaluate((b) => Number(getComputedStyle(b).opacity))).toBe(1);
  await words.locator('[data-value="(Ana)."]').click();
  expect((await doc()).sprint.spots[0].wrong).toBe("(Ana)");
  // The picked word keeps its red border under the mouse (the hover rule must not outrank it).
  const border = (v: string) => words.locator(`[data-value="${v}"]`).evaluate((b) => getComputedStyle(b).borderTopColor);
  await words.locator('[data-value="(Ana)."]').hover();
  const hovered = await border("(Ana).");
  await page.mouse.move(0, 0);
  expect(hovered).toBe(await border("(Ana)."));
  await expect(check(form, `${s}.wrong`)).toHaveAttribute("data-level", "ok");
  await words.locator('[data-value="grasias"]').click();
  await loads(page);
});

test("a conversation keeps two lines and says when one is empty", async ({ page }) => {
  const { form } = await open(page, "conversations");
  await item(form, "conversations", 0).click();
  const c = "conversations.0";
  await at(form, "course-form-field", `${c}.lines.2.text`).fill("");
  await expect(check(form, `${c}.lines`)).toContainText("A line has no text");
  await at(form, "course-form-remove", `${c}.lines.2`).click();
  await expect(check(form, `${c}.lines`)).toHaveAttribute("data-level", "ok");
  await expect(at(form, "course-form-remove", `${c}.lines.0`)).toBeDisabled();
  await expect(at(form, "course-form-remove", `${c}.lines.1`)).toBeDisabled();
  await loads(page);
});

test("a Guide's word count is a whole number, and an optional box of spaces is left out", async ({ page }) => {
  const { form, doc } = await open(page, "guide");
  await form.locator('[data-testid="course-outline-section"][data-sec="guide"]').click();
  await form.locator('[data-testid="course-form-step"][data-path="guide.1"]').click();
  await at(form, "course-form-field", "guide.1.vocab").fill("2.5");
  await expect(check(form, "guide.1")).toContainText("A whole number of words");
  await at(form, "course-form-field", "guide.1.vocab").fill("2");
  await expect(check(form, "guide.1")).toBeHidden();
  await form.locator('[data-testid="course-outline-section"][data-sec="lesson"]').click();
  await at(form, "course-form-field", "group").fill("   ");
  expect((await doc()).group).toBeUndefined();
  await loads(page);
});
