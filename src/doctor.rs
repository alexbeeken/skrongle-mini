//! `skrongle doctor`: one screen of facts about an install, to paste into a
//! bug report (`.github/ISSUE_TEMPLATE/self-hosting-bug.yml`, linked from
//! `docs/self-hosting-faq.md`).
//!
//! It only reads: the database is opened read-only, nothing is migrated and
//! no key file is created. Secrets are reported as yes/no (a secret key,
//! learners' saved AI keys): their values, and
//! values that may embed credentials (URLs), never reach the output.

use std::fmt::Write as _;
use std::path::Path;

use crate::secrets::MasterStatus;
use crate::startup;
use crate::Config;

/// Run the command; the process exit code.
pub fn main(args: &[String]) -> i32 {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--env-file" => {
                let Some(path) = it.next() else {
                    eprintln!("--env-file needs a path, e.g. --env-file /etc/skrongle/env");
                    return 2;
                };
                match std::fs::read_to_string(path) {
                    // Like systemd's EnvironmentFile=: these win over the
                    // environment. Still single-threaded here.
                    Ok(text) => {
                        for (k, v) in parse_env_file(&text) {
                            std::env::set_var(k, v);
                        }
                    }
                    Err(e) => {
                        eprintln!("cannot read {path}: {e}");
                        return 1;
                    }
                }
            }
            "-h" | "--help" => {
                println!("{}", crate::USAGE);
                return 0;
            }
            other => {
                eprintln!("unknown doctor argument {other:?}\n\n{}", crate::USAGE);
                return 2;
            }
        }
    }
    let old_names = crate::old_prefix_warning_now();
    let mut lines = match Config::from_env() {
        Ok(cfg) => report(&cfg, &crate::env),
        Err(e) => vec![Line::new("config", format!("PROBLEM: {e}"))],
    };
    if let Some(w) = old_names {
        lines.push(Line::new("old names", w));
    }
    print!("{}", render(&lines));
    0
}

/// `NAME=value` lines as systemd's `EnvironmentFile=` reads them: blank lines
/// and `#`/`;` comments skipped, an optional `export `, surrounding quotes
/// removed.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (k, v) = line.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            let v = v.trim();
            let v = ['"', '\'']
                .iter()
                .find_map(|q| v.strip_prefix(*q).and_then(|r| r.strip_suffix(*q)))
                .unwrap_or(v);
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

pub struct Line {
    label: &'static str,
    value: String,
}

impl Line {
    fn new(label: &'static str, value: impl Into<String>) -> Line {
        Line { label, value: value.into() }
    }
}

/// The only way a secret's presence is reported.
fn yes_no(set: bool) -> &'static str {
    if set {
        "yes"
    } else {
        "no"
    }
}

/// Everything the doctor prints. `env(name)` answers for `SKRONGLE_<name>`.
pub fn report(cfg: &Config, env: &dyn Fn(&str) -> Option<String>) -> Vec<Line> {
    let mut out = Vec::new();
    out.push(Line::new("version", crate::VERSION));
    out.push(Line::new("os", os_line()));
    if let Ok(exe) = std::env::current_exe() {
        out.push(Line::new("binary", exe.display().to_string()));
    }
    out.push(Line::new("runs as", startup::who()));

    out.push(Line::new("listen", bind_line(cfg.bind)));
    out.push(Line::new("data dir", data_dir_line(&cfg.data_dir)));

    let db_abs = std::path::absolute(&cfg.db_path).unwrap_or_else(|_| cfg.db_path.clone());
    let db = &db_abs;
    out.push(Line::new("database", db.display().to_string()));
    out.push(Line::new("database size", db_size_line(db)));
    out.push(Line::new("db writable", match startup::check_db_writable(db) {
        Ok(()) => "yes".to_string(),
        Err(e) => format!("PROBLEM: {e}"),
    }));
    let facts = read_db(db);
    out.push(Line::new("schema", schema_line(&facts)));
    if let DbFacts::Open { users, saved_keys, .. } = &facts {
        out.push(Line::new("accounts", users.to_string()));
        out.push(Line::new("saved AI keys", format!("{saved_keys} (learners' keys, encrypted with the secret key)")));
    }

    let saved_keys = match &facts {
        DbFacts::Open { saved_keys, .. } => *saved_keys,
        _ => 0,
    };
    let mut secret = secret_line(cfg.secret_key.as_deref(), db, saved_keys);
    if saved_keys > 0 && secret.starts_with("yes") && !secret_opens_saved_keys(cfg.secret_key.as_deref(), db) {
        secret = "PROBLEM: the secret key cannot read the saved AI keys (wrong SKRONGLE_SECRET_KEY, or a .key file from another backup than the database)".to_string();
    }
    out.push(Line::new("secret key", secret));

    // AI: the keys themselves belong to learners (counted above). The server
    // only has a base URL for the OpenAI-compatible provider.
    out.push(Line::new("OpenAI base URL", match env("OPENAI_BASE_URL") {
        Some(_) => "custom (SKRONGLE_OPENAI_BASE_URL is set)",
        None => "default",
    }));
    out.push(Line::new("public URL set", yes_no(env("PUBLIC_URL").is_some())));
    out.push(Line::new("secure cookies", yes_no(cfg.secure_cookies)));
    if !cfg.base_path.is_empty() {
        out.push(Line::new("base path", cfg.base_path.clone()));
    }
    if cfg.dev {
        out.push(Line::new("dev mode", "asked for (SKRONGLE_DEV=1): only for local testing"));
    }

    out.push(Line::new("free disk", disk_line(&startup::db_dir(db))));
    out.push(Line::new("memory", memory_line()));
    out
}

pub fn render(lines: &[Line]) -> String {
    let width = lines.iter().map(|l| l.label.len()).max().unwrap_or(0);
    let mut s = String::from("Skrongle doctor (paste this into a bug report; it contains no secrets)\n\n");
    for l in lines {
        let _ = writeln!(s, "{:width$}  {}", l.label, l.value);
    }
    s
}

fn os_line() -> String {
    let mut s = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let pretty = std::fs::read_to_string("/etc/os-release").ok().and_then(|t| {
        t.lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_string())
    });
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|k| k.trim().to_string());
    match (pretty, kernel) {
        (Some(p), Some(k)) => {
            let _ = write!(s, " ({p}, kernel {k})");
        }
        (Some(p), None) => {
            let _ = write!(s, " ({p})");
        }
        (None, Some(k)) => {
            let _ = write!(s, " (kernel {k})");
        }
        (None, None) => {}
    }
    if let Some(model) = std::fs::read_to_string("/proc/device-tree/model").ok() {
        let _ = write!(s, ", {}", model.trim_end_matches('\0').trim());
    }
    s
}

fn bind_line(addr: std::net::SocketAddr) -> String {
    match std::net::TcpListener::bind(addr) {
        Ok(_) => format!("{addr} (port free: the server is not running)"),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            format!("{addr} (port in use: fine if Skrongle is running, otherwise another program has it)")
        }
        Err(e) => format!("{addr} (PROBLEM: {})", startup::bind_error(addr, &e)),
    }
}

fn data_dir_line(dir: &Path) -> String {
    let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !dir.is_dir() {
        return format!("{} (PROBLEM: missing; set SKRONGLE_DATA_DIR to the folder holding courses/)", abs.display());
    }
    let count = |sub: &str, file: &str| -> usize {
        std::fs::read_dir(dir.join(sub))
            .map(|rd| rd.filter_map(Result::ok).filter(|e| e.path().join(file).is_file()).count())
            .unwrap_or(0)
    };
    let courses = count("courses", crate::content::MANIFEST_FILE);
    let warn = if courses == 0 { " (PROBLEM: no built-in courses found)" } else { "" };
    format!("{} ({courses} courses){warn}", abs.display())
}

fn mb(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    }
}

fn db_size_line(db: &Path) -> String {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    let Some(main) = size(db) else {
        return "not created yet (the server creates it at its first start)".to_string();
    };
    let mut wal = db.as_os_str().to_os_string();
    wal.push("-wal");
    match size(Path::new(&wal)) {
        Some(w) if w > 0 => format!("{} (+ {} write-ahead log)", mb(main), mb(w)),
        _ => mb(main),
    }
}

enum DbFacts {
    Missing,
    Unreadable(String),
    Open { version: i64, users: i64, saved_keys: i64 },
}

fn read_db(db: &Path) -> DbFacts {
    use rusqlite::{Connection, OpenFlags};
    if !db.exists() {
        return DbFacts::Missing;
    }
    let conn = match Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX) {
        Ok(c) => c,
        Err(e) => return DbFacts::Unreadable(e.to_string()),
    };
    let version = match conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)) {
        Ok(v) => v,
        Err(e) => return DbFacts::Unreadable(e.to_string()),
    };
    let count = |table: &str| conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    DbFacts::Open { version, users: count("users"), saved_keys: count("user_api_keys") }
}

fn schema_line(facts: &DbFacts) -> String {
    let want = crate::db::schema_version();
    match facts {
        DbFacts::Missing => format!("- (this binary migrates to v{want})"),
        DbFacts::Unreadable(e) => format!("PROBLEM: cannot read the database: {e}"),
        DbFacts::Open { version, .. } if *version == want => format!("v{version} (up to date)"),
        DbFacts::Open { version, .. } if *version < want => {
            format!("v{version}; this binary applies v{} to v{want} at its next start", version + 1)
        }
        DbFacts::Open { version, .. } => format!(
            "PROBLEM: v{version} is newer than this binary knows (v{want}): it was used by a newer release; install that one again"
        ),
    }
}

fn secret_line(env_value: Option<&str>, db: &Path, saved_keys: i64) -> String {
    match crate::secrets::master_status(env_value, db) {
        MasterStatus::Env => "yes (from SKRONGLE_SECRET_KEY)".to_string(),
        MasterStatus::File(p) => format!("yes (from {})", p.display()),
        MasterStatus::Missing(p) if saved_keys > 0 => format!(
            "PROBLEM: no ({} is missing and SKRONGLE_SECRET_KEY is unset, but {saved_keys} saved AI key(s) need it: restore it from your backup)",
            p.display()
        ),
        MasterStatus::Missing(p) => format!("no (created as {} at the next start)", p.display()),
        MasterStatus::Invalid(why) => format!("PROBLEM: set but unusable: {why}"),
    }
}

/// Whether the secret key opens any of the saved AI keys (the startup check
/// `startup::check_secret_opens`, read-only). `true` when it cannot tell.
fn secret_opens_saved_keys(env_value: Option<&str>, db: &Path) -> bool {
    use rusqlite::{Connection, OpenFlags};
    let Some(vault) = crate::secrets::existing_vault(env_value, db) else { return true };
    let Ok(conn) = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX) else { return true };
    match crate::secrets::readable_count(&conn, &vault) {
        Ok((readable, total)) => total == 0 || readable > 0,
        Err(_) => true,
    }
}

/// Free and total space on the file system holding `dir`.
#[cfg(unix)]
fn disk_line(dir: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return "unknown".into() };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is NUL-terminated and `st` is a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return format!("unknown ({})", std::io::Error::last_os_error());
    }
    let frsize = st.f_frsize as u64;
    let free = st.f_bavail as u64 * frsize;
    let total = st.f_blocks as u64 * frsize;
    let warn = if free < 100 << 20 { " (PROBLEM: under 100 MB left)" } else { "" };
    format!("{} free of {} (at {}){warn}", mb(free), mb(total), dir.display())
}

#[cfg(not(unix))]
fn disk_line(_dir: &Path) -> String {
    "unknown".into()
}

/// From `/proc/meminfo` (Linux); "unknown" elsewhere.
fn memory_line() -> String {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else { return "unknown".into() };
    meminfo_line(&text)
}

fn meminfo_line(text: &str) -> String {
    let kb = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|r| r.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
    };
    match (kb("MemAvailable:"), kb("MemTotal:")) {
        (Some(a), Some(t)) => format!("{} available of {}", mb(a * 1024), mb(t * 1024)),
        _ => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const SECRET_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn lookup(map: HashMap<&'static str, String>) -> impl Fn(&str) -> Option<String> {
        move |name: &str| map.get(name).cloned()
    }

    /// Every secret the server can be given, set to a value we can look for.
    fn secrets(db: &Path) -> HashMap<&'static str, String> {
        HashMap::from([
            ("DB", db.display().to_string()),
            ("BIND", "127.0.0.1:0".to_string()),
            ("SECRET_KEY", SECRET_KEY.to_string()),
            ("PUBLIC_URL", "https://admin:hunter2@learn.example.org".to_string()),
            ("OPENAI_BASE_URL", "https://user:pa55word@llm.internal:11434".to_string()),
        ])
    }

    fn temp_db(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("skrongle-doctor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("learn.db")
    }

    #[test]
    fn no_secret_reaches_the_output() {
        let db = temp_db("redact");
        let map = secrets(&db);
        let env = lookup(map.clone());
        let cfg = Config::from_lookup(&env).unwrap();
        // A database with a learner's saved key, sealed under the secret.
        {
            let d = crate::db::Db::open(&db).unwrap();
            let (vault, _) = crate::secrets::Vault::load(Some(SECRET_KEY.into()), &db).unwrap();
            d.with(|c| {
                c.execute("INSERT INTO users (email, password_hash, created_at) VALUES ('ana@example.org', 'x', 0)", [])?;
                let uid = c.last_insert_rowid();
                crate::secrets::save(c, &vault, uid, crate::secrets::Provider::Anthropic, "sk-ant-learner-key-zz9")
            })
            .unwrap();
        }
        let out = render(&report(&cfg, &env));
        let mut needles: Vec<String> = ["SECRET_KEY", "PUBLIC_URL", "OPENAI_BASE_URL"]
            .iter()
            .map(|k| map[k].clone())
            .collect();
        needles.extend(["hunter2", "pa55word", "sk-ant-learner-key-zz9", "zz9", &SECRET_KEY[..16]].map(String::from));
        for n in &needles {
            assert!(!out.contains(n.as_str()), "doctor output leaks {n:?}:\n{out}");
        }
        // It still says what is configured.
        assert!(out.contains("yes (from SKRONGLE_SECRET_KEY)"), "{out}");
        assert!(out.lines().any(|l| l.starts_with("saved AI keys") && l.contains(" 1 ")), "{out}");
        assert!(out.contains(&format!("v{} (up to date)", crate::db::schema_version())), "{out}");
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn a_secret_key_that_cannot_read_the_saved_keys_is_a_problem() {
        let db = temp_db("wrongkey");
        // A learner's key sealed under another install's secret (the .key file of another backup).
        {
            let d = crate::db::Db::open(&db).unwrap();
            let other = crate::secrets::Vault::from_master(&[0xcd; 32]);
            d.with(|c| {
                let uid = crate::auth::create_user(c, "ana@example.org", "h")?;
                crate::secrets::save(c, &other, uid, crate::secrets::Provider::Anthropic, "sk-ant-learner-key-zz9")
            })
            .unwrap();
        }
        let env = lookup(HashMap::from([("DB", db.display().to_string()), ("BIND", "127.0.0.1:0".to_string()), ("SECRET_KEY", SECRET_KEY.to_string())]));
        let cfg = Config::from_lookup(&env).unwrap();
        let out = render(&report(&cfg, &env));
        let line = out.lines().find(|l| l.starts_with("secret key")).unwrap_or_default().to_string();
        assert!(line.contains("PROBLEM: the secret key cannot read the saved AI keys"), "{out}");
        assert!(!out.contains(&SECRET_KEY[..8]), "{out}");
        // The secret the key was saved with reads it: no problem.
        let right = lookup(HashMap::from([("DB", db.display().to_string()), ("BIND", "127.0.0.1:0".to_string()), ("SECRET_KEY", "cd".repeat(32))]));
        let cfg = Config::from_lookup(&right).unwrap();
        let out = render(&report(&cfg, &right));
        assert!(out.contains("yes (from SKRONGLE_SECRET_KEY)") && !out.contains("cannot read the saved"), "{out}");
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn key_file_contents_are_never_printed() {
        let db = temp_db("keyfile");
        std::fs::write(crate::secrets::key_file_path(&db), format!("{SECRET_KEY}\n")).unwrap();
        let env = lookup(HashMap::from([("DB", db.display().to_string()), ("BIND", "127.0.0.1:0".to_string())]));
        let cfg = Config::from_lookup(&env).unwrap();
        let out = render(&report(&cfg, &env));
        assert!(!out.contains(&SECRET_KEY[..8]), "{out}");
        assert!(out.contains("learn.db.key)"), "{out}");
        // A malformed key is reported by length only.
        std::fs::write(crate::secrets::key_file_path(&db), "deadbeef-not-a-key").unwrap();
        let out = render(&report(&cfg, &env));
        assert!(!out.contains("deadbeef"), "{out}");
        assert!(out.contains("PROBLEM: set but unusable"), "{out}");
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn a_fresh_install_is_described_without_creating_anything() {
        let db = temp_db("fresh");
        let env = lookup(HashMap::from([("DB", db.display().to_string()), ("BIND", "127.0.0.1:0".to_string())]));
        let cfg = Config::from_lookup(&env).unwrap();
        let out = render(&report(&cfg, &env));
        assert!(out.contains("not created yet"), "{out}");
        assert!(out.contains("no (created as"), "{out}");
        assert!(!db.exists() && !crate::secrets::key_file_path(&db).exists(), "the doctor must not create files");
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn env_file_lines() {
        let parsed = parse_env_file(
            "# comment\n\nSKRONGLE_BIND=0.0.0.0:9000\nexport SKRONGLE_LOG=debug\nSKRONGLE_AI_PROVIDER=\"anthropic\"\n; other\nnot a line\n",
        );
        assert_eq!(
            parsed,
            vec![
                ("SKRONGLE_BIND".into(), "0.0.0.0:9000".into()),
                ("SKRONGLE_LOG".into(), "debug".into()),
                ("SKRONGLE_AI_PROVIDER".into(), "anthropic".into()),
            ]
        );
    }

    #[test]
    fn meminfo_parsing() {
        let text = "MemTotal:         427364 kB\nMemFree:           12000 kB\nMemAvailable:     312000 kB\n";
        assert_eq!(meminfo_line(text), "304.7 MB available of 417.3 MB");
        assert_eq!(meminfo_line(""), "unknown");
    }

    #[test]
    fn bind_accepts_a_bare_port_and_names_bad_values() {
        assert_eq!(crate::parse_bind(Some("9000")).unwrap().to_string(), "0.0.0.0:9000");
        assert_eq!(crate::parse_bind(None).unwrap().to_string(), crate::DEFAULT_BIND);
        assert!(crate::parse_bind(Some("localhost:80x")).unwrap_err().contains("SKRONGLE_BIND"));
    }
}
