use super::*;

fn wgsl(source: &str) -> naga::Module {
    naga::front::wgsl::parse_str(source).expect("the WGSL parses")
}

const VERTEX: &str = "
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
    @location(2) c: vec4<f32>,
}
@vertex fn main() -> Out {
    var out: Out;
    return out;
}";

/// How each output location of the vertex entry point is interpolated.
fn outputs(module: &naga::Module) -> Vec<(u32, Option<Interpolation>, Option<Sampling>)> {
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.stage == ShaderStage::Vertex)
        .unwrap();
    let result = entry.function.result.as_ref().unwrap();
    let TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("a struct result");
    };
    members
        .iter()
        .filter_map(|m| match m.binding {
            Some(Binding::Location {
                location,
                interpolation,
                sampling,
                ..
            }) => Some((location, interpolation, sampling)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_vertex_output_takes_the_interpolation_the_pixel_stage_reads_its_location_with() {
    let mut vertex = wgsl(VERTEX);
    let fragment = wgsl(
        "@fragment fn main(
            @location(0) @interpolate(flat) a: vec4<f32>,
            @location(2) @interpolate(linear, centroid) c: vec4<f32>,
        ) -> @location(0) vec4<f32> { return a + c; }",
    );
    // The vertex shader's own: perspective, taken from the centre
    let untouched = Some(Interpolation::Perspective);
    assert!(outputs(&vertex)
        .iter()
        .all(|(_, i, s)| (*i, *s) == (untouched, Some(Sampling::Center))));
    match_interpolation(&mut vertex, &fragment);
    assert_eq!(
        outputs(&vertex),
        vec![
            (0, Some(Interpolation::Flat), None),
            // The pixel stage reads no location 1, so the vertex stage's stays
            (1, untouched, Some(Sampling::Center)),
            (2, Some(Interpolation::Linear), Some(Sampling::Centroid)),
        ]
    );
}

#[test]
fn a_vertex_output_that_is_not_a_struct_is_matched_too() {
    let mut vertex = wgsl("@vertex fn main() -> @location(3) vec4<f32> { return vec4<f32>(0.0); }");
    let fragment = wgsl(
        "@fragment fn main(@location(3) @interpolate(flat) a: vec4<f32>) -> @location(0) vec4<f32> { return a; }",
    );
    match_interpolation(&mut vertex, &fragment);
    let entry = &vertex.entry_points[0];
    let Some(Binding::Location {
        interpolation,
        sampling,
        ..
    }) = entry.function.result.as_ref().unwrap().binding
    else {
        panic!("a location binding");
    };
    assert_eq!((interpolation, sampling), (Some(Interpolation::Flat), None));
}

#[test]
fn inputs_that_arrive_in_a_struct_are_read_and_a_stage_with_none_changes_nothing() {
    let mut vertex = wgsl(VERTEX);
    let fragment = wgsl(
        "struct In { @location(1) @interpolate(flat) b: vec4<f32> }
         @fragment fn main(input: In) -> @location(0) vec4<f32> { return input.b; }",
    );
    match_interpolation(&mut vertex, &fragment);
    assert_eq!(outputs(&vertex)[1].1, Some(Interpolation::Flat));

    let mut vertex = wgsl(VERTEX);
    let before = outputs(&vertex);
    let fragment = wgsl("@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }");
    match_interpolation(&mut vertex, &fragment);
    assert_eq!(outputs(&vertex), before);
}

#[test]
fn a_pixel_stage_that_reads_only_locations_the_vertex_stage_does_not_write_changes_nothing() {
    let mut vertex = wgsl(VERTEX);
    let before = outputs(&vertex);
    let types = vertex.types.len();
    let fragment = wgsl(
        "@fragment fn main(@location(9) @interpolate(flat) z: vec4<f32>) -> @location(0) vec4<f32> { return z; }",
    );
    match_interpolation(&mut vertex, &fragment);
    assert_eq!(outputs(&vertex), before);
    assert_eq!(vertex.types.len(), types, "no type was added");
}

#[test]
fn linking_leaves_the_vertex_stage_valid_and_a_second_link_changes_nothing() {
    let mut vertex = wgsl(VERTEX);
    let fragment = wgsl(
        "@fragment fn main(@location(0) @interpolate(flat) a: vec4<f32>) -> @location(0) vec4<f32> { return a; }",
    );
    match_interpolation(&mut vertex, &fragment);
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&vertex)
    .expect("the linked vertex module validates");
    // Linking a second time finds nothing to change, which `replace` would panic over
    let before = outputs(&vertex);
    match_interpolation(&mut vertex, &fragment);
    assert_eq!(outputs(&vertex), before);
}
