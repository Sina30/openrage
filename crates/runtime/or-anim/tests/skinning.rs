// Round-trip checks for `world * inverse_bind` CPU skinning.

use or_anim::{decode_skinning, Pose, Skeleton, SkinnedMesh};
use or_io::gta5::{CrSkeletonData, DrawableTarget, RmcDrawable};
use or_io::Asset;
use std::path::{Path, PathBuf};
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

fn lod0_skinning<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<SkinnedMesh> {
    drawable.lod_group.lods[0]
        .models
        .iter()
        .flat_map(|model| model.geometries.iter())
        .filter_map(|geometry| decode_skinning(&*geometry.vertex_buffer, &geometry.matrix_palette))
        .filter(|m| !m.is_empty())
        .collect()
}

fn skinned_cases() -> Vec<(String, Skeleton, Vec<SkinnedMesh>)> {
    let mut out = Vec::new();

    let mut push = |label: String, skeleton: Option<&CrSkeletonData>, meshes: Vec<SkinnedMesh>| {
        let Some(skeleton) = skeleton else { return };
        if skeleton.bones.is_empty() || meshes.is_empty() {
            return;
        }
        out.push((label, Skeleton::build(skeleton), meshes));
    };

    for path in fixtures_with_extension("ydd") {
        let name = file_name(&path);
        match or_io::open(&path) {
            Ok(Asset::DrawableDict(dict)) => {
                for (index, entry) in dict.entries.iter().enumerate() {
                    if let Some(d) = entry {
                        let meshes = lod0_skinning(&d.base);
                        push(
                            format!("{name}[{index}]"),
                            d.base.skeleton.as_deref(),
                            meshes,
                        );
                    }
                }
            }
            Ok(Asset::DrawableDictGen9(dict)) => {
                for (index, entry) in dict.entries.iter().enumerate() {
                    if let Some(d) = entry {
                        let meshes = lod0_skinning(&d.base);
                        push(
                            format!("{name}[{index}]"),
                            d.base.skeleton.as_deref(),
                            meshes,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    out
}

#[test]
fn influence_weights_sum_to_one() {
    let cases = skinned_cases();
    if cases.is_empty() {
        or_test_support::skip_corpus(
            "influence_weights_sum_to_one",
            "no fixtures with skinned geometry",
        );
        return;
    }

    let mut vertices_checked = 0usize;
    for (label, _, meshes) in &cases {
        for mesh in meshes {
            for (index, influences) in mesh.influences.iter().enumerate() {
                if !influences.is_bound() {
                    continue;
                }
                let sum: f32 = influences.weights[..influences.count as usize].iter().sum();
                assert!(
                    (sum - 1.0).abs() < 1e-4,
                    "{label}: vertex {index} weights sum to {sum}, not 1.0"
                );
                vertices_checked += 1;
            }
        }
    }
    assert!(vertices_checked > 0, "no bound vertices found");
    eprintln!(
        "weights verified on {vertices_checked} vertices across {} skinned drawables",
        cases.len()
    );
}

#[test]
fn every_influence_resolves_to_a_real_bone() {
    let cases = skinned_cases();
    if cases.is_empty() {
        or_test_support::skip_corpus(
            "every_influence_resolves_to_a_real_bone",
            "no fixtures with skinned geometry",
        );
        return;
    }

    for (label, skeleton, meshes) in &cases {
        let bone_count = skeleton.bone_count() as u16;
        for mesh in meshes {
            for bone in &mesh.bones_used {
                assert!(
                    *bone < bone_count,
                    "{label}: influence names bone {bone} but the skeleton has {bone_count}"
                );
            }
            for (index, influences) in mesh.influences.iter().enumerate() {
                for (lane, &bone) in influences
                    .bones
                    .iter()
                    .enumerate()
                    .take(influences.count as usize)
                {
                    assert!(
                        bone < bone_count,
                        "{label}: vertex {index} lane {lane} names bone {bone} of {bone_count}"
                    );
                }
            }
        }
    }
}

#[test]
fn skinning_at_the_bind_pose_is_the_identity_on_real_meshes() {
    let cases = skinned_cases();
    if cases.is_empty() {
        or_test_support::skip_corpus(
            "skinning_at_the_bind_pose_is_the_identity_on_real_meshes",
            "no fixtures with skinned geometry",
        );
        return;
    }

    // Bind-pose skin matrices are `world * inverse(world)` — error should be ~0.
    const EPSILON: f32 = 1e-3;

    let mut meshes_checked = 0usize;
    for (label, skeleton, meshes) in &cases {
        let skin = Pose::bind(skeleton).skin_matrices(skeleton);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        for mesh in meshes {
            mesh.skin_into(&skin, &mut positions, &mut normals);
            assert_eq!(positions.len(), mesh.bind_positions.len());
            for (index, (posed, authored)) in
                positions.iter().zip(mesh.bind_positions.iter()).enumerate()
            {
                let delta = (glam::Vec3::from(*posed) - glam::Vec3::from(*authored)).length();
                assert!(
                    delta < EPSILON,
                    "{label}: vertex {index} moved {delta} at the bind pose \
                     (authored {authored:?}, posed {posed:?})"
                );
            }
            meshes_checked += 1;
        }
    }
    eprintln!("bind-pose identity verified on {meshes_checked} skinned meshes");
}

#[test]
fn moving_a_bone_moves_the_vertices_bound_to_it() {
    let cases = skinned_cases();
    if cases.is_empty() {
        or_test_support::skip_corpus(
            "moving_a_bone_moves_the_vertices_bound_to_it",
            "no fixtures with skinned geometry",
        );
        return;
    }

    let mut moved_any = false;
    for (label, skeleton, meshes) in &cases {
        let Some(mesh) = meshes.iter().find(|m| !m.bones_used.is_empty()) else {
            continue;
        };
        let mut pose = Pose::bind(skeleton);
        for bone in &mesh.bones_used {
            if let Some(local) = pose.local_mut(*bone) {
                local.translation += glam::Vec3::new(0.0, 0.0, 10.0);
            }
        }
        let skin = pose.skin_matrices(skeleton);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        mesh.skin_into(&skin, &mut positions, &mut normals);

        let moved = positions
            .iter()
            .zip(mesh.bind_positions.iter())
            .filter(|(posed, authored)| {
                (glam::Vec3::from(**posed) - glam::Vec3::from(**authored)).length() > 1.0
            })
            .count();
        assert!(
            moved > 0,
            "{label}: displacing every bone the mesh uses moved no vertices"
        );
        moved_any = true;
    }
    assert!(moved_any, "no mesh had any bones to displace");
}

#[test]
fn report_decode_anomalies_across_the_corpus() {
    let cases = skinned_cases();
    if cases.is_empty() {
        or_test_support::skip_corpus(
            "report_decode_anomalies_across_the_corpus",
            "no fixtures with skinned geometry",
        );
        return;
    }

    let mut out_of_range = 0usize;
    let mut degenerate = 0usize;
    let mut total_vertices = 0usize;
    for (_, _, meshes) in &cases {
        for mesh in meshes {
            out_of_range += mesh.out_of_range_influences;
            degenerate += mesh.degenerate_weight_vertices;
            total_vertices += mesh.vertex_count();
        }
    }
    eprintln!(
        "{total_vertices} skinned vertices: {out_of_range} out-of-range influences, \
         {degenerate} all-zero-weight vertices"
    );
}
