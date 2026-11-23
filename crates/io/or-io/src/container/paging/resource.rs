use crate::error::{IoError, Result};

use crate::desc::PagingModel;

pub use crate::constants::{
    is_physical_address, is_virtual_address, page_base, page_offset, FIXED_PHYSICAL_BASE,
    FIXED_VIRTUAL_BASE, NULL_PTR, PHYSICAL_PAGE_BASE, POINTER_SIZE, VIRTUAL_PAGE_BASE,
};

#[derive(Debug, Clone)]
pub struct DatResource<'a> {
    data: &'a [u8],

    virtual_size: usize,

    physical_size: usize,

    physical_offset: usize,

    total_size: usize,

    paging: PagingModel,
}

impl<'a> DatResource<'a> {
    pub fn new(data: &'a [u8], virtual_size: usize, physical_size: usize) -> Self {
        Self {
            data,
            virtual_size,
            physical_size,
            physical_offset: virtual_size,
            total_size: data.len(),
            paging: PagingModel::RAGE,
        }
    }

    pub fn with_paging(mut self, paging: PagingModel) -> Self {
        self.paging = paging;
        self
    }

    pub fn paging(&self) -> PagingModel {
        self.paging
    }

    pub fn resolve(&self, addr: u64) -> Result<Option<usize>> {
        if addr == NULL_PTR {
            return Ok(None);
        }

        let offset = self.paging.page_offset(addr);

        let file_offset = if self.paging.is_virtual(addr) {
            offset // Virtual page (0x50000000 - 0x5FFFFFFF by default)
        } else if self.paging.is_physical(addr) {
            self.physical_offset + offset // Physical page (0x60000000 - 0x6FFFFFFF)
        } else {
            return Err(IoError::InvalidPointerBase {
                base: self.paging.space_nibble(addr),
            });
        };

        if file_offset >= self.total_size {
            return Err(IoError::PointerOutOfBounds {
                offset: addr,
                size: self.total_size,
            });
        }

        Ok(Some(file_offset))
    }

    pub fn resolve_required(&self, addr: u64) -> Result<usize> {
        self.resolve(addr)?.ok_or(IoError::NullPointer)
    }

    pub fn read_bytes(&self, offset: usize, len: usize) -> Result<&'a [u8]> {
        self.check_bounds(offset, len)?;
        Ok(&self.data[offset..offset + len])
    }

    pub fn read_bytes_at(&self, addr: u64, len: usize) -> Result<&'a [u8]> {
        let offset = self.resolve_required(addr)?;
        self.read_bytes(offset, len)
    }

    pub fn read_u32(&self, offset: usize) -> Result<u32> {
        self.check_bounds(offset, 4)?;
        Ok(u32::from_le_bytes([
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
        ]))
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn virtual_size(&self) -> usize {
        self.virtual_size
    }

    pub fn physical_size(&self) -> usize {
        self.physical_size
    }

    pub fn physical_offset(&self) -> usize {
        self.physical_offset
    }

    pub fn total_size(&self) -> usize {
        self.total_size
    }

    #[inline]
    fn check_bounds(&self, offset: usize, size: usize) -> Result<()> {
        if offset + size > self.data.len() {
            return Err(IoError::PointerOutOfBounds {
                offset: offset as u64,
                size: self.total_size,
            });
        }
        Ok(())
    }
}

pub fn read_pointer(data: &[u8], offset: usize) -> Result<u64> {
    read_le(data, offset).map(u64::from_le_bytes)
}

pub fn read_u16_raw(data: &[u8], offset: usize) -> Result<u16> {
    read_le(data, offset).map(u16::from_le_bytes)
}

fn read_le<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    data.get(offset..)
        .and_then(|s| s.first_chunk::<N>())
        .copied()
        .ok_or(IoError::UnexpectedEof {
            offset: offset as u64,
            needed: N,
            available: data.len().saturating_sub(offset),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_virtual_pointer() {
        let data = vec![0u8; 256];
        let rsc = DatResource::new(&data, 128, 128);

        let offset = rsc.resolve(0x50000010).unwrap().unwrap();
        assert_eq!(offset, 0x10);
    }

    #[test]
    fn test_resolve_physical_pointer() {
        let data = vec![0u8; 256];
        let rsc = DatResource::new(&data, 128, 128);

        let offset = rsc.resolve(0x60000010).unwrap().unwrap();
        assert_eq!(offset, 128 + 0x10);
    }

    #[test]
    fn test_resolve_null_pointer() {
        let data = vec![0u8; 256];
        let rsc = DatResource::new(&data, 128, 128);

        let result = rsc.resolve(0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_required_null() {
        let data = vec![0u8; 256];
        let rsc = DatResource::new(&data, 128, 128);

        assert!(rsc.resolve_required(0).is_err());
    }

    #[test]
    fn test_read_primitives() {
        let mut data = vec![0u8; 256];
        data[4..8].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

        let rsc = DatResource::new(&data, 256, 256);

        assert_eq!(rsc.read_u32(4).unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn a_raw_read_past_the_end_is_unexpected_eof() {
        let data = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
        assert_eq!(read_pointer(&data, 1).unwrap(), 0x9988_7766_5544_3322);
        assert_eq!(read_u16_raw(&data, 7).unwrap(), 0x9988);
        for offset in [2, 8, 9, 10] {
            assert!(
                matches!(
                    read_pointer(&data, offset),
                    Err(IoError::UnexpectedEof { needed: 8, .. })
                ),
                "read_pointer at {offset}"
            );
        }
        for offset in [8, 9, 10] {
            assert!(
                matches!(
                    read_u16_raw(&data, offset),
                    Err(IoError::UnexpectedEof { needed: 2, .. })
                ),
                "read_u16_raw at {offset}"
            );
        }
    }

    #[test]
    fn a_raw_read_near_usize_max_is_unexpected_eof() {
        let data = [0u8; 9];
        for offset in [usize::MAX - 1, usize::MAX] {
            assert!(matches!(
                read_pointer(&data, offset),
                Err(IoError::UnexpectedEof { needed: 8, .. })
            ));
            assert!(matches!(
                read_u16_raw(&data, offset),
                Err(IoError::UnexpectedEof { needed: 2, .. })
            ));
        }
    }
}
