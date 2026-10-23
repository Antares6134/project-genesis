//! Classification of the permanently frozen polar regions.
//!
//! The polar seas freeze first into drifting pack ice and, deeper towards the
//! poles, into a solid glacial ice shelf. On the atlas both of these read as
//! "polar land" rather than open ocean, and mechanically they belong to the
//! same impassable wasteland as the ice-capped polar continents. Keeping the
//! thresholds in one place guarantees the map painter, the hover telemetry and
//! the province generator all agree about where "ice" begins.

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