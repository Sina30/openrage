//! No place decides what a task screen shows by naming its kind of content or its workspace: the table of task screens holds both, and a place that needs either reads the row.
//!
//! The sources of the app are read as text, without their tests. A file that names the clothing content to read or write it with the Wardrobe's engine is listed in `ENGINE`
//! with the reason, and the list shrinks when a file stops naming it, as `DECIDED` of `src/shell_budget.rs` does.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const TABLE: &str = "ui/task_screens.rs";

/// The files that name the clothing content to read or write it with the Wardrobe's engine and not to decide what a screen shows, with why.
const ENGINE: &[(&str, &str)] = &[
    (
        "state/workbench/wardrobe.rs",
        "reads, scans and plans every clothing content with `or_wardrobe`, and the Wardrobe's own selection",
    ),
    (
        "state/workbench/builds.rs",
        "reads the files of a build back against the pack that planned them",
    ),
    (
        "state/workbench/output_sizes.rs",
        "plans a build's own options for each pack to weigh what it writes",
    ),
    (
        "state/workbench/build_edits.rs",
        "leaves the packs out of the matrix of locked cells, since a pack is laid out for the kind of build it ships in",
    ),
    (
        "ui/panels/project/build_options.rs",
        "the options of a build that ships a clothing pack are the Wardrobe engine's",
    ),
    (
        "state/os_drop.rs",
        "a drop of clothes on a project with no pack asks whether to make one",
    ),
    (
        "state/plugins/extensions.rs",
        "reads the wardrobe of each clothing content a build takes, for a validator plugin to read as data",
    ),
    (
        "self_check/front_door.rs",
        "a fixture: a project file with linked clothing, drawn by the self-check",
    ),
];

/// The files that name the weapons content to read or write it with the weapons engine and not to decide what a screen shows, with why.
const WEAPONS_ENGINE: &[(&str, &str)] = &[
    (
        "state/workbench/weapons.rs",
        "reads, scans and plans every weapons content with `or_weapons`, and the Weapons screen's own selection",
    ),
    (
        "state/workbench/imports.rs",
        "hands the end of an import into a pack to the content that started it",
    ),
    (
        "state/workbench/pack_import.rs",
        "runs the import job of each content that keeps its sources in a folder of its own with that content's engine",
    ),
];

/// The files that name the particles content to read or write it with the particles engine and not to decide what a screen shows, with why.
const PARTICLES_ENGINE: &[(&str, &str)] = &[
    (
        "state/workbench/particles.rs",
        "reads, scans and plans every particles content with `or_particles`, and the Effects panel's own selection",
    ),
    (
        "state/workbench/imports.rs",
        "hands the end of an import into a pack to the content that started it",
    ),
    (
        "state/workbench/pack_import.rs",
        "runs the import job of each content that keeps its sources in a folder of its own with that content's engine",
    ),
];

/// The files that name the maps content to read or write it with the maps engine and not to decide what a screen shows, with why.
const MAPS_ENGINE: &[(&str, &str)] = &[
    (
        "state/workbench/maps.rs",
        "reads, scans and plans every maps content with `or_maps`, and the Map screen's own selection",
    ),
    (
        "state/workbench/maps_edit.rs",
        "writes the map files and type files of a maps content with `or_maps`, and the Undo of those writes",
    ),
    (
        "state/workbench/imports.rs",
        "hands the end of an import into a pack to the content that started it",
    ),
    (
        "state/workbench/pack_import.rs",
        "runs the import job of each content that keeps its sources in a folder of its own with that content's engine",
    ),
];

/// The places that once decided what a task screen shows by its workspace, which read the table now.
const READ_THE_TABLE: &[&str] = &[
    "state/workbench/strip.rs",
    "state/workbench/tally.rs",
    "state/front_door/cards.rs",
    "state/front_door/new_project.rs",
    "state/mod.rs",
    "state/project_io.rs",
    "ui/panels/empty_state.rs",
    "ui/panels/start_page.rs",
    "ui/panels/menu.rs",
    "ui/panels/history.rs",
    "ui/panels/status_bar.rs",
    "ui/panels/project/content.rs",
    "ui/panels/project/mod.rs",
    "ui/panels/project/assets.rs",
    "ui/commands/mod.rs",
    "ui/dock_layout.rs",
];

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            into.push(path);
        }
    }
}

/// The names of the modules a `#[cfg(test)]` declares, which are files of tests.
fn test_modules(texts: &[(String, String)]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (_, text) in texts {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        for pair in lines.windows(2) {
            if pair[0] != "#[cfg(test)]" {
                continue;
            }
            let declaration = pair[1].strip_prefix("pub(crate) ").unwrap_or(pair[1]);
            let declaration = declaration.strip_prefix("pub ").unwrap_or(declaration);
            if let Some(name) = declaration
                .strip_prefix("mod ")
                .and_then(|rest| rest.strip_suffix(';'))
            {
                found.insert(name.to_string());
            }
        }
    }
    found
}

/// `text` without the items a `#[cfg(test)]` marks: a module declared in another file, or a block that ends at the line that closes it at its own indent.
fn without_tests(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if lines[at].trim() != "#[cfg(test)]" {
            kept.push(lines[at]);
            at += 1;
            continue;
        }
        let Some(item) = lines.get(at + 1) else {
            break;
        };
        if item.trim_end().ends_with(';') {
            at += 2;
            continue;
        }
        let indent = item.len() - item.trim_start().len();
        let closing = format!("{}}}", " ".repeat(indent));
        at += 2;
        while at < lines.len() && lines[at].trim_end() != closing {
            at += 1;
        }
        at += 1;
    }
    kept.join("\n")
}

/// Each source file of the app that is not a test, with its text without the tests inside it.
fn sources() -> Vec<(String, String)> {
    let root = src();
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    let texts: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read_to_string(path).unwrap())
        })
        .collect();
    let tests = test_modules(&texts);
    texts
        .into_iter()
        .filter(|(rel, _)| {
            !rel.split('/')
                .any(|part| tests.contains(part.trim_end_matches(".rs")) || part == "tests.rs")
        })
        .map(|(rel, text)| (rel, without_tests(&text)))
        .collect()
}

#[test]
fn no_site_matches_on_the_kind_of_a_task_screen() {
    let sources = sources();
    assert!(
        sources.len() > 200,
        "the scan read {} files, which is too few to be the app",
        sources.len()
    );
    let named_in = |rel: &str| {
        sources
            .iter()
            .find(|(path, _)| path == rel)
            .unwrap_or_else(|| panic!("`{rel}` is not a source file of the app"))
    };
    assert!(
        named_in(TABLE).1.contains("ModuleKind::Clothing"),
        "the table does not name the Wardrobe's kind: the scan reads the wrong text"
    );

    let mut sites = Vec::new();
    for (rel, text) in &sources {
        if rel != TABLE
            && text.contains("ModuleKind::Clothing")
            && !ENGINE.iter().any(|(path, _)| path == rel)
        {
            sites.push(format!("{rel} matches on ModuleKind::Clothing"));
        }
    }
    for (rel, text) in &sources {
        if rel != TABLE
            && text.contains("ModuleKind::Weapons")
            && !WEAPONS_ENGINE.iter().any(|(path, _)| path == rel)
        {
            sites.push(format!("{rel} matches on ModuleKind::Weapons"));
        }
    }
    for (rel, text) in &sources {
        if rel != TABLE
            && text.contains("ModuleKind::Particles")
            && !PARTICLES_ENGINE.iter().any(|(path, _)| path == rel)
        {
            sites.push(format!("{rel} matches on ModuleKind::Particles"));
        }
    }
    for (rel, text) in &sources {
        if rel != TABLE
            && text.contains("ModuleKind::Maps")
            && !MAPS_ENGINE.iter().any(|(path, _)| path == rel)
        {
            sites.push(format!("{rel} matches on ModuleKind::Maps"));
        }
    }
    for rel in READ_THE_TABLE {
        for needle in [
            "Workspace::Wardrobe",
            "Action::AddClothing",
            "Workspace::Weapons",
            "Action::AddWeaponPack",
            "Action::AddParticlePack",
            "Workspace::Map",
            "Action::AddMapPack",
        ] {
            if named_in(rel).1.contains(needle) {
                sites.push(format!("{rel} matches on {needle}"));
            }
        }
    }
    assert!(
        sites.is_empty(),
        "a place decides what a task screen shows without reading its row in {TABLE}:\n  {}",
        sites.join("\n  ")
    );
}

#[test]
fn the_list_of_engine_files_names_only_files_that_still_name_the_clothing_content() {
    let sources = sources();
    let stale: Vec<String> = ENGINE
        .iter()
        .filter(|(rel, _)| {
            !sources
                .iter()
                .any(|(path, text)| path == rel && text.contains("ModuleKind::Clothing"))
        })
        .map(|(rel, why)| format!("{rel} ({why})"))
        .collect();
    assert!(
        stale.is_empty(),
        "these files no longer name the clothing content, so they leave ENGINE:\n  {}",
        stale.join("\n  ")
    );
}

#[test]
fn the_list_of_weapons_engine_files_names_only_files_that_still_name_the_weapons_content() {
    let sources = sources();
    let stale: Vec<String> = WEAPONS_ENGINE
        .iter()
        .filter(|(rel, _)| {
            !sources
                .iter()
                .any(|(path, text)| path == rel && text.contains("ModuleKind::Weapons"))
        })
        .map(|(rel, why)| format!("{rel} ({why})"))
        .collect();
    assert!(
        stale.is_empty(),
        "these files no longer name the weapons content, so they leave the list:
  {}",
        stale.join(
            "
  "
        )
    );
}

#[test]
fn the_list_of_particles_engine_files_names_only_files_that_still_name_the_particles_content() {
    let sources = sources();
    let stale: Vec<String> = PARTICLES_ENGINE
        .iter()
        .filter(|(rel, _)| {
            !sources
                .iter()
                .any(|(path, text)| path == rel && text.contains("ModuleKind::Particles"))
        })
        .map(|(rel, why)| format!("{rel} ({why})"))
        .collect();
    assert!(
        stale.is_empty(),
        "these files no longer name the particles content, so they leave the list:
  {}",
        stale.join(
            "
  "
        )
    );
}

#[test]
fn the_dock_names_the_workspace_of_a_task_screen_once_in_the_list_of_all() {
    let sources = sources();
    let (_, dock) = sources
        .iter()
        .find(|(path, _)| path == "ui/dock.rs")
        .expect("the dock is a source file of the app");
    for workspace in ["Workspace::Wardrobe", "Workspace::Weapons"] {
        let named = dock.matches(workspace).count();
        assert!(
            named <= 1,
            "`ui/dock.rs` names {workspace} {named} times: its label, its words and its layout are the row's in {TABLE}, and the list `ALL` is the one place the dock names it"
        );
    }
    assert_eq!(
        dock.matches("Workspace::Wardrobe").count(),
        1,
        "the dock lists the Wardrobe in `ALL`"
    );
}
