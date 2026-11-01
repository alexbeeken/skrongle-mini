use super::*;
use crate::db::Db;

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn a_reason_is_trimmed_cut_and_required() {
    assert_eq!(reason("  "), None);
    assert_eq!(reason(" privacy request, log 12 ").as_deref(), Some("privacy request, log 12"));
    assert_eq!(reason(&"é".repeat(300)).map(|r| r.chars().count()), Some(MAX_REASON));
}

/// The log keeps who, what, why and when, newest first; a deleted
/// superuser's rows stay without the name.
#[test]
fn the_log_outlives_the_superuser() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let su = crate::auth::create_user(c, "boss@example.org", "x")?;
        record(c, su, Action::Export, "request 1", 10)?;
        record(c, su, Action::Forget, "request 2", 20)?;
        let rows = recent(c)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], LogRow { created_at: 20, superuser: "boss@example.org".into(), action: "forget".into(), reason: "request 2".into() });
        crate::privacy::delete_account(c, su)?;
        let rows = recent(c)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].superuser, "");
        assert_eq!(rows[1].reason, "request 1");
        Ok(())
    })
    .unwrap();
}

/// Forgetting an address removes its invitations from every course and the
/// mails to it, and nothing else; an address with an account is refused.
#[test]
fn forgetting_an_address_removes_its_invitations_and_mails() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let owner = crate::auth::create_user(c, "owner@example.org", "x")?;
        c.execute_batch(&format!(
            "INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('c1', {owner}, 'T', 1, 1), ('c2', {owner}, 'T', 1, 1);"
        ))?;
        for (course, email) in [("c1", "gone@example.org"), ("c2", "gone@example.org"), ("c1", "stay@example.org")] {
            crate::share::invite(c, course, email, 1)?.unwrap();
        }
        for to in ["Gone@Example.org", "stay@example.org"] {
            c.execute(
                "INSERT INTO mail_outbox (user_id, to_addr, subject, body_text, purpose, next_attempt_at, created_at) VALUES (?1, ?2, 's', 'b', 'course-share', 1, 1)",
                params![owner, to],
            )?;
        }
        assert_eq!(forget(c, " GONE@example.org ")?, Forgot::Done { shares: 2, mails: 1 });
        assert_eq!(count(c, "SELECT COUNT(*) FROM course_shares"), 1);
        assert_eq!(count(c, "SELECT COUNT(*) FROM mail_outbox WHERE to_addr = 'stay@example.org'"), 1);
        assert_eq!(forget(c, "gone@example.org")?, Forgot::Done { shares: 0, mails: 0 });
        assert_eq!(forget(c, "Owner@example.org")?, Forgot::HasAccount);
        assert_eq!(count(c, "SELECT COUNT(*) FROM courses"), 2);
        Ok(())
    })
    .unwrap();
}
