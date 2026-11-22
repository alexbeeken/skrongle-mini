// Giving a course (#220): the dev user (an owner, so it holds students)
// makes a course and two students, ticks both on Students and gives the
// course; both rows name it under Courses and the Share dialog does not
// list them. A student signs in with JavaScript off and the one subject
// opens straight away; My subjects shows its card. Taken away on the
// student's page, the subjects page says none is given.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";

/** A new student of the signed-in dev user: its username and password. */
async function newStudent(page: Page): Promise<{ id: number; username: string; password: string }> {
  const res = await page.request.get("/dev/student?format=json");
  expect(res.status()).toBe(200);
  return res.json();
}

test("a teacher gives a course to two students, and a student opens it on sign-in", async ({ page, browser }) => {
  await devLogin(page);
  const courseId = await createCourse(page);
  const one = await newStudent(page);
  const two = await newStudent(page);

  await page.goto("/students");
  const row = (username: string) => page.getByTestId("students-row").filter({ has: page.getByTestId("students-username").getByText(username, { exact: true }) });
  await expect(row(one.username).getByTestId("students-courses")).toHaveText("None yet");
  await row(one.username).getByTestId("students-pick").check();
  await row(two.username).getByTestId("students-pick").check();
  await page.getByTestId("students-give-course").selectOption(courseId);
  await page.getByTestId("students-give").click();
  await expect(page.getByTestId("students-notice")).toContainText("to the ticked students (2)");
  const title = (await page.getByTestId("students-give-course").locator(`option[value="${courseId}"]`).textContent())?.trim() ?? "";
  for (const kid of [one, two]) {
    await expect(row(kid.username).getByTestId("students-courses")).toHaveText(title);
  }

  // The Share dialog lists links only: the students are not there.
  await page.goto(`/courses/${courseId}?share=1`);
  await expect(page.getByTestId("course-share-row")).toHaveCount(0);

  // The student, JavaScript off: its one subject opens on sign-in.
  const context = await browser.newContext({ javaScriptEnabled: false });
  const kid = await context.newPage();
  await kid.goto("/login");
  await kid.getByTestId("login-email").fill(one.username);
  await kid.getByTestId("login-password").fill(one.password);
  await kid.getByTestId("login-submit").click();
  await expect(kid).toHaveURL(/\/$/);
  await kid.goto("/courses?pick=1");
  await expect(kid.getByTestId("student-subjects-hi")).toHaveText(`Hi, ${one.username}`);
  const card = kid.getByTestId("student-subject");
  await expect(card).toHaveCount(1);
  await expect(card.getByTestId("student-subject-title")).toHaveText(title);
  await card.click();
  await expect(kid).toHaveURL(/\/$/);

  // Taken away on the student's page: no subject now.
  await page.goto("/students");
  await row(one.username).getByTestId("students-name").click();
  await page.getByTestId("students-course").and(page.locator(`[data-course="${courseId}"]`)).uncheck();
  await page.getByTestId("students-courses-save").click();
  await expect(page.getByTestId("students-notice")).toContainText("Courses saved.");
  await kid.goto("/courses");
  await expect(kid.getByTestId("student-subjects-none")).toBeVisible();
  await context.close();
});
