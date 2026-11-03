//! The course list (`/courses`): its cards, their notices, and the editor's notices.

use super::*;

// ---------- picker ----------

pub struct CourseCard {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub description: String,
    pub lessons: usize,
    pub questions: usize,
    pub terms: usize,
    pub units: usize,
    /// Lessons passed / mastered, when the user has a project here.
    pub progress: Option<(usize, usize)>,
    pub last_opened: Option<String>,
    pub is_active: bool,
    pub editable: bool,
    pub error: Option<String>,
    /// 'generating' | 'ready' | 'failed' (built-in courses are 'ready').
    pub state: &'static str,
    /// The generation job (for live progress), while it is in memory.
    pub job: Option<String>,
    /// While generating: what the job is doing and how far it got.
    pub phase: String,
    pub pct: usize,
    /// A user-made course: only its owner can see it.
    pub private: bool,
    /// Its first generation stopped: the card shows the error with Resume
    /// (the outline is saved) or Retry, and the course has no editor.
    pub stopped: bool,
    /// Stopped with its outline saved: Resume writes the missing lessons.
    pub resumable: bool,
    /// Shared with this learner by someone else (`share.rs`): who. The card
    /// opens the course but has no editor.
    pub shared_by: Option<String>,
    /// A course the learner opened: what its settings dialog shows (#565).
    pub settings: Option<CourseSettings>,
    /// It can be copied (#629): a built-in course, or one of their own that
    /// loads and is not being generated. The copy icon sits in the course's
    /// settings dialog (`settings.copyable`), not on the card, and so does
    /// the editor's pencil (`editable`).
    pub copyable: bool,
    /// The language its explanations are in (`meanings.lang`): "Copy and
    /// translate to…" offers every other app language (#630).
    pub meanings_lang: String,
    /// A copy being translated, or whose translation stopped (#630): its
    /// card resumes the translation rather than a generation.
    pub translation: bool,
}

/// A course's settings dialog (`course_settings.html`): the level (a course
/// with levels) and the motivator worn in the course. On the course list it is
/// one form behind the card's gear; before the course is first opened it is
/// a wizard, one step a page (#574): behind the card's Start button (#587),
/// or on the course home when the course was opened another way.
pub struct CourseSettings {
    pub course_id: String,
    pub course_title: String,
    /// The first visit's wizard (steps, "Get Started!") rather than the form.
    pub wizard: bool,
    /// The wizard behind a Start button on the course list (#587): "Get
    /// Started!" opens the course (`POST /courses/open/{id}`) and closing it
    /// opens nothing. Otherwise the wizard is on the home page of a course
    /// already opened, and closing it counts the course as set up.
    pub start: bool,
    /// Every motivator the app has, the one worn marked.
    pub motivators: Vec<MotivatorView>,
    /// The levels the course can be started at, the one chosen marked;
    /// empty for a course without levels (#587, was the `/start` page).
    pub levels: Vec<super::pages::LevelOption>,
    /// The course's placement test is on offer beside the levels (#1106).
    pub placement: bool,
    /// A copy icon at the top right (#629): on the course list, for a course
    /// the learner may copy (`CourseCard::copyable`). Not on the home page.
    pub copyable: bool,
    /// A pencil icon beside it: the course editor, for a course of the
    /// learner's own (`CourseCard::editable`). Not on the home page.
    pub editable: bool,
    /// "Copy and translate to…" left of the copy icon (#630): on the course
    /// list, for a course the learner may copy, unless AI is off. Its dialog
    /// offers every app language but `meanings_lang` and the providers.
    pub translate: bool,
    pub meanings_lang: String,
    pub translate_providers: Vec<ProviderChoice>,
}

/// A motivator on a course's settings chips (#574).
pub struct MotivatorView {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: String,
    pub about: String,
    pub active: bool,
}

impl MotivatorView {
    fn all(worn: &str) -> Vec<MotivatorView> {
        let worn = crate::motivators::get(worn).id();
        crate::motivators::ALL
            .iter()
            .map(|m| MotivatorView { id: m.id(), icon: m.icon(), title: m.title(), about: m.about(), active: m.id() == worn })
            .collect()
    }
}

/// The settings dialog of the course `pr` (`content`).
pub(in crate::routes) fn settings_view(pr: &courses::ProjectRow, content: &ContentStore, wizard: bool) -> CourseSettings {
    let levels = super::pages::level_options(content, content.start_index(&pr.start_unit));
    settings_view_with(pr, content.title(), levels, crate::placement::offered(content), wizard)
}

/// `settings_view` for a built-in course from its summary, without building it (#32).
fn settings_view_builtin(pr: &courses::ProjectRow, course: &courses::BuiltinCourse, wizard: bool) -> CourseSettings {
    let levels = super::pages::level_options_of(course.units(), course.levels(), course.start_index(&pr.start_unit));
    settings_view_with(pr, course.title(), levels, course.placement_offered(), wizard)
}

fn settings_view_with(pr: &courses::ProjectRow, title: &str, levels: Vec<super::pages::LevelOption>, placement: bool, wizard: bool) -> CourseSettings {
    CourseSettings {
        course_id: pr.course_id.clone(),
        course_title: title.to_string(),
        wizard,
        start: false,
        motivators: MotivatorView::all(&pr.motivator),
        levels,
        placement,
        copyable: false,
        editable: false,
        translate: false,
        meanings_lang: String::new(),
        translate_providers: Vec::new(),
    }
}

#[derive(Template)]
#[template(path = "courses.html")]
pub struct PickerTemplate {
    pub user: Option<CurrentUser>,
    pub projects: Vec<CourseCard>,
    pub builtin: Vec<CourseCard>,
    pub mine: Vec<CourseCard>,
    /// Courses others shared with this learner and not opened yet.
    pub shared: Vec<CourseCard>,
    pub notice: Option<String>,
    /// `?generate=<id>`: the Generate dialog of a stopped course (Retry or Resume).
    pub generate: Option<GenerateView>,
    /// The AI "Copy and translate to…" can use (#630): the providers with a
    /// saved key. Empty with AI off.
    pub translate_providers: Vec<ProviderChoice>,
}

#[derive(Deserialize)]
pub struct PickerParams {
    pub notice: Option<String>,
    pub generate: Option<String>,
}

fn card(content: &ContentStore, editable: bool) -> CourseCard {
    card_of(content.manifest(), content.totals(), editable)
}

/// `card` from a course's manifest and totals: a built-in course's summary
/// makes one without being built (#32).
fn card_of(m: &crate::content::Manifest, (lessons, questions, terms): (usize, usize, usize), editable: bool) -> CourseCard {
    CourseCard {
        id: m.id.clone(),
        title: m.title.clone(),
        icon: m.icon().to_string(),
        description: m.description.clone(),
        lessons,
        questions,
        terms,
        units: m.units.len(),
        progress: None,
        last_opened: None,
        is_active: false,
        editable,
        error: None,
        state: "ready",
        job: None,
        phase: String::new(),
        pct: 0,
        private: editable,
        stopped: false,
        resumable: false,
        shared_by: None,
        settings: None,
        copyable: false,
        meanings_lang: m.meanings.lang.clone(),
        translation: false,
    }
}

impl CourseCard {
    /// The card's progress chips (#684), one per metric, when the learner
    /// has a project here: lessons passed, lessons mastered.
    pub fn stats(&self) -> Vec<CardStat> {
        let Some((passed, mastered)) = self.progress else { return Vec::new() };
        [("passed", passed), ("mastered", mastered)]
            .into_iter()
            .map(|(metric, n)| {
                let name = crate::t(&format!("courses.picker.stat_{metric}"));
                let total = self.lessons;
                let value = crate::t2("courses.picker.stat_of", "n", n, "total", total);
                let label = crate::t2("courses.picker.stat_label", "name", &name, "value", &value);
                let share = if total == 0 { 0 } else { (n * 100 / total).min(100) };
                let info_label = crate::t(&format!("courses.stat.info_label_{metric}"));
                CardStat { metric, n, total, share, name, value, label, info_label }
            })
            .collect()
    }
}

/// One progress chip on a course card and the modal it opens (#684).
pub struct CardStat {
    /// `data-metric`: "passed" or "mastered".
    pub metric: &'static str,
    pub n: usize,
    pub total: usize,
    /// n as a percentage of total: how far the chip's tint has moved to gold.
    pub share: usize,
    /// The metric's full name ("Lessons passed").
    pub name: String,
    /// "3 of 12".
    pub value: String,
    /// "Lessons passed: 3 of 12", for the tooltip and screen readers.
    pub label: String,
    /// The (i) in its modal (#900): "What counts as passed".
    pub info_label: String,
}

/// A user course that does not load: being generated, stopped, or broken.
fn broken_card(state: &AppState, row: &CourseRow) -> CourseCard {
    let job = row.job_id.as_deref().and_then(|j| state.jobs.get(j));
    let (phase, pct) = match (&job, row.state) {
        (Some(j), courses::CourseState::Generating) => {
            let s = state.jobs.status(j);
            let pct = if s.lessons_total > 0 { s.lessons_done * 100 / s.lessons_total } else { 0 };
            (crate::i18n::fill(&crate::t("courses.picker.phase_progress"), &[("phase", &s.phase), ("done", &s.lessons_done), ("total", &s.lessons_total)]), pct)
        }
        (None, courses::CourseState::Generating) => (crate::t("courses.picker.starting"), 0),
        _ => (String::new(), 0),
    };
    CourseCard {
        id: row.id.clone(),
        title: row.title.clone(),
        icon: match row.state {
            courses::CourseState::Generating => "⏳".into(),
            _ if row.source == "translate" => "🌐".into(),
            _ if row.source == "ai" => "✨".into(),
            _ => "🛠️".into(),
        },
        state: row.state.as_str(),
        // Only a card still generating follows its job: a stopped one whose
        // job is still in memory (six hours) would see "done" and reload the
        // page every few seconds, under the dialog open on it (#81).
        job: row.job_id.clone().filter(|_| job.is_some() && row.state == courses::CourseState::Generating),
        phase,
        pct,
        private: true,
        stopped: row.state == courses::CourseState::Failed,
        resumable: row.state == courses::CourseState::Failed && row.outline.is_some(),
        description: String::new(),
        lessons: 0,
        questions: 0,
        terms: 0,
        units: 0,
        progress: None,
        last_opened: None,
        is_active: false,
        editable: true,
        error: row.error.clone(),
        shared_by: None,
        settings: None,
        copyable: false,
        meanings_lang: String::new(),
        translation: row.source == "translate",
    }
}

/// GET /courses[?generate=<stopped course>]
pub async fn picker(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Query(q): Query<PickerParams>,
) -> Result<Response, AppError> {
    let generate = match q.generate.filter(|g| !g.is_empty()) {
        Some(id) => match owned(&state, &user, &id).await {
            Ok(row) if row.state == courses::CourseState::Failed => generate_view(&state, &user, &row, true).await?,
            _ => None,
        },
        None => None,
    };
    let notice = q.notice.as_deref().and_then(picker_notice);
    picker_page(&state, user, notice, generate).await
}

/// The course list's `?notice=` keys and their text (#385): the URL names a
/// message, it never carries one, so a link cannot put its own words in the
/// app's status alert. An unknown key shows nothing.
pub(super) fn picker_notice(key: &str) -> Option<String> {
    Some(crate::t(match key {
        "title-too-long" => "courses.picker.title_too_long",
        "course-deleted" => "courses.picker.deleted_notice",
        "dev-reset" => "courses.picker.dev_reset",
        "share-taken" => "courses.share.link_taken",
        "share-gone" => "courses.share.link_gone",
        "share-not-ready" => "courses.share.not_ready",
        "too-many-writes" => return Some(too_many_writes_text()),
        "copy-not-ready" => "courses.picker.copy_not_ready",
        "translate-same" => "courses.translate.same_language",
        "translate-no-plan" => "courses.translate.no_plan",
        _ => {
            if let Some(bytes) = key.strip_prefix("copy-too-big-") {
                return Some(crate::t1("courses.picker.copy_too_big", "max", courses::megabytes(bytes.parse::<usize>().ok()?)));
            }
            if let Some(bytes) = key.strip_prefix("copy-no-room-") {
                return Some(crate::t1("courses.picker.copy_no_room", "max", courses::megabytes(bytes.parse::<usize>().ok()?)));
            }
            let max = key.strip_prefix("course-count-")?.parse::<usize>().ok()?;
            return Some(crate::t1("courses.picker.course_count", "max", max));
        }
    }))
}

/// The notice for [`too_many_writes`] (#366), on the course list and in the editor.
fn too_many_writes_text() -> String {
    let limit = crate::security::COURSE_WRITES_PER_USER;
    crate::t2("courses.editor.too_many_writes", "max", limit.max, "minutes", limit.window / 60)
}

/// The editor's `?notice=` keys and their text (#385), as `picker_notice`.
/// What a text names comes from the server, never the URL: `undone` and
/// `redone` name the change from the Undo/Redo buttons' labels and a
/// `-broken` notice the course's own load error, both from the database; the
/// Share dialog's counts are digits in the key (`share-sent-2-1`).
pub(super) fn editor_notice(key: &str, undo: Option<&UndoView>, error: Option<&str>) -> Option<String> {
    let redo_label = undo.and_then(|u| u.redo.as_deref()).unwrap_or_default();
    let undo_label = undo.and_then(|u| u.undo.as_deref()).unwrap_or_default();
    let error = error.unwrap_or_default();
    if let Some(rest) = key.strip_prefix("accepted-") {
        let (done, end) = rest.rsplit_once('-')?;
        let done = crate::t(match done {
            "again" => "courses.review.done_in_again",
            "in" => "courses.review.done_in",
            "left" => "courses.review.done_in_without",
            "edits" => "courses.review.done_in_edited",
            "edits-left" => "courses.review.done_in_edited_without",
            _ => return None,
        });
        return Some(match end {
            "kept" => crate::t1("courses.review.notice_done", "done", done),
            "undo" => crate::t1("courses.review.notice_undo", "done", done),
            "broken" => crate::t2("courses.review.notice_broken", "done", done, "error", error),
            _ => return None,
        });
    }
    if let Some((made, counts)) = key.strip_prefix("share-sent-").map(|c| (false, c)).or_else(|| key.strip_prefix("share-made-").map(|c| (true, c))) {
        let (sent, already) = counts.split_once('-')?;
        let (sent, already) = (sent.parse::<u32>().ok()?, already.parse::<u32>().ok()?);
        let mut text = if sent == 0 { String::new() } else { crate::tn(if made { "courses.share.made" } else { "courses.share.sent" }, sent) };
        if already > 0 {
            text = format!("{text} {}", crate::t1("courses.share.already_had", "n", already)).trim().to_string();
        }
        return (!text.is_empty()).then_some(text);
    }
    Some(match key {
        "generating" => crate::t("courses.editor.locked_generating"),
        "revising" => crate::t("courses.editor.locked_revising"),
        "saved" => crate::t("courses.file.saved_notice"),
        "saved-broken" => crate::t1("courses.file.saved_broken_notice", "error", error),
        "file-deleted" => crate::t("courses.file.deleted_notice"),
        "file-deleted-broken" => crate::t1("courses.file.deleted_broken_notice", "error", error),
        "loads" => crate::t("courses.editor.loads_notice"),
        "broken" => crate::t1("courses.editor.not_loading_notice", "error", error),
        "busy" => crate::t("courses.generate.busy"),
        "has-content" => crate::t("courses.generate.has_content"),
        "cancel-revision" => crate::t("courses.generate.cancelling_revision"),
        "cancel-generation" => crate::t("courses.generate.cancelling_generation"),
        "no-undo" => crate::t("courses.undo.nothing_to_undo"),
        "no-redo" => crate::t("courses.undo.nothing_to_redo"),
        "undone" => crate::t1("courses.undo.undone", "label", redo_label),
        "undone-broken" => crate::t2("courses.undo.undone_broken", "label", redo_label, "error", error),
        "redone" => crate::t1("courses.undo.redone", "label", undo_label),
        "redone-broken" => crate::t2("courses.undo.redone_broken", "label", undo_label, "error", error),
        "share-own" => crate::t("courses.share.own_address"),
        "share-full" => crate::t1("courses.share.too_many_course", "max", crate::share::MAX_PER_COURSE),
        "share-none" => crate::t("courses.share.at_least_one"),
        "share-many" => crate::t1("courses.share.too_many_request", "max", crate::share::MAX_PER_REQUEST),
        "share-bad" => crate::t("courses.share.bad_address_notice"),
        "share-removed" => crate::t("courses.share.removed_notice"),
        "share-not-on-list" => crate::t("courses.share.not_on_list"),
        "too-many-writes" => too_many_writes_text(),
        _ => return None,
    })
}

/// The editor at `/courses/{id}` with the notice `key`.
pub(in crate::routes) fn to_editor(id: &str, key: &str) -> Response {
    Redirect::to(&crate::url(&format!("/courses/{id}?notice={key}"))).into_response()
}

pub(super) async fn picker_page(state: &AppState, user: CurrentUser, notice: Option<String>, generate: Option<GenerateView>) -> Result<Response, AppError> {
    let at = now();
    let uid = user.id;
    let (projects, mine, shared, summary, fresh, passed) = state
        .db
        .call(move |c| {
            Ok((
                courses::projects_of(c, uid)?,
                courses::courses_owned_by(c, uid)?,
                crate::share::shared_with(c, uid)?,
                crate::mastery::summary_by_course(c, uid, at)?,
                courses::fresh_project(c, uid, "")?,
                crate::progress::passed_by_course(c, uid)?,
            ))
        })
        .await?;
    let translate_providers = if user.ai_off { Vec::new() } else { offered_providers(state, &user).await? };
    // "Copy and translate to…" beside a settings dialog's copy icon (#630).
    let translate = |st: &mut CourseSettings, c: &CourseCard| {
        st.translate = c.copyable && !user.ai_off;
        st.meanings_lang = c.meanings_lang.clone();
        st.translate_providers = translate_providers.clone();
    };
    // A course not opened yet: its Start button opens the settings wizard (#587).
    let fresh_for = |id: &str| courses::ProjectRow { course_id: id.to_string(), ..fresh.clone() };
    let with_wizard = |mut st: CourseSettings, mut c: CourseCard| {
        st.start = true;
        st.copyable = c.copyable;
        st.editable = c.editable;
        translate(&mut st, &c);
        c.settings = Some(st);
        c
    };

    let mut project_cards = Vec::new();
    for pr in &projects {
        let owned = mine.iter().find(|r| r.id == pr.course_id);
        let shared_by = shared.iter().find(|s| s.course_id == pr.course_id).map(|s| s.owner.clone());
        // A made course is on the list for its owner and for whoever it is shared with, nobody else.
        if !state.courses.is_builtin(&pr.course_id) && owned.is_none() && shared_by.is_none() {
            continue;
        }
        // A built-in course's card and dialog come from its summary: listing
        // it does not build it (#32).
        let (mut c, mut settings) = match state.courses.builtin_by_id(&pr.course_id) {
            Some(b) => (card_of(b.manifest(), b.totals(), false), settings_view_builtin(pr, b, false)),
            None => {
                let Some(content) = state.courses.get(&state.db, &pr.course_id).await? else { continue };
                (card(&content, owned.is_some()), settings_view(pr, &content, false))
            }
        };
        c.copyable = owned.map_or(shared_by.is_none(), |r| !matches!(r.state, courses::CourseState::Generating | courses::CourseState::Failed));
        c.shared_by = shared_by;
        if let Some(row) = owned {
            c.state = row.state.as_str(); // "revising" shows as a tag; the course still opens
        }
        let mastered = summary.get(&pr.course_id).map_or(0, |s| s.1);
        c.progress = Some((passed.get(&pr.course_id).copied().unwrap_or(0), mastered));
        c.last_opened = Some(time_ago(pr.last_opened_at, at));
        c.is_active = user.course_id.as_deref() == Some(pr.course_id.as_str());
        settings.copyable = c.copyable;
        settings.editable = c.editable;
        translate(&mut settings, &c);
        c.settings = Some(settings);
        project_cards.push(c);
    }
    let builtin: Vec<CourseCard> = state
        .courses
        .builtin()
        .iter()
        .filter(|c| !projects.iter().any(|p| p.course_id == c.id()))
        .map(|c| with_wizard(settings_view_builtin(&fresh_for(c.id()), c, true), CourseCard { copyable: true, ..card_of(c.manifest(), c.totals(), false) }))
        .collect();
    let mut my_cards = Vec::new();
    for row in &mine {
        // A course being revised opens as it is, like a ready one.
        let opens = matches!(row.state, courses::CourseState::Ready | courses::CourseState::Revising);
        if projects.iter().any(|p| p.course_id == row.id) && row.error.is_none() && opens {
            continue;
        }
        if !opens {
            my_cards.push(broken_card(state, row));
            continue;
        }
        match state.courses.get(&state.db, &row.id).await? {
            Some(content) if row.error.is_none() => {
                let mut c = card(&content, true);
                c.state = row.state.as_str();
                c.copyable = true;
                my_cards.push(with_wizard(settings_view(&fresh_for(content.id()), &content, true), c))
            }
            _ => my_cards.push(broken_card(state, row)),
        }
    }
    // Shared with this learner and not opened yet: a card of its own, if it loads.
    let mut shared_cards = Vec::new();
    for s in &shared {
        if projects.iter().any(|p| p.course_id == s.course_id) {
            continue;
        }
        let Some(content) = state.courses.get(&state.db, &s.course_id).await? else { continue };
        let mut c = card(&content, false);
        c.shared_by = Some(s.owner.clone());
        shared_cards.push(with_wizard(settings_view(&fresh_for(content.id()), &content, true), c));
    }
    Ok(Page(PickerTemplate { user: Some(user), projects: project_cards, builtin, mine: my_cards, shared: shared_cards, notice, generate, translate_providers }).into_response())
}
