//! The program GTA VI's water is drawn with over the lit frame.

pub(super) const WATER_SHADER: &str = r#"
// Every field of the viewport's `CameraUniform`, in order.
struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera_pos_ws: vec4<f32>,
    camera_misc: vec4<f32>,
    key_light: vec4<f32>,
};
// The world's light: only its layout matters here.
struct Light {
    sun_travel: vec4<f32>,
    sun_colour: vec4<f32>,
    ambient_down: vec4<f32>,
    ambient_up: vec4<f32>,
    frame: vec4<f32>,
};
// `WaterUniform`: the medium a metre at a time, its colour and the light it takes, the frame's size and the day's fog.
struct Water {
    extinction: vec4<f32>,
    // In `w` what the reflection is multiplied by.
    albedo: vec4<f32>,
    // In `w` the depth over which a shore fades in, in metres.
    light: vec4<f32>,
    // One over the frame's width and height; in `z` whether there are ripples, in `w` their scale a metre.
    screen: vec4<f32>,
    fog: array<vec4<f32>, 5>,
    fog_colour: array<vec4<f32>, 4>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> water: Water;
@group(1) @binding(1) var behind: texture_2d<f32>;
// The frame's opaque depth at half its size, as distances along the view.
@group(1) @binding(2) var behind_depth: texture_2d<f32>;
@group(1) @binding(3) var sky: texture_2d<f32>;
// For the cosine of the angle a ray meets the surface at, how much it reflects, in red.
@group(1) @binding(4) var fresnel_row: texture_2d<f32>;
@group(1) @binding(5) var ripples: texture_2d<f32>;
@group(1) @binding(6) var clamped: sampler;
@group(1) @binding(7) var repeated: sampler;
@group(2) @binding(0) var<uniform> light: Light;

// A depth of the frame at or past this is the sky, or no depth at all.
const SKY: f32 = 50000.0;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) ground_depth: f32,
    @location(2) view_depth: f32,
};

// The vertex's colour holds in red how deep the ground lies under it.
@vertex
fn vs_water(@location(0) position: vec3<f32>, @location(3) colour: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.clip = camera.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    out.ground_depth = colour.r;
    out.view_depth = out.clip.w;
    return out;
}

// The texel of the map of two paraboloids that looks along `d`: the upper half of the sky on the right, the lower on the left.
fn paraboloid(d: vec3<f32>) -> vec2<f32> {
    let s = d.xy / (1.0 + abs(d.z));
    if (d.z >= 0.0) {
        return vec2<f32>(0.75 + 0.25 * s.x, 0.5 - 0.5 * s.y);
    }
    return vec2<f32>(0.25 - 0.25 * s.x, 0.5 - 0.5 * s.y);
}

// The day's fog along `ray`, from the eye to a point: its colour, and in `a` how much of the point it hides.
fn fog_of(ray: vec3<f32>) -> vec4<f32> {
    let p = water.fog;
    let full = max(length(ray), 1e-3);
    let dist = max(0.0, full - p[0].x);
    let dz = ray.z * (dist / full);
    let t = p[2].z * dz;
    var integral = 1.0;
    if (abs(dz) > 0.01) {
        integral = (1.0 - exp(-t)) / t;
    }
    let ground = (1.0 - saturate(exp(min(1.0, p[1].w * dist * integral)))) * p[2].y;
    let towards = ray / full;
    let moon = pow(saturate(dot(towards, p[4].xyz)), p[4].w);
    let sun = pow(saturate(dot(towards, p[3].xyz)), p[3].w);
    let haze_blend = p[1].y * (1.0 - ground);
    let haze = haze_blend * (1.0 - exp(p[1].x * max(0.0, dist - p[2].x)));
    let atmosphere = 1.0 - exp(-p[1].z * dist);
    let c = water.fog_colour;
    let moonlit = mix(c[1].rgb, c[3].rgb, moon);
    let air = mix(moonlit, c[0].rgb, sun);
    let near = mix(c[2].rgb, air, atmosphere);
    let colour = mix(near, vec3<f32>(c[0].a, c[1].a, c[2].a), haze_blend);
    return vec4<f32>(colour, saturate(haze + ground));
}

@fragment
fn fs_water(in: VsOut) -> @location(0) vec4<f32> {
    let to_eye = camera.camera_pos_ws.xyz - in.world;
    let dist = max(length(to_eye), 1e-3);
    let v = to_eye / dist;
    var n = vec3<f32>(0.0, 0.0, 1.0);
    if (water.screen.z > 0.5) {
        let at = in.world.xy * water.screen.w;
        let a = textureSample(ripples, repeated, at).xy * 2.0 - 1.0;
        let b = textureSample(ripples, repeated, at * 0.37 + vec2<f32>(0.21, 0.67)).xy * 2.0 - 1.0;
        n = normalize(vec3<f32>((a + b) * 0.5, 1.0));
    }
    // How far the ray runs in the water: to what is opaque behind it, or where the frame drew nothing there, down to the ground the height map gives.
    // ⚠ Read between the texels of the half-size depth: the nearest one is a pixel off every other row, which over flat ground at a glance is metres.
    let scene = textureSampleLevel(behind_depth, clamped, in.clip.xy * water.screen.xy, 0.0).r;
    let steep = max(v.z, 0.02);
    var path = max(in.ground_depth, 0.0) / steep;
    if (scene > 0.0 && scene < SKY) {
        // ⚠ The depth is held in half floats: a difference under one step of them is no water, or flat ground just under the surface shows in bands.
        let step = scene / 1024.0;
        path = max(scene - in.view_depth - step, 0.0) * dist / max(in.view_depth, 1e-3);
    }
    let under = textureSampleLevel(behind, clamped, in.clip.xy * water.screen.xy, 0.0).rgb;
    let through = exp(-water.extinction.rgb * path);
    let cosine = saturate(dot(n, v));
    let fresnel = textureSampleLevel(fresnel_row, clamped, vec2<f32>(cosine, 0.5), 0.0).r;
    let r = reflect(-v, n);
    let reflected = textureSampleLevel(sky, clamped, paraboloid(r), 0.0).rgb * water.albedo.w;
    // What shows of what lies behind, and the light the water gives of its own: what it scatters and what it reflects.
    let passed = (1.0 - fresnel) * through;
    let own = (1.0 - fresnel) * (1.0 - through) * water.albedo.rgb * water.light.rgb + fresnel * reflected;
    // ⚠ What lies behind was fogged as the frame was lit: the fog covers only the water's own share.
    let fog = fog_of(-to_eye);
    let colour = under * passed + mix(own, fog.rgb * (1.0 - passed), fog.a);
    // A shore fades in over its first depth, so that the water meets the ground in no line.
    let shore = smoothstep(0.0, water.light.w, path * steep);
    return vec4<f32>(mix(under, colour, shore), 0.0);
}
"#;
