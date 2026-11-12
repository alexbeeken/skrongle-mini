//! The OAuth rules over a database (#169): redirect addresses, registration
//! and its bounds, PKCE, the authorization request's checks, codes that
//! work once, refresh rotation, revocation and the purge. The HTTP side is
//! tested in `routes/oauth/tests.rs`.

use super::*;
use crate::db::Db;

const NOW: i64 = 1_000_000;
const ISSUER: &str = "https://learn.example.org/skrongle";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CB: &str = "https://app.example/cb";

fn client(redirects: &[&str]) -> Client {
    Client { client_id: "c1".into(), name: "App".into(), redirect_uris: redirects.iter().map(|s| s.to_string()).collect() }
}

/// A request that passes every check, as pairs; `edit` changes it.
fn params(edit: impl FnOnce(&mut AuthorizeParams)) -> AuthorizeParams {
    let mut p = AuthorizeParams {
        response_type: Some("code".into()),
        client_id: Some("c1".into()),
        redirect_uri: Some(CB.into()),
        code_challenge: Some(s256(VERIFIER)),
        code_challenge_method: Some("S256".into()),
        state: Some("xyz".into()),
        resource: Some(format!("{ISSUER}/mcp")),
        scope: None,
    };
    edit(&mut p);
    p
}

fn what(p: AuthorizeParams) -> &'static str {
    check_authorize(&p, Some(client(&[CB])), ISSUER).err().map_or("ok", |e| e.what)
}

#[test]
fn redirect_addresses_are_https_or_loopback_only() {
    for ok in ["https://app.example/cb", "https://app.example:8443/cb?x=1", "http://localhost/cb", "http://127.0.0.1:5000/cb", "http://[::1]:7000/cb"] {
        assert!(valid_redirect_uri(ok), "{ok}");
    }
    for bad in [
        "http://evil.example/cb",
        "http://192.168.1.10/cb",
        "javascript:alert(1)",
        "data:text/html,hi",
        "com.example.app:/cb",
        "myapp://cb",
        "https://app.example/cb#frag",
        "https://user@app.example/cb",
        "https://app.example/c b",
        "https:///cb",
        "https://app.example:/cb",
        "https://app.exa\u{0}mple/cb",
        "HTTPS://app.example/cb",
        "ftp://app.example/cb",
        "http://localhost.evil.example/cb",
        "",
    ] {
        assert!(!valid_redirect_uri(bad), "{bad:?}");
    }
    assert!(!valid_redirect_uri(&format!("https://app.example/{}", "a".repeat(MAX_URI_LEN))), "too long");
}

/// Exact match; on loopback `http://` only the port may differ.
#[test]
fn redirect_addresses_match_exactly_but_for_a_loopback_port() {
    assert!(redirect_matches(CB, CB));
    assert!(!redirect_matches(CB, "https://app.example/cb/"));
    assert!(!redirect_matches(CB, "https://app.example/cb?x=1"));
    assert!(!redirect_matches(CB, "https://app.example:443/cb"));
    assert!(!redirect_matches(CB, "https://APP.example/cb"));
    assert!(!redirect_matches(CB, "https://app.example.evil.example/cb"));
    assert!(!redirect_matches("https://app.example:8443/cb", "https://app.example:9443/cb"), "only loopback ports are free");
    assert!(redirect_matches("http://127.0.0.1/cb", "http://127.0.0.1:53111/cb"));
    assert!(redirect_matches("http://localhost:3000/cb", "http://localhost:4000/cb"));
    assert!(redirect_matches("http://[::1]:3000/cb", "http://[::1]:4000/cb"));
    assert!(!redirect_matches("http://127.0.0.1/cb", "http://127.0.0.1:53111/other"), "the path is not free");
    assert!(!redirect_matches("http://127.0.0.1/cb", "http://localhost:53111/cb"), "nor the host");
    assert!(!redirect_matches("http://127.0.0.1/cb", "https://127.0.0.1/cb"), "nor the scheme");
    assert_eq!(redirect_host("https://app.example:8443/cb"), "app.example");
    assert_eq!(redirect_host("http://[::1]:4000/cb"), "::1");
}

#[test]
fn registration_is_public_clients_with_few_short_addresses() {
    let reg = |name: &str, uris: &[&str]| Registration { client_name: Some(name.into()), redirect_uris: uris.iter().map(|s| s.to_string()).collect(), ..Default::default() };
    assert_eq!(check_registration(&reg("App", &[CB])), Ok(("App".into(), vec![CB.to_string()])));
    assert_eq!(check_registration(&reg("", &[CB])).map(|r| r.0), Ok("app.example".into()), "no name: the host");
    assert_eq!(check_registration(&reg("App", &[])), Err("invalid_redirect_uri"));
    assert_eq!(check_registration(&reg("App", &["http://evil.example/cb"])), Err("invalid_redirect_uri"));
    let six: Vec<String> = (0..6).map(|i| format!("https://app.example/cb{i}")).collect();
    let six: Vec<&str> = six.iter().map(String::as_str).collect();
    assert_eq!(check_registration(&reg("App", &six)), Err("invalid_redirect_uri"), "at most five");
    assert!(check_registration(&reg("App", &six[..5])).is_ok());
    assert_eq!(check_registration(&reg(&"x".repeat(61), &[CB])), Err("invalid_client_metadata"), "60 characters");
    assert!(check_registration(&reg(&"é".repeat(60), &[CB])).is_ok(), "characters, not bytes");
    for name in ["<b>App</b>", "App\u{7}", "App\nX", "App \"x\""] {
        assert_eq!(check_registration(&reg(name, &[CB])), Err("invalid_client_metadata"), "{name:?}");
    }
    let secret = Registration { token_endpoint_auth_method: Some("client_secret_basic".into()), ..reg("App", &[CB]) };
    assert_eq!(check_registration(&secret), Err("invalid_client_metadata"), "public clients only");
    let implicit = Registration { response_types: Some(vec!["token".into()]), ..reg("App", &[CB]) };
    assert_eq!(check_registration(&implicit), Err("invalid_client_metadata"));
    let cc = Registration { grant_types: Some(vec!["client_credentials".into()]), ..reg("App", &[CB]) };
    assert_eq!(check_registration(&cc), Err("invalid_client_metadata"));
}

/// RFC 7636 Appendix B, and the shapes a verifier and a challenge may have.
#[test]
fn pkce_is_s256() {
    assert_eq!(s256(VERIFIER), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    assert!(pkce_ok("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM", VERIFIER));
    assert!(!pkce_ok("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM", "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXX"));
    assert!(!pkce_ok(VERIFIER, VERIFIER), "plain: the verifier as its own challenge");
    assert!(!valid_verifier("short"));
    assert!(!valid_verifier(&"a".repeat(129)));
    assert!(!valid_challenge("plain-challenge"));
    assert_eq!(base64url(b""), "");
    assert_eq!(base64url(b"f"), "Zg");
    assert_eq!(base64url(b"fo"), "Zm8");
    assert_eq!(base64url(b"foo"), "Zm9v");
    assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
}

/// Every check of the request: each one, broken alone, refuses it.
#[test]
fn authorization_requests_are_checked_in_full() {
    assert_eq!(what(params(|_| {})), "ok");
    assert_eq!(what(params(|p| p.resource = Some(ISSUER.into()))), "ok", "the site itself");
    assert_eq!(what(params(|p| p.resource = Some(format!("{ISSUER}/mcp/")))), "ok", "a trailing slash");
    assert_eq!(check_authorize(&params(|_| {}), None, ISSUER).unwrap_err(), AuthorizeError { error: "unauthorized_client", what: "client" });
    assert_eq!(what(params(|p| p.redirect_uri = None)), "redirect_uri");
    assert_eq!(what(params(|p| p.redirect_uri = Some("https://evil.example/cb".into()))), "redirect_uri");
    assert_eq!(what(params(|p| p.redirect_uri = Some("javascript:alert(1)".into()))), "redirect_uri");
    assert_eq!(what(params(|p| p.response_type = Some("token".into()))), "response_type");
    assert_eq!(what(params(|p| p.code_challenge = None)), "code_challenge", "PKCE or nothing");
    assert_eq!(what(params(|p| p.code_challenge_method = Some("plain".into()))), "code_challenge_method", "plain is refused");
    assert_eq!(what(params(|p| p.code_challenge_method = None)), "code_challenge_method", "S256 must be named");
    assert_eq!(what(params(|p| p.state = None)), "state", "state is required");
    assert_eq!(what(params(|p| p.state = Some("s".repeat(MAX_STATE_LEN + 1)))), "state");
    assert_eq!(what(params(|p| p.resource = None)), "resource", "resource is required");
    assert_eq!(what(params(|p| p.resource = Some("https://other.example/mcp".into()))), "resource");
    assert_eq!(what(params(|p| p.resource = Some("https://learn.example.org/mcp".into()))), "resource", "the base path counts");
    assert_eq!(what(params(|p| p.scope = Some("courses:read courses:delete".into()))), "scope");
    let a = check_authorize(&params(|p| p.scope = Some("courses:read".into())), Some(client(&[CB])), ISSUER).unwrap();
    assert_eq!(a.scopes, vec![Scope::Read]);
    assert_eq!(a.state, "xyz");
    let dup = vec![("state".to_string(), "a".to_string()), ("state".to_string(), "b".to_string())];
    assert_eq!(AuthorizeParams::from_pairs(&dup).unwrap_err().what, "duplicate");
}

/// #170's redirect tricks: user names, a trailing dot, case, a default port
/// written out, look-alike loopback names, IPv6 spellings of loopback,
/// percent-encoding, a backslash, a fragment, white space and length. None
/// registers as something it is not, and none matches a registered address
/// it only resembles.
#[test]
fn redirect_tricks_do_not_pass() {
    for bad in [
        "https://good.example@evil.example/cb",
        "https://good.example%40evil.example/cb",
        "https://good.example\\@evil.example/cb",
        "https://evil.example\\.good.example/cb",
        "http://localhost.evil.example/cb",
        "http://127.0.0.1.evil.example/cb",
        "http://127.0.0.1.nip.io/cb",
        "http://LOCALHOST/cb",
        "http://[::ffff:127.0.0.1]/cb",
        "http://[0:0:0:0:0:0:0:1]/cb",
        "http://[::1%25lo0]/cb",
        "http://2130706433/cb",
        "http://0x7f000001/cb",
        "http://127.1/cb",
        "https://app.example/cb#x",
        "https://app.example/cb#",
        "https://app.example/c\tb",
        "https://app.example/cb\n",
        " https://app.example/cb",
        "https://app.example:-1/cb",
        "https:app.example/cb",
        "https:/app.example/cb",
    ] {
        assert!(!valid_redirect_uri(bad), "{bad:?} registers");
        assert!(!redirect_matches(CB, bad), "{bad:?} matches");
    }
    for (registered, asked) in [
        (CB, "https://app.example./cb"),
        (CB, "https://App.Example/cb"),
        (CB, "https://app.example:443/cb"),
        (CB, "https://app.example/%63b"),
        (CB, "https://app.example/cb/."),
        (CB, "https://app.example//cb"),
        ("https://app.example:8443/cb", "https://app.example/cb"),
        ("http://127.0.0.1/cb", "http://127.0.0.1:80@evil.example/cb"),
        ("http://127.0.0.1/cb", "http://127.0.0.1:5000/cb/../x"),
        ("http://127.0.0.1/cb", "http://127.0.0.1:5000/cb?x=1"),
        ("http://localhost/cb", "http://localhost.:5000/cb"),
    ] {
        assert!(!redirect_matches(registered, asked), "{asked:?} matched {registered:?}");
    }
    let long = format!("https://app.example/{}", "a".repeat(10_000));
    assert!(!valid_redirect_uri(&long) && !redirect_matches(&long, &long), "a very long address");
    // What is left is written exactly as registered, so a trailing dot or
    // another case only matches itself, and the consent page names that host.
    assert!(valid_redirect_uri("https://app.example./cb") && redirect_matches("https://app.example./cb", "https://app.example./cb"));
    assert_eq!(redirect_host("https://app.example./cb"), "app.example.");
}

/// #170's `resource` tricks: one trailing slash is the only spelling of the
/// two resources besides their own; a path below `/mcp`, another scheme,
/// port, case or base path, a query or a fragment is another resource.
#[test]
fn resource_tricks_do_not_pass() {
    let mcp = format!("{ISSUER}/mcp");
    assert_eq!(served_resource(&mcp, ISSUER).as_deref(), Some(mcp.as_str()));
    assert_eq!(served_resource(&format!("{mcp}/"), ISSUER).as_deref(), Some(mcp.as_str()));
    assert_eq!(served_resource(&format!("{ISSUER}/"), ISSUER).as_deref(), Some(ISSUER));
    for bad in [
        format!("{mcp}//"),
        format!("{mcp}/x"),
        format!("{mcp}/../mcp"),
        format!("{ISSUER}//mcp"),
        format!("{ISSUER}/MCP"),
        format!("{mcp}?x=1"),
        format!("{mcp}#x"),
        "http://learn.example.org/skrongle/mcp".to_string(),
        "https://learn.example.org:443/skrongle/mcp".to_string(),
        "https://learn.example.org:8443/skrongle/mcp".to_string(),
        "https://LEARN.example.org/skrongle/mcp".to_string(),
        "https://learn.example.org/mcp".to_string(),
        "https://learn.example.org/skrongle2/mcp".to_string(),
        "https://learn.example.org.evil.example/skrongle/mcp".to_string(),
        "https://evil.example/skrongle/mcp".to_string(),
        String::new(),
    ] {
        assert_eq!(served_resource(&bad, ISSUER), None, "{bad:?}");
        assert_eq!(what(params(|p| p.resource = Some(bad.clone()))), "resource", "{bad:?}");
    }
}

/// Ann, apps on, and a registered client; the request it would make.
fn setup(c: &mut Connection) -> (i64, Authorize) {
    crate::settings::set_apps_on(c, true).unwrap();
    let ann = crate::perms::create_teacher(c, "ann@example.org", "h").unwrap();
    let id = register(c, "App", &[CB.to_string()], NOW).unwrap().unwrap();
    let cl = super::client(c, &id, NOW).unwrap().unwrap();
    let mut p = params(|_| {});
    p.client_id = Some(id);
    (ann, check_authorize(&p, Some(cl), ISSUER).unwrap())
}

fn consent(user_id: i64) -> Consent<'static> {
    Consent { user_id, courses: &[], lifetime: Lifetime::Days90 }
}

fn exchange<'a>(a: &'a Authorize, code: &'a str) -> CodeExchange<'a> {
    CodeExchange { code, client_id: &a.client.client_id, redirect_uri: &a.redirect_uri, code_verifier: VERIFIER, resource: &a.resource }
}

fn grants(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM api_grants", [], |r| r.get(0)).unwrap()
}

fn works(c: &Connection, token: &str) -> bool {
    matches!(api::user_for_token(c, token, NOW + 1).unwrap(), api::Lookup::Found(_))
}

/// A code is stored hashed, works once with the right verifier, client,
/// redirect address and resource, and gives a working access token.
#[test]
fn a_code_is_bound_to_its_request() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let stored: String = c.query_row("SELECT code_hash FROM oauth_codes", [], |r| r.get(0)).unwrap();
        assert_eq!(stored, hash_token(&code), "only the hash");
        let last: Option<i64> = c.query_row("SELECT last_grant_at FROM oauth_clients", [], |r| r.get(0)).unwrap();
        assert_eq!(last, Some(NOW));
        let try_with = |c: &mut Connection, edit: &dyn Fn(&mut CodeExchange)| {
            let mut x = exchange(&a, &code);
            edit(&mut x);
            exchange_code(c, &x, NOW + 1).unwrap().err()
        };
        assert_eq!(try_with(c, &|x| x.code_verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXX"), Some(TokenError::InvalidGrant), "wrong verifier");
        assert_eq!(try_with(c, &|x| x.client_id = "other"), Some(TokenError::InvalidGrant), "another client");
        assert_eq!(try_with(c, &|x| x.redirect_uri = "https://app.example/other"), Some(TokenError::InvalidGrant), "another redirect");
        assert_eq!(try_with(c, &|x| x.resource = ISSUER), Some(TokenError::InvalidTarget), "another resource");
        assert_eq!(grants(c), 1, "refusals revoke nothing");
        let t = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        assert_eq!(t.expires_in, ACCESS_TTL_SECS);
        assert!(works(c, &t.access));
        assert!(!works(c, &t.refresh), "a refresh token opens no API route");
        Ok(())
    })
    .unwrap();
}

/// A second use of a code deletes the grant it made and every token under
/// it; the tokens of the first use stop working.
#[test]
fn a_code_used_twice_revokes_its_grant() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let first = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        assert!(works(c, &first.access));
        assert_eq!(exchange_code(c, &exchange(&a, &code), NOW + 2).unwrap().unwrap_err(), TokenError::InvalidGrant);
        assert_eq!(grants(c), 0, "the grant is gone");
        assert!(!works(c, &first.access), "and its tokens");
        let tokens: i64 = c.query_row("SELECT COUNT(*) FROM api_tokens", [], |r| r.get(0))?;
        assert_eq!(tokens, 0);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_code_lasts_ten_minutes() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        assert_eq!(exchange_code(c, &exchange(&a, &code), NOW + CODE_TTL_SECS).unwrap().unwrap_err(), TokenError::InvalidGrant);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        assert!(exchange_code(c, &exchange(&a, &code), NOW + CODE_TTL_SECS - 1).unwrap().is_ok());
        Ok(())
    })
    .unwrap();
}

fn token_rows(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM api_tokens", [], |r| r.get(0)).unwrap()
}

/// Refreshing marks the refresh token used and hands out a new pair.
/// Another client cannot use it, and its attempt does not use it up. An
/// access token refreshes nothing.
#[test]
fn refresh_tokens_rotate() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let first = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        assert_eq!(refresh(c, &first.refresh, "other-client", NOW + 2).unwrap().unwrap_err(), TokenError::InvalidGrant);
        let second = refresh(c, &first.refresh, &a.client.client_id, NOW + 2).unwrap().unwrap();
        assert_ne!(second.refresh, first.refresh);
        assert!(works(c, &second.access));
        assert_eq!(refresh(c, &second.access, &a.client.client_id, NOW + 3).unwrap().unwrap_err(), TokenError::InvalidGrant, "an access token refreshes nothing");
        let used: Option<i64> = c.query_row("SELECT used_at FROM api_tokens WHERE token_hash = ?1", [hash_token(&first.refresh)], |r| r.get(0))?;
        assert_eq!(used, Some(NOW + 2), "kept, marked used");
        // Refreshing in a loop keeps at most two access tokens, one refresh
        // token and one used refresh token.
        let mut last = second;
        for i in 0..10 {
            last = refresh(c, &last.refresh, &a.client.client_id, NOW + 4 + i).unwrap().unwrap();
        }
        assert!(works(c, &last.access));
        assert_eq!(token_rows(c), 4);
        let used: i64 = c.query_row("SELECT COUNT(*) FROM api_tokens WHERE used_at IS NOT NULL", [], |r| r.get(0))?;
        assert_eq!(used, 1, "only the newest used refresh token");
        Ok(())
    })
    .unwrap();
}

/// A used refresh token presented again (RFC 9700 §4.14.2, #184) revokes
/// the grant and every token under it: the newest access and refresh
/// tokens stop working, and the account's list no longer has the app.
#[test]
fn a_reused_refresh_token_revokes_its_grant() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let first = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        let second = refresh(c, &first.refresh, &a.client.client_id, NOW + 2).unwrap().unwrap();
        assert_eq!(api::grants(c, ann, NOW + 3)?.len(), 1);
        assert_eq!(refresh(c, &first.refresh, &a.client.client_id, NOW + 3).unwrap().unwrap_err(), TokenError::InvalidGrant);
        assert_eq!(grants(c), 0, "the grant is gone");
        assert!(api::grants(c, ann, NOW + 3)?.is_empty(), "Connected apps no longer lists it");
        assert!(!works(c, &second.access), "the newest access token stops working");
        assert_eq!(refresh(c, &second.refresh, &a.client.client_id, NOW + 4).unwrap().unwrap_err(), TokenError::InvalidGrant, "and the newest refresh token");
        assert_eq!(token_rows(c), 0);
        Ok(())
    })
    .unwrap();
}

/// Reuse is told by the token alone, but the client check comes first:
/// another client presenting a used refresh token gets `invalid_grant` and
/// revokes nothing; the client the grant was made for still refreshes with
/// its newest token.
#[test]
fn a_used_refresh_token_from_another_client_revokes_nothing() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let first = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        let second = refresh(c, &first.refresh, &a.client.client_id, NOW + 2).unwrap().unwrap();
        assert_eq!(refresh(c, &first.refresh, "other-client", NOW + 3).unwrap().unwrap_err(), TokenError::InvalidGrant);
        assert_eq!(grants(c), 1);
        assert!(works(c, &second.access));
        assert!(refresh(c, &second.refresh, &a.client.client_id, NOW + 4).unwrap().is_ok());
        Ok(())
    })
    .unwrap();
}

/// A used refresh token goes with its grant: when the grant expires, the
/// purge deletes it with every other token; revoked, the cascade does.
#[test]
fn used_refresh_tokens_go_with_their_grant() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let first = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        refresh(c, &first.refresh, &a.client.client_id, NOW + 2).unwrap().unwrap();
        let used = |c: &Connection| c.query_row("SELECT COUNT(*) FROM api_tokens WHERE used_at IS NOT NULL", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(used(c), 1);
        api::purge_expired(c, NOW + 3)?;
        assert_eq!(used(c), 1, "kept while the grant lasts");
        let expires: i64 = c.query_row("SELECT expires_at FROM api_grants", [], |r| r.get(0))?;
        api::purge_expired(c, expires)?;
        assert_eq!((grants(c), token_rows(c)), (0, 0), "gone with the grant");
        // Revoked on the profile: the used row goes too.
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let t = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        refresh(c, &t.refresh, &a.client.client_id, NOW + 2).unwrap().unwrap();
        let id = api::grants(c, ann, NOW + 3)?[0].id;
        assert!(api::revoke(c, ann, id)?);
        assert_eq!(token_rows(c), 0);
        Ok(())
    })
    .unwrap();
}

/// An account that lost Create courses, or has a temporary password
/// waiting, can neither trade a code nor refresh.
#[test]
fn the_account_must_still_be_allowed() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let t = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        crate::perms::set_custom(c, ann, crate::perms::Perms::STUDENT)?;
        assert_eq!(refresh(c, &t.refresh, &a.client.client_id, NOW + 2).unwrap().unwrap_err(), TokenError::InvalidGrant);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        assert_eq!(exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap_err(), TokenError::InvalidGrant);
        Ok(())
    })
    .unwrap();
}

/// Each Allow is one grant against the account's cap of four.
#[test]
fn allow_counts_against_the_grant_cap() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        for _ in 0..crate::caps::MAX_GRANTS_PER_ACCOUNT {
            allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        }
        assert_eq!(allow(c, &a, &consent(ann), NOW).unwrap().unwrap_err(), Refused::TooManyGrants);
        let codes: i64 = c.query_row("SELECT COUNT(*) FROM oauth_codes", [], |r| r.get(0))?;
        assert_eq!(codes, crate::caps::MAX_GRANTS_PER_ACCOUNT as i64, "a refused Allow leaves no code");
        Ok(())
    })
    .unwrap();
}

/// Revoke deletes the grant behind the client's own token only.
#[test]
fn revoke_is_for_the_clients_own_tokens() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let t = exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        let personal = api::make_personal(
            c,
            ann,
            &NewGrant { client_id: api::PERSONAL_CLIENT_ID, client_name: "Mine", scopes: &Scope::ALL, courses: &[], lifetime: Lifetime::Days30 },
            NOW,
        )?
        .unwrap();
        assert!(!revoke(c, &t.refresh, "other-client")?);
        assert!(!revoke(c, &personal, &a.client.client_id)?, "a personal token is not the app's");
        assert!(!revoke(c, "skr_unknown", &a.client.client_id)?);
        assert!(revoke(c, &t.refresh, &a.client.client_id)?);
        assert!(!works(c, &t.access));
        assert!(works(c, &personal));
        Ok(())
    })
    .unwrap();
}

/// The registry holds at most `MAX_OAUTH_CLIENTS` rows: past it the oldest
/// row with no grant goes; full of rows with grants, a registration is
/// refused. Unused rows are purged after a day; one with a grant stays.
#[test]
fn the_registry_is_bounded() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        for i in 0..MAX_OAUTH_CLIENTS {
            register(c, "App", &[CB.to_string()], NOW + 1 + i as i64).unwrap().unwrap();
        }
        let rows = |c: &Connection| c.query_row("SELECT COUNT(*) FROM oauth_clients", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(rows(c), MAX_OAUTH_CLIENTS as i64);
        assert!(super::client(c, &a.client.client_id, NOW + 200)?.is_some(), "the one with a grant was not evicted");
        // Every row with a grant: refused.
        let all: Vec<String> = c.prepare("SELECT client_id FROM oauth_clients")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for id in &all {
            c.execute("INSERT INTO api_grants (user_id, client_id, client_name, scopes, created_at, expires_at) VALUES (?1, ?2, 'x', 'courses:read', ?3, ?4)", params![ann, id, NOW, NOW + 1_000_000])?;
        }
        assert_eq!(register(c, "App", &[CB.to_string()], NOW + 300).unwrap().unwrap_err(), Refused::Full);
        c.execute("DELETE FROM api_grants WHERE client_id <> ?1", [&a.client.client_id])?;
        // A day on: the rows nobody finished a grant for are gone.
        purge(c, NOW + 400 + UNUSED_CLIENT_TTL_SECS)?;
        assert_eq!(rows(c), 1);
        assert!(super::client(c, &a.client.client_id, NOW + 400 + UNUSED_CLIENT_TTL_SECS)?.is_some());
        Ok(())
    })
    .unwrap();
}

/// The purge drops expired codes and the grants an Allow made whose code
/// was never traded.
#[test]
fn purge_drops_expired_codes_and_untraded_grants() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, a) = setup(c);
        allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        let code = allow(c, &a, &consent(ann), NOW).unwrap().unwrap();
        exchange_code(c, &exchange(&a, &code), NOW + 1).unwrap().unwrap();
        assert_eq!(grants(c), 2);
        api::purge_expired(c, NOW + CODE_TTL_SECS)?;
        let codes: i64 = c.query_row("SELECT COUNT(*) FROM oauth_codes", [], |r| r.get(0))?;
        assert_eq!(codes, 0);
        assert_eq!(grants(c), 1, "the traded one stays");
        Ok(())
    })
    .unwrap();
}
