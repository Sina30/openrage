const AT_FRG_LITERAL: u32 = 0xAB34_CAAD;

#[test]
fn pso_stores_the_case_sensitive_hash() {
    assert_eq!(
        or_io::literal_string_hash(b"AT_FRG"),
        AT_FRG_LITERAL,
        "the literal hash of `AT_FRG` should match what a shipped .ymf holds"
    );
}

#[test]
fn the_case_folding_hash_would_have_been_a_different_number() {
    assert_ne!(
        or_io::string_hash(b"AT_FRG"),
        AT_FRG_LITERAL,
        "if these agreed, nothing here could tell the two functions apart"
    );
}

#[test]
fn synthesis_emits_names_the_way_pso_stores_them() {
    for name in ["AT_FRG", "CPedVariationInfo", "mapTypes", "AT_BINARY"] {
        assert_eq!(
            or_io::literal_string_hash(name.as_bytes()),
            or_io::meta::binary::synth::name_hash(name),
            "`{name}` is hashed differently by synthesis than PSO stores it"
        );
    }
}
