// Find it on the keys (#1231): tap the key of a keyboard question. Each key
// carries its pitch (`data-midi`); answers in the course file are ABC notes.
import { expect, type Locator } from "@playwright/test";

const STEP: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
const ACC: Record<string, number> = { "": 0, "=": 0, "^": 1, "^^": 2, _: -1, __: -2 };

/** An ABC note as a MIDI number (C = middle C = 60), as src/notation/keyboard.rs reads it. */
export function midiOf(abc: string): number {
  const m = /^(\^\^|\^|__|_|=)?([A-Ga-g])([,']*)$/.exec(abc.trim());
  if (!m) throw new Error(`"${abc}" is not one ABC note`);
  let n = 60 + STEP[m[2].toUpperCase()] + ACC[m[1] ?? ""] + (m[2] === m[2].toLowerCase() ? 12 : 0);
  for (const c of m[3]) n += c === "'" ? 12 : -12;
  return n;
}

/**
 * Tap a key of the keyboard `box`: one of `answers` (ABC notes; any octave
 * unless `exact`), or with `wrong` a key of another note. Returns the key
 * tapped, as the page posts it (`data-note`). It doesn't submit.
 */
export async function pressKey(box: Locator, answers: string[], exact: boolean, wrong = false): Promise<string> {
  const keys = box.getByTestId("keyboard-key");
  await expect(keys.first()).toBeVisible();
  const midis = (await keys.evaluateAll((els) => els.map((e) => Number(e.getAttribute("data-midi"))))) as number[];
  const want = answers.filter((a) => a.trim()).map(midiOf);
  const right = (m: number) => want.some((w) => (exact ? w === m : (((w - m) % 12) + 12) % 12 === 0));
  const i = midis.findIndex((m) => right(m) !== wrong);
  if (i < 0) throw new Error(`keyboard: no ${wrong ? "wrong" : "right"} key for ${JSON.stringify(answers)} among ${JSON.stringify(midis)}`);
  await keys.nth(i).click();
  const note = (await keys.nth(i).getAttribute("data-note")) ?? "";
  await expect(box).toHaveAttribute("data-chosen", note);
  return note;
}
