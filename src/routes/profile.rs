//! `/profile`: the page behind the picture in the top-right corner. Picture,
//! personal information, account details, password changes, this month's
//! AI usage (allowance, credits), the AI API keys and the "Hide all AI
//! features" switch (#147). It takes `CurrentUser`, not `Project`: a profile exists before any
//! course is chosen.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser};
use crate::billing::{self, SubscriptionView};
use crate::error::AppError;
use crate::profile::{self, Avatar, Profile};
use crate::secrets::{self, KeyStatus, Provider};
use crate::AppState;

#[derive(Template)]
#[template(path = "profile.html")]
pub struct ProfileTemplate {
    pub user: Option<CurrentUser>,
    pub profile: Profile,
    pub display_name: String,
    /// How other learners see the name being edited ("Ana L."), for the privacy note.
    pub public_name: String,
    pub member_since: String,
    pub google_enabled: bool,
    pub notice: Option<String>,
    /// Which form failed, so the message sits next to it.
    pub password_error: Option<String>,
    /// One entry per provider; never the key itself.
    pub api_keys: Vec<KeyStatus>,
    /// Any key on file (readable or not): hiding the AI features asks first, since it deletes them.
    pub keys_on_file: bool,
    /// The server has plans (the hosted site, `billing::plans_on`); off,
    /// the Subscription section, its nav link and the plans modal are left out.
    pub plans: bool,
    /// Plan and subscription (`billing.rs`).
    pub subscription: SubscriptionView,
    /// The plans modal opens on load (`?plans=1`, or sent here to upgrade).
    pub plans_open: bool,
    /// The cancel dialog's reasons (`billing::CANCEL_REASONS`).
    pub cancel_reasons: &'static [(&'static str, &'static str)],
    /// This month's Skrongle AI as a bar (`metering.rs`).
    pub ai: AiUsageView,
    /// Dev mode: show the Plus grant toggle.
    pub dev: bool,
    /// Why deleting the account was refused.
    pub delete_error: Option<String>,
    /// The Terms of Service as HTML, for the dialog the "read" link opens.
    pub terms_html: String,
    /// Two-factor sign-in: why the chosen method was refused.
    pub two_factor_error: Option<String>,
    /// The authenticator app's enrolment dialog opens on load (just started,
    /// or a wrong code), and why its code was refused.
    pub totp_open: bool,
    pub totp_error: Option<String>,
    /// The enrolment under way, decrypted only when its dialog opens.
    pub totp_key: Option<crate::totp::Key>,
    /// The "confirm it's you" dialog (turning two-factor off or switching
    /// it): open on load after a refusal or a code sent, the method asked
    /// for (`''` off), whether it came from the app's Remove, why it was
    /// refused, and where a code went (a Google-only account).
    pub confirm_open: bool,
    pub confirm_method: String,
    pub confirm_remove: bool,
    pub confirm_error: Option<String>,
    pub confirm_sent: Option<String>,
    /// The phone number's verify dialog opens on load (a code was just
    /// sent, or a wrong one typed), and what went wrong.
    pub phone_open: bool,
    pub phone_sent: bool,
    pub phone_error: Option<String>,
    /// A Google-only account without a second step just confirmed it's
    /// its owner with Google (#376): the next change goes ahead.
    pub google_confirmed: bool,
    /// The Google link / unlink dialog to open on load (`link`, `unlink`,
    /// `''`), and why it was refused.
    pub google_open: String,
    pub google_error: Option<String>,
    /// Deleting a Google-only account: where its code went.
    pub delete_sent: Option<String>,
    /// "Download my data": the newest export (exports.rs), and why a link did not work.
    pub export: Option<crate::exports::Status>,
    pub export_error: Option<String>,
    /// What this server can send (#633): without mail no confirmation
    /// link or e-mailed codes; without texts no phone number.
    pub caps: crate::Caps,
}

impl ProfileTemplate {
    /// How this account proves it is its owner (`prove_owner`): `password`;
    /// a Google-only account a `code` from its app, one it is `send`
    /// (texted or e-mailed), or, without a second step, `google`.
    pub fn proof_kind(&self) -> &'static str {
        match (self.profile.has_password, self.profile.two_factor.method) {
            (true, _) => "password",
            (false, Some(crate::two_factor::Method::Totp)) => "code",
            (false, Some(_)) => "send",
            (false, None) => "google",
        }
    }

    /// The newest export's state (`''`: none yet).
    pub fn export_state(&self) -> &str {
        self.export.as_ref().map_or("", |e| e.state.as_str())
    }

    /// Can a new export be asked for now?
    pub fn export_can_ask(&self) -> bool {
        self.export.as_ref().is_none_or(|e| !e.in_flight() && e.next_at.is_none())
    }

    /// What the (i) dialog says about the newest export.
    pub fn export_text(&self) -> String {
        let to = self.profile.email.clone().unwrap_or_else(|| crate::t("profile.data.export_your_address"));
        let Some(e) = &self.export else { return crate::t("profile.data.export_none") };
        let mut s = match e.state.as_str() {
            "queued" | "building" if !self.caps.mail => crate::t("profile.data.export_building_no_mail"),
            "queued" | "building" => crate::t1("profile.data.export_building", "email", &to),
            "ready" if !self.caps.mail => crate::t1("profile.data.export_ready_no_mail", "date", e.expires_at.map(profile::long_date).unwrap_or_default()),
            "ready" => crate::t2("profile.data.export_ready", "email", &to, "date", e.expires_at.map(profile::long_date).unwrap_or_default()),
            "downloaded" => crate::t1("profile.data.export_downloaded", "date", e.downloaded_at.map(profile::long_date).unwrap_or_default()),
            "expired" => crate::t("profile.data.export_expired"),
            _ => crate::t("profile.data.export_failed"),
        };
        if let (Some(t), false) = (e.next_at, e.in_flight()) {
            s.push(' ');
            s.push_str(&crate::t1("profile.data.export_next", "date", profile::long_date(t)));
        }
        s
    }

    /// `''` (off), `sms`, `email` or `totp`.
    pub fn two_factor_method(&self) -> &'static str {
        self.profile.two_factor.method.map(crate::two_factor::Method::as_str).unwrap_or("")
    }

    pub fn email_2fa_ok(&self) -> bool {
        self.profile.two_factor.refusal(crate::two_factor::Method::Email).is_none()
    }

    pub fn sms_2fa_ok(&self) -> bool {
        self.profile.two_factor.refusal(crate::two_factor::Method::Sms).is_none()
    }

    /// Is the method offered at all? Not when its channel is off (#633).
    pub fn email_2fa_offered(&self) -> bool {
        self.caps.mail
    }

    pub fn sms_2fa_offered(&self) -> bool {
        self.caps.sms
    }

    /// The e-mail field's red ! (confirm your address): only on a server
    /// that can send the link.
    pub fn email_needs_confirming(&self) -> bool {
        self.caps.mail && self.user.as_ref().is_some_and(|u| u.needs_email_confirmation())
    }

    /// When the authenticator app was set up, if it is.
    pub fn totp_since(&self) -> Option<String> {
        self.profile.two_factor.totp_since.map(profile::long_date)
    }

    /// The enrolment under way: its key as the app is handed it.
    fn pending_key(&self) -> Option<crate::totp::Key> {
        self.totp_key.clone()
    }

    /// The confirm dialog's action: the method form, or the app's Remove.
    pub fn confirm_action(&self) -> String {
        crate::url(if self.confirm_remove { "/profile/two-factor/totp/remove" } else { "/profile/two-factor" })
    }

    /// What the confirm dialog says the change is.
    pub fn confirm_what(&self) -> String {
        match (self.confirm_remove, crate::two_factor::Method::parse(&self.confirm_method)) {
            (true, _) | (false, None) => crate::t("profile.two_factor.turn_off"),
            (false, Some(m)) if self.profile.two_factor.method.is_none() => crate::t1("profile.two_factor.turn_on_by", "method", method_label(m)),
            (false, Some(m)) => crate::t1("profile.two_factor.switch_to", "method", method_label(m)),
        }
    }

    /// A Google-only account proves it with a code: from the app, or one
    /// sent by the current method (`sms` / `email`).
    pub fn confirm_code_sent_by(&self) -> &'static str {
        match self.profile.two_factor.method {
            Some(crate::two_factor::Method::Sms) => "sms",
            Some(crate::two_factor::Method::Email) => "email",
            _ => "",
        }
    }

    /// The QR code of the link (#304), for the phone's authenticator app to scan.
    pub fn totp_qr(&self) -> String {
        crate::totp::qr_svg(&self.totp_uri(), &crate::t("profile.two_factor.qr_title")).unwrap_or_default()
    }

    /// The setup key in groups of four, as apps show it.
    pub fn totp_secret(&self) -> String {
        let raw = self.pending_key().map(|k| k.secret_base32()).unwrap_or_default();
        raw.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
    }

    pub fn totp_uri(&self) -> String {
        self.pending_key().map(|k| k.uri()).unwrap_or_default()
    }

    /// Dev mode: the "scan" into the dev authenticator.
    pub fn totp_dev_link(&self) -> String {
        crate::url(&format!("/dev/authenticator?uri={}", crate::oauth::urlencode(&self.totp_uri())))
    }
}

/// The profile's "Skrongle AI this month" card: a bar, never dollar amounts
/// (#315). Shown only to Plus learners on a server with Skrongle AI.
#[derive(Clone, Debug, Default)]
pub struct AiUsageView {
    /// The server offers its own key (Skrongle AI).
    pub hosted: bool,
    /// ... to this learner: their plan includes some, or they hold credits (#283).
    pub offered: bool,
    /// The card shows: Skrongle AI is on and the learner is on Plus.
    pub shown: bool,
    /// Of this month's Skrongle AI (allowance and credits), what is spent or
    /// held by running jobs, 0–100.
    pub percent_used: u8,
    pub percent_left: u8,
    /// When the month's allowance comes back: "1 November 2026".
    pub resets_on: String,
    /// The month's Skrongle AI is enough to write the smallest course; else
    /// it is for the tutor only, and the (i) says so (#283).
    pub courses: bool,
}

impl AiUsageView {
    /// The bar's colour: `ok` with plenty left, `low` under a fifth, `out` when none.
    pub fn level(&self) -> &'static str {
        match self.percent_left {
            0 => "out",
            1..=20 => "low",
            _ => "ok",
        }
    }
}

fn ai_usage_view(s: &crate::metering::AiStatus, meter: &crate::metering::Meter) -> AiUsageView {
    let next_month = billing::add_month(crate::metering::month_start(crate::db::now()));
    AiUsageView {
        hosted: s.hosted,
        offered: s.offered(),
        shown: s.hosted && s.plan == billing::Plan::Plus && !s.unmetered,
        percent_used: s.percent_used(),
        percent_left: s.percent_left(),
        resets_on: profile::long_date(next_month),
        courses: s.covers(meter.cheapest(crate::metering::Need::Course)),
    }
}

/// One picture in a course's settings (#565): the built-in icon, or a
/// theme's version of it (same id, so the choice survives switching themes).
#[derive(Clone, Debug)]
pub(crate) struct AvatarChoice {
    pub id: &'static str,
    pub label: String,
    /// Who the built-in picture shows (the tooltip while no theme replaces it).
    pub shows: String,
    /// The picture's own badge colour while the theme is worn
    /// (`profile::own_color`), and the built-in picture's.
    pub color: String,
    /// The themed picture's URL; `None` draws the built-in icon.
    pub url: Option<String>,
}

pub(super) fn avatar_choices(theme: Option<&crate::content::Theme>) -> Vec<AvatarChoice> {
    profile::AVATARS
        .iter()
        .map(|a: &Avatar| {
            let color = profile::own_color(a.id, theme);
            match theme.and_then(|t| t.avatar(a.id)).cloned() {
                Some(t) => AvatarChoice {
                    id: a.id,
                    label: if t.label.trim().is_empty() { a.label.to_string() } else { t.label },
                    shows: a.description(),
                    color,
                    url: theme.map(|th| super::art_url(&th.id, &t.image)),
                },
                None => AvatarChoice { id: a.id, label: a.label.to_string(), shows: a.description(), color, url: None },
            }
        })
        .collect()
}

#[derive(Deserialize)]
pub struct ShowQuery {
    pub saved: Option<String>,
    pub provider: Option<String>,
    /// `done` / `cancelled`: back from Stripe Checkout.
    pub billing: Option<String>,
    /// Sent here by a paid feature (`billing::RequirePremium`).
    pub upgrade: Option<String>,
    /// `1`: open the plans modal (the Upgrade links).
    pub plans: Option<String>,
    /// `done` / `cancelled`: back from buying AI credits.
    pub credits: Option<String>,
    /// `off`: sent here by an AI feature while AI features are off (`AppError::AiOff`).
    pub ai: Option<String>,
    /// `enrol`: an authenticator app's enrolment just started.
    pub totp: Option<String>,
    /// `sent`: a code was just texted to the phone number.
    pub phone: Option<String>,
    /// `google`: back from confirming it's you with Google (#376), for
    /// the two-factor `method` being chosen (else to delete the account).
    pub confirmed: Option<String>,
    pub method: Option<String>,
}

#[derive(Deserialize)]
pub struct AiForm {
    /// `1` turns AI features off, `0` back on.
    #[serde(default)]
    pub off: String,
}

#[derive(Deserialize)]
pub struct OwnKeysForm {
    /// `1`: the learner's own keys take over when Skrongle AI runs out; `0`: Skrongle AI only.
    #[serde(default)]
    pub on: String,
}

#[derive(Deserialize)]
pub struct ApiKeyForm {
    #[serde(default)]
    pub provider: String,
}

#[derive(Deserialize)]
pub struct PasswordForm {
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub new: String,
    #[serde(default)]
    pub confirm: String,
}

/// Everything the page shows besides the form values.
type Loaded = (Profile, Vec<KeyStatus>, SubscriptionView, AiUsageView);

async fn load_profile(state: &AppState, user: &CurrentUser) -> Result<Loaded, AppError> {
    let vault = state.vault.clone();
    let stripe = state.stripe.clone();
    let meter = Arc::clone(&state.meter);
    let user_id = user.id;
    let loaded = state
        .db
        .call(move |c| {
            let Some(p) = profile::load(c, user_id)? else { return Ok(None) };
            let ai = meter.status(c, user_id)?;
            Ok(Some((p, secrets::statuses(c, &vault, user_id)?, billing::view(c, user_id, stripe.as_ref())?, ai)))
        })
        .await?;
    let (p, keys, sub, ai) = loaded.ok_or(AppError::RedirectToLogin)?;
    Ok((p, keys, sub, ai_usage_view(&ai, &state.meter)))
}

fn page(state: &AppState, user: CurrentUser, (p, api_keys, subscription, ai): Loaded) -> ProfileTemplate {
    let google_confirmed = state.step_ups.is_confirmed(user.id, crate::db::now());
    ProfileTemplate {
        google_confirmed,
        google_open: String::new(),
        google_error: None,
        delete_sent: None,
        display_name: profile::display_name(&p.first_name, &p.last_name),
        public_name: profile::public_name(&p.first_name, &p.last_name),
        member_since: profile::member_since(p.created_at),
        google_enabled: state.google.is_some(),
        user: Some(user),
        profile: p,
        notice: None,
        password_error: None,
        keys_on_file: api_keys.iter().any(KeyStatus::is_set),
        two_factor_error: None,
        totp_open: false,
        totp_error: None,
        totp_key: None,
        confirm_open: false,
        confirm_method: String::new(),
        confirm_remove: false,
        confirm_error: None,
        confirm_sent: None,
        phone_open: false,
        phone_sent: false,
        phone_error: None,
        export: None,
        export_error: None,
        caps: state.caps(),
        api_keys,
        subscription,
        plans: billing::plans_on(state),
        plans_open: false,
        cancel_reasons: &billing::CANCEL_REASONS,
        ai,
        dev: state.dev,
        delete_error: None,
        terms_html: terms_html(state),
    }
}

/// The terms for the dialog, without their `# Terms of Service` heading (the dialog has its own).
fn terms_html(state: &AppState) -> String {
    let md = crate::terms::markdown(state.stripe.is_some());
    let body = match md.strip_prefix("# ") {
        Some(rest) => rest.split_once('\n').map_or("", |(_, b)| b),
        None => &md,
    };
    super::docs::render_doc(body)
}

/// GET /profile
pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Query(q): Query<ShowQuery>,
) -> Result<Response, AppError> {
    let p = load_profile(&state, &user).await?;
    let mut t = page(&state, user, p);
    let provider = q.provider.as_deref().and_then(Provider::parse);
    t.notice = match (q.saved.as_deref(), provider) {
        (Some("password"), _) => Some(crate::t("profile.password.changed")),
        (Some("google-joined"), _) => Some(crate::t("profile.google_link.joined_notice")),
        (Some("verify-sent"), _) if t.user.as_ref().is_some_and(|u| u.needs_email_confirmation()) => Some(crate::t1("profile.save.sent_to", "email", t.profile.email.as_deref().unwrap_or_default())),
        (Some("key-removed"), Some(p)) => Some(crate::t1("profile.api_keys.removed", "provider", p.label())),
        (Some("ai-off"), _) => Some(crate::t("profile.ai.hidden_notice")),
        (Some("ai-on"), _) => Some(crate::t("profile.ai.shown_notice")),
        (Some("two-factor"), _) => Some(match t.profile.two_factor.method {
            None => crate::t("profile.two_factor.is_off"),
            Some(m) => crate::t1("profile.two_factor.is_method", "method", method_label(m)),
        }),
        (Some("totp-removed"), _) => Some(crate::t("profile.two_factor.totp_removed")),
        (Some("phone-verified"), _) => Some(crate::t("profile.two_factor.phone_verified")),
        // No mail on this server: no link is coming; the Download button is (#633).
        (Some("export-queued"), _) if !t.caps.mail => Some(crate::t("profile.data.export_building_no_mail")),
        (Some("export-queued"), _) => Some(crate::t("profile.data.export_queued")),
        (Some("signed-out"), _) => Some(crate::t("profile.sessions.signed_out_notice")),
        (Some("google-linked"), _) => Some(crate::t("profile.google_link.linked_notice")),
        (Some("google-unlinked"), _) => Some(crate::t("profile.google_link.unlinked_notice")),
        (Some("plan"), _) if t.subscription.premium => Some(crate::t1("profile.subscription.plan_thanks", "plan", t.subscription.plan.label())),
        (Some("cancelled"), _) => Some(crate::t1("profile.subscription.cancelled_notice", "note", t.subscription.cancel_note())),
        (Some("own-keys-on"), _) => Some(crate::t1("profile.api_keys.own_keys_on", "hosted", crate::tutor::hosted_label())),
        (Some("own-keys-off"), _) => Some(crate::t1("profile.api_keys.own_keys_off", "hosted", crate::tutor::hosted_label())),
        _ if q.ai.as_deref() == Some("off") => Some(crate::t("profile.ai.feature_hidden")),
        // The webhook, not this redirect, makes the subscription count; it
        // usually lands before the page does, but may not.
        _ if q.billing.as_deref() == Some("done") => Some(if t.subscription.premium {
            crate::t("profile.subscription.thanks_active")
        } else {
            crate::t("profile.subscription.thanks_pending")
        }),
        _ if q.billing.as_deref() == Some("cancelled") => Some(crate::t("profile.subscription.cancelled")),
        // Credits arrive with Stripe's webhook, like the subscription.
        _ if q.credits.as_deref() == Some("done") => Some(crate::t("profile.subscription.credits_thanks")),
        _ if q.credits.as_deref() == Some("cancelled") => Some(crate::t("profile.subscription.cancelled")),
        _ if t.plans && q.upgrade.is_some() && !t.subscription.premium => Some(crate::t("profile.subscription.required")),
        _ => None,
    };
    t.plans_open = t.plans && (q.plans.is_some() || (q.upgrade.is_some() && !t.subscription.premium));
    if q.totp.as_deref() == Some("enrol") && t.profile.two_factor.totp_pending {
        t.totp_key = pending_totp_key(&state, user_id(&t)).await?;
        t.totp_open = t.totp_key.is_some();
    }
    // Back from Google with a two-factor change waiting: its dialog again, ready to confirm.
    if let (Some("google"), Some(m), true) = (q.confirmed.as_deref(), q.method.as_deref(), t.google_confirmed) {
        if parse_method(m).is_ok() {
            t.confirm_open = true;
            t.confirm_method = m.to_string();
        }
    }
    let uid = user_id(&t);
    t.export = state.db.call(move |c| crate::exports::status(c, uid, crate::db::now())).await?;
    t.export_error = match q.saved.as_deref() {
        Some("export-busy") => Some(crate::t("profile.data.export_busy")),
        Some("export-soon") => Some(crate::t("profile.data.export_soon")),
        Some("export-gone") => Some(crate::t("profile.data.export_gone")),
        _ => None,
    };
    t.phone_sent = q.phone.as_deref() == Some("sent");
    // `?phone=sent` comes only after a code went out: to an unconfirmed
    // number, or a confirmed one checking it takes texts again (#969),
    // whose flag may already be cleared by the text getting through.
    t.phone_open = t.phone_sent;
    Ok(Page(t).into_response())
}

/// A learner's badge as a page draws it in script (`courses.js`,
/// `profile.js`): the picture (its theme version's URL, else the built-in
/// symbol's id) over the background of `slot`, under `theme`.
pub(super) fn badge_json(avatar: &str, slot: u8, theme: Option<&crate::content::Theme>) -> serde_json::Value {
    let a = profile::avatar(avatar);
    let bg = profile::badge(slot, theme);
    let url = theme.and_then(|t| Some(super::art_url(&t.id, &t.avatar(a.id)?.image)));
    serde_json::json!({ "id": a.id, "url": url, "color": bg.color, "image": bg.image })
}

/// The learner's picture and every other one, as a page draws people with
/// them in script (the verb drill's person slots, #879): each is the theme
/// worn's version (`url`) or the built-in symbol (`id`), with the `style` of
/// its `.avatar`. The others never include the learner's own picture.
/// Empty while character art is off (#1153): the slots keep the course's own
/// pictures.
pub(crate) fn people_json(user: &CurrentUser) -> String {
    if !user.character_art {
        return String::new();
    }
    let theme = user.theme_pack.as_ref().map(|p| &p.theme);
    let me = profile::avatar(&user.avatar).id;
    let others: Vec<serde_json::Value> = avatar_choices(theme)
        .into_iter()
        .filter(|a| a.id != me)
        .map(|a| serde_json::json!({ "id": a.id, "url": a.url, "style": format!("--av:{}", a.color) }))
        .collect();
    let mine = serde_json::json!({ "id": me, "url": user.theme_avatar_url, "style": user.avatar_style() });
    serde_json::json!({ "me": mine, "others": others }).to_string()
}

/// The pictures and backgrounds a theme offers, for a course's settings to
/// redraw when another theme is picked (#565): each picture's version, label
/// and own colour, and the palette (`profile::palette`).
pub(super) fn look_json(theme: Option<&crate::content::Theme>) -> serde_json::Value {
    let pictures: serde_json::Map<String, serde_json::Value> = avatar_choices(theme)
        .into_iter()
        .map(|a| (a.id.to_string(), serde_json::json!({ "url": a.url, "label": a.label, "color": a.color })))
        .collect();
    serde_json::json!({ "pictures": pictures, "palette": profile::palette(theme) })
}

/// What the page autosaves (`static/autosave.js`): any of the fields, as
/// they change. Left out, a field keeps its stored value.
#[derive(Deserialize)]
pub struct ProfileChanges {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    /// The phone number, international format; '' removes it.
    pub phone: Option<String>,
    /// The language the app speaks to this account in (`i18n::languages`).
    pub lang: Option<String>,
    /// AI API keys, as `key_<provider>` (`key_anthropic`, …).
    #[serde(flatten)]
    pub keys: std::collections::HashMap<String, serde_json::Value>,
}

/// POST /api/profile: save the fields sent. All or nothing: a field that
/// does not validate comes back as `422 {"errors": {field: message}}` and
/// nothing is stored. Saved, the answer has what the page redraws: the
/// name, and with a key its last four characters. (The theme and picture
/// are a course's settings, #565 and #577.)
pub async fn save(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Json(changes): Json<ProfileChanges>,
) -> Result<Response, AppError> {
    let mut errors = serde_json::Map::new();
    let mut check = |field: &str, r: Result<String, String>| match r {
        Ok(v) => Some(v),
        Err(e) => {
            errors.insert(field.into(), e.into());
            None
        }
    };
    let first = changes.first_name.as_deref().and_then(|v| check("first_name", profile::validate_name(&crate::t("profile.personal.first_name"), v)));
    let last = changes.last_name.as_deref().and_then(|v| check("last_name", profile::validate_name(&crate::t("profile.personal.last_name"), v)));
    let mut keys = Vec::new();
    for (field, value) in &changes.keys {
        let provider = field.strip_prefix("key_").and_then(Provider::parse);
        let r = match (provider, value.as_str()) {
            (Some(p), Some(v)) => secrets::validate_key(p, v).map(|k| keys.push((p, k))),
            _ => Err(crate::t("profile.save.not_a_field")),
        };
        check(field, r.map(|()| String::new()));
    }
    let phone = match changes.phone.as_deref().map(str::trim) {
        None => None,
        // No text messages on this server: no phone number either (#633).
        Some(_) if !state.caps().sms => check("phone", Err(crate::t("profile.save.sms_off"))),
        Some("") => Some(String::new()),
        Some(v) => check(
            "phone",
            crate::sms::normalize_us_number(v).map_err(|e| crate::t1("profile.save.not_a_phone", "why", e)).and_then(|n| {
                if state.sms.policy.allows(&n) {
                    Ok(n)
                } else {
                    Err(crate::t("profile.save.sms_country"))
                }
            }),
        ),
    };
    let lang = changes.lang.as_deref().map(str::trim).and_then(|v| {
        let ok = if crate::i18n::is_language(v) { Ok(v.to_string()) } else { Err(crate::t("profile.personal.language_unavailable")) };
        check("lang", ok)
    });
    if let Some(p) = phone.clone() {
        let uid = user.id;
        if let Some(why) = state.db.call(move |c| crate::two_factor::phone_change_refusal(c, uid, &p)).await? {
            errors.insert("phone".into(), crate::t(why).into());
        }
    }
    if user.ai_off {
        for field in changes.keys.keys() {
            errors.insert(field.clone(), crate::t("profile.ai.keys_off").into());
        }
    }
    if !errors.is_empty() {
        let body = serde_json::json!({ "errors": errors });
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response());
    }

    let (uid, vault) = (user.id, state.vault.clone());
    let phone_saved = phone.clone();
    let saved_keys: Vec<Provider> = keys.iter().map(|(p, _)| *p).collect();
    let (p, statuses) = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            profile::update(&tx, uid, first.as_deref(), last.as_deref())?;
            if let Some(p) = &phone_saved {
                crate::two_factor::set_phone(&tx, uid, p)?;
            }
            if let Some(l) = &lang {
                profile::set_lang(&tx, uid, l)?;
            }
            for (provider, key) in &keys {
                secrets::save(&tx, &vault, uid, *provider, key)?;
            }
            let p = profile::load(&tx, uid)?;
            let statuses = secrets::statuses(&tx, &vault, uid)?;
            tx.commit()?;
            Ok((p, statuses))
        })
        .await?;
    let p = p.ok_or(AppError::Unauthorized)?;
    tracing::info!("user {uid} updated their profile");

    let mut body = serde_json::json!({ "display_name": profile::display_name(&p.first_name, &p.last_name) });
    if phone.is_some() {
        body["phone"] = serde_json::json!({ "number": p.two_factor.phone, "verified": p.two_factor.phone_verified, "opted_out": p.two_factor.phone_opted_out });
    }
    if !saved_keys.is_empty() {
        let hints: serde_json::Map<String, serde_json::Value> = statuses
            .iter()
            .filter(|k| saved_keys.contains(&k.provider))
            .map(|k| (k.provider.id().to_string(), k.hint.clone().unwrap_or_default().into()))
            .collect();
        body["keys"] = hints.into();
    }
    Ok(Json(body).into_response())
}

/// POST /profile/verify-email/resend: a new confirmation link for the
/// account's address. Past [`crate::security::VERIFY_RESEND_PER_USER`] (in
/// memory, per account) or the address's daily share of verification mail
/// ([`crate::mail::PER_ADDRESS`], persisted, whichever account asks; #395) it
/// sends nothing but says the same, so the form cannot be used to flood an inbox.
pub async fn resend_verification(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    // No mail on this server: there is no link to send again (#633).
    if !state.caps().mail {
        return Ok(Redirect::to(&crate::url("/profile")).into_response());
    }
    if user.needs_email_confirmation() {
        if state.limits.hit(crate::security::VERIFY_RESEND_PER_USER, &user.id.to_string(), crate::db::now()) {
            crate::verify::send(&state, &headers, user.id, &user.email).await?;
            tracing::info!("user {} asked for a new verification e-mail", user.id);
        } else {
            tracing::warn!("verification e-mail rate limit hit for user {}", user.id);
        }
    }
    Ok(Redirect::to(&crate::url("/profile?saved=verify-sent")).into_response())
}

// ---------- two-factor sign-in and the phone number (two_factor.rs) ----------

fn method_label(m: crate::two_factor::Method) -> String {
    match m {
        crate::two_factor::Method::Sms => crate::t("profile.two_factor.method_sms"),
        crate::two_factor::Method::Email => crate::t("profile.two_factor.method_email"),
        crate::two_factor::Method::Totp => crate::t("profile.two_factor.method_totp"),
    }
}

/// The profile page again, with something to say (a refused code, …).
async fn profile_with(state: &AppState, user: CurrentUser, status: StatusCode, f: impl FnOnce(&mut ProfileTemplate)) -> Result<Response, AppError> {
    let p = load_profile(state, &user).await?;
    let mut t = page(state, user, p);
    f(&mut t);
    Ok((status, Page(t)).into_response())
}

fn user_id(t: &ProfileTemplate) -> i64 {
    t.user.as_ref().map(|u| u.id).unwrap_or_default()
}

/// The authenticator app being set up, decrypted for its dialog.
async fn pending_totp_key(state: &AppState, uid: i64) -> Result<Option<crate::totp::Key>, AppError> {
    let vault = state.vault.clone();
    Ok(state.db.call(move |c| crate::two_factor::pending_key(c, &vault, uid)).await?)
}

#[derive(Deserialize)]
pub struct MethodForm {
    /// `''` (off), `sms`, `email` or `totp`.
    #[serde(default)]
    pub method: String,
    /// Turning two-factor off or switching it: the account's password, or
    /// (a Google-only account) a code from the current second step.
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub code: String,
}

fn parse_method(m: &str) -> Result<Option<crate::two_factor::Method>, AppError> {
    match m {
        "" | "off" => Ok(None),
        m => Ok(Some(crate::two_factor::Method::parse(m).ok_or_else(|| AppError::BadRequest("unknown method".into()))?)),
    }
}

/// Is it the account's owner, not just someone with the session? The
/// password, checked like `/login` (argon2 off the async workers, limited
/// per account, a wrong one counting against sign-in, refused while sign-in
/// is blocked); a Google-only account has none, so a code from its current
/// second step instead, or without one a fresh "Confirm with Google"
/// (`StepUps`, #376). Every account change that a stolen session must not
/// make goes through here. `Some((status, message))` refuses.
async fn prove_owner(state: &AppState, user: &CurrentUser, password: &str, code: &str) -> Result<Option<(StatusCode, String)>, AppError> {
    use crate::security::{LOGIN_FAILURES_PER_USER, PASSWORD_PER_USER};
    let (uid, now) = (user.id, crate::db::now());
    if !state.limits.hit(PASSWORD_PER_USER, &uid.to_string(), now) || state.limits.blocked(LOGIN_FAILURES_PER_USER, &user.email, now) {
        tracing::warn!("account change rate limit hit for user {uid}");
        return Ok(Some((StatusCode::TOO_MANY_REQUESTS, crate::t("profile.two_factor.too_many_attempts"))));
    }
    let hash = state.db.call(move |c| profile::password_hash(c, uid)).await?.ok_or(AppError::RedirectToLogin)?;
    if !hash.is_empty() {
        if password.is_empty() {
            return Ok(Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.type_password"))));
        }
        let ok = !auth::password_too_long(password) && {
            let password = password.to_string();
            tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash)).await.map_err(|e| AppError::Internal(e.to_string()))?
        };
        if !ok {
            let _ = state.limits.hit(LOGIN_FAILURES_PER_USER, &user.email, now);
            tracing::info!("user {uid} gave a wrong password to change their account");
            return Ok(Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.password_wrong"))));
        }
        return Ok(None);
    }
    if state.db.call(move |c| crate::two_factor::settings(c, uid)).await?.method.is_none() {
        // No password and no second step: Google itself vouches, once.
        if state.step_ups.spend(uid, now) {
            return Ok(None);
        }
        return Ok(Some((StatusCode::FORBIDDEN, crate::t("profile.google_link.confirm_needed"))));
    }
    if code.trim().is_empty() {
        return Ok(Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.type_code"))));
    }
    let (code, vault) = (code.to_string(), state.vault.clone());
    let checked = state.db.call(move |c| crate::two_factor::check_confirm_code(c, &vault, uid, &code, now)).await?;
    use crate::two_factor::Checked;
    Ok(match checked {
        Checked::Ok => None,
        Checked::Wrong(left) if left < crate::two_factor::MAX_ATTEMPTS => {
            let _ = state.limits.hit(LOGIN_FAILURES_PER_USER, &user.email, now);
            Some((StatusCode::FORBIDDEN, crate::tn("profile.two_factor.code_wrong_tries", left)))
        }
        Checked::Wrong(_) => {
            let _ = state.limits.hit(LOGIN_FAILURES_PER_USER, &user.email, now);
            Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.code_wrong_newest")))
        }
        Checked::Gone => Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.code_gone"))),
    })
}

/// The profile again with the confirm dialog open.
async fn confirm_again(state: &AppState, user: CurrentUser, status: StatusCode, method: String, remove: bool, error: Option<String>, sent: Option<String>) -> Result<Response, AppError> {
    profile_with(state, user, status, move |t| {
        t.confirm_open = true;
        t.confirm_method = method;
        t.confirm_remove = remove;
        t.confirm_error = error;
        t.confirm_sent = sent;
    })
    .await
}

/// After two-factor sign-in changed (#374, #375): every other session
/// ends, and the account's address is told (a failed mail is logged, not
/// shown: the change is made).
async fn two_factor_changed(state: &AppState, user: &CurrentUser, keep: Option<String>, method: Option<crate::two_factor::Method>) -> Result<(), AppError> {
    let (uid, keep) = (user.id, keep.unwrap_or_default());
    let ended = state.db.call(move |c| auth::delete_other_sessions(c, uid, &keep)).await?;
    tracing::info!("user {uid}: two-factor sign-in changed, {ended} other sessions signed out");
    if !user.email.is_empty() {
        let queued = match crate::two_factor::changed_mail(&user.email, method, uid) {
            Ok(mail) => state.mail.queue(&state.db, mail).await.map(|_| ()).map_err(|e| format!("{e:?}")),
            Err(e) => Err(e),
        };
        if let Err(e) = queued {
            tracing::warn!("could not tell user {uid} that two-factor sign-in changed: {e}");
        }
    }
    Ok(())
}

/// POST /profile/two-factor (method, password | code): the second sign-in
/// step, one at a time. Every change, turning it on too, needs proof that
/// it is the owner (`prove_owner`, #374), then signs out the other devices
/// and e-mails the account. The authenticator app, until one is set up,
/// starts its enrolment instead (the dialog with its key opens).
pub async fn set_two_factor(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(f): Form<MethodForm>,
) -> Result<Response, AppError> {
    use crate::two_factor::{self, Method};
    let method = parse_method(&f.method)?;
    let uid = user.id;
    let current = state.db.call(move |c| two_factor::settings(c, uid)).await?;
    // A method whose channel this server lacks is not offered (#633).
    if let Some(m) = method.filter(|m| !two_factor::channel_on(*m, state.caps())) {
        return profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| t.two_factor_error = Some(two_factor::channel_off_text(m))).await;
    }
    // A method that can't be chosen yet is refused before asking for anything.
    if let Some(why) = method.filter(|m| *m != Method::Totp).and_then(|m| current.refusal(m)) {
        return profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| t.two_factor_error = Some(why.into())).await;
    }
    if !two_factor::needs_proof(current.method, method) {
        return Ok(Redirect::to(&crate::url("/profile?saved=two-factor#two-factor")).into_response());
    }
    if let Some((status, why)) = prove_owner(&state, &user, &f.password, &f.code).await? {
        return confirm_again(&state, user, status, f.method, false, Some(why), None).await;
    }
    if method == Some(Method::Totp) && current.totp_since.is_none() {
        let vault = state.vault.clone();
        state.db.call(move |c| two_factor::totp_start(c, &vault, uid, crate::db::now())).await?.map_err(|e| AppError::Internal(e.into()))?;
        tracing::info!("user {uid} started setting up an authenticator app");
        return Ok(Redirect::to(&crate::url("/profile?totp=enrol#two-factor")).into_response());
    }
    match state.db.call(move |c| two_factor::set_method(c, uid, method)).await? {
        Ok(()) => {
            tracing::info!("user {uid} set two-factor sign-in to {}", method.map(Method::as_str).unwrap_or("off"));
            two_factor_changed(&state, &user, token, method).await?;
            Ok(Redirect::to(&crate::url("/profile?saved=two-factor#two-factor")).into_response())
        }
        Err(why) => profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| t.two_factor_error = Some(why.into())).await,
    }
}

#[derive(Deserialize)]
pub struct ConfirmCodeForm {
    #[serde(default)]
    pub method: String,
    /// `1`: the dialog was opened by the app's Remove.
    #[serde(default)]
    pub remove: String,
    /// `delete`: the code is for deleting the account (#376).
    #[serde(default)]
    pub then: String,
}

/// The page after a confirm code was asked for: the two-factor dialog
/// open again, or the delete form with where the code went.
async fn code_sent_page(state: &AppState, user: CurrentUser, f: ConfirmCodeForm, status: StatusCode, error: Option<String>, sent: Option<String>) -> Result<Response, AppError> {
    if f.then == "delete" {
        return profile_with(state, user, status, move |t| {
            t.delete_error = error;
            t.delete_sent = sent;
        })
        .await;
    }
    confirm_again(state, user, status, f.method, f.remove == "1", error, sent).await
}

/// POST /profile/two-factor/code (method, remove | then=delete): a
/// Google-only account (no password) asks for a code from its current
/// texted or e-mailed second step, to change two-factor sign-in or delete
/// the account. Limited like a sign-in's "Send a new code"; the page comes
/// back with the dialog (or the delete form) waiting for it.
pub async fn send_confirm_code(State(state): State<Arc<AppState>>, user: CurrentUser, Form(f): Form<ConfirmCodeForm>) -> Result<Response, AppError> {
    use crate::two_factor::{self, Method};
    parse_method(&f.method)?;
    let (uid, now) = (user.id, crate::db::now());
    if !state.limits.hit(crate::security::TWO_FACTOR_SEND_PER_USER, &uid.to_string(), now) {
        tracing::warn!("two-factor confirm code limit hit for user {uid}");
        return code_sent_page(&state, user, f, StatusCode::TOO_MANY_REQUESTS, Some(crate::t("profile.two_factor.too_many_codes")), None).await;
    }
    let Some((method, to, code)) = state.db.call(move |c| two_factor::issue_confirm_code(c, uid, now)).await? else {
        return Ok(Redirect::to(&crate::url("/profile#two-factor")).into_response());
    };
    let sent = match method {
        Method::Sms => state.sms.send_sms(&state.db, two_factor::PURPOSE_CONFIRM, &to, &two_factor::confirm_text(&code), Some(uid)).await.map(|_| ()).map_err(|e| e.to_string()),
        Method::Email => match two_factor::confirm_mail(&to, &code, uid) {
            Ok(mail) => state.mail.queue(&state.db, mail).await.map(|_| ()).map_err(|e| format!("{e:?}")),
            Err(e) => Err(e),
        },
        Method::Totp => Ok(()),
    };
    if let Err(e) = sent {
        tracing::warn!("could not send user {uid} a two-factor confirm code: {e}");
        return code_sent_page(&state, user, f, StatusCode::SERVICE_UNAVAILABLE, Some(crate::t("profile.two_factor.code_not_sent")), None).await;
    }
    tracing::info!("user {uid} asked for a code to confirm an account change");
    let (email, phone) = if method == Method::Email { (to, String::new()) } else { (String::new(), to) };
    let sent = two_factor::masked(method, &email, &phone);
    code_sent_page(&state, user, f, StatusCode::OK, None, Some(sent)).await
}

/// POST /profile/two-factor/totp/confirm (code): the app's first code
/// finishes the enrolment (which `prove_owner` let start) and makes it the
/// second step; the other devices are signed out and the account told.
pub async fn confirm_totp(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(f): Form<CodeForm>,
) -> Result<Response, AppError> {
    let uid = user.id;
    if !state.limits.hit(crate::security::PASSWORD_PER_USER, &uid.to_string(), crate::db::now()) {
        let key = pending_totp_key(&state, uid).await?;
        return profile_with(&state, user, StatusCode::TOO_MANY_REQUESTS, |t| {
            t.totp_open = key.is_some();
            t.totp_key = key;
            t.totp_error = Some(crate::t("profile.two_factor.too_many_attempts"));
        })
        .await;
    }
    let (code, vault) = (f.code, state.vault.clone());
    if state.db.call(move |c| crate::two_factor::totp_confirm(c, &vault, uid, &code, crate::db::now())).await? {
        tracing::info!("user {uid} set up an authenticator app");
        two_factor_changed(&state, &user, token, Some(crate::two_factor::Method::Totp)).await?;
        return Ok(Redirect::to(&crate::url("/profile?saved=two-factor#two-factor")).into_response());
    }
    let key = pending_totp_key(&state, uid).await?;
    profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| {
        t.totp_open = key.is_some();
        t.totp_key = key;
        t.totp_error = Some(crate::t("profile.two_factor.code_wrong_newest"));
    })
    .await
}

#[derive(Deserialize)]
pub struct CodeForm {
    #[serde(default)]
    pub code: String,
}

/// POST /profile/two-factor/totp/remove (password | code): forget the
/// authenticator app. While it is the second step that turns two-factor
/// sign-in off, so it needs the password like choosing Off.
pub async fn remove_totp(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(f): Form<MethodForm>,
) -> Result<Response, AppError> {
    let uid = user.id;
    let current = state.db.call(move |c| crate::two_factor::settings(c, uid)).await?;
    let in_use = current.method == Some(crate::two_factor::Method::Totp);
    if in_use {
        if let Some((status, why)) = prove_owner(&state, &user, &f.password, &f.code).await? {
            return confirm_again(&state, user, status, String::new(), true, Some(why), None).await;
        }
    }
    state.db.call(move |c| crate::two_factor::totp_remove(c, uid)).await?;
    tracing::info!("user {uid} removed their authenticator app");
    if in_use {
        two_factor_changed(&state, &user, token, None).await?;
    }
    Ok(Redirect::to(&crate::url("/profile?saved=totp-removed#two-factor")).into_response())
}

/// POST /profile/phone/send: text a code to the profile's number to
/// confirm it (limited per account, and by the texts' own limits). Only for
/// an account with a confirmed address or Google (#431), and never to a
/// number another account has verified (#363).
pub async fn send_phone_code(State(state): State<Arc<AppState>>, user: CurrentUser) -> Result<Response, AppError> {
    // No text messages on this server: the phone field is not offered (#633).
    if !state.caps().sms {
        return Err(AppError::NotFound);
    }
    let uid = user.id;
    let now = crate::db::now();
    let fail = |msg: String| (StatusCode::TOO_MANY_REQUESTS, move |t: &mut ProfileTemplate| {
        t.phone_open = true;
        t.phone_error = Some(msg);
    });
    if !state.limits.hit(crate::security::TWO_FACTOR_SEND_PER_USER, &uid.to_string(), now) {
        tracing::warn!("phone code limit hit for user {uid}");
        let (status, f) = fail(crate::t("profile.two_factor.too_many_codes"));
        return profile_with(&state, user, status, f).await;
    }
    let (phone, code) = match state.db.call(move |c| crate::two_factor::issue_phone_code(c, uid, now)).await? {
        crate::two_factor::PhoneCode::Send { phone, code } => (phone, code),
        crate::two_factor::PhoneCode::Nothing => return Ok(Redirect::to(&crate::url("/profile#personal")).into_response()),
        crate::two_factor::PhoneCode::Refused(why) => {
            tracing::warn!("phone code refused for user {uid}: {why}");
            return profile_with(&state, user, StatusCode::FORBIDDEN, |t| {
                t.phone_open = true;
                t.phone_error = Some(crate::t(why));
            })
            .await;
        }
    };
    match state.sms.send_sms(&state.db, crate::two_factor::PURPOSE_PHONE, &phone, &crate::two_factor::phone_text(&code), Some(uid)).await {
        Ok(_) => {
            tracing::info!("user {uid} asked for a phone code");
            Ok(Redirect::to(&crate::url("/profile?phone=sent#personal")).into_response())
        }
        Err(crate::sms::SmsError::RateLimited { retry_after }) => {
            let (status, f) = fail(crate::t1("profile.two_factor.too_many_texts", "minutes", (retry_after + 59) / 60));
            profile_with(&state, user, status, f).await
        }
        Err(crate::sms::SmsError::Invalid(why)) => profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| {
            t.phone_open = true;
            t.phone_error = Some(crate::t1("profile.two_factor.number_no_texts", "why", why));
        })
        .await,
        Err(crate::sms::SmsError::Country) => profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| {
            t.phone_open = true;
            t.phone_error = Some(crate::t("profile.save.sms_country"));
        })
        .await,
        Err(crate::sms::SmsError::Db(e)) => Err(e),
    }
}

/// POST /profile/phone/verify (code): the texted code confirms the number.
pub async fn verify_phone(State(state): State<Arc<AppState>>, user: CurrentUser, Form(f): Form<CodeForm>) -> Result<Response, AppError> {
    use crate::two_factor::Checked;
    let uid = user.id;
    let code = f.code;
    let checked = state.db.call(move |c| crate::two_factor::confirm_phone(c, uid, &code, crate::db::now())).await?;
    let msg = match checked {
        Checked::Ok => {
            tracing::info!("user {uid} verified their phone number");
            return Ok(Redirect::to(&crate::url("/profile?saved=phone-verified#personal")).into_response());
        }
        Checked::Wrong(left) => crate::tn("profile.two_factor.code_wrong_tries", left),
        Checked::Gone => {
            let taken = state
                .db
                .call(move |c| {
                    let phone: String = c.query_row("SELECT phone FROM users WHERE id = ?1", [uid], |r| r.get(0))?;
                    crate::two_factor::phone_taken(c, uid, &phone)
                })
                .await?;
            crate::t(if taken { crate::two_factor::PHONE_TAKEN } else { "profile.two_factor.phone_code_expired" })
        }
    };
    profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| {
        t.phone_open = true;
        t.phone_sent = true;
        t.phone_error = Some(msg);
    })
    .await
}

/// POST /profile/password
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(form): Form<PasswordForm>,
) -> Result<Response, AppError> {
    let (p, keys, sub, ai) = load_profile(&state, &user).await?;
    let fail = |state: &AppState, user: CurrentUser, p: Profile, msg: &str| {
        let mut t = page(state, user, (p, keys.clone(), sub.clone(), ai.clone()));
        t.password_error = Some(msg.to_string());
        Page(t).into_response()
    };
    if !p.has_password {
        return Ok(fail(&state, user, p, &crate::t("profile.password.google_only")));
    }
    if form.new != form.confirm {
        return Ok(fail(&state, user, p, &crate::t("profile.password.mismatch")));
    }
    if let Err(e) = auth::validate_password(&form.new) {
        return Ok(fail(&state, user, p, &e));
    }
    // A stolen session must not become a fast way to guess the current
    // password: checked like every proof of ownership, on the sign-in's
    // per-account counter too (#378).
    if let Some((status, why)) = prove_owner(&state, &user, &form.current, "").await? {
        // A wrong password is the form again with its error (200, as ever); only the limit is a 429.
        let (status, msg) = if status == StatusCode::FORBIDDEN { (StatusCode::OK, crate::t("profile.password.current_wrong")) } else { (status, why) };
        let page = fail(&state, user, p, &msg);
        return Ok((status, page).into_response());
    }

    let uid = user.id;
    let new = form.new;
    let new_hash = tokio::task::spawn_blocking(move || auth::hash_password(&new))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))??;
    // Every other session (another device, or whoever had the old password) ends.
    let ended = state
        .db
        .call(move |c| {
            profile::set_password_hash(c, uid, &new_hash)?;
            auth::delete_other_sessions(c, uid, token.as_deref().unwrap_or(""))
        })
        .await?;
    tracing::info!("user {} changed their password ({ended} other sessions signed out)", user.id);
    Ok(Redirect::to(&crate::url("/profile?saved=password")).into_response())
}

/// POST /profile/sessions/end-others: "Sign out everywhere else" (#375).
/// Every other session of the account ends; this one stays. Needs no
/// proof: it only takes access away.
pub async fn end_other_sessions(State(state): State<Arc<AppState>>, user: CurrentUser, auth::SessionToken(token): auth::SessionToken) -> Result<Response, AppError> {
    let (uid, keep) = (user.id, token.unwrap_or_default());
    let ended = state.db.call(move |c| auth::delete_other_sessions(c, uid, &keep)).await?;
    tracing::info!("user {uid} signed out everywhere else ({ended} sessions)");
    Ok(Redirect::to(&crate::url("/profile?saved=signed-out#account")).into_response())
}

#[derive(Deserialize)]
pub struct ProofForm {
    #[serde(default)]
    pub password: String,
}

/// The profile again with the Google link or unlink dialog open, saying why.
async fn google_again(state: &AppState, user: CurrentUser, status: StatusCode, open: &str, why: String) -> Result<Response, AppError> {
    let open = open.to_string();
    profile_with(state, user, status, move |t| {
        t.google_open = open;
        t.google_error = Some(why);
    })
    .await
}

/// POST /profile/google/link (password): connect a Google account (#360).
/// The password first (`prove_owner`), then Google; the callback links it
/// only for the account that started (`StepUps`) and signs out the other
/// devices. A Google-only account is already linked.
pub async fn link_google(State(state): State<Arc<AppState>>, user: CurrentUser, Form(f): Form<ProofForm>) -> Result<Response, AppError> {
    state.google.as_ref().ok_or(AppError::NotFound)?;
    let uid = user.id;
    if state.db.call(move |c| auth::google_linked(c, uid)).await? {
        return Ok(Redirect::to(&crate::url("/profile#account")).into_response());
    }
    if let Some((status, why)) = prove_owner(&state, &user, &f.password, "").await? {
        return google_again(&state, user, status, "link", why).await;
    }
    tracing::info!("user {uid} is connecting a Google account");
    super::auth::to_google(&state, uid, crate::oauth::StepUp::Link, "/profile?saved=google-linked#account", None)
}

/// POST /profile/google/unlink (password): "Continue with Google" stops
/// signing in to this account (#360), and the other devices are signed
/// out. Only an account with a password: a Google-only one would be left
/// with no way in.
pub async fn unlink_google(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(f): Form<ProofForm>,
) -> Result<Response, AppError> {
    let uid = user.id;
    let has_password = !state.db.call(move |c| profile::password_hash(c, uid)).await?.unwrap_or_default().is_empty();
    if !has_password {
        return google_again(&state, user, StatusCode::UNPROCESSABLE_ENTITY, "unlink", crate::t("profile.google_link.stays_connected")).await;
    }
    if let Some((status, why)) = prove_owner(&state, &user, &f.password, "").await? {
        return google_again(&state, user, status, "unlink", why).await;
    }
    let keep = token.unwrap_or_default();
    let (unlinked, ended) = state
        .db
        .call(move |c| {
            let unlinked = auth::unlink_google(c, uid)?;
            let ended = if unlinked { auth::delete_other_sessions(c, uid, &keep)? } else { 0 };
            Ok((unlinked, ended))
        })
        .await?;
    if unlinked {
        tracing::info!("user {uid} unlinked their Google account ({ended} other sessions signed out)");
    }
    Ok(Redirect::to(&crate::url("/profile?saved=google-unlinked#account")).into_response())
}

#[derive(Deserialize)]
pub struct GoogleConfirmForm {
    /// `two-factor` (with the `method` being chosen) or `delete`.
    #[serde(default)]
    pub then: String,
    #[serde(default)]
    pub method: String,
}

/// POST /profile/google/confirm (then, method): a Google-only account with
/// no second step proves it is its owner by going through Google again
/// (#374, #376). Back on the profile, the change it was making goes ahead
/// once (`prove_owner` spends the confirmation).
pub async fn confirm_with_google(State(state): State<Arc<AppState>>, user: CurrentUser, Form(f): Form<GoogleConfirmForm>) -> Result<Response, AppError> {
    state.google.as_ref().ok_or(AppError::NotFound)?;
    let back = if f.then == "delete" {
        "/profile?confirmed=google#data".to_string()
    } else {
        parse_method(&f.method)?;
        format!("/profile?confirmed=google&method={}#two-factor", crate::oauth::urlencode(&f.method))
    };
    super::auth::to_google(&state, user.id, crate::oauth::StepUp::Confirm, &back, Some(&user.email))
}

/// POST /profile/ai (off=1|0): "Do Not Show Me AI Features" on or off. On, the
/// pages hide every feature that calls an AI provider, their routes refuse
/// (`CurrentUser::require_ai`) and every saved API key is deleted (the
/// profile's dialog warns about that when there is one).
pub async fn set_ai(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    headers: HeaderMap,
    Form(form): Form<AiForm>,
) -> Result<Response, AppError> {
    let off = match form.off.as_str() {
        "1" => true,
        "0" => false,
        _ => return Err(AppError::BadRequest("off must be 1 or 0".into())),
    };
    let uid = user.id;
    let (keys, consent) = state
        .db
        .call(move |c| {
            profile::set_ai_off(c, uid, off)?;
            let keys = if off { secrets::remove_all(c, uid)? } else { 0 };
            Ok((keys, crate::consent::latest(c, uid, crate::consent::AI)?))
        })
        .await?;
    if off {
        // A tutor conversation under way would still be calling the provider.
        state.tutors.drop_user(uid);
    }
    tracing::info!("user {uid} {} AI features ({keys} API keys deleted)", if off { "hid" } else { "showed" });
    // The page's switch (profile.js) asks for JSON and updates in place.
    let wants_json = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json"));
    if wants_json {
        let consent = profile::ai_consent_text(off, consent);
        return Ok(Json(serde_json::json!({ "ai_off": off, "keys_deleted": keys, "consent": consent })).into_response());
    }
    let saved = if off { "ai-off" } else { "ai-on" };
    Ok(Redirect::to(&crate::url(&format!("/profile?saved={saved}#consent"))).into_response())
}

/// POST /profile/own-keys {on=1|0}: "use my keys when my Skrongle AI limit
/// is hit" (#315). The page's pill (profile.js) asks for JSON and shows or
/// hides the key rows in place; without JS the form posts and reloads.
pub async fn set_own_keys(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    headers: HeaderMap,
    Form(form): Form<OwnKeysForm>,
) -> Result<Response, AppError> {
    let on = match form.on.as_str() {
        "1" => true,
        "0" => false,
        _ => return Err(AppError::BadRequest("on must be 1 or 0".into())),
    };
    let uid = user.id;
    state.db.call(move |c| profile::set_own_keys_fallback(c, uid, on)).await?;
    tracing::info!("user {uid} turned own-key fallback {}", if on { "on" } else { "off" });
    let wants_json = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json"));
    if wants_json {
        return Ok(Json(serde_json::json!({ "own_keys_fallback": on })).into_response());
    }
    Ok(Redirect::to(&crate::url(if on { "/profile?saved=own-keys-on#api-keys" } else { "/profile?saved=own-keys-off#api-keys" })).into_response())
}

/// POST /profile/api-keys/remove
pub async fn remove_api_key(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Form(form): Form<ApiKeyForm>,
) -> Result<Response, AppError> {
    let Some(provider) = Provider::parse(&form.provider) else {
        return Err(AppError::BadRequest("unknown provider".into()));
    };
    let uid = user.id;
    state.db.call(move |c| secrets::remove(c, uid, provider)).await?;
    tracing::info!("user {} removed their {} API key", user.id, provider);
    Ok(Redirect::to(&crate::url(&format!("/profile?saved=key-removed&provider={provider}"))).into_response())
}

/// POST /profile/export: queue "Download my data" (exports.rs). The file
/// is built in the background and a one-time link is e-mailed; one export
/// in flight per account, one a day.
pub async fn export(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap) -> Result<Response, AppError> {
    let uid = user.id;
    let base = crate::verify::public_base(&state, &headers);
    let r = state.db.call(move |c| crate::exports::request(c, uid, &base, crate::db::now())).await?;
    let saved = match r {
        crate::exports::Requested::Queued(id) => {
            state.queue.notify();
            tracing::info!("user {uid} asked for their data (export #{id})");
            "export-queued"
        }
        crate::exports::Requested::InFlight => "export-busy",
        crate::exports::Requested::TooSoon(_) => "export-soon",
    };
    Ok(Redirect::to(&crate::url(&format!("/profile?saved={saved}#data"))).into_response())
}

#[derive(Deserialize)]
pub struct ExportDownloadQuery {
    #[serde(default)]
    pub token: String,
}

/// A finished export's file as a response body, read on an OS thread of its
/// own a chunk at a time, so the file never sits in memory whole.
struct FileChunks(tokio::sync::mpsc::Receiver<std::io::Result<axum::body::Bytes>>);

impl futures_core::Stream for FileChunks {
    type Item = std::io::Result<axum::body::Bytes>;
    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// GET /profile/export/download?token=…: the mailed link. Only for the
/// account signed in, once, while it is fresh; the file is deleted as it is
/// sent. Any other case goes back to the profile, which says the link is
/// used or expired.
pub async fn export_download(State(state): State<Arc<AppState>>, user: CurrentUser, Query(q): Query<ExportDownloadQuery>) -> Result<Response, AppError> {
    let uid = user.id;
    let token = q.token;
    let file = state.db.call(move |c| crate::exports::take(c, uid, &token, crate::db::now())).await?;
    send_export(&state, uid, file).await
}

/// POST /profile/export/download: the Data section's Download button, shown
/// once the export is ready (#633: the way to it on a server without mail).
/// Like the mailed link: once, while it is fresh, for the account signed in.
pub async fn export_download_now(State(state): State<Arc<AppState>>, user: CurrentUser) -> Result<Response, AppError> {
    let uid = user.id;
    let file = state.db.call(move |c| crate::exports::take_ready(c, uid, crate::db::now())).await?;
    send_export(&state, uid, file).await
}

/// The export file `file` names (in the exports folder), sent and deleted.
/// `None` (or a file that is gone): back to the profile, which says so.
async fn send_export(state: &AppState, uid: i64, file: Option<String>) -> Result<Response, AppError> {
    use std::io::Read;
    let gone = || Redirect::to(&crate::url("/profile?saved=export-gone#data")).into_response();
    let Some(path) = file.and_then(|f| crate::exports::file_path(&state.exports.dir, &f)) else { return Ok(gone()) };
    let mut f = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("export for user {uid}: {} unreadable: {e}", path.display());
            return Ok(gone());
        }
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    // The open handle still reads it; the link is used up either way.
    if let Err(e) = std::fs::remove_file(&path) {
        tracing::warn!("export for user {uid}: could not delete {}: {e}", path.display());
    }
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    std::thread::Builder::new()
        .name("export-send".into())
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.blocking_send(Ok(axum::body::Bytes::copy_from_slice(&buf[..n]))).is_err() { // panic-ok: read returns n <= buf.len()
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.blocking_send(Err(e));
                        break;
                    }
                }
            }
        })
        .map_err(|e| AppError::Internal(e.to_string()))?;
    tracing::info!("user {uid} downloaded their data ({len} bytes)");
    let file = format!("attachment; filename=\"{}-my-data.json\"", crate::APP_NAME.to_lowercase());
    Ok((
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, file),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        axum::body::Body::from_stream(FileChunks(rx)),
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct DeleteForm {
    /// The account's password; Google-only accounts type their e-mail address instead.
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub confirm: String,
    /// A Google-only account with a second step: a code from it.
    #[serde(default)]
    pub code: String,
}

/// POST /profile/delete: schedule the account's deletion
/// (`account_deletion::schedule`, #970: 14 days out, signed out everywhere
/// until then), after proof that it is the owner (`prove_owner`: the
/// password; a Google-only account types its e-mail address and gives a
/// second-step code, or without one has just confirmed with Google, #376).
pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Form(form): Form<DeleteForm>,
) -> Result<Response, AppError> {
    let loaded = load_profile(&state, &user).await?;
    let fail = |state: &AppState, user: CurrentUser, loaded: Loaded, msg: &str| {
        let mut t = page(state, user, loaded);
        t.delete_error = Some(msg.to_string());
        Page(t).into_response()
    };
    let uid = user.id;
    if !loaded.0.has_password && !form.confirm.trim().eq_ignore_ascii_case(&loaded.0.delete_phrase()) {
        return Ok(fail(&state, user, loaded, &crate::t("profile.data.type_email")));
    }
    // Before the proof, so a code or a Google confirmation is not used up for nothing.
    if state.db.call(move |c| crate::privacy::has_live_subscription(c, uid)).await? {
        return Ok(fail(&state, user, loaded, &crate::t("profile.data.cancel_first")));
    }
    // The same proof as every account change, on the same allowances (#376, #378).
    if let Some((status, why)) = prove_owner(&state, &user, &form.password, &form.code).await? {
        // The form again with its error (200, as ever); only the limit is a 429.
        let status = if status == StatusCode::FORBIDDEN { StatusCode::OK } else { status };
        let page = fail(&state, user, loaded, &why);
        return Ok((status, page).into_response());
    }
    let mail = state.caps().mail;
    let at = state.db.call(move |c| crate::account_deletion::schedule(c, uid, crate::db::now(), mail)).await?.ok_or(AppError::RedirectToLogin)?;
    state.mail.notify();
    tracing::info!("user {uid} asked to delete their account: deleted at {at} unless they cancel");
    Ok(([auth::clear_session_cookie()], Redirect::to(&crate::url(&format!("/login?deletion={at}")))).into_response())
}

/// Delete the account (`privacy::delete_account`) at once and forget what
/// the server holds of it in memory and on disk: a superuser's delete
/// (#967). The learner's own delete waits 14 days (#970).
pub(crate) async fn erase_account(state: &AppState, uid: i64) -> Result<crate::privacy::Deleted, AppError> {
    // What lives in memory goes first, so nothing writes for the account afterwards.
    crate::account_deletion::forget_in_memory(state, uid);
    let deleted = state.db.call(move |c| crate::privacy::delete_account(c, uid)).await?.unwrap_or_default();
    crate::account_deletion::forget_deleted(state, &deleted);
    Ok(deleted)
}

#[cfg(test)]
mod tests;
