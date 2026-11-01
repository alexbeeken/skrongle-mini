//! Percent-encoding for query values.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(urlencode("a-b_c.d~e1"), "a-b_c.d~e1");
        assert_eq!(urlencode("a b&c/é"), "a%20b%26c%2F%C3%A9");
    }
}
