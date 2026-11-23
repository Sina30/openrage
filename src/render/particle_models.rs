use std::collections::HashMap;
use std::sync::Arc;

use or_io::gta5::PtxFxList;
use or_renderer::particle_models::{ModelBatch, ModelGeometry, ModelInstance, ModelVertex};
use or_renderer::particles::ParticleBlendMode;
use or_simulation::{DrawKind, PtfxManager};

use crate::render::gta6_drawable::meshes_at;
use crate::render::meshes::build_meshes_from_ypt_drawable;
use crate::render::particles::{gta6, ParticleTextureCache};
use crate::state::gta6_particle_models::found_drawable;

/// One surface of a model drawable: its mesh and the name of its diffuse texture.
pub struct CachedGeom {
    pub texture_name: Option<String>,
    pub geometry: Arc<ModelGeometry>,
}

#[derive(Default)]
pub struct ParticleModelMeshCache {
    pub cached: HashMap<u32, Arc<Vec<CachedGeom>>>,
    pub doc_id: Option<crate::state::DocumentId>,
}

impl ParticleModelMeshCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate_for_doc(&mut self, doc_id: crate::state::DocumentId) {
        if self.doc_id != Some(doc_id) {
            self.cached.clear();
            self.doc_id = Some(doc_id);
        }
    }

    pub fn get_or_build(
        &mut self,
        asset: &PtxFxList,
        drawable_hash: u32,
    ) -> Option<Arc<Vec<CachedGeom>>> {
        if let Some(g) = self.cached.get(&drawable_hash) {
            return Some(g.clone());
        }
        // The box the engine divides a particle's dimensions by is the drawable's own, which every surface of it shares
        let mut size = or_particle::list::model(asset, drawable_hash).map_or([0.0; 3], |model| {
            let b = &model.lod_group.bounds_box;
            [b.max.x - b.min.x, b.max.y - b.min.y, b.max.z - b.min.z]
        });
        let mut uploads = build_meshes_from_ypt_drawable(asset, drawable_hash);
        // A GTA VI list holds no drawable: the one its rule names is a search's find, if any.
        if uploads.is_empty() {
            if let Some(found) = found_drawable(drawable_hash) {
                uploads = meshes_at(&found, 0, None, None, &|_: u64| None);
                size = std::array::from_fn(|i| found.box_max[i] - found.box_min[i]);
            }
        }
        let geoms: Vec<CachedGeom> = uploads
            .into_iter()
            .filter(|u| !u.vertices.is_empty() && !u.indices.is_empty())
            .map(|u| CachedGeom {
                texture_name: u.texture_name,
                geometry: Arc::new(ModelGeometry {
                    vertices: u
                        .vertices
                        .iter()
                        .map(|v| ModelVertex {
                            position: v.position,
                            normal: v.normal,
                            uv: v.uv,
                        })
                        .collect(),
                    indices: u.indices,
                    size,
                }),
            })
            .collect();
        if geoms.is_empty() {
            return None;
        }
        let arc = Arc::new(geoms);
        self.cached.insert(drawable_hash, arc.clone());
        Some(arc)
    }
}

pub fn build_model_batches(
    mesh_cache: &mut ParticleModelMeshCache,
    texture_cache: &mut ParticleTextureCache,
    asset: Option<&PtxFxList>,
    manager: &PtfxManager,
    camera_pos: glam::Vec3,
    retail_parity: bool,
) -> Vec<ModelBatch> {
    let mut out = Vec::new();
    let Some(asset) = asset else {
        return out;
    };
    for emitter in manager.emitters() {
        if emitter.draw_kind != DrawKind::Model {
            continue;
        }
        if emitter.points.is_empty() {
            continue;
        }
        let rule = &emitter.rules.particle;
        let camera_shrink = find_camera_shrink(&emitter.rules.behaviours);
        if rule.drawables.is_empty() {
            continue;
        }
        let raw_blend = ParticleBlendMode::from_engine_byte(rule.render_state.blend_set as u8);
        let blend = if retail_parity {
            raw_blend.retail_equivalent()
        } else {
            raw_blend
        };
        let n = emitter.points.len();
        let drawable_count = rule.drawables.len();
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); drawable_count];
        for (i, &frame) in emitter.points.init_tex_frame.iter().enumerate().take(n) {
            let idx = (frame as usize) % drawable_count;
            buckets[idx].push(i);
        }
        for (drawable_idx, indices) in buckets.iter().enumerate() {
            if indices.is_empty() {
                continue;
            }
            let drawable_hash = rule.drawables[drawable_idx].hash_name.get();
            let Some(geoms) = mesh_cache.get_or_build(asset, drawable_hash) else {
                continue;
            };
            let instances =
                build_instances_for_indices(&emitter.points, camera_shrink, camera_pos, indices);
            for (surface, geom) in geoms.iter().enumerate() {
                // A GTA VI drawable's surface takes the texture the model view picks for it.
                let picked = gta6::model_texture(texture_cache, drawable_hash, surface);
                let diffuse_image = (picked.as_deref())
                    .or(geom.texture_name.as_deref())
                    .and_then(|name| texture_cache.get_or_decode_image(asset, name));
                out.push(ModelBatch {
                    instances: instances.clone(),
                    blend,
                    shader_technique: model_technique(rule.shader_inst.technique_desc.is_lit.get()),
                    diffuse_image,
                    geometry: Some(geom.geometry.clone()),
                });
            }
        }
    }
    out
}

fn find_camera_shrink(behaviours: &[or_particle::behaviour::Behaviour]) -> f32 {
    use or_particle::behaviour::Behaviour;
    for b in behaviours {
        if let Behaviour::DrawModel(d) = b {
            return d.camera_shrink.max(0.0);
        }
    }
    0.0
}

/// What a model particle `distance` from the camera is scaled by, and its alpha multiplied by: one within the shrink distance and the distance over the shrink distance beyond it. A shrink of nothing turns it off.
fn camera_shrink_factor(distance: f32, shrink: f32) -> f32 {
    if shrink > 1e-4 && distance > shrink {
        distance / shrink
    } else {
        1.0
    }
}

fn build_instances_for_indices(
    buf: &or_simulation::PointBuffer,
    camera_shrink: f32,
    camera_pos: glam::Vec3,
    indices: &[usize],
) -> Vec<ModelInstance> {
    let mut out = Vec::with_capacity(indices.len());
    for &i in indices {
        let factor = camera_shrink_factor((buf.curr_pos[i] - camera_pos).length(), camera_shrink);
        out.push(ModelInstance {
            position: [buf.curr_pos[i].x, buf.curr_pos[i].y, buf.curr_pos[i].z],
            rotation: [
                buf.curr_rot[i].x,
                buf.curr_rot[i].y,
                buf.curr_rot[i].z,
                buf.curr_rot[i].w,
            ],
            // The size the model's box is drawn at: the renderer divides by the box
            scale: [
                (buf.dimensions[i].x * factor).max(0.001),
                (buf.dimensions[i].y * factor).max(0.001),
                (buf.dimensions[i].z * factor).max(0.001),
            ],
            color: [
                buf.color[i].x,
                buf.color[i].y,
                buf.color[i].z,
                // Saturated, since a vertex colour past one blends as nothing the pass means
                (buf.color[i].w * buf.alpha_mult[i] * factor).min(1.0),
            ],
        });
    }
    out
}

/// The technique of the mesh effect a model is drawn with. A model rule names a technique of the sprite effect, which the mesh effect lacks, so what it says of the light is what picks: the forward techniques of the mesh effect are named for how many lights they take, and a model has none besides the sun.
fn model_technique(lit: bool) -> String {
    if lit {
        "lightweight0_draw".into()
    } else {
        "unlit_draw".into()
    }
}

#[cfg(test)]
mod tests {
    use super::camera_shrink_factor;

    #[test]
    fn a_model_is_unscaled_within_the_shrink_distance_and_grows_with_the_distance_past_it() {
        assert_eq!(camera_shrink_factor(1.0, 2.0), 1.0);
        assert_eq!(camera_shrink_factor(2.0, 2.0), 1.0);
        assert_eq!(camera_shrink_factor(8.0, 2.0), 4.0);
        // A shrink distance of nothing is the setting that turns the factor off
        assert_eq!(camera_shrink_factor(50.0, 0.0), 1.0);
    }

    #[test]
    fn a_model_rule_draws_the_gta6_drawable_a_search_found_in_its_own_box() {
        use crate::state::gta6_particle_models::{found_for_tests, one_triangle};
        const HASH: u32 = 0x3A0D_E15F;
        let mut cache = super::ParticleModelMeshCache::new();
        let list = or_io::gta5::PtxFxList::default();
        assert!(cache.get_or_build(&list, HASH).is_none(), "none found");
        found_for_tests(HASH, one_triangle([1.0, 2.0, 3.0]));
        let drawn = cache.get_or_build(&list, HASH);
        let geoms = drawn.expect("the found drawable is drawn");
        assert_eq!(geoms.len(), 1);
        assert_eq!(geoms[0].geometry.indices.len(), 3);
        assert_eq!(geoms[0].geometry.size, [1.0, 2.0, 3.0]);
    }
}
