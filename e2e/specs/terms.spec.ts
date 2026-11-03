// Terms of Service (#72): the public page, the checkbox at registration and
// the notice for an account that accepted an older version (#966): a
// material change blocks until accepted, a minor one can be dismissed.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { login, newAccount } from "../lib/learner";

test("the terms page is public and says when it was last updated", async ({ page }) => {
  const r = await page.goto("/terms");
  expect(r?.status()).toBe(200);
  await expect(page.getByTestId("terms-page")).toContainText("Terms and Conditions");
  await expect(page.getByTestId("terms-page")).toContainText("Last updated:");
  expect((await page.request.get("/terms.md")).status()).toBe(200);
  // Linked from the footer and the login page, logged out.
  await page.goto("/login");
  await expect(page.getByTestId("footer-terms")).toHaveAttribute("href", /\/terms$/);
  await expect(page.getByTestId("login-terms-link")).toHaveAttribute("href", /\/terms$/);
});

test("registration needs the terms checkbox, and records the acceptance", async ({ page }) => {
  const a = newAccount("terms");
  await page.goto("/register");
  await page.getByTestId("register-email").fill(a.email);
  await page.getByTestId("register-password").fill(a.password);
  await expect(page.getByTestId("register-age")).not.toBeChecked();
  await expect(page.getByTestId("register-terms")).not.toBeChecked();

  // The browser's own check (required) is skipped: the server must refuse too.
  const refused = await page.request.post("/register", {
    form: { email: a.email, password: a.password },
    maxRedirects: 0,
  });
  expect(refused.status()).toBe(200);
  expect(await refused.text()).toContain('data-testid="register-error"');

  await page.getByTestId("register-age").check();

  await page.getByTestId("register-terms").check();
  await page.getByTestId("register-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await page.goto("/profile");
  const version = await page.request.get("/terms").then(async (r) => (await r.text()).match(/data-terms-version="([^"]+)"/)![1]);
  await expect(page.getByTestId("profile-terms")).toHaveAttribute("data-terms-version", version);
});

test("a material change: the notice cannot be dismissed and blocks the API until accepted", async ({ page }) => {
  await page.goto("/dev/login?open=spanish&terms=old&next=/lessons");
  await expect(page).toHaveURL(/\/lessons$/);
  const notice = page.getByTestId("terms-gate");
  await expect(notice).toBeVisible();
  await expect(notice).toHaveAttribute("data-tier", "material");
  await expect(page.getByTestId("terms-notice-summary").first()).not.toBeEmpty();
  await expect(page.getByTestId("terms-gate-link")).toHaveAttribute("href", /\/terms$/);
  await expect(page.getByTestId("terms-notice-dismiss")).toHaveCount(0);
  await expect(page.getByTestId("terms-gate-logout")).toBeVisible();
  // Escape does not close it, and it is on every page.
  await page.keyboard.press("Escape");
  await expect(notice).toBeVisible();
  await page.goto("/courses");
  await expect(page.getByTestId("terms-gate")).toHaveAttribute("data-tier", "material");
  expect((await page.request.get("/api/progress")).status()).toBe(403);

  await page.goto("/lessons");
  await page.getByTestId("terms-accept-check").check();
  await page.getByTestId("terms-accept-submit").click();
  await expect(page).toHaveURL(/\/lessons$/);
  await expect(page.getByTestId("terms-gate")).toHaveCount(0);
  expect((await page.request.get("/api/progress")).status()).toBe(200);
  await page.goto("/profile");
  await expect(page.getByTestId("profile-terms")).not.toHaveAttribute("data-terms-version", "0-dev-old");
  // And the dev user is left as the other specs expect.
  await devLogin(page);
});

test("a minor change: the notice shows what changed and Continue closes it for good", async ({ page }) => {
  await page.goto("/dev/login?open=spanish&terms=minor&next=/lessons");
  await expect(page).toHaveURL(/\/lessons$/);
  const notice = page.getByTestId("terms-gate");
  await expect(notice).toBeVisible();
  await expect(notice).toHaveAttribute("data-tier", "minor");
  await expect(page.getByTestId("terms-notice-summary").first()).not.toBeEmpty();
  await expect(page.getByTestId("terms-gate-link")).toHaveAttribute("href", /\/terms$/);
  await expect(page.getByTestId("terms-accept-check")).toHaveCount(0);
  // It blocks nothing.
  expect((await page.request.get("/api/progress")).status()).toBe(200);

  const version = await notice.getAttribute("data-terms-version");
  await page.getByTestId("terms-notice-dismiss").click();
  await expect(page).toHaveURL(/\/lessons$/);
  await expect(page.getByTestId("terms-gate")).toHaveCount(0);
  await page.goto("/courses");
  await expect(page.getByTestId("terms-gate")).toHaveCount(0);
  // Dismissing accepted the current version.
  await page.goto("/profile");
  await expect(page.getByTestId("profile-terms")).toHaveAttribute("data-terms-version", version!);
  await devLogin(page);
});

test("the profile's read link shows the terms in a dialog and closes back to the profile (#152)", async ({ page }) => {
  await devLogin(page);
  await page.goto("/profile");
  const link = page.getByTestId("profile-terms-link");
  await expect(link).toHaveAttribute("href", /\/terms$/);
  await link.click();
  await expect(page).toHaveURL(/\/profile$/);
  const dialog = page.getByTestId("profile-terms-dialog");
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId("profile-terms-text")).toContainText("Terms and Conditions");
  await page.getByTestId("profile-terms-close").click();
  await expect(dialog).toBeHidden();
  // Escape closes it too.
  await link.click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});
