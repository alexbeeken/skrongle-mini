//! The `SKRONGLE_MINI` switch (#578, #632): `SKRONGLE_MINI=1` is Skrongle
//! mini (a handful of users on a small box, SQLite, no e-mail, SMS, Google
//! or payments). The switch is kept until its own issue retires it. The
//! shape is read once at startup and fixed for the process's life; the rest
//! of the code asks [`current`] (or `crate::is_mini()`), never the variable.
//! A test below keeps it that way.
//!
//! A mini has no e-mail, text messages, Google sign-in, payments or S3.
//! There is no second switch for any of them: the settings that would turn
//! them on ([`HOSTED_ONLY`]) read as unset (`crate::env`), and the startup
//! log names any that are set and ignored.

use std::sync::OnceLock;

/// The variable that makes a server a Skrongle mini. Read once, in `main`.
pub const VAR: &str = "SKRONGLE_MINI";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The self-hosted version: `SKRONGLE_MINI=1`.
    Mini,
    /// The public site, with every provider set.
    Hosted,
}

impl Shape {
    /// Parse `SKRONGLE_MINI`: unset, empty, `0` or `false` is the hosted
    /// site; `1` or `true` is a mini; anything else refuses to start.
    pub fn parse(value: Option<&str>) -> Result<Shape, String> {
        match value.map(str::trim).unwrap_or("") {
            "" | "0" | "false" => Ok(Shape::Hosted),
            "1" | "true" => Ok(Shape::Mini),
            other => Err(format!("{VAR}={other}: expected 1 (Skrongle mini) or unset")),
        }
    }

    /// `SKRONGLE_MINI` from the environment (`parse`); read directly, it
    /// has no older spelling.
    pub fn from_env() -> Result<Shape, String> {
        Shape::parse(std::env::var(VAR).ok().as_deref())
    }

    pub fn is_mini(self) -> bool {
        self == Shape::Mini
    }

    /// The startup log line.
    pub fn describe(self) -> String {
        match self {
            Shape::Mini => format!("shape: Skrongle mini ({VAR}=1): one small box, no providers needed"),
            Shape::Hosted => format!("shape: {VAR} unset: every provider required outside dev mode"),
        }
    }

    /// Whether `crate::env` reads `SKRONGLE_<name>` as unset: in a mini,
    /// the settings in [`HOSTED_ONLY`].
    pub fn hides(self, name: &str) -> bool {
        self.is_mini() && HOSTED_ONLY.contains(&name)
    }

    /// The startup warning for settings this shape ignores, given a lookup
    /// of `SKRONGLE_<name>` (before `hides`). `None` when there are none.
    pub fn ignored(self, get: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        let mut set: Vec<String> = HOSTED_ONLY.iter().filter(|n| self.hides(n) && get(n).is_some()).map(|n| format!("SKRONGLE_{n}")).collect();
        // Gone with #578: the shape alone decides whether there are plans.
        if get("PLANS").is_some() {
            set.push("SKRONGLE_PLANS".into());
        }
        if set.is_empty() {
            return None;
        }
        Some(match self {
            Shape::Mini => format!("Skrongle mini ignores {}: e-mail, text messages, Google sign-in, payments and S3 are not available; unset them", set.join(", ")),
            Shape::Hosted => format!("{} is gone; unset it", set.join(", ")),
        })
    }
}

/// The settings (`SKRONGLE_<name>`) that turn on a feature a mini does not have.
/// In a mini they read as unset, so the feature is off wherever it is read.
pub const HOSTED_ONLY: &[&str] = &["MAIL_PROVIDERS", "SMS_PROVIDERS", "GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET", "STRIPE_SECRET_KEY", "STRIPE_WEBHOOK_SECRET", "S3_BUCKET"];

static SHAPE: OnceLock<Shape> = OnceLock::new();

/// Record the shape, once, at startup. A second call is ignored.
pub fn init(shape: Shape) {
    let _ = SHAPE.set(shape);
}

/// The shape this server runs as. Before `init` (unit tests) it is the
/// same as an unset `SKRONGLE_MINI`.
pub fn current() -> Shape {
    SHAPE.get().copied().unwrap_or(Shape::Hosted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch() {
        assert_eq!(Shape::parse(None), Ok(Shape::Hosted));
        assert_eq!(Shape::parse(Some("")), Ok(Shape::Hosted));
        assert_eq!(Shape::parse(Some("0")), Ok(Shape::Hosted));
        assert_eq!(Shape::parse(Some(" 1 ")), Ok(Shape::Mini));
        assert_eq!(Shape::parse(Some("true")), Ok(Shape::Mini));
        let err = Shape::parse(Some("yes")).unwrap_err();
        assert!(err.contains("SKRONGLE_MINI=yes"), "{err}");
        assert!(Shape::Mini.is_mini() && !Shape::Hosted.is_mini());
        assert!(Shape::Mini.describe().contains("Skrongle mini"));
    }

    /// A mini has none of the provider settings; without the switch the server reads
    /// every setting.
    #[test]
    fn a_mini_has_no_production_features() {
        for name in HOSTED_ONLY {
            assert!(Shape::Mini.hides(name) && !Shape::Hosted.hides(name), "{name}");
        }
        assert!(!Shape::Mini.hides("AI_KEY") && !Shape::Mini.hides("SIGNUP"));
        let none = |_: &str| None;
        assert_eq!(Shape::Mini.ignored(&none), None);
        assert_eq!(Shape::Hosted.ignored(&none), None);
        let mail = |n: &str| (n == "MAIL_PROVIDERS" || n == "PLANS").then(|| "x".to_string());
        let w = Shape::Mini.ignored(&mail).unwrap();
        assert!(w.contains("SKRONGLE_MAIL_PROVIDERS") && w.contains("SKRONGLE_PLANS"), "{w}");
        let w = Shape::Hosted.ignored(&mail).unwrap();
        assert!(w.contains("SKRONGLE_PLANS is gone") && !w.contains("MAIL"), "{w}");
    }

    /// Only this module reads `SKRONGLE_MINI`; everything else asks
    /// `shape::current()` or `crate::is_mini()`, so the shape is decided
    /// once (#578).
    #[test]
    fn nothing_else_reads_the_variable() {
        let root = env!("CARGO_MANIFEST_DIR");
        let Ok(out) = std::process::Command::new("git").args(["-C", root, "ls-files", "-z", "src"]).output() else {
            return; // no git: nothing to check
        };
        let readers: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|f| !f.is_empty() && *f != "src/shape.rs")
            .filter(|f| {
                std::fs::read_to_string(format!("{root}/{f}")).is_ok_and(|s| {
                    s.lines().any(|l| l.contains("env::var(") && (l.contains("SKRONGLE_MINI") || l.contains("shape::VAR")))
                })
            })
            .map(str::to_string)
            .collect();
        assert!(readers.is_empty(), "SKRONGLE_MINI is read once, in shape.rs; ask shape::current() instead: {readers:?}");
    }
}
