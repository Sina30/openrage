use or_io::gta5::shaders::shader_defs::{ShaderDef, ShaderParamDef, ShaderParamKind};

use super::*;

const TEXTURE: u32 = 0x1111_0001;
const KEY: u64 = 0x0123_4567_89AB_CDEF;
/// A member of a buffer the engine fills, which no drawable holds.
const ENGINE: u32 = 0x4444_0001;

/// A declared parameter of `kind` named `hash_` and its hash, in `cbuffer`, at `place` (its
/// buffer's rank among those a drawable holds, and its byte offset) for a constant a drawable holds.
fn def(
    hash: u32,
    kind: ShaderParamKind,
    cbuffer: Option<&str>,
    place: Option<(u8, u16)>,
) -> ShaderParamDef {
    ShaderParamDef {
        name: format!("hash_{hash:08X}"),
        name_hash: hash,
        kind,
        array_len: 1,
        register: place.map_or(0, |(_, offset)| (offset / 16) as u8),
        cbuffer: cbuffer.map(str::to_string),
        drawable_place: place,
        semantic: None,
        semantic_hash: None,
        ui_name: None,
        default: None,
        preset_default: None,
    }
}

/// An effect of one texture, four constants in `cb0`, one in `cb1` and one in a buffer the engine
/// fills; the float2 sets a default and lies at 8, where no packing would put it.
fn effect() -> ShaderDef {
    let mut float2 = def(
        0x3333_0002,
        ShaderParamKind::Float2,
        Some("cb0"),
        Some((0, 8)),
    );
    float2.default = Some(ShaderParamValue::Floats(vec![0.5, 2.0]));
    ShaderDef {
        name: "made_up_effect".into(),
        name_hash: 0x00C0_FFEE,
        effect: "made_up_effect".into(),
        draw_bucket: None,
        vertex_type: 0,
        traits: Vec::new(),
        parameters: vec![
            def(TEXTURE, ShaderParamKind::Texture, None, None),
            def(ENGINE, ShaderParamKind::Float4, Some("cbEngine"), None),
            def(
                0x3333_0001,
                ShaderParamKind::Float,
                Some("cb0"),
                Some((0, 0)),
            ),
            float2,
            def(
                0x3333_0003,
                ShaderParamKind::Float3,
                Some("cb0"),
                Some((0, 16)),
            ),
            def(
                0x3333_0004,
                ShaderParamKind::Float4x4,
                Some("cb0"),
                Some((0, 32)),
            ),
            def(
                0x3333_0005,
                ShaderParamKind::Float4,
                Some("cb1"),
                Some((1, 0)),
            ),
        ],
    }
}

fn constant(hash: u32, buffer: u8, offset: u16, words: Vec<u32>) -> Parameter {
    Parameter {
        hash,
        value: ParameterValue::Constant {
            buffer,
            offset,
            words,
        },
    }
}

/// Where each constant of `s` lies: its hash, buffer, offset and word count.
fn laid(s: &Shader) -> Vec<(u32, u8, u16, usize)> {
    (s.parameters.iter())
        .filter_map(|p| match &p.value {
            ParameterValue::Constant {
                buffer,
                offset,
                words,
            } => Some((p.hash, *buffer, *offset, words.len())),
            ParameterValue::Texture { .. } => None,
        })
        .collect()
}

#[test]
fn a_changed_effect_takes_its_held_constants_where_the_effect_places_them_and_keeps_what_matches() {
    let held = Shader {
        name_hash: 0x0BAD_0001,
        texture_keys: vec![Some(KEY)],
        parameters: vec![
            Parameter {
                hash: TEXTURE,
                value: ParameterValue::Texture { slot: 0 },
            },
            constant(0x3333_0003, 7, 0, vec![1, 2, 3]),
            constant(0x7777_0001, 0, 0, vec![9]),
        ],
    };
    let s = declared_shader(&held, &effect()).expect("laid out");
    assert_eq!(s.name_hash, 0x00C0_FFEE);
    assert_eq!(
        s.texture_keys,
        [Some(KEY)],
        "the slot of its hash keeps its key"
    );
    // Each at the buffer and byte the effect states; the engine's buffer is not held.
    let expected = [
        (0x3333_0001, 0, 0, 1),
        (0x3333_0002, 0, 8, 2),
        (0x3333_0003, 0, 16, 3),
        (0x3333_0004, 0, 32, 16),
        (0x3333_0005, 1, 0, 4),
    ];
    assert_eq!(laid(&s), expected);
    let words = |hash: u32| {
        s.parameters.iter().find_map(|p| match &p.value {
            ParameterValue::Constant { words, .. } if p.hash == hash => Some(words.clone()),
            _ => None,
        })
    };
    assert_eq!(
        words(0x3333_0003),
        Some(vec![1, 2, 3]),
        "words of its size kept"
    );
    let default = [0.5f32, 2.0].map(f32::to_bits).to_vec();
    assert_eq!(words(0x3333_0002), Some(default), "the effect's default");
    assert_eq!(words(0x3333_0001), Some(vec![0]));
    assert!(
        words(0x7777_0001).is_none(),
        "one the effect does not declare goes"
    );
    assert!(words(ENGINE).is_none(), "the engine fills its own buffer");
    assert!(or_io::gta6::headerless::shader_layout_holds(&s).is_ok());

    // An array member of a buffer a drawable holds is refused by name.
    let mut array = effect();
    array.parameters[2].array_len = 4;
    let why = declared_shader(&held, &array).expect_err("an array");
    assert!(why.contains("hash_33330001"), "{why}");
    // An effect whose constants all lie in buffers the engine fills gives its textures alone.
    let mut engine = effect();
    for p in &mut engine.parameters {
        p.drawable_place = None;
    }
    let s = declared_shader(&held, &engine).expect("textures alone");
    assert_eq!((laid(&s), s.texture_keys), (Vec::new(), vec![Some(KEY)]));
}

#[test]
fn a_declared_constant_goes_in_the_buffer_and_at_the_byte_its_effect_states() {
    let effect = effect();
    let held = Shader {
        name_hash: effect.name_hash,
        texture_keys: Vec::new(),
        parameters: vec![constant(0x3333_0001, 0, 0, vec![7])],
    };
    let added = with_declared(&held, &effect, 0x3333_0002).expect("added");
    assert_eq!(
        laid(&added),
        [(0x3333_0001, 0, 0, 1), (0x3333_0002, 0, 8, 2)]
    );
    let other = with_declared(&held, &effect, 0x3333_0005).expect("added");
    assert_eq!(
        laid(&other)[1],
        (0x3333_0005, 1, 0, 4),
        "the buffer its buffer's rank names"
    );
    assert!(with_declared(&held, &effect, 0x3333_0001).is_err(), "held");
    assert!(
        with_declared(&held, &effect, 0x5555_0001).is_err(),
        "undeclared"
    );
    assert!(
        with_declared(&held, &effect, ENGINE).is_err(),
        "a member of a buffer the engine fills"
    );

    // A shader holding a constant elsewhere than the effect places it is not added to.
    let stale = Shader {
        parameters: vec![constant(0x3333_0001, 3, 8, vec![7])],
        ..held.clone()
    };
    assert!(with_declared(&stale, &effect, 0x3333_0002).is_err());
}

#[test]
fn what_is_missing_and_each_default_come_from_the_effect_s_own_list() {
    let effect = effect();
    let held = Shader {
        name_hash: effect.name_hash,
        texture_keys: Vec::new(),
        parameters: vec![constant(0x3333_0001, 0, 0, vec![7])],
    };
    let missing: Vec<(u32, bool)> = (missing(&held, &effect).into_iter())
        .map(|(hash, _, texture)| (hash, texture))
        .collect();
    let expected = [
        (TEXTURE, true),
        (0x3333_0002, false),
        (0x3333_0003, false),
        (0x3333_0004, false),
        (0x3333_0005, false),
    ];
    assert_eq!(missing, expected);
    let default = [0.5f32, 2.0].map(f32::to_bits).to_vec();
    assert_eq!(declared_default(&effect, 0x3333_0002, 2), Some(default));
    assert_eq!(
        declared_default(&effect, 0x3333_0002, 3),
        None,
        "another size"
    );
    assert_eq!(declared_default(&effect, 0x3333_0001, 1), None, "none set");
}
