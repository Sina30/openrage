//! The LLVM 3.7 subset DXIL uses, with dx.op calls as their own instruction: types and lookups only.

use std::borrow::Cow;
use std::collections::HashMap;

pub type TypeId = u32;
pub type ValueId = u32;
pub type BlockId = u32;
pub type MdId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Void,
    Label,
    Metadata,
    Half,
    Float,
    Double,
    Int(u32),
    Pointer {
        pointee: TypeId,
        space: u32,
    },
    Array {
        len: u64,
        elem: TypeId,
    },
    Vector {
        len: u32,
        elem: TypeId,
    },
    Struct {
        name: Option<String>,
        fields: Vec<TypeId>,
        packed: bool,
    },
    Function {
        ret: TypeId,
        params: Vec<TypeId>,
        vararg: bool,
    },
    Opaque(Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Undef,
    Null,
    Int(i64),
    /// IEEE bits, widened: a half or a float in the low bits.
    Float(u64),
    Aggregate(Vec<ValueId>),
    Data(Vec<u64>),
    String(Vec<u8>),
    Cast {
        op: CastOp,
        value: ValueId,
    },
    Gep {
        inbounds: bool,
        operands: Vec<ValueId>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValueKind {
    Global {
        space: u32,
        init: Option<ValueId>,
        constant: bool,
    },
    /// The function at this index of [`Module::functions`].
    Function(u32),
    Constant(Constant),
    Argument(u32),
    /// The result of instruction `index` of block `block` of the function being read.
    Inst {
        block: BlockId,
        index: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub ty: TypeId,
    pub kind: ValueKind,
    pub name: Option<String>,
}

/// A dx.op opcode, the first operand of a `dx.op.*` call: numbers from the public DXIL documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DxOp(pub u32);

macro_rules! dx_ops {
    ($($number:literal $constant:ident $name:literal,)*) => {
        impl DxOp {
            $(pub const $constant: Self = Self($number);)*
        }

        /// Every opcode with its public name, in opcode order.
        const DX_OPS: &[(DxOp, &str)] = &[$((DxOp::$constant, $name),)*];
    };
}

dx_ops! {
    0 TEMP_REG_LOAD "TempRegLoad",
    1 TEMP_REG_STORE "TempRegStore",
    2 MIN_PREC_X_REG_LOAD "MinPrecXRegLoad",
    3 MIN_PREC_X_REG_STORE "MinPrecXRegStore",
    4 LOAD_INPUT "LoadInput",
    5 STORE_OUTPUT "StoreOutput",
    6 FABS "FAbs",
    7 SATURATE "Saturate",
    8 IS_NAN "IsNaN",
    9 IS_INF "IsInf",
    10 IS_FINITE "IsFinite",
    11 IS_NORMAL "IsNormal",
    12 COS "Cos",
    13 SIN "Sin",
    14 TAN "Tan",
    15 ACOS "Acos",
    16 ASIN "Asin",
    17 ATAN "Atan",
    18 HCOS "Hcos",
    19 HSIN "Hsin",
    20 HTAN "Htan",
    21 EXP "Exp",
    22 FRC "Frc",
    23 LOG "Log",
    24 SQRT "Sqrt",
    25 RSQRT "Rsqrt",
    26 ROUND_NE "Round_ne",
    27 ROUND_NI "Round_ni",
    28 ROUND_PI "Round_pi",
    29 ROUND_Z "Round_z",
    30 BFREV "Bfrev",
    31 COUNTBITS "Countbits",
    32 FIRSTBIT_LO "FirstbitLo",
    33 FIRSTBIT_HI "FirstbitHi",
    34 FIRSTBIT_SHI "FirstbitSHi",
    35 FMAX "FMax",
    36 FMIN "FMin",
    37 IMAX "IMax",
    38 IMIN "IMin",
    39 UMAX "UMax",
    40 UMIN "UMin",
    41 IMUL "IMul",
    42 UMUL "UMul",
    43 UDIV "UDiv",
    44 UADDC "UAddc",
    45 USUBB "USubb",
    46 FMAD "FMad",
    47 FMA "Fma",
    48 IMAD "IMad",
    49 UMAD "UMad",
    50 MSAD "Msad",
    51 IBFE "Ibfe",
    52 UBFE "Ubfe",
    53 BFI "Bfi",
    54 DOT2 "Dot2",
    55 DOT3 "Dot3",
    56 DOT4 "Dot4",
    57 CREATE_HANDLE "CreateHandle",
    58 CBUFFER_LOAD "CBufferLoad",
    59 CBUFFER_LOAD_LEGACY "CBufferLoadLegacy",
    60 SAMPLE "Sample",
    61 SAMPLE_BIAS "SampleBias",
    62 SAMPLE_LEVEL "SampleLevel",
    63 SAMPLE_GRAD "SampleGrad",
    64 SAMPLE_CMP "SampleCmp",
    65 SAMPLE_CMP_LEVEL_ZERO "SampleCmpLevelZero",
    66 TEXTURE_LOAD "TextureLoad",
    67 TEXTURE_STORE "TextureStore",
    68 BUFFER_LOAD "BufferLoad",
    69 BUFFER_STORE "BufferStore",
    70 BUFFER_UPDATE_COUNTER "BufferUpdateCounter",
    71 CHECK_ACCESS_FULLY_MAPPED "CheckAccessFullyMapped",
    72 GET_DIMENSIONS "GetDimensions",
    73 TEXTURE_GATHER "TextureGather",
    74 TEXTURE_GATHER_CMP "TextureGatherCmp",
    75 TEXTURE_2DMS_GET_SAMPLE_POSITION "Texture2DMSGetSamplePosition",
    76 RENDER_TARGET_GET_SAMPLE_POSITION "RenderTargetGetSamplePosition",
    77 RENDER_TARGET_GET_SAMPLE_COUNT "RenderTargetGetSampleCount",
    78 ATOMIC_BIN_OP "AtomicBinOp",
    79 ATOMIC_COMPARE_EXCHANGE "AtomicCompareExchange",
    80 BARRIER "Barrier",
    81 CALCULATE_LOD "CalculateLOD",
    82 DISCARD "Discard",
    83 DERIV_COARSE_X "DerivCoarseX",
    84 DERIV_COARSE_Y "DerivCoarseY",
    85 DERIV_FINE_X "DerivFineX",
    86 DERIV_FINE_Y "DerivFineY",
    87 EVAL_SNAPPED "EvalSnapped",
    88 EVAL_SAMPLE_INDEX "EvalSampleIndex",
    89 EVAL_CENTROID "EvalCentroid",
    90 SAMPLE_INDEX "SampleIndex",
    91 COVERAGE "Coverage",
    92 INNER_COVERAGE "InnerCoverage",
    93 THREAD_ID "ThreadId",
    94 GROUP_ID "GroupId",
    95 THREAD_ID_IN_GROUP "ThreadIdInGroup",
    96 FLATTENED_THREAD_ID_IN_GROUP "FlattenedThreadIdInGroup",
    97 EMIT_STREAM "EmitStream",
    98 CUT_STREAM "CutStream",
    99 EMIT_THEN_CUT_STREAM "EmitThenCutStream",
    100 GS_INSTANCE_ID "GSInstanceID",
    101 MAKE_DOUBLE "MakeDouble",
    102 SPLIT_DOUBLE "SplitDouble",
    103 LOAD_OUTPUT_CONTROL_POINT "LoadOutputControlPoint",
    104 LOAD_PATCH_CONSTANT "LoadPatchConstant",
    105 DOMAIN_LOCATION "DomainLocation",
    106 STORE_PATCH_CONSTANT "StorePatchConstant",
    107 OUTPUT_CONTROL_POINT_ID "OutputControlPointID",
    108 PRIMITIVE_ID "PrimitiveID",
    109 CYCLE_COUNTER_LEGACY "CycleCounterLegacy",
    110 WAVE_IS_FIRST_LANE "WaveIsFirstLane",
    111 WAVE_GET_LANE_INDEX "WaveGetLaneIndex",
    112 WAVE_GET_LANE_COUNT "WaveGetLaneCount",
    113 WAVE_ANY_TRUE "WaveAnyTrue",
    114 WAVE_ALL_TRUE "WaveAllTrue",
    115 WAVE_ACTIVE_ALL_EQUAL "WaveActiveAllEqual",
    116 WAVE_ACTIVE_BALLOT "WaveActiveBallot",
    117 WAVE_READ_LANE_AT "WaveReadLaneAt",
    118 WAVE_READ_LANE_FIRST "WaveReadLaneFirst",
    119 WAVE_ACTIVE_OP "WaveActiveOp",
    120 WAVE_ACTIVE_BIT "WaveActiveBit",
    121 WAVE_PREFIX_OP "WavePrefixOp",
    122 QUAD_READ_LANE_AT "QuadReadLaneAt",
    123 QUAD_OP "QuadOp",
    124 BITCAST_I16_TO_F16 "BitcastI16toF16",
    125 BITCAST_F16_TO_I16 "BitcastF16toI16",
    126 BITCAST_I32_TO_F32 "BitcastI32toF32",
    127 BITCAST_F32_TO_I32 "BitcastF32toI32",
    128 BITCAST_I64_TO_F64 "BitcastI64toF64",
    129 BITCAST_F64_TO_I64 "BitcastF64toI64",
    130 LEGACY_F32_TO_F16 "LegacyF32ToF16",
    131 LEGACY_F16_TO_F32 "LegacyF16ToF32",
    132 LEGACY_DOUBLE_TO_FLOAT "LegacyDoubleToFloat",
    133 LEGACY_DOUBLE_TO_SINT32 "LegacyDoubleToSInt32",
    134 LEGACY_DOUBLE_TO_UINT32 "LegacyDoubleToUInt32",
    135 WAVE_ALL_BIT_COUNT "WaveAllBitCount",
    136 WAVE_PREFIX_BIT_COUNT "WavePrefixBitCount",
    137 ATTRIBUTE_AT_VERTEX "AttributeAtVertex",
    138 VIEW_ID "ViewID",
    139 RAW_BUFFER_LOAD "RawBufferLoad",
    140 RAW_BUFFER_STORE "RawBufferStore",
    141 INSTANCE_ID "InstanceID",
    142 INSTANCE_INDEX "InstanceIndex",
    143 HIT_KIND "HitKind",
    144 RAY_FLAGS "RayFlags",
    145 DISPATCH_RAYS_INDEX "DispatchRaysIndex",
    146 DISPATCH_RAYS_DIMENSIONS "DispatchRaysDimensions",
    147 WORLD_RAY_ORIGIN "WorldRayOrigin",
    148 WORLD_RAY_DIRECTION "WorldRayDirection",
    149 OBJECT_RAY_ORIGIN "ObjectRayOrigin",
    150 OBJECT_RAY_DIRECTION "ObjectRayDirection",
    151 OBJECT_TO_WORLD "ObjectToWorld",
    152 WORLD_TO_OBJECT "WorldToObject",
    153 RAY_TMIN "RayTMin",
    154 RAY_TCURRENT "RayTCurrent",
    155 IGNORE_HIT "IgnoreHit",
    156 ACCEPT_HIT_AND_END_SEARCH "AcceptHitAndEndSearch",
    157 TRACE_RAY "TraceRay",
    158 REPORT_HIT "ReportHit",
    159 CALL_SHADER "CallShader",
    160 CREATE_HANDLE_FOR_LIB "CreateHandleForLib",
    161 PRIMITIVE_INDEX "PrimitiveIndex",
    162 DOT2_ADD_HALF "Dot2AddHalf",
    163 DOT4_ADD_I8_PACKED "Dot4AddI8Packed",
    164 DOT4_ADD_U8_PACKED "Dot4AddU8Packed",
    165 WAVE_MATCH "WaveMatch",
    166 WAVE_MULTI_PREFIX_OP "WaveMultiPrefixOp",
    167 WAVE_MULTI_PREFIX_BIT_COUNT "WaveMultiPrefixBitCount",
    168 SET_MESH_OUTPUT_COUNTS "SetMeshOutputCounts",
    169 EMIT_INDICES "EmitIndices",
    170 GET_MESH_PAYLOAD "GetMeshPayload",
    171 STORE_VERTEX_OUTPUT "StoreVertexOutput",
    172 STORE_PRIMITIVE_OUTPUT "StorePrimitiveOutput",
    173 DISPATCH_MESH "DispatchMesh",
    174 WRITE_SAMPLER_FEEDBACK "WriteSamplerFeedback",
    175 WRITE_SAMPLER_FEEDBACK_BIAS "WriteSamplerFeedbackBias",
    176 WRITE_SAMPLER_FEEDBACK_LEVEL "WriteSamplerFeedbackLevel",
    177 WRITE_SAMPLER_FEEDBACK_GRAD "WriteSamplerFeedbackGrad",
    178 ALLOCATE_RAY_QUERY "AllocateRayQuery",
    179 RAY_QUERY_TRACE_RAY_INLINE "RayQuery_TraceRayInline",
    180 RAY_QUERY_PROCEED "RayQuery_Proceed",
    181 RAY_QUERY_ABORT "RayQuery_Abort",
    182 RAY_QUERY_COMMIT_NON_OPAQUE_TRIANGLE_HIT "RayQuery_CommitNonOpaqueTriangleHit",
    183 RAY_QUERY_COMMIT_PROCEDURAL_PRIMITIVE_HIT "RayQuery_CommitProceduralPrimitiveHit",
    184 RAY_QUERY_COMMITTED_STATUS "RayQuery_CommittedStatus",
    185 RAY_QUERY_CANDIDATE_TYPE "RayQuery_CandidateType",
    186 RAY_QUERY_CANDIDATE_OBJECT_TO_WORLD_3X4 "RayQuery_CandidateObjectToWorld3x4",
    187 RAY_QUERY_CANDIDATE_WORLD_TO_OBJECT_3X4 "RayQuery_CandidateWorldToObject3x4",
    188 RAY_QUERY_COMMITTED_OBJECT_TO_WORLD_3X4 "RayQuery_CommittedObjectToWorld3x4",
    189 RAY_QUERY_COMMITTED_WORLD_TO_OBJECT_3X4 "RayQuery_CommittedWorldToObject3x4",
    190 RAY_QUERY_CANDIDATE_PROCEDURAL_PRIMITIVE_NON_OPAQUE "RayQuery_CandidateProceduralPrimitiveNonOpaque",
    191 RAY_QUERY_CANDIDATE_TRIANGLE_FRONT_FACE "RayQuery_CandidateTriangleFrontFace",
    192 RAY_QUERY_COMMITTED_TRIANGLE_FRONT_FACE "RayQuery_CommittedTriangleFrontFace",
    193 RAY_QUERY_CANDIDATE_TRIANGLE_BARYCENTRICS "RayQuery_CandidateTriangleBarycentrics",
    194 RAY_QUERY_COMMITTED_TRIANGLE_BARYCENTRICS "RayQuery_CommittedTriangleBarycentrics",
    195 RAY_QUERY_RAY_FLAGS "RayQuery_RayFlags",
    196 RAY_QUERY_WORLD_RAY_ORIGIN "RayQuery_WorldRayOrigin",
    197 RAY_QUERY_WORLD_RAY_DIRECTION "RayQuery_WorldRayDirection",
    198 RAY_QUERY_RAY_TMIN "RayQuery_RayTMin",
    199 RAY_QUERY_CANDIDATE_TRIANGLE_RAY_T "RayQuery_CandidateTriangleRayT",
    200 RAY_QUERY_COMMITTED_RAY_T "RayQuery_CommittedRayT",
    201 RAY_QUERY_CANDIDATE_INSTANCE_INDEX "RayQuery_CandidateInstanceIndex",
    202 RAY_QUERY_CANDIDATE_INSTANCE_ID "RayQuery_CandidateInstanceID",
    203 RAY_QUERY_CANDIDATE_GEOMETRY_INDEX "RayQuery_CandidateGeometryIndex",
    204 RAY_QUERY_CANDIDATE_PRIMITIVE_INDEX "RayQuery_CandidatePrimitiveIndex",
    205 RAY_QUERY_CANDIDATE_OBJECT_RAY_ORIGIN "RayQuery_CandidateObjectRayOrigin",
    206 RAY_QUERY_CANDIDATE_OBJECT_RAY_DIRECTION "RayQuery_CandidateObjectRayDirection",
    207 RAY_QUERY_COMMITTED_INSTANCE_INDEX "RayQuery_CommittedInstanceIndex",
    208 RAY_QUERY_COMMITTED_INSTANCE_ID "RayQuery_CommittedInstanceID",
    209 RAY_QUERY_COMMITTED_GEOMETRY_INDEX "RayQuery_CommittedGeometryIndex",
    210 RAY_QUERY_COMMITTED_PRIMITIVE_INDEX "RayQuery_CommittedPrimitiveIndex",
    211 RAY_QUERY_COMMITTED_OBJECT_RAY_ORIGIN "RayQuery_CommittedObjectRayOrigin",
    212 RAY_QUERY_COMMITTED_OBJECT_RAY_DIRECTION "RayQuery_CommittedObjectRayDirection",
    213 GEOMETRY_INDEX "GeometryIndex",
    214 RAY_QUERY_CANDIDATE_INSTANCE_CONTRIBUTION_TO_HIT_GROUP_INDEX "RayQuery_CandidateInstanceContributionToHitGroupIndex",
    215 RAY_QUERY_COMMITTED_INSTANCE_CONTRIBUTION_TO_HIT_GROUP_INDEX "RayQuery_CommittedInstanceContributionToHitGroupIndex",
    216 ANNOTATE_HANDLE "AnnotateHandle",
    217 CREATE_HANDLE_FROM_BINDING "CreateHandleFromBinding",
    218 CREATE_HANDLE_FROM_HEAP "CreateHandleFromHeap",
    219 UNPACK_4X8 "Unpack4x8",
    220 PACK_4X8 "Pack4x8",
    221 IS_HELPER_LANE "IsHelperLane",
    222 QUAD_VOTE "QuadVote",
    223 TEXTURE_GATHER_RAW "TextureGatherRaw",
    224 SAMPLE_CMP_LEVEL "SampleCmpLevel",
    225 TEXTURE_STORE_SAMPLE "TextureStoreSample",
    226 WAVE_MATRIX_ANNOTATE "WaveMatrix_Annotate",
}

impl DxOp {
    /// The public name, such as `SampleLevel`; `op123` for a number the table does not hold.
    pub fn name(self) -> Cow<'static, str> {
        match DX_OPS.get(self.0 as usize) {
            Some(&(op, name)) if op == self => Cow::Borrowed(name),
            _ => Cow::Owned(format!("op{}", self.0)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    UDiv,
    SDiv,
    URem,
    SRem,
    Shl,
    LShr,
    AShr,
    And,
    Or,
    Xor,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FRem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastOp {
    Trunc,
    ZExt,
    SExt,
    FpToUi,
    FpToSi,
    UiToFp,
    SiToFp,
    FpTrunc,
    FpExt,
    PtrToInt,
    IntToPtr,
    Bitcast,
    AddrSpaceCast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pred {
    FFalse,
    FOeq,
    FOgt,
    FOge,
    FOlt,
    FOle,
    FOne,
    FOrd,
    FUno,
    FUeq,
    FUgt,
    FUge,
    FUlt,
    FUle,
    FUne,
    FTrue,
    IEq,
    INe,
    IUgt,
    IUge,
    IUlt,
    IUle,
    ISgt,
    ISge,
    ISlt,
    ISle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Binary {
        op: BinOp,
        lhs: ValueId,
        rhs: ValueId,
    },
    Cast {
        op: CastOp,
        value: ValueId,
    },
    Cmp {
        pred: Pred,
        lhs: ValueId,
        rhs: ValueId,
    },
    Select {
        cond: ValueId,
        then: ValueId,
        otherwise: ValueId,
    },
    ExtractValue {
        aggregate: ValueId,
        indices: Vec<u32>,
    },
    InsertValue {
        aggregate: ValueId,
        value: ValueId,
        indices: Vec<u32>,
    },
    ExtractElement {
        vector: ValueId,
        index: ValueId,
    },
    InsertElement {
        vector: ValueId,
        value: ValueId,
        index: ValueId,
    },
    ShuffleVector {
        a: ValueId,
        b: ValueId,
        mask: ValueId,
    },
    Phi {
        incoming: Vec<(ValueId, BlockId)>,
    },
    Alloca {
        ty: TypeId,
        count: ValueId,
    },
    Load {
        ptr: ValueId,
    },
    Store {
        ptr: ValueId,
        value: ValueId,
    },
    Gep {
        inbounds: bool,
        base: ValueId,
        indices: Vec<ValueId>,
    },
    AtomicRmw {
        op: u32,
        ptr: ValueId,
        value: ValueId,
    },
    CmpXchg {
        ptr: ValueId,
        expected: ValueId,
        new: ValueId,
    },
    /// A `dx.op.*` call: its constant opcode, the overload type its name ends in, the other operands.
    Dx {
        op: DxOp,
        overload: Option<TypeId>,
        args: Vec<ValueId>,
    },
    /// Any other call; refused by the translation unless it is an `llvm.*` intrinsic it lowers.
    Call {
        callee: ValueId,
        args: Vec<ValueId>,
    },
    Br {
        target: BlockId,
    },
    CondBr {
        cond: ValueId,
        then: BlockId,
        otherwise: BlockId,
    },
    Switch {
        value: ValueId,
        default: BlockId,
        cases: Vec<(i64, BlockId)>,
    },
    Ret {
        value: Option<ValueId>,
    },
    Unreachable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inst {
    /// The value it defines; `None` for a store, a branch or a void call.
    pub result: Option<ValueId>,
    pub ty: TypeId,
    pub op: Op,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BasicBlock {
    /// The terminator last.
    pub insts: Vec<Inst>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub blocks: Vec<BasicBlock>,
    /// Module values first, then arguments, the function's constants and its instructions' results.
    pub values: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub ty: TypeId,
    pub body: Option<Body>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Md {
    String(Vec<u8>),
    Value { ty: TypeId, value: ValueId },
    Node(Vec<Option<MdId>>),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    pub types: Vec<Type>,
    /// Module-level values: globals, functions, aliases, module constants.
    pub values: Vec<Value>,
    pub functions: Vec<Function>,
    pub metadata: Vec<Md>,
    /// Named metadata, such as `dx.entryPoints`, to its operand nodes.
    pub named: HashMap<String, Vec<MdId>>,
}

impl Module {
    pub fn ty(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id as usize)
    }

    /// The function `dx.entryPoints` names.
    pub fn entry(&self) -> Option<&Function> {
        let node = *self.named.get("dx.entryPoints")?.first()?;
        let Md::Node(operands) = self.metadata.get(node as usize)? else {
            return None;
        };
        let operand = (*operands.first()?)?;
        let Md::Value { value, .. } = self.metadata.get(operand as usize)? else {
            return None;
        };
        let value = self.values.get(*value as usize)?;
        match &value.kind {
            ValueKind::Function(index) => self.functions.get(*index as usize),
            _ => {
                let name = value.name.as_deref()?;
                self.functions.iter().find(|f| f.name == name)
            }
        }
    }
}

impl Body {
    pub fn value(&self, id: ValueId) -> Option<&Value> {
        self.values.get(id as usize)
    }

    /// The integer a constant value holds, for an opcode, a register or a component index.
    pub fn int(&self, id: ValueId) -> Option<i64> {
        match &self.value(id)?.kind {
            ValueKind::Constant(Constant::Int(value)) => Some(*value),
            // The bitcode writes every zero constant, an integer zero among them, as null.
            ValueKind::Constant(Constant::Null) => Some(0),
            _ => None,
        }
    }

    /// The blocks the terminator of `block` can branch to, each once, in operand order.
    pub fn successors(&self, block: BlockId) -> Vec<BlockId> {
        let Some(block) = self.blocks.get(block as usize) else {
            return Vec::new();
        };
        let Some(last) = block.insts.last() else {
            return Vec::new();
        };
        let mut targets = Vec::new();
        match &last.op {
            Op::Br { target } => targets.push(*target),
            Op::CondBr {
                then, otherwise, ..
            } => targets.extend([*then, *otherwise]),
            Op::Switch { default, cases, .. } => {
                targets.push(*default);
                targets.extend(cases.iter().map(|case| case.1));
            }
            _ => {}
        }
        let mut unique = Vec::with_capacity(targets.len());
        for target in targets {
            if !unique.contains(&target) {
                unique.push(target);
            }
        }
        unique
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(ty: TypeId, kind: ValueKind) -> Value {
        Value {
            ty,
            kind,
            name: None,
        }
    }

    fn terminator(op: Op) -> BasicBlock {
        let inst = Inst {
            result: None,
            ty: 0,
            op,
        };
        BasicBlock { insts: vec![inst] }
    }

    #[test]
    fn every_opcode_up_to_the_last_is_named_in_order() {
        for (index, &(op, name)) in DX_OPS.iter().enumerate() {
            assert_eq!(op, DxOp(index as u32));
            assert_eq!(op.name(), name);
        }
        assert_eq!(DX_OPS.len(), 227);
        assert_eq!(DxOp::SAMPLE_LEVEL.name(), "SampleLevel");
        assert_eq!(DxOp::CREATE_HANDLE_FROM_HEAP, DxOp(218));
        assert_eq!(DxOp(227).name(), "op227");
        assert_eq!(DxOp(u32::MAX).name(), format!("op{}", u32::MAX));
    }

    #[test]
    fn int_reads_integer_and_null_constants_only() {
        let body = Body {
            blocks: Vec::new(),
            values: vec![
                val(1, ValueKind::Constant(Constant::Int(-7))),
                val(1, ValueKind::Constant(Constant::Null)),
                val(1, ValueKind::Constant(Constant::Undef)),
                val(2, ValueKind::Constant(Constant::Float(0x3F80_0000))),
                val(1, ValueKind::Argument(0)),
            ],
        };
        assert_eq!(body.int(0), Some(-7));
        assert_eq!(body.int(1), Some(0));
        assert_eq!(body.int(2), None);
        assert_eq!(body.int(3), None);
        assert_eq!(body.int(4), None);
        assert_eq!(body.int(5), None);
        assert_eq!(body.value(4).unwrap().kind, ValueKind::Argument(0));
    }

    #[test]
    fn successors_follow_each_terminator_once() {
        let fork = Op::CondBr {
            cond: 9,
            then: 3,
            otherwise: 1,
        };
        let switch = Op::Switch {
            value: 9,
            default: 4,
            cases: vec![(5, 1), (6, 4), (7, 3), (8, 1)],
        };
        let same = Op::CondBr {
            cond: 9,
            then: 2,
            otherwise: 2,
        };
        let body = Body {
            blocks: vec![
                terminator(fork),
                terminator(Op::Br { target: 2 }),
                terminator(switch),
                terminator(same),
                terminator(Op::Ret { value: None }),
                BasicBlock::default(),
                terminator(Op::Unreachable),
            ],
            values: Vec::new(),
        };
        assert_eq!(body.successors(0), vec![3, 1]);
        assert_eq!(body.successors(1), vec![2]);
        assert_eq!(body.successors(2), vec![4, 1, 3]);
        assert_eq!(body.successors(3), vec![2]);
        assert!(body.successors(4).is_empty());
        assert!(body.successors(5).is_empty());
        assert!(body.successors(6).is_empty());
        assert!(body.successors(7).is_empty());
    }

    fn function(name: &str) -> Function {
        Function {
            name: name.to_owned(),
            ty: 1,
            body: None,
        }
    }

    fn module_with_entry(entry: Value) -> Module {
        let function_type = Type::Function {
            ret: 0,
            params: Vec::new(),
            vararg: false,
        };
        Module {
            types: vec![Type::Void, function_type],
            values: vec![val(0, ValueKind::Constant(Constant::Undef)), entry],
            functions: vec![
                function("dx.op.loadInput.f32"),
                function("main"),
                function("other"),
            ],
            metadata: vec![
                Md::String(b"main".to_vec()),
                Md::Value { ty: 1, value: 1 },
                Md::Node(vec![Some(1), Some(0), None]),
            ],
            named: HashMap::from([("dx.entryPoints".to_owned(), vec![2])]),
        }
    }

    #[test]
    fn entry_follows_dx_entry_points_to_its_function() {
        let by_index = module_with_entry(val(1, ValueKind::Function(1)));
        assert_eq!(by_index.entry().map(|f| f.name.as_str()), Some("main"));
        assert!(matches!(by_index.ty(1), Some(Type::Function { .. })));
        assert_eq!(by_index.ty(2), None);

        let kind = ValueKind::Global {
            space: 0,
            init: None,
            constant: true,
        };
        let mut global = val(1, kind);
        global.name = Some("other".to_owned());
        let by_name = module_with_entry(global);
        assert_eq!(by_name.entry().map(|f| f.name.as_str()), Some("other"));

        let mut library = module_with_entry(val(1, ValueKind::Function(1)));
        library.metadata[2] = Md::Node(vec![None, Some(0)]);
        assert!(library.entry().is_none());
        assert!(Module::default().entry().is_none());
    }
}
