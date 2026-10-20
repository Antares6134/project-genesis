use crate::core::Grid;
use crate::climate::config::{ClimateConfig, PlanetaryMode};

#[derive(Clone, Copy, Debug, Default)]
pub struct WindVector {
    pub dx: f32,
    pub dy: f32,
}

/// Generates the global atmospheric circulation wind field.
pub fn simulate_winds(width: usize, height: usize, config: &ClimateConfig) -> Grid<WindVector> {
    let mut wind_grid = Grid::filled_with(width, height, WindVector { dx: 1.0, dy: 0.0 });

    for y in 0..height {
        let v = y as f32 / (height as f32 - 1.0); // 0.0 (North Pole) to 1.0 (South Pole)
        let lat_deg = (0.5 - v) * 180.0;          // +90 (North) to -90 (South)

        // Earth-like 3-cell atmospheric circulation
        let (base_dx, base_dy) = match config.mode {
            PlanetaryMode::RotatingStandard { .. } => {
                let abs_lat = lat_deg.abs();
                if abs_lat < 30.0 {
                    // Hadley Cell: Trade Winds blow East -> West (-dx), heading towards equator
                    let equator_pull = if lat_deg > 0.0 { 0.25 } else { -0.25 };
                    (-1.0, equator_pull)
                } else if abs_lat < 60.0 {
                    // Ferrel Cell: Westerlies blow West -> East (+dx), heading towards poles
                    let pole_pull = if lat_deg > 0.0 { -0.2 } else { 0.2 };
                    (1.2, pole_pull)
                } else {
                    // Polar Cell: Polar Easterlies blow East -> West (-dx)
                    (-0.8, 0.0)
                }
            }
            PlanetaryMode::TidallyLocked => {
                // Wind rushes violently from cold night side to hot day side center
                (0.8, 0.0)
            }
        };

        for x in 0..width {
            wind_grid.set(x as isize, y as isize, WindVector { dx: base_dx, dy: base_dy });
        }
    }

    wind_grid
}