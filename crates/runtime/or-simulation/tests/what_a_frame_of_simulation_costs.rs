use or_io::gta5::particle::PtxFxList;
use or_particle::list;
use or_simulation::point::PointBuffer;
use or_simulation::{build, PreviewTarget, PtfxManager};
use std::time::Instant;

mod synthetic;
fn snapshot_bytes(p: &PointBuffer) -> usize {
    fn col<T>(v: &[T]) -> usize {
        std::mem::size_of_val(v)
    }
    col(&p.init_rotation)
        + col(&p.coln_pos)
        + col(&p.coln_speed)
        + col(&p.coln_normal)
        + col(&p.pause_timer)
        + col(&p.one_over_life)
        + col(&p.emitter_life_ratio)
        + col(&p.playback_rate)
        + col(&p.sort_val)
        + col(&p.flags)
        + col(&p.coln_mtl_id)
        + col(&p.rand_index)
        + col(&p.prev_pos)
        + col(&p.curr_pos)
        + col(&p.spawn_pos)
        + col(&p.matrix_weight)
        + col(&p.curr_vel)
        + col(&p.tex_frame_ratio)
        + col(&p.dimensions)
        + col(&p.curr_rot)
        + col(&p.curr_angle)
        + col(&p.color)
        + col(&p.emissive_intensity)
        + col(&p.tex_frame_a)
        + col(&p.tex_frame_b)
        + col(&p.init_tex_frame)
        + col(&p.alpha_mult)
        + col(&p.is_dead)
        + col(&p.is_broken_trail)
        + col(&p.life_ratio)
        + col(&p.uv_offset)
        + col(&p.evo_values)
        + col(&p.velocity_noise_extra)
        + col(&p.age_atmos_extra)
        + col(&p.colour_tint_extra)
        + col(&p.collision_bounce_extra)
        + col(&p.attractor_orbit_extra)
        + col(&p.prev_in_chain)
        + col(&p.next_in_chain)
}

const COLUMNS_SUMMED: usize = 39;
fn live_capacity_bytes(p: &PointBuffer) -> usize {
    let len = p.curr_pos.len();
    if len == 0 {
        return 0;
    }
    let cap = p.curr_pos.capacity();
    snapshot_bytes(p) * cap / len
}

#[test]
fn the_column_count_is_guarded() {
    let vec_size = std::mem::size_of::<Vec<u8>>();
    let ends = 2 * std::mem::size_of::<Option<usize>>();
    let derived = (std::mem::size_of::<PointBuffer>() - ends) / vec_size;
    println!(
        "PointBuffer is {} bytes = {} columns x {} + {} for the chain ends",
        std::mem::size_of::<PointBuffer>(),
        derived,
        vec_size,
        ends
    );
    assert_eq!(
        derived, COLUMNS_SUMMED,
        "a column was added to PointBuffer; snapshot_bytes() is now an \
         undercount. Add the column to it and raise COLUMNS_SUMMED."
    );
}

fn synthetic(num_events: usize, spawn_rate: f32, life: f32) -> PtxFxList {
    synthetic::list_of(num_events, 600.0, spawn_rate, life, &[])
}

fn warmed(num_events: usize, spawn_rate: f32, life: f32) -> PtfxManager {
    let asset = synthetic(num_events, spawn_rate, life);
    let inst = build(&asset, PreviewTarget::Effect(0)).expect("build effect");
    let mut mgr = PtfxManager::new();
    mgr.set_effect(inst);
    mgr.clock.play();
    let ticks = ((life * 2.0) * 60.0).ceil() as usize + 30;
    for _ in 0..ticks {
        mgr.tick(1.0 / 60.0);
    }
    mgr
}

fn live_particles(mgr: &PtfxManager) -> usize {
    mgr.effect
        .as_ref()
        .map(|e| e.emitters.iter().map(|m| m.points.len()).sum())
        .unwrap_or(0)
}

#[test]
fn a_snapshot_is_a_deep_clone_of_every_live_particle() {
    // One emitter, a pool near the per-emitter soft cap, so the
    // per-particle figure is measured over a buffer big enough that the
    // fixed overhead is not what is being read.
    let mgr = warmed(1, 4000.0, 1.0);
    let eff = mgr.effect.as_ref().expect("effect");
    let em = eff.emitters.first().expect("emitter");
    let n = em.points.len();
    assert!(n > 500, "pool too small to measure: {n} particles");

    let bytes = snapshot_bytes(&em.points);
    let per_particle = bytes as f64 / n as f64;
    let cap_bytes = live_capacity_bytes(&em.points);

    println!("--- what one particle occupies ---");
    println!("  live particles              {n}");
    println!("  snapshot bytes (len)        {bytes}");
    println!("  live bytes     (capacity)   {cap_bytes}");
    println!("  bytes per particle          {per_particle:.1}");

    assert!(
        (200.0..600.0).contains(&per_particle),
        "bytes per particle moved out of the measured band: {per_particle:.1}"
    );

    const RETAINED: usize = 80;
    println!("--- what the retained cache costs, per emitter ---");
    for pool in [64usize, 256, 1024, 4096] {
        let per_snapshot = per_particle * pool as f64;
        println!(
            "  {pool:>5} particles: {:>8.1} KB per snapshot, {:>8.1} MB retained",
            per_snapshot / 1024.0,
            per_snapshot * RETAINED as f64 / (1024.0 * 1024.0)
        );
    }
    println!(
        "  per-emitter soft cap is 4096, so one emitter's worst case is \
         {:.0} MB and an effect's is that times its emitter count",
        per_particle * 4096.0 * RETAINED as f64 / (1024.0 * 1024.0)
    );
}

#[test]
fn the_snapshot_cache_is_capped_at_80() {
    let mut mgr = warmed(1, 600.0, 0.5);
    let before = mgr.snapshots.len();
    for _ in 0..(30 * 60) {
        mgr.tick(1.0 / 60.0);
    }
    println!(
        "after 30 s of play: {} snapshots (was {before} after warm-up), \
         interval {} s",
        mgr.snapshots.len(),
        mgr.snapshot_interval
    );
    assert_eq!(
        mgr.snapshots.len(),
        80,
        "the retention cap moved; the memory figures above scale with it"
    );
}

#[test]
fn a_capture_is_priced_against_the_tick_it_rides_on() {
    for (label, emitters_wanted, spawn, life) in [
        ("a real effect's scale", 7usize, 50.0f32, 1.0f32),
        ("thirty times that", 8usize, 500.0f32, 1.0f32),
    ] {
        price_one_capture(label, emitters_wanted, spawn, life);
    }
}

fn price_one_capture(label: &str, emitters_wanted: usize, spawn: f32, life: f32) {
    let mut mgr = warmed(emitters_wanted, spawn, life);
    let n = live_particles(&mgr);
    assert!(n > 100, "pool too small to price: {n}");
    let emitters = mgr.effect.as_ref().map(|e| e.emitters.len()).unwrap_or(0);

    let shipped_interval = f64::from(mgr.snapshot_interval);
    mgr.snapshot_interval = 1.0e9;
    let mut best_tick = f64::MAX;
    let mut best_capture = f64::MAX;
    for _ in 0..40 {
        let t0 = Instant::now();
        mgr.tick(1.0 / 60.0);
        best_tick = best_tick.min(t0.elapsed().as_secs_f64());

        let t1 = Instant::now();
        mgr.capture_snapshot();
        best_capture = best_capture.min(t1.elapsed().as_secs_f64());
        mgr.snapshots.clear();
    }

    let ratio = best_capture / best_tick;
    let ticks_between = (shipped_interval * 60.0).round().max(1.0);
    println!("--- capture against tick: {label} ---");
    println!("  {emitters} emitters, {n} live particles");
    println!("  tick    {:>9.1} us", best_tick * 1e6);
    println!("  capture {:>9.1} us", best_capture * 1e6);
    println!(
        "  a capture is {ratio:.2} ticks, and with the shipped \
         {shipped_interval} s interval it happens every {ticks_between:.0} \
         ticks at 60 fps"
    );
    println!(
        "  so it adds {:.1}% to the average SIM TICK — and the tick is one \
         part of a frame, not the frame",
        100.0 * ratio / ticks_between
    );
    println!(
        "  ⚠ the average is the less useful half. The shape is a periodic \
         spike: one tick in {ticks_between:.0} costs {:.2}x a normal one, \
         which is a {:.1} Hz hitch rather than a uniform tax.",
        1.0 + ratio,
        60.0 / ticks_between
    );

    assert!(best_tick > 0.0 && best_capture > 0.0);
}

/// Below [`PARALLEL_THRESHOLD`] emitters the walk is serial; at it and
/// above it hands off to rayon. Scanning emitter counts across it through
/// the public API shows the handoff as a discontinuity.
///
/// Every count is measured in the same process in the same loop, so machine
/// load lands on all of them; the statistic is the minimum over passes,
/// because noise adds time.
///
/// ⚠ One build only ever exercises one threshold, so this cannot say whether
/// the constant is *right*; that takes binaries differing only in it. Run
/// that way at 3 emitters of ~1080 particles, threshold 4 ticked in 67.2 us
/// and threshold 3 in 55.5 us (0.83x), against 0.97-1.01x on the counts both
/// builds take the same path; 215 of `core.ypt`'s 964 effects have exactly
/// three emitters.
#[test]
fn the_parallel_threshold_is_where_the_per_emitter_cost_turns() {
    const SPAWN: f32 = 400.0;
    const LIFE: f32 = 1.0;
    const COUNTS: [usize; 9] = [1, 2, 3, 4, 5, 6, 8, 16, 32];
    const PASSES: usize = 200;

    let mut mgrs: Vec<(usize, PtfxManager)> = COUNTS
        .iter()
        .map(|&n| {
            let mut m = warmed(n, SPAWN, LIFE);
            m.snapshot_interval = 1.0e9;
            (n, m)
        })
        .collect();

    let mut best = vec![f64::MAX; COUNTS.len()];
    for _ in 0..PASSES {
        for (i, (_, mgr)) in mgrs.iter_mut().enumerate() {
            let t = Instant::now();
            mgr.tick(1.0 / 60.0);
            best[i] = best[i].min(t.elapsed().as_secs_f64());
        }
    }

    let threshold = or_simulation::parallel::PARALLEL_THRESHOLD;
    println!("--- per-emitter tick cost across the threshold ({threshold}) ---");
    println!(
        "{:>8}  {:>9}  {:>9}  {:>10}  {:>7}",
        "emitters", "particles", "tick us", "us/emitter", "path"
    );
    let mut per_emitter = Vec::new();
    for (i, (n, mgr)) in mgrs.iter().enumerate() {
        let particles = live_particles(mgr);
        let us = best[i] * 1e6;
        let each = us / *n as f64;
        per_emitter.push(each);
        println!(
            "{:>8}  {:>9}  {:>9.1}  {:>10.2}  {:>7}",
            n,
            particles,
            us,
            each,
            if *n >= threshold { "rayon" } else { "serial" }
        );
    }

    assert!(
        COUNTS.iter().any(|&n| n < threshold) && COUNTS.iter().any(|&n| n >= threshold),
        "PARALLEL_THRESHOLD is {threshold}, which the scanned counts \
         {COUNTS:?} do not bracket — this test would measure one path twice"
    );
    let below = COUNTS.iter().rposition(|&n| n < threshold).unwrap();
    let above = COUNTS.iter().position(|&n| n >= threshold).unwrap();
    println!(
        "\n  {} emitters (serial): {:.2} us/emitter\n  \
         {} emitters (rayon):  {:.2} us/emitter\n  \
         crossing the threshold multiplies per-emitter cost by {:.2}x",
        COUNTS[below],
        per_emitter[below],
        COUNTS[above],
        per_emitter[above],
        per_emitter[above] / per_emitter[below]
    );

    let (best_i, best_v) =
        per_emitter
            .iter()
            .enumerate()
            .fold(
                (0usize, f64::MAX),
                |acc, (i, v)| {
                    if *v < acc.1 {
                        (i, *v)
                    } else {
                        acc
                    }
                },
            );
    println!(
        "  cheapest per emitter at {} emitters ({best_v:.2} us/emitter)",
        COUNTS[best_i]
    );

    for (i, v) in per_emitter.iter().enumerate() {
        assert!(
            *v > 0.0,
            "no measurable cost at {} emitters — instrument is broken",
            COUNTS[i]
        );
    }
}

#[test]
fn what_the_snapshot_cache_holds_for_a_real_effect() {
    let path = or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR")).join("core.ypt");
    if !path.exists() {
        or_test_support::skip_corpus(
            "what_the_snapshot_cache_holds_for_a_real_effect",
            format_args!("skip: fixture missing at {}", path.display()),
        );
        return;
    }
    let asset = PtxFxList::open(&path).expect("the fixture opens owned");

    let mut ranked: Vec<(usize, usize)> = list::effect_rules(&asset)
        .enumerate()
        .map(|(i, e)| (i, list::events(e).count()))
        .collect();
    ranked.sort_by_key(|&(_, n)| std::cmp::Reverse(n));

    let mut worst: Option<(String, usize, usize, usize)> = None;
    for &(idx, events) in ranked.iter().take(24) {
        let Some(inst) = build(&asset, PreviewTarget::Effect(idx)) else {
            continue;
        };
        let mut mgr = PtfxManager::new();
        mgr.set_effect(inst);
        mgr.clock.play();
        mgr.snapshot_interval = 1.0e9;
        for _ in 0..180 {
            mgr.tick(1.0 / 60.0);
        }
        let Some(eff) = mgr.effect.as_ref() else {
            continue;
        };
        let bytes: usize = eff.emitters.iter().map(|m| snapshot_bytes(&m.points)).sum();
        let particles: usize = eff.emitters.iter().map(|m| m.points.len()).sum();
        let name = eff.rule.name.clone();
        if worst.as_ref().map(|w| bytes > w.3).unwrap_or(true) {
            worst = Some((name, events, particles, bytes));
        }
    }

    let Some((name, events, particles, bytes)) = worst else {
        or_test_support::skip_corpus(
            "what_the_snapshot_cache_holds_for_a_real_effect",
            "skip: no effect built",
        );
        return;
    };
    const RETAINED: usize = 80;
    println!("--- heaviest of the 24 widest effects in core.ypt, after 3 s ---");
    println!("  effect                {name}");
    println!("  emitter events        {events}");
    println!("  live particles        {particles}");
    println!("  one snapshot          {:.1} KB", bytes as f64 / 1024.0);
    println!(
        "  cache at its 80-frame cap {:.1} MB",
        (bytes * RETAINED) as f64 / (1024.0 * 1024.0)
    );
    println!(
        "  ⚠ this is per *previewed effect*, and pinning effects in the \
         hierarchy ticks several at once"
    );

    assert!(
        particles > 0,
        "no live particles in {name} — the measurement reached nothing and \
         its byte figure says nothing about the cache"
    );
}

#[test]
fn how_many_real_effects_reach_the_parallel_threshold() {
    let path = or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR")).join("core.ypt");
    if !path.exists() {
        or_test_support::skip_corpus(
            "how_many_real_effects_reach_the_parallel_threshold",
            format_args!("skip: fixture missing at {}", path.display()),
        );
        return;
    }
    let asset = PtxFxList::open(&path).expect("the fixture opens owned");

    let counts: Vec<usize> = list::effect_rules(&asset)
        .map(|e| list::events(e).count())
        .collect();
    assert!(!counts.is_empty(), "no effects parsed");

    let threshold = or_simulation::parallel::PARALLEL_THRESHOLD;
    let total = counts.len();
    let reaching = counts.iter().filter(|&&n| n >= threshold).count();
    let mut sorted = counts.clone();
    sorted.sort_unstable();
    let median = sorted[total / 2];
    let max = *sorted.last().unwrap();
    let sum: usize = counts.iter().sum();

    println!("--- emitter events per effect, core.ypt ---");
    println!("  effects                      {total}");
    println!("  emitter events               {sum}");
    println!("  median events per effect     {median}");
    println!("  most events on one effect    {max}");
    println!(
        "  effects with >= {threshold} (parallel) {reaching} ({:.1}%)",
        100.0 * reaching as f64 / total as f64
    );
    println!(
        "  effects with <  {threshold} (serial)   {} ({:.1}%)",
        total - reaching,
        100.0 * (total - reaching) as f64 / total as f64
    );
    let mut hist = std::collections::BTreeMap::new();
    for &n in &counts {
        *hist.entry(n.min(10)).or_insert(0usize) += 1;
    }
    println!("  histogram (10 = ten or more):");
    for (n, c) in hist {
        println!("    {n:>2} events: {c:>4}");
    }

    assert!(sum > 0, "no emitter events across {total} effects");
}
