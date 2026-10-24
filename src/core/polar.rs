//! Classification of the permanently frozen polar regions.
//!
//! The poles are covered by two things, and this module is the single place that
//! defines both of them:
//!
//! * **Polar land ice caps** — a solid, glaciated landmass at each pole, lifted
//!   above the water line by the terrain generator. These are genuine *land*:
//!   the atlas reports their elevation, and every land rule applies to them.
//! * **Frozen seas** — the drifting pack ice and, deeper towards the pole, the
//!   solid glacial ice shelf that form over the remaining open polar water.
//!   The atlas renders these as ice rather than open ocean, and mechanically
//!   they belong to the same impassable wasteland as the ice caps.
//!
//! Keeping the thresholds in one place guarantees the map painter, the hover
//! telemetry and the province generator all agree about where "ice" begins.

/// Sea-surface temperature (°C) below which open water becomes a solid glacial ice shelf.
pub const ICE_SHELF_TEMP: f32 = -8.0;

/// Sea-surface temperature (°C) below which open water freezes into pack ice.
pub const PACK_ICE_TEMP: f32 = -3.0;

/// True when a pixel is frozen sea — i.e. "polar land" as far as the atlas is
/// concerned — and therefore an impassable wasteland rather than open ocean.
#[inline(always)]
pub fn is_frozen_sea(temp: f32, elev: f32, sea_level: f32) -> bool {
    elev <= sea_level && temp < PACK_ICE_TEMP
}

/// Human-readable name of a frozen-sea surface (ice shelf or pack ice).
#[inline(always)]
pub fn frozen_sea_name(temp: f32) -> &'static str {
    if temp < ICE_SHELF_TEMP {
        "Polar Glacial Ice Shelf"
    } else {
        "Polar Pack Sea Ice"
    }
}

// ── Permanent polar land ice caps ────────────────────────────────────────────
//
// Both poles carry a solid, glaciated landmass — an Antarctica-like ice sheet in
// the south and a matching Arctic cap in the north. It is genuine *land*: the
// atlas reports its elevation (never a depth), and every land rule — altitude
// lapse, biomes, provinces, mandatory ice wasteland — applies to it. This is
// distinct from the drifting pack ice and shelf ice that still form over the
// open polar seas ([`is_frozen_sea`]).
//
// The cap is produced mechanically by the terrain generator, which lifts the
// polar heightmap above the water line using [`polar_cap_blend`]; the constants
// below are the single definition of where the ice cap lies and how high it
// rises, so the generator and any reader agree about the poles.

/// |latitude| (radians) at which the permanent polar land ice cap first rises
/// above the water line. ≈ 69.9°.
pub const POLAR_CAP_START_LAT: f32 = 1.22;

/// |latitude| (radians) beyond which the cap is a solid, high ice sheet.
/// ≈ 80.2°.
pub const POLAR_CAP_FULL_LAT: f32 = 1.40;

/// Heightmap elevation of the cap's interior (≈ +1 950 m through the datum).
pub const POLAR_CAP_ELEV: f32 = 0.53;

/// Heightmap elevation the cap starts from, just below the water line, so the
/// raised sheet always breaks the surface and forms a coastline.
pub const POLAR_CAP_BASE_ELEV: f32 = 0.37;

/// Smooth `0 → 1` ramp of the ice cap across the polar latitudes: `0` equator-
/// ward of [`POLAR_CAP_START_LAT`] and `1` in the solid interior beyond
/// [`POLAR_CAP_FULL_LAT`].
#[inline(always)]
pub fn polar_cap_blend(abs_lat: f32) -> f32 {
    let t = ((abs_lat - POLAR_CAP_START_LAT) / (POLAR_CAP_FULL_LAT - POLAR_CAP_START_LAT))
        .clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}