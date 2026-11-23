// Today's work (#223): the dev user (an owner, so it holds students) makes
// a course, a student, gives the course and assigns today's recommended day
// with /dev/student/today. The student, with JavaScript off, sees the list,
// does the Study stop through its form (a miss first, then both right),
// comes back to the list that says so, is refused /lessons, and finishes
// the Study of an earlier day from Earlier days.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";

/** The starter course's answers, by question id. */
const RIGHT: Record<string, string> = {
  "u1-first-01": "meaning",
  "u1-first-02": "example",
  "u1-first-03": "2",
  "u1-first-04": "true",
};

async function newStudent(page: Page): Promise<{ id: number; username: string; password: string }> {
  const res = await page.request.get("/dev/student?format=json");
  expect(res.status()).toBe(200);
  return res.json();
}

/** Answers one question of the stop's form: a radio by its value (any case), else the text box. */
async function answer(question: Locator, value: string, wrong = false): Promise<void> {
  const radios = question.locator("input[type=radio]");
  if ((await radios.count()) > 0) {
    for (const radio of await radios.all()) {
      const v = ((await radio.getAttribute("value")) ?? "").toLowerCase();
      if ((v === value.toLowerCase()) !== wrong) {
        await radio.check();
        return;
      }
    }
    throw new Error(`no option for ${value}`);
  }
  await question.getByTestId("student-study-answer").fill(wrong ? "not it" : value);
}

/** The ids of the questions the stop asks. */
async function asked(kid: Page): Promise<string[]> {
  const ids: string[] = [];
  for (const q of await kid.getByTestId("student-study-question").all()) ids.push((await q.getAttribute("data-qid")) ?? "");
  return ids;
}

/** Answers both questions right and sends the form. */
async function answerRight(kid: Page): Promise<void> {
  const questions = kid.getByTestId("student-study-question");
  for (const [i, id] of (await asked(kid)).entries()) await answer(questions.nth(i), RIGHT[id]);
  await kid.getByTestId("student-study-submit").click();
}

test("with JavaScript off a student does today's Study stop and an earlier day's", async ({ page, browser }) => {
  await devLogin(page);
  const courseId = await createCourse(page);
  const kidAccount = await newStudent(page);
  await page.goto("/students");
  const row = page.getByTestId("students-row").filter({ has: page.getByTestId("students-username").getByText(kidAccount.username, { exact: true }) });
  await row.getByTestId("students-pick").check();
  await page.getByTestId("students-give-course").selectOption(courseId);
  await page.getByTestId("students-give").click();
  await expect(page.getByTestId("students-notice")).toBeVisible();
  const today = await page.request.get(`/dev/student/today?day=0&course=${courseId}&format=json`);
  expect(today.status()).toBe(200);
  expect(await today.json()).toMatchObject({ course_id: courseId, activities: 3, assigned: true });

  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 800, height: 600 } });
  const kid = await context.newPage();
  await kid.goto("/login");
  await kid.getByTestId("login-email").fill(kidAccount.username);
  await kid.getByTestId("login-password").fill(kidAccount.password);
  await kid.getByTestId("login-submit").click();
  await expect(kid).toHaveURL(/\/$/);
  await expect(kid.getByTestId("student-today-kind")).toHaveText(["Study", "Review", "Play"]);
  await expect(kid.getByTestId("student-today-word")).toHaveText(["Next", "After Study", "After Review"]);
  await expect(kid.getByTestId("student-today-line")).toContainText("0 of 3 done");
  await expect(kid.locator("script")).toHaveCount(0);

  // The Study stop: the lesson, two questions; one wrong, then two new ones right.
  await kid.getByTestId("student-today-go").click();
  await expect(kid.getByTestId("student-study-text")).toBeVisible();
  await expect(kid.getByTestId("student-study-question")).toHaveCount(2);
  const first = await asked(kid);
  const questions = kid.getByTestId("student-study-question");
  await answer(questions.nth(0), RIGHT[first[0]], true);
  await answer(questions.nth(1), RIGHT[first[1]]);
  await kid.getByTestId("student-study-submit").click();
  await expect(kid.getByTestId("student-study-missed")).toBeVisible();
  await expect(kid.getByTestId("student-study-question")).toHaveCount(2);
  expect((await asked(kid)).filter((id) => first.includes(id))).toEqual([]);
  await answerRight(kid);
  await expect(kid).toHaveURL(/\/\?done=\d+$/);
  await expect(kid.getByTestId("student-today-done")).toContainText("Done: Study");
  await expect(kid.getByTestId("student-today-word")).toHaveText(["Done", "Next", "After Review"]);
  await expect(kid.getByTestId("student-today-go")).toHaveText("Go to the next activity");

  // Study's own pages are closed to a student.
  expect((await kid.goto("/lessons"))?.status()).toBe(404);

  // Yesterday, assigned on the dev server: finished from Earlier days.
  const yesterday = await page.request.get(`/dev/student/today?day=-1&course=${courseId}&format=json`);
  expect(await yesterday.json()).toMatchObject({ assigned: true, activities: 3 });
  await kid.goto("/");
  await expect(kid.getByTestId("student-today-earlier")).toHaveText("Earlier days · 3 left");
  await kid.getByTestId("student-today-earlier").click();
  await expect(kid.getByTestId("student-earlier-left")).toHaveText(["3 left"]);
  await kid.getByTestId("student-earlier-open").click();
  await expect(kid.getByTestId("student-today-heading")).toContainText("Work for");
  await kid.getByTestId("student-today-go").click();
  await answerRight(kid);
  await expect(kid).toHaveURL(/\/today\/day\/\d+\?done=\d+$/);
  await expect(kid.getByTestId("student-today-done")).toContainText("Done: Study");
  await context.close();
});
