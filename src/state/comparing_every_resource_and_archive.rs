//! Two drawables compared from their declared fields differ in each changed
//! field with both values, and two archives compared entry by entry differ in
//! the entries added, removed and changed.

use std::io::Read as _;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_editor::MAX_ELEMENTS;
use or_io::container::{Gta5Keys, Gta5RetailCryptoProvider, RpfCryptoProvider};
use or_io::decl::Gen8;
use or_io::gta5::bounds::build::{self, BoundMesh};
use or_io::gta5::bounds::{PhBoundAny, PhMaterialMgrId};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::{
    AtMap, AtMapEntry, CrAnimDictionary, CrAnimation, CrBlockStream, CrClipDictionary, File as _,
    GrmModel, GrmShaderGroup, GtaDrawable, PgDictionary, RmcLod,
};
use or_test_support::{skip_install, GAME_ROOT_ENV};

use super::compare::{By, Change, Comparison};
use super::comparing_two_files::{command, fields, run, shown, state};
use super::defragmenting_an_archive::{fragmented, settle};
use super::document_io::serialize_archive_records;
use super::jobs::JobKind;
use super::nested_archives_survive_a_save::archive as archive_bytes;
use super::opening_a_ydd::{node_at, with_keys};
use super::prefs::RpfOutputMode;
use super::what_a_comparison_still_missed::install_roots;
use super::{AppState, DocPayload, DocumentId, DocumentKind, RpfNodeId};
use crate::ui::commands::Action;

/// A changed field as the tab lists it: where, which field, how, and both values.
pub(super) type Row = (String, String, Change, String, String);

pub(super) fn row(record: &str, field: &str, change: Change, left: &str, right: &str) -> Row {
    let text = |s: &str| s.to_string();
    (text(record), text(field), change, text(left), text(right))
}

pub(super) fn rows(comparison: &Comparison) -> Vec<Row> {
    let (_, changes) = fields(comparison);
    (changes.into_iter())
        .map(|c| (c.record, c.field, c.change, c.left, c.right))
        .collect()
}

/// `a` and `b`, files of a scratch folder, compared from their rows' menus: the tab that opens.
pub(super) fn compare_files(a: (&str, &[u8]), b: (&str, &[u8])) -> (Comparison, String) {
    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(dir.path().join(a.0), a.1).expect("the first file");
    std::fs::write(dir.path().join(b.0), b.1).expect("the second file");
    let mut state = state();
    state.open_folder(dir.path().to_path_buf());
    let folder = state.active_document.expect("the folder opens");
    let (left, right) = (
        node_at(&mut state, folder, a.0),
        node_at(&mut state, folder, b.0),
    );
    run(&mut state, Action::SelectForCompare, folder, left);
    run(&mut state, Action::CompareWithSelected, folder, right);
    shown(&state)
}

pub(super) fn drawable(name: &str, distance: f32, radius: f32) -> GtaDrawable<Gen8> {
    let mut d = GtaDrawable::<Gen8>::default();
    d.base.name = name.to_string();
    d.base.lod_group.lod_distances[0] = distance;
    d.base.lod_group.bounds_sphere.center_and_radius.w = radius;
    d
}

pub(super) fn saved(d: &GtaDrawable<Gen8>) -> Vec<u8> {
    d.to_bytes().expect("a drawable saves")
}

/// A drawable whose first level of detail holds one model for each of `flags`.
pub(super) fn with_models(flags: &[u8]) -> Vec<u8> {
    let models = (flags.iter())
        .map(|&flags| {
            let model = GrmModel::<Gen8> {
                flags,
                ..Default::default()
            };
            Box::new(model)
        })
        .collect();
    let mut d = GtaDrawable::<Gen8>::default();
    d.base.lod_group.lods[0] = RmcLod {
        models,
        ..Default::default()
    }
    .into();
    saved(&d)
}

#[test]
fn two_drawables_compared_from_their_declared_fields_differ_in_each_changed_field_with_both_values()
{
    let left = saved(&drawable("one", 9998.0, 5.0));
    let mut changed = drawable("two", 120.5, 7.25);
    changed.base.shader_group = Some(Box::default());
    let right = saved(&changed);

    let (compared, title) = compare_files(("a.ydr", &left), ("b.ydr", &right));
    assert_eq!(title, "a.ydr ↔ b.ydr");
    assert_eq!(fields(&compared).0, By::PartAndField);
    assert_eq!(
        rows(&compared),
        vec![
            row(
                "",
                "Base > LodGroup > BoundsSphere > CenterAndRadius > W",
                Change::Changed,
                "5.0000",
                "7.2500"
            ),
            row(
                "",
                "Base > LodGroup > LodDistances [0]",
                Change::Changed,
                "9998.000",
                "120.500"
            ),
            row("", "Base > Name", Change::Changed, "one", "two"),
            row(
                "Base > ShaderGroup",
                "",
                Change::Added,
                "",
                "GrmShaderGroup"
            ),
        ]
    );
    assert_eq!(compared.summary(), "by part and field, 4 differences");
    assert_eq!(compared.stops(), vec![0, 1, 2, 3], "each is a stop");
}

#[test]
fn two_drawables_that_hold_the_same_compare_as_the_same_whatever_their_bytes() {
    let one = saved(&drawable("same", 100.0, 2.0));
    let (compared, _) = compare_files(("a.ydr", &one), ("b.ydr", &one));
    assert_eq!(fields(&compared).0, By::PartAndField);
    assert_eq!(rows(&compared), Vec::<Row>::new());
    assert_eq!(compared.summary(), "by part and field, no differences");
}

#[test]
fn a_model_added_or_dropped_from_a_run_leaves_the_models_around_it_lined_up() {
    let (compared, _) = compare_files(
        ("a.ydr", &with_models(&[1, 2, 3, 4])),
        ("b.ydr", &with_models(&[2, 3, 9, 4, 5])),
    );
    let model = |n: usize| format!("Base > LodGroup > Lods[0] > Models[{n}]");
    assert_eq!(
        rows(&compared),
        vec![
            row(&model(0), "", Change::Removed, "GrmModel", ""),
            row(&model(2), "", Change::Added, "", "GrmModel"),
            row(&model(4), "", Change::Added, "", "GrmModel"),
        ],
        "the models both hold, 2, 3 and 4, are not reported"
    );

    let (compared, _) = compare_files(
        ("a.ydr", &with_models(&[1, 2])),
        ("b.ydr", &with_models(&[1, 7])),
    );
    assert_eq!(
        rows(&compared),
        vec![row(&model(1), "Flags", Change::Changed, "2", "7")]
    );
}

/// A drawable holding one embedded texture whose pixels are `pixels`.
fn with_pixels(pixels: u8) -> Vec<u8> {
    named_with_pixels("", pixels)
}

/// [`with_pixels`], the drawable called `name`.
fn named_with_pixels(name: &str, pixels: u8) -> Vec<u8> {
    let texture = GrcTexture {
        name: "embedded".into(),
        width: 4,
        height: 4,
        format: u32::from_le_bytes(*b"DXT1"),
        mip_count: 1,
        backing_store: vec![pixels; 8],
        ..Default::default()
    };
    let mut dict = PgDictionary::<GrcTexture>::default();
    dict.insert(texture);
    let group = GrmShaderGroup::<Gen8> {
        texture_dictionary: Some(Box::new(dict)),
        ..Default::default()
    };
    let mut d = GtaDrawable::<Gen8>::default();
    d.base.name = name.to_string();
    d.base.shader_group = Some(Box::new(group));
    saved(&d)
}

#[test]
fn a_difference_in_data_no_field_lists_is_still_a_difference() {
    let (compared, _) = compare_files(("a.ydr", &with_pixels(1)), ("b.ydr", &with_pixels(2)));
    let found = rows(&compared);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].1, "Other data");
    assert_eq!(found[0].2, Change::Changed);
    assert!(
        !found[0].3.is_empty() && found[0].4.contains("differing from byte"),
        "{found:?}"
    );
}

#[test]
fn a_difference_in_data_no_field_lists_is_reported_beside_the_fields_that_differ() {
    let (compared, _) = compare_files(
        ("a.ydr", &named_with_pixels("one", 1)),
        ("b.ydr", &named_with_pixels("two", 2)),
    );
    let found = rows(&compared);
    assert_eq!(found.len(), 2, "the name and the pixels: {found:?}");
    assert_eq!(
        found[0],
        row("", "Base > Name", Change::Changed, "one", "two")
    );
    assert_eq!(found[1].1, "Other data", "{found:?}");
    assert!(found[1].4.contains("differing from byte"), "{found:?}");

    let (named, _) = compare_files(
        ("a.ydr", &named_with_pixels("one", 1)),
        ("b.ydr", &named_with_pixels("two", 1)),
    );
    assert_eq!(
        rows(&named),
        vec![row("", "Base > Name", Change::Changed, "one", "two")],
        "the same pixels under two names are not other data"
    );
}

/// A clip dictionary of one animation lasting `duration`, whose one block of channels holds `channels`.
fn with_channels(duration: f32, channels: &[u8]) -> Vec<u8> {
    let block = CrBlockStream {
        data: channels.to_vec(),
        ..Default::default()
    };
    let animation = CrAnimation {
        duration,
        blocks: vec![Some(Box::new(block))],
        ..Default::default()
    };
    let entry = AtMapEntry {
        key: 0x1000,
        data: Some(Box::new(animation)),
        ..Default::default()
    };
    let dictionary = CrClipDictionary {
        anim_dictionary: Some(Box::new(CrAnimDictionary {
            animations: AtMap {
                hash: vec![Some(Box::new(entry))],
                ..Default::default()
            },
            ..Default::default()
        })),
        ..Default::default()
    };
    dictionary.to_bytes().expect("a clip dictionary saves")
}

#[test]
fn a_blob_a_field_reaches_is_compared_beside_the_fields_that_differ_and_alone() {
    let (compared, _) = compare_files(
        ("a.ycd", &with_channels(1.0, &[1, 2, 3, 4])),
        ("b.ycd", &with_channels(2.0, &[1, 2, 3, 5])),
    );
    let found = rows(&compared);
    assert_eq!(found.len(), 2, "the duration and the channels: {found:?}");
    assert_eq!(found[0].1, "Duration", "{found:?}");
    assert_eq!(found[1].1, "Data", "{found:?}");
    assert_eq!(found[1].2, Change::Changed);
    assert_eq!(found[1].3, "4 B");
    assert_eq!(found[1].4, "4 B, differing from byte 3");

    let (alone, _) = compare_files(
        ("a.ycd", &with_channels(1.0, &[1, 2, 3, 4])),
        ("b.ycd", &with_channels(1.0, &[1, 2, 3, 5])),
    );
    let found = rows(&alone);
    assert_eq!(
        found.len(),
        1,
        "the channels, not the whole file: {found:?}"
    );
    assert_eq!(found[0].1, "Data");
    assert_eq!(found[0].4, "4 B, differing from byte 3");

    let (same, _) = compare_files(
        ("a.ycd", &with_channels(1.0, &[1, 2, 3, 4])),
        ("b.ycd", &with_channels(2.0, &[1, 2, 3, 4])),
    );
    let found = rows(&same);
    assert_eq!(
        found.len(),
        1,
        "the same channels are not a difference: {found:?}"
    );
    assert_eq!(found[0].1, "Duration");
}

#[test]
fn a_blob_that_changes_length_is_one_row_and_not_also_a_row_for_its_run() {
    for (left, right) in [(&[1, 2, 3, 4][..], &[1, 2, 3, 4, 5][..]), (&[], &[1, 2])] {
        let (compared, _) = compare_files(
            ("a.ycd", &with_channels(1.0, left)),
            ("b.ycd", &with_channels(1.0, right)),
        );
        let found = rows(&compared);
        let sizes = |bytes: &[u8]| or_job::human_bytes(bytes.len() as u64);
        let block = "AnimDictionary > Animations > Hash[0] > Data > Blocks[0]";
        assert_eq!(
            found,
            vec![row(
                block,
                "Data",
                Change::Changed,
                &sizes(left),
                &sizes(right)
            )],
            "{left:?} -> {right:?}"
        );
    }
}

/// A quad whose two triangles are of the materials `surface` names, the second being `last`.
pub(super) fn quad_mesh(surface: [u8; 2], last: [u16; 3]) -> BoundMesh {
    let corner = |x: f32, y: f32| [x, y, 0.0];
    let mut mesh = BoundMesh::new(
        vec![
            corner(0.0, 0.0),
            corner(1.0, 0.0),
            corner(1.0, 1.0),
            corner(0.0, 1.0),
        ],
        vec![[0, 1, 2], last],
    );
    mesh.materials = vec![PhMaterialMgrId::default(); 2];
    mesh.triangle_materials = surface.to_vec();
    mesh
}

/// A `.ybn` of one geometry over [`quad_mesh`], whose header states `flags`.
fn quad(flags: u8, surface: [u8; 2], last: [u16; 3]) -> Vec<u8> {
    let mut bound = build::geometry(&quad_mesh(surface, last)).expect("a quad is a geometry");
    bound.base.flags = flags;
    PhBoundAny::PhBoundGeometry(bound)
        .to_bytes()
        .expect("a bound saves")
}

fn with_surface(flags: u8, surface: [u8; 2]) -> Vec<u8> {
    quad(flags, surface, [0, 2, 3])
}

/// A `.ybn` of one BVH over [`quad_mesh`]: a bound whose base is a geometry whose base is the BVH again.
fn bvh_quad(surface: [u8; 2]) -> Vec<u8> {
    let bound = build::bvh(&quad_mesh(surface, [0, 2, 3])).expect("a quad is a BVH");
    PhBoundAny::PhBoundBVH(bound)
        .to_bytes()
        .expect("a bound saves")
}

#[test]
fn a_blob_behind_a_bytes_pointer_is_compared_beside_the_fields_that_differ_and_alone() {
    let (compared, _) = compare_files(
        ("a.ybn", &with_surface(0, [0, 1])),
        ("b.ybn", &with_surface(3, [1, 0])),
    );
    let found = rows(&compared);
    assert_eq!(found.len(), 2, "the flags and the materials: {found:?}");
    assert_eq!(found[0].1, "Base > Flags", "{found:?}");
    assert_eq!(
        found[1],
        row(
            "",
            "PolyMatIndexList",
            Change::Changed,
            "2 B",
            "2 B, differing from byte 0"
        )
    );

    let (alone, _) = compare_files(
        ("a.ybn", &with_surface(0, [0, 1])),
        ("b.ybn", &with_surface(0, [0, 0])),
    );
    let found = rows(&alone);
    assert_eq!(
        found.len(),
        1,
        "the materials, not the whole file: {found:?}"
    );
    assert_eq!(found[0].1, "PolyMatIndexList");
    assert_eq!(found[0].4, "2 B, differing from byte 1");

    let (same, _) = compare_files(
        ("a.ybn", &with_surface(0, [0, 1])),
        ("b.ybn", &with_surface(3, [0, 1])),
    );
    let found = rows(&same);
    assert_eq!(
        found.len(),
        1,
        "the same materials are no difference: {found:?}"
    );
    assert_eq!(found[0].1, "Base > Flags");
}

#[test]
fn a_bound_is_read_once_though_its_tagged_base_repeats_it_down_to_the_depth_limit() {
    let (compared, _) = compare_files(
        ("a.ybn", &quad(0, [0, 1], [0, 2, 3])),
        ("b.ybn", &quad(0, [0, 1], [1, 2, 3])),
    );
    let polygon = |n: usize| format!("Polygons[{n}]");
    assert_eq!(
        rows(&compared),
        vec![
            row(
                &polygon(0),
                "NeighboringPolygons [1]",
                Change::Changed,
                "65535",
                "1"
            ),
            row(
                &polygon(0),
                "NeighboringPolygons [2]",
                Change::Changed,
                "1",
                "65535"
            ),
            row(&polygon(1), "VertexIndices [0]", Change::Changed, "0", "1"),
        ],
        "each change once, not once for each level the base repeats the bound to"
    );
    assert_eq!(
        compared.unread(),
        None,
        "a repeat holds what the bound holds, so nothing under it is left out"
    );
}

#[test]
fn a_bound_whose_base_resolves_to_another_that_holds_it_is_read_once_too() {
    let (compared, _) = compare_files(("a.ybn", &bvh_quad([0, 1])), ("b.ybn", &bvh_quad([1, 0])));
    assert_eq!(
        rows(&compared),
        vec![row(
            "Base",
            "PolyMatIndexList",
            Change::Changed,
            "2 B",
            "2 B, differing from byte 0"
        )],
        "the geometry the BVH holds, once, not again for each time the two hold each other"
    );
    assert_eq!(compared.unread(), None);
}

/// A run of more models than an outline reads, each told apart by its flags.
fn long_run() -> Vec<u8> {
    let run: Vec<u8> = (1..=150).collect();
    assert!(run.len() > MAX_ELEMENTS);
    run
}

fn read_to_the_limit(runs: usize) -> String {
    let word = if runs == 1 { "run" } else { "runs" };
    format!("Not read, in either file: elements past the {MAX_ELEMENTS}th in {runs} {word}.")
}

#[test]
fn a_model_added_to_a_run_past_the_limit_is_one_added_row_and_the_comparison_says_what_it_did_not_read(
) {
    let mut grown = long_run();
    grown.insert(3, 200);
    let (compared, _) = compare_files(
        ("a.ydr", &with_models(&long_run())),
        ("b.ydr", &with_models(&grown)),
    );
    assert_eq!(
        rows(&compared),
        vec![row(
            "Base > LodGroup > Lods[0] > Models[3]",
            "",
            Change::Added,
            "",
            "GrmModel"
        )],
        "the model the add pushed past the limit is not reported removed"
    );
    assert_eq!(compared.unread(), Some(read_to_the_limit(2).as_str()));
}

#[test]
fn an_edit_inside_two_runs_that_both_pass_the_limit_is_one_changed_row() {
    let mut edited = long_run();
    edited[5] = 250;
    let (compared, _) = compare_files(
        ("a.ydr", &with_models(&long_run())),
        ("b.ydr", &with_models(&edited)),
    );
    assert_eq!(
        rows(&compared),
        vec![row(
            "Base > LodGroup > Lods[0] > Models[5]",
            "Flags",
            Change::Changed,
            "6",
            "250"
        )]
    );
    assert_eq!(compared.unread(), Some(read_to_the_limit(2).as_str()));
}

#[test]
fn a_difference_past_the_limit_is_not_listed_and_the_comparison_says_so() {
    let mut past = long_run();
    past[140] = 250;
    let (compared, _) = compare_files(
        ("a.ydr", &with_models(&long_run())),
        ("b.ydr", &with_models(&past)),
    );
    let found = rows(&compared);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].1, "Other data", "no part read differs: {found:?}");
    assert_eq!(compared.unread(), Some(read_to_the_limit(2).as_str()));
}

#[test]
fn nothing_is_said_to_be_unread_where_nothing_was_left_out_or_the_two_are_the_same() {
    let (compared, _) = compare_files(
        ("a.ydr", &saved(&drawable("one", 1.0, 2.0))),
        ("b.ydr", &saved(&drawable("two", 1.0, 2.0))),
    );
    assert_eq!(rows(&compared).len(), 1);
    assert_eq!(compared.unread(), None);

    let (same, _) = compare_files(
        ("a.ydr", &with_models(&long_run())),
        ("b.ydr", &with_models(&long_run())),
    );
    assert_eq!(rows(&same), Vec::<Row>::new());
    assert_eq!(same.unread(), None, "the same needs no part read");
}

/// A folder of `a.rpf` and `b.rpf`, compared from their rows' menus.
fn archives_compared(left: &[u8], right: &[u8]) -> (Comparison, String) {
    compare_files(("a.rpf", left), ("b.rpf", right))
}

#[test]
fn two_archives_compared_entry_by_entry_find_the_entries_added_removed_and_changed() {
    let inside = |deep: &[u8]| archive_bytes(&[("deep.txt", deep), ("kept.txt", b"same")]);
    let left = archive_bytes(&[
        ("kept.txt", b"unchanged"),
        ("edited.dat", b"before"),
        ("same_size.bin", b"abcdefgh"),
        ("gone.txt", b"removed"),
        ("inner.rpf", &inside(b"one")),
    ]);
    let right = archive_bytes(&[
        ("KEPT.txt", b"unchanged"),
        ("edited.dat", b"after, longer"),
        ("same_size.bin", b"abcXefgh"),
        ("new/added.txt", b"added"),
        ("inner.rpf", &inside(b"two!")),
    ]);

    let (compared, title) = archives_compared(&left, &right);
    assert_eq!(title, "a.rpf ↔ b.rpf");
    assert_eq!(fields(&compared).0, By::Entry);
    assert_eq!(
        rows(&compared),
        vec![
            row("edited.dat", "Content", Change::Changed, "6 B", "13 B"),
            row("gone.txt", "", Change::Removed, "7 B", ""),
            row(
                "inner.rpf/deep.txt",
                "Content",
                Change::Changed,
                "3 B",
                "4 B"
            ),
            row("new/added.txt", "", Change::Added, "", "5 B"),
            row(
                "same_size.bin",
                "Content",
                Change::Changed,
                "8 B",
                "8 B, differing from byte 3"
            ),
        ],
        "an entry is found at its path whatever its case; a nested archive by the files in it"
    );
    assert_eq!(compared.summary(), "by entry, 5 differences");

    let (same, _) = archives_compared(&left, &left);
    assert_eq!(rows(&same), Vec::<Row>::new());
    assert_eq!(same.summary(), "by entry, no differences");
}

/// The tree of a folder holding `a.rpf`, `name` and a folder: the folder on disk, which a caller binds before the state so the state is dropped first, and the rows of the archive and of `name`.
fn folder_of(
    name: &str,
    bytes: &[u8],
) -> (
    tempfile::TempDir,
    AppState,
    DocumentId,
    RpfNodeId,
    RpfNodeId,
) {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let at = dir.path();
    std::fs::write(at.join("a.rpf"), archive_bytes(&[("x.txt", b"x")])).expect("the archive");
    std::fs::write(at.join(name), bytes).expect("the file");
    std::fs::create_dir(at.join("folder")).expect("a folder");
    let mut state = state();
    state.open_folder(at.to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let archive = node_at(&mut state, doc, "a.rpf");
    let other = node_at(&mut state, doc, name);
    (dir, state, doc, archive, other)
}

#[test]
fn a_file_compares_only_with_a_file_and_an_archive_with_an_archive() {
    let (_dir, mut state, doc, archive, file) = folder_of("b.txt", b"text");
    let folder = node_at(&mut state, doc, "folder");
    let with_selected =
        |state: &AppState, row| command(state, Action::CompareWithSelected, doc, row).enabled;

    run(&mut state, Action::SelectForCompare, doc, archive);
    assert_eq!(
        with_selected(&state, file),
        Err("An archive compares with another archive: choose one")
    );
    run(&mut state, Action::SelectForCompare, doc, file);
    assert_eq!(
        with_selected(&state, archive),
        Err("A file compares with another file: choose one")
    );
    assert_eq!(
        command(&state, Action::SelectForCompare, doc, folder).enabled,
        Err("A folder is not compared: choose a file or an archive")
    );
}

#[test]
fn an_archive_with_changes_not_saved_is_compared_only_once_it_is_saved() {
    let (_dir, mut state, doc, archive, other) =
        folder_of("b.rpf", &archive_bytes(&[("x.txt", b"y")]));
    run(&mut state, Action::SelectForCompare, doc, other);
    assert_eq!(
        command(&state, Action::CompareWithSelected, doc, archive).enabled,
        Ok(())
    );

    // Edited inside the tree, which holds the edit and the file on disk does not.
    let inside = node_at(&mut state, doc, "a.rpf/x.txt");
    let Some(super::DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        unreachable!("a folder's tree");
    };
    rpf.session
        .set_bytes_for(inside, b"edited".to_vec())
        .expect("the entry takes its bytes");
    const UNSAVED: &str =
        "Save the archive's changes first: a comparison reads it as it was last saved";
    for action in [Action::CompareWithSelected, Action::SelectForCompare] {
        assert_eq!(
            command(&state, action, doc, archive).enabled,
            Err(UNSAVED),
            "{action:?}"
        );
    }
}

#[test]
fn a_comparison_waits_for_a_defragment_of_an_archive_it_reads() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let held = dir.path().join("a.rpf");
    std::fs::write(&held, fragmented()).expect("the fragmented archive");
    std::fs::write(dir.path().join("b.rpf"), archive_bytes(&[("x.txt", b"x")]))
        .expect("the other archive");
    let mut state = state();
    state.open_folder(dir.path().to_path_buf());
    let folder = state.active_document.expect("the folder opens");
    let (a, b) = (
        node_at(&mut state, folder, "a.rpf"),
        node_at(&mut state, folder, "b.rpf"),
    );

    state.defragment_archive(folder, a);
    settle(&mut state);
    assert!(state.defrag.holds(&held), "the size preview is on offer");

    run(&mut state, Action::SelectForCompare, folder, b);
    state.toasts.clear();
    // The command is not offered while one holds it; a call past that is refused too.
    state.compare_with_selected(folder, a);
    assert!(
        state
            .documents
            .iter()
            .all(|d| d.kind != DocumentKind::Compare),
        "a comparison read an archive a defragment may rewrite"
    );
    let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
    assert!(
        said.iter()
            .any(|said| said.contains("is being defragmented")),
        "{said:?}"
    );

    state.answer_defrag(false);
    run(&mut state, Action::CompareWithSelected, folder, a);
    let (compared, _) = shown(&state);
    assert_eq!(compared.summary(), "by entry, 4 differences");
}

/// An unencrypted archive written at `path`, with the folders above it.
fn write(path: &std::path::Path, files: &[(&str, &[u8])]) {
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("its folder");
    std::fs::write(path, archive_bytes(files)).expect("the archive");
}

#[test]
fn an_archive_in_the_overlay_compares_with_the_games_and_one_with_none_says_so() {
    let root = tempfile::tempdir().expect("a scratch install");
    std::fs::write(root.path().join("gta5.exe"), b"").expect("the marker an install has");
    write(
        &root.path().join("update/update.rpf"),
        &[
            ("kept.txt", b"same"),
            ("edited.dat", b"game"),
            ("gone.txt", b"x"),
        ],
    );
    write(
        &root.path().join("mods/update/update.rpf"),
        &[
            ("kept.txt", b"same"),
            ("edited.dat", b"the mod's"),
            ("added.txt", b"new"),
        ],
    );
    write(
        &root.path().join("mods/update/only_in_mods.rpf"),
        &[("a.txt", b"a")],
    );

    let mut state = state();
    let game = state.open_game_root(root.path().to_path_buf());
    let shipped = node_at(&mut state, game, "update/update.rpf");
    assert_eq!(
        command(&state, Action::CompareWithOriginal, game, shipped).enabled,
        Err("Only a file or an archive in the mods overlay has an original to compare with")
    );
    let modded = node_at(&mut state, game, "mods/update/update.rpf");
    run(&mut state, Action::CompareWithOriginal, game, modded);

    let (compared, title) = shown(&state);
    assert_eq!(title, "update.rpf — game ↔ mods");
    assert_eq!(compared.headings, ["Game", "Mods overlay"]);
    // The game's place is the install's path as the platform spells it, so its parts are compared, not its separators.
    let game_side = std::path::Path::new(&compared.places[0]);
    assert!(
        game_side.ends_with("update/update.rpf")
            && game_side.components().all(|c| c.as_os_str() != "mods"),
        "the left is the game's own archive: {:?}",
        compared.places
    );
    assert_eq!(fields(&compared).0, By::Entry);
    assert_eq!(
        rows(&compared),
        vec![
            row("added.txt", "", Change::Added, "", "3 B"),
            row("edited.dat", "Content", Change::Changed, "4 B", "9 B"),
            row("gone.txt", "", Change::Removed, "1 B", ""),
        ]
    );

    let documents = state.documents.len();
    let only = node_at(&mut state, game, "mods/update/only_in_mods.rpf");
    run(&mut state, Action::CompareWithOriginal, game, only);
    assert_eq!(state.documents.len(), documents, "a comparison opened");
    let said = (state.jobs.ended().iter())
        .find(|job| job.kind == JobKind::Compare)
        .map(|job| job.said.clone());
    assert_eq!(
        said.as_deref(),
        Some("The game has no only_in_mods.rpf of its own: the mods overlay adds it")
    );
}

#[test]
fn a_cancel_from_the_job_list_tells_the_comparison_running_to_stop() {
    let mut state = state();
    let (told, heard) = std::sync::mpsc::channel();
    state
        .compare
        .start("a.rpf ↔ b.rpf".into(), Vec::new(), move |stop| {
            let until = Instant::now() + Duration::from_secs(30);
            while !stop.load(Ordering::Relaxed) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = told.send(stop.load(Ordering::Relaxed));
            super::compare::Ended::Failed("stopped".into())
        });
    let compare_running =
        |state: &AppState| (state.running_jobs().iter()).any(|job| job.kind == JobKind::Compare);
    assert!(compare_running(&state), "the comparison is in the job list");
    state.cancel_job(JobKind::Compare);
    assert!(
        !compare_running(&state),
        "the cancel left it in the job list"
    );
    assert_eq!(
        heard.recv_timeout(Duration::from_secs(60)),
        Ok(true),
        "the worker was not told to stop"
    );
}

/// Keys that exist only here: an AES archive written under them is read only by a provider that holds them.
fn synthetic_provider() -> Arc<dyn RpfCryptoProvider> {
    let mut keys = Gta5Keys::empty();
    keys.aes_key = [0xA5; 32];
    keys.provenance = or_io::container::rpf::crypto::KeyProvenance::all_from_executable();
    Arc::new(Gta5RetailCryptoProvider::new(Arc::new(keys)))
}

/// An AES archive of `files`, written at `path` under `crypto`.
fn write_aes(path: &std::path::Path, files: &[(&str, &[u8])], crypto: &Arc<dyn RpfCryptoProvider>) {
    let records = (files.iter())
        .map(|(name, bytes)| (name.to_string(), bytes.to_vec(), false))
        .collect();
    let bytes = serialize_archive_records(records, RpfOutputMode::Aes, Arc::clone(crypto))
        .expect("the archive saves under the keys");
    let magic = u32::from_le_bytes(bytes[12..16].try_into().expect("a header"));
    assert_eq!(
        magic,
        or_io::container::rpf::RPF_AES_MAGIC,
        "the archive is not AES"
    );
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("its folder");
    std::fs::write(path, bytes).expect("the archive");
}

#[test]
fn an_encrypted_archive_in_the_overlay_compares_with_the_games_through_the_keys_the_tree_holds() {
    let crypto = synthetic_provider();
    let root = tempfile::tempdir().expect("a scratch install");
    std::fs::write(root.path().join("gta5.exe"), b"").expect("the marker an install has");
    write_aes(
        &root.path().join("update/update.rpf"),
        &[
            ("kept.txt", b"same"),
            ("edited.dat", b"game"),
            ("gone.txt", b"x"),
        ],
        &crypto,
    );
    write_aes(
        &root.path().join("mods/update/update.rpf"),
        &[
            ("kept.txt", b"same"),
            ("edited.dat", b"the mod's"),
            ("added.txt", b"new"),
        ],
        &crypto,
    );

    let mut state = state();
    let game = state.open_game_root(root.path().to_path_buf());
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(game).map(|d| &mut d.payload) else {
        unreachable!("the game root is an archive document");
    };
    rpf.session.set_crypto(Arc::clone(&crypto));
    // Expanded, so the overlay's archive is one the tree holds open, and the game's is read on the worker.
    node_at(&mut state, game, "mods/update/update.rpf/kept.txt");
    let modded = node_at(&mut state, game, "mods/update/update.rpf");
    run(&mut state, Action::CompareWithOriginal, game, modded);

    let (compared, _) = shown(&state);
    assert_eq!(
        rows(&compared),
        vec![
            row("added.txt", "", Change::Added, "", "3 B"),
            row("edited.dat", "Content", Change::Changed, "4 B", "9 B"),
            row("gone.txt", "", Change::Removed, "1 B", ""),
        ]
    );
}

/// The archives of `update/x64/dlcpacks` a user's install encrypts and keeps small, smallest first, with their path under `root`.
fn small_encrypted_packs(root: &std::path::Path) -> Vec<(u64, String)> {
    const KIB: u64 = 1024;
    const PLAIN: [u32; 2] = [0, 0x4E45_504F];
    let packs = std::fs::read_dir(root.join("update/x64/dlcpacks"))
        .into_iter()
        .flatten();
    let mut found: Vec<(u64, String)> = (packs.flatten())
        .filter_map(|pack| {
            let file = pack.path().join("dlc.rpf");
            let size = std::fs::metadata(&file).ok()?.len();
            if !(100 * KIB..8 * KIB * KIB).contains(&size) {
                return None;
            }
            // Only the 16-byte header: the encryption word is its last four.
            let mut head = [0; 16];
            std::fs::File::open(&file)
                .and_then(|mut file| file.read_exact(&mut head))
                .ok()?;
            let encrypted = !PLAIN.contains(&u32::from_le_bytes([
                head[12], head[13], head[14], head[15],
            ]));
            let name = pack.file_name().to_string_lossy().into_owned();
            encrypted.then(|| (size, format!("update/x64/dlcpacks/{name}/dlc.rpf")))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn an_installs_encrypted_archives_compare_through_the_keys_read_from_its_executable() {
    const WHAT: &str = "an install's encrypted archives compared entry by entry";
    let Some((root, packs)) = install_roots().into_iter().find_map(|root| {
        let packs = small_encrypted_packs(&root);
        (root.join("GTA5.exe").is_file() && packs.len() >= 2).then_some((root, packs))
    }) else {
        skip_install(
            WHAT,
            format_args!(
                "no install with two small encrypted packs; set {GAME_ROOT_ENV} or install the game"
            ),
        );
        return;
    };
    let (first, second) = (&packs[0].1, &packs[1].1);

    // A scratch install that holds copies, so nothing is written to the user's.
    let scratch = tempfile::tempdir().expect("a scratch install");
    std::fs::write(scratch.path().join("gta5.exe"), b"").expect("the marker an install has");
    for (copy, from) in [
        (first.clone(), first),
        (format!("mods/{first}"), first),
        (second.clone(), second),
    ] {
        let to = scratch.path().join(&copy);
        std::fs::create_dir_all(to.parent().expect("a folder")).expect("its folder");
        std::fs::copy(root.join(from), to).expect("a copy of the pack");
    }

    let mut state = with_keys(&root);
    let game = state.open_game_root(scratch.path().to_path_buf());
    let modded = node_at(&mut state, game, &format!("mods/{first}"));
    run(&mut state, Action::CompareWithOriginal, game, modded);
    let (same, _) = shown(&state);
    assert_eq!(rows(&same), Vec::<Row>::new(), "{first} and its own copy");
    assert_eq!(same.summary(), "by entry, no differences");

    let other = node_at(&mut state, game, second);
    run(&mut state, Action::SelectForCompare, game, modded);
    run(&mut state, Action::CompareWithSelected, game, other);
    let (differ, _) = shown(&state);
    let found = rows(&differ);
    assert!(!found.is_empty(), "{first} and {second} hold the same");
    let unread: Vec<_> = found
        .iter()
        .filter(|r| r.3.contains("could not") || r.4.contains("could not"))
        .collect();
    assert!(
        unread.is_empty(),
        "entries the keys could not read: {unread:?}"
    );
}
