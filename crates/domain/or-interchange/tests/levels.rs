//! A level of detail the drawable lacks is made from its highest: the same models, shaders and layouts, fewer triangles, the high level untouched and the skin kept.

use or_interchange::doctor::levels::{add_level, copies, missing, LevelRequest};
use or_interchange::extract::Extracted;
use or_interchange::import::Imported;
use or_interchange::{drawable_scene, ExtractOptions, LodSelection, LodSlot, Primitive};
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{GtaDrawable, PgDictionary, RmcDrawable};
use or_test_support::{Corpus, TARGET_GEN8, TARGET_GEN9};

/// One weight lane is a byte, so a skin that went through one encoding agrees with its source to within a byte.
const WEIGHT_SLACK: f32 = 1.5 / 255.0;

fn scene_of<T: Extracted>(drawable: &RmcDrawable<T>) -> or_interchange::Scene {
    drawable_scene(
        "d",
        drawable,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    )
}

fn level_of(scene: &or_interchange::Scene, level: u8) -> Vec<&Primitive> {
    scene
        .primitives()
        .filter(|p| p.lod == LodSlot::Level(level))
        .collect()
}

fn same_skin(a: &Primitive, i: usize, b: &Primitive, j: usize) -> bool {
    // The lanes that carry a weight, by bone; a lane with none names no bone.
    let lanes = |p: &Primitive, v: usize| -> Vec<(u16, f32)> {
        let mut lanes: Vec<(u16, f32)> = (0..4)
            .filter(|k| p.weights[v][*k] > WEIGHT_SLACK)
            .map(|k| (p.joints[v][k], p.weights[v][k]))
            .collect();
        lanes.sort_by_key(|l| l.0);
        lanes
    };
    let (x, y) = (lanes(a, i), lanes(b, j));
    x.len() == y.len()
        && x.iter()
            .zip(&y)
            .all(|(p, q)| p.0 == q.0 && (p.1 - q.1).abs() <= WEIGHT_SLACK)
}

/// Whether vertex `v` of `kept` is a vertex of one of `from`, in position and skin.
fn comes_from(kept: &Primitive, v: usize, from: &[&Primitive]) -> bool {
    from.iter().any(|source| {
        (0..source.positions.len()).any(|s| {
            kept.positions[v] == source.positions[s]
                && (kept.joints.is_empty() || same_skin(kept, v, source, s))
        })
    })
}

#[derive(Default)]
struct Tally {
    drawables: usize,
    levels: usize,
    skinned: usize,
    triangles_high: usize,
    triangles_made: usize,
    unreached: usize,
    refused: usize,
    skipped: usize,
}

fn check<T: Imported>(
    label: &str,
    drawable: &RmcDrawable<T>,
    ratio: f32,
    tally: &mut Tally,
) -> Option<RmcDrawable<T>> {
    let before = scene_of(drawable);
    let high = level_of(&before, 0);
    if high.is_empty() {
        return None;
    }
    let mut current = drawable.clone();
    // A drawable that switches levels at distances of its own is not one a level is made for, and says why.
    if drawable
        .lod_group
        .lod_distances
        .iter()
        .any(|d| *d != 9998.0)
    {
        if let Some(level) = missing(drawable).first() {
            let request = LevelRequest {
                level: *level,
                ratio,
                replace: false,
            };
            let Err(refused) = add_level(drawable, &request) else {
                panic!("{label}: a level was made")
            };
            assert!(
                refused.to_string().contains("distances of its own"),
                "{label}: {refused}"
            );
            tally.refused += 1;
        }
        return None;
    }
    for level in missing(drawable) {
        let request = LevelRequest {
            level,
            ratio,
            replace: false,
        };
        let (made, report) = match add_level(&current, &request) {
            Ok(done) => done,
            Err(e) => panic!("{label}: level {level} was refused: {e}"),
        };
        let after = scene_of(&made);
        // A geometry with no collapse that is safe, such as a strip of hair cards, makes no level, and the model is as it was.
        if report.skipped.is_some() {
            assert_eq!(
                after.triangle_count(),
                before.triangle_count(),
                "{label}: a level that was skipped changed the model"
            );
            tally.skipped += 1;
            continue;
        }
        tally.levels += 1;

        // The high level is exactly what it was.
        let (was, is) = (level_of(&before, 0), level_of(&after, 0));
        assert_eq!(
            was.len(),
            is.len(),
            "{label}: the high level changed its geometry count"
        );
        for (w, i) in was.iter().zip(&is) {
            assert_eq!(w.positions, i.positions, "{label}: the high level moved");
            assert_eq!(
                w.indices, i.indices,
                "{label}: the high level's triangles changed"
            );
            assert_eq!(
                w.joints, i.joints,
                "{label}: the high level's bones changed"
            );
        }

        // The new level has the high level's models and geometries, each drawing with the same shader.
        let level_n = level_of(&after, level as u8);
        assert_eq!(
            level_n.len(),
            high.len(),
            "{label}: level {level} has other geometries"
        );
        for (n, h) in level_n.iter().zip(&high) {
            assert_eq!(
                (n.model, n.geometry, n.material),
                (h.model, h.geometry, h.material),
                "{label}: level {level} draws other than the high level does"
            );
            assert_eq!(n.uvs.len(), h.uvs.len(), "{label}: the channels changed");
            assert_eq!(
                n.colours.len(),
                h.colours.len(),
                "{label}: the channels changed"
            );
            assert_eq!(n.normals.is_empty(), h.normals.is_empty());
            assert_eq!(n.tangents.is_empty(), h.tangents.is_empty());
            assert_eq!(n.joints.is_empty(), h.joints.is_empty());
        }
        // The skin of every vertex is a skin the high level had at that place.
        for n in &level_n {
            if n.joints.is_empty() {
                continue;
            }
            tally.skinned += 1;
            for v in 0..n.positions.len() {
                assert!(
                    comes_from(n, v, &high),
                    "{label}: level {level} vertex {v} is no vertex of the high level"
                );
            }
        }

        // The ratio, to within the tolerance the report states: a geometry ends at its target or one under it, or says it could not.
        for cut in &report.cuts {
            if cut.reached {
                assert!(
                    cut.triangles_after <= cut.target
                        && cut.triangles_after + 1 >= cut.target.min(cut.triangles_before),
                    "{label}: {} came to {} for a target of {}",
                    cut.at,
                    cut.triangles_after,
                    cut.target
                );
            }
        }
        let made_triangles: usize = level_n.iter().map(|p| p.triangle_count()).sum();
        assert_eq!(
            made_triangles, report.triangles,
            "{label}: the report disagrees with the level"
        );
        tally.triangles_high += report.high_triangles;
        tally.triangles_made += report.triangles;
        tally.unreached += report.unreached();
        // A garment of the body has no strip of hair cards in it, so every geometry of it comes down to the share asked.
        if ["jbib_", "lowr_", "uppr_", "head_"]
            .iter()
            .any(|p| label.starts_with(p))
        {
            assert_eq!(
                report.unreached(),
                0,
                "{label}: level {level} stopped short: {:?}",
                report.cuts
            );
        }

        current = made;
    }
    (!missing(drawable).is_empty()).then_some(current)
}

fn every<T: Imported>(files: &[std::path::PathBuf], ratio: f32, tally: &mut Tally)
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    for path in files {
        let bytes = std::fs::read(path).expect("a staged file reads");
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let Ok(mut dictionary) = PgDictionary::<GtaDrawable<T>>::from_bytes(&bytes) else {
            continue;
        };
        let mut made = Vec::new();
        for (slot, entry) in dictionary.entries.iter_mut().enumerate() {
            if let Some(entry) = entry.as_deref_mut() {
                tally.drawables += 1;
                if let Some(done) = check(&format!("{name}#{slot}"), &entry.base, ratio, tally) {
                    entry.base = done;
                    made.push(slot);
                }
            }
        }
        if made.is_empty() {
            continue;
        }
        // The levels survive a save: the file reads back with the levels the drawables were given.
        let saved = dictionary
            .to_bytes()
            .unwrap_or_else(|e| panic!("{name}: would not save: {e}"));
        let back = PgDictionary::<GtaDrawable<T>>::from_bytes(&saved)
            .unwrap_or_else(|e| panic!("{name}: does not read back: {e}"));
        for slot in made {
            let (kept, read) = (
                dictionary.entries[slot].as_deref().expect("entry"),
                back.entries[slot].as_deref().expect("entry"),
            );
            assert_eq!(
                missing(&read.base),
                missing(&kept.base),
                "{name}#{slot}: a level was lost in the save"
            );
            let (a, b) = (scene_of(&kept.base), scene_of(&read.base));
            assert_eq!(
                a.triangle_count(),
                b.triangle_count(),
                "{name}#{slot}: the triangles changed in the save"
            );
        }
    }
}

#[test]
fn the_ped_components_of_the_corpus_get_the_levels_they_lack_and_keep_their_skin() {
    let (Some(g8), Some(g9)) = (
        Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
            .target(TARGET_GEN8)
            .find_or_skip("ped component drawables of Legacy"),
        Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
            .target(TARGET_GEN9)
            .find_or_skip("ped component drawables of Enhanced"),
    ) else {
        return;
    };
    let mut tally = Tally::default();
    every::<Gen8>(&g8, 0.5, &mut tally);
    every::<Gen9>(&g9, 0.5, &mut tally);
    println!(
        "{} drawables, {} levels made and {} skipped, {} skinned geometries, {} of {} triangles kept, {} geometries short",
        tally.drawables, tally.levels, tally.skipped, tally.skinned, tally.triangles_made, tally.triangles_high, tally.unreached
    );
    assert!(tally.levels > 0, "no staged drawable lacks a level");
    assert!(
        tally.skinned > 0,
        "no skinned geometry was held to its skin"
    );
}

/// The geometries of the staged body garments (the jackets, trousers, tops and heads), at the highest level, of one target.
fn body_geometries<T: Extracted>(files: &[std::path::PathBuf]) -> Vec<Primitive>
where
    PgDictionary<GtaDrawable<T>>: File,
{
    let mut out = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !["jbib_", "lowr_", "uppr_", "head_"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            continue;
        }
        let Ok(dictionary) = PgDictionary::<GtaDrawable<T>>::from_bytes(
            &std::fs::read(path).expect("a staged file reads"),
        ) else {
            continue;
        };
        for entry in dictionary.entries.iter().flatten() {
            let scene = drawable_scene(
                &name,
                &entry.base,
                &ExtractOptions {
                    lods: LodSelection::Highest,
                    textures: false,
                },
            );
            out.extend(
                scene
                    .primitives()
                    .filter(|p| {
                        p.lod == LodSlot::Level(0)
                            && p.triangle_count() > or_interchange::doctor::decimate::KEEP_WHOLE
                    })
                    .cloned(),
            );
        }
    }
    out
}

#[test]
fn every_body_geometry_of_both_targets_reaches_35_and_15_percent_of_its_triangles() {
    use or_interchange::doctor::decimate::decimate;
    let (Some(g8), Some(g9)) = (
        Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
            .target(TARGET_GEN8)
            .find_or_skip("body garments of Legacy"),
        Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
            .target(TARGET_GEN9)
            .find_or_skip("body garments of Enhanced"),
    ) else {
        return;
    };
    let mut all = body_geometries::<Gen8>(&g8);
    all.extend(body_geometries::<Gen9>(&g9));
    assert!(all.len() >= 8, "{} body geometries are staged", all.len());
    for ratio in [0.35f32, 0.15] {
        let (mut before, mut after) = (0usize, 0usize);
        for p in &all {
            let cut = decimate(p, ratio);
            assert!(
                cut.reached,
                "ratio {ratio}: {} of {} triangles, for a target of {}",
                cut.triangles_after, cut.triangles_before, cut.target
            );
            // Each geometry ends at its target or one under it.
            assert!(
                cut.triangles_after + 1 >= cut.target,
                "ratio {ratio}: {} for a target of {}",
                cut.triangles_after,
                cut.target
            );
            before += cut.triangles_before;
            after += cut.triangles_after;
        }
        let share = after as f32 / before as f32;
        // The shares add up to the ratio asked to within what each geometry's rounding and last collapse allow.
        assert!(
            (share - ratio).abs() < 0.01,
            "ratio {ratio}: {after} of {before} triangles is {share}"
        );
    }
}

#[test]
fn a_strip_of_hair_cards_that_has_no_safe_collapse_makes_no_level_and_says_why() {
    let Some(files) = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find_or_skip("a strip of hair cards")
    else {
        return;
    };
    let Some(path) = files
        .iter()
        .find(|p| p.file_name().is_some_and(|n| n == "hair_001_r.ydd"))
    else {
        or_test_support::skip_corpus("hair_001_r.ydd", "not staged".to_string());
        return;
    };
    let dictionary =
        PgDictionary::<GtaDrawable<Gen8>>::from_bytes(&std::fs::read(path).unwrap()).unwrap();
    let hair = &dictionary
        .entries
        .iter()
        .flatten()
        .next()
        .expect("a drawable")
        .base;
    assert!(missing(hair).contains(&1));

    let request = LevelRequest {
        level: 1,
        ratio: 0.35,
        replace: false,
    };
    let (made, report) = add_level(hair, &request).expect("a refusal to cut is no error");

    let why = report.skipped.as_deref().expect("the level was skipped");
    assert!(
        why.contains("medium level was not made")
            && why.contains("no collapse that keeps its seams"),
        "{why}"
    );
    assert_eq!(report.lines(), vec![why.to_string()]);
    assert!(missing(&made).contains(&1), "no level was made");
    assert_eq!(
        scene_of(&made).triangle_count(),
        scene_of(hair).triangle_count()
    );
}

#[test]
fn a_level_that_copies_the_high_one_is_found_and_removed() {
    let Some(g8) = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find_or_skip("ped component drawables of Legacy")
    else {
        return;
    };
    let mut checked = 0;
    for path in g8.iter().take(300) {
        let Ok(dictionary) = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(
            &std::fs::read(path).expect("a staged file reads"),
        ) else {
            continue;
        };
        for entry in dictionary.entries.iter().flatten() {
            let drawable = &entry.base;
            if !missing(drawable).contains(&1) || drawable.lod_group.lods[0].models.is_empty() {
                continue;
            }
            assert!(
                copies(drawable, 0.9).is_empty(),
                "a drawable of one level has no copy of it"
            );
            let request = LevelRequest {
                level: 1,
                ratio: 1.0,
                replace: false,
            };
            let Ok((mut made, report)) = add_level(drawable, &request) else {
                continue;
            };
            if report.skipped.is_some() {
                continue;
            }
            assert_eq!(
                copies(&made, 0.9),
                vec![1],
                "the whole high level made again is a copy of it"
            );
            made.remove_lods(&[or_io::gta5::drawable::LodSlot::Level(1)]);
            assert!(
                copies(&made, 0.9).is_empty() && missing(&made).contains(&1),
                "removed, the copy is gone and the level is missing again"
            );
            checked += 1;
            if checked == 3 {
                return;
            }
        }
    }
    assert!(
        checked > 0,
        "no staged drawable could take a copy of its high level"
    );
}
