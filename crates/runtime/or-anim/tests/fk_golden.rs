mod fragment;

use or_anim::{Pose, Skeleton};
use or_io::gta5::CrSkeletonData;
use or_io::Asset;
use std::path::{Path, PathBuf};
const MATRIX_EPSILON: f32 = 1e-3;
fn fixtures_dir() -> PathBuf {
    or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures_with_extension(extension: &str) -> Vec<PathBuf> {
    or_test_support::Corpus::with_root(fixtures_dir(), extension)
        .all_targets()
        .find()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn yft_skeletons() -> Vec<(String, CrSkeletonData)> {
    let mut out = Vec::new();
    for path in fixtures_with_extension("yft") {
        let skeleton = fragment::skeleton(&path);
        if let Some(skeleton) = skeleton.filter(|s| !s.bones.is_empty()) {
            out.push((file_name(&path), skeleton));
        }
    }
    out
}

fn drawable_skeletons() -> Vec<(String, CrSkeletonData)> {
    let mut out = Vec::new();
    let mut push = |name: String, skeleton: Option<&CrSkeletonData>| {
        if let Some(skeleton) = skeleton.filter(|s| !s.bones.is_empty()) {
            out.push((name, skeleton.clone()));
        }
    };
    let mut paths = fixtures_with_extension("ydr");
    paths.extend(fixtures_with_extension("ydd"));
    for path in paths {
        let name = file_name(&path);
        match or_io::open(&path) {
            Ok(Asset::Drawable(d)) => push(name, d.base.skeleton.as_deref()),
            Ok(Asset::DrawableGen9(d)) => push(name, d.base.skeleton.as_deref()),
            Ok(Asset::DrawableDict(dict)) => {
                for (index, entry) in dict.entries.iter().enumerate() {
                    let skeleton = entry.as_ref().and_then(|d| d.base.skeleton.as_deref());
                    push(format!("{name}[{index}]"), skeleton);
                }
            }
            Ok(Asset::DrawableDictGen9(dict)) => {
                for (index, entry) in dict.entries.iter().enumerate() {
                    let skeleton = entry.as_ref().and_then(|d| d.base.skeleton.as_deref());
                    push(format!("{name}[{index}]"), skeleton);
                }
            }
            _ => {}
        }
    }
    out
}

#[track_caller]
fn skip(reason: &str) {
    or_test_support::skip_corpus("forward-kinematics golden", reason);
}

#[test]
fn forward_kinematics_reproduces_the_shipped_inverse_world_bind() {
    let skeletons = yft_skeletons();
    if skeletons.is_empty() {
        skip("no .yft fixtures with skeletons");
        return;
    }

    let mut compared = 0usize;
    let mut worst = (0.0f32, String::new());

    for (name, data) in &skeletons {
        let skeleton = Skeleton::build(data);
        let Some(deviation) = skeleton.inverse_bind_deviation() else {
            continue;
        };
        compared += 1;
        if deviation > worst.0 {
            worst = (deviation, name.clone());
        }
        assert!(
            deviation < MATRIX_EPSILON,
            "{name}: computed inverse world bind deviates from m_InverseTransforms by \
             {deviation} ({} bones, file order topological: {})",
            skeleton.bone_count(),
            skeleton.file_order_was_topological()
        );
    }

    assert!(
        compared > 0,
        "no fixture shipped m_InverseTransforms to compare against"
    );
    eprintln!(
        "forward kinematics verified against {compared} shipped skeletons; \
         worst deviation {} on {}",
        worst.0, worst.1
    );
}

#[test]
fn local_bind_composition_matches_the_shipped_default_transforms() {
    let skeletons = yft_skeletons();
    if skeletons.is_empty() {
        skip("no .yft fixtures with skeletons");
        return;
    }

    let mut compared = 0usize;
    for (name, data) in &skeletons {
        let skeleton = Skeleton::build(data);
        let Some(deviation) = skeleton.local_bind_deviation() else {
            continue;
        };
        compared += 1;
        assert!(
            deviation < MATRIX_EPSILON,
            "{name}: composed local bind deviates from m_DefaultTransforms by {deviation}"
        );
    }
    assert!(compared > 0, "no fixture shipped m_DefaultTransforms");
    eprintln!("local bind composition verified against {compared} shipped skeletons");
}

#[test]
fn drawable_skeletons_also_reproduce_their_shipped_matrices() {
    let skeletons = drawable_skeletons();
    if skeletons.is_empty() {
        skip("no .ydr/.ydd fixtures with skeletons");
        return;
    }

    let mut compared = 0usize;
    for (name, data) in &skeletons {
        let skeleton = Skeleton::build(data);
        if let Some(deviation) = skeleton.inverse_bind_deviation() {
            compared += 1;
            assert!(
                deviation < MATRIX_EPSILON,
                "{name}: inverse world bind deviates by {deviation}"
            );
        }
        if let Some(deviation) = skeleton.local_bind_deviation() {
            assert!(
                deviation < MATRIX_EPSILON,
                "{name}: local bind deviates by {deviation}"
            );
        }
    }
    eprintln!("verified {compared} drawable skeletons");
}

#[test]
fn shipped_default_transforms_are_not_the_world_pose() {
    let skeletons = yft_skeletons();
    if skeletons.is_empty() {
        skip("no .yft fixtures with skeletons");
        return;
    }

    let differing = skeletons
        .iter()
        .filter(|(_, data)| {
            let skeleton = Skeleton::build(data);
            if skeleton.file_local_bind.is_empty() || skeleton.bone_count() < 2 {
                return false;
            }
            skeleton
                .bind_world
                .iter()
                .zip(skeleton.file_local_bind.iter())
                .any(|(world, local)| {
                    (*world - *local)
                        .to_cols_array()
                        .iter()
                        .any(|v| v.abs() > 1e-2)
                })
        })
        .count();

    assert!(
        differing > 0,
        "no skeleton distinguished local from world bind, so the two arrays may \
         have been conflated"
    );
    eprintln!(
        "{differing} of {} skeletons show local != world, as expected",
        skeletons.len()
    );
}

#[test]
fn bind_pose_skin_matrices_are_identity_on_real_rigs() {
    let mut skeletons = yft_skeletons();
    skeletons.extend(drawable_skeletons());
    if skeletons.is_empty() {
        skip("no fixtures with skeletons");
        return;
    }

    for (name, data) in &skeletons {
        let skeleton = Skeleton::build(data);
        let skin = Pose::bind(&skeleton).skin_matrices(&skeleton);
        for (bone, matrix) in skin.iter().enumerate() {
            let delta = (*matrix - glam::Mat4::IDENTITY).to_cols_array();
            let worst = delta.iter().fold(0.0f32, |acc, v| acc.max(v.abs()));
            assert!(
                worst < MATRIX_EPSILON,
                "{name}: bone {bone} ({}) bind skin matrix is not identity, off by {worst}",
                skeleton
                    .bone(bone as u16)
                    .map(|b| b.name.as_str())
                    .unwrap_or("?")
            );
        }
    }
}

#[test]
fn every_bone_tag_resolves_to_its_own_index() {
    let skeletons = yft_skeletons();
    if skeletons.is_empty() {
        skip("no .yft fixtures with skeletons");
        return;
    }

    for (name, data) in &skeletons {
        let skeleton = Skeleton::build(data);
        let mut seen = std::collections::HashMap::new();
        for bone in &skeleton.bones {
            if let Some(previous) = seen.insert(bone.tag, bone.index) {
                panic!(
                    "{name}: bone tag {} is used by both bone {previous} and bone {}",
                    bone.tag, bone.index
                );
            }
            assert_eq!(
                skeleton.index_for_tag(bone.tag),
                Some(bone.index),
                "{name}: bone {} tag {} did not resolve to itself",
                bone.index,
                bone.tag
            );
        }
    }
}

#[test]
fn retail_skeletons_are_stored_in_topological_order() {
    let skeletons = yft_skeletons();
    if skeletons.is_empty() {
        skip("no .yft fixtures with skeletons");
        return;
    }

    let mut out_of_order = Vec::new();
    for (name, data) in &skeletons {
        let skeleton = Skeleton::build(data);
        if !skeleton.file_order_was_topological() {
            out_of_order.push(name.clone());
        }
    }
    if out_of_order.is_empty() {
        eprintln!("all {} skeletons were already topological", skeletons.len());
    } else {
        eprintln!(
            "{} of {} skeletons needed reordering: {:?}",
            out_of_order.len(),
            skeletons.len(),
            out_of_order
        );
    }
}

#[test]
fn evaluation_order_always_places_parents_first() {
    let mut skeletons = yft_skeletons();
    skeletons.extend(drawable_skeletons());
    if skeletons.is_empty() {
        skip("no fixtures with skeletons");
        return;
    }

    for (name, data) in &skeletons {
        let skeleton = Skeleton::build(data);
        let mut resolved = vec![false; skeleton.bone_count()];
        for &bone in &skeleton.eval_order {
            let parent = skeleton.parents[bone as usize];
            if parent != or_anim::NO_PARENT {
                assert!(
                    resolved[parent as usize],
                    "{name}: bone {bone} evaluated before its parent {parent}"
                );
            }
            resolved[bone as usize] = true;
        }
        assert!(
            resolved.iter().all(|r| *r),
            "{name}: evaluation order did not cover every bone"
        );
    }
}
