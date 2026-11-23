//! The Wardrobe list as a person works it: Shift and Ctrl pick several garments and Delete takes them out as one
//! step, the keys walk the list, Space looks at a garment without opening it, a row's menu copies and shows, a fuzzy
//! query finds a garment by what the game calls it, tags and a group are set on every garment picked, and the
//! picture of a garment's texture is decoded once.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are, as a person meets it.

use std::path::{Path, PathBuf};

use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PgDictionary;
use or_wardrobe::edit::Edit;
use or_wardrobe::{layout, Component, Ped, Skin, Slot};

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::{key, Shell};
use super::the_wardrobe_operations_on_screen::{
    a_clothing_shell, click_on, garments, names, put, shows, until,
};
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;
use crate::ui::theme;

const C: &str = "mp_m_freemode_01_mp_m_demo";
const JBIB: Slot = Slot::Component(Component::Jbib);

const NONE: egui::Modifiers = egui::Modifiers::NONE;
const SHIFT: egui::Modifiers = egui::Modifiers::SHIFT;
const CTRL: egui::Modifiers = egui::Modifiers {
    alt: false,
    ctrl: true,
    shift: false,
    mac_cmd: false,
    command: true,
};

/// A texture dictionary of one `side` by `side` block-compressed texture, whose pixels `shade` decides.
fn ytd(side: u16, shade: u8) -> Vec<u8> {
    let format = GrcTextureFormat::DXT1;
    let wide = u32::from(side);
    let texture = GrcTexture {
        name: "tex".into(),
        width: side,
        height: side,
        depth: 1,
        mip_count: 1,
        mip_stride: u16::try_from(format.legacy_mip_stride(wide)).unwrap(),
        format: format.to_raw_format(),
        backing_store: vec![shade; format.total_size(wide, wide, 1) as usize],
        ..Default::default()
    };
    PgDictionary {
        codes: vec![or_io::string_hash(b"tex")],
        entries: vec![Some(Box::new(texture))],
        ..Default::default()
    }
    .to_bytes()
    .unwrap()
}

/// Whether every edit has been taken in: a rescan can show a change before the worker's answer, and the answer is what puts its step on the Undo stack.
pub(super) fn idle(shell: &Shell) -> bool {
    shell.state.workbench.wardrobe.is_idle()
}

/// A clothing shell holding `n` jackets, jbib 000 on, each with a real 16 by 16 texture; none is selected.
pub(super) fn with_jackets(dir: &Path, n: u32) -> Shell {
    let mut shell = a_clothing_shell(dir);
    until(&mut shell, "the pack", |s| {
        s.state.wardrobe_pack().is_some()
    });
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("the panel is drawn");
    let mut list: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..n {
        list.push((
            format!("{C}^jbib_{i:03}_u.ydd"),
            format!("jacket {i}").into_bytes(),
        ));
        list.push((
            format!("{C}^jbib_diff_{i:03}_a_uni.ytd"),
            ytd(16, 10 + i as u8),
        ));
    }
    let refs: Vec<(&str, &[u8])> = list
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_slice()))
        .collect();
    shell.drop_files(panel.center(), put(dir, &refs));
    until(&mut shell, "the jackets", |s| {
        garments(s) == n as usize && idle(s)
    });
    // The drop left the pointer on the list, where resting would put a tooltip on screen.
    shell.draw(vec![egui::Event::PointerMoved(egui::pos2(1.0, 1.0))]);
    shell.settled();
    shell
}

/// The pack the Wardrobe shows, read.
pub(super) fn read(shell: &Shell) -> &crate::state::workbench::PackRead {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .expect("the pack reads")
}

/// The id of the garment at place `place` of the jackets.
pub(super) fn id_at(shell: &Shell, place: u32) -> String {
    let read = read(shell);
    let placed = (read.allocation.in_slot(Ped::Male, JBIB))
        .find(|p| p.position == place)
        .unwrap_or_else(|| panic!("no jacket at {place}"));
    read.wardrobe.garments[placed.garment].id.clone()
}

/// The places of the garments picked, lowest first.
fn picked(shell: &Shell) -> Vec<u32> {
    let read = read(shell);
    let mut places: Vec<u32> = (shell.state.picked_garments().iter())
        .filter_map(|id| read.wardrobe.garments.iter().position(|g| g.id == *id))
        .filter_map(|at| read.allocation.of(at).map(|p| p.position))
        .collect();
    places.sort_unstable();
    places
}

/// Where the row at `place` is on screen.
fn row(shell: &Shell, place: u32) -> egui::Pos2 {
    let said = format!("place {place:03}");
    (shell.described.iter())
        .find(|w| {
            w.role == egui::accesskit::Role::Button
                && w.name.as_deref().is_some_and(|n| n.contains(&said))
        })
        .unwrap_or_else(|| panic!("no row at {said}: {:?}", names(shell)))
        .bounds
        .center()
}

/// How many rows the list draws: its buttons, not a tooltip's label that says a row's name again.
pub(super) fn rows(shell: &Shell) -> usize {
    (shell.described.iter())
        .filter(|w| w.role == egui::accesskit::Role::Button)
        .filter(|w| w.name.as_deref().is_some_and(|n| n.contains(", place 0")))
        .count()
}

fn click_row(shell: &mut Shell, place: u32, held: egui::Modifiers) {
    let at = row(shell, place);
    shell.modifiers = held;
    shell.click(at);
    shell.modifiers = NONE;
    shell.settled();
}

fn secondary(at: egui::Pos2) -> Vec<egui::Event> {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: NONE,
    };
    vec![egui::Event::PointerMoved(at), press(true), press(false)]
}

/// A key pressed with `held`.
fn with(held: egui::Modifiers, pressed: egui::Key) -> egui::Event {
    egui::Event::Key {
        key: pressed,
        physical_key: Some(pressed),
        pressed: true,
        repeat: false,
        modifiers: held,
    }
}

/// The text typed into the field named `field`, which a click focuses.
pub(super) fn type_into(shell: &mut Shell, field: &str, text: &str) {
    // ⚠ The field, and not a label of the same name beside it: a label is a widget too, and which of the two a frame lists first is no order a test may lean on.
    let at = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(field) && w.role != egui::accesskit::Role::Label)
        .unwrap_or_else(|| panic!("no field named `{field}`: {:?}", names(shell)))
        .bounds
        .center();
    shell.click(at);
    shell.draw(vec![egui::Event::Text(text.to_string())]);
    shell.settled();
}

pub(super) fn run(shell: &mut Shell, action: Action) {
    let ctx = shell.ctx.clone();
    commands::run_action(&mut shell.state, &ctx, action);
    shell.settled();
}

#[test]
fn shift_and_ctrl_pick_several_garments_a_heading_picks_its_own_and_delete_takes_them_out_as_one_step(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 4);

    click_row(&mut shell, 0, NONE);
    assert_eq!(picked(&shell), [0]);
    click_row(&mut shell, 2, SHIFT);
    assert_eq!(
        picked(&shell),
        [0, 1, 2],
        "Shift spans from the one clicked first"
    );
    click_row(&mut shell, 1, CTRL);
    assert_eq!(picked(&shell), [0, 2], "Ctrl takes one out of the pick");
    click_row(&mut shell, 3, CTRL);
    assert_eq!(picked(&shell), [0, 2, 3], "and puts one in");
    click_row(&mut shell, 3, NONE);
    assert_eq!(picked(&shell), [3], "a plain click picks that one alone");
    click_row(&mut shell, 1, SHIFT);
    assert_eq!(
        picked(&shell),
        [1, 2, 3],
        "a span runs from the last plain click"
    );

    let heading = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Male · Tops · 4 garments"))
        .unwrap_or_else(|| panic!("no heading: {:?}", names(&shell)))
        .bounds
        .center();
    shell.click(heading);
    shell.settled();
    assert_eq!(
        picked(&shell),
        [0, 1, 2, 3],
        "a heading picks every garment under it"
    );

    click_row(&mut shell, 1, NONE);
    click_row(&mut shell, 3, SHIFT);
    assert_eq!(
        shell.state.workspace_docks.focused(Workspace::Wardrobe),
        Some(ids::WARDROBE),
        "the list took the keyboard's focus"
    );
    let steps = shell.state.workbench.wardrobe.undo.len();
    run(&mut shell, Action::Delete);
    until(&mut shell, "the removal", |s| garments(s) == 1 && idle(s));
    assert_eq!(
        shell.state.workbench.wardrobe.undo.len(),
        steps + 1,
        "three garments out is one step"
    );
    assert!(shell.state.picked_garments().is_empty());
    click_on(&mut shell, "Undo removal");
    until(&mut shell, "the undo", |s| garments(s) == 4 && idle(s));
}

#[test]
fn space_looks_at_the_garment_without_opening_it_and_shows_its_texture() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    click_row(&mut shell, 1, NONE);
    assert!(!shows(&shell, "Quick look: "));

    shell.draw(vec![key(egui::Key::Space)]);
    shell.settled();
    assert!(
        shows(&shell, "Quick look: "),
        "Space opens the quick look: {:?}",
        names(&shell)
    );
    assert_eq!(
        shell.state.active_workspace,
        Workspace::Wardrobe,
        "looking opens nothing"
    );
    let file = format!("{C}^jbib_diff_001_a_uni.ytd");
    until(&mut shell, "its picture", |s| {
        shows(s, &format!("Picture of {file}, 16 by 16"))
    });
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the quick look of a garment", &mut found);
    assert!(found.is_empty(), "{found:?}");

    shell.draw(vec![key(egui::Key::Space)]);
    shell.settled();
    assert!(!shows(&shell, "Quick look: "), "Space closes it again");
    shell.draw(vec![key(egui::Key::Space)]);
    shell.settled();
    assert!(shows(&shell, "Quick look: "));
    shell.draw(vec![key(egui::Key::Escape)]);
    shell.settled();
    assert!(!shows(&shell, "Quick look: "), "Escape closes it");
    shell.draw(vec![key(egui::Key::Space)]);
    shell.settled();
    // The pointer goes to the button before it presses, as a hand does.
    let at = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Close quick look"))
        .expect("a close button")
        .bounds
        .center();
    shell.draw(vec![egui::Event::PointerMoved(at)]);
    shell.settled();
    click_on(&mut shell, "Close quick look");
    assert!(!shows(&shell, "Quick look: "), "so does its button");
}

#[test]
fn the_arrow_keys_walk_the_list_enter_opens_the_model_and_f2_names_the_garment() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 4);
    click_row(&mut shell, 0, NONE);

    shell.draw(vec![key(egui::Key::ArrowDown)]);
    shell.settled();
    assert_eq!(picked(&shell), [1]);
    shell.draw(vec![key(egui::Key::ArrowDown)]);
    shell.settled();
    assert_eq!(picked(&shell), [2]);
    shell.draw(vec![key(egui::Key::ArrowUp)]);
    shell.settled();
    assert_eq!(picked(&shell), [1]);

    shell.modifiers = SHIFT;
    shell.draw(vec![with(SHIFT, egui::Key::ArrowDown)]);
    shell.modifiers = NONE;
    shell.settled();
    assert_eq!(picked(&shell), [1, 2], "Shift and an arrow extend the pick");

    click_row(&mut shell, 3, NONE);
    shell.draw(vec![key(egui::Key::ArrowDown)]);
    shell.settled();
    assert_eq!(picked(&shell), [3], "the last garment stays at the end");

    run(&mut shell, Action::Rename);
    let (_, held) = shell.focused().expect("a field has the keyboard");
    assert_eq!(held, "Garment name", "F2 puts the keyboard in the name");

    // An Enter that ends a word typed in the filter opens nothing.
    click_row(&mut shell, 2, NONE);
    type_into(&mut shell, "Filter garments", "jbib");
    shell.draw(vec![key(egui::Key::Enter)]);
    shell.settled();
    assert_eq!(shell.state.active_workspace, Workspace::Wardrobe);
    clear(&mut shell, "Filter garments");

    click_row(&mut shell, 2, NONE);
    shell.draw(vec![key(egui::Key::Enter)]);
    shell.settled();
    assert_eq!(
        shell.state.active_workspace,
        Workspace::Model,
        "Enter opens the model of the garment"
    );
}

#[test]
fn a_rows_menu_copies_its_name_game_name_and_path_and_shows_its_file_in_the_file_manager() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 2);
    let id = id_at(&shell, 1);
    shell
        .state
        .edit_garment(id, Edit::Label(Some("Red jacket".into())));
    until(&mut shell, "the name", |s| shows(s, "Red jacket"));

    let (folder, model, game) = {
        let read = read(&shell);
        let garment = &read.wardrobe.garments[read
            .allocation
            .in_slot(Ped::Male, JBIB)
            .nth(1)
            .unwrap()
            .garment];
        let model = garment.model.clone().expect("a model");
        let game = layout::model_name(
            Ped::Male,
            &read.wardrobe.collection(Ped::Male),
            JBIB,
            1,
            Skin::Universal,
        );
        (
            shell.state.wardrobe_pack().unwrap().folder.clone(),
            model,
            game,
        )
    };
    let file: PathBuf = folder.join(model.replace('/', std::path::MAIN_SEPARATOR_STR));

    for (item, copied) in [
        ("Copy name", "Red jacket".to_string()),
        ("Copy game name", game),
        ("Copy path", file.display().to_string()),
    ] {
        let at = row(&shell, 1);
        shell.draw(secondary(at));
        shell.settled();
        if item == "Copy name" {
            for text in [
                "Copy name",
                "Copy game name",
                "Copy path",
                "Reveal in the file manager",
            ] {
                assert!(
                    shows(&shell, text),
                    "the menu lacks `{text}`: {:?}",
                    names(&shell)
                );
            }
            let mut found = std::collections::BTreeMap::new();
            mute(&shell, "a garment's menu", &mut found);
            assert!(found.is_empty(), "{found:?}");
        }
        click_on(&mut shell, item);
        assert_eq!(shell.copied.last(), Some(&copied), "{item}");
    }

    let at = row(&shell, 1);
    shell.draw(secondary(at));
    shell.settled();
    click_on(&mut shell, "Reveal in the file manager");
    assert!(
        crate::platform::shell::fake::was_revealed(&file),
        "the file manager was asked for {}",
        file.display()
    );
}

#[test]
fn a_fuzzy_query_finds_a_garment_by_a_fragment_of_its_game_name_its_slot_or_what_it_is_called() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    let id = id_at(&shell, 1);
    shell
        .state
        .edit_garment(id, Edit::Label(Some("Red jacket".into())));
    until(&mut shell, "the name", |s| shows(s, "Red jacket"));
    // Resting on a row puts its name on screen a second time, as a label of the tooltip, which is no row.
    let at = row(&shell, 0);
    shell.draw(vec![egui::Event::PointerMoved(at)]);
    until(&mut shell, "the tooltip", |s| shows(s, "Texture a"));
    assert_eq!(rows(&shell), 3);

    type_into(&mut shell, "Filter garments", "jbib_001");
    assert_eq!(
        rows(&shell),
        1,
        "a fragment of the game's name: {:?}",
        names(&shell)
    );
    assert!(shows(&shell, "place 001"));

    let at = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Filter garments"))
        .unwrap()
        .bounds
        .center();
    shell.click(at);
    shell.draw(vec![with(CTRL, egui::Key::A)]);
    shell.draw(vec![egui::Event::Text("rdjkt".into())]);
    shell.settled();
    assert_eq!(rows(&shell), 1, "letters of its name in order");
    assert!(shows(&shell, "place 001"));

    shell.draw(vec![with(CTRL, egui::Key::A)]);
    shell.draw(vec![egui::Event::Text("002".into())]);
    shell.settled();
    assert_eq!(rows(&shell), 1, "its place");
    assert!(shows(&shell, "place 002"));

    shell.draw(vec![with(CTRL, egui::Key::A)]);
    shell.draw(vec![egui::Event::Text("tops".into())]);
    shell.settled();
    assert_eq!(rows(&shell), 3, "its slot, in the words the headings use");

    shell.draw(vec![with(CTRL, egui::Key::A)]);
    shell.draw(vec![egui::Event::Text("zzz".into())]);
    shell.settled();
    assert_eq!(rows(&shell), 0);
    assert!(shows(&shell, "No garment matches what is on"));
}

/// How many garments of the pack carry `tag`.
fn tagged(shell: &Shell, tag: &str) -> usize {
    (read(shell).wardrobe.garments.iter())
        .filter(|g| g.tags.iter().any(|t| t == tag))
        .count()
}

/// How many garments of the pack are in `group`.
fn grouped(shell: &Shell, group: &str) -> usize {
    (read(shell).wardrobe.garments.iter())
        .filter(|g| g.group.as_deref() == Some(group))
        .count()
}

/// Empty the field named `field`.
fn clear(shell: &mut Shell, field: &str) {
    let at = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(field))
        .unwrap_or_else(|| panic!("no field named `{field}`"))
        .bounds
        .center();
    shell.click(at);
    shell.draw(vec![with(CTRL, egui::Key::A)]);
    shell.draw(vec![key(egui::Key::Backspace)]);
    shell.settled();
}

#[test]
fn tags_and_a_group_are_set_on_every_garment_picked_with_one_undo_and_a_search_finds_them() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);

    click_row(&mut shell, 1, NONE);
    assert!(!shows(&shell, "garments selected"));
    type_into(&mut shell, "Add a tag", "summer");
    shell.draw(vec![key(egui::Key::Enter)]);
    until(&mut shell, "the tag", |s| {
        tagged(s, "summer") == 1 && idle(s)
    });
    assert!(shows(&shell, "Remove tag summer"), "{:?}", names(&shell));

    type_into(&mut shell, "Filter garments", "summer");
    assert_eq!(rows(&shell), 1, "a search reads tags: {:?}", names(&shell));
    assert!(shows(&shell, "place 001"));
    clear(&mut shell, "Filter garments");
    assert_eq!(rows(&shell), 3);

    click_row(&mut shell, 0, NONE);
    click_row(&mut shell, 2, SHIFT);
    assert!(shows(&shell, "3 garments selected"), "{:?}", names(&shell));
    let steps = shell.state.workbench.wardrobe.undo.len();
    type_into(&mut shell, "Group", "Leather set");
    shell.draw(vec![key(egui::Key::Enter)]);
    until(&mut shell, "the group", |s| {
        grouped(s, "Leather set") == 3 && idle(s)
    });
    assert_eq!(
        shell.state.workbench.wardrobe.undo.len(),
        steps + 1,
        "a group set on three garments is one step"
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the tags and the group of a garment", &mut found);
    assert!(found.is_empty(), "{found:?}");

    click_on(&mut shell, "Undo group");
    until(&mut shell, "the undo", |s| {
        grouped(s, "Leather set") == 0 && idle(s)
    });
    assert_eq!(tagged(&shell, "summer"), 1, "the tag stays");

    click_row(&mut shell, 1, NONE);
    click_on(&mut shell, "Remove tag summer");
    until(&mut shell, "the tag gone", |s| {
        tagged(s, "summer") == 0 && idle(s)
    });
}

/// The file of the first texture of the garment at `place`.
fn texture_file(shell: &Shell, place: u32) -> PathBuf {
    let read = read(shell);
    let at = (read.allocation.in_slot(Ped::Male, JBIB))
        .find(|p| p.position == place)
        .unwrap()
        .garment;
    let texture = read.wardrobe.garments[at].variations[0]
        .texture
        .clone()
        .expect("a texture");
    shell.state.wardrobe_pack().unwrap().folder.join(texture)
}

/// The id of the picture made of `file`, which changes with every decode.
fn picture_of(shell: &Shell, file: &Path) -> Option<u64> {
    shell
        .state
        .workbench
        .wardrobe
        .dressed
        .thumb(file)
        .map(|t| t.id)
}

#[test]
fn a_garments_texture_is_decoded_once_for_its_row_and_hovering_the_row_shows_its_textures() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    let files: Vec<PathBuf> = (0..3).map(|p| texture_file(&shell, p)).collect();

    assert!(shell.state.selected_garment().is_none());
    until(&mut shell, "the row pictures", |s| {
        files.iter().all(|f| picture_of(s, f).is_some())
    });
    let first: Vec<Option<u64>> = files.iter().map(|f| picture_of(&shell, f)).collect();

    // Hiding the rows and showing them again, picking one and looking at it decode nothing again.
    type_into(&mut shell, "Filter garments", "jbib_001");
    assert_eq!(rows(&shell), 1);
    clear(&mut shell, "Filter garments");
    click_row(&mut shell, 1, NONE);
    shell.draw(vec![key(egui::Key::Space)]);
    for _ in 0..20 {
        shell.state.poll_workbench();
        shell.draw(Vec::new());
    }
    let later: Vec<Option<u64>> = files.iter().map(|f| picture_of(&shell, f)).collect();
    assert_eq!(first, later, "every picture was made once");

    // Resting on a row shows its textures.
    shell.draw(vec![key(egui::Key::Escape)]);
    let at = row(&shell, 2);
    shell.draw(vec![egui::Event::PointerMoved(at)]);
    let file = files[2].file_name().unwrap().to_string_lossy().into_owned();
    until(&mut shell, "the preview", |s| {
        shows(s, &format!("Texture a, {file}, 16 by 16"))
    });
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "a garment's hover preview", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

/// Frames of the shell until nothing has been read for ten of them in a row.
fn until_reads_end(shell: &mut Shell) {
    let idle = std::cell::Cell::new(0);
    until(shell, "the reads to end", |s| {
        let reading = s.state.workbench.wardrobe.dressed.is_reading();
        idle.set(if reading { 0 } else { idle.get() + 1 });
        idle.get() >= 10
    });
}

/// The three files outside the pack that a `wardrobe.json` from somebody else names.
struct Outside {
    /// The folder the files are in, removed with this.
    _folder: tempfile::TempDir,
    /// The first texture of the jacket at place 0.
    texture_a: PathBuf,
    /// The second texture of the jacket at place 1.
    texture_b: PathBuf,
    /// The model of the jacket at place 1.
    model: PathBuf,
}

/// The pack's jackets, rewritten as a shared or downloaded `wardrobe.json` may name their files: by an absolute path.
fn point_outside(shell: &mut Shell) -> Outside {
    let folder = tempfile::tempdir().unwrap();
    let file = |name: &str, body: Vec<u8>| {
        let path = folder.path().join(name);
        std::fs::write(&path, body).unwrap();
        path
    };
    let texture_a = file("outside_a.ytd", ytd(16, 70));
    let texture_b = file("outside_b.ytd", ytd(16, 80));
    let model = file("outside.ydd", b"geometry".to_vec());
    let (first, second) = (id_at(shell, 0), id_at(shell, 1));
    let pack = shell.state.wardrobe_pack().unwrap().folder.clone();
    let (a, b, m) = (
        texture_a.display().to_string(),
        texture_b.display().to_string(),
        model.display().to_string(),
    );
    or_wardrobe::edit::update(&pack, |w| {
        for g in &mut w.garments {
            if g.id == first {
                g.variations[0].texture = Some(a.clone());
            } else if g.id == second {
                g.model = Some(m.clone());
                g.variations.push(or_wardrobe::model::Variation {
                    texture: Some(b.clone()),
                    ..Default::default()
                });
            }
        }
        Ok(())
    })
    .unwrap();
    until(shell, "the rewritten pack", |s| {
        read(s)
            .wardrobe
            .garment(&first)
            .is_some_and(|g| g.variations[0].texture.as_deref() == Some(a.as_str()))
    });
    Outside {
        _folder: folder,
        texture_a,
        texture_b,
        model,
    }
}

/// A shell on two jackets whose files a `wardrobe.json` from somebody else moved outside the pack.
fn outside_the_pack(dir: &Path) -> (Shell, Outside) {
    let mut shell = with_jackets(dir, 2);
    let outside = point_outside(&mut shell);
    (shell, outside)
}

#[test]
fn a_row_reads_no_texture_outside_the_pack() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, outside) = outside_the_pack(dir.path());
    let own = texture_file(&shell, 1);

    until(&mut shell, "the row picture", |s| {
        picture_of(s, &own).is_some()
    });
    until_reads_end(&mut shell);
    assert!(
        picture_of(&shell, &outside.texture_a).is_none(),
        "a row read a texture outside the pack"
    );
}

#[test]
fn resting_on_a_row_reads_no_texture_outside_the_pack() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, outside) = outside_the_pack(dir.path());

    let at = row(&shell, 1);
    shell.draw(vec![egui::Event::PointerMoved(at)]);
    until(&mut shell, "the hover", |s| shows(s, "Texture a"));
    until_reads_end(&mut shell);
    assert!(
        picture_of(&shell, &outside.texture_b).is_none(),
        "the hover read a texture outside the pack"
    );
    assert!(!shows(&shell, "Texture b"), "{:?}", names(&shell));
}

#[test]
fn the_quick_look_reads_no_texture_outside_the_pack() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, outside) = outside_the_pack(dir.path());

    click_row(&mut shell, 0, NONE);
    shell.draw(vec![key(egui::Key::Space)]);
    shell.settled();
    assert!(shows(&shell, "Quick look: "), "{:?}", names(&shell));
    until_reads_end(&mut shell);
    assert!(
        !shows(&shell, "Picture of outside_a.ytd"),
        "the quick look read a texture outside the pack: {:?}",
        names(&shell)
    );
    assert!(
        picture_of(&shell, &outside.texture_a).is_none(),
        "the tiles of the garment read a texture outside the pack"
    );
}

#[test]
fn the_preview_reads_no_model_outside_the_pack() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _outside) = outside_the_pack(dir.path());

    click_row(&mut shell, 1, NONE);
    until_reads_end(&mut shell);
    let notes = shell.state.workbench.wardrobe.dressed.notes.clone();
    assert!(
        !notes.iter().any(|n| n.contains("outside")),
        "the preview read a model outside the pack: {notes:?}"
    );
}

#[test]
fn enter_opens_no_model_outside_the_pack() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, _outside) = outside_the_pack(dir.path());

    click_row(&mut shell, 1, NONE);
    shell.draw(vec![key(egui::Key::Enter)]);
    shell.settled();
    assert_eq!(
        shell.state.active_workspace,
        Workspace::Wardrobe,
        "Enter opened a model outside the pack"
    );
}

#[test]
fn a_rows_menu_offers_no_path_to_copy_and_no_file_to_show_outside_the_pack() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, outside) = outside_the_pack(dir.path());

    for item in ["Copy path", "Reveal in the file manager"] {
        let at = row(&shell, 1);
        shell.draw(secondary(at));
        shell.settled();
        let widget = (shell.described.iter())
            .find(|w| w.name.as_deref() == Some(item))
            .unwrap_or_else(|| panic!("the menu lacks `{item}`: {:?}", names(&shell)));
        assert!(
            widget.disabled,
            "`{item}` is offered for a file outside the pack"
        );
        click_on(&mut shell, item);
    }
    assert!(shell.copied.is_empty(), "{:?}", shell.copied);
    assert!(
        !crate::platform::shell::fake::was_revealed(&outside.model),
        "the file manager was asked for a file outside the pack"
    );
}
