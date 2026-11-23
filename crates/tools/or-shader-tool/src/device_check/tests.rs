use super::*;
use crate::spirv_check::{check_with, Reached};
use crate::spirv_testing::{dwords, entry, fragment_with_a_demote, op};

/// The machine's device, or a recorded skip: a CI machine may have no adapter.
fn device() -> Option<DeviceCompiler> {
    match DeviceCompiler::open() {
        Ok(device) => Some(device),
        Err(error) => {
            or_test_support::skip_device("the device check", error);
            None
        }
    }
}

/// A fragment shader that writes a constant colour to location 0.
fn fragment_writing_a_colour() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 12, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.push(5);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    words.extend(op(71, &[5, 30, 0]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[6, 32]));
    words.extend(op(23, &[7, 6, 4]));
    words.extend(op(32, &[8, 3, 7]));
    words.extend(op(59, &[8, 5, 3]));
    words.extend(op(43, &[6, 9, 0x3f80_0000]));
    words.extend(op(44, &[7, 10, 9, 9, 9, 9]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[11]));
    words.extend(op(62, &[5, 10]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn a_fragment_shader_naga_wrote_compiles_on_the_device() {
    let Some(device) = device() else { return };
    let verdict = check_with(&fragment_writing_a_colour(), Some(&device));
    assert_eq!(verdict.reached, Reached::Compiled, "{:?}", verdict.cause);
    assert!(verdict.device);
}

#[test]
fn a_translated_module_reaches_the_device_through_the_legalizer() {
    let Some(device) = device() else { return };
    let verdict = check_with(&fragment_with_a_demote(true), Some(&device));
    assert_eq!(verdict.reached, Reached::Compiled, "{:?}", verdict.cause);
}

#[test]
fn a_module_the_device_refuses_stops_after_msl_with_the_devices_words() {
    let Some(device) = device() else { return };
    // A compute shader whose workgroup is larger than any device allows.
    let verdict = check_with(&compute_shader_with_a_huge_group(), Some(&device));
    assert_eq!(verdict.reached, Reached::MslWritten, "{:?}", verdict.cause);
    assert!(verdict.device);
    assert!(
        verdict.cause.as_deref().is_some_and(|c| !c.is_empty()),
        "{:?}",
        verdict.cause
    );
}

/// A compute shader that does nothing, with a workgroup of 2,048 invocations.
fn compute_shader_with_a_huge_group() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 5, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(5, 1));
    words.extend(op(16, &[1, 17, 2048, 1, 1]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[4]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn a_module_without_a_device_is_not_marked_as_offered_to_one() {
    let verdict = check_with(&fragment_writing_a_colour(), None);
    assert_eq!(verdict.reached, Reached::MslWritten);
    assert!(!verdict.device);
}

#[test]
fn a_scalar_or_vector_is_spelled_as_the_wgsl_a_matching_vertex_stage_is_written_in() {
    assert_eq!(
        wgsl_type(ScalarKind::Float, 3).as_deref(),
        Some("vec3<f32>")
    );
    assert_eq!(wgsl_type(ScalarKind::Uint, 1).as_deref(), Some("u32"));
    assert_eq!(wgsl_type(ScalarKind::Bool, 1), None);
}

/// A fragment shader with no output, as a depth-only pass or a discarding one has.
fn fragment_writing_nothing() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 5, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(4, 1));
    words.extend(op(16, &[1, 7]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[4]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn a_fragment_shader_that_writes_nothing_still_gets_a_pipeline() {
    let Some(device) = device() else { return };
    let verdict = check_with(&fragment_writing_nothing(), Some(&device));
    assert_eq!(verdict.reached, Reached::Compiled, "{:?}", verdict.cause);
}

/// A vertex shader that takes a vec3 at location 0 and a uint at location 1 and writes a position.
fn vertex_with_attributes() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 24, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![0, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.extend([5, 6, 7]);
    words.extend(op(15, &entry_words));
    words.extend(op(71, &[5, 11, 0]));
    words.extend(op(71, &[6, 30, 0]));
    words.extend(op(71, &[7, 30, 1]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[8, 32]));
    words.extend(op(23, &[9, 8, 4]));
    words.extend(op(23, &[10, 8, 3]));
    words.extend(op(21, &[11, 32, 0]));
    words.extend(op(32, &[12, 3, 9]));
    words.extend(op(32, &[13, 1, 10]));
    words.extend(op(32, &[14, 1, 11]));
    words.extend(op(59, &[12, 5, 3]));
    words.extend(op(59, &[13, 6, 1]));
    words.extend(op(59, &[14, 7, 1]));
    words.extend(op(43, &[8, 15, 0x3f80_0000]));
    words.extend(op(44, &[9, 16, 15, 15, 15, 15]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[17]));
    words.extend(op(61, &[10, 18, 6]));
    words.extend(op(61, &[11, 19, 7]));
    words.extend(op(62, &[5, 16]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

/// A fragment shader that reads a vec4 at location 2, interpolated without perspective, and writes it to location 0.
fn fragment_reading_an_input() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 14, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.extend([5, 6]);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    words.extend(op(71, &[5, 30, 2]));
    words.extend(op(71, &[5, 13]));
    words.extend(op(71, &[6, 30, 0]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[7, 32]));
    words.extend(op(23, &[8, 7, 4]));
    words.extend(op(32, &[9, 1, 8]));
    words.extend(op(32, &[10, 3, 8]));
    words.extend(op(59, &[9, 5, 1]));
    words.extend(op(59, &[10, 6, 3]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[11]));
    words.extend(op(61, &[8, 12, 5]));
    words.extend(op(62, &[6, 12]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

/// A compute shader that does nothing, with a one-thread group.
fn empty_compute_shader() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 5, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(5, 1));
    words.extend(op(16, &[1, 17, 1, 1, 1]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[4]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn a_vertex_shader_gets_a_buffer_layout_from_its_inputs() {
    let Some(device) = device() else { return };
    let verdict = check_with(&vertex_with_attributes(), Some(&device));
    assert_eq!(verdict.reached, Reached::Compiled, "{:?}", verdict.cause);
}

#[test]
fn a_fragment_shader_with_an_input_gets_a_vertex_stage_that_outputs_it_to_match() {
    let Some(device) = device() else { return };
    let verdict = check_with(&fragment_reading_an_input(), Some(&device));
    assert_eq!(verdict.reached, Reached::Compiled, "{:?}", verdict.cause);
}

#[test]
fn a_compute_shader_gets_a_compute_pipeline() {
    let Some(device) = device() else { return };
    let verdict = check_with(&empty_compute_shader(), Some(&device));
    assert_eq!(verdict.reached, Reached::Compiled, "{:?}", verdict.cause);
}

fn wgsl(source: &str) -> naga::Module {
    naga::front::wgsl::parse_str(source).expect("the WGSL parses")
}

const VERTEX_WRITING_UV: &str = "
struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn main(@location(0) position: vec4<f32>) -> Out {
    var out: Out;
    out.position = position;
    out.uv = position.xy;
    return out;
}";

const FRAGMENT_READING_UV: &str = "
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(uv, 0.0, 1.0);
}";

#[test]
fn a_vertex_and_a_fragment_stage_that_meet_build_as_one_pipeline() {
    let Some(device) = device() else { return };
    let result =
        device.compile_pipeline(&wgsl(VERTEX_WRITING_UV), Some(&wgsl(FRAGMENT_READING_UV)));
    assert_eq!(result, Ok(()));
}

#[test]
fn a_vertex_stage_alone_builds_as_a_pipeline_with_no_fragment_stage() {
    let Some(device) = device() else { return };
    assert_eq!(
        device.compile_pipeline(&wgsl(VERTEX_WRITING_UV), None),
        Ok(())
    );
}

#[test]
fn a_fragment_stage_that_reads_what_the_vertex_stage_does_not_write_is_refused_with_the_devices_words(
) {
    let Some(device) = device() else { return };
    let reads_location_3 = FRAGMENT_READING_UV.replace("location(0) uv", "location(3) uv");
    let Err(message) =
        device.compile_pipeline(&wgsl(VERTEX_WRITING_UV), Some(&wgsl(&reads_location_3)))
    else {
        panic!("a fragment input nothing writes was accepted");
    };
    assert!(!message.is_empty() && !message.contains('\n'), "{message}");
}

#[test]
fn a_fragment_stage_that_reads_a_wider_type_than_the_vertex_stage_writes_is_refused() {
    let Some(device) = device() else { return };
    let reads_vec3 = FRAGMENT_READING_UV
        .replace("vec2<f32>", "vec3<f32>")
        .replace("vec4<f32>(uv, 0.0, 1.0)", "vec4<f32>(uv, 1.0)");
    let result = device.compile_pipeline(&wgsl(VERTEX_WRITING_UV), Some(&wgsl(&reads_vec3)));
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn a_module_with_no_stage_of_the_kind_asked_for_is_refused_by_name() {
    let Some(device) = device() else { return };
    let result = device.compile_pipeline(&wgsl(FRAGMENT_READING_UV), None);
    assert_eq!(result, Err("no Vertex entry point".to_string()));
    let result = device.compile_pipeline(&wgsl(VERTEX_WRITING_UV), Some(&wgsl(VERTEX_WRITING_UV)));
    assert_eq!(result, Err("no Fragment entry point".to_string()));
}

#[cfg(feature = "native-front")]
#[test]
fn a_flat_pixel_input_meets_a_default_vertex_output_only_once_the_stages_are_linked() {
    let Some(device) = device() else { return };
    let mut vertex = wgsl(VERTEX_WRITING_UV);
    let fragment = wgsl(
        "@fragment fn main(@location(0) @interpolate(flat) uv: vec2<f32>) -> @location(0) vec4<f32> {
            return vec4<f32>(uv, 0.0, 1.0);
        }",
    );
    let Err(message) = device.compile_pipeline(&vertex, Some(&fragment)) else {
        panic!("the stages disagree on how location 0 is interpolated");
    };
    assert!(message.contains("interpolation"), "{message}");
    or_renderer::game_shaders::link::match_interpolation(&mut vertex, &fragment);
    assert_eq!(device.compile_pipeline(&vertex, Some(&fragment)), Ok(()));
}
