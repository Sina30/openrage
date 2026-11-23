//! A field read through a declared view against the same read from a plain
//! struct: the spec says they are one load, and this is where that is held.
//!
//! The same layout, method and record count as `what_a_plan_read_costs`, so
//! the two tables compare. The timing is `#[ignore]`d and run by name in a
//! release build:
//!
//! ```text
//! cargo test --release -p or-io --test what_a_typed_read_costs -- --ignored --nocapture
//! ```

use std::hint::black_box;
use std::time::{Duration, Instant};

use or_io::decl::*;
use or_io::desc;

macro_rules! gauge {
    ($m:ident) => {
        $m! {
            f32: [f00 0x00, f01 0x04, f02 0x08, f03 0x0c, f04 0x10, f05 0x14, f06 0x18, f07 0x1c, f08 0x20, f09 0x24,
                  f10 0x28, f11 0x2c, f12 0x30, f13 0x34, f14 0x38, f15 0x3c, f16 0x40, f17 0x44, f18 0x48, f19 0x4c],
            i32: [signed 0x50],
            u32: [unsigned 0x54],
            u8: [b00 0x58, b01 0x59, b02 0x5a, b03 0x5b, b04 0x5c, b05 0x5d, b06 0x5e, b07 0x5f, b08 0x60, b09 0x61,
                 b10 0x62, b11 0x63, b12 0x64, b13 0x65, b14 0x66, b15 0x67, b16 0x68, b17 0x69, b18 0x6a]
        }
    };
}

macro_rules! declare {
    (f32: [$($f:ident $fo:literal),*], i32: [$($i:ident $io:literal),*], u32: [$($u:ident $uo:literal),*], u8: [$($b:ident $bo:literal),*]) => {
        /// The declared record.
        #[record(size = 0x6c)]
        pub struct Gauge {
            $(#[at($fo)] pub $f: f32,)*
            $(#[at($io)] pub $i: i32,)*
            $(#[at($uo)] pub $u: u32,)*
            $(#[at($bo)] pub $b: u8,)*
            _pad_0x6b: u8,
        }

        /// The baseline: an ordinary `repr(C)` struct with the same fields.
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct PlainGauge {
            $($f: f32,)*
            $($i: i32,)*
            $($u: u32,)*
            $($b: u8,)*
            _pad: u8,
        }

        #[inline(never)]
        fn plain_sum(gauges: &[PlainGauge]) -> f64 {
            let mut total = 0f64;
            for g in gauges {
                let mut f = 0f32;
                $(f += g.$f;)*
                let mut b = 0u32;
                $(b += u32::from(g.$b);)*
                let mut i = 0i64;
                $(i += i64::from(g.$i);)*
                $(i += i64::from(g.$u);)*
                total += f64::from(f) + f64::from(b) + i as f64;
            }
            total
        }

        /// The typed path as a consumer writes it: the run from its owner's accessor, then fields.
        #[inline(never)]
        fn typed_sum(img: desc::Image<'_>) -> f64 {
            let shelf = View::<Raw<Shelf>>::at_offset(img, 0).expect("the root");
            let gauges = shelf.gauges().expect("the run");
            let mut total = 0f64;
            for g in gauges.views() {
                let mut f = 0f32;
                $(f += g.$f;)*
                let mut b = 0u32;
                $(b += u32::from(g.$b);)*
                let mut i = 0i64;
                $(i += i64::from(g.$i);)*
                $(i += i64::from(g.$u);)*
                total += f64::from(f) + f64::from(b) + i as f64;
            }
            total
        }

        /// Every record checked for room and alignment on its own, the costliest typed path.
        #[inline(never)]
        fn per_view_sum(img: desc::Image<'_>, first: usize, count: usize) -> f64 {
            let mut total = 0f64;
            for n in 0..count {
                let Ok(g) = View::<Raw<Gauge>>::at_offset(img, first + n * std::mem::size_of::<Raw<Gauge>>()) else {
                    continue;
                };
                let mut f = 0f32;
                $(f += g.$f;)*
                let mut b = 0u32;
                $(b += u32::from(g.$b);)*
                let mut i = 0i64;
                $(i += i64::from(g.$i);)*
                $(i += i64::from(g.$u);)*
                total += f64::from(f) + f64::from(b) + i as f64;
            }
            total
        }

        const FIELDS: usize = [$(stringify!($f)),*].len() + [$(stringify!($b)),*].len() + 2;
        const F32_OFFSETS: &[usize] = &[$($fo),*];
    };
}

gauge!(declare);

#[record(size = 0x10)]
pub struct Shelf {
    #[len(field = "count")]
    pub gauges: Vec<Gauge>,
    count: u32,
    _pad_0x0c: u32,
}

const COUNT: usize = 1_006;
const FIRST: usize = 0x10;

/// The image: a shelf at 0 and its gauges after it, every field of every gauge different.
fn corpus() -> (Aligned, Vec<PlainGauge>) {
    let size = std::mem::size_of::<Raw<Gauge>>();
    let mut bytes = vec![0u8; FIRST + size * COUNT];
    let mut state = 0x9E37_79B9u32;
    for byte in bytes[FIRST..].iter_mut() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *byte = (state >> 11) as u8;
    }
    for n in 0..COUNT {
        for &offset in F32_OFFSETS {
            let at = FIRST + n * size + offset;
            let v = (n * 7 + offset) as f32 * 0.125;
            bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    bytes[0..8].copy_from_slice(&(0x5000_0000u64 + FIRST as u64).to_le_bytes());
    bytes[8..12].copy_from_slice(&(COUNT as u32).to_le_bytes());
    bytes[12..16].fill(0);
    let plain = (0..COUNT)
        .map(|n| {
            // SAFETY: `PlainGauge` is plain old data and the slice holds a whole one.
            unsafe {
                std::ptr::read_unaligned(bytes[FIRST + n * size..].as_ptr() as *const PlainGauge)
            }
        })
        .collect();
    (Aligned::from_bytes(&bytes), plain)
}

#[test]
fn a_typed_read_agrees_with_the_plain_struct() {
    assert_eq!(
        std::mem::size_of::<Raw<Gauge>>(),
        std::mem::size_of::<PlainGauge>()
    );
    let (bytes, plain) = corpus();
    let img = desc::Image::new(bytes.bytes(), bytes.bytes().len());
    assert_eq!(plain_sum(&plain).to_bits(), typed_sum(img).to_bits());
    assert_eq!(
        plain_sum(&plain).to_bits(),
        per_view_sum(img, FIRST, COUNT).to_bits()
    );
}

fn best_of<F: FnMut() -> f64>(blocks: usize, rounds: usize, mut f: F) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..blocks {
        let start = Instant::now();
        let mut sink = 0f64;
        for _ in 0..rounds {
            sink += f();
        }
        black_box(sink);
        best = best.min(start.elapsed());
    }
    best
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_typed_field_read_costs_against_a_plain_struct() {
    let (bytes, plain) = corpus();
    let img = desc::Image::new(bytes.bytes(), bytes.bytes().len());
    const ROUNDS: usize = 4_000;
    const RUNS: usize = 3;
    let fields = (COUNT * FIELDS * ROUNDS) as f64;
    let ns = |d: Duration| d.as_secs_f64() * 1e9 / fields;
    let (mut plains, mut typeds, mut per_views) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..RUNS {
        plains.push(ns(best_of(3, ROUNDS, || plain_sum(black_box(&plain)))));
        typeds.push(ns(best_of(3, ROUNDS, || typed_sum(black_box(img)))));
        per_views.push(ns(best_of(3, ROUNDS, || {
            per_view_sum(black_box(img), FIRST, COUNT)
        })));
    }
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: &[f64]| v.iter().copied().fold(0f64, f64::max);
    let (plain_ns, typed_ns, per_view_ns) = (min(&plains), min(&typeds), min(&per_views));
    // The spread is the plain baseline's own run-to-run range: noise, by definition.
    let spread = (max(&plains) - min(&plains)) / min(&plains);
    println!("{COUNT} records, {FIELDS} fields each, best of 3 blocks of {ROUNDS}, {RUNS} runs");
    println!(
        "  plain struct field              {plain_ns:.3} ns/field  1.00x  (runs {plains:.3?})"
    );
    println!(
        "  declared view, run from owner   {typed_ns:.3} ns/field  {:.2}x  (runs {typeds:.3?})",
        typed_ns / plain_ns
    );
    println!("  declared view, checked per view {per_view_ns:.3} ns/field  {:.2}x  (runs {per_views:.3?})", per_view_ns / plain_ns);
    println!("  baseline spread                 {:.1}%", spread * 100.0);
    let allowed = plain_ns * (1.0 + spread.max(0.05));
    assert!(
        typed_ns <= allowed,
        "a declared view's field read costs {typed_ns:.3} ns against a plain struct's {plain_ns:.3}; \
         the spread allows {allowed:.3}"
    );
}
