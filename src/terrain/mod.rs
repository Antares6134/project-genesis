pub mod config;
pub mod erosion;
pub mod fractal;
pub mod tectonics;
pub mod thermal;

pub use config::WorldConfig;
pub use erosion::simulate_hydraulic_erosion;
pub use fractal::apply_fractal_detail;
pub use tectonics::simulate_tectonics;
pub use thermal::simulate_thermal_erosion;