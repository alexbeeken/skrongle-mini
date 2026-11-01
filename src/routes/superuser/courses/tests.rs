use super::WorkerView;
use crate::content_check::worker::{Status, SILENT};

/// The banner at the top of /superuser (#1041 review): only while the
/// worker is silent or never seen, with when it last checked in and how
/// many checks wait.
#[test]
fn the_checker_banner_shows_only_while_the_worker_is_silent() {
    let fine = WorkerView::new(&Status { seen_at: Some(1_000), waiting: 2 }, 1_000 + 60);
    assert!(!fine.silent && fine.alert.is_empty(), "no banner while it checks in");

    let silent = WorkerView::new(&Status { seen_at: Some(1_000), waiting: 3 }, 1_000 + SILENT);
    assert!(silent.silent);
    assert!(silent.alert.starts_with("The content checker hasn't checked in since ") && silent.alert.ends_with("00:16 UTC. Checks waiting: 3."), "{}", silent.alert);

    let never = WorkerView::new(&Status { seen_at: None, waiting: 0 }, 5);
    assert_eq!(never.alert, "The content checker has never checked in. Checks waiting: 0.");
}
