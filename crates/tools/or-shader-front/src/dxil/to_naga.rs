//! One DXIL entry point as a naga module: the structured body as a function over private I/O
//! variables, and a wrapper entry point that binds them to the stage's locations and built-ins.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroU32;

use naga::{
    BinaryOperator as Bo, Block, Expression as E, Handle, Literal, Scalar, ScalarKind, Span,
    Statement, TypeInner, VectorSize,
};
use or_io::gta5::shaders::spirv_pack::{Fidelity, SignatureElement};

use super::ir::{
    self, BinOp, BlockId, CastOp, Constant, DxOp, Md, MdId, Op, Pred, TypeId, ValueId, ValueKind,
};
use super::metadata::DxResources;
use super::plan::{BindingPlan, Planned};
use super::structure::Node;
use super::{Register, ResourceClass, ShaderKind};

/// The stage's interface: what `loadInput` and `storeOutput` address, by signature element id.
pub struct Interface {
    pub kind: ShaderKind,
    pub inputs: Vec<SignatureElement>,
    pub outputs: Vec<SignatureElement>,
    pub threads: Option<[u32; 3]>,
}

pub struct Lowered {
    pub module: naga::Module,
    pub fidelity: Fidelity,
    pub lowerings: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("the intrinsic {0} is not lowered by this front end")]
    Op(String),
    #[error("{op} takes a {what} that is not a constant")]
    NotConstant { op: String, what: &'static str },
    #[error("the type {0} has no naga form here")]
    Type(String),
    #[error("the instruction {0} is not lowered")]
    Inst(&'static str),
    #[error("a call to {0}, which is no dx.op intrinsic")]
    Call(String),
    #[error("{op} reads a resource by a descriptor-heap index known only when the game runs")]
    Heap { op: String },
    #[error("{op} names {register:?}, which the program's PSV0 does not bind")]
    Unbound {
        op: String,
        register: super::Register,
    },
}

/// How deep a type, a constant or a chain of handle annotations may nest: it bounds recursion.
const MAX_DEPTH: u32 = 8;
/// D3D's register count: no signature element spans more rows.
const MAX_ROWS: i64 = 32;
/// LLVM's address space of `groupshared` memory in DXIL.
const GROUPSHARED: u32 = 3;

pub fn to_naga(
    module: &ir::Module,
    body: &ir::Body,
    nodes: &[Node],
    interface: &Interface,
    plan: &mut BindingPlan,
) -> Result<Lowered, TranslateError> {
    let stage = match interface.kind {
        ShaderKind::Vertex => naga::ShaderStage::Vertex,
        ShaderKind::Pixel => naga::ShaderStage::Fragment,
        ShaderKind::Compute => naga::ShaderStage::Compute,
        _ => {
            return Err(TranslateError::Inst(
                "a stage other than vertex, pixel or compute",
            ))
        }
    };
    let inputs = elements(module, &interface.inputs, false)?;
    let outputs = elements(module, &interface.outputs, true)?;
    let mut cx = FnCx::new(module, body, plan, interface, inputs, outputs);
    cx.nodes(nodes)?;
    cx.finish(stage)
}

/// The state of one function's lowering that `ops.rs` works through.
pub struct FnCx<'a> {
    pub ir: &'a ir::Module,
    pub body: &'a ir::Body,
    pub plan: &'a mut BindingPlan,
    pub interface: &'a Interface,
    module: naga::Module,
    b: Builder,
    /// Expressions of IR values in scope, and per open block the values it added.
    cache: HashMap<ValueId, Handle<E>>,
    frames: Vec<Vec<ValueId>>,
    /// Values read outside their block, and every phi: kept in function locals.
    crossing: HashSet<ValueId>,
    locals: HashMap<ValueId, Handle<naga::LocalVariable>>,
    globals: HashMap<ValueId, Handle<naga::GlobalVariable>>,
    resources: HashMap<Register, Handle<naga::GlobalVariable>>,
    /// Textures a comparison samples, which naga types as depth images.
    depth: HashSet<Register>,
    decls: DxResources,
    inputs: Vec<Io>,
    outputs: Vec<Io>,
    builtins: Vec<(
        naga::BuiltIn,
        Handle<naga::Type>,
        Handle<naga::GlobalVariable>,
    )>,
    layouter: naga::proc::Layouter,
    lowerings: BTreeMap<String, u32>,
    /// An error a method that returns no `Result` met, reported when the lowering ends.
    failure: Option<TranslateError>,
}

/// A signature element and its private `vec4` per row.
type Io = (Element, Vec<Handle<naga::GlobalVariable>>);

impl<'a> FnCx<'a> {
    fn new(
        ir: &'a ir::Module,
        body: &'a ir::Body,
        plan: &'a mut BindingPlan,
        interface: &'a Interface,
        inputs: Vec<Element>,
        outputs: Vec<Element>,
    ) -> Self {
        let mut module = naga::Module::default();
        let inputs = privates(&mut module, inputs, "in");
        let outputs = privates(&mut module, outputs, "out");
        let mut cx = Self {
            ir,
            body,
            plan,
            interface,
            module,
            b: Builder::new(),
            cache: HashMap::new(),
            frames: vec![Vec::new()],
            crossing: crossing(body),
            locals: HashMap::new(),
            globals: HashMap::new(),
            resources: HashMap::new(),
            depth: HashSet::new(),
            decls: super::metadata::dx_resources(ir).unwrap_or_default(),
            inputs,
            outputs,
            builtins: Vec::new(),
            layouter: naga::proc::Layouter::default(),
            lowerings: BTreeMap::new(),
            failure: None,
        };
        cx.depth = cx.depth_registers();
        cx
    }

    /// The expression of an IR value, loading it from its local where it lives in one.
    pub fn value(&mut self, id: ValueId) -> Result<Handle<E>, TranslateError> {
        if let Some(&handle) = self.cache.get(&id) {
            return Ok(handle);
        }
        let body = self.body;
        let value = body
            .value(id)
            .ok_or(TranslateError::Inst("a value the function does not define"))?;
        let handle = match &value.kind {
            ValueKind::Constant(constant) => self.constant(constant, value.ty)?,
            ValueKind::Global { space, init, .. } => {
                let global = self.global(id, value.ty, *space, *init)?;
                self.b.emit(E::GlobalVariable(global))
            }
            ValueKind::Inst { .. } => {
                let inst = self
                    .inst_of(id)
                    .ok_or(TranslateError::Inst("a value of no instruction"))?;
                match &inst.op {
                    Op::Alloca { ty, count } => {
                        if self.int(*count) != Some(1) {
                            return Err(TranslateError::Inst(
                                "an alloca of other than one element",
                            ));
                        }
                        let local = self.local(id, *ty)?;
                        self.b.emit(E::LocalVariable(local))
                    }
                    Op::Gep { base, indices, .. } => self.gep(*base, indices)?,
                    _ if self.crossing.contains(&id) => {
                        let local = self.local(id, inst.ty)?;
                        let pointer = self.b.emit(E::LocalVariable(local));
                        self.b.emit(E::Load { pointer })
                    }
                    _ => {
                        return Err(TranslateError::Inst(
                            "a value read where it is not in scope",
                        ))
                    }
                }
            }
            ValueKind::Argument(_) => return Err(TranslateError::Inst("a function argument")),
            ValueKind::Function(_) => {
                return Err(TranslateError::Inst("a function used as a value"))
            }
        };
        self.remember(id, handle);
        Ok(handle)
    }

    pub fn int(&self, id: ValueId) -> Option<i64> {
        self.body.int(id)
    }

    pub fn emit(&mut self, expression: naga::Expression) -> Handle<naga::Expression> {
        self.b.emit(expression)
    }

    pub fn push(&mut self, statement: naga::Statement) {
        self.b.push(statement);
    }

    /// The global variable of a handle value: follows createHandle*, annotateHandle to the plan.
    pub fn resource(
        &mut self,
        handle: ValueId,
    ) -> Result<Handle<naga::GlobalVariable>, TranslateError> {
        let bound = self.bound(handle)?;
        let register = bound.register;
        if let Some(&global) = self.resources.get(&register) {
            return Ok(global);
        }
        if let Some((kind, detail)) = bound.props {
            if !self.declared(register) {
                if let Some(planned) = annotated(register.class, kind, detail) {
                    self.plan.shape(register, planned);
                }
            }
        }
        let planned = self
            .plan
            .find(register)
            .cloned()
            .ok_or_else(|| TranslateError::Unbound {
                op: bound.op.name().into_owned(),
                register,
            })?;
        let (inner, space) = match planned.planned {
            Planned::Texture {
                dim,
                arrayed,
                multisampled,
                sampled,
            } => {
                let class = if self.depth.contains(&register) {
                    naga::ImageClass::Depth {
                        multi: multisampled,
                    }
                } else {
                    naga::ImageClass::Sampled {
                        kind: sampled,
                        multi: multisampled,
                    }
                };
                (
                    TypeInner::Image {
                        dim,
                        arrayed,
                        class,
                    },
                    naga::AddressSpace::Handle,
                )
            }
            Planned::StorageTexture {
                dim,
                arrayed,
                format,
            } => {
                let access = naga::StorageAccess::LOAD | naga::StorageAccess::STORE;
                let class = naga::ImageClass::Storage { format, access };
                (
                    TypeInner::Image {
                        dim,
                        arrayed,
                        class,
                    },
                    naga::AddressSpace::Handle,
                )
            }
            Planned::Sampler { comparison } => (
                TypeInner::Sampler { comparison },
                naga::AddressSpace::Handle,
            ),
            Planned::Uniform { rows } => {
                let base = vec4(&mut self.module, Scalar::U32);
                let size =
                    naga::ArraySize::Constant(NonZeroU32::new(rows).unwrap_or(NonZeroU32::MIN));
                (
                    TypeInner::Array {
                        base,
                        size,
                        stride: 16,
                    },
                    naga::AddressSpace::Uniform,
                )
            }
            Planned::Storage { read_only, .. } => {
                let base = insert(&mut self.module, TypeInner::Scalar(Scalar::U32));
                let access = if read_only {
                    naga::StorageAccess::LOAD
                } else {
                    naga::StorageAccess::LOAD | naga::StorageAccess::STORE
                };
                let inner = TypeInner::Array {
                    base,
                    size: naga::ArraySize::Dynamic,
                    stride: 4,
                };
                (inner, naga::AddressSpace::Storage { access })
            }
        };
        let ty = insert(&mut self.module, inner);
        let global = naga::GlobalVariable {
            name: Some(planned.name),
            space,
            binding: Some(naga::ResourceBinding {
                group: planned.group,
                binding: planned.binding,
            }),
            ty,
            init: None,
            memory_decorations: naga::MemoryDecorations::empty(),
        };
        let global = self.module.global_variables.append(global, Span::UNDEFINED);
        self.resources.insert(register, global);
        Ok(global)
    }

    /// The private variable of input or output element `id`, a vec4 of its component type.
    pub fn input(&mut self, id: u32) -> Handle<naga::GlobalVariable> {
        self.io(false, id)
    }

    pub fn output(&mut self, id: u32) -> Handle<naga::GlobalVariable> {
        self.io(true, id)
    }

    /// LLVM's integers are signless: every integer is `u32` here (i1 is `bool`), and a signed operation bitcasts.
    pub fn scalar(&self, ty: TypeId) -> Result<naga::Scalar, TranslateError> {
        Ok(scalar_of(self.num(ty)?.0))
    }

    /// Count a lowering that changes what the program computes; it marks the module Approximated.
    pub fn approximated(&mut self, lowering: &'static str) {
        *self.lowerings.entry(lowering.to_owned()).or_default() += 1;
    }

    /// The module's handle of a type, added where it is new.
    pub fn ty(&mut self, inner: naga::TypeInner) -> Handle<naga::Type> {
        insert(&mut self.module, inner)
    }

    pub fn module(&self) -> &naga::Module {
        &self.module
    }

    fn io(&mut self, output: bool, id: u32) -> Handle<naga::GlobalVariable> {
        let list = if output { &self.outputs } else { &self.inputs };
        if let Some(&var) = list.get(id as usize).and_then(|(_, vars)| vars.first()) {
            return var;
        }
        self.failure.get_or_insert(TranslateError::Inst(
            "a signature element the program does not declare",
        ));
        let ty = vec4(&mut self.module, Scalar::F32);
        self.module
            .global_variables
            .append(private(None, ty), Span::UNDEFINED)
    }

    fn remember(&mut self, id: ValueId, handle: Handle<E>) {
        self.cache.insert(id, handle);
        if let Some(frame) = self.frames.last_mut() {
            frame.push(id);
        }
    }

    /// `nodes` lowered into a block of their own; what they computed goes out of scope with it.
    fn nested(&mut self, nodes: &[Node]) -> Result<Block, TranslateError> {
        self.b.open();
        self.frames.push(Vec::new());
        let lowered = self.nodes(nodes);
        for id in self.frames.pop().unwrap_or_default() {
            self.cache.remove(&id);
        }
        let block = self.b.close();
        lowered.map(|()| block)
    }

    fn nodes(&mut self, nodes: &[Node]) -> Result<(), TranslateError> {
        for node in nodes {
            match node {
                Node::Code(block) => self.code(*block)?,
                Node::Edge { from, to } => self.edge(*from, *to)?,
                Node::If {
                    cond,
                    then,
                    otherwise,
                } => {
                    let condition = self.value(*cond)?;
                    let accept = self.nested(then)?;
                    let reject = self.nested(otherwise)?;
                    self.push(Statement::If {
                        condition,
                        accept,
                        reject,
                    });
                }
                Node::Loop { body } => {
                    let body = self.nested(body)?;
                    self.push(Statement::Loop {
                        body,
                        continuing: Block::new(),
                        break_if: None,
                    });
                }
                Node::Switch {
                    value,
                    cases,
                    default,
                } => {
                    let selector = self.value(*value)?;
                    let mut arms = Vec::new();
                    for (values, nodes) in cases {
                        let body = self.nested(nodes)?;
                        let Some((last, rest)) = values.split_last() else {
                            continue;
                        };
                        arms.extend(rest.iter().map(|&value| naga::SwitchCase {
                            value: naga::SwitchValue::U32(value as u32),
                            body: Block::new(),
                            fall_through: true,
                        }));
                        let value = naga::SwitchValue::U32(*last as u32);
                        arms.push(naga::SwitchCase {
                            value,
                            body,
                            fall_through: false,
                        });
                    }
                    let body = self.nested(default)?;
                    let value = naga::SwitchValue::Default;
                    arms.push(naga::SwitchCase {
                        value,
                        body,
                        fall_through: false,
                    });
                    self.push(Statement::Switch {
                        selector,
                        cases: arms,
                    });
                }
                Node::Break => self.push(Statement::Break),
                Node::Continue => self.push(Statement::Continue),
                Node::Return(None) | Node::Unreachable => {
                    self.push(Statement::Return { value: None })
                }
                Node::Return(Some(_)) => {
                    return Err(TranslateError::Inst(
                        "a return with a value from the entry point",
                    ))
                }
            }
        }
        Ok(())
    }

    /// A block's instructions; phis are stored on edges, pointers are built where they are read.
    fn code(&mut self, block: BlockId) -> Result<(), TranslateError> {
        let body = self.body;
        let basic = body
            .blocks
            .get(block as usize)
            .ok_or(TranslateError::Inst("a block the function does not have"))?;
        for inst in &basic.insts {
            if matches!(
                inst.op,
                Op::Phi { .. }
                    | Op::Alloca { .. }
                    | Op::Gep { .. }
                    | Op::Br { .. }
                    | Op::CondBr { .. }
                    | Op::Switch { .. }
                    | Op::Ret { .. }
                    | Op::Unreachable
            ) {
                continue;
            }
            let expression = self.inst(inst)?;
            match self.num(inst.ty) {
                Ok((Num::Float(16), _)) => self.approximated("f16-as-f32"),
                Ok((Num::Int(8 | 16), _)) => self.approximated("i16-as-i32"),
                _ => {}
            }
            let (Some(result), Some(expression)) = (inst.result, expression) else {
                continue;
            };
            self.remember(result, expression);
            if self.crossing.contains(&result) {
                let local = self.local(result, inst.ty)?;
                let pointer = self.b.emit(E::LocalVariable(local));
                self.push(Statement::Store {
                    pointer,
                    value: expression,
                });
            }
        }
        Ok(())
    }

    /// Store `to`'s phis' values from `from`, every value read before any is stored.
    fn edge(&mut self, from: BlockId, to: BlockId) -> Result<(), TranslateError> {
        let body = self.body;
        let block = body.blocks.get(to as usize).ok_or(TranslateError::Inst(
            "an edge into a block the function does not have",
        ))?;
        let mut copies = Vec::new();
        for inst in &block.insts {
            let Op::Phi { incoming } = &inst.op else {
                continue;
            };
            let result = inst
                .result
                .ok_or(TranslateError::Inst("a phi that defines no value"))?;
            let &(value, _) =
                incoming
                    .iter()
                    .find(|&&(_, block)| block == from)
                    .ok_or(TranslateError::Inst(
                        "a phi with no value for an edge into its block",
                    ))?;
            copies.push((result, inst.ty, self.value(value)?));
        }
        for (result, ty, value) in copies {
            let local = self.local(result, ty)?;
            let pointer = self.b.emit(E::LocalVariable(local));
            self.push(Statement::Store { pointer, value });
            self.cache.remove(&result);
        }
        Ok(())
    }

    fn inst(&mut self, inst: &ir::Inst) -> Result<Option<Handle<E>>, TranslateError> {
        Ok(Some(match &inst.op {
            Op::Binary { op, lhs, rhs } => self.binary(*op, inst.ty, *lhs, *rhs)?,
            Op::Cast { op, value } => self.cast(*op, *value, inst.ty)?,
            Op::Cmp { pred, lhs, rhs } => self.cmp(*pred, *lhs, *rhs)?,
            Op::Select {
                cond,
                then,
                otherwise,
            } => {
                let condition = self.value(*cond)?;
                let accept = self.value(*then)?;
                let reject = self.value(*otherwise)?;
                self.emit(E::Select {
                    condition,
                    accept,
                    reject,
                })
            }
            Op::ExtractValue { aggregate, indices } => self.extract(*aggregate, indices)?,
            Op::ExtractElement { vector, index } => {
                let vector = self.value(*vector)?;
                self.index_into(vector, *index)?
            }
            Op::InsertElement {
                vector,
                value,
                index,
            } => {
                let ir = self.ir;
                let Some(ir::Type::Vector { len, .. }) = ir.ty(inst.ty) else {
                    return Err(TranslateError::Inst("an insertelement into no vector"));
                };
                let at = self.int(*index).ok_or(TranslateError::Inst(
                    "an insertelement at an index known only at run time",
                ))?;
                let (base, value) = (self.value(*vector)?, self.value(*value)?);
                let components = (0..*len)
                    .map(|i| {
                        if i64::from(i) == at {
                            value
                        } else {
                            self.emit(E::AccessIndex { base, index: i })
                        }
                    })
                    .collect();
                let ty = self.ty_of(inst.ty)?;
                self.emit(E::Compose { ty, components })
            }
            Op::ShuffleVector { a, b, mask } => self.shuffle(*a, *b, *mask, inst.ty)?,
            Op::Load { ptr } => {
                let pointer = self.value(*ptr)?;
                self.emit(E::Load { pointer })
            }
            Op::Store { ptr, value } => {
                let pointer = self.value(*ptr)?;
                let value = self.value(*value)?;
                self.push(Statement::Store { pointer, value });
                return Ok(None);
            }
            Op::Dx { op, overload, args } => return self.dx(*op, *overload, args),
            Op::Call { callee, .. } => return self.call(*callee),
            Op::InsertValue { .. } => return Err(TranslateError::Inst("insertvalue")),
            Op::AtomicRmw { .. } => return Err(TranslateError::Inst("atomicrmw")),
            Op::CmpXchg { .. } => return Err(TranslateError::Inst("cmpxchg")),
            Op::Phi { .. }
            | Op::Alloca { .. }
            | Op::Gep { .. }
            | Op::Br { .. }
            | Op::CondBr { .. }
            | Op::Switch { .. }
            | Op::Ret { .. }
            | Op::Unreachable => return Ok(None),
        }))
    }

    /// The interface and thread-id intrinsics, which address the wrapper's variables; the rest go
    /// to `ops.rs`. A handle is no value: [`FnCx::resource`] follows it where it is used.
    fn dx(
        &mut self,
        op: DxOp,
        overload: Option<TypeId>,
        args: &[ValueId],
    ) -> Result<Option<Handle<E>>, TranslateError> {
        let built_in = match op {
            DxOp::CREATE_HANDLE
            | DxOp::CREATE_HANDLE_FROM_BINDING
            | DxOp::ANNOTATE_HANDLE
            | DxOp::CREATE_HANDLE_FROM_HEAP => return Ok(None),
            DxOp::LOAD_INPUT => {
                let (var, from) = self.io_row(false, args, op)?;
                let global = self.emit(E::GlobalVariable(var));
                let row = self.emit(E::Load { pointer: global });
                let column = args
                    .get(2)
                    .copied()
                    .ok_or_else(|| not_constant(op, "column"))?;
                let value = self.index_into(row, column)?;
                let to = match overload {
                    Some(ty) => self.scalar(ty)?,
                    None => from,
                };
                return Ok(Some(self.b.convert(value, from, to)));
            }
            DxOp::STORE_OUTPUT => {
                let (var, to) = self.io_row(true, args, op)?;
                let value = args
                    .get(3)
                    .copied()
                    .ok_or_else(|| not_constant(op, "value"))?;
                let value = self.value(value)?;
                let from = match overload {
                    Some(ty) => self.scalar(ty)?,
                    None => to,
                };
                let value = self.b.convert(value, from, to);
                let global = self.emit(E::GlobalVariable(var));
                let column = args
                    .get(2)
                    .copied()
                    .ok_or_else(|| not_constant(op, "column"))?;
                let pointer = self.index_into(global, column)?;
                self.push(Statement::Store { pointer, value });
                return Ok(None);
            }
            DxOp::THREAD_ID => naga::BuiltIn::GlobalInvocationId,
            DxOp::GROUP_ID => naga::BuiltIn::WorkGroupId,
            DxOp::THREAD_ID_IN_GROUP => naga::BuiltIn::LocalInvocationId,
            DxOp::FLATTENED_THREAD_ID_IN_GROUP => naga::BuiltIn::LocalInvocationIndex,
            _ => return super::ops::lower(self, op, overload, args),
        };
        let var = self.builtin(built_in);
        let pointer = self.emit(E::GlobalVariable(var));
        let value = self.emit(E::Load { pointer });
        if built_in == naga::BuiltIn::LocalInvocationIndex {
            return Ok(Some(value));
        }
        let component = args
            .first()
            .copied()
            .ok_or_else(|| not_constant(op, "component"))?;
        Ok(Some(self.index_into(value, component)?))
    }

    /// The private variable of a `loadInput` or `storeOutput`'s element and row, and its scalar.
    fn io_row(
        &self,
        output: bool,
        args: &[ValueId],
        op: DxOp,
    ) -> Result<(Handle<naga::GlobalVariable>, Scalar), TranslateError> {
        let id = args
            .first()
            .and_then(|&a| self.int(a))
            .ok_or_else(|| not_constant(op, "signature element"))?;
        let row = args
            .get(1)
            .and_then(|&a| self.int(a))
            .ok_or_else(|| not_constant(op, "row"))?;
        let list = if output { &self.outputs } else { &self.inputs };
        let (element, vars) =
            usize::try_from(id)
                .ok()
                .and_then(|id| list.get(id))
                .ok_or(TranslateError::Inst(
                    "a signature element the program does not declare",
                ))?;
        let var = usize::try_from(row)
            .ok()
            .and_then(|row| vars.get(row))
            .ok_or(TranslateError::Inst(
                "a signature row its element does not have",
            ))?;
        Ok((*var, element.scalar))
    }

    fn builtin(&mut self, built_in: naga::BuiltIn) -> Handle<naga::GlobalVariable> {
        if let Some(&(_, _, var)) = self.builtins.iter().find(|(b, _, _)| *b == built_in) {
            return var;
        }
        let ty = match built_in {
            naga::BuiltIn::LocalInvocationIndex => TypeInner::Scalar(Scalar::U32),
            _ => TypeInner::Vector {
                size: VectorSize::Tri,
                scalar: Scalar::U32,
            },
        };
        let ty = insert(&mut self.module, ty);
        let var = self
            .module
            .global_variables
            .append(private(Some(format!("{built_in:?}")), ty), Span::UNDEFINED);
        self.builtins.push((built_in, ty, var));
        var
    }

    /// Component or element `index` of a vector or array value, or of the place a pointer names.
    fn index_into(&mut self, base: Handle<E>, index: ValueId) -> Result<Handle<E>, TranslateError> {
        Ok(match self.int(index) {
            Some(at) => {
                let index =
                    u32::try_from(at).map_err(|_| TranslateError::Inst("an index out of range"))?;
                self.emit(E::AccessIndex { base, index })
            }
            None => {
                let index = self.value(index)?;
                self.emit(E::Access { base, index })
            }
        })
    }

    fn gep(&mut self, base: ValueId, indices: &[ValueId]) -> Result<Handle<E>, TranslateError> {
        let (&first, rest) = indices
            .split_first()
            .ok_or(TranslateError::Inst("a getelementptr with no index"))?;
        if self.int(first) != Some(0) {
            return Err(TranslateError::Inst(
                "a getelementptr that steps past its pointer",
            ));
        }
        // `value` recurses through each base, so a chain is bounded before it can take the stack.
        let mut at = base;
        for _ in 0..=MAX_DEPTH {
            match self.inst_of(at).map(|inst| &inst.op) {
                Some(Op::Gep { base, .. }) => at = *base,
                _ => break,
            }
        }
        if matches!(self.inst_of(at).map(|inst| &inst.op), Some(Op::Gep { .. })) {
            return Err(TranslateError::Inst(
                "a chain of getelementptr deeper than the limit",
            ));
        }
        let mut pointer = self.value(base)?;
        for &index in rest {
            pointer = self.index_into(pointer, index)?;
        }
        Ok(pointer)
    }

    fn extract(
        &mut self,
        aggregate: ValueId,
        indices: &[u32],
    ) -> Result<Handle<E>, TranslateError> {
        let (body, ir) = (self.body, self.ir);
        let ty = body.value(aggregate).and_then(|value| ir.ty(value.ty));
        if let (
            Some(ir::Type::Struct {
                name: Some(name),
                fields,
                ..
            }),
            [index],
        ) = (ty, indices)
        {
            // A resource read's last field is its residency status; every texel here is resident.
            if name.starts_with("dx.types.ResRet") && *index as usize + 1 == fields.len() {
                return Ok(self.emit(E::Literal(Literal::U32(u32::MAX))));
            }
        }
        let mut value = self.value(aggregate)?;
        for &index in indices {
            value = self.emit(E::AccessIndex { base: value, index });
        }
        Ok(value)
    }

    fn shuffle(
        &mut self,
        a: ValueId,
        b: ValueId,
        mask: ValueId,
        ty: TypeId,
    ) -> Result<Handle<E>, TranslateError> {
        let (body, ir) = (self.body, self.ir);
        let len_of = |id: TypeId| match ir.ty(id) {
            Some(ir::Type::Vector { len, elem }) => Ok((*len, *elem)),
            _ => Err(TranslateError::Inst("a shufflevector of no vectors")),
        };
        let (len_a, _) = len_of(body.value(a).map_or(u32::MAX, |v| v.ty))?;
        let (len, elem) = len_of(ty)?;
        let lanes: Vec<Option<i64>> = match body.value(mask).map(|v| &v.kind) {
            Some(ValueKind::Constant(Constant::Data(words))) => {
                words.iter().map(|&w| Some(w as i64)).collect()
            }
            Some(ValueKind::Constant(Constant::Aggregate(items))) => {
                items.iter().map(|&i| self.int(i)).collect()
            }
            Some(ValueKind::Constant(Constant::Null)) => vec![Some(0); len as usize],
            Some(ValueKind::Constant(Constant::Undef)) => vec![None; len as usize],
            _ => {
                return Err(TranslateError::Inst(
                    "a shufflevector whose mask is not a constant",
                ))
            }
        };
        if lanes.len() != len as usize {
            return Err(TranslateError::Inst(
                "a shufflevector whose mask does not match its result",
            ));
        }
        let (va, vb) = (self.value(a)?, self.value(b)?);
        let zero = self.ty_of(elem)?;
        let mut components = Vec::with_capacity(lanes.len());
        for lane in lanes {
            components.push(match lane.and_then(|l| u32::try_from(l).ok()) {
                Some(l) if l < len_a => self.emit(E::AccessIndex { base: va, index: l }),
                Some(l) if l - len_a < len_a => self.emit(E::AccessIndex {
                    base: vb,
                    index: l - len_a,
                }),
                _ => self.emit(E::ZeroValue(zero)),
            });
        }
        let ty = self.ty_of(ty)?;
        Ok(self.emit(E::Compose { ty, components }))
    }

    fn call(&mut self, callee: ValueId) -> Result<Option<Handle<E>>, TranslateError> {
        let (body, ir) = (self.body, self.ir);
        let value = body.value(callee);
        let name = match value.map(|v| &v.kind) {
            Some(ValueKind::Function(index)) => {
                ir.functions.get(*index as usize).map(|f| f.name.clone())
            }
            _ => None,
        }
        .or_else(|| value.and_then(|v| v.name.clone()))
        .unwrap_or_default();
        // Debug and lifetime markers change nothing a shader computes.
        if ["llvm.dbg.", "llvm.lifetime.", "dx.noop"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            return Ok(None);
        }
        Err(TranslateError::Call(name))
    }

    fn binary(
        &mut self,
        op: BinOp,
        ty: TypeId,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<Handle<E>, TranslateError> {
        let (num, _) = self.num(ty)?;
        let float = matches!(
            op,
            BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem
        );
        let logic = matches!(op, BinOp::And | BinOp::Or | BinOp::Xor);
        if float != matches!(num, Num::Float(_)) || (num == Num::Bool && !logic) {
            return Err(TranslateError::Inst(
                "an arithmetic instruction on the wrong kind of value",
            ));
        }
        let (op, signed) = match op {
            BinOp::Add | BinOp::FAdd => (Bo::Add, false),
            BinOp::Sub | BinOp::FSub => (Bo::Subtract, false),
            BinOp::Mul | BinOp::FMul => (Bo::Multiply, false),
            BinOp::UDiv | BinOp::FDiv => (Bo::Divide, false),
            BinOp::URem | BinOp::FRem => (Bo::Modulo, false),
            BinOp::SDiv => (Bo::Divide, true),
            BinOp::SRem => (Bo::Modulo, true),
            BinOp::Shl => (Bo::ShiftLeft, false),
            BinOp::LShr => (Bo::ShiftRight, false),
            BinOp::AShr => (Bo::ShiftRight, true),
            BinOp::And => (Bo::And, false),
            BinOp::Or => (Bo::InclusiveOr, false),
            BinOp::Xor if num == Num::Bool => (Bo::NotEqual, false),
            BinOp::Xor => (Bo::ExclusiveOr, false),
        };
        let (mut left, mut right) = (self.value(lhs)?, self.value(rhs)?);
        if signed {
            left = self.bitcast(left, ScalarKind::Sint);
            // A shift's amount stays unsigned, as naga requires.
            if op != Bo::ShiftRight {
                right = self.bitcast(right, ScalarKind::Sint);
            }
        }
        let out = self.emit(E::Binary { op, left, right });
        Ok(if signed {
            self.bitcast(out, ScalarKind::Uint)
        } else {
            out
        })
    }

    fn cmp(&mut self, pred: Pred, lhs: ValueId, rhs: ValueId) -> Result<Handle<E>, TranslateError> {
        let ty = self
            .body
            .value(lhs)
            .ok_or(TranslateError::Inst("a value the function does not define"))?
            .ty;
        let (_, size) = self.num(ty)?;
        let (l, r) = (self.value(lhs)?, self.value(rhs)?);
        let bin = |cx: &mut Self, op, left, right| cx.emit(E::Binary { op, left, right });
        let not = |cx: &mut Self, expr| {
            cx.emit(E::Unary {
                op: naga::UnaryOperator::LogicalNot,
                expr,
            })
        };
        Ok(match pred {
            Pred::FFalse | Pred::FTrue => self.splat(Literal::Bool(pred == Pred::FTrue), size),
            Pred::FOeq | Pred::IEq => bin(self, Bo::Equal, l, r),
            Pred::FUne | Pred::INe => bin(self, Bo::NotEqual, l, r),
            Pred::FOgt | Pred::IUgt => bin(self, Bo::Greater, l, r),
            Pred::FOge | Pred::IUge => bin(self, Bo::GreaterEqual, l, r),
            Pred::FOlt | Pred::IUlt => bin(self, Bo::Less, l, r),
            Pred::FOle | Pred::IUle => bin(self, Bo::LessEqual, l, r),
            Pred::ISgt | Pred::ISge | Pred::ISlt | Pred::ISle => {
                let op = match pred {
                    Pred::ISgt => Bo::Greater,
                    Pred::ISge => Bo::GreaterEqual,
                    Pred::ISlt => Bo::Less,
                    _ => Bo::LessEqual,
                };
                let (sl, sr) = (
                    self.bitcast(l, ScalarKind::Sint),
                    self.bitcast(r, ScalarKind::Sint),
                );
                bin(self, op, sl, sr)
            }
            // naga's comparisons are ordered (false where either side is NaN) but for `!=`.
            Pred::FOne | Pred::FUeq => {
                let (less, greater) = (bin(self, Bo::Less, l, r), bin(self, Bo::Greater, l, r));
                let one = bin(self, Bo::InclusiveOr, less, greater);
                if pred == Pred::FOne {
                    one
                } else {
                    not(self, one)
                }
            }
            Pred::FUgt | Pred::FUge | Pred::FUlt | Pred::FUle => {
                let op = match pred {
                    Pred::FUgt => Bo::LessEqual,
                    Pred::FUge => Bo::Less,
                    Pred::FUlt => Bo::GreaterEqual,
                    _ => Bo::Greater,
                };
                let ordered = bin(self, op, l, r);
                not(self, ordered)
            }
            Pred::FOrd => {
                let (a, b) = (bin(self, Bo::Equal, l, l), bin(self, Bo::Equal, r, r));
                bin(self, Bo::And, a, b)
            }
            Pred::FUno => {
                let (a, b) = (bin(self, Bo::NotEqual, l, l), bin(self, Bo::NotEqual, r, r));
                bin(self, Bo::InclusiveOr, a, b)
            }
        })
    }

    fn cast(
        &mut self,
        op: CastOp,
        value: ValueId,
        ty: TypeId,
    ) -> Result<Handle<E>, TranslateError> {
        let from_ty = self
            .body
            .value(value)
            .ok_or(TranslateError::Inst("a value the function does not define"))?
            .ty;
        let (from, size) = self.num(from_ty)?;
        let (to, _) = self.num(ty)?;
        let x = self.value(value)?;
        Ok(match (op, from, to) {
            (CastOp::Trunc, _, Num::Bool) => {
                let one = self.splat(Literal::U32(1), size);
                let bit = self.emit(E::Binary {
                    op: Bo::And,
                    left: x,
                    right: one,
                });
                let zero = self.splat(Literal::U32(0), size);
                self.emit(E::Binary {
                    op: Bo::NotEqual,
                    left: bit,
                    right: zero,
                })
            }
            (CastOp::Trunc, _, Num::Int(bits)) => self.narrow(x, bits, size),
            (CastOp::ZExt | CastOp::SExt, Num::Bool, Num::Int(bits)) => {
                let all = if op == CastOp::SExt { mask(bits) } else { 1 };
                let accept = self.splat(Literal::U32(all), size);
                let reject = self.splat(Literal::U32(0), size);
                self.emit(E::Select {
                    condition: x,
                    accept,
                    reject,
                })
            }
            (CastOp::ZExt, Num::Int(_), Num::Int(_)) => x,
            (CastOp::SExt, Num::Int(bits), Num::Int(to_bits)) => {
                let wide = self.sext(x, bits, size);
                self.narrow(wide, to_bits, size)
            }
            (CastOp::FpToUi, Num::Float(_), Num::Int(bits)) => {
                let v = self.emit(E::As {
                    expr: x,
                    kind: ScalarKind::Uint,
                    convert: Some(4),
                });
                self.narrow(v, bits, size)
            }
            (CastOp::FpToSi, Num::Float(_), Num::Int(bits)) => {
                let v = self.emit(E::As {
                    expr: x,
                    kind: ScalarKind::Sint,
                    convert: Some(4),
                });
                let v = self.bitcast(v, ScalarKind::Uint);
                self.narrow(v, bits, size)
            }
            (CastOp::UiToFp | CastOp::SiToFp, Num::Bool, Num::Float(_)) => {
                // `sitofp` reads a true i1 as -1.
                let one = if op == CastOp::SiToFp { -1.0 } else { 1.0 };
                let accept = self.splat(Literal::F32(one), size);
                let reject = self.splat(Literal::F32(0.0), size);
                self.emit(E::Select {
                    condition: x,
                    accept,
                    reject,
                })
            }
            (CastOp::UiToFp, Num::Int(_), Num::Float(_)) => self.emit(E::As {
                expr: x,
                kind: ScalarKind::Float,
                convert: Some(4),
            }),
            (CastOp::SiToFp, Num::Int(bits), Num::Float(_)) => {
                let wide = self.sext(x, bits, size);
                let signed = self.bitcast(wide, ScalarKind::Sint);
                self.emit(E::As {
                    expr: signed,
                    kind: ScalarKind::Float,
                    convert: Some(4),
                })
            }
            (CastOp::FpTrunc | CastOp::FpExt, Num::Float(_), Num::Float(_)) => {
                if op == CastOp::FpTrunc {
                    self.approximated("f16-as-f32");
                }
                x
            }
            (CastOp::Bitcast, a, b) if a == b => x,
            (CastOp::Bitcast, Num::Float(32), Num::Int(32)) => self.bitcast(x, ScalarKind::Uint),
            (CastOp::Bitcast, Num::Int(32), Num::Float(32)) => self.bitcast(x, ScalarKind::Float),
            // A half is held as a float and an i16 as its 16 bits: the bits go through f16 packing.
            (CastOp::Bitcast, Num::Float(16), Num::Int(16)) if size.is_none() => {
                let zero = self.emit(E::Literal(Literal::F32(0.0)));
                let ty = self.ty(TypeInner::Vector {
                    size: VectorSize::Bi,
                    scalar: Scalar::F32,
                });
                let pair = self.emit(E::Compose {
                    ty,
                    components: vec![x, zero],
                });
                self.math(naga::MathFunction::Pack2x16float, pair)
            }
            (CastOp::Bitcast, Num::Int(16), Num::Float(16)) if size.is_none() => {
                let pair = self.math(naga::MathFunction::Unpack2x16float, x);
                self.emit(E::AccessIndex {
                    base: pair,
                    index: 0,
                })
            }
            _ => return Err(TranslateError::Inst("a cast between these types")),
        })
    }

    fn math(&mut self, fun: naga::MathFunction, arg: Handle<E>) -> Handle<E> {
        self.emit(E::Math {
            fun,
            arg,
            arg1: None,
            arg2: None,
            arg3: None,
        })
    }

    fn bitcast(&mut self, expr: Handle<E>, kind: ScalarKind) -> Handle<E> {
        self.emit(E::As {
            expr,
            kind,
            convert: None,
        })
    }

    fn splat(&mut self, literal: Literal, size: Option<VectorSize>) -> Handle<E> {
        let value = self.emit(E::Literal(literal));
        match size {
            Some(size) => self.emit(E::Splat { size, value }),
            None => value,
        }
    }

    /// The low `bits` of an integer held in 32.
    fn narrow(&mut self, x: Handle<E>, bits: u32, size: Option<VectorSize>) -> Handle<E> {
        if bits >= 32 {
            return x;
        }
        let right = self.splat(Literal::U32(mask(bits)), size);
        self.emit(E::Binary {
            op: Bo::And,
            left: x,
            right,
        })
    }

    /// A `bits`-wide integer held in 32 with its sign carried into the high bits.
    fn sext(&mut self, x: Handle<E>, bits: u32, size: Option<VectorSize>) -> Handle<E> {
        if bits >= 32 {
            return x;
        }
        let shift = self.splat(Literal::U32(32 - bits), size);
        let up = self.emit(E::Binary {
            op: Bo::ShiftLeft,
            left: x,
            right: shift,
        });
        let signed = self.bitcast(up, ScalarKind::Sint);
        let down = self.emit(E::Binary {
            op: Bo::ShiftRight,
            left: signed,
            right: shift,
        });
        self.bitcast(down, ScalarKind::Uint)
    }

    fn constant(&mut self, constant: &Constant, ty: TypeId) -> Result<Handle<E>, TranslateError> {
        Ok(match constant {
            Constant::Undef | Constant::Null => {
                let ty = self.ty_of(ty)?;
                self.emit(E::ZeroValue(ty))
            }
            Constant::Int(value) => self.scalar_constant(ty, *value as u64)?,
            Constant::Float(bits) => self.scalar_constant(ty, *bits)?,
            Constant::Aggregate(items) => {
                let mut components = Vec::with_capacity(items.len());
                for &item in items {
                    components.push(self.value(item)?);
                }
                let ty = self.ty_of(ty)?;
                self.emit(E::Compose { ty, components })
            }
            Constant::Data(words) => {
                let elem = element_type(self.ir, ty)?;
                let mut components = Vec::with_capacity(words.len());
                for &word in words {
                    components.push(self.scalar_constant(elem, word)?);
                }
                let ty = self.ty_of(ty)?;
                self.emit(E::Compose { ty, components })
            }
            Constant::Gep { operands, .. } => {
                let (&base, indices) = operands
                    .split_first()
                    .ok_or(TranslateError::Inst("a getelementptr with no base"))?;
                self.gep(base, indices)?
            }
            Constant::String(_) | Constant::Cast { .. } => {
                return Err(TranslateError::Inst("a constant expression"))
            }
        })
    }

    fn scalar_constant(&mut self, ty: TypeId, word: u64) -> Result<Handle<E>, TranslateError> {
        let (num, size) = self.num(ty)?;
        if size.is_some() {
            return Err(TranslateError::Inst(
                "a vector constant written as one number",
            ));
        }
        Ok(match literal(num, word) {
            Ok(literal) => self.emit(E::Literal(literal)),
            // naga spells no infinity or NaN: the float comes from its bits.
            Err(bits) => {
                let raw = self.emit(E::Literal(Literal::U32(bits)));
                self.bitcast(raw, ScalarKind::Float)
            }
        })
    }

    /// The module-scope variable of an IR global: `groupshared` memory, or a private one with its
    /// constant initializer.
    fn global(
        &mut self,
        id: ValueId,
        ty: TypeId,
        space: u32,
        init: Option<ValueId>,
    ) -> Result<Handle<naga::GlobalVariable>, TranslateError> {
        if let Some(&global) = self.globals.get(&id) {
            return Ok(global);
        }
        let ir = self.ir;
        let Some(ir::Type::Pointer { pointee, .. }) = ir.ty(ty) else {
            return Err(TranslateError::Type(
                "a global that is not a pointer".into(),
            ));
        };
        let ty = self.ty_of(*pointee)?;
        let (space, init) = match (space, init) {
            (GROUPSHARED, _) => (naga::AddressSpace::WorkGroup, None),
            (_, Some(init)) => (
                naga::AddressSpace::Private,
                Some(self.global_constant(init, *pointee, 0)?),
            ),
            (_, None) => (naga::AddressSpace::Private, None),
        };
        let name = self.body.value(id).and_then(|value| value.name.clone());
        let global = naga::GlobalVariable {
            init,
            space,
            ..private(name, ty)
        };
        let global = self.module.global_variables.append(global, Span::UNDEFINED);
        self.globals.insert(id, global);
        Ok(global)
    }

    fn global_constant(
        &mut self,
        id: ValueId,
        ty: TypeId,
        depth: u32,
    ) -> Result<Handle<E>, TranslateError> {
        let (body, ir) = (self.body, self.ir);
        if depth > MAX_DEPTH {
            return Err(TranslateError::Type("a constant nested too deep".into()));
        }
        let Some(ValueKind::Constant(constant)) = body.value(id).map(|value| &value.kind) else {
            return Err(TranslateError::Inst(
                "a global initialised by something other than a constant",
            ));
        };
        let expression = match constant {
            Constant::Undef | Constant::Null => E::ZeroValue(self.ty_of(ty)?),
            Constant::Int(value) => E::Literal(self.global_literal(ty, *value as u64)?),
            Constant::Float(bits) => E::Literal(self.global_literal(ty, *bits)?),
            Constant::Aggregate(items) => {
                let elem = element_type(ir, ty)?;
                let mut components = Vec::with_capacity(items.len());
                for &item in items {
                    components.push(self.global_constant(item, elem, depth + 1)?);
                }
                E::Compose {
                    ty: self.ty_of(ty)?,
                    components,
                }
            }
            Constant::Data(words) => {
                let elem = element_type(ir, ty)?;
                let mut components = Vec::with_capacity(words.len());
                for &word in words {
                    let literal = E::Literal(self.global_literal(elem, word)?);
                    components.push(
                        self.module
                            .global_expressions
                            .append(literal, Span::UNDEFINED),
                    );
                }
                E::Compose {
                    ty: self.ty_of(ty)?,
                    components,
                }
            }
            _ => return Err(TranslateError::Inst("a constant expression")),
        };
        Ok(self
            .module
            .global_expressions
            .append(expression, Span::UNDEFINED))
    }

    fn global_literal(&self, ty: TypeId, word: u64) -> Result<Literal, TranslateError> {
        match self.num(ty)? {
            (num, None) => literal(num, word)
                .map_err(|_| TranslateError::Inst("a non-finite float in a constant table")),
            _ => Err(TranslateError::Inst(
                "a vector constant written as one number",
            )),
        }
    }

    fn local(
        &mut self,
        id: ValueId,
        ty: TypeId,
    ) -> Result<Handle<naga::LocalVariable>, TranslateError> {
        if let Some(&local) = self.locals.get(&id) {
            return Ok(local);
        }
        let ty = self.ty_of(ty)?;
        let local = naga::LocalVariable {
            name: None,
            ty,
            init: None,
        };
        let local = self.b.fun.local_variables.append(local, Span::UNDEFINED);
        self.locals.insert(id, local);
        Ok(local)
    }

    fn ty_of(&mut self, ty: TypeId) -> Result<Handle<naga::Type>, TranslateError> {
        self.ty_at(ty, 0)
    }

    fn ty_at(&mut self, ty: TypeId, depth: u32) -> Result<Handle<naga::Type>, TranslateError> {
        if depth > MAX_DEPTH {
            return Err(TranslateError::Type("a type nested too deep".into()));
        }
        let ir = self.ir;
        let inner = match ir.ty(ty) {
            Some(ir::Type::Array { len, elem }) => {
                let base = self.ty_at(*elem, depth + 1)?;
                let size = u32::try_from(*len)
                    .ok()
                    .and_then(NonZeroU32::new)
                    .ok_or_else(|| TranslateError::Type(format!("an array of {len}")))?;
                self.layouter
                    .update(self.module.to_ctx())
                    .map_err(|_| TranslateError::Type("an array too large to lay out".into()))?;
                let stride = self.layouter[base].to_stride();
                TypeInner::Array {
                    base,
                    size: naga::ArraySize::Constant(size),
                    stride,
                }
            }
            _ => match self.num(ty)? {
                (num, Some(size)) => TypeInner::Vector {
                    size,
                    scalar: scalar_of(num),
                },
                (num, None) => TypeInner::Scalar(scalar_of(num)),
            },
        };
        Ok(insert(&mut self.module, inner))
    }

    fn num(&self, ty: TypeId) -> Result<(Num, Option<VectorSize>), TranslateError> {
        let scalar = |ty: Option<&ir::Type>| match ty {
            Some(ir::Type::Int(1)) => Ok(Num::Bool),
            Some(ir::Type::Int(bits @ (8 | 16 | 32))) => Ok(Num::Int(*bits)),
            Some(ir::Type::Half) => Ok(Num::Float(16)),
            Some(ir::Type::Float) => Ok(Num::Float(32)),
            other => Err(TranslateError::Type(type_word(other))),
        };
        match self.ir.ty(ty) {
            Some(ir::Type::Vector { len, elem }) => {
                let size = vector_size(*len)
                    .ok_or_else(|| TranslateError::Type(format!("a vector of {len}")))?;
                Ok((scalar(self.ir.ty(*elem))?, Some(size)))
            }
            other => Ok((scalar(other)?, None)),
        }
    }

    fn inst_of(&self, id: ValueId) -> Option<&'a ir::Inst> {
        let body = self.body;
        match body.value(id)?.kind {
            ValueKind::Inst { block, index } => {
                body.blocks.get(block as usize)?.insts.get(index as usize)
            }
            _ => None,
        }
    }

    /// The register a handle names, following annotations to the call that created it.
    fn bound(&self, handle: ValueId) -> Result<Bound, TranslateError> {
        let mut id = handle;
        let mut props = None;
        for _ in 0..=MAX_DEPTH {
            let Some(Op::Dx { op, args, .. }) = self.inst_of(id).map(|inst| &inst.op) else {
                return Err(TranslateError::Inst(
                    "a resource handle no dx.op call creates",
                ));
            };
            let op = *op;
            let int = |at: usize| {
                args.get(at)
                    .and_then(|&a| self.int(a))
                    .ok_or_else(|| not_constant(op, "resource class or range"))
            };
            match op {
                DxOp::ANNOTATE_HANDLE => {
                    if props.is_none() {
                        props = args
                            .get(1)
                            .and_then(|&p| self.words(p))
                            .and_then(|w| Some((*w.first()? as u32, *w.get(1)? as u32)));
                    }
                    id = *args
                        .first()
                        .ok_or(TranslateError::Inst("an annotateHandle of no handle"))?;
                }
                DxOp::CREATE_HANDLE_FROM_BINDING => {
                    let words = args
                        .first()
                        .and_then(|&b| self.words(b))
                        .ok_or_else(|| not_constant(op, "binding"))?;
                    let &[lower, _, space, class] = words.as_slice() else {
                        return Err(not_constant(op, "binding"));
                    };
                    let register = Register {
                        class: class_of(class)?,
                        space: space as u32,
                        index: lower as u32,
                    };
                    return Ok(Bound {
                        register,
                        props,
                        op,
                    });
                }
                DxOp::CREATE_HANDLE => {
                    let class = class_of(int(0)?)?;
                    let (space, index) = self.range(class, int(1)?).ok_or(TranslateError::Inst(
                        "a handle of a range dx.resources does not declare",
                    ))?;
                    return Ok(Bound {
                        register: Register {
                            class,
                            space,
                            index,
                        },
                        props,
                        op,
                    });
                }
                DxOp::CREATE_HANDLE_FROM_HEAP => {
                    return Err(TranslateError::Heap {
                        op: op.name().into_owned(),
                    })
                }
                _ => return Err(TranslateError::Inst("a resource handle chosen at run time")),
            }
        }
        Err(TranslateError::Inst(
            "a resource handle annotated without end",
        ))
    }

    /// The integers of a constant struct or vector; a null one is zeros.
    fn words(&self, id: ValueId) -> Option<Vec<i64>> {
        let value = self.body.value(id)?;
        match &value.kind {
            ValueKind::Constant(Constant::Aggregate(items)) => {
                items.iter().map(|&i| self.int(i)).collect()
            }
            ValueKind::Constant(Constant::Data(words)) => {
                Some(words.iter().map(|&w| w as i64).collect())
            }
            ValueKind::Constant(Constant::Null) => match self.ir.ty(value.ty)? {
                ir::Type::Struct { fields, .. } => Some(vec![0; fields.len()]),
                _ => None,
            },
            _ => None,
        }
    }

    /// The space and first register of range `id` of `class` in `dx.resources`.
    fn range(&self, class: ResourceClass, id: i64) -> Option<(u32, u32)> {
        let id = u32::try_from(id).ok()?;
        let d = &self.decls;
        match class {
            ResourceClass::Srv => d
                .srvs
                .iter()
                .find(|r| r.id == id)
                .map(|r| (r.space, r.lower)),
            ResourceClass::Uav | ResourceClass::UavCounter => d
                .uavs
                .iter()
                .find(|r| r.id == id)
                .map(|r| (r.space, r.lower)),
            ResourceClass::Cbv => d
                .cbuffers
                .iter()
                .find(|r| r.id == id)
                .map(|r| (r.space, r.lower)),
            ResourceClass::Sampler => d
                .samplers
                .iter()
                .find(|r| r.id == id)
                .map(|r| (r.space, r.lower)),
        }
    }

    /// Whether `dx.resources` states a range holding `r`, whose shape the plan then already has.
    fn declared(&self, r: Register) -> bool {
        let holds = |space: u32, lower: u32, count: u32| {
            space == r.space
                && r.index >= lower
                && u64::from(r.index) < u64::from(lower) + u64::from(count)
        };
        let d = &self.decls;
        match r.class {
            ResourceClass::Srv => d.srvs.iter().any(|x| holds(x.space, x.lower, x.count)),
            ResourceClass::Uav | ResourceClass::UavCounter => {
                d.uavs.iter().any(|x| holds(x.space, x.lower, x.count))
            }
            ResourceClass::Cbv => d.cbuffers.iter().any(|x| holds(x.space, x.lower, x.count)),
            ResourceClass::Sampler => d.samplers.iter().any(|x| holds(x.space, x.lower, x.count)),
        }
    }

    fn depth_registers(&self) -> HashSet<Register> {
        let compare = [
            DxOp::SAMPLE_CMP,
            DxOp::SAMPLE_CMP_LEVEL_ZERO,
            DxOp::SAMPLE_CMP_LEVEL,
            DxOp::TEXTURE_GATHER_CMP,
        ];
        let mut out = HashSet::new();
        for inst in self.body.blocks.iter().flat_map(|block| &block.insts) {
            if let Op::Dx { op, args, .. } = &inst.op {
                if compare.contains(op) {
                    out.extend(
                        args.first()
                            .and_then(|&h| self.bound(h).ok())
                            .map(|b| b.register),
                    );
                }
            }
        }
        out
    }

    /// The body as a function, and the entry point that binds its private variables.
    fn finish(self, stage: naga::ShaderStage) -> Result<Lowered, TranslateError> {
        let FnCx {
            mut module,
            b,
            inputs,
            outputs,
            builtins,
            lowerings,
            interface,
            failure,
            ..
        } = self;
        if let Some(failure) = failure {
            return Err(failure);
        }
        let mut function = b.finish();
        function.name = Some("main_body".into());
        let function = module.functions.append(function, Span::UNDEFINED);

        let mut w = Builder::new();
        let mut arguments = Vec::new();
        let mut locations: BTreeMap<u32, Vec<(&Element, Handle<naga::GlobalVariable>)>> =
            BTreeMap::new();
        for (e, vars) in &inputs {
            if e.sv == Sv::Arbitrary {
                let register = e.register.ok_or(TranslateError::Inst(
                    "a signature element no register holds",
                ))?;
                for (row, &var) in (0u32..).zip(vars) {
                    locations
                        .entry(register.saturating_add(row))
                        .or_default()
                        .push((e, var));
                }
                continue;
            }
            let (kind, scalar) = built_in(e.sv)?;
            let binding = naga::Binding::BuiltIn(kind);
            let value = if e.sv == Sv::Position {
                let ty = vec4(&mut module, Scalar::F32);
                let x = argument(&mut w, &mut arguments, e, ty, binding);
                w.bits(x, Scalar::F32, e.scalar)
            } else {
                let ty = insert(&mut module, TypeInner::Scalar(scalar));
                let x = argument(&mut w, &mut arguments, e, ty, binding);
                let x = w.convert(x, scalar, e.scalar);
                let nil = w.literal(zero(e.scalar));
                compose4(&mut w, &mut module, e.scalar, vec![x, nil, nil, nil])
            };
            store(&mut w, vars[0], value);
        }
        let fragment = stage == naga::ShaderStage::Fragment;
        for (&location, members) in &locations {
            let first = members[0].0;
            let (interpolation, sampling) = if fragment {
                interpolation_of(first)
            } else {
                (None, None)
            };
            let ty = vec4(&mut module, first.scalar);
            let binding = naga::Binding::Location {
                location,
                interpolation,
                sampling,
                blend_src: None,
                per_primitive: false,
            };
            let x = argument(&mut w, &mut arguments, first, ty, binding);
            for &(e, var) in members {
                let value = if e.start == 0 && e.scalar == first.scalar {
                    x
                } else {
                    // Columns after the element's own belong to no one it reads.
                    let components = (0..4)
                        .map(|c| match e.start + c {
                            col @ 0..=3 => {
                                let v = w.emit(E::AccessIndex {
                                    base: x,
                                    index: col,
                                });
                                w.bits(v, first.scalar, e.scalar)
                            }
                            _ => w.literal(zero(e.scalar)),
                        })
                        .collect();
                    compose4(&mut w, &mut module, e.scalar, components)
                };
                store(&mut w, var, value);
            }
        }
        for &(built_in, ty, var) in &builtins {
            let index = arguments.len() as u32;
            let binding = Some(naga::Binding::BuiltIn(built_in));
            arguments.push(naga::FunctionArgument {
                name: Some(format!("{built_in:?}")),
                ty,
                binding,
            });
            let x = w.emit(E::FunctionArgument(index));
            store(&mut w, var, x);
        }
        w.push(Statement::Call {
            function,
            arguments: Vec::new(),
            result: None,
        });

        // (binding, type, is a vec4, value, name)
        let mut members = Vec::new();
        let mut targets: BTreeMap<u32, Vec<(&Element, Handle<E>)>> = BTreeMap::new();
        for (e, vars) in &outputs {
            let loaded: Vec<Handle<E>> = vars.iter().map(|&var| load(&mut w, var)).collect();
            if matches!(e.sv, Sv::Arbitrary | Sv::Target) {
                let base = match e.sv {
                    Sv::Target => e.semantic_index,
                    _ => e.register.ok_or(TranslateError::Inst(
                        "a signature element no register holds",
                    ))?,
                };
                for (row, &x) in (0u32..).zip(&loaded) {
                    targets
                        .entry(base.saturating_add(row))
                        .or_default()
                        .push((e, x));
                }
                continue;
            }
            let (kind, scalar) = built_in(e.sv)?;
            let binding = naga::Binding::BuiltIn(kind);
            if e.sv == Sv::Position {
                let value = w.bits(loaded[0], e.scalar, Scalar::F32);
                members.push((
                    binding,
                    vec4(&mut module, Scalar::F32),
                    true,
                    value,
                    e.semantic.clone(),
                ));
            } else {
                let x = w.emit(E::AccessIndex {
                    base: loaded[0],
                    index: 0,
                });
                let value = w.convert(x, e.scalar, scalar);
                let ty = insert(&mut module, TypeInner::Scalar(scalar));
                members.push((binding, ty, false, value, e.semantic.clone()));
            }
        }
        for (&location, parts) in &targets {
            let scalar = parts[0].0.scalar;
            let mut components = Vec::with_capacity(4);
            for c in 0..4u32 {
                components.push(match parts.iter().find(|(e, _)| (e.mask >> c) & 1 == 1) {
                    Some(&(e, x)) => {
                        let v = w.emit(E::AccessIndex {
                            base: x,
                            index: c - e.start,
                        });
                        w.bits(v, e.scalar, scalar)
                    }
                    None => w.literal(zero(scalar)),
                });
            }
            let value = compose4(&mut w, &mut module, scalar, components);
            let vertex = stage == naga::ShaderStage::Vertex;
            let (interpolation, sampling) = if vertex {
                interpolation_of(parts[0].0)
            } else {
                (None, None)
            };
            let binding = naga::Binding::Location {
                location,
                interpolation,
                sampling,
                blend_src: None,
                per_primitive: false,
            };
            members.push((
                binding,
                vec4(&mut module, scalar),
                true,
                value,
                format!("location{location}"),
            ));
        }

        let result = if members.is_empty() {
            w.push(Statement::Return { value: None });
            None
        } else {
            let mut offset = 0u32;
            let mut struct_members = Vec::with_capacity(members.len());
            let mut components = Vec::with_capacity(members.len());
            for (binding, ty, wide, value, name) in members {
                let (size, align) = if wide { (16, 16) } else { (4, 4) };
                offset = offset.next_multiple_of(align);
                struct_members.push(naga::StructMember {
                    name: Some(name),
                    ty,
                    binding: Some(binding),
                    offset,
                });
                offset += size;
                components.push(value);
            }
            let inner = TypeInner::Struct {
                members: struct_members,
                span: offset.next_multiple_of(16),
            };
            let ty = module.types.insert(
                naga::Type {
                    name: Some("Output".into()),
                    inner,
                },
                Span::UNDEFINED,
            );
            let value = w.emit(E::Compose { ty, components });
            w.push(Statement::Return { value: Some(value) });
            Some(naga::FunctionResult { ty, binding: None })
        };
        let mut function = w.finish();
        function.name = Some("main".into());
        function.arguments = arguments;
        function.result = result;
        let compute = stage == naga::ShaderStage::Compute;
        module.entry_points.push(naga::EntryPoint {
            name: "main".into(),
            stage,
            early_depth_test: None,
            workgroup_size: if compute {
                interface.threads.unwrap_or([1, 1, 1])
            } else {
                [0; 3]
            },
            workgroup_size_overrides: None,
            function,
            mesh_info: None,
            task_payload: None,
            incoming_ray_payload: None,
        });
        let fidelity = if lowerings.is_empty() {
            Fidelity::Equivalent
        } else {
            Fidelity::Approximated
        };
        Ok(Lowered {
            module,
            fidelity,
            lowerings,
        })
    }
}

struct Bound {
    register: Register,
    /// An annotation's two property words: the resource kind, then the component type or stride.
    props: Option<(u32, u32)>,
    op: DxOp,
}

/// A function's statements as they are built, with the `Emit` of the expressions not yet covered.
#[derive(Default)]
struct Builder {
    fun: naga::Function,
    blocks: Vec<Block>,
    pending: Option<(Handle<E>, Handle<E>)>,
}

impl Builder {
    fn new() -> Self {
        Self {
            blocks: vec![Block::new()],
            ..Self::default()
        }
    }

    fn emit(&mut self, expression: E) -> Handle<E> {
        // naga refuses an `Emit` of an expression in scope from the start, or of a statement's result.
        let covered = !expression.needs_pre_emit()
            && !matches!(
                expression,
                E::CallResult(_)
                    | E::AtomicResult { .. }
                    | E::WorkGroupUniformLoadResult { .. }
                    | E::SubgroupBallotResult
                    | E::SubgroupOperationResult { .. }
                    | E::RayQueryProceedResult
            );
        if !covered {
            self.flush();
        }
        let handle = self.fun.expressions.append(expression, Span::UNDEFINED);
        if covered {
            self.pending = Some((self.pending.map_or(handle, |(first, _)| first), handle));
        }
        handle
    }

    fn flush(&mut self) {
        if let Some((first, last)) = self.pending.take() {
            if let Some(block) = self.blocks.last_mut() {
                block.push(
                    Statement::Emit(naga::Range::new_from_bounds(first, last)),
                    Span::UNDEFINED,
                );
            }
        }
    }

    fn push(&mut self, statement: Statement) {
        self.flush();
        if let Some(block) = self.blocks.last_mut() {
            block.push(statement, Span::UNDEFINED);
        }
    }

    fn open(&mut self) {
        self.flush();
        self.blocks.push(Block::new());
    }

    fn close(&mut self) -> Block {
        self.flush();
        self.blocks.pop().unwrap_or_default()
    }

    fn finish(mut self) -> naga::Function {
        self.flush();
        self.fun.body = self.blocks.drain(..).next().unwrap_or_default();
        self.fun
    }

    fn literal(&mut self, literal: Literal) -> Handle<E> {
        self.emit(E::Literal(literal))
    }

    /// `x` as a value of `to`: a number converted, a boolean as zero or not.
    fn convert(&mut self, x: Handle<E>, from: Scalar, to: Scalar) -> Handle<E> {
        if from == to {
            return x;
        }
        if from.kind == ScalarKind::Bool {
            let accept = self.literal(one(to));
            let reject = self.literal(zero(to));
            return self.emit(E::Select {
                condition: x,
                accept,
                reject,
            });
        }
        if to.kind == ScalarKind::Bool {
            let right = self.literal(zero(from));
            return self.emit(E::Binary {
                op: Bo::NotEqual,
                left: x,
                right,
            });
        }
        self.emit(E::As {
            expr: x,
            kind: to.kind,
            convert: Some(4),
        })
    }

    /// `x`'s bits read as `to`, as two elements sharing a register share them.
    fn bits(&mut self, x: Handle<E>, from: Scalar, to: Scalar) -> Handle<E> {
        if from == to || from.kind == ScalarKind::Bool || to.kind == ScalarKind::Bool {
            return self.convert(x, from, to);
        }
        self.emit(E::As {
            expr: x,
            kind: to.kind,
            convert: None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Num {
    Bool,
    Int(u32),
    Float(u32),
}

/// A signature element as the wrapper binds it.
#[derive(Debug, Clone, PartialEq)]
struct Element {
    semantic: String,
    semantic_index: u32,
    sv: Sv,
    /// The first row's register; `None` for a system value no register holds.
    register: Option<u32>,
    rows: u32,
    /// The register column its first component sits in.
    start: u32,
    /// The register columns it covers.
    mask: u8,
    scalar: Scalar,
    /// DXIL's interpolation mode; 0 where the program states none.
    interpolation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sv {
    Arbitrary,
    VertexId,
    InstanceId,
    Position,
    PrimitiveId,
    SampleIndex,
    FrontFace,
    Coverage,
    Target,
    Depth,
    Other,
}

/// The signature elements in element-id order: from `dx.entryPoints`, else one per `ISG1`/`OSG1` row.
fn elements(
    module: &ir::Module,
    signature: &[SignatureElement],
    output: bool,
) -> Result<Vec<Element>, TranslateError> {
    match md_elements(module, output) {
        Some(listed) => listed,
        None => signature.iter().map(isg_element).collect(),
    }
}

fn md_elements(module: &ir::Module, output: bool) -> Option<Result<Vec<Element>, TranslateError>> {
    let entry = *module.named.get("dx.entryPoints")?.first()?;
    let signatures = md_node(module, entry)?.get(2).copied().flatten()?;
    let Some(list) = md_node(module, signatures)?
        .get(usize::from(output))
        .copied()
        .flatten()
    else {
        return Some(Ok(Vec::new()));
    };
    let Some(nodes) = md_node(module, list) else {
        return Some(Err(unstated()));
    };
    Some(
        nodes
            .iter()
            .enumerate()
            .map(|(position, &node)| md_element(module, node, position))
            .collect(),
    )
}

/// An element record: id, name, component type, semantic kind, semantic indices, interpolation,
/// rows, columns, start row and start column, in the public DXIL documentation's order.
fn md_element(
    module: &ir::Module,
    node: Option<MdId>,
    position: usize,
) -> Result<Element, TranslateError> {
    let ops = node
        .and_then(|node| md_node(module, node))
        .ok_or_else(unstated)?;
    let int = |field: usize| {
        ops.get(field)
            .copied()
            .flatten()
            .and_then(|id| md_int(module, id))
    };
    let field = |field: usize| int(field).ok_or_else(unstated);
    if field(0)? != position as i64 {
        return Err(TranslateError::Inst(
            "a signature whose element ids are not in order",
        ));
    }
    let semantic = match ops
        .get(1)
        .copied()
        .flatten()
        .and_then(|id| module.metadata.get(id as usize))
    {
        Some(Md::String(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    };
    let semantic_index = ops
        .get(4)
        .copied()
        .flatten()
        .and_then(|id| md_node(module, id))
        .and_then(|indices| indices.first().copied().flatten())
        .and_then(|id| md_int(module, id))
        .unwrap_or(0);
    let (rows, cols, row, col) = (field(6)?, field(7)?, field(8)?, field(9)?);
    if !(1..=MAX_ROWS).contains(&rows)
        || !(1..=4).contains(&cols)
        || !(0..4).contains(&col)
        || col + cols > 4
    {
        return Err(unstated());
    }
    Ok(Element {
        semantic,
        semantic_index: u32::try_from(semantic_index).unwrap_or(0),
        sv: sv_of_kind(field(3)?),
        // An element no register holds states row -1.
        register: u32::try_from(row).ok().filter(|&row| row != u32::MAX),
        rows: rows as u32,
        start: col as u32,
        mask: (((1u32 << cols) - 1) << col) as u8,
        scalar: component_scalar(field(2)?)?,
        interpolation: int(5)
            .and_then(|mode| u32::try_from(mode).ok())
            .unwrap_or(0),
    })
}

fn isg_element(e: &SignatureElement) -> Result<Element, TranslateError> {
    let scalar = match e.component_type {
        1 | 2 | 4 | 5 => Scalar::U32,
        3 | 6 => Scalar::F32,
        7 | 8 => return Err(TranslateError::Type("i64".into())),
        9 => return Err(TranslateError::Type("f64".into())),
        other => {
            return Err(TranslateError::Type(format!(
                "signature component type {other}"
            )))
        }
    };
    let mask = e.mask & 0xF;
    Ok(Element {
        semantic: e.semantic.clone(),
        semantic_index: e.index,
        sv: sv_of_name(e.system_value, &e.semantic),
        register: (e.register != u32::MAX).then_some(e.register),
        rows: 1,
        start: if mask == 0 { 0 } else { mask.trailing_zeros() },
        mask,
        scalar,
        interpolation: 0,
    })
}

fn unstated() -> TranslateError {
    TranslateError::Inst("a signature element the metadata does not state in full")
}

fn md_node(module: &ir::Module, id: MdId) -> Option<&[Option<MdId>]> {
    match module.metadata.get(id as usize)? {
        Md::Node(ops) => Some(ops),
        _ => None,
    }
}

/// The integer a metadata value holds; the bitcode writes a zero constant as null.
fn md_int(module: &ir::Module, id: MdId) -> Option<i64> {
    let Md::Value { value, .. } = module.metadata.get(id as usize)? else {
        return None;
    };
    match &module.values.get(*value as usize)?.kind {
        ValueKind::Constant(Constant::Int(value)) => Some(*value),
        ValueKind::Constant(Constant::Null) => Some(0),
        _ => None,
    }
}

/// DXIL's semantic kinds, by the public documentation's numbers.
fn sv_of_kind(kind: i64) -> Sv {
    match kind {
        0 => Sv::Arbitrary,
        1 => Sv::VertexId,
        2 => Sv::InstanceId,
        3 => Sv::Position,
        10 => Sv::PrimitiveId,
        12 => Sv::SampleIndex,
        13 => Sv::FrontFace,
        14 => Sv::Coverage,
        16 => Sv::Target,
        17..=19 => Sv::Depth,
        _ => Sv::Other,
    }
}

/// D3D's system-value names as a signature part states them.
fn sv_of_name(system_value: u32, semantic: &str) -> Sv {
    match system_value {
        0 if semantic.eq_ignore_ascii_case("SV_Target") => Sv::Target,
        0 => Sv::Arbitrary,
        1 => Sv::Position,
        6 => Sv::VertexId,
        7 => Sv::PrimitiveId,
        8 => Sv::InstanceId,
        9 => Sv::FrontFace,
        10 => Sv::SampleIndex,
        64 => Sv::Target,
        65 | 67 | 68 => Sv::Depth,
        66 => Sv::Coverage,
        _ => Sv::Other,
    }
}

/// DXIL's component types: 16-bit ones are held in 32 bits.
fn component_scalar(component: i64) -> Result<Scalar, TranslateError> {
    match component {
        1 => Ok(Scalar::BOOL),
        2..=5 => Ok(Scalar::U32),
        8 | 9 | 11..=14 => Ok(Scalar::F32),
        6 | 7 => Err(TranslateError::Type("i64".into())),
        10 | 15 | 16 => Err(TranslateError::Type("f64".into())),
        other => Err(TranslateError::Type(format!(
            "signature component type {other}"
        ))),
    }
}

fn built_in(sv: Sv) -> Result<(naga::BuiltIn, Scalar), TranslateError> {
    Ok(match sv {
        Sv::VertexId => (naga::BuiltIn::VertexIndex, Scalar::U32),
        Sv::InstanceId => (naga::BuiltIn::InstanceIndex, Scalar::U32),
        Sv::Position => (naga::BuiltIn::Position { invariant: false }, Scalar::F32),
        Sv::PrimitiveId => (naga::BuiltIn::PrimitiveIndex, Scalar::U32),
        Sv::SampleIndex => (naga::BuiltIn::SampleIndex, Scalar::U32),
        Sv::FrontFace => (naga::BuiltIn::FrontFacing, Scalar::BOOL),
        Sv::Coverage => (naga::BuiltIn::SampleMask, Scalar::U32),
        Sv::Depth => (naga::BuiltIn::FragDepth, Scalar::F32),
        Sv::Arbitrary | Sv::Target | Sv::Other => {
            return Err(TranslateError::Inst(
                "a system value naga has no built-in for here",
            ))
        }
    })
}

/// DXIL's interpolation modes; anything but a float is flat.
fn interpolation_of(e: &Element) -> (Option<naga::Interpolation>, Option<naga::Sampling>) {
    use naga::{Interpolation as I, Sampling as S};
    if e.scalar.kind != ScalarKind::Float {
        return (Some(I::Flat), None);
    }
    match e.interpolation {
        1 => (Some(I::Flat), None),
        3 => (Some(I::Perspective), Some(S::Centroid)),
        4 => (Some(I::Linear), Some(S::Center)),
        5 => (Some(I::Linear), Some(S::Centroid)),
        6 => (Some(I::Perspective), Some(S::Sample)),
        7 => (Some(I::Linear), Some(S::Sample)),
        _ => (Some(I::Perspective), Some(S::Center)),
    }
}

/// The shape an annotation states, for a resource `dx.resources` does not declare.
fn annotated(class: ResourceClass, kind_word: u32, detail: u32) -> Option<Planned> {
    use naga::ImageDimension as D;
    let kind = kind_word & 0xFF;
    let image = match kind {
        1 => Some((D::D1, false, false)),
        2 => Some((D::D2, false, false)),
        3 => Some((D::D2, false, true)),
        4 => Some((D::D3, false, false)),
        5 => Some((D::Cube, false, false)),
        6 => Some((D::D1, true, false)),
        7 => Some((D::D2, true, false)),
        8 => Some((D::D2, true, true)),
        9 => Some((D::Cube, true, false)),
        _ => None,
    };
    let sampled = match detail & 0xFF {
        2 | 4 | 6 => ScalarKind::Sint,
        3 | 5 | 7 => ScalarKind::Uint,
        _ => ScalarKind::Float,
    };
    match (class, image) {
        (ResourceClass::Srv, Some((dim, arrayed, multisampled))) => Some(Planned::Texture {
            dim,
            arrayed,
            multisampled,
            sampled,
        }),
        (ResourceClass::Uav, Some((dim, arrayed, _))) => {
            let format = match sampled {
                ScalarKind::Sint => naga::StorageFormat::R32Sint,
                ScalarKind::Uint => naga::StorageFormat::R32Uint,
                _ => naga::StorageFormat::R32Float,
            };
            Some(Planned::StorageTexture {
                dim,
                arrayed,
                format,
            })
        }
        (ResourceClass::Srv | ResourceClass::Uav, None) if (10..=12).contains(&kind) => {
            Some(Planned::Storage {
                read_only: class == ResourceClass::Srv,
                stride: (kind == 12).then_some(detail),
            })
        }
        // Bit 19 of the first word is the sampler's comparison flag.
        (ResourceClass::Sampler, None) if kind == 14 => Some(Planned::Sampler {
            comparison: (kind_word >> 19) & 1 == 1,
        }),
        _ => None,
    }
}

/// DXIL's resource classes: SRV, UAV, CBV, sampler.
fn class_of(code: i64) -> Result<ResourceClass, TranslateError> {
    match code {
        0 => Ok(ResourceClass::Srv),
        1 => Ok(ResourceClass::Uav),
        2 => Ok(ResourceClass::Cbv),
        3 => Ok(ResourceClass::Sampler),
        _ => Err(TranslateError::Inst(
            "a resource class DXIL does not define",
        )),
    }
}

fn not_constant(op: DxOp, what: &'static str) -> TranslateError {
    TranslateError::NotConstant {
        op: op.name().into_owned(),
        what,
    }
}

/// Every phi, and every result read in a block other than its own.
fn crossing(body: &ir::Body) -> HashSet<ValueId> {
    let mut out = HashSet::new();
    for (block, basic) in (0u32..).zip(&body.blocks) {
        for inst in &basic.insts {
            match &inst.op {
                Op::Phi { incoming } => {
                    out.extend(inst.result);
                    for &(value, from) in incoming {
                        used(body, value, from, &mut out, 0);
                    }
                }
                // A pointer is built where it is read, so its operands count as read there.
                Op::Gep { .. } | Op::Alloca { .. } => {}
                op => {
                    for value in operands(op) {
                        used(body, value, block, &mut out, 0);
                    }
                }
            }
        }
    }
    out
}

fn used(body: &ir::Body, value: ValueId, block: BlockId, out: &mut HashSet<ValueId>, depth: u32) {
    let Some(ValueKind::Inst { block: home, index }) = body.value(value).map(|v| &v.kind) else {
        return;
    };
    let inst = body
        .blocks
        .get(*home as usize)
        .and_then(|b| b.insts.get(*index as usize));
    match inst.map(|inst| &inst.op) {
        Some(Op::Gep { base, indices, .. }) if depth < MAX_DEPTH => {
            for &operand in std::iter::once(base).chain(indices) {
                used(body, operand, block, out, depth + 1);
            }
        }
        Some(Op::Gep { .. } | Op::Alloca { .. }) => {}
        _ if *home != block => {
            out.insert(value);
        }
        _ => {}
    }
}

fn operands(op: &Op) -> Vec<ValueId> {
    match op {
        Op::Binary { lhs, rhs, .. } | Op::Cmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        Op::Cast { value, .. } | Op::Load { ptr: value } => vec![*value],
        Op::Select {
            cond,
            then,
            otherwise,
        } => vec![*cond, *then, *otherwise],
        Op::ExtractValue { aggregate, .. } => vec![*aggregate],
        Op::InsertValue {
            aggregate, value, ..
        } => vec![*aggregate, *value],
        Op::ExtractElement { vector, index } => vec![*vector, *index],
        Op::InsertElement {
            vector,
            value,
            index,
        } => vec![*vector, *value, *index],
        Op::ShuffleVector { a, b, mask } => vec![*a, *b, *mask],
        Op::Alloca { count, .. } => vec![*count],
        Op::Store { ptr, value } | Op::AtomicRmw { ptr, value, .. } => vec![*ptr, *value],
        Op::Gep { base, indices, .. } => std::iter::once(*base)
            .chain(indices.iter().copied())
            .collect(),
        Op::CmpXchg { ptr, expected, new } => vec![*ptr, *expected, *new],
        Op::Dx { args, .. } | Op::Call { args, .. } => args.clone(),
        Op::CondBr { cond, .. } => vec![*cond],
        Op::Switch { value, .. } => vec![*value],
        Op::Ret { value } => value.iter().copied().collect(),
        Op::Phi { .. } | Op::Br { .. } | Op::Unreachable => Vec::new(),
    }
}

fn element_type(ir: &ir::Module, ty: TypeId) -> Result<TypeId, TranslateError> {
    match ir.ty(ty) {
        Some(ir::Type::Array { elem, .. } | ir::Type::Vector { elem, .. }) => Ok(*elem),
        _ => Err(TranslateError::Type(
            "an aggregate constant of no array or vector".into(),
        )),
    }
}

fn type_word(ty: Option<&ir::Type>) -> String {
    match ty {
        Some(ir::Type::Int(64)) => "i64".into(),
        Some(ir::Type::Double) => "f64".into(),
        Some(ir::Type::Int(bits)) => format!("i{bits}"),
        Some(ir::Type::Void) => "void".into(),
        Some(ir::Type::Pointer { .. }) => "pointer".into(),
        Some(ir::Type::Struct { .. }) => "struct".into(),
        Some(ir::Type::Array { .. }) => "array".into(),
        Some(ir::Type::Vector { .. }) => "vector of vectors".into(),
        Some(ir::Type::Function { .. }) => "function".into(),
        Some(ir::Type::Label | ir::Type::Metadata | ir::Type::Opaque(_)) => "opaque".into(),
        Some(ir::Type::Half | ir::Type::Float) => "float".into(),
        None => "unknown".into(),
    }
}

fn vector_size(len: u32) -> Option<VectorSize> {
    match len {
        2 => Some(VectorSize::Bi),
        3 => Some(VectorSize::Tri),
        4 => Some(VectorSize::Quad),
        _ => None,
    }
}

fn scalar_of(num: Num) -> Scalar {
    match num {
        Num::Bool => Scalar::BOOL,
        Num::Int(_) => Scalar::U32,
        Num::Float(_) => Scalar::F32,
    }
}

fn mask(bits: u32) -> u32 {
    if bits >= 32 {
        u32::MAX
    } else {
        (1 << bits) - 1
    }
}

/// A scalar constant's literal from its raw word, or the bits of a float naga cannot spell.
fn literal(num: Num, word: u64) -> Result<Literal, u32> {
    Ok(match num {
        Num::Bool => Literal::Bool((word & 1) == 1),
        Num::Int(bits) => Literal::U32((word as u32) & mask(bits)),
        Num::Float(bits) => {
            let value = if bits == 16 {
                half_to_f32(word as u16)
            } else {
                f32::from_bits(word as u32)
            };
            if !value.is_finite() {
                return Err(value.to_bits());
            }
            Literal::F32(value)
        }
    })
}

fn half_to_f32(half: u16) -> f32 {
    let sign = u32::from(half >> 15) << 31;
    let exponent = u32::from((half >> 10) & 0x1F);
    let mantissa = u32::from(half & 0x3FF);
    let bits = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, _) => {
            // A subnormal half is its mantissa times 2^-24, which a float holds exactly.
            let magnitude = mantissa as f32 / 16_777_216.0;
            return if sign == 0 { magnitude } else { -magnitude };
        }
        (0x1F, 0) => sign | 0x7F80_0000,
        (0x1F, _) => sign | 0x7FC0_0000 | (mantissa << 13),
        _ => sign | ((exponent + 112) << 23) | (mantissa << 13),
    };
    f32::from_bits(bits)
}

fn zero(scalar: Scalar) -> Literal {
    match scalar.kind {
        ScalarKind::Bool => Literal::Bool(false),
        ScalarKind::Sint => Literal::I32(0),
        ScalarKind::Float => Literal::F32(0.0),
        _ => Literal::U32(0),
    }
}

/// True as a number: D3D's all-ones for an integer, 1.0 for a float.
fn one(scalar: Scalar) -> Literal {
    match scalar.kind {
        ScalarKind::Bool => Literal::Bool(true),
        ScalarKind::Sint => Literal::I32(-1),
        ScalarKind::Float => Literal::F32(1.0),
        _ => Literal::U32(u32::MAX),
    }
}

fn insert(module: &mut naga::Module, inner: TypeInner) -> Handle<naga::Type> {
    module
        .types
        .insert(naga::Type { name: None, inner }, Span::UNDEFINED)
}

fn vec4(module: &mut naga::Module, scalar: Scalar) -> Handle<naga::Type> {
    insert(
        module,
        TypeInner::Vector {
            size: VectorSize::Quad,
            scalar,
        },
    )
}

fn private(name: Option<String>, ty: Handle<naga::Type>) -> naga::GlobalVariable {
    naga::GlobalVariable {
        name,
        space: naga::AddressSpace::Private,
        binding: None,
        ty,
        init: None,
        memory_decorations: naga::MemoryDecorations::empty(),
    }
}

fn privates(module: &mut naga::Module, elements: Vec<Element>, prefix: &str) -> Vec<Io> {
    elements
        .into_iter()
        .map(|e| {
            let ty = vec4(module, e.scalar);
            let vars = (0..e.rows)
                .map(|row| {
                    let name = format!("{prefix}_{}{}_{row}", e.semantic, e.semantic_index);
                    module
                        .global_variables
                        .append(private(Some(name), ty), Span::UNDEFINED)
                })
                .collect();
            (e, vars)
        })
        .collect()
}

fn argument(
    w: &mut Builder,
    arguments: &mut Vec<naga::FunctionArgument>,
    e: &Element,
    ty: Handle<naga::Type>,
    binding: naga::Binding,
) -> Handle<E> {
    let index = arguments.len() as u32;
    let name = Some(format!("{}{}", e.semantic, e.semantic_index));
    arguments.push(naga::FunctionArgument {
        name,
        ty,
        binding: Some(binding),
    });
    w.emit(E::FunctionArgument(index))
}

fn compose4(
    w: &mut Builder,
    module: &mut naga::Module,
    scalar: Scalar,
    components: Vec<Handle<E>>,
) -> Handle<E> {
    let ty = vec4(module, scalar);
    w.emit(E::Compose { ty, components })
}

fn store(w: &mut Builder, var: Handle<naga::GlobalVariable>, value: Handle<E>) {
    let pointer = w.emit(E::GlobalVariable(var));
    w.push(Statement::Store { pointer, value });
}

fn load(w: &mut Builder, var: Handle<naga::GlobalVariable>) -> Handle<E> {
    let pointer = w.emit(E::GlobalVariable(var));
    w.emit(E::Load { pointer })
}

#[cfg(test)]
mod tests;
