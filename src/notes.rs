//! A learner's own note on a lesson (#336): one private text per learner,
//! course and lesson, shown and edited on the lesson page ("My note").
//! Table `lesson_notes` (v93). Term mnemonics are for later.

use rusqlite::{params, Connection, OptionalExtension};

/// The longest note, in characters.
pub const MAX_CHARS: usize = 500;

/// The learner's note on a lesson; empty when there is none.
pub fn load(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT body FROM lesson_notes WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3",
        params![user_id, course_id, lesson_id],
        |r| r.get(0),
    )
    .optional()
    .map(Option::unwrap_or_default)
}

/// Save the note as written; an empty (or blank) note deletes it.
pub fn save(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, body: &str, at: i64) -> rusqlite::Result<()> {
    if body.trim().is_empty() {
        conn.execute(
            "DELETE FROM lesson_notes WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3",
            params![user_id, course_id, lesson_id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO lesson_notes (user_id, course_id, lesson_id, body, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (user_id, course_id, lesson_id) DO UPDATE SET body = excluded.body, updated_at = excluded.updated_at",
            params![user_id, course_id, lesson_id, body, at],
        )?;
    }
    Ok(())
}

/// Why a note can't be saved, as the learner reads it; `None` when it can.
pub fn refusal(body: &str) -> Option<String> {
    if body.chars().count() > MAX_CHARS {
        return Some(crate::t1("study.lesson.note_too_long", "max", MAX_CHARS));
    }
    // Control characters other than line breaks and tabs.
    if body.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Some(crate::t("study.lesson.note_bad_chars"));
    }
    None
}

#[cfg(test)]
mod tests;
