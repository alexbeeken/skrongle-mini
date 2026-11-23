//! Today's work (#223): the list in each state, which stop opens, a Study
//! stop finished through its form, the one-button Review and Play stops,
//! earlier days, the subjects page's line and the closed Study pages.

use crate::daily::{self, tests::class_with_plan, Kind, StopState};
use crate::routes::student_frame::tests::{assert_light, assert_light_with_pictures};
use crate::students::tests::Server;

const COURSE: &str = "daily-1";

/// A teacher's class with today's recommended day assigned (a study, a
/// review and a play of `Lesson a`), and the first student signed in with
/// the subject open: (server, the student's cookie, the student's id, the
/// teacher's cookie, the plan's activity ids).
fn day_one() -> (Server, String, i64, String, Vec<i64>) {
    let s = Server::start();
    let (kid, teacher, ids) = s
        .state
        .db
        .with(|c| {
            let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, crate::db::now())?;
            let kid = crate::auth::create_session(c, k.kids[0])?;
            let teacher = crate::auth::create_session(c, k.teacher)?;
            Ok(((kid, k.kids[0]), teacher, p.activities.iter().map(|a| a.id).collect::<Vec<_>>()))
        })
        .unwrap();
    let cookie = format!("sid={}", kid.0);
    let (status, location, _, _) = s.get(&format!("/courses/open/{COURSE}"), &cookie);
    assert_eq!((status, location.as_str()), (303, "/"), "the subject opens");
    (s, cookie, kid.1, format!("sid={teacher}"), ids)
}

/// The student does the stop to the end straight in the database (`daily::tests::do_stop`).
fn walk(s: &Server, kid: i64, day: i64, activity: i64) {
    s.state.db.with(|c| {
        daily::tests::do_stop(c, kid, COURSE, day, activity, crate::db::now());
        Ok(())
    })
    .unwrap();
}

fn today(s: &Server) -> i64 {
    s.state.db.with(|c| daily::today(c, crate::db::now())).unwrap()
}

/// The values of every `name="{field}" value="…"` in the page, in order.
fn values(page: &str, field: &str) -> Vec<String> {
    let key = format!(r#"name="{field}" value=""#);
    page.match_indices(&key).map(|(i, _)| page[i + key.len()..].split('"').next().unwrap().to_string()).collect()
}

/// The right answer to each question of `course_files`'s lessons, by the
/// question's number (`u1-a-02` is 2).
fn right_answer(qid: &str) -> &'static str {
    match &qid[qid.len() - 2..] {
        "00" => "meaning",
        "01" => "example",
        "02" => "2",
        _ => "true",
    }
}

fn texts(page: &str, testid: &str) -> Vec<String> {
    let key = format!(r#"data-testid="{testid}""#);
    page.match_indices(&key)
        .map(|(i, _)| {
            let rest = &page[i..];
            let body = &rest[rest.find('>').unwrap() + 1..];
            body[..body.find("</").unwrap()].trim().to_string()
        })
        .collect()
}

/// The list: one row per activity with its number, kind, lessons and one
/// word; one line; one button whose words follow the day; after a stop,
/// a line that says it is done; at the end, no button.
#[test]
fn the_list_in_each_state() {
    let (s, kid, kid_id, _, ids) = day_one();
    let (status, _, _, page) = s.get("/", &kid);
    assert_eq!(status, 200);
    assert_light("/ (today)", &page);
    assert_eq!(texts(&page, "student-bar-title"), vec!["Daily"], "the subject's title in the top bar");
    assert_eq!(texts(&page, "student-today-heading"), vec!["Today&#39;s work"]);
    assert_eq!(texts(&page, "student-today-kind"), vec!["Study", "Review", "Play"]);
    assert_eq!(texts(&page, "student-today-lessons"), vec!["Lesson a"; 3]);
    assert_eq!(texts(&page, "student-today-word"), vec!["Next", "After Study", "After Review"]);
    assert!(page.contains(r#"class="next current" data-testid="student-today-row""#), "the next row is highlighted");
    let line = texts(&page, "student-today-line").remove(0);
    assert!(line.contains("0 of 3 done") && line.contains(&super::date_of(today(&s))), "{line}");
    assert!(!line.contains("XP") && !page.contains("Cash"));
    assert_eq!(texts(&page, "student-today-go"), vec!["Get started"]);
    assert!(page.contains(&format!(r#"href="/today/stop/{}" data-testid="student-today-go""#, ids[0])));
    assert_eq!(page.matches("<a ").count() - page.matches(r#"data-testid="student-my-subjects""#).count() - page.matches("student-change-password").count(), 1, "no row is a link: {page}");
    assert!(!page.contains("student-today-earlier"), "no earlier day");

    // Opened: Keep going.
    assert_eq!(s.get(&format!("/today/stop/{}", ids[0]), &kid).0, 200);
    let (_, _, _, page) = s.get("/", &kid);
    assert_eq!(texts(&page, "student-today-go"), vec!["Keep going"]);

    // Done: the line says so, the next is Review.
    let day = today(&s);
    walk(&s, kid_id, day, ids[0]);
    let (_, _, _, page) = s.get(&format!("/?done={}", ids[0]), &kid);
    assert_eq!(texts(&page, "student-today-done"), vec!["Done: Study, Lesson a."]);
    assert_eq!(texts(&page, "student-today-word"), vec!["Done", "Next", "After Review"]);
    assert_eq!(texts(&page, "student-today-go"), vec!["Go to the next activity"]);
    assert!(texts(&page, "student-today-line")[0].contains("1 of 3 done"));
    let (_, _, _, page) = s.get(&format!("/?done={}", ids[1]), &kid);
    assert!(!page.contains("student-today-done"), "only a stop that is done");

    // All done: no button.
    for id in &ids[1..] {
        walk(&s, kid_id, day, *id);
    }
    let (_, _, _, page) = s.get("/", &kid);
    assert!(page.contains("student-today-all-done") && !page.contains("student-today-go"), "{page}");
    assert!(texts(&page, "student-today-line")[0].contains("3 of 3 done"));
}

/// Nothing assigned today: the page says so and has no main button; a
/// draft is nothing to a student.
#[test]
fn nothing_assigned_has_no_button() {
    let s = Server::start();
    let (kid, course) = s
        .state
        .db
        .with(|c| {
            let k = daily::tests::class(c, "t@example.org", "kid-", COURSE)?;
            let day = daily::today(c, crate::db::now())?;
            assert!(daily::recommended(c, &k.course, COURSE, day, crate::db::now())?, "a draft");
            Ok((crate::auth::create_session(c, k.kids[0])?, k.course_id))
        })
        .unwrap();
    let kid = format!("sid={kid}");
    assert_eq!(s.get(&format!("/courses/open/{course}"), &kid).0, 303);
    let (status, _, _, page) = s.get("/", &kid);
    assert_eq!(status, 200);
    assert_light("/ (nothing)", &page);
    assert!(page.contains("student-today-nothing") && !page.contains("student-today-go") && !page.contains("student-today-row"), "{page}");
}

/// A stop opens only when it is in an assigned plan of the active subject,
/// for today or earlier, and is the next one or started; anything else
/// goes back to the list. A normal account has no stop pages.
#[test]
fn only_the_next_stop_opens() {
    let (s, kid, kid_id, teacher, ids) = day_one();
    for (id, why) in [(ids[1], "locked"), (ids[2], "locked"), (999_999, "no such activity")] {
        let (status, location, _, _) = s.get(&format!("/today/stop/{id}"), &kid);
        assert_eq!((status, location.as_str()), (303, "/"), "{why}");
        assert_eq!(s.post(&format!("/today/stop/{id}"), &kid, &[]).1, "/", "{why}, posted");
    }
    let started = |id: i64| -> bool {
        s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM daily_done WHERE activity_id = ?1 AND user_id = ?2", [id, kid_id], |r| r.get::<_, i64>(0))).unwrap() > 0
    };
    assert!(!started(ids[1]), "a locked stop is not started");
    // A plan of tomorrow, assigned: not yet.
    let tomorrow = s
        .state
        .db
        .with(|c| {
            let course = crate::content::ContentStore::from_files(&daily::tests::course_files(COURSE, "Daily")).unwrap();
            let day = daily::today(c, crate::db::now())? + 1;
            assert!(daily::recommended(c, &course, COURSE, day, crate::db::now())? && daily::assign(c, COURSE, day, crate::db::now())?);
            Ok(daily::plan(c, COURSE, day)?.unwrap().activities[0].id)
        })
        .unwrap();
    assert_eq!(s.get(&format!("/today/stop/{tomorrow}"), &kid).1, "/", "tomorrow's stop");
    // Done: back to the list.
    let day = today(&s);
    walk(&s, kid_id, day, ids[0]);
    assert_eq!(s.get(&format!("/today/stop/{}", ids[0]), &kid).1, "/", "a stop that is done");
    // Another subject's stop: not while this one is open.
    let theirs = s
        .state
        .db
        .with(|c| {
            let (_, p) = class_with_plan(c, "o@example.org", "oth-", "daily-2", crate::db::now())?;
            Ok(p.activities[0].id)
        })
        .unwrap();
    assert_eq!(s.get(&format!("/today/stop/{theirs}"), &kid).1, "/", "another course's stop");
    // The teacher: no stop pages.
    for path in [format!("/today/stop/{}", ids[1]), "/today/earlier".into(), format!("/today/day/{}", day - 1)] {
        assert!(matches!(s.get(&path, &teacher).0, 303 | 404), "teacher {path}");
        assert_ne!(s.get(&path, &teacher).0, 200, "teacher {path}");
    }
}

/// A Study stop with JavaScript off: the lesson's text and two questions
/// as one form. A miss explains and asks two fresh ones; both right marks
/// the lesson seen, finishes the stop and goes back to the list, which
/// says so.
#[test]
fn a_study_stop_finishes_through_its_form() {
    let (s, kid, kid_id, _, ids) = day_one();
    let stop = format!("/today/stop/{}", ids[0]);
    let (status, _, _, page) = s.get(&stop, &kid);
    assert_eq!(status, 200);
    assert_light_with_pictures(&stop, &page);
    assert!(page.contains(r#"data-testid="student-study-text""#) && page.contains("Explain the idea in a short paragraph"), "the lesson's text");
    assert!(page.contains(r#"data-testid="student-study-words""#), "its words");
    assert_eq!(texts(&page, "student-study-title"), vec!["Lesson a"]);
    assert!(page.contains(&format!(r#"<form method="post" action="{stop}" data-testid="student-study-form">"#)), "one plain form");
    let asked = [values(&page, "q1"), values(&page, "q2")].concat();
    assert_eq!(asked.len(), 2, "{page}");
    assert_ne!(asked[0], asked[1]);
    assert!(asked.iter().all(|q| q.starts_with("u1-a-")));

    // One wrong: the page explains and asks two fresh ones.
    let (status, _, _, page) = s.post(&stop, &kid, &[("q1", &asked[0]), ("a1", "wrong answer"), ("q2", &asked[1]), ("a2", right_answer(&asked[1]))]);
    assert_eq!(status, 200);
    assert_light_with_pictures("the stop after a miss", &page);
    assert!(page.contains(r#"data-testid="student-study-missed""#), "{page}");
    assert_eq!(page.matches(r#"data-testid="student-study-verdict""#).count(), 2);
    assert!(page.contains(r#"data-right="false""#) && page.contains(r#"data-right="true""#));
    let fresh = [values(&page, "q1"), values(&page, "q2")].concat();
    assert_eq!(fresh.len(), 2);
    assert!(fresh.iter().all(|q| !asked.contains(q)), "two fresh questions: {fresh:?} after {asked:?}");
    let day = today(&s);
    let stop_state = || s.state.db.with(|c| daily::stops(c, kid_id, COURSE, day)).unwrap()[0].state;
    assert_eq!(stop_state(), StopState::Next, "not finished");
    let history: i64 = s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM quiz_history WHERE user_id = ?1", [kid_id], |r| r.get(0))).unwrap();
    assert_eq!(history, 2, "both answers are recorded, as the reading check does");

    // A posted question of another lesson, or one twice: the form again.
    for form in [[("q1", "u1-b-00"), ("a1", "meaning"), ("q2", fresh[1].as_str()), ("a2", right_answer(&fresh[1]))], [("q1", &fresh[0]), ("a1", right_answer(&fresh[0])), ("q2", &fresh[0]), ("a2", right_answer(&fresh[0]))]] {
        let (status, location, _, _) = s.post(&stop, &kid, &form);
        assert_eq!((status, location.as_str()), (303, stop.as_str()));
    }
    assert_eq!(stop_state(), StopState::Next);

    // Both right: seen, finished, back to the list that says so.
    let (status, location, _, _) = s.post(&stop, &kid, &[("q1", &fresh[0]), ("a1", right_answer(&fresh[0])), ("q2", &fresh[1]), ("a2", right_answer(&fresh[1]))]);
    assert_eq!((status, location.as_str()), (303, format!("/?done={}", ids[0]).as_str()));
    assert_eq!(stop_state(), StopState::Done);
    let seen = s.state.db.with(|c| crate::progress::load_one(c, kid_id, COURSE, "u1-a")).unwrap();
    assert!(seen.is_some_and(|p| p.seen()), "the lesson is seen, as the reading check marks it");
    let (_, _, _, page) = s.get(&location, &kid);
    assert_eq!(texts(&page, "student-today-done"), vec!["Done: Study, Lesson a."]);
    assert_eq!(texts(&page, "student-today-word"), vec!["Done", "Next", "After Review"]);
}

/// Until #224: a Review stop and a Play stop are one button each that
/// finishes them, so a day can be walked to the end.
#[test]
fn review_and_play_stops_finish_with_one_button() {
    let (s, kid, kid_id, _, ids) = day_one();
    let day = today(&s);
    walk(&s, kid_id, day, ids[0]);
    for (id, kind) in [(ids[1], "review"), (ids[2], "play")] {
        let stop = format!("/today/stop/{id}");
        let (status, _, _, page) = s.get(&stop, &kid);
        assert_eq!(status, 200);
        assert_light(&stop, &page);
        assert!(page.contains(&format!(r#"data-kind="{kind}""#)) && page.contains("student-stop-finish"), "{page}");
        let (status, location, _, _) = s.post(&stop, &kid, &[]);
        assert_eq!((status, location.as_str()), (303, format!("/?done={id}").as_str()), "{kind}");
    }
    let stops = s.state.db.with(|c| daily::stops(c, kid_id, COURSE, day)).unwrap();
    assert!(stops.iter().all(|s| s.state == StopState::Done));
}

/// A play that asks for rounds and is not the last activity counts one
/// time through per click and says how many are left.
#[test]
fn a_play_with_rounds_counts_each_click() {
    let (s, kid, kid_id, _, ids) = day_one();
    let day = today(&s);
    let course = crate::content::ContentStore::from_files(&daily::tests::course_files(COURSE, "Daily")).unwrap();
    s.state
        .db
        .with(|c| {
            let now = crate::db::now();
            assert!(daily::set_rounds(c, COURSE, day, ids[2], 2, now)?);
            assert!(daily::add(c, &course, COURSE, day, Kind::Study, None, now)?, "the play is no longer last");
            daily::tests::do_stop(c, kid_id, COURSE, day, ids[0], now);
            daily::tests::do_stop(c, kid_id, COURSE, day, ids[1], now);
            Ok(())
        })
        .unwrap();
    let stop = format!("/today/stop/{}", ids[2]);
    let (_, _, _, page) = s.get(&stop, &kid);
    assert_eq!(texts(&page, "student-stop-left"), vec!["2 more times to go"]);
    assert_eq!(s.post(&stop, &kid, &[]).1, stop, "one time through, one to go");
    let (_, _, _, page) = s.get(&stop, &kid);
    assert_eq!(texts(&page, "student-stop-left"), vec!["1 more time to go"]);
    assert_eq!(s.post(&stop, &kid, &[]).1, format!("/?done={}", ids[2]));
}

/// Earlier days: a link with what is left, the list of them, each day's
/// list, and a stop of an earlier day goes back to that day.
#[test]
fn earlier_days_are_finished_from_their_own_list() {
    let (s, kid, kid_id, _, _) = day_one();
    let today = today(&s);
    let yesterday = today - 1;
    let ids: Vec<i64> = s
        .state
        .db
        .with(|c| {
            let course = crate::content::ContentStore::from_files(&daily::tests::course_files(COURSE, "Daily")).unwrap();
            let then = daily::tests::at(yesterday);
            assert!(daily::recommended(c, &course, COURSE, yesterday, then)? && daily::assign(c, COURSE, yesterday, then)?);
            Ok(daily::plan(c, COURSE, yesterday)?.unwrap().activities.iter().map(|a| a.id).collect())
        })
        .unwrap();
    let (_, _, _, page) = s.get("/", &kid);
    assert_eq!(texts(&page, "student-today-earlier"), vec!["Earlier days · 3 left"]);
    let (status, _, _, page) = s.get("/today/earlier", &kid);
    assert_eq!(status, 200);
    assert_light("/today/earlier", &page);
    assert_eq!(texts(&page, "student-earlier-left"), vec!["3 left"]);
    assert!(page.contains(&format!(r#"href="/today/day/{yesterday}""#)));

    let day_page = format!("/today/day/{yesterday}");
    let (status, _, _, page) = s.get(&day_page, &kid);
    assert_eq!(status, 200);
    assert_light(&day_page, &page);
    assert_eq!(texts(&page, "student-today-heading"), vec![format!("Work for {}", super::date_of(yesterday))]);
    assert_eq!(texts(&page, "student-today-go"), vec!["Get started"]);
    assert!(page.contains(&format!(r#"href="/today/stop/{}""#, ids[0])));

    // Its Study stop, through the form: back to that day.
    let stop = format!("/today/stop/{}", ids[0]);
    let (_, _, _, page) = s.get(&stop, &kid);
    assert!(page.contains(&format!(r#"href="{day_page}" data-testid="student-stop-back""#)), "back goes to that day");
    let asked = [values(&page, "q1"), values(&page, "q2")].concat();
    let (status, location, _, _) = s.post(&stop, &kid, &[("q1", &asked[0]), ("a1", right_answer(&asked[0])), ("q2", &asked[1]), ("a2", right_answer(&asked[1]))]);
    assert_eq!((status, location.as_str()), (303, format!("{day_page}?done={}", ids[0]).as_str()));
    let (_, _, _, page) = s.get(&location, &kid);
    assert_eq!(texts(&page, "student-today-done"), vec!["Done: Study, Lesson a."]);
    let (_, _, _, page) = s.get("/", &kid);
    assert_eq!(texts(&page, "student-today-earlier"), vec!["Earlier days · 2 left"]);
    let left = s.state.db.with(|c| daily::left_on(c, kid_id, COURSE, yesterday)).unwrap();
    assert_eq!(left, Some(2));

    // Today, a later day and a day with nothing: the list.
    for day in [today, today + 1, today - 5] {
        assert_eq!(s.get(&format!("/today/day/{day}"), &kid).1, "/", "day {day}");
    }
}

/// My subjects says how much of today is left in each subject.
#[test]
fn the_subjects_page_says_what_is_left_today() {
    let (s, kid, kid_id, _, ids) = day_one();
    let (_, _, _, page) = s.get("/courses?pick=1", &kid);
    assert_light("/courses?pick=1 (with today)", &page);
    assert_eq!(texts(&page, "student-subject-today"), vec!["3 things left today"]);
    let day = today(&s);
    for id in &ids {
        walk(&s, kid_id, day, *id);
    }
    let (_, _, _, page) = s.get("/courses?pick=1", &kid);
    assert_eq!(texts(&page, "student-subject-today"), vec!["Done for today"]);
}

/// A student reads a lesson only through a Study stop: Study mode's own
/// pages answer 404; Flashcards, Play and Sprint stay open.
#[test]
fn study_pages_are_closed_to_a_student() {
    let (s, kid, _, _, _) = day_one();
    for path in ["/lessons", "/lessons/lesson-a", "/practice", "/practice?lesson=lesson-a", "/map", "/history", "/mistakes", "/listening", "/guide", "/guide/lesson-a", "/vocabulary", "/lookup?q=x"] {
        assert_eq!(s.get(path, &kid).0, 404, "{path}");
    }
    assert_eq!(s.post("/lessons/lesson-a/read", &kid, &[]).0, 404);
    assert_eq!(s.post_json("/api/lesson-check", &kid, r#"{"lesson": "lesson-a", "answers": []}"#).0, 404);
    for path in ["/vocabulary/study", "/play", "/sprint"] {
        assert_ne!(s.get(path, &kid).0, 404, "{path}");
    }
}

/// A lesson with no question a plain form can ask: **I've read it** marks
/// it seen and finishes the stop.
#[test]
fn a_lesson_without_plain_questions_is_read_with_one_button() {
    let s = Server::start();
    let (kid, kid_id, id) = s
        .state
        .db
        .with(|c| {
            let teacher = crate::perms::create_teacher(c, "t@example.org", "h")?;
            let mut files = daily::tests::course_files(COURSE, "Daily");
            for f in files.iter_mut().filter(|f| f.path == "lessons/lesson-a.json") {
                let mut l: serde_json::Value = serde_json::from_str(&f.content).unwrap();
                l["quiz"] = serde_json::json!([]);
                f.content = l.to_string();
            }
            crate::courses::create_course(c, teacher, COURSE, "Daily", &files)?;
            let course = crate::content::ContentStore::from_files(&files).expect("the course loads");
            let new = crate::students::NewStudent { username: "kid".into(), password_hash: "h".into(), ..Default::default() };
            let kid = crate::students::create(c, teacher, &[new], false)?.unwrap()[0];
            assert!(crate::students::give(c, teacher, kid, COURSE)?);
            let (now, day) = (crate::db::now(), daily::today(c, crate::db::now())?);
            assert!(daily::recommended(c, &course, COURSE, day, now)? && daily::assign(c, COURSE, day, now)?);
            Ok((crate::auth::create_session(c, kid)?, kid, daily::plan(c, COURSE, day)?.unwrap().activities[0].id))
        })
        .unwrap();
    let kid = format!("sid={kid}");
    assert_eq!(s.get(&format!("/courses/open/{COURSE}"), &kid).0, 303);
    let stop = format!("/today/stop/{id}");
    let (status, _, _, page) = s.get(&stop, &kid);
    assert_eq!(status, 200);
    assert_light_with_pictures(&stop, &page);
    assert!(page.contains("student-study-read") && !page.contains("student-study-question"), "{page}");
    let (status, location, _, _) = s.post(&stop, &kid, &[("read", "1")]);
    assert_eq!((status, location.as_str()), (303, format!("/?done={id}").as_str()));
    let seen = s.state.db.with(|c| crate::progress::load_one(c, kid_id, COURSE, "u1-a")).unwrap();
    assert!(seen.is_some_and(|p| p.seen()));
}
