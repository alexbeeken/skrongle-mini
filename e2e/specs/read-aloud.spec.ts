// Exact spec for #234, Read aloud: Listen reads a lesson one sentence at a
// time in order, marks the sentence being read, pauses, and starts from a
// tapped paragraph; the speaker beside a quiz question reads the question and
// its choices; a listening round and a question with music to hear have no
// speaker; the audio switch off hides them all. Speech is stubbed: the fake
// voice keeps what it is asked to say and finishes an utterance only when the
// test says so (window.__next), so the order can be checked step by step.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, TEST_COURSE } from "../lib/app";

type Said = { text: string; lang: string };

async function fakeVoice(page: Page, langs = ["es", "en"]): Promise<void> {
  await page.addInitScript((langs: string[]) => {
    const w = window as unknown as { __said: Said[]; __next: () => void };
    const said: Said[] = [];
    let current: { onend: (() => void) | null } | null = null;
    w.__said = said;
    w.__next = () => { const u = current; current = null; u?.onend?.(); };
    class Utterance {
      lang = "";
      rate = 1;
      voice: unknown = null;
      onstart: (() => void) | null = null;
      onend: (() => void) | null = null;
      onerror: ((e: unknown) => void) | null = null;
      text: string;
      constructor(text: string) { this.text = text; }
    }
    const voices = langs.map((lang) => ({ lang, name: `Test ${lang}`, voiceURI: `test-${lang}`, default: false, localService: true }));
    const synth = {
      speaking: false, pending: false, paused: false,
      getVoices: () => voices,
      speak(u: Utterance) { said.push({ text: u.text, lang: u.lang }); current = u; setTimeout(() => u.onstart?.(), 0); },
      cancel() { current = null; }, pause() {}, resume() {}, addEventListener() {}, removeEventListener() {}, onvoiceschanged: null,
    };
    Object.defineProperty(window, "speechSynthesis", { value: synth, configurable: true });
    Object.defineProperty(window, "SpeechSynthesisUtterance", { value: Utterance, configurable: true });
  }, langs);
}

const said = (page: Page) => page.evaluate(() => (window as unknown as { __said: Said[] }).__said.slice());
const next = (page: Page) => page.evaluate(() => (window as unknown as { __next: () => void }).__next());

test("Listen reads the lesson sentence by sentence, pauses, and starts from a tapped paragraph", async ({ page }) => {
  await fakeVoice(page);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/lessons/multiple-choice");
  const listen = page.getByTestId("read-aloud-listen");
  await expect(listen).toBeVisible();
  await expect(listen).toHaveText("Listen");

  await listen.click();
  await expect(listen).toHaveText("Pause");
  // The title first, marked while it is read, in the explaining language.
  await expect.poll(() => said(page)).toEqual([{ text: "Multiple choice", lang: "en" }]);
  await expect(page.getByTestId("read-aloud-mark")).toHaveText("Multiple choice");
  const order = [
    { text: "Tap one of two to four options.", lang: "en" },
    { text: "Multiple choice", lang: "en" },
    { text: "Tap one option.", lang: "en" },
    { text: "The options are shuffled every time.", lang: "en" },
  ];
  for (const [i, s] of order.entries()) {
    await next(page);
    await expect.poll(async () => (await said(page)).length).toBe(i + 2);
    expect((await said(page))[i + 1]).toEqual(s);
  }
  await expect(page.getByTestId("read-aloud-mark")).toHaveText("The options are shuffled every time.");

  // Pressed again, it pauses: the mark goes and nothing more is said.
  await listen.click();
  await expect(listen).toHaveText("Resume");
  await expect(page.getByTestId("read-aloud-mark")).toHaveCount(0);
  const n = (await said(page)).length;
  await next(page);
  await page.waitForTimeout(200);
  expect((await said(page)).length).toBe(n);

  // Resume reads on from the sentence it stopped at.
  await listen.click();
  await expect.poll(async () => (await said(page)).at(-1)).toEqual({ text: "The options are shuffled every time.", lang: "en" });

  // A tap on a later paragraph starts reading there.
  await page.getByTestId("lesson-content").getByText("A question offers two options").click();
  await expect.poll(async () => (await said(page)).at(-1)).toEqual({ text: "A question offers two options or more; exactly one is right.", lang: "en" });

  // The vocabulary table is read row by row, each cell in its own language.
  for (let i = 0; i < 6 && !(await said(page)).some((s) => s.text === "el gato"); i++) await next(page);
  await expect.poll(() => said(page)).toContainEqual({ text: "el gato", lang: "es" });
  await next(page);
  await expect.poll(async () => (await said(page)).at(-1)).toEqual({ text: "cat", lang: "en" });
  await next(page);
  await expect.poll(async () => (await said(page)).at(-1)).toEqual({ text: "El gato duerme.", lang: "es" });
});

test("the speaker beside a quiz question reads it and its choices; listening has none", async ({ page }) => {
  await fakeVoice(page);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/practice?lesson=multiple-choice");
  const q = page.locator('[data-testid="quiz-question"]:visible');
  await expect(q).toHaveCount(1);
  const speaker = q.getByTestId("read-aloud-question");
  await expect(speaker).toBeVisible();
  const prompt = (await q.locator(".prompt").innerText()).trim();
  const options = await q.getByTestId("quiz-option").evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value));
  await speaker.click();
  // The prompt first, in the explaining language, then each choice in turn.
  await expect.poll(async () => (await said(page)).length).toBe(1);
  expect((await said(page))[0].lang).toBe("en");
  for (let i = 0; i < 3 + options.length; i++) await next(page);
  const got = await said(page);
  // A word in quotes is the course's own: "What does 'el gato' mean?" is three utterances.
  if (prompt.includes("'el gato'")) expect(got.slice(0, 3)).toEqual([{ text: "What does", lang: "en" }, { text: "el gato", lang: "es" }, { text: "mean?", lang: "en" }]);
  expect(got.slice(-options.length).map((s) => s.text)).toEqual(options);

  // Answering stops it.
  await page.evaluate(() => { (window as unknown as { __said: Said[] }).__said.length = 0; });
  await speaker.click();
  await expect.poll(async () => (await said(page)).length).toBe(1);
  await q.getByTestId("quiz-option").first().check();
  await next(page);
  await page.waitForTimeout(200);
  expect((await said(page)).length).toBe(1);

  // A question with music to hear has no speaker.
  await page.goto("/practice?lesson=ear");
  await expect(page.locator('[data-testid="quiz-question"]:visible')).toHaveCount(1);
  await expect(page.getByTestId("quiz-play").first()).toBeAttached();
  await expect(page.locator('[data-testid="quiz-question"]:visible').getByTestId("read-aloud-question")).toHaveCount(0);

  // A listening round has no speaker beside the sentence to type.
  await page.goto("/listening/play?lesson=listening");
  await expect(page.getByTestId("listen-setup")).toBeVisible();
  await expect(page.getByTestId("read-aloud-question")).toHaveCount(0);
});

test("the Guide's quick checks and Sprint mode's questions have the speaker too", async ({ page }) => {
  await fakeVoice(page);
  await devLogin(page, "open=" + TEST_COURSE);
  // The Guide: through the cards to the first quick check.
  await page.goto("/guide/everything");
  const step = page.locator('[data-testid="stage-step"][data-kind="question"]').first();
  const index = Number(await step.getAttribute("data-index"));
  for (let i = 0; i < index; i++) {
    const card = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
    await expect(card).toBeVisible();
    await card.getByTestId("stage-continue").click();
    await expect(card).toBeHidden();
  }
  await expect(step).toBeVisible();
  await step.getByTestId("read-aloud-question").click();
  await expect.poll(async () => (await said(page)).length).toBe(1);
  expect((await said(page))[0].lang).toBe("en");

  // Sprint mode: past the bites (Check moves on) to the first exercise of a kind.
  const stage = page.getByTestId("sprint-play-stage");
  const sprintTo = async (lesson: string, kind: RegExp) => {
    await page.goto(`/sprint/play?lesson=${lesson}`);
    for (let i = 0; i < 8; i++) {
      await expect(stage).toHaveAttribute("data-kind", /./);
      if (kind.test((await stage.getAttribute("data-kind"))!)) break;
      const pos = await stage.getAttribute("data-pos");
      await page.getByTestId("sprint-play-check").click();
      await expect(stage).not.toHaveAttribute("data-pos", pos!);
    }
    await expect(stage).toHaveAttribute("data-kind", kind);
  };
  // The first question (a choice) has it.
  await sprintTo("multiple-choice", /^choice$/);
  await expect(stage.getByTestId("read-aloud-question")).toHaveCount(1);
  // A listening exercise has none.
  await sprintTo("listening", /^hear-/);
  await expect(stage.getByTestId("read-aloud-question")).toHaveCount(0);
});

test("with the audio switch off there is no Listen button and no speaker", async ({ page }) => {
  await fakeVoice(page);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.evaluate(() => localStorage.setItem("speech:audio", "0"));
  await page.goto("/lessons/multiple-choice");
  await expect(page.getByTestId("lesson-content")).toBeVisible();
  await expect(page.getByTestId("read-aloud")).toBeHidden();
  await page.goto("/practice?lesson=multiple-choice");
  await expect(page.locator('[data-testid="quiz-question"]:visible')).toHaveCount(1);
  await expect(page.getByTestId("read-aloud-question")).toHaveCount(0);
});

test("without a voice for the explaining language there is no Listen button", async ({ page }) => {
  await fakeVoice(page, ["es"]);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/lessons/multiple-choice");
  await expect(page.getByTestId("lesson-content")).toBeVisible();
  await page.waitForTimeout(700); // past speech.js's second look at the voice list
  await expect(page.getByTestId("read-aloud")).toBeHidden();
});
