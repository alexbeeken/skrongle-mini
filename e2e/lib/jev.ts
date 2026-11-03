// Jev (TypeSafe System One): typed questions about a state, answered with
// probabilities. Used only here, in tests; nothing Jev-related ships in the
// app. Switched on by TYPESAFE_API_KEY (or JEV_API_KEY); without a key the
// specs that need it are skipped.
import { choice, noul, TypeSafeClient } from "@typesafe-ai/sdk";
import type { EntryType } from "@typesafe-ai/sdk";

type State = NonNullable<EntryType>;

const key = (process.env.TYPESAFE_API_KEY || process.env.JEV_API_KEY || "").trim();
export const jevEnabled = key !== "";
export const SKIP_REASON = "no Jev key: set TYPESAFE_API_KEY (or put it in e2e/.env or ./.env)";

// Pinned, so tuned thresholds don't drift when the `jev-latest` alias moves.
export const MODEL = process.env.JEV_MODEL || "jev-1.13.0";
// $ per input token (output is free), from docs.typesafe.ai/models.
const PRICE_PER_TOKEN = 0.042 / 1_000_000;

/** Probability at or above which a judged statement counts as true. */
export const PASS = Number(process.env.JEV_PASS || 0.8);
/** Choice confidence below which a navigation step is logged as a guess. */
export const LOW_CONFIDENCE = Number(process.env.JEV_LOW_CONFIDENCE || 0.2);

let client: TypeSafeClient | undefined;
function jev(): TypeSafeClient {
  if (!jevEnabled) throw new Error(SKIP_REASON);
  return (client ??= new TypeSafeClient({ apiKey: key, defaultModel: MODEL }));
}

/** Running totals for this worker, printed by the specs. */
export const usage = { calls: 0, inputTokens: 0, ms: 0 };
export function costUSD(): number {
  return usage.inputTokens * PRICE_PER_TOKEN;
}

async function call<T>(f: () => Promise<{ usage: { input_tokens: number | null } } & T>): Promise<T> {
  const t0 = Date.now();
  const res = await f();
  usage.calls += 1;
  usage.ms += Date.now() - t0;
  usage.inputTokens += res.usage.input_tokens ?? 0;
  return res;
}

/** How true `statement` is of `state`, 0..1. */
export async function judge(state: State, statement: string): Promise<number> {
  const res = await call(() => jev().systemOne({ state, questions: { holds: noul(statement) } }));
  if (process.env.JEV_DEBUG) console.log(`judge p=${res.answers.holds.noul.toFixed(2)} "${statement}"\n${JSON.stringify(state, null, 2)}`);
  return res.answers.holds.noul;
}

/** Pick one of `options` (key -> description). */
export async function choose(
  state: State,
  instructions: string,
  options: Record<string, string>,
): Promise<{ choice: string; confidence: number; probabilities: Record<string, number> }> {
  const res = await call(() => jev().systemOne({ state, questions: { pick: choice(instructions, options) } }));
  const a = res.answers.pick;
  return { choice: String(a.choice), confidence: a.confidence, probabilities: a.probabilities as Record<string, number> };
}

/** One call that judges `statement` and picks the next option: what a navigation step needs. */
export async function judgeAndChoose(
  state: State,
  statement: string,
  instructions: string,
  options: Record<string, string>,
): Promise<{ done: number; choice: string; confidence: number; probabilities: Record<string, number> }> {
  const res = await call(() =>
    jev().systemOne({ state, questions: { done: noul(statement), next: choice(instructions, options) } }),
  );
  const next = res.answers.next;
  return { done: res.answers.done.noul, choice: String(next.choice), confidence: next.confidence, probabilities: next.probabilities as Record<string, number> };
}

/** Any typed questions (Noul, Choice, Score) about `state` in one call; answers by question name. */
export async function ask(state: State, questions: Record<string, ReturnType<typeof noul> | ReturnType<typeof choice>>): Promise<Record<string, any>> {
  const res = await call(() => jev().systemOne({ state, questions } as never));
  return res.answers as Record<string, any>;
}

/** How true each of `statements` (name -> statement) is of `state`, in as few calls as possible. */
export async function judgeMany(state: State, statements: Record<string, string>): Promise<Record<string, number>> {
  const names = Object.keys(statements);
  const out: Record<string, number> = {};
  for (let i = 0; i < names.length; i += 24) {
    const questions = Object.fromEntries(names.slice(i, i + 24).map((n) => [n, noul(statements[n])]));
    const res = await call(() => jev().systemOne({ state, questions }));
    for (const [n, a] of Object.entries(res.answers)) out[n] = (a as { noul: number }).noul;
  }
  if (process.env.JEV_DEBUG) console.log(`judged ${JSON.stringify(out)}\n${JSON.stringify(state, null, 2)}`);
  return out;
}
