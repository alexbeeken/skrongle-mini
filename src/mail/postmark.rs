//! The `postmark` provider kind: Postmark's HTTP API
//! (`POST /email`, <https://postmarkapp.com/developer/api/email-api>).
//!
//! Over HTTPS it works where outbound SMTP ports are blocked, and its
//! structured errors tell "this message is refused" (`Rejected`) apart from
//! "this server's setup or Postmark is the problem" (`Unavailable`).
//!
//! Postmark has no idempotency key on `/email`: a retry after a timeout in
//! which Postmark did accept the message sends it twice. The outbox id goes
//! out as `Metadata.outbox_id`, so duplicates can at least be told apart in
//! Postmark's activity log.

use std::time::Duration;

use lettre::message::Mailbox;

use super::{Outgoing, Provider, SendError};

/// The real endpoint; `SKRONGLE_MAIL_<NAME>_API_URL` replaces it (tests, a mock).
pub const API_URL: &str = "https://api.postmarkapp.com/email";
/// The default message stream (Postmark's transactional one).
pub const DEFAULT_STREAM: &str = "outbound";
/// Connect + request + response, like the SMTP timeout.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Postmark `ErrorCode`s that say the server's setup is wrong, not the
/// message: another provider should take over and this one cool down.
/// 10 bad or missing token, 413 account not approved, 422 invalid server,
/// 1235 no such stream, 1236 stream can't send, 1480 IP not allowed.
const SETUP_ERRORS: &[i64] = &[10, 413, 422, 1235, 1236, 1480];

pub struct PostmarkProvider {
    name: String,
    token: String,
    url: String,
    stream: String,
    agent: ureq::Agent,
}

/// `SKRONGLE_MAIL_<NAME>_API_KEY` (the server token, required), `_API_URL`, `_STREAM`.
pub fn from_config(name: &str, get: &dyn Fn(&str) -> Option<String>) -> Result<PostmarkProvider, String> {
    let n = name.to_ascii_uppercase();
    let token = get("API_KEY").ok_or(format!("SKRONGLE_MAIL_{n}_API_KEY is not set (the Postmark server token)"))?;
    let url = get("API_URL").unwrap_or_else(|| API_URL.to_string());
    if !super::api_url_is_safe(&url) {
        return Err(format!("SKRONGLE_MAIL_{n}_API_URL must be an https:// URL (http only for this machine)"));
    }
    let stream = get("STREAM").unwrap_or_else(|| DEFAULT_STREAM.to_string());
    Ok(PostmarkProvider::new(name, &token, &url, &stream))
}

impl PostmarkProvider {
    pub fn new(name: &str, token: &str, url: &str, stream: &str) -> PostmarkProvider {
        let agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).http_status_as_error(false).max_redirects(0).build().into();
        PostmarkProvider { name: name.to_string(), token: token.to_string(), url: url.to_string(), stream: stream.to_string(), agent }
    }

    fn body(&self, from: &Mailbox, mail: &Outgoing) -> serde_json::Value {
        let mut body = serde_json::json!({
            "From": from_header(from),
            "To": mail.to,
            "Subject": mail.subject,
            "TextBody": mail.text,
            "MessageStream": self.stream,
            "Tag": mail.purpose,
            "Metadata": { "outbox_id": mail.id.to_string() },
        });
        if let Some(html) = &mail.html {
            body["HtmlBody"] = html.clone().into();
        }
        if let Some(r) = &mail.reply_to {
            if let Some(o) = body.as_object_mut() {
                o.insert("ReplyTo".into(), r.clone().into());
            }
        }
        body
    }
}

/// `Name <addr>`, the name quoted when it has characters that need it. Not
/// `Mailbox`'s Display, which RFC 2047-encodes non-ASCII names for headers;
/// the JSON is UTF-8.
fn from_header(from: &Mailbox) -> String {
    match from.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        None => from.email.to_string(),
        Some(name) if name.chars().all(|c| c.is_alphanumeric() || " !#$%&'*+-/=?^_`{|}~".contains(c)) => format!("{name} <{}>", from.email),
        Some(name) => format!("\"{}\" <{}>", name.replace('\\', "\\\\").replace('"', "\\\""), from.email),
    }
}

/// What a Postmark answer means for this message. `body` is the response
/// text (JSON with `ErrorCode` and `Message` when Postmark wrote it).
fn classify(status: u16, body: &str) -> Result<(), SendError> {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let code = json["ErrorCode"].as_i64();
    let message = json["Message"].as_str().unwrap_or("").trim();
    let why = |what: &str| match code {
        Some(c) => format!("Postmark HTTP {status}, ErrorCode {c}: {message}"),
        None => format!("Postmark HTTP {status}: {what}"),
    };
    match status {
        200..=299 if code.unwrap_or(0) == 0 => Ok(()),
        200..=299 => Err(SendError::Rejected(why(""))),
        // A bad token, or a setup problem: not this message's fault.
        401 => Err(SendError::Unavailable(why("the server token was refused"))),
        422 if code.is_some_and(|c| SETUP_ERRORS.contains(&c)) => Err(SendError::Unavailable(why(""))),
        // Inactive recipient (406), invalid address or missing body (300),
        // recipient outside the domain while pending approval (412), …
        422 => Err(SendError::Rejected(why("unprocessable"))),
        413 => Err(SendError::Rejected(why("the message is too large"))),
        // 429, 5xx, and anything unexpected.
        _ => Err(SendError::Unavailable(why("no usable answer"))),
    }
}

impl Provider for PostmarkProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, from: &Mailbox, mail: &Outgoing) -> Result<(), SendError> {
        let result = self
            .agent
            .post(&self.url)
            .header("Accept", "application/json")
            .header("X-Postmark-Server-Token", &self.token)
            .send_json(self.body(from, mail));
        // ureq's errors name the URL, never headers, so the token cannot leak here.
        let mut resp = result.map_err(|e| SendError::Unavailable(format!("Postmark request failed: {e}")))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        classify(status, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::Config;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn from() -> Mailbox {
        "Skrongle <noreply@example.org>".parse().unwrap()
    }

    fn outgoing() -> Outgoing {
        Outgoing { id: 1, to: "ann@example.org".into(), subject: "Hi".into(), text: "Hello".into(), html: None, purpose: "test".into(), reply_to: None }
    }

    fn config(vars: &[(&str, &str)]) -> Result<Config, String> {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(&move |name| map.get(name).cloned())
    }

    /// What the fake server saw: the request line, headers (lowercased
    /// names) and the JSON body.
    struct Seen {
        request_line: String,
        headers: Vec<(String, String)>,
        body: serde_json::Value,
    }

    impl Seen {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
        }
    }

    /// A one-request HTTP server on a free port answering `status` with
    /// `body`; hands back what it received.
    fn fake_postmark(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<Seen>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/email", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((n, v)) = line.split_once(':') {
                    headers.push((n.trim().to_ascii_lowercase(), v.trim().to_string()));
                }
            }
            let len: usize = headers.iter().find(|(n, _)| n == "content-length").map(|(_, v)| v.parse().unwrap()).unwrap_or(0);
            let mut raw = vec![0u8; len];
            reader.read_exact(&mut raw).unwrap();
            let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            Seen { request_line: request_line.trim().to_string(), headers, body: serde_json::from_slice(&raw).unwrap_or_default() }
        });
        (url, handle)
    }

    fn postmark(url: &str) -> PostmarkProvider {
        PostmarkProvider::new("pm", "server-token-123", url, "outbound")
    }

    const OK: &str = r#"{"To":"ann@example.org","SubmittedAt":"2026-10-17T10:00:00Z","MessageID":"b7bc2f4a","ErrorCode":0,"Message":"OK"}"#;

    #[test]
    fn sends_the_message_as_postmark_json() {
        let (url, server) = fake_postmark(200, OK);
        let mut m = outgoing();
        m.html = Some("<p>Hello</p>".into());
        let p = PostmarkProvider::new("pm", "server-token-123", &url, "transactional-2");
        p.send(&from(), &m).unwrap();
        let seen = server.join().unwrap();
        assert_eq!(seen.request_line, "POST /email HTTP/1.1");
        assert_eq!(seen.header("x-postmark-server-token"), Some("server-token-123"));
        assert_eq!(seen.header("accept"), Some("application/json"));
        assert!(seen.header("content-type").unwrap().starts_with("application/json"));
        assert_eq!(
            seen.body,
            serde_json::json!({
                "From": "Skrongle <noreply@example.org>", "To": "ann@example.org", "Subject": "Hi",
                "TextBody": "Hello", "HtmlBody": "<p>Hello</p>", "MessageStream": "transactional-2",
                "Tag": "test", "Metadata": { "outbox_id": "1" },
            })
        );
    }

    #[test]
    fn a_plain_message_has_no_html_body() {
        let (url, server) = fake_postmark(200, OK);
        postmark(&url).send(&from(), &outgoing()).unwrap();
        let body = server.join().unwrap().body;
        assert!(body.get("HtmlBody").is_none(), "{body}");
        assert!(body.get("ReplyTo").is_none(), "{body}");
        assert_eq!(body["MessageStream"], "outbound");
    }

    #[test]
    fn a_reply_goes_to_the_reply_to_address() {
        let (url, server) = fake_postmark(200, OK);
        postmark(&url).send(&from(), &Outgoing { reply_to: Some("support@example.org".into()), ..outgoing() }).unwrap();
        assert_eq!(server.join().unwrap().body["ReplyTo"], "support@example.org");
    }

    #[test]
    fn the_sender_name_is_quoted_when_it_needs_it() {
        assert_eq!(from_header(&"noreply@example.org".parse().unwrap()), "noreply@example.org");
        assert_eq!(from_header(&"Español Ñu <a@example.org>".parse().unwrap()), "Español Ñu <a@example.org>");
        assert_eq!(from_header(&"\"Skrongle, Inc.\" <a@example.org>".parse().unwrap()), "\"Skrongle, Inc.\" <a@example.org>");
    }

    #[test]
    fn an_inactive_recipient_is_a_rejection() {
        let (url, _server) = fake_postmark(422, r#"{"ErrorCode":406,"Message":"You tried to send to recipient(s) that have been marked as inactive. Found inactive addresses: ann@example.org."}"#);
        let err = postmark(&url).send(&from(), &outgoing()).unwrap_err();
        let SendError::Rejected(why) = &err else { panic!("{err:?}") };
        assert!(why.contains("ErrorCode 406"), "{why}");
        // The chain scrubs the address before it is logged or stored.
        assert!(!crate::mail::scrub(why).contains("ann@"), "{why}");
        let (url, _server) = fake_postmark(422, r#"{"ErrorCode":300,"Message":"Invalid 'To' address: 'nope'."}"#);
        assert!(matches!(postmark(&url).send(&from(), &outgoing()), Err(SendError::Rejected(_))));
    }

    #[test]
    fn a_bad_server_token_is_unavailable_and_not_echoed() {
        let (url, _server) = fake_postmark(422, r#"{"ErrorCode":10,"Message":"The Server Token you provided in the X-Postmark-Server-Token request header was invalid."}"#);
        let err = postmark(&url).send(&from(), &outgoing()).unwrap_err();
        assert!(matches!(err, SendError::Unavailable(_)), "{err:?}");
        assert!(!err.to_string().contains("server-token-123"), "{err}");
        let (url, _server) = fake_postmark(401, r#"{"ErrorCode":10,"Message":"No Account or Server API tokens were supplied in the HTTP headers."}"#);
        assert!(matches!(postmark(&url).send(&from(), &outgoing()), Err(SendError::Unavailable(_))));
    }

    #[test]
    fn server_errors_and_rate_limits_are_unavailable() {
        for (status, body) in [(500, r#"{"ErrorCode":101,"Message":"Unexpected error"}"#), (503, "<html>maintenance</html>"), (429, "")] {
            let (url, _server) = fake_postmark(status, body);
            let err = postmark(&url).send(&from(), &outgoing()).unwrap_err();
            assert!(matches!(err, SendError::Unavailable(_)), "{status}: {err:?}");
        }
    }

    #[test]
    fn no_server_is_unavailable() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = postmark(&format!("http://127.0.0.1:{closed}/email")).send(&from(), &outgoing()).unwrap_err();
        assert!(matches!(err, SendError::Unavailable(_)), "{err:?}");
        assert!(!err.to_string().contains("server-token-123"), "{err}");
    }

    #[test]
    fn config_reads_the_token_url_and_stream() {
        let c = config(&[
            ("MAIL_FROM", "Skrongle <noreply@example.org>"),
            ("MAIL_PROVIDERS", "ses,pm"),
            ("MAIL_SES_KIND", "smtp"),
            ("MAIL_SES_HOST", "email-smtp.eu-west-1.amazonaws.com"),
            ("MAIL_PM_KIND", "postmark"),
            ("MAIL_PM_API_KEY", "token"),
        ])
        .unwrap();
        assert_eq!(c.chain.names(), ["ses", "pm"]);
        let get = |vars: &'static [(&'static str, &'static str)]| move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let p = from_config("pm", &get(&[("API_KEY", "t")])).unwrap();
        assert_eq!((p.url.as_str(), p.stream.as_str()), (API_URL, "outbound"));
        let p = from_config("pm", &get(&[("API_KEY", "t"), ("API_URL", "http://127.0.0.1:9/email"), ("STREAM", "broadcast")])).unwrap();
        assert_eq!((p.url.as_str(), p.stream.as_str()), ("http://127.0.0.1:9/email", "broadcast"));
    }

    #[test]
    fn config_errors_name_the_variable() {
        let err = |extra: &[(&str, &str)]| config(&[&[("MAIL_FROM", "a@example.org"), ("MAIL_PROVIDERS", "backup"), ("MAIL_BACKUP_KIND", "postmark")][..], extra].concat()).err().unwrap_or_default();
        assert!(err(&[]).contains("SKRONGLE_MAIL_BACKUP_API_KEY"), "{}", err(&[]));
        assert!(err(&[("MAIL_BACKUP_API_KEY", "t"), ("MAIL_BACKUP_API_URL", "http://api.example.org/email")]).contains("SKRONGLE_MAIL_BACKUP_API_URL"));
        // The kind defaults to the name.
        assert!(config(&[("MAIL_FROM", "a@example.org"), ("MAIL_PROVIDERS", "postmark")]).err().unwrap().contains("SKRONGLE_MAIL_POSTMARK_API_KEY"));
    }

    #[test]
    fn postmark_api_url_loopback_is_checked_on_the_parsed_host() {
        let get = |url: &'static str| move |k: &str| match k {
            "API_KEY" => Some("t".to_string()),
            "API_URL" => Some(url.to_string()),
            _ => None,
        };
        for ok in ["https://api.postmarkapp.com/email", "http://127.0.0.1:9/email", "http://localhost/email", "http://[::1]:9/email"] {
            assert!(from_config("pm", &get(ok)).is_ok(), "{ok}");
        }
        for bad in ["http://127.0.0.1.evil.example/email", "http://localhost.evil.example/email", "http://127.0.0.1:9@evil.example/email", "http://api.example.org/email"] {
            assert!(from_config("pm", &get(bad)).err().unwrap_or_default().contains("SKRONGLE_MAIL_PM_API_URL"), "{bad}");
        }
    }
}
