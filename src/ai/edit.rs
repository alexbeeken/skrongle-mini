//! Edit lessons (#1240): an AI run that rewrites lessons already in the
//! course and adds, removes or moves none, the mirror of Only add
//! (`add_lesson`, checked by `arrange::only_adds`). One request plans which
//! lessons to rewrite (`edit-plan`), then one request per lesson
//! (`write_slots`, shared with `revise`). Nothing is saved unless the result
//! passes `arrange::only_edits`; then it joins the course as one revision,
//! so Undo puts the old set back.

use super::*;
use crate::arrange::LessonFile;

impl Request {
    /// A run that changes a course (feedback, an added lesson, an edit)
    /// rather than writing one: it changes nothing unless it succeeds.
    pub fn changes_a_course(&self) -> bool {
        self.revise.is_some() || self.add_lesson.is_some() || self.edit.is_some()
    }
}

/// What the lesson requests of a revision or an edit share.
pub(super) struct Rewrite<'a> {
    pub job: &'a Job,
    pub req: &'a Request,
    pub system: &'a str,
    /// The course as it is.
    pub files: &'a [CourseFile],
    /// Its files that are neither course.json nor a lesson.
    pub others: &'a [CourseFile],
    /// course.json as the new course has it.
    pub manifest_text: &'a str,
    pub title: &'a str,
    pub feedback: &'a str,
    pub material: &'a str,
    pub kind: SubjectKind,
    pub depth: Depth,
}

/// Write every slot that is not kept, one request each, each validated with
/// the whole course as it would be (up to three tries).
pub(super) fn write_slots(w: &Rewrite, slots: &mut [Slot], to_write: usize) -> Result<(), String> {
    let (job, req) = (w.job, w.req);
    let (course_id, job_id, rules) = (&req.course_id, job.snapshot().id, w.kind.rules());
    let (files, others, manifest_text, new_title, feedback, material) = (w.files, w.others, w.manifest_text, w.title, w.feedback, w.material);
    let outline_summary: Vec<serde_json::Value> = slots
        .iter()
        .map(|s| serde_json::json!({ "id": s.entry.get("id"), "title": s.entry.get("title"), "unit": s.entry.get("unit") }))
        .collect();
    let mut done = 0;
    for i in 0..slots.len() {
        let Some(slot) = slots.get(i).filter(|s| s.change != Change::Keep) else { continue };
        let (entry, change, old) = (slot.entry.clone(), slot.change, slot.old.clone());
        let what = match (change, slot.instructions.is_empty()) {
            (Change::Check, true) => check::CHECK_TASK.to_string(),
            (Change::Check, false) => format!("{} {}", check::CHECK_TASK, slot.instructions),
            (_, true) => "Apply the feedback where it concerns this lesson.".to_string(),
            (_, false) => slot.instructions.clone(),
        };
        let old_file = old.as_ref().and_then(|(path, _)| files.iter().find(|f| &f.path == path));
        let lesson_title = entry.get("title").and_then(|t| t.as_str()).unwrap_or("lesson").to_string();
        let slug = entry.get("slug").and_then(|s| s.as_str()).unwrap_or_default().to_string();
        let verb = match change {
            Change::New => "Writing",
            Change::Check => "Checking",
            _ => "Rewriting",
        };
        job.update(|s| s.phase = format!("{verb} lesson {} of {to_write}: {lesson_title}", done + 1));
        let now_block = match &old {
            Some((path, _)) => format!(
                "Revise this lesson of the course \"{new_title}\". The lesson as it is now:\n{}\n\n",
                files.iter().find(|f| &f.path == path).map(|f| f.content.as_str()).unwrap_or("{}")
            ),
            None => format!("Write a new lesson for the course \"{new_title}\".\n\n"),
        };
        let prompt = format!(
            "{now_block}The learner's feedback on the course, between the <feedback> tags:\n<feedback>\n{}\n</feedback>\n\n\
             What this lesson must do: {what}\n\n\
             Use exactly these values for id, slug, title, unit, group and prerequisites:\n{}\n\n\
             The course manifest is:\n{manifest_text}\n\nThe lessons of the course, in order:\n{}\n\n\
             Keep what the feedback does not ask to change, and keep a question's id when the question stays. \
             {}{}{}{material}",
            feedback.trim(),
            serde_json::to_string_pretty(&entry).unwrap_or_default(),
            serde_json::to_string(&outline_summary).unwrap_or_default(),
            match change {
                Change::Check => check::CHECK_REPLY,
                Change::Rewrite => check::REWRITE_REPLY,
                _ => "Reply with the complete lesson JSON object only.",
            },
            prompt::lesson_line(w.kind),
            w.depth.lesson_line(),
        );
        let purpose = if change == Change::Check { "check-lesson" } else { "revise-lesson" };
        let call = Call::new(purpose).course(course_id).job(&job_id).key(&slug);
        let mut attempt = 0;
        let mut retry = String::new();
        let (mut why, mut unsure) = (vec![], vec![]);
        let file = loop {
            attempt += 1;
            let text = job.llm(req, &call, w.system, &prompt::with_reference(&rules, &format!("{prompt}{retry}")))?;
            let read = parse_json(&text).map(|v| check::read_reply(&v)).and_then(|(lesson, y, c)| {
                (why, unsure) = (y, c);
                // A check that found nothing wrong leaves the file byte for byte.
                if change == Change::Check {
                    if let Some(same) = check::unchanged_file(old_file, lesson.as_ref()) {
                        return Ok(same);
                    }
                }
                lesson_file(&lesson.ok_or("the reply has no lesson")?, &entry)
            });
            let candidate = match read {
                Ok(f) => f,
                Err(e) if attempt < 3 => {
                    job.log(format!("\"{lesson_title}\" came back malformed ({e}); asking again."));
                    retry = format!("\n\nYour previous reply could not be used: {e}. Reply with the corrected lesson JSON only.");
                    continue;
                }
                Err(e) => return Err(format!("\"{lesson_title}\": {e}. The course is unchanged.")),
            };
            let checked = trial(manifest_text, slots, others, Some((i, &candidate)))
                .and_then(|t| check_file_caps(&t).map(|()| t))
                .and_then(|t| ContentStore::from_files(&t).map_err(|e| e.to_string()));
            match checked {
                Ok(_) => break candidate,
                Err(e) if attempt < 3 => {
                    job.log(format!("\"{lesson_title}\" did not validate ({e}); asking for a fix."));
                    retry = format!("\n\nYour previous reply was rejected by the validator: {e}. Fix it and reply with the corrected lesson JSON only.");
                }
                Err(e) => return Err(format!("\"{lesson_title}\" did not validate after {attempt} attempts: {e}. The course is unchanged.")),
            }
        };
        if let Some(s) = slots.get_mut(i) {
            s.done = Some(file);
            (s.why, s.check) = (why, unsure);
        }
        done += 1;
        job.update(|s| s.lessons_done = done);
        job.log(format!("{verb} done: {lesson_title}"));
    }
    Ok(())
}

/// The `focus` of each lesson in a saved AI outline.
fn focus_of(outline: Option<&str>) -> HashMap<String, serde_json::Value> {
    outline
        .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
        .and_then(|o| o.get("lessons").and_then(|l| l.as_array()).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| Some((e.get("id")?.as_str()?.to_string(), e.get("focus")?.clone())))
        .collect()
}

/// Read an edit plan: every lesson of the course once, in course order, each
/// kept or rewritten. Anything else (a new lesson, one left out, another
/// order, another kind of change) is refused. A rewrite keeps its id, slug,
/// unit, group and prerequisites; it may take a new title and summary.
pub(super) fn read_edit_plan(plan: &serde_json::Value, lessons: &[LessonFile], focus: &HashMap<String, serde_json::Value>) -> Result<Vec<Slot>, String> {
    let entries = plan.get("lessons").and_then(|l| l.as_array()).ok_or("the plan has no \"lessons\" array")?;
    let mut slots: Vec<Slot> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let id = e.get("id").and_then(|v| v.as_str()).map(str::trim).unwrap_or_default();
        let Some(l) = lessons.iter().find(|l| l.id == id) else {
            return Err(format!("plan entry {} ('{id}') is not a lesson of the course; an edit adds no lesson", i + 1));
        };
        if slots.iter().any(|s| s.id() == id) {
            return Err(format!("the plan lists lesson '{id}' twice"));
        }
        let change = match e.get("change").and_then(|c| c.as_str()).unwrap_or("keep") {
            "keep" => Change::Keep,
            "rewrite" => Change::Rewrite,
            "check" => Change::Check,
            other => return Err(format!("lesson '{id}' has \"change\": \"{other}\"; an edit may only keep, check or rewrite a lesson")),
        };
        let mut entry = outline_entry(&l.value, e.get("focus").or(focus.get(id)));
        if change == Change::Rewrite {
            for key in ["title", "summary"] {
                if let Some(v) = e.get(key).filter(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())) {
                    if let Some(m) = entry.as_object_mut() {
                        m.insert(key.into(), v.clone());
                    }
                }
            }
        }
        let instructions = e.get("instructions").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        slots.push(Slot { entry, change, instructions, old: Some((l.path.clone(), l.value.clone())), done: None, why: vec![], check: vec![] });
    }
    if let Some(l) = lessons.iter().find(|l| !slots.iter().any(|s| s.id() == l.id)) {
        return Err(format!("the plan leaves out lesson '{}'; an edit removes no lesson", l.id));
    }
    if slots.iter().map(Slot::id).ne(lessons.iter().map(|l| l.id.as_str())) {
        return Err("the plan puts the lessons in another order; an edit moves no lesson".into());
    }
    Ok(slots)
}

/// Edit the lessons of a course from the learner's feedback: plan, rewrite
/// the planned lessons, check the result with `arrange::only_edits`, save it
/// as one revision. Any failure leaves the course exactly as it was. Works on
/// any course that loads, hand-made or AI-made, finished or not.
pub(super) fn edit(job: &Job, db: &Db, registry: &crate::courses::Courses, req: &Request, feedback: &str) -> Result<(), String> {
    let system = if req.instructions.trim().is_empty() { DEFAULT_INSTRUCTIONS } else { req.instructions.as_str() };
    let course_id = req.course_id.clone();
    let job_id = job.snapshot().id;
    job.update(|s| s.phase = "Reading the course".into());
    let cid = course_id.clone();
    let (files, row, notes) = db
        .with(move |c| Ok((courses::load_files(c, &cid)?, courses::course(c, &cid)?, courses::notes_of(c, &cid)?)))
        .map_err(|e| format!("database: {e}"))?;
    let row = row.ok_or("the course is gone")?;
    let (store, lessons) = crate::arrange::lessons_of(&files)?;
    let manifest = files.iter().find(|f| f.path == MANIFEST_FILE).ok_or("the course has no course.json")?;
    let others: Vec<CourseFile> = files.iter().filter(|f| f.path != MANIFEST_FILE && !lessons.iter().any(|l| l.path == f.path)).cloned().collect();
    let focus = focus_of(row.outline.as_deref());
    let current: Vec<serde_json::Value> = (store.lessons().iter().enumerate())
        .filter_map(|(i, l)| lessons.iter().find(|f| f.id == l.id).map(|f| prompt::plan_entry(i + 1, l, outline_entry(&f.value, focus.get(&l.id)))))
        .collect();
    let title = store.title().to_string();
    job.update(|s| s.course_title = Some(title.clone()));
    let material = material_block(&notes);
    let kind = SubjectKind::of_course(row.outline.as_deref(), &manifest.content);
    let rules = kind.rules();

    // ---- 1. the plan: keep or rewrite, nothing else ----
    job.update(|s| s.phase = "Planning the changes".into());
    job.log(format!("Asking {} ({}) which lessons of \"{title}\" to rewrite\u{2026}", provider_name(req.provider.kind), req.provider.model));
    let made = match row.brief.trim() {
        "" => "A learner is studying this course.".to_string(),
        brief => format!("A learner made this course with you earlier. They asked for: \"{brief}\""),
    };
    let plan_prompt = format!(
        "{made}\n\n\
         Its course.json manifest:\n{}\n\nIts lessons, in order:\n{}\n\n\
         {} Now they want its lessons edited. Their feedback, between the <feedback> tags, says what to change:\n\
         <feedback>\n{}\n</feedback>\n\n\
         Plan the edit. It rewrites lessons that are already in the course: it adds no lesson, removes none and keeps them in this order, \
         and course.json stays as it is. A lesson the feedback does not concern stays exactly as it is. \
         Reply with one JSON object {{\"lessons\": [<one entry for every lesson above, in the same order>]}} where each entry is \
         {{\"id\": <its id>, \"title\": <its title, or a new one if the feedback asks for it>, \"summary\": <one line>, \
         \"focus\": <the one thing the lesson teaches>, \"change\": \"keep\" | \"check\" | \"rewrite\", \
         \"instructions\": <for rewrite: what this lesson must cover or do differently>}}.\n\
         - \"keep\": the lesson stays as it is.\n\
         {}\
         - \"rewrite\": the lesson is written again under the same id, so the learner's progress stays with it.{material}",
        manifest.content.trim(),
        serde_json::to_string_pretty(&current).unwrap_or_default(),
        prompt::NUMBER_LINE,
        feedback.trim(),
        check::PLAN_LINE,
    );
    let call = Call::new("edit-plan").course(&course_id).job(&job_id);
    let mut retry = String::new();
    let mut attempt = 0;
    let mut slots = loop {
        attempt += 1;
        let text = job.llm(req, &call, system, &prompt::with_reference(&rules, &format!("{plan_prompt}{retry}")))?;
        let planned = parse_json(&text).and_then(|p| read_edit_plan(&p, &lessons, &focus)).and_then(|slots| {
            // New titles must fit the course, and leave every lesson where it is (lessons without `order` sort by title).
            let planned = ContentStore::from_files(&trial(&manifest.content, &slots, &others, None)?).map_err(|e| format!("the planned course does not load: {e}"))?;
            if planned.lessons().iter().map(|l| l.id.as_str()).ne(lessons.iter().map(|l| l.id.as_str())) {
                return Err("with the new titles the lessons would sort into another order; keep every lesson where it is".into());
            }
            Ok(slots)
        });
        match planned {
            Ok(p) => break p,
            Err(e) if attempt < 2 => {
                job.log(format!("The plan could not be used ({e}); asking again."));
                retry = format!("\n\nYour previous plan could not be used: {e}. Reply with the corrected plan JSON only.");
            }
            Err(e) => return Err(format!("the plan could not be used: {e}. The course is unchanged.")),
        }
    };
    let to_write = slots.iter().filter(|s| s.change != Change::Keep).count();
    if to_write == 0 {
        return Err(NOTHING_CHANGED.into());
    }
    let checks = slots.iter().filter(|s| s.change == Change::Check).count();
    job.log(format!("Plan: keep {}, check {checks}, rewrite {}.", slots.len() - to_write, to_write - checks));
    job.update(|s| {
        s.lessons_total = to_write;
        s.lessons_done = 0;
        s.phase = "Writing lessons".into();
    });
    for s in slots.iter_mut().filter(|s| s.change == Change::Keep) {
        let path = s.old.as_ref().map(|(p, _)| p.clone()).unwrap_or_default();
        let content = files.iter().find(|f| f.path == path).map(|f| f.content.clone()).unwrap_or_default();
        s.done = Some(CourseFile { path, content });
    }

    // ---- 2. one request per lesson to rewrite, always full lessons ----
    let rewrite = Rewrite {
        job,
        req,
        system,
        files: &files,
        others: &others,
        manifest_text: &manifest.content,
        title: &title,
        feedback,
        material: &material,
        kind,
        depth: Depth::Full,
    };
    write_slots(&rewrite, &mut slots, to_write)?;

    // ---- 3. checked in code: the same lessons, in the same places ----
    let mut final_files = trial(&manifest.content, &slots, &others, None)?;
    check_file_caps(&final_files).map_err(|e| format!("the edited course is too large: {e}. The course is unchanged."))?;
    ContentStore::from_files(&final_files).map_err(|e| format!("the edited course does not load: {e}. The course is unchanged."))?;
    if final_files.len() == files.len() && final_files.iter().all(|f| files.iter().any(|b| b.path == f.path && b.content == f.content)) {
        return Err(nothing_changed(job, &slots));
    }
    job.update(|s| s.notes = check::notes_of(&slots, &files));
    crate::arrange::only_edits(&files, &final_files).map_err(|e| format!("the edit was refused because {e}. The course is unchanged."))?;
    final_files.sort_by(|a, b| a.path.cmp(&b.path));
    let outline = crate::arrange::outline_for(row.outline.as_deref(), &final_files);
    let (cid, t, label) = (course_id.clone(), title.clone(), courses::quoted("Edited with feedback:", feedback));
    let err = db
        .with(move |c| {
            courses::commit_revision(c, &cid, &final_files, &t, outline.as_deref(), &label)?;
            courses::revalidate(c, registry, &cid)
        })
        .map_err(|e| format!("database: {e}"))?;
    if let Some(e) = err {
        return Err(format!("the edited course does not load: {e}. Undo it from the course page."));
    }
    job.log("Edit saved. Undo puts the previous version back.");
    Ok(())
}

/// A run whose lessons all came back as they were.
pub(super) const NOTHING_CHANGED: &str = "Nothing changed: the AI kept every lesson as it was. Say more specifically what should change. The course is unchanged.";

/// Why a run that changed nothing stops: a fact check that found nothing
/// wrong says so, and logs what it was unsure of.
pub(super) fn nothing_changed(job: &Job, slots: &[Slot]) -> String {
    if !check::only_checks(slots) {
        return NOTHING_CHANGED.into();
    }
    for s in slots {
        for line in &s.check {
            job.log(format!("Check ({}): {line}", s.entry.get("title").and_then(|t| t.as_str()).unwrap_or_default()));
        }
    }
    check::NOTHING_WRONG.into()
}
