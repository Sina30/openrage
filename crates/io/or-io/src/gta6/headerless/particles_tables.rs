//! A particle rule's first five curves at +0x248 and the +0x388 tables baked from them: each
//! lane is its curve sampled at t = i / 15 in f32 and truncated toward zero to a half float.

use super::copier::Tail;
use super::*;
use crate::half::{f32_to_half, half_to_f32};

const WHAT: &str = "particle curve";
/// Where a rule's curves start, each one's size, and the kind each of the first five states.
const CURVES: usize = 0x248;
const CURVE: usize = 0x18;
const KINDS: [u32; 5] = [3, 2, 7, 6, 6];
/// The curves whose values are half floats; the others hold pairs of floats.
const HALVES: usize = 2;
/// A curve's times fill one record of 16 unorm words.
const TIMES: usize = 0x20;
const KEYS: usize = TIMES / 2;
/// The rule's place of the tables' GPU buffer, the words its object holds, and its data's length.
const TABLES: usize = 0x388;
const TABLE_WORDS: (u16, u16, u32) = (8, 8, 0x980);
const TABLES_LEN: usize = 0x200;
/// Each curve's table and first lane, and the halves of its lanes where it has no key.
const LANES: [(usize, usize); 5] = [(0, 0), (0, 1), (1, 2), (1, 0), (0, 2)];
const ABSENT: [[u16; 2]; 5] = [[0x4000, 0], [0x3C00, 0], [0x3C00, 0], [0, 0], [0, 0]];

/// A curve's keys, none where the rule states none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleCurve {
    /// As stored: 0xFFFF is 1.
    pub times: Vec<u16>,
    /// The second lane is 0 for curves 0 and 1, which hold half floats.
    pub values: Vec<[f32; 2]>,
}

/// A rule's curves 0 to 4, in the order it holds them.
pub type ParticleCurves = [ParticleCurve; 5];

/// Bytes to write, each at its section offset.
type Patches = Vec<(usize, Vec<u8>)>;

const MOVED: &str = "a curve's key count changed: its keys are written only where they stand";

fn lanes(k: usize) -> usize {
    match k < HALVES {
        true => 1,
        false => 2,
    }
}

/// Refused unless the times start at 0 and strictly ascend.
fn keyed(times: &[u16]) -> Result<()> {
    need(
        times.first().is_none_or(|t| *t == 0) && times.windows(2).all(|w| w[0] < w[1]),
        "a curve's first time is not 0, or its times do not ascend",
    )
}

/// The curves of the rule at `at`.
fn curves(v: &Bytes, at: usize) -> Result<ParticleCurves> {
    let mut out: ParticleCurves = Default::default();
    for (k, c) in out.iter_mut().enumerate() {
        let g = at + CURVES + CURVE * k;
        let (n, kind) = (v.u32(WHAT, g + 0x10)? as usize, v.u32(WHAT, g + 0x14)?);
        need(
            kind == KINDS[k],
            "a particle rule's curve is not of the kind measured at its place",
        )?;
        need(n <= KEYS, "a particle rule's curve holds more than 16 keys")?;
        let (a, b) = (v.ptr(WHAT, g)?, v.ptr(WHAT, g + 8)?);
        let (Some(a), Some(b)) = (a, b) else {
            need(
                n == 0 && a.is_none() && b.is_none(),
                "a particle rule's curve has keys but no times or values, or the reverse",
            )?;
            continue;
        };
        need(
            n > 0,
            "a particle rule's curve has keys but no times or values, or the reverse",
        )?;
        let record = v.get(WHAT, a, TIMES)?;
        let mut times: Vec<u16> = (record.as_chunks::<2>().0.iter())
            .map(|h| u16::from_le_bytes(*h))
            .collect();
        need(
            times[n..].iter().all(|t| *t == 0),
            "a curve's time record is not zero past its keys",
        )?;
        times.truncate(n);
        keyed(&times)?;
        c.values = match k < HALVES {
            true => (v.get(WHAT, b, 2 * n)?.as_chunks::<2>().0.iter())
                .map(|h| [half_to_f32(u16::from_le_bytes(*h)), 0.0])
                .collect(),
            false => (v.get(WHAT, b, 8 * n)?.as_chunks::<8>().0.iter())
                .map(|w| [f32_at(w, 0), f32_at(w, 4)])
                .collect(),
        };
        need(
            c.values.iter().flatten().all(|x| x.is_finite()),
            "a curve's value is not finite",
        )?;
        c.times = times;
    }
    Ok(out)
}

/// `x` as a half float rounded toward zero; past the largest finite half, the largest.
fn toward_zero(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    if x.is_nan() {
        return sign | 0x7E00;
    }
    let e = ((bits >> 23) & 0xFF) as i32 - 127;
    match e {
        16.. if x.is_infinite() => sign | 0x7C00,
        16.. => sign | 0x7BFF,
        -14..=15 => sign | (((e + 15) as u16) << 10) | ((bits >> 13) & 0x3FF) as u16,
        // A subnormal half is a multiple of 2^-24.
        _ => sign | (x.abs() * 16_777_216.0) as u16,
    }
}

/// The curve's `lane` at t = i / 15: the first key's value up to it, the last's past the last,
/// and between two keys the line through them, each step in f32.
fn sampled(c: &ParticleCurve, lane: usize, i: usize) -> f32 {
    let t = i as f32 * (1.0f32 / 15.0);
    let time = |j: usize| c.times.get(j).map_or(0.0, |&u| f32::from(u) / 65535.0);
    let value = |j: usize| c.values.get(j).map_or(0.0, |v| v[lane]);
    if t <= time(0) {
        return value(0);
    }
    for j in 1..c.times.len() {
        let (t0, t1) = (time(j - 1), time(j));
        if t <= t1 {
            let f = (t - t0) / (t1 - t0);
            let (v0, v1) = (value(j - 1), value(j));
            return v0 + (v1 - v0) * f;
        }
    }
    value(c.times.len().saturating_sub(1))
}

/// The +0x388 tables `curves` bake to: rows 0 to 15 of lanes A0 to A3, then B0 to B3.
pub fn bake_particle_tables(curves: &ParticleCurves) -> GpuTables {
    let mut out: GpuTables = [[[0; 4]; 16]; 2];
    for (k, c) in curves.iter().enumerate() {
        let (table, first) = LANES[k];
        for (lane, &absent) in ABSENT[k].iter().enumerate().take(lanes(k)) {
            for (i, row) in out[table].iter_mut().enumerate() {
                row[first + lane] = match c.times.is_empty() {
                    true => absent,
                    false => toward_zero(sampled(c, lane, i)),
                };
            }
        }
    }
    out
}

/// Two halves at most one step apart, a zero of either sign 0.
fn near(a: u16, b: u16) -> bool {
    let key = |h: u16| match h & 0x8000 {
        0 => i32::from(h),
        _ => -i32::from(h & 0x7FFF),
    };
    (key(a) - key(b)).abs() <= 1
}

/// The rule at `at`'s curves, where its tables' data starts and the tables as stored; refused
/// where a stored half is not within one step of the curves' bake.
fn checked(v: &Bytes, p: &Bytes, at: usize) -> Result<(ParticleCurves, usize, GpuTables)> {
    let c = curves(v, at)?;
    let Some(object) = v.ptr(WHAT, at + TABLES)? else {
        return refuse(
            "a model rule has no +0x388 tables, and what its curves drive is not measured",
        );
    };
    let words = (
        v.u16(WHAT, object + 0x0C)?,
        v.u16(WHAT, object + 0x0E)?,
        v.u32(WHAT, object + 0x10)?,
    );
    need(
        words == TABLE_WORDS && u64::from(v.u32(WHAT, object + 8)?) * 4 == TABLES_LEN as u64,
        "a rule's +0x388 GPU buffer is not the 512 bytes of its two tables",
    )?;
    let data = v.phys(WHAT, object + 0x18)?;
    let raw = p.get(WHAT, data, TABLES_LEN)?;
    let word = |o: usize| u16::from_le_bytes([raw[o], raw[o + 1]]);
    let stored: GpuTables = std::array::from_fn(|t| {
        std::array::from_fn(|r| std::array::from_fn(|l| word(0x100 * t + 8 * r + 2 * l)))
    });
    let baked = bake_particle_tables(&c);
    let halves = |t: &GpuTables| t.iter().flatten().flatten().copied().collect::<Vec<u16>>();
    need(
        halves(&stored)
            .iter()
            .zip(&halves(&baked))
            .all(|(a, b)| near(*a, *b)),
        "a rule's +0x388 tables are not within one half-float step of the bake of its curves",
    )?;
    Ok((c, data, stored))
}

/// A sprite or trail rule's curves, refused where its +0x388 tables are not their bake.
pub fn read_particle_curves(
    virtual_section: &[u8],
    physical: &[u8],
    rule: &ParticleRule,
) -> Result<ParticleCurves> {
    let at = rule.at as usize;
    sections(virtual_section, Some(physical), |v, p| checked(&v, &p, at)).map(|(c, _, _)| c)
}

/// The value of a curve of half floats as its half, refused where it is none.
fn half(x: &[f32; 2]) -> Result<u16> {
    let h = f32_to_half(x[0]);
    need(
        x[1] == 0.0 && half_to_f32(h) == x[0],
        "a value of curve 0 or 1 is not a half float, or sets a second lane",
    )?;
    Ok(h)
}

/// The writes that give the rule at `at` the curves `edited`, each where its curve's keys are,
/// and where its tables' data starts.
fn plan(v: &Bytes, p: &Bytes, at: usize, edited: &ParticleCurves) -> Result<(Patches, usize)> {
    let (stored, data, _) = checked(v, p, at)?;
    let mut out = Patches::new();
    for (k, (s, e)) in stored.iter().zip(edited).enumerate() {
        if s == e {
            continue;
        }
        need(
            e.times.len() == s.times.len() && e.values.len() == s.times.len(),
            MOVED,
        )?;
        keyed(&e.times)?;
        need(
            e.values.iter().flatten().all(|x| x.is_finite()),
            "a curve's value is not finite",
        )?;
        let g = at + CURVES + CURVE * k;
        let (Some(a), Some(b)) = (v.ptr(WHAT, g)?, v.ptr(WHAT, g + 8)?) else {
            return refuse(MOVED);
        };
        let mut times = [0u8; TIMES];
        for (slot, t) in times.as_chunks_mut::<2>().0.iter_mut().zip(&e.times) {
            slot.copy_from_slice(&t.to_le_bytes());
        }
        out.push((a, times.to_vec()));
        let values: Vec<u8> = match k < HALVES {
            true => (e.values.iter().map(half))
                .collect::<Result<Vec<u16>>>()?
                .iter()
                .flat_map(|h| h.to_le_bytes())
                .collect(),
            false => (e.values.iter().flatten())
                .flat_map(|x| x.to_le_bytes())
                .collect(),
        };
        out.push((b, values));
    }
    Ok((out, data))
}

/// The sections with `edited` written over the rule at `at`'s curves and its +0x388 tables
/// baked from them again, read back; `None` where the curves are as stored, which keeps the
/// stored tables.
fn written(
    virtual_section: &[u8],
    physical: &[u8],
    at: usize,
    edited: &ParticleCurves,
) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
    let (patches, data) = sections(virtual_section, Some(physical), |v, p| {
        plan(&v, &p, at, edited)
    })?;
    if patches.is_empty() {
        return Ok(None);
    }
    let mut v = virtual_section.to_vec();
    for (to, bytes) in patches {
        match v.get_mut(to..to + bytes.len()) {
            Some(slot) => slot.copy_from_slice(&bytes),
            None => return refuse("a curve's key lies past the section's end"),
        }
    }
    let baked = bake_particle_tables(edited);
    let mut p = physical.to_vec();
    let rows = baked.iter().flat_map(|t| {
        (t.iter().flatten().flat_map(|h| h.to_le_bytes())).chain([0; TABLES_LEN / 4])
    });
    match p.get_mut(data..data + TABLES_LEN) {
        Some(slot) => slot.iter_mut().zip(rows).for_each(|(s, b)| *s = b),
        None => return refuse("a rule's +0x388 tables lie past the physical section's end"),
    }
    let (read, _, tables) = sections(&v, Some(&p), |v, p| checked(&v, &p, at))?;
    need(
        read == *edited && tables == baked,
        "the curves written do not read back as edited, or their tables as their bake",
    )?;
    Ok(Some((v, p)))
}

/// A rule's curves written in place and its +0x388 tables baked again, or refused, unchanged.
// Curves as stored keep the stored tables, which a bake differs from by one step at a near tie.
pub fn write_particle_curves(
    virtual_section: &mut [u8],
    physical: &mut [u8],
    rule: &ParticleRule,
    edited: &ParticleCurves,
) -> Result<()> {
    let list = read_particle_effects(virtual_section, None)?;
    let Some((v, p)) = written(virtual_section, physical, rule.at as usize, edited)? else {
        return Ok(());
    };
    need(
        read_particle_effects(&v, None)? == list,
        "writing a rule's curves changes what else the list holds",
    )?;
    virtual_section.copy_from_slice(&v);
    physical.copy_from_slice(&p);
    Ok(())
}

/// Give the rule's copy at `to` its own time record and value array for each of its five curves,
/// so that editing one rule's curves never patches the other's; a copy of the rule record alone
/// leaves the copy's `+0x248` heads aimed at the source's arrays.
pub(super) fn own_particle_curves(t: &mut Tail, v: &Bytes, from: usize, to: usize) -> Result<()> {
    for k in 0..KINDS.len() {
        let g = CURVES + CURVE * k;
        let n = v.u32(WHAT, from + g + 0x10)? as usize;
        if let Some(a) = v.ptr(WHAT, from + g)? {
            let held = t.copy(v, WHAT, a, TIMES)?;
            t.ptr(to + g, held)?;
        }
        if n > 0 {
            if let Some(b) = v.ptr(WHAT, from + g + 8)? {
                let held = t.copy(v, WHAT, b, n * if k < HALVES { 2 } else { 8 })?;
                t.ptr(to + g + 8, held)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULE_LEN: usize = 0x430;
    /// Where the image holds the tables' buffer object and curve 0's and curve 2's keys.
    const OBJECT: usize = RULE_LEN;
    const KEYS_0: (usize, usize) = (0x480, 0x4A0);
    const KEYS_2: (usize, usize) = (0x4C0, 0x4E0);

    fn curve(times: &[u16], values: &[[f32; 2]]) -> ParticleCurve {
        ParticleCurve {
            times: times.to_vec(),
            values: values.to_vec(),
        }
    }

    /// Curve 0 ramps from 0 to 20 by time 0x7299; curve 2 falls from 1 over three keys.
    fn curves_held() -> ParticleCurves {
        let mut c: ParticleCurves = Default::default();
        c[0] = curve(&[0, 0x7299], &[[0.0, 0.0], [20.0, 0.0]]);
        c[2] = curve(
            &[0, 0x338B, 0xFFFF],
            &[[1.0, 0.0], [0.5666, 0.0], [0.3824, 0.0]],
        );
        c
    }

    fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
        v[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// A rule at 0 holding `curves_held` and pointing at its tables, which hold their bake.
    fn image() -> (Vec<u8>, Vec<u8>) {
        let mut v = vec![0u8; 0x500];
        for (k, kind) in [3u32, 2, 7, 6, 6, 2, 1].into_iter().enumerate() {
            put(&mut v, CURVES + CURVE * k + 0x14, &kind.to_le_bytes());
        }
        let held = curves_held();
        for (k, (a, b)) in [(0, KEYS_0), (2, KEYS_2)] {
            let g = CURVES + CURVE * k;
            put(&mut v, g, &(0x5000_0000 + a as u64).to_le_bytes());
            put(&mut v, g + 8, &(0x5000_0000 + b as u64).to_le_bytes());
            let n = held[k].times.len() as u32;
            put(&mut v, g + 0x10, &n.to_le_bytes());
            for (j, (t, x)) in held[k].times.iter().zip(&held[k].values).enumerate() {
                put(&mut v, a + 2 * j, &t.to_le_bytes());
                match k {
                    0 => put(&mut v, b + 2 * j, &f32_to_half(x[0]).to_le_bytes()),
                    _ => {
                        put(&mut v, b + 8 * j, &x[0].to_le_bytes());
                        put(&mut v, b + 8 * j + 4, &x[1].to_le_bytes());
                    }
                }
            }
        }
        put(&mut v, TABLES, &(0x5000_0000 + OBJECT as u64).to_le_bytes());
        put(&mut v, OBJECT + 8, &128u32.to_le_bytes());
        put(&mut v, OBJECT + 0x0C, &[8, 0, 8, 0]);
        put(&mut v, OBJECT + 0x10, &0x980u32.to_le_bytes());
        put(&mut v, OBJECT + 0x18, &0x6000_0000u64.to_le_bytes());
        let mut p = vec![0u8; TABLES_LEN];
        for (t, table) in bake_particle_tables(&held).iter().enumerate() {
            for (r, row) in table.iter().enumerate() {
                for (l, h) in row.iter().enumerate() {
                    put(&mut p, 0x100 * t + 8 * r + 2 * l, &h.to_le_bytes());
                }
            }
        }
        (v, p)
    }

    fn read(v: &[u8], p: &[u8]) -> Result<ParticleCurves> {
        sections(v, Some(p), |v, p| checked(&v, &p, 0)).map(|(c, _, _)| c)
    }

    /// The half `h` truncates `x` to: no farther from zero, and the next half past it.
    fn truncates(h: u16, x: f64) {
        let (low, high) = (f64::from(half_to_f32(h)), f64::from(half_to_f32(h + 1)));
        assert!(low <= x && x < high, "{h:#06x} for {x}");
    }

    #[test]
    fn a_half_is_truncated_toward_zero() {
        for x in [0.0f32, 1.0, -2.5, 0.375, 65504.0, 1.0 / 1024.0] {
            assert_eq!(toward_zero(x), f32_to_half(x), "{x} is a half");
        }
        assert_eq!(toward_zero(1.0 + 1.9 / 1024.0), 0x3C01);
        assert_eq!(toward_zero(-(1.0 + 1.9 / 1024.0)), 0xBC01);
        assert_eq!(toward_zero(70000.0), 0x7BFF);
        assert_eq!(toward_zero(3.5 / 16_777_216.0), 3, "a subnormal");
        assert!(near(0x3C00, 0x3C01) && near(0x8001, 0x0000) && !near(0x3C00, 0x3C02));
    }

    #[test]
    fn a_rules_tables_bake_each_curve_at_sixteen_times() {
        let t = bake_particle_tables(&curves_held());
        // Curve 0, lane A0: a ramp to 20 at time 0x7299, then held.
        assert_eq!(t[0][0][0], 0);
        let end = f64::from(0x7299u16) / 65535.0;
        for (i, row) in t[0].iter().enumerate().skip(1) {
            let at = i as f64 / 15.0;
            let x = if at < end { 20.0 * at / end } else { 20.0 };
            truncates(row[0], x);
        }
        assert_eq!(t[0][15][0], 0x4D00);
        // Curve 2, lanes B2 and B3: from 1 at time 0 to 0.3824 at time 1, the second lane 0.
        assert_eq!(t[1][0][2], 0x3C00);
        truncates(t[1][15][2], f64::from(0.3824f32));
        assert!((0..16).all(|i| t[1][i][3] == 0));
        // The curves with no key: A1 1, A2 and A3 0, B0 and B1 0.
        assert!((0..16).all(|i| t[0][i][1] == 0x3C00 && t[0][i][2..] == [0, 0]));
        assert!((0..16).all(|i| t[1][i][..2] == [0, 0]));
        // A curve whose last time is below 1 is held flat after it.
        let mut c: ParticleCurves = Default::default();
        c[3] = curve(&[0, 0x8000], &[[0.0, 4.0], [1.0, 2.0]]);
        let t = bake_particle_tables(&c);
        assert!((8..16).all(|i| t[1][i][..2] == [0x3C00, 0x4000]));
        assert_eq!(t[0][0][0], 0x4000, "curve 0 with no key gives 2");
    }

    #[test]
    fn a_rules_curves_read_and_their_tables_are_held_to_their_bake() {
        let (v, mut p) = image();
        assert_eq!(read(&v, &p).expect("they read"), curves_held());
        // A half two steps from its bake.
        let h = u16::from_le_bytes([p[8], p[9]]);
        put(&mut p, 8, &(h + 2).to_le_bytes());
        assert!(read(&v, &p).is_err());
        let (v, p) = image();
        // Times that do not ascend, a time past the key count, and a kind not measured.
        for (at, byte) in [
            (KEYS_2.0 + 2, 0x00),
            (KEYS_0.0 + 4, 0x01),
            (CURVES + 0x14, 4),
        ] {
            let mut w = v.clone();
            w[at] = byte;
            w[at + 1] = 0;
            assert!(read(&w, &p).is_err(), "{at:#x}");
        }
        // A model rule, which points at no tables.
        let mut w = v.clone();
        put(&mut w, TABLES, &[0; 8]);
        assert!(read(&w, &p).is_err());
    }

    #[test]
    fn an_edited_curve_is_written_where_it_stands_and_its_tables_baked_again() {
        let (v, p) = image();
        assert_eq!(
            written(&v, &p, 0, &curves_held()).ok(),
            Some(None),
            "as stored"
        );

        let mut edited = curves_held();
        edited[0].values[1][0] = 10.0;
        edited[2].times[1] = 0x4000;
        let (w, q) = written(&v, &p, 0, &edited)
            .expect("it writes")
            .expect("something changed");
        assert_eq!(read(&w, &q).expect("it reads back"), edited);
        let t = bake_particle_tables(&edited);
        assert_eq!(t[0][15][0], 0x4900, "10 at the last row");
        assert_eq!(&q[0x80..0x100], &[0; 0x80][..], "rows 16 to 31 stay 0");
        assert_eq!(&w[KEYS_0.1 + 2..KEYS_0.1 + 4], &0x4900u16.to_le_bytes());
        assert_eq!(&w[KEYS_2.0 + 2..KEYS_2.0 + 4], &0x4000u16.to_le_bytes());

        // A key added, a time that goes back, and a half curve's value no half holds.
        let mut more = curves_held();
        more[0].times.push(0xFFFF);
        more[0].values.push([1.0, 0.0]);
        let mut back = curves_held();
        back[2].times[2] = 0x1000;
        let mut off = curves_held();
        off[0].values[1][0] = 20.001;
        for wrong in [more, back, off] {
            assert!(written(&v, &p, 0, &wrong).is_err(), "{wrong:?}");
        }
    }
}
