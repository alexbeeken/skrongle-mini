//! The content check's control set (#1061): `data/moderation/controls/`,
//! one file per category of [`super::CATEGORIES`]. Each file holds
//! made-course lessons that break that category's rule, `obvious` ones
//! (plain wording, a listed term where the category has one) and `subtle`
//! ones (no listed term, softened or indirect wording), and `fine`
//! look-alikes that must pass. Harmful text is described in placeholder
//! words, never written out.
//!
//! Nothing here runs in the server. The tests do three things:
//!
//! - `cargo test controls` (CI): every file is well formed, and the term
//!   check (`course_flags::terms`) flags each control that names a `term`
//!   with exactly that term, and nothing else. The calibration set's
//!   ordinary lessons pass it too.
//! - `scripts/content-check-controls.sh` (by hand, real keys): every
//!   control through the term check, Jev and the stage 2 review, the same
//!   code the server runs, then a scorecard per category.
//! - `scripts/moderation-redteam.sh` (by hand, real keys): Claude writes a
//!   lesson that tries to get past all three, and explains how when it does.
//!
//! Notes: `data/moderation/CONTROLS.md`. Not in `docs/`: that folder is
//! served at `/docs` to anyone, and the notes say where the filter is weak.

#[cfg(test)]
mod tests;
