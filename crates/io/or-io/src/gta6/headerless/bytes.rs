//! The bounded reads every family's reader makes of a section.

use std::cell::Cell;

use super::{HeaderlessError, Result};

/// The two sections, every array read from either spent from one budget of their bytes, and the
/// read refused once that budget is spent: distinct arrays fit the sections once.
pub(super) fn sections<T>(
    virtual_section: &[u8],
    physical: Option<&[u8]>,
    read: impl FnOnce(Bytes<'_>, Bytes<'_>) -> Result<T>,
) -> Result<T> {
    let size = virtual_section.len() + physical.map_or(0, <[u8]>::len);
    let budget = Cell::new(Some(size));
    let read = read(
        Bytes {
            bytes: virtual_section,
            read: true,
            section: "virtual",
            budget: &budget,
        },
        Bytes {
            bytes: physical.unwrap_or_default(),
            read: physical.is_some(),
            section: "physical",
            budget: &budget,
        },
    );
    match budget.get() {
        Some(_) => read,
        None => Err(HeaderlessError::Aliased { size: size as u64 }),
    }
}

pub(super) fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub(super) fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from(u32_at(b, at)) | (u64::from(u32_at(b, at + 4)) << 32)
}

pub(super) fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(b, at))
}

/// One section's bytes, every read bounded.
///
/// ⚠ A word whose high half is zero and whose low half starts 0x5 addresses the virtual section,
/// one that starts 0x6 the physical; a GTA V reader run on these bodies reads plausible nonsense.
#[derive(Clone, Copy)]
pub(super) struct Bytes<'a> {
    bytes: &'a [u8],
    /// False for a physical section the block map states and that was not read.
    read: bool,
    section: &'static str,
    /// Array bytes both sections may still yield; `None` once a read asked for more.
    budget: &'a Cell<Option<usize>>,
}

impl<'a> Bytes<'a> {
    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) fn get(&self, what: &'static str, at: usize, len: usize) -> Result<&'a [u8]> {
        if !self.read {
            return Err(HeaderlessError::PhysicalNotRead {
                what,
                at: at as u64,
            });
        }
        at.checked_add(len)
            .and_then(|end| self.bytes.get(at..end))
            .ok_or(HeaderlessError::PastEnd {
                what,
                at: at as u64,
                section: self.section,
                len: self.bytes.len() as u64,
            })
    }

    pub(super) fn array(
        &self,
        what: &'static str,
        at: usize,
        count: usize,
        stride: usize,
    ) -> Result<&'a [u8]> {
        let len = count.checked_mul(stride).ok_or(HeaderlessError::PastEnd {
            what,
            at: at as u64,
            section: self.section,
            len: self.bytes.len() as u64,
        })?;
        let bytes = self.get(what, at, len)?;
        let left = self.budget.get().and_then(|left| left.checked_sub(len));
        self.budget.set(left);
        left.map(|_| bytes).ok_or(HeaderlessError::Aliased {
            size: self.bytes.len() as u64,
        })
    }

    pub(super) fn u8(&self, what: &'static str, at: usize) -> Result<u8> {
        Ok(self.get(what, at, 1)?[0])
    }

    pub(super) fn u16(&self, what: &'static str, at: usize) -> Result<u16> {
        let b = self.get(what, at, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(super) fn u32(&self, what: &'static str, at: usize) -> Result<u32> {
        Ok(u32_at(self.get(what, at, 4)?, 0))
    }

    pub(super) fn u64(&self, what: &'static str, at: usize) -> Result<u64> {
        Ok(u64_at(self.get(what, at, 8)?, 0))
    }

    pub(super) fn f32(&self, what: &'static str, at: usize) -> Result<f32> {
        Ok(f32_at(self.get(what, at, 4)?, 0))
    }

    pub(super) fn vec3(&self, what: &'static str, at: usize) -> Result<[f32; 3]> {
        let b = self.get(what, at, 12)?;
        Ok(std::array::from_fn(|a| f32_at(b, 4 * a)))
    }

    /// The virtual pointer at `at`, `None` when it is null.
    pub(super) fn ptr(&self, what: &'static str, at: usize) -> Result<Option<usize>> {
        let word = self.u64(what, at)?;
        if word == 0 {
            return Ok(None);
        }
        if word >> 32 != 0 || (word as u32) >> 28 != 5 {
            return Err(HeaderlessError::NotAPointer {
                what,
                at: at as u64,
            });
        }
        let target = (word & 0x0FFF_FFFF) as usize;
        self.get(what, target, 1)?;
        Ok(Some(target))
    }

    pub(super) fn to(&self, what: &'static str, at: usize) -> Result<usize> {
        self.ptr(what, at)?.ok_or(HeaderlessError::Null {
            what,
            at: at as u64,
        })
    }

    /// The physical-section offset the word at `at` addresses.
    pub(super) fn phys(&self, what: &'static str, at: usize) -> Result<usize> {
        let word = self.u64(what, at)?;
        if word >> 32 != 0 || (word as u32) >> 28 != 6 {
            return Err(HeaderlessError::NotAPointer {
                what,
                at: at as u64,
            });
        }
        Ok((word & 0x0FFF_FFFF) as usize)
    }

    pub(super) fn c_str(&self, what: &'static str, at: usize) -> Result<String> {
        let rest = self.get(what, at, 0).map(|_| &self.bytes[at..])?;
        let end = rest
            .iter()
            .position(|b| *b == 0)
            .ok_or(HeaderlessError::PastEnd {
                what,
                at: at as u64,
                section: self.section,
                len: self.bytes.len() as u64,
            })?;
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
}
