use crate::core::Grid;
use crate::hydrology::config::HydrologyConfig;
use crate::hydrology::flow::{FlowDir, FlowResult};

pub struct RiverResult {
    pub is_river: Grid<bool>,
    pub river_width: Grid<f32>,
    pub basin_id: Grid<usize>,
}

/// Extracts river channels with dynamic widths and delineates continental drainage basins.
pub fn extract_rivers_and_basins(
    filled_heights: &Grid<f32>,
    flow_res: &FlowResult,
    sea_level: f32,
    config: &HydrologyConfig,
) -> RiverResult {
    let w = filled_heights.width;
    let h = filled_heights.height;

    let mut is_river = Grid::filled_with(w, h, false);
    let mut river_width = Grid::filled_with(w, h, 0.0f32);
    let mut basin_id = Grid::filled_with(w, h, 0usize);

    // 1. Identify river channels based on accumulated flow
    for y in 0..h {
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level {
                let water = *flow_res.accumulated_flow.get(x as isize, y as isize);
                if water >= config.min_river_flow {
                    is_river.set(x as isize, y as isize, true);
                    // River width scales with square root of flow volume
                    let normalized_width = ((water - config.min_river_flow) / config.major_river_flow).sqrt().clamp(0.2, 3.5);
                    river_width.set(x as isize, y as isize, normalized_width);
                }
            }
        }
    }

    // 2. Identify Ocean Pour Points (River Mouths) & Assign Drainage Basins
    let mut current_basin = 1usize;
    let mut ocean_mouths = Vec::new();

    for y in 0..h {
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level {
                if let FlowDir::Target(tx, ty) = *flow_res.flow_direction.get(x as isize, y as isize) {
                    if *filled_heights.get(tx, ty) <= sea_level {
                        // This land tile directly pours into the ocean!
                        basin_id.set(x as isize, y as isize, current_basin);
                        ocean_mouths.push((x as isize, y as isize, current_basin));
                        current_basin += 1;
                    }
                }
            }
        }
    }

    RiverResult {
        is_river,
        river_width,
        basin_id,
    }
}