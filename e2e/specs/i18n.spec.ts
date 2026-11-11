// The app in the learner's language (#462): the picker on the register and
// login pages (a cookie), the account keeping the choice, and the profile
// changing it later. English, Spanish and every other i18n/<code>.yaml ship;
// the pickers list them all.
import { expect, test } from "@playwright/test";
import { readdirSync, readFileSync } from "node:fs";
import { newAccount, login, logout } from "../lib/learner";
import { devLogin, devReset } from "../lib/app";

// Every shipped language's own name for itself (`meta.name` in i18n/<code>.yaml),
// which is what the pickers show.
const I18N_DIR = new URL("../../i18n/", import.meta.url);
function shippedLanguageNames(): string[] {
  return readdirSync(I18N_DIR)
    .filter((f) => f.endsWith(".yaml"))
    .map((f) => /^meta:\n  name: (.+)$/m.exec(readFileSync(new URL(f, I18N_DIR), "utf8"))?.[1]?.trim() ?? f);
}

// The Spanish texts the pages must show (i18n/es.yaml).
const ES = {
  loginTitle: "Iniciar sesión",
  registerTitle: "Crear una cuenta",
  navLogout: "Cerrar sesión",
  navProfile: "Ajustes del perfil",
  languageLabel: "Idioma",
  ageLabel: /^Tengo al menos \d+ años$/,
};

test.describe("languages", () => {
  test("a visitor picks Spanish on the register page, the account keeps it, the profile changes it back", async ({ page }) => {
    const account = newAccount("i18n");

    // Logged out: English by default (the browser asks for en), the picker shows every language.
    await page.goto("/register");
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.locator("h1")).toHaveText("Create an account");
    const picker = page.getByTestId("lang-picker");
    await expect(picker).toHaveValue("en");
    const options = await picker.locator("option").allTextContents();
    const shipped = shippedLanguageNames();
    expect(shipped).toEqual(expect.arrayContaining(["English", "Español"]));
    expect(options).toEqual(expect.arrayContaining(shipped));
    expect(options).toHaveLength(shipped.length);

    // Picking Spanish reloads the page in Spanish, and the choice follows to the login page.
    await picker.selectOption("es");
    await page.waitForURL(/\/register$/);
    await expect(page.locator("html")).toHaveAttribute("lang", "es");
    await expect(page.locator("h1")).toHaveText(ES.registerTitle);
    await page.goto("/login");
    await expect(page.locator("h1")).toHaveText(ES.loginTitle);
    await expect(page.getByTestId("lang-picker")).toHaveValue("es");

    // Registering keeps the choice on the account.
    await page.goto("/register");
    await page.getByTestId("register-email").fill(account.email);
    await page.getByTestId("register-password").fill(account.password);
    // The age box, in Spanish. Only the first account on a server (the
    // owner's) is not asked (#126), and a dev-mode server makes the dev user
    // when it starts, so this account is never the first.
    await expect(page.getByTestId("register-age-row").locator(".check-row-label")).toHaveText(ES.ageLabel);
    await page.getByTestId("register-age").check();
    await page.getByTestId("register-submit").click();
    await page.waitForLoadState("domcontentloaded");
    // Registering signs the account in, in the chosen language.
    await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", account.email);
    await expect(page.locator("html")).toHaveAttribute("lang", "es");
    await expect(page.getByTestId("nav-logout")).toHaveText(ES.navLogout);
    await expect(page.getByTestId("nav-profile")).toHaveText(ES.navProfile);

    // The account's language wins over the cookie: clear it, sign in again, still Spanish.
    await logout(page);
    await page.context().clearCookies();
    await page.goto("/login");
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await login(page, account);
    await expect(page.locator("html")).toHaveAttribute("lang", "es");

    // The profile has the same picker; changing it saves at once like any
    // other field (yellow circle, green check on its label) and the page
    // then comes back in English, still showing the pick as saved.
    await page.goto("/profile");
    await expect(page.getByTestId("profile-lang")).toHaveValue("es");
    await expect(page.getByTestId("profile-lang-label")).toContainText(ES.languageLabel);
    await page.getByTestId("profile-lang").selectOption("en");
    await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saving");
    await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saved");
    await expect(page.getByTestId("profile-lang-label")).toHaveAttribute("data-saved", "");
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.getByTestId("profile-lang")).toHaveValue("en");
    await expect(page.getByTestId("profile-lang-label")).toContainText("Language");
    await expect(page.getByTestId("profile-save-status")).toBeVisible();
    await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saved");
    await expect(page.getByTestId("profile-lang-label")).toHaveAttribute("data-saved", "");
    await expect(page.getByTestId("nav-logout")).toHaveText("Log out");
  });

  test("the browser's language is used before any pick, and an unknown one falls back to English", async ({ browser }) => {
    const es = await browser.newContext({ locale: "es-MX" });
    const page = await es.newPage();
    await page.goto("/login");
    await expect(page.locator("html")).toHaveAttribute("lang", "es");
    await expect(page.locator("h1")).toHaveText(ES.loginTitle);
    await es.close();

    const cy = await browser.newContext({ locale: "cy-GB" });
    const page2 = await cy.newPage();
    await page2.goto("/login");
    await expect(page2.locator("html")).toHaveAttribute("lang", "en");
    await expect(page2.locator("h1")).toHaveText("Log in");
    // A bad value on /lang is refused.
    const r = await page2.request.post("/lang", { form: { lang: "xx", back: "/login" }, maxRedirects: 0 });
    expect(r.status()).toBe(400);
    await cy.close();
  });

  test("/dev/reset forgets the dev user's saved language, so the lang cookie decides again (#1363)", async ({ page, context }) => {
    await devLogin(page);
    // Saving English for the account: the account's language beats the cookie.
    const r = await page.request.post("/lang", { form: { lang: "en", back: "/courses" }, maxRedirects: 0 });
    expect(r.status()).toBe(303);
    await devReset(page);
    await context.addCookies([{ name: "lang", value: "fr", url: page.url() }]);
    await page.goto("/courses");
    await expect(page.locator("html")).toHaveAttribute("lang", "fr");
  });
});
