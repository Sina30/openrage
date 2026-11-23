//! The Wardrobe's list column at the smallest window a person has: the list has the room, the header keeps its places, a slot's heading is a control that says so,
//! a filter lets go of the garments it hides, and the rules window says what it is for; all read through AccessKit from whole frames of the shell.
//!
//! ⚠ A frame is drawn at the editor's size unless a test says otherwise, so what clicks does so at that size and what measures the smallest window only measures.

use super::frames_of_the_shell::{Described, Shell};
use super::the_wardrobe_list_ergonomics_on_screen::{
    id_at, idle, read, rows, type_into, with_jackets,
};
use super::the_wardrobe_operations_on_screen::{
    a_clothing_shell, click_on, names, put, shows, until,
};
use crate::state::workbench::Question;
use crate::ui::panels::wardrobe::WardrobeUi;
use crate::ui::theme;

/// The smallest window the editor is made for.
const SMALLEST: (u32, u32) = (1280, 720);

/// What `name` is on screen: the widget, and not a label that says the same.
fn widget<'a>(shell: &'a Shell, name: &str) -> &'a Described {
    (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name) && w.role != egui::accesskit::Role::Label)
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
}

/// A click at `at` in a window of the smallest size.
fn click_small(shell: &mut Shell, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(SMALLEST, vec![egui::Event::PointerMoved(at), button(true)]);
    shell.draw_sized(SMALLEST, vec![button(false)]);
    shell.settled_sized(SMALLEST);
}

/// The text of the filter, set as typing it would.
fn filter(shell: &mut Shell, text: &str) {
    let mut ui = shell.state.panels.take::<WardrobeUi>();
    ui.filter = text.to_string();
    shell.state.panels.put(ui);
    shell.settled_sized(SMALLEST);
}

#[test]
fn the_list_has_its_rows_at_the_smallest_window_and_no_drop_box_stands_over_it_while_no_file_is_held(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 20);
    shell.settled_sized(SMALLEST);

    // Three rows were all the list had left; the header, a drop box and the figures had taken the rest.
    assert!(
        rows(&shell) >= 9,
        "{} rows: {:?}",
        rows(&shell),
        names(&shell)
    );
    assert!(
        !shows(&shell, "Drop models, textures and cloth here"),
        "a pack that has garments draws no drop box: the whole panel takes a drop"
    );
    // The build and its state are one row under the list, and the figures are one line over it.
    let chooser = widget(&shell, "Build the Build button makes").bounds;
    let state = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Not built") && w.bounds.min.x < 600.0)
        .unwrap_or_else(|| panic!("no state of the build: {:?}", names(&shell)))
        .bounds;
    assert!(
        (chooser.center().y - state.center().y).abs() < 4.0,
        "{chooser:?} and {state:?}"
    );
    assert!(
        // "No…" is half this wide in any font the interface is set in.
        state.width() >= 36.0,
        "the state of the build is whole, not cut to `No…`: {state:?}"
    );
    let all_figures = widget(&shell, "All figures").bounds;
    assert!(
        all_figures.max.y <= chooser.min.y,
        "the figures are over the build row"
    );
    // What was a paragraph of twelve figures names the memory, and the rest wait in the menu.
    assert!(shows(&shell, "Estimated memory: "), "{:?}", names(&shell));
    assert!(!shows(&shell, "clothing drawables"));
}

#[test]
fn the_header_keeps_its_places_whatever_the_last_change_was_and_whatever_the_filter_holds() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    shell.settled_sized(SMALLEST);
    let controls = [
        "Add clothes…",
        "Fix",
        "Versions",
        "Redo",
        "Filter garments",
        "Peds shown",
    ];
    let places = |shell: &Shell| controls.map(|name| widget(shell, name).bounds);
    let undo = |shell: &Shell| {
        (shell.described.iter())
            .find(|w| {
                w.role == egui::accesskit::Role::Button
                    && w.name.as_deref().is_some_and(|n| n.starts_with("Undo"))
                    && w.bounds.center().y < 110.0
            })
            .unwrap_or_else(|| panic!("no Undo: {:?}", names(shell)))
            .clone()
    };
    let (before, undone) = (places(&shell), undo(&shell));
    assert_eq!(undone.name.as_deref(), Some("Undo add (3)"));
    // The commands of the Wardrobe are in the row of tabs, on screen and above the list, where a click reaches them; the filter and the peds shown are in the panel. None was cut off.
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let screen = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(SMALLEST.0 as f32, SMALLEST.1 as f32),
    );
    let in_the_row = |at: egui::Rect| screen.contains_rect(at) && at.max.y <= panel.min.y;
    for (name, at) in controls.iter().zip(before) {
        let placed = match *name {
            "Filter garments" | "Peds shown" => panel.contains_rect(at),
            _ => in_the_row(at),
        };
        assert!(
            placed,
            "`{name}` at {at:?} is out of its place; the list is {panel:?}"
        );
    }
    assert!(in_the_row(undone.bounds), "Undo at {:?}", undone.bounds);

    // A change of another kind renames the button, and a term in the filter brings the bar of queries in under the header; nothing moves.
    let id = id_at(&shell, 1);
    shell.state.edit_garment(
        id,
        or_wardrobe::edit::Edit::Label(Some("Red jacket".into())),
    );
    until(&mut shell, "the name", |s| {
        shows(s, "Red jacket") && idle(s)
    });
    shell.settled_sized(SMALLEST);
    let renamed = undo(&shell);
    assert_ne!(renamed.name, undone.name, "the name says the new change");
    assert_eq!(renamed.bounds, undone.bounds, "its place does not move");
    for text in ["tag:wool", "jbib", "-wool", ""] {
        filter(&mut shell, text);
        assert_eq!(places(&shell), before, "with `{text}` in the filter");
    }
    // The first row holds the actions and the second the filter, and none of the controls wrapped under another.
    let (first, second) = (
        widget(&shell, "Add clothes…").bounds,
        widget(&shell, "Filter garments").bounds,
    );
    for name in ["Fix", "Versions", "Redo"] {
        let at = widget(&shell, name).bounds.center().y;
        assert!((at - first.center().y).abs() < 3.0, "{name}");
    }
    let ped = widget(&shell, "Peds shown").bounds.center().y;
    assert!((ped - second.center().y).abs() < 3.0);
    assert!(
        second.min.y > first.max.y,
        "the filter is under the actions"
    );
}

#[test]
fn a_filter_lets_go_of_the_garments_picked_that_it_hides_and_the_panel_counts_what_the_list_shows()
{
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 6);

    click_on(&mut shell, "Male · Tops · 6 garments");
    assert!(shows(&shell, "6 garments selected"), "{:?}", names(&shell));
    // With several selected the panel offers what is set on all of them and none of what is one garment's.
    assert!(shows(&shell, "Tags and group, set on all 6 selected"));
    for one in ["Garment name", "Position", "Replace model…"] {
        assert!(
            !shows(&shell, one),
            "`{one}` is one garment's: {:?}",
            names(&shell)
        );
    }
    for place in [1, 4] {
        let id = id_at(&shell, place);
        shell
            .state
            .edit_garment(id.clone(), or_wardrobe::edit::Edit::AddTag("wool".into()));
        until(&mut shell, "the tag", |s| {
            idle(s)
                && read(s)
                    .wardrobe
                    .garment(&id)
                    .is_some_and(|g| g.tags.iter().any(|t| t == "wool"))
        });
    }

    type_into(&mut shell, "Filter garments", "tag:wool");
    assert_eq!(rows(&shell), 2, "{:?}", names(&shell));
    assert_eq!(
        shell.state.picked_garments().len(),
        2,
        "what is picked is what the list shows"
    );
    assert!(shows(&shell, "2 garments selected"), "{:?}", names(&shell));
    assert!(!shows(&shell, "6 garments selected"));
}

#[test]
fn rows_of_components_are_named_apart_from_a_hat_and_a_slot_heading_says_its_garments() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", |s| {
        s.state.wardrobe_pack().is_some()
    });
    let (c, hat) = ("mp_m_freemode_01_mp_m_demo", "mp_m_freemode_01_p_mp_m_demo");
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let dropped = put(
        dir.path(),
        &[
            (&format!("{c}^jbib_000_u.ydd"), b"jacket 0"),
            (&format!("{c}^jbib_diff_000_a_uni.ytd"), b"red"),
            (&format!("{c}^jbib_001_u.ydd"), b"jacket 1"),
            (&format!("{c}^jbib_diff_001_a_uni.ytd"), b"blue"),
            (&format!("{hat}^p_head_000.ydd"), b"a hat"),
            (&format!("{hat}^p_head_diff_000_a.ytd"), b"straw"),
        ],
    );
    shell.drop_files(panel.center(), dropped);
    until(&mut shell, "the garments", |s| {
        shows(s, "Male · Tops") && shows(s, "Male · Hats")
    });
    shell.settled();

    // The hat's file is named `p_` for its collection, which kept the whole collection's name on every top.
    let said = names(&shell);
    for top in ["000, place 000", "001, place 001"] {
        assert!(
            said.iter().any(|n| n.starts_with(top)),
            "a top is named by what the other tops do not say, `{top}`: {said:?}"
        );
    }
    assert!(
        !said
            .iter()
            .any(|n| n.starts_with("mp_m_freemode_01_mp_m_demo")),
        "{said:?}"
    );
    // The heading says what it holds and is a control, with a button at its end for its menu.
    assert!(shows(&shell, "Male · Tops · 2 garments"));
    assert!(
        !shows(&shell, " of 128"),
        "the cap is said only where it matters"
    );
    assert_eq!(
        widget(&shell, "Male · Tops · 2 garments").role,
        egui::accesskit::Role::Button
    );
    click_on(&mut shell, "Actions for Male · Tops · 2 garments");
    for words in [
        "Set the ped to Female for these 2 garments",
        "Set the slot for these 2 garments",
        "Export the files of these 2 garments…",
        "Fill the gaps in the numbering with empty places",
        "Move the empty places to the end",
        "Remove these 2 garments from the pack (their files stay)",
    ] {
        assert!(shows(&shell, words), "`{words}`: {:?}", names(&shell));
    }
    for old in ["Say these", "holes", "dummies"] {
        assert!(
            !shows(&shell, old),
            "`{old}` is a word somebody had to decode: {:?}",
            names(&shell)
        );
    }
    assert!(
        widget(&shell, "Move the empty places to the end").disabled,
        "no empty place is held, so there is none to move"
    );
}

#[test]
fn the_rules_window_has_two_headed_sections_a_title_for_each_check_and_waits_for_a_change_to_apply()
{
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", |s| {
        s.state.wardrobe_pack().is_some()
    });
    let module = shell.state.wardrobe_module_id().unwrap();
    shell.state.ask_about_rules(module);
    shell.settled_sized(SMALLEST);

    for text in [
        "What a build does",
        "Checks",
        "How serious",
        "Garment moved since the last build",
        "Texture without mipmaps",
    ] {
        assert!(shows(&shell, text), "`{text}`: {:?}", names(&shell));
    }
    assert!(
        !shows(&shell, "wardrobe.json"),
        "the window says the pack's settings: {:?}",
        names(&shell)
    );
    assert!(widget(&shell, "Apply").disabled, "nothing is changed yet");
    assert!(
        widget(&shell, "Reset all checks").disabled,
        "every check is at its default"
    );
    // The window leaves the strip of workspaces and the status bar in sight.
    let title = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Rules of the clothing"))
        .map(|w| w.bounds)
        .unwrap_or_else(|| panic!("no title: {:?}", names(&shell)));
    let apply = widget(&shell, "Apply").bounds;
    assert!(title.min.y > 52.0, "{title:?}");
    assert!(apply.max.y < 696.0, "{apply:?}");

    // A check that is changed wakes Apply and the two Resets; Reset all puts every check back and Apply sleeps again.
    if let Some(Question::Rules { draft, .. }) = &mut shell.state.workbench.asking {
        draft.set("texture-mips", |s| s.on = Some(false));
    }
    shell.settled_sized(SMALLEST);
    assert!(!widget(&shell, "Apply").disabled, "{:?}", names(&shell));
    assert!(!widget(&shell, "Reset: Texture without mipmaps").disabled);
    let all = widget(&shell, "Reset all checks");
    assert!(!all.disabled);
    let at = all.bounds.center();
    click_small(&mut shell, at);
    assert!(widget(&shell, "Apply").disabled, "{:?}", names(&shell));
    assert!(widget(&shell, "Reset all checks").disabled);
}
