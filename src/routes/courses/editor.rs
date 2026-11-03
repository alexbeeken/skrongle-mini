//! The course editor page (`/courses/{id}`) and the views it draws.

use super::*;

pub struct FileRow {
    pub path: String,
    pub bytes: usize,
    pub updated: String,
}

#[derive(Template)]
#[template(path = "course_edit.html")]
pub struct EditorTemplate {
    pub user: Option<CurrentUser>,
    pub id: String,
    pub title: String,
    pub error: Option<String>,
    pub files: Vec<FileRow>,
    pub notice: Option<String>,
    /// 'generating' | 'revising' | 'ready' | 'failed'.
    pub state: &'static str,
    pub private: bool,
    /// "Add content with AI", when the course loads and is free.
    pub ai: Option<AiFormView>,
    /// The last revision, which can be undone.
    pub undo: Option<UndoView>,
    /// Rearrange, remove and add lessons, when the course loads and is free.
    pub arrange: Option<LessonsView>,
    /// The "Add content with AI" job this page follows in its modal.
    pub modal: Option<AiJobModal>,
    /// The modal follows a job that is still running: the page under it is
    /// the usual workspace, inert, with no busy banner or second Cancel.
    pub locked: bool,
    /// "Generate the course": a draft, or a generation that stopped.
    pub generate: Option<GenerateView>,
    /// The latest AI run's changes, in the review modal (`?review=1`).
    pub review: Option<ReviewView>,
    /// Who the course is shared with (the Share dialog, `share.rs`).
    pub shares: Vec<crate::share::Share>,
    /// `?share=1`: open the Share dialog straight away (after sending or removing).
    pub share_open: bool,
    /// The links just made, one per new address, each with a Copy button
    /// (they are stored only hashed: shown this once).
    pub share_links: Vec<ShareLink>,
}

/// An invitation link, for the owner to pass on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareLink {
    pub email: String,
    pub link: String,
}

#[derive(Deserialize)]
pub struct EditorParams {
    pub notice: Option<String>,
    /// `?share=1`: open the Share dialog straight away.
    pub share: Option<String>,
    /// The "Add content with AI" job just started (or cancelled) here: the
    /// modal shows it to its end, even when it finished before the page loaded.
    pub job: Option<String>,
    /// `?generate=1`: open the "Generate the course" dialog straight away.
    pub generate: Option<String>,
    /// "Review AI changes": show the latest AI run's changes in a modal.
    pub review: Option<String>,
}

/// The editor's modal for an "Add content with AI" job (Only add or Allow
/// overall changes): Generating… while it runs, then the error and Close.
/// The page polls `/api/generate/{job}` and reloads with `?job=` when it
/// ends; a job that changed the course opens the review modal instead.
pub struct AiJobModal {
    pub job: String,
    pub status: ai::Status,
    pub course_id: String,
}

impl AiJobModal {
    /// Still drawn as running, and polled: a job not done yet, or an Only
    /// add / Allow changes job done while the row has not caught up (#183;
    /// the reload then opens the review rather than a "done" box).
    pub fn follows(&self) -> bool {
        !self.status.done || (self.status.error.is_none() && self.status.revision)
    }

    /// "running" | "done" | "failed".
    pub fn state(&self) -> &'static str {
        match (self.status.done, self.status.error.is_some()) {
            (false, _) => "running",
            (true, false) => "done",
            (true, true) => "failed",
        }
    }
}

/// The job the editor of `row` shows in its modal: the one named by
/// `?job=` (if it is this user's, on this course, and still in memory), else
/// the one generating or revising the course right now. A generation that
/// succeeded shows none once the row says so: the editor is the result.
/// While the row still says busy, the finished job's modal (Continue) is
/// shown rather than a "being generated" page nothing would refresh.
fn modal_job(state: &AppState, user: &CurrentUser, row: &CourseRow, asked: Option<&str>) -> Option<AiJobModal> {
    if user.ai_off {
        // Hide all AI features: no AI job is followed; the page says the course is busy and offers Cancel.
        return None;
    }
    let live = row.job_id.as_deref().filter(|_| row.is_busy());
    let id = asked.filter(|j| !j.is_empty()).or(live)?;
    let job = state.jobs.get(id).filter(|j| j.user_id == user.id && j.course_id == row.id)?;
    let status = state.jobs.status(&job);
    let finished = status.done && status.error.is_none() && !status.revision && !row.is_busy();
    (!finished).then(|| AiJobModal { job: id.to_string(), status, course_id: row.id.clone() })
}

/// The editor's row and the modal for its job. The generator writes the row
/// (ready or failed) before it marks the job done, so a row read just before
/// that and a snapshot taken just after look like a busy course with a
/// finished job (fixtures finish in milliseconds, so the page the redirect
/// loads can see exactly that, #183): read the row again and decide from it.
async fn settled(
    state: &AppState,
    user: &CurrentUser,
    row: CourseRow,
    asked: Option<&str>,
) -> Result<(CourseRow, Option<AiJobModal>), AppError> {
    let modal = modal_job(state, user, &row, asked);
    if !(row.is_busy() && modal.as_ref().is_some_and(|m| m.status.done)) {
        return Ok((row, modal));
    }
    let row = owned(state, user, &row.id).await?;
    let modal = modal_job(state, user, &row, asked);
    Ok((row, modal))
}

/// "Use AI" (`ai_form.html`), on the editor. It calls the provider of the learner's saved key;
/// without one it only says where to add a key.
pub struct AiFormView {
    pub course_id: String,
    /// The provider whose saved key it uses; `None`: nothing to use.
    pub provider: Option<&'static str>,
    /// Two or more keys are saved: say which one it uses.
    pub named_provider: bool,
    /// "Add and edit" (a revision): a finished course that loads.
    pub can_change: bool,
    /// Offer "Generate the course" (the editor of a draft).
    pub generate: bool,
    /// The chosen mode (#1241): "add", "edit" or "change" (Add and edit).
    pub mode: &'static str,
    /// The Outline switch: new and rewritten lessons are outlines (#1119).
    pub outline: bool,
    pub text: String,
    pub error: Option<String>,
    /// When the latest AI run was, if the course had one: "Review AI changes".
    pub reviewable: Option<String>,
    /// A language course (`Manifest::is_language`): the (i)'s listening and
    /// conversation examples show (#1236).
    pub language: bool,
}

impl AiFormView {
    /// The request box's limit: adding a lesson takes a shorter text than editing.
    pub fn max(&self) -> usize {
        if self.mode == "add" { crate::ai::MAX_BRIEF } else { crate::ai::MAX_FEEDBACK }
    }

    /// The request box's label, for the chosen mode.
    pub fn ask(&self) -> String {
        crate::t(match self.mode {
            "edit" => "courses.ai.ask_edit",
            "change" => "courses.ai.ask_add_or_change",
            _ => "courses.ai.ask_add",
        })
    }

    /// The request box's placeholder, for the chosen mode.
    pub fn placeholder(&self) -> String {
        crate::t(match self.mode {
            "edit" => "courses.ai.placeholder_edit",
            "change" => "courses.ai.placeholder_change",
            _ => "courses.ai.placeholder",
        })
    }
}

/// The review modal (`course_review_modal.html`).
pub struct ReviewView {
    pub course_id: String,
    /// What the run was ("Added with AI: “Fractions”").
    pub label: String,
    pub when: String,
    /// "The AI added 1 lesson and changed 2."
    pub overview: String,
    pub files: Vec<crate::review::ReviewFile>,
    /// "2 changes left out, 1 line edited", or empty.
    pub left_out: String,
    /// Opened by a job that just finished: Close goes on to the lesson table.
    pub after_job: bool,
    /// "Don't show this automatically next time" is ticked (`users.review_quiet`).
    pub quiet: bool,
    /// Accept was refused: why (the boxes stay as they were sent).
    pub error: Option<String>,
    /// The run was undone since (#281): shown read-only, with no − and no Accept.
    pub undone: bool,
    /// `review::labels_json`, for the script that redraws an edited line.
    pub labels: String,
    /// Lessons a fact check read and left alone, with what it was unsure of (#1238).
    pub left_alone: Vec<crate::ai::LessonNote>,
}

/// The Undo and Redo icons (`undo_revision.html`), each greyed out when
/// there is nothing for it to do.
pub struct UndoView {
    pub course_id: String,
    /// The change Undo takes back ("Removed “Simplifying”"), if any.
    pub undo: Option<String>,
    /// Changes before it that can be undone too.
    pub earlier: usize,
    /// The undone change Redo puts back, if any.
    pub redo: Option<String>,
}

/// The lesson table (`lessons_panel.html`): units as header rows holding their
/// lessons in order.
pub struct LessonsView {
    pub course_id: String,
    pub units: Vec<UnitRow>,
    /// Show the unit rows: always, since a unit row's + is how a lesson
    /// is added (false only for a course without units).
    pub folders: bool,
    pub started: bool,
    /// The manifest's language features switch as it stands (`Manifest::is_language`),
    /// for the gear's "Language features" row.
    pub language: bool,
    /// The manifest's labels for the two sides of a vocabulary entry, for
    /// the "Paste a word list" preview's column heads (#453).
    pub terms_label: String,
    pub meanings_label: String,
    /// The manifest's `levels` and whether the course has a placement test
    /// (`placement.json`, #1106), for the gear's Levels and Placement test
    /// rows and the Placement test row under the course settings.
    pub levels: bool,
    pub placement: bool,
    /// The files' fingerprint (`courses::version_of`): the page's saves name
    /// it, so a page that is behind (another tab saved) is refused.
    pub version: String,
    /// The feature usage report modal (`course_report.html`, #328).
    pub report: crate::report::CourseReport,
    /// The course lints shown at the top of that modal (#445).
    pub lints: Vec<crate::lint::Finding>,
    /// Edit with AI on each row (#1242): there only when Use AI has a key to use.
    pub ai: Option<LessonAi>,
}

/// The row's Edit with AI modal (`lessons_ai_dialog.html`, #1242).
pub struct LessonAi {
    /// As `AiFormView`: the provider, which key.
    pub provider: &'static str,
    pub named_provider: bool,
    /// After a refused request: the lesson's file, what was sent and why.
    pub path: String,
    pub text: String,
    pub error: Option<String>,
}

pub struct UnitRow {
    pub id: String,
    pub title: String,
    pub lessons: Vec<LessonRow>,
}

pub struct LessonRow {
    pub id: String,
    pub title: String,
    /// The file, for Edit mode and the JSON drawer; never shown.
    pub path: String,
    /// 1, 2, 3… across the whole course.
    pub position: usize,
}

/// What a refused "Add content with AI" sent, to show it again on the editor.
#[derive(Default)]
pub struct Refill {
    pub text: String,
    pub mode: super::revise::AiMode,
    /// The Outline switch (#1119): lessons come out as outlines.
    pub outline: bool,
    pub error: Option<String>,
    /// The refused request came from Edit with AI on this lesson's row
    /// (#1242): its modal opens again with the text and the error.
    pub lesson: Option<String>,
}

/// The first provider (in `Provider::ALL` order) the server has a key for.
pub(super) async fn saved_provider(state: &AppState) -> Result<Option<crate::secrets::Provider>, AppError> {
    Ok(saved_providers(state).await?.into_iter().next())
}

/// Every provider the server has a key for, in `Provider::ALL` order.
async fn saved_providers(state: &AppState) -> Result<Vec<crate::secrets::Provider>, AppError> {
    let vault = state.vault.clone();
    Ok(state.db.call(move |c| crate::secrets::usable(c, &vault)).await?)
}

/// "Add content with AI" for `row`, when it loads and no job holds it.
async fn ai_view(state: &AppState, user: &CurrentUser, row: &CourseRow, refill: &Refill, locked: bool) -> Result<Option<AiFormView>, AppError> {
    if (row.is_busy() && !locked) || row.error.is_some() || user.ai_off {
        return Ok(None);
    }
    // The server's first key.
    let saved = saved_providers(state).await?;
    let provider = saved.first().map(|p| p.label());
    // Which provider is only worth saying when there was a choice.
    let named_provider = saved.len() > 1;
    // Under a running job's modal the form looks as it did before (inert).
    let can_change = row.can_revise() || (locked && row.error.is_none());
    let cid = row.id.clone();
    let (reviewable, manifest) = state.db.call(move |c| Ok((courses::ai_run_at(c, &cid)?, courses::read_file(c, &cid, "course.json")?))).await?;
    let reviewable = reviewable.map(|t| time_ago(t, now()));
    let language = manifest.and_then(|m| serde_json::from_str::<crate::content::Manifest>(&m).ok()).is_some_and(|m| m.is_language());
    Ok(Some(AiFormView {
        course_id: row.id.clone(),
        provider,
        named_provider,
        can_change,
        generate: false,
        // Add and edit is refused on a course that cannot be revised: Add then.
        mode: if refill.mode == super::revise::AiMode::Change && !can_change { "add" } else { refill.mode.value() },
        outline: refill.outline,
        // A row's Edit with AI shows its own refusal in its own modal.
        text: if refill.lesson.is_some() { String::new() } else { refill.text.clone() },
        error: if refill.lesson.is_some() { None } else { refill.error.clone() },
        reviewable,
        language,
    }))
}

/// The latest AI run on `row`, for the review modal, with what the learner
/// left out of it and edited last time (or, after a refused Accept, what was sent).
pub(super) async fn review_view(state: &AppState, user: &CurrentUser, row: &CourseRow, after_job: bool, refused: Option<(Exclusions, Edits, String)>) -> Result<Option<ReviewView>, AppError> {
    let (cid, uid) = (row.id.clone(), user.id);
    let found = state
        .db
        .call(move |c| {
            let Some(run) = courses::ai_run(c, &cid)? else { return Ok(None) };
            // Lessons are numbered as the course page numbers them now.
            let files = courses::load_files(c, &cid)?;
            let positions: HashMap<String, usize> =
                crate::arrange::lessons_of(&files).map(|(_, l)| l.into_iter().enumerate().map(|(n, l)| (l.path, n + 1)).collect()).unwrap_or_default();
            let stored = (Exclusions::from_json(&run.excluded), Edits::from_json(&run.edits));
            let undone = crate::review::undone(&run, (&stored.0, &stored.1), &files);
            Ok(Some((run, positions, crate::profile::review_quiet(c, uid)?, undone)))
        })
        .await?;
    let Some((run, positions, quiet, undone)) = found else { return Ok(None) };
    let (excluded, edits, error) = match refused {
        Some((x, ed, e)) => (x, ed, Some(e)),
        None => (Exclusions::from_json(&run.excluded), Edits::from_json(&run.edits), None),
    };
    let mut files = crate::review::files(&run, &excluded, &edits, &positions);
    let left_alone = crate::review::attach_notes(&mut files, &run.why);
    if undone {
        crate::review::read_only(&mut files);
    }
    Ok(Some(ReviewView {
        course_id: row.id.clone(),
        overview: crate::review::overview(&files),
        left_out: crate::review::left_out(&files),
        files,
        when: time_ago(run.created_at, now()),
        label: run.label,
        after_job,
        quiet,
        error,
        undone,
        labels: crate::review::labels_json(),
        left_alone,
    }))
}

/// The Undo and Redo icons for `row` (none while a job holds the course).
pub(super) async fn undo_view(state: &AppState, row: &CourseRow, locked: bool) -> Result<Option<UndoView>, AppError> {
    if row.is_busy() && !locked {
        return Ok(None);
    }
    let cid = row.id.clone();
    let (undo, redo) = state.db.call(move |c| Ok((courses::undo_step(c, &cid)?, courses::redo_step(c, &cid)?))).await?;
    Ok(Some(UndoView {
        course_id: row.id.clone(),
        earlier: undo.as_ref().map_or(0, |s| s.steps.saturating_sub(1)),
        undo: undo.map(|s| s.label),
        redo: redo.map(|s| s.label),
    }))
}

/// The lesson table for `row`, when it loads and no job holds it.
/// `locked`: the page is shown under a running job's modal, so the table is
/// drawn (inert) even though the course is busy.
async fn lessons_view(state: &AppState, user: &CurrentUser, row: &CourseRow, locked: bool) -> Result<Option<LessonsView>, AppError> {
    if (row.is_busy() && !locked) || row.error.is_some() {
        return Ok(None);
    }
    let cid = row.id.clone();
    let files = state.db.call(move |c| courses::load_files(c, &cid)).await?;
    let Ok((store, lessons)) = crate::arrange::lessons_of(&files) else { return Ok(None) };
    let version = courses::version_of(&files);
    let (uid, cid) = (user.id, row.id.clone());
    let started = state.db.call(move |c| courses::has_project(c, uid, &cid)).await?;
    let mut position = 0;
    let units: Vec<UnitRow> = store
        .manifest()
        .units
        .iter()
        .map(|u| UnitRow {
            id: u.id.clone(),
            title: u.title.clone(),
            lessons: lessons
                .iter()
                .filter(|l| l.unit() == u.id)
                .map(|l| {
                    position += 1;
                    LessonRow { id: l.id.clone(), title: l.value.get("title").and_then(|t| t.as_str()).unwrap_or(&l.id).to_string(), path: l.path.clone(), position }
                })
                .collect(),
        })
        .collect();
    Ok(Some(LessonsView {
        course_id: row.id.clone(),
        folders: !units.is_empty(),
        units,
        started,
        language: store.manifest().is_language(),
        terms_label: store.manifest().terms.label.clone(),
        meanings_label: store.manifest().meanings.label.clone(),
        levels: store.manifest().levels,
        placement: files.iter().any(|f| f.path == crate::placement::FILE),
        version,
        report: crate::report::feature_usage(&store),
        lints: crate::lint::lint(&store),
        ai: None,
    }))
}

/// Load the course row, refusing anyone but the owner, and the owner too
/// while an owner has it taken down (#967).
pub(in crate::routes) async fn owned(state: &AppState, user: &CurrentUser, id: &str) -> Result<CourseRow, AppError> {
    let cid = id.to_string();
    let row = state.db.call(move |c| courses::course(c, &cid)).await?.ok_or(AppError::NotFound)?;
    if row.owner_id != user.id {
        return Err(AppError::NotFound);
    }
    Ok(row)
}

/// Refuse changes while an AI job is writing or revising the course.
pub(super) fn not_while_generating(row: &CourseRow) -> Result<(), Response> {
    if row.is_busy() {
        return Err(to_editor(&row.id, if row.is_generating() { "generating" } else { "revising" }));
    }
    Ok(())
}

/// Count one write to course files by `user` (`security::COURSE_WRITES_PER_USER`);
/// the refusal when that goes over the limit (#366).
pub(super) fn too_many_writes(state: &AppState, user: &CurrentUser) -> Option<crate::problems::Problem> {
    let limit = crate::security::COURSE_WRITES_PER_USER;
    if state.limits.hit(limit, &user.id.to_string(), now()) {
        return None;
    }
    Some(
        crate::problems::Problem::refused(format!("You have changed courses {} times in {} minutes, the most an account may", limit.max, limit.window / 60))
            .hint("wait a few minutes, then save again"),
    )
}

/// GET /courses/{id}
pub async fn editor(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(q): Query<EditorParams>,
) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    let (row, mut modal) = settled(&state, &user, row, q.job.as_deref()).await?;
    if let Some(to) = stopped_redirect(&row, q.job.is_some()) {
        return Ok(to);
    }
    // A job that has just changed the course opens the review of what it
    // changed in place of its modal; a running or failed one keeps the modal.
    // (A finished first generation has no modal and opens nothing; while the
    // row has not caught up, the modal keeps following the job, #183.)
    let finished = !row.is_busy() && modal.as_ref().is_some_and(|m| m.status.done && m.status.error.is_none());
    let mut notice = match q.notice.as_deref() {
        Some(key) => editor_notice(key, undo_view(&state, &row, false).await?.as_ref(), row.error.as_deref()),
        None => None,
    };
    if finished {
        modal = None;
    }
    // "Don't show this automatically next time" (#187): a finished job says
    // so, and the review waits for its button.
    let uid = user.id;
    let quiet = finished && state.db.call(move |c| crate::profile::review_quiet(c, uid)).await?;
    if quiet && notice.is_none() {
        notice = Some(crate::t("courses.editor.ai_finished_notice"));
    }
    let open = q.generate.is_some_and(|g| g == "1" || g == "true");
    let generate = generate_view(&state, &user, &row, open).await?;
    // Not while a job holds the course: its files are about to change.
    let wanted = q.review.is_some() || (finished && !quiet);
    let review = if wanted && modal.is_none() && !row.is_busy() { review_view(&state, &user, &row, finished, None).await? } else { None };
    let share_open = q.share.is_some_and(|s| s == "1");
    editor_page_with(&state, user, row, notice, Refill::default(), modal, generate, review, share_open).await
}

/// The editor page, with a refused form filled in again.
/// A course whose first generation stopped has no editor: it is its card on
/// the course list, with Resume or Retry. Coming from its job (`?job=`, the
/// progress modal reloading), the list opens that dialog straight away.
pub(super) fn stopped_redirect(row: &CourseRow, from_job: bool) -> Option<Response> {
    (row.state == courses::CourseState::Failed).then(|| {
        let to = if from_job { format!("/courses?generate={}", crate::urlenc::urlencode(&row.id)) } else { "/courses".into() };
        Redirect::to(&crate::url(&to)).into_response()
    })
}

pub(super) async fn editor_page(state: &AppState, user: CurrentUser, row: CourseRow, notice: Option<String>, refill: Refill) -> Result<Response, AppError> {
    let (row, modal) = settled(state, &user, row, None).await?;
    let generate = generate_view(state, &user, &row, false).await?;
    editor_page_with(state, user, row, notice, refill, modal, generate, None, false).await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn editor_page_with(
    state: &AppState,
    user: CurrentUser,
    row: CourseRow,
    notice: Option<String>,
    refill: Refill,
    modal: Option<AiJobModal>,
    generate: Option<GenerateView>,
    review: Option<ReviewView>,
    share_open: bool,
) -> Result<Response, AppError> {
    Ok(Page(editor_template(state, user, row, notice, refill, modal, generate, review, share_open).await?).into_response())
}

/// The Share dialog open after inviting:
/// the course page with `notice` (a `share-made-…` key) and the links made.
pub(in crate::routes) async fn share_links_page(state: &AppState, user: CurrentUser, id: &str, notice: &str, links: Vec<ShareLink>) -> Result<Response, AppError> {
    let row = owned(state, &user, id).await?;
    let (row, modal) = settled(state, &user, row, None).await?;
    let generate = generate_view(state, &user, &row, false).await?;
    let notice = editor_notice(notice, None, None);
    let mut t = editor_template(state, user, row, notice, Refill::default(), modal, generate, None, true).await?;
    t.share_links = links;
    Ok(Page(t).into_response())
}

#[allow(clippy::too_many_arguments)]
async fn editor_template(
    state: &AppState,
    user: CurrentUser,
    row: CourseRow,
    notice: Option<String>,
    refill: Refill,
    modal: Option<AiJobModal>,
    generate: Option<GenerateView>,
    review: Option<ReviewView>,
    share_open: bool,
) -> Result<EditorTemplate, AppError> {
    let id = row.id.clone();
    let cid = id.clone();
    let files = state.db.call(move |c| courses::list_files(c, &cid)).await?;
    let at = now();
    // A running job's modal covers the page: the workspace as it was, inert.
    let locked = row.is_busy() && modal.is_some();
    let mut ai = ai_view(state, &user, &row, &refill, locked).await?;
    if let Some(af) = ai.as_mut() {
        af.generate = generate.is_some();
    }
    let undo = undo_view(state, &row, locked).await?;
    let mut arrange = lessons_view(state, &user, &row, locked).await?;
    if let (Some(lp), Some(af)) = (arrange.as_mut(), ai.as_ref()) {
        lp.ai = af.provider.map(|provider| LessonAi {
            provider,
            named_provider: af.named_provider,
            path: refill.lesson.clone().unwrap_or_default(),
            text: if refill.lesson.is_some() { refill.text.clone() } else { String::new() },
            error: refill.lesson.as_ref().and(refill.error.clone()),
        });
    }
    let sid = row.id.clone();
    let shares = state.db.call(move |c| crate::share::shares_of(c, &sid)).await?;
    Ok(EditorTemplate {
        state: row.state.as_str(),
        private: row.visibility != "public",
        user: Some(user),
        id,
        title: row.title,
        error: row.error,
        files: files.into_iter().map(|(path, bytes, t)| FileRow { path, bytes, updated: time_ago(t, at) }).collect(),
        notice,
        ai,
        undo,
        arrange,
        locked,
        modal,
        generate,
        review,
        shares,
        share_open,
        share_links: Vec::new(),
    })
}
