use super::*;
use crate::db::Db;

fn copyright(course: Option<&str>) -> Report {
    Report {
        course_id: course.map(str::to_string),
        course_title: "Verbs".into(),
        location: "https://example.org/courses/verbs-abc123".into(),
        reason: "copyright".into(),
        email: "claimant@example.org".into(),
        name: "Cleo Claimant".into(),
        work: "My verb tables".into(),
        details: "Lesson 2 copies my tables.".into(),
        good_faith: true,
        created_at: 10,
        ..Report::default()
    }
}

/// A report is stored open, found by the words of a "where", forwarded
/// and resolved once.
#[test]
fn reports_are_stored_matched_and_resolved() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = crate::auth::create_user(c, "owner@example.org", "h").unwrap();
        c.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('verbs-abc123', ?1, 'Verbs', 1, 1)", [ann])?;
        assert_eq!(find_course(c, "see https://x.example/courses/verbs-abc123?a=1")?, Some(("verbs-abc123".into(), "Verbs".into())));
        assert_eq!(find_course(c, "somewhere else entirely")?, None);
        // The link in a share mail names the course by its token.
        c.execute("INSERT INTO course_shares (course_id, email, token_hash, created_at) VALUES ('verbs-abc123', 'bob@example.org', ?1, 1)", [crate::share::hash_token("f00dfeed")])?;
        assert_eq!(find_course(c, "https://x.example/courses/shared/f00dfeed")?, Some(("verbs-abc123".into(), "Verbs".into())));
        assert_eq!(owner(c, "verbs-abc123")?, Some((ann, "owner@example.org".into())));

        let id = insert(c, &copyright(Some("verbs-abc123")))?;
        let open_now = open(c)?;
        assert_eq!(open_now.len(), 1);
        assert!(open_now[0].good_faith && open_now[0].forwarded_at.is_none());
        mark_forwarded(c, id, 20)?;
        assert_eq!(get(c, id)?.unwrap().forwarded_at, Some(20));
        assert!(resolve(c, id, 30)?);
        assert!(!resolve(c, id, 31)?, "only once");
        assert!(open(c)?.is_empty());
        // Deleting the course keeps the record.
        c.execute("DELETE FROM courses WHERE id = 'verbs-abc123'", [])?;
        assert!(get(c, id)?.is_some());
        Ok(())
    })
    .unwrap();
}

/// Without `SKRONGLE_LEGAL_EMAIL` the superusers hear of a report.
#[test]
fn superusers_are_the_operator_without_a_legal_address() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        crate::auth::create_user(c, "root@example.org", "h").unwrap();
        crate::auth::create_user(c, "bob@example.org", "h").unwrap();
        c.execute("UPDATE users SET superuser = 1 WHERE email = 'root@example.org'", [])?;
        assert_eq!(operator_addresses(c)?, vec!["root@example.org".to_string()]);
        Ok(())
    })
    .unwrap();
}

/// The owner's copy carries the claimant's address on a copyright
/// notice and never on another report; the operator's always does.
#[test]
fn the_owner_sees_the_reporter_only_on_a_copyright_notice() {
    let notice = copyright(Some("verbs-abc123"));
    let fwd = forward_message("owner@example.org", 1, &notice).unwrap();
    assert_eq!(fwd.purpose, PURPOSE_FORWARD);
    assert!(fwd.text.contains("claimant@example.org") && fwd.text.contains("Cleo Claimant") && fwd.text.contains("Lesson 2 copies"), "{}", fwd.text);
    assert!(fwd.subject.contains("Verbs"));

    let harmful = Report { reason: "harmful".into(), name: String::new(), work: String::new(), location: String::new(), good_faith: false, ..notice.clone() };
    let fwd = forward_message("owner@example.org", 1, &harmful).unwrap();
    assert!(!fwd.text.contains("claimant@example.org"), "{}", fwd.text);
    assert!(fwd.text.contains("Lesson 2 copies"));

    let op = operator_message("legal@example.org", &harmful, "https://x.example/superuser#reports").unwrap();
    assert_eq!(op.purpose, PURPOSE);
    assert!(op.text.contains("claimant@example.org") && op.text.contains("https://x.example/superuser#reports"), "{}", op.text);
}
