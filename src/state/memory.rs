//! What the app's long-lived structures hold on the CPU beyond its documents' bytes, for the
//! Statistics panel.
//!
//! ⚠ A structure is counted at the capacity of what it allocates, as `Heap::heap_bytes` counts an
//! owned value, and leaves out the allocator's padding, so a figure is the same on every machine.

use std::collections::HashSet;

use or_io::container::RpfArchive;

use super::{AppState, DocPayload};

/// Each long-lived structure at what it holds, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuState {
    pub name_index: usize,
    pub file_index: usize,
    pub search_results: usize,
    pub archive_tree: usize,
    pub archive_tables: usize,
    pub undo_history: usize,
    pub simulation: usize,
    pub dependency_index: usize,
}

/// A hash table's allocation, for the `capacity` it reports: a slot and a control byte for each of
/// its power-of-two slots, and one group of control bytes past them.
pub fn hash_table_bytes<T>(capacity: usize) -> usize {
    let slots = match capacity {
        0 => return 0,
        1..8 => capacity + 1,
        _ => capacity / 7 * 8,
    };
    slots * (std::mem::size_of::<T>() + 1) + CONTROL_GROUP
}

/// [`hash_table_bytes`] for a table of `len` entries inserted one at a time, where its crate gives no capacity.
pub fn hash_table_bytes_for_len<T>(len: usize) -> usize {
    let slots = match len {
        0 => return 0,
        1..4 => 4,
        4..8 => 8,
        _ => (len * 8 / 7).next_power_of_two(),
    };
    slots * (std::mem::size_of::<T>() + 1) + CONTROL_GROUP
}

/// A vector's allocation at its capacity, not counting what its elements hold.
pub fn vec_bytes<T>(items: &Vec<T>) -> usize {
    items.capacity() * std::mem::size_of::<T>()
}

/// The control bytes hashbrown reads at once on x86-64, which it lays past the slots.
const CONTROL_GROUP: usize = 16;

impl AppState {
    pub fn cpu_state(&self) -> CpuState {
        let mut seen: HashSet<*const RpfArchive> = HashSet::new();
        let (mut tree, mut tables, mut history) = (0, 0, 0);
        for doc in &self.documents {
            history += doc.history_bytes();
            if let DocPayload::Rpf(rpf) = &doc.payload {
                tree += rpf.session.tree_bytes();
                tables += rpf.session.table_bytes(&mut seen);
            }
        }
        CpuState {
            name_index: self.name_index.held_bytes(),
            file_index: self.file_index.held_bytes(),
            search_results: self.search.held_bytes() + self.deep_find.held_bytes(),
            archive_tree: tree,
            archive_tables: tables,
            undo_history: history,
            simulation: self.sim.held_bytes(),
            dependency_index: self.dependencies.held_bytes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_is_sized_alike_from_its_capacity_and_from_its_length() {
        let mut table = std::collections::HashMap::new();
        for len in 1..=4_000u64 {
            table.insert(len, len);
            assert_eq!(
                hash_table_bytes::<(u64, u64)>(table.capacity()),
                hash_table_bytes_for_len::<(u64, u64)>(len as usize),
                "{len} entries"
            );
        }
    }
}
