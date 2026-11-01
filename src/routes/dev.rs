//! `/dev/*`: local testing routes, mounted only in dev mode (`crate::dev`).

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser, MaybeUser};
use crate::dev;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct LoginParams {
    /// A course to open (`?open` alone: the seed course).
    pub open: Option<String>,
    /// `dev` (or the older `fake`): the simulated provider's learner keys
    /// for all three providers (`dev_ai::LEARNER_KEYS`), so the AI forms are
    /// unlocked.
    pub keys: Option<String>,
    /// Where to land, an app path such as `/courses/generate`.
    pub next: Option<String>,
    /// `old`: leave the dev user with only an older Terms version accepted,
    /// so the material "terms updated" notice shows; `minor`: one a minor
    /// change behind, so the dismissable notice shows (#966). The next login
    /// without it accepts again.
    pub terms: Option<String>,
    /// With `open`: `1` shows the course's first-visit settings wizard on
    /// its home page again (#574). Without it the course counts as set up,
    /// so a login lands on the home page itself.
    pub wizard: Option<String>,
}

/// Only app-relative paths, so `next` can't send the browser elsewhere.
fn landing(next: Option<&str>) -> String {
    match next {
        Some(n) if super::terms::is_app_path(n) => crate::url(n),
        _ => crate::url("/"),
    }
}

async fn dev_user(state: &AppState) -> Result<i64, AppError> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || dev::ensure_user(&db))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::Internal)
}

/// GET /dev/login[?open=spanish[&wizard=1]][&keys=fake|dev][&next=/courses/generate]
/// Log in as the dev user (created on first use).
pub async fn login(State(state): State<Arc<AppState>>, Query(q): Query<LoginParams>) -> Result<Response, AppError> {
    let uid = dev_user(&state).await?;
    if let Some(open) = q.open.as_deref() {
        let id = if open.is_empty() { dev::SEED_COURSE.to_string() } else { open.to_string() };
        let cid = id.clone();
        let ok = state.courses.is_builtin(&id)
            || state.db.call(move |c| crate::courses::course(c, &cid)).await?.is_some_and(|r| r.owner_id == uid && !r.is_generating());
        if !ok || state.courses.get(&state.db, &id).await?.is_none() {
            return Err(AppError::BadRequest(format!("no course '{id}' the dev user can open")));
        }
        let wizard = q.wizard.as_deref() == Some("1");
        state
            .db
            .call(move |c| {
                crate::courses::open_project(c, uid, &id)?;
                if wizard {
                    c.execute("UPDATE projects SET set_up = 0 WHERE user_id = ?1 AND course_id = ?2", rusqlite::params![uid, id])?;
                } else {
                    crate::courses::set_project_settings(c, uid, &id, &crate::courses::SettingsChange::default())?;
                }
                Ok(())
            })
            .await?;
    }
    match q.keys.as_deref() {
        None | Some("") => {}
        Some("fake" | "dev") => {
            let vault = state.vault.clone();
            state.db.call(move |c| dev::save_dev_keys(c, &vault, uid)).await?;
        }
        Some(other) => return Err(AppError::BadRequest(format!("keys={other}: keys=fake or keys=dev"))),
    }
    match q.terms.as_deref() {
        None | Some("") => {}
        Some(which @ ("old" | "minor")) => {
            let version = if which == "old" { dev::OLD_TERMS_VERSION } else { dev::MINOR_TERMS_VERSION };
            state
                .db
                .call(move |c| {
                    c.execute("DELETE FROM terms_acceptances WHERE user_id = ?1", [uid])?;
                    crate::terms::accept(c, uid, version)
                })
                .await?;
        }
        Some(other) => return Err(AppError::BadRequest(format!("terms={other}: use terms=old (a material change) or terms=minor"))),
    }
    let token = state.db.call(move |c| auth::create_session(c, uid)).await?;
    tracing::info!("dev: logged in as '{}'", dev::DEV_EMAIL);
    Ok(([auth::set_session_cookie(&token, state.secure_cookies)], Redirect::to(&landing(q.next.as_deref()))).into_response())
}

#[derive(Deserialize)]
pub struct ForgetGoogleForm {
    #[serde(default)]
    pub email: String,
}

/// POST /dev/google/forget (email): for testing sign-in with Google, from
/// the login page's dev-mode line. The account signed up or connected with
/// Google under that address forgets Google: a Google-only account is
/// deleted, a password account is disconnected, so the next Google sign-in
/// with the address starts over at the confirm step. Dev mode and the
/// local-only check every `/dev` route gets are the whole gate, like
/// `/dev/login`: there is no user to check on the login page.
pub async fn google_forget(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Form(f): Form<ForgetGoogleForm>) -> Result<Response, AppError> {
    let email = f.email.trim().to_string();
    if email.is_empty() {
        return Err(AppError::BadRequest("which e-mail address?".into()));
    }
    let e = email.clone();
    let found = state.db.call(move |c| dev::google_account_by_email(c, &e)).await?;
    let forgot = match found {
        dev::GoogleAccount::None => "none",
        dev::GoogleAccount::Linked(id) => {
            state.db.call(move |c| dev::unlink_google(c, id)).await?;
            tracing::info!("dev: Google disconnected from user {id} ({email})");
            "unlinked"
        }
        dev::GoogleAccount::GoogleOnly(id) => {
            // As /profile/delete: what lives in memory goes first.
            state.jobs.drop_user(id);
            state.tutors.drop_user(id);
            state.games.drop_user(id);
            let deleted = state.db.call(move |c| crate::privacy::delete_account(c, id)).await?.unwrap_or_default();
            for c in &deleted.courses {
                state.courses.invalidate(c);
            }
            for t in &deleted.themes {
                state.themes.forget(t);
                state.courses.forget_theme(t);
            }
            state.exports.remove(&deleted.export_files);
            tracing::info!("dev: Google-only user {id} ({email}) deleted");
            if user.as_ref().is_some_and(|u| u.id == id) {
                return Ok(([crate::auth::clear_session_cookie()], Redirect::to(&crate::url("/login?forgot=deleted"))).into_response());
            }
            "deleted"
        }
    };
    Ok(Redirect::to(&format!("{}?forgot={forgot}", crate::url("/login"))).into_response())
}

#[derive(Deserialize)]
pub struct ThemeBody {
    /// Path -> content: `theme.json` and `pictures/*.svg`, as a made theme stores them.
    pub files: std::collections::BTreeMap<String, String>,
}

/// POST /dev/theme {"files": {...}}: store a made theme for whoever is signed
/// in, as the theme editor (#64) will. `{"id": "user-<n>"}`, or 400 with why.
pub async fn theme(State(state): State<Arc<AppState>>, user: CurrentUser, Json(body): Json<ThemeBody>) -> Result<Response, AppError> {
    let files = body.files.into_iter().collect();
    match state.themes.create(&state.db, user.id, files).await {
        Ok(id) => Ok(Json(serde_json::json!({ "id": id })).into_response()),
        Err(e) => Err(AppError::ApiBadRequest(e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct VocabBody {
    /// The lesson whose words to collect.
    pub lesson: String,
    /// How many days ago they were collected: 1 or more makes them due today.
    pub days_ago: i64,
}

/// POST /dev/vocab {"lesson": slug, "days_ago": n}: collect a lesson's words
/// for whoever is signed in as if `n` days ago, so a spec can see words due
/// (#1360) without waiting a day. `{"collected": n}`.
pub async fn vocab(State(state): State<Arc<AppState>>, p: auth::Project, Json(body): Json<VocabBody>) -> Result<Response, AppError> {
    let lesson = p.course.lesson_by_slug(&body.lesson).ok_or(AppError::NotFound)?;
    let (uid, tz, cid, lid) = (p.user.id, p.user.tz, p.course_id().to_string(), lesson.id.clone());
    let words: Vec<String> = lesson.vocabulary.iter().map(|v| v.term.clone()).collect();
    let t = crate::db::now() - body.days_ago.clamp(0, 400) * 86_400;
    let n = state
        .db
        .call(move |c| crate::vocab::record_learned_at(c, uid, &cid, &lid, &words.iter().map(String::as_str).collect::<Vec<_>>(), t, tz))
        .await?
        .len();
    Ok(Json(serde_json::json!({ "collected": n })).into_response())
}

#[derive(Deserialize)]
pub struct ResetParams {
    pub next: Option<String>,
}

/// GET /dev/reset[?next=…]: wipe the dev user's progress and own courses.
pub async fn reset(State(state): State<Arc<AppState>>, Query(q): Query<ResetParams>) -> Result<Response, AppError> {
    let uid = dev_user(&state).await?;
    let gone = state.db.call(move |c| dev::reset_user(c, uid)).await?;
    for id in &gone {
        state.courses.invalidate(id);
    }
    // Through Themes::delete, so its cache and the themed courses forget them too.
    for t in state.themes.made_by(&state.db, uid).await? {
        state.themes.delete(&state.db, &state.courses, uid, &t.theme.id).await?;
    }
    tracing::info!("dev: reset '{}' ({} courses deleted)", dev::DEV_EMAIL, gone.len());
    let next = q.next.unwrap_or_else(|| "/courses?notice=dev-reset".into());
    Ok(Redirect::to(&landing(Some(&next))).into_response())
}

/// JSON, unless a browser asks for a page (`Accept: text/html`); `format`
/// (`json`, `html`) overrides.
fn wants_html(headers: &axum::http::HeaderMap, format: Option<&str>) -> bool {
    match format {
        Some("json") => false,
        Some("html") => true,
        _ => headers.get(axum::http::header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("text/html")),
    }
}

/// GET /dev/mail: the newest 50 outbox rows (bodies kept in dev mode),
/// newest first, so tests can read a message the app sent. JSON, unless a
/// browser asks for a page (`Accept: text/html`, or `?format=html`): the
/// dev mailbox page with its Outbox tab showing (see [`inbox`]).
/// `?format=json` forces JSON.
pub async fn mail(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    if !wants_html(&headers, q.format.as_deref()) {
        let rows = state.db.call(|c| crate::mail::recent(c, 50)).await?;
        return Ok(Json(serde_json::json!({ "providers": state.mail.providers, "messages": rows })).into_response());
    }
    box_page(&state, user, "mail", "out", q.to.as_deref()).await
}

#[derive(Deserialize)]
pub struct MailSendForm {
    /// Where to send it; empty: the signed-in user's address, else the dev user's.
    #[serde(default)]
    pub to: String,
}

/// POST /dev/mail/send (to=…): queue a test message through the configured
/// providers (`SKRONGLE_MAIL_PROVIDERS`) to watch one leave, then back to /dev/mail.
pub async fn mail_send(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Form(f): Form<MailSendForm>,
) -> Result<Response, AppError> {
    let to = match f.to.trim() {
        "" => user.as_ref().map(|u| u.email.clone()).unwrap_or_else(|| dev::DEV_EMAIL.to_string()),
        t => t.to_string(),
    };
    let app = crate::APP_NAME;
    let now = crate::db::now();
    let text = format!("A test message from {app} in dev mode, queued at {now} (unix time).\n\nIf you can read this, the mail provider took it.");
    let html = format!("<p>A test message from <strong>{app}</strong> in dev mode, queued at {now} (unix time).</p><p>If you can read this, the mail provider took it.</p>");
    let mail = crate::mail::Mail::new(&to, &format!("{app} test message"), &text, "dev-test").map_err(AppError::BadRequest)?.html(&html);
    let id = state.mail.queue(&state.db, mail).await?;
    tracing::info!("dev: queued test mail #{id}");
    Ok(done(&headers, "/dev/mail"))
}

/// GET /dev/sms: the newest 50 text message outbox rows (bodies kept in dev
/// mode), newest first, so tests can read a code the app texted. JSON,
/// unless a browser asks for a page: the dev mailbox's Text section with its
/// Outbox tab showing (see [`inbox`]).
pub async fn sms(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    if !wants_html(&headers, q.format.as_deref()) {
        let rows = state.db.call(|c| crate::sms::recent(c, 50)).await?;
        return Ok(Json(serde_json::json!({ "providers": state.sms.providers, "messages": rows })).into_response());
    }
    box_page(&state, user, "sms", "out", q.to.as_deref()).await
}

#[derive(Deserialize)]
pub struct SmsSendForm {
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub text: String,
}

/// POST /dev/sms/send (form `to`, `text`): the Text outbox's "Send a test
/// text", through `send_sms` like `POST /dev/sms`; then back to /dev/sms
/// (204 for the page's own posts). 400 or 429 with why, as text.
pub async fn sms_form_send(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Form(f): Form<SmsSendForm>,
) -> Result<Response, AppError> {
    use crate::sms::SmsError;
    let text = if f.text.trim().is_empty() { format!("A test text from {} in dev mode.", crate::APP_NAME) } else { f.text };
    match state.sms.send_sms(&state.db, "dev-test", &f.to, &text, user.map(|u| u.id)).await {
        Ok(id) => {
            tracing::info!("dev: queued test sms #{id}");
            Ok(done(&headers, "/dev/sms"))
        }
        Err(SmsError::Invalid(why)) => Err(AppError::BadRequest(why)),
        Err(e @ SmsError::Country) => Err(AppError::BadRequest(e.to_string())),
        Err(SmsError::RateLimited { retry_after }) => {
            Ok((axum::http::StatusCode::TOO_MANY_REQUESTS, format!("Too many texts to that number; try again in {retry_after} s, or delete some from the outbox.")).into_response())
        }
        Err(SmsError::Db(e)) => Err(e),
    }
}

#[derive(Deserialize)]
pub struct SmsBody {
    pub to: String,
    pub text: String,
}

/// POST /dev/sms {"to": "+44…", "text": "…"}: queue a text through
/// `sms::Texter::send_sms` (purpose `dev-test`, for whoever is signed in, if
/// anyone), to try a provider configuration or the limits by hand.
/// `{"id": n}`, 400 with why, or 429 with `retry_after`.
pub async fn sms_send(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Json(body): Json<SmsBody>) -> Result<Response, AppError> {
    use crate::sms::SmsError;
    match state.sms.send_sms(&state.db, "dev-test", &body.to, &body.text, user.map(|u| u.id)).await {
        Ok(id) => Ok(Json(serde_json::json!({ "id": id })).into_response()),
        Err(SmsError::Invalid(why)) => Err(AppError::ApiBadRequest(why)),
        Err(e @ SmsError::Country) => Err(AppError::ApiBadRequest(e.to_string())),
        Err(SmsError::RateLimited { retry_after }) => Ok((
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "too many text messages", "retry_after": retry_after })),
        )
            .into_response()),
        Err(SmsError::Db(e)) => Err(e),
    }
}

// ---------- the dev mailbox page: e-mail and texts, inbox and outbox ----------
//
// One page with two sections, E-mail (`/dev/inbox`, `/dev/mail`: the dev
// mailbox, mail/sink.rs, and the mail outbox) and Text (`/dev/phone`,
// `/dev/sms`: the dev phone, sms/phone.rs, and the SMS outbox), each with
// an Inbox and an Outbox tab. Every box is a list like a mail client's (the
// texts styled like a messaging app's): unopened rows in bold, archived ones
// struck out at the bottom, Delete removes a row for good. The page polls
// `/dev/inbox/rows` (static/devbox.js) so new messages, state changes and
// the buttons show without a reload. Opened and archived are `dev::Marks`,
// in memory.

/// `dev::Marks` boxes.
const MAIL_IN: &str = "mail-in";
const MAIL_OUT: &str = "mail-out";
const SMS_IN: &str = "sms-in";
const SMS_OUT: &str = "sms-out";

/// One row of a dev box's table.
pub struct BoxRow {
    pub id: i64,
    /// Texts: the avatar's label (the number's last two digits).
    pub badge: String,
    /// The message's page; its buttons post there too.
    pub href: String,
    pub to: String,
    pub subject: String,
    /// The start of the text, shown after the subject.
    pub snippet: String,
    pub ago: String,
    /// Outbox: queued, sending (leased by the job runner), sent or failed.
    /// Empty in the inbox.
    pub state: String,
    /// Outbox, while sending: when the lease ends and who holds it (a tooltip).
    pub lease: String,
    pub purpose: String,
    pub read: bool,
    pub archived: bool,
    /// Outbox: the copy the dev mailbox (or phone) received, if it did.
    pub inbox_href: Option<String>,
}

/// One box's rows: part of the page, and on their own for its poll.
#[derive(Template)]
#[template(path = "dev_box_rows.html")]
pub struct BoxRows {
    /// `mail` (a mail client's table) or `sms` (a messaging app's list).
    pub kind: &'static str,
    /// Test id prefix of the buttons: `dev-inbox`, `dev-mail`, `dev-phone`, `dev-sms`.
    pub prefix: &'static str,
    /// The row's test id (`dev-inbox-item`, `dev-mail-message`).
    pub row_testid: &'static str,
    /// The subject link's test id.
    pub open_testid: &'static str,
    pub empty: String,
    pub rows: Vec<BoxRow>,
}

impl BoxRows {
    /// Unopened and not archived: the count on the tab.
    pub fn unread(&self) -> usize {
        self.rows.iter().filter(|r| !r.read && !r.archived).count()
    }
}

#[derive(Template)]
#[template(path = "dev_box.html")]
pub struct BoxTemplate {
    pub user: Option<CurrentUser>,
    /// The section: `mail` or `sms`.
    pub kind: &'static str,
    /// The tab showing when the page opens: `in` or `out`.
    pub tab: &'static str,
    /// The address (or number) filter, for both tabs.
    pub to: String,
    /// `?to=…` for the tabs' links; empty without a filter.
    pub to_query: String,
    /// E-mail: `127.0.0.1:<port>` of the dev mailbox; texts: `dev-phone`.
    /// `None`: off (providers are configured).
    pub sink: Option<String>,
    pub providers: String,
    /// The test message form's address (e-mail only).
    pub default_to: String,
    pub inbox: BoxRows,
    pub outbox: BoxRows,
}

impl BoxTemplate {
    /// The section, for `dev_box_kinds.html`.
    pub fn section(&self) -> &'static str {
        self.kind
    }
    /// The section's two list URLs.
    pub fn in_url(&self) -> String {
        crate::url(if self.kind == "sms" { "/dev/phone" } else { "/dev/inbox" })
    }
    pub fn out_url(&self) -> String {
        crate::url(if self.kind == "sms" { "/dev/sms" } else { "/dev/mail" })
    }
    /// What the page polls.
    pub fn rows_url(&self) -> String {
        format!("{}?kind={}{}", crate::url("/dev/inbox/rows"), self.kind, self.to_query.replacen('?', "&", 1))
    }
}

#[derive(Template)]
#[template(path = "dev_text_message.html")]
pub struct TextMessageTemplate {
    pub user: Option<CurrentUser>,
    /// `in` (a text the dev phone received) or `out` (an outbox row).
    pub tab: &'static str,
    pub id: i64,
    /// Where its buttons post, and the list to go back to.
    pub href: String,
    pub back: String,
    pub to: String,
    pub purpose: String,
    /// Outbox: queued, sent or failed, the provider, attempts, last error.
    pub state: String,
    pub provider: String,
    pub attempts: i64,
    pub last_error: String,
    pub ago: String,
    /// The conversation with this number, oldest first: on the phone every
    /// text it holds to the number; in the outbox every row to it.
    pub thread: Vec<Bubble>,
    /// The other side's page: the outbox row, or the phone's copy.
    pub other: Option<String>,
    pub archived: bool,
}

/// One text in a conversation (`dev_text_message.html`).
pub struct Bubble {
    pub id: i64,
    /// The same text on both sides (`o<outbox id>` for the app's, `r<phone
    /// id>` for a reply), so switching sides can slide each bubble across.
    pub key: String,
    pub text: String,
    pub ago: String,
    /// Outbox rows: queued, sent or failed; `""` for the phone's copies and replies.
    pub state: String,
    /// Typed on the phone's page (the number texting the app).
    pub from_phone: bool,
    /// Sent by whoever is looking at this side (right, blue): the phone's
    /// replies on the phone, the app's texts in the outbox.
    pub mine: bool,
    /// The text the page is about.
    pub current: bool,
}

impl TextMessageTemplate {
    /// Test id prefix of its buttons.
    pub fn prefix(&self) -> &'static str {
        if self.tab == "out" { "dev-sms" } else { "dev-phone" }
    }

    /// The number's avatar on the outbox side.
    pub fn badge(&self) -> String {
        badge(&self.to)
    }
}

#[derive(Template)]
#[template(path = "dev_inbox_message.html")]
pub struct InboxMessageTemplate {
    pub user: Option<CurrentUser>,
    pub m: Arc<crate::mail::sink::Received>,
    /// The HTML part for the frame, with links opening in this tab.
    pub srcdoc: String,
    pub links: Vec<String>,
    pub archived: bool,
}

#[derive(Template)]
#[template(path = "dev_mail_message.html")]
pub struct MailMessageTemplate {
    pub user: Option<CurrentUser>,
    pub id: i64,
    pub to: String,
    pub subject: String,
    pub purpose: String,
    pub state: String,
    pub attempts: i64,
    pub provider: String,
    pub last_error: String,
    pub created: String,
    pub text: String,
    pub srcdoc: String,
    pub links: Vec<String>,
    pub inbox_id: Option<i64>,
    pub archived: bool,
}

#[derive(Deserialize)]
pub struct InboxParams {
    pub format: Option<String>,
    /// Only messages to this address.
    pub to: Option<String>,
    /// `/dev/inbox/rows`: `mail` (default) or `sms`.
    pub kind: Option<String>,
}

#[derive(Deserialize)]
pub struct BoxActForm {
    /// `archive`, `unarchive` or `delete`.
    #[serde(rename = "do")]
    pub action: String,
}

fn inbox_of(state: &AppState) -> Result<&crate::mail::sink::Inbox, AppError> {
    // Off (SKRONGLE_MAIL_PROVIDERS, or SKRONGLE_DEV_SMTP=0): only the list page explains why.
    state.dev_inbox.as_deref().ok_or(AppError::NotFound)
}

/// The page's own requests (`X-Dev-Box: 1`, from devbox.js) get 204; a
/// plain form post goes back to `list`.
fn done(headers: &axum::http::HeaderMap, list: &str) -> Response {
    if headers.contains_key("x-dev-box") {
        axum::http::StatusCode::NO_CONTENT.into_response()
    } else {
        Redirect::to(&crate::url(list)).into_response()
    }
}

/// The text's first words, on one line.
fn snippet(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(100) {
        Some((i, _)) => format!("{}…", &flat[..i]),
        None => flat,
    }
}

/// Newest first, the archived after the rest (newest first too).
fn ordered(mut rows: Vec<BoxRow>) -> Vec<BoxRow> {
    rows.sort_by_key(|r| (r.archived, std::cmp::Reverse(r.id)));
    rows
}

fn no_messages(to: Option<&str>) -> String {
    match to {
        Some(t) => format!("No messages to {t}."),
        None => "No messages.".into(),
    }
}

fn inbox_rows(state: &AppState, to: Option<&str>, now: i64) -> BoxRows {
    let messages = state.dev_inbox.as_ref().map(|i| i.list(to)).unwrap_or_default();
    let rows = messages
        .iter()
        .map(|m| {
            let mark = state.dev_marks.get(MAIL_IN, m.id);
            BoxRow {
                id: m.id,
                badge: String::new(),
                href: crate::url(&format!("/dev/inbox/{}", m.id)),
                to: if m.to.is_empty() { m.envelope_to.join(", ") } else { m.to.clone() },
                subject: if m.subject.is_empty() { "(no subject)".into() } else { m.subject.clone() },
                snippet: snippet(&m.text),
                ago: super::time_ago(m.received_at, now),
                state: String::new(),
                lease: String::new(),
                purpose: String::new(),
                read: mark.read,
                archived: mark.archived,
                inbox_href: None,
            }
        })
        .collect();
    BoxRows { kind: "mail", prefix: "dev-inbox", row_testid: "dev-inbox-item", open_testid: "dev-inbox-open", empty: no_messages(to), rows: ordered(rows) }
}

/// An outbox row's state for the dev boxes, and its lease as a tooltip: a
/// queued row the job runner holds a live lease on is `sending` (#873).
fn outbox_state(r: &serde_json::Value, now: i64) -> (String, String) {
    let state = r.get("state").and_then(|v| v.as_str()).unwrap_or_default();
    match r.get("locked_until").and_then(|v| v.as_i64()) {
        Some(until) if state == "queued" && until > now => {
            let by = r.get("locked_by").and_then(|v| v.as_str()).unwrap_or_default();
            ("sending".into(), format!("lease ends {} ({by})", jobs_page::when(until, now)))
        }
        _ => (state.to_string(), String::new()),
    }
}

async fn outbox_rows(state: &AppState, to: Option<&str>, now: i64) -> Result<BoxRows, AppError> {
    let all = state.db.call(|c| crate::mail::recent(c, 50)).await?;
    let want = to.map(str::to_ascii_lowercase);
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let rows = all
        .iter()
        .filter(|r| want.as_deref().is_none_or(|w| s(r, "to").to_ascii_lowercase().contains(w)))
        .map(|r| {
            let id = r["id"].as_i64().unwrap_or_default();
            let mark = state.dev_marks.get(MAIL_OUT, id);
            let subject = s(r, "subject");
            BoxRow {
                id,
                badge: String::new(),
                href: crate::url(&format!("/dev/mail/{id}")),
                to: s(r, "to"),
                subject: if subject.is_empty() { "(no subject)".into() } else { subject },
                snippet: snippet(&s(r, "text")),
                ago: super::time_ago(r["created_at"].as_i64().unwrap_or(now), now),
                state: outbox_state(r, now).0,
                lease: outbox_state(r, now).1,
                purpose: s(r, "purpose"),
                read: mark.read,
                archived: mark.archived,
                inbox_href: state.dev_inbox.as_ref().and_then(|i| i.by_outbox(id)).map(|i| crate::url(&format!("/dev/inbox/{i}"))),
            }
        })
        .collect();
    Ok(BoxRows { kind: "mail", prefix: "dev-mail", row_testid: "dev-mail-message", open_testid: "dev-mail-view", empty: no_messages(to), rows: ordered(rows) })
}


/// The number's last two digits, for a text row's avatar.
fn badge(number: &str) -> String {
    let d: Vec<char> = number.chars().filter(char::is_ascii_digit).collect();
    d[d.len().saturating_sub(2)..].iter().collect()
}

/// A Text box lists conversations, not texts: one row per number, its
/// newest text (the rows come newest first), which opens the conversation.
/// So a second text to the same number joins its conversation instead of
/// starting another.
fn one_per_number<T>(rows: impl IntoIterator<Item = T>, number: impl Fn(&T) -> String) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    rows.into_iter().filter(|r| seen.insert(number(r))).collect()
}

fn phone_rows(state: &AppState, to: Option<&str>, now: i64) -> BoxRows {
    let messages = one_per_number(state.dev_phone.as_ref().map(|p| p.list(to)).unwrap_or_default(), |m| m.to.clone());
    let rows = messages
        .iter()
        .map(|m| {
            let mark = state.dev_marks.get(SMS_IN, m.id);
            BoxRow {
                id: m.id,
                badge: badge(&m.to),
                href: crate::url(&format!("/dev/phone/{}", m.id)),
                to: m.to.clone(),
                subject: String::new(),
                snippet: snippet(&m.text),
                ago: super::time_ago(m.received_at, now),
                state: String::new(),
                lease: String::new(),
                purpose: m.purpose.clone(),
                read: mark.read,
                archived: mark.archived,
                inbox_href: None,
            }
        })
        .collect();
    BoxRows { kind: "sms", prefix: "dev-phone", row_testid: "dev-phone-item", open_testid: "dev-phone-open", empty: no_texts(to), rows: ordered(rows) }
}

async fn sms_rows(state: &AppState, to: Option<&str>, now: i64) -> Result<BoxRows, AppError> {
    let all = state.db.call(|c| crate::sms::recent(c, 50)).await?;
    let want: Option<String> = to.map(|t| t.chars().filter(char::is_ascii_digit).collect());
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let rows = one_per_number(all.iter().filter(|r| want.as_deref().is_none_or(|w| s(r, "to").contains(w))), |r| s(r, "to"))
        .into_iter()
        .map(|r| {
            let id = r["id"].as_i64().unwrap_or_default();
            let mark = state.dev_marks.get(SMS_OUT, id);
            let to = s(r, "to");
            BoxRow {
                id,
                badge: badge(&to),
                href: crate::url(&format!("/dev/sms/{id}")),
                to,
                subject: String::new(),
                snippet: snippet(&s(r, "text")),
                ago: super::time_ago(r["created_at"].as_i64().unwrap_or(now), now),
                state: outbox_state(r, now).0,
                lease: outbox_state(r, now).1,
                purpose: s(r, "purpose"),
                read: mark.read,
                archived: mark.archived,
                inbox_href: state.dev_phone.as_ref().and_then(|p| p.by_outbox(id)).map(|i| crate::url(&format!("/dev/phone/{i}"))),
            }
        })
        .collect();
    Ok(BoxRows { kind: "sms", prefix: "dev-sms", row_testid: "dev-sms-message", open_testid: "dev-sms-view", empty: no_texts(to), rows: ordered(rows) })
}

fn no_texts(to: Option<&str>) -> String {
    match to {
        Some(t) => format!("No texts to {t}."),
        None => "No texts.".into(),
    }
}

fn filter_of(to: Option<&str>) -> Option<&str> {
    to.map(str::trim).filter(|t| !t.is_empty())
}

/// Both boxes of a section (`mail` or `sms`).
async fn boxes(state: &AppState, kind: &str, to: Option<&str>) -> Result<(BoxRows, BoxRows), AppError> {
    let now = crate::db::now();
    Ok(if kind == "sms" { (phone_rows(state, to, now), sms_rows(state, to, now).await?) } else { (inbox_rows(state, to, now), outbox_rows(state, to, now).await?) })
}

async fn box_page(state: &AppState, user: Option<CurrentUser>, kind: &'static str, tab: &'static str, to: Option<&str>) -> Result<Response, AppError> {
    let to = filter_of(to);
    let (inbox, outbox) = boxes(state, kind, to).await?;
    let default_to = user.as_ref().map(|u| u.email.clone()).unwrap_or_else(|| dev::DEV_EMAIL.to_string());
    let (sink, providers) = if kind == "sms" {
        (state.dev_phone.as_ref().map(|_| "dev-phone".to_string()), state.sms.providers.join(", then "))
    } else {
        (state.dev_inbox.as_ref().map(|i| i.addr().to_string()), state.mail.providers.join(", then "))
    };
    Ok(Page(BoxTemplate {
        user,
        kind,
        tab,
        to: to.unwrap_or_default().to_string(),
        to_query: to.map(|t| format!("?to={}", crate::oauth::urlencode(t))).unwrap_or_default(),
        sink,
        providers,
        default_to,
        inbox,
        outbox,
    })
    .into_response())
}

/// GET /dev/inbox: what the dev mailbox received, newest first (`?to=` for
/// one address). JSON for scripts (`{"smtp", "messages": [{…, "text",
/// "html", "links"}]}`); for a browser, the dev mailbox page with its
/// E-mail section's Inbox tab showing (the Outbox tab is /dev/mail's).
pub async fn inbox(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    if !wants_html(&headers, q.format.as_deref()) {
        let to = filter_of(q.to.as_deref());
        let messages = state.dev_inbox.as_ref().map(|i| i.list(to)).unwrap_or_default();
        let smtp = state.dev_inbox.as_ref().map(|i| i.addr().to_string());
        return Ok(Json(serde_json::json!({ "smtp": smtp, "messages": messages.iter().map(|m| m.json()).collect::<Vec<_>>() })).into_response());
    }
    box_page(&state, user, "mail", "in", q.to.as_deref()).await
}

/// GET /dev/inbox/rows (`?kind=mail|sms`, `?to=`): one section's two lists
/// as HTML, and the unread counts, for the page's poll:
/// `{"in": {"html", "unread"}, "out": {…}}`.
pub async fn box_rows(State(state): State<Arc<AppState>>, Query(q): Query<InboxParams>) -> Result<Response, AppError> {
    let kind = if q.kind.as_deref() == Some("sms") { "sms" } else { "mail" };
    let (inbox, outbox) = boxes(&state, kind, filter_of(q.to.as_deref())).await?;
    let mut out = serde_json::Map::new();
    for (key, rows) in [("in", inbox), ("out", outbox)] {
        out.insert(key.into(), serde_json::json!({ "html": rows.render()?, "unread": rows.unread() }));
    }
    Ok(Json(serde_json::Value::Object(out)).into_response())
}

fn phone_of(state: &AppState) -> Result<&crate::sms::phone::Phone, AppError> {
    state.dev_phone.as_deref().ok_or(AppError::NotFound)
}

/// GET /dev/phone: the texts the dev phone received, newest first (`?to=`:
/// numbers containing those digits). JSON `{"phone": bool, "messages":
/// [{id, outbox_id, to, text, purpose, received_at}]}`; for a browser, the
/// dev mailbox page with its Text section's Inbox tab showing.
pub async fn phone(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    if !wants_html(&headers, q.format.as_deref()) {
        let messages = state.dev_phone.as_ref().map(|p| p.list(filter_of(q.to.as_deref()))).unwrap_or_default();
        return Ok(Json(serde_json::json!({ "phone": state.dev_phone.is_some(), "messages": messages.iter().map(|m| m.json()).collect::<Vec<_>>() })).into_response());
    }
    box_page(&state, user, "sms", "in", q.to.as_deref()).await
}

/// The conversation on a phone text's page, as that phone sees it: every
/// text it holds to `to` and its replies; `current` is the opened text.
fn thread_of(state: &AppState, to: &str, current: i64, now: i64) -> Vec<Bubble> {
    state
        .dev_phone
        .as_ref()
        .map(|p| p.thread(to))
        .unwrap_or_default()
        .iter()
        .map(|m| Bubble { id: m.id, key: if m.from_phone { format!("r{}", m.id) } else { format!("o{}", m.outbox_id) }, text: m.text.clone(), ago: super::time_ago(m.received_at, now), state: String::new(), from_phone: m.from_phone, mine: m.from_phone, current: m.id == current })
        .collect()
}

/// GET /dev/phone/{id}: one text, in its conversation with that number, as
/// a messaging app shows it. Opening it in a browser marks it read.
/// `?format=json`: the text.
pub async fn phone_message(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    let m = phone_of(&state)?.get(id).ok_or(AppError::NotFound)?;
    if !wants_html(&headers, q.format.as_deref()) {
        return Ok(Json(m.json()).into_response());
    }
    state.dev_marks.set_read(SMS_IN, id, true);
    let now = crate::db::now();
    Ok(Page(TextMessageTemplate {
        user,
        tab: "in",
        id,
        href: crate::url(&format!("/dev/phone/{id}")),
        back: crate::url("/dev/phone"),
        to: m.to.clone(),
        purpose: m.purpose.clone(),
        state: String::new(),
        provider: String::new(),
        attempts: 0,
        last_error: String::new(),
        ago: super::time_ago(m.received_at, now),
        thread: thread_of(&state, &m.to, id, now),
        other: Some(crate::url(&format!("/dev/sms/{}", m.outbox_id))),
        archived: state.dev_marks.get(SMS_IN, id).archived,
    })
    .into_response())
}

/// POST /dev/phone/{id} (`do=archive|unarchive|delete`), as on the e-mail
/// inbox. A row is a conversation, so Delete removes every text with that
/// number from the phone, replies too.
pub async fn phone_act(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Path(id): Path<i64>, Form(f): Form<BoxActForm>) -> Result<Response, AppError> {
    let phone = phone_of(&state)?;
    let m = phone.get(id).ok_or(AppError::NotFound)?;
    match f.action.as_str() {
        "archive" | "unarchive" => state.dev_marks.set_archived(SMS_IN, id, f.action == "archive"),
        "delete" => {
            for gone in phone.remove_thread(&m.to) {
                state.dev_marks.forget(SMS_IN, gone);
            }
        }
        other => return Err(AppError::BadRequest(format!("do must be archive, unarchive or delete, not {other:?}"))),
    }
    Ok(done(&headers, "/dev/phone"))
}

/// POST /dev/phone/reply (form `to`, `text`): a reply typed on a text's page,
/// the number texting the app; it joins the conversation on both sides.
/// Back to /dev/phone (204 for the page's own posts); 400 for an empty text.
pub async fn phone_reply(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Form(f): Form<SmsSendForm>) -> Result<Response, AppError> {
    let phone = phone_of(&state)?;
    let text = f.text.trim();
    if text.is_empty() || f.to.trim().is_empty() {
        return Err(AppError::BadRequest("Type a text to send.".into()));
    }
    let id = phone.reply(f.to.trim(), text);
    tracing::info!("dev-phone: {} replied (#{id})", f.to.trim());
    Ok(done(&headers, "/dev/phone"))
}

/// POST /dev/phone/clear: empty the dev phone, then back to it.
pub async fn phone_clear(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap) -> Result<Response, AppError> {
    phone_of(&state)?.clear();
    state.dev_marks.clear(SMS_IN);
    Ok(done(&headers, "/dev/phone"))
}

/// GET /dev/sms/{id}: one text outbox row: its state, provider and last
/// error, in the outbox's conversation with that number (every row to it,
/// as sent bubbles). Opening it in a browser marks it read.
/// `?format=json`: the row.
pub async fn sms_message(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    let (r, rows) = state
        .db
        .call(move |c| {
            let r = crate::sms::outbox_row(c, id)?;
            let rows = match &r {
                Some(r) => crate::sms::thread(c, r["to"].as_str().unwrap_or_default())?,
                None => Vec::new(),
            };
            Ok((r, rows))
        })
        .await?;
    let r = r.ok_or(AppError::NotFound)?;
    if !wants_html(&headers, q.format.as_deref()) {
        return Ok(Json(r).into_response());
    }
    state.dev_marks.set_read(SMS_OUT, id, true);
    let s = |k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let now = crate::db::now();
    let ago = super::time_ago(r["created_at"].as_i64().unwrap_or(now), now);
    let phone_id = state.dev_phone.as_ref().and_then(|p| p.by_outbox(id));
    // The app's side of the conversation: its rows to the number and the
    // replies typed on the phone's page, in time order. Times are only
    // seconds, so within a second the phone's sequence (its ids) decides,
    // each received text standing in for its outbox row. A row the phone
    // does not hold sits at its own time: a sent one is from before a
    // restart emptied the phone, so it goes before that second's arrivals;
    // a queued or failed one is still on its way, so after.
    let str_of = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let to = s("to");
    let mut by_row: std::collections::HashMap<i64, &serde_json::Value> = rows.iter().map(|v| (v["id"].as_i64().unwrap_or_default(), v)).collect();
    let row_bubble = |v: &serde_json::Value| {
        let rid = v["id"].as_i64().unwrap_or_default();
        Bubble { id: rid, key: format!("o{rid}"), text: str_of(v, "text"), ago: super::time_ago(v["created_at"].as_i64().unwrap_or(now), now), state: str_of(v, "state"), from_phone: false, mine: true, current: rid == id }
    };
    let mut thread: Vec<((i64, u8, i64), Bubble)> = Vec::new();
    for m in state.dev_phone.as_ref().map(|p| p.thread(&to)).unwrap_or_default().iter() {
        let key = (m.received_at, 1, m.id);
        if m.from_phone {
            thread.push((key, Bubble { id: m.id, key: format!("r{}", m.id), text: m.text.clone(), ago: super::time_ago(m.received_at, now), state: String::new(), from_phone: true, mine: false, current: false }));
        } else if let Some(v) = by_row.remove(&m.outbox_id) {
            thread.push((key, row_bubble(v)));
        } else {
            // The row was deleted; the phone still has the text.
            thread.push((key, Bubble { id: m.outbox_id, key: format!("o{}", m.outbox_id), text: m.text.clone(), ago: super::time_ago(m.received_at, now), state: String::new(), from_phone: false, mine: true, current: m.outbox_id == id }));
        }
    }
    for (rid, v) in by_row {
        let group = if v["state"].as_str() == Some("sent") { 0 } else { 2 };
        thread.push(((v["created_at"].as_i64().unwrap_or_default(), group, rid), row_bubble(v)));
    }
    thread.sort_by_key(|(key, _)| *key);
    let thread: Vec<Bubble> = thread.into_iter().map(|(_, b)| b).collect();
    Ok(Page(TextMessageTemplate {
        user,
        tab: "out",
        id,
        href: crate::url(&format!("/dev/sms/{id}")),
        back: crate::url("/dev/sms"),
        to,
        purpose: s("purpose"),
        state: s("state"),
        provider: s("provider"),
        attempts: r["attempts"].as_i64().unwrap_or_default(),
        last_error: s("last_error"),
        thread,
        ago,
        other: phone_id.map(|i| crate::url(&format!("/dev/phone/{i}"))),
        archived: state.dev_marks.get(SMS_OUT, id).archived,
    })
    .into_response())
}

/// POST /dev/sms/{id} (`do=archive|unarchive|delete`): as on the e-mail
/// outbox. A row is a conversation, so Delete removes every row to that
/// number (a queued text is then never sent, and the number's limit starts
/// over).
pub async fn sms_act(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Path(id): Path<i64>, Form(f): Form<BoxActForm>) -> Result<Response, AppError> {
    let row = state.db.call(move |c| crate::sms::outbox_row(c, id)).await?.ok_or(AppError::NotFound)?;
    match f.action.as_str() {
        "archive" | "unarchive" => state.dev_marks.set_archived(SMS_OUT, id, f.action == "archive"),
        "delete" => {
            let to = row["to"].as_str().unwrap_or_default().to_string();
            for gone in state.db.call(move |c| crate::sms::delete_thread(c, &to)).await? {
                state.dev_marks.forget(SMS_OUT, gone);
            }
        }
        other => return Err(AppError::BadRequest(format!("do must be archive, unarchive or delete, not {other:?}"))),
    }
    Ok(done(&headers, "/dev/sms"))
}

/// GET /dev/inbox/latest (`?to=`, default the signed-in user's address):
/// the newest message to that address, or the list when there is none.
pub async fn inbox_latest(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Query(q): Query<InboxParams>) -> Result<Response, AppError> {
    let inbox = inbox_of(&state)?;
    let to = q.to.filter(|t| !t.trim().is_empty()).or_else(|| user.map(|u| u.email));
    let newest = inbox.list(to.as_deref()).first().map(|m| m.id);
    let list = match &to {
        Some(t) => format!("/dev/inbox?to={}", t.replace('%', "%25").replace('+', "%2B").replace('&', "%26").replace('#', "%23")),
        None => "/dev/inbox".into(),
    };
    Ok(Redirect::to(&crate::url(&newest.map(|id| format!("/dev/inbox/{id}")).unwrap_or(list))).into_response())
}

/// GET /dev/inbox/{id}: one message: headers, the HTML part in a sandboxed
/// frame whose links open in the tab, the links, the text and the source.
/// Opening it in a browser marks it read. `?format=json` as on the list.
pub async fn inbox_message(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    let m = inbox_of(&state)?.get(id).ok_or(AppError::NotFound)?;
    if !wants_html(&headers, q.format.as_deref()) {
        return Ok(Json(m.json()).into_response());
    }
    state.dev_marks.set_read(MAIL_IN, id, true);
    let srcdoc = if m.html.is_empty() { String::new() } else { format!("<base target=\"_top\">{}", m.html) };
    let links = m.links();
    let archived = state.dev_marks.get(MAIL_IN, id).archived;
    Ok(Page(InboxMessageTemplate { user, m, srcdoc, links, archived }).into_response())
}

/// POST /dev/inbox/{id} (`do=archive|unarchive|delete`): archive (strike
/// out, move to the bottom), bring back, or delete for good.
pub async fn inbox_act(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Path(id): Path<i64>, Form(f): Form<BoxActForm>) -> Result<Response, AppError> {
    let inbox = inbox_of(&state)?;
    inbox.get(id).ok_or(AppError::NotFound)?;
    match f.action.as_str() {
        "archive" | "unarchive" => state.dev_marks.set_archived(MAIL_IN, id, f.action == "archive"),
        "delete" => {
            inbox.remove(id);
            state.dev_marks.forget(MAIL_IN, id);
        }
        other => return Err(AppError::BadRequest(format!("do must be archive, unarchive or delete, not {other:?}"))),
    }
    Ok(done(&headers, "/dev/inbox"))
}

/// GET /dev/mail/{id}: one outbox row: its state, provider and last error,
/// its links, the HTML part framed as on the inbox's message page, and the
/// text. Opening it in a browser marks it read. `?format=json`: the row.
pub async fn mail_message(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<InboxParams>,
) -> Result<Response, AppError> {
    let r = state.db.call(move |c| crate::mail::outbox_row(c, id)).await?.ok_or(AppError::NotFound)?;
    if !wants_html(&headers, q.format.as_deref()) {
        return Ok(Json(r).into_response());
    }
    state.dev_marks.set_read(MAIL_OUT, id, true);
    let s = |k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let (text, html) = (s("text"), s("html"));
    let created = r["created_at"].as_i64().unwrap_or_default();
    Ok(Page(MailMessageTemplate {
        user,
        id,
        to: s("to"),
        subject: s("subject"),
        purpose: s("purpose"),
        state: s("state"),
        attempts: r["attempts"].as_i64().unwrap_or_default(),
        provider: s("provider"),
        last_error: s("last_error"),
        created: super::time_ago(created, crate::db::now()),
        links: crate::mail::sink::links(&text, &html),
        srcdoc: if html.is_empty() { String::new() } else { format!("<base target=\"_top\">{html}") },
        text,
        inbox_id: state.dev_inbox.as_ref().and_then(|i| i.by_outbox(id)),
        archived: state.dev_marks.get(MAIL_OUT, id).archived,
    })
    .into_response())
}

/// POST /dev/mail/{id} (`do=archive|unarchive|delete`): as on the inbox;
/// Delete removes the outbox row (a queued message is then never sent).
pub async fn mail_act(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Path(id): Path<i64>, Form(f): Form<BoxActForm>) -> Result<Response, AppError> {
    match f.action.as_str() {
        "archive" | "unarchive" => {
            state.db.call(move |c| crate::mail::outbox_row(c, id)).await?.ok_or(AppError::NotFound)?;
            state.dev_marks.set_archived(MAIL_OUT, id, f.action == "archive");
        }
        "delete" => {
            if !state.db.call(move |c| crate::mail::delete_row(c, id)).await? {
                return Err(AppError::NotFound);
            }
            state.dev_marks.forget(MAIL_OUT, id);
        }
        other => return Err(AppError::BadRequest(format!("do must be archive, unarchive or delete, not {other:?}"))),
    }
    Ok(done(&headers, "/dev/mail"))
}

/// GET /dev/inbox/{id}/raw: the message as received, to save as `.eml`.
pub async fn inbox_raw(State(state): State<Arc<AppState>>, Path(id): Path<i64>) -> Result<Response, AppError> {
    let m = inbox_of(&state)?.get(id).ok_or(AppError::NotFound)?;
    Ok(([(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], m.raw.clone()).into_response())
}

/// POST /dev/inbox/clear: empty the dev mailbox, then back to it.
pub async fn inbox_clear(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap) -> Result<Response, AppError> {
    inbox_of(&state)?.clear();
    state.dev_marks.clear(MAIL_IN);
    Ok(done(&headers, "/dev/inbox"))
}

// ---------- the dev authenticator: an authenticator app's TOTP codes ----------
//
// The third section of the dev mailbox page: an authenticator app (like
// Google Authenticator) holding TOTP keys (`dev::Authenticator`, in
// memory). Add one by pasting an `otpauth://totp/…` link or a setup key, or
// by "scanning": a link to `/dev/authenticator?uri=otpauth://…` opens the
// page with it filled in, one click from added. Codes are worked out here
// from the secret, as an app does (`crate::totp`); the page counts down
// to the next one and polls `/dev/authenticator/rows` (static/devbox.js).

/// What the add form opens with when nothing was scanned, and what it
/// offers again when the list goes empty (static/devbox.js): an example
/// account, one click from added.
pub const EXAMPLE_TOTP_URI: &str = "otpauth://totp/Skrongle:ann@example.org?secret=JBSWY3DPEHPK3PXP&issuer=Skrongle";

/// One account's row (`dev_authenticator_rows.html`).
pub struct AuthRow {
    pub id: i64,
    pub issuer: String,
    pub account: String,
    pub code: String,
    /// When `code` stops being the current one (unix seconds).
    pub expires: u64,
    pub remaining: u64,
    pub period: u64,
    /// Not the defaults (SHA-1, 6 digits, 30 s): shown under the name.
    pub settings: String,
}

impl AuthRow {
    /// The code in two halves, as apps show it (`123 456`).
    pub fn grouped(&self) -> String {
        let mid = self.code.len() / 2;
        format!("{} {}", &self.code[..mid], &self.code[mid..])
    }

    /// The avatar's letter.
    pub fn initial(&self) -> String {
        let name = if self.issuer.is_empty() { &self.account } else { &self.issuer };
        name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
    }
}

#[derive(Template)]
#[template(path = "dev_authenticator_rows.html")]
pub struct AuthRows {
    pub rows: Vec<AuthRow>,
}

#[derive(Template)]
#[template(path = "dev_authenticator.html")]
pub struct AuthenticatorTemplate {
    pub user: Option<CurrentUser>,
    /// A "scanned" link (`?uri=`), filled into the add form.
    pub scanned: String,
    /// The add form's value: `scanned`, else `EXAMPLE_TOTP_URI`.
    pub prefill: String,
    pub now: u64,
    pub list: AuthRows,
}

impl AuthenticatorTemplate {
    /// The section, for `dev_box_kinds.html`.
    pub fn section(&self) -> &'static str {
        "auth"
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn auth_rows(state: &AppState, now: u64) -> AuthRows {
    let rows = state
        .dev_authenticator
        .list()
        .into_iter()
        .map(|a| {
            let k = &a.key;
            let defaults = k.algorithm == crate::totp::Algorithm::Sha1 && k.digits == crate::totp::DEFAULT_DIGITS && k.period == crate::totp::DEFAULT_PERIOD;
            AuthRow {
                id: a.id,
                issuer: k.issuer.clone(),
                account: k.account.clone(),
                code: k.code_at(now),
                expires: now + k.remaining(now),
                remaining: k.remaining(now),
                period: k.period,
                settings: if defaults { String::new() } else { format!("{}, {} digits, {} s", k.algorithm.as_str(), k.digits, k.period) },
            }
        })
        .collect();
    AuthRows { rows }
}

#[derive(Deserialize)]
pub struct AuthenticatorParams {
    pub format: Option<String>,
    /// A scanned `otpauth://` link to fill in.
    pub uri: Option<String>,
}

/// GET /dev/authenticator: the dev authenticator's accounts with their
/// current codes. JSON for scripts (`{"now", "accounts": [{id, issuer,
/// account, code, remaining, period, digits, algorithm, secret}]}`); for a
/// browser, the dev mailbox page's Authenticator section. `?uri=otpauth://…`
/// is the "scan": the add form opens filled in with it; without one it
/// opens with an example account (`EXAMPLE_TOTP_URI`).
pub async fn authenticator(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Query(q): Query<AuthenticatorParams>,
) -> Result<Response, AppError> {
    let now = unix_now();
    if !wants_html(&headers, q.format.as_deref()) {
        let accounts: Vec<_> = state
            .dev_authenticator
            .list()
            .iter()
            .map(|a| {
                let k = &a.key;
                serde_json::json!({ "id": a.id, "issuer": k.issuer, "account": k.account, "code": k.code_at(now), "remaining": k.remaining(now), "period": k.period, "digits": k.digits, "algorithm": k.algorithm.as_str(), "secret": k.secret_base32() })
            })
            .collect();
        return Ok(Json(serde_json::json!({ "now": now, "accounts": accounts })).into_response());
    }
    let scanned = q.uri.unwrap_or_default();
    let prefill = if scanned.is_empty() { EXAMPLE_TOTP_URI.to_string() } else { scanned.clone() };
    Ok(Page(AuthenticatorTemplate { user, scanned, prefill, now, list: auth_rows(&state, now) }).into_response())
}

/// GET /dev/authenticator/rows: the list as HTML and the server's clock,
/// for the page's poll: `{"html", "now"}`.
pub async fn authenticator_rows(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let now = unix_now();
    Ok(Json(serde_json::json!({ "html": auth_rows(&state, now).render()?, "now": now })).into_response())
}

#[derive(Deserialize)]
pub struct AuthenticatorAddForm {
    /// An `otpauth://totp/…` link or a base32 setup key.
    #[serde(default)]
    pub uri: String,
}

/// POST /dev/authenticator (form `uri`: an `otpauth://totp/…` link or a
/// base32 setup key): add the account, then back to the page (204 for the
/// page's own posts, `{"id"}` as JSON with `Accept: application/json`);
/// 400 with why as text.
pub async fn authenticator_add(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Form(f): Form<AuthenticatorAddForm>) -> Result<Response, AppError> {
    let key = match crate::totp::Key::parse(&f.uri) {
        Ok(k) => k,
        Err(why) if headers.contains_key("x-dev-box") => return Ok((axum::http::StatusCode::BAD_REQUEST, why).into_response()),
        Err(why) => return Err(AppError::BadRequest(why)),
    };
    let id = state.dev_authenticator.add(key, crate::db::now());
    if headers.get(axum::http::header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json")) {
        return Ok(Json(serde_json::json!({ "id": id })).into_response());
    }
    Ok(done(&headers, "/dev/authenticator"))
}

/// POST /dev/authenticator/{id} (`do=delete`): remove the account, as an
/// app's "Remove" does; then back to the page (204 for the page's own posts).
pub async fn authenticator_act(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Path(id): Path<i64>, Form(f): Form<BoxActForm>) -> Result<Response, AppError> {
    if f.action != "delete" {
        return Err(AppError::BadRequest(format!("unknown action {}", f.action)));
    }
    if !state.dev_authenticator.remove(id) {
        return Err(AppError::NotFound);
    }
    Ok(done(&headers, "/dev/authenticator"))
}

// ---------- the Google stand-in (crate::dev, "the Google stand-in") ----------

type Params = std::collections::HashMap<String, String>;

fn mock_google_cfg(state: &AppState) -> Result<&crate::oauth::GoogleConfig, AppError> {
    state.google.as_ref().filter(|g| g.mock).ok_or(AppError::NotFound)
}

/// GET /dev/google/auth: Google's authorization endpoint. Checks the request
/// the way Google does (client, redirect URI, response type, PKCE) and sends
/// the browser straight back with a code (or `error=access_denied` for
/// `mock_error=cancel`); there is no account picker, the account comes from
/// `login_hint`. Anything else wrong is a 400 that names it, like Google's
/// own error page.
pub async fn google_auth(State(state): State<Arc<AppState>>, Query(q): Query<Params>) -> Result<Response, AppError> {
    let cfg = mock_google_cfg(&state)?;
    let p = |k: &str| q.get(k).map(String::as_str).unwrap_or("");
    let bad = |what: &str| AppError::BadRequest(format!("Mock Google: {what}"));
    if p("client_id") != cfg.client_id {
        return Err(bad("invalid_client: the OAuth client was not found"));
    }
    if p("redirect_uri") != cfg.redirect_uri {
        return Err(bad("redirect_uri_mismatch"));
    }
    if p("response_type") != "code" {
        return Err(bad("unsupported_response_type"));
    }
    if p("code_challenge_method") != "S256" || p("code_challenge").is_empty() {
        return Err(bad("invalid_request: code_challenge (S256) is required"));
    }
    let state_param = p("state");
    if state_param.is_empty() {
        return Err(bad("invalid_request: state is required"));
    }
    let error = match p("mock_error") {
        "" => None,
        e => Some(dev::MockGoogleError::parse(e).ok_or_else(|| {
            let known: Vec<_> = dev::MockGoogleError::ALL.iter().map(|e| e.as_str()).collect();
            bad(&format!("unknown mock_error {e:?}; one of {}", known.join(", ")))
        })?),
    };
    let email = match p("login_hint").trim() {
        "" => dev::MOCK_GOOGLE_EMAIL.to_string(),
        e if e.contains('@') => e.to_lowercase(),
        e => return Err(bad(&format!("login_hint {e:?} is not an e-mail address"))),
    };
    let back = &cfg.redirect_uri;
    let state_q = crate::oauth::urlencode(state_param);
    if error == Some(dev::MockGoogleError::Cancel) {
        tracing::info!("mock google: {email} cancelled");
        return Ok(Redirect::to(&format!("{back}?error=access_denied&state={state_q}")).into_response());
    }
    let code = dev::MockCode {
        email: email.clone(),
        nonce: p("nonce").to_string(),
        challenge: p("code_challenge").to_string(),
        error,
        issued_at: crate::db::now(),
    };
    tracing::info!("mock google: {email} signed in{}", error.map(|e| format!(" (then: {})", e.as_str())).unwrap_or_default());
    let scope = crate::oauth::urlencode("email profile openid");
    Ok(Redirect::to(&format!("{back}?state={state_q}&code={}&scope={scope}&authuser=0&prompt=consent", crate::oauth::urlencode(&code.encode())))
        .into_response())
}

/// POST /dev/google/token: Google's token endpoint, as `oauth::exchange_code`
/// calls it. Checks the client, the grant type and the PKCE verifier, then
/// answers with tokens, or with what the code was told to fail with.
pub async fn google_token(State(state): State<Arc<AppState>>, Form(f): Form<Params>) -> Result<Response, AppError> {
    use axum::http::StatusCode;
    let cfg = mock_google_cfg(&state)?;
    let p = |k: &str| f.get(k).map(String::as_str).unwrap_or("");
    let refuse = |status: StatusCode, error: &str, desc: &str| {
        tracing::info!("mock google token: {error} ({desc})");
        (status, Json(serde_json::json!({ "error": error, "error_description": desc }))).into_response()
    };
    if p("grant_type") != "authorization_code" {
        return Ok(refuse(StatusCode::BAD_REQUEST, "unsupported_grant_type", "Invalid grant_type"));
    }
    if p("client_id") != cfg.client_id || p("client_secret") != cfg.client_secret {
        return Ok(refuse(StatusCode::UNAUTHORIZED, "invalid_client", "Unauthorized"));
    }
    let Some(code) = dev::MockCode::decode(p("code")) else {
        return Ok(refuse(StatusCode::BAD_REQUEST, "invalid_grant", "Malformed auth code."));
    };
    let now = crate::db::now();
    if now - code.issued_at > dev::MOCK_CODE_TTL_SECS || p("redirect_uri") != cfg.redirect_uri {
        return Ok(refuse(StatusCode::BAD_REQUEST, "invalid_grant", "Bad Request"));
    }
    if crate::oauth::pkce_challenge(p("code_verifier")) != code.challenge {
        return Ok(refuse(StatusCode::BAD_REQUEST, "invalid_grant", "Invalid code verifier."));
    }
    match code.error {
        Some(dev::MockGoogleError::InvalidGrant) => return Ok(refuse(StatusCode::BAD_REQUEST, "invalid_grant", "Bad Request")),
        Some(dev::MockGoogleError::Timeout) => {
            // Longer than the app waits (15 s), so its request times out.
            tracing::info!("mock google token: holding the answer for 20 s");
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        }
        Some(dev::MockGoogleError::Unavailable) => {
            tracing::info!("mock google token: 503");
            return Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                "<html><body><h1>503 Service Unavailable</h1><p>The service you requested is not available at this time.</p></body></html>",
            )
                .into_response());
        }
        Some(dev::MockGoogleError::Cancel) | Some(dev::MockGoogleError::Unverified) | None => {}
    }
    let verified = code.error != Some(dev::MockGoogleError::Unverified);
    let id_token = dev::mock_id_token(&cfg.client_id, &code.email, verified, &code.nonce, now);
    Ok(Json(serde_json::json!({
        "access_token": format!("ya29.mock.{}", crate::oauth::random_token()),
        "expires_in": 3599,
        "scope": "openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile",
        "token_type": "Bearer",
        "id_token": id_token,
    }))
    .into_response())
}

// ---------- /dev/jobs: the background job table (dev/jobs_page.rs) ----------

mod jobs_page;
pub use jobs_page::{jobs, jobs_act, jobs_fail};

#[cfg(test)]
mod tests;
