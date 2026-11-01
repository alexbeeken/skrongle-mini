use super::*;

fn lines(s: &str) -> Vec<String> {
    split(s)
}

fn apply_ops(a: &[String], b: &[String], ops: &[Op]) -> (Vec<String>, Vec<String>) {
    let (mut i, mut j) = (0, 0);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for op in ops {
        match op {
            Op::Same => {
                assert_eq!(a[i], b[j]);
                left.push(a[i].clone());
                right.push(b[j].clone());
                i += 1;
                j += 1;
            }
            Op::Del => {
                left.push(a[i].clone());
                i += 1;
            }
            Op::Add => {
                right.push(b[j].clone());
                j += 1;
            }
        }
    }
    (left, right)
}

fn run_file(path: &str, before: Option<&str>, after: Option<&str>) -> AiRunFile {
    AiRunFile { path: path.into(), before: before.map(str::to_string), after: after.map(str::to_string) }
}

fn none() -> Exclusions {
    Exclusions::default()
}

fn ne() -> Edits {
    Edits::default()
}

#[test]
fn the_diff_is_a_longest_common_subsequence() {
    let cases = [("a\nb\nc\nd", "a\nx\nc\nd\ne"), ("", "a\nb"), ("a\nb", ""), ("a\nb\nc", "c\nb\na"), ("x\ny\nx\ny\nx", "y\nx\ny"), ("same", "same")];
    for (x, y) in cases {
        let (a, b) = (lines(x), lines(y));
        let ops = diff(&a, &b);
        // Every line of both sides is used, in order.
        assert_eq!(apply_ops(&a, &b, &ops), (a.clone(), b.clone()), "{x:?} -> {y:?}");
        let same = ops.iter().filter(|o| **o == Op::Same).count();
        // Brute force LCS length.
        let mut t = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for i in 1..=a.len() {
            for j in 1..=b.len() {
                t[i][j] = if a[i - 1] == b[j - 1] { t[i - 1][j - 1] + 1 } else { t[i - 1][j].max(t[i][j - 1]) };
            }
        }
        assert_eq!(same, t[a.len()][b.len()], "{x:?} -> {y:?}");
    }
}

#[test]
fn an_added_line_is_green_on_the_right_only_and_far_context_folds() {
    let before: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    let after = before.replace("line 10\n", "line 10\nnew line\n");
    let f = file(&run_file("pictures/x.svg", Some(&before), Some(&after)), &none(), &ne(), 0);
    assert_eq!((f.kind, f.added, f.removed), ("changed", 1, 0));
    let changed: Vec<&Row> = f.rows.iter().filter(|r| r.kind == "change").collect();
    assert_eq!(changed.len(), 1);
    assert_eq!((changed[0].left.kind, changed[0].right.kind, changed[0].right.pre.as_str()), ("empty", "add", "new line"));
    assert_eq!(changed[0].key, "0-11");
    // 3 lines of context each side, the rest folded.
    assert_eq!(f.rows.first().map(|r| (r.kind, r.gap)), Some(("gap", 7)));
    assert_eq!(f.rows.last().map(|r| (r.kind, r.gap)), Some(("gap", 7)));
    assert_eq!(f.rows.len(), 1 + 3 + 1 + 3 + 1);
}

#[test]
fn whole_files_are_one_column_and_json_reads_as_labelled_text() {
    let added = file(&run_file("lessons/a.json", None, Some(r#"{"title":"Fractions","id":"a","quiz":[{"question":"Half of 4?"}]}"#)), &none(), &ne(), 2);
    assert_eq!((added.kind, added.title.as_str(), added.position), ("added", "Fractions", 2));
    assert!(added.rows.iter().all(|r| r.left.kind == "empty" && r.right.kind == "add"));
    assert_eq!(added.summary, "New, with 1 quiz question.");
    let q = added.rows.iter().find(|r| r.right.label == "Question").unwrap();
    assert_eq!((q.right.pre.as_str(), q.right.raw.trim(), q.right.indent), ("Half of 4?", r#""question": "Half of 4?""#, 3));
    assert!(added.rows[0].right.bare, "a line of brackets only");
    let removed = file(&run_file("lessons/b.json", Some("{}"), None), &none(), &ne(), 0);
    assert_eq!((removed.kind, removed.removed, removed.summary.as_str()), ("removed", 1, "Taken out of the course."));
    let changed = file(&run_file("course.json", Some(r#"{"id":"c","title":"Old"}"#), Some("{\n  \"id\": \"c\",\n  \"title\": \"New\"\n}")), &none(), &ne(), 0);
    assert_eq!((changed.kind, changed.title.as_str(), changed.added, changed.removed), ("changed", "Course settings", 1, 1));
    // The pair marks only what differs in the value.
    let pair = changed.rows.iter().find(|r| r.kind == "change").unwrap();
    assert_eq!((pair.left.label.as_str(), pair.left.pre.as_str(), pair.left.mid.as_str(), pair.left.post.as_str()), ("Title", "", "Old", ""));
    assert_eq!((pair.right.pre.as_str(), pair.right.mid.as_str()), ("", "New"));
}

#[test]
fn summaries_say_in_words_what_changed() {
    let before = r#"{"id":"a","title":"Halves","content":"Old text","quiz":[{"q":1},{"q":2},{"q":3}],"vocabulary":[{"w":"a"}]}"#;
    let after = r#"{"id":"a","title":"Halves and quarters","content":"New text","quiz":[{"q":1},{"q":9},{"q":3},{"q":4}],"vocabulary":[{"w":"a"}],"correct_answer":"x"}"#;
    assert_eq!(
        summary("lessons/a.json", Some(before), Some(after)),
        "Renamed it “Halves and quarters”; rewrote the reading; rewrote 1 quiz question; added 1 quiz question; added the correct answer."
    );
    assert_eq!(summary("lessons/a.json", Some(r#"{"quiz":[1,2]}"#), Some(r#"{"quiz":[2,1]}"#)), "Reordered the quiz questions.");
    assert_eq!(summary("lessons/a.json", Some(r#"{"a":1}"#), Some("{\n  \"a\": 1\n}")), "Only its layout changed.");
    assert_eq!(summary("lessons/a.json", None, Some(r#"{"content":"x","vocabulary":[1,2]}"#)), "New, with a reading and 2 words.");
}

#[test]
fn files_are_named_like_the_lesson_table_and_listed_in_its_order() {
    assert_eq!(title_of("course.json", "{}"), "Course settings");
    assert_eq!(title_of("lessons/a.json", r#"{"id":"a","title":"Halves"}"#), "Halves");
    assert_eq!(title_of("lessons/a.json", r#"{"id":"halves","title":" "}"#), "halves");
    assert_eq!(title_of("lessons/unit/halves.json", "not json"), "halves");
    let run = AiRun {
        label: "l".into(),
        created_at: 0,
        excluded: String::new(),
        edits: String::new(), why: String::new(),
        files: vec![
            run_file("course.json", Some("{}"), Some(r#"{"a":1}"#)),
            run_file("lessons/a.json", Some("{}"), None),
            run_file("lessons/b.json", None, Some("{}")),
            run_file("lessons/c.json", Some("{}"), Some(r#"{"a":1}"#)),
        ],
    };
    let positions = HashMap::from([("lessons/b.json".to_string(), 2), ("lessons/c.json".to_string(), 1)]);
    let order: Vec<String> = files(&run, &none(), &ne(), &positions).into_iter().map(|f| f.path).collect();
    assert_eq!(order, ["course.json", "lessons/c.json", "lessons/b.json", "lessons/a.json"]);
    assert_eq!(overview(&files(&run, &none(), &ne(), &positions)), "The AI added 1 lesson, changed 1, removed 1 and changed the course settings.");
}

#[test]
fn the_form_says_what_is_left_out() {
    let pairs: Vec<(String, String)> = [("file", "lessons/a.json"), ("lines", "lessons/b.json:3-4,0-5"), ("lines", "lessons/b.json:0-5,7-0"), ("lines", "bad"), ("lines", "x.json:a-b"), ("quiet", "1")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let x = Exclusions::from_form(&pairs);
    assert_eq!(x.files, ["lessons/a.json"]);
    // One key in one group only; bad keys and empty groups dropped.
    assert_eq!(x.lines.get("lessons/b.json").unwrap(), &vec![vec!["3-4".to_string(), "0-5".to_string()], vec!["7-0".to_string()]]);
    assert_eq!(x.lines.len(), 1);
    assert_eq!(Exclusions::from_json(&x.to_json()), x);
    assert_eq!(Exclusions::default().to_json(), "");
    assert_eq!(Exclusions::from_json(""), Exclusions::default());
}

fn lesson(quiz: &[&str]) -> String {
    serde_json::to_string_pretty(&serde_json::json!({"id": "a", "title": "Halves", "quiz": quiz})).unwrap()
}

#[test]
fn leaving_lines_out_keeps_the_rest_and_mends_the_json() {
    let before = lesson(&["one", "two"]);
    let after = lesson(&["one", "TWO", "three"]);
    let f = run_file("lessons/a.json", Some(&before), Some(&after));
    let review = file(&f, &none(), &ne(), 1);
    let keys: Vec<(String, String, String)> = review.rows.iter().filter(|r| r.kind == "change").map(|r| (r.key.clone(), r.left.raw.trim().into(), r.right.raw.trim().into())).collect();
    // "two" → "TWO," and a new "three".
    assert_eq!(keys, [("5-5".into(), r#""two""#.into(), r#""TWO","#.into()), ("0-6".into(), String::new(), r#""three""#.into())]);
    let out = |groups: Vec<Vec<&str>>| {
        let mut x = Exclusions::default();
        x.lines.insert(f.path.clone(), groups.into_iter().map(|g| g.into_iter().map(str::to_string).collect()).collect());
        let text = result(&f, "Halves", &x, &ne()).unwrap().unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["quiz"].clone()
    };
    // Leaving out the new line takes the comma off the line before it.
    assert_eq!(out(vec![vec!["0-6"]]), serde_json::json!(["one", "TWO"]));
    // Leaving out the rewrite keeps "two" and still adds "three".
    assert_eq!(out(vec![vec!["5-5"]]), serde_json::json!(["one", "two", "three"]));
    // Everything left out: the file as it was, byte for byte.
    let mut all = Exclusions::default();
    all.lines.insert(f.path.clone(), vec![vec!["5-5".into(), "0-6".into()]]);
    assert_eq!(result(&f, "Halves", &all, &ne()).unwrap().as_deref(), Some(before.as_str()));
    assert_eq!(result(&f, "Halves", &none(), &ne()).unwrap().as_deref(), Some(after.as_str()));
    // Shown ticked, in their group.
    let shown = file(&f, &all, &ne(), 1);
    assert!(shown.rows.iter().filter(|r| r.kind == "change").all(|r| r.excluded && r.group == 1));
}

#[test]
fn half_an_object_left_out_is_refused_in_words() {
    let before = "{\n  \"id\": \"a\"\n}";
    let after = "{\n  \"id\": \"a\",\n  \"extra\": {\n    \"x\": 1\n  }\n}";
    let f = run_file("lessons/a.json", Some(before), Some(after));
    let mut x = Exclusions::default();
    // Leave out only the line opening the object: its close is left alone.
    let key = file(&f, &none(), &ne(), 0).rows.iter().find(|r| r.right.raw.contains("\"extra\"")).unwrap().key.clone();
    x.lines.insert(f.path.clone(), vec![vec![key]]);
    let e = result(&f, "Halves", &x, &ne()).unwrap_err();
    assert!(e.starts_with("Leaving out those lines of “Halves” leaves it unreadable"), "{e}");
}

#[test]
fn applying_leaves_out_files_and_refuses_ones_edited_since() {
    let cf = |p: &str, c: &str| CourseFile { path: p.into(), content: c.into() };
    let run = AiRun {
        label: "l".into(),
        created_at: 0,
        excluded: String::new(),
        edits: String::new(), why: String::new(),
        files: vec![
            run_file("lessons/new.json", None, Some(r#"{"id":"n"}"#)),
            run_file("lessons/gone.json", Some(r#"{"id":"g"}"#), None),
            run_file("lessons/kept.json", Some(r#"{"id":"k","title":"Old"}"#), Some(r#"{"id":"k","title":"New"}"#)),
        ],
    };
    let now = vec![cf("course.json", "{}"), cf("lessons/new.json", r#"{"id":"n"}"#), cf("lessons/kept.json", r#"{"id":"k","title":"New"}"#)];
    assert!(apply(&run, (&none(), &ne()), (&none(), &ne()), &now).unwrap().is_none(), "nothing left out: already so");
    let x = Exclusions { files: vec!["lessons/new.json".into(), "lessons/gone.json".into(), "lessons/kept.json".into()], lines: BTreeMap::new() };
    let mut out = apply(&run, (&x, &ne()), (&none(), &ne()), &now).unwrap().unwrap();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    let paths: Vec<(&str, &str)> = out.iter().map(|f| (f.path.as_str(), f.content.as_str())).collect();
    assert_eq!(paths, [("course.json", "{}"), ("lessons/gone.json", r#"{"id":"g"}"#), ("lessons/kept.json", r#"{"id":"k","title":"Old"}"#)]);
    // Taken back in again from there: the AI's version once more.
    let pairs = |files: Vec<CourseFile>| {
        let mut v: Vec<(String, String)> = files.into_iter().map(|f| (f.path, f.content)).collect();
        v.sort();
        v
    };
    assert_eq!(pairs(apply(&run, (&none(), &ne()), (&x, &ne()), &out).unwrap().unwrap()), pairs(now.clone()));
    // Edited by hand since: refused, naming the lesson.
    let edited = vec![cf("course.json", "{}"), cf("lessons/new.json", r#"{"id":"n"}"#), cf("lessons/kept.json", r#"{"id":"k","title":"Mine"}"#)];
    let only_kept = Exclusions { files: vec!["lessons/kept.json".into()], lines: BTreeMap::new() };
    let e = apply(&run, (&only_kept, &ne()), (&none(), &ne()), &edited).unwrap_err();
    assert!(e.starts_with("“Mine” has been changed since the AI run") || e.starts_with("“New” has been changed since the AI run"), "{e}");
    // Nothing asked of the edited lesson (left out as last time): it is left alone, not refused.
    assert!(apply(&run, (&none(), &ne()), (&none(), &ne()), &edited).unwrap().is_none());
    // The whole run undone (every file as before it): a plain Accept changes nothing and is not refused.
    let undone = vec![cf("course.json", "{}"), cf("lessons/gone.json", r#"{"id":"g"}"#), cf("lessons/kept.json", r#"{"id":"k","title":"Old"}"#)];
    assert!(apply(&run, (&none(), &ne()), (&none(), &ne()), &undone).unwrap().is_none());
    // After an Undo of an Accept (the AI's version back), the same selection applies again.
    assert_eq!(pairs(apply(&run, (&x, &ne()), (&x, &ne()), &now).unwrap().unwrap()), pairs(out.clone()));
}

#[test]
fn lesson_table_saves_since_the_run_do_not_block_leaving_out() {
    // #280: the table writes "order" (and unit, title, prerequisites) into lessons it saves.
    let cf = |p: &str, c: &str| CourseFile { path: p.into(), content: c.into() };
    let before = r#"{"id":"k","title":"Old","content":"Old text","quiz":[]}"#;
    let after = r#"{"id":"k","title":"Old","content":"New text","quiz":["q1"]}"#;
    let run = AiRun { label: "l".into(), created_at: 0, excluded: String::new(), edits: String::new(), why: String::new(), files: vec![run_file("lessons/k.json", Some(before), Some(after))] };
    let pretty = |s: &str| serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(s).unwrap()).unwrap();
    let rows = file(&run.files[0], &none(), &ne(), 1).rows;
    let reading = rows.iter().find(|r| r.kind == "change" && r.right.label == "Reading").unwrap().key.clone();
    let x = Exclusions { files: vec![], lines: BTreeMap::from([("lessons/k.json".to_string(), vec![vec![reading]])]) };
    // Renumbered and renamed in the table since: the reading is left out, the table's changes stay.
    let tabled = cf("lessons/k.json", &(pretty(r#"{"id":"k","title":"Renamed","content":"New text","quiz":["q1"],"order":1}"#) + "\n"));
    let out = apply(&run, (&x, &ne()), (&none(), &ne()), &[tabled.clone()]).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&out[0].content).unwrap();
    assert_eq!(v, serde_json::json!({"id":"k","title":"Renamed","content":"Old text","quiz":["q1"],"order":1}));
    // Taken back in from there: the AI's reading, still with the table's changes.
    let back = apply(&run, (&none(), &ne()), (&x, &ne()), &out).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&back[0].content).unwrap();
    assert_eq!(v, serde_json::json!({"id":"k","title":"Renamed","content":"New text","quiz":["q1"],"order":1}));
    // A change outside the table's keys is still refused.
    let by_hand = cf("lessons/k.json", r#"{"id":"k","title":"Old","content":"Mine","quiz":["q1"],"order":1}"#);
    assert!(apply(&run, (&x, &ne()), (&none(), &ne()), &[by_hand]).unwrap_err().contains("has been changed since the AI run"));
    // So is one where the review changes a key the table changed too.
    let tx = Exclusions { files: vec!["lessons/k.json".into()], lines: BTreeMap::new() };
    let run2 = AiRun { files: vec![run_file("lessons/k.json", Some(before), Some(r#"{"id":"k","title":"AI title","content":"New text","quiz":["q1"]}"#))], ..run.clone() };
    let renamed = cf("lessons/k.json", r#"{"id":"k","title":"Mine","content":"New text","quiz":["q1"]}"#);
    assert!(apply(&run2, (&tx, &ne()), (&none(), &ne()), &[renamed]).is_err());
}

#[test]
fn an_undone_run_is_seen_as_undone() {
    // #281: every file as before the run; not when the review itself left everything out.
    let cf = |p: &str, c: &str| CourseFile { path: p.into(), content: c.into() };
    let run = AiRun {
        label: "l".into(),
        created_at: 0,
        excluded: String::new(),
        edits: String::new(), why: String::new(),
        files: vec![run_file("lessons/new.json", None, Some(r#"{"id":"n"}"#)), run_file("lessons/k.json", Some(r#"{"id":"k","title":"Old"}"#), Some(r#"{"id":"k","title":"New"}"#))],
    };
    let undone = [cf("course.json", "{}"), cf("lessons/k.json", r#"{"id":"k","title":"Old"}"#)];
    assert!(super::undone(&run, (&none(), &ne()), &undone));
    // Renumbered in the table after the Undo: still undone.
    assert!(super::undone(&run, (&none(), &ne()), &[cf("lessons/k.json", r#"{"id":"k","title":"Old","order":1}"#)]));
    assert!(!super::undone(&run, (&none(), &ne()), &[cf("lessons/new.json", r#"{"id":"n"}"#), cf("lessons/k.json", r#"{"id":"k","title":"New"}"#)]));
    let all_out = Exclusions { files: vec!["lessons/new.json".into(), "lessons/k.json".into()], lines: BTreeMap::new() };
    assert!(!super::undone(&run, (&all_out, &ne()), &undone), "the review left it all out: not an Undo");
}

#[test]
fn labels_are_the_plain_names_of_the_course_format() {
    // #282
    assert_eq!(label("prompt"), "Question");
    assert_eq!(label("content"), "Reading");
    assert_eq!(label("correct_answer"), "Answer");
    assert_eq!(label("slug"), "Web address");
    assert_eq!(label("example_meaning"), "Example meaning");
    let c = readable(r#"    "question_id": "q-1","#, true);
    assert!(c.quiet && c.label == "Id");
    assert!(!readable(r#"    "prompt": "Half of 4?","#, true).quiet);
    let m: serde_json::Value = serde_json::from_str(&labels_json()).unwrap();
    assert_eq!(m["prompt"], "Question");
}

#[test]
fn any_line_on_the_right_can_be_edited_as_text() {
    // The text as Accept writes it: trailing blanks and blank lines go; the line as it was, or nothing, is no edit.
    assert_eq!(edited_lines(r#"  "title": "Halves","#, "  \"name\": \"Halves\",  \n").unwrap(), [r#"  "name": "Halves","#]);
    assert_eq!(edited_lines("  [", "  [\n    \"one\",\n\n  ]").unwrap(), ["  [", r#"    "one","#, "  ]"]);
    assert_eq!(edited_lines(r#"  "n": 1"#, r#"  "n": 1"#), None);
    assert_eq!(edited_lines(r#"  "n": 1"#, " \n "), None);
    let pairs: Vec<(String, String)> = [("edit:lessons/a.json:5-5", "Two\r\nlines"), ("edit:lessons/a.json:bad", "x"), ("edit:", "x"), ("lines", "lessons/a.json:5-5")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let e = Edits::from_form(&pairs);
    assert_eq!(e.get("lessons/a.json", "5-5"), Some("Two\nlines"));
    assert_eq!(e.0.values().map(BTreeMap::len).sum::<usize>(), 1);
    assert_eq!(Edits::from_json(&e.to_json()), e);
    assert_eq!(Edits::default().to_json(), "");
}

#[test]
fn edits_go_in_with_accept_and_an_edited_line_is_always_in() {
    let before = lesson(&["one", "two"]);
    let after = lesson(&["one", "TWO", "three"]);
    let f = run_file("lessons/a.json", Some(&before), Some(&after));
    let mut ed = Edits::default();
    ed.0.entry(f.path.clone()).or_default().insert("5-5".into(), r#""Two, mine","#.into());
    let quiz = |x: &Exclusions, e: &Edits| serde_json::from_str::<serde_json::Value>(&result(&f, "Halves", x, e).unwrap().unwrap()).unwrap()["quiz"].clone();
    assert_eq!(quiz(&none(), &ed), serde_json::json!(["one", "Two, mine", "three"]));
    // The new line left out too: the edit stays, the comma is mended.
    let mut x = Exclusions::default();
    x.lines.insert(f.path.clone(), vec![vec!["0-6".into()]]);
    assert_eq!(quiz(&x, &ed), serde_json::json!(["one", "Two, mine"]));
    // The edited line left out with the new one: the edit stays in, the new line goes.
    x.lines.insert(f.path.clone(), vec![vec!["5-5".into(), "0-6".into()]]);
    assert_eq!(quiz(&x, &ed), serde_json::json!(["one", "Two, mine"]));
    // Its lesson left out as a whole: the lesson stays, with the edit (only lines left out go).
    let whole = Exclusions { files: vec![f.path.clone()], lines: x.lines.clone() };
    assert_eq!(quiz(&whole, &ed), serde_json::json!(["one", "Two, mine"]));
    assert_eq!(result(&f, "Halves", &Exclusions { files: vec![f.path.clone()], lines: BTreeMap::new() }, &ne()).unwrap().as_deref(), Some(before.as_str()));
    // The line as the AI wrote it, with or without its indent, changes nothing.
    let mut same = Edits::default();
    same.0.entry(f.path.clone()).or_default().insert("5-5".into(), r#"    "TWO","#.into());
    assert_eq!(result(&f, "Halves", &none(), &same).unwrap().as_deref(), Some(after.as_str()));
    // An unchanged line can be edited too, into several lines (the comma it lacked is mended),
    // and a key can be renamed: it is the whole line that is edited.
    let mut text = Edits::default();
    text.0.entry(f.path.clone()).or_default().insert("4-4".into(), "\"uno\",\n\"dos\"\n".into());
    text.0.entry(f.path.clone()).or_default().insert("7-8".into(), r#""name": "Halves""#.into());
    let out = serde_json::from_str::<serde_json::Value>(&result(&f, "Halves", &none(), &text).unwrap().unwrap()).unwrap();
    assert_eq!(out, serde_json::json!({"id": "a", "name": "Halves", "quiz": ["uno", "dos", "TWO", "three"]}));
    // Shown: the edit in place of the AI's text (the AI's kept), read like any line; never shown left out, and counted.
    let shown = file(&f, &none(), &ed, 1);
    let row = shown.rows.iter().find(|r| r.key == "5-5").unwrap();
    assert_eq!((row.right.editable, row.right.edited, row.right.pre.as_str(), row.right.ai.as_str()), (true, true, "Two, mine", r#"    "TWO","#));
    assert_eq!(row.right.raw, r#""Two, mine","#);
    assert_eq!(row.right.edit_name, "edit:lessons/a.json:5-5");
    assert!(shown.rows.iter().filter(|r| r.kind == "same").all(|r| r.right.editable && !r.left.editable));
    assert_eq!(left_out(&[shown]), "1 line edited");
    let lines = file(&f, &none(), &text, 1);
    let row = lines.rows.iter().find(|r| r.key == "4-4").unwrap();
    assert_eq!((row.right.pre.as_str(), row.right.more.len(), row.right.more[0].pre.as_str(), row.right.raw.as_str()), ("uno", 1, "dos", "\"uno\",\n\"dos\""));
    assert_eq!(lines.rows.iter().find(|r| r.key == "7-8").unwrap().right.label, "Name");
    assert_eq!(left_out(&[lines]), "2 lines edited");
    let shown_out = file(&f, &whole, &ed, 1);
    assert!(!shown_out.excluded, "a lesson with an edited line is not left out");
    let rows: Vec<(&str, bool)> = shown_out.rows.iter().filter(|r| r.kind == "change").map(|r| (r.key.as_str(), r.excluded)).collect();
    assert_eq!(rows, [("5-5", false), ("0-6", true)]);
    assert_eq!(left_out(&[shown_out]), "1 change left out, 1 line edited");
    // Accept writes it; accepting again is a no-op, and taking the edit back restores the AI's.
    let run = AiRun { label: "l".into(), created_at: 0, excluded: String::new(), edits: String::new(), why: String::new(), files: vec![f.clone()] };
    let now = vec![CourseFile { path: f.path.clone(), content: after.clone() }];
    let out = apply(&run, (&none(), &ed), (&none(), &ne()), &now).unwrap().unwrap();
    assert!(out[0].content.contains("Two, mine"));
    assert!(apply(&run, (&none(), &ed), (&none(), &ed), &out).unwrap().is_none());
    assert_eq!(apply(&run, (&none(), &ne()), (&none(), &ed), &out).unwrap().unwrap()[0].content, after);
    // An edit that breaks the JSON is refused by lesson name.
    let mut bad = Edits::default();
    bad.0.entry(f.path.clone()).or_default().insert("5-5".into(), "two".into());
    assert!(result(&f, "Halves", &none(), &bad).unwrap_err().starts_with("Your edits to “Halves” leave it unreadable"));
}

#[test]
fn mending_fixes_commas_only() {
    assert_eq!(mend(&["[", "  1,", "]"]), "[\n  1\n]");
    assert_eq!(mend(&["[", "  1", "  2", "]"]), "[\n  1,\n  2\n]");
    assert_eq!(mend(&["{", "  \"a\": [", "    1", "  ]", "  \"b\": 2", "}"]), "{\n  \"a\": [\n    1\n  ],\n  \"b\": 2\n}");
}
