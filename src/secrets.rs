//! The server's AI API keys (Anthropic, OpenAI, Google, DeepSeek, Mistral,
//! xAI, OpenRouter; and an Ollama-compatible server's base URL, #24), encrypted at rest,
//! and the vault that seals them, which also seals authenticator apps'
//! TOTP secrets (#292, [`seal_totp`]).
//!
//! One set of keys serves every account (table `server_api_keys`, #25);
//! only owners see, add or remove them, on the Manage Users page. Each key
//! is sealed with ChaCha20-Poly1305 under a key derived from a server-side
//! master secret, with the provider as associated data, so a ciphertext
//! cannot be read by another process that only has the database, nor moved
//! to another row. The master secret comes
//! from `SKRONGLE_SECRET_KEY` (64 hex characters) or, when that is unset, from a
//! file next to the database (`<db>.key`, created on first start with mode
//! 0600). A key is written once and never sent back to the browser: the
//! Manage Users page only shows its last four characters.
//!
//! Threat model, honestly: this separates the database file (backups, a
//! copied SD card, a stray `.db` in a download folder) from the secret. Root
//! on the running box has both and can read everything; the same is true of
//! the passwords' argon2 hashes being only as safe as the box itself.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};

// ---------- providers ----------

/// The AI providers the server can hold a key for (#24). Every one but
/// Anthropic speaks the OpenAI chat-completions format (`ai::Wire`);
/// `Ollama` stands for any OpenAI-compatible server the owner runs, and
/// takes its base URL where the others take a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Google,
    DeepSeek,
    Mistral,
    Xai,
    OpenRouter,
    Ollama,
}

impl Provider {
    /// In the order the Manage Users page lists them (and the AI forms offer them).
    pub const ALL: [Provider; 8] = [
        Provider::Anthropic,
        Provider::OpenAi,
        Provider::Google,
        Provider::DeepSeek,
        Provider::Mistral,
        Provider::Xai,
        Provider::OpenRouter,
        Provider::Ollama,
    ];

    /// What `server_api_keys.provider` stores and the forms post.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::Google => "google",
            Provider::DeepSeek => "deepseek",
            Provider::Mistral => "mistral",
            Provider::Xai => "xai",
            Provider::OpenRouter => "openrouter",
            Provider::Ollama => "ollama",
        }
    }

    pub fn parse(id: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Claude (Anthropic)",
            Provider::OpenAi => "OpenAI",
            Provider::Google => "Google (Gemini)",
            Provider::DeepSeek => "DeepSeek",
            Provider::Mistral => "Mistral",
            Provider::Xai => "xAI (Grok)",
            Provider::OpenRouter => "OpenRouter",
            Provider::Ollama => "Ollama (OpenAI-compatible server)",
        }
    }

    /// Where to create a key (for Ollama: where to get the server).
    pub fn console_url(self) -> &'static str {
        match self {
            Provider::Anthropic => "https://console.anthropic.com/settings/keys",
            Provider::OpenAi => "https://platform.openai.com/api-keys",
            Provider::Google => "https://aistudio.google.com/apikey",
            Provider::DeepSeek => "https://platform.deepseek.com/api_keys",
            Provider::Mistral => "https://console.mistral.ai/api-keys",
            Provider::Xai => "https://console.x.ai",
            Provider::OpenRouter => "https://openrouter.ai/keys",
            Provider::Ollama => "https://ollama.com/download",
        }
    }

    /// How every key from this provider starts; used to catch a key pasted
    /// into the wrong box. `""`: its keys have no fixed start (Mistral's),
    /// or it takes no key (Ollama).
    pub fn prefix(self) -> &'static str {
        match self {
            Provider::Anthropic => "sk-ant-",
            Provider::OpenAi | Provider::DeepSeek => "sk-",
            Provider::Google => "AIza",
            Provider::Xai => "xai-",
            Provider::OpenRouter => "sk-or-",
            Provider::Mistral | Provider::Ollama => "",
        }
    }

    /// The key format hint in the empty box.
    pub fn placeholder(self) -> &'static str {
        match self {
            Provider::Anthropic => "sk-ant-…",
            Provider::OpenAi | Provider::DeepSeek => "sk-…",
            Provider::Google => "AIza…",
            Provider::Mistral => "…",
            Provider::Xai => "xai-…",
            Provider::OpenRouter => "sk-or-…",
            Provider::Ollama => "http://localhost:11434",
        }
    }

    /// Ollama takes a base URL (and a model name) instead of a key.
    pub fn takes_url(self) -> bool {
        self == Provider::Ollama
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

pub const KEY_MAX: usize = 400;

/// A pasted key: trimmed, printable ASCII without spaces, at most `KEY_MAX`
/// characters, and starting the way this provider's keys do. For Ollama,
/// an `http://` or `https://` base URL: the owner's own server, so a
/// private address and any port are fine (only an owner sets it).
pub fn validate_key(provider: Provider, raw: &str) -> Result<String, String> {
    let v = raw.trim();
    if v.is_empty() {
        return Err(crate::t(if provider.takes_url() { "manage.api_keys.empty_url" } else { "manage.api_keys.empty" }));
    }
    if v.len() > KEY_MAX {
        return Err(crate::t1("manage.api_keys.too_long", "max", KEY_MAX));
    }
    if !v.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(crate::t("manage.api_keys.bad_chars"));
    }
    if provider.takes_url() {
        let rest = v.strip_prefix("http://").or_else(|| v.strip_prefix("https://"));
        return match rest {
            Some(r) if !r.is_empty() && !r.starts_with('/') => Ok(v.trim_end_matches('/').to_string()),
            _ => Err(crate::t("manage.api_keys.bad_url")),
        };
    }
    let wrong_box = match provider {
        Provider::OpenAi | Provider::DeepSeek => v.starts_with(Provider::Anthropic.prefix()) || v.starts_with(Provider::OpenRouter.prefix()),
        _ => false,
    };
    if wrong_box || !v.starts_with(provider.prefix()) {
        return Err(crate::t2("manage.api_keys.wrong_provider", "provider", provider.label(), "prefix", provider.prefix()));
    }
    Ok(v.to_string())
}

/// The model Ollama runs (`settings` row `ollama_model`): a name as
/// `ollama pull` takes it (`llama3.1`, `qwen2.5:7b`), at most 100
/// printable characters without spaces.
pub fn validate_model(raw: &str) -> Result<String, String> {
    let v = raw.trim();
    if v.is_empty() || v.len() > 100 || !v.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(crate::t("manage.api_keys.bad_model"));
    }
    Ok(v.to_string())
}

// ---------- the vault: master secret and sealing ----------

const MASTER_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const BLOB_VERSION: u8 = 1;

/// Holds the derived encryption key. Cheap to clone into `Db::call` closures.
#[derive(Clone)]
pub struct Vault {
    cipher: ChaCha20Poly1305,
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Vault(..)")
    }
}

impl Vault {
    /// Derive the working key from the master secret. The label separates
    /// this use from any future one of the same secret.
    pub fn from_master(master: &[u8; MASTER_LEN]) -> Vault {
        let key = Sha256::new().chain_update(b"learnmap/api-keys/v1").chain_update(master).finalize();
        Vault { cipher: ChaCha20Poly1305::new(&key) }
    }

    /// The master secret from `SKRONGLE_SECRET_KEY`, or from `<db>.key` next to
    /// the database (generated on first start). Returns where it came from.
    pub fn load(env_value: Option<String>, db_path: &Path) -> Result<(Vault, String), String> {
        if let Some(hex) = env_value {
            let master = decode_master(&hex).map_err(|e| format!("SKRONGLE_SECRET_KEY: {e}"))?;
            return Ok((Vault::from_master(&master), "SKRONGLE_SECRET_KEY".to_string()));
        }
        let path = key_file_path(db_path);
        let master = match std::fs::read_to_string(&path) {
            Ok(text) => {
                tighten_key_file(&path);
                decode_master(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut master = [0u8; MASTER_LEN];
                crate::random::fill(&mut master);
                write_key_file(&path, &master).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
                master
            }
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        Ok((Vault::from_master(&master), path.display().to_string()))
    }

    /// `version || nonce || ciphertext+tag`. A fresh random nonce every time.
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        crate::random::fill(&mut nonce);
        let ct = self
            .cipher
            .encrypt(&Nonce::from(nonce), Payload { msg: plaintext, aad })
            .expect("ChaCha20-Poly1305 encryption cannot fail");
        let mut blob = Vec::with_capacity(1 + NONCE_LEN + ct.len());
        blob.push(BLOB_VERSION);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        blob
    }

    /// `None` when the blob is malformed, tampered with, bound to a
    /// different user/provider, or sealed under another master secret.
    fn open(&self, aad: &[u8], blob: &[u8]) -> Option<Vec<u8>> {
        if blob.len() < 1 + NONCE_LEN + 16 || blob[0] != BLOB_VERSION {
            return None;
        }
        let (nonce, ct) = blob[1..].split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce).ok()?;
        self.cipher.decrypt(&nonce, Payload { msg: ct, aad }).ok()
    }
}

/// `learn.db` -> `learn.db.key`, in the same directory.
pub fn key_file_path(db_path: &Path) -> PathBuf {
    let mut name = db_path.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "learn.db".into());
    name.push(".key");
    db_path.with_file_name(name)
}

fn write_key_file(path: &Path, master: &[u8; MASTER_LEN]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(hex(master).as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_all()
}

/// A key file others can read (copied in by hand, restored from a backup)
/// is made owner-only again, with a warning.
fn tighten_key_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::metadata(path) else { return };
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            match std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
                Ok(()) => tracing::warn!("{} was readable by others (mode {mode:o}); set it to 600", path.display()),
                Err(e) => tracing::warn!("{} is readable by others (mode {mode:o}) and could not be changed: {e}", path.display()),
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn decode_master(text: &str) -> Result<[u8; MASTER_LEN], String> {
    let text = text.trim();
    if text.len() != MASTER_LEN * 2 {
        return Err(format!("expected {} hex characters, found {}", MASTER_LEN * 2, text.len()));
    }
    let mut out = [0u8; MASTER_LEN];
    for (i, pair) in text.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(pair).map_err(|_| "not hex".to_string())?;
        out[i] = u8::from_str_radix(s, 16).map_err(|_| "not hex".to_string())?;
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

// ---------- authenticator apps' secrets (two_factor.rs) ----------

/// Associated data for a TOTP secret: bound to its owner, and kept apart
/// from the API keys sealed under the same vault.
fn totp_aad(user_id: i64) -> Vec<u8> {
    format!("learnmap/totp-secret/v1/{user_id}").into_bytes()
}

/// An authenticator app's secret as `totp_keys.secret` stores it.
pub fn seal_totp(vault: &Vault, user_id: i64, secret: &[u8]) -> Vec<u8> {
    vault.seal(&totp_aad(user_id), secret)
}

/// The secret back; `None` when the row is someone else's, tampered with,
/// or sealed under another master secret (the app must then be set up again).
pub fn open_totp(vault: &Vault, user_id: i64, blob: &[u8]) -> Option<Vec<u8>> {
    vault.open(&totp_aad(user_id), blob)
}

// ---------- persistence (run inside Db::call) ----------

/// Associated data of a key saved for the server (`server_api_keys`, #25).
fn server_aad(provider: Provider) -> Vec<u8> {
    format!("learnmap/server-api-key/v1/{}", provider.id()).into_bytes()
}

/// What the Manage Users page shows about one provider. Never the key itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStatus {
    pub provider: Provider,
    /// Last four characters of the stored key; `None` when none is stored.
    pub hint: Option<String>,
    /// A row exists but no longer decrypts (the master secret changed).
    pub unreadable: bool,
    pub updated_at: i64,
}

impl KeyStatus {
    pub fn is_set(&self) -> bool {
        self.hint.is_some() || self.unreadable
    }
}

type Row = (String, Vec<u8>, i64);

fn rows(conn: &Connection) -> rusqlite::Result<Vec<Row>> {
    let mut stmt = conn.prepare_cached("SELECT provider, ciphertext, set_at FROM server_api_keys")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

/// The server's keys: one entry per provider, in `Provider::ALL` order.
pub fn statuses(conn: &Connection, vault: &Vault) -> rusqlite::Result<Vec<KeyStatus>> {
    let rows = rows(conn)?;
    Ok(Provider::ALL
        .into_iter()
        .map(|p| match rows.iter().find(|(id, ..)| id == p.id()) {
            None => KeyStatus { provider: p, hint: None, unreadable: false, updated_at: 0 },
            Some((_, blob, at)) => match vault.open(&server_aad(p), blob).and_then(|b| String::from_utf8(b).ok()) {
                Some(key) => KeyStatus { provider: p, hint: Some(hint(p, &key)), unreadable: false, updated_at: *at },
                None => KeyStatus { provider: p, hint: None, unreadable: true, updated_at: *at },
            },
        })
        .collect())
}

/// The providers with a readable server key, in `Provider::ALL` order: what
/// the AI forms offer.
pub fn usable(conn: &Connection, vault: &Vault) -> rusqlite::Result<Vec<Provider>> {
    Ok(statuses(conn, vault)?.into_iter().filter(|k| k.hint.is_some()).map(|k| k.provider).collect())
}

/// The server's key for a provider, for a feature that is about to call it.
/// `None` when there is none or it no longer decrypts.
pub fn get(conn: &Connection, vault: &Vault, provider: Provider) -> rusqlite::Result<Option<String>> {
    let blob: Option<Vec<u8>> =
        conn.query_row("SELECT ciphertext FROM server_api_keys WHERE provider = ?1", [provider.id()], |r| r.get(0)).optional()?;
    Ok(blob.and_then(|b| vault.open(&server_aad(provider), &b)).and_then(|b| String::from_utf8(b).ok()))
}

/// Store (or replace) the server's key for a provider, one that
/// `validate_key` accepted. Owners only: the caller checks.
pub fn save(conn: &Connection, vault: &Vault, provider: Provider, key: &str) -> rusqlite::Result<()> {
    let blob = vault.seal(&server_aad(provider), key.as_bytes());
    conn.execute(
        "INSERT INTO server_api_keys (provider, ciphertext, set_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(provider) DO UPDATE SET ciphertext = excluded.ciphertext, set_at = excluded.set_at",
        params![provider.id(), blob, crate::db::now()],
    )?;
    Ok(())
}

/// Delete the server's key for a provider. `false` when there was none.
pub fn remove(conn: &Connection, provider: Provider) -> rusqlite::Result<bool> {
    let n = conn.execute("DELETE FROM server_api_keys WHERE provider = ?1", [provider.id()])?;
    Ok(n == 1)
}

/// How many AI keys the server holds.
pub fn saved_key_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM server_api_keys", [], |r| r.get(0))
}

/// Delete every saved key `vault` cannot open: after the secret key was
/// lost and a new one created (`SKRONGLE_NEW_SECRET_KEY=1`). Returns how many.
pub fn forget_unreadable(conn: &Connection, vault: &Vault) -> rusqlite::Result<usize> {
    let mut n = 0;
    for (provider, blob, _) in rows(conn)? {
        if !row_readable(vault, &provider, &blob) {
            n += conn.execute("DELETE FROM server_api_keys WHERE provider = ?1", [&provider])?;
        }
    }
    Ok(n)
}

/// Turn off the authenticator app of every account whose app secret
/// `vault` cannot open, as `owner::disable_two_factor` does: after the
/// secret key was replaced (`SKRONGLE_NEW_SECRET_KEY=1`, #1068) those
/// learners could not finish signing in. They sign in with the password
/// alone and set the app up again. Returns how many accounts.
pub fn forget_unreadable_totp(conn: &Connection, vault: &Vault) -> rusqlite::Result<usize> {
    let rows: Vec<(i64, Vec<u8>)> = conn.prepare("SELECT user_id, secret FROM totp_keys")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut n = 0;
    for (user_id, blob) in rows {
        if open_totp(vault, user_id, &blob).is_none() {
            crate::owner::disable_two_factor(conn, user_id)?;
            n += 1;
        }
    }
    Ok(n)
}

/// How many saved keys `vault` opens, out of how many there are. None of
/// several opening means the secret key is not the one they were sealed
/// with (`SKRONGLE_SECRET_KEY` from another install, a `.key` file from
/// another backup).
pub fn readable_count(conn: &Connection, vault: &Vault) -> rusqlite::Result<(usize, usize)> {
    let rows = rows(conn)?;
    let readable = rows.iter().filter(|(p, b, _)| row_readable(vault, p, b)).count();
    Ok((readable, rows.len()))
}

fn row_readable(vault: &Vault, provider: &str, blob: &[u8]) -> bool {
    Provider::parse(provider).is_some_and(|p| vault.open(&server_aad(p), blob).is_some())
}

/// The vault the server would use, from `SKRONGLE_SECRET_KEY` or an existing
/// `<db>.key`, without creating anything (for `skrongle doctor`). `None`
/// when there is no usable secret.
pub fn existing_vault(env_value: Option<&str>, db_path: &Path) -> Option<Vault> {
    let text = match env_value {
        Some(hex) => hex.to_string(),
        None => std::fs::read_to_string(key_file_path(db_path)).ok()?,
    };
    decode_master(&text).ok().map(|m| Vault::from_master(&m))
}

/// Where the secret key would come from, checked without creating it (for
/// `skrongle doctor`). Never carries the secret itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MasterStatus {
    /// `SKRONGLE_SECRET_KEY` is set and well-formed.
    Env,
    /// The key file next to the database exists and is well-formed.
    File(PathBuf),
    /// Neither: the server creates the file at its next start.
    Missing(PathBuf),
    /// Set but unusable; the reason names lengths, never characters.
    Invalid(String),
}

pub fn master_status(env_value: Option<&str>, db_path: &Path) -> MasterStatus {
    if let Some(hex) = env_value {
        return match decode_master(hex) {
            Ok(_) => MasterStatus::Env,
            Err(e) => MasterStatus::Invalid(format!("SKRONGLE_SECRET_KEY: {e}")),
        };
    }
    let path = key_file_path(db_path);
    match std::fs::read_to_string(&path) {
        Ok(text) => match decode_master(&text) {
            Ok(_) => MasterStatus::File(path),
            Err(e) => MasterStatus::Invalid(format!("{}: {e}", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => MasterStatus::Missing(path),
        Err(e) => MasterStatus::Invalid(format!("cannot read {}: {e}", path.display())),
    }
}

/// What the page shows of a stored value: a key's last four characters,
/// or for a base URL (Ollama) its host and port, without any user name or
/// password it carries.
fn hint(provider: Provider, key: &str) -> String {
    if provider.takes_url() {
        let rest = key.split_once("://").map_or(key, |(_, r)| r);
        let host = rest.split('/').next().unwrap_or(rest);
        return host.rsplit_once('@').map_or(host, |(_, h)| h).to_string();
    }
    let n = key.chars().count();
    key.chars().skip(n.saturating_sub(4)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(seed: u8) -> Vault {
        Vault::from_master(&[seed; MASTER_LEN])
    }

    #[test]
    fn providers_roundtrip_ids() {
        for p in Provider::ALL {
            assert_eq!(Provider::parse(p.id()), Some(p));
            assert_eq!(p.to_string(), p.id());
            assert!(p.console_url().starts_with("https://") && !p.label().is_empty());
        }
        assert_eq!(Provider::parse("azure"), None);
    }

    #[test]
    fn validates_keys() {
        assert_eq!(validate_key(Provider::Anthropic, "  sk-ant-api03-abc ").unwrap(), "sk-ant-api03-abc");
        assert_eq!(validate_key(Provider::OpenAi, "sk-proj-abc").unwrap(), "sk-proj-abc");
        assert_eq!(validate_key(Provider::Google, "AIzaSyAbc").unwrap(), "AIzaSyAbc");
        assert!(validate_key(Provider::Anthropic, "").is_err());
        assert!(validate_key(Provider::Anthropic, "sk-ant-a b").is_err());
        assert!(validate_key(Provider::Anthropic, "sk-ant-é").is_err());
        assert!(validate_key(Provider::Anthropic, &format!("sk-ant-{}", "x".repeat(KEY_MAX))).is_err());
        // Pasted into the wrong box.
        assert!(validate_key(Provider::Anthropic, "sk-proj-abc").is_err());
        assert!(validate_key(Provider::OpenAi, "sk-ant-api03-abc").is_err());
        assert!(validate_key(Provider::Google, "sk-abc").is_err());
    }

    /// The providers #24 added: each key format, a key for another box
    /// refused, and Ollama's base URL instead of a key, shown by its host.
    #[test]
    fn validates_the_newer_providers() {
        assert_eq!(validate_key(Provider::DeepSeek, "sk-abc123").unwrap(), "sk-abc123");
        assert!(validate_key(Provider::DeepSeek, "sk-ant-api03-abc").is_err(), "an Anthropic key");
        assert!(validate_key(Provider::DeepSeek, "sk-or-v1-abc").is_err(), "an OpenRouter key");
        assert!(validate_key(Provider::OpenAi, "sk-or-v1-abc").is_err(), "an OpenRouter key");
        assert_eq!(validate_key(Provider::Mistral, &"x".repeat(32)).unwrap().len(), 32);
        assert_eq!(validate_key(Provider::Xai, "xai-abc").unwrap(), "xai-abc");
        assert!(validate_key(Provider::Xai, "sk-abc").is_err());
        assert_eq!(validate_key(Provider::OpenRouter, "sk-or-v1-abc").unwrap(), "sk-or-v1-abc");
        assert!(validate_key(Provider::OpenRouter, "sk-abc").is_err());
        assert_eq!(validate_key(Provider::Ollama, " http://192.168.1.20:11434/ ").unwrap(), "http://192.168.1.20:11434");
        assert_eq!(validate_key(Provider::Ollama, "https://ollama.example.org").unwrap(), "https://ollama.example.org");
        for bad in ["localhost:11434", "ftp://x", "http://", "http:///x", "http://a b"] {
            assert!(validate_key(Provider::Ollama, bad).is_err(), "{bad}");
        }
        assert_eq!(hint(Provider::Ollama, "http://me:pw@192.168.1.20:11434/v1"), "192.168.1.20:11434");
        assert_eq!(hint(Provider::Xai, "xai-abcdWXYZ"), "WXYZ");
        assert_eq!(validate_model(" qwen2.5:7b ").unwrap(), "qwen2.5:7b");
        assert!(validate_model("").is_err() && validate_model("two words").is_err() && validate_model(&"m".repeat(101)).is_err());
        assert!(Provider::ALL.iter().all(|p| p.takes_url() == (*p == Provider::Ollama)));
    }

    #[test]
    fn seal_and_open() {
        let v = vault(1);
        let blob = v.seal(b"ctx", b"sk-ant-secret");
        assert_eq!(blob[0], BLOB_VERSION);
        assert_eq!(v.open(b"ctx", &blob).as_deref(), Some(&b"sk-ant-secret"[..]));
        // Different nonce each time, same plaintext.
        assert_ne!(v.seal(b"ctx", b"sk-ant-secret"), blob);
        // Wrong context, wrong secret, tampering, truncation.
        assert_eq!(v.open(b"other", &blob), None);
        assert_eq!(vault(2).open(b"ctx", &blob), None);
        let mut bad = blob.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(v.open(b"ctx", &bad), None);
        assert_eq!(v.open(b"ctx", &blob[..20]), None);
        let mut other_version = blob.clone();
        other_version[0] = 2;
        assert_eq!(v.open(b"ctx", &other_version), None);
    }

    #[test]
    fn totp_secrets_are_sealed_per_user() {
        let v = vault(3);
        let blob = seal_totp(&v, 5, b"0123456789abcdefghij");
        assert!(!blob.windows(20).any(|w| w == b"0123456789abcdefghij"));
        assert_eq!(open_totp(&v, 5, &blob).as_deref(), Some(&b"0123456789abcdefghij"[..]));
        assert_eq!(open_totp(&v, 6, &blob), None, "another user's row");
        assert_eq!(open_totp(&vault(4), 5, &blob), None, "another master secret");
        // Not interchangeable with an API key's ciphertext.
        assert_eq!(v.open(&server_aad(Provider::Anthropic), &blob), None);
    }

    #[test]
    fn master_hex_roundtrip_and_key_file() {
        let master = [0xabu8; MASTER_LEN];
        assert_eq!(decode_master(&hex(&master)).unwrap(), master);
        assert!(decode_master("abc").is_err());
        assert!(decode_master(&"zz".repeat(MASTER_LEN)).is_err());

        let dir = std::env::temp_dir().join(format!("learnmap-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("learn.db");
        assert_eq!(key_file_path(&db), dir.join("learn.db.key"));
        let _ = std::fs::remove_file(key_file_path(&db));

        let (v1, from1) = Vault::load(None, &db).unwrap();
        assert_eq!(from1, key_file_path(&db).display().to_string());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(key_file_path(&db)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // A key file someone made world-readable is made owner-only again.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(key_file_path(&db), std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        // A second start reads the same secret back.
        let (v2, _) = Vault::load(None, &db).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(key_file_path(&db)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let blob = v1.seal(b"a", b"x");
        assert_eq!(v2.open(b"a", &blob).as_deref(), Some(&b"x"[..]));
        // The environment variable wins over the file.
        let (v3, from3) = Vault::load(Some(hex(&master)), &db).unwrap();
        assert_eq!(from3, "SKRONGLE_SECRET_KEY");
        assert_eq!(v3.open(b"a", &blob), None);
        assert!(Vault::load(Some("nope".into()), &db).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn server_keys_roundtrip() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let v = vault(7);
        db.with(|c| {
            let s = statuses(c, &v)?;
            assert_eq!(s.len(), Provider::ALL.len());
            assert!(s.iter().all(|k| !k.is_set()));

            save(c, &v, Provider::Anthropic, "sk-ant-api03-abcdWXYZ")?;
            assert_eq!(get(c, &v, Provider::Anthropic)?.as_deref(), Some("sk-ant-api03-abcdWXYZ"));
            assert_eq!(get(c, &v, Provider::OpenAi)?, None);
            let s = statuses(c, &v)?;
            assert_eq!(s[0].hint.as_deref(), Some("WXYZ"));
            assert!(s[0].is_set() && !s[0].unreadable && s[0].updated_at > 0);
            assert!(!s[1].is_set() && !s[2].is_set());
            assert_eq!(usable(c, &v)?, vec![Provider::Anthropic]);
            assert_eq!(saved_key_count(c)?, 1);

            // Replacing, then removing.
            save(c, &v, Provider::Anthropic, "sk-ant-api03-1234")?;
            assert_eq!(statuses(c, &v)?[0].hint.as_deref(), Some("1234"));
            assert!(remove(c, Provider::Anthropic)?);
            assert!(!remove(c, Provider::Anthropic)?);
            assert_eq!(get(c, &v, Provider::Anthropic)?, None);

            // A ciphertext moved to another provider's row does not open there.
            save(c, &v, Provider::Google, "AIzaSyAbc")?;
            c.execute("INSERT INTO server_api_keys (provider, ciphertext, set_at) SELECT 'openai', ciphertext, set_at FROM server_api_keys WHERE provider = 'google'", [])?;
            assert_eq!(get(c, &v, Provider::OpenAi)?, None);
            let s = statuses(c, &v)?;
            assert!(s[1].unreadable && s[1].is_set() && s[1].hint.is_none());
            assert_eq!(readable_count(c, &v)?, (1, 2));
            // Another master secret cannot read it either.
            assert_eq!(get(c, &vault(8), Provider::Google)?, None);
            assert_eq!(forget_unreadable(c, &v)?, 1);
            assert_eq!(readable_count(c, &v)?, (1, 1));
            Ok(())
        })
        .unwrap();
    }

    /// A new secret key (#1068): the authenticator apps it cannot read are
    /// turned off; one sealed under the new key stays.
    #[test]
    fn a_new_secret_key_turns_off_the_apps_it_cannot_read() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let (old, new) = (vault(1), vault(2));
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            for (u, v) in [(ann, &old), (bob, &new)] {
                c.execute("UPDATE users SET two_factor = 'totp' WHERE id = ?1", [u])?;
                c.execute("INSERT INTO totp_keys (user_id, secret, created_at, confirmed_at) VALUES (?1, ?2, 1, 1)", rusqlite::params![u, seal_totp(v, u, b"0123456789abcdefghij")])?;
            }
            assert_eq!(forget_unreadable_totp(c, &new)?, 1);
            let left: Vec<i64> = c.prepare("SELECT user_id FROM totp_keys")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            assert_eq!(left, vec![bob]);
            let method: String = c.query_row("SELECT two_factor FROM users WHERE id = ?1", [ann], |r| r.get(0))?;
            assert_eq!(method, "");
            Ok(())
        })
        .unwrap();
    }
}
