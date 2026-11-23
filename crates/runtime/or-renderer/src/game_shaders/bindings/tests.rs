use super::*;

fn wgsl(source: &str) -> naga::Module {
    naga::front::wgsl::parse_str(source).expect("the WGSL parses")
}

fn one(source: &str) -> Resource {
    let found = requirements(&wgsl(source)).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    found[0].resource.clone()
}

#[test]
fn a_uniform_buffer_is_as_big_as_its_type() {
    // A vector array like the translator writes a constant buffer as
    let array = one("@group(1) @binding(3) var<uniform> cb: array<vec4<f32>, 7>;
        @fragment fn main() -> @location(0) vec4<f32> { return cb[0]; }");
    assert_eq!(array, Resource::Uniform { size: 7 * 16 });
    let record = one("struct S { a: vec4<f32>, b: f32 }
        @group(0) @binding(0) var<uniform> cb: S;
        @fragment fn main() -> @location(0) vec4<f32> { return cb.a * cb.b; }");
    assert_eq!(record, Resource::Uniform { size: 32 });
}

#[test]
fn group_and_binding_are_the_modules_own_and_the_list_is_in_order() {
    let found = requirements(&wgsl(
        "@group(1) @binding(37) var t: texture_2d<f32>;
         @group(0) @binding(18) var<uniform> u: vec4<f32>;
         @group(1) @binding(3) var<uniform> v: vec4<f32>;
         @fragment fn main() -> @location(0) vec4<f32> {
             return u + v + textureLoad(t, vec2<i32>(0, 0), 0);
         }",
    ))
    .unwrap();
    let at: Vec<_> = found.iter().map(|r| (r.group, r.binding)).collect();
    assert_eq!(at, vec![(0, 18), (1, 3), (1, 37)]);
}

#[test]
fn each_kind_and_shape_of_texture_has_its_own_view_dimension_and_sample_type() {
    use wgpu::{TextureSampleType as S, TextureViewDimension as D};
    let texture = |declaration: &str, load: &str| {
        one(&format!(
            "@group(0) @binding(0) var t: {declaration};
             @fragment fn main() -> @location(0) vec4<f32> {{ {load} }}"
        ))
    };
    let sampled = |dimension, sample, multisampled| Resource::Texture {
        dimension,
        sample,
        multisampled,
    };
    assert_eq!(
        texture("texture_2d<f32>", "return textureLoad(t, vec2<i32>(0), 0);"),
        sampled(D::D2, S::Float { filterable: true }, false)
    );
    assert_eq!(
        texture(
            "texture_2d_array<f32>",
            "return textureLoad(t, vec2<i32>(0), 0, 0);"
        ),
        sampled(D::D2Array, S::Float { filterable: true }, false)
    );
    assert_eq!(
        texture(
            "texture_cube<f32>",
            "return vec4<f32>(vec2<f32>(textureDimensions(t)), 0.0, 0.0);"
        ),
        sampled(D::Cube, S::Float { filterable: true }, false)
    );
    assert_eq!(
        texture(
            "texture_3d<u32>",
            "return vec4<f32>(vec4<u32>(textureLoad(t, vec3<i32>(0), 0)));"
        ),
        sampled(D::D3, S::Uint, false)
    );
    assert_eq!(
        texture(
            "texture_1d<i32>",
            "return vec4<f32>(vec4<i32>(textureLoad(t, 0, 0)));"
        ),
        sampled(D::D1, S::Sint, false)
    );
    assert_eq!(
        texture(
            "texture_depth_2d",
            "return vec4<f32>(textureLoad(t, vec2<i32>(0), 0));"
        ),
        sampled(D::D2, S::Depth, false)
    );
    assert_eq!(
        texture(
            "texture_multisampled_2d<f32>",
            "return textureLoad(t, vec2<i32>(0), 0);"
        ),
        sampled(D::D2, S::Float { filterable: false }, true)
    );
}

#[test]
fn a_sampler_is_a_comparison_sampler_only_when_the_module_says_so() {
    let found = requirements(&wgsl(
        "@group(0) @binding(0) var s: sampler;
         @group(0) @binding(1) var c: sampler_comparison;
         @group(0) @binding(2) var t: texture_depth_2d;
         @group(0) @binding(3) var f: texture_2d<f32>;
         @fragment fn main() -> @location(0) vec4<f32> {
             return textureSample(f, s, vec2<f32>(0.0))
                 + vec4<f32>(textureSampleCompare(t, c, vec2<f32>(0.0), 0.5));
         }",
    ))
    .unwrap();
    assert_eq!(found[0].resource, Resource::Sampler { comparison: false });
    assert_eq!(found[1].resource, Resource::Sampler { comparison: true });
}

#[test]
fn storage_buffers_and_textures_carry_their_access() {
    let buffer = |access: &str| {
        one(&format!(
            "@group(0) @binding(0) var<storage, {access}> b: array<vec4<f32>, 4>;
             @compute @workgroup_size(1) fn main() {{ {} }}",
            if access == "read" {
                "_ = b[0];"
            } else {
                "b[0] = vec4<f32>(1.0);"
            }
        ))
    };
    assert_eq!(
        buffer("read"),
        Resource::Storage {
            size: 64,
            read_only: true
        }
    );
    assert_eq!(
        buffer("read_write"),
        Resource::Storage {
            size: 64,
            read_only: false
        }
    );
    let image = one(
        "@group(0) @binding(0) var t: texture_storage_2d<rgba8unorm, write>;
         @compute @workgroup_size(1) fn main() { textureStore(t, vec2<i32>(0), vec4<f32>(1.0)); }",
    );
    assert_eq!(
        image,
        Resource::StorageTexture {
            dimension: wgpu::TextureViewDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            access: wgpu::StorageTextureAccess::WriteOnly
        }
    );
}

#[test]
fn a_global_with_no_binding_asks_for_nothing() {
    let found = requirements(&wgsl(
        "var<private> x: f32;
         @fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(x); }",
    ))
    .unwrap();
    assert!(found.is_empty());
}

#[test]
fn a_resource_becomes_the_layout_entry_that_holds_it() {
    let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
    let entry = Resource::Uniform { size: 64 }.layout_entry(9, both);
    assert_eq!((entry.binding, entry.visibility), (9, both));
    assert!(matches!(
        entry.ty,
        wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None
        }
    ));
    let sampler = Resource::Sampler { comparison: true }.layout_entry(1, both);
    assert!(matches!(
        sampler.ty,
        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison)
    ));
    let texture = Resource::Texture {
        dimension: wgpu::TextureViewDimension::Cube,
        sample: wgpu::TextureSampleType::Float { filterable: true },
        multisampled: false,
    }
    .layout_entry(2, both);
    assert!(matches!(
        texture.ty,
        wgpu::BindingType::Texture {
            view_dimension: wgpu::TextureViewDimension::Cube,
            multisampled: false,
            ..
        }
    ));
}

#[test]
fn a_storage_texture_carries_each_access_it_is_declared_with() {
    let access = |declared: &str, use_it: &str| {
        let module = naga::front::wgsl::parse_str(&format!(
            "@group(0) @binding(0) var t: texture_storage_2d<r32float, {declared}>;
             @compute @workgroup_size(1) fn main() {{ {use_it} }}"
        ));
        let module = module.expect("the WGSL parses");
        match &requirements(&module).unwrap()[0].resource {
            Resource::StorageTexture { access, .. } => *access,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        access("read", "_ = textureLoad(t, vec2<i32>(0));"),
        wgpu::StorageTextureAccess::ReadOnly
    );
    assert_eq!(
        access("write", "textureStore(t, vec2<i32>(0), vec4<f32>(1.0));"),
        wgpu::StorageTextureAccess::WriteOnly
    );
    assert_eq!(
        access(
            "read_write",
            "textureStore(t, vec2<i32>(0), textureLoad(t, vec2<i32>(0)));"
        ),
        wgpu::StorageTextureAccess::ReadWrite
    );
}
