//! The Particle screen as it is drawn for a project that holds particles: a particle pack opens on it, it shows in Content, Open in and the
//! start page through the table of task screens alone, its Effects panel lists the effects of the files a drop adds and plays the one a
//! click chooses, an effect edit is one step of Undo and a save goes into the source, and the workspace keeps an arrangement for a
//! project apart from the one a window with none uses.
//!
//! Read through AccessKit from whole frames of the shell, with the particle lists this file makes from the editor's own calls.

use std::path::{Path, PathBuf};

use or_io::decl::Hash32;

use super::frames_of_the_shell::Shell;
use super::the_front_door_on_screen::{a_first_launch, a_project, shows};
use super::the_project_screens_on_screen::{centre_of, names, until};
use crate::state::front_door::cards::read_card;
use crate::state::front_door::Template;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;
use crate::ui::panels::selection_ids::ypt_effect_id;

/// A `.ypt` that holds the effects `effects`, each firing one emitter rule and one particle rule of its own.
pub(super) fn a_ypt(name: &str, effects: &[&str]) -> Vec<u8> {
    or_particle::sample::list_of(name, effects)
}

/// Draw frames, polling what the workers left and the loads and saves the window finishes between frames, until `done`.
fn until_finished(shell: &mut Shell, what: &str, done: impl Fn(&Shell) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        shell.state.poll_workbench();
        shell.state.poll_load_result();
        shell.state.poll_save_result();
        shell.draw(Vec::new());
        if done(shell) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what} never happened: {:?}",
            names(shell)
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

/// The place of the effect `name` among the effects of a list, which is the order of the hash of the name.
fn place_of(name: &str, all: &[&str]) -> usize {
    let mut hashes: Vec<u32> = all.iter().map(|n| Hash32::of(n).get()).collect();
    hashes.sort_unstable();
    hashes
        .iter()
        .position(|h| *h == Hash32::of(name).get())
        .expect("the effect is in the list")
}

const EFFECTS: [&str; 2] = ["fx_spark", "fx_smoke"];

/// A shell on a particle pack `sparks`, which holds the file `fx_test` of `EFFECTS` once the scan has read it and its source is cooked.
pub(super) fn a_particles_shell(dir: &Path) -> Shell {
    let file = a_project(dir, "sparks", Template::Particles);
    let mut shell = Shell::new(crate::state::ThemeChoice::Dark);
    shell.state.load_project_from(file);
    let dropped = write(
        &dir.join("drop").join("fx_test.ypt"),
        &a_ypt("fx_test", &EFFECTS),
    );
    shell.state.add_particles(vec![dropped]);
    shell.state.active_workspace = Workspace::Particle;
    shell.settled();
    until(&mut shell, "the effects", |s| {
        s.state
            .particles_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.effects().len() == EFFECTS.len())
    });
    shell
}

#[test]
fn a_particle_pack_opens_on_the_particle_screen_and_shows_in_content_the_strip_and_the_start_page_through_the_table(
) {
    let _serial = crate::ui::theme::serial();
    let _english = crate::i18n::pin("en");
    let dir = tempfile::tempdir().unwrap();
    let file = a_project(dir.path(), "sparks", Template::Particles);
    let card = read_card(&file);
    assert_eq!(
        card.content,
        ["Particles · 0 particle files"],
        "the card counts in the row's word"
    );

    let mut shell = Shell::new(crate::state::ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.settled();
    assert_eq!(shell.state.active_workspace, Workspace::Particle);
    assert!(shell
        .state
        .workspaces_offered()
        .contains(&Workspace::Particle));
    until(&mut shell, "the pack to be counted", |s| {
        shows(s, "0 particle files")
    });
}

#[test]
fn the_empty_pack_says_so_and_offers_the_one_action() {
    let _serial = crate::ui::theme::serial();
    let _english = crate::i18n::pin("en");
    let dir = tempfile::tempdir().unwrap();
    let file = a_project(dir.path(), "sparks", Template::Particles);
    let mut shell = Shell::new(crate::state::ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Particle;
    shell.settled();
    until(&mut shell, "the Effects panel", |s| {
        shows(s, "No particle effects in this project")
    });
    let adds = names(&shell)
        .iter()
        .filter(|name| name.as_str() == "Add particle effects…")
        .count();
    assert_eq!(adds, 1, "one action: {:?}", names(&shell));
}

#[test]
fn a_window_with_no_project_has_no_effects_panel_and_a_project_that_holds_particles_has_one_and_gives_the_other_back(
) {
    let _serial = crate::ui::theme::serial();
    let _english = crate::i18n::pin("en");
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_first_launch();
    shell.state.active_workspace = Workspace::Particle;
    shell.settled();
    shell.state.poll_workbench();
    let shows_effects = |shell: &Shell| {
        shell
            .state
            .workspace_docks
            .shows(Workspace::Particle, ids::EFFECTS)
    };
    assert!(!shows_effects(&shell), "a window with no project");
    let before: Vec<&str> = shell
        .state
        .workspace_docks
        .peek(Workspace::Particle)
        .unwrap()
        .iter_all_tabs()
        .map(|(_, tab)| tab.id())
        .collect();

    let file = a_project(dir.path(), "sparks", Template::Particles);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Particle;
    until(&mut shell, "the arrangement for the project", |s| {
        shows_effects(s)
    });

    // The project's panels do not stay behind in the window of a person with none.
    shell.state.project_carried.container.modules.clear();
    shell.state.poll_workbench();
    shell.state.poll_workbench();
    assert!(!shows_effects(&shell), "the content went");
    let after: Vec<&str> = shell
        .state
        .workspace_docks
        .peek(Workspace::Particle)
        .unwrap()
        .iter_all_tabs()
        .map(|(_, tab)| tab.id())
        .collect();
    assert_eq!(after, before, "the other arrangement comes back as it was");
}

#[test]
fn a_dropped_ypt_lists_its_effects_and_a_click_shows_one_of_them_in_the_particle_workspace() {
    let _serial = crate::ui::theme::serial();
    let _english = crate::i18n::pin("en");
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_particles_shell(dir.path());
    for effect in EFFECTS {
        assert!(shows(&shell, effect), "{effect}: {:?}", names(&shell));
    }
    // The click cooks the source if it is not cooked, and what it cooks to is what the workspace opens.
    let at = centre_of(&shell, "fx_smoke");
    shell.click(at);
    until_finished(&mut shell, "the effect to be selected", |s| {
        s.state
            .active_document
            .and_then(|id| s.state.selection.selected(id))
            == Some(ypt_effect_id(place_of("fx_smoke", &EFFECTS)))
    });
    let source = shell
        .state
        .particles_pack()
        .unwrap()
        .read
        .as_ref()
        .unwrap()
        .model
        .assets[0]
        .source
        .clone();
    assert_eq!(
        shell.state.effect_shown_from(&source),
        Some(ypt_effect_id(place_of("fx_smoke", &EFFECTS))),
        "the panel marks the effect that plays"
    );
}

#[test]
fn an_effect_edit_is_one_step_of_undo_and_a_save_goes_into_the_source() {
    let _serial = crate::ui::theme::serial();
    let _english = crate::i18n::pin("en");
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_particles_shell(dir.path());
    let (module, source) = {
        let pack = shell.state.particles_pack().unwrap();
        (
            pack.module.clone(),
            pack.read.as_ref().unwrap().model.assets[0].source.clone(),
        )
    };
    let place = place_of("fx_spark", &EFFECTS);
    shell.state.show_effect(&module, &source, place);
    until_finished(&mut shell, "the document to open on the effect", |s| {
        s.state
            .active_document
            .and_then(|id| s.state.selection.selected(id))
            == Some(ypt_effect_id(place))
    });
    let doc = shell.state.active_document.unwrap();
    let rules_file = {
        let pack = shell.state.particles_pack().unwrap();
        pack.folder.join(&source).join("particles.json")
    };
    assert!(std::fs::read_to_string(&rules_file)
        .unwrap()
        .contains("fx_spark"));

    crate::ui::panels::hierarchy::delete_particle_row(&mut shell.state, doc, ypt_effect_id(place));
    assert_eq!(
        shell.state.document(doc).unwrap().history.done().len(),
        1,
        "an effect taken out is one step"
    );
    assert!(shell.state.save_active_document());
    until_finished(&mut shell, "the source to hold the edit", |_| {
        std::fs::read_to_string(&rules_file).is_ok_and(|text| !text.contains("\"fx_spark\""))
    });
    let text = std::fs::read_to_string(&rules_file).unwrap();
    assert!(
        text.contains("fx_smoke"),
        "the other effect stayed in the source"
    );
    until_finished(&mut shell, "the document to be saved", |s| {
        s.state.document(doc).is_some_and(|d| d.history.at_saved())
    });
}

/// The Inspector and the Timeline of a chosen effect say their sentences in French, and leave none of the English ones on screen.
#[test]
fn the_inspector_and_the_timeline_of_an_effect_say_their_sentences_in_french() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = {
        let _english = crate::i18n::pin("en");
        a_particles_shell(dir.path())
    };
    let _french = crate::i18n::pin("fr");
    shell.settled();
    let at = centre_of(&shell, "fx_smoke");
    shell.click(at);
    until_finished(&mut shell, "the effect to be selected", |s| {
        s.state
            .active_document
            .and_then(|id| s.state.selection.selected(id))
            == Some(ypt_effect_id(place_of("fx_smoke", &EFFECTS)))
    });
    shell.settled();
    for french in [
        "Détails internes",
        "Boucle",
        "Vitesse",
        "Filtrer les canaux…",
    ] {
        assert!(shows(&shell, french), "{french}: {:?}", names(&shell));
    }
    for english in ["Show internals", "Loop", "Filter channels…", "Weather"] {
        assert!(
            !shows(&shell, english),
            "{english} is still English: {:?}",
            names(&shell)
        );
    }
}
