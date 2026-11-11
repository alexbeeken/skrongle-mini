//! The schema migrations: one SQL string per version, applied in order by
//! `db::migrate`. v1 creates the whole schema; a later change appends its
//! own entry at the end.

/// Schema migrations, applied in order. `PRAGMA user_version` tracks which have run.
pub(crate) const MIGRATIONS: &[&str] = &[
    include_str!("migrations/v001_schema.sql"), // v1: the schema
    include_str!("migrations/v002_no_skc.sql"), // v2: no Skrongle Cash, no shop
    include_str!("migrations/v003_share_links.sql"), // v3: share links carry a name, not an address
    include_str!("migrations/v004_error_kind.sql"), // v4: courses.error_kind
];

/// Migrations (1-based versions) that rebuild a table other tables point at:
/// they run with foreign keys off and are checked before they commit.
pub(crate) const REBUILDS: &[i64] = &[];
