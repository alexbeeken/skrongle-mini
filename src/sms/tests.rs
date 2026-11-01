use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Scripted {
    name: String,
    script: Mutex<Vec<Result<(), SendError>>>,
    calls: Arc<AtomicUsize>,
}

/// A provider that answers from a script, then succeeds; counts calls.
fn scripted(name: &str, script: Vec<Result<(), SendError>>) -> (Box<dyn Provider>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let p = Scripted { name: name.into(), script: Mutex::new(script.into_iter().rev().collect()), calls: Arc::clone(&calls) };
    (Box::new(p), calls)
}

/// Send every due row once, as the job runner would: how many it handled.
fn deliver_due(db: &Db, chain: &Chain, keep_bodies: bool, now: i64) -> rusqlite::Result<usize> {
    let mut n = 0;
    while deliver_one(db, chain, keep_bodies, "test", now)? {
        n += 1;
    }
    Ok(n)
}

impl Provider for Scripted {
    fn name(&self) -> &str {
        &self.name
    }
    fn send(&self, _: &Outgoing) -> Result<(), SendError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.script.lock().unwrap().pop().unwrap_or(Ok(()))
    }
}

fn down() -> Result<(), SendError> {
    Err(SendError::Unavailable("connection refused".into()))
}

fn rejected() -> Result<(), SendError> {
    Err(SendError::Rejected("HTTP 400: +447700900123 is not a mobile number".into()))
}

fn outgoing() -> Outgoing {
    Outgoing { id: 7, to: "+447700900123".into(), text: "Your code is 123456".into(), purpose: "test".into() }
}

fn row(db: &Db, id: i64) -> (String, i64, String, String, String) {
    db.with(|c| c.query_row("SELECT state, attempts, provider, last_error, body_text FROM sms_outbox WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))
        .unwrap()
}

fn queue(db: &Db, to: &str, user: Option<i64>, now: i64) -> Result<i64, i64> {
    queue_for(db, "test", to, user, now)
}

fn queue_for(db: &Db, purpose: &'static str, to: &str, user: Option<i64>, now: i64) -> Result<i64, i64> {
    let sms = Sms::new(purpose, to, "Your code is 123456", user).unwrap();
    db.with(|c| enqueue(c, &sms, &Policy::default(), now)).unwrap()
}

fn user(db: &Db, email: &str) -> i64 {
    db.with(|c| crate::auth::create_user(c, email, "x")).unwrap()
}

#[test]
fn numbers_are_normalized_to_e164() {
    assert_eq!(normalize_number(" +44 7700 900-123 ").unwrap(), "+447700900123");
    assert_eq!(normalize_number("+1 (415) 555.0100").unwrap(), "+14155550100");
    for bad in ["07700900123", "+44 7700 abc", "+1234", "+0123456789", "+1234567890123456", ""] {
        assert!(normalize_number(bad).is_err(), "{bad}");
    }
    assert!(Sms::new("t", "+447700900123", "  ", None).is_err());
    assert!(Sms::new("t", "+447700900123", &"x".repeat(MAX_CHARS + 1), None).is_err());
}

#[test]
fn takes_a_us_number_however_it_is_typed() {
    for good in ["(555) 555 - 5555", "5555555555", "555.555.5555", " +1 (555) 555-5555 ", "1 555 555 5555"] {
        assert_eq!(normalize_us_number(good).unwrap(), "+15555555555", "{good}");
    }
    assert_eq!(normalize_us_number("(415) 555 - 0100").unwrap(), "+14155550100");
    // Ten digits, nothing else.
    for short_or_long in ["555555555", "15555555555 5", "", "55555555555"] {
        assert!(normalize_us_number(short_or_long).unwrap_err().contains("10 digits"), "{short_or_long}");
    }
    // Another country's number is named as one, not bent into a +1 one.
    for abroad in ["+44 7700 900123", "+33 1 23 45 67 89", "+4477009001"] {
        assert!(normalize_us_number(abroad).unwrap_err().contains("US and Canada"), "{abroad}");
    }
    assert!(normalize_us_number("(155) 555 - 5555").unwrap_err().contains("0 or 1"));
    assert!(normalize_us_number("(555) 055 - 5555").unwrap_err().contains("0 or 1"));
    assert!(normalize_us_number("555 555 55x5").unwrap_err().contains("digits only"));
}

#[test]
fn shows_a_us_number_the_way_the_field_does() {
    assert_eq!(format_us_number("+15555555555"), "(555) 555 - 5555");
    assert_eq!(format_us_number(""), "");
    // Not a ten-digit +1 number: shown as stored (an older international one).
    assert_eq!(format_us_number("+447700900123"), "+447700900123");
    assert_eq!(format_us_number("+1555555555"), "+1555555555");
}

#[test]
fn a_queued_text_is_in_the_outbox() {
    let db = Db::open_in_memory().unwrap();
    let uid = user(&db, "ann@example.org");
    let id = queue(&db, "+44 7700 900123", Some(uid), 100).unwrap();
    let rows = db.with(|c| recent(c, 10)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], id);
    assert_eq!(rows[0]["to"], "+447700900123");
    assert_eq!(rows[0]["text"], "Your code is 123456");
    assert_eq!(rows[0]["state"], "queued");
    assert_eq!(rows[0]["user_id"], uid);
}

#[test]
fn the_log_provider_marks_sent_and_dev_mode_keeps_the_text() {
    for keep in [true, false] {
        let db = Db::open_in_memory().unwrap();
        let id = queue(&db, "+447700900123", None, 100).unwrap();
        let chain = Config::from_lookup(&|_| None).unwrap().chain;
        assert_eq!(deliver_due(&db, &chain, keep, 100).unwrap(), 1);
        let (state, attempts, provider, _, body) = row(&db, id);
        assert_eq!((state.as_str(), attempts, provider.as_str()), ("sent", 1, "log"));
        assert_eq!(body.is_empty(), !keep);
    }
}

#[test]
fn the_next_provider_takes_over_when_one_fails() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, "+447700900123", None, 100).unwrap();
    let (a, a_calls) = scripted("a", vec![down()]);
    let (b, b_calls) = scripted("b", vec![]);
    let chain = Chain::new(vec![a, b]);
    deliver_due(&db, &chain, false, 100).unwrap();
    assert_eq!(row(&db, id).0, "sent");
    assert_eq!(row(&db, id).2, "b");
    assert_eq!((a_calls.load(Ordering::SeqCst), b_calls.load(Ordering::SeqCst)), (1, 1));
    // a is cooling down: the next text goes to b first.
    let t0 = Instant::now();
    assert_eq!(chain.send_at(&outgoing(), t0).unwrap(), "b");
    assert_eq!(a_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_text_no_provider_took_is_retried_then_fails_and_one_all_rejected_fails_at_once() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, "+447700900123", None, 0).unwrap();
    let chain = Chain::new(vec![scripted("a", vec![down(); 10]).0]);
    let mut now = 0;
    for delay in RETRY_DELAYS {
        deliver_due(&db, &chain, false, now).unwrap();
        assert_eq!(row(&db, id).0, "queued");
        now += delay;
    }
    deliver_due(&db, &chain, false, now).unwrap();
    assert_eq!(row(&db, id).0, "failed");

    let id = queue(&db, "+447700900124", None, now).unwrap();
    let chain = Chain::new(vec![scripted("a", vec![rejected()]).0, scripted("b", vec![rejected()]).0]);
    deliver_due(&db, &chain, false, now).unwrap();
    let (state, attempts, _, err, _) = row(&db, id);
    assert_eq!((state.as_str(), attempts), ("failed", 1));
    // The stored error does not repeat the number.
    assert!(err.contains("a: rejected: HTTP 400: <number> is not") && !err.contains("7700"), "{err}");
}

/// #361: a failed text keeps no code either (dev mode keeps it for the
/// outbox page), and rows settled earlier are blanked when the server starts.
#[test]
fn failed_and_settled_texts_keep_no_code() {
    let db = Db::open_in_memory().unwrap();
    let chain = Chain::new(vec![scripted("a", vec![rejected(), rejected()]).0]);
    let failed = queue(&db, "+447700900123", None, 0).unwrap();
    deliver_due(&db, &chain, false, 0).unwrap();
    assert_eq!((row(&db, failed).0.as_str(), row(&db, failed).4.as_str()), ("failed", ""));
    let dev = queue(&db, "+447700900124", None, 0).unwrap();
    deliver_due(&db, &chain, true, 0).unwrap();
    assert_eq!((row(&db, dev).0.as_str(), row(&db, dev).4.as_str()), ("failed", "Your code is 123456"));

    let waiting = queue(&db, "+447700900125", None, 99).unwrap();
    assert_eq!(db.with(|c| blank_settled(c)).unwrap(), 1);
    assert_eq!(row(&db, dev).4, "");
    assert_eq!(row(&db, waiting).4, "Your code is 123456");
}

#[test]
fn texts_are_limited_per_account_and_per_number() {
    let db = Db::open_in_memory().unwrap();
    let ann = user(&db, "ann@example.org");
    let bob = user(&db, "bob@example.org");
    // Five an hour per account, to any numbers.
    for i in 0..5 {
        queue(&db, &format!("+4477009001{i:02}"), Some(ann), 1000 + i).unwrap();
    }
    let wait = queue(&db, "+447700900199", Some(ann), 1010).unwrap_err();
    assert_eq!(wait, 1000 + 3600 - 1010);
    // Bob is not affected; an hour later Ann may text again.
    queue(&db, "+447700900199", Some(bob), 1010).unwrap();
    queue(&db, "+447700900198", Some(ann), 1000 + 3600).unwrap();
    // Five an hour to one number, whoever sends.
    let db = Db::open_in_memory().unwrap();
    for _ in 0..5 {
        queue(&db, "+447700900123", None, 1000).unwrap();
    }
    assert!(queue(&db, "+447700900123", Some(user(&db, "c@example.org")), 1000).is_err());
    assert!(queue(&db, "+447700900124", None, 1000).is_ok());
}

/// #362: the limits count the `sms_sends` ledger, whose rows stay (with
/// no account) when the account is deleted, so a new account gets no
/// allowance back for the same number.
#[test]
fn the_limits_outlive_the_account() {
    let db = Db::open_in_memory().unwrap();
    let ann = user(&db, "ann@example.org");
    for i in 0..5 {
        queue(&db, "+447700900123", Some(ann), 1000 + i).unwrap();
    }
    assert!(queue(&db, "+447700900123", Some(ann), 1010).is_err());
    assert!(db.with(|c| crate::privacy::delete_account(c, ann)).unwrap().is_some());
    let (rows, orphans): (i64, i64) =
        db.with(|c| c.query_row("SELECT COUNT(*), COUNT(*) - COUNT(user_id) FROM sms_sends", [], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
    assert_eq!((rows, orphans), (5, 5), "the ledger keeps the deleted account's texts");
    assert_eq!(db.with(|c| c.query_row("SELECT COUNT(*) FROM sms_outbox", [], |r| r.get::<_, i64>(0))).unwrap(), 0);
    let again = user(&db, "ann2@example.org");
    assert!(queue(&db, "+447700900123", Some(again), 1010).is_err(), "a fresh account must not reset the number's limit");
    // The ledger holds no number, only its hash.
    let key: String = db.with(|c| c.query_row("SELECT number_key FROM sms_sends LIMIT 1", [], |r| r.get(0))).unwrap();
    assert!(!key.contains("7700") && key.len() == 64 && key == number_key("+447700900123"));
    // Two days on, purge drops the ledger rows.
    db.with(|c| purge(c, 1000 + 2 * 86_400 + 10)).unwrap();
    assert_eq!(db.with(|c| c.query_row("SELECT COUNT(*) FROM sms_sends", [], |r| r.get::<_, i64>(0))).unwrap(), 0);
}

/// #363: verification texts to a number cannot use up its sign-in
/// codes' allowance (and the other way round).
#[test]
fn sign_in_codes_have_their_own_allowance() {
    let db = Db::open_in_memory().unwrap();
    let to = "+447700900123";
    for i in 0..5 {
        queue_for(&db, crate::two_factor::PURPOSE_PHONE, to, None, 1000 + i).unwrap();
    }
    assert!(queue_for(&db, crate::two_factor::PURPOSE_PHONE, to, None, 1010).is_err());
    for i in 0..5 {
        queue_for(&db, crate::two_factor::PURPOSE_LOGIN, to, None, 1010 + i).unwrap();
    }
    assert!(queue_for(&db, crate::two_factor::PURPOSE_CONFIRM, to, None, 1020).is_err(), "sign-in codes have a limit too");
    assert_eq!(Kind::of(crate::two_factor::PURPOSE_CONFIRM), Kind::SignIn);
    assert_eq!(Kind::of("dev-test"), Kind::Other);
}

/// #362: the whole server sends at most `per_hour` / `per_day` texts of
/// each kind, whatever the accounts and numbers.
#[test]
fn the_server_wide_cap_holds_for_each_kind() {
    let db = Db::open_in_memory().unwrap();
    let policy = Policy { per_hour: 3, per_day: 4, countries: None };
    let put = |purpose: &'static str, i: i64, now: i64| {
        let sms = Sms::new(purpose, &format!("+4477009002{i:02}"), "hi", None).unwrap();
        db.with(|c| enqueue(c, &sms, &policy, now)).unwrap()
    };
    for i in 0..3 {
        put("test", i, 1000).unwrap();
    }
    assert_eq!(put("test", 9, 1000), Err(3600));
    put(crate::two_factor::PURPOSE_LOGIN, 10, 1000).unwrap();
    put("test", 11, 1000 + 3600).unwrap();
    assert!(put("test", 12, 1000 + 7200).is_err(), "the daily cap");
}

/// #362: `SKRONGLE_SMS_COUNTRIES` (calling codes, or `*`), with a default
/// list, and the caps from `SKRONGLE_SMS_MAX_PER_*`.
#[test]
fn only_listed_countries_are_texted() {
    let p = Policy::default();
    assert!(p.allows("+447700900123") && p.allows("+14155550100") && p.allows("+34600000000"));
    assert!(!p.allows("+8613800138000") && !p.allows("+2348000000000") && !p.allows("+88213000000"));
    let cfg = config(&[("SMS_COUNTRIES", "+44, 1415"), ("SMS_MAX_PER_HOUR", "7"), ("SMS_MAX_PER_DAY", "9")]).unwrap();
    assert!(cfg.policy.allows("+447700900123") && cfg.policy.allows("+14155550100"));
    assert!(!cfg.policy.allows("+12125550100"), "a longer prefix narrows a code");
    assert_eq!((cfg.policy.per_hour, cfg.policy.per_day), (7, 9));
    assert!(config(&[("SMS_COUNTRIES", "*")]).unwrap().policy.allows("+8613800138000"));
    assert!(config(&[]).unwrap().policy == Policy::default());
    for bad in ["", "uk", "044"] {
        assert!(config(&[("SMS_COUNTRIES", bad)]).err().unwrap().contains("SKRONGLE_SMS_COUNTRIES"), "{bad}");
    }
    assert!(config(&[("SMS_MAX_PER_DAY", "lots")]).err().unwrap().contains("SKRONGLE_SMS_MAX_PER_DAY"));
    // send_sms refuses before anything is queued.
    let db = Db::open_in_memory().unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let texter = Texter::idle();
    assert!(matches!(rt.block_on(texter.send_sms(&db, "test", "+8613800138000", "Hello", None)), Err(SmsError::Country)));
    assert_eq!(db.with(|c| c.query_row("SELECT COUNT(*) FROM sms_sends", [], |r| r.get::<_, i64>(0))).unwrap(), 0);
}

#[test]
fn send_sms_queues_wakes_the_runner_and_refuses_over_the_limit() {
    let db = Db::open_in_memory().unwrap();
    let (a, _) = scripted("a", vec![down()]);
    let (b, _) = scripted("b", vec![]);
    let (queue, wake) = crate::jobs::Queue::new();
    let outbox = Outbox::new(Chain::new(vec![a, b]), false);
    let texter = Texter::new(&outbox, Policy::default(), true, queue.waker());
    crate::jobs::Runner::new().drain(outbox.drain()).start(db.clone(), wake).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let id = rt.block_on(texter.send_sms(&db, "test", "+447700900123", "Hello", None)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while row(&db, id).0 != "sent" {
        assert!(Instant::now() < deadline, "not sent: {:?}", row(&db, id));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(row(&db, id).2, "b");
    assert!(matches!(rt.block_on(texter.send_sms(&db, "test", "07700", "Hello", None)), Err(SmsError::Invalid(_))));
    for _ in 0..4 {
        rt.block_on(texter.send_sms(&db, "test", "+447700900123", "Hello", None)).unwrap();
    }
    assert!(matches!(rt.block_on(texter.send_sms(&db, "test", "+447700900123", "Hello", None)), Err(SmsError::RateLimited { .. })));
}

#[test]
fn old_finished_rows_are_purged_and_queued_ones_kept() {
    let db = Db::open_in_memory().unwrap();
    let sent = queue(&db, "+447700900123", None, 0).unwrap();
    let waiting = queue(&db, "+447700900123", None, 0).unwrap();
    db.with(|c| c.execute("UPDATE sms_outbox SET state = 'sent' WHERE id = ?1", [sent])).unwrap();
    assert_eq!(db.with(|c| purge(c, KEEP_DAYS * 86_400 + 1)).unwrap(), 1);
    assert_eq!(row(&db, waiting).0, "queued");
}

#[test]
fn templates_fill_placeholders_once_and_escape_them() {
    let mut sms = outgoing();
    sms.text = "Say \"hi\" & {to}\n".into();
    let json = render(BodyFormat::Json.default_template(), BodyFormat::Json, "Skrongle", &sms);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["text"], "Say \"hi\" & {to}\n");
    assert_eq!(v["to"], "+447700900123");
    assert_eq!(v["id"], "7");
    let form = render(BodyFormat::Form.default_template(), BodyFormat::Form, "+15005550006", &sms);
    assert_eq!(form, "To=%2B447700900123&From=%2B15005550006&Body=Say%20%22hi%22%20%26%20%7Bto%7D%0A");
    assert_eq!(render(r#"{"m":{"n":"{to}"}}"#, BodyFormat::Json, "", &sms), r#"{"m":{"n":"+447700900123"}}"#);
}

#[test]
fn basic_auth_is_standard_base64() {
    // RFC 7617's example.
    assert_eq!(basic_auth("Aladdin", "open sesame"), "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
}

fn config(vars: &[(&str, &str)]) -> Result<Config, String> {
    let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Config::from_lookup(&move |name| map.get(name).cloned())
}

#[test]
fn no_providers_means_logging_only_and_a_chain_is_read_in_order() {
    let c = config(&[]).unwrap();
    assert!(c.logging_only);
    assert_eq!(c.chain.names(), ["log"]);
    let c = config(&[
        ("SMS_PROVIDERS", "twilio, hook"),
        ("SMS_FROM", "+15005550006"),
        ("SMS_TWILIO_KIND", "http"),
        ("SMS_TWILIO_URL", "https://api.twilio.com/2010-04-01/Accounts/AC1/Messages.json"),
        ("SMS_TWILIO_FORMAT", "form"),
        ("SMS_TWILIO_USER", "AC1"),
        ("SMS_TWILIO_PASS", "secret"),
        ("SMS_HOOK_KIND", "http"),
        ("SMS_HOOK_URL", "http://127.0.0.1:9000/sms"),
        ("SMS_HOOK_AUTH", "Bearer abc"),
    ])
    .unwrap();
    assert!(!c.logging_only);
    assert_eq!(c.chain.names(), ["twilio", "hook"]);
}

#[test]
fn config_errors_name_the_variable() {
    let err = |vars: &[(&str, &str)]| config(&[&[("SMS_PROVIDERS", "x"), ("SMS_X_KIND", "http")][..], vars].concat()).err().unwrap_or_default();
    assert!(err(&[]).contains("SKRONGLE_SMS_X_URL"));
    assert!(err(&[("SMS_X_URL", "http://sms.example.org/send")]).contains("https://"));
    assert!(err(&[("SMS_X_URL", "ftp://localhost/")]).contains("SKRONGLE_SMS_X_URL"));
    assert!(err(&[("SMS_X_URL", "https://h/"), ("SMS_X_FORMAT", "xml")]).contains("SKRONGLE_SMS_X_FORMAT"));
    assert!(err(&[("SMS_X_URL", "https://h/"), ("SMS_X_BODY", "{\"to\":\"{to}\"}")]).contains("has no {text}"));
    assert!(err(&[("SMS_X_URL", "https://h/"), ("SMS_X_BODY", "{\"to\":{to},\"t\":\"{text}\"}")]).contains("is not JSON"));
    assert!(err(&[("SMS_X_URL", "https://h/"), ("SMS_X_USER", "u")]).contains("SKRONGLE_SMS_X_PASS"));
    assert!(err(&[("SMS_X_URL", "https://h/"), ("SMS_X_AUTH", "a"), ("SMS_X_USER", "u"), ("SMS_X_PASS", "p")]).contains("not both"));
    assert!(err(&[("SMS_X_URL", "https://h/"), ("SMS_X_AUTH", "a"), ("SMS_X_AUTH_HEADER", "bad header")]).contains("SKRONGLE_SMS_X_AUTH_HEADER"));
    assert!(config(&[("SMS_PROVIDERS", "x")]).err().unwrap().contains("SKRONGLE_SMS_X_KIND"));
    assert!(config(&[("SMS_PROVIDERS", "log,log")]).err().unwrap().contains("twice"));
    assert!(config(&[("SMS_PROVIDERS", "h,log"), ("SMS_H_KIND", "http"), ("SMS_H_URL", "https://h/")]).err().unwrap().contains("log cannot be in a chain"));
}

/// A one-request HTTP server on a free port: answers `status` and hands
/// back the request head and body it received.
fn fake_http(status: u16) -> (String, std::thread::JoinHandle<(String, String)>) {
    fake_http_saying(status, "{\"message\":\"+447700900123 is not reachable\"}")
}

fn fake_http_saying(status: u16, reply: &'static str) -> (String, std::thread::JoinHandle<(String, String)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/send", listener.local_addr().unwrap().port());
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut out = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        let (mut head, mut len) = (String::new(), 0usize);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
            head.push_str(&line);
        }
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).unwrap();
        write!(out, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
        (head, String::from_utf8(body).unwrap())
    });
    (url, handle)
}

fn http_provider(url: &str, format: BodyFormat) -> HttpProvider {
    HttpProvider::new("hook", url, "+15005550006", format, format.default_template(), vec![("Authorization".into(), basic_auth("AC1", "secret"))])
}

#[test]
fn the_http_provider_posts_the_filled_template_with_its_auth_header() {
    let (url, server) = fake_http(201);
    http_provider(&url, BodyFormat::Form).send(&outgoing()).unwrap();
    let (head, body) = server.join().unwrap();
    assert!(head.starts_with("POST /send "), "{head}");
    let head = head.to_ascii_lowercase();
    let auth = format!("authorization: {}", basic_auth("AC1", "secret").to_ascii_lowercase());
    for want in [auth.as_str(), "content-type: application/x-www-form-urlencoded", "idempotency-key: sms-7"] {
        assert!(head.contains(want), "{want:?} not in\n{head}");
    }
    assert_eq!(body, "To=%2B447700900123&From=%2B15005550006&Body=Your%20code%20is%20123456");

    let (url, server) = fake_http(200);
    http_provider(&url, BodyFormat::Json).send(&outgoing()).unwrap();
    let (head, body) = server.join().unwrap();
    assert!(head.to_ascii_lowercase().contains("content-type: application/json"));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!((v["to"].as_str(), v["text"].as_str(), v["from"].as_str()), (Some("+447700900123"), Some("Your code is 123456"), Some("+15005550006")));
}

#[test]
fn http_4xx_is_a_rejection_and_5xx_429_or_no_server_is_unavailable() {
    let (url, _s) = fake_http(400);
    assert!(matches!(http_provider(&url, BodyFormat::Json).send(&outgoing()), Err(SendError::Rejected(m)) if m.starts_with("HTTP 400")));
    for status in [500, 503, 429] {
        let (url, _s) = fake_http(status);
        assert!(matches!(http_provider(&url, BodyFormat::Json).send(&outgoing()), Err(SendError::Unavailable(_))), "{status}");
    }
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{closed}/send");
    assert!(matches!(http_provider(&url, BodyFormat::Json).send(&outgoing()), Err(SendError::Unavailable(_))));
}

/// #969: Twilio's 21610 (the number replied STOP) is an opt-out, which
/// `_OPT_OUT` can name for another vendor.
#[test]
fn an_unsubscribed_recipient_is_an_opt_out() {
    const TWILIO: &str = "{\"code\": 21610, \"message\": \"Attempt to send to unsubscribed recipient\"}";
    let (url, _s) = fake_http_saying(400, TWILIO);
    let e = http_provider(&url, BodyFormat::Form).send(&outgoing()).unwrap_err();
    assert!(is_opted_out(&e), "{e}");
    let (url, _s) = fake_http(400);
    assert!(!is_opted_out(&http_provider(&url, BodyFormat::Form).send(&outgoing()).unwrap_err()));
    let (url, _s) = fake_http_saying(422, "{\"error\":\"recipient_opted_out\"}");
    let mut p = http_provider(&url, BodyFormat::Json);
    p.opt_out = vec!["recipient_opted_out".into()];
    assert!(is_opted_out(&p.send(&outgoing()).unwrap_err()));
    let env: HashMap<&str, &str> = [("SMS_PROVIDERS", "hook"), ("SMS_HOOK_KIND", "http"), ("SMS_HOOK_URL", "https://example.org/send"), ("SMS_HOOK_OPT_OUT", " opted_out , 21610 ")].into();
    assert!(Config::from_lookup(&|k| env.get(k).map(|v| v.to_string())).is_ok());
}

/// #969: a text refused as opted out marks the account's number, which
/// a text that goes through again (after START) clears.
#[test]
fn an_opt_out_marks_the_number_until_a_text_goes_through() {
    let db = Db::open_in_memory().unwrap();
    let uid = user(&db, "ann@example.org");
    db.with(|c| c.execute("UPDATE users SET phone = '+447700900123' WHERE id = ?1", [uid])).unwrap();
    let flag = || db.with(|c| c.query_row("SELECT phone_opted_out_at FROM users WHERE id = ?1", [uid], |r| r.get::<_, Option<i64>>(0))).unwrap();
    let id = queue(&db, "+447700900123", Some(uid), 100).unwrap();
    let (p, _) = scripted("a", vec![Err(opted_out("HTTP 400: 21610"))]);
    let chain = Chain::new(vec![p]);
    deliver_due(&db, &chain, false, 100).unwrap();
    assert_eq!(row(&db, id).0, "failed", "an opt-out is not retried");
    assert_eq!(flag(), Some(100));
    queue(&db, "+447700900123", Some(uid), 200).unwrap();
    deliver_due(&db, &chain, false, 200).unwrap();
    assert_eq!(flag(), None);
}

#[test]
fn a_failing_http_provider_falls_back_to_the_next() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, "+447700900123", None, 100).unwrap();
    let (bad, _s1) = fake_http(503);
    let (good, s2) = fake_http(200);
    let chain = Chain::new(vec![Box::new(http_provider(&bad, BodyFormat::Json)), Box::new(HttpProvider::new("backup", &good, "", BodyFormat::Json, BodyFormat::Json.default_template(), vec![]))]);
    deliver_due(&db, &chain, false, 100).unwrap();
    assert_eq!(row(&db, id).0, "sent");
    assert_eq!(row(&db, id).2, "backup");
    assert!(s2.join().unwrap().1.contains("Your code is 123456"));
}

#[test]
fn errors_are_logged_without_numbers() {
    assert_eq!(scrub("HTTP 400: +44 7700 is short, +447700900123 is not mobile (code 21211)"), "HTTP 400: +44 7700 is short, <number> is not mobile (code 21211)");
    assert_eq!(masked("+447700900123"), "…23");
}

/// #611, #612: the purge and the start-up sweep read through indexes,
/// never the whole outbox.
#[test]
fn outbox_queries_use_their_indexes() {
    use crate::db::tests::assert_plan_uses;
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        assert_plan_uses(c, PURGE_OLD, "sms_outbox", "idx_sms_outbox_created");
        assert_plan_uses(c, BLANK_SETTLED, "sms_outbox", "idx_sms_outbox_unblanked");
        Ok(())
    })
    .unwrap();
}

/// #873: a text being sent is leased: no second sender gets it, and once the
/// lease runs out (its server stopped mid-send) it is sent once more.
#[test]
fn a_leased_text_is_not_handed_out_twice_and_a_dead_lease_is_taken_again() {
    let db = Db::open_in_memory().unwrap();
    let id = queue(&db, "+447700900123", None, 100).unwrap();
    let lease = |w: &str, now: i64| db.with(|c| crate::jobs::lease_outbox(c, "sms_outbox", "id", w, now, |r| r.get::<_, i64>(0))).unwrap();
    assert_eq!(lease("a", 100), Some(id));
    assert_eq!(lease("b", 100), None);
    assert_eq!(db.with(|c| next_due(c)).unwrap(), Some(100 + crate::jobs::LEASE));
    let (p, calls) = scripted("p", vec![]);
    let chain = Chain::new(vec![p]);
    assert_eq!(deliver_due(&db, &chain, false, 100 + crate::jobs::LEASE - 1).unwrap(), 0);
    assert_eq!(deliver_due(&db, &chain, false, 100 + crate::jobs::LEASE).unwrap(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(row(&db, id).0, "sent");
    let rows = db.with(|c| recent(c, 1)).unwrap();
    assert_eq!((rows[0]["locked_by"].as_str(), rows[0]["locked_until"].as_i64()), (None, None));
}
