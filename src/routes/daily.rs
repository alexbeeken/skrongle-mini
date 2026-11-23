//! The **Plan** tab (#222, `docs/features.md` "Daily work"): a teacher plans
//! the class's day in one of its courses at `/courses/{id}/daily`. The data
//! and every rule are `crate::daily`'s; this module reads them and renders.
//!
//! - `GET /courses/{id}/daily?day=YYYY-MM-DD`: the week row (Monday to
//!   Sunday, ‹ and ›), the open day (today on the class clock by default)
//!   with its activities and the buttons, and the class time zone. On the
//!   first visit, a browser that sent its offset (the `tz` cookie) sets the
//!   class clock; without one the page asks the teacher to pick it.
//!   `&change=<activity>` opens that activity's lesson picker in place.
//! - `POST /courses/{id}/daily` (`do`, `day`, and what the action needs):
//!   one change, then back to the page (303). A change the rules refuse
//!   (`Ok(false)`) comes back with one error line (`&error=`).
//!
//! Every control is a plain form or link, so the page works with JavaScript
//! off and on a phone. Anyone but the course's owner, and an owner who may
//! not hold students, gets a 404.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::courses::{owned, CourseTabs};
use super::Page;
use crate::auth::CurrentUser;
use crate::content::ContentStore;
use crate::daily::{self, Activity, DayState, Kind, WeekDay};
use crate::error::AppError;
use crate::AppState;

// ---------- dates ----------

/// `YYYY-MM-DD` of a day number: the page's `?day=` and its forms' `day`.
pub fn iso(day: i64) -> String {
    let (y, m, d, _) = daily::civil(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A `YYYY-MM-DD` back to its day number; `None` for anything else.
pub fn parse_iso(s: &str) -> Option<i64> {
    let mut parts = s.trim().splitn(3, '-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    daily::from_civil(y, m, d)
}

const SHORT: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const LONG: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

/// "Mon 16", in the teacher's language.
pub fn short_day(day: i64) -> String {
    let (_, _, d, wd) = daily::civil(day);
    crate::t2("daily_plan.day_short", "weekday", crate::t(&format!("daily_plan.short.{}", SHORT.get(wd as usize).unwrap_or(&"mon"))), "day", d)
}

/// "Thursday 19 November", in the teacher's language.
pub fn long_day(day: i64) -> String {
    let (_, m, d, wd) = daily::civil(day);
    let weekday = crate::t(&format!("daily_plan.long.{}", LONG.get(wd as usize).unwrap_or(&"monday")));
    let month = crate::t(&format!("profile.month.{}", MONTHS.get((m - 1) as usize).unwrap_or(&"january")));
    crate::i18n::fill(&crate::t("daily_plan.day_long"), &[("weekday", &weekday), ("day", &d), ("month", &month)])
}

/// "UTC+05:30" for a class clock in half hours east of UTC.
pub fn zone_name(half_hours: i64) -> String {
    let mins = half_hours * 30;
    let sign = if mins < 0 { '-' } else { '+' };
    format!("UTC{sign}{:02}:{:02}", mins.abs() / 60, mins.abs() % 60)
}

// ---------- the week row: shared with the progress page (#225) ----------

/// One day of the week row (`daily_week.html`).
pub struct DayCell {
    /// `YYYY-MM-DD`, for its link.
    pub date: String,
    /// "Mon 16".
    pub label: String,
    /// "Nothing planned", "Not assigned yet", "3 activities", "32 of 37 finished".
    pub state: String,
    /// `nothing` | `draft` | `assigned` | `past`, for tests and styling.
    pub kind: &'static str,
    pub today: bool,
    /// The day the page has open.
    pub open: bool,
    /// More than `daily::PLAN_AHEAD_DAYS` ahead: greyed out.
    pub ahead: bool,
}

/// The week row of `daily::week`, with `open` the day the page shows.
pub struct WeekRow {
    pub days: Vec<DayCell>,
    /// ‹ and ›: the same weekday a week earlier or later, when the page
    /// goes there (`YYYY-MM-DD`).
    pub prev: Option<String>,
    pub next: Option<String>,
    /// The address each day links to, `?day=` added: the Plan page's, or
    /// another page's that draws the same row.
    pub href: String,
}

/// The earliest and the latest day the page opens: as far back as plans
/// are kept, and the week after the last plannable day.
fn bounds(today: i64) -> (i64, i64) {
    (today - crate::caps::DAILY_KEEP_DAYS, today + daily::PLAN_AHEAD_DAYS + 6)
}

/// The row of `week` (from `daily::week`) with `open` open, for a page at
/// `href` (`/courses/{id}/daily`, or #225's progress page).
pub fn week_row(week: &[WeekDay], today: i64, open: i64, href: String) -> WeekRow {
    let (first, last) = bounds(today);
    let days = week
        .iter()
        .map(|w| {
            let (state, kind) = match w.state {
                DayState::Nothing | DayState::Draft(0) => (crate::t("daily_plan.nothing"), "nothing"),
                DayState::Draft(_) => (crate::t("daily_plan.draft"), "draft"),
                DayState::Assigned(n) => (crate::tn("daily_plan.activities", n), "assigned"),
                DayState::Past { finished, students } => (crate::t2("daily_plan.finished", "finished", finished, "students", students), "past"),
            };
            let mut label = short_day(w.day);
            if w.day == today {
                label = crate::t1("daily_plan.today", "day", label);
            }
            DayCell { date: iso(w.day), label, state, kind, today: w.day == today, open: w.day == open, ahead: w.day > today && !w.plannable }
        })
        .collect();
    let prev = (open - 7 >= first).then(|| iso(open - 7));
    let next = (daily::week_start(open) + 7 <= today + daily::PLAN_AHEAD_DAYS && open + 7 <= last).then(|| iso(open + 7));
    WeekRow { days, prev, next, href }
}

// ---------- the page ----------

/// One game of a play: its name in `daily::GAMES`, its label, on or off.
pub struct GameView {
    pub name: &'static str,
    pub label: String,
    pub on: bool,
}

/// A lesson in an activity's picker.
pub struct LessonPick {
    pub id: String,
    pub title: String,
    pub picked: bool,
}

/// One activity row.
pub struct ActivityRow {
    pub id: i64,
    /// 1, 2, 3 …
    pub number: usize,
    /// `study` | `review` | `play`.
    pub kind: &'static str,
    pub kind_label: String,
    /// The lessons' titles, joined.
    pub lessons: String,
    pub first: bool,
    pub last: bool,
    /// "Change lesson" for a study, "Change lessons" otherwise.
    pub change_label: String,
    /// A play: the games its lessons allow and its times through.
    pub games: Vec<GameView>,
    pub rounds: i64,
    /// A play's games and times through, in words.
    pub play_summary: String,
    /// The picker is open (`?change=`): every lesson of the course.
    pub picker: Option<Vec<LessonPick>>,
}

#[derive(Template)]
#[template(path = "daily.html")]
pub struct DailyTemplate {
    pub user: Option<CurrentUser>,
    pub tabs: CourseTabs,
    /// The page's address, where every form posts.
    pub action: String,
    /// `YYYY-MM-DD` of the open day.
    pub date: String,
    /// "Thursday 19 November".
    pub day_title: String,
    pub week: WeekRow,
    pub rows: Vec<ActivityRow>,
    /// The day may be changed (today up to 28 days ahead) and the course loads.
    pub editable: bool,
    /// Before today: no buttons.
    pub past: bool,
    /// More than 28 days ahead.
    pub ahead: bool,
    /// The course does not load: nothing can be planned.
    pub broken: bool,
    /// The day has a plan row (Clear the day shows).
    pub has_plan: bool,
    pub assigned: bool,
    /// Students given the course.
    pub students: usize,
    pub notice: Option<String>,
    pub error: Option<String>,
    /// The class clock is set; its name, and every choice.
    pub tz_set: bool,
    pub tz: i64,
    pub tz_name: String,
    pub tz_options: Vec<(i64, String)>,
}

#[derive(Deserialize, Default)]
pub struct DailyQuery {
    pub day: Option<String>,
    pub change: Option<i64>,
    pub did: Option<String>,
    pub error: Option<String>,
    /// `did=week`: the days filled, `YYYY-MM-DD`, comma-separated.
    pub days: Option<String>,
    /// `did=week`: it stopped for want of a lesson.
    pub stopped: Option<String>,
}

/// The course's row and its tabs, or a 404: not the owner, or may not hold
/// students.
async fn course(state: &AppState, user: &CurrentUser, id: &str) -> Result<(CourseTabs, Option<Arc<ContentStore>>), AppError> {
    let row = owned(state, user, id).await?;
    let tabs = CourseTabs::of(user, &row, "plan").ok_or(AppError::NotFound)?;
    let content = if row.error.is_none() { state.courses.get(&state.db, &row.id).await? } else { None };
    Ok((tabs, content))
}

fn lesson_title(course: Option<&ContentStore>, id: &str) -> String {
    course.and_then(|c| c.lesson_by_id(id)).map_or_else(|| id.to_string(), |l| l.title.clone())
}

fn game_label(name: &str) -> String {
    crate::t(if name == "matching" { "daily_plan.game_matching" } else { "daily_plan.game_flashcards" })
}

fn activity_row(a: &Activity, n: usize, count: usize, course: Option<&ContentStore>, change: Option<i64>) -> ActivityRow {
    let titles: Vec<String> = a.lessons.iter().map(|id| lesson_title(course, id)).collect();
    let (games, play_summary) = if a.kind == Kind::Play {
        let allowed = course.map(|c| daily::games_for(c, &a.lessons)).unwrap_or_default();
        let games: Vec<GameView> =
            allowed.iter().map(|g| GameView { name: g, label: game_label(g), on: a.games.iter().any(|x| x == g) }).collect();
        let mut parts: Vec<String> = a.games.iter().map(|g| game_label(g)).collect();
        if parts.is_empty() {
            parts.push(crate::t("daily_plan.no_game"));
        }
        parts.push(if a.rounds == 0 { crate::t("daily_plan.no_limit") } else { crate::tn("daily_plan.times", a.rounds) });
        (games, parts.join(" · "))
    } else {
        (Vec::new(), String::new())
    };
    let picker = (change == Some(a.id)).then(|| {
        course
            .map(|c| c.lessons().iter().map(|l| LessonPick { id: l.id.clone(), title: l.title.clone(), picked: a.lessons.contains(&l.id) }).collect())
            .unwrap_or_default()
    });
    ActivityRow {
        id: a.id,
        number: n + 1,
        kind: a.kind.as_str(),
        kind_label: crate::t(&format!("daily_plan.kind_{}", a.kind.as_str())),
        lessons: titles.join(", "),
        first: n == 0,
        last: n + 1 == count,
        change_label: crate::t(if a.kind == Kind::Study { "daily_plan.change_lesson" } else { "daily_plan.change_lessons" }),
        games,
        rounds: a.rounds,
        play_summary,
        picker,
    }
}

/// What a `?did=` says, in words.
fn notice_of(q: &DailyQuery, tz: i64) -> Option<String> {
    match q.did.as_deref()? {
        "tz" => Some(crate::t1("daily_plan.tz_saved", "zone", zone_name(tz))),
        "tz-browser" => Some(crate::t1("daily_plan.tz_from_browser", "zone", zone_name(tz))),
        "week" => {
            let days: Vec<String> = q.days.as_deref().unwrap_or("").split(',').filter_map(parse_iso).map(short_day).collect();
            let mut text = if days.is_empty() { crate::t("daily_plan.week_none") } else { crate::t1("daily_plan.week_done", "days", days.join(", ")) };
            if q.stopped.is_some() {
                text = format!("{text} {}", crate::t("daily_plan.week_stopped"));
            }
            Some(text)
        }
        _ => None,
    }
}

/// What a `?error=` says, in words: only the keys this module sends.
fn error_of(key: &str) -> Option<String> {
    Some(match key {
        "closed" => crate::t("daily_plan.refused_closed"),
        "lessons" => crate::t("daily_plan.refused_lessons"),
        "full" => crate::tn("daily_plan.refused_full", daily::MAX_ACTIVITIES),
        "empty" => crate::t("daily_plan.refused_empty"),
        "tz" => crate::t("daily_plan.refused_tz"),
        "refused" => crate::t("daily_plan.refused"),
        _ => return None,
    })
}

/// GET /courses/{id}/daily
pub async fn page(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(q): Query<DailyQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (tabs, content) = course(&state, &user, &id).await?;
    let browser = super::sprint::tz_cookie_secs(&headers).and_then(daily::half_hours_of);
    let (cid, now, wanted) = (id.clone(), crate::db::now(), q.day.as_deref().and_then(parse_iso));
    let (today, day, plan, week, students, tz, set_now) = state
        .db
        .call(move |c| {
            // The first visit sets the class clock from a browser that said where it is.
            let mut set_now = false;
            if !crate::settings::class_tz_set(c)? {
                if let Some(h) = browser {
                    set_now = crate::settings::set_class_tz(c, h)?;
                }
            }
            let today = daily::today(c, now)?;
            let (first, last) = bounds(today);
            let day = wanted.unwrap_or(today).clamp(first, last);
            let plan = daily::plan(c, &cid, day)?;
            let week = daily::week(c, &cid, daily::week_start(day), now)?;
            let students = daily::students_of(c, &cid)?.len();
            Ok((today, day, plan, week, students, crate::settings::class_tz(c)?, set_now))
        })
        .await?;
    let course = content.as_deref();
    let count = plan.as_ref().map_or(0, |p| p.activities.len());
    let rows = plan.as_ref().map(|p| p.activities.iter().enumerate().map(|(n, a)| activity_row(a, n, count, course, q.change)).collect()).unwrap_or_default();
    let tz_now = tz.unwrap_or(0);
    let mut notice = notice_of(&q, tz_now);
    if set_now {
        notice = Some(crate::t1("daily_plan.tz_from_browser", "zone", zone_name(tz_now)));
    }
    let href = format!("{}/courses/{}/daily", crate::base_path(), crate::urlenc::urlencode(&id));
    Ok(Page(DailyTemplate {
        user: Some(user),
        tabs,
        date: iso(day),
        day_title: long_day(day),
        week: week_row(&week, today, day, href.clone()),
        action: href,
        rows,
        editable: daily::plannable(day, today) && course.is_some(),
        past: day < today,
        ahead: day > today && !daily::plannable(day, today),
        broken: course.is_none(),
        has_plan: plan.is_some(),
        assigned: plan.as_ref().is_some_and(|p| p.assigned),
        students,
        notice,
        error: q.error.as_deref().and_then(error_of),
        tz_set: tz.is_some(),
        tz: tz.or(browser).unwrap_or(0),
        tz_name: zone_name(tz_now),
        tz_options: crate::settings::CLASS_TZ_RANGE.map(|h| (h, zone_name(h))).collect(),
    })
    .into_response())
}

fn one<'a>(form: &'a [(String, String)], key: &str) -> &'a str {
    form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or_default()
}

/// Back to the page on `day`, with `extra` (`&did=…` or `&error=…`).
fn back(id: &str, day: i64, extra: &str) -> Response {
    Redirect::to(&crate::url(&format!("/courses/{}/daily?day={}{extra}", crate::urlenc::urlencode(id), iso(day)))).into_response()
}

/// What one posted change did.
enum Outcome {
    Done,
    /// Refused: the `?error=` key.
    Refused(&'static str),
    /// Plan my week: the days filled, and whether it stopped for want of a lesson.
    Week(Vec<i64>, bool),
}

/// POST /courses/{id}/daily (`do` = add-study | add-review | add-play | up | down | remove | lessons |
/// play | recommended | assign | clear | week | tz): one change, then back.
pub async fn change(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    let (_, content) = course(&state, &user, &id).await?;
    let action = one(&form, "do").to_string();
    let now = crate::db::now();
    let day = parse_iso(one(&form, "day"));
    if action == "tz" {
        let tz: Option<i64> = one(&form, "tz").trim().parse().ok();
        let (ok, today) = state
            .db
            .call(move |c| {
                let ok = match tz {
                    Some(h) => crate::settings::set_class_tz(c, h)?,
                    None => false,
                };
                Ok((ok, daily::today(c, now)?))
            })
            .await?;
        return Ok(back(&id, day.unwrap_or(today), if ok { "&did=tz" } else { "&error=tz" }));
    }
    let Some(course) = content else {
        let today = state.db.call(move |c| daily::today(c, now)).await?;
        return Ok(back(&id, day.unwrap_or(today), "&error=refused"));
    };
    let activity: Option<i64> = one(&form, "activity").trim().parse().ok();
    let lessons: Vec<String> = form.iter().filter(|(k, _)| k == "lesson").map(|(_, v)| v.clone()).collect();
    let games: Vec<String> = form.iter().filter(|(k, _)| k == "game").map(|(_, v)| v.clone()).collect();
    let rounds: Option<i64> = one(&form, "rounds").trim().parse().ok();
    let kind = action.strip_prefix("add-").and_then(Kind::parse);
    let cid = id.clone();
    let (day, outcome) = state
        .db
        .call(move |c| {
            let today = daily::today(c, now)?;
            let day = day.unwrap_or(today);
            if action != "week" && !daily::plannable(day, today) {
                return Ok((day, Outcome::Refused("closed")));
            }
            let c: &rusqlite::Connection = c;
            let course = &*course;
            let position = |id: i64| -> rusqlite::Result<Option<usize>> {
                Ok(daily::plan(c, &cid, day)?.and_then(|p| p.activities.iter().position(|a| a.id == id)))
            };
            let ok = match (action.as_str(), activity) {
                (a, _) if a.starts_with("add-") => {
                    let Some(kind) = kind else { return Ok((day, Outcome::Refused("refused"))) };
                    if daily::add(c, course, &cid, day, kind, None, now)? {
                        true
                    } else if daily::plan(c, &cid, day)?.is_some_and(|p| p.activities.len() >= daily::MAX_ACTIVITIES) {
                        return Ok((day, Outcome::Refused("full")));
                    } else {
                        false
                    }
                }
                ("up", Some(a)) => match position(a)? {
                    Some(pos) if pos > 0 => daily::move_to(c, &cid, day, a, pos - 1, now)?,
                    _ => false,
                },
                ("down", Some(a)) => match (position(a)?, daily::plan(c, &cid, day)?.map_or(0, |p| p.activities.len())) {
                    (Some(pos), n) if pos + 1 < n => daily::move_to(c, &cid, day, a, pos + 1, now)?,
                    _ => false,
                },
                ("remove", Some(a)) => daily::remove(c, &cid, day, a, now)?,
                ("lessons", Some(a)) => {
                    if !daily::set_lessons(c, course, &cid, day, a, &lessons, now)? {
                        return Ok((day, Outcome::Refused("lessons")));
                    }
                    true
                }
                ("play", Some(a)) => {
                    // The games ticked and the times through, all or nothing.
                    let tx = c.unchecked_transaction()?;
                    let lessons = daily::plan(&tx, &cid, day)?.and_then(|p| p.activities.into_iter().find(|x| x.id == a)).map(|x| x.lessons).unwrap_or_default();
                    let mut ok = true;
                    for g in daily::games_for(course, &lessons) {
                        ok &= daily::set_game(&tx, course, &cid, day, a, g, games.iter().any(|x| x == g), now)?;
                    }
                    ok &= daily::set_rounds(&tx, &cid, day, a, rounds.unwrap_or(-1), now)?;
                    if ok {
                        tx.commit()?;
                    }
                    ok
                }
                ("recommended", _) => daily::recommended(c, course, &cid, day, now)?,
                ("assign", _) => {
                    if !daily::assign(c, &cid, day, now)? {
                        return Ok((day, Outcome::Refused("empty")));
                    }
                    true
                }
                ("clear", _) => daily::clear(c, &cid, day, now)?,
                ("week", _) => {
                    let (days, stopped) = daily::auto_week(c, course, &cid, now)?;
                    return Ok((day, Outcome::Week(days, stopped)));
                }
                _ => false,
            };
            Ok((day, if ok { Outcome::Done } else { Outcome::Refused("refused") }))
        })
        .await?;
    Ok(match outcome {
        Outcome::Done => back(&id, day, ""),
        Outcome::Refused(key) => back(&id, day, &format!("&error={key}")),
        Outcome::Week(days, stopped) => {
            let list: Vec<String> = days.iter().map(|d| iso(*d)).collect();
            back(&id, day, &format!("&did=week&days={}{}", list.join(","), if stopped { "&stopped=1" } else { "" }))
        }
    })
}

#[cfg(test)]
mod tests;
