//! "Add content with AI", the review of an AI run, Undo and Redo.

use super::*;

/// Without a saved key, "Add content with AI" says this instead of the form.
fn no_key() -> String {
    crate::t("courses.ai.no_key")
}

#[derive(Deserialize)]
pub struct AiForm {
    #[serde(default)]
    pub text: String,
    /// "add" (Only add), "change" (Allow overall changes) or "edit" (Edit
    /// lessons, #1240: rewrite lessons, add, remove or move none).
    #[serde(default)]
    pub mode: String,
    /// "outline": the lessons it writes are outlines (#1119).
    #[serde(default)]
    pub depth: String,
    /// "Edit with AI" on a lesson's row (#1242): the lesson file, the only
    /// one the edit may change. Implies `mode=edit`.
    #[serde(default)]
    pub lesson: String,
}

/// What a Use AI run does, from `AiForm::mode` (anything else is Only add).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AiMode {
    #[default]
    Add,
    Change,
    Edit,
}

impl AiMode {
    pub(super) fn parse(mode: &str) -> Self {
        match mode {
            "change" => AiMode::Change,
            "edit" => AiMode::Edit,
            _ => AiMode::Add,
        }
    }

    /// The form value: the inverse of `parse`.
    pub(super) fn value(self) -> &'static str {
        match self {
            AiMode::Add => "add",
            AiMode::Change => "change",
            AiMode::Edit => "edit",
        }
    }
}

/// POST /courses/{id}/ai {text, mode}: "Add content with AI", with the
/// learner's saved key. Only add (`mode=add`) has the AI write one new lesson
/// and place it, and the result is refused if anything already there changed
/// (`arrange::only_adds`); Allow overall changes (`mode=change`, AI courses)
/// revises the course from the text; Edit lessons (`mode=edit`, any course
/// that loads) rewrites lessons and is refused if it added, removed or moved
/// one (`arrange::only_edits`), always as full lessons; with `lesson` (Edit with AI on a row,
/// #1242) only that lesson, in one request (`arrange::only_edits_one`). Each runs as a job that the editor's
/// modal follows (`?job=`); the course keeps working and changes only if it
/// succeeds.
pub async fn ai_start(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Form(form): Form<AiForm>,
) -> Result<Response, AppError> {
    user.require_ai()?;
    let row = owned(&state, &user, &id).await?;
    if let Err(r) = not_while_generating(&row) {
        return Ok(r);
    }
    let lesson_path = Some(form.lesson.trim().to_string()).filter(|p| !p.is_empty());
    let mode = if lesson_path.is_some() { AiMode::Edit } else { AiMode::parse(&form.mode) };
    let change = mode == AiMode::Change;
    // An outline of a lesson that exists would throw its content away.
    let depth = if form.depth == "outline" && mode != AiMode::Edit { ai::Depth::Outline } else { ai::Depth::Full };
    let outline = depth == ai::Depth::Outline;
    let text = form.text.replace("\r\n", "\n").trim().to_string();
    let lesson_refill = lesson_path.clone();
    let refuse = |msg: String| editor_page(&state, user.clone(), row.clone(), None, Refill { text: text.clone(), mode, outline, error: Some(msg), ai_out: None, lesson: lesson_refill.clone() });
    if row.error.is_some() {
        return refuse(crate::t("courses.ai.not_loading")).await;
    }
    if change && !row.can_revise() {
        return refuse(crate::t("courses.ai.only_finished")).await;
    }
    if let Some(problem) = ai_text_problem(&text, mode) {
        return refuse(problem).await;
    }
    // Edit with AI on a row: that lesson, with the title the job's progress names.
    let lesson = match &lesson_path {
        Some(path) => {
            let cid = id.clone();
            let files = state.db.call(move |c| courses::load_files(c, &cid)).await?;
            let found = crate::arrange::lessons_of(&files).ok().and_then(|(_, ls)| ls.into_iter().find(|l| &l.path == path));
            match found {
                Some(l) => Some((l.path.clone(), l.value.get("title").and_then(|t| t.as_str()).unwrap_or(&l.id).to_string())),
                None => return refuse(crate::t("courses.ai.lesson_gone")).await,
            }
        }
        None => None,
    };
    // The first saved key, else the server's key when the plan includes AI;
    // on Plus the server's key first (`hosted::hosted_first`), and the saved
    // keys only with "use my keys when my Skrongle AI limit is hit" on.
    let saved = if crate::routes::hosted::own_keys_usable(&state, &user) { saved_provider(&state, user.id).await? } else { None };
    let hosted_first = crate::routes::hosted::hosted_first(&state, &user);
    let purpose = if lesson.is_some() { "edit-lesson" } else { "revise-lesson" };
    let hosted = if saved.is_none() || hosted_first { crate::routes::hosted::hosted_provider(&state, user.id, purpose, Need::Lesson).await? } else { None };
    let mut provider = match (saved, hosted) {
        (_, Some(hosted)) if hosted_first => hosted,
        (Some(saved), _) => match ai_provider(&state, &user, saved.id(), "", false, "", "").await? {
            Ok(p) => p,
            Err(e) => return refuse(e).await,
        },
        // No key saved: the server's key, when the plan includes AI.
        (None, Some(hosted)) => hosted,
        (None, None) => return refuse(no_key()).await,
    };
    if let Err(e) = ai::check_capacity(&state.jobs, user.id) {
        return refuse(e).await;
    }
    if let Some(e) = ai_quota_problem(&state, user.id) {
        return refuse(e).await;
    }
    let job = ai::new_job_id();
    if provider.source == crate::metering::KeySource::Hosted {
        // A revision or an edit may rewrite every lesson after its plan; an added or row-edited lesson is one request.
        let cid = id.clone();
        let one = mode == AiMode::Add || lesson.is_some();
        let lessons = if !one {
            state
                .db
                .call(move |c| c.query_row("SELECT COUNT(*) FROM course_files WHERE course_id = ?1 AND path LIKE 'lessons/%'", [cid], |r| r.get::<_, i64>(0)))
                .await? as usize
        } else {
            1
        };
        if let Err(r) = reserve_hosted(&state, user.id, &job, lessons, !one, 0, None).await? {
            match fallback_key(&state, &user, &r).await? {
                Some(own) => provider = own,
                None => {
                    let out = Some(crate::routes::hosted::OutOfAi::of(&state, user.plan));
                    let msg = r.message(state.meter.credits_offered());
                    return editor_page(&state, user.clone(), row.clone(), None, Refill { text, mode, outline, error: Some(msg), ai_out: out, lesson: lesson_path }).await;
                }
            }
        }
    }
    let (cid, j) = (id.clone(), job.clone());
    if !state.db.call(move |c| courses::set_revising(c, &cid, &j)).await? {
        // The job never starts: give back what it reserved on the server's key.
        state.meter.release(&job);
        return refuse(crate::t("courses.ai.another_started")).await;
    }
    let req = ai::Request {
        user_id: user.id,
        provider,
        course_id: id.clone(),
        wish: row.brief.clone(),
        lesson_count: if mode == AiMode::Add || lesson.is_some() { 1 } else { 0 },
        depth,
        notes: String::new(),
        kind: ai::SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: change.then(|| text.clone()),
        add_lesson: (mode == AiMode::Add).then(|| text.clone()),
        edit: (mode == AiMode::Edit).then(|| ai::EditRun { feedback: text.clone(), lesson: lesson.clone() }),
        meter: Some(Arc::clone(&state.meter)),
        translate: None,
        named: Default::default(),
    };
    match ai::submit(&state, req, job.clone(), false).await {
        Ok(job) => {
            count_ai_job(&state, user.id);
            let doing = match mode {
                AiMode::Add => "adding a lesson to",
                AiMode::Change => "revising",
                AiMode::Edit if lesson.is_some() => "editing one lesson of",
                AiMode::Edit => "editing",
            };
            tracing::info!("user {} started {doing} {id} ({job})", user.id);
            Ok(Redirect::to(&crate::url(&format!("/courses/{id}?job={job}"))).into_response())
        }
        Err(e) => {
            state.meter.release(&job);
            let cid = id.clone();
            state.db.call(move |c| courses::finish_revision(c, &cid)).await?;
            let row = owned(&state, &user, &id).await?;
            editor_page(&state, user, row, None, Refill { text, mode, outline, error: Some(e), ai_out: None, lesson: lesson_path }).await
        }
    }
}

/// POST /courses/{id}/review: Accept in the review of the latest AI run
/// (#189). The course becomes what the AI wrote minus the lines and lessons
/// left out (`file=<path>`, `lines=<path>:<key>,…`), with the lines edited
/// in place (`edit:<path>:<key>=<text>`, #287), as one revision Undo can
/// take back; what was left out and edited is kept on the run, so the review
/// shows it again. `quiet=1`: "Don't show this automatically next time" (#187).
pub async fn review_accept(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    user.require_ai()?;
    let row = owned(&state, &user, &id).await?;
    if let Err(r) = not_while_generating(&row) {
        return Ok(r);
    }
    let quiet = form.iter().any(|(k, v)| k == "quiet" && v == "1");
    let sent = Exclusions::from_form(&form);
    let sent_edits = Edits::from_form(&form);
    let (uid, r, x, ed) = (user.id, row.clone(), sent.clone(), sent_edits.clone());
    let registry = Arc::clone(&state.courses);
    let outcome = state
        .db
        .call(move |c| {
            crate::profile::set_review_quiet(c, uid, quiet)?;
            let Some(run) = courses::ai_run(c, &r.id)? else { return Ok(Err(crate::t("courses.review.none_left"))) };
            let (excluded, edits) = crate::review::known(&run, x, ed);
            let stored = (Exclusions::from_json(&run.excluded), Edits::from_json(&run.edits));
            let current = courses::load_files(c, &r.id)?;
            if crate::review::undone(&run, (&stored.0, &stored.1), &current) {
                return Ok(Err(crate::t("courses.review.undone_refused")));
            }
            let files = match crate::review::apply(&run, (&excluded, &edits), (&stored.0, &stored.1), &current) {
                Ok(f) => f,
                Err(e) => return Ok(Err(e)),
            };
            if let Some(files) = &files {
                // Refused as the editor refuses a file (problems.rs): in plain words, placed.
                if let Err(e) = ContentStore::from_files(files) {
                    let why = crate::problems::Course::new(files).explain(&e).text;
                    let what = if edits.is_empty() { crate::t("courses.review.with_parts_left_out") } else { crate::t("courses.review.with_changes") };
                    return Ok(Err(crate::t2("courses.review.not_loading", "what", what, "why", why)));
                }
                let label = match (excluded.is_empty(), edits.is_empty()) {
                    (true, true) => crate::t("courses.review.label_took_back"),
                    (false, true) => crate::t("courses.review.label_left_out"),
                    (true, false) => crate::t("courses.review.label_edited"),
                    (false, false) => crate::t("courses.review.label_left_out_edited"),
                };
                let outline = crate::arrange::outline_for(r.outline.as_deref(), files);
                courses::commit_revision(c, &r.id, files, &r.title, outline.as_deref(), &label)?;
            }
            courses::set_ai_run_excluded(c, &r.id, &excluded.to_json(), &edits.to_json())?;
            let broken = if files.is_some() { courses::revalidate(c, &registry, &r.id)? } else { None };
            Ok(Ok((files.is_some(), excluded.is_empty(), edits.is_empty(), broken)))
        })
        .await?;
    let notice = match outcome {
        Err(e) => {
            // Refused: the review again, as it was sent, saying why.
            let review = review_view(&state, &user, &row, false, Some((sent, sent_edits, e))).await?;
            let generate = generate_view(&state, &user, &row, false).await?;
            return editor_page_with(&state, user, row, None, Refill::default(), None, generate, review, false).await;
        }
        Ok((wrote, all, unedited, broken)) => {
            let done = match (all, unedited) {
                (true, true) if wrote => "again",
                (true, true) => "in",
                (false, true) => "left",
                (true, false) => "edits",
                (false, false) => "edits-left",
            };
            let end = match broken {
                _ if !wrote => "kept",
                None => "undo",
                Some(_) => "broken",
            };
            format!("accepted-{done}-{end}")
        }
    };
    tracing::info!("user {} accepted the review of {id}", user.id);
    Ok(Redirect::to(&crate::url(&format!("/courses/{id}?notice={notice}#lessons"))).into_response())
}

#[derive(Deserialize)]
pub struct QuietForm {
    #[serde(default)]
    pub quiet: Option<String>,
}

/// POST /api/review-quiet {quiet=1}: the review's "Don't show this
/// automatically next time", saved as soon as it is ticked.
pub async fn review_quiet(State(state): State<Arc<AppState>>, user: CurrentUser, Form(f): Form<QuietForm>) -> Result<Json<serde_json::Value>, AppError> {
    let quiet = f.quiet.as_deref() == Some("1");
    let uid = user.id;
    state.db.call(move |c| crate::profile::set_review_quiet(c, uid, quiet)).await?;
    Ok(Json(serde_json::json!({ "quiet": quiet })))
}

/// POST /courses/{id}/undo: put back the course from before its last revision.
pub async fn undo(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    undo_or_redo(state, user, id, false).await
}

/// POST /courses/{id}/redo: put back the change the last Undo took away.
pub async fn redo(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    undo_or_redo(state, user, id, true).await
}

async fn undo_or_redo(state: Arc<AppState>, user: CurrentUser, id: String, redo: bool) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    if let Err(r) = not_while_generating(&row) {
        return Ok(r);
    }
    if too_many_writes(&state, &user).is_some() {
        return Ok(to_editor(&id, "too-many-writes"));
    }
    let cid = id.clone();
    let registry = Arc::clone(&state.courses);
    let outcome = state
        .db
        .call(move |c| {
            let step = if redo { courses::redo_revision(c, &cid)? } else { courses::undo_revision(c, &cid)? };
            let Some(label) = step else { return Ok(None) };
            Ok(Some((label, courses::revalidate(c, &registry, &cid)?)))
        })
        .await?;
    let notice = match (outcome, redo) {
        (None, false) => "no-undo",
        (None, true) => "no-redo",
        (Some((_, None)), false) => "undone",
        (Some((_, None)), true) => "redone",
        (Some((_, Some(_))), false) => "undone-broken",
        (Some((_, Some(_))), true) => "redone-broken",
    };
    tracing::info!("user {} {} the last revision of {id}", user.id, if redo { "redid" } else { "undid" });
    Ok(to_editor(&id, notice))
}
