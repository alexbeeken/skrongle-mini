// Verb Drill setup (#683): when only a later set has unlocked verbs, the page
// opens on that set, so Start drill works straight away.
import { expect, test } from "../../lib/fixtures.ts";
import { watchBugs } from "../../lib/games.ts";
import { newAccount, register } from "../../lib/learner.ts";

test("the drill opens on the first set with unlocked verbs, not Present", async ({ page }, info) => {
  const bugs = watchBugs(page, info);
  await register(page, newAccount("drill"), "spanish");
  for (const lesson of ["imperfect", "pluperfect"]) {
    const r = await page.request.post("/api/progress/pass", { data: { lesson, answered: 5, correct: 5, evidence: "drill spec: unlocks the imperfect" } });
    expect((await r.json()).passed, `${lesson} passed`).toBe(true);
  }
  await bugs.go("/play/conjugation");
  // One set shows at a time (#837): the chosen one is the hidden input's value and the current tab.
  await expect(page.getByTestId("drill-setup-set")).toHaveValue("imperfect");
  await expect(page.locator('[data-testid="drill-setup-sets-tab"][data-key="imperfect"]')).toHaveAttribute("aria-current", "page");
  await expect(page.locator('[data-testid="drill-setup-sets-tab"][data-key="present"]')).not.toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("drill-setup-start")).toBeEnabled();
  bugs.none();
});

// Without JavaScript (#858) the form sends the ticked verbs as item-<set>
// fields and leaves "items" empty; the server reads those, so the drill starts.
test("Start drill works without JavaScript", async ({ page, browser }) => {
  await register(page, newAccount("drill-nojs"), "spanish");
  for (const lesson of ["imperfect", "pluperfect"]) {
    const r = await page.request.post("/api/progress/pass", { data: { lesson, answered: 5, correct: 5, evidence: "drill spec: unlocks the imperfect" } });
    expect((await r.json()).passed, `${lesson} passed`).toBe(true);
  }
  const ctx = await browser.newContext({ javaScriptEnabled: false, storageState: await page.context().storageState() });
  const plain = await ctx.newPage();
  await plain.goto("/play/conjugation");
  const picked = await plain.locator('[data-testid="drill-setup-item-check"]:checked').evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value));
  expect(picked.length, "ticked verbs").toBeGreaterThan(0);
  await plain.getByTestId("drill-setup-start").click();
  const drill = plain.getByTestId("drill");
  await expect(drill).toHaveAttribute("data-set", "imperfect");
  expect((await drill.getAttribute("data-items"))?.split(",").sort()).toEqual(picked.map((v) => v.toLowerCase()).sort());
  // The drill itself needs script (#949): the page says so instead of "Dealing…".
  await expect(plain.getByTestId("drill-noscript")).toBeVisible();
  await expect(plain.getByTestId("drill-status")).toBeHidden();
  // Nothing ticked: back on the setup page, with a line that says why.
  await plain.goto("/play/conjugation");
  for (const box of await plain.locator('[data-testid="drill-setup-item-check"]:not([disabled])').all()) await box.uncheck();
  await plain.getByTestId("drill-setup-start").click();
  await expect(plain).toHaveURL(/\/play\/conjugation\?set=imperfect&empty=1$/);
  await expect(plain.getByTestId("drill-setup-pick-one")).toBeVisible();
  await expect(plain.getByTestId("drill-setup-set")).toHaveValue("imperfect");
  await ctx.close();
});
