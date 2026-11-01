// The dev payment provider (#314): dev mode's stand-in for Stripe at
// /dev/payments/stripe/v1/…, with fake keys the server is given by default.
// The profile's plans modal (#315) pays through Stripe Checkout only (#963
// removed the pretend card payment): Purchase goes to the simulator's hosted
// Checkout page (Pay / Fail / Cancel), Pay sends signed webhooks to the app's /billing/webhook, and the
// dev mailbox's Payments section (/dev/payments) logs the API calls and
// webhooks and plays what happens later: a renewal paid or failed, a
// cancellation, an event sent again with a broken signature.
// Exact: no Stripe account, no network.
import { expect, test, type Page } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

interface PaymentsState {
  on: boolean;
  subscriptions: { id: string; status: string; metadata: Record<string, string> }[];
  log: { id: number; kind: string; what: string; status: number; event: string | null; tampered: boolean }[];
}

async function payments(page: Page): Promise<PaymentsState> {
  const r = await page.request.get("/dev/payments?format=json");
  expect(r.status(), "GET /dev/payments").toBe(200);
  return r.json();
}

/** Open the plans modal, pick `plan` and Pay with Stripe: lands on the simulator's Checkout page. */
async function payWithStripe(page: Page, plan: "basic" | "plus"): Promise<void> {
  await page.getByTestId("profile-plan-choose").click();
  const dialog = page.getByTestId("plans-dialog");
  await expect(dialog).toBeVisible();
  // Stripe is the only way to pay (#963): no card fields in the modal.
  await expect(dialog.locator("input[name=card_number]")).toHaveCount(0);
  await dialog.locator(`[data-testid="plan-card"][data-plan="${plan}"]`).click();
  await expect(dialog.getByTestId("plans-purchase")).toHaveText(plan === "plus" ? "Pay with Stripe · CA$15 a month" : "Pay with Stripe · CA$3 a month");
  await dialog.getByTestId("plans-purchase").click();
  await expect(page).toHaveURL(/\/dev\/payments\/checkout\/cs_test_dev\w+$/);
}

test("subscribe through the fake checkout, fail a renewal, cancel; a tampered webhook signature is refused", async ({ page }) => {
  // A fresh learner, so the shared dev user's plan is left alone.
  await register(page, newAccount("payer"), "spanish");
  expect((await payments(page)).on).toBe(true);

  // Free; paying with Stripe from the plans modal goes to the simulator's Checkout page, at the plan's price.
  await page.goto("/profile#subscription");
  const card = page.getByTestId("profile-subscription");
  await expect(card).toHaveAttribute("data-premium", "false");
  await payWithStripe(page, "basic");
  await expect(page.getByTestId("dev-checkout")).toHaveAttribute("data-mode", "subscription");
  await expect(page.getByTestId("dev-checkout-amount")).toHaveText("CA$3.00 per month");

  // Cancel goes back with nothing charged; Fail declines the card and stays.
  await page.getByTestId("dev-checkout-cancel").click();
  await expect(page).toHaveURL(/\/profile\?billing=cancelled/);
  await expect(card).toHaveAttribute("data-premium", "false");
  await expect(page.getByTestId("profile-notice")).toContainText("Checkout cancelled; nothing was charged.");
  await payWithStripe(page, "plus");
  await expect(page.getByTestId("dev-checkout-amount")).toHaveText("CA$15.00 per month");
  await page.getByTestId("dev-checkout-fail").click();
  await expect(page.getByTestId("dev-checkout-declined")).toHaveText("Your card was declined.");
  await expect(page.getByTestId("dev-checkout")).toHaveAttribute("data-status", "open");

  // Pay: the webhooks land before the browser is back, so the profile shows the plan.
  await page.getByTestId("dev-checkout-pay").click();
  await expect(page).toHaveURL(/\/profile\?billing=done#subscription$/);
  await expect(card).toHaveAttribute("data-premium", "true");
  await expect(card).toHaveAttribute("data-source", "stripe");
  await expect(page.getByTestId("profile-plan")).toHaveText("Skrongle Plus");
  await expect(page.getByTestId("profile-renewal-label")).toHaveText("Renewal date");
  await expect(page.getByTestId("profile-renewal")).toHaveAttribute("data-cancelled", "false");
  await expect(page.getByTestId("profile-billing-manage")).toBeVisible();
  await expect(page.getByTestId("profile-cancel-open")).toBeVisible();

  // The Payments section: the subscription, and the webhook row that made it.
  const sub = (await payments(page)).subscriptions[0];
  expect(sub.status).toBe("active");
  await page.goto("/dev/payments");
  await expect(page.getByTestId("dev-box-kind-payments")).toHaveAttribute("aria-current", "page");
  const row = page.locator(`[data-testid="dev-payments-sub"][data-id="${sub.id}"]`);
  await expect(row).toHaveAttribute("data-status", "active");
  const created = page.locator('[data-testid="dev-payments-entry"][data-kind="webhook"][data-what="customer.subscription.created"]').first();
  await expect(created).toHaveAttribute("data-status", "200");
  await expect(page.locator('[data-testid="dev-payments-entry"][data-kind="api"][data-what="POST /v1/checkout/sessions"]').first()).toHaveAttribute("data-status", "200");

  // Fail the renewal: past due on the simulator and on the profile.
  await row.getByTestId("dev-payments-fail-next").click();
  await expect(row).toHaveAttribute("data-fail-next", "true");
  await row.getByTestId("dev-payments-renew").click();
  await expect(row).toHaveAttribute("data-status", "past_due");
  await expect(page.locator('[data-testid="dev-payments-entry"][data-what="invoice.payment_failed"]').first()).toHaveAttribute("data-status", "200");
  await page.goto("/profile#subscription");
  await expect(card).toHaveAttribute("data-premium", "true");
  await expect(page.getByTestId("profile-renewal")).toContainText("The last payment failed");

  // Cancel: the plan is free again.
  await page.goto("/dev/payments");
  await row.getByTestId("dev-payments-cancel").click();
  await expect(row).toHaveAttribute("data-status", "canceled");
  await page.goto("/profile#subscription");
  await expect(card).toHaveAttribute("data-premium", "false");
  await expect(page.getByTestId("profile-plan")).toHaveText("Free");
  await expect(card).toHaveAttribute("data-source", "free");
  await expect(page.getByTestId("profile-renewal")).toHaveCount(0);

  // Basic through Stripe: the subscription's price says which plan was bought.
  await payWithStripe(page, "basic");
  await page.getByTestId("dev-checkout-pay").click();
  await expect(page).toHaveURL(/\/profile\?billing=done#subscription$/);
  await expect(card).toHaveAttribute("data-plan", "basic");
  await expect(card).toHaveAttribute("data-source", "stripe");
  await expect(page.getByTestId("profile-plan")).toHaveText("Skrongle Basic");

  // Send an event again with a broken signature: the app's webhook answers 400.
  const event = (await payments(page)).log.find((e) => e.kind === "webhook" && e.what === "customer.subscription.deleted")!;
  const entry = (await payments(page)).log.find((e) => e.event === event.event)!;
  await page.goto(`/dev/payments/log/${entry.id}`);
  await page.getByTestId("dev-payments-entry-tamper").click();
  await expect(page).toHaveURL(/\/dev\/payments(\?|$)/);
  const tampered = page.locator(`[data-testid="dev-payments-entry"][data-kind="webhook"][data-tampered="true"][data-event="${event.event}"]`).first();
  await expect(tampered).toHaveAttribute("data-status", "400");
  // And a forged request straight to the webhook is refused too.
  const forged = await page.request.post("/billing/webhook", { data: '{"id":"evt_forged","type":"customer.subscription.updated"}', headers: { "content-type": "application/json", "stripe-signature": "t=1,v1=00" } });
  expect(forged.status()).toBe(400);
});
