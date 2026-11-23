use super::*;
use crate::spirv_testing::{dwords, entry, fragment_with_a_demote, naga_reads, op};

#[test]
fn a_demoting_fragment_shader_is_refused_by_naga_until_it_is_legalized() {
    let original = fragment_with_a_demote(false);
    assert!(
        naga_reads(&original).is_err(),
        "naga refuses the translator's module"
    );
    let (legal, lowerings) = legalize(&original).unwrap();
    let module = naga_reads(&legal).expect("naga reads the legalized module");
    assert_eq!(module.entry_points.len(), 1);
    assert_eq!(
        lowerings,
        Lowerings {
            memory_model: true,
            capabilities_dropped: 4,
            float_modes_dropped: 1,
            local_size_ids: 0,
            demotes_to_kill: 1,
            demotes_kept: 0,
            ..Lowerings::default()
        }
    );
}

#[test]
fn a_phi_loses_the_operand_of_the_block_that_now_kills() {
    let (legal, _) = legalize(&fragment_with_a_demote(true)).unwrap();
    naga_reads(&legal).expect("naga reads the module once the phi names only live parents");
    let words: Vec<u32> = legal
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let phis: Vec<&[u32]> = instructions(&words)
        .unwrap()
        .into_iter()
        .filter(|i| opcode(i) == OP_PHI)
        .collect();
    assert_eq!(phis.len(), 1);
    assert_eq!(phis[0], &[header(OP_PHI, 5), 4, 9, 5, 6]);
}

#[test]
fn local_size_id_becomes_a_local_size_read_from_its_constants() {
    let mut words = vec![MAGIC, 0x0001_0600, 0, 10, 0];
    words.extend(op(OP_CAPABILITY, &[1]));
    words.extend(op(OP_MEMORY_MODEL, &[0, 1]));
    words.extend(entry(5, 1));
    words.extend(op(OP_EXECUTION_MODE_ID, &[1, MODE_LOCAL_SIZE_ID, 5, 6, 7]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(21, &[4, 32, 0]));
    words.extend(op(OP_CONSTANT, &[4, 5, 8]));
    words.extend(op(OP_CONSTANT, &[4, 6, 4]));
    words.extend(op(OP_CONSTANT, &[4, 7, 1]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(OP_LABEL, &[8]));
    words.extend(op(OP_RETURN, &[]));
    words.extend(op(56, &[]));
    let original = dwords(&words);
    assert!(naga_reads(&original).is_err());
    let (legal, lowerings) = legalize(&original).unwrap();
    let module = naga_reads(&legal).expect("naga reads the legalized module");
    assert_eq!(module.entry_points[0].workgroup_size, [8, 4, 1]);
    assert_eq!(lowerings.local_size_ids, 1);
}

#[test]
fn a_demote_that_does_not_end_its_block_is_kept_and_counted() {
    let mut words = vec![MAGIC, 0x0001_0600, 0, 6, 0];
    words.extend(op(OP_CAPABILITY, &[1]));
    words.extend(op(OP_CAPABILITY, &[5379]));
    words.extend(op(OP_MEMORY_MODEL, &[0, 1]));
    words.extend(entry(4, 1));
    words.extend(op(OP_EXECUTION_MODE, &[1, 7]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(OP_LABEL, &[4]));
    words.extend(op(OP_DEMOTE_TO_HELPER_INVOCATION, &[]));
    words.extend(op(0, &[]));
    words.extend(op(OP_RETURN, &[]));
    words.extend(op(56, &[]));
    let (legal, lowerings) = legalize(&dwords(&words)).unwrap();
    assert_eq!((lowerings.demotes_to_kill, lowerings.demotes_kept), (0, 1));
    assert_eq!(
        lowerings.capabilities_dropped, 0,
        "the capability stays while a demote does"
    );
    assert!(naga_reads(&legal).is_err());
}

#[test]
fn a_module_naga_already_reads_comes_back_unchanged() {
    let mut words = vec![MAGIC, 0x0001_0000, 0, 5, 0];
    words.extend(op(OP_CAPABILITY, &[1]));
    words.extend(op(OP_MEMORY_MODEL, &[0, 1]));
    words.extend(entry(0, 1));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(OP_LABEL, &[4]));
    words.extend(op(OP_RETURN, &[]));
    words.extend(op(56, &[]));
    let original = dwords(&words);
    naga_reads(&original).expect("naga reads it as it is");
    let (legal, lowerings) = legalize(&original).unwrap();
    assert_eq!(legal, original);
    assert_eq!(lowerings, Lowerings::default());
}

#[test]
fn bytes_that_are_not_a_module_are_refused() {
    assert!(matches!(
        legalize(&[1, 2, 3]),
        Err(LegalizeError::NotSpirv(_))
    ));
    assert!(matches!(
        legalize(&dwords(&[1, 2, 3, 4, 5])),
        Err(LegalizeError::NotSpirv(_))
    ));
    let overrun = dwords(&[MAGIC, 0x0001_0000, 0, 5, 0, 0x0009_0011, 1]);
    assert!(matches!(legalize(&overrun), Err(LegalizeError::Overrun(5))));
}

fn words_of(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[test]
fn the_memory_model_is_logical_glsl450_once_the_vulkan_model_capability_is_gone() {
    let (legal, _) = legalize(&fragment_with_a_demote(false)).unwrap();
    let words = words_of(&legal);
    let models: Vec<&[u32]> = instructions(&words)
        .unwrap()
        .into_iter()
        .filter(|i| opcode(i) == OP_MEMORY_MODEL)
        .collect();
    assert_eq!(models, vec![&[header(OP_MEMORY_MODEL, 3), 0, 1][..]]);
}

#[test]
fn a_demote_directly_before_a_return_becomes_a_kill() {
    let mut words = vec![MAGIC, 0x0001_0600, 0, 6, 0];
    words.extend(op(OP_CAPABILITY, &[1]));
    words.extend(op(OP_CAPABILITY, &[5379]));
    words.extend(op(OP_MEMORY_MODEL, &[0, 1]));
    words.extend(entry(4, 1));
    words.extend(op(OP_EXECUTION_MODE, &[1, 7]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(OP_LABEL, &[4]));
    words.extend(op(OP_DEMOTE_TO_HELPER_INVOCATION, &[]));
    words.extend(op(OP_RETURN, &[]));
    words.extend(op(56, &[]));
    let original = dwords(&words);
    assert!(naga_reads(&original).is_err());
    let (legal, lowerings) = legalize(&original).unwrap();
    assert_eq!((lowerings.demotes_to_kill, lowerings.demotes_kept), (1, 0));
    naga_reads(&legal).expect("naga reads the module with a kill in place of the demote");
    let words = words_of(&legal);
    let ops: Vec<u32> = instructions(&words)
        .unwrap()
        .iter()
        .map(|i| opcode(i))
        .collect();
    assert!(
        ops.contains(&OP_KILL) && !ops.contains(&OP_RETURN),
        "{ops:?}"
    );
}

#[test]
fn adding_lowerings_sums_every_count_and_keeps_a_memory_model_either_changed() {
    let one = Lowerings {
        memory_model: false,
        capabilities_dropped: 1,
        float_modes_dropped: 2,
        local_size_ids: 3,
        demotes_to_kill: 4,
        demotes_kept: 5,
        interface_groups_merged: 6,
        interface_locations_widened: 7,
        interface_groups_kept: 8,
        clip_distances_dropped: 9,
        base_parameters_zeroed: 11,
        unsigned_negations_subtracted: 12,
        written_buffers_made_readable: 13,
        workgroup_initializers_dropped: 14,
        sample_masks_scalarized: 15,
        interpolations_loaded: 16,
        helper_calls_inlined: 17,
        bit_searches_retyped: 18,
    };
    let other = Lowerings {
        memory_model: true,
        capabilities_dropped: 10,
        float_modes_dropped: 20,
        local_size_ids: 30,
        demotes_to_kill: 40,
        demotes_kept: 50,
        interface_groups_merged: 60,
        interface_locations_widened: 70,
        interface_groups_kept: 80,
        clip_distances_dropped: 90,
        base_parameters_zeroed: 110,
        unsigned_negations_subtracted: 120,
        written_buffers_made_readable: 130,
        workgroup_initializers_dropped: 140,
        sample_masks_scalarized: 150,
        interpolations_loaded: 160,
        helper_calls_inlined: 170,
        bit_searches_retyped: 180,
    };
    let mut total = one.clone();
    total.add(&other);
    assert_eq!(
        total,
        Lowerings {
            memory_model: true,
            capabilities_dropped: 11,
            float_modes_dropped: 22,
            local_size_ids: 33,
            demotes_to_kill: 44,
            demotes_kept: 55,
            interface_groups_merged: 66,
            interface_locations_widened: 77,
            interface_groups_kept: 88,
            clip_distances_dropped: 99,
            base_parameters_zeroed: 121,
            unsigned_negations_subtracted: 132,
            written_buffers_made_readable: 143,
            workgroup_initializers_dropped: 154,
            sample_masks_scalarized: 165,
            interpolations_loaded: 176,
            helper_calls_inlined: 187,
            bit_searches_retyped: 198,
        }
    );
    // Each is named in what the manifest records, and one that did nothing is not
    let counts = total.counts();
    assert_eq!(counts["base_parameters_zeroed"], 121);
    assert_eq!(counts["clip_distances_dropped"], 99);
    assert_eq!(counts["unsigned_negations_subtracted"], 132);
    assert_eq!(counts["written_buffers_made_readable"], 143);
    assert_eq!(counts["workgroup_initializers_dropped"], 154);
    assert_eq!(counts["sample_masks_scalarized"], 165);
    assert_eq!(counts["interpolations_loaded"], 176);
    assert_eq!(counts["helper_calls_inlined"], 187);
    assert_eq!(counts["bit_searches_retyped"], 198);
    assert!(!Lowerings::default()
        .counts()
        .contains_key("base_parameters_zeroed"));
}
