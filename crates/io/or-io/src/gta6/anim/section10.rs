//! A section-10 channel: per 64-frame segment an initial value, per 8-frame group a bit width, and
//! per frame a zigzag acceleration of that width, the group's eight frames in eight interleaved
//! lanes of 32-bit words. Bits run from the least significant bit of each little-endian word.

/// Frames a segment starts afresh from its stored initial value.
const SEGMENT: usize = 64;
/// Frames a group shares one width over, one in each lane.
const LANES: usize = 8;
/// The four header words before the address table.
const HEADER_BITS: usize = 128;

/// The value at each of `frames` frames of the channel whose constant words are `words`.
///
/// ⚠ Each segment's last step must land on the next segment's initial value: the encoder stores
/// both, so a decode that misses one has misread the stream.
pub(super) fn decode(words: &[u32], frames: usize) -> Result<Vec<f32>, String> {
    let steps = quantised(words, frames)?;
    let (scale, offset) = (f32::from_bits(words[2]), f32::from_bits(words[3]));
    Ok(steps.into_iter().map(|q| value(scale, offset, q)).collect())
}

/// The value quantised step `q` of a channel of `scale` and `offset` decodes to.
pub(super) fn value(scale: f32, offset: f32, q: i64) -> f32 {
    (f64::from(scale) * q as f64 + f64::from(offset)) as f32
}

/// The quantised step at each of `frames` frames of the channel whose constant words are `words`.
pub(super) fn quantised(words: &[u32], frames: usize) -> Result<Vec<i64>, String> {
    if words.len() < 4 {
        return Err(format!(
            "its stride {} leaves no room for its four header words",
            words.len()
        ));
    }
    if frames == 0 {
        return Err("its block has no frame".into());
    }
    let [address_bits, initial_bits, lane_words, top] = words[1].to_le_bytes();
    if top != 0 || address_bits > 32 || initial_bits > 32 {
        return Err(format!(
            "its widths word {:#010X} states more than 32 bits or a non-zero top byte",
            words[1]
        ));
    }
    let (address_bits, initial_bits) = (u32::from(address_bits), u32::from(initial_bits));
    let lanes_at = words
        .len()
        .checked_sub(LANES * usize::from(lane_words))
        .filter(|at| *at >= 4)
        .ok_or_else(|| {
            format!(
                "its {lane_words} words a lane do not fit its stride of {}",
                words.len()
            )
        })?;
    let steps = frames - 1;
    let segments = frames.div_ceil(SEGMENT);
    let groups = steps.div_ceil(LANES);
    let past = |at: usize| format!("it reads bit {at}, past its {} words", words.len());
    let read = |at: usize, n: u32| bits(words, at, n).ok_or_else(|| past(at));
    let mut at = HEADER_BITS;
    let mut addresses = Vec::with_capacity(segments);
    for _ in 0..segments {
        addresses.push(read(at, address_bits)?);
        at += address_bits as usize;
    }
    let mut initials = Vec::with_capacity(segments);
    for _ in 0..segments {
        initials.push(read(at, initial_bits)? as i64);
        at += initial_bits as usize;
    }
    let width_end = 32 * lanes_at;
    let mut widths = Vec::with_capacity(groups);
    for _ in 0..groups {
        let mut width = 0u32;
        while at < width_end && read(at, 1)? == 0 {
            width += 1;
            at += 1;
        }
        if at >= width_end {
            return Err("its group widths run into its lane words".into());
        }
        if width > 32 {
            return Err(format!("a group is {width} bits wide, past 32"));
        }
        at += 1;
        widths.push(width);
    }
    let mut before = 0u64;
    for (segment, address) in addresses.iter().enumerate() {
        if segment > 0 {
            let groups = widths.iter().skip(LANES * (segment - 1)).take(LANES);
            before += groups.map(|w| u64::from(*w)).sum::<u64>();
        }
        if *address != before {
            return Err(format!(
                "segment {segment}'s address is {address}, where its groups before it are {before} bits"
            ));
        }
    }
    let sum: u64 = widths.iter().map(|w| u64::from(*w)).sum();
    if u64::from(lane_words) != sum.div_ceil(32) {
        return Err(format!(
            "it states {lane_words} words a lane for groups of {sum} bits in all"
        ));
    }
    let lane = |j: usize| -> Vec<u32> {
        (0..usize::from(lane_words))
            .map(|r| words[lanes_at + r * LANES + j])
            .collect()
    };
    let lanes: Vec<Vec<u32>> = (0..LANES).map(lane).collect();
    let mut accelerations = Vec::with_capacity(groups * LANES);
    let mut bit = 0usize;
    for &width in &widths {
        for lane in &lanes {
            let code = bits(lane, bit, width).ok_or_else(|| past(bit))?;
            accelerations.push((code >> 1) as i64 ^ -((code & 1) as i64));
        }
        bit += width as usize;
    }
    let mut values = Vec::with_capacity(frames);
    for (segment, &initial) in initials.iter().enumerate() {
        let first = SEGMENT * segment;
        let last = segment + 1 == segments;
        let count = if last { steps - first } else { SEGMENT };
        let (mut value, mut velocity) = (initial, 0i64);
        values.push(value);
        for step in first..first + count {
            velocity += accelerations.get(step).copied().unwrap_or(0);
            value += velocity;
            if step + 1 < first + SEGMENT || last {
                values.push(value);
            }
        }
        if !last && value != initials[segment + 1] {
            return Err(format!(
                "segment {segment} ends on {value}, not on the next segment's initial {}",
                initials[segment + 1]
            ));
        }
    }
    Ok(values)
}

/// The constant words of the channel whose quantised steps are `q`, the inverse of
/// [`quantised`]: every width, address and initial the tightest that holds its value.
pub(super) fn encode(q: &[i64], scale: u32, offset: u32) -> Result<Vec<u32>, String> {
    let frames = q.len();
    if frames == 0 {
        return Err("it has no frame".into());
    }
    let (segments, groups) = (frames.div_ceil(SEGMENT), (frames - 1).div_ceil(LANES));
    let mut codes = vec![0u64; groups * LANES];
    for f in 1..frames {
        let start = SEGMENT * ((f - 1) / SEGMENT);
        let before = if f - 1 > start {
            q[f - 1] - q[f - 2]
        } else {
            0
        };
        let a = i128::from(q[f]) - i128::from(q[f - 1]) - i128::from(before);
        let code = if a >= 0 { 2 * a } else { -2 * a - 1 };
        codes[f - 1] = u64::try_from(code).map_err(|_| "a step is past 64 bits".to_string())?;
    }
    let bits = |v: u64| 64 - v.leading_zeros();
    let widths: Vec<u32> = (codes.chunks(LANES))
        .map(|g| bits(g.iter().copied().max().unwrap_or(0)))
        .collect();
    let initials: Vec<u64> = (0..segments)
        .map(|s| u64::try_from(q[SEGMENT * s]).map_err(|_| "a step is below 0".to_string()))
        .collect::<Result<_, _>>()?;
    let addresses: Vec<u64> = (0..segments)
        .map(|s| widths.iter().take(LANES * s).map(|w| u64::from(*w)).sum())
        .collect();
    let address_bits = bits(addresses.last().copied().unwrap_or(0));
    let initial_bits = bits(initials.iter().copied().max().unwrap_or(0));
    let total: u32 = widths.iter().sum();
    let lane_words = total.div_ceil(32);
    if widths.iter().any(|w| *w > 32) || initial_bits > 32 || address_bits > 32 || lane_words > 255
    {
        return Err(format!(
            "its codes need widths past 32 bits or {lane_words} words a lane, past 255"
        ));
    }
    let end = HEADER_BITS
        + segments * (address_bits + initial_bits) as usize
        + widths.iter().map(|w| *w as usize + 1).sum::<usize>();
    let mut words = vec![0u32; end.div_ceil(32)];
    let mut at = HEADER_BITS;
    for (value, n) in (addresses.iter().map(|a| (*a, address_bits)))
        .chain(initials.iter().map(|i| (*i, initial_bits)))
    {
        put(&mut words, at, n, value);
        at += n as usize;
    }
    for width in &widths {
        at += *width as usize;
        put(&mut words, at, 1, 1);
        at += 1;
    }
    let head = words.len();
    words[..4].copy_from_slice(&[
        (head + LANES * lane_words as usize) as u32,
        address_bits | initial_bits << 8 | lane_words << 16,
        scale,
        offset,
    ]);
    let mut lanes = vec![vec![0u32; lane_words as usize]; LANES];
    let mut bit = 0;
    for (g, width) in widths.iter().enumerate() {
        for (j, lane) in lanes.iter_mut().enumerate() {
            put(lane, bit, *width, codes[LANES * g + j]);
        }
        bit += *width as usize;
    }
    words.extend((0..lane_words as usize).flat_map(|r| lanes.iter().map(move |lane| lane[r])));
    Ok(words)
}

/// Sets the `n` low bits of `value` at bit `at` of `words`, which are zero there.
fn put(words: &mut [u32], at: usize, n: u32, value: u64) {
    for b in 0..n as usize {
        if value >> b & 1 == 1 {
            words[(at + b) / 32] |= 1 << ((at + b) % 32);
        }
    }
}

/// `n` bits (at most 32) at bit `at` of `words`; `None` past their end.
fn bits(words: &[u32], at: usize, n: u32) -> Option<u64> {
    if n == 0 {
        return Some(0);
    }
    let (word, shift) = (at / 32, at % 32);
    let low = u64::from(*words.get(word)?);
    let high = if shift + n as usize > 32 {
        u64::from(*words.get(word + 1)?)
    } else {
        0
    };
    Some(((low | high << 32) >> shift) & ((1u64 << n) - 1))
}
