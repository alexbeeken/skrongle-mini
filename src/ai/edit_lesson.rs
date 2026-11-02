//! "Edit with AI" on one lesson's row (#1242): one request rewrites that
//! lesson from the learner's request. No plan step. The prompt gets the
//! lesson's file, the course manifest and the request. Nothing is saved
//! unless the result passes `arrange::only_edits_one` (only that file
//! changed, same id, slug and place); then it joins the course as one
//! revision, labelled like a course-wide edit, so Undo puts it back.

use super::*;

/// What an Edit lessons run (#1240) works from: the learner's feedback and,
/// for "Edit with AI" on a row, the one lesson file it may change.
#[derive(Clone, Debug)]
pub struct EditRun {
    pub feedback: String,
    /// The file (`lessons/<slug>.json`) and title of the one lesson that may
    /// change (#1242), or `None` for a course-wide edit.
    pub lesson: Option<(String, String)>,
}

/// The fields a lesson edit keeps as they are, as JSON object members
/// (`"id": "u1", "slug": "one", …`): the fixtures' `{{frozen}}`.
fn frozen(lesson: &serde_json::Value) -> String {
    ["id", "slug", "title", "unit", "group", "level", "order", "prerequisites"]
        .iter()
        .filter_map(|k| Some(format!("{}: {}", serde_json::to_string(k).ok()?, serde_json::to_string(lesson.get(*k)?).ok()?)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Rewrite the lesson at `path` from `request`, checked and saved as one
/// revision. Any failure leaves the course exactly as it was.
pub(super) fn edit_lesson(job: &Job, db: &Db, registry: &crate::courses::Courses, req: &Request, path: &str, request: &str) -> Result<(), String> {
    let system = if req.instructions.trim().is_empty() { DEFAULT_INSTRUCTIONS } else { req.instructions.as_str() };
    let course_id = req.course_id.clone();
    let job_id = job.snapshot().id;
    let cid = course_id.clone();
    let (files, row) = db.with(move |c| Ok((courses::load_files(c, &cid)?, courses::course(c, &cid)?))).map_err(|e| format!("database: {e}"))?;
    let row = row.ok_or("the course is gone")?;
    let (store, lessons) = crate::arrange::lessons_of(&files)?;
    let lesson = lessons.iter().find(|l| l.path == path).ok_or("that lesson is no longer in the course. The course is unchanged.")?;
    let manifest = files.iter().find(|f| f.path == MANIFEST_FILE).ok_or("the course has no course.json")?;
    let old = files.iter().find(|f| f.path == path).map(|f| f.content.clone()).unwrap_or_default();
    let lesson_title = lesson.value.get("title").and_then(|t| t.as_str()).unwrap_or(&lesson.id).to_string();
    let title = store.title().to_string();
    job.update(|s| {
        s.course_title = Some(title.clone());
        s.lesson_title = Some(lesson_title.clone());
        s.lessons_total = 1;
        s.phase = format!("Editing \u{201c}{lesson_title}\u{201d}");
    });
    job.log(format!("Asking {} ({}) to edit \"{lesson_title}\"\u{2026}", provider_name(req.provider.kind), req.provider.model));
    let kind = SubjectKind::of_course(row.outline.as_deref(), &manifest.content);
    let prompt = format!(
        "Edit only this one lesson of the course \"{title}\". The lesson as it is now:\n{}\n\n\
         The learner's request, between the <request> tags:\n<request>\n{}\n</request>\n\n\
         The course manifest is:\n{}\n\n\
         Keep its id, slug, unit, group, level, order and prerequisites exactly as they are, and keep what the request does not ask to change, \
         a question's id too when the question stays. Do not add `themes`. Reply with the complete lesson JSON object only.{}{}\n\n\
         ----- Course format reference -----\n\n{FORMAT_DOC}",
        old.trim(),
        request.trim(),
        manifest.content.trim(),
        prompt::lesson_line(kind),
        kind.rules(),
    );
    let call = Call::new("edit-lesson")
        .course(&course_id)
        .job(&job_id)
        .var("frozen", &frozen(&lesson.value))
        .var("id", &lesson.id)
        .var("slug", lesson.value.get("slug").and_then(|s| s.as_str()).unwrap_or_default())
        .var("title", &lesson_title)
        .var("unit", lesson.unit());
    let mut retry = String::new();
    let mut attempt = 0;
    let mut after = loop {
        attempt += 1;
        let text = job.llm(req, &call, system, &format!("{prompt}{retry}"))?;
        let checked = parse_json(&text)
            .and_then(|v| if v.is_object() { serde_json::to_string_pretty(&v).map_err(|e| e.to_string()) } else { Err("the lesson is not a JSON object".into()) })
            .map(|content| files.iter().map(|f| if f.path == path { CourseFile { path: f.path.clone(), content: content.clone() + "\n" } } else { f.clone() }).collect::<Vec<_>>())
            .and_then(|after| {
                check_file_caps(&after)?;
                crate::arrange::same_lesson(&files, &after, path)?;
                ContentStore::from_files(&after).map_err(|e| format!("the edited lesson does not load: {e}"))?;
                let parsed = |c: &str| serde_json::from_str::<serde_json::Value>(c).ok();
                if after.iter().find(|f| f.path == path).is_some_and(|f| parsed(&f.content) == parsed(&old)) {
                    return Err(edit::NOTHING_CHANGED.into());
                }
                crate::arrange::only_edits_one(&files, &after, path).map(|()| after)
            });
        match checked {
            Ok(after) => break after,
            Err(e) if e == edit::NOTHING_CHANGED => return Err(e),
            Err(e) if attempt < 3 => {
                job.log(format!("\"{lesson_title}\" could not be used ({e}); asking again."));
                retry = format!("\n\nYour previous reply could not be used: {e}. Fix it and reply with the corrected lesson JSON only.");
            }
            Err(e) => return Err(format!("the edit was refused because {e}. The course is unchanged.")),
        }
    };
    job.update(|s| s.lessons_done = 1);
    after.sort_by(|a, b| a.path.cmp(&b.path));
    let outline = crate::arrange::outline_for(row.outline.as_deref(), &after);
    let (cid, label) = (course_id.clone(), courses::quoted("Edited with feedback:", request));
    let err = db
        .with(move |c| {
            courses::commit_revision(c, &cid, &after, &title, outline.as_deref(), &label)?;
            courses::revalidate(c, registry, &cid)
        })
        .map_err(|e| format!("database: {e}"))?;
    if let Some(e) = err {
        return Err(format!("the edited course does not load: {e}. Undo it from the course page."));
    }
    job.log("Edit saved. Undo puts the previous version back.");
    Ok(())
}
