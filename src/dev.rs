//! Local development and testing aids, all off unless `SKRONGLEDEV=1`.
//!
//! - A fixed dev user (`/dev/login`, `/dev/reset`, the startup seed), with
//!   API keys for every provider that the simulated provider accepts.
//! - Where model calls go (`SKRONGLE_LLM_MODE`): by default the providers'
//!   default base URLs point at this server's simulated Anthropic, OpenAI
//!   and Gemini APIs (`dev_ai`, `/dev/ai/…`), which answer from the LLM
//!   fixtures over the real wire protocols, with usage limits, injected
//!   failures and requests held for a hand-written reply (Manual replies). The
//!   app's own code does not know: it calls a base URL with a key.
//!
//! `SKRONGLE_DEV` is honoured only on a loopback bind without secure cookies,
//! so a deploy (which binds 0.0.0.0 behind TLS) can never switch it on by
//! accident, and only on a database that holds no account but the dev
//! user's (or with `SKRONGLE_DEV_FORCE=1`), so a real deploy's database never
//! meets `/dev/*` (#388). In dev mode every route refuses a request whose
//! `Host` is not this machine (421), which keeps DNS rebinding out.
//! `docs/testing.md` is the user's guide.

use rusqlite::Connection;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::db::Db;

pub const DEV_EMAIL: &str = "dev@example.com";
/// So the form login works too.
pub const DEV_PASSWORD: &str = "dev-password";
/// The course the seed (and `/dev/login?open=` without a value) opens.
pub const SEED_COURSE: &str = "spanish";

/// Whether dev mode may be on. `Err` says why a requested dev mode was refused.
pub fn resolve(requested: bool, bind: SocketAddr, secure_cookies: bool, public_url: Option<&str>) -> Result<bool, String> {
    if !requested {
        return Ok(false);
    }
    if !bind.ip().is_loopback() {
        return Err(format!("SKRONGLE_DEV=1 ignored: the bind address {bind} is not loopback (use 127.0.0.1)"));
    }
    if secure_cookies {
        return Err("SKRONGLE_DEV=1 ignored: SKRONGLE_SECURE_COOKIES=1 means this is a deploy".into());
    }
    // Deploys bind loopback too (behind nginx or Caddy) and set a public URL.
    if let Some(url) = public_url.filter(|u| !is_local_host(crate::ai::url_host_port(u).map(|(h, _)| h).as_deref().unwrap_or(""))) {
        return Err(format!("SKRONGLE_DEV=1 ignored: SKRONGLE_PUBLIC_URL={url} means this is a deploy"));
    }
    Ok(true)
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Accounts in the database other than the dev user's.
pub fn other_accounts(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM users WHERE email IS NULL OR lower(email) <> lower(?1)",
        [DEV_EMAIL],
        |r| r.get(0),
    )
}

/// Dev mode's second gate (#388): a database with real accounts in it is a
/// deploy's, whatever the environment says, unless `SKRONGLE_DEV_FORCE=1`
/// (`force`) says this one is a developer's own (`./.dev-start` sets it).
pub fn check_accounts(others: i64, force: bool) -> Result<(), String> {
    if others > 0 && !force {
        return Err(format!(
            "SKRONGLE_DEV=1 ignored: the database holds {others} account(s) besides {DEV_EMAIL}, so it looks like a deploy's; set SKRONGLE_DEV_FORCE=1 if it is a development database"
        ));
    }
    Ok(())
}

/// The host a request was made to (`Host`, else the URI's authority),
/// lowercased and without the port; empty when there is none.
fn request_host(req: &axum::extract::Request) -> String {
    let host = req.headers().get(axum::http::header::HOST).and_then(|v| v.to_str().ok()).or_else(|| req.uri().authority().map(|a| a.as_str())).unwrap_or("");
    crate::ai::url_host_port(&format!("http://{host}")).map(|(h, _)| h).unwrap_or_default()
}

/// Layer on every route in dev mode (#388): a request made to any host but
/// this machine (`localhost`, `127.0.0.1`, `[::1]`, any port) gets 421
/// Misdirected Request. A DNS-rebinding page reaches the dev server under
/// its own name, so it is refused before it can sign in with the dev password.
pub async fn local_host_only(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    use axum::response::IntoResponse;
    let host = request_host(&req);
    if !is_local_host(&host) {
        tracing::warn!("dev mode: refused {} made to host {host:?} (421)", req.uri().path());
        return (axum::http::StatusCode::MISDIRECTED_REQUEST, "Dev mode answers only requests made to localhost or 127.0.0.1.\n").into_response();
    }
    next.run(req).await
}

/// Route guard for `/dev/*`: answers only requests made to this machine
/// directly (a loopback `Host`, no proxy headers). Dev mode already needs a
/// loopback bind; this also keeps the routes shut if a reverse proxy in front
/// of a dev-mode server forwards the public's requests to it.
pub async fn local_only(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    use axum::response::IntoResponse;
    let h = req.headers();
    let proxied = ["x-forwarded-for", "x-forwarded-host", "x-real-ip", "forwarded"].iter().any(|n| h.contains_key(*n));
    let host = request_host(&req);
    if proxied || !is_local_host(&host) {
        tracing::warn!("refused {} from a non-local client (host {host:?}, proxied: {proxied})", req.uri().path());
        return crate::error::AppError::NotFound.into_response();
    }
    next.run(req).await
}

// ---------- browser auto-reload (`./.dev-start --watch`) ----------

/// `SKRONGLE_DEV_RELOAD=1` (set by `./.dev-start --watch`): every HTML page
/// polls `/dev/boot` and reloads itself when the answer changes, i.e. when
/// the watcher has restarted the server.
pub fn reload_wanted() -> bool {
    crate::env("DEV_RELOAD").as_deref() == Some("1")
}

/// This process's boot id: different after every restart.
pub fn boot_id() -> &'static str {
    static BOOT: OnceLock<String> = OnceLock::new();
    BOOT.get_or_init(|| format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)))
}

/// Appends the reload poller to every `text/html` response.
pub async fn inject_reload(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    use axum::response::IntoResponse;
    let res = next.run(req).await;
    let html = res.headers().get(axum::http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/html"));
    if !html {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let script = format!(
        "<script>(function(){{var b=\"{}\",u=\"{}\";setInterval(function(){{fetch(u,{{cache:\"no-store\"}}).then(function(r){{return r.ok?r.text():b}}).then(function(t){{if(t!==b)location.reload()}}).catch(function(){{}})}},1000)}})()</script>",
        boot_id(),
        crate::url("/dev/boot")
    );
    let mut out = bytes.to_vec();
    out.extend_from_slice(script.as_bytes());
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    axum::response::Response::from_parts(parts, axum::body::Body::from(out))
}

// ---------- where model calls go in dev mode ----------

/// `SKRONGLE_LLM_MODE`, read once at startup. Dev mode only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmMode {
    /// The default: the providers' default base URLs point at this server's
    /// simulated provider (`dev_ai`, `/dev/ai/…`), which answers from the
    /// fixtures. `dev` and the old `fixtures` mean this too.
    Simulator,
    /// The simulator, with Manual replies on from the start: each request
    /// is written to `SKRONGLE_LLM_DIR` and waits for a reply typed on `/dev/ai`.
    Capture,
    /// The real providers' public APIs, for trying real keys in dev mode.
    Live,
}

/// `SKRONGLE_LLM_DELAY_MS`: how long a canned reply takes when its fixture
/// names no `delay_ms`. `./.dev-start` sets 500 ms so clicking through
/// still shows the waiting states (#313); the test scripts start
/// the server without it.
static DELAY: OnceLock<Duration> = OnceLock::new();

pub fn canned_delay() -> Duration {
    DELAY.get().copied().unwrap_or_default()
}

/// Where this server's simulated providers listen
/// (`http://127.0.0.1:<port><base path>/dev/ai`). Unset outside dev mode
/// and with `SKRONGLE_LLM_MODE=live`.
static SIMULATOR: OnceLock<String> = OnceLock::new();

/// The simulated provider's base URL for `kind`, which
/// `ai::ProviderKind::default_base_url` uses unless a `SKRONGLE_*_BASE_URL`
/// says otherwise. `None` outside dev mode or with `live`: the public APIs.
pub fn provider_base_url(kind: crate::ai::ProviderKind) -> Option<String> {
    let base = SIMULATOR.get()?;
    Some(simulator_base_url(base, kind))
}

/// The base URL of the simulated provider for `kind` under `root`
/// (`http://127.0.0.1:8123/dev/ai`): what dev mode uses by default, and
/// what a learner would type as a custom base URL to reach it.
pub fn simulator_base_url(root: &str, kind: crate::ai::ProviderKind) -> String {
    use crate::ai::ProviderKind;
    match kind {
        ProviderKind::Anthropic => format!("{root}/anthropic"),
        ProviderKind::OpenAi => format!("{root}/openai/v1"),
        // Google's base URL ends in /openai and takes no /v1 (`ai::openai_url`).
        ProviderKind::Google => format!("{root}/google/v1beta/openai"),
        // The others speak OpenAI's format under a path of their own (#24),
        // so the call log tells them apart: `{base}/v1/chat/completions`,
        // OpenRouter's base ending in /v1 as its real one does.
        ProviderKind::DeepSeek => format!("{root}/deepseek"),
        ProviderKind::Mistral => format!("{root}/mistral"),
        ProviderKind::Xai => format!("{root}/xai"),
        ProviderKind::OpenRouter => format!("{root}/openrouter/api/v1"),
        ProviderKind::Ollama => format!("{root}/ollama"),
    }
}

/// Tests: point the default base URLs at a simulator already listening at `root`.
#[cfg(test)]
pub fn simulate_at(root: &str) {
    let _ = SIMULATOR.set(root.to_string());
}

/// Whether the model calls go to the simulator by default.
pub fn simulating() -> bool {
    SIMULATOR.get().is_some()
}

/// Read `SKRONGLE_LLM_MODE` (+ `_DELAY_MS`) once at startup. `None` without dev
/// mode, whatever the variables say: model calls go to the public APIs.
pub fn init_llm(dev: bool, bind: SocketAddr) -> Result<Option<LlmMode>, String> {
    let raw = crate::env("LLM_MODE").unwrap_or_default();
    if !dev {
        if !raw.is_empty() && raw != "live" {
            tracing::warn!("SKRONGLE_LLM_MODE={raw} ignored: dev mode is off (SKRONGLE_DEV=1 on a loopback bind)");
        }
        return Ok(None);
    }
    let ms = crate::env("LLM_DELAY_MS").and_then(|v| v.parse().ok()).unwrap_or(0);
    let _ = DELAY.set(Duration::from_millis(ms));
    let mode = match raw.as_str() {
        "" | "dev" | "simulator" => LlmMode::Simulator,
        "fixtures" => {
            tracing::warn!("SKRONGLE_LLM_MODE=fixtures is gone: the simulated provider answers from the fixtures (the default)");
            LlmMode::Simulator
        }
        "capture" => LlmMode::Capture,
        "live" => LlmMode::Live,
        other => return Err(format!("SKRONGLE_LLM_MODE={other}: expected nothing (the simulated provider), capture or live")),
    };
    if mode != LlmMode::Live {
        if !fixtures_dir().is_dir() {
            return Err(format!("dev mode: no LLM fixture directory at {} (SKRONGLE_LLM_FIXTURES); SKRONGLE_LLM_MODE=live skips the simulator", fixtures_dir().display()));
        }
        let _ = SIMULATOR.set(format!("http://127.0.0.1:{}{}", bind.port(), crate::url("/dev/ai")));
    }
    Ok(Some(mode))
}

/// Where the simulator reads canned replies.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(crate::env("LLM_FIXTURES").unwrap_or_else(|| "data/test-fixtures/llm".into()))
}

/// Where held requests (`SKRONGLE_LLM_MODE=capture`, Manual replies on
/// `/dev/ai`) are written and their replies read: `SKRONGLE_LLM_DIR`, `./dev-llm`.
pub fn capture_dir() -> PathBuf {
    PathBuf::from(crate::env("LLM_DIR").unwrap_or_else(|| "dev-llm".into()))
}

/// How long a held request waits for its reply: `SKRONGLE_LLM_TIMEOUT_SECS`, 600 s.
pub fn capture_timeout() -> Duration {
    crate::env("LLM_TIMEOUT_SECS").and_then(|v| v.parse().ok()).map(Duration::from_secs).unwrap_or(Duration::from_secs(600))
}

/// Fixture files edited on the dev AI page (`/dev/ai/fixtures/…`) for this
/// server run, by path: the simulator sees the edit until it is saved to
/// the file or reset.
fn fixture_edits() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, String>> {
    static EDITS: OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>> = OnceLock::new();
    EDITS.get_or_init(Default::default)
}

/// A fixture file's text: the in-memory edit, else the file.
pub fn fixture_text(path: &Path) -> std::io::Result<String> {
    if let Some(t) = fixture_edits().lock().unwrap_or_else(|p| p.into_inner()).get(path) {
        return Ok(t.clone());
    }
    std::fs::read_to_string(path)
}

/// Replace a fixture file's text for this server run (`None`: back to the file).
pub fn set_fixture_edit(path: &Path, text: Option<String>) {
    let mut edits = fixture_edits().lock().unwrap_or_else(|p| p.into_inner());
    match text {
        Some(t) => {
            edits.insert(path.to_path_buf(), t);
        }
        None => {
            edits.remove(path);
        }
    }
}

/// The fixture files edited in memory, by path.
pub fn fixture_edited(path: &Path) -> bool {
    fixture_edits().lock().unwrap_or_else(|p| p.into_inner()).contains_key(path)
}

/// The paths of edits whose file does not exist yet (a new purpose file).
pub fn fixture_edit_paths() -> Vec<PathBuf> {
    fixture_edits().lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect()
}

/// Poll for a reply file; it counts once it is non-empty and its size held
/// still between two looks (writers should still rename into place).
pub async fn wait_for(path: &Path, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    let mut last_len = None;
    loop {
        if let Ok(meta) = std::fs::metadata(path) {
            let len = meta.len();
            if len > 0 && last_len == Some(len) {
                return std::fs::read_to_string(path).ok();
            }
            last_len = Some(len);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The first `[fixture:NAME]` token in these texts.
pub fn fixture_token<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<String> {
    for t in texts {
        if let Some(rest) = t.split("[fixture:").nth(1) {
            if let Some(name) = rest.split(']').next().filter(|n| !n.is_empty() && n.len() <= 64) {
                return Some(name.trim().to_string());
            }
        }
    }
    None
}

/// A held request on disk, for the Manual replies section of `/dev/ai`.
pub struct Captured {
    pub seq: u32,
    pub stem: String,
    pub request: serde_json::Value,
    pub response: Option<String>,
}

/// Every `<seq>-<purpose>.request.json` in the directory, oldest first.
pub fn list_requests(dir: &Path) -> Vec<Captured> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Captured> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".request.json")?.to_string();
            let seq = stem.split('-').next()?.parse().ok()?;
            let request = std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let response = std::fs::read_to_string(dir.join(format!("{stem}.response.txt"))).ok();
            Some(Captured { seq, stem, request, response })
        })
        .collect();
    out.sort_by_key(|c| c.seq);
    out
}

/// A safe request stem (`0003-generate-lesson`), as named in a URL.
pub fn valid_stem(stem: &str) -> bool {
    !stem.is_empty() && stem.len() < 80 && stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// ---------- the dev user ----------

/// The dev user's id, creating the account on first use. Blocking (argon2).
/// The dev user's address counts as confirmed, and it is an owner
/// (`users.owner`: the Manage Users page).
pub fn ensure_user(db: &Db) -> Result<i64, String> {
    let accept = |id: i64| {
        db.with(|c| {
            c.execute("UPDATE users SET owner = 1 WHERE id = ?1", [id])?;
            crate::profile::mark_verified(c, id, crate::db::now())
        })
        .map_err(|e| e.to_string())
    };
    if let Some((id, _)) = db.with(|c| crate::auth::find_user_credentials(c, DEV_EMAIL)).map_err(|e| e.to_string())? {
        accept(id)?;
        return Ok(id);
    }
    let hash = crate::auth::hash_password(DEV_PASSWORD).map_err(|e| format!("{e:?}"))?;
    let id = db.with(|c| crate::auth::create_user(c, DEV_EMAIL, &hash)).map_err(|e| e.to_string())?;
    accept(id)?;
    tracing::info!("dev: created user '{DEV_EMAIL}' (password '{DEV_PASSWORD}')");
    Ok(id)
}

/// Save the simulated provider's keys (`dev_ai::LEARNER_KEYS`), one for
/// every provider, as the server's keys: what the seed and
/// `/dev/login?keys=dev` (or `keys=fake`) store. With them every AI form is
/// unlocked for every account and offers each provider; the simulator
/// answers them all.
pub fn save_dev_keys(conn: &Connection, vault: &crate::secrets::Vault) -> rusqlite::Result<()> {
    for (p, k) in crate::dev_ai::LEARNER_KEYS {
        crate::secrets::save(conn, vault, p, k)?;
    }
    Ok(())
}

/// Every progress table, keyed by user.
const PROGRESS_TABLES: [&str; 13] = [
    "lesson_passes",
    "user_progress",
    "quiz_history",
    "journey_progress",
    "user_vocab",
    "game_scores",
    "game_activity",
    "game_rounds",
    "activity_log",
    "lesson_touches",
    "lesson_notes",
    "earnings",
    "motivator_purchases",
];

/// Wipe a user's progress and own courses (drafts and generated ones) and
/// switch the review of AI changes back to opening by itself and forget its
/// saved app language, so a `lang` cookie decides again (#1363); the account
/// (owner or not) and its sessions stay. Returns the deleted course ids.
pub fn reset_user(conn: &Connection, user_id: i64) -> rusqlite::Result<Vec<String>> {
    let owned: Vec<String> = crate::courses::courses_owned_by(conn, user_id)?.into_iter().map(|r| r.id).collect();
    for id in &owned {
        crate::courses::delete_course(conn, id)?;
    }
    for t in PROGRESS_TABLES.iter().chain(["projects"].iter()) {
        conn.execute(&format!("DELETE FROM {t} WHERE user_id = ?1"), [user_id])?;
    }
    crate::mastery::cache::forget_all();
    crate::courses::clear_active(conn, user_id)?;
    conn.execute("UPDATE users SET review_quiet = 0, lang = '' WHERE id = ?1", [user_id])?;
    Ok(owned)
}

/// A known starting state: the dev user with `SEED_COURSE` open, the first
/// two lessons passed and the third seen, and (with `keys`) the simulated
/// provider's keys for every provider. Progress is only added to a user
/// with none in that course, so restarts don't pile it up.
pub fn seed(db: &Db, courses: &crate::courses::Courses, keys: Option<&crate::secrets::Vault>) -> Result<String, String> {
    let uid = ensure_user(db)?;
    let id = courses.builtin_by_id(SEED_COURSE).or_else(|| courses.builtin().first()).map(|c| c.id().to_string()).ok_or("no built-in course to seed")?;
    let course = courses.builtin_store(&id).ok_or("the built-in course to seed does not load")?;
    let cid = course.id().to_string();
    db.with(|c| {
        crate::owner::make_owner(c, uid)?;
        crate::courses::open_project(c, uid, &cid)?;
        if let Some(vault) = keys {
            save_dev_keys(c, vault)?;
        }
        if !crate::progress::load_all(c, uid, &cid)?.is_empty() {
            return Ok(());
        }
        for (i, lesson) in course.lessons().iter().take(3).enumerate() {
            if i < 2 {
                let n = crate::progress::required_answers(lesson);
                crate::progress::record_taught_run(c, uid, &cid, lesson, crate::progress::VIA_STUDY, n, n)?;
            } else {
                crate::progress::mark_seen(c, uid, &cid, lesson, crate::progress::VIA_STUDY)?;
            }
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(cid)
}

// ---------- the dev authenticator ----------

/// One account the dev authenticator holds, as an app would after a scan.
#[derive(Clone, Debug)]
pub struct AuthAccount {
    pub id: i64,
    pub key: crate::totp::Key,
    pub added_at: i64,
}

/// The dev authenticator app (`/dev/authenticator`): the TOTP keys it was
/// given, oldest first, each showing its current code, worked out here
/// from the secret as an app does (`crate::totp`). In memory: a restart
/// forgets them.
#[derive(Default)]
pub struct Authenticator(std::sync::Mutex<(i64, Vec<AuthAccount>)>);

impl Authenticator {
    /// Add a key; one with the same secret, issuer and account replaces the
    /// old entry (scanning the same QR twice). Returns its id.
    pub fn add(&self, key: crate::totp::Key, now: i64) -> i64 {
        let mut g = self.0.lock().unwrap();
        g.1.retain(|a| !(a.key.secret == key.secret && a.key.issuer == key.issuer && a.key.account == key.account));
        g.0 += 1;
        let id = g.0;
        g.1.push(AuthAccount { id, key, added_at: now });
        id
    }

    pub fn list(&self) -> Vec<AuthAccount> {
        self.0.lock().unwrap().1.clone()
    }

    pub fn get(&self, id: i64) -> Option<AuthAccount> {
        self.0.lock().unwrap().1.iter().find(|a| a.id == id).cloned()
    }

    /// `false`: it had no such account.
    pub fn remove(&self, id: i64) -> bool {
        let mut g = self.0.lock().unwrap();
        let before = g.1.len();
        g.1.retain(|a| a.id != id);
        g.1.len() != before
    }
}

#[cfg(test)]
mod tests;
