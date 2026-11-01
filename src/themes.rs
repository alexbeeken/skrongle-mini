//! Themes: app-level skins a learner wears whatever course is open. Each is
//! a folder under `data/themes/<id>/`:
//!
//! ```text
//! data/themes/<id>/
//!   theme.json             colours, font, art, strings, profile pictures (content::Theme)
//!   pictures/**/*.svg      the art and pictures theme.json refers to
//!   badges/<badge>.svg     its look of a shop badge (#569, motivators/shop.rs), optional
//! ```
//!
//! Lessons may carry material for a theme under `themes.<id>`, and a
//! manifest praise lines under its own `themes.<id>`; a course never needs
//! a theme, so a theme skins every course and only changes what is shown
//! where the course has something for it. Themes are loaded once at boot
//! and served at `/themes/<id>/<path>`.
//!
//! The three stay apart. A theme is app-level flair, so what it says (its
//! title, praise, colour names) is in the app's language and fits any course. A
//! course's language comes only from the course, themed or not. A profile
//! (`profile.rs`) knows nothing of courses: the theme only changes how the
//! learner's picture looks and what it is called.
//!
//! A learner can also own themes: the same `theme.json` + pictures, stored in
//! SQLite (`user_themes`, `user_theme_files`) and loaded through the same
//! validator. They are private to their owner, worn as `user-<n>` (a prefix
//! built-in themes may not use), and loaded on first use.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::content::{self, validate_theme, ContentError, Theme, PICTURES_DIR, PICTURE_EXT};
use crate::courses::Courses;
use crate::db::Db;
use crate::error::AppError;

pub const THEME_FILE: &str = "theme.json";

/// Ids of learner-made themes start with this; built-in themes may not.
pub const USER_PREFIX: &str = "user-";

/// Per-account limits on made themes, so a small server stays healthy
/// (`SKRONGLE_THEMES_PER_USER`, `SKRONGLE_THEME_BYTES`).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_themes: usize,
    /// Total size of all files of one account's themes.
    pub max_bytes: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_themes: 5, max_bytes: 512 * 1024 }
    }
}

/// A theme with its files.
#[derive(Debug)]
pub struct ThemePack {
    pub theme: Theme,
    /// The learner a made theme belongs to; `None` for a built-in theme.
    pub owner: Option<i64>,
    /// `pictures/<name>.svg` -> file content.
    assets: HashMap<String, String>,
}

impl ThemePack {
    /// A picture of the theme by its path, e.g. `pictures/banner.svg`.
    pub fn asset(&self, path: &str) -> Option<&str> {
        self.assets.get(path).map(String::as_str)
    }
}

/// Every theme the app has, loaded from `data/themes/`.
#[derive(Debug, Default)]
pub struct Themes {
    packs: Vec<Arc<ThemePack>>,
    /// Learner-made themes loaded so far, by `user-<n>`.
    made: Mutex<HashMap<String, Arc<ThemePack>>>,
    limits: Limits,
    /// Theming is turned off (`SKRONGLE_THEMES_ENABLED` is not `1`, #1138):
    /// no theme is offered or worn, made ones included.
    off: bool,
}

impl Themes {
    /// Load every `data/themes/*/theme.json`. No directory is fine: then the
    /// app simply offers no themes.
    pub fn load(data_dir: &Path) -> Result<Themes, ContentError> {
        let root = data_dir.join("themes");
        let mut packs = Vec::new();
        if root.is_dir() {
            let mut dirs: Vec<_> = fs::read_dir(&root)
                .map_err(|e| ContentError::Io(root.clone(), e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join(THEME_FILE).is_file())
                .collect();
            dirs.sort();
            for dir in dirs {
                packs.push(Arc::new(ThemePack::load_dir(&dir)?));
            }
        }
        Ok(Themes { packs, ..Themes::default() })
    }

    /// No themes at all, and none can be made: what the app has while
    /// theming is turned off (#1138). Nothing is read from `data/themes/`.
    pub fn off() -> Themes {
        Themes { off: true, ..Themes::default() }
    }

    pub fn is_off(&self) -> bool {
        self.off
    }

    pub fn with_limits(mut self, limits: Limits) -> Themes {
        self.limits = limits;
        self
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    #[cfg(test)]
    pub fn empty() -> Themes {
        Themes::default()
    }

    pub fn all(&self) -> &[Arc<ThemePack>] {
        &self.packs
    }

    pub fn get(&self, id: &str) -> Option<Arc<ThemePack>> {
        self.packs.iter().find(|p| p.theme.id == id).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }

    /// The theme `id` as `user_id` may wear it: a built-in one, or one they
    /// made. A made theme of anyone else is the same as one that does not exist.
    pub async fn resolve(&self, db: &Db, user_id: i64, id: &str) -> Result<Option<Arc<ThemePack>>, AppError> {
        if id.is_empty() || self.off {
            return Ok(None);
        }
        if let Some(p) = self.get(id) {
            return Ok(Some(p));
        }
        let Some(row) = parse_user_id(id) else { return Ok(None) };
        if let Some(p) = self.made_lock().get(id).cloned() {
            return Ok((p.owner == Some(user_id)).then_some(p));
        }
        let files = db.call(move |c| theme_files(c, row, user_id)).await?;
        let Some(files) = files else { return Ok(None) };
        match ThemePack::from_files(id, Some(user_id), &files) {
            Ok(pack) => {
                let pack = Arc::new(pack);
                self.made_lock().insert(id.to_string(), Arc::clone(&pack));
                Ok(Some(pack))
            }
            Err(e) => {
                tracing::warn!("theme {id} does not load: {e}");
                Ok(None)
            }
        }
    }

    /// The themes `user_id` made, oldest first (those that load).
    pub async fn made_by(&self, db: &Db, user_id: i64) -> Result<Vec<Arc<ThemePack>>, AppError> {
        if self.off {
            return Ok(Vec::new());
        }
        let rows = db.call(move |c| owned_ids(c, user_id)).await?;
        let mut out = Vec::new();
        for row in rows {
            if let Some(p) = self.resolve(db, user_id, &user_theme_id(row)).await? {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// Store a new theme for `owner` (checked first like any theme file) and
    /// return its id. Fails, naming the problem, over the account's limits.
    pub async fn create(&self, db: &Db, owner: i64, files: Vec<(String, String)>) -> Result<String, ThemeError> {
        if self.off {
            return Err(ThemeError::Off);
        }
        ThemePack::from_files(&user_theme_id(0), Some(owner), &files).map_err(|e| ThemeError::Invalid(e.to_string()))?;
        let limits = self.limits;
        let row = db.call(move |c| insert_theme(c, owner, &files, limits)).await.map_err(|e| ThemeError::Db(e.to_string()))??;
        Ok(user_theme_id(row))
    }

    /// Replace the files of a theme `owner` made.
    /// Courses' themed copies are rebuilt with the new files.
    pub async fn replace(&self, db: &Db, courses: &Courses, owner: i64, id: &str, files: Vec<(String, String)>) -> Result<(), ThemeError> {
        if self.off {
            return Err(ThemeError::Off);
        }
        let row = parse_user_id(id).ok_or(ThemeError::NotFound)?;
        ThemePack::from_files(id, Some(owner), &files).map_err(|e| ThemeError::Invalid(e.to_string()))?;
        let limits = self.limits;
        db.call(move |c| update_theme(c, owner, row, &files, limits)).await.map_err(|e| ThemeError::Db(e.to_string()))??;
        self.made_lock().remove(id);
        courses.forget_theme(id);
        Ok(())
    }

    /// Delete a theme `owner` made; if they were wearing it they are back to
    /// Default. `false` when there was no such theme of theirs.
    pub async fn delete(&self, db: &Db, courses: &Courses, owner: i64, id: &str) -> Result<bool, AppError> {
        let Some(row) = parse_user_id(id) else { return Ok(false) };
        let worn = id.to_string();
        let gone = db.call(move |c| delete_theme(c, owner, row, &worn)).await?;
        self.made_lock().remove(id);
        courses.forget_theme(id);
        Ok(gone)
    }

    /// Drop a made theme from the cache (its owner's account was deleted).
    pub fn forget(&self, id: &str) {
        self.made_lock().remove(id);
    }

    fn made_lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<ThemePack>>> {
        self.made.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Why a made theme could not be stored.
#[derive(Debug, PartialEq)]
pub enum ThemeError {
    /// The files do not make a valid theme; the message names the file and field.
    Invalid(String),
    /// Over the account's limits.
    Limit(String),
    NotFound,
    Db(String),
    /// Theming is turned off on this server.
    Off,
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeError::Invalid(m) | ThemeError::Limit(m) | ThemeError::Db(m) => f.write_str(m),
            ThemeError::NotFound => f.write_str("no such theme"),
            ThemeError::Off => f.write_str("themes are turned off (SKRONGLE_THEMES_ENABLED)"),
        }
    }
}

impl From<rusqlite::Error> for ThemeError {
    fn from(e: rusqlite::Error) -> ThemeError {
        ThemeError::Db(e.to_string())
    }
}

/// `user-<n>` for the made theme with row `n`.
pub fn user_theme_id(row: i64) -> String {
    format!("{USER_PREFIX}{row}")
}

/// The row of a `user-<n>` id; `None` for anything else.
pub fn parse_user_id(id: &str) -> Option<i64> {
    let n = id.strip_prefix(USER_PREFIX)?;
    if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

// ---------- storage ----------

/// The files of theme `row` if `owner` made it.
fn theme_files(conn: &Connection, row: i64, owner: i64) -> rusqlite::Result<Option<Vec<(String, String)>>> {
    let mine: Option<i64> = conn.query_row("SELECT id FROM user_themes WHERE id = ?1 AND owner_id = ?2", params![row, owner], |r| r.get(0)).optional()?;
    if mine.is_none() {
        return Ok(None);
    }
    let mut stmt = conn.prepare_cached("SELECT path, content FROM user_theme_files WHERE theme_id = ?1 ORDER BY path")?;
    let files = stmt.query_map([row], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(files))
}

fn owned_ids(conn: &Connection, owner: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached("SELECT id FROM user_themes WHERE owner_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([owner], |r| r.get(0))?.collect();
    rows
}

fn size_of(files: &[(String, String)]) -> usize {
    files.iter().map(|(p, c)| p.len() + c.len()).sum()
}

/// Bytes the account's other themes use (all of them but `except`).
fn bytes_used(conn: &Connection, owner: i64, except: i64) -> rusqlite::Result<usize> {
    conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(CAST(f.path AS BLOB)) + LENGTH(CAST(f.content AS BLOB))), 0)
         FROM user_theme_files f JOIN user_themes t ON t.id = f.theme_id WHERE t.owner_id = ?1 AND t.id != ?2",
        params![owner, except],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n as usize)
}

fn check_size(conn: &Connection, owner: i64, except: i64, files: &[(String, String)], limits: Limits) -> Result<(), ThemeError> {
    let total = bytes_used(conn, owner, except)? + size_of(files);
    if total > limits.max_bytes {
        return Err(ThemeError::Limit(format!(
            "your themes would take {} KB; the limit is {} KB for all of them together",
            total.div_ceil(1024),
            limits.max_bytes / 1024
        )));
    }
    Ok(())
}

fn put_files(conn: &Connection, row: i64, files: &[(String, String)]) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM user_theme_files WHERE theme_id = ?1", [row])?;
    for (path, content) in files {
        conn.execute("INSERT INTO user_theme_files (theme_id, path, content) VALUES (?1, ?2, ?3)", params![row, path, content])?;
    }
    Ok(())
}

fn title_of(files: &[(String, String)]) -> String {
    files
        .iter()
        .find(|(p, _)| p == THEME_FILE)
        .and_then(|(_, c)| serde_json::from_str::<Theme>(c).ok())
        .map(|t| t.title)
        .unwrap_or_default()
}

fn insert_theme(conn: &mut Connection, owner: i64, files: &[(String, String)], limits: Limits) -> rusqlite::Result<Result<i64, ThemeError>> {
    let tx = conn.transaction()?;
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM user_themes WHERE owner_id = ?1", [owner], |r| r.get(0))?;
    if count as usize >= limits.max_themes {
        return Ok(Err(ThemeError::Limit(format!("you can keep {} themes; delete one to make another", limits.max_themes))));
    }
    if let Err(e) = check_size(&tx, owner, 0, files, limits) {
        return Ok(Err(e));
    }
    let at = crate::db::now();
    tx.execute(
        "INSERT INTO user_themes (owner_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![owner, title_of(files), at],
    )?;
    let row = tx.last_insert_rowid();
    put_files(&tx, row, files)?;
    tx.commit()?;
    Ok(Ok(row))
}

fn update_theme(conn: &mut Connection, owner: i64, row: i64, files: &[(String, String)], limits: Limits) -> rusqlite::Result<Result<(), ThemeError>> {
    let tx = conn.transaction()?;
    let mine: Option<i64> = tx.query_row("SELECT id FROM user_themes WHERE id = ?1 AND owner_id = ?2", params![row, owner], |r| r.get(0)).optional()?;
    if mine.is_none() {
        return Ok(Err(ThemeError::NotFound));
    }
    if let Err(e) = check_size(&tx, owner, row, files, limits) {
        return Ok(Err(e));
    }
    tx.execute("UPDATE user_themes SET title = ?2, updated_at = ?3 WHERE id = ?1", params![row, title_of(files), crate::db::now()])?;
    put_files(&tx, row, files)?;
    tx.commit()?;
    Ok(Ok(()))
}

fn delete_theme(conn: &mut Connection, owner: i64, row: i64, worn: &str) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    let n = tx.execute("DELETE FROM user_themes WHERE id = ?1 AND owner_id = ?2", params![row, owner])?;
    if n > 0 {
        // Courses that wore it wear Default.
        tx.execute("UPDATE projects SET theme = '' WHERE user_id = ?1 AND theme = ?2", params![owner, worn])?;
    }
    tx.commit()?;
    Ok(n > 0)
}

impl ThemePack {
    fn load_dir(dir: &Path) -> Result<ThemePack, ContentError> {
        let file = dir.join(THEME_FILE);
        let label = file.display().to_string();
        let src = fs::read_to_string(&file).map_err(|e| ContentError::Io(file.clone(), e))?;
        let theme: Theme = serde_json::from_str(&src).map_err(|e| ContentError::Parse(label.clone(), e))?;
        validate_theme(&theme).map_err(|m| ContentError::Invalid(format!("{label}: {m}")))?;
        let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if theme.id != folder {
            return Err(ContentError::Invalid(format!("{label}: theme id '{}' must match its folder name '{folder}'", theme.id)));
        }
        let mut assets = HashMap::new();
        let pictures = dir.join(PICTURES_DIR.trim_end_matches('/'));
        let mut paths = Vec::new();
        collect_svgs(&pictures, &mut paths)?;
        for p in paths {
            let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            assets.insert(rel, fs::read_to_string(&p).map_err(|e| ContentError::Io(p.clone(), e))?);
        }
        check_art(&label, &theme, &assets, &dir.display().to_string())?;
        // Shop badge variants (#569): `badges/<badge>.svg`.
        let mut theme = theme;
        let mut badges = Vec::new();
        collect_svgs(&dir.join("badges"), &mut badges)?;
        for p in badges {
            let Some(id) = p.file_stem().and_then(|s| s.to_str()).filter(|s| content::is_slug(s)) else {
                return Err(ContentError::Invalid(format!("{label}: badge file '{}' must be named badges/<badge id>.svg", p.display())));
            };
            assets.insert(format!("badges/{id}.svg"), fs::read_to_string(&p).map_err(|e| ContentError::Io(p.clone(), e))?);
            theme.badges.insert(id.to_string());
        }
        if theme.id.starts_with(USER_PREFIX) {
            return Err(ContentError::Invalid(format!("{label}: theme ids starting with '{USER_PREFIX}' are for themes learners make")));
        }
        Ok(ThemePack { theme, owner: None, assets })
    }

    /// A theme made by `owner` from its stored files (`theme.json`,
    /// `pictures/*.svg`). Its id is the one it is worn by, whatever the file says.
    pub fn from_files(id: &str, owner: Option<i64>, files: &[(String, String)]) -> Result<ThemePack, ContentError> {
        let label = format!("theme {id}");
        let Some((_, src)) = files.iter().find(|(p, _)| p == THEME_FILE) else {
            return Err(ContentError::Invalid(format!("{label}: missing {THEME_FILE}")));
        };
        // The worn id goes in before parsing, so a file without one is fine.
        let parse = |e| ContentError::Parse(format!("{label}: {THEME_FILE}"), e);
        let mut json: serde_json::Value = serde_json::from_str(src).map_err(parse)?;
        if let Some(obj) = json.as_object_mut() {
            obj.insert("id".into(), id.into());
        }
        let theme: Theme = serde_json::from_value(json).map_err(parse)?;
        validate_theme(&theme).map_err(|m| ContentError::Invalid(format!("{label}: {THEME_FILE}: {m}")))?;
        let mut assets = HashMap::new();
        for (path, content) in files.iter().filter(|(p, _)| p != THEME_FILE) {
            if !content::is_picture_path(path) {
                return Err(ContentError::Invalid(format!("{label}: file '{path}' must be {THEME_FILE} or {PICTURES_DIR}<name>{PICTURE_EXT}")));
            }
            assets.insert(path.clone(), content.clone());
        }
        check_art(&label, &theme, &assets, "the theme")?;
        Ok(ThemePack { theme, owner, assets })
    }
}

/// Every picture the theme file names is one of its files.
fn check_art(label: &str, theme: &Theme, assets: &HashMap<String, String>, where_: &str) -> Result<(), ContentError> {
    {
        let art = [("banner".to_string(), &theme.art.banner), ("mark".to_string(), &theme.art.mark)];
        let avatars = theme.avatars.iter().map(|a| (format!("avatar '{}'", a.id), &a.image));
        let backgrounds = theme.avatar_backgrounds.iter().map(|b| (format!("avatar_backgrounds slot {}", b.slot), &b.image));
        for (what, path) in art.into_iter().chain(avatars).chain(backgrounds) {
            if !path.is_empty() && !content::is_url(path) && !assets.contains_key(path) {
                return Err(ContentError::Invalid(format!(
                    "{label}: {what} '{path}' is not a file of the theme (expected {PICTURES_DIR}<name>{PICTURE_EXT} under {where_})"
                )));
            }
        }
    }
    Ok(())
}

fn collect_svgs(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), ContentError> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| ContentError::Io(dir.to_path_buf(), e))? {
        let path = entry.map_err(|e| ContentError::Io(dir.to_path_buf(), e))?.path();
        if path.is_dir() {
            collect_svgs(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some(PICTURE_EXT.trim_start_matches('.')) {
            out.push(path);
        }
    }
    Ok(())
}

// ---------- what a theme says, in the learner's language (#463) ----------

/// A built-in theme's texts are app text: `themes.<id>.<key>` in every
/// `i18n/<code>.yaml` (`title`, `description`, `praise_<n>`, `almost_<n>`,
/// `slot_<n>` for the colour and background names). Each read prefers the
/// key in the request's language and falls back to `theme.json`, which is
/// all a theme a learner made has. Picture names are names and stay as
/// written.
impl Theme {
    fn said(&self, key: &str, fallback: &str) -> String {
        crate::i18n::lookup(&format!("themes.{}.{key}", self.id)).unwrap_or(fallback).to_string()
    }

    pub fn title_said(&self) -> String {
        self.said("title", &self.title)
    }

    pub fn description_said(&self) -> String {
        self.said("description", &self.description)
    }

    pub fn praise_said(&self) -> Vec<String> {
        self.strings.praise.iter().zip(1..).map(|(s, n)| self.said(&format!("praise_{n}"), s)).collect()
    }

    pub fn almost_said(&self) -> Vec<String> {
        self.strings.almost.iter().zip(1..).map(|(s, n)| self.said(&format!("almost_{n}"), s)).collect()
    }

    /// The name of palette slot `slot` (a colour 1–8 or a background 9–16).
    pub fn slot_said(&self, slot: u8, fallback: &str) -> String {
        self.said(&format!("slot_{slot}"), fallback)
    }
}

impl content::ContentStore {
    /// The praise lines a page shows: the theme's, in the learner's
    /// language, when the worn theme supplied them (the course has none of
    /// its own for it); the manifest's otherwise.
    pub fn praise(&self) -> Vec<String> {
        match self.theme_lines(|s| &s.praise) {
            Some(t) => t.praise_said(),
            None => self.manifest().strings.praise.clone(),
        }
    }

    /// `praise`, for a wrong answer.
    pub fn almost(&self) -> Vec<String> {
        match self.theme_lines(|s| &s.almost) {
            Some(t) => t.almost_said(),
            None => self.manifest().strings.almost.clone(),
        }
    }

    /// The worn theme, when its lines (picked by `pick`) are the ones in use.
    fn theme_lines(&self, pick: impl Fn(&content::ThemeStrings) -> &Vec<String>) -> Option<&Theme> {
        let t = self.theme()?;
        let course_has = self.manifest().themes.get(&t.id).is_some_and(|c| !pick(&c.strings).is_empty());
        (!course_has && !pick(&t.strings).is_empty()).then_some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn themed_avatar(themes: &Themes, theme: &str, avatar: &str) -> Option<content::ThemeAvatar> {
        themes.get(theme)?.theme.avatar(avatar).cloned()
    }

    #[test]
    fn loads_the_builtin_themes() {
        let themes = Themes::load(Path::new("data")).expect("themes load");
        assert!(!themes.is_empty());
        let realm = themes.get("realm").expect("realm theme");
        assert_eq!(realm.theme.title, "The Realm of Eldmark");
        assert!(realm.asset(&realm.theme.art.banner).is_some() && realm.asset(&realm.theme.art.mark).is_some());
        // One themed version of every built-in picture, each a file of the theme.
        assert_eq!(realm.theme.avatars.len(), crate::profile::AVATARS.len());
        assert!(crate::profile::AVATARS.iter().all(|b| realm.theme.avatar(b.id).is_some()));
        assert!(realm.theme.avatars.iter().all(|a| realm.asset(&a.image).is_some()));
        assert_eq!(themed_avatar(&themes, "realm", "sofia").unwrap().label, "Nym");
        assert_eq!(themed_avatar(&themes, "realm", "leaf").unwrap().label, "Nym"); // the old id of the slot
        assert!(themed_avatar(&themes, "", "sofia").is_none());
        assert!(themed_avatar(&themes, "nope", "sofia").is_none());
        // Meadowbrook uses every option of a theme file.
        let sd = themes.get("meadowbrook").expect("meadowbrook theme");
        let t = &sd.theme;
        assert!(!t.colors.is_empty() && !t.dark.is_empty() && !t.font.is_empty());
        assert!(sd.asset(&t.art.banner).is_some() && sd.asset(&t.art.mark).is_some());
        assert!(!t.strings.praise.is_empty() && !t.strings.almost.is_empty());
        assert_eq!(t.avatars.len(), crate::profile::AVATARS.len());
        assert!(t.icons.len() >= 15 && !t.lesson_icons.is_empty());
        assert_eq!(t.icons().mode_sprint, "🎣");
        // Rustwake: dark terminal colours, a wasteland of profile pictures, every slot.
        let fo = themes.get("rustwake").expect("rustwake theme");
        let t = &fo.theme;
        assert!(!t.colors.is_empty() && !t.dark.is_empty() && t.font.contains("monospace"));
        assert!(fo.asset(&t.art.banner).is_some() && fo.asset(&t.art.mark).is_some());
        assert!(!t.strings.praise.is_empty() && !t.strings.almost.is_empty());
        assert_eq!(t.avatars.len(), crate::profile::AVATARS.len());
        assert!(t.avatars.iter().all(|a| fo.asset(&a.image).is_some()));
        assert_eq!(themed_avatar(&themes, "rustwake", "noor").unwrap().label, "Ash");
        assert_eq!(t.icons.len(), 19);
        assert!(t.lesson_icons.len() >= 12);
        assert_eq!(t.icons().mode_sprint, "📱");
        assert_eq!(themes.all().len(), 3);
        assert!(realm.asset("pictures/nope.svg").is_none());
        assert!(themes.get("nope").is_none());
        assert!(realm.theme.css().contains("--accent:"));
        assert!(Themes::load(Path::new("/nonexistent")).unwrap().is_empty());
    }

    /// A theme belongs to the app, not to a course, so what it says is in
    /// English (the app's language): picture names, praise, title. Words in
    /// a course's language belong to the course (its `themes.<id>` blocks).
    #[test]
    fn builtin_themes_speak_english() {
        let themes = Themes::load(Path::new("data")).expect("themes load");
        let spanish = |s: &str| s.chars().any(|c| "¡¿áéíóúñüÁÉÍÓÚÑÜ".contains(c));
        for pack in themes.all() {
            let t = &pack.theme;
            let said = [&t.title, &t.description]
                .into_iter()
                .chain(&t.strings.praise)
                .chain(&t.strings.almost)
                .chain(t.avatars.iter().map(|a| &a.label))
                .chain(t.avatar_colors.iter().map(|c| &c.label));
            for s in said {
                assert!(!spanish(s), "theme '{}' says {s:?}: theme text is English; put course-language lines in the course's themes.{}", t.id, t.id);
            }
        }
    }

    /// The colour a learner picks goes behind their picture (#227): every
    /// built-in theme names its eight colours, gives each of its pictures
    /// the colour it wears when none is picked, and draws them on a
    /// transparent background (no full-size badge disc) so the colour shows.
    #[test]
    fn builtin_themes_colour_the_pictures() {
        let themes = Themes::load(Path::new("data")).expect("themes load");
        for pack in themes.all() {
            let t = &pack.theme;
            let slots: Vec<u8> = t.avatar_colors.iter().map(|c| c.slot).collect();
            assert_eq!(slots, (1..=8).collect::<Vec<u8>>(), "theme '{}' gives a colour for each of the 8 slots, in order", t.id);
            for a in &t.avatars {
                assert!(content::is_hex_color(&a.color), "theme '{}': picture '{}' needs a color", t.id, a.id);
                let svg = pack.asset(&a.image).unwrap();
                assert!(!svg.contains(r##"r="32" fill="#"##), "theme '{}': {} paints its own badge over the colour", t.id, a.image);
            }
            // The palette worn is the theme's; a picture's own colour (its tile) is its version's.
            let p = crate::profile::palette(Some(t));
            assert!(p.iter().zip(&t.avatar_colors).all(|(s, c)| s.color == c.color && s.label == c.label));
            assert_eq!(crate::profile::own_color("kai", Some(t)), t.avatar("kai").unwrap().color);
            assert_eq!(crate::profile::badge_color(0, Some(t)), t.avatar_colors[0].color, "none chosen shows the theme's first slot");
        }
    }

    /// Each built-in theme adds scenes of its world behind the picture (#248):
    /// files of the theme, small, square, with no raster images or scripts.
    #[test]
    fn builtin_themes_add_backgrounds() {
        let themes = Themes::load(Path::new("data")).expect("themes load");
        for pack in themes.all() {
            let t = &pack.theme;
            assert!(t.avatar_backgrounds.len() >= 4, "theme '{}' adds at least four backgrounds", t.id);
            for b in &t.avatar_backgrounds {
                let svg = pack.asset(&b.image).unwrap_or_else(|| panic!("theme '{}': {} is a file of the theme", t.id, b.image));
                assert!(svg.len() < 4096, "theme '{}': {} is {} bytes; keep a background under 4 KB", t.id, b.image, svg.len());
                assert!(svg.contains(r#"viewBox="0 0 64 64""#), "theme '{}': {} is drawn on a 64 x 64 square", t.id, b.image);
                assert!(!svg.contains("<image") && !svg.contains("<script"), "theme '{}': {} draws only shapes", t.id, b.image);
            }
            let p = crate::profile::palette(Some(t));
            assert_eq!(p.len(), 8 + t.avatar_backgrounds.len());
            assert!(p[8..].iter().all(|s| s.image.contains(&format!("/themes/{}/pictures/", t.id))));
        }
        // The default patterns, embedded in the app, are small too.
        for c in crate::profile::AVATAR_COLORS {
            let file = std::fs::read_to_string(format!("static/avatar-bg/{}.svg", c.pattern)).expect("pattern file");
            assert!(file.len() < 2048 && file.contains(r#"viewBox="0 0 64 64""#), "{}", c.pattern);
        }
    }

    /// Every key a theme file may have is documented in docs/themes.md.
    #[test]
    fn every_theme_key_is_documented() {
        let doc = include_str!("../docs/themes.md");
        let keys = serde_json::to_value(content::Theme::default()).unwrap();
        for k in keys.as_object().unwrap().keys() {
            assert!(doc.contains(&format!("| `{k}")), "docs/themes.md has no row for the theme key `{k}`");
        }
        assert!(doc.contains("`color` (optional)"), "docs/themes.md documents a picture's color");
    }

    #[test]
    fn a_theme_folder_is_checked() {
        let dir = std::env::temp_dir().join(format!("themes-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("themes/night/pictures")).unwrap();
        let file = dir.join("themes/night").join(THEME_FILE);
        fs::write(&file, r##"{"id": "nite", "title": "Night"}"##).unwrap();
        let e = Themes::load(&dir).unwrap_err().to_string();
        assert!(e.contains("must match its folder name 'night'"), "{e}");
        fs::write(&file, r##"{"id": "night", "title": "Night", "art": {"banner": "pictures/b.svg"}}"##).unwrap();
        let e = Themes::load(&dir).unwrap_err().to_string();
        assert!(e.contains("banner 'pictures/b.svg' is not a file of the theme"), "{e}");
        fs::write(dir.join("themes/night/pictures/b.svg"), "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();
        let t = Themes::load(&dir).unwrap();
        assert_eq!(t.all().len(), 1);
        assert!(t.get("night").unwrap().asset("pictures/b.svg").unwrap().starts_with("<svg"));
        let _ = fs::remove_dir_all(&dir);
    }

    // ---------- themes learners make ----------

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    fn night() -> Vec<(String, String)> {
        vec![
            (THEME_FILE.to_string(), r##"{"id": "whatever", "title": "Night", "colors": {"accent": "#334455"}, "art": {"banner": "pictures/b.svg"}}"##.to_string()),
            ("pictures/b.svg".to_string(), "<svg xmlns='http://www.w3.org/2000/svg'/>".to_string()),
        ]
    }

    fn two_users(db: &Db) -> (i64, i64) {
        db.with(|c| Ok((crate::auth::create_user(c, "ann@example.org", "x")?, crate::auth::create_user(c, "bob@example.org", "x")?))).unwrap()
    }

    #[test]
    fn user_theme_ids_are_numbers_after_a_prefix() {
        assert_eq!(user_theme_id(7), "user-7");
        assert_eq!(parse_user_id("user-7"), Some(7));
        for bad in ["", "wow", "user-", "user-0", "user-07", "user--1", "user-1x", "User-1"] {
            assert_eq!(parse_user_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_made_theme_is_checked_like_a_built_in_one() {
        let pack = ThemePack::from_files("user-3", Some(1), &night()).unwrap();
        // The worn id wins over the id in the file.
        assert_eq!(pack.theme.id, "user-3");
        assert!(pack.theme.css().contains("--accent:"));
        assert!(pack.asset("pictures/b.svg").is_some());
        let mut f = night();
        f[0].1 = r##"{"id": "x", "title": "N", "colors": {"accent": "red"}}"##.into();
        let e = ThemePack::from_files("user-3", Some(1), &f).unwrap_err().to_string();
        assert!(e.contains("theme.json") && e.contains("accent"), "{e}");
        // No id in the file at all: fine, it is worn by its own.
        let no_id = [(THEME_FILE.to_string(), r#"{"title": "N"}"#.to_string())];
        assert_eq!(ThemePack::from_files("user-3", Some(1), &no_id).unwrap().theme.id, "user-3");
        let e = ThemePack::from_files("user-3", Some(1), &night()[1..]).unwrap_err().to_string();
        assert!(e.contains("missing theme.json"), "{e}");
        let mut f = night();
        f.push(("evil.html".into(), "<script>".into()));
        let e = ThemePack::from_files("user-3", Some(1), &f).unwrap_err().to_string();
        assert!(e.contains("'evil.html'"), "{e}");
        let e = ThemePack::from_files("user-3", Some(1), &night()[..1]).unwrap_err().to_string();
        assert!(e.contains("banner 'pictures/b.svg' is not a file of the theme"), "{e}");
    }

    #[test]
    fn built_in_themes_cannot_take_the_made_prefix() {
        let dir = std::env::temp_dir().join(format!("themes-prefix-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("themes/user-1")).unwrap();
        fs::write(dir.join("themes/user-1").join(THEME_FILE), r#"{"id": "user-1", "title": "Sneaky"}"#).unwrap();
        let e = Themes::load(&dir).unwrap_err().to_string();
        assert!(e.contains("are for themes learners make"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_made_theme_belongs_to_its_owner_and_survives_a_restart() {
        let path = std::env::temp_dir().join(format!("themes-db-{}.db", std::process::id()));
        let _ = fs::remove_file(&path);
        let db = Db::open(&path).unwrap();
        let (ann, bob) = two_users(&db);
        let themes = Themes::default();
        let id = run(themes.create(&db, ann, night())).unwrap();
        assert_eq!(id, "user-1");
        // Ann can wear it; Bob gets the same answer as for a theme that does not exist.
        let mine = run(themes.resolve(&db, ann, &id)).unwrap().unwrap();
        assert_eq!((mine.theme.title.as_str(), mine.owner), ("Night", Some(ann)));
        assert!(run(themes.resolve(&db, bob, &id)).unwrap().is_none());
        assert!(run(themes.resolve(&db, bob, "user-99")).unwrap().is_none());
        // Still private once cached.
        assert!(run(themes.resolve(&db, bob, &id)).unwrap().is_none());
        assert_eq!(run(themes.made_by(&db, ann)).unwrap().len(), 1);
        assert!(run(themes.made_by(&db, bob)).unwrap().is_empty());
        // Bob cannot replace or delete it.
        assert_eq!(run(themes.replace(&db, &Courses::empty(), bob, &id, night())), Err(ThemeError::NotFound));
        assert!(!run(themes.delete(&db, &Courses::empty(), bob, &id)).unwrap());
        drop(db);
        // A new process: nothing cached, same answers.
        let db = Db::open(&path).unwrap();
        let themes = Themes::default();
        assert!(run(themes.resolve(&db, ann, &id)).unwrap().is_some());
        assert!(run(themes.resolve(&db, bob, &id)).unwrap().is_none());
        // Edits show at once.
        let mut f = night();
        f[0].1 = r##"{"id": "x", "title": "Day"}"##.into();
        f.truncate(1);
        run(themes.replace(&db, &Courses::empty(), ann, &id, f)).unwrap();
        assert_eq!(run(themes.resolve(&db, ann, &id)).unwrap().unwrap().theme.title, "Day");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn deleting_the_theme_a_course_wears_puts_it_back_to_plain() {
        let db = Db::open_in_memory().unwrap();
        let (ann, bob) = two_users(&db);
        let themes = Themes::default();
        let id = run(themes.create(&db, ann, night())).unwrap();
        let wear = |u, t: &str| {
            db.with(|c| {
                crate::courses::open_project(c, u, "spanish")?;
                let change = crate::courses::SettingsChange { theme: Some(t.to_string()), ..Default::default() };
                crate::courses::set_project_settings(c, u, "spanish", &change)
            })
            .unwrap()
        };
        wear(ann, &id);
        wear(bob, "realm");
        assert!(run(themes.delete(&db, &Courses::empty(), ann, &id)).unwrap());
        let worn = |u| db.with(|c| c.query_row("SELECT theme FROM projects WHERE user_id = ?1", [u], |r| r.get::<_, String>(0))).unwrap();
        assert_eq!(worn(ann), "");
        assert_eq!(worn(bob), "realm");
        assert!(run(themes.resolve(&db, ann, &id)).unwrap().is_none());
        let files: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM user_theme_files", [], |r| r.get(0))).unwrap();
        assert_eq!(files, 0);
        assert!(!run(themes.delete(&db, &Courses::empty(), ann, &id)).unwrap());
        // A deleted theme's id is never given to another one.
        assert_eq!(run(themes.create(&db, bob, night())).unwrap(), "user-2");
    }

    /// Themes turned off (#1138): a theme someone made before is not worn
    /// or listed, a new one can't be made, and its files stay stored.
    #[test]
    fn themes_off_refuses_made_themes_too() {
        let db = Db::open_in_memory().unwrap();
        let (ann, _) = two_users(&db);
        let id = run(Themes::default().create(&db, ann, night())).unwrap();
        let themes = Themes::off();
        assert!(themes.is_off() && themes.is_empty());
        assert!(run(themes.resolve(&db, ann, &id)).unwrap().is_none());
        assert!(run(themes.made_by(&db, ann)).unwrap().is_empty());
        assert_eq!(run(themes.create(&db, ann, night())).unwrap_err(), ThemeError::Off);
        assert_eq!(run(Themes::default().resolve(&db, ann, &id)).unwrap().unwrap().theme.id, id);
    }

    #[test]
    fn made_themes_have_limits_per_account() {
        let db = Db::open_in_memory().unwrap();
        let (ann, bob) = two_users(&db);
        let themes = Themes::default().with_limits(Limits { max_themes: 2, max_bytes: 600 });
        let small = || vec![(THEME_FILE.to_string(), r#"{"id": "a", "title": "A"}"#.to_string())];
        run(themes.create(&db, ann, small())).unwrap();
        run(themes.create(&db, ann, small())).unwrap();
        let e = run(themes.create(&db, ann, small())).unwrap_err();
        assert!(matches!(&e, ThemeError::Limit(m) if m.contains("2 themes")), "{e:?}");
        // The limit is per account.
        run(themes.create(&db, bob, small())).unwrap();
        // Total size across all of an account's themes.
        let big = vec![(THEME_FILE.to_string(), format!(r#"{{"id": "a", "title": "{}"}}"#, "x".repeat(700)))];
        let e = run(themes.replace(&db, &Courses::empty(), ann, "user-1", big)).unwrap_err();
        assert!(matches!(&e, ThemeError::Limit(m) if m.contains("limit is 0 KB")), "{e:?}");
        // A theme that is invalid is refused before anything is stored.
        let e = run(themes.create(&db, bob, vec![])).unwrap_err();
        assert!(matches!(e, ThemeError::Invalid(_)), "{e:?}");
        assert_eq!(run(themes.made_by(&db, bob)).unwrap().len(), 1);
    }

    /// en.yaml's `themes.<id>.*` says exactly what theme.json says, so the
    /// file stays the theme's source and every language has each line (#463).
    #[test]
    fn builtin_theme_text_is_app_text() {
        let themes = Themes::load(Path::new("data")).expect("themes load");
        for pack in themes.all() {
            let t = &pack.theme;
            let key = |k: &str| format!("themes.{}.{k}", t.id);
            let en = |k: &str| crate::i18n::in_lang("en", || crate::i18n::lookup(&key(k)).map(str::to_string));
            assert_eq!(en("title").as_deref(), Some(t.title.as_str()), "{}", key("title"));
            assert_eq!(en("description").as_deref(), Some(t.description.as_str()), "{}", key("description"));
            for (n, s) in t.strings.praise.iter().enumerate() {
                assert_eq!(en(&format!("praise_{}", n + 1)).as_deref(), Some(s.as_str()), "{} praise {n}", t.id);
            }
            for (n, s) in t.strings.almost.iter().enumerate() {
                assert_eq!(en(&format!("almost_{}", n + 1)).as_deref(), Some(s.as_str()), "{} almost {n}", t.id);
            }
            let slots = t.avatar_colors.iter().map(|c| (c.slot, &c.label)).chain(t.avatar_backgrounds.iter().map(|b| (b.slot, &b.label)));
            for (slot, label) in slots {
                assert_eq!(en(&format!("slot_{slot}")).as_deref(), Some(label.as_str()), "{} slot {slot}", t.id);
            }
        }
    }

    /// A theme speaks the learner's language; a theme a learner made has no
    /// keys and says what its file says.
    #[test]
    fn theme_text_follows_the_app_language() {
        let themes = Themes::load(Path::new("data")).expect("themes load");
        let realm = &themes.get("realm").unwrap().theme;
        crate::i18n::in_lang("es", || {
            assert_eq!(realm.title_said(), "El reino de Eldmark");
            assert_eq!(realm.praise_said()[0], "¡Victoria!");
            assert_eq!(realm.praise_said().len(), realm.strings.praise.len());
            let palette = crate::profile::palette(Some(realm));
            assert_eq!(palette.iter().find(|s| s.slot == 1).unwrap().label, "Arcano");
            assert_eq!(palette.iter().find(|s| s.slot == 12).unwrap().label, "Auroras boreales");
        });
        assert_eq!(crate::i18n::in_lang("en", || realm.title_said()), "The Realm of Eldmark");
        let mine = ThemePack::from_files("user-1", None, &[(THEME_FILE.to_string(), r#"{"id": "user-1", "title": "Night", "strings": {"praise": ["Yes!"]}}"#.to_string())]).unwrap();
        crate::i18n::in_lang("es", || {
            assert_eq!(mine.theme.title_said(), "Night");
            assert_eq!(mine.theme.praise_said(), ["Yes!"]);
        });
    }

    /// A course's own lines for the theme win and stay as written; without
    /// them a page gets the theme's, in the learner's language.
    #[test]
    fn course_praise_wins_over_the_theme_text() {
        let themes = Themes::load(Path::new("data")).expect("themes load");
        let realm = themes.get("realm").unwrap();
        let courses = Courses::load_builtin(Path::new("data")).expect("courses load");
        let mut files = courses.builtin_files("spanish").unwrap().unwrap();
        let own = content::ContentStore::from_files_themed(&files, Some(&realm.theme)).unwrap();
        let course_lines = own.manifest().themes["realm"].strings.praise.clone();
        assert_eq!(crate::i18n::in_lang("fr", || own.praise()), course_lines);
        let manifest = files.iter_mut().find(|f| f.path == content::MANIFEST_FILE).unwrap();
        let mut json: serde_json::Value = serde_json::from_str(&manifest.content).unwrap();
        json.as_object_mut().unwrap().remove("themes");
        manifest.content = json.to_string();
        let bare = content::ContentStore::from_files_themed(&files, Some(&realm.theme)).unwrap();
        assert_eq!(crate::i18n::in_lang("fr", || bare.praise())[0], "Victoire !");
        assert_eq!(crate::i18n::in_lang("fr", || bare.almost())[0], "Pas tout à fait, aventurier");
        let plain = content::ContentStore::from_files(&files).unwrap();
        assert_eq!(plain.praise(), plain.manifest().strings.praise);
    }
}
