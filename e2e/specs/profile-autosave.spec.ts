// The profile saves as you edit (#157): no Save button; the circle by the
// heading is green when everything is saved, yellow while saving, red when
// something is not (a field that needs attention, or no connection), and
// leaving with unsaved changes asks first. The circle and a field's check
// show only after an edit, turn green 200–500 ms after the server answers,
// and fade out 5 s later (#249, #255).
import { expect, test, type Page } from "@playwright/test";
import { newAccount, register, type Account } from "../lib/learner";
import { mailLink, waitForMail } from "../lib/mail";

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

test("the phone number wears a +1 and punctuates the digits as they are typed (#496)", async ({ page }) => {
  // The field is locked until the e-mail address is confirmed (#431).
  await expect(page.getByTestId("profile-phone")).toBeDisabled();
  const mail = await waitForMail(account.email, /Confirm your e-mail address/);
  await page.goto(mailLink(mail, /\/verify-email\?/));
  await expect(page.getByTestId("verify-email-confirmed")).toBeVisible();
  await page.goto("/profile");

  const phone = page.getByTestId("profile-phone");
  // What is wrong is behind the red ! in the field's corner; the text under
  // the same name is for a screen reader, and takes no room on the page.
  const bad = page.getByTestId("profile-phone-invalid");
  const error = page.getByTestId("profile-phone-error");
  // The country code sits in the field, so nobody types it.
  await expect(page.getByTestId("profile-phone-country")).toHaveText("+1");
  await expect(phone).toHaveValue("");
  await expect(phone).toHaveAttribute("placeholder", "(555) 555 - 5555");

  // What goes to the server: the ten digits as E.164, whatever the field shows.
  const sent = new Promise<unknown>((resolve) => {
    page.on("request", (r) => {
      if (r.url().endsWith("/api/profile") && r.method() === "POST") resolve(JSON.parse(r.postData() || "{}").phone);
    });
  });

  // Digits only: the brackets, the space and the dash appear on their own.
  await phone.click();
  await page.keyboard.type("555");
  await expect(phone).toHaveValue("(555");
  await page.keyboard.type("555");
  await expect(phone).toHaveValue("(555) 555");
  // Half a number is not sent. Nothing is said about it while it is still
  // being typed: every number is too short before its last digit.
  await expect(bad).toBeHidden();
  await expect(status(page)).toHaveAttribute("data-state", "error");
  // Typing stops: the red ! comes, in the field's corner, and says what is
  // missing without covering what was typed or moving the page.
  await expect(bad).toBeVisible({ timeout: 4000 });
  await expect(bad).toHaveAttribute("data-tip", "A phone number has 10 digits, like (555) 555 - 5555.");
  await expect(error).toHaveText("A phone number has 10 digits, like (555) 555 - 5555.");
  await expect(page.getByTestId("profile-save-error")).toBeHidden();
  await expect(phone).toHaveValue("(555) 555");
  await page.keyboard.type("1234");
  await expect(phone).toHaveValue("(555) 555 - 1234");
  // Right again: the ! goes at once, without waiting for a break.
  await expect(bad).toBeHidden();
  await expect(error).toBeHidden();
  await phone.blur();
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  expect(await sent).toBe("+15555551234");

  // Stored as E.164, shown punctuated again on the next visit.
  await page.reload();
  await expect(phone).toHaveValue("(555) 555 - 1234");

  // Pasted with the country code and the punctuation of the day: the same ten digits.
  await phone.fill("+1 (555) 555.6789");
  await expect(phone).toHaveValue("(555) 555 - 6789");
  await expect(status(page)).toHaveAttribute("data-state", "saved");

  // A caret in the middle: the digit lands where it was typed, not at the end.
  await phone.focus();
  await phone.evaluate((el: HTMLInputElement) => el.setSelectionRange(2, 2));
  await page.keyboard.type("0");
  await expect(phone).toHaveValue("(505) 555 - 5678");
  await expect(bad).toBeHidden();
  expect(await phone.evaluate((el: HTMLInputElement) => el.selectionStart)).toBe(3);

  // An area code that starts with 0 or 1 is not a number anyone can be texted on.
  // Leaving the field is a break too: the ! does not wait for the 2 s.
  await phone.fill("1555551234");
  await expect(phone).toHaveValue("(155) 555 - 1234");
  await phone.blur();
  await expect(bad).toBeVisible();
  await expect(status(page)).toHaveAttribute("data-state", "error");
  // The ! holds the message, in its tip and in the dialog it opens.
  await bad.click();
  await expect(page.getByTestId("phone-problem-why")).toHaveText("Neither the area code nor the three digits after it start with 0 or 1.");
  await page.getByTestId("phone-problem-close").click();
  await expect(page.getByTestId("phone-problem-dialog")).toBeHidden();

  // Another country's number is left as it is and named, not reshaped into a +1 one.
  await phone.fill("+44 7700 900123");
  await expect(phone).toHaveValue("+44 7700 900123");
  await phone.blur();
  await expect(bad).toHaveAttribute("data-tip", "Only US and Canada numbers for now. Type the 10 digits after the +1.");
  await expect(error).toHaveText("Only US and Canada numbers for now. Type the 10 digits after the +1.");
  await expect(status(page)).toHaveAttribute("data-state", "error");

  // Emptied, the number is removed.
  await phone.fill("");
  await expect(bad).toBeHidden();
  await expect(error).toBeHidden();
  await expect(status(page)).toHaveAttribute("data-state", "saved");
  await page.reload();
  await expect(phone).toHaveValue("");
});

test("the profile has no picture and no theme: both are a course's settings", async ({ page }) => {
  await expect(page.getByTestId("profile-avatar-shown")).toHaveCount(0);
  await expect(page.locator('[data-testid="profile-theme"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="profile-nav-link"][data-section="theme"]')).toHaveCount(0);
  expect((await page.request.post("/api/profile", { data: { theme: "realm" } })).status()).toBe(422);
});

test("API keys: folded away, a pasted key saves at once, a typed one 3 s after typing stops", async ({ page }) => {
  const row = (p: string) => page.locator(`[data-testid="profile-api-key"][data-provider="${p}"]`);
  const input = (p: string) => row(p).getByTestId("profile-api-key-input");
  await expect(input("anthropic")).toBeHidden(); // all three start closed
  await expect(input("openai")).toBeHidden();
  await expect(page.getByTestId("profile-api-key-save")).toHaveCount(0);

  // Pasted (many characters at once): saved straight away.
  await row("anthropic").getByTestId("profile-api-key-toggle").click();
  const start = Date.now();
  await input("anthropic").fill("sk-ant-test-paste-ABCD");
  await expect(row("anthropic")).toHaveAttribute("data-key-set", "true");
  expect(Date.now() - start).toBeLessThan(2500);
  await expect(row("anthropic").getByTestId("profile-api-key-status")).toContainText("…ABCD");
  await expect(input("anthropic")).toHaveValue("");
  await expect(row("anthropic").locator(".field-wrap")).toHaveAttribute("data-saved", "");

  // Typed: nothing for a while after the last keystroke, then saved.
  await row("openai").getByTestId("profile-api-key-toggle").click();
  await input("openai").pressSequentially("sk-typed-WXYZ", { delay: 30 });
  await page.waitForTimeout(2000);
  await expect(row("openai")).toHaveAttribute("data-key-set", "false");
  await expect(status(page)).toHaveAttribute("data-state", "saving");
  await expect(row("openai")).toHaveAttribute("data-key-set", "true");
  await expect(status(page)).toHaveAttribute("data-state", "saved");

  // A key for the wrong box: red under the field, nothing stored.
  await row("google").getByTestId("profile-api-key-toggle").click();
  await input("google").fill("sk-ant-wrong-box-1234");
  await expect(row("google").getByTestId("profile-api-key-error")).toBeVisible();
  await expect(status(page)).toHaveAttribute("data-state", "error");
  await expect(row("google")).toHaveAttribute("data-key-set", "false");
});
