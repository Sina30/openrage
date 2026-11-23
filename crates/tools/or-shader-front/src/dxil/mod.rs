//! DXIL to naga IR, in Rust: the container, its LLVM bitcode, the control flow structured, and the
//! dx.op intrinsics lowered.

pub mod bitstream;
pub mod container;
pub mod ir;
pub mod metadata;
pub mod ops;
pub mod plan;
pub mod read;
pub mod structure;
pub mod to_naga;

use std::collections::{BTreeMap, HashMap};

use or_io::gta5::shaders::spirv_pack::{ConstantBuffer, Fidelity, Reflection, ShaderStageKind};

pub use crate::layout::ResourceClass;

/// Names what wrote a module, for a cache key: bumped whenever a module would come out different.
pub const DXIL_FRONT: &str = concat!("or-shader-front dxil ", env!("CARGO_PKG_VERSION"), " r1");

/// The stage a DXIL program header states (its kind field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderKind {
    Pixel,
    Vertex,
    Geometry,
    Hull,
    Domain,
    Compute,
    Library,
    Mesh,
    Amplification,
    Other(u16),
}

impl ShaderKind {
    /// The kind in the high half of a program version word (the low half is the shader model).
    pub fn from_program_version(version: u32) -> Self {
        match version >> 16 {
            0 => Self::Pixel,
            1 => Self::Vertex,
            2 => Self::Geometry,
            3 => Self::Hull,
            4 => Self::Domain,
            5 => Self::Compute,
            6 => Self::Library,
            13 => Self::Mesh,
            14 => Self::Amplification,
            kind => Self::Other(kind as u16),
        }
    }

    /// The pack's stage, `None` for a stage the pack has no word for (mesh, amplification, library).
    pub fn stage(self) -> Option<ShaderStageKind> {
        match self {
            Self::Pixel => Some(ShaderStageKind::Pixel),
            Self::Vertex => Some(ShaderStageKind::Vertex),
            Self::Geometry => Some(ShaderStageKind::Geometry),
            Self::Hull => Some(ShaderStageKind::Hull),
            Self::Domain => Some(ShaderStageKind::Domain),
            Self::Compute => Some(ShaderStageKind::Compute),
            Self::Library | Self::Mesh | Self::Amplification | Self::Other(_) => None,
        }
    }

    fn word(self) -> String {
        match self {
            Self::Other(kind) => format!("kind{kind}"),
            _ => format!("{self:?}").to_lowercase(),
        }
    }
}

/// A register of a class in a register space: how a DXIL resource and an effect record meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register {
    pub class: ResourceClass,
    pub space: u32,
    pub index: u32,
}

/// What the caller knows of a program's resources beyond the program itself; empty, a binding is named by its register (`t3_space1`).
#[derive(Debug, Clone, Default)]
pub struct Naming {
    pub resources: HashMap<Register, String>,
    /// By register; a buffer's size comes from here where the bitcode states none.
    pub constant_buffers: HashMap<Register, ConstantBuffer>,
    /// A name by class and register alone, for a record that states no space: used when exactly one space holds that register.
    pub by_register: HashMap<(ResourceClass, u32), String>,
}

/// A program translated: the module, what it binds and reads, and how faithful it is.
#[derive(Debug, Clone)]
pub struct Translated {
    pub stage: ShaderStageKind,
    pub module: naga::Module,
    /// `constant_buffers` holds keys the caller assigns: [`Translated::constant_buffers`] in order.
    pub reflection: Reflection,
    pub constant_buffers: Vec<ConstantBuffer>,
    pub fidelity: Fidelity,
    /// Each lowering that changed what the program does, by name and count (16-bit to 32-bit …).
    pub lowerings: BTreeMap<String, u32>,
}

/// Why a program is not translated; [`Refusal::name`] is the short stable word a census counts by.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error(transparent)]
    Container(#[from] container::ContainerError),
    #[error(transparent)]
    Bitcode(#[from] bitstream::BitcodeError),
    #[error("the program is a {0:?} stage, which this front end does not draw")]
    Stage(ShaderKind),
    #[error(transparent)]
    Plan(#[from] plan::PlanError),
    #[error(transparent)]
    Structure(#[from] structure::StructureError),
    #[error(transparent)]
    Translate(#[from] to_naga::TranslateError),
    #[error("naga's validator refuses the module: {0}")]
    Invalid(String),
}

impl Refusal {
    /// `no-bitcode`, `stage:mesh`, `op:WaveActiveOp`, `cfg:irreducible` …
    pub fn name(&self) -> String {
        use bitstream::BitcodeError as B;
        use container::{ContainerError as C, PsvResourceKind};
        use plan::PlanError as P;
        use structure::StructureError as S;
        use to_naga::TranslateError as T;

        match self {
            Self::Container(error) => match error {
                C::NoBitcode { .. } => "no-bitcode".into(),
                C::NativeCode { stage, .. } => format!("native-code:{stage}"),
                C::Xdat(error) => format!("xdat:{}", error.rule),
                C::NotDxbc => "container:not-dxbc".into(),
                C::Truncated { .. } => "container:truncated".into(),
                C::NoPart(part) => format!("container:no-part:{part}"),
                C::Malformed { part, .. } => format!("container:malformed:{part}"),
            },
            Self::Bitcode(error) => match error {
                B::NoMagic => "bitcode:no-magic".into(),
                B::Truncated { .. } => "bitcode:truncated".into(),
                B::NoAbbrev { .. } => "bitcode:no-abbrev".into(),
                B::TooDeep(_) => "bitcode:too-deep".into(),
                B::Record { .. } => "bitcode:record".into(),
                B::Unsupported(_) => "bitcode:unsupported".into(),
            },
            Self::Stage(kind) => format!("stage:{}", kind.word()),
            Self::Plan(error) => match error {
                P::Range { .. } => "range".into(),
                P::NoSize(_) => "cbuffer:no-size".into(),
                P::Kind(PsvResourceKind::UavStructuredWithCounter) => "kind:uav-counter".into(),
                P::Kind(kind) => format!("kind:{kind:?}").to_lowercase(),
            },
            Self::Structure(error) => match error {
                S::Irreducible(_) => "cfg:irreducible".into(),
                S::Exit { .. } => "cfg:exit".into(),
                S::Fallthrough(_) => "cfg:fallthrough".into(),
                S::Shape(_) => "cfg:shape".into(),
            },
            Self::Translate(error) => match error {
                T::Op(op) => format!("op:{op}"),
                T::NotConstant { op, .. } => format!("not-constant:{op}"),
                T::Type(ty) => format!("type:{ty}"),
                T::Inst(inst) => format!("inst:{inst}"),
                T::Call(callee) => format!("call:{callee}"),
                T::Heap { .. } => "heap".into(),
                T::Unbound { .. } => "unbound".into(),
            },
            Self::Invalid(_) => "invalid".into(),
        }
    }
}

/// Translate one DXBC container holding a DXIL part.
pub fn translate(container: &[u8], naming: &Naming) -> Result<Translated, Refusal> {
    let parts = container::Container::read(container)?;
    let dxil = parts.dxil()?;
    let kind = dxil.header.kind;
    let stage = match kind {
        ShaderKind::Vertex | ShaderKind::Pixel | ShaderKind::Compute => kind.stage(),
        _ => None,
    }
    .ok_or(Refusal::Stage(kind))?;

    let blocks = bitstream::read(dxil.bitcode)?;
    let module = read::module(&blocks)?;
    let decls = metadata::dx_resources(&module)?;
    let psv = parts.psv()?;
    let mut plan = plan::BindingPlan::new(kind, &psv, &decls, naming)?;

    let body = module
        .entry()
        .and_then(|function| function.body.as_ref())
        .ok_or_else(|| {
            bitstream::BitcodeError::Unsupported(
                "a module whose dx.entryPoints names no function with a body".into(),
            )
        })?;
    let nodes = structure::structure(body)?;

    let inputs = parts.inputs()?;
    let outputs = parts.outputs()?;
    let interface = to_naga::Interface {
        kind,
        inputs: inputs.clone(),
        outputs: outputs.clone(),
        threads: psv.threads.or_else(|| metadata::entry_threads(&module)),
    };
    let lowered = to_naga::to_naga(&module, body, &nodes, &interface, &mut plan)?;

    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&lowered.module)
    .map_err(|error| Refusal::Invalid(error.to_string()))?;

    let constant_buffers = plan
        .bindings
        .iter()
        .filter(|binding| binding.register.class == ResourceClass::Cbv)
        .map(|binding| {
            naming
                .constant_buffers
                .get(&binding.register)
                .cloned()
                .unwrap_or_else(|| ConstantBuffer {
                    name: binding.name.clone(),
                    size: match binding.planned {
                        plan::Planned::Uniform { rows } => rows.saturating_mul(16),
                        _ => 0,
                    },
                    variables: Vec::new(),
                })
        })
        .collect();

    Ok(Translated {
        stage,
        module: lowered.module,
        reflection: Reflection {
            bindings: plan.reflection(),
            constant_buffers: Vec::new(),
            inputs,
            outputs,
        },
        constant_buffers,
        fidelity: lowered.fidelity,
        lowerings: lowered.lowerings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_version_names_its_kind_in_the_high_half() {
        let cases = [
            (0, ShaderKind::Pixel, Some(ShaderStageKind::Pixel)),
            (1, ShaderKind::Vertex, Some(ShaderStageKind::Vertex)),
            (2, ShaderKind::Geometry, Some(ShaderStageKind::Geometry)),
            (3, ShaderKind::Hull, Some(ShaderStageKind::Hull)),
            (4, ShaderKind::Domain, Some(ShaderStageKind::Domain)),
            (5, ShaderKind::Compute, Some(ShaderStageKind::Compute)),
            (6, ShaderKind::Library, None),
            (13, ShaderKind::Mesh, None),
            (14, ShaderKind::Amplification, None),
            (40, ShaderKind::Other(40), None),
        ];
        for (raw, kind, stage) in cases {
            // A shader model in the low half must not change the kind.
            assert_eq!(ShaderKind::from_program_version((raw << 16) | 0x66), kind);
            assert_eq!(ShaderKind::from_program_version(raw << 16), kind);
            assert_eq!(kind.stage(), stage);
        }
    }

    #[test]
    fn each_refusal_has_its_census_word() {
        let cases = [
            (
                Refusal::Container(container::ContainerError::NoBitcode {
                    magic: "QZPK".into(),
                }),
                "no-bitcode",
            ),
            (
                Refusal::Container(container::ContainerError::NativeCode {
                    stage: ShaderKind::Mesh.word(),
                    ranges: 1,
                    code_bytes: 0x2C0,
                }),
                "native-code:mesh",
            ),
            (
                Refusal::Container(container::ContainerError::Xdat(
                    or_io::gta5::shaders::xdat::XdatError {
                        rule: "B3",
                        why: "a short stream".into(),
                    },
                )),
                "xdat:B3",
            ),
            (
                Refusal::Container(container::ContainerError::NoPart("DXIL")),
                "container:no-part:DXIL",
            ),
            (Refusal::Stage(ShaderKind::Mesh), "stage:mesh"),
            (Refusal::Stage(ShaderKind::Geometry), "stage:geometry"),
            (Refusal::Stage(ShaderKind::Other(9)), "stage:kind9"),
            (
                Refusal::Plan(plan::PlanError::Kind(
                    container::PsvResourceKind::UavStructuredWithCounter,
                )),
                "kind:uav-counter",
            ),
            (
                Refusal::Plan(plan::PlanError::Range {
                    kind: container::PsvResourceKind::SrvTyped,
                    space: 2,
                    lower: 7,
                    count: 4,
                }),
                "range",
            ),
            (
                Refusal::Structure(structure::StructureError::Irreducible(3)),
                "cfg:irreducible",
            ),
            (
                Refusal::Structure(structure::StructureError::Exit { from: 1, to: 5 }),
                "cfg:exit",
            ),
            (
                Refusal::Translate(to_naga::TranslateError::Op("WaveActiveOp".into())),
                "op:WaveActiveOp",
            ),
            (
                Refusal::Translate(to_naga::TranslateError::Heap {
                    op: "CreateHandleFromHeap".into(),
                }),
                "heap",
            ),
            (Refusal::Invalid("anything".into()), "invalid"),
        ];
        for (refusal, word) in cases {
            assert_eq!(refusal.name(), word, "{refusal}");
        }
    }

    #[test]
    fn bytes_that_are_no_container_are_refused_at_the_first_step() {
        let bytes: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(37) ^ 0x5A).collect();
        let refusal = translate(&bytes, &Naming::default()).unwrap_err();
        assert_eq!(
            refusal,
            Refusal::Container(container::ContainerError::NotDxbc)
        );
        assert_eq!(refusal.name(), "container:not-dxbc");
    }
}
