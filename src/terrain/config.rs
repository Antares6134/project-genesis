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

    // ── Continental layout & coastlines ──────────────────────────────────────
    /// The elevation a pixel of the continental noise field must clear to be
    /// land. The global water line: lower it and ocean basins drain to reveal
    /// land bridges and inland seas, raise it and continents sink into
    /// archipelagos. `0.11` is the Earth-like centre the atlas was tuned at.
    pub sea_threshold: f32,
    /// Frequency of the continental fBm field. Low frequencies aggregate the
    /// crust into one or two supercontinents; high ones shatter it into an
    /// archipelago.
    ///
    /// The three fBm-shape values of this pass are `f64`, not `f32`: the field is
    /// sampled in `f64`, and the atlas's tuned constants are only reproduced
    /// exactly when the panel's value reaches the noise field at the same
    /// precision the engine has always used.
    pub continental_freq: f64,
    /// Fractal persistence across the continental field's octaves. Low values
    /// give smooth, gentle shorelines; high ones craggy coasts and micro-islands.
    pub continental_persistence: f64,
    /// Amplitude of the coordinate warp applied to the crust before the
    /// continental field is read. At `0.0` landmasses are regular and rounded;
    /// high values fold the coast into fjords, bays and chaotic peninsulas.
    pub domain_warp_amp: f32,

    // ── Orogeny & tectonics ──────────────────────────────────────────────────
    /// Maximum vertical uplift along a tectonic fault line, as a share of the
    /// heightmap. Low values leave low-relief shield plains; high ones raise
    /// impassable Alpine ridges.
    pub mountain_lift: f32,
    /// Frequency of the fault-line field, i.e. how densely the globe is crossed
    /// by plate boundaries. Low values yield a few isolated mountain belts,
    /// high values a criss-crossed web of ranges.
    pub fault_noise_freq: f64,
    /// Half-width, in fault-field units, of the orogenic collision zone: the
    /// thickness of a mountain belt. Small values draw sharp narrow spines,
    /// large values broad highland plateaus.
    pub fault_band: f32,

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

            // The continental field the atlas was tuned at: Earth-like sea level,
            // continent-scale landmasses, rugged-but-not-craggy coasts.
            sea_threshold: 0.11,
            continental_freq: 1.05,
            continental_persistence: 0.52,
            domain_warp_amp: 0.32,
            mountain_lift: 0.45,
            fault_noise_freq: 1.60,
            fault_band: 0.10,

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
