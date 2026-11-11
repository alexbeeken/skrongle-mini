//! Per-account permissions (#137, #138): what an account may do, stored as
//! one integer (`users.perms`, one bit a permission), and the presets a set
//! can be given from (table `permission_presets`; Student and Teacher are
//! built in). Enforcement reads only `users.perms`, through
//! `CurrentUser::can` / `require` / `require_api` (`auth.rs`); an owner may
//! do everything whatever is stored.

use rusqlite::{params, Connection, OptionalExtension};

/// One thing an account may be allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perm {
    /// Make a new course of one's own: Create a course, Copy, Copy and translate.
    CreateCourses,
    /// Generate a course, Use AI / Add content, Edit with AI, Copy and translate.
    AiCourses,
    /// The AI tutor (`/tutor`).
    AiTutor,
    /// Make invitation links for one's own course.
    ShareCourses,
    /// Choose a starting level, change level, take the placement test.
    SkipLevels,
    /// The account's study progress and last sign-in are not shown on Manage Users.
    Privacy,
    /// See the accounts on Manage Users, set permissions and presets, see
    /// learners' progress. Not an owner.
    Superuser,
}

impl Perm {
    /// Every permission, in the order the screen lists them.
    pub const ALL: [Perm; 7] = [
        Perm::CreateCourses,
        Perm::AiCourses,
        Perm::AiTutor,
        Perm::ShareCourses,
        Perm::SkipLevels,
        Perm::Privacy,
        Perm::Superuser,
    ];

    /// Its bit in `users.perms`.
    pub fn bit(self) -> i64 {
        match self {
            Perm::CreateCourses => 1,
            Perm::AiCourses => 2,
            Perm::AiTutor => 4,
            Perm::ShareCourses => 8,
            Perm::SkipLevels => 16,
            Perm::Privacy => 32,
            Perm::Superuser => 64,
        }
    }

    /// Its name in code, form fields and test ids.
    pub fn id(self) -> &'static str {
        match self {
            Perm::CreateCourses => "create_courses",
            Perm::AiCourses => "ai_courses",
            Perm::AiTutor => "ai_tutor",
            Perm::ShareCourses => "share_courses",
            Perm::SkipLevels => "skip_levels",
            Perm::Privacy => "privacy",
            Perm::Superuser => "superuser",
        }
    }

    pub fn parse(id: &str) -> Option<Perm> {
        Perm::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The permission this one is no use without: with that one off, this
    /// one counts as off.
    pub fn needs(self) -> Option<Perm> {
        match self {
            Perm::AiCourses | Perm::ShareCourses => Some(Perm::CreateCourses),
            _ => None,
        }
    }

    /// Its name on screen (`perms.<id>`).
    pub fn label(self) -> String {
        crate::t(&format!("perms.{}", self.id()))
    }

    /// One sentence saying what it allows (`perms.<id>_about`).
    pub fn about(self) -> String {
        crate::t(&format!("perms.{}_about", self.id()))
    }
}

/// A set of permissions: the bits of `users.perms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Perms(i64);

impl Perms {
    /// The built-in Student preset: nothing.
    pub const STUDENT: Perms = Perms(0);
    /// The built-in Teacher preset: everything but `Superuser`.
    pub const TEACHER: Perms = Perms(63);
    pub const ALL: Perms = Perms(127);

    /// A stored set: unknown bits dropped, then normalised.
    pub fn from_bits(bits: i64) -> Perms {
        Perms(bits & Perms::ALL.0).normalized()
    }

    pub fn bits(self) -> i64 {
        self.0
    }

    /// Is `p` on? A permission whose needed permission is off is off.
    pub fn has(self, p: Perm) -> bool {
        self.0 & p.bit() != 0 && p.needs().is_none_or(|n| self.0 & n.bit() != 0)
    }

    /// The set with `p` turned on or off. Not normalised, so a form's
    /// toggles can be applied in any order; `has` and `normalized` are right
    /// either way.
    pub fn with(self, p: Perm, on: bool) -> Perms {
        Perms(if on { self.0 | p.bit() } else { self.0 & !p.bit() })
    }

    /// The set as it is stored: every permission whose `needs()` is off cleared.
    pub fn normalized(self) -> Perms {
        let mut bits = self.0;
        for p in Perm::ALL {
            if !self.has(p) {
                bits &= !p.bit();
            }
        }
        Perms(bits)
    }
}

/// `permission_presets.id` of the built-in Student preset.
#[cfg_attr(not(test), allow(dead_code))] // only the tests name it so far
pub const STUDENT_PRESET: i64 = 1;
/// `permission_presets.id` of the built-in Teacher preset.
pub const TEACHER_PRESET: i64 = 2;

/// A row of `permission_presets`. A built-in one is named `student` or
/// `teacher`; the screen translates those (`perms.student`, `perms.teacher`).
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub id: i64,
    pub name: String,
    pub builtin: bool,
    pub perms: Perms,
}

fn read_preset(r: &rusqlite::Row<'_>) -> rusqlite::Result<Preset> {
    Ok(Preset { id: r.get(0)?, name: r.get(1)?, builtin: r.get(2)?, perms: Perms::from_bits(r.get(3)?) })
}

/// Every preset: the built-in ones first, then the saved ones by name.
pub fn presets(conn: &Connection) -> rusqlite::Result<Vec<Preset>> {
    let mut stmt = conn.prepare("SELECT id, name, builtin, perms FROM permission_presets ORDER BY builtin DESC, CASE WHEN builtin THEN id END, name")?;
    let rows = stmt.query_map([], read_preset)?.collect();
    rows
}

pub fn preset(conn: &Connection, id: i64) -> rusqlite::Result<Option<Preset>> {
    conn.query_row("SELECT id, name, builtin, perms FROM permission_presets WHERE id = ?1", [id], read_preset).optional()
}

/// Give an account a set of its own (Custom): stored normalised, no preset.
pub fn set_custom(conn: &Connection, user_id: i64, perms: Perms) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET perms = ?1, preset_id = NULL WHERE id = ?2", params![perms.normalized().bits(), user_id])?;
    Ok(())
}

/// Give an account a preset: the preset's set is copied onto the account.
/// `false` when there is no such preset (nothing changes).
pub fn apply_preset(conn: &Connection, user_id: i64, preset_id: i64) -> rusqlite::Result<bool> {
    let Some(preset) = preset(conn, preset_id)? else {
        return Ok(false);
    };
    conn.execute("UPDATE users SET perms = ?1, preset_id = ?2 WHERE id = ?3", params![preset.perms.bits(), preset.id, user_id])?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(c: &Connection, user: i64) -> rusqlite::Result<(i64, Option<i64>)> {
        c.query_row("SELECT perms, preset_id FROM users WHERE id = ?1", [user], |r| Ok((r.get(0)?, r.get(1)?)))
    }

    #[test]
    fn bits_ids_and_sets_round_trip() {
        assert_eq!(Perm::ALL.map(Perm::bit), [1, 2, 4, 8, 16, 32, 64]);
        for p in Perm::ALL {
            assert_eq!(Perm::parse(p.id()), Some(p));
            assert_eq!(Perms::STUDENT.with(p, true).bits(), p.bit());
            assert!(!Perms::ALL.with(p, false).has(p));
        }
        assert_eq!(Perm::parse("owner"), None);
        for bits in [0, 1, 3, 9, 63, 64, 116, 127] {
            assert_eq!(Perms::from_bits(bits).bits(), bits);
        }
        assert_eq!(Perms::from_bits(127 | 128 | 1024), Perms::ALL, "unknown bits are dropped");
        assert_eq!(Perms::from_bits(-1), Perms::ALL);
        assert_eq!(Perms::ALL.bits(), Perm::ALL.iter().map(|p| p.bit()).sum::<i64>());
    }

    #[test]
    fn a_permission_is_off_when_the_one_it_needs_is_off() {
        assert_eq!(Perm::AiCourses.needs(), Some(Perm::CreateCourses));
        assert_eq!(Perm::ShareCourses.needs(), Some(Perm::CreateCourses));
        assert_eq!(Perm::ALL.into_iter().filter(|p| p.needs().is_some()).count(), 2);

        let no_create = Perms::ALL.with(Perm::CreateCourses, false);
        assert!(!no_create.has(Perm::AiCourses) && !no_create.has(Perm::ShareCourses), "off before it is normalised too");
        let n = no_create.normalized();
        assert_eq!(n.bits(), 4 + 16 + 32 + 64, "ai_courses and share_courses cleared, the others left");
        assert_eq!(n.normalized(), n);
        assert_eq!(Perms::from_bits(2 | 8 | 4), Perms::STUDENT.with(Perm::AiTutor, true));
        // Toggles in any order: the dependent one first still counts once its need is on.
        let set = Perms::STUDENT.with(Perm::AiCourses, true).with(Perm::CreateCourses, true);
        assert!(set.has(Perm::AiCourses));
        assert_eq!(set.normalized().bits(), 3);
    }

    #[test]
    fn teacher_has_the_first_six_and_student_none() {
        for p in Perm::ALL {
            assert_eq!(Perms::TEACHER.has(p), p != Perm::Superuser, "{p:?}");
            assert!(!Perms::STUDENT.has(p));
            assert!(Perms::ALL.has(p));
        }
        assert_eq!(Perms::TEACHER.normalized(), Perms::TEACHER);
    }

    /// Every name the screen will show is in the app text (and so, by
    /// `i18n`'s own test, in every language).
    #[test]
    fn every_permission_and_preset_has_its_text() {
        let en = &crate::i18n::languages()[0];
        for p in Perm::ALL {
            assert!(en.has(&format!("perms.{}", p.id())), "perms.{}", p.id());
            assert!(en.has(&format!("perms.{}_about", p.id())), "perms.{}_about", p.id());
            assert!(!p.label().is_empty() && !p.about().is_empty());
        }
        for key in ["perms.student", "perms.teacher", "perms.custom", "perms.owner", "error.no_permission"] {
            assert!(en.has(key), "{key}");
        }
    }

    #[test]
    fn the_built_in_presets_are_student_and_teacher() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            c.execute("INSERT INTO permission_presets (name, builtin, perms, created_at) VALUES ('Helpers', 0, 5, 1), ('Aides', 0, 127, 1)", [])?;
            let all = presets(c)?;
            assert_eq!(all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["student", "teacher", "Aides", "Helpers"]);
            assert_eq!(all[0], Preset { id: STUDENT_PRESET, name: "student".into(), builtin: true, perms: Perms::STUDENT });
            assert_eq!(all[1], Preset { id: TEACHER_PRESET, name: "teacher".into(), builtin: true, perms: Perms::TEACHER });
            assert!(!all[2].builtin && all[2].perms == Perms::ALL);
            assert_eq!(preset(c, TEACHER_PRESET)?, Some(all[1].clone()));
            assert_eq!(preset(c, 99)?, None);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_preset_is_copied_onto_the_account_and_a_custom_set_clears_it() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            assert_eq!(stored(c, ann)?, (63, Some(TEACHER_PRESET)));

            assert!(apply_preset(c, ann, STUDENT_PRESET)?);
            assert_eq!(stored(c, ann)?, (0, Some(STUDENT_PRESET)));
            assert!(!apply_preset(c, ann, 99)?, "no such preset");
            assert_eq!(stored(c, ann)?, (0, Some(STUDENT_PRESET)), "left as it was");

            // A copy: changing the preset afterwards does not change the account.
            c.execute("INSERT INTO permission_presets (id, name, perms, created_at) VALUES (7, 'Helpers', 5, 1)", [])?;
            assert!(apply_preset(c, ann, 7)?);
            c.execute("UPDATE permission_presets SET perms = 127 WHERE id = 7", [])?;
            assert_eq!(stored(c, ann)?, (5, Some(7)));
            // A deleted preset leaves its accounts their set, as Custom.
            c.execute("DELETE FROM permission_presets WHERE id = 7", [])?;
            assert_eq!(stored(c, ann)?, (5, None));

            assert!(apply_preset(c, ann, TEACHER_PRESET)?);
            set_custom(c, ann, Perms::ALL.with(Perm::CreateCourses, false))?;
            assert_eq!(stored(c, ann)?, (4 + 16 + 32 + 64, None), "stored normalised, preset cleared");
            assert_eq!(stored(c, bob)?, (63, Some(TEACHER_PRESET)), "another account is untouched");
            Ok(())
        })
        .unwrap();
    }
}
