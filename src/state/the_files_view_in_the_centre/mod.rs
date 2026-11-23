//! Browsing gets the centre: the Files view of the folder selected in Files,
//! with its columns, its pictures, its keys and its menu.
//!
//! ⚠ **These draw the real dock and read what the frames wrote**: the rows the
//! view listed, the selection, the rename editor, the prompts. A paint check
//! would pass with every key and menu disconnected.

mod keys;
mod menus;
mod pictures;

use std::sync::Arc;

use super::files_view::{inner_of_address, name_hash, row_facts};
use super::{AppState, DocPayload, DocumentId, DocumentKind, Prefs, RpfNodeId};
use crate::ui::panels::hierarchy::{ui_state, FilesViewUi};
use crate::ui::panels::registry::ids;

const SCREEN: egui::Vec2 = egui::vec2(1600.0, 900.0);

/// One frame of the dock and the floating windows, with `modifiers` held.
pub(super) fn run(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        events,
        modifiers,
        ..Default::default()
    };
    ctx.run_ui(input, |ui| {
        crate::ui::dock::draw(ui, state, None);
        crate::ui::panels::file_prompts::draw(ui.ctx(), state);
    })
}

/// Every labelled widget a frame laid out, and its centre.
pub(super) fn labels(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    let Some(update) = &output.platform_output.accesskit_update else {
        return Vec::new();
    };
    update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            let text = node.label().or_else(|| node.value())?;
            let b = node.bounds()?;
            let centre = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
            Some((text.to_string(), centre))
        })
        .collect()
}

pub(super) fn frame(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    labels(&run(ctx, state, events, egui::Modifiers::NONE))
}

/// Where the workspace on screen last laid out the leaf holding `panel`.
pub(super) fn leaf_of(state: &AppState, panel: &str) -> egui::Rect {
    state
        .workspace_docks
        .peek(state.active_workspace)
        .and_then(|dock| {
            dock.iter_all_nodes().find_map(|(_, node)| match node {
                egui_dock::Node::Leaf(leaf) if leaf.tabs.iter().any(|t| t.id() == panel) => {
                    Some(leaf.rect)
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("{:?} has no {panel}", state.active_workspace))
}

/// An empty spot in the listing, below its rows and left of the preview pane.
pub(super) fn in_the_listing(state: &AppState) -> egui::Pos2 {
    let centre = leaf_of(state, ids::DOCUMENT);
    egui::pos2(centre.left() + 120.0, centre.bottom() - 70.0)
}

/// An archive in Browse: `props` holding two drawables, `textures` an empty
/// folder, and two files beside them.
pub(super) fn browsing() -> (AppState, DocumentId) {
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.new_empty_document(DocumentKind::Rpf);
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("an empty archive document holds an archive");
    };
    let root = rpf.session.root_id();
    let props = rpf.session.add_folder(root, "props").expect("a folder");
    for (name, size) in [("lamp.ydr", 300), ("chair.ydr", 100)] {
        rpf.session
            .add_file_copy(props, name, vec![0; size])
            .expect("a drawable");
    }
    rpf.session.add_folder(root, "textures").expect("a folder");
    rpf.session
        .add_file_copy(root, "notes.txt", b"a note".to_vec())
        .expect("a text file");
    rpf.session
        .add_file_copy(root, "big.bin", vec![7; 5000])
        .expect("a binary file");
    state.set_active_document(doc);
    // The default view and order, whatever the preferences held.
    state.prefs.explorer = Default::default();
    (state, doc)
}

pub(super) fn node(state: &AppState, doc: DocumentId, path: &str) -> RpfNodeId {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        panic!("the archive is open");
    };
    match super::navigation::resolve_path(&rpf.session, path) {
        super::navigation::Resolved::Found(id) => id,
        other => panic!("{path}: {other:?}"),
    }
}

fn name_of(state: &AppState, doc: DocumentId, id: RpfNodeId) -> String {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.node(id).map(|n| n.name.clone()),
        _ => None,
    }
    .unwrap_or_default()
}

/// The names the Files view listed last frame, in order.
pub(super) fn listed(state: &AppState, doc: DocumentId) -> Vec<String> {
    state
        .panels
        .peek::<FilesViewUi>()
        .map(|view| {
            view.listed
                .iter()
                .map(|&id| name_of(state, doc, id))
                .collect()
        })
        .unwrap_or_default()
}

/// The names selected, the focus first.
fn selected(state: &AppState, doc: DocumentId) -> Vec<String> {
    state
        .selection
        .selected_ids(doc)
        .into_iter()
        .map(|id| name_of(state, doc, id))
        .collect()
}

fn button(at: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(super) fn click_with(
    ctx: &egui::Context,
    state: &mut AppState,
    at: egui::Pos2,
    which: egui::PointerButton,
) {
    frame(
        ctx,
        state,
        vec![egui::Event::PointerMoved(at), button(at, which, true)],
    );
    frame(ctx, state, vec![button(at, which, false)]);
}

pub(super) fn click(ctx: &egui::Context, state: &mut AppState, at: egui::Pos2) {
    click_with(ctx, state, at, egui::PointerButton::Primary);
}

/// What the submenu `opener` of the open menu `menu` offers, once a click on its row has opened it.
pub(super) fn submenu(
    ctx: &egui::Context,
    state: &mut AppState,
    menu: &[(String, egui::Pos2)],
    opener: &str,
) -> Vec<(String, egui::Pos2)> {
    let arrow = egui::containers::menu::SubMenuButton::RIGHT_ARROW;
    let at = (menu.iter())
        .find(|(text, _)| text.trim_end_matches(arrow).trim_end() == opener)
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("no `{opener}` to open in {menu:?}"));
    click(ctx, state, at);
    frame(ctx, state, Vec::new())
}

/// Press `key` with the pointer over the listing, then draw a frame for what it did.
fn key_with(ctx: &egui::Context, state: &mut AppState, key: egui::Key, modifiers: egui::Modifiers) {
    let at = in_the_listing(state);
    let event = egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    run(
        ctx,
        state,
        vec![egui::Event::PointerMoved(at), event],
        modifiers,
    );
    frame(ctx, state, Vec::new());
}

fn key(ctx: &egui::Context, state: &mut AppState, key: egui::Key) {
    key_with(ctx, state, key, egui::Modifiers::NONE);
}

fn type_text(ctx: &egui::Context, state: &mut AppState, text: &str) {
    let at = in_the_listing(state);
    frame(
        ctx,
        state,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::Text(text.to_string()),
        ],
    );
    frame(ctx, state, Vec::new());
}

/// The centre of the first widget labelled `label`, after any icon glyphs, inside `area`.
pub(super) fn find(shown: &[(String, egui::Pos2)], area: egui::Rect, label: &str) -> egui::Pos2 {
    shown
        .iter()
        .find(|(text, at)| labelled(text, label) && area.contains(*at))
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("no `{label}` in {area:?}: {shown:?}"))
}

/// Whether a widget's text is `label`, behind the glyphs a row's icon puts first.
fn labelled(text: &str, label: &str) -> bool {
    text.trim_start_matches(|c: char| ('\u{e000}'..='\u{f8ff}').contains(&c)) == label
}

pub(super) fn settled(ctx: &egui::Context, state: &mut AppState) -> Vec<(String, egui::Pos2)> {
    frame(ctx, state, Vec::new());
    frame(ctx, state, Vec::new())
}

pub(super) fn accessible() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx
}

#[test]
fn the_centre_of_browse_is_the_files_view_of_the_folder_selected_in_files() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    assert_eq!(
        listed(&state, doc),
        ["props", "textures", "big.bin", "notes.txt"],
        "the centre lists the archive's top folder"
    );

    let files = leaf_of(&state, ids::FILES);
    let props = find(&shown, files, "props");
    click(&ctx, &mut state, props);
    let shown = frame(&ctx, &mut state, Vec::new());
    assert_eq!(listed(&state, doc), ["chair.ydr", "lamp.ydr"]);
    assert!(
        shown
            .iter()
            .any(|(text, at)| labelled(text, "props") && files.contains(*at)),
        "Files on the left stays the tree"
    );
    assert_eq!(state.active_workspace, crate::ui::dock::Workspace::Browse);
}

#[test]
fn details_shows_every_column_in_words_and_a_header_sorts_by_its_column() {
    let (mut state, doc) = browsing();
    state.panels.get::<FilesViewUi>().preview_hidden = true;
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let centre = leaf_of(&state, ids::DOCUMENT);
    for title in [
        "Name ⏶",
        "Type",
        "Size",
        "Stored",
        "Compression",
        "Encryption",
        "Edition",
        "Version",
    ] {
        find(&shown, centre, title);
    }
    for words in ["Folder", "BIN file", "Text Document"] {
        find(&shown, centre, words);
    }

    click(&ctx, &mut state, find(&shown, centre, "Size"));
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(
        state.prefs.explorer.sort.key,
        super::explorer::SortKey::Size
    );
    assert_eq!(
        listed(&state, doc),
        ["props", "textures", "notes.txt", "big.bin"],
        "folders lead and files run smallest first"
    );
    let shown = frame(&ctx, &mut state, Vec::new());
    click(&ctx, &mut state, find(&shown, centre, "Size ⏶"));
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(
        listed(&state, doc),
        ["props", "textures", "big.bin", "notes.txt"],
        "again, and the files run largest first"
    );
}

/// An archive built in memory: a compressed binary file and a resource whose
/// flags carry a Legacy version, as an archive on disk states them.
fn built_archive() -> (super::rpf_session::RpfEditSession, RpfNodeId, RpfNodeId) {
    use or_io::container::resource::{ResourceType, Rsc7};
    use or_io::container::rpf::RpfBuilder;

    let format = or_io::gta5::formats::format("ydr").expect("declared");
    let version = ResourceType::Ydr
        .read_versions()
        .into_iter()
        .find(|&v| format.target_of_version(v).ok() == Some(or_io::Generation::Gen8))
        .expect("a Legacy version");
    let drawable = [
        Rsc7::invent_single_chunk_seal(version, 0x2000)
            .expect("a seal")
            .header_bytes(),
        vec![0u8; 64],
    ]
    .concat();
    let archive = RpfBuilder::default()
        .insert_with("data/settings.meta", b"<Settings />\n".repeat(400), true)
        .expect("a binary file")
        .insert_with("models/bench.ydr", drawable, false)
        .expect("a resource")
        .build()
        .expect("opens");
    let session = super::rpf_session::RpfEditSession::from_archive(archive).expect("a session");
    let find = |path: &str| match super::navigation::resolve_path(&session, path) {
        super::navigation::Resolved::Found(id) => id,
        other => panic!("{path}: {other:?}"),
    };
    let (meta, ydr) = (find("data/settings.meta"), find("models/bench.ydr"));
    (session, meta, ydr)
}

#[test]
fn a_rows_columns_read_its_entry_in_the_archive() {
    let (session, meta, ydr) = built_archive();

    let text = row_facts(&session, meta).expect("the binary file's columns");
    assert_eq!(text.kind, "Metadata");
    assert_eq!(text.size, Some(13 * 400));
    assert!(
        text.stored.is_some_and(|s| s < 13 * 400),
        "a compressed file takes less in the archive: {text:?}"
    );
    assert_eq!(text.compression, Some("Deflate"));
    assert_eq!(text.encryption, Some("None"));
    assert_eq!((text.edition, text.version), (None, None));

    let model = row_facts(&session, ydr).expect("the resource's columns");
    assert_eq!(model.kind, "Drawable");
    assert_eq!(model.compression, Some("Resource"));
    assert_eq!(model.edition, Some(or_io::Generation::Gen8.edition_label()));
    let version = model.version.expect("a resource states its version");
    assert_eq!(
        or_io::gta5::formats::format("ydr").and_then(|f| f.target_of_version(version).ok()),
        Some(or_io::Generation::Gen8)
    );

    let folder = row_facts(&session, session.root_id()).expect("the root's columns");
    assert_eq!(folder.kind, "Folder");
    assert_eq!(folder.size, None);
}

#[test]
fn a_hash_is_the_names_without_its_extension() {
    assert_eq!(name_hash("adder.yft"), or_io::hash::string_hash(b"adder"));
    assert_ne!(
        name_hash("adder.yft"),
        or_io::hash::string_hash(b"adder.yft")
    );
    assert_eq!(
        name_hash("ADDER.YFT"),
        name_hash("adder.yft"),
        "as RAGE folds case"
    );
}

#[test]
fn the_status_line_counts_the_items_the_selection_and_their_size() {
    let (mut state, _) = browsing();
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let centre = leaf_of(&state, ids::DOCUMENT);
    find(&shown, centre, "4 items   ·   4.9 KiB in this folder");

    click(&ctx, &mut state, find(&shown, centre, "big.bin"));
    let shown = frame(&ctx, &mut state, Vec::new());
    find(
        &shown,
        centre,
        "4 items   ·   1 selected, 4.9 KiB   ·   4.9 KiB in this folder",
    );
}

#[test]
fn the_address_bar_goes_where_a_typed_path_points() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    let shown = settled(&ctx, &mut state);
    let centre = leaf_of(&state, ids::DOCUMENT);
    let bar = shown
        .iter()
        .find(|(text, at)| text == "Untitled.rpf *" && centre.contains(*at))
        .map(|(_, at)| *at)
        .expect("the address bar's first step names the archive");
    click(&ctx, &mut state, bar + egui::vec2(300.0, 0.0));
    assert!(
        state
            .panels
            .peek::<FilesViewUi>()
            .is_some_and(|v| v.address.is_some()),
        "a click past the steps types a path"
    );
    frame(&ctx, &mut state, Vec::new());
    frame(&ctx, &mut state, vec![egui::Event::Text("/PROPS".into())]);
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, &mut state, vec![enter]);
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(listed(&state, doc), ["chair.ydr", "lamp.ydr"]);

    assert!(state.go_to_address(doc, "nowhere/at/all").is_err());
}

#[test]
fn a_typed_path_is_read_inside_the_documents_file() {
    let root = std::path::Path::new("C:/Games/GTAV");
    assert_eq!(
        inner_of_address(Some(root), "C:\\Games\\GTAV\\x64a.rpf\\textures"),
        Ok("x64a.rpf/textures".to_string())
    );
    assert_eq!(
        inner_of_address(Some(root), "c:/games/gtav"),
        Ok(String::new()),
        "the root itself, whatever its case"
    );
    assert_eq!(
        inner_of_address(Some(root), "/x64a.rpf/levels/"),
        Ok("x64a.rpf/levels".to_string()),
        "a leading slash is a path inside, as Copy path writes one"
    );
    assert!(inner_of_address(Some(root), "D:/Elsewhere/x64a.rpf").is_err());
    assert!(
        inner_of_address(Some(root), "C:/Games/GTAVX/a.rpf").is_err(),
        "a folder whose name only starts the same is not inside"
    );
}

#[test]
fn a_typed_address_the_roots_length_cuts_inside_a_letter_of_is_refused_not_a_panic() {
    let root = std::path::Path::new("C:/Games/GTAV");
    // The root is 13 bytes long, and the address's 13th falls inside `Ö`.
    let typed = "D:/Spiele/ÄÖ";
    assert!(typed.len() > 13 && !typed.is_char_boundary(13));
    let said = inner_of_address(Some(root), typed);
    assert!(
        said.as_ref()
            .is_err_and(|why| why.contains("is not inside")),
        "{said:?}"
    );
    assert_eq!(
        inner_of_address(Some(root), "C:/Games/GTAV/Über/Ärger"),
        Ok("Über/Ärger".to_string()),
        "a path inside whose names are not ASCII is read whole"
    );
}

#[test]
fn a_backslash_address_is_on_disk_for_a_lowercase_drive_and_for_a_share() {
    let root = std::path::Path::new("C:/Games/GTAV");
    assert_eq!(
        inner_of_address(Some(root), "c:\\games\\gtav\\x64a.rpf"),
        Ok("x64a.rpf".to_string()),
        "a lowercase drive, whatever the slashes"
    );
    assert!(inner_of_address(Some(root), "d:\\games\\gtav\\x64a.rpf").is_err());

    let share = std::path::Path::new("\\\\server\\share\\GTAV");
    assert_eq!(
        inner_of_address(Some(share), "\\\\server\\share\\GTAV\\x64a.rpf\\levels"),
        Ok("x64a.rpf/levels".to_string()),
        "a share is on disk, and read inside its folder"
    );
    assert!(
        inner_of_address(Some(root), "\\\\server\\share\\GTAV\\x64a.rpf").is_err(),
        "a share is not inside a drive's folder"
    );
    assert!(
        inner_of_address(None, "c:\\games\\gtav").is_err(),
        "a document with no file has nothing on disk to be inside"
    );
}
