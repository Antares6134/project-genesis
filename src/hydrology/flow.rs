use crate::core::progress::Progress;
use crate::core::sphere;
use crate::core::Grid;
use crate::export::live;
use crate::hydrology::config::{HydrologyConfig, SECONDS_PER_YEAR};

/// Rows of latitude between two progress checkpoints.
const ROWS_PER_CHECKPOINT: usize = 256;
/// How many river junctions may drain between two checkpoints.
const CELLS_PER_CHECKPOINT: usize = 200_000;

/// A linear `0 → 1` ramp: `0` at or below `lo`, `1` at or above `hi`, and the
/// fraction in between.
///
/// Used for the two climate gates on runoff. A non-finite input — the NaN
/// temperature the old pole maths could produce — reads as `0`, i.e. fully gated
/// off, so a broken reading can never invent a river.
#[inline(always)]
fn ramp_up(value: f32, lo: f32, hi: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let span = hi - lo;
    if span.abs() < f32::EPSILON {
        return if value >= hi { 1.0 } else { 0.0 };
    }
    ((value - lo) / span).clamp(0.0, 1.0)
}

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

/// Calculates D8 steepest downhill flow routing over a flat-free surface and
/// accumulates real discharge downstream.
///
/// `routing_heights` is the ε-lifted twin from [`crate::hydrology::pit_fill`] —
/// guaranteed to have a strictly-descending path to the sea from every cell, so
/// no water is ever stranded on a filled plateau. `precipitation` and
/// `temperature` are the climate's normalized rainfall and its air temperature:
/// each cell turns its own annual rainfall into cubic metres per second, scaled
/// by how much of that rain actually becomes runoff where it falls, so the
/// resulting grid is honest m³/s from wet ground and near-dry in deserts, frozen
/// wastes and on the ice caps.
pub fn calculate_flow(
    routing_heights: &Grid<f32>,
    precipitation: &Grid<f32>,
    temperature: &Grid<f32>,
    sea_level: f32,
    config: &HydrologyConfig,
    progress: &Progress,
) -> FlowResult {
    let w = routing_heights.width;
    let h = routing_heights.height;

    let mut flow_dir = Grid::filled_with(w, h, FlowDir::None);
    let mut in_degrees = Grid::filled_with(w, h, 0usize);
    let mut flow = Grid::filled_with(w, h, 0.0f32);

    let neighbors: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
        (-1, -1, std::f32::consts::SQRT_2), (1, -1, std::f32::consts::SQRT_2),
        (-1, 1, std::f32::consts::SQRT_2), (1, 1, std::f32::consts::SQRT_2),
    ];

    // 1. Initial surface discharge from precipitation, in m³/s.
    //
    //    Each land cell turns its own annual rainfall into a mean flow: the
    //    rainfall fraction × the reference rainfall depth gives metres of water
    //    per year, that times the cell's real ground area gives a volume per
    //    year, and dividing by the seconds in a year gives cubic metres per
    //    second. But only *some* of that rain becomes runoff where it falls:
    //    frozen ground sheds nothing and desert ground swallows its own rain, so
    //    the coefficient is scaled by the two climate gates before routing. A
    //    river therefore cannot be born on an ice cap or in a desert, while one
    //    that merely crosses those wastes keeps the water gathered upstream.
    progress.sub_step("Surface runoff", h as u64);
    for y in 0..h {
        if y % ROWS_PER_CHECKPOINT == 0 {
            progress.tick(y as u64);
        }
        let cell_area_m2 = sphere::pixel_area_km2(y, h, w) * 1.0e6;
        for x in 0..w {
            let elev = *routing_heights.get(x as isize, y as isize);
            if elev > sea_level {
                let rain = *precipitation.get(x as isize, y as isize);
                let temp = *temperature.get(x as isize, y as isize);
                let runoff = config.runoff_coefficient
                    * ramp_up(temp, config.runoff_freeze_temp, config.runoff_thaw_temp)
                    * ramp_up(rain, config.runoff_arid_precip, config.runoff_wet_precip);
                let rain_m_per_year = rain * config.reference_precip_mm_per_year / 1_000.0;
                let discharge = rain_m_per_year * cell_area_m2 / SECONDS_PER_YEAR * runoff;
                flow.set(x as isize, y as isize, discharge);
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
            let current_h = *routing_heights.get(x, y);
            if current_h <= sea_level {
                continue;
            }

            let mut best_slope = 0.0f32;
            let mut best_target = None;

            for &(dx, dy, dist) in &neighbors {
                let ny = y + dy;
                let nx = routing_heights.wrap_x(x + dx) as isize;

                let neighbor_h = *routing_heights.get(nx, ny);
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
            let elev = *routing_heights.get(x as isize, y as isize);
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
                live::water_at(routing_heights, None, Some(&flow), config.min_river_flow, x, y)
            });
        }

        let (cx, cy) = queue[head];
        head += 1;

        if let FlowDir::Target(tx, ty) = *flow_dir.get(cx, cy) {
            let current_water = *flow.get(cx, cy);
            *flow.get_mut(tx, ty) += current_water;

            let in_deg = in_degrees.get_mut(tx, ty);
            *in_deg -= 1;
            if *in_deg == 0 && *routing_heights.get(tx, ty) > sea_level {
                queue.push((tx, ty));
            }
        }
    }

    FlowResult {
        flow_direction: flow_dir,
        accumulated_flow: flow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::fill_depressions_and_find_lakes;

    /// A little world that drains steadily into a sea in column zero: the land
    /// steps up one notch per column away from the water.
    fn stepped(w: usize, h: usize) -> Grid<f32> {
        let mut heightmap = Grid::filled_with(w, h, 0.40f32);
        for y in 0..h {
            heightmap.set(0, y as isize, 0.35f32);
            for x in 1..w {
                heightmap.set(x as isize, y as isize, 0.42 + 0.02 * x as f32);
            }
        }
        heightmap
    }

    /// The largest accumulated discharge anywhere in the result.
    fn peak(flow: &FlowResult) -> f32 {
        flow.accumulated_flow
            .raw_slice()
            .iter()
            .copied()
            .fold(0.0f32, f32::max)
    }

    /// A headwater's discharge is real cubic metres per second: one cell's
    /// annual rainfall turned through the sphere's true ground area, with no
    /// upstream added. Seeding a single raindrop cell makes the planet's peak
    /// flow exactly that one cell's contribution.
    #[test]
    fn discharge_is_a_real_cubic_metre_per_second() {
        let (w, h) = (6usize, 5usize);
        let sea_level = 0.40f32;
        let heightmap = stepped(w, h);
        // Only one cell catches rain, so the peak flow is that cell alone.
        let mut precip = Grid::filled_with(w, h, 0.0f32);
        let (sx, sy) = ((w - 1) as isize, 2isize);
        precip.set(sx, sy, 0.5f32);
        // Warm, so the climate gates leave the runoff coefficient untouched.
        let temp = Grid::filled_with(w, h, 20.0f32);

        let config = HydrologyConfig::default();
        let progress = Progress::silent();
        let pit = fill_depressions_and_find_lakes(&heightmap, sea_level, &config, &progress);
        let flow =
            calculate_flow(&pit.routing_heightmap, &precip, &temp, sea_level, &config, &progress);

        let rain_m_per_year = 0.5 * config.reference_precip_mm_per_year / 1_000.0;
        let cell_area_m2 = sphere::pixel_area_km2(sy as usize, h, w) * 1.0e6;
        let expected =
            rain_m_per_year * cell_area_m2 / SECONDS_PER_YEAR * config.runoff_coefficient;

        let got = peak(&flow);
        assert!(expected > 0.0, "the conversion must yield positive discharge");
        assert!(
            (got - expected).abs() < expected * 1e-3,
            "peak discharge {got} m³/s should equal one cell's {expected} m³/s"
        );
    }

    /// Frozen ground sheds nothing: with the whole world below freezing, no cell
    /// runs off and no discharge accumulates — which is why no river can reach
    /// an ice cap.
    #[test]
    fn frozen_ground_sheds_no_runoff() {
        let (w, h) = (6usize, 5usize);
        let heightmap = stepped(w, h);
        let precip = Grid::filled_with(w, h, 0.6f32);
        let temp = Grid::filled_with(w, h, -8.0f32);

        let config = HydrologyConfig::default();
        let progress = Progress::silent();
        let pit = fill_depressions_and_find_lakes(&heightmap, 0.40, &config, &progress);
        let flow = calculate_flow(&pit.routing_heightmap, &precip, &temp, 0.40, &config, &progress);

        assert_eq!(peak(&flow), 0.0, "a frozen world must route no water at all");
    }

    /// The Nile rule: a river born on wet ground keeps its water even where it
    /// crosses a desert, but a world that is desert everywhere grows no river.
    #[test]
    fn a_wet_headwater_survives_a_desert_course() {
        let (w, h) = (6usize, 5usize);
        let heightmap = stepped(w, h);
        let warm = Grid::filled_with(w, h, 25.0f32);

        let config = HydrologyConfig::default();
        let progress = Progress::silent();
        let pit = fill_depressions_and_find_lakes(&heightmap, 0.40, &config, &progress);

        // A wet headwater in the top column, a desert course everywhere below it.
        let mut precip = Grid::filled_with(w, h, 0.02f32);
        precip.set((w - 1) as isize, 2, 0.5);
        let nile =
            calculate_flow(&pit.routing_heightmap, &precip, &warm, 0.40, &config, &progress);
        assert!(
            peak(&nile) > 0.0,
            "a river born on wet ground must survive its desert course"
        );

        // The same channel, but the headwater is desert too: nothing to carry.
        let desert = Grid::filled_with(w, h, 0.02f32);
        let dry =
            calculate_flow(&pit.routing_heightmap, &desert, &warm, 0.40, &config, &progress);
        assert_eq!(
            peak(&dry),
            0.0,
            "a desert-born channel is dry and must not be drawn"
        );
    }
}