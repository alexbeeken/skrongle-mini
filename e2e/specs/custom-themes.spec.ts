// Themes learners make (#63): stored per account, private, worn like a
// built-in theme. Since #577 a theme is only a course's setting (the course's
// settings dialog), and the profile has none; deleting one has no page yet
// (POST /theme/delete). There is no editor yet (#64), so themes are made
// through POST /dev/theme, the storage call the editor will use.
import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";
import { devLogin, devReset } from "../lib/app";
import { newAccount, register, wearTheme } from "../lib/learner";

const ACCENT = "#1f6e5a";
const SVG = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><circle cx='5' cy='5' r='4'/></svg>";

/** A complete small theme: colours, an emblem and its own version of the default picture (an owl). */
function nightFiles(title = "Night Garden") {
  return {
    // No "id": a made theme is worn by its user-<n> id.
    "theme.json": JSON.stringify({
      title,
      description: "Moths and lanterns.",
      colors: { accent: ACCENT },
      art: { mark: "pictures/mark.svg" },
      avatars: [{ id: "noor", image: "pictures/owl.svg", label: "Lantern owl" }],
    }),
    "pictures/mark.svg": SVG,
    "pictures/owl.svg": SVG,
  };
}

/** Store a theme for whoever `request` is signed in as; its `user-<n>` id. */
async function makeTheme(request: APIRequestContext, files: Record<string, string> = nightFiles()): Promise<string> {
  const r = await request.post("/dev/theme", { data: { files } });
  expect(r.status(), await r.text()).toBe(200);
  const { id } = await r.json();
  expect(id).toMatch(/^user-\d+$/);
  return id;
}

/** Save a course's settings dialog (one form behind the card's gear, #574). */
async function saveSettings(dialog: Locator): Promise<void> {
  await dialog.getByTestId("courses-settings-save").click();
}

/** Open the Spanish course's settings behind its gear on the course list. */
async function openSettings(page: Page): Promise<Locator> {
  await page.goto("/courses");
  await page.locator('[data-testid="courses-card"][data-course-id="spanish"]').getByTestId("courses-settings").click();
  const dialog = page.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');
  await expect(dialog).toBeVisible();
  return dialog;
}
/** A theme's chip in a course's settings. */
const chip = (dialog: Locator, id: string) => dialog.locator(`[data-testid="courses-settings-theme"][data-theme-id="${id}"]`);
/** Delete a theme you made (no page offers it since #577). */
const deleteTheme = (request: APIRequestContext, id: string) => request.post("/theme/delete", { form: { theme: id }, maxRedirects: 0 });

test.beforeEach(async ({ page }) => {
  await devLogin(page);
  await devReset(page);
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("wear a theme you made in a course, see it everywhere in it, then delete it", async ({ page }) => {
  const id = await makeTheme(page.request);

  // The course's settings offer it after the built-in ones.
  const dialog = await openSettings(page);
  const mine = chip(dialog, id);
  await expect(mine).toContainText("Night Garden");
  const order = await dialog.getByTestId("courses-settings-theme").evaluateAll((els) => els.map((e) => e.getAttribute("data-theme-id")));
  expect(order[0], "Default first").toBe("");
  expect(order.at(-1), "made themes come after the built-in ones").toBe(id);
  expect(order.length, "the built-in themes are still offered").toBeGreaterThan(2);
  await mine.click();
  await saveSettings(dialog);
  await expect(dialog).toBeHidden();

  // The course home: its colours, its emblem next to the course title, its owl in the top bar.
  await page.goto("/");
  await expect(page.locator("html")).toHaveAttribute("data-theme", id);
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--accent").trim())).toBe(ACCENT);
  await expect(page.locator(".theme-tag")).toContainText("Night Garden");
  const owl = page.locator(".avatar.themed img").first();
  await expect(owl).toHaveAttribute("src", new RegExp(`/themes/${id}/pictures/owl.svg$`));
  expect((await page.request.get(`/themes/${id}/pictures/owl.svg`)).status()).toBe(200);

  // Deleted: the course is back to Default and the pictures are gone.
  expect((await deleteTheme(page.request, id)).status()).toBe(303);
  expect((await page.request.get(`/themes/${id}/pictures/owl.svg`)).status()).toBe(404);
  await page.goto("/");
  await expect(page.locator("html")).not.toHaveAttribute("data-theme", /.+/);
  await expect(page.locator(".theme-tag")).toHaveCount(0);
  await expect(page.locator(".avatar.themed")).toHaveCount(0);
  await expect(chip(await openSettings(page), id)).toHaveCount(0);
});

test("a theme you made is invisible to everyone else", async ({ page, browser }) => {
  const id = await makeTheme(page.request);
  await wearTheme(page, "spanish", id);
  await page.goto("/");
  await expect(page.locator("html")).toHaveAttribute("data-theme", id);

  const other = await browser.newContext();
  const them = await other.newPage();
  await register(them, newAccount("theme-other"), "spanish");
  const picture = `/themes/${id}/pictures/mark.svg`;
  expect((await them.request.get(picture)).status(), "another learner").toBe(404);
  const anon = await browser.newContext();
  expect((await anon.request.get(picture)).status(), "no session").toBe(404);
  await anon.close();

  // Not offered, not wearable, not deletable.
  await expect(chip(await openSettings(them), id)).toHaveCount(0);
  expect((await them.request.post("/courses/spanish/settings", { form: { theme: id }, maxRedirects: 0 })).status()).toBe(400);
  await them.goto("/");
  expect(await them.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--accent").trim())).not.toBe(ACCENT);
  expect((await deleteTheme(them.request, id)).status()).toBe(404);
  await other.close();

  // The owner still has it, still worn.
  await page.goto("/");
  await expect(page.locator("html")).toHaveAttribute("data-theme", id);
  expect((await page.request.get(picture)).status()).toBe(200);
});

test("a made theme is checked like a theme file, within the account's limits", async ({ page }) => {
  const made = async () => (await openSettings(page)).locator('[data-testid="courses-settings-theme"][data-theme-id^="user-"]');
  // Refused with the field named; nothing stored.
  const bad = { ...nightFiles(), "theme.json": JSON.stringify({ id: "x", title: "Bad", colors: { accent: "teal" } }) };
  let r = await page.request.post("/dev/theme", { data: { files: bad } });
  expect(r.status()).toBe(400);
  expect(await r.text()).toContain("accent");
  r = await page.request.post("/dev/theme", { data: { files: { "theme.json": JSON.stringify({ id: "x", title: "T" }), "evil.html": "<script>" } } });
  expect(r.status()).toBe(400);
  expect(await r.text()).toContain("evil.html");
  r = await page.request.post("/dev/theme", { data: { files: { ...nightFiles(), "pictures/owl.svg": undefined } } });
  expect(r.status(), "a picture the theme file names must be there").toBe(400);
  await expect(await made()).toHaveCount(0);

  // Five themes per account (the default), then a plain-words refusal.
  // An "id" in the file is allowed and ignored.
  const withId = { ...nightFiles("Theme 1"), "theme.json": JSON.stringify({ id: "realm", title: "Theme 1" }) };
  const first = await makeTheme(page.request, withId);
  expect(first).not.toBe("realm");
  for (let i = 2; i <= 5; i++) await makeTheme(page.request, nightFiles(`Theme ${i}`));
  r = await page.request.post("/dev/theme", { data: { files: nightFiles("Theme 6") } });
  expect(r.status()).toBe(400);
  expect(await r.text()).toContain("5 themes");
  await expect(await made()).toHaveCount(5);
  // Deleting one makes room again.
  expect((await deleteTheme(page.request, first)).status()).toBe(303);
  await expect(await made()).toHaveCount(4);
  await makeTheme(page.request, nightFiles("Theme 6"));
});

test("the course settings with your themes fit a phone", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  for (const t of ["A rather long theme title for a phone", "Second"]) await makeTheme(page.request, nightFiles(t));
  await openSettings(page);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "the page scrolls sideways").toBeLessThanOrEqual(0);
});

test("on a phone the course settings header keeps the title and its (i) on one line (#1183)", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const dialog = await openSettings(page);
  const top = (l: Locator) => l.evaluate((e) => e.getBoundingClientRect().top);
  const title = dialog.locator(".cs-titles h2");
  const info = dialog.getByTestId("courses-settings-about");
  // The (i) sits beside the title, not on a line of its own.
  expect(Math.abs((await top(info)) - (await top(title)))).toBeLessThanOrEqual(4);
  expect((await title.boundingBox())!.height, "the title wraps").toBeLessThan(32);
  // The corner tools moved beside the course name: the header is about 98px, it was 137px.
  expect((await dialog.locator(".cs-head").boundingBox())!.height).toBeLessThanOrEqual(110);
});

test("a course wears a theme of its own; outside a course the app wears Default (#565, #577)", async ({ page }) => {
  const id = await makeTheme(page.request);
  const html = page.locator("html");
  const css = page.locator('link[rel=stylesheet][href*="theme.css"]');

  // The Spanish card's gear: the course's settings, Default until a theme is picked; no "Same as profile".
  const dialog = await openSettings(page);
  const card = page.locator('[data-testid="courses-card"][data-course-id="spanish"]');
  await expect(chip(dialog, "").locator("input")).toBeChecked();
  await expect(chip(dialog, "profile")).toHaveCount(0);
  await expect(card).not.toHaveAttribute("data-card-theme");
  // Picking one shows it on the card at once; closing takes it off again.
  await chip(dialog, "rustwake").click();
  await expect(card).toHaveAttribute("data-card-theme", "rustwake");
  // The theme's banner faintly behind the text.
  const art = () => card.evaluate((e) => { const b = getComputedStyle(e, "::before"); return [b.backgroundImage, b.opacity]; });
  await expect.poll(art).toEqual([expect.stringContaining("/themes/rustwake/pictures/banner.svg"), "0.3"]);
  await dialog.getByTestId("courses-settings-close").click();
  await expect(dialog).toBeHidden();
  await expect(card).not.toHaveAttribute("data-card-theme");
  await card.getByTestId("courses-settings").click();
  await expect(chip(dialog, "").locator("input")).toBeChecked();
  // Save: no reload, the dialog closes, the card keeps the theme and nods.
  let loads = 0;
  page.on("load", () => loads++);
  await page.evaluate(() => {
    const w = window as unknown as { nods: string[] };
    w.nods = [];
    document.addEventListener("animationstart", (e) => w.nods.push(e.animationName));
  });
  await chip(dialog, id).click();
  await saveSettings(dialog);
  await expect(dialog).toBeHidden();
  await expect(card).toHaveAttribute("data-card-theme", id);
  await expect.poll(() => card.evaluate((e) => getComputedStyle(e).getPropertyValue("--accent").trim())).toBe(ACCENT);
  await expect.poll(() => page.evaluate(() => (window as unknown as { nods: string[] }).nods)).toContain("card-accepted");
  await expect(card).not.toHaveClass(/accepted/);
  expect(loads).toBe(0);
  await page.reload();
  await expect(card).toHaveAttribute("data-card-theme", id);

  // The course list and the profile wear Default (#951); the course and every page in it wear its theme.
  await expect(page).toHaveURL(/\/courses$/);
  await expect(html).not.toHaveAttribute("data-theme", /.+/);
  await expect(css).toHaveCount(0);
  for (const path of ["/", "/lessons"]) {
    await page.goto(path);
    await expect(html).toHaveAttribute("data-theme", id);
    await expect(css).toHaveAttribute("href", new RegExp(`t=${id}`));
  }
  await page.goto("/profile");
  await expect(html).not.toHaveAttribute("data-theme", /.+/);
  await expect(css).toHaveCount(0);
  // The profile has no theme to pick.
  await expect(page.locator('[data-testid="profile-theme"]')).toHaveCount(0);

  // Back to Default: the course wears none.
  await expect(chip(await openSettings(page), id).locator("input")).toBeChecked();
  await chip(dialog, "").click();
  await saveSettings(dialog);
  await expect(card).not.toHaveAttribute("data-card-theme");
  await page.goto("/");
  await expect(html).not.toHaveAttribute("data-theme", /.+/);
});

test("a themed example's mark reads right for a theme title that starts with The (#952)", async ({ page }) => {
  await wearTheme(page, "spanish", "realm");
  await page.goto("/lessons/greetings-and-introductions");
  const mark = page.locator('[data-testid="lesson-vocab-item"][data-themed="true"] .themed-mark').first();
  await expect(mark).toHaveAttribute("title", "From the theme The Realm of Eldmark");
  expect(await mark.getAttribute("title")).not.toMatch(/the The/);
});
