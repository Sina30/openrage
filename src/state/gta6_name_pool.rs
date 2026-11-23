//! A scanned body that is a pool of names opens as its names at their offsets, as
//! `rpf scan-open` lists them; Save As still writes the body's own bytes.

use std::path::Path;

use or_io::container::rpf::{Rpf9BodyKind, Rpf9Content, Rpf9ScanFind};

use super::DocPayload;

/// The names of a zlib body no reader fits, where every byte is a name's or its NUL; `None` for any other body.
pub(super) fn payload(path: &Path, find: &Rpf9ScanFind, bytes: &[u8]) -> Option<DocPayload> {
    if !matches!(find.kind, Rpf9BodyKind::Zlib(run) if run.content == Rpf9Content::Other) {
        return None;
    }
    let names = or_cli::gta6_name_pool(bytes)?;
    Some(super::document_io::text_payload(path, names.into_bytes()))
}

#[cfg(test)]
mod tests {
    use crate::state::opening_an_rpf9_archive::zlib;
    use crate::state::rpf9_scan::{body_path, read_body, View};
    use crate::state::DocPayload;

    /// The body at 0x1000 of a file that holds `body` alone, as the list opens it.
    fn opened(body: &[u8]) -> DocPayload {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let archive = dir.path().join("audio.rpf");
        let mut bytes = vec![0u8; 0x1000];
        bytes.extend(zlib(body));
        bytes.resize(0x2000, 0);
        std::fs::write(&archive, &bytes).expect("written");
        let path = body_path(&archive, 0x1000, View::Bytes);
        read_body(&path, &archive, 0x1000).expect("it opens")
    }

    #[test]
    fn a_pool_of_names_opens_as_its_names_and_other_bytes_as_bytes() {
        let pool = b"first_name\0second_name\0";
        let want = or_cli::gta6_name_pool(pool).expect("a pool");
        match opened(pool) {
            DocPayload::Text(text) => assert_eq!(text.text.to_string(), want),
            _ => panic!("a pool of names opens as its names"),
        }
        let not_a_pool = opened(b"first_name\0\x01\x02\0");
        assert!(matches!(not_a_pool, DocPayload::Hex(_)));
    }
}
