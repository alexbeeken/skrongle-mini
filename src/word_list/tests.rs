use super::*;

fn manifest() -> Manifest {
    serde_json::from_value(serde_json::json!({
        "id": "x", "title": "X",
        "terms": { "label": "Deutsch", "lang": "de" },
        "meanings": { "label": "English", "lang": "en" },
        "units": [{ "id": "1", "title": "One" }]
    }))
    .unwrap()
}

fn pair(t: &str, m: &str) -> Pair {
    Pair { term: t.into(), meaning: m.into() }
}

#[test]
fn reads_dashes_commas_equals_and_tabs() {
    let p = parse("hallo — hello\ntschüss – bye\ndanke - thanks\nbitte = please\nja, yes\nnein\tno\tsome tag\n", &manifest());
    assert_eq!(p.rows, vec![pair("hallo", "hello"), pair("tschüss", "bye"), pair("danke", "thanks"), pair("bitte", "please"), pair("ja", "yes"), pair("nein", "no")]);
    assert!(p.skipped.is_empty());
}

#[test]
fn splits_at_the_first_separator_of_the_strongest_kind() {
    let p = parse("sein — to be, to exist\nder Tee, tea\ngehen: to go\nes ist 10:30 = it is half past ten\n", &manifest());
    assert_eq!(p.rows, vec![pair("sein", "to be, to exist"), pair("der Tee", "tea"), pair("gehen", "to go"), pair("es ist 10:30", "it is half past ten")]);
}

#[test]
fn a_hyphen_inside_a_word_does_not_split_it() {
    let p = parse("die E-Mail — e-mail\n", &manifest());
    assert_eq!(p.rows, vec![pair("die E-Mail", "e-mail")]);
    let p = parse("E-Mail\n", &manifest());
    assert!(p.rows.is_empty());
    assert_eq!(p.skipped[0].why, Skip::NoSplit);
}

#[test]
fn flags_lines_it_cannot_read_with_their_line_number() {
    let p = parse("hallo — hello\n\njust a word\n — no term\nnein, \n", &manifest());
    assert_eq!(p.rows, vec![pair("hallo", "hello")]);
    let lines: Vec<(usize, &str)> = p.skipped.iter().map(|s| (s.line, s.text.as_str())).collect();
    assert_eq!(lines, vec![(3, "just a word"), (4, "— no term"), (5, "nein,")]);
    assert!(p.skipped.iter().all(|s| s.why == Skip::NoSplit));
}

#[test]
fn skips_a_header_row_named_by_the_course_labels() {
    let p = parse("Deutsch\tEnglish\nhallo\thello\n", &manifest());
    assert_eq!(p.rows, vec![pair("hallo", "hello")]);
    let p = parse("term, meaning\nhallo, hello\n", &manifest());
    assert_eq!(p.rows, vec![pair("hallo", "hello")]);
    // Only the first line can be a header.
    let p = parse("hallo, hello\nterm, meaning\n", &manifest());
    assert_eq!(p.rows.len(), 2);
}

#[test]
fn strips_bullets_numbers_and_csv_quotes() {
    let p = parse("- hallo - hello\n* danke — thanks\n• ja — yes\n1. nein — no\n12) bitte — please\n\"der Hund\",\"dog\"\n", &manifest());
    assert_eq!(p.rows, vec![pair("hallo", "hello"), pair("danke", "thanks"), pair("ja", "yes"), pair("nein", "no"), pair("bitte", "please"), pair("der Hund", "dog")]);
}

#[test]
fn a_repeated_term_is_flagged_not_added_twice() {
    let p = parse("hallo — hello\nHallo — hi\n", &manifest());
    assert_eq!(p.rows, vec![pair("hallo", "hello")]);
    assert_eq!(p.skipped, vec![Skipped { line: 2, text: "Hallo — hi".into(), why: Skip::Duplicate }]);
}

#[test]
fn stops_at_the_word_cap() {
    let text: String = (0..MAX_WORDS + 2).map(|i| format!("w{i} — m{i}\n")).collect();
    let p = parse(&text, &manifest());
    assert_eq!(p.rows.len(), MAX_WORDS);
    assert_eq!(p.skipped.len(), 2);
    assert!(p.skipped.iter().all(|s| s.why == Skip::TooMany));
}

#[test]
fn the_lesson_holds_the_words_as_vocabulary_and_a_table_headed_with_the_labels() {
    let m = manifest();
    let json = lesson(" Greetings ", &[pair("hallo", "hello"), pair("a|b", "c")], &m);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["title"], "Greetings");
    assert_eq!(v["vocabulary"], serde_json::json!([{ "term": "hallo", "meaning": "hello" }, { "term": "a|b", "meaning": "c" }]));
    let content = v["content"].as_str().unwrap();
    assert!(content.starts_with("## Greetings\n\n| Deutsch | English |\n|---|---|\n| hallo | hello |\n"), "{content}");
    assert!(content.contains("| a\\|b | c |"), "{content}");
}

#[test]
fn the_lesson_goes_in_through_the_lesson_table_and_loads() {
    use crate::content::{ContentStore, CourseFile, MANIFEST_FILE};
    let m = manifest();
    let first = serde_json::json!({ "id": "a", "slug": "a", "title": "A", "unit": "1", "content": "## A\n\nText." });
    let files = vec![
        CourseFile { path: MANIFEST_FILE.into(), content: serde_json::to_string(&m).unwrap() },
        CourseFile { path: "lessons/a.json".into(), content: first.to_string() },
    ];
    let words = parse("hallo — hello\ndanke — thanks\n", &m).rows;
    let rows = [
        crate::arrange::Edit { key: Some("a".into()), unit: "1".into(), title: "A".into(), content: None },
        // The table fills in id, slug, unit and group, as for any new lesson.
        crate::arrange::Edit { key: None, unit: "1".into(), title: "Greetings".into(), content: Some(lesson("Greetings", &words, &m)) },
    ];
    let (out, _, _) = crate::arrange::edit_rows(&files, &rows, None, None).unwrap();
    let store = ContentStore::from_files(&out).unwrap();
    let l = store.lessons().iter().find(|l| l.title == "Greetings").expect("the new lesson");
    assert_eq!(l.id, "greetings");
    assert_eq!(l.vocabulary.iter().map(|v| (v.term.as_str(), v.meaning.as_str())).collect::<Vec<_>>(), vec![("hallo", "hello"), ("danke", "thanks")]);
}
