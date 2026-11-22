// The Students page (#218): the dev user (an owner, so it holds students)
// adds three students by pasting names, changes a username and types one
// password, gets the sign-in sheet once (printed black on white), gives one
// student a new password (the old one is refused, the new one signs in),
// deletes one, and adds one more with JavaScript off.
import { expect, test, type Page } from "@playwright/test";
import { devLogin } from "../lib/app";

async function count(page: Page): Promise<number> {
  return Number(await page.getByTestId("students-count").getAttribute("data-count"));
}

async function signIn(page: Page, username: string, password: string): Promise<void> {
  await page.goto("/login");
  await page.getByTestId("login-email").fill(username);
  await page.getByTestId("login-password").fill(password);
  await page.getByTestId("login-submit").click();
}

test("add a class, print its sheet, a new password, delete, and JavaScript off", async ({ page, browser }) => {
  const tag = Date.now().toString(36).slice(-5).replace(/[^a-z]/g, "x");
  await devLogin(page);
  await page.getByTestId("nav-students").click();
  await expect(page).toHaveURL(/\/students$/);
  const before = await count(page);

  await page.getByTestId("students-names").fill(`Ana ${tag}\nBea ${tag}\nCai ${tag}\n`);
  await page.getByTestId("students-add-next").click();
  const rows = page.getByTestId("students-add-row");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0).getByTestId("students-add-username")).toHaveValue(`ana.${tag}`);
  await rows.nth(1).getByTestId("students-add-username").fill(`bee.${tag}`);
  await rows.nth(2).getByTestId("students-add-password").fill("cai-typed-pw");
  await page.getByTestId("students-add-save").click();

  const slips = page.getByTestId("students-slip");
  await expect(slips).toHaveCount(3);
  await expect(page.getByTestId("students-slip-username")).toHaveText([`ana.${tag}`, `bee.${tag}`, `cai.${tag}`]);
  await expect(slips.nth(2).getByTestId("students-slip-password")).toHaveText("cai-typed-pw");
  const anaPassword = ((await slips.nth(0).getByTestId("students-slip-password").textContent()) ?? "").trim();
  expect(anaPassword).toMatch(/^[a-z2-9]{8}$/);
  await expect(page.getByTestId("students-print")).toBeVisible();

  // Printed: only the slips, black on white.
  await page.emulateMedia({ media: "print" });
  await expect(page.locator("header.topbar")).toBeHidden();
  await expect(page.getByTestId("students-print")).toBeHidden();
  const colours = await slips.first().evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.color, s.backgroundColor];
  });
  expect(colours).toEqual(["rgb(0, 0, 0)", "rgb(255, 255, 255)"]);
  await page.emulateMedia({ media: "screen" });

  // The sheet's address again: no passwords.
  await page.goto("/students/sheet");
  await expect(page.getByTestId("students-sheet-gone")).toBeVisible();
  await expect(page.getByTestId("students-slip")).toHaveCount(0);

  await page.goto("/students");
  expect(await count(page)).toBe(before + 3);

  // A new password for Ana: the sheet shows it once; the old one stops working.
  await page.getByTestId("students-row").filter({ hasText: `ana.${tag}` }).getByTestId("students-name").click();
  await page.getByTestId("students-one-password").fill("ana-new-password");
  await page.getByTestId("students-one-password-save").click();
  await expect(page.getByTestId("students-slip-password")).toHaveText("ana-new-password");

  const kid = await (await browser.newContext()).newPage();
  await signIn(kid, `ana.${tag}`, anaPassword);
  await expect(kid.getByTestId("login-error")).toBeVisible();
  await signIn(kid, `ana.${tag}`, "ana-new-password");
  await expect(kid).not.toHaveURL(/\/login/);
  // No subject yet: its subjects page, in the student frame (#220).
  await expect(kid.getByTestId("student-subjects-none")).toBeVisible();
  await expect(kid.getByTestId("nav-students")).toHaveCount(0);
  expect((await kid.goto("/students"))?.status()).toBe(404);

  // Delete picked, confirmed.
  await page.goto("/students");
  await page.getByTestId("students-row").filter({ hasText: `bee.${tag}` }).getByTestId("students-pick").check();
  await page.getByTestId("students-delete-picked").click();
  await expect(page.getByTestId("students-confirm-list")).toContainText(`bee.${tag}`);
  await page.getByTestId("students-confirm-yes").click();
  await expect(page.getByTestId("students-notice")).toContainText("1 student deleted");
  expect(await count(page)).toBe(before + 2);

  // JavaScript off: the same forms.
  const noScript = await browser.newContext({ javaScriptEnabled: false });
  const plain = await noScript.newPage();
  await plain.goto("/dev/login?open=spanish");
  await plain.goto("/students");
  await plain.getByTestId("students-names").fill(`Dee ${tag}`);
  await plain.getByTestId("students-add-next").click();
  await expect(plain.getByTestId("students-add-row")).toHaveCount(1);
  await plain.getByTestId("students-add-save").click();
  await expect(plain.getByTestId("students-slip-username")).toHaveText(`dee.${tag}`);
  await expect(plain.getByTestId("students-print")).toBeHidden();
  await plain.goto("/students");
  expect(await count(plain)).toBe(before + 3);
  await noScript.close();
});
