//! What a person says about how a garment behaves: its flags, its mask, its audio, and what a hat does to the hair.
//!
//! Each is a change to the engine's own fields, named by the engine ([`crate::flags`], [`crate::mask`],
//! [`crate::hair`], [`crate::audio`]) and refused where the garment has no such field.

use or_io::meta::{CCreatureMetaData, CPedVariationInfo};

use crate::audio::{self, Which};
use crate::edit::Error;
use crate::flags::{self, Flag};
use crate::hair;
use crate::mask::{self, Property, RenderFlag};
use crate::metadata::hashed;
use crate::model::Garment;
use crate::names::Slot;

/// One change of behaviour.
#[derive(Debug, Clone, PartialEq)]
pub enum Behaviour {
    /// State a flag or take it off.
    Flag(Flag, bool),
    /// State exactly these flags, bits no screen names among them.
    Flags(u32),
    /// State a bit of a component's `propMask` or take it off.
    Property(Property, bool),
    /// State exactly this `propMask`.
    Properties(u8),
    /// State a render flag of a prop or take it off.
    Render(RenderFlag, bool),
    /// How firmly a prop stays on, none to state nothing.
    Stickyness(Option<u8>),
    /// What a hat does to the hair under it.
    HairScale(Option<f32>),
    /// How tall a hair stands.
    HairHeight(Option<f32>),
    /// An audio id, none for `NONE`.
    Audio(Which, Option<String>),
    /// The alternate model the first person view streams in beside a component drawable ([`crate::firstperson`]).
    FirstPerson(Option<u8>),
}

/// Make `change` to the garment; it is left as it was where the change is refused.
pub fn apply(garment: &mut Garment, change: Behaviour) -> Result<(), Error> {
    let before = garment.clone();
    let done = match change {
        Behaviour::Flag(flag, on) => flags::set(garment, flag, on),
        Behaviour::Flags(word) => flags::set_word(garment, word),
        Behaviour::Property(property, on) => mask::set_property(garment, property, on),
        Behaviour::Properties(bits) => mask::set_properties(garment, bits),
        Behaviour::Render(flag, on) => mask::set_render(garment, flag, on),
        Behaviour::Stickyness(value) => mask::set_stickyness(garment, value),
        Behaviour::HairScale(value) => hair::set_scale(garment, value),
        Behaviour::HairHeight(value) => hair::set_height(garment, value),
        Behaviour::Audio(which, id) => audio::set(garment, which, id.as_deref()),
        Behaviour::FirstPerson(number) => crate::firstperson::set(garment, number),
    };
    if done.is_err() {
        *garment = before;
    }
    done
}

/// What a variation metadata states of one drawable or prop, and what a pack says it should.
#[derive(Debug, Clone, PartialEq)]
pub struct Stated {
    pub flags: u32,
    /// A component's `propMask`.
    pub mask: Option<u8>,
    /// A prop's `renderFlags`.
    pub render: Option<u8>,
    /// A prop's `stickyness`.
    pub stickyness: Option<u8>,
    pub audio: u32,
    pub audio2: Option<u32>,
    pub mods: [f32; 5],
}

impl Stated {
    /// What the pack says of the garment.
    pub fn of(garment: &Garment) -> Self {
        let prop = matches!(garment.slot, Some(Slot::Prop(_)));
        let mods = match prop {
            true => garment.meta.expression_mods,
            false => garment.meta.ped_xml_expression_mods,
        };
        Self {
            flags: flags::word(garment),
            mask: (!prop).then(|| mask::properties(garment)),
            render: prop.then(|| mask::render_flags(garment)),
            stickyness: prop.then(|| garment.meta.stickyness.unwrap_or(0)),
            audio: hashed(&audio::get(garment, Which::First)).hash,
            audio2: (!prop).then(|| hashed(&audio::get(garment, Which::Second)).hash),
            mods: mods.unwrap_or([0.0; 5]),
        }
    }

    /// The sentences that say how `self`, what a file states, is not `wanted`.
    pub fn differences(&self, wanted: &Self) -> Vec<String> {
        let mut said = Vec::new();
        let mut differ = |what: &str, stated: String, wanted: String| {
            if stated != wanted {
                said.push(format!(
                    "states {what} {stated} in the metadata and the pack says {wanted}"
                ));
            }
        };
        differ(
            "flags",
            format!("{:#X}", self.flags),
            format!("{:#X}", wanted.flags),
        );
        differ(
            "a propMask of",
            format!("{:?}", self.mask),
            format!("{:?}", wanted.mask),
        );
        differ(
            "renderFlags",
            format!("{:?}", self.render),
            format!("{:?}", wanted.render),
        );
        differ(
            "a stickyness of",
            format!("{:?}", self.stickyness),
            format!("{:?}", wanted.stickyness),
        );
        differ(
            "the audio id",
            format!("{:08X}", self.audio),
            format!("{:08X}", wanted.audio),
        );
        differ(
            "the second audio id",
            format!("{:?}", self.audio2),
            format!("{:?}", wanted.audio2),
        );
        differ(
            "the expression modifiers",
            format!("{:?}", self.mods),
            format!("{:?}", wanted.mods),
        );
        said
    }
}

/// What `info` states of the drawable or prop at `slot` and `position`, none where it has none.
pub fn stated(info: &CPedVariationInfo, slot: Slot, position: u32) -> Stated {
    match slot {
        Slot::Component(c) => {
            let drawable = info
                .avail_comp
                .get(usize::from(c.id()))
                .filter(|i| **i != u8::MAX)
                .and_then(|i| info.a_component_data3.get(usize::from(*i)))
                .and_then(|d| d.a_drawbl_data3.get(position as usize));
            let component = info.comp_infos.iter().find(|i| {
                i.ped_xml_comp_idx == c.id() && u32::from(i.ped_xml_drawbl_idx) == position
            });
            let mut mods = [0.0; 5];
            for (m, v) in mods
                .iter_mut()
                .zip(component.map_or(&[][..], |i| &i.ped_xml_expression_mods))
            {
                *m = *v;
            }
            Stated {
                flags: component.map_or(0, |i| i.flags),
                mask: Some(drawable.map_or(0, |d| d.prop_mask)),
                render: None,
                stickyness: None,
                audio: component.map_or(hashed("NONE").hash, |i| i.ped_xml_audio_id.hash),
                audio2: Some(component.map_or(hashed("NONE").hash, |i| i.ped_xml_audio_id2.hash)),
                mods,
            }
        }
        Slot::Prop(p) => {
            let prop = info
                .prop_info
                .a_prop_meta_data
                .iter()
                .find(|m| m.anchor_id == p.anchor() && u32::from(m.prop_id) == position);
            let mut mods = [0.0; 5];
            for (m, v) in mods
                .iter_mut()
                .zip(prop.map_or(&[][..], |m| &m.expression_mods))
            {
                *m = *v;
            }
            Stated {
                // ⚠ The game reads `propFlags` when it is not zero and `flags` otherwise.
                flags: prop.map_or(0, |m| match m.prop_flags {
                    0 => u32::from(m.flags),
                    wide => wide,
                }),
                mask: None,
                render: Some(prop.map_or(0, |m| m.render_flags)),
                stickyness: Some(prop.map_or(0, |m| m.stickyness)),
                audio: prop.map_or(hashed("NONE").hash, |m| m.audio_id.hash),
                audio2: None,
                mods,
            }
        }
    }
}

/// The expressions a garment states that drive the hair: the expression index, the bone and the value.
pub fn hair_driven(garment: &Garment) -> Vec<(u32, u16, f32)> {
    let mut driven = Vec::new();
    if let Some(v) = hair::scale_of(garment) {
        driven.push((hair::SCALE_EXPRESSION, hair::SCALE_BONE, v));
    }
    if let Some(v) = hair::height_of(garment) {
        driven.push((hair::HEIGHT_EXPRESSION, hair::HEIGHT_BONE, v));
    }
    driven
}

/// Whether the creature metadata drives `bone` with expression `index` of the drawable or prop at `slot` and `position`.
pub fn drives(
    creature: &CCreatureMetaData,
    slot: Slot,
    position: u32,
    index: u32,
    bone: u16,
) -> bool {
    let position = i32::try_from(position).unwrap_or(i32::MAX);
    match slot {
        Slot::Component(c) => creature.ped_comp_expressions.iter().any(|e| {
            e.ped_comp_id == u32::from(c.id())
                && e.ped_comp_var_index == position
                && e.ped_comp_expression_index == index
                && e.ids.contains(&bone)
        }),
        Slot::Prop(p) => creature.ped_prop_expressions.iter().any(|e| {
            e.ped_prop_id == u32::from(p.anchor())
                && e.ped_prop_var_index == position
                && e.ped_prop_expression_index == index
                && e.ids.contains(&bone)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{Component, Prop, Slot};

    #[test]
    fn a_refused_change_leaves_the_garment_as_it_was() {
        let mut g = Garment::new();
        g.slot = Some(Slot::Component(Component::Jbib));
        g.meta.flags = Some(5);
        let before = g.clone();
        assert!(apply(&mut g, Behaviour::Stickyness(Some(9))).is_err());
        assert!(apply(&mut g, Behaviour::HairScale(Some(-0.5))).is_err());
        assert!(apply(&mut g, Behaviour::Render(RenderFlag::Alpha, true)).is_err());
        assert_eq!(g, before);
    }

    #[test]
    fn a_hat_takes_a_flag_a_scale_and_a_stickyness() {
        let mut g = Garment::new();
        g.slot = Some(Slot::Prop(Prop::Head));
        apply(&mut g, Behaviour::Flag(Flag::DefaultHelmet, true)).unwrap();
        apply(&mut g, Behaviour::HairScale(Some(-1.0))).unwrap();
        apply(&mut g, Behaviour::Stickyness(Some(100))).unwrap();
        apply(&mut g, Behaviour::Render(RenderFlag::Cutout, true)).unwrap();
        assert!(flags::has(&g, Flag::DefaultHelmet));
        assert_eq!(hair::scale_of(&g), Some(-1.0));
        assert_eq!(g.meta.stickyness, Some(100));
        assert_eq!(g.meta.render_flags, Some(0b100));
    }
}
