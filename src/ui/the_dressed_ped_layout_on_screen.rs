//! The dressed ped's panel at the smallest window the editor allows: the view has the room, nothing is drawn over the tiles under it, the tools stand in
//! rows that keep their places, one hour of the day has one name, the version shown is named in words, and the view from the head says what it is.
//!
//! ⚠ Read through AccessKit from whole frames of the shell, where each widget was drawn.

use super::frames_of_the_shell::{key, Shell};
use super::the_wardrobe_operations_on_screen::{
    a_shell_with_two_jackets, click_on, names, shows, until,
};
use crate::state::workbench::AlternatesEdit;
use crate::ui::theme;

/// The smallest window the editor allows, in points.
const SMALLEST: (u32, u32) = crate::app::EDITOR_MIN_SIZE;

/// The view and the tiles are read at this window, the way a laptop shows them.
fn at_the_smallest_window(dir: &std::path::Path) -> Shell {
    let (mut shell, _) = a_shell_with_two_jackets(dir);
    shell.settled_sized(SMALLEST);
    shell
}

fn bounds_of(shell: &Shell, name: &str) -> egui::Rect {
    (shell.described.iter())
        .filter(|w| w.name.as_deref() == Some(name))
        .map(|w| w.bounds)
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
}

/// Where the control `name` stands in the column of the dressed ped, whose view is `view`: the Garment panel and the list name some alike.
fn in_column(shell: &Shell, name: &str, view: egui::Rect) -> egui::Rect {
    (shell.described.iter())
        .filter(|w| w.name.as_deref() == Some(name))
        .map(|w| w.bounds)
        .find(|b| b.center().x > view.left() && b.center().x < view.right())
        .unwrap_or_else(|| {
            panic!(
                "nothing in the column is named `{name}`: {:?}",
                names(shell)
            )
        })
}

/// The height of the row `controls` stand in, all of them on one line.
fn one_row(shell: &Shell, controls: &[&str]) -> f32 {
    let view = bounds_of(shell, "Dressed ped");
    let ys: Vec<f32> = controls
        .iter()
        .map(|name| in_column(shell, name, view).center().y)
        .collect();
    let (low, high) = (
        ys.iter().cloned().fold(f32::MAX, f32::min),
        ys.iter().cloned().fold(f32::MIN, f32::max),
    );
    assert!(
        high - low < 6.0,
        "{controls:?} should stand on one line, but stand at {ys:?}"
    );
    low
}

#[test]
fn the_view_has_the_room_at_the_smallest_window_and_is_not_drawn_over_the_tiles() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let shell = at_the_smallest_window(dir.path());
    let view = bounds_of(&shell, "Dressed ped");
    assert!(
        view.height() >= 200.0,
        "the dressed ped is the point of the panel: its view is {} points tall at {SMALLEST:?}",
        view.height()
    );
    let tiles: Vec<egui::Rect> = (shell.described.iter())
        .filter(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Texture ") && n.contains("Click to wear it"))
        })
        .map(|w| w.bounds)
        .collect();
    assert!(
        !tiles.is_empty(),
        "the jacket has textures: {:?}",
        names(&shell)
    );
    for tile in tiles {
        assert!(
            !view.intersects(tile),
            "the view {view:?} is drawn over a texture tile {tile:?}"
        );
    }
}

#[test]
fn the_tools_stand_in_rows_by_meaning_that_do_not_reflow() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let shell = at_the_smallest_window(dir.path());
    let wear = one_row(&shell, &["Male", "Skin", "Outfit", "Heels"]);
    let show = one_row(
        &shell,
        &[
            "Without the pack",
            "With the pack",
            "Before the last change",
        ],
    );
    let draw = one_row(&shell, &["Draw", "Level", "Tint"]);
    let parts = one_row(
        &shell,
        &["Ped without hair", "Ped's own parts", "Motion", "Pictures"],
    );
    assert!(
        wear < show && show < draw && draw < parts,
        "who wears it {wear}, which version {show}, how it is drawn {draw}, the parts and the motion {parts}"
    );
    let view = bounds_of(&shell, "Dressed ped");
    assert!(parts < view.top(), "the tools stand above the view");
    for gone in ["Daylight", "A/B", "Hour", "Ped's own"] {
        assert!(
            !(shell.described.iter()).any(|w| w.name.as_deref() == Some(gone)),
            "`{gone}` is a name of the old toolbar"
        );
    }
    // The helpers of the view stand under it, and the camera is placed from there with a name a person can read.
    let helpers = one_row(&shell, &["Bones", "Grid", "First person", "Light"]);
    assert!(
        helpers > view.bottom(),
        "the view helpers stand under the view"
    );
    assert!(shows(&shell, "Frame the ped"));
}

#[test]
fn the_versions_are_named_in_words_and_one_is_chosen_at_a_time() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    // Without the install there is no ped to leave out, so the choice is greyed.
    let game = crate::state::prefs::Game::GtaVLegacy;
    shell
        .state
        .prefs
        .game_paths
        .set(game, Some(dir.path().join("GTAV")));
    shell.state.prefs.active_game = Some(game);
    shell.settled();
    let dressed = |shell: &Shell| {
        let d = &shell.state.workbench.wardrobe.dressed;
        (d.original, d.previous)
    };
    assert_eq!(
        dressed(&shell),
        (false, false),
        "the pack is shown as it is"
    );
    click_on(&mut shell, "Without the pack");
    assert_eq!(dressed(&shell), (true, false));
    click_on(&mut shell, "With the pack");
    assert_eq!(dressed(&shell), (false, false));
    let chosen = |shell: &Shell, name: &str| {
        (shell.described.iter())
            .find(|w| w.name.as_deref() == Some(name))
            .is_some_and(|w| w.role == egui::accesskit::Role::RadioButton)
    };
    for name in [
        "Without the pack",
        "With the pack",
        "Before the last change",
    ] {
        assert!(chosen(&shell, name), "`{name}` is one choice among three");
    }
}

/// Whether `inner` lies inside `outer` side to side.
fn within_sideways(inner: egui::Rect, outer: egui::Rect) -> bool {
    outer.left() <= inner.left() && inner.right() <= outer.right()
}

/// Whether a filled rectangle about the size of `bounds` was painted behind it: a control that is drawn with a frame, and not a line of text on the panel.
fn has_a_fill(shapes: &[egui::epaint::ClippedShape], bounds: egui::Rect) -> bool {
    shapes.iter().any(|clipped| match &clipped.shape {
        egui::Shape::Rect(rect) => {
            rect.fill.a() > 0
                && rect.rect.contains(bounds.center())
                && rect.rect.area() <= bounds.area() * 1.6
        }
        _ => false,
    })
}

#[test]
fn the_model_and_the_alternates_stand_on_a_row_of_their_own_inside_the_panel_at_the_smallest_window(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    shell.state.edit_alternates(AlternatesEdit::New {
        ped: or_wardrobe::Ped::Male,
        name: "Hat and hair".into(),
        join: None,
    });
    let folder = shell.state.wardrobe_pack().unwrap().folder.clone();
    or_wardrobe::edit::update(&folder, |w| {
        w.garments[0].alternates = vec!["models/jacket_1.ydd".into()];
        Ok(())
    })
    .unwrap();
    shell.state.workbench.want_scan();
    until(&mut shell, "the two choices", |s| {
        shows(s, "Alternates") && shows(s, "Model")
    });
    shell.settled_sized(SMALLEST);
    let view = bounds_of(&shell, "Dressed ped");
    // ⚠ The edge is the panel's own, which its tab stands at: a row that runs past it widens the view with it, so the view is no measure.
    let panel = (shell.described.iter())
        .find(|w| {
            w.role == egui::accesskit::Role::Button && w.name.as_deref() == Some("Dressed ped")
        })
        .expect("the tab of the panel")
        .bounds;
    for name in ["Model", "Alternates", "Frame the ped", "Heels", "Outfit"] {
        let at = in_column(&shell, name, view);
        assert!(
            within_sideways(at, panel),
            "`{name}` at {at:?} runs past the panel {panel:?}"
        );
    }
    assert!(
        within_sideways(view, panel),
        "the view is as wide as the panel: {view:?} in {panel:?}"
    );
    // The first row is who wears it, as it was with no alternates; the two drop-downs are the row under it.
    let wear = one_row(&shell, &["Male", "Skin", "Outfit", "Heels"]);
    let choices = one_row(&shell, &["Model", "Alternates"]);
    let show = one_row(&shell, &["Without the pack", "With the pack"]);
    assert!(
        wear < choices && choices < show,
        "who wears it {wear}, its models {choices}, which version {show}"
    );
}

#[test]
fn the_controls_of_a_clip_that_plays_stand_under_the_view_and_not_over_it() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    crate::state::workbench::a_clip_playing(
        &mut shell.state.workbench.wardrobe.dressed,
        or_wardrobe::Ped::Male,
    );
    shell.settled_sized(SMALLEST);
    let view = bounds_of(&shell, "Dressed ped");
    assert!(
        view.height() >= 180.0,
        "the view keeps its height while a clip plays: {} points",
        view.height()
    );
    for name in ["Pause", "Loop", "Speed", "Clip"] {
        let at = in_column(&shell, name, view);
        assert!(
            !view.intersects(at) && at.top() >= view.bottom(),
            "`{name}` at {at:?} is over the view {view:?}"
        );
    }
}

#[test]
fn no_game_install_is_said_over_the_view_with_the_way_to_choose_one_and_what_needs_it_is_greyed() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    assert!(
        shell.state.prefs.active_game_root().is_none(),
        "the test shell has no install"
    );
    shell.settled_sized(SMALLEST);
    let view = bounds_of(&shell, "Dressed ped");
    let said = (shell.described.iter())
        .filter(|w| {
            w.name
                .as_deref()
                .is_some_and(|n| n.contains(crate::state::workbench::NO_GAME))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        said.len(),
        1,
        "said once, over the view: {:?}",
        names(&shell)
    );
    assert!(view.contains_rect(said[0].bounds), "{:?}", said[0].bounds);
    let choose = in_column(&shell, "Choose a game install…", view);
    assert!(view.contains_rect(choose), "{choose:?} is not in {view:?}");
    for name in ["Skin", "Without the pack"] {
        // The Garment panel has a Skin of its own, which does not need the install.
        let widget = (shell.described.iter())
            .find(|w| {
                w.name.as_deref() == Some(name)
                    && w.role != egui::accesskit::Role::Label
                    && w.bounds.center().x > view.left()
                    && w.bounds.center().x < view.right()
            })
            .unwrap_or_else(|| panic!("nothing in the column is named `{name}`"));
        assert!(
            widget.disabled,
            "`{name}` acts on a ped that is not drawn without the install"
        );
    }
    for gone in ["mounted", "Mounted"] {
        assert!(
            !names(&shell).iter().any(|n| n.contains(gone)),
            "`{gone}` is not a word a person reads: {:?}",
            names(&shell)
        );
    }
}

#[test]
fn the_draw_menu_offers_shaded_only_once_the_game_shaders_are_baked() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    click_on(&mut shell, "Draw");
    let radio = |shell: &Shell, name: &str| {
        (shell.described.iter())
            .find(|w| {
                w.name.as_deref() == Some(name) && w.role == egui::accesskit::Role::RadioButton
            })
            .cloned()
            .unwrap_or_else(|| panic!("`{name}` is not offered: {:?}", names(shell)))
    };
    for mode in crate::state::workbench::ViewMode::ALL {
        let row = radio(&shell, mode.name());
        assert_eq!(
            row.disabled,
            mode == crate::state::workbench::ViewMode::Shaded,
            "{}: Shaded draws as Material until the shaders are baked, so it waits for them",
            mode.name()
        );
    }
}

#[test]
fn the_light_menu_shows_only_the_light_that_lights_the_view_and_its_controls_are_drawn_as_controls()
{
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    assert!(!shows(&shell, "Daylight"), "no second light button");
    let count = |shell: &Shell, name: &str| {
        (shell.described.iter())
            .filter(|w| w.name.as_deref() == Some(name) && w.role != egui::accesskit::Role::Label)
            .count()
    };

    // A plain view is lit by the studio light, from a direction set by hand.
    click_on(&mut shell, "Light");
    for text in [
        "Studio light",
        "Light from a direction I set",
        "Light from the sun at the time of day",
        "Light direction",
        "Light height",
        "Light strength",
        "Reset light",
    ] {
        assert!(shows(&shell, text), "`{text}`: {:?}", names(&shell));
    }
    for gone in [
        "Game daylight",
        "Shadows",
        "Time of day",
        "These only light the Shaded view.",
        "Switch Draw to Shaded",
    ] {
        assert!(
            !shows(&shell, gone),
            "`{gone}` lights nothing in this view: {:?}",
            names(&shell)
        );
    }
    // Moved, the light can be put back, and that is a button with a frame.
    shell.state.workbench.wardrobe.dressed.light.azimuth += 20.0;
    let shapes = shell.settled();
    let reset = bounds_of(&shell, "Reset light");
    assert!(has_a_fill(&shapes, reset), "Reset light is drawn as text");

    // From the sun, the hour is what sets the direction, so it stands in the place of the two sliders.
    click_on(&mut shell, "Light from the sun at the time of day");
    click_on(&mut shell, "Light");
    assert_eq!(count(&shell, "Time of day"), 1, "{:?}", names(&shell));
    assert_eq!(count(&shell, "Light direction"), 0);
    assert_eq!(count(&shell, "Hour the sun is lit at"), 0);

    // The Shaded view is lit by the game's daylight, and the way back to the studio light is a button.
    shell.state.workbench.wardrobe.dressed.view = crate::state::workbench::ViewMode::Shaded;
    shell.settled();
    let shapes = shell.settled();
    for text in [
        "Game daylight",
        "Time of day",
        "Shadows",
        "Switch Draw to Material",
    ] {
        assert!(shows(&shell, text), "`{text}`: {:?}", names(&shell));
    }
    for gone in ["Studio light", "Light direction", "Reset light"] {
        assert!(!shows(&shell, gone), "`{gone}`: {:?}", names(&shell));
    }
    let switch = bounds_of(&shell, "Switch Draw to Material");
    assert!(has_a_fill(&shapes, switch), "the switch is drawn as text");
    click_on(&mut shell, "Switch Draw to Material");
    assert_eq!(
        shell.state.workbench.wardrobe.dressed.view,
        crate::state::workbench::ViewMode::Material
    );
}

#[test]
fn the_view_from_the_head_says_what_it_is_and_escape_leaves_it() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _) = a_shell_with_two_jackets(dir.path());
    assert!(!shows(&shell, "First person: drag to look round"));
    shell.state.workbench.wardrobe.dressed.first_person = true;
    shell.settled();
    assert!(
        shows(&shell, "First person: drag to look round"),
        "{:?}",
        names(&shell)
    );
    let view = bounds_of(&shell, "Dressed ped");
    let chip = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Leave first person"))
        .expect("the way out is a button in the view")
        .bounds;
    assert!(view.contains_rect(chip), "{chip:?} is not in {view:?}");

    click_on(&mut shell, "Leave first person");
    assert!(!shell.state.workbench.wardrobe.dressed.first_person);

    shell.state.workbench.wardrobe.dressed.first_person = true;
    shell.settled();
    let at = bounds_of(&shell, "Dressed ped").center();
    shell.draw(vec![egui::Event::PointerMoved(at)]);
    shell.draw(vec![key(egui::Key::Escape)]);
    assert!(
        !shell.state.workbench.wardrobe.dressed.first_person,
        "Escape over the view leaves the view from the head"
    );
}
