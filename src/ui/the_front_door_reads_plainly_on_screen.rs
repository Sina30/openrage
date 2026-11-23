//! The front door as a person reads it on a first launch and on a return: every way in is on the page, the text that orients is
//! as large as the page's own, a card's main action is a button, and the questions say what they do in plain sentences.
//!
//! Read through AccessKit and the painted text of whole frames of the shell, in the smallest window the editor has and the default one.

use std::path::Path;

use egui::epaint::ClippedShape;

use super::frames_of_the_shell::Shell;
use super::the_front_door_on_screen::{
    a_first_launch, a_project, click_on, names, shows, waits_for,
};
use crate::platform::file_dialogs;
use crate::platform::new_window::fake;
use crate::state::front_door::{NewProject, Template};
use crate::state::workbench::Question;
use crate::state::ThemeChoice;
use crate::ui::commands::{self, Action};
use crate::ui::theme;

const SMALLEST: (u32, u32) = (1280, 720);

/// The size, in points, of the first text painted that holds `text`.
fn size_of(shapes: &[ClippedShape], text: &str) -> Option<f32> {
    fn walk(shape: &egui::Shape, text: &str) -> Option<f32> {
        match shape {
            egui::Shape::Text(t) if t.galley.text().contains(text) => {
                t.galley.job.sections.first().map(|s| s.format.font_id.size)
            }
            egui::Shape::Vec(inner) => inner.iter().find_map(|s| walk(s, text)),
            _ => None,
        }
    }
    shapes.iter().find_map(|c| walk(&c.shape, text))
}

/// The top of the widget named `name`, nearest the top of the window.
fn top_of(shell: &Shell, name: &str) -> f32 {
    (shell.described.iter())
        .filter(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
        .map(|w| w.bounds.min.y)
        .min_by(f32::total_cmp)
        .unwrap_or_else(|| panic!("nothing shows `{name}`: {:?}", names(shell)))
}

#[test]
fn a_first_launch_offers_a_built_pack_on_the_page() {
    for size in [SMALLEST, (1600, 900)] {
        let mut shell = a_first_launch();
        shell.settled_sized(size);
        assert!(
            shows(&shell, "Import built pack…"),
            "a built pack is three clicks down: {:?}",
            names(&shell)
        );
    }
}

#[test]
fn a_first_launch_shows_every_install_and_what_starts_a_project_above_the_fold_with_no_panel_around(
) {
    let mut shell = a_first_launch();
    shell.settled_sized(SMALLEST);
    let said = names(&shell);
    let installs: Vec<_> = (shell.described.iter())
        .filter(|w| w.role == egui::accesskit::Role::Button)
        .filter(|w| {
            (w.name.as_deref()).is_some_and(|n| n == "Open" || n.ends_with("Choose its folder…"))
        })
        .collect();
    assert!(installs.len() >= 2, "no row of an install: {said:?}");
    let fold = SMALLEST.1 as f32;
    for install in installs {
        assert!(
            install.bounds.max.y <= fold,
            "an install's button is under the fold, at {:?}",
            install.bounds
        );
    }
    for name in [
        "New project…",
        "Clothing pack",
        "Open file…",
        "Read the guide",
    ] {
        let bottom = (shell.described.iter())
            .filter(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
            .map(|w| w.bounds.max.y)
            .max_by(f32::total_cmp)
            .unwrap_or_else(|| panic!("nothing shows `{name}`: {said:?}"));
        assert!(bottom <= fold, "`{name}` is under the fold, at {bottom}");
    }
    for panel in [
        "No install, folder or archive open",
        "Nothing open",
        "Nothing selected",
    ] {
        assert!(
            !shows(&shell, panel),
            "a panel that says `{panel}` stands beside the page: {said:?}"
        );
    }
}

#[test]
fn a_built_pack_is_imported_and_never_reopened_in_the_menu_the_palette_and_the_dialogs() {
    let shell = a_first_launch();
    let command = commands::of(&shell.state, Action::OpenBuiltPack).expect("a command");
    assert_eq!(command.label, "Import built pack…");
    let said = command.hover.clone().unwrap_or_default();
    assert!(
        said.starts_with("Import") && !said.contains("files/"),
        "the hover keeps the old verb or an internal folder name: {said}"
    );
}

#[test]
fn the_text_that_orients_a_first_launch_is_as_large_as_the_page_and_tips_nothing_that_is_not_there()
{
    for size in [SMALLEST, (1600, 900)] {
        let mut shell = a_first_launch();
        let shapes = shell.settled_sized(size);
        let asked = size_of(&shapes, "Not sure where to start?").expect("the row of help");
        assert!(
            asked > theme::SMALL_TEXT,
            "the row of help is in footnote size ({asked}) at {size:?}"
        );
        for gone in ["Ctrl+B builds", "Check before sharing"] {
            assert!(
                !shows(&shell, gone),
                "a tip about `{gone}` is on the first page"
            );
        }
        // The installs stand above what starts a project, which stands above the other ways in.
        let tops =
            ["Open an install", "New project…", "Open file…"].map(|name| top_of(&shell, name));
        assert!(
            tops[0] < tops[1] && tops[1] < tops[2],
            "the page is not in the order of its doors at {size:?}: {tops:?}"
        );
    }
}

#[test]
fn nothing_open_means_no_word_in_the_corner_no_brackets_in_the_bar_and_no_block_about_keys() {
    let mut shell = a_first_launch();
    shell.settled_sized(SMALLEST);
    let words = names(&shell);
    assert!(
        !words.iter().any(|n| n.starts_with("Read only")),
        "Read only labels nothing while nothing is open: {words:?}"
    );
    assert!(
        !words.iter().any(|n| n.starts_with('[')),
        "the status bar prints the workspace in brackets: {words:?}"
    );
    assert!(
        shows(&shell, "Read the guide"),
        "the page lacks the guide: {words:?}"
    );
    assert!(
        !words.iter().any(|n| n == "Keys" || n == "Game keys"),
        "a heading about keys with nothing to do: {words:?}"
    );
}

#[test]
fn a_recent_project_card_opens_with_a_button_that_says_so_and_names_the_second_window() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let police = a_project(dir.path(), "police", Template::Files);
    let mut shell = a_first_launch();
    shell.state.prefs.touch_recent_project(police.clone());
    shell.draw(Vec::new());
    waits_for(&mut shell, "police");

    let button = |shell: &Shell, name: &str| {
        shell
            .described
            .iter()
            .any(|w| w.role == egui::accesskit::Role::Button && w.name.as_deref() == Some(name))
    };
    assert!(
        button(&shell, "Open"),
        "the card has no Open: {:?}",
        names(&shell)
    );
    assert!(
        button(&shell, "Open in new window") && !button(&shell, "New window"),
        "the small button does not say what it opens: {:?}",
        names(&shell)
    );

    click_on(&mut shell, "Open in new window");
    assert!(fake::was_opened(&police), "a second process was asked for");
    click_on(&mut shell, "Open");
    assert_eq!(shell.state.project_path.as_deref(), Some(police.as_path()));
}

#[test]
fn a_card_counts_in_words_says_one_state_and_cuts_a_long_path_in_the_middle() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let deep = dir
        .path()
        .join("a_folder_with_a_very_long_name")
        .join("and_another_one_below_it")
        .join("and_a_third_so_the_path_is_long");
    std::fs::create_dir_all(&deep).unwrap();
    let project = a_project(&deep, "clothes", Template::Clothing);
    let mut shell = a_first_launch();
    shell.state.prefs.touch_recent_project(project.clone());
    shell.draw(Vec::new());
    waits_for(&mut shell, "Not built");
    shell.settled_sized(SMALLEST);

    assert!(
        shows(&shell, "Clothing · 0 garments"),
        "{:?}",
        names(&shell)
    );
    assert!(!shows(&shell, "Not built yet"), "the state has two names");
    let path = names(&shell)
        .into_iter()
        .find(|n| {
            n.contains("and_a_third_so_the_path_is_long") || n.contains("clothes.openrage-proj")
        })
        .expect("the card shows the path's end");
    assert!(
        path.ends_with("clothes.openrage-proj") || path.contains("clothes.openrage-proj"),
        "the tail of the path, which tells projects apart, was cut: {path}"
    );
}

/// A shell on the start page, asked about the pack at `built` by the command, with the answer to its folder dialog given.
fn asked_about(built: &Path) -> Shell {
    let mut shell = Shell::new(ThemeChoice::Dark);
    file_dialogs::answer_next(built.to_path_buf());
    commands::run_action(&mut shell.state, &shell.ctx.clone(), Action::OpenBuiltPack);
    shell.settled();
    shell
}

#[test]
fn the_question_about_a_built_pack_says_import_and_what_it_does_in_plain_words() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let pack = crate::state::reopening_a_built_pack_from_the_app::a_built_pack(
        dir.path(),
        or_project::OutputKind::SingleplayerDlc,
    );
    let mut shell = asked_about(&pack);
    let said = names(&shell);
    for text in [
        "Import dlc.rpf as a new project?",
        "Every model and texture in the pack becomes a source you can edit, in a new folder. The pack and your other files are never changed.",
        "Create in",
        "Import as project",
    ] {
        assert!(shows(&shell, text), "the question lacks `{text}`: {said:?}");
    }
    for gone in [
        "Reopen",
        "In folder",
        "files/",
        "A project of sources",
        "Pack name",
    ] {
        assert!(
            !shows(&shell, gone),
            "the question still says `{gone}`: {said:?}"
        );
    }
    // The one sentence that settles the decision is not the smallest text on the dialog.
    let shapes = shell.settled();
    let body = size_of(&shapes, "never changed").expect("the sentence is painted");
    assert!(
        body > theme::SMALL_TEXT,
        "the sentence is in footnote size: {body}"
    );
    // The button waits for a name, and the question says so.
    if let Some(Question::Reopen { name, .. }) = shell.state.workbench.asking.as_mut() {
        name.clear();
    }
    shell.settled();
    assert!(shows(&shell, "Name the new project"), "{:?}", names(&shell));
}

#[test]
fn a_file_that_is_no_package_is_refused_in_a_sentence_without_a_path_or_a_library_and_the_buttons_stay_put(
) {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let bad = dir.path().join("ux-demo-pack.oiv");
    std::fs::write(&bad, b"these bytes are no zip").unwrap();
    let mut shell = asked_about(&bad);
    let made = dir.path().join("made");
    std::fs::create_dir_all(&made).unwrap();
    if let Some(Question::Reopen { place, .. }) = shell.state.workbench.asking.as_mut() {
        *place = made.display().to_string();
    }
    shell.settled();
    let cancel_before = top_of(&shell, "Cancel");

    click_on(&mut shell, "Import as project");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !shows(&shell, "is not a complete OIV package") {
        assert!(
            std::time::Instant::now() < until,
            "no refusal: {:?}",
            names(&shell)
        );
        shell.state.poll_workbench();
        shell.draw(Vec::new());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    shell.settled();
    let said = names(&shell);
    assert!(
        said.iter()
            .any(|n| n.contains("ux-demo-pack.oiv is not a complete OIV package")),
        "{said:?}"
    );
    for jargon in ["EOCD", "zip", dir.path().to_string_lossy().as_ref()] {
        assert!(
            !said.iter().any(|n| n.contains(jargon)),
            "the refusal says `{jargon}`: {said:?}"
        );
    }
    assert!(
        (top_of(&shell, "Cancel") - cancel_before).abs() < 1.0,
        "the buttons moved under the pointer when the refusal came"
    );
}

#[test]
fn a_second_project_asks_with_both_names_and_a_button_that_says_it_replaces_one() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let police = a_project(dir.path(), "police", Template::Files);
    let sheriff = a_project(dir.path(), "sheriff", Template::Files);
    let mut shell = a_first_launch();
    shell.state.load_project_from(police.clone());
    shell.settled();
    shell.state.open_project_asking(sheriff.clone());
    shell.settled();

    for text in [
        "Open sheriff?",
        "police is open in this window.",
        "Open in new window",
        "Replace police",
        "Cancel",
    ] {
        assert!(
            shows(&shell, text),
            "the question lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    assert!(
        !shows(&shell, "Open here"),
        "a button that does not say it replaces"
    );
    click_on(&mut shell, "Replace police");
    assert_eq!(shell.state.project_path.as_deref(), Some(sheriff.as_path()));
}

/// A folder holding one jacket and `extra` textures with no model at their place.
fn a_folder_with_textures_that_have_no_model(folder: &Path, extra: usize) {
    use or_wardrobe::{layout, Component, Ped, Skin, Slot};
    let put = |rel: &str| {
        let path = folder.join(rel);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("a folder");
        std::fs::write(path, b"x").expect("a file");
    };
    let slot = Slot::Component(Component::Jbib);
    let directory = layout::directory("harbor", Ped::Male, "mp_m_harbor", slot);
    put(&format!(
        "{directory}/{}",
        layout::model_name(Ped::Male, "mp_m_harbor", slot, 0, Skin::Universal)
    ));
    for position in 0..=extra {
        put(&format!(
            "{directory}/{}",
            layout::texture_name(
                Ped::Male,
                "mp_m_harbor",
                slot,
                position as u32 * 3,
                0,
                "uni"
            )
        ));
    }
}

#[test]
fn adopting_puts_the_promise_first_and_says_each_reason_once_with_the_names_folded() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    a_folder_with_textures_that_have_no_model(dir.path(), 4);
    let mut shell = a_first_launch();
    let mut draft = NewProject::adopting(dir.path());
    draft.focus = false;
    shell.state.front_door.new_project = Some(draft);
    shell.settled_sized(SMALLEST);
    waits_for(&mut shell, "The folder holds");
    shell.settled_sized(SMALLEST);

    let said = names(&shell);
    let reasons = said
        .iter()
        .filter(|n| n.contains("a texture with no model at its place"))
        .count();
    assert_eq!(
        reasons, 1,
        "the same reason is said once per file: {said:?}"
    );
    assert!(
        !said.iter().any(|n| n.starts_with("Not placed")),
        "the heading says not placed while the reason says it ships: {said:?}"
    );
    assert!(!shows(&shell, "0 places held"), "{said:?}");
    assert!(
        top_of(&shell, "Nothing is moved or rewritten")
            < top_of(&shell, "The folder holds") + 200.0,
        "the promise is below the fold"
    );
}

#[test]
fn the_new_project_gallery_says_what_each_template_is_and_keeps_template_file_apart() {
    let mut shell = a_first_launch();
    click_on(&mut shell, "New project…");
    shell.settled_sized(SMALLEST);
    assert!(shows(&shell, "Files project"), "{:?}", names(&shell));
    assert!(
        shows(&shell, "A project with nothing in it yet."),
        "a template is a bare name until it is clicked: {:?}",
        names(&shell)
    );
    let hover = commands::of(&shell.state, Action::CreateProject)
        .and_then(|c| c.hover)
        .unwrap_or_default();
    assert!(
        hover.contains("clothing pack"),
        "the tooltip leaves out the clothing pack: {hover}"
    );
}
