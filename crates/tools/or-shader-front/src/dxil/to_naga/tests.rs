use naga::valid::{Capabilities, ValidationFlags, Validator};

use super::*;
use crate::dxil::ir::{BasicBlock, Body, Function, Inst, Module, Type, Value};
use crate::dxil::plan::PlannedBinding;

const VOID: TypeId = 0;
const FLOAT: TypeId = 1;
const I32: TypeId = 2;
const I8: TypeId = 3;
const I1: TypeId = 4;
const I64: TypeId = 5;
const RES_BIND: TypeId = 6;
const HANDLE: TypeId = 7;
const PROPS: TypeId = 8;
const I16: TypeId = 9;
const ARRAY: TypeId = 10;
const ARRAY_PTR: TypeId = 11;
const FLOAT_PTR: TypeId = 12;

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
            Type::Int(64),
            record("dx.types.ResBind", vec![I32, I32, I32, I8]),
            record("dx.types.Handle", Vec::new()),
            record("dx.types.ResourceProperties", vec![I32, I32]),
            Type::Int(16),
            Type::Array {
                len: 4,
                elem: FLOAT,
            },
            Type::Pointer {
                pointee: ARRAY,
                space: 0,
            },
            Type::Pointer {
                pointee: FLOAT,
                space: 0,
            },
        ],
        ..Module::default()
    }
}

struct Program {
    values: Vec<Value>,
    blocks: Vec<BasicBlock>,
}

impl Program {
    fn new(blocks: usize) -> Self {
        Self {
            values: Vec::new(),
            blocks: vec![BasicBlock::default(); blocks],
        }
    }

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

    fn inst(&mut self, block: BlockId, ty: TypeId, op: Op) -> ValueId {
        let id = self.values.len() as ValueId;
        let index = self.blocks[block as usize].insts.len() as u32;
        self.values.push(Value {
            ty,
            kind: ValueKind::Inst { block, index },
            name: None,
        });
        self.blocks[block as usize].insts.push(Inst {
            result: Some(id),
            ty,
            op,
        });
        id
    }

    fn effect(&mut self, block: BlockId, op: Op) {
        self.blocks[block as usize].insts.push(Inst {
            result: None,
            ty: VOID,
            op,
        });
    }

    fn load_input(&mut self, block: BlockId, ty: TypeId, element: i64, column: i64) -> ValueId {
        let args = vec![
            self.int(I32, element),
            self.int(I32, 0),
            self.int(I8, column),
            self.int(I32, 0),
        ];
        self.inst(
            block,
            ty,
            Op::Dx {
                op: DxOp::LOAD_INPUT,
                overload: Some(ty),
                args,
            },
        )
    }

    fn store_output(
        &mut self,
        block: BlockId,
        ty: TypeId,
        element: i64,
        column: i64,
        value: ValueId,
    ) {
        let args = vec![
            self.int(I32, element),
            self.int(I32, 0),
            self.int(I8, column),
            value,
        ];
        self.effect(
            block,
            Op::Dx {
                op: DxOp::STORE_OUTPUT,
                overload: Some(ty),
                args,
            },
        );
    }

    fn body(self) -> Body {
        Body {
            blocks: self.blocks,
            values: self.values,
        }
    }
}

fn sig(
    semantic: &str,
    index: u32,
    system_value: u32,
    component: u32,
    register: u32,
    mask: u8,
) -> SignatureElement {
    SignatureElement {
        semantic: semantic.into(),
        index,
        system_value,
        component_type: component,
        register,
        mask,
    }
}

fn target() -> SignatureElement {
    sig("SV_Target", 0, 64, 3, 0, 0xF)
}

fn lower_in(
    module: &Module,
    kind: ShaderKind,
    inputs: Vec<SignatureElement>,
    outputs: Vec<SignatureElement>,
    program: Program,
    nodes: &[Node],
) -> Result<Lowered, TranslateError> {
    let body = program.body();
    let interface = Interface {
        kind,
        inputs,
        outputs,
        threads: None,
    };
    to_naga(
        module,
        &body,
        nodes,
        &interface,
        &mut BindingPlan::default(),
    )
}

fn lower(
    kind: ShaderKind,
    inputs: Vec<SignatureElement>,
    outputs: Vec<SignatureElement>,
    program: Program,
    nodes: &[Node],
) -> Lowered {
    let lowered = lower_in(&module(), kind, inputs, outputs, program, nodes).unwrap();
    Validator::new(ValidationFlags::all(), Capabilities::default())
        .validate(&lowered.module)
        .unwrap();
    lowered
}

fn location(
    location: u32,
    interpolation: Option<naga::Interpolation>,
    sampling: Option<naga::Sampling>,
) -> Option<naga::Binding> {
    Some(naga::Binding::Location {
        location,
        interpolation,
        sampling,
        blend_src: None,
        per_primitive: false,
    })
}

fn result_bindings(lowered: &Lowered) -> Vec<Option<naga::Binding>> {
    let result = lowered.module.entry_points[0]
        .function
        .result
        .as_ref()
        .unwrap();
    let TypeInner::Struct { members, .. } = &lowered.module.types[result.ty].inner else {
        panic!("the entry point returns no struct");
    };
    members.iter().map(|m| m.binding.clone()).collect()
}

fn argument_bindings(lowered: &Lowered) -> Vec<Option<naga::Binding>> {
    let entry = &lowered.module.entry_points[0];
    entry
        .function
        .arguments
        .iter()
        .map(|a| a.binding.clone())
        .collect()
}

fn body_of(lowered: &Lowered) -> &naga::Function {
    let mut functions = lowered.module.functions.iter();
    functions.next().unwrap().1
}

#[test]
fn a_vertex_program_binds_its_location_and_writes_the_position() {
    let mut p = Program::new(1);
    let two = p.float(2.0);
    for column in 0..4 {
        let x = p.load_input(0, FLOAT, 0, column);
        let y = match column {
            0 => p.inst(
                0,
                FLOAT,
                Op::Binary {
                    op: BinOp::FMul,
                    lhs: x,
                    rhs: two,
                },
            ),
            _ => x,
        };
        p.store_output(0, FLOAT, 0, column, y);
    }
    p.effect(0, Op::Ret { value: None });
    let inputs = vec![sig("POSITION", 0, 0, 3, 0, 0xF)];
    let outputs = vec![sig("SV_Position", 0, 1, 3, 0, 0xF)];
    let lowered = lower(
        ShaderKind::Vertex,
        inputs,
        outputs,
        p,
        &[Node::Code(0), Node::Return(None)],
    );

    assert_eq!(
        lowered.module.entry_points[0].stage,
        naga::ShaderStage::Vertex
    );
    assert_eq!(argument_bindings(&lowered), [location(0, None, None)]);
    let position = naga::Binding::BuiltIn(naga::BuiltIn::Position { invariant: false });
    assert_eq!(result_bindings(&lowered), [Some(position)]);
    assert_eq!(lowered.fidelity, Fidelity::Equivalent);
    assert!(lowered.lowerings.is_empty());
}

#[test]
fn a_diamond_carries_its_phi_and_a_value_read_in_two_blocks_through_locals() {
    let mut p = Program::new(4);
    let zero = p.float(0.0);
    let one = p.float(1.0);
    let x = p.load_input(0, FLOAT, 0, 0);
    let negative = p.inst(
        0,
        I1,
        Op::Cmp {
            pred: Pred::FOlt,
            lhs: x,
            rhs: zero,
        },
    );
    p.effect(
        0,
        Op::CondBr {
            cond: negative,
            then: 1,
            otherwise: 2,
        },
    );
    p.effect(1, Op::Br { target: 3 });
    let square = p.inst(
        2,
        FLOAT,
        Op::Binary {
            op: BinOp::FMul,
            lhs: x,
            rhs: x,
        },
    );
    p.effect(2, Op::Br { target: 3 });
    let merged = p.inst(
        3,
        FLOAT,
        Op::Phi {
            incoming: vec![(one, 1), (square, 2)],
        },
    );
    p.store_output(3, FLOAT, 0, 0, merged);
    p.store_output(3, FLOAT, 0, 1, x);
    p.effect(3, Op::Ret { value: None });
    let nodes = [
        Node::Code(0),
        Node::If {
            cond: negative,
            then: vec![Node::Code(1), Node::Edge { from: 1, to: 3 }],
            otherwise: vec![Node::Code(2), Node::Edge { from: 2, to: 3 }],
        },
        Node::Code(3),
        Node::Return(None),
    ];
    let inputs = vec![sig("TEXCOORD", 0, 0, 3, 1, 0x3)];
    let lowered = lower(ShaderKind::Pixel, inputs, vec![target()], p, &nodes);

    // The phi and `x`, which block 0 computes and blocks 2 and 3 read.
    assert_eq!(body_of(&lowered).local_variables.len(), 2);
    let perspective = Some(naga::Interpolation::Perspective);
    assert_eq!(
        argument_bindings(&lowered),
        [location(1, perspective, Some(naga::Sampling::Center))]
    );
    assert_eq!(result_bindings(&lowered), [location(0, None, None)]);
}

#[test]
fn a_counted_loop_continues_through_its_header_and_breaks_to_its_exit() {
    let mut p = Program::new(3);
    let start = p.int(I32, 0);
    let step = p.int(I32, 1);
    let limit = p.int(I32, 4);
    p.effect(0, Op::Br { target: 1 });
    let counter = p.inst(
        1,
        I32,
        Op::Phi {
            incoming: Vec::new(),
        },
    );
    let next = p.inst(
        1,
        I32,
        Op::Binary {
            op: BinOp::Add,
            lhs: counter,
            rhs: step,
        },
    );
    p.blocks[1].insts[0].op = Op::Phi {
        incoming: vec![(start, 0), (next, 1)],
    };
    let more = p.inst(
        1,
        I1,
        Op::Cmp {
            pred: Pred::IUlt,
            lhs: next,
            rhs: limit,
        },
    );
    p.effect(
        1,
        Op::CondBr {
            cond: more,
            then: 1,
            otherwise: 2,
        },
    );
    let value = p.inst(
        2,
        FLOAT,
        Op::Cast {
            op: CastOp::UiToFp,
            value: next,
        },
    );
    p.store_output(2, FLOAT, 0, 0, value);
    p.effect(2, Op::Ret { value: None });
    let nodes = [
        Node::Code(0),
        Node::Edge { from: 0, to: 1 },
        Node::Loop {
            body: vec![
                Node::Code(1),
                Node::If {
                    cond: more,
                    then: vec![Node::Edge { from: 1, to: 1 }, Node::Continue],
                    otherwise: vec![Node::Break],
                },
            ],
        },
        Node::Code(2),
        Node::Return(None),
    ];
    let lowered = lower(ShaderKind::Pixel, Vec::new(), vec![target()], p, &nodes);
    assert!(body_of(&lowered)
        .body
        .iter()
        .any(|s| matches!(s, Statement::Loop { .. })));
}

#[test]
fn a_switch_groups_its_case_values_and_ends_in_its_default() {
    let mut p = Program::new(4);
    let x = p.load_input(0, I32, 0, 0);
    p.effect(
        0,
        Op::Switch {
            value: x,
            default: 3,
            cases: vec![(1, 1), (2, 1), (5, 2)],
        },
    );
    p.effect(1, Op::Br { target: 3 });
    p.effect(2, Op::Br { target: 3 });
    let one = p.float(1.0);
    p.store_output(3, FLOAT, 0, 0, one);
    p.effect(3, Op::Ret { value: None });
    let nodes = [
        Node::Code(0),
        Node::Switch {
            value: x,
            cases: vec![
                (vec![1, 2], vec![Node::Code(1)]),
                (vec![5], vec![Node::Code(2)]),
            ],
            default: Vec::new(),
        },
        Node::Code(3),
        Node::Return(None),
    ];
    let inputs = vec![sig("BLENDINDICES", 0, 0, 1, 2, 0x1)];
    let lowered = lower(ShaderKind::Pixel, inputs, vec![target()], p, &nodes);

    assert_eq!(
        argument_bindings(&lowered),
        [location(2, Some(naga::Interpolation::Flat), None)]
    );
    let cases = body_of(&lowered).body.iter().find_map(|s| match s {
        Statement::Switch { cases, .. } => Some(
            cases
                .iter()
                .map(|c| (c.value, c.fall_through))
                .collect::<Vec<_>>(),
        ),
        _ => None,
    });
    use naga::SwitchValue as V;
    assert_eq!(
        cases,
        Some(vec![
            (V::U32(1), true),
            (V::U32(2), false),
            (V::U32(5), false),
            (V::Default, false)
        ])
    );
}

#[test]
fn two_elements_packed_in_one_register_leave_as_one_location() {
    let mut p = Program::new(1);
    let value = p.float(0.5);
    let bits = p.int(I32, 7);
    for column in 0..4 {
        p.store_output(0, FLOAT, 0, column, value);
    }
    for column in 0..2 {
        p.store_output(0, FLOAT, 1, column, value);
        p.store_output(0, I32, 2, column, bits);
    }
    p.effect(0, Op::Ret { value: None });
    let outputs = vec![
        sig("SV_Position", 0, 1, 3, 0, 0xF),
        sig("TEXCOORD", 0, 0, 3, 1, 0x3),
        sig("TEXCOORD", 1, 0, 1, 1, 0xC),
    ];
    let lowered = lower(
        ShaderKind::Vertex,
        Vec::new(),
        outputs,
        p,
        &[Node::Code(0), Node::Return(None)],
    );

    let position = naga::Binding::BuiltIn(naga::BuiltIn::Position { invariant: false });
    let interpolated = location(
        1,
        Some(naga::Interpolation::Perspective),
        Some(naga::Sampling::Center),
    );
    assert_eq!(result_bindings(&lowered), [Some(position), interpolated]);
}

#[test]
fn signed_and_narrow_integers_lower_to_bitcasts_and_count_as_approximated() {
    let mut p = Program::new(1);
    let x = p.load_input(0, I32, 0, 0);
    let three = p.int(I32, 3);
    let shifted = p.inst(
        0,
        I32,
        Op::Binary {
            op: BinOp::AShr,
            lhs: x,
            rhs: three,
        },
    );
    let quotient = p.inst(
        0,
        I32,
        Op::Binary {
            op: BinOp::SDiv,
            lhs: shifted,
            rhs: x,
        },
    );
    let narrow = p.inst(
        0,
        I16,
        Op::Cast {
            op: CastOp::Trunc,
            value: quotient,
        },
    );
    let wide = p.inst(
        0,
        I32,
        Op::Cast {
            op: CastOp::SExt,
            value: narrow,
        },
    );
    let value = p.inst(
        0,
        FLOAT,
        Op::Cast {
            op: CastOp::SiToFp,
            value: wide,
        },
    );
    p.store_output(0, FLOAT, 0, 0, value);
    p.effect(0, Op::Ret { value: None });
    let inputs = vec![sig("BLENDINDICES", 0, 0, 2, 0, 0x1)];
    let lowered = lower(
        ShaderKind::Pixel,
        inputs,
        vec![target()],
        p,
        &[Node::Code(0), Node::Return(None)],
    );

    assert_eq!(lowered.fidelity, Fidelity::Approximated);
    assert_eq!(
        lowered.lowerings,
        BTreeMap::from([("i16-as-i32".to_string(), 1)])
    );
    let body = body_of(&lowered);
    let signed_shift = body.expressions.iter().any(|(_, e)| match *e {
        E::Binary {
            op: Bo::ShiftRight,
            left,
            ..
        } => {
            matches!(
                body.expressions[left],
                E::As {
                    kind: ScalarKind::Sint,
                    convert: None,
                    ..
                }
            )
        }
        _ => false,
    });
    assert!(signed_shift);
}

#[test]
fn a_local_array_is_indexed_through_its_alloca_and_getelementptr() {
    let mut p = Program::new(1);
    let one = p.int(I32, 1);
    let zero = p.int(I32, 0);
    let array = p.inst(
        0,
        ARRAY_PTR,
        Op::Alloca {
            ty: ARRAY,
            count: one,
        },
    );
    let index = p.load_input(0, I32, 0, 0);
    let slot = p.inst(
        0,
        FLOAT_PTR,
        Op::Gep {
            inbounds: true,
            base: array,
            indices: vec![zero, index],
        },
    );
    let value = p.float(2.5);
    p.effect(0, Op::Store { ptr: slot, value });
    let read = p.inst(0, FLOAT, Op::Load { ptr: slot });
    p.store_output(0, FLOAT, 0, 0, read);
    p.effect(0, Op::Ret { value: None });
    let inputs = vec![sig("BLENDINDICES", 0, 0, 1, 0, 0x1)];
    let lowered = lower(
        ShaderKind::Pixel,
        inputs,
        vec![target()],
        p,
        &[Node::Code(0), Node::Return(None)],
    );
    let locals = &body_of(&lowered).local_variables;
    assert_eq!(locals.len(), 1);
    let (_, local) = locals.iter().next().unwrap();
    assert!(matches!(
        lowered.module.types[local.ty].inner,
        TypeInner::Array { .. }
    ));
}

#[test]
fn a_long_chain_of_getelementptr_is_refused_and_never_takes_the_stack() {
    let mut p = Program::new(1);
    let one = p.int(I32, 1);
    let zero = p.int(I32, 0);
    let mut pointer = p.inst(
        0,
        ARRAY_PTR,
        Op::Alloca {
            ty: ARRAY,
            count: one,
        },
    );
    for _ in 0..100_000 {
        pointer = p.inst(
            0,
            ARRAY_PTR,
            Op::Gep {
                inbounds: true,
                base: pointer,
                indices: vec![zero],
            },
        );
    }
    let read = p.inst(0, FLOAT, Op::Load { ptr: pointer });
    p.store_output(0, FLOAT, 0, 0, read);
    p.effect(0, Op::Ret { value: None });
    let nodes = [Node::Code(0), Node::Return(None)];
    let refused = lower_in(
        &module(),
        ShaderKind::Pixel,
        Vec::new(),
        vec![target()],
        p,
        &nodes,
    );
    assert!(matches!(refused, Err(TranslateError::Inst(_))));
}

#[test]
fn a_compute_program_reads_its_thread_id_and_keeps_its_group_size() {
    let mut p = Program::new(1);
    let y = p.int(I32, 1);
    let id = p.inst(
        0,
        I32,
        Op::Dx {
            op: DxOp::THREAD_ID,
            overload: Some(I32),
            args: vec![y],
        },
    );
    p.inst(
        0,
        FLOAT,
        Op::Cast {
            op: CastOp::UiToFp,
            value: id,
        },
    );
    p.effect(0, Op::Ret { value: None });
    let body = p.body();
    let interface = Interface {
        kind: ShaderKind::Compute,
        inputs: Vec::new(),
        outputs: Vec::new(),
        threads: Some([8, 4, 2]),
    };
    let nodes = [Node::Code(0), Node::Return(None)];
    let lowered = to_naga(
        &module(),
        &body,
        &nodes,
        &interface,
        &mut BindingPlan::default(),
    )
    .unwrap();
    Validator::new(ValidationFlags::all(), Capabilities::default())
        .validate(&lowered.module)
        .unwrap();
    let entry = &lowered.module.entry_points[0];
    assert_eq!(entry.workgroup_size, [8, 4, 2]);
    let global_id = naga::Binding::BuiltIn(naga::BuiltIn::GlobalInvocationId);
    assert_eq!(argument_bindings(&lowered), [Some(global_id)]);
}

#[test]
fn what_the_first_version_does_not_lower_is_refused_by_name() {
    let mut p = Program::new(1);
    let wide = p.int(I64, 1);
    p.inst(
        0,
        I64,
        Op::Binary {
            op: BinOp::Add,
            lhs: wide,
            rhs: wide,
        },
    );
    p.effect(0, Op::Ret { value: None });
    let refused = lower_in(
        &module(),
        ShaderKind::Pixel,
        Vec::new(),
        Vec::new(),
        p,
        &[Node::Code(0)],
    );
    assert_eq!(refused.err(), Some(TranslateError::Type("i64".into())));

    let mut with_calls = module();
    with_calls.functions = ["llvm.lifetime.start.p0i8", "rand"]
        .iter()
        .map(|name| Function {
            name: (*name).into(),
            ty: VOID,
            body: None,
        })
        .collect();
    let mut p = Program::new(1);
    let lifetime = p.constant(VOID, Constant::Null);
    p.values[lifetime as usize].kind = ValueKind::Function(0);
    let rand = p.constant(VOID, Constant::Null);
    p.values[rand as usize].kind = ValueKind::Function(1);
    p.effect(
        0,
        Op::Call {
            callee: lifetime,
            args: Vec::new(),
        },
    );
    p.effect(
        0,
        Op::Call {
            callee: rand,
            args: Vec::new(),
        },
    );
    let refused = lower_in(
        &with_calls,
        ShaderKind::Pixel,
        Vec::new(),
        Vec::new(),
        p,
        &[Node::Code(0)],
    );
    assert_eq!(refused.err(), Some(TranslateError::Call("rand".into())));

    let refused = lower_in(
        &module(),
        ShaderKind::Hull,
        Vec::new(),
        Vec::new(),
        Program::new(1),
        &[],
    );
    assert!(matches!(refused, Err(TranslateError::Inst(_))));

    let mut p = Program::new(1);
    p.load_input(0, FLOAT, 3, 0);
    let refused = lower_in(
        &module(),
        ShaderKind::Pixel,
        Vec::new(),
        Vec::new(),
        p,
        &[Node::Code(0)],
    );
    assert!(matches!(refused, Err(TranslateError::Inst(_))));
}

#[test]
fn a_handle_resolves_to_its_planned_binding_shaped_by_its_annotation_and_use() {
    let mut p = Program::new(1);
    let bind = |p: &mut Program, register: i64| {
        let (lower, upper, space, class) = (
            p.int(I32, register),
            p.int(I32, register),
            p.int(I32, 1),
            p.int(I8, 0),
        );
        let bind = p.constant(
            RES_BIND,
            Constant::Aggregate(vec![lower, upper, space, class]),
        );
        let (index, uniform) = (p.int(I32, register), p.int(I1, 0));
        let args = vec![bind, index, uniform];
        p.inst(
            0,
            HANDLE,
            Op::Dx {
                op: DxOp::CREATE_HANDLE_FROM_BINDING,
                overload: None,
                args,
            },
        )
    };
    let created = bind(&mut p, 3);
    let (cube, float) = (p.int(I32, 5), p.int(I32, 9));
    let props = p.constant(PROPS, Constant::Aggregate(vec![cube, float]));
    let annotated = p.inst(
        0,
        HANDLE,
        Op::Dx {
            op: DxOp::ANNOTATE_HANDLE,
            overload: None,
            args: vec![created, props],
        },
    );
    let shadow = bind(&mut p, 4);
    p.effect(
        0,
        Op::Dx {
            op: DxOp::SAMPLE_CMP,
            overload: Some(FLOAT),
            args: vec![shadow],
        },
    );
    let unbound = bind(&mut p, 9);
    let index = p.int(I32, 0);
    let heap = p.inst(
        0,
        HANDLE,
        Op::Dx {
            op: DxOp::CREATE_HANDLE_FROM_HEAP,
            overload: None,
            args: vec![index],
        },
    );
    p.effect(0, Op::Ret { value: None });
    let (module, body) = (module(), p.body());

    let srv = |index| Register {
        class: ResourceClass::Srv,
        space: 1,
        index,
    };
    let flat = Planned::Texture {
        dim: naga::ImageDimension::D2,
        arrayed: false,
        multisampled: false,
        sampled: ScalarKind::Float,
    };
    let planned = |index: u32| PlannedBinding {
        register: srv(index),
        group: 1,
        binding: index - 3,
        name: format!("t{index}"),
        planned: flat.clone(),
    };
    let mut plan = BindingPlan {
        bindings: vec![planned(3), planned(4)],
    };
    let interface = Interface {
        kind: ShaderKind::Pixel,
        inputs: Vec::new(),
        outputs: Vec::new(),
        threads: None,
    };
    let mut cx = FnCx::new(
        &module,
        &body,
        &mut plan,
        &interface,
        Vec::new(),
        Vec::new(),
    );

    let t3 = cx.resource(annotated).unwrap();
    assert_eq!(cx.resource(created), Ok(t3));
    let global = &cx.module.global_variables[t3];
    assert_eq!(
        global.binding,
        Some(naga::ResourceBinding {
            group: 1,
            binding: 0
        })
    );
    let sampled = naga::ImageClass::Sampled {
        kind: ScalarKind::Float,
        multi: false,
    };
    let cube_image = TypeInner::Image {
        dim: naga::ImageDimension::Cube,
        arrayed: false,
        class: sampled,
    };
    assert_eq!(cx.module.types[global.ty].inner, cube_image);

    let t4 = cx.resource(shadow).unwrap();
    let depth = naga::ImageClass::Depth { multi: false };
    let shadow_image = TypeInner::Image {
        dim: naga::ImageDimension::D2,
        arrayed: false,
        class: depth,
    };
    assert_eq!(
        cx.module.types[cx.module.global_variables[t4].ty].inner,
        shadow_image
    );

    assert_eq!(
        cx.resource(heap),
        Err(TranslateError::Heap {
            op: "CreateHandleFromHeap".into()
        })
    );
    let op = "CreateHandleFromBinding".to_string();
    assert_eq!(
        cx.resource(unbound),
        Err(TranslateError::Unbound {
            op,
            register: srv(9)
        })
    );
    drop(cx);
    assert!(matches!(
        plan.bindings[0].planned,
        Planned::Texture {
            dim: naga::ImageDimension::Cube,
            ..
        }
    ));
    assert_eq!(plan.bindings[1].planned, flat);
}

/// An element record: id, name, component type, semantic kind, semantic index, interpolation,
/// rows, columns, start row, start column.
type Row<'a> = (i64, &'a str, i64, i64, i64, i64, i64, i64, i64, i64);

/// A module whose `dx.entryPoints` lists these input elements.
fn with_inputs(elements: &[Row]) -> Module {
    let mut m = module();
    let int = |m: &mut Module, value: i64| {
        m.values.push(Value {
            ty: I32,
            kind: ValueKind::Constant(Constant::Int(value)),
            name: None,
        });
        m.metadata.push(Md::Value {
            ty: I32,
            value: m.values.len() as ValueId - 1,
        });
        Some(m.metadata.len() as MdId - 1)
    };
    let push = |m: &mut Module, md: Md| {
        m.metadata.push(md);
        Some(m.metadata.len() as MdId - 1)
    };
    let mut nodes = Vec::new();
    for &(id, name, component, kind, index, interpolation, rows, cols, row, col) in elements {
        let name = push(&mut m, Md::String(name.as_bytes().to_vec()));
        let index = int(&mut m, index);
        let indices = push(&mut m, Md::Node(vec![index]));
        let mut ops = vec![
            int(&mut m, id),
            name,
            int(&mut m, component),
            int(&mut m, kind),
            indices,
        ];
        for field in [interpolation, rows, cols, row, col] {
            ops.push(int(&mut m, field));
        }
        ops.push(None);
        nodes.push(push(&mut m, Md::Node(ops)));
    }
    let inputs = push(&mut m, Md::Node(nodes));
    let signatures = push(&mut m, Md::Node(vec![inputs, None, None]));
    let entry = push(&mut m, Md::Node(vec![None, None, signatures, None, None]));
    m.named
        .insert("dx.entryPoints".into(), vec![entry.unwrap()]);
    m
}

#[test]
fn signature_elements_come_from_the_entry_point_metadata_before_the_signature_part() {
    let m = with_inputs(&[
        (0, "TEXCOORD", 9, 0, 5, 2, 2, 2, 3, 2),
        (1, "SV_VertexID", 5, 1, 0, 0, 1, 1, -1, 0),
    ]);
    let part = vec![sig("IGNORED", 7, 0, 3, 6, 0xF)];
    let inputs = elements(&m, &part, false).unwrap();
    let texcoord = Element {
        semantic: "TEXCOORD".into(),
        semantic_index: 5,
        sv: Sv::Arbitrary,
        register: Some(3),
        rows: 2,
        start: 2,
        mask: 0b1100,
        scalar: Scalar::F32,
        interpolation: 2,
    };
    let vertex_id = Element {
        semantic: "SV_VertexID".into(),
        semantic_index: 0,
        sv: Sv::VertexId,
        register: None,
        rows: 1,
        start: 0,
        mask: 0b1,
        scalar: Scalar::U32,
        interpolation: 0,
    };
    assert_eq!(inputs, [texcoord, vertex_id]);
    assert!(elements(&m, &part, true).unwrap().is_empty());
    let privates = privates(&mut naga::Module::default(), inputs, "in");
    assert_eq!(
        privates
            .iter()
            .map(|(_, rows)| rows.len())
            .collect::<Vec<_>>(),
        [2, 1]
    );

    let from_part = elements(&module(), &part, false).unwrap();
    assert_eq!(
        (
            from_part[0].register,
            from_part[0].semantic_index,
            from_part[0].mask
        ),
        (Some(6), 7, 0xF)
    );

    let shuffled = with_inputs(&[(1, "TEXCOORD", 9, 0, 0, 2, 1, 4, 0, 0)]);
    assert!(elements(&shuffled, &part, false).is_err());
    let unbounded = with_inputs(&[(0, "TEXCOORD", 9, 0, 0, 2, 1, 3, 0, 2)]);
    assert!(elements(&unbounded, &part, false).is_err());
}

#[test]
fn half_words_widen_to_the_float_they_name() {
    assert_eq!(half_to_f32(0x3C00), 1.0);
    assert_eq!(half_to_f32(0xC000), -2.0);
    assert_eq!(half_to_f32(0x3555), (1.0 + 341.0 / 1024.0) * 0.25);
    assert_eq!(half_to_f32(0x0001), 2f32.powi(-24));
    assert_eq!(half_to_f32(0x83FF), -(1023.0 * 2f32.powi(-24)));
    assert_eq!(half_to_f32(0x8000).to_bits(), 0x8000_0000);
    assert_eq!(half_to_f32(0x7C00), f32::INFINITY);
    assert!(half_to_f32(0x7E01).is_nan());
}

#[test]
fn a_literal_keeps_its_width_and_a_float_naga_cannot_spell_keeps_its_bits() {
    assert_eq!(
        literal(Num::Int(16), 0xFFFF_FFFF_FFFF_8001),
        Ok(Literal::U32(0x8001))
    );
    assert_eq!(
        literal(Num::Int(32), (-2i64) as u64),
        Ok(Literal::U32(0xFFFF_FFFE))
    );
    assert_eq!(literal(Num::Bool, u64::MAX), Ok(Literal::Bool(true)));
    assert_eq!(
        literal(Num::Float(32), u64::from(1.5f32.to_bits())),
        Ok(Literal::F32(1.5))
    );
    assert_eq!(literal(Num::Float(32), 0x7FC0_0000), Err(0x7FC0_0000));
    assert_eq!(
        literal(Num::Float(16), 0xFC00),
        Err(f32::NEG_INFINITY.to_bits())
    );
}
