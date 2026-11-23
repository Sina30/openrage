//! Adding particle files to a pack is an import: it decompiles on the import worker and shows as that job, and it is one step of
//! History that Undo takes back with the sources it made and their place in the pack.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, like `the_project_history`.

use std::path::{Path, PathBuf};

use or_particles::Particles;

use super::front_door::{NewProject, Template};
use super::the_project_history::{open, until, write};
use super::AppState;

const SOURCE: &str = "fx_test.particles.src";

/// The Particles project `sparks` in `dir`, opened in a state, and the folder of its pack.
fn a_particles_project(dir: &Path) -> (AppState, PathBuf) {
    let mut draft = NewProject::new(Template::Particles, dir.to_path_buf());
    draft.set_name("sparks".into());
    let file = draft.create().expect("a project").file;
    let mut state = open(&file);
    until(&mut state, "the pack to be read", |s| {
        s.particles_pack().is_some()
    });
    let folder = state.particles_pack().expect("the pack").folder.clone();
    (state, folder)
}

/// A `.ypt` called `name` in a folder of `dir` that a drop would name.
fn dropped(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join("drop").join(name);
    write(&path, bytes);
    path
}

fn sources_of(folder: &Path) -> Vec<String> {
    Particles::read(folder)
        .expect("the pack's model")
        .assets
        .into_iter()
        .map(|asset| asset.source)
        .collect()
}

#[test]
fn adding_a_particle_file_returns_at_once_and_runs_as_the_import_job() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_particles_project(dir.path());
    let ypt = or_particle::sample::list_of("fx_test", &["fx_spark"]);

    state.add_particles(vec![dropped(dir.path(), "fx_test.ypt", &ypt)]);
    assert!(
        state.workbench.imports.is_running(),
        "the decompile ran inside the frame that added the file"
    );
    assert!(
        state.project_import_job().is_some(),
        "the job list does not show the work"
    );
    assert!(
        !folder.join(SOURCE).exists(),
        "the source was made before the frame ended"
    );

    until(&mut state, "the import", |s| {
        !s.workbench.imports.is_running()
    });
    assert!(folder.join(SOURCE).is_dir());
    assert_eq!(sources_of(&folder), [SOURCE]);
}

#[test]
fn one_undo_takes_the_added_files_out_of_the_pack_and_one_redo_puts_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_particles_project(dir.path());
    let ypt = or_particle::sample::list_of("fx_test", &["fx_spark"]);
    state.add_particles(vec![dropped(dir.path(), "fx_test.ypt", &ypt)]);
    until(&mut state, "the import", |s| {
        !s.workbench.imports.is_running()
    });
    assert!(folder.join(SOURCE).is_dir());

    let said = state
        .undo_project()
        .expect("the add is a step to take back");
    assert!(said.starts_with("Added 1 particle file"), "{said}");
    assert!(
        !folder.join(SOURCE).exists(),
        "Undo left the source the add made"
    );
    assert!(
        sources_of(&folder).is_empty(),
        "Undo left an asset that names a source that is gone: {:?}",
        sources_of(&folder)
    );

    state.redo_project().expect("and make it again");
    assert!(folder.join(SOURCE).is_dir());
    assert_eq!(sources_of(&folder), [SOURCE]);
}

#[test]
fn a_file_the_source_form_cannot_hold_is_listed_with_its_sentence_and_the_others_still_come_in() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_particles_project(dir.path());
    let ypt = or_particle::sample::list_of("fx_test", &["fx_spark"]);
    state.add_particles(vec![
        dropped(dir.path(), "fx_test.ypt", &ypt),
        dropped(dir.path(), "broken.ypt", b"not a particle file"),
    ]);
    until(&mut state, "the import", |s| {
        !s.workbench.imports.is_running()
    });

    assert_eq!(sources_of(&folder), [SOURCE], "the good one came in");
    let report = state.workbench.imports.report().expect("a report");
    let refused: Vec<_> = report
        .report
        .files
        .iter()
        .filter(|file| matches!(file.verdict, or_source::game_import::Verdict::Refused))
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", report.report.files);
    assert_eq!(refused[0].input, "broken.ypt");
    assert!(!refused[0].reason.is_empty());
    assert!(
        state.workbench.imports.shown,
        "a refusal is a report to read"
    );
}

#[test]
fn a_new_particle_list_is_an_import_step_that_one_undo_takes_back_and_one_redo_makes_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, folder) = a_particles_project(dir.path());

    state.new_particles();
    assert!(
        state.workbench.imports.is_running(),
        "the list was made inside the frame that asked for it"
    );
    until(&mut state, "the import", |s| {
        !s.workbench.imports.is_running()
    });
    assert!(folder.join("fx_new.particles.src").is_dir());
    assert_eq!(sources_of(&folder), ["fx_new.particles.src"]);

    let said = state.undo_project().expect("the new list is a step");
    assert!(said.contains("fx_new"), "{said}");
    assert!(!folder.join("fx_new.particles.src").exists());
    assert!(sources_of(&folder).is_empty());

    state.redo_project().expect("and make it again");
    assert_eq!(sources_of(&folder), ["fx_new.particles.src"]);
}
