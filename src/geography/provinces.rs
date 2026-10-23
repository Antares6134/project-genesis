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

    // ─── 1. HABITABILITY SCORE & WASTELAND IDENTIFICATION ─────────────────────
    // EU4-style wilderness, driven by *geography* rather than per-pixel noise:
    //   • LARGE regions — the hearts of continental deserts, high mountain
    //     plateaus and deep rainforest interiors (Sahara, Tibet, the Amazon)
    //   • SMALL pockets — sharp peaks, crags and badlands along mountain chains
    //     (the Alps, the Zagros, the Atlas)
    // Coastal strips and river corridors are always populated, so no landmass
    // is ever severed into disconnected province networks.
    let waste_noise = OpenSimplex::new(seed as u32 + 9999);
    let mut is_ice_cap = Grid::filled_with(w, h, false);
    let mut is_sanctuary = Grid::filled_with(w, h, false);
    // Per-pixel wilderness score (×100, 0 = sanctuary). Used to calibrate the
    // global cutoff once the whole world's climate is known.
    let mut hostility_map = Grid::filled_with(w, h, 0u8);

    // ── Wasteland tuning (distances are in map pixels) ────────────────────────
    const COAST_RING: f32 = 20.0;      // guaranteed-populated coastal belt
    const ICE_LOCK_TEMP: f32 = -14.0;  // frozen coasts may still be wilderness
    const MIN_WASTE_BLOB: usize = 64;  // discard sub-visible speckle
    const WASTE_CUTOFF: f32 = 0.68;    // hostility threshold

    // ── Permanent polar ice cap (latitudes are absolute radians) ──────────────
    // Beyond these latitudes the land becomes an unbroken cap of permanent ice:
    //   • provinces may still nibble at the fringe (≈66–74°), but the deep
    //     polar land is impassable, so no settlement can sprawl all the way to
    //     the pole — the converging longitudes there used to stretch single
    //     provinces into grotesque, thread-thin bands;
    //   • the frozen seas beyond it — rendered as "polar land" by the atlas —
    //     are part of the very same wasteland.
    const POLAR_ICE_START: f32 = 1.15;  // ≈ 65.9° — the ice begins to bite
    const POLAR_ICE_FULL: f32 = 1.40;   // ≈ 80.2° — solid cap from here inward
    const POLAR_ICE_NOISE: f32 = 0.34;  // organic wobble of the cap's edge
    const POLAR_ICE_CUTOFF: f32 = 0.55;

    for y in 0..h {
        let v = y as f32 / (h - 1) as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;
        let abs_lat = lat.abs();

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

            // ── Sanctuaries: coastal belt + river corridors stay populated ────
            // No wilderness is ever allowed within COAST_RING of water, so every
            // landmass keeps an unbroken ring of provinces around its shores.
            let coastal = (dist as f32) <= COAST_RING && temp > ICE_LOCK_TEMP;
            let river   = flow >= min_river_flow * 0.10;
            if coastal || river {
                is_sanctuary.set(x as isize, y as isize, true);
            }

            let mut wasteland = false;
            let mut ice = false;

            // ── Permanent polar ice cap ───────────────────────────────────────
            // Deliberately evaluated for *every* polar land pixel, frozen coasts
            // included: it is the populated strip along the ice rim that used to
            // let a single province snake round and round until it stretched
            // into a paper-thin band reaching the very pole.
            let polar_ice = ramp(abs_lat, POLAR_ICE_START, POLAR_ICE_FULL)
                + waste_noise.get([px * 26.0, py * 26.0, pz * 26.0]) as f32 * POLAR_ICE_NOISE
                > POLAR_ICE_CUTOFF;

            if !coastal && !river {
                let distf    = dist as f32;
                let no_river = 1.0 - ramp(flow, min_river_flow * 0.04, min_river_flow * 0.18);

                // Low-frequency wobble (~250 px) keeps wilderness edges organic,
                // with a medium-frequency ripple (~105 px) that shreds the fringe
                // into the small satellite pockets EU4 maps are full of.
                let macro_n = waste_noise.get([px * 30.0, py * 30.0, pz * 30.0]) as f32;
                let mid_n   = waste_noise.get([px * 78.0, py * 78.0, pz * 78.0]) as f32;
                let wobble  = macro_n * 0.12 + mid_n * 0.07;

                // ── Large wilderness hearts ───────────────────────────────────
                // Hyper-arid interior — the Sahara / Arabian / Gobi heart.
                let desert_heart = (1.0 - ramp(precip, 0.055, 0.150))
                    * ramp(distf, 14.0, 60.0)
                    * no_river;

                // High massif / plateau — Tibet, the altiplano, the Rockies.
                let plateau_heart = ramp(altitude, 0.035, 0.130)
                    * ramp(distf, 8.0, 45.0);

                // Deep rainforest interior — the Amazon / Congo basins.
                let jungle_heart = ramp(precip, 0.60, 0.85)
                    * ramp(temp, 23.0, 29.0)
                    * ramp(distf, 16.0, 65.0)
                    * no_river;

                // ── Small pockets on rugged ground ────────────────────────────
                // Steep crags and peaks (Alps, Zagros, Atlas, Himalaya foothills).
                // A medium-frequency noise scatters them so mountain chains break
                // into individual pockets instead of one long ridge.
                let rugged = (0.55 * ramp(altitude, 0.006, 0.042)
                    + 0.45 * ramp(slope, 0.0012, 0.0065)).min(1.0);
                let pocket = rugged * ramp(mid_n, -0.12, 0.30);

                let hostility = 1.00 * desert_heart
                    + 1.00 * plateau_heart
                    + 0.95 * jungle_heart
                    + 0.95 * pocket
                    + wobble;

                // Frozen interiors (polar ice sheets) — always impassable.
                ice = temp < -8.0 && abs_lat > 1.05 && ramp(distf, 8.0, 40.0) > 0.2;

                // Remember the score (×100, floored at 1 so it never collides
                // with the sanctuary marker 0) for the budget calibration below.
                let q = if ice {
                    255u8
                } else {
                    (hostility * 100.0).clamp(1.0, 255.0) as u8
                };
                hostility_map.set(x as isize, y as isize, q);

                wasteland = ice || hostility > WASTE_CUTOFF;
            }

            if polar_ice {
                // Polar ice overrides every sanctuary: the deep cap can never be
                // reopened by the coastal/river belt, so no populated strip can
                // ever ring the pole.
                wasteland = true;
                ice = true;
            }

            if ice {
                is_ice_cap.set(x as isize, y as isize, true);
                // 255 marks permanent ice: mandatory wilderness that the global
                // wilderness budget below must never dilute away.
                hostility_map.set(x as isize, y as isize, 255);
            }
            if wasteland {
                is_wasteland.set(x as isize, y as isize, true);
            }

            let temp_score   = (-((temp - 18.0) / 12.0).powf(2.0)).exp();
            let precip_score = if precip < 0.08 { 0.18 } else if precip < 0.15 { 0.32 } else { precip.clamp(0.0, 1.0).sqrt() };
            let flat_score   = (1.0 - slope * 12.0).clamp(0.08, 1.0);
            let river_bonus  = if flow >= min_river_flow { 1.4 } else { 1.0 };
            hab_grid.set(
                x as isize, y as isize,
                temp_score * precip_score * flat_score * river_bonus,
            );
        }
    }

    // ─── 1b. CALIBRATE THE WILDERNESS BUDGET ─────────────────────────────────
    // Worlds vary enormously in how much hyper-arid or high ground they hold: a
    // supercontinent can easily be 25% desert. Cap the share of land allowed to
    // become impassable so no seed is buried under wilderness — while a mild
    // world is never given *extra* wasteland. Permanent polar ice is left out of
    // the accounting entirely: it is mandatory wilderness, not a tunable.
    {
        const MAX_WASTE_SHARE: f64 = 0.18;
        let min_q = (WASTE_CUTOFF * 100.0) as usize;

        let mut hist = [0u32; 256];
        let mut total_land_px = 0u64;   // every land pixel, polar ice included
        let mut budget_land_px = 0u64;  // ice-free land the budget actually governs
        for y in 0..h {
            for x in 0..w {
                let xi = x as isize;
                let yi = y as isize;
                if !is_land(heightmap, is_lake, xi, yi, sea_level) { continue; }
                total_land_px += 1;
                // Permanent polar/glacial ice is geography, not noise: it is
                // mandatory wilderness, so it must never be budgeted away — nor
                // allowed to inflate the wilderness share until the
                // climate-driven deserts are squeezed out of existence.
                if *is_ice_cap.get(xi, yi) { continue; }
                budget_land_px += 1;
                hist[*hostility_map.get(xi, yi) as usize] += 1;
            }
        }

        let raw: u64 = hist[min_q.min(255)..].iter().map(|&c| c as u64).sum();
        let budget = (budget_land_px as f64 * MAX_WASTE_SHARE) as u64;

        if raw > budget {
            let mut acc = 0u64;
            let mut cutoff_q = 255u32;
            for v in (1..=255u32).rev() {
                acc += hist[v as usize] as u64;
                if acc > budget {
                    cutoff_q = v + 1;
                    break;
                }
            }
            let cut = cutoff_q.min(255) as u8;
            println!(
                "✓ Wilderness would cover {:.1}% of the ice-free land ({:.1}% of all land) — raising the threshold to {:.2} (ceiling {:.0}%).",
                raw as f64 / budget_land_px.max(1) as f64 * 100.0,
                raw as f64 / total_land_px.max(1) as f64 * 100.0,
                cut as f32 / 100.0,
                MAX_WASTE_SHARE * 100.0,
            );
            for y in 0..h {
                for x in 0..w {
                    let xi = x as isize;
                    let yi = y as isize;
                    // Ice caps stay impassable however the budget moves.
                    if *is_ice_cap.get(xi, yi) { continue; }
                    let v = *hostility_map.get(xi, yi);
                    is_wasteland.set(xi, yi, v > 0 && v >= cut);
                }
            }
        }
    }

    // Discard sub-visible speckle so patches read as wilderness, not noise.
    remove_small_wasteland_blobs(&mut is_wasteland, &is_ice_cap, w, h, MIN_WASTE_BLOB);
    // Solidify the surviving cores (still sparing sanctuaries and ice caps).
    dilate_non_ice_wasteland(&mut is_wasteland, &is_ice_cap, &is_sanctuary, heightmap, is_lake, sea_level, 1);

    // Carve populated corridors through remaining barriers (Maghreb ↔ West Africa).
    carve_habitable_corridors(
        heightmap,
        is_lake,
        &mut is_wasteland,
        &mut is_ice_cap,
        &is_sanctuary,
        &dist_to_water,
        sea_level,
    );

    // Sanctuaries must stay populated even after dilation-like side effects.
    for y in 0..h {
        for x in 0..w {
            if *is_sanctuary.get(x as isize, y as isize) && !*is_ice_cap.get(x as isize, y as isize) {
                is_wasteland.set(x as isize, y as isize, false);
            }
            if *is_wasteland.get(x as isize, y as isize) {
                hab_grid.set(x as isize, y as isize, 0.0);
            }
        }
    }

    let (total_land_px, total_waste_px, waste_blobs, max_non_ice) = wasteland_stats(
        heightmap, is_lake, &is_wasteland, &is_ice_cap, sea_level,
    );
    println!(
        "✓ Physical Land: {} px | Wastelands (incl. polar ice): {} px ({:.1}%)",
        total_land_px,
        total_waste_px,
        (total_waste_px as f32 / total_land_px.max(1) as f32) * 100.0,
    );
    println!(
        "✓ Wasteland patches: {} | Largest non-ice patch: {} px",
        waste_blobs, max_non_ice
    );

    let (landmasses, networks) = connectivity_stats(heightmap, is_lake, &is_wasteland, sea_level);
    println!(
        "✓ Inhabited landmasses: {} | Populated province networks: {}{}",
        landmasses,
        networks,
        if landmasses == networks {
            " (every inhabited landmass fully connected)"
        } else {
            "  ⚠ WARNING: an inhabited landmass was severed by wilderness!"
        }
    );

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

fn is_land(heightmap: &Grid<f32>, is_lake: &Grid<bool>, x: isize, y: isize, sea_level: f32) -> bool {
    *heightmap.get(x, y) > sea_level && !*is_lake.get(x, y)
}

fn dilate_non_ice_wasteland(
    is_wasteland: &mut Grid<bool>,
    is_ice_cap: &Grid<bool>,
    is_sanctuary: &Grid<bool>,
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    sea_level: f32,
    radius: usize,
) {
    let w = is_wasteland.width;
    let h = is_wasteland.height;
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for _ in 0..radius {
        let mut add = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let xi = x as isize;
                let yi = y as isize;
                if *is_wasteland.get(xi, yi) { continue; }
                if *is_sanctuary.get(xi, yi) || *is_ice_cap.get(xi, yi) { continue; }
                if !is_land(heightmap, is_lake, xi, yi, sea_level) { continue; }
                for &(dx, dy) in &dirs {
                    let nx = is_wasteland.wrap_x(xi + dx) as isize;
                    let ny = yi + dy;
                    if ny < 0 || ny >= h as isize { continue; }
                    if *is_wasteland.get(nx, ny) && !*is_ice_cap.get(nx, ny) {
                        add.push((xi, yi));
                        break;
                    }
                }
            }
        }
        for (x, y) in add {
            is_wasteland.set(x, y, true);
        }
    }
}

/// Smooth 0→1 ramp: 0.0 at `lo`, 1.0 at `hi` (assumes `lo < hi`).
#[inline(always)]
fn ramp(x: f32, lo: f32, hi: f32) -> f32 {
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// Drops wasteland blobs smaller than `min_size` so the map shows real
/// wilderness regions instead of scattered single-pixel speckle.
/// Blobs that contain polar ice are always preserved.
fn remove_small_wasteland_blobs(
    is_wasteland: &mut Grid<bool>,
    is_ice_cap: &Grid<bool>,
    w: usize,
    h: usize,
    min_size: usize,
) {
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let mut seen = Grid::filled_with(w, h, false);
    let mut stack: Vec<(isize, isize)> = Vec::new();
    let mut cells: Vec<(isize, isize)> = Vec::new();

    for y0 in 0..h {
        for x0 in 0..w {
            let sx = x0 as isize;
            let sy = y0 as isize;
            if *seen.get(sx, sy) || !*is_wasteland.get(sx, sy) { continue; }

            cells.clear();
            stack.clear();
            seen.set(sx, sy, true);
            stack.push((sx, sy));
            let mut any_ice = false;

            while let Some((cx, cy)) = stack.pop() {
                cells.push((cx, cy));
                if *is_ice_cap.get(cx, cy) { any_ice = true; }
                for &(dx, dy) in &dirs {
                    let nx = is_wasteland.wrap_x(cx + dx) as isize;
                    let ny = cy + dy;
                    if ny < 0 || ny >= h as isize { continue; }
                    if *seen.get(nx, ny) { continue; }
                    if !*is_wasteland.get(nx, ny) { continue; }
                    seen.set(nx, ny, true);
                    stack.push((nx, ny));
                }
            }

            if !any_ice && cells.len() < min_size {
                for &(cx, cy) in &cells {
                    is_wasteland.set(cx, cy, false);
                }
            }
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct CarveState {
    cost: u32,
    x: isize,
    y: isize,
}

impl Ord for CarveState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.x.cmp(&other.x))
            .then_with(|| self.y.cmp(&other.y))
    }
}

impl PartialOrd for CarveState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct CompDsu {
    parent: Vec<u32>,
}

impl CompDsu {
    fn new(n: usize) -> Self {
        Self { parent: (0..n as u32).collect() }
    }
    fn find(&mut self, mut i: u32) -> u32 {
        while self.parent[i as usize] != i {
            self.parent[i as usize] = self.parent[self.parent[i as usize] as usize];
            i = self.parent[i as usize];
        }
        i
    }
    fn union(&mut self, a: u32, b: u32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[rb as usize] = ra;
        }
    }
}

/// Guarantee each landmass has a single populated-province network by carving
/// short corridors through non-ice wasteland (preferring coasts).
fn carve_habitable_corridors(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    is_wasteland: &mut Grid<bool>,
    is_ice_cap: &mut Grid<bool>,
    is_sanctuary: &Grid<bool>,
    dist_to_water: &Grid<u8>,
    sea_level: f32,
) {
    let w = heightmap.width;
    let h = heightmap.height;
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    let mut hab_comp = Grid::filled_with(w, h, 0u32);
    let mut comp_size = vec![0usize];
    let mut comp_touches_water = vec![false];
    let mut comp_touches_sanctuary = vec![false];
    let mut next_comp = 1u32;
    let mut q = VecDeque::new();

    for y in 0..h {
        for x in 0..w {
            let xi = x as isize;
            let yi = y as isize;
            if *hab_comp.get(xi, yi) != 0 { continue; }
            if !is_land(heightmap, is_lake, xi, yi, sea_level) { continue; }
            if *is_wasteland.get(xi, yi) { continue; }

            hab_comp.set(xi, yi, next_comp);
            q.clear();
            q.push_back((xi, yi));
            let mut size = 0usize;
            let mut touches_water = false;
            let mut touches_sanctuary = false;
            while let Some((cx, cy)) = q.pop_front() {
                size += 1;
                if *is_sanctuary.get(cx, cy) { touches_sanctuary = true; }
                for &(dx, dy) in &dirs {
                    let nx = heightmap.wrap_x(cx + dx) as isize;
                    let ny = cy + dy;
                    if ny < 0 || ny >= h as isize { touches_water = true; continue; }
                    if !is_land(heightmap, is_lake, nx, ny, sea_level) {
                        touches_water = true;
                        continue;
                    }
                    if *hab_comp.get(nx, ny) != 0 { continue; }
                    if *is_wasteland.get(nx, ny) { continue; }
                    hab_comp.set(nx, ny, next_comp);
                    q.push_back((nx, ny));
                }
            }
            comp_size.push(size);
            comp_touches_water.push(touches_water);
            comp_touches_sanctuary.push(touches_sanctuary);
            next_comp += 1;
        }
    }

    // Fill tiny *enclosed* pockets (oases smaller than a single province) back
    // into wilderness. Real small islands and river/coastal sanctuaries are
    // kept — they must stay populated.
    const MIN_OASIS: usize = 80;
    for y in 0..h {
        for x in 0..w {
            let id = *hab_comp.get(x as isize, y as isize) as usize;
            if id > 0
                && comp_size[id] < MIN_OASIS
                && !comp_touches_water[id]
                && !comp_touches_sanctuary[id]
            {
                is_wasteland.set(x as isize, y as isize, true);
                hab_comp.set(x as isize, y as isize, 0);
            }
        }
    }

    let n_comp = next_comp as usize;
    if n_comp <= 2 {
        return;
    }

    let mut landmass = Grid::filled_with(w, h, 0u32);
    let mut next_mass = 1u32;
    for y in 0..h {
        for x in 0..w {
            let xi = x as isize;
            let yi = y as isize;
            if *landmass.get(xi, yi) != 0 { continue; }
            if !is_land(heightmap, is_lake, xi, yi, sea_level) { continue; }
            landmass.set(xi, yi, next_mass);
            q.clear();
            q.push_back((xi, yi));
            while let Some((cx, cy)) = q.pop_front() {
                for &(dx, dy) in &dirs {
                    let nx = heightmap.wrap_x(cx + dx) as isize;
                    let ny = cy + dy;
                    if ny < 0 || ny >= h as isize { continue; }
                    if *landmass.get(nx, ny) != 0 { continue; }
                    if !is_land(heightmap, is_lake, nx, ny, sea_level) { continue; }
                    landmass.set(nx, ny, next_mass);
                    q.push_back((nx, ny));
                }
            }
            next_mass += 1;
        }
    }

    let mut mass_largest: Vec<u32> = vec![0; next_mass as usize];
    let mut mass_largest_size: Vec<usize> = vec![0; next_mass as usize];
    let mut mass_comp_count: Vec<u32> = vec![0; next_mass as usize];
    let mut seen_comp_on_mass = vec![false; n_comp];

    for y in 0..h {
        for x in 0..w {
            let cid = *hab_comp.get(x as isize, y as isize);
            if cid == 0 { continue; }
            let mass = *landmass.get(x as isize, y as isize) as usize;
            if !seen_comp_on_mass[cid as usize] {
                seen_comp_on_mass[cid as usize] = true;
                mass_comp_count[mass] += 1;
                let sz = comp_size[cid as usize];
                if sz > mass_largest_size[mass] {
                    mass_largest_size[mass] = sz;
                    mass_largest[mass] = cid;
                }
            }
        }
    }

    let needs_carve = mass_comp_count.iter().any(|&c| c > 1);
    if !needs_carve {
        return;
    }

    let mut owner = Grid::filled_with(w, h, 0u32);
    let mut parent = Grid::filled_with(w, h, u32::MAX);
    let mut heap: BinaryHeap<CarveState> = BinaryHeap::new();

    for y in 0..h {
        for x in 0..w {
            let cid = *hab_comp.get(x as isize, y as isize);
            if cid == 0 { continue; }
            let mass = *landmass.get(x as isize, y as isize) as usize;
            if mass_comp_count[mass] <= 1 { continue; }
            let xi = x as isize;
            let yi = y as isize;
            owner.set(xi, yi, cid);
            parent.set(xi, yi, pack_xy(xi, yi, w));
            heap.push(CarveState { cost: 0, x: xi, y: yi });
        }
    }

    let mut dsu = CompDsu::new(n_comp);
    // Crossing an ice sheet is enormously expensive, so a corridor only ever
    // cuts through one when no other route can rejoin the landmass.
    const ICE_STEP: u32 = 400_000;

    while let Some(state) = heap.pop() {
        let cur_owner = *owner.get(state.x, state.y);
        if cur_owner == 0 { continue; }

        for &(dx, dy) in &dirs {
            let nx = heightmap.wrap_x(state.x + dx) as isize;
            let ny = state.y + dy;
            if ny < 0 || ny >= h as isize { continue; }
            if !is_land(heightmap, is_lake, nx, ny, sea_level) { continue; }

            let step = if *is_wasteland.get(nx, ny) {
                if *is_ice_cap.get(nx, ny) {
                    ICE_STEP
                } else {
                    1 + (*dist_to_water.get(nx, ny) as u32 / 6)
                }
            } else {
                0
            };
            let new_cost = state.cost.saturating_add(step);
            let n_owner = *owner.get(nx, ny);

            if n_owner == 0 {
                owner.set(nx, ny, cur_owner);
                parent.set(nx, ny, pack_xy(state.x, state.y, w));
                heap.push(CarveState { cost: new_cost, x: nx, y: ny });
            } else if dsu.find(n_owner) != dsu.find(cur_owner) {
                carve_path(is_wasteland, is_ice_cap, &parent, state.x, state.y, w, h);
                carve_path(is_wasteland, is_ice_cap, &parent, nx, ny, w, h);
                is_wasteland.set(state.x, state.y, false);
                is_wasteland.set(nx, ny, false);
                dsu.union(n_owner, cur_owner);
            }
        }
    }

    // Networks that could not join their landmass (ice-locked interiors) fall
    // back to wilderness. Sanctuary pixels are spared so the coastal belt — and
    // with it every landmass's province connectivity — is never broken.
    for y in 0..h {
        for x in 0..w {
            let cid = *hab_comp.get(x as isize, y as isize);
            if cid == 0 { continue; }
            let mass = *landmass.get(x as isize, y as isize) as usize;
            let largest = mass_largest[mass];
            if largest == 0 { continue; }
            if dsu.find(cid) != dsu.find(largest) {
                let xi = x as isize;
                let yi = y as isize;
                if !*is_sanctuary.get(xi, yi) {
                    is_wasteland.set(xi, yi, true);
                }
            }
        }
    }
}

fn pack_xy(x: isize, y: isize, w: usize) -> u32 {
    (y as u32) * (w as u32) + (x as u32)
}

fn unpack_xy(p: u32, w: usize) -> (isize, isize) {
    ((p as usize % w) as isize, (p as usize / w) as isize)
}

/// Cuts a short passable corridor along a parent-pointer chain. Ice pixels are
/// cleared too — a severed landmass is always worse than a pierced ice sheet.
fn carve_path(
    is_wasteland: &mut Grid<bool>,
    is_ice_cap: &mut Grid<bool>,
    parent: &Grid<u32>,
    mut x: isize,
    mut y: isize,
    w: usize,
    h: usize,
) {
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for _ in 0..(w + h) {
        is_wasteland.set(x, y, false);
        is_ice_cap.set(x, y, false);
        for &(dx, dy) in &dirs {
            let nx = is_wasteland.wrap_x(x + dx) as isize;
            let ny = y + dy;
            if ny < 0 || ny >= h as isize { continue; }
            is_wasteland.set(nx, ny, false);
            is_ice_cap.set(nx, ny, false);
        }
        let packed = *parent.get(x, y);
        if packed == u32::MAX {
            break;
        }
        let (px, py) = unpack_xy(packed, w);
        if px == x && py == y {
            break;
        }
        x = px;
        y = py;
    }
}

/// Counts inhabited landmasses and populated (non-wasteland) province networks.
///
/// A correctly generated world has exactly one populated network per inhabited
/// landmass: wilderness may never sever a continent into disconnected province
/// clusters. Landmasses that are *entirely* wilderness — a lone ice-capped
/// polar islet, for instance — are not counted at all, since they could never
/// host a province network in the first place.
fn connectivity_stats(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    is_wasteland: &Grid<bool>,
    sea_level: f32,
) -> (u32, u32) {
    let w = heightmap.width;
    let h = heightmap.height;
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let count = |populated_only: bool| -> u32 {
        let mut seen = Grid::filled_with(w, h, false);
        let mut comps = 0u32;
        let mut q: VecDeque<(isize, isize)> = VecDeque::new();
        for y0 in 0..h {
            for x0 in 0..w {
                let sx = x0 as isize;
                let sy = y0 as isize;
                if *seen.get(sx, sy) { continue; }
                if !is_land(heightmap, is_lake, sx, sy, sea_level) { continue; }
                if populated_only && *is_wasteland.get(sx, sy) { continue; }
                seen.set(sx, sy, true);
                q.clear();
                q.push_back((sx, sy));
                let mut has_populated = false;
                while let Some((cx, cy)) = q.pop_front() {
                    if !*is_wasteland.get(cx, cy) { has_populated = true; }
                    for &(dx, dy) in &dirs {
                        let nx = seen.wrap_x(cx + dx) as isize;
                        let ny = cy + dy;
                        if ny < 0 || ny >= h as isize { continue; }
                        if *seen.get(nx, ny) { continue; }
                        if !is_land(heightmap, is_lake, nx, ny, sea_level) { continue; }
                        if populated_only && *is_wasteland.get(nx, ny) { continue; }
                        seen.set(nx, ny, true);
                        q.push_back((nx, ny));
                    }
                }
                if populated_only || has_populated {
                    comps += 1;
                }
            }
        }
        comps
    };
    (count(false), count(true))
}

fn wasteland_stats(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    is_wasteland: &Grid<bool>,
    is_ice_cap: &Grid<bool>,
    sea_level: f32,
) -> (usize, usize, usize, usize) {
    let w = heightmap.width;
    let h = heightmap.height;
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let mut land = 0usize;
    let mut waste = 0usize;
    for y in 0..h {
        for x in 0..w {
            if is_land(heightmap, is_lake, x as isize, y as isize, sea_level) {
                land += 1;
                if *is_wasteland.get(x as isize, y as isize) {
                    waste += 1;
                }
            }
        }
    }

    let mut seen = Grid::filled_with(w, h, false);
    let mut blobs = 0usize;
    let mut max_non_ice = 0usize;
    let mut q = VecDeque::new();

    for y0 in 0..h {
        for x0 in 0..w {
            let sx = x0 as isize;
            let sy = y0 as isize;
            if *seen.get(sx, sy) || !*is_wasteland.get(sx, sy) { continue; }
            blobs += 1;
            let mut size = 0usize;
            let mut any_ice = false;
            seen.set(sx, sy, true);
            q.clear();
            q.push_back((sx, sy));
            while let Some((cx, cy)) = q.pop_front() {
                size += 1;
                if *is_ice_cap.get(cx, cy) { any_ice = true; }
                for &(dx, dy) in &dirs {
                    let nx = is_wasteland.wrap_x(cx + dx) as isize;
                    let ny = cy + dy;
                    if ny < 0 || ny >= h as isize { continue; }
                    if *seen.get(nx, ny) { continue; }
                    if !*is_wasteland.get(nx, ny) { continue; }
                    seen.set(nx, ny, true);
                    q.push_back((nx, ny));
                }
            }
            if !any_ice {
                max_non_ice = max_non_ice.max(size);
            }
        }
    }

    (land, waste, blobs, max_non_ice)
}
