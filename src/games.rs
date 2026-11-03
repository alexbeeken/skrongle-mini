//! Games that test lesson knowledge. First game: Word Match (timed pairing of
//! terms with their meanings). Rounds are dealt from the lessons'
//! `vocabulary` lists, timing is measured on the server between `start` and
//! `finish`, and the finish submission is replayed to count wrong attempts.
//! Scores and activity are stored per course.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;

use crate::content::{ContentStore, Lesson};
use crate::db::now;
use crate::quiz::Rng;

pub const GAME_MATCHING: &str = "matching";
/// Numbers by ear: hear a number spoken, type its digits.
pub const GAME_NUMBERS: &str = "numbers";
/// Date Dash: weekday and month names against the clock.
pub const GAME_DATES: &str = "dates";
/// Games that were removed; rows they left in the activity log are not shown (#835: Picture Hunt).
pub const REMOVED_GAMES: &[&str] = &["pictures"];
pub const BOARD_ALL: &str = "all";
/// Words per round and rounds in "all words" mode.
pub const ROUND_SIZE: usize = 5;
pub const ALL_ROUNDS: usize = 5;
/// Penalty per wrong match attempt.
pub const PENALTY_MS: i64 = 1000;
/// Abandoned sessions are dropped after this long.
pub const SESSION_TTL_SECS: u64 = 30 * 60;
/// Minimum vocabulary size for a lesson to be playable.
pub const MIN_WORDS: usize = 2;

#[derive(Serialize, Clone)]
pub struct Pair {
    pub id: usize,
    pub term: String,
    pub meaning: String,
}

#[derive(Serialize, Clone)]
pub struct Round {
    pub left: Vec<Side>,
    pub right: Vec<Side>,
}

#[derive(Serialize, Clone)]
pub struct Side {
    pub id: usize,
    pub text: String,
}

pub struct MatchSession {
    pub token: String,
    pub user_id: i64,
    pub course_id: String,
    pub board: String,
    pub pairs: Vec<Pair>,
    pub rounds: Vec<Round>,
    pub started: Instant,
}

/// Build rounds for a board: `BOARD_ALL` (random words from every lesson) or a lesson id.
pub fn deal(content: &ContentStore, board: &str, rng: &mut Rng) -> Option<Vec<Pair>> {
    let mut pool: Vec<(String, String)> = Vec::new();
    if board == BOARD_ALL {
        let mut seen = HashSet::new();
        for l in content.lessons() {
            for v in &l.vocabulary {
                if seen.insert(crate::quiz::normalize(&v.term)) {
                    pool.push((v.term.clone(), v.meaning.clone()));
                }
            }
        }
        rng.shuffle(&mut pool);
        pool.truncate(ROUND_SIZE * ALL_ROUNDS);
    } else {
        let lesson = content.lesson_by_id(board)?;
        pool = lesson.vocabulary.iter().map(|v| (v.term.clone(), v.meaning.clone())).collect();
        rng.shuffle(&mut pool);
    }
    if pool.len() < MIN_WORDS {
        return None;
    }
    Some(pool.into_iter().enumerate().map(|(id, (term, meaning))| Pair { id, term, meaning }).collect())
}

/// Split pairs into rounds of (at most) `ROUND_SIZE`, sized as evenly as
/// possible (13 words -> 5, 4, 4 rather than 5, 5, 3), shuffling each side.
/// Two words with the same meaning never share a round.
pub fn make_rounds(pairs: &[Pair], rng: &mut Rng) -> Vec<Round> {
    let n = pairs.len();
    if n == 0 {
        return vec![];
    }
    let round_count = n.div_ceil(ROUND_SIZE);
    let base = n / round_count;
    let extra = n % round_count;
    let mut remaining: Vec<&Pair> = pairs.iter().collect();
    let mut rounds = Vec::with_capacity(round_count);
    for r in 0..round_count {
        let size = base + usize::from(r < extra);
        let mut chosen: Vec<&Pair> = Vec::with_capacity(size);
        let mut meanings: HashSet<String> = HashSet::new();
        let mut i = 0;
        while chosen.len() < size && i < remaining.len() {
            let key = crate::quiz::normalize(&remaining[i].meaning);
            if meanings.insert(key) {
                chosen.push(remaining.remove(i));
            } else {
                i += 1;
            }
        }
        // If duplicates made it impossible to fill, take whatever is left.
        while chosen.len() < size && !remaining.is_empty() {
            chosen.push(remaining.remove(0));
        }
        let mut left: Vec<Side> = chosen.iter().map(|p| Side { id: p.id, text: p.term.clone() }).collect();
        let mut right: Vec<Side> = chosen.iter().map(|p| Side { id: p.id, text: p.meaning.clone() }).collect();
        rng.shuffle(&mut left);
        rng.shuffle(&mut right);
        rounds.push(Round { left, right });
    }
    rounds
}

pub struct Outcome {
    pub elapsed_ms: i64,
    pub wrong: i64,
    pub penalty_ms: i64,
    pub score_ms: i64,
}

/// Nobody matches a pair faster than this; anything quicker is a script.
pub const MIN_MS_PER_PAIR: i64 = 300;

#[derive(Debug, PartialEq, Eq)]
pub enum FinishError {
    NoSession,
    Incomplete,
    TooFast,
}

/// Replay `attempts` (left id, right id) against the session's pairs.
/// Returns None unless every pair ends up matched.
pub fn replay(session: &MatchSession, attempts: &[(usize, usize)]) -> Option<Outcome> {
    let n = session.pairs.len();
    let mut matched = vec![false; n];
    let mut wrong = 0i64;
    for &(l, r) in attempts {
        if l >= n || r >= n {
            return None;
        }
        if l == r {
            if matched[l] {
                return None; // matching the same pair twice is not a legal sequence
            }
            matched[l] = true;
        } else {
            wrong += 1;
        }
    }
    if !matched.iter().all(|&m| m) {
        return None;
    }
    let elapsed_ms = session.started.elapsed().as_millis() as i64;
    let penalty_ms = wrong * PENALTY_MS;
    Some(Outcome { elapsed_ms, wrong, penalty_ms, score_ms: elapsed_ms + penalty_ms })
}

/// Game sessions by token. One entry per account at most (so at most
/// `caps::MAX_ACCOUNTS`): `start` drops expired sessions and the account's
/// earlier one before inserting (#42).
#[derive(Default)]
pub struct Sessions {
    inner: Mutex<HashMap<String, MatchSession>>,
}

impl Sessions {
    /// Start a session for `user_id`; any earlier session of theirs is dropped.
    pub fn start(&self, user_id: i64, course_id: &str, board: &str, pairs: Vec<Pair>, rounds: Vec<Round>) -> String {
        let mut map = self.lock();
        map.retain(|_, s| s.user_id != user_id && s.started.elapsed().as_secs() < SESSION_TTL_SECS);
        let token = crate::auth::generate_token();
        map.insert(
            token.clone(),
            MatchSession {
                token: token.clone(),
                user_id,
                course_id: course_id.to_string(),
                board: board.to_string(),
                pairs,
                rounds,
                started: Instant::now(),
            },
        );
        token
    }

    /// Validate a finish submission. The session is consumed only when it is
    /// accepted, so a buggy or malicious submission does not void the game.
    pub fn finish(&self, token: &str, user_id: i64, attempts: &[(usize, usize)]) -> Result<(MatchSession, Outcome), FinishError> {
        let mut map = self.lock();
        let session = match map.get(token) {
            Some(s) if s.user_id == user_id => s,
            _ => return Err(FinishError::NoSession),
        };
        let outcome = replay(session, attempts).ok_or(FinishError::Incomplete)?;
        if outcome.elapsed_ms < session.pairs.len() as i64 * MIN_MS_PER_PAIR {
            return Err(FinishError::TooFast);
        }
        let session = map.remove(token).expect("present");
        Ok((session, outcome))
    }

    #[cfg(test)]
    pub fn take(&self, token: &str, user_id: i64) -> Option<MatchSession> {
        let mut map = self.lock();
        match map.get(token) {
            Some(s) if s.user_id == user_id => map.remove(token),
            _ => None,
        }
    }

    pub fn purge_expired(&self) -> usize {
        let mut map = self.lock();
        let before = map.len();
        map.retain(|_, s| s.started.elapsed().as_secs() < SESSION_TTL_SECS);
        before - map.len()
    }

    /// Forget every game of `user_id` (their account is being deleted).
    pub fn drop_user(&self, user_id: i64) {
        self.lock().retain(|_, s| s.user_id != user_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, MatchSession>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

// ---------- leaderboard persistence ----------

#[derive(Debug, Clone, Serialize)]
pub struct ScoreRow {
    pub rank: i64,
    pub user_id: i64,
    /// How the board shows the player (`profile::public_name`).
    pub name: String,
    pub score_ms: i64,
    pub wrong: i64,
    pub played_at: i64,
}

pub fn save_score(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    game: &str,
    board: &str,
    score_ms: i64,
    wrong: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO game_scores (user_id, course_id, game, board, score_ms, wrong, played_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![user_id, course_id, game, board, score_ms, wrong, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn leaderboard(conn: &Connection, course_id: &str, game: &str, board: &str, limit: i64) -> rusqlite::Result<Vec<ScoreRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT u.id, u.first_name, u.last_name, s.score_ms, s.wrong, s.played_at FROM game_scores s JOIN users u ON u.id = s.user_id
         WHERE s.course_id = ?1 AND s.game = ?2 AND s.board = ?3 ORDER BY s.score_ms ASC, s.played_at ASC LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![course_id, game, board, limit], |r| {
        let name = crate::profile::public_name(&r.get::<_, String>(1)?, &r.get::<_, String>(2)?);
        Ok(ScoreRow { rank: 0, user_id: r.get(0)?, name, score_ms: r.get(3)?, wrong: r.get(4)?, played_at: r.get(5)? })
    })?;
    let mut out: Vec<ScoreRow> = rows.collect::<Result<_, _>>()?;
    for (i, r) in out.iter_mut().enumerate() {
        r.rank = i as i64 + 1;
    }
    Ok(out)
}

pub fn personal_best(conn: &Connection, user_id: i64, course_id: &str, game: &str, board: &str) -> rusqlite::Result<Option<i64>> {
    conn.prepare_cached(PERSONAL_BEST_SQL)?
        .query_row(params![user_id, course_id, game, board], |r| r.get::<_, Option<i64>>(0))
        .optional()
        .map(|o| o.flatten())
}

const PERSONAL_BEST_SQL: &str = "SELECT MIN(score_ms) FROM game_scores WHERE user_id = ?1 AND course_id = ?2 AND game = ?3 AND board = ?4";

/// How far down the board [`rank_of`] counts: past this a score shows as
/// `RANK_CAP+`, so a slow finish does not count every score ever made.
pub const RANK_CAP: i64 = 1000;

/// 1-based rank a score would have on the board (ties count as better), at
/// most `RANK_CAP + 1`: the count stops after `RANK_CAP` better scores.
pub fn rank_of(conn: &Connection, course_id: &str, game: &str, board: &str, score_ms: i64, id: i64) -> rusqlite::Result<i64> {
    conn.prepare_cached(RANK_SQL)?.query_row(params![course_id, game, board, score_ms, id, RANK_CAP], |r| r.get(0))
}

const RANK_SQL: &str = "SELECT COUNT(*) + 1 FROM (SELECT 1 FROM game_scores WHERE course_id = ?1 AND game = ?2 AND board = ?3
     AND (score_ms < ?4 OR (score_ms = ?4 AND id < ?5)) LIMIT ?6)";

/// A rank as the learner sees it: `17`, or `1000+` past [`RANK_CAP`].
pub fn rank_label(rank: i64) -> String {
    if rank > RANK_CAP { format!("{RANK_CAP}+") } else { rank.to_string() }
}

const RECORD_SQL: &str = "SELECT u.first_name, u.last_name, s.score_ms FROM game_scores s JOIN users u ON u.id = s.user_id
     WHERE s.course_id = ?1 AND s.game = ?2 AND s.board = ?3 ORDER BY s.score_ms ASC, s.played_at ASC LIMIT 1";

/// Best score per board, for the picker page: board -> (player's public name,
/// score_ms). One index lookup per board the course has (`boards`), so the
/// work follows the course's size, not the number of scores.
pub fn record_holders<'a>(
    conn: &Connection,
    course_id: &str,
    game: &str,
    boards: impl IntoIterator<Item = &'a str>,
) -> rusqlite::Result<HashMap<String, (String, i64)>> {
    let mut stmt = conn.prepare_cached(RECORD_SQL)?;
    let mut out = HashMap::new();
    for board in boards {
        let row = stmt
            .query_row(params![course_id, game, board], |r| {
                Ok((crate::profile::public_name(&r.get::<_, String>(0)?, &r.get::<_, String>(1)?), r.get::<_, i64>(2)?))
            })
            .optional()?;
        if let Some(row) = row {
            out.insert(board.to_string(), row);
        }
    }
    Ok(out)
}

/// The learner's best per board, for the picker page: one index lookup per
/// board the course has (`boards`).
pub fn personal_bests<'a>(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    game: &str,
    boards: impl IntoIterator<Item = &'a str>,
) -> rusqlite::Result<HashMap<String, i64>> {
    let mut out = HashMap::new();
    for board in boards {
        if let Some(ms) = personal_best(conn, user_id, course_id, game, board)? {
            out.insert(board.to_string(), ms);
        }
    }
    Ok(out)
}

// ---------- activity log (all games) ----------

#[derive(Debug, Clone)]
pub struct Activity {
    pub game: String,
    pub board: String,
    pub label: String,
    pub duration_ms: i64,
    pub total: i64,
    pub correct: i64,
    pub score_ms: Option<i64>,
    pub played_at: i64,
}

#[allow(clippy::too_many_arguments)]
pub fn record_activity(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    game: &str,
    board: &str,
    label: &str,
    duration_ms: i64,
    total: i64,
    correct: i64,
    score_ms: Option<i64>,
) -> rusqlite::Result<()> {
    record_activity_at(conn, user_id, course_id, game, board, label, duration_ms, total, correct, score_ms, now())
}

/// [`record_activity`] at a given time: the dev crowd's backdated history (`dev_crowd.rs`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_activity_at(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    game: &str,
    board: &str,
    label: &str,
    duration_ms: i64,
    total: i64,
    correct: i64,
    score_ms: Option<i64>,
    played_at: i64,
) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO game_activity (user_id, course_id, game, board, label, duration_ms, total, correct, score_ms, played_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?
    .execute(params![user_id, course_id, game, board, label, duration_ms, total, correct, score_ms, played_at])?;
    conn.prepare_cached(
        "INSERT INTO game_rounds (user_id, course_id, game, rounds) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT (user_id, course_id, game) DO UPDATE SET rounds = rounds + 1",
    )?
    .execute(params![user_id, course_id, game])?;
    Ok(())
}

/// Rounds of a game the learner has finished in the course (`game_rounds`,
/// one row, so a page does not count the activity log).
pub fn rounds_played(conn: &Connection, user_id: i64, course_id: &str, game: &str) -> rusqlite::Result<i64> {
    conn.prepare_cached(ROUNDS_SQL)?
        .query_row(params![user_id, course_id, game], |r| r.get(0))
        .optional()
        .map(|o| o.unwrap_or(0))
}

const ROUNDS_SQL: &str = "SELECT rounds FROM game_rounds WHERE user_id = ?1 AND course_id = ?2 AND game = ?3";

/// How many recent rounds [`recent_labels`] reads.
pub const RECENT_ROUNDS: i64 = 200;

const RECENT_GAME_SQL: &str = "SELECT label FROM game_activity WHERE user_id = ?1 AND course_id = ?2 AND game = ?3 ORDER BY played_at DESC LIMIT ?4";
const RECENT_BOARD_SQL: &str =
    "SELECT label FROM game_activity WHERE user_id = ?1 AND course_id = ?2 AND game = ?3 AND board = ?4 ORDER BY played_at DESC LIMIT ?5";

/// Labels of the learner's last `limit` rounds of a game in the course, on
/// one board or any, newest first. Read from `idx_game_activity_game` /
/// `idx_game_activity_board`, so only those rounds are touched.
pub fn recent_labels(conn: &Connection, user_id: i64, course_id: &str, game: &str, board: Option<&str>, limit: i64) -> rusqlite::Result<Vec<String>> {
    match board {
        None => conn.prepare_cached(RECENT_GAME_SQL)?.query_map(params![user_id, course_id, game, limit], |r| r.get(0))?.collect(),
        Some(b) => conn.prepare_cached(RECENT_BOARD_SQL)?.query_map(params![user_id, course_id, game, b, limit], |r| r.get(0))?.collect(),
    }
}

pub fn recent_activity(conn: &Connection, user_id: i64, course_id: &str, limit: i64) -> rusqlite::Result<Vec<Activity>> {
    let mut stmt = conn.prepare_cached(
        "SELECT game, board, label, duration_ms, total, correct, score_ms, played_at FROM game_activity
         WHERE user_id = ?1 AND course_id = ?2 ORDER BY played_at DESC, id DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, limit], |r| {
        Ok(Activity {
            game: r.get(0)?,
            board: r.get(1)?,
            label: r.get(2)?,
            duration_ms: r.get(3)?,
            total: r.get(4)?,
            correct: r.get(5)?,
            score_ms: r.get(6)?,
            played_at: r.get(7)?,
        })
    })?;
    let rows: Vec<Activity> = rows.collect::<rusqlite::Result<_>>()?;
    Ok(rows.into_iter().filter(|a| !REMOVED_GAMES.contains(&a.game.as_str())).collect())
}

/// Games played since `since` (the course home's Play card).
pub struct ActivityStats {
    pub games: i64,
}

pub fn activity_stats(conn: &Connection, user_id: i64, course_id: &str, since: i64) -> rusqlite::Result<ActivityStats> {
    conn.query_row(
        "SELECT COUNT(*) FROM game_activity WHERE user_id = ?1 AND course_id = ?2 AND played_at >= ?3",
        params![user_id, course_id, since],
        |r| Ok(ActivityStats { games: r.get(0)? }),
    )
}

/// Format milliseconds as seconds with two decimals, or m:ss.xx over a minute.
pub fn fmt_ms(ms: i64) -> String {
    let secs = ms as f64 / 1000.0;
    if secs < 60.0 {
        format!("{secs:.2} s")
    } else {
        let m = (secs / 60.0).floor() as i64;
        format!("{m}:{:05.2}", secs - m as f64 * 60.0)
    }
}

/// A finished round of Numbers by ear, as reported by the browser.
#[derive(Debug, Clone, Copy)]
pub struct NumbersResult {
    pub min: i64,
    pub max: i64,
    pub total: i64,
    pub correct: i64,
    pub best_streak: i64,
    pub duration_ms: i64,
}

/// Largest number the game will ever ask for.
pub const NUMBERS_LIMIT: i64 = 1_000_000_000;

impl NumbersResult {
    /// Reject results the game could not have produced (the numbers are drawn
    /// in the browser, so this is the only check the server can make).
    pub fn plausible(&self) -> bool {
        (0..=NUMBERS_LIMIT).contains(&self.min)
            && self.min < self.max
            && self.max <= NUMBERS_LIMIT
            && (1..=500).contains(&self.total)
            && (0..=self.total).contains(&self.correct)
            && (0..=self.correct).contains(&self.best_streak)
            && self.duration_ms >= 0
    }

    /// Board id for the activity log, e.g. `0-100`.
    pub fn board(&self) -> String {
        format!("{}-{}", self.min, self.max)
    }

    /// Human label for the activity log: the range and the best streak.
    pub fn label(&self) -> String {
        format!("{} to {} · best streak {}", group_digits(self.min), group_digits(self.max), self.best_streak)
    }
}

/// The drills Date Dash offers; the browser sends one back as the board id.
pub const DATES_MODES: &[&str] = &["days", "months", "both", "dates", "all"];
/// Round lengths in seconds the browser may report (0 = a fixed count of prompts).
pub const DATES_SECONDS: &[i64] = &[0, 30, 60, 120];
/// The most points one prompt can earn (speed bonus and combo included).
pub const DATES_MAX_POINTS: i64 = 200 * 4;

/// A finished round of Date Dash, as reported by the browser.
#[derive(Debug, Clone)]
pub struct DatesResult {
    pub mode: String,
    pub seconds: i64,
    pub total: i64,
    pub correct: i64,
    pub best_combo: i64,
    pub score: i64,
    pub duration_ms: i64,
}

impl DatesResult {
    /// Reject results the game could not have produced (prompts are drawn in
    /// the browser, so this is the only check the server can make).
    pub fn plausible(&self) -> bool {
        DATES_MODES.contains(&self.mode.as_str())
            && DATES_SECONDS.contains(&self.seconds)
            && (1..=1000).contains(&self.total)
            && (0..=self.total).contains(&self.correct)
            && (0..=self.correct).contains(&self.best_combo)
            && (0..=self.correct * DATES_MAX_POINTS).contains(&self.score)
            && self.duration_ms >= 0
            && (self.seconds == 0 || self.duration_ms <= (self.seconds + 5) * 1000)
    }

    /// Board id for the activity log: the drill.
    pub fn board(&self) -> &str {
        &self.mode
    }

    /// Human label for the activity log, e.g. `Days & months · 60 s · 1,850 pts`.
    pub fn label(&self) -> String {
        let mode = match self.mode.as_str() {
            "days" => "Days",
            "months" => "Months",
            "both" => "Days & months",
            "dates" => "Full dates",
            _ => "Everything",
        };
        let length = if self.seconds > 0 { format!("{} s", self.seconds) } else { format!("{} prompts", self.total) };
        format!("{mode} · {length} · {} pts", group_digits(self.score))
    }
}

/// The points in a Date Dash label (`… · 1,850 pts`), read back for personal bests.
pub fn dates_points(label: &str) -> i64 {
    label.rsplit(" · ").next().and_then(|s| s.strip_suffix(" pts")).and_then(|s| s.replace(',', "").parse().ok()).unwrap_or(0)
}

/// `1234567` -> `1,234,567`.
pub fn group_digits(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Lessons that can be played (enough vocabulary).
pub fn playable<'a>(content: &'a ContentStore) -> impl Iterator<Item = &'a Lesson> {
    content.lessons().iter().filter(|l| l.vocabulary.len() >= MIN_WORDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(n: usize) -> Vec<Pair> {
        (0..n).map(|i| Pair { id: i, term: format!("es{i}"), meaning: format!("en{i}") }).collect()
    }

    fn session(n: usize) -> MatchSession {
        let p = pairs(n);
        let rounds = make_rounds(&p, &mut Rng::seeded(7));
        MatchSession { token: "t".into(), user_id: 1, course_id: "c".into(), board: "b".into(), pairs: p, rounds, started: Instant::now() }
    }

    #[test]
    fn rounds_are_even_and_complete() {
        let p = pairs(13);
        let rounds = make_rounds(&p, &mut Rng::seeded(1));
        let sizes: Vec<usize> = rounds.iter().map(|r| r.left.len()).collect();
        assert_eq!(sizes, vec![5, 4, 4]);
        let mut ids: Vec<usize> = rounds.iter().flat_map(|r| r.left.iter().map(|s| s.id)).collect();
        ids.sort();
        assert_eq!(ids, (0..13).collect::<Vec<_>>());
        for r in &rounds {
            let mut l: Vec<usize> = r.left.iter().map(|s| s.id).collect();
            let mut rr: Vec<usize> = r.right.iter().map(|s| s.id).collect();
            l.sort();
            rr.sort();
            assert_eq!(l, rr);
        }
        assert!(make_rounds(&[], &mut Rng::seeded(1)).is_empty());
    }

    #[test]
    fn same_meaning_never_shares_a_round() {
        let mut p = pairs(6);
        p[3].meaning = "en0".into(); // duplicate meaning of pair 0
        let rounds = make_rounds(&p, &mut Rng::seeded(3));
        for r in &rounds {
            let ids: Vec<usize> = r.left.iter().map(|s| s.id).collect();
            assert!(!(ids.contains(&0) && ids.contains(&3)), "0 and 3 share a round: {ids:?}");
        }
    }

    #[test]
    fn replay_counts_wrong_and_requires_completion() {
        let s = session(3);
        let ok = replay(&s, &[(0, 1), (0, 0), (1, 1), (2, 2)]).unwrap();
        assert_eq!(ok.wrong, 1);
        assert_eq!(ok.penalty_ms, PENALTY_MS);
        assert_eq!(ok.score_ms, ok.elapsed_ms + PENALTY_MS);
        assert!(replay(&s, &[(0, 0), (1, 1)]).is_none()); // incomplete
        assert!(replay(&s, &[(0, 0), (0, 0), (1, 1), (2, 2)]).is_none()); // double match
        assert!(replay(&s, &[(0, 0), (1, 1), (9, 9)]).is_none()); // out of range
    }

    #[test]
    fn sessions_belong_to_users() {
        let sessions = Sessions::default();
        let t1 = sessions.start(1, "c", "b", pairs(2), vec![]);
        let t2 = sessions.start(1, "c", "b", pairs(2), vec![]);
        assert!(sessions.take(&t1, 1).is_none(), "older session replaced");
        assert!(sessions.take(&t2, 2).is_none(), "wrong user");
        assert!(sessions.take(&t2, 1).is_some());
        assert!(sessions.take(&t2, 1).is_none(), "consumed");
        assert_eq!(sessions.purge_expired(), 0);

        // One slot per account: two users, two entries; an old entry goes on the next insert.
        let t4 = sessions.start(1, "c", "b", pairs(2), vec![]);
        sessions.start(2, "c", "b", pairs(2), vec![]);
        assert_eq!(sessions.lock().len(), 2);
        sessions.lock().get_mut(&t4).unwrap().started = Instant::now() - std::time::Duration::from_secs(SESSION_TTL_SECS + 1);
        sessions.start(2, "c", "b", pairs(2), vec![]);
        assert_eq!(sessions.lock().len(), 1, "expired entry gone after the next insert");
        sessions.drop_user(1);
        sessions.drop_user(2);

        // finish(): rejected submissions keep the session; too-fast finishes are refused.
        let t3 = sessions.start(1, "c", "b", pairs(2), vec![]);
        assert_eq!(sessions.finish(&t3, 1, &[(0, 0)]).err(), Some(FinishError::Incomplete));
        assert_eq!(sessions.finish(&t3, 2, &[(0, 0), (1, 1)]).err(), Some(FinishError::NoSession));
        assert_eq!(sessions.finish(&t3, 1, &[(0, 0), (1, 1)]).err(), Some(FinishError::TooFast));
        std::thread::sleep(std::time::Duration::from_millis(2 * MIN_MS_PER_PAIR as u64 + 50));
        assert!(sessions.finish(&t3, 1, &[(0, 0), (1, 1)]).is_ok());
        assert_eq!(sessions.finish(&t3, 1, &[(0, 0), (1, 1)]).err(), Some(FinishError::NoSession));
    }

    #[test]
    fn scores_and_leaderboard() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            crate::profile::update(c, bob, Some("Bob"), Some("smith"))?;
            save_score(c, ann, "es", GAME_MATCHING, "l1", 12_000, 1)?;
            let id = save_score(c, bob, "es", GAME_MATCHING, "l1", 9_500, 0)?;
            save_score(c, ann, "es", GAME_MATCHING, "l1", 15_000, 3)?;
            save_score(c, ann, "es", GAME_MATCHING, BOARD_ALL, 40_000, 0)?;
            save_score(c, ann, "fr", GAME_MATCHING, "l1", 1_000, 0)?;
            let lb = leaderboard(c, "es", GAME_MATCHING, "l1", 10)?;
            assert_eq!(lb.len(), 3);
            assert_eq!((lb[0].rank, lb[0].user_id, lb[0].name.as_str(), lb[0].score_ms), (1, bob, "Bob S.", 9_500));
            assert_eq!(personal_best(c, ann, "es", GAME_MATCHING, "l1")?, Some(12_000));
            assert_eq!(personal_best(c, bob, "es", GAME_MATCHING, BOARD_ALL)?, None);
            assert_eq!(rank_of(c, "es", GAME_MATCHING, "l1", 9_500, id)?, 1);
            assert_eq!(rank_of(c, "es", GAME_MATCHING, "l1", 13_000, i64::MAX)?, 3);
            let holders = record_holders(c, "es", GAME_MATCHING, ["l1", BOARD_ALL, "l2"])?;
            assert_eq!(holders["l1"], ("Bob S.".to_string(), 9_500));
            assert_eq!(holders[BOARD_ALL], ("Learner".to_string(), 40_000));
            assert!(!holders.contains_key("l2"));
            let bests = personal_bests(c, ann, "es", GAME_MATCHING, ["l1", BOARD_ALL, "l2"])?;
            assert_eq!((bests["l1"], bests[BOARD_ALL], bests.get("l2")), (12_000, 40_000, None));
            Ok(())
        })
        .unwrap();
        assert_eq!(fmt_ms(9_500), "9.50 s");
        assert_eq!(fmt_ms(75_250), "1:15.25");
    }

    #[test]
    fn rank_stops_counting_at_the_cap() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            for i in 0..RANK_CAP + 5 {
                save_score(c, ann, "es", GAME_MATCHING, "l1", 1_000 + i, 0)?;
            }
            assert_eq!(rank_of(c, "es", GAME_MATCHING, "l1", 1_003, i64::MAX)?, 5);
            let last = rank_of(c, "es", GAME_MATCHING, "l1", 1_000_000, i64::MAX)?;
            assert_eq!(last, RANK_CAP + 1);
            assert_eq!(rank_label(last), format!("{RANK_CAP}+"));
            assert_eq!(rank_label(RANK_CAP), RANK_CAP.to_string());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn rounds_are_counted_per_game_and_recent_labels_read_newest_first() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert_eq!(rounds_played(c, ann, "es", GAME_NUMBERS)?, 0);
            for (i, board) in ["a", "b", "a"].iter().enumerate() {
                record_activity_at(c, ann, "es", GAME_NUMBERS, board, &format!("r{i} · {i} pts"), 1, 1, 1, None, 100 + i as i64)?;
            }
            record_activity(c, ann, "es", GAME_DATES, "x", "d", 1, 1, 1, None)?;
            record_activity(c, ann, "fr", GAME_NUMBERS, "a", "f", 1, 1, 1, None)?;
            assert_eq!(rounds_played(c, ann, "es", GAME_NUMBERS)?, 3);
            assert_eq!(rounds_played(c, ann, "es", GAME_DATES)?, 1);
            assert_eq!(rounds_played(c, ann, "fr", GAME_NUMBERS)?, 1);
            assert_eq!(recent_labels(c, ann, "es", GAME_NUMBERS, None, 2)?, ["r2 · 2 pts", "r1 · 1 pts"]);
            assert_eq!(recent_labels(c, ann, "es", GAME_NUMBERS, Some("a"), 10)?, ["r2 · 2 pts", "r0 · 0 pts"]);
            Ok(())
        })
        .unwrap();
    }

    /// The query plan's lines for `sql`.
    fn plan(c: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
        stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3)).unwrap().collect::<Result<_, _>>().unwrap()
    }

    /// Every game query reads through an index (#592-#601): no full or
    /// partial scan of `game_scores` / `game_activity`, and no sort.
    #[test]
    fn game_queries_search_an_index() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            for (sql, index) in [
                (PERSONAL_BEST_SQL, "idx_game_scores_user_board"),
                (RANK_SQL, "idx_game_scores_course_board"),
                (RECORD_SQL, "idx_game_scores_course_board"),
                (ROUNDS_SQL, "PRIMARY KEY"),
                (RECENT_GAME_SQL, "idx_game_activity_game"),
                (RECENT_BOARD_SQL, "idx_game_activity_board"),
            ] {
                let lines = plan(c, sql);
                assert!(lines.iter().any(|l| l.starts_with("SEARCH") && l.contains(index)), "{sql}: {lines:?}");
                for l in &lines {
                    assert!(!l.contains("TEMP B-TREE"), "{sql}: sorts: {lines:?}");
                    assert!(!l.starts_with("SCAN") || l.contains("subquery"), "{sql}: scans: {lines:?}");
                }
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn numbers_results_are_checked() {
        let ok = NumbersResult { min: 0, max: 100, total: 20, correct: 17, best_streak: 9, duration_ms: 60_000 };
        assert!(ok.plausible());
        assert_eq!(ok.board(), "0-100");
        assert_eq!(ok.label(), "0 to 100 · best streak 9");
        assert!(!NumbersResult { max: 0, ..ok }.plausible());
        assert!(!NumbersResult { correct: 21, ..ok }.plausible());
        assert!(!NumbersResult { best_streak: 18, ..ok }.plausible());
        assert!(!NumbersResult { total: 0, ..ok }.plausible());
        assert!(!NumbersResult { max: NUMBERS_LIMIT + 1, ..ok }.plausible());
        assert!(!NumbersResult { duration_ms: -1, ..ok }.plausible());
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1_000_000), "1,000,000");
        assert_eq!(group_digits(1234567), "1,234,567");
    }

    #[test]
    fn dates_results_are_checked() {
        let ok = DatesResult { mode: "both".into(), seconds: 60, total: 30, correct: 27, best_combo: 12, score: 4_850, duration_ms: 60_400 };
        assert!(ok.plausible());
        assert_eq!(ok.board(), "both");
        assert_eq!(ok.label(), "Days & months · 60 s · 4,850 pts");
        assert_eq!(dates_points(&ok.label()), 4_850);
        let fixed = DatesResult { seconds: 0, duration_ms: 500_000, ..ok.clone() };
        assert!(fixed.plausible());
        assert_eq!(fixed.label(), "Days & months · 30 prompts · 4,850 pts");
        assert!(!DatesResult { mode: "weeks".into(), ..ok.clone() }.plausible());
        assert!(!DatesResult { seconds: 45, ..ok.clone() }.plausible());
        assert!(!DatesResult { correct: 31, ..ok.clone() }.plausible());
        assert!(!DatesResult { best_combo: 28, ..ok.clone() }.plausible());
        assert!(!DatesResult { score: 27 * DATES_MAX_POINTS + 1, ..ok.clone() }.plausible());
        assert!(!DatesResult { duration_ms: 70_000, ..ok.clone() }.plausible());
        assert!(!DatesResult { total: 0, ..ok.clone() }.plausible());
    }


    #[test]
    fn activity_log_and_stats() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            record_activity(c, ann, "es", GAME_MATCHING, "l1", "Ser vs. Estar", 12_000, 13, 13, Some(13_000))?;
            record_activity(c, ann, "es", "conjugation", "present", "Present · 7 verbs", 90_000, 20, 15, None)?;
            record_activity(c, ann, "fr", "conjugation", "present", "x", 1_000, 1, 1, None)?;
            let recent = recent_activity(c, ann, "es", 10)?;
            assert_eq!(recent.len(), 2);
            assert_eq!(recent[0].game, "conjugation");
            let s = activity_stats(c, ann, "es", 0)?;
            assert_eq!(s.games, 2);
            assert_eq!(activity_stats(c, ann, "es", i64::MAX)?.games, 0);
            Ok(())
        })
        .unwrap();
    }
}
