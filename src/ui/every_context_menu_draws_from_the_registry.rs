//! Every context menu offers each app command as the registry has it for what
//! was clicked: its label, its binding and whether it can run.
//!
//! ⚠ Read through AccessKit, with a binding given to every command a menu
//! offers, so an item drawn outside the registry fails here: it has no binding
//! to show.

use std::collections::HashSet;

use egui::accesskit::{NodeId, Role};

use crate::state::{AppState, DocPayload, DocumentId, Prefs, RpfNodeId};
use crate::ui::commands::{self, Action, Hits, Target, COMMANDS};
use crate::ui::panels::registry::ids;
use crate::ui::panels::selection_ids::{ypt_effect_id, ypt_emitter_lib_id, ypt_particle_lib_id};
use crate::ui::shortcuts::{self, ShortcutMods};

/// A widget one frame drew, as AccessKit describes it.
#[derive(Debug, Clone)]
pub(super) struct Drawn {
    id: NodeId,
    role: Role,
    pub(super) label: String,
    disabled: bool,
    pub(super) centre: egui::Pos2,
}

type Draw = fn(&mut egui::Ui, &mut AppState);

pub(super) fn shell(ui: &mut egui::Ui, state: &mut AppState) {
    crate::ui::panels::document_tabs::draw(ui, state);
    crate::ui::dock::draw(ui, state, None);
}

fn search_panel(ui: &mut egui::Ui, state: &mut AppState) {
    crate::ui::panels::search::draw(ui, state);
}

pub(super) fn frame(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
    draw: Draw,
) -> Vec<Drawn> {
    state.poll_expansions();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| draw(ui, state));
    let Some(update) = output.platform_output.accesskit_update else {
        return Vec::new();
    };
    (update.nodes.iter())
        .filter_map(|(id, node)| {
            let b = node.bounds()?;
            Some(Drawn {
                id: *id,
                role: node.role(),
                label: node.label().or_else(|| node.value())?.to_string(),
                disabled: node.is_disabled(),
                centre: egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32),
            })
        })
        .collect()
}

/// Draws until a widget labelled `label` is inside `area`, and says where it is.
pub(super) fn find(
    ctx: &egui::Context,
    state: &mut AppState,
    draw: Draw,
    area: Area,
    label: &str,
) -> egui::Pos2 {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let rect = area(state);
        let shown = frame(ctx, state, Vec::new(), draw);
        // ⚠ The lowest: a breadcrumb or the preview pane above it names the row too.
        let found = (shown.iter())
            .filter(|d| named(&d.label, label) && rect.contains(d.centre))
            .max_by(|a, b| a.centre.y.total_cmp(&b.centre.y));
        if let Some(found) = found {
            return found.centre;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "nothing labelled {label} was drawn in {rect:?}: {:?}",
            shown.iter().map(|d| &d.label).collect::<Vec<_>>()
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Where a menu is opened: a panel's leaf, or the whole screen.
type Area = fn(&AppState) -> egui::Rect;

fn everywhere(_: &AppState) -> egui::Rect {
    egui::Rect::EVERYTHING
}

fn leaf(state: &AppState, panel: &str) -> egui::Rect {
    (state.workspace_docks.peek(state.active_workspace))
        .and_then(|dock| {
            dock.iter_all_nodes().find_map(|(_, node)| match node {
                egui_dock::Node::Leaf(leaf) if leaf.tabs.iter().any(|t| t.id() == panel) => {
                    Some(leaf.rect)
                }
                _ => None,
            })
        })
        .unwrap_or(egui::Rect::NOTHING)
}

pub(super) fn in_files(state: &AppState) -> egui::Rect {
    leaf(state, ids::FILES)
}

fn in_the_centre(state: &AppState) -> egui::Rect {
    leaf(state, ids::DOCUMENT)
}

fn in_the_outline(state: &AppState) -> egui::Rect {
    leaf(state, ids::OUTLINE)
}

fn in_the_viewport(state: &AppState) -> egui::Rect {
    leaf(state, ids::VIEWPORT_3D)
}

fn in_the_gallery(state: &AppState) -> egui::Rect {
    leaf(state, ids::GALLERY)
}

fn in_the_timeline(state: &AppState) -> egui::Rect {
    leaf(state, ids::TIMELINE)
}

/// Whether a widget's text is `label`, behind the glyph a row's icon puts first.
fn named(text: &str, label: &str) -> bool {
    text.trim_start_matches(|c: char| ('\u{e000}'..='\u{f8ff}').contains(&c) || c == ' ') == label
}

/// The items of the menu a right click at `at` opens, which is closed again.
///
/// ⚠ Told from what the frame draws once the menu has closed, not before it
/// opened: a right click can select its row, which draws the rows anew.
fn right_click(
    ctx: &egui::Context,
    state: &mut AppState,
    at: egui::Pos2,
    draw: Draw,
) -> Vec<Drawn> {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        ctx,
        state,
        vec![egui::Event::PointerMoved(at), press(true)],
        draw,
    );
    frame(ctx, state, vec![press(false)], draw);
    let mut open = frame(ctx, state, Vec::new(), draw);
    // ⚠ The comparisons, the edits of a row and what is copied of it are each one row that opens them. Each is clicked open and not hovered: a pointer that comes down to a row still reads as moving toward the submenu open beside the row above, which then stays open.
    let arrow = egui::containers::menu::SubMenuButton::RIGHT_ARROW;
    for opener in ["Compare", "Edit", "Copy as"] {
        let over = (open.iter())
            .find(|d| d.label.trim_end_matches(arrow).trim_end() == opener)
            .map(|d| d.centre);
        if let Some(over) = over {
            let click = |pressed| egui::Event::PointerButton {
                pos: over,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let moved = egui::Event::PointerMoved(over);
            frame(ctx, state, vec![moved, click(true)], draw);
            frame(ctx, state, vec![click(false)], draw);
            let first = frame(ctx, state, Vec::new(), draw);
            open = frame(ctx, state, Vec::new(), draw)
                .into_iter()
                .chain(first)
                .chain(open)
                .collect();
        }
    }
    let escape = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(ctx, state, vec![escape.clone()], draw);
    frame(ctx, state, vec![escape], draw);
    let closed: HashSet<NodeId> = frame(ctx, state, Vec::new(), draw)
        .iter()
        .map(|d| d.id)
        .collect();
    (open.into_iter())
        .filter(|d| !closed.contains(&d.id) && matches!(d.role, Role::Button | Role::CheckBox))
        .collect()
}

/// One menu: what it was opened on, the app commands it offers, and the items that are its own.
struct Menu {
    name: &'static str,
    target: Target,
    offers: Vec<Action>,
    local: &'static [&'static str],
}

/// Each way `drawn` differs from the registry, and each item it draws outside it.
fn differences(state: &AppState, menu: &Menu, drawn: &[Drawn]) -> Vec<String> {
    let mut found = Vec::new();
    let mut matched = HashSet::new();
    for &action in &menu.offers {
        // ⚠ A command that acts on no row, as Frame all in the viewport's menu, is the one the palette has.
        let command = commands::on(state, action, menu.target.clone())
            .or_else(|| commands::of(state, action));
        let Some(command) = command else {
            found.push(format!(
                "{}: {action:?} acts on nothing it can be given",
                menu.name
            ));
            continue;
        };
        let label = match &command.binding {
            Some(binding) => format!("{} {binding}", command.label),
            None => command.label.clone(),
        };
        match drawn.iter().find(|d| d.label == label) {
            None => found.push(format!("{}: no `{label}`", menu.name)),
            Some(item) if item.disabled != command.enabled.is_err() => found.push(format!(
                "{}: `{label}` is drawn {}, and the registry says {:?}",
                menu.name,
                if item.disabled { "disabled" } else { "enabled" },
                command.enabled
            )),
            Some(item) => {
                matched.insert(item.id);
            }
        }
    }
    let arrow = egui::containers::menu::SubMenuButton::RIGHT_ARROW;
    for item in drawn.iter().filter(|d| !matched.contains(&d.id)) {
        let name = item.label.trim_end_matches(arrow).trim_end();
        if !menu.local.contains(&name) {
            found.push(format!(
                "{}: `{}` is drawn outside the registry",
                menu.name, item.label
            ));
        }
    }
    found
}

/// A key for every command a menu offers that has none by default.
fn bind_every_command(prefs: &mut Prefs) {
    let alt = ShortcutMods {
        ctrl: false,
        control: false,
        shift: false,
        alt: true,
    };
    for (action, key) in [
        (Action::OpenSelection, egui::Key::O),
        (Action::ImportIntoProject, egui::Key::Y),
        (Action::RevealInTree, egui::Key::T),
        (Action::RevealOnDisk, egui::Key::E),
        (Action::Extract, egui::Key::X),
        (Action::ExportAs, egui::Key::A),
        (Action::Convert, egui::Key::V),
        (Action::ExportGltf, egui::Key::L),
        (Action::ImportGltf, egui::Key::K),
        (Action::NewFolder, egui::Key::N),
        (Action::ImportFile, egui::Key::I),
        (Action::CopyPath, egui::Key::P),
        (Action::CopyName, egui::Key::M),
        (Action::CopyHash, egui::Key::H),
        (Action::Properties, egui::Key::R),
        (Action::AddToFavourites, egui::Key::F),
        (Action::RemoveFromFavourites, egui::Key::G),
        (Action::CreateDlcPack, egui::Key::D),
        (Action::CloseOtherTabs, egui::Key::W),
        (Action::SelectForCompare, egui::Key::B),
        (Action::CompareWithSelected, egui::Key::J),
        (Action::CompareWithOriginal, egui::Key::U),
        (Action::ReplaceTextureFromImage, egui::Key::Q),
        (Action::ExportTexture, egui::Key::Z),
    ] {
        let bound = shortcuts::set_binding(prefs, action, alt, key);
        assert!(bound.is_ok(), "Alt+{key:?} for {action:?}: {bound:?}");
    }
}

fn node(state: &AppState, doc: DocumentId, path: &str) -> RpfNodeId {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        panic!("the folder is open");
    };
    match crate::state::navigation::resolve_path(&rpf.session, path) {
        crate::state::navigation::Resolved::Found(id) => id,
        other => panic!("{path}: {other:?}"),
    }
}

const ROW: &[&str] = &[
    "New file",
    "More extract options",
    "Move to",
    "Compare",
    "Edit",
    "Copy as",
];
const MODEL_ROW: &[&str] = &[
    "New file",
    "More extract options",
    "Move to",
    "Compare",
    "Uses",
    "Placed by",
    "Edit",
    "Copy as",
];
const DUPLICATE: &[&str] = &["Duplicate"];
const FIND_USAGES: &[&str] = &["Find usages"];

#[test]
fn every_context_menu_offers_its_app_commands_with_the_registrys_label_binding_and_enabled_state() {
    use Action::*;

    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::create_dir(dir.path().join("props")).expect("a folder");
    std::fs::write(dir.path().join("props").join("lamp.bin"), [1; 64]).expect("a file");
    std::fs::write(dir.path().join("notes.txt"), b"a note").expect("a file");
    std::fs::write(dir.path().join("big.bin"), [7; 200]).expect("a file");
    std::fs::write(dir.path().join("model.ydr"), [0; 64]).expect("a file");
    std::fs::write(dir.path().join("pack.rpf"), [0; 64]).expect("a file");
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(dir.path().to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    // ⚠ After the open, which saves the preferences that a later test's
    // `AppState::default()` reads; the end asserts nothing below writes them.
    bind_every_command(&mut state.prefs);
    let writes = crate::state::prefs::writes_on_this_thread();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let (mut found, mut offered, mut walked) = (Vec::new(), HashSet::new(), 0);
    let mut open = |state: &mut AppState, draw: Draw, area: Area, row: &str, menu: Menu| {
        let at = match row {
            "" => in_the_listing(state),
            row => find(&ctx, state, draw, area, row),
        };
        let drawn = right_click(&ctx, state, at, draw);
        found.extend(differences(state, &menu, &drawn));
        offered.extend(menu.offers.iter().copied());
        walked += 1;
    };

    find(&ctx, &mut state, shell, in_the_centre, "notes.txt");
    let (notes, props, big, model) = (
        node(&state, doc, "notes.txt"),
        node(&state, doc, "props"),
        node(&state, doc, "big.bin"),
        node(&state, doc, "model.ydr"),
    );
    let rows = |state: &AppState, rows: Vec<RpfNodeId>, centre| {
        Target::rows(state, doc, rows, centre).expect("rows of the folder")
    };
    let compare = [SelectForCompare, CompareWithSelected, CompareWithOriginal];
    let file_row = vec![
        OpenSelection,
        ImportIntoProject,
        RevealOnDisk,
        NewFolder,
        ImportFile,
        SelectForCompare,
        CompareWithSelected,
        CompareWithOriginal,
        Extract,
        Cut,
        Copy,
        Paste,
        Rename,
        Delete,
        CopyPath,
        CopyName,
        CopyHash,
        ExportAs,
        AddToFavourites,
        Properties,
    ];
    let folder_row: Vec<Action> = (file_row.iter().copied())
        .filter(|&a| a != OpenSelection && a != ExportAs && !compare.contains(&a))
        .chain([CreateDlcPack])
        .collect();

    let target = rows(&state, vec![notes], false);
    let menu = Menu {
        name: "a file's row in Files",
        target,
        offers: file_row.clone(),
        local: ROW,
    };
    open(&mut state, shell, in_files, "notes.txt", menu);

    let target = rows(&state, vec![props], false);
    let menu = Menu {
        name: "a folder's row in Files",
        target,
        offers: folder_row,
        local: ROW,
    };
    open(&mut state, shell, in_files, "props", menu);

    // An archive compares entry by entry, so its row offers what a file's does.
    let archive_row: Vec<Action> = (file_row.iter().copied())
        .filter(|&a| a != OpenSelection && a != ExportAs)
        .collect();
    let menu = Menu {
        name: "an archive's row in Files",
        target: rows(&state, vec![node(&state, doc, "pack.rpf")], false),
        offers: archive_row,
        local: &[
            "New file",
            "More extract options",
            "Move to",
            "Compare",
            "Defragment…",
            "Edit",
            "Copy as",
        ],
    };
    open(&mut state, shell, in_files, "pack.rpf", menu);

    state.selection.select(doc, notes);
    state.selection.toggle(doc, big);
    let menu = Menu {
        name: "two rows selected in Files",
        target: rows(&state, vec![notes, big], false),
        offers: vec![
            ImportIntoProject,
            Extract,
            Cut,
            Copy,
            Paste,
            CopyPath,
            Rename,
            Delete,
        ],
        local: &["More extract options"],
    };
    open(&mut state, shell, in_files, "notes.txt", menu);

    let title = state.document(doc).map(|d| d.title()).expect("open");
    let menu = Menu {
        name: "an Open Folder document's row in Files",
        target: Target::Document(doc),
        offers: vec![
            CloseDocument,
            AddToFavourites,
            RevealOnDisk,
            CopyPath,
            CreateDlcPack,
            Properties,
        ],
        local: &["New"],
    };
    open(&mut state, shell, in_files, &title, menu);

    state.selection.clear_all();
    let mut drawable_row = file_row.clone();
    drawable_row.extend([Convert, ExportGltf, ImportGltf]);
    let menu = Menu {
        name: "a drawable's row in the Files view",
        target: rows(&state, vec![model], true),
        offers: drawable_row,
        local: MODEL_ROW,
    };
    open(&mut state, shell, in_the_centre, "model.ydr", menu);

    let place = state.place_of(crate::state::navigation::Location { doc, node: props });
    (state.prefs.explorer).toggle_favourite(place.expect("a folder on disk has a place"));
    let mut favourite_folder = file_row.clone();
    favourite_folder.retain(|&a| a != ExportAs && !compare.contains(&a));
    favourite_folder.push(CreateDlcPack);
    favourite_folder.iter_mut().for_each(|a| {
        if *a == AddToFavourites {
            *a = RemoveFromFavourites;
        }
    });
    let menu = Menu {
        name: "a favourite folder's row in the Files view",
        target: rows(&state, vec![props], true),
        offers: favourite_folder,
        local: ROW,
    };
    open(&mut state, shell, in_the_centre, "props", menu);

    let root = node(&state, doc, "");
    let menu = Menu {
        name: "the Files view's empty space",
        target: rows(&state, vec![root], true),
        offers: vec![NewFolder, ImportFile, Paste, Properties],
        local: &["New file"],
    };
    open(&mut state, shell, in_the_centre, "", menu);

    let menu = Menu {
        name: "a tab",
        target: Target::Document(doc),
        offers: vec![CloseDocument, CloseOtherTabs, CopyPath, RevealOnDisk],
        local: &["Activate"],
    };
    open(&mut state, shell, tab_strip, &title, menu);

    let hierarchy = crate::ui::panels::hierarchy::ui_state(&mut state);
    hierarchy.filter = "**/*.txt".into();
    hierarchy.deep = true;
    let menu = Menu {
        name: "a result of a search inside archives",
        target: Target::Hits {
            list: Hits::Inside(doc),
            rows: vec![0],
        },
        offers: vec![OpenSelection, RevealInTree, Extract, CopyPath],
        local: &[],
    };
    open(&mut state, shell, in_files, "doc:/notes.txt", menu);
    crate::ui::panels::hierarchy::ui_state(&mut state)
        .filter
        .clear();

    let game = crate::state::Game::GtaVLegacy;
    state
        .prefs
        .game_paths
        .set(game, Some(dir.path().to_path_buf()));
    state.prefs.active_game = Some(game);
    state.search.root = Some(dir.path().to_path_buf());
    state.search.hits = vec![or_title::install::content::ContentHit {
        path: "notes.txt".into(),
        container: or_title::install::LOOSE.into(),
        offsets: vec![0],
        file_size: 6,
        preview: None,
    }];
    let menu = Menu {
        name: "a result in the Search panel",
        target: Target::Hits {
            list: Hits::Search,
            rows: vec![0],
        },
        offers: vec![OpenSelection, RevealInTree, Extract, CopyPath],
        local: &[],
    };
    open(
        &mut state,
        search_panel,
        everywhere,
        "notes.txt  (1 hit)",
        menu,
    );

    let ypt = particles(&mut state);
    for (name, id, row, local) in [
        (
            "an effect in a particle file's outline",
            ypt_effect_id(0),
            "fx",
            DUPLICATE,
        ),
        (
            "an emitter rule in the outline",
            ypt_emitter_lib_id(0),
            "em",
            FIND_USAGES,
        ),
        (
            "a particle rule in the outline",
            ypt_particle_lib_id(0),
            "pt",
            FIND_USAGES,
        ),
    ] {
        crate::ui::panels::hierarchy::ui_state(&mut state).scroll_to = Some((ypt, id));
        let menu = Menu {
            name,
            target: Target::Particle { doc: ypt, id },
            offers: vec![Delete],
            local,
        };
        open(&mut state, shell, in_the_outline, row, menu);
    }

    let ydr = state.new_empty_document(crate::state::DocumentKind::Ydr);
    let title = state.tab_title(state.document(ydr).expect("open"));
    let menu = Menu {
        name: "a drawable's tab",
        target: Target::Document(ydr),
        offers: vec![
            CloseDocument,
            CloseOtherTabs,
            CopyPath,
            RevealOnDisk,
            Convert,
            ExportGltf,
            ImportGltf,
        ],
        local: &["Activate"],
    };
    open(&mut state, shell, tab_strip, &format!("{title} ●"), menu);
    let title = state.document(ydr).map(|d| d.title()).expect("open");
    let menu = Menu {
        name: "a drawable's row in the Outline",
        target: Target::Document(ydr),
        offers: vec![CloseDocument, AddToFavourites, ExportGltf, ImportGltf],
        local: &["New"],
    };
    // The dot is the unsaved marker the row's icon carries.
    open(
        &mut state,
        shell,
        in_the_outline,
        &format!("●{title}"),
        menu,
    );

    // The menus over a view, which name no row: each command is the table's for the document in front.
    let gltf = state.offers_gltf(crate::state::gltf::Model::Document(ydr));
    let menu = Menu {
        name: "the viewport over a drawable",
        target: Target::Document(ydr),
        offers: [FrameAll, FrameSelection]
            .into_iter()
            .chain(
                gltf.then_some([ExportGltf, ImportGltf])
                    .into_iter()
                    .flatten(),
            )
            .collect(),
        local: &[],
    };
    open(&mut state, shell, in_the_viewport, "3D viewport", menu);

    let ytd = state.new_empty_document(crate::state::DocumentKind::Ytd);
    let Some(DocPayload::Resource(or_io::Asset::TextureDict(dictionary))) =
        state.document_mut(ytd).map(|d| &mut d.payload)
    else {
        panic!("an empty texture dictionary is a Legacy one");
    };
    dictionary.insert(or_io::gta5::gen8::GrcTexture {
        name: "lone".to_string(),
        width: 4,
        height: 4,
        ..Default::default()
    });
    let menu = Menu {
        name: "a texture's tile in the gallery",
        target: Target::Document(ytd),
        offers: vec![ReplaceTextureFromImage, ExportTexture],
        local: &[],
    };
    open(&mut state, shell, in_the_gallery, "lone", menu);

    let (lamp, name) = gta6_drawable(&mut state);
    let menu = Menu {
        name: "a GTA VI drawable's texture in the Gallery",
        target: Target::Texture { doc: lamp, name },
        offers: vec![ReplaceTextureFromImage, ExportTexture],
        local: &[],
    };
    // ⚠ A tile is painted and has no name to find: the first is under the Textures button of the Gallery's bar.
    let bar = find(&ctx, &mut state, shell, in_the_gallery, "Textures");
    let drawn = right_click(&ctx, &mut state, bar + egui::vec2(0.0, 80.0), shell);
    found.extend(differences(&state, &menu, &drawn));
    offered.extend(menu.offers.iter().copied());
    walked += 1;

    // ⚠ Its items are the transport's own, so the menu is walked for what it draws beside them.
    state.set_active_document(ypt);
    state.active_workspace = crate::ui::dock::Workspace::Particle;
    let at = find(&ctx, &mut state, shell, in_the_timeline, "Timeline");
    let drawn = right_click(&ctx, &mut state, at, shell);
    let mut labels: Vec<&str> = drawn.iter().map(|d| d.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(
        labels,
        ["Go to the start", "Play", "Show the whole length"],
        "the timeline's menu"
    );

    for spec in COMMANDS.iter().filter(|spec| spec.on.is_some()) {
        if !offered.contains(&spec.action) {
            found.push(format!("no menu walked offers {:?}", spec.action));
        }
    }
    assert!(
        found.is_empty(),
        "{} differences from the registry in {walked} menus:\n{}",
        found.len(),
        found.join("\n")
    );
    assert_eq!(
        crate::state::prefs::writes_on_this_thread(),
        writes,
        "the bindings given here were saved for the tests after this one"
    );
}

/// One command, three ways in: a row's menu names the row clicked, and the
/// palette and the command's key act on the row selected in the focused panel.
#[test]
fn a_command_acts_on_the_row_clicked_in_a_menu_and_on_the_selection_from_the_palette_and_its_key() {
    use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    let doc = state.new_empty_document(crate::state::DocumentKind::Rpf);
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("an empty archive holds a tree");
    };
    let root = rpf.session.root_id();
    let a = rpf
        .session
        .add_file_copy(root, "a.txt", b"a".to_vec())
        .expect("a file");
    let b = rpf
        .session
        .add_file_copy(root, "b.txt", b"b".to_vec())
        .expect("a file");
    let ws = state.active_workspace;
    crate::ui::dock_commands::reveal(state.workspace_docks.get_mut(ws), ids::FILES);
    state.selection.select(doc, a);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let renaming = |state: &mut AppState| {
        let tree = crate::ui::panels::hierarchy::ui_state(state);
        tree.renaming.take().map(|r| (r.target, r.in_centre))
    };

    let at = find(&ctx, &mut state, shell, in_files, "b.txt");
    let press = |at, button, pressed| egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let secondary = egui::PointerButton::Secondary;
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::PointerMoved(at), press(at, secondary, true)],
        shell,
    );
    frame(&ctx, &mut state, vec![press(at, secondary, false)], shell);
    let menu = frame(&ctx, &mut state, Vec::new(), shell);
    // What changes the row is under Edit, which a click opens.
    let arrow = egui::containers::menu::SubMenuButton::RIGHT_ARROW;
    let edit = (menu.iter())
        .find(|d| d.label.trim_end_matches(arrow).trim_end() == "Edit")
        .unwrap_or_else(|| panic!("b.txt's menu has no Edit: {menu:?}"))
        .centre;
    let opening = egui::PointerButton::Primary;
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::PointerMoved(edit), press(edit, opening, true)],
        shell,
    );
    frame(&ctx, &mut state, vec![press(edit, opening, false)], shell);
    let menu = frame(&ctx, &mut state, Vec::new(), shell);
    let item = (menu.iter().find(|d| d.label == "Rename F2"))
        .unwrap_or_else(|| panic!("b.txt's menu offers no Rename F2: {menu:?}"))
        .centre;
    let primary = egui::PointerButton::Primary;
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::PointerMoved(item), press(item, primary, true)],
        shell,
    );
    frame(&ctx, &mut state, vec![press(item, primary, false)], shell);
    assert_eq!(
        renaming(&mut state),
        Some((b, false)),
        "the menu renames the row clicked"
    );
    // ⚠ The right click selected b, as Explorer's does; a again, so the selection is not the row clicked.
    state.selection.select(doc, a);

    let rename = (commands::all(&state).into_iter())
        .find(|command| command.label == "Rename")
        .expect("the palette offers Rename");
    assert_eq!(rename.binding.as_deref(), Some("F2"));
    commands::run(&mut state, &ctx, &rename);
    assert_eq!(
        renaming(&mut state),
        Some((a, false)),
        "the palette renames the row selected"
    );

    let f2 = shortcuts::action_for_press(
        &Key::Named(NamedKey::F2),
        PhysicalKey::Code(KeyCode::F2),
        ModifiersState::empty(),
        false,
        &state.prefs,
    )
    .expect("F2 is bound");
    commands::run_action(&mut state, &ctx, f2);
    assert_eq!(
        renaming(&mut state),
        Some((a, false)),
        "F2 renames the row selected"
    );
}

/// A particle file with an effect, an emitter rule and a particle rule, in the Particle workspace.
fn particles(state: &mut AppState) -> DocumentId {
    use or_particle::list;
    let id = state.new_empty_document(crate::state::DocumentKind::Ypt);
    let Some(DocPayload::Resource(or_io::Asset::Particles(fx))) =
        state.document_mut(id).map(|d| &mut d.payload)
    else {
        panic!("an empty particle file holds a list");
    };
    assert!(list::add_effect(fx, "fx"));
    assert!(list::add_emitter_rule(fx, "em"));
    assert!(list::add_particle_rule(fx, "pt"));
    id
}

/// A GTA VI drawable in front, whose one shader binds a key no open document holds, and the name its Gallery tile shows.
fn gta6_drawable(state: &mut AppState) -> (DocumentId, String) {
    use or_io::gta6::headerless::{Drawable, Resource, Shader};
    let shader = Shader {
        name_hash: 1,
        texture_keys: vec![Some(0x0BAD_F00D_1234_5678)],
        parameters: Vec::new(),
    };
    let drawable = Drawable {
        name: "lamp.#dr".to_string(),
        box_min: [0.0; 3],
        box_max: [1.0; 3],
        shaders: vec![shader],
        levels: [None, None, None, None],
        lod_distances: [9998.0; 4],
        bound: None,
        embedded: None,
        extra_skeletons: Vec::new(),
    };
    let payload = DocPayload::Gta6Drawable(std::sync::Arc::new(Resource::Drawable(drawable)));
    let listing = crate::state::textures::textures_in(&payload, None);
    let crate::state::TextureListing::Textures { textures, .. } = listing else {
        panic!("a drawable lists the textures it binds");
    };
    let id = state.documents.iter().map(|d| d.id).max().unwrap_or(0) + 1;
    let path = std::path::PathBuf::from("0x10.drawable");
    (state.documents).push(crate::state::Document::new(id, Some(path), payload));
    state.set_active_document(id);
    (id, textures[0].name.clone())
}

/// An empty spot in the Files view's listing, below its rows and left of the preview pane.
fn in_the_listing(state: &AppState) -> egui::Pos2 {
    let centre = in_the_centre(state);
    egui::pos2(centre.left() + 120.0, centre.bottom() - 70.0)
}

fn tab_strip(_: &AppState) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::pos2(1600.0, crate::ui::theme::TAB_STRIP_HEIGHT),
    )
}

#[test]
fn enter_in_the_field_of_files_opens_the_first_match_with_the_whole_window_drawn() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(dir.path().join("notes.txt"), b"a note").expect("a file");
    std::fs::write(dir.path().join("other.bin"), [3; 16]).expect("a file");
    let mut state = AppState::from_prefs(Prefs::default());
    state.open_folder(dir.path().to_path_buf());
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let mut turn = |state: &mut AppState, events: Vec<egui::Event>| {
        state.poll_load_result();
        state.poll_expansions();
        state.tick_navigation();
        state.tick_file_index();
        time += 1.0 / 60.0;
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            time: Some(time),
            focused: true,
            events,
            ..Default::default()
        });
        crate::ui::draw(&ctx, state, None);
        crate::app::modals::draw_editor_overlays(&ctx, state);
        let _ = ctx.end_pass();
    };
    let key = |key, pressed| egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let idle = |turn: &mut dyn FnMut(&mut AppState, Vec<egui::Event>), state: &mut AppState, n| {
        for _ in 0..n {
            turn(state, Vec::new());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    idle(&mut turn, &mut state, 40);
    // As a person does it: the palette, the command by its name, then the words in the field of Files.
    crate::ui::panels::command_palette::open(&mut state);
    idle(&mut turn, &mut state, 2);
    turn(
        &mut state,
        vec![egui::Event::Text("go to file".to_string())],
    );
    idle(&mut turn, &mut state, 2);
    turn(&mut state, vec![key(egui::Key::Enter, true)]);
    turn(&mut state, vec![key(egui::Key::Enter, false)]);
    idle(&mut turn, &mut state, 3);
    turn(&mut state, vec![egui::Event::Text("notes".to_string())]);
    idle(&mut turn, &mut state, 60);
    let finding = crate::ui::panels::hierarchy::ui_state(&mut state).open_matches;
    assert!(finding, "Go to file did not start the finding");
    let typed = crate::ui::panels::hierarchy::ui_state(&mut state)
        .filter
        .clone();
    assert_eq!(typed, "notes", "the words did not reach the field of Files");
    turn(&mut state, vec![key(egui::Key::Enter, true)]);
    turn(&mut state, vec![key(egui::Key::Enter, false)]);
    idle(&mut turn, &mut state, 200);
    let open: Vec<&str> = state.documents.iter().map(|d| d.name.as_str()).collect();
    assert!(
        open.iter().any(|name| name.starts_with("notes")),
        "Enter opened nothing: {open:?}"
    );
}
