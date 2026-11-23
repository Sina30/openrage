//! IEEE-754 binary16, in both directions.

pub fn half_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exponent = ((bits >> 10) & 0x1F) as u32;
    let mantissa = (bits & 0x3FF) as u32;
    match exponent {
        // ⚠ A subnormal half is `mantissa * 2^-24` exactly. Renormalising it
        // into an f32 exponent by shifting is where the off-by-one lives.
        0 => {
            let magnitude = mantissa as f32 * 5.960_464_5e-8;
            if sign == 1 {
                -magnitude
            } else {
                magnitude
            }
        }
        0x1F => f32::from_bits((sign << 31) | (0xFF << 23) | (mantissa << 13)),
        _ => f32::from_bits((sign << 31) | ((exponent + 127 - 15) << 23) | (mantissa << 13)),
    }
}

pub fn f32_to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x7F_FFFF;

    if exponent == 0xFF {
        // Inf or NaN; keep NaN a NaN rather than letting it become Inf.
        let payload = if mantissa != 0 { 0x200 } else { 0 };
        return sign | 0x7C00 | payload;
    }
    let unbiased = exponent - 127;
    if unbiased > 15 {
        return sign | 0x7C00;
    }
    if unbiased < -24 {
        return sign;
    }
    if unbiased < -14 {
        // Subnormal half: round the exact value to a multiple of 2^-24.
        let scaled = (value.abs() / 5.960_464_5e-8).round() as u32;
        return sign | (scaled.min(0x3FF) as u16);
    }
    let half_exponent = ((unbiased + 15) as u16) << 10;
    // Round to nearest on the 13 bits being discarded; a tie rounds away
    // from zero, not to even.
    let round = (mantissa >> 12) & 1;
    let half_mantissa = ((mantissa >> 13) + round) as u16;
    if half_mantissa > 0x3FF {
        return sign | (half_exponent + (1 << 10));
    }
    sign | half_exponent | half_mantissa
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_decodes_the_values_that_have_exact_representations() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3C00), 1.0);
        assert_eq!(half_to_f32(0xBC00), -1.0);
        assert_eq!(half_to_f32(0x4000), 2.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert!(half_to_f32(0x7C00).is_infinite());
        assert!(half_to_f32(0x7E00).is_nan());
        // Smallest subnormal: 2^-24.
        assert!((half_to_f32(0x0001) - 5.960_464_5e-8).abs() < 1e-14);
    }

    #[test]
    fn a_half_survives_the_trip_out_and_back() {
        for raw in 0u16..=u16::MAX {
            let value = half_to_f32(raw);
            if value.is_nan() {
                assert!(f32_to_half(value) & 0x7C00 == 0x7C00);
                continue;
            }
            assert_eq!(
                f32_to_half(value),
                // Negative zero and positive zero both decode to a zero
                // that re-encodes with its own sign.
                raw,
                "{raw:#06X} decoded to {value} and re-encoded elsewhere"
            );
        }
    }

    /// A half widened to `f64` and narrowed back is the same half, which is
    /// what lets a two-byte field live in an `f64`-shaped value model.
    #[test]
    fn a_half_survives_a_detour_through_f64() {
        for raw in 0u16..=u16::MAX {
            let wide = half_to_f32(raw) as f64;
            if wide.is_nan() {
                continue;
            }
            assert_eq!(
                f32_to_half(wide as f32),
                raw,
                "{raw:#06X} did not come back through f64"
            );
        }
    }
}
