//! `/students` (#218, `docs/features.md` "The Students page"): where a
//! teacher (`Perm::HoldStudents`, or an owner) makes and looks after its
//! student accounts (`students.rs`, #217). Anyone else gets a 404: a student
//! through the router's `auth::student_gate` (`students::refused` lists
//! `/students`), an account that may not hold students here.
//!
//! Every control is a plain form that changes one thing and comes back, so
//! the pages work with JavaScript off:
//!
//! - `GET /students`: the list (all on one page) and **Add students**.
//! - `POST /students/add` (`names`, one per line): a row per name with a
//!   suggested username and an empty password. Nothing is made yet.
//! - `POST /students/sheet` (`name`, `username`, `password` per row): makes
//!   every account or none, then answers with the sign-in sheet.
//! - `GET /students/sheet`: the sheet's address again: no passwords, only
//!   that they were shown once.
//! - `POST /students/passwords` and `POST /students/delete` (`id` per
//!   ticked student): a confirm page first, then (with `confirm=1`) new
//!   passwords on a sheet, or the students deleted.
//! - `GET /students/{id}`, `POST /students/{id}` (username and names),
//!   `POST /students/{id}/password` (typed, or made when empty: a sheet),
//!   `POST /students/{id}/delete` (a confirm page, then `confirm=1`).
//!
//! Passwords are shown once, as Manage Users shows a temporary password and
//! an invitation link: only in the answer to the POST that set them, with
//! `Cache-Control: no-store`. Only their hashes are kept; nothing else holds
//! them, so a later `GET /students/sheet` has nothing to show.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser};
use crate::error::AppError;
use crate::perms::Perm;
use crate::students::{self, NewStudent, Refused, Student};
use crate::AppState;

/// A student quiet for this many days or more has its "last studied" in bold.
pub const QUIET_DAYS: i64 = 7;

/// The page is the teacher's: anyone who may not hold students gets a 404.
fn teacher(user: &CurrentUser) -> Result<(), AppError> {
    if user.can(Perm::HoldStudents) { Ok(()) } else { Err(AppError::NotFound) }
}

/// The name a list or a slip shows: first and last name, else the username.
fn shown_name(s: &Student) -> String {
    let full = format!("{} {}", s.first_name.trim(), s.last_name.trim());
    let full = full.trim();
    if full.is_empty() { s.username.clone() } else { full.to_string() }
}

/// A pasted name as first and last name: the first word, then the rest.
fn split_name(name: &str) -> (String, String) {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    match name.split_once(' ') {
        Some((first, last)) => (first.to_string(), last.to_string()),
        None => (name, String::new()),
    }
}

/// "Today", "Yesterday", "9 days ago" or "Never", in the teacher's local
/// days (`CurrentUser::tz`), and whether it is quiet (`QUIET_DAYS`).
fn last_studied(at: Option<i64>, tz: i64, now: i64) -> (String, bool) {
    let Some(at) = at else {
        return (crate::t("students_page.never"), false);
    };
    let days = (crate::earnings::day_of(tz, now) - crate::earnings::day_of(tz, at)).max(0);
    let text = match days {
        0 => crate::t("students_page.today"),
        1 => crate::t("students_page.yesterday"),
        n => crate::tn("students_page.days_ago", n),
    };
    (text, days >= QUIET_DAYS)
}

/// The values of every field named `key`, in the order sent.
fn all<'a>(form: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    form.iter().filter(|(k, _)| k == key).map(|(_, v)| v.as_str()).collect()
}

fn one<'a>(form: &'a [(String, String)], key: &str) -> &'a str {
    form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or_default()
}

/// The ticked students' ids, each once.
fn picked(form: &[(String, String)]) -> Vec<i64> {
    let mut ids: Vec<i64> = all(form, "id").iter().filter_map(|v| v.trim().parse().ok()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Back to the list after an action: what it did (`did`), and how many.
fn back(did: &str, n: usize) -> Response {
    Redirect::to(&crate::url(&format!("/students?did={did}&n={n}"))).into_response()
}

/// How many more student accounts the box takes (every teacher's count
/// against `caps::student_limit`), and whether it is full (the dev server
/// never is).
async fn room(state: &AppState) -> Result<(i64, bool), AppError> {
    let all = state.db.call(|c| students::count_all(c)).await?;
    let dev = state.dev_mode();
    Ok(((crate::caps::student_limit() as i64 - all).max(0), crate::caps::students_full(all, dev)))
}

/// Why a batch or a rename was refused, in words.
fn refusal(r: &Refused) -> String {
    match r {
        Refused::NotTeacher => crate::t("error.no_permission"),
        Refused::Full => crate::t1("students_page.full", "max", crate::caps::student_limit()),
        Refused::BadUsername(name, why) => format!("{name}: {why}"),
        Refused::Taken(name) => crate::t1("students_page.taken", "username", name),
    }
}

// ---------- the list ----------

pub struct Row {
    pub id: i64,
    pub name: String,
    pub username: String,
    pub passed: i64,
    pub last_studied: String,
    /// Quiet for `QUIET_DAYS` or more: bold.
    pub quiet: bool,
    pub never: bool,
    /// The titles of the courses given to it (#220), by title.
    pub courses: Vec<String>,
}

#[derive(Template)]
#[template(path = "students.html")]
pub struct ListTemplate {
    pub user: Option<CurrentUser>,
    /// The teacher's students, by username.
    pub rows: Vec<Row>,
    /// The box's student limit (`caps::student_limit`, `SKRONGLE_STUDENT_LIMIT`).
    pub limit: usize,
    /// How many more the box takes, and whether it takes none.
    pub room: i64,
    pub full: bool,
    pub notice: Option<String>,
    pub error: Option<String>,
    /// The names pasted, kept when they could not be taken.
    pub names: String,
    /// The courses the teacher can give (`givable`): `(id, title)`.
    pub givable: Vec<(String, String)>,
}

/// The teacher's courses it can give now: its own, ready, that load
/// (`students::givable`, then each course loaded).
async fn givable(state: &AppState, teacher_id: i64) -> Result<Vec<(String, String)>, AppError> {
    let mut out = Vec::new();
    for (id, title) in state.db.call(move |c| students::givable(c, teacher_id)).await? {
        if state.courses.get(&state.db, &id).await?.is_some() {
            out.push((id, title));
        }
    }
    Ok(out)
}

async fn load_list(state: &AppState, user: CurrentUser) -> Result<ListTemplate, AppError> {
    let (teacher, tz, now) = (user.id, user.tz, crate::db::now());
    let (list, progress, given) = state.db.call(move |c| Ok((students::list(c, teacher)?, students::progress(c, teacher)?, students::given(c, teacher)?))).await?;
    let (room, full) = room(state).await?;
    let givable = givable(state, teacher).await?;
    let rows = list
        .iter()
        .map(|s| {
            let p = progress.get(&s.id).copied().unwrap_or_default();
            let (last, quiet) = last_studied(p.last_studied, tz, now);
            let courses = given.iter().filter(|g| g.student_id == s.id).map(|g| g.title.clone()).collect();
            Row { id: s.id, name: shown_name(s), username: s.username.clone(), passed: p.passed, last_studied: last, quiet, never: p.last_studied.is_none(), courses }
        })
        .collect();
    Ok(ListTemplate { user: Some(user), rows, limit: crate::caps::student_limit(), room, full, notice: None, error: None, names: String::new(), givable })
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub did: Option<String>,
    pub n: Option<usize>,
    /// The course given (`did=given`), named in the notice.
    pub course: Option<String>,
}

/// GET /students
pub async fn list(State(state): State<Arc<AppState>>, user: CurrentUser, Query(q): Query<ListQuery>) -> Result<Response, AppError> {
    teacher(&user)?;
    let mut t = load_list(&state, user).await?;
    let n = q.n.unwrap_or(0);
    match q.did.as_deref() {
        Some("deleted") => t.notice = Some(crate::tn("students_page.deleted", n as i64)),
        Some("none-picked") => t.error = Some(crate::t("students_page.none_picked")),
        Some("no-course") => t.error = Some(crate::t("students_page.no_course")),
        Some("given") => {
            // The title comes from the teacher's own courses, never the URL.
            let title = q.course.as_deref().and_then(|id| t.givable.iter().find(|(c, _)| c == id)).map(|(_, title)| title.clone());
            if let Some(title) = title {
                t.notice = Some(crate::t2("students_page.given", "course", title, "n", n));
            }
        }
        _ => {}
    }
    Ok(Page(t).into_response())
}

// ---------- adding: names, then rows, then the sheet ----------

pub struct AddRow {
    pub name: String,
    pub username: String,
    pub password: String,
}

#[derive(Template)]
#[template(path = "students_add.html")]
pub struct AddTemplate {
    pub user: Option<CurrentUser>,
    pub rows: Vec<AddRow>,
    pub room: i64,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct NamesForm {
    #[serde(default)]
    pub names: String,
}

/// POST /students/add (names=…): one row per pasted name, each with a free
/// username (`students::suggest_username`) and an empty password. Nothing
/// is made; too many names or none is the list again with why.
pub async fn add(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<NamesForm>) -> Result<Response, AppError> {
    teacher(&user)?;
    let names: Vec<String> = form.names.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    let (room, full) = room(&state).await?;
    let too_many = full || (!state.dev_mode() && names.len() as i64 > room) || names.len() > *crate::caps::STUDENT_LIMIT_RANGE.end();
    if names.is_empty() || too_many {
        let mut t = load_list(&state, user).await?;
        t.error = Some(if names.is_empty() { crate::t("students_page.no_names") } else { crate::tn("students_page.too_many", room) });
        t.names = form.names;
        return Ok(Page(t).into_response());
    }
    let rows = state
        .db
        .call(move |c| {
            let mut taken: Vec<String> = Vec::with_capacity(names.len());
            let mut rows = Vec::with_capacity(names.len());
            for name in names {
                let username = students::suggest_username(c, &name, &taken)?;
                taken.push(username.clone());
                rows.push(AddRow { name, username, password: String::new() });
            }
            Ok(rows)
        })
        .await?;
    Ok(Page(AddTemplate { user: Some(user), rows, room, error: None }).into_response())
}

/// GET /students/add: the form is on the list.
pub async fn add_page(user: CurrentUser) -> Result<Response, AppError> {
    teacher(&user)?;
    Ok(Redirect::to(&crate::url("/students")).into_response())
}

pub struct Slip {
    pub name: String,
    pub username: String,
    pub password: String,
}

#[derive(Template)]
#[template(path = "students_sheet.html")]
pub struct SheetTemplate {
    pub user: Option<CurrentUser>,
    /// One per student; empty for the sheet's address opened again.
    pub slips: Vec<Slip>,
    /// Where the students sign in: the address in the teacher's browser.
    pub address: String,
    /// What was done, above the slips: "3 students added", "New passwords for 2 students".
    pub done: String,
}

/// The sheet with these passwords: never cached anywhere (as Manage Users'
/// temporary password), and not kept: a reload has nothing to show.
fn sheet(state: &AppState, headers: &HeaderMap, user: CurrentUser, slips: Vec<Slip>, done: String) -> Response {
    let address = crate::routes::public_base(state, headers);
    ([(header::CACHE_CONTROL, "no-store")], Page(SheetTemplate { user: Some(user), slips, address, done })).into_response()
}

/// POST /students/sheet (name, username, password per row): make every
/// account or none. An empty password is made (`students::make_password`).
/// Refused (a bad or taken username, a short password, the box full): the
/// rows again with why, and nothing made.
pub async fn make(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    teacher(&user)?;
    let (names, usernames, passwords) = (all(&form, "name"), all(&form, "username"), all(&form, "password"));
    let mut rows: Vec<AddRow> = names
        .iter()
        .zip(&usernames)
        .zip(passwords.iter().chain(std::iter::repeat(&"")))
        .map(|((n, u), p)| AddRow { name: n.trim().to_string(), username: u.trim().to_string(), password: p.to_string() })
        .take(*crate::caps::STUDENT_LIMIT_RANGE.end())
        .collect();
    if rows.is_empty() {
        return Ok(Redirect::to(&crate::url("/students")).into_response());
    }
    let (room, _) = room(&state).await?;
    let again = |rows: Vec<AddRow>, user: CurrentUser, why: String| Page(AddTemplate { user: Some(user), rows, room, error: Some(why) }).into_response();
    // The cheap checks first, so a refused batch costs no hashing.
    for r in &rows {
        if !r.password.is_empty() {
            if let Err(why) = auth::validate_password(&r.password) {
                let why = format!("{}: {why}", r.username);
                return Ok(again(rows, user, why));
            }
        }
        let (first, last) = split_name(&r.name);
        for (label, v) in [(crate::t("profile.personal.first_name"), first), (crate::t("profile.personal.last_name"), last)] {
            if let Err(why) = crate::profile::validate_name(&label, &v) {
                return Ok(again(rows, user, why));
            }
        }
    }
    let (teacher_id, dev) = (user.id, state.dev_mode());
    let usernames: Vec<String> = rows.iter().map(|r| r.username.clone()).collect();
    let checked = state.db.call(move |c| students::check(c, teacher_id, &usernames.iter().map(String::as_str).collect::<Vec<_>>(), dev)).await?;
    let stored = match checked {
        Ok(stored) => stored,
        Err(why) => return Ok(again(rows, user, refusal(&why))),
    };
    for r in &mut rows {
        if r.password.is_empty() {
            r.password = students::make_password();
        }
    }
    let mut batch = Vec::with_capacity(rows.len());
    for r in &rows {
        let (first_name, last_name) = split_name(&r.name);
        let password_hash = auth::hash_password_async(r.password.clone()).await?;
        batch.push(NewStudent { username: r.username.clone(), password_hash, first_name, last_name });
    }
    let made = state.db.call(move |c| students::create(c, teacher_id, &batch, dev)).await?;
    if let Err(why) = made {
        // Taken between the check and now: the typed passwords stay, the made ones go.
        return Ok(again(rows, user, refusal(&why)));
    }
    tracing::info!("user {teacher_id} made {} student accounts", rows.len());
    let slips: Vec<Slip> = rows
        .into_iter()
        .zip(stored)
        .map(|(r, username)| Slip { name: if r.name.is_empty() { username.clone() } else { r.name }, username, password: r.password })
        .collect();
    let done = crate::tn("students_page.added", slips.len() as i64);
    Ok(sheet(&state, &headers, user, slips, done))
}

/// GET /students/sheet: the sheet's address opened again (a reload, a
/// bookmark). The passwords were shown once and are not kept.
pub async fn sheet_again(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap) -> Result<Response, AppError> {
    teacher(&user)?;
    Ok(sheet(&state, &headers, user, Vec::new(), String::new()))
}

// ---------- the ticked students: new passwords, delete ----------

#[derive(Template)]
#[template(path = "students_confirm.html")]
pub struct ConfirmTemplate {
    pub user: Option<CurrentUser>,
    /// Where the confirm button posts, and the question and button's words.
    pub action: String,
    pub question: String,
    pub button: String,
    /// Delete: a red button.
    pub danger: bool,
    /// The students it is about: (id, name, username).
    pub students: Vec<(i64, String, String)>,
    /// Where Cancel goes.
    pub cancel: String,
}

/// The teacher's students among `ids`, in the list's order.
async fn theirs(state: &AppState, teacher_id: i64, ids: Vec<i64>) -> Result<Vec<Student>, AppError> {
    state.db.call(move |c| Ok(students::list(c, teacher_id)?.into_iter().filter(|s| ids.contains(&s.id)).collect())).await
}

/// New passwords for these students, made and set; the sheet that shows them.
async fn new_passwords(state: &AppState, headers: &HeaderMap, user: CurrentUser, chosen: Vec<(Student, String)>) -> Result<Response, AppError> {
    let teacher_id = user.id;
    let mut set = Vec::with_capacity(chosen.len());
    for (s, password) in &chosen {
        set.push((s.id, auth::hash_password_async(password.clone()).await?));
    }
    let n = state
        .db
        .call(move |c| {
            let mut n = 0;
            for (id, hash) in &set {
                n += usize::from(students::set_password(c, teacher_id, *id, hash)?);
            }
            Ok(n)
        })
        .await?;
    tracing::info!("user {teacher_id} gave {n} students new passwords");
    let slips = chosen.into_iter().map(|(s, password)| Slip { name: shown_name(&s), username: s.username, password }).collect();
    Ok(sheet(state, headers, user, slips, crate::tn("students_page.new_passwords_done", n as i64)))
}

/// POST /students/passwords (id=… per ticked student): the confirm page;
/// with `confirm=1`, a made password for each and the sheet.
pub async fn passwords(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    teacher(&user)?;
    let picked = theirs(&state, user.id, picked(&form)).await?;
    if picked.is_empty() {
        return Ok(back("none-picked", 0));
    }
    if one(&form, "confirm") != "1" {
        let t = ConfirmTemplate {
            user: Some(user),
            action: crate::url("/students/passwords"),
            question: crate::tn("students_page.new_passwords_question", picked.len() as i64),
            button: crate::t("students_page.new_passwords"),
            danger: false,
            students: picked.into_iter().map(|s| (s.id, shown_name(&s), s.username)).collect(),
            cancel: crate::url("/students"),
        };
        return Ok(Page(t).into_response());
    }
    let chosen = picked.into_iter().map(|s| (s, students::make_password())).collect();
    new_passwords(&state, &headers, user, chosen).await
}

/// POST /students/delete (id=… per ticked student): the confirm page; with
/// `confirm=1`, the students go, with everything in them.
pub async fn delete(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    teacher(&user)?;
    let picked = theirs(&state, user.id, picked(&form)).await?;
    if picked.is_empty() {
        return Ok(back("none-picked", 0));
    }
    if one(&form, "confirm") != "1" {
        let t = ConfirmTemplate {
            user: Some(user),
            action: crate::url("/students/delete"),
            question: crate::tn("students_page.delete_question", picked.len() as i64),
            button: crate::t("students_page.delete_picked"),
            danger: true,
            students: picked.into_iter().map(|s| (s.id, shown_name(&s), s.username)).collect(),
            cancel: crate::url("/students"),
        };
        return Ok(Page(t).into_response());
    }
    let (teacher_id, ids): (i64, Vec<i64>) = (user.id, picked.iter().map(|s| s.id).collect());
    let n = state.db.call(move |c| students::delete(c, teacher_id, &ids)).await?;
    tracing::info!("user {teacher_id} deleted {n} students");
    Ok(back("deleted", n))
}

/// POST /students/give (id=… per ticked student, course): the course goes
/// to every ticked student that does not have it yet. Only a course the
/// teacher made, that loads, can be given; anything else is a 404.
pub async fn give(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    teacher(&user)?;
    let course = one(&form, "course").to_string();
    let picked = theirs(&state, user.id, picked(&form)).await?;
    if picked.is_empty() {
        return Ok(back("none-picked", 0));
    }
    if course.is_empty() {
        return Ok(back("no-course", 0));
    }
    if !givable(&state, user.id).await?.iter().any(|(id, _)| *id == course) {
        return Err(AppError::NotFound);
    }
    let (teacher_id, ids, cid) = (user.id, picked.iter().map(|s| s.id).collect::<Vec<_>>(), course.clone());
    let n = state
        .db
        .call(move |c| {
            let mut n = 0;
            for id in ids {
                n += usize::from(students::give(c, teacher_id, id, &cid)?);
            }
            Ok(n)
        })
        .await?;
    tracing::info!("user {teacher_id} gave course {course} to {n} students");
    Ok(Redirect::to(&crate::url(&format!("/students?did=given&n={n}&course={}", crate::urlenc::urlencode(&course)))).into_response())
}

// ---------- one student ----------

#[derive(Template)]
#[template(path = "students_one.html")]
pub struct OneTemplate {
    pub user: Option<CurrentUser>,
    pub student: Student,
    pub name: String,
    pub passed: i64,
    pub last_studied: String,
    pub notice: Option<String>,
    pub error: Option<String>,
    /// The courses on its page (#220): every one the teacher can give, and
    /// any given that no longer can: `(id, title, given)`, by title.
    pub courses: Vec<(String, String, bool)>,
}

/// A student's courses for its page: the teacher's givable ones, and the
/// given ones too, ticked.
async fn courses_of(state: &AppState, teacher_id: i64, student_id: i64) -> Result<Vec<(String, String, bool)>, AppError> {
    let mut courses: Vec<(String, String, bool)> = givable(state, teacher_id).await?.into_iter().map(|(id, title)| (id, title, false)).collect();
    for g in state.db.call(move |c| students::given(c, teacher_id)).await?.into_iter().filter(|g| g.student_id == student_id) {
        match courses.iter_mut().find(|c| c.0 == g.course_id) {
            Some(c) => c.2 = true,
            None => courses.push((g.course_id, g.title, true)),
        }
    }
    courses.sort_by_key(|c| c.1.to_lowercase());
    Ok(courses)
}

async fn load_one(state: &AppState, user: CurrentUser, id: i64) -> Result<OneTemplate, AppError> {
    let (teacher_id, tz, now) = (user.id, user.tz, crate::db::now());
    let found = state.db.call(move |c| Ok(students::find(c, teacher_id, id)?.map(|s| (s, students::progress(c, teacher_id))))).await?;
    let Some((student, progress)) = found else {
        return Err(AppError::NotFound);
    };
    let p = progress?.get(&id).copied().unwrap_or_default();
    let courses = courses_of(state, teacher_id, id).await?;
    Ok(OneTemplate { user: Some(user), name: shown_name(&student), student, passed: p.passed, last_studied: last_studied(p.last_studied, tz, now).0, notice: None, error: None, courses })
}

#[derive(Deserialize)]
pub struct OneQuery {
    pub did: Option<String>,
}

/// GET /students/{id}: another teacher's student, or no student, is a 404.
pub async fn show(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<i64>, Query(q): Query<OneQuery>) -> Result<Response, AppError> {
    teacher(&user)?;
    let mut t = load_one(&state, user, id).await?;
    match q.did.as_deref() {
        Some("saved") => t.notice = Some(crate::t("students_page.saved")),
        Some("courses") => t.notice = Some(crate::t("students_page.courses_saved")),
        _ => {}
    }
    Ok(Page(t).into_response())
}

#[derive(Deserialize)]
pub struct DetailsForm {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: String,
}

/// POST /students/{id} (username, first_name, last_name): saved, back to
/// the student's page; refused, the page again with why and nothing saved.
pub async fn save(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<i64>, Form(form): Form<DetailsForm>) -> Result<Response, AppError> {
    teacher(&user)?;
    let names = crate::profile::validate_name(&crate::t("profile.personal.first_name"), &form.first_name)
        .and_then(|f| crate::profile::validate_name(&crate::t("profile.personal.last_name"), &form.last_name).map(|l| (f, l)));
    let teacher_id = user.id;
    let outcome = match names {
        Err(why) => Err(why),
        Ok((first, last)) => {
            let username = form.username.clone();
            state
                .db
                .call(move |c| {
                    if students::find(c, teacher_id, id)?.is_none() {
                        return Ok(None);
                    }
                    c.execute_batch("SAVEPOINT students_save")?;
                    let r = students::rename(c, teacher_id, id, &username)?;
                    if r.is_ok() {
                        students::set_names(c, teacher_id, id, &first, &last)?;
                        c.execute_batch("RELEASE students_save")?;
                    } else {
                        c.execute_batch("ROLLBACK TO students_save; RELEASE students_save")?;
                    }
                    Ok(Some(r))
                })
                .await?
                .ok_or(AppError::NotFound)?
                .map_err(|r| refusal(&r))
                .map(|_| ())
        }
    };
    match outcome {
        Ok(()) => Ok(Redirect::to(&crate::url(&format!("/students/{id}?did=saved"))).into_response()),
        Err(why) => {
            let mut t = load_one(&state, user, id).await?;
            t.error = Some(why);
            t.student.username = form.username;
            t.student.first_name = form.first_name;
            t.student.last_name = form.last_name;
            Ok(Page(t).into_response())
        }
    }
}

/// POST /students/{id}/courses (course=… per ticked course): the student
/// has the ticked courses from now on. A course ticked that it did not have
/// is given (if the teacher can give it); one it had that is not ticked is
/// taken away, its progress kept.
pub async fn save_courses(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<i64>, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    teacher(&user)?;
    let teacher_id = user.id;
    if state.db.call(move |c| students::find(c, teacher_id, id)).await?.is_none() {
        return Err(AppError::NotFound);
    }
    let ticked: Vec<String> = all(&form, "course").into_iter().map(str::to_string).collect();
    let courses = courses_of(&state, teacher_id, id).await?;
    let (gave, took) = state
        .db
        .call(move |c| {
            let (mut gave, mut took) = (0, 0);
            for (cid, _, has) in &courses {
                let tick = ticked.contains(cid);
                if tick && !has {
                    gave += usize::from(students::give(c, teacher_id, id, cid)?);
                } else if !tick && *has {
                    took += usize::from(students::take_away(c, teacher_id, id, cid)?);
                }
            }
            Ok((gave, took))
        })
        .await?;
    tracing::info!("user {teacher_id} gave student {id} {gave} courses and took {took} away");
    Ok(Redirect::to(&crate::url(&format!("/students/{id}?did=courses"))).into_response())
}

#[derive(Deserialize)]
pub struct PasswordForm {
    #[serde(default)]
    pub password: String,
}

/// POST /students/{id}/password (password, empty for a made one): the
/// student's sessions end, and the sheet shows the password once.
pub async fn set_password(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Path(id): Path<i64>, Form(form): Form<PasswordForm>) -> Result<Response, AppError> {
    teacher(&user)?;
    let teacher_id = user.id;
    let Some(student) = state.db.call(move |c| students::find(c, teacher_id, id)).await? else {
        return Err(AppError::NotFound);
    };
    let password = if form.password.is_empty() { students::make_password() } else { form.password };
    if let Err(why) = auth::validate_password(&password) {
        let mut t = load_one(&state, user, id).await?;
        t.error = Some(why);
        return Ok(Page(t).into_response());
    }
    new_passwords(&state, &headers, user, vec![(student, password)]).await
}

/// POST /students/{id}/delete: the confirm page, whose button posts to
/// `/students/delete` with this student's id and `confirm=1`.
pub async fn delete_one(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<i64>) -> Result<Response, AppError> {
    teacher(&user)?;
    let picked = theirs(&state, user.id, vec![id]).await?;
    if picked.is_empty() {
        return Err(AppError::NotFound);
    }
    let t = ConfirmTemplate {
        user: Some(user),
        action: crate::url("/students/delete"),
        question: crate::tn("students_page.delete_question", 1),
        button: crate::t("students_page.delete"),
        danger: true,
        students: picked.into_iter().map(|s| (s.id, shown_name(&s), s.username)).collect(),
        cancel: crate::url(&format!("/students/{id}")),
    };
    Ok(Page(t).into_response())
}

#[cfg(test)]
mod tests;
