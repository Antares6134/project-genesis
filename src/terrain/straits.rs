//! Natural sea straits — the Gibraltar rule.
//!
//! The terrain can hold a body of water whose floor lies below the water line
//! yet which is walled off from the world ocean. By elevation alone every pixel
//! of it is sea: hydrology's landlocked-water pass then reclaims it as a lake,
//! and the atlas paints it fresh water and gives it no marine provinces. That is
//! the correct treatment of an inland basin — until the basin is no longer a
//! lake at all. A six-hundred-thousand-pixel basin floored two kilometres below
//! the datum, ringed by a two-hundred-metre sill, is not a lake with an
//! unfortunate outlet: it is a sea that never had its Gibraltar.
//!
//! This pass gives it one. It runs at the very end of act one, on the finished
//! terrain, and it is deliberately blind to the later acts: a below-datum body
//! is a sea candidate by its own geometry alone, so the strait can be carved
//! before the climate is read. That ordering matters. Act two's continentality
//! then sees the new sea as sea, act three's priority flood finds it already
//! joined to the world ocean and never calls it a lake, and acts four onward
//! give it marine provinces — with no second hydrology pass and no cache format
//! to migrate.
//!
//! The rule is the one a real marginal sea obeys:
//!
//! * Every connected body of water at or below the datum is found, wrapping in
//!   longitude exactly as the hydrology does. The largest is the world ocean.
//! * Any other body covering at least [`WorldConfig::sea_min_area_share`] of the
//!   canvas is a **sea**, not a lake.
//! * A sea is joined to the world ocean by a strait cut through the lowest
//!   saddle between them, at most [`WorldConfig::strait_max_sill`] above the
//!   water line, no longer than [`WorldConfig::strait_max_span`] pixels, and
//!   never more than [`WorldConfig::sea_max_straits`] seas in one world.
//!
//! The route is a bounded least-cost walk rather than a straight line: entering
//! a pixel costs its distance (10 orthogonal, 14 diagonal, as in the marine
//! flood fill) plus its elevation above the datum scaled by
//! [`WorldConfig::strait_slope_cost`]. A strait therefore bends around a ridge
//! and prefers a low gap to a short mountain — Gibraltar, not a canal. The
//! channel is then cut as a notch a few pixels wide, its floor below the datum
//! and its shoulders at half the centre depth, so the atlas reads an open
//! channel rather than a trench with vertical walls.
//!
//! Two properties are preserved on purpose:
//!
//! * **Nothing is ever raised.** A carve only lowers terrain (`min` with the
//!   target), so an already-deeper basin keeps its floor.
//! * **Nothing is carved on trust.** After cutting, the candidate is flood-filled
//!   over the new water surface; if it does not reach the world ocean the carve
//!   is rolled back exactly, and the body stays a lake. A failed strait can
//!   therefore never leak a sea into a landmass.
//!
//! The pass leaves the deep, above-datum lakes alone. A lake whose surface is
//! hundreds of metres above the sea cannot become a sea by opening a sea-level
//! channel — it would simply drain, leaving a dry basin — so only bodies that
//! already sit at or below the datum are candidates.

use crate::core::Grid;
use crate::core::progress::Progress;
use crate::export::live;
use crate::terrain::config::WorldConfig;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Safety ceiling on the settled pixels of one strait search.
///
/// The step cap already bounds a search to a disc of `strait_max_span` pixels;
/// this is the second belt, for a pathological world whose terrain is so flat
/// that a candidate's whole neighbourhood is traversable. A search that cannot
/// find the ocean inside this many expansions is abandoned, and the body stays
/// a lake.
const MAX_STRAIT_EXPANSIONS: usize = 4_000_000;

/// Depth of a strait's shoulders as a fraction of its centre depth.
///
/// The notch falls from the centre to half depth at its rim, so the channel has
/// sloping banks rather than two vertical walls.
const STRAIT_SHOULDER_RATIO: f32 = 0.5;

/// The eight neighbours, with the step cost the marine flood fill uses:
/// 10 for an orthogonal step and 14 for a diagonal one (√2 × 10, rounded).
const DIRECTIONS: [(isize, isize, u32); 8] = [
    (-1, 0, 10),
    (1, 0, 10),
    (0, -1, 10),
    (0, 1, 10),
    (-1, -1, 14),
    (1, -1, 14),
    (-1, 1, 14),
    (1, 1, 14),
];

/// Marks a search seed, and the absence of a parent on a reconstructed path.
const NO_PARENT: u8 = u8::MAX;

/// One sea opened to the world ocean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeaStrait {
    /// How many water pixels the landlocked basin held before the strait.
    pub sea_cells: usize,
    /// How many land pixels the channel crosses on its way to the ocean.
    pub channel_cells: usize,
    /// Highest natural elevation on the route, in heightmap units. The sill the
    /// channel was cut through.
    pub sill: f32,
}

/// What one run of the strait pass did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StraitReport {
    /// Every sea that was successfully joined to the world ocean.
    pub straits: Vec<SeaStrait>,
    /// How many landlocked bodies were large enough to be seas, before the
    /// `sea_max_straits` cap. Zero when the world has no landlocked sea.
    pub candidates: usize,
}

/// A connected body of water at or below the datum.
#[derive(Clone, Copy, Debug)]
struct WaterBody {
    /// The id written into `body_of` for every pixel of this body.
    id: u32,
    /// Water pixels in the body.
    cells: usize,
    /// The first pixel of the body in raster order — a deterministic anchor for
    /// sorting and for the connectivity proof.
    first: u32,
    /// Inclusive bounding box, in canvas coordinates (`x0, y0, x1, y1`).
    bbox: (usize, usize, usize, usize),
}

/// One node of the least-cost route, ordered as a min-heap by cost, then by
/// fewer steps, then by raster index so that ties are resolved identically on
/// every run.
#[derive(Copy, Clone, Eq, PartialEq)]
struct RouteNode {
    cost: u32,
    steps: u16,
    index: u32,
}

impl Ord for RouteNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.steps.cmp(&self.steps))
            .then_with(|| other.index.cmp(&self.index))
    }
}

impl PartialOrd for RouteNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Wrap a longitude into the cylinder.
#[inline(always)]
fn wrap_x(x: isize, w: usize) -> usize {
    x.rem_euclid(w as isize) as usize
}
/// Open every landlocked sea large enough to warrant one to the world ocean.
///
/// `heightmap` is act one's finished surface, in normalized heightmap units;
/// the pass lowers a channel through the lowest saddle of each qualifying body
/// and nothing else. Returns what it did, for the act's summary line.
///
/// The pass is deterministic: bodies are discovered in raster order, candidates
/// are ordered by size then by first pixel, and the route heap breaks every tie
/// by distance and raster index. The same seed always grows the same straits.
pub fn form_sea_straits(
    heightmap: &mut Grid<f32>,
    sea_level: f32,
    config: &WorldConfig,
    progress: &Progress,
) -> StraitReport {
    let w = heightmap.width;
    let h = heightmap.height;
    if w == 0 || h == 0 {
        return StraitReport::default();
    }

    let (body_of, bodies) = find_water_bodies(heightmap, sea_level);
    if bodies.len() < 2 {
        return StraitReport::default();
    }

    // The world ocean is the largest single body, exactly as hydrology's
    // landlocked-water pass decides it. On a tie the first body in raster order
    // wins, so the choice is deterministic.
    let mut ocean = bodies[0];
    for &body in &bodies[1..] {
        if body.cells > ocean.cells {
            ocean = body;
        }
    }

    let min_sea_cells = (config.sea_min_area_share.max(0.0) * (w * h) as f32).ceil() as usize;
    let mut candidates: Vec<WaterBody> = bodies
        .iter()
        .copied()
        .filter(|body| body.id != ocean.id && body.cells >= min_sea_cells)
        .collect();
    candidates.sort_by(|a, b| b.cells.cmp(&a.cells).then(a.first.cmp(&b.first)));
    let qualifying = candidates.len();
    candidates.truncate(config.sea_max_straits);
    if candidates.is_empty() {
        return StraitReport {
            straits: Vec::new(),
            candidates: qualifying,
        };
    }

    progress.sub_step("Opening sea straits", candidates.len() as u64);

    let total = w * h;
    let mut cost = vec![u32::MAX; total];
    let mut steps = vec![0u16; total];
    let mut parent = vec![NO_PARENT; total];
    let mut visited = vec![false; total];
    let mut stack: Vec<u32> = Vec::new();

    let mut report = StraitReport {
        straits: Vec::new(),
        candidates: qualifying,
    };

    for (done, candidate) in candidates.iter().enumerate() {
        // ── 1. Find the lowest route from this basin to the world ocean ──────
        let route = {
            let search = StraitSearch {
                w,
                h,
                sea_level,
                sill_limit: sea_level + config.strait_max_sill.max(0.0),
                slope_cost: config.strait_slope_cost.max(0.0),
                max_span: config.strait_max_span.clamp(1, u16::MAX as usize),
                elev: heightmap.raw_slice(),
                body_of: &body_of,
            };
            search.find_route(candidate, ocean.id, &mut cost, &mut steps, &mut parent)
        };

        let Some((path, sill)) = route else {
            progress.tick(done as u64 + 1);
            continue;
        };

        // ── 2. Cut the notch ─────────────────────────────────────────────────
        let carved = carve_channel(heightmap, &path, sea_level, config);

        // ── 3. Prove the sea now reaches the world ocean ─────────────────────
        let reaches = reaches_ocean(
            candidate.first as usize,
            w,
            h,
            sea_level,
            &body_of,
            ocean.id,
            heightmap.raw_slice(),
            &mut visited,
            &mut stack,
        );

        if reaches {
            report.straits.push(SeaStrait {
                sea_cells: candidate.cells,
                channel_cells: path.len(),
                sill,
            });
            progress.tick(done as u64 + 1);
            let snapshot: &Grid<f32> = heightmap;
            progress.preview_map("Sea straits", (w, h), |x, y| {
                live::relief_at(snapshot, x, y)
            });
            drop(carved);
        } else {
            // Roll the terrain back exactly. `carved` records each pixel's
            // previous height in the order it was lowered, so unwinding it
            // backwards restores the original surface even where two swathes
            // overlapped.
            let data = heightmap.raw_slice_mut();
            let mut carved = carved;
            while let Some((idx, before)) = carved.pop() {
                data[idx as usize] = before;
            }
            progress.tick(done as u64 + 1);
        }
    }

    report
}

/// The immutable terrain and limits one strait search reads.
struct StraitSearch<'a> {
    w: usize,
    h: usize,
    sea_level: f32,
    /// Highest land the route may cross, in heightmap units. Pixels above it
    /// are walls, not passes.
    sill_limit: f32,
    /// Cost of one heightmap unit of uplift, in route-cost units.
    slope_cost: f32,
    /// Longest route, in pixels, the carver will consider.
    max_span: usize,
    elev: &'a [f32],
    body_of: &'a [u32],
}

impl StraitSearch<'_> {
    /// Find the least-cost land route from the candidate's shore to a pixel
    /// touching the world ocean, or `None` if there is none inside the limits.
    ///
    /// Multi-source Dijkstra: every shore pixel is a seed at cost zero, and the
    /// first popped pixel that touches the ocean ends the search. Because edge
    /// costs are non-negative and the heap is deterministic, the route is the
    /// cheapest crossing — lowest saddle first, shortest run second.
    fn find_route(
        &self,
        candidate: &WaterBody,
        ocean_id: u32,
        cost: &mut [u32],
        steps: &mut [u16],
        parent: &mut [u8],
    ) -> Option<(Vec<u32>, f32)> {
        cost.fill(u32::MAX);
        let mut heap = BinaryHeap::new();

        // ── Seed every land pixel on the basin's shore ───────────────────────
        let (x0, y0, x1, y1) = candidate.bbox;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let idx = y * self.w + x;
                if self.body_of[idx] != candidate.id {
                    continue;
                }
                for &(dx, dy, _) in &DIRECTIONS {
                    let ny = y as isize + dy;
                    if ny < 0 || ny >= self.h as isize {
                        continue;
                    }
                    let nx = wrap_x(x as isize + dx, self.w);
                    let nidx = ny as usize * self.w + nx;
                    if !self.traversable(nidx) || cost[nidx] == 0 {
                        continue;
                    }
                    cost[nidx] = 0;
                    steps[nidx] = 0;
                    parent[nidx] = NO_PARENT;
                    heap.push(RouteNode {
                        cost: 0,
                        steps: 0,
                        index: nidx as u32,
                    });
                }
            }
        }

        // ── Settle the cheapest pixels until one touches the ocean ───────────
        let mut settled = 0usize;
        let mut goal = None;
        while let Some(node) = heap.pop() {
            let cur = node.index as usize;
            if node.cost > cost[cur] {
                continue; // a cheaper route to this pixel was already found
            }
            if self.touches_ocean(cur, ocean_id) {
                goal = Some(cur);
                break;
            }
            settled += 1;
            if settled > MAX_STRAIT_EXPANSIONS {
                break;
            }

            let cur_steps = steps[cur] as usize;
            if cur_steps >= self.max_span {
                continue; // the route would be longer than a strait may be
            }
            let cy = cur / self.w;
            let cx = cur % self.w;
            for (dir, &(dx, dy, step)) in DIRECTIONS.iter().enumerate() {
                let ny = cy as isize + dy;
                if ny < 0 || ny >= self.h as isize {
                    continue;
                }
                let nx = wrap_x(cx as isize + dx, self.w);
                let nidx = ny as usize * self.w + nx;
                if !self.traversable(nidx) {
                    continue;
                }
                let uplift = (self.elev[nidx] - self.sea_level).max(0.0);
                let edge = step.saturating_add((uplift * self.slope_cost) as u32);
                let new_cost = node.cost.saturating_add(edge);
                if new_cost < cost[nidx] {
                    cost[nidx] = new_cost;
                    steps[nidx] = (cur_steps + 1) as u16;
                    parent[nidx] = dir as u8;
                    heap.push(RouteNode {
                        cost: new_cost,
                        steps: steps[nidx],
                        index: nidx as u32,
                    });
                }
            }
        }

        // ── Walk the parents back to the shore ───────────────────────────────
        let goal = goal?;
        let mut path = Vec::new();
        let mut cur = goal;
        loop {
            path.push(cur as u32);
            let dir = parent[cur];
            if dir == NO_PARENT {
                break;
            }
            let (dx, dy, _) = DIRECTIONS[dir as usize];
            let py = (cur / self.w) as isize - dy;
            if py < 0 || py >= self.h as isize {
                break;
            }
            let px = wrap_x((cur % self.w) as isize - dx, self.w);
            cur = py as usize * self.w + px;
        }
        path.reverse();

        let sill = path
            .iter()
            .map(|&idx| self.elev[idx as usize])
            .fold(self.sea_level, f32::max);
        Some((path, sill))
    }

    /// Land within the sill limit may carry a channel; water and high ground may
    /// not.
    #[inline(always)]
    fn traversable(&self, idx: usize) -> bool {
        let elev = self.elev[idx];
        elev > self.sea_level && elev <= self.sill_limit
    }

    /// True when a land pixel has a water neighbour belonging to the world
    /// ocean.
    fn touches_ocean(&self, idx: usize, ocean_id: u32) -> bool {
        let cy = idx / self.w;
        let cx = idx % self.w;
        for &(dx, dy, _) in &DIRECTIONS {
            let ny = cy as isize + dy;
            if ny < 0 || ny >= self.h as isize {
                continue;
            }
            let nx = wrap_x(cx as isize + dx, self.w);
            if self.body_of[ny as usize * self.w + nx] == ocean_id {
                return true;
            }
        }
        false
    }
}

/// Flood the original heightmap into connected bodies of water at or below the
/// datum, wrapping in longitude.
///
/// `body_of` is `0` on land; every water pixel carries the id of the body it
/// belongs to. Bodies are numbered in raster order, so the scan is
/// deterministic. This is the same eight-connected, x-wrapped neighbourhood that
/// hydrology's landlocked-water reclaim uses, which is what makes "the largest
/// body is the ocean" mean exactly the same thing here as it does in act three.
fn find_water_bodies(heightmap: &Grid<f32>, sea_level: f32) -> (Vec<u32>, Vec<WaterBody>) {
    let w = heightmap.width;
    let h = heightmap.height;
    let elev = heightmap.raw_slice();
    let mut body_of = vec![0u32; w * h];
    let mut bodies: Vec<WaterBody> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    let mut next_id = 0u32;

    for y in 0..h {
        for x in 0..w {
            let start = y * w + x;
            if body_of[start] != 0 || elev[start] > sea_level {
                continue;
            }

            next_id += 1;
            let id = next_id;
            let mut cells = 0usize;
            let (mut x0, mut y0, mut x1, mut y1) = (x, y, x, y);
            body_of[start] = id;
            stack.clear();
            stack.push(start as u32);
            while let Some(cur) = stack.pop() {
                let cy = cur as usize / w;
                let cx = cur as usize % w;
                cells += 1;
                x0 = x0.min(cx);
                y0 = y0.min(cy);
                x1 = x1.max(cx);
                y1 = y1.max(cy);

                for &(dx, dy, _) in &DIRECTIONS {
                    let ny = cy as isize + dy;
                    if ny < 0 || ny >= h as isize {
                        continue;
                    }
                    let nx = wrap_x(cx as isize + dx, w);
                    let nidx = ny as usize * w + nx;
                    if body_of[nidx] != 0 || elev[nidx] > sea_level {
                        continue;
                    }
                    body_of[nidx] = id;
                    stack.push(nidx as u32);
                }
            }

            bodies.push(WaterBody {
                id,
                cells,
                first: start as u32,
                bbox: (x0, y0, x1, y1),
            });
        }
    }

    (body_of, bodies)
}

/// Lower a notch along the route and return every pixel it touched, paired with
/// the height it held before the cut.
///
/// The return value is the rollback log: unwinding it in reverse restores the
/// surface exactly, however many times a swathe overlapped itself. The notch is
/// cut with `min`, so a cell that was already deeper than the target keeps its
/// floor; the sea is never made shallower.
fn carve_channel(
    heightmap: &mut Grid<f32>,
    path: &[u32],
    sea_level: f32,
    config: &WorldConfig,
) -> Vec<(u32, f32)> {
    let w = heightmap.width;
    let h = heightmap.height;
    let radius = (config.strait_width.max(1) / 2) as isize;
    let depth = config.strait_depth.max(1.0e-4);
    let span = radius as f32 + 0.5;
    let mut carved: Vec<(u32, f32)> = Vec::new();

    let data = heightmap.raw_slice_mut();
    for &idx in path {
        let cy = idx as usize / w;
        let cx = idx as usize % w;
        for dy in -radius..=radius {
            let ny = cy as isize + dy;
            if ny < 0 || ny >= h as isize {
                continue;
            }
            for dx in -radius..=radius {
                let nx = wrap_x(cx as isize + dx, w);
                let nidx = ny as usize * w + nx;
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                let t = (dist / span).min(1.0);
                let target = sea_level - depth * (1.0 - STRAIT_SHOULDER_RATIO * t);
                if data[nidx] > target {
                    carved.push((nidx as u32, data[nidx]));
                    data[nidx] = target;
                }
            }
        }
    }
    carved
}

/// Prove that `start` now reaches the world ocean across water alone.
///
/// The carve is only trusted once this returns true: a channel that misses the
/// sea, or one whose notch did not break the surface, must not turn a lake into
/// a leaking basin. The flood is eight-connected and wraps in longitude, like
/// every other water pass in the engine.
#[allow(clippy::too_many_arguments)]
fn reaches_ocean(
    start: usize,
    w: usize,
    h: usize,
    sea_level: f32,
    body_of: &[u32],
    ocean_id: u32,
    elev: &[f32],
    visited: &mut [bool],
    stack: &mut Vec<u32>,
) -> bool {
    visited.fill(false);
    visited[start] = true;
    stack.clear();
    stack.push(start as u32);

    while let Some(cur) = stack.pop() {
        let idx = cur as usize;
        if body_of[idx] == ocean_id {
            return true;
        }
        let cy = idx / w;
        let cx = idx % w;
        for &(dx, dy, _) in &DIRECTIONS {
            let ny = cy as isize + dy;
            if ny < 0 || ny >= h as isize {
                continue;
            }
            let nx = wrap_x(cx as isize + dx, w);
            let nidx = ny as usize * w + nx;
            if visited[nidx] || elev[nidx] > sea_level {
                continue;
            }
            visited[nidx] = true;
            stack.push(nidx as u32);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test configuration: any body counts, a five-pixel channel, a generous
    /// sill and span so the tiny test canvases behave like miniature worlds.
    fn config() -> WorldConfig {
        WorldConfig {
            sea_min_area_share: 0.0001,
            sea_max_straits: 3,
            strait_width: 5,
            strait_depth: 0.008,
            strait_max_sill: 0.03,
            strait_max_span: 64,
            strait_slope_cost: 2_000.0,
            ..Default::default()
        }
    }

    /// A 60×21 world: a twenty-column ocean in the west, a high plain, and a
    /// 15×13 basin a little below the datum (195 px), walled off from the sea by
    /// ten columns of land. The ocean is kept away from the antimeridian so the
    /// intended western crossing is the shortest one; the wrap route around the
    /// east is deliberately longer.
    fn sea_world() -> Grid<f32> {
        let mut map = Grid::filled_with(60, 21, 0.42f32);
        for y in 0..21isize {
            for x in 10..=29isize {
                map.set(x, y, 0.30);
            }
        }
        for y in 4..=16isize {
            for x in 40..=54isize {
                map.set(x, y, 0.35);
            }
        }
        map
    }

    /// True when two cells are joined across water at or below `sea_level`.
    fn connected(
        heightmap: &Grid<f32>,
        sea_level: f32,
        from: (isize, isize),
        to: (isize, isize),
    ) -> bool {
        let (w, h) = (heightmap.width, heightmap.height);
        let mut seen = vec![false; w * h];
        let start = from.1 as usize * w + heightmap.wrap_x(from.0);
        seen[start] = true;
        let mut stack = vec![from];
        while let Some((x, y)) = stack.pop() {
            if (x, y) == to {
                return true;
            }
            for &(dx, dy, _) in &DIRECTIONS {
                let ny = y as isize + dy;
                if ny < 0 || ny >= h as isize {
                    continue;
                }
                let nx = heightmap.wrap_x(x + dx) as isize;
                let nidx = ny as usize * w + nx as usize;
                if seen[nidx] || *heightmap.get(nx, ny) > sea_level {
                    continue;
                }
                seen[nidx] = true;
                stack.push((nx, ny));
            }
        }
        false
    }

    /// The central case: a sea-sized landlocked basin is joined to the world
    /// ocean by a canal cut only through its low wall, and every cut pixel is
    /// water. The basin's own floor is never touched, because a carve only
    /// lowers and the floor is already deeper than the notch.
    #[test]
    fn a_landlocked_sea_is_opened_to_the_world_ocean() {
        let mut map = sea_world();
        let before = map.clone();
        let report = form_sea_straits(&mut map, 0.40, &config(), &Progress::silent());

        assert_eq!(report.candidates, 1);
        assert_eq!(report.straits.len(), 1, "the one sea-sized basin is opened");
        assert_eq!(report.straits[0].sea_cells, 15 * 13);
        assert!(
            report.straits[0].channel_cells >= 10,
            "the channel must cross the ten-column wall"
        );
        assert!(
            connected(&map, 0.40, (47, 10), (20, 10)),
            "the new sea must reach the world ocean"
        );

        let mut lowered = 0;
        for y in 0..21isize {
            for x in 0..60isize {
                let after = *map.get(x, y);
                let old = *before.get(x, y);
                if after != old {
                    lowered += 1;
                    assert!(after <= 0.40, "carved pixel ({x},{y}) must be water");
                    assert!(after < old, "a carve may only lower terrain");
                }
            }
        }
        assert!(lowered > 0);
        assert_eq!(
            *map.get(47, 10),
            *before.get(47, 10),
            "the basin's own floor is deeper than the notch and is left alone"
        );
    }

    /// A below-datum body too small to be a sea is left exactly as it was: the
    /// threshold is what separates a lake from a marginal sea.
    #[test]
    fn a_landlocked_lake_below_sea_size_stays_a_lake() {
        let mut map = sea_world();
        let before = map.clone();
        let mut cfg = config();
        cfg.sea_min_area_share = 0.2; // 252 px, above the 195-pixel basin
        let report = form_sea_straits(&mut map, 0.40, &cfg, &Progress::silent());

        assert_eq!(report.candidates, 0);
        assert!(report.straits.is_empty());
        assert_eq!(map.raw_slice(), before.raw_slice());
    }

    /// A sea ringed by real mountains has no natural portal. The pass must not
    /// tunnel through them: the sill limit is a wall, and the world is untouched.
    #[test]
    fn a_sea_walled_in_by_real_mountains_stays_a_lake() {
        let mut map = sea_world();
        // Every shore is now high ground, all the way around the cylinder.
        for y in 0..21isize {
            for x in 0..60isize {
                if *map.get(x, y) > 0.40 {
                    map.set(x, y, 0.50);
                }
            }
        }
        let before = map.clone();
        let report = form_sea_straits(&mut map, 0.40, &config(), &Progress::silent());

        assert_eq!(report.straits.len(), 0);
        assert_eq!(map.raw_slice(), before.raw_slice());
    }

    /// The route bends to a low pass and leaves the mountain beside it intact.
    #[test]
    fn the_strait_finds_the_low_pass_and_leaves_the_mountain_alone() {
        let (w, h) = (30usize, 21usize);
        let mut map = Grid::filled_with(w, h, 0.42f32);
        for y in 0..21isize {
            for x in 0..=5isize {
                map.set(x, y, 0.30);
            }
        }
        for y in 6..=14isize {
            for x in 20..=27isize {
                map.set(x, y, 0.35);
            }
        }
        // A thick mountain wall across the whole world, with a single low pass
        // at its northern end; the eastern side is walled too, so the only way
        // around the cylinder is through the pass.
        for y in 0..21isize {
            for x in 10..=17isize {
                map.set(x, y, 0.55);
            }
            for x in 28..=29isize {
                map.set(x, y, 0.55);
            }
        }
        for y in 2..=4isize {
            for x in 10..=17isize {
                map.set(x, y, 0.402);
            }
        }

        let report = form_sea_straits(&mut map, 0.40, &config(), &Progress::silent());
        assert_eq!(report.straits.len(), 1);
        assert_eq!(report.straits[0].sea_cells, 8 * 9);
        assert!(*map.get(14, 3) <= 0.40, "the low pass must be cut");
        for y in 7..=16isize {
            assert_eq!(
                *map.get(14, y),
                0.55,
                "the mountain interior at (14,{y}) must survive"
            );
        }
    }

    /// A body split across the antimeridian is one sea, not two lakes, and its
    /// strait is cut across the shortest shore like any other.
    #[test]
    fn a_sea_split_across_the_antimeridian_is_one_body() {
        let (w, h) = (40usize, 15usize);
        let mut map = Grid::filled_with(w, h, 0.42f32);
        for y in 0..15isize {
            for x in 5..=12isize {
                map.set(x, y, 0.30);
            }
        }
        for y in 4..=10isize {
            for x in [0isize, 1, 38, 39] {
                map.set(x, y, 0.35);
            }
        }

        let report = form_sea_straits(&mut map, 0.40, &config(), &Progress::silent());
        assert_eq!(report.straits.len(), 1);
        assert_eq!(
            report.straits[0].sea_cells,
            4 * 7,
            "the wrapped halves are one body"
        );
        assert!(connected(&map, 0.40, (39, 7), (8, 7)));
    }

    /// The same terrain always grows the same strait.
    #[test]
    fn the_same_terrain_grows_the_same_straits() {
        let mut first = sea_world();
        let mut second = first.clone();
        let a = form_sea_straits(&mut first, 0.40, &config(), &Progress::silent());
        let b = form_sea_straits(&mut second, 0.40, &config(), &Progress::silent());

        assert_eq!(first.raw_slice(), second.raw_slice());
        assert_eq!(a, b);
    }

    /// The cap is honoured: a qualifying body is reported, but not opened.
    #[test]
    fn no_more_seas_are_opened_than_the_cap_allows() {
        let mut map = sea_world();
        let before = map.clone();
        let mut cfg = config();
        cfg.sea_max_straits = 0;
        let report = form_sea_straits(&mut map, 0.40, &cfg, &Progress::silent());

        assert_eq!(report.candidates, 1, "the basin still qualifies");
        assert!(report.straits.is_empty());
        assert_eq!(map.raw_slice(), before.raw_slice());
    }

    /// The hand-off to act three: the basin is a lake before the strait and the
    /// ocean after it. This is the whole point of the pass.
    #[test]
    fn the_opened_sea_reads_as_ocean_to_the_hydrology() {
        use crate::hydrology::{HydrologyConfig, fill_depressions_and_find_lakes};

        let mut map = sea_world();
        let progress = Progress::silent();
        let hydro = HydrologyConfig::default();

        let closed = fill_depressions_and_find_lakes(&map, 0.40, &hydro, &progress);
        assert!(
            *closed.is_lake.get(47, 10),
            "a walled-off basin starts as a lake"
        );

        let report = form_sea_straits(&mut map, 0.40, &config(), &progress);
        assert_eq!(report.straits.len(), 1);

        let opened = fill_depressions_and_find_lakes(&map, 0.40, &hydro, &progress);
        assert!(
            !*opened.is_lake.get(47, 10),
            "the opened sea is ocean to the hydrology now"
        );
    }
}
