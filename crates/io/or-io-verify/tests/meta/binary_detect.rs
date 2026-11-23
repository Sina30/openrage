use or_io::meta::{detect_binary_format, BinaryFormat};

#[test]
fn detects_pso() {
    assert_eq!(
        detect_binary_format(b"PSIN\x00\x00\x00\x10rest..."),
        Some(BinaryFormat::Pso)
    );
}

#[test]
fn detects_rbf() {
    // RBF0 little-endian = 0x30464252.
    assert_eq!(
        detect_binary_format(b"RBF0\x00\x01\x02\x03"),
        Some(BinaryFormat::Rbf)
    );
}

#[test]
fn detects_rsc7_as_rbm() {
    assert_eq!(
        detect_binary_format(b"RSC7\x02\x00\x00\x00more..."),
        Some(BinaryFormat::Rbm)
    );
}

#[test]
fn ignores_xml_doc() {
    assert!(detect_binary_format(b"<?xml version=\"1.0\"?>\n").is_none());
    assert!(detect_binary_format(b"<CAmbientModelSet>\n").is_none());
}

#[test]
fn ignores_short_input() {
    assert!(detect_binary_format(b"").is_none());
    assert!(detect_binary_format(b"PSI").is_none());
}
