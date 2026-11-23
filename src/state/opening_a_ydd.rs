//! A Legacy `.ydd` opens in the app from the tree and from disk: a shipped one,
//! and one whose geometries share one matrix palette, as other tools write.
//!
//! ⚠ The shipped case is install-gated: the archives a user browses are the
//! only honest input for the tree's expansion and nested archives.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::decl::Aligned;
use or_io::desc::{Image, Record, Value};
use or_io::Generation;
use or_test_support::{game_roots, skip_corpus, skip_install, Corpus, GAME_ROOT_ENV};

use super::document::{DocumentId, RpfNodeId};
use super::{AppState, DocPayload, DocumentKind, LoadingState, RpfOutputMode};

/// A ped component, a ped prop and a map dictionary, each in a nested archive.
const SHIPPED_YDD: &[&str] = &[
    "x64v.rpf/models/cdimages/streamedpeds_players.rpf/player_two/uppr_029_u.ydd",
    "x64v.rpf/models/cdimages/componentpeds_s_f_y.rpf/s_f_y_sweatshop_01.ydd",
    "x64e.rpf/models/cdimages/pedprops.rpf/a_f_y_yoga_01_p.ydd",
    "x64k.rpf/levels/gta5/_citye/port_01/port.rpf/po1_06_props_combo01_slod_children.ydd",
];

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn with_keys(root: &Path) -> AppState {
    let mut state = AppState::default();
    state.keys.start_extraction(root.join("GTA5.exe"));
    wait_until("the key extraction", || state.keys.poll().is_some());
    assert!(
        state.keys.has_keys(),
        "no keys: {:?}",
        state.keys.extraction.last_error
    );
    state
}

/// The node `path` names under the game root, each archive on the way expanded as the tree does.
pub(super) fn node_at(state: &mut AppState, game: DocumentId, path: &str) -> RpfNodeId {
    let mut node = match &state.document(game).expect("the game root").payload {
        DocPayload::Rpf(rpf) => rpf.session.root_id(),
        _ => unreachable!("the game root is an archive document"),
    };
    for name in path.split('/') {
        state.request_expand(game, node);
        wait_until("the tree's expansion", || {
            state.poll_expansions();
            !state.expander.is_in_flight(game, node)
        });
        let DocPayload::Rpf(rpf) = &state.document(game).expect("the game root").payload else {
            unreachable!("the game root is an archive document");
        };
        node = rpf
            .session
            .children_of(node)
            .into_iter()
            .find(|id| {
                rpf.session
                    .node(*id)
                    .is_some_and(|n| n.name.eq_ignore_ascii_case(name))
            })
            .unwrap_or_else(|| panic!("{path}: the tree has no {name}"));
    }
    node
}

/// The document a finished load left active, or the error it reported.
pub(super) fn finish_load(state: &mut AppState) -> Result<DocumentId, String> {
    wait_until("the load", || {
        state.poll_load_result();
        state.load_receiver.is_none()
    });
    match &state.loading {
        LoadingState::Error(message) => Err(message.clone()),
        _ => Ok(state.active_document.expect("the load left a document")),
    }
}

/// `path` opened through the tree's open action, or the error the app reported.
fn open_from_tree(state: &mut AppState, game: DocumentId, path: &str) -> Result<(), String> {
    let node = node_at(state, game, path);
    state.clear_error();
    state.open_rpf_entry(game, node);
    finish_load(state).map(|doc| opened_as_a_drawable_dictionary(state, doc, path))
}

/// `path` opened as a file from disk, or the error the app reported.
fn open_from_disk(state: &mut AppState, path: PathBuf) -> Result<(), String> {
    state.clear_error();
    state.load_file(path.clone());
    finish_load(state)
        .map(|doc| opened_as_a_drawable_dictionary(state, doc, &path.display().to_string()))
}

fn opened_as_a_drawable_dictionary(state: &AppState, doc: DocumentId, what: &str) {
    let doc = state.document(doc).expect("the opened document");
    assert_eq!(doc.kind, DocumentKind::Ydd, "{what}");
    let DocPayload::Resource(or_io::Asset::DrawableDict(dict)) = &doc.payload else {
        panic!("{what}: opened as {:?}", doc.payload.kind());
    };
    assert!(!dict.entries.is_empty(), "{what}: no drawable");
}

#[test]
fn a_shipped_legacy_ydd_opens_from_the_tree_and_from_disk() {
    const WHAT: &str = "a shipped Legacy .ydd opens from the install's tree and from disk";
    let Some(root) = game_roots().into_iter().find(|root| {
        root.join("GTA5.exe").is_file()
            && SHIPPED_YDD
                .iter()
                .all(|p| root.join(p.split('/').next().unwrap()).is_file())
    }) else {
        skip_install(
            WHAT,
            format_args!("no Legacy install root; set {GAME_ROOT_ENV}"),
        );
        return;
    };

    let mut state = with_keys(&root);
    state.open_game_root(root.clone());
    let game = state.documents[0].id;
    let mut failures: Vec<String> = SHIPPED_YDD
        .iter()
        .filter_map(|path| open_from_tree(&mut state, game, path).err())
        .collect();

    let node = node_at(&mut state, game, SHIPPED_YDD[0]);
    let DocPayload::Rpf(rpf) = &state.document(game).expect("the game root").payload else {
        unreachable!("the game root is an archive document");
    };
    let bytes = rpf.session.bytes_for(node).expect("the entry's bytes");
    let dir = tempfile::tempdir().expect("a scratch directory");
    let on_disk = dir.path().join("uppr_029_u.ydd");
    std::fs::write(&on_disk, &*bytes).expect("written");
    failures.extend(open_from_disk(&mut state, on_disk).err());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Each record of `v`, outermost first.
fn each_record(v: &mut Value, f: &mut dyn FnMut(&mut Record)) {
    match v {
        Value::Struct(record) => {
            f(record);
            for (_, child) in record.children_mut() {
                each_record(child, f);
            }
        }
        Value::Ptr {
            target: Some(t), ..
        } => each_record(t, f),
        Value::Array { elements, .. } => elements.iter_mut().for_each(|e| each_record(e, f)),
        Value::Blob { parts, .. } => parts.iter_mut().for_each(|(_, p)| each_record(p, f)),
        _ => {}
    }
}

/// `file` saved with its second skinned geometry's matrix palette aimed at the first's; `None` with fewer than two,
/// or when its geometries already share one, as the mod files staged in the corpus do.
fn sharing_a_palette(file: &[u8]) -> Option<Vec<u8>> {
    let opened = Rsc7::open(file).expect("an RSC7 resource");
    let bytes = Aligned::from_bytes(&opened.data);
    let format = or_io::gta5::formats::format("ydd").expect("the .ydd format");
    let image = Image::new(bytes.bytes(), opened.virtual_size as usize);
    let mut tree = format
        .read(Generation::Gen8, image)
        .expect("the tree reads");
    let mut first = None;
    let mut shared = false;
    let mut already_shared = false;
    each_record(&mut tree, &mut |record| {
        if matches!(record.child("matrix_palette"), Some(Value::Alias(_))) {
            already_shared = true;
        }
        let Some(Value::Ptr {
            source,
            target: Some(_),
        }) = record.child("matrix_palette")
        else {
            return;
        };
        let source = *source;
        match first.clone() {
            None => first = Some((source, record.scalar("matrix_count").expect("a count"))),
            Some((owner, count)) if !shared => {
                *record.child_mut("matrix_palette").expect("a palette") = Value::Alias(owner);
                record
                    .set_structural("matrix_count", count)
                    .expect("the count is written");
                shared = true;
            }
            Some(_) => {}
        }
    });
    (shared && !already_shared).then(|| {
        format
            .save(Generation::Gen8, &tree)
            .expect("the tree saves")
    })
}

#[test]
fn a_ydd_whose_geometries_share_a_matrix_palette_opens_from_the_tree_and_from_disk() {
    const WHAT: &str = "a .ydd whose geometries share one matrix palette opens in the app";
    let Some(shared) = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .find()
        .iter()
        .find_map(|path| sharing_a_palette(&std::fs::read(path).expect("a corpus file")))
    else {
        skip_corpus(WHAT, "no Legacy .ydd with two skinned geometries");
        return;
    };
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::default();
    let on_disk = dir.path().join("shared.ydd");
    std::fs::write(&on_disk, &shared).expect("written");
    let mut failures: Vec<String> = open_from_disk(&mut state, on_disk)
        .err()
        .into_iter()
        .collect();

    let root = dir.path().join("install");
    std::fs::create_dir_all(&root).expect("the install root");
    let archive = |name: &str, bytes: Vec<u8>| {
        super::document_io::serialize_archive_records(
            vec![(name.to_string(), bytes, false)],
            RpfOutputMode::Open,
            state.keys.provider(),
        )
        .expect("archive bytes")
    };
    let inner = archive("shared.ydd", shared);
    std::fs::write(
        root.join("mods.rpf"),
        archive("x64/models/cdimages/peds.rpf", inner),
    )
    .expect("the archive is written");
    state.open_game_root(root);
    let game = state.documents[0].id;
    failures.extend(
        open_from_tree(
            &mut state,
            game,
            "mods.rpf/x64/models/cdimages/peds.rpf/shared.ydd",
        )
        .err(),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
