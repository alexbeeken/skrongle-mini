use super::*;
use crate::db::Db;

#[test]
fn a_note_saves_replaces_and_empties_away() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let uid = crate::auth::create_user(c, "notes@example.org", "x").unwrap();
        assert_eq!(load(c, uid, "spanish", "a1").unwrap(), "");
        save(c, uid, "spanish", "a1", "ser = DOCTOR", 1).unwrap();
        save(c, uid, "spanish", "a2", "other lesson", 1).unwrap();
        save(c, uid, "spanish", "a1", "ser = DOCTOR, estar = PLACE", 2).unwrap();
        assert_eq!(load(c, uid, "spanish", "a1").unwrap(), "ser = DOCTOR, estar = PLACE");
        assert_eq!(load(c, uid, "french", "a1").unwrap(), "", "per course");
        save(c, uid, "spanish", "a1", "  \n", 3).unwrap();
        assert_eq!(load(c, uid, "spanish", "a1").unwrap(), "");
        let n: i64 = c.query_row("SELECT COUNT(*) FROM lesson_notes WHERE user_id = ?1", [uid], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "the emptied note's row is gone");
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_long_note_or_control_characters_are_refused() {
    assert!(refusal("line one\nline two\ttab").is_none());
    assert!(refusal(&"é".repeat(MAX_CHARS)).is_none());
    assert!(refusal(&"é".repeat(MAX_CHARS + 1)).is_some());
    assert!(refusal("bell\u{7}").is_some());
}
