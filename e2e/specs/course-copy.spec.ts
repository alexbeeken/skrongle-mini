// Copy a course (#629): the copy icon at the top right of a course's settings
// dialog behind the card's gear (not in the Start wizard of a course not
// opened yet) makes a private course of the learner's own with every file of the
// original, called "Copy of <title>", and opens it in the editor. The card
// itself has no Copy button. The original is not touched, and copies go
// through the course limits: past the account's bytes the copy is refused
// with a notice on the course list.
import { expect, test, type Page } from "@playwright/test";
import { readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { courseDir, createCourse, devLogin, devReset, generationEnds } from "../lib/app";
import { login, newAccount, openCourse, signUp } from "../lib/learner";
import { mailLink, waitForMail } from "../lib/mail";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

/** The paths a built-in course is made of, as the app reads its folder. */
function coursePaths(id: string): string[] {
  const root = courseDir(id);
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else out.push(relative(root, p));
    }
  };
  out.push("course.json");
  walk(join(root, "lessons"));
  try {
    walk(join(root, "pictures"));
  } catch {
    // no pictures
  }
  return out.filter((p) => p === "course.json" || (p.startsWith("lessons/") && p.endsWith(".json")) || (p.startsWith("pictures/") && p.endsWith(".svg"))).sort();
}

const card = (page: Page, id: string) => page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);

/** Opens the course's settings dialog behind the card's gear, opening the course first if it has none yet
 * (its Start wizard has no editor, copy or translate icons). */
async function openSettings(page: Page, id: string) {
  if (!(await card(page, id).getByTestId("courses-settings").count())) {
    await openCourse(page, id);
    await page.goto("/courses");
  }
  const c = card(page, id);
  await c.getByTestId("courses-settings").click();
  const dialog = c.getByTestId("courses-settings-dialog");
  await expect(dialog).toBeVisible();
  return dialog;
}

type Box = { x: number; y: number; width: number; height: number } | null;

/** The dialog's corner icons, measured in one go (the wizard script is still sizing the dialog as it opens,
 * so two reads one after the other can disagree). */
const corner = (dialog: ReturnType<Page["locator"]>) =>
  dialog.evaluate((d) => {
    const box = (t: string): Box => {
      const el = d.querySelector(`[data-testid="${t}"]`);
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    };
    return { edit: box("courses-edit"), copy: box("courses-copy"), close: box("courses-settings-close") };
  });

/** `a` sits just left of `b` on the same row. */
function besides(a: Box, b: Box) {
  expect(a && b).toBeTruthy();
  expect(a!.x + a!.width).toBeLessThanOrEqual(b!.x + 1);
  expect(b!.x - (a!.x + a!.width)).toBeLessThan(16);
  expect(Math.abs(a!.y - b!.y)).toBeLessThanOrEqual(1);
}

/** The copy icon: on the X's row at the dialog's top right, just left of it, with its hover text. */
async function expectCopyIcon(dialog: ReturnType<Page["locator"]>) {
  const copy = dialog.getByTestId("courses-copy");
  await expect(copy).toBeVisible();
  await expect(copy).toHaveAttribute("data-tip", "Create a copy of this course.");
  await expect(copy.locator("svg")).toHaveCount(1);
  const k = await corner(dialog);
  besides(k.copy, k.close);
  return copy;
}

test("the copy icon in Spanish's settings opens Copy of Spanish in the editor, with every file, and lists it", async ({ page }) => {
  await devLogin(page, "next=/courses");
  // Not on the card: in its settings dialog, behind the gear.
  await expect(card(page, "spanish").locator(".course-actions [data-testid=courses-copy]")).toHaveCount(0);
  await expect(card(page, "spanish").getByTestId("courses-copy")).toBeHidden();
  const spanish = await openSettings(page, "spanish");
  await expect(spanish.getByTestId("courses-edit")).toHaveCount(0); // not theirs to edit
  await (await expectCopyIcon(spanish)).click();
  await expect(page).toHaveURL(/\/courses\/copy-of-spanish-[a-z0-9]+$/);
  const id = new URL(page.url()).pathname.split("/").pop() ?? "";
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-course-id", id);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  await expect(page.getByTestId("course-edit-title")).toHaveText("Copy of Spanish");

  // Every file of the original, at the same paths.
  const files = (await (await page.request.get(`/api/courses/${id}/files`)).json()) as { files: { path: string }[]; error: string | null };
  expect(files.error).toBeNull();
  expect(files.files.map((f) => f.path).sort()).toEqual(coursePaths("spanish"));

  // On the list as one of their own, beside the untouched original.
  await page.goto("/courses");
  await expect(card(page, id).locator(".course-body h3")).toContainText("Copy of Spanish");
  await expect(card(page, id).locator(".course-actions [data-testid=courses-copy], .course-actions [data-testid=courses-edit]")).toHaveCount(0);
  await expect(card(page, "spanish").locator(".course-body h3")).toHaveText(/^\s*Spanish\b/);
  await expect(card(page, "spanish").getByTestId("courses-edit")).toHaveCount(0);

  // The copy has not been opened yet: its Start wizard has no pencil, copy or translate icon.
  await card(page, id).getByTestId("courses-open").click();
  const wizard = card(page, id).locator('[data-testid="courses-settings-dialog"][data-start]');
  await expect(wizard).toBeVisible();
  await expect(wizard.locator('[data-testid="courses-edit"], [data-testid="courses-copy"], [data-testid="courses-translate"]')).toHaveCount(0);
  await wizard.getByTestId("courses-settings-close").click();

  // Behind the gear (once opened), the settings hold the editor's pencil
  // left of the copy icon, with its own tip. The pencil opens the editor.
  let dialog = await openSettings(page, id);
  let copy = await expectCopyIcon(dialog);
  const edit = dialog.getByTestId("courses-edit");
  await expect(edit).toHaveAttribute("data-tip", "Edit this course's files.");
  await expect(edit.locator("svg")).toHaveCount(1);
  // One measurement for both boxes: two would straddle the dialog's opening slide.
  const k = await corner(dialog);
  besides(k.edit, k.copy);
  await edit.click();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}$`));
  await expect(page.getByTestId("course-edit-title")).toHaveText("Copy of Spanish");

  // A copy of the copy is "Copy of Copy of Spanish".
  await page.goto("/courses");
  dialog = await openSettings(page, id);
  copy = await expectCopyIcon(dialog);
  await copy.click();
  await expect(page.getByTestId("course-edit-title")).toHaveText("Copy of Copy of Spanish");
});

test("copies count against the account's course bytes: past the limit the copy is refused with a notice", async ({ page }) => {
  test.setTimeout(120_000);
  const acct = newAccount("copy");
  await signUp(page, acct);
  await login(page, acct); // registering signs nobody in (#379)
  // Copy until the account is full (32 MB; the Spanish course takes 3.6 MB).
  let made = 0;
  let refused = "";
  for (let i = 0; i < 12 && !refused; i++) {
    const r = await page.request.post("/courses/spanish/copy", { maxRedirects: 0 });
    expect(r.status()).toBe(303);
    const to = r.headers()["location"] ?? "";
    if (/\/courses\/copy-of-spanish-/.test(to)) made++;
    else refused = to;
  }
  expect(made).toBeGreaterThan(1);
  expect(refused).toMatch(/\/courses\?notice=copy-no-room-\d+$/);

  // The same from the card: the notice says why, and no course was added.
  await page.goto("/courses");
  await expect(page.locator('[data-testid="courses-card"][data-course-id^="copy-of-spanish-"]')).toHaveCount(made);
  await (await openSettings(page, "spanish")).getByTestId("courses-copy").click();
  await expect(page).toHaveURL(/\/courses\?notice=copy-no-room-/);
  await expect(page.getByTestId("courses-notice")).toHaveText(
    "There is no room for a copy: all of an account's courses together may take at most 32 MB. Delete a course you no longer need.",
  );
  await expect(page.locator('[data-testid="courses-card"][data-course-id^="copy-of-spanish-"]')).toHaveCount(made);
});

// QA (#655): where the copy icon must not be, and who may copy what.
// #965: nor "Copy and translate to…"; translating it is a 404 as well.
test("no copy or translate icon in the settings of a course shared with you; copying or translating it, or another user's private course, is a 404", async ({ page, browser }) => {
  await devLogin(page, "next=/courses");
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const friend = newAccount("copyshare");
  await page.getByTestId("course-edit-share").click();
  await page.getByTestId("course-share-emails").fill(friend.email);
  await page.getByTestId("course-share-send").click();
  await expect(page.getByTestId("course-share-notice")).toHaveText("Invitation sent to 1 address.");
  const link = mailLink(await waitForMail(friend.email, /shared a course with you/), /\/courses\/shared\//);
  // A private course of the owner's, not shared.
  await page.goto("/courses");
  const priv = await createCourse(page);

  const guest = await (await browser.newContext()).newPage();
  await signUp(guest, friend);
  await login(guest, friend); // registering signs nobody in (#379)
  await guest.goto(link);
  await guest.goto("/courses");
  const card = guest.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card).toBeVisible();
  await expect(card.getByTestId("courses-card-shared")).toBeVisible();
  // Its settings dialog is there (behind the gear: the share link opened the course), without the icon.
  const dialog = await openSettings(guest, id);
  await expect(dialog.getByTestId("courses-settings-close")).toBeVisible();
  await expect(card.getByTestId("courses-copy")).toHaveCount(0);
  await expect(card.getByTestId("courses-translate")).toHaveCount(0);
  await dialog.getByTestId("courses-settings-close").click();
  const t1 = await guest.request.post(`/courses/${id}/translate`, { form: { lang: "fr" }, maxRedirects: 0 });
  expect(t1.status()).toBe(404);
  const t2 = await guest.request.post(`/courses/${priv}/translate`, { form: { lang: "fr" }, maxRedirects: 0 });
  expect(t2.status()).toBe(404);
  const r1 = await guest.request.post(`/courses/${id}/copy`, { maxRedirects: 0 });
  expect(r1.status()).toBe(404);
  const r2 = await guest.request.post(`/courses/${priv}/copy`, { maxRedirects: 0 });
  expect(r2.status()).toBe(404);
  const r3 = await guest.request.post(`/courses/no-such-course/copy`, { maxRedirects: 0 });
  expect(r3.status()).toBe(404);
  await guest.goto("/courses");
  await expect(guest.locator('[data-testid="courses-card"][data-course-id^="copy-"]')).toHaveCount(0);
});

test("no copy icon while a course is being generated, nor after it stopped; a POST says why", async ({ page }) => {
  await devLogin(page, "keys=fake&next=/courses");
  const id = await createCourse(page);
  // [fixture:timeout]: the outline never answers (it fails at the 8 s client
  // limit), so the job is still running at Cancel however slow the runner;
  // [fixture:slow]'s 3 s ran out on CI before this test got there.
  await page.getByTestId("generate-wish").fill("Fractions, slowly [fixture:timeout]");
  await page.getByTestId("generate-submit").click();
  await expect(page.getByTestId("course-ai-modal")).toHaveAttribute("data-state", "running");
  const r = await page.request.post(`/courses/${id}/copy`, { maxRedirects: 0 });
  expect(r.status()).toBe(303);
  expect(r.headers()["location"]).toMatch(/\/courses\?notice=copy-not-ready$/);
  const list = await page.context().newPage();
  await list.goto("/courses");
  const card = list.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card).toHaveAttribute("data-course-status", "generating");
  await expect(card.getByTestId("courses-copy")).toHaveCount(0);
  await list.goto("/courses?notice=copy-not-ready");
  await expect(list.getByTestId("courses-notice")).toHaveText("A course can be copied once it has been written.");
  await page.getByTestId("course-ai-modal-cancel").click();
  expect(await generationEnds(page)).toBe("failed");
  await list.goto("/courses");
  await expect(card.getByTestId("courses-copy")).toHaveCount(0);
  const r2 = await page.request.post(`/courses/${id}/copy`, { maxRedirects: 0 });
  expect(r2.headers()["location"]).toMatch(/notice=copy-not-ready$/);
});

test("at phone width the copy icon sits beside the X, inside the dialog and clear of its title", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const acct2 = newAccount("copyphone");
  await signUp(page, acct2);
  await login(page, acct2); // registering signs nobody in (#379)
  await page.request.post("/courses/spanish/copy", { maxRedirects: 0 });
  await page.goto("/courses");
  const over = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(over).toBeLessThanOrEqual(0);
  for (const id of ["spanish", (await page.locator('[data-testid="courses-card"][data-course-id^="copy-of-spanish-"]').getAttribute("data-course-id"))!]) {
    const dialog = await openSettings(page, id);
    await expectCopyIcon(dialog);
    // The title's text does not run under the icons: left of them, under
    // them where there is no room beside, or above them (since #1183 a phone
    // puts the icons on the line under the title).
    const m = await dialog.evaluate((d) => {
      const h = d.querySelector("h2")!;
      const r = document.createRange();
      r.selectNodeContents(h);
      const rects = Array.from(r.getClientRects());
      const icons = ["courses-edit", "courses-translate", "courses-copy", "courses-settings-close"]
        .map((t) => d.querySelector(`[data-testid="${t}"]`)?.getBoundingClientRect())
        .filter((b): b is DOMRect => !!b);
      return {
        textRight: Math.max(...rects.map((b) => b.right)),
        textTop: Math.min(...rects.map((b) => b.top)),
        textBottom: Math.max(...rects.map((b) => b.bottom)),
        iconsLeft: Math.min(...icons.map((b) => b.left)),
        iconsRight: Math.max(...icons.map((b) => b.right)),
        iconsTop: Math.min(...icons.map((b) => b.top)),
        iconsBottom: Math.max(...icons.map((b) => b.bottom)),
        n: icons.length,
      };
    });
    expect(m.iconsRight).toBeLessThanOrEqual(390);
    expect(m.textRight <= m.iconsLeft || m.textTop >= m.iconsBottom - 1 || m.textBottom <= m.iconsTop + 1, JSON.stringify(m)).toBe(true);
    await dialog.getByTestId("courses-settings-close").click();
    await expect(dialog).toBeHidden();
  }
});
