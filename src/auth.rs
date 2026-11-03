//! Password hashing, session tokens, and the `CurrentUser` extractor.

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use axum::extract::FromRequestParts;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;

use crate::db::now;
use crate::error::AppError;
use crate::AppState;

mod cache;
pub use cache::SessionCache;

pub const SESSION_COOKIE: &str = "sid";
/// Sessions live for 30 days of inactivity-free time (TTL is fixed, not sliding).
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Live sessions one account keeps; signing in again ends the oldest (#30).
pub use crate::caps::MAX_SESSIONS_PER_ACCOUNT;

// Argon2id parameters tuned for a Cortex-A53 at ~1 GHz: ~8 MiB memory, 3 passes.
// Roughly 60-120 ms per hash on the Pi Zero 2 W, and never more than 8 MiB of
// transient memory, which matters a lot more than on a desktop.
const ARGON_M_COST_KIB: u32 = 8 * 1024;
const ARGON_T_COST: u32 = 3;
const ARGON_P_COST: u32 = 1;

fn hasher() -> Argon2<'static> {
    let params = Params::new(ARGON_M_COST_KIB, ARGON_T_COST, ARGON_P_COST, None)
        .expect("valid argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    hasher()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("hash failure: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => hasher().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// One argon2 hash or verify at a time (#31, part of the `mini-caps` epic
/// #48). Each one holds `ARGON_M_COST_KIB` of transient memory, and the
/// Tokio blocking pool has three threads: three at once would cost 24 MiB on
/// a box with `MemoryMax=64M`. A family never needs parallel hashing; a short
/// queue is invisible, an OOM kill is not. Request handlers go through
/// [`hash_password_async`] and [`verify_password_async`], so nothing bypasses it.
static HASH_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Hashes running right now, and the most there ever were (tests only).
#[cfg(test)]
static HASHES_RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static HASHES_PEAK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Run `work` on the blocking pool while holding the one hash slot. Only the
/// hashing waits; the rest of the request does not.
async fn in_hash_slot<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
    let _permit = HASH_SLOT.acquire().await.map_err(|e| AppError::Internal(e.to_string()))?;
    tokio::task::spawn_blocking(move || {
        #[cfg(test)]
        {
            use std::sync::atomic::Ordering::SeqCst;
            let now = HASHES_RUNNING.fetch_add(1, SeqCst) + 1;
            HASHES_PEAK.fetch_max(now, SeqCst);
        }
        let out = work();
        #[cfg(test)]
        HASHES_RUNNING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        out
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))
}

/// [`hash_password`] behind the one hash slot.
pub async fn hash_password_async(password: String) -> Result<String, AppError> {
    in_hash_slot(move || hash_password(&password)).await?
}

/// [`verify_password`] behind the one hash slot.
pub async fn verify_password_async(password: String, hash: String) -> Result<bool, AppError> {
    in_hash_slot(move || verify_password(&password, &hash)).await
}

/// 256-bit random token, hex encoded.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    crate::random::fill(&mut bytes);
    let mut s = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: i64,
    /// The address the account signs in with; '' for an old account that
    /// never gave one.
    pub email: String,
    /// `users.email_verified_at` is set: the address was confirmed by a link
    /// (`verify.rs`) or by Google. Nothing is gated on it yet.
    pub email_verified: bool,
    /// Profile fields shown in the navigation bar (see `profile.rs`).
    pub first_name: String,
    pub last_name: String,
    /// The course the user is currently working in (`users.active_course`).
    pub course_id: Option<String>,
    /// Its title, for the navigation bar.
    pub course_title: Option<String>,
    /// Its owner when it is a user-made course (`courses.owner_id`); `None`
    /// for a built-in one or an id with no `courses` row. `Project` opens a
    /// user-made course only for its owner (#383).
    pub course_owner: Option<i64>,
    /// The level they start that course at (`projects.start_unit`): a unit
    /// id, '' for the beginning. `Project::start` is the unit index.
    pub start_unit: String,
    /// Accepted the current Terms of Service (`terms::version()`). The
    /// `CurrentUser` extractor sends anyone else to `/terms/accept`, unless
    /// `terms_notice` is set.
    pub terms_ok: bool,
    /// Not `terms_ok`, but an earlier built-in version was accepted: the
    /// pages show this notice of what changed (`terms_notice.html`, #966).
    /// A minor one blocks nothing; a material one lets only GET pages
    /// through (with the notice over them, until accepted).
    pub terms_notice: Option<crate::terms::Notice>,
    /// `users.must_change_password`: signed in with a temporary password the
    /// owner set (#635, `owner.rs`). The `CurrentUser` extractor sends the
    /// account to `/password/change` before anything else.
    pub must_change_password: bool,
    /// `users.delete_at`: the learner asked to delete the account, and it
    /// is deleted at this time (#970, `account_deletion.rs`). Until then
    /// the `CurrentUser` extractor sends every page to `/account/deletion`,
    /// whose one action is to cancel it.
    pub delete_at: Option<i64>,
    /// `users.superuser`: sees the profile's Superuser section, whose actions
    /// reach any account. Set by hand (and on the dev user in dev mode).
    pub superuser: bool,
    /// `users.ai_off`: "Hide all AI features" (profile, or the box at
    /// sign-up). Pages hide the tutor, course generation and AI revisions,
    /// and their routes refuse through `require_ai`.
    pub ai_off: bool,
    /// `users.lang`: the language the app speaks to this account in
    /// (`i18n`), '' for "whatever the browser asks for".
    pub lang: String,
    /// The browser's offset from UTC in seconds (the `tz` cookie
    /// `sprint.js` sets; 0 without one): where this request's local day
    /// starts, for the daily goal, the streak and the earnings ledger.
    pub tz: i64,
}

impl CurrentUser {
    #[cfg(test)]
    pub fn test(id: i64) -> CurrentUser {
        CurrentUser {
            id,
            email: String::new(),
            email_verified: false,
            first_name: String::new(),
            last_name: String::new(),
            course_id: None,
            course_title: None,
            course_owner: None,
            start_unit: String::new(),
            terms_ok: true,
            terms_notice: None,
            must_change_password: false,
            delete_at: None,
            superuser: false,
            ai_off: false,
            lang: String::new(),
            tz: 0,
        }
    }

    /// A page that calls an AI provider: `Err` sends an account with AI
    /// features off to the profile section that turns them back on.
    pub fn require_ai(&self) -> Result<(), AppError> {
        if self.ai_off { Err(AppError::AiOff) } else { Ok(()) }
    }

    /// The same for a JSON route: a 403 body.
    pub fn require_ai_api(&self) -> Result<(), AppError> {
        if self.ai_off { Err(AppError::ApiAiOff) } else { Ok(()) }
    }

    /// Has an address that is not confirmed yet: the dashboard says so.
    pub fn needs_email_confirmation(&self) -> bool {
        !self.email_verified && !self.email.is_empty()
    }

    /// First name if known, else "Learner": for greetings and the top bar.
    pub fn short_name(&self) -> String {
        crate::profile::short_name(&self.first_name)
    }

    /// "First Last", or "Learner" when no name is set.
    pub fn display_name(&self) -> String {
        crate::profile::display_name(&self.first_name, &self.last_name)
    }

}

// ---------- DB helpers (run inside Db::call) ----------

/// Longest password (characters). Longer ones are not hashed at all, so a
/// huge one can't make argon2 work for nothing; it can't be right anyway.
pub const PASSWORD_MAX: usize = 128;

pub fn validate_password(p: &str) -> Result<(), String> {
    let len = p.chars().count();
    if len < 8 {
        return Err(crate::t("auth.password.too_short"));
    }
    if len > PASSWORD_MAX {
        return Err(crate::t1("auth.password.too_long", "max", PASSWORD_MAX));
    }
    Ok(())
}

/// A password typed to sign in or confirm that cannot be anyone's (too long to have been set).
pub fn password_too_long(p: &str) -> bool {
    p.chars().count() > PASSWORD_MAX
}

/// Create a password account. The e-mail address (already validated and
/// lower-cased) names the account and is fixed from here on.
pub fn create_user(conn: &Connection, email: &str, password_hash: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO users (password_hash, created_at, email) VALUES (?1, ?2, ?3)",
        params![password_hash, now(), email],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Finds the account through `idx_users_email_lower` (#604): every lookup by
/// address is written `lower(email) = lower(?)` so it matches that index.
const CREDENTIALS_SQL: &str = "SELECT id, password_hash FROM users WHERE lower(email) = lower(?1) AND password_hash <> '' ORDER BY id LIMIT 1";

/// The password account an e-mail address signs in to: (id, password hash).
/// Accounts without a password are never found here.
pub fn find_user_credentials(conn: &Connection, email: &str) -> rusqlite::Result<Option<(i64, String)>> {
    conn.query_row(
        CREDENTIALS_SQL,
        [email],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// What `sessions.session_token` stores: the SHA-256 of the cookie's token,
/// in hex. A leaked database (or backup) then holds no token that signs
/// anyone in; the cookie's 256 random bits need no salt or slow hash.
pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(token.as_bytes()) {
        out.push(HEX[usize::from(b >> 4)] as char);
        out.push(HEX[usize::from(b & 15)] as char);
    }
    out
}

/// Start a session; returns the token for the cookie (only its hash is stored).
/// The only place a `sessions` row is inserted: it first ends the account's
/// oldest sessions so that, with the new one, it holds at most
/// [`MAX_SESSIONS_PER_ACCOUNT`] (#30). Two plain statements, no savepoint of
/// its own: the one connection sits behind a mutex, so nothing runs between
/// them, and inside a caller's transaction they commit or roll back with it.
pub fn create_session(conn: &Connection, user_id: i64) -> rusqlite::Result<String> {
    let token = generate_token();
    let t = now();
    conn.execute(
        "DELETE FROM sessions WHERE id IN \
         (SELECT id FROM sessions WHERE user_id = ?1 ORDER BY created_at DESC, id DESC LIMIT -1 OFFSET ?2)",
        params![user_id, MAX_SESSIONS_PER_ACCOUNT as i64 - 1],
    )?;
    conn.execute(
        "INSERT INTO sessions (user_id, session_token, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![user_id, hash_token(&token), t, t + SESSION_TTL_SECS],
    )?;
    Ok(token)
}

pub fn delete_session(conn: &Connection, token: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sessions WHERE session_token = ?1", [hash_token(token)])?;
    Ok(())
}

/// End every session of `user_id` except `keep` (after a password change).
pub fn delete_other_sessions(conn: &Connection, user_id: i64, keep: &str) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM sessions WHERE user_id = ?1 AND session_token <> ?2", params![user_id, hash_token(keep)])
}

pub fn purge_expired_sessions(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM sessions WHERE expires_at < ?1", [now()])
}

/// The user behind a session token. The course title is filled in for
/// user-made courses here; built-in ones are titled by the registry.
/// Requests go through `cached_user`, which asks this only on a miss.
#[cfg(test)]
pub fn user_for_session(conn: &Connection, token: &str) -> rusqlite::Result<Option<CurrentUser>> {
    Ok(read_session(conn, &hash_token(token), now())?.map(|(u, _)| u))
}

/// The session query: every table it reads must be in `cache::WATCHED`.
fn session_sql() -> String {
    format!(
        "SELECT u.id, COALESCE(u.email, ''), u.active_course, c.title, u.first_name, u.last_name, NULL, NULL, {}, u.ai_off, u.email_verified_at IS NOT NULL, u.superuser,
                COALESCE(p.start_unit, ''), NULL, u.lang, c.owner_id, u.must_change_password,
                (SELECT version FROM terms_acceptances WHERE user_id = u.id ORDER BY accepted_at DESC, rowid DESC LIMIT 1), u.delete_at, s.expires_at
         FROM sessions s JOIN users u ON u.id = s.user_id LEFT JOIN courses c ON c.id = u.active_course
         LEFT JOIN projects p ON p.user_id = u.id AND p.course_id = u.active_course
         WHERE s.session_token = ?1 AND s.expires_at > ?2",
        crate::terms::accepted_sql("u.id")
    )
}

/// The user of the session whose token hashes to `hash`, at time `now`,
/// and when that session expires.
fn read_session(conn: &Connection, hash: &str, now: i64) -> rusqlite::Result<Option<(CurrentUser, i64)>> {
    conn.query_row(
        &session_sql(),
        params![hash, now],
        |r| {
            let terms_ok: bool = r.get(8)?;
            let terms_notice = if terms_ok { None } else { crate::terms::notice(r.get::<_, Option<String>>(17)?.as_deref()) };
            let user = CurrentUser {
                id: r.get(0)?,
                email: r.get(1)?,
                course_id: r.get(2)?,
                course_title: r.get(3)?,
                course_owner: r.get(15)?,
                first_name: r.get(4)?,
                last_name: r.get(5)?,
                terms_ok,
                terms_notice,
                must_change_password: r.get(16)?,
                delete_at: r.get(18)?,
                ai_off: r.get(9)?,
                email_verified: r.get(10)?,
                superuser: r.get(11)?,
                start_unit: r.get(12)?,
                lang: r.get(14)?,
                tz: 0,
            };
            Ok((user, r.get(19)?))
        },
    )
    .optional()
}

/// `user_for_session` for a request: from the in-memory cache when the
/// session is there (#37), else from the database, caching what it finds.
async fn cached_user(state: &AppState, token: &str) -> Result<Option<CurrentUser>, AppError> {
    let hash = hash_token(token);
    let cache = state.db.sessions();
    if let Some(user) = cache.get(&hash, now()) {
        return Ok(Some(user));
    }
    state
        .db
        .call(move |c| {
            let t = now();
            let found = read_session(c, &hash, t)?;
            // Under the database's lock: no write (and so no forget_all) can
            // come between the read and the put.
            if let Some((user, expires_at)) = &found {
                cache.put(hash, user.clone(), t, *expires_at);
            }
            Ok(found.map(|(u, _)| u))
        })
        .await
}

// ---------- Cookies ----------

pub fn session_cookie_from_headers(headers: &HeaderMap) -> Option<String> {
    headers.get_all(COOKIE).iter().find_map(|v| {
        let raw = v.to_str().ok()?;
        raw.split(';').find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            if k.trim() == SESSION_COOKIE {
                let v = v.trim();
                // Tokens are hex; anything else is garbage.
                if v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Some(v.to_string());
                }
            }
            None
        })
    })
}

/// `Set-Cookie` header establishing a session. Not `Secure` because the Pi is
/// typically reached over plain HTTP on a LAN; put a TLS reverse proxy in
/// front and set `SKRONGLE_SECURE_COOKIES=1` to add the flag.
pub fn set_session_cookie(token: &str, secure: bool) -> (axum::http::HeaderName, HeaderValue) {
    let v = format!(
        "{SESSION_COOKIE}={token}; Path={}; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}{}",
        crate::cookie_path(),
        if secure { "; Secure" } else { "" }
    );
    (SET_COOKIE, HeaderValue::from_str(&v).expect("valid cookie header"))
}

pub fn clear_session_cookie() -> (axum::http::HeaderName, HeaderValue) {
    let v = format!("{SESSION_COOKIE}=; Path={}; HttpOnly; SameSite=Lax; Max-Age=0", crate::cookie_path());
    (SET_COOKIE, HeaderValue::from_str(&v).expect("valid cookie header"))
}

// ---------- Extractors ----------

async fn resolve_user(parts: &Parts, state: &AppState) -> Result<Option<(String, CurrentUser)>, AppError> {
    let Some(token) = session_cookie_from_headers(&parts.headers) else {
        return Ok(None);
    };
    let mut user = cached_user(state, &token).await?;
    if let Some(u) = user.as_mut() {
        u.tz = crate::routes::sprint::tz_offset_secs(&parts.headers);
        // The account's language wins over the cookie and the browser's.
        crate::i18n::set_current(&u.lang);
        if u.course_title.is_none() {
            u.course_title = u.course_id.as_deref().and_then(|id| state.courses.builtin_by_id(id)).map(|c| c.title().to_string());
        }
    }
    Ok(user.map(|u| (token, u)))
}

/// The signed-in user of a request with these headers, if any (the error
/// page's layout, which runs after the handler and has no extractor).
pub async fn user_from_headers(state: &AppState, headers: &HeaderMap) -> Option<CurrentUser> {
    let (mut parts, _) = axum::http::Request::new(()).into_parts();
    parts.headers = headers.clone();
    resolve_user(&parts, state).await.ok().flatten().map(|(_, u)| u)
}

/// Route guard: resolves the session cookie to a user, or rejects.
/// Browser routes get a redirect to `/login`; `/api/*` routes get a 401 JSON body.
impl FromRequestParts<Arc<AppState>> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let api = parts.uri.path().starts_with("/api/");
        match resolve_user(parts, state).await? {
            // Deletion scheduled (#970): nothing but the page that cancels it.
            Some((_, user)) if user.delete_at.is_some() && api => Err(AppError::ApiDeletionScheduled),
            Some((_, user)) if user.delete_at.is_some() => Err(AppError::DeletionScheduled),
            // A temporary password (#635): a new one first, then the rest.
            Some((_, user)) if user.must_change_password && api => Err(AppError::ApiPasswordChangeRequired),
            Some((_, user)) if user.must_change_password => Err(AppError::PasswordChangeRequired(terms_next(parts))),
            Some((_, user)) if user.terms_ok => Ok(user),
            // A newer version of the built-in terms (#966): a minor change
            // is a notice and nothing more; a material one shows over GET
            // pages and refuses everything else until accepted, except
            // closing the account, which the notice offers instead.
            Some((_, mut user)) if user.terms_notice.as_ref().is_some_and(|n| !n.material || (!api && ((parts.method == axum::http::Method::GET && !own_layout(parts)) || app_path(parts) == "/profile/delete"))) => {
                if let Some(n) = user.terms_notice.as_mut() {
                    n.next = terms_next(parts);
                }
                Ok(user)
            }
            // Terms not accepted (a new version, or an account from before
            // them): the gate first, then back where they were going.
            Some(_) if api => Err(AppError::ApiTermsRequired),
            Some(_) => Err(AppError::TermsRequired(terms_next(parts))),
            None if api => Err(AppError::Unauthorized),
            None => Err(AppError::RedirectToLogin),
        }
    }
}

/// A page with its own layout instead of `base.html` (Sprint, the tutor),
/// which cannot show the terms notice: a material change sends it to the
/// gate instead, as before the notice existed.
fn own_layout(parts: &Parts) -> bool {
    let path = app_path(parts);
    ["/sprint", "/tutor"].iter().any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// The request's path inside the app (without the base path).
fn app_path(parts: &Parts) -> &str {
    let path = parts.uri.path();
    path.strip_prefix(crate::base_path()).filter(|p| p.starts_with('/')).unwrap_or(path)
}

/// Where to return after the terms gate (or the password one): the app path of a GET request, else home.
fn terms_next(parts: &Parts) -> String {
    if parts.method != axum::http::Method::GET {
        return "/".into();
    }
    let path = app_path(parts);
    match parts.uri.query() {
        Some(q) => format!("{path}?{q}"),
        None => path.to_string(),
    }
}

/// Like `CurrentUser` but never rejects (for pages that work logged-out).
pub struct MaybeUser(pub Option<CurrentUser>);

impl FromRequestParts<Arc<AppState>> for MaybeUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        Ok(MaybeUser(resolve_user(parts, state).await?.map(|(_, u)| u)))
    }
}

/// A signed-in user working in a course: the extractor every learning page
/// uses. Without an active course (or with one that no longer loads) browser
/// routes are sent to the course picker and API routes get a 410
/// (`AppError::ApiNoCourse`), which the page scripts follow to the picker.
pub struct Project {
    pub user: CurrentUser,
    pub course: Arc<crate::content::ContentStore>,
    /// The unit index the learner starts this course at (0 unless they
    /// picked a later level of a course with `levels`). Lessons of the units
    /// below it are skipped until touched (`progress::is_skipped`).
    pub start: usize,
}

impl Project {
    pub fn course_id(&self) -> &str {
        self.course.id()
    }
}

impl FromRequestParts<Arc<AppState>> for Project {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        let is_api = parts.uri.path().starts_with("/api/");
        let no_course = || if is_api { AppError::ApiNoCourse } else { AppError::RedirectToCourses };
        let Some(id) = user.course_id.clone() else { return Err(no_course()) };
        // A user-made course opens for its owner, or for a learner it is
        // shared with (#451), whatever wrote `users.active_course` (#383);
        // otherwise it is dropped as active.
        let allowed = state.courses.is_builtin(&id) || user.course_owner == Some(user.id) || {
            let (uid, cid) = (user.id, id.clone());
            user.course_owner.is_some() && state.db.call(move |c| crate::share::can_open(c, uid, &cid)).await?
        };
        let course = if allowed {
            state.courses.get(&state.db, &id).await?
        } else {
            None
        };
        match course {
            Some(course) => {
                let start = course.start_index(&user.start_unit);
                Ok(Project { user, course, start })
            }
            None => {
                // The course was deleted, does not validate any more or is not
                // theirs: drop it as active.
                let uid = user.id;
                state.db.call(move |c| crate::courses::clear_active(c, uid)).await?;
                Err(no_course())
            }
        }
    }
}

/// The raw session token, for logout.
pub struct SessionToken(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for SessionToken {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(SessionToken(session_cookie_from_headers(&parts.headers)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hex lookup gives what the per-byte `format!` did.
    #[test]
    fn hash_token_hex_matches_format() {
        use sha2::{Digest, Sha256};
        let old: String = Sha256::digest(b"abc").iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hash_token("abc"), old);
        assert_eq!(hash_token("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// A request after the first finds its user in memory (#37), and every
    /// change to what the session query reads is seen on the next request:
    /// a profile field, the course, terms, signing out elsewhere.
    #[test]
    fn the_signed_in_user_is_cached_until_something_it_shows_changes() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let state = crate::routes::test_state(false);
        let ann = state.db.with(|c| create_user(c, "ann@example.org", "h")).unwrap();
        let session = |u| state.db.with(|c| create_session(c, u)).unwrap();
        let (here, elsewhere) = (session(ann), session(ann));
        let cache = state.db.sessions();
        let request = |token: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(COOKIE, HeaderValue::from_str(&format!("sid={token}")).unwrap());
            rt.block_on(user_from_headers(&state, &headers))
        };

        let first = request(&here).unwrap();
        assert!(!first.terms_ok);
        let reads = cache.reads();
        for _ in 0..3 {
            assert_eq!(request(&here).map(|u| u.id), Some(ann));
        }
        assert_eq!(cache.reads(), reads, "later requests make no session query");

        state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
        assert!(request(&here).unwrap().terms_ok, "terms accepted");

        state.db.with(|c| crate::profile::update(c, ann, Some("Ann"), None)).unwrap();
        assert_eq!(request(&here).unwrap().first_name, "Ann", "profile field");

        state.db.with(|c| crate::courses::open_project(c, ann, "spanish")).unwrap();
        assert_eq!(request(&here).unwrap().course_id.as_deref(), Some("spanish"), "course switched");

        // `projects` and `courses` are WITHOUT ROWID tables: no update hook
        // sees them, the triggers do.
        let change = crate::courses::SettingsChange { start_unit: Some("2".into()), ..Default::default() };
        state.db.with(|c| crate::courses::set_project_settings(c, ann, "spanish", &change)).unwrap();
        assert_eq!(request(&here).unwrap().start_unit, "2", "course setting");
        state.db.with(|c| c.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('mine', ?1, 'Mine', 1, 1)", [ann])).unwrap();
        state.db.with(|c| crate::courses::open_project(c, ann, "mine")).unwrap();
        assert_eq!(request(&here).unwrap().course_title.as_deref(), Some("Mine"));
        state.db.with(|c| c.execute("UPDATE courses SET title = 'Renamed' WHERE id = 'mine'", [])).unwrap();
        assert_eq!(request(&here).unwrap().course_title.as_deref(), Some("Renamed"), "course title");

        assert!(request(&elsewhere).is_some());
        state.db.with(|c| delete_other_sessions(c, ann, &here)).unwrap();
        assert!(request(&elsewhere).is_none(), "signed out elsewhere");
        assert!(request(&here).is_some());
        state.db.with(|c| delete_session(c, &here)).unwrap();
        assert!(request(&here).is_none(), "signed out");

        let reads = cache.reads();
        let last = session(ann);
        request(&last);
        request(&last);
        assert_eq!(cache.reads(), reads + 1);
    }

    #[test]
    fn sign_in_finds_the_address_in_any_case_through_the_index() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            assert_eq!(crate::db::table_scans(c, CREDENTIALS_SQL).unwrap(), Vec::<String>::new());
            // One stored before addresses were lower-cased, one after.
            let ann = create_user(c, "Ann@Example.org", "h1")?;
            let bob = create_user(c, "bob@example.org", "h2")?;
            assert_eq!(find_user_credentials(c, "ann@example.org")?, Some((ann, "h1".to_string())));
            assert_eq!(find_user_credentials(c, "BOB@Example.ORG")?, Some((bob, "h2".to_string())));
            assert_eq!(find_user_credentials(c, "carol@example.org")?, None);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=8192,t=3,p=1$"), "{h}");
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not-a-hash"));
    }

    /// Many sign-ins at once still hash one after another (#31).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn hashes_run_one_at_a_time() {
        use std::sync::atomic::Ordering::SeqCst;
        let hash = hash_password("pw-for-the-queue").unwrap();
        HASHES_PEAK.store(0, SeqCst);
        let mut jobs = Vec::new();
        for i in 0..6 {
            let hash = hash.clone();
            jobs.push(tokio::spawn(async move {
                if i % 2 == 0 {
                    verify_password_async("pw-for-the-queue".into(), hash).await.unwrap()
                } else {
                    hash_password_async("another-pw".into()).await.unwrap().starts_with("$argon2id$")
                }
            }));
        }
        for j in jobs {
            assert!(j.await.unwrap());
        }
        assert_eq!(HASHES_PEAK.load(SeqCst), 1, "more than one hash ran at once");
        assert_eq!(HASHES_RUNNING.load(SeqCst), 0);
    }

    /// A hash written by an earlier argon2 crate (0.5, a dev account's
    /// `dev-password`) still verifies after the crate upgrade (#585).
    #[test]
    fn old_hashes_still_verify() {
        let h = "$argon2id$v=19$m=8192,t=3,p=1$pPPj4hcUihbRf+ODyZiZ3A$qS2aYMKnieD73TCIQQ2TWTeJ19mmhj83LAu+nmG2XGE";
        assert!(verify_password("dev-password", h));
        assert!(!verify_password("dev-passwor", h));
    }

    /// Only the token's hash is stored, and every lookup goes through it:
    /// sign in, sign out, and "every other session" after a password change.
    #[test]
    fn sessions_are_stored_and_found_by_hash() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = create_user(c, "ann@example.org", "h")?;
            crate::terms::accept(c, ann, crate::terms::version())?;
            let (here, elsewhere) = (create_session(c, ann)?, create_session(c, ann)?);
            let stored: Vec<String> = {
                let mut stmt = c.prepare("SELECT session_token FROM sessions ORDER BY rowid")?;
                let v = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
                v
            };
            assert_eq!(stored, vec![hash_token(&here), hash_token(&elsewhere)]);
            assert!(!stored.contains(&here) && !stored.contains(&elsewhere), "a token stored in plain text");
            assert_eq!(hash_token("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
            assert_eq!(user_for_session(c, &here)?.map(|u| u.id), Some(ann));
            // The stored hash is no use as a cookie.
            assert!(user_for_session(c, &stored[0])?.is_none());
            assert_eq!(delete_other_sessions(c, ann, &here)?, 1);
            assert!(user_for_session(c, &elsewhere)?.is_none());
            assert!(user_for_session(c, &here)?.is_some());
            delete_session(c, &here)?;
            assert!(user_for_session(c, &here)?.is_none());
            Ok(())
        })
        .unwrap();
    }

    /// The fifth sign-in ends the oldest session; the other four work (#30).
    #[test]
    fn fifth_session_ends_the_oldest() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = create_user(c, "ann@example.org", "h")?;
            let bea = create_user(c, "bea@example.org", "h")?;
            crate::terms::accept(c, ann, crate::terms::version())?;
            crate::terms::accept(c, bea, crate::terms::version())?;
            let other = create_session(c, bea)?;
            let tokens: Vec<String> = (0..5).map(|_| create_session(c, ann)).collect::<Result<_, _>>()?;
            assert!(user_for_session(c, &tokens[0])?.is_none());
            for t in &tokens[1..] {
                assert_eq!(user_for_session(c, t)?.map(|u| u.id), Some(ann));
            }
            // Another account's session is not counted against Ann's.
            assert_eq!(user_for_session(c, &other)?.map(|u| u.id), Some(bea));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn credentials_are_found_whatever_the_case_of_the_address() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            assert_eq!(find_user_credentials(c, "ana@example.org")?, None);
            let pw = create_user(c, "ana@example.org", "h")?;
            assert_eq!(find_user_credentials(c, "ANA@example.org")?, Some((pw, "h".to_string())));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn tokens_are_unique_hex() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }

    #[test]
    fn parses_cookie_header() {
        let mut h = HeaderMap::new();
        let tok = "a".repeat(64);
        h.insert(COOKIE, HeaderValue::from_str(&format!("theme=dark; sid={tok}; x=1")).unwrap());
        assert_eq!(session_cookie_from_headers(&h).as_deref(), Some(tok.as_str()));
        h.insert(COOKIE, HeaderValue::from_static("sid=garbage"));
        assert!(session_cookie_from_headers(&h).is_none());
    }
}
