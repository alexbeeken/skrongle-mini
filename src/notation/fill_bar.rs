//! Fill the bar (#1230). A `fill-bar` question's `abc` holds one invisible
//! rest (`x2`) that marks a gap in the bar; `tiles` are note values written
//! in ABC (`B2`, `BB`, `B/B/B`). The learner taps tiles into the gap, each
//! as often as they like, and the answer is the tiles tapped, separated by
//! spaces. It is right when the tiles add up to the gap, or, with
//! `"exact": true`, when they are `correct_answer` (or an accepted answer).
//! `static/fill-bar.js` reads the lengths the same way to draw the bar.
//!
//! Lengths understood: notes and rests with a length (`B2`, `B/`, `B3/2`,
//! `z`), chords (`[CEG]2`), duplets to quadruplets (`(3BBB`), and ties,
//! slurs, broken rhythm (`B>B`), grace notes, chord symbols and decorations,
//! which add nothing.

use crate::content::QuizItem;
use crate::quiz::Verdict;

/// A length as a fraction of the tune's unit note length (`L:`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Len {
    n: i64,
    d: i64,
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs().max(1) } else { gcd(b, a % b) }
}

impl Len {
    fn new(n: i64, d: i64) -> Len {
        let g = gcd(n, d);
        Len { n: n / g, d: d / g }
    }
    const ZERO: Len = Len { n: 0, d: 1 };
    fn add(self, o: Len) -> Len {
        Len::new(self.n * o.d + o.n * self.d, self.d * o.d)
    }
    fn mul(self, o: Len) -> Len {
        Len::new(self.n * o.n, self.d * o.d)
    }
}

/// A length suffix at the start of `s` (`2`, `/`, `//`, `3/2`, `/4`, or none):
/// the length and the bytes it took.
fn suffix(s: &[u8]) -> (Len, usize) {
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while s.get(*i).is_some_and(u8::is_ascii_digit) && *i - start < 4 {
            *i += 1;
        }
        std::str::from_utf8(s.get(start..*i).unwrap_or_default()).ok().and_then(|d| d.parse::<i64>().ok())
    };
    let n = digits(&mut i).unwrap_or(1);
    let mut d = 1;
    while s.get(i) == Some(&b'/') && d < 1024 {
        i += 1;
        d *= digits(&mut i).unwrap_or(2);
    }
    (Len::new(n, d), i)
}

/// The summed length of ABC notes (a tile), or what in it isn't one.
pub fn length(s: &str) -> Result<Len, String> {
    let b = s.as_bytes();
    let (mut i, mut total) = (0, Len::ZERO);
    // A tuplet in progress: notes left in it and the factor of each.
    let mut tuplet: Option<(u32, Len)> = None;
    let note = |len: Len, tuplet: &mut Option<(u32, Len)>, total: &mut Len| {
        let mut len = len;
        if let Some((left, f)) = tuplet {
            len = len.mul(*f);
            *left -= 1;
            if *left == 0 {
                *tuplet = None;
            }
        }
        *total = total.add(len);
    };
    let skip_to = |i: usize, end: u8| b.get(i + 1..).and_then(|r| r.iter().position(|&c| c == end)).map(|p| i + p + 2);
    // One pitch (accidentals, letter, octave marks) at `i`: where it ends.
    let pitch = |mut i: usize| -> Option<usize> {
        while matches!(b.get(i), Some(b'^' | b'_' | b'=')) {
            i += 1;
        }
        if !b.get(i).is_some_and(|c| matches!(c, b'A'..=b'G' | b'a'..=b'g' | b'z')) {
            return None;
        }
        i += 1;
        while matches!(b.get(i), Some(b',' | b'\'')) {
            i += 1;
        }
        Some(i)
    };
    while let Some(&c) = b.get(i) {
        match c {
            b' ' | b'\t' | b'-' | b'>' | b'<' | b')' | b'.' | b'~' => i += 1,
            b'(' => match b.get(i + 1) {
                Some(p @ b'2'..=b'4') => {
                    let p = u32::from(p - b'0');
                    let q = if p == 3 { 2 } else { 3 };
                    tuplet = Some((p, Len::new(q, i64::from(p))));
                    i += 2;
                }
                Some(d) if d.is_ascii_digit() => return Err(format!("'{s}': only (2, (3 and (4 tuplets are understood")),
                _ => i += 1,
            },
            b'{' | b'"' | b'!' => {
                let end = match c { b'{' => b'}', other => other };
                i = skip_to(i, end).ok_or_else(|| format!("'{s}': '{}' is not closed", c as char))?;
            }
            b'[' => {
                // A chord lasts as long as its first note, times the length after `]`.
                let mut j = i + 1;
                let mut first = None;
                while let Some(end) = pitch(j) {
                    let (len, used) = suffix(b.get(end..).unwrap_or_default());
                    first.get_or_insert(len);
                    j = end + used;
                }
                let first = first.filter(|_| b.get(j) == Some(&b']')).ok_or_else(|| format!("'{s}': a chord must be notes in [ ]"))?;
                let (len, used) = suffix(b.get(j + 1..).unwrap_or_default());
                note(first.mul(len), &mut tuplet, &mut total);
                i = j + 1 + used;
            }
            _ => {
                let end = pitch(i).ok_or_else(|| format!("'{s}': '{}' is not a note", s.get(i..).and_then(|r| r.chars().next()).unwrap_or(' ')))?;
                let (len, used) = suffix(b.get(end..).unwrap_or_default());
                note(len, &mut tuplet, &mut total);
                i = end + used;
            }
        }
    }
    Ok(total)
}

/// A fill-bar's music: the length of its gap and how many beats one unit
/// note length is (`L:` times the denominator of `M:`).
#[derive(Debug, PartialEq, Eq)]
pub struct Bar {
    pub gap: Len,
    pub beat: Len,
}

/// Read the gap of a fill-bar's `abc`: exactly one `x` in the music after
/// the `K:` line, outside chord symbols, decorations, comments and inline fields.
pub fn bar(abc: &str) -> Result<Bar, String> {
    let lines: Vec<&str> = abc.lines().collect();
    let k = lines.iter().position(|l| l.trim_start().starts_with("K:")).ok_or("abc needs a K: line")?;
    let field = |name: &str| lines.iter().take(k).find_map(|l| l.trim().strip_prefix(name)).map(str::trim);
    let frac = |s: &str| s.split_once('/').and_then(|(a, b)| Some((a.trim().parse::<i64>().ok()?, b.trim().parse::<i64>().ok()?))).filter(|(a, b)| *a > 0 && *b > 0);
    let meter = match field("M:") {
        Some("C") => Some((4, 4)),
        Some("C|") => Some((2, 2)),
        Some(m) => frac(m),
        None => None,
    };
    let unit = match (field("L:").and_then(frac), meter) {
        (Some((n, d)), _) => Len::new(n, d),
        (None, Some((a, b))) if a * 4 < b * 3 => Len::new(1, 16),
        _ => Len::new(1, 8),
    };
    let beat = unit.mul(Len::new(meter.map_or(4, |m| m.1), 1));
    let body = lines.get(k + 1..).unwrap_or_default().join("\n");
    let b = body.as_bytes();
    let (mut i, mut gaps) = (0, Vec::new());
    while let Some(&c) = b.get(i) {
        let end = match c {
            b'"' | b'!' => Some(c),
            b'%' => Some(b'\n'),
            b'[' if b.get(i + 2) == Some(&b':') => Some(b']'),
            _ => None,
        };
        match end {
            Some(end) => i = b.get(i + 1..).and_then(|r| r.iter().position(|&x| x == end)).map_or(b.len(), |p| i + p + 2),
            None => {
                if c == b'x' {
                    gaps.push(suffix(b.get(i + 1..).unwrap_or_default()).0);
                }
                i += 1;
            }
        }
    }
    match gaps.as_slice() {
        [gap] if gap.n > 0 => Ok(Bar { gap: *gap, beat }),
        [] => Err("abc needs an invisible rest (x2) marking the gap".into()),
        [_] => Err("the gap (x) must have a length".into()),
        _ => Err("abc may have only one invisible rest (x): the gap".into()),
    }
}

/// A length in beats, as a learner reads it: 2, 1.5, 0.333.
fn beats(len: Len, beat: Len) -> String {
    let b = len.mul(beat);
    let s = format!("{:.3}", b.n as f64 / b.d as f64);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The tiles an answer (tiles separated by spaces) is made of, and their
/// summed length; None when a part is not one of `tiles`.
fn read(tiles: &[String], answer: &str) -> Option<(Vec<String>, Len)> {
    let parts: Vec<String> = answer.split_whitespace().map(String::from).collect();
    let mut total = Len::ZERO;
    for p in &parts {
        tiles.iter().find(|t| *t == p)?;
        total = total.add(length(p).ok()?);
    }
    Some((parts, total))
}

/// The fewest tiles (each as often as needed) that fill the gap exactly.
fn fewest(tiles: &[String], gap: Len) -> Option<String> {
    let lens: Vec<Len> = tiles.iter().filter_map(|t| length(t).ok()).collect();
    let den = lens.iter().fold(gap.d, |d, l| d / gcd(d, l.d) * l.d);
    let target = usize::try_from(gap.n * (den / gap.d)).ok().filter(|t| *t <= 4096)?;
    let steps: Vec<usize> = lens.iter().map(|l| usize::try_from(l.n * (den / l.d)).unwrap_or(0)).collect();
    // best[s]: the fewest tiles that last s, and the last of them.
    let mut best: Vec<Option<(usize, usize)>> = vec![None; target + 1];
    if let Some(b) = best.first_mut() {
        *b = Some((0, usize::MAX));
    }
    for s in 1..=target {
        let pick = steps
            .iter()
            .enumerate()
            .filter(|(_, &st)| st > 0 && st <= s)
            .filter_map(|(i, &st)| best.get(s - st).copied().flatten().map(|(n, _)| (n + 1, i)))
            .min_by_key(|(n, _)| *n);
        if let Some(b) = best.get_mut(s) {
            *b = pick;
        }
    }
    let mut out = Vec::new();
    let mut s = target;
    while s > 0 {
        let (_, i) = best.get(s).copied().flatten()?;
        out.push(tiles.get(i)?.clone());
        s -= steps.get(i)?;
    }
    out.reverse();
    Some(out.join(" "))
}

/// Validate a fill-bar question: the gap, the tiles, and that some tiles
/// fill it (and the given answers do).
pub fn check(q: &QuizItem) -> Result<(), String> {
    let bar = bar(&q.abc)?;
    if q.tiles.len() < 2 {
        return Err("a fill-bar question needs at least 2 tiles".into());
    }
    for (i, t) in q.tiles.iter().enumerate() {
        if t.split_whitespace().count() != 1 {
            return Err(format!("tile '{t}' must be one group of notes, without spaces"));
        }
        if q.tiles.iter().take(i).any(|x| x == t) {
            return Err(format!("tile '{t}' is there twice"));
        }
        if length(t)?.n <= 0 {
            return Err(format!("tile '{t}' has no length"));
        }
    }
    if q.exact && q.correct_answer.trim().is_empty() {
        return Err("an exact fill-bar question needs correct_answer: the tiles that fill the gap".into());
    }
    for a in std::iter::once(&q.correct_answer).chain(&q.accepted_answers).filter(|a| !a.trim().is_empty()) {
        match read(&q.tiles, a) {
            None => return Err(format!("answer '{a}' must be tiles of the question, separated by spaces")),
            Some((_, len)) if len != bar.gap => {
                return Err(format!("answer '{a}' does not fill the gap: {} of {} beats", beats(len, bar.beat), beats(bar.gap, bar.beat)))
            }
            _ => {}
        }
    }
    if fewest(&q.tiles, bar.gap).is_none() {
        return Err(format!("no tiles add up to the gap of {} beats", beats(bar.gap, bar.beat)));
    }
    Ok(())
}

/// A fill-bar question without `correct_answer` gets the fewest tiles that
/// fill its gap, so every mode has an answer to show.
pub fn answer_missing(quiz: &mut [QuizItem]) {
    for q in quiz.iter_mut().filter(|q| q.kind == crate::content::QuizType::FillBar && q.correct_answer.trim().is_empty()) {
        if let Some(a) = bar(&q.abc).ok().and_then(|b| fewest(&q.tiles, b.gap)) {
            q.correct_answer = a;
        }
    }
}

/// Grade a fill-bar answer. A wrong length says how many beats went in.
pub fn grade(q: &QuizItem, given: &str) -> Verdict {
    let wrong = |note: Option<String>| Verdict { correct: false, note };
    let (Ok(bar), Some((parts, len))) = (bar(&q.abc), read(&q.tiles, given)) else { return wrong(None) };
    if len != bar.gap {
        return wrong(Some(crate::t2("quiz.check.fill_bar_beats", "given", beats(len, bar.beat), "gap", beats(bar.gap, bar.beat))));
    }
    let same = |a: &String| a.split_whitespace().eq(parts.iter().map(String::as_str));
    if q.exact && !std::iter::once(&q.correct_answer).chain(&q.accepted_answers).any(same) {
        return wrong(Some(crate::t("quiz.check.fill_bar_exact")));
    }
    Verdict { correct: true, note: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(abc: &str, tiles: &[&str], extra: serde_json::Value) -> QuizItem {
        let mut v = serde_json::json!({ "question_id": "q", "prompt": "Fill the bar.", "type": "fill-bar", "abc": abc, "tiles": tiles });
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }
    const ABC: &str = "X:1\nM:4/4\nL:1/4\nK:C\nB B x2|";

    #[test]
    fn note_lengths_add_up() {
        let l = |s: &str| length(s).unwrap();
        assert_eq!(l("B2"), Len::new(2, 1));
        assert_eq!(l("BB"), Len::new(2, 1));
        assert_eq!(l("B/B/B"), Len::new(2, 1));
        assert_eq!(l("B3/2B/"), Len::new(2, 1));
        assert_eq!(l("B//"), Len::new(1, 4));
        assert_eq!(l("^F,2 z"), Len::new(3, 1));
        assert_eq!(l("[CEG]2"), Len::new(2, 1));
        assert_eq!(l("[C2E2]"), Len::new(2, 1));
        assert_eq!(l("(3BBB"), Len::new(2, 1));
        assert_eq!(l("B>B"), Len::new(2, 1));
        assert_eq!(l("{g}B2-B"), Len::new(3, 1));
        assert_eq!(l("\"Am\"!f!A2"), Len::new(2, 1));
        for bad in ["H", "B|", "[CE", "(5BBBBB", "x2", "\"Am"] {
            assert!(length(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_gap_is_the_one_invisible_rest() {
        assert_eq!(bar(ABC).unwrap(), Bar { gap: Len::new(2, 1), beat: Len::new(1, 1) });
        // 6/8 counts in eighths; without L: a 6/8 tune's unit is an eighth.
        assert_eq!(bar("X:1\nM:6/8\nK:C\nB2 x4|").unwrap(), Bar { gap: Len::new(4, 1), beat: Len::new(1, 1) });
        // No M: or L:: the unit is an eighth, the beat a quarter.
        assert_eq!(bar("X:1\nK:C\n\"Cmaj7\"!p!B x/|").unwrap().beat, Len::new(1, 2));
        assert!(bar("X:1\nM:4/4\nL:1/4\nK:C\nB B B B|").unwrap_err().contains("invisible rest"));
        assert!(bar("X:1\nM:4/4\nL:1/4\nK:C\nB x x2|").unwrap_err().contains("only one"));
        assert!(bar("X:1\nM:4/4\nB x2|").unwrap_err().contains("K:"));
    }

    #[test]
    fn any_tiles_that_fill_the_gap_are_right() {
        let q = item(ABC, &["B2", "BB", "B/B/B", "B4", "B"], serde_json::json!({}));
        check(&q).unwrap();
        for right in ["B2", "BB", "B/B/B", "B B"] {
            assert!(grade(&q, right).correct, "{right}");
        }
        let v = grade(&q, "B4");
        assert!(!v.correct);
        assert_eq!(v.note.unwrap(), crate::t2("quiz.check.fill_bar_beats", "given", "4", "gap", "2"));
        assert!(!grade(&q, "B").correct, "one beat is short");
        assert_eq!(grade(&q, "B BB").note.unwrap(), crate::t2("quiz.check.fill_bar_beats", "given", "3", "gap", "2"));
        assert!(grade(&q, "b2").note.is_none() && !grade(&q, "b2").correct, "not one of the tiles");
        let mut quiz = vec![q];
        answer_missing(&mut quiz);
        assert_eq!(quiz[0].correct_answer, "B2", "the fewest tiles, first in order");
    }

    #[test]
    fn an_exact_question_wants_its_answer() {
        let q = item(ABC, &["B2", "BB", "B/B/B"], serde_json::json!({ "exact": true, "correct_answer": "B/B/B" }));
        check(&q).unwrap();
        assert!(grade(&q, "B/B/B").correct);
        let v = grade(&q, "B2");
        assert!(!v.correct && v.note.unwrap() == crate::t("quiz.check.fill_bar_exact"));
    }

    #[test]
    fn the_validator_names_what_is_wrong() {
        let err = |abc: &str, tiles: &[&str], extra: serde_json::Value| check(&item(abc, tiles, extra)).unwrap_err();
        assert!(err(ABC, &["B2"], serde_json::json!({})).contains("at least 2 tiles"));
        assert!(err(ABC, &["B2", "B H"], serde_json::json!({})).contains("without spaces"));
        assert!(err(ABC, &["B2", "Bq"], serde_json::json!({})).contains("is not a note"));
        assert!(err(ABC, &["B3", "B4"], serde_json::json!({})).contains("no tiles add up to the gap of 2 beats"));
        assert!(err(ABC, &["B2", "B"], serde_json::json!({ "exact": true })).contains("needs correct_answer"));
        assert!(err(ABC, &["B2", "B"], serde_json::json!({ "correct_answer": "B" })).contains("does not fill the gap: 1 of 2 beats"));
        assert!(err("X:1\nK:C\nB B|", &["B2", "B"], serde_json::json!({})).contains("invisible rest"));
    }
}
