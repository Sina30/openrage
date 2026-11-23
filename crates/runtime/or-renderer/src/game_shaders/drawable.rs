//! A drawable extracted to a scene, as the surfaces and materials the game's passes draw.

use std::collections::BTreeMap;

use or_interchange::extract::{bones_of, Extracted};
use or_interchange::{
    AlphaMode, ExtractOptions, Material as SceneMaterial, Primitive, Scene, SceneBuilder,
};
use or_io::codec::texture::{decode_level, TextureSource};
use or_io::gta5::{DrawableTarget, FragDrawable, FragType};

use glam::{Mat4, Quat, Vec3};

use super::mesh::{bounds, Coverage, Image, Material, Surface};

/// The `GrcFvfChannel` numbers of the first texture coordinate and the first colour.
const FIRST_TEX_CHANNEL: u8 = 6;
const FIRST_COLOUR_CHANNEL: u8 = 4;

/// The effect a material's shader names: the file stem, in the case the pack keys it by.
pub fn effect_of(shader: &str) -> String {
    let name = shader.rsplit(['/', '\\']).next().unwrap_or(shader);
    name.strip_suffix(".sps").unwrap_or(name).to_string()
}

/// The surface a primitive draws, its channels fed to the semantics of their own numbers.
pub fn surface_of(primitive: &Primitive) -> Surface {
    let by_semantic = |channels: &[u8], first: u8| {
        channels
            .iter()
            .map(|c| u32::from(c.saturating_sub(first)))
            .collect::<Vec<_>>()
    };
    let tex = by_semantic(&primitive.uv_channels, FIRST_TEX_CHANNEL);
    let colour = by_semantic(&primitive.colour_channels, FIRST_COLOUR_CHANNEL);
    Surface {
        positions: primitive.positions.clone(),
        normals: primitive.normals.clone(),
        tangents: primitive.tangents.clone(),
        uvs: tex
            .into_iter()
            .zip(&primitive.uvs)
            .map(|(semantic, values)| (semantic, values.clone()))
            .collect(),
        colours: colour
            .into_iter()
            .zip(&primitive.colours)
            .map(|(semantic, values)| (semantic, values.clone()))
            .collect(),
        weights: primitive.weights.clone(),
        joints: primitive.joints.clone(),
        indices: primitive.indices.clone(),
        skinned: primitive.is_skinned(),
    }
}

/// A PNG as the pixels of an image.
pub fn image_of(png: &[u8]) -> Option<Image> {
    let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let rgba = decoded.to_rgba8();
    Some(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
        // A PNG in a scene is a drawable's own texture, whose colour space it does not carry
        srgb: false,
    })
}

/// Textures by lower-case name.
pub type Textures = BTreeMap<String, Image>;

/// The first level of a texture, in the colour space it states; `None` where it will not decode.
pub fn first_level(texture: &dyn TextureSource) -> Option<Image> {
    let level = decode_level(texture, 0).ok()?;
    Some(Image {
        width: level.width,
        height: level.height,
        rgba: level.data,
        srgb: level.is_srgb,
    })
}

/// The first level of each texture by lower-case name; one that will not decode is left out.
pub fn first_levels<'a>(
    textures: impl Iterator<Item = (String, &'a dyn TextureSource)>,
) -> Textures {
    textures
        .filter_map(|(name, texture)| Some((name.to_ascii_lowercase(), first_level(texture)?)))
        .collect()
}

/// Which side of a vehicle a wheel bone is on and whether it is a front one, from its name: `wheel_lf`, `wheel_rr`, `wheel_lm1` of a truck, `wheel_f` of a bike; `None` for a bone that is no wheel.
fn wheel_place(bone: &str) -> Option<(Side, bool)> {
    let rest = bone.strip_prefix("wheel_")?;
    let (side, position) = match rest.as_bytes() {
        [b'l', _, ..] => (Side::Left, &rest[1..]),
        [b'r', _, ..] => (Side::Right, &rest[1..]),
        _ => (Side::Middle, rest),
    };
    Some((side, position.starts_with('f')))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Left,
    Middle,
    Right,
}

/// The matrix of bone `at` in the frame of the skeleton: its own transform under its parents'.
fn bone_matrix(bones: &[or_interchange::Bone], at: usize) -> Mat4 {
    let mut matrix = Mat4::IDENTITY;
    let mut next = Some(at);
    // A skeleton whose parents loop is cut at its own length
    for _ in 0..=bones.len() {
        let Some(bone) = next.and_then(|i| bones.get(i)) else {
            break;
        };
        matrix = Mat4::from_scale_rotation_translation(
            Vec3::from(bone.scale),
            Quat::from_array(bone.rotation),
            Vec3::from(bone.translation),
        ) * matrix;
        next = bone.parent;
    }
    matrix
}

/// The wheel drawables a vehicle fragment ships, each with the matrix that puts it at a wheel bone of the vehicle.
///
/// A vehicle ships one wheel model (the left front, and a left rear as well for a bike or a truck) and the game draws it at every wheel bone, turned half about the vertical where it stands on the other side of the vehicle.
pub fn wheel_placements<T: DrawableTarget>(
    fragment: &FragType<T>,
) -> Vec<(&FragDrawable<T>, Mat4)> {
    let Some(skeleton) = fragment
        .common_drawable
        .as_deref()
        .and_then(|d| d.base.skeleton.as_deref())
    else {
        return Vec::new();
    };
    let Some(lod) = fragment
        .physics_lod_group
        .as_deref()
        .and_then(|g| g.lods().next())
    else {
        return Vec::new();
    };
    let bones = bones_of(skeleton);
    // The wheel models the fragment ships, by the bone of the child that holds each
    let models: Vec<(usize, &FragDrawable<T>)> = lod
        .children
        .iter()
        .filter_map(|child| {
            let at = bones.iter().position(|b| b.tag == child.bone_id)?;
            wheel_place(&bones[at].name)?;
            let entity = child.undamaged_entity.as_deref()?;
            let drawn = (entity.base.lod_group.lods.iter())
                .any(|level| level.models.iter().any(|m| !m.geometries.is_empty()));
            drawn.then_some((at, entity))
        })
        .collect();
    let mut placed = Vec::new();
    for (at, bone) in bones.iter().enumerate() {
        let Some((side, front)) = wheel_place(&bone.name) else {
            continue;
        };
        // Its own model where it has one, else a front wheel's for a front bone and a rear one's for the rest, whichever the vehicle ships
        let own = models.iter().find(|(m, _)| *m == at);
        let by_place = |want: bool| {
            models
                .iter()
                .find(|(m, _)| wheel_place(&bones[*m].name).is_some_and(|(_, f)| f == want))
        };
        let Some((model_at, entity)) = own.or_else(|| by_place(front)).or_else(|| models.first())
        else {
            continue;
        };
        let model_side = wheel_place(&bones[*model_at].name).map(|(s, _)| s);
        let opposite = matches!(
            (model_side, side),
            (Some(Side::Left), Side::Right) | (Some(Side::Right), Side::Left)
        );
        let turned = if opposite {
            Mat4::from_rotation_z(std::f32::consts::PI)
        } else {
            Mat4::IDENTITY
        };
        placed.push((*entity, bone_matrix(&bones, at) * turned));
    }
    placed
}

/// The wheels a vehicle fragment is drawn with, each as a scene of its own with the matrix that puts it at its wheel bone.
pub fn fragment_wheels<T: Extracted>(
    fragment: &FragType<T>,
    options: &ExtractOptions,
) -> Vec<(Scene, Mat4)> {
    let Some(common) = fragment.common_drawable.as_deref().map(|d| &d.base) else {
        return Vec::new();
    };
    wheel_placements(fragment)
        .into_iter()
        .map(|(entity, at)| {
            // A child has no shader group of its own and indexes the vehicle's
            let mut builder = SceneBuilder::new("wheel");
            let base = builder.material_count();
            if entity.base.shader_group.is_some() {
                builder.push("wheel", &entity.base, options);
            } else {
                builder.push_materials(common, options);
                builder.push_sharing("wheel", &entity.base, base, options);
            }
            (builder.finish(), at)
        })
        .collect()
}

/// `surfaces` carried by `at`: positions as points, normals and tangents as directions.
pub fn carried(surfaces: &mut [(Surface, Material)], at: Mat4) {
    for (surface, _) in surfaces {
        for p in &mut surface.positions {
            *p = at.transform_point3((*p).into()).into();
        }
        for n in &mut surface.normals {
            *n = at.transform_vector3((*n).into()).normalize_or_zero().into();
        }
        for t in &mut surface.tangents {
            let [x, y, z] = at
                .transform_vector3(Vec3::new(t[0], t[1], t[2]))
                .normalize_or_zero()
                .into();
            *t = [x, y, z, t[3]];
        }
    }
}

/// A flat ground of a mild grey with its top at `centre`, `2 * half` wide, for light and a shadow to land on.
pub fn ground(centre: Vec3, half: f32) -> (Surface, Material) {
    let corners = [[-half, -half], [half, -half], [half, half], [-half, half]];
    let surface = Surface {
        positions: corners
            .iter()
            .map(|c| [centre.x + c[0], centre.y + c[1], centre.z])
            .collect(),
        normals: vec![[0.0, 0.0, 1.0]; 4],
        uvs: [(0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])].into(),
        indices: vec![0, 1, 2, 0, 2, 3],
        ..Surface::default()
    };
    let grey = Image {
        width: 1,
        height: 1,
        rgba: vec![64, 64, 64, 255],
        srgb: false,
    };
    let material = Material {
        effect: "default".into(),
        vectors: vec![
            ("specularIntensityMult".into(), vec![[0.1, 0.0, 0.0, 0.0]]),
            ("specularFalloffMult".into(), vec![[20.0, 0.0, 0.0, 0.0]]),
            ("reflectivePower".into(), vec![[0.0; 4]]),
        ],
        textures: [("DiffuseSampler".to_string(), grey)].into(),
        ..Material::default()
    };
    (surface, material)
}

/// A ground under `surfaces`, a little below their lowest point so that nothing that rests on it fights it for the pixel, and as wide as three of them are.
pub fn lay_ground_under(surfaces: &mut Vec<(Surface, Material)>) {
    let Some((min, max)) = bounds(surfaces) else {
        return;
    };
    let size = max - min;
    let centre = Vec3::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        min.z - 0.002 * size.max_element(),
    );
    surfaces.push(ground(centre, 1.5 * size.x.max(size.y)));
}

/// The colours a vehicle takes from its colour combination for the slots a material names, which a preview with no combination to choose takes from here: the body and its kin white, the wheels silver, the interior and the dashboard dark.
const SLOT_COLOURS: [[f32; 3]; 7] = [
    [1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.55, 0.55, 0.58],
    [0.12, 0.12, 0.13],
    [0.08, 0.08, 0.09],
    [0.9, 0.9, 0.9],
];

/// Whether a parameter is `wanted`: by its name, or by its hash, which is what a parameter no shader database names reads as.
fn is_named(name: &str, wanted: &str) -> bool {
    name == wanted || name == format!("0x{:08X}", or_io::hash::string_hash_str(wanted))
}

/// The colour number `k` a material's `matDiffuseColor` row stands for, when it is a marker `(2, k, k)` and not a colour.
fn marker_slot(row: &[f32; 4]) -> Option<usize> {
    let k = row[1];
    let marker = row[0] == 2.0 && row[2] == k && k.fract() == 0.0 && (1.0..=7.0).contains(&k);
    marker.then_some(k as usize)
}

/// A vehicle material's `matDiffuseColor` is not a colour but a marker, `(2, k, k)` for the vehicle's colour number `k`, which the game replaces with that colour as the vehicle is made; left as it is, the interior marker is a colour five times too bright in green and blue.
fn vehicle_colours(name: &str, rows: &[[f32; 4]]) -> Vec<[f32; 4]> {
    rows.iter()
        .map(
            |row| match (is_named(name, "matDiffuseColor"), marker_slot(row)) {
                (true, Some(k)) => {
                    let [r, g, b] = SLOT_COLOURS[k - 1];
                    [r, g, b, row[3]]
                }
                _ => *row,
            },
        )
        .collect()
}

/// What the game sets a body paint's specular to from the metallic setting of its colour, the first of the table every colour of the common kind takes: intensity, falloff and Fresnel. The file's own values are placeholders, and an intensity of twenty leaves the paint no diffuse light.
const BODY_METALLIC: [(&str, f32); 3] = [
    ("specularIntensityMult", 0.8),
    ("specularFalloffMult", 510.0),
    ("specularFresnel", 0.96),
];

/// A vehicle material's values as the game makes them: the colour markers replaced by their colours, and a body paint's specular set to its metallic setting.
fn vehicle_vectors(vectors: &[(String, Vec<[f32; 4]>)]) -> Vec<(String, Vec<[f32; 4]>)> {
    let mut made: Vec<_> = (vectors.iter())
        .map(|(name, rows)| (name.clone(), vehicle_colours(name, rows)))
        .collect();
    let body = vectors.iter().any(|(name, rows)| {
        is_named(name, "matDiffuseColor")
            && rows.first().and_then(marker_slot).is_some_and(|k| k <= 3)
    });
    if body {
        for (wanted, value) in BODY_METALLIC {
            match made.iter_mut().find(|(name, _)| is_named(name, wanted)) {
                Some((_, rows)) => rows[0][0] = value,
                None => made.push((wanted.to_string(), vec![[value, 0.0, 0.0, 0.0]])),
            }
        }
    }
    made
}

/// The material a scene's material makes: its effect, its values and the textures it has pixels for.
pub fn material_of(scene: &Scene, material: &SceneMaterial, beside: &Textures) -> Material {
    let mut textures = BTreeMap::new();
    for binding in &material.textures {
        // The drawable's own dictionary first, then the one beside it by the name the material gives
        let own = binding
            .image
            .and_then(|i| scene.images.get(i))
            .and_then(|image| image_of(&image.png));
        let found = own.or_else(|| beside.get(&binding.texture.to_ascii_lowercase()).cloned());
        if let Some(image) = found {
            textures.insert(binding.parameter.clone(), image);
        }
    }
    Material {
        effect: effect_of(&material.shader),
        vectors: vehicle_vectors(&material.vectors),
        textures,
        coverage: match material.alpha_mode {
            // Water is lit by its own pass over the frame, which has no deferred one
            AlphaMode::Opaque if effect_of(&material.shader).starts_with("water") => {
                Coverage::Blended
            }
            AlphaMode::Opaque => Coverage::Opaque,
            AlphaMode::Mask => Coverage::Cutout,
            AlphaMode::Blend => Coverage::Blended,
        },
        pass: 0,
        views: Default::default(),
    }
}

/// Every primitive of `scene` as a surface and the material it draws with, a texture its dictionary lacks taken from `beside`.
pub fn surfaces_of(scene: &Scene, beside: &Textures) -> Vec<(Surface, Material)> {
    scene
        .primitives()
        .map(|primitive| {
            let material = primitive
                .material
                .and_then(|i| scene.materials.get(i))
                .map(|m| material_of(scene, m, beside))
                .unwrap_or_default();
            (surface_of(primitive), material)
        })
        .collect()
}

#[cfg(test)]
mod tests;
