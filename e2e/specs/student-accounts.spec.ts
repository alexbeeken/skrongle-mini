// Student accounts (#217): a teacher's student signs in with a username
// (in any case) and a password, has a profile with its username, language
// and password only, and gets "not found" for what is not a student's. The
// dev user (an owner) makes one with /dev/student; Manage Users offers the
// Hold students permission and counts the dev user's students.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, signUp } from "../lib/learner";

test("a student signs in with its username and is refused what is not a student's", async ({ page, browser }) => {
  await devLogin(page);
  await page.goto("/dev/student");
  const username = (await page.getByTestId("dev-student-username").textContent())?.trim() ?? "";
  const password = (await page.getByTestId("dev-student-password").textContent())?.trim() ?? "";
  expect(username).toMatch(/^student-\d+-\d+$/);
  expect(password).toBe("dev-password");

  const kid = await (await browser.newContext()).newPage();
  await kid.goto("/login");
  await expect(kid.getByTestId("login-student-note")).toContainText("asks their teacher");
  await kid.getByTestId("login-email").fill(username.toUpperCase());
  await kid.getByTestId("login-password").fill(password);
  await kid.getByTestId("login-submit").click();
  await expect(kid).not.toHaveURL(/\/login/);
  // No subject yet: its subjects page, in the student frame (#220).
  await expect(kid.getByTestId("student-subjects-none")).toBeVisible();

  for (const path of ["/courses/new", "/tutor", "/manage"]) {
    const res = await kid.goto(path);
    expect(res?.status(), path).toBe(404);
  }
  expect((await kid.request.get("/api/tutor/session")).status()).toBe(404);

  await kid.goto("/profile");
  await expect(kid.getByTestId("profile-username")).toHaveValue(username);
  await expect(kid.getByTestId("profile-lang")).toBeVisible();
  await expect(kid.getByTestId("profile-password-open")).toBeVisible();
  for (const gone of ["profile-email", "profile-first-name", "profile-two-factor", "profile-apps"]) {
    await expect(kid.getByTestId(gone), gone).toHaveCount(0);
  }

  // A wrong password is the login page again, with its error.
  const other = await (await browser.newContext()).newPage();
  await other.goto("/login");
  await other.getByTestId("login-email").fill(username);
  await other.getByTestId("login-password").fill("not-the-password");
  await other.getByTestId("login-submit").click();
  await expect(other.getByTestId("login-error")).toBeVisible();
});

test("Manage Users offers Hold students and counts a teacher's students", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("hold-students");
  await signUp(them, a);

  await devLogin(page);
  await page.goto("/dev/student");
  await expect(page.getByTestId("dev-student-username")).toBeVisible();
  await page.goto("/manage");
  const dev = page.getByTestId("manage-account-row").filter({ hasText: "dev@example.com" });
  await expect(dev.getByTestId("manage-account-students")).toContainText("Holds");

  const row = page.getByTestId("manage-account-row").filter({ hasText: a.email.toLowerCase() });
  await row.getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator('[data-perm="hold_students"]')).toContainText("Hold students");
});
