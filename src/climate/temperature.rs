use crate::core::{relief, Grid};
use crate::climate::config::{ClimateConfig, PlanetaryMode};
use noise::{NoiseFn, OpenSimplex};

pub fn simulate_temperature(
    heightmap: &Grid<f32>,
    sea_level: f32,
    config: &ClimateConfig,
) -> Grid<f32> {
    let w = heightmap.width;
    let h = heightmap.height;
    let mut temp_grid = Grid::new(w, h);
    let noise = OpenSimplex::new(config.seed as u32);
    
    for y in 0..h {
        let fy = y as f32;
        let v = fy / (h as f32 - 1.0); // 0.0 (North Pole) to 1.0 (South Pole)

        for x in 0..w {
            let fx = x as f32;
            let u = fx / w as f32;

            // Insolation calculation
            let insolation = match config.mode {
                PlanetaryMode::RotatingStandard { .. } => {
                    let lat_rad = (v - 0.5) * std::f32::consts::PI;
                    lat_rad.cos()
                }
                PlanetaryMode::TidallyLocked => {
                    let dx = (u - 0.5) * 2.0;
                    let dy = (v - 0.5) * 2.0;
                    let dist_sq = dx * dx + dy * dy;
                    (1.0 - dist_sq.sqrt()).max(0.0)
                }
            };

            // Steeper cosine curve: Keeps mid-latitudes temperate, freezing ONLY at extreme poles (75°+)
            let polar_taper = (insolation * 1.15).min(1.0).powf(0.65);
            let mut base_temp = config.pole_temp + polar_taper * (config.equator_temp - config.pole_temp);

            // Organic Atmospheric Waves (bends horizontal climate bands organically!)
            let angle = u * std::f32::consts::TAU;
            let atmospheric_wave = noise.get([
                angle.cos() as f64 * 1.8,
                angle.sin() as f64 * 1.8,
                v as f64 * 3.5,
            ]) as f32;
            base_temp += atmospheric_wave * 6.5;

            // 3. Altitude Lapse Rate (higher altitude cools significantly).
            // Altitude is read through the planet's vertical datum, so the
            // configured lapse rate is the familiar °C per 1000 m of climb.
            let elev = *heightmap.get(x as isize, y as isize);
            let altitude_km = (elev - sea_level).max(0.0) * relief::LAND_M_PER_UNIT / 1000.0;
            let temp = base_temp - altitude_km * config.lapse_rate;

            temp_grid.set(x as isize, y as isize, temp);
        }
    }

    temp_grid
}