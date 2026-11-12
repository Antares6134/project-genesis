/// Tuning for **Languages & Phonetics** — the eighth act of creation.
///
/// Every distance here is in **map pixels**, like [`CivilizationConfig`], so a
/// rule reads the same on a 2K test world as on the 8K production plate. The
/// stage works on the province graph rather than on pixels, so these are the
/// only figures a world needs to reproduce its own languages exactly.
///
/// [`CivilizationConfig`]: crate::civilization::CivilizationConfig
#[derive(Clone, Debug)]
pub struct LanguageConfig {
    /// Fewest ancestral families the world should carry.
    pub min_families: usize,
    /// Most ancestral families the world may carry.
    pub max_families: usize,

    /// Minimum spacing between two Urheimats, as a fraction of map width.
    pub family_separation_frac: f32,
    /// The absolute floor between any two Urheimats, as a fraction of map width.
    pub family_floor_frac: f32,
    /// Most families one landmass may cradle.
    pub max_families_per_continent: usize,

    /// Fraction of the inhabited provinces one macro-family may sweep across.
    pub family_cap_share: f32,

    /// Population density, in people per km², above which a province resists
    /// linguistic absorption: a city keeps its own speech instead of taking its
    /// neighbour's.
    pub urban_density_per_km2: f32,
    /// Multiplier applied to an edge that steps into such a province.
    pub urban_resistance: f32,
    /// Multiplier applied to an edge crossing wilderness or pasture: a sweeping
    /// macro-family blob is cheaper over empty country than over farmland.
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

    /// Hard ceiling on one diffusion step. A barrier above this is a wall the
    /// language never crossed.
    pub max_step_cost: f32,
    /// How sharply distance beyond an adjacent province inflates a step.
    pub hop_divisor_px: f32,
    pub hop_exponent: f32,

    /// Shortest sea gap, in pixels, a language may cross once its family has
    /// learned to sail. Zero disables seafaring entirely.
    pub sea_crossing_max_px: f32,
    /// Cost multiplier for such a crossing.
    pub sea_crossing_cost: f32,
    /// The age at or after which the leading world may cross open water.
    pub seafaring_age: u8,

    /// Most daughter languages one family may split into.
    pub languages_per_family_max: usize,
    /// Fewest daughter languages a family in a dense cradle may split into.
    pub languages_per_family_dense_min: usize,
    /// A family is "dense" when at least this many of its provinces are urban.
    pub dense_family_urban_provinces: usize,
    /// Hard ceiling on every language in the world.
    pub max_languages: usize,

    /// How many languages a society's dossier may name as neighbours.
    pub contact_limit: usize,
    /// How many roots a language may borrow from a neighbour.
    pub max_borrowings: usize,
}

impl Default for LanguageConfig {
    fn default() -> Self {
        Self {
            min_families: 20,
            max_families: 30,

            family_separation_frac: 0.045,
            family_floor_frac: 0.012,
            max_families_per_continent: 5,

            family_cap_share: 0.34,

            urban_density_per_km2: 1.8,
            urban_resistance: 2.4,
            wilderness_discount: 0.78,

            river_discount: 0.66,
            grassland_friction: 0.88,
            woodland_friction: 1.12,
            deep_forest_friction: 1.65,
            arid_friction: 1.48,

            hill_start_m: 600.0,
            mountain_wall_m: 2_400.0,
            hill_friction: 1.55,
            mountain_friction: 3.40,

            max_step_cost: 1_400.0,
            hop_divisor_px: 90.0,
            hop_exponent: 1.5,

            sea_crossing_max_px: 120.0,
            sea_crossing_cost: 14.0,
            seafaring_age: 5,

            languages_per_family_max: 6,
            languages_per_family_dense_min: 3,
            dense_family_urban_provinces: 6,
            max_languages: 240,

            contact_limit: 6,
            max_borrowings: 3,
        }
    }
}
