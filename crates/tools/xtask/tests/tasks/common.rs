//! A scratch directory and a way to run the built `xtask` in it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A folder that is removed when it drops, a failed test's too. `.0` is its
/// canonical path: a temporary folder is a symlink on some systems, and the
/// paths a tool prints are the resolved ones.
pub struct Scratch(pub PathBuf, #[allow(dead_code)] tempfile::TempDir);

impl Scratch {
    pub fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("xtask-{label}-"))
            .tempdir()
            .expect("a scratch directory");
        let path = std::fs::canonicalize(dir.path()).expect("the scratch directory exists");
        // ⚠ Windows resolves to a `\\?\` path, which `git worktree add` refuses as a location.
        let path = match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
            Some(plain) => PathBuf::from(plain),
            None => path,
        };
        Scratch(path, dir)
    }

    pub fn write(&self, name: &str, bytes: &[u8]) {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(path, bytes).expect("a scratch file");
    }

    pub fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(&self.0)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository with an identity, so a commit can be made in it.
    pub fn repository(label: &str) -> Self {
        let scratch = Scratch::new(label);
        scratch.git(&["init", "-q"]);
        scratch.git(&["config", "user.email", "xtask@example.invalid"]);
        scratch.git(&["config", "user.name", "xtask"]);
        scratch
    }

    /// `xtask <args>` run in this directory.
    pub fn xtask(&self, args: &[&str]) -> Output {
        run_in(&self.0, args)
    }
}

pub fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the xtask binary runs")
}

pub fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
