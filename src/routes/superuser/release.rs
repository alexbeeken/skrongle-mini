//! `/superuser/release` (#934): the waitlist and its switch. Who joined,
//! who confirmed, who was invited and who signed up; invite one person,
//! or the next N in the order they joined; turn the waitlist on (sign-up
//! by e-mailed invitation only) or off (the launch). Superusers on the
//! hosted site only: Skrongle mini has no e-mail. A 404 for anyone else.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use crate::routes::Page;
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::profile;
use crate::waitlist::{self, Counts, Invitation};
use crate::AppState;

#[derive(Template)]
#[template(path = "superuser_release.html")]
pub struct ReleaseTemplate {
    pub user: Option<CurrentUser>,
    pub notice: Option<String>,
    pub error: Option<String>,
    /// Dev mode with the dev mailbox: where the invitation just sent landed.
    pub inbox: Option<String>,
    /// The waitlist is on.
    pub on: bool,
    pub counts: Counts,
    pub list: Vec<Row>,
    pub page: i64,
    pub pages: i64,
    pub prev: Option<String>,
    pub next: Option<String>,
    pub max_batch: i64,
}

pub struct Row {
    pub id: i64,
    pub email: String,
    pub joined: String,
    /// `waitlist::Status::as_str`, and its label.
    pub status: &'static str,
    pub status_label: String,
    /// When the last invitation went, or ''.
    pub invited: String,
    pub invites_sent: i64,
    /// Invite (waiting) or Resend (invited, expired): None when neither fits.
    pub action: Option<String>,
}

/// May this user use the page? A superuser on a hosted server that sends mail.
fn allowed(state: &AppState, user: &CurrentUser) -> bool {
    user.superuser && !crate::is_mini() && state.caps().mail
}

#[derive(Deserialize)]
pub struct ShowQuery {
    pub page: Option<i64>,
    /// Back from an action: `on`, `off`, `invited` (about `email`),
    /// `invited-n` (`n` people), `not-invited`.
    pub did: Option<String>,
    pub email: Option<String>,
    pub n: Option<i64>,
}

/// GET /superuser/release?page=N
pub async fn show(State(state): State<Arc<AppState>>, user: CurrentUser, Query(q): Query<ShowQuery>) -> Result<Response, AppError> {
    if !allowed(&state, &user) {
        return Err(AppError::NotFound);
    }
    let now = crate::db::now();
    let page = q.page.unwrap_or(1);
    let (counts, (entries, page, pages)) = state.db.call(move |c| Ok((waitlist::counts(c)?, waitlist::list(c, page, now)?))).await?;
    let link = |p: i64| crate::url(&format!("/superuser/release?page={p}"));
    let email = q.email.unwrap_or_default();
    let (mut notice, mut error, mut inbox) = (None, None, None);
    match q.did.as_deref() {
        Some("on") => notice = Some(crate::t("release.turned_on")),
        Some("off") => notice = Some(crate::t("release.turned_off")),
        Some("invited") => {
            notice = Some(crate::t1("release.invited_one", "email", &email));
            if state.dev_inbox.is_some() {
                inbox = Some(crate::url(&format!("/dev/inbox/latest?to={}", crate::oauth::urlencode(&email))));
            }
        }
        Some("invited-n") => notice = Some(crate::tn("release.invited_n", q.n.unwrap_or(0))),
        Some("not-invited") => error = Some(crate::t1("release.not_invited", "email", &email)),
        _ => {}
    }
    let t = ReleaseTemplate {
        user: Some(user),
        notice,
        error,
        inbox,
        on: waitlist::on(),
        counts,
        list: entries
            .into_iter()
            .map(|e| {
                use waitlist::Status::*;
                Row {
                    id: e.id,
                    joined: profile::long_date(e.joined_at),
                    status: e.status.as_str(),
                    status_label: crate::t(&format!("release.status.{}", e.status.as_str().replace('-', "_"))),
                    invited: e.invited_at.map(profile::long_date).unwrap_or_default(),
                    invites_sent: e.invites_sent,
                    action: match e.status {
                        Waiting => Some(crate::t("release.invite")),
                        Invited | Expired => Some(crate::t("release.resend")),
                        Unconfirmed | SignedUp => None,
                    },
                    email: e.email,
                }
            })
            .collect(),
        page,
        pages,
        prev: (page > 1).then(|| link(page - 1)),
        next: (page < pages).then(|| link(page + 1)),
        max_batch: waitlist::MAX_BATCH,
    };
    Ok(Page(t).into_response())
}

fn back(query: &str) -> Response {
    Redirect::to(&crate::url(&format!("/superuser/release?{query}"))).into_response()
}

#[derive(Deserialize)]
pub struct SwitchForm {
    /// "on" or "off".
    #[serde(default)]
    pub to: String,
}

/// POST /superuser/release/switch (to=on|off): the waitlist on or off, at once.
pub async fn switch(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<SwitchForm>) -> Result<Response, AppError> {
    if !allowed(&state, &user) {
        return Err(AppError::NotFound);
    }
    let on = form.to == "on";
    let st = Arc::clone(&state);
    state.db.call(move |c| st.waitlist.set(c, on)).await?;
    tracing::info!("superuser {} turned the waitlist {}", user.id, if on { "on" } else { "off" });
    Ok(back(if on { "did=on" } else { "did=off" }))
}

/// Queues one invitation e-mail inside the invite's transaction: false when
/// a mail cap says no (the batch stops there).
fn sender(site: String, ceiling: Option<crate::mail::Ceiling>) -> impl FnMut(&rusqlite::Connection, &Invitation) -> rusqlite::Result<bool> {
    move |c, inv| {
        let link = format!("{site}/register?invite={}", inv.token);
        match waitlist::invite_mail(&inv.email, &link) {
            Ok(mail) => Ok(crate::mail::enqueue(c, &mail, crate::db::now(), ceiling)?.is_ok()),
            Err(e) => {
                tracing::warn!("waitlist: no invitation for an address the mailer refuses: {e}");
                Ok(false)
            }
        }
    }
}

#[derive(Deserialize)]
pub struct InviteForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /superuser/release/invite (id=…): invite this person, or invite
/// them again with a new link (the old one stops working).
pub async fn invite(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<InviteForm>) -> Result<Response, AppError> {
    if !allowed(&state, &user) {
        return Err(AppError::NotFound);
    }
    let (id, uid) = (form.id, user.id);
    let mut send = sender(crate::verify::public_base(&state, &headers), state.mail.ceiling());
    let found = state
        .db
        .call(move |c| {
            let email: Option<String> = rusqlite::OptionalExtension::optional(c.query_row("SELECT email FROM waitlist WHERE id = ?1", [id], |r| r.get(0)))?;
            Ok((waitlist::invite_one(c, id, uid, crate::db::now(), &mut send)?, email.unwrap_or_default()))
        })
        .await?;
    let (sent, email) = found;
    if !sent {
        return Ok(back(&format!("did=not-invited&email={}", crate::oauth::urlencode(&email))));
    }
    state.mail.notify();
    tracing::info!("superuser {uid} invited waitlist entry {id}");
    Ok(back(&format!("did=invited&email={}", crate::oauth::urlencode(&email))))
}

#[derive(Deserialize)]
pub struct InviteNextForm {
    #[serde(default)]
    pub n: i64,
}

/// POST /superuser/release/invite-next (n=…): invite the next `n` confirmed
/// people in the order they joined (at most `waitlist::MAX_BATCH`).
pub async fn invite_next(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<InviteNextForm>) -> Result<Response, AppError> {
    if !allowed(&state, &user) {
        return Err(AppError::NotFound);
    }
    let (n, uid) = (form.n.clamp(0, waitlist::MAX_BATCH), user.id);
    let mut send = sender(crate::verify::public_base(&state, &headers), state.mail.ceiling());
    let invited = state.db.call(move |c| waitlist::invite_next(c, n, uid, crate::db::now(), &mut send)).await?;
    if invited > 0 {
        state.mail.notify();
    }
    tracing::info!("superuser {uid} invited the next {invited} of {n} asked from the waitlist");
    Ok(back(&format!("did=invited-n&n={invited}")))
}
