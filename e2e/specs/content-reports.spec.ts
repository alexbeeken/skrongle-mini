// Content reports and copyright notices (#968): a learner reports a course
// shared with them from its "shared with you" dialog; anyone sends a
// copyright notice from /legal/copyright, linked from /terms. Both reach the
// superuser (the dev user) by mail and on /superuser, where Forward to owner
// mails the course's owner a copy and Resolve takes the report off the list.
import { expect, test } from "@playwright/test";
import { createCourse, devLogin, devReset } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";
import { inbox, type InboxMail, mailLink, waitForMail } from "../lib/mail";

/** The operator's copy: to SKRONGLE_LEGAL_EMAIL when the server has one, else
 * to the superusers (the dev user), so it is found by its subject alone. */
async function operatorMail(subject: RegExp): Promise<InboxMail> {
  let found: InboxMail | undefined;
  await expect.poll(async () => (found = (await inbox()).find((m) => subject.test(m.subject))) !== undefined, { message: `no mail matching ${subject}` }).toBe(true);
  return found!;
}

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a shared course is reported, a copyright notice is sent, and the superuser forwards and resolves them", async ({ page, browser }) => {
  // The dev user makes a course and shares it with a new learner.
  await devLogin(page, "next=/courses");
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const friend = newAccount("report");
  await page.getByTestId("course-edit-share").click();
  await page.getByTestId("course-share-emails").fill(friend.email);
  await page.getByTestId("course-share-send").click();
  await expect(page.getByTestId("course-share-notice")).toHaveText("Invitation sent to 1 address.");
  const link = mailLink(await waitForMail(friend.email, /shared a course with you/), /\/courses\/shared\//);

  // A built-in course has no report link (the dev user's Spanish card).
  await page.goto("/courses");
  await expect(page.getByTestId("content-report-open")).toHaveCount(0);

  // The learner opens the link, then reports the course from its circle's dialog.
  const learner = await (await browser.newContext()).newPage();
  await signUp(learner, friend);
  await login(learner, friend);
  await learner.goto(link);
  await learner.goto("/courses");
  const card = learner.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await card.getByTestId("courses-card-shared").click();
  await card.getByTestId("content-report-open").click();
  const dialog = card.getByTestId("content-report-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("content-report-email")).toHaveValue(friend.email.toLowerCase());
  await dialog.locator('[data-testid="content-report-reason"][data-reason="harmful"]').check();
  await dialog.getByTestId("content-report-details").fill("Lesson one mocks people.");
  await dialog.getByTestId("content-report-send").click();
  await expect(learner).toHaveURL(/\/courses\?notice=reported$/);
  await expect(learner.getByTestId("courses-notice")).toContainText("Your report reached us");
  const toOperator = await operatorMail(/new report \(Harmful\)/);
  expect(toOperator.text).toContain("Lesson one mocks people.");
  await learner.context().close();

  // Signed out: the copyright form, from the Terms page.
  const guest = await (await browser.newContext()).newPage();
  await guest.goto("/terms");
  await guest.getByTestId("terms-copyright-link").click();
  await expect(guest).toHaveURL(/\/legal\/copyright$/);
  await guest.getByTestId("copyright-name").fill("Cleo Claimant");
  await guest.getByTestId("copyright-email").fill("cleo@example.org");
  await guest.getByTestId("copyright-work").fill("My verb tables");
  await guest.getByTestId("copyright-location").fill(`http://localhost/courses/${id}`);
  await guest.getByTestId("copyright-details").fill("The whole of unit one is my tables.");
  await guest.getByTestId("copyright-good-faith").check();
  await guest.getByTestId("copyright-send").click();
  await expect(guest.getByTestId("copyright-sent")).toBeVisible();
  await operatorMail(/new report \(Copyright\)/);
  await guest.context().close();

  // The superuser page lists both; Forward to owner mails the owner (the dev user here); Resolve clears them.
  await page.goto("/superuser");
  const harmful = page.locator(`[data-testid="superuser-report"][data-reason="harmful"][data-course-id="${id}"]`);
  const notice = page.locator(`[data-testid="superuser-report"][data-reason="copyright"][data-course-id="${id}"]`);
  await expect(harmful).toHaveCount(1);
  await expect(notice).toHaveCount(1);
  await notice.getByTestId("superuser-report-forward").click();
  await expect(page.getByTestId("superuser-notice")).toContainText("A copy went to the course's owner.");
  const toOwner = await waitForMail("dev@example.com", /a report about your course/);
  expect(toOwner.text).toContain("cleo@example.org");
  await expect(notice).toHaveAttribute("data-forwarded", "true");
  await notice.getByTestId("superuser-report-resolve").click();
  await expect(page.getByTestId("superuser-notice")).toContainText("resolved");
  await harmful.getByTestId("superuser-report-resolve").click();
  await expect(harmful).toHaveCount(0);
  await expect(notice).toHaveCount(0);
});
