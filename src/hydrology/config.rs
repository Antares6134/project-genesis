#[derive(Clone, Debug)]
pub struct HydrologyConfig {
    pub min_lake_depth: f32,
    pub min_river_flow: f32,       // Raised from 35.0 -> 160.0 to eliminate the parallel "rake" scratches!
    pub major_river_flow: f32,
    pub runoff_multiplier: f32,
}

impl Default for HydrologyConfig {
    fn default() -> Self {
        Self {
            min_lake_depth: 0.015,     // Only genuine deep tectonic rift depressions become lakes!
            min_river_flow: 180.0,
            major_river_flow: 750.0,
            runoff_multiplier: 1.0,
        }
    }
}