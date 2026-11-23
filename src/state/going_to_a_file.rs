//! Go to File finds any file of the install by name, fuzzily, or by the hash of
//! its name, opens it or reveals it in Files, and narrows to the base game or
//! the mods overlay; the search of every archive reads the same index.
//!
//! ⚠ Every case starts cold: no archive on a hit's path is open in the tree,
//! and the index is built while the case types, as at a launch.

use std::path::Path;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use winit::keyboard::{Key, KeyCode, PhysicalKey};

use super::document::{DocumentId, RpfNodeId};
use super::navigation::{resolve_path, Resolved};
use super::nested_archives_survive_a_save::{archive, open, save, Opened};
use super::opening_a_ydd::node_at;
use super::prefs::EditMode;
use super::{AppState, DocPayload, DocumentKind, Game, Prefs, RetailEditDecision};

const NONE: egui::Modifiers = egui::Modifiers::NONE;

const ADDER: &str = "x64e.rpf/levels/gta5/vehicles.rpf/adder.yft";

/// The scope controls, which are not results.
const SCOPES: [&str; 3] = ["All", "Base game", "Mods overlay"];

/// Where Files is drawn; Go to File floats to the right of it.
const FILES_WIDTH: f32 = 420.0;

/// Files and Go to File drawn through real frames, polling what the editor's redraw polls.
struct Screen {
    ctx: egui::Context,
    time: f64,
}

impl Screen {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self { ctx, time: 0.0 }
    }

    /// Every labelled widget one frame lays out: its role, its label and its centre.
    fn frame(
        &mut self,
        state: &mut AppState,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> Vec<(Role, String, egui::Pos2)> {
        state.poll_load_result();
        state.poll_save_result();
        state.poll_expansions();
        state.tick_navigation();
        state.tick_file_index();
        self.time += 1.0 / 30.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            time: Some(self.time),
            modifiers,
            events,
            ..Default::default()
        };
        let output = self.ctx.run_ui(input, |ui| {
            let files = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(FILES_WIDTH, 900.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(files), |ui| {
                crate::ui::panels::hierarchy::draw(ui, state);
            });
            crate::ui::panels::command_palette::draw(ui.ctx(), state);
        });
        let Some(update) = output.platform_output.accesskit_update else {
            return Vec::new();
        };
        let mut drawn: Vec<(Role, String, egui::Pos2)> = update
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                let text = node.label().or_else(|| node.value())?;
                let b = node.bounds()?;
                let centre = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                Some((node.role(), text.to_string(), centre))
            })
            .collect();
        drawn.sort_by(|a, b| a.2.y.total_cmp(&b.2.y));
        drawn
    }

    /// `Ctrl+P` as the editor dispatches it: through the bindings to the command they name.
    fn ctrl_p(&mut self, state: &mut AppState) {
        let action = crate::ui::shortcuts::action_for_press(
            &Key::Character("p".into()),
            PhysicalKey::Code(KeyCode::KeyP),
            crate::ui::shortcuts::PRIMARY_STATE,
            false,
            &state.prefs,
        )
        .expect("Ctrl+P is bound to a command");
        crate::ui::commands::run_action(state, &self.ctx, action);
        self.frame(state, Vec::new(), NONE);
    }

    fn type_text(&mut self, state: &mut AppState, text: &str) {
        self.frame(state, vec![egui::Event::Text(text.into())], NONE);
    }

    fn press(&mut self, state: &mut AppState, key: egui::Key, modifiers: egui::Modifiers) {
        let event = egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        };
        self.frame(state, vec![event], modifiers);
    }

    fn click(&mut self, state: &mut AppState, at: egui::Pos2) {
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: NONE,
        };
        self.frame(
            state,
            vec![egui::Event::PointerMoved(at), press(true)],
            NONE,
        );
        self.frame(state, vec![press(false)], NONE);
    }

    /// Draw until `done` holds of what a frame drew and of the state.
    fn until(
        &mut self,
        state: &mut AppState,
        what: &str,
        mut done: impl FnMut(&AppState, &[(Role, String, egui::Pos2)]) -> bool,
    ) -> Vec<(Role, String, egui::Pos2)> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let drawn = self.frame(state, Vec::new(), NONE);
            if done(state, &drawn) {
                return drawn;
            }
            let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
            let files: Vec<&str> = drawn
                .iter()
                .filter(|(_, _, at)| at.x <= FILES_WIDTH)
                .map(|(_, text, _)| text.as_str())
                .collect();
            assert!(
                Instant::now() < deadline,
                "{what}; Go to File drew {:#?}; Files drew {files:?}; the app said {said:?}",
                results(&drawn)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Draw until Go to File's first result starts with `leaf`.
    fn until_first(&mut self, state: &mut AppState, leaf: &str) -> Vec<(Role, String, egui::Pos2)> {
        let what = format!("{leaf} never came first");
        self.until(state, &what, |_, drawn| {
            results(drawn)
                .first()
                .is_some_and(|row| row.starts_with(leaf))
        })
    }
}

/// Go to File's results, top to bottom: the buttons of Files between its scopes and the line that says what Enter does, which stands under them only while there are some.
fn results(drawn: &[(Role, String, egui::Pos2)]) -> Vec<String> {
    let scopes = (drawn.iter())
        .find(|(_, text, _)| SCOPES.contains(&text.as_str()))
        .map(|(_, _, at)| at.y);
    let foot = (drawn.iter())
        .find(|(_, text, _)| text.starts_with("Enter opens"))
        .map(|(_, _, at)| at.y);
    let (Some(scopes), Some(foot)) = (scopes, foot) else {
        return Vec::new();
    };
    drawn
        .iter()
        .filter(|(role, label, at)| {
            *role == Role::Button
                && at.y > scopes
                && at.y < foot
                && !SCOPES.contains(&label.as_str())
        })
        .map(|(_, label, _)| label.clone())
        .collect()
}

/// Where the Go to File control labelled `label` is.
fn control(drawn: &[(Role, String, egui::Pos2)], label: &str) -> Option<egui::Pos2> {
    drawn
        .iter()
        .find(|(_, text, _)| text == label)
        .map(|(_, _, at)| *at)
}

/// A Legacy fragment the app opens.
fn fragment() -> Vec<u8> {
    or_io::Asset::Fragment(or_io::gta5::GtaFragType::default())
        .to_bytes()
        .expect("a fragment saves")
}

/// `x64e.rpf`, holding `vehicles.rpf`, which holds the adder and names like it,
/// and `props.rpf`, which holds a bench.
///
/// ⚠ The last two outranked `adder.yft` for `addr` in a retail install while
/// letters at word starts, and an extension's first letter, scored above a run.
fn x64e() -> Vec<u8> {
    let vehicles = archive(&[
        ("adder.yft", &fragment()),
        ("adder.ytd", b"never opened"),
        ("adder_hi.yft", b"never opened"),
        ("adder+hi.ytd", b"never opened"),
        ("ladder_01.ydr", b"never opened"),
        ("paddler.ydd", b"never opened"),
        ("mph3_add.rel", b"never opened"),
        (
            "anim@veh@train@front@ds@driveby_grenade.ycd",
            b"never opened",
        ),
    ]);
    let props = archive(&[("prop_bench_01a.ydr", b"never opened")]);
    archive(&[
        ("levels/gta5/vehicles.rpf", &vehicles),
        ("levels/gta5/props.rpf", &props),
    ])
}

fn install() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("an install folder");
    std::fs::write(dir.path().join("x64e.rpf"), x64e()).expect("x64e.rpf");
    dir
}

/// `root` as the remembered install, opened as a launch opens it, and its tree.
fn cold(root: &Path) -> (AppState, DocumentId) {
    let mut prefs = Prefs::default();
    prefs
        .game_paths
        .set(Game::GtaVLegacy, Some(root.to_path_buf()));
    prefs.active_game = Some(Game::GtaVLegacy);
    let state = AppState::from_prefs(prefs);
    let tree = state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::GameRoot)
        .expect("the install's tree")
        .id;
    assert!(
        row_at(&state, tree, "x64e.rpf/levels").is_none(),
        "the tree opened x64e.rpf by itself"
    );
    (state, tree)
}

/// The row `path` names in `doc`, if the tree has loaded it.
fn row_at(state: &AppState, doc: DocumentId, path: &str) -> Option<RpfNodeId> {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => match resolve_path(&rpf.session, path) {
            Resolved::Found(row) => Some(row),
            _ => None,
        },
        _ => None,
    }
}

/// The path inside `doc` of each archive entry open as a document of its own.
fn opened_entries(state: &AppState, doc: DocumentId) -> Vec<String> {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        return Vec::new();
    };
    state
        .documents
        .iter()
        .filter_map(|d| d.rpf_entry_binding)
        .filter(|binding| binding.parent_doc_id == doc)
        .filter_map(|binding| rpf.session.path_of(binding.entry_id))
        .collect()
}

#[test]
fn ctrl_p_then_adder_then_enter_opens_adder_yft_from_a_cold_start() {
    let root = install();
    let (mut state, tree) = cold(root.path());
    let mut screen = Screen::new();

    screen.ctrl_p(&mut state);
    screen.type_text(&mut state, "adder");
    screen.until_first(&mut state, "adder.yft");
    screen.press(&mut state, egui::Key::Enter, NONE);

    screen.until(&mut state, "adder.yft never opened", |state, _| {
        opened_entries(state, tree).contains(&ADDER.to_string())
    });

    let adder = state
        .documents
        .iter()
        .find(|d| d.rpf_entry_binding.is_some_and(|b| b.parent_doc_id == tree))
        .map(|d| d.id);
    screen.ctrl_p(&mut state);
    let recent = results(&screen.frame(&mut state, Vec::new(), NONE));
    assert!(
        recent
            .first()
            .is_some_and(|row| row.starts_with("adder.yft")),
        "the file opened last is not first when Go to File opens again: {recent:#?}"
    );

    state.set_active_document(tree);
    screen.press(&mut state, egui::Key::Enter, NONE);
    screen.until(
        &mut state,
        "opening adder.yft again never focused its tab",
        |state, _| state.active_document.is_some() && state.active_document == adder,
    );
    let tabs = opened_entries(&state, tree);
    assert_eq!(
        tabs.iter().filter(|path| *path == ADDER).count(),
        1,
        "opening an open file again made a second tab: {tabs:?}"
    );
}

#[test]
fn a_hash_typed_in_hex_or_in_decimal_finds_the_file_it_names() {
    let root = install();
    let (mut state, _tree) = cold(root.path());
    let mut screen = Screen::new();
    let hash = or_io::hash::string_hash(b"prop_bench_01a");

    for typed in [format!("0x{hash:08X}"), hash.to_string()] {
        screen.ctrl_p(&mut state);
        screen.type_text(&mut state, &typed);
        screen.until_first(&mut state, "prop_bench_01a.ydr");
        screen.press(&mut state, egui::Key::Escape, NONE);
    }
}

#[test]
fn shift_enter_reveals_the_result_in_files_instead_of_opening_it() {
    let root = install();
    let (mut state, tree) = cold(root.path());
    let mut screen = Screen::new();

    screen.ctrl_p(&mut state);
    screen.type_text(&mut state, "adder");
    screen.until_first(&mut state, "adder.yft");
    screen.press(&mut state, egui::Key::Enter, egui::Modifiers::SHIFT);

    screen.until(
        &mut state,
        "the reveal never selected adder.yft",
        |state, _| {
            state.selection.selected(tree).is_some()
                && state.selection.selected(tree) == row_at(state, tree, ADDER)
        },
    );
    let row = state.selection.selected(tree).expect("selected above");
    screen.until(
        &mut state,
        "Files never drew the revealed row",
        |state, _| {
            crate::ui::panels::hierarchy::ui_state_ref(state)
                .and_then(|files| files.visible_rows.get(&tree))
                .is_some_and(|rows| rows.contains(&row))
        },
    );
    assert!(
        opened_entries(&state, tree).is_empty(),
        "the reveal opened the file"
    );
}

#[test]
fn a_fuzzy_query_ranks_the_closest_name_first() {
    let root = install();
    let (mut state, _tree) = cold(root.path());
    let mut screen = Screen::new();

    screen.ctrl_p(&mut state);
    screen.type_text(&mut state, "addr");
    let drawn = screen.until_first(&mut state, "adder.yft");
    let shown = results(&drawn);
    let place = |leaf: &str| shown.iter().position(|row| row.starts_with(leaf));
    assert!(
        place("adder_hi.yft") < place("ladder_01.ydr") && place("ladder_01.ydr").is_some(),
        "a name matched in order from its start does not come before one matched inside a word: {shown:#?}"
    );
}

#[test]
fn the_scope_keeps_a_base_game_file_out_of_a_mods_overlay_search() {
    let root = install();
    let overlay = root.path().join("mods");
    std::fs::create_dir_all(&overlay).expect("the mods overlay");
    std::fs::write(overlay.join("x64e.rpf"), x64e()).expect("the overlay's x64e.rpf");
    let (mut state, _tree) = cold(root.path());
    let mut screen = Screen::new();
    let base = "adder.yft x64e.rpf/levels/gta5/vehicles.rpf";
    let modded = "adder.yft mods/x64e.rpf/levels/gta5/vehicles.rpf";

    screen.ctrl_p(&mut state);
    screen.type_text(&mut state, "adder.yft");
    let drawn = screen.until(
        &mut state,
        "both copies of the adder never showed",
        |_, drawn| {
            let shown = results(drawn);
            shown.iter().any(|r| r == base) && shown.iter().any(|r| r == modded)
        },
    );

    for (scope, kept, left_out) in [("Mods overlay", modded, base), ("Base game", base, modded)] {
        let at = control(&drawn, scope).unwrap_or_else(|| panic!("no {scope} control: {drawn:#?}"));
        screen.click(&mut state, at);
        let shown = results(&screen.frame(&mut state, Vec::new(), NONE));
        assert!(
            shown.iter().any(|r| r == kept) && !shown.iter().any(|r| r == left_out),
            "{scope} shows {shown:#?}"
        );
    }
}

#[test]
fn the_searches_of_every_archive_in_two_open_archives_both_arrive() {
    let first = tempfile::tempdir().expect("a scratch folder");
    let second = tempfile::tempdir().expect("another");
    let mut state = AppState::from_prefs(Prefs::default());
    let inner = archive(&[("first_deep.txt", b"deep")]);
    open(
        &mut state,
        first.path(),
        &archive(&[("first.txt", b"one"), ("inner.rpf", &inner)]),
        Opened::AsADocument,
    );
    open(
        &mut state,
        second.path(),
        &archive(&[("second.txt", b"two")]),
        Opened::AsADocument,
    );
    let files = crate::ui::panels::hierarchy::ui_state(&mut state);
    files.filter = "**/*.txt".into();
    files.deep = true;
    // What Files' Expand button asks: both archives' trees open.
    files.pulse_expand_all = true;
    let mut screen = Screen::new();

    screen.until(
        &mut state,
        "the two searches never both arrived",
        |_, drawn| {
            let shown = |label: &str| drawn.iter().any(|(_, text, _)| text == label);
            shown("doc:/inner.rpf/first_deep.txt") && shown("doc:/second.txt")
        },
    );
}

#[test]
fn a_file_saved_into_an_archive_is_found_once_the_save_lands() {
    let root = install();
    let (mut state, tree) = cold(root.path());
    let mut screen = Screen::new();
    screen.ctrl_p(&mut state);
    screen.type_text(&mut state, "adder");
    screen.until_first(&mut state, "adder.yft");
    screen.press(&mut state, egui::Key::Escape, NONE);

    let loose = tempfile::tempdir().expect("a scratch folder");
    let added = loose.path().join("freshly_added.yft");
    std::fs::write(&added, b"added").expect("a file to add");
    let x64e = node_at(&mut state, tree, "x64e.rpf");
    state.prefs.edit_mode = EditMode::Everything;
    let asked = state.rpf_import_file_to_folder(tree, x64e, added);
    assert!(
        asked.is_err() && state.pending_retail_edit.is_some(),
        "the edit gate did not ask before adding to the install: {asked:?}"
    );
    state.apply_pending_retail_edit(RetailEditDecision::YesSession);
    save(&mut state, tree);

    screen.until(
        &mut state,
        "the index never took the saved file in",
        |state, _| {
            state.file_index.root_of(tree).is_some_and(|root| {
                root.listing
                    .files
                    .iter()
                    .any(|file| &**file == "x64e.rpf/freshly_added.yft")
            })
        },
    );
    screen.ctrl_p(&mut state);
    screen.type_text(&mut state, "freshly");
    screen.until_first(&mut state, "freshly_added.yft");
}
