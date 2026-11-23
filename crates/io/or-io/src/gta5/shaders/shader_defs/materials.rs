//! One material as a file states it: a shader hash and the hashes of its entries.

use std::path::Path;

use crate::decl::{File, Gen8};
use crate::error::Error;
use crate::gta5::gen8::{EntryData, GrcInstanceData};
use crate::gta5::{
    DrawableTarget, FragType, GtaDrawable, GtaFragType, PgDictionary, PtxFxList, PtxParticleRule,
    PtxShaderVarAny, RmcDrawable,
};

/// One entry of one material: the hash a file carries instead of a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialEntry {
    pub name_hash: u32,
    /// A texture slot rather than a constant-buffer member.
    pub binding: bool,
    /// The shader constant register. ⚠ **Not the entry's position**: the
    /// name-hash table is keyed by position and the shader reads by register,
    /// and the two differ in real files.
    pub register: u8,
}

/// One material, as the file states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialRef {
    /// `atStringHash` of the shader the material names — a `.sps` preset for a
    /// drawable, a shader template for a particle rule.
    pub shader_hash: u32,
    /// ⚠ **Only a `.ypt` carries this.** A drawable stores the hash alone, so
    /// a caller that prints a name for one is printing the database's answer,
    /// not the file's.
    pub shader_name: Option<String>,
    /// Where in the file it sits, for a report that has to point at one.
    pub owner: String,
    /// `grcInstanceData::texture_count` as the file states it, where the
    /// format has such a field. ⚠ `None` for a `.ypt`'s particle rule, whose
    /// shader vars carry no separate texture count, and for every Enhanced
    /// material, which reaches each region by index rather than as a prefix,
    /// so "the file disagrees with itself" is not a question either answers.
    pub declared_texture_count: Option<u8>,
    pub entries: Vec<MaterialEntry>,
}

impl MaterialRef {
    /// How many leading entries are texture slots.
    ///
    /// ⚠ **The engine reads the texture block as a prefix**, so this is the
    /// number `declared_texture_count` has to agree with, not the total count
    /// of bindings anywhere in the list.
    pub fn leading_bindings(&self) -> usize {
        self.entries.iter().take_while(|e| e.binding).count()
    }

    /// Whether the entry list is a run of texture slots followed by a run of
    /// constants, which is the shape `declared_texture_count` describes.
    pub fn bindings_are_a_prefix(&self) -> bool {
        self.leading_bindings() == self.entries.iter().filter(|e| e.binding).count()
    }
}

/// The extensions of the formats that name a shader.
pub const MATERIAL_BEARING: [&str; 4] = ["ydr", "ydd", "yft", "ypt"];

pub fn is_material_bearing(extension: &str) -> bool {
    MATERIAL_BEARING
        .iter()
        .any(|e| extension.eq_ignore_ascii_case(e))
}

/// Every material in one file, opened owned as the format `path`'s extension names at the target its RSC7 version names.
///
/// ⚠ A `.ypt` holds materials of both kinds: a particle rule's shader
/// variables, and a `grcInstanceData` on every drawable in its model
/// dictionary. The second is what the save-time layout refusal reaches.
pub fn materials_in(path: &Path, bytes: &[u8]) -> Result<Vec<MaterialRef>, Error> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    #[cfg(feature = "gen9")]
    {
        use crate::container::resource::{ResourceEnvelope, Rsc7};
        let target = crate::gta5::formats::format(&extension)
            .zip(Rsc7::peek(bytes).ok())
            .map(|(format, seal)| format.target_of_version(seal.version()));
        if matches!(target, Some(Ok(crate::Generation::Gen9))) {
            return read::<crate::decl::Gen9>(&extension, bytes);
        }
    }
    read::<Gen8>(&extension, bytes)
}

fn read<T>(extension: &str, bytes: &[u8]) -> Result<Vec<MaterialRef>, Error>
where
    T: DrawableTarget,
    T::InstanceData: Shader,
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    GtaFragType<T>: File,
    PtxFxList<T>: File,
{
    Ok(match extension {
        "ydr" => of_drawable(&GtaDrawable::<T>::from_bytes(bytes)?.base, ""),
        "ydd" => {
            let dict = PgDictionary::<GtaDrawable<T>>::from_bytes(bytes)?;
            keyed(
                &dict.codes,
                dict.entries.iter().map(|d| d.as_deref().map(|d| &d.base)),
            )
        }
        "yft" => of_fragment(&GtaFragType::<T>::from_bytes(bytes)?.base),
        "ypt" => {
            let list = PtxFxList::<T>::from_bytes(bytes)?;
            let mut out: Vec<MaterialRef> = (list.particle_rule_dict.iter())
                .flat_map(|d| d.entries.iter().flatten())
                .map(|rule| of_rule(rule))
                .collect();
            if let Some(dict) = list.model_dict.as_deref() {
                out.extend(keyed(
                    &dict.codes,
                    dict.entries.iter().map(|d| d.as_deref()),
                ));
            }
            out
        }
        other => {
            return Err(Error::Invalid {
                what: format!("`.{other}`"),
                why: "it does not name a shader, so it has no materials to check".into(),
            })
        }
    })
}

fn keyed<'a, T: DrawableTarget>(
    codes: &[u32],
    drawables: impl Iterator<Item = Option<&'a RmcDrawable<T>>>,
) -> Vec<MaterialRef>
where
    T::InstanceData: Shader,
{
    drawables
        .enumerate()
        .filter_map(|(i, d)| Some((i, d?)))
        .flat_map(|(i, d)| {
            let owner = codes
                .get(i)
                .map_or_else(|| format!("[{i}]"), |c| format!("0x{c:08X}"));
            of_drawable(d, &owner)
        })
        .collect()
}

fn of_drawable<T: DrawableTarget>(drawable: &RmcDrawable<T>, owner: &str) -> Vec<MaterialRef>
where
    T::InstanceData: Shader,
{
    let Some(group) = drawable.shader_group.as_deref() else {
        return Vec::new();
    };
    (group.shaders.iter())
        .map(|shader| shader.material(owner))
        .collect()
}

/// A shader as one target lays it out, read as the material it states.
trait Shader {
    fn material(&self, owner: &str) -> MaterialRef;
}

impl Shader for GrcInstanceData {
    fn material(&self, owner: &str) -> MaterialRef {
        MaterialRef {
            shader_hash: self.basis_hash.get(),
            shader_name: None,
            owner: owner.to_string(),
            declared_texture_count: Some(
                self.entries
                    .iter()
                    .take_while(|e| matches!(e.data, EntryData::Texture(_)))
                    .count() as u8,
            ),
            entries: self
                .entries
                .iter()
                .map(|e| MaterialEntry {
                    name_hash: e.name_hash.get(),
                    binding: matches!(e.data, EntryData::Texture(_)),
                    register: e.register,
                })
                .collect(),
        }
    }
}

/// ⚠ An Enhanced material is keyed by its `name`, not its `preset`, which is
/// one constant tag in every material shipped; a constant's register is the
/// 16-byte row its byte offset falls in.
#[cfg(feature = "gen9")]
impl Shader for crate::gta5::gen9::GrcInstanceDataG9 {
    fn material(&self, owner: &str) -> MaterialRef {
        MaterialRef {
            shader_hash: self.name.get(),
            shader_name: None,
            owner: owner.to_string(),
            declared_texture_count: None,
            entries: (self.param_infos.params.iter())
                .map(|p| {
                    let constant = p.constant().map(|(_, range)| range.start / 16);
                    MaterialEntry {
                        name_hash: p.name.get(),
                        binding: constant.is_none(),
                        register: (p.texture().or(p.sampler()).or(p.other()).or(constant))
                            .and_then(|r| u8::try_from(r).ok())
                            .unwrap_or(u8::MAX),
                    }
                })
                .collect(),
        }
    }
}

fn of_fragment<T: DrawableTarget>(frag: &FragType<T>) -> Vec<MaterialRef>
where
    T::InstanceData: Shader,
{
    let mut out = Vec::new();
    if let Some(d) = frag.common_drawable.as_deref() {
        out.extend(of_drawable(&d.base, "common"));
    }
    for (i, extra) in frag.extra_drawables.iter().enumerate() {
        let name =
            (frag.extra_drawable_names.get(i).cloned()).unwrap_or_else(|| format!("extra[{i}]"));
        out.extend(of_drawable(&extra.base, &name));
    }
    if let Some(d) = frag.common_cloth_drawable.as_deref() {
        out.extend(of_drawable(&d.base, "cloth"));
    }
    if let Some(group) = frag.physics_lod_group.as_deref() {
        let lods = [&group.high_lod, &group.medium_lod, &group.low_lod];
        for (l, lod) in lods.into_iter().enumerate() {
            let Some(lod) = lod.as_deref() else { continue };
            for (c, child) in lod.children.iter().enumerate() {
                let owner = format!("lod{l}/child{c}");
                if let Some(d) = child.undamaged_entity.as_deref() {
                    out.extend(of_drawable(&d.base, &owner));
                }
                if let Some(d) = child.damaged_entity.as_deref() {
                    out.extend(of_drawable(&d.base, &format!("{owner}/damaged")));
                }
            }
        }
    }
    out
}

fn of_rule<T: DrawableTarget>(rule: &PtxParticleRule<T>) -> MaterialRef {
    let inst = &rule.shader_inst;
    MaterialRef {
        shader_hash: inst.shader_template_hash_name.get(),
        shader_name: Some(inst.shader_template_name.clone()),
        owner: rule.name.clone(),
        declared_texture_count: None,
        entries: (inst.inst_vars.iter().flatten())
            .map(|var| {
                let (base, binding) = match &**var {
                    PtxShaderVarAny::PtxShaderVarTexture(t) => (&t.base, true),
                    PtxShaderVarAny::PtxShaderVarVector(v) => (&v.base, false),
                    PtxShaderVarAny::PtxShaderVarKeyframe(k) => (&k.base, false),
                    PtxShaderVarAny::Base(b) => (b, false),
                };
                MaterialEntry {
                    name_hash: base.hash_name.get(),
                    binding,
                    register: base.id as u8,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_four_formats_that_name_a_shader_are_offered() {
        for ext in ["ydr", "YDD", "yft", "ypt"] {
            assert!(is_material_bearing(ext), "{ext}");
        }
        for ext in ["ytd", "ybn", "ycd", "yld", "rpf", ""] {
            assert!(!is_material_bearing(ext), "{ext}");
        }
    }

    #[test]
    fn a_format_with_no_materials_is_refused_rather_than_read_as_empty() {
        let err = materials_in(Path::new("x.ybn"), &[]).expect_err("should refuse");
        assert!(err.to_string().contains("does not name a shader"), "{err}");
    }
}
