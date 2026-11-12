use crate::core::progress::Progress;
use crate::core::Grid;
use crate::export::live;
use crate::hydrology::config::HydrologyConfig;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// How many cells the flood may drain between two progress checkpoints.
const CELLS_PER_CHECKPOINT: usize = 200_000;

/// Flat-resolution slope: the height, in heightmap units, that the *routing*
/// surface is lifted by per step away from an outlet.
///
/// Priority flooding leaves every filled depression as a dead-flat plateau, and
/// a strict D8 router cannot cross a flat — it needs a strictly lower neighbour,
/// and every neighbour of a plateau cell is the very same height. Left alone
/// this strands roughly a tenth of the world's land and shreds the drainage
/// network into disconnected fragments: measured on an 8K world it collapsed the
/// largest connected river to ~1 500 px and the peak discharge to ~3 800, so
/// almost nothing crossed the map's river threshold.
///
/// Barnes' *Priority-Flood + ε* fixes it. While flooding, each newly reached
/// cell is lifted to `max(land, parent + EPSILON)`, so the routing surface has
/// no flats at all and every cell owns a strictly-descending path to the sea.
/// The lift is invisible — one part in ten million of the vertical datum, a few
/// centimetres across an entire continent — and it is applied to a *separate*
/// grid, so lake detection still reads the honest, un-lifted fill.
const EPSILON: f32 = 1.0e-6;

#[derive(Copy, Clone, PartialEq)]
struct QueueItem {
    /// Physical filled height — the heap key, so the flood still finds the true
    /// spill level of each depression.
    height: f32,
    /// The ε-lifted routing height, carried alongside the physical one so the
    /// routing surface can be built in the same single flood.
    routing: f32,
    x: isize,
    y: isize,
}

impl Eq for QueueItem {}

// Reverse ordering to make BinaryHeap act as a Min-Heap
impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .height
            .partial_cmp(&self.height)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct PitFillResult {
    /// The honest, un-lifted fill. This is what lake depth is read from, so a
    /// lake keeps exactly the shoreline it always had.
    pub filled_heightmap: Grid<f32>,
    /// The ε-lifted twin, flat-free everywhere: the surface D8 routes over.
    pub routing_heightmap: Grid<f32>,
    pub is_lake: Grid<bool>,
    pub lake_depth: Grid<f32>,
}

/// Simulates water filling land depressions using the Barnes Priority-Flood
/// algorithm. Guarantees that all terrain slopes downhill to an ocean pour
/// point — in the routing surface by construction (`+ ε`), and physically in the
/// fill.
pub fn fill_depressions_and_find_lakes(
    heightmap: &Grid<f32>,
    sea_level: f32,
    config: &HydrologyConfig,
    progress: &Progress,
) -> PitFillResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut filled = heightmap.clone();
    let mut routing = heightmap.clone();
    let mut visited = Grid::filled_with(w, h, false);
    let mut is_lake = Grid::filled_with(w, h, false);
    let mut lake_depth = Grid::filled_with(w, h, 0.0f32);
    let mut heap = BinaryHeap::new();

    // 1. Initialize heap with all ocean cells and polar boundaries
    for y in 0..h {
        for x in 0..w {
            let elev = *heightmap.get(x as isize, y as isize);
            if elev <= sea_level || y == 0 || y == h - 1 {
                visited.set(x as isize, y as isize, true);
                heap.push(QueueItem {
                    height: elev,
                    routing: elev,
                    x: x as isize,
                    y: y as isize,
                });
            }
        }
    }

    let neighbors: [(isize, isize); 8] = [
        (-1, 0), (1, 0), (0, -1), (0, 1),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];

    // 2. Priority flood across terrain
    //    The flood's own size is not known in advance — it pops until the heap
    //    is empty — so this sub-step reports a count and an indeterminate bar,
    //    and shows the lakes filling as the water finds them.
    progress.sub_step("Filling depressions", 0);
    let mut drained = 0usize;
    while let Some(current) = heap.pop() {
        drained += 1;
        if drained % CELLS_PER_CHECKPOINT == 0 {
            progress.tick(drained as u64);
            progress.preview_map("Lakes filling", (w, h), |x, y| {
                live::water_at(&filled, Some(&is_lake), None, 0.0, x, y)
            });
        }

        for &(dx, dy) in &neighbors {
            let ny = current.y + dy;
            if ny < 0 || ny >= h as isize {
                continue;
            }
            let nx = heightmap.wrap_x(current.x + dx) as isize;

            if !*visited.get(nx, ny) {
                visited.set(nx, ny, true);

                let original_h = *heightmap.get(nx, ny);
                let filled_h = original_h.max(current.height);
                filled.set(nx, ny, filled_h);

                // The flat-free twin. Because `routing` is always strictly above
                // its parent, D8 can never stall: every cell has at least the
                // cell the flood reached it from as a strictly-lower neighbour.
                let routing_h = original_h.max(current.routing + EPSILON);
                routing.set(nx, ny, routing_h);

                // Detect inland lake (ONLY deep, major geological depressions become lakes!)
                let depth = filled_h - original_h;
                let ny_norm = ny as f32 / h as f32;
                let is_polar = ny_norm < 0.12 || ny_norm > 0.88;

                if depth > config.min_lake_depth && original_h > sea_level && !is_polar {
                    is_lake.set(nx, ny, true);
                    lake_depth.set(nx, ny, depth);
                }

                heap.push(QueueItem {
                    height: filled_h,
                    routing: routing_h,
                    x: nx,
                    y: ny,
                });
            }
        }
    }

    // Erase the specks: a single-pixel pit is heightmap noise, not a lake, and no
    // atlas should ink it. Only bodies at least `min_lake_cells` wide survive.
    erase_small_lakes(&mut is_lake, &mut lake_depth, config.min_lake_cells, w, h);

    // The priority flood seeded every below-datum cell as an ocean outlet, so a
    // basin walled off from the world ocean was never filled and still reads as
    // sea. Reclassify those landlocked bodies as inland lakes now, before the
    // result is handed to every later pass that asks "is this water the sea?".
    progress.sub_step("Reclaiming landlocked water", 0);
    reclaim_landlocked_water(&heightmap, &mut is_lake, &mut lake_depth, sea_level, w, h);

    PitFillResult {
        filled_heightmap: filled,
        routing_heightmap: routing,
        is_lake,
        lake_depth,
    }
}

/// Reclassify water trapped below the datum as inland lakes.
///
/// The terrain can hold a body of water whose floor lies below sea level yet
/// which is walled off from the world ocean — a landlocked basin. By elevation
/// alone every cell of it reads as sea, so the land provinces, the marine
/// hierarchy and the atlas would all count it as ocean. But it has no channel to
/// the global ocean network: it is a lake.
///
/// The priority flood seeds every below-datum cell as an ocean outlet, so such a
/// basin is never filled and keeps its below-sea-level water line. This pass
/// therefore works on the *original* heightmap: it flood-fills every connected
/// body of water at or below the datum and takes the largest — the single world
/// ocean — as the sea, marking every other body as an inland lake. The engine
/// builds one connected world ocean (`geography::oceans`), so a body the main one
/// cannot reach is, by construction, landlocked. Neighbourhood and longitude
/// wrapping match the marine flood fill, so "connected" means exactly what it
/// means there.
fn reclaim_landlocked_water(
    heightmap: &Grid<f32>,
    is_lake: &mut Grid<bool>,
    lake_depth: &mut Grid<f32>,
    sea_level: f32,
    w: usize,
    h: usize,
) {
    if w == 0 || h == 0 {
        return;
    }

    let mut visited = vec![false; w * h];
    let mut stack: Vec<(isize, isize)> = Vec::new();

    // 1. Find the largest connected body of water: the world ocean.
    let mut ocean: Option<(isize, isize)> = None;
    let mut ocean_size = 0usize;
    for y0 in 0..h as isize {
        for x0 in 0..w as isize {
            let idx = y0 as usize * w + x0 as usize;
            if visited[idx] || *heightmap.get(x0, y0) > sea_level {
                continue;
            }
            let mut size = 0usize;
            stack.clear();
            stack.push((x0, y0));
            visited[idx] = true;
            while let Some((x, y)) = stack.pop() {
                size += 1;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let ny = y + dy;
                        if ny < 0 || ny >= h as isize {
                            continue;
                        }
                        let nx = heightmap.wrap_x(x + dx) as isize;
                        let nidx = ny as usize * w + nx as usize;
                        if visited[nidx] || *heightmap.get(nx, ny) > sea_level {
                            continue;
                        }
                        visited[nidx] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            if size > ocean_size {
                ocean_size = size;
                ocean = Some((x0, y0));
            }
        }
    }
    let Some((ox, oy)) = ocean else {
        return;
    };

    // 2. Mark the world ocean's body again, so every water cell left unvisited
    //    belongs to a landlocked body.
    visited.fill(false);
    stack.clear();
    stack.push((ox, oy));
    visited[oy as usize * w + ox as usize] = true;
    while let Some((x, y)) = stack.pop() {
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let ny = y + dy;
                if ny < 0 || ny >= h as isize {
                    continue;
                }
                let nx = heightmap.wrap_x(x + dx) as isize;
                let nidx = ny as usize * w + nx as usize;
                if visited[nidx] || *heightmap.get(nx, ny) > sea_level {
                    continue;
                }
                visited[nidx] = true;
                stack.push((nx, ny));
            }
        }
    }

    // 3. Every remaining body is a lake. Its surface is the water line, so its
    //    depth is simply how far the floor lies beneath the datum.
    for y0 in 0..h as isize {
        for x0 in 0..w as isize {
            let idx = y0 as usize * w + x0 as usize;
            if visited[idx] || *heightmap.get(x0, y0) > sea_level {
                continue;
            }
            stack.clear();
            stack.push((x0, y0));
            visited[idx] = true;
            is_lake.set(x0, y0, true);
            lake_depth.set(x0, y0, sea_level - *heightmap.get(x0, y0));
            while let Some((x, y)) = stack.pop() {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let ny = y + dy;
                        if ny < 0 || ny >= h as isize {
                            continue;
                        }
                        let nx = heightmap.wrap_x(x + dx) as isize;
                        let nidx = ny as usize * w + nx as usize;
                        if visited[nidx] || *heightmap.get(nx, ny) > sea_level {
                            continue;
                        }
                        visited[nidx] = true;
                        is_lake.set(nx, ny, true);
                        lake_depth.set(nx, ny, sea_level - *heightmap.get(nx, ny));
                        stack.push((nx, ny));
                    }
                }
            }
        }
    }
}

/// Clear every connected lake body smaller than `min_cells`, in place.
///
/// Priority flooding turns each closed pit into a lake, and the heightmap's own
/// roughness leaves thousands of single-pixel pits. An 8-connected sweep of the
/// lake mask gathers each body; any body narrower than the threshold is drained,
/// so `is_lake` ends up holding only water an atlas would actually draw.
fn erase_small_lakes(
    is_lake: &mut Grid<bool>,
    lake_depth: &mut Grid<f32>,
    min_cells: usize,
    w: usize,
    h: usize,
) {
    if min_cells <= 1 {
        return;
    }
    let mut visited = vec![false; w * h];
    let mut stack: Vec<(isize, isize)> = Vec::new();
    let mut body: Vec<(isize, isize)> = Vec::new();

    for y0 in 0..h as isize {
        for x0 in 0..w as isize {
            if visited[y0 as usize * w + x0 as usize] || !*is_lake.get(x0, y0) {
                continue;
            }
            body.clear();
            stack.clear();
            stack.push((x0, y0));
            visited[y0 as usize * w + x0 as usize] = true;
            while let Some((x, y)) = stack.pop() {
                body.push((x, y));
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let ny = y + dy;
                        if ny < 0 || ny >= h as isize {
                            continue;
                        }
                        let nx = is_lake.wrap_x(x + dx) as isize;
                        let idx = ny as usize * w + nx as usize;
                        if !visited[idx] && *is_lake.get(nx, ny) {
                            visited[idx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            if body.len() < min_cells {
                for &(x, y) in &body {
                    is_lake.set(x, y, false);
                    lake_depth.set(x, y, 0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A perfectly flat plain meeting the sea has no downhill anywhere, so a
    /// strict D8 router would strand every cell on it — this is the bug that
    /// shredded the river network. The ε-lifted routing surface tilts such a
    /// plain towards its outlet, so *every* interior land cell owns a
    /// strictly-lower neighbour to drain to, while the honest fill (which lake
    /// depth reads) is left flat.
    #[test]
    fn the_routing_surface_leaves_no_flat_to_strand_water() {
        let (w, h) = (9usize, 5usize);
        let sea_level = 0.40f32;
        // A dead-flat plain, with column four carved out as the sea.
        let mut heightmap = Grid::filled_with(w, h, 0.45f32);
        for y in 0..h {
            heightmap.set(4, y as isize, 0.35f32);
        }

        let config = HydrologyConfig::default();
        let result =
            fill_depressions_and_find_lakes(&heightmap, sea_level, &config, &Progress::silent());

        // The honest fill stays flat: nothing was deep enough to be a lake.
        for y in 0..h as isize {
            for x in 0..w as isize {
                if *heightmap.get(x, y) > sea_level {
                    assert!(
                        (*result.filled_heightmap.get(x, y) - 0.45).abs() < 1e-4,
                        "the honest fill must not lift the plain"
                    );
                }
            }
        }

        // The routing surface, though, drains. Interior rows are the ones the
        // production router actually routes over (it skips the polar rows).
        let neighbours = [
            (-1, 0), (1, 0), (0, -1), (0, 1),
            (-1, -1), (1, -1), (-1, 1), (1, 1),
        ];
        let mut stuck = 0;
        for y in 1..h as isize - 1 {
            for x in 0..w as isize {
                if *heightmap.get(x, y) <= sea_level {
                    continue;
                }
                let here = *result.routing_heightmap.get(x, y);
                let drains = neighbours
                    .iter()
                    .any(|&(dx, dy)| *result.routing_heightmap.get(x + dx, y + dy) < here);
                if !drains {
                    stuck += 1;
                }
            }
        }
        assert_eq!(stuck, 0, "no interior land cell may be stranded on a flat");
    }

    /// A one-pixel pit is heightmap noise, not a lake: it is drained, while a
    /// genuine basin scooped to the same depth survives the sweep.
    #[test]
    fn speck_lakes_are_drained_but_real_ones_survive() {
        let (w, h) = (11usize, 7usize);
        let sea_level = 0.40f32;
        // A high plain spilling to the sea in column zero, with two closed pits
        // scooped into it at the same depth: a single-cell speck and a 3x3 basin.
        let mut heightmap = Grid::filled_with(w, h, 0.60f32);
        for y in 0..h {
            heightmap.set(0, y as isize, 0.30f32);
            heightmap.set(1, y as isize, 0.55f32);
        }
        heightmap.set(5, 3, 0.55); // the speck
        for dy in -1..=1 {
            for dx in -1..=1 {
                heightmap.set(8 + dx, 3 + dy, 0.55); // the basin
            }
        }

        let mut config = HydrologyConfig::default();
        config.min_lake_cells = 5; // drain anything narrower than the 3x3 basin
        let result =
            fill_depressions_and_find_lakes(&heightmap, sea_level, &config, &Progress::silent());

        assert!(!*result.is_lake.get(5, 3), "a one-pixel speck must be drained");
        assert!(*result.is_lake.get(8, 3), "a real basin must survive");
        let lakes = result.is_lake.raw_slice().iter().filter(|&&l| l).count();
        assert_eq!(lakes, 9, "exactly the 3x3 basin should remain");
    }

    /// A basin below the datum that has no channel to the world ocean is a lake,
    /// not sea: the priority flood seeds it as an ocean outlet, so elevation alone
    /// would count it as ocean for ever. The reclaim pass must catch it.
    #[test]
    fn landlocked_water_below_the_datum_becomes_a_lake() {
        let (w, h) = (14usize, 7usize);
        let sea_level = 0.40f32;
        // A high plain, the world ocean along the western edge, and a landlocked
        // basin below the datum walled off by the plain.
        let mut heightmap = Grid::filled_with(w, h, 0.60f32);
        for y in 0..h {
            for x in 0..6 {
                heightmap.set(x as isize, y as isize, 0.30f32); // the world ocean
            }
        }
        for y in 2..=4 {
            for x in 9..=11 {
                heightmap.set(x, y, 0.35f32); // the landlocked basin
            }
        }

        let config = HydrologyConfig::default();
        let result =
            fill_depressions_and_find_lakes(&heightmap, sea_level, &config, &Progress::silent());

        assert!(!*result.is_lake.get(2, 3), "the world ocean must stay ocean");
        assert!(
            *result.is_lake.get(10, 3),
            "a walled-off basin below the datum is a lake"
        );
    }

    /// A bay below the datum that a channel joins to the world ocean is still the
    /// sea: connectivity, not elevation, decides. Only landlocked water is a lake.
    #[test]
    fn a_bay_joined_to_the_sea_stays_ocean() {
        let (w, h) = (14usize, 7usize);
        let sea_level = 0.40f32;
        let mut heightmap = Grid::filled_with(w, h, 0.60f32);
        for y in 0..h {
            for x in 0..6 {
                heightmap.set(x as isize, y as isize, 0.30f32); // the world ocean
            }
        }
        // A deep bay joined to the ocean by a one-cell channel at row three.
        for x in 6..=11 {
            heightmap.set(x, 3, 0.35f32);
        }
        for y in 2..=4 {
            for x in 9..=11 {
                heightmap.set(x, y, 0.35f32);
            }
        }

        let config = HydrologyConfig::default();
        let result =
            fill_depressions_and_find_lakes(&heightmap, sea_level, &config, &Progress::silent());

        assert!(
            !*result.is_lake.get(10, 3),
            "a bay joined to the sea is still the sea, not a lake"
        );
    }
}