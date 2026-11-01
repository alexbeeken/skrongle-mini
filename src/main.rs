//! Lightweight self-hosted learning app (lessons, quizzes,
//! games and phone sessions) driven by JSON courses. Sized for a Raspberry
//! Pi Zero 2 W.

mod account_deletion;
mod activity;
mod algebra_tiles;
mod ai;
mod arrange;
mod auth;
mod billing;
mod breach;
mod check_worker;
mod clock;
mod conjugation;
mod consent;
mod content;
mod content_check;
mod course_flags;
mod courses;
mod db;
mod dev;
mod dev_ai;
mod dev_crowd;
mod dev_payments;
mod doctor;
mod earnings;
mod sprint;
mod error;
mod exports;
mod games;
mod guide;
mod i18n;
mod jobs;
mod lint;
mod listening;
mod mail;
mod mathcheck;
mod measure;
mod moderation;
mod notation;
mod notes;
mod mastery;
mod metering;
mod motivators;
mod oauth;
mod owner;
mod placement;
mod privacy;
mod privacy_requests;
mod problems;
mod profile;
mod progress;
mod proxy_auth;
mod quiz;
mod random;
mod report;
mod reset;
mod review;
mod routes;
mod secrets;
mod share;
mod shape;
mod signup;
mod startup;
mod storage;
mod security;
mod server;
mod sms;
mod terms;
mod totp;
mod translate;
mod two_factor;
#[cfg(test)]
mod test_course;
mod themes;
mod tutor;
mod verify;
mod waitlist;
mod vocab;
mod word_list;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use courses::Courses;
use db::Db;

pub use i18n::{t, t1, t2, tn};

/// Shown in the navigation bar and page titles.
pub const APP_NAME: &str = "Skrongle";

/// The release version: `SKRONGLE_VERSION` at build time (the release
/// workflow sets it from the tag), otherwise the Cargo package version.
pub const VERSION: &str = match option_env!("SKRONGLE_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// Defaults for the three settings every install has.
pub const DEFAULT_BIND: &str = "0.0.0.0:8080";
pub const DEFAULT_DATA_DIR: &str = "data";
pub const DEFAULT_DB: &str = "learn.db";

/// Skrongle mini (#641, #578): the same binary as the self-hosted version,
/// with the small-box defaults `deploy/skrongle.service` sets (invite-only
/// sign-up, no plans, the owner's own Terms). The server is a mini when
/// `SKRONGLE_MINI=1` (`shape.rs`, read once at startup); otherwise it is
/// the hosted site.
pub fn is_mini() -> bool {
    shape::current().is_mini()
}

/// What the server calls itself in the startup log and on `/healthz`:
/// `skrongle mini 0.1.0` or `skrongle 0.1.0` ([`VERSION`]).
pub fn edition(mini: bool) -> String {
    if mini { format!("skrongle mini {VERSION}") } else { format!("skrongle {VERSION}") }
}

static BASE_PATH: OnceLock<String> = OnceLock::new();

/// URL prefix the app is mounted under ("" or e.g. "/spanish"), without a trailing slash.
/// Set once at startup from `SKRONGLE_BASE_PATH`; used by templates, redirects and the router.
pub fn base_path() -> &'static str {
    BASE_PATH.get().map(String::as_str).unwrap_or("")
}

/// Build an app-absolute URL: `url("/login")` -> "/spanish/login" when mounted under /spanish.
/// The app root is `url("/")`, which is "/" unmounted and "/spanish" (no trailing slash) mounted.
pub fn url(path: &str) -> String {
    let base = base_path();
    if path == "/" && !base.is_empty() {
        base.to_string()
    } else {
        format!("{base}{path}")
    }
}

/// Cookie `Path` attribute: "/" unmounted, the prefix itself when mounted.
pub fn cookie_path() -> &'static str {
    let base = base_path();
    if base.is_empty() { "/" } else { base }
}

fn normalize_base_path(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// Shared, immutable application state.
pub struct AppState {
    pub db: Db,
    /// Built-in courses plus a cache of user-made ones.
    pub courses: Arc<Courses>,
    /// The themes under data/themes/, loaded at boot.
    pub themes: Arc<themes::Themes>,
    pub secure_cookies: bool,
    /// Hash verified against for unknown addresses, to keep login timing uniform.
    pub dummy_hash: String,
    /// In-flight timed game sessions (in memory only).
    pub games: games::Sessions,
    /// Running and recently finished course-generation jobs.
    pub jobs: ai::Jobs,
    /// "Continue with Google", when configured.
    pub google: Option<oauth::GoogleConfig>,
    /// New Google accounts waiting for the Terms confirm step.
    pub signups: oauth::Signups,
    /// Google joins waiting for the two-factor code (#677).
    pub google_joins: oauth::Joins,
    /// Google trips started from the profile (link, confirm it's you).
    pub step_ups: oauth::StepUps,
    /// Paid subscriptions through Stripe, when configured. Without it
    /// nobody can subscribe and only grants make a learner premium.
    pub stripe: Option<billing::StripeConfig>,
    /// Encrypts learners' AI API keys at rest.
    pub vault: secrets::Vault,
    /// AI tutoring conversations (in memory only).
    pub tutors: tutor::Sessions,
    /// Hosted AI (the server's key, when configured) and the meter every
    /// model call is recorded by.
    pub meter: Arc<metering::Meter>,
    /// `SKRONGLE_DEV=1` on a loopback bind: the `/dev/*` routes exist.
    pub dev: bool,
    /// Rate limits for sign-in, registration and password changes (in memory; off in dev mode).
    pub limits: security::RateLimits,
    /// The e-mail outbox's sender (`SKRONGLE_MAIL_*`; logs only when unset).
    pub mail: mail::Mailer,
    /// The text message outbox's sender (`SKRONGLE_SMS_*`; logs only when unset).
    pub sms: sms::Texter,
    /// "Download my data": the folder the export job writes to.
    pub exports: exports::Exports,
    /// Wakes the background job runner (`jobs.rs`).
    pub queue: jobs::Queue,
    /// Dev mode: the SMTP mailbox the mail goes to (`/dev/inbox`); `None`
    /// with `SKRONGLE_MAIL_PROVIDERS`, `SKRONGLE_DEV_SMTP=0` or outside dev mode.
    pub dev_inbox: Option<Arc<mail::sink::Inbox>>,
    /// Dev mode without `SKRONGLE_SMS_PROVIDERS`: where texts go (`/dev/phone`).
    pub dev_phone: Option<Arc<sms::phone::Phone>>,
    /// Dev mode: which rows of the dev inbox pages were opened or archived.
    pub dev_marks: dev::Marks,
    /// Dev mode: the simulated AI provider's log, usage and settings (`/dev/ai`).
    pub dev_ai: dev_ai::Simulator,
    /// Dev mode: the simulated Stripe's customers, subscriptions, payments and log (`/dev/payments`).
    pub dev_payments: dev_payments::Simulator,
    /// Dev mode: the dev authenticator app's accounts (`/dev/authenticator`).
    pub dev_authenticator: dev::Authenticator,
    /// `SKRONGLE_PUBLIC_URL` without a trailing slash: where links in e-mail
    /// point. Unset, they are built from the request (`verify::public_base`).
    pub public_url: Option<String>,
    /// `SKRONGLE_SIGNUP`: who may make an account (`signup.rs`).
    pub signup: signup::Mode,
    /// The waitlist switch (#934), set on Superuser → Release.
    pub waitlist: waitlist::Switch,
    /// `SKRONGLE_PROXY_AUTH_HEADER` + `SKRONGLE_PROXY_AUTH_FROM`: the proxy in
    /// front signs people in (`proxy_auth.rs`); `None` when unset.
    pub proxy_auth: Option<proxy_auth::Config>,
    /// Addresses a superuser blocked (#967), checked on every request.
    pub blocked: moderation::Blocklist,
}

/// What this server can send (#633): features that would e-mail or text
/// someone ask here, and hide themselves when the channel is off. Templates
/// that need it carry a copy (`caps`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// `SKRONGLE_MAIL_PROVIDERS` is set (or dev mode's mailbox stands in).
    pub mail: bool,
    /// `SKRONGLE_SMS_PROVIDERS` is set (or dev mode's phone stands in).
    pub sms: bool,
}

impl Caps {
    /// Does a channel reach anyone? Not when its chain is the `log`
    /// provider alone, unless dev mode's mailbox or phone stands in for a
    /// provider (so the dev server and its specs work as before).
    pub fn reaches(logging_only: bool, dev_stand_in: bool) -> bool {
        !logging_only || dev_stand_in
    }
}

/// The hosted site needs every provider; only Skrongle mini (`SKRONGLE_MINI=1`;
/// and the dev server, whose mailbox, phone, Google and Stripe stand in)
/// runs without them. Self-hosting the full build is not supported (CLAUDE.md, "Two
/// shapes"). `missing` names the settings that are not set. The hosted site
/// is served over HTTPS, so it also needs `Secure` cookies, and proxy
/// sign-in (`proxy_auth.rs`) is a mini feature it refuses (audit M3, #980).
/// `Err` stops the server at startup.
pub fn require_providers(mini: bool, dev: bool, missing: &[&str], secure_cookies: bool, proxy_auth: bool) -> Result<(), String> {
    if mini || dev {
        return Ok(());
    }
    if proxy_auth {
        return Err(format!(
            "the hosted server signs people in itself; unset {} and {}, or run Skrongle mini (SKRONGLE_MINI=1) on a small box",
            proxy_auth::HEADER_VAR,
            proxy_auth::FROM_VAR
        ));
    }
    let mut missing = missing.to_vec();
    if !secure_cookies {
        missing.push("SKRONGLE_SECURE_COOKIES=1");
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the hosted server needs {}; set them, or run Skrongle mini (SKRONGLE_MINI=1) on a small box",
        missing.join(", ")
    ))
}

impl AppState {
    /// What this server can send: [`Caps`].
    pub fn caps(&self) -> Caps {
        Caps { mail: self.mail.can_send(), sms: self.sms.can_send() }
    }

    /// "Continue with Google" goes to dev mode's stand-in, not to Google.
    pub fn google_mock(&self) -> bool {
        self.google.as_ref().is_some_and(|g| g.mock)
    }
}

struct Config {
    bind: SocketAddr,
    data_dir: PathBuf,
    db_path: PathBuf,
    secure_cookies: bool,
    base_path: String,
    log_level: tracing::Level,
    google: Option<oauth::GoogleConfig>,
    stripe: Option<billing::StripeConfig>,
    /// `SKRONGLE_SECRET_KEY`; without it the secret lives in `<db>.key`.
    secret_key: Option<String>,
    /// `SKRONGLE_DEV=1`: asked for; `dev::resolve` decides.
    dev: bool,
    /// `SKRONGLE_DEV_SEED=1`: create the dev user with a course and progress at boot.
    dev_seed: bool,
    /// `SKRONGLE_DEV_SEED_USERS=N`: a crowd of N made-up learners with history
    /// (`dev_crowd.rs`), over the last `SKRONGLE_DEV_SEED_DAYS` days (90).
    dev_seed_users: usize,
    dev_seed_days: i64,
    /// `TEST_COURSE=1` (or `SKRONGLE_TEST_COURSE=1`): also load the courses for
    /// automated tests under `data/test-courses/`.
    test_course: bool,
    /// `SKRONGLE_THEMES_ENABLED=1`: offer and wear themes. Off by default
    /// until the themes have art people made (#1138).
    themes_enabled: bool,
    /// `SKRONGLE_THEMES_PER_USER` / `SKRONGLE_THEME_BYTES`: limits on themes a learner makes.
    theme_limits: themes::Limits,
    /// `SKRONGLE_COURSES_PER_USER` / `SKRONGLE_COURSE_BYTES` / `SKRONGLE_COURSE_BYTES_PER_USER`:
    /// limits on the courses a learner makes. `None` for the course count
    /// when unset: the default, or no limit in dev mode (browser tests make
    /// every course as the one dev user).
    course_limits: courses::Limits,
    courses_per_user: Option<usize>,
    /// `SKRONGLE_MAX_CONNECTIONS`: open connections the listener allows (#369).
    server: server::Limits,
    /// `SKRONGLE_AI_KEY`: the server's AI key; hosted AI is off without it.
    ai_key: Option<String>,
    /// The other `SKRONGLE_AI_*` settings (models, prices, allowances, caps).
    meter: Result<metering::MeterConfig, String>,
}

/// The settings' prefix, then the old ones `env` still reads, newest first.
/// The old prefixes are read for one release only (#584); the next release
/// drops them, and with them `old_prefix_warning`.
const ENV_PREFIXES: [&str; 3] = ["SKRONGLE_", "LEARN_", "SPANISH_"];

/// Take a secret out of the environment: read it, then remove every
/// spelling so threads and child processes never see it.
fn take_env(name: &str) -> Option<String> {
    let v = env(name);
    for prefix in ENV_PREFIXES {
        std::env::remove_var(format!("{prefix}{name}"));
    }
    v
}

/// `SKRONGLE_<name>`, falling back for one release to the old
/// `LEARN_<name>` and `SPANISH_<name>` spellings. Empty counts as unset.
///
/// In Skrongle mini the hosted-only settings (`shape::HOSTED_ONLY`) read as
/// unset, so e-mail, texts, Google, Stripe and S3 are off wherever they
/// are read.
pub fn env(name: &str) -> Option<String> {
    if shape::current().hides(name) {
        return None;
    }
    env_from(name, &|key| std::env::var(key).ok())
}

/// `env` over any lookup (a map in tests).
fn env_from(name: &str, get: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    ENV_PREFIXES.iter().find_map(|prefix| get(&format!("{prefix}{name}")).filter(|v| !v.is_empty()))
}

/// `old_prefix_warning` for this process's environment.
fn old_prefix_warning_now() -> Option<String> {
    let vars: Vec<(String, String)> = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
    old_prefix_warning(&vars)
}

/// The startup warning for settings still spelled with an old prefix: each
/// one with its new name, and which spelling wins when several are set.
/// `None` when there are none.
fn old_prefix_warning(vars: &[(String, String)]) -> Option<String> {
    let get = |key: &str| vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let mut lines = Vec::new();
    for (key, value) in vars {
        let Some(name) = ENV_PREFIXES[1..].iter().find_map(|p| key.strip_prefix(p)) else { continue };
        if value.is_empty() || name.is_empty() {
            continue;
        }
        let new = format!("{}{name}", ENV_PREFIXES[0]);
        let winner = ENV_PREFIXES.iter().map(|p| format!("{p}{name}")).find(|k| get(k).is_some_and(|v| !v.is_empty()));
        match winner {
            Some(w) if w != *key => lines.push(format!("{key} -> {new} (ignored: {w} is set and wins)")),
            _ => lines.push(format!("{key} -> {new}")),
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.sort();
    Some(format!(
        "settings with an old name, read for this release only; rename them (docs/self-hosting.md, \"Renamed settings\"): {}",
        lines.join(", ")
    ))
}

/// `SKRONGLE_BIND`: `address:port`, or a bare port meaning every interface.
fn parse_bind(raw: Option<&str>) -> Result<SocketAddr, String> {
    let raw = raw.map(str::trim).unwrap_or(DEFAULT_BIND);
    if let Ok(port) = raw.parse::<u16>() {
        return Ok(SocketAddr::from(([0, 0, 0, 0], port)));
    }
    raw.parse().map_err(|_| format!("SKRONGLE_BIND must be address:port, like {DEFAULT_BIND} or 127.0.0.1:8080; got {raw:?}"))
}

impl Config {
    fn from_env() -> Result<Config, String> {
        Config::from_lookup(&env)
    }

    /// The configuration from `env(name)`, which answers for `SKRONGLE_<name>`
    /// (`env` in the server, a map in tests).
    fn from_lookup(env: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        Ok(Config {
            bind: parse_bind(env("BIND").as_deref())?,
            data_dir: env("DATA_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR)),
            db_path: env("DB").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_DB)),
            secure_cookies: env("SECURE_COOKIES").map(|v| v == "1" || v == "true").unwrap_or(false),
            base_path: normalize_base_path(&env("BASE_PATH").unwrap_or_default()),
            log_level: env("LOG").and_then(|v| v.parse().ok()).unwrap_or(tracing::Level::INFO),
            secret_key: env("SECRET_KEY"),
            ai_key: take_env("AI_KEY"),
            meter: metering::MeterConfig::from_env(),
            dev: env("DEV").is_some_and(|v| v == "1" || v == "true"),
            dev_seed: env("DEV_SEED").is_some_and(|v| v == "1" || v == "true"),
            dev_seed_users: env("DEV_SEED_USERS").and_then(|v| v.parse().ok()).unwrap_or(0),
            dev_seed_days: env("DEV_SEED_DAYS").and_then(|v| v.parse().ok()).unwrap_or(90),
            themes_enabled: env("THEMES_ENABLED").is_some_and(|v| v.trim() == "1"),
            theme_limits: {
                let d = themes::Limits::default();
                themes::Limits {
                    max_themes: env("THEMES_PER_USER").and_then(|v| v.parse().ok()).unwrap_or(d.max_themes),
                    max_bytes: env("THEME_BYTES").and_then(|v| v.parse().ok()).unwrap_or(d.max_bytes),
                }
            },
            course_limits: {
                let d = courses::Limits::default();
                courses::Limits {
                    max_courses: d.max_courses,
                    course_bytes: env("COURSE_BYTES").and_then(|v| v.parse().ok()).unwrap_or(d.course_bytes),
                    account_bytes: env("COURSE_BYTES_PER_USER").and_then(|v| v.parse().ok()).unwrap_or(d.account_bytes),
                }
            },
            courses_per_user: env("COURSES_PER_USER").and_then(|v| v.parse().ok()),
            server: server::Limits {
                max_connections: env("MAX_CONNECTIONS").and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(server::Limits::default().max_connections),
                ..server::Limits::default()
            },
            test_course: std::env::var("TEST_COURSE").ok().or_else(|| env("TEST_COURSE")).is_some_and(|v| v == "1" || v == "true"),
            google: match (env("GOOGLE_CLIENT_ID"), env("GOOGLE_CLIENT_SECRET"), env("PUBLIC_URL")) {
                (Some(client_id), Some(client_secret), Some(public_url)) => Some(oauth::GoogleConfig::new(
                    client_id,
                    client_secret,
                    format!("{}/auth/google/callback", public_url.trim_end_matches('/')),
                )),
                (None, None, _) => None,
                _ => {
                    eprintln!("Google sign-in needs SKRONGLE_GOOGLE_CLIENT_ID, SKRONGLE_GOOGLE_CLIENT_SECRET and SKRONGLE_PUBLIC_URL; disabled");
                    None
                }
            },
            stripe: match (env("STRIPE_SECRET_KEY"), env("STRIPE_WEBHOOK_SECRET"), env("STRIPE_PRICE_ID"), env("PUBLIC_URL")) {
                (Some(secret_key), Some(webhook_secret), Some(price_id), Some(public_url)) => Some(billing::StripeConfig {
                    secret_key,
                    webhook_secret,
                    price_id,
                    price_id_basic: env("STRIPE_PRICE_ID_BASIC").or_else(|| env("STRIPE_PRICE_ID_PLUS")), // the old name, still read
                    price_label: env("STRIPE_PRICE_LABEL").unwrap_or_else(|| "Monthly".to_string()),
                    public_url: public_url.trim_end_matches('/').to_string(),
                    api_base: env("STRIPE_API_URL").unwrap_or_else(|| billing::STRIPE_API.to_string()),
                }),
                (None, None, None, _) => None,
                _ => {
                    eprintln!("Subscriptions need SKRONGLE_STRIPE_SECRET_KEY, SKRONGLE_STRIPE_WEBHOOK_SECRET, SKRONGLE_STRIPE_PRICE_ID and SKRONGLE_PUBLIC_URL; disabled");
                    None
                }
            },
        })
    }
}

const USAGE: &str = "\
Skrongle, a self-hosted learning app.

Usage:
  skrongle                     start the server (settings from SKRONGLE_* environment variables)
  skrongle doctor [--env-file FILE]
                               print diagnostics to paste into a bug report (no secrets)
  skrongle reset-password <email>
                               give the account a temporary password, printed here; it must be changed at the next sign-in
  skrongle disable-2fa <email> turn the account's two-factor sign-in off
  skrongle make-owner <email>  make the account a superuser (the Superuser page)
  skrongle content-check       run the content check beside the server (with SKRONGLE_CONTENT_CHECK=external); hosted only
  skrongle --version           print the version
  skrongle --schema-version    print the database schema version this binary migrates to

The account commands open SKRONGLE_DB (default learn.db) like the server does and work beside it or with it stopped.

Settings and troubleshooting: /docs/self-hosting and /docs/self-hosting-faq on a running server.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("doctor" | "--doctor") => std::process::exit(doctor::main(&args[1..])),
        Some("content-check") => std::process::exit(check_worker::main(&args[1..])),
        Some("--version" | "-V" | "version") => {
            println!("skrongle {VERSION}");
            return;
        }
        // The deploy compares it with the live database's (#1329).
        Some("--schema-version") => {
            println!("{}", db::schema_version());
            return;
        }
        Some("--help" | "-h" | "help") => {
            println!("{USAGE}");
            return;
        }
        // The owner's account commands (`reset-password <email>`, …:
        // `owner.rs`) run against SKRONGLE_DB and exit.
        Some("reset-password" | "disable-2fa" | "make-owner") => std::process::exit(owner::command(&args).unwrap_or(2)),
        Some(other) => {
            eprintln!("unknown argument {other:?}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
    // The shape first (`shape.rs`): in a mini `env` hides the hosted-only
    // settings `Config::from_env` would read.
    let ignored = match shape::Shape::from_env() {
        Ok(s) => {
            shape::init(s);
            s.ignored(&|name| env_from(name, &|key| std::env::var(key).ok()))
        }
        Err(e) => {
            eprintln!("fatal: {e}");
            std::process::exit(1);
        }
    };
    // Before `Config::from_env`, which takes the secrets out of the environment.
    let old_names = old_prefix_warning_now();
    let cfg = match Config::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("fatal: {e}");
            std::process::exit(1);
        }
    };
    tracing_subscriber::fmt()
        .with_max_level(cfg.log_level)
        // journald stores raw bytes; keep colours only for a real terminal.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .with_target(false)
        .compact()
        .init();
    for w in [old_names, ignored].into_iter().flatten() {
        tracing::warn!("{w}");
    }

    // Two async workers for the Cortex-A53; a small blocking pool for SQLite
    // and argon2. Everything else (thread stacks, etc.) stays at tokio defaults.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(3)
        .thread_name("learn-worker")
        .enable_io()
        .enable_time()
        .build()
        .expect("failed to build tokio runtime");

    if let Err(e) = rt.block_on(run(cfg)) {
        tracing::error!("fatal: {e}");
        std::process::exit(1);
    }
}

async fn run(cfg: Config) -> Result<(), Box<dyn std::error::Error>> {
    let _ = BASE_PATH.set(cfg.base_path.clone());
    tracing::info!("{} starting", edition(is_mini()));
    tracing::info!("{}", shape::current().describe());
    // Bind first, so a taken port fails before anything else is loaded.
    let listener = tokio::net::TcpListener::bind(cfg.bind).await.map_err(|e| startup::bind_error(cfg.bind, &e))?;
    let signup = signup::Mode::from_env()?;
    tracing::info!("{}", signup.describe());
    let proxy_auth = proxy_auth::Config::from_env()?;
    match &proxy_auth {
        Some(p) => {
            tracing::info!("{}", p.describe());
            if p.too_wide() {
                tracing::warn!("{} trusts a very wide range: anyone in it can sign in as anyone by sending the header", proxy_auth::FROM_VAR);
            }
        }
        None => tracing::info!("proxy sign-in: off ({} unset)", proxy_auth::HEADER_VAR),
    }
    let min_age = match env("MIN_AGE") {
        Some(v) => Some(v.trim().parse::<u32>().map_err(|_| format!("SKRONGLE_MIN_AGE must be a whole number, got {v:?}"))?),
        None => None,
    };
    // SKRONGLE_TERMS is read directly: it has no older spelling (#637).
    let terms_source = terms::Source::load(std::env::var(terms::SOURCE_VAR).ok().as_deref())?;
    if let terms::Source::File { path, version, .. } = &terms_source {
        tracing::info!("terms of service: {} (version {version})", path.display());
    }
    let own_terms = terms_source != terms::Source::Builtin;
    terms::init(min_age, env("LEGAL_EMAIL"), env("SUPPORT_EMAIL"), terms_source);
    if env("LEGAL_EMAIL").is_none() && !own_terms {
        tracing::info!("SKRONGLE_LEGAL_EMAIL is unset: the terms page shows a placeholder for the copyright contact");
    }
    let courses = Courses::load(&cfg.data_dir, cfg.test_course)
        .map_err(|e| format!("failed to load courses from {}: {e}", cfg.data_dir.display()))?;
    if cfg.test_course {
        tracing::warn!(
            "TEST_COURSE=1: the courses under {} are offered too; they exist for automated tests only",
            cfg.data_dir.join(courses::TEST_COURSES_DIR).display()
        );
    }
    for c in courses.builtin() {
        let (lessons, questions, terms) = c.totals();
        tracing::info!("course '{}' ({}): {lessons} lessons / {questions} questions / {terms} terms", c.id(), c.title());
    }
    if courses.builtin().is_empty() {
        tracing::warn!(
            "no built-in courses under {}; only user-made courses will be offered (set SKRONGLE_DATA_DIR to the folder that holds courses/ and themes/)",
            cfg.data_dir.join("courses").display()
        );
    }

    startup::check_db_writable(&cfg.db_path)?;
    // An older release on a database a newer one upgraded stops here, before
    // anything writes to it, and systemd does not restart it (#1329).
    if let Err(msg) = db::check_not_newer(&cfg.db_path) {
        tracing::error!("fatal: {msg}");
        std::process::exit(db::NEWER_DB_EXIT);
    }
    let db = Db::open(&cfg.db_path).map_err(|e| {
        // A dev database from another branch may have the same migration
        // number for a different change (stacked PRs renumber them).
        let hint = if cfg.dev { " (a dev database from another branch? ./.dev-start --reset recreates it)" } else { "" };
        format!("{}{hint}", startup::db_error(&cfg.db_path, &e))
    })?;
    let purged = db.with(|c| auth::purge_expired_sessions(c))?;
    tracing::info!("database ready at {} ({purged} expired sessions purged)", cfg.db_path.display());
    let retention = privacy::Retention::from_env();
    let swept = db.with(|c| privacy::apply_retention(c, &retention, db::now()))?;
    let notes_rule = if retention.keep_notes { "kept" } else { "deleted" };
    match retention.history_days {
        Some(d) => tracing::info!("retention: answers and game rounds kept {d} days ({} answers, {} rounds older deleted); notes of finished AI courses {notes_rule}", swept.answers, swept.rounds),
        None => tracing::info!("retention: answers and game rounds kept while the account exists; notes of finished AI courses {notes_rule} ({} now)", swept.notes),
    }
    match retention.failed_course_days {
        Some(d) => tracing::info!("retention: failed AI courses kept {d} days ({} older deleted)", swept.failed_courses.len()),
        None => tracing::info!("retention: failed AI courses kept until their owner deletes them"),
    }
    // A generation whose key can be had again waits in line to go on (#874).
    let waiting = db.with(|c| ai::at_boot(c, db::now()))?;
    if waiting > 0 {
        tracing::info!("{waiting} course generation(s) were interrupted by the restart; they go on by themselves");
    }
    let interrupted = db.with(|c| courses::fail_interrupted(c))?;
    if interrupted > 0 {
        tracing::warn!("{interrupted} course generation(s) were interrupted by the restart; their owners can resume them");
    }
    let saved_keys = db.with(|c| secrets::saved_key_count(c))?;
    let new_key = env("NEW_SECRET_KEY").is_some_and(|v| v == "1" || v == "true");
    startup::check_secret_key(cfg.secret_key.is_some(), &cfg.db_path, saved_keys, new_key)?;
    let (vault, secret_source) = secrets::Vault::load(cfg.secret_key.clone(), &cfg.db_path)?;
    let (readable, total) = db.with(|c| secrets::readable_count(c, &vault))?;
    startup::check_secret_opens(&cfg.db_path, &secret_source, readable, total, new_key)?;
    if new_key && saved_keys > 0 {
        let n = db.with(|c| secrets::forget_unreadable(c, &vault))?;
        tracing::warn!("SKRONGLE_NEW_SECRET_KEY=1: removed {n} saved AI key(s) the new secret key cannot read; learners enter them again");
    }
    if new_key {
        let n = db.with(|c| secrets::forget_unreadable_totp(c, &vault))?;
        if n > 0 {
            tracing::warn!("SKRONGLE_NEW_SECRET_KEY=1: turned off the authenticator app of {n} account(s) the new secret key cannot read; they sign in with their password and set it up again");
        }
    }
    tracing::info!("API keys are encrypted with the secret from {secret_source}");

    let themes = if cfg.themes_enabled {
        themes::Themes::load(&cfg.data_dir).map_err(|e| format!("failed to load themes from {}: {e}", cfg.data_dir.join("themes").display()))?
    } else {
        tracing::info!("themes are off (set SKRONGLE_THEMES_ENABLED=1 to offer them)");
        themes::Themes::off()
    };
    for t in themes.all() {
        tracing::info!("theme {}: {} ({} pictures)", t.theme.id, t.theme.title, t.theme.avatars.len());
    }
    let shop = motivators::shop::init(&cfg.data_dir).map_err(|e| format!("failed to load the shop's badges from {}: {e}", cfg.data_dir.join("badges").display()))?;
    tracing::info!("shop: {} badges", shop.badges.len());

    let force = env("DEV_FORCE").as_deref() == Some("1");
    let dev = match dev::resolve(cfg.dev, cfg.bind, cfg.secure_cookies, env("PUBLIC_URL").as_deref())
        .and_then(|on| if on { db.with(|c| dev::other_accounts(c)).map_err(|e| e.to_string()).and_then(|n| dev::check_accounts(n, force)).map(|()| true) } else { Ok(false) })
    {
        Ok(on) => on,
        Err(why) => {
            tracing::warn!("{why}");
            false
        }
    };
    for w in security::https_warnings(cfg.bind, cfg.secure_cookies, env("PUBLIC_URL").as_deref()) {
        tracing::warn!("{w}");
    }
    let llm_mode = dev::init_llm(dev, cfg.bind)?;
    // Dev mode without a real Google client: this server stands in for Google
    // (`/dev/google/*`), so "Continue with Google" works locally. SKRONGLE_GOOGLE_MOCK=0 opts out.
    // Dev mode without real Stripe settings: this server stands in for
    // Stripe (`/dev/payments`), with fake keys, so subscribing works locally.
    // SKRONGLE_DEV_STRIPE=0 opts out.
    // Skrongle mini (`billing::plans_on`) has no plans and sells nothing, and
    // no dev stand-in for Google, e-mail or texts either: it looks like a
    // real mini in dev mode too.
    let meter_cfg = cfg.meter.clone()?;
    let stripe = match cfg.stripe.clone() {
        _ if !meter_cfg.plans => None,
        Some(s) => Some(s),
        None if dev_payments::wanted(dev, false) => Some(dev_payments::dev_config(cfg.bind.port(), env("PUBLIC_URL").as_deref())),
        None => None,
    };
    let google = cfg.google.clone().or_else(|| (!is_mini() && dev::mock_google_wanted(dev, cfg.google.is_some())).then(|| dev::mock_google(cfg.bind)));
    if dev {
        tracing::warn!("DEV MODE: /dev/login, /dev/reset and the dev mailbox (/dev/inbox, /dev/ai) are open to anyone who can reach {}", cfg.bind);
        match llm_mode {
            Some(dev::LlmMode::Live) => tracing::warn!("DEV MODE: SKRONGLE_LLM_MODE=live: model calls go to the real providers"),
            Some(m) => tracing::warn!(
                "DEV MODE: model calls go to the simulated provider at http://{}{}{}; no real provider is contacted",
                cfg.bind,
                url("/dev/ai"),
                if m == dev::LlmMode::Capture { " (Manual replies: holding each request for a reply on /dev/ai)" } else { "" }
            ),
            None => {}
        }
        if let Some(seed) = env("RNG_SEED") {
            let n: u64 = seed.parse().map_err(|_| format!("SKRONGLE_RNG_SEED must be a number, got {seed:?}"))?;
            quiz::seed_draws(n);
            tracing::warn!("DEV MODE: SKRONGLE_RNG_SEED={n}: quizzes, sessions and games draw from this seed, not the OS");
        }
        if cfg.dev_seed {
            // The simulator's keys for every provider, unless the calls go to the real ones.
            let cid = dev::seed(&db, &courses, dev::simulating().then_some(&vault))?;
            tracing::info!("dev: seeded user '{}' with course '{cid}' open", dev::DEV_EMAIL);
        } else {
            // So the login form takes dev@example.com / dev-password on a fresh database.
            dev::ensure_user(&db)?;
        }
        if cfg.dev_seed_users > 0 {
            // The seed course first, so most of the crowd is where the dev user is.
            let mut crowd_courses = courses.builtin().to_vec();
            crowd_courses.sort_by_key(|c| c.id() != dev::SEED_COURSE);
            let added = dev_crowd::seed(&db, &crowd_courses, cfg.dev_seed_users, cfg.dev_seed_days)?;
            tracing::info!("dev: a crowd of {} learner(s) ({added} new), learner-<n>@{} / '{}'", cfg.dev_seed_users, dev_crowd::DOMAIN, dev::DEV_PASSWORD);
        }
        tracing::info!("dev: log in as '{}' / '{}', or open /dev/login", dev::DEV_EMAIL, dev::DEV_PASSWORD);
    }

    let hosted_note = cfg.ai_key.is_some().then(|| {
        if !meter_cfg.plans {
            return format!(
                "Skrongle AI enabled for every account ({:?}; tutor {}, courses {}; no plans, no monthly allowance; daily cap {}; {} calls a minute)",
                meter_cfg.provider,
                meter_cfg.tutor_model,
                meter_cfg.courses_model,
                metering::dollars(meter_cfg.daily_cap),
                meter_cfg.rate_per_minute
            );
        }
        format!(
            "Skrongle AI enabled ({:?}; tutor {}, courses {}; allowance {} a month on Plus; sign-up credit {}; daily cap {}; {} calls a minute; {} credit packs)",
            meter_cfg.provider,
            meter_cfg.tutor_model,
            meter_cfg.courses_model,
            metering::dollars(meter_cfg.allowance_plus),
            metering::dollars(meter_cfg.signup_credit),
            metering::dollars(meter_cfg.daily_cap),
            meter_cfg.rate_per_minute,
            meter_cfg.packs.len()
        )
    });
    let meter = Arc::new(metering::Meter::new(meter_cfg, cfg.ai_key.clone(), &vault, stripe.is_some()));
    match hosted_note {
        Some(n) => tracing::info!("{n}"),
        None => tracing::info!("Skrongle AI disabled (no SKRONGLE_AI_KEY): AI features use learners' own keys"),
    }
    let check_mode = content_check::Mode::from_env()?;
    let mut mail_cfg = mail::Config::from_env()?;
    let mut sms_cfg = sms::Config::from_env()?;
    let storage = Arc::new(storage::Storage::from_settings(env("S3_BUCKET"), &|k| std::env::var(k).ok())?);
    let missing: Vec<&str> = [
        (mail_cfg.logging_only, "SKRONGLE_MAIL_PROVIDERS"),
        (storage.bucket().is_none(), storage::BUCKET_VAR),
        (sms_cfg.logging_only, "SKRONGLE_SMS_PROVIDERS"),
        (cfg.google.is_none(), "SKRONGLE_GOOGLE_CLIENT_ID"),
        (cfg.stripe.is_none(), "SKRONGLE_STRIPE_SECRET_KEY"),
        (content_check::key_missing(check_mode), content_check::KEY_VAR),
        // Both plans are sold, each at its own price: the webhook tells them apart by price (#963).
        (cfg.stripe.as_ref().is_some_and(|s| s.price_for(billing::Plan::Basic).is_none()), "SKRONGLE_STRIPE_PRICE_ID_BASIC (its own price, not SKRONGLE_STRIPE_PRICE_ID)"),
    ]
    .into_iter()
    .filter_map(|(gone, name)| gone.then_some(name))
    .collect();
    require_providers(is_mini(), dev, &missing, cfg.secure_cookies, proxy_auth.is_some())?;
    // Dev mode without providers: mail goes over SMTP to the dev mailbox (mail/sink.rs).
    let dev_port = if dev && !is_mini() && mail_cfg.logging_only { mail::sink::port_from_env(env("DEV_SMTP").as_deref())? } else { None };
    let dev_inbox = match dev_port {
        Some(port) => {
            let inbox = mail::sink::start(port).map_err(|e| format!("dev mailbox on 127.0.0.1:{port} (SKRONGLE_DEV_SMTP): {e}"))?;
            mail_cfg.chain = mail::Chain::new(vec![mail::sink::provider(&inbox, &mail_cfg.from)?]);
            Some(inbox)
        }
        None => None,
    };
    let mail_on = Caps::reaches(mail_cfg.logging_only, dev_inbox.is_some());
    if !mail_on {
        tracing::info!("e-mail: not configured; features that need it are off (SKRONGLE_MAIL_PROVIDERS)");
    } else if dev_inbox.is_none() {
        tracing::info!("e-mail from {} via {}", mail_cfg.from, mail_cfg.chain.names().join(", then "));
    }
    if env("PUBLIC_URL").is_none() {
        tracing::info!("links in e-mail point to the host each request was made to; set SKRONGLE_PUBLIC_URL to fix them");
    }
    let ceiling = mail_cfg.ceiling(dev);
    match ceiling {
        Some(c) => tracing::info!("e-mail: at most {} messages of each purpose due per {}s (SKRONGLE_MAIL_PER_HOUR); more wait for the next window", c.per_window, c.window),
        None => tracing::info!("e-mail: no hourly ceiling (SKRONGLE_MAIL_PER_HOUR)"),
    }
    // The job runner (jobs.rs) sends the mail and the texts; the handles
    // in AppState queue them and wake it.
    let (queue, queue_wake) = jobs::Queue::new();
    let mail_outbox = mail::Outbox::new(mail_cfg.chain, mail_cfg.from, mail_cfg.reply_to, dev);
    let mailer = mail::Mailer::new(&mail_outbox, ceiling, mail_on, queue.waker());
    // Dev mode without providers: texts go to the dev phone (sms/phone.rs),
    // unless SKRONGLE_DEV_PHONE=0 (then they are only logged, as on a server).
    let dev_phone = (dev && !is_mini() && sms_cfg.logging_only && env("DEV_PHONE").as_deref() != Some("0")).then(sms::phone::Phone::new);
    let sms_on = Caps::reaches(sms_cfg.logging_only, dev_phone.is_some());
    if let Some(phone) = &dev_phone {
        sms_cfg.chain = sms::Chain::new(vec![sms::phone::provider(phone)]);
    } else if sms_cfg.logging_only {
        tracing::info!("text messages: not configured; features that need them are off (SKRONGLE_SMS_PROVIDERS)");
    } else {
        tracing::info!("text messages via {}", sms_cfg.chain.names().join(", then "));
    }
    // The dev mailbox page holds the e-mail and the texts: one link for both.
    match (&dev_inbox, dev_phone.is_some()) {
        (Some(inbox), true) => tracing::info!("dev: e-mail (SMTP on {}) and text messages go to the dev mailbox; read them at http://{}{} (SKRONGLE_DEV_SMTP=0 to only log e-mail)", inbox.addr(), cfg.bind, url("/dev/inbox")),
        (Some(inbox), false) => tracing::info!("dev: e-mail goes to the dev mailbox (SMTP on {}); read it at http://{}{} (SKRONGLE_DEV_SMTP=0 to only log it)", inbox.addr(), cfg.bind, url("/dev/inbox")),
        (None, true) => tracing::info!("dev: text messages go to the dev mailbox; read them at http://{}{}", cfg.bind, url("/dev/phone")),
        (None, false) => {}
    }
    let sms_outbox = sms::Outbox::new(sms_cfg.chain, dev);
    let texter = sms::Texter::new(&sms_outbox, sms_cfg.policy, sms_on, queue.waker());
    tracing::info!("{}", storage.describe());
    let exports_dir = exports::Exports::open(&cfg.db_path, Arc::clone(&storage)).map_err(|e| format!("data exports folder for {}: {e}", cfg.db_path.display()))?;
    let export_mail: Option<Box<dyn Fn() + Send>> = mail_on.then(|| {
        let wake = mailer.waker();
        Box::new(move || {
            let _ = wake.send(());
        }) as Box<dyn Fn() + Send>
    });
    let mut export_builder = exports::Builder::new(exports_dir.dir.clone(), export_mail);
    export_builder.storage = storage;
    // Dev: a slow export, to watch it in slices on /dev/jobs or stop the
    // server halfway through (SKRONGLE_DEV_EXPORT_PAUSE_MS after each page,
    // five pages a slice).
    if let Some(ms) = env("DEV_EXPORT_PAUSE_MS").filter(|_| dev).and_then(|v| v.parse().ok()) {
        export_builder.pause = std::time::Duration::from_millis(ms);
        export_builder.slice_pages = 5;
        tracing::warn!("DEV MODE: data exports pause {ms} ms after each page, five pages a slice (SKRONGLE_DEV_EXPORT_PAUSE_MS)");
    }

    let state = Arc::new(AppState {
        db: db.clone(),
        courses: Arc::new(courses.with_limits(courses::Limits {
            max_courses: cfg.courses_per_user.unwrap_or(if dev { usize::MAX } else { cfg.course_limits.max_courses }),
            ..cfg.course_limits
        })),
        themes: Arc::new(themes.with_limits(cfg.theme_limits)),
        secure_cookies: cfg.secure_cookies,
        dummy_hash: auth::hash_password("placeholder-for-timing-only")?,
        games: games::Sessions::default(),
        jobs: ai::Jobs::default(),
        google: google.clone(),
        signups: Default::default(),
        google_joins: Default::default(),
        step_ups: Default::default(),
        stripe: stripe.clone(),
        vault,
        tutors: tutor::Sessions::default(),
        meter,
        dev,
        limits: security::RateLimits::new(dev),
        mail: mailer,
        sms: texter,
        exports: exports_dir,
        queue,
        dev_inbox,
        dev_phone,
        dev_marks: Default::default(),
        dev_ai: if llm_mode == Some(dev::LlmMode::Capture) { dev_ai::Simulator::holding_from_start() } else { Default::default() },
        dev_authenticator: Default::default(),
        dev_payments: Default::default(),
        public_url: env("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()),
        signup,
        waitlist: Default::default(),
        proxy_auth,
        blocked: Default::default(),
    });
    db.with(|c| state.blocked.reload(c))?;
    match &google {
        Some(g) if g.mock => tracing::warn!(
            "DEV MODE: Google sign-in is mocked: {} stands in for Google; /auth/google?mock_email=…&mock_error=… picks the account or the failure (SKRONGLE_GOOGLE_MOCK=0 to disable)",
            g.auth_url
        ),
        Some(g) => tracing::info!("Google sign-in enabled (redirect URI {})", g.redirect_uri),
        None => tracing::info!("Google sign-in disabled (no SKRONGLE_GOOGLE_CLIENT_ID)"),
    }
    match &stripe {
        _ if !billing::plans_on(&state) => tracing::info!("plans: off (every account has every feature)"),
        Some(s) if s.api_base.contains("/dev/payments/stripe") => tracing::warn!(
            "DEV MODE: subscriptions go to the simulated Stripe at {} with fake keys; pay on its Checkout page, watch /dev/payments (SKRONGLE_DEV_STRIPE=0 to turn it off)",
            s.api_base
        ),
        Some(s) => tracing::info!("subscriptions enabled (Stripe price {}, webhook at {}/billing/webhook)", s.price_id, s.public_url),
        None => tracing::info!("subscriptions disabled (no SKRONGLE_STRIPE_SECRET_KEY): every learner without a grant is on the free plan"),
    }

    // Background jobs: the mail and text outboxes (drained first on every
    // pass), the hourly housekeeping (one job per purge, each queuing its
    // next run; due at once on a fresh database), data exports (a slice per
    // run), AI jobs waiting in line (each started on its own thread), and in
    // dev mode the always-failing kind /dev/jobs queues.
    mail_outbox.blank_at_start(&db);
    sms_outbox.blank_at_start(&db);
    state.jobs.wake_with(state.queue.waker());
    ai::adopt(&state.jobs, &db)?;
    let gen_state = Arc::clone(&state);
    let mut runner = jobs::housekeeping(jobs::Runner::new().drain(mail_outbox.drain()).drain(sms_outbox.drain()), &state, retention)
        .handle_steps(jobs::EXPORT, move |db, payload| export_builder.run(db, payload))
        .handle_claimed(ai::GENERATE, move || ai::slot_free(&gen_state.jobs), ai::handler(Arc::clone(&state)));
    tracing::info!("{}", content_check::init(is_mini(), dev, cfg.bind.port(), check_mode));
    let cache = state.courses.clone();
    runner = content_check::jobs(runner, state.meter.clone(), Arc::new(move |id: &str| cache.invalidate(id)));
    if dev {
        runner = jobs::dev(runner);
    }
    runner.start(db, queue_wake).map_err(|e| format!("background job thread: {e}"))?;

    // Kept for the checkpoint after shutdown below.
    let shutdown_db = state.db.clone();
    // No account yet: the owner's account is made only through this link,
    // so only someone who can read this log can claim the server.
    if state.signup != signup::Mode::Open {
        if let Some(token) = state.db.with(|c| signup::owner_link(c, db::now()))? {
            let link = signup::owner_link_url(state.public_url.as_deref(), cfg.bind, &token);
            tracing::warn!("no account yet. Owner link, to make your own account (the owner's): {link}");
        }
    }
    // The waitlist switch (#934), set on Superuser → Release; never on Skrongle mini.
    if !is_mini() {
        state.db.with(|c| state.waitlist.load(c, env(waitlist::LOGIN_VAR).is_some_and(|v| v.trim() == "1")))?;
        if state.waitlist.is_on() {
            tracing::info!("waitlist: on; sign-up only through an e-mailed invitation (Superuser → Release)");
        }
    }
    let app = routes::router(state);
    release_boot_memory();
    tracing::info!("{} listening on http://{}{}", edition(is_mini()), cfg.bind, url("/"));
    // Handlers get the client's address (ConnectInfo) for the rate limits.
    server::serve(listener, app, cfg.server, shutdown_signal()).await?;
    // Fold the WAL back into learn.db, so after a clean stop that one file
    // holds everything: a copy of it alone (update.sh's learn.db.before-<v>,
    // a move to a new machine) is complete.
    if let Err(e) = shutdown_db.with(|c| c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")) {
        tracing::warn!("checkpoint at shutdown failed: {e}");
    }
    tracing::info!("shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::{env_from, old_prefix_warning, require_providers};

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key| vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn owned(vars: &[(&str, &str)]) -> Vec<(String, String)> {
        vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn env_reads_the_new_prefix_first_then_the_old_ones() {
        let new = "SKRONGLE_DB";
        let learn = concat!("LEA", "RN_DB");
        assert_eq!(env_from("DB", &lookup(&[(new, "a.db")])).as_deref(), Some("a.db"));
        assert_eq!(env_from("DB", &lookup(&[(learn, "b.db")])).as_deref(), Some("b.db"), "old name still works");
        assert_eq!(env_from("DB", &lookup(&[("SPANISH_DB", "c.db")])).as_deref(), Some("c.db"), "oldest name too");
        assert_eq!(env_from("DB", &lookup(&[(learn, "b.db"), (new, "a.db"), ("SPANISH_DB", "c.db")])).as_deref(), Some("a.db"), "new wins");
        assert_eq!(env_from("DB", &lookup(&[(learn, "b.db"), ("SPANISH_DB", "c.db")])).as_deref(), Some("b.db"));
        assert_eq!(env_from("DB", &lookup(&[(new, ""), (learn, "b.db")])).as_deref(), Some("b.db"), "empty is unset");
        assert_eq!(env_from("DB", &lookup(&[])), None);
    }

    #[test]
    fn old_names_get_one_warning_naming_the_new_ones() {
        let learn_db = concat!("LEA", "RN_DB");
        let learn_log = concat!("LEA", "RN_LOG");
        assert_eq!(old_prefix_warning(&owned(&[("SKRONGLE_DB", "a.db"), ("HOME", "/root")])), None);
        let w = old_prefix_warning(&owned(&[(learn_db, "b.db"), ("SPANISH_BIND", "0.0.0.0:80"), ("SKRONGLE_LOG", "info")])).unwrap();
        assert!(w.contains(&format!("{learn_db} -> SKRONGLE_DB")) && w.contains("SPANISH_BIND -> SKRONGLE_BIND"), "{w}");
        assert!(!w.contains("SKRONGLE_LOG"), "{w}");
        assert_eq!(w.matches(" -> ").count(), 2, "{w}");
        // Both spellings, different values: the new one wins and the warning says so.
        let w = old_prefix_warning(&owned(&[(learn_log, "debug"), ("SKRONGLE_LOG", "info")])).unwrap();
        assert!(w.contains(&format!("{learn_log} -> SKRONGLE_LOG (ignored: SKRONGLE_LOG is set and wins)")), "{w}");
        let w = old_prefix_warning(&owned(&[(learn_db, "b.db"), ("SPANISH_DB", "c.db")])).unwrap();
        assert!(w.contains(&format!("SPANISH_DB -> SKRONGLE_DB (ignored: {learn_db} is set and wins)")), "{w}");
        assert_eq!(old_prefix_warning(&owned(&[(learn_db, "")])), None, "empty is unset");
    }

    /// The old prefix appears only where it is still read (#584): the
    /// fallback here, the deploy scripts that rename it (on AWS, the
    /// Parameter Store settings and deploy.env leftovers), and the docs that
    /// tell people to rename theirs. Remove this list with the fallback.
    #[test]
    fn the_old_prefix_does_not_come_back() {
        let allowed = ["src/main.rs", "docs/self-hosting.md", "docs/architecture.md", "scripts/deploy-env.sh", "scripts/deploy.sh", "deploy/update.sh", "scripts/aws-env.sh", "scripts/deploy-aws.sh"];
        let needle = concat!("LEA", "RN_");
        let root = env!("CARGO_MANIFEST_DIR");
        let Ok(out) = std::process::Command::new("git").args(["-C", root, "ls-files", "-z"]).output() else {
            eprintln!("no git: skipped");
            return;
        };
        let files: Vec<String> = out.stdout.split(|b| *b == 0).filter(|f| !f.is_empty()).map(|f| String::from_utf8_lossy(f).into_owned()).collect();
        assert!(files.len() > 100 || !out.status.success(), "git ls-files listed {} files", files.len());
        let found: Vec<&String> = files
            .iter()
            .filter(|f| !allowed.contains(&f.as_str()))
            .filter(|f| std::fs::read(format!("{root}/{f}")).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes())))
            .collect();
        assert!(found.is_empty(), "settings are named SKRONGLE_ now; the old prefix is back in {found:?}");
    }

    #[test]
    fn only_the_hosted_server_requires_providers() {
        let none = ["SKRONGLE_MAIL_PROVIDERS", "SKRONGLE_SMS_PROVIDERS", "SKRONGLE_GOOGLE_CLIENT_ID", "SKRONGLE_STRIPE_SECRET_KEY", "TYPESAFE_API_KEY"];
        let why = require_providers(false, false, &none, true, false).unwrap_err();
        assert!(none.iter().all(|n| why.contains(n)), "{why}");
        assert!(why.contains("SKRONGLE_MINI=1"), "{why}");
        assert!(require_providers(false, false, &["SKRONGLE_SMS_PROVIDERS"], true, false).is_err());
        assert_eq!(require_providers(false, false, &[], true, false), Ok(()));
        assert_eq!(require_providers(true, false, &none, false, false), Ok(()), "mini needs none");
        assert_eq!(require_providers(false, true, &none, false, false), Ok(()), "dev stands in");
    }

    #[test]
    fn the_hosted_server_requires_secure_cookies() {
        let why = require_providers(false, false, &[], false, false).unwrap_err();
        assert!(why.contains("SKRONGLE_SECURE_COOKIES=1") && why.contains("SKRONGLE_MINI=1"), "{why}");
        let why = require_providers(false, false, &["SKRONGLE_SMS_PROVIDERS"], false, false).unwrap_err();
        assert!(why.contains("SKRONGLE_SMS_PROVIDERS") && why.contains("SKRONGLE_SECURE_COOKIES=1"), "{why}");
        assert_eq!(require_providers(false, false, &[], true, false), Ok(()));
        assert_eq!(require_providers(true, false, &[], false, false), Ok(()), "mini may run on plain HTTP");
        assert_eq!(require_providers(false, true, &[], false, false), Ok(()), "dev mode refuses secure cookies");
    }

    #[test]
    fn the_hosted_server_refuses_proxy_sign_in() {
        let why = require_providers(false, false, &[], true, true).unwrap_err();
        assert!(why.contains(crate::proxy_auth::HEADER_VAR) && why.contains(crate::proxy_auth::FROM_VAR), "{why}");
        assert!(why.contains("SKRONGLE_MINI=1"), "{why}");
        assert_eq!(require_providers(true, false, &[], false, true), Ok(()), "proxy sign-in is a mini feature");
        assert_eq!(require_providers(false, true, &[], false, true), Ok(()), "dev mode may try it");
    }
}

/// Hand the heap pages that booting freed back to the OS. Loading the
/// courses reads every file, parses it and drops the text; glibc on its own
/// only trims the top of the heap, so what stays resident depends on the
/// sizes and order of those files (deleting fourteen small SVGs moved the
/// idle RSS on the Pi by 2.5 MiB, #842). `malloc_trim(0)` releases every
/// free page in every arena, whatever the layout. Other allocators have no
/// such call and do not keep freed pages the same way.
fn release_boot_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim only walks glibc's own heap and takes no pointers.
    unsafe {
        libc::malloc_trim(0);
    }
}
