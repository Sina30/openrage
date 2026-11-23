use or_io::gta5::shaders::spirv_pack::SignatureElement;

use super::*;

fn input(semantic: &str, register: u32) -> SignatureElement {
    SignatureElement {
        semantic: semantic.into(),
        index: 0,
        system_value: 0,
        component_type: 3,
        register,
        mask: 15,
    }
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|w| f32::from_le_bytes(*w)).collect()
}

#[test]
fn a_position_is_the_corner_in_the_range_the_vertex_stage_maps_and_a_texture_coordinate_runs_down()
{
    let attributes = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 8,
            shader_location: 1,
        },
    ];
    let inputs = [input("POSITION", 0), input("TEXCOORD", 1)];
    let unit = floats(&quad_vertices(&attributes, 16, &inputs, Space::Unit));
    let clip = floats(&quad_vertices(&attributes, 16, &inputs, Space::Clip));
    // Six vertices of four numbers: the second is the corner at the right of the bottom edge
    assert_eq!(unit.len(), 24);
    assert_eq!(&unit[4..8], &[1.0, 0.0, 1.0, 1.0]);
    assert_eq!(&clip[4..8], &[1.0, -1.0, 1.0, 1.0]);
    // Whichever range the corner is written in, the top of the picture is the top of the target
    assert_eq!(&unit[20..24], &[0.0, 1.0, 0.0, 0.0]);
    assert_eq!(&clip[20..24], &[-1.0, 1.0, 0.0, 0.0]);
}

#[test]
fn an_input_no_semantic_names_reads_nothing() {
    let attributes = [wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 0,
        shader_location: 3,
    }];
    let bytes = quad_vertices(&attributes, 16, &[input("COLOR", 3)], Space::Unit);
    assert!(floats(&bytes).iter().all(|v| *v == 0.0));
}

#[test]
fn constants_are_written_again_only_when_what_they_were_written_for_changed() {
    let written = Written::default();
    assert!(written.changed([1.0, 2.0]), "nothing was written yet");
    assert!(!written.changed([1.0, 2.0]));
    assert!(!written.changed([1.0, 2.0]));
    assert!(written.changed([1.0, 3.0]));
    assert!(!written.changed([1.0, 3.0]));
    // Buffers made again hold nothing, whatever was written to the old ones
    written.reset();
    assert!(written.changed([1.0, 3.0]));
}
