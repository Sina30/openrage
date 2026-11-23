//! Small faults in the default views, menus and keys: an Open Folder document's own row offers what
//! a folder's row does, a key a disabled command refuses says why, a menu held open while a load
//! makes another document active closes only if it belonged to the one left, and a drawable of one
//! model says "1 model".

use std::path::{Path, PathBuf};

use egui::{PointerButton, Rect};

use super::nothing_draws_over_what_it_belongs_to::{app_context, input, widgets, window};
use super::opening_a_ydd::finish_load;
use super::the_files_view_in_the_centre::{
    accessible, browsing, click, click_with, find, frame, leaf_of, run, settled,
};
use super::the_inspector_leads_with_a_summary::drawn_by;
use super::{AppState, DocumentId, Prefs};
use crate::ui::panels::registry::ids;

/// `name`'s row in the Files tree, which has its icon before the name, right-clicked, and the menu it opens.
fn documents_own_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    name: &str,
) -> Vec<(String, egui::Pos2)> {
    let icon = |c: char| ('\u{e000}'..='\u{f8ff}').contains(&c);
    let rows = settled(ctx, state);
    let tree = leaf_of(state, ids::FILES);
    let row = (rows.iter())
        .find(|(text, at)| {
            text.starts_with(icon) && text.trim_start_matches(icon) == name && tree.contains(*at)
        })
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("no row of `{name}` in {tree:?}: {rows:?}"));
    click_with(ctx, state, row, PointerButton::Secondary);
    frame(ctx, state, Vec::new())
}

/// The text a click on the item of `menu` starting with `label` put on the clipboard.
fn copied_by(ctx: &egui::Context, state: &mut AppState, at: egui::Pos2) -> Vec<String> {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    run(
        ctx,
        state,
        vec![egui::Event::PointerMoved(at), press(true)],
        egui::Modifiers::NONE,
    );
    let output = run(ctx, state, vec![press(false)], egui::Modifiers::NONE);
    (output.platform_output.commands.into_iter())
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn an_open_folder_documents_own_row_reveals_it_copies_its_path_and_shows_its_properties() {
    let dir = tempfile::tempdir().expect("a folder");
    std::fs::write(dir.path().join("adder.yft"), b"not really").expect("a file");
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(dir.path().to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let name = state.document(doc).expect("open").name.clone();
    let ctx = accessible();

    let menu = documents_own_menu(&ctx, &mut state, &name);
    let reveal = (menu.iter())
        .find(|(text, _)| text.starts_with("Reveal in "))
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("the row offers no Reveal in the file manager: {menu:?}"));
    click(&ctx, &mut state, reveal);
    assert!(
        crate::platform::shell::fake::was_revealed(dir.path()),
        "Reveal did not show the folder opened"
    );

    let menu = documents_own_menu(&ctx, &mut state, &name);
    let at = find(&menu, egui::Rect::EVERYTHING, "Copy path");
    assert_eq!(
        copied_by(&ctx, &mut state, at),
        [dir.path().display().to_string()],
        "Copy path did not copy the folder opened"
    );

    let menu = documents_own_menu(&ctx, &mut state, &name);
    click(
        &ctx,
        &mut state,
        find(&menu, egui::Rect::EVERYTHING, "Properties"),
    );
    let window = settled(&ctx, &mut state);
    for fact in ["Type", "Hash", "Path"] {
        find(&window, egui::Rect::EVERYTHING, fact);
    }
    find(
        &window,
        egui::Rect::EVERYTHING,
        &dir.path().display().to_string(),
    );
}

/// Presses the Delete key as the editor's window hands it over, which runs the command bound to it.
fn press_delete(ctx: &egui::Context, state: &mut AppState) {
    use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
    let action = crate::ui::shortcuts::action_for_press(
        &Key::Named(NamedKey::Delete),
        PhysicalKey::Code(KeyCode::Delete),
        ModifiersState::empty(),
        false,
        &state.prefs,
    )
    .expect("Delete is bound");
    crate::ui::commands::run_action(state, ctx, action);
}

#[test]
fn delete_pressed_with_several_rows_selected_says_why_nothing_was_deleted() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    settled(&ctx, &mut state);
    let ws = state.active_workspace;
    crate::ui::dock_commands::reveal(state.workspace_docks.get_mut(ws), ids::FILES);
    state.selection.select(
        doc,
        super::the_files_view_in_the_centre::node(&state, doc, "notes.txt"),
    );
    state.selection.toggle(
        doc,
        super::the_files_view_in_the_centre::node(&state, doc, "big.bin"),
    );
    assert!(
        state.toasts.iter().next().is_none(),
        "a toast was up before the key"
    );

    press_delete(&ctx, &mut state);

    let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
    assert!(
        said.iter()
            .any(|toast| toast.contains("Several rows cannot be deleted at once yet")),
        "the key refused and said nothing: {said:?}"
    );
    assert!(
        state.pending_delete.is_none(),
        "a delete was asked after all"
    );
}

/// ⚠ egui copies a selected label's text on Ctrl+C itself, so a refusal of Cut, Copy or Paste is no
/// fault to say aloud: the copy it refuses to make may be one that just worked.
#[test]
fn a_clipboard_key_refused_for_want_of_a_selection_says_nothing() {
    use winit::keyboard::{Key, KeyCode, PhysicalKey};

    let (mut state, _) = browsing();
    let ctx = accessible();
    settled(&ctx, &mut state);
    for (letter, code) in [
        ("x", KeyCode::KeyX),
        ("c", KeyCode::KeyC),
        ("v", KeyCode::KeyV),
    ] {
        let action = crate::ui::shortcuts::action_for_press(
            &Key::Character(letter.into()),
            PhysicalKey::Code(code),
            crate::ui::shortcuts::PRIMARY_STATE,
            false,
            &state.prefs,
        )
        .unwrap_or_else(|| panic!("Ctrl+{letter} is bound"));
        let command = crate::ui::commands::of(&state, action).expect("a command");
        assert!(
            command.enabled.is_err(),
            "Ctrl+{letter} can run, so nothing is refused: {:?}",
            command.id
        );

        crate::ui::commands::run_action(&mut state, &ctx, action);

        let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
        assert!(
            said.is_empty(),
            "Ctrl+{letter} was refused and toasted: {said:?}"
        );
    }
}

/// A frame of the editor's own shell, which a test of what stays open across documents draws.
struct Shell {
    ctx: egui::Context,
    time: f64,
}

impl Shell {
    fn new() -> Self {
        let ctx = app_context();
        ctx.enable_accesskit();
        Self { ctx, time: 0.0 }
    }

    fn frame(&mut self, state: &mut AppState, events: Vec<egui::Event>) -> Vec<(String, Rect)> {
        self.time += 1.0 / 30.0;
        let screen = window(crate::app::EDITOR_SIZE);
        self.ctx.begin_pass(input(screen, self.time, events));
        crate::ui::draw(&self.ctx, state, None);
        widgets(&self.ctx.end_pass())
    }

    /// Presses and releases `button` over the topmost widget labelled `label`.
    fn click(&mut self, state: &mut AppState, label: &str, button: PointerButton) {
        let drawn = self.frame(state, Vec::new());
        let at = (drawn.iter())
            .filter(|(text, _)| text == label)
            .map(|(_, rect)| rect.center())
            .min_by(|a, b| a.y.total_cmp(&b.y))
            .unwrap_or_else(|| panic!("no `{label}` was drawn: {drawn:?}"));
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(state, vec![egui::Event::PointerMoved(at), press(true)]);
        self.frame(state, vec![press(false)]);
    }

    /// Whether a widget labelled `label...` is drawn two frames on.
    fn shows(&mut self, state: &mut AppState, label: &str) -> bool {
        self.frame(state, Vec::new());
        let drawn = self.frame(state, Vec::new());
        drawn.iter().any(|(text, _)| text.starts_with(label))
    }
}

/// Two text files open, `first.txt` active, and the path of a third a load adds.
fn two_open_and_one_to_load(dir: &Path) -> (AppState, PathBuf) {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    for name in ["second.txt", "first.txt", "third.txt"] {
        std::fs::write(dir.join(name), "text").expect("a file");
    }
    for name in ["second.txt", "first.txt"] {
        state.load_file(dir.join(name));
        finish_load(&mut state).expect("a text file opens");
    }
    (state, dir.join("third.txt"))
}

/// Whether the menu `open` holds open is still drawn once a background load has made the third document active.
fn survives_a_load(open: impl FnOnce(&mut Shell, &mut AppState), item: &str) -> bool {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let (mut state, third) = two_open_and_one_to_load(dir.path());
    let mut shell = Shell::new();
    shell.frame(&mut state, Vec::new());
    open(&mut shell, &mut state);
    assert!(
        shell.shows(&mut state, item),
        "the menu never opened, so nothing is tested"
    );
    let left = state.active_document;
    state.load_file(third);
    finish_load(&mut state).expect("the load finishes");
    assert_ne!(
        state.active_document, left,
        "the load left the active document as it was"
    );
    shell.shows(&mut state, item)
}

#[test]
fn a_menu_of_the_shell_stays_open_when_a_load_makes_another_document_active() {
    let _serial = crate::ui::theme::serial();
    let stays = survives_a_load(
        |shell, state| shell.click(state, "File", PointerButton::Primary),
        "Open file",
    );
    assert!(
        stays,
        "the File menu closed under the pointer when a load finished"
    );
}

#[test]
fn a_tab_menu_stays_open_when_the_load_leaves_another_document() {
    let _serial = crate::ui::theme::serial();
    let stays = survives_a_load(
        |shell, state| shell.click(state, "second.txt", PointerButton::Secondary),
        "Activate",
    );
    assert!(
        stays,
        "the menu of a tab that was not the active one closed when the active one was left"
    );
}

#[test]
fn a_tab_menu_closes_when_a_load_leaves_the_document_it_belonged_to() {
    let _serial = crate::ui::theme::serial();
    let stays = survives_a_load(
        |shell, state| shell.click(state, "first.txt", PointerButton::Secondary),
        "Activate",
    );
    assert!(
        !stays,
        "the menu of the document the load left stayed open over the one it made active"
    );
}

/// A Legacy drawable of no corpus holding one model, active.
fn a_drawable_of_one_model(state: &mut AppState) -> DocumentId {
    let doc = super::every_edit_undoes::drawable(state, "probe.ydr");
    let document = state.document_mut(doc).expect("open");
    let drawable = super::drawable_mut(&mut document.payload, 0).expect("a Legacy drawable");
    drawable.lod_group.lods[0].models.push(Box::new(
        or_io::gta5::GrmModel::<or_io::decl::Gen8>::default(),
    ));
    state.active_workspace = state.document(doc).expect("open").kind.info().workspace;
    doc
}

#[test]
fn a_drawable_of_one_model_says_1_model_in_its_outline_and_its_gallery() {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    a_drawable_of_one_model(&mut state);
    let said = |state: &mut AppState, draw: fn(&mut egui::Ui, &mut AppState)| -> Vec<String> {
        (drawn_by(state, draw).into_iter())
            .map(|(_, text)| text)
            .filter(|text| text.contains("model"))
            .collect()
    };
    let outline = said(&mut state, |ui, state| {
        crate::ui::panels::hierarchy::draw_outline(ui, state)
    });
    let gallery = said(&mut state, |ui, state| {
        crate::ui::panels::gallery::draw(ui, state, None)
    });
    for (view, texts) in [("outline", outline), ("gallery", gallery)] {
        assert!(
            texts.iter().any(|text| text.contains("1 model")),
            "the {view} does not count the one model: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("1 models")),
            "the {view} says `1 models`: {texts:?}"
        );
    }
}

/// A Legacy particle file whose one model holds a geometry, with that geometry selected.
// The struct update the lint asks for does not compile: the list holds private fields.
#[allow(clippy::field_reassign_with_default)]
fn a_particle_models_geometry_selected(state: &mut AppState) -> DocumentId {
    use crate::ui::panels::selection_ids::{
        pack_lod_sub_index, ypt_model_part_id, YPT_MODEL_KIND_LOD,
    };
    use or_io::decl::Gen8;
    use or_io::gta5::{GrmModel, PgDictionary, PtxFxList, RmcDrawable};

    const MODEL: u32 = 0x5EED_0003;
    let model = GrmModel::<Gen8> {
        geometries: vec![Box::default()],
        ..Default::default()
    };
    let mut drawable = RmcDrawable::<Gen8>::default();
    drawable.lod_group.lods[0].models.push(Box::new(model));
    let mut list = PtxFxList::<Gen8>::default();
    list.model_dict = Some(Box::new(PgDictionary {
        codes: vec![MODEL],
        entries: vec![Some(Box::new(drawable))],
        ..Default::default()
    }));
    let doc = state.insert_document(
        PathBuf::from("probe.ypt"),
        super::DocPayload::Resource(or_io::Asset::Particles(list)),
        None,
        None,
    );
    let geometry = pack_lod_sub_index(0, 0, Some(0));
    state
        .selection
        .select(doc, ypt_model_part_id(MODEL, YPT_MODEL_KIND_LOD, geometry));
    doc
}

#[test]
fn a_particle_models_geometry_states_its_index_count_once() {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    a_particle_models_geometry_selected(&mut state);
    let drawn = drawn_by(&mut state, |ui, state| {
        crate::ui::panels::inspector::draw(ui, state)
    });
    // A text run repeats the label it lays out.
    let shown: Vec<String> = (drawn.into_iter())
        .filter(|(role, _)| *role != egui::accesskit::Role::TextRun)
        .map(|(_, text)| text)
        .collect();
    assert!(
        shown.iter().any(|text| text == "Vertex count"),
        "the geometry's rows are not drawn, so nothing is tested: {shown:?}"
    );
    let counts: Vec<&String> = (shown.iter())
        .filter(|text| matches!(text.as_str(), "Index count" | "Indices in buffer"))
        .collect();
    assert_eq!(counts.len(), 1, "the index count is stated {counts:?}");
}

/// A Legacy or Enhanced texture dictionary of one texture, active in its workspace.
fn a_dictionary_of_one_texture(state: &mut AppState, enhanced: bool) -> super::DocPayload {
    use or_io::decl::Open;
    use or_io::gta5::{gen8, gen9, PgDictionary};

    let asset = if enhanced {
        let texture = gen9::GrcTextureG9 {
            name: "lone".to_string(),
            width: 4,
            height: 4,
            format: Open::new(gen9::PixelFormat::Bc3UnormSrgb),
            ..Default::default()
        };
        or_io::Asset::TextureDictGen9(PgDictionary {
            codes: vec![or_io::hash::string_hash_str("lone")],
            entries: vec![Some(Box::new(texture))],
            ..Default::default()
        })
    } else {
        let mut dictionary = PgDictionary::default();
        dictionary.insert(gen8::GrcTexture {
            name: "lone".to_string(),
            width: 4,
            height: 4,
            ..Default::default()
        });
        or_io::Asset::TextureDict(dictionary)
    };
    let payload = super::DocPayload::Resource(asset);
    let doc = state.insert_document(PathBuf::from("probe.ytd"), payload.clone(), None, None);
    state.active_workspace = state.document(doc).expect("open").kind.info().workspace;
    payload
}

#[test]
fn a_dictionary_of_one_texture_says_1_texture_in_its_outline_and_its_summary() {
    for enhanced in [false, true] {
        let edition = if enhanced { "Enhanced" } else { "Legacy" };
        let mut state = AppState::from_prefs(Prefs::default());
        state.documents.clear();
        let payload = a_dictionary_of_one_texture(&mut state, enhanced);
        let outline: Vec<String> = (drawn_by(&mut state, |ui, state| {
            crate::ui::panels::hierarchy::draw_outline(ui, state)
        })
        .into_iter())
        .map(|(_, text)| text)
        .filter(|text| text.contains("texture"))
        .collect();
        let summary = super::capabilities::for_payload(&payload)
            .and_then(|row| row.selection_summary)
            .map_or_else(String::new, |summary| summary(&payload, 0));
        for (view, texts) in [("outline", outline), ("summary", vec![summary])] {
            assert!(
                texts.iter().any(|text| text.contains("1 texture")),
                "the {edition} {view} does not count the one texture: {texts:?}"
            );
            assert!(
                !texts.iter().any(|text| text.contains("1 textures")),
                "the {edition} {view} says `1 textures`: {texts:?}"
            );
        }
    }
}
