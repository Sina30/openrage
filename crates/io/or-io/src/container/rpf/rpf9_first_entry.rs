//! Where a first entry whose end its bytes do not state stops: at the next body a scan from the
//! file's start lists after it, or at the file's end where none follows.

use std::io::{Read, Seek};

use super::rpf9_scan::{Rpf9BodyKind, Rpf9Scan, Rpf9ScanError};

/// The end of the first entry of unstated end at `offset` of `source`; `None` where no scan lists one there.
// ⚠ It scans from the file's start, as the headers before the entry place it.
pub fn rpf9_first_entry_end<R: Read + Seek>(
    source: R,
    offset: u64,
) -> Result<Option<u64>, Rpf9ScanError> {
    let mut scan = Rpf9Scan::new(source, 0..u64::MAX)?;
    let file_len = scan.file_len();
    let mut entry = false;
    for find in scan.by_ref() {
        let find = find?;
        if find.offset > offset {
            return Ok(entry.then_some(find.offset));
        }
        entry = find.offset == offset
            && matches!(find.kind, Rpf9BodyKind::FirstEntry { .. })
            && find.stored_len.is_none();
    }
    Ok(entry.then_some(file_len))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

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

    #[test]
    fn a_first_entry_of_unstated_end_ends_at_the_next_body_or_the_file_s_end() {
        // Bytes no detector starts on: 0xA7 begins a DEFLATE block of the reserved type.
        let (entry, next) = (0x1B0u64, 0x200u64);
        let mut bytes = head(2, entry as u32);
        bytes.resize(next as usize, 0xA7);
        let alone = bytes.clone();
        bytes.extend(zlib(b"<?xml version=\"1.0\"?>\n<CMapTypes/>\n"));
        fn end(bytes: &[u8], at: u64) -> Option<u64> {
            rpf9_first_entry_end(Cursor::new(bytes), at).expect("a scan")
        }
        assert_eq!(end(&bytes, entry), Some(next));
        assert_eq!(end(&alone, entry), Some(alone.len() as u64));
        // Neither the body after it nor bytes inside it are a first entry.
        assert_eq!(end(&bytes, next), None);
        assert_eq!(end(&bytes, entry + 16), None);
        assert_eq!(end(&bytes, 0), None);
    }
}
