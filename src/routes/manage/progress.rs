//! `GET /manage/progress?email=<address>` (#147): how one account is getting
//! on, for whoever manages the accounts (an owner or a superuser,
//! `CurrentUser::manages_users`). One row per course the account has opened:
//! lessons passed (`lesson_passes`, the rule every mode shares,
//! `docs/progress.md`), when it last studied, its day streak. Counts and
//! dates only: no answers, no notes, no history of single questions.
//!
//! The page does not exist (a 404) for an owner's account or one with the
//! Privacy permission, as for an address nobody has and for a viewer who
//! manages nothing. A superuser may open their own.
//!
//! No course is built to draw it: a built-in course's title and lesson total
//! come from its summary (`Courses::builtin_by_id`), a user-made one's title
//! from its `courses` row, and its total is left out.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::perms::{Perm, Perms};
use crate::routes::Page;
use crate::AppState;

/// Is an account's progress (and, from a superuser, its last sign-in) kept
/// from the viewer? An owner's always is; an account with Privacy's is,
/// except from itself.
pub fn hidden(owner: bool, perms: Perms, own: bool) -> bool {
    owner || (perms.has(Perm::Privacy) && !own)
}

/// The progress page of the account with this address.
pub fn url_of(email: &str) -> String {
    crate::url(&format!("/manage/progress?email={}", crate::urlenc::urlencode(email)))
}

#[derive(Template)]
#[template(path = "manage_progress.html")]
pub struct ProgressTemplate {
    pub user: Option<CurrentUser>,
    /// The account looked at: its address and its name ("Learner" without one).
    pub email: String,
    pub name: String,
    /// The courses it has opened, most recently opened first.
    pub courses: Vec<CourseLine>,
}

pub struct CourseLine {
    pub id: String,
    /// The course's title; its id when the course no longer exists.
    pub title: String,
    /// "12 of 48", or "12" when the total is not known without building the course.
    pub passed: String,
    /// A date, or "never".
    pub last_studied: String,
    /// "3 days".
    pub streak: String,
}

/// What the database has of one course the account opened.
struct Opened {
    course_id: String,
    /// `courses.title`, for a user-made course that still exists.
    title: Option<String>,
    passed: usize,
    last_studied: Option<i64>,
    streak: i64,
}

#[derive(Deserialize)]
pub struct ProgressQuery {
    #[serde(default)]
    pub email: String,
}

/// GET /manage/progress?email=…
pub async fn show(State(state): State<Arc<AppState>>, user: CurrentUser, Query(q): Query<ProgressQuery>) -> Result<Response, AppError> {
    if !user.manages_users() {
        return Err(AppError::NotFound);
    }
    let (viewer, tz, now) = (user.id, user.tz, crate::db::now());
    let found = state
        .db
        .call(move |c| {
            let Some(id) = crate::owner::find(c, &q.email)? else {
                return Ok(None);
            };
            let (email, first, last, owner, bits): (String, String, String, bool, i64) = c.query_row(
                "SELECT COALESCE(email, ''), COALESCE(first_name, ''), COALESCE(last_name, ''), owner, perms FROM users WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
            if hidden(owner, Perms::from_bits(bits), id == viewer) {
                return Ok(None);
            }
            let passed = crate::progress::passed_by_course(c, id)?;
            let mut opened = Vec::new();
            for pr in crate::courses::projects_of(c, id)? {
                opened.push(Opened {
                    title: c.query_row("SELECT title FROM courses WHERE id = ?1", [&pr.course_id], |r| r.get(0)).optional()?,
                    passed: passed.get(&pr.course_id).copied().unwrap_or(0),
                    last_studied: crate::activity::last_touched(c, id, &pr.course_id)?.values().map(|(at, _)| *at).max(),
                    streak: crate::earnings::streak_on_record(c, id, &pr.course_id, tz, now)?,
                    course_id: pr.course_id,
                });
            }
            Ok(Some((email, crate::profile::display_name(&first, &last), opened)))
        })
        .await?;
    let Some((email, name, opened)) = found else {
        return Err(AppError::NotFound);
    };
    let courses = opened
        .into_iter()
        .map(|o| {
            let builtin = state.courses.builtin_by_id(&o.course_id);
            CourseLine {
                title: builtin.map(|b| b.title().to_string()).or(o.title).unwrap_or_else(|| o.course_id.clone()),
                passed: match builtin.map(|b| b.totals().0) {
                    Some(total) => crate::t2("manage.progress_of", "n", o.passed, "total", total),
                    None => o.passed.to_string(),
                },
                last_studied: o.last_studied.map_or_else(|| crate::t("manage.progress_never"), crate::profile::long_date),
                streak: if o.streak == 1 { crate::t("manage.progress_streak_one") } else { crate::t1("manage.progress_streak_days", "n", o.streak) },
                id: o.course_id,
            }
        })
        .collect();
    Ok(Page(ProgressTemplate { user: Some(user), email, name, courses }).into_response())
}

#[cfg(test)]
mod tests {
    use crate::perms::{self, Perm, Perms};

    fn lesson(id: &str) -> crate::content::Lesson {
        let quiz: Vec<String> = (0..3).map(|i| format!(r#"{{"question_id":"{id}-q{i}","prompt":"p","type":"multiple-choice","options":["a","b"],"correct_answer":"a"}}"#)).collect();
        serde_json::from_str(&format!(r#"{{"id":"{id}","slug":"{id}","title":"L","unit":"1","content":"x","quiz":[{}]}}"#, quiz.join(","))).unwrap()
    }

    /// The progress page and what Manage Users shows of it (#147): a
    /// Student's page opens for a superuser and for the owner, with the
    /// lessons passed counted from `lesson_passes`; a private account's, an
    /// owner's and an unknown address are 404s, as is every page for an
    /// account that manages nothing; a superuser's Manage Users holds no
    /// last sign-in and no Progress link for a private account or an owner.
    #[test]
    fn progress_is_shown_for_accounts_without_privacy_only() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();
        let state = crate::routes::test_state(false);
        let now = crate::db::now();
        let (root, sue, tea, stu) = state
            .db
            .with(|c| {
                let root = crate::auth::create_user(c, "root@example.org", "h")?;
                crate::owner::make_owner(c, root)?;
                // A superuser with Privacy, a Teacher (Privacy), and two Students.
                let sue = crate::auth::create_user(c, "sue@example.org", "h")?;
                perms::set_custom(c, sue, Perms::TEACHER.with(Perm::Superuser, true))?;
                let tea = crate::auth::create_user(c, "tea@example.org", "h")?;
                perms::apply_preset(c, tea, perms::TEACHER_PRESET)?;
                let stu = crate::auth::create_user(c, "stu@example.org", "h")?;
                perms::apply_preset(c, stu, perms::STUDENT_PRESET)?;
                c.execute("UPDATE users SET first_name = 'Stu', last_name = 'Dent' WHERE id = ?1", [stu])?;
                let new = crate::auth::create_user(c, "new@example.org", "h")?;
                perms::apply_preset(c, new, perms::STUDENT_PRESET)?;

                // stu: two lessons passed in Spanish (seen + a run of 3 of 3),
                // one seen only; one passed in a course somebody made; and a
                // course that is gone.
                c.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('u-knots', ?1, 'Knots & <Hitches>', 1, 1)", [tea])?;
                for course in ["spanish", "u-knots", "u-gone"] {
                    crate::courses::open_project(c, stu, course)?;
                }
                for (course, id, run) in [("spanish", "a", true), ("spanish", "b", true), ("spanish", "c", false), ("u-knots", "k", true)] {
                    let l = lesson(id);
                    crate::progress::mark_seen(c, stu, course, &l, crate::progress::VIA_STUDY)?;
                    if run {
                        assert!(crate::progress::record_run(c, stu, course, &l, crate::progress::VIA_STUDY, 3, 3)?.passed);
                    }
                }
                // A three-day streak that reached today.
                c.execute(
                    "INSERT INTO earnings (user_id, course_id, points, goals, day, day_points, streak) VALUES (?1, 'spanish', 9, 0, ?2, 3, 3)",
                    rusqlite::params![stu, crate::earnings::day_of(0, now)],
                )?;
                Ok((root, sue, tea, stu))
            })
            .unwrap();
        let cookie = |u| format!("sid={}", state.db.with(|c| crate::auth::create_session(c, u)).unwrap());
        let (root_c, sue_c, tea_c, stu_c) = (cookie(root), cookie(sue), cookie(tea), cookie(stu));
        let total = state.courses.builtin_by_id("spanish").unwrap().totals().0;
        let earnings_rows = || state.db.with(|c| c.query_row("SELECT COUNT(*) FROM earnings", [], |r| r.get::<_, i64>(0))).unwrap();
        let app = crate::routes::router(state.clone());
        rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        let base = format!("http://{addr}");
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        // (status, body)
        let get = |path: &str, cookie: &str| {
            let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
            (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
        };
        // The row of a list for one thing it holds.
        let row = |page: &str, has: &str| page.split("<tr ").find(|r| r.contains(has)).unwrap_or_else(|| panic!("no row with {has}: {page}")).to_string();
        let of = |email: &str| format!("/manage/progress?email={}", crate::urlenc::urlencode(email));
        let today = crate::profile::long_date(now);

        // A Student's page: for a superuser and for the owner.
        for viewer in [&sue_c, &root_c] {
            let (status, page) = get(&of("stu@example.org"), viewer);
            assert_eq!(status, 200, "{page}");
            assert!(page.contains(r#"data-testid="manage-progress-email">stu@example.org<"#) && page.contains(r#"data-testid="manage-progress-name">Stu Dent<"#), "{page}");
            assert_eq!(page.matches(r#"data-testid="manage-progress-course""#).count(), 3);
            // Two passes of the three lessons begun, of the course's total; studied today; the streak.
            let es = row(&page, r#"data-course="spanish""#);
            assert!(es.contains(&format!(r#"data-testid="manage-progress-passed">2 of {total}<"#)), "{es}");
            assert!(es.contains(&format!(r#"data-testid="manage-progress-last-studied">{today}<"#)) && es.contains(r#"data-testid="manage-progress-streak">3 days<"#), "{es}");
            // A made course: its title from its row, the count without a total.
            let knots = row(&page, r#"data-course="u-knots""#);
            assert!(knots.contains("Knots &") && !knots.contains("<Hitches>") && knots.contains(r#"data-testid="manage-progress-passed">1<"#) && knots.contains(">0 days<"), "{knots}");
            // A course that is gone: its id, nothing passed, never studied.
            let gone = row(&page, r#"data-course="u-gone""#);
            assert!(gone.contains(r#"data-testid="manage-progress-course-title">u-gone<"#) && gone.contains(r#"data-testid="manage-progress-passed">0<"#) && gone.contains(">never<"), "{gone}");
            assert!(page.contains(r#"href="/manage" data-testid="manage-progress-back""#), "{page}");
        }
        assert_eq!(get(&of(" STU@example.org "), &sue_c).0, 200, "any case, as the other routes of the page");
        assert_eq!(earnings_rows(), 1, "looking writes nothing: no ledger is made for the courses without one");
        // An account that has opened nothing.
        let (status, page) = get(&of("new@example.org"), &sue_c);
        assert!(status == 200 && page.contains(r#"data-testid="manage-progress-empty""#) && !page.contains(r#"data-testid="manage-progress-course""#), "{page}");

        // No page: an account with Privacy, an owner (even for itself), an address nobody has, no address.
        for viewer in [&sue_c, &root_c] {
            for email in ["tea@example.org", "root@example.org", "nobody@example.org", ""] {
                assert_eq!(get(&of(email), viewer).0, 404, "{email}");
            }
        }
        assert_eq!(get("/manage/progress", &sue_c).0, 404);
        assert_eq!(get(&of("sue@example.org"), &root_c).0, 404, "a superuser with Privacy is private to the owner too");
        // A superuser's own is theirs to open, Privacy or not.
        assert_eq!(get(&of("sue@example.org"), &sue_c).0, 200);
        // An account that manages nothing: no page at all, its own included.
        for viewer in [&tea_c, &stu_c] {
            for email in ["stu@example.org", "tea@example.org", "nobody@example.org"] {
                assert_eq!(get(&of(email), viewer).0, 404, "{email}");
            }
        }

        // Manage Users as a superuser: a private account's row and an owner's hold no last
        // sign-in (each has signed in today) and say Private; the others have the link.
        let dates = |r: &str| r.matches(today.as_str()).count();
        let (_, page) = get("/manage", &sue_c);
        for email in ["tea@example.org", "root@example.org"] {
            let r = row(&page, &format!("<td>{email}</td>"));
            assert_eq!(dates(&r), 1, "created, and no last sign-in: {r}");
            assert!(r.contains(r#"data-testid="manage-account-private""#) && !r.contains("manage-account-progress"), "{r}");
        }
        let r = row(&page, "<td>stu@example.org</td>");
        assert_eq!(dates(&r), 2, "{r}");
        assert!(r.contains(r#"<a class="small" href="/manage/progress?email=stu%40example.org" data-testid="manage-account-progress""#) && !r.contains("manage-account-private"), "{r}");
        let own = row(&page, "<td>sue@example.org</td>");
        assert!(dates(&own) == 2 && own.contains("manage-account-progress") && !own.contains("manage-account-private"), "their own row: {own}");

        // As the owner: every last sign-in, and the same Private marks (their own row too).
        let (_, page) = get("/manage", &root_c);
        for email in ["tea@example.org", "root@example.org", "sue@example.org"] {
            let r = row(&page, &format!("<td>{email}</td>"));
            assert_eq!(dates(&r), 2, "{r}");
            assert!(r.contains(r#"data-testid="manage-account-private""#) && !r.contains("manage-account-progress"), "{r}");
        }
        assert!(row(&page, "<td>stu@example.org</td>").contains("manage-account-progress"));

        // Privacy given: the page is gone and the row says Private. The profile says which is true.
        assert!(get("/profile", &stu_c).1.contains(r#"data-testid="profile-progress-visibility" data-private="false""#));
        state.db.with(|c| perms::set_custom(c, stu, Perms::STUDENT.with(Perm::Privacy, true))).unwrap();
        assert_eq!(get(&of("stu@example.org"), &sue_c).0, 404);
        assert_eq!(get(&of("stu@example.org"), &root_c).0, 404);
        let r = row(&get("/manage", &sue_c).1, "<td>stu@example.org</td>");
        assert!(dates(&r) == 1 && r.contains("manage-account-private") && !r.contains("manage-account-progress"), "{r}");
        for private in [&stu_c, &tea_c, &root_c] {
            assert!(get("/profile", private).1.contains(r#"data-testid="profile-progress-visibility" data-private="true""#));
        }
    }
}
