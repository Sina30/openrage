use std::path::{Path, PathBuf};

fn pinned_channel(root: &Path) -> String {
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .expect("rust-toolchain.toml should be at the workspace root");

    for line in text.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("channel") else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('=') else {
            continue;
        };
        return value.trim().trim_matches('"').to_string();
    }
    panic!("rust-toolchain.toml declares no channel:\n{text}");
}

fn declared_toolchains(path: &Path) -> Vec<(usize, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let rest = line.trim().strip_prefix("toolchain:")?;
            Some((i + 1, rest.trim().trim_matches('"').to_string()))
        })
        .collect()
}

fn workflows(root: &Path) -> Vec<PathBuf> {
    let dir = root.join(".github/workflows");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("no workflows at {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "yml" || e == "yaml"))
        .collect();
    out.sort();
    out
}

fn root() -> PathBuf {
    or_test_support::workspace_root(env!("CARGO_MANIFEST_DIR")).expect("a workspace above us")
}

#[test]
fn every_workflow_builds_on_the_pinned_toolchain() {
    let root = root();
    let pinned = pinned_channel(&root);

    let mut seen = 0;
    for workflow in workflows(&root) {
        for (line, declared) in declared_toolchains(&workflow) {
            seen += 1;
            assert_eq!(
                declared,
                pinned,
                "{}:{line} builds on {declared}, but rust-toolchain.toml pins {pinned}.\n\
                 A job on a different compiler than a developer's produces a red build \
                 nobody can reproduce locally.",
                workflow.display()
            );
        }
    }

    assert!(
        seen >= 3,
        "expected every job to name its toolchain, found {seen}. \
         A job using `@stable` declares nothing and this test cannot see it."
    );
}

#[test]
fn no_workflow_installs_stable_instead_of_the_pin() {
    let root = root();
    let mut offenders = Vec::new();

    for workflow in workflows(&root) {
        let text = std::fs::read_to_string(&workflow).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            if line.contains("rust-toolchain@stable") || line.contains("rust-toolchain@beta") {
                offenders.push(format!("{}:{}", workflow.display(), i + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these install a floating toolchain rather than the pin in \
         rust-toolchain.toml:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn agents_md_states_the_pin_it_is_built_with() {
    let root = root();
    let pinned = pinned_channel(&root);

    let text = std::fs::read_to_string(root.join("AGENTS.md")).expect("AGENTS.md at the root");

    assert!(
        text.contains(&pinned),
        "AGENTS.md does not mention the pinned toolchain {pinned}. \
         If the pin moved, the sentence naming it has to move too."
    );
}
