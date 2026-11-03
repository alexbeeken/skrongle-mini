//! The dev mailbox: a small SMTP server on 127.0.0.1 that keeps what it is
//! sent in memory, so dev mode and the browser tests send real SMTP and read
//! the result back at `/dev/inbox` (routes/dev.rs).
//!
//! In dev mode without `SKRONGLE_MAIL_PROVIDERS` the chain is one `smtp`
//! provider (`dev-smtp`) pointed here, so a message goes through the outbox,
//! lettre and an SMTP dialogue like it would in production. `SKRONGLE_DEV_SMTP`
//! picks the port (default: a free one) or turns it off (`0`: back to `log`).
//! Anything on this machine may send to it (`swaks --server 127.0.0.1:<port>`).
//!
//! It speaks just enough SMTP for lettre and the usual tools: EHLO/HELO,
//! MAIL, RCPT, DATA, RSET, NOOP, QUIT; no TLS, no AUTH. Each message is
//! decoded (multipart, base64, quoted-printable, RFC 2047 headers) into its
//! text and HTML parts. Only the newest [`KEEP`] messages are kept, and a
//! restart empties it.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use lettre::message::Mailbox;

use super::{Provider, SmtpProvider, SmtpTls};

/// Messages kept; older ones are dropped.
pub const KEEP: usize = 200;
/// The largest message taken (SIZE in EHLO); a bigger one gets 552.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINE: u64 = 64 * 1024;
const IDLE: Duration = Duration::from_secs(60);

/// What `SKRONGLE_DEV_SMTP` asks for: `None` is off.
pub fn port_from_env(v: Option<&str>) -> Result<Option<u16>, String> {
    match v.map(str::trim) {
        None | Some("1" | "true" | "on") => Ok(Some(0)),
        Some("0" | "false" | "off") => Ok(None),
        Some(p) => p.parse().map(Some).map_err(|_| format!("SKRONGLE_DEV_SMTP must be 0 (off), 1 (a free port) or a port number, not {p:?}")),
    }
}

/// One message as the mailbox received it.
pub struct Received {
    pub id: i64,
    pub received_at: i64,
    /// The SMTP envelope: MAIL FROM and every RCPT TO.
    pub envelope_from: String,
    pub envelope_to: Vec<String>,
    /// Headers, decoded.
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub message_id: String,
    /// The outbox row it came from, read out of the app's Message-ID
    /// (`<mail-<id>.<time>@…>`); `None` for mail from anywhere else.
    pub outbox_id: Option<i64>,
    /// The first text/plain and text/html parts, decoded; empty when absent.
    pub text: String,
    pub html: String,
    /// The message as it came over the wire, after dot-unstuffing.
    pub raw: String,
}

impl Received {
    pub fn links(&self) -> Vec<String> {
        links(&self.text, &self.html)
    }

    /// Whether `addr` is a recipient (envelope or To header), ignoring case.
    pub fn is_to(&self, addr: &str) -> bool {
        let addr = addr.trim().to_ascii_lowercase();
        self.envelope_to.iter().any(|t| t.eq_ignore_ascii_case(&addr)) || self.to.to_ascii_lowercase().contains(&addr)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id, "received_at": self.received_at, "outbox_id": self.outbox_id,
            "envelope_from": self.envelope_from, "envelope_to": self.envelope_to,
            "from": self.from, "to": self.to, "subject": self.subject, "date": self.date, "message_id": self.message_id,
            "text": self.text, "html": self.html, "links": self.links(),
        })
    }
}

/// The messages received, newest last.
pub struct Inbox {
    addr: SocketAddr,
    messages: Mutex<VecDeque<Arc<Received>>>,
    next_id: AtomicI64,
}

impl Inbox {
    fn new(addr: SocketAddr) -> Inbox {
        Inbox { addr, messages: Mutex::new(VecDeque::new()), next_id: AtomicI64::new(1) }
    }

    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Newest first; only those to `to` when given.
    pub fn list(&self, to: Option<&str>) -> Vec<Arc<Received>> {
        let all = self.messages.lock().unwrap();
        all.iter().rev().filter(|m| to.is_none_or(|t| m.is_to(t))).cloned().collect()
    }

    pub fn get(&self, id: i64) -> Option<Arc<Received>> {
        self.messages.lock().unwrap().iter().find(|m| m.id == id).cloned()
    }

    /// The newest message that came from outbox row `outbox_id`.
    pub fn by_outbox(&self, outbox_id: i64) -> Option<i64> {
        self.messages.lock().unwrap().iter().rev().find(|m| m.outbox_id == Some(outbox_id)).map(|m| m.id)
    }

    /// Delete one message; whether it was there.
    pub fn remove(&self, id: i64) -> bool {
        let mut all = self.messages.lock().unwrap();
        let before = all.len();
        all.retain(|m| m.id != id);
        all.len() != before
    }

    pub fn clear(&self) {
        self.messages.lock().unwrap().clear();
    }

    fn push(&self, envelope_from: String, envelope_to: Vec<String>, raw: String) -> i64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let p = parse(&raw);
        let header = |name: &str| p.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| decode_words(v)).unwrap_or_default();
        let message_id = header("Message-ID");
        let outbox_id = message_id.trim_start_matches('<').strip_prefix("mail-").and_then(|r| r.split('.').next()).and_then(|n| n.parse().ok());
        let m = Received {
            id,
            received_at: crate::db::now(),
            envelope_from,
            envelope_to,
            from: header("From"),
            to: header("To"),
            subject: header("Subject"),
            date: header("Date"),
            message_id,
            outbox_id,
            text: p.text.unwrap_or_default(),
            html: p.html.unwrap_or_default(),
            raw,
        };
        let mut all = self.messages.lock().unwrap();
        all.push_back(Arc::new(m));
        while all.len() > KEEP {
            all.pop_front();
        }
        id
    }
}

/// Listen on 127.0.0.1:`port` (0: a free one), one thread for the listener
/// and one per connection.
pub fn start(port: u16) -> io::Result<Arc<Inbox>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let inbox = Arc::new(Inbox::new(listener.local_addr()?));
    let shared = inbox.clone();
    std::thread::Builder::new().name("dev-smtp".into()).spawn(move || {
        for conn in listener.incoming() {
            match conn {
                Ok(stream) => {
                    let inbox = shared.clone();
                    let spawned = std::thread::Builder::new().name("dev-smtp-conn".into()).spawn(move || {
                        if let Err(e) = session(stream, &inbox) {
                            tracing::debug!("dev-smtp: connection ended: {e}");
                        }
                    });
                    if let Err(e) = spawned {
                        tracing::warn!("dev-smtp: could not start a connection thread: {e}");
                    }
                }
                Err(e) => tracing::warn!("dev-smtp: accept failed: {e}"),
            }
        }
    })?;
    Ok(inbox)
}

/// The `dev-smtp` provider: plain SMTP to the mailbox.
pub fn provider(inbox: &Inbox, from: &Mailbox) -> Result<Box<dyn Provider>, String> {
    Ok(Box::new(SmtpProvider::new("dev-smtp", "127.0.0.1", inbox.addr.port(), SmtpTls::None, None, from.email.domain())?))
}

// ---------- SMTP ----------

fn session(stream: TcpStream, inbox: &Inbox) -> io::Result<()> {
    stream.set_read_timeout(Some(IDLE))?;
    let mut out = stream.try_clone()?;
    let mut input = BufReader::new(stream);
    let mut say = |s: &str| out.write_all(format!("{s}\r\n").as_bytes());
    say("220 localhost Skrongle dev mailbox ESMTP")?;
    let mut from: Option<String> = None;
    let mut rcpt: Vec<String> = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.by_ref().take(MAX_LINE).read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        let cmd = String::from_utf8_lossy(&line);
        let cmd = cmd.trim_end();
        let verb = cmd.split(' ').next().unwrap_or("").to_ascii_uppercase();
        match verb.as_str() {
            "EHLO" => say(&format!("250-localhost\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250 SIZE {MAX_BYTES}"))?,
            "HELO" => say("250 localhost")?,
            "MAIL" => {
                from = Some(angle(cmd));
                rcpt.clear();
                say("250 2.1.0 OK")?;
            }
            "RCPT" if from.is_none() => say("503 5.5.1 MAIL first")?,
            "RCPT" => {
                rcpt.push(angle(cmd).to_ascii_lowercase());
                say("250 2.1.5 OK")?;
            }
            "DATA" if rcpt.is_empty() => say("503 5.5.1 RCPT first")?,
            "DATA" => {
                say("354 End data with <CR><LF>.<CR><LF>")?;
                match read_data(&mut input)? {
                    Some(raw) => {
                        let id = inbox.push(from.take().unwrap_or_default(), std::mem::take(&mut rcpt), raw);
                        tracing::info!("dev-smtp: received message {id}; read it at /dev/inbox/{id}");
                        say(&format!("250 2.0.0 OK queued as {id}"))?;
                    }
                    None => say("552 5.3.4 Message too big")?,
                }
                from = None;
                rcpt.clear();
            }
            "RSET" => {
                from = None;
                rcpt.clear();
                say("250 2.0.0 OK")?;
            }
            "NOOP" => say("250 2.0.0 OK")?,
            "QUIT" => return say("221 2.0.0 Bye"),
            "" => say("500 5.5.2 Empty command")?,
            _ => say("502 5.5.1 Not implemented")?,
        }
    }
}

/// The address between `<` and `>` in `MAIL FROM:<a@b> BODY=8BITMIME`.
fn angle(cmd: &str) -> String {
    match (cmd.find('<'), cmd.find('>')) {
        (Some(a), Some(b)) if a < b => cmd[a + 1..b].to_string(),
        _ => cmd.split_once(':').map(|(_, r)| r.trim().to_string()).unwrap_or_default(),
    }
}

/// The DATA lines up to the lone `.`, unstuffed; `None` past [`MAX_BYTES`]
/// (read to the end anyway, so the session can go on).
fn read_data(input: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut data = Vec::new();
    let mut line = Vec::new();
    let mut too_big = false;
    loop {
        line.clear();
        if input.by_ref().take(MAX_LINE).read_until(b'\n', &mut line)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed in DATA"));
        }
        if line == b".\r\n" || line == b".\n" {
            break;
        }
        let l = if line.first() == Some(&b'.') { &line[1..] } else { &line[..] };
        if data.len() + l.len() > MAX_BYTES {
            too_big = true;
        } else if !too_big {
            data.extend_from_slice(l);
        }
    }
    Ok((!too_big).then(|| String::from_utf8_lossy(&data).into_owned()))
}

// ---------- MIME ----------

struct Parsed {
    headers: Vec<(String, String)>,
    text: Option<String>,
    html: Option<String>,
}

fn parse(raw: &str) -> Parsed {
    let (headers, body) = split_headers(raw);
    let mut p = Parsed { headers: Vec::new(), text: None, html: None };
    walk(&headers, body, &mut p);
    p.headers = headers;
    p
}

/// The unfolded headers and the body after the blank line.
fn split_headers(raw: &str) -> (Vec<(String, String)>, &str) {
    let (head, body) = match (raw.find("\r\n\r\n"), raw.find("\n\n")) {
        (Some(a), Some(b)) if b < a => (&raw[..b], &raw[b + 2..]),
        (Some(a), _) => (&raw[..a], &raw[a + 4..]),
        (None, Some(b)) => (&raw[..b], &raw[b + 2..]),
        (None, None) => (raw, ""),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = headers.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
        } else if let Some((n, v)) = line.split_once(':') {
            headers.push((n.trim().to_string(), v.trim().to_string()));
        }
    }
    (headers, body)
}

/// Fill in the first text/plain and text/html leaves under this part.
fn walk(headers: &[(String, String)], body: &str, p: &mut Parsed) {
    let get = |name: &str| headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let ctype = get("Content-Type").unwrap_or("text/plain");
    let mime = ctype.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if mime.starts_with("multipart/") {
        let Some(boundary) = param(ctype, "boundary") else { return };
        let delim = format!("--{boundary}");
        let mut parts = body.split(delim.as_str()).skip(1);
        while let Some(part) = parts.next().filter(|s| !s.starts_with("--")) {
            let part = part.strip_prefix("\r\n").or_else(|| part.strip_prefix('\n')).unwrap_or(part);
            let (h, b) = split_headers(part);
            walk(&h, b, p);
        }
        return;
    }
    let slot = match mime.as_str() {
        "text/plain" => &mut p.text,
        "text/html" => &mut p.html,
        _ => return,
    };
    if slot.is_some() {
        return;
    }
    let body = body.strip_suffix("\r\n").unwrap_or(body);
    let decoded = match get("Content-Transfer-Encoding").map(|e| e.trim().to_ascii_lowercase()).as_deref() {
        Some("base64") => {
            let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            base64::engine::general_purpose::STANDARD.decode(compact).unwrap_or_else(|_| body.as_bytes().to_vec())
        }
        Some("quoted-printable") => quoted_printable::decode(body, quoted_printable::ParseMode::Robust).unwrap_or_else(|_| body.as_bytes().to_vec()),
        _ => body.as_bytes().to_vec(),
    };
    *slot = Some(String::from_utf8_lossy(&decoded).replace("\r\n", "\n"));
}

/// `name=value` (quoted or not) from a header like `Content-Type`.
fn param(header: &str, name: &str) -> Option<String> {
    header.split(';').skip(1).find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// RFC 2047 encoded words (`=?utf-8?b?…?=`, `=?utf-8?q?…?=`) decoded; the
/// space between two of them goes.
fn decode_words(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    let mut after_word = false;
    while let Some(start) = rest.find("=?") {
        let Some((word, len)) = encoded_word(&rest[start..]) else { break };
        let between = &rest[..start];
        if !(after_word && between.trim().is_empty()) {
            out.push_str(between);
        }
        out.push_str(&word);
        rest = &rest[start + len..];
        after_word = true;
    }
    out.push_str(rest);
    out
}

/// The encoded word `s` starts with, decoded, and its length.
fn encoded_word(s: &str) -> Option<(String, usize)> {
    let body = s.strip_prefix("=?")?;
    let charset_end = body.find('?')?;
    let enc = body.get(charset_end + 1..charset_end + 2)?.to_ascii_lowercase();
    let text_start = charset_end + 3;
    if body.get(charset_end + 2..text_start)? != "?" {
        return None;
    }
    let text_end = text_start + body[text_start..].find("?=")?;
    let text = &body[text_start..text_end];
    let bytes = match enc.as_str() {
        "b" => base64::engine::general_purpose::STANDARD.decode(text).ok()?,
        "q" => quoted_printable::decode(text.replace('_', " "), quoted_printable::ParseMode::Robust).ok()?,
        _ => return None,
    };
    Some((String::from_utf8_lossy(&bytes).into_owned(), 2 + text_end + 2))
}

/// The http(s) links in a message: `href`s in the HTML, then URLs in the
/// text, each once, in order.
pub fn links(text: &str, html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let hrefs = html.split("href=").skip(1).filter_map(|s| {
        let q = s.chars().next()?;
        let s = if q == '"' || q == '\'' { &s[1..] } else { s };
        let end = s.find(|c: char| c == q || (q != '"' && q != '\'' && (c.is_whitespace() || c == '>'))).unwrap_or(s.len());
        Some(s[..end].replace("&amp;", "&"))
    });
    let urls = text.split_whitespace().map(|w| w.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | ',' | '.' | '"' | '\'')).to_string());
    for l in hrefs.chain(urls) {
        if (l.starts_with("http://") || l.starts_with("https://")) && !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::Outgoing;

    fn send(inbox: &Inbox, subject: &str, text: &str, html: Option<&str>) {
        let from: Mailbox = "Skrongle <noreply@localhost>".parse().unwrap();
        let p = provider(inbox, &from).unwrap();
        let mail = Outgoing { id: 41, to: "Ann@Example.org".into(), subject: subject.into(), text: text.into(), html: html.map(str::to_string), purpose: "test".into(), reply_to: None };
        p.send(&from, &mail).unwrap();
    }

    #[test]
    fn lettre_mail_arrives_decoded() {
        let inbox = start(0).unwrap();
        // Non-ASCII subject (RFC 2047), a long line and accents (quoted-printable
        // or base64), a line starting with a dot (dot-stuffing).
        let long = "word ".repeat(40);
        let text = format!("¡Hola, Ann!\n.hidden dot\n{long}\nhttps://example.org/verify-email?t=abc\n");
        send(&inbox, "Confirma tu dirección — ñ", &text, Some("<p>Hi</p><a href=\"http://127.0.0.1:1/verify-email?t=abc&amp;x=1\">Confirm</a>"));
        let list = inbox.list(Some("ann@example.org"));
        assert_eq!(list.len(), 1);
        let m = &list[0];
        assert_eq!(m.subject, "Confirma tu dirección — ñ");
        assert_eq!(m.envelope_from, "noreply@localhost");
        assert_eq!(m.envelope_to, vec!["ann@example.org"]);
        assert!(m.from.contains("noreply@localhost"), "{}", m.from);
        assert_eq!(m.outbox_id, Some(41));
        assert_eq!(inbox.by_outbox(41), Some(m.id));
        assert_eq!(m.text.trim_end(), text.trim_end());
        assert!(m.html.contains("<a href="), "{}", m.html);
        assert_eq!(m.links(), vec!["http://127.0.0.1:1/verify-email?t=abc&x=1", "https://example.org/verify-email?t=abc"]);
        assert!(inbox.list(Some("bob@example.org")).is_empty());

        send(&inbox, "Plain", "Only text", None);
        assert_eq!(inbox.list(None).iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(), ["Plain", "Confirma tu dirección — ñ"]);
        let plain = &inbox.list(None)[0];
        assert_eq!((plain.text.trim_end(), plain.html.as_str()), ("Only text", ""));
        let plain_id = plain.id;
        assert!(inbox.remove(plain_id));
        assert!(!inbox.remove(plain_id));
        assert_eq!(inbox.list(None).len(), 1);
        inbox.clear();
        assert!(inbox.list(None).is_empty());
    }

    #[test]
    fn speaks_enough_smtp_for_a_hand_written_client() {
        let inbox = start(0).unwrap();
        let s = TcpStream::connect(inbox.addr()).unwrap();
        let mut out = s.try_clone().unwrap();
        let mut input = BufReader::new(s);
        let mut said = |send: &str| {
            if !send.is_empty() {
                out.write_all(send.as_bytes()).unwrap();
            }
            let mut l = String::new();
            loop {
                l.clear();
                input.read_line(&mut l).unwrap();
                if l.as_bytes().get(3) != Some(&b'-') {
                    return l[..3].to_string();
                }
            }
        };
        assert_eq!(said(""), "220");
        assert_eq!(said("HELO x\r\n"), "250");
        assert_eq!(said("RCPT TO:<a@b.c>\r\n"), "503");
        assert_eq!(said("MAIL FROM:<x@y.z> BODY=8BITMIME\r\n"), "250");
        assert_eq!(said("DATA\r\n"), "503");
        assert_eq!(said("RCPT TO:<a@b.c>\r\n"), "250");
        assert_eq!(said("DATA\r\n"), "354");
        assert_eq!(said("Subject: =?utf-8?q?Caf=C3=A9?= =?utf-8?b?IGF5ZXI=?=\r\nTo: a@b.c\r\n\r\n..dot\r\nbody\r\n.\r\n"), "250");
        assert_eq!(said("AUTH PLAIN x\r\n"), "502");
        assert_eq!(said("QUIT\r\n"), "221");
        let m = &inbox.list(Some("a@b.c"))[0];
        assert_eq!((m.subject.as_str(), m.text.as_str(), m.outbox_id), ("Café ayer", ".dot\nbody", None));
    }

    #[test]
    fn the_env_switch() {
        assert_eq!(port_from_env(None), Ok(Some(0)));
        assert_eq!(port_from_env(Some("1")), Ok(Some(0)));
        assert_eq!(port_from_env(Some("0")), Ok(None));
        assert_eq!(port_from_env(Some("2525")), Ok(Some(2525)));
        assert!(port_from_env(Some("lots")).is_err());
    }

    #[test]
    fn links_skip_non_http_and_repeats() {
        let l = links("See https://a.example/x. and (https://a.example/x) mailto:z@y", "<a href='https://b.example/'>b</a><a href=mailto:q@r>q</a>");
        assert_eq!(l, vec!["https://b.example/", "https://a.example/x"]);
    }
}
