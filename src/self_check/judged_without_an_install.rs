//! A self-check passes on a machine that found no GTA V install, which is every CI runner and every Mac.

use super::{run_removing, Options};
use crate::state::test_scratch::run_with_a_temporary_folder_of_its_own;

/// Set for the process that runs the self-check the next test starts a process for.
const COMPLETES_HERE: &str = "OPENRAGE_TEST_SELF_CHECK_COMPLETES_HERE";

/// The self-check the next test starts a process for; with the variable unset it runs nothing.
///
/// ⚠ A process of its own: a self-check replaces the panic hook and registers panels for good, and
/// nothing seeds its scratch folder with a `prefs.json`, so the run is judged on what it wrote.
#[test]
fn a_self_check_that_runs_to_the_end_is_run_for_the_next_test() {
    if std::env::var_os(COMPLETES_HERE).is_none() {
        return;
    }
    assert_eq!(run_removing(Options::default(), None), 0);
}

#[test]
fn a_self_check_that_finds_no_install_still_writes_its_preferences_and_passes() {
    let module = module_path!().split_once("::").map_or("", |(_, rest)| rest);
    let tests = [
        format!("{module}::a_self_check_that_runs_to_the_end_is_run_for_the_next_test"),
        "--nocapture".to_string(),
    ];

    let (ran, _left) = run_with_a_temporary_folder_of_its_own(&tests, &[(COMPLETES_HERE, "1")]);

    let said = String::from_utf8_lossy(&ran.stdout);
    assert!(
        ran.status.success() && said.contains("0 failed") && said.contains("prefs.json written: yes"),
        "the self-check did not pass, or wrote no preferences, so a machine with no GTA V fails it: {said}"
    );
}
