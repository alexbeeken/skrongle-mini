//! A learner's data as a whole: what the export holds (built by exports.rs),
//! account deletion, the retention rules the hourly housekeeping applies, and
//! the age confirmation at sign-up. What is stored where, and why, is in
//! `docs/data-inventory.md`; keep the two in step.

use rusqlite::types::ValueRef;
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};

/// Every table that holds a learner's rows, with the condition that selects
/// them (`?1` is the user id). Deleting the `users` row removes all of them
/// through `ON DELETE CASCADE`; the export reads them from here. A new table
/// with user data must be added here (the tests fail until it is).
pub const USER_TABLES: &[(&str, &str)] = &[
    ("users", "id = ?1"),
    ("sessions", "user_id = ?1"),
    ("user_progress", "user_id = ?1"),
    ("quiz_history", "user_id = ?1"),
    ("journey_progress", "user_id = ?1"),
    ("user_vocab", "user_id = ?1"),
    ("lesson_passes", "user_id = ?1"),
    ("game_scores", "user_id = ?1"),
    ("game_activity", "user_id = ?1"),
    ("activity_log", "user_id = ?1"),
    ("lesson_touches", "user_id = ?1"),
    ("lesson_notes", "user_id = ?1"),
    ("earnings", "user_id = ?1"),
    ("motivator_purchases", "user_id = ?1"),
    ("game_rounds", "user_id = ?1"),
    ("projects", "user_id = ?1"),
    ("user_api_keys", "user_id = ?1"),
    ("subscriptions", "user_id = ?1"),
    ("plan_grants", "user_id = ?1"),
    ("plan_cancellations", "user_id = ?1"),
    ("ai_usage", "user_id = ?1"),
    ("ai_usage_months", "user_id = ?1"),
    ("ai_credit_purchases", "user_id = ?1"),
    ("terms_acceptances", "user_id = ?1"),
    ("terms_announcements", "user_id = ?1"),
    ("breach_notices", "user_id = ?1"),
    ("consents", "user_id = ?1"),
    ("mail_outbox", "user_id = ?1"),
    ("sms_outbox", "user_id = ?1"),
    ("sms_sends", "user_id = ?1"),
    ("email_verifications", "user_id = ?1"),
    ("password_resets", "user_id = ?1"),
    ("totp_keys", "user_id = ?1"),
    ("phone_verifications", "user_id = ?1"),
    ("two_factor_challenges", "user_id = ?1"),
    ("two_factor_confirmations", "user_id = ?1"),
    ("courses", "owner_id = ?1"),
    ("course_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_undo", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_undo_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_ai_runs", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_ai_run_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_shares", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1) OR user_id = ?1"),
    ("course_flags", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("content_scores", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("content_category_scores", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("content_checks", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("user_themes", "owner_id = ?1"),
    ("user_theme_files", "theme_id IN (SELECT id FROM user_themes WHERE owner_id = ?1)"),
    ("data_exports", "user_id = ?1"),
    ("signup_invites", "created_by = ?1 OR used_by = ?1"),
    ("content_reports", "user_id = ?1"),
    ("privacy_actions", "superuser_id = ?1"),
    ("waitlist", "user_id = ?1"),
];

/// User tables whose rows outlive the account: deleting it sets their
/// `user_id` to NULL instead (`ON DELETE SET NULL`). `sms_sends` is
/// the leftover texts' rate-limit ledger; its table goes with the schema
/// squash. `privacy_actions` is the log of privacy requests
/// answered (#981): a superuser's rows stay, without their id.
#[cfg_attr(not(test), allow(dead_code))]
pub const KEPT_ON_DELETE: &[&str] = &["sms_sends", "privacy_actions"];

/// Tables that hold no learner's rows, only server-wide figures:
/// `ai_spend_days` is everyone's hosted AI spend per day (the daily cap,
/// #623). Not exported; an account's deletion only lowers it.
/// `blocked_ips` and `content_check_worker` are unused tables (#9), tied to no account; they go with the schema squash (#2).
/// `jobs` is the background job queue (jobs.rs, #872): housekeeping rows, no account.
/// `release_settings` is the waitlist switch (#934).
#[cfg_attr(not(test), allow(dead_code))]
pub const SHARED_TABLES: &[&str] = &["ai_spend_days", "blocked_ips", "jobs", "content_check_worker", "release_settings"];

/// What the export includes: per user table, the columns it copies, and no
/// others. A column that is not listed is not exported, so a new column
/// cannot slip into the downloaded file by default; the test
/// `every_column_has_an_export_decision` fails until each column of a user
/// table is either here or in [`WITHHELD`] (#429).
pub const EXPORTED: &[(&str, &[&str])] = &[
    (
        "users",
        &[
            "id", "created_at", "google_sub", "email", "active_course", "first_name", "last_name", "avatar", "theme", "age_confirmed", "developer", "ai_off",
            "email_verified_at", "superuser", "avatar_color", "review_quiet", "phone", "phone_verified_at", "two_factor", "lang", "billing_ref", "stripe_customer", "motivator",
            "ai_credits", "signup_credit_at", "own_keys_fallback", "must_change_password", "phone_opted_out_at", "suspended_at", "beta", "delete_at",
            "ai_effort",
        ],
    ),
    ("sessions", &["id", "user_id", "created_at", "expires_at"]),
    ("user_progress", &["user_id", "course_id", "lesson_id", "mastery_score", "times_tested", "times_correct", "streak", "last_practiced_at", "read_at"]),
    ("quiz_history", &["id", "user_id", "lesson_id", "question_id", "selected_answer", "was_correct", "answered_at", "course_id"]),
    ("journey_progress", &["user_id", "course_id", "lesson_id", "step", "completed", "best_stars", "xp", "run_correct", "run_answered", "updated_at"]),
    ("user_vocab", &["user_id", "course_id", "lesson_id", "word", "learned_at", "times_seen", "times_known", "last_reviewed_at", "due_at", "gap_days"]),
    (
        "lesson_passes",
        &["user_id", "course_id", "lesson_id", "seen_at", "seen_via", "best_pct", "best_via", "passed_at", "passed_via", "runs", "evidence", "updated_at"],
    ),
    ("game_scores", &["id", "user_id", "game", "board", "score_ms", "wrong", "played_at", "course_id"]),
    ("game_activity", &["id", "user_id", "game", "board", "label", "duration_ms", "total", "correct", "score_ms", "played_at", "xp", "course_id"]),
    ("activity_log", &["id", "user_id", "course_id", "kind", "n", "at"]),
    ("lesson_touches", &["user_id", "course_id", "lesson_id", "mode", "at"]),
    ("lesson_notes", &["user_id", "course_id", "lesson_id", "body", "updated_at"]),
    ("earnings", &["user_id", "course_id", "points", "goals", "day", "day_points", "streak"]),
    ("motivator_purchases", &["user_id", "course_id", "item", "price", "created_at"]),
    ("game_rounds", &["user_id", "course_id", "game", "rounds"]),
    ("projects", &["user_id", "course_id", "created_at", "last_opened_at", "start_unit", "theme", "avatar", "avatar_color", "motivator", "set_up"]),
    ("user_api_keys", &["user_id", "provider", "updated_at"]),
    (
        "subscriptions",
        &["user_id", "customer_id", "subscription_id", "status", "price_id", "current_period_end", "cancel_at_period_end", "event_at", "updated_at", "plan"],
    ),
    ("plan_grants", &["user_id", "granted_at", "expires_at", "note"]),
    ("plan_cancellations", &["id", "user_id", "plan", "reason", "feedback", "created_at"]),
    // Skrongle AI metering (#299): what each model call cost, and the credit packs bought.
    (
        "ai_usage",
        &[
            "id", "user_id", "course_id", "job_id", "purpose", "provider", "model", "input_tokens", "output_tokens", "cached_tokens", "cost_micros", "credit_micros",
            "key_source", "created_at",
        ],
    ),
    ("ai_usage_months", &["user_id", "month", "cost_micros", "credit_micros", "hosted_calls", "own_calls"]),
    (
        "ai_credit_purchases",
        &["session_id", "user_id", "payment_intent", "price_id", "amount_cents", "credit_micros", "refunded_micros", "created_at"],
    ),
    ("terms_acceptances", &["user_id", "version", "accepted_at"]),
    ("terms_announcements", &["user_id", "version", "mailed_at"]),
    ("breach_notices", &["user_id", "mailed_at"]),
    ("consents", &["id", "user_id", "purpose", "granted_at", "withdrawn_at"]),
    // That a message was sent, when and to whom; never what it said.
    ("mail_outbox", &["id", "user_id", "to_addr", "purpose", "state", "attempts", "created_at", "sent_at", "provider"]),
    ("sms_outbox", &["id", "user_id", "to_number", "purpose", "state", "attempts", "created_at", "sent_at", "provider"]),
    // Security rows: what and when, never the token, code or secret.
    ("email_verifications", &["id", "user_id", "email", "created_at", "expires_at", "used_at"]),
    ("password_resets", &["id", "user_id", "email", "created_at", "expires_at", "used_at"]),
    ("totp_keys", &["user_id", "created_at", "confirmed_at"]),
    ("phone_verifications", &["id", "user_id", "phone", "created_at", "expires_at"]),
    ("two_factor_challenges", &["id", "user_id", "method", "created_at", "expires_at", "sent_by"]),
    ("two_factor_confirmations", &["user_id", "method", "created_at", "expires_at"]),
    ("courses", &["id", "owner_id", "title", "created_at", "updated_at", "error", "state", "source", "visibility", "job_id", "brief", "outline", "notes", "taken_down_at", "owner_emailed_at"]),
    ("course_files", &["course_id", "path", "content", "updated_at"]),
    ("course_undo", &["course_id", "seq", "title", "outline", "label", "created_at", "session", "started_at"]),
    ("course_undo_files", &["course_id", "seq", "path", "content"]),
    ("course_ai_runs", &["course_id", "label", "created_at", "excluded", "edits", "why"]),
    ("course_ai_run_files", &["course_id", "side", "path", "content"]),
    ("user_themes", &["id", "owner_id", "title", "created_at", "updated_at"]),
    ("user_theme_files", &["theme_id", "path", "content"]),
    ("course_shares", &["id", "course_id", "email", "user_id", "created_at", "opened_at"]),
    // The content check's flag on a course of theirs (#1033): what matched and where.
    ("course_flags", &["course_id", "state", "source", "matches_json", "reason", "checked_at", "override_note", "overridden_at", "terms_version", "ai_version"]),
    // The AI check's scores of each part of a course of theirs (#1036).
    ("content_scores", &["course_id", "chunk", "lesson", "high", "model", "input_tokens", "cost_usd", "review", "scanned_at", "ai_version"]),
    // The AI check's score of each part of a course of theirs per category (#1046).
    ("content_category_scores", &["course_id", "chunk", "category", "score"]),
    // When the content check last looked at a course of theirs (#1038).
    ("content_checks", &["course_id", "checked_at"]),
    ("data_exports", &["id", "user_id", "state", "bytes", "created_at", "ready_at", "expires_at", "downloaded_at"]),
    // The texts' rate-limit ledger (#362): when and which allowance, never the number.
    ("sms_sends", &["id", "user_id", "kind", "created_at"]),
    // Invitation links a superuser made, or the one this account came in by.
    ("signup_invites", &["id", "created_by", "created_at", "expires_at", "used_at", "used_by", "email"]),
    // The waitlist row this account signed up from (#934).
    ("waitlist", &["id", "email", "joined_at", "confirmed_at", "invite_id", "invited_at", "invites_sent", "signed_up_at", "user_id"]),
    // Courses this account reported and copyright notices it sent (#968).
    (
        "content_reports",
        &[
            "id", "user_id", "course_id", "course_title", "location", "reason", "email", "name", "work", "details", "good_faith", "created_at", "state", "forwarded_at",
            "resolved_at",
        ],
    ),    // What this account did as a superuser about privacy requests (#981).
    ("privacy_actions", &["id", "superuser_id", "action", "reason", "created_at"]),
];

/// Columns the export leaves out on purpose: secrets, security codes and
/// links, and the text of messages (which carried sign-in codes and
/// confirmation links). A session holder must not be able to read a proof
/// code meant to need the inbox or the phone (#361, #429).
#[allow(dead_code)] // the decision record: the test that keeps every column decided reads it
pub const WITHHELD: &[(&str, &[&str])] = &[
    ("users", &["password_hash"]),
    ("sessions", &["session_token"]),
    ("user_api_keys", &["ciphertext"]),
    ("mail_outbox", &["subject", "body_text", "body_html", "next_attempt_at", "scheduled_at", "last_error", "locked_by", "locked_until"]),
    ("sms_outbox", &["body_text", "next_attempt_at", "last_error", "locked_by", "locked_until"]),
    ("email_verifications", &["token_hash"]),
    ("password_resets", &["token_hash"]),
    ("course_shares", &["token_hash"]),
    // The hash of the checked files, and which superuser let a flagged course through.
    ("course_flags", &["content_hash", "overridden_by"]),
    // The hash of the text sent to Jev.
    ("content_scores", &["hash"]),
    ("signup_invites", &["token_hash"]),
    ("waitlist", &["confirm_hash"]),
    // Which notice text: a hash, the text itself is in no table.
    ("breach_notices", &["notice"]),
    ("sms_sends", &["number_key"]),
    ("totp_keys", &["secret", "last_step"]),
    ("phone_verifications", &["code_hash", "salt", "attempts"]),
    ("two_factor_challenges", &["token_hash", "code_hash", "salt", "attempts"]),
    ("two_factor_confirmations", &["code_hash", "salt", "attempts"]),
    // The download link's token hash and where the file sits on the server.
    ("data_exports", &["token_hash", "file", "link_base", "error"]),
];

/// The columns the export copies from `table` (empty for a table not listed).
pub fn exported_columns(table: &str) -> &'static [&'static str] {
    EXPORTED.iter().find(|(t, _)| *t == table).map(|(_, c)| *c).unwrap_or(&[])
}

/// What the export says about itself (its `about` field).
pub const EXPORT_ABOUT: &str = "Every row stored for your account, by table. Left out: your password hash, session tokens, the encrypted API keys (only which providers have one), e-mail confirmation links and sign-in, phone and two-factor codes (and their hashes), your authenticator app's secret, and the text of the e-mails and text messages sent to you (which mails and texts were sent, when and to where, is in). docs/data-inventory.md explains each table.";

/// Everything stored about `user_id`, as JSON: one array of rows per table
/// (course files included), with the [`EXPORTED`] columns only. `None` when
/// there is no such user.
#[cfg(test)] // the whole account in one value: the reference the paged export (exports.rs) is tested against
pub fn export(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Value>> {
    let Some(mut head) = export_header(conn, user_id)? else { return Ok(None) };
    let mut tables = Map::new();
    for (table, filter) in USER_TABLES {
        tables.insert(table.to_string(), Value::Array(rows(conn, table, filter, user_id)?));
    }
    head.insert("tables".into(), Value::Object(tables));
    Ok(Some(Value::Object(head)))
}

/// The export's fields besides `tables`; `None` when there is no such user.
pub fn export_header(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Map<String, Value>>> {
    let has_password: Option<bool> = conn
        .query_row("SELECT password_hash <> '' FROM users WHERE id = ?1", [user_id], |r| r.get(0))
        .map(Some)
        .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
    let Some(has_password) = has_password else { return Ok(None) };
    let mut providers = Vec::new();
    let mut stmt = conn.prepare("SELECT provider FROM user_api_keys WHERE user_id = ?1 ORDER BY provider")?;
    for p in stmt.query_map([user_id], |r| r.get::<_, String>(0))? {
        providers.push(p?);
    }
    let mut m = Map::new();
    m.insert("app".into(), json!(crate::APP_NAME));
    m.insert("exported_at".into(), json!(crate::db::now()));
    m.insert("user_id".into(), json!(user_id));
    m.insert("has_password".into(), json!(has_password));
    m.insert("api_key_providers".into(), json!(providers));
    m.insert("about".into(), json!(EXPORT_ABOUT));
    Ok(Some(m))
}

#[cfg(test)]
fn rows(conn: &Connection, table: &str, filter: &str, user_id: i64) -> rusqlite::Result<Vec<Value>> {
    let cols = exported_columns(table);
    if cols.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(&format!("SELECT {} FROM {table} WHERE {filter}", cols.join(", ")))?;
    let mut out = Vec::new();
    let mut q = stmt.query([user_id])?;
    while let Some(r) = q.next()? {
        out.push(row_json(r, cols)?);
    }
    Ok(out)
}

/// Where a paged read of a table stands: the ordering key of the last row
/// read (empty before the first page).
pub type After = Vec<rusqlite::types::Value>;

/// One page of `table`'s rows for `user_id` (its [`USER_TABLES`] filter),
/// in primary-key order (rowid without one), after the row whose key is
/// `after` (empty: from the start): at most `max_rows`, and it stops after
/// the row that takes the text read past `max_bytes`, so one page of course
/// files stays small. Returns the rows and the key of the last one, to pass
/// as `after` for the next page. Keyset paging: each page starts at its key
/// in the index instead of skipping the rows before it (#625). The
/// background export (exports.rs) reads a table this way, a short
/// `Db::call` per page.
pub fn page(conn: &Connection, table: &str, user_id: i64, after: &[rusqlite::types::Value], max_rows: usize, max_bytes: usize) -> rusqlite::Result<(Vec<Value>, After)> {
    let cols = exported_columns(table);
    let Some((_, filter)) = USER_TABLES.iter().find(|(t, _)| *t == table) else { return Ok((Vec::new(), Vec::new())) };
    if cols.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let sql = page_sql(conn, table, filter, cols, !after.is_empty())?;
    let mut stmt = conn.prepare(&sql)?;
    let mut args: Vec<rusqlite::types::Value> = vec![(max_rows as i64).into(), user_id.into()];
    args.extend(after.iter().cloned());
    let mut q = stmt.query(rusqlite::params_from_iter(args))?;
    let (mut out, mut bytes, mut last) = (Vec::new(), 0usize, Vec::new());
    while let Some(r) = q.next()? {
        for i in 0..cols.len() {
            bytes += match r.get_ref(i)? {
                ValueRef::Text(t) | ValueRef::Blob(t) => t.len(),
                _ => 8,
            };
        }
        out.push(row_json(r, cols)?);
        last = (cols.len()..r.as_ref().column_count()).map(|i| r.get::<_, rusqlite::types::Value>(i)).collect::<Result<_, _>>()?;
        if bytes >= max_bytes {
            break;
        }
    }
    Ok((out, last))
}

/// The statement [`page`] runs: the exported columns, then the ordering
/// key; `?1` the page size, `?2` the user id, `?3`… the key to start after.
fn page_sql(conn: &Connection, table: &str, filter: &str, cols: &[&str], after: bool) -> rusqlite::Result<String> {
    let mut pk = conn.prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")?;
    let mut keys: Vec<String> = pk.query_map([table], |r| r.get(0))?.collect::<Result<_, _>>()?;
    if keys.is_empty() {
        keys.push("rowid".to_string());
    }
    let order = keys.join(", ");
    let mut filter = format!("({})", filter.replace("?1", "?2"));
    if after {
        let marks: Vec<String> = (0..keys.len()).map(|i| format!("?{}", i + 3)).collect();
        if keys.len() == 1 {
            filter.push_str(&format!(" AND {order} > {}", marks.join(", ")));
        } else {
            filter.push_str(&format!(" AND ({order}) > ({})", marks.join(", ")));
        }
    }
    Ok(format!("SELECT {}, {order} FROM {table} WHERE {filter} ORDER BY {order} LIMIT ?1", cols.join(", ")))
}

/// One row of `cols` as a JSON object.
pub fn row_json(r: &rusqlite::Row, cols: &[&str]) -> rusqlite::Result<Value> {
    let mut row = Map::new();
    for (i, name) in cols.iter().enumerate() {
        let v = match r.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => json!(n),
            ValueRef::Real(f) => json!(f),
            ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
            // No exported column keeps a blob; say how big one is if it ever does.
            ValueRef::Blob(b) => json!(format!("<{} bytes>", b.len())),
        };
        row.insert(name.to_string(), v);
    }
    Ok(Value::Object(row))
}

/// What deleting an account removed that lives in caches too.
#[derive(Debug, Default, PartialEq)]
pub struct Deleted {
    /// The user's courses (their cached `ContentStore`s must go).
    pub courses: Vec<String>,
    /// The user's themes, as worn ids (`user-<n>`).
    pub themes: Vec<String>,
    /// Files of the user's data exports still on disk (names in the
    /// exports folder, `exports.rs`): the caller deletes them.
    pub export_files: Vec<String>,
}

/// Delete the account and everything it owns, in one transaction. Every
/// user table cascades from `users(id)` (the courses' and themes' files
/// through their parent rows); the few rows that only name a course by id
/// (`projects`, `users.active_course`) and wearers of the user's themes are
/// cleared here. `None` when there was no such user.
pub fn delete_account(conn: &mut Connection, user_id: i64) -> rusqlite::Result<Option<Deleted>> {
    let tx = conn.transaction()?;
    let courses: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id FROM courses WHERE owner_id = ?1 ORDER BY id")?;
        let v = stmt.query_map([user_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        v
    };
    let themes: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id FROM user_themes WHERE owner_id = ?1 ORDER BY id")?;
        let v = stmt.query_map([user_id], |r| r.get::<_, i64>(0))?.map(|r| r.map(crate::themes::user_theme_id)).collect::<Result<_, _>>()?;
        v
    };
    let export_files: Vec<String> = {
        let mut stmt = tx.prepare("SELECT file FROM data_exports WHERE user_id = ?1 AND file <> '' ORDER BY id")?;
        let v = stmt.query_map([user_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        v
    };
    for c in &courses {
        tx.execute("DELETE FROM projects WHERE course_id = ?1", [c])?;
        tx.execute("UPDATE users SET active_course = NULL WHERE active_course = ?1", [c])?;
    }
    for t in &themes {
        // `theme <> ''` lets the partial `idx_users_theme` answer it; a
        // worn id is never empty.
        tx.execute("UPDATE users SET theme = '' WHERE theme = ?1 AND theme <> ''", [t])?;
    }
    // Recent verification mail outlives the account for its day (#395).
    crate::mail::keep_for_caps(&tx, user_id, crate::db::now())?;
    let n = tx.execute("DELETE FROM users WHERE id = ?1", [user_id])?;
    tx.commit()?;
    Ok((n > 0).then_some(Deleted { courses, themes, export_files }))
}

// ---------- retention ----------

/// `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` when unset.
pub const DEFAULT_FAILED_COURSE_DAYS: i64 = 30;

/// `SKRONGLE_RETAIN_HISTORY_DAYS` when unset (#381): two years of answers and
/// finished rounds, so a script answering for an account cannot keep its
/// rows forever, while a learner's streak and history stay whole.
pub const DEFAULT_HISTORY_DAYS: i64 = 730;

/// How long data is kept, from the environment. The periods are
/// placeholders until the owner decides them (TODO.md, "Decide the privacy
/// facts").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    /// `SKRONGLE_RETAIN_HISTORY_DAYS` (default [`DEFAULT_HISTORY_DAYS`]):
    /// answers in `quiz_history` and finished rounds in `game_activity`
    /// older than this are deleted. `None` (0): keep them.
    pub history_days: Option<i64>,
    /// `SKRONGLE_KEEP_NOTES=1`: keep a finished AI course's notes (so revisions
    /// see the original material). Default: delete them once it is ready.
    pub keep_notes: bool,
    /// `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` (default 30): an AI course whose
    /// generation failed, untouched for this long, is deleted with its notes
    /// and files. `None` (0): keep them.
    pub failed_course_days: Option<i64>,
}

impl Retention {
    pub fn from_env() -> Retention {
        let days = |name: &str| crate::env(name).and_then(|v| v.trim().parse::<i64>().ok());
        Retention {
            history_days: Some(days("RETAIN_HISTORY_DAYS").unwrap_or(DEFAULT_HISTORY_DAYS)).filter(|d| *d > 0),
            keep_notes: crate::env("KEEP_NOTES").is_some_and(|v| v == "1" || v == "true"),
            failed_course_days: Some(days("FAILED_COURSE_RETENTION_DAYS").unwrap_or(DEFAULT_FAILED_COURSE_DAYS)).filter(|d| *d > 0),
        }
    }
}

/// What one sweep removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    /// Answers deleted from `quiz_history`.
    pub answers: usize,
    /// Finished rounds deleted from `game_activity`.
    pub rounds: usize,
    /// Finished AI courses whose notes were emptied.
    pub notes: usize,
    /// Failed AI courses deleted (their cached `ContentStore`s must go).
    pub failed_courses: Vec<String>,
}

/// Apply the retention rules: old answers and finished rounds go, the notes of AI courses whose
/// generation finished successfully are emptied (a failed one keeps them so
/// it can resume), and a failed course nobody resumed or edited within
/// `failed_course_days` is deleted, notes and all.
pub fn apply_retention(conn: &Connection, r: &Retention, now: i64) -> rusqlite::Result<Swept> {
    let (answers, rounds) = match r.history_days {
        Some(days) => {
            // The daily goal's log goes with them: it is read for today, a
            // run or the last week; the running totals are in `earnings`.
            conn.execute("DELETE FROM activity_log WHERE at < ?1", [now - days * 86_400])?;
            (
                conn.execute("DELETE FROM quiz_history WHERE answered_at < ?1", [now - days * 86_400])?,
                conn.execute("DELETE FROM game_activity WHERE played_at < ?1", [now - days * 86_400])?,
            )
        }
        None => (0, 0),
    };
    let notes = if r.keep_notes { 0 } else { conn.execute("UPDATE courses SET notes = '' WHERE state = 'ready' AND notes <> ''", [])? };
    let mut failed_courses = Vec::new();
    if let Some(days) = r.failed_course_days {
        let mut stmt = conn.prepare("SELECT id FROM courses WHERE state = 'failed' AND updated_at < ?1 ORDER BY id")?;
        failed_courses = stmt.query_map([now - days * 86_400], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
        for id in &failed_courses {
            crate::courses::delete_course(conn, id)?;
        }
    }
    Ok(Swept { answers, rounds, notes, failed_courses })
}

// ---------- age at sign-up ----------

/// The "I am at least N" confirmation at sign-up. It is part of the Terms
/// checkbox ("I am at least N and agree to the Terms and Privacy Policy",
/// #72), which registration requires, so
/// the gate is always on; N is `SKRONGLE_MIN_AGE` (`terms::min_age`, 14 by
/// default, #976). `check` refuses a sign-up without the
/// box, and `record_age_confirmed` stores it on the new account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgeGate {
    pub min_age: u32,
}

impl AgeGate {
    /// The gate as this server is configured.
    pub fn current() -> AgeGate {
        AgeGate { min_age: crate::terms::min_age() }
    }

    /// The sign-up forms' two boxes: "I am at least N years old" (`age`)
    /// and "I agree to the Terms" (`accept`). `Err` is the sentence for the
    /// form, naming the missing box; `to` ends it ("register", "continue").
    pub fn check(&self, age: bool, accept: bool, to: &str) -> Result<(), String> {
        let min_age = self.min_age;
        let to = match to {
            "register" => crate::t("auth.age_gate.to_register"),
            "continue" => crate::t("auth.age_gate.to_continue"),
            other => other.to_string(),
        };
        match (age, accept) {
            (true, true) => Ok(()),
            (false, false) => Err(crate::t2("auth.age_gate.both", "min_age", min_age, "to", to)),
            (false, true) => Err(crate::t2("auth.age_gate.age", "min_age", min_age, "to", to)),
            (true, false) => Err(crate::t1("auth.age_gate.terms", "to", to)),
        }
    }
}

/// Store the confirmation on the account (`users.age_confirmed`).
pub fn record_age_confirmed(conn: &Connection, user_id: i64, confirmed: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET age_confirmed = ?2 WHERE id = ?1", params![user_id, confirmed])?;
    Ok(())
}

#[cfg(test)]
mod tests;
