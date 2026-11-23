//! The first entry after an archive's table whose end its bytes state: the placeholder an archive
//! of one entry that would otherwise be empty holds alone, opened and extracted as its bytes.

use or_io::container::rpf::{Rpf9BodyKind, Rpf9ScanFind};

/// Whether `find` is a first entry whose length the scan stated, so a read gives it an end.
pub(super) fn ends(find: &Rpf9ScanFind) -> bool {
    matches!(find.kind, Rpf9BodyKind::FirstEntry { .. }) && find.stored_len.is_some()
}

/// Why no view reads a first entry whose end is stated.
pub(super) fn banner(find: &Rpf9ScanFind) -> String {
    tr!(
        "The first entry after an archive's table, where its header puts it: the {count} bytes an archive of one entry that would otherwise be empty holds alone. No view reads them, so they show as bytes.",
        count = find.stored_len.unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use or_io::container::rpf::{read_rpf9_body, EMPTY_ARCHIVE_PLACEHOLDER, RPF9_SLOT};

    use super::*;

    #[test]
    fn a_placeholder_opens_and_extracts_as_its_bytes_under_why_no_view_reads_it() {
        use crate::state::rpf9_scan::{body_path, extracted_body, read_body, View};
        use crate::state::DocPayload;

        let dir = tempfile::tempdir().expect("a scratch directory");
        let archive = dir.path().join("one.rpf");
        let mut bytes = vec![0u8; 0x1000];
        bytes.extend_from_slice(EMPTY_ARCHIVE_PLACEHOLDER);
        bytes.resize(0x2000, 0);
        std::fs::write(&archive, &bytes).expect("written");
        let path = body_path(&archive, 0x1000, View::Bytes);
        let hex = match read_body(&path, &archive, 0x1000) {
            Ok(DocPayload::Hex(hex)) => hex,
            other => panic!("its bytes: {:?}", other.err()),
        };
        assert_eq!(hex.bytes, EMPTY_ARCHIVE_PLACEHOLDER.to_vec());
        let read = read_rpf9_body(Cursor::new(&bytes), 0x1000, u64::MAX);
        let (find, _) = read.expect("read");
        assert!(ends(&find));
        assert_eq!(find.stored_len, Some(RPF9_SLOT));
        assert_eq!(hex.banner, banner(&find));
        assert!(banner(&find).contains(&RPF9_SLOT.to_string()));
        let unstated = Rpf9ScanFind {
            stored_len: None,
            ..find
        };
        assert!(!ends(&unstated));
        assert_eq!(
            extracted_body(&archive, 0x1000),
            Ok(EMPTY_ARCHIVE_PLACEHOLDER.to_vec())
        );
        assert!(
            read_body(&path, &archive, 0x800).is_err(),
            "zero bytes are no body"
        );
    }
}
