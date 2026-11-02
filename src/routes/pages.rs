//! Home menu, Study mode overview (dashboard), lesson list, lesson viewer, history.
//! All inside the active course.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::quiz::QuestionView;
use super::{time_ago, CourseView, Page};
use crate::activity::{self, Today};
use crate::auth::{CurrentUser, MaybeUser, Project};
use crate::content::{ContentStore, Lesson};
use crate::db::now;
use crate::error::AppError;
use crate::mastery::{self, Progress, Status};
use crate::progress::{self, LessonPass};
use crate::quiz::{check_questions, current_unit, Rng};
use crate::AppState;

// ---------- view models ----------

pub struct TopicRow {
    pub slug: String,
    pub title: String,
    pub group: String,
    pub summary: String,
    pub score: u32,
    pub status_label: String,
    pub status_css: &'static str,
    pub accuracy: u32,
    pub times_tested: i64,
    pub last_practiced: String,
    pub question_count: usize,
    /// The lesson's reading check has been passed.
    pub read: bool,
    /// Passed in any mode (`progress.rs`), and that status in words.
    pub passed: bool,
    pub pass: String,
}

impl TopicRow {
    pub fn with_pass(mut self, pass: Option<&LessonPass>) -> TopicRow {
        self.passed = pass.is_some_and(LessonPass::passed);
        self.pass = pass.map(LessonPass::summary).unwrap_or_default();
        self
    }

    /// `start`: the unit index the learner started the course at (`Project::start`).
    pub fn new(content: &ContentStore, lesson: &Lesson, p: Option<&Progress>, start: usize, at: i64) -> TopicRow {
        let status = Status::of_lesson(p, at, lesson, start);
        TopicRow {
            read: p.map(|p| p.is_read()).unwrap_or(false),
            slug: lesson.slug.clone(),
            title: lesson.title.clone(),
            group: content.group_title(&lesson.group),
            summary: lesson.summary.clone(),
            score: p.map(|p| p.effective(at).round() as u32).unwrap_or(0),
            status_label: status.label(),
            status_css: status.css(),
            accuracy: p.map(|p| p.accuracy_pct()).unwrap_or(0),
            times_tested: p.map(|p| p.times_tested).unwrap_or(0),
            last_practiced: p
                .and_then(|p| p.last_practiced_at)
                .map(|t| time_ago(t, at))
                .unwrap_or_else(|| crate::t("study.never")),
            question_count: lesson.quiz.len(),
            passed: false,
            pass: String::new(),
        }
    }
}

/// One ready-made course on the landing page.
pub struct LandingCourse {
    pub title: String,
    pub icon: String,
    pub description: String,
    pub units: usize,
    pub lessons: usize,
}

/// `/` for a visitor without a session: the sales page. What the app is,
/// how a course is just a folder of JSON files, and where to register.
#[derive(Template)]
#[template(path = "landing.html")]
pub struct LandingTemplate {
    pub user: Option<CurrentUser>,
    pub courses: Vec<LandingCourse>,
}

pub fn landing_courses(courses: &crate::courses::Courses) -> Vec<LandingCourse> {
    courses
        .builtin()
        .iter()
        .map(|c| LandingCourse {
            title: c.title().to_string(),
            icon: c.manifest().icon().to_string(),
            description: c.manifest().description.clone(),
            units: c.units().len(),
            lessons: c.lessons().len(),
        })
        .collect()
}

/// `/`: the landing page for visitors, the study home for signed-in users.
pub async fn root(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Result<Response, AppError> {
    if user.is_none() {
        let courses = landing_courses(&state.courses);
        return Ok(Page(LandingTemplate { user: None, courses }).into_response());
    }
    let (mut parts, _) = req.into_parts();
    let p = <Project as axum::extract::FromRequestParts<Arc<AppState>>>::from_request_parts(&mut parts, &state).await?;
    home(State(state), p, headers).await
}

/// The course home: today's goal as a ring with the streak, lessons passed,
/// terms collected and "Continue" where the learner left off (#558), the
/// unit they are in, then the four modes (Sprint, Study, Play, Guide) and AI
/// tutoring, with one stat each and their submenus.
#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    /// The top of the page, drawn by the motivator worn (`motivators.rs`).
    pub hero: String,
    /// The shared measures the motivator decorated; the mode cards read some too.
    pub progress: crate::motivators::Progress,
    pub weak_count: usize,
    pub week_answered: i64,
    pub games_week: i64,
    /// Label of the AI provider whose key is on file, if any (the tutor uses it).
    pub tutor_provider: Option<String>,
    /// The tutor has nothing to run on: no usable key of their own and no
    /// Skrongle AI for it. The card shows a red ! and "Start a conversation"
    /// opens the tutor's "add an API key" page in a modal instead (#283).
    pub tutor_blocked: bool,
    /// Why Skrongle AI is not offered for the tutor, for that modal.
    pub tutor_gap: Option<super::hosted::NotIncluded>,
    /// Guide stages complete, of the stages not skipped.
    pub guide_completed: usize,
    pub guide_total: usize,
    /// The stage "Continue the guide" opens (none when every stage is done),
    /// and whether any stage has been begun (Start vs Continue).
    pub guide_next: Option<(String, String)>,
    pub guide_begun: bool,
    /// The course's first visit (#574): its settings as a wizard over the
    /// page, until the learner gets started or closes it.
    pub settings: Option<super::courses::CourseSettings>,
    /// Each theme choice's look for the wizard (`SettingsChoices::looks`).
    pub looks: String,
    /// The "Words due" card (#1360): how many words are due today and the
    /// first three of them. No card when nothing is due.
    pub due_count: i64,
    pub due_words: Vec<String>,
    /// How many the card's button reviews: `vocab::DUE_CAP` at most.
    pub due_round: i64,
}

pub struct GameRow {
    pub game: String,
    pub label: String,
    pub result: String,
    pub when: String,
    pub href: String,
    /// What the game recorded, for `history-game[data-correct][data-total]`.
    pub correct: i64,
    pub total: i64,
}

/// Turn an activity row into a dashboard/history line.
pub fn game_row(a: &crate::games::Activity, content: &ContentStore, at: i64) -> GameRow {
    let (game, result, href) = if a.game == crate::games::GAME_MATCHING {
        let q = if a.board == crate::games::BOARD_ALL {
            "all".to_string()
        } else {
            content.lesson_by_id(&a.board).map(|l| l.slug.clone()).unwrap_or_else(|| a.board.clone())
        };
        (
            crate::t("study.games.word_match"),
            a.score_ms.map(crate::games::fmt_ms).unwrap_or_default(),
            crate::url(&format!("/play/matching/play?lesson={q}")),
        )
    } else if a.game == crate::games::GAME_NUMBERS {
        (crate::t("study.games.numbers"), timed_result(a), crate::url("/play/numbers"))
    } else if a.game == crate::games::GAME_DATES {
        (crate::t("study.games.date_dash"), timed_result(a), crate::url("/play/dates"))
    } else if a.game == crate::listening::GAME {
        (crate::t("study.games.listening"), timed_result(a), crate::listening::href(content, &a.board))
    } else if a.game == crate::sprint::GAME {
        let href = match a.board.as_str() {
            crate::sprint::BOARD_REVIEW => crate::url("/sprint/play?mode=review"),
            crate::sprint::BOARD_LIGHTNING => crate::url("/sprint/play?mode=lightning"),
            id => match content.lesson_by_id(id) {
                Some(l) => crate::url(&format!("/sprint/play?lesson={}", l.slug)),
                None => crate::url("/sprint"),
            },
        };
        (crate::t("study.games.sprint"), timed_result(a), href)
    } else {
        let name = content
            .paradigm()
            .map(|p| crate::t1("study.games.drill", "item", &p.item_label))
            .unwrap_or_else(|| crate::t("study.games.drill_plain"));
        (name, timed_result(a), crate::url("/play/conjugation"))
    };
    GameRow { game, label: a.label.clone(), result, when: time_ago(a.played_at, at), href, correct: a.correct, total: a.total }
}

/// "8 / 10 right · 1:23", the result line of a timed game.
fn timed_result(a: &crate::games::Activity) -> String {
    crate::i18n::fill(
        &crate::t("study.games.result"),
        &[("correct", &a.correct), ("total", &a.total), ("time", &crate::games::fmt_ms(a.duration_ms))],
    )
}

pub struct UnitGroup {
    /// The section's anchor (`unit-<id>`).
    pub anchor: String,
    /// How many of `lessons` are passed.
    pub passed: usize,
    pub unit: String,
    pub description: String,
    pub lessons: Vec<TopicRow>,
}

/// One tab of a picker (a unit on /lessons, a set on the drill setup, #837):
/// a link, a count and how full its bar is.
pub struct PickTab {
    pub key: String,
    pub title: String,
    pub href: String,
    pub done: usize,
    pub total: usize,
    pub current: bool,
    /// The count in words, for the tab's tooltip.
    pub hint: String,
}

impl PickTab {
    /// The bar's width in percent.
    pub fn pct(&self) -> usize {
        (self.done * 100).checked_div(self.total).unwrap_or(0)
    }
}

/// Page links under a long list (#837). `href` is the list's URL up to
/// where `page=<n>` goes (it ends in `?` or `&`). Empty when one page holds it all.
pub struct Pager {
    pub page: usize,
    pub count: usize,
    pub links: Vec<(usize, String)>,
}

impl Pager {
    /// Page `page` (1-based, clamped) of `len` rows, `per` a page; the rows to show.
    pub fn new(len: usize, per: usize, page: usize, href: &str) -> (Pager, std::ops::Range<usize>) {
        let count = len.div_ceil(per).max(1);
        let page = page.clamp(1, count);
        let links = if count > 1 { (1..=count).map(|n| (n, format!("{href}page={n}"))).collect() } else { Vec::new() };
        let start = (page - 1) * per;
        (Pager { page, count, links }, start..(start + per).min(len))
    }
    pub fn prev(&self) -> &str {
        self.links.get(self.page.wrapping_sub(2)).map(|l| l.1.as_str()).unwrap_or("")
    }
    pub fn next(&self) -> &str {
        self.links.get(self.page).map(|l| l.1.as_str()).unwrap_or("")
    }
}

#[derive(Template)]
#[template(path = "lessons.html")]
pub struct LessonsTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    /// One tab per unit that has lessons.
    pub tabs: Vec<PickTab>,
    /// The units shown: one, or every unit with a match while searching.
    pub groups: Vec<UnitGroup>,
    pub pager: Pager,
    /// The search (`?q=`), or every unit at once (`?unit=all`): no paging.
    pub search: bool,
    pub q: String,
}

/// A level a learner may start a course at (`ContentStore::start_units`),
/// a choice of the course's settings dialog (#587).
pub struct LevelOption {
    /// The unit id, '' for the first level (the beginning).
    pub id: String,
    pub title: String,
    pub description: String,
    pub lessons: usize,
    pub chosen: bool,
}

pub fn level_options(content: &ContentStore, start: usize) -> Vec<LevelOption> {
    let units = content.start_units();
    // Starting at the first level is starting at the beginning.
    let chosen = units.iter().rev().find(|&&u| u <= start).or(units.first()).copied();
    units
        .iter()
        .enumerate()
        .map(|(i, &u)| {
            let unit = content.unit(u);
            LevelOption {
                id: if i == 0 { String::new() } else { unit.id.clone() },
                title: unit.title.clone(),
                description: unit.description.clone(),
                lessons: content.lessons_in(u).count(),
                chosen: Some(u) == chosen,
            }
        })
        .collect()
}

pub struct PrereqRow {
    pub slug: String,
    pub title: String,
    pub status_label: String,
    pub status_css: &'static str,
}

/// A link to a neighbouring lesson (the lesson page's previous / next row).
pub struct LessonLink {
    pub slug: String,
    pub title: String,
}

/// The lessons either side of `lesson` in course order, leaving out the ones
/// the learner skipped by starting at a higher level.
fn neighbours(
    content: &ContentStore,
    lesson: &Lesson,
    progress: &HashMap<String, Progress>,
    start: usize,
) -> (Option<LessonLink>, Option<LessonLink>) {
    let all = content.lessons();
    let Some(i) = all.iter().position(|l| l.id == lesson.id) else { return (None, None) };
    let open = |l: &&Lesson| !crate::quiz::is_skipped(progress.get(&l.id), l, start);
    let link = |l: &Lesson| LessonLink { slug: l.slug.clone(), title: l.title.clone() };
    let prev = all.iter().take(i).rev().find(open).map(link);
    let next = all.iter().skip(i + 1).find(open).map(link);
    (prev, next)
}

/// A paradigm table row for the lesson page: the forms in slot order.
pub struct ConjRow {
    pub item: String,
    pub meaning: String,
    pub set: String,
    pub forms: Vec<String>,
}

/// One role of a conversation on the lesson page (#989): its name from the
/// course and the portrait that speaks it, in the theme worn.
pub struct ConvSpeaker {
    pub name: String,
    /// The built-in picture's id (`#av-<id>` in `avatars.html`).
    pub avatar: &'static str,
    /// The theme's version of that picture, when the theme worn has one.
    pub image: Option<String>,
    /// The `style` of its `.avatar`: the picture's own colour.
    pub style: String,
}

pub struct ConvLine {
    /// Role B says it (drawn on the right).
    pub b: bool,
    pub text: String,
    pub meaning: String,
}

/// A lesson conversation drawn as two characters talking (#989).
pub struct ConvView {
    pub id: String,
    pub title: String,
    pub scenario: String,
    pub a: ConvSpeaker,
    pub b: ConvSpeaker,
    pub lines: Vec<ConvLine>,
    /// Portraits are drawn beside the lines; off with character art
    /// (#1153), each line shows only the speaker's name.
    pub portraits: bool,
}

impl ConvView {
    /// The role that says this line.
    pub fn who(&self, line: &ConvLine) -> &ConvSpeaker {
        if line.b {
            &self.b
        } else {
            &self.a
        }
    }
}

/// The lesson's conversations, each cast with two portraits other than the
/// learner's own (`profile::cast`), as the theme worn draws them.
fn conversation_views(lesson: &Lesson, user: &CurrentUser) -> Vec<ConvView> {
    let theme = user.theme_pack.as_ref().map(|p| &p.theme);
    let speaker = |name: &str, id: &'static str| ConvSpeaker {
        name: name.to_string(),
        avatar: id,
        image: theme.and_then(|t| t.avatar(id)).map(|a| super::art_url(&user.theme, &a.image)),
        style: format!("--av:{}", crate::profile::own_color(id, theme)),
    };
    lesson
        .conversations
        .iter()
        .filter(|c| !c.lines.is_empty())
        .map(|c| {
            let [a, b] = crate::profile::cast(&c.role_a, &c.role_b, &user.avatar);
            ConvView {
                id: c.id.clone(),
                title: c.title.clone(),
                scenario: c.scenario.clone(),
                a: speaker(&c.role_a, a),
                b: speaker(&c.role_b, b),
                lines: c
                    .lines
                    .iter()
                    .map(|l| ConvLine { b: l.speaker == crate::content::Speaker::B, text: l.text.clone(), meaning: l.meaning.clone() })
                    .collect(),
                portraits: user.character_art,
            }
        })
        .collect()
}

#[derive(Template)]
#[template(path = "lesson.html")]
pub struct LessonTemplate<'a> {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub lesson: &'a Lesson,
    pub unit: String,
    pub group: String,
    pub topic: TopicRow,
    /// The lesson was read (or taught) in some mode.
    pub seen: bool,
    pub pass_pct: i64,
    pub required_answers: i64,
    pub prerequisites: Vec<PrereqRow>,
    /// The lessons before and after this one in course order (unit, `order`,
    /// title), passing over the levels below the learner's start; `None` at
    /// the ends.
    pub prev_lesson: Option<LessonLink>,
    pub next_lesson: Option<LessonLink>,
    pub streak: i64,
    pub slot_labels: Vec<String>,
    pub conjugations: Vec<ConjRow>,
    /// The lesson's conversations, as two characters talking (#989).
    pub conversations: Vec<ConvView>,
    /// Questions for the reading check at the bottom (empty once read, or
    /// when the lesson has no quiz).
    pub check: Vec<QuestionView>,
    /// When the check was passed, as "3 days ago"; empty if unread.
    pub read_when: String,
    /// The worn motivator's line for the reading just reported (#568).
    pub reward: Option<String>,
    /// The learner's own note on the lesson ("My note", #336); empty if none.
    pub note: String,
    pub note_max: usize,
}

pub struct HistoryView {
    pub when: String,
    pub lesson_slug: String,
    pub lesson_title: String,
    pub prompt: String,
    pub selected: String,
    pub correct_answer: String,
    pub was_correct: bool,
}

#[derive(Template)]
#[template(path = "history.html")]
pub struct HistoryTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub rows: Vec<HistoryView>,
    pub games: Vec<GameRow>,
}

/// One row of the mistakes list (#338): a quiz question, your last wrong
/// answer and the right one.
pub struct MistakeView {
    pub when: String,
    pub lesson_slug: String,
    pub lesson_title: String,
    pub question_id: String,
    pub prompt: String,
    pub selected: String,
    pub correct_answer: String,
}

#[derive(Template)]
#[template(path = "mistakes.html")]
pub struct MistakesTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub rows: Vec<MistakeView>,
    pub limit: i64,
}

/// Rows the mistakes page shows at most.
const MISTAKES_SHOWN: i64 = 50;

// ---------- handlers ----------

pub async fn load_progress(state: &AppState, user_id: i64, course_id: &str) -> Result<HashMap<String, Progress>, AppError> {
    let cid = course_id.to_string();
    state.db.call(move |c| mastery::load_progress(c, user_id, &cid)).await
}

pub async fn load_passes(state: &AppState, user_id: i64, course_id: &str) -> Result<HashMap<String, LessonPass>, AppError> {
    let cid = course_id.to_string();
    state.db.call(move |c| progress::load_all(c, user_id, &cid)).await
}

pub async fn home(State(state): State<Arc<AppState>>, p: Project, headers: HeaderMap) -> Result<Response, AppError> {
    let at = now();
    let content = &p.course;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let tz = super::sprint::tz_offset_secs(&headers);
    let day_start = crate::sprint::local_day_start(tz, at);
    let vault = state.vault.clone();
    let ai_off = p.user.ai_off;
    let store = p.course.clone();
    let (progress, (streak, practised_today), today, week, words_learned, games_week, tutor_provider, stage_progress, passes, touched, hero, project) = state
        .db
        .call(move |c| {
            let hero = crate::motivators::worn(c, uid, &cid)?.load(c, uid, &store, tz)?;
            let project = crate::courses::project(c, uid, &cid)?;
            let progress = mastery::load_progress(c, uid, &cid)?;
            let streak = crate::sprint::day_streak(c, uid, &cid, tz, at)?;
            let today = Today::load(c, uid, &cid, day_start)?;
            let (week, _) = mastery::stats_since(c, uid, &cid, at - 7 * 86_400)?;
            let words = crate::vocab::count(c, uid, &cid)?;
            let games = crate::games::activity_stats(c, uid, &cid, at - 7 * 86_400)?;
            // The AI tutoring card is not drawn for an account with AI features off.
            let tutor = if ai_off {
                None
            } else {
                crate::secrets::statuses(c, &vault, uid)?.into_iter().find(|k| k.hint.is_some()).map(|k| k.provider.label().to_string())
            };
            let stages = crate::guide::load_all(c, uid, &cid)?;
            let passes = progress::load_all(c, uid, &cid)?;
            let touched = activity::last_touched(c, uid, &cid)?;
            Ok((progress, streak, today, week, words, games.games, tutor, stages, passes, touched, hero, project))
        })
        .await?;
    let resume = activity::resume(content, &passes, &touched, p.start);
    let mut weak_count = 0;
    let mut skipped_count = 0;
    // The unit shown is the one of the lesson to resume at; with every lesson
    // passed, the one mastery says the learner is working on.
    let current = resume.as_ref().map_or_else(|| current_unit(content, &progress, p.start, at), |r| r.lesson.unit_index);
    let (mut unit_passed, mut unit_total, mut passed_count) = (0, 0, 0);
    for lesson in content.lessons() {
        let status = Status::of_lesson(progress.get(&lesson.id), at, lesson, p.start);
        match status {
            Status::NeedsPractice => weak_count += 1,
            Status::Skipped => skipped_count += 1,
            _ => {}
        }
        let passed = passes.get(&lesson.id).is_some_and(LessonPass::passed);
        passed_count += usize::from(passed);
        if lesson.unit_index == current && !matches!(status, Status::Skipped) {
            unit_total += 1;
            unit_passed += usize::from(passed);
        }
    }
    let stages = super::guide::stages(content, &stage_progress, &passes, &touched, p.start);
    let guide_total = stages.iter().filter(|(_, s)| *s != super::guide::StageState::Skipped).count();
    let guide_completed = stages.iter().filter(|(_, s)| *s == super::guide::StageState::Complete).count();
    let guide_next = stages.iter().find(|(c, _)| c.is_current).map(|(c, _)| (c.slug.clone(), c.title.clone()));
    let (tutor_blocked, tutor_gap) = if ai_off {
        (false, None)
    } else {
        let own = tutor_provider.is_some() && super::hosted::own_keys_usable(&state, &p.user);
        let hosted = super::hosted::hosted_offer(&state, uid, crate::metering::Need::Tutor).await?.is_some();
        let blocked = !own && !hosted;
        (blocked, if blocked { super::hosted::NotIncluded::of(&state, &p.user, crate::metering::Need::Tutor).await? } else { None })
    };
    let measures = crate::motivators::Progress {
        today,
        practised_today,
        streak,
        resume: resume.map(|r| (r.lesson.slug.clone(), r.lesson.title.clone(), r.mode.continue_url(&r.lesson.slug))),
        effort_table: activity::table(),
        passed_count,
        total_lessons: content.lessons().len() - skipped_count,
        skipped_count,
        words_learned,
        current_unit: content.unit(current).title.clone(),
        unit_number: current + 1,
        unit_count: content.units().len(),
        unit_passed,
        unit_total,
        unit_percent: if unit_total == 0 { 0 } else { unit_passed * 100 / unit_total },
    };
    let (due_count, due) = super::vocab::due_cards(&state, &p, 3).await?;
    let course = CourseView::of(content);
    let (settings, looks) = match project.filter(|pr| !pr.set_up) {
        Some(pr) => {
            let choices = super::courses::SettingsChoices::load(&state, &p.user).await?;
            (Some(choices.view(&pr, content, true)), choices.looks)
        }
        None => (None, String::new()),
    };
    Ok(Page(HomeTemplate {
        settings,
        looks,
        hero: hero.render(&course, &measures)?,
        course,
        user: Some(p.user),
        progress: measures,
        weak_count,
        week_answered: week,
        games_week,
        tutor_provider,
        tutor_blocked,
        tutor_gap,
        guide_completed,
        guide_total,
        guide_begun: guide_completed > 0 || !stage_progress.is_empty(),
        guide_next,
        due_count,
        due_round: due_count.min(crate::vocab::DUE_CAP as i64),
        due_words: due.into_iter().map(|c| c.term).collect(),
    })
    .into_response())
}

#[derive(serde::Deserialize)]
pub struct ThemeForm {
    #[serde(default)]
    pub theme: String,
    /// Where to go back to: "profile" for the profile page, anything else the home page.
    #[serde(default)]
    pub next: String,
}

fn back_to(next: &str) -> Response {
    Redirect::to(&crate::url(if next == "profile" { "/profile" } else { "/" })).into_response()
}

/// POST /theme/delete: delete a theme the user made. Courses wearing it are
/// back to Default. Someone else's theme, or none, is a 404.
pub async fn delete_theme(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<ThemeForm>) -> Result<Response, AppError> {
    if !state.themes.delete(&state.db, &state.courses, user.id, &form.theme).await? {
        return Err(AppError::NotFound);
    }
    Ok(back_to(&form.next))
}

/// Lessons a page of /lessons lists (#837).
pub const LESSONS_PER_PAGE: usize = 12;

#[derive(serde::Deserialize, Default)]
pub struct LessonsQuery {
    /// A unit id, or `all` for every unit on one page.
    pub unit: Option<String>,
    pub page: Option<String>,
    /// Words to search every unit for (the no-script search; lessons.js filters as you type).
    pub q: Option<String>,
}

fn fold(s: &str) -> String {
    crate::quiz::strip_accents(&s.to_lowercase())
}

/// GET /lessons: one unit at a time, a page of it, with a tab per unit (#837).
/// `?unit=all` and `?q=` list every unit on one page.
pub async fn lessons(State(state): State<Arc<AppState>>, p: Project, Query(lq): Query<LessonsQuery>) -> Result<Response, AppError> {
    let at = now();
    let content = &p.course;
    let progress = load_progress(&state, p.user.id, content.id()).await?;
    let passes = load_passes(&state, p.user.id, content.id()).await?;
    let mut groups = Vec::new();
    let mut ids = Vec::new();
    for (ui, unit) in content.units().iter().enumerate() {
        let lessons: Vec<TopicRow> = content
            .lessons_in(ui)
            .map(|l| TopicRow::new(content, l, progress.get(&l.id), p.start, at).with_pass(passes.get(&l.id)))
            .collect();
        if !lessons.is_empty() {
            let anchor = format!("unit-{}", unit.id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect::<String>());
            let passed = lessons.iter().filter(|t| t.passed).count();
            groups.push(UnitGroup { anchor, passed, unit: unit.title.clone(), description: unit.description.clone(), lessons });
            ids.push(unit.id.clone());
        }
    }
    // Where the learner is: the first lesson neither passed nor skipped.
    let todo = |t: &TopicRow| !t.passed && t.status_css != "skipped";
    let q = lq.q.as_deref().unwrap_or("").trim().chars().take(100).collect::<String>();
    let all = lq.unit.as_deref() == Some("all");
    let chosen = if q.is_empty() && !all {
        lq.unit
            .as_deref()
            .and_then(|u| ids.iter().position(|id| id == u))
            .or_else(|| groups.iter().position(|g| g.lessons.iter().any(todo)))
            .or(if groups.is_empty() { None } else { Some(0) })
    } else {
        None
    };
    let base = crate::url("/lessons");
    let tabs = groups
        .iter()
        .zip(&ids)
        .enumerate()
        .map(|(i, (g, id))| PickTab {
            key: g.anchor.clone(),
            title: g.unit.clone(),
            href: format!("{base}?unit={}", crate::urlenc::urlencode(id)),
            done: g.passed,
            total: g.lessons.len(),
            current: chosen == Some(i),
            hint: crate::t2("study.lessons.jump_passed", "n", g.passed, "total", g.lessons.len()),
        })
        .collect();
    let mut pager = Pager { page: 1, count: 1, links: Vec::new() };
    if !q.is_empty() {
        let words: Vec<String> = fold(&q).split_whitespace().map(str::to_string).collect();
        for g in &mut groups {
            g.lessons.retain(|t| {
                let text = fold(&format!("{} {} {}", t.title, t.group, t.summary));
                words.iter().all(|w| text.contains(w.as_str()))
            });
        }
        groups.retain(|g| !g.lessons.is_empty());
    } else if let Some(i) = chosen {
        let mut g = groups.swap_remove(i);
        // Without ?page=, open the page that holds the next lesson to do.
        let page = match lq.page.as_deref().and_then(|n| n.parse::<usize>().ok()) {
            Some(n) => n,
            None => g.lessons.iter().position(todo).map_or(1, |k| k / LESSONS_PER_PAGE + 1),
        };
        let href = format!("{base}?unit={}&", crate::urlenc::urlencode(ids.get(i).map_or("", String::as_str)));
        let (pg, rows) = Pager::new(g.lessons.len(), LESSONS_PER_PAGE, page, &href);
        g.lessons = g.lessons.drain(rows).collect();
        pager = pg;
        groups = vec![g];
    }
    Ok(Page(LessonsTemplate { course: CourseView::of(content), user: Some(p.user), tabs, groups, pager, search: chosen.is_none(), q }).into_response())
}

#[derive(serde::Deserialize)]
pub struct LessonQuery {
    /// Set by "I've read it" (`POST /lessons/{slug}/read`): when the reading
    /// was reported, so the page can show the reward line (#568).
    pub read: Option<i64>,
}

pub async fn lesson(
    State(state): State<Arc<AppState>>,
    p: Project,
    Path(slug): Path<String>,
    Query(q): Query<LessonQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let content = &p.course;
    let lesson = content.lesson_by_slug(&slug).ok_or(AppError::NotFound)?;
    let at = now();
    let uid = p.user.id;
    let cid = content.id().to_string();
    let lid = lesson.id.clone();
    let tz = super::sprint::tz_offset_secs(&headers);
    // Only a reading reported in the last few minutes still gets its line.
    let read_since = q.read.filter(|t| (at - 600..=at).contains(t));
    let course = Arc::clone(&p.course);
    let (progress, recent, reward) = state
        .db
        .call(move |c| {
            crate::activity::touch(c, uid, &cid, &lid, crate::activity::Mode::Study)?;
            let p = mastery::load_progress(c, uid, &cid)?;
            let r = mastery::recently_correct(c, uid, &cid, 24 * 3600)?;
            let reward = match read_since {
                Some(since) => crate::motivators::reward(c, uid, &course, tz, since, Some(&lid))?,
                None => None,
            };
            Ok((p, r, reward))
        })
        .await?;
    let reward = crate::motivators::reward_html(reward);
    let pr = progress.get(&lesson.id);
    let (uid, cid, lid) = (p.user.id, content.id().to_string(), lesson.id.clone());
    let (pass, note) = state
        .db
        .call(move |c| Ok((progress::load_one(c, uid, &cid, &lid)?, crate::notes::load(c, uid, &cid, &lid)?)))
        .await?;
    let read_when = pr.and_then(|p| p.read_at).map(|t| time_ago(t, at)).unwrap_or_default();
    let check = if read_when.is_empty() {
        let avoid: HashSet<String> = recent.into_iter().collect();
        let mut rng = Rng::from_entropy();
        check_questions(lesson, &avoid, &mut rng)
            .into_iter()
            .map(|item| QuestionView::new(content, lesson, item, &mut rng))
            .collect()
    } else {
        Vec::new()
    };
    let prerequisites = lesson
        .prerequisites
        .iter()
        .filter_map(|id| content.lesson_by_id(id))
        .map(|pre| {
            let s = Status::of_lesson(progress.get(&pre.id), at, pre, p.start);
            PrereqRow { slug: pre.slug.clone(), title: pre.title.clone(), status_label: s.label(), status_css: s.css() }
        })
        .collect();
    let (slot_labels, conjugations) = match content.paradigm() {
        Some(pd) => (
            pd.slots.iter().map(|s| s.label.clone()).collect(),
            lesson
                .conjugations
                .iter()
                .map(|k| ConjRow {
                    item: k.item.clone(),
                    meaning: k.meaning.clone(),
                    set: pd.set_label(&k.set).to_string(),
                    forms: pd.slots.iter().map(|s| k.form(&s.key).to_string()).collect(),
                })
                .collect(),
        ),
        None => (Vec::new(), Vec::new()),
    };

    let (prev_lesson, next_lesson) = neighbours(content, lesson, &progress, p.start);
    let tpl = LessonTemplate {
        course: CourseView::of(content),
        user: Some(p.user.clone()),
        lesson,
        unit: content.unit(lesson.unit_index).title.clone(),
        group: content.group_title(&lesson.group),
        topic: TopicRow::new(content, lesson, pr, p.start, at).with_pass(pass.as_ref()),
        seen: pass.as_ref().is_some_and(LessonPass::seen),
        pass_pct: progress::PASS_PCT,
        required_answers: progress::required_answers(lesson),
        prerequisites,
        prev_lesson,
        next_lesson,
        streak: pr.map(|p| p.streak).unwrap_or(0),
        slot_labels,
        conjugations,
        conversations: conversation_views(lesson, &p.user),
        check,
        read_when,
        reward,
        note,
        note_max: crate::notes::MAX_CHARS,
    };
    // Render inside the handler because the template borrows from the course.
    Ok(axum::response::Html(tpl.render()?).into_response())
}

#[derive(serde::Deserialize)]
pub struct NoteChange {
    pub note: String,
}

/// POST /api/lessons/{slug}/note — save the learner's note on the lesson
/// (static/lesson-note.js autosaves it); an empty note deletes it.
pub async fn save_note(
    State(state): State<Arc<AppState>>,
    p: Project,
    Path(slug): Path<String>,
    axum::Json(change): axum::Json<NoteChange>,
) -> Result<Response, AppError> {
    let lesson = p.course.lesson_by_slug(&slug).ok_or(AppError::NotFound)?;
    if let Some(why) = crate::notes::refusal(&change.note) {
        let body = serde_json::json!({ "errors": { "note": why } });
        return Ok((axum::http::StatusCode::UNPROCESSABLE_ENTITY, axum::Json(body)).into_response());
    }
    let (uid, cid, lid) = (p.user.id, p.course_id().to_string(), lesson.id.clone());
    state.db.call(move |c| crate::notes::save(c, uid, &cid, &lid, &change.note, now())).await?;
    Ok(axum::Json(serde_json::json!({ "ok": true })).into_response())
}

pub async fn history(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let at = now();
    let uid = p.user.id;
    let content = &p.course;
    let cid = content.id().to_string();
    let (rows, games) = state
        .db
        .call(move |c| Ok((mastery::recent_history(c, uid, &cid, 60)?, crate::games::recent_activity(c, uid, &cid, 30)?)))
        .await?;
    let games: Vec<GameRow> = games.iter().map(|a| game_row(a, content, at)).collect();
    let rows = rows
        .into_iter()
        .map(|h| {
            let (title, slug, prompt, answer) = match content.question(&h.question_id) {
                Some((l, q)) => (l.title.clone(), l.slug.clone(), q.prompt.clone(), q.correct_answer.clone()),
                None => match crate::sprint::describe(content, &h.question_id) {
                    Some((l, prompt, answer)) => (l.title.clone(), l.slug.clone(), prompt, answer),
                    None => match content.lesson_by_id(&h.lesson_id) {
                        Some(l) => {
                            let prompt = match h.question_id.strip_prefix(super::progress::AGENT_PREFIX) {
                                Some(asked) => crate::t1("study.history.agent_prompt", "asked", asked),
                                None => h.question_id.clone(),
                            };
                            (l.title.clone(), l.slug.clone(), prompt, String::new())
                        }
                        None => (crate::t("study.history.removed_lesson"), String::new(), h.question_id.clone(), String::new()),
                    },
                },
            };
            HistoryView {
                when: time_ago(h.answered_at, at),
                lesson_slug: slug,
                lesson_title: title,
                prompt,
                selected: crate::sprint::history_answer(content, &h.question_id, &h.selected_answer),
                correct_answer: answer,
                was_correct: h.was_correct,
            }
        })
        .collect();
    Ok(Page(HistoryTemplate { course: CourseView::of(content), user: Some(p.user), rows, games }).into_response())
}

/// The mistakes list (#338): quiz questions whose latest answer was wrong,
/// newest first. Answers that are not a lesson's quiz question (Sprint drills,
/// agent questions, removed lessons) are left out.
pub async fn mistakes(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let at = now();
    let uid = p.user.id;
    let content = &p.course;
    let cid = content.id().to_string();
    let found = state.db.call(move |c| mastery::recent_mistakes(c, uid, &cid, MISTAKES_SHOWN)).await?;
    let rows = found
        .into_iter()
        .filter_map(|h| {
            let (l, q) = content.question(&h.question_id)?;
            Some(MistakeView {
                when: time_ago(h.answered_at, at),
                lesson_slug: l.slug.clone(),
                lesson_title: l.title.clone(),
                question_id: h.question_id,
                prompt: q.prompt.clone(),
                selected: h.selected_answer,
                correct_answer: q.correct_answer.clone(),
            })
        })
        .collect();
    Ok(Page(MistakesTemplate { course: CourseView::of(content), user: Some(p.user), rows, limit: MISTAKES_SHOWN }).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn landing_page_renders_the_builtin_courses() {
        let courses = crate::courses::Courses::load_builtin(Path::new("data")).expect("built-in courses load");
        let rows = landing_courses(&courses);
        assert!(rows.iter().any(|c| c.title == "Spanish" && c.lessons > 0 && c.units > 0));
        let html = LandingTemplate { user: None, courses: rows }.render().expect("landing renders");
        assert!(html.contains("JSON"));
        assert!(html.contains("lessons/greetings.json"));
        assert!(html.contains("/docs/course-format"));
        assert!(html.contains("/register"));
        assert!(html.contains("Spanish"));
    }
}
