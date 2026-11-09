use super::*;
use crate::content::ContentStore;
use std::path::Path;

impl Exercise {
    fn is_graded(&self) -> bool {
        !matches!(self, Exercise::Bite { .. })
    }

    /// Needs the browser to speak (skipped by the player when audio is off).
    fn is_listening(&self) -> bool {
        matches!(
            self,
            Exercise::HearBuild { .. } | Exercise::HearGap { .. } | Exercise::HearPick { .. } | Exercise::HearWhich { .. } | Exercise::HearMatch { .. }
        )
    }
}

fn store() -> ContentStore {
    ContentStore::load_dir(Path::new("data/courses/spanish")).expect("content loads")
}

#[test]
fn lesson_session_mixes_kinds() {
    let c = store();
    let l = c.lesson_by_slug("ser-vs-estar").unwrap();
    let mut rng = Rng::seeded(7);
    let s = lesson_session(&c, l, &HashSet::new(), &mut rng);
    assert!(matches!(s.exercises[0], Exercise::Bite { .. }));
    let graded = s.exercises.iter().filter(|e| e.is_graded()).count();
    assert!(graded >= 8, "only {graded} graded exercises");
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Sort { .. })));
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Spot { .. })));
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Ladder { .. })));
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Match { .. })));
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Choice { .. } | Exercise::Tiles { .. })));
    assert!(s.exercises.len() <= 18);
    assert_eq!(s.course.term_lang, "es");
    assert!(!s.course.chars.is_empty());
}

#[test]
fn every_lesson_builds_a_session() {
    let courses = crate::courses::Courses::load_builtin(Path::new("data")).expect("built-in courses load");
    assert!(!courses.builtin().is_empty());
    for c in courses.builtin().iter().map(|c| courses.builtin_store(c.id()).expect("builds")) {
        let c = &*c;
        let mut rng = Rng::seeded(3);
        for l in c.lessons() {
            let s = lesson_session(c, l, &HashSet::new(), &mut rng);
            assert!(s.exercises.iter().filter(|e| e.is_graded()).count() >= 5, "{}/{}", c.id(), l.slug);
            serde_json::to_string(&s).unwrap();
        }
        let s = review_session(c, &HashMap::new(), &HashSet::new(), &Default::default(), 0, 0, &mut rng);
        assert!(s.exercises.len() >= 8, "{}", c.id());
        let s = lightning_session(c, &HashMap::new(), &HashSet::new(), &HashSet::new(), 0, 0, &mut rng);
        assert_eq!(s.timed_secs, Some(LIGHTNING_SECS));
        assert!(s.exercises.iter().all(|e| e.is_graded()));
    }
}

/// A quiz exercise's id and the tag shown above it.
fn quiz_tag(e: &Exercise) -> Option<(&str, &str)> {
    match e {
        Exercise::Choice { id, lesson, .. } | Exercise::Tiles { id, lesson, .. } => Some((id, lesson)),
        _ => None,
    }
}

#[test]
fn review_mix_draws_from_read_lessons_then_a_taste_of_whats_next() {
    let c = ContentStore::load_dir(Path::new("data/test-courses/test-course")).unwrap();
    let lesson_of = |qid: &str| c.lessons().iter().find(|l| l.quiz.iter().any(|q| q.question_id == qid)).unwrap();
    let taste = |l: &Lesson| crate::t1("sprint.session.taste_tag", "lesson", &l.title);
    let quizzed: Vec<&Lesson> = c.lessons().iter().filter(|l| !l.quiz.is_empty()).collect();
    let read_at = |ls: &[&Lesson]| -> HashMap<String, Progress> {
        ls.iter().map(|l| (l.id.clone(), Progress { read_at: Some(1), ..Progress::new(&l.id) })).collect()
    };

    // One short lesson read: all of it, then the next lessons on the path, tagged.
    let read = *quizzed.iter().find(|l| l.quiz.len() < REVIEW_QUIZ_LEN).expect("a lesson with fewer questions than a round");
    let progress = read_at(&[read]);
    for seed in 0..10 {
        let s = review_session(&c, &progress, &HashSet::new(), &Default::default(), 0, 0, &mut Rng::seeded(seed));
        let quiz: Vec<(&str, &str)> = s.exercises.iter().filter_map(quiz_tag).collect();
        assert_eq!(quiz.len(), REVIEW_QUIZ_LEN);
        let (mine, next): (Vec<_>, Vec<_>) = quiz.iter().partition(|(id, _)| lesson_of(id).id == read.id);
        assert_eq!(mine.len(), read.quiz.len(), "every question of the read lesson");
        assert!(mine.iter().all(|(_, tag)| *tag == read.title));
        // The taste comes from the first unread lessons in course order.
        let first_unread = quizzed.iter().find(|l| l.id != read.id).unwrap();
        for (id, tag) in &next {
            let l = lesson_of(id);
            assert!(!crate::quiz::is_read(&progress, l));
            assert_eq!(*tag, taste(l));
        }
        assert!(next.iter().any(|(id, _)| lesson_of(id).id == first_unread.id));
        assert_eq!(s.subtitle, crate::t("sprint.session.review_subtitle_taste"));
        // Grammar, words and listening come only from what was read.
        for e in &s.exercises {
            if quiz_tag(e).is_none() {
                let j = serde_json::to_value(e).unwrap();
                if let Some(tag) = j.get("lesson").and_then(|v| v.as_str()) {
                    assert!(tag == read.title || tag == crate::t("sprint.session.your_words"), "{tag}");
                }
            }
        }
    }

    // Nothing read: the whole quiz is a taste, and says so.
    let s = review_session(&c, &HashMap::new(), &HashSet::new(), &Default::default(), 0, 0, &mut Rng::seeded(1));
    let quiz: Vec<(&str, &str)> = s.exercises.iter().filter_map(quiz_tag).collect();
    assert_eq!(quiz.len(), REVIEW_QUIZ_LEN);
    assert!(quiz.iter().all(|(id, tag)| *tag == taste(lesson_of(id))));
    assert_eq!(s.subtitle, crate::t("sprint.session.review_subtitle_new"));

    // A round's worth read: no taste at all, only read lessons.
    let progress = read_at(&quizzed[..3]);
    let s = review_session(&c, &progress, &HashSet::new(), &Default::default(), 0, 0, &mut Rng::seeded(2));
    let quiz: Vec<(&str, &str)> = s.exercises.iter().filter_map(quiz_tag).collect();
    assert_eq!(quiz.len(), REVIEW_QUIZ_LEN);
    assert!(quiz.iter().all(|(id, tag)| crate::quiz::is_read(&progress, lesson_of(id)) && *tag == lesson_of(id).title));
    assert_eq!(s.subtitle, crate::t("sprint.session.review_subtitle"));
}

#[test]
fn a_course_without_paradigm_or_sprint_still_plays() {
    let c = crate::content::tests::store(&[r###"{"id":"l","slug":"l","title":"Rust ownership","unit":"A1","group":"basics",
            "content":"## Moves\n\nA value has one owner.\n\n## Borrows\n\nReferences borrow.",
            "vocabulary":[{"term":"owner","meaning":"the variable that holds a value"},{"term":"borrow","meaning":"a reference"},{"term":"move","meaning":"transfer of ownership"}],
            "quiz":[{"question_id":"q1","prompt":"let b = a; moves or copies a String?","type":"multiple-choice","options":["moves","copies"],"correct_answer":"moves","explanation":"String is not Copy."}]}"###])
    .unwrap();
    let l = &c.lessons()[0];
    let s = lesson_session(&c, l, &HashSet::new(), &mut Rng::seeded(2));
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Bite { .. })));
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Choice { .. })));
    assert!(s.exercises.iter().any(|e| matches!(e, Exercise::Match { .. })));
    assert!(!s.exercises.iter().any(|e| matches!(e, Exercise::Ladder { .. })));
    assert_eq!(s.course.term_label, "Español");
}

#[test]
fn lesson_session_listens_in_language_courses() {
    let c = store();
    let l = c.lesson_by_slug("ser-vs-estar").unwrap();
    let mut rng = Rng::seeded(11);
    let s = lesson_session(&c, l, &HashSet::new(), &mut rng);
    let listening = s.exercises.iter().filter(|e| e.is_listening()).count();
    assert!((3..=4).contains(&listening), "{listening} listening exercises");
    assert!(s.exercises.len() <= 18);
    // Each family is dealt from the lesson's own material.
    let pool = listening_exercises(&c, l, &mut rng);
    assert!(pool.iter().all(|e| e.is_listening()));
    for n in 0..l.listening.len() {
        if let Some(Exercise::HearBuild { text, tiles, .. }) = hear_build(l, n, &mut rng) {
            assert!(tiles.len() > words_of(&text).len(), "distractors added");
        }
        if let Some(Exercise::HearGap { text, gap, tiles, .. }) = hear_gap(l, n, None, &mut rng) {
            let word = bare(&words_of(&text)[gap]);
            assert!(tiles.iter().any(|t| normalize(t) == normalize(&word)));
            assert!(tiles.len() >= 2 && tiles.len() <= 4);
        }
    }
    match hear_pick(l, 0, &mut rng).unwrap() {
        Exercise::HearPick { text, options, to_meaning, .. } => {
            assert_eq!(text, l.vocabulary[0].term);
            assert!(options.contains(&text) && options.len() >= 2 && !to_meaning);
        }
        other => panic!("{other:?}"),
    }
    match hear_which(l, 0, &mut rng).unwrap() {
        Exercise::HearWhich { prompt, options, .. } => {
            assert_eq!(prompt, l.vocabulary[0].meaning);
            assert!(options.contains(&l.vocabulary[0].term) && options.len() <= 3);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(hear_match(l, &mut rng), Some(Exercise::HearMatch { .. })));
    // The review mix listens too (from the lessons read).
    let read = HashMap::from([(l.id.clone(), Progress { read_at: Some(1), ..Progress::new(&l.id) })]);
    let s = review_session(&c, &read, &HashSet::new(), &Default::default(), 0, 0, &mut rng);
    assert!(s.exercises.iter().any(|e| e.is_listening()));
    // Lightning never does: audio is too slow for one-tap questions.
    let s = lightning_session(&c, &HashMap::new(), &HashSet::new(), &HashSet::new(), 0, 0, &mut rng);
    assert!(!s.exercises.iter().any(|e| e.is_listening()));
}

#[test]
fn no_listening_outside_language_courses() {
    // A course whose terms and meanings share a language, even with listening sentences in a lesson.
    let c = &crate::content::tests::plain_store();
    assert!(!c.lessons()[0].listening.is_empty());
    let mut rng = Rng::seeded(5);
    for l in c.lessons() {
        assert!(listening_exercises(c, l, &mut rng).is_empty());
        let s = lesson_session(c, l, &HashSet::new(), &mut rng);
        assert!(!s.exercises.iter().any(|e| e.is_listening()));
    }
}

#[test]
fn hear_build_tiles_carry_no_punctuation() {
    // Issue #413: "mano." on a tile gave the word order away.
    assert_eq!(tile_words("¿Dónde está la mano?"), ["Dónde", "está", "la", "mano"]);
    assert_eq!(tile_words("«Hola», dijo — y se fue..."), ["Hola", "dijo", "y", "se", "fue"]);
    assert_eq!(tile_words("Peut-être qu'il vient, ¡l'été!"), ["Peut-être", "qu'il", "vient", "l'été"]);
    let edge = |w: &str| w.chars().next().is_some_and(|c| !c.is_alphanumeric()) || w.chars().last().is_some_and(|c| !c.is_alphanumeric());
    let mut built = 0;
    for c in [store(), ContentStore::load_dir(Path::new("data/courses/french")).expect("french loads")] {
        for l in c.lessons() {
            for (n, item) in l.listening.iter().enumerate() {
                let Some(Exercise::HearBuild { id, tiles, .. }) = hear_build(l, n, &mut Rng::seeded(n as u64)) else { continue };
                assert!(!tiles.iter().any(|t| edge(t)), "{id}: punctuation on a tile: {tiles:?}");
                // The sentence's own tiles in order, as the page sends them, are right.
                let answer = tile_words(&item.text).join(" ");
                assert!(check(&c, &id, &serde_json::json!(answer), 0).unwrap().correct, "{id}: {answer:?} not taken for {:?}", item.text);
                built += 1;
            }
        }
    }
    assert!(built > 50, "only {built} hear-builds");
}

#[test]
fn grades_listening_exercises() {
    let c = store();
    let l = c.lesson_by_slug("ser-vs-estar").unwrap();
    let item = &l.listening[0];
    let g = check(&c, &format!("hbuild:{}:0", l.id), &serde_json::json!(item.text), 0).unwrap();
    assert!(g.correct && matches!(g.effect, Effect::Mastery { .. }));
    let g = check(&c, &format!("hbuild:{}:0", l.id), &serde_json::json!("nada"), 0).unwrap();
    assert!(!g.correct && g.correct_answer == item.text);

    let g = check(&c, &format!("htype:{}:0", l.id), &serde_json::json!(item.text.to_uppercase()), 0).unwrap();
    assert!(g.correct);
    let g = check(&c, &format!("htype:{}:0", l.id), &serde_json::json!(strip_accents(&item.text)), 0).unwrap();
    if strip_accents(&item.text) != item.text {
        assert!(!g.correct && g.note.as_deref() == Some("Almost! Check the accents."));
    }
    let g = check(&c, &format!("htype:{}:0", l.id), &serde_json::json!("hola"), 0).unwrap();
    assert!(!g.correct && g.note.unwrap().contains("words right"));

    let words = words_of(&item.text);
    let g = check(&c, &format!("hgap:{}:0:0", l.id), &serde_json::json!(bare(&words[0])), 0).unwrap();
    assert!(g.correct && g.correct_answer == bare(&words[0]));
    assert!(check(&c, &format!("hgap:{}:0:99", l.id), &serde_json::json!("x"), 0).is_err());

    let g = check(&c, &format!("hmean:{}:0", l.id), &serde_json::json!(item.meaning), 0).unwrap();
    assert!(g.correct);
    let g = check(&c, &format!("hmean:{}:0", l.id), &serde_json::json!("something else"), 0).unwrap();
    assert!(!g.correct && g.correct_answer == item.meaning);

    let v = &l.vocabulary[0];
    let g = check(&c, &format!("hpick:{}:0", l.id), &serde_json::json!(v.term), 0).unwrap();
    assert!(g.correct);
    assert_eq!(g.effect, Effect::Vocab { lesson_id: l.id.clone(), known: vec![v.term.clone()], missed: vec![] });
    let g = check(&c, &format!("hwhich:{}:0", l.id), &serde_json::json!("nope"), 0).unwrap();
    assert!(!g.correct);
    assert_eq!(g.effect, Effect::Vocab { lesson_id: l.id.clone(), known: vec![], missed: vec![v.term.clone()] });

    let g = check(&c, &format!("hmatch:{}", l.id), &serde_json::json!({ "known": [v.term], "missed": [] }), 0).unwrap();
    assert!(g.correct);
    assert_eq!(g.effect, Effect::Vocab { lesson_id: l.id.clone(), known: vec![v.term.clone()], missed: vec![] });

    let (dl, prompt, answer) = describe(&c, &format!("hgap:{}:0:0", l.id)).unwrap();
    assert_eq!(dl.id, l.id);
    assert!(prompt.starts_with("Fill the gap") && answer == bare(&words[0]));
    assert!(describe(&c, &format!("htype:{}:0", l.id)).is_some());
    assert!(describe(&c, &format!("htype:{}:99", l.id)).is_none());
}

#[test]
fn tiles_include_accent_trap() {
    let c = store();
    let l = c.lesson_by_slug("ser-vs-estar").unwrap();
    let q = l.quiz.iter().find(|q| q.kind == QuizType::FillInBlank && q.correct_answer == "están").unwrap();
    let mut rng = Rng::seeded(1);
    match quiz_exercise(&c, l, q, &mut rng) {
        Exercise::Tiles { tiles, .. } => {
            assert!(tiles.iter().any(|t| t == "están"));
            assert!(tiles.iter().any(|t| t == "estan"));
            assert!(tiles.len() <= MAX_TILES);
        }
        other => panic!("expected tiles, got {other:?}"),
    }
}

#[test]
fn numeric_tiles_never_offer_an_accepted_answer() {
    // Issue #680: "29" (accepted for February) turned up as a wrong tile.
    let c = ContentStore::load_dir(Path::new("data/test-courses/test-course")).expect("test course loads");
    let l = c.lesson_by_slug("numeric").unwrap();
    let q = l.quiz.iter().find(|q| q.question_id == "tc-numeric-05").unwrap();
    assert_eq!(q.accepted_answers, ["29"]);
    for seed in 0..200 {
        let tiles = numeric_tiles(l, q, &mut Rng::seeded(seed));
        assert!(tiles.iter().any(|t| t == "28"), "seed {seed}: {tiles:?}");
        assert!(!tiles.iter().any(|t| t == "29"), "seed {seed}: {tiles:?}");
    }
}

#[test]
fn numeric_tiles_offer_one_right_tile() {
    // A slip inside the tolerance ("12.58" for 12.57, tolerance 0.01) is graded right too.
    let c = ContentStore::load_dir(Path::new("data/test-courses/test-course")).expect("test course loads");
    let l = c.lesson_by_slug("numeric").unwrap();
    for q in l.quiz.iter().filter(|q| q.kind == QuizType::Numeric) {
        for seed in 0..200 {
            let tiles = numeric_tiles(l, q, &mut Rng::seeded(seed));
            let right: Vec<&String> = tiles.iter().filter(|t| crate::quiz::check_answer(q, t).correct).collect();
            assert!(tiles.is_empty() || right.len() == 1, "{} seed {seed}: right tiles {right:?} of {tiles:?}", q.question_id);
        }
    }
}

#[test]
fn gap_tiles_come_only_from_fill_in_blank_answers() {
    // Issue #410: "True" from a true/false item turned up as a verb-form tile.
    let c = store();
    let mut l = c.lesson_by_slug("ser-vs-estar").unwrap().clone();
    let fib = l.quiz.iter().find(|q| q.kind == QuizType::FillInBlank).unwrap().clone();
    let mut tf = fib.clone();
    tf.question_id = "t-tf".into();
    tf.kind = QuizType::TrueFalse;
    tf.correct_answer = "True".into();
    tf.options = vec!["True".into(), "False".into()];
    let mut mc = fib.clone();
    mc.question_id = "t-mc".into();
    mc.kind = QuizType::MultipleChoice;
    mc.correct_answer = "Zanahoria".into();
    mc.options = vec!["Zanahoria".into(), "Pepino".into()];
    l.quiz.push(tf);
    l.quiz.push(mc);
    // Without the paradigm tiles there is room for every other answer.
    l.conjugations.clear();
    let mut seen_other = false;
    for seed in 0..40 {
        for q in l.quiz.iter().filter(|q| q.kind == QuizType::FillInBlank) {
            match quiz_exercise(&c, &l, q, &mut Rng::seeded(seed)) {
                Exercise::Tiles { tiles, .. } => {
                    for bad in ["True", "False", "Zanahoria", "Pepino"] {
                        assert!(!tiles.iter().any(|t| t == bad), "{bad} among the tiles of {}: {tiles:?}", q.question_id);
                    }
                    seen_other |= tiles.iter().any(|t| l.quiz.iter().any(|o| o.question_id != q.question_id && o.kind == QuizType::FillInBlank && *t == o.correct_answer));
                }
                other => panic!("expected tiles, got {other:?}"),
            }
        }
    }
    assert!(seen_other, "other fill-in-blank answers still make tiles");
}

#[test]
fn gap_tiles_never_give_the_answer_away() {
    // Issue #678: since #525 some gaps had one tile (the answer), or two (it
    // and its accent-less spelling). A bank has three tiles that differ by
    // more than accents, or there is none and the answer is typed.
    let mut typed = 0;
    let mut banks = 0;
    for dir in ["data/courses/spanish", "data/courses/french", "data/test-courses/test-course"] {
        let c = ContentStore::load_dir(Path::new(dir)).expect("course loads");
        for l in c.lessons() {
            for q in l.quiz.iter().filter(|q| q.kind == QuizType::FillInBlank) {
                for seed in 0..3 {
                    let Exercise::Tiles { tiles, .. } = quiz_exercise(&c, l, q, &mut Rng::seeded(seed)) else { panic!("{}: not tiles", q.question_id) };
                    if tiles.is_empty() {
                        typed += 1;
                        continue;
                    }
                    banks += 1;
                    let distinct: HashSet<String> = tiles.iter().map(|t| strip_accents(&normalize(t))).collect();
                    assert!(distinct.len() >= 3, "{}: {tiles:?}", q.question_id);
                    assert!(tiles.contains(&q.correct_answer), "{}: no answer among {tiles:?}", q.question_id);
                    // No padding tile is graded right (the accent-less spelling may be: a near miss).
                    let plain = strip_accents(&q.correct_answer);
                    let right: Vec<&String> = tiles.iter().filter(|t| **t != q.correct_answer && **t != plain && crate::quiz::check_answer(q, t).correct).collect();
                    assert!(right.is_empty(), "{}: other tiles are right too: {right:?}", q.question_id);
                }
            }
        }
    }
    assert!(banks > 10 * typed, "{banks} banks, {typed} typed");
}

#[test]
fn grades_sort_spot_build_ladder_pick_match() {
    let c = store();
    let l = c.lesson_by_slug("ser-vs-estar").unwrap();
    let spec = &l.sprint.sorts[0];
    let right: Vec<serde_json::Value> =
        spec.items.iter().map(|it| serde_json::json!({ "text": it.text, "bucket": it.bucket })).collect();
    let g = check(&c, &format!("sort:{}:0", l.id), &serde_json::Value::Array(right.clone()), 0).unwrap();
    assert!(g.correct);
    assert!(matches!(g.effect, Effect::Mastery { .. }));
    let g = check(&c, &format!("sort:{}:0", l.id), &serde_json::Value::Array(right), 2).unwrap();
    assert!(!g.correct && g.note.is_some());

    let spot = &l.sprint.spots[0];
    let words = words_of(&spot.sentence);
    let idx = words.iter().position(|w| normalize(w) == normalize(&spot.wrong)).unwrap();
    let g = check(&c, &format!("spot:{}:0", l.id), &serde_json::json!(idx), 0).unwrap();
    assert!(g.correct);
    let g = check(&c, &format!("spot:{}:0", l.id), &serde_json::json!((idx + 1) % words.len()), 0).unwrap();
    assert!(!g.correct);

    let b = &l.sprint.builds[0];
    let g = check(&c, &format!("build:{}:0", l.id), &serde_json::json!(b.text), 0).unwrap();
    assert!(g.correct);
    let g = check(&c, &format!("build:{}:0", l.id), &serde_json::json!(strip_accents(&b.text)), 0).unwrap();
    assert!(!g.correct);

    let k = &l.conjugations[0];
    let forms: Vec<String> = c.paradigm().unwrap().slots.iter().map(|s| k.form(&s.key).to_string()).collect();
    let id = format!("ladder:{}:{}:present", l.id, k.item);
    let g = check(&c, &id, &serde_json::json!(forms), 0).unwrap();
    assert!(g.correct);
    assert_eq!(g.note.as_deref(), Some(format!("{n} of {n} forms right", n = forms.len()).as_str()));
    assert_eq!(g.effect, Effect::Mastery { lesson_id: l.id.clone(), selected: format!("{n}/{n}", n = forms.len()) });
    // Graded at Check (#640): wrong taps from an older client change nothing.
    let g = check(&c, &id, &serde_json::json!(forms), 3).unwrap();
    assert!(g.correct);
    assert_eq!(g.effect, Effect::Mastery { lesson_id: l.id.clone(), selected: format!("{n}/{n}", n = forms.len()) });
    // One rung wrong: two forms swapped (or a made-up one when they are the same).
    let mut swapped = forms.clone();
    if swapped[0] != swapped[1] { swapped.swap(0, 1); } else { swapped[0] = "xx".into(); }
    let right = forms.iter().zip(&swapped).filter(|(a, b)| a == b).count();
    let g = check(&c, &id, &serde_json::json!(swapped), 0).unwrap();
    assert!(!g.correct);
    assert_eq!(g.note.as_deref(), Some(format!("{right} of {} forms right", forms.len()).as_str()));
    assert!(g.correct_answer.contains(&forms[0]), "the key shows the right forms");

    let v = &l.vocabulary[0];
    let g = check(&c, &format!("pick:s:{}:0", l.id), &serde_json::json!(v.term), 0).unwrap();
    assert!(g.correct);
    assert_eq!(g.effect, Effect::Vocab { lesson_id: l.id.clone(), known: vec![v.term.clone()], missed: vec![] });

    let g = check(&c, &format!("match:{}", l.id), &serde_json::json!({ "known": [v.term], "missed": ["nope"] }), 0).unwrap();
    assert!(!g.correct);
    assert_eq!(g.effect, Effect::Vocab { lesson_id: l.id.clone(), known: vec![v.term.clone()], missed: vec![] });

    assert!(check(&c, "bogus:x", &serde_json::json!(1), 0).is_err());
    assert!(check(&c, "sort:nope:0", &serde_json::json!([]), 0).is_err());
}

#[test]
fn describes_history_ids() {
    let c = store();
    let (l, prompt, answer) = describe(&c, "spot:a1-ser-vs-estar:0").unwrap();
    assert_eq!(l.slug, "ser-vs-estar");
    assert!(prompt.starts_with("Spot the mistake: "));
    assert!(answer.contains("→"));
    assert!(describe(&c, "a1-ser-vs-estar-01").is_none());
    assert!(describe(&c, "ladder:a1-ser-vs-estar:ser:present").is_some());
    assert!(describe(&c, "sort:nope:0").is_none());
}

#[test]
fn history_rows_for_sorts_and_ladders_read_as_a_score() {
    let c = store();
    let l = c.lessons().iter().find(|l| !l.sprint.sorts.is_empty()).expect("a lesson with a sort");
    let id = format!("sort:{}:0", l.id);
    let n = l.sprint.sorts[0].items.len();
    // Nothing points at the item count as if it were the answer.
    assert_eq!(describe(&c, &id).unwrap().2, "");
    // A sort graded through `check` stores a score, read back in words.
    let all: Vec<_> = l.sprint.sorts[0].items.iter().map(|it| serde_json::json!({"text": it.text, "bucket": it.bucket})).collect();
    let g = check(&c, &id, &serde_json::json!(all), 2).unwrap();
    let Effect::Mastery { selected, .. } = g.effect else { panic!("mastery effect") };
    assert_eq!(selected, format!("{n}/{n} +2"));
    assert_eq!(history_answer(&c, &id, &selected), format!("2 wrong taps, then all {n} sorted correctly"));
    assert_eq!(history_answer(&c, &id, "3/8"), "Sorted 3 of 8 correctly");
    // Rows stored before: the old wording reads the same way, taps first when all ended right.
    assert_eq!(history_answer(&c, &id, "sorted with 1 slip(s)"), format!("1 wrong tap, then all {n} sorted correctly"));
    assert_eq!(history_answer(&c, &id, "sorted wrongly"), "Some in the wrong group");
    assert_eq!(history_answer(&c, "ladder:x:ser:present", "5/6 rungs right"), "5 of 6 forms right");
    // Ladders graded at Check (#640) store a plain score; rows from before keep their taps.
    assert_eq!(history_answer(&c, "ladder:x:comer:present", "2/3"), "2 of 3 forms right");
    assert_eq!(history_answer(&c, "ladder:x:comer:present", "3/3"), "3 of 3 forms right");
    assert_eq!(history_answer(&c, "ladder:x:comer:present", "3/3 +1"), "1 wrong tap, then all 3 forms right");
    assert_eq!(history_answer(&c, "ladder:x:comer:present", "6/6 +2"), "2 wrong taps, then all 6 forms right");
    assert_eq!(history_answer(&c, "ladder:x:comer:present", "2/3 +1"), "2 of 3 forms right · 1 wrong tap");
    assert_eq!(history_answer(&c, "ladder:x:ser:present", "6/6 rungs right"), "6 of 6 forms right");
    assert_eq!(history_answer(&c, "spot:x:0", "casa"), "casa");
}

#[test]
fn chilis_follow_mastery() {
    assert_eq!(chilis(0.0), 0);
    assert_eq!(chilis(30.0), 1);
    assert_eq!(chilis(60.0), 2);
    assert_eq!(chilis(85.0), 3);
}

#[test]
fn streaks_follow_local_days() {
    use crate::activity::{record_at, Kind};
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        let day = 86_400;
        let now = 1_700_000_000 - (1_700_000_000 % day) + 3600; // 01:00 UTC
        assert_eq!(day_streak(c, uid, "es", 0, now)?, (0, false));
        for d in (1..=3).rev() {
            record_at(c, uid, "es", Kind::Answer, 1, now - d * day + 7200, 0)?;
        }
        // Yesterday, the day before and the one before that: streak of 3, nothing today yet.
        assert_eq!(day_streak(c, uid, "es", 0, now)?, (3, false));
        // Another course's activity does not count.
        assert_eq!(day_streak(c, uid, "fr", 0, now)?, (0, false));
        // An answer at 23:00 UTC yesterday is still yesterday in UTC...
        record_at(c, uid, "es", Kind::Answer, 1, now - 7200, 0)?;
        assert_eq!(day_streak(c, uid, "es", 0, now)?, (3, false));
        // ...but at 04:00 five hours east it is today's, so there the
        // streak is 4 and today is done (the day is the learner's at the
        // time the effort is logged).
        record_at(c, uid, "es", Kind::Answer, 1, now - 7200, 5 * 3600)?;
        assert_eq!(day_streak(c, uid, "es", 5 * 3600, now)?, (4, true));
        // A gap of two days breaks it.
        assert_eq!(day_streak(c, uid, "es", 0, now + 2 * day)?, (0, false));
        assert_eq!(local_day_start(0, now), now - 3600);
        Ok(())
    })
    .unwrap();
}

#[test]
fn strips_tags() {
    assert_eq!(strip_tags("<p>Hola <em>mundo</em> &amp; adiós</p>"), "Hola mundo & adiós");
    assert_eq!(truncate_words("one two three four", 10), "one two…");
}

#[test]
fn only_a_language_course_talks() {
    // Issue #1015: Sprint read tapped tiles aloud in maths and general courses.
    // `say_lang` is what sprint.js speaks in; empty means silent, and the
    // player page leaves out its speaker switch.
    use askama::Template;
    let spanish = store();
    assert_eq!(CourseInfo::of(&spanish).say_lang, spanish.manifest().terms.lang);
    let fractions = ContentStore::from_files(&crate::courses::starter_files("fractions", "Fractions")).expect("starter loads");
    assert!(!fractions.manifest().is_language());
    assert_eq!(CourseInfo::of(&fractions).say_lang, "");

    let page = |c: &ContentStore| {
        crate::routes::sprint::PlayTemplate {
            title: "t".into(),
            course_title: c.title().to_string(),
            course: c.view(),
            session_json: "{}".into(),
            notation: false,
            sound: false,
            staff_note: false,
            keyboard: false,
        }
        .render()
        .unwrap()
    };
    assert!(page(&spanish).contains("data-testid=\"sprint-play-audio\""));
    assert!(!page(&fractions).contains("data-testid=\"sprint-play-audio\""));
}

#[test]
fn a_music_course_keeps_its_builds() {
    // Issue #1232: a music course (`music` in course.json) with the language
    // features off still plays a lesson's builds, here the circle of fifths.
    let mut files = crate::courses::starter_files("music", "Music");
    for f in files.iter_mut() {
        let mut v: serde_json::Value = serde_json::from_str(&f.content).unwrap();
        if f.path == "course.json" {
            v["language_features"] = serde_json::json!(false);
            v["music"] = serde_json::json!({ "note_names": "letters" });
        } else {
            v["sprint"]["builds"] = serde_json::json!([{ "text": "C G D A E B", "meaning": "Each key adds one sharp." }]);
        }
        f.content = v.to_string();
    }
    let c = ContentStore::from_files(&files).expect("music course loads");
    assert!(!c.manifest().is_language());
    let l = c.lessons().first().unwrap();
    for seed in 0..10 {
        let s = lesson_session(&c, l, &HashSet::new(), &mut Rng::seeded(seed));
        let tiles = s.exercises.iter().find_map(|e| match e { Exercise::Build { meaning, tiles, .. } if meaning == "Each key adds one sharp." => Some(tiles.clone()), _ => None });
        let mut tiles = tiles.unwrap_or_else(|| panic!("seed {seed}: no build"));
        tiles.sort();
        assert_eq!(tiles, ["A", "B", "C", "D", "E", "G"]);
    }
}

#[test]
fn speaker_tiles_an_octave_apart_are_graded_apart() {
    // In ABC, case and the marks ' and , are the octave: C and c are different notes (#1219).
    let mut files = crate::content::read_course_dir(Path::new("data/test-courses/test-course")).unwrap();
    let ear = files.iter_mut().find(|f| f.path.ends_with("ear.json")).unwrap();
    let mut lesson: serde_json::Value = serde_json::from_str(&ear.content).unwrap();
    lesson["sprint"]["sorts"] = serde_json::json!([{ "title": "High or low?", "buckets": ["low", "high"], "items": [
        { "play": "C", "bucket": 0 }, { "play": "c", "bucket": 1 }, { "play": "C,", "bucket": 0 }, { "play": "c'", "bucket": 1 }
    ] }]);
    ear.content = lesson.to_string();
    let c = ContentStore::from_files(&files).unwrap();
    let l = c.lesson_by_slug("ear").unwrap();
    let right: Vec<serde_json::Value> = l.sprint.sorts[0].items.iter().map(|it| serde_json::json!({ "text": it.play, "bucket": it.bucket })).collect();
    let g = check(&c, &format!("sort:{}:0", l.id), &serde_json::Value::Array(right), 0).unwrap();
    assert!(g.correct, "{:?}", g.note);
    let swapped: Vec<serde_json::Value> = l.sprint.sorts[0].items.iter().map(|it| serde_json::json!({ "text": it.play, "bucket": 1 - it.bucket })).collect();
    assert!(!check(&c, &format!("sort:{}:0", l.id), &serde_json::Value::Array(swapped), 0).unwrap().correct);
}

#[test]
fn a_spot_on_a_staff_is_graded_on_the_note_and_its_fix() {
    let c = ContentStore::load_dir(Path::new("data/test-courses/test-course")).expect("test course loads");
    let l = c.lesson_by_slug("notation").unwrap();
    let id = format!("spot:{}:0", l.id);
    let mut rng = Rng::seeded(3);
    let Some(Exercise::Spot { words, abc, tiles, .. }) = spot::exercise(&c, l, 0, &mut rng) else { panic!("no spot") };
    assert!(words.is_empty() && abc.contains("CDEG GABc"));
    let mut names: Vec<&str> = tiles.iter().map(|t| t.name.as_str()).collect();
    names.sort();
    // The fix F and its neighbours, never the wrong note (G) itself.
    assert_eq!(names, ["A", "E", "F"]);
    let grade = |note: usize, fix: &str| check(&c, &id, &serde_json::json!({ "note": note, "fix": fix }), 0).unwrap();
    let g = grade(3, "F");
    assert!(g.correct && g.note.is_none());
    assert_eq!(g.correct_answer, "Note 4 should be F");
    let g = grade(3, "E");
    assert!(!g.correct && g.note.as_deref() == Some("You found the note, but not its fix."));
    let g = grade(4, "F");
    assert!(!g.correct && g.note.is_none());
    assert!(check(&c, &id, &serde_json::json!(3), 0).is_err(), "a staff spot wants a note and a fix");
    let (prompt, answer) = describe(&c, &id).map(|(_, p, a)| (p, a)).unwrap();
    assert!(prompt.contains("C D E G G A B C") && answer == "Note 4 should be F", "{prompt} / {answer}");
}

#[test]
fn a_spot_on_a_staff_is_validated() {
    let spot = |v: serde_json::Value| -> crate::content::SpotSpec { serde_json::from_value(v).unwrap() };
    let ok = spot(serde_json::json!({ "abc": "X:1\nK:C\nCDEG|", "wrong": 4, "fix": "F" }));
    assert_eq!(spot::validate(&ok), Ok(()));
    for (v, msg) in [
        (serde_json::json!({ "abc": "X:1\nK:C\nCDEG|", "wrong": 5, "fix": "F" }), "from 1 to 4"),
        (serde_json::json!({ "abc": "X:1\nK:C\nCDEG|", "wrong": 0, "fix": "F" }), "from 1 to 4"),
        (serde_json::json!({ "abc": "X:1\nK:C\nCDEG|", "wrong": 4, "fix": "F major" }), "one note in ABC"),
        (serde_json::json!({ "abc": "X:1\nK:C\nCDEG|", "wrong": 4, "fix": "g" }), "already there"),
        (serde_json::json!({ "abc": "X:1\nK:C\nC|", "wrong": 1, "fix": "D" }), "at least two notes"),
        (serde_json::json!({ "abc": "X:1\nK:C\nCD|", "sentence": "C D", "wrong": 1, "fix": "E" }), "both a sentence and abc"),
        (serde_json::json!({ "sentence": "Mi madre está médica.", "wrong": "es", "fix": "es" }), "must appear exactly once"),
    ] {
        let e = spot::validate(&spot(v)).unwrap_err();
        assert!(e.contains(msg), "{e}");
    }
}

#[test]
fn a_course_with_neither_language_nor_music_drops_its_builds() {
    // Issue #1312: with the language features off and no `music` block, a
    // lesson's builds stay valid but Sprint does not play them.
    let mut files = crate::courses::starter_files("plain", "Plain");
    for f in files.iter_mut() {
        let mut v: serde_json::Value = serde_json::from_str(&f.content).unwrap();
        if f.path == "course.json" {
            v["language_features"] = serde_json::json!(false);
            v.as_object_mut().unwrap().remove("music");
        } else {
            v["sprint"]["builds"] = serde_json::json!([{ "text": "C G D A E B", "meaning": "Each key adds one sharp." }]);
        }
        f.content = v.to_string();
    }
    let c = ContentStore::from_files(&files).expect("plain course loads");
    assert!(!c.manifest().is_language() && c.manifest().music.is_none());
    let l = c.lessons().first().unwrap();
    assert_eq!(l.sprint.builds.len(), 1);
    for seed in 0..10 {
        let s = lesson_session(&c, l, &HashSet::new(), &mut Rng::seeded(seed));
        assert!(!s.exercises.iter().any(|e| matches!(e, Exercise::Build { .. })), "seed {seed}: a build was played");
    }
}
