//! The window that edits a pack holds its lock; a second window gets the answer that another window edits it, and gets the
//! pack once the first lets go. A folder the lock cannot be made in gives a lock that guards nothing and no refusal.

use or_wardrobe::lock::acquire;
use or_wardrobe::Wardrobe;

#[test]
fn a_second_window_cannot_take_a_pack_the_first_holds_and_can_once_it_lets_go() {
    let dir = tempfile::tempdir().unwrap();
    Wardrobe::new("pack").write(dir.path()).unwrap();

    let first = acquire(dir.path()).expect("nobody holds it");
    assert!(first.is_held());
    let refused = acquire(dir.path()).expect_err("the first window holds it");
    assert!(
        refused.to_string().contains("another OpenRage window"),
        "{refused}"
    );
    // Reading is never refused.
    assert!(Wardrobe::read(dir.path()).is_ok());

    drop(first);
    acquire(dir.path()).expect("the lock is the first window's no longer");
}

#[test]
fn a_pack_in_a_folder_that_cannot_hold_a_lock_file_is_not_refused() {
    let dir = tempfile::tempdir().unwrap();
    // A file where the private folder would be: the lock file cannot be made, so there is nothing to guard with.
    std::fs::write(dir.path().join(".openrage"), b"in the way").unwrap();
    let lock = acquire(dir.path()).expect("an edit is not refused for a lock that cannot be had");
    assert!(!lock.is_held());
}
