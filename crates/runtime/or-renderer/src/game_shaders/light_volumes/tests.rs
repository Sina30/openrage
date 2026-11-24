use super::*;

#[test]
fn the_records_of_many_lights_are_each_lights_record_side_by_side() {
    let input = |semantic: &str, index: u32, register: u32| SignatureElement {
        semantic: semantic.into(),
        index,
        system_value: 0,
        component_type: 3,
        register,
        mask: 15,
    };
    let inputs = [input("TEXCOORD", 0, 1), input("TEXCOORD", 1, 3)];
    let attribute = |offset, shader_location, format| wgpu::VertexAttribute {
        format,
        offset,
        shader_location,
    };
    // A float4, a float3, and one whose location no input reads, in a stride with room left over
    let attributes = [
        attribute(0, 1, wgpu::VertexFormat::Float32x4),
        attribute(16, 3, wgpu::VertexFormat::Float32x3),
        attribute(28, 9, wgpu::VertexFormat::Float32),
    ];
    let value = |i: &SignatureElement, light: &f32| [*light, i.index as f32, 2.0, 3.0];
    let lights = [1.5f32, -4.0, 7.25];
    let one = |light: &f32| record_bytes(&attributes, 40, &inputs, &|i| value(i, light));
    let each: Vec<u8> = lights.iter().flat_map(one).collect();
    assert_eq!(records(&attributes, 40, &inputs, &lights, &value), each);
    assert!(records(&attributes, 40, &inputs, &[] as &[f32], &value).is_empty());
}

#[test]
fn the_sphere_is_a_unit_one_cut_into_the_triangles_the_cuts_make_and_every_one_faces_outward() {
    let corners = sphere();
    assert_eq!(corners.len(), 8 * 4usize.pow(CUTS) * 3);
    for triangle in corners.as_chunks::<3>().0 {
        for corner in triangle {
            assert!((corner.length() - 1.0).abs() < 1e-5, "{corner:?}");
        }
        let normal = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
        let middle = (triangle[0] + triangle[1] + triangle[2]) / 3.0;
        assert!(normal.dot(middle) > 0.0, "{triangle:?} faces inward");
    }
}
