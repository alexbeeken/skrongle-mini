// E-mail a flagged course's owner (#1035): from the course's dialog in the
// superuser's course table, a form drafted with the title, that it was
// flagged and a link to sign in and see what to fix (never the word);
// the superuser edits it and sends it by hand. The dev mailbox gets it with
// the edit, and the row says "owner e-mailed". Nothing goes out on its own.
// The draft's language and the 404s are checked over HTTP in course_flags::tests.
import { expect, test } from "@playwright/test";
import { createCourse, devLogin } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";
import { inbox, waitForMail } from "../lib/mail";

const SUBJECT = /can't be started or shared/;
/** Mail about the flag the owner has had (not the sign-up's verification). */
const flagMail = async (to: string) => (await inbox(to)).filter((m) => SUBJECT.test(m.subject));

test.skip(process.env.E2E_MINI === "1", "moderation is hosted only; Skrongle mini leaves it out");

test("e-mail a flagged course's owner from its dialog, with an edit", async ({ page, browser }) => {
  // The owner: a course with a listed word in a lesson's summary.
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("emailowner");
  const to = a.email.toLowerCase();
  await signUp(them, a);
  await login(them, a);
  const id = await createCourse(them);
  await them.getByTestId("generate-close").click();
  const listing = (await (await them.request.get(`/api/courses/${id}/files`)).json()) as { files: { path: string }[] };
  const path = listing.files.map((f) => f.path).find((p) => p.startsWith("lessons/"))!;
  const lesson = JSON.parse(((await (await them.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()) as { content: string }).content);
  lesson.summary = "Never say wetback.";
  expect((await them.request.post(`/api/courses/${id}/files`, { data: { files: [{ path, content: JSON.stringify(lesson, null, 2) }] } })).ok()).toBe(true);
  await them.goto("/courses");
  await them.context().close();
  expect(await flagMail(to), "flagging sends nothing").toHaveLength(0);

  await devLogin(page);
  await page.goto(`/superuser?course_q=${encodeURIComponent(id)}#courses`);
  const row = page.locator(`[data-testid="superuser-course-row"][data-course-id="${id}"]`);
  await expect(row).toHaveAttribute("data-state", "flagged");
  await expect(row.getByTestId("superuser-course-emailed")).toHaveCount(0);
  await row.getByTestId("superuser-course-open").click();
  const detail = page.locator(`[data-testid="superuser-course-dialog"][data-course-id="${id}"]`);
  const title = (await detail.locator("h2").textContent())!.trim();
  await detail.getByTestId("superuser-course-email-owner").click();

  const form = page.locator(`[data-testid="superuser-email-owner-dialog"][data-course-id="${id}"]`);
  await expect(form).toBeVisible();
  await expect(form).toContainText(to);
  await expect(form.getByTestId("superuser-email-owner-subject")).toHaveValue(new RegExp(title.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  const text = form.getByTestId("superuser-email-owner-text");
  await expect(text).toHaveValue(/Our content check flagged it\./);
  await expect(text).toHaveValue(/Sign in to see what to fix: http\S+\/courses/);
  expect(await text.inputValue()).not.toContain("wetback");
  await text.fill(`${await text.inputValue()}\n\nLook at the lesson summary.`);
  expect(await flagMail(to), "nothing goes out before Send").toHaveLength(0);
  await form.getByTestId("superuser-email-owner-send").click();

  await expect(page.getByTestId("superuser-notice")).toContainText(`The e-mail to the owner of ${id} is on its way.`);
  const mail = await waitForMail(to, SUBJECT);
  expect(mail.text).not.toContain("wetback");
  expect(mail.text).toContain("Sign in to see what to fix:");
  expect(mail.text).toContain("Look at the lesson summary.");
  expect(mail.subject).toContain(title);

  await page.goto(`/superuser?course_q=${encodeURIComponent(id)}#courses`);
  await expect(row.getByTestId("superuser-course-emailed")).toContainText("owner e-mailed");
  await row.getByTestId("superuser-course-open").click();
  await detail.getByTestId("superuser-course-email-owner").click();
  await expect(form.getByTestId("superuser-email-owner-before")).toBeVisible();
});
