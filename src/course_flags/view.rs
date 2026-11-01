//! How a page shows what a check matched: never the word itself. A page
//! draws each match with `templates/flag_matches.html`, which masks the word
//! ("w******") with `templates/masked_word.html`; holding it down shows it
//! (`static/modal.js`). More than [`LIST_AFTER`] matches show as a count that
//! opens the whole list, ten to a page.

use super::{Flag, Match, Source};

/// Up to this many matches are listed in place; more show as a count.
pub const LIST_AFTER: usize = 5;

/// One match as a page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchView {
    /// What matched, in clear: `masked_word.html` masks it.
    pub text: String,
    /// The i18n key of the hover hint ("Click to reveal word").
    pub hint: &'static str,
    /// The lesson's title.
    pub lesson: String,
    /// Where in the lesson: the JSON field (and, for the superuser, the file).
    pub place: String,
    /// The text around it (superuser only), masked as a whole.
    pub context: Option<String>,
    /// For a quote the AI check found (#1037): the rule it breaks, in
    /// plain text; empty for a listed word.
    pub why: String,
}

impl MatchView {
    /// A match as the course's owner sees it: the word (or the AI's quote,
    /// #1037, with the category it breaks, #1046), the lesson and the field.
    pub fn owner(flag: &Flag, m: &Match) -> MatchView {
        let base = MatchView { text: m.term.clone(), hint: "courses.masked.reveal_word", lesson: m.lesson.clone(), place: m.field.clone(), context: None, why: String::new() };
        match ai_category(flag, m) {
            Some(c) => MatchView { text: m.text.clone(), hint: "courses.masked.reveal_text", why: crate::t(&format!("courses.picker.flagged_why_{}", c.name)), ..base },
            None => base,
        }
    }

    /// A match as the superuser sees it (#1034): also the file, and the
    /// text around the word. An AI quote (#1037) is its own text, under the
    /// name of the category it breaks (#1046).
    pub fn superuser(flag: &Flag, m: &Match) -> MatchView {
        let place = if m.field.is_empty() { m.path.clone() } else { format!("{} {}", m.path, m.field) };
        match ai_category(flag, m) {
            Some(c) => MatchView { place, why: crate::t(&format!("superuser.courses.category.{}", c.name)), ..MatchView::owner(flag, m) },
            None => MatchView { place, context: Some(m.text.clone()), ..MatchView::owner(flag, m) },
        }
    }
}

/// The category an AI finding (#1037, #1046) breaks, or `None` for a
/// listed word.
fn ai_category(flag: &Flag, m: &Match) -> Option<&'static crate::content_check::Category> {
    if flag.source == Source::Ai {
        crate::content_check::Category::named(&m.term)
    } else {
        None
    }
}

/// The matches of a flag as its owner sees them, once each.
pub fn owner_views(flag: &Flag) -> Vec<MatchView> {
    let mut out: Vec<MatchView> = Vec::new();
    for v in flag.matches.iter().map(|m| MatchView::owner(flag, m)) {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// The first letter and an asterisk for each letter after it ("w******").
/// Spaces stay, so a masked quote still wraps.
pub fn mask(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.trim().chars().enumerate() {
        out.push(if i == 0 || c.is_whitespace() { c } else { '*' });
    }
    out
}
