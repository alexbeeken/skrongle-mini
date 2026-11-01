//! Terms of Service: the text (`docs/terms.md`), its version, and who
//! accepted which version when (table `terms_acceptances`).
//!
//! A learner accepts at registration (the checkbox) and again whenever the version (`version()`) changes.
//!
//! The built-in text has a changelog, `docs/terms-changes.json` (#966): one
//! [`Change`] per version with its dates, whether it is material and a
//! summary. The current version is the newest change in effect. An account
//! that accepted an earlier one gets a notice on its pages ([`notice`],
//! `templates/terms_notice.html`): a minor change can be dismissed (that is
//! acceptance), a material one blocks until accepted. A material change
//! with a later effective date is e-mailed to every account once, when a
//! superuser presses Send on `/superuser` ([`announce_upcoming`], table
//! `terms_announcements`), and only takes effect on its date. An account that never accepted any version gets the
//! full-page gate, `/terms/accept`.
//!
//! With `SKRONGLE_TERMS=<path>` (Skrongle mini) the text is the
//! self-hoster's own Markdown file, whose SHA-256 is the version (#637): no
//! changelog, no summary, no e-mail, and any edit sends everyone to the gate.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::db::now;
use crate::AppState;

/// One version of the built-in text, from `docs/terms-changes.json`
/// (oldest first). Editing `docs/terms.md` means adding one; the
/// `terms-lint` CI job and the tests below check it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Change {
    pub version: String,
    /// `YYYY-MM-DD`: the day the change was published (and, if material, e-mailed).
    pub announced: String,
    /// `YYYY-MM-DD` (UTC): from this day it is the current version. A
    /// material change takes effect at least [`NOTICE_DAYS`] after it is announced.
    pub effective: String,
    /// Affects learners' rights or a paid plan: e-mailed ahead, and the
    /// notice blocks until accepted. A minor change's notice can be dismissed.
    pub material: bool,
    /// What changed, a few plain sentences (legal text: English, like the terms).
    pub summary: Vec<String>,
}

/// Days between announcing a material change and its effective date, at
/// least (the tests hold the changelog to it).
#[cfg(test)]
const NOTICE_DAYS: i64 = 30;

/// The version served if the changelog cannot be read (the tests fail first).
const NO_VERSION: &str = "unknown";

/// The changelog, oldest first.
pub fn changes() -> &'static [Change] {
    static CHANGES: OnceLock<Vec<Change>> = OnceLock::new();
    CHANGES.get_or_init(|| {
        serde_json::from_str(include_str!("../docs/terms-changes.json")).unwrap_or_else(|e| {
            tracing::error!("docs/terms-changes.json: {e}");
            Vec::new()
        })
    })
}

/// Days since the Unix epoch of a `YYYY-MM-DD` date.
pub fn day_of(date: &str) -> Option<i64> {
    let mut it = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    ((1..=12).contains(&m) && (1..=31).contains(&d) && date.len() == 10).then(|| crate::profile::days_from_civil(y, m as usize, d))
}

/// Today, in days since the Unix epoch (UTC).
fn today() -> i64 {
    now().div_euclid(86_400)
}

/// The index of the newest change in effect on `today`.
fn current_index(changes: &[Change], today: i64) -> Option<usize> {
    changes.iter().rposition(|c| day_of(&c.effective).is_some_and(|d| d <= today))
}

/// The version of the built-in text in effect now.
pub fn builtin_version() -> &'static str {
    let all = changes();
    current_index(all, today()).and_then(|i| all.get(i)).map_or(NO_VERSION, |c| c.version.as_str())
}

/// What an account that accepted an earlier version is shown: the changes
/// since, and whether any of them is material.
#[derive(Clone, Debug)]
pub struct Notice {
    pub material: bool,
    /// Newest first.
    pub changes: Vec<&'static Change>,
    /// Where the accept form returns to (the page it is shown on).
    pub next: String,
}

impl Notice {
    /// Shown on this page? Everywhere but the profile for a material change,
    /// so someone who does not agree can close their account there.
    pub fn shows_here(&self) -> bool {
        !(self.material && (self.next == "/profile" || self.next.starts_with("/profile?")))
    }
}

/// The changes between the version an account accepted and the current one
/// (`None`: nothing to show). A version not in the changelog (one from
/// before it, or a typo) counts as material, with every change in effect.
fn changes_since<'a>(changes: &'a [Change], today: i64, accepted: &str) -> Option<(bool, Vec<&'a Change>)> {
    let current = current_index(changes, today)?;
    let from = changes.iter().position(|c| c.version == accepted).map(|i| i + 1);
    let since: Vec<&Change> = changes.get(from.unwrap_or(0)..=current)?.iter().rev().collect();
    if since.is_empty() {
        return None;
    }
    let material = from.is_none() || since.iter().any(|c| c.material);
    Some((material, since))
}

/// The notice for an account whose latest acceptance is `accepted` and is
/// not the current version. `None`: the full-page gate instead (no version
/// accepted yet, or the server's own file, which has no changelog).
/// `/dev/login?terms=minor` leaves the dev user on [`crate::dev::MINOR_TERMS_VERSION`],
/// which reads as a minor change to the current version, for the browser tests.
pub fn notice(accepted: Option<&str>) -> Option<Notice> {
    if !matches!(SETTINGS.get().map(|s| &s.source), None | Some(Source::Builtin)) {
        return None;
    }
    let accepted = accepted?;
    let all = changes();
    let (material, changes) = if accepted == crate::dev::MINOR_TERMS_VERSION {
        (false, current_index(all, today()).and_then(|i| all.get(i)).into_iter().collect())
    } else {
        changes_since(all, today(), accepted)?
    };
    Some(Notice { material, changes, next: "/".into() })
}

/// The setting naming the text: `builtin` (default) or a Markdown file's path.
/// Read directly, not through `crate::env` (no older spelling).
pub const SOURCE_VAR: &str = "SKRONGLE_TERMS";

/// Where the Terms text comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// `docs/terms.md`, compiled in, with the contact and section 7 filled in.
    Builtin,
    /// The self-hoster's own file, served as it is.
    File { path: PathBuf, markdown: String, version: String },
}

impl Source {
    /// From the value of `SKRONGLE_TERMS`: unset, empty or `builtin` is the
    /// built-in text; anything else must be a readable, non-empty file.
    pub fn load(value: Option<&str>) -> Result<Source, String> {
        match value.map(str::trim) {
            None | Some("") | Some("builtin") => Ok(Source::Builtin),
            Some(p) => Source::from_file(Path::new(p)),
        }
    }

    fn from_file(path: &Path) -> Result<Source, String> {
        let hint = "set it to `builtin` or the path of a Markdown file";
        let markdown = std::fs::read_to_string(path)
            .map_err(|e| format!("{SOURCE_VAR}: cannot read the terms file {}: {e} ({hint})", path.display()))?;
        if markdown.trim().is_empty() {
            return Err(format!("{SOURCE_VAR}: the terms file {} is empty ({hint})", path.display()));
        }
        let version = file_version(&markdown);
        Ok(Source::File { path: path.to_path_buf(), markdown, version })
    }

    /// The version learners accept.
    pub fn version(&self) -> &str {
        match self {
            Source::Builtin => builtin_version(),
            Source::File { version, .. } => version,
        }
    }
}

/// A file's version: the first 16 hex characters of its SHA-256, so any
/// edit asks everyone to accept again.
fn file_version(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The minimum age the registration checkbox states (`SKRONGLE_MIN_AGE`):
/// 14 everywhere, because Quebec's Law 25 needs a parent's consent under 14
/// (#976). The Terms and the privacy policy say the same.
pub const DEFAULT_MIN_AGE: u32 = 14;

/// Shown where the copyright contact goes when `SKRONGLE_LEGAL_EMAIL` is unset.
pub const EMAIL_PLACEHOLDER: &str = "[CONTACT EMAIL]";

/// Where the built-in text names the address for help with the site
/// (`SKRONGLE_SUPPORT_EMAIL`; unset, the legal contact).
pub const SUPPORT_PLACEHOLDER: &str = "[SUPPORT EMAIL]";

struct Settings {
    min_age: u32,
    legal_email: Option<String>,
    support_email: Option<String>,
    source: Source,
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// Set once at startup from `SKRONGLE_MIN_AGE`, `SKRONGLE_LEGAL_EMAIL`,
/// `SKRONGLE_SUPPORT_EMAIL` and `SKRONGLE_TERMS` (`Source::load`).
pub fn init(min_age: Option<u32>, legal_email: Option<String>, support_email: Option<String>, source: Source) {
    let clean = |e: Option<String>| e.map(|e| e.trim().to_string()).filter(|e| !e.is_empty());
    let _ = SETTINGS.set(Settings { min_age: min_age.unwrap_or(DEFAULT_MIN_AGE), legal_email: clean(legal_email), support_email: clean(support_email), source });
}

/// The version of the text this server shows, which learners accept.
pub fn version() -> &'static str {
    SETTINGS.get().map_or_else(builtin_version, |s| s.source.version())
}

pub fn min_age() -> u32 {
    SETTINGS.get().map(|s| s.min_age).unwrap_or(DEFAULT_MIN_AGE)
}

/// The copyright / legal contact, or the placeholder.
pub fn legal_email() -> String {
    SETTINGS.get().and_then(|s| s.legal_email.clone()).unwrap_or_else(|| EMAIL_PLACEHOLDER.to_string())
}

/// The raw Markdown, as committed.
pub fn markdown_raw() -> String {
    crate::routes::Docs::get("terms.md").map(|f| String::from_utf8_lossy(&f.data).into_owned()).unwrap_or_default()
}

const SUBS_START: &str = "<!-- subscriptions -->";
const SUBS_END: &str = "<!-- /subscriptions -->";

/// The text as this server shows it: the self-hoster's file as it is, or the
/// built-in text with the contact address filled in and the subscription
/// section only when paid plans exist (`subscriptions`).
pub fn markdown(subscriptions: bool) -> String {
    match SETTINGS.get().map(|s| &s.source) {
        Some(Source::File { markdown, .. }) => markdown.clone(),
        _ => builtin_markdown(subscriptions),
    }
}

fn builtin_markdown(subscriptions: bool) -> String {
    let mut md = markdown_raw();
    if !subscriptions {
        while let (Some(a), Some(b)) = (md.find(SUBS_START), md.find(SUBS_END)) {
            if b < a {
                break;
            }
            md.replace_range(a..b + SUBS_END.len(), "");
        }
    }
    let s = SETTINGS.get();
    fill_contacts(&md, s.and_then(|s| s.legal_email.as_deref()), s.and_then(|s| s.support_email.as_deref()))
}

/// The built-in text with its two addresses filled in as `mailto:` links:
/// the legal contact for `[CONTACT EMAIL]`, the support address (or the
/// legal contact) for `[SUPPORT EMAIL]`. An address that isn't set leaves
/// its placeholder.
fn fill_contacts(md: &str, legal: Option<&str>, support: Option<&str>) -> String {
    let link = |e: &str| format!("[{e}](mailto:{e})");
    let mut md = md.to_string();
    if let Some(e) = support.or(legal) {
        md = md.replace(SUPPORT_PLACEHOLDER, &link(e));
    }
    if let Some(e) = legal {
        md = md.replace(EMAIL_PLACEHOLDER, &link(e));
    }
    md
}

/// One acceptance: the version and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Acceptance {
    pub version: String,
    pub accepted_at: i64,
}

/// Record that `user_id` accepted `version` now. Accepting the same version
/// again keeps the first time.
pub fn accept(conn: &Connection, user_id: i64, version: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO terms_acceptances (user_id, version, accepted_at) VALUES (?1, ?2, ?3)",
        params![user_id, version, now()],
    )?;
    Ok(())
}

/// The most recent acceptance, if any.
pub fn latest(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Acceptance>> {
    conn.query_row(
        "SELECT version, accepted_at FROM terms_acceptances WHERE user_id = ?1 ORDER BY accepted_at DESC, rowid DESC LIMIT 1",
        [user_id],
        |r| Ok(Acceptance { version: r.get(0)?, accepted_at: r.get(1)? }),
    )
    .optional()
}

/// SQL for "this user accepted the current version", for the session query.
pub fn accepted_sql(user: &str) -> String {
    accepted_sql_for(user, version())
}

/// The same for a given version (a constant or a hex hash; quoted anyway).
fn accepted_sql_for(user: &str, version: &str) -> String {
    let v = version.replace('\'', "''");
    format!("EXISTS(SELECT 1 FROM terms_acceptances WHERE user_id = {user} AND version = '{v}')")
}

/// The `mail_outbox` purpose of the e-mail announcing a material change.
pub const PURPOSE: &str = "terms-change";

/// The message announcing `change` to `email`, linking to `link`.
fn announcement(email: &str, change: &Change, link: &str, user_id: i64) -> Result<crate::mail::Mail, String> {
    let app = crate::APP_NAME;
    let date = change.effective.as_str();
    let subject = crate::i18n::fill(&crate::t("mail.terms_change.subject"), &[("app", &app), ("date", &date)]);
    let summary = change.summary.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n");
    let text = crate::i18n::fill(&crate::t("mail.terms_change.text"), &[("app", &app), ("date", &date), ("summary", &summary), ("link", &link)]);
    let esc = crate::error::html_escape;
    let items: String = change.summary.iter().map(|s| format!("<li>{}</li>", esc(s))).collect();
    let html = crate::i18n::fill(
        &crate::t("mail.terms_change.html"),
        &[("app", &app), ("date", &date), ("summary", &format!("<ul>{items}</ul>")), ("link", &esc(link))],
    );
    Ok(crate::mail::Mail::new(email, &subject, &text, PURPOSE)?.html(&html).for_user(user_id))
}

/// Queue the announcement of `change` for every account with an e-mail
/// address that has not had it, recording each in `terms_announcements` in
/// the same transaction, so a restart never mails anyone twice. Stops when
/// the hourly ceiling is full; the next press carries on. Returns how many
/// were queued.
fn announce(conn: &Connection, change: &Change, link: &str, now: i64, ceiling: Option<crate::mail::Ceiling>) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let todo: Vec<(i64, String)> = tx
        .prepare(
            "SELECT id, email FROM users WHERE COALESCE(email, '') <> ''
             AND id NOT IN (SELECT user_id FROM terms_announcements WHERE version = ?1) ORDER BY id",
        )?
        .query_map([&change.version], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut queued = 0;
    for (uid, email) in todo {
        match announcement(&email, change, link, uid) {
            Ok(mail) => match crate::mail::enqueue(&tx, &mail, now, ceiling)? {
                Ok(_) => queued += 1,
                Err(crate::mail::Capped::Hourly) => break,
                // No per-address cap applies to this purpose; skip the address.
                Err(crate::mail::Capped::Address) => continue,
            },
            // An address the mailer refuses: recorded, so it is not retried every hour.
            Err(e) => tracing::warn!("terms change {}: no e-mail for user {uid}: {e}", change.version),
        }
        tx.execute(
            "INSERT OR IGNORE INTO terms_announcements (user_id, version, mailed_at) VALUES (?1, ?2, ?3)",
            params![uid, change.version, now],
        )?;
    }
    tx.commit()?;
    Ok(queued)
}

/// Accounts with an address that have not had the e-mail of `version`.
fn owed(conn: &Connection, version: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM users WHERE COALESCE(email, '') <> ''
         AND id NOT IN (SELECT user_id FROM terms_announcements WHERE version = ?1)",
        [version],
        |r| r.get(0),
    )
}

/// The material changes not yet in effect: the ones to e-mail ahead. None
/// on a server that cannot send mail or has its own terms file (Skrongle mini).
fn upcoming(state: &AppState) -> Vec<&'static Change> {
    if !state.mail.can_send() || !matches!(SETTINGS.get().map(|s| &s.source), None | Some(Source::Builtin)) {
        return Vec::new();
    }
    let today = today();
    changes().iter().filter(|c| c.material && day_of(&c.effective).is_some_and(|d| d > today)).collect()
}

/// How many e-mails about upcoming changes are still to send: the
/// Superuser page shows its notice and Send button while this is above 0.
pub async fn unannounced(state: &AppState) -> Result<i64, crate::error::AppError> {
    let versions: Vec<String> = upcoming(state).into_iter().map(|c| c.version.clone()).collect();
    if versions.is_empty() {
        return Ok(0);
    }
    state.db.call(move |c| versions.iter().try_fold(0, |n, v| owed(c, v).map(|o| n + o))).await
}

/// E-mail every upcoming material change to the accounts that have not had
/// it: the Superuser page's Send button, nothing else. Returns how many
/// were queued; past the hourly ceiling the rest wait for the next press.
pub async fn announce_upcoming(state: &AppState) -> Result<usize, crate::error::AppError> {
    // Hosted servers set SKRONGLE_PUBLIC_URL (payments need it); without it the link is a bare path.
    let link = format!("{}{}", state.public_url.as_deref().unwrap_or(""), if state.public_url.is_some() { "/terms".to_string() } else { crate::url("/terms") });
    let ceiling = state.mail.ceiling();
    let mut total = 0;
    for change in upcoming(state) {
        let link = link.clone();
        let n = state.db.call(move |c| announce(c, change, &link, now(), ceiling)).await?;
        tracing::info!("terms change {} (effective {}): e-mailed {n} account(s)", change.version, change.effective);
        total += n;
    }
    if total > 0 {
        state.mail.notify();
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceptances_keep_history() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert_eq!(latest(c, ann)?, None);
            accept(c, ann, "old")?;
            c.execute("UPDATE terms_acceptances SET accepted_at = 5 WHERE version = 'old'", [])?;
            accept(c, ann, version())?;
            accept(c, ann, version())?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM terms_acceptances WHERE user_id = ?1", [ann], |r| r.get(0))?;
            assert_eq!(n, 2);
            assert_eq!(latest(c, ann)?.unwrap().version, version());
            let ok: bool = c.query_row(&format!("SELECT {}", accepted_sql("?1")), [ann], |r| r.get(0))?;
            assert!(ok);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_text_is_the_published_terms() {
        let raw = markdown_raw();
        assert!(raw.starts_with("# Terms and Conditions\n"));
        assert!(!raw.contains("Draft, not yet reviewed") && !raw.contains("[COMPANY NAME]"));
        assert!(builtin_markdown(false).contains("## 6. SUBSCRIPTIONS"));
        // No address is written into the text: they come from the settings (#1328).
        assert!(!raw.contains("gmail") && !raw.contains("@skrongle.com") && !raw.contains("mailto:"), "an address in docs/terms.md");
        assert!(raw.contains(SUPPORT_PLACEHOLDER) && raw.contains(EMAIL_PLACEHOLDER));
    }

    #[test]
    fn the_addresses_are_filled_in_from_the_settings() {
        let md = "Help: [SUPPORT EMAIL]. Notices: [CONTACT EMAIL].";
        assert_eq!(fill_contacts(md, None, None), md);
        assert_eq!(
            fill_contacts(md, Some("privacy@example.org"), Some("support@example.org")),
            "Help: [support@example.org](mailto:support@example.org). Notices: [privacy@example.org](mailto:privacy@example.org)."
        );
        // Only a legal contact: it answers for help too.
        assert_eq!(fill_contacts(md, Some("legal@example.org"), None).matches("legal@example.org").count(), 4);
        assert_eq!(fill_contacts(md, None, Some("support@example.org")), "Help: [support@example.org](mailto:support@example.org). Notices: [CONTACT EMAIL].");
    }

    /// "October 25, 2026" for `2026-10-25`, as the top of `docs/terms.md` says it.
    fn long_date(date: &str) -> String {
        const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
        let (y, m, d) = crate::profile::civil(day_of(date).unwrap() * 86_400);
        format!("{} {d}, {y}", MONTHS[m - 1])
    }

    /// The changelog (`docs/terms-changes.json`) is complete and agrees with
    /// the text: what the `terms-lint` CI job cannot see from a diff.
    #[test]
    fn the_changelog_matches_the_text() {
        let all = changes();
        assert!(!all.is_empty(), "docs/terms-changes.json has no entries (or does not parse)");
        let mut seen = std::collections::HashSet::new();
        let mut last = i64::MIN;
        for (i, c) in all.iter().enumerate() {
            let at = |what: &str| format!("docs/terms-changes.json entry {} ({}): {what}", i + 1, c.version);
            assert!(!c.version.trim().is_empty() && seen.insert(c.version.as_str()), "{}", at("the version is empty or used twice"));
            let effective = day_of(&c.effective).unwrap_or_else(|| panic!("{}", at("effective is not YYYY-MM-DD")));
            let announced = day_of(&c.announced).unwrap_or_else(|| panic!("{}", at("announced is not YYYY-MM-DD")));
            assert!(effective >= last, "{}", at("effective dates must not go back in time"));
            assert!(announced <= effective, "{}", at("announced after it takes effect"));
            assert!(!c.summary.is_empty() && c.summary.iter().all(|s| !s.trim().is_empty()), "{}", at("the summary is empty"));
            // The first version had no earlier one to give notice of.
            if c.material && i > 0 {
                assert!(effective - announced >= NOTICE_DAYS, "{}", at(&format!("a material change takes effect at least {NOTICE_DAYS} days after it is announced")));
            }
            last = effective;
        }
        let current = current_index(all, today()).expect("no version of the terms is in effect today");
        assert_eq!(version(), all[current].version);
        // A privacy change is logged here too (#978): its newest entry dates
        // docs/privacy.md, the newest Terms entry dates docs/terms.md.
        let top_of = |md: &str| -> String { md.lines().take(5).collect::<Vec<_>>().join("\n") };
        for (file, text, newest) in [
            ("docs/terms.md", markdown_raw(), all.iter().rev().find(|c| !is_privacy_change(c))),
            ("docs/privacy.md", include_str!("../docs/privacy.md").to_string(), all.iter().rev().find(|c| is_privacy_change(c))),
        ] {
            let Some(newest) = newest else { continue };
            let date = long_date(&newest.effective);
            assert!(top_of(&text).contains(&format!("**Last updated:** {date}")), "{file} must say \"**Last updated:** {date}\" at the top (its newest entry of docs/terms-changes.json, {})", newest.version);
        }
    }

    /// An entry about the privacy notice rather than the Terms (#978): its
    /// summary starts "The privacy policy changed".
    fn is_privacy_change(c: &Change) -> bool {
        c.summary.first().is_some_and(|s| s.starts_with("The privacy policy changed"))
    }

    fn change(version: &str, effective: &str, material: bool) -> Change {
        Change { version: version.into(), announced: "2026-01-01".into(), effective: effective.into(), material, summary: vec!["x".into()] }
    }

    #[test]
    fn the_notice_lists_the_changes_since_the_accepted_version() {
        let log = [change("a", "2026-01-01", true), change("b", "2026-02-01", false), change("c", "2026-03-01", false), change("d", "2026-06-01", true)];
        let today = day_of("2026-04-01").unwrap();
        fn versions(r: Option<(bool, Vec<&Change>)>) -> Option<(bool, String)> {
            r.map(|(m, cs)| (m, cs.iter().map(|c| c.version.as_str()).collect::<Vec<_>>().join(",")))
        }
        // d is announced but not in effect: c is current.
        assert_eq!(versions(changes_since(&log, today, "c")), None);
        assert_eq!(versions(changes_since(&log, today, "b")), Some((false, "c".into())));
        assert_eq!(versions(changes_since(&log, today, "a")), Some((false, "c,b".into())));
        // A version the changelog does not know: material, everything in effect.
        assert_eq!(versions(changes_since(&log, today, "2026-10-17-draft")), Some((true, "c,b,a".into())));
        // On d's date it is current, and material.
        let later = day_of("2026-06-01").unwrap();
        assert_eq!(versions(changes_since(&log, later, "c")), Some((true, "d".into())));
        assert_eq!(versions(changes_since(&log, later, "b")), Some((true, "d,c".into())));
        // Before the first version nothing is in effect.
        assert_eq!(versions(changes_since(&log, day_of("2025-12-31").unwrap(), "x")), None);
    }

    #[test]
    fn dates_parse() {
        assert_eq!(day_of("1970-01-02"), Some(1));
        assert_eq!(day_of("2026-10-25"), Some(crate::profile::days_from_civil(2026, 10, 25)));
        assert_eq!(long_date("2026-10-25"), "October 25, 2026");
        for bad in ["2026-13-01", "2026-1-01", "26-10-25", "2026-10", "soon", ""] {
            assert_eq!(day_of(bad), None, "{bad}");
        }
    }

    /// The 30-day e-mail goes to each account with an address once, however
    /// often Send is pressed, and new accounts get it on the next press.
    #[test]
    fn a_material_change_is_mailed_once_per_account() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let mut c = change("next", "2099-01-01", true);
        c.summary = vec!["Prices <change>.".into()];
        db.with(|conn| {
            crate::auth::create_user(conn, "ann@example.org", "h")?;
            crate::auth::create_user(conn, "bea@example.org", "h")?;
            assert_eq!(owed(conn, "next")?, 2);
            assert_eq!(announce(conn, &c, "https://skrongle.test/terms", 100, None)?, 2);
            assert_eq!(owed(conn, "next")?, 0);
            assert_eq!(announce(conn, &c, "https://skrongle.test/terms", 200, None)?, 0);
            crate::auth::create_user(conn, "cat@example.org", "h")?;
            assert_eq!(owed(conn, "next")?, 1);
            assert_eq!(announce(conn, &c, "https://skrongle.test/terms", 300, None)?, 1);
            let n: i64 = conn.query_row("SELECT COUNT(*) FROM mail_outbox WHERE purpose = ?1", [PURPOSE], |r| r.get(0))?;
            assert_eq!(n, 3);
            let rows: i64 = conn.query_row("SELECT COUNT(*) FROM terms_announcements WHERE version = 'next'", [], |r| r.get(0))?;
            assert_eq!(rows, 3);
            let (subject, text, html): (String, String, String) =
                conn.query_row("SELECT subject, body_text, body_html FROM mail_outbox WHERE to_addr = 'ann@example.org'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            assert!(subject.contains("2099-01-01"), "{subject}");
            assert!(text.contains("- Prices <change>.") && text.contains("https://skrongle.test/terms"), "{text}");
            assert!(html.contains("<li>Prices &lt;change&gt;.</li>"), "{html}");
            Ok(())
        })
        .unwrap();
    }

    fn temp_file(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("skrongle-terms-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("terms.md");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn builtin_is_the_default() {
        assert_eq!(Source::load(None).unwrap(), Source::Builtin);
        assert_eq!(Source::load(Some("")).unwrap(), Source::Builtin);
        assert_eq!(Source::load(Some("builtin")).unwrap(), Source::Builtin);
        assert_eq!(Source::Builtin.version(), builtin_version());
        // Tests never call init: the server-wide values are the built-in ones.
        assert_eq!(version(), builtin_version());
        assert!(markdown(false).contains("# Terms and Conditions"));
    }

    #[test]
    fn a_file_is_served_as_it_is_and_its_hash_is_the_version() {
        let path = temp_file("serve", "# Our terms\n\nRun by Ann for the family. [CONTACT EMAIL] stays.\n");
        let src = Source::load(path.to_str()).unwrap();
        let Source::File { markdown, version, .. } = &src else { panic!("not a file: {src:?}") };
        assert!(markdown.contains("Run by Ann") && markdown.contains("[CONTACT EMAIL] stays"));
        assert_eq!(version.len(), 16);
        assert!(version.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(version, builtin_version());
        // The same text is the same version.
        assert_eq!(Source::load(path.to_str()).unwrap().version(), version);
    }

    #[test]
    fn editing_the_file_asks_for_acceptance_again() {
        let path = temp_file("edit", "# Terms\n\nVersion one.\n");
        let v1 = Source::load(path.to_str()).unwrap().version().to_string();
        std::fs::write(&path, "# Terms\n\nVersion two.\n").unwrap();
        let v2 = Source::load(path.to_str()).unwrap().version().to_string();
        assert_ne!(v1, v2);
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            accept(c, ann, &v1)?;
            let ok = |v: &str| -> rusqlite::Result<bool> {
                c.query_row(&format!("SELECT {}", accepted_sql_for("?1", v)), [ann], |r| r.get(0))
            };
            assert!(ok(&v1)?);
            assert!(!ok(&v2)?, "the edited file is a new version");
            accept(c, ann, &v2)?;
            assert!(ok(&v2)?);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_bad_value_or_missing_file_refuses_to_start() {
        let missing = std::env::temp_dir().join("skrongle-terms-no-such-dir/terms.md");
        let err = Source::load(missing.to_str()).unwrap_err();
        assert!(err.contains(SOURCE_VAR) && err.contains(&missing.display().to_string()), "{err}");
        let err = Source::load(Some("off")).unwrap_err();
        assert!(err.contains(SOURCE_VAR) && err.contains("off"), "{err}");
        let empty = temp_file("empty", "  \n");
        let err = Source::load(empty.to_str()).unwrap_err();
        assert!(err.contains("empty") && err.contains(&empty.display().to_string()), "{err}");
        // A folder is not a file.
        let dir = empty.parent().unwrap();
        assert!(Source::load(dir.to_str()).is_err());
    }
}
