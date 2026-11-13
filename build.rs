//! Pre-gzips `static/` into `$OUT_DIR/static_gz/` so the server never
//! compresses per request (`AssetsGz` in src/routes/mod.rs). A file that does
//! not shrink (PNG, WOFF2) gets no `.gz` form; it is copied to
//! `$OUT_DIR/static_raw/` instead, and a release build embeds only those two
//! folders: each file once, gzipped where that is smaller (#93). The rare
//! client without gzip gets the file inflated, never more than
//! `SKRONGLE_STATIC_MAX_RAW` bytes (the largest gzipped file's raw size).

use flate2::{write::GzEncoder, Compression};
use std::{fs, io::Write, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=static");
    let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR");
    let gz = Path::new(&out_dir).join("static_gz");
    let raw = Path::new(&out_dir).join("static_raw");
    for dir in [&gz, &raw] {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).expect("create static output dir");
    }
    let mut max_raw = 0;
    walk(Path::new("static"), Path::new(""), &gz, &raw, &mut max_raw);
    println!("cargo:rustc-env=SKRONGLE_STATIC_MAX_RAW={max_raw}");
}

fn write(dest: &Path, bytes: &[u8]) {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).expect("create dir");
    }
    fs::write(dest, bytes).expect("write static file");
}

fn walk(root: &Path, rel: &Path, gz_out: &Path, raw_out: &Path, max_raw: &mut usize) {
    let Ok(entries) = fs::read_dir(root.join(rel)) else { return };
    for entry in entries.flatten() {
        let rel = rel.join(entry.file_name());
        let full = root.join(&rel);
        if full.is_dir() {
            walk(root, &rel, gz_out, raw_out, max_raw);
            continue;
        }
        let raw = fs::read(&full).expect("read static file");
        let mut enc = GzEncoder::new(Vec::new(), Compression::best());
        enc.write_all(&raw).expect("gzip static file");
        let gz = enc.finish().expect("gzip static file");
        if gz.len() < raw.len() {
            write(&gz_out.join(&rel), &gz);
            *max_raw = (*max_raw).max(raw.len());
        } else {
            write(&raw_out.join(&rel), &raw);
        }
    }
}
