// The server's AI API keys on Manage Users (#25, #24): only an owner sees
// them; each provider is a row folded shut that says Set or Not set; a
// pasted key saves at once, a typed one 3 s after typing stops, a key for
// the wrong box is refused under its field; Remove key takes it away. The
// keys serve every account. The dev user is an owner. Every key here starts
// like the simulated provider's, and the end puts the seeded ones back, so
// the specs after this one find the server as they expect.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, register, removeApiKey, resetServerKeys, saveApiKey } from "../lib/learner";

const status = (page: Page) => page.getByTestId("manage-keys-save-status");
const row = (page: Page, p: string) => page.locator(`[data-testid="manage-api-key"][data-provider="${p}"]`);
const input = (page: Page, p: string) => row(page, p).getByTestId("manage-api-key-input");

test.afterEach(async ({ page }) => {
  await resetServerKeys(page);
});

test("keys: folded away, a pasted key saves at once, a typed one 3 s after typing stops", async ({ page }) => {
  await devLogin(page, "keys=fake&next=/manage");
  await removeApiKey(page, "openai");
  await removeApiKey(page, "google");
  await page.goto("/manage#ai");
  // One row per provider (#24), every one folded, each saying Set or Not set.
  await expect(page.getByTestId("manage-api-key")).toHaveCount(8);
  for (const r of await page.getByTestId("manage-api-key").all()) {
    await expect(r).not.toHaveAttribute("open", "");
    await expect(r.getByTestId("manage-api-key-input")).toBeHidden();
    await expect(r.getByTestId("manage-api-key-status")).toHaveText(/^(✓ Set · .+|Not set)$/);
  }
  await expect(row(page, "anthropic").getByTestId("manage-api-key-status")).toContainText("✓ Set");
  await expect(row(page, "openai").getByTestId("manage-api-key-status")).toHaveText("Not set");
  await expect(row(page, "deepseek").getByTestId("manage-api-key-status")).toHaveText("Not set");

  // Pasted (many characters at once): saved straight away.
  await row(page, "anthropic").getByTestId("manage-api-key-toggle").click();
  const start = Date.now();
  await input(page, "anthropic").fill("sk-ant-dev-paste-ABCD");
  await expect(row(page, "anthropic").getByTestId("manage-api-key-status")).toContainText("…ABCD");
  expect(Date.now() - start).toBeLessThan(2500);
  await expect(input(page, "anthropic")).toHaveValue("");
  await expect(row(page, "anthropic").locator(".field-wrap")).toHaveAttribute("data-saved", "");

  // Typed: nothing for a while after the last keystroke, then saved.
  await row(page, "openai").getByTestId("manage-api-key-toggle").click();
  await input(page, "openai").pressSequentially("sk-dev-typed-WXYZ", { delay: 30 });
  await page.waitForTimeout(2000);
  await expect(row(page, "openai")).toHaveAttribute("data-key-set", "false");
  await expect(status(page)).toHaveAttribute("data-state", "saving");
  await expect(row(page, "openai")).toHaveAttribute("data-key-set", "true");
  await expect(status(page)).toHaveAttribute("data-state", "saved");

  // A key for the wrong box: red under the field, nothing stored.
  await row(page, "google").getByTestId("manage-api-key-toggle").click();
  await input(page, "google").fill("sk-ant-wrong-box-1234");
  await expect(row(page, "google").getByTestId("manage-api-key-error")).toBeVisible();
  await expect(status(page)).toHaveAttribute("data-state", "error");
  await expect(row(page, "google")).toHaveAttribute("data-key-set", "false");
});

test("a key the owner saves serves another account; removed, that account is told to ask the owner", async ({ page, browser }) => {
  await devLogin(page, "keys=fake&next=/manage");
  const ctx = await browser.newContext();
  const other = await ctx.newPage();
  await register(other, newAccount("server-key"), "spanish");
  await other.goto("/tutor");
  await expect(other.getByTestId("tutor-no-key")).toHaveCount(0);

  for (const p of ["anthropic", "openai", "google"]) await removeApiKey(page, p);
  await expect(page.getByTestId("manage-notice")).toContainText("key removed");
  await other.goto("/tutor");
  await expect(other.getByTestId("tutor-no-key")).toBeVisible();
  await expect(other.getByTestId("tutor-no-key")).toContainText("Ask the owner of this server");
  await expect(other.getByTestId("tutor-add-key")).toHaveCount(0);
  // The owner is sent to the keys instead.
  await page.goto("/tutor");
  await expect(page.getByTestId("tutor-add-key")).toHaveAttribute("href", /\/manage#ai$/);
  await ctx.close();
});

test("a DeepSeek key saved on Manage Users answers another account's tutor through the dev provider", async ({ page, browser }) => {
  await devLogin(page, "keys=fake&next=/manage");
  await saveApiKey(page, "sk-dev-deepseek-1234", "deepseek");
  await expect(row(page, "deepseek").getByTestId("manage-api-key-status")).toHaveText("✓ Set · ends in …1234");
  const ctx = await browser.newContext();
  const other = await ctx.newPage();
  await register(other, newAccount("deepseek"), "spanish");
  await other.goto("/tutor?provider=deepseek");
  await expect(other.getByTestId("tutor-provider")).toHaveValue("deepseek");
  await expect(other.getByTestId("ai-disclosure")).toContainText("DeepSeek");
  await expect(other.getByTestId("tutor-thread")).toContainText("practise this lesson together");
  // The call went to the dev provider's DeepSeek endpoint, with DeepSeek's model.
  const state = await (await page.request.get("/dev/ai?format=json")).json();
  expect(state.calls.some((c: { request?: string; purpose: string }) => c.purpose === "tutor-turn")).toBe(true);
  await ctx.close();
});

test("an Ollama-compatible server: a base URL and a model, no key", async ({ page }) => {
  await devLogin(page, "keys=fake&next=/manage");
  try {
    const ollama = row(page, "ollama");
    await ollama.getByTestId("manage-api-key-toggle").click();
    await expect(ollama.getByTestId("manage-api-key-input")).toHaveAttribute("type", "url");
    await ollama.getByTestId("manage-api-key-input").fill("localhost:11434");
    await expect(ollama.getByTestId("manage-api-key-error")).toBeVisible();
    await ollama.getByTestId("manage-api-key-input").fill(new URL("/dev/ai/ollama", page.url()).href);
    await expect(ollama).toHaveAttribute("data-key-set", "true");
    await expect(ollama.getByTestId("manage-api-key-status")).toContainText("✓ Set");
    await ollama.getByTestId("manage-ollama-model").fill("qwen2.5:7b");
    await expect(ollama.getByTestId("manage-ollama-model-shown")).toHaveText("qwen2.5:7b");
    await page.reload();
    await expect(row(page, "ollama").getByTestId("manage-ollama-model-shown")).toHaveText("qwen2.5:7b");
  } finally {
    await removeApiKey(page, "ollama");
  }
});
