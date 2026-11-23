//! Hand-built DXBC containers for tests, spelled with literal numbers so they do not share a mistake with the code they test.

pub fn dwords(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

pub fn signature(name: &str, system_value: u32) -> Vec<u8> {
    let mut out = dwords(&[1, 8, 32, 0, system_value, 3, 0]);
    out.extend_from_slice(&[15, 0, 0, 0]);
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

/// A container of `(tag, data)` parts.
pub fn container(parts: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_end = 32 + parts.len() * 4;
    let mut offsets = Vec::new();
    let mut body = Vec::new();
    for (tag, data) in parts {
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

/// A `ps_4_0` that writes (1, 0, 0, 1) to its one render target.
pub fn red_pixel_shader() -> Vec<u8> {
    container(&[
        (*b"ISGN", signature("SV_Position", 1)),
        (*b"OSGN", signature("SV_Target", 0)),
        (
            *b"SHDR",
            dwords(&[
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
            ]),
        ),
    ])
}

/// An `RDEF` part listing `(name, kind, register)` for each binding, `kind` being Direct3D's input type: 0 a constant buffer, 2 a texture, 3 a sampler, 4 a typed UAV, 9 an append UAV.
pub fn rdef(bindings: &[(&str, u32, u32)]) -> Vec<u8> {
    rdef_with_buffers(bindings, &[])
}

/// A constant buffer as `(name, size, [(variable, offset, size)])`.
pub type BufferSpec<'a> = (&'a str, u32, &'a [(&'a str, u32, u32)]);

/// [`rdef`] that also defines constant buffers, every variable a `float4`.
pub fn rdef_with_buffers(bindings: &[(&str, u32, u32)], buffers: &[BufferSpec<'_>]) -> Vec<u8> {
    let buffer_table = 28;
    let binding_table = buffer_table + buffers.len() * 24;
    let variables: usize = buffers.iter().map(|b| b.2.len()).sum();
    let variable_table = binding_table + bindings.len() * 32;
    let type_at = variable_table + variables * 24;
    let names_at = type_at + 16;
    let mut names = Vec::new();
    let mut name = |text: &str| {
        let at = (names_at + names.len()) as u32;
        names.extend_from_slice(text.as_bytes());
        names.push(0);
        at
    };
    let mut buffer_entries = Vec::new();
    let mut variable_entries = Vec::new();
    let mut placed = 0;
    for (buffer, size, members) in buffers {
        buffer_entries.extend(dwords(&[
            name(buffer),
            members.len() as u32,
            (variable_table + placed * 24) as u32,
            *size,
            0,
            0,
        ]));
        for (variable, offset, size) in *members {
            // Flag 2 marks a variable the shader reads; the type is the one `float4` below
            variable_entries.extend(dwords(&[
                name(variable),
                *offset,
                *size,
                2,
                type_at as u32,
                0,
            ]));
            placed += 1;
        }
    }
    let mut binding_entries = Vec::new();
    for (binding, kind, register) in bindings {
        binding_entries.extend(dwords(&[name(binding), *kind, 0, 0, 0, *register, 1, 0]));
    }
    // Shader model 4.0
    let mut out = dwords(&[
        buffers.len() as u32,
        buffer_table as u32,
        bindings.len() as u32,
        binding_table as u32,
        0x400,
        0,
        0,
    ]);
    out.extend(buffer_entries);
    out.extend(binding_entries);
    out.extend(variable_entries);
    // class 1 (vector), type 3 (float), 1 row, 4 columns, no array
    out.extend(dwords(&[0x0003_0001, 0x0004_0001, 0, 0]));
    out.extend(names);
    out
}

/// A `ps_4_0` that samples `t{texture}` through `s{sampler}` at its first texture coordinate and scales the result by `cb{buffer}[0]`, with `reflection` as its `RDEF` part when given.
pub fn textured_pixel_shader(
    buffer: u32,
    texture: u32,
    sampler: u32,
    reflection: Option<Vec<u8>>,
) -> Vec<u8> {
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
    let mut parts = vec![
        (*b"ISGN", signature("TEXCOORD", 0)),
        (*b"OSGN", signature("SV_Target", 0)),
        (*b"SHDR", dwords(&program)),
    ];
    if let Some(rdef) = reflection {
        parts.insert(0, (*b"RDEF", rdef));
    }
    container(&parts)
}

/// A `vs_4_0` that passes its `POSITION` input through as `SV_Position`.
pub fn plain_vertex_shader() -> Vec<u8> {
    container(&[
        (*b"ISGN", signature("POSITION", 0)),
        (*b"OSGN", signature("SV_Position", 1)),
        (
            *b"SHDR",
            dwords(&[
                0x0001_0040,
                15,
                // dcl_input v0.xyzw
                0x0300_005f,
                0x0010_10f2,
                0,
                // dcl_output_siv o0.xyzw, position
                0x0400_0067,
                0x0010_20f2,
                0,
                1,
                // mov o0.xyzw, v0.xyzw
                0x0500_0036,
                0x0010_20f2,
                0,
                0x0010_1e46,
                0,
                // ret
                0x0100_003e,
            ]),
        ),
    ])
}

/// A `ps_4_0` that writes (1, 0, 0, 1) and then discards: `discard_nz l(-1)`.
pub fn discarding_pixel_shader() -> Vec<u8> {
    container(&[
        (*b"ISGN", signature("SV_Position", 1)),
        (*b"OSGN", signature("SV_Target", 0)),
        (
            *b"SHDR",
            dwords(&[
                0x40,
                17,
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
                // discard_nz l(-1)
                0x0304_000d,
                0x4001,
                0xffff_ffff,
                0x0100_003e,
            ]),
        ),
    ])
}

/// A `vs_4_0` that writes its `POSITION` input to a `TEXCOORD` output and has no `SV_Position`, which naga's reader refuses to take as a vertex shader.
pub fn vertex_shader_without_a_position() -> Vec<u8> {
    container(&[
        (*b"ISGN", signature("POSITION", 0)),
        (*b"OSGN", signature("TEXCOORD", 0)),
        (
            *b"SHDR",
            dwords(&[
                0x0001_0040,
                14,
                0x0300_005f,
                0x0010_10f2,
                0,
                0x0300_0065,
                0x0010_20f2,
                0,
                0x0500_0036,
                0x0010_20f2,
                0,
                0x0010_1e46,
                0,
                0x0100_003e,
            ]),
        ),
    ])
}

/// A `vs_4_0` that writes `cb{register}[0]` to `SV_Position`, with `reflection` as its `RDEF` part when given.
pub fn vertex_shader_with_a_buffer(register: u32, reflection: Option<Vec<u8>>) -> Vec<u8> {
    let mut parts = vec![
        (*b"ISGN", signature("POSITION", 0)),
        (*b"OSGN", signature("SV_Position", 1)),
        (
            *b"SHDR",
            dwords(&[
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
            ]),
        ),
    ];
    if let Some(rdef) = reflection {
        parts.insert(0, (*b"RDEF", rdef));
    }
    container(&parts)
}
