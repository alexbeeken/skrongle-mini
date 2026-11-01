//! "Continue with Google": OpenID Connect authorization-code flow with PKCE.
//!
//! 1. `GET /auth/google` builds the Google authorization URL with a random
//!    `state`, a PKCE verifier and a `nonce`, keeps the three in a short-lived
//!    HttpOnly cookie, and redirects.
//! 2. Google sends the browser back to `/auth/google/callback?code=…&state=…`.
//!    The handler checks the state against the cookie, exchanges the code for
//!    tokens at Google's token endpoint (server to server, over TLS) and reads
//!    the identity from the returned ID token.
//! 3. The `sub` claim is Google's stable account id: an existing user linked to
//!    it is logged in, a logged-in user is linked to it, anyone else gets a new
//!    account named after their e-mail address.
//!
//! The ID token's signature is not verified: it comes straight from Google's
//! token endpoint over TLS in exchange for a code only this server knows,
//! which is what the OpenID Connect spec allows (§3.1.3.7, step 6). Issuer,
//! audience, expiry, nonce and `email_verified` are checked.

use sha2::{Digest, Sha256};
use std::time::Duration;

pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// How long the browser has to come back from Google.
pub const PENDING_TTL_SECS: i64 = 600;

#[derive(Clone)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: String,
    /// Must match a redirect URI registered for the OAuth client, exactly.
    pub redirect_uri: String,
    /// Where the browser is sent to sign in (`AUTH_URL`, or an override).
    pub auth_url: String,
    /// Where the code is exchanged, server to server (`TOKEN_URL`, or an override).
    pub token_url: String,
    /// Dev mode's stand-in Google (`crate::dev::mock_google`): the two
    /// endpoints are this server's own `/dev/google/*` routes.
    pub mock: bool,
}

impl GoogleConfig {
    /// A real Google client. `SKRONGLE_GOOGLE_AUTH_URL` / `SKRONGLE_GOOGLE_TOKEN_URL`
    /// point the endpoints at a stand-in server for local testing; never set
    /// them in production.
    pub fn new(client_id: String, client_secret: String, redirect_uri: String) -> GoogleConfig {
        GoogleConfig {
            client_id,
            client_secret,
            redirect_uri,
            auth_url: crate::env("GOOGLE_AUTH_URL").unwrap_or_else(|| AUTH_URL.to_string()),
            token_url: crate::env("GOOGLE_TOKEN_URL").unwrap_or_else(|| TOKEN_URL.to_string()),
            mock: false,
        }
    }
}

/// Never prints the client secret.
impl std::fmt::Debug for GoogleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_uri", &self.redirect_uri)
            .field("auth_url", &self.auth_url)
            .field("token_url", &self.token_url)
            .field("mock", &self.mock)
            .finish()
    }
}

/// What is remembered between the redirect to Google and the callback.
#[derive(Debug, PartialEq, Eq)]
pub struct Pending {
    pub state: String,
    pub verifier: String,
    pub nonce: String,
    /// `true` when a logged-in user is attaching Google to their account.
    pub link: bool,
}

impl Pending {
    pub fn new(link: bool) -> Pending {
        Pending { state: random_token(), verifier: random_token(), nonce: random_token(), link }
    }

    /// Cookie value: four dot-separated fields, all URL-safe.
    pub fn encode(&self) -> String {
        format!("{}.{}.{}.{}", self.state, self.verifier, self.nonce, u8::from(self.link))
    }

    pub fn decode(s: &str) -> Option<Pending> {
        let mut it = s.split('.');
        let (state, verifier, nonce, link) = (it.next()?, it.next()?, it.next()?, it.next()?);
        if it.next().is_some() || [state, verifier, nonce].iter().any(|p| !is_token(p)) {
            return None;
        }
        Some(Pending { state: state.into(), verifier: verifier.into(), nonce: nonce.into(), link: link == "1" })
    }
}

fn is_token(s: &str) -> bool {
    s.len() == 43 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// 32 random bytes, base64url without padding (43 chars): a valid PKCE verifier.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    crate::random::fill(&mut bytes);
    b64url_encode(&bytes)
}

pub fn b64url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |acc, &b| (acc << 8) | b as u32) << (8 * (3 - chunk.len()));
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.as_bytes().chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

/// PKCE S256 challenge for a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    b64url_encode(&Sha256::digest(verifier.as_bytes()))
}

/// Percent-encode a query value (RFC 3986 unreserved characters pass through).
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The Google authorization URL for a pending sign-in.
pub fn auth_url(cfg: &GoogleConfig, p: &Pending) -> String {
    let params = [
        ("client_id", cfg.client_id.as_str()),
        ("redirect_uri", cfg.redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", "openid email profile"),
        ("state", p.state.as_str()),
        ("nonce", p.nonce.as_str()),
        ("code_challenge", &pkce_challenge(&p.verifier)),
        ("code_challenge_method", "S256"),
        ("prompt", "select_account"),
    ];
    let q: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", urlencode(v))).collect();
    format!("{}?{}", cfg.auth_url, q.join("&"))
}

/// Who Google says the user is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub sub: String,
    pub email: String,
    pub name: Option<String>,
}

/// Exchange the authorization code for tokens and return the raw ID token.
/// Blocking (network): call from the blocking pool.
pub fn exchange_code(cfg: &GoogleConfig, code: &str, verifier: &str) -> Result<String, String> {
    // Google answers a refused code with a 400 and a JSON body that says why;
    // read it instead of stopping at the status, so the log names the reason.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent
        .post(&cfg.token_url)
        .send_form([
            ("code", code),
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("redirect_uri", cfg.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier),
        ])
        .map_err(|e| format!("token request failed: {e}"))?;
    let status = resp.status();
    let body = resp.body_mut().read_to_string().map_err(|e| format!("token response unreadable: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("token response is not JSON (status {status}): {e}"))?;
    if let Some(err) = json.get("error").and_then(|e| e.as_str()) {
        let desc = json.get("error_description").and_then(|d| d.as_str()).unwrap_or("");
        return Err(format!("Google refused the code: {err} {desc}").trim_end().to_string());
    }
    json.get("id_token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| "token response has no id_token".to_string())
}

/// Read and check the claims of an ID token obtained from the token endpoint.
pub fn parse_id_token(id_token: &str, client_id: &str, nonce: &str, now: i64) -> Result<Identity, String> {
    let mut parts = id_token.split('.');
    let (_header, payload) = (parts.next().ok_or("malformed id_token")?, parts.next().ok_or("malformed id_token")?);
    let bytes = b64url_decode(payload).ok_or("id_token payload is not base64url")?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "id_token payload is not JSON")?;
    let s = |k: &str| claims.get(k).and_then(|v| v.as_str()).map(str::to_string);

    let iss = s("iss").unwrap_or_default();
    if iss != "https://accounts.google.com" && iss != "accounts.google.com" {
        return Err(format!("unexpected issuer {iss:?}"));
    }
    let aud_ok = match claims.get("aud") {
        Some(serde_json::Value::String(a)) => a == client_id,
        Some(serde_json::Value::Array(list)) => list.iter().any(|a| a.as_str() == Some(client_id)),
        _ => false,
    };
    if !aud_ok {
        return Err("id_token was issued for a different client".into());
    }
    let exp = claims.get("exp").and_then(|e| e.as_i64()).unwrap_or(0);
    if exp <= now {
        return Err("id_token has expired".into());
    }
    if s("nonce").as_deref() != Some(nonce) {
        return Err("nonce mismatch".into());
    }
    if claims.get("email_verified").and_then(|v| v.as_bool()) != Some(true) {
        return Err("Google has not verified this e-mail address".into());
    }
    let sub = s("sub").filter(|v| !v.is_empty()).ok_or("id_token has no subject")?;
    let email = s("email").filter(|v| v.contains('@')).ok_or("id_token has no e-mail address")?;
    Ok(Identity { sub, email: email.to_lowercase(), name: s("name") })
}

/// Google sign-ups waiting for the confirm step (`/auth/google/confirm`):
/// a new account is only created once its owner ticks the Terms checkbox.
/// In memory, keyed by a random token kept in a cookie; an entry lives
/// `PENDING_TTL_SECS` and is used once.
#[derive(Default)]
pub struct Signups(std::sync::Mutex<std::collections::HashMap<String, (Identity, i64)>>);

/// At most this many sign-ups wait at once (oldest dropped first), so the map stays small.
const MAX_SIGNUPS: usize = 256;

impl Signups {
    /// Remember a verified identity; returns the token for the cookie.
    pub fn insert(&self, identity: Identity, now: i64) -> String {
        let token = random_token();
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        m.retain(|_, (_, at)| now - *at < PENDING_TTL_SECS);
        while m.len() >= MAX_SIGNUPS {
            let Some(oldest) = m.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        m.insert(token.clone(), (identity, now));
        token
    }

    /// The identity waiting under `token`, if it has not expired.
    pub fn get(&self, token: &str, now: i64) -> Option<Identity> {
        let m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        m.get(token).filter(|(_, at)| now - *at < PENDING_TTL_SECS).map(|(i, _)| i.clone())
    }

    /// Take the identity out (the account is being created).
    pub fn take(&self, token: &str, now: i64) -> Option<Identity> {
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        m.remove(token).filter(|(_, at)| now - *at < PENDING_TTL_SECS).map(|(i, _)| i)
    }
}

/// Google sign-ins that join an account with two-factor sign-in on (#677):
/// Google is connected only once the code is accepted, so the identity
/// waits here under the sign-in's `twofa` token. In memory, like
/// [`Signups`]; a sign-in abandoned at its code leaves Google unconnected.
#[derive(Default)]
pub struct Joins(std::sync::Mutex<std::collections::HashMap<String, (Identity, i64)>>);

/// How long a join waits for its code: past a code's life and its resends.
/// An entry only counts while its sign-in is still waiting anyway.
const JOIN_TTL_SECS: i64 = 60 * 60;

impl Joins {
    /// Remember the identity for the sign-in waiting under `token`.
    pub fn insert(&self, token: &str, identity: Identity, now: i64) {
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        m.retain(|_, (_, at)| now - *at < JOIN_TTL_SECS);
        while m.len() >= MAX_SIGNUPS {
            let Some(oldest) = m.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        m.insert(token.to_string(), (identity, now));
    }

    /// Take the identity waiting under `token` (the code was accepted, or
    /// the sign-in was given up). Used once.
    pub fn take(&self, token: &str, now: i64) -> Option<Identity> {
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        m.remove(token).filter(|(_, at)| now - *at < JOIN_TTL_SECS).map(|(i, _)| i)
    }
}

/// What a signed-in learner sends the browser to Google for, from the
/// profile (#360, #376): attaching a Google account (after `prove_owner`),
/// or a Google-only account proving it is its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepUp {
    Link,
    Confirm,
}

/// Google trips started from the profile, bound on the server to the
/// account and purpose (the `oauth` cookie alone is no proof: a session
/// holder can write any cookie), and the Google-only accounts that just
/// confirmed with Google. In memory, like [`Signups`]; an entry lives
/// `PENDING_TTL_SECS` and is used once.
#[derive(Default)]
pub struct StepUps {
    /// `state` → (user, purpose, where to go back to, when).
    pending: std::sync::Mutex<std::collections::HashMap<String, (i64, StepUp, String, i64)>>,
    /// user → when Google confirmed it.
    confirmed: std::sync::Mutex<std::collections::HashMap<i64, i64>>,
}

impl StepUps {
    /// Remember a trip to Google under its `state`.
    pub fn start(&self, state: &str, user_id: i64, what: StepUp, back: &str, now: i64) {
        let mut m = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        m.retain(|_, e| now - e.3 < PENDING_TTL_SECS);
        while m.len() >= MAX_SIGNUPS {
            let Some(oldest) = m.iter().min_by_key(|(_, e)| e.3).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        m.insert(state.to_string(), (user_id, what, back.to_string(), now));
    }

    /// The trip coming back under `state`, if `user_id` started it and it
    /// has not expired: its purpose and where to go back to. Used once.
    pub fn finish(&self, state: &str, user_id: i64, now: i64) -> Option<(StepUp, String)> {
        let mut m = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        let (uid, what, back, at) = m.remove(state)?;
        (uid == user_id && now - at < PENDING_TTL_SECS).then_some((what, back))
    }

    /// Google just confirmed the account's owner.
    pub fn confirm(&self, user_id: i64, now: i64) {
        let mut m = self.confirmed.lock().unwrap_or_else(|p| p.into_inner());
        m.retain(|_, at| now - *at < PENDING_TTL_SECS);
        m.insert(user_id, now);
    }

    /// Is a confirmation waiting to be used (the page says so)?
    pub fn is_confirmed(&self, user_id: i64, now: i64) -> bool {
        let m = self.confirmed.lock().unwrap_or_else(|p| p.into_inner());
        m.get(&user_id).is_some_and(|at| now - *at < PENDING_TTL_SECS)
    }

    /// Use the confirmation up: one change per trip to Google.
    pub fn spend(&self, user_id: i64, now: i64) -> bool {
        let mut m = self.confirmed.lock().unwrap_or_else(|p| p.into_inner());
        m.remove(&user_id).is_some_and(|at| now - at < PENDING_TTL_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_ups_are_bound_to_the_account_used_once_and_expire() {
        let s = StepUps::default();
        s.start("st1", 7, StepUp::Link, "/profile", 1000);
        // Another account (a cookie written by hand) gets nothing, and the trip is gone.
        assert_eq!(s.finish("st1", 8, 1001), None);
        assert_eq!(s.finish("st1", 7, 1001), None);
        s.start("st2", 7, StepUp::Confirm, "/profile#data", 1000);
        assert_eq!(s.finish("st2", 7, 1001), Some((StepUp::Confirm, "/profile#data".into())));
        assert_eq!(s.finish("st2", 7, 1002), None);
        s.start("st3", 7, StepUp::Link, "/", 1000);
        assert_eq!(s.finish("st3", 7, 1000 + PENDING_TTL_SECS), None);
        assert_eq!(s.finish("nope", 7, 1000), None);

        assert!(!s.is_confirmed(7, 1000) && !s.spend(7, 1000));
        s.confirm(7, 1000);
        assert!(s.is_confirmed(7, 1001) && !s.is_confirmed(8, 1001));
        assert!(!s.spend(8, 1001));
        assert!(s.spend(7, 1002));
        assert!(!s.spend(7, 1003), "one change per confirmation");
        s.confirm(7, 1000);
        assert!(!s.spend(7, 1000 + PENDING_TTL_SECS), "a confirmation expires");
    }

    #[test]
    fn signups_wait_once_and_expire() {
        let s = Signups::default();
        let id = Identity { sub: "g1".into(), email: "a@b.c".into(), name: None };
        let t = s.insert(id.clone(), 1000);
        assert_eq!(s.get(&t, 1001), Some(id.clone()));
        assert_eq!(s.take(&t, 1002), Some(id.clone()));
        assert_eq!(s.take(&t, 1003), None);
        let t2 = s.insert(id, 1000);
        assert_eq!(s.get(&t2, 1000 + PENDING_TTL_SECS), None);
        assert_eq!(s.get("nope", 1000), None);
    }

    #[test]
    fn base64url_roundtrip_and_known_vectors() {
        assert_eq!(b64url_encode(b""), "");
        assert_eq!(b64url_encode(b"f"), "Zg");
        assert_eq!(b64url_encode(b"fo"), "Zm8");
        assert_eq!(b64url_encode(b"foo"), "Zm9v");
        assert_eq!(b64url_encode(&[0xfb, 0xff]), "-_8");
        for n in 0..40u8 {
            let data: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(b64url_decode(&b64url_encode(&data)).unwrap(), data);
        }
        assert_eq!(b64url_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(b64url_decode("Zm9vYg==").unwrap(), b"foob");
        assert!(b64url_decode("Z").is_none());
        assert!(b64url_decode("Zm9!").is_none());
    }

    #[test]
    fn pkce_matches_rfc_7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn pending_roundtrip() {
        let p = Pending::new(true);
        assert_eq!(p.state.len(), 43);
        assert_ne!(p.state, p.nonce);
        let back = Pending::decode(&p.encode()).unwrap();
        assert_eq!(back, p);
        assert!(Pending::decode("a.b.c.0").is_none());
        assert!(Pending::decode(&format!("{}.{}.{}", p.state, p.verifier, p.nonce)).is_none());
    }

    #[test]
    fn builds_auth_url() {
        let cfg = GoogleConfig::new(
            "id.apps.googleusercontent.com".into(),
            "s".into(),
            "https://example.org/spanish/auth/google/callback".into(),
        );
        assert_eq!(cfg.token_url, TOKEN_URL);
        assert!(!cfg.mock);
        let p = Pending::new(false);
        let url = auth_url(&cfg, &p);
        assert!(url.starts_with(AUTH_URL));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fexample.org%2Fspanish%2Fauth%2Fgoogle%2Fcallback"));
        assert!(url.contains(&format!("state={}", p.state)));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=openid%20email%20profile"));
    }

    fn token(claims: serde_json::Value) -> String {
        let header = b64url_encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = b64url_encode(claims.to_string().as_bytes());
        format!("{header}.{payload}.sig")
    }

    #[test]
    fn checks_id_token_claims() {
        let now = 1_700_000_000;
        let good = serde_json::json!({
            "iss": "https://accounts.google.com", "aud": "cid", "exp": now + 60, "nonce": "n1",
            "sub": "1234567890", "email": "Ana.Lopez@gmail.com", "email_verified": true, "name": "Ana"
        });
        let id = parse_id_token(&token(good.clone()), "cid", "n1", now).unwrap();
        assert_eq!(id, Identity { sub: "1234567890".into(), email: "ana.lopez@gmail.com".into(), name: Some("Ana".into()) });

        let mut bad = good.clone();
        bad["aud"] = "other".into();
        assert!(parse_id_token(&token(bad), "cid", "n1", now).unwrap_err().contains("different client"));
        let mut bad = good.clone();
        bad["exp"] = (now - 1).into();
        assert!(parse_id_token(&token(bad), "cid", "n1", now).unwrap_err().contains("expired"));
        assert!(parse_id_token(&token(good.clone()), "cid", "n2", now).unwrap_err().contains("nonce"));
        let mut bad = good.clone();
        bad["email_verified"] = false.into();
        assert!(parse_id_token(&token(bad), "cid", "n1", now).unwrap_err().contains("verified"));
        let mut bad = good.clone();
        bad["iss"] = "https://evil.example".into();
        assert!(parse_id_token(&token(bad), "cid", "n1", now).unwrap_err().contains("issuer"));
        assert!(parse_id_token("nope", "cid", "n1", now).is_err());
    }
}
