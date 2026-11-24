//! The first entry after an archive's table, opened and extracted as its bytes: the placeholder
//! an archive of one entry that would otherwise be empty holds alone, whose end its bytes state,
//! and any other, which ends where the next row of the list that opened it starts. A nested
//! archive extracts as its bytes from its header to the end of its names, copied a slice at a
//! time so no size holds it in memory.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use or_io::container::rpf::{rpf9_first_entry_end, Rpf9BodyKind, Rpf9ScanError, Rpf9ScanFind};
use or_job::{Cancel, Step};

use super::document::HexDocument;
use super::rpf9_scan::{Ended, Found, Row};

/// Where each first entry whose end is not stated, and each nested archive, ends, by archive and
/// offset, as its list gave it.
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

/// Give `find`, a row of `found`, the list of `archive`, the end a read takes: a first entry whose
/// end is not stated ends where the list's next row starts, or at the file's end where a whole
/// scan of the file lists none after it, and a nested archive where its names row ends. `false`
/// where no row gives it one.
pub(super) fn note(archive: &Path, found: &Found, find: &Rpf9ScanFind) -> bool {
    let rows = &found.rows;
    let end = match find.kind {
        Rpf9BodyKind::FirstEntry { .. } if !ends(find) => {
            let start = |row: &Row| match row {
                Row::Body(find) => find.offset,
                Row::Missing { from, .. } => *from,
            };
            let next = rows.iter().map(start).filter(|at| *at > find.offset).min();
            // ⚠ Only a scan that reached the file's end says no body follows.
            let whole = found.start == 0 && found.ended == Some(Ended::Whole);
            let file_len = || std::fs::metadata(archive).ok().map(|meta| meta.len());
            next.or_else(|| whole.then(file_len).flatten())
        }
        Rpf9BodyKind::Archive(_) => {
            let own = Rpf9BodyKind::Names {
                archive: find.offset,
            };
            rows.iter().find_map(|row| match row {
                Row::Body(names) if names.kind == own => Some(names.offset + names.span),
                _ => None,
            })
        }
        _ => return true,
    };
    let Some(end) = end else {
        return false;
    };
    given().insert((archive.to_path_buf(), find.offset), end);
    true
}

/// The end [`note`] gave the body at `offset` of `archive`, if it gave one.
pub(super) fn noted_end(archive: &Path, offset: u64) -> Option<u64> {
    given().get(&(archive.to_path_buf(), offset)).copied()
}

/// The bytes copied at a time, between two looks for a stop.
const SLICE: usize = 8 << 20;

/// The body at `offset` of `archive` copied to `target` as `rel_path` to the end [`note`] gave
/// it; `None` where it gave none, and the body is read as any other.
pub(super) fn copied(
    archive: &Path,
    offset: u64,
    target: &Path,
    rel_path: &str,
    cancel: &Cancel,
) -> Option<Step> {
    let end = noted_end(archive, offset)?;
    Some(match copy_span(archive, offset, end, target, cancel) {
        Ok(len) => Step::Done(len),
        // A stop leaves no file, and the run counts it as the item it did not reach.
        Err(_) if cancel.is_set() => Step::Skipped,
        Err(e) => Step::Failed(format!("{rel_path}: {e}")),
    })
}

/// The bytes of `archive` from `from` to `to` written to `out` through a file beside it; how many.
fn copy_span(
    archive: &Path,
    from: u64,
    to: u64,
    out: &Path,
    cancel: &Cancel,
) -> Result<u64, String> {
    let len = to
        .checked_sub(from)
        .ok_or_else(|| format!("the span from {from:#X} ends before it, at {to:#X}"))?;
    let at = |e: std::io::Error| format!("{}: {e}", archive.display());
    let mut file = File::open(archive).map_err(at)?;
    file.seek(SeekFrom::Start(from)).map_err(at)?;
    let mut part = out.as_os_str().to_owned();
    part.push(".part");
    let part = Path::new(&part);
    let why = match copy(&mut file.take(len), part, cancel) {
        Ok(n) if n == len => match fs::rename(part, out) {
            Ok(()) => return Ok(n),
            Err(e) => format!("{}: {e}", out.display()),
        },
        Ok(n) => format!(
            "{} ends at {:#X}, before {to:#X}",
            archive.display(),
            from + n
        ),
        Err(e) => e,
    };
    let _ = fs::remove_file(part);
    Err(why)
}

/// Everything `from` holds written to a new file at `to` unless a stop comes first; how many.
fn copy(from: &mut impl Read, to: &Path, cancel: &Cancel) -> Result<u64, String> {
    let at = |e: std::io::Error| format!("{}: {e}", to.display());
    let mut out = File::create(to).map_err(at)?;
    let mut slice = vec![0u8; SLICE];
    let mut copied = 0u64;
    loop {
        if cancel.is_set() {
            return Err("stopped".to_string());
        }
        let n = match from.read(&mut slice) {
            Ok(0) => return Ok(copied),
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("read: {e}")),
        };
        out.write_all(&slice[..n]).map_err(at)?;
        copied += n as u64;
    }
}

/// The bytes of the body at `offset` of `file`, the file at `archive`, that a read refused as
/// `refused`, to the end [`note`] gave it, refused past `limit`; `None` where it gave none.
pub(super) fn bytes(
    archive: &Path,
    file: &File,
    offset: u64,
    limit: u64,
    refused: &Rpf9ScanError,
) -> Option<Result<Vec<u8>, String>> {
    let end = match (noted_end(archive, offset), refused) {
        (Some(end), _) => end,
        // No list gave it an end: the scan from the file's start gives the one its list would.
        (None, Rpf9ScanError::NoBodyAt { .. }) => match rpf9_first_entry_end(file, offset) {
            Ok(end) => end?,
            Err(e) => return Some(Err(e.to_string())),
        },
        (None, _) => return None,
    };
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
    refused: &Rpf9ScanError,
) -> Option<Result<(Rpf9ScanFind, HexDocument), String>> {
    let bytes = match bytes(archive, file, offset, limit, refused)? {
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
        let held = bytes[entry as usize..next as usize].to_vec();
        assert_eq!(
            extracted_body(&archive, entry),
            Ok(held.clone()),
            "with no list, a scan from the file's start gives it the end its list would"
        );
        assert!(extracted_body(&archive, entry + 16).is_err());
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

    #[test]
    fn a_nested_archive_extracts_from_its_header_to_the_end_of_its_names() {
        use std::time::{Duration, Instant};

        use crate::state::opening_an_rpf9_archive::{
            row_at, rows, rpf9_head, scanned, written, zlib,
        };
        use crate::state::AppState;

        const NESTED: u64 = 0x1000;
        let xml = |root: &str| format!("<?xml version=\"1.0\"?>\n<{root}/>\n").into_bytes();
        // A nested archive of one entry whose names only a count places, then a top-level body.
        let mut bytes = rpf9_head(2, 0x1B0);
        bytes.resize(NESTED as usize, 0);
        bytes.extend(rpf9_head(1, 0x1B0));
        bytes.extend(zlib(&xml("CMapTypes")));
        bytes.resize(bytes.len().next_multiple_of(16), 0);
        let names_end = bytes.len() + 0x40;
        bytes.extend([0x33; 0x40]);
        bytes.extend(zlib(&xml("CMapData")));
        let dir = tempfile::tempdir().expect("a scratch directory");
        let mut state = AppState::new();
        let archive = written(dir.path(), &bytes);
        let doc = scanned(&mut state, archive.clone());
        let listed = rows(&state, doc);
        let out = dir.path().join("out");
        let picked = [row_at(&listed, NESTED), row_at(&listed, 0)];
        assert!(state.extract_scanned_bodies(doc, &picked, out.clone()));
        let deadline = Instant::now() + Duration::from_secs(30);
        while state.extract_jobs.is_running() {
            state.poll_extract_job();
            assert!(Instant::now() < deadline, "the extraction never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
        let nested = std::fs::read(out.join("0x1000.rpf")).expect("the nested archive");
        assert_eq!(nested, bytes[NESTED as usize..names_end]);
        // Its end was noted, so the worker copied it as a range rather than reading it whole.
        assert_eq!(noted_end(&archive, NESTED), Some(names_end as u64));
        // The archive the list is of has no names row, so nothing is written of it.
        assert!(!out.join("0x0.rpf").exists());
    }

    #[test]
    fn a_noted_span_is_copied_whole_where_a_read_into_memory_is_refused_past_its_limit() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let archive = dir.path().join("a.rpf");
        // More than one slice, so the copy loops.
        let data: Vec<u8> = (0..SLICE + 0x123).map(|i| (i % 251) as u8).collect();
        std::fs::write(&archive, &data).expect("written");
        let (from, to) = (0x10u64, data.len() as u64 - 0x20);
        let out = dir.path().join("0x10.rpf");
        let copy = |at: u64, out: &Path, cancel: &Cancel| copied(&archive, at, out, "x", cancel);
        assert!(copy(from, &out, &Cancel::new()).is_none(), "no end noted");

        given().insert((archive.clone(), from), to);
        let file = File::open(&archive).expect("opened");
        let refused = Rpf9ScanError::ExtentNotKnown { offset: from };
        let lowered = (to - from) / 2;
        let read = bytes(&archive, &file, from, lowered, &refused);
        assert!(
            matches!(read, Some(Err(_))),
            "a read is refused past its limit"
        );
        let step = copy(from, &out, &Cancel::new());
        assert!(matches!(step, Some(Step::Done(n)) if n == to - from));
        assert_eq!(
            std::fs::read(&out).expect("read back"),
            data[from as usize..to as usize]
        );

        let part = |out: &Path| {
            let mut part = out.as_os_str().to_owned();
            part.push(".part");
            PathBuf::from(part)
        };
        // A span past the file's end, and a stop, leave no file.
        let past = dir.path().join("0x20.rpf");
        given().insert((archive.clone(), 0x20), data.len() as u64 + 1);
        let step = copy(0x20, &past, &Cancel::new());
        assert!(matches!(step, Some(Step::Failed(_))));
        assert!(!past.exists() && !part(&past).exists());
        let stop = Cancel::new();
        stop.set();
        let stopped = dir.path().join("stopped.rpf");
        let step = copied(&archive, from, &stopped, "x", &stop);
        assert!(matches!(step, Some(Step::Skipped)));
        assert!(!stopped.exists() && !part(&stopped).exists());
    }

    #[test]
    fn a_first_entry_no_row_follows_ends_at_the_file_s_end_once_the_scan_is_whole() {
        use crate::state::opening_an_rpf9_archive::{
            row_at, rows, rpf9_head, scanned, settle, written,
        };
        use crate::state::{AppState, DocPayload};

        // Bytes no detector starts on: 0xA7 begins a DEFLATE block of the reserved type.
        let entry = 0x1B0u64;
        let mut data = rpf9_head(2, entry as u32);
        data.resize(0x400, 0xA7);
        let dir = tempfile::tempdir().expect("a scratch directory");
        let archive = written(dir.path(), &data);
        let mut state = AppState::new();
        let doc = scanned(&mut state, archive.clone());
        let listed = rows(&state, doc);
        let at = row_at(&listed, entry);
        assert_eq!(at, listed.len() - 1, "no row follows it: {listed:?}");
        let Row::Body(find) = &listed[at] else {
            panic!("a body");
        };
        assert!(matches!(find.kind, Rpf9BodyKind::FirstEntry { .. }) && !ends(find));
        // A list that has not reached the file's end does not know nothing follows.
        let partial = Found {
            rows: listed.clone(),
            ..Found::default()
        };
        assert!(!note(&archive, &partial, find));
        assert_eq!(noted_end(&archive, entry), None);

        state.open_scanned_body(doc, at);
        settle(&mut state);
        let opened = state.active_document.expect("an open document");
        let Some(DocPayload::Hex(hex)) = state.document(opened).map(|d| &d.payload) else {
            panic!("its bytes: {}", state.error_dialog_message);
        };
        assert_eq!(hex.bytes, data[entry as usize..]);
        assert_eq!(noted_end(&archive, entry), Some(data.len() as u64));
    }
}
