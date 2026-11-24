//! Overflow-safe integer timescale arithmetic for ISO-BMFF and EBML timestamps.
//!
//! Clean-room, MIT-licensed implementation derived only from the public container
//! definitions (ISO/IEC 14496-12 `mvhd`/`mdhd`/`stts`/`ctts`/`elst` and the
//! Matroska `TimestampScale` element). No third-party code is copied.
//!
//! Why this module exists: converting a tick count between two time bases with
//! `f64` (`ticks as f64 / timescale as f64 * movie_timescale as f64`) silently
//! loses precision once the product exceeds 2^53 — a 2-hour asset at a 90 kHz
//! clock is already ~6.5e8 ticks, and 10+ hour assets at 48 kHz exceed 1.7e9
//! ticks, where the fractional part of the product no longer round-trips. All
//! conversions here run on `u128`/`i128` integers, so:
//!
//! * the median of a 10-hour timeline never drifts by a frame;
//! * a fractional base such as `30000/1001` stays exact;
//! * `u64` tick counts can no longer wrap into a wrong duration.
//!
//! `f64` still enters the module at exactly one place — [`secs_to_nanos`] — where
//! a user-supplied `--start`/`--duration` value is quantised to integer
//! nanoseconds once, before any container math happens.

use std::cmp::Ordering;

/// Nanoseconds in one second; the common denominator for every conversion here.
pub const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Tie-breaking rule used when a rescaled value is not an exact integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// Truncate towards zero (container durations, offsets).
    Floor,
    /// Round half away from zero (durations shown to a player, `mvhd`/`mdhd`).
    Nearest,
    /// Round up (end-of-clip boundaries that must cover the last sample).
    Ceil,
}

impl Round {
    #[inline]
    fn bump(self, _quotient: u128, remainder: u128, divisor: u128) -> u128 {
        match self {
            Round::Floor => 0,
            Round::Ceil => u128::from(remainder != 0),
            Round::Nearest => u128::from(remainder.saturating_mul(2) >= divisor),
        }
    }
}

/// `value * num / den` on 128-bit integers, saturating instead of wrapping.
///
/// `den == 0` is treated as `1` so a malformed `mdhd` (`timescale = 0`) degrades
/// to "one tick per unit" instead of panicking or dividing by zero.
#[inline]
pub fn rescale_u64(value: u64, num: u64, den: u64, rounding: Round) -> u64 {
    let divisor = u128::from(den.max(1));
    let product = u128::from(value).saturating_mul(u128::from(num));
    let quotient = product / divisor;
    let remainder = product % divisor;
    let total = quotient.saturating_add(rounding.bump(quotient, remainder, divisor));
    u64::try_from(total).unwrap_or(u64::MAX)
}

/// Rescale and clamp into a 32-bit container field (`tkhd`/`mdhd` version 0).
#[inline]
pub fn rescale_u32(value: u64, num: u64, den: u64, rounding: Round) -> u32 {
    u32::try_from(rescale_u64(value, num, den, rounding)).unwrap_or(u32::MAX)
}

/// Quantise user-supplied seconds to integer nanoseconds (the only float entry point).
///
/// Negative input is clamped to zero; non-finite input returns zero so a bad CLI
/// argument can never poison the rest of the timeline.
pub fn secs_to_nanos(secs: f64) -> u64 {
    if !secs.is_finite() || secs <= 0.0 {
        return 0;
    }
    let nanos = secs * NANOS_PER_SEC as f64;
    if nanos >= u64::MAX as f64 {
        u64::MAX
    } else {
        nanos.round() as u64
    }
}

/// Nanoseconds back to seconds, for reporting only (never for container math).
pub fn nanos_to_secs(nanos: u64) -> f64 {
    nanos as f64 / NANOS_PER_SEC as f64
}

/// `ticks` of a `1/timescale` clock expressed in nanoseconds.
#[inline]
pub fn ticks_to_nanos(ticks: u64, timescale: u32, rounding: Round) -> u64 {
    rescale_u64(ticks, NANOS_PER_SEC, u64::from(timescale.max(1)), rounding)
}

/// Nanoseconds expressed in ticks of a `1/timescale` clock.
#[inline]
pub fn nanos_to_ticks(nanos: u64, timescale: u32, rounding: Round) -> u64 {
    rescale_u64(nanos, u64::from(timescale.max(1)), NANOS_PER_SEC, rounding)
}

/// Nanoseconds expressed in units of `tick_ns` (EBML `TimestampScale`).
#[inline]
pub fn nanos_to_scaled(nanos: u64, tick_ns: u64, rounding: Round) -> u64 {
    rescale_u64(nanos, 1, tick_ns.max(1), rounding)
}

/// Milliseconds (the `mvhd`/`mehd`/`elst` time base) expressed in media ticks.
#[inline]
pub fn ms_to_ticks(ms: u32, timescale: u32, rounding: Round) -> u64 {
    rescale_u64(u64::from(ms), u64::from(timescale.max(1)), 1000, rounding)
}

/// Media ticks expressed in milliseconds, saturated to the 32-bit `mvhd` field.
#[inline]
pub fn ticks_to_ms(ticks: u64, timescale: u32, rounding: Round) -> u32 {
    rescale_u32(ticks, 1000, u64::from(timescale.max(1)), rounding)
}

/// Fractional seconds (`f64`) expressed in milliseconds, saturated to `u32`.
#[inline]
pub fn secs_to_ms(secs: f64) -> u32 {
    let ms = if secs.is_finite() && secs > 0.0 {
        (secs * 1000.0).round()
    } else {
        0.0
    };
    if ms >= u32::MAX as f64 {
        u32::MAX
    } else {
        ms as u32
    }
}

/// Compare `a / a_ts` against `b / b_ts` without leaving the integers.
///
/// Used to order DASH fragments coming from tracks with different time bases:
/// `f64` seconds compare incorrectly for fragments a few ticks apart.
#[inline]
pub fn ratio_cmp(a: u64, a_ts: u32, b: u64, b_ts: u32) -> Ordering {
    let lhs = u128::from(a).saturating_mul(u128::from(b_ts.max(1)));
    let rhs = u128::from(b).saturating_mul(u128::from(a_ts.max(1)));
    lhs.cmp(&rhs)
}

/// `a / a_ts <= b / b_ts` in exact integer arithmetic.
#[inline]
pub fn ratio_leq(a: u64, a_ts: u32, b: u64, b_ts: u32) -> bool {
    ratio_cmp(a, a_ts, b, b_ts) != Ordering::Greater
}

/// Total duration of a sample run, in the sample's own media time base.
///
/// `stts`/`trun` store `u32` durations; summing them in `u64` keeps a 10+ hour
/// track (≈1.7e9 ticks at 48 kHz) exact while `u32` would wrap after ~24 hours
/// at a 48 kHz clock and much earlier at 90 kHz.
pub fn sum_sample_durations<I: IntoIterator<Item = u32>>(durations: I) -> u64 {
    durations
        .into_iter()
        .map(|d| u64::from(d.max(1)))
        .fold(0u64, |acc, d| acc.saturating_add(d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescale_keeps_exact_fractional_timescale() {
        // 29.97 fps (30000/1001) second 0 -> 1 s expressed in a 90 kHz clock.
        let ticks = nanos_to_ticks(NANOS_PER_SEC, 90_000, Round::Nearest);
        assert_eq!(ticks, 90_000);
        // 10 hours in a 48 kHz clock: f64 would lose the unit digit.
        let ten_hours_ns = 10 * 3600 * NANOS_PER_SEC;
        let ticks = nanos_to_ticks(ten_hours_ns, 48_000, Round::Nearest);
        assert_eq!(ticks, 1_728_000_000);
        // And back again, bit-exact.
        assert_eq!(ticks_to_nanos(ticks, 48_000, Round::Nearest), ten_hours_ns);
    }

    #[test]
    fn rescale_saturates_instead_of_wrapping() {
        assert_eq!(rescale_u64(u64::MAX, u64::MAX, 1, Round::Floor), u64::MAX);
        assert_eq!(rescale_u32(u64::MAX, 1000, 1, Round::Floor), u32::MAX);
    }

    #[test]
    fn rescale_survives_zero_denominator() {
        assert_eq!(rescale_u64(7, 3, 0, Round::Floor), 21);
        // A zero `mdhd` timescale degrades to a 1 Hz clock (one tick per second).
        assert_eq!(ms_to_ticks(1000, 0, Round::Nearest), 1);
        assert_eq!(ticks_to_ms(1, 0, Round::Nearest), 1000);
    }

    #[test]
    fn rounding_modes_agree_on_exact_values_and_differ_on_remainders() {
        // 3 ticks of a 2 Hz clock = 1.5 s
        assert_eq!(ticks_to_nanos(3, 2, Round::Floor), 1_500_000_000);
        // 1 tick of a 3 Hz clock = 333.333.. ms
        assert_eq!(ticks_to_ms(1, 3, Round::Floor), 333);
        assert_eq!(ticks_to_ms(1, 3, Round::Ceil), 334);
        assert_eq!(ticks_to_ms(1, 3, Round::Nearest), 333);
        assert_eq!(ticks_to_ms(2, 3, Round::Nearest), 667);
    }

    #[test]
    fn ratio_compare_is_exact_across_time_bases() {
        // 1/1000 == 90/90000 exactly; f64 equality here is fragile.
        assert_eq!(ratio_cmp(1, 1000, 90, 90_000), Ordering::Equal);
        assert!(ratio_leq(1, 1000, 90, 90_000));
        // 30000/1001 ≈ 29.97 s is shorter than 30 s; the cross product proves it exactly.
        assert!(ratio_leq(30_000, 1001, 30, 1));
        assert!(!ratio_leq(30, 1, 30_000, 1001));
        assert!(ratio_leq(29_999, 1001, 30, 1));
    }

    #[test]
    fn sample_duration_sum_does_not_wrap_u32() {
        // 900_000 samples of 5000 ticks = 4.5e9 ticks, past the u32 ceiling.
        let total = sum_sample_durations(std::iter::repeat_n(5000u32, 900_000));
        assert_eq!(total, 4_500_000_000);
        assert!(total > u64::from(u32::MAX));
    }

    #[test]
    fn secs_to_nanos_clamps_garbage() {
        assert_eq!(secs_to_nanos(-5.0), 0);
        assert_eq!(secs_to_nanos(f64::NAN), 0);
        assert_eq!(secs_to_nanos(f64::INFINITY), 0);
        assert_eq!(secs_to_nanos(1.5), 1_500_000_000);
        assert_eq!(secs_to_ms(f64::NAN), 0);
    }

    #[test]
    fn ebml_scaled_ticks_match_nanosecond_clock() {
        // Matroska default TimestampScale = 1_000_000 ns (1 ms ticks).
        assert_eq!(
            nanos_to_scaled(NANOS_PER_SEC, 1_000_000, Round::Floor),
            1000
        );
        assert_eq!(
            nanos_to_scaled(10 * 3600 * NANOS_PER_SEC, 1_000_000, Round::Floor),
            36_000_000
        );
    }
}
