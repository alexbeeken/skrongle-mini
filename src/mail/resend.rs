//! The `resend` kind: Resend's HTTP API (`POST /emails`), for hosts that
//! block outbound SMTP and for errors that tell "this address is bad" from
//! "the service is down".
//!
//! `SKRONGLE_MAIL_<NAME>_API_KEY` (required) and `SKRONGLE_MAIL_<NAME>_API_URL`
//! (default [`API_URL`]; a mock in tests). Each request carries an
//! `Idempotency-Key` made of the outbox id and a hash of the message, so a
//! retry after a timeout does not send twice (Resend keeps keys 24 hours).

use std::time::Duration;

use lettre::message::Mailbox;

use super::{scrub, Outgoing, Provider, SendError};

/// Resend's API; `SKRONGLE_MAIL_<NAME>_API_URL` replaces it.
pub const API_URL: &str = "https://api.resend.com";
/// Connect + whole-request timeout, like SMTP's.
const TIMEOUT: Duration = Duration::from_secs(20);

pub struct ResendProvider {
    name: String,
    api_key: String,
    /// The base URL, without a trailing `/`; requests go to `{url}/emails`.
    url: String,
    agent: ureq::Agent,
}

impl ResendProvider {
    pub fn new(name: &str, api_key: &str, url: &str) -> ResendProvider {
        let agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).http_status_as_error(false).max_redirects(0).build().into();
        ResendProvider { name: name.to_string(), api_key: api_key.to_string(), url: url.trim_end_matches('/').to_string(), agent }
    }

    /// From `SKRONGLE_MAIL_<NAME>_*`; `get(key)` reads `SKRONGLE_MAIL_<NAME>_<key>`.
    pub fn from_config(name: &str, get: &dyn Fn(&str) -> Option<String>) -> Result<ResendProvider, String> {
        let n = name.to_ascii_uppercase();
        let key = get("API_KEY").filter(|k| !k.trim().is_empty()).ok_or(format!("SKRONGLE_MAIL_{n}_API_KEY is not set"))?;
        let url = get("API_URL").unwrap_or_else(|| API_URL.to_string());
        if !super::api_url_is_safe(&url) {
            return Err(format!("SKRONGLE_MAIL_{n}_API_URL must be an https:// URL (http:// only on this machine), not {url:?}"));
        }
        Ok(ResendProvider::new(name, key.trim(), &url))
    }

    /// The JSON body of `POST /emails`.
    fn body(from: &Mailbox, mail: &Outgoing) -> serde_json::Value {
        let mut body = serde_json::json!({
            "from": from_field(from),
            "to": [mail.to],
            "subject": mail.subject,
            "text": mail.text,
        });
        if let Some(html) = &mail.html {
            body["html"] = html.clone().into();
        }
        if let Some(r) = &mail.reply_to {
            if let Some(o) = body.as_object_mut() {
                o.insert("reply_to".into(), r.clone().into());
            }
        }
        body
    }
}

/// `Name <addr>` (the name quoted when it holds punctuation), or the bare address.
fn from_field(from: &Mailbox) -> String {
    match from.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        None => from.email.to_string(),
        Some(name) if name.contains(['(', ')', '<', '>', '[', ']', ':', ';', '@', '\\', ',', '.', '"']) => {
            format!("\"{}\" <{}>", name.replace('\\', "\\\\").replace('"', "\\\""), from.email)
        }
        Some(name) => format!("{name} <{}>", from.email),
    }
}

/// The outbox id plus a hash of what is sent: stable across retries of one
/// message, different for a new message that reuses an id (a reset dev DB).
fn idempotency_key(from: &Mailbox, mail: &Outgoing) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a: stable across builds, unlike DefaultHasher
    let from = from_field(from);
    for part in [from.as_str(), &mail.to, &mail.subject, &mail.text, mail.html.as_deref().unwrap_or("")] {
        for b in part.bytes().chain([0]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("mail-{}-{h:016x}", mail.id)
}

/// What an error answer means for this message. Resend answers
/// `{"statusCode", "name", "message"}`; see resend.com/docs/api-reference/errors.
fn classify(status: u16, body: &str) -> SendError {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let kind = json["name"].as_str().unwrap_or("");
    let message: String = json["message"].as_str().unwrap_or("no message").chars().take(300).collect();
    let why = scrub(&format!("HTTP {status} {kind}: {message}"));
    match (status, kind) {
        // The message itself is refused: a field (the address, say) is invalid,
        // or (403 validation_error) the sender's domain is not verified or the
        // account may only send to its owner.
        (400 | 422, _) | (403, "validation_error") => SendError::Rejected(why),
        // The same key was used for a different message within 24 hours.
        (409, "invalid_idempotent_request") => SendError::Rejected(why),
        // Auth (a wrong, restricted or suspended key), rate limits and quotas,
        // a request with this key still in flight, a wrong URL, 5xx: not about
        // this message, so another provider should take it now and this one later.
        _ => SendError::Unavailable(why),
    }
}

impl Provider for ResendProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, from: &Mailbox, mail: &Outgoing) -> Result<(), SendError> {
        let result = self
            .agent
            .post(&format!("{}/emails", self.url))
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Idempotency-Key", &idempotency_key(from, mail))
            .header("User-Agent", "skrongle")
            .send_json(ResendProvider::body(from, mail));
        // No connection, TLS, timeout: nothing was decided about the message.
        // The error names the URL at most, never the headers.
        let mut resp = result.map_err(|e| SendError::Unavailable(scrub(&e.to_string())))?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = resp.body_mut().with_config().limit(64 * 1024).read_to_string().unwrap_or_default();
        Err(classify(status, &body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// What the fake server saw: the request line, headers (lower-cased names), body.
    #[derive(Debug, Default)]
    struct Seen {
        line: String,
        headers: HashMap<String, String>,
        body: String,
    }

    /// A one-request HTTP server on a free port that answers `status` with
    /// `body` and hands back the request it got.
    fn fake_resend(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<Seen>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut seen = Seen::default();
            reader.read_line(&mut seen.line).unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    seen.headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
            }
            let len = seen.headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).unwrap();
            seen.body = String::from_utf8(buf).unwrap();
            let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            seen
        });
        (url, handle)
    }

    fn from() -> Mailbox {
        "Skrongle <noreply@example.org>".parse().unwrap()
    }

    fn outgoing() -> Outgoing {
        Outgoing { id: 7, to: "ann@example.org".into(), subject: "Hi".into(), text: "Hello".into(), html: Some("<p>Hello</p>".into()), purpose: "test".into(), reply_to: None }
    }

    fn send(url: &str) -> Result<(), SendError> {
        ResendProvider::new("resend", "re_secret_key", url).send(&from(), &outgoing())
    }

    #[test]
    fn sends_the_message_as_json_with_the_key_and_an_idempotency_key() {
        let (url, server) = fake_resend(200, r#"{"id":"49a3999c-0ce1-4ea6-ab68-afcd6dc2e794"}"#);
        send(&url).unwrap();
        let seen = server.join().unwrap();
        assert!(seen.line.starts_with("POST /emails HTTP/1.1"), "{}", seen.line);
        assert_eq!(seen.headers["authorization"], "Bearer re_secret_key");
        assert!(seen.headers["content-type"].starts_with("application/json"), "{:?}", seen.headers);
        let key = &seen.headers["idempotency-key"];
        assert!(key.starts_with("mail-7-") && key.len() <= 256, "{key}");
        let body: serde_json::Value = serde_json::from_str(&seen.body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "from": "Skrongle <noreply@example.org>", "to": ["ann@example.org"], "subject": "Hi", "text": "Hello", "html": "<p>Hello</p>" })
        );
    }

    #[test]
    fn the_idempotency_key_is_stable_per_message_and_differs_between_messages() {
        let (a, mut b) = (outgoing(), outgoing());
        assert_eq!(idempotency_key(&from(), &a), idempotency_key(&from(), &a));
        b.text = "Another".into();
        assert_ne!(idempotency_key(&from(), &a), idempotency_key(&from(), &b));
    }

    #[test]
    fn a_plain_text_message_has_no_html_and_the_sender_is_formatted() {
        let mut m = outgoing();
        m.html = None;
        assert!(ResendProvider::body(&from(), &m).get("html").is_none());
        assert!(ResendProvider::body(&from(), &m).get("reply_to").is_none());
        m.reply_to = Some("support@example.org".into());
        assert_eq!(ResendProvider::body(&from(), &m)["reply_to"], "support@example.org");
        assert_eq!(from_field(&"noreply@example.org".parse().unwrap()), "noreply@example.org");
        assert_eq!(from_field(&"\"Skrongle, Inc.\" <a@example.org>".parse().unwrap()), "\"Skrongle, Inc.\" <a@example.org>");
    }

    #[test]
    fn a_validation_error_is_a_rejection_without_the_address() {
        let (url, _s) = fake_resend(422, r#"{"statusCode":422,"name":"validation_error","message":"Invalid `to` field: ann@example.org"}"#);
        match send(&url) {
            Err(SendError::Rejected(m)) => {
                assert!(m.contains("422") && m.contains("validation_error"), "{m}");
                assert!(!m.contains("ann@example.org") && !m.contains("re_secret_key"), "{m}");
            }
            other => panic!("{other:?}"),
        }
        // An unverified sender domain is about this message too.
        let (url, _s) = fake_resend(403, r#"{"statusCode":403,"name":"validation_error","message":"The example.org domain is not verified."}"#);
        assert!(matches!(send(&url), Err(SendError::Rejected(_))));
    }

    #[test]
    fn server_errors_rate_limits_and_bad_keys_are_unavailable() {
        for (status, body) in [
            (500, r#"{"statusCode":500,"name":"application_error","message":"oops"}"#),
            (429, r#"{"statusCode":429,"name":"rate_limit_exceeded","message":"Too many requests"}"#),
            (401, r#"{"statusCode":401,"name":"validation_error","message":"API key is invalid"}"#),
            (403, r#"{"statusCode":403,"name":"restricted_api_key","message":"This API key is restricted"}"#),
            (503, "<html>bad gateway</html>"),
        ] {
            let (url, _s) = fake_resend(status, body);
            match send(&url) {
                Err(SendError::Unavailable(m)) => assert!(m.contains(&status.to_string()) && !m.contains("re_secret_key"), "{m}"),
                other => panic!("{status}: {other:?}"),
            }
        }
    }

    #[test]
    fn no_server_is_unavailable() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        match send(&format!("http://127.0.0.1:{closed}")) {
            Err(SendError::Unavailable(m)) => assert!(!m.contains("re_secret_key"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    fn config(vars: &[(&str, &str)]) -> Result<super::super::Config, String> {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        super::super::Config::from_lookup(&move |name| map.get(name).cloned())
    }

    #[test]
    fn config_reads_the_key_and_url_and_names_missing_variables() {
        let base = [("MAIL_FROM", "Skrongle <noreply@example.org>")];
        let cfg = |extra: &[(&str, &str)]| config(&[&base[..], extra].concat());
        let c = cfg(&[("MAIL_PROVIDERS", "ses,backup"), ("MAIL_SES_KIND", "smtp"), ("MAIL_SES_HOST", "h"), ("MAIL_BACKUP_KIND", "resend"), ("MAIL_BACKUP_API_KEY", "re_x")]).unwrap();
        assert_eq!(c.chain.names(), ["ses", "backup"]);
        // The kind defaults to the name.
        assert!(cfg(&[("MAIL_PROVIDERS", "resend"), ("MAIL_RESEND_API_KEY", "re_x"), ("MAIL_RESEND_API_URL", "http://127.0.0.1:9")]).is_ok());
        let err = |extra: &[(&str, &str)]| cfg(extra).err().unwrap_or_default();
        assert!(err(&[("MAIL_PROVIDERS", "backup"), ("MAIL_BACKUP_KIND", "resend")]).contains("SKRONGLE_MAIL_BACKUP_API_KEY"));
        assert!(err(&[("MAIL_PROVIDERS", "resend"), ("MAIL_RESEND_API_KEY", " ")]).contains("SKRONGLE_MAIL_RESEND_API_KEY"));
        let e = err(&[("MAIL_PROVIDERS", "resend"), ("MAIL_RESEND_API_KEY", "re_x"), ("MAIL_RESEND_API_URL", "http://api.example.org")]);
        assert!(e.contains("SKRONGLE_MAIL_RESEND_API_URL") && !e.contains("re_x"), "{e}");
        for bad in ["http://127.0.0.1.evil.example", "http://localhost:80@evil.example", "http://localhost:x"] {
            assert!(err(&[("MAIL_PROVIDERS", "resend"), ("MAIL_RESEND_API_KEY", "re_x"), ("MAIL_RESEND_API_URL", bad)]).contains("SKRONGLE_MAIL_RESEND_API_URL"), "{bad}");
        }
    }
}
