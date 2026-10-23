#[derive(Clone, Debug)]
pub struct GeographyConfig {
    pub num_provinces: usize,
}

impl Default for GeographyConfig {
    fn default() -> Self {
        Self {
            num_provinces: 30_000, // Grand Strategy Location Scale!
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