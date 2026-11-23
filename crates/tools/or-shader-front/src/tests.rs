use super::*;

fn dwords(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A signature part with one element: its semantic name, system value, register and mask.
fn signature(name: &str, system_value: u32, register: u32, mask: u8) -> Vec<u8> {
    let mut out = dwords(&[1, 8, 32, 0, system_value, 3, register]);
    out.extend_from_slice(&[mask, 0, 0, 0]);
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

/// A container holding `program` as its `SHDR`, with the signatures of a pixel shader that writes `SV_Target`.
fn container_with(program: &[u32]) -> Vec<u8> {
    container_reading(program, signature("SV_Position", 1, 0, 15))
}

/// [`container_with`] reading the input signature `isgn`.
fn container_reading(program: &[u32], isgn: Vec<u8>) -> Vec<u8> {
    let parts = [
        (*b"ISGN", isgn),
        (*b"OSGN", signature("SV_Target", 0, 0, 15)),
        (*b"SHDR", dwords(program)),
    ];
    let table_end = 32 + parts.len() * 4;
    let mut offsets = Vec::new();
    let mut body = Vec::new();
    for (tag, data) in &parts {
        offsets.push((table_end + body.len()) as u32);
        body.extend_from_slice(tag);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
    }
    let mut out = b"DXBC".to_vec();
    out.extend_from_slice(&[0; 16]);
    out.extend_from_slice(&[1, 0, 0, 0]);
    out.extend_from_slice(&((table_end + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(parts.len() as u32).to_le_bytes());
    for offset in offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend(body);
    out
}

/// A `vs_4_0` that writes `cb{register}[0]` to `SV_Position`.
fn vertex_shader_reading(register: u32) -> Vec<u8> {
    container_reading(
        &[
            0x0001_0040,
            16,
            // dcl_constantbuffer cb[register][1], immediateIndexed
            0x0400_0059,
            0x0020_8e4a,
            register,
            1,
            // dcl_output_siv o0.xyzw, position
            0x0400_0067,
            0x0010_20f2,
            0,
            1,
            // mov o0.xyzw, cb[register][0].xyzw
            0x0600_0036,
            0x0010_20f2,
            0,
            0x0020_8e46,
            register,
            0,
            // ret
            0x0100_003e,
        ],
        signature("POSITION", 0, 0, 15),
    )
}

/// A `ps_4_0` that writes the colour (1, 0, 0, 1): `dcl_output o0.xyzw`, `mov o0.xyzw, l(1, 0, 0, 1)`, `ret`.
fn red_pixel_shader() -> Vec<u8> {
    container_with(&[
        0x40,
        14,
        0x0300_0065,
        0x0010_20f2,
        0,
        0x0800_0036,
        0x0010_20f2,
        0,
        0x4002,
        0x3f80_0000,
        0,
        0,
        0x3f80_0000,
        0x0100_003e,
    ])
}

#[test]
fn the_identity_names_the_vendored_revision_the_licence_notice_records() {
    let identity = identity();
    let prefix = format!("dxbc-spirv {VENDORED_REVISION} ");
    let hash = identity
        .strip_prefix(&prefix)
        .expect("the revision leads the identity");
    assert_eq!(hash.len(), 16, "{identity}");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{identity}");
    assert!(
        include_str!("../../../../LICENSE-third-party.md").contains(VENDORED_REVISION),
        "LICENSE-third-party.md names another revision of dxbc-spirv"
    );
}

#[test]
fn a_register_lands_at_its_classs_base_plus_the_register_in_the_set_of_its_space() {
    let layout = BindingLayout::OPENRAGE;
    let at = |class, space, register| {
        layout
            .binding(class, space, register)
            .map(|b| (b.set, b.binding))
    };
    assert_eq!(at(ResourceClass::Cbv, 0, 3), Some((0, 3)));
    assert_eq!(at(ResourceClass::Sampler, 0, 2), Some((0, 18)));
    assert_eq!(at(ResourceClass::Srv, 0, 5), Some((0, 37)));
    assert_eq!(at(ResourceClass::Uav, 0, 1), Some((0, 161)));
    assert_eq!(at(ResourceClass::UavCounter, 0, 1), Some((0, 225)));
    // A register space steps the set by two, so each space has a set for each of the vertex and pixel stages
    assert_eq!(at(ResourceClass::Srv, 2, 5), Some((4, 37)));
}

#[test]
fn a_pixel_shaders_registers_take_the_set_after_a_vertex_shaders() {
    let in_stage = |program_type, space| {
        BindingLayout::OPENRAGE
            .for_program_type(program_type)
            .binding(ResourceClass::Srv, space, 5)
            .map(|b| (b.set, b.binding))
    };
    // Program types: 0 pixel, 1 vertex, 5 compute
    assert_eq!(in_stage(1, 0), Some((0, 37)));
    assert_eq!(in_stage(0, 0), Some((1, 37)));
    assert_eq!(in_stage(5, 0), Some((0, 37)));
    assert_eq!(in_stage(1, 1), Some((2, 37)));
    assert_eq!(in_stage(0, 1), Some((3, 37)));
}

#[test]
fn the_program_type_is_read_from_the_programs_first_word() {
    assert_eq!(program_type(&red_pixel_shader()), Some(0));
    assert_eq!(program_type(&vertex_shader_reading(3)), Some(1));
    assert_eq!(program_type(b"not a container"), None);
    assert_eq!(program_type(&[]), None);
    let blob = red_pixel_shader();
    assert_eq!(program_type(&blob[..40]), None);
}

#[test]
fn a_register_past_the_end_of_its_window_has_no_binding() {
    let layout = BindingLayout::OPENRAGE;
    assert!(layout.binding(ResourceClass::Cbv, 0, 15).is_some());
    assert_eq!(layout.binding(ResourceClass::Cbv, 0, 16), None);
    assert!(layout.binding(ResourceClass::Srv, 0, 127).is_some());
    assert_eq!(layout.binding(ResourceClass::Srv, 0, 128), None);
}

#[test]
fn no_two_classes_share_a_binding_in_one_set() {
    let layout = BindingLayout::OPENRAGE;
    let classes = [
        ResourceClass::Cbv,
        ResourceClass::Sampler,
        ResourceClass::Srv,
        ResourceClass::Uav,
        ResourceClass::UavCounter,
    ];
    for a in classes {
        for b in classes.iter().copied().filter(|b| *b != a) {
            let (wa, wb) = (layout.window(a), layout.window(b));
            assert!(
                wa.base + wa.len <= wb.base || wb.base + wb.len <= wa.base,
                "{a:?} and {b:?} overlap"
            );
        }
    }
}

#[cfg(feature = "native")]
mod native {
    use super::*;

    /// A `ps_4_0` that samples `t{texture}` through `s{sampler}` at its first texture coordinate and scales the result by `cb{buffer}[0]`.
    fn textured_pixel_shader(buffer: u32, texture: u32, sampler: u32) -> Vec<u8> {
        let program = [
            0x40,
            39,
            // dcl_constantbuffer cb[buffer][1], immediateIndexed
            0x0400_0059,
            0x0020_8e4a,
            buffer,
            1,
            // dcl_sampler s[sampler], mode_default
            0x0300_005a,
            0x0010_6000,
            sampler,
            // dcl_resource_texture2d (float, float, float, float) t[texture]
            0x0400_1858,
            0x0010_7000,
            texture,
            0x5555,
            // dcl_input_ps linear v0.xy
            0x0300_1062,
            0x0010_1032,
            0,
            // dcl_output o0.xyzw
            0x0300_0065,
            0x0010_20f2,
            0,
            // dcl_temps 1
            0x0200_0068,
            1,
            // sample r0.xyzw, v0.xyxx, t[texture].xyzw, s[sampler]
            0x0900_0045,
            0x0010_00f2,
            0,
            0x0010_1046,
            0,
            0x0010_7e46,
            texture,
            0x0010_6000,
            sampler,
            // mul o0.xyzw, r0.xyzw, cb[buffer][0].xyzw
            0x0800_0038,
            0x0010_20f2,
            0,
            0x0010_0e46,
            0,
            0x0020_8e46,
            buffer,
            0,
            // ret
            0x0100_003e,
        ];
        container_reading(&program, signature("TEXCOORD", 0, 0, 3))
    }

    /// A `ps_5_1` that copies `cb0[0]` to its target, the constant buffer being register `b{register}` in space `space`.
    fn shader_model_5_1_pixel_shader(register: u32, space: u32) -> Vec<u8> {
        container_with(&[
            0x51,
            20,
            // dcl_constantbuffer CB0[register:register][1], immediateIndexed, space = space
            0x0700_0059,
            0x0030_8e4a,
            0,
            register,
            register,
            1,
            space,
            // dcl_output o0.xyzw
            0x0300_0065,
            0x0010_20f2,
            0,
            // mov o0.xyzw, CB0[register][0].xyzw: the range, the absolute register, the element
            0x0700_0036,
            0x0010_20f2,
            0,
            0x0030_8e46,
            0,
            register,
            0,
            // ret
            0x0100_003e,
        ])
    }

    /// Every `(set, binding)` the module decorates, sorted.
    fn descriptor_bindings(spirv: &[u8]) -> Vec<(u32, u32)> {
        let words = words(spirv);
        let mut sets = std::collections::BTreeMap::new();
        let mut bindings = std::collections::BTreeMap::new();
        let mut at = 5;
        while at < words.len() {
            let count = (words[at] >> 16) as usize;
            if words[at] & 0xffff == 71 {
                match words[at + 2] {
                    34 => drop(sets.insert(words[at + 1], words[at + 3])),
                    33 => drop(bindings.insert(words[at + 1], words[at + 3])),
                    _ => {}
                }
            }
            at += count.max(1);
        }
        let mut out: Vec<(u32, u32)> = bindings
            .iter()
            .map(|(id, binding)| (sets[id], *binding))
            .collect();
        out.sort_unstable();
        out
    }

    /// A `ps_4_0` that converts `cb0[0].x` from float to integer: `ftoi o0.x, cb0[0].x`.
    fn float_to_integer_pixel_shader() -> Vec<u8> {
        container_with(&[
            0x40,
            16,
            0x0400_0059,
            0x0020_8e4a,
            0,
            1,
            0x0300_0065,
            0x0010_20f2,
            0,
            0x0600_001b,
            0x0010_2012,
            0,
            0x0020_800a,
            0,
            0,
            0x0100_003e,
        ])
    }

    /// A `ps_4_0` that reads one register of `cb0` as a float, a two-component float and an integer, so a typed layout would hold members at bytes 32, 36 and 44.
    fn constant_buffer_pixel_shader() -> Vec<u8> {
        container_with(&[
            0x40,
            28,
            // dcl_constantbuffer cb0[3], immediateIndexed
            0x0400_0059,
            0x0020_8e4a,
            0,
            3,
            // dcl_output o0.xyzw
            0x0300_0065,
            0x0010_20f2,
            0,
            // mov o0.x, cb0[2].x
            0x0600_0036,
            0x0010_2012,
            0,
            0x0020_800a,
            0,
            2,
            // mov o0.yz, cb0[2].yzzz
            0x0600_0036,
            0x0010_2062,
            0,
            0x0020_8a9a,
            0,
            2,
            // utof o0.w, cb0[2].wwww
            0x0600_0056,
            0x0010_2082,
            0,
            0x0020_8ffa,
            0,
            2,
            // ret
            0x0100_003e,
        ])
    }

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// The execution model of every entry point in the module.
    fn entry_models(words: &[u32]) -> Vec<u32> {
        let mut models = Vec::new();
        let mut at = 5;
        while at < words.len() {
            let count = (words[at] >> 16) as usize;
            if words[at] & 0xffff == 15 {
                models.push(words[at + 1]);
            }
            at += count.max(1);
        }
        models
    }

    #[test]
    fn a_pixel_shader_that_writes_a_colour_becomes_a_fragment_module() {
        let spirv = dxbc_to_spirv(&red_pixel_shader()).expect("the translator takes the shader");
        let words = words(&spirv);
        assert_eq!(words[0], 0x0723_0203, "the SPIR-V magic number");
        assert_eq!(words[1] >> 16, 1, "version 1.x");
        assert_eq!(entry_models(&words), vec![4], "one fragment entry point");
    }

    /// Every `Offset` decoration of a structure member, as `(member, byte offset)`.
    fn member_offsets(words: &[u32]) -> Vec<(u32, u32)> {
        let mut offsets = Vec::new();
        let mut at = 5;
        while at < words.len() {
            let count = (words[at] >> 16) as usize;
            if words[at] & 0xffff == 72 && words[at + 3] == 35 {
                offsets.push((words[at + 2], words[at + 4]));
            }
            at += count.max(1);
        }
        offsets
    }

    #[test]
    fn a_constant_buffer_is_a_vector_array_with_no_member_at_an_odd_offset() {
        let spirv =
            dxbc_to_spirv(&constant_buffer_pixel_shader()).expect("the translator takes it");
        let offsets = member_offsets(&words(&spirv));
        assert!(
            offsets.iter().all(|(_, offset)| offset % 16 == 0),
            "a register array has no member inside a register: {offsets:?}"
        );
    }

    #[test]
    fn a_float_to_integer_conversion_is_native_and_not_a_call_to_a_helper_function() {
        let spirv =
            dxbc_to_spirv(&float_to_integer_pixel_shader()).expect("the translator takes it");
        let mut calls = 0;
        let words = words(&spirv);
        let mut at = 5;
        while at < words.len() {
            calls += usize::from(words[at] & 0xffff == 57);
            at += (words[at] >> 16) as usize;
        }
        assert_eq!(
            calls, 0,
            "no OpFunctionCall: naga's Metal writer trips over a call's result"
        );
    }

    #[test]
    fn each_register_is_bound_where_the_layout_places_it() {
        let spirv = dxbc_to_spirv(&textured_pixel_shader(3, 5, 2)).expect("translates");
        // b3 at 3, s2 at 16 + 2 and t5 at 32 + 5, all in the pixel stage's set
        assert_eq!(descriptor_bindings(&spirv), vec![(1, 3), (1, 18), (1, 37)]);
    }

    #[test]
    fn a_vertex_shaders_registers_are_in_the_set_before_a_pixel_shaders() {
        let spirv = dxbc_to_spirv(&vertex_shader_reading(3)).expect("translates");
        assert_eq!(descriptor_bindings(&spirv), vec![(0, 3)]);
    }

    #[test]
    fn a_layout_other_than_the_default_moves_every_binding() {
        let layout = BindingLayout {
            cbv: Window { base: 100, len: 4 },
            sampler: Window { base: 200, len: 4 },
            srv: Window { base: 300, len: 8 },
            ..BindingLayout::OPENRAGE
        };
        let blob = textured_pixel_shader(3, 5, 2);
        let spirv = dxbc_to_spirv_with(&blob, &layout).expect("translates");
        // The layout is taken as given, with no stage added to it
        assert_eq!(
            descriptor_bindings(&spirv),
            vec![(0, 103), (0, 202), (0, 305)]
        );
    }

    #[test]
    fn a_register_space_steps_the_set_by_two_from_the_stages_own() {
        let spirv = dxbc_to_spirv(&shader_model_5_1_pixel_shader(3, 2)).expect("translates");
        // The pixel stage's set 1, and two spaces on
        assert_eq!(descriptor_bindings(&spirv), vec![(5, 3)]);
        let spirv = dxbc_to_spirv(&shader_model_5_1_pixel_shader(3, 0)).expect("translates");
        assert_eq!(descriptor_bindings(&spirv), vec![(1, 3)]);
    }

    #[test]
    fn a_register_the_layout_has_no_room_for_refuses_the_shader_and_says_which() {
        let blob = textured_pixel_shader(16, 5, 2);
        let Err(FrontError::Refused(message)) = dxbc_to_spirv(&blob) else {
            panic!("a constant buffer at b16 is past the window");
        };
        assert!(message.contains("constant buffer b16"), "{message}");
    }

    #[test]
    fn the_same_shader_translates_to_the_same_bytes_each_time() {
        let blob = red_pixel_shader();
        assert_eq!(dxbc_to_spirv(&blob).unwrap(), dxbc_to_spirv(&blob).unwrap());
    }

    #[test]
    fn bytes_that_are_not_a_container_are_refused_with_the_translators_message() {
        let Err(FrontError::Refused(message)) = dxbc_to_spirv(b"not a shader at all, no") else {
            panic!("expected a refusal");
        };
        assert!(message.contains("not a DXBC container"), "{message}");
        assert!(matches!(dxbc_to_spirv(&[]), Err(FrontError::Refused(_))));
    }

    #[test]
    fn a_container_cut_short_is_refused_and_not_translated() {
        let blob = red_pixel_shader();
        assert!(dxbc_to_spirv(&blob[..blob.len() - 20]).is_err());
    }
}

#[cfg(not(feature = "native"))]
#[test]
fn without_the_native_feature_every_call_says_so() {
    assert_eq!(
        dxbc_to_spirv(&red_pixel_shader()),
        Err(FrontError::NotCompiledIn)
    );
}
