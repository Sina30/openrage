//! The installed hook runs the built binary, refuses a stale or missing one
//! rather than skipping checks, and checks the commit and not the disk.

use std::path::PathBuf;
use std::process::{Command, Output};

use crate::common::{text, Scratch};

fn shim() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.githooks/pre-commit");
    std::fs::read(path).expect("the repository's hook template")
}

/// A repository shaped like this one as far as the hook can tell: a workspace
/// with one formatted crate, the hook template, and a tracked `xtask` directory.
fn repository(label: &str) -> Scratch {
    let repo = Scratch::repository(label);
    repo.write(
        "Cargo.toml",
        b"[package]\nname = \"p\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    repo.write("src/lib.rs", b"pub fn f() -> u32 {\n    1\n}\n");
    repo.write(".githooks/pre-commit", &shim());
    repo.write("crates/tools/xtask/README.md", b"the tasks\n");
    repo.git(&["add", "-A"]);
    repo
}

fn commit(repo: &Scratch, message: &str) -> Output {
    Command::new("git")
        .current_dir(&repo.0)
        .args(["commit", "-q", "-m", message])
        .output()
        .expect("git runs")
}

#[test]
fn a_commit_without_an_installed_hook_binary_is_refused_and_says_how_to_install_it() {
    let repo = repository("hook-missing");
    // Only the shim, as a clone that never ran the installer would have it.
    let hooks = repo.0.join(".git/hooks");
    std::fs::write(hooks.join("pre-commit"), shim()).expect("the shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            hooks.join("pre-commit"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("executable");
    }
    let out = commit(&repo, "first");
    assert!(
        !out.status.success(),
        "a commit went through with no checks installed"
    );
    assert!(text(&out).contains("just install-hooks"), "{}", text(&out));
}

#[test]
fn an_installed_hook_passes_a_clean_commit_and_stops_a_staged_bom() {
    let repo = repository("hook-installed");
    assert!(repo.xtask(&["install-hooks"]).status.success());

    let out = commit(&repo, "first");
    assert!(
        out.status.success(),
        "a clean commit was refused: {}",
        text(&out)
    );
    assert!(text(&out).contains("pre-commit: ok"), "{}", text(&out));

    repo.write("bad.md", &[0xEF, 0xBB, 0xBF, b'x']);
    repo.git(&["add", "bad.md"]);
    let out = commit(&repo, "second");
    assert!(!out.status.success(), "a staged BOM was committed");
    assert!(text(&out).contains("bad.md"), "{}", text(&out));
}

#[test]
fn the_hook_refuses_while_anything_is_unstaged() {
    let repo = repository("hook-unstaged");
    assert!(repo.xtask(&["install-hooks"]).status.success());
    assert!(commit(&repo, "first").status.success());

    repo.write("src/lib.rs", b"pub fn f() -> u32 {\n    2\n}\n");
    let out = commit(&repo, "second");
    assert!(!out.status.success());
    assert!(text(&out).contains("src/lib.rs"), "{}", text(&out));
}

#[test]
fn a_change_to_the_tasks_makes_the_installed_copy_stale() {
    let repo = repository("hook-stale");
    assert!(repo.xtask(&["install-hooks"]).status.success());
    assert!(commit(&repo, "first").status.success());

    repo.write("crates/tools/xtask/README.md", b"the tasks, changed\n");
    repo.git(&["add", "-A"]);
    let out = commit(&repo, "second");
    assert!(!out.status.success(), "a stale hook went on checking");
    assert!(text(&out).contains("just install-hooks"), "{}", text(&out));

    // Installed again, it is the current one.
    assert!(repo.xtask(&["install-hooks"]).status.success());
    let out = commit(&repo, "second");
    assert!(out.status.success(), "{}", text(&out));
}
