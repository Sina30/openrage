use super::*;
use crate::legalize::{instructions, legalize};
use crate::spirv_testing::{dwords, entry, naga_reads, naga_validates, op};

fn words_of(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn insts_of(bytes: &[u8]) -> Vec<Vec<u32>> {
    let words = words_of(bytes);
    instructions(&words)
        .unwrap()
        .into_iter()
        .map(<[u32]>::to_vec)
        .collect()
}

/// A fragment shader that negates a value of the integer type `int_type` (an `OpTypeInt` width and signedness), or of a vector of two of it.
fn negating(signed: u32, vector: bool) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 12, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(4, 1));
    words.extend(op(16, &[1, 7]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(21, &[4, 32, signed]));
    words.extend(op(43, &[4, 5, 7]));
    let ty = if vector {
        words.extend(op(23, &[6, 4, 2]));
        words.extend(op(44, &[6, 8, 5, 5]));
        6
    } else {
        4
    };
    let operand = if vector { 8 } else { 5 };
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[9]));
    words.extend(op(126, &[ty, 10, operand]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn naga_refuses_to_negate_an_unsigned_integer_until_it_is_subtracted_from_nought() {
    for vector in [false, true] {
        let raw = negating(0, vector);
        assert!(
            naga_validates(&raw).is_err(),
            "vector {vector}: naga takes it as it is"
        );
        let (legal, lowerings) = legalize(&raw).expect("legalized");
        assert_eq!(
            lowerings.unsigned_negations_subtracted, 1,
            "vector {vector}"
        );
        naga_validates(&legal).unwrap_or_else(|e| panic!("vector {vector}: {e}"));
        let insts = insts_of(&legal);
        assert!(
            !insts.iter().any(|i| opcode(i) == OP_S_NEGATE),
            "vector {vector}: no negation is left"
        );
        let sub = insts
            .iter()
            .find(|i| opcode(i) == OP_I_SUB)
            .expect("a subtraction");
        let zero = insts
            .iter()
            .find(|i| opcode(i) == OP_CONSTANT_NULL && i[2] == sub[3])
            .expect("its first operand is a null constant");
        assert_eq!(zero[1], sub[1], "of the type being negated");
    }
}

#[test]
fn a_signed_negation_is_left_alone() {
    let raw = negating(1, false);
    naga_validates(&raw).expect("naga takes a signed negation");
    let (legal, lowerings) = legalize(&raw).expect("legalized");
    assert_eq!(lowerings.unsigned_negations_subtracted, 0);
    assert_eq!(insts_of(&legal), insts_of(&raw));
}

#[test]
fn one_zero_is_declared_for_every_negation_of_a_type() {
    let raw = negating(0, false);
    let mut insts = insts_of(&raw);
    let at = insts.iter().position(|i| opcode(i) == OP_S_NEGATE).unwrap();
    let again = {
        let mut next = insts[at].clone();
        next[2] = 11;
        next
    };
    insts.insert(at + 1, again);
    let mut bound = 12;
    let (out, count) = subtract_unsigned_negations(insts, &mut bound);
    assert_eq!(count, 2);
    assert_eq!(
        out.iter().filter(|i| opcode(i) == OP_CONSTANT_NULL).count(),
        1
    );
    assert_eq!(bound, 13, "one id was taken");
}

/// A compute shader that writes one word of a storage buffer that it decorates `NonReadable`, on the variable or on the struct's member.
fn writing_a_buffer(decorate_the_variable: bool) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0300, 0, 14, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(5, 1));
    words.extend(op(16, &[1, 17, 1, 1, 1]));
    words.extend(op(71, &[6, 2]));
    words.extend(op(72, &[6, 0, 35, 0]));
    words.extend(op(71, &[8, 34, 0]));
    words.extend(op(71, &[8, 33, 0]));
    if decorate_the_variable {
        words.extend(op(71, &[8, 25]));
    } else {
        words.extend(op(72, &[6, 0, 25]));
    }
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(21, &[4, 32, 0]));
    words.extend(op(43, &[4, 5, 0]));
    words.extend(op(30, &[6, 4]));
    words.extend(op(32, &[7, 12, 6]));
    words.extend(op(59, &[7, 8, 12]));
    words.extend(op(32, &[9, 12, 4]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[10]));
    words.extend(op(65, &[9, 11, 8, 5]));
    words.extend(op(62, &[11, 5]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn naga_refuses_a_buffer_nothing_reads_until_it_can_be_read() {
    let raw = writing_a_buffer(true);
    assert!(
        naga_validates(&raw).is_err(),
        "naga has no write-only buffer"
    );
    let (legal, lowerings) = legalize(&raw).expect("legalized");
    assert_eq!(lowerings.written_buffers_made_readable, 1);
    naga_validates(&legal).expect("naga takes it now");
    assert!(!insts_of(&legal)
        .iter()
        .any(|i| opcode(i) == OP_DECORATE && i.get(2) == Some(&DECORATION_NON_READABLE)));
}

#[test]
fn a_member_that_is_not_readable_is_left_as_naga_takes_it() {
    let raw = writing_a_buffer(false);
    naga_validates(&raw).expect("naga takes the member form");
    let (legal, lowerings) = legalize(&raw).expect("legalized");
    assert_eq!(lowerings.written_buffers_made_readable, 0);
    assert_eq!(insts_of(&legal), insts_of(&raw));
}

#[test]
fn a_write_only_image_keeps_the_decoration_that_says_so() {
    let insts = vec![
        vec![header(OP_VARIABLE, 4), 7, 8, 0],
        vec![header(OP_DECORATE, 3), 8, DECORATION_NON_READABLE],
    ];
    let (out, count) = make_written_buffers_readable(insts.clone());
    assert_eq!((count, out), (0, insts));
}

/// A compute shader with a workgroup variable that starts at nought.
fn workgroup_with_an_initializer() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0300, 0, 10, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(5, 1));
    words.extend(op(16, &[1, 17, 1, 1, 1]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(21, &[4, 32, 0]));
    words.extend(op(43, &[4, 5, 0]));
    words.extend(op(32, &[6, 4, 4]));
    words.extend(op(59, &[6, 7, 4, 5]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[8]));
    words.extend(op(62, &[7, 5]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn naga_refuses_an_initial_value_on_workgroup_memory_until_it_is_taken_off() {
    let raw = workgroup_with_an_initializer();
    assert!(naga_reads(&raw).is_ok() && naga_validates(&raw).is_err());
    let (legal, lowerings) = legalize(&raw).expect("legalized");
    assert_eq!(lowerings.workgroup_initializers_dropped, 1);
    naga_validates(&legal).expect("naga takes it now");
    let variable = insts_of(&legal)
        .into_iter()
        .find(|i| opcode(i) == OP_VARIABLE)
        .unwrap();
    assert_eq!(variable.len(), 4);
}

#[test]
fn a_private_variable_keeps_its_initializer() {
    let insts = vec![vec![header(OP_VARIABLE, 5), 6, 7, 6, 5]];
    let (out, count) = drop_workgroup_initializers(insts.clone());
    assert_eq!((count, out), (0, insts));
}

/// A fragment shader that writes the coverage mask through an array of one, as a translator declares it; `pointer_first` has the pointer type to the integer declared before the variable and not after it.
fn writing_a_sample_mask(pointer_first: bool) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 14, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.push(8);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    words.extend(op(71, &[8, 11, 20]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(21, &[4, 32, 0]));
    words.extend(op(43, &[4, 5, 1]));
    words.extend(op(28, &[6, 4, 5]));
    if pointer_first {
        words.extend(op(32, &[9, 3, 4]));
    }
    words.extend(op(32, &[7, 3, 6]));
    words.extend(op(59, &[7, 8, 3]));
    if !pointer_first {
        words.extend(op(32, &[9, 3, 4]));
    }
    words.extend(op(43, &[4, 10, 0]));
    words.extend(op(43, &[4, 11, 0xFFFF_FFFF]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[12]));
    words.extend(op(65, &[9, 13, 8, 10]));
    words.extend(op(62, &[13, 11]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn naga_refuses_a_coverage_mask_declared_as_an_array_until_it_is_one_integer() {
    for pointer_first in [false, true] {
        let raw = writing_a_sample_mask(pointer_first);
        assert!(
            naga_validates(&raw).is_err(),
            "{pointer_first}: naga takes it as it is"
        );
        let (legal, lowerings) = legalize(&raw).expect("legalized");
        assert_eq!(lowerings.sample_masks_scalarized, 1, "{pointer_first}");
        let module = naga_validates(&legal).unwrap_or_else(|e| panic!("{pointer_first}: {e}"));
        let mask = module
            .entry_points
            .iter()
            .flat_map(|e| e.function.result.iter())
            .any(|r| {
                matches!(
                    r.binding,
                    Some(naga::Binding::BuiltIn(naga::BuiltIn::SampleMask))
                )
            });
        assert!(
            mask,
            "{pointer_first}: the entry point writes the sample mask"
        );
        let insts = insts_of(&legal);
        assert!(
            !insts.iter().any(|i| opcode(i) == OP_ACCESS_CHAIN),
            "{pointer_first}: the chain into the array is gone"
        );
        assert_eq!(
            words_of(&legal)[3],
            if pointer_first { 14 } else { 15 },
            "{pointer_first}: a pointer type is declared only when there is none to use"
        );
        // A pointer type to the integer is declared before the variable that uses it, whichever way the module had it
        let variable = insts.iter().position(|i| opcode(i) == OP_VARIABLE).unwrap();
        let variable_type = insts[variable][1];
        assert!(
            insts[..variable]
                .iter()
                .any(|i| opcode(i) == OP_TYPE_POINTER
                    && i[1] == variable_type
                    && i[2] == STORAGE_OUTPUT
                    && i[3] == 4),
            "{pointer_first}"
        );
    }
}

#[test]
fn a_module_with_no_coverage_mask_is_returned_as_it_was() {
    let insts = insts_of(&negating(1, false));
    let mut bound = 12;
    let (out, count) = scalarize_sample_masks(insts.clone(), &mut bound);
    assert_eq!((count, bound, out), (0, 12, insts));
}

/// A fragment shader that reads its input at a sample, as `EvaluateAttributeAtSample` does, or at a centroid.
fn interpolating_at(instruction: u32) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 16, 0];
    words.extend(op(17, &[1]));
    words.extend(op(17, &[CAPABILITY_INTERPOLATION_FUNCTION]));
    let mut import = vec![10];
    import.extend(crate::spirv_testing::string("GLSL.std.450"));
    words.extend(op(11, &import));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.extend([6, 8]);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    words.extend(op(71, &[6, 30, 0]));
    words.extend(op(71, &[8, 30, 0]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[4, 32]));
    words.extend(op(32, &[5, 1, 4]));
    words.extend(op(59, &[5, 6, 1]));
    words.extend(op(32, &[7, 3, 4]));
    words.extend(op(59, &[7, 8, 3]));
    words.extend(op(21, &[11, 32, 1]));
    words.extend(op(43, &[11, 12, 0]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[13]));
    let mut call = vec![4, 14, 10, instruction, 6];
    if instruction != 76 {
        call.push(12);
    }
    words.extend(op(12, &call));
    words.extend(op(62, &[8, 14]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn naga_refuses_an_input_read_at_a_sample_a_centroid_or_an_offset_until_it_is_read_where_it_is() {
    for instruction in INTERPOLATE_AT {
        let raw = interpolating_at(instruction);
        assert!(naga_reads(&raw).is_err(), "{instruction}: naga reads it");
        let (legal, lowerings) = legalize(&raw).expect("legalized");
        assert_eq!(lowerings.interpolations_loaded, 1, "{instruction}");
        assert!(lowerings.approximates(), "{instruction}");
        naga_validates(&legal).unwrap_or_else(|e| panic!("{instruction}: {e}"));
        let insts = insts_of(&legal);
        assert!(
            !insts.iter().any(|i| opcode(i) == OP_EXT_INST),
            "{instruction}: no extended instruction is left"
        );
        assert!(
            !insts.iter().any(|i| opcode(i) == OP_CAPABILITY
                && i.get(1) == Some(&CAPABILITY_INTERPOLATION_FUNCTION)),
            "{instruction}: nor the capability for them"
        );
    }
}

#[test]
fn an_extended_instruction_of_another_set_or_number_is_left_alone() {
    let raw = interpolating_at(77);
    let insts = insts_of(&raw);
    // Another instruction of the same set: a square root, whose number is not one of the three
    let mut other = insts.clone();
    for i in other.iter_mut().filter(|i| opcode(i) == OP_EXT_INST) {
        i[4] = 31;
    }
    let (out, count) = load_interpolations(other.clone());
    assert_eq!((count, out), (0, other));
    // The same number from a set that is not GLSL's
    let mut foreign = insts;
    let mut import = vec![header(OP_EXT_INST_IMPORT, 2), 99];
    import.extend(crate::spirv_testing::string("Other.set"));
    import[0] = header(OP_EXT_INST_IMPORT, import.len());
    let at = foreign
        .iter()
        .position(|i| opcode(i) == OP_EXT_INST_IMPORT)
        .unwrap();
    foreign.insert(at + 1, import);
    for i in foreign.iter_mut().filter(|i| opcode(i) == OP_EXT_INST) {
        i[3] = 99;
    }
    let (out, count) = load_interpolations(foreign.clone());
    assert_eq!((count, out), (0, foreign));
}

/// A fragment shader that searches an input of the given signedness for a bit with `instruction`, which SPIR-V types as the other signedness, and adds a number of that type to what it finds.
fn searching_for_a_bit(operand_signed: u32, instruction: u32) -> Vec<u8> {
    let result_signed = 1 - operand_signed;
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 24, 0];
    words.extend(op(17, &[1]));
    let mut import = vec![20];
    import.extend(crate::spirv_testing::string("GLSL.std.450"));
    words.extend(op(11, &import));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.extend([8, 10]);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    words.extend(op(71, &[8, 30, 0]));
    words.extend(op(71, &[8, 14]));
    words.extend(op(71, &[10, 30, 0]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(21, &[4, 32, operand_signed]));
    words.extend(op(21, &[5, 32, result_signed]));
    words.extend(op(22, &[6, 32]));
    words.extend(op(32, &[7, 1, 4]));
    words.extend(op(59, &[7, 8, 1]));
    words.extend(op(32, &[9, 3, 6]));
    words.extend(op(59, &[9, 10, 3]));
    words.extend(op(43, &[5, 11, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[12]));
    words.extend(op(61, &[4, 13, 8]));
    words.extend(op(12, &[5, 14, 20, instruction, 13]));
    words.extend(op(128, &[5, 15, 14, 11]));
    words.extend(op(if result_signed == 1 { 111 } else { 112 }, &[6, 16, 15]));
    words.extend(op(62, &[10, 16]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

#[test]
fn naga_refuses_to_add_to_a_bit_search_until_it_is_typed_as_its_operand_and_bitcast() {
    for (operand_signed, instruction) in [(0, 73), (0, 75), (1, 74)] {
        let raw = searching_for_a_bit(operand_signed, instruction);
        assert!(
            naga_validates(&raw).is_err(),
            "{instruction}: naga takes it as it is"
        );
        let (legal, lowerings) = legalize(&raw).expect("legalized");
        assert_eq!(lowerings.bit_searches_retyped, 1, "{instruction}");
        naga_validates(&legal).unwrap_or_else(|e| panic!("{instruction}: {e}"));
        let insts = insts_of(&legal);
        let search = insts
            .iter()
            .find(|i| opcode(i) == OP_EXT_INST && i[4] == instruction)
            .expect("the search");
        assert_eq!(search[1], 4, "{instruction}: typed as its operand");
        let cast = insts
            .iter()
            .find(|i| opcode(i) == OP_BITCAST)
            .expect("a bitcast");
        assert_eq!(
            (cast[1], cast[2], cast[3]),
            (5, 14, search[2]),
            "{instruction}"
        );
    }
}

#[test]
fn a_bit_search_typed_as_its_operand_is_left_alone() {
    // Result and operand of one signedness, which is the shape naga takes as it is
    let mut insts = insts_of(&searching_for_a_bit(0, 73));
    for i in insts.iter_mut().filter(|i| opcode(i) == OP_EXT_INST) {
        i[1] = 4;
    }
    let mut bound = 24;
    let (out, count) = retype_bit_searches(insts.clone(), &mut bound);
    assert_eq!((count, bound, out), (0, 24, insts));
}

#[test]
fn a_bit_search_of_an_operand_whose_type_is_not_known_is_left_alone() {
    let mut insts = insts_of(&searching_for_a_bit(0, 73));
    for i in insts.iter_mut().filter(|i| opcode(i) == OP_EXT_INST) {
        i[5] = 999;
    }
    let mut bound = 24;
    let (out, count) = retype_bit_searches(insts.clone(), &mut bound);
    assert_eq!((count, bound, out), (0, 24, insts));
}
