// Record a Jev walk toward a goal and turn it into a draft smoke spec
// (issue #48). Runs only when scripts/e2e-record.sh asks (E2E_RECORD_*);
// writes results/record/<id>/trace.json and <id>.spec.ts.
//
// Jev chooses the steps (lib/navigate.ts). It can't write, and the answers
// are not what is being recorded, so the recorder answers questions from
// the answer key before Jev chooses (a reading check, a Practice or Guide
// question, a whole Sprint mode session), and Jev is kept off the answer inputs.
// A script in the page reports each click, fill and pick with the testids
// around it, for the converter (record/convert.ts).
import type { Page } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { answerQuestion, passReadingCheck } from "../lib/answer.ts";
import { playSprintRight } from "../lib/sprint-play.ts";
import { SEED, test } from "../lib/fixtures.ts";
import { costUSD, jevEnabled, MODEL, SKIP_REASON, usage } from "../lib/jev";
import { AnswerKey } from "../lib/key.ts";
import { newAccount, register } from "../lib/learner.ts";
import { describe as stepLog, navigate } from "../lib/navigate";
import { convert, type RecEvent, type RecStep, type Snapshot, type Trace } from "../record/convert.ts";

const env = process.env;
const id = env.E2E_RECORD_ID || "";
const goal = env.E2E_RECORD_GOAL || "";
const start = env.E2E_RECORD_START || "/";
const course = env.E2E_RECORD_COURSE || "spanish";
const budget = Number(env.E2E_RECORD_BUDGET || 20);
const OUT = new URL(`../results/record/${id}/`, import.meta.url);

/** Answer inputs: the recorder fills them from the key, so Jev may not touch them. */
const ANSWERS = ["lesson-check-option", "lesson-check-answer", "quiz-option", "quiz-answer", "stage-option", "stage-answer"];

/** In the page, before its scripts: report user events (not the recorder's own) to __e2eRecord. */
function recordEvents() {
  const w = window as unknown as { __recAuto?: boolean; __e2eRecord: (e: unknown) => void };
  const vis = (el: Element) => el.checkVisibility({ visibilityProperty: true, opacityProperty: true });
  const ACT = "a[href], button, input, select, textarea, summary, [role=button], [data-testid]";
  const describe = (el: Element, type: string, value?: string) => {
    const own = el.getAttribute("data-testid") ?? "";
    const holder = el.closest("[data-testid]");
    const testid = holder?.getAttribute("data-testid") ?? "";
    const tag = el.tagName.toLowerCase();
    const inputType = (el as HTMLInputElement).type || undefined;
    let peers: Element[], holderNth: number | undefined, holderCount: number | undefined;
    if (own) peers = Array.from(document.querySelectorAll(`[data-testid="${own}"]`)).filter(vis);
    else if (holder) {
      const holders = Array.from(document.querySelectorAll(`[data-testid="${testid}"]`)).filter(vis);
      holderNth = holders.indexOf(holder);
      holderCount = holders.length;
      peers = Array.from(holder.querySelectorAll(tag + (tag === "input" && inputType ? `[type=${inputType}]` : ""))).filter(vis);
    } else peers = Array.from(document.querySelectorAll(tag)).filter(vis);
    const stage = el.closest('[data-testid="sprint-play-stage"]');
    w.__e2eRecord({
      type,
      tag,
      inputType: tag === "input" ? inputType : undefined,
      ownTestid: own,
      testid,
      nth: Math.max(0, peers.indexOf(el)),
      count: peers.length,
      holderNth,
      holderCount,
      lessonId: el.closest("[data-lesson-id]")?.getAttribute("data-lesson-id") ?? undefined,
      qid: el.closest("[data-qid]")?.getAttribute("data-qid") ?? undefined,
      exercise: (stage && stage.getAttribute("data-exercise-id")) || (stage && stage.getAttribute("data-kind")) || undefined,
      value,
    });
  };
  document.addEventListener(
    "click",
    (ev) => {
      if (w.__recAuto || !ev.isTrusted) return;
      const el = (ev.target as Element).closest(ACT);
      if (!el || el.matches("input, textarea, select, label") || el.closest("label")) return; // their change says it
      describe(el, "click");
    },
    true,
  );
  document.addEventListener(
    "change",
    (ev) => {
      if (w.__recAuto) return;
      const el = ev.target as HTMLInputElement;
      if (el.type === "radio" || el.type === "checkbox") describe(el, "check");
      else if (el.tagName === "SELECT") describe(el, "select", el.value);
      else describe(el, "fill", el.value);
    },
    true,
  );
}

/** What the converter diffs: data-* and a few aria-* of the first visible element of each testid. */
async function snapshot(page: Page): Promise<{ url: string; state: Snapshot }> {
  const state = await page.evaluate(() => {
    const out: Record<string, Record<string, string>> = {};
    for (const el of Array.from(document.querySelectorAll("[data-testid]"))) {
      const tid = el.getAttribute("data-testid")!;
      if (out[tid] || !el.checkVisibility({ visibilityProperty: true })) continue;
      const attrs: Record<string, string> = {};
      for (const a of Array.from(el.attributes)) {
        if (a.value.length >= 60 || ["data-testid", "data-e2e-ref"].includes(a.name)) continue;
        if (a.name.startsWith("data-")) attrs[a.name.slice(5)] = a.value;
        else if (["aria-pressed", "aria-current", "aria-expanded"].includes(a.name)) attrs[a.name] = a.value;
      }
      out[tid] = attrs;
    }
    return out;
  });
  const u = new URL(page.url());
  return { url: u.pathname + u.search, state };
}

/** Answer what is on screen from the key, as the recorder's own step. Returns what it did. */
async function autoAnswer(page: Page, key: AnswerKey): Promise<RecStep["auto"]> {
  const check = page.getByTestId("lesson-check");
  const stage = page.getByTestId("sprint-play-stage");
  if ((await stage.isVisible()) && !["", "done"].includes((await stage.getAttribute("data-kind")) ?? "")) {
    await playSprintRight(page, key);
    return "sprint-play";
  }
  if ((await check.isVisible()) && (await check.getAttribute("data-done")) === "false" && (await page.getByTestId("lesson-check-question").count())) {
    // Fill only: Jev submits (passReadingCheck in the spec does both).
    for (const q of await page.getByTestId("lesson-check-question").all()) await answerQuestion(q, key);
    return "check";
  }
  const q = page.locator('[data-qid]:visible:not([data-testid="lesson-check-question"])').first();
  if ((await q.count()) && (await q.locator("input:not([disabled])").count())) {
    const filled = await q.locator("input:checked, input.fill").evaluateAll((els) => els.some((e) => (e as HTMLInputElement).checked || ((e as HTMLInputElement).type === "text" && (e as HTMLInputElement).value)));
    if (!filled) {
      await answerQuestion(q, key);
      return "question";
    }
  }
  return undefined;
}

test.describe("record", { tag: "@record" }, () => {
  test.skip(!id || !goal, "recording runs only when asked: scripts/e2e-record.sh --goal … --id …");
  test.skip(!jevEnabled, SKIP_REASON);

  test(`record ${id}`, async ({ page }) => {
    test.setTimeout(60_000 + budget * 15_000);
    const key = new AnswerKey(course);
    const events: RecEvent[] = [];
    await page.exposeBinding("__e2eRecord", (_src, e: RecEvent) => void events.push(e));
    await page.addInitScript(recordEvents);
    const auto = async <T>(f: () => Promise<T>) => {
      await page.evaluate(() => ((window as unknown as { __recAuto: boolean }).__recAuto = true));
      try {
        return await f();
      } finally {
        await page.evaluate(() => ((window as unknown as { __recAuto: boolean }).__recAuto = false)).catch(() => {});
      }
    };

    const account = newAccount("rec");
    if (start.startsWith("/register")) await page.goto(start);
    else {
      await register(page, account, course);
      await page.goto(start);
    }

    const steps: RecStep[] = [];
    const t0 = Date.now();
    const outcome = await navigate(
      page,
      {
        id,
        start,
        goal,
        done: `The page shows that this goal is reached: ${goal}`,
        maxSteps: budget,
        avoid: ANSWERS,
        inputs: { "register-email": account.email, "register-password": account.password },
      },
      {
        onPage: async (seen, so_far) => {
          // The events since the last page belong to the step Jev took there.
          const prev = steps.at(-1);
          if (prev) {
            prev.events.push(...events.splice(0));
            prev.action = so_far.at(-1)?.action;
          }
          events.splice(0);
          const snap = await snapshot(page);
          const step: RecStep = { n: steps.length, url: seen.url, state: snap.state, events: [] };
          step.auto = await auto(() => autoAnswer(page, key));
          steps.push(step);
        },
      },
    );
    await page.waitForTimeout(300);
    const last = steps.at(-1);
    if (last) {
      last.events.push(...events.splice(0));
      last.action = outcome.steps.at(-1)?.action;
    }
    const trace: Trace = {
      id,
      goal,
      start,
      course,
      date: new Date().toISOString().slice(0, 10),
      model: MODEL,
      seed: SEED,
      reached: outcome.reached,
      reason: outcome.reason,
      jev: { calls: usage.calls, usd: Number(costUSD().toFixed(5)) },
      steps,
      final: await snapshot(page),
    };
    mkdirSync(OUT, { recursive: true });
    const dir = new URL(".", OUT).pathname;
    writeFileSync(join(dir, "trace.json"), JSON.stringify(trace, null, 2));
    writeFileSync(join(dir, `${id}.spec.ts`), convert(trace));
    console.log(`record ${id}: ${stepLog(outcome)}`);
    console.log(`Jev: ${usage.calls} calls ≈ $${costUSD().toFixed(5)}, ${Math.round((Date.now() - t0) / 1000)} s`);
    console.log(`==> draft: e2e/results/record/${id}/${id}.spec.ts (trace.json beside it)`);
    console.log(`==> review it, then: cp e2e/results/record/${id}/${id}.spec.ts e2e/specs/smoke/`);
    if (!outcome.reached) throw new Error(`goal not reached; the draft has the steps so far. ${outcome.reason}`);
  });
});
