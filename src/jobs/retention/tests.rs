use super::*;
use crate::db::Db;
use rusqlite::params;

/// One learner with a row in every user table (and a course with files
/// and an undo step).
fn fill(c: &Connection, name: &str) -> i64 {
    let uid = crate::auth::create_user(c, &format!("{name}@example.org"), "$argon2id$secret-hash").unwrap();
    let course = format!("{name}-course");
    let token = format!("{name}-token");
    let sql = format!(
        "INSERT INTO sessions (user_id, session_token, created_at, expires_at) VALUES ({uid}, '{token}', 1, 9999999999);
             INSERT INTO user_progress (user_id, course_id, lesson_id, mastery_score) VALUES ({uid}, 'spanish', 'a1', 50);
             INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES ({uid}, 'spanish', 'a1', 'q1', '{name}-answer', 1, 100);
             INSERT INTO journey_progress (user_id, course_id, lesson_id, updated_at) VALUES ({uid}, 'spanish', 'a1', 1);
             INSERT INTO user_vocab (user_id, course_id, lesson_id, word, learned_at) VALUES ({uid}, 'spanish', 'a1', 'hola', 1);
             INSERT INTO lesson_passes (user_id, course_id, lesson_id, updated_at) VALUES ({uid}, 'spanish', 'a1', 1);
             INSERT INTO game_scores (user_id, course_id, game, board, score_ms, played_at) VALUES ({uid}, 'spanish', 'matching', 'all', 1000, 1);
             INSERT INTO game_activity (user_id, course_id, game, board, label, duration_ms, total, correct, played_at) VALUES ({uid}, 'spanish', 'matching', 'all', 'x', 1, 1, 1, 1);
             INSERT INTO game_rounds (user_id, course_id, game, rounds) VALUES ({uid}, 'spanish', 'matching', 1);
             INSERT INTO activity_log (user_id, course_id, kind, n, at) VALUES ({uid}, 'spanish', 'answer', 1, 1);
             INSERT INTO lesson_touches (user_id, course_id, lesson_id, mode, at) VALUES ({uid}, 'spanish', 'a1', 'study', 1);
             INSERT INTO lesson_notes (user_id, course_id, lesson_id, body, updated_at) VALUES ({uid}, 'spanish', 'a1', '{name}-note', 1);
             INSERT INTO earnings (user_id, course_id, points, goals, day, day_points, streak) VALUES ({uid}, 'spanish', 1, 0, 0, 1, 1);
             INSERT INTO motivator_purchases (user_id, course_id, item, price, created_at) VALUES ({uid}, 'spanish', 'sprout', 50, 1);
             INSERT INTO totp_keys (user_id, secret, created_at, confirmed_at) VALUES ({uid}, '{name}-totpsecret', 1, 2);
             INSERT INTO two_factor_challenges (user_id, token_hash, created_at, expires_at) VALUES ({uid}, '{name}-chtoken', 1, 2);
             UPDATE users SET two_factor = 'totp' WHERE id = {uid};
             INSERT INTO courses (id, owner_id, title, created_at, updated_at, notes) VALUES ('{course}', {uid}, 'T', 1, 1, '{name}-notes');
             INSERT INTO course_files (course_id, path, content, updated_at) VALUES ('{course}', 'course.json', '{{}}', 1);
             INSERT INTO course_undo (course_id, seq, title, label, created_at) VALUES ('{course}', 1, 'T', 'l', 1);
             INSERT INTO course_undo_files (course_id, seq, path, content) VALUES ('{course}', 1, 'course.json', '{{}}');
             INSERT INTO course_ai_runs (course_id, label, created_at) VALUES ('{course}', 'l', 1);
             INSERT INTO course_ai_run_files (course_id, side, path, content) VALUES ('{course}', 'after', 'course.json', '{{}}');
             INSERT INTO course_shares (course_id, email, token_hash, created_at) VALUES ('{course}', '{name}-friend@example.org', '{name}-shash', 1);
             INSERT INTO projects (user_id, course_id, created_at, last_opened_at) VALUES ({uid}, 'spanish', 1, 1), ({uid}, '{course}', 1, 1);
             INSERT INTO signup_invites (token_hash, created_by, created_at, expires_at) VALUES ('{name}-invitehash', {uid}, 1, 2);
             UPDATE users SET active_course = '{course}' WHERE id = {uid};"
    );
    c.execute_batch(&sql).unwrap();
    uid
}
fn count(c: &Connection, t: &str) -> i64 {
    c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap()
}
#[test]
fn retention_drops_old_answers_rounds_and_finished_notes() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = fill(c, "ann");
        c.execute(
            "INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (?1, 'spanish', 'a1', 'q2', 'new', 1, ?2)",
            params![ann, 100 + 40 * 86_400],
        )?;
        c.execute(
            "INSERT INTO game_activity (user_id, course_id, game, board, label, duration_ms, total, correct, played_at) VALUES (?1, 'spanish', 'matching', 'all', 'new', 1, 1, 1, ?2)",
            params![ann, 100 + 40 * 86_400],
        )?;
        c.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at, state, notes) VALUES ('failed', ?1, 'F', 1, 1, 'failed', 'resume me')", [ann])?;
        let now = 100 + 45 * 86_400;
        // Default: history kept, notes of ready courses removed.
        let keep = Retention { history_days: None, keep_notes: false, failed_course_days: None };
        assert_eq!(apply_retention(c, &keep, now)?, Swept { answers: 0, rounds: 0, notes: 1, failed_courses: vec![] });
        let notes = |id: &str| c.query_row("SELECT notes FROM courses WHERE id = ?1", [id], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!((notes("ann-course"), notes("failed")), (String::new(), "resume me".to_string()));
        assert_eq!(count(c, "quiz_history"), 2);
        let month = Retention { history_days: Some(30), keep_notes: true, failed_course_days: None };
        assert_eq!(apply_retention(c, &month, now)?, Swept { answers: 1, rounds: 1, notes: 0, failed_courses: vec![] });
        let left: String = c.query_row("SELECT selected_answer FROM quiz_history", [], |r| r.get(0))?;
        assert_eq!(left, "new");
        let round: String = c.query_row("SELECT label FROM game_activity", [], |r| r.get(0))?;
        assert_eq!(round, "new");
        Ok(())
    })
    .unwrap();
}

/// A failed AI course goes once it has sat untouched for the retention
/// period, with its notes and files and the references to it; a newer
/// failed one and a ready one stay.
#[test]
fn retention_deletes_old_failed_courses() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = fill(c, "ann");
        let day = 86_400;
        let now = 100 * day;
        for (id, state, updated) in [("old-failed", "failed", now - 31 * day), ("new-failed", "failed", now - 29 * day), ("old-ready", "ready", now - 90 * day)] {
            c.execute(
                "INSERT INTO courses (id, owner_id, title, created_at, updated_at, state, source, notes) VALUES (?1, ?2, 'T', 1, ?3, ?4, 'ai', 'my notes')",
                params![id, ann, updated, state],
            )?;
            c.execute("INSERT INTO course_files (course_id, path, content, updated_at) VALUES (?1, 'course.json', '{}', 1)", [id])?;
            c.execute("INSERT INTO projects (user_id, course_id, created_at, last_opened_at) VALUES (?1, ?2, 1, 1)", params![ann, id])?;
        }
        c.execute("UPDATE users SET active_course = 'old-failed' WHERE id = ?1", [ann])?;
        let exists = |id: &str| c.query_row("SELECT COUNT(*) FROM courses WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).unwrap() == 1;

        let off = Retention { history_days: None, keep_notes: true, failed_course_days: None };
        assert_eq!(apply_retention(c, &off, now)?.failed_courses, Vec::<String>::new());
        assert!(exists("old-failed"));

        let r = Retention { history_days: None, keep_notes: true, failed_course_days: Some(DEFAULT_FAILED_COURSE_DAYS) };
        assert_eq!(apply_retention(c, &r, now)?.failed_courses, vec!["old-failed".to_string()]);
        assert!(!exists("old-failed") && exists("new-failed") && exists("old-ready"));
        let left = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(left("SELECT COUNT(*) FROM course_files WHERE course_id = 'old-failed'"), 0);
        assert_eq!(left("SELECT COUNT(*) FROM projects WHERE course_id = 'old-failed'"), 0);
        assert_eq!(left("SELECT COUNT(*) FROM users WHERE active_course = 'old-failed'"), 0);
        assert_eq!(apply_retention(c, &r, now)?, Swept::default());
        Ok(())
    })
    .unwrap();
}

/// The hourly sweep and account deletion find their rows through an
/// index, so they stay quick however many learners and answers there
/// are (#614–#617, #620).
#[test]
fn retention_and_account_deletion_use_indexes() {
    use crate::db::plan::assert_indexed;
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        assert_indexed(c, "DELETE FROM activity_log WHERE at < ?1", "activity_log", Some("idx_activity_log_at"));
        assert_indexed(c, "DELETE FROM quiz_history WHERE answered_at < ?1", "quiz_history", Some("idx_history_answered"));
        assert_indexed(c, "DELETE FROM game_activity WHERE played_at < ?1", "game_activity", Some("idx_game_activity_played"));
        assert_indexed(c, "UPDATE courses SET notes = '' WHERE state = 'ready' AND notes <> ''", "courses", Some("idx_courses_with_notes"));
        assert_indexed(c, "SELECT id FROM courses WHERE state = 'failed' AND updated_at < ?1 ORDER BY id", "courses", Some("idx_courses_state_updated"));
        assert_indexed(c, "DELETE FROM projects WHERE course_id = ?1", "projects", Some("idx_projects_course"));
        assert_indexed(c, "UPDATE users SET active_course = NULL WHERE active_course = ?1", "users", Some("idx_users_active_course"));
        Ok(())
    })
    .unwrap();
}
