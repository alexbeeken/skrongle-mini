// What the release specs (specs/release, Playwright project `release`) may
// use: only what a self-hoster has. People get in through /register and
// /login, the owner through the first-run owner link, learners through an
// invitation link from Manage Users. Nothing here, and nothing in
// specs/release, may touch a /dev/ route: a release binary has none, so
// every context made here fails the test on a request to one.
//
// The server is whatever E2E_BASE_URL points at: a tarball started by
// scripts/release-qa/package-smoke.sh (KEEP=1), a container from
// scripts/release-qa/compose.yaml, a VM, a Pi, or a dev build run the way
// scripts/release-e2e.sh runs it. It needs SKRONGLE_SIGNUP=invite (the
// systemd unit's setting) and one of
//   E2E_OWNER_LINK                          the first-run owner link from the log
//   E2E_OWNER_EMAIL + E2E_OWNER_PASSWORD    an owner that already exists
// A run needs the server's unused owner link, so use a fresh server for each run.
import { expect, type Browser, type BrowserContext, type Page } from "@playwright/test";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test as base } from "./fixtures.ts";
import { login, newAccount, type Account } from "./learner.ts";

export { expect };

/** Where the first-run spec leaves the owner it made, for the specs after it. */
const OWNER_FILE = fileURLToPath(new URL("../results/release-owner.json", import.meta.url));

const DEV_PATH = /^\/dev(\/|$)/;

/** Requests to /dev/ routes made in the running test, and the contexts it opened. Specs run one at a time. */
const state: { hits: string[]; contexts: BrowserContext[] } = { hits: [], contexts: [] };

/** Refuse a request to a /dev/ route, and remember it so that the test fails. */
async function guardDevRoutes(context: BrowserContext): Promise<void> {
  await context.route(
    (u) => DEV_PATH.test(u.pathname),
    async (route) => {
      state.hits.push(route.request().url());
      await route.abort("blockedbyclient");
    },
  );
}

/** `test` for the release specs: the smoke specs' (seeded draws, a fake voice, no page that answers with an HTTP error), and any /dev/ request fails the test. */
export const test = base.extend<{ devGuard: void }>({
  devGuard: [
    async ({ context }, use) => {
      state.hits = [];
      state.contexts = [];
      await guardDevRoutes(context);
      await use();
      for (const c of state.contexts) await c.close().catch(() => {});
      expect(state.hits, "requests to /dev/ routes, which a release server does not have").toEqual([]);
    },
    { auto: true },
  ],
});

/** A browser context of its own (its own cookies: another person), guarded like the test's and closed with it. */
export async function newPerson(browser: Browser): Promise<{ context: BrowserContext; page: Page }> {
  const context = await browser.newContext();
  await guardDevRoutes(context);
  state.contexts.push(context);
  return { context, page: await context.newPage() };
}

export function baseURL(): string {
  const u = process.env.E2E_BASE_URL;
  if (!u) throw new Error("E2E_BASE_URL is not set: point it at the server under test (scripts/release-e2e.sh starts one)");
  return u.replace(/\/$/, "");
}

/** The owner link from the environment, when there is one. */
export function ownerLink(): string | undefined {
  return process.env.E2E_OWNER_LINK || undefined;
}

/** The owner's account: the one named in the environment, or the one the first-run spec made through the owner link. */
export function ownerAccount(): Account {
  const email = process.env.E2E_OWNER_EMAIL;
  const password = process.env.E2E_OWNER_PASSWORD;
  if (email && password) return { email, password };
  try {
    const saved = JSON.parse(readFileSync(OWNER_FILE, "utf8"));
    if (saved.baseURL === baseURL() && saved.link === ownerLink()) return { email: saved.email, password: saved.password };
  } catch {
    // none yet
  }
  throw new Error("no owner: set E2E_OWNER_LINK (the first-run spec makes the owner with it, and runs first) or E2E_OWNER_EMAIL and E2E_OWNER_PASSWORD");
}

/** Remember the owner the first-run spec made. */
export function saveOwner(account: Account): void {
  mkdirSync(new URL("../results/", import.meta.url), { recursive: true });
  writeFileSync(OWNER_FILE, JSON.stringify({ baseURL: baseURL(), link: ownerLink(), ...account }));
}

/** Fill in and send the /register page that is open (the invitation, if any, is in its address); the account is signed in. */
export async function registerHere(page: Page, account: Account): Promise<void> {
  await page.getByTestId("register-email").fill(account.email);
  await page.getByTestId("register-password").fill(account.password);
  // The owner's form has no age box (#126), nor has anyone's while new
  // accounts start as Student, a release server's default (#206).
  const age = page.getByTestId("register-age");
  if (await age.count()) await age.check();
  await page.getByTestId("register-submit").click();
  await page.waitForLoadState("domcontentloaded");
  if (await page.getByTestId("register-error").isVisible()) throw new Error(`registration failed: ${await page.getByTestId("register-error").innerText()}`);
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", account.email.toLowerCase());
}

/** Open an invitation (or owner) link and register through it; the account is signed in. */
export async function registerWithLink(page: Page, link: string, account: Account): Promise<void> {
  await page.goto(link);
  await registerHere(page, account);
}

/** Sign in as the owner through /login. */
export async function signInOwner(page: Page): Promise<Account> {
  const owner = ownerAccount();
  await login(page, { email: owner.email.toLowerCase(), password: owner.password });
  return owner;
}

/** As the owner on `page`: make an invitation link on Manage Users; the link it shows. */
export async function makeInvite(page: Page): Promise<string> {
  await page.goto("/manage");
  await page.getByTestId("manage-invite-make").click();
  const input = page.getByTestId("manage-invite-link");
  await expect(input).toHaveValue(/\/register\?invite=[A-Za-z0-9_-]+$/);
  return input.inputValue();
}

/** The owner, signed in, in a person of their own. */
export async function ownerPerson(browser: Browser): Promise<{ context: BrowserContext; page: Page; owner: Account }> {
  const p = await newPerson(browser);
  const owner = await signInOwner(p.page);
  return { ...p, owner };
}

/** As the owner on `page`: give the account `email` the preset named `preset` through its Permissions dialog on Manage Users. */
export async function givePreset(page: Page, email: string, preset: string): Promise<void> {
  await page.goto("/manage");
  await page.getByTestId("manage-account-row").filter({ hasText: email.toLowerCase() }).getByTestId("manage-account-permissions").click();
  const dialog = page.getByTestId("manage-permissions-dialog");
  await dialog.getByRole("radio", { name: preset, exact: true }).check();
  await dialog.getByTestId("manage-permissions-save").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Permissions saved for ${email.toLowerCase()}`);
}

/** A new learner registered through an invitation the owner makes, signed in, in a person of their own.
 * New accounts start as Student (#142); `preset` (Teacher, say) is given to this one by the owner before it is returned. */
export async function newLearner(browser: Browser, prefix: string, preset?: string): Promise<{ context: BrowserContext; page: Page; account: Account }> {
  const o = await ownerPerson(browser);
  const link = await makeInvite(o.page);
  const p = await newPerson(browser);
  const account = newAccount(prefix);
  await registerWithLink(p.page, link, account);
  if (preset) {
    await givePreset(o.page, account.email, preset);
    await p.page.goto("/courses"); // the page it landed on was made before the preset
  }
  await o.context.close();
  return { ...p, account };
}
