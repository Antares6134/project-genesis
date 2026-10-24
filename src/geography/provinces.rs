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
    permanent_ice: &Grid<bool>,
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

    // ── Permanent polar ice ───────────────────────────────────────────────────
    // The belt is not this generator's own invention: `polar::permanent_polar_ice`
    // decides where the world's permanent ice lies (the polar belt with its ragged
    // equatorward edge, plus every landmass cut off from a pole), the temperature
    // grid is pinned to match, and the finished mask arrives here as
    // `permanent_ice`. Ice overrides every sanctuary below, so no populated strip
    // can ring a pole or hug a polar coast — while a pole-connected continent may
    // still keep a ragged handful of provinces right at the rim of the ice. The
    // frozen seas beyond it — rendered as "polar land" by the atlas — are part of
    // the very same wasteland.

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

            // Permanent polar ice — read once, for every land pixel, frozen
            // coasts included. It is the populated strip along the ice rim that
            // used to let a single province snake round and round until it
            // stretched into a paper-thin band reaching the very pole.
            let on_polar_ice = *permanent_ice.get(x as isize, y as isize);

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

            if on_polar_ice {
                // Permafrost overrides every sanctuary: the coastal and river
                // belts can never reopen the ice, so no populated strip can ring
                // a pole or hug a polar coast.
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
    // The carver measures itself in province widths, so a corridor can never grow
    // into a province-long sliver (see [`carve_habitable_corridors`]).
    let corridor_span = province_span(
        hab_grid.raw_slice().iter().filter(|&&v| v > 0.0).count() as f32,
        config.num_provinces,
    );
    carve_habitable_corridors(
        heightmap,
        is_lake,
        &mut is_wasteland,
        &is_ice_cap,
        &is_sanctuary,
        &dist_to_water,
        sea_level,
        (MAX_CORRIDOR_SPANS * corridor_span).round() as usize,
    );

    // Every province has to be able to follow settled land to the sea — not
    // merely to a lake. The carver bridges what it can, but a pocket it could
    // not reach — and any corridor left dangling by such a pocket — is reverted
    // here, on the finished grid, so the map never keeps an island of
    // habitability with no path to the ocean.
    let stranded_px = revert_stranded_habitation(heightmap, is_lake, &mut is_wasteland, sea_level);
    if stranded_px > 0 {
        println!(
            "✓ Stranded habitation reverted: {} px (no path to the sea)",
            stranded_px
        );
    }

    // Wasteland carries no habitability, so the province seeder can never pick a
    // centre on it.
    //
    // Sanctuary ground is deliberately *not* re-opened here. The carver and the
    // pass above have the last word on what stays settled, and they revert a
    // pocket that cannot reach water — sanctuary or not. Quietly reviving such a
    // pocket here is exactly what used to leave an island of habitability
    // marooned in the desert. Coastal and river ground that *can* reach water is
    // already populated by construction, so nothing legitimate is lost.
    for y in 0..h {
        for x in 0..w {
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

    let (landmasses, networks, stranded, stranded_px) =
        connectivity_stats(heightmap, is_lake, &is_wasteland, sea_level);
    println!(
        "✓ Inhabited landmasses: {} | Populated province networks: {} | Provinces adrift: {} ({} px){}",
        landmasses,
        networks,
        stranded,
        stranded_px,
        if stranded == 0 {
            " — every network reaches the sea"
        } else {
            "  ⚠ WARNING: a populated network is cut off from the sea!"
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
        province_span(land_px, config.num_provinces)
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

/// True when `(x, y)` is open **sea** — the single connected world ocean —
/// as opposed to dry land or an inland lake.
///
/// [`is_land`] counts every stretch of water as "not land", which is what the
/// flood fills want, but the connectivity passes must tell salt water from a
/// lake: provinces are joined across the sea, so "can this network reach the
/// water?" has to mean "can it reach the *sea*?". A pocket that only laps a lake
/// is as cut off as one that touches nothing at all — it can never reach the
/// ocean, and therefore never the rest of the world.
fn is_sea(heightmap: &Grid<f32>, is_lake: &Grid<bool>, x: isize, y: isize, sea_level: f32) -> bool {
    *heightmap.get(x, y) <= sea_level && !*is_lake.get(x, y)
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

/// Narrowest spacing the province seeder ever leaves between two centres, in map
/// pixels. Wilderness, coastlines and rugged ground stretch the real spacing well
/// beyond it, but it is the floor every "is this gap one province wide?" question
/// is measured against.
const MIN_PROVINCE_SPAN: f32 = 18.0;

/// Longest corridor the carver may cut, in province widths (see
/// [`province_span`]). A pocket further from its landmass's populated network
/// than this is left to the wilderness instead of being threaded to it by a
/// province-long, three-pixel-wide sliver.
const MAX_CORRIDOR_SPANS: f32 = 2.5;

/// Typical width of one province, in map pixels: the centre-to-centre spacing the
/// seeder aims for when `land_px` pixels of land have to carry `num_provinces`
/// provinces. Used both to space the seeds and to keep carved corridors to the
/// scale of the provinces they join.
fn province_span(land_px: f32, num_provinces: usize) -> f32 {
    let area_per_province = land_px / num_provinces.max(1) as f32;
    (area_per_province / std::f32::consts::PI).sqrt().max(MIN_PROVINCE_SPAN)
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

/// Disjoint-set over populated components, used to tell which networks the
/// carver has already joined. Every group remembers whether any of its members
/// reaches the sea.
struct CompDsu {
    parent: Vec<u32>,
    coastal: Vec<bool>,
}

impl CompDsu {
    fn new(comp_touches_sea: &[bool]) -> Self {
        Self {
            parent: (0..comp_touches_sea.len() as u32).collect(),
            coastal: comp_touches_sea.to_vec(),
        }
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
            self.coastal[ra as usize] |= self.coastal[rb as usize];
        }
    }
    /// A network only needs a corridor while the sea cannot reach it. Once a
    /// group holds a single coastal province, every pocket in it — and every
    /// network it may later touch — is already reachable by water.
    fn needs_corridor(&mut self, i: u32) -> bool {
        let root = self.find(i) as usize;
        !self.coastal[root]
    }
}

/// Rejoin the populated networks that wilderness has cut off from the sea, and
/// only those; whatever cannot be rejoined falls back to wilderness.
///
/// Habitability islands (oases, alpine valleys, rainforest clearings) can end up
/// walled in by impassable ground. A pocket like that has no coast of its own: if
/// nothing joins it to the rest of its landmass it is a province adrift, reachable
/// by nothing. Those pockets are bridged by a corridor carved through the
/// wasteland — preferring the coast, because a corridor is cheapest where the
/// ground is lowest and nearest the water.
///
/// Two networks that each keep their own stretch of *sea* shore are *not*
/// bridged. Wilderness is allowed to split a landmass into several networks as
/// long as every one of them reaches the sea: the sea is a road, and the
/// geopolitical graph already joins coastal provinces across straits
/// ([`extract_province_graph`](crate::geopolitics::graph::extract_province_graph)),
/// while forcing them together on land would only smear a paper-thin province
/// along the waterline. That is what used to happen along the polar belt, where a
/// single corridor would hug a frozen coast for a thousand pixels.
///
/// A lake is not a shore in this sense. A network that only laps an inland lake
/// is as cut off as one touching nothing at all, so it still earns a corridor
/// ([`is_sea`]).
///
/// Two more limits keep the carving honest:
///
/// * **Permanent ice is impassable.** The polar belt is mandatory wilderness for
///   every other stage, and a corridor must not tunnel through it or punch holes
///   in it — a strip of provinces on the rim of an ice sheet is exactly the
///   thread-thin province this pass exists to avoid.
/// * **A corridor may not exceed `max_corridor` pixels.** A pocket further from
///   its landmass's network than that is left to the wilderness, where it at
///   least reads as one more patch of impassable ground.
///
/// Either way the outcome is the same: when the carve is done, **every surviving
/// populated network reaches the sea**. A network that could not be bridged within
/// the budget is reverted to wilderness — sanctuary ground included — so the map
/// never keeps an island of habitability stranded in the desert with no path to
/// the sea.
fn carve_habitable_corridors(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    is_wasteland: &mut Grid<bool>,
    is_ice_cap: &Grid<bool>,
    is_sanctuary: &Grid<bool>,
    dist_to_water: &Grid<u8>,
    sea_level: f32,
    max_corridor: usize,
) {
    let w = heightmap.width;
    let h = heightmap.height;
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    let mut hab_comp = Grid::filled_with(w, h, 0u32);
    let mut comp_size = vec![0usize];
    let mut comp_touches_sea = vec![false];
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
            let mut touches_sea = false;
            let mut touches_sanctuary = false;
            while let Some((cx, cy)) = q.pop_front() {
                size += 1;
                if *is_sanctuary.get(cx, cy) { touches_sanctuary = true; }
                for &(dx, dy) in &dirs {
                    let nx = heightmap.wrap_x(cx + dx) as isize;
                    let ny = cy + dy;
                    if ny < 0 || ny >= h as isize { continue; }
                    if is_sea(heightmap, is_lake, nx, ny, sea_level) {
                        touches_sea = true;
                        continue;
                    }
                    if !is_land(heightmap, is_lake, nx, ny, sea_level) {
                        // A lake: water, but no road to the sea. It grants the
                        // network nothing, so keep flooding past it.
                        continue;
                    }
                    if *hab_comp.get(nx, ny) != 0 { continue; }
                    if *is_wasteland.get(nx, ny) { continue; }
                    hab_comp.set(nx, ny, next_comp);
                    q.push_back((nx, ny));
                }
            }
            comp_size.push(size);
            comp_touches_sea.push(touches_sea);
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
                && !comp_touches_sea[id]
                && !comp_touches_sanctuary[id]
            {
                is_wasteland.set(x as isize, y as isize, true);
                hab_comp.set(x as isize, y as isize, 0);
            }
        }
    }

    let n_comp = next_comp as usize;
    // A landmass whose every network already reaches the sea has nothing to
    // gain from a corridor, so the whole expensive flood is skipped.
    if n_comp <= 2 || !(1..n_comp).any(|c| !comp_touches_sea[c]) {
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

    let mut mass_comp_count: Vec<u32> = vec![0; next_mass as usize];
    let mut seen_comp_on_mass = vec![false; n_comp];

    // How many populated networks each landmass carries. A landmass with a
    // single network has nothing for a corridor to join, so its ground is left
    // alone; a landmass with several may have pockets that need bridging.
    for y in 0..h {
        for x in 0..w {
            let cid = *hab_comp.get(x as isize, y as isize);
            if cid == 0 { continue; }
            let mass = *landmass.get(x as isize, y as isize) as usize;
            if !seen_comp_on_mass[cid as usize] {
                seen_comp_on_mass[cid as usize] = true;
                mass_comp_count[mass] += 1;
            }
        }
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

    let mut dsu = CompDsu::new(&comp_touches_sea);

    while let Some(state) = heap.pop() {
        let cur_owner = *owner.get(state.x, state.y);
        if cur_owner == 0 { continue; }

        for &(dx, dy) in &dirs {
            let nx = heightmap.wrap_x(state.x + dx) as isize;
            let ny = state.y + dy;
            if ny < 0 || ny >= h as isize { continue; }
            if !is_land(heightmap, is_lake, nx, ny, sea_level) { continue; }
            // Permanent ice is impassable: the polar belt is mandatory
            // wilderness, so no corridor may tunnel through it.
            if *is_ice_cap.get(nx, ny) { continue; }

            let step = if *is_wasteland.get(nx, ny) {
                1 + (*dist_to_water.get(nx, ny) as u32 / 6)
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
                // Wilderness may split a landmass wherever the sea can still
                // reach both halves; only a network without a coast of its own
                // is worth a corridor.
                if !dsu.needs_corridor(cur_owner) && !dsu.needs_corridor(n_owner) {
                    continue;
                }
                // ... and never a corridor longer than a couple of provinces:
                // beyond that the pocket stays wilderness instead of becoming a
                // province-long sliver.
                if path_len(&parent, state.x, state.y, w, h)
                    + path_len(&parent, nx, ny, w, h)
                    > max_corridor
                {
                    continue;
                }
                carve_path(is_wasteland, is_ice_cap, &parent, state.x, state.y, w, h);
                carve_path(is_wasteland, is_ice_cap, &parent, nx, ny, w, h);
                is_wasteland.set(state.x, state.y, false);
                is_wasteland.set(nx, ny, false);
                dsu.union(n_owner, cur_owner);
            }
        }
    }

    // Whatever could not be bridged within the budget is reverted to wilderness
    // by `revert_stranded_habitation`, which runs on the finished grid — so the
    // corridors carved here are judged together with the networks they joined,
    // and a corridor left dangling by a reverted pocket goes with it.
}

/// Reverts every populated network that cannot reach the **sea**, and returns
/// how many pixels of habitability that reclaimed.
///
/// The carver joins a landlocked pocket to its landmass's network, but a pocket
/// it could not reach — or a corridor it cut for a pocket that then failed to
/// connect — can still be left as a strip of settled ground walled in by
/// wilderness. Such a strip is a province adrift: no road, land or sea, reaches
/// it. This pass finds every populated network that touches no open sea and puts
/// it back under wilderness, so the map never keeps an island of habitability
/// with no path to the ocean.
///
/// "Reaches the sea" is not "touches any water": a network that only laps an
/// inland lake is still cut off from every other province in the world, so the
/// lake does not save it. Only a genuine stretch of salt water does
/// ([`is_sea`]).
///
/// It runs *after* the carving, on the finished grid, so a corridor is judged
/// together with the networks it joined: a corridor that leads to a coast keeps
/// its network alive, while one left dangling by a reverted pocket goes with it
/// instead of surviving as a thread-thin province of its own.
///
/// One pass is enough. A network's access to the sea is a property of its own
/// pixels — its edge either touches the sea or it does not — so removing a
/// stranded network can never strand another one.
fn revert_stranded_habitation(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    is_wasteland: &mut Grid<bool>,
    sea_level: f32,
) -> usize {
    let w = heightmap.width;
    let h = heightmap.height;
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let mut seen = Grid::filled_with(w, h, false);
    let mut q: VecDeque<(isize, isize)> = VecDeque::new();
    let mut cells: Vec<(isize, isize)> = Vec::new();
    let mut reverted = 0usize;

    for y0 in 0..h {
        for x0 in 0..w {
            let sx = x0 as isize;
            let sy = y0 as isize;
            if *seen.get(sx, sy) { continue; }
            if !is_land(heightmap, is_lake, sx, sy, sea_level) { continue; }
            if *is_wasteland.get(sx, sy) { continue; }

            // Flood the whole populated network, remembering its pixels and
            // watching for any stretch of open sea along its edge.
            cells.clear();
            let mut touches_sea = false;
            seen.set(sx, sy, true);
            q.clear();
            q.push_back((sx, sy));
            while let Some((cx, cy)) = q.pop_front() {
                cells.push((cx, cy));
                for &(dx, dy) in &dirs {
                    let nx = heightmap.wrap_x(cx + dx) as isize;
                    let ny = cy + dy;
                    if ny < 0 || ny >= h as isize {
                        continue;
                    }
                    if is_sea(heightmap, is_lake, nx, ny, sea_level) {
                        touches_sea = true;
                        continue;
                    }
                    if !is_land(heightmap, is_lake, nx, ny, sea_level) {
                        // A lake. The network can lap it and still be an
                        // island in every sense that matters: nothing but salt
                        // water puts a province in touch with the world.
                        continue;
                    }
                    if *is_wasteland.get(nx, ny) { continue; }
                    if *seen.get(nx, ny) { continue; }
                    seen.set(nx, ny, true);
                    q.push_back((nx, ny));
                }
            }

            if !touches_sea {
                for &(cx, cy) in &cells {
                    is_wasteland.set(cx, cy, true);
                }
                reverted += cells.len();
            }
        }
    }

    reverted
}

fn pack_xy(x: isize, y: isize, w: usize) -> u32 {
    (y as u32) * (w as u32) + (x as u32)
}

fn unpack_xy(p: u32, w: usize) -> (isize, isize) {
    ((p as usize % w) as isize, (p as usize / w) as isize)
}

/// Number of pixels a parent chain spans: `1` for a source, one more for every
/// step back towards it. It measures a prospective corridor before any of its
/// pixels are touched.
fn path_len(parent: &Grid<u32>, mut x: isize, mut y: isize, w: usize, h: usize) -> usize {
    let mut steps = 1usize;
    while steps <= w + h {
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
        steps += 1;
    }
    steps
}

/// Cuts a short, passable corridor along a parent-pointer chain. The chain and
/// its four-neighbours are opened up, but permanent ice is left alone: the polar
/// belt is impassable ground, and a corridor may hug it without ever piercing it.
fn carve_path(
    is_wasteland: &mut Grid<bool>,
    is_ice_cap: &Grid<bool>,
    parent: &Grid<u32>,
    mut x: isize,
    mut y: isize,
    w: usize,
    h: usize,
) {
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for _ in 0..(w + h) {
        is_wasteland.set(x, y, false);
        for &(dx, dy) in &dirs {
            let nx = is_wasteland.wrap_x(x + dx) as isize;
            let ny = y + dy;
            if ny < 0 || ny >= h as isize { continue; }
            if *is_ice_cap.get(nx, ny) { continue; }
            is_wasteland.set(nx, ny, false);
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

/// Counts inhabited landmasses, their populated province networks, and the
/// networks that cannot reach the sea — the "provinces adrift".
///
/// Wilderness is allowed to split a landmass's province network in two as long
/// as every piece keeps a stretch of its own coast: the sea is the road between
/// them ([`carve_habitable_corridors`]), so the world is not severed, and
/// forcing them together on land would only smear a thread-thin province along
/// the waterline.
///
/// What must never survive, though, is a network with no water at all. Every
/// province has to be able to follow settled land to a coast — its own, or one
/// reached through a corridor the carver cut. A populated network that can do
/// neither is a province adrift: no road, land or sea, reaches it. The carver is
/// expected to have either bridged it or reverted it to wilderness, so this
/// count is a hard zero on a healthy world, and the pixels reported are how much
/// habitability was reclaimed when it is not.
///
/// A landmass that is *entirely* wilderness — a lone ice-capped polar islet, for
/// instance — is not counted at all, since it could never host a province
/// network in the first place.
fn connectivity_stats(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    is_wasteland: &Grid<bool>,
    sea_level: f32,
) -> (u32, u32, u32, u32) {
    let w = heightmap.width;
    let h = heightmap.height;
    let dirs: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    // ── Pass 1: label the landmasses, noting which of them carry anyone -------
    let mut land_of = vec![u32::MAX; w * h];
    let mut mass_count = 0u32;
    let mut inhabited: Vec<bool> = vec![false];
    {
        let mut q: VecDeque<(isize, isize)> = VecDeque::new();
        for y0 in 0..h {
            for x0 in 0..w {
                let sx = x0 as isize;
                let sy = y0 as isize;
                if land_of[sy as usize * w + sx as usize] != u32::MAX { continue; }
                if !is_land(heightmap, is_lake, sx, sy, sea_level) { continue; }

                let mass = mass_count;
                mass_count += 1;
                let mut has_populated = false;
                land_of[sy as usize * w + sx as usize] = mass;
                q.clear();
                q.push_back((sx, sy));
                while let Some((cx, cy)) = q.pop_front() {
                    if !*is_wasteland.get(cx, cy) { has_populated = true; }
                    for &(dx, dy) in &dirs {
                        let nx = heightmap.wrap_x(cx + dx) as isize;
                        let ny = cy + dy;
                        if ny < 0 || ny >= h as isize { continue; }
                        let idx = ny as usize * w + nx as usize;
                        if land_of[idx] != u32::MAX { continue; }
                        if !is_land(heightmap, is_lake, nx, ny, sea_level) { continue; }
                        land_of[idx] = mass;
                        q.push_back((nx, ny));
                    }
                }
                inhabited.push(has_populated);
            }
        }
    }

    // ── Pass 2: label the populated networks and see which ones reach the sea -
    // Every populated network must be able to reach the sea: a coast of its own,
    // or a corridor of settled land joining it to one that has. A network with
    // neither is a province adrift, and the carver is expected to have either
    // bridged it or reverted it to wilderness. A lake does not count — a pocket
    // that only laps a lake can never reach the ocean, or the rest of the world.
    let mut network_count = 0u32;
    let mut stranded = 0u32;
    let mut stranded_px = 0u32;
    {
        let mut seen = Grid::filled_with(w, h, false);
        let mut q: VecDeque<(isize, isize)> = VecDeque::new();
        for y0 in 0..h {
            for x0 in 0..w {
                let sx = x0 as isize;
                let sy = y0 as isize;
                if *seen.get(sx, sy) { continue; }
                if !is_land(heightmap, is_lake, sx, sy, sea_level) { continue; }
                if *is_wasteland.get(sx, sy) { continue; }

                network_count += 1;
                let mut touches_sea = false;
                let mut size = 0u32;
                seen.set(sx, sy, true);
                q.clear();
                q.push_back((sx, sy));
                while let Some((cx, cy)) = q.pop_front() {
                    size += 1;
                    for &(dx, dy) in &dirs {
                        let nx = seen.wrap_x(cx + dx) as isize;
                        let ny = cy + dy;
                        if ny < 0 || ny >= h as isize {
                            continue;
                        }
                        if is_sea(heightmap, is_lake, nx, ny, sea_level) {
                            touches_sea = true;
                            continue;
                        }
                        if !is_land(heightmap, is_lake, nx, ny, sea_level) {
                            continue;
                        }
                        if *is_wasteland.get(nx, ny) { continue; }
                        if *seen.get(nx, ny) { continue; }
                        seen.set(nx, ny, true);
                        q.push_back((nx, ny));
                    }
                }
                if !touches_sea {
                    stranded += 1;
                    stranded_px += size;
                }
            }
        }
    }

    let landmasses = inhabited.iter().filter(|&&has| has).count() as u32;
    (landmasses, network_count, stranded, stranded_px)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Vertical datum of the little test worlds below: land is `0.50`, water `0.20`.
    const SEA_LEVEL: f32 = 0.40;
    const W: usize = 64;
    const H: usize = 48;
    /// The single landmass of a test world: a rectangle from (8, 8) to (56, 40).
    const LAND: (isize, isize, isize, isize) = (8, 8, 56, 40);
    /// The cap `generate_provinces` actually hands the carver, in pixels.
    const CAP: usize = (MAX_CORRIDOR_SPANS * MIN_PROVINCE_SPAN) as usize;

    /// A world with one rectangular landmass in the middle of an ocean. Every
    /// land pixel starts populated; a test turns the ground it wants impassable
    /// into wilderness (or permanent ice) and then carves.
    struct TestWorld {
        heightmap: Grid<f32>,
        is_lake: Grid<bool>,
        is_wasteland: Grid<bool>,
        is_ice_cap: Grid<bool>,
        is_sanctuary: Grid<bool>,
        dist_to_water: Grid<u8>,
    }

    impl TestWorld {
        fn new() -> Self {
            let mut heightmap = Grid::filled_with(W, H, 0.20f32);
            for y in LAND.1..LAND.3 {
                for x in LAND.0..LAND.2 {
                    heightmap.set(x, y, 0.50);
                }
            }
            let is_lake = Grid::filled_with(W, H, false);
            let dist_to_water = distances_to_water(&heightmap, &is_lake);
            Self {
                heightmap,
                is_lake,
                is_wasteland: Grid::filled_with(W, H, false),
                is_ice_cap: Grid::filled_with(W, H, false),
                is_sanctuary: Grid::filled_with(W, H, false),
                dist_to_water,
            }
        }

        /// Makes a rectangle of ground impassable wilderness.
        fn wilderness(&mut self, x0: isize, y0: isize, x1: isize, y1: isize) {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.is_wasteland.set(x, y, true);
                }
            }
        }

        /// Makes a rectangle of ground populated again (undoing [`Self::wilderness`]).
        fn settled(&mut self, x0: isize, y0: isize, x1: isize, y1: isize) {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.is_wasteland.set(x, y, false);
                }
            }
        }

        /// Turns a rectangle of ground into permanent polar ice: impassable, and
        /// never to be pierced.
        fn ice(&mut self, x0: isize, y0: isize, x1: isize, y1: isize) {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.is_wasteland.set(x, y, true);
                    self.is_ice_cap.set(x, y, true);
                }
            }
        }

        /// Marks a rectangle as sanctuary ground (a river course, or the coastal belt).
        fn sanctuary(&mut self, x0: isize, y0: isize, x1: isize, y1: isize) {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.is_sanctuary.set(x, y, true);
                }
            }
        }

        /// Runs the two passes the generator runs, in the order it runs them:
        /// carve the corridors, then revert whatever still cannot reach water.
        fn carve(&mut self, max_corridor: usize) {
            carve_habitable_corridors(
                &self.heightmap,
                &self.is_lake,
                &mut self.is_wasteland,
                &self.is_ice_cap,
                &self.is_sanctuary,
                &self.dist_to_water,
                SEA_LEVEL,
                max_corridor,
            );
            revert_stranded_habitation(
                &self.heightmap,
                &self.is_lake,
                &mut self.is_wasteland,
                SEA_LEVEL,
            );
        }

        fn populated(&self, x: isize, y: isize) -> bool {
            is_land(&self.heightmap, &self.is_lake, x, y, SEA_LEVEL)
                && !*self.is_wasteland.get(x, y)
        }

        /// Is there a four-connected path of populated ground between two pixels?
        fn joined(&self, from: (isize, isize), to: (isize, isize)) -> bool {
            let mut seen = Grid::filled_with(W, H, false);
            let mut q: VecDeque<(isize, isize)> = VecDeque::new();
            seen.set(from.0, from.1, true);
            q.push_back(from);
            while let Some((cx, cy)) = q.pop_front() {
                if (cx, cy) == to {
                    return true;
                }
                for &(dx, dy) in &[(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = self.is_wasteland.wrap_x(cx + dx) as isize;
                    let ny = cy + dy;
                    if ny < 0 || ny >= H as isize { continue; }
                    if *seen.get(nx, ny) || !self.populated(nx, ny) { continue; }
                    seen.set(nx, ny, true);
                    q.push_back((nx, ny));
                }
            }
            false
        }
    }

    /// Four-connected distance from every pixel to the nearest water, mirroring
    /// the chamfer transform the generator feeds the carver.
    fn distances_to_water(heightmap: &Grid<f32>, is_lake: &Grid<bool>) -> Grid<u8> {
        let mut dist = Grid::filled_with(W, H, 255u8);
        let mut q: VecDeque<(isize, isize)> = VecDeque::new();
        for y in 0..H {
            for x in 0..W {
                let (xi, yi) = (x as isize, y as isize);
                if !is_land(heightmap, is_lake, xi, yi, SEA_LEVEL) {
                    dist.set(xi, yi, 0);
                    q.push_back((xi, yi));
                }
            }
        }
        while let Some((cx, cy)) = q.pop_front() {
            let d = *dist.get(cx, cy);
            for &(dx, dy) in &[(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = dist.wrap_x(cx + dx) as isize;
                let ny = cy + dy;
                if ny < 0 || ny >= H as isize { continue; }
                if *dist.get(nx, ny) != 255 { continue; }
                dist.set(nx, ny, d.saturating_add(1));
                q.push_back((nx, ny));
            }
        }
        dist
    }

    #[test]
    fn wilderness_may_split_a_landmass_where_each_half_keeps_its_coast() {
        // A belt of wilderness across the landmass, with open sea on either side
        // of it: each half keeps a coast of its own, so the sea is already the
        // road between them and no corridor may be threaded through.
        let mut world = TestWorld::new();
        world.wilderness(30, LAND.1, 36, LAND.3);

        world.carve(CAP);

        assert!(
            world.is_wasteland.get(33, 24),
            "no corridor may be cut between two halves that each hold a coast"
        );
        assert!(
            !world.joined((12, 24), (50, 24)),
            "two coastal halves must stay separate networks"
        );
    }

    #[test]
    fn a_landlocked_pocket_is_bridged_to_its_landmass() {
        // A coastal band on the west shore and an oasis two provinces inland:
        // the oasis has no coast at all, so it must be joined to the band.
        let mut world = TestWorld::new();
        world.wilderness(LAND.0, LAND.1, LAND.2, LAND.3);
        world.settled(8, 8, 20, 40);
        world.settled(36, 20, 48, 28);

        world.carve(CAP);

        assert!(
            world.joined((12, 24), (40, 24)),
            "a pocket with no coast of its own must be bridged to its landmass"
        );
    }

    #[test]
    fn a_pocket_further_than_a_corridor_may_reach_stays_wilderness() {
        // The same oasis, but with a corridor budget too small to reach it: it
        // falls back to wilderness instead of growing a province-long sliver.
        let mut world = TestWorld::new();
        world.wilderness(LAND.0, LAND.1, LAND.2, LAND.3);
        world.settled(8, 8, 20, 40);
        world.settled(36, 20, 48, 28);

        world.carve(8);

        assert!(!world.joined((12, 24), (40, 24)), "no corridor may exceed the budget");
        assert!(world.is_wasteland.get(40, 24), "an unreachable pocket becomes wilderness");
    }

    #[test]
    fn permanent_ice_is_never_pierced() {
        // The only route from the coast to the inland pocket crosses an ice
        // sheet. The belt is mandatory wilderness, so the pocket stays cut off —
        // and not a single pixel of the ice may be opened up.
        let mut world = TestWorld::new();
        world.wilderness(LAND.0, LAND.1, LAND.2, LAND.3);
        world.settled(8, 8, 20, 40);
        world.settled(40, 20, 52, 28);
        world.ice(28, 0, 32, H as isize);

        world.carve(200); // generous: only the ice itself can stop the corridor

        assert!(!world.joined((12, 24), (44, 24)), "an ice sheet must never be pierced");
        assert!(world.is_wasteland.get(44, 24), "a pocket buried in ice stays wilderness");
        for y in 0..H as isize {
            for x in 28..32isize {
                assert!(
                    *world.is_wasteland.get(x, y),
                    "the ice at ({x}, {y}) must be left impassable"
                );
            }
        }
    }

    #[test]
    fn a_stranded_sanctuary_pocket_falls_back_to_wilderness() {
        // Sanctuary keeps the coastal belt and river courses settled — but only
        // while they can still reach water. A pocket of it beyond the corridor
        // budget, with no coast of its own, is an island of habitability in the
        // wilderness: it is reverted rather than left marooned, so no province
        // ends up adrift.
        let mut world = TestWorld::new();
        world.wilderness(LAND.0, LAND.1, LAND.2, LAND.3);
        world.settled(8, 8, 20, 40);
        world.settled(36, 20, 48, 28);
        world.sanctuary(36, 20, 48, 28);

        world.carve(8);

        assert!(
            !world.populated(40, 24),
            "an unreachable sanctuary pocket must not survive as isolated land"
        );
        assert!(
            world.populated(12, 24),
            "the coastal band keeps its own coast and stays settled"
        );
    }

    #[test]
    fn the_largest_network_is_not_spared_when_it_has_no_coast() {
        // The biggest populated network on a landmass used to be kept whatever
        // happened to it — which left a large inland block marooned whenever
        // the coast was a smaller, separate network. Size is no excuse: without
        // a path to water it falls back to wilderness like any other pocket.
        let mut world = TestWorld::new();
        world.wilderness(LAND.0, LAND.1, LAND.2, LAND.3);
        // A narrow coastal band on the west shore ...
        world.settled(8, 8, 14, 40);
        // ... and a much larger inland block, further from the water than the
        // corridor budget can reach.
        world.settled(30, 12, 54, 38);

        world.carve(8);

        assert!(
            world.populated(12, 24),
            "the coastal band keeps its own coast"
        );
        assert!(
            !world.populated(42, 24),
            "the largest network must not survive without a path to water"
        );
        assert!(
            !world.joined((12, 24), (42, 24)),
            "no corridor may exceed the budget"
        );
    }

    #[test]
    fn province_span_never_falls_below_one_province() {
        assert_eq!(province_span(0.0, 30_000), MIN_PROVINCE_SPAN);
        assert_eq!(province_span(1.0, 1_000_000), MIN_PROVINCE_SPAN);
        assert!(province_span(30_000.0 * 1_000_000.0, 30_000) > MIN_PROVINCE_SPAN);
    }
}
