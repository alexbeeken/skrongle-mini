// One test per entry in ../journeys.ts: Jev chooses each step toward the
// goal, Playwright acts. A missed goal fails with the step log; every run
// writes results/journeys/<id>.json. Skipped without a Jev key.
import { test } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import { devLogin } from "../lib/app";
import { costUSD, jevEnabled, MODEL, SKIP_REASON, usage } from "../lib/jev";
import { describe, navigate } from "../lib/navigate";
import { loadPersona } from "../lib/persona";
import { journeys } from "../journeys";

const OUT = new URL("../results/journeys/", import.meta.url);

test.describe("journeys", { tag: "@jev" }, () => {
  test.skip(!jevEnabled, SKIP_REASON);

  for (const journey of journeys) {
    test(`journey ${journey.id}`, async ({ page }, info) => {
      test.setTimeout(30_000 + (journey.maxSteps ?? 15) * 5_000);
      if (journey.phone) await page.setViewportSize({ width: 390, height: 844 });
      await devLogin(page, journey.login ?? "open=spanish");
      const start = (await journey.setup?.(page)) || journey.start;
      await page.goto(start);

      const before = { ...usage };
      const t0 = Date.now();
      const persona = journey.persona ? await loadPersona(journey.persona) : undefined;
      const outcome = await navigate(page, journey, { persona }).finally(() => journey.teardown?.(page));
      const report = {
        id: journey.id,
        goal: journey.goal,
        model: MODEL,
        reached: outcome.reached,
        reason: outcome.reason,
        seconds: (Date.now() - t0) / 1000,
        jev: { calls: usage.calls - before.calls, input_tokens: usage.inputTokens - before.inputTokens, ms: usage.ms - before.ms },
        steps: outcome.steps,
      };
      mkdirSync(OUT, { recursive: true });
      writeFileSync(new URL(`${journey.id}.json`, OUT), JSON.stringify(report, null, 2));
      await info.attach("journey", { body: JSON.stringify(report, null, 2), contentType: "application/json" });
      info.annotations.push({ type: "jev", description: `${report.jev.calls} calls, ${report.jev.input_tokens} tokens, ${report.seconds}s` });
      console.log(`journey ${journey.id}: ${describe(outcome)}`);

      if (!outcome.reached) throw new Error(`journey ${journey.id}: ${describe(outcome)}`);
    });
  }

  test.afterAll(() => {
    if (usage.calls) console.log(`Jev: ${usage.calls} calls, ${usage.inputTokens} input tokens, ≈ $${costUSD().toFixed(5)}`);
  });
});
