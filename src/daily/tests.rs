//! Daily work: the teacher's plan (positions, lessons, games, rounds, the
//! 28 days ahead), the student's way through it (in order, the work each
//! kind asks for, the play rounds rule, earlier days), the class clock, the
//! teacher's views and the row bounds.
//!
//! The helpers marked `pub(crate)` are for the tests of the pages built on
//! this module (#222 to #225): [`class`], [`class_with_plan`],
//! [`course_files`], [`at`] and [`do_stop`].

use rusqlite::Connection;
use serde_json::json;

use super::*;
use crate::content::CourseFile;
use crate::db::Db;
use crate::students::NewStudent;

const COURSE: &str = "daily-1";
/// A Friday (20 000 days after 1 January 1970: 4 October 2024).
const DAY: i64 = 20_000;
/// `DAY + 3`, a Monday.
const MONDAY: i64 = DAY + 3;
/// Noon of `DAY` on a class clock that is not set (UTC).
const NOW: i64 = DAY * 86_400 + 12 * 3600;

/// Noon (UTC) of `day`: a `now` whose class-clock day is `day` while the
/// clock is not set.
pub(crate) fn at(day: i64) -> i64 {
    day * 86_400 + 12 * 3600
}

/// A course `course_id` titled `title` with three lessons: `u1-a` and
/// `u1-b` with three words (Word Match and Flashcards), `u1-c` with one
/// (Flashcards only).
pub(crate) fn course_files(course_id: &str, title: &str) -> Vec<CourseFile> {
    let mut files = crate::courses::starter_files(course_id, title);
    let lesson: serde_json::Value = serde_json::from_str(&files.pop().unwrap().content).unwrap();
    for (n, id) in ["a", "b", "c"].into_iter().enumerate() {
        let mut l = lesson.clone();
        l["id"] = json!(format!("u1-{id}"));
        l["slug"] = json!(format!("lesson-{id}"));
        l["title"] = json!(format!("Lesson {id}"));
        l["order"] = json!(n + 1);
        for (i, q) in l["quiz"].as_array_mut().unwrap().iter_mut().enumerate() {
            q["question_id"] = json!(format!("u1-{id}-{i:02}"));
        }
        if id == "c" {
            l["vocabulary"] = json!([{ "term": "meaning", "meaning": "what the term stands for" }]);
        }
        files.push(CourseFile { path: format!("lessons/lesson-{id}.json"), content: serde_json::to_string_pretty(&l).unwrap() });
    }
    files
}

/// A teacher, its course and its class.
pub(crate) struct Class {
    pub teacher: i64,
    /// The students given the course: Cy Moss, Ana Lee and `zed` (no name),
    /// in that order.
    pub kids: Vec<i64>,
    /// A student of the same teacher not given the course.
    pub out: i64,
    pub course_id: String,
    pub course: ContentStore,
}

/// A teacher (`email`) with the course `course_id` ([`course_files`]) given
/// to three students (usernames `<prefix>c`, `<prefix>a`, `<prefix>zed`),
/// and one student (`<prefix>out`) not given it.
pub(crate) fn class(c: &mut Connection, email: &str, prefix: &str, course_id: &str) -> rusqlite::Result<Class> {
    let teacher = crate::perms::create_teacher(c, email, "h")?;
    let files = course_files(course_id, "Daily");
    crate::courses::create_course(c, teacher, course_id, "Daily", &files)?;
    let course = ContentStore::from_files(&files).expect("the course loads");
    let new = |user: &str, first: &str, last: &str| NewStudent {
        username: format!("{prefix}{user}"),
        password_hash: "h".into(),
        first_name: first.into(),
        last_name: last.into(),
    };
    let rows = [new("c", "Cy", "Moss"), new("a", "Ana", "Lee"), new("zed", "", ""), new("out", "Out", "Side")];
    let mut ids = crate::students::create(c, teacher, &rows, false)?.expect("the teacher holds students");
    let out = ids.pop().unwrap();
    for kid in &ids {
        assert!(crate::students::give(c, teacher, *kid, course_id)?);
    }
    Ok(Class { teacher, kids: ids, out, course_id: course_id.to_string(), course })
}

/// [`class`] with the recommended day (a study, a review and a play of
/// `u1-a`) planned and assigned for the class-clock day of `now`: the class
/// and that plan.
pub(crate) fn class_with_plan(c: &mut Connection, email: &str, prefix: &str, course_id: &str, now: i64) -> rusqlite::Result<(Class, Plan)> {
    let k = class(c, email, prefix, course_id)?;
    let day = today(c, now)?;
    assert!(recommended(c, &k.course, &k.course_id, day, now)?);
    assert!(assign(c, &k.course_id, day, now)?);
    let p = plan(c, &k.course_id, day)?.unwrap();
    Ok((k, p))
}

/// The student does their next stop `activity` of `day` to the end, as the
/// pages will: start, the kind's work (lesson read, round finished, the
/// rounds played) and finish. Asserts each step.
pub(crate) fn do_stop(c: &Connection, user: i64, course_id: &str, day: i64, activity: i64, now: i64) {
    assert!(start(c, user, course_id, day, activity, now).unwrap(), "start {activity}");
    let p = plan_of_activity(c, activity).unwrap().unwrap();
    let a = p.activities.iter().find(|a| a.id == activity).unwrap();
    match a.kind {
        Kind::Study => assert!(mark_studied(c, user, activity).unwrap()),
        Kind::Review => assert!(mark_reviewed(c, user, activity).unwrap()),
        Kind::Play => {
            for _ in 0..a.rounds {
                add_play(c, user, activity).unwrap();
            }
        }
    }
    assert!(finish(c, user, course_id, day, activity, now).unwrap(), "finish {activity}");
}

fn k(c: &mut Connection) -> Class {
    class(c, "t@example.org", "kid-", COURSE).unwrap()
}

fn kinds(p: &Plan) -> Vec<Kind> {
    p.activities.iter().map(|a| a.kind).collect()
}

fn positions(p: &Plan) -> Vec<i64> {
    p.activities.iter().map(|a| a.position).collect()
}

fn ids(p: &Plan) -> Vec<i64> {
    p.activities.iter().map(|a| a.id).collect()
}

fn states(c: &Connection, user: i64, day: i64) -> Vec<StopState> {
    stops(c, user, COURSE, day).unwrap().into_iter().map(|s| s.state).collect()
}

fn s(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|i| i.to_string()).collect()
}

/// Adds an activity at the end of the day's plan over these lessons; its id.
fn add_over(c: &Connection, k: &Class, day: i64, kind: Kind, lessons: &[&str]) -> i64 {
    assert!(add(c, &k.course, COURSE, day, kind, None, at(day)).unwrap());
    let id = plan(c, COURSE, day).unwrap().unwrap().activities.last().unwrap().id;
    assert!(set_lessons(c, &k.course, COURSE, day, id, &s(lessons), at(day)).unwrap());
    id
}

fn done_count(c: &Connection, activity: i64) -> i64 {
    c.query_row("SELECT COUNT(*) FROM daily_done WHERE activity_id = ?1", [activity], |r| r.get(0)).unwrap()
}

#[test]
fn kinds_round_trip() {
    for k in [Kind::Study, Kind::Review, Kind::Play] {
        assert_eq!(Kind::parse(k.as_str()), Some(k));
    }
    assert_eq!(Kind::parse("quiz"), None);
}

#[test]
fn add_move_remove_keep_positions_in_order() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        assert_eq!(plan(c, COURSE, DAY)?, None);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Study, None, NOW)?);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Review, None, NOW)?);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Play, Some(0), NOW)?);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Review, Some(99), NOW)?, "past the end is the end");
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert!(!p.assigned, "a new plan is a draft");
        assert_eq!(kinds(&p), vec![Kind::Play, Kind::Study, Kind::Review, Kind::Review]);
        assert_eq!(positions(&p), vec![0, 1, 2, 3]);
        // The study took u1-a; the activities added after it get the next free lesson.
        assert_eq!(p.activities[1].lessons, s(&["u1-a"]));
        assert_eq!(p.activities[0].lessons, s(&["u1-b"]));
        assert_eq!(p.activities[0].games, s(&["matching", "flashcards"]));
        assert_eq!(p.activities[0].rounds, 0);
        assert!(p.activities[1].games.is_empty(), "only a play has games");
        let before = ids(&p);

        assert!(move_to(c, COURSE, DAY, before[0], 2, NOW)?);
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert_eq!(ids(&p), vec![before[1], before[2], before[0], before[3]]);
        assert_eq!(positions(&p), vec![0, 1, 2, 3]);
        assert!(move_to(c, COURSE, DAY, before[3], 0, NOW)?);
        assert!(move_to(c, COURSE, DAY, before[1], 50, NOW)?);
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert_eq!(ids(&p), vec![before[3], before[2], before[0], before[1]]);
        assert_eq!(positions(&p), vec![0, 1, 2, 3]);

        assert!(remove(c, COURSE, DAY, before[2], NOW)?);
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert_eq!(ids(&p), vec![before[3], before[0], before[1]]);
        assert_eq!(positions(&p), vec![0, 1, 2]);

        // Another day's or another course's activity is refused.
        assert!(!move_to(c, COURSE, DAY + 1, before[0], 0, NOW)?);
        assert!(!remove(c, "other", DAY, before[0], NOW)?);
        assert!(!remove(c, COURSE, DAY, before[2], NOW)?, "already gone");

        // At most MAX_ACTIVITIES.
        while plan(c, COURSE, DAY)?.unwrap().activities.len() < MAX_ACTIVITIES {
            assert!(add(c, &k.course, COURSE, DAY, Kind::Review, None, NOW)?);
        }
        assert!(!add(c, &k.course, COURSE, DAY, Kind::Review, None, NOW)?);
        assert!(!recommended(c, &k.course, COURSE, DAY, NOW)?, "not an empty day");
        assert_eq!(plan(c, COURSE, DAY)?.unwrap().activities.len(), MAX_ACTIVITIES);

        assert!(clear(c, COURSE, DAY, NOW)?);
        assert_eq!(plan(c, COURSE, DAY)?, None);
        assert!(!clear(c, COURSE, DAY, NOW)?);
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_activity_id_is_never_reused() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Study, None, NOW)?);
        let first = plan(c, COURSE, DAY)?.unwrap().activities[0].id;
        assert!(remove(c, COURSE, DAY, first, NOW)?);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Study, None, NOW)?);
        assert!(plan(c, COURSE, DAY)?.unwrap().activities[0].id > first, "a stop's id never meets a later activity");
        Ok(())
    })
    .unwrap();
}

#[test]
fn next_lesson_skips_lessons_studied_on_or_before_the_day() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let yesterday = at(DAY - 1);
        assert_eq!(next_lesson(c, &k.course, COURSE, DAY)?.as_deref(), Some("u1-a"));
        add(c, &k.course, COURSE, DAY, Kind::Study, None, yesterday)?;
        assert_eq!(next_lesson(c, &k.course, COURSE, DAY)?.as_deref(), Some("u1-b"));
        assert_eq!(next_lesson(c, &k.course, COURSE, DAY - 1)?.as_deref(), Some("u1-a"), "a later day's study does not count");
        add(c, &k.course, COURSE, DAY - 1, Kind::Study, None, yesterday)?; // u1-a again: it was free on that day
        add(c, &k.course, COURSE, DAY, Kind::Study, None, yesterday)?;
        add(c, &k.course, COURSE, DAY, Kind::Study, None, yesterday)?;
        assert_eq!(fresh_lesson(c, &k.course, COURSE, DAY)?, None);
        assert_eq!(next_lesson(c, &k.course, COURSE, DAY)?.as_deref(), Some("u1-a"), "every lesson taken: the first");
        // A review or a play takes no lesson away.
        assert!(add(c, &k.course, COURSE, DAY + 1, Kind::Review, None, yesterday)?);
        assert_eq!(plan(c, COURSE, DAY + 1)?.unwrap().activities[0].lessons, s(&["u1-a"]));
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_course_with_no_lessons_gets_no_activity() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        k(c);
        let mut files = crate::courses::starter_files(COURSE, "Daily");
        files.retain(|f| !f.path.starts_with("lessons/"));
        let empty = ContentStore::from_files(&files).unwrap();
        assert!(empty.lessons().is_empty());
        assert_eq!(next_lesson(c, &empty, COURSE, DAY)?, None);
        assert!(!add(c, &empty, COURSE, DAY, Kind::Study, None, NOW)?);
        assert!(!recommended(c, &empty, COURSE, DAY, NOW)?);
        assert!(plan(c, COURSE, DAY)?.is_none(), "no empty plan left behind");
        Ok(())
    })
    .unwrap();
}

#[test]
fn lessons_follow_the_kind_and_the_course() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        for kind in [Kind::Study, Kind::Review, Kind::Play] {
            add(c, &k.course, COURSE, DAY, kind, None, NOW)?;
        }
        let p = plan(c, COURSE, DAY)?.unwrap();
        let (study, review, play) = (p.activities[0].id, p.activities[1].id, p.activities[2].id);
        let set = |c: &Connection, id: i64, l: &[&str]| set_lessons(c, &k.course, COURSE, DAY, id, &s(l), NOW).unwrap();

        assert!(!set(c, study, &["u1-a", "u1-b"]), "a study takes one lesson");
        assert!(!set(c, study, &[]), "and not none");
        assert!(set(c, study, &["u1-c"]));
        assert!(!set(c, review, &["u1-a", "nope"]), "an unknown lesson");
        assert!(!set(c, review, &[]));
        assert!(set(c, review, &["u1-c", "u1-a", "u1-c"]));
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert_eq!(p.activities[0].lessons, s(&["u1-c"]));
        assert_eq!(p.activities[1].lessons, s(&["u1-a", "u1-c"]), "no duplicates, the course's order");
        assert!(!set_lessons(c, &k.course, COURSE, DAY + 1, review, &s(&["u1-a"]), NOW)?, "another day");

        // Games: Word Match and Flashcards only, for a play, only those its lessons allow.
        assert_eq!(GAMES, ["matching", "flashcards"]);
        assert_eq!(games_for(&k.course, &s(&["u1-c"])), vec!["flashcards"]);
        assert_eq!(games_for(&k.course, &s(&["u1-a", "u1-c"])), vec!["matching", "flashcards"]);
        assert!(games_for(&k.course, &[]).is_empty());
        let game = |c: &Connection, id: i64, g: &str, on: bool| set_game(c, &k.course, COURSE, DAY, id, g, on, NOW).unwrap();
        assert!(!game(c, review, "flashcards", false));
        assert!(!game(c, play, "sprint", true), "no other game");
        assert!(game(c, play, "matching", false));
        assert_eq!(plan(c, COURSE, DAY)?.unwrap().activities[2].games, s(&["flashcards"]));
        assert!(game(c, play, "matching", true));
        assert_eq!(plan(c, COURSE, DAY)?.unwrap().activities[2].games, s(&["matching", "flashcards"]), "the stored order");
        assert!(set(c, play, &["u1-c"]));
        assert_eq!(plan(c, COURSE, DAY)?.unwrap().activities[2].games, s(&["flashcards"]), "Word Match no longer fits");
        assert!(!game(c, play, "matching", true));

        // Rounds: only for a play, 0 to MAX_ROUNDS.
        assert!(!set_rounds(c, COURSE, DAY, review, 2, NOW)?);
        assert!(!set_rounds(c, COURSE, DAY, play, MAX_ROUNDS + 1, NOW)?);
        assert!(!set_rounds(c, COURSE, DAY, play, -1, NOW)?);
        assert!(set_rounds(c, COURSE, DAY, play, MAX_ROUNDS, NOW)?);
        assert_eq!(plan(c, COURSE, DAY)?.unwrap().activities[2].rounds, MAX_ROUNDS);
        assert!(set_rounds(c, COURSE, DAY, play, 0, NOW)?, "0: no limit");
        Ok(())
    })
    .unwrap();
}

#[test]
fn recommended_fills_only_an_empty_day() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        add(c, &k.course, COURSE, DAY - 1, Kind::Study, None, at(DAY - 1))?; // u1-a
        assert!(recommended(c, &k.course, COURSE, DAY, NOW)?);
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert!(!p.assigned);
        assert_eq!(kinds(&p), vec![Kind::Study, Kind::Review, Kind::Play]);
        assert_eq!(positions(&p), vec![0, 1, 2]);
        assert!(p.activities.iter().all(|a| a.lessons == s(&["u1-b"])), "{p:?}");
        assert_eq!(p.activities[2].games, s(&["matching", "flashcards"]));
        assert!(!recommended(c, &k.course, COURSE, DAY, NOW)?);
        assert_eq!(plan(c, COURSE, DAY)?.unwrap().activities.len(), 3);
        Ok(())
    })
    .unwrap();
}

#[test]
fn students_see_an_assigned_plan_in_order() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        assert!(!assign(c, COURSE, DAY, NOW)?, "no plan");
        recommended(c, &k.course, COURSE, DAY, NOW)?;
        let p = plan(c, COURSE, DAY)?.unwrap();
        let (first, second) = (p.activities[0].id, p.activities[1].id);
        assert!(stops(c, kid, COURSE, DAY)?.is_empty(), "a draft shows nothing");
        assert_eq!(next_stop(c, kid, COURSE, DAY)?, None);
        assert!(!start(c, kid, COURSE, DAY, first, NOW)?);

        assert!(assign(c, COURSE, DAY, NOW)?);
        assert!(plan(c, COURSE, DAY)?.unwrap().assigned);
        assert_eq!(states(c, kid, DAY), vec![StopState::Next, StopState::Locked, StopState::Locked]);
        assert_eq!(next_stop(c, kid, COURSE, DAY)?.map(|s| s.activity.id), Some(first));
        assert!(!start(c, kid, COURSE, DAY, second, NOW)?, "locked");
        assert!(!finish(c, kid, COURSE, DAY, first, NOW)?, "not started");
        assert!(start(c, kid, COURSE, DAY, first, NOW)?);
        assert!(start(c, kid, COURSE, DAY, first, NOW + 5)?, "again: the first row stays");
        let started: i64 = c.query_row("SELECT started_at FROM daily_done WHERE activity_id = ?1", [first], |r| r.get(0))?;
        assert_eq!(started, NOW);
        let st = stops(c, kid, COURSE, DAY)?;
        assert!(st[0].started && !st[1].started);
        assert!(mark_studied(c, kid, first)?);
        assert!(!finish(c, kid, COURSE, DAY, second, NOW)?, "locked");
        assert!(finish(c, kid, COURSE, DAY, first, NOW)?);
        assert_eq!(states(c, kid, DAY), vec![StopState::Done, StopState::Next, StopState::Locked]);
        assert_eq!(next_stop(c, kid, COURSE, DAY)?.map(|s| s.activity.id), Some(second));
        assert!(!start(c, kid, COURSE, DAY, first, NOW)?, "done is not next");
        assert!(!finish(c, kid, COURSE, DAY, first, NOW)?, "done stays done");
        assert_eq!(states(c, k.kids[1], DAY), vec![StopState::Next, StopState::Locked, StopState::Locked], "progress is per student");

        // Clearing the day takes it away from the students.
        assert!(clear(c, COURSE, DAY, NOW)?);
        assert!(stops(c, kid, COURSE, DAY)?.is_empty());
        assert_eq!(done_count(c, first), 0, "with everyone's progress");
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_study_and_a_review_finish_only_once_their_work_is_recorded() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let kid = k.kids[0];
        let (study, review) = (p.activities[0].id, p.activities[1].id);
        assert!(!mark_studied(c, kid, study)?, "not started: nothing to mark");
        assert!(start(c, kid, COURSE, DAY, study, NOW)?);
        assert!(!finish(c, kid, COURSE, DAY, study, NOW)?, "the lesson is not read yet");
        assert!(!mark_reviewed(c, kid, study)?, "a study is not a review");
        assert!(!stops(c, kid, COURSE, DAY)?[0].marked);
        assert!(mark_studied(c, kid, study)?);
        assert!(stops(c, kid, COURSE, DAY)?[0].marked);
        assert!(finish(c, kid, COURSE, DAY, study, NOW)?);
        assert!(!mark_studied(c, kid, study)?, "a finished row does not change");

        assert!(start(c, kid, COURSE, DAY, review, NOW)?);
        assert!(!mark_studied(c, kid, review)?, "a review is not a study");
        assert!(!finish(c, kid, COURSE, DAY, review, NOW)?, "the round is not finished yet");
        assert!(mark_reviewed(c, kid, review)?);
        assert!(finish(c, kid, COURSE, DAY, review, NOW)?);
        assert_eq!(states(c, kid, DAY), vec![StopState::Done, StopState::Done, StopState::Next]);
        Ok(())
    })
    .unwrap();
}

/// A plan of study, play (`rounds`), review, assigned; the first student
/// finished the study. The play's id.
fn play_in_the_middle(c: &Connection, k: &Class, rounds: i64) -> i64 {
    recommended(c, &k.course, COURSE, DAY, NOW).unwrap();
    let p = plan(c, COURSE, DAY).unwrap().unwrap();
    let (study, review, play) = (p.activities[0].id, p.activities[1].id, p.activities[2].id);
    assert!(move_to(c, COURSE, DAY, review, 2, NOW).unwrap());
    assert!(set_rounds(c, COURSE, DAY, play, rounds, NOW).unwrap());
    assert!(assign(c, COURSE, DAY, NOW).unwrap());
    do_stop(c, k.kids[0], COURSE, DAY, study, NOW);
    play
}

#[test]
fn a_limited_play_needs_its_rounds() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        let play = play_in_the_middle(c, &k, 3);
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert_eq!(p.activities[1].id, play);
        assert!(play_limited(&p, &p.activities[1]));
        assert_eq!(add_play(c, kid, play)?, 0, "not started: nothing counted");
        assert!(start(c, kid, COURSE, DAY, play, NOW)?);
        for plays in 0..3 {
            assert!(!finish(c, kid, COURSE, DAY, play, NOW)?, "refused at {plays} plays");
            assert_eq!(add_play(c, kid, play)?, plays + 1);
        }
        assert_eq!(add_play(c, kid, play)?, 3, "never above the rounds");
        assert_eq!(stops(c, kid, COURSE, DAY)?[1].plays, 3);
        assert!(!stops(c, kid, COURSE, DAY)?[1].marked, "a play is never marked");
        assert!(finish(c, kid, COURSE, DAY, play, NOW)?);
        assert_eq!(add_play(c, kid, play)?, 3, "a finished play counts no more");
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_last_play_one_without_rounds_or_one_without_games_finishes_at_once() {
    // The play last: rounds do not hold the student back, and plays keep counting.
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let kid = k.kids[0];
        let (study, review, play) = (p.activities[0].id, p.activities[1].id, p.activities[2].id);
        assert!(set_rounds(c, COURSE, DAY, play, 3, NOW)?);
        for a in [study, review] {
            do_stop(c, kid, COURSE, DAY, a, NOW);
        }
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert!(!play_limited(&p, &p.activities[2]));
        assert!(start(c, kid, COURSE, DAY, play, NOW)?);
        for n in 1..=5 {
            assert_eq!(add_play(c, kid, play)?, n);
        }
        let other = k.kids[1];
        for a in [study, review] {
            do_stop(c, other, COURSE, DAY, a, NOW);
        }
        assert!(start(c, other, COURSE, DAY, play, NOW)?);
        assert!(finish(c, other, COURSE, DAY, play, NOW)?, "allowed at 0 plays");
        Ok(())
    })
    .unwrap();
    // A play with no rounds in the middle.
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        let play = play_in_the_middle(c, &k, 0);
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert!(!play_limited(&p, &p.activities[1]));
        assert!(start(c, kid, COURSE, DAY, play, NOW)?);
        assert!(finish(c, kid, COURSE, DAY, play, NOW)?, "allowed at 0 plays");
        Ok(())
    })
    .unwrap();
    // A play with rounds in the middle but every game switched off.
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        let play = play_in_the_middle(c, &k, 3);
        for g in GAMES {
            assert!(set_game(c, &k.course, COURSE, DAY, play, g, false, NOW)?);
        }
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert!(p.activities[1].games.is_empty());
        assert!(!play_limited(&p, &p.activities[1]), "no game to play the rounds with");
        assert!(start(c, kid, COURSE, DAY, play, NOW)?);
        assert!(finish(c, kid, COURSE, DAY, play, NOW)?);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_play_count_is_never_lowered() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let kid = k.kids[0];
        let (study, review, play) = (p.activities[0].id, p.activities[1].id, p.activities[2].id);
        // The play is last, so its rounds do not limit it: the student plays it 5 times.
        set_rounds(c, COURSE, DAY, play, 3, NOW)?;
        for a in [study, review] {
            do_stop(c, kid, COURSE, DAY, a, NOW);
        }
        assert!(start(c, kid, COURSE, DAY, play, NOW)?);
        for n in 1..=5 {
            assert_eq!(add_play(c, kid, play)?, n);
        }
        // The teacher adds an activity after it: now it is limited to 3, and 5 stays 5.
        assert!(add(c, &k.course, COURSE, DAY, Kind::Review, None, NOW)?);
        let p = plan(c, COURSE, DAY)?.unwrap();
        assert!(play_limited(&p, &p.activities[2]));
        assert_eq!(add_play(c, kid, play)?, 5);
        assert_eq!(stops(c, kid, COURSE, DAY)?[2].plays, 5);
        assert!(finish(c, kid, COURSE, DAY, play, NOW)?, "past its rounds");
        Ok(())
    })
    .unwrap();
}

#[test]
fn changing_an_assigned_plan_keeps_progress_per_activity() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let kid = k.kids[0];
        let (study, review, play) = (p.activities[0].id, p.activities[1].id, p.activities[2].id);
        for a in [study, review] {
            do_stop(c, kid, COURSE, DAY, a, NOW);
        }
        // The finished study moves to the end: it stays done, and the play is next.
        assert!(move_to(c, COURSE, DAY, study, 2, NOW)?);
        let st = stops(c, kid, COURSE, DAY)?;
        let by_id = |id: i64| st.iter().find(|s| s.activity.id == id).unwrap().state;
        assert_eq!(by_id(review), StopState::Done);
        assert_eq!(by_id(play), StopState::Next);
        assert_eq!(by_id(study), StopState::Done, "progress is kept by activity, wherever it moves");
        // New lessons keep it too.
        assert!(set_lessons(c, &k.course, COURSE, DAY, review, &s(&["u1-a", "u1-b"]), NOW)?);
        assert_eq!(by_id(review), StopState::Done);
        // Removing the review removes its progress only.
        assert!(remove(c, COURSE, DAY, review, NOW)?);
        let left: i64 = c.query_row("SELECT COUNT(*) FROM daily_done WHERE user_id = ?1", [kid], |r| r.get(0))?;
        assert_eq!(left, 1, "the study's row stays");
        assert_eq!(done_count(c, review), 0);
        assert_eq!(states(c, kid, DAY), vec![StopState::Next, StopState::Done]);
        do_stop(c, kid, COURSE, DAY, play, NOW);
        assert_eq!(states(c, kid, DAY), vec![StopState::Done, StopState::Done], "the moved study is still done");
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_lesson_comes_out_of_todays_and_later_plans_only() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        // A past day, assigned, its study of u1-b finished.
        let past_study = add_over(c, &k, DAY - 1, Kind::Study, &["u1-b"]);
        add_over(c, &k, DAY - 1, Kind::Review, &["u1-b"]);
        assert!(assign(c, COURSE, DAY - 1, at(DAY - 1))?);
        do_stop(c, kid, COURSE, DAY - 1, past_study, at(DAY - 1));
        // Today, assigned: a study of u1-b, a review of u1-a and u1-b, a play of u1-b, a study of u1-a.
        let study = add_over(c, &k, DAY, Kind::Study, &["u1-b"]);
        let review = add_over(c, &k, DAY, Kind::Review, &["u1-a", "u1-b"]);
        add_over(c, &k, DAY, Kind::Play, &["u1-b"]);
        let other = add_over(c, &k, DAY, Kind::Study, &["u1-a"]);
        assign(c, COURSE, DAY, NOW)?;
        for a in [study, review] {
            do_stop(c, kid, COURSE, DAY, a, NOW);
        }
        // A future draft: a review of u1-b and a study of u1-a.
        add_over(c, &k, DAY + 3, Kind::Review, &["u1-b"]);
        let later = add_over(c, &k, DAY + 3, Kind::Study, &["u1-a"]);

        assert_eq!(
            lesson_days(c, COURSE, "u1-b", NOW)?,
            vec![
                LessonDay { day: DAY, assigned: true, activities: vec![(Kind::Study, 1), (Kind::Review, 2), (Kind::Play, 1)] },
                LessonDay { day: DAY + 3, assigned: false, activities: vec![(Kind::Review, 1)] },
            ],
            "the past day is not listed"
        );

        let past = plan(c, COURSE, DAY - 1)?;
        assert_eq!(remove_lesson(c, COURSE, "u1-b", NOW)?, 2);
        assert_eq!(plan(c, COURSE, DAY - 1)?, past, "the past day is untouched");
        assert_eq!(done_count(c, past_study), 1, "and so is its progress");

        let p = plan(c, COURSE, DAY)?.unwrap();
        assert!(p.assigned);
        assert_eq!(ids(&p), vec![review, other], "the study and the play of u1-b alone are gone");
        assert_eq!(positions(&p), vec![0, 1]);
        assert_eq!(p.activities[0].lessons, s(&["u1-a"]), "the review keeps its other lesson");
        assert_eq!(done_count(c, study), 0, "the removed study's progress goes with it");
        assert_eq!(done_count(c, review), 1, "the review keeps its progress");
        assert_eq!(states(c, kid, DAY), vec![StopState::Done, StopState::Next]);

        let p = plan(c, COURSE, DAY + 3)?.unwrap();
        assert!(!p.assigned);
        assert_eq!(ids(&p), vec![later]);
        assert_eq!(positions(&p), vec![0]);
        assert_eq!(lesson_days(c, COURSE, "u1-b", NOW)?, vec![]);
        // A lesson in no plan changes nothing.
        let before = (plan(c, COURSE, DAY)?, plan(c, COURSE, DAY + 3)?);
        assert_eq!(remove_lesson(c, COURSE, "u1-c", NOW)?, 0);
        assert_eq!((plan(c, COURSE, DAY)?, plan(c, COURSE, DAY + 3)?), before);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_play_that_loses_a_lesson_loses_the_games_it_no_longer_allows() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        // A play of u1-a (3 words) and u1-c (1 word): Word Match and Flashcards.
        let play = add_over(c, &k, DAY, Kind::Play, &["u1-a", "u1-c"]);
        let other = add_over(c, &k, DAY, Kind::Play, &["u1-b", "u1-c"]);
        assert!(set_game(c, &k.course, COURSE, DAY, other, "flashcards", false, NOW)?);
        let games = |c: &Connection, id: i64| plan(c, COURSE, DAY).unwrap().unwrap().activities.into_iter().find(|a| a.id == id).unwrap().games;
        assert_eq!(games(c, play), s(&["matching", "flashcards"]));
        assert_eq!(remove_lesson(c, COURSE, "u1-a", NOW)?, 1);
        assert_eq!(games(c, play), s(&["flashcards"]), "u1-c alone has too few words for Word Match");
        assert_eq!(games(c, other), s(&["matching"]), "a play that keeps its lessons keeps its games");
        assert_eq!(remove_lesson(c, COURSE, "u1-c", NOW)?, 1);
        assert!(plan(c, COURSE, DAY)?.unwrap().activities.iter().all(|a| a.id != play), "its last lesson gone, the play goes");
        assert_eq!(games(c, other), s(&["matching"]), "u1-b still allows Word Match");
        Ok(())
    })
    .unwrap();
}

#[test]
fn plans_go_28_days_ahead_and_a_past_day_is_never_changed() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        assert_eq!(PLAN_AHEAD_DAYS, 28);
        assert!(plannable(DAY, DAY) && plannable(DAY + 28, DAY));
        assert!(!plannable(DAY + 29, DAY) && !plannable(DAY - 1, DAY));
        assert!(add(c, &k.course, COURSE, DAY + 28, Kind::Study, None, NOW)?, "the last day");
        assert!(!add(c, &k.course, COURSE, DAY + 29, Kind::Study, None, NOW)?, "a day too far");
        assert!(!recommended(c, &k.course, COURSE, DAY + 29, NOW)?);
        assert!(plan(c, COURSE, DAY + 29)?.is_none());
        // Yesterday's plan, made then, is closed to every change today.
        let then = at(DAY - 1);
        assert!(recommended(c, &k.course, COURSE, DAY - 1, then)?);
        let a = plan(c, COURSE, DAY - 1)?.unwrap().activities;
        let before = plan(c, COURSE, DAY - 1)?;
        assert!(!add(c, &k.course, COURSE, DAY - 1, Kind::Study, None, NOW)?);
        assert!(!move_to(c, COURSE, DAY - 1, a[0].id, 2, NOW)?);
        assert!(!set_lessons(c, &k.course, COURSE, DAY - 1, a[1].id, &s(&["u1-c"]), NOW)?);
        assert!(!set_game(c, &k.course, COURSE, DAY - 1, a[2].id, "matching", false, NOW)?);
        assert!(!set_rounds(c, COURSE, DAY - 1, a[2].id, 2, NOW)?);
        assert!(!assign(c, COURSE, DAY - 1, NOW)?);
        assert!(!remove(c, COURSE, DAY - 1, a[0].id, NOW)?);
        assert!(!clear(c, COURSE, DAY - 1, NOW)?);
        assert_eq!(plan(c, COURSE, DAY - 1)?, before);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_day_turns_on_the_class_clock() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        // 23:00 UTC on DAY: still DAY on an unset clock (UTC), already DAY + 1 two hours east.
        let late = DAY * 86_400 + 23 * 3600;
        assert_eq!(today(c, late)?, DAY);
        assert!(crate::settings::set_class_tz(c, 4)?);
        assert_eq!(today(c, late)?, DAY + 1);
        assert_eq!(today(c, DAY * 86_400 + 22 * 3600 - 1)?, DAY, "one second before midnight on the clock");
        assert!(crate::settings::set_class_tz(c, -24)?);
        assert_eq!(today(c, DAY * 86_400 + 11 * 3600)?, DAY - 1, "twelve hours behind: still yesterday");
        assert!(crate::settings::set_class_tz(c, 4)?);

        // A plan for DAY + 1, assigned the evening before: a student starts it
        // once the class clock reaches it, not before.
        let evening = DAY * 86_400 + 20 * 3600; // 22:00 on the clock
        assert!(recommended(c, &k.course, COURSE, DAY + 1, evening)?);
        assert!(assign(c, COURSE, DAY + 1, evening)?);
        let first = plan(c, COURSE, DAY + 1)?.unwrap().activities[0].id;
        assert!(!start(c, kid, COURSE, DAY + 1, first, evening)?, "tomorrow's work waits for tomorrow");
        assert!(start(c, kid, COURSE, DAY + 1, first, late)?, "00:00 on the clock");
        assert!(mark_studied(c, kid, first)?);
        assert!(finish(c, kid, COURSE, DAY + 1, first, late)?);
        // At 23:00 UTC the teacher's DAY is over: its plans are closed.
        assert!(!add(c, &k.course, COURSE, DAY, Kind::Study, None, late)?);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Study, None, evening)?);
        // The student's own browser clock plays no part: nothing here reads it.
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_browser_offset_becomes_half_hours() {
    assert_eq!(half_hours_of(0), Some(0));
    assert_eq!(half_hours_of(3600), Some(2));
    assert_eq!(half_hours_of(19_800), Some(11), "India, +5:30");
    assert_eq!(half_hours_of(20_700), Some(12), "Nepal, +5:45: the nearest half hour");
    assert_eq!(half_hours_of(-12_600), Some(-7), "Newfoundland, -3:30");
    assert_eq!(half_hours_of(-12 * 3600), Some(-24));
    assert_eq!(half_hours_of(14 * 3600), Some(28));
    assert_eq!(half_hours_of(15 * 3600), None);
    assert_eq!(half_hours_of(-13 * 3600), None);
}

#[test]
fn only_the_class_sees_the_plan() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let first = p.activities[0].id;
        // A student of the teacher not given the course, and the teacher itself.
        for who in [k.out, k.teacher] {
            assert!(stops(c, who, COURSE, DAY)?.is_empty());
            assert!(!start(c, who, COURSE, DAY, first, NOW)?);
            assert_eq!(left_on(c, who, COURSE, DAY)?, None);
        }
        // Taking the course away hides it and keeps the progress.
        let kid = k.kids[0];
        do_stop(c, kid, COURSE, DAY, first, NOW);
        assert!(crate::students::take_away(c, k.teacher, kid, COURSE)?);
        assert!(stops(c, kid, COURSE, DAY)?.is_empty());
        assert_eq!(done_count(c, first), 1);
        assert!(crate::students::give(c, k.teacher, kid, COURSE)?);
        assert_eq!(states(c, kid, DAY), vec![StopState::Done, StopState::Next, StopState::Locked]);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_course_taken_away_records_nothing_more() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let (study, review, play) = (p.activities[0].id, p.activities[1].id, p.activities[2].id);
        let (ana, cy, zed) = (k.kids[1], k.kids[0], k.kids[2]);
        // Ana has started the study, Cy the review, Zed the play (rounds 2, so it counts).
        assert!(start(c, ana, COURSE, DAY, study, NOW)?);
        do_stop(c, cy, COURSE, DAY, study, NOW);
        assert!(start(c, cy, COURSE, DAY, review, NOW)?);
        do_stop(c, zed, COURSE, DAY, study, NOW);
        do_stop(c, zed, COURSE, DAY, review, NOW);
        assert!(add(c, &k.course, COURSE, DAY, Kind::Review, None, NOW)?);
        assert!(set_rounds(c, COURSE, DAY, play, 2, NOW)?);
        assert!(start(c, zed, COURSE, DAY, play, NOW)?);
        assert_eq!(add_play(c, zed, play)?, 1);
        assert!(open_study_on(c, ana, COURSE, "u1-a")?);
        for kid in [ana, cy, zed] {
            assert!(crate::students::take_away(c, k.teacher, kid, COURSE)?);
        }
        assert!(!mark_studied(c, ana, study)?, "study: not marked");
        assert!(!open_study_on(c, ana, COURSE, "u1-a")?, "and the lesson not opened");
        assert!(!mark_reviewed(c, cy, review)?, "review: not marked");
        assert_eq!(add_play(c, zed, play)?, 1, "play: not counted");
        let marks: i64 = c.query_row("SELECT COUNT(*) FROM daily_done WHERE mark != 0 AND done_at IS NULL", [], |r| r.get(0))?;
        assert_eq!(marks, 0);
        // Given back, the same rows go on.
        for kid in [ana, cy, zed] {
            assert!(crate::students::give(c, k.teacher, kid, COURSE)?);
        }
        assert!(open_study_on(c, ana, COURSE, "u1-a")?);
        assert!(mark_studied(c, ana, study)? && mark_reviewed(c, cy, review)?);
        assert_eq!(add_play(c, zed, play)?, 2);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_teacher_sees_each_students_progress() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let names: Vec<String> = students_of(c, COURSE)?.into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, s(&["Ana Lee", "Cy Moss", "kid-zed"]), "students given the course, by name; not the one left out");
        assert!(progress(c, COURSE, DAY)?.iter().all(|r| r.cells.is_empty()), "no plan, no cells");
        assert_eq!(finished_counts(c, COURSE, DAY)?, (0, 3));
        recommended(c, &k.course, COURSE, DAY, NOW)?;
        let p = plan(c, COURSE, DAY)?.unwrap();
        let (study, review, play) = (p.activities[0].id, p.activities[1].id, p.activities[2].id);
        assign(c, COURSE, DAY, NOW)?;
        let (cy, ana, zed) = (k.kids[0], k.kids[1], k.kids[2]);
        for a in [study, review, play] {
            do_stop(c, ana, COURSE, DAY, a, NOW);
        }
        do_stop(c, cy, COURSE, DAY, study, NOW);
        assert!(start(c, cy, COURSE, DAY, review, NOW)?);
        let got: Vec<(i64, Vec<Cell>)> = progress(c, COURSE, DAY)?.into_iter().map(|r| (r.student_id, r.cells)).collect();
        assert_eq!(
            got,
            vec![
                (ana, vec![Cell::Done, Cell::Done, Cell::Done]),
                (cy, vec![Cell::Done, Cell::Doing, Cell::NotStarted]),
                (zed, vec![Cell::NotStarted, Cell::NotStarted, Cell::NotStarted]),
            ]
        );
        assert_eq!(finished_counts(c, COURSE, DAY)?, (1, 3));
        // Another teacher's student given another course is not in this class.
        let other = class(c, "u@example.org", "pup-", "daily-2")?;
        assert_eq!(students_of(c, "daily-2")?.len(), 3);
        assert!(students_of(c, COURSE)?.iter().all(|(id, _)| !other.kids.contains(id)));
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_week_row_says_what_each_day_holds() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let mon = week_start(DAY); // DAY is a Friday
        assert_eq!(mon, DAY - 4);
        // Monday: assigned, Ana finished it. Tuesday: a draft. Wednesday: nothing.
        recommended(c, &k.course, COURSE, mon, at(mon))?;
        assign(c, COURSE, mon, at(mon))?;
        for a in plan(c, COURSE, mon)?.unwrap().activities {
            do_stop(c, k.kids[1], COURSE, mon, a.id, at(mon));
        }
        add(c, &k.course, COURSE, mon + 1, Kind::Review, None, at(mon))?;
        // Friday (today): assigned, two activities. Saturday: a draft with nothing in it.
        add(c, &k.course, COURSE, DAY, Kind::Study, None, NOW)?;
        add(c, &k.course, COURSE, DAY, Kind::Play, None, NOW)?;
        assign(c, COURSE, DAY, NOW)?;
        add(c, &k.course, COURSE, DAY + 1, Kind::Study, None, NOW)?;
        let gone = plan(c, COURSE, DAY + 1)?.unwrap().activities[0].id;
        remove(c, COURSE, DAY + 1, gone, NOW)?;

        let row = week(c, COURSE, mon, NOW)?;
        assert_eq!(row.iter().map(|d| d.day).collect::<Vec<_>>(), (mon..mon + 7).collect::<Vec<_>>());
        assert_eq!(
            row.iter().map(|d| d.state).collect::<Vec<_>>(),
            vec![
                DayState::Past { finished: 1, students: 3 },
                DayState::Draft(1),
                DayState::Nothing,
                DayState::Nothing,
                DayState::Assigned(2),
                DayState::Draft(0),
                DayState::Nothing,
            ]
        );
        assert_eq!(row.iter().map(|d| d.plannable).collect::<Vec<_>>(), vec![false, false, false, false, true, true, true]);
        assert_eq!(first_plan_day(c, COURSE)?, Some(mon));
        // Four weeks on, the row ends at the last plannable day.
        let far = week(c, COURSE, mon + 28, NOW)?;
        assert_eq!(far.iter().filter(|d| d.plannable).count(), 5, "up to DAY + 28, a Friday");
        Ok(())
    })
    .unwrap();
}

#[test]
fn earlier_days_count_only_assigned_plans() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        // Three days back: assigned, one review. Two days back: assigned, one of
        // three done. One day back: a draft. Today: assigned.
        add(c, &k.course, COURSE, DAY - 3, Kind::Review, None, at(DAY - 3))?;
        assign(c, COURSE, DAY - 3, at(DAY - 3))?;
        recommended(c, &k.course, COURSE, DAY - 2, at(DAY - 2))?;
        assign(c, COURSE, DAY - 2, at(DAY - 2))?;
        recommended(c, &k.course, COURSE, DAY - 1, at(DAY - 1))?;
        recommended(c, &k.course, COURSE, DAY, NOW)?;
        assign(c, COURSE, DAY, NOW)?;
        // Done today, late, with no difference.
        let first = plan(c, COURSE, DAY - 2)?.unwrap().activities[0].id;
        do_stop(c, kid, COURSE, DAY - 2, first, NOW);
        assert_eq!(states(c, kid, DAY - 2), vec![StopState::Done, StopState::Next, StopState::Locked]);

        assert_eq!(unfinished_before(c, kid, COURSE, DAY)?, 3);
        assert_eq!(unfinished_before(c, k.kids[1], COURSE, DAY)?, 4);
        assert_eq!(unfinished_before(c, kid, COURSE, DAY - 2)?, 1);
        assert_eq!(unfinished_before(c, k.out, COURSE, DAY)?, 0, "not in the class");
        assert_eq!(
            earlier_days(c, kid, COURSE, DAY)?,
            vec![EarlierDay { day: DAY - 2, activities: 3, left: 2 }, EarlierDay { day: DAY - 3, activities: 1, left: 1 }]
        );
        assert!(earlier_days(c, kid, COURSE, DAY - 3)?.is_empty());
        assert!(earlier_days(c, k.out, COURSE, DAY)?.is_empty());
        Ok(())
    })
    .unwrap();
}

#[test]
fn what_is_left_today_per_subject() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let kid = k.kids[0];
        // A second subject with only a draft today, a third with nothing.
        for (id, title) in [("daily-2", "Two"), ("daily-3", "Three")] {
            crate::courses::create_course(c, k.teacher, id, title, &course_files(id, title))?;
            assert!(crate::students::give(c, k.teacher, kid, id)?);
        }
        assert!(add(c, &k.course, "daily-2", DAY, Kind::Study, None, NOW)?);
        assert_eq!(left_on(c, kid, COURSE, DAY)?, Some(3));
        assert_eq!(left_on(c, kid, "daily-2", DAY)?, None, "a draft");
        assert_eq!(left_on(c, kid, "daily-3", DAY)?, None);
        assert_eq!(left_today(c, kid, NOW)?, vec![(COURSE.to_string(), 3)]);
        do_stop(c, kid, COURSE, DAY, p.activities[0].id, NOW);
        assert_eq!(left_today(c, kid, NOW)?, vec![(COURSE.to_string(), 2)]);
        for a in &p.activities[1..] {
            do_stop(c, kid, COURSE, DAY, a.id, NOW);
        }
        assert_eq!(left_on(c, kid, COURSE, DAY)?, Some(0), "done for today");
        assert_eq!(left_today(c, kid, NOW)?, vec![(COURSE.to_string(), 0)]);
        assert_eq!(left_today(c, kid, at(DAY + 1))?, vec![], "nothing assigned tomorrow");
        assert_eq!(left_today(c, k.out, NOW)?, vec![]);
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_open_study_lets_the_lesson_be_read() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        let (kid, study) = (k.kids[0], p.activities[0].id);
        assert!(!open_study_on(c, kid, COURSE, "u1-a")?, "not started");
        assert!(start(c, kid, COURSE, DAY, study, NOW)?);
        assert!(open_study_on(c, kid, COURSE, "u1-a")?);
        assert!(!open_study_on(c, kid, COURSE, "u1-b")?);
        assert!(!open_study_on(c, k.kids[1], COURSE, "u1-a")?);
        assert!(mark_studied(c, kid, study)? && finish(c, kid, COURSE, DAY, study, NOW)?);
        assert!(!open_study_on(c, kid, COURSE, "u1-a")?, "finished");
        Ok(())
    })
    .unwrap();
}

#[test]
fn civil_turns_a_day_number_into_a_date() {
    assert_eq!(civil(0), (1970, 1, 1, 4), "1 January 1970 was a Thursday");
    assert_eq!(civil(DAY), (2024, 10, 4, 5), "a Friday");
    let monday = crate::earnings::day_of(0, 1_794_787_200);
    assert_eq!(civil(monday), (2026, 11, 16, 1), "16 November 2026 is a Monday");
    assert_eq!(civil(-1), (1969, 12, 31, 3));
    assert_eq!(civil(11_016), (2000, 2, 29, 2), "a leap day");
}

#[test]
fn from_civil_turns_a_date_back_into_its_day_number() {
    for day in [-800, -1, 0, 1, 11_016, DAY, MONDAY, 20_413, 40_000] {
        let (y, m, d, _) = civil(day);
        assert_eq!(from_civil(y, m, d), Some(day), "{y}-{m}-{d}");
    }
    assert_eq!(from_civil(2000, 2, 29), Some(11_016), "a leap day");
    for (y, m, d) in [(2026, 4, 31), (2026, 2, 29), (1900, 2, 29), (2026, 13, 1), (2026, 0, 1), (2026, 1, 0), (2026, 1, 32)] {
        assert_eq!(from_civil(y, m, d), None, "{y}-{m}-{d} does not exist");
    }
}

#[test]
fn school_days_are_monday_to_friday() {
    let got: Vec<bool> = (MONDAY..MONDAY + 7).map(school_day).collect();
    assert_eq!(got, vec![true, true, true, true, true, false, false]);
    assert_eq!(civil(MONDAY).3, 1);
    assert!(school_day(0), "1 January 1970, a Thursday");
    for d in MONDAY..MONDAY + 7 {
        assert_eq!(week_start(d), MONDAY);
    }
    assert_eq!(week_start(MONDAY - 1), MONDAY - 7);
    assert_eq!(week_start(-1), -3, "before 1970 too");
}

fn study_lessons(c: &Connection, day: i64) -> Vec<String> {
    plan(c, COURSE, day).unwrap().map(|p| p.activities[0].lessons.clone()).unwrap_or_default()
}

#[test]
fn plan_my_week_fills_an_empty_week_and_assigns_it() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        // Three lessons: three days, then it stops.
        let (filled, out) = auto_week(c, &k.course, COURSE, at(MONDAY))?;
        assert_eq!(filled, (MONDAY..MONDAY + 3).collect::<Vec<_>>());
        assert!(out, "the lessons ran out");
        for d in MONDAY..MONDAY + 3 {
            let p = plan(c, COURSE, d)?.unwrap();
            assert!(p.assigned, "day {d} assigned");
            assert_eq!(kinds(&p), vec![Kind::Study, Kind::Review, Kind::Play]);
        }
        let lessons: Vec<Vec<String>> = (MONDAY..MONDAY + 3).map(|d| study_lessons(c, d)).collect();
        assert_eq!(lessons, vec![s(&["u1-a"]), s(&["u1-b"]), s(&["u1-c"])], "consecutive lessons");
        for d in MONDAY + 3..MONDAY + 7 {
            assert!(plan(c, COURSE, d)?.is_none(), "day {d} stays empty");
        }
        assert_eq!(auto_week(c, &k.course, COURSE, at(MONDAY))?, (vec![], true), "a second press has no lesson left");
        // With every weekday planned, a press finds none empty.
        add(c, &k.course, COURSE, MONDAY + 3, Kind::Review, None, at(MONDAY))?;
        add(c, &k.course, COURSE, MONDAY + 4, Kind::Review, None, at(MONDAY))?;
        assert_eq!(auto_week(c, &k.course, COURSE, at(MONDAY))?, (vec![], false));
        Ok(())
    })
    .unwrap();
}

#[test]
fn plan_my_week_from_the_weekend_or_midweek_takes_the_next_seven_days() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        // Saturday: the weekend is skipped, Monday comes first.
        assert_eq!(auto_week(c, &k.course, COURSE, at(MONDAY - 2))?, (vec![MONDAY, MONDAY + 1, MONDAY + 2], true));
        assert!(plan(c, COURSE, MONDAY - 2)?.is_none() && plan(c, COURSE, MONDAY - 1)?.is_none());
        Ok(())
    })
    .unwrap();
    // Thursday, with two lessons left: Thursday and Friday, then it stops.
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let thu = MONDAY + 3;
        recommended(c, &k.course, COURSE, thu - 1, at(thu - 1))?;
        assert_eq!(auto_week(c, &k.course, COURSE, at(thu))?, (vec![thu, thu + 1], true));
        assert!(plan(c, COURSE, thu + 4)?.is_none(), "Monday stays empty");
        Ok(())
    })
    .unwrap();
}

#[test]
fn plan_my_week_leaves_a_day_with_a_plan_alone() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let now = at(MONDAY);
        add(c, &k.course, COURSE, MONDAY + 1, Kind::Review, None, now)?; // a draft
        add(c, &k.course, COURSE, MONDAY + 2, Kind::Study, None, now)?; // then an empty plan
        let gone = plan(c, COURSE, MONDAY + 2)?.unwrap().activities[0].id;
        remove(c, COURSE, MONDAY + 2, gone, now)?;
        recommended(c, &k.course, COURSE, MONDAY + 3, now)?; // assigned
        assign(c, COURSE, MONDAY + 3, now)?;
        recommended(c, &k.course, COURSE, MONDAY + 4, now)?; // cleared: no plan again
        clear(c, COURSE, MONDAY + 4, now)?;
        let before: Vec<_> = (MONDAY + 1..MONDAY + 4).map(|d| plan(c, COURSE, d).unwrap()).collect();

        assert_eq!(auto_week(c, &k.course, COURSE, now)?, (vec![MONDAY, MONDAY + 4], false));
        let after: Vec<_> = (MONDAY + 1..MONDAY + 4).map(|d| plan(c, COURSE, d).unwrap()).collect();
        assert_eq!(before, after, "the draft, the empty plan and the assigned day are untouched");
        assert!(!plan(c, COURSE, MONDAY + 1)?.unwrap().assigned);
        assert!(plan(c, COURSE, MONDAY + 4)?.unwrap().assigned);
        assert!(plan(c, COURSE, MONDAY - 1)?.is_none(), "a day before today is never touched");
        Ok(())
    })
    .unwrap();
}

#[test]
fn plan_my_week_fills_nothing_in_a_course_with_no_lessons() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        k(c);
        let mut files = crate::courses::starter_files(COURSE, "Daily");
        files.retain(|f| !f.path.starts_with("lessons/"));
        let empty = ContentStore::from_files(&files).unwrap();
        assert_eq!(auto_week(c, &empty, COURSE, at(MONDAY))?, (vec![], true));
        assert!(plan(c, COURSE, MONDAY)?.is_none(), "no empty plan left behind");
        Ok(())
    })
    .unwrap();
}

fn plan_count(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM daily_plans", [], |r| r.get(0)).unwrap()
}

#[test]
fn old_plans_are_deleted_so_a_course_holds_a_known_number() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let k = k(c);
        let kid = k.kids[0];
        assert_eq!(crate::caps::DAILY_KEEP_DAYS, 90);
        assert_eq!(crate::caps::DAILY_PLANS_PER_COURSE, 119);
        // A plan on the oldest day kept and one the day before it, both done by a student.
        let oldest = DAY - crate::caps::DAILY_KEEP_DAYS;
        for d in [oldest - 1, oldest] {
            recommended(c, &k.course, COURSE, d, at(d))?;
            assign(c, COURSE, d, at(d))?;
            do_stop(c, kid, COURSE, d, plan(c, COURSE, d)?.unwrap().activities[0].id, at(d));
        }
        assert_eq!(prune(c, NOW)?, 1);
        assert!(plan(c, COURSE, oldest - 1)?.is_none());
        assert!(plan(c, COURSE, oldest)?.is_some(), "the oldest day kept stays");
        let rows: i64 = c.query_row("SELECT COUNT(*) FROM daily_done", [], |r| r.get(0))?;
        assert_eq!(rows, 1, "the progress went with its plan");
        assert_eq!(earlier_days(c, kid, COURSE, DAY)?.len(), 1);
        assert_eq!(prune(c, NOW)?, 0);

        // Every day planned, every day for 150 days, pruned each day: never
        // more than DAILY_PLANS_PER_COURSE plans.
        c.execute("DELETE FROM daily_plans", [])?;
        let mut most = 0;
        for d in DAY..DAY + 150 {
            for ahead in 0..=PLAN_AHEAD_DAYS {
                if plan(c, COURSE, d + ahead)?.is_none() {
                    assert!(add(c, &k.course, COURSE, d + ahead, Kind::Review, None, at(d))?);
                }
            }
            prune(c, at(d))?;
            most = most.max(plan_count(c));
        }
        assert_eq!(most, crate::caps::DAILY_PLANS_PER_COURSE);
        Ok(())
    })
    .unwrap();
}

#[test]
fn deleting_a_course_or_a_student_takes_its_rows() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (k, p) = class_with_plan(c, "t@example.org", "kid-", COURSE, NOW)?;
        for kid in &k.kids {
            do_stop(c, *kid, COURSE, DAY, p.activities[0].id, NOW);
        }
        assert_eq!(crate::students::delete(c, k.teacher, &[k.kids[0]])?, 1);
        assert_eq!(done_count(c, p.activities[0].id), 2, "the deleted student's row went");
        c.execute("DELETE FROM courses WHERE id = ?1", [COURSE])?;
        assert_eq!(plan_count(c), 0);
        let rows: i64 = c.query_row("SELECT (SELECT COUNT(*) FROM daily_activities) + (SELECT COUNT(*) FROM daily_done)", [], |r| r.get(0))?;
        assert_eq!(rows, 0);
        Ok(())
    })
    .unwrap();
}
