#[derive(Clone, Debug)]
pub struct GeographyConfig {
    pub num_provinces: usize,
}

impl Default for GeographyConfig {
    fn default() -> Self {
        Self {
            num_provinces: 30_000, // Grand Strategy Location Scale!
        }
    }
}