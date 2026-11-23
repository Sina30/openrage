//! Build a resource under a second address model and read it back under it.
//!
//! The write-half twin of `a_second_paging_model.rs`.
//!
//! ⚠ `OTHER` moves both bases **and widens the mask to twenty bits**. A model
//! that only moved the two nibbles would still pass under an implementation
//! that had hard-coded `addr >> 28`, because the shifted values differ per
//! space either way.
//!
//! ⚠ Every assertion is `DatResource` resolving an address the writer produced.
//! A writer checked against its own arithmetic would pass with the model
//! ignored: the addresses would be self-consistent and belong to the wrong
//! space.

use std::path::{Path, PathBuf};

use or_io::container::paging::{
    DatResource, PgRscBuilder, ResourceHeap, FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE,
};
use or_io::desc::PagingModel;

const OTHER: PagingModel = PagingModel {
    virtual_base: 0x0010_0000,
    physical_base: 0x0020_0000,
    offset_mask: 0x000F_FFFF,
};

struct Built {
    data: Vec<u8>,
    virtual_size: usize,
    physical_size: usize,
    virtual_pointer: u64,
    physical_pointer: u64,
}

fn build(paging: PagingModel) -> Built {
    let mut heap = ResourceHeap::new().with_paging(paging);

    let root = heap.alloc_virtual(0x10);
    assert_eq!(
        root, paging.virtual_base,
        "the first virtual allocation sits at the model's base"
    );

    let virtual_pointer = heap.write_bytes(&0xABCD_1234u32.to_le_bytes());
    let physical_pointer = heap.write_physical_bytes(&0x5566_7788u32.to_le_bytes());

    heap.write_u64_at(0x00, virtual_pointer).expect("slot 0");
    heap.write_u64_at(0x08, physical_pointer).expect("slot 1");

    let (virtual_data, physical_data) = heap.finish();
    let (virtual_size, physical_size) = (virtual_data.len(), physical_data.len());

    let mut data = virtual_data;
    data.extend_from_slice(&physical_data);

    Built {
        data,
        virtual_size,
        physical_size,
        virtual_pointer,
        physical_pointer,
    }
}

#[test]
fn rage_is_the_default_and_nothing_had_to_ask_for_it() {
    let built = build(PagingModel::RAGE);

    assert_eq!(built.virtual_pointer, FIXED_VIRTUAL_BASE + 0x10);
    assert_eq!(built.physical_pointer, FIXED_PHYSICAL_BASE);

    let default = build(ResourceHeap::new().paging());
    assert_eq!(default.data, built.data, "the default model is RAGE's");
}

#[test]
fn the_round_trip_closes_under_a_second_model() {
    let built = build(OTHER);

    assert_eq!(built.virtual_pointer, OTHER.virtual_base + 0x10);
    assert_eq!(built.physical_pointer, OTHER.physical_base);

    let rsc =
        DatResource::new(&built.data, built.virtual_size, built.physical_size).with_paging(OTHER);

    assert_eq!(
        rsc.resolve(built.virtual_pointer).expect("resolves"),
        Some(0x10)
    );
    assert_eq!(
        rsc.resolve(built.physical_pointer).expect("resolves"),
        Some(built.virtual_size),
        "a physical address lands past the system half"
    );
    assert_eq!(
        rsc.read_u32(rsc.resolve_required(built.virtual_pointer).unwrap())
            .unwrap(),
        0xABCD_1234,
        "and the value is reachable, not merely the offset plausible"
    );
}

/// What the writer produced under one model does not read under the other.
///
/// Without it `the_round_trip_closes_under_a_second_model` would pass unchanged
/// under a `with_paging` that did nothing, because RAGE's own addresses resolve
/// under RAGE.
#[test]
fn what_one_model_built_the_other_refuses() {
    let under_other = build(OTHER);
    let under_rage = build(PagingModel::RAGE);

    assert_ne!(
        under_other.data, under_rage.data,
        "the two models produced the same bytes, so `with_paging` reached nothing"
    );

    let as_rage = DatResource::new(
        &under_other.data,
        under_other.virtual_size,
        under_other.physical_size,
    );
    assert!(
        as_rage.resolve(under_other.virtual_pointer).is_err(),
        "{:#x} resolved under RAGE, which it does not belong to",
        under_other.virtual_pointer
    );

    let as_other = DatResource::new(
        &under_rage.data,
        under_rage.virtual_size,
        under_rage.physical_size,
    )
    .with_paging(OTHER);
    assert!(
        as_other.resolve(under_rage.virtual_pointer).is_err(),
        "{:#x} resolved under a model it does not belong to",
        under_rage.virtual_pointer
    );
}

/// The bucketed builder takes the model too, not only the heap it wraps.
///
/// ⚠ A model threaded into `ResourceHeap` and not into `PgRscBuilder` would
/// pass every test above, because they all hold the heap directly. The
/// application holds the builder.
#[test]
fn the_bucketed_builder_carries_it_into_both_halves() {
    let mut builder = PgRscBuilder::new_bucketed(0x1000, 0x1000).with_paging(OTHER);
    assert_eq!(builder.paging(), OTHER);

    let address = builder.heap_mut().write_bytes(&[1, 2, 3, 4]);
    assert_eq!(
        address, OTHER.virtual_base,
        "the builder's heap invents addresses under the model it was given"
    );

    let (virtual_chunks, _) = builder.ready_build();
    assert!(virtual_chunks > 0, "the bucket packer still runs");
}

/// ⚠ Of `ResourceHeap`, `SinglePassAllocator` and `chunk_map`, only the heap
/// invents an address that reaches a file:
///
/// - `chunk_map` holds **bucket counts and no address**. The RSC7 header
///   carries those counts and the game rebuilds the chunk map from them, so
///   nothing the writer emits holds a chunk's address.
/// - `SinglePassAllocator::allocate` returns an address and **both production
///   call sites discard it** — `ready_build` allocates once for each heap's
///   whole length to get bucket counts.
#[test]
fn only_the_heap_invents_an_address_that_reaches_a_file() {
    const BASES: &[&str] = &["FIXED_VIRTUAL_BASE", "FIXED_PHYSICAL_BASE"];

    let paging = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/container/paging");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&paging)
        .expect("or-io has a paging module")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    assert!(files.len() > 3, "the walk found {} file(s)", files.len());

    let mut naming: Vec<(String, usize)> = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path).expect("read");
        let hits = production_hits(&source, BASES);
        if hits > 0 {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            naming.push((name.into_owned(), hits));
        }
    }

    eprintln!("\n=== what still names a paging base in `paging/`, outside a test module ===");
    for (name, hits) in &naming {
        eprintln!("  {name}: {hits}");
    }

    let heap = naming.iter().find(|(name, _)| name == "heap.rs");
    assert!(
        heap.is_none(),
        "`heap.rs` names a base constant again ({:?}). It is the one type whose \
         addresses reach a file, so it is the one that must take them from its \
         model and nowhere else.",
        heap
    );

    // `allocator.rs` and `chunk_map.rs` are the two the doc comment above
    // accounts for; `mod.rs` and `resource.rs` re-export rather than compute.
    const RULED: &[&str] = &["allocator.rs", "chunk_map.rs", "mod.rs", "resource.rs"];
    let unexpected: Vec<&(String, usize)> = naming
        .iter()
        .filter(|(name, _)| !RULED.contains(&name.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "a paging file names a base constant and this test has no ruling on it: \
         {unexpected:?}.\n\n\
         Either it invents an address, in which case it wants the model — \
         `ResourceHeap::virtual_offset_of` is the inverse — or it re-exports one, which `mod.rs` and `resource.rs` \
         do. `at_map.rs` and `dictionary.rs` were the first answer and are why \
         the inverse is a method rather than a subtraction at each call site."
    );
}

fn production_hits(source: &str, needles: &[&str]) -> usize {
    let mut hits = 0usize;
    let mut in_tests = false;
    let mut depth: i32 = 0;
    let mut armed = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if !in_tests {
            if trimmed.starts_with("#[cfg(test)]") {
                armed = true;
            } else if armed && trimmed.starts_with("mod ") {
                in_tests = true;
                armed = false;
                depth = braces(line);
                continue;
            } else if armed && !trimmed.is_empty() && !trimmed.starts_with('#') {
                armed = false;
            }
        }

        let code = match trimmed.find("//") {
            Some(at) => &trimmed[..at],
            None => trimmed,
        };
        let hit = needles.iter().any(|n| code.contains(n));

        if in_tests {
            depth += braces(line);
            if depth <= 0 {
                in_tests = false;
            }
        } else if hit {
            hits += 1;
        }
    }
    hits
}

fn braces(line: &str) -> i32 {
    line.chars().filter(|c| *c == '{').count() as i32
        - line.chars().filter(|c| *c == '}').count() as i32
}
