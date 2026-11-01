//! The superuser's course table (#1034): every made course, a page at a
//! time, searchable by title, id or owner's address, filtered by what
//! happened to it, flagged ones first, then those the AI check (#1036)
//! wants a closer look at. Each row carries the course's flag
//! row ([`Flag`]) for its detail dialog. Later columns and filters (#1038,
//! #1039, #1040) add a field to [`CourseRow`] or a [`Show`] variant.

use rusqlite::{params, Connection};

use super::{get, Flag, State};

pub const COURSES_PER_PAGE: i64 = 25;

/// Which courses the table shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Show {
    #[default]
    All,
    Flagged,
    /// A part scored high in the AI check (#1036) and waits for stage 2.
    NeedsReview,
    Overridden,
    TakenDown,
    /// Its AI check is the setting's number of versions behind or more
    /// (#1040): the nightly run checks it again.
    Behind,
}

impl Show {
    /// Every filter, in the order the picker lists them.
    pub const ALL: [Show; 6] = [Show::All, Show::Flagged, Show::NeedsReview, Show::Overridden, Show::TakenDown, Show::Behind];

    /// The value in the query string (`course_show=`).
    pub fn as_str(self) -> &'static str {
        match self {
            Show::All => "all",
            Show::Flagged => "flagged",
            Show::NeedsReview => "review",
            Show::Overridden => "overridden",
            Show::TakenDown => "down",
            Show::Behind => "behind",
        }
    }

    /// Anything unknown is All.
    pub fn parse(s: &str) -> Show {
        Show::ALL.into_iter().find(|f| f.as_str() == s).unwrap_or_default()
    }

    /// The i18n key of its name in the picker.
    pub fn label_key(self) -> &'static str {
        match self {
            Show::All => "superuser.courses.show_all",
            Show::Flagged => "superuser.courses.show_flagged",
            Show::NeedsReview => "superuser.courses.show_review",
            Show::Overridden => "superuser.courses.show_overridden",
            Show::TakenDown => "superuser.courses.show_down",
            Show::Behind => "superuser.courses.show_behind",
        }
    }

    /// Its condition on `c` (courses) and `f` (course_flags, left-joined).
    fn sql(self) -> String {
        match self {
            Show::All => "1".into(),
            Show::Flagged => "f.state = 'flagged'".into(),
            Show::NeedsReview => "EXISTS (SELECT 1 FROM content_scores s WHERE s.course_id = c.id AND s.review = 'queued')".into(),
            Show::Overridden => "f.state = 'overridden'".into(),
            Show::TakenDown => "c.taken_down_at IS NOT NULL".into(),
            Show::Behind => behind_sql(),
        }
    }
}

/// Courses that are not taken down and whose AI check is the nightly
/// setting's number of versions behind or more (#1040).
fn behind_sql() -> String {
    format!("c.taken_down_at IS NULL AND {}", crate::content_check::nightly::behind_sql(crate::content_check::nightly::settings().recheck_behind))
}

/// How many courses wait for an AI recheck (#1040): the [`Show::Behind`] filter's count.
pub fn behind_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(&format!("SELECT COUNT(*) FROM courses c WHERE {}", behind_sql()), [], |r| r.get(0))
}

/// One course in the table.
#[derive(Debug, Clone)]
pub struct CourseRow {
    pub id: String,
    pub title: String,
    pub owner_id: i64,
    /// '' for an account with no address.
    pub owner_email: String,
    pub owner_suspended: bool,
    /// The owner's `users.lang` ('' follows the browser): an e-mail to
    /// them is drafted in it (#1035).
    pub owner_lang: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub taken_down: bool,
    /// 'generating' or 'revising': not deleted while a job writes to it.
    pub busy: bool,
    /// When a superuser last e-mailed the owner about it (#1035).
    pub owner_emailed_at: Option<i64>,
    /// Its flag row; `None` when no check has looked at it yet.
    pub flag: Option<Flag>,
    /// Its AI check scores (#1036); `None` when it was never scanned.
    pub scan: Option<crate::content_check::Summary>,
    /// When the content check last read its files (#1038); `None` when never.
    pub checked_at: Option<i64>,
    /// The address of the superuser who overrode the flag, '' when none
    /// (or that account is gone).
    pub overridden_by_email: String,
}

impl CourseRow {
    /// clear / flagged / overridden, or down when it is taken down (that
    /// wins: nothing opens it either way), or review when it is clear but
    /// the AI check scored a part high (#1036).
    pub fn state(&self) -> &'static str {
        if self.taken_down {
            return "down";
        }
        let state = self.flag.as_ref().map_or(State::Clear, |f| f.state);
        if state == State::Clear && self.scan.as_ref().is_some_and(|s| s.needs_review) {
            return "review";
        }
        state.as_str()
    }
}

pub struct CoursePage {
    pub list: Vec<CourseRow>,
    /// How many courses match the search and filter.
    pub total: i64,
    /// This page (from 1) and how many there are (at least 1).
    pub page: i64,
    pub pages: i64,
}

/// Page `page` (from 1; out of range means the nearest page) of the made
/// courses whose title, id or owner's address contains `q` (any case),
/// limited by `show`: flagged ones first, then the most recently edited.
pub fn courses(conn: &Connection, q: &str, show: Show, page: i64) -> rusqlite::Result<CoursePage> {
    let q = q.trim().to_lowercase();
    let filter = format!(
        "FROM courses c JOIN users u ON u.id = c.owner_id LEFT JOIN course_flags f ON f.course_id = c.id
         WHERE {} AND (?1 = '' OR instr(lower(c.title), ?1) > 0 OR instr(lower(c.id), ?1) > 0 OR instr(lower(COALESCE(u.email, '')), ?1) > 0)",
        show.sql()
    );
    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) {filter}"), [&q], |r| r.get(0))?;
    let pages = (total + COURSES_PER_PAGE - 1).max(COURSES_PER_PAGE) / COURSES_PER_PAGE;
    let page = page.clamp(1, pages);
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT c.id, c.title, u.id, COALESCE(u.email, ''), u.suspended_at IS NOT NULL, c.created_at, c.updated_at,
                c.taken_down_at IS NOT NULL, c.state IN ('generating', 'revising'),
                (SELECT k.checked_at FROM content_checks k WHERE k.course_id = c.id),
                u.lang, c.owner_emailed_at
         {filter}
         ORDER BY (COALESCE(f.state, '') = 'flagged' AND c.taken_down_at IS NULL) DESC,
                  EXISTS (SELECT 1 FROM content_scores s WHERE s.course_id = c.id AND s.review = 'queued') DESC, c.updated_at DESC, c.id
         LIMIT ?2 OFFSET ?3"
    ))?;
    let rows = stmt.query_map(params![q, COURSES_PER_PAGE, (page - 1) * COURSES_PER_PAGE], |r| {
        Ok(CourseRow {
            id: r.get(0)?,
            title: r.get(1)?,
            owner_id: r.get(2)?,
            owner_email: r.get(3)?,
            owner_suspended: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            taken_down: r.get(7)?,
            busy: r.get(8)?,
            checked_at: r.get(9)?,
            owner_lang: r.get(10)?,
            owner_emailed_at: r.get(11)?,
            flag: None,
            scan: None,
            overridden_by_email: String::new(),
        })
    })?;
    let mut list = rows.collect::<Result<Vec<_>, _>>()?;
    for row in &mut list {
        row.flag = get(conn, &row.id)?;
        row.scan = crate::content_check::summary(conn, &row.id)?;
        if let Some(by) = row.flag.as_ref().and_then(|f| f.overridden_by) {
            row.overridden_by_email = conn.query_row("SELECT COALESCE(email, '') FROM users WHERE id = ?1", [by], |r| r.get(0)).unwrap_or_default();
        }
    }
    Ok(CoursePage { list, total, page, pages })
}
