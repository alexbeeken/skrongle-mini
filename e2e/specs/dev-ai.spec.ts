// The dev AI provider (#297): dev mode's stand-in for Anthropic and OpenAI,
// at /dev/ai/…, speaking their real wire protocols. Its page in the dev
// mailbox (/dev/ai) logs every call it served and controls each key
// profile's usage limit and injected failures, and edits the fixtures it
// answers from. Dev mode's default base URLs point there, and the dev user's
// seeded keys are its keys (#298): the app's own Generate dialog reaches it
// like any provider. "Manual replies" (a pill) makes each request wait, as a
// collapsible row at the bottom of the page, for an answer typed there.
// Exact: no real key, no network.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, generationEnds } from "../lib/app";

interface AiState {
  base_urls: Record<string, string>;
  client_timeout_secs: number | null;
  holding: boolean;
  held: { stem: string; pending: boolean }[];
  calls: { id: number; who: string; format: string; purpose: string; status: number; fixture: string; outcome: string; error: string }[];
  profiles: Record<string, { usage: { requests: number }; over: boolean; inject: string | null }>;
}

async function aiState(page: Page): Promise<AiState> {
  const r = await page.request.get("/dev/ai?format=json");
  expect(r.status(), "GET /dev/ai").toBe(200);
  return r.json();
}

/** Back to a clean learner profile: no limit, nothing injected, the fixture as on disk. */
async function resetSimulator(page: Page): Promise<void> {
  const forms: Record<string, string>[] = [{ do: "settings", limit: "", measure: "requests" }, { do: "reset" }, { do: "clear-inject" }];
  for (const form of forms) {
    expect((await page.request.post("/dev/ai/profiles/learner", { form, maxRedirects: 0 })).status()).toBe(303);
  }
  expect((await page.request.post("/dev/ai/fixtures/generate-outline", { form: { do: "reset" }, maxRedirects: 0 })).status()).toBe(303);
  expect((await page.request.post("/dev/ai/hold", { form: { do: "off" }, maxRedirects: 0 })).status()).toBe(303);
}

/**
 * Generate a new course from the Generate dialog with the dev user's saved
 * Anthropic key (the first one, checked): the default base URL is the
 * simulator's Anthropic API. Lands where the dialog does.
 */
async function generateViaSimulator(page: Page, wish: string): Promise<string> {
  const id = await createCourse(page);
  await expect(page.locator('[data-testid="generate-provider"][value="anthropic"]')).toBeChecked();
  await page.getByTestId("generate-wish").fill(wish);
  await page.getByTestId("generate-submit").click();
  return id;
}

test.describe.configure({ mode: "serial" });

test.afterEach(async ({ page }) => {
  await resetSimulator(page).catch(() => {});
});

test("a course generated through the dev AI provider shows in /dev/ai; limits, a timeout and an edited fixture reach the learner", async ({ page }) => {
  test.setTimeout(150_000);
  await devLogin(page, "keys=fake&next=/courses");
  await resetSimulator(page);
  expect((await aiState(page)).base_urls.anthropic).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/dev\/ai\/anthropic$/);
  const before = (await aiState(page)).calls.length ? (await aiState(page)).calls[0].id : 0;

  // 1. A course, written by the simulator from the fixtures, over the Anthropic wire protocol.
  await generateViaSimulator(page, "Fractions via the simulator");
  expect(await generationEnds(page)).toBe("done");
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(3);
  let calls = (await aiState(page)).calls.filter((c) => c.id > before);
  expect(calls.map((c) => c.purpose).reverse()).toEqual(["generate-outline", "generate-lesson", "generate-lesson", "generate-lesson"]);
  expect(calls.every((c) => c.who === "learner" && c.format === "anthropic" && c.status === 200)).toBe(true);
  expect((await aiState(page)).profiles.learner.usage.requests).toBeGreaterThanOrEqual(4);

  // The log on the page, and one call's request and reply.
  await page.goto("/dev/ai");
  await expect(page.getByTestId("dev-box-kind-ai")).toHaveAttribute("aria-current", "page");
  const outline = page.locator(`[data-testid="dev-ai-call"][data-id="${calls[calls.length - 1].id}"]`);
  await expect(outline).toHaveAttribute("data-purpose", "generate-outline");
  await expect(outline).toHaveAttribute("data-fixture", "generate-outline/default");
  await expect(outline.getByTestId("dev-ai-call-status")).toHaveText("200");
  await outline.getByTestId("dev-ai-call-open").click();
  await expect(page.getByTestId("dev-ai-call-fixture")).toHaveText("generate-outline/default");
  await expect(page.getByTestId("dev-ai-call-request")).toContainText("Fractions via the simulator");
  await expect(page.getByTestId("dev-ai-call-reply")).toContainText('"stop_reason": "end_turn"');

  // 2. An edited fixture is what the next call gets (in memory; Reset puts the file back).
  await page.goto("/dev/ai/fixtures/generate-outline");
  const text = page.getByTestId("dev-ai-fixture-text");
  const fixture = JSON.parse(await text.inputValue());
  fixture.replies.default.reply.lessons[0].title = "Edited in the dev AI page";
  await text.fill(JSON.stringify(fixture, null, 2));
  await page.getByTestId("dev-ai-fixture-apply").click();
  await expect(page.getByTestId("dev-ai-notice")).toContainText("edited for this server run");
  await expect(page.getByTestId("dev-ai-fixture-detail")).toHaveAttribute("data-edited", "true");
  await page.goto("/courses");
  await generateViaSimulator(page, "Fractions, from the edited fixture");
  expect(await generationEnds(page)).toBe("done");
  await expect(page.getByTestId("course-lessons-row").first()).toContainText("Edited in the dev AI page");
  await page.goto("/dev/ai/fixtures/generate-outline");
  await page.getByTestId("dev-ai-fixture-reset").click();
  await expect(page.getByTestId("dev-ai-fixture-detail")).toHaveAttribute("data-edited", "false");
  await expect(page.getByTestId("dev-ai-fixture-text")).not.toHaveValue(/Edited in the dev AI page/);

  // 3. "Hit the limit now": the provider's quota error, in words a learner can act on.
  await page.goto("/dev/ai");
  const learner = page.locator('[data-testid="dev-ai-profile"][data-who="learner"]');
  await learner.getByTestId("dev-ai-exhaust").click();
  await expect(learner).toHaveAttribute("data-over", "true");
  await expect(learner.getByTestId("dev-ai-over")).toBeVisible();
  await page.goto("/courses");
  await generateViaSimulator(page, "Fractions over the limit");
  expect(await generationEnds(page)).toBe("failed");
  await expect(page.getByTestId("generate-error")).toHaveText(/run out of credit or reached its usage limit/);
  await expect(page.getByTestId("generate-error")).not.toContainText("HTTP");
  calls = (await aiState(page)).calls;
  expect(calls[0]).toMatchObject({ purpose: "generate-outline", status: 400, outcome: "usage limit" });
  expect(calls[0].error).toContain("You have reached your specified API usage limits");

  // A limit in requests: the counter, then the same error once it is reached.
  await page.goto("/dev/ai");
  await learner.getByTestId("dev-ai-reset").click();
  await expect(learner).toHaveAttribute("data-over", "false");
  await learner.getByTestId("dev-ai-limit").fill("1");
  await learner.getByTestId("dev-ai-measure").selectOption("requests");
  await learner.getByTestId("dev-ai-limit-save").click();
  await expect(learner.getByTestId("dev-ai-usage")).toHaveText("0 requests of 1 request");
  await page.goto("/courses");
  await generateViaSimulator(page, "Fractions, one request allowed");
  expect(await generationEnds(page)).toBe("failed"); // the outline is served; the first lesson is over the limit
  await expect(page.getByTestId("generate-error")).toHaveText(/run out of credit or reached its usage limit/);
  await resetSimulator(page);

  // 4. An injected timeout: the simulator hangs past the app's client timeout.
  const timeout = (await aiState(page)).client_timeout_secs;
  test.skip(timeout === null || timeout > 20, "the server's SKRONGLE_AI_TIMEOUT_SECS is unset or long (scripts/e2e.sh sets 8)");
  await page.goto("/dev/ai");
  await learner.getByTestId("dev-ai-fault").selectOption("timeout");
  await learner.getByTestId("dev-ai-hang").fill(String(timeout! + 4));
  await learner.getByTestId("dev-ai-inject").click();
  await expect(learner.getByTestId("dev-ai-injected")).toHaveText(`hang ${timeout! + 4} s, next request only`);
  await page.goto("/courses");
  await generateViaSimulator(page, "Fractions that time out");
  expect(await generationEnds(page, 60_000)).toBe("failed");
  await expect(page.getByTestId("generate-error")).toHaveText(/didn't answer in time/);
  // One-shot: used up.
  await expect.poll(async () => (await aiState(page)).profiles.learner.inject).toBeNull();
});

test("the wire API refuses unknown keys and answers the OpenAI format; the platform key has its own profile", async ({ page }) => {
  await devLogin(page);
  await resetSimulator(page);
  const bases = (await aiState(page)).base_urls;
  const body = { model: "gpt-5", messages: [{ role: "user", content: "Hola [fixture:default]" }] };
  const headers = { "x-skrongle-call": "purpose=tutor-turn" };
  let r = await page.request.post(`${bases.openai}/chat/completions`, { data: body, headers: { ...headers, authorization: "Bearer sk-not-a-dev-key" } });
  expect(r.status()).toBe(401);
  expect((await r.json()).error.code).toBe("invalid_api_key");
  r = await page.request.post(`${bases.openai}/chat/completions`, { data: body, headers: { ...headers, authorization: "Bearer sk-dev-platform" } });
  expect(r.status()).toBe(200);
  expect((await r.json()).choices[0].message.content).toContain("[[ask]]");
  const state = await aiState(page);
  expect(state.calls[0]).toMatchObject({ who: "platform", format: "openai", purpose: "tutor-turn", status: 200 });
  expect(state.calls[1]).toMatchObject({ who: "unknown", status: 401 });
  // Anthropic's format needs its version header, as the real API does.
  r = await page.request.post(`${bases.anthropic}/v1/messages`, { data: { ...body, max_tokens: 10 }, headers: { "x-api-key": "sk-ant-dev-platform" } });
  expect(r.status()).toBe(400);
  expect((await r.json()).error.type).toBe("invalid_request_error");
  // The platform key over its limit: OpenAI's insufficient_quota.
  expect((await page.request.post("/dev/ai/profiles/platform", { form: { do: "exhaust" }, maxRedirects: 0 })).status()).toBe(303);
  r = await page.request.post(`${bases.openai}/chat/completions`, { data: body, headers: { ...headers, authorization: "Bearer sk-dev-platform" } });
  expect(r.status()).toBe(429);
  expect((await r.json()).error.type).toBe("insufficient_quota");
  expect((await page.request.post("/dev/ai/profiles/platform", { form: { do: "reset" }, maxRedirects: 0 })).status()).toBe(303);
});

test("Manual replies: a request waits, folded at the bottom of /dev/ai, for the answer typed there; an ERROR: answer is the provider's error", async ({ page }) => {
  test.setTimeout(90_000);
  await devLogin(page, "keys=fake&next=/dev/ai");
  await resetSimulator(page);
  await page.goto("/dev/ai");
  // /dev/llm is gone: the held requests live on this page.
  expect((await page.request.get("/dev/llm")).status()).toBe(404);
  const held = page.getByTestId("dev-ai-held");
  const toggle = page.getByTestId("dev-ai-hold-toggle");
  await expect(page.getByTestId("dev-ai-hold-state")).toHaveText("off");
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(held).toBeHidden();
  // The pill flips in place, and the section appears.
  await toggle.click();
  await expect(page.getByTestId("dev-ai-hold-state")).toHaveText("on");
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("dev-ai-hold-form")).toHaveAttribute("data-holding", "true");
  await expect(held).toBeVisible();
  expect((await aiState(page)).holding).toBe(true);

  await page.goto("/courses");
  await generateViaSimulator(page, "Fractions, answered by hand");
  // The outline request lands at the bottom of /dev/ai as the page polls: folded, with the wish inside.
  await page.goto("/dev/ai");
  const pending = page.locator('[data-testid="dev-ai-request"][data-purpose="generate-outline"][data-pending="true"]').first();
  await expect(pending).toBeVisible({ timeout: 20_000 });
  await expect(pending).toHaveJSProperty("open", false);
  await expect(page.getByTestId("dev-ai-held-link")).toContainText("waiting");
  expect((await aiState(page)).held.some((h) => h.pending)).toBe(true);
  await pending.locator("summary").first().click();
  await expect(pending).toContainText("Fractions, answered by hand");
  // The reply box is folded too.
  const answer = pending.getByTestId("dev-ai-answer");
  await expect(answer).toHaveJSProperty("open", false);
  await answer.locator("summary").click();
  await pending.getByTestId("dev-ai-answer-input").fill("ERROR: the API answered HTTP 429. rate limited");
  await pending.getByTestId("dev-ai-answer-submit").click();
  // Answered in place, no reload ...
  await expect(page.locator('[data-testid="dev-ai-request"][data-purpose="generate-outline"]').first()).toHaveAttribute("data-pending", "false");
  // ... and the generation stops with the provider's own words, as a fixture's error would.
  await expect.poll(async () => (await aiState(page)).calls.find((c) => c.purpose === "generate-outline")?.status, { timeout: 20_000 }).toBe(429);
  const call = (await aiState(page)).calls.find((c) => c.purpose === "generate-outline")!;
  expect(call.outcome).toContain("answered by hand");

  // Off again: the fixtures answer, and the section folds away.
  await toggle.click();
  await expect(page.getByTestId("dev-ai-hold-state")).toHaveText("off");
  await expect(held).toBeHidden();
});

test("leaving the tutor while its first turn waits on the model does not lock the tutor (#659)", async ({ page }) => {
  test.setTimeout(90_000);
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await resetSimulator(page);
  expect((await page.request.post("/dev/ai/hold", { form: { do: "on" }, maxRedirects: 0 })).status()).toBe(303);
  // The first turn reaches the model and waits there; the learner goes back.
  const start = page.waitForRequest((r) => r.url().includes("/api/tutor/start"));
  await page.goto("/tutor", { waitUntil: "commit" });
  await start;
  let stem = "";
  await expect.poll(async () => (stem = (await aiState(page)).held.find((h) => h.pending && h.stem.includes("tutor-turn"))?.stem ?? ""), { timeout: 20_000 }).not.toBe("");
  await page.goto("/courses");
  // The abandoned call ends; nobody is waiting for its answer.
  expect((await page.request.post(`/dev/ai/held/${stem}`, { form: { reply: "ERROR: the API answered HTTP 500. gone" }, maxRedirects: 0 })).status()).toBe(303);
  expect((await page.request.post("/dev/ai/hold", { form: { do: "off" }, maxRedirects: 0 })).status()).toBe(303);
  // Back on the tutor: a greeting, not "the tutor is still answering".
  await page.goto("/tutor");
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible();
});

test("a learner who leaves the tutor mid-turn twice has one call with the model, not two (#848)", async ({ page }) => {
  test.setTimeout(90_000);
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await resetSimulator(page);
  expect((await page.request.post("/dev/ai/hold", { form: { do: "on" }, maxRedirects: 0 })).status()).toBe(303);
  const pending = async () => (await aiState(page)).held.filter((h) => h.pending && h.stem.includes("tutor-turn"));
  // First visit: the opening turn reaches the model and waits; the learner goes back.
  const start = page.waitForRequest((r) => r.url().includes("/api/tutor/start"));
  await page.goto("/tutor", { waitUntil: "commit" });
  await start;
  await expect.poll(async () => (await pending()).length, { timeout: 20_000 }).toBe(1);
  await page.goto("/courses");
  // Second visit: refused as still answering while the first call runs.
  const second = page.waitForResponse((r) => r.url().includes("/api/tutor/start"));
  await page.goto("/tutor");
  expect((await second).status()).toBe(429);
  await page.goto("/courses");
  const held = await pending();
  expect(held.length).toBe(1);
  // Clean up: the abandoned call ends.
  expect((await page.request.post(`/dev/ai/held/${held[0].stem}`, { form: { reply: "ERROR: the API answered HTTP 500. gone" }, maxRedirects: 0 })).status()).toBe(303);
  expect((await page.request.post("/dev/ai/hold", { form: { do: "off" }, maxRedirects: 0 })).status()).toBe(303);
});

test("coming back while the tutor is still answering, Cancel lets go of the old reply and starts again (#890)", async ({ page }) => {
  test.setTimeout(90_000);
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await resetSimulator(page);
  await page.evaluate(() => sessionStorage.clear());
  expect((await page.request.post("/dev/ai/hold", { form: { do: "on" }, maxRedirects: 0 })).status()).toBe(303);
  // Only this test's requests: an earlier run may have left some waiting.
  const before = new Set((await aiState(page)).held.map((h) => h.stem));
  const pending = async () => (await aiState(page)).held.filter((h) => h.pending && h.stem.includes("tutor-turn") && !before.has(h.stem));
  const start = page.waitForRequest((r) => r.url().includes("/api/tutor/start"));
  await page.goto("/tutor", { waitUntil: "commit" });
  await start;
  await expect.poll(async () => (await pending()).length, { timeout: 20_000 }).toBe(1);
  await page.goto("/courses");
  await page.goto("/tutor");
  const still = page.getByTestId("tutor-still-answering");
  await expect(still).toBeVisible({ timeout: 20_000 });
  await still.getByTestId("tutor-cancel").click();
  // The old call runs on with the model; a new one starts beside it.
  await expect.poll(async () => (await pending()).length, { timeout: 20_000 }).toBe(2);
  // One cancelled reply at a time: the new call cannot be cancelled while the old one runs.
  const again = await page.request.post("/api/tutor/cancel", { data: {} });
  expect(again.status()).toBe(429);
  const [old, fresh] = (await pending()).sort((a, b) => a.stem.localeCompare(b.stem));
  // The cancelled call ends first and its reply goes nowhere; the next test may cancel again.
  expect((await page.request.post(`/dev/ai/held/${old.stem}`, { form: { reply: "This late reply is thrown away." }, maxRedirects: 0 })).status()).toBe(303);
  await expect.poll(async () => (await aiState(page)).calls.some((c) => c.fixture === `answered by hand: ${old.stem}`), { timeout: 20_000 }).toBe(true);
  expect((await page.request.post(`/dev/ai/held/${fresh.stem}`, { form: { reply: "¡Hola! Empecemos." }, maxRedirects: 0 })).status()).toBe(303);
  await expect(page.locator("#thread .msg.tutor").first()).toContainText("Empecemos", { timeout: 20_000 });
  await expect(page.locator("#thread")).not.toContainText("thrown away");
  await expect(still).toBeHidden();
  expect((await page.request.post("/dev/ai/hold", { form: { do: "off" }, maxRedirects: 0 })).status()).toBe(303);
});

test("a slow reply gets a Cancel after ten seconds, and the learner can carry on (#890)", async ({ page }) => {
  test.setTimeout(90_000);
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await resetSimulator(page);
  await page.evaluate(() => sessionStorage.clear());
  // Model calls give up after 8 s under e2e.sh: jump the page's clock to the Cancel instead of waiting.
  await page.clock.install();
  await page.goto("/tutor");
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible({ timeout: 20_000 });
  expect((await page.request.post("/dev/ai/hold", { form: { do: "on" }, maxRedirects: 0 })).status()).toBe(303);
  await page.locator("#say").fill("¿Qué significa hola?");
  const cancel = page.locator(".typing").getByTestId("tutor-cancel");
  // The held call gives up after 8 s of real time: no polling before the Cancel.
  const sent = page.waitForRequest((r) => r.url().includes("/api/tutor/turn"));
  await page.locator("#send").click();
  await sent;
  await expect(cancel).toBeHidden();
  await page.clock.fastForward(10_000);
  await cancel.click({ timeout: 3_000 });
  await expect(page.locator("#thread .msg.system").last()).toHaveText("Reply cancelled.");
  await expect(page.locator(".typing")).toHaveCount(0);
  await expect(page.locator("#send")).toBeEnabled();
  // The cancelled call is still with the model.
  expect((await aiState(page)).held.some((h) => h.pending && h.stem.includes("tutor-turn"))).toBe(true);
  const held = (await aiState(page)).held.filter((h) => h.pending && h.stem.includes("tutor-turn"));
  for (const h of held) {
    expect((await page.request.post(`/dev/ai/held/${h.stem}`, { form: { reply: "ERROR: the API answered HTTP 500. gone" }, maxRedirects: 0 })).status()).toBe(303);
  }
  expect((await page.request.post("/dev/ai/hold", { form: { do: "off" }, maxRedirects: 0 })).status()).toBe(303);
});

test("in another language, coming back while the tutor is still answering also offers Cancel (#890)", async ({ page, context }) => {
  test.setTimeout(90_000);
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await context.addCookies([{ name: "lang", value: "de", url: page.url() }]);
  await resetSimulator(page);
  await page.evaluate(() => sessionStorage.clear());
  expect((await page.request.post("/dev/ai/hold", { form: { do: "on" }, maxRedirects: 0 })).status()).toBe(303);
  const before = new Set((await aiState(page)).held.map((h) => h.stem));
  const pending = async () => (await aiState(page)).held.filter((h) => h.pending && h.stem.includes("tutor-turn") && !before.has(h.stem));
  const start = page.waitForRequest((r) => r.url().includes("/api/tutor/start"));
  await page.goto("/tutor", { waitUntil: "commit" });
  await start;
  await expect.poll(async () => (await pending()).length, { timeout: 20_000 }).toBe(1);
  await page.goto("/courses");
  await page.goto("/tutor");
  // The refusal is in German; the page knows it by its status, not its English text.
  const still = page.getByTestId("tutor-still-answering");
  await expect(still).toContainText("der Tutor antwortet noch", { timeout: 20_000 });
  await expect(still.getByTestId("tutor-cancel")).toBeVisible();
  // Clean up: no more waiting, then the abandoned call ends.
  expect((await page.request.post("/dev/ai/hold", { form: { do: "off" }, maxRedirects: 0 })).status()).toBe(303);
  for (const h of await pending()) {
    expect((await page.request.post(`/dev/ai/held/${h.stem}`, { form: { reply: "ERROR: the API answered HTTP 500. gone" }, maxRedirects: 0 })).status()).toBe(303);
  }
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible({ timeout: 20_000 });
});

test("coming back to the tutor in the same tab after a failed first turn starts again (#932)", async ({ page }) => {
  test.setTimeout(90_000);
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await resetSimulator(page);
  await page.evaluate(() => sessionStorage.clear());
  expect((await page.request.post("/dev/ai/hold", { form: { do: "on" }, maxRedirects: 0 })).status()).toBe(303);
  await page.goto("/tutor");
  let stem = "";
  await expect.poll(async () => (stem = (await aiState(page)).held.find((h) => h.pending && h.stem.includes("tutor-turn"))?.stem ?? ""), { timeout: 20_000 }).not.toBe("");
  // The first turn fails while the learner watches; the page keeps the session id.
  expect((await page.request.post(`/dev/ai/held/${stem}`, { form: { reply: "ERROR: the API answered HTTP 500. gone" }, maxRedirects: 0 })).status()).toBe(303);
  await expect(page.locator("#thread .msg.system.error")).toBeVisible({ timeout: 20_000 });
  await expect(page.locator("#thread .msg.tutor")).toHaveCount(0);
  expect(await page.evaluate(() => sessionStorage.getItem("tutor-session"))).not.toBeNull();
  expect((await page.request.post("/dev/ai/hold", { form: { do: "off" }, maxRedirects: 0 })).status()).toBe(303);
  // Same tab, back on the tutor: a greeting, not an empty thread.
  await page.goto("/tutor");
  await expect(page.locator("#thread .msg.tutor").first()).toBeVisible({ timeout: 20_000 });
});

test("each profile's keys are copy buttons in their provider's colours", async ({ page, context }) => {
  await devLogin(page, "next=/dev/ai");
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  const keys = (who: string) => page.locator(`[data-testid="dev-ai-profile"][data-who="${who}"] [data-testid="dev-ai-key"]`);
  await expect(keys("platform")).toHaveCount(2);
  expect(await keys("platform").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.provider))).toEqual(["anthropic", "openai"]);
  expect(await keys("learner").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.provider))).toEqual(["anthropic", "openai", "google"]);
  // Branded: Anthropic's clay, not the app's accent.
  const anthropic = keys("learner").first();
  expect(await anthropic.evaluate((e) => getComputedStyle(e).backgroundColor)).toBe("rgb(217, 119, 87)");

  await anthropic.click();
  await expect(anthropic).toHaveText("Copied");
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe("sk-ant-dev-anthropic");
  await expect(anthropic).toHaveText("Claude (Anthropic)");
  await keys("learner").nth(2).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe("AIza-dev-google");
});
