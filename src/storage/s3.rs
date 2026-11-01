//! A small S3 client (#871): PUT, GET, DELETE and a pre-signed GET link,
//! signed with AWS Signature Version 4 by hand on `ureq` and `sha2`. No AWS
//! SDK: four calls don't need its async stack, and the Pi budget couldn't
//! carry it. Hosted only; Skrongle mini keeps its files on disk.
//!
//! Credentials: `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` (and
//! `AWS_SESSION_TOKEN`) when set, else the instance role through the
//! metadata service (IMDSv2, `deploy/aws/cloudformation.yaml`), kept until
//! five minutes before they expire.
//!
//! Every call blocks: run it on the jobs thread or an OS thread of its own.

use std::sync::Mutex;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::totp::{hmac, Algorithm};

/// The payload hash of a pre-signed link: the body isn't known when signing.
pub const UNSIGNED: &str = "UNSIGNED-PAYLOAD";
/// SHA-256 of nothing: the payload hash of GET and DELETE.
pub const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const SERVICE: &str = "s3";
const TIMEOUT: Duration = Duration::from_secs(60);
const IMDS: &str = "http://169.254.169.254";
const IMDS_TIMEOUT: Duration = Duration::from_secs(3);
/// Fetch new instance credentials this long before the old ones expire.
const REFRESH_EARLY: i64 = 300;

#[derive(Clone, PartialEq, Eq)]
pub struct Creds {
    pub key_id: String,
    pub secret: String,
    /// The session token of temporary credentials (the instance role's).
    pub token: Option<String>,
}

impl std::fmt::Debug for Creds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Creds({}, secret hidden)", self.key_id)
    }
}

enum Source {
    Fixed(Creds),
    Instance,
}

pub struct Bucket {
    pub name: String,
    pub region: String,
    /// `https://<bucket>.s3.<region>.amazonaws.com`.
    endpoint: String,
    source: Source,
    /// The instance role's credentials and when to fetch new ones.
    cached: Mutex<Option<(Creds, i64)>>,
    agent: ureq::Agent,
    imds: ureq::Agent,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).max_redirects(0).http_status_as_error(false).build().into()
}

impl Bucket {
    /// `creds`: fixed keys; `None` uses the instance role.
    pub fn new(name: &str, region: &str, creds: Option<Creds>) -> Bucket {
        Bucket {
            endpoint: format!("https://{name}.s3.{region}.amazonaws.com"),
            name: name.to_string(),
            region: region.to_string(),
            source: creds.map_or(Source::Instance, Source::Fixed),
            cached: Mutex::new(None),
            agent: agent(TIMEOUT),
            imds: agent(IMDS_TIMEOUT),
        }
    }

    /// Talk to `endpoint` (a stand-in server in tests) instead of AWS.
    #[cfg(test)]
    pub fn at(endpoint: &str, region: &str, creds: Creds) -> Bucket {
        Bucket { endpoint: endpoint.to_string(), ..Bucket::new("test", region, Some(creds)) }
    }

    fn host(&self) -> &str {
        self.endpoint.split_once("://").map_or(self.endpoint.as_str(), |(_, h)| h)
    }

    /// The credentials to sign with at `now`.
    pub fn credentials(&self, now: i64) -> Result<Creds, String> {
        if let Source::Fixed(c) = &self.source {
            return Ok(c.clone());
        }
        let mut cached = self.cached.lock().map_err(|_| "credentials lock poisoned".to_string())?;
        if let Some((c, refresh)) = cached.as_ref() {
            if now < *refresh {
                return Ok(c.clone());
            }
        }
        let (c, expires) = instance_creds(&self.imds)?;
        *cached = Some((c.clone(), (expires - REFRESH_EARLY).max(now + 60)));
        Ok(c)
    }

    /// The headers of a signed request (besides `host`).
    fn signed(&self, method: &str, key: &str, payload: &str, now: i64) -> Result<(String, Vec<(String, String)>), String> {
        let creds = self.credentials(now)?;
        let path = key_path(key);
        let mut headers = vec![("x-amz-content-sha256".to_string(), payload.to_string()), ("x-amz-date".to_string(), amz_date(now))];
        if let Some(t) = &creds.token {
            headers.push(("x-amz-security-token".to_string(), t.clone()));
        }
        let req = Request { method, host: self.host(), path: &path, query: &[], headers: &headers, payload };
        headers.push(("authorization".to_string(), authorization(&creds, &self.region, SERVICE, now, &req)));
        Ok((format!("{}{path}", self.endpoint), headers))
    }

    /// Upload the file at `local` as `key`, read from disk as it is sent.
    pub fn put_file(&self, key: &str, local: &std::path::Path, now: i64) -> Result<(), String> {
        let hash = file_sha256(local).map_err(|e| format!("{}: {e}", local.display()))?;
        let file = std::fs::File::open(local).map_err(|e| format!("{}: {e}", local.display()))?;
        let (url, headers) = self.signed("PUT", key, &hash, now)?;
        let mut req = self.agent.put(&url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        expect_ok("PUT", key, req.send(file))
    }

    /// The object `key`, whole (at most 10 MB).
    pub fn get(&self, key: &str, now: i64) -> Result<Vec<u8>, String> {
        let (url, headers) = self.signed("GET", key, EMPTY, now)?;
        let mut req = self.agent.get(&url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let mut res = req.call().map_err(|e| format!("S3 GET {key}: {e}"))?;
        if res.status() != 200 {
            return Err(format!("S3 GET {key}: HTTP {}", res.status()));
        }
        res.body_mut().read_to_vec().map_err(|e| format!("S3 GET {key}: {e}"))
    }

    /// Delete `key`; deleting one that isn't there is fine (S3 says 204 either way).
    pub fn delete(&self, key: &str, now: i64) -> Result<(), String> {
        let (url, headers) = self.signed("DELETE", key, EMPTY, now)?;
        let mut req = self.agent.delete(&url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        expect_ok("DELETE", key, req.call())
    }

    /// A link that GETs `key` for `expires` seconds from `now`, with no
    /// other credentials. `extra`: more query parameters, signed with it
    /// (`response-content-disposition` names the downloaded file).
    pub fn presign_get(&self, key: &str, expires: i64, extra: &[(&str, &str)], now: i64) -> Result<String, String> {
        let creds = self.credentials(now)?;
        presign(&creds, &self.endpoint, self.host(), &self.region, key, expires, extra, now)
    }
}

/// A pre-signed GET link (query-string authentication).
#[allow(clippy::too_many_arguments)]
pub fn presign(creds: &Creds, endpoint: &str, host: &str, region: &str, key: &str, expires: i64, extra: &[(&str, &str)], now: i64) -> Result<String, String> {
    let path = key_path(key);
    let mut query: Vec<(String, String)> = vec![
        ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
        ("X-Amz-Credential".into(), format!("{}/{}", creds.key_id, scope(now, region, SERVICE))),
        ("X-Amz-Date".into(), amz_date(now)),
        ("X-Amz-Expires".into(), expires.to_string()),
        ("X-Amz-SignedHeaders".into(), "host".into()),
    ];
    if let Some(t) = &creds.token {
        query.push(("X-Amz-Security-Token".into(), t.clone()));
    }
    query.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    let req = Request { method: "GET", host, path: &path, query: &query, headers: &[], payload: UNSIGNED };
    let sig = signature(creds, region, SERVICE, now, &req);
    Ok(format!("{endpoint}{path}?{}&X-Amz-Signature={sig}", canonical_query(&query)))
}

fn expect_ok(method: &str, key: &str, res: Result<http::Response<ureq::Body>, ureq::Error>) -> Result<(), String> {
    match res {
        Ok(r) if r.status().is_success() => Ok(()),
        Ok(mut r) => {
            let body = r.body_mut().read_to_string().unwrap_or_default();
            // S3's error is XML; its <Code> says enough.
            let code = body.split("<Code>").nth(1).and_then(|s| s.split("</Code>").next()).unwrap_or("");
            Err(format!("S3 {method} {key}: HTTP {} {code}", r.status()))
        }
        Err(e) => Err(format!("S3 {method} {key}: {e}")),
    }
}

/// The instance role's credentials and when they expire, from IMDSv2.
fn instance_creds(agent: &ureq::Agent) -> Result<(Creds, i64), String> {
    let text = |res: Result<http::Response<ureq::Body>, ureq::Error>, what: &str| -> Result<String, String> {
        let mut r = res.map_err(|e| format!("instance metadata ({what}): {e}"))?;
        if r.status() != 200 {
            return Err(format!("instance metadata ({what}): HTTP {}", r.status()));
        }
        r.body_mut().read_to_string().map_err(|e| format!("instance metadata ({what}): {e}"))
    };
    let token = text(agent.put(format!("{IMDS}/latest/api/token")).header("x-aws-ec2-metadata-token-ttl-seconds", "60").send_empty(), "token")?;
    let path = format!("{IMDS}/latest/meta-data/iam/security-credentials/");
    let roles = text(agent.get(&path).header("x-aws-ec2-metadata-token", &token).call(), "role")?;
    let role = roles.lines().next().map(str::trim).filter(|r| !r.is_empty()).ok_or("instance metadata: the instance has no role")?;
    let body = text(agent.get(format!("{path}{role}")).header("x-aws-ec2-metadata-token", &token).call(), "credentials")?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("instance metadata: {e}"))?;
    let field = |k: &str| v.get(k).and_then(|s| s.as_str()).map(str::to_string).ok_or(format!("instance metadata: no {k}"));
    let creds = Creds { key_id: field("AccessKeyId")?, secret: field("SecretAccessKey")?, token: Some(field("Token")?) };
    let expires = parse_time(&field("Expiration")?).ok_or("instance metadata: unreadable Expiration")?;
    Ok((creds, expires))
}

/// Unix time of `2026-10-28T12:34:56Z`.
pub fn parse_time(s: &str) -> Option<i64> {
    let (date, time) = s.trim().trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|p| p.split('.').next().and_then(|p| p.parse::<i64>().ok()));
    let (h, min, sec) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
    if !(1..=12).contains(&m) {
        return None;
    }
    Some(crate::profile::days_from_civil(y, m as usize, day) * 86_400 + h * 3600 + min * 60 + sec)
}

fn file_sha256(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(buf.get(..n).unwrap_or_default());
    }
    Ok(hex(&h.finalize()))
}

// ---------- Signature Version 4 ----------

/// What is signed of a request.
pub struct Request<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// The URI-encoded path ([`key_path`]).
    pub path: &'a str,
    /// Query parameters, not encoded yet.
    pub query: &'a [(String, String)],
    /// Headers besides `host`, lowercase names.
    pub headers: &'a [(String, String)],
    /// Hex SHA-256 of the body, or [`UNSIGNED`].
    pub payload: &'a str,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// AWS's URI encoding: everything but `A-Za-z0-9-_.~` as `%XX`, and `/`
/// too unless `keep_slash`.
pub fn encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `/` and the key, encoded.
pub fn key_path(key: &str) -> String {
    format!("/{}", encode(key.trim_start_matches('/'), true))
}

/// `20261028T123456Z`.
pub fn amz_date(t: i64) -> String {
    let s = t.rem_euclid(86_400);
    format!("{}T{:02}{:02}{:02}Z", day(t), s / 3600, s / 60 % 60, s % 60)
}

/// `20261028`.
fn day(t: i64) -> String {
    let (y, m, d) = crate::profile::civil(t);
    format!("{y:04}{m:02}{d:02}")
}

fn scope(t: i64, region: &str, service: &str) -> String {
    format!("{}/{region}/{service}/aws4_request", day(t))
}

pub fn canonical_query(query: &[(String, String)]) -> String {
    let mut q: Vec<(String, String)> = query.iter().map(|(k, v)| (encode(k, false), encode(v, false))).collect();
    q.sort();
    q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

/// The canonical request and the signed header names.
pub fn canonical_request(req: &Request) -> (String, String) {
    let mut headers: Vec<(String, String)> = req.headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string())).collect();
    headers.push(("host".into(), req.host.to_string()));
    headers.sort();
    let lines: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    (format!("{}\n{}\n{}\n{lines}\n{signed}\n{}", req.method, req.path, canonical_query(req.query), req.payload), signed)
}

/// The hex signature of `req` at `t`.
pub fn signature(creds: &Creds, region: &str, service: &str, t: i64, req: &Request) -> String {
    let (canonical, _) = canonical_request(req);
    let to_sign = format!("AWS4-HMAC-SHA256\n{}\n{}\n{}", amz_date(t), scope(t, region, service), hex(&Sha256::digest(canonical.as_bytes())));
    let mut k = hmac(Algorithm::Sha256, format!("AWS4{}", creds.secret).as_bytes(), day(t).as_bytes());
    for part in [region, service, "aws4_request"] {
        k = hmac(Algorithm::Sha256, &k, part.as_bytes());
    }
    hex(&hmac(Algorithm::Sha256, &k, to_sign.as_bytes()))
}

/// The `Authorization` header of `req`.
pub fn authorization(creds: &Creds, region: &str, service: &str, t: i64, req: &Request) -> String {
    let (_, signed) = canonical_request(req);
    format!("AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={signed}, Signature={}", creds.key_id, scope(t, region, service), signature(creds, region, service, t, req))
}
