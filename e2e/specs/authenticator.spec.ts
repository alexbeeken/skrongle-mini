// The dev authenticator (#291): an authenticator app in the dev mailbox
// page. It takes an otpauth://totp/ link (pasted, or "scanned" through
// /dev/authenticator?uri=) or a setup key and shows each account's current
// RFC 6238 code, which must match what Node's crypto works out. No sign-in:
// the dev mailbox is open to whoever is testing.
import { expect, test, type Locator } from "@playwright/test";
import { base32Encode, totpForStep } from "../lib/totp";
import { randomBytes } from "node:crypto";

function setup(prefix: string, extra = "") {
  const issuer = `${prefix}-${Date.now()}-${Math.floor(Math.random() * 1000)}`;
  const secret = base32Encode(randomBytes(20));
  const account = `${issuer}@example.com`;
  const uri = `otpauth://totp/${encodeURIComponent(issuer)}:${encodeURIComponent(account)}?secret=${secret}&issuer=${encodeURIComponent(issuer)}${extra}`;
  return { issuer, secret, account, uri };
}

// Everything a row says about its code, read in one evaluation so it all
// comes from the same render. devbox.js replaces the list when a step
// expires (and every few seconds), so two separate reads can straddle a
// rollover and pair one step's expiry with the next step's code (#557).
async function snapshot(row: Locator) {
  return row.evaluate((el: HTMLElement) => ({
    expires: Number(el.dataset.expires),
    period: Number(el.dataset.period),
    code: el.dataset.code ?? "",
    copy: el.querySelector<HTMLElement>("[data-copy]")?.dataset.copy ?? "",
    shown: el.querySelector('[data-testid="dev-authenticator-code"]')?.textContent?.trim() ?? "",
    left: Number(el.querySelector('[data-testid="dev-authenticator-remaining"]')?.textContent),
  }));
}

test("a pasted otpauth link is added, shows the RFC 6238 code with a countdown, copies it, and is removed (#291)", async ({ page, context }) => {
  const a = setup("paste");
  await page.goto("/dev/authenticator");
  await expect(page.getByTestId("dev-box-kind-auth")).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("dev-authenticator-uri")).toHaveValue(/^otpauth:\/\/totp\/Skrongle:ann@example\.org\?secret=JBSWY3DPEHPK3PXP/);
  await page.evaluate(() => ((window as unknown as { stay: boolean }).stay = true));
  await page.getByTestId("dev-authenticator-uri").fill(a.uri);
  await page.getByTestId("dev-authenticator-add").click();

  const row = page.locator(`[data-testid="dev-authenticator-item"][data-issuer="${a.issuer}"]`);
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-account", a.account);
  await expect(row.getByTestId("dev-authenticator-issuer")).toHaveText(a.issuer);
  await expect(page.getByTestId("dev-authenticator-uri")).toHaveValue("");
  expect(await page.evaluate(() => (window as unknown as { stay?: boolean }).stay)).toBe(true);

  // The code is the one for the step that ends at data-expires, shown in
  // groups of three, with the seconds left in that step.
  const snap = await snapshot(row);
  expect(snap.period).toBe(30);
  const step = snap.expires / 30 - 1;
  expect(snap.code).toBe(totpForStep(a.secret, step));
  expect(snap.shown).toBe(`${snap.code.slice(0, 3)} ${snap.code.slice(3)}`);
  expect(snap.left).toBeGreaterThanOrEqual(0);
  expect(snap.left).toBeLessThanOrEqual(30);

  // Copy puts the code on the clipboard: the one in the snapshot, or the next
  // step's if the 30 s step ran out between the snapshot and the click.
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  expect(snap.copy).toBe(snap.code);
  const either = [snap.code, totpForStep(a.secret, step + 1)];
  await row.getByTestId("dev-authenticator-copy").click();
  await expect.poll(async () => either.includes(await page.evaluate(() => navigator.clipboard.readText()))).toBe(true);

  await row.getByTestId("dev-authenticator-remove").click();
  await expect(row).toHaveCount(0);
  const r = await page.request.get("/dev/authenticator", { params: { format: "json" } });
  expect((await r.json()).accounts.some((x: { issuer: string }) => x.issuer === a.issuer)).toBe(false);
});

test("the next code shows by itself when the period runs out, and a link's settings are honoured", async ({ page }) => {
  // A 2-second period so the rollover comes quickly; SHA-1 with 8 digits.
  const a = setup("period", "&digits=8&period=2");
  await page.goto("/dev/authenticator");
  await page.getByTestId("dev-authenticator-uri").fill(a.uri);
  await page.getByTestId("dev-authenticator-add").click();
  const row = page.locator(`[data-testid="dev-authenticator-item"][data-issuer="${a.issuer}"]`);
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("SHA1, 8 digits, 2 s");
  const first = await snapshot(row);
  expect(first.period).toBe(2);
  expect(first.code).toBe(totpForStep(a.secret, first.expires / 2 - 1, 8));
  // No reload: the page swaps in a later step's code by itself. Each snapshot
  // is one render, so the later code must belong to the step ending at the
  // later expiry, however many rollovers happen in between.
  let later = first;
  await expect
    .poll(async () => (later = await snapshot(row)).expires, { timeout: 8_000 })
    .toBeGreaterThan(first.expires);
  expect(later.code).toBe(totpForStep(a.secret, later.expires / 2 - 1, 8));
  expect(later.code).not.toBe(first.code);
  await row.getByTestId("dev-authenticator-remove").click();
  await expect(row).toHaveCount(0);
});

test("a scan link opens the form filled in, a setup key works too, and a bad key says why", async ({ page }) => {
  const a = setup("scan");
  await page.goto(`/dev/authenticator?uri=${encodeURIComponent(a.uri)}`);
  await expect(page.getByTestId("dev-authenticator-uri")).toHaveValue(a.uri);
  await page.getByTestId("dev-authenticator-add").click();
  const row = page.locator(`[data-testid="dev-authenticator-item"][data-issuer="${a.issuer}"]`);
  await expect(row).toHaveCount(1);
  await expect(page).toHaveURL(/\/dev\/authenticator$/);
  // Scanning the same link again replaces the entry instead of adding a second.
  await page.goto(`/dev/authenticator?uri=${encodeURIComponent(a.uri)}`);
  await page.getByTestId("dev-authenticator-add").click();
  await expect(page).toHaveURL(/\/dev\/authenticator$/);
  await expect(row).toHaveCount(1);
  // Remove posts over fetch and re-renders the list; once it is empty the box
  // is refilled with the example account. Wait for that before typing the key,
  // or the refill lands on top of it and the example is what gets added.
  await row.getByTestId("dev-authenticator-remove").click();
  await expect(row).toHaveCount(0);

  // A bare setup key, in the groups of four apps show: no name, the defaults.
  const key = base32Encode(randomBytes(10));
  const grouped = key.toLowerCase().match(/.{1,4}/g)!.join(" ");
  const before = (await page.request.get("/dev/authenticator?format=json").then((r) => r.json())).accounts.length;
  await page.getByTestId("dev-authenticator-uri").fill(grouped);
  await page.getByTestId("dev-authenticator-add").click();
  await expect.poll(async () => (await page.request.get("/dev/authenticator?format=json").then((r) => r.json())).accounts.length).toBe(before + 1);
  const added = (await page.request.get("/dev/authenticator?format=json").then((r) => r.json())).accounts.find((x: { secret: string }) => x.secret === key);
  expect(added).toMatchObject({ issuer: "", account: "", digits: 6, period: 30, algorithm: "SHA1" });
  const keyRow = page.locator(`[data-testid="dev-authenticator-item"][data-id="${added.id}"]`);
  await keyRow.getByTestId("dev-authenticator-remove").click();
  await expect(keyRow).toHaveCount(0);

  await page.getByTestId("dev-authenticator-uri").fill("otpauth://hotp/x?secret=JBSWY3DPEHPK3PXP&counter=0");
  await page.getByTestId("dev-authenticator-add").click();
  await expect(page.getByTestId("dev-authenticator-error")).toContainText("time-based");
});

test("when the list goes empty, an empty add box offers the example account again (#291)", async ({ page }) => {
  // Other specs share the server, so the poll is answered with an empty list.
  await page.goto("/dev/authenticator");
  const box = page.getByTestId("dev-authenticator-uri");
  const example = (await box.getAttribute("data-example"))!;
  expect(example).toMatch(/^otpauth:\/\/totp\/Skrongle:/);
  await box.fill("");
  await page.route("**/dev/authenticator/rows", (route) =>
    route.fulfill({ json: { html: '<li class="devbox-empty" data-testid="dev-authenticator-empty">No accounts yet.</li>', now: Math.floor(Date.now() / 1000) } }),
  );
  await expect(box).toHaveValue(example);
});
