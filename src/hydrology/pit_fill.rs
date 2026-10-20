use crate::core::Grid;
use crate::hydrology::config::HydrologyConfig;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, PartialEq)]
struct QueueItem {
    height: f32,
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
    pub filled_heightmap: Grid<f32>,
    pub is_lake: Grid<bool>,
    pub lake_depth: Grid<f32>,
}

/// Simulates water filling land depressions using the Barnes Priority-Flood algorithm.
/// Guarantees that all terrain slopes downhill to an ocean pour point.
pub fn fill_depressions_and_find_lakes(
    heightmap: &Grid<f32>,
    sea_level: f32,
    config: &HydrologyConfig,
) -> PitFillResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut filled = heightmap.clone();
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
    while let Some(current) = heap.pop() {
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

                // Detect inland lake (ONLY deep, major geological depressions become lakes!)
                let depth = filled_h - original_h;
                let ny_norm = ny as f32 / h as f32;
                let is_polar = ny_norm < 0.12 || ny_norm > 0.88;

                if depth > 0.012 && original_h > sea_level && !is_polar {
                    is_lake.set(nx, ny, true);
                    lake_depth.set(nx, ny, depth);
                }

                heap.push(QueueItem {
                    height: filled_h,
                    x: nx,
                    y: ny,
                });
            }
        }
    }

    PitFillResult {
        filled_heightmap: filled,
        is_lake,
        lake_depth,
    }
}