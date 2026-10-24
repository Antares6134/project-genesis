//! The planet's vertical datum — the single source of truth that turns the
//! engine's normalized heightmap coordinate into metres.
//!
//! Every stage of the pipeline works in a normalized heightmap coordinate:
//! `0.0` is the floor of the deepest trench, `1.0` the summit of the highest
//! peak, and the world ocean surface is pinned at [`SEA_LEVEL`]. Terrain maths
//! (erosion thresholds, slope costs, biome altitudes) stays in those units, but
//! everything a *human* ever reads — hover telemetry, hypsometric legends,
//! relief reports — is in metres, and all of it is derived from the constants
//! below so the figures can never drift apart again.
//!
//! The scale is deliberately piecewise, exactly as on a real hypsometric chart:
//! continental relief above the water line and abyssal relief below it cover
//! very different vertical ranges, so a single linear factor would either flatten
//! the mountains or make the trenches absurd. A heightmap unit is therefore worth
//! [`LAND_M_PER_UNIT`] above sea level and [`OCEAN_M_PER_UNIT`] below it.
//!
//! With the default constants the datum reads:
//!
//! | heightmap | metres | example |
//! | :--- | :--- | :--- |
//! | `1.00` | `+9 000` | highest peak |
//! | `0.45` | `+750` | continental upland |
//! | `0.40` | `0` | sea level (shoreline) |
//! | `0.39` | `-275` | continental shelf break |
//! | `0.25` | `-4 125` | abyssal plain |
//! | `0.00` | `-11 000` | deepest trench |

/// Heightmap coordinate of the world ocean surface — the 0 m datum.
pub const SEA_LEVEL: f32 = 0.40;

/// Elevation, in metres, of heightmap coordinate `1.0`.
pub const HIGHEST_PEAK_M: f32 = 9_000.0;

/// Depth, in metres, of heightmap coordinate `0.0`.
pub const DEEPEST_TRENCH_M: f32 = 11_000.0;

/// Metres per heightmap unit above sea level (continental relief).
pub const LAND_M_PER_UNIT: f32 = HIGHEST_PEAK_M / (1.0 - SEA_LEVEL);

/// Metres per heightmap unit below sea level (abyssal relief).
pub const OCEAN_M_PER_UNIT: f32 = DEEPEST_TRENCH_M / SEA_LEVEL;

/// Signed elevation of a heightmap coordinate, in metres; negative below water.
#[inline(always)]
pub fn to_metres(unit: f32) -> f32 {
    if unit >= SEA_LEVEL {
        (unit - SEA_LEVEL) * LAND_M_PER_UNIT
    } else {
        (unit - SEA_LEVEL) * OCEAN_M_PER_UNIT
    }
}

/// Metres above the water line (`0.0` for anything at or below sea level).
#[inline(always)]
pub fn altitude_metres(unit: f32) -> f32 {
    ((unit - SEA_LEVEL) * LAND_M_PER_UNIT).max(0.0)
}

/// Water depth in metres (`0.0` on dry land).
#[inline(always)]
pub fn depth_metres(unit: f32) -> f32 {
    ((SEA_LEVEL - unit) * OCEAN_M_PER_UNIT).max(0.0)
}

/// Inverse of [`to_metres`] — the heightmap coordinate that sits at an exact
/// elevation, used by generators that reason in metres.
#[inline(always)]
pub fn from_metres(metres: f32) -> f32 {
    if metres >= 0.0 {
        SEA_LEVEL + metres / LAND_M_PER_UNIT
    } else {
        SEA_LEVEL + metres / OCEAN_M_PER_UNIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datum_is_pinned_at_the_documented_anchors() {
        assert!((to_metres(SEA_LEVEL) - 0.0).abs() < 1e-3);
        assert!((to_metres(1.0) - HIGHEST_PEAK_M).abs() < 1.0);
        assert!((to_metres(0.0) + DEEPEST_TRENCH_M).abs() < 1.0);
        assert!((from_metres(HIGHEST_PEAK_M) - 1.0).abs() < 1e-4);
        assert!((from_metres(-DEEPEST_TRENCH_M) - 0.0).abs() < 1e-4);
    }

    #[test]
    fn conversions_round_trip_on_both_sides_of_the_water_line() {
        for metres in [-11_000.0f32, -4_150.0, -200.0, -1.0, 0.0, 1.0, 250.0, 6_975.0, 9_000.0] {
            assert!((to_metres(from_metres(metres)) - metres).abs() < 0.5, "failed at {metres}");
        }
    }

    #[test]
    fn altitude_and_depth_are_clamped_to_their_own_element() {
        assert_eq!(altitude_metres(0.10), 0.0);
        assert_eq!(depth_metres(0.90), 0.0);
        assert!(depth_metres(0.30) > 2_000.0); // an abyssal heightmap value is kilometres deep
        assert!(altitude_metres(0.50) > 1_000.0); // and an inland value is kilometres high
    }
}
