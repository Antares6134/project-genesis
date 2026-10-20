use crate::core::Grid;
use crate::terrain::config::WorldConfig;
use noise::{NoiseFn, OpenSimplex};

pub struct TectonicsResult {
    pub heightmap: Grid<f32>,
}

fn sample_3d_fbm(noise: &OpenSimplex, x: f64, y: f64, z: f64, octaves: usize, lacunarity: f64, persistence: f64) -> f32 {
    let mut total = 0.0f64;
    let mut amplitude = 1.0f64;
    let mut frequency = 1.0f64;
    let mut max_val = 0.0f64;
    for _ in 0..octaves {
        total += noise.get([x * frequency, y * frequency, z * frequency]) * amplitude;
        max_val += amplitude;
        amplitude *= persistence;
        frequency *= lacunarity;
    }
    (total / max_val) as f32
}

pub fn simulate_tectonics(config: &WorldConfig) -> TectonicsResult {
    let w = config.width;
    let h = config.height;

    let continent_noise = OpenSimplex::new(config.seed as u32);
    let warp_x = OpenSimplex::new(config.seed.wrapping_add(101) as u32);
    let warp_y = OpenSimplex::new(config.seed.wrapping_add(202) as u32);
    let warp_z = OpenSimplex::new(config.seed.wrapping_add(303) as u32);
    let mountain_noise = OpenSimplex::new(config.seed.wrapping_add(404) as u32);
    let fault_noise = OpenSimplex::new(config.seed.wrapping_add(505) as u32);

    let mut heightmap = Grid::new(w, h);

    for y in 0..h {
        let v = y as f32 / (h - 1) as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;
        let cos_lat = lat.cos() as f64;
        let sin_lat = lat.sin() as f64;

        for x in 0..w {
            let u = x as f32 / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;

            let px = cos_lat * (lon as f64).cos();
            let py = sin_lat;
            let pz = cos_lat * (lon as f64).sin();

            // Slightly stronger warp for jagged, realistic coastlines
            let wx = warp_x.get([px * 1.5, py * 1.5, pz * 1.5]) * 0.32;
            let wy = warp_y.get([px * 1.5, py * 1.5, pz * 1.5]) * 0.32;
            let wz = warp_z.get([px * 1.5, py * 1.5, pz * 1.5]) * 0.32;

            let sx = px + wx;
            let sy = py + wy;
            let sz = pz + wz;

            // LOWER FREQUENCY (1.05 instead of 1.15) = Larger, fatter massive continents!
            let continent_val = sample_3d_fbm(&continent_noise, sx * 1.05, sy * 1.05, sz * 1.05, 6, 2.1, 0.52);
            
            // LOWER THRESHOLD (0.04 instead of 0.15) = More land rises above sea level!
            let sea_threshold = 0.11f32; 
            
            let mut elev: f32;

            if continent_val > sea_threshold {
                let land_t = (continent_val - sea_threshold) / (1.0 - sea_threshold);
                // Broad fertile lowlands
                let base_plain = 0.415 + land_t.powf(1.3) * 0.065;

                // CONTINUOUS FAULT LINES (The Andes / Rockies system)
                let fault_val = sample_3d_fbm(&fault_noise, sx * 1.6, sy * 1.6, sz * 1.6, 3, 2.0, 0.5);
                let fault_dist = fault_val.abs();
                
                let mut mountain_lift = 0.0;
                // Mountains ONLY rise if they are near a continental fault line!
                if fault_dist < 0.10 && land_t > 0.05 {
                    let m_sample = sample_3d_fbm(&mountain_noise, sx * 4.0, sy * 4.0, sz * 4.0, 5, 2.2, 0.48);
                    let ridge = (1.0 - m_sample.abs()).powf(3.0);
                    let fault_intensity = (1.0 - (fault_dist / 0.10)).powf(1.5);
                    mountain_lift = ridge * fault_intensity * 0.45;
                }

                // Add rolling hills to plains
                let hills = sample_3d_fbm(&mountain_noise, sx * 4.5, sy * 4.5, sz * 4.5, 4, 2.0, 0.5) * 0.035;

                elev = base_plain + mountain_lift + hills;
            } else {
                let ocean_t = ((sea_threshold - continent_val) / (1.0 + sea_threshold)).clamp(0.0, 1.0);
                elev = 0.395 - ocean_t.powf(0.7) * 0.26;
                let ocean_ridge = (1.0 - (sample_3d_fbm(&mountain_noise, px * 2.5, py * 2.5, pz * 2.5, 3, 2.0, 0.5)).abs()).powf(5.0);
                elev += ocean_ridge * 0.045;
            }

            heightmap.set(x as isize, y as isize, elev.clamp(0.04, 1.0));
        }
    }
    TectonicsResult { heightmap }
}