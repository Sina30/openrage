pub const FIXED_VIRTUAL_BASE: u64 = 0x50000000;

pub const FIXED_PHYSICAL_BASE: u64 = 0x60000000;

pub const NULL_PTR: u64 = 0;

pub const VIRTUAL_PAGE_BASE: u8 = (FIXED_VIRTUAL_BASE >> 28) as u8;

pub const PHYSICAL_PAGE_BASE: u8 = (FIXED_PHYSICAL_BASE >> 28) as u8;

pub const LOD_COUNT: usize = 4;

pub const HASH_SIZE: usize = std::mem::size_of::<u32>();

pub const POINTER_SIZE: usize = std::mem::size_of::<u64>();

pub const STRING_ALIGNMENT: usize = 4;

pub const ATL_ARRAY_MIN_ALIGN: usize = 16;

pub const TEXTURE_BACKING_STORE_ALIGN: usize = 256;

pub const DEFAULT_VFT: u64 = 0x140000000;

#[inline]
pub const fn is_virtual_address(addr: u64) -> bool {
    (addr >> 28) == (FIXED_VIRTUAL_BASE >> 28)
}

#[inline]
pub const fn is_physical_address(addr: u64) -> bool {
    (addr >> 28) == (FIXED_PHYSICAL_BASE >> 28)
}

#[inline]
pub const fn page_base(addr: u64) -> u8 {
    (addr >> 28) as u8
}

#[inline]
pub const fn page_offset(addr: u64) -> usize {
    (addr & 0x0FFFFFFF) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_address_detection() {
        assert!(is_virtual_address(FIXED_VIRTUAL_BASE));
        assert!(is_virtual_address(0x50001000));
        assert!(is_virtual_address(0x5FFFFFFF));
        assert!(!is_virtual_address(FIXED_PHYSICAL_BASE));
        assert!(!is_virtual_address(0x60001000));
        assert!(!is_virtual_address(NULL_PTR));
    }

    #[test]
    fn test_physical_address_detection() {
        assert!(is_physical_address(FIXED_PHYSICAL_BASE));
        assert!(is_physical_address(0x60001000));
        assert!(is_physical_address(0x6FFFFFFF));
        assert!(!is_physical_address(FIXED_VIRTUAL_BASE));
        assert!(!is_physical_address(0x50001000));
        assert!(!is_physical_address(NULL_PTR));
    }

    #[test]
    fn test_page_base_extraction() {
        assert_eq!(page_base(FIXED_VIRTUAL_BASE), VIRTUAL_PAGE_BASE);
        assert_eq!(page_base(FIXED_PHYSICAL_BASE), PHYSICAL_PAGE_BASE);
        assert_eq!(VIRTUAL_PAGE_BASE, 5);
        assert_eq!(PHYSICAL_PAGE_BASE, 6);
        assert_eq!(page_base(NULL_PTR), 0);
    }

    #[test]
    fn test_page_offset_extraction() {
        assert_eq!(page_offset(FIXED_VIRTUAL_BASE), 0);
        assert_eq!(page_offset(0x50001234), 0x1234);
        assert_eq!(page_offset(0x6FABCDEF), 0x0FABCDEF);
    }
}
