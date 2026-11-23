//! A resource whose last block, stored raw, runs on into the names of the nested archive that
//! holds it: no zero line parts them, so the block's end is where the names start.

use std::io::{Read, Seek};

use super::{
    names_find, Rpf9Block, Rpf9BodyKind, Rpf9Scan, Rpf9ScanError, Rpf9ScanFind, RPF9_SLOT,
};

impl<R: Read + Seek> Rpf9Scan<R> {
    /// `find` claimed, cut where the open archive's names start when its stored last block runs
    /// into them, with the names queued after it.
    pub(super) fn claim_before_names(
        &mut self,
        mut find: Rpf9ScanFind,
    ) -> Result<Rpf9ScanFind, Rpf9ScanError> {
        let names = self.names_inside(&mut find)?;
        let find = self.claim(find);
        if let Some(names) = names {
            let names = self.claim(names);
            self.queued.push_back(names);
        }
        Ok(find)
    }

    /// The open archive's names where the stored last block of `find` holds them: its last
    /// `size` bytes to the end of its last line pass the names' start tests and the next archive
    /// header follows them on the page strictly after their end. `find` is then cut to end before
    /// them.
    fn names_inside(
        &mut self,
        find: &mut Rpf9ScanFind,
    ) -> Result<Option<Rpf9ScanFind>, Rpf9ScanError> {
        let Some((archive, size, _)) = self.carry.archive else {
            return Ok(None);
        };
        let Some(stored) = find.stored_len else {
            return Ok(None);
        };
        let Rpf9BodyKind::Zlib(run) = &mut find.kind else {
            return Ok(None);
        };
        let Some(resource) = run.resource.as_mut() else {
            return Ok(None);
        };
        let Some(i) = resource.blocks.iter().rposition(Option::is_some) else {
            return Ok(None);
        };
        let Some(last) = resource.blocks[i].filter(|b| !b.zlib) else {
            return Ok(None);
        };
        let end = (find.offset + stored).next_multiple_of(RPF9_SLOT);
        let Some(start) = end.checked_sub(u64::from(size)) else {
            return Ok(None);
        };
        if start < last.offset || self.reader.names_after(start, size, false)? != Some(start) {
            return Ok(None);
        }
        // The block reads every byte before the names, its trailing zeros (map slots among them)
        // too, though as every stored block it states only the bytes to its last non-zero one.
        // A stored block is found within 128 KiB, well inside the reader's window.
        let kept = self
            .reader
            .get(last.offset, (start - last.offset) as usize)?;
        let len = kept
            .iter()
            .rposition(|b| *b != 0)
            .map_or(0, |k| k as u64 + 1);
        let inflated = (start - last.offset).next_multiple_of(RPF9_SLOT);
        let cut = last.inflated_len.saturating_sub(inflated);
        resource.blocks[i] = (inflated > 0).then_some(Rpf9Block {
            stored_len: len,
            inflated_len: inflated,
            ..last
        });
        if last.physical {
            resource.physical_len = resource.physical_len.saturating_sub(cut);
        } else {
            resource.virtual_len = resource.virtual_len.saturating_sub(cut);
        }
        if inflated == 0 {
            run.stored_pieces = run.stored_pieces.saturating_sub(1);
        }
        run.inflated = resource.virtual_len + resource.physical_len;
        let body_end = resource
            .blocks
            .iter()
            .flatten()
            .last()
            .map_or(find.offset, |b| b.offset + b.stored_len);
        find.stored_len = Some(body_end - find.offset);
        find.inflated_len = Some(run.inflated);
        find.span = body_end.next_multiple_of(RPF9_SLOT) - find.offset;
        Ok(Some(names_find(start, u64::from(size), archive)))
    }
}
