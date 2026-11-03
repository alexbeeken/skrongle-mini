// The server's AI API keys on Manage Users (#25): only an owner sees them;
// each provider is a row folded shut that says whether its key is set; a
// pasted key saves at once, a typed one 3 s after typing stops, a key for
// the wrong box is refused under its field; Remove key takes it away. The
// keys serve every account. The dev user is an owner. Every key here starts
// like the simulated provider's, and the end puts the seeded ones back, so
// the specs after this one find the server as they expect.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, register, removeApiKey, resetServerKeys } from "../lib/learner";

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
  await expect(input(page, "anthropic")).toBeHidden(); // all three start closed
  await expect(input(page, "openai")).toBeHidden();
  await expect(row(page, "openai").getByTestId("manage-api-key-status")).toHaveText("No key yet");

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
