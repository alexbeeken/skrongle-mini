//! Where finished files live (#871). The only files so far are data exports
//! (`exports.rs`).
//!
//! - [`Storage::Disk`]: Skrongle mini and the dev server. The finished file
//!   stays in the exports folder next to the database, and the download
//!   streams it from there.
//! - [`Storage::S3`]: the hosted site. The finished file is uploaded to a
//!   private bucket (`SKRONGLE_S3_BUCKET`, in `AWS_REGION`) under
//!   `exports/` and the local copy deleted. The download still checks the
//!   session and the one-time link, then redirects to a pre-signed link that
//!   works for [`LINK_SECS`]. The bucket's lifecycle rule deletes anything
//!   under `exports/` after two days, whatever the server misses.
//!
//! The hosted site refuses to start without the bucket (`require_providers`
//! in `main.rs`); in Skrongle mini `SKRONGLE_S3_BUCKET` reads as unset.

pub mod s3;

/// The setting that names the bucket.
pub const BUCKET_VAR: &str = "SKRONGLE_S3_BUCKET";
/// How long a pre-signed download link works.
pub const LINK_SECS: i64 = 60;

pub enum Storage {
    Disk,
    S3(Box<s3::Bucket>),
}

impl Storage {
    /// From the settings: `SKRONGLE_S3_BUCKET` (`bucket`) and the
    /// environment's `AWS_REGION` and keys (`raw`).
    pub fn from_settings(bucket: Option<String>, raw: &dyn Fn(&str) -> Option<String>) -> Result<Storage, String> {
        let Some(name) = bucket else { return Ok(Storage::Disk) };
        let region = raw("AWS_REGION").filter(|r| !r.is_empty()).ok_or_else(|| format!("{BUCKET_VAR} needs AWS_REGION, the bucket's region"))?;
        let creds = match (raw("AWS_ACCESS_KEY_ID"), raw("AWS_SECRET_ACCESS_KEY")) {
            (Some(key_id), Some(secret)) if !key_id.is_empty() && !secret.is_empty() => {
                Some(s3::Creds { key_id, secret, token: raw("AWS_SESSION_TOKEN").filter(|t| !t.is_empty()) })
            }
            _ => None,
        };
        Ok(Storage::S3(Box::new(s3::Bucket::new(&name, &region, creds))))
    }

    pub fn bucket(&self) -> Option<&s3::Bucket> {
        match self {
            Storage::Disk => None,
            Storage::S3(b) => Some(b),
        }
    }

    /// For the startup log.
    pub fn describe(&self) -> String {
        match self {
            Storage::Disk => "data exports: on disk, next to the database".to_string(),
            Storage::S3(b) => format!("data exports: S3 bucket {} ({}), pre-signed download links for {LINK_SECS}s", b.name, b.region),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
