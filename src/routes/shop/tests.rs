use crate::motivators::shop::catalogue;

/// The handler refuses a second buy and one the balance cannot pay,
/// charges the price once, and shows what was bought on the hero.
#[test]
fn the_shop_sells_each_badge_once_and_only_when_affordable() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::super::test_state(false, None);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let app = super::super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", &cookie).send_form(form.iter().copied()).unwrap();
        (r.status().as_u16(), r.headers().get("location").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default())
    };
    let balance = || {
        let page = get("/").1;
        let at = page.find(r#"data-testid="skc-balance" data-skc=""#).expect("the SKC hero") + r#"data-testid="skc-balance" data-skc=""#.len();
        page[at..].split('"').next().unwrap().parse::<i64>().unwrap()
    };
    let cheap = catalogue().badges[0].clone();
    let dear = catalogue().badges.last().unwrap().clone();

    assert_eq!(post("/courses/open/spanish", &[]).0, 303);
    // Not wearing SKC: no shop.
    assert_eq!(post("/courses/spanish/settings", &[("motivator", "stats")]).0, 303);
    assert_eq!(get("/shop").0, 303);
    assert_eq!(post("/api/shop/buy", &[("item", cheap.id.as_str())]).0, 400);
    assert_eq!(post("/courses/spanish/settings", &[("motivator", "skc")]).0, 303);
    let earned = cheap.price + 30;
    state.db.with(|c| crate::activity::record(c, ann, "spanish", crate::activity::Kind::Answer, earned - 10, 0)).unwrap();
    let before = balance();
    assert!(before >= cheap.price && before < dear.price, "{before}");
    let (status, shop) = get("/shop");
    assert_eq!(status, 200);
    assert!(shop.contains(&format!(r#"data-testid="shop-badge" data-badge="{}""#, dear.id)) && !shop.contains("shop-refusal"));

    // Too dear: refused, nothing charged.
    let (s, to) = post("/api/shop/buy", &[("item", dear.id.as_str())]);
    assert_eq!((s, to.as_str()), (303, format!("/shop?refused=short&short={}&item={}", dear.price - before, dear.id).as_str()));
    // Drawn open, so it shows without script too (#926).
    assert!(get(&to).1.contains(r#"data-testid="shop-refusal" open data-open-on-load>"#));
    assert_eq!(balance(), before);
    // Bought: the balance drops by the price, the badge is on the hero.
    assert_eq!(post("/api/shop/buy", &[("item", cheap.id.as_str())]), (303, "/shop".to_string()));
    assert_eq!(balance(), before - cheap.price);
    assert!(get("/").1.contains(&format!(r#"data-testid="skc-badge" data-badge="{}" data-variant="plain""#, cheap.id)));
    // Twice: refused, nothing charged.
    assert_eq!(post("/api/shop/buy", &[("item", cheap.id.as_str())]).1, format!("/shop?refused=owned&item={}", cheap.id));
    assert_eq!(balance(), before - cheap.price);
    // Not a badge.
    assert_eq!(post("/api/shop/buy", &[("item", "nope")]).1, "/shop?refused=unknown&item=");
    // The plain art is served; a name that is not a badge is not.
    assert_eq!(get(&format!("/badges/{}.svg", cheap.id)).0, 200);
    assert_eq!(get("/badges/nope.svg").0, 404);
}
