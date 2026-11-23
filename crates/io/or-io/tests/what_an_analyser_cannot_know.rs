//! An analyser must never report a stride from one instance.
//!
//! A census counts `.ynv` at 10,567 files, `.awc` at 7,742 and `.yvr` at 7,533,
//! all detected and unread, so a wrong stride would repeat at that scale.
//!
//! One instance says what a block contains and nothing about where it ends: a
//! single-element allocation is padded to whatever the allocator rounded to, so
//! its span is an upper bound on the element size. A pane declared at 0x70
//! measured 0x40, and only a four-pane carrier showed it. So
//! [`Extent::Stride`] exists only where a count of two or more divided the span,
//! and [`Extent::StrideUnknown`] carries which of three reasons it could not.
//!
//! Every fixture here is bytes built in the test. No corpus, no pack.

use or_io::desc::analyse::{layout_of, Confidence, Extent, NoStride, Space};
use or_io::desc::Image;

/// `0x50000000`, the virtual base every RAGE pointer is rebased against.
const VIRTUAL_BASE: u64 = 0x5000_0000;

/// A heap under construction.
struct Heap(Vec<u8>);

impl Heap {
    fn of(len: usize) -> Self {
        Heap(vec![0u8; len])
    }

    /// A rebased pointer at `at`, aimed at `target`.
    fn ptr(&mut self, at: usize, target: usize) -> &mut Self {
        let addr = VIRTUAL_BASE | target as u64;
        self.0[at..at + 8].copy_from_slice(&addr.to_le_bytes());
        self
    }

    fn u16(&mut self, at: usize, v: u16) -> &mut Self {
        self.0[at..at + 2].copy_from_slice(&v.to_le_bytes());
        self
    }

    fn image(&self) -> Image<'_> {
        // Everything virtual, which is what a system-page-only resource is.
        Image::new(&self.0, self.0.len())
    }
}

/// ⚠ One pane in an allocation the allocator rounded to 0x70. The true element
/// size is 0x40 and **this file cannot say so**, so the analyser must refuse.
#[test]
fn a_single_element_allocation_refuses_to_name_a_stride() {
    let mut heap = Heap::of(0x100);
    // A root pointing at one element, with a count of 1 beside the pointer.
    heap.ptr(0x00, 0x20).u16(0x08, 1);
    // A second allocation at 0x90, so the first one's span is 0x70.
    heap.ptr(0x10, 0x90).u16(0x18, 1);

    let layout = layout_of(heap.image());
    let first = layout
        .blocks
        .iter()
        .find(|b| b.at == 0x20)
        .expect("the first allocation");

    assert_eq!(first.span, 0x70, "the span is what the file measures");
    assert_eq!(
        first.extent,
        Extent::StrideUnknown {
            because: NoStride::SingleElement
        },
        "⚠ and the stride must be refused. Reporting 0x70 here is precisely the \
         error R11 had refuted by a four-pane carrier: one instance says what a \
         block contains and nothing about where it ends. If this ever names a \
         stride, the analyser has started publishing padding as element size, at \
         10,567 files a time."
    );

    let because = first.extent.clone();
    if let Extent::StrideUnknown { because } = because {
        assert!(
            because.to_string().contains("one element"),
            "the refusal must say why, got {because}"
        );
    }
}

/// Two or more elements, and the span divides.
#[test]
fn a_count_of_two_or_more_divides_the_span_into_a_stride() {
    let mut heap = Heap::of(0x200);
    // Four panes of 0x40 at 0x40, so 0x100 of span, and a count of 4.
    heap.ptr(0x00, 0x40).u16(0x08, 4);
    heap.ptr(0x10, 0x140).u16(0x18, 1);

    let layout = layout_of(heap.image());
    let run = layout
        .blocks
        .iter()
        .find(|b| b.at == 0x40)
        .expect("the run");

    assert_eq!(run.span, 0x100);
    assert_eq!(
        run.extent,
        Extent::Stride {
            bytes: 0x40,
            from_count: 4
        },
        "four panes across 0x100 is 0x40 each — which is the figure a four-pane \
         carrier is what gave R11, and the only kind of evidence that can"
    );
}

/// ⚠ A count that does not divide is reported as a disagreement, not rounded.
#[test]
fn a_count_that_does_not_divide_the_span_is_a_disagreement_rather_than_a_guess() {
    let mut heap = Heap::of(0x200);
    // A span of 0x30 and a count of 7: 0x30 / 7 is not an integer.
    heap.ptr(0x00, 0x20).u16(0x08, 7);
    heap.ptr(0x10, 0x50).u16(0x18, 1);

    let layout = layout_of(heap.image());
    let run = layout
        .blocks
        .iter()
        .find(|b| b.at == 0x20)
        .expect("the run");

    assert!(
        matches!(
            run.extent,
            Extent::StrideUnknown {
                because: NoStride::CountDoesNotDivide { .. }
            }
        ),
        "⚠ at least one of the two readings is wrong and the analyser cannot tell \
         which. Rounding to the nearest divisor would hide that a count field was \
         misidentified, got {:?}",
        run.extent
    );
}

/// No count beside the pointer, so no stride — and the span is still reported.
#[test]
fn a_pointer_with_no_count_beside_it_still_reports_its_span() {
    let mut heap = Heap::of(0x100);
    // A pointer to 0x40 with zeroes where a count would be.
    heap.ptr(0x00, 0x40);
    heap.ptr(0x10, 0x80);

    let layout = layout_of(heap.image());
    let block = layout.blocks.iter().find(|b| b.at == 0x40).expect("block");

    assert_eq!(block.span, 0x40, "the span is measurable regardless");
    assert_eq!(
        block.extent,
        Extent::StrideUnknown {
            because: NoStride::NoCount
        },
        "a single struct behind a pointer has no count, and inventing one from the \
         span would turn every struct into a one-element array"
    );
}

/// An `atArray`'s count-and-capacity pair reads as its count, on evidence.
///
/// An `atArray` puts a `u16` count at `+8` and a `u16` capacity at `+10`, so the
/// `u32` reading of that word is `capacity << 16 | count` — here `0x00080004`,
/// which is 524,292. Refusing the two readings as a disagreement is wrong: **a
/// count larger than the allocation it describes is impossible**, so discarding
/// it is evidence and not preference.
#[test]
fn a_count_larger_than_its_own_allocation_is_discarded_on_evidence() {
    let mut heap = Heap::of(0x200);
    heap.ptr(0x00, 0x40);
    // count = 4 at +8, capacity = 8 at +10, as an `atArray` writes it.
    heap.u16(0x08, 4).u16(0x0A, 8);
    heap.ptr(0x10, 0x140);

    let layout = layout_of(heap.image());
    let run = layout.blocks.iter().find(|b| b.at == 0x40).expect("run");

    assert_eq!(run.span, 0x100);
    assert_eq!(
        run.extent,
        Extent::Stride {
            bytes: 0x40,
            from_count: 4
        },
        "the `u32` reading is 524,292 elements in a 256-byte allocation, which is \
         refuted by the span rather than merely unattractive. Refusing here would \
         throw away every `atArray` in the game."
    );
}

/// ⚠ A shared allocation reached by two pointers whose count fields disagree is
/// either two different views of one run or one misread field, and the analyser
/// cannot tell. Taking either number is picking without evidence.
#[test]
fn two_pointers_disagreeing_about_a_shared_count_yield_no_count() {
    let mut heap = Heap::of(0x200);
    // Both point at 0x40. One says four elements, the other says two.
    heap.ptr(0x00, 0x40).u16(0x08, 4);
    heap.ptr(0x10, 0x40).u16(0x18, 2);
    heap.ptr(0x20, 0x140).u16(0x28, 1);

    let layout = layout_of(heap.image());
    let run = layout.blocks.iter().find(|b| b.at == 0x40).expect("run");

    assert_eq!(run.referrers, 2, "it is shared");
    assert_eq!(run.span, 0x100);
    assert_eq!(
        run.extent,
        Extent::StrideUnknown {
            because: NoStride::NoCount
        },
        "⚠ 0x100 divides by both 4 and 2, so whichever pointer the walk reached \
         first would decide whether the stride is 0x40 or 0x80. Order of traversal \
         is not evidence."
    );
}

/// Sharing is visible, and a rebuild has to reproduce it.
#[test]
fn two_pointers_at_one_target_is_reported_as_sharing() {
    let mut heap = Heap::of(0x100);
    // Both `last_matrices` and `current_matrices`, as `phBoundComposite` does.
    heap.ptr(0x00, 0x40).u16(0x08, 2);
    heap.ptr(0x10, 0x40).u16(0x18, 2);
    heap.ptr(0x20, 0xC0);

    let layout = layout_of(heap.image());
    let shared = layout.blocks.iter().find(|b| b.at == 0x40).expect("block");

    assert_eq!(
        shared.referrers, 2,
        "two pointers at one allocation is a fact about the file — measured on \
         `phBoundComposite`, where all 21 `.ybn` files in the corpus point \
         `last_matrices` and `current_matrices` at the same run. A rebuild that \
         does not reproduce it writes a second copy."
    );
    assert_eq!(
        shared.confidence,
        Confidence::Corroborated,
        "and being pointed at twice is what corroborates a candidate"
    );
}

/// ⚠ A candidate is not a pointer, and an unaligned target says so.
#[test]
fn an_unaligned_target_is_weak_rather_than_reported_as_a_pointer() {
    let mut heap = Heap::of(0x100);
    // A value that resolves but lands at an odd offset: real RAGE allocations are
    // at least 8-aligned, so this is more likely a number that happened to fit.
    heap.ptr(0x00, 0x33);
    heap.ptr(0x10, 0x80);

    let layout = layout_of(heap.image());
    let odd = layout.blocks.iter().find(|b| b.at == 0x33).expect("block");

    assert_eq!(
        odd.confidence,
        Confidence::Weak,
        "⚠ the analyser reports it and grades it down rather than discarding it. \
         Discarding would hide a genuinely packed layout; promoting it would fill \
         a report with arithmetic that looked like structure."
    );

    let aligned = layout.blocks.iter().find(|b| b.at == 0x80).expect("block");
    assert_eq!(aligned.confidence, Confidence::Plausible);
    assert!(
        odd.confidence < aligned.confidence,
        "confidence has to order, so a report can put the trustworthy rows first"
    );
}

/// ⚠ A null is not a pointer to offset zero.
#[test]
fn a_null_pointer_does_not_count_as_a_reference_to_the_root() {
    let mut heap = Heap::of(0x100);
    // 0x20 onwards is all zeroes, so eight-byte windows there read as 0.
    heap.ptr(0x00, 0x40);

    let layout = layout_of(heap.image());
    assert!(
        !layout.blocks.iter().any(|b| b.at == 0),
        "every null in the file would otherwise reference offset zero, and a root \
         struct with forty referrers reads as the most shared allocation in the \
         file rather than as the one thing nothing points at"
    );
    assert!(layout.blocks.iter().any(|b| b.at == 0x40));
}

/// ⚠ A span never crosses from system pages into graphics pages.
#[test]
fn a_virtual_blocks_span_stops_at_the_region_boundary() {
    let mut bytes = vec![0u8; 0x200];
    // A virtual pointer to 0x40, and a physical one to 0x180.
    bytes[0x00..0x08].copy_from_slice(&(VIRTUAL_BASE | 0x40).to_le_bytes());
    bytes[0x10..0x18].copy_from_slice(&(0x6000_0000u64 | 0x80).to_le_bytes());

    // 0x100 of system pages, then graphics pages.
    let image = Image::new(&bytes, 0x100);
    let layout = layout_of(image);

    let virtual_block = layout
        .blocks
        .iter()
        .find(|b| b.space == Space::Virtual)
        .expect("a virtual block");
    assert_eq!(
        virtual_block.span,
        0x100 - 0x40,
        "⚠ the span ends at the virtual/physical boundary. The two regions are \
         separate allocations in the file, so measuring across the seam would \
         report a system allocation that swallowed the graphics data."
    );

    assert_eq!(layout.virtual_size, 0x100);
    assert_eq!(layout.physical_size, 0x100);
}

/// The headline a report leads with is honest about coverage.
#[test]
fn a_layout_says_how_much_of_itself_it_could_not_explain() {
    let mut heap = Heap::of(0x200);
    heap.ptr(0x00, 0x40).u16(0x08, 4); // divides
    heap.ptr(0x10, 0x140).u16(0x18, 1); // single element, refused

    let layout = layout_of(heap.image());
    let (known, total) = layout.strides_known();

    assert_eq!(
        (known, total),
        (1, 2),
        "a layout that derived one stride from two blocks has described half the \
         file, and leading with that is the difference between a tool somebody \
         trusts and one they have to check"
    );
}

/// ⚠ Coverage is reported by bytes as well as by blocks, because the block count
/// flatters.
///
/// A list of unreferenced regions reports the arithmetic, not the file: spans
/// reach the next target, so blocks tile the region by construction, and on a
/// retail `navmesh[102][111].ynv` such a list held **70 runs for 71 blocks**.
/// `explained()` and `root_bytes` report coverage instead.
#[test]
fn coverage_is_reported_by_bytes_because_a_block_count_flatters() {
    let mut heap = Heap::of(0x400);
    // A large block whose stride divides, and a small one that refuses.
    heap.ptr(0x00, 0x40).u16(0x08, 4); // span 0x300, count 4 -> stride 0xC0
    heap.ptr(0x10, 0x340).u16(0x18, 1); // single element, refused

    let layout = layout_of(heap.image());

    let (known, total) = layout.strides_known();
    assert_eq!((known, total), (1, 2), "one block of two names a stride");

    let (explained, in_blocks) = layout.explained();
    assert_eq!(in_blocks, 0x400 - 0x40, "blocks tile from the first target");
    assert_eq!(
        explained, 0x300,
        "and the explained share is the large block's bytes, which is a different \
         and more useful number than one-of-two"
    );
    assert!(
        explained * 2 > in_blocks,
        "by bytes this file is mostly explained while by blocks it is half, and a \
         report leading with the flattering figure is one somebody stops checking"
    );

    assert_eq!(
        layout.root_bytes, 0x40,
        "the run before the first target is the root struct — nothing points at it \
         by definition, so it is named rather than counted as a gap"
    );
}
