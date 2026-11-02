use crate::core::progress::Progress;
use crate::core::Grid;
use crate::export::live;
use crate::hydrology::config::HydrologyConfig;

/// Rows of latitude between two progress checkpoints.
const ROWS_PER_CHECKPOINT: usize = 256;
/// How many river junctions may drain between two checkpoints.
const CELLS_PER_CHECKPOINT: usize = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum FlowDir {
    #[default]
    None,
    Target(isize, isize),
}

pub struct FlowResult {
    pub flow_direction: Grid<FlowDir>,
    pub accumulated_flow: Grid<f32>,
}

/// Calculates D8 steepest downhill flow routing and accumulates precipitation downstream.
pub fn calculate_flow(
    filled_heights: &Grid<f32>,
    precipitation: &Grid<f32>,
    sea_level: f32,
    config: &HydrologyConfig,
    progress: &Progress,
) -> FlowResult {
    let w = filled_heights.width;
    let h = filled_heights.height;

    let mut flow_dir = Grid::filled_with(w, h, FlowDir::None);
    let mut in_degrees = Grid::filled_with(w, h, 0usize);
    let mut flow = Grid::filled_with(w, h, 0.0f32);

    let neighbors: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
        (-1, -1, std::f32::consts::SQRT_2), (1, -1, std::f32::consts::SQRT_2),
        (-1, 1, std::f32::consts::SQRT_2), (1, 1, std::f32::consts::SQRT_2),
    ];

    // 1. Initial surface runoff from precipitation
    progress.sub_step("Surface runoff", h as u64);
    for y in 0..h {
        if y % ROWS_PER_CHECKPOINT == 0 {
            progress.tick(y as u64);
        }
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level {
                let rain = *precipitation.get(x as isize, y as isize);
                flow.set(x as isize, y as isize, (rain * config.runoff_multiplier).max(0.05));
            }
        }
    }

    // 2. Determine D8 steepest downhill neighbor
    progress.sub_step("Steepest descent", h as u64);
    for y in 1..(h as isize - 1) {
        if (y as usize) % ROWS_PER_CHECKPOINT == 0 {
            progress.tick(y as u64);
        }
        for x in 0..(w as isize) {
            let current_h = *filled_heights.get(x, y);
            if current_h <= sea_level {
                continue;
            }

            let mut best_slope = 0.0f32;
            let mut best_target = None;

            for &(dx, dy, dist) in &neighbors {
                let ny = y + dy;
                let nx = filled_heights.wrap_x(x + dx) as isize;

                let neighbor_h = *filled_heights.get(nx, ny);
                let slope = (current_h - neighbor_h) / dist;

                if slope > best_slope {
                    best_slope = slope;
                    best_target = Some((nx, ny));
                }
            }

            if let Some((tx, ty)) = best_target {
                flow_dir.set(x, y, FlowDir::Target(tx, ty));
                *in_degrees.get_mut(tx, ty) += 1;
            }
        }
    }

    // 3. Topological sort flow accumulation (Kahn's Algorithm: leaves to roots)
    //    This is where the world's rivers actually appear: water gathers
    //    downstream, so the atlas is shown the network growing as it drains.
    progress.sub_step("Flow accumulation", 0);
    let mut queue = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level && *in_degrees.get(x as isize, y as isize) == 0 {
                queue.push((x as isize, y as isize));
            }
        }
    }

    let mut head = 0;
    while head < queue.len() {
        if head % CELLS_PER_CHECKPOINT == 0 {
            progress.tick(head as u64);
            progress.preview_map("Rivers reaching the sea", (w, h), |x, y| {
                live::water_at(filled_heights, None, Some(&flow), config.min_river_flow, x, y)
            });
        }

        let (cx, cy) = queue[head];
        head += 1;

        if let FlowDir::Target(tx, ty) = *flow_dir.get(cx, cy) {
            let current_water = *flow.get(cx, cy);
            *flow.get_mut(tx, ty) += current_water;

            let in_deg = in_degrees.get_mut(tx, ty);
            *in_deg -= 1;
            if *in_deg == 0 && *filled_heights.get(tx, ty) > sea_level {
                queue.push((tx, ty));
            }
        }
    }

    FlowResult {
        flow_direction: flow_dir,
        accumulated_flow: flow,
    }
}