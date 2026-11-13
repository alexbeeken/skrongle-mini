// The AI simulator the release AI specs (specs/release/ai*.spec.ts, #44) read
// from: a second process, the dev-feature build of the same code, that
// scripts/release-e2e.sh starts and points the release server's provider base
// URLs at (docs/testing.md, "AI on a release server"). E2E_AI_SIM_URL is that
// process. Its call log is read from the spec's node side, never through the
// browser: the release server has no /dev routes, and a person using it does
// not see the simulator.
import { request, type Page } from "@playwright/test";
import { generationEnds, createCourse } from "./app.ts";
import { expect } from "./release.ts";

/** The simulator's root (`http://127.0.0.1:<port>`), or undefined when the run has none. */
export const SIM = process.env.E2E_AI_SIM_URL?.replace(/\/$/, "");

/** Why the AI specs skip without a simulator. */
export const NO_SIM = "no AI simulator: scripts/release-e2e.sh needs cargo (or SIM_BIN) to start one";

/** One request in the simulator's call log (`/dev/ai?format=json`). */
export interface Call {
  id: number;
  /** The wire format it arrived in: `anthropic` (Messages API) or `openai` (chat completions, every OpenAI-format provider). */
  format: string;
  purpose: string;
  model: string;
  /** The start of the key it carried. */
  key: string;
  status: number;
}

/** The simulator's call log, oldest first, read straight from the simulator process. */
export async function simCalls(): Promise<Call[]> {
  // A request context of its own: this is the test talking to the simulator, not a person using the release server.
  const ctx = await request.newContext();
  let calls: Call[];
  try {
    const r = await ctx.get(`${SIM}/dev/ai?format=json`);
    expect(r.status(), "the simulator's /dev/ai").toBe(200);
    calls = (await r.json()).calls;
  } finally {
    await ctx.dispose();
  }
  return [...calls].sort((a, b) => a.id - b.id);
}

/** The id of the newest call in the log, 0 for none. */
export async function lastCallId(): Promise<number> {
  const calls = await simCalls();
  return calls.length ? calls[calls.length - 1].id : 0;
}

/** The calls made since `lastId`, each checked to be a 200 in wire format `format`. */
export async function callsSince(lastId: number, format: string): Promise<Call[]> {
  const calls = (await simCalls()).filter((c) => c.id > lastId);
  for (const c of calls) {
    expect(c.format, `call #${c.id} ${c.purpose}: its wire format`).toBe(format);
    expect(c.status, `call #${c.id} ${c.purpose}`).toBe(200);
  }
  return calls;
}

/** Generate the fixture course from the dialog of a fresh draft; stays on its editor. Returns the course id. */
export async function generateFixtureCourse(page: Page, wish: string): Promise<string> {
  const id = await createCourse(page);
  await page.getByTestId("generate-wish").fill(wish);
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");
  await expect(page.getByTestId("course-lessons-title")).toHaveText(["Simplifying", "What a fraction is", "Adding fractions"]);
  return id;
}

/** The purposes of the calls a generation of the fixture course makes, in order. */
export const GENERATION = ["generate-outline", "generate-lesson", "generate-lesson", "generate-lesson"];
