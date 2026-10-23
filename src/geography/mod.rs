pub mod config;
pub mod oceans;
pub mod provinces;

pub use config::{GeographyConfig, OceanConfig};
pub use oceans::{generate_ocean_provinces, OceanResult};
pub use provinces::{generate_provinces, ProvinceResult};