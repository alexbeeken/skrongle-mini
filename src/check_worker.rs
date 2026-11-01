//! `skrongle content-check` (#1041): set up the process that runs the
//! content check (`content_check::worker`) beside the server. It reads the
//! server's settings: `SKRONGLE_DB` (the same file), `TYPESAFE_API_KEY` and
//! `SKRONGLE_AI_*` (the keys live here when the server runs with
//! `SKRONGLE_CONTENT_CHECK=external`). In dev mode (`SKRONGLE_DEV=1`,
//! `SKRONGLE_BIND` = the dev server's address) it calls the dev server's
//! stand-ins for Jev and the model. Not in Skrongle mini.

use std::path::PathBuf;
use std::sync::Arc;

use crate::db::Db;

pub const HELP: &str = "\
skrongle content-check: run the content check beside the server, on the same database (SKRONGLE_DB).

Start the server with SKRONGLE_CONTENT_CHECK=external so it only queues the checks. This process
needs TYPESAFE_API_KEY (stage 1) and SKRONGLE_AI_KEY (stage 2, the AI review). It runs until stopped;
a check it was in the middle of is taken again when its lease runs out (10 minutes).";

/// The command's exit code.
pub fn main(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return 0;
    }
    if let Some(a) = args.first() {
        eprintln!("unknown argument {a:?}\n\n{HELP}");
        return 2;
    }
    let level = crate::env("LOG").and_then(|v| v.parse().ok()).unwrap_or(tracing::Level::INFO);
    tracing_subscriber::fmt().with_max_level(level).with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout())).with_target(false).compact().init();
    match run() {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("content check: {e}");
            1
        }
    }
}

fn run() -> Result<(), String> {
    // This command returns from `main` before the server records its shape,
    // so it reads `SKRONGLE_MINI` itself (#578).
    let shape = crate::shape::Shape::from_env()?;
    crate::shape::init(shape);
    refuse_mini(shape)?;
    let bind = crate::parse_bind(crate::env("BIND").as_deref())?;
    let secure = crate::env("SECURE_COOKIES").is_some_and(|v| v == "1" || v == "true");
    let asked = crate::env("DEV").is_some_and(|v| v == "1" || v == "true");
    let dev = crate::dev::resolve(asked, bind, secure, crate::env("PUBLIC_URL").as_deref()).unwrap_or_else(|why| {
        tracing::warn!("{why}");
        false
    });
    crate::dev::init_llm(dev, bind)?;
    tracing::info!("{}", crate::content_check::init_worker(dev, bind.port())?);
    let ai_key = crate::take_env("AI_KEY");
    // The key is only sealed in memory here; a fresh secret each run will do.
    let mut secret = [0u8; 32];
    crate::random::fill(&mut secret);
    let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    let (vault, _) = crate::secrets::Vault::load(Some(hex), std::path::Path::new("unused.db"))?;
    let meter = Arc::new(crate::metering::Meter::new(crate::metering::MeterConfig::from_env()?, ai_key, &vault, false));
    if !meter.hosted() {
        tracing::warn!("content check: no SKRONGLE_AI_KEY: stage 2 (the AI review) is off; parts that score high stay queued");
    }
    let path = crate::env("DB").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(crate::DEFAULT_DB));
    let db = Db::open(&path).map_err(|e| crate::startup::db_error(&path, &e))?;
    tracing::info!("content check: database {}", path.display());
    crate::content_check::worker::run(db, meter)
}

/// Skrongle mini has no content check.
fn refuse_mini(shape: crate::shape::Shape) -> Result<(), String> {
    if shape.is_mini() {
        return Err("Skrongle mini has no content check".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::shape::Shape;

    /// `skrongle content-check` on a mini refuses (#578: the shape is
    /// read here, since `main` returns before it records it).
    #[test]
    fn a_mini_has_no_content_check() {
        assert!(super::refuse_mini(Shape::Mini).unwrap_err().contains("Skrongle mini"));
        assert_eq!(super::refuse_mini(Shape::Hosted), Ok(()));
    }
}
