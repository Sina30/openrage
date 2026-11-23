//! The Wardrobe's list column, Garment panel and Rules window as a second critique found them: what a menu acts on is what is
//! highlighted, several garments show the tags they really share, an empty pack is one drop area, a filter in force is said and can be
//! cleared, the header names its menus in words, nothing the Garment panel adds runs past its edge, and the rules window reads in whole
//! sentences; all read through AccessKit and the text the frame paints, from whole frames of the shell at the smallest window.

use egui::epaint::ClippedShape;

use super::frames_of_the_shell::{Described, Shell};
use super::the_wardrobe_list_ergonomics_on_screen::{
    id_at, idle, read, rows, type_into, with_jackets,
};
use super::the_wardrobe_operations_on_screen::{a_clothing_shell, click_on, names, shows, until};
use crate::ui::theme;

/// The smallest window the editor is made for.
const SMALLEST: (u32, u32) = (1280, 720);

/// What `name` is on screen: the widget, and not a label that says the same.
fn widget<'a>(shell: &'a Shell, name: &str) -> &'a Described {
    (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name) && w.role != egui::accesskit::Role::Label)
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
}

/// How many labels say exactly `text`.
fn labels(shell: &Shell, text: &str) -> usize {
    (shell.described.iter())
        .filter(|w| w.role == egui::accesskit::Role::Label && w.name.as_deref() == Some(text))
        .count()
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

/// The text of every galley a frame painted.
fn painted(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            _ => {}
        }
    }
    let mut found = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut found));
    found
}

/// What a frame says after the pointer rests on `at` long enough for a tooltip.
fn rest_on(shell: &mut Shell, at: egui::Pos2) -> Vec<String> {
    shell.draw_sized(SMALLEST, vec![egui::Event::PointerMoved(at)]);
    let mut shapes = Vec::new();
    for _ in 0..45 {
        shapes = shell.draw_sized(SMALLEST, Vec::new());
    }
    painted(&shapes)
}

#[test]
fn a_headings_menu_picks_what_it_acts_on_says_its_scope_and_greys_what_would_change_nothing() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 6);
    let module = shell.state.wardrobe_module_id().unwrap();
    for place in [0, 2, 3] {
        let id = id_at(&shell, place);
        shell.state.toggle_garment(&module, id);
    }
    shell.settled_sized(SMALLEST);
    assert!(shows(&shell, "3 garments selected"), "{:?}", names(&shell));

    let button = widget(&shell, "Actions for Male · Tops · 6 garments")
        .bounds
        .center();
    click_small(&mut shell, button);
    assert_eq!(
        shell.state.picked_garments().len(),
        6,
        "the menu of a heading acts on all six, so all six are what is picked"
    );
    assert!(shows(&shell, "6 garments selected"), "{:?}", names(&shell));
    // The heading's own label and the first line of its menu both say what the menu acts on.
    assert_eq!(
        labels(&shell, "Male · Tops · 6 garments"),
        2,
        "{:?}",
        names(&shell)
    );
    assert!(
        widget(&shell, "Set the ped to Male for these 6 garments").disabled,
        "all six are Male already, so the item changes nothing"
    );
    assert!(!widget(&shell, "Set the ped to Female for these 6 garments").disabled);
}

#[test]
fn several_garments_show_each_tag_as_on_all_or_on_some_and_a_group_that_differs_reads_as_several() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    let ids: Vec<String> = (0..3).map(|place| id_at(&shell, place)).collect();
    shell.state.tag_garments(ids.clone(), "wool".into());
    until(&mut shell, "the first tag", |s| {
        idle(s) && read(s).wardrobe.garments.iter().all(|g| g.tags == ["wool"])
    });
    shell
        .state
        .tag_garments(vec![ids[0].clone()], "formal".into());
    until(&mut shell, "the second tag", |s| {
        idle(s) && read(s).wardrobe.garment(&ids[0]).unwrap().tags.len() == 2
    });
    shell
        .state
        .group_garments(vec![ids[0].clone()], Some("Smart".into()));
    until(&mut shell, "the group", |s| {
        idle(s) && read(s).wardrobe.garment(&ids[0]).unwrap().group.as_deref() == Some("Smart")
    });

    click_on(&mut shell, "Male · Tops · 3 garments");
    assert!(shows(&shell, "3 garments selected"), "{:?}", names(&shell));
    assert!(
        shows(&shell, "Remove tag wool") && !shows(&shell, "Remove tag wool (on"),
        "wool is on all three: {:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "Remove tag formal (on 1 of 3)"),
        "formal is on one of the three, and the panel says so: {:?}",
        names(&shell)
    );
    let shapes = shell.settled();
    assert!(
        painted(&shapes).iter().any(|t| t == "Several"),
        "the groups differ, which the field reads as Several: {:?}",
        painted(&shapes)
    );
    // The group stands in the column of the form's labels, under the ped and the slot.
    let (ped, group) = (
        widget(&shell, "Garment ped").bounds,
        widget(&shell, "Group").bounds,
    );
    assert!(
        (ped.min.x - group.min.x).abs() < 2.0,
        "{ped:?} and {group:?}"
    );
}

#[test]
fn an_empty_pack_is_one_drop_area_with_no_filters_of_nobody_and_no_counts_of_nothing() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", |s| {
        s.state.wardrobe_pack().is_some()
    });
    until(&mut shell, "the scan", |s| s.state.workbench.is_scanned());
    shell.settled_sized(SMALLEST);

    for gone in [
        "Filter garments",
        "Peds shown",
        "Estimated memory",
        "All figures",
        "Alternate variations",
        "Name shop entries as Durty Cloth Tool does",
        "Waivers",
        "in the metadata it plans",
        "every file is there",
    ] {
        assert!(
            !shows(&shell, gone),
            "an empty pack shows `{gone}`: {:?}",
            names(&shell)
        );
    }
    for stays in [
        "Add clothes…",
        "Drop models, textures and cloth here",
        "Pack name",
        "Add package…",
        "Pack settings",
        "Nothing to check yet",
    ] {
        assert!(
            shows(&shell, stays),
            "an empty pack lacks `{stays}`: {:?}",
            names(&shell)
        );
    }
}

#[test]
fn a_filter_in_force_is_said_and_cleared_where_it_is_typed_and_saved_queries_are_one_menu() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 6);
    shell.settled_sized(SMALLEST);
    let ids: Vec<String> = (0..6).map(|place| id_at(&shell, place)).collect();
    for place in [1, 4] {
        shell
            .state
            .tag_garments(vec![ids[place].clone()], "wool".into());
    }
    until(&mut shell, "the tags", |s| {
        idle(s)
            && read(s)
                .wardrobe
                .garments
                .iter()
                .filter(|g| g.tags.iter().any(|t| t == "wool"))
                .count()
                == 2
    });
    assert!(shows(&shell, "6 garments"), "{:?}", names(&shell));
    assert!(!shows(&shell, "Clear the filter"));

    type_into(&mut shell, "Filter garments", "tag:wool");
    assert_eq!(rows(&shell), 2, "{:?}", names(&shell));
    assert!(
        shows(&shell, "2 of 6 garments"),
        "the counts say that a filter is in force: {:?}",
        names(&shell)
    );
    click_on(&mut shell, "Clear the filter");
    assert_eq!(rows(&shell), 6, "{:?}", names(&shell));
    assert!(!shows(&shell, "of 6 garments"));

    type_into(&mut shell, "Filter garments", "zzzz");
    assert_eq!(rows(&shell), 0);
    assert!(
        shows(&shell, "zzzz") && shows(&shell, "No garment matches"),
        "an empty list says what is on: {:?}",
        names(&shell)
    );
    click_on(&mut shell, "Clear filters");
    assert_eq!(rows(&shell), 6, "{:?}", names(&shell));

    // Two saved queries are one menu, whose button names what it holds, and not a row of chips for each.
    let module = shell.state.wardrobe_module_id().unwrap();
    shell
        .state
        .save_query(module.clone(), "Wool".into(), "tag:wool".into());
    until(&mut shell, "the first query", |s| {
        idle(s) && read(s).wardrobe.queries.len() == 1
    });
    shell
        .state
        .save_query(module, "Winter".into(), "tag:winter".into());
    until(&mut shell, "the second query", |s| {
        idle(s) && read(s).wardrobe.queries.len() == 2
    });
    shell.settled_sized(SMALLEST);
    assert!(
        !shows(&shell, "Saved query: Wool"),
        "no chip for each query: {:?}",
        names(&shell)
    );
    let menu = widget(&shell, "Saved queries").bounds.center();
    click_small(&mut shell, menu);
    assert!(
        shows(&shell, "Saved query: Wool") && shows(&shell, "Saved query: Winter"),
        "{:?}",
        names(&shell)
    );
}

#[test]
fn the_header_names_its_menus_in_words_at_the_smallest_window_and_add_clothes_offers_files_or_a_folder(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    let shapes = shell.settled_sized(SMALLEST);
    let said = painted(&shapes);
    for word in ["Add clothes…", "Fix", "Versions"] {
        assert!(
            said.iter().any(|t| t.trim() == word),
            "`{word}` is not written on a button of the header: {said:?}"
        );
    }
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    for name in ["Add clothes…", "Fix", "Versions", "Redo"] {
        let at = widget(&shell, name).bounds;
        // The commands are in the row of tabs, above the list and on screen.
        assert!(
            at.max.y <= panel.min.y && at.min.x >= 0.0 && at.max.x <= SMALLEST.0 as f32,
            "`{name}` at {at:?}, the list being {panel:?}"
        );
    }
    assert!(
        !shows(&shell, "Add a folder of clothes…"),
        "the folder is not a second button of the header"
    );
    let add = widget(&shell, "Add clothes…").bounds.center();
    click_small(&mut shell, add);
    assert!(
        shows(&shell, "Files…") && shows(&shell, "Folder…"),
        "Add clothes… offers the files and the folder: {:?}",
        names(&shell)
    );
}

#[test]
fn what_a_shop_entry_adds_stands_inside_the_garment_panel_and_its_other_elements_are_folded_away() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 2);
    let module = shell.state.wardrobe_module_id().unwrap();
    let id = id_at(&shell, 0);
    shell.state.select_garment(&module, Some(id));
    shell.settled_sized(SMALLEST);
    let open = |shell: &mut Shell, text: &str| {
        let at = (shell.described.iter())
            .find(|w| w.name.as_deref().is_some_and(|n| n.starts_with(text)))
            .unwrap_or_else(|| panic!("nothing starts `{text}`: {:?}", names(shell)))
            .bounds
            .center();
        click_small(shell, at);
    };
    // The panel is scrolled down with the wheel, as far as the shop section goes.
    let scroll = |shell: &mut Shell| {
        for _ in 0..8 {
            shell.draw_sized(
                SMALLEST,
                vec![
                    egui::Event::PointerMoved(egui::pos2(1130.0, 400.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -400.0),
                        modifiers: egui::Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                    },
                ],
            );
        }
        shell.settled_sized(SMALLEST);
    };
    scroll(&mut shell);
    open(&mut shell, "Entry of texture a");
    assert!(
        !shows(&shell, "Name of a new element of texture a"),
        "the raw elements are folded away until asked for: {:?}",
        names(&shell)
    );
    scroll(&mut shell);
    open(&mut shell, "Other elements");
    for name in [
        "Shop id of texture a",
        "Name of a new element of texture a",
        "XML of a new element of texture a",
        "Add the element to texture a",
    ] {
        let at = widget(&shell, name).bounds;
        assert!(
            at.max.x <= 1275.0,
            "`{name}` runs past the edge of the panel: {at:?}"
        );
    }
}

#[test]
fn the_rules_window_names_the_pack_shows_whole_sentences_aligns_its_header_and_offers_reset_only_where_a_rule_differs(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", |s| {
        s.state.wardrobe_pack().is_some()
    });
    let module = shell.state.wardrobe_module_id().unwrap();
    let pack = read(&shell).wardrobe.pack.clone();
    shell.state.ask_about_rules(module);
    let shapes = shell.settled_sized(SMALLEST);

    assert!(
        shows(&shell, &format!("Rules for the pack {pack}")),
        "{:?}",
        names(&shell)
    );
    assert!(!shows(&shell, "What the content"), "{:?}", names(&shell));
    // The words over the column of choices end where the choices end, and not over the buttons beside them.
    let how = (shell.described.iter())
        .find(|w| {
            w.role == egui::accesskit::Role::Label && w.name.as_deref() == Some("How serious")
        })
        .expect("the header of the column")
        .bounds;
    let choice = (shell.described.iter())
        .find(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.starts_with("How serious:"))
        })
        .expect("a choice of how serious")
        .bounds;
    assert!(
        (how.max.x - choice.max.x).abs() < 8.0,
        "{how:?} over {choice:?}"
    );
    // No sentence of the window is cut short with an ellipsis.
    let title = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Rules of the clothing"))
        .map(|w| w.bounds)
        .expect("the title of the window");
    let apply = widget(&shell, "Apply").bounds;
    let mut cut = Vec::new();
    fn walk(shape: &egui::Shape, inside: egui::Rect, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(all) => all.iter().for_each(|s| walk(s, inside, out)),
            egui::Shape::Text(text) if text.galley.elided && inside.contains(text.pos) => {
                out.push(text.galley.text().to_string())
            }
            _ => {}
        }
    }
    let inside = egui::Rect::from_min_max(title.min, egui::pos2(title.min.x + 640.0, apply.min.y));
    shapes.iter().for_each(|c| walk(&c.shape, inside, &mut cut));
    assert!(cut.is_empty(), "sentences cut short: {cut:?}");

    // Reset is there where a rule is not at its default and nowhere else.
    assert!(!shows(&shell, "Reset: "), "{:?}", names(&shell));
    if let Some(crate::state::workbench::Question::Rules { draft, .. }) =
        &mut shell.state.workbench.asking
    {
        draft.set("texture-mips", |s| s.on = Some(false));
    }
    shell.settled_sized(SMALLEST);
    assert!(shows(&shell, "Reset: Texture without mipmaps"));
    assert_eq!(
        (shell.described.iter())
            .filter(|w| w.name.as_deref().is_some_and(|n| n.starts_with("Reset: ")))
            .count(),
        1
    );
}

#[test]
fn a_waiver_says_why_it_cannot_be_recorded_yet_and_a_disabled_control_of_the_garment_panel_says_why(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 1);
    let module = shell.state.wardrobe_module_id().unwrap();
    let id = id_at(&shell, 0);
    shell.state.select_garment(&module, Some(id));
    shell.settled_sized(SMALLEST);
    // The Add of a tag waits for a tag, and a tip says so.
    let add = widget(&shell, "Add tag");
    assert!(add.disabled);
    let at = add.bounds.center();
    let said = rest_on(&mut shell, at);
    assert!(
        said.iter().any(|t| t.contains("Type a tag first")),
        "the disabled Add says why: {said:?}"
    );

    // A waiver asks for its reason in a visible line while it has none.
    let finding = read(&shell)
        .findings
        .first()
        .cloned()
        .expect("a garment with one texture still has a finding to waive");
    shell
        .state
        .ask_about_fix(&crate::state::workbench::Fix::Waive { module, finding });
    shell.settled_sized(SMALLEST);
    assert!(widget(&shell, "Record the waiver").disabled);
    assert!(shows(&shell, "A reason is needed"), "{:?}", names(&shell));
    type_into(&mut shell, "Reason for the waiver", "the shop wants it");
    assert!(!widget(&shell, "Record the waiver").disabled);
    assert!(!shows(&shell, "A reason is needed"));
}
