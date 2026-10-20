use crate::core::Grid;
use crate::terrain::config::WorldConfig;
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*; // Required for High-Definition speed

pub struct TectonicsResult {
    pub heightmap: Grid<f32>,
}

#[inline(always)]
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

    // PARALLELIZE THE ENTIRE GLOBE ACROSS ALL CPU CORES
    heightmap.raw_slice_mut().par_chunks_exact_mut(w).enumerate().for_each(|(y, row)| {
        let fy = y as f32;
        let v = fy / (h - 1) as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;
        let cos_lat = lat.cos() as f64;
        let sin_lat = lat.sin() as f64;

        for x in 0..w {
            let fx = x as f32;
            let u = fx / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;

            let px = cos_lat * (lon as f64).cos();
            let py = sin_lat;
            let pz = cos_lat * (lon as f64).sin();

            // Slightly stronger warp for jagged, realistic coastlines (HD: 6 octaves)
            let wx = sample_3d_fbm(&warp_x, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * 0.32;
            let wy = sample_3d_fbm(&warp_y, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * 0.32;
            let wz = sample_3d_fbm(&warp_z, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * 0.32;

            let sx = px + wx as f64;
            let sy = py + wy as f64;
            let sz = pz + wz as f64;

            // The exact frequency and threshold that generated the perfect continents!
            // HD UPGRADE: Raised from 6 to 9 octaves for millimeter-level coastal detail!
            let continent_val = sample_3d_fbm(&continent_noise, sx * 1.05, sy * 1.05, sz * 1.05, 9, 2.1, 0.52);
            let sea_threshold = 0.11f32; 
            
            let mut elev: f32;

            if continent_val > sea_threshold {
                let land_t = (continent_val - sea_threshold) / (1.0 - sea_threshold);
                let base_plain = 0.415 + land_t.powf(1.3) * 0.065;

                // CONTINUOUS FAULT LINES (The Andes / Rockies system)
                let fault_val = sample_3d_fbm(&fault_noise, sx * 1.6, sy * 1.6, sz * 1.6, 4, 2.0, 0.5);
                let fault_dist = fault_val.abs();
                
                let mut mountain_lift = 0.0;
                if fault_dist < 0.10 && land_t > 0.05 {
                    // HD Mountains: Raised to 9 octaves for extreme craggy details
                    let m_sample = sample_3d_fbm(&mountain_noise, sx * 4.0, sy * 4.0, sz * 4.0, 9, 2.2, 0.48);
                    let ridge = (1.0 - m_sample.abs()).powf(3.0);
                    let fault_intensity = (1.0 - (fault_dist / 0.10)).powf(1.5);
                    mountain_lift = ridge * fault_intensity * 0.45;
                }

                // Add rolling hills to plains (HD: 6 octaves)
                let hills = sample_3d_fbm(&mountain_noise, sx * 4.5, sy * 4.5, sz * 4.5, 6, 2.0, 0.5) * 0.035;

                elev = base_plain + mountain_lift + hills;
            } else {
                let ocean_t = ((sea_threshold - continent_val) / (1.0 + sea_threshold)).clamp(0.0, 1.0);
                elev = 0.395 - ocean_t.powf(0.7) * 0.26;
                // Submarine ridges
                let ocean_ridge = (1.0 - (sample_3d_fbm(&mountain_noise, px * 2.5, py * 2.5, pz * 2.5, 5, 2.0, 0.5)).abs()).powf(5.0);
                elev += ocean_ridge * 0.045;
            }

            row[x] = elev.clamp(0.04, 1.0);
        }
    });

    TectonicsResult { heightmap }
}