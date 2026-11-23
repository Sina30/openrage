use serde::{Deserialize, Serialize};

use or_io::decl::{Hash32, Ref};
use or_io::gta5::particle::{self as ptx, RawKeyframeEntry};

use crate::behaviour::record::set_flag;
use crate::keyframe::{raw_entry, set_entries};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Evolution {
    pub name: String,
    pub name_hash: u32,
    pub override_value: f32,
    pub is_overridden: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvolvedKeyframe {
    pub entries: Vec<RawKeyframeEntry>,
    pub evolution_index: i32,
    pub is_lod_evolution: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvolvedKeyframeProp {
    pub evolved_keyframes: Vec<EvolvedKeyframe>,
    pub property_id: u32,
    pub blend_mode: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvolutionList {
    pub evolutions: Vec<Evolution>,
    pub evolved_keyframe_props: Vec<EvolvedKeyframeProp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Colour {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Colour {
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn white() -> Self {
        Self::new(1.0, 1.0, 1.0, 1.0)
    }

    pub fn black() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }

    pub fn from_rgba_bytes(bytes: [u8; 4]) -> Self {
        Self {
            r: bytes[0] as f32 / 255.0,
            g: bytes[1] as f32 / 255.0,
            b: bytes[2] as f32 / 255.0,
            a: bytes[3] as f32 / 255.0,
        }
    }

    pub fn to_rgba_bytes(&self) -> [u8; 4] {
        [
            (self.r * 255.0).clamp(0.0, 255.0) as u8,
            (self.g * 255.0).clamp(0.0, 255.0) as u8,
            (self.b * 255.0).clamp(0.0, 255.0) as u8,
            (self.a * 255.0).clamp(0.0, 255.0) as u8,
        ]
    }
}

impl Default for Colour {
    fn default() -> Self {
        Self::white()
    }
}

impl EvolutionList {
    /// The evolutions a list states, and what each varies.
    pub fn of(l: &ptx::PtxEvolutionList) -> Self {
        Self {
            evolutions: l
                .evolutions
                .iter()
                .map(|e| Evolution {
                    name: e.name.clone(),
                    name_hash: e.name_hash.get(),
                    override_value: e.override_value,
                    is_overridden: e.is_overriden.get(),
                })
                .collect(),
            evolved_keyframe_props: l
                .evolved_keyframe_props
                .iter()
                .map(|p| EvolvedKeyframeProp {
                    evolved_keyframes: p
                        .evolved_keyframes
                        .iter()
                        .map(|k| EvolvedKeyframe {
                            entries: k.keyframe.entries.iter().map(raw_entry).collect(),
                            evolution_index: k.evolution_idx,
                            is_lod_evolution: k.is_lod_evolution.get(),
                        })
                        .collect(),
                    property_id: p.property_id.get(),
                    blend_mode: u32::from(p.blend_mode),
                })
                .collect(),
        }
    }

    /// This list written into `out`, which sits at `path` in its particle list.
    ///
    /// ⚠ The map names each evolved value by its place in the list's own run:
    /// kept while the values it names are, else rebuilt against `path`.
    pub fn write_to(&self, out: &mut ptx::PtxEvolutionList, path: &str) {
        out.evolutions
            .resize_with(self.evolutions.len(), Default::default);
        for (slot, e) in out.evolutions.iter_mut().zip(&self.evolutions) {
            slot.name = e.name.clone();
            slot.name_hash = Hash32::from_raw(e.name_hash);
            slot.override_value = e.override_value;
            set_flag(&mut slot.is_overriden, e.is_overridden);
        }
        let ids = |props: &[ptx::PtxEvolvedKeyframeProp]| -> Vec<u32> {
            props.iter().map(|p| p.property_id.get()).collect()
        };
        let before = ids(&out.evolved_keyframe_props);
        out.evolved_keyframe_props
            .resize_with(self.evolved_keyframe_props.len(), Default::default);
        for (slot, p) in out
            .evolved_keyframe_props
            .iter_mut()
            .zip(&self.evolved_keyframe_props)
        {
            slot.evolved_keyframes
                .resize_with(p.evolved_keyframes.len(), Default::default);
            for (k, e) in slot.evolved_keyframes.iter_mut().zip(&p.evolved_keyframes) {
                set_entries(&mut k.keyframe, &e.entries);
                k.evolution_idx = e.evolution_index;
                set_flag(&mut k.is_lod_evolution, e.is_lod_evolution);
            }
            slot.property_id = Hash32::from_raw(p.property_id);
            slot.blend_mode = p.blend_mode as u8;
        }
        let map = &mut out.evolved_keyframe_prop_map;
        let named: Vec<&str> = map
            .data
            .iter()
            .filter_map(|p| match &p.data {
                Ref::To(to) => Some(&**to),
                Ref::Own(_) => None,
            })
            .collect();
        if ids(&out.evolved_keyframe_props) == before
            && named
                .iter()
                .all(|to| to.starts_with(&format!("{path}.evolved_keyframe_props[")))
        {
            return;
        }
        let mut pairs: Vec<(u32, usize)> = self
            .evolved_keyframe_props
            .iter()
            .enumerate()
            .map(|(j, p)| (p.property_id, j))
            .collect();
        pairs.sort_by_key(|(key, _)| *key);
        map.data.resize_with(pairs.len(), Default::default);
        for (slot, (key, j)) in map.data.iter_mut().zip(pairs) {
            slot.key = key;
            slot.data = Ref::To(format!("{path}.evolved_keyframe_props[{j}]").into());
        }
        set_flag(&mut map.sorted, !map.data.is_empty());
    }
}
