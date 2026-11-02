use crate::core::Grid;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};

#[derive(Clone, Serialize, Deserialize)]
pub struct WorldCache {
    pub width: usize,
    pub height: usize,
    pub heightmap: Grid<f32>,
    pub temp_grid: Grid<f32>,
    pub precip_grid: Grid<f32>,
    pub flow_grid: Grid<f32>,
    pub is_lake: Grid<bool>,
}

/// The same five grids, held by reference, for writing a cache without copying.
///
/// The pipeline keeps each grid behind its own `Arc` — the atlas has to be able
/// to read the world while the next act writes the one after it — so it never
/// holds a `WorldCache` to hand to [`WorldCache::save`]. Copying nearly 600 MB
/// into a throwaway struct just to serialize it would be absurd, and this is the
/// way out: bincode writes struct *fields*, in order, and knows nothing of their
/// names, so a view serialises to exactly the bytes `WorldCache` does.
#[derive(Serialize)]
pub struct WorldCacheView<'a> {
    pub width: usize,
    pub height: usize,
    pub heightmap: &'a Grid<f32>,
    pub temp_grid: &'a Grid<f32>,
    pub precip_grid: &'a Grid<f32>,
    pub flow_grid: &'a Grid<f32>,
    pub is_lake: &'a Grid<bool>,
}

impl WorldCache {
    /// Dumps the entire world data to a highly compressed binary file.
    ///
    /// The whole-payload pair with [`WorldCache::load`], kept for any caller that
    /// holds a complete cache. The pipeline does not: it holds the five grids
    /// separately, behind their own `Arc`s, so that the atlas can read a world
    /// while the next act writes the one after it — hence
    /// [`WorldCache::save_view`] and [`WorldCache::into_parts`], which move the
    /// same bytes without copying them. Both write exactly the same file.
    #[allow(dead_code)]
    pub fn save(&self, path: &str) {
        let file = File::create(path).expect("Failed to create cache file");
        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, self).expect("Failed to serialize world data");
    }

    /// Write a world held as separate grids, without assembling a copy of it.
    pub fn save_view(view: &WorldCacheView<'_>, path: &str) -> Result<(), String> {
        let file = File::create(path).map_err(|why| why.to_string())?;
        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, view).map_err(|why| why.to_string())
    }

    /// Loads the world data back into RAM instantly
    #[allow(dead_code)]
    pub fn load(path: &str) -> Self {
        Self::try_load(path).unwrap_or_else(|why| panic!("Failed to load cache '{path}': {why}"))
    }

    /// Loads the world data, reporting a missing or unreadable file instead of
    /// panicking: a reader who asks to load a world that is not there deserves a
    /// sentence, not a crash.
    pub fn try_load(path: &str) -> Result<Self, String> {
        let file = File::open(path).map_err(|why| format!("{path}: {why}"))?;
        let reader = BufReader::new(file);
        bincode::deserialize_from(reader).map_err(|why| format!("{path}: {why}"))
    }

    /// Take a loaded world apart into the grids the pipeline holds, so that
    /// loading costs no copying at all.
    #[allow(clippy::type_complexity)]
    pub fn into_parts(
        self,
    ) -> (usize, usize, Grid<f32>, Grid<f32>, Grid<f32>, Grid<f32>, Grid<bool>) {
        (
            self.width,
            self.height,
            self.heightmap,
            self.temp_grid,
            self.precip_grid,
            self.flow_grid,
            self.is_lake,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn little_world() -> WorldCache {
        WorldCache {
            width: 3,
            height: 2,
            heightmap: Grid::filled_with(3, 2, 0.42f32),
            temp_grid: Grid::filled_with(3, 2, -7.5f32),
            precip_grid: Grid::filled_with(3, 2, 0.11f32),
            flow_grid: Grid::filled_with(3, 2, 1_800.0f32),
            is_lake: Grid::filled_with(3, 2, false),
        }
    }

    /// A written cache and a written view must be the same file, byte for byte.
    ///
    /// The pipeline writes its world as a view; every world already on disk was
    /// written as a `WorldCache`. If these ever diverge, `--load` starts failing
    /// on worlds that used to open — so this is the test that guards every saved
    /// seed in `saved_seeds/`.
    #[test]
    fn a_cache_written_as_a_view_is_the_cache_written_whole() {
        let cache = little_world();

        let whole = bincode::serialize(&cache).expect("serialize the cache");
        let view = bincode::serialize(&WorldCacheView {
            width: cache.width,
            height: cache.height,
            heightmap: &cache.heightmap,
            temp_grid: &cache.temp_grid,
            precip_grid: &cache.precip_grid,
            flow_grid: &cache.flow_grid,
            is_lake: &cache.is_lake,
        })
        .expect("serialize the view");

        assert_eq!(whole, view, "the view must write exactly the cache's bytes");
    }

    /// A world written as a view must come back as the same world.
    #[test]
    fn a_world_written_as_a_view_loads_back_unchanged() {
        let cache = little_world();
        let path = std::env::temp_dir().join("project_genesis_cache_view_test.bin");
        let path = path.to_string_lossy().to_string();

        WorldCache::save_view(
            &WorldCacheView {
                width: cache.width,
                height: cache.height,
                heightmap: &cache.heightmap,
                temp_grid: &cache.temp_grid,
                precip_grid: &cache.precip_grid,
                flow_grid: &cache.flow_grid,
                is_lake: &cache.is_lake,
            },
            &path,
        )
        .expect("write the world");

        let loaded = WorldCache::try_load(&path).expect("read the world back");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.width, cache.width);
        assert_eq!(loaded.height, cache.height);
        assert_eq!(loaded.heightmap.raw_slice(), cache.heightmap.raw_slice());
        assert_eq!(loaded.temp_grid.raw_slice(), cache.temp_grid.raw_slice());
        assert_eq!(loaded.precip_grid.raw_slice(), cache.precip_grid.raw_slice());
        assert_eq!(loaded.flow_grid.raw_slice(), cache.flow_grid.raw_slice());
        assert_eq!(loaded.is_lake.raw_slice(), cache.is_lake.raw_slice());
    }

    /// A missing world is an error the atlas can show, never a panic.
    #[test]
    fn loading_a_world_that_is_not_there_is_an_error() {
        let missing = std::env::temp_dir().join("project_genesis_no_such_world.bin");
        assert!(
            WorldCache::try_load(&missing.to_string_lossy()).is_err(),
            "a missing cache must be reported, not panic"
        );
    }
}