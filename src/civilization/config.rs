/// Tuning for the dawn of history — the stage that decides where civilizations
/// rise and how far they spread.
///
/// Every distance here is in **map pixels** (so a rule reads the same on a 2K
/// test world as on the 16K production plate) and every `*_frac` value is a
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
    /// The epoch is the founding of the first city, while this is where a world's
    /// real depth lives.
    pub species_emergence_years: Option<i32>,

    /// The world's **era of first cities**: the nominal years within which every
    /// cradle after the first rises.
    ///
    /// Contemporaries are not a relay — they share the same centuries. Our own
    /// first cities span roughly fifteen hundred years, and this is that span: the
    /// first city stands at Year 0 and every later cradle rises somewhere inside the
    /// window, so the youngest realm is a few
    /// centuries to a couple of millennia behind the first rather than thousands of
    /// years. Divided by the world's pace, so a slow world's dawn lasts longer.
    pub founding_window_years: i32,

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

    /// Hard ceiling on the cost of a single expansion step. Stops a realm from
    /// vaulting a mountain wall or an ocean to grab a distant province: the
    /// strongest barriers on the map are simply walls that history did not cross.
    pub max_step_cost: f32,

    /// How faiths cross the ground once they exist. See [`ReligionConfig`].
    pub religion: ReligionConfig,
}

/// Tuning for the province layer of religion: how a creed spreads from the
/// provinces that keep it into their neighbours.
///
/// The figures deliberately rhyme with [`LanguageConfig`], because mountains,
/// rivers and cities are the same barriers to a tongue and to a faith. A faith
/// is not a language, though: its `OutgroupStance` and `Transmission` decide how
/// far it travels, and a frontier between two realms costs more than an internal
/// border. Nothing here decides *what* a faith believes — that stays the creed
/// generator's business.
///
/// [`LanguageConfig`]: crate::language::LanguageConfig
#[derive(Clone, Debug)]
pub struct ReligionConfig {
    /// Frontier passes to run. Each pass reads a snapshot and applies its
    /// conversions together, so one atlas date cannot carry a faith across two
    /// realms' worth of provinces in a single step.
    pub rounds: usize,

    /// Cost multiplier for a step across a realm frontier. Faiths do cross
    /// borders — that is how one creed comes to live in several countries — but
    /// not as readily as they move inside one.
    pub border_friction: f32,

    /// How strongly a province resists a new faith: the incumbent gets this as a
    /// flat score bonus before any neighbour's argument is heard.
    pub incumbent_resistance: f32,
    /// How much better a rival's neighbour score must be before the province
    /// turns. A low value makes a noisy checkerboard; a high one leaves every
    /// realm single-faithed.
    pub flip_margin: f32,

    /// Population density, in people per km², above which a province counts as
    /// urban: a city keeps — or gains — a faith of its own.
    pub urban_density_per_km2: f32,
    /// Cost multiplier for entering such a province.
    pub urban_resistance: f32,
    /// Cost multiplier for entering empty country. Creeds sweep over wilderness
    /// more cheaply than over farmland.
    pub wilderness_discount: f32,

    /// Cost multiplier for a step along a river valley.
    pub river_discount: f32,
    /// Cost multiplier for a step across dry grassland.
    pub grassland_friction: f32,
    /// Cost multiplier for a step through woodland.
    pub woodland_friction: f32,
    /// Cost multiplier for a step through taiga or tropical forest.
    pub deep_forest_friction: f32,
    /// Cost multiplier for a step across desert or steppe.
    pub arid_friction: f32,

    /// Mean elevation, in metres, at which hills begin to cost more.
    pub hill_start_m: f32,
    /// Mean elevation, in metres, at which a province is a mountain wall.
    pub mountain_wall_m: f32,
    /// Cost multiplier for a hill step.
    pub hill_friction: f32,
    /// Cost multiplier for a mountain step.
    pub mountain_friction: f32,

    /// How much of a converted realm's rural countryside may keep the displaced
    /// faith when the realm adopts a new one. Towns and the seat take the new
    /// creed; the fields often do not, which is where a diaspora begins.
    pub residual_share: f32,

    // ── Where faiths begin ──────────────────────────────────────────────────
    // A world does not have one religion per band. A hearth is a people large
    // enough, or isolated enough, to grow a tradition of its own; the small
    // societies around it adopt a neighbour's faith through the province walk.
    /// Fewest provinces a non-state society must hold to seed a faith of its own.
    pub hearth_min_provinces: usize,
    /// Chance that a small society seeds an endemic faith anyway. This is the
    /// one-province cult on a remote island or in a high valley: rare, but real.
    pub endemic_faith_chance: f32,

    // ── Consolidation ───────────────────────────────────────────────────────
    // A province-level map is not a census of every local recension. The same
    // map that draws a forager people across a continent must not draw forty
    // near-identical faiths across it; a state, by contrast, really is a mosaic
    // of local cults. These two ceilings are read in `SocietyTier::LADDER`
    // order — hunters, herders, simple farmers, chiefdoms, states — and
    // `usize::MAX` means "no ceiling".
    /// Most faiths one society may keep.
    ///
    /// A hunter-gatherer people is one tradition across a whole country, so a
    /// continent-sized band keeps two or three local recensions, not forty.
    /// The ceiling rises with organisation: a simple farming people keeps a
    /// handful, a chiefdom a regional mosaic, and a state as many local cults
    /// as its ground grows.
    pub faiths_per_tier: [usize; 5],
    /// Most broad traditions a superregion may keep.
    ///
    /// A tradition does not stop at a realm frontier. The two mobile tiers get
    /// a second, coarser consolidation over each superregion: a forager
    /// superregion is a family of traditions, not one per people, and a herd
    /// people's is a slightly wider family. Settled tiers are left alone —
    /// their regions really do keep their own faiths.
    pub faiths_per_superregion: [usize; 5],

    // ── Regional schisms ────────────────────────────────────────────────────
    /// A faith this large is a family of regional traditions, not one parish.
    pub schism_min_provinces: usize,
    /// Fewest provinces one region must hold to receive a daughter faith.
    pub schism_min_group: usize,

    // ── Diasporas ───────────────────────────────────────────────────────────
    /// Chance that an urban or coastal province keeps a foreign faith as a
    /// minority community. This is the city congregation, the merchant quarter
    /// and the exiled people — one province that the map can believe in.
    pub diaspora_chance: f32,
    /// Most of a realm's provinces that may become such communities, as a share.
    pub diaspora_max_share: f32,
    /// Absolute ceiling on diaspora provinces in one realm, so a huge band does
    /// not turn into a cosmopolitan archipelago.
    pub diaspora_max_per_realm: usize,
    /// Chance that such a community grows a tradition of its own rather than
    /// keeping a neighbour's. This is the small sect that exists in a handful of
    /// towns and nowhere as a state: rare, but the map must be able to show it.
    pub diaspora_new_faith_chance: f32,
}

impl Default for ReligionConfig {
    fn default() -> Self {
        Self {
            rounds: 4,

            border_friction: 1.45,

            incumbent_resistance: 0.35,
            flip_margin: 1.15,

            urban_density_per_km2: 1.8,
            urban_resistance: 2.20,
            wilderness_discount: 0.80,

            river_discount: 0.70,
            grassland_friction: 0.90,
            woodland_friction: 1.10,
            deep_forest_friction: 1.60,
            arid_friction: 1.45,

            hill_start_m: 600.0,
            mountain_wall_m: 2_400.0,
            hill_friction: 1.50,
            mountain_friction: 3.30,

            residual_share: 0.65,

            faiths_per_tier: [3, 4, 6, 12, usize::MAX],
            faiths_per_superregion: [4, 5, usize::MAX, usize::MAX, usize::MAX],

            hearth_min_provinces: 6,
            endemic_faith_chance: 0.12,

            schism_min_provinces: 30,
            schism_min_group: 12,

            diaspora_chance: 0.10,
            diaspora_max_share: 0.20,
            diaspora_max_per_realm: 4,
            diaspora_new_faith_chance: 0.30,
        }
    }
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

            great_river_flow: 3_000.0,

            family_radius_frac: 0.05,

            species_emergence_years: None,
            founding_window_years: 1_500,

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

            max_step_cost: 900.0,

            religion: ReligionConfig::default(),
        }
    }
}
