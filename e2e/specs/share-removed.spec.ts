// A course share removed while the learner is in it (#1083): the page's
// next API call gets a 410 and the page goes to the course list, instead of
// Sprint's "try again" error that trying again cannot fix.
import { expect, test } from "@playwright/test";
import { createCourse, devLogin, devReset, makeShareLink, shareRow } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";

const LESSON = "first-lesson";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("the share removed mid-Sprint: the page goes to the course list instead of 'try again'", async ({ page, browser }) => {
  // The owner makes a course and shares it.
  await devLogin(page, "next=/courses");
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const friend = newAccount("share-removed");
  const guest = await (await browser.newContext()).newPage();
  await signUp(guest, friend);
  await page.goto(`/courses/${id}?share=1`);
  const link = await makeShareLink(page, "Friend");
  const row = shareRow(page, "Friend");
  await expect(row).toBeVisible();
  await guest.goto(link);
  await expect(guest).not.toHaveURL(/\/courses(\?|$)/);

  // The learner starts a Sprint; the owner removes the share.
  await guest.goto(`/sprint/play?lesson=${LESSON}`);
  const stage = guest.getByTestId("sprint-play-stage");
  await expect(stage).toBeVisible();
  await row.getByTestId("course-share-remove").click();
  await expect(page.getByTestId("course-share-notice")).toContainText("Link removed");

  // Quick-bite cards come first ("Got it" moves on in the page); then answer the
  // first exercise, right or wrong: checking it talks to the server.
  const check = guest.getByTestId("sprint-play-check");
  for (let i = 0; i < 5 && (await stage.getAttribute("data-kind")) === "bite"; i++) {
    const pos = (await stage.getAttribute("data-pos")) ?? "";
    await check.click();
    await expect(stage).not.toHaveAttribute("data-pos", pos);
  }
  if ((await stage.getAttribute("data-kind")) === "sort") {
    // A sort judges each tap and checks itself once the pile is empty (#1148):
    // tap the buckets in turn, so each word finds its own, until the page leaves.
    const buckets = stage.getByTestId("sprint-sort-bucket");
    const n = await buckets.count();
    for (let i = 0; i < 60 && /\/sprint\//.test(guest.url()); i++) await buckets.nth(i % n).click({ timeout: 1000 }).catch(() => {});
  } else {
    // Tap the first button until Check is enabled.
    for (let i = 0; i < 10 && !(await check.isEnabled()); i++) await stage.locator("button:visible").first().click();
    await check.click();
  }
  await expect(guest).toHaveURL(/\/courses$/);
  await expect(guest.getByTestId("sprint-play-error")).toHaveCount(0);
  await expect(guest.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveCount(0);

  // Any API call on the course now answers 410, which every mode's script follows.
  expect((await guest.request.get("/api/progress")).status()).toBe(410);
  await guest.context().close();
});
