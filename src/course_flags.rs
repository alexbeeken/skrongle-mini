//! The content flag of a made course (#1033, part of #919): one row per
//! course in `course_flags`. A **flagged** course doesn't start and can't be
//! shared until it is fixed: `courses::load_ready_files` gives nothing for
//! it (so it opens for nobody, its sharees included), and the Share
//! dialog refuses it. Its owner's card has a red ! that names what matched.
//!
//! Two checks write the flag: the listed-term check here ([`check`], over
//! `data/moderation/terms.txt`, see [`terms`]), and later the AI scan
//! (#1036, #1037), which records through [`record`] with [`Source::Ai`]. A
//! superuser can override a flag (#1034); the override columns are kept
//! whatever a check writes afterwards.
//!
//! Hosted only: Skrongle mini checks nothing (CLAUDE.md "Two shapes").
//! Built-in courses are never checked: they don't come from the database.

pub mod owner_mail;
pub mod table;
pub mod terms;
pub mod view;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::content::CourseFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Clear,
    /// Nobody starts it or shares it until it is fixed.
    Flagged,
    /// A superuser looked and let it through (#1034).
    Overridden,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Clear => "clear",
            State::Flagged => "flagged",
            State::Overridden => "overridden",
        }
    }

    fn parse(s: &str) -> State {
        match s {
            "flagged" => State::Flagged,
            "overridden" => State::Overridden,
            _ => State::Clear,
        }
    }
}

/// Which check wrote the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The listed-term check ([`check`]).
    Terms,
    /// The AI scan (#1036, #1037).
    Ai,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Terms => "terms",
            Source::Ai => "ai",
        }
    }

    fn parse(s: &str) -> Source {
        if s == "ai" {
            Source::Ai
        } else {
            Source::Terms
        }
    }
}

/// One thing that triggered a flag: the term (or the AI's quoted text), the
/// file and the field it is in, the lesson's title, and the text around it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub term: String,
    /// The course file (`lessons/x.json`).
    pub path: String,
    /// The JSON field, as a pointer (`/sections/0/body`); empty for a file
    /// that is not JSON (a picture).
    pub field: String,
    /// The lesson's title, or the path when it has none.
    pub lesson: String,
    /// The field's text around the match, for the superuser.
    pub text: String,
}

/// A course's row of `course_flags`.
#[derive(Debug, Clone, PartialEq)]
pub struct Flag {
    pub course_id: String,
    pub state: State,
    pub source: Source,
    pub matches: Vec<Match>,
    pub reason: String,
    pub content_hash: String,
    pub checked_at: i64,
    pub override_note: Option<String>,
    pub overridden_by: Option<i64>,
    pub overridden_at: Option<i64>,
    /// The term list version the term check last ran under (#1039, [`terms::TERMS_VERSION`]).
    pub terms_version: i64,
    /// The AI check version of the AI's last verdict on this row (#1039,
    /// [`crate::content_check::AI_CHECK_VERSION`]); `None` when the AI never wrote one.
    pub ai_version: Option<i64>,
}

impl Flag {
    /// Flagged: it doesn't start and can't be shared.
    pub fn blocks(&self) -> bool {
        self.state == State::Flagged
    }

    /// The distinct terms that matched, in order.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn terms(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for m in &self.matches {
            if !out.contains(&m.term.as_str()) {
                out.push(&m.term);
            }
        }
        out
    }
}

const COLS: &str = "course_id, state, source, matches_json, reason, content_hash, checked_at, override_note, overridden_by, overridden_at, terms_version, ai_version";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Flag> {
    let matches: String = r.get(3)?;
    Ok(Flag {
        course_id: r.get(0)?,
        state: State::parse(&r.get::<_, String>(1)?),
        source: Source::parse(&r.get::<_, String>(2)?),
        matches: serde_json::from_str(&matches).unwrap_or_default(),
        reason: r.get(4)?,
        content_hash: r.get(5)?,
        checked_at: r.get(6)?,
        override_note: r.get(7)?,
        overridden_by: r.get(8)?,
        overridden_at: r.get(9)?,
        terms_version: r.get(10)?,
        ai_version: r.get(11)?,
    })
}

/// The course's flag row, if it was ever checked.
pub fn get(conn: &Connection, course_id: &str) -> rusqlite::Result<Option<Flag>> {
    conn.query_row(&format!("SELECT {COLS} FROM course_flags WHERE course_id = ?1"), [course_id], row).optional()
}

/// Is the course flagged (no start, no share)?
pub fn blocks(conn: &Connection, course_id: &str) -> rusqlite::Result<bool> {
    Ok(get(conn, course_id)?.is_some_and(|f| f.blocks()))
}

/// Write what a check found: the state, which check, what matched, the
/// reason and the hash of what was checked. The override columns stay as
/// they are.
#[allow(clippy::too_many_arguments)]
pub fn record(conn: &Connection, course_id: &str, state: State, source: Source, matches: &[Match], reason: &str, content_hash: &str, now: i64) -> rusqlite::Result<()> {
    let json = serde_json::to_string(matches).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO course_flags (course_id, state, source, matches_json, reason, content_hash, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(course_id) DO UPDATE SET state = excluded.state, source = excluded.source, matches_json = excluded.matches_json,
             reason = excluded.reason, content_hash = excluded.content_hash, checked_at = excluded.checked_at",
        params![course_id, state.as_str(), source.as_str(), json, reason, content_hash, now],
    )?;
    Ok(())
}

/// Save that `ran` (a check) just looked at the course under the current
/// version of its rules (#1039). Call it after [`record`], or when a check
/// ran and found nothing to write.
pub fn stamp(conn: &Connection, course_id: &str, ran: Source) -> rusqlite::Result<()> {
    let (col, version) = match ran {
        Source::Terms => ("terms_version", terms::TERMS_VERSION),
        Source::Ai => ("ai_version", crate::content_check::AI_CHECK_VERSION),
    };
    conn.execute(&format!("UPDATE course_flags SET {col} = ?2 WHERE course_id = ?1"), params![course_id, version])?;
    Ok(())
}

/// A superuser lets a flagged course through (#1034) with a short note
/// saying why: it opens and shares again. The matches and reason stay, so a
/// later check keeps the override for the same terms (see [`check`]) and the
/// AI's second stage can read the note (#1037). `false` when the course is
/// not flagged.
pub fn override_flag(conn: &Connection, course_id: &str, note: &str, by: i64, now: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE course_flags SET state = 'overridden', override_note = ?2, overridden_by = ?3, overridden_at = ?4
         WHERE course_id = ?1 AND state = 'flagged'",
        params![course_id, note, by, now],
    )? > 0)
}

/// Take back an override (#1034): the course is flagged again and the note
/// goes. The hash is emptied, so the next load checks the files again and
/// the flag is whatever the last verdict says now (a term still there flags
/// it, the AI's flag stays until the AI looks again, nothing clears it).
/// `false` when it was not overridden.
pub fn clear_override(conn: &Connection, course_id: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE course_flags SET state = 'flagged', content_hash = '', override_note = NULL, overridden_by = NULL, overridden_at = NULL
         WHERE course_id = ?1 AND state = 'overridden'",
        [course_id],
    )? > 0)
}

/// SHA-256 (hex) of a course's files and of the term list: the same files
/// checked against the same list need no second look.
pub fn content_hash(files: &[CourseFile]) -> String {
    let mut h = Sha256::new();
    h.update(terms::list_hash().as_bytes());
    for f in files {
        h.update(f.path.as_bytes());
        h.update([0]);
        h.update(f.content.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The listed-term check of a made course, given its files: skipped when the
/// files are the ones last checked (same hash) under the current
/// [`terms::TERMS_VERSION`], otherwise run and recorded with that version.
/// Returns the flag as it now stands; `None` on Skrongle mini, which checks
/// nothing. Pure string work, no model call.
///
/// What it writes: no match clears the flag, unless the AI scan flagged
/// the course (that stays until the AI looks again) or a superuser let it
/// through (it stays overridden). A match flags the course, unless a
/// superuser let these same terms through already.
pub fn check(conn: &Connection, course_id: &str, files: &[CourseFile]) -> rusqlite::Result<Option<Flag>> {
    if crate::is_mini() {
        return Ok(None);
    }
    let hash = content_hash(files);
    let old = get(conn, course_id)?;
    if old.as_ref().is_some_and(|f| f.content_hash == hash && f.terms_version == terms::TERMS_VERSION) {
        return Ok(old);
    }
    let found = terms::scan(files);
    let now = crate::db::now();
    // An override keeps what the superuser saw (its matches and reason).
    let kept = |f: &Flag| (State::Overridden, f.source, f.matches.clone(), f.reason.clone());
    let (state, source, matches, reason) = match &old {
        Some(f) if found.is_empty() && f.source == Source::Ai && f.state == State::Flagged => {
            stamp(conn, course_id, Source::Terms)?;
            return get(conn, course_id);
        }
        Some(f) if found.is_empty() && f.state == State::Overridden => kept(f),
        _ if found.is_empty() => (State::Clear, Source::Terms, Vec::new(), String::new()),
        Some(f) if f.state == State::Overridden && found.iter().all(|m| f.matches.iter().any(|o| o.term == m.term)) => kept(f),
        _ => (State::Flagged, Source::Terms, found, String::new()),
    };
    if state == State::Flagged && !old.as_ref().is_some_and(|f| f.blocks()) {
        tracing::info!("course {course_id} is flagged: it contains a listed term");
    }
    record(conn, course_id, state, source, &matches, &reason, &hash, now)?;
    stamp(conn, course_id, Source::Terms)?;
    get(conn, course_id)
}

#[cfg(test)]
mod tests;
