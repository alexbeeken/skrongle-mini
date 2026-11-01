//! JSON-driven content: a *course* is a manifest (`course.json`) plus one JSON
//! file per lesson. Everything the app shows is built from these files.
//!
//! ```text
//! <course>/
//!   course.json            manifest: title, units, groups, paradigm, labels…
//!   lessons/**/*.json      one lesson per file (any folder structure)
//!   pictures/**/*.svg      SVG pictures lesson text can show
//! ```
//!
//! Built-in courses live under `data/courses/<id>/` and are loaded at boot;
//! courses people build in the app are stored in SQLite as the same set of
//! files and loaded through the same code (`ContentStore::from_files`).
//!
//! Themes: a *theme* is an app-level skin (`data/themes/<id>/theme.json`,
//! see `themes.rs`) a learner wears whatever course is open. A lesson may
//! carry themed material (`themes.<id>`) that is merged in front of its own
//! when that theme is on; a lesson without it is simply unchanged. Both live
//! in JSON files; the app only renders what the files declare.
//!
//! Field names: the canonical names are the ones documented in
//! `docs/course-format.md`. Older Spanish-specific spellings (`level`,
//! `category`, `es`/`en`, `target_grammar_point`, `journey`, `verb`/`tense`)
//! are accepted as aliases so existing files keep loading.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

// ====================================================================
// Manifest (course.json)
// ====================================================================

/// One side of a term/meaning pair: how it is labelled and which language it
/// is in (for `lang` attributes and spell-checking). For a language course the
/// terms are the target language and the meanings the learner's language; for
/// anything else the terms are the terminology and the meanings their
/// definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Side {
    pub label: String,
    #[serde(default)]
    pub lang: String,
}

fn default_terms() -> Side {
    Side { label: "Term".into(), lang: String::new() }
}

fn default_meanings() -> Side {
    Side { label: "Meaning".into(), lang: String::new() }
}

/// A unit (CEFR level, module, chapter…): the top of the hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unit {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
}

/// A lesson group inside a unit ("Grammar", "Vocabulary", "Basics"…).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub title: String,
}

mod paradigm;
pub use paradigm::Paradigm;

/// Small UI strings a course may customise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strings {
    /// Shown on a correct answer; several are picked from at random.
    #[serde(default = "default_praise")]
    pub praise: Vec<String>,
    /// Shown on a wrong answer.
    #[serde(default = "default_almost")]
    pub almost: Vec<String>,
}

fn default_praise() -> Vec<String> {
    vec!["Correct!".into(), "Nice!".into(), "Exactly!".into(), "Spot on!".into()]
}

fn default_almost() -> Vec<String> {
    vec!["Not quite".into(), "Hmm, no".into(), "Close…".into()]
}

impl Default for Strings {
    fn default() -> Self {
        Strings { praise: default_praise(), almost: default_almost() }
    }
}

/// Optional: how the course's language names the days of the week and the
/// months, for the date game ("Date Dash"). Without it the game is hidden.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calendar {
    /// The seven weekday names, Monday first.
    pub days: Vec<String>,
    /// The twelve month names, January first.
    pub months: Vec<String>,
    /// How a full date is written, with `{day}` `{d}` `{month}` `{year}` placeholders.
    #[serde(default = "default_date_format")]
    pub format: String,
}

fn default_date_format() -> String {
    "{day}, {d} {month} {year}".into()
}

impl Calendar {
    /// Writes a date with the course's format, e.g. "jueves, 9 de octubre de 2026".
    pub fn write(&self, weekday: usize, d: u32, month: usize, year: i32) -> String {
        self.format
            .replace("{day}", self.days.get(weekday).map(String::as_str).unwrap_or(""))
            .replace("{month}", self.months.get(month).map(String::as_str).unwrap_or(""))
            .replace("{d}", &d.to_string())
            .replace("{year}", &year.to_string())
    }
}

/// A visual skin: `data/themes/<id>/theme.json`. Colours, a font, art,
/// praise strings and profile pictures; lessons may carry material for it
/// under `themes.<id>`, and the manifest praise lines. A theme is flair worn
/// by the learner across every course; nothing in it is needed for a course
/// to work.
///
/// Themes, courses and profiles stay apart (docs/themes.md, "Themes,
/// courses and profiles"): a theme belongs to the app, not to a course, so
/// everything it says (titles, praise) is in the app's language (English in
/// theme.json, others in i18n, themes.rs). A course's words come only from it:
/// its lessons' `themes.<id>` blocks and its manifest's `themes.<id>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Theme {
    /// Lowercase a-z, 0-9 and '-'; the folder name. Lessons refer to it in `themes`.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Emoji for the picker.
    #[serde(default)]
    pub icon: String,
    /// Colour tokens (`accent`, `bg`, `surface`, …; see `THEME_COLOR_KEYS`),
    /// hex colours only.
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    /// The same tokens again, used when the browser prefers a dark scheme.
    #[serde(default)]
    pub dark: BTreeMap<String, String>,
    /// A CSS `font-family` stack for the whole site.
    #[serde(default)]
    pub font: String,
    #[serde(default)]
    pub art: ThemeArt,
    /// Praise / "almost" strings that replace the manifest's while the theme is on.
    #[serde(default)]
    pub strings: ThemeStrings,
    /// Themed versions of the built-in profile pictures, by the picture's id
    /// (`owl`, `fox`, …): while the theme is on, a learner who picked that
    /// picture is shown this one instead.
    #[serde(default)]
    pub avatars: Vec<ThemeAvatar>,
    /// The theme's colours for the eight slots of the profile picture's
    /// palette (`profile::AVATAR_COLORS`), by slot 1-8; a slot not given
    /// keeps the default colour.
    #[serde(default)]
    pub avatar_colors: Vec<ThemeAvatarColor>,
    /// Backgrounds the theme adds behind the profile picture, in slots 9-16
    /// (`profile::THEME_SLOTS`): pictures of places in the theme's world,
    /// offered next to the default eight only while the theme is worn.
    #[serde(default)]
    pub avatar_backgrounds: Vec<ThemeAvatarBackground>,
    /// The app's own little icons, by slot (`streak`, `xp`, `mode_sprint`, …; see
    /// `ICON_KEYS`): an emoji or short glyph for each one the theme changes.
    /// The old slot names in `ICON_ALIASES` are read as the new ones.
    #[serde(default, deserialize_with = "icons_with_aliases")]
    pub icons: BTreeMap<String, String>,
    /// Icons for lessons that set none of their own on the Sprint mode path,
    /// used in turn; replaces the app's default list when given.
    #[serde(default)]
    pub lesson_icons: Vec<String>,
    /// Shop badges (#569) this theme draws its own way, by badge id: the
    /// files `badges/<id>.svg` of a built-in theme, found when it loads (not
    /// a field of theme.json). A badge it lacks draws plain.
    #[serde(skip)]
    pub badges: std::collections::BTreeSet<String>,
}

/// The slots of `Theme::icons`, with what the app shows by default.
pub const ICON_KEYS: &[(&str, &str)] = &[
    ("streak", "🔥"),   // the day streak, and a hot run of right answers
    ("xp", "⚡"),       // the lightning round (the slot kept its name from when it was XP)
    ("words", "📚"),    // terms collected
    ("goal", "🎉"),     // daily goal done, session complete
    ("flawless", "🌟"), // a perfect session
    ("effort", "💪"),   // a rough session
    ("mastery", "🌶️"),  // one unit of mastery on the path
    ("locked", "🔒"),   // a lesson not yet reachable
    ("trophy", "🏆"),   // every lesson done
    ("review", "🔁"),   // the review mix
    ("path", "🗺️"),     // the Sprint mode path
    ("site", "🖥️"),     // back to the full site
    ("start", "🏁"),    // the first lesson, when it has no icon
    ("bite", "💡"),     // a bite without an emoji of its own
    ("hint", "💡"),     // the hint toggle
    ("mode_sprint", "📱"),   // the modes on the home page and in the mode bars
    ("mode_study", "📖"),
    ("mode_play", "🎲"),
    ("mode_guide", "🧭"),
    ("mode_ai", "🤖"),
];

/// Icon slots that were renamed: a theme that sets the old one sets the new
/// one (the new one wins if a theme somehow has both).
pub const ICON_ALIASES: &[(&str, &str)] = &[("mode_d", "mode_sprint"), ("mode_k", "mode_study"), ("mode_g", "mode_play")];

fn icons_with_aliases<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, String>, D::Error> {
    let mut icons = BTreeMap::<String, String>::deserialize(d)?;
    for (old, new) in ICON_ALIASES {
        if let Some(v) = icons.remove(*old) {
            icons.entry((*new).to_string()).or_insert(v);
        }
    }
    Ok(icons)
}

/// Icons for lessons that set none, used in turn along the path.
pub const DEFAULT_LESSON_ICONS: &[&str] = &["📘", "📗", "📙", "📕", "📒", "📓", "📔", "🧭", "🔑", "💡", "🧩", "🎯"];

/// The app's icons as templates and the Sprint mode player see them: the
/// defaults, with the worn theme's replacements swapped in.
#[derive(Debug, Clone, Serialize)]
pub struct Icons {
    pub streak: String,
    pub xp: String,
    pub words: String,
    pub goal: String,
    pub flawless: String,
    pub effort: String,
    pub mastery: String,
    pub locked: String,
    pub trophy: String,
    pub review: String,
    pub path: String,
    pub site: String,
    pub start: String,
    pub bite: String,
    pub hint: String,
    pub mode_sprint: String,
    pub mode_study: String,
    pub mode_play: String,
    pub mode_guide: String,
    pub mode_ai: String,
}

impl Icons {
    pub fn of(theme: Option<&Theme>) -> Icons {
        let get = |key: &str| -> String {
            let fallback = ICON_KEYS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).unwrap_or("");
            theme.and_then(|t| t.icons.get(key)).cloned().unwrap_or_else(|| fallback.to_string())
        };
        Icons {
            streak: get("streak"),
            xp: get("xp"),
            words: get("words"),
            goal: get("goal"),
            flawless: get("flawless"),
            effort: get("effort"),
            mastery: get("mastery"),
            locked: get("locked"),
            trophy: get("trophy"),
            review: get("review"),
            path: get("path"),
            site: get("site"),
            start: get("start"),
            bite: get("bite"),
            hint: get("hint"),
            mode_sprint: get("mode_sprint"),
            mode_study: get("mode_study"),
            mode_play: get("mode_play"),
            mode_guide: get("mode_guide"),
            mode_ai: get("mode_ai"),
        }
    }

    /// The icon of the `position`-th lesson without one of its own.
    pub fn lesson(theme: Option<&Theme>, position: usize) -> String {
        match theme.filter(|t| !t.lesson_icons.is_empty()) {
            Some(t) => t.lesson_icons[position % t.lesson_icons.len()].clone(),
            None => DEFAULT_LESSON_ICONS[position % DEFAULT_LESSON_ICONS.len()].to_string(),
        }
    }
}

impl Default for Icons {
    fn default() -> Self {
        Icons::of(None)
    }
}

/// A short glyph for an icon slot: one to four characters, no markup.
pub fn is_icon(s: &str) -> bool {
    let n = s.chars().count();
    (1..=4).contains(&n) && !s.chars().any(|c| c.is_whitespace() || matches!(c, '<' | '>' | '&' | '"'))
}

/// A themed version of one built-in profile picture (`profile::AVATARS`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeAvatar {
    /// The built-in picture this one stands in for (`noor`, `kai`, …; an
    /// id from before #180 such as `owl` still names its slot).
    pub id: String,
    /// The name shown under the picture while the theme is on; defaults to
    /// the built-in picture's name.
    #[serde(default)]
    pub label: String,
    /// A file of the theme (`pictures/<name>.svg`) or an `https://` URL.
    pub image: String,
    /// Hex colour of the picture's tile in the picture dialog
    /// (`profile::own_color`); the badge behind the learner's picture is the
    /// background they chose. Draw the picture on a transparent background
    /// so that shows; empty puts the page's surface colour behind the tile.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
}

/// A theme's colour for one slot of the profile picture's palette.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeAvatarColor {
    /// 1-8: which of the default colours (`profile::AVATAR_COLORS`) it replaces.
    pub slot: u8,
    /// The colour's name, in English: the swatch's accessible name.
    pub label: String,
    /// A hex colour.
    pub color: String,
}

/// A background a theme adds behind the profile picture: a little scene of
/// its world, in a slot of its own after the default eight.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeAvatarBackground {
    /// 9-16 (`profile::THEME_SLOTS`): what `users.avatar_color` stores. Another
    /// theme's background in the same slot stands in for it while that theme
    /// is worn; with none, slot 1 shows (`profile::shown_slot`).
    pub slot: u8,
    /// The background's name, in English: the swatch's accessible name.
    pub label: String,
    /// A file of the theme (`pictures/<name>.svg`) or an `https://` URL, drawn
    /// to fill a square that is cut to a circle.
    pub image: String,
    /// A hex colour under the picture, shown before it loads.
    pub color: String,
}

/// Pictures a theme brings: files under the theme's `pictures/` or `https://` URLs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeArt {
    /// A wide picture shown on the home page and the Sprint mode home.
    #[serde(default)]
    pub banner: String,
    /// A small emblem shown in the theme picker and next to the title.
    #[serde(default)]
    pub mark: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeStrings {
    #[serde(default)]
    pub praise: Vec<String>,
    #[serde(default)]
    pub almost: Vec<String>,
}

/// The colour tokens a theme may set, as written in the JSON (`accent_dark`);
/// each becomes the CSS custom property of the same name (`--accent-dark`)
/// that `style.css` and `sprint.css` are built on.
pub const THEME_COLOR_KEYS: &[&str] = &[
    "bg", "surface", "text", "muted", "line", "accent", "accent_dark", "gold", "gold_dark", "ok", "ok_dark", "ok_bg", "bad",
    "bad_bg", "warn_bg", "sky", "violet",
];

impl Theme {
    pub fn icon(&self) -> &str {
        if self.icon.is_empty() { "🎨" } else { &self.icon }
    }

    /// The theme's version of a built-in picture, by either's id, old or new.
    pub fn avatar(&self, id: &str) -> Option<&ThemeAvatar> {
        let want = crate::profile::canonical(id)?;
        self.avatars.iter().find(|a| crate::profile::canonical(&a.id) == Some(want))
    }

    pub fn icons(&self) -> Icons {
        Icons::of(Some(self))
    }

    /// The stylesheet that applies the theme: custom properties on `:root`,
    /// served after the app's own stylesheets so they win over the defaults
    /// (both the light ones and the dark-scheme block).
    pub fn css(&self) -> String {
        self.css_for(":root")
    }

    /// The same custom properties set on `selector` instead of `:root` (a
    /// course card wearing its course's theme on the course list, #565).
    pub fn css_for(&self, selector: &str) -> String {
        fn block(out: &mut String, selector: &str, colors: &BTreeMap<String, String>, font: &str) {
            out.push_str(selector);
            out.push('{');
            for (k, v) in colors {
                out.push_str(&format!("--{}:{v};", k.replace('_', "-")));
            }
            if !font.is_empty() {
                out.push_str(&format!("--font:{font};"));
            }
            out.push_str("}\n");
        }
        let mut out = format!("/* theme: {} */\n", self.id);
        if !self.colors.is_empty() || !self.font.is_empty() {
            block(&mut out, selector, &self.colors, &self.font);
        }
        if !self.dark.is_empty() {
            out.push_str("@media (prefers-color-scheme: dark){");
            block(&mut out, selector, &self.dark, "");
            out.push_str("}\n");
        }
        out
    }
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`: the only colour syntax a theme
/// may use, so a course file can never smuggle CSS into the page.
pub fn is_hex_color(s: &str) -> bool {
    let Some(h) = s.strip_prefix('#') else { return false };
    matches!(h.len(), 3 | 4 | 6 | 8) && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A `font-family` stack: names, quotes, commas and hyphens only.
pub fn is_font_stack(s: &str) -> bool {
    !s.trim().is_empty()
        && s.chars().count() <= 160
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | ',' | '\'' | '"' | '-'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Course id: lowercase a-z, 0-9 and '-'. Used in URLs and the database.
    pub id: String,
    pub title: String,
    /// What is being learnt ("Spanish", "Music theory"). Defaults to the title.
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub description: String,
    /// Emoji for the course card.
    #[serde(default)]
    pub icon: String,
    #[serde(default = "default_terms")]
    pub terms: Side,
    #[serde(default = "default_meanings")]
    pub meanings: Side,
    pub units: Vec<Unit>,
    /// The units are levels (A1, A2…, Beginner, Intermediate…): a learner
    /// opening the course may start at a later one (`ContentStore::start_units`).
    /// Off for units that build on each other, like chapters.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub levels: bool,
    /// The language features (Listening, sentence builds, translation
    /// fields, read-aloud, Numbers by ear): on, off, or unset, which means
    /// on when the terms have a language of their own (`is_language`).
    /// The lesson editor's gear sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_features: Option<bool>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub paradigm: Option<Paradigm>,
    /// Characters offered as tap-to-insert buttons next to text inputs (accents, symbols).
    #[serde(default)]
    pub chars: Vec<String>,
    #[serde(default)]
    pub strings: Strings,
    /// Weekday and month names for the date game; optional.
    #[serde(default)]
    pub calendar: Option<Calendar>,
    /// Course material for a theme, by theme id: praise lines in the
    /// course's language that fit the theme's world. A theme file is
    /// app-level and in English; anything in a course's language belongs
    /// here or in a lesson's `themes` block. Ids of themes that are not
    /// installed are kept and never used.
    #[serde(default)]
    pub themes: BTreeMap<String, CourseTheme>,
    /// A music course: how notes are named on screen (`notation::Music`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music: Option<crate::notation::Music>,
}

/// `course.json` `themes.<id>`: what the course says while that theme is worn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CourseTheme {
    /// Replace the theme's strings (and the course's own) while it is worn.
    #[serde(default)]
    pub strings: ThemeStrings,
}

impl Manifest {
    pub fn unit_index(&self, id: &str) -> Option<usize> {
        self.units.iter().position(|u| u.id == id)
    }

    pub fn group_title(&self, id: &str) -> String {
        self.groups.iter().find(|g| g.id == id).map(|g| g.title.clone()).unwrap_or_else(|| id.to_string())
    }

    pub fn subject(&self) -> &str {
        if self.subject.is_empty() { &self.title } else { &self.subject }
    }

    pub fn icon(&self) -> &str {
        if self.icon.is_empty() { "📚" } else { &self.icon }
    }

    /// Is this a language course? `language_features` when set; else true
    /// when the terms are written in a language of their own (a `lang` tag
    /// that differs from the meanings'). Listening, the Sprint listening
    /// exercises and sentence builds, read-aloud and Numbers by ear need it.
    pub fn is_language(&self) -> bool {
        if let Some(on) = self.language_features {
            return on;
        }
        let t = self.terms.lang.trim().to_ascii_lowercase();
        let m = self.meanings.lang.trim().to_ascii_lowercase();
        !t.is_empty() && t != m
    }
}

// ====================================================================
// Lesson
// ====================================================================

mod quiz_type;
pub use quiz_type::QuizType;

/// A single quiz question belonging to a lesson.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizItem {
    pub question_id: String,
    pub prompt: String,
    #[serde(rename = "type")]
    pub kind: QuizType,
    /// Only used for `multiple-choice`.
    #[serde(default)] pub options: Vec<String>,
    #[serde(default)] pub correct_answer: String,
    /// Extra answers accepted as correct (fill-in-blank, numeric, expression).
    #[serde(default)] pub accepted_answers: Vec<String>,
    /// `numeric` only: how far off an answer may be and still count (absolute).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub tolerance: Option<f64>,
    /// `numeric` only: the unit shown after the answer box; typing it is optional.
    #[serde(default, skip_serializing_if = "String::is_empty")] pub unit: String,
    /// `algebra-tiles` only: the answer's form (`product`); its pile (`x^2 + 5x + 6`) is in `tiles`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub form: String,
    #[serde(flatten)]
    pub measure: crate::measure::Measure,
    /// `clock` only: how far one step of the minute hand goes (default 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_minutes: Option<u32>,
    #[serde(default)] pub explanation: String,
    #[serde(default)] pub hints: Vec<String>,
    /// Music drawn as a staff above the prompt, in ABC notation (`notation.rs`).
    #[serde(default, skip_serializing_if = "String::is_empty")] pub abc: String,
    /// Music a speaker button plays, in ABC notation; `option_kind: "abc"` draws the options as staves (`notation.rs`).
    #[serde(default, skip_serializing_if = "String::is_empty")] pub play: String,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub option_kind: String,
    /// `fill-bar` and `chord-spell`: the note values or notes to tap (a string of notes separated by spaces, or a list).
    /// `algebra-tiles`: the pile, read back joined by spaces (`algebra_tiles::read`).
    #[serde(default, deserialize_with = "crate::notation::chord_spell::tiles_de", skip_serializing_if = "Vec::is_empty")] pub tiles: Vec<String>,
    /// `fill-bar` only: whether only `correct_answer` counts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub exact: bool,
    /// `staff-note` only: `treble` or `bass`, and whether the note counts in any octave (`notation::staff_note`).
    #[serde(default, skip_serializing_if = "String::is_empty")] pub clef: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub any_octave: bool,
    /// `keyboard` only (`notation::keyboard`): grade the exact key, not its pitch class; 1 or 2 octaves of keys from middle C.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub octave: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub octaves: Option<u8>,
    /// `keyboard` only: the answers as ABC notes, kept when the loader names `correct_answer` (`notation::name_notes`).
    #[serde(skip)] pub keys: Vec<String>,
    /// Vocabulary entries (by their `term`) a learner "collects" by answering
    /// this question correctly. Entries whose term equals an accepted answer
    /// are collected automatically.
    #[serde(default)]
    pub teaches: Vec<String>,
    /// Came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

/// A paradigm table for one item (a verb in one tense, a noun in one case…).
/// `forms` maps paradigm slot keys to the form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conjugation {
    #[serde(alias = "verb")]
    pub item: String,
    pub meaning: String,
    #[serde(alias = "tense")]
    pub set: String,
    /// ASCII picture for the item (one per item across the course; a lesson
    /// may omit it when another lesson already draws it).
    #[serde(default)]
    pub art: Vec<String>,
    pub forms: BTreeMap<String, String>,
}

impl Conjugation {
    pub fn form(&self, slot: &str) -> &str { self.forms.get(slot).map(String::as_str).unwrap_or("") }
}

/// A term with its meaning, optionally with an example.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabEntry {
    #[serde(alias = "es")]
    pub term: String,
    #[serde(alias = "en")]
    pub meaning: String,
    #[serde(default)]
    pub example: Option<String>,
    /// The example in the meaning language; shown as a hint on the example.
    #[serde(default, alias = "example_en", skip_serializing_if = "Option::is_none")]
    pub example_meaning: Option<String>,
    /// The entry, or its example, came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

/// Which of the two speakers says a dialogue line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    A,
    B,
}

impl Speaker {
    pub fn as_str(self) -> &'static str {
        match self {
            Speaker::A => "a",
            Speaker::B => "b",
        }
    }

    pub fn other(self) -> Speaker {
        match self {
            Speaker::A => Speaker::B,
            Speaker::B => Speaker::A,
        }
    }

    pub fn parse(s: &str) -> Option<Speaker> {
        match s {
            "a" => Some(Speaker::A),
            "b" => Some(Speaker::B),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueLine {
    pub speaker: Speaker,
    #[serde(alias = "es")]
    pub text: String,
    /// The line in the learner's language (optional for non-language courses).
    #[serde(alias = "en", default)]
    pub meaning: String,
    /// Optional pointer back to the point being practised.
    #[serde(default)]
    pub note: Option<String>,
}

/// A scripted two-person conversation (or interview, role play, Q&A).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    /// One or two sentences setting the scene.
    #[serde(default)]
    pub scenario: String,
    /// Names for the two roles.
    pub role_a: String,
    pub role_b: String,
    pub lines: Vec<DialogueLine>,
    /// Came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

/// One bite-sized card of a lesson's Markdown (split on `## ` headings).
#[derive(Debug, Clone)]
pub struct Section {
    pub heading: String,
    pub html: String,
}

/// Optional hand-ordered Guide step, as written in the lesson JSON. Without a
/// `guide` array the steps are derived automatically (see `build_guide`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GuideSpec {
    /// A Markdown section of this lesson, matched by heading prefix (case-insensitive).
    Section { section: String },
    /// A quiz question of this lesson, by id.
    Question { question: String },
    /// The next `vocab` vocabulary entries as tap-to-reveal cards.
    Vocab { vocab: usize },
    /// A short inline note.
    Tip { tip: String },
    /// Extra inline content that only exists in the guide.
    Concept { concept: ConceptSpec },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptSpec {
    pub title: String,
    pub markdown: String,
}

/// A step of the Guide, built once at load.
#[derive(Debug, Clone)]
pub enum GuideStep {
    Concept { title: String, html: String },
    /// Index into `Lesson::quiz`.
    Question { index: usize },
    Vocab { entries: Vec<VocabEntry> },
    Tip { text: String },
}

impl GuideStep {
    pub fn is_question(&self) -> bool {
        matches!(self, GuideStep::Question { .. })
    }

}

/// Sprint mode: extra, mobile-first material for a lesson. Everything is optional;
/// a lesson without a `sprint` block still plays (bites are derived from the
/// Markdown sections and exercises from the quiz, vocabulary, conjugations
/// and conversations).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sprint {
    /// Emoji for the lesson's node on the Sprint mode path.
    #[serde(default)]
    pub icon: String,
    /// The idea in one breath: a title, a punchy rule and one example.
    #[serde(default)]
    pub bites: Vec<Bite>,
    /// "Which bucket?" sorting exercises.
    #[serde(default)]
    pub sorts: Vec<SortSpec>,
    /// "Tap the mistake" sentences.
    #[serde(default)]
    pub spots: Vec<SpotSpec>,
    /// Sentences to rebuild from shuffled tiles.
    #[serde(default)]
    pub builds: Vec<BuildSpec>,
    /// "Build the scale" ladders in a music course (#1229, `sprint::scale`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scales: Vec<crate::sprint::scale::ScaleSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bite {
    pub title: String,
    pub rule: String,
    #[serde(alias = "example_es", default)]
    pub example: String,
    #[serde(alias = "example_en", default)]
    pub example_meaning: String,
    #[serde(default)]
    pub emoji: String,
    /// Came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortSpec {
    pub title: String,
    pub buckets: Vec<String>,
    pub items: Vec<SortItem>,
    #[serde(default)]
    pub why: String,
    /// Came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortItem {
    #[serde(default)]
    pub text: String,
    /// A speaker tile: music in ABC notation, played on a tap, in place of `text`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub play: String,
    /// Index into `SortSpec::buckets`.
    pub bucket: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotSpec {
    /// A sentence (or line of code, formula…) with exactly one wrong word.
    #[serde(default)]
    pub sentence: String,
    /// Or a tune in ABC with one wrong note, drawn as a staff (#1227).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub abc: String,
    /// The wrong word, exactly as it appears in `sentence` (punctuation aside); with `abc`, the wrong note's number from 1.
    #[serde(deserialize_with = "crate::sprint::spot::text_or_number")]
    pub wrong: String,
    /// What it should have been.
    pub fix: String,
    #[serde(default)]
    pub why: String,
    /// Came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSpec {
    /// The sentence to rebuild from its words.
    #[serde(alias = "es")]
    pub text: String,
    /// What it means / the prompt shown to the learner.
    #[serde(alias = "en")]
    pub meaning: String,
    /// Extra wrong tiles mixed into the word bank.
    #[serde(default)]
    pub distractors: Vec<String>,
    /// Came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

/// Where picture files live inside a course, and what they are.
pub const PICTURES_DIR: &str = "pictures/";
pub const PICTURE_EXT: &str = ".svg";

pub fn is_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

/// A sentence for listening practice: the app reads it aloud with the
/// browser's speech synthesis and the learner types what they heard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListeningItem {
    /// The sentence in the course's language, exactly as it should be typed.
    pub text: String,
    /// What it means, in the learner's language (shown after answering).
    #[serde(default)]
    pub meaning: String,
    /// Optional pointer shown with the answer: the grammar at work, or the
    /// new word the sentence slips in ("New word: la oficina = the office.").
    #[serde(default)]
    pub note: Option<String>,
    /// Came from a theme block (not part of the JSON).
    #[serde(skip)]
    pub themed: bool,
}

/// Themed material a lesson carries for one theme: the same shapes as the
/// lesson's own fields. When the theme is on it is merged *in front of* the
/// lesson's material (see `apply_theme`); the lesson's own content is never
/// removed, so a theme can only add flair, not take teaching away.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LessonTheme {
    /// The lesson's icon on the Sprint mode path while the theme is on.
    #[serde(default)]
    pub icon: String,
    /// Markdown appended to the lesson text (extra `## ` sections).
    #[serde(default)]
    pub content: String,
    /// Re-examples existing terms (by `term`) or adds flair terms (with a `meaning`).
    #[serde(default)]
    pub vocabulary: Vec<ThemeVocab>,
    #[serde(default)]
    pub quiz: Vec<QuizItem>,
    #[serde(default)]
    pub listening: Vec<ListeningItem>,
    #[serde(default)]
    pub conversations: Vec<Conversation>,
    #[serde(default)]
    pub bites: Vec<Bite>,
    #[serde(default)]
    pub sorts: Vec<SortSpec>,
    #[serde(default)]
    pub spots: Vec<SpotSpec>,
    #[serde(default)]
    pub builds: Vec<BuildSpec>,
}

impl LessonTheme {
    pub fn is_empty(&self) -> bool {
        self.icon.trim().is_empty()
            && self.content.trim().is_empty()
            && self.vocabulary.is_empty()
            && self.quiz.is_empty()
            && self.listening.is_empty()
            && self.conversations.is_empty()
            && self.bites.is_empty()
            && self.sorts.is_empty()
            && self.spots.is_empty()
            && self.builds.is_empty()
    }
}

/// A themed vocabulary entry: for a term the lesson already teaches only
/// the `example` is taken; a new term needs a `meaning` and is added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeVocab {
    pub term: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub example: Option<String>,
    #[serde(default, alias = "example_en")]
    pub example_meaning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub slug: String,
    pub title: String,
    /// Unit id from the manifest.
    #[serde(alias = "level")]
    pub unit: String,
    /// Group id (free-form; titles come from the manifest when listed there).
    #[serde(alias = "category", default = "default_group")]
    pub group: String,
    /// Short one-line summary shown in lists.
    #[serde(default)]
    pub summary: String,
    /// Ordering inside a unit (lower first). Defaults to 100.
    #[serde(default = "default_order")]
    pub order: u32,
    /// The lesson text, in Markdown. `## ` headings split it into cards.
    #[serde(alias = "markdown_content")]
    pub content: String,
    /// Lesson ids that should ideally be studied first.
    #[serde(default)]
    pub prerequisites: Vec<String>,
    /// The one thing this lesson teaches, in a phrase.
    #[serde(alias = "target_grammar_point", default)]
    pub focus: String,
    #[serde(default)]
    pub vocabulary: Vec<VocabEntry>,
    #[serde(default)]
    pub quiz: Vec<QuizItem>,
    /// Paradigm tables this lesson teaches (needs `paradigm` in the manifest).
    #[serde(default)]
    pub conjugations: Vec<Conjugation>,
    /// Scripted conversations building on this lesson (Sprint mode builds sentences from their lines).
    #[serde(default)]
    pub conversations: Vec<Conversation>,
    /// Optional hand-ordered steps for the Guide; derived automatically when absent.
    #[serde(alias = "journey", default)]
    pub guide: Vec<GuideSpec>,
    /// Optional Sprint mode extras.
    #[serde(alias = "dmode", default)]
    pub sprint: Sprint,
    /// Sentences for listening practice (hear it, type it); needs a language course.
    #[serde(default)]
    pub listening: Vec<ListeningItem>,
    /// Themed material by theme id (each declared in the manifest); optional flair.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub themes: BTreeMap<String, LessonTheme>,

    /// Position of `unit` in the manifest (not part of the JSON).
    #[serde(skip)]
    pub unit_index: usize,
    /// Markdown rendered to HTML once at load (not part of the JSON).
    #[serde(skip)]
    pub html_content: String,
    /// Markdown split into `## ` sections, rendered (not part of the JSON).
    #[serde(skip)]
    pub sections: Vec<Section>,
    /// Guide steps built from `guide` or derived (not part of the JSON).
    #[serde(skip)]
    pub guide_steps: Vec<GuideStep>,
}

impl Lesson {
    pub fn guide_question_count(&self) -> usize {
        self.guide_steps.iter().filter(|s| s.is_question()).count()
    }

    /// Does the lesson carry material for this theme?
    pub fn has_theme(&self, id: &str) -> bool {
        self.themes.get(id).is_some_and(|t| !t.is_empty())
    }
}

/// Theme ids that were renamed: a lesson block under the old id is read as the
/// new one (the new id wins if a lesson somehow has both).
pub const THEME_ALIASES: &[(&str, &str)] = &[("wow", "realm"), ("stardew", "meadowbrook"), ("fallout", "rustwake")];

/// Move `themes.<old id>` blocks to the theme's current id.
fn rename_theme_aliases(lesson: &mut Lesson) {
    for (old, new) in THEME_ALIASES {
        if let Some(block) = lesson.themes.remove(*old) {
            lesson.themes.entry((*new).to_string()).or_insert(block);
        }
    }
}

/// The lesson with the material of theme `id` merged in: themed bites,
/// sentences, exercises, questions and conversations go in front of the
/// lesson's own (so the features that take "the first few" show the theme),
/// themed vocabulary re-examples existing terms or adds new ones at the end,
/// themed Markdown is appended as extra sections. `None` when the lesson has
/// nothing for that theme. The result is validated like any lesson.
pub fn apply_theme(lesson: &Lesson, id: &str) -> Result<Option<Lesson>, String> {
    let Some(t) = lesson.themes.get(id) else { return Ok(None) };
    if t.is_empty() {
        return Ok(None);
    }
    let mut out = lesson.clone();
    if !t.icon.trim().is_empty() {
        if !is_icon(t.icon.trim()) {
            return Err(format!("icon '{}' must be a short glyph such as an emoji", t.icon));
        }
        out.sprint.icon = t.icon.trim().to_string();
    }
    if !t.content.trim().is_empty() {
        out.content = format!("{}\n\n{}\n", lesson.content.trim_end(), t.content.trim());
    }
    for tv in &t.vocabulary {
        if tv.term.trim().is_empty() {
            return Err("vocabulary entries need a term".into());
        }
        match out.vocabulary.iter_mut().find(|v| v.term == tv.term) {
            Some(v) => {
                if !tv.meaning.trim().is_empty() && tv.meaning != v.meaning {
                    return Err(format!("vocabulary '{}' is already taught by the lesson (meaning '{}'); a theme may only give it another example", tv.term, v.meaning));
                }
                match &tv.example {
                    Some(e) if !e.trim().is_empty() => {
                        v.example = Some(e.clone());
                        v.example_meaning = tv.example_meaning.clone();
                        v.themed = true;
                    }
                    _ => return Err(format!("vocabulary '{}': give an example, the term is already in the lesson", tv.term)),
                }
            }
            None => {
                if tv.meaning.trim().is_empty() {
                    return Err(format!("vocabulary '{}' is new to the lesson, so it needs a meaning", tv.term));
                }
                out.vocabulary.push(VocabEntry {
                    term: tv.term.clone(),
                    meaning: tv.meaning.clone(),
                    example: tv.example.clone(),
                    example_meaning: tv.example_meaning.clone(),
                    themed: true,
                });
            }
        }
    }
    fn front<T: Clone>(themed: &[T], own: &[T], mark: impl Fn(&mut T)) -> Vec<T> {
        let mut out: Vec<T> = themed.to_vec();
        out.iter_mut().for_each(mark);
        out.extend_from_slice(own);
        out
    }
    out.quiz = front(&t.quiz, &lesson.quiz, |q| q.themed = true);
    out.listening = front(&t.listening, &lesson.listening, |l| l.themed = true);
    out.conversations = front(&t.conversations, &lesson.conversations, |c| c.themed = true);
    out.sprint.bites = front(&t.bites, &lesson.sprint.bites, |b| b.themed = true);
    out.sprint.sorts = front(&t.sorts, &lesson.sprint.sorts, |s| s.themed = true);
    out.sprint.spots = front(&t.spots, &lesson.sprint.spots, |s| s.themed = true);
    out.sprint.builds = front(&t.builds, &lesson.sprint.builds, |b| b.themed = true);
    Ok(Some(out))
}

fn default_order() -> u32 {
    100
}

fn default_group() -> String {
    "general".into()
}

// ====================================================================
// Store
// ====================================================================

/// In-memory, immutable course built once from its files.
pub struct ContentStore {
    manifest: Manifest,
    /// Lessons sorted by (unit, order, title).
    lessons: Vec<Lesson>,
    by_id: HashMap<String, usize>,
    by_slug: HashMap<String, usize>,
    /// question_id -> (lesson index, quiz item index)
    questions: HashMap<String, (usize, usize)>,
    /// conversation id -> (lesson index, conversation index)
    conversations: HashMap<String, (usize, usize)>,
    /// (item, set) -> (lesson index, conjugation index)
    conjugations: HashMap<(String, String), (usize, usize)>,
    /// item -> ASCII art (first lesson that draws it)
    item_art: HashMap<String, Vec<String>>,
    /// Non-lesson course files (the SVG pictures), by path.
    assets: HashMap<String, String>,
    /// The theme this store was built with (its material merged into the lessons), if any.
    theme: Option<Theme>,
    /// `placement.json`, the placement test (#1106), when the course has one.
    placement: Option<crate::placement::PlacementTest>,
}

impl fmt::Debug for ContentStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentStore({} · {} lessons", self.manifest.id, self.lessons.len())?;
        if let Some(t) = &self.theme {
            write!(f, " · theme {}", t.id)?;
        }
        write!(f, ")")
    }
}

#[derive(Debug)]
pub enum ContentError {
    Io(PathBuf, std::io::Error),
    Parse(String, serde_json::Error),
    Duplicate(String),
    /// Two things of a course that must be unique share an id. `what` says
    /// which ("lesson id"), `files` the lesson files holding them (one when it
    /// is the same file or the other is a theme's), `note` the theme or set.
    Clash { what: &'static str, id: String, files: Vec<String>, note: Option<String> },
    MissingPrerequisite { lesson: String, prerequisite: String },
    Invalid(String),
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContentError::Io(p, e) => write!(f, "{}: {}", p.display(), e),
            ContentError::Parse(p, e) => write!(f, "{p}: {e}"),
            ContentError::Duplicate(what) => write!(f, "duplicate {what}"),
            ContentError::Clash { what, id, note, .. } => match note {
                Some(n) => write!(f, "duplicate {what} '{id}' ({n})"),
                None => write!(f, "duplicate {what} '{id}'"),
            },
            ContentError::MissingPrerequisite { lesson, prerequisite } => {
                write!(f, "lesson '{lesson}' references unknown prerequisite '{prerequisite}'")
            }
            ContentError::Invalid(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ContentError {}

pub const MANIFEST_FILE: &str = "course.json";

/// A course file as text: a path relative to the course root and its content.
#[derive(Debug, Clone)]
pub struct CourseFile {
    pub path: String,
    pub content: String,
}

impl ContentStore {
    /// Load a course directory: `course.json` plus every `*.json` under `lessons/`.
    pub fn load_dir(dir: &Path) -> Result<ContentStore, ContentError> {
        ContentStore::load_dir_themed(dir, None)
    }

    /// `load_dir` with a theme switched on.
    pub fn load_dir_themed(dir: &Path, theme: Option<&Theme>) -> Result<ContentStore, ContentError> {
        ContentStore::from_files_themed(&read_course_dir(dir)?, theme)
    }

    /// Build a course from its files (the manifest at `course.json`, lessons
    /// in any other `.json` file). This is the single entry point for built-in
    /// and user-made courses alike. Every theme's material is validated here
    /// too, whichever theme (if any) the store is built with.
    pub fn from_files(files: &[CourseFile]) -> Result<ContentStore, ContentError> {
        ContentStore::from_files_themed(files, None)
    }

    /// `from_files` with a theme switched on: the lessons' material for it
    /// is merged in (`apply_theme`) and its strings replace the manifest's.
    /// Lessons without material for it are unchanged.
    pub fn from_files_themed(files: &[CourseFile], theme: Option<&Theme>) -> Result<ContentStore, ContentError> {
        let manifest_src = files.iter().find(|f| f.path == MANIFEST_FILE).ok_or_else(|| ContentError::Invalid(format!("the course has no {MANIFEST_FILE}")))?;
        let mut manifest: Manifest = serde_json::from_str(&manifest_src.content).map_err(|e| ContentError::Parse(MANIFEST_FILE.into(), e))?;
        validate_manifest(&manifest)?;

        let assets: HashMap<String, String> = files.iter().filter(|f| is_picture_path(&f.path)).map(|f| (f.path.clone(), f.content.clone())).collect();
        let active_theme = theme.cloned();
        if let Some(t) = &active_theme {
            // The course's lines for this theme first (they are in its
            // language), then the theme's own (English, for any course).
            let course = manifest.themes.get(&t.id).map(|c| c.strings.clone()).unwrap_or_default();
            for (lines, mine, theirs) in [
                (&mut manifest.strings.praise, course.praise, &t.strings.praise),
                (&mut manifest.strings.almost, course.almost, &t.strings.almost),
            ] {
                if !mine.is_empty() {
                    *lines = mine;
                } else if !theirs.is_empty() {
                    *lines = theirs.clone();
                }
            }
        }

        // Ids a theme adds (question, conversation) must be unique within that
        // theme and never collide with the lessons' own, since a themed store
        // is "the course plus one theme".
        let mut themed_questions: HashMap<String, HashSet<String>> = HashMap::new();
        let mut themed_conversations: HashMap<String, HashSet<String>> = HashMap::new();
        let mut lessons = Vec::new();
        let placement = files.iter().find(|f| f.path == crate::placement::FILE).map(|f| crate::placement::parse(&f.content, &manifest)).transpose()?;
        for f in files.iter().filter(|f| f.path != MANIFEST_FILE && f.path != crate::placement::FILE && f.path.ends_with(".json")) {
            let mut lesson: Lesson =
                serde_json::from_str(&f.content).map_err(|e| ContentError::Parse(f.path.clone(), e))?;
            validate_lesson(&lesson, &manifest, &f.path)?;
            rename_theme_aliases(&mut lesson);
            // Every theme block is validated whichever theme is on; a block for
            // a theme this install does not have is simply never shown.
            for (id, t) in &lesson.themes {
                if !is_slug(id) {
                    return Err(ContentError::Invalid(format!("{}: themes: '{id}' is not a theme id (lowercase a-z, 0-9, '-')", f.path)));
                }
                let merged = apply_theme(&lesson, id).map_err(|m| ContentError::Invalid(format!("{}: themes.{id}: {m}", f.path)))?;
                if let Some(merged) = &merged {
                    validate_lesson(merged, &manifest, &format!("{} (theme '{id}')", f.path))?;
                }
                for q in &t.quiz {
                    if !themed_questions.entry(id.clone()).or_default().insert(q.question_id.clone()) {
                        return Err(ContentError::Clash { what: "question id", id: q.question_id.clone(), files: vec![f.path.clone()], note: Some(format!("theme '{id}'")) });
                    }
                }
                for c in &t.conversations {
                    if !themed_conversations.entry(id.clone()).or_default().insert(c.id.clone()) {
                        return Err(ContentError::Clash { what: "conversation id", id: c.id.clone(), files: vec![f.path.clone()], note: Some(format!("theme '{id}'")) });
                    }
                }
            }
            if let Some(t) = &active_theme {
                if let Some(merged) = apply_theme(&lesson, &t.id).expect("validated above") {
                    lesson = merged;
                }
            }
            normalise_quiz(&mut lesson.quiz);
            crate::notation::fill_bar::answer_missing(&mut lesson.quiz);
            crate::notation::name_notes(&mut lesson.quiz, manifest.music.as_ref().map(|m| m.note_names).unwrap_or_default()).map_err(|m| ContentError::Invalid(format!("{}: {m}", f.path)))?;
            lesson.unit_index = manifest.unit_index(&lesson.unit).expect("validated");
            lesson.html_content = render_markdown(&lesson.content);
            lesson.sections = split_sections(&lesson.content);
            lesson.guide_steps =
                build_guide(&lesson).map_err(|m| ContentError::Invalid(format!("{}: guide: {m}", f.path)))?;
            lessons.push((lesson, f.path.clone()));
        }

        lessons.sort_by(|(a, _), (b, _)| (a.unit_index, a.order, a.title.as_str()).cmp(&(b.unit_index, b.order, b.title.as_str())));
        let (lessons, paths): (Vec<Lesson>, Vec<String>) = lessons.into_iter().unzip();
        // The files of lessons `a` and `b` (once when they are the same).
        let both = |a: usize, b: usize| if a == b { vec![paths[a].clone()] } else { vec![paths[a].clone(), paths[b].clone()] };

        let mut by_id = HashMap::new();
        let mut by_slug = HashMap::new();
        let mut questions = HashMap::new();
        let mut conversations = HashMap::new();
        let mut conjugations = HashMap::new();
        let mut item_art: HashMap<String, Vec<String>> = HashMap::new();
        for (li, lesson) in lessons.iter().enumerate() {
            if let Some(prev) = by_id.insert(lesson.id.clone(), li) {
                return Err(ContentError::Clash { what: "lesson id", id: lesson.id.clone(), files: both(prev, li), note: None });
            }
            if let Some(prev) = by_slug.insert(lesson.slug.clone(), li) {
                return Err(ContentError::Clash { what: "lesson slug", id: lesson.slug.clone(), files: both(prev, li), note: None });
            }
            for (qi, q) in lesson.quiz.iter().enumerate() {
                if let Some((prev, _)) = questions.insert(q.question_id.clone(), (li, qi)) {
                    return Err(ContentError::Clash { what: "question id", id: q.question_id.clone(), files: both(prev, li), note: None });
                }
                if !q.themed {
                    if let Some(theme) = themed_questions.iter().find(|(_, ids)| ids.contains(&q.question_id)).map(|(t, _)| t) {
                        let note = Some(format!("theme '{theme}' uses it too"));
                        return Err(ContentError::Clash { what: "question id", id: q.question_id.clone(), files: vec![paths[li].clone()], note });
                    }
                }
            }
            for (ci, c) in lesson.conversations.iter().enumerate() {
                if let Some((prev, _)) = conversations.insert(c.id.clone(), (li, ci)) {
                    return Err(ContentError::Clash { what: "conversation id", id: c.id.clone(), files: both(prev, li), note: None });
                }
                if !c.themed {
                    if let Some(theme) = themed_conversations.iter().find(|(_, ids)| ids.contains(&c.id)).map(|(t, _)| t) {
                        let note = Some(format!("theme '{theme}' uses it too"));
                        return Err(ContentError::Clash { what: "conversation id", id: c.id.clone(), files: vec![paths[li].clone()], note });
                    }
                }
            }
            for (ki, k) in lesson.conjugations.iter().enumerate() {
                if let Some((prev, _)) = conjugations.insert((k.item.clone(), k.set.clone()), (li, ki)) {
                    let note = Some(format!("in set '{}'", k.set));
                    return Err(ContentError::Clash { what: "conjugation", id: k.item.clone(), files: both(prev, li), note });
                }
                if !k.art.is_empty() {
                    item_art.entry(k.item.clone()).or_insert_with(|| k.art.clone());
                }
            }
        }
        for lesson in &lessons {
            for p in &lesson.prerequisites {
                if !by_id.contains_key(p) {
                    return Err(ContentError::MissingPrerequisite { lesson: lesson.id.clone(), prerequisite: p.clone() });
                }
            }
        }

        Ok(ContentStore { manifest, lessons, by_id, by_slug, questions, conversations, conjugations, item_art, assets, theme: active_theme, placement })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The theme this store was built with, if any.
    pub fn theme(&self) -> Option<&Theme> {
        self.theme.as_ref()
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn title(&self) -> &str {
        &self.manifest.title
    }

    pub fn units(&self) -> &[Unit] {
        &self.manifest.units
    }

    pub fn unit(&self, index: usize) -> &Unit {
        &self.manifest.units[index.min(self.manifest.units.len().saturating_sub(1))]
    }

    /// The levels a learner may start this course at: the indexes of the
    /// units that have lessons, when the manifest says its units are
    /// `levels` and there are at least two of them; empty otherwise (no
    /// picker).
    pub fn start_units(&self) -> Vec<usize> {
        if !self.manifest.levels {
            return Vec::new();
        }
        let units: Vec<usize> = (0..self.units().len()).filter(|&u| self.lessons_in(u).next().is_some()).collect();
        if units.len() < 2 { Vec::new() } else { units }
    }

    /// The unit index a learner who chose to start at unit `id` starts at:
    /// 0 for '' (the beginning), for a course without levels, and for an id
    /// the course no longer has.
    pub fn start_index(&self, id: &str) -> usize {
        if id.is_empty() {
            return 0;
        }
        self.manifest.unit_index(id).filter(|u| self.start_units().contains(u)).unwrap_or(0)
    }

    /// The placement test, for a course a learner may start further in.
    pub fn placement(&self) -> Option<&crate::placement::PlacementTest> { self.placement.as_ref().filter(|_| !self.start_units().is_empty()) }

    pub fn paradigm(&self) -> Option<&Paradigm> {
        self.manifest.paradigm.as_ref()
    }

    pub fn group_title(&self, id: &str) -> String {
        self.manifest.group_title(id)
    }

    pub fn lessons(&self) -> &[Lesson] {
        &self.lessons
    }

    pub fn lesson_by_id(&self, id: &str) -> Option<&Lesson> {
        self.by_id.get(id).map(|&i| &self.lessons[i])
    }

    pub fn lesson_by_slug(&self, slug: &str) -> Option<&Lesson> {
        self.by_slug.get(slug).map(|&i| &self.lessons[i])
    }

    pub fn question(&self, question_id: &str) -> Option<(&Lesson, &QuizItem)> {
        self.questions.get(question_id).map(|&(li, qi)| {
            let lesson = &self.lessons[li];
            (lesson, &lesson.quiz[qi])
        })
    }

    pub fn question_count(&self) -> usize {
        self.questions.len()
    }

    /// Vocabulary entries a correct answer to `item` teaches: the explicit
    /// `teaches` list plus any entry whose term matches an accepted answer.
    pub fn words_taught<'a>(&self, lesson: &'a Lesson, item: &QuizItem) -> Vec<&'a VocabEntry> {
        let answers: Vec<String> = std::iter::once(&item.correct_answer).chain(item.accepted_answers.iter()).map(|a| crate::quiz::normalize(a)).collect();
        let mut out: Vec<&VocabEntry> = Vec::new();
        for entry in &lesson.vocabulary {
            let explicit = item.teaches.iter().any(|t| t == &entry.term);
            let automatic = answers.iter().any(|a| a == &crate::quiz::normalize(&entry.term));
            if (explicit || automatic) && !out.iter().any(|e| e.term == entry.term) {
                out.push(entry);
            }
        }
        out
    }

    pub fn conversation(&self, id: &str) -> Option<(&Lesson, &Conversation)> {
        self.conversations.get(id).map(|&(li, ci)| {
            let lesson = &self.lessons[li];
            (lesson, &lesson.conversations[ci])
        })
    }

    /// All conversations with their lessons, in lesson order.
    pub fn conversations(&self) -> impl Iterator<Item = (&Lesson, &Conversation)> {
        self.lessons.iter().flat_map(|l| l.conversations.iter().map(move |c| (l, c)))
    }

    /// All conjugations with their lessons, in lesson order.
    pub fn conjugations(&self) -> impl Iterator<Item = (&Lesson, &Conjugation)> {
        self.lessons.iter().flat_map(|l| l.conjugations.iter().map(move |k| (l, k)))
    }

    pub fn conjugation(&self, item: &str, set: &str) -> Option<(&Lesson, &Conjugation)> {
        self.conjugations.get(&(item.to_string(), set.to_string())).map(|&(li, ki)| {
            let lesson = &self.lessons[li];
            (lesson, &lesson.conjugations[ki])
        })
    }

    pub fn item_art(&self, item: &str) -> &[String] {
        self.item_art.get(item).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn lessons_in(&self, unit_index: usize) -> impl Iterator<Item = &Lesson> {
        self.lessons.iter().filter(move |l| l.unit_index == unit_index)
    }

    /// A non-lesson course file (an SVG picture) by its path, e.g. `pictures/kitchen.svg`.
    pub fn asset(&self, path: &str) -> Option<&str> {
        self.assets.get(path).map(String::as_str)
    }

    /// Does any lesson carry a paradigm table?
    pub fn has_conjugations(&self) -> bool {
        self.paradigm().is_some() && self.lessons.iter().any(|l| !l.conjugations.is_empty())
    }

    /// Does any lesson carry listening sentences (and can the browser be
    /// expected to speak them)? Listening practice needs a language course.
    pub fn has_listening(&self) -> bool {
        self.manifest.is_language() && self.lessons.iter().any(|l| !l.listening.is_empty())
    }

    /// Counts for course cards: (lessons, questions, terms).
    pub fn totals(&self) -> (usize, usize, usize) {
        (self.lessons.len(), self.questions.len(), self.lessons.iter().map(|l| l.vocabulary.len()).sum())
    }
}

/// The files of a course directory as `from_files` takes them: `course.json`,
/// `placement.json`, every `*.json` under `lessons/` and every picture, by path (sorted).
pub fn read_course_dir(dir: &Path) -> Result<Vec<CourseFile>, ContentError> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let mut files = vec![CourseFile {
        path: MANIFEST_FILE.into(),
        content: fs::read_to_string(&manifest_path).map_err(|e| ContentError::Io(manifest_path.clone(), e))?,
    }];
    if let Ok(content) = fs::read_to_string(dir.join(crate::placement::FILE)) {
        files.push(CourseFile { path: crate::placement::FILE.into(), content });
    }
    let lessons_dir = dir.join("lessons");
    let mut paths = Vec::new();
    collect_files(&lessons_dir, "json", &mut paths)?;
    collect_files(&dir.join(PICTURES_DIR.trim_end_matches('/')), PICTURE_EXT.trim_start_matches('.'), &mut paths)?;
    paths.sort();
    for p in paths {
        let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        files.push(CourseFile { path: rel, content: fs::read_to_string(&p).map_err(|e| ContentError::Io(p.clone(), e))? });
    }
    Ok(files)
}

fn collect_files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> Result<(), ContentError> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|e| ContentError::Io(dir.to_path_buf(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| ContentError::Io(dir.to_path_buf(), e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, ext, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    Ok(())
}

/// `pictures/<anything>.svg`: a picture file of the course.
/// `pictures/<name>.svg`, with the characters `courses::valid_path` allows
/// (#387): ASCII letters, digits, `-`, `_` and `.` in non-empty segments,
/// no `..`, so a path is safe in any attribute or URL it is written into.
pub fn is_picture_path(path: &str) -> bool {
    path.strip_prefix(PICTURES_DIR)
        .is_some_and(|rest| rest.len() > PICTURE_EXT.len() && rest.ends_with(PICTURE_EXT) && is_safe_rel_path(rest))
}

/// A relative path of at most 120 bytes whose segments are non-empty, not
/// `..`, and only ASCII letters, digits, `-`, `_` and `.`.
pub fn is_safe_rel_path(rest: &str) -> bool {
    !rest.is_empty()
        && rest.len() <= 120
        && rest.split('/').all(|seg| {
            !seg.is_empty() && seg != ".." && seg.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

pub fn is_slug(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_manifest(m: &Manifest) -> Result<(), ContentError> {
    let ctx = |msg: String| ContentError::Invalid(format!("{MANIFEST_FILE}: {msg}"));
    if !is_slug(&m.id) {
        return Err(ctx(format!("id '{}' must be lowercase a-z, 0-9 and '-'", m.id)));
    }
    if m.title.trim().is_empty() {
        return Err(ctx("title must not be empty".into()));
    }
    if m.units.is_empty() {
        return Err(ctx("units must list at least one unit".into()));
    }
    let mut seen = HashSet::new();
    for u in &m.units {
        if u.id.trim().is_empty() || u.title.trim().is_empty() {
            return Err(ctx("every unit needs an id and a title".into()));
        }
        if !seen.insert(u.id.as_str()) {
            return Err(ctx(format!("duplicate unit id '{}'", u.id)));
        }
    }
    let mut seen = HashSet::new();
    for g in &m.groups {
        if g.id.trim().is_empty() || g.title.trim().is_empty() {
            return Err(ctx("every group needs an id and a title".into()));
        }
        if !seen.insert(g.id.as_str()) {
            return Err(ctx(format!("duplicate group id '{}'", g.id)));
        }
    }
    if let Some(p) = &m.paradigm {
        if p.slots.len() < 2 || p.slots.len() > 12 {
            return Err(ctx("paradigm needs 2 to 12 slots".into()));
        }
        if p.sets.is_empty() {
            return Err(ctx("paradigm needs at least one set".into()));
        }
        let mut seen = HashSet::new();
        for s in &p.slots {
            if s.key.trim().is_empty() || s.label.trim().is_empty() {
                return Err(ctx("every paradigm slot needs a key and a label".into()));
            }
            if !seen.insert(s.key.as_str()) {
                return Err(ctx(format!("duplicate paradigm slot '{}'", s.key)));
            }
            if s.art.iter().any(|line| line.chars().count() > 24) || s.art.len() > 8 {
                return Err(ctx(format!("art for slot '{}' must be at most 8 lines of 24 characters", s.key)));
            }
        }
        let mut seen = HashSet::new();
        for s in &p.sets {
            if s.key.trim().is_empty() || s.label.trim().is_empty() {
                return Err(ctx("every paradigm set needs a key and a label".into()));
            }
            if !seen.insert(s.key.as_str()) {
                return Err(ctx(format!("duplicate paradigm set '{}'", s.key)));
            }
        }
    }
    if m.chars.iter().any(|c| c.chars().count() != 1) {
        return Err(ctx("chars must be single characters".into()));
    }
    if let Some(cal) = &m.calendar {
        if cal.days.len() != 7 {
            return Err(ctx(format!("calendar.days must list the 7 weekday names (Monday first), found {}", cal.days.len())));
        }
        if cal.months.len() != 12 {
            return Err(ctx(format!("calendar.months must list the 12 month names (January first), found {}", cal.months.len())));
        }
        if cal.days.iter().chain(cal.months.iter()).any(|n| n.trim().is_empty()) {
            return Err(ctx("calendar names must not be empty".into()));
        }
        let mut seen = HashSet::new();
        if !cal.days.iter().all(|d| seen.insert(d.trim().to_lowercase())) {
            return Err(ctx("calendar.days must be distinct".into()));
        }
        let mut seen = HashSet::new();
        if !cal.months.iter().all(|d| seen.insert(d.trim().to_lowercase())) {
            return Err(ctx("calendar.months must be distinct".into()));
        }
        if !cal.format.contains("{day}") || !cal.format.contains("{month}") {
            return Err(ctx("calendar.format must contain {day} and {month}".into()));
        }
    }
    for (id, t) in &m.themes {
        if t.strings.praise.iter().chain(t.strings.almost.iter()).any(|s| s.trim().is_empty()) {
            return Err(ctx(format!("themes.{id}.strings must not be empty")));
        }
    }
    Ok(())
}

/// Checks a theme file: ids, colours (hex only), font, art paths, strings
/// and pictures. Whether the art files exist is checked by the loader, which
/// has the theme's files.
pub fn validate_theme(t: &Theme) -> Result<(), String> {
    if !is_slug(&t.id) {
        return Err(format!("theme id '{}' must be lowercase a-z, 0-9 and '-'", t.id));
    }
    if t.title.trim().is_empty() {
        return Err(format!("theme '{}' needs a title", t.id));
    }
    for (what, colors) in [("colors", &t.colors), ("dark", &t.dark)] {
        for (k, v) in colors {
            if !THEME_COLOR_KEYS.contains(&k.as_str()) {
                return Err(format!("theme '{}': unknown {what} token '{k}' (tokens: {})", t.id, THEME_COLOR_KEYS.join(", ")));
            }
            if !is_hex_color(v) {
                return Err(format!("theme '{}': {what}.{k} must be a hex colour like #c0392b, found '{v}'", t.id));
            }
        }
    }
    if !t.font.is_empty() && !is_font_stack(&t.font) {
        return Err(format!("theme '{}': font must be a font-family list such as \"Georgia, serif\"", t.id));
    }
    for (what, path) in [("banner", &t.art.banner), ("mark", &t.art.mark)] {
        if !path.is_empty() && !is_url(path) && !is_picture_path(path) {
            return Err(format!("theme '{}': {what} '{path}' must be a file of the theme {PICTURES_DIR}<name>{PICTURE_EXT} or an https:// URL", t.id));
        }
    }
    if t.strings.praise.iter().chain(t.strings.almost.iter()).any(|s| s.trim().is_empty()) {
        return Err(format!("theme '{}': strings must not be empty", t.id));
    }
    for (k, v) in &t.icons {
        if !ICON_KEYS.iter().any(|(key, _)| key == k) {
            let known: Vec<&str> = ICON_KEYS.iter().map(|(key, _)| *key).collect();
            return Err(format!("theme '{}': unknown icon slot '{k}' (slots: {})", t.id, known.join(", ")));
        }
        if !is_icon(v) {
            return Err(format!("theme '{}': icons.{k} must be a short glyph such as an emoji, found '{v}'", t.id));
        }
    }
    if let Some(bad) = t.lesson_icons.iter().find(|v| !is_icon(v)) {
        return Err(format!("theme '{}': lesson_icons must be short glyphs such as emoji, found '{bad}'", t.id));
    }
    let mut seen = HashSet::new();
    for a in &t.avatars {
        if !crate::profile::is_avatar(&a.id) {
            let known: Vec<&str> = crate::profile::AVATARS.iter().map(|b| b.id).collect();
            return Err(format!("theme '{}': avatar '{}' is not one of the profile pictures ({})", t.id, a.id, known.join(", ")));
        }
        if !seen.insert(crate::profile::canonical(&a.id).unwrap_or_default()) {
            return Err(format!("theme '{}': duplicate avatar '{}'", t.id, a.id));
        }
        if !is_url(&a.image) && !is_picture_path(&a.image) {
            return Err(format!("theme '{}': avatar '{}': image '{}' must be a file of the theme {PICTURES_DIR}<name>{PICTURE_EXT} or an https:// URL", t.id, a.id, a.image));
        }
        if !a.color.is_empty() && !is_hex_color(&a.color) {
            return Err(format!("theme '{}': avatar '{}': color must be a hex colour like #c0392b, found '{}'", t.id, a.id, a.color));
        }
    }
    let mut slots = HashSet::new();
    for c in &t.avatar_colors {
        let n = crate::profile::AVATAR_COLORS.len();
        if c.slot == 0 || c.slot as usize > n {
            return Err(format!("theme '{}': avatar_colors slot {} must be 1 to {n}", t.id, c.slot));
        }
        if !slots.insert(c.slot) {
            return Err(format!("theme '{}': duplicate avatar_colors slot {}", t.id, c.slot));
        }
        if c.label.trim().is_empty() {
            return Err(format!("theme '{}': avatar_colors slot {} needs a label (the colour's name)", t.id, c.slot));
        }
        if !is_hex_color(&c.color) {
            return Err(format!("theme '{}': avatar_colors slot {}: color must be a hex colour like #c0392b, found '{}'", t.id, c.slot, c.color));
        }
    }
    let mut slots = HashSet::new();
    for b in &t.avatar_backgrounds {
        let (lo, hi) = (crate::profile::THEME_SLOTS.start(), crate::profile::THEME_SLOTS.end());
        if !crate::profile::THEME_SLOTS.contains(&b.slot) {
            return Err(format!("theme '{}': avatar_backgrounds slot {} must be {lo} to {hi}", t.id, b.slot));
        }
        if !slots.insert(b.slot) {
            return Err(format!("theme '{}': duplicate avatar_backgrounds slot {}", t.id, b.slot));
        }
        if b.label.trim().is_empty() {
            return Err(format!("theme '{}': avatar_backgrounds slot {} needs a label (the background's name)", t.id, b.slot));
        }
        if !is_url(&b.image) && !is_picture_path(&b.image) {
            return Err(format!("theme '{}': avatar_backgrounds slot {}: image '{}' must be a file of the theme {PICTURES_DIR}<name>{PICTURE_EXT} or an https:// URL", t.id, b.slot, b.image));
        }
        if !is_hex_color(&b.color) {
            return Err(format!("theme '{}': avatar_backgrounds slot {}: color must be a hex colour like #c0392b, found '{}'", t.id, b.slot, b.color));
        }
    }
    Ok(())
}

/// "true"/"false" (any case, also yes/no) as a bool.
fn true_false(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

/// Turn every `true-false` question into a two-option multiple choice, so
/// the quiz, Sprint mode, the Guide, the reading check and the tutor serve it
/// like any other choice.
fn normalise_quiz(quiz: &mut [QuizItem]) {
    for q in quiz.iter_mut().filter(|q| q.kind == QuizType::TrueFalse) {
        let right = true_false(&q.correct_answer).unwrap_or(true);
        q.kind = QuizType::MultipleChoice;
        q.options = vec!["True".into(), "False".into()];
        q.correct_answer = if right { "True" } else { "False" }.into();
    }
}

fn validate_lesson(lesson: &Lesson, m: &Manifest, path: &str) -> Result<(), ContentError> {
    let ctx = |msg: String| ContentError::Invalid(format!("{path}: {msg}"));
    if lesson.id.trim().is_empty() || lesson.slug.trim().is_empty() {
        return Err(ctx("lesson id and slug must not be empty".into()));
    }
    if !is_slug(&lesson.slug) {
        return Err(ctx(format!("slug '{}' must be lowercase a-z, 0-9 and '-'", lesson.slug)));
    }
    if lesson.title.trim().is_empty() {
        return Err(ctx("title must not be empty".into()));
    }
    if m.unit_index(&lesson.unit).is_none() {
        let known: Vec<&str> = m.units.iter().map(|u| u.id.as_str()).collect();
        return Err(ctx(format!("unit '{}' is not in the manifest (units: {})", lesson.unit, known.join(", "))));
    }
    if lesson.group.trim().is_empty() {
        return Err(ctx("group must not be empty".into()));
    }
    for q in &lesson.quiz {
        if q.question_id.trim().is_empty() {
            return Err(ctx("question_id must not be empty".into()));
        }
        if q.prompt.trim().is_empty() {
            return Err(ctx(format!("question '{}' has an empty prompt", q.question_id)));
        }
        match q.kind {
            QuizType::MultipleChoice => {
                if q.options.len() < 2 {
                    return Err(ctx(format!("question '{}' needs at least 2 options", q.question_id)));
                }
                if !q.options.iter().any(|o| o == &q.correct_answer) {
                    return Err(ctx(format!("question '{}': correct_answer must be one of the options", q.question_id)));
                }
            }
            QuizType::FillInBlank => {
                if q.correct_answer.trim().is_empty() {
                    return Err(ctx(format!("question '{}' has an empty correct_answer", q.question_id)));
                }
            }
            QuizType::TrueFalse => {
                if true_false(&q.correct_answer).is_none() {
                    return Err(ctx(format!("question '{}': a true-false correct_answer must be \"true\" or \"false\"", q.question_id)));
                }
            }
            QuizType::Numeric | QuizType::Expression => {
                crate::mathcheck::validate_answers(q).map_err(|(_, e)| ctx(format!("question '{}': {e}", q.question_id)))?;
                if q.kind == QuizType::Numeric && q.tolerance.is_some_and(|t| !(t >= 0.0 && t.is_finite())) {
                    return Err(ctx(format!("question '{}': tolerance must be a number >= 0", q.question_id)));
                }
            }
            QuizType::FillBar => crate::notation::fill_bar::check(q).map_err(|e| ctx(format!("question '{}': {e}", q.question_id)))?,
            QuizType::ChordSpell => crate::notation::chord_spell::check(q).map_err(|e| ctx(format!("question '{}': {e}", q.question_id)))?,
            QuizType::StaffNote => crate::notation::staff_note::check(q).map_err(|e| ctx(format!("question '{}': {e}", q.question_id)))?,
            QuizType::Keyboard => crate::notation::keyboard::check(q).map_err(|e| ctx(format!("question '{}': {e}", q.question_id)))?,
            QuizType::AlgebraTiles => crate::algebra_tiles::validate(q).map_err(|e| ctx(format!("question '{}': {e}", q.question_id)))?,
            QuizType::Clock => crate::clock::validate(q).map_err(|e| ctx(format!("question '{}': {e}", q.question_id)))?,
            QuizType::Measure => {}
        }
        if q.kind != QuizType::Clock && q.step_minutes.is_some() {
            return Err(ctx(format!("question '{}': step_minutes is for clock questions", q.question_id)));
        }
        crate::measure::validate(q).map_err(|e| ctx(format!("question '{}': {e}", q.question_id)))?;
        if !q.kind.is_math() && (q.tolerance.is_some() || !q.unit.is_empty()) {
            return Err(ctx(format!("question '{}': tolerance and unit are for numeric questions", q.question_id)));
        }
    }
    for q in &lesson.quiz {
        for t in &q.teaches {
            if !lesson.vocabulary.iter().any(|v| &v.term == t) {
                return Err(ctx(format!("question '{}' teaches '{}', which is not in this lesson's vocabulary", q.question_id, t)));
            }
        }
    }
    let mut seen = HashSet::new();
    for v in &lesson.vocabulary {
        if v.term.trim().is_empty() || v.meaning.trim().is_empty() {
            return Err(ctx("vocabulary entries need a term and a meaning".into()));
        }
        if !seen.insert(v.term.as_str()) {
            return Err(ctx(format!("duplicate vocabulary entry '{}'", v.term)));
        }
    }
    if !lesson.conjugations.is_empty() {
        let Some(p) = &m.paradigm else {
            return Err(ctx("this lesson has conjugations but the manifest defines no paradigm".into()));
        };
        for k in &lesson.conjugations {
            if k.item.trim().is_empty() || k.meaning.trim().is_empty() {
                return Err(ctx("conjugation entries need an item and a meaning".into()));
            }
            if p.set(&k.set).is_none() {
                let known: Vec<&str> = p.sets.iter().map(|s| s.key.as_str()).collect();
                return Err(ctx(format!("conjugation of '{}': set '{}' is not in the paradigm (sets: {})", k.item, k.set, known.join(", "))));
            }
            for s in &p.slots {
                if k.form(&s.key).trim().is_empty() {
                    return Err(ctx(format!("conjugation of '{}' is missing the '{}' form", k.item, s.key)));
                }
            }
            for key in k.forms.keys() {
                if p.slot(key).is_none() {
                    return Err(ctx(format!("conjugation of '{}' has a form for unknown slot '{}'", k.item, key)));
                }
            }
            if k.art.iter().any(|line| line.chars().count() > 24) || k.art.len() > 8 {
                return Err(ctx(format!("art for '{}' must be at most 8 lines of 24 characters", k.item)));
            }
        }
    }
    for (i, sort) in lesson.sprint.sorts.iter().enumerate() {
        if sort.buckets.len() < 2 || sort.buckets.len() > 4 {
            return Err(ctx(format!("sprint sort {} needs 2 to 4 buckets", i + 1)));
        }
        if sort.items.len() < 3 || sort.items.len() > 10 {
            return Err(ctx(format!("sprint sort {} needs 3 to 10 items", i + 1)));
        }
        for it in &sort.items {
            if it.bucket >= sort.buckets.len() {
                return Err(ctx(format!("sprint sort {}: '{}' points at bucket {}, which does not exist", i + 1, it.text, it.bucket)));
            }
            if it.text.trim().is_empty() && it.play.trim().is_empty() {
                return Err(ctx(format!("sprint sort {} has an empty item", i + 1)));
            }
        }
    }
    for (i, spot) in lesson.sprint.spots.iter().enumerate() {
        crate::sprint::spot::validate(spot).map_err(|m| ctx(format!("sprint spot {}{m}", i + 1)))?;
    }
    crate::notation::validate(lesson).map_err(ctx)?;
    for (i, b) in lesson.sprint.builds.iter().enumerate() {
        let n = words_of(&b.text).len();
        if !(2..=12).contains(&n) || b.meaning.trim().is_empty() {
            return Err(ctx(format!("sprint build {} needs a text of 2 to 12 words and a meaning", i + 1)));
        }
    }
    for (i, b) in lesson.sprint.bites.iter().enumerate() {
        if b.title.trim().is_empty() || b.rule.trim().is_empty() {
            return Err(ctx(format!("sprint bite {} needs a title and a rule", i + 1)));
        }
    }
    let mut seen = HashSet::new();
    for (i, item) in lesson.listening.iter().enumerate() {
        let n = words_of(&item.text).len();
        if !(1..=LISTENING_MAX_WORDS).contains(&n) {
            return Err(ctx(format!("listening {} needs a text of 1 to {LISTENING_MAX_WORDS} words", i + 1)));
        }
        if !seen.insert(crate::quiz::normalize(&item.text)) {
            return Err(ctx(format!("listening {}: '{}' appears twice", i + 1, item.text)));
        }
        if item.note.as_deref().is_some_and(|n| n.trim().is_empty()) {
            return Err(ctx(format!("listening {}: note must not be empty (omit it instead)", i + 1)));
        }
    }
    for c in &lesson.conversations {
        if c.id.trim().is_empty() {
            return Err(ctx("conversation id must not be empty".into()));
        }
        if c.lines.len() < 2 {
            return Err(ctx(format!("conversation '{}' needs at least 2 lines", c.id)));
        }
        if !c.lines.iter().any(|l| l.speaker == Speaker::A) || !c.lines.iter().any(|l| l.speaker == Speaker::B) {
            return Err(ctx(format!("conversation '{}' must give both roles something to say", c.id)));
        }
        if c.lines.iter().any(|l| l.text.trim().is_empty()) {
            return Err(ctx(format!("conversation '{}' has a line with empty text", c.id)));
        }
    }
    Ok(())
}

/// Longest listening sentence, in words: a dictation, not a paragraph.
pub const LISTENING_MAX_WORDS: usize = 30;

/// Split a sentence into word tiles: whitespace-separated, punctuation kept
/// attached (so "¿Dónde" and "parque." stay single tiles), empties dropped.
pub fn words_of(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// Split Markdown on level-2 headings. Text before the first heading becomes
/// an "Introduction" section (if non-empty).
pub fn split_sections(md: &str) -> Vec<Section> {
    let mut out = Vec::new();
    let mut heading = String::from("Introduction");
    let mut body = String::new();
    let mut in_code = false;
    let flush = |heading: &str, body: &str, out: &mut Vec<Section>| {
        if !body.trim().is_empty() {
            out.push(Section { heading: heading.to_string(), html: render_markdown(body.trim()) });
        }
    };
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        if !in_code {
            if let Some(h) = line.strip_prefix("## ") {
                flush(&heading, &body, &mut out);
                heading = strip_inline_markdown(h.trim());
                body.clear();
                continue;
            }
        }
        body.push_str(line);
        body.push('\n');
    }
    flush(&heading, &body, &mut out);
    out
}

/// Remove `*`, `_` and backtick emphasis markers from a heading for display.
pub fn strip_inline_markdown(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '*' | '_' | '`')).collect::<String>().trim().to_string()
}

/// Turn a lesson into bite-sized Guide steps.
///
/// With an explicit `guide` array in the JSON, each entry is resolved against
/// the lesson. Otherwise: every section becomes a concept card, vocabulary is
/// dealt out in hands of five after the first card, and the quiz questions are
/// interleaved one per card from the third card on, the remainder at the end.
pub fn build_guide(lesson: &Lesson) -> Result<Vec<GuideStep>, String> {
    let mut steps = Vec::new();
    if lesson.guide.is_empty() {
        let mut questions = lesson.quiz.iter().enumerate().map(|(i, _)| i);
        let mut vocab = lesson.vocabulary.chunks(5);
        for (i, sec) in lesson.sections.iter().enumerate() {
            steps.push(GuideStep::Concept { title: sec.heading.clone(), html: sec.html.clone() });
            if i == 0 {
                if let Some(chunk) = vocab.next() {
                    steps.push(GuideStep::Vocab { entries: chunk.to_vec() });
                }
            }
            if i >= 2 {
                if let Some(q) = questions.next() {
                    steps.push(GuideStep::Question { index: q });
                }
            }
        }
        for chunk in vocab {
            steps.push(GuideStep::Vocab { entries: chunk.to_vec() });
        }
        for q in questions {
            steps.push(GuideStep::Question { index: q });
        }
    } else {
        let mut vocab_pos = 0usize;
        for spec in &lesson.guide {
            match spec {
                GuideSpec::Section { section } => {
                    let needle = strip_inline_markdown(section).to_lowercase();
                    let sec = lesson
                        .sections
                        .iter()
                        .find(|s| s.heading.to_lowercase().starts_with(&needle))
                        .ok_or_else(|| format!("no section starting with '{section}'"))?;
                    steps.push(GuideStep::Concept { title: sec.heading.clone(), html: sec.html.clone() });
                }
                GuideSpec::Question { question } => {
                    let index = lesson.quiz.iter().position(|q| &q.question_id == question).ok_or_else(|| format!("question '{question}' is not in this lesson"))?;
                    steps.push(GuideStep::Question { index });
                }
                GuideSpec::Vocab { vocab } => {
                    let end = (vocab_pos + vocab.max(&1)).min(lesson.vocabulary.len());
                    if vocab_pos < end {
                        steps.push(GuideStep::Vocab { entries: lesson.vocabulary[vocab_pos..end].to_vec() });
                        vocab_pos = end;
                    }
                }
                GuideSpec::Tip { tip } => steps.push(GuideStep::Tip { text: tip.clone() }),
                GuideSpec::Concept { concept } => steps.push(GuideStep::Concept {
                    title: concept.title.clone(),
                    html: render_markdown(&concept.markdown),
                }),
            }
        }
        // A worn theme's questions are merged in front of the lesson's own,
        // and no hand-written guide can name them, so they go in after the
        // first card, in their order: the theme's quick checks come first
        // here as everywhere else, and the hand-written order stays.
        let named: HashSet<usize> = steps.iter().filter_map(|s| match s { GuideStep::Question { index } => Some(*index), _ => None }).collect();
        let themed: Vec<GuideStep> =
            lesson.quiz.iter().enumerate().filter(|(i, q)| q.themed && !named.contains(i)).map(|(i, _)| GuideStep::Question { index: i }).collect();
        let at = steps.len().min(1);
        steps.splice(at..at, themed);
    }
    if steps.is_empty() {
        return Err("lesson produced no guide steps (it needs some content, vocabulary or questions)".into());
    }
    Ok(steps)
}

/// Render lesson Markdown to HTML with a conservative option set.
///
/// Course text is untrusted: a learner can upload it, and an AI writes it
/// from pasted material that may carry instructions. So raw HTML in the
/// Markdown is shown as text (escaped), never passed through, and a link or
/// image whose URL has a scheme other than http, https or mailto
/// (`javascript:`, `data:`, `vbscript:`…) points nowhere (`#`).
pub fn render_markdown(md: &str) -> String {
    use pulldown_cmark::{html, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    fn safe(url: CowStr<'_>) -> CowStr<'_> {
        if is_safe_url(&url) { url } else { CowStr::Borrowed("#") }
    }
    // An image from another host would load without a click and tell that
    // host who reads the lesson (#386): it becomes a plain link, its alt text
    // (or else its URL) the link text. Each open image is `None` when it
    // stays an image, or the URL a link still has to show when it has had no
    // text yet.
    let mut images: Vec<Option<Option<String>>> = Vec::new();
    let parser = Parser::new_ext(md, opts).flat_map(move |ev| -> Vec<Event<'_>> {
        match ev {
            Event::Html(raw) | Event::InlineHtml(raw) => vec![Event::Text(raw)],
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => vec![Event::Start(Tag::Link { link_type, dest_url: safe(dest_url), title, id })],
            Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
                if is_inline_image(&dest_url) {
                    images.push(None);
                    vec![Event::Start(Tag::Image { link_type, dest_url, title, id })]
                } else if !is_safe_url(&dest_url) || is_relative_url(&dest_url) {
                    images.push(None);
                    vec![Event::Start(Tag::Image { link_type, dest_url: safe(dest_url), title, id })]
                } else {
                    images.push(Some(Some(dest_url.to_string())));
                    vec![Event::Start(Tag::Link { link_type: LinkType::Inline, dest_url, title, id })]
                }
            }
            Event::End(TagEnd::Image) => match images.pop() {
                Some(Some(Some(url))) => vec![Event::Text(url.into()), Event::End(TagEnd::Link)],
                Some(Some(None)) => vec![Event::End(TagEnd::Link)],
                _ => vec![Event::End(TagEnd::Image)],
            },
            ev => {
                if matches!(ev, Event::Text(_) | Event::Code(_)) {
                    if let Some(Some(url)) = images.last_mut() {
                        *url = None;
                    }
                }
                vec![ev]
            }
        }
    })
    // A wide table (a paradigm, a comparison) scrolls inside its own box on
    // a phone instead of pushing the whole page sideways.
    .flat_map(|ev| match ev {
        Event::Start(Tag::Table(a)) => vec![Event::Html(CowStr::Borrowed(r#"<div class="table-scroll" data-testid="table-scroll">"#)), Event::Start(Tag::Table(a))],
        Event::End(TagEnd::Table) => vec![Event::End(TagEnd::Table), Event::Html(CowStr::Borrowed("</div>"))],
        ev => vec![ev],
    });
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, parser);
    out
}

/// A URL with no scheme and no host: a path on this site (`//host/x` and
/// `\\host/x` name another host). Tabs, newlines and control characters are
/// dropped first, as browsers do.
fn is_relative_url(url: &str) -> bool {
    let cleaned: String = url.chars().filter(|c| !c.is_ascii_whitespace() && !c.is_control()).collect();
    let scheme = cleaned.find(':').is_some_and(|i| !cleaned[..i].contains(['/', '?', '#'])); // panic-ok: i is where find(':') matched, a char boundary
    let mut lead = cleaned.chars().take(2);
    let host = lead.next().is_some_and(|c| c == '/' || c == '\\') && lead.next().is_some_and(|c| c == '/' || c == '\\');
    !scheme && !host
}

/// An image written into the lesson itself (`data:image/…`): it loads
/// nothing from anywhere.
fn is_inline_image(url: &str) -> bool {
    let cleaned: String = url.chars().filter(|c| !c.is_ascii_whitespace() && !c.is_control()).collect::<String>().to_ascii_lowercase();
    cleaned.starts_with("data:image/")
}

/// A link target that cannot run script: relative, or http(s)/mailto.
/// Browsers ignore tabs, newlines and other control characters in a URL's
/// scheme, so they are dropped before looking at it.
pub fn is_safe_url(url: &str) -> bool {
    let cleaned: String = url.chars().filter(|c| !c.is_ascii_whitespace() && !c.is_control()).collect::<String>().to_ascii_lowercase();
    match cleaned.find(':') {
        // A colon after a path, query or fragment starts no scheme.
        Some(i) if !cleaned[..i].contains(['/', '?', '#']) => matches!(&cleaned[..i], "http" | "https" | "mailto"),
        _ => true,
    }
}

#[cfg(test)]
pub(crate) mod tests;
