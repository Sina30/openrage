//! What a GTA VI particle list reads and its GTA V form has no member for, as raw rows: a
//! velocity's angle, a colour's values past the three GTA V names, a domain's fifth and sixth, and
//! each evolved keyframe's kind and flags.

use or_io::gta5::particle::KFP_COLOUR;
use or_io::gta6::headerless::{
    EvolutionList, KfpEntry, ParticleEffects, BEHAVIOUR_KFPS, COLOUR_RGBA_KFPS,
};

/// The rows of `list`, appended to `lines` in the list's order.
pub(super) fn lines(list: &ParticleEffects, lines: &mut Vec<String>) {
    for e in &list.emitters {
        for (slot, d) in e.domains.iter().enumerate() {
            let Some(d) = d else { continue };
            // Position, rotation and the two sizes are GTA V's; the two after them are not.
            for k in &d.kfps[4..] {
                lines.push(tr!(
                    "Emitter rule {name}, domain {slot}: keyframed value {value}, whose name is not known, holds {keys}.",
                    name = e.name,
                    slot = slot,
                    value = name_of(k.id),
                    keys = keys(&k.entries),
                ));
            }
        }
    }
    for p in &list.particles {
        for (place, b) in p.behaviours.iter().enumerate() {
            let Some(b) = list.behaviours.get(*b) else {
                continue;
            };
            let Some(class) = b.class else { continue };
            for k in b.kfps.iter().filter(|k| unshown(class, k.id)) {
                lines.push(tr!(
                    "Particle rule {rule}, behaviour {place}, {class}: keyframed value {value}, which GTA V's record has no member for, holds {keys}.",
                    rule = p.name,
                    place = place,
                    class = class,
                    value = name_of(k.id),
                    keys = keys(&k.entries),
                ));
            }
        }
    }
    for e in &list.effects {
        let owner = tr!("Effect rule {effect}", effect = e.name);
        evolved(&owner, e.evolution_list.as_ref(), lines);
        for (i, ev) in e.events.iter().enumerate() {
            let owner = tr!(
                "Effect rule {effect}, event {event}",
                effect = e.name,
                event = i
            );
            evolved(&owner, ev.evolution_list.as_ref(), lines);
        }
    }
}

/// Whether a keyframed value of id `id` in a behaviour of `class` has no member in GTA V's record.
fn unshown(class: &str, id: u32) -> bool {
    match class {
        "ptxu_Velocity" => true,
        "ptxu_Colour" => !COLOUR_RGBA_KFPS.contains(&id) && id != KFP_COLOUR[2],
        _ => false,
    }
}

/// The `class:member` a keyframed value's id is found to name, else `hash_` and the id.
fn name_of(id: u32) -> String {
    (BEHAVIOUR_KFPS.iter())
        .find(|(hash, _)| *hash == id)
        .map_or_else(|| format!("hash_{id:08X}"), |(_, name)| (*name).to_string())
}

/// Each key as its time and its four values.
fn keys(entries: &[KfpEntry]) -> String {
    if entries.is_empty() {
        return tr!("none").to_string();
    }
    let keys: Vec<String> = (entries.iter())
        .map(|e| {
            let [x, y, z, w] = e.value;
            format!("{:.3}: ({x}, {y}, {z}, {w})", e.time[0])
        })
        .collect();
    keys.join("; ")
}

/// A row per evolved keyframe of `list`, which `owner` holds.
fn evolved(owner: &str, list: Option<&EvolutionList>, lines: &mut Vec<String>) {
    let Some(list) = list else { return };
    for p in &list.props {
        for k in &p.keyframes {
            let evolution = (list.evolutions.get(k.evolution as usize))
                .map_or_else(|| k.evolution.to_string(), |e| e.name.clone());
            lines.push(tr!(
                "{owner}: evolved value {value} under evolution {evolution} has kind {kind}, where 1 is the LOD distance and 2 to 4 the camera's forward, top and side views, and flags {first} and {second}, whose meaning is not known.",
                owner = owner,
                value = name_of(p.id),
                evolution = evolution,
                kind = k.kind,
                first = k.flags[0],
                second = k.flags[1],
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use or_io::decl::Hash32;
    use or_io::gta6::headerless::{Evolution, EvolvedKeyframe, EvolvedProp};

    use super::*;

    #[test]
    fn a_velocity_s_values_and_a_colour_s_past_the_three_gta_v_names_are_the_unshown_ones() {
        assert!(!unshown("ptxu_Colour", COLOUR_RGBA_KFPS[0]));
        assert!(!unshown("ptxu_Colour", COLOUR_RGBA_KFPS[1]));
        assert!(!unshown("ptxu_Colour", KFP_COLOUR[2]));
        assert!(unshown("ptxu_Colour", 0x1234_5678));
        let angle = Hash32::of("ptxu_Velocity:m_angleKFP").get();
        assert!(unshown("ptxu_Velocity", angle));
        assert_eq!(name_of(angle), "ptxu_Velocity:m_angleKFP");
        assert!(!unshown("ptxu_Size", 0x1234_5678));
        assert_eq!(name_of(0x1234_5678), "hash_12345678");
    }

    #[test]
    fn a_value_s_keys_read_as_their_time_and_four_values() {
        let key = KfpEntry {
            time: [0.5, 2.0, 0.0, 0.0],
            value: [1.0, 2.5, 3.0, -4.0],
        };
        assert_eq!(
            keys(&[key, key]),
            "0.500: (1, 2.5, 3, -4); 0.500: (1, 2.5, 3, -4)"
        );
    }

    #[test]
    fn each_evolved_keyframe_is_a_row_with_its_evolution_kind_and_flags() {
        let keyframe = |evolution, kind, flags| EvolvedKeyframe {
            entries: Vec::new(),
            evolution,
            kind,
            flags,
        };
        let list = EvolutionList {
            evolutions: vec![Evolution {
                name: "speed".into(),
                override_value: 0.0,
                is_overridden: false,
            }],
            props: vec![EvolvedProp {
                id: 0xABCD_0123,
                blend_mode: 0,
                keyframes: vec![keyframe(0, 3, [1, 0]), keyframe(7, 2, [0, 1])],
            }],
        };
        let mut lines = Vec::new();
        evolved("E", Some(&list), &mut lines);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("hash_ABCD0123"), "{}", lines[0]);
        assert!(lines[0].contains("speed"), "{}", lines[0]);
        // An evolution its list does not hold is named by its number.
        assert!(lines[1].contains(" 7 "), "{}", lines[1]);
        evolved("E", None, &mut lines);
        assert_eq!(lines.len(), 2);
    }
}
