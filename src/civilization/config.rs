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

    /// The world's **era of first cities**: the nominal years within which every
    /// cradle after the first rises.
    ///
    /// Contemporaries are not a relay — they share the same centuries. Our own
    /// first cities span about fifteen hundred years (3 500 → 2 070 BCE), and this
    /// is that span: the first city stands at internal year zero and every later
    /// cradle rises somewhere inside the window, so the youngest realm is a few
    /// centuries to a couple of millennia behind the first rather than thousands of
    /// years. Divided by the world's pace, so a slow world's dawn lasts longer.
    pub founding_window_years: i32,

    /// How many years *after* the atlas date the calendar's epoch event is placed.
    ///
    /// The atlas portrays the world in its dawn, before its own calendar's year
    /// zero, so every date it shows is BCE — exactly as a map of our own 2000 BCE is
    /// dated in the years before the Common Era. Drawn per world over this range, so
    /// a Copper-age map reads a few centuries to a few millennia before the epoch.
    pub epoch_after_atlas_years: (i32, i32),

    /// How many of the best provinces the epoch-site selector draws from.
    ///
    /// The world's calendar is reckoned from one province, chosen by a seeded
    /// weighted draw among the finest few candidates — so it is never the top
    /// score, and never arbitrary.
    pub epoch_site_candidates: usize,

    /// A province smaller than this (km²) can never be the place a calendar is
    /// reckoned from: an epoch needs a place, not a speck.
    pub epoch_site_min_area_km2: f32,

    /// People per km² a province supports at full habitability once its civilization
    /// is fully developed. Everything downstream is this number times the province's
    /// own habitability, altitude penalty and the tier's own draw.
    ///
    /// Calibrated for a world at the dawn: nine people per km² over a whole province
    /// — hills, marsh and all, not just the fields — is a Bronze Age country, and it
    /// is what puts a world's total population in the tens of millions rather than
    /// the hundreds.
    pub max_density_per_km2: f32,

    /// Province count a small realm aims for — a Greek polis against its neighbours.
    pub small_provinces: (usize, usize),
    /// Province count a middling realm aims for — Egyptian scale.
    pub medium_provinces: (usize, usize),
    /// Province count a large realm aims for — Indus / Chinese scale.
    pub large_provinces: (usize, usize),

    // ── The five-way scheme ──────────────────────────────────────────────────
    //
    // The dawn partitions the *whole* inhabited world, not a handful of clearings:
    // every province belongs to somebody, and the tiers below decide who. The
    // engine is a hybrid — a province's own country says what it *can* feed, and
    // its cost-distance from the nearest state says how much of that potential has
    // actually been worked.
    /// Farming potential a province needs before a field is possible at all.
    pub farm_floor: f32,
    /// Pasture potential a province needs before herds are possible at all.
    pub pasture_floor: f32,
    /// Influence cost inside which farming land is a ranked chiefdom country.
    pub chiefdom_cost: f32,
    /// Influence cost inside which farming land is worked at all. This single
    /// number is how far farming has spread from the states that invented it: raise
    /// it and the neolithic reaches further, lower it and the world stays wild.
    pub farm_cost: f32,
    /// Influence cost beyond which a farming country counts as isolated.
    pub far_cost: f32,
    /// Farming potential an isolated country needs before it can rise on its own,
    /// as the Amazon and New Guinea did.
    pub rise_floor: f32,
    /// Cost budget of an independent rise: how far its own neolithic spreads.
    pub independent_reach: f32,
    /// Most provinces one hunter-gatherer people may span before the belt is split
    /// into another named people. Deliberately large: a taiga or a rainforest is
    /// one country, and the map should say so.
    ///
    /// Only a guard on the flood that assembles the peoples: the real ceiling is
    /// the ground budget below, and the two are set to agree.
    pub forager_max_provinces: usize,
    /// Most provinces one herd people may span.
    pub pastoralist_max_provinces: usize,
    /// Most provinces one simple-farming people may span.
    pub simple_farmer_max_provinces: usize,
    /// Most provinces one chiefdom people may span.
    pub complex_farmer_max_provinces: usize,

    // ── How large one people may grow ────────────────────────────────────────
    //
    // The province caps above only stop a single breadth-first flood from running
    // away; they are a guard on the walk, not a statement about the world. What
    // actually decides how many peoples a map carries is *ground*: a belt is one
    // people until it covers more country than one people plausibly held, and the
    // neighbouring pieces are then rejoined until the budget is spent. Measured in
    // km², so a world of small provinces and a world of large ones both come out
    // looking like a world rather than like a partition count.
    /// Ground, in km², that one hunter-gatherer people may span.
    ///
    /// Bands are thin on the ground — a whole taiga or a whole desert is a handful
    /// of peoples, not a confetti of them — so this is the largest budget in the
    /// table.
    pub forager_merge_km2: f32,
    /// Ground, in km², that one herd people may span. A steppe is one or two
    /// peoples, because a horse and a herd can cross it in a season.
    pub pastoralist_merge_km2: f32,
    /// Ground, in km², that one simple-farming people may span.
    pub simple_farmer_merge_km2: f32,
    /// Ground, in km², that one chiefdom people may span.
    pub complex_farmer_merge_km2: f32,
    /// How many neighbours a society's dossier may list. A partitioned world has
    /// hundreds of societies, so an uncapped list is unusable.
    pub contact_limit: usize,
    /// How many kin a society's dossier may name, for the same reason.
    pub kin_limit: usize,

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
            founding_window_years: 1_500,
            epoch_after_atlas_years: (1_200, 3_200),
            epoch_site_candidates: 24,
            epoch_site_min_area_km2: 500.0,

            max_density_per_km2: 9.0,

            // A state is a city-state or a river kingdom, not a continent: the
            // atlas shows a world in which states hold barely one per cent of the
            // inhabited ground, as they did.
            small_provinces: (2, 5),
            medium_provinces: (5, 10),
            large_provinces: (10, 20),

            farm_floor: 0.10,
            pasture_floor: 0.50,
            chiefdom_cost: 15.0,
            farm_cost: 38.0,
            far_cost: 1_500.0,
            rise_floor: 0.45,
            independent_reach: 260.0,
            forager_max_provinces: 600,
            pastoralist_max_provinces: 900,
            simple_farmer_max_provinces: 200,
            complex_farmer_max_provinces: 100,

            forager_merge_km2: 5.0e6,
            pastoralist_merge_km2: 7.0e6,
            simple_farmer_merge_km2: 1.6e6,
            complex_farmer_merge_km2: 8.0e5,
            contact_limit: 8,
            kin_limit: 6,

            max_step_cost: 900.0,
        }
    }
}
