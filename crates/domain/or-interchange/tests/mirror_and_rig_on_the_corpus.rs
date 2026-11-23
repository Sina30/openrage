//! The staged ped components mirror twice to themselves, their rigs are mended where a group is renamed, and cleaning their weights is done once and for all.

use or_interchange::doctor::mirror::mirror_drawable;
use or_interchange::doctor::rig::{self, Defect};
use or_interchange::extract::{bones_of, Extracted};
use or_interchange::import::Imported;
use or_interchange::{
    drawable_scene, Bone, ExtractOptions, LodSelection, Primitive, Scene, SceneBuilder,
};
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{GtaDrawable, GtaFragType, PgDictionary, RmcDrawable};
use or_test_support::{Corpus, TARGET_GEN8, TARGET_GEN9};

fn scene_with<T: Extracted>(drawable: &RmcDrawable<T>, skeleton: Option<&[Bone]>) -> Scene {
    let mut builder = SceneBuilder::new("d");
    if let Some(bones) = skeleton {
        builder = builder.with_reference_skeleton(bones.to_vec());
    }
    builder.push(
        "d",
        drawable,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    builder.finish()
}

/// The staged skeleton with the most bones, which every ped component's palette fits.
fn largest_skeleton<T>(target: &str) -> Option<Vec<Bone>>
where
    T: or_io::gta5::DrawableTarget,
    GtaFragType<T>: File,
{
    Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft")
        .target(target)
        .find()
        .iter()
        .filter_map(|path| {
            let fragment = GtaFragType::<T>::from_bytes(&std::fs::read(path).ok()?).ok()?;
            fragment
                .drawables()
                .into_iter()
                .find_map(|d| d.base.skeleton.as_deref().map(bones_of))
        })
        .max_by_key(Vec::len)
        // A ped's skeleton has hundreds of bones; a vehicle's or a prop's is not one a palette fits.
        .filter(|bones| bones.len() >= 100)
}

fn same(a: &Primitive, b: &Primitive) -> bool {
    difference(a, b).is_none()
}

/// What differs between two meshes, if anything.
fn difference(a: &Primitive, b: &Primitive) -> Option<String> {
    // A normal is scaled to unit length on its way out of a drawable, which may move its last bit.
    let close = |p: &[f32], q: &[f32]| {
        p.len() == q.len() && p.iter().zip(q).all(|(x, y)| (x - y).abs() <= 2e-6)
    };
    let flat = |v: &[[f32; 3]]| v.iter().flatten().copied().collect::<Vec<f32>>();
    let flat4 = |v: &[[f32; 4]]| v.iter().flatten().copied().collect::<Vec<f32>>();
    let fields: [(&str, bool); 6] = [
        ("positions", a.positions == b.positions),
        ("normals", close(&flat(&a.normals), &flat(&b.normals))),
        ("tangents", close(&flat4(&a.tangents), &flat4(&b.tangents))),
        ("indices", a.indices == b.indices),
        ("joints", a.joints == b.joints),
        ("weights", a.weights == b.weights),
    ];
    let wrong: Vec<&str> = fields.iter().filter(|f| !f.1).map(|f| f.0).collect();
    (!wrong.is_empty()).then(|| wrong.join(", "))
}

#[derive(Default)]
struct Tally {
    mirrored: usize,
    mended: usize,
    cleaned: usize,
    skipped: usize,
}

fn go<T: Imported>(
    label: &str,
    drawable: &RmcDrawable<T>,
    reference: Option<&[Bone]>,
    tally: &mut Tally,
) {
    let own = drawable.skeleton.is_some();
    if own || reference.is_some() {
        let scene = scene_with(drawable, reference);
        if scene.primitives().any(|p| p.is_skinned()) || scene.primitives().next().is_some() {
            // Mirror twice: what comes back is what went.
            match mirror_drawable(drawable, reference) {
                Ok((once, report)) => {
                    let (twice, _) = mirror_drawable(&once, reference)
                        .unwrap_or_else(|e| panic!("{label}: would not mirror back: {e}"));
                    let (a, b, c) = (
                        scene,
                        scene_with(&once, reference),
                        scene_with(&twice, reference),
                    );
                    assert_eq!(a.primitives().count(), c.primitives().count());
                    for (x, z) in a.primitives().zip(c.primitives()) {
                        assert!(
                            same(x, z),
                            "{label}: mirroring twice changed {}",
                            difference(x, z).unwrap_or_default()
                        );
                    }
                    for (x, y) in a.primitives().zip(b.primitives()) {
                        if x.positions.iter().any(|p| p[0] != 0.0) {
                            assert_ne!(x.positions, y.positions, "{label}: nothing moved");
                        }
                        assert_eq!(x.triangle_count(), y.triangle_count());
                    }
                    assert!(report.vertices > 0);
                    tally.mirrored += 1;
                }
                // A skin that has a side and no partner is refused by name, which is the answer for a part of the rig that is one-sided.
                Err(e) => {
                    assert!(
                        e.to_string().contains("has a side and no bone opposite"),
                        "{label}: {e}"
                    );
                    tally.skipped += 1;
                }
            }
        }
    }

    // Rename the bone most vertices are weighted to `Group.001`: an unnamed group, which a rig repair gives back to the bone above it.
    let scene = scene_with(drawable, reference);
    let Some(object) = scene.objects.iter().find(|o| !o.bones.is_empty()) else {
        return;
    };
    let skeleton: Vec<String> = object.bones.iter().map(|b| b.name.clone()).collect();
    let mut uses = vec![0usize; skeleton.len()];
    for p in scene.primitives().filter(|p| p.is_skinned()) {
        for (j, w) in p.joints.iter().zip(&p.weights) {
            for l in (0..4).filter(|l| w[*l] > 0.0) {
                if let Some(u) = uses.get_mut(usize::from(j[l])) {
                    *u += 1;
                }
            }
        }
    }
    let Some((used, _)) = uses
        .iter()
        .enumerate()
        .filter(|(i, n)| **n > 0 && object.bones[*i].parent.is_some())
        .max_by_key(|(_, n)| **n)
    else {
        return;
    };
    assert!(
        rig::check(&scene, &skeleton)
            .iter()
            .all(|f| !matches!(f.defect, Defect::UnnamedGroup | Defect::UnknownBone)),
        "{label}: a shipped rig has a group no skeleton knows: {:?} ({} bones)",
        rig::check(&scene, &skeleton),
        skeleton.len()
    );
    let mut broken = scene.clone();
    broken.objects[0].bones[used].name = "Group.001".to_string();
    let found = rig::check(&broken, &skeleton);
    assert!(
        found.iter().any(|f| f.defect == Defect::UnnamedGroup),
        "{label}: the group was not found"
    );
    let said = rig::repair(&mut broken, &skeleton);
    assert!(!said.is_empty());
    let left = rig::check(&broken, &skeleton);
    assert!(
        left.iter()
            .all(|f| !matches!(f.defect, Defect::UnnamedGroup | Defect::UnknownBone)),
        "{label}: {left:?}"
    );
    // The weights of every vertex still add up, and none rests on the renamed group.
    for p in broken.primitives().filter(|p| p.is_skinned()) {
        for (j, w) in p.joints.iter().zip(&p.weights) {
            assert!(
                (w.iter().sum::<f32>() - 1.0).abs() < 2e-3,
                "{label}: weights add to {}",
                w.iter().sum::<f32>()
            );
            assert!(
                (0..4).all(|l| w[l] == 0.0 || usize::from(j[l]) != used),
                "{label}: a weight stayed on the group"
            );
        }
    }
    tally.mended += 1;

    // Cleaning the weights of a shipped primitive and then cleaning again does nothing the second time.
    let mut again = scene.clone();
    for primitive in again
        .objects
        .iter_mut()
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
    {
        rig::clean_weights(primitive, rig::FAINT);
        assert!(
            rig::clean_weights(primitive, rig::FAINT).is_empty(),
            "{label}: a second cleaning found more"
        );
    }
    tally.cleaned += 1;
    let _ = drawable_scene::<T>;
}

fn every<T: Imported>(files: &[std::path::PathBuf], reference: Option<&[Bone]>, tally: &mut Tally)
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    for path in files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        // Ped components only: the placeholder dictionaries of the corpus are not skinned to a ped.
        if ![
            "accs_", "hair_", "hand_", "head_", "jbib_", "lowr_", "task_", "uppr_", "p_", "feet_",
            "berd_", "decl_", "teef_",
        ]
        .iter()
        .any(|p| name.starts_with(p))
        {
            continue;
        }
        let Ok(dictionary) = PgDictionary::<GtaDrawable<T>>::from_bytes(&bytes) else {
            continue;
        };
        for (slot, entry) in dictionary.entries.iter().enumerate() {
            if let Some(entry) = entry.as_deref() {
                go(&format!("{name}#{slot}"), &entry.base, reference, tally);
            }
        }
    }
}

#[test]
fn the_ped_components_of_the_corpus_mirror_twice_to_themselves_and_their_renamed_groups_are_mended()
{
    let Some(g8) = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find_or_skip("ped components of Legacy")
    else {
        return;
    };
    let reference8 = largest_skeleton::<Gen8>(TARGET_GEN8);
    let mut tally = Tally::default();
    every::<Gen8>(&g8, reference8.as_deref(), &mut tally);
    if let Some(g9) = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN9)
        .find_or_skip("ped components of Enhanced")
    {
        // No staged Enhanced file carries a ped's skeleton, so the Legacy one names the bones: the pairing only needs their names.
        let reference9 = largest_skeleton::<Gen9>(TARGET_GEN9).or(reference8);
        every::<Gen9>(&g9, reference9.as_deref(), &mut tally);
    }
    println!(
        "{} mirrored twice, {} one-sided and refused, {} renamed groups mended, {} cleaned twice",
        tally.mirrored, tally.skipped, tally.mended, tally.cleaned
    );
    assert!(tally.mirrored > 0, "no ped component was mirrored");
    assert!(tally.mended > 0, "no rig was mended");
}

#[test]
fn the_freemode_skeleton_pairs_its_left_bones_with_its_right_ones() {
    use or_interchange::doctor::mirror::BoneMirror;
    let Some(reference) = largest_skeleton::<Gen8>(TARGET_GEN8) else {
        or_test_support::skip_corpus(
            "a staged ped skeleton",
            "no staged .yft holds a ped's skeleton".to_string(),
        );
        return;
    };
    let pairs = BoneMirror::of(&reference);
    let unpaired: Vec<&str> = reference
        .iter()
        .enumerate()
        .filter(|(i, _)| pairs.counterpart(*i as u16).is_none())
        .map(|(_, b)| b.name.as_str())
        .collect();
    println!(
        "{} bones, {} pairs, unpaired: {unpaired:?}",
        reference.len(),
        pairs.pairs()
    );
    // Every side has its other: the freemode skeleton is symmetric.
    assert!(pairs.pairs() > 100, "{} pairs", pairs.pairs());
    assert!(unpaired.is_empty(), "{unpaired:?}");
    // The involution holds on all of it.
    for i in 0..reference.len() as u16 {
        let other = pairs.counterpart(i).unwrap();
        assert_eq!(pairs.counterpart(other), Some(i));
    }
}
