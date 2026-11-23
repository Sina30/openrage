//! Where each binding a program declares goes in the module, and what it is called: one bind group
//! per stage, bindings numbered densely in `PSV0` order, named by the game where the caller knows.

use or_io::gta5::shaders::spirv_pack::{Binding, BindingClass};
use or_io::hash::string_hash_str;

use super::container::{Psv, PsvResourceKind};
use super::metadata::{ComponentType, DxResources, ResourceDecl, ResourceShape};
use super::{Naming, Register, ResourceClass, ShaderKind};

/// The bind group of a stage's bindings: the pixel stage's after the others', so no pass collides.
// As GTA V's `BindingLayout::for_program_type` places them.
pub const fn group_of(kind: ShaderKind) -> u32 {
    match kind {
        ShaderKind::Pixel => 1,
        _ => 0,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Planned {
    Texture {
        dim: naga::ImageDimension,
        arrayed: bool,
        multisampled: bool,
        sampled: naga::ScalarKind,
    },
    StorageTexture {
        dim: naga::ImageDimension,
        arrayed: bool,
        format: naga::StorageFormat,
    },
    Sampler {
        comparison: bool,
    },
    /// A constant buffer as `array<vec4<u32>, rows>`.
    Uniform {
        rows: u32,
    },
    /// A raw, typed or structured buffer as `array<u32>`; `stride` for a structured one.
    Storage {
        read_only: bool,
        stride: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedBinding {
    pub register: Register,
    pub group: u32,
    /// Dense, in `PSV0` order.
    pub binding: u32,
    pub name: String,
    pub planned: Planned,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BindingPlan {
    pub bindings: Vec<PlannedBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("the {kind:?} range at {space}:{lower} is unbounded or spans {count} registers, which the first version does not bind")]
    Range {
        kind: PsvResourceKind,
        space: u32,
        lower: u32,
        count: u64,
    },
    #[error("constant buffer {0:?} states no size, and the effect gives none")]
    NoSize(Register),
    #[error("{0:?} is a kind of resource the first version does not bind")]
    Kind(PsvResourceKind),
}

/// The constant and resource names the renderer fills for GTA V; a hash of one of them is given
/// that name, because the renderer looks some of them up by their exact name.
const ENGINE_NAMES: &[&str] = &[
    "cloudShadowMinusBaseColourTimesShadowStrength",
    "deferredProjectionParams",
    "gAlphaRefVec0",
    "gAlphaRefVec1",
    "gBoneMtx",
    "gCSMDepthBias",
    "gCSMDepthSlopeBias",
    "gCSMResolution",
    "gCSMShaderVars_deferred",
    "gCSMShaderVars_shared",
    "gCSMShadowTexture",
    "gCSMShadowTextureSamp",
    "gCableParams",
    "gCameraPos",
    "gCloudViewProj",
    "gDeferredLightSampler0P",
    "gDeferredLightSampler2",
    "gDirectionalAmbientColour",
    "gDirectionalColour",
    "gDirectionalLight",
    "gDynamicBakesAndWetness",
    "gInvScreenSize",
    "gLightArtificialExtAmbient0",
    "gLightArtificialExtAmbient1",
    "gLightArtificialIntAmbient0",
    "gLightArtificialIntAmbient1",
    "gLightNaturalAmbient0",
    "gLightNaturalAmbient1",
    "gNormalOffset",
    "gOffsetScale0",
    "gOffsetScale1",
    "gReflectionMipCount",
    "gReflectionWorldViewProj",
    "gRefractionWorldViewProj",
    "gSuperAlpha",
    "gTransparencyAASamples",
    "gViewInverse",
    "gViewProj",
    "gWaterAmbientColor",
    "gWaterDirectionalColor",
    "gWorld",
    "gWorldView",
    "gWorldViewProj",
    "g_SSAOStrength",
    "g_projParams",
    "gbuffer",
    "gbufferStencilTextureGlobal",
    "glass",
    "globalAnimUV0",
    "globalAnimUV1",
    "globalFade",
    "globalFogColor",
    "globalFogColorE",
    "globalFogColorMoon",
    "globalFogColorN",
    "globalFogParams",
    "globalScalars",
    "globalScalars2",
    "globalScalars3",
    "globalScreenSize",
    "matWheelWorld",
    "matWheelWorldViewProj",
    "viewToWorldProjectionParam",
];

/// The name a hash takes: the GTA V engine name it is the hash of, else `0x` and eight hex digits.
pub fn hash_name(hash: u32) -> String {
    ENGINE_NAMES
        .iter()
        .find(|name| string_hash_str(name) == hash)
        .map_or_else(|| format!("0x{hash:08X}"), |name| (*name).to_string())
}

/// `name` with a hash spelling (`0x` and up to eight hex digits) put in [`hash_name`]'s form.
fn spelled(name: &str) -> String {
    name.strip_prefix("0x")
        .or_else(|| name.strip_prefix("0X"))
        .filter(|digits| (1..=8).contains(&digits.len()))
        .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .map_or_else(|| name.to_string(), hash_name)
}

/// Whether a range of `count` registers from `lower` in `space` holds `register`.
fn holds(space: u32, lower: u32, count: u32, register: Register) -> bool {
    space == register.space
        && register.index >= lower
        && u64::from(register.index) < u64::from(lower) + u64::from(count)
}

fn letter(class: ResourceClass) -> char {
    match class {
        ResourceClass::Cbv => 'b',
        ResourceClass::Sampler => 's',
        ResourceClass::Srv => 't',
        ResourceClass::Uav | ResourceClass::UavCounter => 'u',
    }
}

fn binding_class(class: ResourceClass) -> BindingClass {
    match class {
        ResourceClass::Cbv => BindingClass::Cbv,
        ResourceClass::Sampler => BindingClass::Sampler,
        ResourceClass::Srv => BindingClass::Srv,
        ResourceClass::Uav => BindingClass::Uav,
        ResourceClass::UavCounter => BindingClass::UavCounter,
    }
}

fn scalar_kind(component: ComponentType) -> naga::ScalarKind {
    match component {
        ComponentType::I16 | ComponentType::I32 | ComponentType::I64 => naga::ScalarKind::Sint,
        ComponentType::U16 | ComponentType::U32 | ComponentType::U64 => naga::ScalarKind::Uint,
        _ => naga::ScalarKind::Float,
    }
}

/// The image a texture shape is: dimension, arrayed, multisampled; `None` for a buffer or an
/// unknown shape.
fn image(shape: ResourceShape) -> Option<(naga::ImageDimension, bool, bool)> {
    use naga::ImageDimension as D;
    Some(match shape {
        ResourceShape::Texture1D => (D::D1, false, false),
        ResourceShape::Texture1DArray => (D::D1, true, false),
        ResourceShape::Texture2D => (D::D2, false, false),
        ResourceShape::Texture2DArray => (D::D2, true, false),
        ResourceShape::Texture2DMS => (D::D2, false, true),
        ResourceShape::Texture2DMSArray => (D::D2, true, true),
        ResourceShape::Texture3D => (D::D3, false, false),
        ResourceShape::TextureCube => (D::Cube, false, false),
        ResourceShape::TextureCubeArray => (D::Cube, true, false),
        _ => return None,
    })
}

/// A typed UAV's storage format: the element's scalar in one 32-bit channel, since the format bound
/// at run time is not in the program.
fn storage_format(component: ComponentType) -> naga::StorageFormat {
    match scalar_kind(component) {
        naga::ScalarKind::Sint => naga::StorageFormat::R32Sint,
        naga::ScalarKind::Uint => naga::StorageFormat::R32Uint,
        _ => naga::StorageFormat::R32Float,
    }
}

/// The shape of a shader-resource or unordered-access binding, from its declaration where the
/// module has one.
fn shaped(
    kind: PsvResourceKind,
    writable: bool,
    decl: Option<&ResourceDecl>,
) -> Result<Planned, PlanError> {
    let Some(decl) = decl else {
        // The 6.6 handle's annotation states the shape; the lowering sets it through `shape`.
        return Ok(match kind {
            PsvResourceKind::SrvTyped => Planned::Texture {
                dim: naga::ImageDimension::D2,
                arrayed: false,
                multisampled: false,
                sampled: naga::ScalarKind::Float,
            },
            PsvResourceKind::UavTyped => Planned::StorageTexture {
                dim: naga::ImageDimension::D2,
                arrayed: false,
                format: naga::StorageFormat::R32Float,
            },
            _ => Planned::Storage {
                read_only: !writable,
                stride: None,
            },
        });
    };
    if let Some((dim, arrayed, multisampled)) = image(decl.shape) {
        return Ok(if writable {
            Planned::StorageTexture {
                dim,
                arrayed,
                format: storage_format(decl.component),
            }
        } else {
            Planned::Texture {
                dim,
                arrayed,
                multisampled,
                sampled: scalar_kind(decl.component),
            }
        });
    }
    match decl.shape {
        ResourceShape::TypedBuffer | ResourceShape::RawBuffer => Ok(Planned::Storage {
            read_only: !writable,
            stride: None,
        }),
        ResourceShape::StructuredBuffer => Ok(Planned::Storage {
            read_only: !writable,
            stride: decl.stride,
        }),
        _ => Err(PlanError::Kind(kind)),
    }
}

impl BindingPlan {
    /// Every `PSV0` resource placed, shaped by `decls` or else by [`BindingPlan::shape`].
    pub fn new(
        kind: ShaderKind,
        psv: &Psv,
        decls: &DxResources,
        naming: &Naming,
    ) -> Result<Self, PlanError> {
        let group = group_of(kind);
        let mut bindings = Vec::with_capacity(psv.resources.len());
        // How many spaces hold each class and register, counted once: a PSV0 may state millions.
        let mut spaces: std::collections::HashMap<(ResourceClass, u32), usize> = Default::default();
        for r in &psv.resources {
            if let Some(class) = r.kind.class() {
                *spaces.entry((class, r.lower)).or_default() += 1;
            }
        }
        for (binding, resource) in (0u32..).zip(&psv.resources) {
            let class = match resource.kind {
                PsvResourceKind::UavStructuredWithCounter | PsvResourceKind::Other(_) => None,
                other => other.class(),
            }
            .ok_or(PlanError::Kind(resource.kind))?;
            let count = (u64::from(resource.upper) + 1).saturating_sub(u64::from(resource.lower));
            if count != 1 {
                return Err(PlanError::Range {
                    kind: resource.kind,
                    space: resource.space,
                    lower: resource.lower,
                    count,
                });
            }
            let register = Register {
                class,
                space: resource.space,
                index: resource.lower,
            };

            let (planned, bitcode_name) = match class {
                ResourceClass::Cbv => {
                    let decl = decls
                        .cbuffers
                        .iter()
                        .find(|decl| holds(decl.space, decl.lower, decl.count, register));
                    let size = naming
                        .constant_buffers
                        .get(&register)
                        .map(|buffer| buffer.size)
                        .filter(|&size| size > 0)
                        .or_else(|| decl.map(|decl| decl.size).filter(|&size| size > 0))
                        .ok_or(PlanError::NoSize(register))?;
                    let rows = size.div_ceil(16);
                    (
                        Planned::Uniform { rows },
                        decl.map(|decl| decl.name.as_str()),
                    )
                }
                ResourceClass::Sampler => {
                    let decl = decls
                        .samplers
                        .iter()
                        .find(|decl| holds(decl.space, decl.lower, decl.count, register));
                    let comparison = decl.is_some_and(|decl| decl.comparison);
                    (
                        Planned::Sampler { comparison },
                        decl.map(|decl| decl.name.as_str()),
                    )
                }
                ResourceClass::Srv | ResourceClass::Uav | ResourceClass::UavCounter => {
                    let writable = class != ResourceClass::Srv;
                    let list = if writable { &decls.uavs } else { &decls.srvs };
                    let decl = list
                        .iter()
                        .find(|decl| holds(decl.space, decl.lower, decl.count, register));
                    let planned = shaped(resource.kind, writable, decl)?;
                    (planned, decl.map(|decl| decl.name.as_str()))
                }
            };

            let one_space = spaces.get(&(class, register.index)) == Some(&1);
            let name = naming
                .resources
                .get(&register)
                .or_else(|| {
                    naming
                        .by_register
                        .get(&(class, register.index))
                        .filter(|_| one_space)
                })
                .map(String::as_str)
                .or(bitcode_name.filter(|name| !name.is_empty()))
                .map(spelled)
                .unwrap_or_else(|| {
                    format!(
                        "{}{}_space{}",
                        letter(class),
                        register.index,
                        register.space
                    )
                });

            bindings.push(PlannedBinding {
                register,
                group,
                binding,
                name,
                planned,
            });
        }
        Ok(Self { bindings })
    }

    pub fn find(&self, register: Register) -> Option<&PlannedBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.register == register)
    }

    /// Set the shape a 6.6 handle's annotation states, where `decls` had none.
    pub fn shape(&mut self, register: Register, planned: Planned) {
        if let Some(binding) = self
            .bindings
            .iter_mut()
            .find(|binding| binding.register == register)
        {
            binding.planned = planned;
        }
    }

    /// The manifest's bindings: name, kind word, class, register, group as `set`, binding.
    pub fn reflection(&self) -> Vec<Binding> {
        self.bindings
            .iter()
            .map(|binding| Binding {
                name: binding.name.clone(),
                kind: kind_word(&binding.planned).to_string(),
                class: binding_class(binding.register.class),
                register: binding.register.index,
                set: binding.group,
                binding: binding.binding,
            })
            .collect()
    }
}

/// The reflection's word for a binding's kind, in the words GTA V's `RDEF` reflection uses.
fn kind_word(planned: &Planned) -> &'static str {
    match planned {
        Planned::Texture { .. } => "texture",
        Planned::StorageTexture { .. } => "uav",
        Planned::Sampler { .. } => "sampler",
        Planned::Uniform { .. } => "cbuffer",
        Planned::Storage {
            read_only: true,
            stride: Some(_),
        } => "structured",
        Planned::Storage {
            read_only: true,
            stride: None,
        } => "byteaddress",
        Planned::Storage {
            read_only: false,
            stride: Some(_),
        } => "uav-structured",
        Planned::Storage {
            read_only: false,
            stride: None,
        } => "uav-byteaddress",
    }
}

#[cfg(test)]
mod tests {
    use or_io::gta5::shaders::spirv_pack::ConstantBuffer;

    use super::super::container::PsvResource;
    use super::super::metadata::{CbufferDecl, SamplerDecl};
    use super::*;

    fn psv(resources: &[(PsvResourceKind, u32, u32, u32)]) -> Psv {
        Psv {
            runtime_info_size: 52,
            resources: resources
                .iter()
                .map(|&(kind, space, lower, upper)| PsvResource {
                    kind,
                    space,
                    lower,
                    upper,
                })
                .collect(),
            threads: None,
        }
    }

    fn srv(name: &str, space: u32, lower: u32, shape: ResourceShape) -> ResourceDecl {
        ResourceDecl {
            id: lower + 100,
            name: name.into(),
            space,
            lower,
            count: 1,
            shape,
            component: ComponentType::U32,
            stride: Some(24),
        }
    }

    fn reg(class: ResourceClass, space: u32, index: u32) -> Register {
        Register {
            class,
            space,
            index,
        }
    }

    fn cbuffer(size: u32) -> ConstantBuffer {
        ConstantBuffer {
            name: "ignored".into(),
            size,
            variables: Vec::new(),
        }
    }

    #[test]
    fn bindings_are_dense_in_psv_order_in_the_stage_group() {
        let table = psv(&[
            (PsvResourceKind::Cbv, 0, 2, 2),
            (PsvResourceKind::SrvTyped, 1, 3, 3),
            (PsvResourceKind::Sampler, 0, 5, 5),
            (PsvResourceKind::SrvStructured, 2, 9, 9),
            (PsvResourceKind::UavTyped, 3, 1, 1),
        ]);
        let decls = DxResources {
            srvs: vec![
                srv("tex", 1, 3, ResourceShape::Texture2DArray),
                srv("items", 2, 9, ResourceShape::StructuredBuffer),
            ],
            uavs: vec![srv("out", 3, 1, ResourceShape::Texture3D)],
            cbuffers: vec![CbufferDecl {
                id: 0,
                name: "consts".into(),
                space: 0,
                lower: 2,
                count: 1,
                size: 40,
            }],
            samplers: vec![SamplerDecl {
                id: 0,
                name: "shadow".into(),
                space: 0,
                lower: 4,
                count: 2,
                comparison: true,
            }],
        };
        for (kind, group) in [(ShaderKind::Pixel, 1), (ShaderKind::Vertex, 0)] {
            let plan = BindingPlan::new(kind, &table, &decls, &Naming::default()).unwrap();
            let placed: Vec<_> = plan
                .bindings
                .iter()
                .map(|b| (b.group, b.binding, b.name.as_str()))
                .collect();
            assert_eq!(
                placed,
                [
                    (group, 0, "consts"),
                    (group, 1, "tex"),
                    (group, 2, "shadow"),
                    (group, 3, "items"),
                    (group, 4, "out"),
                ]
            );
            let shapes: Vec<_> = plan.bindings.iter().map(|b| b.planned.clone()).collect();
            assert_eq!(
                shapes,
                [
                    Planned::Uniform {
                        rows: 40u32.div_ceil(16)
                    },
                    Planned::Texture {
                        dim: naga::ImageDimension::D2,
                        arrayed: true,
                        multisampled: false,
                        sampled: naga::ScalarKind::Uint,
                    },
                    Planned::Sampler { comparison: true },
                    Planned::Storage {
                        read_only: true,
                        stride: Some(24)
                    },
                    Planned::StorageTexture {
                        dim: naga::ImageDimension::D3,
                        arrayed: false,
                        format: naga::StorageFormat::R32Uint,
                    },
                ]
            );
        }
    }

    #[test]
    fn names_come_by_register_then_by_class_then_from_the_bitcode_then_the_register() {
        let table = psv(&[
            (PsvResourceKind::SrvTyped, 0, 3, 3),
            (PsvResourceKind::SrvTyped, 1, 3, 3),
            (PsvResourceKind::SrvTyped, 2, 7, 7),
            (PsvResourceKind::SrvRaw, 1, 8, 8),
            (PsvResourceKind::SrvTyped, 3, 6, 6),
            (PsvResourceKind::UavRaw, 0, 6, 6),
        ]);
        let decls = DxResources {
            srvs: vec![srv("from_bitcode", 1, 3, ResourceShape::Texture2D)],
            ..DxResources::default()
        };
        let mut naming = Naming::default();
        naming
            .resources
            .insert(reg(ResourceClass::Srv, 0, 3), "by_register".into());
        naming
            .by_register
            .insert((ResourceClass::Srv, 3), "ambiguous".into());
        naming
            .by_register
            .insert((ResourceClass::Srv, 7), "by_class".into());
        // A UAV holding the same number does not make an SRV's register ambiguous.
        naming
            .by_register
            .insert((ResourceClass::Srv, 6), "unique".into());
        let plan = BindingPlan::new(ShaderKind::Vertex, &table, &decls, &naming).unwrap();
        let names: Vec<_> = plan.bindings.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "by_register",
                "from_bitcode",
                "by_class",
                "t8_space1",
                "unique",
                "u6_space0"
            ]
        );
    }

    #[test]
    fn a_hash_is_spelled_as_is_named_matches_and_an_engine_hash_takes_its_name() {
        let table = psv(&[
            (PsvResourceKind::SrvTyped, 0, 0, 0),
            (PsvResourceKind::SrvTyped, 0, 1, 1),
            (PsvResourceKind::SrvTyped, 0, 2, 2),
        ]);
        let unknown = 0x00AB_C0DEu32;
        assert!(ENGINE_NAMES.iter().all(|n| string_hash_str(n) != unknown));
        let mut naming = Naming::default();
        naming
            .resources
            .insert(reg(ResourceClass::Srv, 0, 0), format!("0x{unknown:x}"));
        naming.resources.insert(
            reg(ResourceClass::Srv, 0, 1),
            format!("0x{:x}", string_hash_str("gViewProj")),
        );
        naming
            .resources
            .insert(reg(ResourceClass::Srv, 0, 2), "0xnothex".into());
        let plan =
            BindingPlan::new(ShaderKind::Pixel, &table, &DxResources::default(), &naming).unwrap();
        let names: Vec<_> = plan.bindings.iter().map(|b| b.name.clone()).collect();
        assert_eq!(
            names,
            [
                format!("0x{unknown:08X}"),
                "gViewProj".into(),
                "0xnothex".into()
            ]
        );
        assert_eq!(hash_name(string_hash_str("gViewInverse")), "gViewInverse");
    }

    #[test]
    fn what_the_first_version_does_not_bind_is_refused_by_name() {
        let refused = |resources: &[(PsvResourceKind, u32, u32, u32)]| {
            BindingPlan::new(
                ShaderKind::Vertex,
                &psv(resources),
                &DxResources::default(),
                &Naming::default(),
            )
            .unwrap_err()
        };
        assert_eq!(
            refused(&[(PsvResourceKind::SrvTyped, 2, 7, 9)]),
            PlanError::Range {
                kind: PsvResourceKind::SrvTyped,
                space: 2,
                lower: 7,
                count: 3
            }
        );
        assert_eq!(
            refused(&[(PsvResourceKind::SrvRaw, 1, 4, u32::MAX)]),
            PlanError::Range {
                kind: PsvResourceKind::SrvRaw,
                space: 1,
                lower: 4,
                count: u64::from(u32::MAX) + 1 - 4
            }
        );
        assert_eq!(
            refused(&[(PsvResourceKind::UavStructuredWithCounter, 0, 1, 1)]),
            PlanError::Kind(PsvResourceKind::UavStructuredWithCounter)
        );
        assert_eq!(
            refused(&[(PsvResourceKind::Other(77), 0, 1, 1)]),
            PlanError::Kind(PsvResourceKind::Other(77))
        );
        assert_eq!(
            refused(&[(PsvResourceKind::Cbv, 3, 5, 5)]),
            PlanError::NoSize(reg(ResourceClass::Cbv, 3, 5))
        );
    }

    #[test]
    fn the_effects_buffer_size_wins_over_the_bitcodes() {
        let table = psv(&[(PsvResourceKind::Cbv, 1, 4, 4)]);
        let decls = DxResources {
            cbuffers: vec![CbufferDecl {
                id: 0,
                name: "cb".into(),
                space: 1,
                lower: 4,
                count: 1,
                size: 32,
            }],
            ..DxResources::default()
        };
        let mut naming = Naming::default();
        naming
            .constant_buffers
            .insert(reg(ResourceClass::Cbv, 1, 4), cbuffer(100));
        let plan = BindingPlan::new(ShaderKind::Vertex, &table, &decls, &naming).unwrap();
        assert_eq!(
            plan.bindings[0].planned,
            Planned::Uniform {
                rows: 100u32.div_ceil(16)
            }
        );
        let plan =
            BindingPlan::new(ShaderKind::Vertex, &table, &decls, &Naming::default()).unwrap();
        assert_eq!(
            plan.bindings[0].planned,
            Planned::Uniform {
                rows: 32u32.div_ceil(16)
            }
        );
    }

    #[test]
    fn shape_find_and_reflection_report_the_placed_bindings() {
        let table = psv(&[
            (PsvResourceKind::Sampler, 0, 2, 2),
            (PsvResourceKind::SrvTyped, 1, 5, 5),
            (PsvResourceKind::UavRaw, 0, 3, 3),
        ]);
        let mut plan = BindingPlan::new(
            ShaderKind::Pixel,
            &table,
            &DxResources::default(),
            &Naming::default(),
        )
        .unwrap();
        let cube = Planned::Texture {
            dim: naga::ImageDimension::Cube,
            arrayed: false,
            multisampled: false,
            sampled: naga::ScalarKind::Sint,
        };
        plan.shape(reg(ResourceClass::Srv, 1, 5), cube.clone());
        assert_eq!(
            plan.find(reg(ResourceClass::Srv, 1, 5)).map(|b| &b.planned),
            Some(&cube)
        );
        assert!(plan.find(reg(ResourceClass::Srv, 0, 5)).is_none());

        let reflected: Vec<_> = plan
            .reflection()
            .into_iter()
            .map(|b| (b.name, b.kind, b.class, b.register, b.set, b.binding))
            .collect();
        assert_eq!(
            reflected,
            [
                (
                    "s2_space0".to_string(),
                    "sampler".to_string(),
                    BindingClass::Sampler,
                    2,
                    1,
                    0
                ),
                (
                    "t5_space1".into(),
                    "texture".into(),
                    BindingClass::Srv,
                    5,
                    1,
                    1
                ),
                (
                    "u3_space0".into(),
                    "uav-byteaddress".into(),
                    BindingClass::Uav,
                    3,
                    1,
                    2
                ),
            ]
        );
    }
}
