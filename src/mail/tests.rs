use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A provider that answers from a script, then succeeds; counts calls.
struct Scripted {
    name: String,
    script: Mutex<Vec<Result<(), SendError>>>,
    calls: Arc<AtomicUsize>,
}

fn scripted(name: &str, script: Vec<Result<(), SendError>>) -> (Box<dyn Provider>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let p = Scripted { name: name.into(), script: Mutex::new(script.into_iter().rev().collect()), calls: Arc::clone(&calls) };
    (Box::new(p), calls)
}

impl Provider for Scripted {
    fn name(&self) -> &str {
        &self.name
    }
    fn send(&self, _: &Mailbox, _: &Outgoing) -> Result<(), SendError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.script.lock().unwrap().pop().unwrap_or(Ok(()))
    }
}

/// Send every due row once, as the job runner would: how many it handled.
fn deliver_due(db: &Db, chain: &Chain, from: &Mailbox, keep_bodies: bool, now: i64) -> rusqlite::Result<usize> {
    let mut n = 0;
    while deliver_one(db, chain, from, None, keep_bodies, "test", now)? {
        n += 1;
    }
    Ok(n)
}

fn from() -> Mailbox {
    "Skrongle <noreply@example.org>".parse().unwrap()
}

fn outgoing() -> Outgoing {
    Outgoing { id: 1, to: "ann@example.org".into(), subject: "Hi".into(), text: "Hello".into(), html: None, purpose: "test".into(), reply_to: None }
}

fn down() -> Result<(), SendError> {
    Err(SendError::Unavailable("connection refused".into()))
}

fn rejected() -> Result<(), SendError> {
    Err(SendError::Rejected("550 no such user".into()))
}

#[test]
fn the_next_provider_takes_over_when_one_fails() {
    let (a, a_calls) = scripted("a", vec![down()]);
    let (b, b_calls) = scripted("b", vec![]);
    let chain = Chain::new(vec![a, b]);
    assert_eq!(chain.send(&from(), &outgoing()).unwrap(), "b");
    assert_eq!((a_calls.load(Ordering::SeqCst), b_calls.load(Ordering::SeqCst)), (1, 1));
}

#[test]
fn an_unreachable_provider_goes_last_until_its_cooldown_ends() {
    let (a, a_calls) = scripted("a", vec![down()]);
    let (b, _) = scripted("b", vec![]);
    let chain = Chain::new(vec![a, b]);
    let t0 = Instant::now();
    assert_eq!(chain.send_at(&from(), &outgoing(), t0).unwrap(), "b");
    // Within the cooldown b goes first; a is not even asked.
    assert_eq!(chain.send_at(&from(), &outgoing(), t0 + Duration::from_secs(10)).unwrap(), "b");
    assert_eq!(a_calls.load(Ordering::SeqCst), 1);
    // After it, a is first again.
    assert_eq!(chain.send_at(&from(), &outgoing(), t0 + COOLDOWN).unwrap(), "a");
}

#[test]
fn a_rejection_tries_the_next_provider_without_a_cooldown() {
    let (a, _) = scripted("a", vec![rejected()]);
    let (b, _) = scripted("b", vec![]);
    let chain = Chain::new(vec![a, b]);
    let t0 = Instant::now();
    assert_eq!(chain.send_at(&from(), &outgoing(), t0).unwrap(), "b");
    assert_eq!(chain.send_at(&from(), &outgoing(), t0).unwrap(), "a");
}

#[test]
fn providers_in_cooldown_are_still_tried_last() {
    let (a, _) = scripted("a", vec![down(), down()]);
    let (b, _) = scripted("b", vec![down()]);
    let chain = Chain::new(vec![a, b]);
    let t0 = Instant::now();
    let err = chain.send_at(&from(), &outgoing(), t0).unwrap_err();
    assert!(!err.all_rejected());
    assert_eq!(err.attempts.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    // Both cooling: tried anyway, in order; b now works.
    assert_eq!(chain.send_at(&from(), &outgoing(), t0).unwrap(), "b");
}

fn row(db: &Db, id: i64) -> (String, i64, String, String, String) {
    db.with(|c| c.query_row("SELECT state, attempts, provider, last_error, body_text FROM mail_outbox WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))
        .unwrap()
}

fn queue(db: &Db, now: i64) -> i64 {
    let m = Mail::new("ann@example.org", "Reset", "Your link: https://x/reset/abc", "test").unwrap();
    db.with(|c| enqueue(c, &m, now, None)).unwrap().unwrap()
}

#[test]
fn a_sent_message_records_the_provider_and_drops_its_body() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, 100);
    let (a, _) = scripted("a", vec![down()]);
    let (b, _) = scripted("b", vec![]);
    let chain = Chain::new(vec![a, b]);
    assert_eq!(deliver_due(&db, &chain, &from(), false, 100).unwrap(), 1);
    assert_eq!(row(&db, id), ("sent".into(), 1, "b".into(), "".into(), "".into()));
    // Nothing left to do.
    assert_eq!(deliver_due(&db, &chain, &from(), false, 100).unwrap(), 0);
}

#[test]
fn dev_mode_keeps_the_body() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, 100);
    let chain = Chain::new(vec![scripted("a", vec![]).0]);
    deliver_due(&db, &chain, &from(), true, 100).unwrap();
    assert!(row(&db, id).4.contains("reset/abc"));
}

fn texts(db: &Db, id: i64) -> (String, String, String, String) {
    db.with(|c| c.query_row("SELECT subject, body_text, body_html, purpose FROM mail_outbox WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))))
        .unwrap()
}

fn queue_code(db: &Db, now: i64) -> i64 {
    let m = Mail::new("ann@example.org", "Your data", "Your link: https://x/profile/export/download?token=482913", crate::exports::PURPOSE).unwrap();
    db.with(|c| enqueue(c, &m, now, None)).unwrap().unwrap()
}

/// #361: a code-bearing message keeps neither its code nor its subject
/// once sent, or once it has failed for good; purpose and recipient stay.
#[test]
fn code_mail_is_blanked_after_sending_and_on_failure() {
    let db = Db::open_in_memory().unwrap();
    let sent = queue_code(&db, 0);
    let other = queue(&db, 0);
    deliver_due(&db, &Chain::new(vec![scripted("a", vec![]).0]), &from(), false, 0).unwrap();
    assert_eq!(texts(&db, sent), ("".into(), "".into(), "".into(), "data-export".into()));
    // A message without a code keeps its subject (bodies go as before).
    assert_eq!(texts(&db, other).0, "Reset");
    assert_eq!(texts(&db, other).1, "");

    let failed = queue_code(&db, 0);
    deliver_due(&db, &Chain::new(vec![scripted("a", vec![rejected()]).0]), &from(), false, 0).unwrap();
    assert_eq!(row(&db, failed).0, "failed");
    assert_eq!(texts(&db, failed), ("".into(), "".into(), "".into(), "data-export".into()));
}

/// Rows settled before #361 (or while bodies were kept) are blanked
/// when the sender starts; queued ones keep what they still have to send.
#[test]
fn settled_rows_with_codes_are_blanked_at_start() {
    let db = Db::open_in_memory().unwrap();
    let old = queue_code(&db, 0);
    let waiting = queue_code(&db, 0);
    db.with(|c| c.execute("UPDATE mail_outbox SET state = 'sent', subject = 'Your sign-in code: 482913' WHERE id = ?1", [old])).unwrap();
    assert_eq!(db.with(|c| blank_settled(c)).unwrap(), 1);
    assert_eq!(texts(&db, old), ("".into(), "".into(), "".into(), "data-export".into()));
    assert!(texts(&db, waiting).1.contains("482913"));
    assert_eq!(db.with(|c| blank_settled(c)).unwrap(), 0);
}

#[test]
fn a_message_no_provider_took_is_retried_then_fails() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, 0);
    let chain = Chain::new(vec![scripted("a", vec![down(); 10]).0]);
    let mut now = 0;
    for (i, delay) in RETRY_DELAYS.iter().enumerate() {
        deliver_due(&db, &chain, &from(), false, now).unwrap();
        let (state, attempts, _, err, _) = row(&db, id);
        assert_eq!((state.as_str(), attempts), ("queued", i as i64 + 1));
        assert!(err.contains("connection refused"), "{err}");
        // Not due before the delay is over.
        assert_eq!(deliver_due(&db, &chain, &from(), false, now + delay - 1).unwrap(), 0);
        now += delay;
    }
    deliver_due(&db, &chain, &from(), false, now).unwrap();
    assert_eq!(row(&db, id).0, "failed");
    assert_eq!(row(&db, id).1, RETRY_DELAYS.len() as i64 + 1);
}

#[test]
fn a_message_every_provider_rejected_fails_at_once() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, 0);
    let chain = Chain::new(vec![scripted("a", vec![rejected()]).0, scripted("b", vec![rejected()]).0]);
    deliver_due(&db, &chain, &from(), false, 0).unwrap();
    let (state, attempts, _, err, _) = row(&db, id);
    assert_eq!((state.as_str(), attempts), ("failed", 1));
    assert!(err.starts_with("a: rejected") && err.contains("b: rejected"), "{err}");
}

#[test]
fn the_job_runner_sends_what_is_queued() {
    let db = Db::open_in_memory().unwrap();
    let (a, _) = scripted("a", vec![down()]);
    let (b, b_calls) = scripted("b", vec![]);
    let (queue, wake) = crate::jobs::Queue::new();
    let outbox = Outbox::new(Chain::new(vec![a, b]), from(), None, false);
    let mailer = Mailer::new(&outbox, None, true, queue.waker());
    crate::jobs::Runner::new().drain(outbox.drain()).start(db.clone(), wake).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let id = rt.block_on(mailer.queue(&db, Mail::new("ann@example.org", "Hi", "Hello", "test").unwrap())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while row(&db, id).0 != "sent" {
        assert!(Instant::now() < deadline, "not sent: {:?}", row(&db, id));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(row(&db, id).2, "b");
    assert_eq!(b_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn errors_are_stored_and_logged_without_addresses() {
    assert_eq!(scrub("550 5.1.1 <ann@example.org>: user unknown"), "550 5.1.1 <address> user unknown");
}

#[test]
fn old_finished_rows_are_purged_and_queued_ones_kept() {
    let db = Db::open_in_memory().unwrap();
    let sent = queue(&db, 0);
    let waiting = queue(&db, 0);
    db.with(|c| c.execute("UPDATE mail_outbox SET state = 'sent' WHERE id = ?1", [sent])).unwrap();
    assert_eq!(db.with(|c| purge(c, KEEP_DAYS * 86_400 - 1)).unwrap(), 0);
    assert_eq!(db.with(|c| purge(c, KEEP_DAYS * 86_400 + 1)).unwrap(), 1);
    assert_eq!(row(&db, waiting).0, "queued");
}

#[test]
fn mail_checks_the_address_and_subject() {
    assert!(Mail::new("not-an-address", "s", "t", "test").is_err());
    assert!(Mail::new("a@b.example", "two\nlines", "t", "test").is_err());
    assert_eq!(Mail::new(" a@b.example ", "s", "t", "test").unwrap().to, "a@b.example");
}

fn config(vars: &[(&str, &str)]) -> Result<Config, String> {
    let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Config::from_lookup(&move |name| map.get(name).cloned())
}

#[test]
fn no_providers_means_logging_only() {
    let c = config(&[]).unwrap();
    assert!(c.logging_only);
    assert_eq!(c.chain.names(), ["log"]);
}

#[test]
fn providers_are_read_in_order_with_their_kinds() {
    let c = config(&[
        ("MAIL_FROM", "Skrongle <noreply@example.org>"),
        ("MAIL_PROVIDERS", "ses, backup"),
        ("MAIL_SES_KIND", "smtp"),
        ("MAIL_SES_HOST", "email-smtp.eu-west-1.amazonaws.com"),
        ("MAIL_SES_USER", "AKIA"),
        ("MAIL_SES_PASS", "secret"),
        ("MAIL_BACKUP_KIND", "smtp"),
        ("MAIL_BACKUP_HOST", "smtp.example.net"),
        ("MAIL_BACKUP_TLS", "tls"),
    ])
    .unwrap();
    assert!(!c.logging_only);
    assert_eq!(c.chain.names(), ["ses", "backup"]);
    assert_eq!(c.from.email.domain(), "example.org");
}

#[test]
fn replies_go_to_the_support_address() {
    assert_eq!(config(&[]).unwrap().reply_to, None);
    let c = config(&[("SUPPORT_EMAIL", " support@example.org ")]).unwrap();
    assert_eq!(c.reply_to.map(|m| m.email.to_string()).as_deref(), Some("support@example.org"));
    assert!(config(&[("SUPPORT_EMAIL", "nonsense")]).err().unwrap().contains("SKRONGLE_SUPPORT_EMAIL"));
    let smtp = SmtpProvider::new("smtp", "localhost", 25, SmtpTls::None, None, "example.org").unwrap();
    let mail = Outgoing { reply_to: Some("support@example.org".into()), ..outgoing() };
    let sent = String::from_utf8(smtp.message(&from(), &mail).unwrap().formatted()).unwrap();
    assert!(sent.contains("Reply-To: support@example.org"), "{sent}");
    let sent = String::from_utf8(smtp.message(&from(), &outgoing()).unwrap().formatted()).unwrap();
    assert!(!sent.contains("Reply-To"), "{sent}");
}

#[test]
fn config_errors_name_the_variable() {
    let base = [("MAIL_FROM", "a@example.org")];
    let err = |extra: &[(&str, &str)]| config(&[&base[..], extra].concat()).err().unwrap_or_default();
    assert!(config(&[("MAIL_PROVIDERS", "smtp")]).err().unwrap().contains("SKRONGLE_MAIL_FROM"));
    assert!(err(&[("MAIL_PROVIDERS", "smtp")]).contains("SKRONGLE_MAIL_SMTP_HOST"));
    assert!(err(&[("MAIL_PROVIDERS", "x"), ("MAIL_X_HOST", "h")]).contains("SKRONGLE_MAIL_X_KIND"));
    assert!(err(&[("MAIL_PROVIDERS", "smtp"), ("MAIL_SMTP_HOST", "h"), ("MAIL_SMTP_USER", "u")]).contains("SKRONGLE_MAIL_SMTP_PASS"));
    assert!(err(&[("MAIL_PROVIDERS", "smtp"), ("MAIL_SMTP_HOST", "mail.example.org"), ("MAIL_SMTP_TLS", "none")]).contains("only for a server on this machine"));
    assert!(err(&[("MAIL_PROVIDERS", "smtp"), ("MAIL_SMTP_HOST", "h"), ("MAIL_SMTP_PORT", "x")]).contains("SKRONGLE_MAIL_SMTP_PORT"));
    assert!(err(&[("MAIL_PROVIDERS", "smtp,smtp"), ("MAIL_SMTP_HOST", "h")]).contains("twice"));
    assert!(err(&[("MAIL_PROVIDERS", "smtp,log"), ("MAIL_SMTP_HOST", "h")]).contains("log cannot be in a chain"));
    assert!(config(&[("MAIL_FROM", "nonsense")]).err().unwrap().contains("SKRONGLE_MAIL_FROM"));
}

/// A one-connection SMTP server on a free port: answers `rcpt` to RCPT
/// TO, then hands back the DATA it received.
fn fake_smtp(rcpt: &'static str) -> (u16, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut out = stream.try_clone().unwrap();
        let mut lines = BufReader::new(stream);
        let mut data = String::new();
        out.write_all(b"220 fake ESMTP\r\n").unwrap();
        let mut line = String::new();
        while lines.read_line(&mut line).unwrap_or(0) > 0 {
            let cmd = line.to_ascii_uppercase();
            let reply = if cmd.starts_with("EHLO") {
                "250 fake\r\n"
            } else if cmd.starts_with("RCPT") {
                rcpt
            } else if cmd.starts_with("DATA") {
                out.write_all(b"354 go\r\n").unwrap();
                let mut l = String::new();
                while lines.read_line(&mut l).unwrap() > 0 && l != ".\r\n" {
                    data.push_str(&l);
                    l.clear();
                }
                "250 queued\r\n"
            } else if cmd.starts_with("QUIT") {
                out.write_all(b"221 bye\r\n").unwrap();
                break;
            } else {
                "250 ok\r\n"
            };
            out.write_all(reply.as_bytes()).unwrap();
            line.clear();
        }
        data
    });
    (port, handle)
}

fn smtp(port: u16) -> SmtpProvider {
    SmtpProvider::new("local", "127.0.0.1", port, SmtpTls::None, None, "example.org").unwrap()
}

#[test]
fn smtp_sends_a_plain_and_html_message() {
    let (port, server) = fake_smtp("250 ok\r\n");
    let mut m = outgoing();
    m.html = Some("<p>Hello</p>".into());
    smtp(port).send(&from(), &m).unwrap();
    let data = server.join().unwrap();
    for want in ["From: Skrongle <noreply@example.org>", "To: ann@example.org", "Subject: Hi", "multipart/alternative", "<p>Hello</p>", "Message-ID: <mail-1."] {
        assert!(data.contains(want), "{want:?} not in\n{data}");
    }
}

#[test]
fn smtp_5xx_is_a_rejection_and_no_server_is_unavailable() {
    let (port, _server) = fake_smtp("550 5.1.1 <ann@example.org>: no such user\r\n");
    assert!(matches!(smtp(port).send(&from(), &outgoing()), Err(SendError::Rejected(_))));
    let (port, _server) = fake_smtp("451 try later\r\n");
    assert!(matches!(smtp(port).send(&from(), &outgoing()), Err(SendError::Unavailable(_))));
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(matches!(smtp(closed).send(&from(), &outgoing()), Err(SendError::Unavailable(_))));
}

fn verify_mail(to: &str, user: Option<i64>) -> Mail {
    let m = Mail::new(to, "Hello", "Link: https://x/somewhere", "test-other").unwrap();
    match user {
        Some(u) => m.for_user(u),
        None => m,
    }
}

fn due_at(db: &Db, id: i64) -> (i64, i64) {
    db.with(|c| c.query_row("SELECT next_attempt_at, scheduled_at FROM mail_outbox WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap()
}

/// #579: `SKRONGLE_MAIL_PER_HOUR` is a budget per purpose per hour. Over
/// it a message is queued for the next hour with room instead of being
/// dropped; another purpose's budget is untouched; 0 turns it off; unset
/// it is [`DEFAULT_PER_HOUR`] except in dev mode.
#[test]
fn over_the_hourly_ceiling_mail_waits_for_the_next_hour() {
    let db = Db::open_in_memory().unwrap();
    let hour = 3600;
    let t0 = 1000 * hour + 10;
    let ceiling = Some(Ceiling { per_window: 3, window: hour });
    let verify = |i: i64| verify_mail(&format!("u{i}@example.org"), None);
    let ids: Vec<i64> = (0..7).map(|i| db.with(|c| enqueue(c, &verify(i), t0 + i, ceiling)).unwrap().unwrap()).collect();
    // Three now, three at the start of the next hour, one the hour after.
    let due: Vec<i64> = ids.iter().map(|&id| due_at(&db, id).0).collect();
    assert_eq!(due, [t0, t0 + 1, t0 + 2, 1001 * hour, 1001 * hour, 1001 * hour, 1002 * hour]);
    assert_eq!(due_at(&db, ids[3]), (1001 * hour, 1001 * hour), "scheduled_at is the hour it counts against");

    // Only so far ahead: with every hour it may wait for full, the message is not queued.
    let full = Some(Ceiling { per_window: 1, window: hour });
    let share = |i: i64| Mail::new(&format!("s{i}@example.org"), "Shared", "t", crate::share::PURPOSE).unwrap();
    for i in 0..=MAX_WAIT_WINDOWS {
        assert!(db.with(|c| enqueue(c, &share(i), t0, full)).unwrap().is_ok());
    }
    assert_eq!(db.with(|c| enqueue(c, &share(99), t0, full)).unwrap(), Err(Capped::Hourly));
    assert!(db.with(|c| enqueue(c, &share(99), t0, None)).unwrap().is_ok(), "no ceiling");

    assert_eq!(config(&[]).unwrap().per_hour(false), Some(DEFAULT_PER_HOUR));
    assert_eq!(config(&[]).unwrap().per_hour(true), None);
    assert_eq!(config(&[("MAIL_PER_HOUR", "40")]).unwrap().per_hour(true), Some(40));
    assert_eq!(config(&[("MAIL_PER_HOUR", "0")]).unwrap().per_hour(false), None);
    assert_eq!(config(&[("MAIL_PER_HOUR", "40")]).unwrap().ceiling(false), Some(Ceiling { per_window: 40, window: DEFAULT_WINDOW }));
    assert!(config(&[("MAIL_PER_HOUR", "lots")]).err().unwrap().contains("SKRONGLE_MAIL_PER_HOUR"));
    assert!(config(&[("MAIL_PER_HOUR", "-1")]).is_err());
    assert_eq!(parse_window(None), Ok(DEFAULT_WINDOW));
    assert_eq!(parse_window(Some(" 60 ".into())), Ok(60));
    assert!(parse_window(Some("0".into())).unwrap_err().contains("SKRONGLE_MAIL_WINDOW"));
}

/// The mailer sends a delayed message once its hour comes, not before.
#[test]
fn a_delayed_message_is_sent_when_its_window_opens() {
    let db = Db::open_in_memory().unwrap();
    let ceiling = Some(Ceiling { per_window: 1, window: 60 });
    let m = |i: i64| Mail::new(&format!("u{i}@example.org"), "Hi", "t", crate::share::PURPOSE).unwrap();
    let first = db.with(|c| enqueue(c, &m(0), 6000, ceiling)).unwrap().unwrap();
    let second = db.with(|c| enqueue(c, &m(1), 6001, ceiling)).unwrap().unwrap();
    let chain = Chain::new(vec![scripted("a", vec![]).0]);
    assert_eq!(deliver_due(&db, &chain, &from(), false, 6059).unwrap(), 1);
    assert_eq!((row(&db, first).0.as_str(), row(&db, second).0.as_str()), ("sent", "queued"));
    assert_eq!(deliver_due(&db, &chain, &from(), false, 6060).unwrap(), 1);
    assert_eq!(row(&db, second).0, "sent");
}

/// #607-#610: the caps, the purge and the start-up sweep read through
/// indexes, never the whole outbox.
#[test]
fn outbox_queries_use_their_indexes() {
    use crate::db::tests::assert_plan_uses;
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        assert_plan_uses(c, ADDRESS_COUNT, "mail_outbox", "idx_mail_outbox_addr");
        assert_plan_uses(c, WINDOW_COUNT, "mail_outbox", "idx_mail_outbox_scheduled");
        assert_plan_uses(c, PURGE_OLD, "mail_outbox", "idx_mail_outbox_created");
        assert_plan_uses(c, PURGE_KEPT, "mail_outbox", "idx_mail_outbox_user");
        // Fails if SECRET_PURPOSES and the index's WHERE (db.rs v58) drift apart.
        assert_plan_uses(c, &blank_settled_sql(), "mail_outbox", "idx_mail_outbox_unblanked");
        Ok(())
    })
    .unwrap();
}

fn lease_of(db: &Db, id: i64) -> (Option<String>, Option<i64>) {
    db.with(|c| c.query_row("SELECT locked_by, locked_until FROM mail_outbox WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap()
}

/// #873: a row being sent is leased, so a second sender (another server,
/// or this one on its next pass) does not get it; when the lease runs out
/// (its server stopped mid-send) the row is sent again, once, and the lease
/// is cleared. The row stays `queued` while leased.
#[test]
fn a_leased_row_is_not_handed_out_twice_and_a_dead_lease_is_taken_again() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, 100);
    let lease = |w: &str, now: i64| db.with(|c| crate::jobs::lease_outbox(c, "mail_outbox", "id", w, now, |r| r.get::<_, i64>(0))).unwrap();
    assert_eq!(lease("a", 100), Some(id));
    assert_eq!(lease("b", 100), None, "a's lease holds it");
    assert_eq!(row(&db, id).0, "queued");
    assert_eq!(lease_of(&db, id), (Some("a".into()), Some(100 + crate::jobs::LEASE)));
    assert_eq!(db.with(|c| next_due(c)).unwrap(), Some(100 + crate::jobs::LEASE), "due again when the lease ends");
    let (p, calls) = scripted("p", vec![]);
    let chain = Chain::new(vec![p]);
    assert_eq!(deliver_due(&db, &chain, &from(), false, 100 + crate::jobs::LEASE - 1).unwrap(), 0, "not while a holds it");
    assert_eq!(deliver_due(&db, &chain, &from(), false, 100 + crate::jobs::LEASE).unwrap(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!((row(&db, id).0.as_str(), row(&db, id).1), ("sent", 1));
    assert_eq!(lease_of(&db, id), (None, None));
    assert_eq!(db.with(|c| next_due(c)).unwrap(), None);
}

/// A retry clears the lease and waits out its delay, as before #873.
#[test]
fn a_retry_clears_the_lease() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, 0);
    deliver_due(&db, &Chain::new(vec![scripted("a", vec![down()]).0]), &from(), false, 0).unwrap();
    assert_eq!(lease_of(&db, id), (None, None));
    assert_eq!(db.with(|c| next_due(c)).unwrap(), Some(RETRY_DELAYS[0]));
    let rows = db.with(|c| recent(c, 1)).unwrap();
    assert_eq!((rows[0]["state"].as_str(), rows[0]["locked_until"].as_i64()), (Some("queued"), None));
}
