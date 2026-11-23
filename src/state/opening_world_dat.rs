//! The binary world `.dat` files (height map, distant lights, water height map,
//! world blanket and nav precalc), a `*_cache_y.dat`, `metadata.dat` and
//! `audioworldsectorsinfo.dat` open as their own kinds by file name, from disk
//! and from inside an archive, and save back to the bytes they were opened
//! from.
//!
//! ⚠ Every file is built by hand here; the shipped ones are held by `or-io`'s
//! install-gated `height_map_and_distant_lights` and
//! `cache_files_and_world_tables` suites.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::distant_lights::{DistantLightGroup, DistantLightPoint, DistantLights, BLOCKS_HD};
use or_io::height_map::HeightMap;

use super::{AppState, DatTable, DocPayload, DocumentId, DocumentKind, LoadingState};

fn height_map() -> Vec<u8> {
    HeightMap {
        version: 1,
        flags: HeightMap::RLE_DATA,
        num_cols: 3,
        num_rows: 2,
        bounds_min: [-300.0, -200.0, 0.0],
        bounds_max: [300.0, 200.0, 800.0],
        max: vec![0, 40, 90, 0, 0, 0],
        min: vec![0, 10, 20, 0, 0, 0],
        water_mask: Vec::new(),
    }
    .save()
    .expect("saves")
}

fn distant_lights() -> Vec<u8> {
    let cells = BLOCKS_HD * BLOCKS_HD;
    let mut coastal = vec![0; cells];
    coastal[0] = 1;
    DistantLights {
        block_first_group: vec![0; cells],
        block_num_coastal_groups: coastal,
        block_num_inland_groups: vec![0; cells],
        points: vec![DistantLightPoint { x: 1, y: 2, z: 3 }; 4],
        groups: vec![DistantLightGroup {
            centre_x: -500,
            centre_y: 900,
            centre_z: 30,
            radius: 120,
            point_offset: 0,
            point_count: 4,
            display_properties: 0x1211,
            distance_offset: 3.5,
            random_seed: 42,
        }],
    }
    .save()
    .expect("saves")
}

fn open(state: &mut AppState, path: PathBuf) -> DocumentId {
    state.load_file(path.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.load_receiver.is_some() && Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(state.loading, LoadingState::Idle),
        "{}: {}",
        path.display(),
        state.error_dialog_message
    );
    state
        .active_document
        .expect("the file opened as a document")
}

fn saved_as(state: &mut AppState, path: &Path) -> Vec<u8> {
    assert!(state.save_active_document_as(path.to_path_buf()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::read(path).expect("the save wrote the file")
}

#[test]
fn a_heightmap_and_distant_lights_route_by_file_name() {
    for (path, kind) in [
        (
            "common/data/levels/gta5/heightmap.dat",
            DocumentKind::HeightMap,
        ),
        ("HeightMapHeistIsland.dat", DocumentKind::HeightMap),
        ("distantlights.dat", DocumentKind::DistantLights),
        ("distantlights_hd.dat", DocumentKind::DistantLights),
        ("data/gta5_cache_y.dat", DocumentKind::DatTable),
        ("mp2023_01_3100635916_cache_y.dat", DocumentKind::DatTable),
        ("x64/metadata.dat", DocumentKind::DatTable),
        (
            "levels/gta5/AudioWorldSectorsInfo.dat",
            DocumentKind::DatTable,
        ),
        ("popcycle.dat", DocumentKind::Text),
    ] {
        assert_eq!(
            DocumentKind::from_path(Path::new(path)),
            Some(kind),
            "{path}"
        );
    }
}

#[test]
fn a_heightmap_opens_from_disk_and_saves_its_bytes() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bytes = height_map();
    let path = dir.path().join("heightmap.dat");
    std::fs::write(&path, &bytes).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    let Some(DocPayload::HeightMap(map)) = state.document(doc).map(|d| &d.payload) else {
        panic!("opened as {:?}", state.document(doc).map(|d| d.kind));
    };
    assert_eq!((map.num_cols, map.num_rows, map.max[2]), (3, 2, 90));
    assert_eq!(saved_as(&mut state, &dir.path().join("saved.dat")), bytes);
}

#[test]
fn a_heightmap_offers_its_image_to_the_texture_workspace() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("heightmap.dat");
    std::fs::write(&path, height_map()).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    assert_eq!(state.active_workspace, crate::ui::dock::Workspace::Texture);

    let payload = &state.document(doc).expect("open").payload;
    assert!(super::textures::image_label(payload).is_some());
    let image = super::textures::document_image(payload).expect("the height map is an image");
    assert_eq!((image.width, image.height), (3, 2));
    let grey = |x: u32, y: u32| image.data[((y * image.width + x) * 4) as usize];
    // Row 0 of the grid is its south edge, so it is the image's bottom row.
    assert_eq!([grey(0, 1), grey(1, 1), grey(2, 1)], [0, 40, 90]);
    assert_eq!([grey(0, 0), grey(1, 0), grey(2, 0)], [0, 0, 0]);
}

#[test]
fn a_water_map_world_blanket_and_nav_precalc_open_save_and_offer_a_picture() {
    use or_io::nav_precalc::{NavPrecalc, NavPrecalcCell};
    use or_io::water_map::{WaterMap, WaterMapRefId, WaterMapTileRow};
    use or_io::world_blanket::WorldBlanket;

    // Row 0 (north) stores column 1, whose one reference is a lake.
    let water = WaterMap {
        num_tile_cols: 2,
        tile_rows: vec![
            WaterMapTileRow {
                first: 1,
                count: 1,
                start: 0,
            },
            WaterMapTileRow::default(),
        ],
        tiles: vec![0],
        ref_ids: vec![WaterMapRefId(0x8000 | 0x4000)],
        ..WaterMap::default()
    };
    let blanket = WorldBlanket {
        cells: vec![0, 2, 1, 0],
    };
    let cell = |n: usize| NavPrecalcCell {
        groups: [vec![[0; 4]; n], vec![], vec![], vec![]],
        ..NavPrecalcCell::default()
    };
    let precalc = NavPrecalc {
        header: [3, 1],
        cells: vec![cell(0), cell(4), cell(2), cell(0)],
        bits: vec![1, 2, 3],
    };
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    for (name, kind, bytes, top_row) in [
        (
            "waterheight.dat",
            DocumentKind::WaterMap,
            water.save().expect("saves"),
            [0, 0x40],
        ),
        (
            "worldblanket.dat",
            DocumentKind::WorldBlanket,
            blanket.save(),
            [0, 255],
        ),
        (
            "navprecalc.dat",
            DocumentKind::NavPrecalc,
            precalc.save().expect("saves"),
            [0, 255],
        ),
    ] {
        assert_eq!(
            DocumentKind::from_path(Path::new(name)),
            Some(kind),
            "{name}"
        );
        let path = dir.path().join(name);
        std::fs::write(&path, &bytes).expect("written");
        let doc = open(&mut state, path);
        assert_eq!(state.document(doc).map(|d| d.kind), Some(kind), "{name}");
        assert_eq!(state.active_workspace, crate::ui::dock::Workspace::Texture);

        let payload = &state.document(doc).expect("open").payload;
        assert!(super::textures::image_label(payload).is_some(), "{name}");
        let image = super::textures::document_image(payload).expect("a picture");
        assert_eq!((image.width, image.height), (2, 2), "{name}");
        assert_eq!([image.data[0], image.data[4]], top_row, "{name}");

        let saved = saved_as(&mut state, &dir.path().join(format!("saved_{name}")));
        assert_eq!(saved, bytes, "{name}");
    }
}

#[test]
fn distant_lights_open_from_an_archive_and_from_disk_and_save_their_bytes() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bytes = distant_lights();
    let mut state = AppState::new();

    let root = dir.path().join("install");
    std::fs::create_dir_all(&root).expect("the install root");
    let archive = super::document_io::serialize_archive_records(
        vec![(
            "data/levels/gta5/distantlights_hd.dat".to_string(),
            bytes.clone(),
            false,
        )],
        super::RpfOutputMode::Open,
        state.keys.provider(),
    )
    .expect("archive bytes");
    let rpf = root.join("common.rpf");
    std::fs::write(&rpf, archive).expect("the archive is written");
    let archive_doc = open(&mut state, rpf);
    let entry = {
        let Some(DocPayload::Rpf(rpf)) = state.document(archive_doc).map(|d| &d.payload) else {
            panic!(
                "the archive opened as {:?}",
                state.document(archive_doc).map(|d| d.kind)
            );
        };
        let mut stack = vec![rpf.session.root_id()];
        let mut found = None;
        while let Some(id) = stack.pop() {
            if rpf
                .session
                .node(id)
                .is_some_and(|n| n.name == "distantlights_hd.dat")
            {
                found = Some(id);
            }
            stack.extend(rpf.session.children_of(id));
        }
        found.expect("the archive lists the entry")
    };
    state.open_rpf_entry(archive_doc, entry);
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.load_receiver.is_some() && Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    let from_archive = state.active_document.expect("the entry opened");
    assert_eq!(
        state.document(from_archive).map(|d| d.kind),
        Some(DocumentKind::DistantLights),
        "{}",
        state.error_dialog_message
    );

    let path = dir.path().join("distantlights_hd.dat");
    std::fs::write(&path, &bytes).expect("written");
    let doc = open(&mut state, path);
    let Some(DocPayload::DistantLights(lights)) = state.document(doc).map(|d| &d.payload) else {
        panic!("opened as {:?}", state.document(doc).map(|d| d.kind));
    };
    assert_eq!(lights.blocks(), BLOCKS_HD);
    assert_eq!(lights.groups[0].random_seed, 42);
    assert_eq!(lights.points_of(&lights.groups[0]).len(), 4);
    assert_eq!(
        saved_as(&mut state, &dir.path().join("saved_hd.dat")),
        bytes
    );
}

#[test]
fn a_cache_file_metadata_and_the_audio_sectors_open_from_disk_and_save_their_bytes() {
    let cache = or_io::cache_loader::CacheFile {
        bounds_store: vec![
            or_io::cache_loader::BoundsStoreCacheEntry {
                hash_key: 9,
                asset_type: 1,
                pad: [0x7f, 0, 0],
                ..Default::default()
            };
            2
        ],
        ..Default::default()
    };
    let metadata = or_io::social_club_metadata::SocialClubMetadata {
        format_word: 1,
        built: "2024-02-01T15:54:51".into(),
        body: vec![2, 4, 4, 0],
    };
    let mut grid = or_io::audio_world_sectors::AudioWorldSectors::default();
    grid.sectors[3].is_water_sector = true;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::new();
    for (name, bytes, kind) in [
        (
            "gta5_cache_y.dat",
            cache.save().expect("saves"),
            DocumentKind::DatTable,
        ),
        (
            "metadata.dat",
            metadata.save().expect("saves"),
            DocumentKind::DatTable,
        ),
        (
            "audioworldsectorsinfo.dat",
            grid.save().expect("saves"),
            DocumentKind::DatTable,
        ),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, &bytes).expect("written");
        let doc = open(&mut state, path);
        let opened = state.document(doc).map(|d| d.kind);
        assert_eq!(opened, Some(kind), "{name}");
        match state.document(doc).map(|d| &d.payload) {
            Some(DocPayload::DatTable(DatTable::CacheFile(read))) => assert_eq!(read, &cache),
            Some(DocPayload::DatTable(DatTable::SocialClubMetadata(read))) => {
                assert_eq!(read, &metadata)
            }
            Some(DocPayload::DatTable(DatTable::AudioWorldSectors(read))) => {
                assert_eq!(read, &grid)
            }
            _ => panic!("{name} opened as {opened:?}"),
        }
        let saved = dir.path().join(format!("saved_{name}"));
        assert_eq!(saved_as(&mut state, &saved), bytes, "{name}");
    }
}
