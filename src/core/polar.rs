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

use crate::core::sphere::row_latitude_rad;
use crate::core::Grid;
use noise::{NoiseFn, OpenSimplex};

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

// ── A world's polar geography, as one value ──────────────────────────────────

/// How wide a latitude band the ice cap spans, in radians: the gap the engine
/// has always drawn between [`POLAR_CAP_START_LAT`] and [`POLAR_CAP_FULL_LAT`].
///
/// A reader who moves the cap edge moves the whole ramp with it: what a slider
/// chooses is *where* the sheet begins, not how steeply it rises. That is what
/// keeps a world with the cap pushed equatorward looking like an ice age rather
/// than like a planet with a different polar geometry.
pub const POLAR_CAP_SPAN_RAD: f32 = POLAR_CAP_FULL_LAT - POLAR_CAP_START_LAT;

/// The same span for the belt of permanent arctic ice
/// ([`POLAR_BELT_START_LAT`] → [`POLAR_BELT_FULL_LAT`]).
pub const POLAR_BELT_SPAN_RAD: f32 = POLAR_BELT_FULL_LAT - POLAR_BELT_START_LAT;

/// Where one world's polar ice lies, in radians of `|latitude|`.
///
/// Four latitudes, not four loose arguments: the land ice cap (where it starts
/// to rise, where it is a solid high sheet) and the belt of permanent arctic ice
/// (the same two). The terrain generator, the province generator and the atlas's
/// own ice rules all travel together through this one value, so no reader can
/// end up with a cap in one place and a belt in another.
///
/// The defaults are the engine's own constants, so a world grown with the panel
/// untouched is the world this program has always drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolarConfig {
    /// `|latitude|` at which the permanent land ice cap first rises above the
    /// water line.
    pub cap_start_lat: f32,
    /// `|latitude|` beyond which the cap is a solid, high ice sheet.
    pub cap_full_lat: f32,
    /// `|latitude|` at which the belt of permanent arctic ice begins.
    pub belt_start_lat: f32,
    /// `|latitude|` beyond which the belt is solid ice whatever the noise.
    pub belt_full_lat: f32,
}

impl Default for PolarConfig {
    fn default() -> Self {
        Self {
            cap_start_lat: POLAR_CAP_START_LAT,
            cap_full_lat: POLAR_CAP_FULL_LAT,
            belt_start_lat: POLAR_BELT_START_LAT,
            belt_full_lat: POLAR_BELT_FULL_LAT,
        }
    }
}

impl PolarConfig {
    /// The polar geography a reader asked for, in **degrees of latitude**.
    ///
    /// The two edges a reader chooses are where the land cap begins and where
    /// the sea ice begins; the solid interior follows at the engine's own span.
    /// Two rules are enforced here rather than in the panel, because they are
    /// properties of the planet and not of the widget:
    ///
    /// * **The sea freezes first.** Pack ice is water below −3 °C, a glaciated
    ///   sheet is land under an ice cap, so the sea-ice edge always lies at or
    ///   equatorward of the land-ice edge. A belt asked for poleward of the cap
    ///   is pulled back to it.
    /// * **The ramp always has a width.** An edge asked for at the pole itself
    ///   still leaves a hair of latitude for the sheet to climb through, so the
    ///   blend below can never divide by zero.
    pub fn from_degrees(cap_start_deg: f32, belt_start_deg: f32) -> Self {
        let pole = std::f32::consts::FRAC_PI_2;
        // A hair short of the pole: the ramp needs somewhere to climb.
        let farthest = pole - 0.001;
        let cap_start = cap_start_deg.to_radians().clamp(0.0, farthest);
        let belt_start = belt_start_deg.to_radians().clamp(0.0, cap_start);
        Self {
            cap_start_lat: cap_start,
            cap_full_lat: (cap_start + POLAR_CAP_SPAN_RAD).min(farthest),
            belt_start_lat: belt_start,
            belt_full_lat: (belt_start + POLAR_BELT_SPAN_RAD).min(farthest),
        }
    }

    /// Smooth `0 → 1` ramp of the ice cap across the polar latitudes: `0`
    /// equatorward of [`PolarConfig::cap_start_lat`] and `1` in the solid
    /// interior beyond [`PolarConfig::cap_full_lat`].
    #[inline(always)]
    pub fn cap_blend(&self, abs_lat: f32) -> f32 {
        let span = (self.cap_full_lat - self.cap_start_lat).max(1.0e-4);
        let t = ((abs_lat - self.cap_start_lat) / span).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

// ── Permanent polar land ice ─────────────────────────────────────────────────
//
// Beyond a certain latitude a landmass stops being merely cold and becomes
// permanent ice: an arctic waste that never thaws into tundra at its shore and
// never carries a province. That belt is defined here, once, for the whole world:
//
// * it is genuine **land** — the atlas reports its elevation, never a depth;
// * it must read as ice on *every* scope. The province generator, the biome
//   palette, the climate and terrain scopes and the hover telemetry all decide
//   "ice or tundra" by reading the temperature grid, so the belt's surface
//   temperature is pinned ([`freeze_polar_ice`]) and the grid itself learns the
//   difference;
// * provinces may only creep to its **equatorward** edge. That edge is ragged
//   rather than a perfect latitude circle, so a pole-connected continent keeps a
//   handful of provinces right at the rim of the belt — but none run deep into
//   it;
// * a landmass that never reaches a pole has no such rim to begin with. Inside
//   the belt a lone arctic island is solid ice from shore to shore.

/// |latitude| (radians) at which the permanent polar belt begins. ≈ 65.9°.
///
/// Equatorward of this the climate is merely cold; poleward of it land is
/// permanent arctic ice.
pub const POLAR_BELT_START_LAT: f32 = 1.15;

/// |latitude| (radians) beyond which the belt is solid ice whatever the noise.
/// ≈ 72.2°.
pub const POLAR_BELT_FULL_LAT: f32 = 1.26;

/// Amplitude, in ramp units, of the noise that makes the belt's equatorward edge
/// ragged instead of a perfect latitude circle.
pub const POLAR_ICE_EDGE_NOISE: f32 = 0.22;

/// Ramp threshold at which the belt freezes, before the edge noise is added.
///
/// With the constants above the edge lands between ≈ 66.4° and ≈ 69.2°: the belt
/// swallows every polar coast, yet a pole-connected continent may still keep a
/// few provinces right at its rim.
pub const POLAR_ICE_EDGE_CUTOFF: f32 = 0.30;

/// Surface temperature (°C) pinned onto the belt by [`freeze_polar_ice`].
///
/// Below [`ICE_SHELF_TEMP`] by a comfortable margin, so the atlas's own
/// micro-climate jitter (±5.5 °C) can never nudge the ice back into the tundra
/// palette.
pub const POLAR_ICE_SURFACE_TEMP: f32 = -15.0;

/// Noise seed offset shared by the belt's ragged edge, so the mask the province
/// generator reads and the ice the painter draws can never disagree.
const POLAR_ICE_EDGE_SALT: u64 = 9999;

/// Smooth `0 → 1` ramp: `0.0` at `lo`, `1.0` at `hi`.
#[inline(always)]
fn ramp(x: f32, lo: f32, hi: f32) -> f32 {
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// The permanent polar ice of a whole world, as a `true`/`false` grid.
///
/// `true` for every land pixel that is permanent arctic ice: the polar belt
/// ([`PolarConfig::belt_start_lat`]) with its ragged equatorward edge and — in
/// full — the belt of any landmass that never reaches a pole. Every downstream
/// stage that has to agree about where the ice begins reads this one grid: the
/// province generator turns it into mandatory wasteland, and [`freeze_polar_ice`]
/// writes it into the temperature so the atlas paints it.
///
/// Each pole carries one landmass that *contains* the pole — the generator raises
/// the ice sheet above the water line at every longitude beyond
/// [`PolarConfig::cap_full_lat`]. That landmass is *pole-connected*, and there the
/// belt's edge is allowed to wander, so a continent keeps a ragged handful of
/// provinces right at the rim of the ice. Land cut off from every pole gets no
/// such rim.
///
/// Connectivity is a four-connected flood fill over land, wrapping in longitude
/// exactly like the terrain, hydrology and province stages do.
pub fn permanent_polar_ice(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    sea_level: f32,
    seed: u64,
    polar: &PolarConfig,
) -> Grid<bool> {
    let w = heightmap.width;
    let h = heightmap.height;
    let elev = heightmap.raw_slice();
    let lake = is_lake.raw_slice();
    let edge_noise = OpenSimplex::new(seed as u32 + POLAR_ICE_EDGE_SALT as u32);

    // 1. Flood-fill every landmass that holds a polar cap. Beyond
    //    `cap_full_lat` the generator lifts the sheet above the water line at
    //    every longitude, so those rows are guaranteed land and seed the fill.
    let mut pole_connected = vec![false; w * h];
    let mut stack: Vec<u32> = Vec::new();
    for y in 0..h {
        if row_latitude_rad(y as f32, h).abs() < polar.cap_full_lat {
            continue;
        }
        let row = y * w;
        for x in 0..w {
            let i = row + x;
            if elev[i] <= sea_level || lake[i] {
                continue;
            }
            pole_connected[i] = true;
            stack.push(i as u32);
        }
    }

    while let Some(idx) = stack.pop() {
        let i = idx as usize;
        let y = i / w;
        let x = i % w;
        let mut neighbours = [usize::MAX; 4];
        neighbours[0] = y * w + if x == 0 { w - 1 } else { x - 1 };
        neighbours[1] = y * w + if x + 1 == w { 0 } else { x + 1 };
        if y > 0 { neighbours[2] = i - w; }
        if y + 1 < h { neighbours[3] = i + w; }
        for &j in &neighbours {
            if j == usize::MAX || pole_connected[j] {
                continue;
            }
            if elev[j] <= sea_level || lake[j] {
                continue;
            }
            pole_connected[j] = true;
            stack.push(j as u32);
        }
    }

    // 2. Freeze the belt. Its equatorward edge wanders with the noise, unless the
    //    landmass never reaches a pole — then there is no province-bearing rim to
    //    spare, and the ice is solid.
    let mut ice = Grid::filled_with(w, h, false);
    {
        let out = ice.raw_slice_mut();
        for y in 0..h {
            let lat = row_latitude_rad(y as f32, h);
            let abs_lat = lat.abs();
            if abs_lat < polar.belt_start_lat {
                continue;
            }
            let row = y * w;
            let base = ramp(abs_lat, polar.belt_start_lat, polar.belt_full_lat);
            for x in 0..w {
                let i = row + x;
                if elev[i] <= sea_level || lake[i] {
                    continue;
                }
                if !pole_connected[i] {
                    out[i] = true;
                    continue;
                }
                let lon = (x as f32 / w as f32 - 0.5) * std::f32::consts::TAU;
                let nx = (lat.cos() * lon.cos()) as f64;
                let ny = lat.sin() as f64;
                let nz = (lat.cos() * lon.sin()) as f64;
                let wobble = edge_noise.get([nx * 26.0, ny * 26.0, nz * 26.0]) as f32;
                if base + wobble * POLAR_ICE_EDGE_NOISE > POLAR_ICE_EDGE_CUTOFF {
                    out[i] = true;
                }
            }
        }
    }
    ice
}

/// Pins the surface temperature of [`permanent_polar_ice`] into the ice band.
///
/// A glaciated surface keeps its own climate — high albedo, meltwater and cold
/// air pooling at the foot of the sheet — so its mean temperature never climbs
/// back into the tundra range. Writing that into the temperature grid is what
/// keeps the atlas honest: the province generator, the biome palette, the climate
/// and terrain scopes and the hover telemetry all already read *temperature* to
/// decide where ice begins, so once the grid agrees they cannot disagree with
/// each other.
pub fn freeze_polar_ice(temp_grid: &mut Grid<f32>, polar_ice: &Grid<bool>) {
    for (temp, &frozen) in temp_grid.raw_slice_mut().iter_mut().zip(polar_ice.raw_slice()) {
        if frozen {
            *temp = temp.min(POLAR_ICE_SURFACE_TEMP);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEA_LEVEL: f32 = 0.40;
    const W: usize = 16;
    /// Roughly one row per degree of latitude, so the belt's boundaries land on
    /// rows that are easy to reason about: rows 0–21 and 159–179 are solidly
    /// inside the belt, rows 24–155 are solidly outside it, and rows 22–23 and
    /// 156–158 sit on the ragged edge itself.
    const H: usize = 180;

    fn world(dry: impl Fn(usize, usize) -> bool) -> (Grid<f32>, Grid<bool>) {
        let mut heightmap = Grid::filled_with(W, H, 0.20f32);
        for y in 0..H {
            for x in 0..W {
                if dry(x, y) {
                    heightmap.set(x as isize, y as isize, 0.50);
                }
            }
        }
        (heightmap, Grid::filled_with(W, H, false))
    }

    fn is_ice(ice: &Grid<bool>, x: usize, y: usize) -> bool {
        *ice.get(x as isize, y as isize)
    }

    #[test]
    fn the_belt_freezes_a_pole_connected_continent_up_to_its_equatorward_edge() {
        // A cap chunk in the cap latitudes (rows 0–3) with a coast running
        // equatorward from it down to row 40.
        let (heightmap, is_lake) = world(|x, y| (4..=7).contains(&x) && y <= 40);
        let ice = permanent_polar_ice(&heightmap, &is_lake, SEA_LEVEL, 42, &PolarConfig::default());

        assert!(is_ice(&ice, 5, 5), "deep polar land must be permanent ice");
        assert!(is_ice(&ice, 5, 20), "the belt must reach its equatorward edge");
        assert!(!is_ice(&ice, 5, 30), "land outside the belt must stay habitable");
        assert!(!is_ice(&ice, 5, 40), "land outside the belt must stay habitable");
        assert!(!is_ice(&ice, 0, 5), "open water never freezes here");
    }

    #[test]
    fn land_cut_off_from_the_poles_is_solid_ice_across_the_whole_belt() {
        // A cap chunk, one island straddling the belt's ragged edge, and one
        // island wholly equatorward of the belt.
        let (heightmap, is_lake) = world(|x, y| {
            ((4..=7).contains(&x) && y <= 3)
                || ((12..=13).contains(&x) && (20..=28).contains(&y))
                || ((2..=3).contains(&x) && (30..=34).contains(&y))
        });
        let ice = permanent_polar_ice(&heightmap, &is_lake, SEA_LEVEL, 42, &PolarConfig::default());

        // The straddling island: polar half solid ice, sub-polar half spared —
        // even where a pole-connected continent would sit on the ragged edge.
        assert!(is_ice(&ice, 12, 22), "an island cut off from the pole freezes whole");
        assert!(is_ice(&ice, 13, 23), "an island cut off from the pole freezes whole");
        assert!(!is_ice(&ice, 12, 26), "the island's sub-polar half stays habitable");
        assert!(!is_ice(&ice, 2, 32), "land equatorward of the belt is unaffected");
    }

    // ── THE POLAR CONFIG ─────────────────────────────────────────────────────

    /// A panel that has never been touched must draw the world the engine has
    /// always drawn: the default config is the constants, to the last bit.
    #[test]
    fn the_default_config_is_the_engine_constants() {
        let polar = PolarConfig::default();
        assert_eq!(polar.cap_start_lat, POLAR_CAP_START_LAT);
        assert_eq!(polar.cap_full_lat, POLAR_CAP_FULL_LAT);
        assert_eq!(polar.belt_start_lat, POLAR_BELT_START_LAT);
        assert_eq!(polar.belt_full_lat, POLAR_BELT_FULL_LAT);
    }

    /// The two edges a reader may pick are in degrees; the ramp between them
    /// keeps the engine's own width, and the sea-ice edge never runs poleward of
    /// the land-ice edge whatever the reader asks for.
    #[test]
    fn a_worlds_edges_follow_its_degrees_and_never_cross() {
        let polar = PolarConfig::from_degrees(69.9, 65.9);
        assert!((polar.cap_start_lat.to_degrees() - 69.9).abs() < 1.0e-3);
        assert!((polar.belt_start_lat.to_degrees() - 65.9).abs() < 1.0e-3);
        // The spans travel with the edges, so the interior is as wide as ever.
        assert!((polar.cap_full_lat - polar.cap_start_lat - POLAR_CAP_SPAN_RAD).abs() < 1.0e-5);
        assert!((polar.belt_full_lat - polar.belt_start_lat - POLAR_BELT_SPAN_RAD).abs() < 1.0e-5);

        // A belt asked for *poleward* of the cap is pulled back to it: the sea
        // freezes before the land is buried, on every world.
        let crossed = PolarConfig::from_degrees(60.0, 80.0);
        assert!(crossed.belt_start_lat <= crossed.cap_start_lat);
    }

    /// The blend runs 0 → 1 across the cap and never divides by zero, even when
    /// the reader drags the edge to the pole itself.
    #[test]
    fn the_cap_ramp_stays_finite_at_any_edge() {
        let polar = PolarConfig::from_degrees(89.9, 89.9);
        assert!(polar.cap_blend(0.0).is_finite());
        assert_eq!(polar.cap_blend(0.0), 0.0, "the equator is never ice");
        assert_eq!(polar.cap_blend(std::f32::consts::FRAC_PI_2), 1.0);

        let earth = PolarConfig::default();
        assert_eq!(earth.cap_blend(0.0), 0.0);
        assert_eq!(earth.cap_blend(1.0), 0.0, "70° of latitude is inside the ramp's start");
        assert_eq!(earth.cap_blend(1.5708), 1.0, "the pole is solid sheet");
    }

    /// An ice age is a cap pushed equatorward, and the world must report it: the
    /// cap reaches ground that was temperate before.
    #[test]
    fn moving_the_cap_equatorward_buries_formerly_temperate_ground() {
        let temperate_lat = 55.0f32.to_radians();
        assert_eq!(PolarConfig::default().cap_blend(temperate_lat), 0.0);
        assert!(PolarConfig::from_degrees(45.0, 40.0).cap_blend(temperate_lat) > 0.9);
    }

    #[test]
    fn freezing_pins_temperatures_into_the_ice_band_and_never_warms() {
        let mut temp = Grid::filled_with(W, H, 6.0f32);
        temp.set(1, 1, -40.0);
        let mut mask = Grid::filled_with(W, H, false);
        mask.set(1, 1, true);
        mask.set(2, 2, true);

        freeze_polar_ice(&mut temp, &mask);

        assert_eq!(*temp.get(1, 1), -40.0, "already colder ground is left alone");
        assert_eq!(*temp.get(2, 2), POLAR_ICE_SURFACE_TEMP);
        assert!(POLAR_ICE_SURFACE_TEMP < ICE_SHELF_TEMP);
        assert_eq!(*temp.get(3, 3), 6.0, "unmarked ground is untouched");
    }
}