//! The program the world is drawn with: one vertex stage for a view and one for a cascade of the sun's shadow, and a fragment stage for each bucket in each kind of pass.

pub(super) const WORLD_SHADER: &str = r#"
// Every field of the viewport's `CameraUniform`, in order: a uniform struct is addressed by byte offset.
struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera_pos_ws: vec4<f32>,
    camera_misc: vec4<f32>,
    key_light: vec4<f32>,
};
// The light of the game's frame, in the units its lighting passes sum: what blends over that frame is lit by it.
struct Light {
    sun_travel: vec4<f32>,
    sun_colour: vec4<f32>,
    ambient_down: vec4<f32>,
    ambient_up: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var colour_tex: texture_2d<f32>;
@group(1) @binding(1) var normal_tex: texture_2d<f32>;
@group(1) @binding(2) var specular_tex: texture_2d<f32>;
@group(1) @binding(3) var map_smp: sampler;
@group(2) @binding(0) var<uniform> light: Light;

struct VsIn {
    @location(0) position:   vec3<f32>,
    @location(1) normal:     vec3<f32>,
    @location(2) uv:         vec2<f32>,
    @location(3) colour:     vec4<f32>,
    @location(4) depth_bias: f32,
    @location(5) row0:       vec4<f32>,
    @location(6) row1:       vec4<f32>,
    @location(7) row2:       vec4<f32>,
    @location(8) tint:       vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) world_pos: vec3<f32>,
    // How much of the instance its fade leaves, which is the alpha of its tint and no alpha of the surface.
    @location(4) @interpolate(flat) fade: f32,
};

// The pull towards the camera a vertex's depth bias asks for, as the viewport's mesh shader makes it.
const BIAS_MAX_PULL: f32 = 0.05;
const BIAS_DIST_SCALE: f32 = 0.002;

fn placed(in: VsIn) -> vec3<f32> {
    let local = vec4<f32>(in.position, 1.0);
    return vec3<f32>(dot(in.row0, local), dot(in.row1, local), dot(in.row2, local));
}

// What both vertex stages hand on, but for where the vertex is drawn.
fn handed_on(in: VsIn, world: vec3<f32>) -> VsOut {
    var out: VsOut;
    // The rows turn the normal as they turn the surface, which is right for a uniform scale; a non-uniform one shears it slightly.
    out.world_normal = normalize(vec3<f32>(
        dot(in.row0.xyz, in.normal),
        dot(in.row1.xyz, in.normal),
        dot(in.row2.xyz, in.normal),
    ));
    out.uv = in.uv;
    out.colour = in.colour * vec4<f32>(in.tint.rgb, 1.0);
    out.world_pos = world;
    out.fade = in.tint.a;
    return out;
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let world = placed(in);
    var out = handed_on(in, world);
    let to_camera = camera.camera_pos_ws.xyz - world;
    let dist_to_cam = length(to_camera);
    let pull_metres = in.depth_bias * min(BIAS_MAX_PULL, dist_to_cam * BIAS_DIST_SCALE);
    let biased = world + to_camera / max(dist_to_cam, 1e-6) * pull_metres;
    out.clip_pos = camera.view_proj * vec4<f32>(biased, 1.0);
    return out;
}

// Into a tile of the map the sun's shadow is cast into: the camera's uniform holds the view of the cascade.
@vertex
fn vs_cast(in: VsIn) -> VsOut {
    let world = placed(in);
    var out = handed_on(in, world);
    out.clip_pos = camera.view_proj * vec4<f32>(world, 1.0);
    // What stands between the sun and the near side of the tile still casts: it is laid on that side instead of clipped by it.
    out.clip_pos.z = max(out.clip_pos.z, 0.0);
    return out;
}

// The threshold of a pixel in a 4 by 4 ordered dither: sixteen steps from 1/17 to 16/17, so a fade of one keeps every pixel and a fade of nought none.
fn dither(pixel: vec2<f32>) -> f32 {
    var bayer = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    let at = (u32(pixel.y) & 3u) * 4u + (u32(pixel.x) & 3u);
    return (bayer[at] + 1.0) / 17.0;
}

// Whether nothing of the pixel is drawn: its coverage is under `cut`, or the fade of its instance leaves it out.
fn dropped(in: VsOut, alpha: f32, cut: f32) -> bool {
    return alpha < cut || in.fade < dither(in.clip_pos.xy);
}

// Below this a texel of a cutout is not drawn.
const CUTOUT_ALPHA: f32 = 0.5;
// Below this a texel of what blends adds nothing.
const BLEND_ALPHA: f32 = 1.0 / 255.0;
// The exponent a specular map's alpha of one stands for, the most a G-buffer holds, and the fresnel word of every surface.
const SPECULAR_FALLOFF: f32 = 100.0;
const SPECULAR_EXPONENT_MAX: f32 = 512.0;
const FRESNEL: f32 = 0.98;
// The light an emissive surface gives of its own colour, and the most the G-buffer's word for it holds.
const EMISSIVE: f32 = 1.0;
const EMISSIVE_MAX: f32 = 16.0;

// What the maps and the vertex give a pixel: its linear colour with its coverage, its unit normal in the world, and how strongly and how tightly it shines.
struct Surface {
    colour: vec4<f32>,
    normal: vec3<f32>,
    intensity: f32,
    exponent: f32,
};

// ⚠ Every map is sampled and every derivative taken before anything is discarded.
fn surface_of(in: VsOut, front_facing: bool) -> Surface {
    let colour = textureSample(colour_tex, map_smp, in.uv) * in.colour;
    let bump = textureSample(normal_tex, map_smp, in.uv).xy * 2.0 - 1.0;
    let gloss = textureSample(specular_tex, map_smp, in.uv);
    // Culling is off, so a face seen from behind is shaded by the normal turned towards the viewer.
    let n = normalize(in.world_normal) * select(-1.0, 1.0, front_facing);
    // No vertex carries a tangent: the frame is how the position and the texture coordinate change across the pixel, the map's x along u and its y along v.
    let dp1 = dpdx(in.world_pos);
    let dp2 = dpdy(in.world_pos);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    // ⚠ Which way round the two changes of position turn about the normal: a frame whose rows run down the screen turns them the other way, and without this both axes come out reversed.
    let handed = sign(dot(dp1, dp2perp));
    let tangent = (dp2perp * duv1.x + dp1perp * duv2.x) * handed;
    let bitangent = (dp2perp * duv1.y + dp1perp * duv2.y) * handed;
    let longest = max(dot(tangent, tangent), dot(bitangent, bitangent));
    let scale = inverseSqrt(max(longest, 1e-30));
    // The map's z is worked out from its x and y, so a map of two channels reads as one of three.
    let z = max(sqrt(max(1.0 - dot(bump, bump), 0.0)), 0.01);
    var s: Surface;
    s.colour = colour;
    s.normal = normalize(tangent * (bump.x * scale) + bitangent * (bump.y * scale) + n * z);
    // The specular map as the game's default effects read it: intensity in red, falloff in alpha.
    s.intensity = clamp(gloss.r, 0.0, 1.0);
    s.exponent = clamp(gloss.a * SPECULAR_FALLOFF, 0.0, SPECULAR_EXPONENT_MAX);
    return s;
}

// The highlight a light towards `towards` leaves on the surface seen from `view_dir`.
fn shine(s: Surface, towards: vec3<f32>, view_dir: vec3<f32>) -> f32 {
    // A light straight behind the viewer's line has no half way: the sum is then of no length, and is left so
    let sum = towards + view_dir;
    let half_way = sum * inverseSqrt(max(dot(sum, sum), 1e-12));
    let lit = step(0.0, dot(s.normal, towards));
    return s.intensity * lit * pow(max(dot(s.normal, half_way), 0.0), max(s.exponent, 1.0));
}

// The key light, the head light and the ambient light of the viewport's material mode.
fn plain_lit(s: Surface, world_pos: vec3<f32>) -> vec3<f32> {
    let view_dir = normalize(camera.camera_pos_ws.xyz - world_pos);
    let key = max(dot(s.normal, camera.key_light.xyz), 0.0) * camera.key_light.w;
    let head = max(dot(s.normal, view_dir), 0.0) * 0.30;
    let highlight = shine(s, camera.key_light.xyz, view_dir) * camera.key_light.w;
    return s.colour.rgb * (key + head + 0.15) + vec3<f32>(highlight);
}

// A surface of the plain modes that writes depth: `cut` is the coverage under which it is not drawn, and `glow` how far it shows its own colour unlit.
fn forward(in: VsOut, front_facing: bool, cut: f32, glow: f32) -> vec4<f32> {
    let s = surface_of(in, front_facing);
    if (dropped(in, s.colour.a, cut)) {
        discard;
    }
    return vec4<f32>(mix(plain_lit(s, in.world_pos), s.colour.rgb, glow), 1.0);
}

@fragment
fn fs_forward(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return forward(in, front_facing, 0.0, 0.0);
}

@fragment
fn fs_forward_cutout(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return forward(in, front_facing, CUTOUT_ALPHA, 0.0);
}

@fragment
fn fs_forward_emissive(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return forward(in, front_facing, 0.0, 1.0);
}

// What blends in the plain modes, a decal or a glass: its coverage is the alpha it is blended by.
@fragment
fn fs_forward_blend(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let s = surface_of(in, front_facing);
    if (dropped(in, s.colour.a, BLEND_ALPHA)) {
        discard;
    }
    return vec4<f32>(plain_lit(s, in.world_pos), s.colour.a);
}

// The four targets of the game's G-buffer, in the words its lighting passes read.
struct GBufferOut {
    @location(0) albedo: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) specular: vec4<f32>,
    @location(3) ambient: vec4<f32>,
};

fn words(s: Surface, emissive: f32) -> GBufferOut {
    var out: GBufferOut;
    // The lighting pass squares the colour it reads here, so the target holds the square root of the linear colour.
    out.albedo = vec4<f32>(sqrt(max(s.colour.rgb, vec3<f32>(0.0))), 1.0);
    // The normal in the world, from -1..1 into 0..1; the default effect leaves the alpha at nought for every normal but one straight up.
    out.normal = vec4<f32>(s.normal * 0.5 + 0.5, 0.0);
    // The square root of the specular intensity in red and of the exponent over the most there is in green, the fresnel word in blue, and in alpha a surface the sun reaches whole.
    out.specular = vec4<f32>(
        sqrt(s.intensity),
        sqrt(s.exponent / SPECULAR_EXPONENT_MAX),
        FRESNEL,
        1.0,
    );
    // The whole of the natural ambient light in red, none of the artificial in green, and in blue the light the surface gives of its own colour, which the lighting pass adds.
    out.ambient = vec4<f32>(1.0, 0.0, emissive / EMISSIVE_MAX, 1.0);
    return out;
}

fn gbuffer(in: VsOut, front_facing: bool, cut: f32, emissive: f32) -> GBufferOut {
    let s = surface_of(in, front_facing);
    if (dropped(in, s.colour.a, cut)) {
        discard;
    }
    return words(s, emissive);
}

@fragment
fn fs_gbuffer(in: VsOut, @builtin(front_facing) front_facing: bool) -> GBufferOut {
    return gbuffer(in, front_facing, 0.0, 0.0);
}

@fragment
fn fs_gbuffer_cutout(in: VsOut, @builtin(front_facing) front_facing: bool) -> GBufferOut {
    return gbuffer(in, front_facing, CUTOUT_ALPHA, 0.0);
}

@fragment
fn fs_gbuffer_emissive(in: VsOut, @builtin(front_facing) front_facing: bool) -> GBufferOut {
    return gbuffer(in, front_facing, 0.0, EMISSIVE);
}

// A decal is blended onto what the G-buffer holds: each target it writes takes its coverage as the alpha it is blended by, and the pipeline keeps the alpha that was there.
@fragment
fn fs_gbuffer_decal(in: VsOut, @builtin(front_facing) front_facing: bool) -> GBufferOut {
    let s = surface_of(in, front_facing);
    if (dropped(in, s.colour.a, BLEND_ALPHA)) {
        discard;
    }
    var out = words(s, 0.0);
    out.albedo.a = s.colour.a;
    out.normal.a = s.colour.a;
    out.specular.a = s.colour.a;
    return out;
}

// What blends over the frame the game's passes lit, lit by the same sun and ambient light.
@fragment
fn fs_lit(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let s = surface_of(in, front_facing);
    if (dropped(in, s.colour.a, BLEND_ALPHA)) {
        discard;
    }
    let towards = -light.sun_travel.xyz;
    let sun = light.sun_colour.rgb * max(dot(s.normal, towards), 0.0);
    let ambient = mix(light.ambient_down.rgb, light.ambient_up.rgb, s.normal.z * 0.5 + 0.5);
    let view_dir = normalize(camera.camera_pos_ws.xyz - in.world_pos);
    let highlight = light.sun_colour.rgb * shine(s, towards, view_dir);
    return vec4<f32>(s.colour.rgb * (sun + ambient) + highlight, s.colour.a);
}

// What casts the sun's shadow whole: only the fade of its instance leaves a texel out.
@fragment
fn fs_cast(in: VsOut) {
    if (dropped(in, 1.0, 0.0)) {
        discard;
    }
}

// A cutout casts the shadow of what it draws.
@fragment
fn fs_cast_cutout(in: VsOut) {
    let alpha = textureSample(colour_tex, map_smp, in.uv).a * in.colour.a;
    if (dropped(in, alpha, CUTOUT_ALPHA)) {
        discard;
    }
}
"#;
