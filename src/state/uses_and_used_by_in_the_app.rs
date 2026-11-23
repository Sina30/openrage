//! A model's row offers Uses, which lists what `openrage-cli depends` says its
//! file uses and opens each, and Placed by, which lists the maps placing it;
//! the install is read once, as a job, the first time either asks.
//!
//! ⚠ The install is built here, its `.ytyp` and `.ymap` written as PSO from
//! the schema [`library`] declares, so a clean checkout with no schema
//! snapshot reads them as a retail install's are read.

use std::path::Path;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use or_io::meta::binary::layout::{ContainerLayout, MetaLayout};
use or_io::meta::binary::synth::pso_layout_for_instance;
use or_io::meta::instance::HashedString;
use or_io::meta::{
    ArrayKind, BinaryFormat, Field, FieldHint, FieldType, MetaInstance, PointerPolicy,
    SchemaLibrary, StringKind, StructDef, Value,
};

use super::document::{DocumentId, RpfNodeId};
use super::document_io::empty_file_bytes_for_kind;
use super::navigation::{resolve_path, Arrival, Resolved};
use super::nested_archives_survive_a_save::archive;
use super::the_files_view_in_the_centre::leaf_of;
use super::{AppState, DocPayload, DocumentKind, Game, Prefs};
use crate::ui::panels::registry::ids;

pub(super) const PROPS: &str = "x64e.rpf/levels/gta5/props.rpf";
const MAP: &str = "x64e.rpf/levels/gta5/hw_props.ymap";
pub(super) const DEADLINE: Duration = Duration::from_secs(120);
/// What the jobs list calls the install's read.
const JOB: &str = "Uses and Placed by";

fn field(name: &str, ty: FieldType) -> Field {
    Field {
        name: name.to_string(),
        ty,
        init: None,
        min: None,
        max: None,
        step: None,
        ui_key: false,
        hide_widgets: false,
        description: None,
        hint: FieldHint::None,
    }
}

fn structure(name: &str, fields: Vec<Field>) -> StructDef {
    StructDef {
        name: name.to_string(),
        base: None,
        abstract_: false,
        simple: true,
        fields,
        source_path: String::new(),
    }
}

/// A list of `of`, each behind a pointer, as `archetypes` and `entities` are.
fn pointers(of: &str) -> FieldType {
    FieldType::Array {
        elem: Box::new(FieldType::Pointer {
            base: of.into(),
            policy: PointerPolicy::Owner,
        }),
        kind: ArrayKind::AtArray,
    }
}

/// `CMapTypes` and `CMapData` as far as a dependency index reads them.
pub(super) fn library() -> SchemaLibrary {
    let hash = || FieldType::String(StringKind::AtHashString);
    let mut lib = SchemaLibrary::new();
    lib.insert_struct(structure(
        "CBaseArchetypeDef",
        vec![
            field("name", hash()),
            field("assetName", hash()),
            field("textureDictionary", hash()),
        ],
    ));
    lib.insert_struct(structure(
        "CMapTypes",
        vec![
            field("name", hash()),
            field("archetypes", pointers("CBaseArchetypeDef")),
        ],
    ));
    lib.insert_struct(structure(
        "CEntityDef",
        vec![
            field("archetypeName", hash()),
            field("position", FieldType::Vector3),
        ],
    ));
    lib.insert_struct(structure(
        "CMapData",
        vec![
            field("name", hash()),
            field("entities", pointers("CEntityDef")),
            field("streamingExtentsMin", FieldType::Vector3),
            field("streamingExtentsMax", FieldType::Vector3),
        ],
    ));
    lib
}

/// `root` as PSO, which carries its own layout, so it reads with no schema.
pub(super) fn pso(root: &MetaInstance) -> Vec<u8> {
    let layout = MetaLayout {
        format: BinaryFormat::Pso,
        container: ContainerLayout::Pso(
            pso_layout_for_instance(&library(), root).expect("a layout"),
        ),
    };
    or_io::meta::write_meta_binary(BinaryFormat::Pso, Some(&layout), root).expect("PSO")
}

pub(super) fn hashed(name: &str) -> Value {
    Value::Hash(HashedString::new(name))
}

/// `v_props.ytyp`: `prop_bench`, textured by `props_txd`, which the install
/// holds, and `prop_lamp`, by `lamp_txd`, which it does not.
pub(super) fn types() -> Vec<u8> {
    let archetype = |name: &str, txd: &str| {
        let mut archetype = MetaInstance::new("CBaseArchetypeDef");
        archetype.set("name", hashed(name));
        archetype.set("assetName", hashed(name));
        archetype.set("textureDictionary", hashed(txd));
        Value::Pointer(Some(Box::new(archetype)))
    };
    let mut root = MetaInstance::new("CMapTypes");
    root.set("name", hashed("v_props"));
    root.set(
        "archetypes",
        Value::Array(vec![
            archetype("prop_bench", "props_txd"),
            archetype("prop_lamp", "lamp_txd"),
        ]),
    );
    pso(&root)
}

/// `hw_props.ymap`, placing `prop_bench` twice.
pub(super) fn map() -> Vec<u8> {
    placing("hw_props")
}

/// The map `name`, placing `prop_bench` twice: its stem and its `name` agree, as a placement is found by the one.
pub(super) fn placing(name: &str) -> Vec<u8> {
    let entity = |x: f32| {
        let mut entity = MetaInstance::new("CEntityDef");
        entity.set("archetypeName", hashed("prop_bench"));
        entity.set("position", Value::Vec3([x, 0.0, 0.0]));
        Value::Pointer(Some(Box::new(entity)))
    };
    let mut root = MetaInstance::new("CMapData");
    root.set("name", hashed(name));
    root.set("entities", Value::Array(vec![entity(1.0), entity(2.0)]));
    root.set("streamingExtentsMin", Value::Vec3([0.0; 3]));
    root.set("streamingExtentsMax", Value::Vec3([0.0; 3]));
    pso(&root)
}

/// An install: `common.rpf`, which a mount needs, and `x64e.rpf`, holding the
/// map and `props.rpf`, which holds the two drawables, a texture dictionary
/// and the type file declaring them.
pub(super) fn install() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("an install folder");
    let ydr = empty_file_bytes_for_kind(DocumentKind::Ydr).expect("a blank drawable");
    let ytd = empty_file_bytes_for_kind(DocumentKind::Ytd).expect("a blank texture dictionary");
    let props = archive(&[
        ("prop_bench.ydr", &ydr),
        ("prop_lamp.ydr", &ydr),
        ("props_txd.ytd", &ytd),
        ("v_props.ytyp", &types()),
    ]);
    let x64e = archive(&[
        ("levels/gta5/props.rpf", &props),
        ("levels/gta5/hw_props.ymap", &map()),
    ]);
    let common = archive(&[("data/readme.txt", b"common")]);
    std::fs::write(dir.path().join("common.rpf"), common).expect("common.rpf");
    std::fs::write(dir.path().join("x64e.rpf"), x64e).expect("x64e.rpf");
    dir
}

/// `root` as the remembered install, opened as a launch opens it, and its tree.
pub(super) fn cold(root: &Path) -> (AppState, DocumentId) {
    let mut prefs = Prefs::default();
    (prefs.game_paths).set(Game::GtaVLegacy, Some(root.to_path_buf()));
    prefs.active_game = Some(Game::GtaVLegacy);
    let state = AppState::from_prefs(prefs);
    let tree = (state.documents.iter())
        .find(|d| d.kind == DocumentKind::GameRoot)
        .expect("the install's tree")
        .id;
    (state, tree)
}

/// The leaves of the files `openrage-cli depends export` copies for `subject`.
pub(super) fn clis_set(root: &Path, subject: &str) -> Vec<String> {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let out = scratch.path().join("set");
    // ⚠ A key file that is not there, so the command reads no user's keys cache.
    let keys = scratch.path().join("no-keys.bin");
    let words = [
        "depends".to_string(),
        "export".into(),
        "--game".into(),
        root.display().to_string(),
        "--keys".into(),
        keys.display().to_string(),
        "--out".into(),
        out.display().to_string(),
        subject.into(),
    ];
    let _ = or_cli::run(&words);
    let manifest = std::fs::read_to_string(out.join(or_depends::MANIFEST))
        .unwrap_or_else(|e| panic!("the command exported {subject}: {e}"));
    let mut leaves: Vec<String> = (manifest.lines())
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split('\t').next())
        .map(String::from)
        .collect();
    leaves.sort();
    leaves.dedup();
    leaves
}

/// A widget one frame drew.
#[derive(Debug, Clone)]
pub(super) struct Drawn {
    pub(super) role: Role,
    pub(super) label: String,
    pub(super) disabled: bool,
    pub(super) at: egui::Pos2,
}

/// Whether a widget's text is `label`, behind the glyphs a row's icon puts first.
fn named(text: &str, label: &str) -> bool {
    text.trim_start_matches(|c: char| ('\u{e000}'..='\u{f8ff}').contains(&c) || c == ' ') == label
}

/// The dock drawn through real frames, ticking what the editor's redraw ticks.
pub(super) struct Screen {
    ctx: egui::Context,
    time: std::cell::Cell<f64>,
}

impl Screen {
    pub(super) fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self {
            ctx,
            time: Default::default(),
        }
    }

    pub(super) fn frame(&self, state: &mut AppState, events: Vec<egui::Event>) -> Vec<Drawn> {
        state.poll_load_result();
        state.poll_expansions();
        state.tick_navigation();
        state.poll_jobs();
        self.time.set(self.time.get() + 1.0 / 30.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            time: Some(self.time.get()),
            events,
            ..Default::default()
        };
        let output = self
            .ctx
            .run_ui(input, |ui| crate::ui::dock::draw(ui, state, None));
        let Some(update) = output.platform_output.accesskit_update else {
            return Vec::new();
        };
        (update.nodes.iter())
            .filter_map(|(_, node)| {
                let b = node.bounds()?;
                Some(Drawn {
                    role: node.role(),
                    label: node.label().or_else(|| node.value())?.to_string(),
                    disabled: node.is_disabled(),
                    at: egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32),
                })
            })
            .collect()
    }

    /// What the frames of a click and the one after drew, each frame's widgets after the last's.
    pub(super) fn click(
        &self,
        state: &mut AppState,
        at: egui::Pos2,
        button: egui::PointerButton,
    ) -> Vec<Drawn> {
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut drawn = self.frame(state, vec![egui::Event::PointerMoved(at), press(true)]);
        drawn.extend(self.frame(state, vec![press(false)]));
        drawn.extend(self.frame(state, Vec::new()));
        drawn
    }

    /// Draw until `done` holds of the state and of what a frame drew.
    pub(super) fn until(
        &self,
        state: &mut AppState,
        what: &str,
        done: impl Fn(&AppState, &[Drawn]) -> bool,
    ) -> Vec<Drawn> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let drawn = self.frame(state, Vec::new());
            if done(state, &drawn) {
                return drawn;
            }
            let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
            assert!(
                Instant::now() < deadline,
                "{what}; the frame drew {:?}; the app said {said:?}",
                drawn.iter().map(|d| &d.label).collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Reveal `path` of `doc` in Files, open its row's menu, then its submenu
    /// `submenu`, and return what the frames that opened it drew.
    pub(super) fn submenu(
        &self,
        state: &mut AppState,
        doc: DocumentId,
        source: &Path,
        path: &str,
        submenu: &str,
    ) -> Vec<Drawn> {
        // ⚠ A menu still open takes the next right click to close itself, and
        // Escape closes an open submenu before its menu.
        for _ in 0..2 {
            self.frame(state, vec![key(egui::Key::Escape)]);
        }
        state.walk_to(source, vec![path.to_string()], Arrival::Reveal);
        let leaf = path.rsplit('/').next().expect("a file name");
        let drawn = self.until(
            state,
            &format!("Files never drew {path}"),
            |state, drawn| {
                let files = leaf_of(state, ids::FILES);
                row_at(state, doc, path)
                    .is_some_and(|row| state.selection.selected(doc) == Some(row))
                    && drawn
                        .iter()
                        .any(|d| named(&d.label, leaf) && files.contains(d.at))
            },
        );
        // ⚠ The lowest: a breadcrumb above the tree names the row too.
        let files = leaf_of(state, ids::FILES);
        let row = (drawn.iter())
            .filter(|d| named(&d.label, leaf) && files.contains(d.at))
            .max_by(|a, b| a.at.y.total_cmp(&b.at.y))
            .map(|d| d.at)
            .expect("drawn above");
        let menu = self.click(state, row, egui::PointerButton::Secondary);
        let item = (menu.iter())
            .find(|d| d.role == Role::Button && d.label.starts_with(submenu))
            .unwrap_or_else(|| {
                panic!(
                    "{leaf}'s menu offers no {submenu}: {:?}",
                    menu.iter().map(|d| &d.label).collect::<Vec<_>>()
                )
            })
            .at;
        self.click(state, item, egui::PointerButton::Primary)
    }
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
pub(super) fn opened_entries(state: &AppState, doc: DocumentId) -> Vec<String> {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        return Vec::new();
    };
    (state.documents.iter())
        .filter_map(|d| d.rpf_entry_binding)
        .filter(|binding| binding.parent_doc_id == doc)
        .filter_map(|binding| rpf.session.path_of(binding.entry_id))
        .collect()
}

/// Each file a submenu lists: the item just left of each Reveal in tree.
///
/// ⚠ A submenu's rows line up with its menu's, so the menu's own items share
/// their rows: only the nearest item to the left is beside the Reveal.
pub(super) fn listed(drawn: &[Drawn]) -> Vec<&Drawn> {
    (drawn.iter())
        .filter(|reveal| reveal.label == "Reveal in tree")
        .filter_map(|reveal| {
            (drawn.iter())
                .filter(|d| d.role == Role::Button && d.label != "Reveal in tree")
                .filter(|d| (d.at.y - reveal.at.y).abs() < 2.0 && d.at.x < reveal.at.x)
                .max_by(|a, b| a.at.x.total_cmp(&b.at.x))
        })
        .collect()
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(super) fn reading(state: &AppState) -> bool {
    (state.running_jobs().iter()).any(|job| job.kind.label() == JOB)
}

pub(super) fn read(state: &AppState) -> bool {
    (state.jobs.ended().iter()).any(|job| job.kind.label() == JOB)
}

pub(super) const LOOKING: &str = "Looking for what it uses…";

#[test]
fn a_drawables_uses_lists_its_texture_dictionary_and_opens_it() {
    let root = install();
    let clis = clis_set(root.path(), "prop_bench.ydr");
    assert!(
        clis.contains(&"props_txd.ytd".to_string()),
        "the command's set has the dictionary: {clis:?}"
    );
    let (mut state, tree) = cold(root.path());
    let screen = Screen::new();

    let bench = format!("{PROPS}/prop_bench.ydr");
    let first = screen.submenu(&mut state, tree, root.path(), &bench, "Uses");
    assert!(
        first.iter().any(|d| d.label == LOOKING),
        "the menu says it is looking until the install is read: {:?}",
        first.iter().map(|d| &d.label).collect::<Vec<_>>()
    );
    let drawn = screen.until(
        &mut state,
        "Uses never listed the dictionary",
        |_, drawn| {
            listed(drawn)
                .iter()
                .any(|d| d.label.starts_with("props_txd.ytd"))
        },
    );
    assert!(
        read(&state) && !reading(&state),
        "the install was read as a job of the jobs list: {:?}",
        state.jobs.ended()
    );
    let mut shown: Vec<String> = (listed(&drawn).iter())
        .filter_map(|d| d.label.split(' ').next())
        .map(String::from)
        .collect();
    shown.sort();
    let others: Vec<String> = clis.into_iter().filter(|f| f != "prop_bench.ydr").collect();
    assert_eq!(
        shown, others,
        "Uses lists what the command exports with the drawable, the drawable aside"
    );

    let txd = (listed(&drawn).into_iter())
        .find(|d| d.label.starts_with("props_txd.ytd"))
        .map(|d| d.at)
        .expect("listed above");
    screen.click(&mut state, txd, egui::PointerButton::Primary);
    let opened = format!("{PROPS}/props_txd.ytd");
    screen.until(&mut state, "the dictionary never opened", |state, _| {
        opened_entries(state, tree).contains(&opened)
    });
}

#[test]
fn a_dependency_the_install_does_not_hold_shows_disabled_with_the_reason() {
    let root = install();
    let (mut state, tree) = cold(root.path());
    let screen = Screen::new();

    let bench = format!("{PROPS}/prop_bench.ydr");
    screen.submenu(&mut state, tree, root.path(), &bench, "Uses");
    screen.until(&mut state, "Uses never answered", |_, drawn| {
        !listed(drawn).is_empty()
    });

    let lamp = format!("{PROPS}/prop_lamp.ydr");
    let drawn = screen.submenu(&mut state, tree, root.path(), &lamp, "Uses");
    assert!(
        !drawn.iter().any(|d| d.label == LOOKING),
        "the install is read once, so a second menu answers at once"
    );
    let missing = format!("{:#010X}.ytd", or_io::string_hash(b"lamp_txd"));
    let item = (drawn.iter())
        .find(|d| d.role == Role::Button && d.label.starts_with(&missing))
        .unwrap_or_else(|| {
            panic!(
                "no {missing}: {:?}",
                drawn.iter().map(|d| &d.label).collect::<Vec<_>>()
            )
        });
    assert!(
        item.disabled && item.label.contains("not in this install"),
        "a dependency the install lacks is offered disabled with the reason: {item:?}"
    );
    assert!(
        (drawn.iter()).any(|d| d.label.starts_with("v_props.ytyp") && !d.disabled),
        "what the install holds is still offered"
    );
}

#[test]
fn a_drawable_in_a_loose_folder_says_why_uses_cannot_answer() {
    let folder = tempfile::tempdir().expect("a loose folder");
    let ydr = empty_file_bytes_for_kind(DocumentKind::Ydr).expect("a blank drawable");
    std::fs::write(folder.path().join("prop_bench.ydr"), ydr).expect("a drawable");
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(folder.path().to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let screen = Screen::new();

    let drawn = screen.submenu(&mut state, doc, folder.path(), "prop_bench.ydr", "Uses");
    assert!(
        (drawn.iter()).any(|d| d
            .label
            .starts_with("Only a game install says what a file uses")),
        "Uses says why it cannot answer: {:?}",
        drawn.iter().map(|d| &d.label).collect::<Vec<_>>()
    );
    assert!(!reading(&state), "nothing is read for a file in no install");
}

#[test]
fn placed_by_lists_the_map_placing_a_drawable_and_opens_it() {
    let root = install();
    let (mut state, tree) = cold(root.path());
    let screen = Screen::new();

    let bench = format!("{PROPS}/prop_bench.ydr");
    screen.submenu(&mut state, tree, root.path(), &bench, "Placed by");
    let drawn = screen.until(&mut state, "Placed by never listed the map", |_, drawn| {
        listed(drawn)
            .iter()
            .any(|d| d.label.starts_with("hw_props.ymap"))
    });
    let map = (listed(&drawn).into_iter())
        .find(|d| d.label.starts_with("hw_props.ymap"))
        .expect("listed above");
    assert!(
        map.label.contains("2 placements"),
        "the map places it twice: {map:?}"
    );
    screen.click(&mut state, map.at, egui::PointerButton::Primary);
    screen.until(&mut state, "the map never opened", |state, _| {
        opened_entries(state, tree).contains(&MAP.to_string())
    });
}
