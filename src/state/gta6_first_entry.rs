//! The first entry after an archive's table, opened and extracted as its bytes: the placeholder
//! an archive of one entry that would otherwise be empty holds alone, whose end its bytes state,
//! and any other, which ends where the next row of the list that opened it starts.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use or_io::container::rpf::{Rpf9BodyKind, Rpf9ScanError, Rpf9ScanFind};

use super::document::HexDocument;
use super::rpf9_scan::Row;

/// Where each first entry whose end is not stated ends, by archive and offset, as its list gave it.
// ponytail: one map for the whole app, which no close empties; an install holds 15 such entries.
static GIVEN: Mutex<BTreeMap<(PathBuf, u64), u64>> = Mutex::new(BTreeMap::new());

fn given() -> MutexGuard<'static, BTreeMap<(PathBuf, u64), u64>> {
    (GIVEN.lock()).unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

/// Give `find`, a row of `rows`, the list of `archive`, the end a read takes: a first entry whose
/// end is not stated ends where the list's next row starts. `false` where no row follows it.
pub(super) fn note(archive: &Path, rows: &[Row], find: &Rpf9ScanFind) -> bool {
    if !matches!(find.kind, Rpf9BodyKind::FirstEntry { .. }) || ends(find) {
        return true;
    }
    let start = |row: &Row| match row {
        Row::Body(find) => find.offset,
        Row::Missing { from, .. } => *from,
    };
    let next = rows.iter().map(start).filter(|at| *at > find.offset).min();
    let Some(end) = next else {
        return false;
    };
    given().insert((archive.to_path_buf(), find.offset), end);
    true
}

/// The bytes of the first entry at `offset` of `file`, the file at `archive`, to the end [`note`]
/// gave it, refused past `limit`; `None` where it gave none.
pub(super) fn bytes(
    archive: &Path,
    file: &File,
    offset: u64,
    limit: u64,
) -> Option<Result<Vec<u8>, String>> {
    let end = given().get(&(archive.to_path_buf(), offset)).copied()?;
    let len = end.saturating_sub(offset);
    if len > limit {
        return Some(Err(
            Rpf9ScanError::InflateLimit { offset, limit }.to_string()
        ));
    }
    let mut bytes = vec![0; len as usize];
    let mut reader = file;
    let read = (reader.seek(SeekFrom::Start(offset))).and_then(|_| reader.read_exact(&mut bytes));
    Some(
        read.map(|()| bytes)
            .map_err(|e| format!("{}: {e}", archive.display())),
    )
}

/// The first entry at `offset` of `archive` read to the end [`note`] gave it: the find it is
/// opened as and its bytes under why no view reads them; `None` where it gave none.
pub(super) fn opened(
    archive: &Path,
    file: &File,
    offset: u64,
    limit: u64,
) -> Option<Result<(Rpf9ScanFind, HexDocument), String>> {
    let bytes = match bytes(archive, file, offset, limit)? {
        Ok(bytes) => bytes,
        Err(e) => return Some(Err(e)),
    };
    let head = std::array::from_fn(|k| bytes.get(k).copied().unwrap_or(0));
    let find = Rpf9ScanFind {
        offset,
        kind: Rpf9BodyKind::FirstEntry { head },
        span: bytes.len() as u64,
        stored_len: None,
        inflated_len: None,
        container: None,
    };
    let banner = tr!(
        "The first entry after an archive's table, where its header puts it, whose end its bytes do not state: the {count} bytes from it to the next body the list holds. No view reads them, so they show as bytes.",
        count = bytes.len(),
    );
    Some(Ok((find, HexDocument { bytes, banner })))
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

    #[test]
    fn a_first_entry_of_no_stated_end_opens_and_extracts_as_its_bytes_to_the_next_row() {
        use crate::state::opening_an_rpf9_archive::{
            row_at, rows, rpf9_head, scanned, settle, written, zlib,
        };
        use crate::state::rpf9_scan::{body_path, extracted_body, scan_origin, View};
        use crate::state::{AppState, DocPayload};

        // Bytes no detector starts on: 0xA7 begins a DEFLATE block of the reserved type.
        let (entry, next) = (0x1B0u64, 0x200u64);
        let mut bytes = rpf9_head(2, entry as u32);
        bytes.resize(next as usize, 0xA7);
        bytes.extend(zlib(b"<?xml version=\"1.0\"?>\n<CMapTypes/>\n"));
        let dir = tempfile::tempdir().expect("a scratch directory");
        let archive = written(dir.path(), &bytes);
        assert!(
            extracted_body(&archive, entry).is_err(),
            "no list gave it an end yet"
        );
        let mut state = AppState::new();
        let parent = scanned(&mut state, archive.clone());
        let listed = rows(&state, parent);
        let at = row_at(&listed, entry);
        let Row::Body(find) = &listed[at] else {
            panic!("a body");
        };
        assert!(matches!(find.kind, Rpf9BodyKind::FirstEntry { .. }));
        assert_eq!(find.stored_len, None, "its bytes state no end");
        assert!(listed
            .iter()
            .any(|r| matches!(r, Row::Body(f) if f.offset == next)));

        state.open_scanned_body(parent, at);
        settle(&mut state);
        let doc = state.active_document.expect("an open document");
        let Some(DocPayload::Hex(hex)) = state.document(doc).map(|d| &d.payload) else {
            panic!("its bytes: {}", state.error_dialog_message);
        };
        let held = bytes[entry as usize..next as usize].to_vec();
        assert_eq!(hex.bytes, held);
        assert!(
            hex.banner.contains(&held.len().to_string()),
            "{}",
            hex.banner
        );
        assert_eq!(extracted_body(&archive, entry), Ok(held));
        let path = body_path(&archive, entry, View::Bytes);
        let origin = scan_origin(&path).map(|(find, _)| find.offset);
        assert_eq!(origin, Some(entry), "its origin is kept for the inspector");
    }
}
