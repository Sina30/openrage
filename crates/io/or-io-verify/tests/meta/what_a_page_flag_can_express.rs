//! What an RSC7 flag word can say about a page, and what shipped RBM files
//! actually say — the two facts a synthesised allocation has to be built
//! against.
//!
//! ⚠ **A page size is not free.** `datResourceInfo::Sizes` packs a leaf shift
//! and five bucket counts into one word, so the reachable sizes are a lattice
//! rather than a range, and an allocator that sizes a payload to whatever
//! length the data came to produces a file `wrap_resource` refuses.
//!
//! ⚠ A multi-chunk page is several separate allocations at run time, so an
//! object that straddles a boundary has its pointer resolved against the wrong
//! chunk. Whether shipped content ever straddles is what says whether a
//! synthesiser may choose a multi-chunk shape.

use crate::corpus;
use crate::support;

use corpus::Corpus;
use or_io::container::resource::{DatResourceSizes, RSC_VIRTUAL_LEAF_SIZE};
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::parse_meta_binary_full;
use or_io::meta::{BinaryFormat, Diagnostics};

/// The chunk sizes a flag word describes, in the order the page concatenates
/// them.
///
/// ⚠ Its sum is asserted against [`DatResourceSizes::get_size`] below, so
/// chunk sizes that disagree with the size arithmetic cannot pass.
fn chunks_of(sizes: &DatResourceSizes, leaf: u32) -> Vec<u64> {
    let base = (leaf as u64) << (sizes.leaf_shift.min(31) as u64);
    let mut out = Vec::new();
    for _ in 0..sizes.head16_count {
        out.push(base * 16);
    }
    for _ in 0..sizes.head8_count {
        out.push(base * 8);
    }
    for _ in 0..sizes.head4_count {
        out.push(base * 4);
    }
    for _ in 0..sizes.head2_count {
        out.push(base * 2);
    }
    for _ in 0..sizes.base_count {
        out.push(base);
    }
    for (present, divisor) in [
        (sizes.has_tail2, 1u64),
        (sizes.has_tail4, 2),
        (sizes.has_tail8, 3),
        (sizes.has_tail16, 4),
    ] {
        if present {
            out.push(base >> divisor);
        }
    }
    out
}

fn virtual_flags_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?))
}

fn physical_flags_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?))
}

/// The five bucket counts filled greedily for a multiplier of `m` base
/// chunks, or `None` when `m` is past what they can sum to.
fn buckets_for(m: u32, shift: u8) -> Option<DatResourceSizes> {
    let mut sizes = DatResourceSizes {
        leaf_shift: shift,
        ..Default::default()
    };
    let mut left = m;
    for (cap, weight, slot) in [
        (1u32, 16u32, 0usize),
        (3, 8, 1),
        (15, 4, 2),
        (63, 2, 3),
        (127, 1, 4),
    ] {
        let take = (left / weight).min(cap);
        match slot {
            0 => sizes.head16_count = take as u8,
            1 => sizes.head8_count = take as u8,
            2 => sizes.head4_count = take as u8,
            3 => sizes.head2_count = take as u8,
            _ => sizes.base_count = take as u8,
        }
        left -= take * weight;
    }
    (left == 0).then_some(sizes)
}

/// Every page size the virtual flag word can name, at the granularity the
/// leaf size sets.
///
/// **An allocator only has to round a payload up to a multiple of the
/// 8,192-byte leaf**, because one leaf shift can name every such multiple up
/// to the bound the bitfield's own widths give.
#[test]
fn the_reachable_page_sizes_are_the_multiples_of_one_leaf() {
    // The largest main-chunk multiplier the five counts can sum to, in units
    // of one base chunk.
    let max_multiplier: u32 = 16 + 8 * 3 + 4 * 15 + 2 * 63 + 127;
    assert_eq!(max_multiplier, 353);
    assert!(
        buckets_for(max_multiplier, 4).is_some() && buckets_for(max_multiplier + 1, 4).is_none(),
        "the counts reach {max_multiplier} base chunks and no further"
    );

    // At leaf_shift 4 the four tail bits are all legal and their divisors are
    // 1/2, 1/4, 1/8 and 1/16 of a base chunk, so the tail expresses exactly k
    // sixteenths for every k in 0..16 — and one sixteenth of the base chunk
    // is one leaf.
    let shift = 4u8;
    assert_eq!(
        (u64::from(RSC_VIRTUAL_LEAF_SIZE) << shift) / 16,
        u64::from(RSC_VIRTUAL_LEAF_SIZE)
    );

    let mut unreachable = Vec::new();
    let leaves = max_multiplier * 16 + 15;
    for n in 1..=leaves {
        let want = u64::from(n) * u64::from(RSC_VIRTUAL_LEAF_SIZE);
        let k = n % 16;
        let Some(mut sizes) = buckets_for(n / 16, shift) else {
            unreachable.push((n, want, 0));
            continue;
        };
        sizes.has_tail2 = k & 8 != 0;
        sizes.has_tail4 = k & 4 != 0;
        sizes.has_tail8 = k & 2 != 0;
        sizes.has_tail16 = k & 1 != 0;
        assert!(
            sizes.validate_tail_chunks(),
            "leaf_shift {shift} allows all four tails"
        );
        if u64::from(sizes.get_size(RSC_VIRTUAL_LEAF_SIZE)) != want {
            unreachable.push((n, want, u64::from(sizes.get_size(RSC_VIRTUAL_LEAF_SIZE))));
            continue;
        }
        assert_eq!(
            u64::from(
                DatResourceSizes::from_flags(sizes.to_flags()).get_size(RSC_VIRTUAL_LEAF_SIZE)
            ),
            want,
            "{want} does not survive the round trip through a flag word"
        );
        assert_eq!(
            chunks_of(&sizes, RSC_VIRTUAL_LEAF_SIZE).iter().sum::<u64>(),
            want
        );
    }

    assert!(
        unreachable.is_empty(),
        "{} of {leaves} multiples of {RSC_VIRTUAL_LEAF_SIZE} are not expressible, first {:?}",
        unreachable.len(),
        unreachable.first()
    );
    println!(
        "every multiple of {RSC_VIRTUAL_LEAF_SIZE} from {RSC_VIRTUAL_LEAF_SIZE} to {} is \
         expressible at leaf_shift {shift}",
        u64::from(leaves) * u64::from(RSC_VIRTUAL_LEAF_SIZE)
    );
}

/// A page of one chunk is a page nothing can straddle, and the sizes that buys
/// are exactly the power-of-two multiples of a leaf.
#[test]
fn a_one_chunk_page_is_a_power_of_two_leaves() {
    let mut reachable = std::collections::BTreeSet::new();
    let mut saturated = 0usize;
    for shift in 0u8..16 {
        for sizes in [
            DatResourceSizes {
                leaf_shift: shift,
                base_count: 1,
                ..Default::default()
            },
            DatResourceSizes {
                leaf_shift: shift,
                head2_count: 1,
                ..Default::default()
            },
            DatResourceSizes {
                leaf_shift: shift,
                head4_count: 1,
                ..Default::default()
            },
            DatResourceSizes {
                leaf_shift: shift,
                head8_count: 1,
                ..Default::default()
            },
            DatResourceSizes {
                leaf_shift: shift,
                head16_count: 1,
                ..Default::default()
            },
        ] {
            assert_eq!(sizes.get_chunk_count(), 1);
            let size = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);
            // ⚠ `get_size` clamps to `u32::MAX`, so a shape past that reports
            // a size no page has. The clamp is the ceiling, not a rounding.
            if size != 0 && size != u32::MAX {
                reachable.insert(size);
            } else if size == u32::MAX {
                saturated += 1;
            }
        }
    }
    for size in &reachable {
        let leaves = size / RSC_VIRTUAL_LEAF_SIZE;
        assert_eq!(size % RSC_VIRTUAL_LEAF_SIZE, 0);
        assert!(
            leaves.is_power_of_two(),
            "{size} is one chunk but {leaves} leaves, which is not a power of two"
        );
    }
    assert!(reachable.contains(&RSC_VIRTUAL_LEAF_SIZE));
    assert!(
        saturated > 0,
        "nothing reached the clamp, so it went untested"
    );
    println!(
        "{} one-chunk page sizes, from {} to {}; {saturated} shapes are past what a u32 size \
         can report",
        reachable.len(),
        reachable.iter().next().copied().unwrap_or(0),
        reachable.iter().next_back().copied().unwrap_or(0)
    );
}

/// The header fields an invented RBM header has to choose a value for, and
/// what shipped content puts in them.
///
/// ⚠ A synthesiser has to write something into each, the `unknown_*` ones
/// included, and the corpus is the only evidence for what. Printed per field,
/// so one that turns out to vary shows up here rather than as a file the game
/// rejects.
#[test]
fn what_a_shipped_rbm_header_puts_in_the_fields_nobody_has_named() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "what_a_shipped_rbm_header_puts_in_the_fields_nobody_has_named",
            "no RBM fixtures present",
        );
        return;
    }
    let lib = support::schema();
    let mut seen: std::collections::BTreeMap<&'static str, std::collections::BTreeMap<u64, usize>> =
        std::collections::BTreeMap::new();
    let mut examined = 0usize;

    for fixture in &fixtures {
        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        examined += 1;
        let h = &rbm.header;
        for (name, value) in [
            ("vft", h.vft),
            ("block_map_ptr", h.block_map_ptr),
            ("unknown_14h", u64::from(h.unknown_14h)),
            ("has_encrypted_strings", u64::from(h.has_encrypted_strings)),
            ("unknown_17h", u64::from(h.unknown_17h)),
            ("unknown_18h", u64::from(h.unknown_18h)),
            ("root_block_id", u64::from(h.root_block_id)),
            ("name_ptr", h.name_ptr),
            ("encrypted_strings_ptr", h.encrypted_strings_ptr),
            ("unknown_4eh", u64::from(h.unknown_4eh)),
            ("unknown_50h[0]", u64::from(h.unknown_50h[0])),
            ("unknown_50h[7]", u64::from(h.unknown_50h[7])),
        ] {
            *seen.entry(name).or_default().entry(value).or_default() += 1;
        }
    }

    assert!(
        examined > 0,
        "no RBM fixture parsed, so nothing was measured"
    );
    for (name, values) in &seen {
        let mut top: Vec<_> = values.iter().collect();
        top.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        let shown: Vec<String> = top
            .iter()
            .take(3)
            .map(|(v, c)| format!("{v:#X}×{c}"))
            .collect();
        println!(
            "{name}: {} distinct over {examined} documents, {}",
            values.len(),
            shown.join(", ")
        );
    }
}

/// What `block_map_ptr` points at, and why it is derivable rather than
/// carried.
///
/// The 24 bytes it reaches are a page-count record: two byte counts at `+0x08`
/// and `+0x09` and zero everywhere else. **Those two counts are the chunk
/// counts the RSC7 flag words already state**, so a synthesiser derives them
/// from its own allocation.
#[test]
fn the_block_map_is_the_page_counts_the_flag_words_already_state() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "the_block_map_is_the_page_counts_the_flag_words_already_state",
            "no RBM fixtures present",
        );
        return;
    }
    let lib = support::schema();
    let mut examined = 0usize;
    let mut disagreed = Vec::new();
    let mut nonzero_elsewhere = Vec::new();

    for fixture in &fixtures {
        let Some(vflags) = virtual_flags_of(&fixture.bytes) else {
            continue;
        };
        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        let at = (rbm.header.block_map_ptr & 0x0FFF_FFFF) as usize;
        let Some(record) = rbm.decompressed.get(at..at + 0x18) else {
            continue;
        };
        examined += 1;

        let want = DatResourceSizes::from_flags(vflags).get_chunk_count();
        if u32::from(record[0x08]) != want {
            disagreed.push(format!(
                "{}: block map says {} virtual chunks, the flag word says {want}",
                fixture.name, record[0x08]
            ));
        }
        if record[0x09] != 0 {
            disagreed.push(format!(
                "{}: block map says {} physical chunks and the file declares no physical page",
                fixture.name, record[0x09]
            ));
        }
        if record
            .iter()
            .enumerate()
            .any(|(i, b)| i != 0x08 && i != 0x09 && *b != 0)
        {
            nonzero_elsewhere.push(fixture.name.clone());
        }
    }

    println!(
        "{examined} of {} RBM fixtures carry a readable block map; {} disagree with the flag \
         word, {} put something other than zero outside the two count bytes",
        fixtures.len(),
        disagreed.len(),
        nonzero_elsewhere.len()
    );
    assert!(
        examined > 0,
        "no fixture's block map was readable, so this run asserted nothing"
    );
    assert!(
        disagreed.is_empty(),
        "{} of {examined} block maps do not state the chunk counts the flag word does, so a \
         synthesiser cannot derive one:\n  {}",
        disagreed.len(),
        disagreed
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        nonzero_elsewhere.is_empty(),
        "{} of {examined} block maps hold something this model does not account for, first {:?}",
        nonzero_elsewhere.len(),
        nonzero_elsewhere.first()
    );
}

/// ⚠ **A leaf is the floor, and nothing finer exists.** A length that is not
/// a multiple of one leaf is not a page size at any shift, which is why the
/// synthesiser rounds up rather than sizing to the data.
#[test]
fn nothing_between_two_leaves_is_a_page_size() {
    let mut expressible = std::collections::BTreeSet::new();
    for shift in 0u8..16 {
        // One chunk of each class is enough to show the granularity: every
        // other reachable size at this shift is a sum of these.
        for sizes in [
            DatResourceSizes {
                leaf_shift: shift,
                base_count: 1,
                ..Default::default()
            },
            DatResourceSizes {
                leaf_shift: shift,
                has_tail16: shift > 3,
                base_count: u8::from(shift <= 3),
                ..Default::default()
            },
        ] {
            if !sizes.validate_tail_chunks() {
                continue;
            }
            let size = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);
            if size != 0 {
                expressible.insert(size);
            }
        }
    }
    let smallest = *expressible.iter().next().expect("some size is reachable");
    assert_eq!(
        smallest, RSC_VIRTUAL_LEAF_SIZE,
        "the smallest page a flag word can name is one leaf"
    );
    for size in &expressible {
        assert_eq!(
            size % RSC_VIRTUAL_LEAF_SIZE,
            0,
            "{size} is reachable but is not a whole number of leaves"
        );
    }
}

/// The shapes shipped RBM files actually use, and whether any of them puts a
/// data block across a chunk boundary: the measurement the one-chunk
/// allocator is designed against.
#[test]
fn what_shipped_rbm_pages_are_shaped_like() {
    let fixtures = Corpus::of_format(BinaryFormat::Rbm);
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "what_shipped_rbm_pages_are_shaped_like",
            "no RBM fixtures present",
        );
        return;
    }

    let lib = support::schema();
    let mut examined = 0usize;
    let mut single_chunk = 0usize;
    let mut multi_chunk = 0usize;
    let mut with_physical = 0usize;
    let mut straddling = Vec::new();
    let mut regions_checked = 0usize;
    let mut shipped_bytes = 0u64;
    let mut one_chunk_bytes = 0u64;
    let mut already_a_power_of_two = 0usize;
    let mut physical_nibbles = std::collections::BTreeMap::<u8, usize>::new();

    for fixture in &fixtures {
        let (Some(vflags), Some(pflags)) = (
            virtual_flags_of(&fixture.bytes),
            physical_flags_of(&fixture.bytes),
        ) else {
            continue;
        };
        *physical_nibbles
            .entry(((pflags >> 28) & 0xF) as u8)
            .or_default() += 1;
        if DatResourceSizes::from_flags(pflags).get_size(RSC_VIRTUAL_LEAF_SIZE) != 0 {
            with_physical += 1;
        }

        let sizes = DatResourceSizes::from_flags(vflags);
        let chunks = chunks_of(&sizes, RSC_VIRTUAL_LEAF_SIZE);
        if chunks.len() <= 1 {
            single_chunk += 1;
        } else {
            multi_chunk += 1;
        }

        let mut diags = Diagnostics::lenient();
        let Ok((_, layout)) = parse_meta_binary_full(lib, &fixture.bytes, &mut diags) else {
            continue;
        };
        let ContainerLayout::Rbm(rbm) = &layout.container else {
            continue;
        };
        examined += 1;

        let mut boundaries = Vec::new();
        let mut at = 0u64;
        for c in &chunks {
            at += c;
            boundaries.push(at);
        }
        boundaries.pop();

        // Every region the page holds that this crate can locate: the four
        // tables the header names, and each data block's payload.
        let h = &rbm.header;
        let mut regions: Vec<(String, u64, u64)> = vec![
            ("header".into(), 0, 112),
            (
                "class table".into(),
                h.class_refs_ptr,
                u64::from(h.class_count) * 0x20,
            ),
            (
                "enum table".into(),
                h.enum_refs_ptr,
                u64::from(h.enum_count) * 0x18,
            ),
            (
                "block table".into(),
                h.data_blocks_ptr,
                u64::from(h.data_block_count) * 0x10,
            ),
        ];
        for (i, block) in rbm.model.data_blocks.iter().enumerate() {
            regions.push((
                format!("block {i}"),
                block.data_addr,
                block.data.len() as u64,
            ));
        }
        regions_checked += regions.len();

        for (what, addr, len) in regions {
            if len == 0 || addr == 0 {
                continue;
            }
            let start = addr & 0x0FFF_FFFF;
            let end = start + len;
            if let Some(b) = boundaries.iter().find(|b| start < **b && end > **b) {
                straddling.push(format!(
                    "{}: {what} spans {start:#X}..{end:#X} across the boundary at {b:#X}",
                    fixture.name
                ));
            }
        }

        // What a one-chunk page would have cost this document.
        let actual = u64::from(sizes.get_size(RSC_VIRTUAL_LEAF_SIZE));
        let leaves = actual.div_ceil(u64::from(RSC_VIRTUAL_LEAF_SIZE)).max(1);
        let rounded = leaves.next_power_of_two() * u64::from(RSC_VIRTUAL_LEAF_SIZE);
        shipped_bytes += actual;
        one_chunk_bytes += rounded;
        if rounded == actual {
            already_a_power_of_two += 1;
        }
    }

    println!(
        "{examined} of {} RBM fixtures parsed; {single_chunk} declare one virtual chunk, \
         {multi_chunk} declare more than one, {with_physical} declare a physical page",
        fixtures.len()
    );
    println!("physical version nibbles: {physical_nibbles:?}");
    println!(
        "{} of {regions_checked} locatable regions straddle a chunk boundary",
        straddling.len()
    );
    for line in straddling.iter().take(8) {
        println!("  {line}");
    }
    println!(
        "a one-chunk page would cost {one_chunk_bytes} bytes where the shipped shapes cost \
         {shipped_bytes}; {already_a_power_of_two} of {examined} shipped pages are already a \
         power-of-two number of leaves"
    );

    assert!(
        examined > 0,
        "{} RBM fixtures are staged and none parsed, so this run measured nothing",
        fixtures.len()
    );
    assert!(
        regions_checked > examined,
        "{regions_checked} regions across {examined} documents is fewer than one apiece, so \
         the straddle question went unasked"
    );
    assert!(
        multi_chunk > 0,
        "no shipped page declares more than one chunk, so the straddle question is vacuous \
         and the allocator's justification is not this measurement"
    );
    assert!(
        straddling.is_empty(),
        "{} regions straddle a chunk boundary, which contradicts the one-chunk rule the \
         synthesiser is built on:\n  {}",
        straddling.len(),
        straddling
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
