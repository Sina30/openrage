//! The dx.op intrinsics a material's vertex, pixel and compute programs use, as naga expressions
//! and statements; any other opcode is refused by its public name.

use naga::{
    BinaryOperator as Bo, Block, DerivativeAxis, DerivativeControl, Expression as E, Handle,
    ImageClass, ImageDimension, ImageQuery, Literal, MathFunction as Mf, RelationalFunction,
    SampleLevel, Scalar, ScalarKind, Span, Statement, SwizzleComponent, TypeInner, UnaryOperator,
    VectorSize,
};

use super::ir::{self, Constant, DxOp, TypeId, ValueId, ValueKind};
use super::metadata::ResourceShape;
use super::plan::Planned;
use super::to_naga::{FnCx, TranslateError};
use super::{Register, ResourceClass};

type Out = Result<Option<Handle<E>>, TranslateError>;

/// DXIL's barrier mode bits.
const SYNC_THREAD_GROUP: i64 = 1;
const UAV_FENCE_GLOBAL: i64 = 2;
const UAV_FENCE_THREAD_GROUP: i64 = 4;
const TGSM_FENCE: i64 = 8;

/// One dx.op call lowered: its result, or `None` for a store, a discard or a barrier it pushed.
pub fn lower(cx: &mut FnCx<'_>, op: DxOp, overload: Option<TypeId>, args: &[ValueId]) -> Out {
    if let Some(fun) = unary(op) {
        let x = operand(cx, args, 0)?;
        return Ok(Some(math(cx, fun, x, &[])));
    }
    Ok(Some(match op {
        DxOp::IS_NAN | DxOp::IS_INF => {
            let argument = operand(cx, args, 0)?;
            let fun = if op == DxOp::IS_NAN {
                RelationalFunction::IsNan
            } else {
                RelationalFunction::IsInf
            };
            cx.emit(E::Relational { fun, argument })
        }
        DxOp::IS_FINITE => {
            let argument = operand(cx, args, 0)?;
            let nan = cx.emit(E::Relational {
                fun: RelationalFunction::IsNan,
                argument,
            });
            let inf = cx.emit(E::Relational {
                fun: RelationalFunction::IsInf,
                argument,
            });
            let either = bin(cx, Bo::LogicalOr, nan, inf);
            cx.emit(E::Unary {
                op: UnaryOperator::LogicalNot,
                expr: either,
            })
        }
        DxOp::BFREV => {
            wide(cx, op, overload)?;
            let x = operand(cx, args, 0)?;
            math(cx, Mf::ReverseBits, x, &[])
        }
        // D3D counts the first set bit from the top, and answers all ones for none.
        DxOp::FIRSTBIT_HI | DxOp::FIRSTBIT_SHI => {
            wide(cx, op, overload)?;
            let mut x = operand(cx, args, 0)?;
            if op == DxOp::FIRSTBIT_SHI {
                // A negative number's first bit is its first zero below the sign.
                let signed = bitcast(cx, x, ScalarKind::Sint);
                let zero = lit(cx, Literal::I32(0));
                let negative = bin(cx, Bo::Less, signed, zero);
                let inverted = cx.emit(E::Unary {
                    op: UnaryOperator::BitwiseNot,
                    expr: x,
                });
                x = cx.emit(E::Select {
                    condition: negative,
                    accept: inverted,
                    reject: x,
                });
            }
            let leading = math(cx, Mf::CountLeadingZeros, x, &[]);
            let zero = lit(cx, Literal::U32(0));
            let none = bin(cx, Bo::Equal, x, zero);
            let all = lit(cx, Literal::U32(u32::MAX));
            cx.emit(E::Select {
                condition: none,
                accept: all,
                reject: leading,
            })
        }
        DxOp::FMAX | DxOp::FMIN | DxOp::UMAX | DxOp::UMIN => {
            let fun = if matches!(op, DxOp::FMAX | DxOp::UMAX) {
                Mf::Max
            } else {
                Mf::Min
            };
            let (a, b) = (operand(cx, args, 0)?, operand(cx, args, 1)?);
            math(cx, fun, a, &[b])
        }
        DxOp::IMAX | DxOp::IMIN => {
            let fun = if op == DxOp::IMAX { Mf::Max } else { Mf::Min };
            let bits = int_bits(cx, overload);
            let a = operand(cx, args, 0)?;
            let a = signed(cx, a, bits);
            let b = operand(cx, args, 1)?;
            let b = signed(cx, b, bits);
            let m = math(cx, fun, a, &[b]);
            let m = bitcast(cx, m, ScalarKind::Uint);
            narrow(cx, m, bits)
        }
        DxOp::FMAD | DxOp::IMAD | DxOp::UMAD => {
            let (a, b, c) = (
                operand(cx, args, 0)?,
                operand(cx, args, 1)?,
                operand(cx, args, 2)?,
            );
            let product = bin(cx, Bo::Multiply, a, b);
            let sum = bin(cx, Bo::Add, product, c);
            if op == DxOp::FMAD {
                sum
            } else {
                let bits = int_bits(cx, overload);
                narrow(cx, sum, bits)
            }
        }
        DxOp::FMA => {
            let (a, b, c) = (
                operand(cx, args, 0)?,
                operand(cx, args, 1)?,
                operand(cx, args, 2)?,
            );
            math(cx, Mf::Fma, a, &[b, c])
        }
        // Width, offset, value: D3D reads the low five bits of the width and the offset.
        DxOp::UBFE | DxOp::IBFE => {
            wide(cx, op, overload)?;
            let width = operand(cx, args, 0)?;
            let width = low5(cx, width);
            let offset = operand(cx, args, 1)?;
            let offset = low5(cx, offset);
            let value = operand(cx, args, 2)?;
            if op == DxOp::UBFE {
                math(cx, Mf::ExtractBits, value, &[offset, width])
            } else {
                let value = bitcast(cx, value, ScalarKind::Sint);
                let field = math(cx, Mf::ExtractBits, value, &[offset, width]);
                bitcast(cx, field, ScalarKind::Uint)
            }
        }
        // Width, offset, the bits inserted, the value they go in.
        DxOp::BFI => {
            wide(cx, op, overload)?;
            let width = operand(cx, args, 0)?;
            let width = low5(cx, width);
            let offset = operand(cx, args, 1)?;
            let offset = low5(cx, offset);
            let bits = operand(cx, args, 2)?;
            let base = operand(cx, args, 3)?;
            math(cx, Mf::InsertBits, base, &[bits, offset, width])
        }
        DxOp::DOT2 | DxOp::DOT3 | DxOp::DOT4 => {
            let n = match op {
                DxOp::DOT2 => 2,
                DxOp::DOT3 => 3,
                _ => 4,
            };
            let a = operands(cx, args, 0, n)?;
            let a = compose(cx, a, Scalar::F32)?;
            let b = operands(cx, args, n, n)?;
            let b = compose(cx, b, Scalar::F32)?;
            math(cx, Mf::Dot, a, &[b])
        }
        DxOp::BITCAST_I32_TO_F32 => {
            let x = operand(cx, args, 0)?;
            bitcast(cx, x, ScalarKind::Float)
        }
        DxOp::BITCAST_F32_TO_I32 => {
            let x = operand(cx, args, 0)?;
            bitcast(cx, x, ScalarKind::Uint)
        }
        // A half is held as a float: its 16 bits come and go through f16 packing.
        DxOp::BITCAST_F16_TO_I16 | DxOp::LEGACY_F32_TO_F16 => {
            let x = operand(cx, args, 0)?;
            let zero = lit(cx, Literal::F32(0.0));
            let pair = compose(cx, vec![x, zero], Scalar::F32)?;
            math(cx, Mf::Pack2x16float, pair, &[])
        }
        DxOp::BITCAST_I16_TO_F16 | DxOp::LEGACY_F16_TO_F32 => {
            let x = operand(cx, args, 0)?;
            let pair = math(cx, Mf::Unpack2x16float, x, &[]);
            cx.emit(E::AccessIndex {
                base: pair,
                index: 0,
            })
        }
        DxOp::DERIV_COARSE_X | DxOp::DERIV_COARSE_Y | DxOp::DERIV_FINE_X | DxOp::DERIV_FINE_Y => {
            let (axis, ctrl) = match op {
                DxOp::DERIV_COARSE_X => (DerivativeAxis::X, DerivativeControl::Coarse),
                DxOp::DERIV_COARSE_Y => (DerivativeAxis::Y, DerivativeControl::Coarse),
                DxOp::DERIV_FINE_X => (DerivativeAxis::X, DerivativeControl::Fine),
                _ => (DerivativeAxis::Y, DerivativeControl::Fine),
            };
            let expr = operand(cx, args, 0)?;
            cx.emit(E::Derivative { axis, ctrl, expr })
        }
        DxOp::DISCARD => {
            let condition = operand(cx, args, 0)?;
            let mut accept = Block::new();
            accept.push(Statement::Kill, Span::UNDEFINED);
            cx.push(Statement::If {
                condition,
                accept,
                reject: Block::new(),
            });
            return Ok(None);
        }
        DxOp::BARRIER => {
            let mode = args
                .first()
                .and_then(|&a| cx.int(a))
                .ok_or_else(|| not_constant(op, "barrier mode"))?;
            let mut flags = naga::Barrier::empty();
            if mode & (UAV_FENCE_GLOBAL | UAV_FENCE_THREAD_GROUP) != 0 {
                flags |= naga::Barrier::STORAGE | naga::Barrier::TEXTURE;
            }
            if mode & TGSM_FENCE != 0 {
                flags |= naga::Barrier::WORK_GROUP;
            }
            cx.push(if mode & SYNC_THREAD_GROUP != 0 {
                Statement::ControlBarrier(flags)
            } else {
                Statement::MemoryBarrier(flags)
            });
            return Ok(None);
        }
        DxOp::IS_HELPER_LANE => {
            // naga states no helper lane: every lane is taken as one that runs the program.
            cx.approximated("helper-lane-as-false");
            lit(cx, Literal::Bool(false))
        }
        // Every texel a resource read returns is resident, as its status word already says.
        DxOp::CHECK_ACCESS_FULLY_MAPPED => lit(cx, Literal::Bool(true)),
        DxOp::CBUFFER_LOAD_LEGACY => cbuffer_row(cx, op, overload, args)?,
        DxOp::SAMPLE
        | DxOp::SAMPLE_BIAS
        | DxOp::SAMPLE_LEVEL
        | DxOp::SAMPLE_GRAD
        | DxOp::SAMPLE_CMP
        | DxOp::SAMPLE_CMP_LEVEL_ZERO
        | DxOp::SAMPLE_CMP_LEVEL
        | DxOp::TEXTURE_GATHER
        | DxOp::TEXTURE_GATHER_CMP => sample(cx, op, overload, args)?,
        DxOp::TEXTURE_LOAD => texture_load(cx, op, overload, args)?,
        DxOp::TEXTURE_STORE => {
            texture_store(cx, overload, args)?;
            return Ok(None);
        }
        DxOp::BUFFER_LOAD | DxOp::RAW_BUFFER_LOAD => buffer_load(cx, op, overload, args)?,
        DxOp::BUFFER_STORE | DxOp::RAW_BUFFER_STORE => {
            buffer_store(cx, op, overload, args)?;
            return Ok(None);
        }
        DxOp::GET_DIMENSIONS => dimensions(cx, op, args)?,
        _ => return Err(TranslateError::Op(op.name().into_owned())),
    }))
}

/// The ops that are one naga math function of their one operand.
fn unary(op: DxOp) -> Option<Mf> {
    Some(match op {
        DxOp::FABS => Mf::Abs,
        DxOp::SATURATE => Mf::Saturate,
        DxOp::COS => Mf::Cos,
        DxOp::SIN => Mf::Sin,
        DxOp::TAN => Mf::Tan,
        DxOp::ACOS => Mf::Acos,
        DxOp::ASIN => Mf::Asin,
        DxOp::ATAN => Mf::Atan,
        DxOp::HCOS => Mf::Cosh,
        DxOp::HSIN => Mf::Sinh,
        DxOp::HTAN => Mf::Tanh,
        // DXIL's exponential and logarithm are base 2.
        DxOp::EXP => Mf::Exp2,
        DxOp::LOG => Mf::Log2,
        DxOp::FRC => Mf::Fract,
        DxOp::SQRT => Mf::Sqrt,
        DxOp::RSQRT => Mf::InverseSqrt,
        DxOp::ROUND_NE => Mf::Round,
        DxOp::ROUND_NI => Mf::Floor,
        DxOp::ROUND_PI => Mf::Ceil,
        DxOp::ROUND_Z => Mf::Trunc,
        DxOp::COUNTBITS => Mf::CountOneBits,
        DxOp::FIRSTBIT_LO => Mf::FirstTrailingBit,
        _ => return None,
    })
}

/// `CBufferLoadLegacy`: one 16-byte row of a constant buffer, read as the overload's scalar.
fn cbuffer_row(
    cx: &mut FnCx<'_>,
    op: DxOp,
    overload: Option<TypeId>,
    args: &[ValueId],
) -> Result<Handle<E>, TranslateError> {
    wide(cx, op, overload)?;
    let global = cx.resource(arg(args, 0)?)?;
    if !matches!(
        cx.module().global_variables[global].space,
        naga::AddressSpace::Uniform
    ) {
        return Err(TranslateError::Inst(
            "a constant buffer load from a resource that is no constant buffer",
        ));
    }
    let base = cx.emit(E::GlobalVariable(global));
    let row = arg(args, 1)?;
    let pointer = match cx.int(row) {
        Some(at) => {
            let index = u32::try_from(at)
                .map_err(|_| TranslateError::Inst("a constant buffer row out of range"))?;
            cx.emit(E::AccessIndex { base, index })
        }
        None => {
            let index = cx.value(row)?;
            cx.emit(E::Access { base, index })
        }
    };
    let words = cx.emit(E::Load { pointer });
    retype(cx, words, ScalarKind::Uint, overload)
}

/// The sample, comparison and gather ops; operands from the third are the coordinates, then the
/// offsets, then what each op adds.
fn sample(
    cx: &mut FnCx<'_>,
    op: DxOp,
    overload: Option<TypeId>,
    args: &[ValueId],
) -> Result<Handle<E>, TranslateError> {
    let image_global = cx.resource(arg(args, 0)?)?;
    let sampler_global = cx.resource(arg(args, 1)?)?;
    let (dim, arrayed, class) = image(cx, image_global)?;
    let depth = matches!(class, ImageClass::Depth { .. });
    let n = components(dim);
    let coordinate = operands(cx, args, 2, n)?;
    let coordinate = compose(cx, coordinate, Scalar::F32)?;
    let array_index = if arrayed {
        // D3D rounds a float array layer to the nearest.
        let layer = operand(cx, args, 2 + n)?;
        let layer = math(cx, Mf::Round, layer, &[]);
        Some(cx.emit(E::As {
            expr: layer,
            kind: ScalarKind::Sint,
            convert: Some(4),
        }))
    } else {
        None
    };
    let gather = matches!(op, DxOp::TEXTURE_GATHER | DxOp::TEXTURE_GATHER_CMP);
    // A gather states two offsets, every other op three; one past the image's dimension is unused.
    let stated = if gather { 2 } else { 3 };
    let offsets = (0..n)
        .map(|i| {
            if i < stated {
                offset(cx, op, args, 6 + i)
            } else {
                Ok(0)
            }
        })
        .collect::<Result<Vec<i32>, _>>()?;
    let offset = if offsets.iter().all(|&o| o == 0) {
        None
    } else {
        let parts = offsets
            .iter()
            .map(|&o| cx.emit(E::Literal(Literal::I32(o))))
            .collect();
        Some(compose(cx, parts, Scalar::I32)?)
    };
    let (level, depth_ref, component) = match op {
        DxOp::SAMPLE => (SampleLevel::Auto, None, None),
        DxOp::SAMPLE_BIAS => (SampleLevel::Bias(operand(cx, args, 9)?), None, None),
        DxOp::SAMPLE_LEVEL => {
            let lod = operand(cx, args, 9)?;
            // naga reads a depth texture's level as an integer.
            let lod = if depth {
                cx.emit(E::As {
                    expr: lod,
                    kind: ScalarKind::Sint,
                    convert: Some(4),
                })
            } else {
                lod
            };
            (SampleLevel::Exact(lod), None, None)
        }
        DxOp::SAMPLE_GRAD => {
            let x = operands(cx, args, 9, n)?;
            let x = compose(cx, x, Scalar::F32)?;
            let y = operands(cx, args, 12, n)?;
            let y = compose(cx, y, Scalar::F32)?;
            (SampleLevel::Gradient { x, y }, None, None)
        }
        DxOp::SAMPLE_CMP => (SampleLevel::Auto, Some(operand(cx, args, 9)?), None),
        DxOp::SAMPLE_CMP_LEVEL_ZERO => (SampleLevel::Zero, Some(operand(cx, args, 9)?), None),
        // naga compares at the first level or the automatic one only.
        DxOp::SAMPLE_CMP_LEVEL if zero(cx, args, 10) => {
            (SampleLevel::Zero, Some(operand(cx, args, 9)?), None)
        }
        DxOp::TEXTURE_GATHER | DxOp::TEXTURE_GATHER_CMP => {
            let channel = args
                .get(8)
                .and_then(|&a| cx.int(a))
                .ok_or_else(|| not_constant(op, "channel"))?;
            let channel =
                u32::try_from(channel)
                    .ok()
                    .filter(|&c| c < 4)
                    .ok_or(TranslateError::Inst(
                        "a gather of a channel past the fourth",
                    ))?;
            let depth_ref = if op == DxOp::TEXTURE_GATHER_CMP {
                Some(operand(cx, args, 9)?)
            } else {
                None
            };
            (
                SampleLevel::Zero,
                depth_ref,
                Some(SwizzleComponent::from_index(channel)),
            )
        }
        _ => return Err(TranslateError::Op(format!("{}.level", op.name()))),
    };
    let clamp = match op {
        DxOp::SAMPLE => Some(9),
        DxOp::SAMPLE_BIAS | DxOp::SAMPLE_CMP => Some(10),
        DxOp::SAMPLE_GRAD => Some(15),
        _ => None,
    };
    if clamp.is_some_and(|at| stated_at(cx, args, at)) {
        cx.approximated("sample-clamp-ignored");
    }
    let image = cx.emit(E::GlobalVariable(image_global));
    let sampler = cx.emit(E::GlobalVariable(sampler_global));
    let texel = cx.emit(E::ImageSample {
        image,
        sampler,
        gather: component,
        coordinate,
        array_index,
        offset,
        level,
        depth_ref,
        clamp_to_edge: false,
    });
    // A depth texture sampled without a gather gives one float; DXIL reads four.
    let texel = if depth && component.is_none() {
        cx.emit(E::Splat {
            size: VectorSize::Quad,
            value: texel,
        })
    } else {
        texel
    };
    let kind = if depth {
        ScalarKind::Float
    } else {
        texel_kind(class)
    };
    retype(cx, texel, kind, overload)
}

/// `TextureLoad`: texture, level or sample, three coordinates, three constant offsets.
fn texture_load(
    cx: &mut FnCx<'_>,
    op: DxOp,
    overload: Option<TypeId>,
    args: &[ValueId],
) -> Result<Handle<E>, TranslateError> {
    let global = cx.resource(arg(args, 0)?)?;
    let (dim, arrayed, class) = image(cx, global)?;
    if dim == ImageDimension::Cube {
        return Err(TranslateError::Inst("a texel load from a cube texture"));
    }
    let n = components(dim);
    let mut coordinate = Vec::with_capacity(n);
    for i in 0..n {
        let c = operand(cx, args, 2 + i)?;
        coordinate.push(match offset(cx, op, args, 5 + i)? {
            0 => c,
            o => {
                let step = lit(cx, Literal::U32(o as u32));
                bin(cx, Bo::Add, c, step)
            }
        });
    }
    let coordinate = compose(cx, coordinate, Scalar::U32)?;
    let array_index = if arrayed {
        Some(operand(cx, args, 2 + n)?)
    } else {
        None
    };
    let (sample, level) = if class.is_multisampled() {
        (Some(operand(cx, args, 1)?), None)
    } else if class.is_mipmapped() {
        (None, Some(operand(cx, args, 1)?))
    } else {
        (None, None)
    };
    let image = cx.emit(E::GlobalVariable(global));
    let texel = cx.emit(E::ImageLoad {
        image,
        coordinate,
        array_index,
        sample,
        level,
    });
    let depth = matches!(class, ImageClass::Depth { .. });
    let texel = if depth {
        cx.emit(E::Splat {
            size: VectorSize::Quad,
            value: texel,
        })
    } else {
        texel
    };
    let kind = if depth {
        ScalarKind::Float
    } else {
        texel_kind(class)
    };
    retype(cx, texel, kind, overload)
}

/// `TextureStore`: texture, three coordinates, four values, a mask; the whole texel is written.
fn texture_store(
    cx: &mut FnCx<'_>,
    overload: Option<TypeId>,
    args: &[ValueId],
) -> Result<(), TranslateError> {
    let global = cx.resource(arg(args, 0)?)?;
    let (dim, arrayed, class) = image(cx, global)?;
    let n = components(dim);
    let coordinate = operands(cx, args, 1, n)?;
    let coordinate = compose(cx, coordinate, Scalar::U32)?;
    let array_index = if arrayed {
        Some(operand(cx, args, 1 + n)?)
    } else {
        None
    };
    let scalar = overloaded(cx, overload)?;
    let values = operands(cx, args, 4, 4)?;
    let value = compose(cx, values, scalar)?;
    let kind = texel_kind(class);
    let value = if scalar.kind == kind {
        value
    } else {
        bitcast(cx, value, kind)
    };
    let image = cx.emit(E::GlobalVariable(global));
    cx.push(Statement::ImageStore {
        image,
        coordinate,
        array_index,
        value,
    });
    Ok(())
}

/// `BufferLoad` and `RawBufferLoad`: buffer, element index (a byte address in a raw buffer), byte
/// offset in the element; `RawBufferLoad`'s mask names the components it reads, `BufferLoad` four.
fn buffer_load(
    cx: &mut FnCx<'_>,
    op: DxOp,
    overload: Option<TypeId>,
    args: &[ValueId],
) -> Result<Handle<E>, TranslateError> {
    wide(cx, op, overload)?;
    let (global, stride) = buffer(cx, op, arg(args, 0)?, op == DxOp::RAW_BUFFER_LOAD)?;
    let word = word(cx, args, stride)?;
    let count = if op == DxOp::RAW_BUFFER_LOAD {
        let mask = args
            .get(3)
            .and_then(|&a| cx.int(a))
            .ok_or_else(|| not_constant(op, "mask"))?;
        let mask = (mask & 0xF) as u8;
        u8::BITS - mask.leading_zeros()
    } else {
        4
    };
    let base = cx.emit(E::GlobalVariable(global));
    let mut parts = Vec::with_capacity(4);
    for c in 0..4u32 {
        parts.push(if c < count {
            let index = step(cx, word, c);
            let pointer = cx.emit(E::Access { base, index });
            cx.emit(E::Load { pointer })
        } else {
            lit(cx, Literal::U32(0))
        });
    }
    let value = compose(cx, parts, Scalar::U32)?;
    retype(cx, value, ScalarKind::Uint, overload)
}

/// `BufferStore` and `RawBufferStore`: buffer, the two address operands of a load, four values and
/// the mask of the ones written.
fn buffer_store(
    cx: &mut FnCx<'_>,
    op: DxOp,
    overload: Option<TypeId>,
    args: &[ValueId],
) -> Result<(), TranslateError> {
    wide(cx, op, overload)?;
    let (global, stride) = buffer(cx, op, arg(args, 0)?, op == DxOp::RAW_BUFFER_STORE)?;
    let word = word(cx, args, stride)?;
    let mask = args
        .get(7)
        .and_then(|&a| cx.int(a))
        .ok_or_else(|| not_constant(op, "mask"))?;
    let scalar = overloaded(cx, overload)?;
    let base = cx.emit(E::GlobalVariable(global));
    for c in 0..4u32 {
        if (mask >> c) & 1 == 0 {
            continue;
        }
        let value = operand(cx, args, 3 + c as usize)?;
        let value = if scalar.kind == ScalarKind::Uint {
            value
        } else {
            bitcast(cx, value, ScalarKind::Uint)
        };
        let index = step(cx, word, c);
        let pointer = cx.emit(E::Access { base, index });
        cx.push(Statement::Store { pointer, value });
    }
    Ok(())
}

/// `GetDimensions`: width, height, depth or layers, then levels or samples; a buffer's width is its
/// bytes, or its elements for a structured one.
fn dimensions(cx: &mut FnCx<'_>, op: DxOp, args: &[ValueId]) -> Result<Handle<E>, TranslateError> {
    let handle = arg(args, 0)?;
    let global = cx.resource(handle)?;
    let module = cx.module();
    let is_image = matches!(
        module.types[module.global_variables[global].ty].inner,
        TypeInner::Image { .. }
    );
    let parts = if is_image {
        let (dim, arrayed, class) = image(cx, global)?;
        let image = cx.emit(E::GlobalVariable(global));
        let level = if class.is_mipmapped() {
            Some(operand(cx, args, 1)?)
        } else {
            None
        };
        let size = cx.emit(E::ImageQuery {
            image,
            query: ImageQuery::Size { level },
        });
        let mut parts: Vec<Handle<E>> = match dim {
            ImageDimension::D1 => vec![size],
            ImageDimension::D2 | ImageDimension::Cube => (0..2)
                .map(|index| cx.emit(E::AccessIndex { base: size, index }))
                .collect(),
            ImageDimension::D3 => (0..3)
                .map(|index| cx.emit(E::AccessIndex { base: size, index }))
                .collect(),
        };
        if arrayed {
            parts.push(cx.emit(E::ImageQuery {
                image,
                query: ImageQuery::NumLayers,
            }));
        }
        parts.resize_with(3, || cx.emit(E::Literal(Literal::U32(0))));
        parts.push(if class.is_multisampled() {
            cx.emit(E::ImageQuery {
                image,
                query: ImageQuery::NumSamples,
            })
        } else if class.is_mipmapped() {
            cx.emit(E::ImageQuery {
                image,
                query: ImageQuery::NumLevels,
            })
        } else {
            lit(cx, Literal::U32(0))
        });
        parts
    } else {
        let (global, stride) = buffer(cx, op, handle, false)?;
        let pointer = cx.emit(E::GlobalVariable(global));
        let words = cx.emit(E::ArrayLength(pointer));
        let four = lit(cx, Literal::U32(4));
        let bytes = bin(cx, Bo::Multiply, words, four);
        let width = match stride {
            Some(stride) if stride > 0 => {
                let stride = lit(cx, Literal::U32(stride));
                bin(cx, Bo::Divide, bytes, stride)
            }
            _ => bytes,
        };
        let zero = lit(cx, Literal::U32(0));
        vec![width, zero, zero, zero]
    };
    compose(cx, parts, Scalar::U32)
}

/// A buffer's global and, for a structured one, its stride; a typed buffer is refused, as its
/// element format is bound only when the game runs.
fn buffer(
    cx: &mut FnCx<'_>,
    op: DxOp,
    handle: ValueId,
    raw: bool,
) -> Result<(Handle<naga::GlobalVariable>, Option<u32>), TranslateError> {
    let global = cx.resource(handle)?;
    let variable = &cx.module().global_variables[global];
    if !matches!(variable.space, naga::AddressSpace::Storage { .. }) {
        return Err(TranslateError::Inst(
            "a buffer operation on a resource that is no buffer",
        ));
    }
    let key = variable.binding.as_ref().map(|b| (b.group, b.binding));
    let planned = cx
        .plan
        .bindings
        .iter()
        .find(|b| key == Some((b.group, b.binding)))
        .ok_or(TranslateError::Inst(
            "a buffer the binding plan does not hold",
        ))?;
    let Planned::Storage { stride, .. } = planned.planned else {
        return Err(TranslateError::Inst(
            "a buffer operation on a resource that is no buffer",
        ));
    };
    // A buffer `dx.resources` does not declare raw is taken as typed.
    if stride.is_none() && !raw && !raw_buffer(cx, planned.register) {
        return Err(TranslateError::Op(format!("{}.typed", op.name())));
    }
    Ok((global, stride))
}

fn raw_buffer(cx: &FnCx<'_>, r: Register) -> bool {
    let decls = super::metadata::dx_resources(cx.ir).unwrap_or_default();
    let list = if r.class == ResourceClass::Srv {
        decls.srvs
    } else {
        decls.uavs
    };
    list.iter().any(|d| {
        d.shape == ResourceShape::RawBuffer
            && d.space == r.space
            && r.index >= d.lower
            && u64::from(r.index) < u64::from(d.lower) + u64::from(d.count)
    })
}

/// The `u32` of a buffer access: operand 1 the element (a byte address with no stride), operand 2
/// the byte offset in a structured element.
fn word(
    cx: &mut FnCx<'_>,
    args: &[ValueId],
    stride: Option<u32>,
) -> Result<Handle<E>, TranslateError> {
    let index = operand(cx, args, 1)?;
    let byte = match stride {
        Some(stride) => {
            let size = lit(cx, Literal::U32(stride));
            let start = bin(cx, Bo::Multiply, index, size);
            if stated_at(cx, args, 2) {
                let offset = operand(cx, args, 2)?;
                bin(cx, Bo::Add, start, offset)
            } else {
                start
            }
        }
        None => index,
    };
    let two = lit(cx, Literal::U32(2));
    Ok(bin(cx, Bo::ShiftRight, byte, two))
}

fn step(cx: &mut FnCx<'_>, word: Handle<E>, by: u32) -> Handle<E> {
    if by == 0 {
        return word;
    }
    let by = lit(cx, Literal::U32(by));
    bin(cx, Bo::Add, word, by)
}

fn image(
    cx: &FnCx<'_>,
    global: Handle<naga::GlobalVariable>,
) -> Result<(ImageDimension, bool, ImageClass), TranslateError> {
    let module = cx.module();
    match module.types[module.global_variables[global].ty].inner {
        TypeInner::Image {
            dim,
            arrayed,
            class,
        } => Ok((dim, arrayed, class)),
        _ => Err(TranslateError::Inst(
            "a texture operation on a resource that is no texture",
        )),
    }
}

/// The scalar kind of a texel the image returns or takes.
fn texel_kind(class: ImageClass) -> ScalarKind {
    match class {
        ImageClass::Sampled { kind, .. } => kind,
        ImageClass::Storage {
            format: naga::StorageFormat::R32Uint,
            ..
        } => ScalarKind::Uint,
        ImageClass::Storage {
            format: naga::StorageFormat::R32Sint,
            ..
        } => ScalarKind::Sint,
        _ => ScalarKind::Float,
    }
}

fn components(dim: ImageDimension) -> usize {
    match dim {
        ImageDimension::D1 => 1,
        ImageDimension::D2 => 2,
        ImageDimension::D3 | ImageDimension::Cube => 3,
    }
}

/// `x`, of `from`'s kind, as the overload's: bits reinterpreted, never converted.
fn retype(
    cx: &mut FnCx<'_>,
    x: Handle<E>,
    from: ScalarKind,
    overload: Option<TypeId>,
) -> Result<Handle<E>, TranslateError> {
    let to = match overload {
        Some(ty) => cx.scalar(ty)?.kind,
        None => return Ok(x),
    };
    Ok(if to == from || to == ScalarKind::Bool {
        x
    } else {
        bitcast(cx, x, to)
    })
}

fn overloaded(cx: &FnCx<'_>, overload: Option<TypeId>) -> Result<Scalar, TranslateError> {
    cx.scalar(overload.ok_or(TranslateError::Inst(
        "a dx.op call whose name states no overload",
    ))?)
}

/// Refuse a 16-bit overload where what the op does depends on the 32 bits a 16-bit value is held in.
fn wide(cx: &FnCx<'_>, op: DxOp, overload: Option<TypeId>) -> Result<(), TranslateError> {
    match overload.and_then(|ty| cx.ir.ty(ty)) {
        Some(ir::Type::Half) => Err(TranslateError::Op(format!("{}.f16", op.name()))),
        Some(ir::Type::Int(16)) => Err(TranslateError::Op(format!("{}.i16", op.name()))),
        _ => Ok(()),
    }
}

fn int_bits(cx: &FnCx<'_>, overload: Option<TypeId>) -> u32 {
    match overload.and_then(|ty| cx.ir.ty(ty)) {
        Some(ir::Type::Int(bits)) => *bits,
        _ => 32,
    }
}

/// A `bits`-wide integer held in a `u32`, as an `i32` with its sign carried up.
fn signed(cx: &mut FnCx<'_>, x: Handle<E>, bits: u32) -> Handle<E> {
    if bits >= 32 {
        return bitcast(cx, x, ScalarKind::Sint);
    }
    let shift = lit(cx, Literal::U32(32 - bits));
    let up = bin(cx, Bo::ShiftLeft, x, shift);
    let up = bitcast(cx, up, ScalarKind::Sint);
    bin(cx, Bo::ShiftRight, up, shift)
}

/// The low `bits` of a `u32`.
fn narrow(cx: &mut FnCx<'_>, x: Handle<E>, bits: u32) -> Handle<E> {
    if bits >= 32 {
        return x;
    }
    let mask = lit(cx, Literal::U32((1 << bits) - 1));
    bin(cx, Bo::And, x, mask)
}

fn low5(cx: &mut FnCx<'_>, x: Handle<E>) -> Handle<E> {
    let mask = lit(cx, Literal::U32(31));
    bin(cx, Bo::And, x, mask)
}

/// `parts` as one value: the one itself, else a vector of `scalar`.
fn compose(
    cx: &mut FnCx<'_>,
    parts: Vec<Handle<E>>,
    scalar: Scalar,
) -> Result<Handle<E>, TranslateError> {
    let size = match parts.as_slice() {
        [one] => return Ok(*one),
        [_, _] => VectorSize::Bi,
        [_, _, _] => VectorSize::Tri,
        [_, _, _, _] => VectorSize::Quad,
        _ => {
            return Err(TranslateError::Inst(
                "a vector of other than one to four components",
            ))
        }
    };
    let ty = cx.ty(TypeInner::Vector { size, scalar });
    Ok(cx.emit(E::Compose {
        ty,
        components: parts,
    }))
}

/// A constant texel offset; an undef one, how DXIL states none, is zero.
fn offset(cx: &FnCx<'_>, op: DxOp, args: &[ValueId], at: usize) -> Result<i32, TranslateError> {
    if !stated_at(cx, args, at) {
        return Ok(0);
    }
    let id = arg(args, at)?;
    cx.int(id)
        .map(|v| v as i32)
        .ok_or_else(|| not_constant(op, "texel offset"))
}

/// Whether operand `at` is there and is not undef.
fn stated_at(cx: &FnCx<'_>, args: &[ValueId], at: usize) -> bool {
    args.get(at)
        .and_then(|&id| cx.body.value(id))
        .is_some_and(|value| !matches!(value.kind, ValueKind::Constant(Constant::Undef)))
}

/// Whether operand `at` is a constant zero, or unstated.
fn zero(cx: &FnCx<'_>, args: &[ValueId], at: usize) -> bool {
    match args
        .get(at)
        .and_then(|&id| cx.body.value(id))
        .map(|value| &value.kind)
    {
        None | Some(ValueKind::Constant(Constant::Null | Constant::Undef | Constant::Int(0))) => {
            true
        }
        Some(ValueKind::Constant(Constant::Float(bits))) => f32::from_bits(*bits as u32) == 0.0,
        _ => false,
    }
}

fn arg(args: &[ValueId], at: usize) -> Result<ValueId, TranslateError> {
    args.get(at).copied().ok_or(TranslateError::Inst(
        "a dx.op call with fewer operands than its opcode takes",
    ))
}

fn operand(cx: &mut FnCx<'_>, args: &[ValueId], at: usize) -> Result<Handle<E>, TranslateError> {
    let id = arg(args, at)?;
    cx.value(id)
}

fn operands(
    cx: &mut FnCx<'_>,
    args: &[ValueId],
    from: usize,
    n: usize,
) -> Result<Vec<Handle<E>>, TranslateError> {
    (from..from + n).map(|at| operand(cx, args, at)).collect()
}

fn not_constant(op: DxOp, what: &'static str) -> TranslateError {
    TranslateError::NotConstant {
        op: op.name().into_owned(),
        what,
    }
}

fn math(cx: &mut FnCx<'_>, fun: Mf, arg: Handle<E>, rest: &[Handle<E>]) -> Handle<E> {
    cx.emit(E::Math {
        fun,
        arg,
        arg1: rest.first().copied(),
        arg2: rest.get(1).copied(),
        arg3: rest.get(2).copied(),
    })
}

fn bitcast(cx: &mut FnCx<'_>, expr: Handle<E>, kind: ScalarKind) -> Handle<E> {
    cx.emit(E::As {
        expr,
        kind,
        convert: None,
    })
}

fn bin(cx: &mut FnCx<'_>, op: Bo, left: Handle<E>, right: Handle<E>) -> Handle<E> {
    cx.emit(E::Binary { op, left, right })
}

fn lit(cx: &mut FnCx<'_>, literal: Literal) -> Handle<E> {
    cx.emit(E::Literal(literal))
}

#[cfg(test)]
mod tests {
    use naga::valid::{Capabilities, ValidationFlags, Validator};
    use or_io::gta5::shaders::spirv_pack::{Fidelity, SignatureElement};

    use super::super::ir::{BasicBlock, BinOp, Body, Inst, Module, Op, Type, Value};
    use super::super::plan::{BindingPlan, PlannedBinding};
    use super::super::structure::Node;
    use super::super::to_naga::{to_naga, Interface, Lowered};
    use super::super::ShaderKind;
    use super::*;

    const VOID: TypeId = 0;
    const FLOAT: TypeId = 1;
    const I32: TypeId = 2;
    const I8: TypeId = 3;
    const I1: TypeId = 4;
    const RES_BIND: TypeId = 5;
    const HANDLE: TypeId = 6;
    const CBUF_RET: TypeId = 7;
    const RES_RET: TypeId = 8;
    const DIMENSIONS: TypeId = 9;

    fn module() -> Module {
        let record = |name: &str, fields: Vec<TypeId>| Type::Struct {
            name: Some(name.into()),
            fields,
            packed: false,
        };
        Module {
            types: vec![
                Type::Void,
                Type::Float,
                Type::Int(32),
                Type::Int(8),
                Type::Int(1),
                record("dx.types.ResBind", vec![I32, I32, I32, I8]),
                record("dx.types.Handle", Vec::new()),
                record("dx.types.CBufRet.f32", vec![FLOAT; 4]),
                record("dx.types.ResRet.f32", vec![FLOAT, FLOAT, FLOAT, FLOAT, I32]),
                record("dx.types.Dimensions", vec![I32; 4]),
            ],
            ..Module::default()
        }
    }

    /// One block of instructions, a pixel program reading `TEXCOORD0` and writing `SV_Target0`.
    #[derive(Default)]
    struct Program {
        values: Vec<Value>,
        insts: Vec<Inst>,
    }

    impl Program {
        fn constant(&mut self, ty: TypeId, constant: Constant) -> ValueId {
            self.values.push(Value {
                ty,
                kind: ValueKind::Constant(constant),
                name: None,
            });
            self.values.len() as ValueId - 1
        }

        fn int(&mut self, ty: TypeId, value: i64) -> ValueId {
            self.constant(ty, Constant::Int(value))
        }

        fn float(&mut self, value: f32) -> ValueId {
            self.constant(FLOAT, Constant::Float(u64::from(value.to_bits())))
        }

        fn inst(&mut self, ty: TypeId, op: Op) -> ValueId {
            let id = self.values.len() as ValueId;
            let index = self.insts.len() as u32;
            self.values.push(Value {
                ty,
                kind: ValueKind::Inst { block: 0, index },
                name: None,
            });
            self.insts.push(Inst {
                result: Some(id),
                ty,
                op,
            });
            id
        }

        fn dx(
            &mut self,
            ty: TypeId,
            op: DxOp,
            overload: Option<TypeId>,
            args: Vec<ValueId>,
        ) -> ValueId {
            self.inst(ty, Op::Dx { op, overload, args })
        }

        fn effect(&mut self, op: DxOp, overload: Option<TypeId>, args: Vec<ValueId>) {
            self.insts.push(Inst {
                result: None,
                ty: VOID,
                op: Op::Dx { op, overload, args },
            });
        }

        fn input(&mut self, column: i64) -> ValueId {
            let args = vec![
                self.int(I32, 0),
                self.int(I32, 0),
                self.int(I8, column),
                self.int(I32, 0),
            ];
            self.dx(FLOAT, DxOp::LOAD_INPUT, Some(FLOAT), args)
        }

        fn store(&mut self, ty: TypeId, column: i64, value: ValueId) {
            let args = vec![
                self.int(I32, 0),
                self.int(I32, 0),
                self.int(I8, column),
                value,
            ];
            self.effect(DxOp::STORE_OUTPUT, Some(ty), args);
        }

        /// A handle of register `index` in space 0 of DXIL class `class` (0 SRV, 2 CBV, 3 sampler).
        fn bind(&mut self, class: i64, index: i64) -> ValueId {
            let (lower, upper, space, class) = (
                self.int(I32, index),
                self.int(I32, index),
                self.int(I32, 0),
                self.int(I8, class),
            );
            let binding = self.constant(
                RES_BIND,
                Constant::Aggregate(vec![lower, upper, space, class]),
            );
            let (at, non_uniform) = (self.int(I32, index), self.int(I1, 0));
            self.dx(
                HANDLE,
                DxOp::CREATE_HANDLE_FROM_BINDING,
                None,
                vec![binding, at, non_uniform],
            )
        }

        fn lower(mut self, plan: &mut BindingPlan) -> Result<Lowered, TranslateError> {
            self.insts.push(Inst {
                result: None,
                ty: VOID,
                op: Op::Ret { value: None },
            });
            let body = Body {
                blocks: vec![BasicBlock { insts: self.insts }],
                values: self.values,
            };
            let element = |semantic: &str, system_value, register, mask| SignatureElement {
                semantic: semantic.into(),
                index: 0,
                system_value,
                component_type: 3,
                register,
                mask,
            };
            let interface = Interface {
                kind: ShaderKind::Pixel,
                inputs: vec![element("TEXCOORD", 0, 1, 0x3)],
                outputs: vec![element("SV_Target", 64, 0, 0xF)],
                threads: None,
            };
            to_naga(
                &module(),
                &body,
                &[Node::Code(0), Node::Return(None)],
                &interface,
                plan,
            )
        }
    }

    fn valid(lowered: Result<Lowered, TranslateError>) -> Lowered {
        let lowered = lowered.unwrap();
        Validator::new(ValidationFlags::all(), Capabilities::default())
            .validate(&lowered.module)
            .unwrap();
        lowered
    }

    fn body_of(lowered: &Lowered) -> &naga::Function {
        lowered.module.functions.iter().next().unwrap().1
    }

    fn planned(class: ResourceClass, index: u32, binding: u32, planned: Planned) -> PlannedBinding {
        let register = Register {
            class,
            space: 0,
            index,
        };
        PlannedBinding {
            register,
            group: 1,
            binding,
            name: format!("r{binding}"),
            planned,
        }
    }

    #[test]
    fn arithmetic_lowers_to_naga_math_and_a_discard_to_a_kill() {
        let mut p = Program::default();
        let (x, y) = (p.input(0), p.input(1));
        let (two, half) = (p.float(2.0), p.float(0.5));
        let mad = p.dx(FLOAT, DxOp::FMAD, Some(FLOAT), vec![x, two, half]);
        let dot = p.dx(FLOAT, DxOp::DOT2, Some(FLOAT), vec![x, y, y, x]);
        let most = p.dx(FLOAT, DxOp::FMAX, Some(FLOAT), vec![mad, dot]);
        let nan = p.dx(I1, DxOp::IS_NAN, Some(FLOAT), vec![most]);
        p.effect(DxOp::DISCARD, None, vec![nan]);
        let bits = p.dx(I32, DxOp::BITCAST_F32_TO_I32, None, vec![most]);
        let (width, offset) = (p.int(I32, 5), p.int(I32, 3));
        let field = p.dx(I32, DxOp::UBFE, Some(I32), vec![width, offset, bits]);
        let top = p.dx(I32, DxOp::FIRSTBIT_HI, Some(I32), vec![field]);
        p.store(FLOAT, 0, most);
        p.store(I32, 1, field);
        p.store(I32, 2, top);
        p.store(I1, 3, nan);
        let lowered = valid(p.lower(&mut BindingPlan::default()));

        let body = body_of(&lowered);
        let kills = body.body.iter().any(|s| match s {
            Statement::If { accept, .. } => accept.iter().any(|s| matches!(s, Statement::Kill)),
            _ => false,
        });
        assert!(kills);
        let funs: Vec<Mf> = body
            .expressions
            .iter()
            .filter_map(|(_, e)| match *e {
                E::Math { fun, .. } => Some(fun),
                _ => None,
            })
            .collect();
        for fun in [Mf::Dot, Mf::Max, Mf::ExtractBits, Mf::CountLeadingZeros] {
            assert!(funs.contains(&fun), "{fun:?}");
        }
        // FMad is a multiply and an add, which a fused multiply-add is not.
        assert!(!funs.contains(&Mf::Fma));
        assert_eq!(lowered.fidelity, Fidelity::Equivalent);
    }

    #[test]
    fn a_constant_row_and_a_sample_read_their_planned_bindings() {
        let mut p = Program::default();
        let constants = p.bind(2, 4);
        let texture = p.bind(0, 1);
        let sampler = p.bind(3, 6);
        let row_index = p.int(I32, 3);
        let row = p.dx(
            CBUF_RET,
            DxOp::CBUFFER_LOAD_LEGACY,
            Some(FLOAT),
            vec![constants, row_index],
        );
        let scale = p.inst(
            FLOAT,
            Op::ExtractValue {
                aggregate: row,
                indices: vec![2],
            },
        );
        let (u, v) = (p.input(0), p.input(1));
        let (f, i) = (
            p.constant(FLOAT, Constant::Undef),
            p.constant(I32, Constant::Undef),
        );
        let texel = p.dx(
            RES_RET,
            DxOp::SAMPLE,
            Some(FLOAT),
            vec![texture, sampler, u, v, f, f, i, i, i, f],
        );
        let red = p.inst(
            FLOAT,
            Op::ExtractValue {
                aggregate: texel,
                indices: vec![0],
            },
        );
        let status = p.inst(
            I32,
            Op::ExtractValue {
                aggregate: texel,
                indices: vec![4],
            },
        );
        let mapped = p.dx(I1, DxOp::CHECK_ACCESS_FULLY_MAPPED, None, vec![status]);
        let lit = p.inst(
            FLOAT,
            Op::Binary {
                op: BinOp::FMul,
                lhs: red,
                rhs: scale,
            },
        );
        p.store(FLOAT, 0, lit);
        p.store(I1, 1, mapped);
        let flat = Planned::Texture {
            dim: naga::ImageDimension::D2,
            arrayed: false,
            multisampled: false,
            sampled: ScalarKind::Float,
        };
        let mut plan = BindingPlan {
            bindings: vec![
                planned(ResourceClass::Cbv, 4, 0, Planned::Uniform { rows: 8 }),
                planned(ResourceClass::Srv, 1, 1, flat),
                planned(
                    ResourceClass::Sampler,
                    6,
                    2,
                    Planned::Sampler { comparison: false },
                ),
            ],
        };
        let lowered = valid(p.lower(&mut plan));

        let uniform = lowered
            .module
            .global_variables
            .iter()
            .find(|(_, g)| g.name.as_deref() == Some("r0"));
        let (_, uniform) = uniform.unwrap();
        assert!(matches!(uniform.space, naga::AddressSpace::Uniform));
        assert_eq!(
            uniform.binding,
            Some(naga::ResourceBinding {
                group: 1,
                binding: 0
            })
        );
        let body = body_of(&lowered);
        let sampled = body.expressions.iter().any(|(_, e)| {
            matches!(
                e,
                E::ImageSample {
                    level: SampleLevel::Auto,
                    offset: None,
                    depth_ref: None,
                    gather: None,
                    ..
                }
            )
        });
        assert!(sampled);
        assert_eq!(lowered.fidelity, Fidelity::Equivalent);
    }

    #[test]
    fn a_structured_load_reads_the_words_its_mask_names_and_its_size_counts_elements() {
        let mut p = Program::default();
        let items = p.bind(0, 2);
        let (index, offset, mask, align) =
            (p.int(I32, 3), p.int(I32, 8), p.int(I8, 0b11), p.int(I32, 4));
        let loaded = p.dx(
            RES_RET,
            DxOp::RAW_BUFFER_LOAD,
            Some(FLOAT),
            vec![items, index, offset, mask, align],
        );
        let second = p.inst(
            FLOAT,
            Op::ExtractValue {
                aggregate: loaded,
                indices: vec![1],
            },
        );
        let level = p.constant(I32, Constant::Undef);
        let size = p.dx(DIMENSIONS, DxOp::GET_DIMENSIONS, None, vec![items, level]);
        let count = p.inst(
            I32,
            Op::ExtractValue {
                aggregate: size,
                indices: vec![0],
            },
        );
        p.store(FLOAT, 0, second);
        p.store(I32, 1, count);
        let structured = Planned::Storage {
            read_only: true,
            stride: Some(24),
        };
        let mut plan = BindingPlan {
            bindings: vec![planned(ResourceClass::Srv, 2, 0, structured)],
        };
        let lowered = valid(p.lower(&mut plan));

        let body = body_of(&lowered);
        let loads = body
            .expressions
            .iter()
            .filter(|(_, e)| match **e {
                E::Load { pointer } => matches!(body.expressions[pointer], E::Access { .. }),
                _ => false,
            })
            .count();
        assert_eq!(loads, 2);
        assert!(body
            .expressions
            .iter()
            .any(|(_, e)| matches!(e, E::ArrayLength(_))));
    }

    #[test]
    fn an_intrinsic_not_in_the_table_is_refused_by_its_name() {
        let mut p = Program::default();
        let x = p.input(0);
        let bits = p.dx(I32, DxOp::BITCAST_F32_TO_I32, None, vec![x]);
        let (reduction, signed) = (p.int(I8, 0), p.int(I8, 1));
        p.dx(
            I32,
            DxOp::WAVE_ACTIVE_OP,
            Some(I32),
            vec![bits, reduction, signed],
        );
        let refused = p.lower(&mut BindingPlan::default()).err();
        assert_eq!(refused, Some(TranslateError::Op("WaveActiveOp".into())));
    }
}
