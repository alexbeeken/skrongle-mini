use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::s3::{self, Creds, Request, EMPTY};
use super::*;

/// The keys of AWS's Signature Version 4 test suite.
fn suite_creds() -> Creds {
    Creds { key_id: "AKIDEXAMPLE".into(), secret: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(), token: None }
}

/// The keys of the S3 signing examples in AWS's S3 API reference.
fn s3_doc_creds() -> Creds {
    Creds { key_id: "AKIAIOSFODNN7EXAMPLE".into(), secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(), token: None }
}

/// 20150830T123600Z and 20130524T000000Z.
const SUITE_TIME: i64 = 1_440_938_160;
const S3_DOC_TIME: i64 = 1_369_353_600;

#[test]
fn dates_in_the_signature_format() {
    assert_eq!(s3::amz_date(SUITE_TIME), "20150830T123600Z");
    assert_eq!(s3::amz_date(S3_DOC_TIME), "20130524T000000Z");
    assert_eq!(s3::parse_time("2015-08-30T12:36:00Z"), Some(SUITE_TIME));
    assert_eq!(s3::parse_time("2013-05-24T00:00:00Z"), Some(S3_DOC_TIME));
    assert_eq!(s3::parse_time("soon"), None);
}

/// "get-vanilla" from AWS's published Signature Version 4 test suite.
#[test]
fn signature_matches_the_aws_test_suite() {
    let headers = vec![("X-Amz-Date".to_string(), "20150830T123600Z".to_string())];
    let req = Request { method: "GET", host: "example.amazonaws.com", path: "/", query: &[], headers: &headers, payload: EMPTY };
    assert_eq!(s3::signature(&suite_creds(), "us-east-1", "service", SUITE_TIME, &req), "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31");
    assert_eq!(
        s3::authorization(&suite_creds(), "us-east-1", "service", SUITE_TIME, &req),
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );
}

/// "Example: GET Object" from the S3 API reference (header signing).
#[test]
fn signature_matches_the_s3_get_object_example() {
    let headers = vec![
        ("range".to_string(), "bytes=0-9".to_string()),
        ("x-amz-content-sha256".to_string(), EMPTY.to_string()),
        ("x-amz-date".to_string(), "20130524T000000Z".to_string()),
    ];
    let req = Request { method: "GET", host: "examplebucket.s3.amazonaws.com", path: "/test.txt", query: &[], headers: &headers, payload: EMPTY };
    assert_eq!(s3::signature(&s3_doc_creds(), "us-east-1", "s3", S3_DOC_TIME, &req), "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41");
}

/// The pre-signed URL example from the S3 API reference.
#[test]
fn presigned_link_matches_the_s3_example() {
    let url = s3::presign(&s3_doc_creds(), "https://examplebucket.s3.amazonaws.com", "examplebucket.s3.amazonaws.com", "us-east-1", "test.txt", 86_400, &[], S3_DOC_TIME).unwrap();
    assert_eq!(
        url,
        "https://examplebucket.s3.amazonaws.com/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host&X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
    );
}

#[test]
fn keys_and_values_are_encoded_the_aws_way() {
    assert_eq!(s3::key_path("exports/12-ab.json"), "/exports/12-ab.json");
    assert_eq!(s3::key_path("a b/ü"), "/a%20b/%C3%BC");
    assert_eq!(s3::encode("attachment; filename=\"x.json\"", false), "attachment%3B%20filename%3D%22x.json%22");
}

#[test]
fn settings() {
    let none = |_: &str| None;
    assert!(matches!(Storage::from_settings(None, &none), Ok(Storage::Disk)));
    let err = Storage::from_settings(Some("b".into()), &none).err().unwrap();
    assert!(err.contains("AWS_REGION"), "{err}");
    let region = |k: &str| (k == "AWS_REGION").then(|| "us-west-2".to_string());
    let s = Storage::from_settings(Some("skrongle-files".into()), &region).unwrap();
    let b = s.bucket().unwrap();
    assert_eq!((b.name.as_str(), b.region.as_str()), ("skrongle-files", "us-west-2"));
    assert!(s.describe().contains("skrongle-files"));
    // Keys from the environment are used as they are: no metadata service.
    let keys = |k: &str| match k {
        "AWS_REGION" => Some("us-west-2".to_string()),
        "AWS_ACCESS_KEY_ID" => Some("AKID".to_string()),
        "AWS_SECRET_ACCESS_KEY" => Some("secret".to_string()),
        _ => None,
    };
    let s = Storage::from_settings(Some("b".into()), &keys).unwrap();
    assert_eq!(s.bucket().unwrap().credentials(0).unwrap().key_id, "AKID");
}

/// A stand-in for S3 on 127.0.0.1: keeps objects in memory and answers
/// PUT, GET and DELETE of signed requests. Each request's headers are kept.
pub(crate) fn stand_in() -> (String, Arc<Mutex<Vec<HashMap<String, String>>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut objects: HashMap<String, Vec<u8>> = HashMap::new();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut r = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let mut parts = line.split_whitespace();
                let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
                let mut headers = HashMap::new();
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    let h = h.trim_end();
                    if h.is_empty() {
                        break;
                    }
                    let (k, v) = h.split_once(':').unwrap();
                    headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
                let len: usize = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let signed = headers.get("authorization").is_some_and(|a| a.starts_with("AWS4-HMAC-SHA256 Credential=AKID/"));
                headers.insert(":method".into(), method.clone());
                log.lock().unwrap().push(headers);
                let (status, out) = match (signed, method.as_str()) {
                    (false, _) => ("403 Forbidden", b"<Error><Code>AccessDenied</Code></Error>".to_vec()),
                    (true, "PUT") => {
                        objects.insert(path, body);
                        ("200 OK", Vec::new())
                    }
                    (true, "GET") => match objects.get(&path) {
                        Some(b) => ("200 OK", b.clone()),
                        None => ("404 Not Found", b"<Error><Code>NoSuchKey</Code></Error>".to_vec()),
                    },
                    (true, "DELETE") => {
                        objects.remove(&path);
                        ("204 No Content", Vec::new())
                    }
                    _ => ("405 Method Not Allowed", Vec::new()),
                };
                let head = format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\n\r\n", out.len());
                if stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&out)).is_err() {
                    break;
                }
            }
        }
    });
    (format!("http://{addr}"), seen)
}

/// Upload, read back, delete: the client's three calls against the stand-in.
#[test]
fn put_get_and_delete_against_a_stand_in() {
    let (endpoint, seen) = stand_in();
    let creds = Creds { key_id: "AKID".into(), secret: "secret".into(), token: Some("session".into()) };
    let bucket = s3::Bucket::at(&endpoint, "us-west-2", creds);
    let dir = std::env::temp_dir().join(format!("skrongle-s3-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("1-abc.json");
    std::fs::write(&file, b"{\"tables\":{}}\n").unwrap();
    let now = crate::db::now();
    bucket.put_file("exports/1-abc.json", &file, now).unwrap();
    assert_eq!(bucket.get("exports/1-abc.json", now).unwrap(), b"{\"tables\":{}}\n");
    bucket.delete("exports/1-abc.json", now).unwrap();
    let err = bucket.get("exports/1-abc.json", now).unwrap_err();
    assert!(err.contains("404"), "{err}");
    let put = seen.lock().unwrap()[0].clone();
    // The body's own hash is signed, and the session token is sent.
    assert_eq!(put.get("x-amz-content-sha256").map(String::as_str), Some(s3::hex(&<sha2::Sha256 as sha2::Digest>::digest(b"{\"tables\":{}}\n")).as_str()));
    assert_eq!(put.get("x-amz-security-token").map(String::as_str), Some("session"));
    assert!(put["authorization"].contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token,"));
    // A pre-signed link carries the token and the response headers in its query.
    let link = bucket.presign_get("exports/1-abc.json", LINK_SECS, &[("response-content-disposition", "attachment")], now).unwrap();
    assert!(link.starts_with(&format!("{endpoint}/exports/1-abc.json?")), "{link}");
    assert!(link.contains("X-Amz-Expires=60&") && link.contains("X-Amz-Security-Token=session") && link.contains("response-content-disposition=attachment"), "{link}");
    let _ = std::fs::remove_dir_all(&dir);
}
