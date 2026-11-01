use super::*;
use axum::routing::{get, post};
use axum::Router;
use std::sync::Mutex;

const SECRET: &str = "whsec_test";

fn config(api_base: String) -> billing::StripeConfig {
    billing::StripeConfig {
        secret_key: "sk_test_x".into(),
        webhook_secret: SECRET.into(),
        price_id: "price_monthly".into(),
        price_id_basic: None,
        price_label: "€4 a month".into(),
        public_url: "https://learn.example".into(),
        api_base,
    }
}

/// A stand-in for Stripe's API: records each form body, answers with a session URL.
async fn mock_stripe(seen: Arc<Mutex<Vec<String>>>) -> String {
    let record = |url: &'static str| {
        let seen = seen.clone();
        post(move |body: String| async move {
            seen.lock().unwrap().push(body);
            axum::Json(serde_json::json!({ "url": url }))
        })
    };
    let seen_customers = seen.clone();
    let app = Router::new()
        .route(
            "/v1/customers",
            post(move |headers: HeaderMap, body: String| async move {
                let key = headers.get("idempotency-key").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                seen_customers.lock().unwrap().push(format!("customer {key} {body}"));
                axum::Json(serde_json::json!({ "id": "cus_mock" }))
            }),
        )
        .route("/v1/checkout/sessions", record("https://checkout.example/s1"))
        .route("/v1/billing_portal/sessions", record("https://portal.example/p1"))
        .route("/v1/subscriptions/{id}", record("unused"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// The real router plus a page and an API route behind `RequirePremium`.
async fn serve(state: Arc<AppState>) -> String {
    let gated = Router::new()
        .route("/paid", get(|_: billing::RequirePremium| async { "paid" }))
        .route("/api/paid", get(|_: billing::RequirePremium| async { "paid" }))
        .with_state(state.clone());
    let app = crate::routes::router(state).merge(gated);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

struct Client {
    base: String,
    agent: ureq::Agent,
    cookie: String,
}

impl Client {
    fn new(base: String) -> Client {
        let agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        Client { base, agent, cookie: String::new() }
    }

    fn result(mut r: http::Response<ureq::Body>) -> (u16, String, String) {
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
    }

    fn get(&mut self, path: &str) -> (u16, String, String) {
        let r = self.agent.get(&format!("{}{path}", self.base)).header("cookie", &self.cookie).call().unwrap();
        if let Some(c) = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()) {
            self.cookie = c.split(';').next().unwrap().to_string();
        }
        Self::result(r)
    }

    fn post(&self, path: &str) -> (u16, String, String) {
        Self::result(self.agent.post(&format!("{}{path}", self.base)).header("cookie", &self.cookie).send_empty().unwrap())
    }

    fn post_form(&self, path: &str, form: &[(&str, &str)]) -> (u16, String, String) {
        Self::result(self.agent.post(&format!("{}{path}", self.base)).header("cookie", &self.cookie).send_form(form.iter().copied()).unwrap())
    }

    fn webhook(&self, body: &str, signature: &str) -> u16 {
        let r = self
            .agent
            .post(&format!("{}/billing/webhook", self.base))
            .header("content-type", "application/json")
            .header("stripe-signature", signature)
            .send(body)
            .unwrap();
        r.status().as_u16()
    }
}

fn run<T: Send + 'static>(f: impl FnOnce(tokio::runtime::Handle) -> T + Send + 'static) -> T {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let handle = rt.handle().clone();
    rt.block_on(async move { tokio::task::spawn_blocking(move || f(handle)).await.unwrap() })
}

#[test]
fn subscribe_through_stripe_then_use_a_paid_feature() {
    run(|rt| {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stripe_url = rt.block_on(mock_stripe(seen.clone()));
        let state = crate::routes::test_state(true, Some(config(stripe_url)));
        let mut app = Client::new(rt.block_on(serve(state.clone())));

        assert_eq!(app.get("/paid").1, "/login");
        assert_eq!(app.get("/dev/login").0, 303);
        let billing_ref: String =
            state.db.with(|c| c.query_row("SELECT billing_ref FROM users WHERE email = ?1", [crate::dev::DEV_EMAIL], |r| r.get(0))).unwrap();

        // Free: the page sends the learner to subscribe, the API answers 402.
        assert_eq!(app.get("/paid"), (303, "/profile?upgrade=1#subscription".into(), String::new()));
        assert_eq!(app.get("/api/paid").0, 402);
        let (code, _, page) = app.get("/profile?upgrade=1");
        assert_eq!(code, 200);
        // The plans modal opens on load, with Plus's price.
        assert!(page.contains("That feature comes with a subscription.") && page.contains("data-open-on-load") && page.contains("CA$15 a month"), "{page}");

        // Checkout: a Stripe customer first, then off to Stripe, with the
        // price, the customer, the account's billing_ref (not its id) and
        // the plan on the subscription.
        assert_eq!(app.post_form("/billing/checkout", &[]).1, "https://checkout.example/s1");
        let form = seen.lock().unwrap().pop().unwrap();
        assert!(form.contains("price%5D=price_monthly") && form.contains(&format!("%5Bbilling_ref%5D={billing_ref}")) && form.contains("%5Bplan%5D=plus"), "{form}");
        assert!(form.contains("customer=cus_mock") && !form.contains("user_id"), "{form}");
        assert!(seen.lock().unwrap().pop().unwrap().starts_with(&format!("customer customer-{billing_ref} ")));
        // Basic has no price of its own here, so it is not sold (#963: the price decides the plan).
        let before = seen.lock().unwrap().len();
        assert_eq!(app.post_form("/billing/checkout", &[("plan", "basic")]).0, 400);
        assert_eq!(app.post_form("/billing/checkout", &[("plan", "gold")]).0, 400);
        assert_eq!(seen.lock().unwrap().len(), before);

        // Only a signed webhook changes the plan.
        let body = serde_json::json!({
            "id": "evt_1", "type": "customer.subscription.created", "created": crate::db::now(),
            "data": { "object": { "id": "sub_1", "customer": "cus_mock", "status": "active", "created": crate::db::now(),
                "metadata": { "billing_ref": billing_ref }, "cancel_at_period_end": false,
                "items": { "data": [ { "price": { "id": "price_monthly" }, "current_period_end": 1_900_000_000 } ] } } }
        })
        .to_string();
        assert_eq!(app.webhook(&body, "t=1,v1=00"), 400);
        assert_eq!(app.get("/api/paid").0, 402);
        assert_eq!(app.webhook(&body, &billing::sign(SECRET, body.as_bytes(), crate::db::now())), 200);
        assert_eq!(app.get("/paid"), (200, String::new(), "paid".into()));
        let page = app.get("/profile").2;
        let plus = format!("{} Plus", crate::APP_NAME);
        assert!(page.contains(&format!(r#"data-testid="profile-plan">{plus}"#)) && page.contains(r#"profile-renewal-label">Renewal date</dt>"#) && page.contains("17 March 2030") && page.contains("profile-cancel-open"), "{page}");

        // Premium: checkout and the portal both open the Billing Portal for the customer.
        assert_eq!(app.post_form("/billing/checkout", &[]).1, "https://portal.example/p1");
        assert!(seen.lock().unwrap().pop().unwrap().contains("customer=cus_mock"));
        assert_eq!(app.post("/billing/portal").1, "https://portal.example/p1");
        assert!(seen.lock().unwrap().pop().unwrap().contains("customer=cus_mock"));

        // Cancelling asks Stripe to end it with the period; the plan stays until then.
        assert_eq!(app.post_form("/billing/cancel", &[("reason", "unused"), ("feedback", "")]).1, "/profile?saved=cancelled#subscription");
        assert!(seen.lock().unwrap().pop().unwrap().contains("cancel_at_period_end=true"));
        let page = app.get("/profile?saved=cancelled").2;
        assert!(page.contains(r#"profile-renewal-label">Ends on</dt>"#) && page.contains("17 March 2030") && !page.contains("profile-cancel-open") && page.contains(&format!("You keep {plus} until 17 March 2030")), "{page}");
        assert_eq!(app.get("/paid").0, 200);
    });
}

/// Skrongle mini (#636): a Free account reaches a paid feature, is
/// offered the server's AI for the tutor with its own keys usable too,
/// sees no plan anywhere on the profile, and `/billing/*` is gone.
#[test]
fn with_plans_off_a_free_account_has_every_feature() {
    run(|rt| {
        let meter = crate::metering::MeterConfig { plans: false, ..Default::default() };
        let state = crate::routes::test_state_with(true, None, Some(("sk-ant-server".into(), meter)));
        assert!(!billing::plans_on(&state));
        let mut app = Client::new(rt.block_on(serve(state.clone())));
        assert_eq!(app.get("/dev/login").0, 303);
        let uid: i64 = state.db.with(|c| c.query_row("SELECT id FROM users WHERE email = ?1", [crate::dev::DEV_EMAIL], |r| r.get(0))).unwrap();
        assert_eq!(state.db.with(|c| billing::plan_of(c, uid)).unwrap(), Plan::Free, "nothing bought or granted");

        assert_eq!(app.get("/paid"), (200, String::new(), "paid".into()));
        assert_eq!(app.get("/api/paid").0, 200);

        let offer = rt.block_on(crate::routes::hosted::hosted_offer(&state, uid, crate::metering::Need::Tutor)).unwrap();
        assert!(offer.as_ref().is_some_and(|s| s.unmetered && s.plan == Plan::Plus), "{offer:?}");
        assert!(offer.as_ref().and_then(crate::routes::hosted::left_line).is_none(), "no 'left this month'");
        assert!(crate::routes::hosted::own_keys_usable(&state, &CurrentUser::test(uid)));
        assert!(!state.meter.credits_offered());

        let (code, _, page) = app.get("/profile?plans=1");
        assert_eq!(code, 200);
        for gone in ["profile-subscription", "href=\"#subscription\"", "plans-dialog", "profile-ai-usage", "profile-own-keys-switch", "CA$15 a month"] {
            assert!(!page.contains(gone), "{gone} on the profile: {page}");
        }
        assert!(page.contains("profile-api-keys"));
        // A stale "upgrade" link (a paid feature's redirect) says nothing about a subscription either.
        let (code, _, page) = app.get("/profile?upgrade=1");
        assert_eq!(code, 200);
        assert!(!page.contains("comes with a subscription") && !page.contains("plans-dialog"), "{page}");

        for path in ["/billing/subscribe", "/billing/cancel", "/billing/checkout", "/billing/portal", "/billing/credits"] {
            assert_eq!(app.post_form(path, &[("plan", "plus")]).0, 404, "{path}");
        }
        assert_eq!(app.webhook("{}", "t=1,v1=00"), 404);
    });
}

/// Buying a credit pack: Checkout in payment mode, credits only from the
/// signed webhook, applied once, shown on the profile.
#[test]
fn buy_ai_credits_through_stripe() {
    run(|rt| {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stripe_url = rt.block_on(mock_stripe(seen.clone()));
        let meter = crate::metering::MeterConfig { packs: crate::metering::parse_packs("price_pack3:2.00:$3").unwrap(), ..Default::default() };
        let state = crate::routes::test_state_with(true, Some(config(stripe_url)), Some(("sk-ant-server".into(), meter)));
        let mut app = Client::new(rt.block_on(serve(state.clone())));
        app.get("/dev/login");
        let uid: i64 = state.db.with(|c| c.query_row("SELECT id FROM users WHERE email = ?1", [crate::dev::DEV_EMAIL], |r| r.get(0))).unwrap();
        let billing_ref = state.db.with(|c| billing::account(c, uid)).unwrap().unwrap().billing_ref;
        // Learners never see dollar amounts on the profile: packs are offered where Skrongle AI runs out, not here.
        let page = app.get("/profile").2;
        assert!(!page.contains("profile-ai-buy") && !page.contains("$2.00"), "{page}");
        // Packs are sold on every plan (#931): this learner is on Free.
        let r = app.agent.post(&format!("{}/billing/credits", app.base)).header("cookie", &app.cookie).send_form([("pack", "price_pack3")]).unwrap();
        assert_eq!(Client::result(r).1, "https://checkout.example/s1");
        let form = seen.lock().unwrap().pop().unwrap();
        assert!(form.contains("mode=payment") && form.contains("price%5D=price_pack3") && form.contains("%5Bcredit_micros%5D=2000000"), "{form}");
        let r = app.agent.post(&format!("{}/billing/credits", app.base)).header("cookie", &app.cookie).send_form([("pack", "price_other")]).unwrap();
        assert_eq!(r.status().as_u16(), 400);

        let body = serde_json::json!({
            "id": "evt_c1", "type": "checkout.session.completed", "created": crate::db::now(),
            "data": { "object": { "id": "cs_1", "mode": "payment", "payment_status": "paid", "payment_intent": "pi_1", "amount_total": 300,
                "metadata": { "kind": "ai-credits", "billing_ref": billing_ref, "credit_micros": "2000000", "pack": "price_pack3" } } }
        })
        .to_string();
        assert_eq!(app.webhook(&body, "t=1,v1=00"), 400);
        for _ in 0..2 {
            assert_eq!(app.webhook(&body, &billing::sign(SECRET, body.as_bytes(), crate::db::now())), 200);
        }
        assert_eq!(state.db.with(|c| crate::metering::credit_balance(c, uid)).unwrap(), 2_000_000, "applied once");
        let page = app.get("/profile?credits=done").2;
        assert!(page.contains("Your AI credits are added") && !page.contains("$2.00"), "{page}");
    });
}

/// A subscription event for the dev user's customer at `price`, with
/// metadata naming `plan` (which the app must not believe, #963).
fn subscription_event(billing_ref: &str, status: &str, price: &str, plan: &str, created: i64) -> String {
    serde_json::json!({
        "id": format!("evt_{created}"), "type": "customer.subscription.updated", "created": created,
        "data": { "object": { "id": "sub_1", "customer": "cus_mock", "status": status, "created": created,
            "metadata": { "billing_ref": billing_ref, "plan": plan }, "cancel_at_period_end": false,
            "items": { "data": [ { "price": { "id": price }, "current_period_end": 1_900_000_000 } ] } } }
    })
    .to_string()
}

/// #963: on the hosted shape nothing grants a paid plan but a verified
/// Stripe event, and the plan is the one sold at the price paid. The
/// pretend card payment (`/billing/subscribe`) is gone; the plans modal has
/// no card fields and posts to Checkout; a paying learner's Purchase goes to
/// the Billing Portal; cancelling asks Stripe and keeps the reason.
#[test]
fn only_a_verified_stripe_event_grants_a_plan() {
    run(|rt| {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stripe_url = rt.block_on(mock_stripe(seen.clone()));
        let state = crate::routes::test_state(true, Some(billing::StripeConfig { price_id_basic: Some("price_basic".into()), ..config(stripe_url) }));
        let mut app = Client::new(rt.block_on(serve(state.clone())));
        app.get("/dev/login");
        let uid: i64 = state.db.with(|c| c.query_row("SELECT id FROM users WHERE email = ?1", [crate::dev::DEV_EMAIL], |r| r.get(0))).unwrap();
        let billing_ref = state.db.with(|c| billing::account(c, uid)).unwrap().unwrap().billing_ref;
        let plan = |state: &Arc<AppState>| state.db.with(|c| billing::plan_of(c, uid)).unwrap();

        // The modal: the two plans, no card fields, Purchase goes to Checkout.
        let page = app.get("/profile").2;
        assert!(page.contains(r#"data-plan="basic""#) && page.contains(r#"data-plan="plus""#) && page.contains("CA$3 a month") && page.contains("CA$15 a month"), "{page}");
        assert!(page.contains("/billing/checkout\" class=\"plans-form\""), "{page}");
        for gone in ["card_number", "plans-card-fields", "plans-method", "/billing/subscribe"] {
            assert!(!page.contains(gone), "{gone} on the profile: {page}");
        }

        // The pretend payment is gone: posting a card buys nothing.
        let card = [("plan", "plus"), ("card_name", "Ana"), ("card_number", "4242 4242 4242 4242"), ("card_expiry", "12/30"), ("card_cvc", "123")];
        assert_eq!(app.post_form("/billing/subscribe", &card).0, 404);
        assert_eq!(app.get("/api/paid").0, 402);
        assert_eq!(plan(&state), Plan::Free);

        // Checkout sells each plan at its own price; Free and unknown plans are refused.
        assert_eq!(app.post_form("/billing/checkout", &[("plan", "basic")]).1, "https://checkout.example/s1");
        assert!(seen.lock().unwrap().pop().unwrap().contains("price%5D=price_basic"));
        assert_eq!(app.post_form("/billing/checkout", &[("plan", "free")]).0, 400);
        assert_eq!(app.post_form("/billing/checkout", &[("plan", "gold")]).0, 400);

        // An event with no or a forged signature changes nothing.
        let now = crate::db::now();
        let forged = subscription_event(&billing_ref, "active", "price_monthly", "plus", now);
        assert_eq!(app.webhook(&forged, "t=1,v1=00"), 400);
        assert_eq!(app.webhook(&forged, &billing::sign("whsec_someone_else", forged.as_bytes(), now)), 400);
        assert_eq!(app.webhook(&forged, ""), 400);
        assert_eq!(plan(&state), Plan::Free);

        // Signed, at a price that is not a plan's: still nothing, whatever the metadata says.
        let elsewhere = subscription_event(&billing_ref, "active", "price_someone_elses", "plus", now);
        assert_eq!(app.webhook(&elsewhere, &billing::sign(SECRET, elsewhere.as_bytes(), now)), 200);
        assert_eq!(plan(&state), Plan::Free);
        assert_eq!(app.get("/api/paid").0, 402);

        // Signed, paid at the Basic price, metadata saying Plus: Basic.
        let basic = subscription_event(&billing_ref, "active", "price_basic", "plus", now + 1);
        assert_eq!(app.webhook(&basic, &billing::sign(SECRET, basic.as_bytes(), now)), 200);
        assert_eq!(plan(&state), Plan::Basic);
        assert_eq!(app.get("/api/paid").0, 200);
        let page = app.get("/profile").2;
        assert!(page.contains(&format!(r#"data-testid="profile-plan">{} Basic"#, crate::APP_NAME)) && page.contains(r#"data-source="stripe""#), "{page}");

        // Paying already: Purchase (an upgrade) opens the Billing Portal, where plans change.
        assert_eq!(app.post_form("/billing/checkout", &[("plan", "plus")]).1, "https://portal.example/p1");
        assert!(seen.lock().unwrap().pop().unwrap().contains("customer=cus_mock"));

        // Cancel: Stripe is asked, the reason kept; a second Cancel finds nothing to end.
        assert_eq!(app.post_form("/billing/cancel", &[("reason", "nope")]).0, 400);
        assert_eq!(app.post_form("/billing/cancel", &[("reason", "missing"), ("feedback", "  No offline mode.  ")]).1, "/profile?saved=cancelled#subscription");
        assert!(seen.lock().unwrap().pop().unwrap().contains("cancel_at_period_end=true"));
        let (p, reason, feedback): (String, String, String) = state
            .db
            .with(|c| c.query_row("SELECT plan, reason, feedback FROM plan_cancellations WHERE user_id = ?1", [uid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))
            .unwrap();
        assert_eq!((p.as_str(), reason.as_str(), feedback.as_str()), ("basic", "missing", "No offline mode."));
        assert_eq!(app.post_form("/billing/cancel", &[]).1, "/profile#subscription");
        assert_eq!(state.db.with(|c| c.query_row("SELECT COUNT(*) FROM plan_cancellations", [], |r| r.get::<_, i64>(0))).unwrap(), 1);
        assert_eq!(plan(&state), Plan::Basic, "kept until Stripe ends it");
    });
}

/// #394: every Checkout of an account shares one Stripe customer, made
/// once, and no Checkout starts while a payment is still being confirmed.
#[test]
fn checkouts_share_one_customer_and_wait_for_an_incomplete_payment() {
    run(|rt| {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stripe_url = rt.block_on(mock_stripe(seen.clone()));
        let state = crate::routes::test_state(true, Some(config(stripe_url)));
        let mut app = Client::new(rt.block_on(serve(state.clone())));
        app.get("/dev/login");
        let uid: i64 = state.db.with(|c| c.query_row("SELECT id FROM users WHERE email = ?1", [crate::dev::DEV_EMAIL], |r| r.get(0))).unwrap();

        // Two tabs: one customer created, both sessions name it.
        assert_eq!(app.post_form("/billing/checkout", &[]).1, "https://checkout.example/s1");
        assert_eq!(app.post_form("/billing/checkout", &[]).1, "https://checkout.example/s1");
        let calls = seen.lock().unwrap().clone();
        assert_eq!(calls.iter().filter(|c| c.starts_with("customer ")).count(), 1, "{calls:?}");
        assert_eq!(calls.iter().filter(|c| c.contains("customer=cus_mock")).count(), 2, "{calls:?}");
        assert_eq!(state.db.with(|c| billing::account(c, uid)).unwrap().unwrap().stripe_customer.as_deref(), Some("cus_mock"));

        // One of them is still being paid: no third Checkout.
        let body = serde_json::json!({
            "id": "evt_1", "type": "customer.subscription.created", "created": crate::db::now(),
            "data": { "object": { "id": "sub_1", "customer": "cus_mock", "status": "incomplete", "created": crate::db::now(),
                "metadata": {}, "items": { "data": [ { "price": { "id": "price_monthly" } } ] } } }
        })
        .to_string();
        assert_eq!(app.webhook(&body, &billing::sign(SECRET, body.as_bytes(), crate::db::now())), 200);
        let before = seen.lock().unwrap().len();
        assert_eq!(app.post_form("/billing/checkout", &[]).0, 400);
        assert_eq!(seen.lock().unwrap().len(), before);
    });
}

#[test]
fn without_stripe_everyone_is_free_unless_granted() {
    run(|rt| {
        let state = crate::routes::test_state(true, None);
        let mut app = Client::new(rt.block_on(serve(state)));
        assert_eq!(app.webhook("{}", "t=1,v1=00"), 404);
        app.get("/dev/login");
        assert_eq!(app.post_form("/billing/checkout", &[]).0, 400);
        let page = app.get("/profile").2;
        assert!(page.contains(r#"data-testid="profile-plan">Free"#) && page.contains("Dev: grant Plus"), "{page}");
        assert_eq!(app.get("/api/paid").0, 402);
        app.get("/dev/login?premium=1");
        assert_eq!(app.get("/api/paid").0, 200);
        let page = app.get("/profile").2;
        assert!(page.contains(&format!(r#"data-testid="profile-plan">{} Plus"#, crate::APP_NAME)) && page.contains("given to this account, no end date"), "{page}");
        app.get("/dev/login?premium=0");
        assert_eq!(app.get("/api/paid").0, 402);
    });
}
