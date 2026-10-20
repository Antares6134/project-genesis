use crate::core::Grid;
use crate::geography::config::GeographyConfig;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::BinaryHeap;

pub struct ProvinceResult {
    pub province_ids: Grid<u32>,
    pub habitability: Grid<f32>,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct ExpandState {
    cost: u32,
    x: isize,
    y: isize,
    id: u32,
}

// Order for Min-Heap (Lowest cost expands first)
impl Ord for ExpandState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.x.cmp(&other.x))
            .then_with(|| self.y.cmp(&other.y))
    }
}

impl PartialOrd for ExpandState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub fn generate_provinces(
    heightmap: &Grid<f32>,
    temp_grid: &Grid<f32>,
    precip_grid: &Grid<f32>,
    flow_grid: &Grid<f32>,
    sea_level: f32,
    min_river_flow: f32,
    config: &GeographyConfig,
    seed: u64,
) -> ProvinceResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut hab_grid = Grid::filled_with(w, h, 0.0f32);
    let mut province_ids = Grid::filled_with(w, h, 0u32);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    println!("Calculating habitability matrix across {}x{} grid...", w, h);

    // 1. Calculate Habitability Score
    for y in 0..h {
        for x in 0..w {
            let elev = *heightmap.get(x as isize, y as isize);
            if elev <= sea_level {
                continue;
            }

            let temp = *temp_grid.get(x as isize, y as isize);
            let precip = *precip_grid.get(x as isize, y as isize);
            let flow = *flow_grid.get(x as isize, y as isize);

            // Optimal human settlement temperature curve (peak at ~18°C)
            let temp_score = (-((temp - 18.0) / 12.0).powf(2.0)).exp();

            // Precipitation score
            let precip_score = if precip < 0.15 { 0.1 } else { precip.clamp(0.0, 1.0).sqrt() };

            // Flat plains vs mountain slopes
            let (gx, gy) = heightmap.gradient(x as f32, y as f32);
            let slope = (gx * gx + gy * gy).sqrt();
            let flat_score = (1.0 - slope * 15.0).clamp(0.05, 1.0);

            // Major freshwater river bonus
            let river_bonus = if flow >= min_river_flow { 1.5 } else { 1.0 };

            let score = temp_score * precip_score * flat_score * river_bonus;
            hab_grid.set(x as isize, y as isize, score);
        }
    }

    // 2. Select Province Seeds (Explicitly typed!)
    let mut heap: BinaryHeap<ExpandState> = BinaryHeap::new();
    let mut seed_candidates: Vec<(f32, isize, isize)> = Vec::new();

    // Stride sampling across 8K grid to keep seeding fast and efficient
    let stride = 2;
    for y in (0..h).step_by(stride) {
        for x in (0..w).step_by(stride) {
            let hab = *hab_grid.get(x as isize, y as isize);
            if hab > 0.05 {
                let randomized_score = hab * rng.random_range(0.3..1.0);
                seed_candidates.push((randomized_score, x as isize, y as isize));
            }
        }
    }

    // Sort by best score descending
    seed_candidates.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    // Explicitly type accepted_seeds: Vec<(isize, isize)>!
    let mut accepted_seeds: Vec<(isize, isize)> = Vec::with_capacity(config.num_provinces);
    let mut current_id = 1u32;

    for &(_, cx, cy) in &seed_candidates {
        if accepted_seeds.len() >= config.num_provinces {
            break;
        }

        let local_hab = *hab_grid.get(cx, cy);
        // Dynamic province radius: fertile valleys get dense small provinces; deserts get large ones
        let min_dist_sq: isize = ((12.0 / (local_hab + 0.15)).powf(2.0)) as isize;

        let mut too_close = false;
        for &(ax, ay) in &accepted_seeds {
            let mut dx: isize = (cx - ax).abs();
            if dx > (w as isize) / 2 {
                dx = w as isize - dx;
            }
            let dy: isize = cy - ay;
            if dx * dx + dy * dy < min_dist_sq {
                too_close = true;
                break;
            }
        }

        if !too_close {
            accepted_seeds.push((cx, cy));
            province_ids.set(cx, cy, current_id);
            heap.push(ExpandState { cost: 0, x: cx, y: cy, id: current_id });
            current_id += 1;
        }
    }

    println!("✓ Successfully seeded {} strategic province capitals.", current_id - 1);
    println!("Growing natural borders (Dijkstra terrain flood fill)...");

    // 3. Geographic Voronoi Expansion (Borders snap to mountains & rivers!)
    let neighbors: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
        (-1, -1, 1.414), (1, -1, 1.414), (-1, 1, 1.414), (1, 1, 1.414),
    ];

    while let Some(state) = heap.pop() {
        let current_id = *province_ids.get(state.x, state.y);
        if current_id != state.id { continue; }

        let current_elev = *heightmap.get(state.x, state.y);

        for &(dx, dy, dist) in &neighbors {
            let nx = heightmap.wrap_x(state.x + dx) as isize;
            let ny = state.y + dy;

            if ny < 0 || ny >= h as isize { continue; }

            let target_elev = *heightmap.get(nx, ny);
            if target_elev <= sea_level { continue; } // Natural border at coastlines!

            let target_id = *province_ids.get(nx, ny);
            if target_id == 0 {
                let elev_diff = (target_elev - current_elev).abs();

                // Mountain crest penalty (makes borders follow mountain ridgelines)
                let mut move_cost = dist * 10.0 + elev_diff * 1800.0;

                // River crossing penalty (makes borders follow rivers)
                let flow = *flow_grid.get(nx, ny);
                if flow >= min_river_flow {
                    move_cost += 60.0;
                }

                let new_cost = state.cost + move_cost as u32;

                province_ids.set(nx, ny, state.id);
                heap.push(ExpandState { cost: new_cost, x: nx, y: ny, id: state.id });
            }
        }
    }

    ProvinceResult {
        province_ids,
        habitability: hab_grid,
    }
}