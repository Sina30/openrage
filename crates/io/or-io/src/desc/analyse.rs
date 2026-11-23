//! What a resource's bytes say about their own shape, with no description at all.
//!
//! A `u64` that [`Image::resolve`] accepts is a **candidate pointer**, so the
//! reference graph falls out of the bytes: every target starts an allocation,
//! the span to the next start is its extent, and a shared target is aliasing.
//!
//! ⚠ A span is not a stride. An allocation for one element is padded to the
//! allocator's rounding, so its span only bounds the element size: a pane
//! declared 0x70 measured 0x40, and it took a four-pane carrier to show it.
//! [`Extent`] names a stride only when a count of **two or more** divides the
//! span; `a_single_element_allocation_refuses_to_name_a_stride` holds it.

use std::collections::BTreeMap;

use crate::desc::image::Image;

/// A `u64` in the image that resolves to a valid offset.
///
/// ⚠ **Candidate, not pointer.** Any 8-byte window whose top nibble is 5 or 6 and
/// whose low bits land in bounds reads as one, and a count, a hash or a float can
/// do that by accident. [`Site::confidence`] is what a reader judges by; nothing
/// here claims certainty it does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    /// Where the candidate pointer itself lives.
    pub at: usize,
    /// Where it points.
    pub target: usize,
    pub space: Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    Virtual,
    Physical,
}

/// How much a candidate looks like a real pointer.
///
/// Ordered worst to best so a report can sort by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// Resolves, but to an unaligned offset. Real RAGE allocations are at least
    /// 8-byte aligned, so this is more likely a number that happened to fit.
    Weak,
    /// Resolves to an aligned offset.
    Plausible,
    /// Resolves to an aligned offset more than one candidate points at.
    Corroborated,
}

/// The extent of one allocation, and what can be said about its element stride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extent {
    /// A stride the count divided exactly.
    Stride {
        bytes: usize,
        /// The count that divided the span. ⚠ Always two or more; see
        /// [`StrideUnknown`](Extent::StrideUnknown).
        from_count: usize,
    },
    /// The span is known and the stride is not, with the reason.
    StrideUnknown { because: NoStride },
}

/// Why a stride could not be derived from a span: each is a refusal to guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoStride {
    /// One element. The span is padding plus the element, inseparably.
    SingleElement,
    /// No plausible count field was found beside the pointer.
    NoCount,
    /// A count was found and does not divide the span, so at least one of the
    /// two readings is wrong.
    CountDoesNotDivide { count: usize, span: usize },
}

impl std::fmt::Display for NoStride {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SingleElement => f.write_str(
                "one element, so the span is the element plus whatever the \
                 allocator rounded up to and the two cannot be separated",
            ),
            Self::NoCount => f.write_str("no count field beside the pointer"),
            Self::CountDoesNotDivide { count, span } => {
                write!(f, "a count of {count} does not divide a span of {span}")
            }
        }
    }
}

/// One allocation the graph found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub at: usize,
    /// Bytes to the next allocation start, or to the end of the region.
    ///
    /// ⚠ This is the **allocation's extent, including any padding**. It is not an
    /// element size and not a struct size.
    pub span: usize,
    /// How many candidate pointers target this offset. More than one is sharing,
    /// which a rebuild has to reproduce rather than duplicate.
    pub referrers: usize,
    pub extent: Extent,
    pub confidence: Confidence,
    pub space: Space,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    pub blocks: Vec<Block>,
    pub sites: Vec<Site>,
    /// Bytes before the first target: the root struct, which nothing points at.
    ///
    /// ⚠ There is no list of unreferenced gaps: spans reach the next target, so
    /// blocks tile the region by construction, and such a list reported one gap
    /// per block (70 for 71 on a real `.ynv`).
    pub root_bytes: usize,
    pub virtual_size: usize,
    pub physical_size: usize,
}

impl Layout {
    /// How many blocks name a stride, against how many refuse to.
    pub fn strides_known(&self) -> (usize, usize) {
        let known = self
            .blocks
            .iter()
            .filter(|b| matches!(b.extent, Extent::Stride { .. }))
            .count();
        (known, self.blocks.len())
    }

    /// Bytes inside a block whose stride was derived, against bytes in blocks.
    ///
    /// ⚠ Bytes, not blocks, because the two disagree: on a retail
    /// `navmesh[102][111].ynv` **13 strides of 71 blocks** reads as a fifth
    /// explained, and those thirteen are the large ones.
    pub fn explained(&self) -> (usize, usize) {
        let mut explained = 0usize;
        let mut total = 0usize;
        for block in &self.blocks {
            total += block.span;
            if matches!(block.extent, Extent::Stride { .. }) {
                explained += block.span;
            }
        }
        (explained, total)
    }
}

/// Every 8-aligned `u64` in `image` that resolves, as a candidate pointer.
fn sites_in(image: Image<'_>) -> Vec<Site> {
    let mut out = Vec::new();
    let len = image.len();
    let mut at = 0usize;
    while at + 8 <= len {
        if let Ok(addr) = image.pointer_at(at) {
            // ⚠ Zero is a null pointer, not a candidate targeting offset zero.
            // Counting it would make every null in the file reference the root.
            if addr != 0 {
                if let Ok(target) = image.resolve(addr) {
                    let space = if target < image.virtual_size() {
                        Space::Virtual
                    } else {
                        Space::Physical
                    };
                    out.push(Site { at, target, space });
                }
            }
        }
        at += 8;
    }
    out
}

/// A plausible element count beside the **pointer** at `site_at`.
///
/// ⚠ **Beside the pointer, not beside the target**: a count lives in the struct
/// that holds the pointer, and the target is a payload that knows nothing about
/// how many of itself there are.
///
/// ⚠ An `atArray` puts a `u16` count at `+8` and a `u16` capacity at `+10`;
/// other layouts put a `u32` at `+8`. Both are tried and a disagreement is no
/// count: picking whichever divides the span would publish a plausible wrong
/// stride.
fn count_beside(image: Image<'_>, site_at: usize, span: usize) -> Option<usize> {
    let u16_count = image
        .u16_at(site_at + 8)
        .ok()
        .map(usize::from)
        .filter(|n| *n > 0);
    let u32_count = image
        .slice_at(site_at + 8, 4)
        .ok()
        .map(|s| u32::from_le_bytes(s.try_into().expect("four bytes")) as usize)
        .filter(|n| *n > 0 && *n <= span.max(1));

    match (u16_count, u32_count) {
        // The two readings agree exactly when the high half is zero.
        (Some(a), Some(b)) if a == b => Some(a),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        _ => None,
    }
}

/// The count every pointer at `target` agrees on, if they agree.
///
/// ⚠ **Disagreement yields no count.** A shared allocation reached by two
/// pointers whose counts differ is either two different views of one run or one
/// misread field, and taking either number would be picking without evidence.
fn agreed_count(image: Image<'_>, sites: &[Site], target: usize, span: usize) -> Option<usize> {
    let mut agreed: Option<usize> = None;
    for site in sites.iter().filter(|s| s.target == target) {
        match (agreed, count_beside(image, site.at, span)) {
            (None, found) => agreed = found,
            (Some(a), Some(b)) if a == b => {}
            _ => return None,
        }
    }
    agreed
}

/// Walk `image` and report what its own pointers say about its shape.
pub fn layout_of(image: Image<'_>) -> Layout {
    let sites = sites_in(image);

    let mut referrers: BTreeMap<usize, usize> = BTreeMap::new();
    for site in &sites {
        *referrers.entry(site.target).or_default() += 1;
    }

    let mut starts: Vec<usize> = referrers.keys().copied().collect();
    starts.sort_unstable();

    let virtual_size = image.virtual_size();
    let total = image.len();

    let mut blocks = Vec::with_capacity(starts.len());
    for (i, &at) in starts.iter().enumerate() {
        let in_virtual = at < virtual_size;
        // ⚠ A span never crosses the virtual/physical boundary. The two regions
        // are separate allocations in the file, so a virtual block's span ends at
        // `virtual_size` however far the next target happens to be.
        let region_end = if in_virtual { virtual_size } else { total };
        let next = starts
            .get(i + 1)
            .copied()
            .filter(|n| *n <= region_end)
            .unwrap_or(region_end);
        let span = next.saturating_sub(at);

        let count = agreed_count(image, &sites, at, span);
        let extent = match count {
            Some(1) => Extent::StrideUnknown {
                because: NoStride::SingleElement,
            },
            Some(n) if span % n == 0 && span > 0 => Extent::Stride {
                bytes: span / n,
                from_count: n,
            },
            Some(n) => Extent::StrideUnknown {
                because: NoStride::CountDoesNotDivide { count: n, span },
            },
            None => Extent::StrideUnknown {
                because: NoStride::NoCount,
            },
        };

        let count_of_referrers = referrers.get(&at).copied().unwrap_or(0);
        let confidence = if at % 8 != 0 {
            Confidence::Weak
        } else if count_of_referrers > 1 {
            Confidence::Corroborated
        } else {
            Confidence::Plausible
        };

        blocks.push(Block {
            at,
            span,
            referrers: count_of_referrers,
            extent,
            confidence,
            space: if in_virtual {
                Space::Virtual
            } else {
                Space::Physical
            },
        });
    }

    Layout {
        root_bytes: starts.first().copied().unwrap_or(virtual_size),
        blocks,
        sites,
        virtual_size,
        physical_size: total.saturating_sub(virtual_size),
    }
}
