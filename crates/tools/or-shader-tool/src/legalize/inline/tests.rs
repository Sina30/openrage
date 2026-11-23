use super::*;
use crate::backend_check::{check_module, Backend};
use crate::legalize::{instructions, legalize};
use crate::spirv_testing::{dwords, naga_validates, op, string};

fn insts_of(bytes: &[u8]) -> Vec<Vec<u32>> {
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    instructions(&words)
        .unwrap()
        .into_iter()
        .map(<[u32]>::to_vec)
        .collect()
}

/// A fragment shader that adds a helper's result to a total in a loop, so the result is read in the loop's continue block: `twice` is the helper, which doubles with a multiply, or returns its parameter as it is.
fn looping(helper_multiplies: bool) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 40, 0];
    words.extend(op(17, &[1]));
    let mut import = vec![31];
    import.extend(string("GLSL.std.450"));
    words.extend(op(11, &import));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(string("main"));
    entry_words.push(9);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    words.extend(op(71, &[9, 30, 0]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[4, 32]));
    words.extend(op(21, &[5, 32, 0]));
    words.extend(op(20, &[6]));
    words.extend(op(33, &[7, 4, 4]));
    words.extend(op(32, &[8, 3, 4]));
    words.extend(op(59, &[8, 9, 3]));
    words.extend(op(43, &[5, 10, 0]));
    words.extend(op(43, &[5, 11, 1]));
    words.extend(op(43, &[5, 12, 4]));
    words.extend(op(43, &[4, 13, 0x3f80_0000]));
    words.extend(op(43, &[4, 14, 0x4000_0000]));
    // The helper
    words.extend(op(54, &[4, 15, 0, 7]));
    words.extend(op(55, &[4, 16]));
    words.extend(op(248, &[17]));
    if helper_multiplies {
        words.extend(op(133, &[4, 18, 16, 14]));
        words.extend(op(254, &[18]));
    } else {
        words.extend(op(254, &[16]));
    }
    words.extend(op(56, &[]));
    // The shader: a loop of four turns
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[20]));
    words.extend(op(249, &[21]));
    words.extend(op(248, &[21]));
    words.extend(op(245, &[5, 22, 10, 20, 29, 26]));
    words.extend(op(245, &[4, 23, 13, 20, 28, 26]));
    words.extend(op(246, &[27, 26, 0]));
    words.extend(op(249, &[31]));
    // The turn that ends the loop leaves from the top of the body, as a translator writes a loop
    words.extend(op(248, &[31]));
    words.extend(op(174, &[6, 24, 22, 12]));
    words.extend(op(247, &[33, 0]));
    words.extend(op(250, &[24, 32, 33]));
    words.extend(op(248, &[32]));
    words.extend(op(249, &[27]));
    words.extend(op(248, &[33]));
    words.extend(op(57, &[4, 30, 15, 23]));
    // The new total is made in the body, so the continue block that feeds the phi reads it from there
    words.extend(op(129, &[4, 28, 23, 30]));
    words.extend(op(128, &[5, 29, 22, 11]));
    words.extend(op(249, &[26]));
    words.extend(op(248, &[26]));
    words.extend(op(249, &[21]));
    words.extend(op(248, &[27]));
    words.extend(op(62, &[9, 23]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
#[cfg_attr(
    all(windows, debug_assertions),
    ignore = "Cranelift cannot unwind a panic on Windows: run it with `cargo test --release`"
)]
fn naga_writes_a_call_in_a_loop_for_metal_only_once_the_call_is_inlined() {
    for multiplies in [true, false] {
        let raw = looping(multiplies);
        naga_validates(&raw).unwrap_or_else(|e| panic!("{multiplies}: {e}"));
        let before = check_module(&raw);
        assert!(
            before[&Backend::Msl]
                .as_ref()
                .err()
                .is_some_and(|c| c.contains("panicked")),
            "{multiplies}: the Metal writer takes it as it is: {:?}",
            before[&Backend::Msl]
        );
        let (legal, lowerings) = legalize(&raw).expect("legalized");
        assert_eq!(lowerings.helper_calls_inlined, 1, "{multiplies}");
        let after = check_module(&legal);
        for backend in Backend::ALL {
            assert_eq!(after[&backend], Ok(()), "{multiplies}: {backend:?}");
        }
        assert!(
            !insts_of(&legal)
                .iter()
                .any(|i| opcode(i) == OP_FUNCTION_CALL),
            "{multiplies}: no call is left"
        );
    }
}

#[test]
fn the_inlined_instructions_define_the_calls_result_and_read_its_argument() {
    let insts = insts_of(&looping(true));
    let mut bound = 40;
    let (out, count) = inline_helpers(insts, &mut bound);
    assert_eq!(count, 1);
    // The call was `%30 = f(%23)` and the helper multiplies its parameter by %14
    let multiply = out
        .iter()
        .find(|i| opcode(i) == OP_F_MUL && i[2] == 30)
        .expect("a multiply that defines the call's result");
    assert_eq!(&multiply[3..], &[23, 14]);
    assert_eq!(bound, 40, "the one instruction took the call's own id");
}

#[test]
fn a_helper_that_returns_its_parameter_is_a_copy_of_the_argument() {
    let insts = insts_of(&looping(false));
    let mut bound = 40;
    let (out, count) = inline_helpers(insts, &mut bound);
    assert_eq!(count, 1);
    let copy = out
        .iter()
        .find(|i| opcode(i) == OP_COPY_OBJECT)
        .expect("a copy");
    assert_eq!(&copy[1..], &[4, 30, 23]);
}

#[test]
fn a_helper_with_an_instruction_this_does_not_know_or_a_second_block_is_called_as_it_was() {
    let mut unknown = insts_of(&looping(true));
    for i in unknown.iter_mut().filter(|i| opcode(i) == OP_F_MUL) {
        i[0] = header(0x4242, i.len());
    }
    let mut bound = 40;
    let (out, count) = inline_helpers(unknown.clone(), &mut bound);
    assert_eq!((count, out), (0, unknown));

    let mut two_blocks = insts_of(&looping(true));
    let at = two_blocks
        .iter()
        .position(|i| opcode(i) == OP_RETURN_VALUE)
        .unwrap();
    two_blocks.insert(at, vec![header(OP_LABEL, 2), 99]);
    let (out, count) = inline_helpers(two_blocks.clone(), &mut bound);
    assert_eq!((count, out), (0, two_blocks));
}

#[test]
fn a_call_with_the_wrong_number_of_arguments_is_left_alone() {
    let mut insts = insts_of(&looping(true));
    for i in insts.iter_mut().filter(|i| opcode(i) == OP_FUNCTION_CALL) {
        i.push(23);
        i[0] = header(OP_FUNCTION_CALL, i.len());
    }
    let mut bound = 40;
    let (out, count) = inline_helpers(insts.clone(), &mut bound);
    assert_eq!((count, out), (0, insts));
}
