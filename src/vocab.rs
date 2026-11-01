//! Vocabulary (terminology) collected by passing quiz questions, and
//! flashcard review stats. Scoped per course.
//!
//! Each collected word has a review schedule (#1360): `due_at`, the start
//! of the learner's local day it is due on, and `gap_days`, the rung of
//! [`LADDER`] it sits on. A new word is due tomorrow; "knew it" on a due
//! word moves it up the ladder, "didn't know" drops it back to one day, and
//! a review before the word is due only counts in the stats.

use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

use crate::db::now;
use crate::sprint::local_day_start;

const DAY: i64 = 86_400;

/// Days until a word is due again after each "knew it" on a due word.
pub const LADDER: [i64; 7] = [1, 3, 7, 16, 35, 75, 150];

/// Most words one due round deals (`/vocabulary/study?due=1`).
pub const DUE_CAP: usize = 20;

/// The rung after `gap`: the smallest gap on the ladder above it, the top one at most.
pub fn next_gap(gap: i64) -> i64 {
    LADDER.iter().copied().find(|&g| g > gap).unwrap_or(150)
}

/// Start of the day after the learner's today: a word is due today when its
/// `due_at` is before this.
fn tomorrow(tz: i64, at: i64) -> i64 {
    local_day_start(tz, at) + DAY
}

/// Where one review leaves a word, as (gap_days, due_at), or `None` when it
/// stays where it is (known, but not due yet). `today` is the start of the
/// learner's local day. A word with no schedule yet counts as due.
pub fn reschedule(gap: i64, due_at: Option<i64>, known: bool, today: i64) -> Option<(i64, i64)> {
    if !known {
        return Some((1, today + DAY));
    }
    if due_at.is_some_and(|d| d >= today + DAY) {
        return None;
    }
    let g = next_gap(gap);
    Some((g, today + g * DAY))
}

#[derive(Debug, Clone)]
pub struct LearnedWord {
    pub lesson_id: String,
    pub word: String,
    pub learned_at: i64,
    pub times_seen: i64,
    pub times_known: i64,
    pub last_reviewed_at: Option<i64>,
}

/// Record `words` as learned for the lesson, due tomorrow in the learner's
/// time zone (`tz`, `CurrentUser::tz`). Returns the ones that were new.
pub fn record_learned(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson_id: &str,
    words: &[&str],
    tz: i64,
) -> rusqlite::Result<Vec<String>> {
    record_learned_at(conn, user_id, course_id, lesson_id, words, now(), tz)
}

/// [`record_learned`] at a given time: the dev crowd's backdated history (`dev_crowd.rs`).
pub(crate) fn record_learned_at(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson_id: &str,
    words: &[&str],
    t: i64,
    tz: i64,
) -> rusqlite::Result<Vec<String>> {
    let mut new = Vec::new();
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO user_vocab (user_id, course_id, lesson_id, word, learned_at, due_at, gap_days)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
    )?;
    let due = tomorrow(tz, t);
    for w in words {
        if stmt.execute(params![user_id, course_id, lesson_id, w, t, due])? > 0 {
            new.push((*w).to_string());
        }
    }
    Ok(new)
}

/// All learned words of a course, keyed by (lesson_id, word).
pub fn load_all(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashMap<(String, String), LearnedWord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lesson_id, word, learned_at, times_seen, times_known, last_reviewed_at
         FROM user_vocab WHERE user_id = ?1 AND course_id = ?2",
    )?;
    let rows = stmt.query_map(params![user_id, course_id], |r| {
        Ok(LearnedWord {
            lesson_id: r.get(0)?,
            word: r.get(1)?,
            learned_at: r.get(2)?,
            times_seen: r.get(3)?,
            times_known: r.get(4)?,
            last_reviewed_at: r.get(5)?,
        })
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let w = row?;
        map.insert((w.lesson_id.clone(), w.word.clone()), w);
    }
    Ok(map)
}

const LEARNED_KEYS: &str = "SELECT lesson_id, word FROM user_vocab WHERE user_id = ?1 AND course_id = ?2";

/// The (lesson_id, word) keys of a course's learned words, without the
/// review stats: what Sprint's review and lightning mixes draw from. Read
/// from the primary key alone.
pub fn learned_keys(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashSet<(String, String)>> {
    let mut stmt = conn.prepare_cached(LEARNED_KEYS)?;
    let rows = stmt.query_map(params![user_id, course_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// Record one flashcard review and move the word's schedule
/// ([`reschedule`]). Returns false if the word was never learned.
pub fn review(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, word: &str, known: bool, tz: i64) -> rusqlite::Result<bool> {
    review_at(conn, user_id, course_id, lesson_id, word, known, tz, now())
}

#[allow(clippy::too_many_arguments)]
fn review_at(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, word: &str, known: bool, tz: i64, at: i64) -> rusqlite::Result<bool> {
    let key = params![user_id, course_id, lesson_id, word];
    let row: Option<(i64, Option<i64>)> = conn
        .prepare_cached("SELECT gap_days, due_at FROM user_vocab WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3 AND word = ?4")?
        .query_row(key, |r| Ok((r.get(0)?, r.get(1)?)))
        .map(Some)
        .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
    let Some((gap, due_at)) = row else { return Ok(false) };
    let (gap, due_at) = reschedule(gap, due_at, known, local_day_start(tz, at)).map_or((gap, due_at), |(g, d)| (g, Some(d)));
    conn.prepare_cached(
        "UPDATE user_vocab SET times_seen = times_seen + 1, times_known = times_known + ?5, last_reviewed_at = ?6, gap_days = ?7, due_at = ?8
         WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3 AND word = ?4",
    )?
    .execute(params![user_id, course_id, lesson_id, word, known as i64, at, gap, due_at])?;
    Ok(true)
}

const DUE_KEYS: &str = "SELECT lesson_id, word FROM user_vocab WHERE user_id = ?1 AND course_id = ?2 AND due_at < ?3
     ORDER BY due_at, learned_at, lesson_id, word LIMIT ?4";

/// How many words are due by the end of the learner's today.
pub fn due_count(conn: &Connection, user_id: i64, course_id: &str, tz: i64, at: i64) -> rusqlite::Result<i64> {
    conn.prepare_cached("SELECT COUNT(*) FROM user_vocab WHERE user_id = ?1 AND course_id = ?2 AND due_at < ?3")?
        .query_row(params![user_id, course_id, tomorrow(tz, at)], |r| r.get(0))
}

/// The (lesson_id, word) keys due by the end of today, longest overdue
/// first, at most `limit`.
pub fn due(conn: &Connection, user_id: i64, course_id: &str, tz: i64, at: i64, limit: usize) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare_cached(DUE_KEYS)?;
    let rows = stmt.query_map(params![user_id, course_id, tomorrow(tz, at), limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// The next day after today that has words due: (start of that day, how
/// many are due on it). `None` when nothing is scheduled after today.
pub fn next_due(conn: &Connection, user_id: i64, course_id: &str, tz: i64, at: i64) -> rusqlite::Result<Option<(i64, i64)>> {
    let first: Option<i64> = conn
        .prepare_cached("SELECT MIN(due_at) FROM user_vocab WHERE user_id = ?1 AND course_id = ?2 AND due_at >= ?3")?
        .query_row(params![user_id, course_id, tomorrow(tz, at)], |r| r.get(0))?;
    let Some(first) = first else { return Ok(None) };
    let day = local_day_start(tz, first);
    let n: i64 = conn
        .prepare_cached("SELECT COUNT(*) FROM user_vocab WHERE user_id = ?1 AND course_id = ?2 AND due_at >= ?3 AND due_at < ?4")?
        .query_row(params![user_id, course_id, day, day + DAY], |r| r.get(0))?;
    Ok(Some((day, n)))
}

/// The learned words a Sprint mix draws from, and which of them are due:
/// the review mix deals those first ([`due_first`]). Derefs to the keys.
#[derive(Debug, Default)]
pub struct Learned {
    pub keys: HashSet<(String, String)>,
    pub due: HashSet<(String, String)>,
}

impl std::ops::Deref for Learned {
    type Target = HashSet<(String, String)>;
    fn deref(&self) -> &Self::Target {
        &self.keys
    }
}

/// [`learned_keys`] and the ones due by the end of today.
pub fn learned_for_mix(conn: &Connection, user_id: i64, course_id: &str, tz: i64, at: i64) -> rusqlite::Result<Learned> {
    let due = due(conn, user_id, course_id, tz, at, usize::MAX >> 1)?.into_iter().collect();
    Ok(Learned { keys: learned_keys(conn, user_id, course_id)?, due })
}

/// Shuffle a mix's collected words, then put the due ones first
/// (`src/sprint.rs` is at its size limit, so the order lives here).
pub fn due_first(words: &mut [(&crate::content::Lesson, usize)], learned: &Learned, rng: &mut crate::quiz::Rng) {
    rng.shuffle(words);
    words.sort_by_cached_key(|(l, i)| l.vocabulary.get(*i).is_none_or(|v| !learned.due.contains(&(l.id.clone(), v.term.clone()))));
}

pub fn count(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM user_vocab WHERE user_id = ?1 AND course_id = ?2",
        params![user_id, course_id],
        |r| r.get(0),
    )
}

/// Most hits the word lookup shows (#348).
pub const LOOKUP_CAP: usize = 20;

/// A lesson a looked-up word is taught in.
#[derive(Debug, Clone, PartialEq)]
pub struct HitLesson {
    pub id: String,
    pub slug: String,
    pub title: String,
}

/// One word the lookup found: a vocabulary entry, or a form from a paradigm
/// table (then `form_of` names its item, set and slots: "ir · Preterite · nosotros").
#[derive(Debug, Clone)]
pub struct Hit {
    pub term: String,
    pub meaning: String,
    pub form_of: String,
    pub lessons: Vec<HitLesson>,
}

/// What `lookup` found: at most `LOOKUP_CAP` hits, and how many there were.
pub struct Lookup {
    pub hits: Vec<Hit>,
    pub total: usize,
}

fn fold(s: &str) -> String {
    crate::quiz::strip_accents(&s.to_lowercase())
}

fn words(s: &str) -> Vec<String> {
    fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// Does every query word start a word of `text`? Accents and case ignored.
fn matches(query: &[String], text: &str) -> bool {
    let have = words(text);
    query.iter().all(|q| have.iter().any(|w| w.starts_with(q.as_str())))
}

/// Look a word up across the course (#348): every lesson's vocabulary (term
/// or meaning) and the forms of its paradigm tables. Each query word must
/// start a word of the entry. Exact terms first, then terms that match, then
/// meanings; course order within each. The same entry taught in several
/// lessons is one hit listing them all.
pub fn lookup(store: &crate::content::ContentStore, q: &str) -> Lookup {
    let query = words(q);
    if query.is_empty() {
        return Lookup { hits: Vec::new(), total: 0 };
    }
    let exact = query.join(" ");
    let rank = |term: &str| if words(term).join(" ") == exact { 0 } else if matches(&query, term) { 1 } else { 2 };
    let mut found: Vec<(u8, Hit)> = Vec::new();
    let mut add = |rank: u8, term: &str, meaning: &str, form_of: String, l: &crate::content::Lesson| {
        let lesson = HitLesson { id: l.id.clone(), slug: l.slug.clone(), title: l.title.clone() };
        if let Some((_, h)) = found.iter_mut().find(|(_, h)| h.term == term && h.meaning == meaning && h.form_of == form_of) {
            if !h.lessons.contains(&lesson) {
                h.lessons.push(lesson);
            }
        } else {
            found.push((rank, Hit { term: term.to_string(), meaning: meaning.to_string(), form_of, lessons: vec![lesson] }));
        }
    };
    for l in store.lessons() {
        for v in &l.vocabulary {
            if matches(&query, &format!("{} {}", v.term, v.meaning)) {
                add(rank(&v.term), &v.term, &v.meaning, String::new(), l);
            }
        }
    }
    if let Some(paradigm) = store.paradigm() {
        for (l, k) in store.conjugations() {
            // One hit per form of a table, naming every slot it fills.
            let mut forms: Vec<(&str, Vec<&str>)> = Vec::new();
            for s in &paradigm.slots {
                let form = k.form(&s.key).trim();
                if form.is_empty() || !matches(&query, form) {
                    continue;
                }
                match forms.iter_mut().find(|(f, _)| *f == form) {
                    Some((_, slots)) => slots.push(&s.label),
                    None => forms.push((form, vec![&s.label])),
                }
            }
            for (form, slots) in forms {
                let of = format!("{} · {} · {}", k.item, paradigm.set_label(&k.set), slots.join(" / "));
                add(rank(form), form, &k.meaning, of, l);
            }
        }
    }
    found.sort_by_key(|(r, _)| *r);
    let total = found.len();
    Lookup { hits: found.into_iter().take(LOOKUP_CAP).map(|(_, h)| h).collect(), total }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spanish() -> crate::content::ContentStore {
        crate::content::ContentStore::load_dir(std::path::Path::new("data/courses/spanish")).expect("spanish course loads")
    }

    #[test]
    fn lookup_finds_terms_meanings_and_forms() {
        let s = spanish();
        // A term, without its accent, exact match first.
        let r = lookup(&s, "manana");
        let first = &r.hits[0];
        assert_eq!(first.term, "mañana", "{first:?}");
        assert!(!first.lessons.is_empty());
        // A meaning finds the term.
        assert!(lookup(&s, "table").hits.iter().any(|h| h.term.contains("mesa")));
        // A paradigm form names both verbs it belongs to.
        let r = lookup(&s, "fuimos");
        let of: Vec<&str> = r.hits.iter().map(|h| h.form_of.as_str()).collect();
        assert!(of.iter().any(|o| o.starts_with("ir ·")) && of.iter().any(|o| o.starts_with("ser ·")), "{of:?}");
        // Word starts only: "esa" does not find "mesa".
        assert!(!lookup(&s, "esa").hits.iter().any(|h| h.term.contains("mesa")));
        // Capped, with the full count kept.
        let r = lookup(&s, "a");
        assert_eq!(r.hits.len(), LOOKUP_CAP);
        assert!(r.total > LOOKUP_CAP);
        assert!(lookup(&s, " ¿? ").hits.is_empty());
    }

    #[test]
    fn learn_once_then_review() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert_eq!(record_learned(c, uid, "es", "l1", &["hola", "adiós"], 0)?, vec!["hola", "adiós"]);
            // Already known words are not reported again.
            assert_eq!(record_learned(c, uid, "es", "l1", &["hola", "gracias"], 0)?, vec!["gracias"]);
            assert_eq!(count(c, uid, "es")?, 3);
            assert_eq!(count(c, uid, "fr")?, 0);
            assert!(review(c, uid, "es", "l1", "hola", true, 0)?);
            assert!(review(c, uid, "es", "l1", "hola", false, 0)?);
            assert!(!review(c, uid, "es", "l1", "nunca", true, 0)?);
            assert!(!review(c, uid, "fr", "l1", "hola", true, 0)?);
            let all = load_all(c, uid, "es")?;
            let hola = &all[&("l1".to_string(), "hola".to_string())];
            assert_eq!((hola.times_seen, hola.times_known), (2, 1));
            assert!(hola.last_reviewed_at.is_some());
            let keys: HashSet<(String, String)> = all.into_keys().collect();
            assert_eq!(learned_keys(c, uid, "es")?, keys);
            assert!(learned_keys(c, uid, "fr")?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_ladder_climbs_and_a_miss_resets_it() {
        let today = 100 * DAY;
        // Knew it on a due word: up one rung each time, then it stays on the top one.
        let mut gap = 1;
        let mut got = Vec::new();
        for _ in 0..8 {
            let (g, due) = reschedule(gap, Some(today), true, today).unwrap();
            assert_eq!(due, today + g * DAY);
            got.push(g);
            gap = g;
        }
        assert_eq!(got, [3, 7, 16, 35, 75, 150, 150, 150]);
        // Not due yet: knew it changes nothing, a miss still drops it to one day.
        assert_eq!(reschedule(16, Some(today + DAY), true, today), None);
        assert_eq!(reschedule(16, Some(today + DAY), false, today), Some((1, today + DAY)));
        // Due later today (in the learner's day) counts as due; overdue too.
        assert_eq!(reschedule(3, Some(today + DAY - 1), true, today), Some((7, today + 7 * DAY)));
        assert_eq!(reschedule(3, Some(today - 9 * DAY), true, today), Some((7, today + 7 * DAY)));
        // No schedule yet counts as due.
        assert_eq!(reschedule(0, None, true, today), Some((1, today + DAY)));
    }

    #[test]
    fn new_words_are_due_tomorrow_in_the_learners_day() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            // 23:00 local, five hours behind UTC.
            let tz = -5 * 3600;
            let t = 200 * DAY + 4 * 3600;
            record_learned_at(c, uid, "es", "l1", &["hola", "adiós"], t, tz)?;
            assert_eq!(due_count(c, uid, "es", tz, t)?, 0);
            assert_eq!(next_due(c, uid, "es", tz, t)?, Some((local_day_start(tz, t) + DAY, 2)));
            // An hour later it is the next local day: both are due.
            let t2 = t + 3600;
            assert_eq!(due_count(c, uid, "es", tz, t2)?, 2);
            assert_eq!(next_due(c, uid, "es", tz, t2)?, None);
            // Knew one: three days off. Missed the other: tomorrow.
            assert!(review_at(c, uid, "es", "l1", "hola", true, tz, t2)?);
            assert!(review_at(c, uid, "es", "l1", "adiós", false, tz, t2)?);
            assert_eq!(due_count(c, uid, "es", tz, t2)?, 0);
            let today = local_day_start(tz, t2);
            assert_eq!(next_due(c, uid, "es", tz, t2)?, Some((today + DAY, 1)));
            // Reviewed early (not due): stats move, the schedule doesn't.
            assert!(review_at(c, uid, "es", "l1", "hola", true, tz, t2 + 60)?);
            let hola = &load_all(c, uid, "es")?[&("l1".to_string(), "hola".to_string())];
            assert_eq!((hola.times_seen, hola.times_known), (2, 2));
            assert_eq!(due_count(c, uid, "es", tz, today + 3 * DAY)?, 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn due_words_come_longest_overdue_first_and_capped() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            let words: Vec<String> = (0..30).map(|i| format!("w{i:02}")).collect();
            for (i, w) in words.iter().enumerate() {
                // w00 was collected last, w29 first: w29 is the longest overdue.
                record_learned_at(c, uid, "es", "l1", &[w.as_str()], (100 - i as i64) * DAY, 0)?;
            }
            let at = 200 * DAY;
            assert_eq!(due_count(c, uid, "es", 0, at)?, 30);
            let got = due(c, uid, "es", 0, at, DUE_CAP)?;
            assert_eq!(got.len(), DUE_CAP);
            assert_eq!(got[0].1, "w29");
            assert_eq!(got[19].1, "w10");
            let mix = learned_for_mix(c, uid, "es", 0, at)?;
            assert_eq!((mix.len(), mix.due.len()), (30, 30));
            assert!(due(c, uid, "fr", 0, at, DUE_CAP)?.is_empty());
            // The due list is a search of the new index.
            crate::db::tests::assert_plan_uses(c, DUE_KEYS, "user_vocab", "idx_user_vocab_due");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_review_mix_deals_due_words_first() {
        let s = spanish();
        let l = s.lessons().iter().find(|l| l.vocabulary.len() >= 4).unwrap();
        let key = |i: usize| (l.id.clone(), l.vocabulary[i].term.clone());
        let learned = Learned { keys: (0..l.vocabulary.len()).map(key).collect(), due: [key(2), key(3)].into_iter().collect() };
        for seed in 0..10 {
            let mut words: Vec<(&crate::content::Lesson, usize)> = (0..l.vocabulary.len()).map(|i| (l, i)).collect();
            due_first(&mut words, &learned, &mut crate::quiz::Rng::seeded(seed));
            let mut first: Vec<usize> = words[..2].iter().map(|&(_, i)| i).collect();
            first.sort();
            assert_eq!(first, [2, 3]);
        }
    }

    /// #627: the learned keys come from an index search, not a table scan.
    #[test]
    fn learned_keys_search_the_primary_key() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {LEARNED_KEYS}"))?;
            let plan: Vec<String> = stmt.query_map(params![1, "es"], |r| r.get::<_, String>(3))?.collect::<Result<_, _>>()?;
            assert!(plan.iter().all(|d| !d.contains("SCAN")), "{plan:?}");
            // The primary key, or the due index (#1360), which covers the keys too.
            assert!(plan.iter().any(|d| d.starts_with("SEARCH user_vocab USING")), "{plan:?}");
            Ok(())
        })
        .unwrap();
    }
}
