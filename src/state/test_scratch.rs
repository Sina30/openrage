//! The scratch configuration a test process works in, and its removal when the process ends.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::OnceLock;

use super::config_root;

static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Where `config_root` is in a test build: one folder for the process, removed when it ends.
///
/// ⚠ Not a `TempDir` held in a static: a static is never dropped, so that left its folder behind.
pub(super) fn root() -> Option<PathBuf> {
    ROOT.get_or_init(|| {
        let root = tempfile::Builder::new()
            .prefix("openrage-test-")
            .tempdir()
            .ok()?
            .keep();
        extern "C" {
            fn atexit(callback: extern "C" fn()) -> i32;
        }
        extern "C" fn remove_at_exit() {
            remove();
        }
        // SAFETY: `atexit` takes a plain function and runs it once, as the process exits.
        unsafe { atexit(remove_at_exit) };
        Some(root)
    })
    .clone()
}

fn remove() {
    if let Some(Some(root)) = ROOT.get() {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// ⚠ Windows' `std::process::exit` is `ExitProcess`, which skips the C runtime's `atexit` handlers, and
/// libtest calls it once a test has failed. The loader calls an executable's TLS callbacks with
/// `DLL_PROCESS_DETACH` however the process ends, as std's own thread-local destructors rely on, but
/// not when a worker dies holding the loader's lock, so a process that returns from `main` is cleaned
/// by `atexit`, which runs before `ExitProcess`.
#[cfg(windows)]
#[link_section = ".CRT$XLB"]
#[used]
static REMOVE_AT_EXIT: unsafe extern "system" fn(
    *mut core::ffi::c_void,
    u32,
    *mut core::ffi::c_void,
) = remove_at_detach;

#[cfg(windows)]
unsafe extern "system" fn remove_at_detach(
    _: *mut core::ffi::c_void,
    reason: u32,
    _: *mut core::ffi::c_void,
) {
    const DLL_PROCESS_DETACH: u32 = 0;
    if reason == DLL_PROCESS_DETACH {
        remove();
    }
}

/// Set for the child of the next test that has to fail, as a failing test does.
const FAILS: &str = "OPENRAGE_TEST_SCRATCH_FAILS";

/// The test the next one starts a process for: any test that reads the configuration root makes the scratch.
#[test]
fn the_scratch_configuration_is_a_folder_of_the_temporary_folder() {
    let root = config_root().expect("the scratch configuration");
    assert!(
        root.is_dir() && root.starts_with(std::env::temp_dir()),
        "{} is not a folder under {}",
        root.display(),
        std::env::temp_dir().display()
    );
    assert!(
        std::env::var_os(FAILS).is_none(),
        "failing on purpose, with the scratch made"
    );
    #[cfg(windows)]
    if std::env::var_os(LOADING).is_some() {
        keep_the_loader_busy();
    }
}

/// Set for the child whose worker is loading libraries as the process ends.
#[cfg(windows)]
const LOADING: &str = "OPENRAGE_TEST_SCRATCH_LOADING";

/// A worker that is in the loader whenever the process ends, as a game-discovery thread can be.
#[cfg(windows)]
fn keep_the_loader_busy() {
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    std::thread::spawn(|| {
        let name: Vec<u16> = "version.dll\0".encode_utf16().collect();
        loop {
            // SAFETY: `name` is NUL-terminated, and the module is released at once.
            unsafe {
                let module = LoadLibraryW(name.as_ptr());
                if !module.is_null() {
                    FreeLibrary(module);
                }
            }
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
}

/// `tests` run in a process of their own with a temporary folder of its own and `env` set, and what that folder holds after.
pub(crate) fn run_with_a_temporary_folder_of_its_own(
    tests: &[String],
    env: &[(&str, &str)],
) -> (Output, Vec<OsString>) {
    let temp = tempfile::tempdir().expect("a temporary folder for the process to use");
    let mut process = Command::new(std::env::current_exe().expect("this test binary"));
    process
        .arg("--exact")
        .args(tests)
        .env("TMP", temp.path())
        .env("TEMP", temp.path())
        .env("TMPDIR", temp.path())
        .envs(env.iter().copied());
    let ran = process.output().expect("the test binary starts");
    let left = std::fs::read_dir(temp.path())
        .expect("the temporary folder")
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    (ran, left)
}

/// ⚠ A static is never dropped, and nextest starts a process per test, so each test left a folder in
/// the owner's `%TEMP%`. The second test builds a wasm guest, which once went to a folder of its own there.
#[test]
fn a_test_process_leaves_no_folder_in_the_temporary_folder_when_it_ends() {
    let module = module_path!().split_once("::").map_or("", |(_, rest)| rest);
    let tests = [
        format!("{module}::the_scratch_configuration_is_a_folder_of_the_temporary_folder"),
        "state::a_plugin_asks_before_it_runs::the_sample_guest_shows_what_it_declared_before_it_runs"
            .to_string(),
    ];
    let (ran, left) = run_with_a_temporary_folder_of_its_own(&tests, &[]);
    let said = String::from_utf8_lossy(&ran.stdout);
    assert!(
        ran.status.success() && said.contains("2 passed"),
        "the process did not run its two tests: {said}"
    );
    assert!(
        left.is_empty(),
        "the process left {left:?} in its temporary folder"
    );
}

/// ⚠ A process whose worker holds the loader's lock as it ends never gets its `DLL_PROCESS_DETACH`, so
/// the removal that waits for it is skipped: this was the folder a test left in one of about 1,500.
#[cfg(windows)]
#[test]
fn a_test_process_that_ends_while_a_worker_loads_libraries_leaves_no_folder_in_the_temporary_folder(
) {
    let module = module_path!().split_once("::").map_or("", |(_, rest)| rest);
    let tests = [format!(
        "{module}::the_scratch_configuration_is_a_folder_of_the_temporary_folder"
    )];
    for run in 0..8 {
        let (ran, left) = run_with_a_temporary_folder_of_its_own(&tests, &[(LOADING, "1")]);
        let said = String::from_utf8_lossy(&ran.stdout);
        assert!(
            ran.status.success() && said.contains("1 passed"),
            "run {run} did not run its test: {said}"
        );
        assert!(
            left.is_empty(),
            "run {run} left {left:?} in its temporary folder"
        );
    }
}
