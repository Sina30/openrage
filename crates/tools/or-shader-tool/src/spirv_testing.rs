//! Hand-built SPIR-V modules for tests, spelled with literal opcodes so they do not share a mistake with the code they test.

pub fn dwords(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

pub fn op(opcode: u32, operands: &[u32]) -> Vec<u32> {
    let mut words = vec![(((operands.len() + 1) as u32) << 16) | opcode];
    words.extend_from_slice(operands);
    words
}

pub fn string(text: &str) -> Vec<u32> {
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

pub fn entry(model: u32, id: u32) -> Vec<u32> {
    let mut operands = vec![model, id];
    operands.extend(string("main"));
    op(15, &operands)
}

pub fn naga_reads(bytes: &[u8]) -> Result<naga::Module, String> {
    naga::front::spv::parse_u8_slice(bytes, &naga::front::spv::Options::default())
        .map_err(|e| e.to_string())
}

/// A fragment shader that demotes in one arm, as a translator writes `discard`; `with_phi` joins the arms with a phi.
pub fn fragment_with_a_demote(with_phi: bool) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0600, 0, 10, 0];
    for capability in [1, 5345, 5347, 5379, 4467] {
        words.extend(op(17, &[capability]));
    }
    words.extend(op(14, &[5348, 3]));
    words.extend(entry(4, 1));
    words.extend(op(16, &[1, 7]));
    words.extend(op(16, &[1, 4462, 32]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(20, &[4]));
    words.extend(op(41, &[4, 5]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[6]));
    words.extend(op(247, &[8, 0]));
    words.extend(op(250, &[5, 7, 8]));
    words.extend(op(248, &[7]));
    words.extend(op(5380, &[]));
    words.extend(op(249, &[8]));
    words.extend(op(248, &[8]));
    if with_phi {
        words.extend(op(245, &[4, 9, 5, 6, 5, 7]));
    }
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

/// Parse `bytes` as naga does, then validate against every capability, and say why it stopped.
pub fn naga_validates(bytes: &[u8]) -> Result<naga::Module, String> {
    let module = naga_reads(bytes)?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("{:?}", e.as_inner()))?;
    Ok(module)
}

/// A vertex shader that writes a position and one clip distance, directly or through an access chain into the array.
pub fn vertex_with_clip_distance(through_chain: bool) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 24, 0];
    words.extend(op(17, &[1]));
    words.extend(op(17, &[32]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![0, 1];
    entry_words.extend(string("main"));
    entry_words.extend([5, 6]);
    words.extend(op(15, &entry_words));
    words.extend(op(71, &[5, 11, 0]));
    words.extend(op(71, &[6, 11, 3]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[7, 32]));
    words.extend(op(23, &[8, 7, 4]));
    words.extend(op(21, &[9, 32, 0]));
    words.extend(op(43, &[9, 10, 1]));
    words.extend(op(28, &[11, 7, 10]));
    words.extend(op(32, &[12, 3, 8]));
    words.extend(op(32, &[13, 3, 11]));
    words.extend(op(59, &[12, 5, 3]));
    words.extend(op(59, &[13, 6, 3]));
    words.extend(op(43, &[7, 14, 0x3f80_0000]));
    words.extend(op(44, &[8, 15, 14, 14, 14, 14]));
    words.extend(op(44, &[11, 16, 14]));
    words.extend(op(32, &[17, 3, 7]));
    words.extend(op(43, &[9, 18, 0]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[19]));
    words.extend(op(62, &[5, 15]));
    if through_chain {
        words.extend(op(65, &[17, 20, 6, 18]));
        words.extend(op(62, &[20, 14]));
    } else {
        words.extend(op(62, &[6, 16]));
    }
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

/// A fragment shader that reads its clip distance input and writes it to its output, as a colour.
pub fn fragment_with_clip_distance_input() -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 24, 0];
    words.extend(op(17, &[1]));
    words.extend(op(17, &[32]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(string("main"));
    entry_words.extend([5, 6]);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    words.extend(op(71, &[5, 11, 3]));
    words.extend(op(71, &[6, 30, 0]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[7, 32]));
    words.extend(op(23, &[8, 7, 4]));
    words.extend(op(21, &[9, 32, 0]));
    words.extend(op(43, &[9, 10, 1]));
    words.extend(op(28, &[11, 7, 10]));
    words.extend(op(32, &[12, 1, 11]));
    words.extend(op(32, &[13, 3, 8]));
    words.extend(op(59, &[12, 5, 1]));
    words.extend(op(59, &[13, 6, 3]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[14]));
    words.extend(op(61, &[11, 15, 5]));
    words.extend(op(81, &[7, 16, 15, 0]));
    words.extend(op(80, &[8, 17, 16, 16, 16, 16]));
    words.extend(op(62, &[6, 17]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

/// [`naga_validates`] against a device that offers no clip distance, as Metal does not.
pub fn naga_validates_without_clip_distance(bytes: &[u8]) -> Result<naga::Module, String> {
    let module = naga_reads(bytes)?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all() - naga::valid::Capabilities::CLIP_DISTANCE,
    )
    .validate(&module)
    .map_err(|e| format!("{:?}", e.as_inner()))?;
    Ok(module)
}

/// A vertex shader that reads a built-in draw parameter, `built_in` of the SPIR-V enumeration, and writes it as the position's x.
pub fn vertex_reading_a_draw_parameter(built_in: u32) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 20, 0];
    words.extend(op(17, &[1]));
    // DrawParameters
    words.extend(op(17, &[4427]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![0, 1];
    entry_words.extend(string("main"));
    entry_words.extend([5, 6]);
    words.extend(op(15, &entry_words));
    words.extend(op(71, &[5, 11, built_in]));
    words.extend(op(71, &[6, 11, 0]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[7, 32]));
    words.extend(op(23, &[8, 7, 4]));
    words.extend(op(21, &[9, 32, 0]));
    words.extend(op(32, &[10, 1, 9]));
    words.extend(op(32, &[11, 3, 8]));
    words.extend(op(59, &[10, 5, 1]));
    words.extend(op(59, &[11, 6, 3]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[12]));
    words.extend(op(61, &[9, 13, 5]));
    words.extend(op(112, &[7, 14, 13]));
    words.extend(op(80, &[8, 15, 14, 14, 14, 14]));
    words.extend(op(62, &[6, 15]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}
