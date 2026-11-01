// Plans on the profile (#315): the chooser modal with the Basic (CA$3 a month)
// and Plus (CA$15 a month) cards, paying through Stripe Checkout (dev mode's
// stand-in, #314; #963 removed the pretend card payment, so a plan only
// comes from Stripe's signed webhook), the Subscription card's plan name and
// renewal date, cancelling with a reason and feedback (the plan is kept to
// the month's end), the Plus-only Skrongle AI bar, and the Plus-only "use my
// keys when my Skrongle AI limit is hit" pill on the API keys card.
// Exact: no Stripe account, no network.
import { expect, test, type Page } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

/** Open the chooser, pick `plan`, Pay with Stripe, Pay on the dev Checkout page; lands back on the profile. */
async function buy(page: Page, plan: "basic" | "plus"): Promise<void> {
  await page.getByTestId("profile-plan-choose").click();
  const dialog = page.getByTestId("plans-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("plan-card")).toHaveCount(2);
  await expect(dialog.locator('[data-testid="plan-card"][data-plan="basic"]')).toContainText("CA$3 a month");
  await expect(dialog.locator('[data-testid="plan-card"][data-plan="plus"]')).toContainText("CA$15 a month");
  // No card fields: the card is typed on Stripe's page, never on ours.
  await expect(dialog.locator("input[name=card_number]")).toHaveCount(0);
  await dialog.locator(`[data-testid="plan-card"][data-plan="${plan}"]`).click();
  await expect(dialog.locator(`[data-testid="plan-pick"][value="${plan}"]`)).toBeChecked();
  await expect(dialog.getByTestId("plans-purchase")).toContainText(plan === "plus" ? "CA$15 a month" : "CA$3 a month");
  await dialog.getByTestId("plans-purchase").click();
  await expect(page).toHaveURL(/\/dev\/payments\/checkout\/cs_test_dev\w+$/);
  await expect(page.getByTestId("dev-checkout-amount")).toHaveText(plan === "plus" ? "CA$15.00 per month" : "CA$3.00 per month");
  await page.getByTestId("dev-checkout-pay").click();
  await expect(page).toHaveURL(/\/profile\?billing=done#subscription$/);
  await expect(page.getByTestId("profile-subscription")).toHaveAttribute("data-plan", plan);
}

test("Plus: bought through Checkout, shown with its renewal date, the AI bar and the own-keys pill; cancelled, it lasts the month", async ({ page }) => {
  await register(page, newAccount("sub-plus"), "spanish");
  await page.goto("/profile#subscription");
  const section = page.getByTestId("profile-subscription");
  await expect(section).toHaveAttribute("data-plan", "free");
  await expect(page.getByTestId("profile-plan")).toHaveText("Free");
  await expect(page.getByTestId("profile-renewal")).toHaveCount(0);
  await expect(page.getByTestId("profile-ai-usage")).toHaveCount(0);

  // The pretend payment is gone: posting a card to the old route buys nothing.
  const old = await page.request.post("/billing/subscribe", { form: { plan: "plus", card_number: "4242 4242 4242 4242" } });
  expect(old.status()).toBe(404);
  await page.reload();
  await expect(section).toHaveAttribute("data-plan", "free");

  await buy(page, "plus");
  await expect(page.getByTestId("profile-plan")).toHaveText("Skrongle Plus");
  await expect(section).toHaveAttribute("data-source", "stripe");
  await expect(page.getByTestId("profile-notice")).toContainText("Your subscription is active");
  const renewal = page.getByTestId("profile-renewal");
  await expect(renewal).toHaveAttribute("data-cancelled", "false");
  await expect(renewal).toHaveText(/^\d{1,2} [A-Z][a-z]+ \d{4}$/);
  await expect(page.getByTestId("profile-renewal-label")).toHaveText("Renewal date");
  await expect(page.getByTestId("profile-plan-choose")).toHaveCount(0);
  await expect(page.getByTestId("profile-cancel-open")).toBeVisible();

  // Skrongle AI this month: a bar only, nothing in dollars.
  const usage = page.getByTestId("profile-ai-usage");
  await expect(usage).toBeVisible();
  await expect(usage).not.toContainText("$");
  const bar = page.getByTestId("profile-ai-bar");
  await expect(bar).toHaveAttribute("aria-valuenow", "0");
  await expect(page.getByTestId("profile-ai-left")).toHaveText("100 % left");
  await expect(page.locator('[data-testid="profile-nav-link"][data-section="ai-usage"]')).toBeVisible();

  // The API keys card is just the pill until it is turned on.
  const keys = page.getByTestId("profile-api-keys");
  await expect(keys).toHaveAttribute("data-plus-ai", "true");
  await expect(keys).toHaveAttribute("data-own-keys", "false");
  const pill = page.getByTestId("profile-own-keys-switch");
  await expect(pill).toContainText("Use my keys when my Skrongle AI limit is hit");
  await expect(pill).toHaveAttribute("aria-checked", "false");
  await expect(page.getByTestId("profile-own-keys-body")).toBeHidden();
  await pill.click();
  await expect(pill).toHaveAttribute("aria-checked", "true");
  await expect(keys).toHaveAttribute("data-own-keys", "true");
  await expect(page.getByTestId("profile-own-keys-body")).toBeVisible();
  await expect(page.getByTestId("profile-api-key").first()).toBeVisible();
  await page.reload();
  await expect(page.getByTestId("profile-own-keys-switch")).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("profile-own-keys-body")).toBeVisible();
  await page.getByTestId("profile-own-keys-switch").click();
  await expect(page.getByTestId("profile-own-keys-switch")).toHaveAttribute("aria-checked", "false");
  await expect(page.getByTestId("profile-own-keys-body")).toBeHidden();

  // Cancel: one confirm, with a reason and some words; Stripe ends it with the month, Plus stays until then.
  await page.getByTestId("profile-cancel-open").click();
  const cancel = page.getByTestId("cancel-dialog");
  await expect(cancel).toBeVisible();
  await expect(cancel.getByTestId("cancel-note")).toContainText("You keep Skrongle Plus until");
  await expect(cancel.getByTestId("cancel-reason")).toHaveCount(4);
  await cancel.locator('[data-testid="cancel-reason"][value="unused"]').check();
  await cancel.getByTestId("cancel-feedback").fill("Back after the holidays.");
  await cancel.getByTestId("cancel-confirm").click();
  await expect(page.getByTestId("profile-notice")).toContainText("Your subscription is cancelled.");
  await expect(page.getByTestId("profile-plan")).toHaveText("Skrongle Plus");
  await expect(page.getByTestId("profile-renewal")).toHaveAttribute("data-cancelled", "true");
  await expect(page.getByTestId("profile-renewal-label")).toHaveText("Ends on");
  await expect(page.getByTestId("profile-renewal")).toContainText("(cancelled)");
  await expect(page.getByTestId("profile-cancel-open")).toHaveCount(0);
  await expect(page.getByTestId("profile-ai-usage")).toBeVisible();

  // Choosing again opens the Billing Portal, where the cancellation is taken back.
  await expect(page.getByTestId("profile-plan-choose")).toHaveText("Choose a plan");
  await page.getByTestId("profile-plan-choose").click();
  await page.locator('[data-testid="plan-card"][data-plan="plus"]').click();
  await page.getByTestId("plans-purchase").click();
  await expect(page.getByTestId("dev-portal")).toBeVisible();
  await page.getByTestId("dev-portal-resume").click();
  await page.getByTestId("dev-portal-return").click();
  await expect(page.getByTestId("profile-renewal-label")).toHaveText("Renewal date");
  await expect(page.getByTestId("profile-cancel-open")).toBeVisible();
});

test("Basic: no Skrongle AI card or pill, an Upgrade to Plus button that opens the Billing Portal, and the chooser marks Basic as the current plan", async ({ page }) => {
  await register(page, newAccount("sub-basic"), "spanish");
  await page.goto("/profile#subscription");
  await buy(page, "basic");
  await expect(page.getByTestId("profile-plan")).toHaveText("Skrongle Basic");
  await expect(page.getByTestId("profile-renewal-label")).toHaveText("Renewal date");
  await expect(page.getByTestId("profile-ai-usage")).toHaveCount(0);
  await expect(page.getByTestId("profile-own-keys-switch")).toHaveCount(0);
  await expect(page.getByTestId("profile-own-keys-body")).toBeVisible();
  await expect(page.getByTestId("profile-plan-choose")).toHaveText("Upgrade to Plus");

  await page.getByTestId("profile-plan-choose").click();
  const basic = page.locator('[data-testid="plan-card"][data-plan="basic"]');
  await expect(basic).toHaveAttribute("data-current", "true");
  await expect(basic.getByTestId("plan-current")).toHaveText("Your plan");
  await expect(basic.getByTestId("plan-pick")).toBeDisabled();
  await page.getByTestId("plans-close").click();
  await expect(page.getByTestId("plans-dialog")).toBeHidden();

  // The modal can also be asked for by link (the Upgrade buttons where Skrongle AI runs out).
  await page.goto("/profile?plans=1#subscription");
  await expect(page.getByTestId("plans-dialog")).toBeVisible();
  // Already paying: Plus goes to the Billing Portal, not to a second Checkout.
  await page.locator('[data-testid="plan-card"][data-plan="plus"]').click();
  await page.getByTestId("plans-purchase").click();
  await expect(page.getByTestId("dev-portal")).toBeVisible();
  await expect(page.getByTestId("dev-portal-sub")).toHaveCount(1);
});

test("dev login ?plan= takes basic, plus and the old pro (now Plus) through the dev Checkout; anything else names the plans", async ({ page }) => {
  await register(page, newAccount("sub-devplan"), "spanish");
  const section = page.getByTestId("profile-subscription");
  for (const [asked, plan] of [["basic", "basic"], ["plus", "plus"], ["pro", "plus"], ["free", "free"]]) {
    await page.goto(`/dev/login?plan=${asked}&next=/profile`);
    await expect(section, `?plan=${asked}`).toHaveAttribute("data-plan", plan);
    if (plan !== "free") await expect(section, `?plan=${asked}`).toHaveAttribute("data-source", "stripe");
  }
  const bad = await page.request.get("/dev/login?plan=gold");
  expect(bad.status()).toBe(400);
  expect(await bad.text()).toContain("plan=gold: use basic, plus or free");
});
