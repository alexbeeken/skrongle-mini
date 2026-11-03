//! Lightweight self-hosted learning app (lessons, quizzes,
//! games and phone sessions) driven by JSON courses. Sized for a Raspberry
//! Pi Zero 2 W.

mod account_deletion;
mod activity;
mod algebra_tiles;
mod ai;
mod arrange;
mod auth;
mod caps;
mod clock;
mod conjugation;
mod consent;
mod content;
mod courses;
mod db;
mod dev;
mod dev_ai;
mod dev_crowd;
mod doctor;
mod earnings;
mod sprint;
mod error;
mod games;
mod guide;
mod i18n;
mod jobs;
mod lint;
mod listening;
mod mathcheck;
mod measure;
mod notation;
mod notes;
mod mastery;
mod prices;
mod motivators;
mod urlenc;
mod owner;
mod placement;
mod privacy;
mod problems;
mod profile;
mod progress;
mod proxy_auth;
mod quiz;
mod random;
mod report;
mod review;
mod routes;
mod secrets;
mod settings;
mod share;
mod shape;
mod signup;
mod startup;
mod security;
mod server;
mod totp;
mod translate;
mod two_factor;
#[cfg(test)]
mod test_course;
mod tutor;
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

/// Skrongle mini (#641, #578): the small-box defaults
/// `deploy/skrongle.service` sets (invite-only sign-up, the owner's own
/// Terms). The server is a mini when `SKRONGLE_MINI=1` (`shape.rs`, read
/// once at startup).
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
    pub secure_cookies: bool,
    /// Hash verified against for unknown addresses, to keep login timing uniform.
    pub dummy_hash: String,
    /// In-flight timed game sessions (in memory only).
    pub games: games::Sessions,
    /// Running and recently finished course-generation jobs.
    pub jobs: ai::Jobs,
    /// Encrypts learners' AI API keys at rest.
    pub vault: secrets::Vault,
    /// AI tutoring conversations (in memory only).
    pub tutors: tutor::Sessions,
    /// The price table behind the Generate dialog's cost estimate.
    pub prices: prices::PriceTable,
    /// `SKRONGLE_DEV=1` on a loopback bind: the `/dev/*` routes exist.
    pub dev: bool,
    /// Rate limits for sign-in, registration and password changes (in memory; off in dev mode).
    pub limits: security::RateLimits,
    /// Wakes the background job runner (`jobs.rs`).
    pub queue: jobs::Queue,
    /// Dev mode: the simulated AI provider's log, usage and settings (`/dev/ai`).
    pub dev_ai: dev_ai::Simulator,
    /// Dev mode: the dev authenticator app's accounts (`/dev/authenticator`).
    pub dev_authenticator: dev::Authenticator,
    /// `SKRONGLE_PUBLIC_URL` without a trailing slash: where the links the
    /// app hands out (owner link, invitations, share links) point. Unset, they are built from the request (`routes::public_base`).
    pub public_url: Option<String>,
    /// `SKRONGLE_SIGNUP`: who may make an account (`signup.rs`).
    pub signup: signup::Mode,
    /// `SKRONGLE_PROXY_AUTH_HEADER` + `SKRONGLE_PROXY_AUTH_FROM`: the proxy in
    /// front signs people in (`proxy_auth.rs`); `None` when unset.
    pub proxy_auth: Option<proxy_auth::Config>,
}

/// Outside Skrongle mini (`SKRONGLE_MINI=1`) and the dev server, the server
/// is served over HTTPS, so it needs `Secure` cookies, and proxy sign-in
/// (`proxy_auth.rs`) is a mini feature it refuses (audit M3, #980).
/// `Err` stops the server at startup.
pub fn require_providers(mini: bool, dev: bool, secure_cookies: bool, proxy_auth: bool) -> Result<(), String> {
    if mini || dev {
        return Ok(());
    }
    if proxy_auth {
        return Err(format!(
            "this server signs people in itself; unset {} and {}, or run Skrongle mini (SKRONGLE_MINI=1) on a small box",
            proxy_auth::HEADER_VAR,
            proxy_auth::FROM_VAR
        ));
    }
    if secure_cookies {
        return Ok(());
    }
    Err("this server needs SKRONGLE_SECURE_COOKIES=1; set it, or run Skrongle mini (SKRONGLE_MINI=1) on a small box".into())
}

struct Config {
    bind: SocketAddr,
    data_dir: PathBuf,
    db_path: PathBuf,
    secure_cookies: bool,
    base_path: String,
    log_level: tracing::level_filters::LevelFilter,
    /// An unparseable `SKRONGLE_LOG`, warned about once the logger is up.
    log_invalid: Option<String>,
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
    /// `SKRONGLE_COURSES_PER_USER` / `SKRONGLE_COURSE_BYTES` / `SKRONGLE_COURSE_BYTES_PER_USER`:
    /// limits on the courses a learner makes. `None` for the course count
    /// when unset: the default, or no limit in dev mode (browser tests make
    /// every course as the one dev user).
    course_limits: courses::Limits,
    courses_per_user: Option<usize>,
    /// `SKRONGLE_AI_PRICES`: the price table behind the cost estimate.
    prices: Result<prices::PriceTable, String>,
}

/// The settings' prefix, then the old ones `env` still reads, newest first.
/// The old prefixes are read for one release only (#584); the next release
/// drops them, and with them `old_prefix_warning`.
const ENV_PREFIXES: [&str; 3] = ["SKRONGLE_", "LEARN_", "SPANISH_"];

/// `SKRONGLE_<name>`, falling back for one release to the old
/// `LEARN_<name>` and `SPANISH_<name>` spellings. Empty counts as unset.
///
/// In Skrongle mini the provider settings (`shape::HOSTED_ONLY`) read as
/// unset, so texts, Google and S3 are off wherever they are read.
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
            log_level: env("LOG").and_then(|v| v.parse().ok()).unwrap_or(tracing::level_filters::LevelFilter::INFO),
            log_invalid: env("LOG").filter(|v| v.parse::<tracing::level_filters::LevelFilter>().is_err()),
            secret_key: env("SECRET_KEY"),
            prices: prices::PriceTable::from_env(),
            dev: env("DEV").is_some_and(|v| v == "1" || v == "true"),
            dev_seed: env("DEV_SEED").is_some_and(|v| v == "1" || v == "true"),
            dev_seed_users: env("DEV_SEED_USERS").and_then(|v| v.parse().ok()).unwrap_or(0),
            dev_seed_days: env("DEV_SEED_DAYS").and_then(|v| v.parse().ok()).unwrap_or(90),
            course_limits: {
                let d = courses::Limits::default();
                courses::Limits {
                    max_courses: d.max_courses,
                    course_bytes: env("COURSE_BYTES").and_then(|v| v.parse().ok()).unwrap_or(d.course_bytes),
                    account_bytes: env("COURSE_BYTES_PER_USER").and_then(|v| v.parse().ok()).unwrap_or(d.account_bytes),
                }
            },
            courses_per_user: env("COURSES_PER_USER").and_then(|v| v.parse().ok()),
            test_course: std::env::var("TEST_COURSE").ok().or_else(|| env("TEST_COURSE")).is_some_and(|v| v == "1" || v == "true"),
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
  skrongle make-owner <email>  make the account an owner (the Manage Users page)
  skrongle --version           print the version
  skrongle --schema-version    print the database schema version this binary migrates to

The account commands open SKRONGLE_DB (default learn.db) like the server does and work beside it or with it stopped.

Settings and troubleshooting: /docs/self-hosting and /docs/self-hosting-faq on a running server.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("doctor" | "--doctor") => std::process::exit(doctor::main(&args[1..])),
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
    // The shape first (`shape.rs`): in a mini `env` hides the provider
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
        .with_target(false)
        .compact()
        .init();
    for w in [old_names, ignored].into_iter().flatten() {
        tracing::warn!("{w}");
    }
    if let Some(v) = &cfg.log_invalid {
        tracing::warn!("SKRONGLE_LOG={v:?} is not a level (error, warn, info, debug or trace); using info");
    }

    // One async worker and two blocking threads (#35): at most 8 accounts and
    // 32 connections, so one worker serves every request, and two blocking
    // threads let a `Db::call` overlap an argon2 hash. Multi-thread flavour
    // stays so `spawn_blocking` exists. Nothing on a request path may block
    // the worker (docs/architecture.md, "Request flow").
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(2)
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
    privacy::init_min_age(min_age);
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
            "no built-in courses under {}; only user-made courses will be offered (set SKRONGLE_DATA_DIR to the folder that holds courses/)",
            cfg.data_dir.join("courses").display()
        );
    }

    startup::check_db_writable(&cfg.db_path)?;
    // An older release on a database a newer one upgraded, or a database from
    // before the 1.0 schema (#49), stops here, before anything writes to it,
    // and systemd does not restart it (#1329).
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
    // Give back the page cache and temp memory the migrations grew.
    db.with(|c| c.execute_batch("PRAGMA shrink_memory;"))?;
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
    // A generation whose key can be had again goes on after the restart (#874).
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
    // Skrongle mini has no dev stand-in for Google or texts: it looks like a
    // real mini in dev mode too.
    let prices = cfg.prices.clone()?;
    if dev {
        tracing::warn!("DEV MODE: /dev/login, /dev/reset and the dev pages (/dev/authenticator, /dev/ai) are open to anyone who can reach {}", cfg.bind);
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
            let mut crowd_courses: Vec<_> = courses.builtin().iter().filter_map(|c| courses.builtin_store(c.id())).collect();
            crowd_courses.sort_by_key(|c| c.id() != dev::SEED_COURSE);
            let added = dev_crowd::seed(&db, &crowd_courses, cfg.dev_seed_users, cfg.dev_seed_days)?;
            tracing::info!("dev: a crowd of {} learner(s) ({added} new), learner-<n>@{} / '{}'", cfg.dev_seed_users, dev_crowd::DOMAIN, dev::DEV_PASSWORD);
        }
        tracing::info!("dev: log in as '{}' / '{}', or open /dev/login", dev::DEV_EMAIL, dev::DEV_PASSWORD);
    }

    require_providers(is_mini(), dev, cfg.secure_cookies, proxy_auth.is_some())?;
    if env("PUBLIC_URL").is_none() {
        tracing::info!("links the app hands out point to the host each request was made to; set SKRONGLE_PUBLIC_URL to fix them");
    }
    // The job runner (jobs.rs); the handle in AppState wakes it.
    let (queue, queue_wake) = jobs::Queue::new();
    let state = Arc::new(AppState {
        db: db.clone(),
        courses: Arc::new(courses.with_limits(courses::Limits {
            max_courses: cfg.courses_per_user.unwrap_or(if dev { usize::MAX } else { cfg.course_limits.max_courses }),
            ..cfg.course_limits
        })),
        secure_cookies: cfg.secure_cookies,
        dummy_hash: auth::hash_password("placeholder-for-timing-only")?,
        games: games::Sessions::default(),
        jobs: ai::Jobs::default(),
        vault,
        tutors: tutor::Sessions::default(),
        prices,
        dev,
        limits: security::RateLimits::new(dev),
        queue,
        dev_ai: if llm_mode == Some(dev::LlmMode::Capture) { dev_ai::Simulator::holding_from_start() } else { Default::default() },
        dev_authenticator: Default::default(),
        public_url: env("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()),
        signup,
        proxy_auth,
    });

    // Background jobs: the hourly housekeeping (one job per purge, each queuing its
    // next run; due at once on a fresh database), a generation resumed after a restart (started on its own thread), and in
    // dev mode the always-failing kind /dev/jobs queues.
    state.jobs.wake_with(state.queue.waker());
    ai::adopt(&state.jobs, &db)?;
    let gen_state = Arc::clone(&state);
    let mut runner = jobs::housekeeping(jobs::Runner::new(), &state, retention)
        .handle_claimed(ai::GENERATE, move || ai::slot_free(&gen_state.jobs), ai::handler(Arc::clone(&state)));
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
    let app = routes::router(state);
    release_boot_memory();
    tracing::info!("{} listening on http://{}{}", edition(is_mini()), cfg.bind, url("/"));
    // Handlers get the client's address (ConnectInfo) for the rate limits.
    server::serve(listener, app, server::Limits::default(), shutdown_signal()).await?;
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
    /// fallback here, the deploy scripts that rename it (deploy.env
    /// leftovers), and the docs that
    /// tell people to rename theirs. Remove this list with the fallback.
    #[test]
    fn the_old_prefix_does_not_come_back() {
        let allowed = ["src/main.rs", "docs/self-hosting.md", "docs/architecture.md", "scripts/deploy-env.sh", "scripts/deploy.sh", "deploy/update.sh"];
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
    fn a_server_outside_mini_requires_secure_cookies() {
        let why = require_providers(false, false, false, false).unwrap_err();
        assert!(why.contains("SKRONGLE_SECURE_COOKIES=1") && why.contains("SKRONGLE_MINI=1"), "{why}");
        assert_eq!(require_providers(false, false, true, false), Ok(()));
        assert_eq!(require_providers(true, false, false, false), Ok(()), "mini may run on plain HTTP");
        assert_eq!(require_providers(false, true, false, false), Ok(()), "dev mode refuses secure cookies");
    }

    #[test]
    fn a_server_outside_mini_refuses_proxy_sign_in() {
        let why = require_providers(false, false, true, true).unwrap_err();
        assert!(why.contains(crate::proxy_auth::HEADER_VAR) && why.contains(crate::proxy_auth::FROM_VAR), "{why}");
        assert!(why.contains("SKRONGLE_MINI=1"), "{why}");
        assert_eq!(require_providers(true, false, false, true), Ok(()), "proxy sign-in is a mini feature");
        assert_eq!(require_providers(false, true, false, true), Ok(()), "dev mode may try it");
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
