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
    // One over the frame's width and height.
    screen: vec4<f32>,
    // The medium's grid: its west and north edges in metres and one over its extent; nought in `z` where there is none.
    grid: vec4<f32>,
    fog: array<vec4<f32>, 5>,
    fog_colour: array<vec4<f32>, 4>,
    // Each wave's number a metre along x and y, its height and its phase.
    waves: array<vec4<f32>, 12>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> water: Water;
@group(1) @binding(1) var behind: texture_2d<f32>;
// The frame's opaque depth at half its size, as distances along the view.
@group(1) @binding(2) var behind_depth: texture_2d<f32>;
@group(1) @binding(3) var sky: texture_2d<f32>;
// For the cosine of the angle a ray meets the surface at, how much it reflects, in red.
@group(1) @binding(4) var fresnel_row: texture_2d<f32>;
@group(1) @binding(6) var clamped: sampler;
// The medium by place: how much of each channel a metre takes out, and how much it scatters.
@group(1) @binding(8) var medium_extinction: texture_2d<f32>;
@group(1) @binding(9) var medium_scattering: texture_2d<f32>;
@group(2) @binding(0) var<uniform> light: Light;

// A depth of the frame at or past this is the sky, or no depth at all.
const SKY: f32 = 50000.0;
// The sun's glint, as GTA V's water has it (assumed: GTA VI's water program was not read): its strength and the power of its falloff.
const GLINT: f32 = 1.0;
const GLINT_FALLOFF: f32 = 1118.0;
// How many steps a reflected ray takes across the frame, how many halve the last, and how each step grows.
const TRACE_STEPS: i32 = 32;
const REFINE_STEPS: i32 = 5;
// How many pixels a wave must span before it is drawn in full; shorter ones fade out rather than shimmer.
const WAVE_PIXELS: f32 = 6.0;
const TRACE_GROWTH: f32 = 1.35;

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

// Where a point lies on the frame, and how far along the view; `w` at or below nought behind the eye.
fn on_frame(p: vec3<f32>) -> vec3<f32> {
    let clip = camera.view_proj * vec4<f32>(p, 1.0);
    let ndc = clip.xy / max(clip.w, 1e-4);
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, clip.w);
}

// Whether a point on the frame lies behind what the frame drew there, and by how far.
fn behind_drawn(at: vec3<f32>) -> f32 {
    let scene = textureSampleLevel(behind_depth, clamped, at.xy, 0.0).r;
    if (scene <= 0.0 || scene >= SKY) {
        return -1.0;
    }
    return at.z - scene;
}

// The lit frame where a ray from `start` along `r` first passes behind what the frame drew, and in `a` how far it is to be trusted: nought where it leaves the frame or meets nothing.
fn traced(start: vec3<f32>, r: vec3<f32>) -> vec4<f32> {
    var before = 0.0;
    var t = 0.5;
    for (var i = 0; i < TRACE_STEPS; i++) {
        let at = on_frame(start + r * t);
        if (at.z <= 0.0 || any(at.xy < vec2<f32>(0.0)) || any(at.xy > vec2<f32>(1.0))) {
            return vec4<f32>(0.0);
        }
        let past = behind_drawn(at);
        if (past > 0.0) {
            // A ray that has gone far behind a surface went past its edge, not into it.
            if (past > 2.0 * (t - before) + 1.0) {
                return vec4<f32>(0.0);
            }
            var lo = before;
            var hi = t;
            for (var k = 0; k < REFINE_STEPS; k++) {
                let mid = 0.5 * (lo + hi);
                if (behind_drawn(on_frame(start + r * mid)) > 0.0) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let hit = on_frame(start + r * hi);
            let edge = min(min(hit.x, 1.0 - hit.x), min(hit.y, 1.0 - hit.y));
            let trust = smoothstep(0.0, 0.08, edge) * smoothstep(0.0, 0.15, r.z + 0.15);
            return vec4<f32>(textureSampleLevel(behind, clamped, hit.xy, 0.0).rgb, trust);
        }
        before = t;
        t = t * TRACE_GROWTH + 0.5;
    }
    return vec4<f32>(0.0);
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
    // The surface's slope, summed over its waves: each a sine along its number, faded where the frame cannot show it.
    let footprint = max(length(fwidth(in.world.xy)), 1e-4);
    var slope = vec2<f32>(0.0);
    for (var i = 0; i < 12; i++) {
        let w = water.waves[i];
        let number = length(w.xy);
        if (number <= 0.0) {
            continue;
        }
        let seen = smoothstep(1.0, 2.0, 6.2831853 / (number * footprint * WAVE_PIXELS * 0.5));
        slope += w.xy * (w.z * cos(dot(w.xy, in.world.xy) + w.w) * seen);
    }
    let n = normalize(vec3<f32>(-slope, 1.0));
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
    var extinction = water.extinction.rgb;
    var albedo = water.albedo.rgb;
    if (water.grid.z > 0.0) {
        let at = vec2<f32>((in.world.x - water.grid.x) * water.grid.z, (water.grid.y - in.world.y) * water.grid.w);
        extinction = max(textureSampleLevel(medium_extinction, clamped, at, 0.0).rgb, vec3<f32>(1e-6));
        let scattering = textureSampleLevel(medium_scattering, clamped, at, 0.0).rgb;
        albedo = min(scattering / extinction, vec3<f32>(1.0));
    }
    let through = exp(-extinction * path);
    let cosine = saturate(dot(n, v));
    let fresnel = textureSampleLevel(fresnel_row, clamped, vec2<f32>(cosine, 0.5), 0.0).r;
    var r = reflect(-v, n);
    // A ray a wave turns down into the water still leaves upward.
    r.z = abs(r.z);
    // What the water mirrors: the lit frame where the reflected ray meets something it drew, else the sky.
    let sky_seen = textureSampleLevel(sky, clamped, paraboloid(r), 0.0).rgb;
    let world_seen = traced(in.world, r);
    let reflected = mix(sky_seen, world_seen.rgb, world_seen.a) * water.albedo.w;
    let towards_sun = -light.sun_travel.xyz;
    let glint = light.sun_colour.rgb * GLINT * pow(saturate(dot(n, normalize(towards_sun + v))), GLINT_FALLOFF);
    // What shows of what lies behind, and the light the water gives of its own: what it scatters and what it reflects.
    let passed = (1.0 - fresnel) * through;
    let own = (1.0 - fresnel) * (1.0 - through) * albedo * water.light.rgb + fresnel * reflected + glint;
    // ⚠ What lies behind was fogged as the frame was lit: the fog covers only the water's own share.
    let fog = fog_of(-to_eye);
    let colour = under * passed + mix(own, fog.rgb * (1.0 - passed), fog.a);
    // A shore fades in over its first depth, so that the water meets the ground in no line.
    let shore = smoothstep(0.0, water.light.w, path * steep);
    return vec4<f32>(mix(under, colour, shore), 0.0);
}
"#;
