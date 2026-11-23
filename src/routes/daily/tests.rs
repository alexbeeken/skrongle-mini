//! The Plan tab's routes (#222), against a real router: the tab row, each
//! form, each refusal (a past day, past 28 days ahead, not the owner) and
//! the class clock's first visit.

use super::*;
use crate::daily::tests::{class, course_files};
use crate::students::tests::Server;

const COURSE: &str = "plan-1";

/// A teacher with the class of `daily::tests::class` (course `plan-1`, three
/// lessons, three students given it) and its cookie.
fn teacher(s: &Server) -> (crate::daily::tests::Class, String) {
    s.state
        .db
        .with(|c| {
            let k = class(c, "t@example.org", "kid", COURSE)?;
            let sid = crate::auth::create_session(c, k.teacher)?;
            Ok((k, format!("sid={sid}")))
        })
        .unwrap()
}

fn today(s: &Server) -> i64 {
    s.state.db.with(|c| daily::today(c, crate::db::now())).unwrap()
}

fn plan(s: &Server, day: i64) -> Option<daily::Plan> {
    s.state.db.with(|c| daily::plan(c, COURSE, day)).unwrap()
}

fn kinds(s: &Server, day: i64) -> Vec<Kind> {
    plan(s, day).map(|p| p.activities.iter().map(|a| a.kind).collect()).unwrap_or_default()
}

/// Every `data-testid="<id>"` element's text up to its first tag.
fn texts(page: &str, id: &str) -> Vec<String> {
    let marker = format!(r#"data-testid="{id}""#);
    page.split(&marker)
        .skip(1)
        .map(|rest| {
            let after = &rest[rest.find('>').map_or(0, |i| i + 1)..];
            after[..after.find('<').unwrap_or(after.len())].trim().to_string()
        })
        .collect()
}

const PAGE: &str = "/courses/plan-1/daily";

/// Posts one change for `day` and follows the redirect: (Location, page).
fn post(s: &Server, cookie: &str, day: i64, form: &[(&str, &str)]) -> (String, String) {
    let date = iso(day);
    let mut all: Vec<(&str, &str)> = vec![("day", date.as_str())];
    all.extend_from_slice(form);
    let (status, location, _, _) = s.post(PAGE, cookie, &all);
    assert_eq!(status, 303, "{form:?}");
    let (status, _, _, page) = s.get(&location, cookie);
    assert_eq!(status, 200, "{location}");
    (location, page)
}

#[test]
fn dates_go_both_ways() {
    for day in [0, 11_016, 20_413] {
        assert_eq!(parse_iso(&iso(day)), Some(day));
    }
    assert_eq!(iso(20_413), "2025-11-21");
    assert_eq!(parse_iso("2026-02-30"), None);
    assert_eq!(parse_iso("20413"), None);
    assert_eq!(zone_name(0), "UTC+00:00");
    assert_eq!(zone_name(11), "UTC+05:30");
    assert_eq!(zone_name(-7), "UTC-03:30");
}

/// The owner who may hold students has Editor | Plan on the course page and
/// on the Plan page; an owner who may not has neither the row nor the page;
/// another teacher and a student get a 404 on the page and its form.
#[test]
fn the_tabs_and_who_gets_the_page() {
    let s = Server::start();
    let (k, tc) = teacher(&s);
    let (status, _, _, editor) = s.get("/courses/plan-1", &tc);
    assert_eq!(status, 200);
    assert!(editor.contains(r#"data-testid="course-tabs" data-tab="editor""#), "{editor}");
    assert!(editor.contains(r#"/courses/plan-1/daily" data-testid="course-tab-plan""#));
    let (status, _, _, page) = s.get(PAGE, &tc);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="course-tabs" data-tab="plan""#));
    assert!(page.contains(r#"/daily" aria-current="page" data-testid="course-tab-plan""#), "{page}");
    assert_eq!(texts(&page, "course-header-title"), ["Daily"]);
    assert!(!page.contains("course-tab-class"), "no Class tab yet (#226)");

    let (plain, other, kid) = s
        .state
        .db
        .with(|c| {
            let plain = crate::auth::create_user(c, "plain@example.org", "h")?;
            crate::courses::create_course(c, plain, "own-1", "Own", &course_files("own-1", "Own"))?;
            let other = crate::perms::create_teacher(c, "other@example.org", "h")?;
            Ok((crate::auth::create_session(c, plain)?, crate::auth::create_session(c, other)?, crate::auth::create_session(c, k.kids[0])?))
        })
        .unwrap();
    let (plain, other, kid) = (format!("sid={plain}"), format!("sid={other}"), format!("sid={kid}"));
    let (status, _, _, own) = s.get("/courses/own-1", &plain);
    assert_eq!(status, 200);
    assert!(!own.contains("course-tabs"), "an owner who may not hold students sees no tabs");
    assert_eq!(s.get("/courses/own-1/daily", &plain).0, 404);
    assert_eq!(s.post("/courses/own-1/daily", &plain, &[("do", "add-study")]).0, 404);
    for cookie in [&other, &kid] {
        assert_eq!(s.get(PAGE, cookie).0, 404, "{cookie}");
        assert_eq!(s.post(PAGE, cookie, &[("do", "recommended")]).0, 404, "{cookie}");
    }
    assert!(plan(&s, today(&s)).is_none(), "nothing was planned");
}

/// The recommended day, Down and Up, + Review and Remove, Assign, a lesson
/// changed, a play's games and times through, Clear the day: each form
/// makes its one change and comes back to the page.
#[test]
fn each_form_makes_one_change() {
    let s = Server::start();
    let (_, tc) = teacher(&s);
    let day = today(&s);

    let page = s.get(PAGE, &tc).3;
    assert!(page.contains(r#"data-testid="daily-empty""#));
    assert!(page.contains(r#"data-testid="daily-recommended""#));
    assert!(page.contains(r#"data-testid="daily-assign" disabled"#) || page.contains(r#"disabled data-testid="daily-assign""#), "Assign is off for an empty day");
    let days = texts(&page, "daily-day-state");
    assert_eq!(days.len(), 7);
    assert!(days.iter().all(|d| d == "Nothing planned"), "{days:?}");
    assert_eq!(page.matches(r#"data-today="1""#).count(), 1, "today is outlined");

    let (location, page) = post(&s, &tc, day, &[("do", "recommended")]);
    assert!(location.ends_with(&format!("?day={}", iso(day))), "{location}");
    assert_eq!(kinds(&s, day), [Kind::Study, Kind::Review, Kind::Play]);
    assert_eq!(texts(&page, "daily-activity-kind"), ["Study", "Review", "Play"]);
    assert_eq!(texts(&page, "daily-activity-lessons"), ["Lesson a", "Lesson a", "Lesson a"]);
    assert_eq!(texts(&page, "daily-draft"), ["Not assigned yet"]);
    assert!(!page.contains(r#"data-testid="daily-recommended""#), "no recommended button on a day with activities");
    let up = page.split(r#"data-testid="daily-activity""#).nth(1).unwrap();
    assert!(up.contains(r#" disabled aria-label="Move activity 1 up""#), "Up is off on the first row");
    let last = page.split(r#"data-testid="daily-activity""#).nth(3).unwrap();
    assert!(last.contains(r#" disabled aria-label="Move activity 3 down""#), "Down is off on the last row");

    let study = plan(&s, day).unwrap().activities[0].id.to_string();
    post(&s, &tc, day, &[("do", "down"), ("activity", &study)]);
    assert_eq!(kinds(&s, day), [Kind::Review, Kind::Study, Kind::Play]);
    post(&s, &tc, day, &[("do", "up"), ("activity", &study)]);
    assert_eq!(kinds(&s, day), [Kind::Study, Kind::Review, Kind::Play]);
    let (location, _) = post(&s, &tc, day, &[("do", "up"), ("activity", &study)]);
    assert!(location.ends_with("&error=refused"), "the first row cannot go up: {location}");

    post(&s, &tc, day, &[("do", "add-review")]);
    assert_eq!(kinds(&s, day), [Kind::Study, Kind::Review, Kind::Play, Kind::Review]);
    let extra = plan(&s, day).unwrap().activities[3].id.to_string();
    post(&s, &tc, day, &[("do", "remove"), ("activity", &extra)]);
    assert_eq!(kinds(&s, day), [Kind::Study, Kind::Review, Kind::Play]);

    // Change lesson: the picker opens in place; a study takes exactly one.
    let page = s.get(&format!("{PAGE}?day={}&change={study}", iso(day)), &tc).3;
    assert!(page.contains(r#"data-testid="daily-picker""#));
    assert!(page.contains(r#"type="radio" name="lesson" value="u1-b""#), "a study picks one lesson");
    let (location, page) = post(&s, &tc, day, &[("do", "lessons"), ("activity", &study), ("lesson", "u1-a"), ("lesson", "u1-b")]);
    assert!(location.ends_with("&error=lessons"), "{location}");
    assert_eq!(texts(&page, "daily-error").len(), 1);
    post(&s, &tc, day, &[("do", "lessons"), ("activity", &study), ("lesson", "u1-b")]);
    assert_eq!(plan(&s, day).unwrap().activities[0].lessons, ["u1-b"]);

    // A play: its games and its times through in one form.
    let play = plan(&s, day).unwrap().activities[2].clone();
    assert_eq!(play.games, ["matching", "flashcards"]);
    let (_, page) = post(&s, &tc, day, &[("do", "play"), ("activity", &play.id.to_string()), ("game", "flashcards"), ("rounds", "2")]);
    let play = plan(&s, day).unwrap().activities[2].clone();
    assert_eq!((play.games.as_slice(), play.rounds), (["flashcards".to_string()].as_slice(), 2));
    assert_eq!(texts(&page, "daily-activity-play"), ["Flashcards · 2 times through"]);

    // Assign: the badge counts the class; the plan stays editable.
    let (_, page) = post(&s, &tc, day, &[("do", "assign")]);
    assert!(plan(&s, day).unwrap().assigned);
    assert_eq!(texts(&page, "daily-assigned"), ["Assigned to 3 students"]);
    assert!(page.contains(r#"data-testid="daily-add-study""#) && !page.contains(r#"data-testid="daily-assign""#));
    let today_cell = page.split(r#"data-today="1""#).nth(1).unwrap();
    assert!(today_cell.contains(">3 activities<"), "{today_cell}");

    post(&s, &tc, day, &[("do", "clear")]);
    assert!(plan(&s, day).is_none());
}

/// Plan my week names the days it filled; a day before today and one past
/// 28 days ahead refuse every change with one error line and show no button.
#[test]
fn plan_my_week_and_the_days_that_refuse() {
    let s = Server::start();
    let (_, tc) = teacher(&s);
    let day = today(&s);

    let (location, page) = post(&s, &tc, day, &[("do", "week")]);
    assert!(location.contains("&did=week"), "{location}");
    let filled: Vec<i64> = (day..day + daily::AUTO_DAYS).filter(|d| plan(&s, *d).is_some()).collect();
    assert!(!filled.is_empty() && filled.len() <= 3, "three lessons: at most three days {filled:?}");
    let notice = texts(&page, "daily-notice").join("");
    assert!(notice.contains("Planned and assigned:") && notice.contains(&short_day(filled[0])), "{notice}");

    for (d, why) in [(day - 1, "yesterday"), (day + daily::PLAN_AHEAD_DAYS + 1, "29 days ahead")] {
        for form in [vec![("do", "recommended")], vec![("do", "add-study")], vec![("do", "assign")], vec![("do", "clear")]] {
            let (location, page) = post(&s, &tc, d, &form);
            assert!(location.ends_with("&error=closed"), "{why} {form:?}: {location}");
            assert_eq!(texts(&page, "daily-error").len(), 1);
            assert!(plan(&s, d).is_none(), "{why}: nothing made");
        }
        let page = s.get(&format!("{PAGE}?day={}", iso(d)), &tc).3;
        for button in ["daily-bar", "daily-recommended", "daily-up", "daily-clear"] {
            assert!(!page.contains(&format!(r#"data-testid="{button}""#)), "{why}: no {button}");
        }
    }
    let page = s.get(&format!("{PAGE}?day={}", iso(day - 1)), &tc).3;
    assert!(page.contains(r#"data-testid="daily-past""#));
    let page = s.get(&format!("{PAGE}?day={}", iso(day + daily::PLAN_AHEAD_DAYS + 1)), &tc).3;
    assert!(page.contains(r#"data-testid="daily-ahead""#));
    assert!(page.contains(r#"class="daily-day ahead"#) || page.contains("ahead\" href"), "the day is greyed out");
}

/// The class clock: unset, the page asks for it; a browser that sends its
/// offset sets it on the first visit; the form changes it.
#[test]
fn the_class_time_zone() {
    let s = Server::start();
    let (_, tc) = teacher(&s);
    let page = s.get(PAGE, &tc).3;
    assert!(page.contains(r#"data-testid="daily-tz-ask""#));
    assert_eq!(s.state.db.with(|c| crate::settings::class_tz(c)).unwrap(), None);

    let page = s.get(PAGE, &format!("{tc}; tz=330")).3;
    assert_eq!(s.state.db.with(|c| crate::settings::class_tz(c)).unwrap(), Some(11));
    assert!(!page.contains(r#"data-testid="daily-tz-ask""#));
    assert!(texts(&page, "daily-notice").join("").contains("UTC+05:30"));
    s.get(PAGE, &format!("{tc}; tz=-300"));
    assert_eq!(s.state.db.with(|c| crate::settings::class_tz(c)).unwrap(), Some(11), "only the first visit sets it");

    let (location, page) = post(&s, &tc, today(&s), &[("do", "tz"), ("tz", "2")]);
    assert!(location.contains("&did=tz"));
    assert_eq!(s.state.db.with(|c| crate::settings::class_tz(c)).unwrap(), Some(2));
    assert!(page.contains(r#"data-zone="UTC+01:00""#));
    let (location, _) = post(&s, &tc, today(&s), &[("do", "tz"), ("tz", "99")]);
    assert!(location.ends_with("&error=tz"));
}

/// The student's page links each given course's plan.
#[test]
fn the_students_page_links_the_plan() {
    let s = Server::start();
    let (k, tc) = teacher(&s);
    let page = s.get(&format!("/students/{}", k.kids[0]), &tc).3;
    assert!(page.contains(r#"/courses/plan-1/daily" data-testid="students-course-plan""#), "{page}");
    let page = s.get(&format!("/students/{}", k.out), &tc).3;
    assert!(!page.contains("students-course-plan"), "not given: no link");
}
