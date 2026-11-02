//! User profiles: the picture (one of a fixed set of named people and
//! animals), personal information and the account settings behind the
//! top-right menu.
//!
//! A profile never depends on a course: nothing here reads the active
//! course, and picture names are English (the app's language) like the
//! labels below. The worn theme may show its own version of the picture,
//! named in English too; practice in a course's language comes from the
//! course, not the profile (docs/themes.md, "Themes, courses and profiles").
//!
//! The e-mail address is set once (at registration, by Google sign-in, or on
//! the profile page for accounts that predate it) and never changes after
//! that; everything else here can be edited at any time.

use rusqlite::{params, Connection, OptionalExtension};

/// One of the pictures a learner can pick. `id` is what `users.avatar`
/// stores and what the SVG sprite in `templates/avatars.html` is keyed by;
/// `label` is the picture's name (every picture is a person or an animal
/// with a name of their own, not a thing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Avatar {
    pub id: &'static str,
    pub label: &'static str,
    /// Who or what the picture shows, for authors drawing a theme's version.
    pub shows: &'static str,
    /// Background colour of the badge behind the portrait.
    pub color: &'static str,
}

impl Avatar {
    /// `shows`, in the learner's language: the tooltip of the built-in picture.
    pub fn description(&self) -> String {
        match self.id {
            "noor" => crate::t("profile.avatar.shows.noor"),
            "amani" => crate::t("profile.avatar.shows.amani"),
            "yuki" => crate::t("profile.avatar.shows.yuki"),
            "kai" => crate::t("profile.avatar.shows.kai"),
            "arjun" => crate::t("profile.avatar.shows.arjun"),
            "sofia" => crate::t("profile.avatar.shows.sofia"),
            "mochi" => crate::t("profile.avatar.shows.mochi"),
            "coco" => crate::t("profile.avatar.shows.coco"),
            _ => self.shows.to_string(),
        }
    }
}

/// The eight pictures on offer, in the order the profile page shows them:
/// six people from around the world and two pets, with short names most of
/// which fit anyone. A theme gives its own version of each by id.
pub const AVATARS: [Avatar; 8] = [
    Avatar { id: "noor", label: "Noor", shows: "a person in a blue hijab", color: "#c9873a" },
    Avatar { id: "amani", label: "Amani", shows: "a Black woman in a bright head wrap and hoop earrings", color: "#2e8b57" },
    Avatar { id: "yuki", label: "Yuki", shows: "a person in a navy haori, hair in a bun with a hair stick", color: "#c2556f" },
    Avatar { id: "kai", label: "Kai", shows: "a brown-skinned person with short curls and a pale blue, pink and white scarf", color: "#3b8ea5" },
    Avatar { id: "arjun", label: "Arjun", shows: "a bearded Sikh man in a saffron dastar", color: "#4f6d8f" },
    Avatar { id: "sofia", label: "Sofía", shows: "a Latina woman with a braid, in an embroidered white blouse", color: "#7f8c3a" },
    Avatar { id: "mochi", label: "Mochi", shows: "a white cat with orange and black patches", color: "#6b4f9e" },
    Avatar { id: "coco", label: "Coco", shows: "a brown dog with floppy ears and a red collar", color: "#d4941a" },
];

/// The ids the pictures had before #180 (ten icons: an owl, a fox, a leaf…),
/// each mapped to one of today's. `users.avatar` was rewritten by migration
/// v31; theme files and requests may still say them and are read as the new
/// id, the way a renamed lesson field keeps its old spelling as an alias.
pub const ALIASES: [(&str, &str); 10] = [
    ("owl", "noor"),
    ("fox", "kai"),
    ("cat", "mochi"),
    ("bear", "coco"),
    ("rabbit", "sofia"),
    ("robot", "arjun"),
    ("rocket", "yuki"),
    ("star", "amani"),
    ("leaf", "sofia"),
    ("bolt", "mochi"),
];

pub const DEFAULT_AVATAR: &str = "noor";

/// One of the backgrounds a learner can put behind their picture: the badge
/// disc of the round avatar, a colour with a light pattern over it. Slot `n`
/// (1-8) is `AVATAR_COLORS[n - 1]`, and a theme may give its own colour for
/// any slot (`avatar_colors` in theme.json), the way it gives its own version
/// of each picture; the pattern stays. A theme adds scenes of its world in
/// slots 9-16 (`THEME_SLOTS`, `avatar_backgrounds`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvatarColor {
    /// The background's name: the swatch's accessible name, in English.
    pub label: &'static str,
    pub color: &'static str,
    /// The pattern drawn over the colour: `static/avatar-bg/<pattern>.svg`,
    /// white shapes at low opacity, so it suits any colour a theme gives.
    pub pattern: &'static str,
}

impl AvatarColor {
    /// `label`, in the learner's language.
    pub fn name(&self) -> String {
        match self.pattern {
            "sunburst" => crate::t("profile.colors.sunburst"),
            "waves" => crate::t("profile.colors.waves"),
            "confetti" => crate::t("profile.colors.confetti"),
            "stars" => crate::t("profile.colors.stars"),
            "stripes" => crate::t("profile.colors.stripes"),
            "dots" => crate::t("profile.colors.dots"),
            "rings" => crate::t("profile.colors.rings"),
            "zigzag" => crate::t("profile.colors.zigzag"),
            _ => self.label.to_string(),
        }
    }
}

/// The default palette, slots 1 to 8: the pictures' own badge colours (so
/// any picture can wear any other's), each with a pattern.
pub const AVATAR_COLORS: [AvatarColor; 8] = [
    AvatarColor { label: "Amber sunburst", color: "#c9873a", pattern: "sunburst" },
    AvatarColor { label: "Green waves", color: "#2e8b57", pattern: "waves" },
    AvatarColor { label: "Rose confetti", color: "#c2556f", pattern: "confetti" },
    AvatarColor { label: "Teal stars", color: "#3b8ea5", pattern: "stars" },
    AvatarColor { label: "Slate stripes", color: "#4f6d8f", pattern: "stripes" },
    AvatarColor { label: "Olive dots", color: "#7f8c3a", pattern: "dots" },
    AvatarColor { label: "Violet rings", color: "#6b4f9e", pattern: "rings" },
    AvatarColor { label: "Gold zigzag", color: "#d4941a", pattern: "zigzag" },
];

/// The slots a theme's own backgrounds (`avatar_backgrounds`) go in. A slot
/// is what is stored, so a background follows the theme like a colour does:
/// slot 9 is each theme's first scene, and while a theme without one is worn
/// (or none) `DEFAULT_COLOR` shows instead.
pub const THEME_SLOTS: std::ops::RangeInclusive<u8> = 9..=16;

/// The background of an account that has picked none (`users.avatar_color`
/// is 0 until then): slot 1, Amber sunburst. The background belongs to the
/// account, not the picture, so changing the picture keeps it (#263).
pub const DEFAULT_COLOR: u8 = 1;

/// A background slot a learner can pick: 1-8, or a theme's 9-16.
pub fn is_color_slot(slot: u8) -> bool {
    (1..=*THEME_SLOTS.end()).contains(&slot)
}

/// The slot an account has chosen: the one stored, else `DEFAULT_COLOR`.
pub fn color_slot(stored: u8) -> u8 {
    if is_color_slot(stored) { stored } else { DEFAULT_COLOR }
}

/// The slot shown with `palette` worn: the one chosen when the palette offers
/// it, else `DEFAULT_COLOR` (a theme's scene on Default, or on a theme without
/// that slot). The choice stays stored, so the theme brings it back.
pub fn shown_slot(palette: &[Swatch], stored: u8) -> u8 {
    let slot = color_slot(stored);
    if palette.iter().any(|s| s.slot == slot) { slot } else { DEFAULT_COLOR }
}

/// One swatch of the palette worn.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Swatch {
    pub slot: u8,
    pub label: String,
    pub color: String,
    /// The picture over the colour (a pattern, or a theme's scene), as a URL.
    pub image: String,
}

impl Swatch {
    /// The swatch's look, for its `style`: `--sw` and `--sw-img`.
    pub fn style(&self) -> String {
        format!("--sw:{};--sw-img:{}", self.color, css_image(&self.image))
    }
}

/// A URL as a CSS `url(…)`, or `none` for no picture (or one whose URL
/// could break out of the declaration: a theme learners make may give any
/// `https://` URL).
pub fn css_image(url: &str) -> String {
    let safe = !url.is_empty() && !url.chars().any(|c| c.is_whitespace() || c.is_control() || "\"'()\\;<>".contains(c));
    if safe { format!("url({url})") } else { "none".to_string() }
}

/// The eight backgrounds on offer while `theme` is worn, then the theme's own
/// scenes: the theme's colour for each slot it gives one for, the default
/// for the rest (a theme only adds; a theme without `avatar_colors` offers
/// the default palette, one without `avatar_backgrounds` no scenes).
pub fn palette(theme: Option<&crate::content::Theme>) -> Vec<Swatch> {
    let defaults = AVATAR_COLORS.iter().zip(1u8..).map(|(d, slot)| {
        let image = crate::routes::asset_url(&format!("avatar-bg/{}.svg", d.pattern));
        match theme.and_then(|t| t.avatar_colors.iter().find(|c| c.slot == slot).map(|c| (t, c))) {
            Some((t, c)) => Swatch { slot, label: t.slot_said(slot, &c.label), color: c.color.clone(), image },
            None => Swatch { slot, label: d.name(), color: d.color.to_string(), image },
        }
    });
    let mut scenes: Vec<Swatch> = theme
        .map(|t| {
            t.avatar_backgrounds
                .iter()
                .map(|b| Swatch { slot: b.slot, label: t.slot_said(b.slot, &b.label), color: b.color.clone(), image: crate::routes::art_url(&t.id, &b.image) })
                .collect()
        })
        .unwrap_or_default();
    scenes.sort_by_key(|s| s.slot);
    defaults.chain(scenes).collect()
}

/// The colour a picture brings of its own while `theme` is worn, for its
/// tile in the picture dialog: the theme's version's `color`, else the
/// built-in picture's. A theme's version without one has the page's surface
/// behind it (its art usually paints its own badge).
pub fn own_color(avatar_id: &str, theme: Option<&crate::content::Theme>) -> String {
    match theme.and_then(|t| t.avatar(avatar_id)) {
        Some(t) if !t.color.is_empty() => t.color.clone(),
        Some(_) => "var(--surface)".to_string(),
        None => avatar(avatar_id).color.to_string(),
    }
}

/// What is behind a learner's picture: a colour and, over it, a picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Badge {
    pub color: String,
    /// A URL, empty for a plain colour.
    pub image: String,
}

impl Badge {
    /// The `style` of every `.avatar` showing it: `--av` and `--av-img`.
    pub fn style(&self) -> String {
        format!("--av:{};--av-img:{}", self.color, css_image(&self.image))
    }
}

/// The background behind a learner's picture: the chosen slot of the palette
/// worn (`shown_slot`: `DEFAULT_COLOR` if none, or a theme's slot that the
/// theme worn does not have).
pub fn badge(slot: u8, theme: Option<&crate::content::Theme>) -> Badge {
    let palette = palette(theme);
    let slot = shown_slot(&palette, slot);
    palette
        .into_iter()
        .find(|s| s.slot == slot)
        .map(|s| Badge { color: s.color, image: s.image })
        .unwrap_or_else(|| Badge { color: String::new(), image: String::new() })
}

/// The badge colour behind a learner's picture (the `--av` of `.avatar`).
#[cfg(test)]
pub fn badge_color(slot: u8, theme: Option<&crate::content::Theme>) -> String {
    badge(slot, theme).color
}

/// The id as stored today: a current id as is, an old id (`ALIASES`) as
/// the picture it became, anything else `None`.
pub fn canonical(id: &str) -> Option<&'static str> {
    AVATARS
        .iter()
        .map(|a| a.id)
        .find(|a| *a == id)
        .or_else(|| ALIASES.iter().find(|(old, _)| *old == id).map(|(_, new)| *new))
}

/// The avatar with this id (an old id resolves too), or the default one for
/// anything unknown (a hand-edited value): a profile never fails to render.
pub fn avatar(id: &str) -> Avatar {
    canonical(id).and_then(|c| AVATARS.iter().copied().find(|a| a.id == c)).unwrap_or(AVATARS[0])
}

/// The two portraits that speak a lesson conversation's roles (#989): two
/// of the six people in `AVATARS` (the pets don't take a named role), never
/// the learner's own picture `own`, so the exchange reads as two other
/// people. A role keeps its portrait wherever its name comes back (Lucía is
/// the same face in every lesson she is in, for the same learner).
pub fn cast(role_a: &str, role_b: &str, own: &str) -> [&'static str; 2] {
    let own = canonical(own);
    let people: Vec<&'static str> = AVATARS.iter().take(6).map(|a| a.id).filter(|id| Some(*id) != own).collect();
    // FNV-1a: stable across builds, unlike `DefaultHasher`.
    let hash = |s: &str| s.trim().to_lowercase().bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    let n = people.len() as u64;
    let a = (hash(role_a) % n) as usize;
    let mut b = (hash(role_b) % n) as usize;
    if b == a {
        b = (b + 1) % people.len();
    }
    [people.get(a).copied().unwrap_or(DEFAULT_AVATAR), people.get(b).copied().unwrap_or(DEFAULT_AVATAR)]
}

/// A picture id, current or old.
pub fn is_avatar(id: &str) -> bool {
    canonical(id).is_some()
}


// ---------- validation ----------

pub const NAME_MAX: usize = 60;
pub const EMAIL_MAX: usize = 254;
/// The most the part before the `@` may have (RFC 5321); `lettre` refuses more.
pub const EMAIL_LOCAL_MAX: usize = 64;

/// A first or last name: optional, up to `NAME_MAX` characters, no control
/// characters. Returns the trimmed value.
pub fn validate_name(label: &str, raw: &str) -> Result<String, String> {
    let v = raw.trim();
    if v.chars().count() > NAME_MAX {
        return Err(crate::t2("profile.name.too_long", "label", label, "max", NAME_MAX));
    }
    if v.chars().any(char::is_control) {
        return Err(crate::t1("profile.name.bad_chars", "label", label));
    }
    Ok(v.to_string())
}

/// A plausible e-mail address, normalised to lower case. Deliberately loose:
/// one `@`, something on both sides, a dot in the domain, no whitespace.
pub fn validate_email(raw: &str) -> Result<String, String> {
    let v = raw.trim().to_lowercase();
    if v.is_empty() {
        return Err(crate::t("profile.email.empty"));
    }
    if v.len() > EMAIL_MAX {
        return Err(crate::t1("profile.email.too_long", "max", EMAIL_MAX));
    }
    if v.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(crate::t("profile.email.spaces"));
    }
    let Some((local, domain)) = v.split_once('@') else {
        return Err(crate::t("profile.email.no_at"));
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return Err(crate::t("profile.email.not_email"));
    }
    if local.len() > EMAIL_LOCAL_MAX {
        return Err(crate::t1("profile.email.local_too_long", "max", EMAIL_LOCAL_MAX));
    }
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return Err(crate::t("profile.email.no_domain"));
    }
    // What we accept must be an address mail can be sent to (#1003).
    if v.parse::<lettre::Address>().is_err() {
        return Err(crate::t("profile.email.not_email"));
    }
    Ok(v)
}

// ---------- persistence (run inside Db::call) ----------

/// Everything the profile page shows about an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub first_name: String,
    pub last_name: String,
    /// `None` until the account has one; afterwards fixed.
    pub email: Option<String>,
    pub avatar: String,
    /// The background behind the picture: a palette slot (1-8) or a theme's
    /// (9-16), `DEFAULT_COLOR` until one is picked.
    pub avatar_color: u8,
    pub created_at: i64,
    /// Accounts without one (signed in through a proxy) have no password to change.
    pub has_password: bool,
    /// The Terms of Service version last accepted, and when.
    pub terms: Option<crate::terms::Acceptance>,
    /// "Hide all AI features" (`users.ai_off`).
    pub ai_off: bool,
    /// When the age box was ticked (`consent::age_confirmed_at`).
    pub age_confirmed_at: Option<i64>,
    /// The AI features consent's latest row (`consent::AI`), for its date.
    pub ai_consent: Option<crate::consent::Record>,
    /// The phone number and the second sign-in step (`two_factor.rs`).
    pub two_factor: crate::two_factor::Settings,
}

impl Profile {
    /// The "Do Not Show Me AI Features" row of the Privacy and consent card:
    /// shown or hidden, and since when if the record says.
    pub fn ai_consent_text(&self) -> String {
        ai_consent_text(self.ai_off, self.ai_consent)
    }

    /// The age row of the Privacy and consent card.
    pub fn age_text(&self) -> String {
        match self.age_confirmed_at {
            Some(t) => crate::t1("profile.consent.age_confirmed", "date", long_date(t)),
            None => crate::t("profile.consent.age_not_confirmed"),
        }
    }

    /// What a Google-only account types to confirm deleting itself: its
    /// address (Google always gives one), or "delete" should it have none.
    pub fn delete_phrase(&self) -> String {
        self.email.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| "delete".into())
    }
}

/// "Shown since 3 March 2026", "Hidden since …", or plain "Shown"/"Hidden" for an
/// account with no record (made before v82 with AI hidden, or by a script).
pub fn ai_consent_text(ai_off: bool, latest: Option<crate::consent::Record>) -> String {
    match (ai_off, latest) {
        (false, Some(r)) if r.withdrawn_at.is_none() => crate::t1("profile.consent.ai_on_since", "date", long_date(r.granted_at)),
        (true, Some(crate::consent::Record { withdrawn_at: Some(t), .. })) => crate::t1("profile.consent.ai_off_since", "date", long_date(t)),
        (false, _) => crate::t("profile.consent.ai_on"),
        (true, _) => crate::t("profile.consent.ai_off"),
    }
}

pub fn load(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Profile>> {
    let terms = crate::terms::latest(conn, user_id)?;
    let age_confirmed_at = crate::consent::age_confirmed_at(conn, user_id)?;
    let ai_consent = crate::consent::latest(conn, user_id, crate::consent::AI)?;
    let two_factor = match crate::two_factor::settings(conn, user_id) {
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        r => r?,
    };
    conn.query_row(
        "SELECT first_name, last_name, NULLIF(email, ''), avatar, created_at,
                password_hash != '', ai_off, avatar_color
         FROM users WHERE id = ?1",
        [user_id],
        |r| {
            Ok(Profile {
                first_name: r.get(0)?,
                last_name: r.get(1)?,
                email: r.get(2)?,
                avatar: r.get(3)?,
                created_at: r.get(4)?,
                has_password: r.get(5)?,
                terms: terms.clone(),
                ai_off: r.get(6)?,
                age_confirmed_at,
                ai_consent,
                avatar_color: color_slot(r.get(7)?),
                two_factor: two_factor.clone(),
            })
        },
    )
    .optional()
}

/// Through `idx_users_email_lower` (#605), like every lookup by address.
const EMAIL_TAKEN_SQL: &str = "SELECT EXISTS(SELECT 1 FROM users WHERE lower(email) = lower(?1) AND id != ?2)";

/// Is this address already on another account? Case-insensitive.
/// (Registration uses `reset::account_for` since #379: it needs the account.)
#[allow(dead_code)]
pub fn email_taken(conn: &Connection, email: &str, except_user: Option<i64>) -> rusqlite::Result<bool> {
    conn.query_row(
        EMAIL_TAKEN_SQL,
        params![email, except_user.unwrap_or(-1)],
        |r| r.get(0),
    )
}

/// Save the editable part of a profile; `None` keeps a field as it is. The
/// names are already validated. (The picture and the motivator are a
/// course's settings, #565 and #574: `courses::set_project_settings`.)
pub fn update(conn: &Connection, user_id: i64, first_name: Option<&str>, last_name: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE users SET first_name = COALESCE(?2, first_name), last_name = COALESCE(?3, last_name) WHERE id = ?1",
        params![user_id, first_name, last_name],
    )?;
    Ok(())
}

pub fn password_hash(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT password_hash FROM users WHERE id = ?1", [user_id], |r| r.get(0)).optional()
}

/// The account's own new password: a temporary one the owner set
/// (`users.must_change_password`, #635) is replaced, so the gate lifts.
pub fn set_password_hash(conn: &Connection, user_id: i64, hash: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET password_hash = ?2, must_change_password = 0 WHERE id = ?1", params![user_id, hash])?;
    Ok(())
}

/// "Don't show this automatically next time" in the review of AI changes
/// (`users.review_quiet`, #187): the review waits for its button.
pub fn review_quiet(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    conn.query_row("SELECT review_quiet FROM users WHERE id = ?1", [user_id], |r| r.get(0)).optional().map(|q| q.unwrap_or(false))
}

pub fn set_review_quiet(conn: &Connection, user_id: i64, quiet: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET review_quiet = ?2 WHERE id = ?1", params![user_id, quiet])?;
    Ok(())
}

/// "Hide all AI features" (`users.ai_off`): set at sign-up or from the
/// profile's AI features section, and undone there. Each change is a
/// consent given or withdrawn (`consent::AI`, #974).
pub fn set_ai_off(conn: &Connection, user_id: i64, off: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET ai_off = ?2 WHERE id = ?1", params![user_id, off])?;
    crate::consent::record(conn, user_id, crate::consent::AI, !off)
}

/// `users.lang`: the language the app speaks to this account in ('' follows the browser).
pub fn set_lang(conn: &Connection, user_id: i64, lang: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET lang = ?2 WHERE id = ?1", params![user_id, lang])?;
    Ok(())
}


/// What a learner is called until they give a name ("Learner").
pub fn no_name() -> String {
    crate::t("profile.no_name")
}

/// What the navigation bar and greetings call someone: first name when
/// known, else "Learner".
pub fn short_name(first_name: &str) -> String {
    let f = first_name.trim();
    if f.is_empty() { no_name() } else { f.to_string() }
}

/// "First Last", or "Learner" when neither name is set.
pub fn display_name(first_name: &str, last_name: &str) -> String {
    let full = format!("{} {}", first_name.trim(), last_name.trim());
    let full = full.trim();
    if full.is_empty() { no_name() } else { full.to_string() }
}

/// How other learners see someone (leaderboards): "Ana L.", "Ana", or
/// "Learner" when no first name is set.
pub fn public_name(first_name: &str, last_name: &str) -> String {
    let f = first_name.trim();
    if f.is_empty() {
        return no_name();
    }
    match last_name.trim().chars().next() {
        Some(i) => format!("{f} {}.", i.to_uppercase()),
        None => f.to_string(),
    }
}

/// Month and year an account was created, for "member since".
pub fn member_since(created_at: i64) -> String {
    let (y, m, _) = civil(created_at);
    crate::t2("profile.date.month_year", "month", month_name(m), "year", y)
}

/// "16 October 2026", for dates such as a subscription's renewal.
pub fn long_date(t: i64) -> String {
    let (y, m, d) = civil(t);
    crate::i18n::fill(&crate::t("profile.date.long"), &[("day", &d), ("month", &month_name(m)), ("year", &y)])
}

/// The name of month `m` (1–12) in the learner's language.
fn month_name(m: usize) -> String {
    crate::t(match m {
        1 => "profile.month.january",
        2 => "profile.month.february",
        3 => "profile.month.march",
        4 => "profile.month.april",
        5 => "profile.month.may",
        6 => "profile.month.june",
        7 => "profile.month.july",
        8 => "profile.month.august",
        9 => "profile.month.september",
        10 => "profile.month.october",
        11 => "profile.month.november",
        _ => "profile.month.december",
    })
}

/// (year, month 1–12, day) of a Unix time, UTC.
pub(crate) fn civil(t: i64) -> (i64, usize, i64) {
    // Civil-from-days (Howard Hinnant's algorithm); no chrono dependency.
    let days = t.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as usize, d)
}

/// Days since the Unix epoch of a civil date (Howard Hinnant's days-from-civil).
pub(crate) fn days_from_civil(y: i64, m: usize, d: i64) -> i64 {
    let m = m as i64;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;

    #[test]
    fn email_taken_reads_through_the_index_in_any_case() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            assert_eq!(crate::db::table_scans(c, EMAIL_TAKEN_SQL).unwrap(), Vec::<String>::new());
            let ann = auth::create_user(c, "Ann@Example.org", "h")?;
            assert!(email_taken(c, "ann@example.org", None)?);
            assert!(email_taken(c, "ANN@example.org", Some(ann + 1))?);
            assert!(!email_taken(c, "ann@example.org", Some(ann))?, "their own address");
            assert!(!email_taken(c, "bob@example.org", None)?);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_conversation_is_cast_with_two_other_people() {
        for own in AVATARS.iter().map(|a| a.id) {
            for (a, b) in [("Lucía", "Leo"), ("Marta", "Marta"), ("", ""), ("Nora", "Javier")] {
                let [x, y] = cast(a, b, own);
                assert_ne!(x, y, "{a}/{b} for {own}");
                assert!(x != own && y != own, "{a}/{b} for {own}");
                assert!(!["mochi", "coco"].contains(&x) && !["mochi", "coco"].contains(&y));
            }
        }
        assert_eq!(cast("Lucía", "Leo", "noor"), cast("lucía", "Leo", "noor"), "a name keeps its face");
        assert_eq!(cast("Lucía", "Leo", "owl"), cast("Lucía", "Leo", "noor"), "an old id is its new one");
    }

    #[test]
    fn avatars_are_distinct_and_default_exists() {
        let mut ids: Vec<_> = AVATARS.iter().map(|a| a.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 8);
        assert!(is_avatar(DEFAULT_AVATAR));
        assert_eq!(avatar("kai").label, "Kai");
        assert_eq!(avatar("no-such-thing").id, DEFAULT_AVATAR);
    }

    #[test]
    fn validates_names() {
        assert_eq!(validate_name("First name", "  Ana ").unwrap(), "Ana");
        assert_eq!(validate_name("First name", "").unwrap(), "");
        assert!(validate_name("First name", &"x".repeat(61)).is_err());
        assert!(validate_name("Last name", "a\u{0}b").is_err());
    }

    #[test]
    fn validates_emails() {
        assert_eq!(validate_email(" Ana.Lopez@Example.ORG ").unwrap(), "ana.lopez@example.org");
        for bad in ["", "ana", "ana@", "@example.org", "ana@example", "ana@.org", "ana@ex..org", "a na@example.org", "a@b@c.org"] {
            assert!(validate_email(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn email_local_part_is_at_most_64() {
        let ok = format!("{}@example.org", "a".repeat(64));
        assert_eq!(validate_email(&ok).unwrap(), ok);
        assert!(validate_email(&format!("{}@example.org", "a".repeat(65))).is_err());
        assert!(validate_email("a-rather-long-owner-address-for-the-phone-tip-check-1027015107-11@example.com").is_err());
    }

    /// Whatever `validate_email` accepts, the mailer can address (#1003).
    #[test]
    fn accepted_emails_can_be_mailed() {
        let long = format!("{}@example.org", "x".repeat(64));
        let tries = [
            "ana@example.org", "Ana.Lopez+tag@Example.ORG", "o'brien@example.co.uk", "a_b-c@sub.example.org", long.as_str(),
            "a(b)@example.org", "a<b@example.org", "a\\b@example.org", "a\"b@example.org", "a,b@example.org", "é@example.org",
        ];
        for raw in tries {
            if let Ok(v) = validate_email(raw) {
                assert!(crate::mail::Mail::new(&v, "Hi", "text", "test").is_ok(), "{v:?} passed validation but cannot be mailed");
            }
        }
        assert!(validate_email("ana@example.org").is_ok());
        assert!(validate_email(&long).is_ok());
    }

    #[test]
    fn names_and_dates() {
        assert_eq!(short_name(""), "Learner");
        assert_eq!(short_name(" Ana "), "Ana");
        assert_eq!(display_name("", ""), "Learner");
        assert_eq!(display_name("", "López"), "López");
        assert_eq!(display_name("Ana", ""), "Ana");
        assert_eq!(display_name("Ana", "López"), "Ana López");
        assert_eq!(public_name("Ana", "López"), "Ana L.");
        assert_eq!(public_name("Ana", "élan"), "Ana É.");
        assert_eq!(public_name(" Ana ", ""), "Ana");
        assert_eq!(public_name("", "López"), "Learner");
        assert_eq!(member_since(0), "January 1970");
        assert_eq!(member_since(1_759_881_600), "October 2025"); // 2025-10-08
        assert_eq!(member_since(951_782_400), "February 2000"); // 2000-02-29
        assert_eq!(long_date(951_782_400), "29 February 2000");
        assert_eq!(long_date(1_759_881_600 + 86_399), "8 October 2025");
    }

    #[test]
    fn pictures_resolve_by_new_and_old_ids() {
        assert_eq!(avatar("noor").label, "Noor");
        assert_eq!(avatar("coco").id, "coco");
        // The ids from before #180 still name a picture; anything else is the default.
        assert_eq!((canonical("owl"), canonical("leaf"), canonical("dragon")), (Some("noor"), Some("sofia"), None));
        assert_eq!((avatar("fox").id, avatar("dragon").id, avatar("").id), ("kai", DEFAULT_AVATAR, DEFAULT_AVATAR));
        assert!(is_avatar("bolt") && is_avatar("mochi") && !is_avatar("Mochi"));
        assert!(ALIASES.iter().all(|(_, new)| AVATARS.iter().any(|a| a.id == *new)));
        assert_eq!(AVATARS.len(), 8);
        assert!(AVATARS.iter().all(|a| !a.label.is_empty() && !a.shows.is_empty() && a.color.starts_with('#')));
    }

    #[test]
    fn avatar_colours_fall_back_to_the_default_palette() {
        use crate::content::{Theme, ThemeAvatar, ThemeAvatarBackground, ThemeAvatarColor};
        let plain = palette(None);
        assert_eq!(plain.len(), 8);
        assert_eq!((plain[0].slot, plain[0].label.as_str(), plain[7].slot), (1, "Amber sunburst", 8));
        assert!(plain.iter().all(|s| s.image.contains("/static/avatar-bg/") && s.image.contains(".svg")));
        // No colour chosen: Amber sunburst, whichever the picture.
        assert_eq!(badge_color(0, None), "#c9873a");
        assert_eq!(badge_color(2, None), "#2e8b57");
        assert!(!is_color_slot(0) && is_color_slot(1) && is_color_slot(8) && is_color_slot(16) && !is_color_slot(17));
        assert_eq!((color_slot(0), color_slot(5), color_slot(9), color_slot(17)), (DEFAULT_COLOR, 5, 9, DEFAULT_COLOR));
        // A theme's slot with no theme worn: the first background.
        assert_eq!(shown_slot(&plain, 9), DEFAULT_COLOR);
        assert_eq!(badge(9, None), badge(DEFAULT_COLOR, None));
        assert!(badge(4, None).style().starts_with("--av:#3b8ea5;--av-img:url(/static/avatar-bg/stars.svg"));

        // A theme without a palette offers the default one; one that gives
        // some slots changes only those.
        let bare = Theme { id: "bare".into(), title: "Bare".into(), ..Theme::default() };
        assert_eq!(palette(Some(&bare)), plain);
        let some = Theme {
            avatar_colors: vec![ThemeAvatarColor { slot: 3, label: "Ember".into(), color: "#aa3311".into() }],
            avatars: vec![
                ThemeAvatar { id: "kai".into(), label: "Night fox".into(), image: "pictures/fox.svg".into(), color: "#102030".into() },
                ThemeAvatar { id: "noor".into(), label: String::new(), image: "pictures/owl.svg".into(), color: String::new() },
            ],
            ..bare.clone()
        };
        let p = palette(Some(&some));
        assert_eq!((p[2].label.as_str(), p[2].color.as_str()), ("Ember", "#aa3311"));
        assert_eq!(p.iter().filter(|s| *s != &plain[s.slot as usize - 1]).count(), 1);
        assert_eq!(badge_color(3, Some(&some)), "#aa3311");
        assert_eq!(badge_color(4, Some(&some)), "#3b8ea5");
        // A picture's own colour (its tile in the dialog) while worn: the
        // theme's version's, else the surface (a version without one), else
        // the built-in picture's.
        assert_eq!(own_color("kai", Some(&some)), "#102030");
        assert_eq!(own_color("noor", Some(&some)), "var(--surface)");
        assert_eq!(own_color("coco", Some(&some)), avatar("coco").color);
        // None chosen while worn: the theme's slot 1, whatever the picture.
        assert_eq!(badge_color(0, Some(&some)), "#c9873a");

        // A theme's scenes come after the eight, in slot order, as theme files.
        let scenic = Theme {
            avatar_backgrounds: vec![
                ThemeAvatarBackground { slot: 11, label: "Dunes".into(), image: "pictures/backgrounds/dunes.svg".into(), color: "#aa8844".into() },
                ThemeAvatarBackground { slot: 9, label: "Hills".into(), image: "https://example.org/hills.svg".into(), color: "#335533".into() },
            ],
            ..some.clone()
        };
        let p = palette(Some(&scenic));
        assert_eq!(p.iter().map(|s| s.slot).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6, 7, 8, 9, 11]);
        assert_eq!((p[8].label.as_str(), p[8].image.as_str()), ("Hills", "https://example.org/hills.svg"));
        assert!(p[9].image.ends_with("/themes/bare/pictures/backgrounds/dunes.svg"));
        assert_eq!(badge(11, Some(&scenic)).color, "#aa8844");
        // Slot 10 is not this theme's: the first background of the palette worn.
        assert_eq!(shown_slot(&p, 10), DEFAULT_COLOR);
        assert_eq!(badge(10, Some(&scenic)), badge(DEFAULT_COLOR, Some(&scenic)));
        // A URL that could break out of the style is dropped.
        assert_eq!(css_image("https://x.org/a.svg);color:red"), "none");
        assert_eq!(css_image(""), "none");
    }

    #[test]
    fn profile_roundtrip() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let id = auth::create_user(c, "ana@example.org", "h")?;
            let p = load(c, id)?.unwrap();
            assert_eq!((p.avatar.as_str(), p.email.as_deref(), p.avatar_color), (DEFAULT_AVATAR, Some("ana@example.org"), DEFAULT_COLOR));
            assert!(p.has_password);

            update(c, id, Some("Ana"), Some("López"))?;
            // Fields left out keep their value.
            update(c, id, None, Some(""))?;
            let p = load(c, id)?.unwrap();
            assert_eq!((p.first_name.as_str(), p.last_name.as_str()), ("Ana", ""));
            assert!(!p.ai_off);
            set_ai_off(c, id, true)?;
            assert!(load(c, id)?.unwrap().ai_off);
            set_ai_off(c, id, false)?;
            assert!(!load(c, id)?.unwrap().ai_off);

            assert!(email_taken(c, "ANA@example.org", None)?);
            assert!(!email_taken(c, "ana@example.org", Some(id))?);
            assert!(!email_taken(c, "bob@example.org", None)?);
            assert_eq!(load(c, 999)?, None);
            Ok(())
        })
        .unwrap();
    }
}
