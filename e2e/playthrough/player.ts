// The generic player: Jev works through whatever exercise page is open, one
// action at a time, until the mode's `done` check holds. Nothing here knows
// a mode or a course. It reads `<main>` the way the navigator does (the
// conventions in docs/testing.md, "Markup hooks": testids, aria-pressed,
// data-state, role=status feedback), offers every enabled button, option
// and box as a Choice, and performs the pick. Jev can't write, so a text box
// is filled word by word from a pool: the words of the lesson the learner
// read (see `readLesson`) and of the question itself. Jev has the lesson's
// text in front of it throughout, as a learner who just read it would; that
// is measured by `scripts/jev-playthrough.sh calibrate` (calibrate.ts).
import type { Page } from "@playwright/test";
import { choose } from "../lib/jev.ts";

/** One part of a lesson in a mode: a page to open and a goal to reach there. */
export interface Phase {
  name: string;
  url: string;
  /** What the learner is doing, for Jev. */
  goal: string;
  /** The exact check that the phase is over (a results screen, a done flag). */
  done: (page: Page) => Promise<boolean>;
  maxSteps?: number;
}

/** What a mode tells the player beyond the phase. */
export interface PlayerOptions {
  /** Testids never offered (they leave the session or start something else). */
  avoid: string[];
  /** CSS selectors never offered (speakers, "type it instead"). */
  skip: string[];
  /** Mode-specific advice appended to the instructions. */
  advice: string;
  /** The lesson the learner has read: its text, for every choice, and its words, for typed answers. */
  lesson: LessonRead;
  /** Every question put to Jev, for the report's traces. */
  trace?: (t: Trace) => void;
  /** Called with each page URL the first time the player sees it (page checks). */
  onPage?: (url: string) => Promise<void>;
}

export interface Graded {
  phase: string;
  question: string; // the question the answer was for, shortened
  actions: string[]; // what Jev did since the previous graded answer, each with its confidence
  right: boolean;
  feedback: string;
}

export interface LessonRead {
  text: string; // the lesson's own text (content, terms, tables), shortened
  words: string[]; // its words in the course's language
  lang: string; // the course's language code ("" when the page doesn't say)
}

/** One question put to Jev and its answer: what a report needs to tell Jev's slips from ours. */
export interface Trace {
  phase: string;
  kind: "step" | "word";
  state: Record<string, unknown>;
  options: Record<string, string>;
  choice: string;
  picked: string;
  confidence: number;
  runners_up: string[]; // the next most likely options with their probabilities
}

export interface PhaseResult {
  phase: string;
  outcome: "done" | "stuck" | "out of steps" | "error";
  note?: string;
  steps: number;
  graded: Graded[];
  log: string[]; // the last actions, for a report
}

/** Always left alone: speakers, accent helpers, sound toggles. A word pool has the accents. */
const SKIP = ['[aria-label="Hear it"]', '[aria-label="Play"]', '[aria-label="Play slowly"]', '[aria-label^="Type "]', '[aria-label^="Toggle "]', "[data-say]"];
const MAX_TEXT = 6_000;
const SETTLE_MS = 150;
const STUCK_AFTER = 8;
const MAX_WORDS = 6;

interface Item {
  ref: string;
  kind: "click" | "check" | "fill" | "select";
  label: string;
  context: string; // the question it belongs to
  prompt: string; // its first line
  lang: string; // a box's language (lang attribute), for typed answers
  math: boolean; // a box for a number or a formula (data-math): typed from symbols, not words
  options: string[];
}

async function readMain(page: Page, avoid: string[], skip: string[]) {
  return page.evaluate(
    ({ avoid, skip, maxText }) => {
      const main = (document.querySelector("main") ?? document.body) as HTMLElement;
      const visible = (el: Element) => el.checkVisibility({ visibilityProperty: true, opacityProperty: true }) && (el as HTMLElement).getBoundingClientRect().width > 0;
      const line = (s: string, n = 100) => s.replace(/\s+/g, " ").trim().slice(0, n);
      document.querySelectorAll("[data-pt]").forEach((el) => el.removeAttribute("data-pt"));
      const items: { ref: string; kind: string; label: string; context: string; prompt: string; lang: string; math: boolean; options: string[] }[] = [];
      // The whole page (a session's Check button may sit in a footer), but not the site's own
      // header and footer (nav-*, footer-*). Links lead out of the session; the phase's URL is the way in.
      for (const el of Array.from(document.body.querySelectorAll("button, input, select, textarea, [role=button], details > summary"))) {
        const h = el as HTMLInputElement;
        if (!visible(el) || h.disabled || h.type === "hidden" || h.type === "file") continue;
        const testid = el.closest("[data-testid]")?.getAttribute("data-testid") ?? "";
        if (avoid.includes(testid) || /^(nav|footer)-/.test(testid) || el.closest("[data-testid^=nav-], [data-testid^=footer-]")) continue;
        if (skip.some((s) => el.matches(s) || el.closest(s))) continue;
        const state = el.getAttribute("data-state") ?? el.closest("label")?.getAttribute("data-state") ?? "";
        if (["used", "done"].includes(state) || el.getAttribute("aria-disabled") === "true") continue;
        if ((h.type === "radio" || h.type === "checkbox") && h.checked) continue;
        // An open hint has said what it has to say; closing it again is a way to loop.
        if (el.tagName === "SUMMARY" && (el.parentElement as HTMLDetailsElement).open) continue;
        const ref = String(items.length);
        el.setAttribute("data-pt", ref);
        // A button's name: its aria-label, the value it stands for, its heading (a sort bucket), its text.
        let label = el.getAttribute("aria-label") || (el as HTMLElement).dataset.value || (el.querySelector("strong") as HTMLElement | null)?.innerText || (h.labels?.[0]?.innerText ?? "") || (el as HTMLElement).innerText || h.placeholder || "";
        label = line(label);
        const flags = [
          (el.getAttribute("aria-pressed") === "true" || el.getAttribute("aria-selected") === "true") && "selected",
          (state === "right" || state === "correct") && "marked right",
          state === "wrong" && "marked wrong",
        ].filter(Boolean);
        if (flags.length) label += ` [${flags.join(", ")}]`;
        const textual = el.tagName === "TEXTAREA" || (el.tagName === "INPUT" && !["checkbox", "radio", "submit", "button", "range"].includes(h.type));
        const kind = textual ? "fill" : el.tagName === "SELECT" ? "select" : h.type === "radio" || h.type === "checkbox" ? "check" : "click";
        const q = el.closest("form, [data-testid$=question], [data-testid$=stage], article, section") as HTMLElement | null;
        items.push({
          ref,
          kind,
          label: label || kind,
          context: q ? line(q.innerText, 600) : "",
          prompt: q ? line(q.innerText.split("\n").find((l) => l.trim().length > 3) ?? "", 90) : "",
          lang: (el.closest("[lang]") as HTMLElement | null)?.lang ?? "",
          math: el.getAttribute("data-math") === "1",
          options: el.tagName === "SELECT" ? Array.from((el as HTMLSelectElement).options).map((o) => o.value).slice(0, 12) : [],
        });
      }
      // Graded feedback: a role=status that says ok or bad, reported once per text.
      const graded: { right: boolean; feedback: string }[] = [];
      for (const st of Array.from(main.querySelectorAll("[role=status]"))) {
        const cls = st.classList;
        if (!visible(st) || !(cls.contains("ok") || cls.contains("bad"))) continue;
        // Its own text, without the Continue button some result sheets carry.
        let own = (st as HTMLElement).innerText;
        for (const b of Array.from(st.querySelectorAll("button"))) own = own.replace((b as HTMLElement).innerText, "");
        const text = line(own, 400);
        if (st.getAttribute("data-pt-graded") === text) continue;
        st.setAttribute("data-pt-graded", text);
        graded.push({ right: cls.contains("ok"), feedback: text });
      }
      let text = main.checkVisibility() ? main.innerText.replace(/\n{3,}/g, "\n\n") : "";
      // Long pages (a lesson and its check): the start and the end, where the questions are.
      if (text.length > maxText) text = text.slice(0, maxText / 2) + "\n…\n" + text.slice(-maxText / 2);
      return { text, items, graded };
    },
    { avoid, skip, maxText: MAX_TEXT },
  ) as Promise<{ text: string; items: Item[]; graded: { right: boolean; feedback: string }[] }>;
}

/** The lesson page as a learner reads it: its text, and its words in the course's language. */
export async function readLesson(page: Page, url: string): Promise<LessonRead> {
  await page.goto(url);
  return page.evaluate(() => {
    const main = (document.querySelector("main") ?? document.body) as HTMLElement;
    // The lesson itself, not its reading check (that is the test).
    const parts = Array.from(main.querySelectorAll("[data-testid=lesson-content], [data-testid=lesson-vocabulary], [data-testid=lesson-conjugations]")) as HTMLElement[];
    const text = (parts.length ? parts.map((e) => e.innerText).join("\n\n") : main.innerText).replace(/\n{3,}/g, "\n\n").slice(0, 5000);
    const lang = document.querySelector("[data-lang]")?.getAttribute("data-lang") || "";
    const tagged = Array.from(main.querySelectorAll(lang ? `[lang="${lang}"]` : "[lang]"));
    const texts = tagged.length ? tagged.map((e) => (e as HTMLElement).innerText) : [main.innerText];
    const seen = new Map<string, string>();
    for (const t of texts)
      for (const w of t.split(/\s+/)) {
        const word = w.replace(/^[¿¡"'«(\[—–-]+|[.,;:!?"'»)\]—–…]+$/g, "");
        if (word && word.length < 30 && !seen.has(word.toLowerCase())) seen.set(word.toLowerCase(), word);
      }
    return { text, words: Array.from(seen.values()), lang };
  });
}

const words = (s: string) => s.split(/\s+/).map((w) => w.replace(/^[¿¡"'«(\[]+|[.,;:!?"'»)\]…]+$/g, "")).filter((w) => w && w !== "___" && w.length < 30);

const languageName = (code: string) => {
  try {
    return (code && new Intl.DisplayNames(["en"], { type: "language" }).of(code)) || "";
  } catch {
    return "";
  }
};

/**
 * Jev types an answer to `question` one word at a time: the lesson's words
 * first, then the question's own (a gap's neighbours, a name). It is told the
 * box's language, so a "Translate" question isn't answered by copying it.
 */
export async function compose(
  q: { question: string; box: string; lang: string; math?: boolean },
  goal: string,
  lesson: LessonRead,
  trace?: (t: Omit<Trace, "phase">) => void,
): Promise<string> {
  const seen = new Set<string>();
  // A number or a formula (data-math) is typed from symbols: digits, operators, and the
  // numbers and letters of the question and the lesson; the pieces are joined without spaces.
  const pool = q.math
    ? [...MATH, ...mathTokens(q.question), ...mathTokens(lesson.text)].filter((w) => !seen.has(w) && seen.add(w)).slice(0, 120)
    : [...lesson.words, ...words(q.question)].filter((w) => !seen.has(w.toLowerCase()) && seen.add(w.toLowerCase())).slice(0, 240);
  const language = q.math ? "" : languageName(q.lang);
  const typed: string[] = [];
  while (typed.length < (q.math ? MAX_SYMBOLS : MAX_WORDS)) {
    const options: Record<string, string> = Object.fromEntries(pool.map((w, i) => [`w${i}`, w]));
    if (typed.length) options.end = "(the answer is complete)";
    const state = {
      goal,
      lesson_you_read: lesson.text,
      question: q.question,
      box: q.box,
      ...(language && { answer_language: language }),
      typed_so_far: typed.join(q.math ? "" : " ") || "(nothing)",
    };
    const res = await choose(
      state,
      (q.math
        ? "You are typing your answer to `question` into the box, a number or a formula, one piece at a time (digits, operators, letters). " +
          "Choose the next piece of the right answer, or the end once `typed_so_far` is the whole answer. Work the answer out first; `lesson_you_read` teaches what the question asks."
        : "You are typing your answer to `question` into the box, one word at a time. Choose the next word of the right answer, " +
          "or choose the end once `typed_so_far` is the whole answer. Most answers are one or two words. `lesson_you_read` teaches what the question asks.") +
        (language ? " The answer is in `answer_language`: never copy the question's own words in another language; a 'Translate' question wants the answer_language." : ""),
      options,
    );
    trace?.({ kind: "word", state, options, choice: res.choice, picked: options[res.choice] ?? res.choice, confidence: round(res.confidence), runners_up: runnersUp(res.probabilities, options, res.choice) });
    if (res.choice === "end" || !(res.choice in options)) break;
    typed.push(options[res.choice]);
  }
  return typed.join(q.math ? "" : " ");
}

const MAX_SYMBOLS = 14;
const MATH = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ".", "/", "-", "+", "*", "^", "(", ")", "x", "; ", " "];
/** Numbers (12, 0.75, 3/4) and one-letter variables in `s`. */
const mathTokens = (s: string) => s.match(/\d+(?:[./]\d+)?|\b[a-z]\b/g) ?? [];

const round = (x: number) => Math.round(x * 100) / 100;
function runnersUp(probs: Record<string, number>, options: Record<string, string>, choice: string): string[] {
  return Object.entries(probs)
    .filter(([k]) => k !== choice && k in options)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([k, p]) => `${options[k]} (${round(p)})`);
}

/** Play `phase` on `page` until it's done, stuck, or out of steps. Never throws for a failed phase. */
export async function play(page: Page, phase: Phase, opts: PlayerOptions): Promise<PhaseResult> {
  const r: PhaseResult = { phase: phase.name, outcome: "out of steps", steps: 0, graded: [], log: [] };
  const log = (s: string) => r.log.push(s) > 30 && r.log.shift();
  const avoid = opts.avoid, skip = [...SKIP, ...opts.skip];
  await page.goto(phase.url);
  await opts.onPage?.(page.url());
  const useless = new Set<string>(); // "page action" pairs that changed nothing
  let lastSig = "", sameFor = 0, lastAction = "", question = "";
  let since: string[] = [];
  const maxSteps = phase.maxSteps ?? 400;
  try {
    for (; r.steps < maxSteps; r.steps++) {
      const finished = await phase.done(page);
      const seen = await readMain(page, avoid, skip);
      // The last page's grading too (a reading check's results are on the page that ends the phase).
      for (const g of seen.graded) {
        r.graded.push({ phase: phase.name, question: question.slice(0, 300), actions: since, right: g.right, feedback: g.feedback });
        since = [];
      }
      if (finished) {
        r.outcome = "done";
        return r;
      }
      const sig = JSON.stringify([page.url(), seen.text, seen.items.map((i) => i.label)]);
      if (sig === lastSig) {
        sameFor += 1;
        if (lastAction) useless.add(`${sig} ${lastAction}`);
      } else sameFor = 0;
      lastSig = sig;
      if (sameFor >= STUCK_AFTER) {
        r.outcome = "stuck";
        r.note = `the page did not change for ${STUCK_AFTER} steps`;
        return r;
      }
      const offered = seen.items.filter((i) => !useless.has(`${sig} ${describe(i)}`));
      if (!offered.length) {
        lastAction = "";
        await page.waitForTimeout(250); // an animation or a request in flight
        continue;
      }
      let pick = offered[0], confidence = 1;
      if (offered.length > 1) {
        // Several questions on one page (a reading check): say which one each answer is for.
        const multi = new Set(offered.filter((i) => i.kind !== "click" && i.context).map((i) => i.context)).size > 1;
        const options = Object.fromEntries(
          offered.map((i) => [`a${i.ref}`, describe(i) + (multi && i.kind !== "click" ? `, for the question "${i.prompt}"` : "")]),
        );
        const state = { goal: phase.goal, lesson_you_read: opts.lesson.text, page_text: seen.text, your_last_actions: r.log.slice(-6) };
        const res = await choose(
          state,
          "You are a learner doing this lesson and you want every answer right. Choose the next action toward the `goal`. " +
            "Answer a question (pick an option or type into its box), then press its Check or submit button; press Continue or Next to move on. " +
            "Don't pick what is already [selected]: submit instead. You can't hear audio: skip a listening exercise when you can. " +
            opts.advice,
          options,
        );
        pick = offered.find((i) => `a${i.ref}` === res.choice) ?? pick;
        confidence = round(res.confidence);
        opts.trace?.({ phase: phase.name, kind: "step", state, options, choice: res.choice, picked: options[res.choice] ?? res.choice, confidence, runners_up: runnersUp(res.probabilities, options, res.choice) });
      }
      lastAction = describe(pick);
      if (pick.context) question = pick.context;
      const el = page.locator(`[data-pt="${pick.ref}"]`);
      if (pick.kind === "fill") {
        const text = await compose({ question: pick.context, box: pick.label, lang: pick.lang, math: pick.math }, phase.goal, opts.lesson, (t) => opts.trace?.({ phase: phase.name, ...t }));
        lastAction += ` "${text}"`;
        await el.fill(text, { timeout: 2_000 }).catch(() => {});
        useless.add(`${sig} ${describe(pick)}`); // typed: don't offer the same box again on this page
      } else if (pick.kind === "select") {
        await el.selectOption(pick.options[0] ?? "", { timeout: 2_000 }).catch(() => {});
      } else if (pick.kind === "check") {
        await el.check({ timeout: 2_000 }).catch(() => el.click({ timeout: 2_000, force: true }).catch(() => {}));
      } else {
        await el.click({ timeout: 2_000 }).catch(() => {});
      }
      log(lastAction);
      since.push(confidence < 1 ? `${lastAction} (${confidence})` : lastAction);
      await page.waitForLoadState("domcontentloaded").catch(() => {});
      await page.waitForTimeout(SETTLE_MS);
      await opts.onPage?.(page.url());
    }
  } catch (e) {
    r.outcome = "error";
    r.note = (e as Error).message.split("\n")[0];
  }
  return r;
}

function describe(i: Item): string {
  if (i.kind === "fill") return `Type an answer into "${i.label}"`;
  if (i.kind === "check") return `Choose "${i.label}"`;
  if (i.kind === "select") return `Choose in "${i.label}"`;
  return `Press "${i.label}"`;
}
