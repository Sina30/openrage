// Procedural water-stencil mask — single hand-rolled WGSL pass
// (no engine `.fxc` analogue; allowlisted by `tests/bake_first_lint.rs`).
//
// Renders a fullscreen triangle, back-projects each NDC fragment
// through the camera's inverse view-proj into a world-space ray
// direction, then ray-vs-plane checks the procedural water plane at
// `z = 0`. Output is `R8Unorm`: `1.0` where the ray hits the water
// plane in front of the camera, `0.0` otherwise.
//
// Engine reference: this pass is editor-side synthesis. The real
// engine builds a proper water stencil from the water render phase
// (`CRenderPhaseWater`); when that lands the pass goes away.

struct WaterStencilParams {
    camera_pos: vec3<f32>,
    _pad: f32,
    inv_view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> params: WaterStencilParams;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_dir: vec3<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    // Fullscreen triangle: emits NDC corners at (-1,-1), (3,-1), (-1,3).
    let x = f32((vid & 2u)) * 2.0 - 1.0;
    let y = f32((vid & 1u) << 1u) * 2.0 - 1.0;

    // Back-project an NDC point at the far plane to recover a
    // world-space ray direction from the camera.
    let ndc_far = vec4<f32>(x, y, 1.0, 1.0);
    let world_h = params.inv_view_proj * ndc_far;
    let world_pos = world_h.xyz / world_h.w;

    var out: VsOut;
    out.clip_pos = vec4<f32>(x, y, 0.0, 1.0);
    out.world_dir = world_pos - params.camera_pos;
    return out;
}

@fragment
fn fs_water_stencil(in: VsOut) -> @location(0) f32 {
    let dir = in.world_dir;
    let cam_z = params.camera_pos.z;
    let dir_z = dir.z;

    // Camera below water plane: every fragment is "below water" so
    // everything reads as water (1.0).
    if (cam_z < 0.0) {
        return 1.0;
    }

    // Ray parallel to the plane never intersects the water in front.
    if (abs(dir_z) < 1e-6) {
        return 0.0;
    }

    // Ray-vs-plane: t = -cam_z / dir_z. Hit "in front" means t > 0,
    // which only happens when the ray is descending (dir_z < 0).
    let t = -cam_z / dir_z;
    return select(0.0, 1.0, t > 0.0);
}
