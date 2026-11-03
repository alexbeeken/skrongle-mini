// Goal-directed navigation: Playwright reads the page and acts, Jev decides.
// Each step sends Jev the goal, the page (visible text, testid state, the
// actions on offer) and the steps so far, and asks two questions in one
// call: is the goal reached (Noul), and which action next (Choice). Jev
// can't write text, so anything typed comes from the journey's `inputs`.
// Walked as a persona (options.persona), `who` goes into the state, `prefers`
// into the instructions, `patience` sets the step budget, and the profile's
// `avoid` and `inputs` add to the journey's.
import type { Page } from "@playwright/test";
import { judgeAndChoose, LOW_CONFIDENCE, PASS } from "./jev";
import type { Journey } from "../journeys";
import type { Persona } from "./persona";

/** Never offered as actions: they end the session or destroy data. */
const NEVER = ["nav-logout", "nav-logout-form", "course-edit-delete", "course-edit-file-delete", "manage-api-key-remove", "dev-ai-answer-submit"];
const MAX_ACTIONS = 120; // Choice takes up to 255 options; fewer keeps state small
const MAX_TEXT = 4_000;
const SETTLE_MS = 150; // let page scripts render after an action
const STUCK_AFTER = 6; // unchanged pages in a row before giving up

interface Action {
  key: string; // "a0", "a1", …: what Jev picks
  ref: string; // data-e2e-ref put on the element
  kind: "click" | "fill" | "check" | "select";
  label: string; // what Jev reads
  value?: string;
  href?: string; // a link's target, path and query
}

export interface Step {
  n: number;
  url: string;
  done: number; // Jev: probability the goal is reached
  action?: string;
  confidence?: number;
  guess?: boolean; // confidence under LOW_CONFIDENCE
  note?: string;
}

export interface Outcome {
  reached: boolean;
  steps: Step[];
  reason: string;
}

/** What the navigator read off a page: handed to `onPage` for exploration. */
export interface Seen {
  url: string;
  title: string;
  text: string;
  state: Record<string, Record<string, string>>;
  actions: string[];
}

export interface NavigateOptions {
  /** Walk as this persona (load the journey's `persona` id with loadPersona). */
  persona?: Persona;
  /** Called with every page read, before Jev chooses there, and the steps so far. */
  onPage?: (seen: Seen, steps: readonly Step[]) => Promise<void>;
  /** Asked before each step; a reason ends the walk there (an exploration's budget). */
  stop?: () => string | undefined;
}

async function readPage(page: Page, journey: Pick<Journey, "avoid">) {
  const avoid = [...NEVER, ...(journey.avoid ?? [])];
  return page.evaluate(
    ({ avoid, maxActions, maxText }) => {
      // checkVisibility also hides what sits in a closed <details> (the account menu);
      // its <summary> is offered instead, to open it.
      const visible = (el: Element) =>
        el.checkVisibility({ visibilityProperty: true, opacityProperty: true }) && (el as HTMLElement).getBoundingClientRect().width > 0;
      const sel = "a[href], button, input, select, textarea, [role=button], [role=tab], details > summary";
      document.querySelectorAll("[data-e2e-ref]").forEach((el) => el.removeAttribute("data-e2e-ref"));
      const items: { ref: string; tag: string; type: string; testid: string; text: string; checked: boolean; value: string; options: string[]; href: string; open: boolean }[] = [];
      // A modal dialog (showModal) makes the rest of the page inert: clicks behind
      // it time out. While one is open only its own controls are on offer (#285).
      const modal = document.querySelector("dialog:modal");
      for (const el of Array.from((modal ?? document).querySelectorAll(sel))) {
        if (items.length >= maxActions) break;
        const h = el as HTMLInputElement;
        // A see-through checkbox laid over its own mark (the review's − "leave out") is
        // still what a person taps: opacity alone does not hide a radio or checkbox.
        const toggle = h.type === "checkbox" || h.type === "radio";
        const shown = toggle ? el.checkVisibility({ visibilityProperty: true }) && h.getBoundingClientRect().width > 0 : visible(el);
        if (!shown || h.disabled) continue;
        const testid = el.closest("[data-testid]")?.getAttribute("data-testid") ?? "";
        if (avoid.includes(testid)) continue;
        const href = el.getAttribute("href") ?? "";
        if (/^(https?:|mailto:)/.test(href) && !href.startsWith(location.origin)) continue;
        if (h.type === "hidden" || h.type === "file") continue;
        // A link to the page you are on goes nowhere; the markup says which (aria-current).
        if (el.getAttribute("aria-current") === "page") continue;
        // Used up: a matched pair, a ladder tile already placed.
        const state = el.getAttribute("data-state") ?? "";
        if (state === "used" || state === "done" || el.getAttribute("aria-disabled") === "true") continue;
        const ref = String(items.length);
        // A visually hidden radio or checkbox (sr-only, inside a chip or pill) can't take a
        // pointer: the action goes to its label, which is what a person taps.
        const r = el.getBoundingClientRect();
        const tap = (h.type === "radio" || h.type === "checkbox") && (r.width <= 2 || r.height <= 2) ? h.labels?.[0] : undefined;
        (tap ?? el).setAttribute("data-e2e-ref", ref);
        let label = (el.getAttribute("aria-label") || (h.labels?.[0]?.innerText ?? "") || (el as HTMLElement).innerText || el.textContent || h.value || h.placeholder || href)
          .replace(/\s+/g, " ")
          .trim();
        // A title often says what a terse link does; an aria-label is the author's name for it and wins.
        const title = el.getAttribute("title")?.trim();
        if (title && !el.hasAttribute("aria-label") && !label.includes(title)) label += ` (${title})`;
        label = label.slice(0, 100);
        // State as the markup says it (aria-pressed, data-state), so Jev sees what is picked or graded.
        const flags = [
          (el.getAttribute("aria-pressed") === "true" || el.getAttribute("aria-selected") === "true") && "selected",
          (state === "right" || state === "correct") && "marked right",
          state === "wrong" && "marked wrong",
        ].filter(Boolean);
        if (flags.length) label += ` [${flags.join(", ")}]`;
        items.push({
          ref,
          tag: el.tagName.toLowerCase(),
          type: h.type ?? "",
          testid,
          text: label,
          checked: !!h.checked,
          value: ["INPUT", "SELECT", "TEXTAREA"].includes(el.tagName) ? h.value : "",
          options: el.tagName === "SELECT" ? Array.from((el as HTMLSelectElement).options).map((o) => o.value) : [],
          href: el.tagName === "A" ? new URL((el as HTMLAnchorElement).href).pathname + new URL((el as HTMLAnchorElement).href).search : "",
          open: el.tagName === "SUMMARY" && !!(el.parentElement as HTMLDetailsElement).open,
        });
      }
      // State attributes: what the page says about itself (data-passed, data-done, …).
      const state: Record<string, Record<string, string>> = {};
      for (const el of Array.from(document.querySelectorAll("[data-testid]"))) {
        const attrs = Object.fromEntries(
          Array.from(el.attributes)
            .filter((a) => a.name.startsWith("data-") && !["data-testid", "data-e2e-ref"].includes(a.name) && a.value.length < 60)
            .map((a) => [a.name.slice(5), a.value]),
        );
        if (Object.keys(attrs).length && visible(el)) state[el.getAttribute("data-testid")!] ??= attrs;
      }
      // The page-wide state on <html> (data-… attributes): a choice that only changes that still has an effect.
      const root = Object.fromEntries(Array.from(document.documentElement.attributes).filter((a) => a.name.startsWith("data-")).map((a) => [a.name.slice(5), a.value]));
      if (Object.keys(root).length) state.html = root;
      // The page's own content is <main>: Sprint mode keeps its answer sheet and results in there too.
      // (innerText of a hidden <main> would be its whole textContent, so hidden means empty.)
      const main = document.querySelector("main") ?? document.body;
      const pageText = main.checkVisibility() ? (main as HTMLElement).innerText : "";
      // With a modal open, the dialog is what a person reads first; the page behind it after.
      const text = modal ? `A dialog is open over the page:\n${(modal as HTMLElement).innerText}\n\nBehind it (not reachable until the dialog is closed):\n${pageText}` : pageText;
      return {
        title: document.title,
        text: text.replace(/\n{3,}/g, "\n\n").slice(0, maxText),
        state,
        items,
      };
    },
    { avoid, maxActions: MAX_ACTIONS, maxText: MAX_TEXT },
  );
}

function actionsFor(items: Awaited<ReturnType<typeof readPage>>["items"], journey: Pick<Journey, "inputs">): Action[] {
  const inputs = journey.inputs ?? {};
  const out: Action[] = [];
  const add = (a: Omit<Action, "key">) => out.push({ ...a, key: `a${out.length}` });
  for (const it of items) {
    const where = it.testid ? ` [${it.testid}]` : "";
    const textual = it.tag === "textarea" || (it.tag === "input" && !["checkbox", "radio", "submit", "button"].includes(it.type));
    if (textual) {
      const value = inputs[it.testid] ?? inputs["*"];
      // Typing what the box already holds does nothing: not offered.
      if (value !== undefined && value !== it.value) add({ ref: it.ref, kind: "fill", value, label: `Type "${value}" into the "${it.text}" box${where}` });
    } else if (it.tag === "select") {
      for (const v of it.options.slice(0, 8)) add({ ref: it.ref, kind: "select", value: v, label: `Choose "${v}" in "${it.text}"${where}` });
    } else if (it.tag === "summary") {
      add({ ref: it.ref, kind: "click", label: `${it.open ? "Close" : "Open"} the menu "${it.text}"${where}` });
    } else if (it.type === "checkbox" || it.type === "radio") {
      if (!it.checked) add({ ref: it.ref, kind: "check", label: `Select the option "${it.text}"${where}` });
    } else {
      const what = it.tag === "a" ? "link" : "button";
      add({ ref: it.ref, kind: "click", label: `Click the ${what} "${it.text}"${where}`, ...(it.href && { href: it.href }) });
    }
  }
  return out;
}

async function perform(page: Page, a: Action): Promise<void> {
  const el = page.locator(`[data-e2e-ref="${a.ref}"]`);
  const timeout = 2_000; // a failed action is logged and the walk goes on
  // Typing ends the way a person's does, by moving on: a box that keeps its value on
  // blur (the editor's rename boxes) takes it then, before the next step is judged.
  if (a.kind === "fill") await el.fill(a.value!, { timeout }).then(() => el.blur({ timeout }).catch(() => {}));
  else if (a.kind === "select") await el.selectOption(a.value!, { timeout });
  else if (a.kind === "check" && (await el.evaluate((e) => e.tagName === "LABEL"))) await el.click({ timeout }); // a hidden control's label
  else if (a.kind === "check") await el.check({ timeout });
  else await el.click({ timeout });
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(SETTLE_MS);
}

/** Read the current page the way a navigation step does. */
export async function see(page: Page, journey: Pick<Journey, "avoid" | "inputs"> = {}): Promise<Seen> {
  const seen = await readPage(page, journey);
  const url = new URL(page.url());
  return { url: url.pathname + url.search, title: seen.title, text: seen.text, state: seen.state, actions: actionsFor(seen.items, journey).map((a) => a.label) };
}

/** Walk toward `journey.goal` from the current page. Never throws for a missed goal; the caller decides. */
export async function navigate(page: Page, journey: Journey, opts: NavigateOptions = {}): Promise<Outcome> {
  const steps: Step[] = [];
  const persona = opts.persona;
  journey = { ...journey, avoid: [...(journey.avoid ?? []), ...(persona?.avoid ?? [])], inputs: { ...persona?.inputs, ...journey.inputs } };
  const maxSteps = journey.maxSteps ?? persona?.patience ?? 15;
  // A persona gives up ("lost") after this many dead taps or unchanged pages in all.
  const lostAfter = persona ? Math.max(2, Math.ceil(persona.patience / 2)) : Infinity;
  let noEffect = 0;
  let lastSig = "", sameFor = 0;
  const useless = new Set<string>(); // "page signature action" pairs that changed nothing
  const navigations = new Map<string, number>(); // "url action" -> times it led to another page
  for (let n = 0; n <= maxSteps; n++) {
    if (journey.check && (await journey.check(page))) {
      return { reached: true, steps, reason: "the journey's exact check passed" };
    }
    const seen = await readPage(page, journey);
    const actions = actionsFor(seen.items, journey);
    // Tapping a tile can move it without changing the text, so the actions, form values and state count too.
    const sig = JSON.stringify([page.url(), seen.text, seen.items.map((i) => i.text + i.value + i.checked), seen.state]);
    sameFor = sig === lastSig ? sameFor + 1 : 0;
    lastSig = sig;
    const prev = steps[steps.length - 1];
    if (prev && sameFor > 0) {
      prev.note = prev.note ? `${prev.note}; no effect` : "no effect";
      useless.add(`${sig} ${prev.action}`);
      noEffect += 1;
    }
    if (sameFor >= STUCK_AFTER) return { reached: false, steps, reason: `stuck: the page did not change for ${STUCK_AFTER} steps` };
    if (noEffect >= lostAfter) return { reached: false, steps, reason: `lost: ${noEffect} steps had no effect, more than ${persona!.id} puts up with` };

    const url = new URL(page.url()).pathname + new URL(page.url()).search;
    await opts.onPage?.({ url, title: seen.title, text: seen.text, state: seen.state, actions: actions.map((a) => a.label) }, steps);
    const stop = opts.stop?.();
    if (stop) return { reached: false, steps, reason: stop };
    const state = {
      ...(persona && { who: persona.who }),
      goal: journey.goal,
      url,
      title: seen.title,
      page_text: seen.text,
      page_state: seen.state,
      steps_so_far: steps.slice(-8).map((s) => `${s.url}: ${s.action}${s.note ? ` (${s.note})` : ""}`),
    };
    // An action that left this exact page unchanged is not offered again (a wrong bucket, a dead button).
    const options: Record<string, string> = Object.fromEntries(
      actions.filter((a) => !useless.has(`${sig} ${a.label}`)).map((a) => [a.key, a.label]),
    );
    options.back = "Go back to the previous page";
    const res = await judgeAndChoose(
      state,
      journey.done,
      "Which action moves most directly toward the `goal`, given the `page_text`, `page_state` and `steps_so_far`? " +
        "A step marked (no effect) was wrong or useless: choose something else. Don't go back and forth between the same pages." +
        (persona ? ` You are the person in \`who\`. ${persona.prefers}` : ""),
      options,
    );
    if (process.env.JEV_DEBUG) console.log(`step ${n} ${state.url}\n` + actions.map((a) => `  ${a.key} ${a.label}`).join("\n"));
    const step: Step = { n, url: state.url, done: round(res.done) };
    steps.push(step);
    if (!journey.check && res.done >= PASS) {
      return { reached: true, steps, reason: `Jev judged the goal reached (p=${step.done})` };
    }
    if (n === maxSteps) break;
    // Loop guard: an action that already led away from this page twice is not taken again; the next most likely one is.
    const tried = (key: string) => {
      const label = key === "back" ? "Go back" : actions.find((x) => x.key === key)?.label;
      return navigations.get(`${state.url} ${label}`) ?? 0;
    };
    let pick = res.choice;
    if (tried(pick) >= 2) {
      const alt = Object.entries(res.probabilities).sort((x, y) => y[1] - x[1]).find(([k]) => tried(k) < 2);
      if (alt) {
        pick = alt[0];
        step.note = "loop guard: took the next most likely action";
      }
    }
    const a = actions.find((x) => x.key === pick);
    const before = page.url();
    step.action = a ? a.label : "Go back";
    step.confidence = round(pick === res.choice ? res.confidence : res.probabilities[pick] ?? 0);
    step.guess = step.confidence < LOW_CONFIDENCE;
    if (a?.href === state.url) step.note = "a link to this page";
    try {
      if (a) await perform(page, a);
      else await page.goBack();
    } catch (e) {
      step.note = `action failed: ${(e as Error).message.split("\n")[0]}`;
    }
    if (page.url() !== before) {
      const k = `${state.url} ${step.action}`;
      navigations.set(k, (navigations.get(k) ?? 0) + 1);
    }
  }
  return { reached: false, steps, reason: `goal not reached in ${maxSteps} steps` };
}

const round = (x: number) => Math.round(x * 100) / 100;

/**
 * Redo a recorded step on the current page, with no Jev: the action whose
 * label matches the step log's ("Click the link "Lessons" [nav-lessons]"),
 * or "Go back". Returns what happened, for the replay log.
 */
export async function redo(page: Page, label: string, journey: Pick<Journey, "avoid" | "inputs"> = {}): Promise<"done" | "missing" | "failed"> {
  if (label === "Go back") {
    await page.goBack().catch(() => {});
    return "done";
  }
  const actions = actionsFor((await readPage(page, journey)).items, journey);
  // Exact label first; then the same kind of action on the same testid with the same leading text
  // (a [selected] flag or a counter in the label may differ between runs).
  const stem = (l: string) => l.replace(/ \[[^\]]*\]$/, "").replace(/ \[[^\]]*\]$/, "").slice(0, 40);
  const tid = (l: string) => / \[([\w-]+)\]$/.exec(l)?.[1];
  const a = actions.find((x) => x.label === label) ?? actions.find((x) => tid(x.label) === tid(label) && stem(x.label) === stem(label));
  if (!a) return "missing";
  try {
    await perform(page, a);
    return "done";
  } catch {
    return "failed";
  }
}

/** A readable step log for a failure message or a report. */
export function describe(outcome: Outcome): string {
  const lines = outcome.steps.map(
    (s) =>
      `  ${String(s.n).padStart(2)}. ${s.url}  done=${s.done}` +
      (s.action ? `  → ${s.action} (conf ${s.confidence}${s.guess ? ", guess" : ""})` : "") +
      (s.note ? `  !! ${s.note}` : ""),
  );
  return `${outcome.reason}\n${lines.join("\n")}`;
}
