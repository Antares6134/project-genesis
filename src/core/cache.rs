use crate::core::Grid;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};

#[derive(Serialize, Deserialize)]
pub struct WorldCache {
    pub width: usize,
    pub height: usize,
    pub heightmap: Grid<f32>,
    pub temp_grid: Grid<f32>,
    pub precip_grid: Grid<f32>,
    pub flow_grid: Grid<f32>,
    pub is_lake: Grid<bool>,
}

impl WorldCache {
    /// Dumps the entire world data to a highly compressed binary file
    pub fn save(&self, path: &str) {
        let file = File::create(path).expect("Failed to create cache file");
        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, self).expect("Failed to serialize world data");
    }

    /// Loads the world data back into RAM instantly
    pub fn load(path: &str) -> Self {
        let file = File::open(path).expect("Failed to open cache file");
        let reader = BufReader::new(file);
        bincode::deserialize_from(reader).expect("Failed to deserialize world data")
    }
}