//! The Skrongle Cash shop (#569): badges a learner buys with a course's SKC
//! and shows on that course's SKC hero. Looks only: nothing bought touches
//! passes, the streak, effort points or lessons.
//!
//! ```text
//! data/badges/catalogue.json     [{"id": "sprout", "price": 50}, ...]
//! data/badges/<id>.svg           the art
//! ```
//!
//! A badge is bought once, per course, and kept whichever motivator is worn
//! (`motivator_purchases`). A badge's title is app text: `motivator.skc.badge.<id>` in every
//! `i18n/<code>.yaml`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use rusqlite::{params, Connection};
use serde::Deserialize;

use crate::content::{ContentError, ContentStore};

pub const CATALOGUE_FILE: &str = "catalogue.json";

/// One badge of the catalogue.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Badge {
    pub id: String,
    pub price: i64,
}

impl Badge {
    /// Its title in the app's language.
    pub fn title(&self) -> String {
        title(&self.id)
    }
}

/// A badge's title in the app's language.
pub fn title(id: &str) -> String {
    crate::t(&format!("motivator.skc.badge.{id}"))
}

/// The catalogue and the plain art, loaded once at boot.
#[derive(Debug, Default)]
pub struct Catalogue {
    pub badges: Vec<Badge>,
    art: HashMap<String, String>,
}

impl Catalogue {
    /// Read `<data_dir>/badges/`: the catalogue, and a plain picture for
    /// every badge in it. No folder is an empty shop.
    pub fn read(data_dir: &Path) -> Result<Catalogue, ContentError> {
        let dir = data_dir.join("badges");
        let file = dir.join(CATALOGUE_FILE);
        if !file.is_file() {
            return Ok(Catalogue::default());
        }
        let label = file.display().to_string();
        let src = std::fs::read_to_string(&file).map_err(|e| ContentError::Io(file.clone(), e))?;
        let badges: Vec<Badge> = serde_json::from_str(&src).map_err(|e| ContentError::Parse(label.clone(), e))?;
        let mut art = HashMap::new();
        for b in &badges {
            if !crate::content::is_slug(&b.id) {
                return Err(ContentError::Invalid(format!("{label}: badge id '{}' must be lowercase a-z, 0-9 and '-'", b.id)));
            }
            if b.price <= 0 {
                return Err(ContentError::Invalid(format!("{label}: badge '{}': price must be above 0", b.id)));
            }
            if art.contains_key(&b.id) {
                return Err(ContentError::Duplicate(format!("badge id '{}' in {label}", b.id)));
            }
            let svg = dir.join(format!("{}.svg", b.id));
            let body = std::fs::read_to_string(&svg).map_err(|e| ContentError::Io(svg.clone(), e))?;
            art.insert(b.id.clone(), body);
        }
        Ok(Catalogue { badges, art })
    }

    pub fn get(&self, id: &str) -> Option<&Badge> {
        self.badges.iter().find(|b| b.id == id)
    }

    /// The plain art of a badge.
    pub fn art(&self, id: &str) -> Option<&str> {
        self.art.get(id).map(String::as_str)
    }
}

static CATALOGUE: OnceLock<Catalogue> = OnceLock::new();

/// Load the catalogue from the data folder (at boot, before serving).
pub fn init(data_dir: &Path) -> Result<&'static Catalogue, ContentError> {
    let c = Catalogue::read(data_dir)?;
    Ok(CATALOGUE.get_or_init(|| c))
}

/// The catalogue (the repository's `data/` when [`init`] never ran: tests).
pub fn catalogue() -> &'static Catalogue {
    CATALOGUE.get_or_init(|| {
        let dir = if cfg!(test) { Path::new(env!("CARGO_MANIFEST_DIR")).join("data") } else { Path::new(crate::DEFAULT_DATA_DIR).to_path_buf() };
        Catalogue::read(&dir).unwrap_or_default()
    })
}

/// Where a badge's picture is served from.
pub fn art_url(id: &str) -> String {
    format!("{}/badges/{id}.svg", crate::base_path())
}

/// A badge ready to draw: on the hero's shelf and in the shop.
#[derive(Debug, Clone)]
pub struct BadgeView {
    pub id: String,
    pub title: String,
    pub price: i64,
    pub src: String,
    pub owned: bool,
}

impl BadgeView {
    pub fn of(b: &Badge, owned: bool) -> BadgeView {
        BadgeView { id: b.id.clone(), title: b.title(), price: b.price, src: art_url(&b.id), owned }
    }
}

/// The badges the learner owns in the course, in catalogue order (a badge
/// no longer in the catalogue is left out but stays bought).
pub fn owned(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT item FROM motivator_purchases WHERE user_id = ?1 AND course_id = ?2")?;
    let bought: Vec<String> = stmt.query_map(params![user_id, course_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(catalogue().badges.iter().filter(|b| bought.contains(&b.id)).map(|b| b.id.clone()).collect())
}

/// SKC spent in the course.
pub fn spent(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COALESCE(SUM(price), 0) FROM motivator_purchases WHERE user_id = ?1 AND course_id = ?2", params![user_id, course_id], |r| r.get(0))
}

/// Why a buy was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No such badge in the catalogue.
    Unknown,
    /// Already on the shelf.
    Owned,
    /// The balance is short of the price by this much.
    Short(i64),
    /// The course does not wear Skrongle Cash.
    NotWorn,
}

impl Refusal {
    /// The query value the shop page reads back (`?refused=`).
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::Unknown => "unknown",
            Refusal::Owned => "owned",
            Refusal::Short(_) => "short",
            Refusal::NotWorn => "not_worn",
        }
    }
}

/// Buy badge `item` in the course (inside `Db::call`): the price and the
/// balance are checked here, on the server. Ok(balance after) or the refusal.
pub fn buy(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64, item: &str) -> rusqlite::Result<Result<i64, Refusal>> {
    if super::worn(conn, user_id, course.id())?.id() != "skc" {
        return Ok(Err(Refusal::NotWorn));
    }
    let Some(badge) = catalogue().get(item) else { return Ok(Err(Refusal::Unknown)) };
    if owned(conn, user_id, course.id())?.iter().any(|b| b == item) {
        return Ok(Err(Refusal::Owned));
    }
    let balance = super::skc::balance(conn, user_id, course, tz)?;
    if balance < badge.price {
        return Ok(Err(Refusal::Short(badge.price - balance)));
    }
    let n = conn.execute(
        "INSERT OR IGNORE INTO motivator_purchases (user_id, course_id, item, price, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, course.id(), item, badge.price, crate::db::now()],
    )?;
    if n == 0 {
        return Ok(Err(Refusal::Owned));
    }
    Ok(Ok(balance - badge.price))
}

#[cfg(test)]
mod tests;
