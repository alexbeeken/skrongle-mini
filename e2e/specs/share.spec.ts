// Sharing a course (#451, #16): the owner's Share dialog on the course page
// makes an invitation link with a name; the link leads a visitor without an
// account to the login page, which says what the link is for, and brings
// them back after registering; the course is then on their list, read only,
// with the circle that says whose it is. Removing the link takes it away.
import { expect, test } from "@playwright/test";
import { createCourse, devLogin, devReset, makeShareLink, shareRow } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a friend registers from the link and has the course read only, until the owner removes the link", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const friend = newAccount("share");

  // The owner makes a link named for the friend: the dialog shows it once and lists it, not opened yet.
  await page.getByTestId("course-edit-share").click();
  const dialog = page.getByTestId("course-share");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("course-share-emails")).toHaveCount(0);
  await expect(dialog).not.toContainText(/e-?mail/i);
  const link = await makeShareLink(page, "My friend");
  const row = shareRow(page, "My friend");
  await expect(row).toHaveAttribute("data-opened", "false");
  await expect(row).toContainText("not opened yet");
  await expect(page.getByTestId("course-edit-share")).toHaveText("Share (1)");

  // The owner opening their own link does not take it: back to the dialog, the link still unopened.
  await page.goto(link);
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?share=1$`));
  await expect(page.getByTestId("course-share")).toBeVisible();
  await expect(row).toHaveAttribute("data-opened", "false");

  // A visitor with no account: the login page says what the link is for; registering there opens the course.
  const guest = await (await browser.newContext()).newPage();
  await guest.goto(link);
  await expect(guest).toHaveURL(/\/login\?share=/);
  await expect(guest.getByTestId("login-share-notice")).toHaveText(
    "It looks like you don't have an account with us! Please login or signup to continue viewing your course: Untitled course",
  );
  await guest.getByTestId("login-register-link").click();
  await expect(guest.getByTestId("register-share-notice")).toContainText("continue viewing your course: Untitled course");
  await guest.getByTestId("register-email").fill(friend.email);
  await guest.getByTestId("register-password").fill(friend.password);
  await guest.getByTestId("register-age").check();
  await guest.getByTestId("register-submit").click();
  // Registering signs the account in and goes on to the link.
  await expect(guest.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", friend.email.toLowerCase());
  await expect(guest).toHaveURL(/\/$/);

  // On their list with the circle, no editor; the course's pages open.
  await guest.goto("/courses");
  const card = guest.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card).toHaveAttribute("data-active", "true");
  await expect(card.getByTestId("courses-edit")).toHaveCount(0);
  await card.getByTestId("courses-card-shared").click();
  await expect(guest.getByTestId("courses-card-shared-dialog")).toContainText("belongs to dev@example.com");
  await guest.getByTestId("courses-card-shared-close").click();
  expect((await guest.goto(`/courses/${id}`))?.status()).toBe(404);
  await guest.goto("/lessons");
  await expect(guest.getByTestId("nav-account-toggle")).toBeVisible();
  await expect(guest).toHaveURL(/\/lessons$/);

  // The owner sees who opened it and removes the link: gone from the list, and the session falls back to the course picker.
  await page.reload(); // still ?share=1: the dialog opens by itself
  await expect(page.getByTestId("course-share")).toBeVisible();
  await expect(row).toHaveAttribute("data-opened", "true");
  await expect(row).toContainText(`opened by ${friend.email.toLowerCase()}`);
  await row.getByTestId("course-share-remove").click();
  await expect(page.getByTestId("course-share-notice")).toContainText("Link removed");
  await expect(page.getByTestId("course-share-row")).toHaveCount(0);
  await guest.goto("/courses");
  await expect(card).toHaveCount(0);
  await guest.goto("/lessons");
  await expect(guest).toHaveURL(/\/courses$/);
  await guest.context().close();
});

test("a shared course's circle tip wraps inside a phone screen while it shows, even with a long owner address (#954)", async ({ page, browser }) => {
  // The owner has a long address, so "Shared with you by ..." is wider than the screen on one line.
  // newAccount adds about 17 characters; the part before the @ stays within the 64 that mail allows (#1003).
  const owner = newAccount("a-long-owner-address-for-the-tip-check");
  const ownerPage = await (await browser.newContext()).newPage();
  await signUp(ownerPage, owner);
  const id = await createCourse(ownerPage);
  await ownerPage.getByTestId("generate-close").click();
  await ownerPage.getByTestId("course-edit-share").click();
  const link = await makeShareLink(ownerPage);
  await ownerPage.context().close();

  await devLogin(page, "next=/courses");
  await page.goto(link);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/courses");
  const dot = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`).getByTestId("courses-card-shared");
  // The tip is always laid out (opacity 0); hover and keyboard focus make it show.
  const tipOpacity = () => dot.evaluate((el) => getComputedStyle(el, "::after").opacity);
  await dot.hover();
  await expect.poll(tipOpacity).toBe("1");
  expect(await dot.evaluate((el) => getComputedStyle(el, "::after").content)).toContain(owner.email);
  const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
  expect(sw).toBeLessThanOrEqual(cw);

  // Keyboard focus shows the same tip, also inside the screen.
  await page.mouse.move(0, 0);
  await expect.poll(tipOpacity).toBe("0");
  await dot.focus();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  await expect(dot).toBeFocused();
  await expect.poll(tipOpacity).toBe("1");
  const focused = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
  expect(focused.sw).toBeLessThanOrEqual(focused.cw);
});
