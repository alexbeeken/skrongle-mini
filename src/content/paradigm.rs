//! The paradigm a course may declare: the rows (slots) and columns (sets)
//! of its `conjugations` tables, and who each slot stands for (#879).

use serde::{Deserialize, Serialize};

/// A row of a paradigm table: a grammatical person for verb conjugations, a
/// case for declensions, a key for scales… `art` is an optional ASCII picture
/// for the drill game; `who` says which person the row is, and the drill then
/// draws profile pictures for it instead (#879).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub art: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<Who>,
}

/// The kind of person a paradigm slot stands for, drawn in the drill with
/// profile pictures: the learner's own, and others chosen at random.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    /// I: the learner's picture.
    Me,
    /// You (one person): another picture, facing the learner.
    You,
    /// He, she: another picture, off to the side.
    Other,
    /// We: the learner and two others.
    Us,
    /// You (several): three others, facing the learner.
    YouAll,
    /// They: three others, smaller and farther away.
    Them,
}

impl Who {
    pub fn as_str(self) -> &'static str {
        match self {
            Who::Me => "me",
            Who::You => "you",
            Who::Other => "other",
            Who::Us => "us",
            Who::YouAll => "you_all",
            Who::Them => "them",
        }
    }
}

/// A column of a paradigm table: a tense, a mood, a number…
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParadigmSet {
    pub key: String,
    pub label: String,
}

/// Optional: the shape of the `conjugations` tables lessons may carry. Without
/// a paradigm the conjugation drill and ladders are simply not offered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paradigm {
    /// Heading for the tables and the game ("Conjugations").
    #[serde(default = "default_paradigm_label")]
    pub label: String,
    /// What one table describes ("Verb", "Noun", "Chord").
    #[serde(default = "default_item_label")]
    pub item_label: String,
    pub slots: Vec<Slot>,
    pub sets: Vec<ParadigmSet>,
}

fn default_paradigm_label() -> String {
    "Conjugations".into()
}

fn default_item_label() -> String {
    "Verb".into()
}

impl Paradigm {
    pub fn slot(&self, key: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.key == key)
    }

    pub fn set(&self, key: &str) -> Option<&ParadigmSet> {
        self.sets.iter().find(|s| s.key == key)
    }

    pub fn set_label<'a>(&'a self, key: &'a str) -> &'a str {
        self.set(key).map(|s| s.label.as_str()).unwrap_or(key)
    }
}
