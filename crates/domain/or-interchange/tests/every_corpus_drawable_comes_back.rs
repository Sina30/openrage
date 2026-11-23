//! Every staged drawable of both targets exported, imported, saved, and held to what survived.
//!
//! Real geometries use `PackedNormal`, `Half2`, `Short2Unorm` and thirteen
//! other encodings, skip `Tex0`, carry tangents without normals and ship
//! palettes of one bone, which the synthetic `a_mesh_comes_back.rs` never
//! builds. One file is resident at a time.

use or_interchange::extract::Extracted;
use or_interchange::gltf::{self, Container};
use or_interchange::import::Imported;
use or_interchange::{drawable_scene, replace_geometry, ExtractOptions, ImportOptions};
use or_io::codec::mesh::{GrcFvfChannel, VertexReader};
use or_io::decl::{Gen8, Gen9};
use or_io::gta5::{
    DrawableTarget, File, GrmGeometry, GtaDrawable, GtaFragType, PgDictionary, RmcDrawable, SpdAABB,
};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8, TARGET_GEN9};

/// How many files of each extension this reads.
const CAP: usize = 80;

/// Every geometry of every level, in walk order.
fn geometries<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<&GrmGeometry<T>> {
    drawable
        .lod_group
        .lods
        .iter()
        .flat_map(|lod| lod.models.iter())
        .flat_map(|model| model.geometries.iter().map(|g| &**g))
        .collect()
}

/// Every geometry's position run, in walk order.
fn positions<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<Vec<[f32; 3]>> {
    geometries(drawable)
        .into_iter()
        .map(|g| {
            VertexReader::new(&*g.vertex_buffer)
                .map(|r| r.positions().collect())
                .unwrap_or_default()
        })
        .collect()
}

/// Every geometry's index run.
fn indices<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<Vec<u16>> {
    geometries(drawable)
        .into_iter()
        .map(|g| T::indices(&g.index_buffer).to_vec())
        .collect()
}

/// The skeleton bone each vertex's first weighted lane resolves to.
fn bones<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    for geometry in geometries(drawable) {
        let mut run = Vec::new();
        if let Some(reader) = VertexReader::new(&*geometry.vertex_buffer) {
            if reader.layout().has(GrcFvfChannel::Binding) {
                for vertex in 0..reader.count() {
                    let weights = or_interchange::extract::channel_lanes(
                        &reader,
                        GrcFvfChannel::Weight,
                        vertex,
                    );
                    let slots = or_interchange::extract::channel_lanes(
                        &reader,
                        GrcFvfChannel::Binding,
                        vertex,
                    );
                    // The lane with the most weight is the one whose
                    // bone a quantisation cannot reassign.
                    let lane = (0..4).max_by_key(|l| weights[*l]).unwrap_or(0);
                    run.push(
                        geometry
                            .matrix_palette
                            .get(usize::from(slots[lane]))
                            .copied()
                            .unwrap_or(u16::MAX),
                    );
                }
            }
        }
        out.push(run);
    }
    out
}

/// Export a drawable, read it straight back, and put it into a clone.
///
/// Returns `None` where the export carried no mesh, which is not a failure:
/// a drawable of empty geometries has nothing to round-trip.
fn round_trip<T: Extracted + Imported>(
    drawable: &RmcDrawable<T>,
) -> Option<Result<RmcDrawable<T>, String>> {
    let exported = drawable_scene(
        "subject",
        drawable,
        &ExtractOptions {
            // Decoding BCn for a geometry comparison is minutes for nothing.
            textures: false,
            ..ExtractOptions::default()
        },
    );
    if exported.objects.iter().all(|o| o.meshes.is_empty()) {
        return None;
    }
    let files = gltf::write(&exported, "subject", Container::Glb);
    let scene = match gltf::read::load(std::path::Path::new("subject.glb"), &files.primary) {
        Ok(scene) => scene,
        Err(e) => {
            return Some(Err(format!(
                "the reader refused the writer's own output: {e}"
            )))
        }
    };
    Some(
        replace_geometry(
            drawable,
            &scene,
            &ImportOptions {
                // A ped component is skinned against a skeleton in another
                // file, so glTF cannot carry its influences: 28 of the
                // corpus's 416 drawables.
                keep_skinning: true,
                ..ImportOptions::default()
            },
        )
        .map(|(built, _)| built)
        .map_err(|e| e.to_string()),
    )
}

#[derive(Default)]
struct Tally {
    drawables: usize,
    round_tripped: usize,
    /// A drawable the export declined to carry, which is not a defect.
    no_mesh: usize,
    /// A drawable the import refused, with the first few reasons.
    refused: usize,
    reasons: Vec<String>,
    geometries: usize,
    vertices: usize,
    /// Files saved with every imported drawable in place and read back.
    saved: usize,
}

/// How many refusal reasons the tally spells.
const REASONS: usize = 6;

/// The drawable an import built, held to the one it replaced.
fn check<T: Extracted + Imported>(
    tally: &mut Tally,
    label: &str,
    drawable: &RmcDrawable<T>,
) -> Option<RmcDrawable<T>> {
    tally.drawables += 1;
    let Some(outcome) = round_trip(drawable) else {
        tally.no_mesh += 1;
        return None;
    };
    let after = match outcome {
        Ok(after) => after,
        Err(reason) => {
            tally.refused += 1;
            if tally.reasons.len() < REASONS {
                tally.reasons.push(format!("{label}: {reason}"));
            }
            return None;
        }
    };
    tally.round_tripped += 1;

    let before_positions = positions(drawable);
    let after_positions = positions(&after);
    assert_eq!(
        after_positions.len(),
        before_positions.len(),
        "{label}: the geometry count changed"
    );
    for (slot, (before, after)) in before_positions
        .iter()
        .zip(after_positions.iter())
        .enumerate()
    {
        assert_eq!(
            after.len(),
            before.len(),
            "{label}: geometry {slot} changed vertex count"
        );
        tally.geometries += 1;
        tally.vertices += before.len();
        for (vertex, (was, is)) in before.iter().zip(after.iter()).enumerate() {
            for axis in 0..3 {
                // ⚠ A tolerance rather than equality, because a position
                // channel is not always `Float3`: `Half3` and `Short2Unorm`
                // both occur, and a value that left one comes back quantised
                // to the same one. A double-applied basis is a quarter turn
                // and shows up far above this.
                let scale = was[axis].abs().max(1.0);
                assert!(
                    (was[axis] - is[axis]).abs() <= 1e-3 * scale,
                    "{label}: geometry {slot} vertex {vertex} axis {axis} was {} and is {}",
                    was[axis],
                    is[axis]
                );
            }
        }
    }

    assert_eq!(
        indices(&after),
        indices(drawable),
        "{label}: the winding or the index run changed, and the corpus says neither should"
    );
    assert_eq!(
        bones(&after),
        bones(drawable),
        "{label}: a vertex's heaviest bone changed, so the palette was rebuilt wrongly"
    );

    // The drawable's own box is authored, so an import must not move it.
    //
    // ⚠ **Compared by bits, not by value.** A shipped drawable box carries
    // `0x7F800001` in both `w` lanes, which is a NaN, and `==` on a NaN is
    // false however unchanged it is — so a value comparison here fails on
    // every real file and says the box moved when nothing touched it.
    assert_eq!(
        box_bits(&after.lod_group.bounds_box),
        box_bits(&drawable.lod_group.bounds_box),
        "{label}: the authored drawable box moved"
    );
    Some(after)
}

/// A file with its imported drawables in place saves, and reads back with their meshes.
fn saves<F: File>(tally: &mut Tally, label: &str, file: &F, meshes: impl Fn(&F) -> Vec<Mesh>) {
    let bytes = file
        .to_bytes()
        .unwrap_or_else(|e| panic!("{label}: the imported file would not save: {e}"));
    let reopened =
        F::from_bytes(&bytes).unwrap_or_else(|e| panic!("{label}: the save would not reopen: {e}"));
    assert!(
        meshes(&reopened) == meshes(file),
        "{label}: the saved file reads back with other meshes than were imported"
    );
    tally.saved += 1;
}

/// A drawable's position and index runs.
type Mesh = (Vec<Vec<[f32; 3]>>, Vec<Vec<u16>>);

fn mesh<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Mesh {
    (positions(drawable), indices(drawable))
}

fn box_bits(box_: &SpdAABB) -> [u32; 8] {
    [
        box_.min.x.to_bits(),
        box_.min.y.to_bits(),
        box_.min.z.to_bits(),
        box_.min.w.to_bits(),
        box_.max.x.to_bits(),
        box_.max.y.to_bits(),
        box_.max.z.to_bits(),
        box_.max.w.to_bits(),
    ]
}

fn report(what: &str, tally: &Tally) {
    eprintln!(
        "{what}: {} drawables, {} round-tripped, {} carried no mesh, {} refused; \
         {} geometries and {} vertices compared; {} files saved and reopened",
        tally.drawables,
        tally.round_tripped,
        tally.no_mesh,
        tally.refused,
        tally.geometries,
        tally.vertices,
        tally.saved
    );
    for reason in &tally.reasons {
        eprintln!("  refused: {reason}");
    }
    assert!(
        tally.round_tripped > 0 && tally.vertices > 0 && tally.saved > 0,
        "{what} compared nothing, so it asserted nothing"
    );
    // ⚠ Every staged drawable must survive. A refusal here is either a real
    // shape the importer cannot take — which belongs in the documentation
    // rather than in a silent skip — or a defect.
    assert_eq!(
        tally.refused, 0,
        "{what}: {} shipped drawables were refused by the importer",
        tally.refused
    );
}

fn corpus(ext: &str, target: &str, what: &str) -> Option<Vec<std::path::PathBuf>> {
    Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), ext)
        .target(target)
        .limit(CAP)
        .find_or_skip(what)
}

fn stem(path: &std::path::Path) -> String {
    path.file_stem().unwrap().to_string_lossy().to_string()
}

fn every_ydr<T: Extracted + Imported>(target: &str, what: &str)
where
    GtaDrawable<T>: File,
{
    let Some(files) = corpus("ydr", target, what) else {
        return;
    };

    let mut tally = Tally::default();
    for path in files {
        let bytes = std::fs::read(&path).expect("a staged fixture reads");
        let Ok(mut document) = GtaDrawable::<T>::from_bytes(&bytes) else {
            continue;
        };
        let label = stem(&path);
        if let Some(after) = check(&mut tally, &label, &document) {
            document.base = after;
            saves(&mut tally, &label, &document, |d: &GtaDrawable<T>| {
                vec![mesh(d)]
            });
        }
    }
    report(what, &tally);
}

fn every_ydd<T: Extracted + Imported>(target: &str, what: &str)
where
    PgDictionary<GtaDrawable<T>>: File,
{
    let Some(files) = corpus("ydd", target, what) else {
        return;
    };

    let mut tally = Tally::default();
    for path in files {
        let bytes = std::fs::read(&path).expect("a staged fixture reads");
        let Ok(mut dictionary) = PgDictionary::<GtaDrawable<T>>::from_bytes(&bytes) else {
            continue;
        };
        let stem = stem(&path);
        let mut imported = false;
        for (index, entry) in dictionary.entries.iter_mut().enumerate() {
            let Some(entry) = entry.as_deref_mut() else {
                continue;
            };
            if let Some(after) = check(&mut tally, &format!("{stem}[{index}]"), entry) {
                entry.base = after;
                imported = true;
            }
        }
        if imported {
            saves(&mut tally, &stem, &dictionary, |d| {
                d.entries.iter().flatten().map(|e| mesh(e)).collect()
            });
        }
    }
    report(what, &tally);
}

fn every_yft<T: Extracted + Imported>(target: &str, what: &str)
where
    GtaFragType<T>: File,
{
    let Some(files) = corpus("yft", target, what) else {
        return;
    };

    let mut tally = Tally::default();
    for path in files {
        let bytes = std::fs::read(&path).expect("a staged fixture reads");
        let Ok(mut fragment) = GtaFragType::<T>::from_bytes(&bytes) else {
            continue;
        };
        let stem = stem(&path);
        let mut imported = false;
        let fragment_base = &mut fragment.base;
        for (index, drawable) in fragment_base
            .common_drawable
            .iter_mut()
            .chain(fragment_base.extra_drawables.iter_mut())
            .enumerate()
        {
            if let Some(after) = check(&mut tally, &format!("{stem}[{index}]"), drawable) {
                drawable.base = after;
                imported = true;
            }
        }
        if imported {
            saves(&mut tally, &stem, &fragment, |f: &GtaFragType<T>| {
                f.common_drawable
                    .iter()
                    .chain(f.extra_drawables.iter())
                    .map(|d| mesh(d))
                    .collect()
            });
        }
    }
    report(what, &tally);
}

#[test]
fn every_corpus_ydr_survives_the_round_trip() {
    every_ydr::<Gen8>(TARGET_GEN8, "every_corpus_ydr_survives_the_round_trip");
}

#[test]
fn every_corpus_ydd_entry_survives_the_round_trip() {
    every_ydd::<Gen8>(
        TARGET_GEN8,
        "every_corpus_ydd_entry_survives_the_round_trip",
    );
}

#[test]
fn every_corpus_yft_drawable_survives_the_round_trip() {
    every_yft::<Gen8>(
        TARGET_GEN8,
        "every_corpus_yft_drawable_survives_the_round_trip",
    );
}

#[test]
fn every_enhanced_ydr_survives_the_round_trip() {
    every_ydr::<Gen9>(TARGET_GEN9, "every_enhanced_ydr_survives_the_round_trip");
}

#[test]
fn every_enhanced_ydd_entry_survives_the_round_trip() {
    every_ydd::<Gen9>(
        TARGET_GEN9,
        "every_enhanced_ydd_entry_survives_the_round_trip",
    );
}

#[test]
fn every_enhanced_yft_drawable_survives_the_round_trip() {
    every_yft::<Gen9>(
        TARGET_GEN9,
        "every_enhanced_yft_drawable_survives_the_round_trip",
    );
}

/// ⚠ A round trip of unchanged geometry passes when the import writes nothing, so this one moves it.
#[test]
fn an_enhanced_edit_lands_in_the_saved_file() {
    let what = "an_enhanced_edit_lands_in_the_saved_file";
    let Some(files) = corpus("ydr", TARGET_GEN9, what) else {
        return;
    };
    let path = &files[0];
    let original = std::fs::read(path).expect("a staged fixture reads");
    let before = GtaDrawable::<Gen9>::from_bytes(&original).expect("the fixture opens");

    let mut scene = drawable_scene(
        "subject",
        &before.base,
        &ExtractOptions {
            textures: false,
            ..ExtractOptions::default()
        },
    );
    const LIFT: f32 = 2.0;
    for object in &mut scene.objects {
        for mesh in &mut object.meshes {
            for primitive in &mut mesh.primitives {
                for position in &mut primitive.positions {
                    position[2] += LIFT;
                }
            }
        }
    }

    let (bytes, _) = or_interchange::replace_in_file::<Gen9>(
        &original,
        or_interchange::Destination::Ydr,
        &scene,
        &ImportOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{}: the import was refused: {e}", path.display()));
    let after = GtaDrawable::<Gen9>::from_bytes(&bytes).expect("the import's save reopens");

    // Only the highest level was exported, so only it moved.
    let moved = |d: &RmcDrawable<Gen9>| positions(d)[..scene.primitives().count()].to_vec();
    let (was, is) = (moved(&before.base), moved(&after.base));
    assert_eq!(is.len(), was.len());
    for (was, is) in was.iter().flatten().zip(is.iter().flatten()) {
        assert_eq!(*is, [was[0], was[1], was[2] + LIFT], "{}", path.display());
    }
    assert_eq!(indices(&after.base), indices(&before.base));
}
