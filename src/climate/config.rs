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
            seed: 42,
        }
    }
}