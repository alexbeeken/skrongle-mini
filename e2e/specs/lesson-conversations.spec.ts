// Exact spec for #989: a lesson's conversations show on its page as two
// characters talking, each line under its speaker's name and portrait, A on
// the left and B on the right. A lesson without conversations shows none.
// #1264: a line's meaning starts hidden; tapping the bubble shows it and
// reads the line aloud in the course's language.
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { devLogin, TEST_COURSE } from "../lib/app";

type Line = { speaker: "a" | "b"; text: string; meaning?: string };
type Conversation = { id: string; title: string; role_a: string; role_b: string; lines: Line[] };
const lesson = JSON.parse(
  readFileSync(new URL("../../data/test-courses/test-course/lessons/2-exercises/conversation.json", import.meta.url), "utf8"),
) as { slug: string; conversations: Conversation[] };

test("a lesson with a conversation shows both speakers' lines", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto(`/lessons/${lesson.slug}`);
  const conv = lesson.conversations[0];
  const box = page.locator(`[data-testid="lesson-conversation"][data-conversation-id="${conv.id}"]`);
  await expect(box.getByRole("heading", { name: conv.title })).toBeVisible();
  const lines = box.getByTestId("lesson-conversation-line");
  await expect(lines).toHaveCount(conv.lines.length);
  for (const [i, l] of conv.lines.entries()) {
    const row = lines.nth(i);
    await expect(row).toHaveAttribute("data-speaker", l.speaker);
    await expect(row.getByTestId("lesson-conversation-text")).toHaveText(l.text);
    await expect(row.getByTestId("lesson-conversation-speaker")).toHaveText(l.speaker === "a" ? conv.role_a : conv.role_b);
    await expect(row.locator(".avatar")).toBeVisible();
  }
  // Both roles speak, with two different portraits, B's lines on the right.
  const a = lines.and(page.locator('[data-speaker="a"]')).first();
  const b = lines.and(page.locator('[data-speaker="b"]')).first();
  const portrait = (row: typeof a) => row.locator(".avatar use").getAttribute("href");
  expect(await portrait(a)).not.toEqual(await portrait(b));
  const ax = (await a.locator(".avatar").boundingBox())!.x;
  const bx = (await b.locator(".avatar").boundingBox())!.x;
  expect(bx).toBeGreaterThan(ax);

  // A lesson without conversations has no conversation block.
  await page.goto("/lessons/true-false");
  await expect(page.getByTestId("lesson-content")).toBeVisible();
  await expect(page.getByTestId("lesson-conversations")).toHaveCount(0);
});

test("tapping a conversation line shows its meaning and reads it aloud", async ({ page }) => {
  // A fake voice that keeps what it was asked to say.
  await page.addInitScript(() => {
    const said: { text: string; lang: string }[] = [];
    (window as unknown as { __said: typeof said }).__said = said;
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
    const voices = ["es", "en"].map((lang) => ({ lang, name: `Test ${lang}`, voiceURI: `test-${lang}`, default: false, localService: true }));
    const synth = {
      speaking: false, pending: false, paused: false,
      getVoices: () => voices,
      speak(u: Utterance) { said.push({ text: u.text, lang: u.lang }); setTimeout(() => { u.onstart?.(); u.onend?.(); }, 0); },
      cancel() {}, pause() {}, resume() {}, addEventListener() {}, removeEventListener() {}, onvoiceschanged: null,
    };
    Object.defineProperty(window, "speechSynthesis", { value: synth, configurable: true });
    Object.defineProperty(window, "SpeechSynthesisUtterance", { value: Utterance, configurable: true });
  });
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto(`/lessons/${lesson.slug}`);
  const conv = lesson.conversations[0];
  const l = conv.lines[0];
  const row = page.locator(`[data-testid="lesson-conversation"][data-conversation-id="${conv.id}"]`).getByTestId("lesson-conversation-line").first();
  const bubble = row.getByTestId("lesson-conversation-bubble");
  const meaning = row.getByTestId("lesson-conversation-meaning");
  await expect(meaning).toBeHidden();
  await expect(bubble).toHaveAttribute("aria-expanded", "false");

  await bubble.click();
  await expect(meaning).toBeVisible();
  await expect(meaning).toHaveText(l.meaning!);
  await expect(bubble).toHaveAttribute("aria-expanded", "true");
  await expect.poll(() => page.evaluate(() => (window as unknown as { __said: { text: string; lang: string }[] }).__said)).toContainEqual({ text: l.text, lang: "es" });

  // A second tap hides the meaning again and reads the line once more.
  await bubble.click();
  await expect(meaning).toBeHidden();
  await expect.poll(() => page.evaluate(() => (window as unknown as { __said: unknown[] }).__said.length)).toBe(2);

  // With the audio switch off, a tap still shows the meaning but says nothing.
  await page.evaluate(() => { localStorage.setItem("speech:audio", "0"); (window as unknown as { __said: unknown[] }).__said.length = 0; });
  await bubble.click();
  await expect(meaning).toBeVisible();
  await page.waitForTimeout(300);
  expect(await page.evaluate(() => (window as unknown as { __said: unknown[] }).__said.length)).toBe(0);
});
