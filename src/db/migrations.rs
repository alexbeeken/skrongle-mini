//! The schema migrations: one step per version, each with the SQL that
//! applies it (`up`) and the SQL that takes it back (`down`), run by
//! `db::migrate_to` through `rusqlite_migration`. v1 creates the whole
//! schema and has no `down`: there is no schema before it. A later change
//! appends its own step at the end, with both halves.
//!
//! A `down` leaves the schema exactly as the version before made it
//! (`db::tests::every_down_restores_the_schema_before_it`). It keeps the
//! rows it can; what the older schema has no place for is said in the
//! `.down.sql` file.

use rusqlite_migration::{Migrations, M};

const STEPS: &[M<'static>] = &[
    // v1: the schema
    M::up(include_str!("migrations/v001_schema.sql")),
    // v2: no Skrongle Cash, no shop
    M::up(include_str!("migrations/v002_no_skc.sql")).down(include_str!("migrations/v002_no_skc.down.sql")),
    // v3: share links carry a name, not an address
    M::up(include_str!("migrations/v003_share_links.sql")).down(include_str!("migrations/v003_share_links.down.sql")),
    // v4: per-account permissions and their presets
    M::up(include_str!("migrations/v004_permissions.sql")).down(include_str!("migrations/v004_permissions.down.sql")),
    // v5: courses.error_kind
    M::up(include_str!("migrations/v005_error_kind.sql")).down(include_str!("migrations/v005_error_kind.down.sql")),
];

/// Schema migrations, applied in order. `PRAGMA user_version` tracks which have run.
pub(crate) const MIGRATIONS: Migrations<'static> = Migrations::from_slice(STEPS);

/// How many there are: the schema level this binary migrates to.
pub(crate) const COUNT: usize = STEPS.len();

/// Migrations (1-based versions) that rebuild a table other tables point at,
/// in their `up` or their `down`: they run with foreign keys off. A step
/// listed here also carries `.foreign_key_check()`, so it is checked before
/// it commits.
pub(crate) const REBUILDS: &[i64] = &[];
