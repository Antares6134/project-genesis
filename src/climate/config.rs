#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlanetaryMode {
    RotatingStandard { axial_tilt_deg: f32 }, // Earth-like rotating planet
    TidallyLocked,                            // Eyeball world with permanent day/night sides
}

#[derive(Clone, Debug)]
pub struct ClimateConfig {
    pub seed: u64,
    pub mode: PlanetaryMode,

    // Temperatures in Celsius
    pub equator_temp: f32, // e.g. 30.0 °C
    pub pole_temp: f32,    // e.g. -25.0 °C
    pub lapse_rate: f32,   // Temperature drop per kilometre of altitude (°C/km), e.g. 6.5

    // Moisture & Precipitation
    pub evaporation_rate: f32,    // How fast oceans vaporize moisture into winds
    pub rain_shadow_decay: f32,   // How quickly clouds deplete over mountains
    pub wind_passes: usize,       // Steps for moisture advection across the globe

    // ── The precipitation model ───────────────────────────────────────────────
    /// Baseline annual precipitation before latitude belts, rain shadows and
    /// distance from the sea are accounted for. Sets how lush or dry the planet
    /// is as a whole: `0.45` is the temperate Earth the atlas was tuned at.
    pub base_rainfall: f32,
    /// How deep the descending limb of the Hadley cell dries the subtropics
    /// (around ±26°). At `0.0` subtropical deserts disappear; high values girdle
    /// the globe in Sahara-like belts.
    pub hadley_dry_depth: f32,
    /// Share of oceanic rainfall that survives to the deepest continental
    /// interior, where distance from the sea is at its greatest. Low values dry
    /// supercontinent hearts into salt flats; high values let the sea's rain
    /// reach far inland.
    pub inland_factor_floor: f32,
    /// Multiplier on mountain-induced precipitation. High values deluge the
    /// windward slopes and starve the lee into rain-shadow deserts.
    pub orographic_strength: f32,
}

impl Default for ClimateConfig {
    fn default() -> Self {
        Self {
            mode: PlanetaryMode::RotatingStandard { axial_tilt_deg: 23.5 },
            equator_temp: 32.0,
            pole_temp: -28.0,
            lapse_rate: 6.5,
            evaporation_rate: 0.12,
            rain_shadow_decay: 0.08,
            wind_passes: 45,
            // The precipitation model the atlas was tuned at: a temperate globe
            // with two subtropical desert belts and real rain shadows.
            base_rainfall: 0.45,
            hadley_dry_depth: 0.48,
            inland_factor_floor: 0.40,
            orographic_strength: 15.0,
            seed: 42,
        }
    }
}