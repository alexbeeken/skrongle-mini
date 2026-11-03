//! User profiles: personal information and the account settings behind the
//! top-right menu.
//!
//! A profile never depends on a course: nothing here reads the active
//! course; practice in a course's language comes from the course, not the
//! profile.
//!
//! The e-mail address is set once (at registration, by Google sign-in, or on
//! the profile page for accounts that predate it) and never changes after
//! that; everything else here can be edited at any time.

use rusqlite::{params, Connection, OptionalExtension};

// ---------- validation ----------

pub const NAME_MAX: usize = 60;
pub const EMAIL_MAX: usize = 254;
/// The most the part before the `@` may have (RFC 5321); `lettre` refuses more.
pub const EMAIL_LOCAL_MAX: usize = 64;

/// A first or last name: optional, up to `NAME_MAX` characters, no control
/// characters. Returns the trimmed value.
pub fn validate_name(label: &str, raw: &str) -> Result<String, String> {
    let v = raw.trim();
    if v.chars().count() > NAME_MAX {
        return Err(crate::t2("profile.name.too_long", "label", label, "max", NAME_MAX));
    }
    if v.chars().any(char::is_control) {
        return Err(crate::t1("profile.name.bad_chars", "label", label));
    }
    Ok(v.to_string())
}

/// A plausible e-mail address, normalised to lower case. Deliberately loose:
/// one `@`, something on both sides, a dot in the domain, no whitespace.
pub fn validate_email(raw: &str) -> Result<String, String> {
    let v = raw.trim().to_lowercase();
    if v.is_empty() {
        return Err(crate::t("profile.email.empty"));
    }
    if v.len() > EMAIL_MAX {
        return Err(crate::t1("profile.email.too_long", "max", EMAIL_MAX));
    }
    if v.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(crate::t("profile.email.spaces"));
    }
    let Some((local, domain)) = v.split_once('@') else {
        return Err(crate::t("profile.email.no_at"));
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return Err(crate::t("profile.email.not_email"));
    }
    if local.len() > EMAIL_LOCAL_MAX {
        return Err(crate::t1("profile.email.local_too_long", "max", EMAIL_LOCAL_MAX));
    }
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return Err(crate::t("profile.email.no_domain"));
    }
    // What we accept must be an address mail can be sent to (#1003).
    if v.parse::<lettre::Address>().is_err() {
        return Err(crate::t("profile.email.not_email"));
    }
    Ok(v)
}

// ---------- persistence (run inside Db::call) ----------

/// Everything the profile page shows about an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub first_name: String,
    pub last_name: String,
    /// `None` until the account has one; afterwards fixed.
    pub email: Option<String>,
    pub created_at: i64,
    /// Accounts without one (signed in through a proxy) have no password to change.
    pub has_password: bool,
    /// The Terms of Service version last accepted, and when.
    pub terms: Option<crate::terms::Acceptance>,
    /// "Hide all AI features" (`users.ai_off`).
    pub ai_off: bool,
    /// When the age box was ticked (`consent::age_confirmed_at`).
    pub age_confirmed_at: Option<i64>,
    /// The AI features consent's latest row (`consent::AI`), for its date.
    pub ai_consent: Option<crate::consent::Record>,
    /// The phone number and the second sign-in step (`two_factor.rs`).
    pub two_factor: crate::two_factor::Settings,
}

impl Profile {
    /// The "Do Not Show Me AI Features" row of the Privacy and consent card:
    /// shown or hidden, and since when if the record says.
    pub fn ai_consent_text(&self) -> String {
        ai_consent_text(self.ai_off, self.ai_consent)
    }

    /// The age row of the Privacy and consent card.
    pub fn age_text(&self) -> String {
        match self.age_confirmed_at {
            Some(t) => crate::t1("profile.consent.age_confirmed", "date", long_date(t)),
            None => crate::t("profile.consent.age_not_confirmed"),
        }
    }

    /// What a Google-only account types to confirm deleting itself: its
    /// address (Google always gives one), or "delete" should it have none.
    pub fn delete_phrase(&self) -> String {
        self.email.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| "delete".into())
    }
}

/// "Shown since 3 March 2026", "Hidden since …", or plain "Shown"/"Hidden" for an
/// account with no record (made before v82 with AI hidden, or by a script).
pub fn ai_consent_text(ai_off: bool, latest: Option<crate::consent::Record>) -> String {
    match (ai_off, latest) {
        (false, Some(r)) if r.withdrawn_at.is_none() => crate::t1("profile.consent.ai_on_since", "date", long_date(r.granted_at)),
        (true, Some(crate::consent::Record { withdrawn_at: Some(t), .. })) => crate::t1("profile.consent.ai_off_since", "date", long_date(t)),
        (false, _) => crate::t("profile.consent.ai_on"),
        (true, _) => crate::t("profile.consent.ai_off"),
    }
}

pub fn load(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Profile>> {
    let terms = crate::terms::latest(conn, user_id)?;
    let age_confirmed_at = crate::consent::age_confirmed_at(conn, user_id)?;
    let ai_consent = crate::consent::latest(conn, user_id, crate::consent::AI)?;
    let two_factor = match crate::two_factor::settings(conn, user_id) {
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        r => r?,
    };
    conn.query_row(
        "SELECT first_name, last_name, NULLIF(email, ''), created_at,
                password_hash != '', ai_off
         FROM users WHERE id = ?1",
        [user_id],
        |r| {
            Ok(Profile {
                first_name: r.get(0)?,
                last_name: r.get(1)?,
                email: r.get(2)?,
                created_at: r.get(3)?,
                has_password: r.get(4)?,
                terms: terms.clone(),
                ai_off: r.get(5)?,
                age_confirmed_at,
                ai_consent,
                two_factor: two_factor.clone(),
            })
        },
    )
    .optional()
}

/// Through `idx_users_email_lower` (#605), like every lookup by address.
const EMAIL_TAKEN_SQL: &str = "SELECT EXISTS(SELECT 1 FROM users WHERE lower(email) = lower(?1) AND id != ?2)";

/// Is this address already on another account? Case-insensitive.
pub fn email_taken(conn: &Connection, email: &str, except_user: Option<i64>) -> rusqlite::Result<bool> {
    conn.query_row(
        EMAIL_TAKEN_SQL,
        params![email, except_user.unwrap_or(-1)],
        |r| r.get(0),
    )
}

/// The account's address counts as confirmed (the dev user; tests). Keeps
/// the first confirmation time.
pub fn mark_verified(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET email_verified_at = COALESCE(email_verified_at, ?2) WHERE id = ?1", params![user_id, now])?;
    Ok(())
}

/// Save the editable part of a profile; `None` keeps a field as it is. The
/// names are already validated. (The picture and the motivator are a
/// course's settings, #565 and #574: `courses::set_project_settings`.)
pub fn update(conn: &Connection, user_id: i64, first_name: Option<&str>, last_name: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE users SET first_name = COALESCE(?2, first_name), last_name = COALESCE(?3, last_name) WHERE id = ?1",
        params![user_id, first_name, last_name],
    )?;
    Ok(())
}

pub fn password_hash(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT password_hash FROM users WHERE id = ?1", [user_id], |r| r.get(0)).optional()
}

/// The account's own new password: a temporary one the owner set
/// (`users.must_change_password`, #635) is replaced, so the gate lifts.
pub fn set_password_hash(conn: &Connection, user_id: i64, hash: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET password_hash = ?2, must_change_password = 0 WHERE id = ?1", params![user_id, hash])?;
    Ok(())
}

/// "Don't show this automatically next time" in the review of AI changes
/// (`users.review_quiet`, #187): the review waits for its button.
pub fn review_quiet(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    conn.query_row("SELECT review_quiet FROM users WHERE id = ?1", [user_id], |r| r.get(0)).optional().map(|q| q.unwrap_or(false))
}

pub fn set_review_quiet(conn: &Connection, user_id: i64, quiet: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET review_quiet = ?2 WHERE id = ?1", params![user_id, quiet])?;
    Ok(())
}

/// "Hide all AI features" (`users.ai_off`): set at sign-up or from the
/// profile's AI features section, and undone there. Each change is a
/// consent given or withdrawn (`consent::AI`, #974).
pub fn set_ai_off(conn: &Connection, user_id: i64, off: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET ai_off = ?2 WHERE id = ?1", params![user_id, off])?;
    crate::consent::record(conn, user_id, crate::consent::AI, !off)
}

/// `users.lang`: the language the app speaks to this account in ('' follows the browser).
pub fn set_lang(conn: &Connection, user_id: i64, lang: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET lang = ?2 WHERE id = ?1", params![user_id, lang])?;
    Ok(())
}


/// What a learner is called until they give a name ("Learner").
pub fn no_name() -> String {
    crate::t("profile.no_name")
}

/// What the navigation bar and greetings call someone: first name when
/// known, else "Learner".
pub fn short_name(first_name: &str) -> String {
    let f = first_name.trim();
    if f.is_empty() { no_name() } else { f.to_string() }
}

/// "First Last", or "Learner" when neither name is set.
pub fn display_name(first_name: &str, last_name: &str) -> String {
    let full = format!("{} {}", first_name.trim(), last_name.trim());
    let full = full.trim();
    if full.is_empty() { no_name() } else { full.to_string() }
}

/// How other learners see someone (leaderboards): "Ana L.", "Ana", or
/// "Learner" when no first name is set.
pub fn public_name(first_name: &str, last_name: &str) -> String {
    let f = first_name.trim();
    if f.is_empty() {
        return no_name();
    }
    match last_name.trim().chars().next() {
        Some(i) => format!("{f} {}.", i.to_uppercase()),
        None => f.to_string(),
    }
}

/// Month and year an account was created, for "member since".
pub fn member_since(created_at: i64) -> String {
    let (y, m, _) = civil(created_at);
    crate::t2("profile.date.month_year", "month", month_name(m), "year", y)
}

/// "16 October 2026", for dates such as a subscription's renewal.
pub fn long_date(t: i64) -> String {
    let (y, m, d) = civil(t);
    crate::i18n::fill(&crate::t("profile.date.long"), &[("day", &d), ("month", &month_name(m)), ("year", &y)])
}

/// The name of month `m` (1–12) in the learner's language.
fn month_name(m: usize) -> String {
    crate::t(match m {
        1 => "profile.month.january",
        2 => "profile.month.february",
        3 => "profile.month.march",
        4 => "profile.month.april",
        5 => "profile.month.may",
        6 => "profile.month.june",
        7 => "profile.month.july",
        8 => "profile.month.august",
        9 => "profile.month.september",
        10 => "profile.month.october",
        11 => "profile.month.november",
        _ => "profile.month.december",
    })
}

/// (year, month 1–12, day) of a Unix time, UTC.
pub(crate) fn civil(t: i64) -> (i64, usize, i64) {
    // Civil-from-days (Howard Hinnant's algorithm); no chrono dependency.
    let days = t.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as usize, d)
}

/// Days since the Unix epoch of a civil date (Howard Hinnant's days-from-civil).
pub(crate) fn days_from_civil(y: i64, m: usize, d: i64) -> i64 {
    let m = m as i64;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;

    #[test]
    fn email_taken_reads_through_the_index_in_any_case() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            assert_eq!(crate::db::table_scans(c, EMAIL_TAKEN_SQL).unwrap(), Vec::<String>::new());
            let ann = auth::create_user(c, "Ann@Example.org", "h")?;
            assert!(email_taken(c, "ann@example.org", None)?);
            assert!(email_taken(c, "ANN@example.org", Some(ann + 1))?);
            assert!(!email_taken(c, "ann@example.org", Some(ann))?, "their own address");
            assert!(!email_taken(c, "bob@example.org", None)?);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn validates_names() {
        assert_eq!(validate_name("First name", "  Ana ").unwrap(), "Ana");
        assert_eq!(validate_name("First name", "").unwrap(), "");
        assert!(validate_name("First name", &"x".repeat(61)).is_err());
        assert!(validate_name("Last name", "a\u{0}b").is_err());
    }

    #[test]
    fn validates_emails() {
        assert_eq!(validate_email(" Ana.Lopez@Example.ORG ").unwrap(), "ana.lopez@example.org");
        for bad in ["", "ana", "ana@", "@example.org", "ana@example", "ana@.org", "ana@ex..org", "a na@example.org", "a@b@c.org"] {
            assert!(validate_email(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn email_local_part_is_at_most_64() {
        let ok = format!("{}@example.org", "a".repeat(64));
        assert_eq!(validate_email(&ok).unwrap(), ok);
        assert!(validate_email(&format!("{}@example.org", "a".repeat(65))).is_err());
        assert!(validate_email("a-rather-long-owner-address-for-the-phone-tip-check-1027015107-11@example.com").is_err());
    }

    /// Whatever `validate_email` accepts, the mailer can address (#1003).
    #[test]
    fn accepted_emails_can_be_mailed() {
        let long = format!("{}@example.org", "x".repeat(64));
        let tries = [
            "ana@example.org", "Ana.Lopez+tag@Example.ORG", "o'brien@example.co.uk", "a_b-c@sub.example.org", long.as_str(),
            "a(b)@example.org", "a<b@example.org", "a\\b@example.org", "a\"b@example.org", "a,b@example.org", "é@example.org",
        ];
        for raw in tries {
            if let Ok(v) = validate_email(raw) {
                assert!(crate::mail::Mail::new(&v, "Hi", "text", "test").is_ok(), "{v:?} passed validation but cannot be mailed");
            }
        }
        assert!(validate_email("ana@example.org").is_ok());
        assert!(validate_email(&long).is_ok());
    }

    #[test]
    fn names_and_dates() {
        assert_eq!(short_name(""), "Learner");
        assert_eq!(short_name(" Ana "), "Ana");
        assert_eq!(display_name("", ""), "Learner");
        assert_eq!(display_name("", "López"), "López");
        assert_eq!(display_name("Ana", ""), "Ana");
        assert_eq!(display_name("Ana", "López"), "Ana López");
        assert_eq!(public_name("Ana", "López"), "Ana L.");
        assert_eq!(public_name("Ana", "élan"), "Ana É.");
        assert_eq!(public_name(" Ana ", ""), "Ana");
        assert_eq!(public_name("", "López"), "Learner");
        assert_eq!(member_since(0), "January 1970");
        assert_eq!(member_since(1_759_881_600), "October 2025"); // 2025-10-08
        assert_eq!(member_since(951_782_400), "February 2000"); // 2000-02-29
        assert_eq!(long_date(951_782_400), "29 February 2000");
        assert_eq!(long_date(1_759_881_600 + 86_399), "8 October 2025");
    }

    #[test]
    fn profile_roundtrip() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let id = auth::create_user(c, "ana@example.org", "h")?;
            let p = load(c, id)?.unwrap();
            assert_eq!(p.email.as_deref(), Some("ana@example.org"));
            assert!(p.has_password);

            update(c, id, Some("Ana"), Some("López"))?;
            // Fields left out keep their value.
            update(c, id, None, Some(""))?;
            let p = load(c, id)?.unwrap();
            assert_eq!((p.first_name.as_str(), p.last_name.as_str()), ("Ana", ""));
            assert!(!p.ai_off);
            set_ai_off(c, id, true)?;
            assert!(load(c, id)?.unwrap().ai_off);
            set_ai_off(c, id, false)?;
            assert!(!load(c, id)?.unwrap().ai_off);

            assert!(email_taken(c, "ANA@example.org", None)?);
            assert!(!email_taken(c, "ana@example.org", Some(id))?);
            assert!(!email_taken(c, "bob@example.org", None)?);
            assert_eq!(load(c, 999)?, None);
            Ok(())
        })
        .unwrap();
    }
}
