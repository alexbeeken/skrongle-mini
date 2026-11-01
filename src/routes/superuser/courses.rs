//! The course table on `/superuser` (#1034, hosted only): every made course,
//! searchable and filtered, flagged ones first (`course_flags::table`). A
//! row's title opens its detail dialog: why it was flagged, when and by
//! which check, any override note; and the actions, each behind a confirm
//! dialog: override the flag (with a note), take the override back, take
//! down or put back (#967), delete the course, suspend the owner (#967),
//! and e-mail the owner why it doesn't open (#1035, by hand only).
//! Every action is logged as who did what to which course, never the
//! content or the note.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::{done_about, moderates};
use crate::auth::CurrentUser;
use crate::course_flags::owner_mail::{self, Why};
use crate::course_flags::view::MatchView;
use crate::course_flags::table::{self, CourseRow, Show};
use crate::course_flags::{Source, State as FlagState};
use crate::error::AppError;
use crate::profile;
use crate::AppState;

/// The longest override note kept, in characters.
const NOTE_MAX: usize = 500;

/// The table's search, filter and page, from the query string. Kept on
/// every link the page makes, so paging either table keeps the other's place.
#[derive(Debug, Clone, Default)]
pub struct CourseQuery {
    pub q: String,
    pub show: Show,
    pub page: i64,
}

impl CourseQuery {
    pub fn new(q: Option<&str>, show: Option<&str>, page: Option<i64>) -> CourseQuery {
        CourseQuery { q: q.unwrap_or_default().trim().to_string(), show: Show::parse(show.unwrap_or_default()), page: page.unwrap_or(1) }
    }

    /// `&course_q=…&course_show=…&course_page=…`, the parts that differ from the default.
    pub fn params(&self) -> String {
        let mut s = String::new();
        if !self.q.is_empty() {
            s.push_str(&format!("&course_q={}", crate::oauth::urlencode(&self.q)));
        }
        if self.show != Show::All {
            s.push_str(&format!("&course_show={}", self.show.as_str()));
        }
        if self.page > 1 {
            s.push_str(&format!("&course_page={}", self.page));
        }
        s
    }
}

/// One choice of the filter picker.
pub struct ShowOption {
    pub value: &'static str,
    pub label: String,
    pub selected: bool,
}

/// One category's score (#1046), for the detail dialog.
pub struct ScoreView {
    pub category: String,
    pub label: String,
    /// Two decimals.
    pub score: String,
    /// At or over its threshold.
    pub high: bool,
}

/// A category's name in the superuser's language (an old name as it is).
pub fn category_label(name: &str) -> String {
    if crate::content_check::Category::named(name).is_some() {
        crate::t(&format!("superuser.courses.category.{name}"))
    } else {
        name.to_string()
    }
}

impl ScoreView {
    fn new(category: &str, score: f64) -> ScoreView {
        let threshold = crate::content_check::Category::named(category).map_or(f64::INFINITY, |c| c.threshold);
        ScoreView { category: category.to_string(), label: category_label(category), score: format!("{score:.2}"), high: score >= threshold }
    }
}

/// A part the AI check scored high (#1036), for the detail dialog.
pub struct HighPartView {
    pub lesson: String,
    pub chunk: String,
    /// The categories it scored high on, with their scores (#1046).
    pub high: Vec<ScoreView>,
    /// The AI review's word on it (#1037): queued, fine or harmful, and its name.
    pub review: String,
    pub review_label: String,
}

/// The AI check's scores of a course (#1036), for the detail dialog.
pub struct ScanView {
    /// The highest part score per category (#1046).
    pub scores: Vec<ScoreView>,
    pub parts: i64,
    /// What the stored scores cost, in dollars.
    pub cost: String,
    pub high: Vec<HighPartView>,
    /// The categories of the parts that are queued or harmful, by name,
    /// for the table (#1046); '' when none.
    pub high_in: String,
}

impl ScanView {
    fn new(s: &crate::content_check::Summary) -> ScanView {
        ScanView {
            scores: s.scores.iter().map(|(c, v)| ScoreView::new(c, *v)).collect(),
            parts: s.parts,
            cost: format!("${:.5}", s.cost_usd),
            high: s.high.iter().map(|h| HighPartView {
                    lesson: h.lesson.clone(),
                    chunk: h.chunk.clone(),
                    high: h.categories.iter().map(|c| ScoreView::new(c, h.score(c))).collect(),
                    review_label: crate::t(match h.review.as_str() {
                        "fine" => "superuser.courses.review_fine",
                        "harmful" => "superuser.courses.review_harmful",
                        _ => "superuser.courses.review_queued",
                    }),
                    review: h.review.clone(),
                }).collect(),
            high_in: s.high_categories().iter().map(|c| category_label(c)).collect::<Vec<_>>().join(", "),
        }
    }
}

/// One course of the table, ready for the template.
pub struct CourseView {
    pub id: String,
    pub title: String,
    pub owner: String,
    /// The owner is this superuser: no Suspend.
    pub owner_me: bool,
    pub owner_suspended: bool,
    pub created: String,
    pub edited: String,
    /// clear / flagged / review / overridden / down, and its name.
    pub state: &'static str,
    pub state_label: String,
    /// terms / ai, '' when nothing flagged it; and its name.
    pub source: &'static str,
    pub source_label: String,
    /// When the last check ran, '' when none has.
    pub checked: String,
    /// When the content check last looked at it (#1038), '' when never.
    pub ai_checked: String,
    /// The rules its checks ran under (#1039): one per check, '' when that
    /// check never ran.
    pub terms_version: Version,
    pub ai_version: Version,
    /// What triggered the flag, each word and its text around it masked
    /// (`templates/flag_matches.html`).
    pub matches: Vec<MatchView>,
    /// The AI's report (#1037), '' when there is none.
    pub reason: String,
    pub override_note: String,
    pub overridden_by: String,
    pub overridden_at: String,
    pub taken_down: bool,
    pub busy: bool,
    /// The AI check's scores, `None` when it never scanned the course.
    pub scan: Option<ScanView>,
    /// When the owner was last e-mailed about it (#1035), '' when never.
    pub owner_emailed: String,
    /// The e-mail to the owner, drafted in their language (#1035): `Some`
    /// when the course is flagged or taken down, the owner has an address
    /// and this server sends mail.
    pub mail: Option<OwnerMail>,
}

/// The draft of the e-mail to a course's owner (#1035).
pub struct OwnerMail {
    pub subject: String,
    pub text: String,
}

/// The rules version one check ran under (#1039).
pub struct Version {
    /// "Terms v1", '' when the check never ran.
    pub label: String,
    /// Older than the current rules: the nightly run checks it again.
    pub behind: bool,
    /// Says so, with the current version, '' when not behind.
    pub behind_label: String,
}

impl Version {
    fn new(key: &str, ran: Option<i64>, current: i64) -> Version {
        let behind = ran.is_some_and(|v| v < current);
        Version {
            label: ran.map(|v| crate::t1(key, "version", v)).unwrap_or_default(),
            behind,
            behind_label: if behind { crate::t1("superuser.courses.version_behind", "current", current) } else { String::new() },
        }
    }
}

impl CourseView {
    fn new(r: CourseRow, me: i64, sends_mail: bool, site: &str) -> CourseView {
        let state = r.state();
        let flag = r.flag.as_ref();
        let mail = match Why::of(r.taken_down, flag) {
            Some(why) if sends_mail && !r.owner_email.is_empty() => {
                let (subject, text) = crate::i18n::in_lang(&r.owner_lang, || owner_mail::draft(&r.title, &why, site));
                Some(OwnerMail { subject, text })
            }
            _ => None,
        };
        let flagged = flag.is_some_and(|f| f.state != FlagState::Clear);
        let source = match flag.map(|f| f.source) {
            Some(Source::Ai) if flagged => "ai",
            Some(Source::Terms) if flagged => "terms",
            _ => "",
        };
        CourseView {
            owner_me: r.owner_id == me,
            owner_suspended: r.owner_suspended,
            created: profile::long_date(r.created_at),
            edited: profile::long_date(r.updated_at),
            state,
            state_label: crate::t(&format!("superuser.courses.state_{state}")),
            source,
            source_label: if source.is_empty() { String::new() } else { crate::t(&format!("superuser.courses.source_{source}")) },
            checked: flag.map(|f| profile::long_date(f.checked_at)).unwrap_or_default(),
            ai_checked: r.checked_at.map(profile::long_date).unwrap_or_default(),
            terms_version: Version::new("superuser.courses.version_terms", flag.map(|f| f.terms_version), crate::course_flags::terms::TERMS_VERSION),
            ai_version: Version::new("superuser.courses.version_ai", r.scan.as_ref().map(|s| s.ai_version), crate::content_check::AI_CHECK_VERSION),
            matches: flag
                .map(|f| f.matches.iter().map(|m| MatchView::superuser(f, m)).collect())
                .unwrap_or_default(),
            reason: flag.map(|f| f.reason.clone()).unwrap_or_default(),
            override_note: flag.and_then(|f| f.override_note.clone()).unwrap_or_default(),
            overridden_at: flag.and_then(|f| f.overridden_at).map(profile::long_date).unwrap_or_default(),
            overridden_by: r.overridden_by_email,
            taken_down: r.taken_down,
            busy: r.busy,
            owner_emailed: r.owner_emailed_at.map(profile::long_date).unwrap_or_default(),
            mail,
            scan: r.scan.as_ref().map(ScanView::new),
            id: r.id,
            title: r.title,
            owner: r.owner_email,
        }
    }

    pub fn flagged(&self) -> bool {
        self.state == "flagged"
    }

    pub fn overridden(&self) -> bool {
        self.state == "overridden"
    }

    /// Each check's rules version, by name (#1039).
    pub fn versions(&self) -> [(&'static str, &Version); 2] {
        [("terms", &self.terms_version), ("ai", &self.ai_version)]
    }
}

/// The table as the template shows it.
pub struct CourseTable {
    pub list: Vec<CourseView>,
    pub q: String,
    pub shows: Vec<ShowOption>,
    pub total: i64,
    pub page: i64,
    pub pages: i64,
    pub prev: Option<String>,
    pub next: Option<String>,
    /// `SKRONGLE_CONTENT_CHECK=external` (#1041): the worker's check-in.
    pub worker: Option<WorkerView>,
}

/// The content-check worker as /superuser shows it (#1041): an (i) on the
/// course table's heading with the details, and a banner at the top of the
/// page while it is silent.
pub struct WorkerView {
    /// "Last check-in: …", or that it never checked in.
    pub seen: String,
    pub waiting: i64,
    /// Silent for a day or more, or never seen.
    pub silent: bool,
    /// The banner's text while it is silent, '' while it is fine.
    pub alert: String,
}

impl WorkerView {
    fn new(s: &crate::content_check::worker::Status, now: i64) -> WorkerView {
        let when = s.seen_at.map(|t| format!("{} {:02}:{:02} UTC", profile::long_date(t), t.rem_euclid(86_400) / 3600, t.rem_euclid(3600) / 60));
        let seen = match &when {
            Some(w) => crate::t1("superuser.courses.checker_seen", "when", w),
            None => crate::t("superuser.courses.checker_never"),
        };
        let silent = s.silent(now);
        let alert = match (&when, silent) {
            (_, false) => String::new(),
            (Some(w), true) => crate::t2("superuser.courses.checker_alert_since", "when", w, "n", s.waiting),
            (None, true) => crate::t1("superuser.courses.checker_alert_never", "n", s.waiting),
        };
        WorkerView { seen, waiting: s.waiting, silent, alert }
    }
}

/// A filter's name in the picker; "behind" (#1040) says by how much and how many courses wait.
fn show_label(s: Show, behind: i64) -> String {
    match s {
        Show::Behind => crate::t2(s.label_key(), "n", crate::content_check::nightly::settings().recheck_behind, "count", behind),
        _ => crate::t(s.label_key()),
    }
}

/// The table at `query`; `accounts_page` is kept on its links.
/// `site` is where links in e-mail point (the owner e-mail's link, #1035).
pub async fn load(state: &AppState, user: &CurrentUser, query: &CourseQuery, accounts_page: i64, site: &str) -> Result<CourseTable, AppError> {
    let (q, show, page) = (query.q.clone(), query.show, query.page);
    let (found, behind) = state.db.call(move |c| Ok((table::courses(c, &q, show, page)?, table::behind_count(c)?))).await?;
    let sends_mail = state.caps().mail;
    let worker = match crate::content_check::external() {
        true => Some(state.db.call(|c| crate::content_check::worker::status(c)).await?),
        false => None,
    };
    let link = |p: i64| {
        let at = CourseQuery { page: p, ..query.clone() };
        crate::url(&format!("/superuser?page={accounts_page}{}#courses", at.params()))
    };
    Ok(CourseTable {
        list: found.list.into_iter().map(|r| CourseView::new(r, user.id, sends_mail, site)).collect(),
        q: query.q.clone(),
        shows: Show::ALL.into_iter().map(|s| ShowOption { value: s.as_str(), label: show_label(s, behind), selected: s == query.show }).collect(),
        total: found.total,
        page: found.page,
        pages: found.pages,
        prev: (found.page > 1).then(|| link(found.page - 1)),
        next: (found.page < found.pages).then(|| link(found.page + 1)),
        worker: worker.map(|w| WorkerView::new(&w, crate::db::now())),
    })
}

#[derive(Deserialize)]
pub struct OverrideForm {
    #[serde(default)]
    pub course: String,
    #[serde(default)]
    pub note: String,
}

/// POST /superuser/courses/override (course=…, note=…): the flag was
/// wrong; the course opens and shares again. The note is required.
pub async fn override_flag(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<OverrideForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let id = form.course.trim().to_string();
    let note: String = form.note.trim().chars().take(NOTE_MAX).collect();
    if note.is_empty() {
        return Ok(done_about("no-note", &id));
    }
    let (cid, uid) = (id.clone(), user.id);
    if !state.db.call(move |c| crate::course_flags::override_flag(c, &cid, &note, uid, crate::db::now())).await? {
        return Ok(done_about("no-course", &id));
    }
    state.courses.invalidate(&id);
    tracing::info!("superuser {} overrode the flag on course {id}", user.id);
    Ok(done_about("overridden", &id))
}

#[derive(Deserialize)]
pub struct CourseForm {
    #[serde(default)]
    pub course: String,
}

/// POST /superuser/courses/clear-override (course=…): the override is taken
/// back, and the course is checked again on its next load.
pub async fn clear_override(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<CourseForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let id = form.course.trim().to_string();
    let cid = id.clone();
    let cleared = state
        .db
        .call(move |c| {
            if !crate::course_flags::clear_override(c, &cid)? {
                return Ok(false);
            }
            // The verdict now: a check of the files as they are.
            let files = crate::courses::load_files(c, &cid)?;
            crate::course_flags::check(c, &cid, &files)?;
            Ok(true)
        })
        .await?;
    if !cleared {
        return Ok(done_about("no-course", &id));
    }
    state.courses.invalidate(&id);
    tracing::info!("superuser {} took back the override on course {id}", user.id);
    Ok(done_about("override-cleared", &id))
}

/// POST /superuser/courses/delete (course=…): the course, its files and its
/// shares are deleted. Not undone. Its content reports stay (they are the
/// record, docs/content-reports.md). Refused while a generation or revision
/// writes to it.
pub async fn delete_course(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<CourseForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let id = form.course.trim().to_string();
    let cid = id.clone();
    let row = state.db.call(move |c| crate::courses::course(c, &cid)).await?;
    let Some(row) = row else {
        return Ok(done_about("no-course", &id));
    };
    if row.is_busy() {
        return Ok(done_about("course-busy", &id));
    }
    let cid = id.clone();
    state.db.call(move |c| crate::courses::delete_course(c, &cid)).await?;
    state.courses.invalidate(&id);
    tracing::info!("superuser {} deleted course {id} (owner user {})", user.id, row.owner_id);
    Ok(done_about("course-deleted", &id))
}

#[derive(Deserialize)]
pub struct OwnerMailForm {
    #[serde(default)]
    pub course: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub text: String,
}

/// POST /superuser/courses/email-owner (course=…, subject=…, text=…): the
/// owner of a flagged or taken-down course gets the superuser's e-mail
/// (#1035), through the outbox; the course remembers when. Only ever sent
/// from this form, never on its own. A 404 on a server without mail. The
/// log names who sent which kind of e-mail, never the text.
pub async fn email_owner(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<OwnerMailForm>) -> Result<Response, AppError> {
    if !moderates(&user) || !state.caps().mail {
        return Err(AppError::NotFound);
    }
    let id = form.course.trim().to_string();
    let (subject, text) = (owner_mail::clean_subject(&form.subject), owner_mail::clean_text(&form.text));
    if subject.is_empty() || text.is_empty() {
        return Ok(done_about("no-mail-text", &id));
    }
    let cid = id.clone();
    let owner = state.db.call(move |c| owner_mail::owner(c, &cid)).await?;
    let Some(owner_mail::Owner { id: owner_id, email, why: Some(why) }) = owner else {
        return Ok(done_about("no-course", &id));
    };
    let mail = match crate::mail::Mail::new(&email, &subject, &text, owner_mail::PURPOSE) {
        Ok(m) => m.for_user(owner_id),
        Err(e) => {
            tracing::warn!("course {id}: cannot mail the owner (user {owner_id}): {e}");
            return Ok(done_about("no-owner-address", &id));
        }
    };
    state.mail.queue(&state.db, mail).await?;
    let cid = id.clone();
    state.db.call(move |c| owner_mail::mark_emailed(c, &cid, crate::db::now())).await?;
    tracing::info!("superuser {} e-mailed the owner (user {owner_id}) of course {id}: {}", user.id, why.kind());
    let to = format!("/superuser?did=owner-emailed&about={}&email={}", crate::oauth::urlencode(&id), crate::oauth::urlencode(&email));
    Ok(axum::response::Redirect::to(&crate::url(&to)).into_response())
}
#[cfg(test)]
mod tests;
