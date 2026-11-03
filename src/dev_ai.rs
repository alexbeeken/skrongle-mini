//! The dev AI provider (#297): dev mode's stand-in for Anthropic and OpenAI
//! (Gemini goes through the OpenAI format, as in production). It speaks the
//! real wire protocols at `/dev/ai/anthropic/v1/messages` and
//! `/dev/ai/openai/v1/chat/completions` (`routes::dev_ai`), so the app's own
//! clients in `ai` call it exactly as they call the providers: the app only
//! sees a base URL (dev mode points the defaults here, unless
//! `SKRONGLE_LLM_MODE=live`; any custom base URL may too) and a key.
//!
//! It answers from the LLM fixtures (`data/test-fixtures/llm/<purpose>.json`,
//! picked by the call's purpose and a `[fixture:NAME]` token in the prompt),
//! or holds a request until a reply is typed on `/dev/ai` ("Manual replies",
//! `SKRONGLE_LLM_MODE=capture`), and plays what a provider does to an account: usage counted per key
//! profile against a limit (requests, tokens or dollars), the provider's own
//! "quota used up" error once it is reached, and failures injected on
//! request (a hang, a 500, overloaded, a refused key, a broken body, a reply
//! that does not fit the request). Everything is in memory for this server
//! run; the dev mailbox's AI section (`/dev/ai`) shows the log and the controls.
//!
//! One key profile, **the learner's own** keys, told by the key in the
//! request. Any other key is refused as a provider refuses an unknown one.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::ai::ProviderKind;
use crate::secrets::Provider;

/// The learner's own keys the seed and `/dev/login?keys=dev` store: one
/// for every provider the app supports.
pub const LEARNER_KEYS: [(Provider, &str); 3] =
    [(Provider::Anthropic, "sk-ant-dev-anthropic"), (Provider::OpenAi, "sk-dev-openai"), (Provider::Google, "AIza-dev-google")];
/// How many calls the log keeps (newest first; older ones drop off).
pub const LOG_MAX: usize = 100;
/// A request or reply body is kept up to this many bytes in the log.
const BODY_MAX: usize = 96 * 1024;
/// A fixture with `"timeout": true` hangs this long: past any client's limit.
pub const HANG_FOREVER_SECS: u64 = 900;
/// The hang an injected timeout starts with.
pub const DEFAULT_HANG_SECS: u64 = 30;

/// Whose key a request carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Learner,
}

impl Who {
    pub const ALL: [Who; 1] = [Who::Learner];

    pub fn id(self) -> &'static str {
        match self {
            Who::Learner => "learner",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Who::Learner => "Learner's own keys",
        }
    }

    pub fn parse(s: &str) -> Option<Who> {
        Who::ALL.into_iter().find(|w| w.id() == s)
    }

    /// The keys this profile answers to (the learner's also take any
    /// `sk-ant-dev-…`, `sk-dev-…` or `AIza-dev-…` key).
    pub fn keys(self) -> Vec<&'static str> {
        match self {
            Who::Learner => LEARNER_KEYS.iter().map(|(_, k)| *k).collect(),
        }
    }
}

/// The provider a dev key is for, from the longest prefix it starts with
/// (`sk-ant-`: Anthropic, `sk-or-`: OpenRouter, `xai-`: xAI, `AIza`:
/// Google, any other `sk-`: OpenAI).
pub fn key_provider(key: &str) -> Provider {
    Provider::ALL
        .into_iter()
        .filter(|p| !p.prefix().is_empty() && key.starts_with(p.prefix()))
        .max_by_key(|p| (p.prefix().len(), std::cmp::Reverse(p.id())))
        .unwrap_or(Provider::OpenAi)
}

/// What a dev key starts with: the simulator answers any key that does
/// (#24 added xAI's and OpenRouter's; DeepSeek and Mistral take `sk-dev-…`).
pub const DEV_KEY_PREFIXES: [&str; 5] = ["sk-ant-dev-", "sk-dev-", "AIza-dev-", "xai-dev-", "sk-or-dev-"];

/// The profile a key belongs to; `None`: a key the simulator refuses (401).
pub fn who(key: &str) -> Option<Who> {
    let key = key.trim();
    let dev_key = DEV_KEY_PREFIXES.iter().any(|p| key.len() > p.len() && key.starts_with(p));
    dev_key.then_some(Who::Learner)
}

/// Which wire protocol a request came in on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Anthropic,
    OpenAi,
    /// Gemini's OpenAI-compatible endpoint: OpenAI's format.
    Google,
}

impl Format {
    pub fn id(self) -> &'static str {
        match self {
            Format::Anthropic => "anthropic",
            Format::OpenAi => "openai",
            Format::Google => "google",
        }
    }

    fn openai_like(self) -> bool {
        self != Format::Anthropic
    }
}

/// How a profile's usage is measured against its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Measure {
    #[default]
    Requests,
    /// Input plus output tokens (estimated: 4 characters a token).
    Tokens,
    /// US dollars, from `PRICES`.
    Dollars,
}

impl Measure {
    pub const ALL: [Measure; 3] = [Measure::Requests, Measure::Tokens, Measure::Dollars];

    pub fn id(self) -> &'static str {
        match self {
            Measure::Requests => "requests",
            Measure::Tokens => "tokens",
            Measure::Dollars => "dollars",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Measure::Requests => "Requests",
            Measure::Tokens => "Tokens (in + out)",
            Measure::Dollars => "Dollars (price table)",
        }
    }

    pub fn parse(s: &str) -> Option<Measure> {
        Measure::ALL.into_iter().find(|m| m.id() == s)
    }

    /// `n` in this measure's unit.
    pub fn show(self, n: f64) -> String {
        match self {
            Measure::Requests if n as u64 == 1 => "1 request".to_string(),
            Measure::Requests => format!("{} requests", n as u64),
            Measure::Tokens => format!("{} tokens", crate::ai::grouped(n as usize)),
            Measure::Dollars => format!("${n:.4}"),
        }
    }
}

/// A failure the simulator can be told to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Hang `hang_secs`, then answer: past the client's limit it gives up first.
    Timeout,
    /// HTTP 500 `api_error` / `server_error`.
    ServerError,
    /// Anthropic's HTTP 529 `overloaded_error`; OpenAI's HTTP 503.
    Overloaded,
    /// HTTP 401: the key is refused.
    InvalidKey,
    /// HTTP 200 with a body that is not JSON.
    Malformed,
    /// A well-formed reply whose text is not what the call asked for.
    WrongShape,
}

impl Fault {
    pub const ALL: [Fault; 6] = [Fault::Timeout, Fault::ServerError, Fault::Overloaded, Fault::InvalidKey, Fault::Malformed, Fault::WrongShape];

    pub fn id(self) -> &'static str {
        match self {
            Fault::Timeout => "timeout",
            Fault::ServerError => "server-error",
            Fault::Overloaded => "overloaded",
            Fault::InvalidKey => "invalid-key",
            Fault::Malformed => "malformed",
            Fault::WrongShape => "wrong-shape",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Fault::Timeout => "Timeout (hang)",
            Fault::ServerError => "500 server error",
            Fault::Overloaded => "Overloaded (529 / 503)",
            Fault::InvalidKey => "401 invalid key",
            Fault::Malformed => "Malformed JSON body",
            Fault::WrongShape => "Reply not valid for the purpose",
        }
    }

    pub fn parse(s: &str) -> Option<Fault> {
        Fault::ALL.into_iter().find(|f| f.id() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Injection {
    pub fault: Fault,
    /// Every request until cleared; else only the next one.
    pub sticky: bool,
    /// `Fault::Timeout`: how long to hang.
    pub hang_secs: u64,
}

impl Injection {
    pub fn describe(&self) -> String {
        let what = match self.fault {
            Fault::Timeout => format!("hang {} s", self.hang_secs),
            f => f.label().to_string(),
        };
        format!("{what}, {}", if self.sticky { "every request" } else { "next request only" })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub requests: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub dollars: f64,
}

/// One key profile's counters and settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// `None`: no limit.
    pub limit: Option<f64>,
    pub measure: Measure,
    pub usage: Usage,
    /// "Hit the limit now": over the limit whatever the counters say.
    pub exhausted: bool,
    pub inject: Option<Injection>,
}

impl Profile {
    /// The usage in the limit's measure.
    pub fn used(&self) -> f64 {
        match self.measure {
            Measure::Requests => self.usage.requests as f64,
            Measure::Tokens => (self.usage.tokens_in + self.usage.tokens_out) as f64,
            Measure::Dollars => self.usage.dollars,
        }
    }

    /// The next request gets the provider's quota error.
    pub fn over(&self) -> bool {
        self.exhausted || self.limit.is_some_and(|l| self.used() >= l)
    }

    /// "3 requests of 10", "120 tokens (no limit)".
    pub fn usage_line(&self) -> String {
        match self.limit {
            Some(l) => format!("{} of {}", self.measure.show(self.used()), self.measure.show(l)),
            None => format!("{} (no limit)", self.measure.show(self.used())),
        }
    }
}

/// What the app's `x-skrongle-call` header says about a request (`ai::call_header`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub purpose: String,
    pub key: String,
    pub job: String,
    pub vars: Vec<(String, String)>,
}

impl Meta {
    pub fn parse(header: &str) -> Meta {
        let mut m = Meta::default();
        for pair in header.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = urldecode(v);
            match k {
                "purpose" => m.purpose = v,
                "key" => m.key = v,
                "job" => m.job = v,
                _ => {
                    if let Some(name) = k.strip_prefix("var.") {
                        m.vars.push((urldecode(name), v));
                    }
                }
            }
        }
        m
    }
}

pub(crate) fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The call site of a request without the header (a client other than this
/// app's): from the prompts, which differ per call site. `None` when they
/// match none.
pub fn guess_purpose(system: &str, messages: &[(String, String)]) -> Option<&'static str> {
    let user = messages.iter().find(|(r, _)| r == "user").map(|(_, t)| t.as_str()).unwrap_or("");
    if system.contains("personal tutor") {
        return Some("tutor-turn");
    }
    [
        ("Edit only this one lesson of the course", "edit-lesson"),
        ("What this lesson must do:", "revise-lesson"),
        ("Now they want it changed.", "revise-plan"),
        ("Now they want its lessons edited.", "edit-plan"),
        ("They want one more lesson.", "add-lesson"),
        ("Write the complete lesson JSON file", "generate-lesson"),
        ("The learner says they want to learn", "generate-outline"),
    ]
    .into_iter()
    .find(|(marker, _)| user.contains(marker))
    .map(|(_, p)| p)
}

/// One request as the simulator sees it, whichever format it came in.
#[derive(Clone, Debug)]
pub struct WireRequest {
    pub format: Format,
    pub who: Who,
    pub key: String,
    pub model: String,
    pub system: String,
    /// (role, text), in order.
    pub messages: Vec<(String, String)>,
    pub meta: Meta,
    /// The body as received, for the log.
    pub body: String,
}

impl WireRequest {
    pub fn purpose(&self) -> String {
        if !self.meta.purpose.is_empty() {
            return self.meta.purpose.clone();
        }
        guess_purpose(&self.system, &self.messages).unwrap_or("unknown").to_string()
    }

    /// The input, estimated in tokens.
    pub fn tokens_in(&self) -> u64 {
        tokens(&self.system) + self.messages.iter().map(|(_, t)| tokens(t)).sum::<u64>()
    }
}

/// Tokens for `text`, the usual rough estimate: 4 characters a token.
pub fn tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// The simulator's price table, US dollars per million tokens (input,
/// output), by model name prefix, first match wins. Made up for the
/// simulator's own counters, not the providers' price lists.
pub const PRICES: [(&str, f64, f64); 8] = [
    ("claude-opus", 15.0, 75.0),
    ("claude-sonnet", 3.0, 15.0),
    ("claude-fable", 3.0, 15.0),
    ("claude-haiku", 1.0, 5.0),
    ("gpt-5-mini", 0.25, 2.0),
    ("gpt-5", 1.25, 10.0),
    ("gemini", 0.30, 2.50),
    ("", 1.0, 5.0),
];

pub fn price(model: &str) -> (f64, f64) {
    PRICES.iter().find(|(p, _, _)| model.starts_with(p)).map(|(_, i, o)| (*i, *o)).unwrap_or((1.0, 5.0))
}

pub fn cost(model: &str, tokens_in: u64, tokens_out: u64) -> f64 {
    let (i, o) = price(model);
    (tokens_in as f64 * i + tokens_out as f64 * o) / 1_000_000.0
}

/// What to do with one request.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Answer with `text` after `delay`; `fixture` names the reply (`generate-outline/default`).
    Reply { text: String, delay: Duration, fixture: String },
    /// An error in the format's shape: HTTP `status`, error `kind`, `message`.
    Fail { status: u16, kind: String, message: String, delay: Duration, why: String },
    /// Hang `secs`, then go on with `then`.
    Hang { secs: u64, then: Box<Plan> },
    /// HTTP 200 whose body is not JSON.
    Malformed,
    /// Manual replies is on: write the request to the capture directory and
    /// answer with the reply typed on `/dev/ai` (`routes::dev_ai`).
    Hold,
}

/// The error type a provider gives an HTTP status.
fn error_kind(format: Format, status: u16) -> &'static str {
    match (format.openai_like(), status) {
        (false, 400) => "invalid_request_error",
        (false, 401) => "authentication_error",
        (false, 403) => "permission_error",
        (false, 404) => "not_found_error",
        (false, 413) => "request_too_large",
        (false, 429) => "rate_limit_error",
        (false, 529) => "overloaded_error",
        (false, _) => "api_error",
        (true, 401) => "invalid_request_error",
        (true, 429) => "rate_limit_exceeded",
        (true, s) if s >= 500 => "server_error",
        (true, _) => "invalid_request_error",
    }
}

/// A fixture's error text (`"the API answered HTTP 401. invalid x-api-key"`)
/// as the wire error that makes the app's client say the same.
pub fn fixture_error(format: Format, text: &str, delay: Duration, why: String) -> Plan {
    let status = text.split("HTTP ").nth(1).and_then(|r| r.get(..3)).and_then(|c| c.parse::<u16>().ok()).filter(|s| (400..600).contains(s));
    let (status, message) = match status {
        Some(s) => {
            let after = text.split_once(&format!("HTTP {s}")).map(|(_, a)| a).unwrap_or("");
            (s, after.trim_start_matches(['.', ':', ' ']).trim().to_string())
        }
        None => (500, text.trim().to_string()),
    };
    Plan::Fail { status, kind: error_kind(format, status).to_string(), message, delay, why }
}

/// The provider's own answer once an account's quota is used up.
///
/// Anthropic: HTTP 400 `invalid_request_error`, "You have reached your
/// specified API usage limits. You will regain access on <first of next
/// month> at 00:00 UTC." (what a workspace or organisation spend limit
/// answers; a prepaid balance at zero says "Your credit balance is too low
/// to access the Anthropic API…" with the same status). OpenAI and Gemini's
/// OpenAI endpoint: HTTP 429 `insufficient_quota`, "You exceeded your
/// current quota, please check your plan and billing details…".
pub fn quota_error(format: Format, now: i64) -> Plan {
    let why = "usage limit".to_string();
    if format.openai_like() {
        return Plan::Fail {
            status: 429,
            kind: "insufficient_quota".into(),
            message: "You exceeded your current quota, please check your plan and billing details. For more information on this error, read the docs: https://platform.openai.com/docs/guides/error-codes/api-errors.".into(),
            delay: Duration::ZERO,
            why,
        };
    }
    let (y, m, _) = civil_from_days(now.div_euclid(86_400));
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    Plan::Fail {
        status: 400,
        kind: "invalid_request_error".into(),
        message: format!("You have reached your specified API usage limits. You will regain access on {ny:04}-{nm:02}-01 at 00:00 UTC."),
        delay: Duration::ZERO,
        why,
    }
}

/// (year, month, day) of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn fault_plan(format: Format, inj: Injection, then: Plan) -> Plan {
    let fail = |status: u16, message: &str| Plan::Fail {
        status,
        kind: error_kind(format, status).to_string(),
        message: message.to_string(),
        delay: Duration::ZERO,
        why: format!("injected: {}", inj.fault.label()),
    };
    match inj.fault {
        Fault::Timeout => Plan::Hang { secs: inj.hang_secs, then: Box::new(then) },
        Fault::ServerError if format.openai_like() => fail(500, "The server had an error while processing your request. Sorry about that!"),
        Fault::ServerError => fail(500, "Internal server error"),
        Fault::Overloaded if format.openai_like() => fail(503, "The server is overloaded or not ready yet."),
        Fault::Overloaded => fail(529, "Overloaded"),
        Fault::InvalidKey if format.openai_like() => fail(401, "Incorrect API key provided. You can find your API key at https://platform.openai.com/account/api-keys."),
        Fault::InvalidKey => fail(401, "invalid x-api-key"),
        Fault::Malformed => Plan::Malformed,
        Fault::WrongShape => Plan::Reply { text: "Sorry, I would rather chat about the weather today.".into(), delay: Duration::ZERO, fixture: "(injected: not valid for the purpose)".into() },
    }
}

/// The reply the fixtures give this request: `<purpose>.json` in `dir`, the
/// entry for the call's key (a lesson slug), else a `[fixture:NAME]` token
/// in the prompts, else `default`; then `*`. `delay_ms` waits, `error` and
/// an `ERROR:` reply answer with that error, `timeout` hangs past the
/// client's limit (docs/testing.md, "Fixtures").
pub fn fixture_plan(req: &WireRequest, dir: &Path) -> Plan {
    let purpose = req.purpose();
    let fail = |message: String| Plan::Fail { status: 500, kind: error_kind(req.format, 500).into(), message, delay: Duration::ZERO, why: "no fixture".into() };
    let path = dir.join(format!("{purpose}.json"));
    let text = match crate::dev::fixture_text(&path) {
        Ok(t) => t,
        Err(e) => return fail(format!("dev simulator: no fixture for purpose '{purpose}' ({}: {e})", path.display())),
    };
    let file: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return fail(format!("dev simulator: {} is not JSON: {e}", path.display())),
    };
    let key = if !req.meta.key.is_empty() {
        req.meta.key.clone()
    } else {
        crate::dev::fixture_token(std::iter::once(req.system.as_str()).chain(req.messages.iter().map(|(_, t)| t.as_str()))).unwrap_or_else(|| "default".into())
    };
    let Some(replies) = file.get("replies") else { return fail(format!("dev simulator: {} has no \"replies\"", path.display())) };
    let (name, entry) = match replies.get(&key) {
        Some(e) => (key.clone(), e),
        None => match replies.get("*") {
            Some(e) => ("*".to_string(), e),
            None => return fail(format!("dev simulator: {} has no reply for key '{key}' and no \"*\"", path.display())),
        },
    };
    let fixture = format!("{purpose}/{name}");
    if entry.get("timeout").and_then(|t| t.as_bool()) == Some(true) {
        let then = Plan::Fail { status: 504, kind: error_kind(req.format, 504).into(), message: "dev simulator: the fixture never answers".into(), delay: Duration::ZERO, why: format!("fixture {fixture}: timeout") };
        return Plan::Hang { secs: HANG_FOREVER_SECS, then: Box::new(then) };
    }
    let delay = entry.get("delay_ms").and_then(|d| d.as_u64()).map(Duration::from_millis).unwrap_or_else(crate::dev::canned_delay);
    if let Some(e) = entry.get("error").and_then(|e| e.as_str()) {
        return fixture_error(req.format, e, delay, format!("fixture {fixture}"));
    }
    let reply = match entry.get("reply") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) => serde_json::to_string_pretty(v).unwrap_or_default(),
        None => return fail(format!("dev simulator: the '{name}' entry in {} has no \"reply\"", path.display())),
    };
    let mut reply = reply.replace("{{key}}", &key).replace("{{job}}", &req.meta.job);
    for (name, value) in &req.meta.vars {
        reply = reply.replace(&format!("{{{{{name}}}}}"), value);
    }
    match reply.strip_prefix("ERROR:") {
        Some(msg) => fixture_error(req.format, msg, delay, format!("fixture {fixture}")),
        None => Plan::Reply { text: reply, delay, fixture },
    }
}

/// One call in the log.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: i64,
    pub at: i64,
    /// `learner`, or `unknown` for a refused key.
    pub who: String,
    /// The start of the key, for telling keys apart.
    pub key_hint: String,
    pub format: Format,
    pub purpose: String,
    pub model: String,
    /// The fixture reply chosen (`generate-outline/default`), if any.
    pub fixture: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub dollars: f64,
    /// 0 while it is still being answered (a hang).
    pub status: u16,
    /// What happened, in a few words (`reply`, `usage limit`, `injected: …`).
    pub outcome: String,
    pub error: String,
    pub request: String,
    pub reply: String,
}

/// The start of a key, never the whole of it.
pub fn key_hint(key: &str) -> String {
    let k: String = key.chars().take(14).collect();
    if key.chars().count() > 14 { format!("{k}…") } else { k }
}

fn clip(s: &str) -> String {
    if s.len() <= BODY_MAX {
        return s.to_string();
    }
    let mut end = BODY_MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… ({} more bytes not kept)", &s[..end], s.len() - end)
}

#[derive(Default)]
struct Inner {
    learner: Profile,
    log: VecDeque<Entry>,
    next_id: i64,
    /// Manual replies: requests wait for a hand-written reply typed on `/dev/ai`.
    hold: bool,
    /// The last held request's number in the capture directory (0: not looked yet).
    hold_seq: u32,
}

impl Inner {
    fn profile(&mut self, who: Who) -> &mut Profile {
        match who {
            Who::Learner => &mut self.learner,
        }
    }
}

/// The simulator's state for this server run (`AppState::dev_ai`).
#[derive(Default)]
pub struct Simulator(Mutex<Inner>);

impl Simulator {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn profile(&self, who: Who) -> Profile {
        self.lock().profile(who).clone()
    }

    pub fn update(&self, who: Who, f: impl FnOnce(&mut Profile)) {
        f(self.lock().profile(who));
    }

    /// A simulator that holds every request from the start (`SKRONGLE_LLM_MODE=capture`).
    pub fn holding_from_start() -> Simulator {
        let s = Simulator::default();
        s.set_hold(true);
        s
    }

    /// Whether Manual replies is on.
    pub fn holding(&self) -> bool {
        self.lock().hold
    }

    pub fn set_hold(&self, on: bool) {
        self.lock().hold = on;
    }

    /// The next held request's number, after the files already in `dir`
    /// (so a restart does not reuse names).
    pub fn next_hold_seq(&self, dir: &Path) -> u32 {
        let mut inner = self.lock();
        if inner.hold_seq == 0 {
            inner.hold_seq = crate::dev::list_requests(dir).last().map(|r| r.seq).unwrap_or(0);
        }
        inner.hold_seq += 1;
        inner.hold_seq
    }

    /// Decide the answer: the quota error once the profile is over its
    /// limit, else an injected failure (a one-shot one is used up), else a
    /// held request when Manual replies is on, else the fixtures.
    pub fn plan(&self, req: &WireRequest, fixtures: &Path, now: i64) -> Plan {
        let (inject, hold) = {
            let mut inner = self.lock();
            let hold = inner.hold;
            let p = inner.profile(req.who);
            if p.over() {
                return quota_error(req.format, now);
            }
            let inj = p.inject;
            if inj.is_some_and(|i| !i.sticky) {
                p.inject = None;
            }
            (inj, hold)
        };
        let answer = if hold { Plan::Hold } else { fixture_plan(req, fixtures) };
        match inject {
            Some(inj) => fault_plan(req.format, inj, answer),
            None => answer,
        }
    }

    /// Count a served reply against the profile.
    pub fn charge(&self, who: Who, model: &str, tokens_in: u64, tokens_out: u64) -> f64 {
        let dollars = cost(model, tokens_in, tokens_out);
        let mut inner = self.lock();
        let u = &mut inner.profile(who).usage;
        u.requests += 1;
        u.tokens_in += tokens_in;
        u.tokens_out += tokens_out;
        u.dollars += dollars;
        dollars
    }

    /// Start a log entry; returns its id.
    pub fn begin(&self, mut e: Entry) -> i64 {
        let mut inner = self.lock();
        inner.next_id += 1;
        e.id = inner.next_id;
        e.request = clip(&e.request);
        let id = e.id;
        inner.log.push_front(e);
        inner.log.truncate(LOG_MAX);
        id
    }

    /// Change a log entry (it may have dropped off the log by now).
    pub fn finish(&self, id: i64, f: impl FnOnce(&mut Entry)) {
        let mut inner = self.lock();
        if let Some(e) = inner.log.iter_mut().find(|e| e.id == id) {
            f(e);
            e.reply = clip(&e.reply);
        }
    }

    /// The log, newest first.
    pub fn log(&self) -> Vec<Entry> {
        self.lock().log.iter().cloned().collect()
    }

    pub fn entry(&self, id: i64) -> Option<Entry> {
        self.lock().log.iter().find(|e| e.id == id).cloned()
    }

    pub fn clear_log(&self) {
        self.lock().log.clear();
    }
}

// ---------- the wire formats ----------

/// A successful reply's body.
pub fn success_body(format: Format, id: i64, model: &str, text: &str, tokens_in: u64, tokens_out: u64, now: i64) -> serde_json::Value {
    if format.openai_like() {
        return serde_json::json!({
            "id": format!("chatcmpl-dev-{id}"),
            "object": "chat.completion",
            "created": now,
            "model": model,
            "choices": [{ "index": 0, "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": tokens_in, "completion_tokens": tokens_out, "total_tokens": tokens_in + tokens_out },
        });
    }
    serde_json::json!({
        "id": format!("msg_dev_{id}"),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{ "type": "text", "text": text }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": { "input_tokens": tokens_in, "output_tokens": tokens_out },
    })
}

/// An error's body, as each provider shapes it.
pub fn error_body(format: Format, kind: &str, message: &str, id: i64) -> serde_json::Value {
    if format.openai_like() {
        let code = match kind {
            "insufficient_quota" => serde_json::json!("insufficient_quota"),
            "invalid_request_error" if message.starts_with("Incorrect API key") => serde_json::json!("invalid_api_key"),
            _ => serde_json::Value::Null,
        };
        return serde_json::json!({ "error": { "message": message, "type": kind, "param": null, "code": code } });
    }
    serde_json::json!({ "type": "error", "error": { "type": kind, "message": message }, "request_id": format!("req_dev_{id}") })
}

/// The models list (`GET …/v1/models`), for clients that look first.
pub fn models_body(format: Format) -> serde_json::Value {
    let names: Vec<&str> = match format {
        Format::Anthropic => vec![ProviderKind::Anthropic.default_model(), "claude-sonnet-5-5", "claude-haiku-4-5"],
        Format::OpenAi => vec![ProviderKind::OpenAi.default_model(), "gpt-5-mini"],
        Format::Google => vec![ProviderKind::Google.default_model()],
    };
    if format.openai_like() {
        return serde_json::json!({ "object": "list", "data": names.iter().map(|n| serde_json::json!({ "id": n, "object": "model", "created": 0, "owned_by": "dev-simulator" })).collect::<Vec<_>>() });
    }
    serde_json::json!({
        "data": names.iter().map(|n| serde_json::json!({ "type": "model", "id": n, "display_name": n, "created_at": "2026-01-01T00:00:00Z" })).collect::<Vec<_>>(),
        "has_more": false,
        "first_id": names.first(),
        "last_id": names.last(),
    })
}

/// The text of a message's `content`: a string, or the text blocks of an array.
pub fn content_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => blocks.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(""),
        _ => String::new(),
    }
}

/// Parse a request body in `format`. `Err` is the provider's 400 message.
pub fn parse_body(format: Format, body: &[u8]) -> Result<(String, String, Vec<(String, String)>), String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("There was an error parsing the body of your request: {e}"))?;
    if v.get("stream").and_then(|s| s.as_bool()) == Some(true) {
        return Err("stream: the dev simulator does not stream; send the request without \"stream\": true".into());
    }
    let model = v.get("model").and_then(|m| m.as_str()).ok_or("model: Field required")?.to_string();
    let msgs = v.get("messages").and_then(|m| m.as_array()).ok_or("messages: Field required")?;
    let mut system = match format {
        Format::Anthropic => {
            if v.get("max_tokens").and_then(|m| m.as_u64()).is_none() {
                return Err("max_tokens: Field required".into());
            }
            v.get("system").map(content_text).unwrap_or_default()
        }
        _ => String::new(),
    };
    let mut messages = Vec::new();
    for m in msgs {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string();
        let text = m.get("content").map(content_text).unwrap_or_default();
        if role == "system" || role == "developer" {
            if !system.is_empty() {
                system.push_str("\n\n");
            }
            system.push_str(&text);
        } else {
            messages.push((role, text));
        }
    }
    if messages.is_empty() {
        return Err("messages: at least one message is required".into());
    }
    Ok((model, system, messages))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("learn-devai-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn req(format: Format, who: Who, meta: &str, user: &str) -> WireRequest {
        WireRequest {
            format,
            who,
            key: "k".into(),
            model: "claude-opus-5-5".into(),
            system: "sys".into(),
            messages: vec![("user".into(), user.into())],
            meta: Meta::parse(meta),
            body: String::new(),
        }
    }

    #[test]
    fn keys_belong_to_profiles() {
        for (_, k) in LEARNER_KEYS {
            assert_eq!(who(k), Some(Who::Learner), "{k}");
        }
        assert_eq!(who("sk-ant-dev-fake-key"), Some(Who::Learner), "the old ?keys=fake key still works");
        assert_eq!(who("sk-ant-dev-mine"), Some(Who::Learner));
        assert_eq!(who("sk-ant-api03-real"), None);
        assert_eq!(who("sk-dev-"), None);
        assert_eq!(who(""), None);
    }

    #[test]
    fn the_call_header_round_trips() {
        let call = crate::ai::Call::new("add-lesson").job("j 1").key("a&b=c").var("last_lesson", "u2-add");
        let m = Meta::parse(&crate::ai::call_header(&call));
        assert_eq!(m, Meta { purpose: "add-lesson".into(), key: "a&b=c".into(), job: "j 1".into(), vars: vec![("last_lesson".into(), "u2-add".into())] });
        assert_eq!(urldecode("%41%zz%"), "A%zz%");
    }

    #[test]
    fn the_purpose_comes_from_the_header_else_the_prompts() {
        let r = req(Format::Anthropic, Who::Learner, "purpose=tutor-turn", "The learner says they want to learn: x");
        assert_eq!(r.purpose(), "tutor-turn");
        let r = req(Format::Anthropic, Who::Learner, "", "The learner says they want to learn: \"x\"");
        assert_eq!(r.purpose(), "generate-outline");
        let r = req(Format::OpenAi, Who::Learner, "", "Write the complete lesson JSON file for this lesson");
        assert_eq!(r.purpose(), "generate-lesson");
        assert_eq!(req(Format::OpenAi, Who::Learner, "", "hello").purpose(), "unknown");
        assert_eq!(guess_purpose("You are Ana's personal tutor for", &[]), Some("tutor-turn"));
    }

    #[test]
    fn fixtures_pick_a_reply_as_the_fixtures_mode_does() {
        let dir = scratch("pick");
        std::fs::write(
            dir.join("tutor-turn.json"),
            r#"{"replies": {"*": {"reply": "Hola {{key}} {{job}} {{last}}"}, "broken": {"error": "the API answered HTTP 401. invalid x-api-key"}, "err": {"reply": "ERROR: the API answered HTTP 429. rate limited"}, "plain": {"error": "boom"}, "slow": {"reply": "late", "delay_ms": 1500}, "dead": {"timeout": true}, "obj": {"reply": {"a": 1}}}}"#,
        )
        .unwrap();
        let plan = |meta: &str, user: &str| fixture_plan(&req(Format::Anthropic, Who::Learner, meta, user), &dir);
        assert_eq!(plan("purpose=tutor-turn&job=j1&var.last=L", "hi"), Plan::Reply { text: "Hola default j1 L".into(), delay: Duration::ZERO, fixture: "tutor-turn/*".into() });
        // A key in the header wins over a token in the prompt.
        assert!(matches!(plan("purpose=tutor-turn&key=obj", "[fixture:broken]"), Plan::Reply { text, .. } if text.contains("\"a\": 1")));
        assert_eq!(
            plan("purpose=tutor-turn", "x [fixture:broken] y"),
            Plan::Fail { status: 401, kind: "authentication_error".into(), message: "invalid x-api-key".into(), delay: Duration::ZERO, why: "fixture tutor-turn/broken".into() }
        );
        assert!(matches!(plan("purpose=tutor-turn", "[fixture:err]"), Plan::Fail { status: 429, kind, message, .. } if kind == "rate_limit_error" && message == "rate limited"));
        assert!(matches!(plan("purpose=tutor-turn", "[fixture:plain]"), Plan::Fail { status: 500, message, .. } if message == "boom"));
        assert!(matches!(plan("purpose=tutor-turn", "[fixture:slow]"), Plan::Reply { delay, .. } if delay == Duration::from_millis(1500)));
        assert!(matches!(plan("purpose=tutor-turn", "[fixture:dead]"), Plan::Hang { secs: HANG_FOREVER_SECS, .. }));
        assert!(matches!(plan("purpose=nope", "hi"), Plan::Fail { status: 500, message, .. } if message.contains("nope")));
        // OpenAI's error types.
        let r = req(Format::OpenAi, Who::Learner, "purpose=tutor-turn", "[fixture:broken]");
        assert!(matches!(fixture_plan(&r, &dir), Plan::Fail { status: 401, kind, .. } if kind == "invalid_request_error"));
        // An edit in memory is what every reader sees, until it is reset.
        let path = dir.join("tutor-turn.json");
        crate::dev::set_fixture_edit(&path, Some(r#"{"replies": {"*": {"reply": "edited"}}}"#.into()));
        assert!(matches!(plan("purpose=tutor-turn", "hi"), Plan::Reply { text, .. } if text == "edited"));
        crate::dev::set_fixture_edit(&path, None);
        assert!(matches!(plan("purpose=tutor-turn", "hi"), Plan::Reply { text, .. } if text.starts_with("Hola")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn usage_is_counted_per_profile_and_the_limit_answers_with_the_quota_error() {
        let dir = scratch("limit");
        std::fs::write(dir.join("tutor-turn.json"), r#"{"replies": {"*": {"reply": "ok"}}}"#).unwrap();
        let sim = Simulator::default();
        let now = 1_792_000_000; // 2026-10-15
        let learner = req(Format::Anthropic, Who::Learner, "purpose=tutor-turn", "hello there");
        sim.update(Who::Learner, |p| p.limit = Some(2.0));
        for _ in 0..2 {
            assert!(matches!(sim.plan(&learner, &dir, now), Plan::Reply { .. }));
            sim.charge(Who::Learner, &learner.model, learner.tokens_in(), 1);
        }
        assert_eq!(sim.profile(Who::Learner).usage.requests, 2);
        assert_eq!(sim.profile(Who::Learner).usage_line(), "2 requests of 2 requests");
        assert_eq!(
            sim.plan(&learner, &dir, now),
            Plan::Fail {
                status: 400,
                kind: "invalid_request_error".into(),
                message: "You have reached your specified API usage limits. You will regain access on 2026-11-01 at 00:00 UTC.".into(),
                delay: Duration::ZERO,
                why: "usage limit".into()
            }
        );
        // "Hit the limit now" stops it too, in OpenAI's shape.
        let o = req(Format::OpenAi, Who::Learner, "purpose=tutor-turn", "hello there");
        sim.update(Who::Learner, |p| p.exhausted = true);
        assert!(matches!(sim.plan(&o, &dir, now), Plan::Fail { status: 429, kind, .. } if kind == "insufficient_quota"));
        sim.update(Who::Learner, |p| p.exhausted = false);
        // Measured in tokens or dollars instead.
        sim.update(Who::Learner, |p| {
            p.measure = Measure::Tokens;
            p.limit = Some(1000.0);
        });
        assert!(matches!(sim.plan(&learner, &dir, now), Plan::Reply { .. }));
        sim.update(Who::Learner, |p| {
            p.measure = Measure::Dollars;
            p.limit = Some(0.0001);
        });
        assert!(sim.profile(Who::Learner).usage.dollars > 0.0001);
        assert!(matches!(sim.plan(&learner, &dir, now), Plan::Fail { status: 400, .. }));
        // Reset: the counters start over.
        sim.update(Who::Learner, |p| p.usage = Usage::default());
        assert!(matches!(sim.plan(&learner, &dir, now), Plan::Reply { .. }));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert!(matches!(quota_error(Format::Anthropic, 1_798_000_000), Plan::Fail { message, .. } if message.contains("2027-01-01")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn injected_failures_are_one_shot_or_sticky() {
        let dir = scratch("inject");
        std::fs::write(dir.join("tutor-turn.json"), r#"{"replies": {"*": {"reply": "ok"}}}"#).unwrap();
        let sim = Simulator::default();
        let r = req(Format::Anthropic, Who::Learner, "purpose=tutor-turn", "hi");
        sim.update(Who::Learner, |p| p.inject = Some(Injection { fault: Fault::Overloaded, sticky: false, hang_secs: 0 }));
        assert!(matches!(sim.plan(&r, &dir, 0), Plan::Fail { status: 529, kind, .. } if kind == "overloaded_error"));
        assert!(matches!(sim.plan(&r, &dir, 0), Plan::Reply { .. }), "one-shot: used up");
        sim.update(Who::Learner, |p| p.inject = Some(Injection { fault: Fault::Timeout, sticky: true, hang_secs: 7 }));
        for _ in 0..2 {
            assert_eq!(sim.plan(&r, &dir, 0), Plan::Hang { secs: 7, then: Box::new(Plan::Reply { text: "ok".into(), delay: Duration::ZERO, fixture: "tutor-turn/*".into() }) });
        }
        let o = req(Format::OpenAi, Who::Learner, "purpose=tutor-turn", "hi");
        sim.update(Who::Learner, |p| p.inject = Some(Injection { fault: Fault::Overloaded, sticky: true, hang_secs: 0 }));
        assert!(matches!(sim.plan(&o, &dir, 0), Plan::Fail { status: 503, .. }));
        sim.update(Who::Learner, |p| p.inject = Some(Injection { fault: Fault::InvalidKey, sticky: true, hang_secs: 0 }));
        assert!(matches!(sim.plan(&r, &dir, 0), Plan::Fail { status: 401, .. }));
        sim.update(Who::Learner, |p| p.inject = Some(Injection { fault: Fault::Malformed, sticky: true, hang_secs: 0 }));
        assert_eq!(sim.plan(&r, &dir, 0), Plan::Malformed);
        // The quota comes before an injected failure.
        sim.update(Who::Learner, |p| p.exhausted = true);
        assert!(matches!(sim.plan(&r, &dir, 0), Plan::Fail { status: 400, .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hold_replies_holds_what_the_fixtures_would_answer() {
        let dir = scratch("hold");
        std::fs::write(dir.join("tutor-turn.json"), r#"{"replies": {"*": {"reply": "ok"}}}"#).unwrap();
        let sim = Simulator::holding_from_start();
        assert!(sim.holding());
        let r = req(Format::Anthropic, Who::Learner, "purpose=tutor-turn", "hi");
        assert_eq!(sim.plan(&r, &dir, 0), Plan::Hold);
        // An injected hang comes first; the quota still answers at once.
        sim.update(Who::Learner, |p| p.inject = Some(Injection { fault: Fault::Timeout, sticky: false, hang_secs: 3 }));
        assert_eq!(sim.plan(&r, &dir, 0), Plan::Hang { secs: 3, then: Box::new(Plan::Hold) });
        sim.update(Who::Learner, |p| p.exhausted = true);
        assert!(matches!(sim.plan(&r, &dir, 0), Plan::Fail { status: 400, .. }));
        sim.update(Who::Learner, |p| p.exhausted = false);
        sim.set_hold(false);
        assert!(matches!(sim.plan(&r, &dir, 0), Plan::Reply { .. }));
        // Held requests are numbered after the files already there.
        std::fs::write(dir.join("0007-tutor-turn.request.json"), "{}").unwrap();
        assert_eq!(sim.next_hold_seq(&dir), 8);
        assert_eq!(sim.next_hold_seq(&dir), 9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bodies_parse_and_answer_in_each_format() {
        let a = br#"{"model": "claude-opus-5-5", "max_tokens": 10, "system": "sys", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]}"#;
        assert_eq!(parse_body(Format::Anthropic, a).unwrap(), ("claude-opus-5-5".into(), "sys".into(), vec![("user".into(), "hi".into())]));
        assert!(parse_body(Format::Anthropic, br#"{"model": "m", "messages": [{"role": "user", "content": "x"}]}"#).unwrap_err().contains("max_tokens"));
        assert!(parse_body(Format::Anthropic, br#"{"model": "m", "max_tokens": 1, "stream": true, "messages": [{"role": "user", "content": "x"}]}"#).unwrap_err().contains("stream"));
        let o = br#"{"model": "gpt-5", "messages": [{"role": "system", "content": "sys"}, {"role": "user", "content": "hi"}]}"#;
        assert_eq!(parse_body(Format::OpenAi, o).unwrap(), ("gpt-5".into(), "sys".into(), vec![("user".into(), "hi".into())]));
        assert!(parse_body(Format::OpenAi, b"not json").is_err());
        let ok = success_body(Format::Anthropic, 3, "m", "hello", 5, 2, 0);
        assert_eq!((ok["content"][0]["text"].as_str(), ok["usage"]["output_tokens"].as_u64()), (Some("hello"), Some(2)));
        let ok = success_body(Format::OpenAi, 3, "m", "hello", 5, 2, 0);
        assert_eq!(ok.pointer("/choices/0/message/content").and_then(|t| t.as_str()), Some("hello"));
        let e = error_body(Format::OpenAi, "insufficient_quota", "You exceeded your current quota", 1);
        assert_eq!((e["error"]["type"].as_str(), e["error"]["code"].as_str()), (Some("insufficient_quota"), Some("insufficient_quota")));
        let e = error_body(Format::Anthropic, "overloaded_error", "Overloaded", 1);
        assert_eq!((e["type"].as_str(), e["error"]["type"].as_str()), (Some("error"), Some("overloaded_error")));
        assert_eq!(tokens("abcde"), 2);
        assert!((cost("claude-opus-5-5", 1_000_000, 0) - 15.0).abs() < 1e-9);
        assert_eq!(price("unheard-of"), (1.0, 5.0));
    }
}
