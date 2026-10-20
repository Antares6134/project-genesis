use crate::core::Grid;
use crate::climate::config::ClimateConfig;
use crate::climate::wind::WindVector;
use noise::{NoiseFn, OpenSimplex};

pub struct MoistureResult {
    pub precipitation: Grid<f32>,
}

pub fn simulate_moisture(
    heightmap: &Grid<f32>,
    _temperature: &Grid<f32>,
    winds: &Grid<WindVector>,
    sea_level: f32,
    config: &ClimateConfig,
) -> MoistureResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut precip_grid = Grid::new(w, h);
    let gyre_noise = OpenSimplex::new(config.seed as u32 + 777);
    let detail_noise = OpenSimplex::new(config.seed as u32 + 888);

    // 1. Calculate approximate distance to ocean (Continentality)
    let mut ocean_dist = Grid::filled_with(w, h, 25.0f32);
    for y in 0..h {
        for x in 0..w {
            if *heightmap.get(x as isize, y as isize) <= sea_level {
                ocean_dist.set(x as isize, y as isize, 0.0);
            }
        }
    }

    for _ in 0..4 {
        for y in 1..(h as isize - 1) {
            for x in 0..(w as isize) {
                let current = *ocean_dist.get(x, y);
                if current > 0.0 {
                    let min_n = (*ocean_dist.get(x - 1, y))
                        .min(*ocean_dist.get(x + 1, y))
                        .min(*ocean_dist.get(x, y - 1))
                        .min(*ocean_dist.get(x, y + 1));
                    ocean_dist.set(x, y, current.min(min_n + 1.0));
                }
            }
        }
    }

    // 2. Synthesize physically authentic planetary precipitation
    for y in 0..h {
        let v = y as f32 / (h as f32 - 1.0);
        let base_lat = ((0.5 - v) * 180.0).abs();

        for x in 0..w {
            let fx = x as f32;
            let u = fx / w as f32;
            let elev = *heightmap.get(x as isize, y as isize);

            if elev <= sea_level {
                precip_grid.set(x as isize, y as isize, 0.8);
                continue;
            }

            // A. Planetary Atmospheric Circulation Bands
            let angle = u * std::f32::consts::TAU;
            let gyre = gyre_noise.get([
                angle.cos() as f64 * 1.8,
                angle.sin() as f64 * 1.8,
                v as f64 * 3.0,
            ]) as f32;

            let organic_lat = (base_lat + gyre * 12.0).clamp(0.0, 90.0);

            let itcz = (-((organic_lat / 12.0).powf(2.0))).exp() * 0.70;
            let hadley_dist = (organic_lat - 26.0) / 7.0;
            let hadley_dry = (-hadley_dist.powf(2.0)).exp();
            let temperate_dist = (organic_lat - 48.0) / 10.0;
            let temperate = (-temperate_dist.powf(2.0)).exp() * 0.55;
            let polar_dry = ((organic_lat - 65.0).max(0.0) / 25.0).clamp(0.0, 0.7);

            let mut base_rain = 0.45 + itcz + temperate - hadley_dry * 0.48 - polar_dry;
            base_rain = base_rain.clamp(0.02, 1.2);

            // B. Continentality (Distance to Ocean)
            let dist = *ocean_dist.get(x as isize, y as isize);
            let continentality = (-dist / 14.0).exp(); 
            let inland_factor = 0.40 + 0.60 * continentality;

            // C. Orographic Rain & Rain Shadows
            let wind = *winds.get(x as isize, y as isize);
            let (gx, gy) = heightmap.gradient(fx, y as f32);
            let slope_upwind = -(wind.dx * gx + wind.dy * gy);
            let orographic = (slope_upwind * 15.0).clamp(-0.35, 0.60);

            // D. Local High-Frequency Detail Noise
            let local_noise = detail_noise.get([
                angle.cos() as f64 * 4.0,
                angle.sin() as f64 * 4.0,
                v as f64 * 6.0,
            ]) as f32 * 0.12;

            let final_precip = (base_rain * inland_factor + orographic + local_noise).clamp(0.01, 1.0);
            precip_grid.set(x as isize, y as isize, final_precip);
        }
    }

    // 3. Smooth blur pass
    let mut smoothed = precip_grid.clone();
    for y in 1..(h as isize - 1) {
        for x in 0..(w as isize) {
            let mut sum = *precip_grid.get(x, y) * 4.0;
            sum += *precip_grid.get(x - 1, y) * 2.0; sum += *precip_grid.get(x + 1, y) * 2.0;
            sum += *precip_grid.get(x, y - 1) * 2.0; sum += *precip_grid.get(x, y + 1) * 2.0;
            sum += *precip_grid.get(x - 1, y - 1);   sum += *precip_grid.get(x + 1, y - 1);
            sum += *precip_grid.get(x - 1, y + 1);   sum += *precip_grid.get(x + 1, y + 1);
            *smoothed.get_mut(x, y) = sum / 16.0;
        }
    }

    MoistureResult { precipitation: smoothed }
}