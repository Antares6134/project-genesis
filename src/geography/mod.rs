pub mod config;
pub mod oceans;
pub mod provinces;

pub use config::{GeographyConfig, OceanConfig};
pub use oceans::{OceanResult, generate_ocean_provinces};
pub use provinces::{ProvinceResult, generate_provinces};
