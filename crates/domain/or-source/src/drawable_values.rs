//! What a drawable says that glTF cannot, as `drawable.json` states it and a compile puts it back.
//!
//! An extraction reads each of these from the base and writes it down; a compile reads the file
//! and, wherever a value differs from the base's, sets it there and nothing else. A source written
//! before they existed has none of them, and a compile of it leaves the base's all as they were.
//! The shader a material draws with, the vertex declaration of its geometry and the number of
//! geometries are the base's and stay so: choosing them is authoring, which this build does not do.

use std::collections::{BTreeMap, BTreeSet};

use or_interchange::extract::{effect_name, parameter_name};
use or_interchange::{ImportNames, LodSlot};
use or_io::decl::{Gen8, Gen9, Vec4};
use or_io::gta5::drawable_codecs::gen8::EntryData;
use or_io::gta5::drawable_codecs::TextureSlot;
use or_io::gta5::{gen8, gen9, GrmModel, GrmShaderGroup, RmcDrawable, SpdAABB};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

use crate::models::Rules;
use crate::Problem;

/// A number as the shortest text that reads back as the same `f32`, so a file a person edits is not full of `0.30000001192092896`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct Real(pub f32);

impl Serialize for Real {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let shortest = self.0.to_string().parse::<f64>();
        serializer.serialize_f64(shortest.unwrap_or_else(|_| f64::from(self.0)))
    }
}

/// A parameter of a material: the name of the texture it binds, or its rows of numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Parameter {
    /// The texture's name, which is the DDS a dictionary supplies; empty where the slot is unbound.
    Texture(String),
    /// A constant, one row of up to four numbers per register.
    Vectors(Vec<Vec<Real>>),
}

/// One shader of the base, with the values it states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Material {
    /// What a glb calls this material. An export names it for its shader, so two materials of one shader share a name.
    pub name: String,
    /// The shader the base draws with, by name. A compile refuses a different one.
    pub shader: String,
    pub draw_bucket: u8,
    pub draw_bucket_mask: u32,
    /// A Legacy shader's own flag words; an Enhanced one has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_flags: Option<u8>,
    /// By the name the shader database gives a parameter, else `0x` and the eight digits of its hash.
    #[serde(default)]
    pub parameters: BTreeMap<String, Parameter>,
}

/// One model of the base: the name its mesh goes by in the glb, and its words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelRule {
    /// `lod0/model0`: the level and the model in it.
    pub at: String,
    /// What the glb calls this model's mesh, which an import pairs it by.
    pub name: String,
    pub flags: u8,
    pub mask: u8,
}

/// `spdSphere`: the centre and the radius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sphere {
    pub center: [Real; 3],
    pub radius: Real,
}

/// `spdAABB`: two corners. The fourth lane of each stays the base's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Extent {
    pub min: [Real; 3],
    pub max: [Real; 3],
}

/// One bone of the skeleton the drawable carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoneRule {
    /// The base's name for the bone, which a compile checks and never changes.
    pub name: String,
    /// What an animation addresses the bone by (`bone_id` of `crBoneData`); no two bones share one.
    pub tag: u16,
}

/// The two words a target's shader differs in, and the way each lays its parameters out.
pub(crate) trait Edited: or_interchange::import::Imported {
    /// The material `shader` states, in the words of `drawable.json`.
    fn material(shader: &Self::InstanceData, group: &GrmShaderGroup<Self>) -> Material;

    /// Puts what `material` states into shader `index` of `group`, and names what changed.
    fn set_material(
        group: &mut GrmShaderGroup<Self>,
        index: usize,
        material: &Material,
    ) -> Result<Vec<String>, String>;
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}

/// A parameter's hash from the key `drawable.json` states it by.
fn parameter_hash(key: &str) -> u32 {
    key.strip_prefix("0x")
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .unwrap_or_else(|| or_io::hash::string_hash_str(key))
}

/// The key a parameter is written under: its name where the name hashes back to it, else its hash.
fn parameter_key(effect: u32, hash: u32) -> String {
    let name = parameter_name(effect, hash);
    if parameter_hash(&name) == hash {
        name
    } else {
        format!("0x{hash:08X}")
    }
}

fn rows_of(floats: &[f32]) -> Option<Parameter> {
    finite(floats).then(|| {
        Parameter::Vectors(
            floats
                .chunks(4)
                .map(|row| row.iter().copied().map(Real).collect())
                .collect(),
        )
    })
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}

impl Edited for Gen8 {
    fn material(shader: &gen8::GrcInstanceData, group: &GrmShaderGroup<Gen8>) -> Material {
        let effect = shader.basis_hash.get();
        let mut parameters = BTreeMap::new();
        for entry in &shader.entries {
            let hash = entry.name_hash.get();
            let value = match &entry.data {
                EntryData::Texture(slot) => {
                    Parameter::Texture(group.slot_name(slot).unwrap_or("").to_string())
                }
                EntryData::Float(rows) => {
                    let floats: Vec<f32> = rows.iter().flat_map(|v| [v.x, v.y, v.z, v.w]).collect();
                    match rows_of(&floats) {
                        Some(rows) => rows,
                        None => continue,
                    }
                }
            };
            parameters
                .entry(parameter_key(effect, hash))
                .or_insert(value);
        }
        let name = effect_name(effect);
        Material {
            name: name.clone(),
            shader: name,
            draw_bucket: shader.draw_bucket,
            draw_bucket_mask: shader.draw_bucket_mask,
            flags: Some(shader.flags),
            user_flags: Some(shader.user_flags),
            parameters,
        }
    }

    fn set_material(
        group: &mut GrmShaderGroup<Gen8>,
        index: usize,
        material: &Material,
    ) -> Result<Vec<String>, String> {
        // Every value is checked against the shader before any is set, so a refusal leaves it as it was.
        let mut vectors: Vec<(u32, String, Vec<Vec4>)> = Vec::new();
        let mut bindings = Vec::new();
        {
            let shader = &group.shaders[index];
            for (key, value) in &material.parameters {
                let hash = parameter_hash(key);
                let Some(entry) = shader.entry(hash) else {
                    return Err(unknown_parameter(key));
                };
                match (value, &entry.data) {
                    (Parameter::Vectors(rows), EntryData::Float(held)) => {
                        if rows.len() != held.len() || rows.iter().any(|r| r.len() != 4) {
                            return Err(format!(
                                "parameter {key} holds {} rows of four numbers",
                                held.len()
                            ));
                        }
                        let new: Vec<Vec4> = rows
                            .iter()
                            .map(|r| Vec4 {
                                x: r[0].0,
                                y: r[1].0,
                                z: r[2].0,
                                w: r[3].0,
                            })
                            .collect();
                        let flat = |rows: &[Vec4]| -> Vec<f32> {
                            rows.iter().flat_map(|v| [v.x, v.y, v.z, v.w]).collect()
                        };
                        if !same_bits(&flat(&new), &flat(held)) {
                            vectors.push((hash, key.clone(), new));
                        }
                    }
                    (Parameter::Texture(name), EntryData::Texture(slot)) => {
                        if name != group.slot_name(slot).unwrap_or("") {
                            let new = if name.is_empty() {
                                TextureSlot::Unbound
                            } else {
                                group.binding(name)
                            };
                            bindings.push((hash, key.clone(), name.clone(), new));
                        }
                    }
                    (Parameter::Vectors(_), EntryData::Texture(_)) => {
                        return Err(format!("parameter {key} is a texture and not numbers"))
                    }
                    (Parameter::Texture(_), EntryData::Float(_)) => {
                        return Err(format!("parameter {key} is numbers and not a texture"))
                    }
                }
            }
        }

        let shader = &mut group.shaders[index];
        let mut changed = Vec::new();
        if material.draw_bucket != shader.draw_bucket {
            shader.draw_bucket = material.draw_bucket;
            changed.push(format!("draw_bucket {}", material.draw_bucket));
        }
        if material.draw_bucket_mask != shader.draw_bucket_mask {
            shader.draw_bucket_mask = material.draw_bucket_mask;
            changed.push(format!("draw_bucket_mask {}", material.draw_bucket_mask));
        }
        if let Some(flags) = material.flags.filter(|f| *f != shader.flags) {
            shader.flags = flags;
            changed.push(format!("flags {flags}"));
        }
        if let Some(flags) = material.user_flags.filter(|f| *f != shader.user_flags) {
            shader.user_flags = flags;
            changed.push(format!("user_flags {flags}"));
        }
        for (hash, key, rows) in vectors {
            if let Some(entry) = shader.entry_mut(hash) {
                entry.data = EntryData::Float(rows);
                changed.push(key);
            }
        }
        for (hash, key, name, slot) in bindings {
            if let Some(entry) = shader.entry_mut(hash) {
                entry.data = EntryData::Texture(slot);
                changed.push(if name.is_empty() {
                    format!("{key} unbound")
                } else {
                    format!("{key} bound to {name}")
                });
            }
        }
        Ok(changed)
    }
}

fn unknown_parameter(key: &str) -> String {
    format!(
        "parameter {key} is not one of the shader's, and adding or removing a parameter means \
         choosing a shader, which is authoring"
    )
}

impl Edited for Gen9 {
    fn material(shader: &gen9::GrcInstanceDataG9, group: &GrmShaderGroup<Gen9>) -> Material {
        let effect = shader.name.get();
        let mut parameters = BTreeMap::new();
        for info in &shader.param_infos.params {
            let hash = info.name.get();
            let value = if let Some(slot) = info.texture() {
                let Some(slot) = shader.params.textures.get(slot) else {
                    continue;
                };
                Parameter::Texture(group.slot_name(slot).unwrap_or("").to_string())
            } else if let Some((buffer, range)) = info.constant() {
                let Some(bytes) = shader.params.buffers.get(buffer).and_then(|b| b.get(range))
                else {
                    continue;
                };
                match rows_of(&floats_of(bytes)) {
                    Some(rows) => rows,
                    None => continue,
                }
            } else {
                continue;
            };
            parameters
                .entry(parameter_key(effect, hash))
                .or_insert(value);
        }
        let name = effect_name(effect);
        Material {
            name: name.clone(),
            shader: name,
            draw_bucket: shader.draw_bucket,
            draw_bucket_mask: shader.draw_bucket_mask,
            flags: None,
            user_flags: None,
            parameters,
        }
    }

    fn set_material(
        group: &mut GrmShaderGroup<Gen9>,
        index: usize,
        material: &Material,
    ) -> Result<Vec<String>, String> {
        if material.flags.is_some() || material.user_flags.is_some() {
            return Err(
                "flags and user_flags are words of a Legacy shader, and an Enhanced one has neither"
                    .to_string(),
            );
        }
        let mut writes: Vec<(u32, String, Vec<u8>)> = Vec::new();
        {
            let shader = &group.shaders[index];
            for (key, value) in &material.parameters {
                let hash = parameter_hash(key);
                let Some(info) = shader.param(hash) else {
                    return Err(unknown_parameter(key));
                };
                match value {
                    Parameter::Vectors(rows) => {
                        let Some((buffer, range)) = info.constant() else {
                            return Err(format!("parameter {key} is not numbers in the shader"));
                        };
                        let held = shader
                            .params
                            .buffers
                            .get(buffer)
                            .and_then(|b| b.get(range))
                            .map(floats_of)
                            .unwrap_or_default();
                        let flat: Vec<f32> = rows.iter().flatten().map(|r| r.0).collect();
                        if flat.len() != held.len() {
                            return Err(format!(
                                "parameter {key} holds {} numbers, and the shader states {}",
                                held.len(),
                                flat.len()
                            ));
                        }
                        if !same_bits(&flat, &held) {
                            let bytes = flat.iter().flat_map(|f| f.to_le_bytes()).collect();
                            writes.push((hash, key.clone(), bytes));
                        }
                    }
                    Parameter::Texture(name) => {
                        let bound = info
                            .texture()
                            .and_then(|slot| shader.params.textures.get(slot))
                            .and_then(|slot| group.slot_name(slot))
                            .unwrap_or("");
                        if name != bound {
                            return Err(format!(
                                "parameter {key} names the texture {name:?} where the shader binds \
                                 {bound:?}; an Enhanced material's reference states the size and \
                                 format of its texture, which a name cannot say, so it is not rebound"
                            ));
                        }
                    }
                }
            }
        }

        let shader = &mut group.shaders[index];
        let mut changed = Vec::new();
        if material.draw_bucket != shader.draw_bucket {
            shader.draw_bucket = material.draw_bucket;
            changed.push(format!("draw_bucket {}", material.draw_bucket));
        }
        if material.draw_bucket_mask != shader.draw_bucket_mask {
            shader.draw_bucket_mask = material.draw_bucket_mask;
            changed.push(format!("draw_bucket_mask {}", material.draw_bucket_mask));
        }
        for (hash, key, bytes) in writes {
            if let Some(held) = shader.constant_mut(hash) {
                held.copy_from_slice(&bytes);
                changed.push(key);
            }
        }
        Ok(changed)
    }
}

fn floats_of(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// `lod0/model1` as the level and model it names.
fn place(at: &str) -> Option<(LodSlot, usize)> {
    let (lod, model) = at.split_once('/')?;
    let lod = match lod {
        "container" => LodSlot::Container,
        other => LodSlot::Level(other.strip_prefix("lod")?.parse().ok()?),
    };
    Some((lod, model.strip_prefix("model")?.parse().ok()?))
}

/// The models of a drawable with the place each is at, in the order of the walk an export makes.
fn models_of<T: Edited>(drawable: &RmcDrawable<T>) -> Vec<(LodSlot, usize, &GrmModel<T>)> {
    let mut out = Vec::new();
    for (level, lod) in drawable.lod_group.lods.iter().enumerate() {
        for (model, held) in lod.models.iter().enumerate() {
            out.push((LodSlot::Level(level as u8), model, &**held));
        }
    }
    if let or_io::decl::Shared::Own(Some(container)) = &drawable.container {
        for (model, held) in container.models.iter().enumerate() {
            out.push((LodSlot::Container, model, &**held));
        }
    }
    out
}

fn model_at<T: Edited>(
    drawable: &mut RmcDrawable<T>,
    lod: LodSlot,
    model: usize,
) -> Option<&mut GrmModel<T>> {
    let holder = match (lod, &mut drawable.container) {
        (LodSlot::Level(level), _) => &mut **drawable.lod_group.lods.get_mut(usize::from(level))?,
        (LodSlot::Container, or_io::decl::Shared::Own(Some(own))) => &mut **own,
        (LodSlot::Container, _) => return None,
    };
    holder.models.get_mut(model).map(|m| &mut **m)
}

fn sphere_of(drawable: &RmcDrawable<impl Edited>) -> Option<Sphere> {
    let s = &drawable.lod_group.bounds_sphere.center_and_radius;
    finite(&[s.x, s.y, s.z, s.w]).then_some(Sphere {
        center: [Real(s.x), Real(s.y), Real(s.z)],
        radius: Real(s.w),
    })
}

fn extent_of(b: &SpdAABB) -> Option<Extent> {
    let all = [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z];
    finite(&all).then_some(Extent {
        min: [Real(b.min.x), Real(b.min.y), Real(b.min.z)],
        max: [Real(b.max.x), Real(b.max.y), Real(b.max.z)],
    })
}

/// What `drawable` states, in the words of `drawable.json`; `name` is the drawable's name in the glb, which its meshes are named for.
pub(crate) fn describe_into<T: Edited>(rules: &mut Rules, name: &str, drawable: &RmcDrawable<T>) {
    let lod_group = &drawable.lod_group;
    rules.materials = (drawable.shader_group.as_deref())
        .map(|group| {
            (group.shaders.iter())
                .map(|shader| T::material(shader, group))
                .collect()
        })
        .unwrap_or_default();
    rules.models = models_of(drawable)
        .into_iter()
        .map(|(lod, model, held)| ModelRule {
            at: format!("{}/model{model}", lod.label()),
            name: format!("{name}_{}_model{model}", lod.label()),
            flags: held.flags,
            mask: held.mask,
        })
        .collect();
    rules.lod_distances =
        finite(&lod_group.lod_distances).then(|| lod_group.lod_distances.map(Real));
    rules.bucket_masks = Some(lod_group.bucket_masks);
    rules.bounds_sphere = sphere_of(drawable);
    rules.bounds_box = extent_of(&lod_group.bounds_box);
    rules.bones = (drawable.skeleton.as_deref())
        .map(|skeleton| {
            (skeleton.bones.iter())
                .map(|bone| BoneRule {
                    name: bone.name.clone(),
                    tag: bone.bone_id,
                })
                .collect()
        })
        .unwrap_or_default();
}

/// The names `rules` gives the parts of the glb, for an import to pair by.
pub(crate) fn names_of(rules: &Rules) -> ImportNames {
    ImportNames {
        models: rules
            .models
            .iter()
            .filter_map(|m| place(&m.at).map(|(lod, model)| (lod, model, m.name.clone())))
            .collect(),
        materials: rules.materials.iter().map(|m| m.name.clone()).collect(),
    }
}

/// Checks that `rules` can be applied to `drawable` at all, and says what is wrong with it if not.
fn check<T: Edited>(drawable: &RmcDrawable<T>, rules: &Rules) -> Result<(), Vec<String>> {
    let mut found = Vec::new();
    let shaders = drawable
        .shader_group
        .as_deref()
        .map_or(&[][..], |g| g.shaders.as_slice());
    if !rules.materials.is_empty() && rules.materials.len() != shaders.len() {
        found.push(format!(
            "materials lists {} and the base has {} shaders; adding or removing a shader is authoring",
            rules.materials.len(),
            shaders.len()
        ));
    } else if let Some(group) = drawable.shader_group.as_deref() {
        for (at, (material, shader)) in rules.materials.iter().zip(&group.shaders).enumerate() {
            let held = effect_name(T::effect_hash(shader));
            if material.shader != held {
                found.push(format!(
                    "materials[{at}] is the shader {:?} and the base draws with {held:?}; changing \
                     a shader means choosing its vertex declaration and its parameters, which is \
                     authoring",
                    material.shader
                ));
            }
        }
    }
    let models = models_of(drawable);
    let mut seen = BTreeSet::new();
    for rule in &rules.models {
        match place(&rule.at) {
            Some((lod, model)) if models.iter().any(|(l, m, _)| (*l, *m) == (lod, model)) => {
                if !seen.insert((lod, model)) {
                    found.push(format!("models lists {} twice", rule.at));
                }
            }
            _ => found.push(format!(
                "models names {:?}, which the base does not hold",
                rule.at
            )),
        }
    }
    if !rules.bones.is_empty() {
        let held = drawable.skeleton.as_deref().map_or(0, |s| s.bones.len());
        if held != rules.bones.len() {
            found.push(format!(
                "bones lists {} and the drawable's own skeleton has {held}; a bone cannot be added \
                 or removed here",
                rules.bones.len()
            ));
        } else if let Some(skeleton) = drawable.skeleton.as_deref() {
            for (at, (rule, bone)) in rules.bones.iter().zip(&skeleton.bones).enumerate() {
                if rule.name != bone.name {
                    found.push(format!(
                        "bones[{at}] is {:?} and the base's is {:?}; a bone's name is not changed here",
                        rule.name, bone.name
                    ));
                }
            }
            if let Some(shared) = shared_tag(&rules.bones) {
                found.push(format!(
                    "two bones would share the tag {shared}, and the table that finds a bone by its tag could reach only one"
                ));
            }
        }
    }
    if found.is_empty() {
        Ok(())
    } else {
        Err(found)
    }
}

/// A tag two bones would hold once `bones` is applied.
fn shared_tag(bones: &[BoneRule]) -> Option<u16> {
    let mut seen = BTreeSet::new();
    bones.iter().map(|b| b.tag).find(|tag| !seen.insert(*tag))
}

/// Sets every value `rules` states that the base states otherwise, and names each change.
pub(crate) fn apply<T: Edited>(
    drawable: &mut RmcDrawable<T>,
    rules: &Rules,
    file: &str,
) -> Result<Vec<String>, Vec<Problem>> {
    check(drawable, rules).map_err(|found| {
        found
            .into_iter()
            .map(|why| Problem::new(why).in_file(file))
            .collect::<Vec<_>>()
    })?;
    let refuse = |why: String| vec![Problem::new(why).in_file(file)];
    let mut lines = Vec::new();
    let mut said = |what: &str, changed: &[String]| {
        if !changed.is_empty() {
            lines.push(format!("{file}: {what}: {}", changed.join(", ")));
        }
    };

    if let Some(group) = drawable.shader_group.as_deref_mut() {
        for (at, material) in rules.materials.iter().enumerate() {
            let changed = T::set_material(group, at, material)
                .map_err(|why| refuse(format!("materials[{at}] ({}): {why}", material.shader)))?;
            said(
                &format!("material {at} ({}) set", material.shader),
                &changed,
            );
        }
    }

    for rule in &rules.models {
        let Some((lod, model)) = place(&rule.at) else {
            continue;
        };
        let Some(held) = model_at(drawable, lod, model) else {
            continue;
        };
        let mut changed = Vec::new();
        if held.flags != rule.flags {
            held.flags = rule.flags;
            changed.push(format!("flags {}", rule.flags));
        }
        if held.mask != rule.mask {
            held.mask = rule.mask;
            changed.push(format!("mask {}", rule.mask));
        }
        said(&format!("{} set", rule.at), &changed);
    }

    let lod_group = &mut drawable.lod_group;
    if let Some(distances) = rules.lod_distances {
        let new = distances.map(|d| d.0);
        if !same_bits(&new, &lod_group.lod_distances) {
            lod_group.lod_distances = new;
            said(
                "lod_distances set",
                &[format!("{:?}", lod_group.lod_distances)],
            );
        }
    }
    if let Some(masks) = rules.bucket_masks {
        if masks != lod_group.bucket_masks {
            lod_group.bucket_masks = masks;
            said("bucket_masks set", &[format!("{masks:?}")]);
        }
    }
    if let Some(sphere) = &rules.bounds_sphere {
        let held = &mut lod_group.bounds_sphere.center_and_radius;
        let new = [
            sphere.center[0].0,
            sphere.center[1].0,
            sphere.center[2].0,
            sphere.radius.0,
        ];
        if !same_bits(&new, &[held.x, held.y, held.z, held.w]) {
            [held.x, held.y, held.z, held.w] = new;
            said("bounds_sphere set", &[format!("{new:?}")]);
        }
    }
    if let Some(extent) = &rules.bounds_box {
        let held = &mut lod_group.bounds_box;
        let new = [
            extent.min[0].0,
            extent.min[1].0,
            extent.min[2].0,
            extent.max[0].0,
            extent.max[1].0,
            extent.max[2].0,
        ];
        let now = [
            held.min.x, held.min.y, held.min.z, held.max.x, held.max.y, held.max.z,
        ];
        if !same_bits(&new, &now) {
            [
                held.min.x, held.min.y, held.min.z, held.max.x, held.max.y, held.max.z,
            ] = new;
            said("bounds_box set", &[format!("{new:?}")]);
        }
    }

    if let Some(skeleton) = drawable.skeleton.as_deref_mut() {
        let mut changed = Vec::new();
        for (rule, bone) in rules.bones.iter().zip(&mut skeleton.bones) {
            if bone.bone_id != rule.tag {
                changed.push(format!("{} {} -> {}", bone.name, bone.bone_id, rule.tag));
                bone.bone_id = rule.tag;
            }
        }
        said("bone tags set", &changed);
    }
    Ok(lines)
}
