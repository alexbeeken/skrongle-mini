use super::*;
use crate::content::CourseFile;
use crate::path::record;

const DAY: i64 = 86_400;
/// Noon on day 100, UTC.
const T0: i64 = 100 * DAY + 12 * 3600;

/// Unit 1: lessons `a` and `b`. Unit 2: `c` (no vocabulary). Each is the
/// starter lesson otherwise.
fn course() -> ContentStore {
    let files = crate::courses::starter_files("p", "Path");
    let mut manifest: serde_json::Value = serde_json::from_str(&files[0].content).unwrap();
    manifest["units"].as_array_mut().unwrap().push(serde_json::json!({ "id": "2", "title": "Two", "description": "More" }));
    let mut out = vec![CourseFile { path: files[0].path.clone(), content: manifest.to_string() }];
    for (n, (id, unit)) in [("a", "1"), ("b", "1"), ("c", "2")].into_iter().enumerate() {
        let mut l: serde_json::Value = serde_json::from_str(&files[1].content).unwrap();
        l["id"] = id.into();
        l["slug"] = id.into();
        l["title"] = format!("Lesson {id}").into();
        l["unit"] = unit.into();
        l["order"] = (n + 1).into();
        for q in l["quiz"].as_array_mut().unwrap() {
            q["question_id"] = format!("{id}-{}", q["question_id"].as_str().unwrap()).into();
        }
        if id == "c" {
            l["vocabulary"] = serde_json::json!([]);
            for q in l["quiz"].as_array_mut().unwrap() {
                q.as_object_mut().unwrap().remove("teaches");
            }
        }
        out.push(CourseFile { path: format!("lessons/{id}.json"), content: l.to_string() });
    }
    ContentStore::from_files(&out).unwrap()
}

fn names(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| format!("{} {}", i.kind.as_str(), i.lesson_id)).collect()
}

/// Runs `f` on a fresh database with one learner (id 1).
fn with_db(f: impl FnOnce(&Connection) -> rusqlite::Result<()>) {
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        crate::auth::create_user(c, "ann@example.org", "x")?;
        f(c)
    })
    .unwrap();
}

fn at(c: &Connection, store: &ContentStore, start: usize, now: i64) -> Path {
    list(c, 1, store, start, now, 0).unwrap()
}

/// Meets learner 1's daily goal in course `p` on the day of `now`.
fn meet_goal(c: &Connection, now: i64) -> rusqlite::Result<()> {
    crate::activity::record_at(c, 1, "p", crate::activity::Kind::Answer, crate::activity::DAILY_GOAL, now, 0)
}

#[test]
fn a_new_learner_starts_at_the_first_lessons_study_and_review() {
    let store = course();
    with_db(|c| {
        let p = at(c, &store, 0, T0);
        assert_eq!(names(&p.next), ["study a", "review a", "study b", "review b", "study c", "review c"]);
        assert!(p.done_today.is_empty() && p.hidden.is_empty());
        assert!(p.next.iter().all(|i| !i.tried && i.score == Some(0.0) && i.streak == 0));
        assert_eq!(p.next[0].title, "Lesson a");
        Ok(())
    });
}

#[test]
fn what_was_done_today_leaves_next_and_a_review_returns_the_next_day() {
    let store = course();
    with_db(|c| {
        record(c, 1, "p", "a", Kind::Study, 100, T0 - 60)?;
        record(c, 1, "p", "a", Kind::Review, 100, T0)?;
        meet_goal(c, T0)?;
        let p = at(c, &store, 0, T0 + 60);
        assert_eq!(names(&p.next)[..2], ["play a", "study b"]);
        assert_eq!(names(&p.done_today), ["study a", "review a"]);
        assert!(p.done_today.iter().all(|i| i.tried));
        let p = at(c, &store, 0, T0 + DAY);
        assert_eq!(names(&p.next)[..2], ["review a", "study b"], "the Study does not repeat, and a new day's goal is not met yet");
        assert!(p.done_today.is_empty());
        Ok(())
    });
}

/// The day of [`T0`] plus `d`.
fn day(d: i64) -> i64 {
    T0 + d * DAY
}

#[test]
fn a_review_tried_cleanly_each_time_it_is_due_rests_for_two_weeks_once_mastered() {
    let store = course();
    with_db(|c| {
        for d in [0, 1, 3] {
            record(c, 1, "p", "a", Kind::Review, 100, day(d))?;
        }
        let has = |c: &Connection, d| names(&at(c, &store, 0, day(d)).next).contains(&"review a".to_string());
        assert!((4..17).all(|d| !has(c, d)), "two clean reviews after the first rest it 14 days");
        assert!(has(c, 17));
        let review = at(c, &store, 0, day(17)).next.into_iter().find(|i| i.lesson_id == "a" && i.kind == Kind::Review).unwrap();
        assert_eq!((review.streak, review.gap, review.score), (4, 14.0, Some(0.0)));
        Ok(())
    });
}

#[test]
fn a_hidden_activity_leaves_next_alone_and_comes_back_as_it_was() {
    let store = course();
    with_db(|c| {
        hide(c, 1, "p", "b", Kind::Study, T0)?;
        let p = at(c, &store, 0, T0);
        assert_eq!(names(&p.next)[2], "review b", "the Review stays");
        assert_eq!(names(&p.hidden), ["study b"]);
        unhide(c, 1, &store, 0, "b", Kind::Study, T0)?;
        let p = at(c, &store, 0, T0);
        assert_eq!(names(&p.next)[2], "study b");
        assert!(p.hidden.is_empty());

        // A returning Review hidden and brought back keeps its record; hiding records nothing.
        record(c, 1, "p", "a", Kind::Review, 100, day(-3))?;
        hide(c, 1, "p", "a", Kind::Review, T0)?;
        let p = at(c, &store, 0, T0);
        assert!(!names(&p.next).contains(&"review a".to_string()));
        assert_eq!(p.hidden[0].score, Some(-2.0));
        let tried: i64 = c.query_row("SELECT COUNT(*) FROM path_activity", [], |r| r.get(0))?;
        assert_eq!(tried, 1);
        unhide(c, 1, &store, 0, "a", Kind::Review, T0)?;
        let p = at(c, &store, 0, T0);
        assert_eq!((names(&p.next)[0].as_str(), p.next[0].score), ("review a", Some(-2.0)));
        Ok(())
    });
}

#[test]
fn a_lesson_without_vocabulary_has_no_play() {
    let store = course();
    assert_eq!(kinds_of(&store, store.lesson_by_id("c").unwrap()), [Kind::Study, Kind::Review]);
    assert_eq!(kinds_of(&store, store.lesson_by_id("a").unwrap()), Kind::ALL);
}

#[test]
fn the_lessons_a_higher_start_skipped_are_hidden_until_one_is_brought_back() {
    let store = course();
    with_db(|c| {
        let p = at(c, &store, 1, T0);
        assert_eq!(names(&p.next), ["study c", "review c"]);
        assert_eq!(names(&p.hidden), ["study a", "review a", "play a", "study b", "review b", "play b"]);
        unhide(c, 1, &store, 1, "a", Kind::Review, T0)?;
        let p = at(c, &store, 1, T0);
        assert_eq!(names(&p.next), ["review a", "study c", "review c"]);
        assert_eq!(names(&p.hidden), ["study a", "play a", "study b", "review b", "play b"]);
        // Touching a skipped lesson makes it the learner's.
        crate::activity::touch(c, 1, "p", "b", crate::activity::Mode::Study)?;
        let p = at(c, &store, 1, T0);
        assert_eq!(names(&p.next), ["review a", "study b", "review b", "study c", "review c"]);
        Ok(())
    });
}

#[test]
fn the_day_is_the_learners() {
    let store = course();
    with_db(|c| {
        // 23:00 UTC on day 100 is 01:00 on day 101 two hours east.
        record(c, 1, "p", "a", Kind::Review, 100, 100 * DAY + 23 * 3600)?;
        let p = list(c, 1, &store, 0, 101 * DAY + 1800, 2 * 3600)?;
        assert_eq!(names(&p.done_today), ["review a"]);
        let p = list(c, 1, &store, 0, 101 * DAY + 1800, 0)?;
        assert!(p.done_today.is_empty());
        assert_eq!(names(&p.next)[..2], ["study a", "review a"]);
        Ok(())
    });
}

#[test]
fn a_play_is_listed_once_the_days_goal_is_met() {
    let store = course();
    with_db(|c| {
        hide(c, 1, "p", "b", Kind::Play, T0)?;
        let p = at(c, &store, 0, T0);
        assert!(p.next.iter().all(|i| i.kind != Kind::Play), "no Play before the goal");
        assert_eq!(names(&p.hidden), ["play b"], "a hidden Play shows as hidden either way");
        meet_goal(c, T0 - 60)?;
        let p = at(c, &store, 0, T0);
        assert_eq!(names(&p.next), ["study a", "review a", "play a", "study b", "review b", "study c", "review c"]);
        assert_eq!(names(&p.hidden), ["play b"]);
        // Yesterday's goal does not count today; a Play done today still shows as done.
        record(c, 1, "p", "a", Kind::Play, 100, T0)?;
        let p = at(c, &store, 0, T0 + DAY / 2 - 60);
        assert_eq!(names(&p.done_today), ["play a"]);
        let p = at(c, &store, 0, T0 + DAY);
        assert!(p.next.iter().all(|i| i.kind != Kind::Play));
        Ok(())
    });
}
