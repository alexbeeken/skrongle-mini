//! Pre-gzips `static/` into `$OUT_DIR/static_gz/` so the server never
//! compresses per request (`AssetsGz` in src/routes/mod.rs embeds that
//! folder next to `static/`). A file that does not shrink (PNG, WOFF2) gets
//! no `.gz` sibling and is served as is.

use flate2::{write::GzEncoder, Compression};
use std::{fs, io::Write, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=static");
    let out = Path::new(&std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("static_gz");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).expect("create static_gz");
    walk(Path::new("static"), Path::new(""), &out);
}

fn walk(root: &Path, rel: &Path, out: &Path) {
    let Ok(entries) = fs::read_dir(root.join(rel)) else { return };
    for entry in entries.flatten() {
        let rel = rel.join(entry.file_name());
        let full = root.join(&rel);
        if full.is_dir() {
            walk(root, &rel, out);
            continue;
        }
        let Ok(raw) = fs::read(&full) else { continue };
        let mut enc = GzEncoder::new(Vec::new(), Compression::best());
        if enc.write_all(&raw).is_err() {
            continue;
        }
        let Ok(gz) = enc.finish() else { continue };
        if gz.len() >= raw.len() {
            continue;
        }
        let dest = out.join(&rel);
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir).expect("create dir");
        }
        fs::write(dest, gz).expect("write gz");
    }
}
