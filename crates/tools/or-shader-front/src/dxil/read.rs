//! LLVM 3.7 bitcode blocks to the instruction model: the type table, globals, functions and their
//! bodies, constants, names and metadata, with relative operand numbering resolved.

use super::bitstream::{BitcodeError, Block, Item, Record};
use super::ir::{
    BasicBlock, BinOp, BlockId, Body, CastOp, Constant, DxOp, Function, Inst, Md, Module, Op, Pred,
    Type, TypeId, Value, ValueId, ValueKind,
};

const MODULE: u32 = 8;
const CONSTANTS: u32 = 11;
const FUNCTION: u32 = 12;
const SYMTAB: u32 = 14;
const METADATA: u32 = 15;
const TYPES: u32 = 17;

/// The module of `blocks`, operand ids absolute; a phi's forward reference keeps the id it names.
pub fn module(blocks: &[Block]) -> Result<Module, BitcodeError> {
    let block = blocks
        .iter()
        .find(|block| block.id == MODULE)
        .ok_or_else(|| BitcodeError::Unsupported("bitcode with no module block".into()))?;
    let mut m = Module::default();
    let mut relative = false;
    let mut bodies = Vec::new();
    let mut names = Vec::new();
    // Bodies are read last: they number their values after every module value.
    for item in &block.items {
        match item {
            Item::Block(sub) => match sub.id {
                TYPES => m.types = types(sub)?,
                CONSTANTS => constants(sub, &m.types, &mut m.values)?,
                METADATA => metadata(sub, &mut m)?,
                SYMTAB => names.extend(symbols(sub)?),
                _ => {}
            },
            Item::Record(record) => match record.code {
                1 => {
                    relative = match record.ops.first().copied() {
                        None | Some(0) => false,
                        Some(1) => true,
                        Some(version) => {
                            return Err(bad(MODULE, 1, format!("module version {version}")));
                        }
                    }
                }
                7 => global(&mut m, record)?,
                8 => {
                    if let Some(index) = function(&mut m, record)? {
                        bodies.push(index);
                    }
                }
                9 | 14 => return Err(bad(MODULE, record.code, "an alias, which DXIL has none of")),
                _ => {}
            },
        }
    }
    name(&mut m.values, &mut m.functions, names)?;

    let functions: Vec<&Block> = block
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Block(sub) if sub.id == FUNCTION => Some(sub),
            _ => None,
        })
        .collect();
    if functions.len() != bodies.len() {
        return Err(bad(
            MODULE,
            8,
            format!("{} bodies declared, {} read", bodies.len(), functions.len()),
        ));
    }
    for (index, sub) in bodies.into_iter().zip(functions) {
        let read = body(&mut m, index, sub, relative)?;
        if let Some(function) = m.functions.get_mut(index) {
            function.body = Some(read);
        }
    }
    Ok(m)
}

fn bad(block: u32, code: u32, why: impl Into<String>) -> BitcodeError {
    BitcodeError::Record {
        block,
        code,
        why: why.into(),
    }
}

/// A record's operands, read in order.
struct Ops<'r> {
    block: u32,
    record: &'r Record,
    at: usize,
}

impl<'r> Ops<'r> {
    fn new(block: u32, record: &'r Record) -> Self {
        Self {
            block,
            record,
            at: 0,
        }
    }

    fn err(&self, why: impl Into<String>) -> BitcodeError {
        bad(self.block, self.record.code, why)
    }

    fn left(&self) -> usize {
        self.record.ops.len().saturating_sub(self.at)
    }

    fn next(&mut self) -> Result<u64, BitcodeError> {
        let op = self.record.ops.get(self.at).copied();
        let op = op.ok_or_else(|| self.err("the record ends before an operand it needs"))?;
        self.at += 1;
        Ok(op)
    }

    fn u32(&mut self) -> Result<u32, BitcodeError> {
        let op = self.next()?;
        u32::try_from(op).map_err(|_| self.err(format!("operand {op} does not fit 32 bits")))
    }

    fn ty(&mut self, types: &[Type]) -> Result<TypeId, BitcodeError> {
        let id = self.u32()?;
        if (id as usize) < types.len() {
            Ok(id)
        } else {
            Err(self.err(format!("type {id} is not in the type table")))
        }
    }

    /// The operands from `from` on as bytes, one character each.
    fn text(&self, from: usize) -> Result<Vec<u8>, BitcodeError> {
        let chars = self.record.ops.get(from..).unwrap_or_default();
        chars
            .iter()
            .map(|&c| u8::try_from(c).map_err(|_| self.err("a character wider than a byte")))
            .collect()
    }
}

fn ids(ops: &mut Ops) -> Result<Vec<u32>, BitcodeError> {
    (0..ops.left()).map(|_| ops.u32()).collect()
}

fn string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A sign-rotated VBR value: the sign in bit 0, the magnitude above it.
fn signed(raw: u64) -> i64 {
    match (raw & 1, raw >> 1) {
        (0, magnitude) => magnitude as i64,
        (_, 0) => i64::MIN,
        (_, magnitude) => -(magnitude as i64),
    }
}

/// The id of `ty`, appended to the table when no equal type is in it.
fn intern(types: &mut Vec<Type>, ty: Type) -> TypeId {
    match types.iter().position(|known| *known == ty) {
        Some(at) => at as TypeId,
        None => {
            types.push(ty);
            (types.len() - 1) as TypeId
        }
    }
}

fn pointee(types: &[Type], ty: TypeId) -> Option<(TypeId, u32)> {
    match types.get(ty as usize)? {
        Type::Pointer { pointee, space } => Some((*pointee, *space)),
        _ => None,
    }
}

/// The type one index into `ty` reaches; a struct needs the index, an array or a vector does not.
fn field(types: &[Type], ty: TypeId, index: Option<i64>) -> Option<TypeId> {
    match types.get(ty as usize)? {
        Type::Struct { fields, .. } => fields.get(usize::try_from(index?).ok()?).copied(),
        Type::Array { elem, .. } | Type::Vector { elem, .. } => Some(*elem),
        _ => None,
    }
}

fn is_fp(types: &[Type], ty: TypeId) -> bool {
    let scalar = match types.get(ty as usize) {
        Some(Type::Vector { elem, .. }) => types.get(*elem as usize),
        other => other,
    };
    matches!(scalar, Some(Type::Half | Type::Float | Type::Double))
}

/// The type a `dx.op.<name>.<overload>` function name ends in.
fn overload(types: &mut Vec<Type>, name: &str) -> Option<TypeId> {
    let (_, suffix) = name.strip_prefix("dx.op.")?.split_once('.')?;
    let ty = match suffix {
        "f16" => Type::Half,
        "f32" => Type::Float,
        "f64" => Type::Double,
        _ => match suffix.strip_prefix('i')?.parse::<u32>().ok()? {
            width @ (1 | 8 | 16 | 32 | 64) => Type::Int(width),
            _ => return None,
        },
    };
    Some(intern(types, ty))
}

fn cast(code: u64) -> Option<CastOp> {
    use CastOp as C;
    const CASTS: [CastOp; 13] = [
        C::Trunc,
        C::ZExt,
        C::SExt,
        C::FpToUi,
        C::FpToSi,
        C::UiToFp,
        C::SiToFp,
        C::FpTrunc,
        C::FpExt,
        C::PtrToInt,
        C::IntToPtr,
        C::Bitcast,
        C::AddrSpaceCast,
    ];
    CASTS.get(usize::try_from(code).ok()?).copied()
}

/// LLVM's binary opcode, which names the floating-point form for a floating-point type.
fn binop(code: u64, fp: bool) -> Option<BinOp> {
    use BinOp as B;
    Some(match (code, fp) {
        (0, false) => B::Add,
        (0, true) => B::FAdd,
        (1, false) => B::Sub,
        (1, true) => B::FSub,
        (2, false) => B::Mul,
        (2, true) => B::FMul,
        (3, false) => B::UDiv,
        (4, false) => B::SDiv,
        (4, true) => B::FDiv,
        (5, false) => B::URem,
        (6, false) => B::SRem,
        (6, true) => B::FRem,
        (7, false) => B::Shl,
        (8, false) => B::LShr,
        (9, false) => B::AShr,
        (10, false) => B::And,
        (11, false) => B::Or,
        (12, false) => B::Xor,
        _ => return None,
    })
}

/// LLVM's predicate numbers: 0 to 15 for floats, 32 to 41 for integers.
fn pred(code: u64) -> Option<Pred> {
    use Pred as P;
    const FLOAT: [Pred; 16] = [
        P::FFalse,
        P::FOeq,
        P::FOgt,
        P::FOge,
        P::FOlt,
        P::FOle,
        P::FOne,
        P::FOrd,
        P::FUno,
        P::FUeq,
        P::FUgt,
        P::FUge,
        P::FUlt,
        P::FUle,
        P::FUne,
        P::FTrue,
    ];
    const INT: [Pred; 10] = [
        P::IEq,
        P::INe,
        P::IUgt,
        P::IUge,
        P::IUlt,
        P::IUle,
        P::ISgt,
        P::ISge,
        P::ISlt,
        P::ISle,
    ];
    match code {
        0..=15 => FLOAT.get(code as usize).copied(),
        32..=41 => INT.get((code - 32) as usize).copied(),
        _ => None,
    }
}

fn types(block: &Block) -> Result<Vec<Type>, BitcodeError> {
    let mut types = Vec::new();
    // The name a struct-name record gives the struct or opaque record after it.
    let mut name: Option<String> = None;
    for item in &block.items {
        let Item::Record(record) = item else { continue };
        let mut ops = Ops::new(TYPES, record);
        let ty = match record.code {
            1 => continue,
            2 => Type::Void,
            3 => Type::Float,
            4 => Type::Double,
            5 => Type::Label,
            6 => Type::Opaque(name.take()),
            7 => Type::Int(ops.u32()?),
            8 => {
                let pointee = ops.u32()?;
                let space = if ops.left() > 0 { ops.u32()? } else { 0 };
                Type::Pointer { pointee, space }
            }
            // The old form carries an attribute id before the return type.
            9 | 21 => {
                let vararg = ops.next()? != 0;
                if record.code == 9 {
                    ops.next()?;
                }
                let ret = ops.u32()?;
                Type::Function {
                    ret,
                    params: ids(&mut ops)?,
                    vararg,
                }
            }
            10 => Type::Half,
            11 => {
                let len = ops.next()?;
                Type::Array {
                    len,
                    elem: ops.u32()?,
                }
            }
            12 => {
                let len = ops.u32()?;
                Type::Vector {
                    len,
                    elem: ops.u32()?,
                }
            }
            16 => Type::Metadata,
            18 | 20 => {
                let packed = ops.next()? != 0;
                let fields = ids(&mut ops)?;
                let name = if record.code == 20 { name.take() } else { None };
                Type::Struct {
                    name,
                    fields,
                    packed,
                }
            }
            19 => {
                name = Some(string(&ops.text(0)?));
                continue;
            }
            code => return Err(ops.err(format!("type code {code}, which DXIL does not use"))),
        };
        types.push(ty);
    }
    let len = types.len();
    let known = |id: &TypeId| (*id as usize) < len;
    for (at, ty) in types.iter().enumerate() {
        let held = match ty {
            Type::Pointer { pointee, .. } => known(pointee),
            Type::Array { elem, .. } | Type::Vector { elem, .. } => known(elem),
            Type::Struct { fields, .. } => fields.iter().all(known),
            Type::Function { ret, params, .. } => known(ret) && params.iter().all(known),
            _ => true,
        };
        if !held {
            return Err(bad(
                TYPES,
                0,
                format!("type {at} names a type the table does not hold"),
            ));
        }
    }
    Ok(types)
}

/// Constant ids are absolute in every constants block, the module's and a function's.
fn constants(block: &Block, types: &[Type], values: &mut Vec<Value>) -> Result<(), BitcodeError> {
    let mut current: Option<TypeId> = None;
    for item in &block.items {
        let Item::Record(record) = item else { continue };
        let mut ops = Ops::new(CONSTANTS, record);
        if record.code == 1 {
            current = Some(ops.ty(types)?);
            continue;
        }
        let ty = current.ok_or_else(|| ops.err("a constant before its type is set"))?;
        let constant = match record.code {
            2 => Constant::Null,
            3 => Constant::Undef,
            // Sign-extended from the type's width, as stored: an i1 true reads -1.
            4 => Constant::Int(signed(ops.next()?)),
            6 => Constant::Float(ops.next()?),
            7 => Constant::Aggregate(ids(&mut ops)?),
            8 => Constant::String(ops.text(0)?),
            // A C string is stored without its terminating zero.
            9 => {
                let mut text = ops.text(0)?;
                text.push(0);
                Constant::String(text)
            }
            // [opcode, operand type, operand]
            11 => {
                let code = ops.next()?;
                let op = cast(code).ok_or_else(|| ops.err(format!("cast opcode {code}")))?;
                ops.ty(types)?;
                Constant::Cast {
                    op,
                    value: ops.u32()?,
                }
            }
            // [source type when the count is odd, then (type, operand) pairs]
            12 | 20 => {
                if record.ops.len() % 2 == 1 {
                    ops.ty(types)?;
                }
                let mut operands = Vec::with_capacity(ops.left() / 2);
                while ops.left() > 0 {
                    ops.ty(types)?;
                    operands.push(ops.u32()?);
                }
                Constant::Gep {
                    inbounds: record.code == 20,
                    operands,
                }
            }
            22 => Constant::Data(record.ops.clone()),
            code => return Err(ops.err(format!("constant code {code} is not read"))),
        };
        values.push(Value {
            ty,
            kind: ValueKind::Constant(constant),
            name: None,
        });
    }
    Ok(())
}

fn metadata(block: &Block, m: &mut Module) -> Result<(), BitcodeError> {
    // The name a name record gives the named node after it.
    let mut pending: Option<String> = None;
    for item in &block.items {
        let Item::Record(record) = item else { continue };
        let mut ops = Ops::new(METADATA, record);
        let md = match record.code {
            1 => Md::String(ops.text(0)?),
            2 => Md::Value {
                ty: ops.ty(&m.types)?,
                value: ops.u32()?,
            },
            // Each operand is its node's id plus one; zero is no node.
            3 | 5 => Md::Node(
                record
                    .ops
                    .iter()
                    .map(|&op| match op.checked_sub(1) {
                        None => Ok(None),
                        Some(id) => u32::try_from(id)
                            .map(Some)
                            .map_err(|_| ops.err("a node operand wider than 32 bits")),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            4 => {
                pending = Some(string(&ops.text(0)?));
                continue;
            }
            6 => continue,
            10 => {
                let name = pending
                    .take()
                    .ok_or_else(|| ops.err("a named node with no name"))?;
                let nodes = ids(&mut ops)?;
                m.named.insert(name, nodes);
                continue;
            }
            // Debug information keeps its number; its operands are not read.
            7 | 12..=31 => Md::Node(Vec::new()),
            code => return Err(ops.err(format!("metadata code {code} is not read"))),
        };
        m.metadata.push(md);
    }
    Ok(())
}

/// Value names; block names are not kept.
fn symbols(block: &Block) -> Result<Vec<(ValueId, String)>, BitcodeError> {
    let mut names = Vec::new();
    for item in &block.items {
        let Item::Record(record) = item else { continue };
        // An entry is [value, chars]; a function entry [value, offset, chars].
        let from = match record.code {
            1 => 1,
            3 => 2,
            _ => continue,
        };
        let mut ops = Ops::new(SYMTAB, record);
        let id = ops.u32()?;
        names.push((id, string(&ops.text(from)?)));
    }
    Ok(names)
}

fn name(
    values: &mut [Value],
    functions: &mut [Function],
    names: Vec<(ValueId, String)>,
) -> Result<(), BitcodeError> {
    for (id, name) in names {
        let value = values.get_mut(id as usize).ok_or_else(|| {
            bad(
                SYMTAB,
                1,
                format!("a name for value {id}, which is not defined"),
            )
        })?;
        if let ValueKind::Function(index) = value.kind {
            if let Some(function) = functions.get_mut(index as usize) {
                function.name = name.clone();
            }
        }
        value.name = Some(name);
    }
    Ok(())
}

/// [type, flags, init + 1, ...]; flags hold constness in bit 0, an explicit type in bit 1 and the
/// address space above.
fn global(m: &mut Module, record: &Record) -> Result<(), BitcodeError> {
    let mut ops = Ops::new(MODULE, record);
    let ty = ops.ty(&m.types)?;
    let flags = ops.next()?;
    let init = ops.u32()?.checked_sub(1);
    let (pointee, space) = if flags & 2 != 0 {
        let space = u32::try_from(flags >> 2);
        let space = space.map_err(|_| ops.err("an address space past 32 bits"))?;
        (ty, space)
    } else {
        pointee(&m.types, ty).ok_or_else(|| ops.err("a global whose type is no pointer"))?
    };
    let ptr = intern(&mut m.types, Type::Pointer { pointee, space });
    let kind = ValueKind::Global {
        space,
        init,
        constant: flags & 1 != 0,
    };
    m.values.push(Value {
        ty: ptr,
        kind,
        name: None,
    });
    Ok(())
}

/// [type, calling convention, declaration, ...]: the function's index when it has a body.
fn function(m: &mut Module, record: &Record) -> Result<Option<usize>, BitcodeError> {
    let mut ops = Ops::new(MODULE, record);
    let ty = ops.ty(&m.types)?;
    let ty = pointee(&m.types, ty).map_or(ty, |(pointee, _)| pointee);
    if !matches!(m.types.get(ty as usize), Some(Type::Function { .. })) {
        return Err(ops.err("a function whose type is no function type"));
    }
    ops.next()?;
    let declaration = ops.next()? != 0;
    let index = m.functions.len();
    let ptr = intern(
        &mut m.types,
        Type::Pointer {
            pointee: ty,
            space: 0,
        },
    );
    m.values.push(Value {
        ty: ptr,
        kind: ValueKind::Function(index as u32),
        name: None,
    });
    m.functions.push(Function {
        name: String::new(),
        ty,
        body: None,
    });
    Ok((!declaration).then_some(index))
}

fn body(m: &mut Module, index: usize, block: &Block, relative: bool) -> Result<Body, BitcodeError> {
    let params = match m
        .functions
        .get(index)
        .and_then(|f| m.types.get(f.ty as usize))
    {
        Some(Type::Function { params, .. }) => params.clone(),
        _ => {
            return Err(bad(
                MODULE,
                8,
                format!("function {index} has no function type"),
            ))
        }
    };
    let mut types = std::mem::take(&mut m.types);
    let void = intern(&mut types, Type::Void);
    let mut values = m.values.clone();
    for (at, &ty) in params.iter().enumerate() {
        values.push(Value {
            ty,
            kind: ValueKind::Argument(at as u32),
            name: None,
        });
    }
    let mut reader = Reader {
        types,
        functions: &m.functions,
        body: Body {
            blocks: Vec::new(),
            values,
        },
        current: 0,
        relative,
        forward: Vec::new(),
        void,
    };
    let mut names = Vec::new();
    for item in &block.items {
        match item {
            Item::Block(sub) if sub.id == CONSTANTS => {
                constants(sub, &reader.types, &mut reader.body.values)?;
            }
            Item::Block(sub) if sub.id == SYMTAB => names.extend(symbols(sub)?),
            // Function metadata, attachments and use lists are not read.
            Item::Block(_) => {}
            Item::Record(record) => reader.inst(record, block.items.len())?,
        }
    }
    let Reader {
        types,
        mut body,
        current,
        forward,
        ..
    } = reader;
    m.types = types;
    if body.blocks.is_empty() || current != body.blocks.len() {
        return Err(bad(
            FUNCTION,
            1,
            format!(
                "the body ends inside block {current} of {}",
                body.blocks.len()
            ),
        ));
    }
    if let Some((id, code)) = forward
        .iter()
        .find(|(id, _)| *id as usize >= body.values.len())
    {
        return Err(bad(
            FUNCTION,
            *code,
            format!("value {id} is referenced and never defined"),
        ));
    }
    name(&mut body.values, &mut [], names)?;
    Ok(body)
}

/// One function body being read.
struct Reader<'m> {
    types: Vec<Type>,
    functions: &'m [Function],
    body: Body,
    /// The block instructions go to; a terminator moves to the next.
    current: usize,
    relative: bool,
    /// Ids named before their value, each with the record code that named it.
    forward: Vec<(ValueId, u32)>,
    void: TypeId,
}

impl Reader<'_> {
    /// An operand id: relative to the next value's id when the module numbers so, wrapping as
    /// LLVM's unsigned arithmetic does, so a forward reference lands past the values read.
    fn id(&self, ops: &mut Ops) -> Result<ValueId, BitcodeError> {
        let op = ops.u32()?;
        Ok(if self.relative {
            (self.body.values.len() as u32).wrapping_sub(op)
        } else {
            op
        })
    }

    fn note(&mut self, id: ValueId, code: u32) {
        if id as usize >= self.body.values.len() {
            self.forward.push((id, code));
        }
    }

    /// A value and its type; the record states the type only for a forward reference.
    fn pair(&mut self, ops: &mut Ops) -> Result<(ValueId, TypeId), BitcodeError> {
        let id = self.id(ops)?;
        if let Some(value) = self.body.values.get(id as usize) {
            return Ok((id, value.ty));
        }
        let ty = ops.ty(&self.types)?;
        self.note(id, ops.record.code);
        Ok((id, ty))
    }

    /// A value whose type the instruction implies.
    fn typed(&mut self, ops: &mut Ops) -> Result<ValueId, BitcodeError> {
        let id = self.id(ops)?;
        self.note(id, ops.record.code);
        Ok(id)
    }

    fn block(&self, ops: &mut Ops) -> Result<BlockId, BitcodeError> {
        let id = ops.u32()?;
        if (id as usize) < self.body.blocks.len() {
            Ok(id)
        } else {
            Err(ops.err(format!("block {id} is not declared")))
        }
    }

    fn vector(&self, ty: TypeId) -> Option<(u32, TypeId)> {
        match self.types.get(ty as usize)? {
            Type::Vector { len, elem } => Some((*len, *elem)),
            _ => None,
        }
    }

    fn push(&mut self, ops: &Ops, ty: TypeId, op: Op, defines: bool) -> Result<(), BitcodeError> {
        let block = self.current;
        let terminator = matches!(
            op,
            Op::Br { .. }
                | Op::CondBr { .. }
                | Op::Switch { .. }
                | Op::Ret { .. }
                | Op::Unreachable
        );
        let next = self.body.values.len() as ValueId;
        let Some(insts) = self.body.blocks.get_mut(block).map(|b| &mut b.insts) else {
            return Err(ops.err("an instruction after the last declared block"));
        };
        let result = defines.then_some(next);
        if defines {
            let kind = ValueKind::Inst {
                block: block as BlockId,
                index: insts.len() as u32,
            };
            self.body.values.push(Value {
                ty,
                kind,
                name: None,
            });
        }
        insts.push(Inst { result, ty, op });
        if terminator {
            self.current += 1;
        }
        Ok(())
    }

    /// `records` bounds the blocks a body may declare: each needs a terminator record.
    fn inst(&mut self, record: &Record, records: usize) -> Result<(), BitcodeError> {
        let mut ops = Ops::new(FUNCTION, record);
        let void = self.void;
        let (ty, op, defines) = match record.code {
            1 => {
                let count = ops.next()?;
                if !self.body.blocks.is_empty() || count == 0 || count > records as u64 {
                    return Err(ops.err(format!("{count} blocks in a body of {records} records")));
                }
                self.body
                    .blocks
                    .resize_with(count as usize, BasicBlock::default);
                return Ok(());
            }
            // Debug locations.
            33 | 35 => return Ok(()),
            // [lhs, rhs, opcode, flags]
            2 => {
                let (lhs, ty) = self.pair(&mut ops)?;
                let rhs = self.typed(&mut ops)?;
                let code = ops.next()?;
                let op = binop(code, is_fp(&self.types, ty))
                    .ok_or_else(|| ops.err(format!("binary opcode {code} on type {ty}")))?;
                (ty, Op::Binary { op, lhs, rhs }, true)
            }
            // [value, type, opcode]
            3 => {
                let (value, _) = self.pair(&mut ops)?;
                let ty = ops.ty(&self.types)?;
                let code = ops.next()?;
                let op = cast(code).ok_or_else(|| ops.err(format!("cast opcode {code}")))?;
                (ty, Op::Cast { op, value }, true)
            }
            4 | 30 | 43 => self.gep(&mut ops)?,
            // [vector, index]
            6 => {
                let (vector, vt) = self.pair(&mut ops)?;
                let (index, _) = self.pair(&mut ops)?;
                let (_, ty) = self
                    .vector(vt)
                    .ok_or_else(|| ops.err("an element of no vector"))?;
                (ty, Op::ExtractElement { vector, index }, true)
            }
            // [vector, element, index]
            7 => {
                let (vector, ty) = self.pair(&mut ops)?;
                let value = self.typed(&mut ops)?;
                let (index, _) = self.pair(&mut ops)?;
                (
                    ty,
                    Op::InsertElement {
                        vector,
                        value,
                        index,
                    },
                    true,
                )
            }
            // [a, b, mask]: as many elements as the mask has.
            8 => {
                let (a, at) = self.pair(&mut ops)?;
                let b = self.typed(&mut ops)?;
                let mask = self.typed(&mut ops)?;
                let mask_ty = self.body.values.get(mask as usize).map(|v| v.ty);
                let shape = mask_ty.and_then(|ty| self.vector(ty)).zip(self.vector(at));
                let Some(((len, _), (_, elem))) = shape else {
                    return Err(ops.err("a shuffle of no vectors or by a mask not yet defined"));
                };
                let ty = intern(&mut self.types, Type::Vector { len, elem });
                (ty, Op::ShuffleVector { a, b, mask }, true)
            }
            // [lhs, rhs, predicate]
            9 | 28 => {
                let (lhs, lt) = self.pair(&mut ops)?;
                let rhs = self.typed(&mut ops)?;
                let code = ops.next()?;
                let pred = pred(code).ok_or_else(|| ops.err(format!("predicate {code}")))?;
                let flag = intern(&mut self.types, Type::Int(1));
                let ty = match self.vector(lt) {
                    Some((len, _)) => intern(&mut self.types, Type::Vector { len, elem: flag }),
                    None => flag,
                };
                (ty, Op::Cmp { pred, lhs, rhs }, true)
            }
            // [value?]
            10 => {
                let value = if ops.left() == 0 {
                    None
                } else {
                    Some(self.pair(&mut ops)?.0)
                };
                (void, Op::Ret { value }, false)
            }
            // [target] or [then, otherwise, condition]
            11 => {
                let target = self.block(&mut ops)?;
                let op = if ops.left() == 0 {
                    Op::Br { target }
                } else {
                    let otherwise = self.block(&mut ops)?;
                    let cond = self.typed(&mut ops)?;
                    Op::CondBr {
                        cond,
                        then: target,
                        otherwise,
                    }
                };
                (void, op, false)
            }
            // [type, value, default, (case constant, block)...]; case constants are absolute.
            12 => {
                if record.ops.first().is_some_and(|op| op >> 16 == 0x4B5) {
                    return Err(ops.err("the case-range form of switch"));
                }
                ops.ty(&self.types)?;
                let value = self.typed(&mut ops)?;
                let default = self.block(&mut ops)?;
                let mut cases = Vec::with_capacity(ops.left() / 2);
                while ops.left() > 0 {
                    let id = ops.u32()?;
                    let case = self.body.int(id).ok_or_else(|| {
                        ops.err(format!("case value {id} is no integer constant"))
                    })?;
                    cases.push((case, self.block(&mut ops)?));
                }
                (
                    void,
                    Op::Switch {
                        value,
                        default,
                        cases,
                    },
                    false,
                )
            }
            15 => (void, Op::Unreachable, false),
            // [type, (value, block)...]; a relative value is sign-rotated, since a phi may name a
            // value defined after it.
            16 => {
                let ty = ops.ty(&self.types)?;
                if !ops.left().is_multiple_of(2) {
                    return Err(ops.err("a phi with an incoming value and no block"));
                }
                let mut incoming = Vec::with_capacity(ops.left() / 2);
                while ops.left() > 0 {
                    let raw = ops.next()?;
                    let id = if self.relative {
                        // Truncated to 32 bits, as LLVM's unsigned arithmetic does.
                        (self.body.values.len() as u32).wrapping_sub(signed(raw) as u32)
                    } else {
                        u32::try_from(raw).map_err(|_| ops.err("a phi operand past 32 bits"))?
                    };
                    self.note(id, record.code);
                    incoming.push((id, self.block(&mut ops)?));
                }
                (ty, Op::Phi { incoming }, true)
            }
            // [type, count type, count (absolute), alignment and flags]
            19 => {
                if record.ops.len() != 4 {
                    return Err(ops.err("an alloca of other than four operands"));
                }
                let mut ty = ops.ty(&self.types)?;
                ops.ty(&self.types)?;
                let count = ops.u32()?;
                self.note(count, record.code);
                // Bit 6: the type is the allocated one, not a pointer to it.
                if ops.next()? & (1 << 6) == 0 {
                    let allocated = pointee(&self.types, ty);
                    ty = allocated
                        .ok_or_else(|| ops.err("an alloca of no pointer type"))?
                        .0;
                }
                let ptr = intern(
                    &mut self.types,
                    Type::Pointer {
                        pointee: ty,
                        space: 0,
                    },
                );
                (ptr, Op::Alloca { ty, count }, true)
            }
            // [pointer, type?, alignment, volatile]
            20 => {
                let (ptr, pt) = self.pair(&mut ops)?;
                let ty = match ops.left() {
                    3 => ops.ty(&self.types)?,
                    2 => {
                        let loaded = pointee(&self.types, pt);
                        loaded
                            .ok_or_else(|| ops.err("a load through no pointer"))?
                            .0
                    }
                    _ => return Err(ops.err("a load of other than two or three trailing operands")),
                };
                (ty, Op::Load { ptr }, true)
            }
            // [aggregate, indices...]
            26 => {
                let (aggregate, mut ty) = self.pair(&mut ops)?;
                let indices = ids(&mut ops)?;
                if indices.is_empty() {
                    return Err(ops.err("an extractvalue with no index"));
                }
                for &index in &indices {
                    ty = field(&self.types, ty, Some(i64::from(index)))
                        .ok_or_else(|| ops.err(format!("index {index} past its aggregate")))?;
                }
                (ty, Op::ExtractValue { aggregate, indices }, true)
            }
            // [aggregate, value, indices...]
            27 => {
                let (aggregate, ty) = self.pair(&mut ops)?;
                let (value, _) = self.pair(&mut ops)?;
                let indices = ids(&mut ops)?;
                if indices.is_empty() {
                    return Err(ops.err("an insertvalue with no index"));
                }
                (
                    ty,
                    Op::InsertValue {
                        aggregate,
                        value,
                        indices,
                    },
                    true,
                )
            }
            // [then, otherwise, condition]
            29 => {
                let (then, ty) = self.pair(&mut ops)?;
                let otherwise = self.typed(&mut ops)?;
                let (cond, _) = self.pair(&mut ops)?;
                (
                    ty,
                    Op::Select {
                        cond,
                        then,
                        otherwise,
                    },
                    true,
                )
            }
            34 => self.call(&mut ops)?,
            // [pointer, value, operation, volatile, ordering, scope]
            38 => {
                let (ptr, pt) = self.pair(&mut ops)?;
                let value = self.typed(&mut ops)?;
                let op = ops.u32()?;
                let held = pointee(&self.types, pt);
                let ty = held
                    .ok_or_else(|| ops.err("an atomic through no pointer"))?
                    .0;
                (ty, Op::AtomicRmw { op, ptr, value }, true)
            }
            // [pointer, value, alignment, volatile]
            44 => {
                let (ptr, _) = self.pair(&mut ops)?;
                let (value, _) = self.pair(&mut ops)?;
                (void, Op::Store { ptr, value }, false)
            }
            // [pointer, expected, new, ...]: the old value and whether it was swapped.
            46 => {
                let (ptr, _) = self.pair(&mut ops)?;
                let (expected, et) = self.pair(&mut ops)?;
                let new = self.typed(&mut ops)?;
                let flag = intern(&mut self.types, Type::Int(1));
                let pair = Type::Struct {
                    name: None,
                    fields: vec![et, flag],
                    packed: false,
                };
                let ty = intern(&mut self.types, pair);
                (ty, Op::CmpXchg { ptr, expected, new }, true)
            }
            code => return Err(ops.err(format!("instruction code {code} is not read"))),
        };
        self.push(&ops, ty, op, defines)
    }

    /// [inbounds, source type, base, indices...]; the old forms carry no flag and no type.
    fn gep(&mut self, ops: &mut Ops) -> Result<(TypeId, Op, bool), BitcodeError> {
        let code = ops.record.code;
        let (inbounds, source) = if code == 43 {
            (ops.next()? != 0, Some(ops.ty(&self.types)?))
        } else {
            (code == 30, None)
        };
        let (base, bt) = self.pair(ops)?;
        let mut indices = Vec::with_capacity(ops.left());
        while ops.left() > 0 {
            indices.push(self.pair(ops)?.0);
        }
        let (pointee, space) =
            pointee(&self.types, bt).ok_or_else(|| ops.err("a getelementptr of no pointer"))?;
        // The first index steps over the pointer; each after it steps into the type.
        let mut ty = source.unwrap_or(pointee);
        for &index in indices.iter().skip(1) {
            ty = field(&self.types, ty, self.body.int(index))
                .ok_or_else(|| ops.err("a getelementptr index past its type"))?;
        }
        let ty = intern(&mut self.types, Type::Pointer { pointee: ty, space });
        Ok((
            ty,
            Op::Gep {
                inbounds,
                base,
                indices,
            },
            true,
        ))
    }

    /// [attributes, flags, fast-math?, type?, callee, arguments...]: flag bit 17 adds the
    /// fast-math operand, bit 15 the explicit function type.
    fn call(&mut self, ops: &mut Ops) -> Result<(TypeId, Op, bool), BitcodeError> {
        ops.next()?;
        let flags = ops.next()?;
        if (flags >> 17) & 1 != 0 {
            ops.next()?;
        }
        let explicit = if (flags >> 15) & 1 != 0 {
            Some(ops.ty(&self.types)?)
        } else {
            None
        };
        let (callee, ct) = self.pair(ops)?;
        let ty = match explicit {
            Some(ty) => ty,
            None => {
                pointee(&self.types, ct)
                    .ok_or_else(|| ops.err("a call of no pointer"))?
                    .0
            }
        };
        let function = self.types.get(ty as usize).cloned();
        let Some(Type::Function {
            ret,
            params,
            vararg,
        }) = function
        else {
            return Err(ops.err("a call through a type that is no function type"));
        };
        let mut args = Vec::with_capacity(params.len());
        for param in &params {
            if matches!(self.types.get(*param as usize), Some(Type::Label)) {
                return Err(ops.err("a call that takes a block label"));
            }
            args.push(self.typed(ops)?);
        }
        if vararg {
            while ops.left() > 0 {
                args.push(self.pair(ops)?.0);
            }
        } else if ops.left() > 0 {
            return Err(ops.err("a call with more arguments than its type takes"));
        }
        let defines = !matches!(self.types.get(ret as usize), Some(Type::Void));
        Ok((ret, self.dx(callee, args), defines))
    }

    /// A call to a `dx.op.*` function whose first argument is a constant opcode, else a plain call.
    fn dx(&mut self, callee: ValueId, args: Vec<ValueId>) -> Op {
        let functions = self.functions;
        let name = match self.body.values.get(callee as usize).map(|v| &v.kind) {
            Some(ValueKind::Function(index)) => functions.get(*index as usize),
            _ => None,
        }
        .map(|function| function.name.as_str());
        let opcode = args.first().and_then(|&id| self.body.int(id));
        let opcode = opcode.and_then(|n| u32::try_from(n).ok());
        match (name, opcode) {
            (Some(name), Some(opcode)) if name.starts_with("dx.op.") => Op::Dx {
                op: DxOp(opcode),
                overload: overload(&mut self.types, name),
                args: args.into_iter().skip(1).collect(),
            },
            _ => Op::Call { callee, args },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn rec(code: u32, ops: &[u64]) -> Item {
        Item::Record(Record {
            code,
            ops: ops.to_vec(),
            blob: None,
        })
    }

    /// A record of `head` operands then one per character of `text`.
    fn named(code: u32, head: &[u64], text: &str) -> Item {
        let ops: Vec<u64> = head
            .iter()
            .copied()
            .chain(text.bytes().map(u64::from))
            .collect();
        rec(code, &ops)
    }

    fn block(id: u32, items: Vec<Item>) -> Item {
        Item::Block(Block { id, items })
    }

    fn module_block(items: Vec<Item>) -> Vec<Block> {
        vec![Block { id: MODULE, items }]
    }

    /// Values: 0 g, 1 the loadInput declaration, 2 main, 3 i32 4, 4 i32 -1, 5 float 1.0; main's
    /// body adds 6 i32 7 and the results 7 to 14, every operand relative.
    fn shader() -> Vec<Block> {
        let types = vec![
            rec(1, &[10]),
            rec(2, &[]),
            rec(3, &[]),
            rec(7, &[32]),
            rec(7, &[1]),
            rec(21, &[0, 1, 2, 2]),
            rec(21, &[0, 0]),
            rec(8, &[4, 0]),
            rec(8, &[5]),
            rec(8, &[1, 3]),
            named(19, &[], "S"),
            rec(20, &[0, 1, 2]),
        ];
        let body = vec![
            rec(1, &[4]),
            block(CONSTANTS, vec![rec(1, &[2]), rec(4, &[14])]),
            // Block 0.
            rec(34, &[0, 1 << 15, 4, 6, 4, 1]),
            rec(28, &[1, 3, 4]),
            rec(11, &[1, 2, 1]),
            // Block 1.
            rec(2, &[2, 2, 0]),
            rec(11, &[2]),
            // Block 2: the phi names value 11, read after it.
            rec(16, &[1, 2, 1, 3, 2]),
            rec(2, &[1, 6, 2]),
            rec(12, &[2, 6, 3, 3, 2]),
            // Block 3.
            rec(19, &[1, 2, 6, 1 << 6]),
            rec(43, &[1, 1, 1, 7]),
            rec(44, &[1, 3, 0, 0]),
            rec(20, &[14, 1, 0]),
            rec(10, &[]),
            block(SYMTAB, vec![named(1, &[7], "x"), named(2, &[0], "entry")]),
        ];
        module_block(vec![
            rec(1, &[1]),
            block(TYPES, types),
            rec(7, &[1, 1 | 2 | (3 << 2), 6, 0, 0, 0]),
            rec(8, &[6, 0, 1, 0]),
            rec(8, &[5, 0, 0, 0]),
            // i32 4, i32 -1, float 1.0.
            block(
                CONSTANTS,
                vec![
                    rec(1, &[2]),
                    rec(4, &[8]),
                    rec(4, &[3]),
                    rec(1, &[1]),
                    rec(6, &[0x3F80_0000]),
                ],
            ),
            block(
                METADATA,
                vec![
                    named(1, &[], "main"),
                    rec(2, &[7, 2]),
                    rec(3, &[2, 1, 0]),
                    named(4, &[], "dx.entryPoints"),
                    rec(10, &[2]),
                ],
            ),
            block(
                SYMTAB,
                vec![
                    named(1, &[1], "dx.op.loadInput.f32"),
                    named(1, &[2], "main"),
                    named(1, &[0], "g"),
                ],
            ),
            block(FUNCTION, body),
        ])
    }

    fn inst(result: Option<ValueId>, ty: TypeId, op: Op) -> Inst {
        Inst { result, ty, op }
    }

    #[test]
    fn the_module_reads_its_types_values_names_and_metadata() {
        let m = module(&shader()).unwrap();
        // The alloca's pointer to float in space 0 is the one type the table lacked.
        assert_eq!(m.types.len(), 11);
        let s = Type::Struct {
            name: Some("S".into()),
            fields: vec![1, 2],
            packed: false,
        };
        assert_eq!(m.types[9], s);
        assert_eq!(
            m.types[10],
            Type::Pointer {
                pointee: 1,
                space: 0
            }
        );
        assert_eq!(
            m.values[0],
            Value {
                ty: 8,
                kind: ValueKind::Global {
                    space: 3,
                    init: Some(5),
                    constant: true
                },
                name: Some("g".into()),
            }
        );
        assert_eq!(
            (m.values[1].ty, &m.values[1].kind),
            (6, &ValueKind::Function(0))
        );
        assert_eq!(
            (m.values[2].ty, &m.values[2].kind),
            (7, &ValueKind::Function(1))
        );
        assert_eq!(m.values[3].kind, ValueKind::Constant(Constant::Int(4)));
        assert_eq!(m.values[4].kind, ValueKind::Constant(Constant::Int(-1)));
        assert_eq!(
            m.values[5].kind,
            ValueKind::Constant(Constant::Float(0x3F80_0000))
        );
        assert_eq!(m.functions[0].name, "dx.op.loadInput.f32");
        assert_eq!(
            (m.functions[0].ty, m.functions[0].body.is_none()),
            (4, true)
        );
        assert_eq!(
            m.metadata,
            vec![
                Md::String(b"main".to_vec()),
                Md::Value { ty: 7, value: 2 },
                Md::Node(vec![Some(1), Some(0), None]),
            ]
        );
        assert_eq!(
            m.named,
            HashMap::from([("dx.entryPoints".to_owned(), vec![2])])
        );
        let entry = m.entry().unwrap();
        assert_eq!((entry.name.as_str(), entry.ty), ("main", 5));
    }

    #[test]
    fn relative_operands_resolve_to_absolute_ids() {
        let m = module(&shader()).unwrap();
        let body = m.entry().and_then(|f| f.body.as_ref()).unwrap();
        assert_eq!(body.values.len(), 15);
        assert_eq!(body.values[..6], m.values[..]);
        assert_eq!(body.values[6].kind, ValueKind::Constant(Constant::Int(7)));
        assert_eq!(body.values[7].name.as_deref(), Some("x"));
        assert_eq!(body.values[11].kind, ValueKind::Inst { block: 2, index: 1 });
        assert_eq!(
            body.blocks[0].insts,
            vec![
                inst(
                    Some(7),
                    1,
                    Op::Dx {
                        op: DxOp::LOAD_INPUT,
                        overload: Some(1),
                        args: vec![6]
                    }
                ),
                inst(
                    Some(8),
                    3,
                    Op::Cmp {
                        pred: Pred::FOlt,
                        lhs: 7,
                        rhs: 5
                    }
                ),
                inst(
                    None,
                    0,
                    Op::CondBr {
                        cond: 8,
                        then: 1,
                        otherwise: 2
                    }
                ),
            ]
        );
        assert_eq!(
            body.blocks[1].insts,
            vec![
                inst(
                    Some(9),
                    1,
                    Op::Binary {
                        op: BinOp::FAdd,
                        lhs: 7,
                        rhs: 7
                    }
                ),
                inst(None, 0, Op::Br { target: 2 }),
            ]
        );
        assert_eq!(
            body.blocks[2].insts,
            vec![
                inst(
                    Some(10),
                    1,
                    Op::Phi {
                        incoming: vec![(9, 1), (11, 2)]
                    }
                ),
                inst(
                    Some(11),
                    1,
                    Op::Binary {
                        op: BinOp::FMul,
                        lhs: 10,
                        rhs: 5
                    }
                ),
                inst(
                    None,
                    0,
                    Op::Switch {
                        value: 6,
                        default: 3,
                        cases: vec![(4, 2)]
                    }
                ),
            ]
        );
        assert_eq!(
            body.blocks[3].insts,
            vec![
                inst(Some(12), 10, Op::Alloca { ty: 1, count: 6 }),
                inst(
                    Some(13),
                    10,
                    Op::Gep {
                        inbounds: true,
                        base: 12,
                        indices: vec![6]
                    }
                ),
                inst(None, 0, Op::Store { ptr: 13, value: 11 }),
                inst(Some(14), 1, Op::Load { ptr: 0 }),
                inst(None, 0, Op::Ret { value: None }),
            ]
        );
        assert_eq!(body.successors(2), vec![3, 2]);
    }

    /// Value 0 is a function returning float, value 1 a float constant.
    fn small(version: u64, body: Vec<Item>) -> Vec<Block> {
        module_block(vec![
            rec(1, &[version]),
            block(TYPES, vec![rec(2, &[]), rec(3, &[]), rec(21, &[0, 1])]),
            rec(8, &[2, 0, 0]),
            block(CONSTANTS, vec![rec(1, &[1]), rec(6, &[0x4049_0FDB])]),
            block(FUNCTION, body),
        ])
    }

    /// The block and record code a refusal names.
    fn refused(blocks: &[Block]) -> Option<(u32, u32)> {
        match module(blocks) {
            Err(BitcodeError::Record { block, code, .. }) => Some((block, code)),
            _ => None,
        }
    }

    fn ret(blocks: &[Block]) -> Op {
        let m = module(blocks).unwrap();
        let body = m.functions[0].body.as_ref().unwrap();
        body.blocks[0].insts[0].op.clone()
    }

    #[test]
    fn version_zero_numbers_operands_absolutely() {
        let relative = small(1, vec![rec(1, &[1]), rec(10, &[1])]);
        let absolute = small(0, vec![rec(1, &[1]), rec(10, &[1])]);
        assert_eq!(ret(&relative), Op::Ret { value: Some(1) });
        assert_eq!(ret(&absolute), Op::Ret { value: Some(1) });
        let mut later = small(2, vec![rec(1, &[1]), rec(10, &[])]);
        assert_eq!(refused(&later), Some((MODULE, 1)));
        later[0].items[0] = rec(1, &[1]);
        assert_eq!(ret(&later), Op::Ret { value: None });
    }

    #[test]
    fn a_forward_reference_never_defined_is_refused() {
        // Relative 0 names the value the instruction itself would define, with its type after it.
        let blocks = small(1, vec![rec(1, &[1]), rec(10, &[0, 1])]);
        assert_eq!(refused(&blocks), Some((FUNCTION, 10)));
    }

    #[test]
    fn an_instruction_the_reader_does_not_know_is_named() {
        let blocks = small(1, vec![rec(1, &[1]), rec(13, &[0])]);
        assert_eq!(refused(&blocks), Some((FUNCTION, 13)));
    }

    #[test]
    fn a_body_must_end_every_block_it_declares() {
        let blocks = small(1, vec![rec(1, &[2]), rec(10, &[])]);
        assert_eq!(refused(&blocks), Some((FUNCTION, 1)));
        let blocks = small(1, vec![rec(10, &[])]);
        assert_eq!(refused(&blocks).map(|(block, _)| block), Some(FUNCTION));
        let blocks = small(1, vec![rec(1, &[3]), rec(10, &[])]);
        assert_eq!(refused(&blocks), Some((FUNCTION, 1)));
    }

    #[test]
    fn each_declared_body_needs_its_block() {
        let mut blocks = small(1, vec![rec(1, &[1]), rec(10, &[])]);
        blocks[0].items.pop();
        assert_eq!(refused(&blocks), Some((MODULE, 8)));
        assert!(matches!(module(&[]), Err(BitcodeError::Unsupported(_))));
    }

    #[test]
    fn a_type_naming_one_past_the_table_is_refused() {
        let blocks = module_block(vec![block(TYPES, vec![rec(3, &[]), rec(8, &[2])])]);
        assert_eq!(refused(&blocks).map(|(block, _)| block), Some(TYPES));
        let blocks = module_block(vec![block(TYPES, vec![rec(3, &[])]), rec(7, &[1, 2, 0])]);
        assert_eq!(refused(&blocks), Some((MODULE, 7)));
    }

    #[test]
    fn sign_rotated_values_decode() {
        assert_eq!(signed(0), 0);
        assert_eq!(signed(2), 1);
        assert_eq!(signed(3), -1);
        assert_eq!(signed(10), 5);
        assert_eq!(signed(11), -5);
        assert_eq!(signed(1), i64::MIN);
    }
}
