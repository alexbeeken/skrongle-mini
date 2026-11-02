//! The app's own text in the learner's language.
//!
//! Every piece of static text the app shows (navigation, forms, buttons,
//! messages, what the scripts draw) lives in `i18n/<code>.yaml`, one file per
//! language, every entry under a key. Adding a language is adding one file;
//! `cargo test` checks it has every key `en.yaml` has.
//!
//! Which language a request is in is decided once per request: the signed-in
//! user's `users.lang` (set by the `CurrentUser` extractor), else the `lang`
//! cookie (the picker on the login and register pages), else the browser's
//! `Accept-Language`, else English. It is kept in a task-local, so templates
//! and handlers simply call `crate::t("key")`.
//!
//! The files are a small subset of YAML (`i18n/README.md`): nested maps by
//! two-space indentation, scalars plain, `"double-quoted"` (JSON escapes),
//! `'single-quoted'` or `|` / `>` blocks, `#` comments. Keys are joined with
//! dots: `nav.login`. A value may hold `{name}` placeholders (`t1`, `t2`,
//! `tn`); a key ending in `_html` may hold markup and is rendered unescaped
//! by the template, so only trusted values go into its placeholders.
//!
//! Courses are not translated here; a built-in theme's text is (`themes.<id>`, #463).

use axum::extract::Request;
use axum::http::header::{ACCEPT_LANGUAGE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use rust_embed::RustEmbed;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::OnceLock;

/// `i18n/*.yaml`, compiled into the binary (read from disk in debug builds).
#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Files;

/// The language the app falls back to, and the one every other file is
/// checked against.
pub const DEFAULT: &str = "en";

/// The cookie that remembers an anonymous visitor's choice (the picker on
/// the login and register pages), and what a new account starts with.
pub const COOKIE_NAME: &str = "lang";
const COOKIE_TTL_SECS: i64 = 365 * 24 * 60 * 60;

pub struct Language {
    /// The file's name: `en`, `es`, … (also the `<html lang>` value).
    pub code: String,
    /// Its own name for itself (`meta.name`): "English", "Español".
    pub name: String,
    /// The file's texts, parsed the first time something asks (`strings`).
    strings: OnceLock<BTreeMap<String, String>>,
}

#[cfg(test)]
thread_local! {
    /// Full-file parses done on this thread (the lazy-loading test counts them).
    static PARSES: Cell<usize> = const { Cell::new(0) };
}

impl Language {
    fn new(code: String, name: String) -> Self {
        Language { code, name, strings: OnceLock::new() }
    }

    /// This language's texts, parsed from its embedded file on first use.
    fn strings(&self) -> &BTreeMap<String, String> {
        self.strings.get_or_init(|| {
            #[cfg(test)]
            PARSES.with(|c| c.set(c.get() + 1));
            let file = format!("{}.yaml", self.code);
            let text = file_text(&file);
            parse(&text).unwrap_or_else(|e| panic!("i18n/{file}: {e}")) // panic-ok: embedded file, cargo test i18n parses every one
        })
    }
}

fn file_text(file: &str) -> String {
    let data = Files::get(file).unwrap_or_else(|| panic!("i18n/{file}: missing")); // panic-ok: embedded file, cargo test i18n parses every one
    std::str::from_utf8(&data.data).unwrap_or_else(|e| panic!("i18n/{file}: not UTF-8: {e}")).to_string() // panic-ok: embedded file, cargo test i18n parses every one
}

/// `meta.name` from the top of a file without parsing the rest: the
/// `meta:` block's `name:` line. `None` when the file does not start that
/// way (the caller then parses the whole file).
fn scan_name(text: &str) -> Option<String> {
    let mut in_meta = false;
    for line in text.lines().map(|l| l.trim_end_matches('\r')) {
        let content = line.trim_start_matches(' ');
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        if content.len() == line.len() {
            if in_meta {
                return None;
            }
            in_meta = content.trim_end() == "meta:";
            if !in_meta {
                return None;
            }
        } else if in_meta {
            if let Some(v) = content.strip_prefix("name:").filter(|v| v.starts_with(' ')) {
                return scalar(v.trim()).ok().filter(|n| !n.is_empty());
            }
        }
    }
    None
}

static LANGUAGES: OnceLock<Vec<Language>> = OnceLock::new();

/// Every language shipped, the default first, the rest by code. Reads only
/// each file's `meta.name`; a language's texts load when first asked for.
pub fn languages() -> &'static [Language] {
    LANGUAGES.get_or_init(|| {
        let mut all: Vec<Language> = Files::iter()
            .filter_map(|name| {
                let code = name.strip_suffix(".yaml")?.to_string();
                if code.contains('/') {
                    return None;
                }
                let lang = match scan_name(&file_text(&name)) {
                    Some(n) => Language::new(code, n),
                    None => {
                        let l = Language::new(code, String::new());
                        let n = l.strings().get("meta.name").cloned().unwrap_or_else(|| panic!("i18n/{name}: no meta.name")); // panic-ok: embedded file, cargo test i18n parses every one
                        Language { name: n, ..l }
                    }
                };
                Some(lang)
            })
            .collect();
        all.sort_by(|a, b| (a.code != DEFAULT).cmp(&(b.code != DEFAULT)).then_with(|| a.code.cmp(&b.code)));
        assert!(all.first().is_some_and(|l| l.code == DEFAULT), "i18n/{DEFAULT}.yaml is missing");
        all
    })
}

/// The language with this code, if shipped.
pub fn language(code: &str) -> Option<&'static Language> {
    languages().iter().find(|l| l.code == code)
}

pub fn is_language(code: &str) -> bool {
    language(code).is_some()
}

tokio::task_local! {
    /// Index into `languages()` for the request being handled.
    static CURRENT: Cell<usize>;
}

/// The language of the request being handled (the default outside one).
pub fn current() -> &'static Language {
    let i = CURRENT.try_with(|c| c.get()).unwrap_or(0);
    &languages()[i]
}

/// The current language's code, for `<html lang>` and the pickers.
pub fn lang() -> &'static str {
    &current().code
}

/// Switch the request being handled to `code` (an unknown code is ignored).
/// The `CurrentUser` extractor calls this with the account's choice.
pub fn set_current(code: &str) {
    if let Some(i) = languages().iter().position(|l| l.code == code) {
        let _ = CURRENT.try_with(|c| c.set(i));
    }
}

/// The request's language as a token `in_scope` restores on another
/// thread (`Db::call` carries it into its blocking closure, so a message
/// built there is in the learner's language too).
pub fn snapshot() -> usize {
    CURRENT.try_with(|c| c.get()).unwrap_or(0)
}

/// Run `f` with the language `snapshot` captured.
pub fn in_scope<T>(snapshot: usize, f: impl FnOnce() -> T) -> T {
    CURRENT.sync_scope(Cell::new(snapshot), f)
}

/// Run `f` in the language `code` (an account's `users.lang`, for a message
/// to that account); in the current one when `code` names no language
/// (`''`: the account follows the browser).
pub fn in_lang<T>(code: &str, f: impl FnOnce() -> T) -> T {
    let i = languages().iter().position(|l| l.code == code).unwrap_or_else(snapshot);
    in_scope(i, f)
}

/// Middleware: decide the request's language from its cookie or
/// `Accept-Language` and run the rest of the request in it.
pub async fn middleware(req: Request, next: Next) -> Response {
    let code = from_headers(req.headers());
    let i = languages().iter().position(|l| l.code == code).unwrap_or(0);
    CURRENT.scope(Cell::new(i), next.run(req)).await
}

/// The language a request's headers ask for: the `lang` cookie, else the
/// best `Accept-Language` match, else the default.
pub fn from_headers(headers: &HeaderMap) -> String {
    if let Some(c) = cookie(headers).filter(|c| is_language(c)) {
        return c;
    }
    headers
        .get(ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .and_then(negotiate)
        .unwrap_or(DEFAULT)
        .to_string()
}

/// The language the `lang` cookie names, when it is one shipped: what the
/// visitor picked themselves (a new account starts with it).
pub fn chosen(headers: &HeaderMap) -> Option<String> {
    cookie(headers).filter(|c| is_language(c))
}

fn cookie(headers: &HeaderMap) -> Option<String> {
    headers.get_all(axum::http::header::COOKIE).iter().find_map(|v| {
        v.to_str().ok()?.split(';').find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k.trim() == COOKIE_NAME).then(|| v.trim().to_string())
        })
    })
}

/// The first language of an `Accept-Language` value (by its `q`) that is
/// shipped, matching on the primary subtag (`es-MX` finds `es`).
pub fn negotiate(accept: &str) -> Option<&'static str> {
    let mut wanted: Vec<(f32, &str)> = accept
        .split(',')
        .filter_map(|part| {
            let mut it = part.trim().split(';');
            let tag = it.next()?.trim();
            if tag.is_empty() {
                return None;
            }
            let q = it
                .find_map(|p| p.trim().strip_prefix("q="))
                .and_then(|q| q.trim().parse::<f32>().ok())
                .unwrap_or(1.0);
            Some((q, tag))
        })
        .collect();
    wanted.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    wanted.into_iter().filter(|(q, _)| *q > 0.0).find_map(|(_, tag)| {
        let primary = tag.split('-').next()?.to_ascii_lowercase();
        languages().iter().find(|l| l.code == primary).map(|l| l.code.as_str())
    })
}

/// `Set-Cookie` remembering `code` for a year.
pub fn set_cookie(code: &str, secure: bool) -> (HeaderName, HeaderValue) {
    let v = format!(
        "{COOKIE_NAME}={code}; Path={}; SameSite=Lax; Max-Age={COOKIE_TTL_SECS}{}",
        crate::cookie_path(),
        if secure { "; Secure" } else { "" }
    );
    (SET_COOKIE, HeaderValue::from_str(&v).expect("valid cookie header"))
}

// ---------- lookups ----------

impl Language {
    /// The text under `key`, falling back to the default language, then to
    /// the key itself (a missing key is a bug `cargo test` catches).
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(s) = self.strings().get(key) {
            return s;
        }
        if let Some(s) = languages()[0].strings().get(key) {
            return s;
        }
        tracing::warn!("i18n: no text for {key}");
        key
    }

    #[cfg(test)]
    pub fn has(&self, key: &str) -> bool {
        self.strings().contains_key(key)
    }

    /// Every key, in order.
    #[cfg(test)]
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.strings().keys().map(String::as_str)
    }

    /// The keys under `prefix.` with their texts, as a JSON object (what
    /// `js_json` embeds for the scripts).
    fn json_under(&self, prefix: &str) -> String {
        let full = format!("{prefix}.");
        let mut map = serde_json::Map::new();
        // The default's keys first so a missing translation falls back.
        for lang in [&languages()[0], self] {
            for (k, v) in lang.strings().range(full.clone()..) {
                if !k.starts_with(&full) {
                    break;
                }
                map.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
        }
        serde_json::Value::Object(map).to_string()
    }
}

/// The text under `key` in the request's language.
pub fn t(key: &str) -> String {
    current().get(key).to_string()
}

/// The text under `key` in the request's language (else English), or
/// `None` when en.yaml has no such key: for a key built at run time, such
/// as a theme's (`themes.<id>.title`), where a theme a learner made has none.
pub fn lookup(key: &str) -> Option<&'static str> {
    let cur = current();
    cur.strings().get(key).or_else(|| languages().first()?.strings().get(key)).map(String::as_str)
}

/// `t`, with `{name}` replaced by `value`.
pub fn t1(key: &str, name: &str, value: impl Display) -> String {
    fill(current().get(key), &[(name, &value)])
}

/// `t`, with two placeholders filled.
pub fn t2(key: &str, n1: &str, v1: impl Display, n2: &str, v2: impl Display) -> String {
    fill(current().get(key), &[(n1, &v1), (n2, &v2)])
}

/// A count: `key.one` for exactly one, else `key.other`, with `{n}` filled.
pub fn tn(key: &str, n: impl Count) -> String {
    let n = n.count();
    let form = if n == 1 { "one" } else { "other" };
    fill(current().get(&format!("{key}.{form}")), &[("n", &n)])
}

/// A number `tn` can count with (a template passes its field as it is).
pub trait Count: Copy {
    fn count(self) -> i64;
}
macro_rules! count {
    ($($t:ty),*) => { $(impl Count for $t { fn count(self) -> i64 { self as i64 } })* };
}
count!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
impl<T: Count> Count for &T {
    fn count(self) -> i64 {
        (*self).count()
    }
}

/// Replace every `{name}` in `text` from `values`; an unknown placeholder
/// stays as it is.
pub fn fill(text: &str, values: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if after[..end].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => {
                let name = &after[..end];
                match values.iter().find(|(n, _)| *n == name) {
                    Some((_, v)) => out.push_str(&v.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The texts the scripts need, as a JSON object of the keys under
/// `<prefix>.` (the key in full → the text), for `Object.assign(I18N, …)`
/// in a template's script block: `js_json("sprint.js")`.
pub fn js_json(prefix: &str) -> String {
    let json = current().json_under(prefix);
    // Inside a <script>, "</" must not appear in the JSON text.
    json.replace("</", "<\\/")
}

// ---------- the file format ----------

/// Parse one language file into `key.path → text`.
pub fn parse(text: &str) -> Result<BTreeMap<String, String>, String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out = BTreeMap::new();
    // The open maps: (indent, key path).
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let no = i + 1;
        i += 1;
        let content = line.trim_start_matches(' ');
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        if content.starts_with('\t') {
            return Err(format!("line {no}: indent with spaces, not tabs"));
        }
        let indent = line.len() - content.len();
        while stack.last().is_some_and(|(ind, _)| *ind >= indent) {
            stack.pop();
        }
        let (key, value) = content
            .split_once(':')
            .ok_or_else(|| format!("line {no}: expected `key: value` or `key:`"))?;
        let key = key.trim();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return Err(format!("line {no}: a key is letters, digits, `_` and `-` (got `{key}`)"));
        }
        if !value.is_empty() && !value.starts_with(' ') {
            return Err(format!("line {no}: a space after the colon, please"));
        }
        let path = match stack.last() {
            Some((_, parent)) => format!("{parent}.{key}"),
            None => key.to_string(),
        };
        let value = value.trim();
        let value = if value.starts_with('#') { "" } else { value };
        if value.is_empty() {
            // Opens a nested map; a key with nothing under it is an error on
            // the next line that is not deeper.
            stack.push((indent, path.clone()));
            let deeper = lines.get(i..).and_then(|rest| {
                rest.iter().find(|l| {
                    let c = l.trim_start_matches(' ');
                    !c.is_empty() && !c.starts_with('#')
                })
            });
            match deeper {
                Some(l) if l.len() - l.trim_start_matches(' ').len() > indent => {}
                _ => return Err(format!("line {no}: `{key}:` has nothing under it (an empty text is `{key}: \"\"`)")),
            }
            continue;
        }
        let text = if let Some(block) = value.strip_prefix('|').or_else(|| value.strip_prefix('>')) {
            let folded = value.starts_with('>');
            let chomp = block.trim_start_matches('-').trim();
            if !chomp.is_empty() && !chomp.starts_with('#') {
                return Err(format!("line {no}: a block scalar is `|`, `|-`, `>` or `>-` alone"));
            }
            let keep = !block.starts_with('-');
            let mut body: Vec<&str> = Vec::new();
            let mut body_indent = None;
            while i < lines.len() {
                let l = lines[i];
                let c = l.trim_start_matches(' ');
                let ind = l.len() - c.len();
                if c.is_empty() {
                    body.push("");
                    i += 1;
                    continue;
                }
                if ind <= indent {
                    break;
                }
                let bi = *body_indent.get_or_insert(ind);
                if ind < bi {
                    return Err(format!("line {}: a block's lines keep the same indent", i + 1));
                }
                body.push(&l[bi..]);
                i += 1;
            }
            while body.last().is_some_and(|l| l.is_empty()) {
                body.pop();
            }
            let joined = if folded { body.join(" ").split_whitespace().collect::<Vec<_>>().join(" ") } else { body.join("\n") };
            if keep { format!("{joined}\n") } else { joined }
        } else {
            scalar(value).map_err(|e| format!("line {no}: {e}"))?
        };
        if out.insert(path.clone(), text).is_some() {
            return Err(format!("line {no}: `{path}` is defined twice"));
        }
    }
    Ok(out)
}

/// A scalar on one line: plain, `"…"` (JSON escapes) or `'…'` (`''` is `'`).
fn scalar(value: &str) -> Result<String, String> {
    if let Some(rest) = value.strip_prefix('"') {
        let mut end = None;
        let mut escaped = false;
        for (i, ch) in rest.char_indices() {
            match ch {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => {
                    end = Some(i);
                    break;
                }
                _ => escaped = false,
            }
        }
        let end = end.ok_or("unterminated \"…\"")?;
        let tail = rest[end + 1..].trim();
        if !tail.is_empty() && !tail.starts_with('#') {
            return Err(format!("unexpected `{tail}` after the closing quote"));
        }
        return serde_json::from_str::<String>(&value[..end + 2]).map_err(|e| format!("bad escape in \"…\": {e}"));
    }
    if let Some(rest) = value.strip_prefix('\'') {
        let mut out = String::new();
        let mut chars = rest.chars().peekable();
        loop {
            match chars.next() {
                None => return Err("unterminated '…'".into()),
                Some('\'') if chars.peek() == Some(&'\'') => {
                    chars.next();
                    out.push('\'');
                }
                Some('\'') => break,
                Some(c) => out.push(c),
            }
        }
        let tail: String = chars.collect();
        let tail = tail.trim();
        if !tail.is_empty() && !tail.starts_with('#') {
            return Err(format!("unexpected `{tail}` after the closing quote"));
        }
        return Ok(out);
    }
    if value.starts_with(['[', '{', '&', '*', '!', '%', '@', '`', '-', '?']) || value.starts_with(": ") || value.ends_with(':') {
        return Err(format!("quote a text that starts with `{}`", &value[..value.chars().next().map_or(0, char::len_utf8)]));
    }
    // A plain scalar ends at ` #` (a comment).
    let text = match value.find(" #") {
        Some(i) => value[..i].trim_end(),
        None => value,
    };
    if text.contains(": ") {
        return Err("quote a text that contains `: `".into());
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_name_agrees_with_the_full_parse() {
        for f in Files::iter().filter(|f| f.ends_with(".yaml")) {
            let text = file_text(&f);
            let full = parse(&text).unwrap()["meta.name"].clone();
            assert_eq!(scan_name(&text), Some(full), "{f}");
        }
        assert_eq!(scan_name("meta:\n  name: \"A: b\"\nx: 1\n"), Some("A: b".into()));
        assert_eq!(scan_name("x: 1\nmeta:\n  name: A\n"), None);
    }

    #[test]
    fn a_language_parses_once_and_the_list_parses_none() {
        let before = PARSES.with(Cell::get);
        let all = languages();
        assert_eq!(PARSES.with(Cell::get), before, "languages() must not parse a file");
        assert!(all.len() >= 6);
        let en = Language::new("en".into(), "English".into());
        assert_eq!(en.get("meta.name"), "English");
        assert_eq!(en.get("meta.name"), "English");
        assert_eq!(PARSES.with(Cell::get), before + 1, "English twice is one parse");
    }

    #[test]
    fn parses_the_subset() {
        let text = "# comment\nmeta:\n  name: English\nnav:\n  login: Log in # trailing comment\n  quoted: \"It's \\\"here\\\"\\n\"\n  single: 'It''s'\n  block: |\n    one\n    two\n\n  folded: >-\n    a\n    b\n  count:\n    one: \"{n} lesson\"\n    other: \"{n} lessons\"\n";
        let m = parse(text).unwrap();
        assert_eq!(m["meta.name"], "English");
        assert_eq!(m["nav.login"], "Log in");
        assert_eq!(m["nav.quoted"], "It's \"here\"\n");
        assert_eq!(m["nav.single"], "It's");
        assert_eq!(m["nav.block"], "one\ntwo\n");
        assert_eq!(m["nav.folded"], "a b");
        assert_eq!(m["nav.count.other"], "{n} lessons");
    }

    #[test]
    fn rejects_what_it_does_not_understand() {
        assert!(parse("a:\n").unwrap_err().contains("nothing under it"));
        assert!(parse("a: [x]\n").unwrap_err().contains("quote"));
        assert!(parse("a: x\na: y\n").unwrap_err().contains("twice"));
        assert!(parse("a:b\n").unwrap_err().contains("space after the colon"));
        assert!(parse("\ta: b\n").unwrap_err().contains("tabs"));
        assert!(parse("a: \"open\n").unwrap_err().contains("unterminated"));
    }

    #[test]
    fn fills_placeholders() {
        assert_eq!(fill("{n} of {total} done", &[("n", &3), ("total", &9)]), "3 of 9 done");
        assert_eq!(fill("keep {this} and {x", &[("x", &1)]), "keep {this} and {x");
        assert_eq!(fill("{ not a placeholder }", &[]), "{ not a placeholder }");
    }

    #[test]
    fn negotiates_accept_language() {
        assert_eq!(negotiate("es-MX,es;q=0.9,en;q=0.8"), Some("es"));
        assert_eq!(negotiate("xx;q=0.9, en;q=0.5, es;q=0.7"), Some("es"));
        assert_eq!(negotiate("xx, yy"), None);
        assert_eq!(negotiate("*"), None);
        assert_eq!(negotiate("en;q=0"), None);
    }

    #[test]
    fn cookie_wins_over_the_header() {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en"));
        h.insert(axum::http::header::COOKIE, HeaderValue::from_static("sid=abc; lang=es"));
        assert_eq!(from_headers(&h), "es");
        h.insert(axum::http::header::COOKIE, HeaderValue::from_static("lang=xx"));
        assert_eq!(from_headers(&h), "en");
    }

    #[test]
    fn every_language_has_every_key_of_the_default() {
        let en = &languages()[0];
        assert_eq!(en.code, DEFAULT);
        for lang in &languages()[1..] {
            let missing: Vec<&str> = en.keys().filter(|k| !lang.has(k)).collect();
            assert!(missing.is_empty(), "i18n/{}.yaml lacks {} keys of en.yaml: {}", lang.code, missing.len(), missing.join(", "));
            let extra: Vec<&str> = lang.keys().filter(|k| !en.has(k)).collect();
            assert!(extra.is_empty(), "i18n/{}.yaml has keys en.yaml does not: {}", lang.code, extra.join(", "));
            for k in en.keys() {
                let a = placeholders(en.get(k));
                let b = placeholders(lang.get(k));
                assert_eq!(a, b, "i18n/{}.yaml: `{k}` has other placeholders than en.yaml", lang.code);
            }
        }
    }

    fn placeholders(s: &str) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            match after.find('}') {
                Some(end) if after[..end].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => {
                    out.insert(after[..end].to_string());
                    rest = &after[end + 1..];
                }
                _ => rest = after,
            }
        }
        out
    }

    /// Every `crate::t("…")` / `t1` / `t2` in a template or source file,
    /// and every `t('…')` / `tn('…')` in a script, names a key en.yaml has.
    #[test]
    fn every_key_used_exists() {
        let en = &languages()[0];
        let mut missing = Vec::new();
        let mut used = std::collections::BTreeSet::new();
        for dir in ["templates", "src", "src/routes", "static"] {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !path.is_file() || !["html", "rs", "js"].contains(&ext) {
                    continue;
                }
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                let text = std::fs::read_to_string(&path).unwrap();
                for key in keys_in(&text) {
                    let plural = key.1;
                    let ok = if plural { en.has(&format!("{}.one", key.0)) && en.has(&format!("{}.other", key.0)) } else { en.has(&key.0) };
                    if !ok {
                        missing.push(format!("{name}: {}", key.0));
                    }
                    used.insert(key.0);
                }
            }
        }
        assert!(missing.is_empty(), "keys used but not in i18n/en.yaml:\n{}", missing.join("\n"));
        assert!(used.len() > 50, "the key scan found only {} keys; is it broken?", used.len());
    }

    /// `(key, is a plural)` for every lookup call in `text`.
    fn keys_in(text: &str) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        for (call, plural) in [("t(\"", false), ("t1(\"", false), ("t2(\"", false), ("tn(\"", true), ("t('", false), ("tn('", true)] {
            let mut rest = text;
            while let Some(i) = rest.find(call) {
                let before = &rest[..i];
                let boundary = before.chars().last().map_or(true, |c| !c.is_alphanumeric() && c != '_');
                rest = &rest[i + call.len()..];
                if !boundary {
                    continue;
                }
                let quote = call.chars().last().unwrap();
                let Some(end) = rest.find(quote) else { break };
                let key = &rest[..end];
                if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-') && key.contains('.') {
                    out.push((key.to_string(), plural));
                }
            }
        }
        out
    }
}

