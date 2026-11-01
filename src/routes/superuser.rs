//! `/superuser`: the page for whoever runs the server (superusers only,
//! `users.superuser`). Every account, newest first, a page at a time, with
//! the actions on one: a temporary password and two-factor off (#635,
//! `owner.rs`), reset e-mail verification (#220); and with
//! `SKRONGLE_SIGNUP=invite`, the invitation links (#634, `signup.rs`); and
//! with Skrongle AI, everyone's spend today against the daily cap (#623);
//! while a material change of the terms is coming, a notice and the button
//! that e-mails it to every account (#966, `terms.rs`); and on a server that
//! sends mail, the open content reports and copyright notices with Forward
//! to owner and Resolve (#968, `routes/reports.rs`);
//! and on the hosted site, moderation (#967, `moderation.rs`): the course
//! table (#1034, `superuser/courses.rs`) with the flag of each course and
//! what to do about it, suspend or delete an account, block an address;
//! privacy requests that came by e-mail (#981, `superuser/privacy.rs`);
//! the breach tools: sign everyone out, e-mail a notice (#1068, `superuser/breach.rs`);
//! and a card that opens Release, the waitlist (#934, `superuser/release.rs`).
//! Anyone else gets a 404, as if the page did not exist; the account menu
//! links here only for superusers.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser};
use crate::error::AppError;
use crate::profile;
use crate::AppState;

pub mod breach;
pub mod courses;
pub mod privacy;
pub mod release;

#[derive(Template)]
#[template(path = "superuser.html")]
pub struct SuperuserTemplate {
    pub user: Option<CurrentUser>,
    /// What the last action did, or why it could not.
    pub notice: Option<String>,
    pub error: Option<String>,
    /// Dev mode with the dev mailbox: where a new confirmation link landed.
    pub inbox: Option<String>,
    /// `SKRONGLE_SIGNUP`: `open`, `invite` or `closed`.
    pub mode: &'static str,
    /// The server can send e-mail: without it there is no confirmation
    /// link to send, so Reset email verification is not offered (#633).
    pub mail: bool,
    /// An invitation link just made: shown this once.
    pub new_link: Option<String>,
    /// A temporary password just set: the account's address and the
    /// password, shown this once.
    pub temp_password: Option<(String, String)>,
    /// Open invitations: id, made, expires.
    pub invites: Vec<(i64, String, String)>,
    /// This page of accounts.
    pub list: Vec<AccountRow>,
    pub total: i64,
    pub page: i64,
    pub pages: i64,
    pub per_page: i64,
    /// The newer and older pages, when there are any.
    pub prev: Option<String>,
    pub next: Option<String>,
    /// Skrongle AI today, everyone: on a server with Skrongle AI only.
    pub ai_today: Option<AiTodayView>,
    /// Accounts still to e-mail about a coming change of the terms: above 0,
    /// the page says the terms have changed and offers Send.
    pub terms_owed: i64,
    /// Open content reports and copyright notices, oldest first (#968).
    pub reports: Vec<ReportRow>,
    /// Moderation (#967): on the hosted site only, not Skrongle mini.
    pub moderate: bool,
    /// The course table (#1034): with `moderate` only.
    pub courses: Option<courses::CourseTable>,
    /// Blocked addresses and ranges: id, range.
    pub blocked: Vec<(i64, String)>,
    /// The privacy request log (#981): with `moderate` only.
    pub privacy_log: Vec<privacy::LogRow>,
    /// The breach tools (#1068): with `moderate` only.
    pub breach: Option<breach::Card>,
}

/// One open report on the page (#968, `content_reports::Report`).
pub struct ReportRow {
    pub id: i64,
    pub received: String,
    pub reason: String,
    pub reason_label: String,
    /// The course's title, or '' when the notice names no course here.
    pub course_title: String,
    pub course_id: String,
    pub location: String,
    pub name: String,
    pub email: String,
    pub work: String,
    pub details: String,
    /// When it was forwarded to the owner, or ''.
    pub forwarded: String,
}

impl ReportRow {
    fn new(r: crate::content_reports::Report) -> ReportRow {
        ReportRow {
            id: r.id,
            received: profile::long_date(r.created_at),
            reason_label: crate::content_reports::reason_label(&r.reason),
            reason: r.reason,
            course_title: r.course_title,
            course_id: r.course_id.unwrap_or_default(),
            location: r.location,
            name: r.name,
            email: r.email,
            work: r.work,
            details: r.details,
            forwarded: r.forwarded_at.map(profile::long_date).unwrap_or_default(),
        }
    }
}

/// Everyone's Skrongle AI spend today against the daily cap (#623): the one
/// place the server-wide figure (`ai_spend_days`) is shown. Learners only
/// ever see their own month, as a bar without dollars (#315).
#[derive(Clone, Debug)]
pub struct AiTodayView {
    /// "$3.20" and "$10.00".
    pub spent: String,
    pub cap: String,
    /// Of the cap, what today's spend used, 0–100.
    pub percent_used: u8,
}

impl AiTodayView {
    fn new(spent: i64, cap: i64) -> AiTodayView {
        let percent_used = if cap <= 0 { 100 } else { (spent.clamp(0, cap) * 100 / cap) as u8 };
        AiTodayView { spent: crate::metering::dollars(spent), cap: crate::metering::dollars(cap), percent_used }
    }

    /// The bar's colour, as on the learner's card: `low` past four fifths, `out` at the cap.
    pub fn level(&self) -> &'static str {
        match self.percent_used {
            100.. => "out",
            80..=99 => "low",
            _ => "ok",
        }
    }
}

pub struct AccountRow {
    pub email: String,
    pub created: String,
    /// '' when no session is on record.
    pub last_sign_in: String,
    pub superuser: bool,
    pub suspended: bool,
    /// The superuser's own row: no Suspend or Delete account, which would
    /// lock them out (#967).
    pub me: bool,
}

/// The page, for a superuser, at `page` (clamped to what exists). `site`
/// is where links in e-mail point (`verify::public_base`).
async fn load(state: &AppState, user: CurrentUser, page: i64, site: &str) -> Result<SuperuserTemplate, AppError> {
    load_at(state, user, page, &courses::CourseQuery::default(), site).await
}

/// The page at accounts page `page` and the course table at `course`.
async fn load_at(state: &AppState, user: CurrentUser, page: i64, course: &courses::CourseQuery, site: &str) -> Result<SuperuserTemplate, AppError> {
    let now = crate::db::now();
    let meter = Arc::clone(&state.meter);
    let (invites, accounts, spent, reports) = state
        .db
        .call(move |c| {
            Ok((
                crate::signup::open_invites(c, now)?,
                crate::signup::accounts(c, page)?,
                if meter.hosted() { Some(meter.spent_today(c)?) } else { None },
                crate::content_reports::open(c)?,
            ))
        })
        .await?;
    let terms_owed = crate::terms::unannounced(state).await?;
    let moderate = moderates(&user);
    let me = user.email.to_lowercase();
    let blocked = if moderate { state.db.call(|c| crate::moderation::blocked(c)).await? } else { Vec::new() };
    let course_table = if moderate { Some(courses::load(state, &user, course, page, site).await?) } else { None };
    let privacy_log = if moderate { privacy::log(state).await? } else { Vec::new() };
    let breach = if moderate { Some(breach::card(state).await?) } else { None };
    let course_params = if moderate { course.params() } else { String::new() };
    let link = |p: i64| crate::url(&format!("/superuser?page={p}{course_params}"));
    Ok(SuperuserTemplate {
        user: Some(user),
        notice: None,
        error: None,
        inbox: None,
        mode: state.signup.as_str(),
        mail: state.caps().mail,
        new_link: None,
        temp_password: None,
        invites: invites.into_iter().map(|i| (i.id, profile::long_date(i.created_at), profile::long_date(i.expires_at))).collect(),
        list: accounts
            .list
            .into_iter()
            .map(|a| AccountRow {
                me: a.email.to_lowercase() == me,
                email: a.email,
                created: profile::long_date(a.created_at),
                last_sign_in: a.last_sign_in.map(profile::long_date).unwrap_or_default(),
                superuser: a.superuser,
                suspended: a.suspended,
            })
            .collect(),
        total: accounts.total,
        page: accounts.page,
        pages: accounts.pages,
        per_page: crate::signup::ACCOUNTS_PER_PAGE,
        prev: (accounts.page > 1).then(|| link(accounts.page - 1)),
        next: (accounts.page < accounts.pages).then(|| link(accounts.page + 1)),
        ai_today: spent.map(|s| AiTodayView::new(s, state.meter.cfg.daily_cap)),
        terms_owed,
        reports: reports.into_iter().map(ReportRow::new).collect(),
        moderate,
        courses: course_table,
        blocked,
        privacy_log,
        breach,
    })
}

#[derive(Deserialize)]
pub struct ShowQuery {
    pub page: Option<i64>,
    /// Back from an action: `reset`, `unknown`, `revoked` or
    /// `two-factor-off`, about `email`; or `terms-sent`, `n` e-mails; or a moderation
    /// action (`moderated`).
    pub did: Option<String>,
    pub email: Option<String>,
    pub n: Option<i64>,
    /// More counts of a breach action (#1068).
    pub m: Option<i64>,
    pub k: Option<i64>,
    /// A moderation action's course id or address range.
    pub about: Option<String>,
    /// The course table's search, filter and page (#1034).
    pub course_q: Option<String>,
    pub course_show: Option<String>,
    pub course_page: Option<i64>,
}

/// GET /superuser?page=N
pub async fn show(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Query(q): Query<ShowQuery>) -> Result<Response, AppError> {
    if !user.superuser {
        return Err(AppError::NotFound);
    }
    let course = courses::CourseQuery::new(q.course_q.as_deref(), q.course_show.as_deref(), q.course_page);
    let mut t = load_at(&state, user, q.page.unwrap_or(1), &course, &crate::verify::public_base(&state, &headers)).await?;
    let email = q.email.as_deref().unwrap_or_default();
    match q.did.as_deref() {
        Some("reset") => {
            t.notice = Some(crate::t1("superuser.reset_done", "email", email));
            if state.dev_inbox.is_some() {
                t.inbox = Some(crate::url(&format!("/dev/inbox/latest?to={}", crate::urlenc::urlencode(email))));
            }
        }
        Some("unknown") => t.error = Some(crate::t1("superuser.unknown", "email", format!("{email:?}"))),
        Some("revoked") => t.notice = Some(crate::t("superuser.invite_revoked")),
        Some("two-factor-off") => t.notice = Some(crate::t1("superuser.two_factor_off_done", "email", email)),
        Some("terms-sent") => t.notice = Some(crate::tn("superuser.terms_sent", q.n.unwrap_or(0))),
        Some("report-forwarded") => t.notice = Some(crate::t("reports.superuser.forwarded")),
        Some("report-resolved") => t.notice = Some(crate::t("reports.superuser.resolved")),
        Some("report-no-owner") => t.error = Some(crate::t("reports.superuser.no_owner")),
        Some("report-unknown") => t.error = Some(crate::t("reports.superuser.unknown")),
        Some("owner-emailed") => {
            t.notice = Some(crate::t1("superuser.courses.owner_emailed", "id", q.about.as_deref().unwrap_or_default()));
            if state.dev_inbox.is_some() {
                t.inbox = Some(crate::url(&format!("/dev/inbox/latest?to={}", crate::urlenc::urlencode(email))));
            }
        }
        Some(did) if did.starts_with("breach-") => {
            if let Some((notice, error)) = breach::notice(did, q.n.unwrap_or(0), q.m.unwrap_or(0), q.k.unwrap_or(0)) {
                (t.notice, t.error) = (notice, error);
            }
        }
        Some(did) => moderated(&mut t, did, email, q.about.as_deref().unwrap_or_default()),
        None => {}
    }
    Ok(Page(t).into_response())
}

/// Back to the page after an action: what it did, about which address.
fn done(did: &str, email: &str) -> Response {
    let to = format!("/superuser?did={did}&email={}", crate::urlenc::urlencode(email));
    Redirect::to(&crate::url(&to)).into_response()
}

#[derive(Deserialize)]
pub struct AccountForm {
    #[serde(default)]
    pub email: String,
}

/// POST /superuser/reset-email-verification (email=…): the account with
/// that address is unconfirmed again and gets a new link. Superusers only:
/// anyone else gets a 404.
pub async fn reset_email_verification(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    // Without mail there is no link to send: the action is not offered (#633).
    if !user.superuser || !state.caps().mail {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let did = match crate::verify::reset(&state, &headers, &email).await? {
        Some(target) => {
            tracing::info!("superuser {} reset the e-mail verification of user {target}", user.id);
            "reset"
        }
        None => "unknown",
    };
    Ok(done(did, &email))
}

/// POST /superuser/invites: a new invitation link (`SKRONGLE_SIGNUP=invite`),
/// shown on the page this once; only its hash is kept. Superusers only, and
/// only in invite mode: a 404 otherwise.
pub async fn make_invite(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap) -> Result<Response, AppError> {
    if !user.superuser || state.signup != crate::signup::Mode::Invite {
        return Err(AppError::NotFound);
    }
    let uid = user.id;
    let token = state.db.call(move |c| crate::signup::make(c, uid, crate::db::now())).await?;
    tracing::info!("superuser {uid} made an invitation link");
    let site = crate::verify::public_base(&state, &headers);
    let link = format!("{site}/register?invite={token}");
    let mut t = load(&state, user, 1, &site).await?;
    t.new_link = Some(link);
    // The link is a key to the server: not to be cached anywhere.
    Ok(([(header::CACHE_CONTROL, "no-store")], Page(t)).into_response())
}

#[derive(Deserialize)]
pub struct RevokeInviteForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /superuser/invites/revoke (id=…): an open invitation stops working.
/// Superusers only: a 404 otherwise.
pub async fn revoke_invite(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<RevokeInviteForm>) -> Result<Response, AppError> {
    if !user.superuser {
        return Err(AppError::NotFound);
    }
    let id = form.id;
    if state.db.call(move |c| crate::signup::revoke(c, id)).await? {
        tracing::info!("superuser {} revoked invitation {id}", user.id);
    }
    Ok(Redirect::to(&crate::url("/superuser?did=revoked")).into_response())
}

/// POST /superuser/temp-password (email=…): the account gets a generated
/// temporary password (`owner.rs`, #635), shown on the page this once,
/// which it must change at its next sign-in; its sessions end (an owner
/// resetting their own keeps this one). Superusers only: a 404 otherwise.
/// The log line never holds the password.
pub async fn set_temp_password(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    headers: HeaderMap,
    Form(form): Form<AccountForm>,
) -> Result<Response, AppError> {
    if !user.superuser {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    let password = crate::owner::temp_password();
    let p = password.clone();
    let hash = tokio::task::spawn_blocking(move || auth::hash_password(&p)).await.map_err(|e| AppError::Internal(e.to_string()))??;
    let keep = if target == user.id { token } else { None };
    let ended = state.db.call(move |c| crate::owner::set_temporary_password(c, target, &hash, keep.as_deref())).await?;
    tracing::info!("superuser {} set a temporary password for user {target} ({ended} sessions signed out)", user.id);
    let mut t = load(&state, user, 1, &crate::verify::public_base(&state, &headers)).await?;
    t.temp_password = Some((email, password));
    // The password is a key to the account: not to be cached anywhere.
    Ok(([(header::CACHE_CONTROL, "no-store")], Page(t)).into_response())
}

/// POST /superuser/two-factor-off (email=…): the account's two-factor
/// sign-in is turned off (`owner::disable_two_factor`). Superusers only: a
/// 404 otherwise.
pub async fn turn_off_two_factor(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    if !user.superuser {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    state.db.call(move |c| crate::owner::disable_two_factor(c, target)).await?;
    tracing::info!("superuser {} turned off two-factor sign-in for user {target}", user.id);
    Ok(done("two-factor-off", &email))
}

/// POST /superuser/terms-announce: e-mail the coming material change of the
/// terms to every account that has not had it (`terms::announce_upcoming`).
/// Superusers only, on a server that can send mail: a 404 otherwise.
pub async fn announce_terms(State(state): State<Arc<AppState>>, user: CurrentUser) -> Result<Response, AppError> {
    if !user.superuser || !state.caps().mail {
        return Err(AppError::NotFound);
    }
    let n = crate::terms::announce_upcoming(&state).await?;
    tracing::info!("superuser {} sent the terms change e-mail to {n} account(s)", user.id);
    Ok(Redirect::to(&crate::url(&format!("/superuser?did=terms-sent&n={n}"))).into_response())
}

// ---------- moderation (#967) ----------

/// May this user moderate? A superuser on the hosted site: Skrongle mini's
/// owner has the database.
fn moderates(user: &CurrentUser) -> bool {
    user.superuser && !crate::is_mini()
}

/// The notice or error a moderation action left (`?did=`), about an
/// account (`email`) or a course or address (`about`). An unknown key shows nothing.
fn moderated(t: &mut SuperuserTemplate, did: &str, email: &str, about: &str) {
    if let Some((notice, error)) = privacy::notice(did, email) {
        (t.notice, t.error) = (notice, error);
        return;
    }
    let ok = |key: &str, name: &str, value: &str| Some(crate::t1(key, name, value));
    let (notice, error) = match did {
        "taken-down" => (ok("superuser.mod.taken_down", "id", about), None),
        "put-back" => (ok("superuser.mod.put_back", "id", about), None),
        "no-course" => (None, ok("superuser.mod.no_course", "id", &format!("{about:?}"))),
        "overridden" => (ok("superuser.courses.overridden", "id", about), None),
        "override-cleared" => (ok("superuser.courses.override_cleared", "id", about), None),
        "no-note" => (None, Some(crate::t("superuser.courses.no_note"))),
        "course-deleted" => (ok("superuser.courses.deleted", "id", about), None),
        "course-busy" => (None, ok("superuser.courses.busy", "id", about)),
        "no-mail-text" => (None, Some(crate::t("superuser.courses.no_mail_text"))),
        "no-owner-address" => (None, ok("superuser.courses.no_owner_address", "id", about)),
        "suspended" => (ok("superuser.mod.suspended", "email", email), None),
        "unsuspended" => (ok("superuser.mod.unsuspended", "email", email), None),
        "deleted" => (ok("superuser.mod.deleted", "email", email), None),
        "yourself" => (None, Some(crate::t("superuser.mod.yourself"))),
        "subscribed" => (None, ok("superuser.mod.subscribed", "email", email)),
        "blocked" => (ok("superuser.mod.blocked", "ip", about), None),
        "unblocked" => (ok("superuser.mod.unblocked", "ip", about), None),
        "bad-ip" => (None, ok("superuser.mod.bad_ip", "ip", &format!("{about:?}"))),
        _ => (None, None),
    };
    t.notice = notice.or(t.notice.take());
    t.error = error.or(t.error.take());
}

/// Back to the page after a moderation action about a course or address.
fn done_about(did: &str, about: &str) -> Response {
    let to = format!("/superuser?did={did}&about={}", crate::urlenc::urlencode(about));
    Redirect::to(&crate::url(&to)).into_response()
}

#[derive(Deserialize)]
pub struct CourseForm {
    #[serde(default)]
    pub course: String,
}

/// POST /superuser/courses/take-down (course=…): a made course stops
/// opening for everyone; its owner sees why. Superusers on the hosted site
/// only: a 404 otherwise.
pub async fn take_down_course(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<CourseForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let id = form.course.trim().to_string();
    let cid = id.clone();
    if !state.db.call(move |c| crate::moderation::take_down(c, &cid, crate::db::now())).await? {
        return Ok(done_about("no-course", &id));
    }
    state.courses.invalidate(&id);
    tracing::info!("superuser {} took down course {id}", user.id);
    Ok(done_about("taken-down", &id))
}

/// POST /superuser/courses/put-back (course=…): a course taken down opens
/// again, for its owner and everyone it is shared with.
pub async fn put_back_course(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<CourseForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let id = form.course.trim().to_string();
    let cid = id.clone();
    if !state.db.call(move |c| crate::moderation::put_back(c, &cid)).await? {
        return Ok(done_about("no-course", &id));
    }
    state.courses.invalidate(&id);
    tracing::info!("superuser {} put course {id} back", user.id);
    Ok(done_about("put-back", &id))
}

/// The account `email` names, unless it is the superuser's own (suspending
/// or deleting that would lock them out): the redirect to show otherwise.
async fn other_account(state: &AppState, user: &CurrentUser, email: &str) -> Result<Result<i64, Response>, AppError> {
    let e = email.to_string();
    Ok(match state.db.call(move |c| crate::owner::find(c, &e)).await? {
        None => Err(done("unknown", email)),
        Some(id) if id == user.id => Err(done("yourself", email)),
        Some(id) => Ok(id),
    })
}

/// POST /superuser/suspend (email=…): the account is signed out
/// everywhere, cannot sign in, and its courses are not shared out until the
/// suspension is lifted.
pub async fn suspend(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let target = match other_account(&state, &user, &email).await? {
        Ok(id) => id,
        Err(back) => return Ok(back),
    };
    if let Some(ended) = state.db.call(move |c| crate::moderation::suspend(c, target, crate::db::now())).await? {
        tracing::info!("superuser {} suspended user {target} ({ended} sessions signed out)", user.id);
    }
    Ok(done("suspended", &email))
}

/// POST /superuser/unsuspend (email=…): the suspension is lifted.
pub async fn unsuspend(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    if state.db.call(move |c| crate::moderation::lift(c, target)).await? {
        tracing::info!("superuser {} lifted the suspension of user {target}", user.id);
    }
    Ok(done("unsuspended", &email))
}

/// POST /superuser/delete-account (email, reason): the account and
/// everything it owns are deleted, as from its own profile (a terms breach,
/// a child's data, a privacy request). Not undone. Refused while a
/// subscription would keep billing. The reason goes in the privacy log (#981).
pub async fn delete_account(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<privacy::RequestForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let Some(reason) = crate::privacy_requests::reason(&form.reason) else { return Ok(done("no-reason", &email)) };
    let target = match other_account(&state, &user, &email).await? {
        Ok(id) => id,
        Err(back) => return Ok(back),
    };
    if state.db.call(move |c| crate::privacy::has_live_subscription(c, target)).await? {
        return Ok(done("subscribed", &email));
    }
    let deleted = super::profile::erase_account(&state, target).await?;
    let su = user.id;
    state.db.call(move |c| crate::privacy_requests::record(c, su, crate::privacy_requests::Action::DeleteAccount, &reason, crate::db::now())).await?;
    tracing::info!("superuser {} deleted user {target} ({} courses, {} themes)", user.id, deleted.courses.len(), deleted.themes.len());
    Ok(done("deleted", &email))
}

#[derive(Deserialize)]
pub struct BlockForm {
    #[serde(default)]
    pub ip: String,
}

/// POST /superuser/block-ip (ip=…): every request from the address or
/// range gets a plain 403 (`moderation::block`).
pub async fn block_ip(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<BlockForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let Some(net) = crate::moderation::Net::parse(&form.ip) else {
        return Ok(done_about("bad-ip", form.ip.trim()));
    };
    let st = Arc::clone(&state);
    state
        .db
        .call(move |c| {
            crate::moderation::block_net(c, net, crate::db::now())?;
            st.blocked.reload(c)
        })
        .await?;
    tracing::info!("superuser {} blocked {net}", user.id);
    Ok(done_about("blocked", &net.to_string()))
}

#[derive(Deserialize)]
pub struct UnblockForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /superuser/unblock-ip (id=…): a block is removed.
pub async fn unblock_ip(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<UnblockForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let (id, st) = (form.id, Arc::clone(&state));
    let removed = state
        .db
        .call(move |c| {
            let r = crate::moderation::unblock(c, id)?;
            st.blocked.reload(c)?;
            Ok(r)
        })
        .await?;
    let Some(net) = removed else { return Ok(Redirect::to(&crate::url("/superuser")).into_response()) };
    tracing::info!("superuser {} removed the block on {net}", user.id);
    Ok(done_about("unblocked", &net))
}
