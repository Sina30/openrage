use super::*;

fn dwords(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A container of `(tag, data)` parts, each padded to whole dwords.
fn container(parts: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_end = CONTAINER_HEADER + parts.len() * 4;
    let mut offsets = Vec::new();
    let mut body = Vec::new();
    for (tag, data) in parts {
        offsets.push((table_end + body.len()) as u32);
        body.extend_from_slice(*tag);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
    }
    let mut out = DXBC_MAGIC.to_vec();
    out.extend_from_slice(&[0; 16]);
    out.extend_from_slice(&[1, 0, 0, 0]);
    out.extend_from_slice(&((table_end + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(parts.len() as u32).to_le_bytes());
    for offset in offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend(body);
    out
}

/// The tokens of a `ps_5_0` that moves four dwords and returns.
const PS_5_0_MOV_RET: [u32; 8] = [0x0000_0050, 8, 0x0500_0036, 1, 2, 3, 4, 0x0100_003e];

fn scanned(name: &str, bytes: &[u8]) -> Inventory {
    let mut inventory = Inventory::default();
    inventory.scan(name, bytes);
    inventory
}

#[test]
fn version_tokens_read_as_the_stage_and_model_a_dump_shows() {
    assert_eq!(split_version(0x0001_0050), (1, (5, 0)));
    assert_eq!(split_version(0x0000_0050), (0, (5, 0)));
    assert_eq!(split_version(0x0000_0040), (0, (4, 0)));
    assert_eq!(split_version(0x0005_0051), (5, (5, 1)));
    assert_eq!(split_version(0x000d_0066), (13, (6, 6)));
}

#[test]
fn real_instruction_tokens_walk_by_their_stated_length() {
    let (opcodes, whole) = walk_instructions(&dwords(&PS_5_0_MOV_RET));
    assert!(whole);
    assert_eq!(opcodes, BTreeMap::from([(0x36, 1), (0x3e, 1)]));
}

#[test]
fn custom_data_takes_its_length_from_the_dword_after_its_token() {
    let words = [0x0001_0050, 8, 53, 5, 7, 7, 7, 0x0100_003e];
    let (opcodes, whole) = walk_instructions(&dwords(&words));
    assert!(whole);
    assert_eq!(opcodes, BTreeMap::from([(53, 1), (0x3e, 1)]));
}

#[test]
fn an_instruction_that_overruns_or_has_no_length_marks_the_program_malformed() {
    let overrun = [0x0000_0050, 4, 0x0500_0036, 1];
    assert!(!walk_instructions(&dwords(&overrun)).1);
    let empty = [0x0000_0050, 3, 0x0000_0036];
    assert!(!walk_instructions(&dwords(&empty)).1);
    let short = [0x0000_0050, 9, 0x0100_003e];
    assert!(!walk_instructions(&dwords(&short)).1);
}

#[test]
fn a_dxbc_container_is_counted_by_stage_model_and_parts() {
    let bytes = container(&[
        (b"RDEF", vec![0; 8]),
        (b"ISGN", vec![0; 8]),
        (b"SHEX", dwords(&PS_5_0_MOV_RET)),
    ]);
    let inventory = scanned("a.fxc", &bytes);
    let profile = Profile {
        kind: Kind::Dxbc,
        stage: Some(0),
        model: Some((5, 0)),
    };
    let tally = &inventory.profiles[&profile];
    assert_eq!(inventory.shaders(), 1);
    assert_eq!((tally.shaders, tally.malformed), (1, 0));
    assert_eq!(tally.parts[b"RDEF"], 1);
    assert_eq!(tally.opcode_shaders[&0x36], 1);
    assert_eq!(inventory.sources[0].containers, 1);
}

#[test]
fn a_dxil_container_reads_the_program_header_and_the_feature_flags() {
    let program = dwords(&[0x0001_0066, 6, 0x4C49_5844, 0x102, 16, 0]);
    let features = (1u64 << 25 | 1 << 14).to_le_bytes().to_vec();
    let bytes = container(&[
        (b"SFI0", features),
        (b"RTS0", vec![0; 4]),
        (b"PSV0", vec![0; 4]),
        (b"DXIL", program),
    ]);
    let inventory = scanned("sga.awc", &bytes);
    let profile = Profile {
        kind: Kind::Dxil,
        stage: Some(1),
        model: Some((6, 6)),
    };
    let tally = &inventory.profiles[&profile];
    assert_eq!(tally.features[&25], 1);
    assert_eq!(tally.features[&14], 1);
    assert_eq!(tally.parts[b"RTS0"], 1);
    assert!(tally.opcode_shaders.is_empty());
}

#[test]
fn a_dxil_part_without_the_magic_is_malformed_and_still_counted() {
    let bytes = container(&[(b"DXIL", dwords(&[0x0000_0060, 6, 0x1234_5678, 0, 0, 0]))]);
    let inventory = scanned("x", &bytes);
    let tally = &inventory.profiles[&Profile {
        kind: Kind::Dxil,
        stage: None,
        model: None,
    }];
    assert_eq!((tally.shaders, tally.malformed), (1, 1));
}

#[test]
fn a_container_with_neither_program_part_is_counted_as_neither() {
    let bytes = container(&[(b"RDEF", vec![0; 8])]);
    let inventory = scanned("x", &bytes);
    assert!(inventory.profiles.keys().all(|p| p.kind == Kind::Neither));
    assert_eq!(inventory.shaders(), 1);
}

#[test]
fn a_dxbc_tag_with_no_container_behind_it_is_refused_and_counted() {
    let mut bytes = b"DXBC".to_vec();
    bytes.extend_from_slice(&[0xff; 60]);
    let inventory = scanned("x", &bytes);
    assert_eq!(inventory.shaders(), 0);
    assert_eq!(inventory.rejected_candidates, 1);
}

#[test]
fn a_container_with_an_offset_outside_itself_is_refused() {
    let mut bytes = container(&[(b"SHEX", dwords(&PS_5_0_MOV_RET))]);
    bytes[CONTAINER_HEADER..CONTAINER_HEADER + 4].copy_from_slice(&0xffffu32.to_le_bytes());
    assert_eq!(scanned("x", &bytes).shaders(), 0);
}

fn spirv_op(opcode: u32, operands: &[u32]) -> Vec<u32> {
    let mut words = vec![((operands.len() as u32 + 1) << 16) | opcode];
    words.extend_from_slice(operands);
    words
}

fn spirv_string(text: &str) -> Vec<u32> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes
        .chunks(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// A vertex entry point with an empty body: the smallest module naga opens.
fn vertex_module() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 5, 0];
    words.extend(spirv_op(OP_CAPABILITY, &[1]));
    words.extend(spirv_op(OP_MEMORY_MODEL, &[0, 1]));
    let mut entry = vec![0, 1];
    entry.extend(spirv_string("main"));
    words.extend(spirv_op(OP_ENTRY_POINT, &entry));
    words.extend(spirv_op(19, &[2]));
    words.extend(spirv_op(33, &[3, 2]));
    words.extend(spirv_op(54, &[2, 1, 0, 3]));
    words.extend(spirv_op(248, &[4]));
    words.extend(spirv_op(253, &[]));
    words.extend(spirv_op(56, &[]));
    dwords(&words)
}

#[test]
fn a_spirv_module_is_read_from_its_header_and_naga_agrees_it_is_one() {
    let bytes = vertex_module();
    naga::front::spv::parse_u8_slice(&bytes, &naga::front::spv::Options::default())
        .expect("naga opens the module the scanner reads");
    let module = spirv_module(&bytes, 0).expect("a module");
    assert_eq!(module.version, (1, 0));
    assert_eq!(module.capabilities, vec![1]);
    assert_eq!(module.memory_model, Some((0, 1)));
    assert_eq!(module.entry_stages, vec![0]);
    assert!(module.extensions.is_empty());
}

#[test]
fn a_spirv_module_names_its_extensions_and_imports() {
    let mut words = vec![0x0723_0203, 0x0001_0500, 0, 9, 0];
    words.extend(spirv_op(OP_CAPABILITY, &[1]));
    words.extend(spirv_op(OP_CAPABILITY, &[5347]));
    words.extend(spirv_op(OP_EXTENSION, &spirv_string("SPV_KHR_example")));
    let mut import = vec![1];
    import.extend(spirv_string("GLSL.std.450"));
    words.extend(spirv_op(OP_EXT_INST_IMPORT, &import));
    words.extend(spirv_op(OP_MEMORY_MODEL, &[2, 3]));
    let module = spirv_module(&dwords(&words), 0).expect("a module");
    assert_eq!(module.version, (1, 5));
    assert_eq!(module.capabilities, vec![1, 5347]);
    assert_eq!(module.extensions, vec!["SPV_KHR_example"]);
    assert_eq!(module.imports, vec!["GLSL.std.450"]);
    assert_eq!(module.memory_model, Some((2, 3)));
}

#[test]
fn the_spirv_magic_without_a_capability_first_is_not_a_module() {
    let words = [0x0723_0203, 0x0001_0000, 0, 5, 0, 0x0003_000e, 0, 1];
    assert_eq!(spirv_module(&dwords(&words), 0), None);
    let bad_version = [0x0723_0203, 0x0002_0000, 0, 5, 0, 0x0002_0011, 1];
    assert_eq!(spirv_module(&dwords(&bad_version), 0), None);
}

#[test]
fn containers_and_modules_are_found_at_any_offset() {
    let mut bytes = vec![9, 9, 9];
    bytes.extend(container(&[(b"SHEX", dwords(&PS_5_0_MOV_RET))]));
    bytes.push(1);
    bytes.extend(vertex_module());
    let inventory = scanned("mixed", &bytes);
    assert_eq!(inventory.sources[0].containers, 1);
    assert_eq!(inventory.sources[0].spirv_modules, 1);
    assert_eq!(inventory.spirv.entry_stages[&0], 1);
}

#[test]
fn a_container_is_skipped_whole_so_a_module_inside_one_is_not_counted() {
    let bytes = container(&[(b"RDEF", vertex_module())]);
    let inventory = scanned("x", &bytes);
    assert_eq!(inventory.shaders(), 1);
    assert_eq!(inventory.spirv.modules, 0);
}

#[test]
fn feature_names_sit_on_the_bits_the_flags_are_documented_at() {
    assert_eq!(1u64 << 14, 0x4000);
    assert_eq!(feature_name(14), "WaveOps");
    assert_eq!(1u64 << 15, 0x8000);
    assert_eq!(feature_name(15), "Int64Ops");
    assert_eq!(1u64 << 20, 0x10_0000);
    assert_eq!(feature_name(20), "Raytracing_Tier_1_1");
    assert_eq!(1u64 << 25, 0x200_0000);
    assert_eq!(feature_name(25), "ResourceDescriptorHeapIndexing");
    assert_eq!(feature_name(27), "bit_27");
    let bits: BTreeSet<u32> = FEATURES.iter().map(|(bit, _)| *bit).collect();
    assert_eq!(bits.len(), FEATURES.len());
}

#[test]
fn the_json_and_the_report_carry_the_same_counts() {
    let bytes = container(&[(b"SHEX", dwords(&PS_5_0_MOV_RET))]);
    let inventory = scanned("a.fxc", &bytes);
    let json = inventory.to_json();
    assert_eq!(json["profiles"][0]["shaders"], 1);
    assert_eq!(json["profiles"][0]["stage"], "PS");
    assert_eq!(json["profiles"][0]["model"], "5.0");
    assert_eq!(json["profiles"][0]["opcodes"]["0x36"]["shaders"], 1);
    let report = inventory.report();
    assert!(report.contains("dxbc"), "{report}");
    assert!(report.contains("PS"), "{report}");
}

#[test]
fn a_directory_of_loose_files_is_scanned_through_its_subfolders() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("any.bin"),
        container(&[(b"SHEX", dwords(&PS_5_0_MOV_RET))]),
    )
    .unwrap();
    std::fs::write(tmp.path().join("noise.txt"), b"nothing here").unwrap();
    let inventory = scan_path(tmp.path(), Target::Auto).unwrap();
    assert_eq!(inventory.shaders(), 1);
    assert_eq!(inventory.sources.len(), 2);
}

#[test]
fn a_missing_path_is_refused_before_anything_is_read() {
    let error = scan_path(Path::new("/nonexistent/shaders"), Target::Auto).unwrap_err();
    assert!(matches!(error, InventoryError::MissingInput(_)));
}

#[test]
fn the_target_names_parse_and_anything_else_does_not() {
    assert_eq!(Target::parse("auto"), Some(Target::Auto));
    assert_eq!(Target::parse("gen8"), Some(Target::Gen8));
    assert_eq!(Target::parse("gen9"), Some(Target::Gen9));
    assert_eq!(Target::parse("gen10"), None);
}

#[test]
fn containers_are_listed_wherever_they_sit_and_each_is_skipped_whole() {
    let first = container(&[(b"SHEX", dwords(&PS_5_0_MOV_RET))]);
    let second = container(&[(b"RDEF", first.clone())]);
    let mut bytes = vec![7, 7];
    bytes.extend_from_slice(&first);
    bytes.push(1);
    bytes.extend_from_slice(&second);
    let found = containers(&bytes);
    assert_eq!(found, vec![&first[..], &second[..]]);
}
