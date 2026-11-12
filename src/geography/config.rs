#[derive(Clone, Debug)]
pub struct GeographyConfig {
    pub num_provinces: usize,
    /// Most of the ice-free land that may be handed to impassable wilderness.
    ///
    /// Worlds differ enormously in how much hyper-arid or high ground they hold
    /// — a supercontinent can easily be a quarter desert — so the wilderness is
    /// budgeted rather than left to the hostility score alone. Set to `0.0` for a
    /// completely settled planet; `0.18` is the Earth-like ceiling the atlas was
    /// tuned at. Permanent polar ice is excluded from the budget: it is
    /// geography, not a tunable.
    ///
    /// `f64`: the share is multiplied by a land-pixel count in the tens of
    /// millions, so the ceiling is kept at the precision the budget is spent in.
    pub max_waste_share: f64,
    /// Environmental hostility above which a province is abandoned to the wild.
    ///
    /// Lowering it makes the planet harsher and harder to colonise; raising it
    /// leaves even the deep desert settled.
    pub waste_cutoff: f32,
}

impl Default for GeographyConfig {
    fn default() -> Self {
        Self {
            num_provinces: 30_000, // Grand Strategy Location Scale!
            max_waste_share: 0.18,
            waste_cutoff: 0.68,
        }
    }
}

/// Tuning for the hydro-political subdivision of the world ocean.
///
/// The ocean mirrors the land's five-tier hierarchy
/// (`Province → Area → Region → Superregion → Continent`), but because it is
/// by far the larger domain every tier is scaled up: one ocean province is many
/// times the area of a land province, and each parent tier is correspondingly
/// larger. The `*_target` values are the number of ocean provinces a single
/// cluster of that tier should aim to hold.
#[derive(Clone, Debug)]
pub struct OceanConfig {
    /// Total number of ocean provinces to scatter across the world ocean.
    pub num_provinces: usize,
    /// Target number of ocean provinces per Continental Basin.
    pub continent_target: usize,
    /// Target number of ocean provinces per Oceanic Superregion.
    pub superregion_target: usize,
    /// Target number of ocean provinces per Oceanic Region.
    pub region_target: usize,
    /// Target number of ocean provinces per Oceanic Area.
    pub area_target: usize,
}

impl Default for OceanConfig {
    fn default() -> Self {
        Self {
            // ~2 500 marine provinces ≈ 22× the area of a land province.
            num_provinces: 2_500,
            // ~5 continental basins, ~17 superregions, ~62 regions, ~208 areas.
            continent_target: 400,
            superregion_target: 150,
            region_target: 40,
            area_target: 12,
        }
    }
}