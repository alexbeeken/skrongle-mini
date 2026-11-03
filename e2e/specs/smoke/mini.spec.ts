// The mini shape (#632, lane #639): what a self-hoster runs, with no mail,
// Google or payments configured. CI runs this in the
// e2e-mini job (E2E_MINI=1 scripts/e2e.sh, docs/testing.md "The mini lane"),
// where the server has the dev stand-ins off; elsewhere it skips.
//
// Each #632 issue adds its browser check here as it lands. Here: #633 no
// red ! on the e-mail field, #636 no Subscription
// section, #641 /healthz names the mini, #667 the share flow shows the
// link once and a reload goes back to the dialog. Still to come: #634
// the closed register page (the lane keeps sign-up open: the specs register).
import { AnswerKey } from "../../lib/key.ts";
import { playStage } from "../../lib/answer.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, expectProgressAgrees, login, logout, newAccount, register, signUp } from "../../lib/learner.ts";

test.skip(process.env.E2E_MINI !== "1", "the mini lane only (E2E_MINI=1): it needs the dev stand-ins off");

const COURSE = "spanish";
const key = new AnswerKey(COURSE);

test("a learner registers, signs in again and passes a lesson with nothing configured", async ({ page }) => {
  const account = newAccount("mini");
  await register(page, account, COURSE);
  await logout(page);
  await login(page, account);
  const [first] = await apiProgress(page);
  await page.goto(`/guide/${first.slug}`);
  const [right, total] = await playStage(page, key);
  expect(right, "every quick check right").toBe(total);
  expect((await apiProgress(page)).find((l) => l.slug === first.slug)?.passed, `${first.slug} passed`).toBe(true);
  await expectProgressAgrees(page);
});

test("there is no dev mailbox", async ({ request }) => {
  const mail = await (await request.get("/dev/inbox?format=json")).json();
  expect(mail.smtp, "/dev/inbox: no SMTP mailbox").toBeNull();
  expect((await request.get("/dev/phone?format=json")).status(), "/dev/phone is gone").toBe(404);
});

test("the server says it is a Skrongle mini", async ({ request }) => {
  const res = await request.get("/healthz");
  expect(res.status()).toBe(200);
  expect(await res.text(), "/healthz names the edition (#641)").toMatch(/^ok skrongle mini \d+\.\d+\.\d+/);
});

test("the profile asks nobody to confirm their e-mail", async ({ page }) => {
  await signUp(page, newAccount("mini-profile"));
  await page.goto("/profile");
  await expect(page.getByTestId("profile-form")).toBeVisible();
  // #10: nothing confirms an address, so neither a red ! nor an (i) about it.
  await expect(page.getByTestId("email-unverified")).toHaveCount(0);
  await expect(page.getByTestId("email-no-mail-info")).toHaveCount(0);
});

test("sharing shows the link once, and a reload goes back to the dialog without inviting again", async ({ page }) => {
  await signUp(page, newAccount("mini-share"));
  await page.goto("/courses");
  await page.getByTestId("courses-create").click();
  const id = await page.getByTestId("course-edit").getAttribute("data-course-id");
  expect(id, "the new course's id").toBeTruthy();
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-edit-share").click();
  await page.getByTestId("course-share-emails").fill("friend@example.org");
  await page.getByTestId("course-share-send").click();
  // #633: no mail, so the dialog shows the link to pass on.
  await expect(page.getByTestId("course-share-link")).toHaveValue(/\/courses\/shared\//);
  // #667: the address bar holds the course page, not the form's POST target.
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?share=1$`));
  await page.reload();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?share=1$`));
  await expect(page.getByTestId("course-share")).toBeVisible();
  await expect(page.getByTestId("course-share-link")).toHaveCount(0);
  await expect(page.getByTestId("course-share-row")).toHaveCount(1);
  await expect(page.locator('[data-testid="course-share-row"][data-email="friend@example.org"]')).toHaveAttribute("data-opened", "false");
  // The POST target opened by hand is a redirect to the dialog, not a 405.
  const res = await page.request.get(`/courses/${id}/share`, { maxRedirects: 0 });
  expect(res.status()).toBe(303);
  expect(res.headers()["location"]).toMatch(new RegExp(`/courses/${id}\\?share=1$`));
});
