//! The bodies a read gives no end, written as the bytes a scan gives them: a first entry whose
//! end its bytes do not state, to the next body the scan lists, and a nested archive, from its
//! header to the end of its names.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use or_io::container::rpf::{
    rpf9_first_entry_end, rpf9_names_by_count_at, Rpf9BodyKind, Rpf9NamesByCount, Rpf9ScanError,
    Rpf9ScanFind,
};

/// Where the archive nested at `offset` of `file` ends: with the names a scan from its header
/// places, else with the names placed by counting the entries its header states.
pub(crate) fn archive_end(file: &fs::File, offset: u64) -> Result<Option<u64>, String> {
    if let Some(end) = super::rpf::nested_end(file, offset)? {
        return Ok(Some(end));
    }
    let placed = rpf9_names_by_count_at(file, offset).map_err(|e| e.to_string())?;
    Ok(placed.map(|names| names.offset + names.span))
}

/// The bytes of `file` from `from` to `to` written to `out`, through a file beside it; how many.
pub(crate) fn write_span(file: &fs::File, from: u64, to: u64, out: &Path) -> Result<u64, String> {
    let len = to
        .checked_sub(from)
        .ok_or_else(|| format!("the span from {from:#X} ends before it, at {to:#X}"))?;
    let mut reader = file;
    reader
        .seek(SeekFrom::Start(from))
        .map_err(|e| format!("seek to {from:#X}: {e}"))?;
    let mut part = out.as_os_str().to_owned();
    part.push(".part");
    let part = Path::new(&part);
    let mut copy =
        fs::File::create(part).map_err(|e| format!("create({}): {e}", part.display()))?;
    let copied = std::io::copy(&mut reader.take(len), &mut copy);
    drop(copy);
    let why = match copied {
        Ok(n) if n == len => match fs::rename(part, out) {
            Ok(()) => return Ok(n),
            Err(e) => format!("write({}): {e}", out.display()),
        },
        Ok(n) => format!("the file ends at {:#X}, before {to:#X}", from + n),
        Err(e) => format!("write({}): {e}", out.display()),
    };
    let _ = fs::remove_file(part);
    Err(why)
}

/// `rpf scan-body` at `offset` of `file`, which a read refused as `refused`: the span a scan
/// gives the body written to `out`, or the refusal where it gives none.
pub(crate) fn write_refused(
    file: &fs::File,
    offset: u64,
    refused: &Rpf9ScanError,
    out: &str,
) -> Result<(), String> {
    let (end, what) = match refused {
        Rpf9ScanError::ExtentNotKnown { .. } if offset > 0 => (
            archive_end(file, offset)?,
            "the archive nested here, from its header to the end of its names: `rpf scan` lists \
             it from offset 0",
        ),
        Rpf9ScanError::NoBodyAt { .. } => (
            rpf9_first_entry_end(file, offset).map_err(|e| e.to_string())?,
            "a first entry whose end its bytes do not state, to the next body a scan of the \
             archive lists: its bytes, which no reader reads",
        ),
        _ => (None, ""),
    };
    let Some(end) = end else {
        return Err(refused.to_string());
    };
    let len = write_span(file, offset, end, Path::new(out))?;
    println!("  {offset:#012X}  {what}");
    println!("  wrote {len} bytes to {out}, the bytes from {offset:#X} to {end:#X}");
    Ok(())
}

/// The ends a whole scan's finds give the bodies a read gives none, as [`write_refused`] gives
/// each alone.
#[derive(Default)]
pub(crate) struct Spans {
    ends: BTreeMap<u64, u64>,
    /// A first entry of unstated end that no find has followed yet.
    entry: Option<u64>,
    by_count: Rpf9NamesByCount,
}

impl Spans {
    /// Keep what `find`, the scan's next find, says of the ends.
    pub(crate) fn push(&mut self, find: &Rpf9ScanFind) {
        if let Some(entry) = self.entry.filter(|entry| find.offset > *entry) {
            self.ends.insert(entry, find.offset);
            self.entry = None;
        }
        match find.kind {
            Rpf9BodyKind::FirstEntry { .. } if find.stored_len.is_none() => {
                self.entry = Some(find.offset);
            }
            Rpf9BodyKind::Names { archive } => {
                self.ends.insert(archive, find.offset + find.span);
            }
            _ => {}
        }
        self.by_count.push(find);
    }

    /// The end of each such body by its offset, once the scan of `file`, `len` bytes, has ended.
    pub(crate) fn ends(mut self, file: &fs::File, len: u64) -> Result<BTreeMap<u64, u64>, String> {
        if let Some(entry) = self.entry.take() {
            self.ends.insert(entry, len);
        }
        for names in self.by_count.place(file).map_err(|e| e.to_string())? {
            if let Rpf9BodyKind::Names { archive } = names.kind {
                self.ends.insert(archive, names.offset + names.span);
            }
        }
        Ok(self.ends)
    }
}

#[cfg(test)]
mod tests {
    use or_io::container::rpf::Rpf9Scan;

    use super::*;

    /// An RPF9 head of layout 4 of `entries` entries that holds every measured rule, and its
    /// table, `toc_end` bytes in all.
    fn head(entries: u32, toc_end: u32) -> Vec<u8> {
        let mut bytes = vec![0x5Au8; toc_end as usize];
        bytes[..0x40].fill(0);
        bytes[..4].copy_from_slice(b"9FPR");
        bytes[0x04..0x08].copy_from_slice(&toc_end.to_le_bytes());
        let index = toc_end - (entries * 9).next_multiple_of(16);
        bytes[0x08..0x0C].copy_from_slice(&index.to_le_bytes());
        bytes[0x0C..0x10].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x1C..0x20].copy_from_slice(&0x150u32.to_le_bytes());
        bytes[0x22..0x24].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x24..0x28].copy_from_slice(&entries.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&0x10u32.to_le_bytes());
        bytes[0x2C] = 1;
        (bytes[0x19], bytes[0x1A], bytes[0x20]) = (0x7A, 4, 5);
        (bytes[0x2D], bytes[0x2E]) = (0x0F, 0x09);
        bytes
    }

    /// `data` as a zlib stream of one stored deflate block under the `78 DA` head.
    fn zlib(data: &[u8]) -> Vec<u8> {
        let len = u16::try_from(data.len()).expect("one stored block");
        let mut out = vec![0x78, 0xDA, 0x01];
        out.extend(len.to_le_bytes());
        out.extend((!len).to_le_bytes());
        out.extend_from_slice(data);
        let (a, b) = data.iter().fold((1u32, 0u32), |(a, b), &c| {
            let a = (a + u32::from(c)) % 65_521;
            (a, (b + a) % 65_521)
        });
        out.extend(((b << 16) | a).to_be_bytes());
        out
    }

    fn xml(root: &str) -> Vec<u8> {
        format!("<?xml version=\"1.0\"?>\n<{root}/>\n").into_bytes()
    }

    const NESTED: u64 = 0x1000;
    const ENTRY: u64 = 0x1B0;

    /// An archive whose first entry, at `ENTRY`, no detector names, then a nested archive of one
    /// entry whose names only a count places, then a top-level body; and where those names start.
    fn archive() -> (Vec<u8>, u64) {
        // Bytes no detector starts on: 0xA7 begins a DEFLATE block of the reserved type.
        let mut bytes = head(2, ENTRY as u32);
        bytes.resize(0x200, 0xA7);
        bytes.extend(zlib(&xml("CMapTypes")));
        bytes.resize(NESTED as usize, 0);
        bytes.extend(head(1, 0x1B0));
        bytes.extend(zlib(&xml("CMapTypes")));
        bytes.resize(bytes.len().next_multiple_of(16), 0);
        let names_at = bytes.len() as u64;
        bytes.extend([0x33; 0x40]);
        bytes.extend(zlib(&xml("CMapData")));
        (bytes, names_at)
    }

    #[test]
    fn a_first_entry_and_a_nested_archive_end_where_a_scan_places_them_alone_or_in_its_list() {
        let (bytes, names_at) = archive();
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("spans.rpf");
        fs::write(&path, &bytes).expect("written");
        let file = fs::File::open(&path).expect("opened");

        let mut spans = Spans::default();
        for find in Rpf9Scan::new(&file, 0..u64::MAX).expect("a scan") {
            spans.push(&find.expect("a find"));
        }
        let ends = spans.ends(&file, bytes.len() as u64).expect("placed");
        let expected = BTreeMap::from([(ENTRY, 0x200), (NESTED, names_at + 0x40)]);
        assert_eq!(ends, expected);
        assert_eq!(archive_end(&file, NESTED), Ok(Some(names_at + 0x40)));

        for (offset, refused, end) in [
            (ENTRY, Rpf9ScanError::NoBodyAt { offset: ENTRY }, 0x200),
            (
                NESTED,
                Rpf9ScanError::ExtentNotKnown { offset: NESTED },
                names_at + 0x40,
            ),
        ] {
            let out = dir.path().join(format!("{offset:#X}.bin"));
            let out_name = out.to_str().expect("a path in UTF-8");
            write_refused(&file, offset, &refused, out_name).expect("written");
            let held = &bytes[offset as usize..end as usize];
            assert_eq!(fs::read(&out).expect("read back"), held, "{offset:#X}");
        }
        // The bytes inside the first entry are no body, and nothing is written for them.
        let inside = ENTRY + 16;
        let out = dir.path().join("inside.bin");
        let refused = Rpf9ScanError::NoBodyAt { offset: inside };
        let out_name = out.to_str().expect("a path in UTF-8");
        assert_eq!(
            write_refused(&file, inside, &refused, out_name),
            Err(refused.to_string())
        );
        assert!(!out.exists());
    }

    #[test]
    fn a_span_past_the_file_s_end_is_refused_and_leaves_the_output_as_it_was() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("short.bin");
        fs::write(&path, [7u8; 0x40]).expect("written");
        let file = fs::File::open(&path).expect("opened");
        let out = dir.path().join("out.bin");
        assert_eq!(write_span(&file, 0x10, 0x30, &out), Ok(0x20));
        assert_eq!(fs::read(&out).expect("read back"), [7u8; 0x20]);
        let refused = write_span(&file, 0x10, 0x80, &out);
        assert!(refused.is_err_and(|e| e.contains("0x40")));
        assert_eq!(
            fs::read(&out).expect("kept"),
            [7u8; 0x20],
            "the earlier span is kept"
        );
        assert!(!dir.path().join("out.bin.part").exists());
        assert!(write_span(&file, 0x30, 0x10, &out).is_err());
    }
}
