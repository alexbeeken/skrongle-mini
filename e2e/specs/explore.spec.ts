// Exploratory run of one profile (e2e/personas/<id>.ts), or a replay of a
// recorded one. Runs only when scripts/e2e-explore.sh asks (it sets the
// JEV_EXPLORE_* variables below); it reports and never fails unless
// --fail-on names a kind of finding. Each run writes
// results/explore/<profile>-<timestamp>/report.{md,json} and shots/.
//
//   scripts/e2e-explore.sh impatient
//   scripts/e2e-explore.sh classroom --goal 1 --budget 30
//   scripts/e2e-explore.sh --replay e2e/results/explore/impatient-…/report.json
import { test } from "@playwright/test";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { devLogin, devReset } from "../lib/app";
import { explorer } from "../lib/explore";
import { buildReport, EXPLORE_DIR, renderMarkdown, terminalSummary, type Report } from "../lib/explore-report";
import { costUSD, jevEnabled, MODEL, SKIP_REASON, usage } from "../lib/jev";
import { describe as stepLog } from "../lib/navigate";
import { loadPersona, VIEWPORTS, type PersonaGoal } from "../lib/persona";

const env = process.env;
const who = env.JEV_EXPLORE_FILE || env.JEV_EXPLORE || "";
const replayPath = env.JEV_EXPLORE_REPLAY || "";
const goalArg = env.JEV_EXPLORE_GOAL || "";
const budget = Number(env.JEV_EXPLORE_BUDGET || 0); // total steps; 0: each goal gets the profile's patience
const maxCalls = Number(env.JEV_EXPLORE_MAX_CALLS || 300);
const wander = Number(env.JEV_EXPLORE_WANDER || 0);
const failOn = (env.JEV_EXPLORE_FAIL_ON || "").split(",").map((s) => s.trim()).filter(Boolean);

const stamp = () => new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15);

test.describe("explore", { tag: "@explore" }, () => {
  test.skip(!who && !replayPath, "exploration runs only when asked: scripts/e2e-explore.sh <profile>");
  test.skip(!replayPath && !jevEnabled, SKIP_REASON);

  test(`explore ${replayPath ? "replay" : who}`, async ({ page }, info) => {
    test.setTimeout(30 * 60_000); // --budget and --max-calls bound the run; this only catches a hang
    const recorded: Report | undefined = replayPath ? JSON.parse(readFileSync(replayPath, "utf8")) : undefined;
    const persona = await loadPersona(recorded ? (recorded.persona_file ?? recorded.profile) : who);
    const dir = join(EXPLORE_DIR, `${persona.id}-${recorded ? "replay-" : ""}${stamp()}`);
    mkdirSync(join(dir, "shots"), { recursive: true });

    const t0 = Date.now();
    const started = new Date();
    const ex = explorer(persona, { dir, judge: !recorded, maxCalls });
    ex.attach(page);
    await devLogin(page);

    const skipped: string[] = [];
    if (recorded) {
      for (const rec of recorded.goals) {
        await page.setViewportSize(VIEWPORTS[rec.viewport]);
        await page.goto(rec.start);
        const g = await ex.replay(page, rec, persona.goals.find((x) => x.goal === rec.goal));
        console.log(`replay [${g.viewport}] ${g.goal}: ${g.reached ? "reached" : "not reached"}; ${g.steps.filter((s) => s.replay && s.replay !== "done").length} steps not redone`);
      }
    } else {
      const goals: PersonaGoal[] = /^\d+$/.test(goalArg)
        ? [persona.goals[Number(goalArg)] ?? (() => { throw new Error(`${persona.id} has goals 0..${persona.goals.length - 1}`); })()]
        : goalArg
          ? [{ goal: goalArg, done: `The page shows that this goal is reached: ${goalArg}` }]
          : persona.goals;
      let used = 0;
      const left = () => (budget ? budget - used : Infinity);
      const walk = async (goal: PersonaGoal, i: number, extra = {}) => {
        const viewport = persona.viewports[i % persona.viewports.length];
        await page.setViewportSize(VIEWPORTS[viewport]);
        await page.goto(goal.start ?? "/");
        const g = await ex.run(page, goal, viewport, Math.min(persona.patience, left() - 1), extra);
        used += g.steps.length;
        console.log(`${persona.id} [${viewport}] ${goal.goal}\n${stepLog({ reached: g.reached, reason: g.reason, steps: g.steps })}`);
      };
      for (const [i, goal] of goals.entries()) {
        if (left() < 2) skipped.push(goal.goal);
        else await walk(goal, i);
      }
      for (let w = 0; w < wander && left() >= 2; w++) {
        const pick = await ex.pickSection(page);
        if (!pick) break;
        await walk(pick.goal, goals.length + w, { wander: true });
      }
    }

    const report = buildReport({
      mode: recorded ? "replay" : "explore",
      profile: persona.id,
      who: persona.who,
      personaFile: env.JEV_EXPLORE_FILE || recorded?.persona_file,
      replayOf: recorded,
      replayOfPath: replayPath ? relative(join(EXPLORE_DIR, "../../.."), replayPath) : undefined,
      model: recorded ? "none (replay)" : MODEL,
      commit: env.E2E_COMMIT || "unknown",
      started,
      seconds: (Date.now() - t0) / 1000,
      options: {
        ...(goalArg && { goal: goalArg }),
        ...(budget && { budget }),
        max_calls: maxCalls,
        ...(wander && { wander }),
        ...(skipped.length && { skipped_goals: skipped }),
        flag_at: Number(env.JEV_FLAG || 0.5),
      },
      jev: usage,
      costUSD: costUSD(),
      result: ex.result(),
      dir,
    });
    writeFileSync(join(dir, "report.json"), JSON.stringify(report, null, 2));
    writeFileSync(join(dir, "report.md"), renderMarkdown(report));
    await info.attach("report.md", { path: join(dir, "report.md"), contentType: "text/markdown" });
    console.log(`\n${terminalSummary(report, dir)}`);

    const failing = report.findings.filter((f) => failOn.includes(f.kind));
    if (failing.length) throw new Error(`--fail-on ${failOn.join(",")}: ${failing.length} finding(s)\n${failing.map((f) => `  ${f.kind}: ${f.hit}`).join("\n")}`);
  });

  test.afterAll(async ({ browser }) => {
    const page = await browser.newPage();
    await devReset(page);
    await page.close();
  });
});
