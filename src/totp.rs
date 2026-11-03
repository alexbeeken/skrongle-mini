//! Time-based one-time passwords (RFC 6238 over RFC 4226's HOTP), the codes
//! authenticator apps show, and the `otpauth://totp/…` URIs they are handed
//! (the "Key Uri Format" every app reads from a QR code).
//!
//! HMAC-SHA1 is the protocol's default and what every app supports; SHA-256
//! and SHA-512 are here too because the URI may ask for them. SHA-1 is
//! written out below (RFC 3174) rather than pulled in as a crate: it is
//! small, and HMAC-SHA1 is still sound for HOTP (RFC 4226 appendix A).
//! Base32 (RFC 4648) likewise.

use sha2::{Digest, Sha256, Sha512};

/// The defaults of the Key Uri Format, which every app assumes.
pub const DEFAULT_DIGITS: u32 = 6;
pub const DEFAULT_PERIOD: u64 = 30;

/// SHA-256 and SHA-512 come only from a parsed key: the dev authenticator's.
#[cfg_attr(not(any(feature = "dev", test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Algorithm {
    #[default]
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Algorithm::Sha1 => "SHA1",
            Algorithm::Sha256 => "SHA256",
            Algorithm::Sha512 => "SHA512",
        }
    }

    #[cfg(any(feature = "dev", test))]
    fn parse(s: &str) -> Option<Algorithm> {
        match s.to_ascii_uppercase().replace('-', "").as_str() {
            "SHA1" => Some(Algorithm::Sha1),
            "SHA256" => Some(Algorithm::Sha256),
            "SHA512" => Some(Algorithm::Sha512),
            _ => None,
        }
    }
}

// ---------- SHA-1 (RFC 3174) ----------

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn hash(alg: Algorithm, data: &[u8]) -> Vec<u8> {
    match alg {
        Algorithm::Sha1 => sha1(data).to_vec(),
        Algorithm::Sha256 => Sha256::digest(data).to_vec(),
        Algorithm::Sha512 => Sha512::digest(data).to_vec(),
    }
}

/// HMAC (RFC 2104) with the given hash.
pub fn hmac(alg: Algorithm, key: &[u8], msg: &[u8]) -> Vec<u8> {
    let block = if alg == Algorithm::Sha512 { 128 } else { 64 };
    let mut k = if key.len() > block { hash(alg, key) } else { key.to_vec() };
    k.resize(block, 0);
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&hash(alg, &inner));
    hash(alg, &outer)
}

// ---------- HOTP / TOTP ----------

/// RFC 4226 HOTP: the code for one counter value, `digits` long, zero-padded.
pub fn hotp(alg: Algorithm, secret: &[u8], counter: u64, digits: u32) -> String {
    let mac = hmac(alg, secret, &counter.to_be_bytes());
    let offset = (mac[mac.len() - 1] & 0x0f) as usize;
    let bin = u32::from_be_bytes([mac[offset] & 0x7f, mac[offset + 1], mac[offset + 2], mac[offset + 3]]);
    let code = bin as u64 % 10u64.pow(digits);
    format!("{code:0width$}", width = digits as usize)
}

/// The RFC 6238 time step a moment falls in (T0 = 0).
pub fn step(unix_time: u64, period: u64) -> u64 {
    unix_time / period.max(1)
}

/// A whole TOTP set-up: what an authenticator app stores for one account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub secret: Vec<u8>,
    pub algorithm: Algorithm,
    pub digits: u32,
    pub period: u64,
    /// The URI's issuer (the service), maybe empty.
    pub issuer: String,
    /// The account name (usually an address), maybe empty.
    pub account: String,
}

impl Key {
    /// The protocol's defaults (SHA-1, 6 digits, 30 s) around a secret.
    pub fn new(secret: Vec<u8>, issuer: &str, account: &str) -> Key {
        Key { secret, algorithm: Algorithm::Sha1, digits: DEFAULT_DIGITS, period: DEFAULT_PERIOD, issuer: issuer.into(), account: account.into() }
    }

    /// The code for a moment.
    #[cfg(any(feature = "dev", test))]
    pub fn code_at(&self, unix_time: u64) -> String {
        hotp(self.algorithm, &self.secret, step(unix_time, self.period), self.digits)
    }

    /// The code for a time step.
    pub fn code_for_step(&self, step: u64) -> String {
        hotp(self.algorithm, &self.secret, step, self.digits)
    }

    /// Seconds until the code shown at `unix_time` changes.
    #[cfg(any(feature = "dev", test))]
    pub fn remaining(&self, unix_time: u64) -> u64 {
        self.period - unix_time % self.period
    }

    /// Check a code typed at `unix_time`: the current step or one either
    /// side (clocks drift, and a code read just before it changed is still
    /// typed), and only a step after `last_step`, so a code works once.
    /// The step it matched, to store as the new `last_step`.
    pub fn verify(&self, code: &str, unix_time: u64, last_step: Option<u64>) -> Option<u64> {
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        if code.len() != self.digits as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let now = step(unix_time, self.period);
        // Every candidate is computed, so the time taken does not say which matched.
        let mut found = None;
        for s in [now.saturating_sub(1), now, now + 1] {
            let ok = constant_time_eq(self.code_for_step(s).as_bytes(), code.as_bytes());
            if ok && last_step.is_none_or(|l| s > l) && found.is_none() {
                found = Some(s);
            }
        }
        found
    }

    pub fn secret_base32(&self) -> String {
        base32_encode(&self.secret)
    }

    /// The Key Uri Format: `otpauth://totp/Issuer:account?secret=…&issuer=…`
    /// with the algorithm, digits and period spelled out.
    pub fn uri(&self) -> String {
        let label = match (self.issuer.is_empty(), self.account.is_empty()) {
            (false, false) => format!("{}:{}", uri_encode(&self.issuer), uri_encode(&self.account)),
            (false, true) => uri_encode(&self.issuer),
            _ => uri_encode(&self.account),
        };
        let mut uri = format!("otpauth://totp/{label}?secret={}", self.secret_base32());
        if !self.issuer.is_empty() {
            uri.push_str(&format!("&issuer={}", uri_encode(&self.issuer)));
        }
        uri.push_str(&format!("&algorithm={}&digits={}&period={}", self.algorithm.as_str(), self.digits, self.period));
        uri
    }

    /// What an app accepts as "enter a setup key" or "scan": an
    /// `otpauth://totp/…` URI, or a bare base32 secret (the defaults apply).
    #[cfg(any(feature = "dev", test))]
    pub fn parse(input: &str) -> Result<Key, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("Paste an otpauth://totp/… link or a setup key.".into());
        }
        if !input.to_ascii_lowercase().starts_with("otpauth://") {
            let secret = base32_decode(input).ok_or("That setup key is not base32 (letters A-Z and digits 2-7).")?;
            if secret.is_empty() {
                return Err("That setup key is empty.".into());
            }
            return Ok(Key::new(secret, "", ""));
        }
        let rest = &input["otpauth://".len()..];
        let (kind, rest) = rest.split_once('/').ok_or("The link has no type (otpauth://totp/…).")?;
        if !kind.eq_ignore_ascii_case("totp") {
            return Err(format!("Only time-based codes (otpauth://totp/…) are supported, not {kind}."));
        }
        let (label, query) = rest.split_once('?').unwrap_or((rest, ""));
        let label = uri_decode(label);
        let (mut issuer, account) = match label.split_once(':') {
            Some((i, a)) => (i.trim().to_string(), a.trim().to_string()),
            None => (String::new(), label.trim().to_string()),
        };
        let mut key = Key::new(Vec::new(), "", &account);
        let mut secret = None;
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = uri_decode(v);
            match k.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(base32_decode(&v).ok_or("The link's secret is not base32.")?),
                "issuer" => issuer = v.trim().to_string(),
                "algorithm" => key.algorithm = Algorithm::parse(&v).ok_or(format!("Unknown algorithm {v}."))?,
                "digits" => key.digits = v.parse().ok().filter(|d| (6..=8).contains(d)).ok_or("Digits must be 6, 7 or 8.")?,
                "period" => key.period = v.parse().ok().filter(|p| (1..=300).contains(p)).ok_or("The period must be 1 to 300 seconds.")?,
                _ => {}
            }
        }
        key.secret = secret.filter(|s| !s.is_empty()).ok_or("The link has no secret.")?;
        key.issuer = issuer;
        Ok(key)
    }
}

/// Compare without stopping at the first difference.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A fresh 160-bit secret, the length RFC 4226 recommends.
pub fn new_secret() -> Vec<u8> {
    let mut s = vec![0u8; 20];
    crate::random::fill(&mut s);
    s
}

// ---------- base32 (RFC 4648, no padding) ----------

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Any case; spaces, hyphens and `=` padding are ignored (apps show keys in
/// groups of four).
#[cfg(any(feature = "dev", test))]
pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        if matches!(c, b' ' | b'-' | b'=' | b'\t') {
            continue;
        }
        let v = B32.iter().position(|&x| x == c.to_ascii_uppercase())? as u32;
        buf = ((buf << 5) | v) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'@') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(any(feature = "dev", test))]
fn uri_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = if b[i] == b'%' { b.get(i + 1..i + 3) } else { None };
        if let Some(v) = hex.and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The QR code of `data` (an `otpauth://` link) as inline SVG, one `<path>`
/// of unit squares on a `viewBox` of its modules plus a 4-module quiet zone,
/// so CSS sets its size. Error correction M, the apps' usual. `None` when
/// the text is too long for a QR code (a TOTP link never is).
pub fn qr_svg(data: &str, label: &str) -> Option<String> {
    use qrcode::{Color, EcLevel, QrCode};
    let code = QrCode::with_error_correction_level(data, EcLevel::M).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    const QUIET: usize = 4;
    let size = width + 2 * QUIET;
    let mut path = String::new();
    for y in 0..width {
        let row = &colors[y * width..(y + 1) * width];
        let mut x = 0;
        while x < width {
            if row[x] != Color::Dark {
                x += 1;
                continue;
            }
            let start = x;
            while x < width && row[x] == Color::Dark {
                x += 1;
            }
            path.push_str(&format!("M{} {}h{}v1h-{}z", start + QUIET, y + QUIET, x - start, x - start));
        }
    }
    Some(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" shape-rendering="crispEdges" role="img" aria-label="{}" data-modules="{width}"><rect width="{size}" height="{size}" fill="#fff"/><path d="{path}" fill="#000"/></svg>"##,
        crate::error::html_escape(label)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_matches_rfc_3174() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        assert_eq!(hex(&sha1(&vec![b'a'; 1_000_000])), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn hmac_sha1_matches_rfc_2202() {
        assert_eq!(hex(&hmac(Algorithm::Sha1, &[0x0b; 20], b"Hi There")), "b617318655057264e28bc0b6fb378c8ef146be00");
        assert_eq!(hex(&hmac(Algorithm::Sha1, b"Jefe", b"what do ya want for nothing?")), "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
        // A key longer than the block is hashed first.
        assert_eq!(hex(&hmac(Algorithm::Sha1, &[0xaa; 80], b"Test Using Larger Than Block-Size Key - Hash Key First")), "aa4ae5e15272d00e95705637ce8a3b55ed402112");
    }

    #[test]
    fn hotp_matches_rfc_4226() {
        let want = ["755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871", "520489"];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(hotp(Algorithm::Sha1, b"12345678901234567890", i as u64, 6), *w, "counter {i}");
        }
    }

    /// RFC 6238 appendix B: 8 digits, the three algorithms with their seeds.
    #[test]
    fn totp_matches_rfc_6238() {
        let seed1 = b"12345678901234567890".to_vec();
        let seed256 = b"12345678901234567890123456789012".to_vec();
        let seed512 = b"1234567890123456789012345678901234567890123456789012345678901234".to_vec();
        let rows: [(u64, &str, &str, &str); 6] = [
            (59, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ];
        for (t, s1, s256, s512) in rows {
            for (alg, seed, want) in [(Algorithm::Sha1, &seed1, s1), (Algorithm::Sha256, &seed256, s256), (Algorithm::Sha512, &seed512, s512)] {
                let key = Key { digits: 8, algorithm: alg, ..Key::new(seed.clone(), "", "") };
                assert_eq!(key.code_at(t), want, "{alg:?} at {t}");
            }
        }
        // Six digits are the last six of the same number.
        assert_eq!(Key::new(seed1, "", "").code_at(59), "287082");
    }

    #[test]
    fn base32_roundtrips_and_reads_what_apps_show() {
        assert_eq!(base32_encode(b"12345678901234567890"), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_decode("MZXW6YTBOI======").unwrap(), b"foobar");
        assert_eq!(base32_decode("mzxw 6ytb-oi").unwrap(), b"foobar");
        assert_eq!(base32_decode("MZXW1"), None);
        for n in 0..30 {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base32_decode(&base32_encode(&data)).unwrap(), data);
        }
    }

    #[test]
    fn uris_parse_and_print_the_key_uri_format() {
        let k = Key::parse("otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA1&digits=6&period=30").unwrap();
        assert_eq!((k.issuer.as_str(), k.account.as_str(), k.digits, k.period, k.algorithm), ("ACME Co", "john.doe@email.com", 6, 30, Algorithm::Sha1));
        assert_eq!(k.secret_base32(), "HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ");
        assert_eq!(Key::parse(&k.uri()).unwrap(), k);
        assert_eq!(k.uri(), "otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA1&digits=6&period=30");

        // The issuer parameter wins over the label's prefix; defaults fill in.
        let k = Key::parse("otpauth://TOTP/Old:ann?secret=jbswy3dpehpk3pxp&issuer=New").unwrap();
        assert_eq!((k.issuer.as_str(), k.account.as_str(), k.digits, k.period), ("New", "ann", 6, 30));
        let k = Key::parse("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&algorithm=SHA256&digits=8&period=60").unwrap();
        assert_eq!((k.algorithm, k.digits, k.period), (Algorithm::Sha256, 8, 60));

        // A bare setup key.
        let k = Key::parse(" jbsw y3dp ehpk 3pxp ").unwrap();
        assert_eq!((k.secret_base32().as_str(), k.issuer.as_str()), ("JBSWY3DPEHPK3PXP", ""));

        assert!(Key::parse("otpauth://hotp/x?secret=JBSWY3DPEHPK3PXP&counter=1").unwrap_err().contains("time-based"));
        assert!(Key::parse("otpauth://totp/x?issuer=a").unwrap_err().contains("no secret"));
        assert!(Key::parse("otpauth://totp/x?secret=JBSW&digits=4").unwrap_err().contains("Digits"));
        assert!(Key::parse("not a key!").is_err());
        assert!(Key::parse("").is_err());
    }

    #[test]
    fn verify_takes_one_step_of_drift_and_each_step_once() {
        let k = Key::new(b"12345678901234567890".to_vec(), "", "");
        let t = 1_111_111_109; // step 37037036
        let s = step(t, 30);
        assert_eq!(k.verify(&k.code_at(t), t, None), Some(s));
        assert_eq!(k.verify(&k.code_at(t - 30), t, None), Some(s - 1));
        assert_eq!(k.verify(&k.code_at(t + 30), t, None), Some(s + 1));
        assert_eq!(k.verify(&k.code_at(t - 60), t, None), None, "two steps old");
        assert_eq!(k.verify(&k.code_at(t + 60), t, None), None, "two steps ahead");
        // Used once, the same code (or an older one) is refused.
        assert_eq!(k.verify(&k.code_at(t), t, Some(s)), None);
        assert_eq!(k.verify(&k.code_at(t - 30), t, Some(s - 1)), None);
        assert_eq!(k.verify(&k.code_at(t + 30), t, Some(s)), Some(s + 1));
        // Spaces are fine; other shapes are not.
        let c = k.code_at(t);
        assert_eq!(k.verify(&format!("{} {}", &c[..3], &c[3..]), t, None), Some(s));
        assert_eq!(k.verify("12345", t, None), None);
        assert_eq!(k.verify("abcdef", t, None), None);
        assert!(constant_time_eq(b"123", b"123") && !constant_time_eq(b"123", b"124") && !constant_time_eq(b"12", b"123"));
    }

    #[test]
    fn remaining_counts_down_to_the_next_step() {
        let k = Key::new(new_secret(), "", "");
        assert_eq!(k.secret.len(), 20);
        assert_eq!(k.remaining(60), 30);
        assert_eq!(k.remaining(89), 1);
        assert_eq!(k.code_at(60), k.code_at(89));
        assert_eq!(k.code_at(60), k.code_for_step(2));
    }

    #[test]
    fn qr_svg_draws_the_link_with_a_quiet_zone() {
        let key = Key::new(vec![0x42; 20], "Skrongle", "a@example.org");
        let svg = qr_svg(&key.uri(), "QR code").unwrap();
        // A link this long needs a version 5 or 6 code (37 or 41 modules); the quiet zone adds 4 each side.
        let modules: usize = svg.split("data-modules=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        assert!((29..=57).contains(&modules) && modules % 4 == 1, "modules: {modules}");
        assert!(svg.contains(&format!("viewBox=\"0 0 {0} {0}\"", modules + 8)));
        // The finder pattern's top row: seven dark modules at the top-left corner, after the quiet zone.
        assert!(svg.contains("M4 4h7v1h-7z"), "{svg}");
        assert_eq!(qr_svg(&key.uri(), "QR code"), qr_svg(&key.uri(), "QR code"), "deterministic");
        assert!(qr_svg(&"x".repeat(5000), "").is_none());
    }
}
