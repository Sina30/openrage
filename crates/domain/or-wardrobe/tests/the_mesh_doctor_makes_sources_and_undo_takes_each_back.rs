//! The mesh doctor as the Wardrobe runs it: each operation makes a new drawables source that the garment names, the source it was is not touched, and Undo names it again.
//!
//! ⚠ Reads ped components staged in the corpus; a machine without them skips, and says so.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, snapshot, JBIB};
use or_interchange::{drawable_scene, Bone, ExtractOptions, LodSelection, LodSlot, Primitive};
use or_io::decl::{File, Gen8};
use or_io::gta5::{GtaDrawable, GtaFragType, PgDictionary};
use or_source::asset::Source;
use or_source::models::{self, Extraction};
use or_test_support::{Corpus, TARGET_GEN8};
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::mesh::{examine, run, Op};
use or_wardrobe::{Ped, Wardrobe};

fn staged(name: &str) -> Option<PathBuf> {
    let found = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == name));
    if found.is_none() {
        or_test_support::skip_corpus(
            &format!("the ped component {name}"),
            "not staged".to_string(),
        );
    }
    found
}

/// The staged skeleton with the most bones: a ped's.
fn ped_skeleton() -> Option<Vec<Bone>> {
    Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft")
        .target(TARGET_GEN8)
        .find()
        .iter()
        .filter_map(|p| {
            let fragment = GtaFragType::<Gen8>::from_bytes(&std::fs::read(p).ok()?).ok()?;
            fragment.drawables().into_iter().find_map(|d| {
                d.base
                    .skeleton
                    .as_deref()
                    .map(or_interchange::extract::bones_of)
            })
        })
        .max_by_key(Vec::len)
        .filter(|b| b.len() >= 100)
}

/// A pack whose one garment names a drawables source made of `file`, extracted beside `skeleton` when it is given.
fn a_pack(dir: &Path, file: &Path, skeleton: Option<&[Bone]>) -> (PathBuf, String) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("models")).unwrap();
    let bytes = std::fs::read(file).unwrap();
    let made = models::extract_drawables(
        &bytes,
        "model",
        &pack.join("models"),
        &Extraction {
            from_game: false,
            virtual_path: None,
            textures: false,
            skeleton,
        },
    )
    .expect("a staged ped component extracts");
    assert_eq!(made.file_name().unwrap(), "model.drawables.src");
    let mut w = Wardrobe::new("demo");
    let g = garment(Ped::Male, JBIB, None, "models/model.drawables.src", &[]);
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    (pack, id)
}

/// The bones and the weights of every vertex of a geometry.
type Skin = (Vec<[u16; 4]>, Vec<[f32; 4]>);

fn compiled(pack: &Path, rel: &str) -> Vec<u8> {
    let source = Source::open(&pack.join(rel)).expect("a source");
    models::compile(&source)
        .expect("a source that compiles")
        .bytes
}

fn scene_levels(bytes: &[u8]) -> Vec<Vec<Primitive>> {
    let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).expect("a dictionary");
    let entry = dictionary
        .entries
        .iter()
        .flatten()
        .next()
        .expect("a drawable");
    let scene = drawable_scene(
        "d",
        &entry.base,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    (0..4)
        .map(|l| {
            scene
                .primitives()
                .filter(|p| p.lod == LodSlot::Level(l))
                .cloned()
                .collect()
        })
        .collect()
}

fn triangles(level: &[Primitive]) -> usize {
    level.iter().map(Primitive::triangle_count).sum()
}

#[test]
fn levels_are_added_to_a_model_that_lacks_them_and_undo_takes_them_away() {
    let Some(file) = staged("jbib_000_u_1.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path(), &file, None);
    let old = "models/model.drawables.src";
    let before_files = snapshot(&pack);
    let high = scene_levels(&compiled(&pack, old));
    assert_eq!(
        (triangles(&high[1]), triangles(&high[2])),
        (0, 0),
        "the staged model has no lower level"
    );

    let op = Op::Levels {
        levels: vec![(1, 0.5), (2, 0.15)],
        replace: false,
    };
    let (report, revision) =
        run(&pack, &id, &op, None, &Watch::default()).expect("levels are made");
    assert_eq!(report.models.len(), 1, "{report:?}");
    let done = &report.models[0];
    assert_eq!(done.file, old);
    assert!(
        done.into.ends_with(".drawables.src") && done.into != old,
        "{}",
        done.into
    );

    // The garment names the new source; the old one is as it was.
    let w = Wardrobe::read(&pack).unwrap();
    assert_eq!(w.garments[0].model.as_deref(), Some(done.into.as_str()));
    for (path, bytes) in before_files
        .iter()
        .filter(|(p, _)| p.starts_with("models/"))
    {
        assert_eq!(snapshot(&pack).get(path), Some(bytes), "{path} was touched");
    }

    // The new source compiles to a model with the levels, whose high level is the model's own.
    let levels = scene_levels(&compiled(&pack, &done.into));
    assert_eq!(levels[0].len(), high[0].len());
    for (a, b) in levels[0].iter().zip(&high[0]) {
        assert_eq!(a.positions, b.positions, "the high level moved");
        assert_eq!(a.indices, b.indices, "the high level's triangles changed");
    }
    let asked = |ratio: f32| (triangles(&high[0]) as f32 * ratio).ceil() as usize;
    for (level, ratio) in [(1usize, 0.5f32), (2, 0.15)] {
        let made = triangles(&levels[level]);
        let target = asked(ratio);
        assert!(
            (target.saturating_sub(1)..=target).contains(&made),
            "level {level}: {made} triangles for a target of {target}"
        );
    }
    assert_eq!(done.after[1].triangles, triangles(&levels[1]));
    assert!(
        done.lines.iter().any(|l| l.contains("Medium level")),
        "{:?}",
        done.lines
    );

    // The garment's own examination sees the levels now, and a second run has nothing to do.
    let looked = examine(&pack, &id, None).unwrap();
    assert!(
        looked.models[0].missing.is_empty(),
        "{:?}",
        looked.models[0]
    );
    let (again, none) = run(&pack, &id, &op, None, &Watch::default()).unwrap();
    assert!(!again.changed() && none.is_none(), "{again:?}");

    // Undo names the old source and takes the new one out of the pack.
    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    assert!(!pack.join(&done.into).exists());
    // Undo holds what it took out under .openrage for Redo; the pack itself is as it was.
    let models_now: Vec<String> = snapshot(&pack)
        .into_keys()
        .filter(|p| p.starts_with("models/"))
        .collect();
    let models_then: Vec<String> = before_files
        .keys()
        .filter(|p| p.starts_with("models/"))
        .cloned()
        .collect();
    assert_eq!(models_now, models_then);
    assert_eq!(
        Wardrobe::read(&pack).unwrap().garments[0].model.as_deref(),
        Some(old)
    );
}

#[test]
fn levels_that_the_decimator_skips_leave_the_pack_alone_and_the_report_says_why() {
    let Some(file) = staged("hair_001_r.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path(), &file, None);
    let before_files = snapshot(&pack);

    let op = Op::Levels {
        levels: vec![(1, 0.35), (2, 0.15)],
        replace: false,
    };
    let (report, revision) =
        run(&pack, &id, &op, None, &Watch::default()).expect("a skip is a report");
    assert!(
        !report.changed() && report.refused.is_empty() && revision.is_none(),
        "{report:?}"
    );
    assert_eq!(snapshot(&pack), before_files, "a skipped level wrote");

    // Strips of hair cards cannot be cut without breaking their borders: the person is told so, not that there is nothing to do.
    let why = "level was not made";
    assert!(report.notes.iter().any(|n| n.contains(why)), "{report:?}");
    assert!(report.lines().iter().any(|l| l.contains(why)), "{report:?}");
    assert!(report.sentence().contains(why), "{}", report.sentence());
}

#[test]
fn a_model_mirrored_twice_is_the_model_it_was() {
    let (Some(file), Some(skeleton)) = (staged("jbib_000_u_1.ydd"), ped_skeleton()) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path(), &file, Some(&skeleton));
    let start = scene_levels(&compiled(&pack, "models/model.drawables.src"));

    let (first, _) =
        run(&pack, &id, &Op::Mirror, Some(&skeleton), &Watch::default()).expect("mirrors");
    let once = first.models[0].into.clone();
    let (second, _) =
        run(&pack, &id, &Op::Mirror, Some(&skeleton), &Watch::default()).expect("mirrors back");
    let twice = second.models[0].into.clone();
    assert_ne!(once, twice);

    let mirrored = scene_levels(&compiled(&pack, &once));
    let back = scene_levels(&compiled(&pack, &twice));
    for ((a, m), z) in start[0].iter().zip(&mirrored[0]).zip(&back[0]) {
        assert_ne!(a.positions, m.positions, "nothing moved");
        assert_eq!(a.positions, z.positions, "mirroring twice moved a vertex");
        assert_eq!(a.indices, z.indices);
        assert_eq!(a.joints, z.joints);
        assert_eq!(a.weights, z.weights);
    }
}

#[test]
fn a_rig_with_a_group_no_skeleton_knows_is_found_and_mended_and_the_skin_is_kept() {
    let Some(file) = staged("head_010_r.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path(), &file, None);
    let old = "models/model.drawables.src";
    let skin = |bytes: &[u8]| -> Vec<Skin> {
        scene_levels(bytes)[0]
            .iter()
            .map(|p| (p.joints.clone(), p.weights.clone()))
            .collect()
    };
    let start = skin(&compiled(&pack, old));

    // A tool names a group it made `Group.001`: rename the bone most vertices use in the source's glb.
    let source = pack.join(old);
    let entry = std::fs::read_dir(source.join("drawables"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    let rules: models::Rules =
        serde_json::from_str(&std::fs::read_to_string(entry.join("drawable.json")).unwrap())
            .unwrap();
    assert!(
        !rules.skeleton.is_empty(),
        "the drawable's own skeleton is listed by name"
    );
    let glb = std::fs::read(entry.join("model.glb")).unwrap();
    let mut scene = or_interchange::gltf::read::load(Path::new("model.glb"), &glb).unwrap();
    let mut uses = vec![0usize; scene.objects[0].bones.len()];
    for p in scene.primitives() {
        for (j, w) in p.joints.iter().zip(&p.weights) {
            for l in (0..4).filter(|l| w[*l] > 0.0) {
                uses[usize::from(j[l])] += 1;
            }
        }
    }
    let bones = &scene.objects[0].bones;
    let used = (0..uses.len())
        .filter(|i| bones[*i].parent.is_some())
        .max_by_key(|i| uses[*i])
        .unwrap();
    let was = bones[used].name.clone();
    scene.objects[0].bones[used].name = "Group.001".to_string();
    let tampered =
        or_interchange::gltf::write(&scene, "model", or_interchange::gltf::Container::Glb).primary;
    std::fs::write(entry.join("model.glb"), tampered).unwrap();
    let before_files = snapshot(&pack);

    // The compile is refused for the group, and the examination says which.
    let refusal =
        models::compile(&Source::open(&source).unwrap()).expect_err("a group no skeleton knows");
    assert!(
        refusal.iter().any(|p| p.to_string().contains("Group.001")),
        "{refusal:?}"
    );
    let looked = examine(&pack, &id, None).unwrap();
    assert!(
        looked.models[0]
            .rig
            .iter()
            .any(|r| r.contains("Group.001") && r.contains("no name of a bone")),
        "{:?}",
        looked.models[0]
    );

    // Mended, the model compiles, and the weights that were on the group are on the bone above it.
    let (report, revision) =
        run(&pack, &id, &Op::Rig, None, &Watch::default()).expect("the rig is mended");
    assert_eq!(report.models.len(), 1, "{report:?}");
    assert!(
        report.models[0]
            .lines
            .iter()
            .any(|l| l.contains("Group.001")),
        "{:?}",
        report.models[0].lines
    );
    let into = report.models[0].into.clone();
    let mended = skin(&compiled(&pack, &into));
    assert_eq!(mended.len(), start.len());
    let parent = scene.objects[0].bones[used].parent.unwrap() as u16;
    for ((j0, w0), (j1, w1)) in start.iter().zip(&mended) {
        for v in 0..j0.len() {
            let weight_on = |j: &[u16; 4], w: &[f32; 4], bone: u16| -> f32 {
                (0..4).filter(|l| j[*l] == bone).map(|l| w[l]).sum()
            };
            let moved = weight_on(&j0[v], &w0[v], used as u16);
            let expected_parent = weight_on(&j0[v], &w0[v], parent) + moved;
            let got_parent = weight_on(&j1[v], &w1[v], parent);
            // A weight under the noise floor is cleaned away with the repair, which moves what the others hold.
            let noisy = w0[v]
                .iter()
                .any(|w| *w > 0.0 && *w < or_interchange::doctor::rig::FAINT);
            if !noisy {
                assert!(
                    (got_parent - expected_parent).abs() < 3.0 / 255.0,
                    "vertex {v}: {got_parent} for {expected_parent} (the group was {was})"
                );
            }
            assert!(
                (w1[v].iter().sum::<f32>() - 1.0).abs() < 3.0 / 255.0,
                "vertex {v}: the weights do not add up"
            );
            assert!(
                weight_on(&j1[v], &w1[v], used as u16) < 1.0 / 255.0,
                "vertex {v}: a weight stayed on the group"
            );
        }
    }
    // The tampered source is as it was, and Undo gives it back.
    for (path, bytes) in before_files
        .iter()
        .filter(|(p, _)| p.starts_with("models/"))
    {
        assert_eq!(snapshot(&pack).get(path), Some(bytes), "{path} was touched");
    }
    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    assert!(!pack.join(&into).exists());
}
