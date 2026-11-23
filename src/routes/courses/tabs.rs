//! The course page's tab row (#222, `course_tabs.html`): **Editor** and
//! **Plan** for the course's owner when it may hold students
//! (`Perm::HoldStudents`). Anyone else sees the course page as before, with
//! no row. A page of the course that is not the editor draws the whole
//! header (`course_header.html`: "Courses /", the title, the row); the
//! editor keeps its own title and adds the row under it.
//!
//! A new tab is one link in `course_tabs.html`, current when `tabs.tab`
//! is its name; its page builds `CourseTabs::of(…, "<name>")`.

use crate::auth::CurrentUser;
use crate::courses::CourseRow;
use crate::perms::Perm;

/// The tab row of one course, and which tab is current.
pub struct CourseTabs {
    pub course_id: String,
    pub title: String,
    /// The current tab: "editor" | "plan".
    pub tab: &'static str,
}

impl CourseTabs {
    /// The row for `user` on `row`'s page `tab`, or `None`: not the
    /// course's owner, or it may not hold students.
    pub fn of(user: &CurrentUser, row: &CourseRow, tab: &'static str) -> Option<CourseTabs> {
        (row.owner_id == user.id && user.can(Perm::HoldStudents)).then(|| CourseTabs { course_id: row.id.clone(), title: row.title.clone(), tab })
    }
}
