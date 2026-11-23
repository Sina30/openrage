//! The front door after a second look: one name for one thing, a line that promises only what a drop does, the two words for a game
//! file said where a newcomer first reads them, and the questions that say who and what they act on.
//!
//! Read through AccessKit and the painted shapes of whole frames of the shell, in the smallest window the editor has and the default one.

use egui::epaint::{ClippedShape, Shape};

use super::frames_of_the_shell::Shell;
use super::the_front_door_on_screen::{a_first_launch, a_project, names, shows};
use crate::state::front_door::{NewProject, Step, Template};
use crate::state::workbench::Question;
use crate::state::ThemeChoice;
use crate::ui::theme;

const SMALLEST: (u32, u32) = (1280, 720);
const SIZES: [(u32, u32); 2] = [SMALLEST, (1600, 900)];

/// Every text painted, in the order it was painted.
fn painted(shapes: &[ClippedShape]) -> Vec<(egui::Rect, String)> {
    fn walk(shape: &Shape, out: &mut Vec<(egui::Rect, String)>) {
        match shape {
            Shape::Text(text) => out.push((
                text.galley.rect.translate(text.pos.to_vec2()),
                text.galley.text().to_string(),
            )),
            Shape::Vec(inner) => inner.iter().for_each(|shape| walk(shape, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, &mut out));
    out
}

/// The first character of the first text that holds `text`: the glyph a button carries.
fn glyph_of(shapes: &[ClippedShape], text: &str) -> char {
    (painted(shapes).into_iter())
        .find(|(_, painted)| painted.contains(text))
        .and_then(|(_, painted)| painted.chars().next())
        .unwrap_or_else(|| panic!("no text holds `{text}`"))
}

/// A first launch on the page of the game, with a project of the person's own among the recent ones.
fn a_return(dir: &std::path::Path) -> Shell {
    let mut shell = a_first_launch();
    let file = a_project(dir, "clothes", Template::Clothing);
    shell.state.prefs.recent_projects = vec![file];
    shell
}

#[test]
fn the_page_leaves_what_a_drop_does_to_the_hint_of_a_held_drag_on_every_launch() {
    for size in SIZES {
        let again = tempfile::tempdir().expect("a scratch folder");
        for mut shell in [a_first_launch(), a_return(again.path())] {
            shell.settled_sized(size);
            let said = names(&shell);
            for line in [
                "drop a project",
                "Open looks at a game file",
                "Import turns it into a source",
            ] {
                assert!(
                    !shows(&shell, line),
                    "{size:?}: the page explains `{line}`, which the hint of a held drag says where it applies: {said:?}"
                );
            }
        }
    }
}

#[test]
fn a_chip_has_the_name_of_what_it_starts_and_repeats_no_button_of_the_row_under_it() {
    for size in SIZES {
        let mut shell = a_first_launch();
        shell.settled_sized(size);
        let said = names(&shell);
        for text in ["Files project", "Clothing pack", "Project from a folder"] {
            assert!(
                shows(&shell, text),
                "{size:?}: no chip is called `{text}`: {said:?}"
            );
        }
        for old in [
            "Adopt",
            "Explore a game file",
            "Files to ship",
            "Ship files as a resource",
            "Make clothes",
            "Use a folder you have",
            "Good to know",
        ] {
            assert!(
                !shows(&shell, old),
                "{size:?}: `{old}` is a second name for something the page names another way: {said:?}"
            );
        }
        // The glyph of a button is the start of its name.
        let open_file = (shell.described.iter())
            .filter(|w| w.role == egui::accesskit::Role::Button)
            .filter(|w| w.name.as_deref().is_some_and(|n| n.ends_with("Open file…")))
            .count();
        assert_eq!(
            open_file, 1,
            "{size:?}: Open file… is on the page {open_file} times"
        );
        assert!(
            !said.iter().any(|name| name.starts_with("Help:")),
            "{size:?}: a card has a ? that opens the guide a link already opens: {said:?}"
        );
    }
    assert_eq!(Template::Files.label(), "Files project");
    assert_eq!(Template::Adopt.label(), "Project from a folder");
}

#[test]
fn the_gallery_and_the_details_call_each_template_by_the_name_its_card_has() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    shell.state.front_door.new_project = Some(NewProject::new(Template::Empty, dir.path().into()));
    shell.settled_sized(SMALLEST);
    for text in [
        "Empty project",
        "Files project",
        "Clothing pack",
        "Project from a folder",
    ] {
        assert!(
            shows(&shell, text),
            "the gallery lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    for old in ["Adopt a folder", "Files to ship"] {
        assert!(!shows(&shell, old), "the gallery still says `{old}`");
    }
    for (template, title) in [
        (Template::Files, "New files project"),
        (Template::Clothing, "New clothing pack"),
        (Template::Adopt, "New project from a folder"),
    ] {
        let mut draft = NewProject::new(template, dir.path().into());
        draft.step = Step::Details;
        shell.state.front_door.new_project = Some(draft);
        shell.settled_sized(SMALLEST);
        assert!(
            shows(&shell, title),
            "{template:?}: the dialog is not titled `{title}`"
        );
    }
}

#[test]
fn the_sample_project_and_a_project_file_do_not_wear_one_glyph() {
    let mut shell = a_first_launch();
    let shapes = shell.settled_sized(SMALLEST);
    assert_ne!(
        glyph_of(&shapes, "Open the sample project"),
        glyph_of(&shapes, "Open project…"),
        "two buttons that do different things carry one glyph"
    );
}

/// Two projects, one open, and the other dropped on the window: the question about them is on screen.
fn asked_which_project(dir: &std::path::Path) -> (Shell, std::path::PathBuf) {
    let (open, other) = (
        a_project(dir, "harbor_ok", Template::Files),
        a_project(dir, "harbor", Template::Files),
    );
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(open);
    shell.state.front_door.switching = Some(other.clone());
    shell.settled_sized(SMALLEST);
    (shell, other)
}

#[test]
fn the_question_of_a_second_project_says_where_it_is_and_what_each_button_does() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let (shell, other) = asked_which_project(dir.path());
    let said = names(&shell);
    assert!(
        said.iter()
            .any(|name| name.ends_with("harbor.openrage-proj")),
        "the question does not say where {} is: {said:?}",
        other.display()
    );
    for text in [
        "harbor_ok stays here and harbor opens in a second window",
        "harbor_ok closes in this window and harbor opens in its place",
        "asks first if there is unsaved work",
    ] {
        assert!(shows(&shell, text), "the question lacks `{text}`: {said:?}");
    }
}

/// The widest horizontal rule drawn under a heading in the area of the question named `key`, as its right end.
fn rule_end(shell: &Shell, shapes: &[ClippedShape], key: &str) -> f32 {
    fn walk(shape: &Shape, within: egui::Rect, out: &mut Vec<f32>) {
        match shape {
            Shape::LineSegment { points, .. }
                if points[0].y == points[1].y && within.contains(points[0]) =>
            {
                out.push(points[0].x.max(points[1].x));
            }
            Shape::Vec(inner) => inner.iter().for_each(|shape| walk(shape, within, out)),
            _ => {}
        }
    }
    let area = shell
        .ctx
        .memory(|m| m.area_rect(egui::Id::new(("question", key))))
        .expect("the question is on screen");
    let mut ends = Vec::new();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, area, &mut ends));
    ends.into_iter().fold(0.0, f32::max)
}

/// The built pack the app asks about: a dlc.rpf made in `dir`.
fn asked_about_a_pack(dir: &std::path::Path) -> Shell {
    let pack = crate::state::reopening_a_built_pack_from_the_app::a_built_pack(
        dir,
        or_project::OutputKind::SingleplayerDlc,
    );
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.ask_to_reopen(pack);
    shell.settled_sized(SMALLEST);
    shell
}

#[test]
fn the_question_about_a_built_pack_keeps_one_width_marks_its_main_button_and_sends_nobody_to_the_file_manager(
) {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = asked_about_a_pack(dir.path());
    let shapes = shell.draw_sized(SMALLEST, Vec::new());
    let text = painted(&shapes);
    let (sentence, _) = text
        .iter()
        .find(|(_, text)| text.contains("becomes a source you can edit"))
        .expect("the sentence is painted");
    let rule = rule_end(&shell, &shapes, "Import built pack");
    assert!(
        sentence.max.x <= rule + 1.0,
        "the text runs to {} and the rule under the title stops at {rule}",
        sentence.max.x
    );
    let button = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Import as project"))
        .expect("the button");
    let accent = painted_fills(&shapes, theme::ACCENT.get());
    assert!(
        accent
            .iter()
            .any(|fill| fill.contains(button.bounds.center())),
        "the button that goes on is not filled as the others of the front door are"
    );
    let said = names(&shell);
    assert!(
        !said.iter().any(|n| n.contains("delete the new folder")),
        "the question sends the person to the file manager to undo: {said:?}"
    );
    assert!(matches!(
        shell.state.workbench.asking,
        Some(Question::Reopen { .. })
    ));
}

/// Every rectangle painted with `fill`.
fn painted_fills(shapes: &[ClippedShape], fill: egui::Color32) -> Vec<egui::Rect> {
    fn walk(shape: &Shape, fill: egui::Color32, out: &mut Vec<egui::Rect>) {
        match shape {
            Shape::Rect(rect) if rect.fill == fill => out.push(rect.rect),
            Shape::Vec(inner) => inner.iter().for_each(|shape| walk(shape, fill, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, fill, &mut out));
    out
}

#[test]
fn the_details_of_a_new_pack_say_what_the_name_is_for_what_the_game_changes_and_why_create_waits() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mut shell = a_first_launch();
    let mut draft = NewProject::new(Template::Clothing, dir.path().into());
    draft.step = Step::Details;
    shell.state.front_door.new_project = Some(draft);
    shell.settled_sized(SMALLEST);
    let said = names(&shell);
    for text in [
        "Give the clothing pack a name; it becomes the folder.",
        "The edition the pack is made for.",
        "Filled in from the name",
    ] {
        assert!(shows(&shell, text), "the form lacks `{text}`: {said:?}");
    }
    assert!(
        !shows(&shell, "Name the clothing pack"),
        "the hint only repeats the label above it: {said:?}"
    );
    let why = shell
        .state
        .front_door
        .new_project
        .as_ref()
        .and_then(|draft| draft.why_it_waits().map(str::to_string));
    assert_eq!(
        why.as_deref(),
        Some("Give the clothing pack a name; it becomes the folder."),
        "the button that waits does not say why"
    );
}
