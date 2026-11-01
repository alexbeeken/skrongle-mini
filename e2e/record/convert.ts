// Turn a recorded Jev walk (trace.json, written by specs/record.spec.ts) into
// a draft smoke spec. Pure: no browser, no Jev, so it reruns on an old trace:
//
//   node e2e/record/convert.ts --from e2e/results/record/<id>/trace.json [--out file.spec.ts]
//
// The rules (issue #48, docs/testing.md "Recording a smoke spec"):
// - a step becomes a testid locator plus an action, never coordinates or
//   text that comes from the course;
// - an answer (a pick or a fill inside an element with data-qid, a tap on a
//   Sprint mode exercise) becomes answerQuestion / passReadingCheck / playSprintRight,
//   never what was typed;
// - after each step, expects on the state attributes that changed;
// - a fresh account through register(), so specs never share a user.
import { readFileSync, writeFileSync } from "node:fs";

/** One DOM event the recorder saw (the init script in specs/record.spec.ts). */
export interface RecEvent {
  type: "click" | "fill" | "check" | "select";
  tag: string; // a, button, input, …
  inputType?: string;
  ownTestid: string; // the element's own data-testid
  testid: string; // the nearest one, itself or an ancestor
  nth: number; // among the visible elements the locator matches
  count: number;
  holderNth?: number; // without its own testid: the holder's index among visible holders
  holderCount?: number;
  lessonId?: string; // nearest data-lesson-id
  qid?: string; // nearest data-qid: an answer
  exercise?: string; // Sprint mode: sprint-play-stage[data-exercise-id] when the event was on the stage
  value?: string; // what was typed or selected (only from the recorder's inputs)
}

/** testid -> attributes (data-*, aria-pressed/-current/-expanded) of its first visible element. */
export type Snapshot = Record<string, Record<string, string>>;

export interface RecStep {
  n: number;
  url: string; // path and query when Jev looked at the page
  state: Snapshot; // before the step
  action?: string; // Jev's choice, as the navigator labels it
  auto?: "check" | "question" | "sprint-play"; // the recorder answered from the key before Jev chose
  events: RecEvent[]; // what Jev's action did in the page
}

export interface Trace {
  id: string;
  goal: string;
  start: string;
  course: string;
  date: string;
  model: string;
  seed: number;
  reached: boolean;
  reason: string;
  jev: { calls: number; usd: number };
  steps: RecStep[];
  final: { url: string; state: Snapshot };
}

/** State attributes worth an expect: what a flow changes, not what content says. */
const STATE_ATTRS = new Set(["read", "passed", "done", "state", "seen", "current", "here", "stars", "achieved", "error", "index", "pos", "aria-pressed", "aria-current", "aria-expanded"]);
const MAX_EXPECTS = 8;

const q = (s: string) => JSON.stringify(s);
const css = (s: string) => s.replace(/["\\]/g, "\\$&");

/** A locator for the element an event happened on, from testids only. */
export function locatorFor(e: RecEvent): string {
  const pick = (base: string, nth: number, count: number, lessonId?: string) => {
    if (count <= 1 || nth === 0) return `${base}.first()`;
    return `${base}.nth(${nth})` + (lessonId ? ` /* lesson ${lessonId} */` : "");
  };
  if (e.ownTestid) {
    // A lesson's own button (a list of lessons): pick it by the lesson, not by its place.
    if (e.count > 1 && e.nth > 0 && e.lessonId)
      return `page.locator(${q(`:is([data-lesson-id="${css(e.lessonId)}"] [data-testid="${css(e.ownTestid)}"], [data-testid="${css(e.ownTestid)}"][data-lesson-id="${css(e.lessonId)}"]):visible`)}).first()`;
    return pick(`page.locator(${q(`[data-testid="${css(e.ownTestid)}"]:visible`)})`, e.nth, e.count, e.lessonId);
  }
  const inner = e.tag + (e.inputType && e.tag === "input" ? `[type=${e.inputType}]` : "");
  if (e.testid) {
    const holder = pick(`page.locator(${q(`[data-testid="${css(e.testid)}"]:visible`)})`, e.holderNth ?? 0, e.holderCount ?? 1, e.lessonId);
    return pick(`${holder}.locator(${q(`${inner}:visible`)})`, e.nth, e.count);
  }
  return `${pick(`page.locator(${q(`${inner}:visible`)})`, e.nth, e.count)} /* REVIEW: no testid here; add one to the template */`;
}

/** The expects for what changed between two snapshots (and the page moving on). */
export function diffExpects(before: { url: string; state: Snapshot }, after: { url: string; state: Snapshot }): string[] {
  const out: string[] = [];
  const seg = (u: string) => u.split("?")[0].split("/")[1] ?? "";
  if (seg(before.url) !== seg(after.url) || before.url.split("?")[0] !== after.url.split("?")[0]) {
    const s = seg(after.url);
    out.push(`await expect(page).toHaveURL(${s ? `/\\/${s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(\\/|\\?|$)/` : `/\\/(\\?|$)/`});`);
  }
  for (const [tid, attrs] of Object.entries(after.state)) {
    if (tid === "html") continue;
    for (const [k, v] of Object.entries(attrs)) {
      if (!STATE_ATTRS.has(k) || v.length > 30) continue;
      if (before.state[tid]?.[k] === v) continue;
      const attr = k.startsWith("aria-") ? k : `data-${k}`;
      out.push(`await expect(page.locator(${q(`[data-testid="${css(tid)}"]:visible`)}).first()).toHaveAttribute(${q(attr)}, ${q(v)});`);
      if (out.length >= MAX_EXPECTS) return [...out, "// … more state changed here; see trace.json"];
    }
  }
  return out;
}

const inQuestion = (e: RecEvent) => !!e.qid && (e.type === "fill" || e.type === "check" || e.type === "select");
const onStage = (e: RecEvent) => !!e.exercise;

/** The draft spec for `t`. */
export function convert(t: Trace): string {
  const lines: string[] = [];
  const emit = (s = "") => lines.push(s ? `  ${s}` : "");
  const imports = new Set<string>();
  // From /register: the account and the course come from register(); the walk's own steps there are dropped.
  let steps = t.steps;
  if (t.start.startsWith("/register")) {
    // register() ends where opening the course lands: keep what comes after the last "open" before the walk moves on.
    const end = steps.findIndex((s) => !/^\/(register|courses)?(\/|\?|$)/.test(s.url));
    const prefix = end < 0 ? steps : steps.slice(0, end);
    const opened = prefix.map((s) => s.events.some((e) => e.ownTestid === "courses-open")).lastIndexOf(true);
    steps = opened < 0 ? steps.slice(Math.max(end, 0)) : steps.slice(opened + 1);
  }
  let first = true;
  let skipSubmit = false;
  for (let k = 0; k < steps.length; k++) {
    const s = steps[k];
    const next = steps[k + 1] ?? t.final;
    emit(`// ${s.n}. ${s.url}${s.action ? ` → ${s.action.replace(/\s+/g, " ").slice(0, 110)}` : ""}`);
    if (first) {
      emit(`await page.goto(${q(s.url)});`);
      first = false;
    }
    if (s.auto === "check") {
      imports.add("passReadingCheck");
      emit("await passReadingCheck(page, key); // the reading check, from the answer key (it submits too)");
      skipSubmit = true;
    } else if (s.auto === "question") {
      imports.add("answerQuestion");
      emit(`await answerQuestion(page.locator('[data-qid]:visible').first(), key);`);
    } else if (s.auto === "sprint-play") {
      imports.add("playSprintRight");
      emit("expect((await playSprintRight(page, key)).wrong, \"Sprint mode answers the app marked wrong\").toEqual([]);");
    }
    let played = s.auto === "sprint-play";
    const answered = new Set<string>();
    for (const e of s.events) {
      if (onStage(e)) {
        if (!played) {
          imports.add("playSprintRight");
          emit("expect((await playSprintRight(page, key)).wrong, \"Sprint mode answers the app marked wrong\").toEqual([]);");
          played = true;
        }
        continue;
      }
      if (inQuestion(e)) {
        // passReadingCheck answered the check already; a pick Jev made there too is dropped.
        if (answered.has(e.qid!) || (s.auto === "check" && e.testid.startsWith("lesson-check"))) continue;
        answered.add(e.qid!);
        imports.add("answerQuestion");
        emit(`await answerQuestion(page.locator(${q(`[data-qid="${css(e.qid!)}"]`)}), key); // REVIEW: a question id names content; prefer the question on screen`);
        continue;
      }
      if (skipSubmit && e.type === "click" && e.ownTestid === "lesson-check-submit") {
        skipSubmit = false;
        continue;
      }
      const loc = locatorFor(e);
      if (e.type === "click") emit(`await ${loc}.click();`);
      else if (e.type === "check") emit(`await ${loc}.check();`);
      else if (e.type === "fill") emit(`await ${loc}.fill(${q(e.value ?? "")});`);
      else emit(`await ${loc}.selectOption(${q(e.value ?? "")});`);
    }
    if (!s.events.length && s.action === "Go back") emit("await page.goBack();");
    else if (!s.events.length && s.action && !s.auto) emit(`// (no effect in the page: ${s.action.slice(0, 80)})`);
    for (const x of diffExpects({ url: s.url, state: s.state }, next)) emit(x);
    emit();
  }

  const libs = [
    `import { expect, test } from "../../lib/fixtures.ts";`,
    `import { AnswerKey } from "../../lib/key.ts";`,
    `import { newAccount, register } from "../../lib/learner.ts";`,
    ...(imports.has("answerQuestion") || imports.has("passReadingCheck")
      ? [`import { ${[...imports].filter((x) => x !== "playSprintRight").sort().join(", ")} } from "../../lib/answer.ts";`]
      : []),
    ...(imports.has("playSprintRight") ? [`import { playSprintRight } from "../../lib/sprint-play.ts";`] : []),
  ];
  const head = [
    `// DRAFT: review before commit. Recorded by scripts/e2e-record.sh (issue #48).`,
    `//`,
    `// Goal: ${t.goal}`,
    `// Recorded ${t.date} from ${t.start}, course ${t.course}; Jev ${t.model}, seed ${t.seed};`,
    `// ${t.reached ? "goal reached" : "GOAL NOT REACHED"} (${t.reason}); ${t.jev.calls} Jev calls ≈ $${t.jev.usd.toFixed(4)}.`,
    `//`,
    `// Review checklist (docs/testing.md, "Recording a smoke spec"):`,
    `// - every expect is about the flow (passed, read, done, locked), not the content;`,
    `// - drop steps Jev took by accident (a menu opened and closed, a page visited and left);`,
    `// - lines marked REVIEW name content or miss a testid: rewrite them or add the testid;`,
    `// - rename the test after the user's goal, then delete this DRAFT header.`,
  ];
  return [
    ...head,
    ...libs,
    "",
    `const COURSE = ${q(t.course)};`,
    `const key = new AnswerKey(COURSE);`,
    "",
    `test(${q(t.id)}, async ({ page }) => {`,
    `  test.setTimeout(120_000);`,
    `  await register(page, newAccount(${q(t.id.slice(0, 12))}), COURSE);`,
    "",
    ...lines,
    `});`,
    "",
  ].join("\n");
}

// CLI: node e2e/record/convert.ts --from trace.json [--out file]
if (import.meta.url === `file://${process.argv[1]}`) {
  const a = process.argv.slice(2);
  const from = a[a.indexOf("--from") + 1];
  if (!a.includes("--from") || !from) {
    console.error("usage: node e2e/record/convert.ts --from <trace.json> [--out <file.spec.ts>]");
    process.exit(2);
  }
  const spec = convert(JSON.parse(readFileSync(from, "utf8")));
  const out = a.includes("--out") ? a[a.indexOf("--out") + 1] : "";
  if (out) writeFileSync(out, spec);
  else process.stdout.write(spec);
}
