use super::*;
use crate::db::Db;

/// One learner with a row in every user table (and a course and a theme
/// with files and an undo step).
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
             INSERT INTO user_api_keys (user_id, provider, ciphertext, updated_at) VALUES ({uid}, 'anthropic', x'deadbeef', 1);
             INSERT INTO subscriptions (user_id, customer_id, subscription_id, status, event_at, updated_at) VALUES ({uid}, 'cus_{name}', 'sub_{name}', 'canceled', 1, 1);
             INSERT INTO plan_grants (user_id, granted_at) VALUES ({uid}, 1);
             INSERT INTO plan_cancellations (user_id, plan, reason, feedback, created_at) VALUES ({uid}, 'plus', 'price', 'too dear', 1);
             INSERT INTO ai_usage (user_id, purpose, provider, model, key_source, created_at) VALUES ({uid}, 'tutor-turn', 'anthropic', 'm', 'hosted', 1);
             INSERT INTO ai_credit_purchases (session_id, user_id, credit_micros, created_at) VALUES ('cs_{name}', {uid}, 1, 1);
             INSERT INTO terms_acceptances (user_id, version, accepted_at) VALUES ({uid}, 'v1', 1);
             INSERT INTO consents (user_id, purpose, granted_at, withdrawn_at) VALUES ({uid}, 'ai', 1, 2);
             INSERT INTO terms_announcements (user_id, version, mailed_at) VALUES ({uid}, 'v2', 1);
             INSERT INTO breach_notices (user_id, notice, mailed_at) VALUES ({uid}, '{name}-noticehash', 1);
             INSERT INTO mail_outbox (user_id, to_addr, subject, body_text, purpose, next_attempt_at, created_at) VALUES ({uid}, '{name}@example.org', '{name}-subject', '{name}-mailbody', 'login-code', 1, 1);
             INSERT INTO sms_outbox (user_id, to_number, body_text, purpose, next_attempt_at, created_at) VALUES ({uid}, '+447700900123', '{name}-text', 'login-code', 1, 1);
             INSERT INTO sms_sends (user_id, number_key, kind, created_at) VALUES ({uid}, '{name}-numberkey', 'other', 1);
             INSERT INTO email_verifications (user_id, email, token_hash, created_at, expires_at) VALUES ({uid}, '{name}@example.org', '{name}-vhash', 1, 2);
             INSERT INTO password_resets (user_id, email, token_hash, created_at, expires_at) VALUES ({uid}, '{name}@example.org', '{name}-rhash', 1, 2);
             INSERT INTO totp_keys (user_id, secret, created_at, confirmed_at) VALUES ({uid}, '{name}-totpsecret', 1, 2);
             INSERT INTO phone_verifications (user_id, phone, code_hash, salt, created_at, expires_at) VALUES ({uid}, '+447700900123', '{name}-phash', '{name}-psalt', 1, 2);
             INSERT INTO two_factor_challenges (user_id, token_hash, method, code_hash, salt, created_at, expires_at) VALUES ({uid}, '{name}-chtoken', 'email', '{name}-chcode', '{name}-chsalt', 1, 2);
             INSERT INTO two_factor_confirmations (user_id, method, code_hash, salt, created_at, expires_at) VALUES ({uid}, 'email', '{name}-cfcode', '{name}-cfsalt', 1, 2);
             UPDATE users SET phone = '+447700900123', two_factor = 'totp' WHERE id = {uid};
             INSERT INTO courses (id, owner_id, title, created_at, updated_at, notes) VALUES ('{course}', {uid}, 'T', 1, 1, '{name}-notes');
             INSERT INTO course_files (course_id, path, content, updated_at) VALUES ('{course}', 'course.json', '{{}}', 1);
             INSERT INTO course_undo (course_id, seq, title, label, created_at) VALUES ('{course}', 1, 'T', 'l', 1);
             INSERT INTO course_undo_files (course_id, seq, path, content) VALUES ('{course}', 1, 'course.json', '{{}}');
             INSERT INTO course_ai_runs (course_id, label, created_at) VALUES ('{course}', 'l', 1);
             INSERT INTO course_ai_run_files (course_id, side, path, content) VALUES ('{course}', 'after', 'course.json', '{{}}');
             INSERT INTO course_shares (course_id, email, token_hash, created_at) VALUES ('{course}', '{name}-friend@example.org', '{name}-shash', 1);
             INSERT INTO course_flags (course_id, state, matches_json, content_hash, checked_at) VALUES ('{course}', 'flagged', '[]', '{name}-flaghash', 1);
             INSERT INTO content_scores (course_id, chunk, hash, model, scanned_at) VALUES ('{course}', 'lessons/a.json', '{name}-scorehash', 'jev', 1);
             INSERT INTO content_category_scores (course_id, chunk, category, score) VALUES ('{course}', 'lessons/a.json', 'hate', 0.1);
             INSERT INTO content_checks (course_id, checked_at) VALUES ('{course}', 1);
             INSERT INTO projects (user_id, course_id, created_at, last_opened_at) VALUES ({uid}, 'spanish', 1, 1), ({uid}, '{course}', 1, 1);
             INSERT INTO user_themes (owner_id, title, created_at, updated_at) VALUES ({uid}, 'th', 1, 1);
             INSERT INTO user_theme_files (theme_id, path, content) VALUES (last_insert_rowid(), 'theme.json', '{{}}');
             INSERT INTO data_exports (user_id, state, token_hash, file, created_at) VALUES ({uid}, 'ready', '{name}-exporthash', '{name}-export.json', 1);
             INSERT INTO signup_invites (token_hash, created_by, created_at, expires_at) VALUES ('{name}-invitehash', {uid}, 1, 2);
             INSERT INTO waitlist (email, joined_at, confirmed_at, signed_up_at, user_id) VALUES ('{name}-waitlist@example.org', 1, 1, 2, {uid});
             INSERT INTO content_reports (user_id, course_id, reason, email, details, created_at) VALUES ({uid}, 'someone-course', 'harmful', '{name}@example.org', '{name}-report', 1);
             INSERT INTO privacy_actions (superuser_id, action, reason, created_at) VALUES ({uid}, 'export', '{name}-reason', 1);
             UPDATE users SET active_course = '{course}' WHERE id = {uid};"
    );
    c.execute_batch(&sql).unwrap();
    uid
}

fn tables(c: &Connection) -> Vec<String> {
    let mut stmt = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

fn count(c: &Connection, t: &str) -> i64 {
    c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap()
}

/// Every table of the schema is a user table listed in `USER_TABLES`
/// (or a server-wide one in `SHARED_TABLES`): /// it is deleted.
#[test]
fn every_table_is_covered() {
    let db = Db::open_in_memory().unwrap();
    let listed: Vec<&str> = USER_TABLES.iter().map(|(t, _)| *t).collect();
    db.with(|c| {
        for t in tables(c).into_iter().filter(|t| !SHARED_TABLES.contains(&t.as_str())) {
            assert!(listed.contains(&t.as_str()), "table {t} is not in privacy::USER_TABLES (deletion, docs/data-inventory.md)");
        }
        Ok(())
    })
    .unwrap();
}

/// Deleting ann removes exactly her rows from every table, and bob's stay.
#[test]
fn deleting_an_account_removes_every_row_of_it() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = fill(c, "ann");
        let bob = fill(c, "bob");
        let before: Vec<(String, i64)> = tables(c).into_iter().filter(|t| !SHARED_TABLES.contains(&t.as_str())).map(|t| (t.clone(), count(c, &t))).collect();
        for (t, n) in &before {
            assert!(*n >= 2, "the fixture has no rows in {t}: add them so deletion is tested there");
        }
        let deleted = delete_account(c, ann)?.unwrap();
        assert_eq!(deleted.courses, vec!["ann-course".to_string()]);
        for (t, n) in &before {
            let want = if KEPT_ON_DELETE.contains(&t.as_str()) { *n } else { n / 2 };
            assert_eq!(count(c, t), want, "rows left in {t} after deleting ann");
        }
        for (t, filter) in USER_TABLES {
            let left: i64 = c.query_row(&format!("SELECT COUNT(*) FROM {t} WHERE {filter}"), [ann], |r| r.get(0))?;
            assert_eq!(left, 0, "{t} still has ann's rows");
            let kept: i64 = c.query_row(&format!("SELECT COUNT(*) FROM {t} WHERE {filter}"), [bob], |r| r.get(0))?;
            assert!(kept > 0, "{t} lost bob's rows");
        }
        assert_eq!(delete_account(c, ann)?, None);
        Ok(())
    })
    .unwrap();
}

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
        assert_indexed(c, "UPDATE users SET theme = '' WHERE theme = ?1 AND theme <> ''", "users", Some("idx_users_theme"));
        // What `DELETE FROM users` cascades into (every other child is
        // covered by `db::tests::every_foreign_key_is_indexed`).
        assert_indexed(c, "DELETE FROM ai_credit_purchases WHERE user_id = ?1", "ai_credit_purchases", Some("idx_ai_credit_purchases_user"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_age_gate_refuses_an_unticked_box_and_records_a_ticked_one() {
    let gate = AgeGate { min_age: 16 };
    assert!(gate.check(true, true, "register").is_ok());
    assert_eq!(gate.check(false, false, "register").unwrap_err(), "Confirm you are at least 16 and accept the Terms to register.");
    assert_eq!(gate.check(false, true, "continue").unwrap_err(), "Confirm you are at least 16 to continue.");
    assert_eq!(gate.check(true, false, "register").unwrap_err(), "Accept the Terms to register.");
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let id = crate::auth::create_user(c, "ann@example.org", "h")?;
        let read = || c.query_row("SELECT age_confirmed FROM users WHERE id = ?1", [id], |r| r.get::<_, bool>(0)).unwrap();
        assert!(!read());
        record_age_confirmed(c, id, true)?;
        assert!(read());
        Ok(())
    })
    .unwrap();
}

/// The minimum age is 14 everywhere (#976): the default gate names 14 in
/// its box and refuses a sign-up without it.
#[test]
fn the_default_minimum_age_is_14() {
    assert_eq!(crate::terms::DEFAULT_MIN_AGE, 14);
    let gate = AgeGate { min_age: crate::terms::DEFAULT_MIN_AGE };
    assert_eq!(crate::t1("terms.check.age", "min_age", gate.min_age), "I am at least 14 years old");
    assert_eq!(gate.check(false, true, "register").unwrap_err(), "Confirm you are at least 14 to register.");
    assert_eq!(gate.check(false, false, "register").unwrap_err(), "Confirm you are at least 14 and accept the Terms to register.");
}
