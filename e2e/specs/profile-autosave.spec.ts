// The profile saves as you edit (#157): no Save button; the circle by the
// heading is green when everything is saved, yellow while saving, red when
// something is not (a field that needs attention, or no connection), and
// leaving with unsaved changes asks first. The circle and a field's check
// show only after an edit, turn green 200–500 ms after the server answers,
// and fade out 5 s later (#249, #255).
import { expect, test, type Page } from "@playwright/test";
import { newAccount, register, type Account } from "../lib/learner";

const status = (page: Page) => page.getByTestId("profile-save-status");

let account: Account;

test.beforeEach(async ({ page }) => {
  account = newAccount("autosave");
  await register(page, account, "spanish");
  await page.goto("/profile");
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  // Nothing edited yet: nothing to say.
  await expect(status(page)).toBeHidden();
});

test("the green checks come a moment after the save, fade after 5 s, and come back with the next edit", async ({ page }) => {
  const wrap = page.locator('.field-wrap:has([data-testid="profile-first-name"])');
  let answered = 0;
  page.on("response", (r) => {
    if (r.url().endsWith("/api/profile")) answered = Date.now();
  });
  await page.getByTestId("profile-first-name").fill("Ana");
  await page.getByTestId("profile-last-name").focus();
  await expect(status(page)).toBeVisible();
  await expect(status(page)).toHaveAttribute("data-state", "saving");
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  // Never before the server said so, and not the instant it did.
  expect(answered).toBeGreaterThan(0);
  expect(Date.now() - answered).toBeGreaterThanOrEqual(150);
  await expect(wrap).toHaveAttribute("data-saved", "");
  await expect(wrap).not.toHaveAttribute("data-faded");
  await expect(status(page)).not.toHaveAttribute("data-faded");

  // Five seconds on, both fade away.
  await expect(status(page)).toHaveAttribute("data-faded", "", { timeout: 8_000 });
  await expect(status(page)).toBeHidden();
  await expect(wrap).toHaveAttribute("data-faded", "");

  // The next edit brings them back.
  await page.getByTestId("profile-first-name").fill("Anabel");
  await expect(status(page)).toBeVisible();
  await expect(status(page)).not.toHaveAttribute("data-faded");
  await expect(wrap).not.toHaveAttribute("data-saved");
  await page.getByTestId("profile-last-name").focus();
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  await expect(wrap).toHaveAttribute("data-saved", "");
  await expect(wrap).not.toHaveAttribute("data-faded");

  // Another page and back: gone.
  await page.getByTestId("nav-account-toggle").click();
  await page.getByTestId("nav-docs").click();
  await expect(page).toHaveURL(/\/docs/);
  await page.goBack();
  await expect(page.getByTestId("profile-first-name")).toHaveValue("Anabel");
  await expect(status(page)).toBeHidden();
  await expect(wrap).not.toHaveAttribute("data-saved");
});

test("names save a second after typing stops, in one request, and the heading follows", async ({ page }) => {
  await expect(page.getByTestId("profile-form").getByRole("button", { name: /save/i })).toHaveCount(0);
  await expect(page.getByTestId("profile-display-name")).toHaveText("Learner");
  const posts: string[] = [];
  page.on("request", (r) => {
    if (r.url().endsWith("/api/profile")) posts.push(r.postData() ?? "");
  });

  await page.getByTestId("profile-first-name").pressSequentially("Ana", { delay: 50 });
  await expect(status(page)).toHaveAttribute("data-state", "saving");
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  expect(posts).toEqual(['{"first_name":"Ana"}']);
  await expect(page.getByTestId("profile-display-name")).toHaveText("Ana");
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("title", "Ana");
  // The field itself shows a check until it is edited again.
  const wrap = (testid: string) => page.locator(`.field-wrap:has([data-testid="${testid}"])`);
  await expect(wrap("profile-first-name")).toHaveAttribute("data-saved", "");
  await expect(wrap("profile-last-name")).not.toHaveAttribute("data-saved");
  await page.getByTestId("profile-first-name").press("End");
  await page.getByTestId("profile-first-name").press("Backspace");
  await expect(wrap("profile-first-name")).not.toHaveAttribute("data-saved");
  await page.getByTestId("profile-first-name").press("a");
  await expect(wrap("profile-first-name")).toHaveAttribute("data-saved", "");

  // Leaving the field saves straight away.
  await page.getByTestId("profile-last-name").fill("López");
  await page.getByTestId("profile-first-name").focus();
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  await page.reload();
  await expect(page.getByTestId("profile-first-name")).toHaveValue("Ana");
  await expect(page.getByTestId("profile-last-name")).toHaveValue("López");
  await expect(page.getByTestId("profile-display-name")).toHaveText("Ana López");
  // The address is shown, not edited.
  await expect(page.getByTestId("profile-email")).toHaveAttribute("readonly", "");
});

test("a field that needs attention turns red at once, is not sent, and leaving asks first", async ({ page }) => {
  const first = page.getByTestId("profile-first-name");
  let posts = 0;
  page.on("request", (r) => {
    if (r.url().endsWith("/api/profile")) posts++;
  });
  await first.click();
  // Past the 60-character limit (the browser's maxlength stops typing, not a paste by script).
  await first.evaluate((el: HTMLInputElement) => {
    el.value = "x".repeat(61);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await expect(status(page)).toHaveAttribute("data-state", "error");
  await expect(first).toHaveAttribute("aria-invalid", "");
  await expect(page.getByTestId("profile-first-name-error")).toHaveText("First name is too long (max 60 characters).");
  // The field says it itself, so nothing appears at the top of the page and
  // pushes the form down.
  await expect(page.getByTestId("profile-save-error")).toBeHidden();

  // A link in the page: the app's own dialog says why; Stay keeps the page.
  await page.getByTestId("nav-account-toggle").click();
  await page.getByTestId("nav-docs").click();
  const leave = page.getByTestId("autosave-leave-dialog");
  await expect(leave).toBeVisible();
  await expect(leave).toContainText("Some fields need attention and are not saved");
  await page.getByTestId("autosave-stay").click();
  await expect(page).toHaveURL(/\/profile$/);

  // Reloading gets the browser's own prompt.
  let prompted = false;
  page.once("dialog", async (d) => {
    prompted = d.type() === "beforeunload";
    await d.dismiss();
  });
  await page.evaluate(() => location.reload()); // dismissing cancels it
  await expect.poll(() => prompted).toBe(true);

  // Fixed, it saves, and the page lets go.
  await first.fill("Ana");
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  await expect(first).not.toHaveAttribute("aria-invalid");
  await expect(page.getByTestId("profile-first-name-error")).toBeHidden();
  await expect(page.getByTestId("profile-save-error")).toBeHidden();
  expect(posts).toBe(1);
  await page.getByTestId("nav-account-toggle").click();
  await page.getByTestId("nav-docs").click();
  await expect(page).toHaveURL(/\/docs/);
});

test("the server refusing a field marks it red with the server's reason", async ({ page }) => {
  await page.route("**/api/profile", (r) =>
    r.fulfill({ status: 422, contentType: "application/json", body: JSON.stringify({ errors: { last_name: "Last name contains characters that are not allowed." } }) }),
  );
  await page.getByTestId("profile-last-name").fill("Bad");
  await page.getByTestId("profile-first-name").focus();
  await expect(status(page)).toHaveAttribute("data-state", "error");
  await expect(page.getByTestId("profile-last-name-error")).toHaveText("Last name contains characters that are not allowed.");
  await expect(page.getByTestId("profile-last-name")).toHaveAttribute("aria-invalid", "");
});

test("without a connection the circle turns red, leaving asks, and it saves once back online", async ({ page, context }) => {
  await context.setOffline(true);
  await page.getByTestId("profile-first-name").fill("Offline");
  await page.getByTestId("profile-last-name").focus();
  await expect(status(page)).toHaveAttribute("data-state", "error");
  await expect(page.getByTestId("profile-save-error")).toHaveText(/Could not reach the server/);

  await page.getByTestId("nav-account-toggle").click();
  await page.getByTestId("nav-profile").click();
  await expect(page.getByTestId("autosave-leave-dialog")).toContainText("not saved yet");
  await page.getByTestId("autosave-stay").click();

  await context.setOffline(false);
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  await expect(page.getByTestId("profile-save-error")).toBeHidden();
  await page.reload();
  await expect(page.getByTestId("profile-first-name")).toHaveValue("Offline");
});

test("the profile has no picture and no theme", async ({ page }) => {
  await expect(page.getByTestId("profile-avatar-shown")).toHaveCount(0);
  await expect(page.locator('[data-testid="profile-nav-link"][data-section="theme"]')).toHaveCount(0);
  expect((await page.request.post("/api/profile", { data: { theme: "realm" } })).status()).toBe(422);
  expect((await page.request.post("/api/profile", { data: { avatar: "kai" } })).status()).toBe(422);
});

test("the profile has no API keys: they are the server's, on Manage Users", async ({ page }) => {
  await expect(page.locator('[data-testid^="profile-api-key"]')).toHaveCount(0);
  expect((await page.request.post("/api/profile", { data: { key_anthropic: "sk-ant-api03-abcdWXYZ" } })).status()).toBe(422);
});
