//! What a change touches: the packages whose files differ from the merge base
//! with `origin/main`, and every package that depends on one of them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::git;

/// A change to one of these changes how every crate builds.
const BUILDS_EVERY_CRATE: &[&str] = &["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];

pub struct Workspace {
    /// The package whose manifest is the workspace's own: the app.
    root_package: String,
    /// Every other member's directory, relative to the root with `/`, and its name.
    dirs: Vec<(String, String)>,
    /// Each member and the members it names as a dependency of any kind.
    deps: BTreeMap<String, BTreeSet<String>>,
}

impl Workspace {
    pub fn read(root: &Path) -> Result<Self> {
        let out = Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(root)
            .output()
            .context("could not run `cargo metadata`")?;
        let metadata: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        let workspace_root = metadata["workspace_root"]
            .as_str()
            .context("cargo metadata named no workspace root")?
            .replace('\\', "/");
        let mut root_package = String::new();
        let mut dirs = Vec::new();
        let mut deps = BTreeMap::new();
        for package in metadata["packages"].as_array().into_iter().flatten() {
            let name = package["name"].as_str().unwrap_or_default().to_string();
            let manifest = package["manifest_path"]
                .as_str()
                .unwrap_or_default()
                .replace('\\', "/");
            let dir = manifest
                .trim_end_matches("Cargo.toml")
                .trim_end_matches('/')
                .strip_prefix(workspace_root.as_str())
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_string();
            if dir.is_empty() {
                root_package.clone_from(&name);
            } else {
                dirs.push((dir, name.clone()));
            }
            let named = package["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d["name"].as_str().map(str::to_string))
                .collect();
            deps.insert(name, named);
        }
        Ok(Self {
            root_package,
            dirs,
            deps,
        })
    }

    fn members(&self) -> usize {
        self.deps.len()
    }

    /// The member whose directory holds `file`; the app for a file in none.
    pub fn package_of(&self, file: &str) -> &str {
        self.dirs
            .iter()
            .filter(|(dir, _)| {
                file.strip_prefix(dir.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|(dir, _)| dir.len())
            .map_or(self.root_package.as_str(), |(_, name)| name)
    }

    /// The packages `files` belong to and every member depending on one; `None` when a file builds every crate.
    ///
    /// ⚠ A file in no crate (a document, the justfile, a workflow) belongs to the
    /// app, which holds the document gates, and to each package `mentions`
    /// finds naming it: the tests that read a file name it.
    fn affected(
        &self,
        files: &[String],
        mentions: impl Fn(&str) -> Vec<String>,
    ) -> Option<BTreeSet<String>> {
        let mut touched = BTreeSet::new();
        for file in files {
            if BUILDS_EVERY_CRATE.contains(&file.as_str()) || file.starts_with(".cargo/") {
                return None;
            }
            let package = self.package_of(file);
            touched.insert(package.to_string());
            let in_the_app_itself = file.starts_with("src/") || file == "build.rs";
            if package == self.root_package && !in_the_app_itself {
                let name = file.rsplit('/').next().unwrap_or(file);
                for source in mentions(name) {
                    touched.insert(self.package_of(&source).to_string());
                }
            }
        }
        let mut affected = touched;
        loop {
            let more: Vec<String> = self
                .deps
                .iter()
                .filter(|(name, deps)| {
                    !affected.contains(*name) && deps.iter().any(|d| affected.contains(d))
                })
                .map(|(name, _)| name.clone())
                .collect();
            if more.is_empty() {
                return Some(affected);
            }
            affected.extend(more);
        }
    }
}

/// What a run covers.
pub enum Scope {
    /// Nothing differs.
    Nothing,
    Workspace,
    Packages(BTreeSet<String>),
}

/// The packages the tree at `root` touches since its merge base with `since`; the workspace with no merge base.
pub fn scope(root: &Path, since: &str) -> Result<Scope> {
    let Some(files) = changed_files(root, since)? else {
        println!("no merge base with {since}, so the whole workspace is checked");
        return Ok(Scope::Workspace);
    };
    if files.is_empty() {
        return Ok(Scope::Nothing);
    }
    let workspace = Workspace::read(root)?;
    Ok(
        match workspace.affected(&files, |name| sources_naming(root, name)) {
            Some(packages) if packages.len() < workspace.members() => Scope::Packages(packages),
            _ => Scope::Workspace,
        },
    )
}

/// The files that differ from the merge base with `since`, untracked ones included; `None` with no merge base.
fn changed_files(root: &Path, since: &str) -> Result<Option<Vec<String>>> {
    let base = git::output(root, &["merge-base", "HEAD", since])?;
    let base = String::from_utf8_lossy(&base.stdout).trim().to_string();
    if base.is_empty() {
        return Ok(None);
    }
    let mut changed = git::lines(&git::run(root, &["diff", "--name-only", &base])?);
    changed.extend(git::lines(&git::run(
        root,
        &["ls-files", "--others", "--exclude-standard"],
    )?));
    Ok(Some(changed))
}

/// The tracked Rust sources that name `file_name`.
fn sources_naming(root: &Path, file_name: &str) -> Vec<String> {
    // ⚠ `git grep` exits 1 on no match, which is an answer and not a failure.
    git::output(root, &["grep", "-l", "-F", "-e", file_name, "--", "*.rs"])
        .map(|out| git::lines(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Workspace {
        let deps = [
            ("or-io", vec![]),
            ("or-anim", vec!["or-io"]),
            ("or-wardrobe", vec!["or-io"]),
            ("xtask", vec![]),
            ("openrage", vec!["or-io", "or-anim", "or-wardrobe"]),
        ];
        Workspace {
            root_package: "openrage".to_string(),
            dirs: vec![
                ("crates/io/or-io".to_string(), "or-io".to_string()),
                ("crates/runtime/or-anim".to_string(), "or-anim".to_string()),
                (
                    "crates/domain/or-wardrobe".to_string(),
                    "or-wardrobe".to_string(),
                ),
                ("crates/tools/xtask".to_string(), "xtask".to_string()),
            ],
            deps: deps
                .into_iter()
                .map(|(name, on)| {
                    (
                        name.to_string(),
                        on.into_iter().map(str::to_string).collect(),
                    )
                })
                .collect(),
        }
    }

    fn names(set: Option<BTreeSet<String>>) -> Vec<String> {
        set.expect("a set of packages").into_iter().collect()
    }

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    #[test]
    fn a_leaf_crate_reaches_only_itself_and_what_depends_on_it() {
        let ws = workspace();
        let hit = ws.affected(&files(&["crates/runtime/or-anim/src/pose.rs"]), |_| vec![]);
        assert_eq!(names(hit), ["openrage", "or-anim"]);
        let hit = ws.affected(&files(&["crates/tools/xtask/src/main.rs"]), |_| vec![]);
        assert_eq!(names(hit), ["xtask"]);
    }

    #[test]
    fn the_lowest_crate_reaches_everything_that_builds_on_it() {
        let ws = workspace();
        let hit = ws.affected(&files(&["crates/io/or-io/src/lib.rs"]), |_| vec![]);
        assert_eq!(names(hit), ["openrage", "or-anim", "or-io", "or-wardrobe"]);
    }

    #[test]
    fn a_document_reaches_the_app_and_the_crates_whose_sources_name_it() {
        let ws = workspace();
        let hit = ws.affected(&files(&["docs/guides/Wardrobe.md"]), |name| {
            assert_eq!(name, "Wardrobe.md");
            vec!["crates/domain/or-wardrobe/tests/the_guide.rs".to_string()]
        });
        assert_eq!(names(hit), ["openrage", "or-wardrobe"]);
    }

    #[test]
    fn a_source_of_the_app_asks_nothing_of_the_other_crates() {
        let ws = workspace();
        let hit = ws.affected(&files(&["src/state/kinds.rs"]), |_| {
            panic!("a compiled source is not looked up by name")
        });
        assert_eq!(names(hit), ["openrage"]);
    }

    #[test]
    fn the_manifest_the_lock_and_the_toolchain_reach_every_crate() {
        let ws = workspace();
        for file in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".cargo/config.toml",
        ] {
            assert!(ws.affected(&files(&[file]), |_| vec![]).is_none(), "{file}");
        }
        // A member's own manifest is that member's.
        let hit = ws.affected(&files(&["crates/runtime/or-anim/Cargo.toml"]), |_| vec![]);
        assert_eq!(names(hit), ["openrage", "or-anim"]);
    }
}
