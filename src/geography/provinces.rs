use crate::core::Grid;
use crate::geography::config::GeographyConfig;
use noise::{NoiseFn, OpenSimplex};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::{BinaryHeap, VecDeque};

#[derive(Clone)]
pub struct ProvinceResult {
    pub province_ids: Grid<u32>,
    pub habitability: Grid<f32>,
    pub is_wasteland: Grid<bool>,
    pub num_provinces: usize,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct ExpandState {
    cost: u32,
    x: isize,
    y: isize,
    id: u32,
}

impl Ord for ExpandState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost)
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
    is_lake: &Grid<bool>,
    sea_level: f32,
    min_river_flow: f32,
    config: &GeographyConfig,
    seed: u64,
) -> ProvinceResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut hab_grid = Grid::filled_with(w, h, 0.0f32);
    let mut is_wasteland = Grid::filled_with(w, h, false);
    let mut province_ids = Grid::filled_with(w, h, 0u32);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // ─── 0. FAST DISTANCE-TO-WATER TRANSFORM (CHAMFER PASS) ───────────────────
    let mut dist_to_water = Grid::filled_with(w, h, 255u8);
    {
        let dist_slice = dist_to_water.raw_slice_mut();
        let elev_slice = heightmap.raw_slice();
        let lake_slice = is_lake.raw_slice();

        // Initialize water as 0
        for i in 0..(w * h) {
            if elev_slice[i] <= sea_level || lake_slice[i] {
                dist_slice[i] = 0;
            }
        }

        // Forward pass
        for y in 0..h {
            let row_offset = y * w;
            let up_offset = if y > 0 { (y - 1) * w } else { row_offset };
            for x in 0..w {
                let idx = row_offset + x;
                if dist_slice[idx] > 0 {
                    let left_idx = if x > 0 { idx - 1 } else { row_offset + w - 1 };
                    let left = dist_slice[left_idx];
                    let up = if y > 0 { dist_slice[up_offset + x] } else { 255 };
                    let d = left.min(up).saturating_add(1);
                    if d < dist_slice[idx] {
                        dist_slice[idx] = d;
                    }
                }
            }
        }

        // Backward pass
        for y in (0..h).rev() {
            let row_offset = y * w;
            let down_offset = if y + 1 < h { (y + 1) * w } else { row_offset };
            for x in (0..w).rev() {
                let idx = row_offset + x;
                let cur = dist_slice[idx];
                if cur > 0 {
                    let right_idx = if x + 1 < w { idx + 1 } else { row_offset };
                    let right = dist_slice[right_idx];
                    let down = if y + 1 < h { dist_slice[down_offset + x] } else { 255 };
                    let d = cur.min(right.min(down).saturating_add(1));
                    dist_slice[idx] = d;
                }
            }
        }
    }

    // ─── 1. HABITABILITY SCORE & CONTROLLED WASTELAND IDENTIFICATION ───────────
    // Wastelands are restrained to:
    // 1. A few large ones (Greenland interior ice cap, central erg hearts)
    // 2. Many small ones scattered on high mountain peaks, crags, and salt sinks
    // 3. Strict coastal & river corridors ensuring UNBROKEN province connectivity!
    let waste_noise = OpenSimplex::new(seed as u32 + 9999);

    for y in 0..h {
        let v = y as f32 / (h - 1) as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;

        for x in 0..w {
            let elev = *heightmap.get(x as isize, y as isize);
            if elev <= sea_level || *is_lake.get(x as isize, y as isize) {
                continue;
            }

            let u = x as f32 / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;

            let px = (lat.cos() * lon.cos()) as f64;
            let py = lat.sin() as f64;
            let pz = (lat.cos() * lon.sin()) as f64;

            let temp     = *temp_grid.get(x as isize, y as isize);
            let precip   = *precip_grid.get(x as isize, y as isize);
            let flow     = *flow_grid.get(x as isize, y as isize);
            let altitude = elev - sea_level;
            let (gx, gy) = heightmap.gradient(x as f32, y as f32);
            let slope    = (gx * gx + gy * gy).sqrt();
            let dist     = *dist_to_water.get(x as isize, y as isize);

            // SANCTUARIES: Coastal & River Corridors are protected from wastelands
            // 1. Coastal Corridors (within ~22 pixels = ~110km of coast):
            //    Humans have always settled and navigated coasts (fishing, trade, milder climate).
            //    Guarantees unbroken continental perimeters (e.g., Maghreb to West Africa along Atlantic).
            let is_coastal_sanctuary = dist <= 22 && temp > -14.0;

            // 2. River Corridors:
            //    Rivers flowing through deserts/mountains (Nile, Indus) are fertile settlement zones.
            let is_river_sanctuary = flow >= min_river_flow * 0.08;

            let mut wasteland = false;

            if !is_coastal_sanctuary && !is_river_sanctuary {
                // A. Polar Glacial Ice Caps (interior deep freeze)
                let is_polar_ice = temp < -14.0 || (temp < -8.5 && altitude > 0.12 && dist > 16);

                // B. Deep Desert Ergs & Sand Seas (interior hyper-arid core)
                let is_erg_core = if dist > 25 && flow < min_river_flow * 0.04 && temp > 27.5 && precip < 0.038 {
                    let erg_noise = waste_noise.get([px * 16.0, py * 16.0, pz * 16.0]) as f32;
                    erg_noise > 0.36
                } else {
                    false
                };

                // C. High Alpine Summits & Knife-edge Peaks (small scattered crests)
                let peak_noise = waste_noise.get([px * 48.0, py * 48.0, pz * 48.0]) as f32;
                let is_alpine_crest = (altitude > 0.38 && peak_noise > -0.15)
                    || (altitude > 0.27 && slope > 0.095 && peak_noise > 0.20)
                    || (altitude > 0.21 && temp < -3.5 && slope > 0.065 && peak_noise > 0.28);

                // D. Harsh Wilderness / Badlands / Salt Pans (small isolated spots)
                let spot_noise = waste_noise.get([px * 65.0, py * 65.0, pz * 65.0]) as f32;
                let is_spot_wasteland = ((precip < 0.09 && slope > 0.075 && dist > 12) || (precip < 0.040 && dist > 16))
                    && spot_noise > 0.72;

                wasteland = is_polar_ice || is_erg_core || is_alpine_crest || is_spot_wasteland;
            }

            if wasteland {
                is_wasteland.set(x as isize, y as isize, true);
                continue; // Wasteland has 0 habitability
            }

            // Temperature bell-curve centred at 18 °C
            let temp_score   = (-((temp - 18.0) / 12.0).powf(2.0)).exp();
            // Precipitation score: higher floor so arid corridors/oases have viable habitability
            let precip_score = if precip < 0.08 { 0.18 } else if precip < 0.15 { 0.32 } else { precip.clamp(0.0, 1.0).sqrt() };
            // Slope: flat land scores higher
            let flat_score   = (1.0 - slope * 12.0).clamp(0.08, 1.0);
            // River adjacency bonus
            let river_bonus  = if flow >= min_river_flow { 1.4 } else { 1.0 };

            hab_grid.set(
                x as isize, y as isize,
                temp_score * precip_score * flat_score * river_bonus,
            );
        }
    }

    // ─── 1B. ENSURE HABITABLE LAND IS GLOBALLY CONNECTED TO COASTS ───────────
    // Start a BFS from all coastal habitable pixels (dist <= 2).
    // Any habitable pixel reached is connected to the continent's coast.
    let mut reached: Grid<bool> = Grid::filled_with(w, h, false);
    let mut q = VecDeque::new();

    for y in 0..h {
        for x in 0..w {
            let elev = *heightmap.get(x as isize, y as isize);
            if elev > sea_level && !*is_lake.get(x as isize, y as isize) && !*is_wasteland.get(x as isize, y as isize) {
                if *dist_to_water.get(x as isize, y as isize) <= 2 {
                    reached.set(x as isize, y as isize, true);
                    q.push_back((x as isize, y as isize));
                }
            }
        }
    }

    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    while let Some((cx, cy)) = q.pop_front() {
        for &(dx, dy) in &dirs {
            let nx = heightmap.wrap_x(cx + dx) as isize;
            let ny = cy + dy;
            if ny < 0 || ny >= h as isize { continue; }
            if *reached.get(nx, ny) { continue; }

            let elev = *heightmap.get(nx, ny);
            if elev > sea_level && !*is_lake.get(nx, ny) && !*is_wasteland.get(nx, ny) {
                reached.set(nx, ny, true);
                q.push_back((nx, ny));
            }
        }
    }

    // Convert any completely trapped interior pocket into wasteland
    for y in 0..h {
        for x in 0..w {
            let elev = *heightmap.get(x as isize, y as isize);
            if elev > sea_level && !*is_lake.get(x as isize, y as isize) && !*is_wasteland.get(x as isize, y as isize) {
                if !*reached.get(x as isize, y as isize) {
                    is_wasteland.set(x as isize, y as isize, true);
                    hab_grid.set(x as isize, y as isize, 0.0);
                }
            }
        }
    }

    let total_land_px = is_wasteland.raw_slice().iter().enumerate()
        .filter(|&(idx, _)| {
            let x = idx % w;
            let y = idx / w;
            let elev = *heightmap.get(x as isize, y as isize);
            elev > sea_level && !*is_lake.get(x as isize, y as isize)
        }).count();
    let total_waste_px = is_wasteland.raw_slice().iter().filter(|&&w| w).count();
    println!("✓ Physical Land: {} px | Wastelands: {} px ({:.1}%)",
        total_land_px, total_waste_px, (total_waste_px as f32 / total_land_px as f32) * 100.0);

    // ─── 2. SEED CANDIDATE SELECTION ─────────────────────────────────────────
    let stride = 4usize;
    let mut seed_candidates: Vec<(f32, isize, isize)> = Vec::new();

    for y in (0..h).step_by(stride) {
        for x in (0..w).step_by(stride) {
            if *is_wasteland.get(x as isize, y as isize) {
                continue; // Wastelands can never host province settlements
            }
            let hab = *hab_grid.get(x as isize, y as isize);
            if hab > 0.015 {
                let r_score = hab * rng.random_range(0.5..1.0_f32);
                seed_candidates.push((r_score, x as isize, y as isize));
            }
        }
    }

    seed_candidates.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    let base_min_dist: f32 = {
        let land_px = hab_grid
            .raw_slice()
            .iter()
            .filter(|&&v| v > 0.0)
            .count() as f32;
        let area_per_prov = land_px / config.num_provinces as f32;
        (area_per_prov / std::f32::consts::PI).sqrt().max(18.0)
    };

    let mut seeds: Vec<(isize, isize)> = Vec::with_capacity(config.num_provinces);

    for &(score, cx, cy) in &seed_candidates {
        if seeds.len() >= config.num_provinces { break; }

        let local_hab   = score;
        let min_dist    = (base_min_dist * (0.6 / (local_hab + 0.05).sqrt())).max(8.0);
        let min_dist_sq = (min_dist * min_dist) as isize;

        let mut too_close = false;
        for &(ax, ay) in &seeds {
            let mut dx = (cx - ax).abs();
            if dx > (w as isize) / 2 { dx = w as isize - dx; }
            let dy = cy - ay;
            if dx * dx + dy * dy < min_dist_sq {
                too_close = true;
                break;
            }
        }

        if !too_close {
            seeds.push((cx, cy));
        }
    }

    let num_seeds = seeds.len();
    println!("✓ Seeded {} province centers (excluding wastelands).", num_seeds);

    if num_seeds == 0 {
        return ProvinceResult {
            province_ids,
            habitability: hab_grid,
            is_wasteland,
            num_provinces: 0,
        };
    }

    // ─── 3. NEIGHBOUR OFFSETS ────────────────────────────────────────────────
    let neighbors: [(isize, isize, f32); 8] = [
        (-1,  0, 1.0), ( 1,  0, 1.0), ( 0, -1, 1.0), ( 0,  1, 1.0),
        (-1, -1, 1.414), ( 1, -1, 1.414), (-1,  1, 1.414), ( 1,  1, 1.414),
    ];

    // ─── 4. TWO-PASS RELAXED FLOOD FILL ──────────────────────────────────────
    for pass in 0..2usize {
        let is_last = pass == 1;
        province_ids = Grid::filled_with(w, h, 0u32);
        let mut heap: BinaryHeap<ExpandState> = BinaryHeap::new();

        for (idx, &(sx, sy)) in seeds.iter().enumerate() {
            let pid = (idx + 1) as u32;
            province_ids.set(sx, sy, pid);
            heap.push(ExpandState { cost: 0, x: sx, y: sy, id: pid });
        }

        while let Some(state) = heap.pop() {
            if *province_ids.get(state.x, state.y) != state.id { continue; }

            let current_elev = *heightmap.get(state.x, state.y);

            for &(dx, dy, dist) in &neighbors {
                let nx = heightmap.wrap_x(state.x + dx) as isize;
                let ny = state.y + dy;
                if ny < 0 || ny >= h as isize { continue; }

                let target_elev = *heightmap.get(nx, ny);
                // Borders stop dead at water AND wastelands!
                if target_elev <= sea_level || *is_lake.get(nx, ny) || *is_wasteland.get(nx, ny) {
                    continue;
                }

                if *province_ids.get(nx, ny) == 0 {
                    let elev_diff = (target_elev - current_elev).abs();

                    let mountain_penalty = (elev_diff * 350.0).min(45.0);
                    let river_flow       = *flow_grid.get(nx, ny);
                    let river_penalty    = if river_flow >= min_river_flow { 28.0 } else { 0.0 };

                    let move_cost = dist * 12.0 + mountain_penalty + river_penalty;
                    let new_cost  = state.cost + move_cost as u32;

                    province_ids.set(nx, ny, state.id);
                    heap.push(ExpandState { cost: new_cost, x: nx, y: ny, id: state.id });
                }
            }
        }

        // ── Lloyd centroid relaxation (after first pass) ──────────────────
        if !is_last {
            let mut sum_x  = vec![0.0f64; num_seeds + 1];
            let mut sum_y  = vec![0.0f64; num_seeds + 1];
            let mut counts = vec![0usize;  num_seeds + 1];

            for y in 0..h {
                for x in 0..w {
                    let id = *province_ids.get(x as isize, y as isize) as usize;
                    if id > 0 && id <= num_seeds {
                        sum_x[id]  += x as f64;
                        sum_y[id]  += y as f64;
                        counts[id] += 1;
                    }
                }
            }

            for idx in 0..num_seeds {
                let id = idx + 1;
                if counts[id] == 0 { continue; }

                let raw_cx = (sum_x[id] / counts[id] as f64).round() as isize;
                let raw_cy = (sum_y[id] / counts[id] as f64).round() as isize;
                let cx     = heightmap.wrap_x(raw_cx) as isize;
                let cy     = raw_cy.clamp(0, h as isize - 1);

                let candidate = find_valid_land(heightmap, is_lake, &is_wasteland, cx, cy, sea_level, h);
                if let Some(pos) = candidate {
                    seeds[idx] = pos;
                }
            }
        }
    }

    let mut seen = vec![false; num_seeds + 1];
    for &v in province_ids.raw_slice() {
        if v > 0 && (v as usize) <= num_seeds {
            seen[v as usize] = true;
        }
    }
    let actual_province_count = seen.iter().filter(|&&s| s).count();

    ProvinceResult {
        province_ids,
        habitability: hab_grid,
        is_wasteland,
        num_provinces: actual_province_count,
    }
}

/// Searches an expanding spiral for the nearest valid land pixel (not water, not wasteland).
fn find_valid_land(
    heightmap:    &Grid<f32>,
    is_lake:      &Grid<bool>,
    is_wasteland: &Grid<bool>,
    cx:           isize,
    cy:           isize,
    sea_level:    f32,
    h:            usize,
) -> Option<(isize, isize)> {
    if cy >= 0 && cy < h as isize {
        let elev = *heightmap.get(cx, cy);
        if elev > sea_level && !*is_lake.get(cx, cy) && !*is_wasteland.get(cx, cy) {
            return Some((cx, cy));
        }
    }

    for r in 1..=12isize {
        for dx in -r..=r {
            for dy in [-r, r] {
                let nx = heightmap.wrap_x(cx + dx) as isize;
                let ny = cy + dy;
                if ny < 0 || ny >= h as isize { continue; }
                let elev = *heightmap.get(nx, ny);
                if elev > sea_level && !*is_lake.get(nx, ny) && !*is_wasteland.get(nx, ny) {
                    return Some((nx, ny));
                }
            }
        }
        for dy in (-r + 1)..r {
            for dx in [-r, r] {
                let nx = heightmap.wrap_x(cx + dx) as isize;
                let ny = cy + dy;
                if ny < 0 || ny >= h as isize { continue; }
                let elev = *heightmap.get(nx, ny);
                if elev > sea_level && !*is_lake.get(nx, ny) && !*is_wasteland.get(nx, ny) {
                    return Some((nx, ny));
                }
            }
        }
    }
    None
}
