//! `/tutor`: the AI tutoring chat, and its JSON API.
//!
//! GET  /tutor                 the page (empty state when no API key is on file and no hosted AI)
//! GET  /api/tutor/session?id= rebuild the page after a reload
//! POST /api/tutor/start       { provider, lesson? } -> first turn
//! POST /api/tutor/turn        { session, kind: say|answer|skip, text? } -> the entries this turn added
//! POST /api/tutor/cancel      {} -> { cancelled }: stop waiting for the reply in flight
//!
//! A turn is: the deterministic part first (grade, record, pick the next
//! exercise), then the model talks, then its reply's control marker is
//! applied (show the exercise on deck, drop the pending one, or grade the
//! attempt the model found in a typed message and let it react).

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::hosted::hosted_provider;
use super::quiz::grade_and_record;
use super::{CourseView, Page};
use crate::ai;
use crate::auth::Project;
use crate::db::now;
use crate::error::AppError;
use crate::mastery;
use crate::metering::{CallError, KeySource, Refusal};
use crate::progress;
use crate::quiz::Rng;
use crate::secrets::{self, Provider};
use crate::tutor::{self, Control, Entry, KeyChoice, QuestionCard, Session};
use crate::AppState;

pub struct ProviderChoice {
    pub id: &'static str,
    pub label: String,
    pub selected: bool,
}

#[derive(Template)]
#[template(path = "tutor.html")]
pub struct TutorTemplate {
    pub course: CourseView,
    /// Providers with a key on file; empty means the page shows how to add one.
    pub providers: Vec<ProviderChoice>,
    pub provider: String,
    pub lesson_slug: String,
    pub lesson_title: String,
    /// The course's helper characters as a JSON array, for typing answers.
    pub chars_json: String,
}

#[derive(Deserialize)]
pub struct TutorQuery {
    pub provider: Option<String>,
    pub lesson: Option<String>,
}

/// GET /tutor
pub async fn page(State(state): State<Arc<AppState>>, p: Project, Query(q): Query<TutorQuery>) -> Result<Response, AppError> {
    p.user.require_ai()?;
    let content = &p.course;
    let lesson = match q.lesson.as_deref() {
        Some(slug) => Some(content.lesson_by_slug(slug).ok_or(AppError::NotFound)?),
        None => None,
    };
    // The server's key first when it has one, then the learner's own keys.
    let mut choices: Vec<KeyChoice> = Vec::new();
    if state.meter.hosted() {
        choices.push(KeyChoice::Hosted);
    }
    choices.extend(keys_on_file(&state, p.user.id).await?.into_iter().map(|k| KeyChoice::Own(ai::ProviderKind::from_secret(k))));
    let wanted = q.provider.as_deref().and_then(|w| choices.iter().copied().find(|c| c.id() == w));
    let chosen = wanted.or_else(|| choices.first().copied());
    let providers = choices.iter().map(|c| ProviderChoice { id: c.id(), label: choice_label(*c), selected: Some(*c) == chosen }).collect();
    let chars_json = serde_json::to_string(&content.manifest().chars).unwrap_or_else(|_| "[]".into());
    Ok(Page(TutorTemplate {
        chars_json,
        course: CourseView::of(content),
        providers,
        provider: chosen.map(|p| p.id().to_string()).unwrap_or_default(),
        lesson_slug: lesson.map(|l| l.slug.clone()).unwrap_or_default(),
        lesson_title: lesson.map(|l| l.title.clone()).unwrap_or_default(),
    })
    .into_response())
}

fn choice_label(c: KeyChoice) -> String {
    match c {
        KeyChoice::Own(kind) => kind.secret().label().to_string(),
        KeyChoice::Hosted => crate::t1("tutor.hosted_included", "hosted", tutor::hosted_label()),
    }
}

async fn keys_on_file(state: &AppState, user_id: i64) -> Result<Vec<Provider>, AppError> {
    let vault = state.vault.clone();
    let statuses = state.db.call(move |c| secrets::statuses(c, &vault, user_id)).await?;
    Ok(statuses.into_iter().filter(|s| s.hint.is_some()).map(|s| s.provider).collect())
}

#[derive(Deserialize)]
pub struct SessionQuery {
    pub id: String,
}

/// GET /api/tutor/session?id=
pub async fn session(State(state): State<Arc<AppState>>, p: Project, Query(q): Query<SessionQuery>) -> Result<Json<tutor::Snapshot>, AppError> {
    p.user.require_ai_api()?;
    state
        .tutors
        .with(&q.id, p.user.id, p.course_id(), |s| s.snapshot())
        .map(Json)
        .ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
pub struct StartRequest {
    pub provider: String,
    #[serde(default)]
    pub lesson: String,
}

#[derive(Serialize)]
pub struct TurnResponse {
    pub session: String,
    /// Entries this turn appended, in order.
    pub entries: Vec<Entry>,
    pub pending: Option<String>,
    pub answered: u32,
    pub correct: u32,
    /// Nothing pending and no exercise left to set.
    pub done: bool,
    /// The model could not answer; everything else in the turn still happened.
    pub error: Option<String>,
    /// The model was not asked because Skrongle AI paused:
    /// `daily-cap` | `rate`. `error` says it in words.
    pub ai_out: Option<&'static str>,
}

/// POST /api/tutor/start
pub async fn start(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<StartRequest>,
) -> Result<Json<TurnResponse>, AppError> {
    p.user.require_ai_api()?;
    let provider = match req.provider.as_str() {
        tutor::HOSTED if state.meter.hosted() => KeyChoice::Hosted,
        tutor::HOSTED => return Err(AppError::ApiBadRequest(crate::t1("courses.hosted.not_offered", "hosted", tutor::hosted_label()))),
        other => KeyChoice::Own(Provider::parse(other).map(ai::ProviderKind::from_secret).ok_or_else(|| AppError::ApiBadRequest(crate::t("tutor.api.unknown_provider")))?),
    };
    let lesson = if req.lesson.is_empty() {
        None
    } else {
        Some(p.course.lesson_by_slug(&req.lesson).ok_or_else(|| AppError::ApiBadRequest(crate::t("tutor.api.unknown_lesson")))?.slug.clone())
    };
    if p.course.question_count() == 0 {
        return Err(AppError::ApiBadRequest(crate::t("tutor.api.no_questions")));
    }
    still_calling(p.user.id)?;
    let session = state.tutors.start(p.user.id, p.course_id(), provider, lesson).map_err(refused)?;
    run_turn(&state, &p, session, Turn::Start, super::sprint::tz_offset_secs(&headers)).await.map(Json)
}

#[derive(Deserialize)]
pub struct TurnRequest {
    pub session: String,
    pub kind: String,
    #[serde(default)]
    pub text: String,
}

enum Turn {
    Start,
    Say(String),
    Answer(String),
    Skip,
}

/// POST /api/tutor/turn
pub async fn turn(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<TurnRequest>,
) -> Result<Json<TurnResponse>, AppError> {
    p.user.require_ai_api()?;
    let text = req.text.trim().to_string();
    if text.chars().count() > tutor::MAX_MESSAGE_CHARS {
        return Err(AppError::ApiBadRequest(crate::t("tutor.api.message_too_long")));
    }
    let kind = match req.kind.as_str() {
        "say" if !text.is_empty() => Turn::Say(text),
        "answer" if !text.is_empty() => Turn::Answer(text),
        "skip" => Turn::Skip,
        _ => return Err(AppError::ApiBadRequest(crate::t("tutor.api.bad_turn"))),
    };
    still_calling(p.user.id)?;
    let session = state.tutors.begin(&req.session, p.user.id, p.course_id()).map_err(refused)?;
    run_turn(&state, &p, session, kind, super::sprint::tz_offset_secs(&headers)).await.map(Json)
}

/// POST /api/tutor/cancel: stop waiting for this learner's reply (#890).
/// The call itself runs on until the provider answers and its reply is
/// thrown away; a turn waiting for it ends with `tutor.api.cancelled`.
/// `cancelled` is false when nothing was running; a 429 while a reply
/// they cancelled before is still running.
pub async fn cancel(p: Project) -> Result<Json<serde_json::Value>, AppError> {
    p.user.require_ai_api()?;
    let cancelled = tutor::cancel(p.user.id).map_err(|_| AppError::ApiTooManyRequests(crate::t("tutor.api.already_cancelled")))?;
    Ok(Json(serde_json::json!({ "cancelled": cancelled })))
}

/// Refuse a turn while a call of this learner is still with the model,
/// also one whose request has gone (they left mid-turn, #848).
fn still_calling(user_id: i64) -> Result<(), AppError> {
    if tutor::calling(user_id) {
        return Err(refused(tutor::STILL_ANSWERING));
    }
    Ok(())
}

/// A turn `Sessions` would not start: a 429 while this learner's previous
/// turn is still with the model (#382), else a 400. The page shows either
/// inline.
fn refused(why: &'static str) -> AppError {
    if why == tutor::STILL_ANSWERING {
        AppError::ApiTooManyRequests(crate::t("tutor.api.still_answering"))
    } else {
        AppError::ApiBadRequest(why.into())
    }
}

/// One turn on a copy of the session; the result is written back and the
/// session released whatever happens.
async fn run_turn(state: &AppState, p: &Project, session: Session, kind: Turn, tz: i64) -> Result<TurnResponse, AppError> {
    let (id, uid, cid) = (session.id.clone(), p.user.id, p.course_id().to_string());
    // Released on drop too: the learner may leave mid-call (#659).
    let claim = state.tutors.claim(&id, uid, &cid);
    let mut work = session;
    work.tz = tz;
    let outcome = turn_inner(state, p, &mut work, kind).await;
    let (added, failure) = match outcome {
        Ok(r) => r,
        Err(e) => {
            claim.finish(|_| {});
            return Err(e);
        }
    };
    let provider = work.provider;
    let mut snap = None;
    claim.finish(|s| {
        s.provider = provider;
        for e in &added {
            if let Entry::Answer(o) = e {
                s.answered += 1;
                if o.result.correct {
                    s.correct += 1;
                }
            }
            s.push(e.clone());
        }
        s.on_deck = work.on_deck.take();
        s.reward_since = work.reward_since;
        snap = Some((s.pending.clone(), s.answered, s.correct, s.done()));
    });
    let (pending, answered, correct, done) = snap.unwrap_or((None, 0, 0, true));
    let (error, ai_out) = match failure {
        None => (None, None),
        Some(Fail::Model(e)) => (Some(e), None),
        Some(Fail::Refused(r)) => (Some(r.message()), Some(r.kind())),
    };
    Ok(TurnResponse { session: id, entries: added, pending, answered, correct, done, error, ai_out })
}

/// Why the model did not reply in a turn.
enum Fail {
    /// It was asked and failed (network, key, provider).
    Model(String),
    /// It was not asked: Skrongle AI does not cover the call.
    Refused(Refusal),
}

/// The turn itself. `session` is the working copy: `on_deck` and `pending`
/// are kept current here so each model call sees the right exercise; the
/// entries to append are returned.
/// Returns the entries added and why the model did not reply (if it did not).
async fn turn_inner(state: &AppState, p: &Project, session: &mut Session, kind: Turn) -> Result<(Vec<Entry>, Option<Fail>), AppError> {
    let content = &p.course;
    let mut added: Vec<Entry> = Vec::new();
    let pending_item = |s: &Session| s.pending.as_deref().and_then(|q| content.question(q)).map(|(_, q)| q);

    // 1. The deterministic part: grade, record, pick.
    let instruction = match kind {
        Turn::Start => {
            session.on_deck = next_card(state, p, session).await?;
            tutor::INSTRUCT_START
        }
        Turn::Say(text) => {
            added.push(Entry::Learner { text: text.clone() });
            match pending_item(session).and_then(|q| tutor::obvious_attempt(q, &text)) {
                Some(attempt) => answer(state, p, session, &mut added, attempt).await?,
                None if session.pending.is_some() => tutor::INSTRUCT_SAY_PENDING,
                None => "",
            }
        }
        Turn::Answer(text) => {
            session.pending.as_ref().ok_or_else(|| AppError::ApiBadRequest(crate::t("tutor.api.nothing_pending")))?;
            added.push(Entry::Learner { text: text.clone() });
            answer(state, p, session, &mut added, text).await?
        }
        Turn::Skip => {
            skip(state, p, session, &mut added).await?;
            tutor::INSTRUCT_SKIPPED
        }
    };

    // 2. The model's part, and what its reply asks for.
    let llm = match session.provider {
        KeyChoice::Hosted => hosted_provider(state, "tutor-turn").ok_or_else(|| AppError::ApiBadRequest(crate::t1("courses.hosted.not_offered", "hosted", tutor::hosted_label())))?,
        KeyChoice::Own(kind) => own_provider(state, p, kind).await?,
    };
    let instruction = (!instruction.is_empty()).then_some(instruction);
    let reply = match talk(state, p, &llm, session, &mut added, instruction).await {
        Ok(r) => r,
        Err(e) => return Ok((added, Some(e))),
    };
    match reply {
        Control::Answer(attempt) if session.pending.is_some() => {
            // The typed message was an attempt: grade it, then let the model react.
            let instruction = answer(state, p, session, &mut added, attempt).await?;
            match talk(state, p, &llm, session, &mut added, Some(instruction)).await {
                Ok(Control::Ask) => show_on_deck(session, &mut added),
                Ok(_) => {}
                Err(e) => return Ok((added, Some(e))),
            }
        }
        Control::Skip { ask } if session.pending.is_some() => {
            skip(state, p, session, &mut added).await?;
            if ask {
                show_on_deck(session, &mut added);
            }
        }
        Control::Ask => show_on_deck(session, &mut added),
        _ => {}
    }
    Ok((added, None))
}

/// The learner's saved key for `kind`, as a provider.
async fn own_provider(state: &AppState, p: &Project, kind: ai::ProviderKind) -> Result<ai::Provider, AppError> {
    let provider = kind.secret();
    let (uid, vault) = (p.user.id, state.vault.clone());
    let Some(key) = state.db.call(move |c| secrets::get(c, &vault, uid, provider)).await? else {
        return Err(AppError::ApiBadRequest(crate::t1("tutor.api.no_key", "provider", provider.label())));
    };
    Ok(kind.provider(key))
}

/// One model call over the transcript plus `added`; the words go into
/// `added` as a tutor entry and the control comes back. An `[[ask]]` from
/// the model is applied by the caller, since only some replies may use it.
/// With the server's key the allowance is checked first: a call it does not
/// cover is never started, so no reply is ever cut off. Every call that
/// answers is recorded (`ai_usage`).
async fn talk(
    state: &AppState,
    p: &Project,
    llm: &ai::Provider,
    session: &Session,
    added: &mut Vec<Entry>,
    instruction: Option<&str>,
) -> Result<Control, Fail> {
    let content = &p.course;
    let pending = session.pending.as_deref().and_then(|q| content.question(q));
    let on_deck = session.on_deck.as_ref().and_then(|c| content.question(&c.question_id));
    let lesson = pending.or(on_deck).map(|(l, _)| l);
    let system = tutor::system_prompt(content, &p.user.short_name(), lesson, pending.map(|(_, q)| q), on_deck.map(|(_, q)| q));
    let mut transcript = session.entries.clone();
    transcript.extend(added.iter().cloned());
    let msgs = tutor::messages(&transcript, instruction);
    let call = ai::Call::new("tutor-turn").course(p.course_id());
    let uid = p.user.id;
    let estimate = if llm.source == KeySource::Hosted {
        state.meter.rate_limit(uid).map_err(Fail::Refused)?;
        let tokens = ai::Usage::estimate(&system, &msgs, "");
        state.meter.estimate(&llm.model, tokens.input, u64::from(tutor::REPLY_MAX_TOKENS))
    } else {
        0
    };
    let reply = match tutor::ask(state.db.clone(), Arc::clone(&state.meter), uid, llm.clone(), call, system, msgs, estimate).await {
        Ok(r) => r,
        Err(CallError::Refused(r)) => return Err(Fail::Refused(r)),
        Err(CallError::Failed(e)) => {
            tracing::warn!("tutor ({}) for user {}: {e}", session.provider.id(), p.user.id);
            return Err(Fail::Model(e));
        }
    };
    let (text, control) = tutor::parse_reply(&reply.text);
    if !text.is_empty() {
        added.push(Entry::Tutor { text });
    }
    Ok(control)
}

/// Grade the pending exercise, record the answer, pick the next one on
/// deck; returns the instruction for the model's reaction.
async fn answer(state: &AppState, p: &Project, session: &mut Session, added: &mut Vec<Entry>, text: String) -> Result<&'static str, AppError> {
    let qid = session.pending.take().ok_or_else(|| AppError::ApiBadRequest(crate::t("tutor.api.nothing_pending")))?;
    let text: String = text.chars().take(200).collect();
    let result = grade_and_record(state, p, &qid, &text, crate::activity::Kind::TutorAnswer).await?;
    // A session on one lesson is where the learner left off.
    if let Some(l) = session.lesson.as_deref().and_then(|s| p.course.lesson_by_slug(s)) {
        let (uid, cid, id) = (p.user.id, p.course_id().to_string(), l.id.clone());
        state.db.call(move |c| crate::activity::touch(c, uid, &cid, &id, crate::activity::Mode::Tutor)).await?;
    }
    let passed_lesson = record_tutor_run(state, p, session, added, &result).await?;
    session.on_deck = next_card(state, p, session).await?;
    // A lesson passed, or the last exercise there is, ends a run: the worn
    // motivator's line for what was earned since the last one (#568).
    let reward = if passed_lesson || session.on_deck.is_none() {
        let line = super::reward_line(state, p, session.tz, session.reward_since, Some(result.lesson_id.clone())).await?;
        session.reward_since = now();
        line
    } else {
        None
    };
    added.push(Entry::Answer(tutor::Outcome { question_id: qid, answer: text, result, passed_lesson, reward }));
    Ok(if session.on_deck.is_some() { tutor::INSTRUCT_ANSWERED } else { tutor::INSTRUCT_ANSWERED_LAST })
}

/// The session's answers on this answer's lesson, this one included, as a
/// run of that lesson: the tutor taught it, so once the run counts it is
/// reported (`progress.rs`), until the lesson is passed. True when this
/// answer passed it.
async fn record_tutor_run(
    state: &AppState,
    p: &Project,
    session: &Session,
    added: &[Entry],
    result: &crate::routes::quiz::AnswerResponse,
) -> Result<bool, AppError> {
    let (mut answered, mut correct) = (1i64, i64::from(result.correct));
    for e in session.entries.iter().chain(added) {
        match e {
            Entry::Answer(o) if o.result.lesson_id == result.lesson_id => {
                answered += 1;
                correct += i64::from(o.result.correct);
            }
            // A skipped exercise counts as a miss.
            Entry::Skipped { question_id } if p.course.question(question_id).is_some_and(|(l, _)| l.id == result.lesson_id) => {
                answered += 1;
            }
            _ => {}
        }
    }
    let Some(lesson) = p.course.lesson_by_id(&result.lesson_id) else { return Ok(false) };
    let required = progress::required_answers(lesson);
    if answered < required {
        return Ok(false);
    }
    let first = answered == required;
    let (uid, cid, course, id) = (p.user.id, p.course_id().to_string(), Arc::clone(&p.course), lesson.id.clone());
    state
        .db
        .call(move |c| {
            let l = course.lesson_by_id(&id).expect("lesson exists");
            let passed = progress::load_one(c, uid, &cid, &id)?.is_some_and(|p| p.passed());
            if passed || !(first || progress::run_passes(l, correct, answered)) {
                return Ok(false);
            }
            Ok(progress::record_taught_run(c, uid, &cid, l, "tutor", correct, answered)?.passed_now)
        })
        .await
}

async fn skip(state: &AppState, p: &Project, session: &mut Session, added: &mut Vec<Entry>) -> Result<(), AppError> {
    let qid = session.pending.take().ok_or_else(|| AppError::ApiBadRequest(crate::t("tutor.api.nothing_pending")))?;
    added.push(Entry::Skipped { question_id: qid });
    session.on_deck = next_card(state, p, session).await?;
    Ok(())
}

/// The model asked for the exercise on deck: it becomes pending.
fn show_on_deck(session: &mut Session, added: &mut Vec<Entry>) {
    if let Some(card) = session.on_deck.take() {
        session.pending = Some(card.question_id.clone());
        session.asked.push(card.question_id.clone());
        added.push(Entry::Question(card));
    }
}

async fn next_card(state: &AppState, p: &Project, session: &Session) -> Result<Option<QuestionCard>, AppError> {
    let content = &p.course;
    let at = now();
    let (uid, cid) = (p.user.id, p.course_id().to_string());
    let (progress, recent) = state
        .db
        .call(move |c| Ok((mastery::load_progress(c, uid, &cid)?, mastery::recently_correct(c, uid, &cid, 24 * 3600)?)))
        .await?;
    let recent: HashSet<String> = recent.into_iter().collect();
    let only = session.lesson.as_deref().and_then(|s| content.lesson_by_slug(s));
    let mut rng = Rng::from_entropy();
    Ok(tutor::pick(content, &progress, only, &recent, &session.asked, p.start, at, &mut rng).map(|pk| QuestionCard::of(content, &pk, &mut rng)))
}

#[cfg(test)]
mod tests {
    use crate::metering::{dollars_to_micros, KeySource, MeterConfig, Record};
    use serde_json::{json, Value};

    /// Without the server's key nothing is offered: the tutor, the home
    /// card and the Generate dialog say to add a key, and the API refuses it.
    #[test]
    fn without_the_servers_key_skrongle_ai_is_not_offered() {
        let state = crate::routes::test_state_with(true, None);
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let app_state = state.clone();
        let addr = rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, crate::routes::router(app_state)).await.unwrap() });
            addr
        });
        let work = move || {
            let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
            let url = |p: &str| format!("http://{addr}{p}");
            let r = agent.get(&url("/dev/login?open=spanish")).call().unwrap();
            let cookie = r.headers().get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_string();
            let get = |p: &str| agent.get(&url(p)).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
            let tutor = get("/tutor");
            assert!(!tutor.contains("Skrongle AI (included)") && !tutor.contains(r#"data-provider="hosted""#), "{tutor}");
            assert!(tutor.contains(r#"data-testid="tutor-no-key""#), "{tutor}");
            // The home page's tutor card: a red ! saying so, and "Start a
            // conversation" opens that page as a modal instead of going there.
            let home = get("/");
            assert!(home.contains(r#"data-tip="No AI keys set" data-testid="home-ai-blocked""#) && !home.contains(r#"data-testid="home-info-ai""#), "the ! replaces the (i): {home}");
            assert!(home.contains(r#"<button type="button" class="btn big tutor" data-testid="home-start-tutor" commandfor="home-tutor-gate""#), "{home}");
            assert!(home.contains(r#"data-testid="home-tutor-gate""#), "{home}");
            let start = agent.post(&url("/api/tutor/start")).header("cookie", &cookie).send_json(json!({ "provider": "hosted" })).unwrap();
            assert_eq!(start.status().as_u16(), 400);
            let made = agent.post(&url("/courses/new")).header("cookie", &cookie).send_form([("title", "")]).unwrap();
            let editor = made.headers().get("location").unwrap().to_str().unwrap().to_string();
            let form = get(&editor);
            assert!(form.contains("generate-no-key") && !form.contains(r#"value="hosted""#), "{form}");
        };
        rt.block_on(async move { tokio::task::spawn_blocking(work).await }).unwrap();
    }

    /// Hosted AI end to end over HTTP, through the test simulator on its platform key: the tutor and the
    /// generator offer the server's key to a learner without one, meter it,
    /// refuse before calling once the day's cap is reached (with the way
    /// on), and a generation job reserves and gives back.
    #[test]
    fn hosted_ai_over_http() {
        let mut cfg = MeterConfig {
            tutor_model: "tutor-model".into(),
            courses_model: "course-model".into(),
            daily_cap: dollars_to_micros("1").unwrap(),
            ..MeterConfig::default()
        };
        cfg.prices.merge("tutor-model=1/2,course-model=1/2").unwrap();
        let state = crate::routes::test_state_with(true, Some((crate::dev_ai::PLATFORM_KEYS[0].into(), cfg)));
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let app_state = state.clone();
        let addr = rt.block_on(async move {
            let app = crate::routes::router(app_state);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            addr
        });
        let st = state.clone();
        let work = move || {
            let state = st;
            let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
            let url = |p: &str| format!("http://{addr}{p}");
            let r = agent.get(&url("/dev/login?open=spanish")).call().unwrap();
            let cookie = r.headers().get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_string();
            let get = |p: &str| agent.get(&url(p)).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
            let post = |p: &str, body: Value| -> Value {
                agent.post(&url(p)).header("cookie", &cookie).send_json(&body).unwrap().body_mut().read_json().unwrap()
            };
            let uid: i64 = state.db.with(|c| c.query_row("SELECT id FROM users WHERE email = 'dev@example.com'", [], |r| r.get(0))).unwrap();
            // No key of their own: the tutor runs on the server's key. The home card goes straight to it, with no red !.
            let home = get("/");
            assert!(home.contains(r#"data-testid="home-start-tutor" href="/tutor""#) && !home.contains("home-ai-blocked") && !home.contains("home-tutor-gate") && home.contains(r#"data-testid="home-info-ai""#), "{home}");
            let page = get("/tutor");
            assert!(page.contains(r#"data-provider="hosted""#) && page.contains("Skrongle AI (included)"), "{page}");
            assert!(page.contains("Your messages are sent to the AI provider this server uses, on its key."), "not \"your key\": {page}");
            let t = post("/api/tutor/start", json!({ "provider": "hosted" }));
            assert_eq!(t["error"], Value::Null, "{t}");
            assert!(t["entries"].as_array().unwrap().iter().any(|e| e["kind"] == "tutor"));
            // The simulator's usage at $1 / $2 per MTok.
            let (cost, source, purpose): (i64, String, String) = state
                .db
                .with(|c| c.query_row("SELECT cost_micros, key_source, purpose FROM ai_usage", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))
                .unwrap();
            assert!(cost > 0 && cost < 10_000, "{cost}");
            assert_eq!((source.as_str(), purpose.as_str()), ("hosted", "tutor-turn"));
            // A hosted generation job reserves the estimate, writes the course, settles and releases.
            let made = agent.post(&url("/courses/new")).header("cookie", &cookie).send_form([("title", "")]).unwrap();
            let editor = made.headers().get("location").unwrap().to_str().unwrap().to_string();
            let id = editor.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).unwrap().to_string();
            let form = get(&editor);
            assert!(form.contains(r#"value="hosted""#) && !form.contains("generate-no-key"), "{form}");
            let mut r = agent
                .post(&url(&format!("/courses/{id}/generate")))
                .header("cookie", &cookie)
                .send_form([("provider", "hosted"), ("wish", "Fractions, for the simulator test"), ("lessons", "3")])
                .unwrap();
            assert_eq!(r.status().as_u16(), 303, "{}", r.body_mut().read_to_string().unwrap());
            let loc = r.headers().get("location").unwrap().to_str().unwrap().to_string();
            let job = loc.strip_prefix(&format!("/courses/{id}?job=")).expect("to the editor's progress modal").to_string();
            let mut done = false;
            for _ in 0..100 {
                let s: Value = agent.get(&url(&format!("/api/generate/{job}"))).header("cookie", &cookie).call().unwrap().body_mut().read_json().unwrap();
                if s["done"] == true {
                    assert_eq!(s["error"], Value::Null, "{s}");
                    done = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            assert!(done, "the hosted generation did not finish");
            assert_eq!(state.meter.reservation(&job), None, "released");
            let rows: Vec<(String, String, String)> = state
                .db
                .with(|c| {
                    c.prepare("SELECT purpose, model, key_source FROM ai_usage WHERE job_id = ?1 ORDER BY id")?
                        .query_map([&job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                        .collect()
                })
                .unwrap();
            assert_eq!(rows.len(), 4, "{rows:?}");
            assert!(rows.iter().all(|(_, m, s)| m == "course-model" && s == "hosted"));
            assert_eq!(rows[0].0, "generate-outline");
            let calls = |state: &crate::AppState| state.db.with(|c| c.query_row("SELECT COUNT(*) FROM ai_usage", [], |r| r.get::<_, i64>(0))).unwrap();
            let before = calls(&state);

            // The day's cap is reached: the turn is graded as usual but the model is not asked.
            let call = crate::ai::Call::new("tutor-turn");
            let big = Record {
                user_id: uid,
                call: &call,
                provider: "anthropic",
                model: "tutor-model",
                usage: crate::ai::Usage { input: 2_000_000, output: 0, cached: 0 },
                source: KeySource::Hosted,
            };
            state.db.with(|c| state.meter.record(c, &big)).unwrap();
            let t2 = post("/api/tutor/turn", json!({ "session": t["session"], "kind": "say", "text": "hola" }));
            assert_eq!(t2["ai_out"], "daily-cap", "{t2}");
            assert!(t2["error"].as_str().unwrap().contains("own API key"), "{t2}");
            assert_eq!(t2["entries"][0]["kind"], "learner");
            assert_eq!(calls(&state), before + 1, "only the recorded row: the model was not asked");
            // Asking for a course the same day is refused before any job starts, with the way on.
            let made = agent.post(&url("/courses/new")).header("cookie", &cookie).send_form([("title", "")]).unwrap();
            let editor = made.headers().get("location").unwrap().to_str().unwrap().to_string();
            let id2 = editor.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).unwrap().to_string();
            let mut r = agent
                .post(&url(&format!("/courses/{id2}/generate")))
                .header("cookie", &cookie)
                .send_form([("provider", "hosted"), ("wish", "Fractions, for the simulator test"), ("lessons", "3")])
                .unwrap();
            assert_eq!(r.status().as_u16(), 200);
            let refused = r.body_mut().read_to_string().unwrap();
            assert!(refused.contains("daily spending limit") && refused.contains("ai-out-own-key"), "{refused}");
            assert_eq!(calls(&state), before + 1, "no model call");
        };
        rt.block_on(async move { tokio::task::spawn_blocking(work).await }).unwrap();
    }
}
