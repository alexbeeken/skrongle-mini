use super::*;
use crate::activity::Kind;

fn course() -> ContentStore {
    let lesson = r#"{"id":"l0","slug":"l0","title":"L0","unit":"A1","content":"x","quiz":[{"question_id":"q0","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#;
    crate::content::tests::store(&[lesson]).unwrap()
}

#[test]
fn the_catalogue_has_six_to_eight_badges_with_art_and_titles() {
    let c = catalogue();
    assert!((6..=8).contains(&c.badges.len()), "{} badges", c.badges.len());
    let prices: Vec<i64> = c.badges.iter().map(|b| b.price).collect();
    assert!(prices.windows(2).all(|w| w[0] < w[1]), "prices rise: {prices:?}");
    assert!(prices[0] >= 50 && *prices.last().unwrap() <= 1500, "{prices:?}");
    for b in &c.badges {
        assert!(c.art(&b.id).is_some_and(|s| s.starts_with("<svg")), "plain art for {}", b.id);
        assert_ne!(b.title(), format!("motivator.skc.badge.{}", b.id), "title of {}", b.id);
    }
}

#[test]
fn buying_spends_the_balance_once_and_only_when_affordable() {
    let course = course();
    let cheapest = catalogue().badges[0].clone();
    let dearest = catalogue().badges.last().unwrap().clone();
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h").unwrap();
        // Not wearing SKC: refused, whatever the balance.
        crate::activity::record(c, uid, course.id(), Kind::Answer, cheapest.price + 3, 0)?;
        assert_eq!(buy(c, uid, &course, 0, &cheapest.id)?, Err(Refusal::NotWorn));
        super::super::set(c, uid, course.id(), "skc")?;
        let earned = crate::earnings::SKC_RATES.earned(&crate::earnings::total(c, uid, &course, 0)?);
        assert_eq!(super::super::skc::balance(c, uid, &course, 0)?, earned);
        assert!(earned >= cheapest.price && earned < dearest.price);

        assert_eq!(buy(c, uid, &course, 0, "no-such-badge")?, Err(Refusal::Unknown));
        assert_eq!(buy(c, uid, &course, 0, &dearest.id)?, Err(Refusal::Short(dearest.price - earned)));
        assert_eq!(buy(c, uid, &course, 0, &cheapest.id)?, Ok(earned - cheapest.price));
        // Balance = earned − spent; a second buy is refused and charges nothing.
        assert_eq!(super::super::skc::balance(c, uid, &course, 0)?, earned - cheapest.price);
        assert_eq!(buy(c, uid, &course, 0, &cheapest.id)?, Err(Refusal::Owned));
        assert_eq!(spent(c, uid, course.id())?, cheapest.price);
        assert_eq!(owned(c, uid, course.id())?, vec![cheapest.id.clone()]);
        // Switching motivators keeps it; another course has its own shelf.
        super::super::set(c, uid, course.id(), "stats")?;
        super::super::set(c, uid, course.id(), "skc")?;
        assert_eq!(owned(c, uid, course.id())?, vec![cheapest.id.clone()]);
        assert!(owned(c, uid, "other-course")?.is_empty());
        Ok(())
    })
    .unwrap();
}
