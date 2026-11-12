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

    // Landlocked seas — the Gibraltar rule
    /// Share of the canvas a landlocked body of water must cover before it is
    /// treated as a **sea** rather than a lake.
    ///
    /// The pass runs at the end of act one. A below-datum body is measured
    /// against `sea_min_area_share × width × height`; the largest body of water
    /// in the world is always the ocean and is never a candidate. At 8 192 ×
    /// 4 096 the default `0.0004` is ≈ 13 400 pixels — roughly 200 000 km², a
    /// Great Lakes or Black Sea rather than a pond.
    pub sea_min_area_share: f32,
    /// Most landlocked seas one world will open to the ocean.
    pub sea_max_straits: usize,
    /// Width, in pixels, of a carved strait's swath. Rounded down to an odd
    /// centre-plus-shoulders width (`5` cuts a channel five pixels across).
    pub strait_width: usize,
    /// How far below the datum a strait's centre floor is cut, in heightmap
    /// units. The shoulders are cut to half this; every carved pixel is water.
    pub strait_depth: f32,
    /// Highest saddle, in heightmap units above the datum, a strait may cut
    /// through. Higher ground is a wall: the body stays a lake. The default
    /// `0.02` is ≈ +300 m.
    pub strait_max_sill: f32,
    /// Longest strait, in pixels, the carver will consider. The default `600`
    /// is ≈ 2 900 km at the atlas's 8K scale.
    pub strait_max_span: usize,
    /// Cost of one heightmap unit of uplift in the least-cost route. Larger
    /// values make the strait walk further to find a lower pass; smaller values
    /// favour the shortest cut regardless of the ridge it crosses.
    pub strait_slope_cost: f32,
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

            // Landlocked seas: ≈13 400 px at 8K (Great Lakes scale), a five-pixel
            // channel cut 220 m below the datum through saddles no higher than
            // +300 m, within 600 px, and at most three seas per world.
            sea_min_area_share: 0.0004,
            sea_max_straits: 3,
            strait_width: 5,
            strait_depth: 0.008,
            strait_max_sill: 0.02,
            strait_max_span: 600,
            strait_slope_cost: 2_000.0,
        }
    }
}
