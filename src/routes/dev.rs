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
    /// `1`: give the dev user premium (a Plus grant); `0`: take it away.
    pub premium: Option<String>,
    /// `basic` | `plus`: put the dev user on that plan through the dev
    /// payment provider (a Checkout paid at once, #963); `free`: cancel it.
    pub plan: Option<String>,
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
    if let Some(p) = q.premium.as_deref() {
        set_premium(&state, uid, p).await?;
    }
    if let Some(p) = q.plan.as_deref() {
        let plan = crate::billing::Plan::parse(p).ok_or_else(|| AppError::BadRequest(format!("plan={p}: use basic, plus or free")))?;
        super::dev_payments::set_plan(&state, uid, plan).await?;
        tracing::info!("dev: user {uid} put on the {} plan", plan.as_str());
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

/// `1` grants Plus (no end), `0` revokes the grant; a Stripe subscription or
/// a plan bought on the profile, if any, is left alone.
async fn set_premium(state: &AppState, uid: i64, on: &str) -> Result<(), AppError> {
    let on = match on {
        "1" => true,
        "0" => false,
        other => return Err(AppError::BadRequest(format!("premium={other}: use 1 or 0"))),
    };
    state
        .db
        .call(move |c| if on { crate::billing::set_grant(c, uid, None, "dev toggle") } else { crate::billing::revoke_grant(c, uid) })
        .await?;
    tracing::info!("dev: premium {} for user {uid}", if on { "granted" } else { "revoked" });
    Ok(())
}

#[derive(Deserialize)]
pub struct PremiumForm {
    #[serde(default)]
    pub premium: String,
}

/// POST /dev/premium (premium=1|0): the profile page's dev toggle, for whoever is signed in.
pub async fn premium(State(state): State<Arc<AppState>>, user: CurrentUser, Form(f): Form<PremiumForm>) -> Result<Response, AppError> {
    set_premium(&state, user.id, &f.premium).await?;
    Ok(Redirect::to(&crate::url("/profile#subscription")).into_response())
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
    box_page(&state, user, "out", q.to.as_deref()).await
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

// ---------- the dev mailbox page: e-mail, inbox and outbox ----------
//
// One page with an Inbox and an Outbox tab (`/dev/inbox`, `/dev/mail`: the
// dev mailbox, mail/sink.rs, and the mail outbox). Every box is a list like
// a mail client's: unopened rows in bold, archived ones
// struck out at the bottom, Delete removes a row for good. The page polls
// `/dev/inbox/rows` (static/devbox.js) so new messages, state changes and
// the buttons show without a reload. Opened and archived are `dev::Marks`,
// in memory.

/// `dev::Marks` boxes.
const MAIL_IN: &str = "mail-in";
const MAIL_OUT: &str = "mail-out";

/// One row of a dev box's table.
pub struct BoxRow {
    pub id: i64,
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
    /// Outbox: the copy the dev mailbox received, if it did.
    pub inbox_href: Option<String>,
}

/// One box's rows: part of the page, and on their own for its poll.
#[derive(Template)]
#[template(path = "dev_box_rows.html")]
pub struct BoxRows {
    /// Test id prefix of the buttons: `dev-inbox`, `dev-mail`.
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
    /// The section: `mail`.
    pub kind: &'static str,
    /// The tab showing when the page opens: `in` or `out`.
    pub tab: &'static str,
    /// The address filter, for both tabs.
    pub to: String,
    /// `?to=…` for the tabs' links; empty without a filter.
    pub to_query: String,
    /// `127.0.0.1:<port>` of the dev mailbox.
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
        crate::url("/dev/inbox")
    }
    pub fn out_url(&self) -> String {
        crate::url("/dev/mail")
    }
    /// What the page polls.
    pub fn rows_url(&self) -> String {
        format!("{}?kind={}{}", crate::url("/dev/inbox/rows"), self.kind, self.to_query.replacen('?', "&", 1))
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
    BoxRows { prefix: "dev-inbox", row_testid: "dev-inbox-item", open_testid: "dev-inbox-open", empty: no_messages(to), rows: ordered(rows) }
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
    Ok(BoxRows { prefix: "dev-mail", row_testid: "dev-mail-message", open_testid: "dev-mail-view", empty: no_messages(to), rows: ordered(rows) })
}


fn filter_of(to: Option<&str>) -> Option<&str> {
    to.map(str::trim).filter(|t| !t.is_empty())
}

/// Both boxes.
async fn boxes(state: &AppState, to: Option<&str>) -> Result<(BoxRows, BoxRows), AppError> {
    let now = crate::db::now();
    Ok((inbox_rows(state, to, now), outbox_rows(state, to, now).await?))
}

async fn box_page(state: &AppState, user: Option<CurrentUser>, tab: &'static str, to: Option<&str>) -> Result<Response, AppError> {
    let to = filter_of(to);
    let (inbox, outbox) = boxes(state, to).await?;
    let default_to = user.as_ref().map(|u| u.email.clone()).unwrap_or_else(|| dev::DEV_EMAIL.to_string());
    let (sink, providers) = (state.dev_inbox.as_ref().map(|i| i.addr().to_string()), state.mail.providers.join(", then "));
    Ok(Page(BoxTemplate {
        user,
        kind: "mail",
        tab,
        to: to.unwrap_or_default().to_string(),
        to_query: to.map(|t| format!("?to={}", crate::urlenc::urlencode(t))).unwrap_or_default(),
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
    box_page(&state, user, "in", q.to.as_deref()).await
}

/// GET /dev/inbox/rows (`?to=`): one section's two lists
/// as HTML, and the unread counts, for the page's poll:
/// `{"in": {"html", "unread"}, "out": {…}}`.
pub async fn box_rows(State(state): State<Arc<AppState>>, Query(q): Query<InboxParams>) -> Result<Response, AppError> {
    let (inbox, outbox) = boxes(&state, filter_of(q.to.as_deref())).await?;
    let mut out = serde_json::Map::new();
    for (key, rows) in [("in", inbox), ("out", outbox)] {
        out.insert(key.into(), serde_json::json!({ "html": rows.render()?, "unread": rows.unread() }));
    }
    Ok(Json(serde_json::Value::Object(out)).into_response())
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

// ---------- /dev/jobs: the background job table (dev/jobs_page.rs) ----------

mod jobs_page;
pub use jobs_page::{jobs, jobs_act, jobs_fail};

#[cfg(test)]
mod tests;
