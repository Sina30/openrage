//! The picker and the dialogs of New as a person meets them: reached and answered with the keyboard alone, drawn whole in a
//! window 800 points wide, every control named, and each answer of the question a file of elsewhere asks leaving the
//! project as it says.
//!
//! ⚠ Read through AccessKit from whole frames of the shell; the file dialogs are the stand-ins of a test build, which answer
//! with the path a case names.

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::{key, Shell};
use super::the_wardrobe_operations_on_screen::{
    a_shell_with_two_jackets, click_on, names, put, shows, until,
};
use crate::platform::file_dialogs;
use crate::ui::commands::{self, Action};
use crate::ui::theme;

/// The narrowest window the dialogs are held to, in points.
const WINDOW: (u32, u32) = (800, 600);

fn run(shell: &mut Shell, action: Action) {
    let ctx = shell.ctx.clone();
    commands::run_action(&mut shell.state, &ctx, action);
    shell.settled_sized(WINDOW);
}

fn press(shell: &mut Shell, events: Vec<egui::Event>) {
    shell.draw_sized(WINDOW, events);
    shell.settled_sized(WINDOW);
}

/// The model the first garment names, as the pack states it.
fn model_of_first(shell: &Shell) -> String {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments[0].model.clone())
        .unwrap()
}

/// Each of `controls` is named in the last frame and lies whole inside a window of `WINDOW` points.
fn all_inside(shell: &Shell, controls: &[&str]) {
    let window = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(WINDOW.0 as f32, WINDOW.1 as f32),
    );
    for name in controls {
        let widget = (shell.described.iter())
            .find(|w| w.name.as_deref() == Some(*name))
            .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
        assert!(
            window.contains_rect(widget.bounds),
            "`{name}` leaves a window {WINDOW:?} wide: {:?}",
            widget.bounds
        );
    }
}

#[test]
fn the_picker_is_walked_by_the_arrow_keys_and_answered_by_enter_and_draws_whole_in_800_points() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, folder) = a_shell_with_two_jackets(dir.path());
    let before = model_of_first(&shell);

    run(&mut shell, Action::ReplaceModel);
    assert!(
        shows(&shell, "Choose the model"),
        "the command opens the picker: {:?}",
        names(&shell)
    );
    assert_eq!(
        shell.focused().map(|(_, name)| name).as_deref(),
        Some("Search the picker"),
        "the keyboard is in the search field with no click"
    );
    all_inside(
        &shell,
        &[
            "Search the picker",
            "Browse…",
            "Browse a source folder…",
            "The name of a game file typed by hand",
            "Choose",
            "Cancel",
        ],
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the picker", &mut found);
    assert!(found.is_empty(), "{found:?}");
    assert!(
        shows(&shell, "jbib_000_u") && shows(&shell, "jbib_001_u"),
        "the sources of the pack are the project's own: {:?}",
        names(&shell)
    );

    // Nothing is chosen until the person chooses it: Enter on the list as it opens answers nothing.
    press(&mut shell, vec![key(egui::Key::Enter)]);
    assert!(
        shows(&shell, "Choose the model"),
        "Enter with no row chosen leaves the picker open"
    );
    assert_eq!(model_of_first(&shell), before, "and chose nothing");
    // Down walks onto the first source and Down again to the second, Enter chooses it, and the garment names it by a reference.
    press(&mut shell, vec![key(egui::Key::ArrowDown)]);
    press(&mut shell, vec![key(egui::Key::ArrowDown)]);
    press(&mut shell, vec![key(egui::Key::Enter)]);
    until(&mut shell, "the reference", |s| {
        model_of_first(s).ends_with(".ref.json")
    });
    assert!(!shows(&shell, "Choose the model"), "the answer closes it");
    let reference = or_project::reference::Reference::read(&folder.join(model_of_first(&shell)))
        .expect("the reference reads");
    assert!(
        matches!(&reference, or_project::reference::Reference::Here { path, .. } if path.contains("jbib_001_u")),
        "{reference:?}"
    );
    assert!(
        folder.join(&before).is_dir(),
        "the source it stood for is not touched"
    );
}

#[test]
fn the_enter_that_ran_the_command_does_not_choose_the_first_row_of_the_picker_it_opened() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let before = model_of_first(&shell);

    // The palette runs the command on Enter, and that Enter is still in the frame the picker is first drawn in.
    let ctx = shell.ctx.clone();
    commands::run_action(&mut shell.state, &ctx, Action::ReplaceModel);
    press(&mut shell, vec![key(egui::Key::Enter)]);
    assert!(
        shows(&shell, "Choose the model"),
        "the picker stays open: {:?}",
        names(&shell)
    );
    assert_eq!(model_of_first(&shell), before, "and chose nothing");
}

#[test]
fn typing_narrows_the_picker_and_escape_closes_it_with_nothing_changed() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let before = model_of_first(&shell);

    run(&mut shell, Action::ReplaceModel);
    press(&mut shell, vec![egui::Event::Text("001".into())]);
    assert!(
        shows(&shell, "jbib_001_u") && !shows(&shell, "jbib_000_u, "),
        "the search keeps what it names: {:?}",
        names(&shell)
    );

    press(&mut shell, vec![key(egui::Key::Escape)]);
    assert!(!shows(&shell, "Choose the model"), "Escape closes it");
    assert_eq!(model_of_first(&shell), before, "and nothing was chosen");
}

#[test]
fn a_file_of_elsewhere_asks_whether_to_import_or_only_reference_and_each_answer_leaves_the_project_as_it_says(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, folder) = a_shell_with_two_jackets(dir.path());
    let before = model_of_first(&shell);
    let elsewhere = put(dir.path(), &[("another jacket.ydd", b"another jacket")]).remove(0);
    let sources = || {
        let mut found: Vec<_> = std::fs::read_dir(folder.join("models"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    };
    let held = sources();

    // Cancel (Escape) does neither.
    file_dialogs::answer_next(elsewhere.clone());
    run(&mut shell, Action::ReplaceModel);
    click_on(&mut shell, "Browse…");
    let title = "Only reference it";
    assert!(shows(&shell, title), "{:?}", names(&shell));
    shell.settled_sized(WINDOW);
    all_inside(
        &shell,
        &["Import into the project", "Only reference it", "Cancel"],
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the question of a file of elsewhere", &mut found);
    assert!(found.is_empty(), "{found:?}");
    press(&mut shell, vec![key(egui::Key::Escape)]);
    assert!(!shows(&shell, title), "Escape closes the question");
    assert_eq!(model_of_first(&shell), before);
    assert_eq!(sources(), held, "and imported nothing");

    // Only reference leaves the file where it is and adds no source.
    file_dialogs::answer_next(elsewhere.clone());
    run(&mut shell, Action::ReplaceModel);
    click_on(&mut shell, "Browse…");
    click_on(&mut shell, "Only reference it");
    until(&mut shell, "the reference", |s| {
        model_of_first(s).ends_with(".ref.json")
    });
    assert!(elsewhere.is_file(), "the file stays where it was");
    assert_eq!(
        sources().len(),
        held.len() + 1,
        "only the reference is added: {:?}",
        sources()
    );
    assert!(
        !sources().iter().any(|n| n.contains("another jacket")),
        "no source was made of it: {:?}",
        sources()
    );

    // Import decompiles it into a source of the project.
    file_dialogs::answer_next(elsewhere.clone());
    run(&mut shell, Action::ReplaceModel);
    click_on(&mut shell, "Browse…");
    click_on(&mut shell, "Import into the project");
    until(&mut shell, "the source", |s| {
        model_of_first(s).ends_with(".drawables.src")
    });
    assert!(
        sources().iter().any(|n| n.contains("another jacket")),
        "a source was made of it: {:?}",
        sources()
    );
}

#[test]
fn new_texture_dictionary_takes_a_name_from_the_keyboard_and_makes_an_empty_source() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    let project = or_project::project_folder(shell.state.project_path.as_deref().unwrap());

    run(&mut shell, Action::NewTexturesSource);
    assert!(
        shows(&shell, "New texture dictionary"),
        "the command asks for a name: {:?}",
        names(&shell)
    );
    assert_eq!(
        shell.focused().map(|(_, name)| name).as_deref(),
        Some("Name of the new source"),
        "the keyboard is in the name with no click"
    );
    all_inside(&shell, &["Name of the new source", "Create", "Cancel"]);
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the New dialog", &mut found);
    assert!(found.is_empty(), "{found:?}");

    press(&mut shell, vec![egui::Event::Text("faces".into())]);
    press(&mut shell, vec![key(egui::Key::Enter)]);
    until(&mut shell, "the source", |_| {
        project
            .join("sources/faces.textures.src/asset.json")
            .is_file()
    });
    assert!(!shows(&shell, "New texture dictionary"), "Enter closes it");
}

#[test]
fn the_picker_for_a_missing_model_names_the_garment_offers_only_what_is_there_and_chooses_nothing_for_the_person(
) {
    use crate::state::workbench::{Missing, Wanting};
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, folder) = a_shell_with_two_jackets(dir.path());
    let module = shell.state.wardrobe_module_id().unwrap();
    let (garment, model) = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| {
            let first = &read.wardrobe.garments[0];
            (first.id.clone(), first.model.clone().unwrap())
        })
        .unwrap();
    std::fs::remove_dir_all(folder.join(&model)).expect("the model goes missing");

    shell.state.ask_for_a_file(Wanting::Locate {
        module,
        garment,
        what: Missing::Model,
    });
    shell.settled_sized(WINDOW);

    assert!(
        shows(&shell, "Locate the model of jbib_000_u"),
        "the title says which garment it is for: {:?}",
        names(&shell)
    );
    let rows: Vec<String> = names(&shell)
        .into_iter()
        .filter(|n| n.contains("jbib_0") && n.contains(", "))
        .collect();
    assert!(
        rows.iter().any(|n| n.contains("jbib_001_u"))
            && rows.iter().all(|n| !n.contains("jbib_000_u")),
        "the picker lists what is there and not the file that is missing: {rows:?}"
    );
    let choose = |shell: &Shell| {
        (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("Choose"))
            .expect("a Choose button")
            .disabled
    };
    assert!(choose(&shell), "no row is chosen, so Choose waits");
    press(&mut shell, vec![key(egui::Key::Enter)]);
    assert!(
        shows(&shell, "Locate the model"),
        "Enter answers for nobody: the picker stays"
    );
    press(&mut shell, vec![key(egui::Key::ArrowDown)]);
    assert!(!choose(&shell), "a row the person walked to is chosen");
}

#[test]
fn the_picker_lists_each_file_by_the_name_of_its_garment_the_garments_own_first_and_has_one_answer()
{
    use or_wardrobe::Wardrobe;
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, folder) = a_shell_with_two_jackets(dir.path());
    let mut pack = Wardrobe::read(&folder).unwrap();
    pack.garments[0].label = Some("Wool blazer".into());
    pack.garments[1].label = Some("Trench coat".into());
    pack.write(&folder).unwrap();
    shell.state.workbench.want_scan();
    until(&mut shell, "the garments' names", |s| {
        (s.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.wardrobe.garments[1].label.as_deref() == Some("Trench coat"))
    });

    run(&mut shell, Action::ReplaceModel);
    assert!(
        shows(&shell, "Choose the model of Wool blazer"),
        "{:?}",
        names(&shell)
    );
    let mut rows: Vec<(f32, String)> = (shell.described.iter())
        .filter(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.contains("jbib_00") && n.contains(", "))
        })
        .map(|w| (w.bounds.min.y, w.name.clone().unwrap()))
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let rows: Vec<&str> = rows.iter().map(|(_, name)| name.as_str()).collect();
    // ⚠ The thing to recognise is the garment, so it leads; the file's own name and folder are the line under it.
    assert!(
        rows.len() == 2
            && rows[0].starts_with("Wool blazer (current), ")
            && rows[0].contains("jbib_000_u")
            && rows[1].starts_with("Trench coat, ")
            && rows[1].contains("jbib_001_u"),
        "the garment's own file first, each named by its garment: {rows:?}"
    );
    assert!(
        !shows(&shell, "For Wool blazer"),
        "the title says it already: {:?}",
        names(&shell)
    );
    // One answer: the name of a game file is chosen by the same button, and no second button is there for it.
    assert!(
        shows(&shell, "The name of a game file typed by hand"),
        "{:?}",
        names(&shell)
    );
    assert!(
        !shows(&shell, "Use this id") && !shows(&shell, "Use this name"),
        "a second confirm button: {:?}",
        names(&shell)
    );
    let choose = |shell: &Shell| {
        (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("Choose"))
            .expect("a Choose button")
            .disabled
    };
    assert!(choose(&shell), "nothing chosen, nothing typed");
    click_on(&mut shell, "The name of a game file typed by hand");
    press(
        &mut shell,
        vec![egui::Event::Text("mp_m_freemode_01_jbib_001".into())],
    );
    assert!(!choose(&shell), "a name typed is an answer Choose gives");
}
