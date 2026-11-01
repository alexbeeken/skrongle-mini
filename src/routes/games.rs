//! Games hub + Word Match (timed matching with a shared leaderboard) + the
//! paradigm drill ("Conjugation Drill" for a language course) + Numbers by
//! ear (hear a number, type its digits; language courses only) + Date Dash
//! (weekday and month names against the clock; courses with a `calendar`).

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{time_ago, CourseView, Page};
use crate::auth::{CurrentUser, Project};
use crate::db::now;
use crate::error::AppError;
use crate::games::{self, DatesResult, FinishError, NumbersResult, Round, ScoreRow, BOARD_ALL, GAME_DATES, GAME_MATCHING, GAME_NUMBERS};
use crate::quiz::Rng;
use crate::AppState;

#[derive(Template)]
#[template(path = "games.html")]
pub struct GamesTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub playable_lessons: usize,
    pub total_words: usize,
    pub words_learned: i64,
    pub stages_done: usize,
    pub stages_total: usize,
    /// Best Numbers-by-ear streak so far (0 when never played).
    pub best_streak: i64,
    /// Best Date Dash score so far (0 when never played).
    pub best_dates: i64,
}

pub struct BoardCard {
    pub query: String,
    pub title: String,
    pub subtitle: String,
    pub words: usize,
    pub rounds: usize,
    pub record: Option<String>,
    pub record_by: Option<String>,
    pub your_best: Option<String>,
}

#[derive(Template)]
#[template(path = "matching_pick.html")]
pub struct PickTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub boards: Vec<BoardCard>,
}

pub struct ScoreView {
    pub rank: i64,
    pub name: String,
    pub time: String,
    pub wrong: i64,
    pub when: String,
    pub is_you: bool,
}

#[derive(Template)]
#[template(path = "matching.html")]
pub struct MatchingTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub board: String,
    pub title: String,
    pub subtitle: String,
    pub words: usize,
    pub rounds: usize,
    pub lesson_slug: Option<String>,
    pub leaderboard: Vec<ScoreView>,
    pub your_best: Option<String>,
    pub penalty_secs: i64,
}

/// GET /play
/// GET /play -> the Play mode hub: every game and one-off exercise.
pub async fn hub(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let content = &p.course;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let (words_learned, stages_done, best_streak, best_dates) = state
        .db
        .call(move |c| {
            let words = crate::vocab::count(c, uid, &cid)?;
            let stages = crate::progress::passed_ids(c, uid, &cid)?.len();
            let streak = numbers_best_streak(c, uid, &cid)?;
            let dates = dates_labels(c, uid, &cid)?.iter().map(|l| games::dates_points(l)).max().unwrap_or(0);
            Ok((words, stages, streak, dates))
        })
        .await?;
    let playable_lessons = games::playable(content).count();
    let total_words = content.lessons().iter().map(|l| l.vocabulary.len()).sum();
    Ok(Page(GamesTemplate {
        course: CourseView::of(content),
        user: Some(p.user),
        playable_lessons,
        total_words,
        words_learned,
        stages_done,
        stages_total: content.lessons().len(),
        best_streak,
        best_dates,
    })
    .into_response())
}

fn rounds_for(words: usize) -> usize {
    words.div_ceil(games::ROUND_SIZE)
}

fn unique_words(content: &crate::content::ContentStore) -> usize {
    let mut seen = std::collections::HashSet::new();
    content.lessons().iter().flat_map(|l| l.vocabulary.iter()).filter(|v| seen.insert(crate::quiz::normalize(&v.term))).count()
}

/// GET /play/matching  -> choose a board
pub async fn pick(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let uid = p.user.id;
    let content = &p.course;
    let cid = content.id().to_string();
    let course = Arc::clone(&p.course);
    let (holders, bests) = state
        .db
        .call(move |c| {
            let boards = || std::iter::once(BOARD_ALL).chain(games::playable(&course).map(|l| l.id.as_str()));
            let holders = games::record_holders(c, &cid, GAME_MATCHING, boards())?;
            let bests = games::personal_bests(c, uid, &cid, GAME_MATCHING, boards())?;
            Ok((holders, bests))
        })
        .await?;

    let all_words = unique_words(content).min(games::ROUND_SIZE * games::ALL_ROUNDS);
    let mut boards = vec![BoardCard {
        query: "all".into(),
        title: crate::t("play.matching.all_terms"),
        subtitle: crate::t1("play.matching.all_subtitle", "n", all_words),
        words: all_words,
        rounds: rounds_for(all_words),
        record: holders.get(BOARD_ALL).map(|(_, ms)| games::fmt_ms(*ms)),
        record_by: holders.get(BOARD_ALL).map(|(u, _)| u.clone()),
        your_best: bests.get(BOARD_ALL).map(|ms| games::fmt_ms(*ms)),
    }];
    for l in games::playable(content) {
        boards.push(BoardCard {
            query: l.slug.clone(),
            title: l.title.clone(),
            subtitle: format!("{} · {}", content.unit(l.unit_index).title, content.group_title(&l.group)),
            words: l.vocabulary.len(),
            rounds: rounds_for(l.vocabulary.len()),
            record: holders.get(&l.id).map(|(_, ms)| games::fmt_ms(*ms)),
            record_by: holders.get(&l.id).map(|(u, _)| u.clone()),
            your_best: bests.get(&l.id).map(|ms| games::fmt_ms(*ms)),
        });
    }
    Ok(Page(PickTemplate { course: CourseView::of(content), user: Some(p.user), boards }).into_response())
}

#[derive(Deserialize)]
pub struct PlayParams {
    pub lesson: Option<String>,
}

/// Resolve `?lesson=slug` (or none / "all") to a board id.
fn resolve_board(content: &crate::content::ContentStore, lesson: Option<&str>) -> Result<(String, String, String, usize, Option<String>), AppError> {
    match lesson {
        None | Some("all") | Some("") => {
            let words = unique_words(content).min(games::ROUND_SIZE * games::ALL_ROUNDS);
            Ok((BOARD_ALL.into(), crate::t("play.matching.all_terms"), crate::t1("play.matching.all_subtitle", "n", words), words, None))
        }
        Some(slug) => {
            let l = content.lesson_by_slug(slug).ok_or(AppError::NotFound)?;
            if l.vocabulary.len() < games::MIN_WORDS {
                return Err(AppError::NotFound);
            }
            Ok((
                l.id.clone(),
                l.title.clone(),
                format!("{} · {}", content.unit(l.unit_index).title, content.group_title(&l.group)),
                l.vocabulary.len(),
                Some(l.slug.clone()),
            ))
        }
    }
}

fn score_views(rows: Vec<ScoreRow>, me: i64, at: i64) -> Vec<ScoreView> {
    rows.into_iter()
        .map(|r| ScoreView {
            rank: r.rank,
            is_you: r.user_id == me,
            name: r.name,
            time: games::fmt_ms(r.score_ms),
            wrong: r.wrong,
            when: time_ago(r.played_at, at),
        })
        .collect()
}

/// GET /play/matching/play?lesson=slug|all
pub async fn play(
    State(state): State<Arc<AppState>>,
    p: Project,
    Query(params): Query<PlayParams>,
) -> Result<Response, AppError> {
    let content = &p.course;
    let (board, title, subtitle, words, lesson_slug) = resolve_board(content, params.lesson.as_deref())?;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let b = board.clone();
    let (rows, best) = state
        .db
        .call(move |c| Ok((games::leaderboard(c, &cid, GAME_MATCHING, &b, 10)?, games::personal_best(c, uid, &cid, GAME_MATCHING, &b)?)))
        .await?;
    let tpl = MatchingTemplate {
        course: CourseView::of(content),
        user: Some(p.user.clone()),
        board,
        title,
        subtitle,
        words,
        rounds: rounds_for(words),
        lesson_slug,
        leaderboard: score_views(rows, p.user.id, now()),
        your_best: best.map(games::fmt_ms),
        penalty_secs: games::PENALTY_MS / 1000,
    };
    Ok(Page(tpl).into_response())
}

#[derive(Deserialize)]
pub struct StartRequest {
    pub board: String,
}

#[derive(Serialize)]
pub struct StartResponse {
    pub session: String,
    pub rounds: Vec<Round>,
    pub penalty_ms: i64,
}

/// POST /api/games/matching/start { board }  -> deals the rounds and starts the clock
pub async fn start(
    State(state): State<Arc<AppState>>,
    p: Project,
    Json(req): Json<StartRequest>,
) -> Result<Json<StartResponse>, AppError> {
    let content = &p.course;
    let board = if req.board == BOARD_ALL { BOARD_ALL.to_string() } else {
        content.lesson_by_id(&req.board).map(|l| l.id.clone()).ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.unknown_board")))?
    };
    let mut rng = Rng::from_entropy();
    let pairs = games::deal(content, &board, &mut rng).ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.not_enough_terms")))?;
    let rounds = games::make_rounds(&pairs, &mut rng);
    let session = state.games.start(p.user.id, content.id(), &board, pairs, rounds.clone());
    Ok(Json(StartResponse { session, rounds, penalty_ms: games::PENALTY_MS }))
}

#[derive(Deserialize)]
pub struct FinishRequest {
    pub session: String,
    pub attempts: Vec<(usize, usize)>,
}

#[derive(Serialize)]
pub struct FinishResponse {
    pub score_ms: i64,
    pub elapsed_ms: i64,
    pub wrong: i64,
    pub penalty_ms: i64,
    pub score: String,
    /// The rank on the board, `1000+` past `games::RANK_CAP`.
    pub rank: String,
    pub personal_best: bool,
    pub leaderboard: Vec<LeaderboardEntry>,
    /// The worn motivator's line for the round, as HTML (#568).
    pub reward: Option<String>,
}

#[derive(Serialize)]
pub struct LeaderboardEntry {
    pub rank: i64,
    pub name: String,
    pub time: String,
    pub wrong: i64,
    pub when: String,
    pub is_you: bool,
}

/// POST /api/games/matching/finish { session, attempts: [[left,right],...] }
pub async fn finish(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<FinishRequest>,
) -> Result<Json<FinishResponse>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    let since = now();
    if req.attempts.len() > 2000 {
        return Err(AppError::ApiBadRequest(crate::t("play.api.too_many_attempts")));
    }
    let content = &p.course;
    let (session, outcome) = state.games.finish(&req.session, p.user.id, &req.attempts).map_err(|e| {
        AppError::ApiBadRequest(match e {
            FinishError::NoSession => crate::t("play.api.no_session"),
            FinishError::Incomplete => crate::t("play.api.incomplete"),
            FinishError::TooFast => crate::t("play.api.too_fast"),
        })
    })?;
    if session.course_id != content.id() {
        return Err(AppError::ApiBadRequest(crate::t("play.api.other_course")));
    }
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = content.id().to_string();
    let board = session.board.clone();
    let (score_ms, wrong) = (outcome.score_ms, outcome.wrong);
    let label = if session.board == BOARD_ALL {
        "All terms".to_string()
    } else {
        content.lesson_by_id(&session.board).map(|l| l.title.clone()).unwrap_or_else(|| session.board.clone())
    };
    let (pairs, elapsed_ms) = (session.pairs.len() as i64, outcome.elapsed_ms);
    let course = Arc::clone(&p.course);
    let (rank, previous_best, rows) = state
        .db
        .call(move |c| {
            // Word Match on one lesson's terms is a run of that lesson (Play mode):
            // every pair is an answer, every wrong try a miss.
            if let Some(l) = course.lesson_by_id(&board) {
                crate::progress::record_run(c, uid, &cid, l, crate::progress::VIA_PLAY, pairs, pairs + wrong)?;
                crate::activity::touch(c, uid, &cid, &l.id, crate::activity::Mode::Play)?;
            }
            crate::activity::record(c, uid, &cid, crate::activity::Kind::GameItem, pairs, tz)?;
            let previous = games::personal_best(c, uid, &cid, GAME_MATCHING, &board)?;
            let id = games::save_score(c, uid, &cid, GAME_MATCHING, &board, score_ms, wrong)?;
            let rank = games::rank_of(c, &cid, GAME_MATCHING, &board, score_ms, id)?;
            games::record_activity(c, uid, &cid, GAME_MATCHING, &board, &label, elapsed_ms, pairs, pairs, Some(score_ms))?;
            Ok((rank, previous, games::leaderboard(c, &cid, GAME_MATCHING, &board, 10)?))
        })
        .await?;
    tracing::info!("user {} finished matching/{} in {} (rank {rank})", p.user.id, session.board, games::fmt_ms(score_ms));
    let lesson = content.lesson_by_id(&session.board).map(|l| l.id.clone());
    let reward = super::reward_line(&state, &p, super::sprint::tz_offset_secs(&headers), since, lesson).await?;
    let at = now();
    let leaderboard = rows
        .into_iter()
        .map(|r| LeaderboardEntry {
            rank: r.rank,
            is_you: r.user_id == p.user.id,
            name: r.name,
            time: games::fmt_ms(r.score_ms),
            wrong: r.wrong,
            when: time_ago(r.played_at, at),
        })
        .collect();
    Ok(Json(FinishResponse {
        score_ms,
        elapsed_ms: outcome.elapsed_ms,
        wrong,
        penalty_ms: outcome.penalty_ms,
        score: games::fmt_ms(score_ms),
        rank: games::rank_label(rank),
        personal_best: previous_best.map(|b| score_ms < b).unwrap_or(true),
        leaderboard,
        reward,
    }))
}

// =====================================================================
// Paradigm drill
// =====================================================================

use crate::conjugation::{self, Prompt, Verdict};
use std::collections::HashSet;

pub struct ItemOption {
    pub item: String,
    pub meaning: String,
    pub lesson_title: String,
    pub lesson_slug: String,
    pub unlocked: bool,
}

pub struct SetGroup {
    pub key: String,
    pub label: String,
    /// The unlocked items, offered first.
    pub items: Vec<ItemOption>,
    /// The locked items, one group per lesson that unlocks them, in course order.
    pub locked: Vec<LockedGroup>,
    pub unlocked: usize,
    pub total: usize,
}

/// Locked items that one lesson unlocks: the page names the lesson once (#416).
pub struct LockedGroup {
    pub lesson_title: String,
    pub lesson_slug: String,
    pub items: Vec<ItemOption>,
}

impl LockedGroup {
    /// "hablar, trabajar, estudiar" for the folded group's summary.
    pub fn names(&self) -> String {
        self.items.iter().map(|v| v.item.as_str()).collect::<Vec<_>>().join(", ")
    }
}

pub struct SlotLegend {
    pub key: String,
    pub label: String,
    pub hint: String,
    /// The slot's ASCII picture as a JSON array, for the script.
    pub art_json: String,
    /// The person it stands for (`content::Who`), '' for none: the script
    /// draws profile pictures instead of `art` (#879).
    pub who: &'static str,
}

#[derive(Template)]
#[template(path = "conjugation_setup.html")]
pub struct ConjugationSetupTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    /// One tab per set (#837); only the chosen set's items are on the page.
    pub tabs: Vec<super::pages::PickTab>,
    /// The chosen set, with one page of its locked lessons.
    pub sets: Vec<SetGroup>,
    pub pager: super::pages::Pager,
    pub slots: Vec<SlotLegend>,
    /// The pictures the person slots are drawn with (`profile::people_json`).
    pub people_json: String,
    pub any_unlocked: bool,
    pub any_items: bool,
    /// Start drill came back with nothing picked (#949): say why.
    pub pick_one: bool,
}

#[derive(Template)]
#[template(path = "conjugation.html")]
pub struct ConjugationTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub set: String,
    pub set_label: String,
    pub items: Vec<String>,
    pub items_csv: String,
    pub count: usize,
    pub hide_item: bool,
    pub slots: Vec<SlotLegend>,
    /// The pictures the person slots are drawn with (`profile::people_json`).
    pub people_json: String,
}

fn slot_legend(content: &crate::content::ContentStore) -> Vec<SlotLegend> {
    content
        .paradigm()
        .map(|p| {
            p.slots
                .iter()
                .map(|s| SlotLegend {
                    key: s.key.clone(),
                    label: s.label.clone(),
                    hint: s.hint.clone(),
                    art_json: serde_json::to_string(&s.art).unwrap_or_else(|_| "[]".into()),
                    who: s.who.map(|w| w.as_str()).unwrap_or(""),
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn completed(state: &AppState, user_id: i64, course_id: &str) -> Result<HashSet<String>, AppError> {
    let cid = course_id.to_string();
    state.db.call(move |c| conjugation::completed_lessons(c, user_id, &cid)).await
}

/// Locked lessons a page of the drill setup lists (#837).
pub const LOCKED_PER_PAGE: usize = 8;

#[derive(Deserialize)]
pub struct SetupParams {
    pub set: Option<String>,
    pub page: Option<String>,
    /// `empty=1`: the play route sent the learner back with no items.
    pub empty: Option<String>,
}

/// GET /play/conjugation?set=present&page=2  -> choose set + items. One set
/// at a time (a tab per set), its locked lessons a page at a time (#837).
pub async fn conjugation_setup(State(state): State<Arc<AppState>>, p: Project, Query(sp): Query<SetupParams>) -> Result<Response, AppError> {
    let content = &p.course;
    let Some(paradigm) = content.paradigm() else { return Ok(Redirect::to(&crate::url("/play")).into_response()) };
    let done = completed(&state, p.user.id, content.id()).await?;
    let mut sets = Vec::new();
    for set in &paradigm.sets {
        let all: Vec<ItemOption> = content
            .conjugations()
            .filter(|(_, k)| k.set == set.key)
            .map(|(l, k)| ItemOption {
                item: k.item.clone(),
                meaning: k.meaning.clone(),
                lesson_title: l.title.clone(),
                lesson_slug: l.slug.clone(),
                unlocked: done.contains(&l.id),
            })
            .collect();
        if all.is_empty() {
            continue;
        }
        let total = all.len();
        let (items, locked_items): (Vec<ItemOption>, Vec<ItemOption>) = all.into_iter().partition(|v| v.unlocked);
        let mut locked: Vec<LockedGroup> = Vec::new();
        for v in locked_items {
            match locked.iter_mut().find(|g| g.lesson_slug == v.lesson_slug) {
                Some(g) => g.items.push(v),
                None => locked.push(LockedGroup { lesson_title: v.lesson_title.clone(), lesson_slug: v.lesson_slug.clone(), items: vec![v] }),
            }
        }
        let unlocked = items.len();
        sets.push(SetGroup { key: set.key.clone(), label: set.label.clone(), items, locked, unlocked, total });
    }
    let any_unlocked = sets.iter().any(|t| t.unlocked > 0);
    let any_items = !sets.is_empty();
    // The asked-for set, else the first one with something to drill.
    let chosen = sp
        .set
        .as_deref()
        .and_then(|k| sets.iter().position(|t| t.key == k))
        .or_else(|| sets.iter().position(|t| t.unlocked > 0))
        .unwrap_or(0);
    let pick_one = sp.empty.as_deref() == Some("1");
    let base = crate::url("/play/conjugation");
    let tabs = sets
        .iter()
        .enumerate()
        .map(|(i, t)| super::pages::PickTab {
            key: t.key.clone(),
            title: t.label.clone(),
            href: format!("{base}?set={}", crate::urlenc::urlencode(&t.key)),
            done: t.unlocked,
            total: t.total,
            current: i == chosen,
            hint: crate::t2("play.drill.unlocked_count", "n", t.unlocked, "total", t.total),
        })
        .collect();
    let mut pager = super::pages::Pager { page: 1, count: 1, links: Vec::new() };
    let sets = if sets.is_empty() {
        sets
    } else {
        let mut t = sets.swap_remove(chosen.min(sets.len() - 1));
        let page = sp.page.as_deref().and_then(|n| n.parse().ok()).unwrap_or(1);
        let href = format!("{base}?set={}&", crate::urlenc::urlencode(&t.key));
        let (pg, rows) = super::pages::Pager::new(t.locked.len(), LOCKED_PER_PAGE, page, &href);
        t.locked = t.locked.drain(rows).collect();
        pager = pg;
        vec![t]
    };
    Ok(Page(ConjugationSetupTemplate {
        course: CourseView::of(content),
        people_json: super::profile::people_json(&p.user),
        user: Some(p.user),
        tabs,
        sets,
        pager,
        slots: slot_legend(content),
        any_unlocked,
        any_items,
        pick_one,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct ConjugationParams {
    pub set: Option<String>,
    /// Comma-separated items.
    pub items: Option<String>,
    pub n: Option<usize>,
    pub hide: Option<String>,
}

/// Keep only items that exist in `set` and whose lesson the user completed.
fn allowed_items(content: &crate::content::ContentStore, done: &HashSet<String>, set: &str, requested: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for v in requested {
        if let Some((l, k)) = content.conjugation(v, set) {
            if done.contains(&l.id) && !out.contains(&k.item) {
                out.push(k.item.clone());
            }
        }
    }
    out
}

fn parse_items(csv: Option<&str>) -> Vec<String> {
    csv.unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The items a setup form sent: the `items` list its script fills in, else
/// (no JavaScript, #858) the ticked `item-<set>` checkboxes.
fn requested_items(items: Option<&str>, pairs: &[(String, String)], set: &str) -> Vec<String> {
    let listed = parse_items(items);
    if !listed.is_empty() {
        return listed;
    }
    let field = format!("item-{set}");
    pairs.iter().filter(|(k, _)| *k == field).flat_map(|(_, v)| parse_items(Some(v))).collect()
}

/// GET /play/conjugation/play?set=present&items=hablar,cantar&n=20&hide=1
/// (or `item-present=hablar&item-present=cantar` from the form without script)
pub async fn conjugation_play(
    State(state): State<Arc<AppState>>,
    p: Project,
    Query(q): Query<ConjugationParams>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<Response, AppError> {
    let content = &p.course;
    let paradigm = content.paradigm().ok_or(AppError::NotFound)?;
    let set = q.set.as_deref().and_then(|s| paradigm.set(s)).ok_or_else(|| AppError::BadRequest(crate::t("play.api.choose_set")))?;
    let done = completed(&state, p.user.id, content.id()).await?;
    let items = allowed_items(content, &done, &set.key, &requested_items(q.items.as_deref(), &pairs, &set.key));
    if items.is_empty() {
        // Nothing ticked (only possible without script, #949): back to this
        // set's setup with a line that says why.
        let to = format!("{}?set={}&empty=1", crate::url("/play/conjugation"), crate::urlenc::urlencode(&set.key));
        return Ok(Redirect::to(&to).into_response());
    }
    let count = q.n.unwrap_or(20).min(120);
    Ok(Page(ConjugationTemplate {
        course: CourseView::of(content),
        people_json: super::profile::people_json(&p.user),
        user: Some(p.user),
        set: set.key.clone(),
        set_label: set.label.clone(),
        items_csv: items.join(","),
        items,
        count,
        hide_item: q.hide.as_deref().map(|h| h == "1").unwrap_or(false),
        slots: slot_legend(content),
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct DealRequest {
    pub set: String,
    pub items: Vec<String>,
    #[serde(default)]
    pub count: usize,
}

#[derive(Serialize)]
pub struct DealResponse {
    pub prompts: Vec<Prompt>,
}

/// POST /api/games/conjugation/deal { set, items, count }
pub async fn conjugation_deal(
    State(state): State<Arc<AppState>>,
    p: Project,
    Json(req): Json<DealRequest>,
) -> Result<Json<DealResponse>, AppError> {
    let content = &p.course;
    let paradigm = content.paradigm().ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.no_paradigm")))?;
    let set = paradigm.set(&req.set).ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.unknown_set")))?;
    let done = completed(&state, p.user.id, content.id()).await?;
    let items = allowed_items(content, &done, &set.key, &req.items);
    if items.is_empty() {
        return Err(AppError::ApiBadRequest(crate::t("play.api.none_unlocked")));
    }
    let prompts = conjugation::deal(content, &set.key, &items, req.count.min(120), &mut Rng::from_entropy());
    Ok(Json(DealResponse { prompts }))
}

#[derive(Deserialize)]
pub struct CheckRequest {
    pub set: String,
    pub item: String,
    pub slot: String,
    pub answer: String,
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub correct: bool,
    pub expected: String,
    pub note: Option<String>,
    pub slot_label: String,
    pub slot_hint: String,
}

#[derive(Deserialize)]
pub struct ConjugationFinishRequest {
    pub set: String,
    pub items: Vec<String>,
    pub total: i64,
    pub correct: i64,
    pub duration_ms: i64,
}

/// POST /api/games/conjugation/finish { set, items, total, correct, duration_ms }
/// Records a finished drill round for the dashboard and history (no leaderboard).
pub async fn conjugation_finish(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<ConjugationFinishRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    // The drill's items were reported as they were answered: it began `duration_ms` ago.
    let since = now() - req.duration_ms.clamp(0, 6 * 3600 * 1000) / 1000 - 1;
    let content = &p.course;
    let paradigm = content.paradigm().ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.no_paradigm")))?;
    let set = paradigm.set(&req.set).ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.unknown_set")))?;
    if req.total <= 0 || req.total > 500 || req.correct < 0 || req.correct > req.total || req.duration_ms < 0 {
        return Err(AppError::ApiBadRequest(crate::t("play.api.implausible")));
    }
    let items: Vec<String> = req.items.iter().filter(|v| content.conjugation(v, &set.key).is_some()).cloned().collect();
    let label = format!("{} · {} {}", set.label, items.len(), if items.len() == 1 { paradigm.item_label.to_lowercase() } else { format!("{}s", paradigm.item_label.to_lowercase()) });
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = content.id().to_string();
    let board = set.key.clone();
    let (total, correct, duration) = (req.total, req.correct, req.duration_ms.min(6 * 3600 * 1000));
    state
        .db
        .call(move |c| {
            crate::activity::record(c, uid, &cid, crate::activity::Kind::GameItem, total, tz)?;
            games::record_activity(c, uid, &cid, "conjugation", &board, &label, duration, total, correct, None)
        })
        .await?;
    let reward = super::reward_line(&state, &p, super::sprint::tz_offset_secs(&headers), since, None).await?;
    Ok(Json(serde_json::json!({ "ok": true, "reward": reward })))
}

/// POST /api/games/conjugation/check { set, item, slot, answer }
pub async fn conjugation_check(p: Project, Json(req): Json<CheckRequest>) -> Result<Json<CheckResponse>, AppError> {
    let content = &p.course;
    let paradigm = content.paradigm().ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.no_paradigm")))?;
    let set = paradigm.set(&req.set).ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.unknown_set")))?;
    let slot = paradigm.slot(&req.slot).ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.unknown_slot")))?;
    let (_, k) = content.conjugation(&req.item, &set.key).ok_or_else(|| AppError::ApiBadRequest(crate::t("play.api.unknown_item")))?;
    if req.answer.chars().count() > 60 {
        return Err(AppError::ApiBadRequest(crate::t("play.api.answer_too_long")));
    }
    let Verdict { correct, expected, note } = conjugation::check(k.form(&slot.key), &req.answer, &conjugation::slot_words(content));
    Ok(Json(CheckResponse { correct, expected, note, slot_label: slot.label.clone(), slot_hint: slot.hint.clone() }))
}

// =====================================================================
// Numbers by ear
// =====================================================================

/// A preset range offered on the Numbers page.
pub struct RangePreset {
    pub min: i64,
    pub max: i64,
    pub label: String,
}

#[derive(Template)]
#[template(path = "numbers.html")]
pub struct NumbersTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub presets: Vec<RangePreset>,
    pub limit: i64,
    pub rounds_played: i64,
    pub best_streak: i64,
}

fn range_presets() -> Vec<RangePreset> {
    let mut out: Vec<RangePreset> = [(0, 10), (0, 20), (0, 100), (0, 1_000), (0, 10_000), (0, 1_000_000)]
        .iter()
        .map(|&(min, max)| RangePreset { min, max, label: crate::t2("play.numbers.range_preset", "min", games::group_digits(min), "max", games::group_digits(max)) })
        .collect();
    out.push(RangePreset { min: 1900, max: 2030, label: crate::t("play.numbers.range_years") });
    out
}

/// Best streak recorded for this user, read back from the activity labels.
fn parse_best_streak(label: &str) -> i64 {
    label.rsplit("best streak ").next().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// Labels of the user's last Numbers rounds in this course (newest first).
fn numbers_labels(c: &rusqlite::Connection, uid: i64, cid: &str) -> rusqlite::Result<Vec<String>> {
    games::recent_labels(c, uid, cid, GAME_NUMBERS, None, games::RECENT_ROUNDS)
}

fn numbers_best_streak(c: &rusqlite::Connection, uid: i64, cid: &str) -> rusqlite::Result<i64> {
    Ok(numbers_labels(c, uid, cid)?.iter().map(|l| parse_best_streak(l)).max().unwrap_or(0))
}

/// GET /play/numbers  -> the whole game lives on one page; the browser speaks the numbers.
pub async fn numbers(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let content = &p.course;
    if !content.manifest().is_language() {
        return Ok(Redirect::to(&crate::url("/play")).into_response());
    }
    let uid = p.user.id;
    let cid = content.id().to_string();
    let (rounds_played, best_streak) = state
        .db
        .call(move |c| {
            Ok((games::rounds_played(c, uid, &cid, GAME_NUMBERS)?, numbers_best_streak(c, uid, &cid)?))
        })
        .await?;
    Ok(Page(NumbersTemplate {
        course: CourseView::of(content),
        user: Some(p.user),
        presets: range_presets(),
        limit: games::NUMBERS_LIMIT,
        rounds_played,
        best_streak,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct NumbersFinishRequest {
    pub min: i64,
    pub max: i64,
    pub total: i64,
    pub correct: i64,
    pub best_streak: i64,
    pub duration_ms: i64,
}

/// POST /api/games/numbers/finish { min, max, total, correct, best_streak, duration_ms }
/// Records a finished round for the dashboard and history (no leaderboard).
pub async fn numbers_finish(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<NumbersFinishRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    let since = now();
    let content = &p.course;
    if !content.manifest().is_language() {
        return Err(AppError::ApiBadRequest(crate::t("play.api.no_listening")));
    }
    let result = NumbersResult {
        min: req.min,
        max: req.max,
        total: req.total,
        correct: req.correct,
        best_streak: req.best_streak,
        duration_ms: req.duration_ms.min(6 * 3600 * 1000),
    };
    if !result.plausible() {
        return Err(AppError::ApiBadRequest(crate::t("play.api.implausible")));
    }
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = content.id().to_string();
    state
        .db
        .call(move |c| {
            crate::activity::record(c, uid, &cid, crate::activity::Kind::GameItem, result.total, tz)?;
            games::record_activity(c, uid, &cid, GAME_NUMBERS, &result.board(), &result.label(), result.duration_ms, result.total, result.correct, None)
        })
        .await?;
    let reward = super::reward_line(&state, &p, super::sprint::tz_offset_secs(&headers), since, None).await?;
    Ok(Json(serde_json::json!({ "ok": true, "reward": reward })))
}

// =====================================================================
// Date Dash
// =====================================================================

#[derive(Template)]
#[template(path = "dates.html")]
pub struct DatesTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    /// The manifest's `calendar` as JSON for the script (days, months, format).
    pub calendar_json: String,
    pub rounds_played: i64,
    pub best_score: i64,
}

/// Labels of the user's last Date Dash rounds in this course (newest first).
fn dates_labels(c: &rusqlite::Connection, uid: i64, cid: &str) -> rusqlite::Result<Vec<String>> {
    games::recent_labels(c, uid, cid, GAME_DATES, None, games::RECENT_ROUNDS)
}

/// GET /play/dates -> the whole game lives on one page; the browser deals the prompts.
pub async fn dates(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let content = &p.course;
    let Some(cal) = content.manifest().calendar.as_ref() else {
        return Ok(Redirect::to(&crate::url("/play")).into_response());
    };
    let calendar_json = serde_json::to_string(cal).map_err(|e| AppError::Internal(e.to_string()))?;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let (rounds_played, best_score) = state
        .db
        .call(move |c| {
            let best = dates_labels(c, uid, &cid)?.iter().map(|l| games::dates_points(l)).max().unwrap_or(0);
            Ok((games::rounds_played(c, uid, &cid, GAME_DATES)?, best))
        })
        .await?;
    Ok(Page(DatesTemplate { course: CourseView::of(content), user: Some(p.user), calendar_json, rounds_played, best_score }).into_response())
}

#[derive(Deserialize)]
pub struct DatesFinishRequest {
    pub mode: String,
    pub seconds: i64,
    pub total: i64,
    pub correct: i64,
    pub best_combo: i64,
    pub score: i64,
    pub duration_ms: i64,
}

/// POST /api/games/dates/finish { mode, seconds, total, correct, best_combo, score, duration_ms }
/// Records a finished round for the dashboard and history; answers `{ ok, best }`
/// where `best` is the personal best for that drill before this round.
pub async fn dates_finish(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<DatesFinishRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    let since = now();
    let content = &p.course;
    if content.manifest().calendar.is_none() {
        return Err(AppError::ApiBadRequest(crate::t("play.api.no_calendar")));
    }
    let result = DatesResult {
        mode: req.mode,
        seconds: req.seconds,
        total: req.total,
        correct: req.correct,
        best_combo: req.best_combo,
        score: req.score,
        duration_ms: req.duration_ms.min(6 * 3600 * 1000),
    };
    if !result.plausible() {
        return Err(AppError::ApiBadRequest(crate::t("play.api.implausible")));
    }
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = content.id().to_string();
    let best = state
        .db
        .call(move |c| {
            let best = games::recent_labels(c, uid, &cid, GAME_DATES, Some(result.board()), games::RECENT_ROUNDS)?
                .iter()
                .map(|l| games::dates_points(l))
                .max()
                .unwrap_or(0);
            crate::activity::record(c, uid, &cid, crate::activity::Kind::GameItem, result.total, tz)?;
            games::record_activity(c, uid, &cid, GAME_DATES, result.board(), &result.label(), result.duration_ms, result.total, result.correct, None)?;
            Ok(best)
        })
        .await?;
    let reward = super::reward_line(&state, &p, super::sprint::tz_offset_secs(&headers), since, None).await?;
    Ok(Json(serde_json::json!({ "ok": true, "best": best, "reward": reward })))
}
