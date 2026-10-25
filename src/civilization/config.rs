/// Tuning for the dawn of history — the stage that decides where civilizations
/// rise and how far they spread.
///
/// Every distance here is in **map pixels** (so a rule reads the same on a 2K
/// test world as on the 8K production plate) and every `*_frac` value is a
/// fraction of the map *width*: on an equirectangular canvas the width spans the
/// whole equator, so it is the only honest ruler for "how far apart are two
/// cradles on this planet". The centimetre-precise figures a reader sees —
/// kilometres, km², people — are produced from these by
/// [`crate::core::sphere`], never by counting pixels.
#[derive(Clone, Debug)]
pub struct CivilizationConfig {
    /// Fewest civilizations the world should carry. Also the floor the cradle
    /// selector relaxes its spacing rules to reach.
    pub min_civilizations: usize,
    /// Most civilizations the world may carry.
    pub max_civilizations: usize,

    /// Minimum spacing between two cradles *inside one region*, as a fraction of
    /// map width.
    ///
    /// Real contemporaries stood shoulder to shoulder — Greeks, Thracians,
    /// Phrygians and Hittites within fifteen hundred kilometres — so two realms
    /// may share a region. A few provinces' worth of ground between them is
    /// enough: what is forbidden is the same patch of floodplain, not the same
    /// country.
    pub cradle_separation_frac: f32,

    /// The absolute floor between any two cradles, as a fraction of map width.
    /// Small enough that neighbours can crowd, large enough that two realms never
    /// share their ground.
    pub cradle_floor_frac: f32,

    /// Most cradles one region may hold — a shared valley can carry rivals.
    pub cradle_max_per_region: usize,

    /// Most cradles one superregion may hold. This is the quota that puts
    /// neighbours and cultural cousins on the same map.
    pub cradle_max_per_superregion: usize,

    /// Two cradles closer than this (px) are rivals who share a region: whoever
    /// is older takes the ground, the younger one grows up against it.
    pub contact_distance_px: f32,
    /// A cradle further than this (px) from every other rose on its own.
    pub isolated_distance_px: f32,
    /// Territory-to-territory reach (px) within which two realms still *know*
    /// of each other — a traveller's tale, a traded bead, an intermediary.
    pub rumour_distance_px: f32,
    /// Province-to-province distance (px) that still counts as a shared land
    /// frontier rather than a sea crossing.
    pub frontier_distance_px: f32,
    /// Province-to-province distance (px) up to which a contact is a sea route.
    ///
    /// Deliberately short: at the dawn of history a sea route is a strait or a
    /// narrow sea, not an ocean passage. Province centres in one province sit
    /// about twenty pixels apart, so a hundred and twenty pixels is a real water
    /// gap of a few hundred kilometres at most.
    pub searoute_distance_px: f32,

    /// Discharge, in m³/s, that makes a river *great*: the bar for a floodplain or
    /// delta cradle, and for calling a river the reason a civilization exists.
    ///
    /// Deliberately far below the map painter's own `major_river_flow`
    /// (15 000 m³/s), which is Amazon-class — 3 000 m³/s is about the Nile's mean
    /// discharge, and the Nile is the river that fed one of the two oldest
    /// civilizations on Earth.
    pub great_river_flow: f32,

    /// Two cradles this close (fraction of map width) and on the same landmass
    /// share an ancestral stock: their peoples parted *before* either founded a
    /// city, so their tongues, rites and crafts are cousins.
    ///
    /// About 2 000 km of ground at the equator — the width of a shared prehistory,
    /// not of an empire.
    pub family_radius_frac: f32,

    // ── The calendar ─────────────────────────────────────────────────────────
    /// How many years the world's peoples lived before the first city — the depth
    /// of prehistory.
    ///
    /// `None` draws it from the seed over a wide log-uniform range (five thousand
    /// to two million years), divided by the world's own pace of development (see
    /// [`crate::civilization::epoch::DevelopmentPace`]); `Some(years)` fixes it.
    /// Deliberately not tied to the epoch: a calendar's zero is a recent
    /// convention, while this is where a world's real depth lives.
    pub species_emergence_years: Option<i32>,

    /// How many of the best provinces the epoch-site selector draws from.
    ///
    /// The world's calendar is reckoned from one province, chosen by a seeded
    /// weighted draw among the finest few candidates — so it is never the top
    /// score, and never arbitrary.
    pub epoch_site_candidates: usize,

    /// A province smaller than this (km²) can never be the place a calendar is
    /// reckoned from: an epoch needs a place, not a speck.
    pub epoch_site_min_area_km2: f32,

    /// People per km² a province supports at full habitability once its
    /// civilization is fully developed. Everything downstream is this number
    /// times the province's own habitability, altitude penalty and the realm's
    /// development multiplier.
    pub max_density_per_km2: f32,

    /// Province count a small realm aims for — a Greek polis against its neighbours.
    pub small_provinces: (usize, usize),
    /// Province count a middling realm aims for — Egyptian scale.
    pub medium_provinces: (usize, usize),
    /// Province count a large realm aims for — Indus / Chinese scale.
    pub large_provinces: (usize, usize),

    /// Hard ceiling on the cost of a single expansion step. Stops a realm from
    /// vaulting a mountain wall or an ocean to grab a distant province: the
    /// strongest barriers on the map are simply walls that history did not cross.
    pub max_step_cost: f32,
}

impl Default for CivilizationConfig {
    fn default() -> Self {
        Self {
            min_civilizations: 18,
            max_civilizations: 32,

            cradle_separation_frac: 0.010,
            cradle_floor_frac: 0.003,
            cradle_max_per_region: 3,
            cradle_max_per_superregion: 5,

            contact_distance_px: 430.0,
            isolated_distance_px: 620.0,
            rumour_distance_px: 1_500.0,
            frontier_distance_px: 55.0,
            searoute_distance_px: 120.0,

            great_river_flow: 3_000.0,

            family_radius_frac: 0.05,

            species_emergence_years: None,
            epoch_site_candidates: 24,
            epoch_site_min_area_km2: 500.0,

            max_density_per_km2: 22.0,

            small_provinces: (8, 16),
            medium_provinces: (20, 32),
            large_provinces: (40, 72),

            max_step_cost: 900.0,
        }
    }
}
