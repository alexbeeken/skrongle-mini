//! What went wrong on the wire to a model provider, as a value instead of a
//! sentence. `Display` is the sentence the app has always produced (stored
//! `courses.error` values and the dev provider's tests compare against it);
//! `kind` is what `failure_kind` works out from that sentence, here from the
//! fields instead. The wire code (`send`, `call_anthropic`, `call_openai`,
//! `complete`, `chat`) returns this; the first caller outside it turns it
//! into a `String`.

use super::{quota_exhausted, Failure};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// The provider answered >= 300 with a body. `quota` is true when the
    /// provider's own error field says the account is out of credit.
    Http { status: u16, message: String, quota: bool },
    /// The provider answered a bare status the HTTP client turned into an error.
    Status(u16),
    Timeout { secs: u64 },
    /// The connection failed.
    Unreachable(String),
    /// `stop_reason` "refusal"; the model's explanation, possibly empty.
    Declined(String),
    /// Not JSON, no text, too large. `status` is set when the sentence names one.
    BadReply { status: Option<u16>, message: String },
}

impl AiError {
    pub(super) fn bad_reply(message: impl Into<String>) -> AiError {
        AiError::BadReply { status: None, message: message.into() }
    }

    /// The same error with `f` applied to every piece of provider text.
    pub(super) fn map_text(self, f: impl Fn(&str) -> String) -> AiError {
        match self {
            AiError::Http { status, message, quota } => AiError::Http { status, message: f(&message), quota },
            AiError::Unreachable(m) => AiError::Unreachable(f(&m)),
            AiError::Declined(m) => AiError::Declined(f(&m)),
            AiError::BadReply { status, message } => AiError::BadReply { status, message: f(&message) },
            other @ (AiError::Status(_) | AiError::Timeout { .. }) => other,
        }
    }

    /// What a learner is told about this failure; `None` when nothing in it
    /// is worth explaining beyond the raw text.
    #[cfg_attr(not(test), allow(dead_code))] // used by PR 2 (the stored kind); tested here
    pub(super) fn kind(&self) -> Option<Failure> {
        match self {
            AiError::Http { status, message, quota } => Some(http_kind(*status, *quota || quota_exhausted(message))),
            AiError::Status(status) => Some(http_kind(*status, false)),
            AiError::Timeout { .. } | AiError::Unreachable(_) => Some(Failure::Unreachable),
            AiError::Declined(_) => None,
            AiError::BadReply { status: Some(status), message } => Some(http_kind(*status, quota_exhausted(message))),
            AiError::BadReply { status: None, .. } => Some(Failure::Unusable),
        }
    }
}

/// The learner's reading of an HTTP status; `quota` says the account is out of credit.
#[cfg_attr(not(test), allow(dead_code))]
fn http_kind(status: u16, quota: bool) -> Failure {
    if quota && matches!(status, 400 | 402 | 403 | 429) {
        return Failure::Quota;
    }
    match status {
        401 | 403 => Failure::Key,
        429 | 529 | 500..=599 => Failure::Busy,
        200..=299 => Failure::Unusable,
        400 | 404 => Failure::TurnedDown,
        _ => Failure::ProviderError,
    }
}

/// Provider error fields that say the account is out of credit: OpenAI's
/// `insufficient_quota` (`error.type` and `error.code`) and Anthropic's
/// `billing_error`.
pub(super) fn quota_field(json: &serde_json::Value) -> bool {
    ["/error/type", "/error/code"].iter().filter_map(|p| json.pointer(p)).filter_map(|v| v.as_str()).any(|v| v == "insufficient_quota" || v == "billing_error")
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::Http { status, message, .. } => f.write_str(format!("the API answered HTTP {status}. {message}").trim()),
            AiError::Status(status) => write!(f, "the API answered HTTP {status} (check the key, model name and base URL)"),
            AiError::Timeout { secs } => write!(f, "request failed: timed out after {secs} s"),
            AiError::Unreachable(why) => write!(f, "request failed: {why}"),
            AiError::Declined(why) => f.write_str(format!("the model declined the request. {why}").trim()),
            AiError::BadReply { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for AiError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant prints today's sentence and `kind` agrees with what
    /// `failure_kind` reads back from that sentence.
    #[test]
    fn display_and_kind_match_the_text_classifier() {
        let http = |status: u16, message: &str| AiError::Http { status, message: message.into(), quota: false };
        let not_json = |status: u16, tail: &str| AiError::BadReply { status: Some(status), message: format!("HTTP {status}: reply is not JSON: {tail}") };
        let cases: Vec<(AiError, String)> = vec![
            (http(401, "invalid x-api-key"), "the API answered HTTP 401. invalid x-api-key".into()),
            (http(500, ""), "the API answered HTTP 500.".into()),
            (http(529, "Overloaded"), "the API answered HTTP 529. Overloaded".into()),
            (http(404, "model not found"), "the API answered HTTP 404. model not found".into()),
            (http(418, "teapot"), "the API answered HTTP 418. teapot".into()),
            (http(400, "Your credit balance is too low"), "the API answered HTTP 400. Your credit balance is too low".into()),
            (http(429, "You exceeded your current quota"), "the API answered HTTP 429. You exceeded your current quota".into()),
            (http(429, "slow down"), "the API answered HTTP 429. slow down".into()),
            (AiError::Status(403), "the API answered HTTP 403 (check the key, model name and base URL)".into()),
            (AiError::Status(502), "the API answered HTTP 502 (check the key, model name and base URL)".into()),
            (AiError::Timeout { secs: 120 }, "request failed: timed out after 120 s".into()),
            (AiError::Unreachable("connection refused".into()), "request failed: connection refused".into()),
            (AiError::Declined("".into()), "the model declined the request.".into()),
            (AiError::Declined("It is not allowed".into()), "the model declined the request. It is not allowed".into()),
            (not_json(200, "<html>"), "HTTP 200: reply is not JSON: <html>".into()),
            (not_json(502, "Bad Gateway"), "HTTP 502: reply is not JSON: Bad Gateway".into()),
            (not_json(429, "billing"), "HTTP 429: reply is not JSON: billing".into()),
            (AiError::bad_reply("the model returned no text"), "the model returned no text".into()),
            (AiError::bad_reply("the reply has no choices[0].message.content"), "the reply has no choices[0].message.content".into()),
            (AiError::bad_reply("the reply is larger than 512 KB, more than a course file may be"), "the reply is larger than 512 KB, more than a course file may be".into()),
            (AiError::bad_reply("could not read the reply: io error"), "could not read the reply: io error".into()),
        ];
        for (err, text) in cases {
            assert_eq!(err.to_string(), text);
            let (new, old) = (err.kind().map(|k| k as u8), super::super::failure_kind(&text).map(|k| k as u8));
            assert_eq!(new, old, "{text}");
        }
    }

    /// A provider's own field decides the quota, whatever the message says.
    #[test]
    fn the_providers_error_field_marks_quota() {
        let openai = serde_json::json!({"error": {"message": "Please check your plan", "type": "insufficient_quota", "code": "insufficient_quota"}});
        assert!(quota_field(&openai));
        assert!(!quota_field(&serde_json::json!({"error": {"message": "billing", "type": "invalid_request_error"}})));
        let err = AiError::Http { status: 429, message: "Please check your plan".into(), quota: true };
        assert!(matches!(err.kind(), Some(Failure::Quota)));
        // Only on the statuses a quota error comes with.
        let err = AiError::Http { status: 500, message: "x".into(), quota: true };
        assert!(matches!(err.kind(), Some(Failure::Busy)));
    }

    #[test]
    fn map_text_reaches_every_piece_of_provider_text() {
        let hide = |t: &str| t.replace("sk-secret-key", "***");
        let err = AiError::Http { status: 400, message: "bad sk-secret-key".into(), quota: false }.map_text(hide);
        assert_eq!(err.to_string(), "the API answered HTTP 400. bad ***");
        assert_eq!(AiError::Unreachable("sk-secret-key".into()).map_text(hide).to_string(), "request failed: ***");
        assert_eq!(AiError::Declined("sk-secret-key".into()).map_text(hide).to_string(), "the model declined the request. ***");
        assert_eq!(AiError::bad_reply("sk-secret-key").map_text(hide).to_string(), "***");
    }
}
