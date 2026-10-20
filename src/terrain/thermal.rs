use crate::core::Grid;
use crate::terrain::config::WorldConfig;

/// Simulates thermal weathering and talus slope relaxation.
/// Whenever the slope between neighboring cells exceeds the critical angle of repose,
/// rock material slumps downhill to form natural scree slopes.
pub fn simulate_thermal_erosion(heightmap: &mut Grid<f32>, config: &WorldConfig) {
    let w = config.width;
    let h = config.height;

    // 8-directional neighbor offsets (dx, dy)
    let neighbors: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0),
        (1, 0, 1.0),
        (0, -1, 1.0),
        (0, 1, 1.0),
        (-1, -1, std::f32::consts::SQRT_2),
        (1, -1, std::f32::consts::SQRT_2),
        (-1, 1, std::f32::consts::SQRT_2),
        (1, 1, std::f32::consts::SQRT_2),
    ];

    for _ in 0..config.thermal_iterations {
        let mut delta_grid = Grid::filled_with(w, h, 0.0f32);

        for y in 1..(h as isize - 1) {
            for x in 0..(w as isize) {
                let current_h = *heightmap.get(x, y);

                let mut max_slope_diff = 0.0f32;
                let mut total_excess = 0.0f32;
                let mut target_count = 0;

                for &(dx, dy, dist) in &neighbors {
                    let nx = x + dx;
                    let ny = y + dy;
                    let neighbor_h = *heightmap.get(nx, ny);

                    let diff = current_h - neighbor_h;
                    let slope = diff / dist;

                    if slope > config.talus_threshold {
                        let excess = (slope - config.talus_threshold) * dist;
                        total_excess += excess;
                        target_count += 1;
                        if diff > max_slope_diff {
                            max_slope_diff = diff;
                        }
                    }
                }

                // If unstable, distribute material to lower neighbors
                if target_count > 0 && max_slope_diff > 0.0 {
                    let move_amount = (total_excess / target_count as f32) * config.thermal_rate;
                    let move_per_neighbor = move_amount / target_count as f32;

                    *delta_grid.get_mut(x, y) -= move_amount;

                    for &(dx, dy, dist) in &neighbors {
                        let nx = x + dx;
                        let ny = y + dy;
                        let neighbor_h = *heightmap.get(nx, ny);
                        let slope = (current_h - neighbor_h) / dist;

                        if slope > config.talus_threshold {
                            *delta_grid.get_mut(nx, ny) += move_per_neighbor;
                        }
                    }
                }
            }
        }

        // Apply deltas back to heightmap
        for y in 0..h {
            for x in 0..w {
                let d = *delta_grid.get(x as isize, y as isize);
                let current = heightmap.get_mut(x as isize, y as isize);
                *current = (*current + d).clamp(0.01, 1.0);
            }
        }
    }
}