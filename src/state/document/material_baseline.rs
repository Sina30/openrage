//! What a material's parameters were when the document arrived, so
//! "reset to the file's value" can mean something.

use std::collections::HashMap;

use or_io::decl::{Gen8, Gen9};
use or_io::gta5::gen8::{EntryData, GrcInstanceData, GrcTexture, GrcTextureReference};
use or_io::gta5::{GrmShaderGroup, RmcDrawable, TextureSlot};

use super::DocPayload;

/// What a Legacy material binds a texture slot to.
pub type Slot = TextureSlot<GrcTexture, GrcTextureReference>;

/// Which drawable inside a document a material belongs to.
///
/// ⚠ **A `.ydr` is one drawable and a `.ydd` or `.yft` is many**, so a
/// material is not addressed by shader index alone. Keying the baseline on the
/// shader index only would make every dictionary entry's first material share
/// one remembered value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialAddress {
    pub owner: u32,
    pub shader: u32,
}

impl MaterialAddress {
    pub fn new(owner: usize, shader: usize) -> Self {
        Self {
            owner: owner as u32,
            shader: shader as u32,
        }
    }
}

/// The value one entry held.
#[derive(Debug, Clone, PartialEq)]
pub enum BaselineValue {
    Texture(String),
    Registers(Vec<[f32; 4]>),
}

impl BaselineValue {
    fn of(data: &EntryData<Slot>, group: &GrmShaderGroup<Gen8>) -> Self {
        match data {
            EntryData::Texture(slot) => {
                Self::Texture(group.slot_name(slot).unwrap_or_default().into())
            }
            EntryData::Float(rows) => {
                Self::Registers(rows.iter().map(|v| [v.x, v.y, v.z, v.w]).collect())
            }
        }
    }
}

/// Every material entry's value as of the last time this document agreed with
/// its file.
///
/// ⚠ **"The file's value" means as of load or last save.** The payload is the
/// live edited model, and re-reading the file needs a path an in-archive
/// document may not have. [`Document::mark_saved`] re-captures.
#[derive(Debug, Clone, Default)]
pub struct MaterialBaseline {
    entries: HashMap<(MaterialAddress, u32), BaselineValue>,
    /// Which entries existed at all, so a *removed* one can be told from one
    /// that was never there.
    present: HashMap<MaterialAddress, Vec<u32>>,
}

impl MaterialBaseline {
    pub fn of(payload: &DocPayload) -> Self {
        let mut out = Self::default();
        for (owner, drawable) in drawables_of(payload) {
            let Some(group) = drawable.shader_group.as_deref() else {
                continue;
            };
            for (shader_index, shader) in group.shaders.iter().enumerate() {
                let at = MaterialAddress::new(owner, shader_index);
                out.present.insert(
                    at,
                    shader.entries.iter().map(|e| e.name_hash.get()).collect(),
                );
                for entry in &shader.entries {
                    out.entries.insert(
                        (at, entry.name_hash.get()),
                        BaselineValue::of(&entry.data, group),
                    );
                }
            }
        }
        out
    }

    pub fn value(&self, at: MaterialAddress, name_hash: u32) -> Option<&BaselineValue> {
        self.entries.get(&(at, name_hash))
    }

    /// Entries the file had that this material no longer carries.
    pub fn removed(&self, at: MaterialAddress, shader: &GrcInstanceData) -> Vec<u32> {
        self.present
            .get(&at)
            .map(|was| {
                was.iter()
                    .copied()
                    .filter(|h| shader.entry(*h).is_none())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Every drawable a document holds that can carry materials, with the owner
/// index the baseline keys on.
///
/// ⚠ **Gen9 payloads are absent.** A `BaselineValue` is a Legacy material
/// entry, and an Enhanced material's values live in a constant buffer its own
/// descriptor places, so the Enhanced inspector (through
/// [`enhanced_drawable_mut`]) offers no reset-to-file.
pub fn drawables_of(payload: &DocPayload) -> Vec<(usize, &RmcDrawable<Gen8>)> {
    match payload {
        DocPayload::Resource(or_io::Asset::Drawable(d)) => vec![(0, &d.base)],
        DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => (dict.entries.iter())
            .enumerate()
            .filter_map(|(i, d)| Some((i, &d.as_deref()?.base)))
            .collect(),
        DocPayload::Resource(or_io::Asset::Fragment(frag)) => {
            (frag.drawables().into_iter().map(|d| &d.base).enumerate()).collect()
        }
        _ => Vec::new(),
    }
}

/// The mutable half, addressed by owner index.
pub fn drawable_mut(payload: &mut DocPayload, owner: u32) -> Option<&mut RmcDrawable<Gen8>> {
    let owner = owner as usize;
    match payload {
        DocPayload::Resource(or_io::Asset::Drawable(d)) if owner == 0 => Some(&mut d.base),
        DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => dict
            .entries
            .get_mut(owner)?
            .as_deref_mut()
            .map(|d| &mut d.base),
        DocPayload::Resource(or_io::Asset::Fragment(frag)) => {
            frag.drawables_mut().into_iter().nth(owner)
        }
        _ => None,
    }
}

/// Every Enhanced drawable a document holds, read-only.
pub fn enhanced_drawables_of(payload: &DocPayload) -> Vec<(usize, &RmcDrawable<Gen9>)> {
    match payload {
        DocPayload::Resource(or_io::Asset::DrawableGen9(d)) => vec![(0, &d.base)],
        DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => (dict.entries.iter())
            .enumerate()
            .filter_map(|(i, d)| Some((i, &d.as_deref()?.base)))
            .collect(),
        DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => {
            (frag.drawables().into_iter().map(|d| &d.base).enumerate()).collect()
        }
        _ => Vec::new(),
    }
}

/// The mutable half, addressed by owner index.
///
/// Not generic over the target: `RmcDrawable<Gen8>` and `RmcDrawable<Gen9>`
/// hold different materials.
pub fn enhanced_drawable_mut(
    payload: &mut DocPayload,
    owner: u32,
) -> Option<&mut RmcDrawable<Gen9>> {
    let owner = owner as usize;
    match payload {
        DocPayload::Resource(or_io::Asset::DrawableGen9(d)) if owner == 0 => Some(&mut d.base),
        DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => dict
            .entries
            .get_mut(owner)?
            .as_deref_mut()
            .map(|d| &mut d.base),
        DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => {
            frag.drawables_mut().into_iter().nth(owner)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::decl::{Hash32, Vec4};
    use or_io::gta5::gen8::GrcInstanceDataEntry;
    use or_io::gta5::shaders::shader_defs::hash_name;
    use or_io::gta5::{GtaDrawable, PgDictionary};

    fn drawable_with_material(texture: &str) -> GtaDrawable {
        let mut shader = GrcInstanceData::default();
        shader.entries.push(GrcInstanceDataEntry {
            register: 0,
            name_hash: Hash32::from_raw(hash_name("DiffuseSampler")),
            data: EntryData::Texture(TextureSlot::Reference(Box::new(
                GrcTextureReference::named(texture),
            ))),
        });
        shader.entries.push(GrcInstanceDataEntry {
            register: 4,
            name_hash: Hash32::from_raw(hash_name("bumpiness")),
            data: EntryData::Float(vec![Vec4 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            }]),
        });
        let mut group = GrmShaderGroup::<Gen8>::default();
        group.shaders.push(Box::new(shader));

        let mut drawable = GtaDrawable::default();
        drawable.base.shader_group = Some(Box::new(group));
        drawable
    }

    fn drawable_payload(texture: &str) -> DocPayload {
        DocPayload::Resource(or_io::Asset::Drawable(drawable_with_material(texture)))
    }

    #[test]
    fn the_baseline_remembers_the_value_each_entry_arrived_with() {
        let baseline = MaterialBaseline::of(&drawable_payload("stone"));
        let at = MaterialAddress::new(0, 0);
        assert_eq!(
            baseline.value(at, hash_name("DiffuseSampler")),
            Some(&BaselineValue::Texture("stone".into()))
        );
        assert_eq!(
            baseline.value(at, hash_name("bumpiness")),
            Some(&BaselineValue::Registers(vec![[1.0, 0.0, 0.0, 0.0]]))
        );
        assert!(baseline.value(at, hash_name("nothing")).is_none());
    }

    /// ⚠ A removed entry and an entry that was never there are different
    /// things: only the first can be put back.
    #[test]
    fn a_removed_entry_is_distinguishable_from_one_that_was_never_there() {
        let mut payload = drawable_payload("stone");
        let baseline = MaterialBaseline::of(&payload);
        let at = MaterialAddress::new(0, 0);

        let drawable = drawable_mut(&mut payload, 0).expect("the drawable");
        let shader = &mut drawable.shader_group.as_mut().unwrap().shaders[0];
        shader
            .entries
            .retain(|e| e.name_hash.get() != hash_name("bumpiness"));

        assert_eq!(baseline.removed(at, shader), vec![hash_name("bumpiness")]);
    }

    /// ⚠ Two dictionary entries' first materials must not share a baseline.
    #[test]
    fn a_dictionary_entry_is_part_of_the_key() {
        let mut dict = PgDictionary::<GtaDrawable>::default();
        for (name, texture) in [("a", "stone"), ("b", "metal")] {
            dict.codes.push(or_io::hash::string_hash_str(name));
            dict.entries
                .push(Some(Box::new(drawable_with_material(texture))));
        }

        let baseline =
            MaterialBaseline::of(&DocPayload::Resource(or_io::Asset::DrawableDict(dict)));
        assert_eq!(
            baseline.value(MaterialAddress::new(0, 0), hash_name("DiffuseSampler")),
            Some(&BaselineValue::Texture("stone".into()))
        );
        assert_eq!(
            baseline.value(MaterialAddress::new(1, 0), hash_name("DiffuseSampler")),
            Some(&BaselineValue::Texture("metal".into()))
        );
    }

    /// ⚠ Gen9 instance data is a different field set, so a Gen9 payload must
    /// never come back from the Legacy accessor — it would be edited through
    /// a type that cannot describe it.
    #[test]
    fn a_gen9_payload_offers_no_legacy_drawable_to_edit() {
        let mut payload = DocPayload::Resource(or_io::Asset::DrawableGen9(Default::default()));
        assert!(drawables_of(&payload).is_empty());
        assert!(drawable_mut(&mut payload, 0).is_none());
        assert!(MaterialBaseline::of(&payload)
            .value(MaterialAddress::new(0, 0), hash_name("DiffuseSampler"))
            .is_none());
    }

    /// ...and the Enhanced accessor is the one that reaches it, with the
    /// Legacy payloads absent from it for the same reason.
    #[test]
    fn an_enhanced_payload_is_reached_only_through_the_enhanced_accessor() {
        let mut enhanced = DocPayload::Resource(or_io::Asset::DrawableGen9(Default::default()));
        assert_eq!(enhanced_drawables_of(&enhanced).len(), 1);
        assert!(enhanced_drawable_mut(&mut enhanced, 0).is_some());
        assert!(enhanced_drawable_mut(&mut enhanced, 1).is_none());

        let mut legacy = DocPayload::Resource(or_io::Asset::Drawable(Default::default()));
        assert!(enhanced_drawables_of(&legacy).is_empty());
        assert!(enhanced_drawable_mut(&mut legacy, 0).is_none());
    }
}
