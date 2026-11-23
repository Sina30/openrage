//! A self-check that stops before its end removes the folder it made for its preferences.

use std::path::Path;

use super::{run_removing, Options};

/// What the first frames of a `--self-check` process leave in its scratch folder.
fn make(scratch: &Path) {
    std::fs::create_dir_all(scratch).expect("the scratch folder");
    std::fs::write(scratch.join("prefs.json"), b"{}").expect("its preferences");
}

#[test]
fn a_self_check_refused_at_its_first_check_removes_its_scratch_folder() {
    let outer = tempfile::tempdir().expect("a temporary folder");
    let scratch = outer.path().join("openrage-self-check-1");
    make(&scratch);
    let refused = Options {
        shots: Some(Default::default()),
        ..Default::default()
    };

    assert_eq!(run_removing(refused, Some(scratch.clone())), 1);

    assert!(
        !scratch.exists(),
        "a run refused at its first check left {}",
        scratch.display()
    );
}
