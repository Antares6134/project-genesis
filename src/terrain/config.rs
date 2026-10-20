#[derive(Clone, Debug)]
pub struct WorldConfig {
    pub width: usize,
    pub height: usize,
    pub seed: u64,

    // Tectonic plate parameters
    pub num_plates: usize,
    pub ocean_ratio: f32,
    pub continental_base_elev: f32,
    pub oceanic_base_elev: f32,
    pub mountain_height: f32,
    pub trench_depth: f32,
    pub boundary_spread: f32,

    // Hydraulic Erosion parameters (NEW)
    pub erosion_droplets: usize, // e.g. 150_000 droplets
    pub erosion_inertia: f32,    // 0.0 (no inertia) to 1.0 (droplet moves in straight line)
    pub sediment_capacity_factor: f32, // Higher = droplets carry and carve more rock
    pub min_sediment_capacity: f32,
    pub erode_speed: f32,        // Speed of picking up soil
    pub deposit_speed: f32,      // Speed of depositing sediment in valleys
    pub evaporation_rate: f32,   // Water evaporation per step
    pub max_droplet_lifetime: usize, // Max movement steps per droplet

    // Thermal weathering parameters (NEW)
    pub thermal_iterations: usize, // e.g. 4 passes across the map
    pub talus_threshold: f32,      // Max slope before rock slips downhill (e.g. 0.025)
    pub thermal_rate: f32,         // Fraction of excess material that slides down (e.g. 0.15)
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 512,
            seed: 42,
            num_plates: 28,
            ocean_ratio: 0.65,
            continental_base_elev: 0.58,
            oceanic_base_elev: 0.22,
            mountain_height: 0.35,
            trench_depth: 0.18,
            boundary_spread: 28.0,

            // Default erosion values for crisp geological weathering
            erosion_droplets: 120_000,
            erosion_inertia: 0.15,
            sediment_capacity_factor: 4.0,
            min_sediment_capacity: 0.01,
            erode_speed: 0.3,
            deposit_speed: 0.3,
            evaporation_rate: 0.02,
            max_droplet_lifetime: 40,

            // Thermal defaults
            thermal_iterations: 4,
            talus_threshold: 0.025,
            thermal_rate: 0.15,
        }
    }
}