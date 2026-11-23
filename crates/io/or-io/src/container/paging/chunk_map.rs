#[derive(Debug, Clone, Default)]
pub struct DatResourceSizeInfo {
    pub leaf_shift: u8,
    pub head16_count: u8,
    pub head8_count: u8,
    pub head4_count: u8,
    pub head2_count: u8,
    pub base_count: u8,
    pub has_tail2: bool,
    pub has_tail4: bool,
    pub has_tail8: bool,
    pub has_tail16: bool,
}

impl DatResourceSizeInfo {
    pub fn chunk_count(&self) -> u8 {
        self.head16_count
            + self.head8_count
            + self.head4_count
            + self.head2_count
            + self.base_count
            + self.has_tail2 as u8
            + self.has_tail4 as u8
            + self.has_tail8 as u8
            + self.has_tail16 as u8
    }

    pub fn total_size(&self, leaf_size: usize) -> usize {
        let base_chunk = leaf_size << self.leaf_shift;

        let main_size = ((self.head16_count as usize) << 4)
            + ((self.head8_count as usize) << 3)
            + ((self.head4_count as usize) << 2)
            + ((self.head2_count as usize) << 1)
            + (self.base_count as usize);

        (main_size * base_chunk)
            + (if self.has_tail2 { base_chunk >> 1 } else { 0 })
            + (if self.has_tail4 { base_chunk >> 2 } else { 0 })
            + (if self.has_tail8 { base_chunk >> 3 } else { 0 })
            + (if self.has_tail16 { base_chunk >> 4 } else { 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::resource::RSC_VIRTUAL_LEAF_SIZE;

    #[test]
    fn test_size_info_chunk_count() {
        let info = DatResourceSizeInfo {
            leaf_shift: 0,
            head16_count: 0,
            head8_count: 0,
            head4_count: 0,
            head2_count: 0,
            base_count: 1,
            has_tail2: false,
            has_tail4: false,
            has_tail8: false,
            has_tail16: false,
        };

        assert_eq!(info.chunk_count(), 1);

        let info2 = DatResourceSizeInfo {
            leaf_shift: 0,
            head16_count: 1,
            head8_count: 2,
            head4_count: 0,
            head2_count: 0,
            base_count: 1,
            has_tail2: true,
            has_tail4: false,
            has_tail8: false,
            has_tail16: false,
        };

        assert_eq!(info2.chunk_count(), 5);
    }

    #[test]
    fn test_size_info_total_size() {
        let info = DatResourceSizeInfo {
            leaf_shift: 0,
            head16_count: 0,
            head8_count: 0,
            head4_count: 0,
            head2_count: 0,
            base_count: 1,
            has_tail2: false,
            has_tail4: false,
            has_tail8: false,
            has_tail16: false,
        };

        assert_eq!(info.total_size(RSC_VIRTUAL_LEAF_SIZE as usize), 8192);
    }
}
