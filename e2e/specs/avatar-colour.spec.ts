// The background behind the profile picture (#227, #248): since #565 a
// course's setting, picked in the course's settings behind its gear on the
// course list (one form since #574), saved with Save, worn by the avatar in the corner on
// the course's pages, and kept when the picture changes (#263). Slots 1-8 are
// a colour with a pattern; a theme brings its own eight colours for the same
// slots and adds scenes of its world.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

const dialog = (page: Page) => page.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');
const swatch = (page: Page, slot: number) => dialog(page).locator(`[data-testid="courses-settings-color"][value="${slot}"]`);
/** Click a colour's circle (the radio inside is visually hidden). */
const pick = (page: Page, slot: number) => swatch(page, slot).locator("xpath=..").click();
/** Open the Spanish course's settings on the course list. */
async function openSettings(page: Page) {
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator('[data-course-id="spanish"]').getByTestId("courses-settings").click();
  await expect(dialog(page)).toBeVisible();
}
async function save(page: Page) {
  const saved = page.waitForResponse((r) => r.url().endsWith("/courses/spanish/settings") && r.request().method() === "POST");
  await dialog(page).getByTestId("courses-settings-save").click();
  expect((await saved).status()).toBeLessThan(400);
  await expect(dialog(page)).toBeHidden();
}
/** The badge colour an avatar wears (its --av custom property). */
const badge = (el: Locator) => el.first().evaluate((e) => getComputedStyle(e).getPropertyValue("--av").trim().toLowerCase());
/** The picture over that colour (its --av-img custom property): `none` or a `url(…)`. */
const badgeImage = (el: Locator) => el.first().evaluate((e) => getComputedStyle(e).getPropertyValue("--av-img").trim());
const header = (page: Page) => page.locator('[data-testid="nav-account-toggle"] .avatar');
const preview = (page: Page) => dialog(page).locator(".cs-preview .avatar");

test.beforeEach(async ({ page }) => {
  await register(page, newAccount("colour"), "spanish");
});

test("a colour is picked in the course's settings and the corner wears it in the course", async ({ page }) => {
  await openSettings(page);
  const colours = dialog(page).getByTestId("courses-settings-colors");
  // The eight of the palette, by name; Amber sunburst (the default picture's) chosen by default.
  await expect(colours.getByTestId("courses-settings-color")).toHaveCount(8);
  await expect(swatch(page, 0)).toHaveCount(0);
  await expect(swatch(page, 1)).toBeChecked();
  expect(await badge(preview(page))).toBe("#c9873a");
  await expect(colours.getByRole("radio", { name: "Teal stars" })).toHaveAttribute("value", "4");

  // The preview follows the pick; Save saves it.
  await pick(page, 4);
  await expect.poll(() => badge(preview(page))).toBe("#3b8ea5");
  expect(await badgeImage(preview(page))).toContain("/static/avatar-bg/stars.svg");
  await save(page);

  // Kept: in the course's corner, and checked when the settings open again.
  await page.goto("/");
  expect(await badge(header(page))).toBe("#3b8ea5");
  expect(await badgeImage(header(page))).toContain("/static/avatar-bg/stars.svg");
  await openSettings(page);
  await expect(swatch(page, 4)).toBeChecked();

  // Another picture keeps the colour.
  await dialog(page).locator('[data-testid="courses-settings-avatar"][value="mochi"]').locator("xpath=..").click();
  expect(await badge(preview(page))).toBe("#3b8ea5");
  await save(page);
  await page.goto("/");
  await expect(page.locator('[data-testid="nav-account-toggle"] use[href="#av-mochi"]')).toHaveCount(1);
  expect(await badge(header(page))).toBe("#3b8ea5");
});

test("a theme picked in the settings adds scenes of its world, offered only while it is picked", async ({ page }) => {
  await openSettings(page);
  await dialog(page).locator('[data-testid="courses-settings-theme"][data-theme-id="realm"]').click();
  // The eight, then Eldmark's four scenes in slots 9-12.
  await expect(dialog(page).getByTestId("courses-settings-color")).toHaveCount(12);
  await expect(swatch(page, 9)).toHaveAttribute("aria-label", "Moonlit pines");
  await pick(page, 9);
  await expect.poll(() => badgeImage(preview(page))).toContain("moonlit-pines.svg");
  await save(page);
  await page.goto("/");
  expect(await badgeImage(page.locator('[data-testid="nav-account-toggle"] .avatar.themed'))).toContain("/themes/realm/pictures/backgrounds/moonlit-pines.svg");

  // Another theme's scene for the same slot.
  await openSettings(page);
  await dialog(page).locator('[data-testid="courses-settings-theme"][data-theme-id="meadowbrook"]').click();
  await expect(swatch(page, 9)).toHaveAttribute("aria-label", "Crop fields");
  await expect(swatch(page, 9)).toBeChecked();
  // Default has no scenes: they go, and the first background (Amber sunburst) is chosen.
  await dialog(page).locator('[data-testid="courses-settings-theme"][data-theme-id=""]').click();
  await expect(dialog(page).getByTestId("courses-settings-color")).toHaveCount(8);
  await expect(swatch(page, 1)).toBeChecked();
  // Closed without Save: Eldmark and its pines are still what is saved.
  await dialog(page).getByTestId("courses-settings-close").click();
  await openSettings(page);
  await expect(dialog(page).locator('[data-testid="courses-settings-theme"][data-theme-id="realm"] input')).toBeChecked();
  await expect(swatch(page, 9)).toBeChecked();
});

test("a theme brings its own colours for the same slot", async ({ page }) => {
  await openSettings(page);
  await pick(page, 3);
  await expect.poll(() => badge(preview(page))).toBe("#c2556f"); // Rose
  // Slot 3 is Ember in Eldmark: the swatch and the preview follow.
  await dialog(page).locator('[data-testid="courses-settings-theme"][data-theme-id="realm"]').click();
  await expect(swatch(page, 3)).toHaveAttribute("aria-label", "Ember");
  await expect(swatch(page, 3)).toBeChecked();
  await expect.poll(() => badge(preview(page))).toBe("#a5432a");
  await save(page);
  await page.goto("/");
  expect(await badge(page.locator('[data-testid="nav-account-toggle"] .avatar.themed'))).toBe("#a5432a");
});
