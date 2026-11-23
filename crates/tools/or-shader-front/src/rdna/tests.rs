//! Every input is assembled here from the encodings' field layouts; none comes from a game file.

use naga::valid::{Capabilities, ValidationFlags, Validator};
use or_io::gta5::shaders::xdat::{XdatProgram, XdatStage};

use super::*;

const S_ENDPGM: u32 = 0xBF81_0000;
const ONE: u32 = 0x3F80_0000;
/// Operand codes: VGPR `n` is 256 + n, and 255 says a literal word follows.
const V: u32 = 256;
const LITERAL: u32 = 255;
/// The inline constant 1.0.
const F_ONE: u32 = 242;

fn sopp(op: u32, imm: u16) -> u32 {
    0xBF80_0000 | op << 16 | u32::from(imm)
}

fn sop1(op: u32, dst: u32, src: u32) -> u32 {
    0xBE80_0000 | dst << 16 | op << 8 | src
}

fn sop2(op: u32, dst: u32, src0: u32, src1: u32) -> u32 {
    0x8000_0000 | op << 23 | dst << 16 | src1 << 8 | src0
}

fn sopk(op: u32, dst: u32, imm: u16) -> u32 {
    0xB000_0000 | op << 23 | dst << 16 | u32::from(imm)
}

fn vop1(op: u32, dst: u32, src: u32) -> u32 {
    0x7E00_0000 | dst << 17 | op << 9 | src
}

fn vop2(op: u32, dst: u32, src0: u32, src1: u32) -> u32 {
    op << 25 | dst << 17 | src1 << 9 | src0
}

fn vop3(op: u32, dst: u32, srcs: [u32; 3], abs: u32, neg: u32) -> [u32; 2] {
    [
        0xD400_0000 | op << 16 | abs << 8 | dst,
        srcs[0] | srcs[1] << 9 | srcs[2] << 18 | neg << 29,
    ]
}

fn vintrp(op: u32, dst: u32, attr: u32, chan: u32, src: u32) -> u32 {
    0xC800_0000 | dst << 18 | op << 16 | attr << 10 | chan << 8 | src
}

/// An export of four VGPRs to `target`, marked as the last.
fn exp(target: u32, srcs: [u8; 4]) -> [u32; 2] {
    [
        0xF800_0000 | 1 << 11 | target << 4 | 0xF,
        u32::from_le_bytes(srcs),
    ]
}

const V_MOV: u32 = 0x01;
const V_ADD_F32: u32 = 0x03;
const V_MUL_F32: u32 = 0x08;
const V_MAD_F32: u32 = 0x141;
const S_BRANCH: u32 = 2;
const S_CBRANCH_SCC1: u32 = 5;
const POS0: u32 = 12;

fn text(words: &[u32]) -> Vec<String> {
    (decode(words).unwrap().iter())
        .map(|(_, inst)| inst.to_string())
        .collect()
}

fn validate(module: &naga::Module) {
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(module)
        .unwrap();
}

#[test]
fn published_encodings_disassemble_to_their_assembly() {
    assert_eq!(
        text(&[0x7E00_0301, 0x0600_0501, 0xBE80_0301, 0xBE84_2104, S_ENDPGM]),
        [
            "v_mov_b32 v0, v1",
            "v_add_f32 v0, v1, v2",
            "s_mov_b32 s0, s1",
            "s_swappc_b64 s4, s4",
            "s_endpgm",
        ]
    );
}

#[test]
fn each_family_is_told_by_its_leading_bits() {
    let family = |word: u32| {
        let (inst, _) = decode_one(&[word, 0, 0, 0], 0).unwrap();
        inst.op().map(|op| op.family)
    };
    for (word, expected) in [
        (vop2(V_ADD_F32, 0, V, 0), Family::Vop2),
        (vop1(V_MOV, 0, V), Family::Vop1),
        (0x7C00_0000, Family::Vopc),
        (sop2(0, 0, 1, 2), Family::Sop2),
        (sopk(0, 0, 7), Family::Sopk),
        (sop1(3, 0, 1), Family::Sop1),
        (0xBF00_0000, Family::Sopc),
        (S_ENDPGM, Family::Sopp),
        (vintrp(0, 0, 0, 0, 0), Family::Vintrp),
        (0xCC00_0000, Family::Vop3p),
        (0xD400_0000, Family::Vop3),
        (0xD800_0000, Family::Ds),
        (0xDC00_0000, Family::Flat),
        (0xDC00_8000, Family::Global),
        (0xE000_0000, Family::Mubuf),
        (0xE800_0000, Family::Mtbuf),
        (0xF000_0000, Family::Mimg),
        (0xF400_0000, Family::Smem),
        (0xF800_0000, Family::Exp),
    ] {
        assert_eq!(family(word), Some(expected), "{word:#010x}");
    }
    assert_eq!(family(0xC000_0000), None);
}

#[test]
fn an_instruction_is_as_long_as_its_operands_make_it() {
    let len = |words: &[u32]| decode_one(words, 0).unwrap().1;
    assert_eq!(len(&[vop1(V_MOV, 0, V + 1)]), 1);
    assert_eq!(len(&[vop1(V_MOV, 0, LITERAL), ONE]), 2);
    assert_eq!(len(&[sop1(3, 0, LITERAL), 7]), 2);
    // `s_setreg_imm32_b32`, and `v_madak_f32`'s constant.
    assert_eq!(len(&[sopk(0x15, 0, 0), 7]), 2);
    assert_eq!(len(&[vop2(0x21, 0, V, 1), ONE]), 2);
    let mad = vop3(V_MAD_F32, 0, [V, V + 1, V + 2], 0, 0);
    assert_eq!(len(&mad), 2);
    let mad = vop3(V_MAD_F32, 0, [V, LITERAL, V + 2], 0, 0);
    assert_eq!(len(&[mad[0], mad[1], ONE]), 3);
    // An image instruction with two extra address words.
    assert_eq!(len(&[0xF000_0000 | 0x20 << 18 | 2 << 1, 0, 0, 0]), 4);

    let (inst, _) = decode_one(&[vop1(V_MOV, 3, LITERAL), ONE], 0).unwrap();
    assert_eq!(inst.to_string(), "v_mov_b32 v3, 0x3f800000");
}

#[test]
fn modifiers_and_exports_are_spelled_out() {
    let mad = vop3(V_MAD_F32, 4, [V + 1, V + 2, F_ONE], 0b010, 0b001);
    let export = exp(POS0, [0, 1, 2, 3]);
    assert_eq!(
        text(&[mad[0], mad[1], export[0], export[1]]),
        [
            "v_mad_f32 v4, -v1, |v2|, 1.0",
            "exp pos0, v0, v1, v2, v3 done"
        ]
    );
}

#[test]
fn an_instruction_cut_short_or_of_no_family_is_said() {
    assert_eq!(
        decode_one(&[vop1(V_MOV, 0, LITERAL)], 0),
        Err(DecodeError::Truncated {
            pc: 0,
            family: Family::Vop1,
            need: 2
        })
    );
    assert_eq!(
        decode_one(&[], 0),
        Err(DecodeError::OutOfRange { pc: 0, len: 0 })
    );
    assert_eq!(text(&[0xC000_0000]), [".long 0xc0000000"]);
    // A number its family does not define keeps its family and number.
    assert_eq!(text(&[sopp(0x7F, 0)]), ["sopp_0x7f"]);
}

#[test]
fn a_conditional_branch_cuts_three_blocks() {
    // 0: s_cbranch_scc1 +1 (to word 2)   1: v_mov v0, v1   2: s_endpgm
    let words = [sopp(S_CBRANCH_SCC1, 1), vop1(V_MOV, 0, V + 1), S_ENDPGM];
    let cfg = Cfg::build(&words).unwrap();
    assert_eq!(cfg.blocks.keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(
        cfg.blocks[&0].terminator,
        Terminator::Branch {
            taken: 2,
            fallthrough: 1
        }
    );
    assert_eq!(cfg.blocks[&1].terminator, Terminator::Fall(2));
    assert_eq!(cfg.blocks[&2].terminator, Terminator::End);
    assert_eq!(cfg.successors(0), [2, 1]);
    assert_eq!(cfg.successors(2), Vec::<u32>::new());
}

#[test]
fn a_jump_steps_over_data_and_a_loop_closes() {
    // 0: s_branch +1 (to 2)   1: a word of no family   2: s_branch -1 (to 2)
    let words = [sopp(S_BRANCH, 1), 0xC000_0000, sopp(S_BRANCH, 0xFFFF)];
    let cfg = Cfg::build(&words).unwrap();
    assert_eq!(cfg.blocks.keys().copied().collect::<Vec<_>>(), [0, 2]);
    assert_eq!(cfg.blocks[&2].terminator, Terminator::Jump(2));
}

#[test]
fn a_graph_that_leaves_the_code_is_refused() {
    assert_eq!(
        Cfg::build(&[sopp(S_BRANCH, 5), S_ENDPGM]),
        Err(CfgError::Target {
            pc: 0,
            target: 6,
            len: 2
        })
    );
    assert_eq!(
        Cfg::build(&[sopp(S_BRANCH, 0xFFFE)]),
        Err(CfgError::Target {
            pc: 0,
            target: -1,
            len: 1
        })
    );
    assert_eq!(
        Cfg::build(&[vop1(V_MOV, 0, V + 1)]),
        Err(CfgError::RunsOff { pc: 1 })
    );
    assert_eq!(
        Cfg::build(&[0xC000_0000]),
        Err(CfgError::Undecoded {
            pc: 0,
            word: 0xC000_0000
        })
    );
    // The branch lands on the literal of the two-word instruction at word 1.
    assert_eq!(
        Cfg::build(&[
            sopp(S_CBRANCH_SCC1, 1),
            vop1(V_MOV, 0, LITERAL),
            S_ENDPGM,
            S_ENDPGM
        ]),
        Err(CfgError::Overlap { pc: 1, inside: 2 })
    );
}

fn program(kind: ShaderKind, words: Vec<u32>) -> Program {
    Program {
        kind,
        slot: 0,
        words,
        wave32: false,
        // Attribute `n` is the vertex stage's parameter `n + 1`.
        inputs: (1..4).map(|slot| PixelInput { slot, flags: 2 }).collect(),
        vertex_inputs: vec![VertexInput {
            register: 0,
            mask: 0b0111,
            component_type: 3,
        }],
        // A pixel program reads the position's x and y, then the facing.
        position_read: 0b0011,
        front_face: true,
    }
}

fn sopc(op: u32, src0: u32, src1: u32) -> u32 {
    0xBF00_0000 | op << 16 | src1 << 8 | src0
}

fn vopc(op: u32, src0: u32, src1: u32) -> u32 {
    0x7C00_0000 | op << 17 | src1 << 9 | src0
}

/// A scalar memory instruction with an immediate offset and no offset register.
fn smem(op: u32, data: u32, base: u32, offset: u32) -> [u32; 2] {
    [
        0xF400_0000 | op << 18 | data << 6 | (base / 2),
        offset | 0x7D << 25,
    ]
}

fn swizzle(dst: u32, src: u32, pattern: u32) -> [u32; 2] {
    [0xD800_0000 | DS_SWIZZLE << 18 | pattern, src | dst << 24]
}

/// `image_sample` of a two-dimensional image, all four components, address and data from `v0`.
fn sample(rsrc: u32, samp: u32) -> [u32; 2] {
    [
        0xF000_0000 | IMAGE_SAMPLE << 18 | 0xF << 8 | 1 << 3,
        (rsrc / 4) << 16 | (samp / 4) << 21,
    ]
}

const S_MOVK: u32 = 0x0;
const S_ADD_U32: u32 = 0x0;
const S_ADD_I32: u32 = 0x2;
const S_CMP_EQ_U32: u32 = 0x6;
const S_CMP_LG_U32: u32 = 0x7;
const S_CBRANCH_SCC0: u32 = 4;
const S_SWAPPC: u32 = 0x21;
const S_LOAD_X2: u32 = 0x1;
const S_LOAD_X4: u32 = 0x2;
const S_LOAD_X8: u32 = 0x3;
const S_BUFFER_LOAD: u32 = 0x8;
const V_SUB_F32: u32 = 0x04;
const V_CVT_PKRTZ: u32 = 0x2F;
const V_CMPX_GT_F32: u32 = 0x14;
const DS_SWIZZLE: u32 = 0x35;
const IMAGE_SAMPLE: u32 = 0x20;
const F_HALF: u32 = 240;
/// The inline integers 0, 1 and -1.
const I_ZERO: u32 = 128;
const I_ONE: u32 = 129;
const I_MINUS_ONE: u32 = 193;

fn lowered(kind: ShaderKind, words: Vec<u32>) -> Lowered {
    let lowered = translate(&program(kind, words)).unwrap();
    validate(&lowered.module);
    lowered
}

fn refusal(kind: ShaderKind, words: Vec<u32>) -> RdnaError {
    translate(&program(kind, words)).unwrap_err()
}

/// Whether any statement of `block`, nested ones included, satisfies `wanted`.
fn holds(block: &naga::Block, wanted: &dyn Fn(&naga::Statement) -> bool) -> bool {
    use naga::Statement as S;
    block.iter().any(|statement| {
        wanted(statement)
            || match statement {
                S::Block(inner) => holds(inner, wanted),
                S::If { accept, reject, .. } => holds(accept, wanted) || holds(reject, wanted),
                S::Loop {
                    body, continuing, ..
                } => holds(body, wanted) || holds(continuing, wanted),
                _ => false,
            }
    })
}

/// The function the entry point calls, which holds the program's own statements.
fn body(module: &naga::Module) -> &naga::Function {
    module.functions.iter().next().unwrap().1
}

#[test]
fn a_vertex_program_of_arithmetic_and_exports_is_a_valid_module() {
    let mad = vop3(V_MAD_F32, 2, [V, V + 1, F_ONE], 0, 0b001);
    let position = exp(POS0, [0, 1, 2, 2]);
    let param = exp(32 + 3, [2, 2, 0, 1]);
    let words = vec![
        vop1(V_MOV, 0, LITERAL),
        ONE,
        vop2(V_ADD_F32, 1, F_ONE, 0),
        mad[0],
        mad[1],
        position[0],
        position[1],
        param[0],
        param[1],
        S_ENDPGM,
    ];
    let module = lowered(ShaderKind::Vertex, words).module;
    let entry = &module.entry_points[0];
    assert_eq!(entry.stage, naga::ShaderStage::Vertex);
    let result = entry.function.result.as_ref().unwrap();
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("the result is not a struct");
    };
    let bindings: Vec<_> = members.iter().map(|m| m.binding.clone().unwrap()).collect();
    assert!(matches!(
        bindings.as_slice(),
        [
            naga::Binding::BuiltIn(naga::BuiltIn::Position { .. }),
            naga::Binding::Location { location: 3, .. }
        ]
    ));
}

#[test]
fn a_pixel_program_reads_its_interpolants_at_their_parameter_slots() {
    // v_interp_p2_f32 v0, v1, attr2.y; v_mul_f32 v0, v0, v0; exp mrt0.
    let target = exp(0, [0, 0, 0, 0]);
    let words = vec![
        vintrp(1, 0, 2, 1, 1),
        vop2(V_MUL_F32, 0, V, 0),
        target[0],
        target[1],
        S_ENDPGM,
    ];
    let module = lowered(ShaderKind::Pixel, words).module;
    let entry = &module.entry_points[0];
    assert_eq!(entry.stage, naga::ShaderStage::Fragment);
    // Attribute 2 is the third record of the input table, whose slot is 3.
    assert!(matches!(
        entry.function.arguments[0].binding,
        Some(naga::Binding::Location { location: 3, .. })
    ));
    // An attribute the table does not list is refused, not guessed.
    let words = vec![vintrp(1, 0, 7, 0, 1), target[0], target[1], S_ENDPGM];
    assert!(matches!(
        refusal(ShaderKind::Pixel, words),
        RdnaError::NotTranslated(NotTranslated::Form { pc: 0, .. })
    ));
}

#[test]
fn a_branch_on_the_scalar_condition_becomes_an_if() {
    let position = exp(POS0, [0, 0, 0, 0]);
    let words = vec![
        sopk(S_MOVK, 0, 1),
        sopc(S_CMP_EQ_U32, 0, I_ONE),
        sopp(S_CBRANCH_SCC0, 2),
        vop1(V_MOV, 0, F_ONE),
        sopp(S_BRANCH, 1),
        vop1(V_MOV, 0, F_HALF),
        position[0],
        position[1],
        S_ENDPGM,
    ];
    let module = lowered(ShaderKind::Vertex, words).module;
    let function = body(&module);
    assert!(holds(&function.body, &|s| matches!(
        s,
        naga::Statement::If { .. }
    )));
    assert!(!holds(&function.body, &|s| matches!(
        s,
        naga::Statement::Loop { .. }
    )));
}

#[test]
fn a_backward_branch_becomes_a_loop() {
    let position = exp(POS0, [0, 0, 0, 0]);
    let words = vec![
        sopk(S_MOVK, 0, 3),
        vop1(V_MOV, 0, F_ONE),
        sop2(S_ADD_I32, 0, 0, I_MINUS_ONE),
        sopc(S_CMP_LG_U32, 0, I_ZERO),
        // Back to the addition at word 2.
        sopp(S_CBRANCH_SCC1, 0xFFFD),
        position[0],
        position[1],
        S_ENDPGM,
    ];
    let module = lowered(ShaderKind::Vertex, words).module;
    assert!(holds(&body(&module).body, &|s| matches!(
        s,
        naga::Statement::Loop { .. }
    )));
}

#[test]
fn a_lane_switched_off_is_discarded_and_packed_halves_are_exported() {
    // v0 = attr0.x; the lane stays on where 1.0 > v0; v1 = (v0, v0) as halves; exp mrt0 compr.
    let words = vec![
        vintrp(1, 0, 0, 0, 1),
        vopc(V_CMPX_GT_F32, F_ONE, 0),
        vop2(V_CVT_PKRTZ, 1, V, 0),
        0xF800_0000 | 1 << 11 | 1 << 10 | 0xF,
        u32::from_le_bytes([1, 1, 0, 0]),
        S_ENDPGM,
    ];
    let masked = lowered(ShaderKind::Pixel, words);
    let function = body(&masked.module);
    assert!(holds(&function.body, &|s| matches!(
        s,
        naga::Statement::Kill
    )));
    assert!(masked
        .approximations
        .contains_key("a 16-bit float rounded to nearest"));
    // With no instruction that masks lanes, nothing is discarded.
    let target = exp(0, [0, 0, 0, 0]);
    let plain = vec![vintrp(1, 0, 0, 0, 1), target[0], target[1], S_ENDPGM];
    let plain = lowered(ShaderKind::Pixel, plain).module;
    assert!(!holds(&body(&plain).body, &|s| matches!(
        s,
        naga::Statement::Kill
    )));
}

#[test]
fn tables_lead_to_a_constant_buffer_a_texture_and_a_sampler() {
    // s2 points at a table: a buffer descriptor at 0x10, an image's at 0x40, a sampler's at 0x80.
    let buffer = smem(S_LOAD_X4, 4, 2, 0x10);
    let read = smem(S_BUFFER_LOAD, 20, 4, 0x24);
    let image = smem(S_LOAD_X8, 8, 2, 0x40);
    let sampler = smem(S_LOAD_X4, 16, 2, 0x80);
    let texel = sample(8, 16);
    let target = exp(0, [0, 1, 2, 3]);
    let words = vec![
        buffer[0],
        buffer[1],
        read[0],
        read[1],
        vop1(V_MOV, 0, 20),
        vop1(V_MOV, 1, 20),
        image[0],
        image[1],
        sampler[0],
        sampler[1],
        texel[0],
        texel[1],
        target[0],
        target[1],
        S_ENDPGM,
    ];
    let out = lowered(ShaderKind::Pixel, words);
    let at = |offset| {
        Some(Loc {
            root: 2,
            path: Vec::new(),
            offset,
            index: None,
        })
    };
    assert_eq!(
        out.resources,
        [
            Resource {
                // The read at byte 0x24 is the tenth dword, in the third row of four.
                kind: ResourceKind::ConstantBuffer { rows: 3 },
                at: at(0x10),
                words: None,
                group: 0,
                binding: 0
            },
            Resource {
                kind: ResourceKind::Texture {
                    dim: 1,
                    compared: false
                },
                at: at(0x40),
                words: None,
                group: 0,
                binding: 1
            },
            Resource {
                kind: ResourceKind::Sampler { comparison: false },
                at: at(0x80),
                words: None,
                group: 0,
                binding: 2
            },
        ]
    );
}

#[test]
fn a_table_reached_through_another_keeps_its_path() {
    // s[4:5] = the pointer at s2's table + 8; s[8:11] = that table's words from 0x20.
    let pointer = smem(S_LOAD_X2, 4, 2, 8);
    let words = smem(S_LOAD_X4, 8, 4, 0x20);
    let code = [pointer[0], pointer[1], words[0], words[1], S_ENDPGM];
    let cfg = Cfg::build(&code).unwrap();
    let mut state = sym::entry();
    for (_, inst) in &cfg.blocks[&0].insts {
        sym::step(&mut state, inst);
    }
    assert_eq!(
        sym::descriptor(&state, 8, 4),
        Some(Loc {
            root: 2,
            path: vec![8],
            offset: 0x20,
            index: None,
        })
    );
    // Three words of it are no four-word descriptor from the second.
    assert_eq!(sym::descriptor(&state, 9, 4), None);
}

#[test]
fn two_lanes_of_a_quad_subtracted_are_a_derivative() {
    // v1 = v0 of the quad's second lane, v2 = v0 of its first, v3 = v1 - v2: the coarse x derivative.
    let (right, left) = (swizzle(1, 0, 0x8055), swizzle(2, 0, 0x8000));
    let target = exp(0, [3, 3, 3, 3]);
    let words = vec![
        vintrp(1, 0, 0, 0, 1),
        right[0],
        right[1],
        left[0],
        left[1],
        vop2(V_SUB_F32, 3, V + 1, 2),
        target[0],
        target[1],
        S_ENDPGM,
    ];
    let module = lowered(ShaderKind::Pixel, words).module;
    assert!(body(&module).expressions.iter().any(|(_, e)| matches!(
        e,
        naga::Expression::Derivative {
            axis: naga::DerivativeAxis::X,
            ctrl: naga::DerivativeControl::Coarse,
            ..
        }
    )));
    // A neighbouring lane's value read any other way is refused.
    let target = exp(0, [1, 1, 1, 1]);
    let words = vec![
        vintrp(1, 0, 0, 0, 1),
        right[0],
        right[1],
        target[0],
        target[1],
        S_ENDPGM,
    ];
    assert!(matches!(
        refusal(ShaderKind::Pixel, words),
        RdnaError::NotTranslated(NotTranslated::Form { pc: 3, .. })
    ));
}

#[test]
fn the_fetch_call_becomes_vertex_inputs() {
    // The signature's one element, register 0, arrives in v4..v7.
    let position = exp(POS0, [4, 5, 6, 7]);
    let words = vec![sop1(S_SWAPPC, 4, 4), position[0], position[1], S_ENDPGM];
    let module = lowered(ShaderKind::Vertex, words).module;
    assert!(matches!(
        module.entry_points[0].function.arguments[0].binding,
        Some(naga::Binding::Location { location: 0, .. })
    ));
}

#[test]
fn what_is_not_translated_is_named() {
    // A table's word is no number to add.
    let table = smem(S_LOAD_X4, 4, 2, 0);
    let add = sop2(S_ADD_U32, 0, 4, 4);
    assert_eq!(
        refusal(ShaderKind::Vertex, vec![table[0], table[1], add, S_ENDPGM]),
        RdnaError::NotTranslated(NotTranslated::Descriptor {
            pc: 2,
            what: "a table's word as a number"
        })
    );
    // A trace word, which no lowering reads.
    let refused = refusal(ShaderKind::Vertex, vec![sopp(0x28, 0), S_ENDPGM]);
    assert_eq!(
        refused,
        RdnaError::NotTranslated(NotTranslated::Opcode {
            pc: 0,
            name: "s_ttracedata_imm".to_owned()
        })
    );
    let RdnaError::NotTranslated(refused) = refused else {
        panic!("not a refusal of the lowering");
    };
    assert_eq!(refused.name(), "s_ttracedata_imm");
    // A buffer read through registers no table filled.
    let load = mubuf(op(opcodes::MUBUF, "buffer_load_dword"), 0, 0, 0, false);
    assert_eq!(
        refusal(ShaderKind::Vertex, vec![load[0], load[1], S_ENDPGM]),
        RdnaError::NotTranslated(NotTranslated::Descriptor {
            pc: 0,
            what: "a buffer whose descriptor it did not load from a table"
        })
    );
    let position = exp(POS0, [0, 0, 0, 0]);
    assert_eq!(
        refusal(
            ShaderKind::Vertex,
            vec![vop1(V_MOV, 0, V + 5), position[0], position[1], S_ENDPGM]
        ),
        RdnaError::NotTranslated(NotTranslated::LiveIn {
            pc: 0,
            register: "v5".to_owned()
        })
    );
    assert_eq!(
        refusal(ShaderKind::Compute, vec![S_ENDPGM]),
        RdnaError::NotTranslated(NotTranslated::Stage("compute".to_owned()))
    );
    assert_eq!(
        refusal(ShaderKind::Vertex, vec![S_ENDPGM]),
        RdnaError::NotTranslated(NotTranslated::NoPosition)
    );
}

/// The number of a mnemonic in its family's table.
fn op(table: &[(u16, &str)], name: &str) -> u32 {
    u32::from(table.iter().find(|row| row.1 == name).unwrap().0)
}

/// The number of a mnemonic of the three-source family.
fn op3(name: &str) -> u32 {
    u32::from(opcodes::VOP3.iter().find(|row| row.1 == name).unwrap().0)
}

/// A scalar memory instruction whose offset register is `soffset`.
fn smem_at(op: u32, data: u32, base: u32, offset: u32, soffset: u32) -> [u32; 2] {
    [
        0xF400_0000 | op << 18 | data << 6 | (base / 2),
        offset | soffset << 25,
    ]
}

/// A buffer instruction with no immediate offset and no scalar one: the address register is an
/// index where `idxen`, and reads nothing otherwise.
fn mubuf(op: u32, data: u32, addr: u32, rsrc: u32, idxen: bool) -> [u32; 2] {
    [
        0xE000_0000 | (op & 0x7F) << 18 | (op >> 7) << 25 | u32::from(idxen) << 13,
        addr | data << 8 | (rsrc / 4) << 16 | I_ZERO << 24,
    ]
}

fn plain(root: u8, offset: u32) -> Loc {
    Loc {
        root,
        path: Vec::new(),
        offset,
        index: None,
    }
}

#[test]
fn a_graph_that_does_not_nest_runs_in_a_dispatch_loop() {
    // The entry branches into the middle of a loop that the block after it enters at its head.
    let colour = exp(0, [0, 0, 0, 0]);
    let words = vec![
        vop1(V_MOV, 0, F_ONE),
        sopc(S_CMP_EQ_U32, I_ZERO, I_ZERO),
        sopp(S_CBRANCH_SCC1, 1),
        vop1(V_MOV, 0, F_ONE),
        sopc(S_CMP_LG_U32, I_ZERO, I_ZERO),
        sopp(S_CBRANCH_SCC1, 0xFFFD),
        colour[0],
        colour[1],
        S_ENDPGM,
    ];
    let out = lowered(ShaderKind::Pixel, words);
    assert!(out
        .approximations
        .contains_key("a dispatch loop for a graph that does not nest"));
    assert!(holds(&body(&out.module).body, &|s| matches!(
        s,
        naga::Statement::Switch { .. }
    )));
}

#[test]
fn a_table_read_at_an_index_names_where_the_index_comes_from() {
    // s14 = s9 << 4; s[16:19] = the table at s10, at that index.
    let load = smem_at(S_LOAD_X4, 16, 10, 0, 14);
    let shift = sop2(op(opcodes::SOP2, "s_lshl_b32"), 14, 9, I_ZERO + 4);
    let code = [shift, load[0], load[1], S_ENDPGM];
    let cfg = Cfg::build(&code).unwrap();
    let mut state = sym::entry();
    for (_, inst) in &cfg.blocks[&0].insts {
        sym::step(&mut state, inst);
    }
    assert_eq!(
        sym::descriptor(&state, 16, 4),
        Some(Loc {
            index: Some(Index {
                source: IndexSource::User(9),
                shift: 4
            }),
            ..plain(10, 0)
        })
    );

    // s20 = a constant buffer's dword at 0x24, shifted by 5: the index of an image in the table at s12.
    let buffer = smem(S_LOAD_X4, 4, 2, 0x10);
    let read = smem(S_BUFFER_LOAD, 20, 4, 0x24);
    let shift = sop2(op(opcodes::SOP2, "s_lshl_b32"), 20, 20, I_ZERO + 5);
    let image = smem_at(S_LOAD_X8, 8, 12, 0, 20);
    let code = [
        buffer[0], buffer[1], read[0], read[1], shift, image[0], image[1], S_ENDPGM,
    ];
    let cfg = Cfg::build(&code).unwrap();
    let mut state = sym::entry();
    for (_, inst) in &cfg.blocks[&0].insts {
        sym::step(&mut state, inst);
    }
    assert_eq!(
        sym::descriptor(&state, 8, 8),
        Some(Loc {
            index: Some(Index {
                source: IndexSource::Data {
                    buffer: Box::new(plain(2, 0x10)),
                    offset: 0x24,
                    plus: None
                },
                shift: 5
            }),
            ..plain(12, 0)
        })
    );
    // An index the program computed any other way names nothing.
    let add = sop2(S_ADD_U32, 14, 9, 9);
    let code = [add, load[0], load[1], S_ENDPGM];
    assert_eq!(
        refusal(ShaderKind::Vertex, code.to_vec()),
        RdnaError::NotTranslated(NotTranslated::Descriptor {
            pc: 1,
            what: "a table through a pointer or at an offset the program computed"
        })
    );
}

#[test]
fn a_sampler_the_code_writes_is_bound_by_its_words() {
    let image = smem(S_LOAD_X8, 8, 2, 0x40);
    let texel = sample(8, 16);
    let target = exp(0, [0, 1, 2, 3]);
    let words = vec![
        image[0],
        image[1],
        sopk(S_MOVK, 16, 0x92),
        sopk(S_MOVK, 17, 0),
        sopk(S_MOVK, 18, 0),
        sopk(S_MOVK, 19, 0),
        vop1(V_MOV, 0, F_ONE),
        vop1(V_MOV, 1, F_ONE),
        texel[0],
        texel[1],
        target[0],
        target[1],
        S_ENDPGM,
    ];
    let out = lowered(ShaderKind::Pixel, words);
    assert_eq!(
        out.resources[1],
        Resource {
            kind: ResourceKind::Sampler { comparison: false },
            at: None,
            words: Some([0x92, 0, 0, 0]),
            group: 0,
            binding: 1
        }
    );
}

#[test]
fn a_pixel_program_finds_the_position_and_the_facing_after_the_barycentrics() {
    // The helper's signature reads x and y, then the facing: v2, v3 and v4.
    let colour = exp(0, [2, 3, 4, 4]);
    let module = lowered(ShaderKind::Pixel, vec![colour[0], colour[1], S_ENDPGM]).module;
    let arguments = &module.entry_points[0].function.arguments;
    let built_in = |wanted: naga::BuiltIn| {
        (arguments.iter()).any(|a| a.binding == Some(naga::Binding::BuiltIn(wanted)))
    };
    assert!(built_in(naga::BuiltIn::Position { invariant: false }));
    assert!(built_in(naga::BuiltIn::FrontFacing));
    // The register after them holds nothing the signature names.
    let colour = exp(0, [5, 5, 5, 5]);
    assert_eq!(
        refusal(ShaderKind::Pixel, vec![colour[0], colour[1], S_ENDPGM]),
        RdnaError::NotTranslated(NotTranslated::LiveIn {
            pc: 0,
            register: "v5".to_owned()
        })
    );
}

#[test]
fn a_buffer_is_read_by_address_with_its_stride_from_its_header_or_from_the_code() {
    // A table's buffer, four dwords at the vertex index times the stride its header states.
    let descriptor = smem(S_LOAD_X4, 4, 2, 0x20);
    let load = mubuf(op(opcodes::MUBUF, "buffer_load_dwordx4"), 4, 0, 4, true);
    let store = mubuf(op(opcodes::MUBUF, "buffer_store_dword"), 4, 0, 4, true);
    let position = exp(POS0, [4, 5, 6, 7]);
    let words = vec![
        descriptor[0],
        descriptor[1],
        load[0],
        load[1],
        store[0],
        store[1],
        position[0],
        position[1],
        S_ENDPGM,
    ];
    let out = lowered(ShaderKind::Vertex, words);
    assert_eq!(
        out.resources,
        [Resource {
            kind: ResourceKind::Buffer { stride: None },
            at: Some(plain(2, 0x20)),
            words: None,
            group: 0,
            binding: 0
        }]
    );
    assert!(out
        .approximations
        .contains_key("a buffer store before the pixel stage, dropped"));

    // A descriptor the code builds around the address in s9 and s10: a stride of one byte, 32-bit unsigned elements.
    let s_mov = op(opcodes::SOP1, "s_mov_b32");
    let typed = mubuf(op(opcodes::MUBUF, "buffer_load_format_x"), 4, 0, 0, true);
    let position = exp(POS0, [4, 4, 4, 4]);
    let words = vec![
        sop1(s_mov, 0, 9),
        sop2(op(opcodes::SOP2, "s_or_b32"), 1, 10, LITERAL),
        0x1_0000,
        sop1(s_mov, 2, I_MINUS_ONE),
        sop1(s_mov, 3, LITERAL),
        20 << 12 | 0xFAC,
        typed[0],
        typed[1],
        position[0],
        position[1],
        S_ENDPGM,
    ];
    let out = lowered(ShaderKind::Vertex, words);
    assert!(out.resources.contains(&Resource {
        kind: ResourceKind::Buffer { stride: Some(1) },
        at: Some(plain(9, 0)),
        words: None,
        group: 0,
        binding: 0
    }));
}

#[test]
fn a_parameter_read_as_halves_crosses_the_stages_as_two_vectors() {
    // The pixel program interpolates the high half of attribute 0's x, which is parameter 1.
    let read = vop3(op3("v_interp_p2_f16"), 4, [V, V + 1, V], 0, 0);
    let colour = exp(0, [4, 4, 4, 4]);
    let out = lowered(
        ShaderKind::Pixel,
        vec![read[0], read[1], colour[0], colour[1], S_ENDPGM],
    );
    assert_eq!(out.half_slots, std::collections::BTreeSet::from([1]));
    assert!(matches!(
        out.module.entry_points[0].function.arguments[0].binding,
        Some(naga::Binding::Location { location: 33, .. })
    ));

    // Told so, the vertex program exports parameter 1 as its low halves and its high halves.
    let (position, parameter) = (exp(POS0, [0, 0, 0, 0]), exp(33, [0, 0, 0, 0]));
    let words = vec![
        vop1(V_MOV, 0, F_ONE),
        position[0],
        position[1],
        parameter[0],
        parameter[1],
        S_ENDPGM,
    ];
    let options = Options {
        half_slots: out.half_slots,
    };
    let module = translate_with(&program(ShaderKind::Vertex, words), &options)
        .unwrap()
        .module;
    validate(&module);
    let result = module.entry_points[0].function.result.as_ref().unwrap();
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("the vertex stage returns no structure");
    };
    let locations: Vec<u32> = (members.iter())
        .filter_map(|member| match member.binding {
            Some(naga::Binding::Location { location, .. }) => Some(location),
            _ => None,
        })
        .collect();
    assert_eq!(locations, [1, 33]);
}

#[test]
fn the_high_dword_of_a_product_is_a_valid_module() {
    let product = vop3(op3("v_mul_hi_u32"), 1, [V, V, 0], 0, 0);
    let position = exp(POS0, [1, 1, 1, 1]);
    lowered(
        ShaderKind::Vertex,
        vec![product[0], product[1], position[0], position[1], S_ENDPGM],
    );
}

#[test]
fn a_mesh_program_runs_as_a_vertex_program_that_calls_its_body_twice() {
    // Each thread places a vertex at (1, 1, 1, 1) and a primitive made of its own number.
    let lane = vop3(op3("v_mbcnt_lo_u32_b32"), 0, [I_MINUS_ONE, I_ZERO, 0], 0, 0);
    let (position, primitive) = (exp(POS0, [1, 1, 1, 1]), exp(20, [0, 0, 0, 0]));
    let words = vec![
        lane[0],
        lane[1],
        vop1(V_MOV, 1, F_ONE),
        sopp(op(opcodes::SOPP, "s_sendmsg"), 9),
        sopp(op(opcodes::SOPP, "s_barrier"), 0),
        position[0],
        position[1],
        primitive[0],
        primitive[1],
        S_ENDPGM,
    ];
    let module = lowered(ShaderKind::Mesh, words.clone()).module;
    let entry = &module.entry_points[0];
    assert_eq!(entry.stage, naga::ShaderStage::Vertex);
    for wanted in [naga::BuiltIn::VertexIndex, naga::BuiltIn::InstanceIndex] {
        assert!((entry.function.arguments.iter())
            .any(|a| a.binding == Some(naga::Binding::BuiltIn(wanted))));
    }
    let calls = (entry.function.body.iter())
        .filter(|s| matches!(s, naga::Statement::Call { .. }))
        .count();
    assert_eq!(calls, 2);
    // No other stage knows which lane of its wave it is.
    assert!(matches!(
        refusal(ShaderKind::Vertex, words),
        RdnaError::NotTranslated(NotTranslated::Form { pc: 0, .. })
    ));
}

#[test]
fn a_part_of_a_register_is_read_where_a_word_selects_it() {
    // v1 = the high half of v0 as a float: the word names v0, its second 16 bits, and the whole destination.
    let position = exp(POS0, [1, 1, 1, 1]);
    let words = vec![
        vop1(op(opcodes::VOP1, "v_cvt_f32_u32"), 1, 249),
        5 << 16 | 6 << 8,
        position[0],
        position[1],
        S_ENDPGM,
    ];
    lowered(ShaderKind::Vertex, words);
    // A selector past the whole register names nothing.
    let words = vec![
        vop1(op(opcodes::VOP1, "v_cvt_f32_u32"), 1, 249),
        7 << 16 | 6 << 8,
        position[0],
        position[1],
        S_ENDPGM,
    ];
    assert!(matches!(
        refusal(ShaderKind::Vertex, words),
        RdnaError::NotTranslated(NotTranslated::Form { pc: 0, .. })
    ));
}

#[test]
fn the_corrections_the_published_encodings_made_hold() {
    let len = |words: &[u32]| decode_one(words, 0).unwrap().1;
    // `v_fmaak_f32` carries its constant in a second word.
    assert_eq!(len(&[vop2(0x2D, 0, V, 1), ONE]), 2);
    // The `sdwa` code means nothing in a scalar field and takes no word.
    assert_eq!(len(&[sop1(3, 0, 249)]), 1);
    assert_eq!(text(&[sopp(0x28, 0)]), ["s_ttracedata_imm"]);
    // A buffer opcode's eighth bit is bit 25 of the first word.
    let (inst, _) = decode_one(&[0xE000_0000 | 1 << 25, 0], 0).unwrap();
    assert_eq!(inst.op().map(|op| op.number), Some(0x80));
    // A global offset is signed: twelve set bits are -1.
    let (inst, _) = decode_one(&[0xDC00_8FFF, 0], 0).unwrap();
    assert!(matches!(inst, Inst::Flat { offset: -1, .. }));
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// One block whose used slot is `slot` and whose code range is six descriptor words, `words`, then filler to whole 64-byte lines.
fn stage(slot: usize, stage_word: u32, words: &[u32]) -> XdatStage {
    const EMPTY: u64 = 0xDEAD_BEEF_DEAD_BEEF;
    const CODE_OFFSET: usize = 0x300;
    let mut code = vec![0xD580_0000, 0x03FE_0000, 0, 0xD580_0000, 0x03FF_0000, 0];
    code.extend_from_slice(words);
    code.push(S_CODE_END);
    while !code.len().is_multiple_of(16) {
        code.push(S_CODE_END);
    }
    let start = 0x10 + CODE_OFFSET;
    let mut m = vec![0u8; start + 4 * code.len() + 16];
    let len = m.len();
    m[..4].copy_from_slice(b"FSSM");
    put32(&mut m, 4, 0x101);
    put32(&mut m, 8, (len - 16) as u32);
    put32(&mut m, 0x10, 6);
    for s in 0..12 {
        put64(&mut m, 0x18 + 8 * s, if s == slot { 0xB0 } else { EMPTY });
    }
    put32(&mut m, 0x78, 0x3_0002);
    put64(&mut m, 0xC0, CODE_OFFSET as u64);
    put64(&mut m, 0xC8, 4 * code.len() as u64);
    put32(&mut m, 0xD8, 0xC);
    put64(&mut m, 0xE8, (CODE_OFFSET + 4 * code.len()) as u64);
    put32(&mut m, 0x100, stage_word);
    for (i, w) in code.iter().enumerate() {
        put32(&mut m, start + 4 * i, *w);
    }
    XdatStage {
        slot,
        stage_word,
        package: m,
        code: start..start + 4 * code.len(),
    }
}

/// A DXBC container whose one part is `dxil`.
fn container(dxil: &[u8]) -> Vec<u8> {
    let size = 36 + 8 + dxil.len();
    let mut out = b"DXBC".to_vec();
    out.extend_from_slice(&[0; 16]);
    for word in [1, size as u32, 1, 36] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(b"DXIL");
    out.extend_from_slice(&(dxil.len() as u32).to_le_bytes());
    out.extend_from_slice(dxil);
    out
}

#[test]
fn a_container_unwraps_to_every_block_of_its_directory() {
    let first = [vop1(V_MOV, 0, V + 1), S_ENDPGM];
    let second = [vop1(V_MOV, 2, V + 3), vop1(V_MOV, 4, V + 5), S_ENDPGM];
    let xdat = XdatProgram {
        program_version: 0x0001_0066,
        digest: [7; 16],
        stages: vec![stage(2, 0x2_0002, &first), stage(4, 0x2_0004, &second)],
        stub_words: 0x1A5,
    };
    let bytes = container(&xdat.write().unwrap());
    let programs = unwrap(&bytes).unwrap();
    assert_eq!(programs.len(), 2);
    assert_eq!(
        (programs[0].kind, programs[0].slot, &programs[0].words[..]),
        (ShaderKind::Vertex, 2, &first[..])
    );
    assert_eq!((programs[1].slot, &programs[1].words[..]), (4, &second[..]));
}

#[test]
fn a_container_that_is_not_one_is_refused() {
    assert_eq!(unwrap(b"FSSM"), Err(ContainerError::NotDxbc));
    assert_eq!(
        unwrap(&container(&[])[..36]),
        Err(ContainerError::Truncated {
            what: "the size the header states",
            at: 36
        })
    );
    let xdat = XdatProgram {
        program_version: 0x66,
        digest: [0; 16],
        stages: vec![stage(5, 0x2_0105, &[S_ENDPGM])],
        stub_words: 0,
    };
    let mut bytes = container(&xdat.write().unwrap());
    // The second directory entry's tag, 0x0D, becomes one no rule allows.
    let tag = 36 + 8 + 24 + 12 + 12;
    assert_eq!(bytes[tag], 0x0D);
    bytes[tag] = 0x0E;
    assert!(matches!(unwrap(&bytes), Err(ContainerError::Xdat(_))));
}
