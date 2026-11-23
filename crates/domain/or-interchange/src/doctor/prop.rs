//! A ped component made into a prop: the same mesh, worn at the bone a prop hangs from, built the way the game's own props are.
//!
//! ⚠ **The game's props are rigid and share one material and one vertex.** Measured over the 166 `p_head` models of the stock peds: every one draws with
//! `shader_34D90761` (opaque, bucket 0) or `shader_44789AA6` (alpha, bucket 1), and every vertex is position, normal, both colours, two texture
//! coordinates and a tangent, with no weight, no binding and no palette. A component is skinned, so it is carried into a stock prop's frame (see
//! [`super::place`]), rebuilt in the layout of a stock prop, given a shader of a stock prop and stripped of its skin.
//!
//! ⚠ A shader a stock prop uses is taken whole (its constants too), and only the textures the component binds are carried over to it: the diffuse, the
//! normal and the specular map. A texture the component does not bind is bound by name, since a stock prop's own maps are not embedded here.

use or_io::codec::mesh::GrcFvfChannel;
use or_io::decl::Gen8;
use or_io::error::{IoError, Result};
use or_io::gta5::drawable_codecs::gen8::EntryData;
use or_io::gta5::drawable_codecs::TextureSlot;
use or_io::gta5::{gen8, RmcDrawable};
use or_io::hash::string_hash_str;

use super::place::{place_scene, Frame};
use crate::extract::{ExtractOptions, LodSelection, SceneBuilder};
use crate::import::{replace_geometry_named, ImportNames, ImportOptions};
use crate::scene::Primitive;

/// What a stock prop's shader binds when the component binds nothing there.
const FLAT_NORMAL: &str = "blank_normal";
const NEUTRAL_SPECULAR: &str = "carbon_mesh_shader_spec";

/// What the conversion did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropReport {
    pub vertices: usize,
    /// The shader each material of the component became: `opaque` or `alpha`.
    pub shaders: Vec<&'static str>,
    /// `lod0/model0/geometry0: Specular`, once per channel the component did not state.
    pub filled: Vec<String>,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

fn refuse<T>(context: impl Into<String>) -> Result<T> {
    Err(IoError::InvalidData {
        context: context.into(),
    })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let l = dot(v, v).sqrt();
    (l > 1e-12).then(|| [v[0] / l, v[1] / l, v[2] / l])
}

/// The tangent of each vertex from its texture coordinates: the direction `u` grows in, turned square to the normal, with the sign of the bitangent.
pub fn tangents_of(p: &Primitive) -> Vec<[f32; 4]> {
    let n = p.positions.len();
    let uv = &p.uvs[0];
    let mut tan = vec![[0f32; 3]; n];
    let mut bit = vec![[0f32; 3]; n];
    for t in p.indices.as_chunks::<3>().0 {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (e1, e2) = (
            sub(p.positions[b], p.positions[a]),
            sub(p.positions[c], p.positions[a]),
        );
        let (du1, dv1) = (uv[b][0] - uv[a][0], uv[b][1] - uv[a][1]);
        let (du2, dv2) = (uv[c][0] - uv[a][0], uv[c][1] - uv[a][1]);
        let r = du1 * dv2 - du2 * dv1;
        if r.abs() < 1e-20 {
            continue;
        }
        let f = 1.0 / r;
        for k in 0..3 {
            let (tk, bk) = (
                (e1[k] * dv2 - e2[k] * dv1) * f,
                (e2[k] * du1 - e1[k] * du2) * f,
            );
            for v in [a, b, c] {
                tan[v][k] += tk;
                bit[v][k] += bk;
            }
        }
    }
    (0..n)
        .map(|v| {
            let normal = p.normals.get(v).copied().unwrap_or([0.0, 0.0, 1.0]);
            let off = dot(normal, tan[v]);
            let square = [
                tan[v][0] - normal[0] * off,
                tan[v][1] - normal[1] * off,
                tan[v][2] - normal[2] * off,
            ];
            // A vertex no triangle with texture area reaches takes any direction square to its normal.
            let t = unit(square).unwrap_or_else(|| {
                let axis = if normal[0].abs() < 0.9 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                };
                unit(cross(normal, axis)).unwrap_or([1.0, 0.0, 0.0])
            });
            // ⚠ The game's sign is the opposite of the usual one: the stock props' 780 vertices of `p_head_000` all agree with the tangents rebuilt here, in direction and in this sign.
            let w = if dot(cross(normal, t), bit[v]) < 0.0 {
                1.0
            } else {
                -1.0
            };
            [t[0], t[1], t[2], w]
        })
        .collect()
}

/// A slot that binds `name` by name.
fn by_name(name: &str) -> TextureSlot<gen8::GrcTexture, gen8::GrcTextureReference> {
    TextureSlot::Reference(Box::new(gen8::GrcTextureReference::named(name)))
}

/// The first shader of a stock prop that draws in the opaque bucket, or in another one where `opaque` is false.
fn shader_of(stock: &RmcDrawable<Gen8>, opaque: bool) -> Result<&gen8::GrcInstanceData> {
    stock
        .shader_group
        .as_deref()
        .and_then(|g| g.shaders.iter().find(|s| (s.draw_bucket == 0) == opaque))
        .map(|s| &**s)
        .ok_or_else(|| IoError::InvalidData {
            context: format!(
                "the {} stock prop has no such shader",
                if opaque { "opaque" } else { "alpha" }
            ),
        })
}

/// `component` as a prop worn at the bone `frame` stands for, drawn with the shaders of `opaque` and `alpha` (stock props) and laid out as `opaque` is.
pub fn as_prop(
    component: &RmcDrawable<Gen8>,
    opaque: &RmcDrawable<Gen8>,
    alpha: &RmcDrawable<Gen8>,
    frame: &Frame,
) -> Result<(RmcDrawable<Gen8>, PropReport)> {
    let (opaque_shader, alpha_shader) = (shader_of(opaque, true)?, shader_of(alpha, false)?);
    let stock_model = opaque
        .lod_group
        .lods
        .iter()
        .find_map(|l| l.models.first())
        .ok_or_else(|| IoError::InvalidData {
            context: "the opaque stock prop has no model".into(),
        })?;
    let stock_geometry = stock_model
        .geometries
        .first()
        .ok_or_else(|| IoError::InvalidData {
            context: "the opaque stock prop has no geometry".into(),
        })?;

    // The mesh, in the frame of the bone, with the vertex a prop has.
    let mut builder = SceneBuilder::new(&component.name);
    builder.push(
        &component.name,
        component,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    let mut scene = builder.finish();
    let placed = place_scene(&mut scene, frame)?;
    for object in &mut scene.objects {
        for p in object
            .meshes
            .iter_mut()
            .flat_map(|m| m.primitives.iter_mut())
        {
            let Some(uv) = p.uvs.first().cloned() else {
                return refuse("a geometry of the component has no texture coordinates");
            };
            p.joints.clear();
            p.weights.clear();
            p.tangents = tangents_of(p);
            p.uvs = vec![uv.clone(), uv];
            p.uv_channels = vec![GrcFvfChannel::Tex0 as u8, GrcFvfChannel::Tex1 as u8];
        }
    }

    // The component's geometries in a stock prop's buffer: its declaration, and no bone to skin against.
    let mut target = component.clone();
    for lod in target.lod_group.lods.iter_mut() {
        for model in lod.models.iter_mut() {
            for geometry in model.geometries.iter_mut() {
                geometry.vertex_buffer = stock_geometry.vertex_buffer.clone();
                geometry.matrix_palette.clear();
                geometry.take_layout_from_buffer();
            }
            model.matrix_count = stock_model.matrix_count;
            model.flags = stock_model.flags;
            model.geom_type = stock_model.geom_type;
            model.matrix_index = stock_model.matrix_index;
            model.mask = stock_model.mask;
            model.skin_flag_and_tess = stock_model.skin_flag_and_tess;
        }
    }
    let options = ImportOptions {
        fill_missing: true,
        keep_skinning: false,
    };
    let (mut made, imported) =
        replace_geometry_named(&target, &scene, &options, &ImportNames::default())?;
    made.skeleton = None;

    // The shaders of a stock prop, with the component's own textures.
    let mut report = PropReport {
        vertices: placed.vertices,
        filled: imported.channels_filled,
        min: placed.min,
        max: placed.max,
        ..PropReport::default()
    };
    let group = made
        .shader_group
        .as_deref_mut()
        .ok_or_else(|| IoError::InvalidData {
            context: "the component has no shader group".into(),
        })?;
    let (diffuse, bump, spec) = (
        string_hash_str("DiffuseSampler"),
        string_hash_str("BumpSampler"),
        string_hash_str("SpecSampler"),
    );
    let was = std::mem::take(&mut group.shaders);
    let mut any_alpha = false;
    for shader in &was {
        let (template, kind) = match shader.draw_bucket {
            0 => (opaque_shader, "opaque"),
            _ => (alpha_shader, "alpha"),
        };
        any_alpha |= kind == "alpha";
        report.shaders.push(kind);
        let mut now = template.clone();
        for entry in &mut now.entries {
            let EntryData::Texture(slot) = &mut entry.data else {
                continue;
            };
            let hash = entry.name_hash.get();
            // Only the three maps a component binds are carried over; any other sampler stays as the stock prop has it.
            if hash != diffuse && hash != bump && hash != spec {
                if let TextureSlot::Embedded(_) = slot {
                    *slot = TextureSlot::Unbound;
                }
                continue;
            }
            let brought = shader.entries.iter().find_map(|e| match &e.data {
                EntryData::Texture(s)
                    if e.name_hash.get() == hash && !matches!(s, TextureSlot::Unbound) =>
                {
                    Some(s.clone())
                }
                _ => None,
            });
            let current = slot.clone();
            *slot = match (brought, hash) {
                (Some(own), _) => own,
                (None, h) if h == bump => by_name(FLAT_NORMAL),
                (None, h) if h == spec => by_name(NEUTRAL_SPECULAR),
                // The picture is the variation's and the game binds it over this one: the stock prop's own stub stays, unless it is an embedded map.
                (None, _) => match current {
                    TextureSlot::Embedded(_) => by_name("givemechecker"),
                    other => other,
                },
            };
        }
        group.shaders.push(Box::new(now));
    }
    made.lod_group.bucket_masks = opaque.lod_group.bucket_masks;
    if any_alpha {
        for (mask, alpha_mask) in made
            .lod_group
            .bucket_masks
            .iter_mut()
            .zip(alpha.lod_group.bucket_masks)
        {
            *mask |= alpha_mask;
        }
    }

    // The drawable's own box and sphere, from the mesh where it now stands.
    if placed.vertices > 0 {
        let b = &mut made.lod_group.bounds_box;
        (b.min.x, b.min.y, b.min.z) = (placed.min[0], placed.min[1], placed.min[2]);
        (b.max.x, b.max.y, b.max.z) = (placed.max[0], placed.max[1], placed.max[2]);
        let centre = [
            (placed.min[0] + placed.max[0]) / 2.0,
            (placed.min[1] + placed.max[1]) / 2.0,
            (placed.min[2] + placed.max[2]) / 2.0,
        ];
        let radius = (0..3)
            .map(|k| ((placed.max[k] - placed.min[k]) / 2.0).powi(2))
            .sum::<f32>()
            .sqrt();
        let s = &mut made.lod_group.bounds_sphere.center_and_radius;
        (s.x, s.y, s.z, s.w) = (centre[0], centre[1], centre[2], radius);
    }
    Ok((made, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_square_has_the_tangent_u_grows_along() {
        let p = Primitive {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]],
            indices: vec![0, 1, 2],
            ..Primitive::default()
        };
        let t = tangents_of(&p);
        assert!(t
            .iter()
            .all(|v| (v[0] - 1.0).abs() < 1e-6 && v[1].abs() < 1e-6 && v[3] == -1.0));
    }
}
