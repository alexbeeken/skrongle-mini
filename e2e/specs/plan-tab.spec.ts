// The Plan tab (#222): the dev user (an owner, so it holds students) makes a
// course, gives it to two students, opens the course and clicks Plan. The
// week row has seven days, today outlined; the recommended day makes Study,
// Review and Play; Down and Up move a row; + Review and Remove; Assign says
// "Assigned to 2 students"; ‹ shows the earlier week with no buttons. The
// same adding and moving work with JavaScript off.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";

/** A new student of the signed-in dev user. */
async function newStudent(page: Page): Promise<{ id: number; username: string; password: string }> {
  const res = await page.request.get("/dev/student?format=json");
  expect(res.status()).toBe(200);
  return res.json();
}

test("a teacher plans and assigns a day with buttons, with JavaScript on and off", async ({ page, browser }) => {
  await devLogin(page);
  const courseId = await createCourse(page);
  const one = await newStudent(page);
  const two = await newStudent(page);

  await page.goto("/students");
  const row = (username: string) => page.getByTestId("students-row").filter({ has: page.getByTestId("students-username").getByText(username, { exact: true }) });
  await row(one.username).getByTestId("students-pick").check();
  await row(two.username).getByTestId("students-pick").check();
  await page.getByTestId("students-give-course").selectOption(courseId);
  await page.getByTestId("students-give").click();
  await expect(page.getByTestId("students-notice")).toContainText("(2)");

  // The student's page links the course's plan.
  await row(one.username).getByTestId("students-name").click();
  await page.getByTestId("students-course-plan").and(page.locator(`[data-course="${courseId}"]`)).click();
  await expect(page).toHaveURL(new RegExp(`/courses/${courseId}/daily$`));

  // From the course page's tab row.
  await page.goto(`/courses/${courseId}`);
  await expect(page.getByTestId("course-tab-editor")).toHaveAttribute("aria-current", "page");
  await page.getByTestId("course-tab-plan").click();
  await expect(page.getByTestId("course-tab-plan")).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("daily-day")).toHaveCount(7);
  await expect(page.locator('[data-testid="daily-day"][data-today="1"]')).toHaveCount(1);
  await expect(page.getByTestId("daily-assign")).toBeDisabled();

  const kinds = page.getByTestId("daily-activity-kind");
  await page.getByTestId("daily-recommended").click();
  await expect(kinds).toHaveText(["Study", "Review", "Play"]);
  await expect(page.getByTestId("daily-up").first()).toBeDisabled();
  await expect(page.getByTestId("daily-down").last()).toBeDisabled();
  await page.getByTestId("daily-down").first().click();
  await expect(kinds).toHaveText(["Review", "Study", "Play"]);
  await page.getByTestId("daily-up").nth(1).click();
  await expect(kinds).toHaveText(["Study", "Review", "Play"]);
  await page.getByTestId("daily-add-review").click();
  await expect(kinds).toHaveText(["Study", "Review", "Play", "Review"]);
  await page.getByTestId("daily-remove").last().click();
  await expect(kinds).toHaveText(["Study", "Review", "Play"]);

  await page.getByTestId("daily-assign").click();
  await expect(page.getByTestId("daily-assigned")).toHaveText("Assigned to 2 students");
  await expect(page.locator('[data-testid="daily-day"][data-today="1"] [data-testid="daily-day-state"]')).toHaveText("3 activities");

  // The week before: no buttons on any day.
  await page.getByTestId("daily-week-prev").click();
  await expect(page.getByTestId("daily-day")).toHaveCount(7);
  await expect(page.locator('[data-testid="daily-day"][data-today="1"]')).toHaveCount(0);
  for (const n of [0, 6]) {
    await page.getByTestId("daily-day").nth(n).click();
    await expect(page.getByTestId("daily-past")).toBeVisible();
    await expect(page.getByTestId("daily-page")).toHaveAttribute("data-editable", "false");
    await expect(page.getByTestId("daily-bar")).toHaveCount(0);
    await expect(page.getByTestId("daily-recommended")).toHaveCount(0);
  }

  // JavaScript off: adding and moving a row work the same.
  const noScript = await browser.newContext({ javaScriptEnabled: false });
  const plain = await noScript.newPage();
  await plain.goto("/dev/login?open=spanish");
  await plain.goto(`/courses/${courseId}/daily`);
  const plainKinds = plain.getByTestId("daily-activity-kind");
  await expect(plainKinds).toHaveText(["Study", "Review", "Play"]);
  await plain.getByTestId("daily-add-play").click();
  await expect(plainKinds).toHaveText(["Study", "Review", "Play", "Play"]);
  await plain.getByTestId("daily-up").last().click();
  await expect(plainKinds).toHaveText(["Study", "Review", "Play", "Play"]);
  await plain.getByTestId("daily-up").nth(2).click();
  await expect(plainKinds).toHaveText(["Study", "Play", "Review", "Play"]);
  await noScript.close();
});
