//! The `dx.*` named metadata: the resource ranges a shader model 6.0 to 6.5 program binds by range
//! id, and the entry point's thread group size. Field numbers are the public DXIL documentation's.

use super::bitstream::BitcodeError;
use super::ir::{Constant, Md, MdId, Module, ValueKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceShape {
    Texture1D,
    Texture1DArray,
    Texture2D,
    Texture2DArray,
    Texture2DMS,
    Texture2DMSArray,
    Texture3D,
    TextureCube,
    TextureCubeArray,
    TypedBuffer,
    RawBuffer,
    StructuredBuffer,
    Other(u32),
}

impl ResourceShape {
    fn from_kind(kind: u32) -> Self {
        match kind {
            1 => Self::Texture1D,
            2 => Self::Texture2D,
            3 => Self::Texture2DMS,
            4 => Self::Texture3D,
            5 => Self::TextureCube,
            6 => Self::Texture1DArray,
            7 => Self::Texture2DArray,
            8 => Self::Texture2DMSArray,
            9 => Self::TextureCubeArray,
            10 => Self::TypedBuffer,
            11 => Self::RawBuffer,
            12 => Self::StructuredBuffer,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentType {
    F16,
    F32,
    I16,
    I32,
    U16,
    U32,
    F64,
    I64,
    U64,
    UNorm,
    SNorm,
    Other(u32),
}

impl ComponentType {
    /// `Other(0)` where the record states no element type, as a raw or structured buffer does.
    fn from_raw(raw: u32) -> Self {
        match raw {
            2 => Self::I16,
            3 => Self::U16,
            4 => Self::I32,
            5 => Self::U32,
            6 => Self::I64,
            7 => Self::U64,
            8 => Self::F16,
            9 => Self::F32,
            10 => Self::F64,
            11 | 13 | 15 => Self::SNorm,
            12 | 14 | 16 => Self::UNorm,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDecl {
    pub id: u32,
    pub name: String,
    pub space: u32,
    pub lower: u32,
    /// 1 for one register; `u32::MAX` unbounded.
    pub count: u32,
    pub shape: ResourceShape,
    pub component: ComponentType,
    /// A structured buffer's stride in bytes.
    pub stride: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CbufferDecl {
    pub id: u32,
    pub name: String,
    pub space: u32,
    pub lower: u32,
    pub count: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamplerDecl {
    pub id: u32,
    pub name: String,
    pub space: u32,
    pub lower: u32,
    pub count: u32,
    pub comparison: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DxResources {
    pub srvs: Vec<ResourceDecl>,
    pub uavs: Vec<ResourceDecl>,
    pub cbuffers: Vec<CbufferDecl>,
    pub samplers: Vec<SamplerDecl>,
}

/// The extended-metadata tags of an SRV or UAV record.
const TAG_ELEMENT_TYPE: u32 = 0;
const TAG_STRUCTURED_STRIDE: u32 = 1;
/// The entry point property tag whose value is the thread group size.
const TAG_NUM_THREADS: u32 = 4;
const SAMPLER_COMPARISON: u32 = 1;

/// `dx.resources`: what each resource range is. Empty lists where the module has none.
pub fn dx_resources(module: &Module) -> Result<DxResources, BitcodeError> {
    // DXC names the list both ways; the entry point's operand is the fallback.
    let list = match module.named.get("dx.resources") {
        Some(nodes) => nodes.first().copied(),
        None => entry_operand(module, 3),
    };
    let mut out = DxResources::default();
    let Some(list) = list else {
        return Ok(out);
    };
    let classes = Fields::of(module, Some(list), "dx.resources list")?;
    if classes.ops.len() > 4 {
        return Err(malformed(
            "dx.resources list",
            "operands are more than four classes",
        ));
    }
    const WHAT: [&str; 4] = ["SRV record", "UAV record", "CBV record", "sampler record"];
    for (class, records) in classes.ops.iter().enumerate() {
        let Some(records) = *records else {
            continue;
        };
        for &record in Fields::of(module, Some(records), WHAT[class])?.ops {
            let r = Fields::of(module, record, WHAT[class])?;
            let (id, name) = (r.int(0)?, r.string(2)?);
            let (space, lower, count) = (r.int(3)?, r.int(4)?, r.int(5)?);
            match class {
                0 | 1 => {
                    let extended = if class == 0 { 8 } else { 10 };
                    let component = r.tag(extended, TAG_ELEMENT_TYPE)?.unwrap_or(0);
                    let decl = ResourceDecl {
                        id,
                        name,
                        space,
                        lower,
                        count,
                        shape: ResourceShape::from_kind(r.int(6)?),
                        component: ComponentType::from_raw(component),
                        stride: r.tag(extended, TAG_STRUCTURED_STRIDE)?,
                    };
                    if class == 0 {
                        out.srvs.push(decl);
                    } else {
                        out.uavs.push(decl);
                    }
                }
                2 => {
                    let size = r.int(6)?;
                    out.cbuffers.push(CbufferDecl {
                        id,
                        name,
                        space,
                        lower,
                        count,
                        size,
                    });
                }
                _ => out.samplers.push(SamplerDecl {
                    id,
                    name,
                    space,
                    lower,
                    count,
                    comparison: r.int(6)? == SAMPLER_COMPARISON,
                }),
            }
        }
    }
    Ok(out)
}

/// The thread group size `dx.entryPoints` states, which a compute entry point carries.
pub fn entry_threads(module: &Module) -> Option<[u32; 3]> {
    let properties = entry_operand(module, 4);
    let properties = Fields::of(module, properties, "entry point properties").ok()?;
    let value = properties.tag_operand(TAG_NUM_THREADS).ok()??;
    let threads = Fields::of(module, Some(value), "thread group size").ok()?;
    Some([
        threads.int(0).ok()?,
        threads.int(1).ok()?,
        threads.int(2).ok()?,
    ])
}

/// Operand `index` of the first `dx.entryPoints` node.
fn entry_operand(module: &Module, index: usize) -> Option<MdId> {
    let node = *module.named.get("dx.entryPoints")?.first()?;
    match module.metadata.get(node as usize)? {
        Md::Node(ops) => ops.get(index).copied().flatten(),
        _ => None,
    }
}

fn malformed(what: &str, why: &str) -> BitcodeError {
    BitcodeError::Unsupported(format!("a {what} whose {why}"))
}

/// The integer a metadata value holds; the bitcode writes a zero constant as null.
fn md_int(module: &Module, id: MdId) -> Option<i64> {
    let Md::Value { value, .. } = module.metadata.get(id as usize)? else {
        return None;
    };
    match &module.values.get(*value as usize)?.kind {
        ValueKind::Constant(Constant::Int(value)) => Some(*value),
        ValueKind::Constant(Constant::Null) => Some(0),
        _ => None,
    }
}

/// An `i32` read back sign-extended or not: -1 and 0xFFFFFFFF are both `u32::MAX`.
fn to_u32(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .or_else(|| i32::try_from(value).ok().map(|v| v as u32))
}

/// A metadata node's operands, with the record name an error reports.
struct Fields<'a> {
    module: &'a Module,
    what: &'static str,
    ops: &'a [Option<MdId>],
}

impl<'a> Fields<'a> {
    fn of(module: &'a Module, id: Option<MdId>, what: &'static str) -> Result<Self, BitcodeError> {
        match id.and_then(|id| module.metadata.get(id as usize)) {
            Some(Md::Node(ops)) => Ok(Self { module, what, ops }),
            _ => Err(malformed(what, "operand is not a metadata node")),
        }
    }

    fn int(&self, field: usize) -> Result<u32, BitcodeError> {
        self.ops
            .get(field)
            .copied()
            .flatten()
            .and_then(|id| md_int(self.module, id))
            .and_then(to_u32)
            .ok_or_else(|| malformed(self.what, &format!("field {field} is no 32-bit integer")))
    }

    /// An absent or null name is empty.
    fn string(&self, field: usize) -> Result<String, BitcodeError> {
        let md = self.ops.get(field).copied().flatten();
        match md.map(|id| self.module.metadata.get(id as usize)) {
            None => Ok(String::new()),
            Some(Some(Md::String(bytes))) => Ok(String::from_utf8_lossy(bytes).into_owned()),
            Some(_) => Err(malformed(self.what, &format!("field {field} is no string"))),
        }
    }

    /// The value operand that follows `tag` in this tag and value list.
    fn tag_operand(&self, tag: u32) -> Result<Option<MdId>, BitcodeError> {
        let (pairs, odd) = self.ops.as_chunks::<2>();
        if !odd.is_empty() {
            return Err(malformed(
                self.what,
                "tag list has an odd number of operands",
            ));
        }
        for pair in pairs {
            let key = pair[0]
                .and_then(|id| md_int(self.module, id))
                .and_then(to_u32);
            match key {
                Some(key) if key == tag => return Ok(pair[1]),
                Some(_) => {}
                None => return Err(malformed(self.what, "tag list holds a non-integer tag")),
            }
        }
        Ok(None)
    }

    /// The integer `tag` states in the tag list at `field`; `None` where either is absent.
    fn tag(&self, field: usize, tag: u32) -> Result<Option<u32>, BitcodeError> {
        let Some(list) = self.ops.get(field).copied().flatten() else {
            return Ok(None);
        };
        let list = Fields::of(self.module, Some(list), self.what)?;
        let Some(value) = list.tag_operand(tag)? else {
            return Ok(None);
        };
        md_int(self.module, value)
            .and_then(to_u32)
            .map(Some)
            .ok_or_else(|| malformed(self.what, &format!("tag {tag} holds no 32-bit integer")))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ir::Value;
    use super::*;

    #[derive(Default)]
    struct Build {
        module: Module,
    }

    impl Build {
        fn md(&mut self, md: Md) -> Option<MdId> {
            self.module.metadata.push(md);
            Some(self.module.metadata.len() as MdId - 1)
        }

        fn constant(&mut self, constant: Constant) -> Option<MdId> {
            let kind = ValueKind::Constant(constant);
            self.module.values.push(Value {
                ty: 0,
                kind,
                name: None,
            });
            let value = self.module.values.len() as u32 - 1;
            self.md(Md::Value { ty: 0, value })
        }

        fn int(&mut self, value: i64) -> Option<MdId> {
            self.constant(Constant::Int(value))
        }

        fn string(&mut self, text: &str) -> Option<MdId> {
            self.md(Md::String(text.as_bytes().to_vec()))
        }

        fn node(&mut self, ops: Vec<Option<MdId>>) -> Option<MdId> {
            self.md(Md::Node(ops))
        }

        /// The fields every record opens with: id, symbol, name, space, lower, count.
        fn head(&mut self, id: i64, name: &str, at: [i64; 3]) -> Vec<Option<MdId>> {
            // Id 0 is written as a null constant, as the bitcode writes every zero.
            let id = if id == 0 {
                self.constant(Constant::Null)
            } else {
                self.int(id)
            };
            let symbol = self.constant(Constant::Undef);
            let name = self.string(name);
            vec![
                id,
                symbol,
                name,
                self.int(at[0]),
                self.int(at[1]),
                self.int(at[2]),
            ]
        }

        fn named(&mut self, name: &str, node: Option<MdId>) {
            self.module
                .named
                .insert(name.to_owned(), node.into_iter().collect());
        }
    }

    /// Two SRVs, one UAV, one CBV and two samplers, every field distinct.
    fn resources(build: &mut Build) -> Option<MdId> {
        let mut texture = build.head(0, "gDiffuse", [2, 5, 1]);
        let (kind, samples) = (build.int(2), build.int(0));
        let (tag, float) = (build.int(0), build.int(9));
        let extended = build.node(vec![tag, float]);
        texture.extend([kind, samples, extended]);
        let texture = build.node(texture);

        // An unbounded range, its count written sign-extended.
        let mut structured = build.head(1, "gInstances", [0, 9, -1]);
        let (kind, samples) = (build.int(12), build.int(0));
        let (tag, stride) = (build.int(1), build.int(48));
        let extended = build.node(vec![tag, stride]);
        structured.extend([kind, samples, extended]);
        let structured = build.node(structured);

        let mut uav = build.head(7, "gOut", [3, 1, 4]);
        let kind = build.int(10);
        let (coherent, counter, rov) = (build.int(0), build.int(0), build.int(-1));
        let (tag, uint) = (build.int(0), build.int(5));
        let extended = build.node(vec![tag, uint]);
        uav.extend([kind, coherent, counter, rov, extended]);
        let uav = build.node(uav);

        let mut cbv = build.head(3, "", [1, 4, 1]);
        let size = build.int(256);
        cbv.extend([size, None]);
        let cbv = build.node(cbv);

        let mut shadow = build.head(4, "gShadowSampler", [6, 2, 1]);
        let comparison = build.int(1);
        shadow.extend([comparison, None]);
        let shadow = build.node(shadow);
        let mut linear = build.head(5, "gLinear", [0, 0, 2]);
        let default = build.int(0);
        linear.extend([default, None]);
        let linear = build.node(linear);

        let srvs = build.node(vec![texture, structured]);
        let uavs = build.node(vec![uav]);
        let cbvs = build.node(vec![cbv]);
        let samplers = build.node(vec![shadow, linear]);
        build.node(vec![srvs, uavs, cbvs, samplers])
    }

    fn expected() -> DxResources {
        DxResources {
            srvs: vec![
                ResourceDecl {
                    id: 0,
                    name: "gDiffuse".into(),
                    space: 2,
                    lower: 5,
                    count: 1,
                    shape: ResourceShape::Texture2D,
                    component: ComponentType::F32,
                    stride: None,
                },
                ResourceDecl {
                    id: 1,
                    name: "gInstances".into(),
                    space: 0,
                    lower: 9,
                    count: u32::MAX,
                    shape: ResourceShape::StructuredBuffer,
                    component: ComponentType::Other(0),
                    stride: Some(48),
                },
            ],
            uavs: vec![ResourceDecl {
                id: 7,
                name: "gOut".into(),
                space: 3,
                lower: 1,
                count: 4,
                shape: ResourceShape::TypedBuffer,
                component: ComponentType::U32,
                stride: None,
            }],
            cbuffers: vec![CbufferDecl {
                id: 3,
                name: String::new(),
                space: 1,
                lower: 4,
                count: 1,
                size: 256,
            }],
            samplers: vec![
                SamplerDecl {
                    id: 4,
                    name: "gShadowSampler".into(),
                    space: 6,
                    lower: 2,
                    count: 1,
                    comparison: true,
                },
                SamplerDecl {
                    id: 5,
                    name: "gLinear".into(),
                    space: 0,
                    lower: 0,
                    count: 2,
                    comparison: false,
                },
            ],
        }
    }

    #[test]
    fn every_class_of_record_reads_its_fields() {
        let mut build = Build::default();
        let list = resources(&mut build);
        build.named("dx.resources", list);
        assert_eq!(dx_resources(&build.module), Ok(expected()));
    }

    #[test]
    fn the_entry_points_list_stands_in_for_a_missing_dx_resources() {
        let mut build = Build::default();
        assert_eq!(dx_resources(&build.module), Ok(DxResources::default()));

        let list = resources(&mut build);
        let (function, name) = (build.constant(Constant::Undef), build.string("main"));
        let entry = build.node(vec![function, name, None, list, None]);
        build.named("dx.entryPoints", entry);
        assert_eq!(dx_resources(&build.module), Ok(expected()));

        let empty = build.node(vec![function, name, None, None, None]);
        build.named("dx.entryPoints", empty);
        assert_eq!(dx_resources(&build.module), Ok(DxResources::default()));
    }

    #[test]
    fn a_record_that_breaks_its_layout_is_an_error() {
        let mut build = Build::default();
        let mut cbv = build.head(1, "cb", [0, 0, 1]);
        // The size field holds a string where an integer belongs.
        let size = build.string("256");
        cbv.extend([size, None]);
        let cbv = build.node(cbv);
        let cbvs = build.node(vec![cbv]);
        let list = build.node(vec![None, None, cbvs, None]);
        build.named("dx.resources", list);
        assert!(matches!(
            dx_resources(&build.module),
            Err(BitcodeError::Unsupported(_))
        ));

        // A lower bound past 32 bits.
        let mut build = Build::default();
        let mut sampler = build.head(1, "s", [0, 1 << 33, 1]);
        let default = build.int(0);
        sampler.extend([default, None]);
        let sampler = build.node(sampler);
        let samplers = build.node(vec![sampler]);
        let list = build.node(vec![None, None, None, samplers]);
        build.named("dx.resources", list);
        assert!(matches!(
            dx_resources(&build.module),
            Err(BitcodeError::Unsupported(_))
        ));

        // A list that is no node, and a record index past the table.
        let mut build = Build::default();
        let not_a_node = build.int(4);
        build.named("dx.resources", not_a_node);
        assert!(dx_resources(&build.module).is_err());
        let srvs = build.node(vec![Some(999)]);
        let list = build.node(vec![srvs]);
        build.named("dx.resources", list);
        assert!(dx_resources(&build.module).is_err());
    }

    #[test]
    fn shapes_and_component_types_follow_the_public_numbers() {
        assert_eq!(ResourceShape::from_kind(6), ResourceShape::Texture1DArray);
        assert_eq!(ResourceShape::from_kind(9), ResourceShape::TextureCubeArray);
        assert_eq!(ResourceShape::from_kind(11), ResourceShape::RawBuffer);
        assert_eq!(ResourceShape::from_kind(13), ResourceShape::Other(13));
        assert_eq!(ResourceShape::from_kind(0), ResourceShape::Other(0));
        assert_eq!(ComponentType::from_raw(3), ComponentType::U16);
        assert_eq!(ComponentType::from_raw(8), ComponentType::F16);
        assert_eq!(ComponentType::from_raw(13), ComponentType::SNorm);
        assert_eq!(ComponentType::from_raw(14), ComponentType::UNorm);
        assert_eq!(ComponentType::from_raw(1), ComponentType::Other(1));
        assert_eq!(ComponentType::from_raw(17), ComponentType::Other(17));
    }

    #[test]
    fn entry_threads_reads_the_num_threads_property() {
        let mut build = Build::default();
        assert_eq!(entry_threads(&build.module), None);

        let (flags_tag, flags) = (build.int(0), build.int(0x1_0000_0010));
        let (threads_tag, x, y, z) = (build.int(4), build.int(8), build.int(4), build.int(2));
        let size = build.node(vec![x, y, z]);
        let properties = build.node(vec![flags_tag, flags, threads_tag, size]);
        let (function, name) = (build.constant(Constant::Undef), build.string("main"));
        let entry = build.node(vec![function, name, None, None, properties]);
        build.named("dx.entryPoints", entry);
        assert_eq!(entry_threads(&build.module), Some([8, 4, 2]));

        let flags_only = build.node(vec![flags_tag, flags]);
        let entry = build.node(vec![function, name, None, None, flags_only]);
        build.named("dx.entryPoints", entry);
        assert_eq!(entry_threads(&build.module), None);

        let odd = build.node(vec![threads_tag]);
        let entry = build.node(vec![function, name, None, None, odd]);
        build.named("dx.entryPoints", entry);
        assert_eq!(entry_threads(&build.module), None);
    }
}
