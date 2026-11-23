use std::time::{Duration, Instant};

use or_io::gta6::map::{Aabb, ArchetypeClass, InstanceList, MapTypes};

use super::super::opening_an_rpf9_archive::{rpf9_head, scanned, settle, written, zlib};
use super::*;
use crate::state::document::MetaDocument;
use crate::state::rpf9_scan::body_path;
use crate::state::workbench::{Pick, PickKind};

const WORD: u64 = 0x0123_4567_89AB_CDEF;

fn entity(interior: bool, word: u64, name: u32, rotation: [f32; 4]) -> Entity {
    Entity {
        interior,
        id: 0x77,
        rotation,
        archetype_word: word,
        position: [10.0, -20.0, 5.0],
        archetype_name: name,
        flags: 0,
        scale_xy: 2.0,
        scale_z: 3.0,
        parent_index: -1,
        hash_e74c3c23: 0,
        lod_dist: -1.0,
        child_lod_dist: 0.0,
        num_children: 0,
        tint_value: 0,
        priority_level: 0,
        lod_level: 5,
    }
}

fn archetype(word: u64, name: u32) -> Archetype {
    Archetype {
        class: ArchetypeClass::Base,
        name,
        name_word: word,
        texture_dictionary: 0,
        drawable_dictionary: 0,
        drawable: word,
        fragment: 0,
        lod_dist: 250.0,
        special_attribute: 0,
        bb_min: [-1.0, -2.0, -3.0],
        bb_max: [4.0, 5.0, 6.0],
        bs_centre: [0.0; 3],
        bs_radius: 1.0,
        flags: 0,
        hd_texture_dist: 0.0,
        clip_dictionary: 0,
        physics_dictionary: 0,
        asset_type: 2,
        asset_name: name,
    }
}

/// A map of an entity turned a quarter about Z, an interior turned the same way, and one prop list.
fn map() -> MapBody {
    let quarter = [
        0.0,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ];
    MapBody::Data(MapData {
        name: 1,
        name_word: 2,
        parent: 0,
        hash_6c0f9c8c: 0,
        hash_9e8d32a0: 0,
        hash_ec16451d: 0,
        hash_36c898aa: 0,
        flags: 0,
        content_flags: 0,
        streaming_extents: Aabb {
            min: [-500.0; 3],
            max: [500.0; 3],
        },
        entities_extents: Aabb {
            min: [-50.0; 3],
            max: [50.0; 3],
        },
        entities: vec![
            entity(false, WORD, 0xAAAA_0001, quarter),
            entity(true, 0x55, 0xBBBB_0002, quarter),
        ],
        prop_lists: vec![InstanceList {
            archetype_name: 0xCCCC_0003,
            instances: 9,
            batch_aabb: Aabb {
                min: [100.0, 200.0, 0.0],
                max: [110.0, 220.0, 4.0],
            },
        }],
        grass_lists: Vec::new(),
    })
}

fn open(state: &mut AppState, path: PathBuf) -> crate::state::DocumentId {
    state.insert_document(path, DocPayload::Meta(MetaDocument::empty()), None, None)
}

#[test]
fn a_scanned_map_places_its_entities_as_gta_v_s_with_the_box_of_an_open_types_body_or_a_marker() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (map_path, types_path) = (dir.path().join("0x10.rbm"), dir.path().join("0x20.rbm"));
    keep_read(&map_path, map());
    keep_read(
        &types_path,
        MapBody::Types(MapTypes {
            name: 3,
            archetypes: vec![archetype(WORD, 0xAAAA_0001)],
        }),
    );
    let mut state = AppState::new();
    open(&mut state, types_path.clone());
    let doc = open(&mut state, map_path.clone());
    state.active_document = Some(doc);
    assert!(!state.workspaces_offered().contains(&Workspace::Map));
    state.refresh_scanned_map();
    assert!(
        state.workspaces_offered().contains(&Workspace::Map),
        "the Map screen is offered while a scanned map is the active document"
    );
    for on in [state.active_workspace, Workspace::Map] {
        state.active_workspace = on;
        assert!(
            state.workspaces_in_the_strip().contains(&Workspace::Map),
            "the strip draws a button for the Map screen on {on:?}"
        );
    }

    let shown = state.scanned_map().expect("the map is shown");
    let placed = &shown.scene.placed;
    assert_eq!(placed.len(), 3, "two entities and one list");
    let (entity, interior, list) = (&placed[0], &placed[1], &placed[2]);
    assert!(entity.declared);
    assert_eq!(
        (entity.obb.min, entity.obb.max),
        (Vec3::new(-1.0, -2.0, -3.0), Vec3::new(4.0, 5.0, 6.0))
    );
    assert_eq!(entity.lod_dist, 250.0, "-1 takes the archetype's distance");
    assert_eq!(entity.obb.scale, Vec3::new(2.0, 2.0, 3.0));
    // The entity's model is turned by the conjugate, the interior's by the quaternion as stored.
    let x = Vec3::X;
    assert!((entity.obb.rotation * x - Vec3::new(0.0, -1.0, 0.0)).length() < 1e-5);
    assert!((interior.obb.rotation * x - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5);
    assert!(!interior.declared, "no open body holds its archetype");
    assert_eq!(interior.obb.max, Vec3::splat(or_maps::scene::UNKNOWN_HALF));
    assert_eq!(shown.markers(), 1);
    assert_eq!(
        shown.archetypes[0].as_ref().map(|(s, _)| s),
        Some(&Source::Open(types_path.clone()))
    );
    assert_eq!(list.index, 2);
    assert_eq!(list.obb.position, Vec3::new(105.0, 210.0, 2.0));
    assert_eq!(list.obb.max, Vec3::new(5.0, 10.0, 2.0));
    assert_eq!(state.shown_scene().map(|s| s.placed.len()), Some(3));

    state.pick_shown(
        None,
        None,
        Some(Pick {
            source: map_path.display().to_string(),
            kind: PickKind::Entity(1),
        }),
    );
    assert!(state.selected_placed().is_some_and(|p| p.interior));
    assert!(
        crate::ui::commands::of(&state, crate::ui::commands::Action::RemoveEntity)
            .is_some_and(|c| c.enabled.is_err()),
        "a scanned map is read-only"
    );

    // Another document made active takes the map off the screen.
    state.active_document = state
        .documents
        .iter()
        .find(|d| d.path.as_deref() == Some(&types_path))
        .map(|d| d.id);
    state.refresh_scanned_map();
    assert!(state.scanned_map().is_none());
    assert!(!state.workspaces_offered().contains(&Workspace::Map));
    assert!(!state.workspaces_in_the_strip().contains(&Workspace::Map));
}

/// A headerless map types body of one archetype, as a virtual section; it declares no class, which
/// the map reader needs only to check the members it reads.
fn map_types_body(word: u64, name: u32) -> Vec<u8> {
    const VIRTUAL: u64 = 0x5000_0000;
    let (blocks, root, pointers, archetype) = (0x70usize, 0xA0usize, 0x100usize, 0x110usize);
    let mut v = vec![0u8; 0x200];
    let mut put = |at: usize, bytes: &[u8]| v[at..at + bytes.len()].copy_from_slice(bytes);
    put(0x08, &(VIRTUAL + 0x60).to_le_bytes());
    put(0x68, &[1, 0]);
    put(0x10, b"0DRP");
    put(0x14, &0x7Au16.to_le_bytes());
    put(0x20, &1u32.to_le_bytes());
    put(0x38, &(VIRTUAL + blocks as u64).to_le_bytes());
    put(0x4C, &3u16.to_le_bytes());
    for (i, (kind, size, at)) in [
        (map::C_MAP_TYPES, 0x50u32, root),
        (7, 8, pointers),
        (
            or_io::hash::literal_string_hash(b"CBaseArchetypeDef"),
            0xB8,
            archetype,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        put(blocks + 16 * i, &kind.to_le_bytes());
        put(blocks + 16 * i + 4, &size.to_le_bytes());
        put(blocks + 16 * i + 8, &(VIRTUAL + at as u64).to_le_bytes());
    }
    // `archetypes`: a pointer to block 2, one element.
    put(root + 0x18, &2u64.to_le_bytes());
    put(root + 0x20, &[1, 0, 1, 0]);
    put(pointers, &3u64.to_le_bytes());
    put(archetype + 0x08, &word.to_le_bytes());
    put(archetype + 0x20, &word.to_le_bytes());
    for (at, value) in [
        (0x40, -7.0f32),
        (0x44, -8.0),
        (0x48, -9.0),
        (0x4C, 7.0),
        (0x50, 8.0),
        (0x54, 9.0),
    ] {
        put(archetype + at, &value.to_le_bytes());
    }
    put(archetype + 0x78, &name.to_le_bytes());
    put(archetype + 0x84, &2u32.to_le_bytes());
    v
}

fn finished(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !state.map_search.is_empty() {
        state.poll_map_search();
        assert!(Instant::now() < deadline, "the search never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_archetype_a_map_names_is_found_among_the_bodies_of_its_scan_as_a_job() {
    let _serial = crate::ui::theme::serial();
    let mut bytes = rpf9_head(1, 0x1A0);
    bytes.resize(0x1000, 0);
    let at = bytes.len() as u64;
    bytes.extend_from_slice(&zlib(&map_types_body(WORD, 0xAAAA_0001)));
    let dir = tempfile::tempdir().expect("a scratch directory");
    let archive = written(dir.path(), &bytes);
    let mut state = AppState::new();
    let scan = scanned(&mut state, archive.clone());
    let map_path = body_path(&archive, 0x8000, View::Headerless);
    keep_read(&map_path, map());
    let doc = open(&mut state, map_path);
    state.active_document = Some(doc);
    state.refresh_scanned_map();
    assert_eq!(state.scanned_map().map(ScannedMap::markers), Some(2));

    state.find_gta6_archetypes().expect("a scan to look in");
    assert_eq!(state.map_search.len(), 1, "the search is a job");
    assert!(state
        .running_jobs()
        .iter()
        .any(|j| j.kind == JobKind::ArchetypeSearch));
    finished(&mut state);
    let shown = state.scanned_map().expect("the map is shown");
    assert_eq!(
        shown.markers(),
        1,
        "the one archetype the scan holds is found"
    );
    assert_eq!(
        shown.archetypes[0].as_ref().map(|(s, _)| s),
        Some(&Source::Scan(at))
    );
    assert_eq!(shown.scene.placed[0].obb.min, Vec3::new(-7.0, -8.0, -9.0));

    // With the scan closed there is nothing to look in, and it says so.
    state.documents.retain(|d| d.id != scan);
    let err = state.find_gta6_archetypes().expect_err("no scan");
    assert!(err.contains("Open the scan of"), "{err}");
}

#[test]
fn an_entity_s_model_is_the_drawable_body_of_its_scan_whose_stated_name_its_archetype_names() {
    let _serial = crate::ui::theme::serial();
    // The drawable states the name "edit#dr", which carries no ending to drop.
    let word = or_io::gta6::hash::name_hash64(b"edit#dr");
    let mut bytes = rpf9_head(1, 0x1A0);
    bytes.resize(0x1000, 0);
    bytes.extend_from_slice(&zlib(&map_types_body(word, 0xAAAA_0001)));
    // A resource starts after a zero slot.
    bytes.resize(bytes.len().next_multiple_of(16) + 16, 0);
    let model = bytes.len() as u64;
    bytes.extend_from_slice(&zlib(&crate::state::gta6_edit::tests::drawable_body()));
    let dir = tempfile::tempdir().expect("a scratch directory");
    let archive = written(dir.path(), &bytes);
    let mut state = AppState::new();
    scanned(&mut state, archive.clone());
    let map_path = body_path(&archive, 0x8000, View::Headerless);
    let MapBody::Data(mut data) = map() else {
        unreachable!("map() is a map's data");
    };
    data.entities[0].archetype_word = word;
    keep_read(&map_path, MapBody::Data(data));
    let doc = open(&mut state, map_path);
    state.active_document = Some(doc);
    state.refresh_scanned_map();
    let shown = state.scanned_map().expect("the map is shown");
    assert_eq!((shown.markers(), shown.unmodelled()), (2, 0));

    state.find_gta6_archetypes().expect("a scan to look in");
    finished(&mut state);
    let shown = state.scanned_map().expect("the map is shown");
    assert_eq!(shown.markers(), 1, "its archetype is found first");
    assert_eq!(shown.models, [Some(model), None], "then the model it names");
    assert_eq!(shown.unmodelled(), 0);
    assert_eq!(
        shown.scene.placed[0].model.as_deref(),
        Some(format!("{model:#X}").as_str())
    );
    let said = state.jobs.ended().first().map(|j| j.said.clone());
    assert!(
        said.as_deref()
            .is_some_and(|s| s.contains("1 of 1 GTA VI model found")),
        "{said:?}"
    );

    state.open_gta6_model(model);
    settle(&mut state);
    let opened = state.active_document().map(|d| (d.kind, d.path.clone()));
    assert_eq!(
        opened,
        Some((
            crate::state::DocumentKind::Gta6Drawable,
            Some(body_path(&archive, model, View::Drawable))
        )),
        "{}",
        state.error_dialog_message
    );
}
