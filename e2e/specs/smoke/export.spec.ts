// "Download my data" (#430): the profile's button queues an export, a
// background thread builds it, and the link arrives by e-mail (the dev
// mailbox, /dev/inbox). The link downloads the JSON once, and only for the
// account signed in. Exact: no model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { newAccount, register } from "../../lib/learner.ts";
import { devPage, mailLink, waitForMail } from "../../lib/mail.ts";

// The link only arrives through the dev mailbox, which the mini lane
// (E2E_MINI=1) lacks; #633 gives the mini a Download button instead.
test.skip(process.env.E2E_MINI === "1", "no dev mailbox in the mini lane: the download link is only logged");

test("a learner asks for their data, gets the link by e-mail and downloads the JSON once", async ({ page }) => {
  const account = newAccount("export");
  await register(page, account, "spanish");
  await page.goto("/profile");

  // The button queues it; the (i) says what is going on.
  const button = page.getByTestId("profile-export");
  await expect(button).toHaveText(/Prepare my data/);
  await button.click();
  await expect(page).toHaveURL(/\/profile\?saved=export-queued#data$/);
  await expect(page.getByTestId("profile-notice")).toContainText("Preparing your data");

  // The mail lands in the dev mailbox; its link is the download.
  const mail = await waitForMail(account.email, /data is ready/);
  expect(mail.subject).not.toMatch(/token/);
  const link = mailLink(mail, /\/profile\/export\/download\?token=[0-9a-f]{64}$/);
  const dev = await devPage(page.context().browser()!, `/dev/inbox/${mail.id}`);
  await expect(dev.getByTestId("dev-inbox-subject")).toHaveText(mail.subject);
  await dev.context().close();

  // Once ready, the profile says so behind the (i), and a second copy waits a day.
  await page.reload();
  await expect(button).toBeDisabled();
  await page.getByTestId("profile-export-info").click();
  await expect(page.getByTestId("profile-export-dialog")).toBeVisible();
  await expect(page.getByTestId("profile-export-state")).toHaveAttribute("data-state", "ready");
  await expect(page.getByTestId("profile-export-state")).toContainText(account.email);
  await page.getByTestId("profile-export-close").click();

  // Signed out, the link is no use (and is not used up).
  const stranger = await page.context().browser()!.newContext();
  const refused = await stranger.request.get(link, { maxRedirects: 0 });
  expect(refused.status()).toBe(303);
  await stranger.close();

  // Signed in: the JSON, with the learner's rows and no secrets.
  const res = await page.request.get(link, { maxRedirects: 0 });
  expect(res.status()).toBe(200);
  expect(res.headers()["content-disposition"]).toContain("attachment");
  const data = await res.json();
  expect(data.tables.users).toHaveLength(1);
  expect(data.tables.users[0]).not.toHaveProperty("username");
  expect(data.tables.users[0]).not.toHaveProperty("password_hash");
  expect(data.tables.users[0].email).toBe(account.email);
  expect(data.tables.projects.map((p: { course_id: string }) => p.course_id)).toContain("spanish");
  expect(data.tables.sessions.length).toBeGreaterThan(0);
  expect(data.tables.data_exports).toHaveLength(1);
  const text = JSON.stringify(data);
  expect(text).not.toContain("argon2");
  expect(text).not.toContain("session_token");
  expect(data.api_key_providers).toEqual([]);

  // The link works once.
  const again = await page.request.get(link, { maxRedirects: 0 });
  expect(again.status()).toBe(303);
  expect(again.headers()["location"]).toContain("saved=export-gone");
  await page.goto("/profile?saved=export-gone");
  await expect(page.getByTestId("profile-export-error")).toContainText("used");
  await page.getByTestId("profile-export-info").click();
  await expect(page.getByTestId("profile-export-state")).toHaveAttribute("data-state", "downloaded");
});
