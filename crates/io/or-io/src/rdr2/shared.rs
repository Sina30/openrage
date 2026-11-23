//! Declarations more than one RDR2 format uses.
//!
//! These lay out as the engine's classes of the same names do in GTA V, and are declared again here
//! rather than reached into `gta5`, so that neither title's declarations depend on the other's.

use crate::decl::*;

/// `pgBase`: the vtable, and the defragmenter's list head, a runtime pointer.
#[record(size = 0x10)]
pub struct PgBase {
    vft: u64,
    first_node: Ptr<Opaque>,
}

/// `pgDictionary<T>`: entries keyed by a 32-bit name hash, in two runs of one length.
///
/// ⚠ `codes` is ascending and a lookup binary-searches it. Nothing here writes a dictionary yet, so
/// nothing keeps that order for an edit.
#[record(size = 0x40)]
pub struct PgDictionary<T> {
    pub base: PgBase,
    parent: Ptr<Opaque>,
    #[pad_after(4)]
    #[init(1)]
    ref_count: i32,
    pub codes: Vec<u32>,
    #[keyed_by(codes)]
    pub entries: Vec<Option<Box<T>>>,
}
