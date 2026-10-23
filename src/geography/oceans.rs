//! Subdivision of the world ocean into its own multi-tier hydro-political
//! hierarchy.
//!
//! The land is partitioned into *provinces, areas, regions, superregions and
//! continents*; the ocean receives exactly the same treatment so that the atlas
//! can be read politically on water just as it is on land. Because the ocean is
//! by far the larger domain, every tier is scaled up — a single ocean province
//! is many times the area of a land province, so the resulting basins read as
//! genuinely planetary features rather than a fine mesh.
//!
//! This module produces the base tier only (the marine *provinces*). The parent
//! tiers are assembled from these by the same graph-clustering machinery the
//! land hierarchy uses (`geopolitics::build_ocean_hierarchy`).

use crate::core::Grid;
use crate::geography::config::OceanConfig;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::BinaryHeap;

/// The base tier of the ocean hierarchy: a grid of marine province IDs.
///
/// Ocean pixels (open water *and* the frozen polar seas — the Arctic and
/// Southern Oceans are still oceans) carry IDs `1..=num_provinces`; land and
/// inland freshwater lakes are `0`.
#[derive(Clone)]
pub struct OceanResult {
    pub ocean_province_ids: Grid<u32>,
    pub num_provinces: usize,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct OceanState {
    cost: u32,
    x: isize,
    y: isize,
    id: u32,
}

impl Ord for OceanState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.x.cmp(&other.x))
            .then_with(|| self.y.cmp(&other.y))
    }
}

impl PartialOrd for OceanState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// True when a pixel is world ocean — open sea or frozen polar sea alike.
///
/// Inland freshwater lakes are deliberately excluded: they are hydrology
/// features of the continents, not part of the world ocean.
#[inline(always)]
fn is_ocean(heightmap: &Grid<f32>, is_lake: &Grid<bool>, x: isize, y: isize, sea_level: f32) -> bool {
    *heightmap.get(x, y) <= sea_level && !*is_lake.get(x, y)
}


/// Nearest ocean pixel to `(sx, sy)`, searched in expanding square rings.
///
/// Used after centroid relaxation, where the floating-point centroid of a
/// province can round onto a coastline pixel.
fn snap_to_ocean(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    sx: isize,
    sy: isize,
    sea_level: f32,
) -> (isize, isize) {
    if is_ocean(heightmap, is_lake, sx, sy, sea_level) {
        return (sx, sy);
    }
    let h = heightmap.height as isize;
    for r in 1..=64isize {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let nx = heightmap.wrap_x(sx + dx) as isize;
                let ny = sy + dy;
                if ny < 0 || ny >= h {
                    continue;
                }
                if is_ocean(heightmap, is_lake, nx, ny, sea_level) {
                    return (nx, ny);
                }
            }
        }
    }
    (heightmap.wrap_x(sx) as isize, sy.clamp(0, h - 1))
}

/// Multi-source Dijkstra flood fill: every ocean pixel is claimed by the
/// lowest-cost seed.
///
/// Bathymetry acts as a soft barrier (deep-water transitions cost more), so
/// marine province borders naturally prefer to follow mid-ocean ridges,
/// trenches and continental shelves instead of running as arbitrary straight
/// lines across open water.
fn flood_fill(
    seeds: &[(u32, isize, isize)],
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    sea_level: f32,
) -> Grid<u32> {
    let w = heightmap.width;
    let h = heightmap.height;
    let mut ids = Grid::filled_with(w, h, 0u32);

    let mut heap: BinaryHeap<OceanState> = BinaryHeap::new();
    for &(id, sx, sy) in seeds {
        if !is_ocean(heightmap, is_lake, sx, sy, sea_level) {
            continue;
        }
        ids.set(sx, sy, id);
        heap.push(OceanState { cost: 0, x: sx, y: sy, id });
    }

    // (dx, dy, step cost) — orthogonal steps are 10, diagonals ≈ 14 (√2).
    let dirs: [(isize, isize, u32); 8] = [
        (-1, -1, 14), (0, -1, 10), (1, -1, 14),
        (-1,  0, 10),               (1,  0, 10),
        (-1,  1, 14), (0,  1, 10), (1,  1, 14),
    ];

    while let Some(state) = heap.pop() {
        let cur_elev = *heightmap.get(state.x, state.y);
        for &(dx, dy, dist_cost) in &dirs {
            let nx = ids.wrap_x(state.x + dx) as isize;
            let ny = state.y + dy;
            if ny < 0 || ny >= h as isize {
                continue;
            }
            if *ids.get(nx, ny) != 0 {
                continue; // already claimed by a lower-cost seed
            }
            if !is_ocean(heightmap, is_lake, nx, ny, sea_level) {
                continue;
            }
            let target_elev = *heightmap.get(nx, ny);
            let bathymetry = (((target_elev - cur_elev).abs() * 260.0).min(28.0)) as u32;
            let new_cost = state.cost + dist_cost + bathymetry;
            ids.set(nx, ny, state.id);
            heap.push(OceanState { cost: new_cost, x: nx, y: ny, id: state.id });
        }
    }

    ids
}

/// Partition the world ocean into `config.num_provinces` balanced marine
/// provinces.
///
/// Mirrors the land province generator: a Poisson-disc style scatter of
/// jittered seeds followed by a two-pass relaxed flood fill (Lloyd relaxation
/// recentres every province so no seed drowns in an oversized cell).
pub fn generate_ocean_provinces(
    heightmap: &Grid<f32>,
    is_lake: &Grid<bool>,
    sea_level: f32,
    config: &OceanConfig,
    seed: u64,
) -> OceanResult {
    let w = heightmap.width;
    let h = heightmap.height;
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x0CEA_0CEA_0CEA_0CEA);

    // ── 1. Count the world ocean ─────────────────────────────────────────────
    let mut ocean_px = 0usize;
    for y in 0..h {
        for x in 0..w {
            if is_ocean(heightmap, is_lake, x as isize, y as isize, sea_level) {
                ocean_px += 1;
            }
        }
    }

    if ocean_px == 0 || config.num_provinces == 0 {
        println!("⚠ No open ocean found — skipping marine provinces.");
        return OceanResult {
            ocean_province_ids: Grid::filled_with(w, h, 0u32),
            num_provinces: 0,
        };
    }

    // ── 2. Poisson-disc style seed scatter ───────────────────────────────────
    let area_per_prov = ocean_px as f32 / config.num_provinces as f32;
    let stride = (area_per_prov.sqrt().max(2.0)).round() as usize;
    let min_dist = area_per_prov.sqrt() * 0.6;
    let min_d2 = min_dist * min_dist;

    let mut candidates: Vec<(isize, isize)> = Vec::new();
    for by in (0..h).step_by(stride) {
        for bx in (0..w).step_by(stride) {
            let jx = bx as isize + rng.random_range(0..stride) as isize;
            let jy = by as isize + rng.random_range(0..stride) as isize;
            if jy < 0 || jy >= h as isize {
                continue;
            }
            let jx = heightmap.wrap_x(jx) as isize;
            if is_ocean(heightmap, is_lake, jx, jy, sea_level) {
                candidates.push((jx, jy));
            }
        }
    }
    candidates.shuffle(&mut rng);

    let mut seeds: Vec<(u32, isize, isize)> = Vec::with_capacity(config.num_provinces);
    for (cx, cy) in candidates {
        if seeds.len() >= config.num_provinces {
            break;
        }
        let mut too_close = false;
        for &(_, ax, ay) in &seeds {
            let mut dx = (cx - ax).abs() as f32;
            if dx > w as f32 * 0.5 {
                dx = w as f32 - dx;
            }
            let dy = (cy - ay) as f32;
            if dx * dx + dy * dy < min_d2 {
                too_close = true;
                break;
            }
        }
        if !too_close {
            let id = seeds.len() as u32 + 1;
            seeds.push((id, cx, cy));
        }
    }

    if seeds.is_empty() {
        println!("⚠ No marine seed positions found — skipping marine provinces.");
        return OceanResult {
            ocean_province_ids: Grid::filled_with(w, h, 0u32),
            num_provinces: 0,
        };
    }

    let num_seeds = seeds.len();

    // ── 3. Two-pass relaxed flood fill (Dijkstra + Lloyd relaxation) ─────────
    let mut ids = flood_fill(&seeds, heightmap, is_lake, sea_level);

    let mut sum_cos = vec![0.0f64; num_seeds + 1];
    let mut sum_sin = vec![0.0f64; num_seeds + 1];
    let mut sum_y = vec![0.0f64; num_seeds + 1];
    let mut count = vec![0usize; num_seeds + 1];
    for y in 0..h {
        for x in 0..w {
            let id = ids.raw_slice()[y * w + x];
            if id == 0 {
                continue;
            }
            let angle = (x as f64 / w as f64) * std::f64::consts::TAU;
            sum_cos[id as usize] += angle.cos();
            sum_sin[id as usize] += angle.sin();
            sum_y[id as usize] += y as f64;
            count[id as usize] += 1;
        }
    }

    let mut relaxed: Vec<(u32, isize, isize)> = Vec::with_capacity(num_seeds);
    for &(id, sx, sy) in &seeds {
        let c = count[id as usize];
        if c == 0 {
            relaxed.push((id, sx, sy));
            continue;
        }
        // Circular mean in longitude so the antimeridian wraps cleanly.
        let avg_cos = sum_cos[id as usize] / c as f64;
        let avg_sin = sum_sin[id as usize] / c as f64;
        let angle = avg_sin.atan2(avg_cos);
        let cx = (((angle / std::f64::consts::TAU) + 0.5) * w as f64).round() as isize;
        let cx = heightmap.wrap_x(cx) as isize;
        let cy = (sum_y[id as usize] / c as f64).round() as isize;
        let (fx, fy) = snap_to_ocean(heightmap, is_lake, cx, cy, sea_level);
        relaxed.push((id, fx, fy));
    }

    ids = flood_fill(&relaxed, heightmap, is_lake, sea_level);

    let mut used = 0u32;
    for &id in ids.raw_slice() {
        if id > used {
            used = id;
        }
    }

    println!(
        "✓ Generated {} marine province(s) across {} ocean px (mean ≈ {} px each).",
        used,
        ocean_px,
        ocean_px / used.max(1) as usize
    );

    OceanResult {
        ocean_province_ids: ids,
        num_provinces: used as usize,
    }
}
