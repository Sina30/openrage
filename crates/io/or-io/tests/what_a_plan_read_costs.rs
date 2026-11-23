//! A plan read against a typed struct of the same layout, on a synthetic pack.
//!
//! Invented type and field names at invented offsets, so it runs in a clean
//! checkout. The timing is `#[ignore]`d and run by name in a release build:
//!
//! ```text
//! cargo test --release -p or-io --test what_a_plan_read_costs -- --ignored --nocapture
//! ```

use std::hint::black_box;
use std::time::{Duration, Instant};

use or_io::at;
use or_io::desc::{
    FieldRepr, IntWidth, Pack, PackField, PackType, PlanError, Scalar, Slot, TypePlan,
};

macro_rules! widget {
    ($m:ident) => {
        $m! {
            f32: [f00, f01, f02, f03, f04, f05, f06, f07, f08, f09,
                  f10, f11, f12, f13, f14, f15, f16, f17, f18, f19],
            u8: [b00, b01, b02, b03, b04, b05, b06, b07, b08, b09,
                 b10, b11, b12, b13, b14, b15, b16, b17, b18],
            i32: [signed],
            u32: [unsigned]
        }
    };
}

macro_rules! declare {
    (f32: [$($f:ident),*], u8: [$($b:ident),*], i32: [$($i:ident),*], u32: [$($u:ident),*]) => {
        /// The typed baseline: an ordinary `repr(C)` struct with the synthetic layout.
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Widget {
            $($f: f32,)*
            $($i: i32,)*
            $($u: u32,)*
            $($b: u8,)*
        }

        fn widget_pack() -> Pack {
            let mut fields = Vec::new();
            $(fields.push(field(stringify!($f), std::mem::offset_of!(Widget, $f), Scalar::F32));)*
            $(fields.push(field(stringify!($i), std::mem::offset_of!(Widget, $i), Scalar::Int(IntWidth::I32)));)*
            $(fields.push(field(stringify!($u), std::mem::offset_of!(Widget, $u), Scalar::Int(IntWidth::U32)));)*
            $(fields.push(field(stringify!($b), std::mem::offset_of!(Widget, $b), Scalar::Int(IntWidth::U8)));)*
            let mut pack = Pack::new("synthetic", "bench");
            pack.types.push(PackType {
                name: "Widget".into(),
                size: std::mem::size_of::<Widget>(),
                fields,
                selector: None,
            });
            pack
        }

        struct Slots {
            f32s: Vec<Slot<f32>>,
            u8s: Vec<Slot<u8>>,
            i32s: Vec<Slot<i32>>,
            u32s: Vec<Slot<u32>>,
        }

        fn slots(plan: &TypePlan) -> Result<Slots, PlanError> {
            Ok(Slots {
                f32s: vec![$(at!(plan, f32, stringify!($f))?),*],
                u8s: vec![$(at!(plan, u8, stringify!($b))?),*],
                i32s: vec![$(at!(plan, i32, stringify!($i))?),*],
                u32s: vec![$(at!(plan, u32, stringify!($u))?),*],
            })
        }

        /// One pass the way a consumer writes it: each field a local, resolved at its own call site.
        #[inline(never)]
        fn pass_sum(plan: &TypePlan, bytes: &[u8], count: usize) -> Result<f64, PlanError> {
            $(let $f = at!(plan, f32, stringify!($f))?;)*
            $(let $b = at!(plan, u8, stringify!($b))?;)*
            $(let $i = at!(plan, i32, stringify!($i))?;)*
            $(let $u = at!(plan, u32, stringify!($u))?;)*
            let size = plan.size();
            Ok(plan.pass(|pass| {
                $(let $f = pass.bind($f).expect("resolved by this plan");)*
                $(let $b = pass.bind($b).expect("resolved by this plan");)*
                $(let $i = pass.bind($i).expect("resolved by this plan");)*
                $(let $u = pass.bind($u).expect("resolved by this plan");)*
                let mut total = 0f64;
                for n in 0..count {
                    let Some(rec) = pass.record(bytes, n * size) else {
                        continue;
                    };
                    let mut f = 0f32;
                    $(f += rec.read($f);)*
                    let mut b = 0u32;
                    $(b += u32::from(rec.read($b));)*
                    let mut i = 0i64;
                    $(i += i64::from(rec.read($i));)*
                    $(i += i64::from(rec.read($u));)*
                    total += f64::from(f) + f64::from(b) + i as f64;
                }
                total
            }))
        }

        #[inline(never)]
        fn typed_sum(widgets: &[Widget]) -> f64 {
            let mut total = 0f64;
            for w in widgets {
                let mut f = 0f32;
                $(f += w.$f;)*
                let mut b = 0u32;
                $(b += u32::from(w.$b);)*
                let mut i = 0i64;
                $(i += i64::from(w.$i);)*
                $(i += i64::from(w.$u);)*
                total += f64::from(f) + f64::from(b) + i as f64;
            }
            total
        }

        const FIELDS: usize = [$(stringify!($f)),*].len() + [$(stringify!($b)),*].len() + 2;
    };
}

widget!(declare);

fn field(name: &str, offset: usize, scalar: Scalar) -> PackField {
    PackField {
        name: name.into(),
        offset,
        repr: FieldRepr::Scalar(scalar),
        space: None,
        bits: Vec::new(),
        note: None,
    }
}

#[inline(never)]
fn plan_sum(plan: &TypePlan, slots: &Slots, bytes: &[u8], count: usize) -> f64 {
    let size = plan.size();
    let mut total = 0f64;
    for n in 0..count {
        let Some(rec) = plan.record(bytes, n * size) else {
            continue;
        };
        let mut f = 0f32;
        for &s in &slots.f32s {
            f += rec.get(s).unwrap_or(0.0);
        }
        let mut b = 0u32;
        for &s in &slots.u8s {
            b += u32::from(rec.get(s).unwrap_or(0));
        }
        let mut i = 0i64;
        for &s in &slots.i32s {
            i += i64::from(rec.get(s).unwrap_or(0));
        }
        for &s in &slots.u32s {
            i += i64::from(rec.get(s).unwrap_or(0));
        }
        total += f64::from(f) + f64::from(b) + i as f64;
    }
    total
}

const COUNT: usize = 1_006;

/// Deterministic, varied bytes: every field of every widget different.
fn corpus() -> (Vec<u8>, Vec<Widget>) {
    let size = std::mem::size_of::<Widget>();
    let mut bytes = vec![0u8; size * COUNT];
    let mut state = 0x9E37_79B9u32;
    for byte in bytes.iter_mut() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *byte = (state >> 11) as u8;
    }
    // Keep every f32 finite so a sum compares by bits.
    let pack = widget_pack();
    let plan = pack.plan("Widget").unwrap();
    for n in 0..COUNT {
        for f in plan
            .own_fields()
            .filter(|f| f.kind == Some(or_io::desc::Kind::F32))
        {
            let at = n * size + f.offset;
            let v = (n * 7 + f.offset) as f32 * 0.125;
            bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    let widgets = (0..COUNT)
        .map(|n| unsafe { std::ptr::read_unaligned(bytes[n * size..].as_ptr() as *const Widget) })
        .collect();
    (bytes, widgets)
}

#[test]
fn a_plan_read_agrees_with_the_typed_struct_it_describes() {
    let pack = widget_pack();
    let plan = pack.plan("Widget").expect("planned");
    let slots = slots(plan).expect("every field is described");
    let (bytes, widgets) = corpus();
    assert_eq!(
        typed_sum(&widgets).to_bits(),
        plan_sum(plan, &slots, &bytes, COUNT).to_bits()
    );
    assert_eq!(
        typed_sum(&widgets).to_bits(),
        pass_sum(plan, &bytes, COUNT).expect("planned").to_bits()
    );
}

#[test]
fn a_consumer_asking_for_a_field_the_pack_lacks_is_told_which() {
    let mut pack = widget_pack();
    pack.types[0].fields.retain(|f| f.name != "f07");
    let plan = pack.plan("Widget").expect("planned");
    let why = slots(plan).err().expect("f07 is not described");
    assert_eq!(
        why,
        PlanError::NoField {
            pack: "synthetic/bench".into(),
            type_name: "Widget".into(),
            field: "f07".into(),
        }
    );
    assert_eq!(
        why.to_string(),
        "the synthetic/bench pack does not describe Widget.f07"
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
fn what_a_plan_read_costs_against_a_typed_struct() {
    let pack = widget_pack();
    let plan = pack.plan("Widget").expect("planned");
    let (bytes, widgets) = corpus();
    const ROUNDS: usize = 4_000;
    let fields = (COUNT * FIELDS * ROUNDS) as f64;
    let typed = best_of(3, ROUNDS, || typed_sum(black_box(&widgets))).as_secs_f64() * 1e9 / fields;
    let looped = best_of(3, ROUNDS, || {
        let slots = slots(black_box(plan)).expect("planned");
        plan_sum(plan, &slots, black_box(&bytes), COUNT)
    })
    .as_secs_f64()
        * 1e9
        / fields;
    let pass = best_of(3, ROUNDS, || {
        pass_sum(black_box(plan), black_box(&bytes), COUNT).expect("planned")
    })
    .as_secs_f64()
        * 1e9
        / fields;
    println!("{COUNT} synthetic records, {FIELDS} fields each, best of 3 blocks of {ROUNDS}");
    println!("  typed struct field                         {typed:.3} ns/field  1.00x");
    println!(
        "  plan, fields as locals, sites once a pass  {pass:.3} ns/field  {:.2}x",
        pass / typed
    );
    println!(
        "  plan, slots looped from a vector           {looped:.3} ns/field  {:.2}x",
        looped / typed
    );
}
