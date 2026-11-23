//! What a GTA VI list's evolved keyframes, colours, labelled behaviours, shader words and sprites
//! become in the Particle workspace.

use or_io::gta6::hash::name_hash32;
use or_io::gta6::headerless::{EvolvedKeyframe, EvolvedProp};

use super::*;

fn named(name: &str) -> u32 {
    name_hash32(name.as_bytes())
}

fn evolution(name: &str) -> Evolution {
    Evolution {
        name: name.into(),
        override_value: 0.0,
        is_overridden: false,
    }
}

#[test]
fn an_evolved_value_becomes_the_gta_v_evolved_keyframes_the_sliders_play() {
    let keys = vec![KfpEntry {
        time: [0.0; 4],
        value: [2.0, 3.0, 0.0, 0.0],
    }];
    let list = EvolutionList {
        evolutions: vec![evolution("speed"), evolution("lod_far")],
        props: vec![EvolvedProp {
            id: COLOUR_RGBA_KFPS[1],
            blend_mode: 2,
            keyframes: vec![
                EvolvedKeyframe {
                    entries: keys.clone(),
                    evolution: 0,
                    kind: 3,
                    flags: [1, 0],
                },
                EvolvedKeyframe {
                    entries: keys,
                    evolution: 1,
                    kind: 1,
                    flags: [0, 0],
                },
            ],
        }],
    };
    let gta5 = super::super::evolution_list(&list);
    let [prop] = &gta5.evolved_keyframe_props[..] else {
        panic!("one evolved value");
    };
    assert_eq!(
        (prop.property_id.get(), prop.blend_mode),
        (KFP_COLOUR[1], 2),
        "a colour's maximum is keyed by GTA V's id"
    );
    let played: Vec<(i32, bool, f32)> = (prop.evolved_keyframes.iter())
        .map(|k| {
            let y = k.keyframe.entries[0].value.y;
            (k.evolution_idx, k.is_lod_evolution.get(), y)
        })
        .collect();
    assert_eq!(played, [(0, false, 3.0), (1, true, 3.0)]);
}

#[test]
fn a_colours_unnamed_values_are_its_minimum_and_maximum_and_a_last_frame_of_minus_1_is_0() {
    let min = [([0.0; 4], [0.25, 0.5, 0.75, 1.0])];
    let max = [([0.0; 4], [1.0, 0.5, 0.25, 0.5])];
    let kfps = vec![
        value(COLOUR_RGBA_KFPS[0], &min),
        value(COLOUR_RGBA_KFPS[1], &max),
    ];
    let colour = behaviour(Some("ptxu_Colour"), 0x1E0, kfps);
    let Some(PtxBehaviourAny::PtxuColour(c)) = super::super::behaviour(&colour, 1) else {
        panic!("a ptxu_Colour");
    };
    assert_eq!(c.rgba_min_kfp.property_id.get(), KFP_COLOUR[0]);
    assert_eq!(c.rgba_min_kfp.keyframe.entries[0].value.z, 0.75);
    assert_eq!(c.rgba_max_kfp.keyframe.entries[0].value.x, 1.0);

    let mut animated = behaviour(Some("ptxu_AnimateTexture"), 0x78, vec![]);
    animated.fields = BehaviourFields::AnimateTexture {
        keyframe_mode: 1,
        last_frame_id: -1,
        loop_mode: 4,
        flags: [0; 4],
    };
    let Some(PtxBehaviourAny::PtxuAnimateTexture(a)) = super::super::behaviour(&animated, 1) else {
        panic!("a ptxu_AnimateTexture");
    };
    assert_eq!((a.keyframe_mode, a.last_frame_id, a.loop_mode), (1, 0, 4));
}

#[test]
fn a_sprite_rules_last_behaviour_is_a_ptxd_sprite_of_its_alignment_and_flip_chances() {
    let mut sprite = behaviour(None, 0x50, vec![]);
    sprite.fields = BehaviourFields::Sprite {
        flip_chance: [0.5, 0.25],
        alignment: 3,
        bytes: [0; 16],
    };
    let Some(PtxBehaviourAny::PtxdSprite(s)) = super::super::behaviour(&sprite, 3) else {
        panic!("a ptxd_Sprite");
    };
    assert_eq!(
        (s.alignment_mode, s.flip_chance_u, s.flip_chance_v),
        (3, 0.5, 0.25)
    );
    assert_eq!(s.base.hash_name, BEHAVIOUR_HASH_SPRITE);
}

#[test]
fn a_rules_sprite_is_kept_with_its_key_slot_and_frames() {
    let mut list = list();
    assert!(
        sprites::of_list(&list).is_empty(),
        "slot 4 alone is no sprite"
    );
    list.particles[0].texture_keys[6] = 0x0A;
    list.particles[0].texture_keys[7] = 0x0B;
    let kept = sprites::of_list(&list);
    let sprite = sprites::Sprite {
        key: 0x0A,
        slot: 6,
        frames: 6,
        ramp: Some(0x0B),
    };
    assert_eq!(kept.get("part_a"), Some(&sprite));
    list.behaviours[2].fields = BehaviourFields::AnimateTexture {
        keyframe_mode: 0,
        last_frame_id: 15,
        loop_mode: 1,
        flags: [0; 4],
    };
    let frames = sprites::of_list(&list).get("part_a").map(|s| s.frames);
    assert_eq!(frames, Some(16));
    list.particles[0].draw_type = 1;
    assert!(
        sprites::of_list(&list).is_empty(),
        "a model rule draws no sprite"
    );
}

#[test]
fn the_lines_name_the_labelled_behaviours_the_shader_words_and_the_sprite() {
    let mut list = list();
    list.behaviours[2].fields = BehaviourFields::Sprite {
        flip_chance: [0.5, 0.0],
        alignment: 10,
        bytes: [0; 16],
    };
    list.particles[0].shader_words = vec![
        (named("lightingmode"), named("unlit")),
        (named("projectionmode"), named("none")),
        (named("diffusemode"), named("rgba")),
        (named("soft"), 0),
    ];
    list.particles[0].texture_keys[0] = 0x0C;
    let said = lines(&list);
    let has = |part: &str| said.iter().any(|l| l.contains(part));
    assert!(has(
        "shown as ptxd_Sprite by its class bit: alignment mode 10"
    ));
    assert!(has("Its alignment mode 10 is not known"));
    assert!(has(
        "lightingmode unlit, projectionmode none and diffusemode rgba"
    ));
    assert!(has("Its shader switches: soft."));
    assert!(has("Its sprite is texture slot 0, of 6 frames."));

    list.behaviours[2].fields = BehaviourFields::Unclassed(0x89DC_E430);
    list.behaviours[2].class_bit = Some(11);
    let said = lines(&list);
    let has = |part: &str| said.iter().any(|l| l.contains(part));
    assert!(has("id is hash_89DCE430"));
    assert!(has(
        "of class bit 11, which GTA V's update order names ptxu_Wind"
    ));
}
