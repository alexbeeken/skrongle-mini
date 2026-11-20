//! Paper mode (#236): a lesson or a whole unit laid out for printing, with
//! room to write and an answer key on its own sheet.
//!
//! Only the questions that work on paper are printed (`on_paper`), in the
//! lesson's own order, so two prints match and one key fits both. Opening a
//! print page records nothing: no progress, no activity, no reading.

use askama::Template;
use axum::extract::{Path, Query};
use axum::response::{Html, IntoResponse, Response};

use super::CourseView;
use crate::auth::{CurrentUser, Project};
use crate::content::{ContentStore, Lesson, QuizItem, QuizType};
use crate::error::AppError;

/// Can this question be asked on paper? Choices, true or false, gap fills,
/// numbers and expressions can. Anything that needs the screen cannot: a
/// clock, a keyboard, a staff, tiles, a ruler, sound (`play`), and music
/// drawn by a script (`abc`, options drawn as staves), since the print page
/// runs no drawing scripts.
pub fn on_paper(item: &QuizItem) -> bool {
    let kind = matches!(
        item.kind,
        QuizType::MultipleChoice | QuizType::TrueFalse | QuizType::FillInBlank | QuizType::Numeric | QuizType::Expression
    );
    kind && item.play.is_empty() && item.abc.is_empty() && item.option_kind.is_empty()
}

/// Does the lesson have anything to print (one question that works on paper)?
pub fn printable(lesson: &Lesson) -> bool {
    lesson.quiz.iter().any(on_paper)
}

/// A unit has a Print the unit link when one of its lessons is printable.
pub fn unit_printable(content: &ContentStore, unit_index: usize) -> bool {
    content.lessons_in(unit_index).any(printable)
}

/// One question on the sheet, and its line in the key.
pub struct PaperQuestion<'a> {
    pub item: &'a QuizItem,
    /// The options as "a", "b"… with their text, in the lesson's order.
    pub options: Vec<(char, &'a str)>,
    /// The key's answer: "b) text" for a choice, the answer otherwise.
    pub answer: String,
    /// Other answers that count too (gap fills, numbers, expressions).
    pub also: String,
}

impl<'a> PaperQuestion<'a> {
    fn new(item: &'a QuizItem) -> PaperQuestion<'a> {
        let options: Vec<(char, &str)> = item.options.iter().zip('a'..='z').map(|(o, c)| (c, o.as_str())).collect();
        let answer = if item.is_choice() {
            options
                .iter()
                .find(|(_, o)| *o == item.correct_answer)
                .map(|(c, o)| format!("{c}) {o}"))
                .unwrap_or_else(|| item.correct_answer.clone())
        } else if item.unit.is_empty() {
            item.correct_answer.clone()
        } else {
            format!("{} {}", item.correct_answer, item.unit)
        };
        let also = if item.is_choice() { String::new() } else { item.accepted_answers.join(" · ") };
        PaperQuestion { item, options, answer, also }
    }

    pub fn choice(&self) -> bool {
        self.item.is_choice()
    }
}

/// One lesson on the sheet.
pub struct PaperLesson<'a> {
    pub lesson: &'a Lesson,
    pub questions: Vec<PaperQuestion<'a>>,
}

/// The questions of a lesson that go on paper, in the lesson's order.
pub fn paper_lesson(lesson: &Lesson) -> PaperLesson<'_> {
    PaperLesson { lesson, questions: lesson.quiz.iter().filter(|q| on_paper(q)).map(PaperQuestion::new).collect() }
}

#[derive(Template)]
#[template(path = "lesson_print.html")]
pub struct PrintTemplate<'a> {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    /// The page's title: the lesson's, or the unit's.
    pub title: String,
    /// Where "Back" goes (the lesson, or the unit on /lessons), and its words.
    pub back_href: String,
    pub back_label: String,
    pub lessons: Vec<PaperLesson<'a>>,
}

impl PrintTemplate<'_> {
    /// Some lesson has a question, so there is a key.
    pub fn has_key(&self) -> bool {
        self.lessons.iter().any(|l| !l.questions.is_empty())
    }
}

/// GET /lessons/{slug}/print: one lesson as a worksheet. Not found when no
/// question of the lesson works on paper.
pub async fn lesson(p: Project, Path(slug): Path<String>) -> Result<Response, AppError> {
    let content = &p.course;
    let lesson = content.lesson_by_slug(&slug).ok_or(AppError::NotFound)?;
    if !printable(lesson) {
        return Err(AppError::NotFound);
    }
    let tpl = PrintTemplate {
        user: Some(p.user.clone()),
        course: content.view(),
        title: lesson.title.clone(),
        back_href: crate::url(&format!("/lessons/{}", lesson.slug)),
        back_label: crate::t("study.paper.back_lesson"),
        lessons: vec![paper_lesson(lesson)],
    };
    // Rendered here because the template borrows from the course.
    Ok(Html(tpl.render()?).into_response())
}

#[derive(serde::Deserialize)]
pub struct UnitQuery {
    pub unit: Option<String>,
}

/// GET /lessons/print?unit=<id>: every lesson of a unit, one after another,
/// with one key at the end. Not found when no lesson of it has a question
/// that works on paper.
pub async fn unit(p: Project, Query(q): Query<UnitQuery>) -> Result<Response, AppError> {
    let content = &p.course;
    let id = q.unit.unwrap_or_default();
    let ui = content.units().iter().position(|u| u.id == id).ok_or(AppError::NotFound)?;
    if !unit_printable(content, ui) {
        return Err(AppError::NotFound);
    }
    let tpl = PrintTemplate {
        user: Some(p.user.clone()),
        course: content.view(),
        title: content.unit(ui).title.clone(),
        back_href: crate::url(&format!("/lessons?unit={}", crate::urlenc::urlencode(&id))),
        back_label: crate::t("study.paper.back_lessons"),
        lessons: content.lessons_in(ui).map(paper_lesson).collect(),
    };
    Ok(Html(tpl.render()?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> QuizItem {
        serde_json::from_str(json).expect("quiz item parses")
    }

    #[test]
    fn on_paper_keeps_the_written_kinds_and_drops_the_screen_ones() {
        for kind in ["multiple-choice", "true-false", "fill-in-blank", "numeric", "expression"] {
            let q = item(&format!(r#"{{"question_id":"q","prompt":"p","type":"{kind}","correct_answer":"1"}}"#));
            assert!(on_paper(&q), "{kind} goes on paper");
        }
        for kind in ["fill-bar", "chord-spell", "staff-note", "keyboard", "algebra-tiles", "clock", "measure"] {
            let q = item(&format!(r#"{{"question_id":"q","prompt":"p","type":"{kind}","correct_answer":"1"}}"#));
            assert!(!on_paper(&q), "{kind} needs the screen");
        }
        let sound = item(r#"{"question_id":"q","prompt":"p","type":"multiple-choice","options":["a","b"],"correct_answer":"a","play":"CDE"}"#);
        assert!(!on_paper(&sound), "a question with sound needs the screen");
        let staff = item(r#"{"question_id":"q","prompt":"p","type":"fill-in-blank","correct_answer":"C","abc":"C"}"#);
        assert!(!on_paper(&staff), "a staff is drawn by a script");
        let staves = item(r#"{"question_id":"q","prompt":"p","type":"multiple-choice","options":["C","D"],"correct_answer":"C","option_kind":"abc"}"#);
        assert!(!on_paper(&staves), "options drawn as staves need the screen");
    }

    /// The print pages over HTTP, on the test course (it has a lesson of
    /// every question type): same questions in the same order twice, the
    /// screen-only types left out, no page and no link for a lesson with
    /// none, and nothing recorded.
    #[test]
    fn print_pages_are_stable_leave_out_the_screen_and_record_nothing() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let mut state = std::sync::Arc::into_inner(crate::routes::test_state(true)).unwrap();
        state.courses = std::sync::Arc::new(crate::courses::Courses::load(std::path::Path::new("data"), true).unwrap());
        let state = std::sync::Arc::new(state);
        let course_id = "test-course";
        let ann = state
            .db
            .with(|c| {
                let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
                crate::courses::open_project(c, ann, course_id)?;
                crate::courses::set_project_settings(c, ann, course_id, &crate::courses::SettingsChange::default())?;
                Ok(ann)
            })
            .unwrap();
        let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
        let store = state.courses.builtin_store(course_id).unwrap();
        let app = crate::routes::router(state.clone());
        let base = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            format!("http://{addr}")
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let get = |path: &str| {
            let mut res = agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap();
            (res.status().as_u16(), res.body_mut().read_to_string().unwrap())
        };
        let ids = |html: &str, testid: &str| -> Vec<String> {
            let pat = format!(r#"data-testid="{testid}" data-question-id=""#);
            html.split(&pat).skip(1).map(|s| s[..s.find('"').unwrap()].to_string()).collect()
        };

        // A printable lesson of the unit that holds the clock lesson (the
        // question types, one lesson each): its paper questions, in its own
        // order, twice alike.
        let clock = store.lesson_by_slug("clock").unwrap();
        let mixed = store.lessons_in(clock.unit_index).find(|l| printable(l)).expect("a printable lesson beside the clock");
        let want: Vec<String> = mixed.quiz.iter().filter(|q| on_paper(q)).map(|q| q.question_id.clone()).collect();
        let (status, first) = get(&format!("/lessons/{}/print", mixed.slug));
        assert_eq!(status, 200, "{first}");
        assert_eq!(ids(&first, "paper-question"), want, "the lesson's order, screen-only kinds left out");
        assert_eq!(ids(&first, "paper-answer"), want, "the key follows the sheet");
        assert_eq!(ids(&get(&format!("/lessons/{}/print", mixed.slug)).1, "paper-question"), want, "a second print matches");
        assert!(first.contains(r#"data-testid="paper-key""#));

        // The clock lesson has nothing for paper: no page and no link.
        assert!(!printable(clock));
        assert_eq!(get("/lessons/clock/print").0, 404);

        // The unit: every lesson of it, one key at the end.
        let ui = mixed.unit_index;
        let unit = &store.units()[ui];
        let (status, sheet) = get(&format!("/lessons/print?unit={}", unit.id));
        assert_eq!(status, 200);
        let lessons: Vec<&Lesson> = store.lessons_in(ui).collect();
        assert_eq!(sheet.matches(r#"data-testid="paper-lesson""#).count(), lessons.len());
        assert_eq!(sheet.matches(r#"data-testid="paper-key""#).count(), 1);
        let all: Vec<String> = lessons.iter().flat_map(|l| l.quiz.iter().filter(|q| on_paper(q)).map(|q| q.question_id.clone())).collect();
        assert_eq!(ids(&sheet, "paper-answer"), all);
        let shown = ids(&sheet, "paper-question");
        assert_eq!(shown, all);
        let screen: Vec<&QuizItem> = lessons.iter().flat_map(|l| l.quiz.iter()).filter(|q| !on_paper(q)).collect();
        assert!(!screen.is_empty(), "the unit has screen-only questions to leave out");
        assert!(screen.iter().all(|q| !shown.contains(&q.question_id)), "a screen-only question was printed");
        assert_eq!(get("/lessons/print?unit=no-such-unit").0, 404);

        // Nothing of it was recorded: no reading, no pass, no resume point.
        let (touched, passes) = state
            .db
            .with(|c| Ok((crate::activity::last_touched(c, ann, course_id)?, crate::progress::load_all(c, ann, course_id)?)))
            .unwrap();
        assert!(touched.is_empty(), "printing touched a lesson");
        assert!(passes.is_empty(), "printing recorded progress");

        // The links: Print on a lesson with paper questions, none on the clock lesson, Print the unit.
        assert!(get(&format!("/lessons/{}", mixed.slug)).1.contains(r#"data-testid="lesson-print""#), "the lesson offers Print");
        assert!(!get("/lessons/clock").1.contains(r#"data-testid="lesson-print""#), "no Print on the clock lesson");
        assert!(get(&format!("/lessons?unit={}", unit.id)).1.contains(r#"data-testid="lessons-print-unit""#));
    }

    #[test]
    fn a_choice_answer_names_its_letter() {
        let q = item(r#"{"question_id":"q","prompt":"p","type":"multiple-choice","options":["x","y","z"],"correct_answer":"y"}"#);
        let pq = PaperQuestion::new(&q);
        assert_eq!(pq.answer, "b) y");
        assert_eq!(pq.options.iter().map(|o| o.0).collect::<String>(), "abc");
        let n = item(r#"{"question_id":"q","prompt":"p","type":"numeric","correct_answer":"12","unit":"cm","accepted_answers":["12.0"]}"#);
        let pn = PaperQuestion::new(&n);
        assert_eq!(pn.answer, "12 cm");
        assert_eq!(pn.also, "12.0");
    }
}
