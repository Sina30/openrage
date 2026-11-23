//! A comparison reports a difference in a blob a pointer reaches, in a part
//! deeper than the inspector's outline goes and in an entry of an archive; a
//! compare command that cannot act says why; and what the staged files hold
//! that the comparison's limits and blobs assume is measured.

use std::path::{Path, PathBuf};

use or_editor::{Node, Outline, MAX_DEPTH};
use or_io::decl::Gen8;
use or_io::desc::{FieldRepr, Image, Pack};
use or_io::gta5::bounds::build;
use or_io::gta5::bounds::PhBoundAny;
use or_io::gta5::{File as _, GrmGeometry, GrmModel, GtaDrawable, RmcLod, SpdAABB};
use or_test_support::{game_roots, skip_corpus, Corpus};
use or_title::game::Game;
use or_title::install::discovery::{discover_installs, DiscoveredInstall, DiscoverySource};

use super::compare::Change;
use super::comparing_every_resource_and_archive::{compare_files, quad_mesh, row, rows, saved};
use super::comparing_two_files::{command, run, shown, state};
use super::defragmenting_an_archive::{fragmented, settle};
use super::jobs::JobKind;
use super::nested_archives_survive_a_save::archive as archive_bytes;
use super::opening_a_ydd::node_at;
use super::prefs::EditMode;
use super::{document_io, DocumentKind, RetailEditDecision};
use crate::render::bound_meshes::shapes::composite;
use crate::ui::commands::Action;

/// A drawable called `name` whose bound is a quad of the materials `surface` names.
fn drawable_over(name: &str, surface: [u8; 2]) -> Vec<u8> {
    let bound = build::geometry(&quad_mesh(surface, [0, 2, 3])).expect("a quad is a geometry");
    let mut drawable = GtaDrawable::<Gen8>::default();
    drawable.base.name = name.to_string();
    drawable.bound = Some(Box::new(PhBoundAny::PhBoundGeometry(bound)));
    saved(&drawable)
}

#[test]
fn two_drawables_that_differ_in_a_blob_behind_a_bytes_pointer_and_in_a_field_are_reported_in_both()
{
    let (compared, _) = compare_files(
        ("a.ydr", &drawable_over("one", [0, 1])),
        ("b.ydr", &drawable_over("two", [1, 0])),
    );
    assert_eq!(
        rows(&compared),
        vec![
            row("", "Base > Name", Change::Changed, "one", "two"),
            row(
                "Bound",
                "PolyMatIndexList",
                Change::Changed,
                "2 B",
                "2 B, differing from byte 0"
            ),
        ]
    );
}

/// A drawable of one model of one geometry, which draws with the shader `shader`.
fn model_with_shader(shader: u16) -> Vec<u8> {
    let model = GrmModel::<Gen8> {
        geometries: vec![Box::<GrmGeometry<Gen8>>::default()],
        aabbs: vec![SpdAABB::default()],
        shader_indices: vec![shader],
        ..Default::default()
    };
    let mut drawable = GtaDrawable::<Gen8>::default();
    drawable.base.lod_group.lods[0] = RmcLod {
        models: vec![Box::new(model)],
        ..Default::default()
    }
    .into();
    saved(&drawable)
}

#[test]
fn two_models_that_differ_in_the_bytes_of_a_run_with_no_element_type_are_reported() {
    let (compared, _) = compare_files(
        ("a.ydr", &model_with_shader(1)),
        ("b.ydr", &model_with_shader(2)),
    );
    assert_eq!(
        rows(&compared),
        vec![row(
            "Base > LodGroup > Lods[0] > Models[0]",
            "ShaderIndices",
            Change::Changed,
            "2 B",
            "2 B, differing from byte 0"
        )]
    );
}

/// A chain of `levels` composites around one quad whose materials are `surface`.
fn nested(levels: usize, surface: [u8; 2]) -> Vec<u8> {
    let bound = build::geometry(&quad_mesh(surface, [0, 2, 3])).expect("a quad is a geometry");
    let chain = (0..levels).fold(PhBoundAny::PhBoundGeometry(bound), |inside, _| {
        composite(vec![inside])
    });
    chain.to_bytes().expect("a bound saves")
}

/// More levels of parts than the inspector's outline descends, fewer than a comparison reads.
const DEEP: usize = MAX_DEPTH + 4;

#[test]
fn a_difference_in_the_last_part_of_a_bound_nested_past_the_outlines_limit_is_found() {
    let (compared, _) = compare_files(
        ("a.ybn", &nested(DEEP, [0, 1])),
        ("b.ybn", &nested(DEEP, [1, 0])),
    );
    let down = ["Bounds[0]"; DEEP].join(" > ");
    assert_eq!(
        rows(&compared),
        vec![row(
            &down,
            "PolyMatIndexList",
            Change::Changed,
            "2 B",
            "2 B, differing from byte 0"
        )],
        "the quad at the bottom of {DEEP} composites"
    );
    assert_eq!(compared.unread(), None);
}

#[test]
fn two_archives_holding_a_changed_resource_find_the_fields_that_changed_inside_it() {
    use super::comparing_every_resource_and_archive::drawable;
    let same = saved(&drawable("same", 1.0, 1.0));
    let one = |name: &str, distance: f32| saved(&drawable(name, distance, 2.0));
    let left = archive_bytes(&[
        ("models/car.ydr", &one("one", 100.0)),
        ("models/same.ydr", &same),
        ("notes.txt", b"before"),
    ]);
    let right = archive_bytes(&[
        ("models/car.ydr", &one("two", 250.5)),
        ("models/same.ydr", &same),
        ("notes.txt", b"after!"),
    ]);
    let (compared, _) = compare_files(("a.rpf", &left), ("b.rpf", &right));
    assert_eq!(
        rows(&compared),
        vec![
            row(
                "models/car.ydr",
                "Base > LodGroup > LodDistances [0]",
                Change::Changed,
                "100.000",
                "250.500"
            ),
            row(
                "models/car.ydr",
                "Base > Name",
                Change::Changed,
                "one",
                "two"
            ),
            row(
                "notes.txt",
                "Content",
                Change::Changed,
                "6 B",
                "6 B, differing from byte 0"
            ),
        ],
        "the resource by field, the text by its bytes, the same resource not at all"
    );
    assert_eq!(compared.summary(), "by entry, 3 differences");
    assert_eq!(compared.unread(), None);
}

#[test]
fn an_archive_whose_changed_resource_was_not_read_whole_says_so() {
    let run: Vec<u8> = (1..=150).collect();
    let mut edited = run.clone();
    edited[5] = 250;
    let wrap = |flags: &[u8]| {
        archive_bytes(&[(
            "models/long.ydr",
            &super::comparing_every_resource_and_archive::with_models(flags),
        )])
    };
    let (compared, _) = compare_files(("a.rpf", &wrap(&run)), ("b.rpf", &wrap(&edited)));
    assert_eq!(
        rows(&compared),
        vec![row(
            "models/long.ydr > Base > LodGroup > Lods[0] > Models[5]",
            "Flags",
            Change::Changed,
            "6",
            "250"
        )]
    );
    assert_eq!(
        compared.unread(),
        Some(
            "Not read whole, in 1 changed entry: compare one on its own to see what was left out."
        )
    );
}

/// The outline of the resource `name` holds, as the inspector builds it.
fn outline_of(name: &str, bytes: &[u8]) -> Outline {
    let kind = DocumentKind::from_path(Path::new(name)).expect("a kind");
    let image = document_io::resource_image_for(kind, bytes).expect("a resource");
    let format = or_io::gta5::formats::format(kind.extension()).expect("a format");
    let (pack, root) = document_io::described_for(format, &image).expect("a description");
    Outline::of(&pack, &root, image.view(), 0).expect("an outline")
}

/// Where `node` or a part under it is a part above it over again.
fn under_itself(node: &Node, above: &mut Vec<(String, usize)>) -> Option<String> {
    let here = node.type_name.clone().map(|ty| (ty, node.at));
    if here.as_ref().is_some_and(|here| above.contains(here)) {
        return Some(format!("{} at {:#x}", node.label, node.at));
    }
    let entered = here.is_some();
    above.extend(here);
    let found = (node.children.iter()).find_map(|child| under_itself(child, above));
    if entered {
        above.pop();
    }
    found
}

#[test]
fn the_outline_of_a_composite_bound_holds_no_part_under_itself() {
    let bvh = build::bvh(&quad_mesh([0, 1], [0, 2, 3])).expect("a quad is a BVH");
    let geometry = build::geometry(&quad_mesh([0, 1], [0, 2, 3])).expect("a quad is a geometry");
    let bound = composite(vec![
        PhBoundAny::PhBoundBVH(bvh),
        PhBoundAny::PhBoundGeometry(geometry),
    ]);
    let bytes = bound.to_bytes().expect("a bound saves");
    let outline = outline_of("a.ybn", &bytes);
    assert_eq!(under_itself(&outline.root, &mut Vec::new()), None);
    let leaves = |node: &Node| node.children.len();
    assert!(leaves(&outline.root) > 0, "the outline is not empty");
}

#[test]
fn compare_with_selected_is_disabled_with_its_reason_on_an_archive_being_defragmented() {
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
    run(&mut state, Action::SelectForCompare, folder, b);
    let with_selected = |state: &super::AppState, row| {
        command(state, Action::CompareWithSelected, folder, row).enabled
    };
    assert_eq!(with_selected(&state, a), Ok(()), "nothing holds it yet");

    state.defragment_archive(folder, a);
    settle(&mut state);
    assert!(state.defrag.holds(&held), "the size preview is on offer");
    assert_eq!(
        with_selected(&state, a),
        Err("The archive this is read from is being defragmented: compare once that is done")
    );

    // The archive chosen first is the one held: the other row says so of it.
    run_select(&mut state, folder, a);
    assert_eq!(
        with_selected(&state, b),
        Err(
            "The archive the selected file is read from is being defragmented: compare once \
             that is done"
        )
    );

    state.answer_defrag(false);
    assert_eq!(with_selected(&state, b), Ok(()), "released once answered");
}

/// How many comparisons are running, and how many tabs show one.
fn comparisons(state: &super::AppState) -> (usize, usize) {
    let running = (state.running_jobs().iter())
        .filter(|job| job.kind == JobKind::Compare)
        .count();
    let tabs = (state.documents.iter())
        .filter(|d| d.kind == DocumentKind::Compare)
        .count();
    (running, tabs)
}

#[test]
fn compare_with_original_is_disabled_and_refused_while_a_defragment_holds_either_archive() {
    const HELD: &str =
        "The archive this is read from is being defragmented: compare once that is done";
    let root = tempfile::tempdir().expect("a scratch install");
    std::fs::write(root.path().join("gta5.exe"), b"").expect("the marker an install has");
    for place in ["update/update.rpf", "mods/update/update.rpf"] {
        let at = root.path().join(place);
        std::fs::create_dir_all(at.parent().expect("a folder")).expect("its folder");
        std::fs::write(at, fragmented()).expect("the fragmented archive");
    }
    let mut state = state();
    state.prefs.edit_mode = EditMode::Everything;
    let game = state.open_game_root(root.path().to_path_buf());
    let shipped = node_at(&mut state, game, "update/update.rpf");
    let modded = node_at(&mut state, game, "mods/update/update.rpf");
    let with_original =
        |state: &super::AppState| command(state, Action::CompareWithOriginal, game, modded).enabled;
    assert_eq!(with_original(&state), Ok(()), "nothing holds either yet");

    // The game's own copy, which a defragment asks about first, then the overlay's.
    for (held, name) in [(shipped, "the game's"), (modded, "the overlay's")] {
        state.defragment_archive(game, held);
        if state.pending_retail_edit.is_some() {
            state.apply_pending_retail_edit(RetailEditDecision::YesSession);
        }
        settle(&mut state);
        assert!(
            state.defrag.offer.is_some(),
            "{name}: a size preview is on offer"
        );
        assert_eq!(with_original(&state), Err(HELD), "{name}");

        // A menu drawn before the defragment began still has its click.
        state.toasts.clear();
        state.compare_with_original(game, modded);
        assert_eq!(comparisons(&state), (0, 0), "{name}: a comparison began");
        let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
        assert!(
            said.iter()
                .any(|said| said.contains("is being defragmented")),
            "{name}: {said:?}"
        );

        state.answer_defrag(false);
        assert_eq!(
            with_original(&state),
            Ok(()),
            "{name}: released once answered"
        );
    }
    run(&mut state, Action::CompareWithOriginal, game, modded);
    let (compared, _) = shown(&state);
    assert_eq!(compared.summary(), "by entry, no differences");
}

/// Select for compare on `row`, which a defragment may hold.
fn run_select(state: &mut super::AppState, doc: super::DocumentId, row: super::RpfNodeId) {
    run(state, Action::SelectForCompare, doc, row);
}

/// The staged files of `format`, at most a dozen, from the smallest to the largest at even steps in their size.
fn sampled(format: &or_io::gta5::formats::Format) -> Vec<(String, Vec<u8>)> {
    let mut paths = Corpus::new(env!("CARGO_MANIFEST_DIR"), format.ext)
        .all_targets()
        .find();
    paths.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    let last = paths.len().saturating_sub(1);
    let picked: Vec<usize> = match paths.len() {
        0..=12 => (0..paths.len()).collect(),
        n => (0..12).map(|k| k * (n - 1) / 11).collect(),
    };
    debug_assert!(picked.iter().all(|&at| at <= last));
    let read = |&at: &usize| {
        let name = paths[at].file_name()?.to_string_lossy().into_owned();
        Some((name, std::fs::read(&paths[at]).ok()?))
    };
    picked.iter().filter_map(read).collect()
}

/// The pack and image of the resource `name`, with the outline `depth` levels deep.
fn staged_outline(
    format: &or_io::gta5::formats::Format,
    name: &str,
    bytes: &[u8],
    depth: usize,
) -> Option<(
    std::sync::Arc<Pack>,
    std::sync::Arc<super::ResourceImage>,
    Outline,
)> {
    let kind = DocumentKind::from_path(Path::new(name))?;
    let image = document_io::resource_image_for(kind, bytes)?;
    let (pack, root) = document_io::described_for(format, &image)?;
    let outline = Outline::of_to(&pack, &root, image.view(), 0, depth)?;
    Some((pack, image, outline))
}

/// How many parts of `node` and under it lie where a limit left what they hold unread.
fn cuts(node: &Node) -> usize {
    usize::from(node.cut == Some(or_editor::Cut::Depth))
        + node.children.iter().map(cuts).sum::<usize>()
}

#[test]
fn a_comparison_reads_every_staged_file_to_its_last_part() {
    const WHAT: &str = "a staged file read to its last part";
    let (mut read, mut cut_at_the_inspectors, mut cut_here) = (0, 0, Vec::new());
    for format in or_io::gta5::formats::FORMATS {
        for (name, bytes) in sampled(format) {
            let outline = |depth| staged_outline(format, &name, &bytes, depth);
            let (Some((.., inspectors)), Some((.., here))) =
                (outline(MAX_DEPTH), outline(super::compare::DEPTH))
            else {
                continue;
            };
            read += 1;
            cut_at_the_inspectors += usize::from(cuts(&inspectors.root) > 0);
            if cuts(&here.root) > 0 {
                cut_here.push(name);
            }
        }
    }
    if read == 0 {
        skip_corpus(WHAT, "no resource staged");
        return;
    }
    eprintln!(
        "{cut_at_the_inspectors} of {read} sampled staged files have a part at the inspector's \
         depth of {MAX_DEPTH} that holds more"
    );
    assert!(
        cut_here.is_empty(),
        "parts past a comparison's depth of {}: {cut_here:?}",
        super::compare::DEPTH
    );
}

/// How many untyped trailing runs `node` and the parts under it hold, and how many start in the physical segment.
fn untyped_trailing_runs(pack: &Pack, image: Image<'_>, node: &Node) -> (usize, usize) {
    let own = (node
        .type_name
        .as_deref()
        .and_then(|ty| pack.type_by_name(ty)))
    .map(|ty| {
        let runs = ty.fields.iter().filter(|field| {
            matches!(
                field.repr,
                FieldRepr::Trailing {
                    type_name: None,
                    ..
                }
            )
        });
        let start = |offset: usize| node.at + offset;
        let all = runs.clone().count();
        let physical = runs
            .filter(|f| start(f.offset) >= image.virtual_size())
            .count();
        (all, physical)
    });
    let (mut all, mut physical) = own.unwrap_or_default();
    for child in &node.children {
        let (a, p) = untyped_trailing_runs(pack, image, child);
        (all, physical) = (all + a, physical + p);
    }
    (all, physical)
}

#[test]
fn no_staged_file_holds_an_untyped_trailing_run_in_the_physical_segment() {
    const WHAT: &str = "an untyped trailing run in the physical segment";
    let (mut files, mut runs, mut physical, mut where_) = (0, 0, 0, Vec::new());
    for format in or_io::gta5::formats::FORMATS {
        for (name, bytes) in sampled(format) {
            let Some((pack, image, outline)) =
                staged_outline(format, &name, &bytes, super::compare::DEPTH)
            else {
                continue;
            };
            files += 1;
            let (all, past) = untyped_trailing_runs(&pack, image.view(), &outline.root);
            runs += all;
            physical += past;
            if past > 0 {
                where_.push(name);
            }
        }
    }
    if files == 0 {
        skip_corpus(WHAT, "no resource staged");
        return;
    }
    eprintln!("{physical} of {runs} untyped trailing runs in {files} sampled staged files start in the physical segment");
    assert!(
        runs > 0,
        "no untyped trailing run was seen: the measure reads nothing"
    );
    assert_eq!(physical, 0, "{where_:?}");
}

/// The install roots a test reads: those `told` names, else those `found` says the app finds.
pub(super) fn install_roots_told_else_found(
    told: Vec<PathBuf>,
    found: impl FnOnce() -> Vec<DiscoveredInstall>,
) -> Vec<PathBuf> {
    if !told.is_empty() {
        return told;
    }
    let gta5 = |install: &DiscoveredInstall| {
        install.game == Game::GtaVLegacy || install.game == Game::GtaVEnhanced
    };
    (found().into_iter().filter(gta5))
        .map(|install| install.install_dir)
        .collect()
}

/// The installs an install-gated test reads, `OPENRAGE_GAME_ROOT` or none named and then the app's own discovery.
pub(super) fn install_roots() -> Vec<PathBuf> {
    install_roots_told_else_found(game_roots(), discover_installs)
}

#[test]
fn an_install_the_app_finds_is_read_where_none_is_named_and_a_named_one_is_read_alone() {
    let dir = tempfile::tempdir().expect("a scratch install");
    // Both titles' executables, so the filter is all that keeps the other title out.
    for exe in ["GTA5.exe", "RDR2.exe"] {
        std::fs::write(dir.path().join(exe), b"").expect("the executable");
    }
    let found = |game| {
        let source = DiscoverySource::UserConfigured;
        DiscoveredInstall::probe(game, dir.path().to_path_buf(), source)
    };
    let looked = || found(Game::GtaVLegacy).into_iter().collect::<Vec<_>>();
    assert_eq!(
        install_roots_told_else_found(Vec::new(), looked),
        vec![dir.path().to_path_buf()],
        "no root is named, so the one the app finds is read"
    );
    let named = vec![PathBuf::from("named")];
    let never = || -> Vec<DiscoveredInstall> { panic!("a named root is read without looking") };
    assert_eq!(
        install_roots_told_else_found(named.clone(), never),
        named,
        "a root that is named is the only one read"
    );
    let other_title = || found(Game::Rdr2).into_iter().collect::<Vec<_>>();
    assert_eq!(other_title().len(), 1, "the other title is found here");
    assert_eq!(
        install_roots_told_else_found(Vec::new(), other_title),
        Vec::<PathBuf>::new(),
        "a title this is not about is not read"
    );
}
