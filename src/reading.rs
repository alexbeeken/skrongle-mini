//! Reading settings (#233): text size, spacing, letters, contrast and
//! movement, chosen on Profile (`routes/profile.rs`) and kept in
//! `users.reading`. This is the one place that column is read or written:
//! a space-separated list of words (`size-3 wide easy contrast calm`), ''
//! for all standard. The signed-in user carries it (`CurrentUser::reading`);
//! the `CurrentUser` extractor makes it the request's (`set_current`), and
//! every page writes `classes()` on its `<html>` element, so a page is drawn
//! in the chosen look from the start. Signed-out pages stay standard.
//! The CSS is `static/reading.css`.

use std::cell::Cell;

/// The largest text size step (1 is standard).
pub const MAX_SIZE: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Text size, 1 (standard) to `MAX_SIZE`.
    pub size: u8,
    /// Wider spacing between lines, words and letters.
    pub wide: bool,
    /// A typeface whose letters are easier to tell apart.
    pub easy: bool,
    /// Stronger contrast: darker text, firmer borders, underlined links.
    pub contrast: bool,
    /// Less movement: what a device asking for reduced motion gets.
    pub calm: bool,
}

impl Default for Reading {
    fn default() -> Self {
        Reading { size: 1, wide: false, easy: false, contrast: false, calm: false }
    }
}

/// The five settings as the profile page and `POST /api/profile` name
/// them: (field, its values, standard first).
pub const FIELDS: &[(&str, &[&str])] = &[
    ("read_size", &["1", "2", "3", "4"]),
    ("read_spacing", &["standard", "wider"]),
    ("read_letters", &["standard", "easier"]),
    ("read_contrast", &["standard", "stronger"]),
    ("read_motion", &["standard", "less"]),
];

impl Reading {
    /// The stored form; anything it does not know is left out, so an old
    /// or hand-edited value never breaks a page.
    pub fn parse(stored: &str) -> Reading {
        let mut r = Reading::default();
        for word in stored.split_whitespace() {
            match word {
                "wide" => r.wide = true,
                "easy" => r.easy = true,
                "contrast" => r.contrast = true,
                "calm" => r.calm = true,
                w => {
                    if let Some(n) = w.strip_prefix("size-").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=MAX_SIZE).contains(n)) {
                        r.size = n;
                    }
                }
            }
        }
        r
    }

    /// What `users.reading` stores: '' when everything is standard.
    pub fn stored(&self) -> String {
        let mut words = Vec::new();
        if self.size > 1 {
            words.push(format!("size-{}", self.size));
        }
        for (on, w) in [(self.wide, "wide"), (self.easy, "easy"), (self.contrast, "contrast"), (self.calm, "calm")] {
            if on {
                words.push(w.to_string());
            }
        }
        words.join(" ")
    }

    /// The classes on `<html>` (`static/reading.css`): '' when standard.
    pub fn classes(&self) -> String {
        self.stored().split_whitespace().map(|w| format!("read-{w}")).collect::<Vec<_>>().join(" ")
    }

    /// The value of one of `FIELDS` as the page shows it.
    pub fn value(&self, field: &str) -> &'static str {
        let on = match field {
            "read_size" => return FIELDS[0].1[usize::from(self.size.clamp(1, MAX_SIZE) - 1)],
            "read_spacing" => self.wide,
            "read_letters" => self.easy,
            "read_contrast" => self.contrast,
            "read_motion" => self.calm,
            _ => return "",
        };
        FIELDS.iter().find(|(f, _)| *f == field).map_or("", |(_, v)| v[usize::from(on)])
    }

    /// Set one of `FIELDS` from what the page sent; `Err` for a field or
    /// value it does not have.
    pub fn set(&mut self, field: &str, value: &str) -> Result<(), ()> {
        let values = FIELDS.iter().find(|(f, _)| *f == field).map(|(_, v)| *v).ok_or(())?;
        let i = values.iter().position(|v| *v == value).ok_or(())?;
        match field {
            "read_size" => self.size = i as u8 + 1,
            "read_spacing" => self.wide = i == 1,
            "read_letters" => self.easy = i == 1,
            "read_contrast" => self.contrast = i == 1,
            "read_motion" => self.calm = i == 1,
            _ => return Err(()),
        }
        Ok(())
    }
}

/// `users.reading` of an account.
pub fn load(conn: &rusqlite::Connection, user_id: i64) -> rusqlite::Result<Reading> {
    use rusqlite::OptionalExtension;
    let s: Option<String> = conn.query_row("SELECT reading FROM users WHERE id = ?1", [user_id], |r| r.get(0)).optional()?;
    Ok(Reading::parse(&s.unwrap_or_default()))
}

pub fn save(conn: &rusqlite::Connection, user_id: i64, reading: &Reading) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET reading = ?2 WHERE id = ?1", rusqlite::params![user_id, reading.stored()])?;
    Ok(())
}

tokio::task_local! {
    /// The reading settings of the request being handled.
    static CURRENT: Cell<Reading>;
}

/// The request's reading settings (standard outside one, or signed out).
pub fn current() -> Reading {
    CURRENT.try_with(|c| c.get()).unwrap_or_default()
}

/// The classes for the page's `<html>` element (`templates/reading_head.html`).
pub fn classes() -> String {
    current().classes()
}

/// Make `r` the request's: the `CurrentUser` extractor calls this with the
/// account's settings, as it does with its language.
pub fn set_current(r: Reading) {
    let _ = CURRENT.try_with(|c| c.set(r));
}

/// Middleware: every request starts standard.
pub async fn middleware(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    CURRENT.scope(Cell::new(Reading::default()), next.run(req)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_values_round_trip_and_unknown_words_are_ignored() {
        assert_eq!(Reading::parse(""), Reading::default());
        assert_eq!(Reading::default().stored(), "");
        assert_eq!(Reading::default().classes(), "");
        let all = Reading { size: 4, wide: true, easy: true, contrast: true, calm: true };
        assert_eq!(all.stored(), "size-4 wide easy contrast calm");
        assert_eq!(all.classes(), "read-size-4 read-wide read-easy read-contrast read-calm");
        assert_eq!(Reading::parse(&all.stored()), all);
        assert_eq!(Reading::parse("size-9 size-0 sparkly wide"), Reading { wide: true, ..Reading::default() });
    }

    #[test]
    fn fields_set_and_read_back() {
        let mut r = Reading::default();
        for (field, values) in FIELDS {
            assert_eq!(r.value(field), values[0], "{field} starts standard");
            for v in *values {
                r.set(field, v).unwrap();
                assert_eq!(r.value(field), *v);
            }
        }
        assert!(r.set("read_size", "5").is_err());
        assert!(r.set("read_spacing", "huge").is_err());
        assert!(r.set("read_colour", "standard").is_err());
    }
}
