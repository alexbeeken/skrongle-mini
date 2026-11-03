// Sharing a course (#451) past the happy path (#513). share.spec.ts is the
// happy path, share-sign-in.spec.ts the sign-in routes; this file is what
// happens around them: a used link (the invited address included), unknown
// and odd tokens, an old link after the owner removes and invites again,
// every mode on a shared course, the share removed mid-session, the owner
// deleting the course or the account, and the invitation's language. The
// server-only cases (limits, a course being written) are Rust tests in
// src/routes/share/tests.rs.
import { expect, test, type Browser, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, shareLink } from "../lib/app";
import { login, newAccount, openCourse, progressOf, signUp, type Account } from "../lib/learner";

const LESSON = "first-lesson";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

/** A new account, signed in, in a browser context of its own (registering does not sign in, #379). */
async function learner(browser: Browser, prefix: string): Promise<{ page: Page; account: Account }> {
  const page = await (await browser.newContext()).newPage();
  const account = newAccount(prefix);
  await signUp(page, account);
  return { page, account };
}

/** As the owner on `page`: a new draft course (it has one lesson, `first-lesson`); its id. */
async function newCourse(page: Page): Promise<string> {
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  return id;
}

/** As the owner: share course `id` with `email` from the Share dialog; the link it shows. */
async function invite(page: Page, id: string, email: string): Promise<string> {
  await page.goto(`/courses/${id}?share=1`);
  const dialog = page.getByTestId("course-share");
  await expect(dialog).toBeVisible();
  await dialog.getByTestId("course-share-emails").fill(email);
  await dialog.getByTestId("course-share-send").click();
  await expect(shareRow(page, email)).toBeVisible();
  return shareLink(page, email);
}

/** The Share dialog's row for `email`. */
function shareRow(page: Page, email: string) {
  return page.locator(`[data-testid="course-share-row"][data-email="${email.toLowerCase()}"]`);
}

/** As the owner: stop sharing course `id` with `email`. */
async function unshare(page: Page, id: string, email: string): Promise<void> {
  await page.goto(`/courses/${id}?share=1`);
  await expect(page.getByTestId("course-share")).toBeVisible();
  await shareRow(page, email).getByTestId("course-share-remove").click();
  await expect(page.getByTestId("course-share-notice")).toContainText("No longer shared with");
  await expect(shareRow(page, email)).toHaveCount(0);
}

/** The course's card on the learner's list (count 0 when it is not there). */
async function card(page: Page, id: string) {
  await page.goto("/courses");
  return page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
}

/** Signed in, the link opens the course: home, with it active. */
async function opens(page: Page, link: string, id: string): Promise<void> {
  await page.goto(link);
  await expect(page).not.toHaveURL(/\/courses(\?|$)/);
  await expect(await card(page, id)).toHaveAttribute("data-active", "true");
}

/** Without a course the learning pages go to the course list. */
async function backAtThePicker(page: Page): Promise<void> {
  await page.goto("/lessons");
  await expect(page).toHaveURL(/\/courses$/);
}

test("a used link turns every other account away, the invited address included", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await newCourse(page);
  const invited = newAccount("share-edge-invited");
  const link = await invite(page, id, invited.email);

  // Someone the link was forwarded to opens it first: it is theirs.
  const first = await learner(browser, "share-edge-first");
  await opens(first.page, link, id);

  // A third account: back to its list, told why, no course.
  const third = await learner(browser, "share-edge-third");
  await third.page.goto(link);
  await expect(third.page).toHaveURL(/\/courses\?notice=share-taken$/);
  await expect(third.page.getByTestId("courses-notice")).toHaveText(
    "Another account has already used this link. Ask the course's owner to share it with your address.",
  );
  await expect(await card(third.page, id)).toHaveCount(0);

  // The invited address registers afterwards: an address is only a label
  // (#10), so the course is not on its list, and the used link turns it away too.
  const own = await (await browser.newContext()).newPage();
  await signUp(own, invited);
  await expect(await card(own, id)).toHaveCount(0);
  await own.goto(link);
  await expect(own).toHaveURL(/\/courses\?notice=share-taken$/);
  await expect(await card(own, id)).toHaveCount(0);

  // The first account keeps the course.
  await expect(await card(first.page, id)).toHaveCount(1);

  // The owner sees the address's row opened (by the first account).
  await page.goto(`/courses/${id}?share=1`);
  await expect(shareRow(page, invited.email)).toHaveAttribute("data-opened", "true");
  for (const p of [first.page, third.page, own]) await p.context().close();
});

const GONE = "This link no longer works. Ask the course's owner to share the course with you again.";

test("unknown, odd and padded tokens: the link says it no longer works (#1082), the sign-in pages carry it", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await newCourse(page);
  const invited = newAccount("share-edge-token");
  const link = await invite(page, id, invited.email);
  const token = link.split("/").pop()!;

  const guest = await (await browser.newContext()).newPage();
  const unknown = "0".repeat(64);
  const odd = encodeURIComponent('<b>"x"</b>&share=1');
  // Signed out, a token that names no share goes to the login page. A token shaped like one (hex)
  // gets the "no longer works" notice and is carried on; one that is not gets the usual page.
  const shaped = (t: string) => /^[0-9a-fA-F]+$/.test(t);
  for (const t of [unknown, odd, token.toUpperCase()]) {
    await guest.goto(`/courses/shared/${t}`);
    await expect(guest, t).toHaveURL(/\/login\?share=/);
    await expect(guest.getByTestId("login-share-notice"), t).toHaveCount(shaped(t) ? 1 : 0);
  }
  // The login and register pages with such a token: the notice (shaped tokens), nothing echoed.
  for (const path of ["/login", "/register"]) {
    for (const t of [unknown, odd, "zz"]) {
      const r = await guest.goto(`${path}?share=${t}`);
      expect(r?.status(), `${path}?share=${t}`).toBe(200);
      const prefix = path.slice(1);
      if (shaped(t)) {
        await expect(guest.getByTestId(`${prefix}-share-notice`)).toHaveText(GONE);
        await expect(guest.getByTestId(`${prefix}-share-notice`)).toHaveAttribute("data-course-title", "");
        await expect(guest.locator('input[name="share"]')).toHaveCount(1);
      } else {
        await expect(guest.getByTestId(`${prefix}-share-notice`)).toHaveCount(0);
        await expect(guest.locator('input[name="share"]')).toHaveCount(0);
      }
      expect(await guest.content()).not.toContain('<b>"x"</b>');
    }
  }
  // Spaces around the token (a copy and paste, say) are trimmed: the link still works.
  await guest.goto(`/courses/shared/%20${token}%20`);
  await expect(guest).toHaveURL(new RegExp(`/login\\?share=${token}$`));
  await expect(guest.getByTestId("login-share-notice")).toBeVisible();

  // Signed in, an unknown token goes to the course list with the notice, and nothing joins the list.
  await page.goto(`/courses/shared/${unknown}`);
  await expect(page).toHaveURL(/\/courses\?notice=share-gone$/);
  await expect(page.getByTestId("courses-notice")).toHaveText(GONE);
  await guest.context().close();
});

test("removed and invited again: a new link, the old one dead, the learner's progress back", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await newCourse(page);
  const r = await learner(browser, "share-edge-again");
  const oldLink = await invite(page, id, r.account.email);
  await opens(r.page, oldLink, id);
  expect((await r.page.request.post(`/lessons/${LESSON}/read`, { maxRedirects: 0 })).status()).toBe(303);
  expect((await progressOf(r.page, LESSON)).seen).toBe(true);

  await unshare(page, id, r.account.email);
  await expect(await card(r.page, id)).toHaveCount(0);
  await r.page.goto(oldLink);
  await expect(r.page, "the link went with the share").toHaveURL(/\/courses\?notice=share-gone$/);

  // Invited again: a different link.
  const newLink = await invite(page, id, r.account.email);
  expect(newLink).not.toBe(oldLink);
  await r.page.goto(oldLink);
  await expect(r.page, "the old link stays dead").toHaveURL(/\/courses\?notice=share-gone$/);
  await opens(r.page, newLink, id);
  expect((await progressOf(r.page, LESSON)).seen, "progress is kept through the removal").toBe(true);
  await r.page.context().close();
});

test("an old link after the share was removed says it no longer works (#1082)", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await newCourse(page);
  const r = await learner(browser, "share-edge-gone");
  const link = await invite(page, id, r.account.email);
  await opens(r.page, link, id);
  await unshare(page, id, r.account.email);
  await r.page.goto(link);
  await expect(r.page).toHaveURL(/\/courses\?notice=share-gone$/);
  await expect(r.page.getByTestId("courses-notice")).toHaveText(GONE);
  await r.page.context().close();
});

test("every mode opens on a shared course with the learner's own progress, kept when the owner edits the lesson", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await newCourse(page);
  const r = await learner(browser, "share-edge-modes");
  await opens(r.page, await invite(page, id, r.account.email), id);

  for (const path of ["/", "/lessons", `/lessons/${LESSON}`, "/map", "/guide", "/sprint", `/sprint/play?lesson=${LESSON}`, "/play", "/history"]) {
    const res = await r.page.goto(path);
    expect(res?.status(), path).toBe(200);
    expect(new URL(r.page.url()).pathname, `${path} stays in the course`).not.toMatch(/^\/(courses|login)$/);
  }
  expect((await r.page.request.post(`/lessons/${LESSON}/read`, { maxRedirects: 0 })).status()).toBe(303);
  expect((await progressOf(r.page, LESSON)).seen).toBe(true);
  // The progress is the learner's: the owner's own is untouched.
  await openCourse(page, id);
  const owners = (await (await page.request.get("/api/progress")).json()).lessons as { slug: string; seen: boolean }[];
  expect(owners.find((l) => l.slug === LESSON)?.seen, "the owner has not read it").toBe(false);

  // The owner renames the lesson: the learner sees the new title and still has it read.
  const path = `lessons/${LESSON}.json`;
  const file = await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`);
  const lesson = JSON.parse(((await file.json()) as { content: string }).content);
  lesson.title = "Renamed by the owner";
  expect((await page.request.post(`/courses/${id}/file`, { form: { path, content: JSON.stringify(lesson, null, 2) } })).ok()).toBeTruthy();
  const after = await progressOf(r.page, LESSON);
  expect(after.title).toBe("Renamed by the owner");
  expect(after.seen).toBe(true);
  await r.page.context().close();
});

test("the share removed mid-session: the next page goes to the course list, the API answers 410", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await newCourse(page);
  const r = await learner(browser, "share-edge-midway");
  await opens(r.page, await invite(page, id, r.account.email), id);
  await r.page.goto(`/sprint/play?lesson=${LESSON}`);
  await expect(r.page.getByTestId("sprint-play-stage")).toBeVisible();

  await unshare(page, id, r.account.email);
  expect((await r.page.request.get("/api/progress")).status()).toBe(410);
  await backAtThePicker(r.page);
  await expect(await card(r.page, id)).toHaveCount(0);
  await r.page.context().close();
});

// The share removed mid-Sprint (#1083) is specs/share-removed.spec.ts.

test("the owner deletes the course, or the whole account: the course leaves the learner's list and session", async ({ page, browser }) => {
  // The course.
  await devLogin(page, "next=/courses");
  const id = await newCourse(page);
  const r = await learner(browser, "share-edge-deleted");
  const link = await invite(page, id, r.account.email);
  await opens(r.page, link, id);
  expect((await page.request.post(`/courses/${id}/delete`, { maxRedirects: 0 })).status()).toBe(303);
  await expect(await card(r.page, id)).toHaveCount(0);
  await backAtThePicker(r.page);

  // The account.
  const owner = await learner(browser, "share-edge-owner");
  await owner.page.goto("/courses");
  const id2 = await newCourse(owner.page);
  await opens(r.page, await invite(owner.page, id2, r.account.email), id2);
  await owner.page.goto("/profile");
  await owner.page.getByTestId("profile-delete-password").fill(owner.account.password);
  await owner.page.getByTestId("profile-delete-submit").click();
  await expect(owner.page).not.toHaveURL(/\/profile/);
  await expect(await card(r.page, id2)).toHaveCount(0);
  await backAtThePicker(r.page);
  for (const p of [r.page, owner.page]) await p.context().close();
});

test("the Share dialog is in the owner's language; the learner's pages stay in theirs", async ({ browser }) => {
  const owner = await learner(browser, "share-edge-es");
  expect((await owner.page.request.post("/lang", { form: { lang: "es", back: "/courses" }, maxRedirects: 0 })).status()).toBe(303);
  await owner.page.goto("/courses");
  await expect(owner.page.locator("html")).toHaveAttribute("lang", "es");
  const id = await newCourse(owner.page);
  const r = await learner(browser, "share-edge-en");
  const link = await invite(owner.page, id, r.account.email);
  await expect(owner.page.getByTestId("course-share-notice")).toContainText("1 enlace de invitación creado");
  await opens(r.page, link, id);
  await r.page.goto("/lessons");
  await expect(r.page.locator("html")).toHaveAttribute("lang", "en");
  for (const p of [r.page, owner.page]) await p.context().close();
});
