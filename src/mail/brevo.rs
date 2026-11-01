//! The `brevo` provider kind: Brevo's transactional e-mail HTTP API
//! (`POST /v3/smtp/email`), for hosts that block outbound SMTP and for
//! errors that tell "this message is refused" from "the service is down".
//!
//! Config (`SKRONGLE_MAIL_<NAME>_…`): `KIND=brevo`, `API_KEY` (required) and
//! `API_URL` (default [`API_URL`]; tests point it at a fake server).
//!
//! Idempotency: Brevo has no idempotency HTTP header; it takes an
//! `idempotencyKey` (a UUID, remembered for 30 minutes) inside the body's
//! `headers` object and refuses a repeat with `duplicate_parameter`. The key
//! is derived from the outbox id and a salt drawn when the provider is built,
//! so a retry of the same row after a timeout cannot send twice, while
//! another server (or a reset dev database) reusing the same row id on the
//! same Brevo account gets other keys.

use std::time::Duration;

use lettre::message::Mailbox;
use sha2::{Digest, Sha256};

use super::{Outgoing, Provider, SendError};

/// The real endpoint.
pub const API_URL: &str = "https://api.brevo.com/v3/smtp/email";
/// Whole-request timeout, like SMTP's.
const TIMEOUT: Duration = Duration::from_secs(20);

pub struct BrevoProvider {
    name: String,
    api_key: String,
    url: String,
    agent: ureq::Agent,
    /// Mixed into idempotency keys (see the module docs).
    salt: [u8; 16],
}

impl BrevoProvider {
    /// `get(key)` reads `SKRONGLE_MAIL_<NAME>_<key>`.
    pub fn from_config(name: &str, get: &dyn Fn(&str) -> Option<String>) -> Result<BrevoProvider, String> {
        let n = name.to_ascii_uppercase();
        let api_key = get("API_KEY").ok_or(format!("SKRONGLE_MAIL_{n}_API_KEY is not set (Brevo: SMTP & API → API Keys)"))?;
        let url = get("API_URL").unwrap_or_else(|| API_URL.to_string());
        if !super::api_url_is_safe(&url) {
            return Err(format!("SKRONGLE_MAIL_{n}_API_URL must be an https:// URL (http:// only on this machine), not {url:?}"));
        }
        Ok(BrevoProvider::new(name, api_key, url))
    }

    pub fn new(name: &str, api_key: String, url: String) -> BrevoProvider {
        let agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).http_status_as_error(false).max_redirects(0).build().into();
        let mut salt = [0u8; 16];
        crate::random::fill(&mut salt);
        BrevoProvider { name: name.to_string(), api_key, url, agent, salt }
    }

    /// A UUID (v4-shaped) for outbox row `id`, the same on every retry from
    /// this process.
    fn idempotency_key(&self, id: i64) -> String {
        let hash = Sha256::new().chain_update(self.salt).chain_update(id.to_be_bytes()).finalize();
        let mut b = [0u8; 16];
        b.copy_from_slice(&hash[..16]);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
    }

    fn body(&self, from: &Mailbox, mail: &Outgoing) -> serde_json::Value {
        let mut sender = serde_json::json!({ "email": from.email.to_string() });
        if let Some(name) = from.name.as_deref().filter(|n| !n.is_empty()) {
            sender["name"] = name.into();
        }
        // Brevo wants htmlContent unless a template is used; a text-only
        // message gets its text, escaped, as the HTML part.
        let html = mail.html.clone().unwrap_or_else(|| text_as_html(&mail.text));
        let mut body = serde_json::json!({
            "sender": sender,
            "to": [{ "email": mail.to }],
            "subject": mail.subject,
            "textContent": mail.text,
            "htmlContent": html,
            "tags": [mail.purpose],
            "headers": { "idempotencyKey": self.idempotency_key(mail.id) },
        });
        if let Some(r) = &mail.reply_to {
            if let Some(o) = body.as_object_mut() {
                o.insert("replyTo".into(), serde_json::json!({ "email": r }));
            }
        }
        body
    }
}

fn text_as_html(text: &str) -> String {
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!("<pre style=\"white-space:pre-wrap;font-family:inherit\">{escaped}</pre>")
}

impl Provider for BrevoProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, from: &Mailbox, mail: &Outgoing) -> Result<(), SendError> {
        let resp = self.agent.post(&self.url).header("api-key", &self.api_key).header("accept", "application/json").send_json(self.body(from, mail));
        // Transport errors name the host at most, never the api-key header.
        let mut resp = resp.map_err(|e| SendError::Unavailable(format!("Brevo: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        outcome(status, &text)
    }
}

/// Brevo's answer as a [`SendError`]. Its errors are `{"code", "message"}`.
fn outcome(status: u16, body: &str) -> Result<(), SendError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let code = json["code"].as_str().unwrap_or("");
    let message: String = json["message"].as_str().unwrap_or("").chars().take(200).collect();
    let why = format!("Brevo answered HTTP {status} {code}: {message}").trim_end_matches([' ', ':']).to_string();
    match status {
        // A repeat of our idempotency key: an earlier attempt at this row
        // (whose answer we never saw) was accepted.
        400 if code == "duplicate_request" || (code == "duplicate_parameter" && message.to_ascii_lowercase().contains("idempotency")) => {
            tracing::info!("Brevo already had this message (idempotency key repeated)");
            Ok(())
        }
        // 400s about the account, not the message: a config problem.
        400 if matches!(code, "unauthorized" | "permission_denied" | "not_enough_credits" | "account_under_validation") => Err(SendError::Unavailable(why)),
        // Invalid or missing parameter, bad address, unverified sender, …
        400 | 413 | 422 => Err(SendError::Rejected(why)),
        // 401/403 (bad key, IP not authorised, account suspended), 429, 5xx, anything else.
        _ => Err(SendError::Unavailable(why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// What the fake server saw.
    struct Seen {
        request_line: String,
        headers: HashMap<String, String>,
        body: serde_json::Value,
    }

    /// A one-request HTTP server on a free port that answers `status` with `reply`.
    fn fake_brevo(status: u16, reply: &'static str) -> (String, std::thread::JoinHandle<Seen>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v3/smtp/email", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut headers = HashMap::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
            }
            let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            Seen { request_line: request_line.trim().to_string(), headers, body: serde_json::from_slice(&body).unwrap() }
        });
        (url, handle)
    }

    fn brevo(url: String) -> BrevoProvider {
        BrevoProvider::new("brevo", "xkeysib-secret".into(), url)
    }

    fn from() -> Mailbox {
        "Skrongle <noreply@example.org>".parse().unwrap()
    }

    fn outgoing() -> Outgoing {
        Outgoing { id: 7, to: "ann@example.org".into(), subject: "Hi".into(), text: "Hello <you>".into(), html: None, purpose: "password-reset".into(), reply_to: None }
    }

    #[test]
    fn sends_the_message_as_brevo_expects() {
        let (url, server) = fake_brevo(201, r#"{"messageId":"<1@smtp-relay.mailin.fr>"}"#);
        let p = brevo(url);
        p.send(&from(), &outgoing()).unwrap();
        let seen = server.join().unwrap();
        assert_eq!(seen.request_line, "POST /v3/smtp/email HTTP/1.1");
        assert_eq!(seen.headers["api-key"], "xkeysib-secret");
        assert!(seen.headers["content-type"].starts_with("application/json"), "{:?}", seen.headers);
        assert_eq!(seen.headers["accept"], "application/json");
        let b = &seen.body;
        assert_eq!(b["sender"], serde_json::json!({ "name": "Skrongle", "email": "noreply@example.org" }));
        assert_eq!(b["to"], serde_json::json!([{ "email": "ann@example.org" }]));
        assert_eq!(b["subject"], "Hi");
        assert_eq!(b["textContent"], "Hello <you>");
        assert_eq!(b["htmlContent"], "<pre style=\"white-space:pre-wrap;font-family:inherit\">Hello &lt;you&gt;</pre>");
        assert_eq!(b["tags"], serde_json::json!(["password-reset"]));
        let key = b["headers"]["idempotencyKey"].as_str().unwrap();
        assert_eq!(key, p.idempotency_key(7));
        assert_eq!((key.len(), &key[14..15]), (36, "4"));
    }

    #[test]
    fn html_is_sent_as_is_and_a_sender_without_a_name_has_none() {
        let (url, server) = fake_brevo(201, "{}");
        let mut m = outgoing();
        m.html = Some("<p>Hello</p>".into());
        m.reply_to = Some("support@example.org".into());
        brevo(url).send(&"noreply@example.org".parse().unwrap(), &m).unwrap();
        let b = server.join().unwrap().body;
        assert_eq!(b["htmlContent"], "<p>Hello</p>");
        assert_eq!(b["replyTo"], serde_json::json!({ "email": "support@example.org" }));
        assert_eq!(b["sender"], serde_json::json!({ "email": "noreply@example.org" }));
    }

    #[test]
    fn idempotency_keys_are_stable_per_row_and_differ_across_rows_and_providers() {
        let p = brevo("http://x".into());
        assert_eq!(p.idempotency_key(1), p.idempotency_key(1));
        assert_ne!(p.idempotency_key(1), p.idempotency_key(2));
        assert_ne!(p.idempotency_key(1), brevo("http://x".into()).idempotency_key(1));
    }

    fn answer(status: u16, reply: &'static str) -> Result<(), SendError> {
        let (url, server) = fake_brevo(status, reply);
        let r = brevo(url).send(&from(), &outgoing());
        server.join().unwrap();
        r
    }

    #[test]
    fn a_bad_request_is_a_rejection() {
        let r = answer(400, r#"{"code":"invalid_parameter","message":"email is not valid in to"}"#);
        assert_eq!(r, Err(SendError::Rejected("Brevo answered HTTP 400 invalid_parameter: email is not valid in to".into())));
    }

    #[test]
    fn account_problems_and_outages_are_unavailable() {
        for (status, reply) in [
            (401, r#"{"code":"unauthorized","message":"Key not found"}"#),
            (403, r#"{"code":"permission_denied","message":"unrecognised IP address"}"#),
            (400, r#"{"code":"not_enough_credits","message":"no credits"}"#),
            (429, r#"{"code":"too_many_requests","message":"slow down"}"#),
            (500, "oops"),
            (503, ""),
        ] {
            let r = answer(status, reply);
            assert!(matches!(&r, Err(SendError::Unavailable(m)) if m.contains(&status.to_string()) && !m.contains("xkeysib")), "{status}: {r:?}");
        }
    }

    #[test]
    fn a_repeated_idempotency_key_means_it_was_sent() {
        assert_eq!(answer(400, r#"{"code":"duplicate_parameter","message":"idempotencyKey already used"}"#), Ok(()));
        assert!(matches!(answer(400, r#"{"code":"duplicate_parameter","message":"duplicate email in to"}"#), Err(SendError::Rejected(_))));
    }

    #[test]
    fn no_server_is_unavailable_without_the_key_in_the_error() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let r = brevo(format!("http://127.0.0.1:{closed}/v3/smtp/email")).send(&from(), &outgoing());
        assert!(matches!(&r, Err(SendError::Unavailable(m)) if !m.contains("xkeysib")), "{r:?}");
    }

    fn config(vars: &[(&str, &str)]) -> Result<super::super::Config, String> {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        super::super::Config::from_lookup(&move |name| map.get(name).cloned())
    }

    #[test]
    fn brevo_is_configured_by_kind_and_key() {
        let c = config(&[
            ("MAIL_FROM", "Skrongle <noreply@example.org>"),
            ("MAIL_PROVIDERS", "ses,backup"),
            ("MAIL_SES_KIND", "smtp"),
            ("MAIL_SES_HOST", "email-smtp.eu-west-1.amazonaws.com"),
            ("MAIL_BACKUP_KIND", "brevo"),
            ("MAIL_BACKUP_API_KEY", "xkeysib-1"),
        ])
        .unwrap();
        assert_eq!(c.chain.names(), ["ses", "backup"]);
        // The kind defaults to the name.
        assert!(config(&[("MAIL_FROM", "a@example.org"), ("MAIL_PROVIDERS", "brevo"), ("MAIL_BREVO_API_KEY", "k")]).is_ok());
    }

    #[test]
    fn brevo_config_errors_name_the_variable() {
        let err = |vars: &[(&str, &str)]| config(&[&[("MAIL_FROM", "a@example.org")][..], vars].concat()).err().unwrap_or_default();
        let e = err(&[("MAIL_PROVIDERS", "backup"), ("MAIL_BACKUP_KIND", "brevo")]);
        assert!(e.contains("SKRONGLE_MAIL_BACKUP_API_KEY"), "{e}");
        let e = err(&[("MAIL_PROVIDERS", "brevo"), ("MAIL_BREVO_API_KEY", "k"), ("MAIL_BREVO_API_URL", "api.brevo.com")]);
        assert!(e.contains("SKRONGLE_MAIL_BREVO_API_URL"), "{e}");
        assert!(err(&[("MAIL_PROVIDERS", "x"), ("MAIL_X_KIND", "nope")]).contains("brevo"));
    }

    #[test]
    fn brevo_api_url_must_be_https_or_this_machine() {
        let get = |url: &'static str| move |k: &str| match k {
            "API_KEY" => Some("xkeysib-1".to_string()),
            "API_URL" => Some(url.to_string()),
            _ => None,
        };
        for ok in ["https://api.brevo.com/v3/smtp/email", "http://127.0.0.1:9/v3/smtp/email", "http://localhost:8025", "http://[::1]:9/x"] {
            assert!(BrevoProvider::from_config("brevo", &get(ok)).is_ok(), "{ok}");
        }
        for bad in ["http://api.brevo.com/v3/smtp/email", "http://127.0.0.1.evil.example/x", "http://localhost:80@evil.example/x", "ftp://x"] {
            let e = BrevoProvider::from_config("brevo", &get(bad)).err().unwrap_or_default();
            assert!(e.contains("SKRONGLE_MAIL_BREVO_API_URL") && !e.contains("xkeysib"), "{bad}: {e}");
        }
    }
}
