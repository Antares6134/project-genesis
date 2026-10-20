use crate::core::Grid;
use crate::terrain::config::WorldConfig;

pub struct FractalDetailer;

impl FractalDetailer {
    pub fn new(_seed: u32) -> Self {
        Self
    }
}

/// Applies gentle micro-topography without modifying macro continental shapes.
pub fn apply_fractal_detail(
    _heightmap: &mut Grid<f32>,
    _config: &WorldConfig,
) {
    // Topography is generated in full fidelity directly by the 3D spherical engine!
}