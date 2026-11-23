//! Where the time of a photo run goes: each stage's total and count, printed to stderr when the run ends.
//!
//! On with `OPENRAGE_PHOTO_TIMINGS` set (to anything); off, a stage costs one flag read.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static ON: OnceLock<bool> = OnceLock::new();
static TOTALS: Mutex<BTreeMap<&'static str, (Duration, u32)>> = Mutex::new(BTreeMap::new());

fn on() -> bool {
    *ON.get_or_init(|| std::env::var_os("OPENRAGE_PHOTO_TIMINGS").is_some())
}

/// `stage` run, its time added to that stage's total.
pub(crate) fn time<T>(stage: &'static str, run: impl FnOnce() -> T) -> T {
    if !on() {
        return run();
    }
    let start = Instant::now();
    let result = run();
    let mut totals = TOTALS.lock().unwrap_or_else(|e| e.into_inner());
    let total = totals.entry(stage).or_default();
    total.0 += start.elapsed();
    total.1 += 1;
    result
}

/// Every stage's total, its count and its mean, in the order of the stages' names.
pub(crate) fn report() {
    if !on() {
        return;
    }
    let totals = TOTALS.lock().unwrap_or_else(|e| e.into_inner());
    eprintln!("photo timings (stage: total, count, mean)");
    for (stage, (total, count)) in totals.iter() {
        let mean = total.as_secs_f64() * 1000.0 / f64::from((*count).max(1));
        eprintln!(
            "  {stage:<28} {:>9.1} ms {count:>6} x {mean:>8.1} ms",
            total.as_secs_f64() * 1000.0
        );
    }
}
