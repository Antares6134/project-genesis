use crate::core::Grid;
use crate::terrain::config::WorldConfig;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

/// Distributes an erosion/deposition delta across the 4 neighboring grid vertices
/// using bilinear weights so terrain changes are smooth and crack-free.
#[inline(always)]
fn distribute_delta(heightmap: &mut Grid<f32>, fx: f32, fy: f32, delta: f32) {
    let x0 = fx.floor() as isize;
    let y0 = fy.floor() as isize;
    let x1 = x0 + 1;
    let y1 = y0 + 1;

    let tx = fx - fx.floor();
    let ty = fy - fy.floor();

    let w00 = (1.0 - tx) * (1.0 - ty);
    let w10 = tx * (1.0 - ty);
    let w01 = (1.0 - tx) * ty;
    let w11 = tx * ty;

    let h00 = *heightmap.get(x0, y0);
    let h10 = *heightmap.get(x1, y0);
    let h01 = *heightmap.get(x0, y1);
    let h11 = *heightmap.get(x1, y1);

    heightmap.set(x0, y0, h00 + delta * w00);
    heightmap.set(x1, y0, h10 + delta * w10);
    heightmap.set(x0, y1, h01 + delta * w01);
    heightmap.set(x1, y1, h11 + delta * w11);
}

pub fn simulate_hydraulic_erosion(heightmap: &mut Grid<f32>, config: &WorldConfig) {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed.wrapping_add(9999));
    let w = config.width as f32;
    let h = config.height as f32;

    for _ in 0..config.erosion_droplets {
        // Spawn droplet randomly across the world
        let mut px = rng.random_range(0.0..w);
        let mut py = rng.random_range(1.0..h - 2.0);

        // Optimization: don't waste time on deep underwater ocean abysses
        if heightmap.sample_bilinear(px, py) < 0.38 {
            continue;
        }

        let mut dir_x = 0.0;
        let mut dir_y = 0.0;
        let mut speed = 1.0;
        let mut water = 1.0;
        let mut sediment = 0.0;

        for _ in 0..config.max_droplet_lifetime {
            let (gx, gy) = heightmap.gradient(px, py);

            // Compute new flow direction with inertia: inertia * old_dir + (1 - inertia) * downhill_gradient
            dir_x = dir_x * config.erosion_inertia - gx * (1.0 - config.erosion_inertia);
            dir_y = dir_y * config.erosion_inertia - gy * (1.0 - config.erosion_inertia);

            let len = (dir_x * dir_x + dir_y * dir_y).sqrt();
            if len < 1e-5 {
                break; // Flat depression or sinkhole
            }
            dir_x /= len;
            dir_y /= len;

            let old_px = px;
            let old_py = py;

            // Move droplet
            px += dir_x;
            py += dir_y;

            // Cylindrical wrapping on X, pole check on Y
            if px < 0.0 { px += w; }
            if px >= w { px -= w; }
            if py < 1.0 || py >= h - 2.0 {
                break;
            }

            let old_height = heightmap.sample_bilinear(old_px, old_py);
            let new_height = heightmap.sample_bilinear(px, py);
            let height_diff = new_height - old_height;

            // Calculate sediment carrying capacity
            // Faster water on steeper slopes carries dramatically more soil
            let capacity = ((-height_diff).max(0.0) * speed * water * config.sediment_capacity_factor)
                .max(config.min_sediment_capacity);

            if sediment > capacity || height_diff > 0.0 {
                // Moving uphill or carrying too much sediment: DEPOSIT
                let deposit_amount = if height_diff > 0.0 {
                    // Fill sinkholes
                    sediment.min(height_diff)
                } else {
                    (sediment - capacity) * config.deposit_speed
                };
                sediment -= deposit_amount;
                distribute_delta(heightmap, old_px, old_py, deposit_amount);
            } else {
                // ERODE rock from current position
                let erode_amount = ((capacity - sediment) * config.erode_speed).min(-height_diff);
                sediment += erode_amount;
                distribute_delta(heightmap, old_px, old_py, -erode_amount);
            }

            // Update droplet physics
            speed = (speed * speed + height_diff.abs() * 4.0).sqrt();
            water *= 1.0 - config.evaporation_rate;

            if water < 0.01 {
                break;
            }
        }
    }
}