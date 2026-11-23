use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::Technique;

use super::super::synthetic::{material, pack_on_disk, quad};
use super::super::test_pack::{element, gpu};
use super::*;

/// A camera above the origin looking straight down, so a quad at height `z` is `5 - z` away.
fn overhead() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 0.0, 5.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        fov_y: 60f32.to_radians(),
        near: 0.1,
        far: 50.0,
    }
}

const SIZE: u32 = 32;

fn render(
    dir: &tempfile::TempDir,
    surfaces: &[(Surface, Material)],
    test: &str,
) -> Option<Result<MeshRender, MeshError>> {
    let (device, queue) = gpu(test)?;
    let pack = GamePack::open(dir.path()).unwrap();
    Some(render_surfaces(
        &device,
        &queue,
        &pack,
        surfaces,
        &overhead(),
        SIZE,
    ))
}

fn middle(rendered: &Rendered) -> [u8; 4] {
    let at = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    [
        rendered.bytes[at],
        rendered.bytes[at + 1],
        rendered.bytes[at + 2],
        rendered.bytes[at + 3],
    ]
}

fn near(got: [u8; 4], want: [u8; 4]) -> bool {
    got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 2)
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_surface_is_drawn_with_the_constant_and_the_texture_its_material_names() {
    let dir = pack_on_disk();
    let surfaces = [(
        quad(0.0),
        material([0.5, 1.0, 1.0, 1.0], Some([255, 128, 0, 255])),
    )];
    let Some(rendered) = render(&dir, &surfaces, "a_surface_is_drawn_with_the_constant") else {
        return;
    };
    let render = rendered.expect("the surface renders");
    assert_eq!((render.drawn, render.skipped.len()), (1, 0));
    let rendered = render.rendered;
    // Both triangles of the quad are drawn: the corners off its diagonal, each covered by only one
    for (x, y) in [(SIZE / 2 - 4, SIZE / 2 + 4), (SIZE / 2 + 4, SIZE / 2 - 4)] {
        let at = ((y * SIZE + x) * 4 + 3) as usize;
        assert_eq!(rendered.bytes[at], 255, "pixel ({x}, {y}) is covered");
    }
    // The orange texel scaled by the tint, lit by a normal facing the camera
    let got = middle(&rendered);
    assert!(near(got, [128, 128, 0, 255]), "{got:?}");
}

fn hashed(name: &str) -> String {
    format!("0x{:08X}", or_io::hash::string_hash_str(name))
}

#[test]
fn a_parameter_is_a_name_in_any_case_or_the_hash_of_it() {
    assert!(is_named("matTint", "matTint"));
    assert!(is_named("MATTINT", "matTint"));
    assert!(is_named(&hashed("matTint"), "matTint"));
    assert!(is_named(&hashed("MatTint").to_lowercase(), "matTint"));
    assert!(!is_named(&hashed("matTint"), "otherTint"));
    assert!(!is_named("0x", "matTint"));
    assert!(!is_named("0xZZZZZZZZ", "matTint"));
    assert!(!is_named("tint", "matTint"));
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_constant_and_a_texture_a_material_names_by_hash_are_set_and_bound() {
    let dir = pack_on_disk();
    let mut by_hash = material([0.5, 1.0, 1.0, 1.0], Some([255, 128, 0, 255]));
    by_hash.vectors[0].0 = hashed("matTint");
    let (name, image) = by_hash.textures.pop_first().expect("one texture");
    by_hash.textures.insert(hashed(&name), image);
    let surfaces = [(quad(0.0), by_hash)];
    let Some(rendered) = render(&dir, &surfaces, "a_constant_and_a_texture_by_hash") else {
        return;
    };
    let got = middle(&rendered.expect("the surface renders").rendered);
    assert!(near(got, [128, 128, 0, 255]), "{got:?}");
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_sun_a_surface_is_drawn_with_is_the_one_the_camera_lights_it_by() {
    let dir = pack_on_disk();
    let mut sunlit = material([1.0; 4], None);
    sunlit.effect = "sunlit".into();
    let Some(rendered) = render(
        &dir,
        &[(quad(0.0), sunlit)],
        "the_sun_a_surface_is_drawn_with",
    ) else {
        return;
    };
    let got = middle(&rendered.unwrap().rendered);
    // The way its light travels, away from the sun
    let want = -overhead().light_direction();
    for (channel, component) in got.iter().zip(want.to_array()) {
        let drawn = f32::from(*channel) / 255.0 * 2.0 - 1.0;
        assert!((drawn - component).abs() < 0.02, "{got:?} {want:?}");
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_texture_that_states_the_srgb_colour_space_is_lit_as_linear() {
    let dir = pack_on_disk();
    let mut stated = material([1.0; 4], Some([188, 188, 188, 255]));
    stated
        .textures
        .values_mut()
        .for_each(|image| image.srgb = true);
    let plain = material([1.0; 4], Some([188, 188, 188, 255]));
    let mut centre = Vec::new();
    for material in [plain, stated] {
        let Some(rendered) = render(&dir, &[(quad(0.0), material)], "a_texture_that_states_srgb")
        else {
            return;
        };
        centre.push(middle(&rendered.unwrap().rendered)[0]);
    }
    assert!(centre[0].abs_diff(188) <= 2, "{centre:?}");
    assert!(centre[1].abs_diff(128) <= 2, "{centre:?}");
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_texture_the_material_does_not_bind_is_grey() {
    let dir = pack_on_disk();
    let surfaces = [(quad(0.0), material([1.0, 1.0, 1.0, 1.0], None))];
    let Some(rendered) = render(&dir, &surfaces, "a_texture_the_material_does_not_bind") else {
        return;
    };
    let got = middle(&rendered.unwrap().rendered);
    assert!(near(got, [200, 200, 200, 255]), "{got:?}");
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn the_nearer_of_two_surfaces_shows_whichever_comes_first() {
    let dir = pack_on_disk();
    let red = material([1.0, 0.0, 0.0, 1.0], Some([255, 255, 255, 255]));
    let green = material([0.0, 1.0, 0.0, 1.0], Some([255, 255, 255, 255]));
    // Height 0 is nearer the camera at height 5 than height minus one is
    for surfaces in [
        [(quad(0.0), red.clone()), (quad(-1.0), green.clone())],
        [(quad(-1.0), green), (quad(0.0), red)],
    ] {
        let Some(rendered) = render(&dir, &surfaces, "the_nearer_of_two_surfaces") else {
            return;
        };
        let got = middle(&rendered.unwrap().rendered);
        assert!(near(got, [255, 0, 0, 255]), "{got:?}");
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_surface_the_pack_cannot_draw_is_left_out_and_listed_and_the_rest_still_draw() {
    let dir = pack_on_disk();
    let mut missing = material([1.0; 4], None);
    missing.effect = "nothing".into();
    // The synthetic effect has no skinned technique
    let skinned = Surface {
        skinned: true,
        ..quad(0.0)
    };
    let surfaces = [
        (quad(0.0), missing),
        (skinned, material([1.0; 4], None)),
        (
            quad(0.0),
            material([1.0, 0.0, 0.0, 1.0], Some([255, 255, 255, 255])),
        ),
    ];
    let Some(result) = render(&dir, &surfaces, "a_surface_the_pack_cannot_draw") else {
        return;
    };
    let render = result.expect("the render itself works");
    assert_eq!(render.drawn, 1);
    assert_eq!(render.skipped.len(), 2);
    assert!(
        matches!(&render.skipped[0], (effect, MeshError::NoEffect(name)) if effect == "nothing" && name == "nothing")
    );
    assert!(
        matches!(&render.skipped[1], (effect, MeshError::NoPass(name)) if effect == "synthetic" && name == "synthetic")
    );
    assert!(near(middle(&render.rendered), [255, 0, 0, 255]));
}

#[test]
fn the_forward_technique_is_the_one_chosen_and_a_skinned_surface_takes_a_skinned_one() {
    fn effect(names: &[&str]) -> Effect {
        let pass = || Pass {
            stages: [(ShaderStageKind::Vertex, "vs".to_string())]
                .into_iter()
                .collect(),
        };
        Effect {
            name: "e".into(),
            source_path: "e.fxc".into(),
            vertex_type: 0,
            defaults: Default::default(),
            techniques: names
                .iter()
                .map(|name| Technique {
                    name: (*name).into(),
                    passes: vec![pass()],
                })
                .collect(),
        }
    }
    let opaque = Coverage::Opaque;
    let both = effect(&[
        "draw",
        "lightweight0_draw",
        "drawskinned",
        "lightweight0_drawskinned",
    ]);
    assert!(select_pass(&both, false, opaque).is_some());
    let only_draw = effect(&["draw"]);
    assert!(select_pass(&only_draw, false, opaque).is_some());
    assert!(
        select_pass(&only_draw, true, opaque).is_none(),
        "no skinned technique"
    );
    assert!(select_pass(&effect(&["unlit_drawskinned"]), true, opaque).is_some());
    assert!(select_pass(&effect(&["PostFx"]), false, opaque).is_none());
    // The first technique tried is the forward one
    assert_eq!(techniques(false, opaque)[0], "lightweight0_draw");
    assert_eq!(techniques(true, opaque)[0], "lightweight0_drawskinned");
    // A cutout clips by the technique an effect has for it, else it draws as an opaque surface does
    assert_eq!(
        techniques(false, Coverage::Cutout)[0],
        "lightweight0CutOut_draw"
    );
    assert_eq!(
        techniques(true, Coverage::Cutout)[0],
        "lightweight0CutOut_drawskinned"
    );
    assert_eq!(
        techniques(false, Coverage::Blended),
        techniques(false, opaque)
    );
    let clipped = effect(&["lightweight0_draw", "lightweight0CutOut_draw"]);
    let chosen = |coverage| {
        select_pass(&clipped, false, coverage)
            .map(std::ptr::from_ref)
            .and_then(|pass| {
                clipped
                    .techniques
                    .iter()
                    .position(|t| std::ptr::eq(&t.passes[0], pass))
            })
    };
    assert_eq!(
        chosen(Coverage::Cutout),
        Some(1),
        "the cutout's own technique"
    );
    assert_eq!(chosen(opaque), Some(0));
    assert_eq!(chosen(Coverage::Blended), Some(0));
    let no_cutout = effect(&["lightweight0_draw"]);
    assert!(select_pass(&no_cutout, false, Coverage::Cutout).is_some());
    // An effect with no forward technique is drawn by its deferred one, which writes the albedo first
    let deferred_only = effect(&["deferred_draw", "deferredalphaclip_draw"]);
    assert!(select_pass(&deferred_only, false, opaque).is_some());
    let at = |effect: &Effect, coverage| {
        let pass = select_pass(effect, false, coverage)?;
        effect
            .techniques
            .iter()
            .position(|t| std::ptr::eq(&t.passes[0], pass))
    };
    assert_eq!(
        at(&deferred_only, opaque),
        Some(0),
        "the plain one for what is opaque"
    );
    assert_eq!(
        at(&deferred_only, Coverage::Cutout),
        Some(1),
        "the one that clips for a cutout"
    );
    let grass = effect(&["deferredalphaclip_draw", "deferredsubsamplealpha_draw"]);
    assert_eq!(at(&grass, opaque), Some(0));
    assert!(
        select_pass(&grass, true, opaque).is_none(),
        "no skinned deferred technique"
    );
    assert!(select_pass(&effect(&["deferred_drawskinned"]), true, opaque).is_some());
    assert!(
        select_pass(&effect(&["deferredalphaclip_drawskinned"]), true, opaque).is_some(),
        "a skinned effect with only the pass that clips"
    );
    // What lights itself comes before what only writes the albedo
    let both_kinds = effect(&["deferred_draw", "lightweight0_draw"]);
    assert_eq!(at(&both_kinds, opaque), Some(1));
    // A pass that binds a stage this does not draw is not chosen
    let mut geometry = effect(&["lightweight0_draw"]);
    geometry.techniques[0].passes[0]
        .stages
        .insert(ShaderStageKind::Geometry, "gs".into());
    assert!(select_pass(&geometry, false, opaque).is_none());
}

#[test]
fn a_route_takes_the_surfaces_and_the_techniques_it_draws_with() {
    fn effect(names: &[&str]) -> Effect {
        Effect {
            name: "e".into(),
            source_path: "e.fxc".into(),
            vertex_type: 0,
            defaults: Default::default(),
            techniques: names
                .iter()
                .map(|name| Technique {
                    name: (*name).into(),
                    passes: vec![Pass {
                        stages: [(ShaderStageKind::Vertex, "vs".to_string())]
                            .into_iter()
                            .collect(),
                    }],
                })
                .collect(),
        }
    }
    let at = |effect: &Effect, route, skinned, coverage| {
        let pass = select_pass_for(effect, route, skinned, coverage)?;
        effect
            .techniques
            .iter()
            .position(|t| std::ptr::eq(&t.passes[0], pass))
    };
    let (opaque, cutout, blended) = (Coverage::Opaque, Coverage::Cutout, Coverage::Blended);
    // What is opaque and what is cut out are written to the G-buffer, and what blends is not
    for (route, taken) in [
        (Route::Forward, [true, true, true]),
        (Route::GBuffer, [true, true, false]),
        (Route::Blended, [false, false, true]),
    ] {
        assert_eq!([opaque, cutout, blended].map(|c| route.takes(c)), taken);
    }
    // The G-buffer route skips the forward technique the forward route prefers
    let both = effect(&[
        "lightweight0_draw",
        "deferred_draw",
        "deferredalphaclip_draw",
    ]);
    assert_eq!(at(&both, Route::Forward, false, opaque), Some(0));
    assert_eq!(at(&both, Route::GBuffer, false, opaque), Some(1));
    assert_eq!(at(&both, Route::GBuffer, false, cutout), Some(2));
    // What blends is never written to the G-buffer, though the effect has nothing else
    let deferred_only = effect(&["deferred_draw"]);
    assert_eq!(at(&deferred_only, Route::Blended, false, blended), None);
    assert_eq!(at(&deferred_only, Route::Forward, false, blended), Some(0));
    assert_eq!(at(&both, Route::Blended, false, blended), Some(0));
    // A skinned surface takes the skinned technique of its route
    let skinned = effect(&["lightweight0_drawskinned", "deferred_drawskinned"]);
    assert_eq!(at(&skinned, Route::GBuffer, true, opaque), Some(1));
    assert_eq!(at(&skinned, Route::GBuffer, false, opaque), None);
}

#[test]
fn the_water_fog_route_draws_a_water_effect_by_its_fog_technique_and_takes_nothing_else() {
    let effect = |name: &str, techniques: &[&str]| Effect {
        name: name.into(),
        source_path: "e.fxc".into(),
        vertex_type: 0,
        defaults: Default::default(),
        techniques: techniques
            .iter()
            .map(|technique| Technique {
                name: (*technique).into(),
                passes: vec![Pass {
                    stages: [(ShaderStageKind::Vertex, "vs".to_string())]
                        .into_iter()
                        .collect(),
                }],
            })
            .collect(),
    };
    // The fog technique is the one the game names for the river fog pass; the others are what the water draws itself with
    let water = effect("water_shallow", &["alt4_draw", "imposter_draw"]);
    let pass =
        select_pass_for(&water, Route::WaterFog, false, Coverage::Blended).expect("a fog pass");
    assert!(std::ptr::eq(&water.techniques[1].passes[0], pass));
    // What is opaque is left to the G-buffer, and an effect with no fog technique has no fog pass
    assert!(!Route::WaterFog.takes(Coverage::Opaque));
    assert!(Route::WaterFog.takes(Coverage::Blended));
    let plain = effect("default", &["deferred_draw", "lightweight0_draw"]);
    assert!(select_pass_for(&plain, Route::WaterFog, false, Coverage::Blended).is_none());
}

#[test]
fn each_vertex_is_one_index_of_the_surface_with_each_attribute_written_by_semantic() {
    let surface = Surface {
        positions: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]],
        normals: vec![[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        uvs: [(1, vec![[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]])]
            .into_iter()
            .collect(),
        colours: [(0, vec![[0.5; 4]; 3])].into_iter().collect(),
        indices: vec![2, 0, 1],
        ..Surface::default()
    };
    let attribute = |format, offset, location| wgpu::VertexAttribute {
        format,
        offset,
        shader_location: location,
    };
    use wgpu::VertexFormat as F;
    let attributes = [
        attribute(F::Float32x3, 0, 0),
        attribute(F::Float32x3, 12, 1),
        attribute(F::Float32x2, 24, 2),
        attribute(F::Float32x4, 32, 3),
        // A texture coordinate set the surface lacks, and a semantic nothing writes
        attribute(F::Float32x2, 48, 4),
        attribute(F::Float32x2, 56, 5),
    ];
    let inputs = [
        element("POSITION", 0, 0, 7),
        element("NORMAL", 0, 1, 7),
        element("TEXCOORD", 1, 2, 3),
        element("COLOR", 0, 3, 15),
        element("TEXCOORD", 0, 4, 3),
        element("BLENDINDICES", 0, 5, 3),
    ];
    let bytes = surface_vertices(&surface, &attributes, 64, &inputs, &[]);
    assert_eq!(bytes.len(), 3 * 64);
    let words: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    // The first vertex is index 2
    assert_eq!(&words[..3], &[7.0, 8.0, 9.0]);
    assert_eq!(&words[3..6], &[1.0, 0.0, 0.0]);
    assert_eq!(&words[6..8], &[0.5, 0.6]);
    assert_eq!(&words[8..12], &[0.5; 4]);
    assert_eq!(
        &words[12..16],
        &[0.0; 4],
        "an absent set and an unwritten semantic are zero"
    );
    // The second is index 0, and the third index 1
    assert_eq!(&words[16..19], &[1.0, 2.0, 3.0]);
    assert_eq!(&words[32..35], &[4.0, 5.0, 6.0]);
}

/// The vertices written one at a time, as the first version of `surface_vertices` did.
fn vertices_one_at_a_time(
    surface: &Surface,
    attributes: &[wgpu::VertexAttribute],
    stride: u64,
    inputs: &[SignatureElement],
    palette: &[u16],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in &surface.indices {
        let at = *index as usize;
        let mut written = vec![0u8; stride as usize];
        for attribute in attributes {
            let values = inputs
                .iter()
                .find(|i| i.register == attribute.shader_location)
                .map_or([0.0; 4], |element| {
                    input_value(surface, element, at, palette)
                });
            let words = (attribute.format.size() / 4) as usize;
            let by_the_byte = inputs.iter().any(|input| {
                input.register == attribute.shader_location && input.semantic == "BLENDINDICES"
            }) && attribute.format == wgpu::VertexFormat::Float32x4;
            let values = if by_the_byte {
                values.map(|slot| slot / 255.0)
            } else {
                values
            };
            let integer = matches!(
                attribute.format,
                wgpu::VertexFormat::Uint32
                    | wgpu::VertexFormat::Uint32x2
                    | wgpu::VertexFormat::Uint32x3
                    | wgpu::VertexFormat::Uint32x4
            );
            for (i, value) in values.iter().take(words).enumerate() {
                let place = attribute.offset as usize + 4 * i;
                let bits = if integer {
                    (*value as u32).to_le_bytes()
                } else {
                    value.to_le_bytes()
                };
                written[place..place + 4].copy_from_slice(&bits);
            }
        }
        bytes.extend(written);
    }
    bytes
}

#[test]
fn a_big_skinned_surface_is_written_byte_for_byte_as_one_vertex_at_a_time_writes_it() {
    let count = 5000usize;
    let at = |n: usize, k: usize| ((n * 31 + k * 17) % 97) as f32 / 7.0;
    let surface = Surface {
        positions: (0..count).map(|n| [at(n, 0), at(n, 1), at(n, 2)]).collect(),
        normals: (0..count).map(|n| [at(n, 3), at(n, 4), at(n, 5)]).collect(),
        tangents: (0..count)
            .map(|n| [at(n, 6), at(n, 7), at(n, 8), 1.0])
            .collect(),
        uvs: [(0, (0..count).map(|n| [at(n, 9), at(n, 10)]).collect())]
            .into_iter()
            .collect(),
        weights: (0..count)
            .map(|n| [at(n, 11), at(n, 12), 0.0, 0.0])
            .collect(),
        joints: (0..count)
            .map(|n| [(n % 5) as u16, ((n + 1) % 5) as u16, 0, 0])
            .collect(),
        // Every vertex used twice, in an order that is not the vertices'
        indices: (0..2 * count as u32)
            .map(|i| (i * 7) % count as u32)
            .collect(),
        skinned: true,
        ..Surface::default()
    };
    let attribute = |format, offset, location| wgpu::VertexAttribute {
        format,
        offset,
        shader_location: location,
    };
    use wgpu::VertexFormat as F;
    let attributes = [
        attribute(F::Float32x3, 0, 0),
        attribute(F::Float32x3, 12, 1),
        attribute(F::Float32x4, 24, 2),
        attribute(F::Float32x2, 40, 3),
        attribute(F::Float32x4, 48, 4),
        attribute(F::Float32x4, 64, 5),
        attribute(F::Uint32x4, 80, 6),
        attribute(F::Float32x2, 96, 7),
    ];
    let inputs = [
        element("POSITION", 0, 0, 7),
        element("NORMAL", 0, 1, 7),
        element("TANGENT", 0, 2, 15),
        element("TEXCOORD", 0, 3, 3),
        element("BLENDWEIGHT", 0, 4, 15),
        element("BLENDINDICES", 0, 5, 15),
        element("BLENDINDICES", 0, 6, 15),
    ];
    let palette = [0u16, 1, 2, 3, 4];
    assert_eq!(
        surface_vertices(&surface, &attributes, 112, &inputs, &palette),
        vertices_one_at_a_time(&surface, &attributes, 112, &inputs, &palette)
    );
}

#[test]
fn a_surface_without_normals_tangents_or_weights_gets_the_defaults_an_upright_unskinned_one_has() {
    let surface = Surface {
        positions: vec![[0.0; 3]],
        indices: vec![0],
        ..Surface::default()
    };
    let at =
        |semantic: &str, index| input_value(&surface, &element(semantic, index, 0, 15), 0, &[]);
    assert_eq!(at("NORMAL", 0), [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(at("TANGENT", 0), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(at("BLENDWEIGHT", 0), [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(at("COLOR", 0), [1.0; 4]);
    assert_eq!(at("POSITION", 0), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn a_camera_framing_a_box_sees_all_of_it_from_in_front_and_above() {
    let (lo, hi) = (Vec3::new(-2.0, -1.0, 0.0), Vec3::new(2.0, 1.0, 3.0));
    let camera = Camera::framing(lo, hi);
    assert_eq!(camera.target, Vec3::new(0.0, 0.0, 1.5));
    assert_eq!(camera.up, Vec3::Z);
    assert!(camera.eye.z > camera.target.z, "above the middle");
    assert!(camera.eye.y < camera.target.y, "on the front side");
    let clip = camera.projection() * camera.view();
    for x in [lo.x, hi.x] {
        for y in [lo.y, hi.y] {
            for z in [lo.z, hi.z] {
                let p = clip * glam::Vec4::new(x, y, z, 1.0);
                let (ndc_x, ndc_y, ndc_z) = (p.x / p.w, p.y / p.w, p.z / p.w);
                assert!(ndc_x.abs() <= 1.0 && ndc_y.abs() <= 1.0, "{ndc_x} {ndc_y}");
                assert!((0.0..=1.0).contains(&ndc_z), "{ndc_z}");
            }
        }
    }
    // A degenerate box still has a camera that is somewhere
    let point = Camera::framing(Vec3::ONE, Vec3::ONE);
    assert!(point.eye.is_finite() && point.near > 0.0 && point.far > point.near);
}

#[test]
fn the_light_comes_from_the_viewers_side_and_from_above() {
    let camera = Camera::framing(Vec3::new(-2.0, -1.0, 0.0), Vec3::new(2.0, 1.0, 3.0));
    let sun = camera.light_direction();
    let to_camera = (camera.eye - camera.target).normalize();
    assert!((sun.length() - 1.0).abs() < 1e-5, "{sun:?}");
    assert!(
        sun.dot(to_camera) > 0.5,
        "a surface facing the camera is lit"
    );
    assert!(sun.z > to_camera.z + 0.2, "higher than the camera is");
    let right = (camera.target - camera.eye).cross(camera.up).normalize();
    assert!(sun.dot(right) > 0.2, "to the right of the view");
    // Turning the camera about the target turns the light with it
    let behind = Camera {
        eye: camera.target
            + (camera.target - camera.eye) * Vec3::new(1.0, 1.0, 0.0)
            + Vec3::new(0.0, 0.0, camera.eye.z - camera.target.z),
        ..camera
    };
    assert!(behind.light_direction().dot(sun) < 0.5);
    // A camera looking straight down has no side, and the light is still a direction
    let down = Camera {
        eye: camera.target + Vec3::Z * 5.0,
        up: Vec3::Y,
        ..camera
    };
    assert!((down.light_direction().length() - 1.0).abs() < 1e-5);
}

#[test]
fn the_bounds_of_surfaces_cover_every_position_and_an_empty_set_has_none() {
    let a = Surface {
        positions: vec![[0.0, 1.0, 2.0], [3.0, -1.0, 0.0]],
        ..Surface::default()
    };
    let b = Surface {
        positions: vec![[-4.0, 0.0, 9.0]],
        ..Surface::default()
    };
    let surfaces = [(a, Material::default()), (b, Material::default())];
    assert_eq!(
        bounds(&surfaces),
        Some((Vec3::new(-4.0, -1.0, 0.0), Vec3::new(3.0, 1.0, 9.0)))
    );
    assert_eq!(bounds(&[]), None);
}

/// `scene` drawn once into a fresh `SIZE`-square target of `target`, its constants written from `engine` first.
fn draw_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &MeshScene,
    target: &Target,
    engine: &EngineConstants,
) -> Rendered {
    let texture = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let colour = texture(
        target.colour,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = texture(target.depth, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let far = if target.depth_compare == wgpu::CompareFunction::GreaterEqual {
        0.0
    } else {
        1.0
    };
    scene.update(queue, engine);
    let colour_view = colour.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(far),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        scene.record(&mut pass);
    }
    queue.submit([encoder.finish()]);
    super::super::frame::read_back(device, queue, &colour, target.colour, SIZE)
}

fn overhead_engine() -> EngineConstants {
    EngineConstants {
        view: overhead().view(),
        projection: overhead().projection(),
        ..EngineConstants::default()
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_scene_draws_into_a_target_of_the_callers_format_and_depth_direction() {
    let dir = pack_on_disk();
    let Some((device, queue)) = gpu("a_scene_draws_into_a_target") else {
        return;
    };
    let pack = GamePack::open(dir.path()).unwrap();
    // Reverse depth, as a viewport has it: one at the near plane and none at the far, and the greater wins
    let target = Target {
        colour: wgpu::TextureFormat::Rgba8UnormSrgb,
        depth: wgpu::TextureFormat::Depth16Unorm,
        depth_compare: wgpu::CompareFunction::GreaterEqual,
        ..Target::default()
    };
    let engine = EngineConstants {
        projection: Mat4::perspective_infinite_reverse_rh(60f32.to_radians(), 1.0, 0.1),
        ..overhead_engine()
    };
    let white = Some([255, 255, 255, 255]);
    let half = material([0.5, 0.5, 0.5, 1.0], white);
    let green = material([0.0, 1.0, 0.0, 1.0], white);
    for surfaces in [
        [(quad(0.0), half.clone()), (quad(-1.0), green.clone())],
        [(quad(-1.0), green), (quad(0.0), half)],
    ] {
        let scene = MeshScene::prepare(&device, &queue, &pack, &target, &surfaces);
        assert_eq!((scene.drawn(), scene.skipped().len()), (2, 0));
        let got = middle(&draw_scene(&device, &queue, &scene, &target, &engine));
        // The nearer one, whose linear half the target stores as 188
        assert!(near(got, [188, 188, 188, 255]), "{got:?}");
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_prepared_scene_takes_new_constants_each_frame() {
    let dir = pack_on_disk();
    let Some((device, queue)) = gpu("a_prepared_scene_takes_new_constants") else {
        return;
    };
    let pack = GamePack::open(dir.path()).unwrap();
    let mut sunlit = material([1.0; 4], None);
    sunlit.effect = "sunlit".into();
    let target = Target::default();
    let scene = MeshScene::prepare(&device, &queue, &pack, &target, &[(quad(0.0), sunlit)]);
    let mut seen = Vec::new();
    for sun in [Vec3::X, -Vec3::X, Vec3::X] {
        let engine = EngineConstants {
            sun_direction: sun,
            ..overhead_engine()
        };
        seen.push(middle(&draw_scene(&device, &queue, &scene, &target, &engine))[0]);
    }
    // The x component of the way the light travels, from minus one to one, as a colour channel
    assert!(seen[0].abs_diff(0) <= 2, "{seen:?}");
    assert!(seen[1].abs_diff(255) <= 2, "{seen:?}");
    assert_eq!(seen[0], seen[2]);
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_pass_that_writes_a_second_colour_target_draws_into_a_pass_of_one_attachment() {
    let dir = pack_on_disk();
    let Some((device, queue)) = gpu("a_pass_that_writes_a_second_colour_target") else {
        return;
    };
    let pack = GamePack::open(dir.path()).unwrap();
    let mut two = material([1.0; 4], None);
    two.effect = "two".into();
    let target = Target::default();
    let scene = MeshScene::prepare(&device, &queue, &pack, &target, &[(quad(0.0), two)]);
    assert_eq!((scene.drawn(), scene.skipped().len()), (1, 0));
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let got = middle(&draw_scene(
        &device,
        &queue,
        &scene,
        &target,
        &overhead_engine(),
    ));
    assert!(pollster::block_on(validation.pop()).is_none());
    // What it writes first is what the target holds; the second is ignored
    assert!(near(got, [255, 255, 255, 255]), "{got:?}");
}

fn blended(tint: [f32; 4]) -> Material {
    Material {
        coverage: Coverage::Blended,
        ..material(tint, Some([255; 4]))
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_blended_surface_is_drawn_over_what_is_opaque_whichever_comes_first() {
    let dir = pack_on_disk();
    let opaque = || (quad(-1.0), material([0.0, 0.0, 1.0, 1.0], Some([255; 4])));
    // Half red, nearer the camera than the blue square it covers, listed before it and after it
    for surfaces in [
        [(quad(0.0), blended([1.0, 0.0, 0.0, 0.5])), opaque()],
        [opaque(), (quad(0.0), blended([1.0, 0.0, 0.0, 0.5]))],
    ] {
        let Some(rendered) = render(&dir, &surfaces, "a_blended_surface_is_drawn_over") else {
            return;
        };
        let got = middle(&rendered.unwrap().rendered);
        assert!(near(got, [128, 0, 128, got[3]]), "{got:?}");
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_blended_surface_does_not_hide_one_behind_it() {
    let dir = pack_on_disk();
    // Half red nearer, then half green behind it: both show, which a depth write by the first would stop
    let surfaces = [
        (quad(0.0), blended([1.0, 0.0, 0.0, 0.5])),
        (quad(-1.0), blended([0.0, 1.0, 0.0, 0.5])),
    ];
    let Some(rendered) = render(&dir, &surfaces, "a_blended_surface_does_not_hide") else {
        return;
    };
    let got = middle(&rendered.unwrap().rendered);
    assert!(near(got, [64, 128, 0, got[3]]), "{got:?}");
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_scene_made_again_builds_none_of_the_pipelines_it_has_built() {
    let dir = pack_on_disk();
    let Some((device, queue)) = gpu("a_scene_made_again_builds_none") else {
        return;
    };
    let pack = GamePack::open(dir.path()).unwrap();
    let target = Target::default();
    let mut cache = PipelineCache::new();
    assert!(cache.is_empty());
    let mut sunlit = material([1.0; 4], None);
    sunlit.effect = "sunlit".into();
    let surfaces = [
        (quad(0.0), material([1.0; 4], None)),
        (quad(0.0), material([0.5; 4], None)),
        (quad(0.0), sunlit),
    ];
    let first = MeshScene::prepare_with(&mut cache, &device, &queue, &pack, &target, &surfaces);
    assert_eq!(first.drawn(), 3);
    assert_eq!(cache.len(), 2, "one for each effect, whatever the surfaces");
    let again =
        MeshScene::prepare_with(&mut cache, &device, &queue, &pack, &target, &surfaces[..1]);
    assert_eq!(again.drawn(), 1);
    assert_eq!(cache.len(), 2, "the second scene built nothing");
    // A pair that will not build is remembered as well, and a surface of it is left out each time
    let mut missing = material([1.0; 4], None);
    missing.effect = "nothing".into();
    let scene = MeshScene::prepare_with(
        &mut cache,
        &device,
        &queue,
        &pack,
        &target,
        &[(quad(0.0), missing)],
    );
    assert_eq!(scene.drawn(), 0);
    // The drawn scenes are real: each draws what a scene made with no cache does
    let engine = overhead_engine();
    let a = middle(&draw_scene(&device, &queue, &again, &target, &engine));
    let b = middle(&draw_scene(
        &device,
        &queue,
        &MeshScene::prepare(&device, &queue, &pack, &target, &surfaces[..1]),
        &target,
        &engine,
    ));
    assert_eq!(a, b);
}

/// The synthetic pack with the effect `synthetic` declaring half a tint and `sunlit` declaring a sun straight up.
fn pack_with_defaults() -> tempfile::TempDir {
    crate::game_shaders::synthetic::pack_builder()
        // Two words more than the variable has room for, which are cut
        .default_of(
            "synthetic",
            "matTint",
            &[0.5f32, 1.0, 1.0, 1.0, 7.0, 7.0].map(f32::to_bits),
        )
        .default_of(
            "sunlit",
            "gDirectionalLight",
            &[0.0f32, 0.0, 1.0, 0.0].map(f32::to_bits),
        )
        .write()
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_constant_nothing_sets_holds_what_the_effect_declares_and_a_material_sets_over_it() {
    let dir = pack_with_defaults();
    let white = Some([255u8; 4]);
    let bare = Material {
        vectors: Vec::new(),
        ..material([0.0; 4], white)
    };
    let Some(rendered) = render(&dir, &[(quad(0.0), bare)], "a_constant_nothing_sets") else {
        return;
    };
    let got = middle(&rendered.unwrap().rendered);
    // The declared tint scales white to half red, and is no black
    assert!(near(got, [128, 255, 255, 255]), "{got:?}");
    let set = material([0.0, 0.0, 1.0, 1.0], white);
    let got = middle(
        &render(&dir, &[(quad(0.0), set)], "a_constant_nothing_sets")
            .unwrap()
            .unwrap()
            .rendered,
    );
    assert!(
        near(got, [0, 0, 255, 255]),
        "the material's value wins: {got:?}"
    );
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn what_the_engine_sets_wins_over_what_the_effect_declares() {
    let dir = pack_with_defaults();
    let mut sunlit = material([1.0; 4], None);
    sunlit.effect = "sunlit".into();
    let Some(rendered) = render(&dir, &[(quad(0.0), sunlit)], "what_the_engine_sets_wins") else {
        return;
    };
    let got = middle(&rendered.unwrap().rendered);
    let sun = overhead().light_direction();
    let want = (-sun.x * 0.5 + 0.5) * 255.0;
    // The declared sun is straight up, whose x is nought and so mid-grey; the engine's is not
    assert!((f32::from(got[0]) - want).abs() <= 2.5, "{got:?} {sun:?}");
    assert!(
        (f32::from(got[0]) - 127.5).abs() > 3.0,
        "not the declared one: {got:?}"
    );
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn what_the_engine_sets_wins_over_what_a_drawable_carries_for_it() {
    let dir = pack_with_defaults();
    let mut sunlit = material([1.0; 4], None);
    sunlit.effect = "sunlit".into();
    // The drawable's own value for the sun is straight up, which is not what the camera's light is
    sunlit
        .vectors
        .push(("gDirectionalLight".into(), vec![[0.0, 0.0, 1.0, 0.0]]));
    let Some(rendered) = render(
        &dir,
        &[(quad(0.0), sunlit)],
        "what_the_engine_sets_wins_over",
    ) else {
        return;
    };
    let got = middle(&rendered.unwrap().rendered);
    let sun = overhead().light_direction();
    let want = (-sun.x * 0.5 + 0.5) * 255.0;
    assert!((f32::from(got[0]) - want).abs() <= 2.5, "{got:?} {sun:?}");
    assert!(
        (f32::from(got[0]) - 127.5).abs() > 3.0,
        "not the drawable's: {got:?}"
    );
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn a_texture_has_levels_below_its_first_and_the_coarsest_is_its_average() {
    let dir = pack_on_disk();
    // A checkerboard of single texels, sixty-four a side
    let mut checker = Vec::new();
    for y in 0..64u32 {
        for x in 0..64u32 {
            let on = (x + y) % 2 == 0;
            checker.extend([if on { 255 } else { 0 }; 3]);
            checker.push(255);
        }
    }
    let mut surface = material([1.0; 4], None);
    surface.effect = "coarse".into();
    surface.textures.insert(
        "DiffuseSampler".into(),
        Image {
            width: 64,
            height: 64,
            rgba: checker,
            srgb: false,
        },
    );
    let Some(rendered) = render(&dir, &[(quad(0.0), surface)], "a_texture_has_levels") else {
        return;
    };
    let rendered = rendered.expect("the surface renders").rendered;
    let row = (SIZE / 2 * SIZE * 4) as usize;
    for x in 12..20usize {
        let red = rendered.bytes[row + x * 4];
        assert!(
            red.abs_diff(128) <= 2,
            "pixel {x} reads {red}, a texel of the first level and not the average of all"
        );
    }
}

#[test]
fn a_normal_map_the_material_does_not_bind_is_flat_and_any_other_texture_grey() {
    let texel = |sampler: &str| match missing(sampler) {
        Provided::Texture2d { rgba, srgb, .. } => (rgba, srgb),
        _ => panic!("a texture is provided as texels"),
    };
    // A flat normal points along the surface's own, a grey one tilts every surface the same way
    assert_eq!(texel("BumpSampler"), (vec![128, 128, 255, 255], false));
    assert_eq!(texel("bumpsampler2"), (vec![128, 128, 255, 255], false));
    assert_eq!(texel("DiffuseSampler"), (vec![200, 200, 200, 255], false));
    assert_eq!(texel("SpecSampler"), (vec![200, 200, 200, 255], false));
}

#[test]
fn a_palette_is_read_by_point_and_no_other_sampler_is() {
    for palette in [
        "TextureSamplerDiffPal",
        "DecorationTintPaletteTexSampler",
        "textureSamplerDiffPal",
    ] {
        assert!(is_palette(palette), "{palette}");
    }
    for other in [
        "DiffuseSampler",
        "BumpSampler",
        "SpecSampler",
        "DiffuseRampTextureSampler",
        "DiffuseExtraSampler",
    ] {
        assert!(!is_palette(other), "{other}");
    }
}

#[test]
fn glass_and_a_rotor_blur_are_seen_through_and_no_other_effect_is() {
    for clear in [
        "vehicle_vehglass",
        "vehicle_vehglass_inner",
        "glass_emissive",
        "vehicle_blurredrotor",
        "Vehicle_BlurredRotor_Emissive",
    ] {
        assert!(sees_through(clear), "{clear}");
    }
    for other in [
        "decal",
        "normal_spec_decal",
        "ped_emissive",
        "default",
        "normal_spec",
    ] {
        assert!(!sees_through(other), "{other}");
    }
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn surfaces_whose_constants_come_to_the_same_bytes_share_a_buffer_and_each_keeps_its_own_otherwise()
{
    let test = "surfaces_whose_constants_come_to_the_same_bytes";
    let Some((device, queue)) = gpu(test) else {
        return;
    };
    let dir = pack_on_disk();
    let pack = GamePack::open(dir.path()).unwrap();
    let red = material([1.0, 0.0, 0.0, 1.0], Some([255, 255, 255, 255]));
    let green = material([0.0, 1.0, 0.0, 1.0], Some([255, 255, 255, 255]));
    let target = Target::default();
    let surfaces = [
        (quad(0.0), red.clone()),
        (quad(-1.0), red),
        (quad(-2.0), green),
    ];
    let scene = MeshScene::prepare(&device, &queue, &pack, &target, &surfaces);
    assert_eq!(scene.drawn(), 3);
    // The matrices are the engine's alone, one buffer for the three; the tint is the material's, one for the two reds and one for the green
    assert_eq!(scene.buffers(), 3);
}

#[cfg_attr(
    not(feature = "gpu-tests"),
    ignore = "needs a GPU adapter: acquisition can hang, not just fail"
)]
#[test]
fn water_replaces_what_it_covers_with_the_colour_it_writes_though_its_alpha_is_nought() {
    let dir = pack_on_disk();
    let Some((device, queue)) = gpu("water_replaces_what_it_covers") else {
        return;
    };
    let pack = GamePack::open(dir.path()).unwrap();
    let mut water = material([1.0; 4], None);
    water.effect = "water_test".into();
    water.coverage = Coverage::Blended;
    let target = Target::default();
    let scene = MeshScene::prepare(&device, &queue, &pack, &target, &[(quad(0.0), water)]);
    // Its pass is the one an effect of its kind names by number, and it draws
    assert_eq!((scene.drawn(), scene.skipped().len()), (1, 0));
    let got = middle(&draw_scene(
        &device,
        &queue,
        &scene,
        &target,
        &overhead_engine(),
    ));
    // 0.5, 0.25 and 0.125 as the target stores them, where alpha blending of a nought alpha would leave none of it
    assert!(near(got, [128, 64, 32, 0]), "{got:?}");
}
