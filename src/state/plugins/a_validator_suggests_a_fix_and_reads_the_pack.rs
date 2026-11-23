//! A validator plugin reads the clothing of a project as data, and its finding carries a fix that a
//! person accepts with Fix…: the fix lists the files it writes, asks, writes them together as one step
//! of History, and Undo takes them back. Declining changes nothing, and a plugin never writes by
//! itself because it suggested.
//!
//! ⚠ The guest is the sample built with its `fixes` feature by a nested `cargo build` for
//! wasm32-unknown-unknown: it labels each garment of a pack that has none.

use std::path::{Path, PathBuf};

use or_plugin::{Capability, Manifest, ABI_VERSION};
use or_wardrobe::{Garment, Wardrobe};

use super::{Consent, InstalledPlugin};
use crate::state::a_plugin_asks_before_it_runs::built_sample;
use crate::state::front_door::{NewProject, Template};
use crate::state::importing_game_files_from_the_app::until;
use crate::state::workbench::{Fix, Question};
use crate::state::{AppState, Prefs};

const ID: &str = "labeller";
const CODE: &str = "plugin.labeller.garment-unlabelled";

fn guest() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| built_sample("fixes", "openrage-plugin-sample-fixes"))
        .clone()
}

fn labeller(wants: &[Capability]) -> InstalledPlugin {
    InstalledPlugin {
        manifest: Manifest {
            id: ID.into(),
            name: "Labeller".into(),
            abi: ABI_VERSION,
            wants: wants.to_vec(),
        },
        wasm: guest(),
        refused: None,
    }
}

fn answered(state: &mut AppState, plugin: &InstalledPlugin) {
    state.plugins.scanned = Some(vec![plugin.clone()]);
    state.prefs.plugin_consent.insert(
        plugin.manifest.id.clone(),
        Consent::Granted(
            plugin
                .manifest
                .wants
                .iter()
                .map(|c| c.key().to_string())
                .collect(),
        ),
    );
}

/// A clothing project holding a pack of one garment with no label, the validator installed, and the folder of the pack.
fn with_an_unlabelled_garment(dir: &Path, plugin: &InstalledPlugin) -> (AppState, PathBuf) {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let created = draft.create().expect("the template makes a project");
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    answered(&mut state, plugin);
    state.load_project_from(created.file);
    until(&mut state, "the first scan", |s| s.workbench.is_scanned());

    let pack = state
        .wardrobe_pack()
        .expect("the template makes a pack")
        .folder
        .clone();
    let mut wardrobe = Wardrobe::read(&pack).expect("the pack reads");
    wardrobe.garments.push(Garment::new());
    wardrobe.write(&pack).expect("the pack is written");
    state.workbench.want_scan();
    (state, pack)
}

fn plugin_fix(state: &AppState) -> Option<Fix> {
    (state.workbench.findings().iter())
        .flat_map(|f| f.fixes.clone())
        .find(|fix| matches!(fix, Fix::Plugin { .. }))
}

fn labels(pack: &Path) -> Vec<Option<String>> {
    let wardrobe = Wardrobe::read(pack).expect("the pack reads");
    wardrobe.garments.iter().map(|g| g.label.clone()).collect()
}

fn reading_and_suggesting() -> [Capability; 2] {
    [Capability::ReadPack, Capability::SuggestFixes]
}

#[test]
fn a_validator_that_reads_the_pack_returns_a_finding_with_a_fix_beside_its_waiver() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = labeller(&reading_and_suggesting());
    let (mut state, _) = with_an_unlabelled_garment(dir.path(), &plugin);

    until(&mut state, "the finding", |s| {
        s.workbench.scan_is_current() && plugin_fix(s).is_some()
    });

    let finding = (state.workbench.findings().iter())
        .find(|f| f.text.starts_with(CODE))
        .expect("the plugin's finding is listed");
    assert!(
        finding.text.contains("1 garment(s) have no label"),
        "it counted the garments in the pack it read: {}",
        finding.text
    );
    let labels: Vec<&str> = finding.fixes.iter().map(Fix::label).collect();
    assert_eq!(
        labels,
        ["Fix…", "Waive…"],
        "a fix like a rule's, then the waiver"
    );
}

#[test]
fn accepting_the_fix_writes_the_pack_as_one_step_that_undo_takes_back() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = labeller(&reading_and_suggesting());
    let (mut state, pack) = with_an_unlabelled_garment(dir.path(), &plugin);
    until(&mut state, "the finding", |s| {
        s.workbench.scan_is_current() && plugin_fix(s).is_some()
    });
    let fix = plugin_fix(&state).unwrap();
    assert_eq!(labels(&pack), [None]);

    // Fix… asks first, and nothing is written by asking.
    state.ask_about_fix(&fix);
    match &state.workbench.asking {
        Some(Question::PluginFix { name, fix, .. }) => {
            assert_eq!(name, "Labeller");
            assert_eq!(fix.label, "Give each a label");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(labels(&pack), [None], "asking changed the pack");

    let Fix::Plugin { name, fix, .. } = fix else {
        unreachable!()
    };
    let listed = state.plugin_fix_files(&fix);
    assert_eq!(listed.len(), 1);
    assert!(
        listed[0].0.ends_with("wardrobe.json") && listed[0].1,
        "it replaces the pack: {listed:?}"
    );

    state.apply_plugin_fix(&name, &fix);
    assert_eq!(labels(&pack), [Some("Garment 1".to_string())]);
    assert_eq!(
        state.project_undo_label().as_deref(),
        Some("Labeller: Give each a label")
    );

    // The pack is read again, and the plugin no longer has anything to say.
    until(&mut state, "the rescan", |s| {
        s.workbench.scan_is_current()
            && !s
                .workbench
                .findings()
                .iter()
                .any(|f| f.text.starts_with(CODE))
    });

    state.undo_project().expect("Undo takes the fix back");
    assert_eq!(labels(&pack), [None]);
    state.redo_project().expect("Redo puts it back");
    assert_eq!(labels(&pack), [Some("Garment 1".to_string())]);
}

#[test]
fn a_plugin_that_is_not_granted_the_pack_is_not_handed_it_and_one_that_cannot_suggest_has_no_fix() {
    let dir = tempfile::tempdir().unwrap();
    // Asking for the fix alone: the guest imports `pack_size`, which only the pack buys, so it does not run.
    let plugin = labeller(&[Capability::SuggestFixes]);
    let (mut state, _) = with_an_unlabelled_garment(dir.path(), &plugin);

    until(&mut state, "the scan", |s| s.workbench.scan_is_current());

    assert!(plugin_fix(&state).is_none());
    assert!(
        (state.workbench.findings().iter())
            .any(|f| f.text.contains("could not run over this plan")),
        "the plugin says it could not run: {:?}",
        state.workbench.findings()
    );
}

#[test]
fn a_fix_that_writes_what_a_project_cannot_hold_is_refused_whole_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = labeller(&reading_and_suggesting());
    let (mut state, pack) = with_an_unlabelled_garment(dir.path(), &plugin);
    until(&mut state, "the finding", |s| {
        s.workbench.scan_is_current() && plugin_fix(s).is_some()
    });
    let Some(Fix::Plugin { fix, .. }) = plugin_fix(&state) else {
        unreachable!()
    };
    let mut broken = fix.clone();
    broken.files.files.push(or_plugin::FileWrite {
        path: "models/jacket.ydd".into(),
        bytes: b"x".to_vec(),
    });
    let label_before = state.project_undo_label();

    let why = state
        .apply_plugin_files("Labeller: a fix", &broken.files)
        .expect_err("a game file is not written into a project");

    assert!(
        why.contains("No file was written") && why.contains("game file"),
        "{why}"
    );
    assert_eq!(
        labels(&pack),
        [None],
        "the first file of the set was written"
    );
    assert_eq!(state.project_undo_label(), label_before);
}
