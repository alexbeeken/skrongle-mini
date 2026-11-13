use super::*;

/// #628: the lessons view asks about one course with a primary-key lookup.
#[test]
fn has_project_is_one_key_lookup() {
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        assert!(!has_project(c, uid, "spanish")?);
        open_project(c, uid, "spanish")?;
        assert!(has_project(c, uid, "spanish")?);
        assert!(!has_project(c, uid, "french")?);
        let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {HAS_PROJECT}"))?;
        let plan: Vec<String> = stmt.query_map(params![uid, "spanish"], |r| r.get::<_, String>(3))?.collect::<Result<_, _>>()?;
        assert!(plan.iter().all(|d| !d.contains("SCAN")), "{plan:?}");
        assert!(plan.iter().any(|d| d.contains("SEARCH projects USING PRIMARY KEY")), "{plan:?}");
        Ok(())
    })
    .unwrap();
}

/// A draft is the starter course with nothing but names changed.
#[test]
fn a_draft_changes_only_names() {
    let starter = starter_files("c-1", &untitled());
    assert_eq!(draft_of(&starter), Some(Draft::default()));
    let edit = |path: &str, f: &dyn Fn(&mut serde_json::Value)| {
        let mut files = starter.clone();
        let file = files.iter_mut().find(|x| x.path == path).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&file.content).unwrap();
        f(&mut v);
        file.content = serde_json::to_string_pretty(&v).unwrap();
        files
    };
    // Named: the course, the starter lesson, a lesson added by name.
    let named = edit(MANIFEST_FILE, &|v| {
        v["title"] = "Rust".into();
        v["subject"] = "Rust".into();
    });
    assert_eq!(draft_of(&named), Some(Draft { title: Some("Rust".into()), lessons: vec![] }));
    let mut renamed = edit("lessons/first-lesson.json", &|v| v["title"] = "Ownership".into());
    renamed.push(CourseFile {
        path: "lessons/borrowing.json".into(),
        content: crate::arrange::blank_lesson("borrowing", "Borrowing", "1", "basics"),
    });
    let mut later = serde_json::from_str::<serde_json::Value>(&renamed[2].content).unwrap();
    later["title"] = "Borrowing rules".into();
    renamed[2].content = later.to_string();
    assert_eq!(draft_of(&renamed), Some(Draft { title: None, lessons: vec!["Ownership".into(), "Borrowing rules".into()] }));
    // Content: not a draft any more.
    assert_eq!(draft_of(&edit("lessons/first-lesson.json", &|v| v["summary"] = "Mine".into())), None);
    assert_eq!(draft_of(&edit(MANIFEST_FILE, &|v| v["description"] = "Mine".into())), None);
    let mut extra = starter.clone();
    extra.push(CourseFile { path: "pictures/a.svg".into(), content: "<svg/>".into() });
    assert_eq!(draft_of(&extra), None);
    let mut written = renamed.clone();
    written[2].content = crate::arrange::blank_lesson("borrowing", "Borrowing", "1", "basics").replace("Write the lesson here", "Borrowing is");
    assert_eq!(draft_of(&written), None);
}

/// #1255: "Create a course" in German writes German starter names, and the
/// course still counts as an untouched draft whatever language checks it.
#[test]
fn a_starter_made_in_german_is_german_and_still_a_draft() {
    let (title, starter) = crate::i18n::in_lang("de", || (untitled(), starter_files("c-1", &untitled())));
    assert_eq!(title, "Kurs ohne Titel");
    let manifest: serde_json::Value = serde_json::from_str(&starter[0].content).unwrap();
    assert_eq!(manifest["description"], "Beschreib den Kurs in ein oder zwei Sätzen.");
    assert_eq!(manifest["units"][0]["title"], "Einheit 1");
    let lesson: serde_json::Value = serde_json::from_str(&starter[1].content).unwrap();
    assert_eq!(lesson["title"], "Deine erste Lektion");
    assert_eq!(draft_of(&starter), Some(Draft::default()));
    assert_eq!(crate::i18n::in_lang("fr", || draft_of(&starter)), Some(Draft::default()));
}

#[test]
fn starter_course_is_valid_and_paths_are_checked() {
    let files = starter_files("my-course-abc123", "My course");
    let store = ContentStore::from_files(&files).unwrap();
    assert_eq!(store.id(), "my-course-abc123");
    assert_eq!(store.lessons().len(), 1);
    assert!(valid_path("course.json"));
    assert!(valid_path("lessons/a1/hello.json"));
    assert!(!valid_path("lessons/../x.json"));
    assert!(!valid_path("other.json"));
    assert!(valid_path("pictures/kitchen.svg"));
    assert!(!valid_path("pictures/kitchen.json"));
    assert!(!valid_path("lessons/kitchen.svg"));
    assert!(!valid_path("pictures/../x.svg"));
    assert!(!valid_path("lessons/"));
    assert!(!valid_path("/lessons/x.json"));
    let id = new_course_id("Music Theory 101!");
    assert!(id.starts_with("music-theory-101-"));
    assert!(crate::content::is_slug(&id));
}

/// Startup and course deletion find their rows through an index, not by
/// reading every course, project and learner (#618, #619).
#[test]
fn startup_and_delete_course_use_indexes() {
    use crate::db::plan::assert_indexed;
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        assert_indexed(c, "UPDATE courses SET state = 'ready' WHERE state = 'revising'", "courses", Some("idx_courses_state_updated"));
        assert_indexed(c, "UPDATE courses SET state = 'failed', updated_at = ?1, error = '' WHERE state = 'generating'", "courses", None);
        assert_indexed(c, "DELETE FROM courses WHERE id = ?1", "courses", None);
        assert_indexed(c, "DELETE FROM projects WHERE course_id = ?1", "projects", Some("idx_projects_course"));
        assert_indexed(c, "UPDATE users SET active_course = NULL WHERE active_course = ?1", "users", Some("idx_users_active_course"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn generated_courses_have_a_lifecycle() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let registry = Courses::empty();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let id = "algebra-abc123";
        create_generating(c, uid, id, "Algebra", "factoring for Friday", "x^2 + 5x + 6 = (x+2)(x+3)")?;
        set_generating(c, id, "job1")?;
        let row = course(c, id)?.unwrap();
        assert!(row.is_generating());
        assert_eq!((row.source.as_str(), row.job_id.as_deref()), ("ai", Some("job1")));
        assert_eq!(notes_of(c, id)?, "x^2 + 5x + 6 = (x+2)(x+3)");
        // Half-written: the files exist but the course cannot be loaded.
        for f in starter_files(id, "Algebra") {
            write_file(c, id, &f.path, &f.content)?;
        }
        assert!(load_ready_files(c, id)?.is_empty());
        set_outline(c, id, "Algebra I", "{}")?;
        // A restart interrupts it: failed, resumable.
        assert_eq!(fail_interrupted(c)?, 1);
        let row = course(c, id)?.unwrap();
        assert_eq!(row.state, CourseState::Failed);
        assert!(row.error.unwrap().contains("restarted"));
        assert_eq!(row.title, "Algebra I");
        assert_eq!(load_ready_files(c, id)?.len(), 2);
        // Saving into it, even so it loads, keeps it stopped and resumable, with its reason (#81).
        assert_eq!(check_here(c, &registry, id)?, None);
        let row = course(c, id)?.unwrap();
        assert_eq!(row.state, CourseState::Failed);
        assert!(row.outline.is_some());
        assert!(row.error.unwrap().contains("restarted"));
        // A save that breaks it does not replace the reason either.
        write_file(c, id, "lessons/broken.json", "{}")?;
        assert!(check_here(c, &registry, id)?.is_some());
        let row = course(c, id)?.unwrap();
        assert_eq!(row.state, CourseState::Failed);
        assert!(row.error.unwrap().contains("restarted"));
        assert!(delete_file(c, id, "lessons/broken.json")?);
        assert_eq!(check_here(c, &registry, id)?, None);
        // One that stopped before its outline, fixed by hand, is a course now.
        c.execute("UPDATE courses SET outline = NULL WHERE id = ?1", [id])?;
        assert_eq!(check_here(c, &registry, id)?, None);
        let row = course(c, id)?.unwrap();
        assert_eq!((row.state, row.error), (CourseState::Ready, None));
        set_outline(c, id, "Algebra I", "{}")?;
        // A job that finishes cleanly marks it ready; one that fails keeps the reason.
        set_generating(c, id, "job2")?;
        finish_generation(c, id, Some("Cancelled."))?;
        let row = course(c, id)?.unwrap();
        assert_eq!((row.state, row.error.as_deref()), (CourseState::Failed, Some("Cancelled.")));
        set_generating(c, id, "job3")?;
        finish_generation(c, id, None)?;
        assert_eq!(course(c, id)?.unwrap().state, CourseState::Ready);
        Ok(())
    })
    .unwrap();
}

#[test]
fn courses_round_trip_through_the_database() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let registry = Courses::empty();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let id = "my-course-abc123";
        create_course(c, uid, id, "My course", &starter_files(id, "My course"))?;
        assert!(course_exists(c, id)?);
        assert_eq!(courses_owned_by(c, uid)?.len(), 1);
        assert_eq!(file_count(c, id)?, 2);
        assert_eq!(check_here(c, &registry, id)?, None);
        // Break the manifest: the error is recorded and the cache dropped.
        write_file(c, id, "course.json", "{ not json")?;
        let err = check_here(c, &registry, id)?.unwrap();
        assert!(err.starts_with("Not valid JSON in the course settings, line 1"), "{err}");
        assert!(course(c, id)?.unwrap().error.is_some());
        assert!(registry.lock().get(id).is_none());
        // Fix it again and open it as a project.
        let good = starter_files(id, "My course");
        write_file(c, id, "course.json", &good[0].content)?;
        assert_eq!(check_here(c, &registry, id)?, None);
        assert!(open_project(c, uid, id)?, "the first open");
        assert!(!open_project(c, uid, id)?, "opened before");
        assert_eq!(projects_of(c, uid)?[0].course_id, id);
        // The starting level travels with the session's user, per course.
        set_project_settings(c, uid, id, &SettingsChange { start_unit: Some("2".into()), ..Default::default() })?;
        let tok = crate::auth::create_session(c, uid)?;
        assert_eq!(crate::auth::user_for_session(c, &tok)?.unwrap().start_unit, "2");
        open_project(c, uid, "spanish")?;
        assert_eq!(crate::auth::user_for_session(c, &tok)?.unwrap().start_unit, "");
        open_project(c, uid, id)?;
        let active: Option<String> = c.query_row("SELECT active_course FROM users WHERE id = ?1", [uid], |r| r.get(0))?;
        assert_eq!(active.as_deref(), Some(id));
        assert!(delete_file(c, id, "lessons/first-lesson.json")?);
        assert!(!delete_file(c, id, "lessons/first-lesson.json")?);
        delete_course(c, id)?;
        assert!(!course_exists(c, id)?);
        assert!(projects_of(c, uid)?.iter().all(|p| p.course_id != id));
        Ok(())
    })
    .unwrap();
}

#[test]
fn one_editor_session_is_one_undo_step() {
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let id = "session-abc123";
        create_course(c, uid, id, "Session", &starter_files(id, "Session"))?;
        let original = read_file(c, id, "course.json")?.unwrap();
        let manifest = |n: usize| CourseFile { path: "course.json".into(), content: format!("v{n}") };
        // No step of its own yet: nothing to join.
        assert!(!amend_session_step(c, id, "tab-a", &[manifest(1)], None, |_| "x".into())?);
        commit_revision_in(c, id, &[manifest(1)], "Session", None, "first save", Some("tab-a"))?;
        // Later saves of the same page join it; the label sees the files from before the step.
        let mut seen = String::new();
        assert!(amend_session_step(c, id, "tab-a", &[manifest(2)], None, |before| {
            seen = before.iter().find(|f| f.path == "course.json").unwrap().content.clone();
            "Your edits".into()
        })?);
        assert_eq!(seen, original);
        assert_eq!(read_file(c, id, "course.json")?.unwrap(), "v2");
        assert_eq!(undo_step(c, id)?.map(|s| (s.label, s.steps)), Some(("Your edits".to_string(), 1)));
        // Another tab, or a step that is not this page's, starts a new step.
        assert!(!amend_session_step(c, id, "tab-b", &[manifest(3)], None, |_| "x".into())?);
        commit_revision(c, id, &[manifest(3)], "Session", None, "Added with AI")?;
        assert!(!amend_session_step(c, id, "tab-a", &[manifest(4)], None, |_| "x".into())?);
        // Ten minutes on, the same page starts a new step too.
        commit_revision_in(c, id, &[manifest(4)], "Session", None, "later", Some("tab-a"))?;
        c.execute("UPDATE course_undo SET started_at = started_at - ?2 WHERE course_id = ?1", params![id, SESSION_STEP_SECS])?;
        assert!(!amend_session_step(c, id, "tab-a", &[manifest(5)], None, |_| "x".into())?);
        // One Undo takes the whole session back.
        undo_revision(c, id)?;
        undo_revision(c, id)?;
        assert_eq!(undo_revision(c, id)?.as_deref(), Some("Your edits"));
        assert_eq!(read_file(c, id, "course.json")?.unwrap(), original);
        Ok(())
    })
    .unwrap();
}

#[test]
fn redo_takes_an_undo_back_until_a_new_change() {
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let id = "redo-abc123";
        create_course(c, uid, id, "Redo", &starter_files(id, "Redo"))?;
        let original = read_file(c, id, "course.json")?.unwrap();
        let manifest = |n: usize| CourseFile { path: "course.json".into(), content: format!("v{n}") };
        let now_is = |c: &Connection| read_file(c, id, "course.json").map(|f| f.unwrap());
        assert_eq!(redo_revision(c, id)?, None);
        for n in 1..=3 {
            commit_revision(c, id, &[manifest(n)], &format!("Redo {n}"), None, &format!("change {n}"))?;
        }
        assert_eq!(redo_step(c, id)?, None);
        // Two undos, then two redos: back where it was, title too.
        assert_eq!(undo_revision(c, id)?.as_deref(), Some("change 3"));
        assert_eq!(undo_revision(c, id)?.as_deref(), Some("change 2"));
        assert_eq!(now_is(c)?, "v1");
        assert_eq!(redo_step(c, id)?.map(|s| (s.label, s.steps)), Some(("change 2".to_string(), 2)));
        assert_eq!(undo_step(c, id)?.map(|s| s.steps), Some(1));
        assert_eq!(redo_revision(c, id)?.as_deref(), Some("change 2"));
        assert_eq!(now_is(c)?, "v2");
        assert_eq!(undo_step(c, id)?.map(|s| (s.label, s.steps)), Some(("change 2".to_string(), 2)));
        assert_eq!(redo_revision(c, id)?.as_deref(), Some("change 3"));
        assert_eq!(now_is(c)?, "v3");
        assert_eq!(course(c, id)?.unwrap().title, "Redo 3");
        assert_eq!(redo_step(c, id)?, None);
        // Undo all the way and redo all the way.
        for _ in 0..3 {
            undo_revision(c, id)?;
        }
        assert_eq!(now_is(c)?, original);
        for _ in 0..3 {
            redo_revision(c, id)?;
        }
        assert_eq!(now_is(c)?, "v3");
        // A new change after an undo drops what could be redone.
        undo_revision(c, id)?;
        assert!(redo_step(c, id)?.is_some());
        commit_revision(c, id, &[manifest(9)], "Redo", None, "change 9")?;
        assert_eq!(redo_step(c, id)?, None);
        assert_eq!(undo_step(c, id)?.map(|s| (s.label, s.steps)), Some(("change 9".to_string(), 3)));
        // So does an editor session's save joining its step.
        commit_revision_in(c, id, &[manifest(10)], "Redo", None, "tab", Some("tab-a"))?;
        commit_revision(c, id, &[manifest(11)], "Redo", None, "Added with AI")?;
        undo_revision(c, id)?;
        assert!(redo_step(c, id)?.is_some());
        assert!(amend_session_step(c, id, "tab-a", &[manifest(12)], None, |_| "tab".into())?);
        assert_eq!(redo_step(c, id)?, None);
        // A redone step is no session's: the page's next save is a step of its own.
        undo_revision(c, id)?;
        redo_revision(c, id)?;
        assert!(!amend_session_step(c, id, "tab-a", &[manifest(13)], None, |_| "tab".into())?);
        let stray: i64 = c.query_row("SELECT COUNT(*) FROM course_undo_files WHERE seq < 0", [], |r| r.get(0))?;
        assert_eq!(stray, 0);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_version_changes_with_any_file() {
    let f = |p: &str, c: &str| CourseFile { path: p.into(), content: c.into() };
    let a = version_of(&[f("course.json", "{}"), f("lessons/a.json", "1")]);
    assert_eq!(a, version_of(&[f("lessons/a.json", "1"), f("course.json", "{}")]));
    assert_ne!(a, version_of(&[f("course.json", "{}"), f("lessons/a.json", "2")]));
    assert_ne!(a, version_of(&[f("course.json", "{}"), f("lessons/b.json", "1")]));
    assert_ne!(version_of(&[f("ab", "c")]), version_of(&[f("a", "bc")]));
}

#[test]
fn undo_walks_back_through_the_last_ten_changes() {
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let id = "undo-abc123";
        create_course(c, uid, id, "Undo", &starter_files(id, "Undo"))?;
        let manifest = |n: usize| CourseFile { path: "course.json".into(), content: format!("v{n}") };
        for n in 1..=12 {
            commit_revision(c, id, &[manifest(n)], "Undo", None, &format!("change {n}"))?;
        }
        let step = undo_step(c, id)?.unwrap();
        assert_eq!((step.label.as_str(), step.steps), ("change 12", UNDO_STEPS as usize));
        // Newest first, each putting back the files from before it.
        for n in (3..=12).rev() {
            assert_eq!(undo_revision(c, id)?.as_deref(), Some(format!("change {n}").as_str()));
            assert_eq!(read_file(c, id, "course.json")?.unwrap(), format!("v{}", n - 1));
        }
        assert_eq!(undo_revision(c, id)?, None);
        assert_eq!(undo_step(c, id)?, None);
        let left: i64 = c.query_row("SELECT COUNT(*) FROM course_undo_files WHERE seq > 0", [], |r| r.get(0))?;
        assert_eq!(left, 0);
        // Every undo can be redone.
        assert_eq!(redo_step(c, id)?.map(|s| s.steps), Some(UNDO_STEPS as usize));
        // The history goes with the course.
        commit_revision(c, id, &[manifest(1)], "Undo", None, "again")?;
        delete_course(c, id)?;
        let left: i64 = c.query_row("SELECT COUNT(*) FROM course_undo", [], |r| r.get(0))?;
        assert_eq!(left, 0);
        Ok(())
    })
    .unwrap();
    assert_eq!(quoted("Removed", "a  b\nc"), "Removed \u{201c}a b c\u{201d}");
    assert!(quoted("X", &"y".repeat(300)).ends_with("\u{2026}\u{201d}"));
}

/// #366: a course and an account have a size limit, an account a course
/// count; `room_for` measures what the course would take after a write.
#[test]
fn course_and_account_size_limits() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let limits = Limits { max_courses: 2, course_bytes: 10_000, account_bytes: 15_000 };
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        assert!(room_for_course(c, uid, limits)?.is_none());
        let (a, b) = ("a-abc123", "b-abc123");
        create_course(c, uid, a, "A", &starter_files(a, "A"))?;
        create_course(c, uid, b, "B", &starter_files(b, "B"))?;
        let p = room_for_course(c, uid, limits)?.expect("two courses is the limit");
        assert!(p.text.contains("an account may have at most 2"), "{}", p.text);

        let start = size_of(&starter_files(a, "A"));
        assert_eq!(size_after(c, a, &[])?, start);
        let big = CourseFile { path: "lessons/big.json".into(), content: "x".repeat(6_000) };
        // Replacing a file counts its new size only.
        let manifest = &starter_files(a, "A")[0];
        assert_eq!(size_after(c, a, std::slice::from_ref(manifest))?, start);
        assert_eq!(size_after(c, a, std::slice::from_ref(&big))?, start + big.path.len() + 6_000);
        // Multi-byte text counts in bytes.
        let accents = CourseFile { path: "lessons/big.json".into(), content: "é".repeat(100) };
        assert_eq!(size_after(c, a, std::slice::from_ref(&accents))?, start + accents.path.len() + 200);

        assert!(room_for(c, a, size_after(c, a, std::slice::from_ref(&big))?, limits)?.is_none());
        write_file(c, a, &big.path, &big.content)?;
        // Over the course's own limit.
        let huge = CourseFile { path: "lessons/huge.json".into(), content: "x".repeat(5_000) };
        let p = room_for(c, a, size_after(c, a, std::slice::from_ref(&huge))?, limits)?.expect("over the course limit");
        assert!(p.text.contains("a course may take at most"), "{}", p.text);
        // Within B's limit, but A and B together are over the account's.
        let p = room_for(c, b, size_after(c, b, std::slice::from_ref(&huge))?, limits)?.expect("over the account limit");
        assert!(p.text.contains("all of an account's courses together may take at most"), "{}", p.text);
        // Another account's courses do not count.
        let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
        let bc = "bob-abc123";
        create_course(c, bob, bc, "Bob", &starter_files(bc, "Bob"))?;
        assert!(room_for(c, bc, size_after(c, bc, std::slice::from_ref(&huge))?, limits)?.is_none());
        Ok(())
    })
    .unwrap();
    // The defaults leave room for the biggest real course, twice over.
    let d = Limits::default();
    assert!(d.course_bytes >= 2 * 3_725_754 && d.account_bytes >= 4 * d.course_bytes, "{d:?}");
    assert_eq!(megabytes(8 * 1024 * 1024), "8 MB");
    assert_eq!(megabytes(3 * 1024 * 1024 + 1), "3.1 MB");
}

/// #629: a copy of a built-in course has every file of it, the manifest
/// renamed to the copy's id and title; it loads, and it counts against
/// the account like any other course.
#[test]
fn a_copy_has_every_file_under_its_own_id() {
    let registry = Courses::load_builtin(Path::new("data")).unwrap();
    let original = registry.builtin_files("spanish").unwrap().unwrap();
    assert!(registry.builtin_files("no-such-course").is_none());
    let title = short_title(&format!("Copy of {}", "x".repeat(100)));
    assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
    let copy = copied_files(&original, "copy-abc123", "Copy of Spanish").unwrap();
    assert_eq!(copy.len(), original.len());
    for (a, b) in original.iter().zip(&copy) {
        assert_eq!(a.path, b.path);
        if a.path != MANIFEST_FILE {
            assert_eq!(a.content, b.content, "{}", a.path);
        }
    }
    let store = ContentStore::from_files(&copy).unwrap();
    assert_eq!((store.id(), store.title()), ("copy-abc123", "Copy of Spanish"));
    let plain = ContentStore::from_files(&original).unwrap();
    assert_eq!(store.lessons().len(), plain.lessons().len());
    assert_eq!(store.manifest().subject, plain.manifest().subject, "the subject stays");
    assert!(copied_files(&original[1..], "x", "y").is_err(), "no manifest, no copy");

    let db = crate::db::Db::open_in_memory().unwrap();
    let size = size_of(&copy);
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let roomy = Limits { max_courses: 5, course_bytes: size, account_bytes: 2 * size };
        assert!(room_for_copy(c, uid, size, roomy)?.is_none());
        create_course(c, uid, "copy-abc123", "Copy of Spanish", &copy)?;
        assert!(room_for_copy(c, uid, size, roomy)?.is_none(), "two copies fit");
        create_course(c, uid, "copy-def456", "Copy of Spanish", &copy)?;
        let p = room_for_copy(c, uid, size, roomy)?.expect("a third is over the account limit");
        assert!(p.text.contains("all of an account's courses together"), "{}", p.text);
        let small = Limits { course_bytes: size - 1, ..roomy };
        let p = room_for_copy(c, uid, size, small)?.expect("over the course limit");
        assert!(p.text.contains("a course may take at most"), "{}", p.text);
        Ok(())
    })
    .unwrap();
}

/// #33: the course cache keeps at most `MAX_STORES` built courses, least
/// recently used out first; the course just added always stays.
#[test]
fn the_course_cache_drops_the_least_recently_used() {
    let store = |id: &str| Arc::new(ContentStore::from_files(&starter_files(id, "C")).unwrap());
    let mut cache = Cache { max_entries: 2, ..Cache::default() };
    cache.insert("a", store("a"));
    cache.insert("b", store("b"));
    assert!(cache.get("a").is_some(), "a is now the most recently used");
    cache.insert("c", store("c"));
    assert!(cache.get("b").is_none(), "b was the least recently used");
    assert!(cache.get("a").is_some() && cache.get("c").is_some());
    // Replacing an entry does not count it twice.
    cache.insert("c", store("c"));
    assert_eq!(cache.entries.len(), 2);
    // The store just put in stays, whatever the bound.
    let mut tiny = Cache { max_entries: 0, ..Cache::default() };
    tiny.insert("x", store("x"));
    assert!(tiny.get("x").is_some());
}

/// #33: with 9 courses opened in turn, 8 stay built and the oldest is built
/// again on its next use.
#[test]
fn nine_courses_keep_eight_built() {
    assert_eq!(MAX_STORES, 8);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let db = crate::db::Db::open_in_memory().unwrap();
    let registry = Courses::empty();
    let ids: Vec<String> = (0..9).map(|i| format!("c{i}-abc123")).collect();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        for id in &ids {
            create_course(c, uid, id, "C", &starter_files(id, "C"))?;
        }
        Ok(())
    })
    .unwrap();
    let first = rt.block_on(registry.get(&db, &ids[0])).unwrap().unwrap();
    for id in &ids[1..] {
        rt.block_on(registry.get(&db, id)).unwrap().unwrap();
        assert!(registry.lock().entries.len() <= MAX_STORES);
    }
    assert_eq!(registry.lock().entries.len(), MAX_STORES);
    assert!(registry.lock().get(&ids[0]).is_none(), "the oldest was dropped");
    let again = rt.block_on(registry.get(&db, &ids[0])).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&first, &again), "and is built again on its next use");
    assert!(registry.lock().get(&ids[1]).is_none(), "which pushed out the next oldest");
}

/// #380: a course whose manifest names another course's id does not
/// open (or get cached) under either id.
#[test]
fn a_course_claiming_another_id_does_not_open() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let db = crate::db::Db::open_in_memory().unwrap();
    let registry = Courses::empty();
    let id = "mine-abc123";
    let err = db
        .with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            create_course(c, uid, id, "Mine", &starter_files("spanish", "Mine"))?;
            check_here(c, &registry, id)
        })
        .unwrap();
    assert!(err.unwrap().contains("must be “mine-abc123”"));
    assert!(rt.block_on(registry.get(&db, id)).unwrap().is_none());
    assert!(registry.lock().get(id).is_none(), "not cached");
    assert!(rt.block_on(registry.get(&db, "spanish")).unwrap().is_none());
    // Its own id again: it opens and is cached.
    db.with(|c| write_file(c, id, MANIFEST_FILE, &starter_files(id, "Mine")[0].content)).unwrap();
    assert_eq!(rt.block_on(registry.get(&db, id)).unwrap().unwrap().id(), id);
    assert!(registry.lock().get(id).is_some());
}

/// #32, #5: loading the registry builds every built-in course once and
/// keeps the seed course in the cache, so its first `get` builds nothing;
/// another built-in is built on its first `get`, and a course the cache
/// dropped is built again from its folder.
#[test]
fn builtin_courses_stay_built_from_boot() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let db = crate::db::Db::open_in_memory().unwrap();
    let registry = Courses::load_builtin(Path::new("data")).unwrap();
    let summary = registry.builtin_by_id("spanish").expect("spanish is listed");
    assert!(registry.is_builtin("spanish"));
    assert!(registry.builtin().len() > 1);
    let cached: Vec<String> = registry.lock().entries.keys().cloned().collect();
    assert_eq!(cached, [super::SEED_COURSE], "boot keeps the seed course built, only it");

    let built = rt.block_on(registry.get(&db, "spanish")).unwrap().expect("spanish builds");
    assert_eq!(registry.rebuilds(), 0, "the first request is served from what boot built");
    assert_eq!(summary.totals(), built.totals());
    let levels: Vec<(usize, usize)> = built.start_units().into_iter().map(|u| (u, built.lessons_in(u).count())).collect();
    assert_eq!(summary.levels(), levels.as_slice());
    let second = &built.units()[levels[1].0].id;
    assert_eq!(summary.start_index(second), built.start_index(second));
    assert_eq!(summary.placement_offered(), crate::placement::offered(&built));
    let again = rt.block_on(registry.get(&db, "spanish")).unwrap().unwrap();
    assert!(Arc::ptr_eq(&built, &again), "the second request is served from the cache");

    registry.invalidate("spanish");
    assert!(registry.lock().get("spanish").is_none());
    let rebuilt = rt.block_on(registry.get(&db, "spanish")).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&built, &rebuilt), "dropped from the cache, it is built again");
    assert_eq!(registry.rebuilds(), 1);
    assert!(rt.block_on(registry.get(&db, "french")).unwrap().is_some());
    assert_eq!(registry.rebuilds(), 2, "a built-in boot did not keep is built on first use");
    assert!(rt.block_on(registry.get(&db, "no-such-course")).unwrap().is_none());
}

/// The course under `data/test-courses/`.
const TEST_COURSE: &str = "test-course";

/// A runtime like the server's: one async worker (`main.rs`).
fn one_worker() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(1).max_blocking_threads(2).enable_all().build().unwrap()
}

/// The test course (small and quick to build), dropped from the cache so
/// the next `get` builds it, with `hook` run before each build.
fn test_course_registry(hook: impl Fn(&str) + Send + Sync + 'static) -> Arc<Courses> {
    let registry = Courses::load(Path::new("data"), true).unwrap().with_build_hook(hook);
    registry.invalidate(TEST_COURSE);
    Arc::new(registry)
}

/// #5: a slow build of one course does not hold up a request for another
/// course (here a user-made one, which reads the database and builds too).
#[test]
fn a_slow_build_does_not_block_other_requests() {
    let rt = one_worker();
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        create_course(c, uid, "mine-abc123", "Mine", &starter_files("mine-abc123", "Mine"))
    })
    .unwrap();
    let registry = test_course_registry(|_| std::thread::sleep(std::time::Duration::from_secs(2)));
    rt.block_on(async {
        let slow = tokio::spawn({
            let (registry, db) = (Arc::clone(&registry), db.clone());
            async move { registry.get(&db, TEST_COURSE).await.unwrap() }
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let started = std::time::Instant::now();
        let other = registry.get(&db, "mine-abc123").await.unwrap();
        assert!(other.is_some());
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "waited {:?}", started.elapsed());
        assert!(!slow.is_finished(), "the slow build is still running");
        assert!(slow.await.unwrap().is_some());
    });
}

/// #5: two requests for a course not built yet build it once; both get it.
#[test]
fn two_first_requests_build_once() {
    let rt = one_worker();
    let db = crate::db::Db::open_in_memory().unwrap();
    let registry = test_course_registry(|_| std::thread::sleep(std::time::Duration::from_millis(300)));
    let (a, b) = rt.block_on(async {
        let get = || {
            let (registry, db) = (Arc::clone(&registry), db.clone());
            tokio::spawn(async move { registry.get(&db, TEST_COURSE).await.unwrap().unwrap() })
        };
        let (a, b) = (get(), get());
        (a.await.unwrap(), b.await.unwrap())
    });
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(registry.rebuilds(), 1);
}

/// #5: a build that panics answers like one that fails (no course), and
/// lets the lock go: the next request builds it.
#[test]
fn a_panicking_build_lets_the_lock_go() {
    let rt = one_worker();
    let db = crate::db::Db::open_in_memory().unwrap();
    let first = std::sync::atomic::AtomicBool::new(true);
    let registry = test_course_registry(move |_| {
        if first.swap(false, Ordering::Relaxed) {
            panic!("a build that panics (on purpose, in a test)");
        }
    });
    let course = rt.block_on(async {
        let failed = tokio::time::timeout(std::time::Duration::from_secs(10), registry.get(&db, TEST_COURSE)).await;
        assert!(failed.expect("answers").unwrap().is_none());
        tokio::time::timeout(std::time::Duration::from_secs(10), registry.get(&db, TEST_COURSE)).await
    });
    assert!(course.expect("not stuck on the lock").unwrap().is_some());
    assert_eq!(registry.rebuilds(), 1, "counted after the hook: only the second build");
}

/// #84: `record` writes what `check` found only if the files are still the
/// ones that were checked; a save that landed in between records its own result.
#[test]
fn a_stale_check_records_nothing() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let registry = Courses::empty();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let id = "stale-abc123";
        create_course(c, uid, id, "Old title", &starter_files(id, "Old title"))?;
        let files = load_files(c, id)?;
        let version = version_of(&files);
        let checked = check(id, &files);
        assert!(matches!(checked, Checked::Built(_)));
        // Another save lands before the result is recorded.
        write_file(c, id, "lessons/extra.json", "{}")?;
        assert_eq!(record(c, &registry, id, &version, checked)?, None);
        assert!(registry.lock().get(id).is_none(), "the old build is not cached");
        // A failure is not recorded either, though it is still reported.
        let version = version_of(&load_files(c, id)?);
        write_file(c, id, "lessons/extra.json", "{ other")?;
        assert_eq!(record(c, &registry, id, &version, Checked::Failed("nope".into()))?, Some("nope".into()));
        assert!(course(c, id)?.unwrap().error.is_none());
        // With the files unchanged it is recorded.
        let version = version_of(&load_files(c, id)?);
        assert_eq!(record(c, &registry, id, &version, Checked::Failed("nope".into()))?, Some("nope".into()));
        assert_eq!(course(c, id)?.unwrap().error.as_deref(), Some("nope"));
        Ok(())
    })
    .unwrap();
}

/// #84: the build thread works in the request's language, not the default.
#[test]
fn the_check_runs_in_the_requests_language() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = Arc::clone(&seen);
    let hook: BuildHook = Arc::new(move |_| log.lock().unwrap().push(crate::i18n::lang().to_string()));
    CHECK_HOOK.lock().unwrap().push(("lang-abc123".into(), hook));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let files = starter_files("lang-abc123", "Lang");
    let checked = crate::i18n::in_lang("es", || rt.block_on(check_off_lock("lang-abc123".into(), files)));
    CHECK_HOOK.lock().unwrap().retain(|(id, _)| id != "lang-abc123");
    assert!(matches!(checked, Checked::Built(_)));
    assert_eq!(*seen.lock().unwrap(), vec!["es".to_string()]);
}

/// `check` and `record` in one go, on a connection the test already holds.
fn check_here(c: &Connection, registry: &Courses, id: &str) -> rusqlite::Result<Option<String>> {
    let files = load_files(c, id)?;
    record(c, registry, id, &version_of(&files), check(id, &files))
}
