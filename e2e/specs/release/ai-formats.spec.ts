// The AI providers that speak OpenAI's chat-completions format, on a release
// server (#44): OpenAI itself, through SKRONGLE_OPENAI_BASE_URL (the operator's
// setting, which scripts/release-e2e.sh points at the simulator's OpenAI route),
// and an Ollama-compatible server, whose base URL and model the owner types on
// Manage Users. ai.spec.ts is the long path on Anthropic's format; this is the
// shortest useful one on the other format: save the key (or the URL), a learner
// generates the fixture course, one tutor turn. The simulator's call log says
// which format, model and key each request came with.
//
// Ollama's row and the private-address rule: the row takes any http(s) URL,
// loopback and any port included, with SKRONGLE_ALLOW_PRIVATE_AI_URLS unset
// (secrets::validate_key: only the owner sets it, and it is their own server).
// The rule (ai::check_base_url) guards only the generate form's `base_url`
// field, which no page offers, so a page cannot show it; cargo tests cover it.
import type { Page } from "@playwright/test";
import { removeApiKey, saveApiKey } from "../../lib/learner.ts";
import { callsSince, GENERATION, generateFixtureCourse, lastCallId, NO_SIM, SIM } from "../../lib/release-ai.ts";
import { expect, newLearner, ownerPerson, test } from "../../lib/release.ts";

const PROVIDERS = ["anthropic", "openai", "ollama"];

/** As the owner on `page`: remove every key the release AI specs save but `keep`, so the dialog and the tutor use that one. */
async function onlyKey(page: Page, keep: string): Promise<void> {
  for (const p of PROVIDERS) if (p !== keep) await removeApiKey(page, p);
}

/** A new Teacher generates the fixture course and opens the tutor, which greets them; the calls that made, each a 200 in OpenAI's format. */
async function generateAndTutor(browser: Parameters<typeof newLearner>[0], prefix: string) {
  const start = await lastCallId();
  const learner = await newLearner(browser, prefix, "Teacher");
  const lp = learner.page;
  const id = await generateFixtureCourse(lp, "Fractions over the other wire");
  expect((await callsSince(start, "openai")).map((c) => c.purpose)).toEqual(GENERATION);
  expect((await lp.request.post(`/courses/open/${id}`, { maxRedirects: 0 })).status()).toBe(303);
  await lp.goto("/tutor");
  await expect(lp.getByTestId("tutor-thread")).toContainText("practise this lesson together");
  const calls = await callsSince(start, "openai");
  expect(calls.map((c) => c.purpose)).toEqual([...GENERATION, "tutor-turn"]);
  return calls;
}

test("OpenAI's format: the owner's OpenAI key, a generated course and a tutor turn, all as chat completions", async ({ browser }) => {
  test.skip(!SIM, NO_SIM);
  test.setTimeout(180_000);
  const owner = await ownerPerson(browser);
  await onlyKey(owner.page, "openai");
  // The simulator's Learner key profile takes any sk-dev-… key.
  await saveApiKey(owner.page, "sk-dev-release-openai-5678", "openai");

  const calls = await generateAndTutor(browser, "ai-openai");
  for (const c of calls) {
    expect(c.model, `call #${c.id} ${c.purpose}: OpenAI's model`).toBe("gpt-5");
    expect(c.key, `call #${c.id} ${c.purpose}: the owner's key, as a bearer token`).toMatch(/^sk-dev-releas/);
  }

  await removeApiKey(owner.page, "openai");
});

test("Ollama's row: a loopback base URL and a model typed on Manage Users; the generation and the tutor go there with no key", async ({ browser }) => {
  test.skip(!SIM, NO_SIM);
  test.setTimeout(180_000);
  const owner = await ownerPerson(browser);
  const op = owner.page;
  await onlyKey(op, "ollama");

  await op.goto("/manage#ai");
  const row = op.locator('[data-testid="manage-api-key"][data-provider="ollama"]');
  await row.getByTestId("manage-api-key-toggle").click();
  const input = row.getByTestId("manage-api-key-input");
  // Not a URL: refused under the field, nothing saved.
  await input.fill("localhost:11434");
  await expect(row.getByTestId("manage-api-key-error")).toHaveText(
    "A base URL starts with http:// or https:// and a host, for example http://localhost:11434.",
  );
  await expect(row).toHaveAttribute("data-key-set", "false");
  // The simulator's Ollama route: 127.0.0.1 on a port other than 80 or 443. The server runs without
  // SKRONGLE_ALLOW_PRIVATE_AI_URLS, and the owner's row takes it all the same.
  const url = `${SIM}/dev/ai/ollama`;
  await input.fill(url);
  await expect(row).toHaveAttribute("data-key-set", "true");
  // The row shows the server's host and port, and the model it runs.
  await expect(row.getByTestId("manage-api-key-status")).toContainText(new URL(url).host);
  await row.getByTestId("manage-ollama-model").fill("qwen2.5:7b");
  await expect(row.getByTestId("manage-ollama-model-shown")).toHaveText("qwen2.5:7b");
  await op.reload();
  await expect(op.locator('[data-testid="manage-api-key"][data-provider="ollama"]').getByTestId("manage-ollama-model-shown")).toHaveText("qwen2.5:7b");

  const calls = await generateAndTutor(browser, "ai-ollama");
  for (const c of calls) {
    expect(c.model, `call #${c.id} ${c.purpose}: the model typed on Manage Users`).toBe("qwen2.5:7b");
    // The simulator's Ollama route stands in this key for a request that carries none.
    expect(c.key, `call #${c.id} ${c.purpose}: no key sent`).toBe("sk-dev-ollama");
  }

  await removeApiKey(op, "ollama");
});
