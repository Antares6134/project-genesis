//! The seven acts of creation, as a state the atlas can drive.
//!
//! The engine used to be a straight line: `main` ran every stage to completion
//! and only then opened a window. It is now a *pipeline* — a [`Genesis`] value
//! that holds whichever acts have been run so far, hands each finished act to the
//! atlas, and knows which act may be run next.
//!
//! ```text
//!   1 Geology & Erosion          heightmap, after 3.5 M droplets
//!   2 Atmospheric Climate        temperature, winds, rainfall
//!   3 Hydrology & Rivers         lakes, flow accumulation  → world cache writable
//!   4 Geography & Provinces      the land cut into provinces
//!   5 Geopolitical Hierarchy     provinces → areas → regions → superregions
//!   6 Hydro-Political Hierarchy  the world ocean, same five tiers
//!   7 Dawn of Civilization       realms, societies, populations
//! ```
//!
//! Three properties make this work, and every change here must preserve them:
//!
//! - **Each act produces data and hands it over.** Nothing is generated in the
//!   viewer, and nothing is generated during painting: [`Genesis::run_stage`]
//!   either completes an act or leaves the pipeline exactly as it was.
//! - **Artefacts are shared, not copied.** Every finished result is an `Arc`
//!   clone away, so a stage running on a worker thread can read the world the
//!   atlas is still painting, and the atlas pays nothing but an atomic increment
//!   to keep looking at it. This is what lets the map stay live while the next
//!   act runs — and it rests on one rule, which every stage here obeys: **a grid
//!   that has been published is never mutated again.** A stage reads the grids it
//!   was given and builds the new ones beside them; it hands the new ones over
//!   only when they are whole. There is therefore no lock anywhere in this file,
//!   and no way for the atlas to see a half-written world.
//! - **The order and the numbers are unchanged.** The same seed still grows the
//!   same planet as it did before the pipeline existed — the only difference is
//!   *when* each act runs, never *what* it computes.

use crate::civilization::{
    generate_civilizations, CivilizationConfig, CivilizationWorld,
};
use crate::climate::{
    simulate_moisture, simulate_temperature, simulate_winds, ClimateConfig,
};
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::progress::{MapRefs, Progress, Sink};
use crate::core::{polar, relief, Grid, WorldCache, WorldCacheView};
use crate::geography::{
    generate_ocean_provinces, generate_provinces, GeographyConfig, OceanConfig, OceanResult,
    ProvinceResult,
};
use crate::geopolitics::{
    build_geopolitical_hierarchy, build_ocean_hierarchy, extract_ocean_graph,
    extract_province_graph, GeopoliticalHierarchy,
};
use crate::hydrology::{
    calculate_flow, fill_depressions_and_find_lakes, HydrologyConfig,
};
use crate::terrain::{
    apply_fractal_detail, simulate_hydraulic_erosion, simulate_tectonics, simulate_thermal_erosion,
    WorldConfig,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Canvas the engine grows: 8K. Fixed, because a world's resolution is part of
/// its identity — a seed must always mean exactly one planet.
pub const WORLD_WIDTH: usize = 8192;
pub const WORLD_HEIGHT: usize = 4096;

/// Directory a world's cache is written to.
pub const CACHE_DIR: &str = "./saved_seeds";

/// The seven acts of creation, in the only order they can be run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Geology,
    Climate,
    Hydrology,
    Provinces,
    Geopolitics,
    HydroPolitics,
    Civilization,
}

impl Stage {
    /// Every act, in order. The atlas lays its buttons out from this.
    pub const ALL: [Stage; 7] = [
        Stage::Geology,
        Stage::Climate,
        Stage::Hydrology,
        Stage::Provinces,
        Stage::Geopolitics,
        Stage::HydroPolitics,
        Stage::Civilization,
    ];

    /// Where the act sits in the sequence, counting from one.
    pub fn index(self) -> usize {
        match self {
            Stage::Geology => 1,
            Stage::Climate => 2,
            Stage::Hydrology => 3,
            Stage::Provinces => 4,
            Stage::Geopolitics => 5,
            Stage::HydroPolitics => 6,
            Stage::Civilization => 7,
        }
    }

    /// The act's name, as the atlas and the terminal print it.
    pub fn title(self) -> &'static str {
        match self {
            Stage::Geology => "Geology & Erosion",
            Stage::Climate => "Atmospheric Climate",
            Stage::Hydrology => "Hydrology & Rivers",
            Stage::Provinces => "Geography & Provinces",
            Stage::Geopolitics => "Geopolitical Hierarchy",
            Stage::HydroPolitics => "Hydro-Political Hierarchy",
            Stage::Civilization => "Dawn of Civilization",
        }
    }

    /// One line saying what the act makes, for a button's tooltip.
    pub fn blurb(self) -> &'static str {
        match self {
            Stage::Geology => {
                "Tectonic plates, fractal detail and hydraulic + thermal erosion: the raw land."
            }
            Stage::Climate => {
                "Insolation, circulation and rain shadows: temperature, winds and rainfall."
            }
            Stage::Hydrology => {
                "Depressions fill into lakes and rainfall gathers into rivers. The world cache is \
                 written here."
            }
            Stage::Provinces => "The inhabited land divided into provinces, its wilderness mapped.",
            Stage::Geopolitics => "Provinces gathered into areas, regions, superregions and continents.",
            Stage::HydroPolitics => "The world ocean partitioned into its own five matching tiers.",
            Stage::Civilization => "Realms rise, spread and are given a history, an age and a people.",
        }
    }

    /// True when a world that has reached *this* act is old enough to have what
    /// `needed` produced — that is, when this act is the same one or a later one.
    ///
    /// This is how the painter asks whether a grid exists at all: the atmosphere
    /// arrives at act two and the water at act three, and a world younger than
    /// that has no temperature and no rivers to read.
    pub fn available(self, needed: Stage) -> bool {
        self.index() >= needed.index()
    }

    /// The act that must be finished before this one can run.
    #[allow(dead_code)]
    fn previous(self) -> Option<Stage> {
        match self.index() {
            1 => None,
            n => Some(Stage::ALL[n - 2]),
        }
    }
}

/// What the pipeline knows about one act.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StageState {
    /// An earlier act has not been run yet.
    Locked,
    /// The act that may be run next.
    Ready,
    /// Finished, with how long it took.
    Done { secs: f32 },
}

/// What a finished act reports back to whoever ran it.
pub struct StageReport {
    pub stage: Stage,
    pub secs: f32,
    /// A few figures the atlas can show for the act: what it produced, in
    /// numbers. Never a substitute for the terminal's own narration.
    pub summary: Vec<String>,
}

/// Why a world could not be loaded.
#[derive(Debug)]
pub enum LoadError {
    /// There is no cache for this seed at all.
    Missing(PathBuf),
    /// There is a cache, and it could not be read.
    Unreadable(PathBuf, String),
}

/// Everything the atlas needs to paint the world as it stands.
///
/// Assembled from whichever acts have been run, so a world three acts old and a
/// world seven acts old are both paintable. The fields whose act has not been
/// run yet hold *empty* values — an empty hierarchy, a civilization with nobody
/// in it — never stale or placeholder data, and `stage` tells the painter which
/// is which: it must not read the province grids before act four, or the realms
/// before act seven.
#[derive(Clone)]
pub struct AtlasWorld {
    /// The last act that finished. Everything below is real up to here.
    pub stage: Stage,
    /// Canvas size — known from the moment act one lands.
    pub width: usize,
    pub height: usize,
    /// The surface, in heightmap units. Act one onward.
    pub heightmap: Arc<Grid<f32>>,
    /// Mean annual temperature — act two onward.
    pub temperature: Option<Arc<Grid<f32>>>,
    /// Annual rainfall, in the engine's own units — act two onward.
    pub precipitation: Option<Arc<Grid<f32>>>,
    /// Accumulated downhill flow — act three onward.
    pub flow: Option<Arc<Grid<f32>>>,
    /// Inland water — act three onward.
    pub is_lake: Option<Arc<Grid<bool>>>,
    /// Land provinces — act four onward.
    pub provinces: Option<Arc<ProvinceResult>>,
    /// The land hierarchy — act five onward.
    pub hierarchy: Arc<GeopoliticalHierarchy>,
    /// Marine provinces — act six onward.
    pub ocean: Option<Arc<OceanResult>>,
    /// The ocean hierarchy — act six onward.
    pub ocean_hierarchy: Arc<GeopoliticalHierarchy>,
    /// The dawn of history — act seven onward.
    pub civilizations: Arc<CivilizationWorld>,
}

/// The world being built, act by act.
///
/// This is the whole of the engine's state: what has been made, what may be made
/// next, and what the atlas is allowed to look at while it waits.
///
/// `Clone` is cheap and deliberate: every grid in here is behind its own `Arc`,
/// so cloning a pipeline copies a handful of pointers rather than a planet. That
/// is what lets the atlas keep a copy of the world as it was before an act, and
/// put it back if that act fails to finish.
#[derive(Clone)]
pub struct Genesis {
    seed: u64,
    /// The last act that finished, and therefore the act everything below is
    /// valid up to.
    stage: Option<Stage>,
    /// The canvas, and the five grids of a world — each behind its own `Arc`, so
    /// that a finished act is handed to the atlas without copying a pixel of it.
    /// A grid exists exactly when its act has run: there is no placeholder and no
    /// half-written world for the painter to mistake for real data.
    width: usize,
    height: usize,
    heightmap: Option<Arc<Grid<f32>>>,
    temperature: Option<Arc<Grid<f32>>>,
    precipitation: Option<Arc<Grid<f32>>>,
    flow: Option<Arc<Grid<f32>>>,
    is_lake: Option<Arc<Grid<bool>>>,
    /// Permanent polar ice, resolved at the end of act three: the belt is
    /// impassable wilderness, so the province generator has to know where it is.
    polar_ice: Option<Arc<Grid<bool>>>,
    provinces: Option<Arc<ProvinceResult>>,
    hierarchy: Option<Arc<GeopoliticalHierarchy>>,
    ocean: Option<Arc<OceanResult>>,
    ocean_hierarchy: Option<Arc<GeopoliticalHierarchy>>,
    civilizations: Option<Arc<CivilizationWorld>>,
    /// How long each act took, so the atlas can show it done.
    timings: [Option<f32>; 7],
    /// Write the world cache as soon as act three lands.
    autosave: bool,
}

impl Genesis {
    /// A world that has not been begun: a seed, and nothing else.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            stage: None,
            width: WORLD_WIDTH,
            height: WORLD_HEIGHT,
            heightmap: None,
            temperature: None,
            precipitation: None,
            flow: None,
            is_lake: None,
            polar_ice: None,
            provinces: None,
            hierarchy: None,
            ocean: None,
            ocean_hierarchy: None,
            civilizations: None,
            timings: [None; 7],
            autosave: false,
        }
    }

    /// The seed this world grows from.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Start over on a new seed, forgetting everything the old one grew.
    pub fn set_seed(&mut self, seed: u64) {
        if seed == self.seed {
            return;
        }
        let autosave = self.autosave;
        *self = Self::new(seed);
        self.autosave = autosave;
    }

    /// Whether act three writes the world cache when it finishes.
    pub fn set_autosave(&mut self, on: bool) {
        self.autosave = on;
    }

    /// Whether act three would write the world cache right now.
    pub fn autosave(&self) -> bool {
        self.autosave
    }

    /// The act that may be run next, or `None` when the world is finished.
    pub fn ready(&self) -> Option<Stage> {
        match self.stage {
            None => Some(Stage::Geology),
            Some(last) => Stage::ALL.get(last.index()).copied(),
        }
    }

    /// What the pipeline knows about one act.
    pub fn state(&self, stage: Stage) -> StageState {
        if let Some(secs) = self.timings[stage.index() - 1] {
            return StageState::Done { secs };
        }
        if self.ready() == Some(stage) {
            StageState::Ready
        } else {
            StageState::Locked
        }
    }

    // ─── THE WORLD'S OWN CONSTANTS ───────────────────────────────────────────
    //
    // Held here rather than in `main`, because every act needs them and they all
    // derive from the one thing that identifies a world: its seed.

    pub fn world_config(&self) -> WorldConfig {
        WorldConfig {
            width: WORLD_WIDTH,
            height: WORLD_HEIGHT,
            seed: self.seed,
            num_plates: 32,
            ocean_ratio: 0.65,
            erosion_droplets: 3_500_000,
            ..Default::default()
        }
    }

    pub fn climate_config(&self) -> ClimateConfig {
        ClimateConfig { seed: self.seed, ..Default::default() }
    }

    pub fn hydro_config(&self) -> HydrologyConfig {
        HydrologyConfig {
            min_lake_depth: 0.015,
            min_river_flow: 1800.0,
            major_river_flow: 15_000.0,
            ..Default::default()
        }
    }

    pub fn geography_config(&self) -> GeographyConfig {
        GeographyConfig::default()
    }

    pub fn ocean_config(&self) -> OceanConfig {
        OceanConfig::default()
    }

    pub fn civilization_config(&self) -> CivilizationConfig {
        CivilizationConfig::default()
    }

    // ─── THE WORLD CACHE ─────────────────────────────────────────────────────

    /// Where this world's cache lives.
    pub fn cache_path(&self) -> PathBuf {
        cache_path(self.seed)
    }

    /// Write the completed acts one to three to disk, as `world_<seed>.bin`.
    ///
    /// Only the first three — the heightmap, the climate and the water — because
    /// those are the expensive, purely physical half of the pipeline, and the
    /// half that a later act never changes. Everything after act three is cheap
    /// enough to run again from the cache on demand.
    pub fn save_cache(&self) -> Result<PathBuf, LoadError> {
        let path = self.cache_path();
        let (Some(heightmap), Some(temperature), Some(precipitation), Some(flow), Some(is_lake)) = (
            &self.heightmap,
            &self.temperature,
            &self.precipitation,
            &self.flow,
            &self.is_lake,
        ) else {
            // The first three acts are not finished, so there is no world yet.
            return Err(LoadError::Missing(path));
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        println!("Saving world state to cache '{}'…", path.display());
        WorldCache::save_view(
            &WorldCacheView {
                width: self.width,
                height: self.height,
                heightmap,
                temp_grid: temperature,
                precip_grid: precipitation,
                flow_grid: flow,
                is_lake,
            },
            &path.to_string_lossy(),
        )
        .map_err(|why| LoadError::Unreadable(path.clone(), why))?;
        Ok(path)
    }

    /// Adopt the acts one to three that are already on disk for this seed.
    ///
    /// On success the pipeline holds a complete physical world and the next
    /// pressable act is *Geography & Provinces*: the same thing `--load` has
    /// always done, only now it happens mid-session instead of before the window
    /// opens.
    pub fn load_cache(&mut self) -> Result<(), LoadError> {
        let path = self.cache_path();
        if !Path::new(&path).exists() {
            return Err(LoadError::Missing(path));
        }
        let start = Instant::now();
        println!("Loading world from cache '{}'…", path.display());
        let cache = WorldCache::try_load(&path.to_string_lossy())
            .map_err(|why| LoadError::Unreadable(path.clone(), why))?;

        // A loaded world has acts one to three behind it, and no polar ice yet:
        // the belt is resolved below exactly as it is after act three is run,
        // which is what keeps a loaded world identical to a grown one.
        let (width, height, heightmap, temperature, precipitation, flow, is_lake) =
            cache.into_parts();
        self.width = width;
        self.height = height;
        self.heightmap = Some(Arc::new(heightmap));
        self.temperature = Some(Arc::new(temperature));
        self.precipitation = Some(Arc::new(precipitation));
        self.flow = Some(Arc::new(flow));
        self.is_lake = Some(Arc::new(is_lake));
        self.finish_polar_ice();
        self.stage = Some(Stage::Hydrology);
        // The first two acts are not timed — they happened in another session —
        // so they report their `Done` state with the zero that marks a stage
        // adopted from disk rather than run just now.
        self.timings[0] = Some(0.0);
        self.timings[1] = Some(0.0);
        self.timings[2] = Some(0.0);
        println!("✓ Loaded cached world data in:             {:.2?}", start.elapsed());
        Ok(())
    }
}

/// Where a world's cache lives.
pub fn cache_path(seed: u64) -> PathBuf {
    PathBuf::from(format!("{CACHE_DIR}/world_{seed}.bin"))
}

impl Genesis {
    /// Run one act of creation, reporting to `sink` if anyone is watching.
    ///
    /// `None` for a locked act: an act whose predecessor has not been run cannot
    /// be run, and the pipeline is left untouched rather than half-built.
    pub fn run_stage(&mut self, stage: Stage, sink: Option<Arc<Sink>>) -> Option<StageReport> {
        if self.state(stage) != StageState::Ready {
            return None;
        }
        let start = Instant::now();
        let summary = match stage {
            Stage::Geology => self.run_geology(&sink),
            Stage::Climate => self.run_climate(&sink),
            Stage::Hydrology => self.run_hydrology(&sink),
            Stage::Provinces => self.run_provinces(&sink),
            Stage::Geopolitics => self.run_geopolitics(&sink),
            Stage::HydroPolitics => self.run_hydro_politics(&sink),
            Stage::Civilization => self.run_civilization(&sink),
        };
        let secs = start.elapsed().as_secs_f32();
        self.timings[stage.index() - 1] = Some(secs);
        self.stage = Some(stage);
        println!(
            "✓ [{}/7] {} completed: {:.2?}",
            stage.index(),
            stage.title(),
            start.elapsed()
        );
        Some(StageReport { stage, secs, summary })
    }

    /// Run every act up to and including `last`, in order.
    ///
    /// This is the headless path: `--save`, `--civs` and `--plates` all want a
    /// finished world and a terminal full of figures, and none of them wants a
    /// window.
    pub fn run_through(&mut self, last: Stage, sink: Option<Arc<Sink>>) {
        while let Some(next) = self.ready() {
            if next.index() > last.index() {
                break;
            }
            self.run_stage(next, sink.clone());
        }
    }

    /// A world of a given age, for tests that need one without growing one.
    ///
    /// A four-by-two planet with whatever grids its acts would have made — small
    /// enough to paint in a test, and shaped exactly like a real world's state:
    /// the same grids present, the same acts done, the same act waiting.
    #[cfg(test)]
    pub fn for_test(seed: u64, stage: Stage) -> Self {
        let (width, height) = (4, 2);
        let mut genesis = Self::new(seed);
        genesis.width = width;
        genesis.height = height;
        genesis.stage = Some(stage);
        genesis.heightmap = Some(Arc::new(Grid::filled_with(width, height, 0.45f32)));
        if stage.available(Stage::Climate) {
            genesis.temperature = Some(Arc::new(Grid::filled_with(width, height, 12.0f32)));
            genesis.precipitation = Some(Arc::new(Grid::filled_with(width, height, 0.4f32)));
        }
        if stage.available(Stage::Hydrology) {
            genesis.flow = Some(Arc::new(Grid::filled_with(width, height, 0.0f32)));
            genesis.is_lake = Some(Arc::new(Grid::filled_with(width, height, false)));
        }
        for act in Stage::ALL {
            // Acts already behind a world adopted from disk were not timed here,
            // which is what the panel reports as `loaded`.
            genesis.timings[act.index() - 1] =
                if act.index() <= stage.index() { Some(0.0) } else { None };
        }
        genesis
    }

    /// The world as the atlas may paint it right now, or `None` before act one.
    pub fn atlas(&self) -> Option<AtlasWorld> {
        let heightmap = self.heightmap.clone()?;
        let stage = self.stage?;
        Some(AtlasWorld {
            stage,
            width: self.width,
            height: self.height,
            heightmap,
            temperature: self.temperature.clone(),
            precipitation: self.precipitation.clone(),
            flow: self.flow.clone(),
            is_lake: self.is_lake.clone(),
            provinces: self.provinces.clone(),
            hierarchy: self
                .hierarchy
                .clone()
                .unwrap_or_else(|| Arc::new(GeopoliticalHierarchy::empty())),
            ocean: self.ocean.clone(),
            ocean_hierarchy: self
                .ocean_hierarchy
                .clone()
                .unwrap_or_else(|| Arc::new(GeopoliticalHierarchy::empty())),
            civilizations: self
                .civilizations
                .clone()
                .unwrap_or_else(|| Arc::new(CivilizationWorld::empty(self.seed))),
        })
    }

    // ─── 1. GEOLOGY & EROSION ────────────────────────────────────────────────

    fn run_geology(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let world_config = self.world_config();
        let progress = watching(sink);

        let mut result = simulate_tectonics(&world_config, &progress);
        apply_fractal_detail(&mut result.heightmap, &world_config);
        simulate_hydraulic_erosion(&mut result.heightmap, &world_config, &progress);
        simulate_thermal_erosion(&mut result.heightmap, &world_config, &progress);

        let profile = relief_profile(&result.heightmap);
        report_relief(&profile);
        self.heightmap = Some(Arc::new(result.heightmap));

        vec![
            format!(
                "Surface: {:.1}% land / {:.1}% ocean",
                100.0 * profile.land_share,
                100.0 * (1.0 - profile.land_share)
            ),
            format!(
                "Peak {} m · deepest trench {} m",
                thousands(profile.peak),
                thousands(-profile.trench).replacen('-', "−", 1)
            ),
        ]
    }

    // ─── 2. ATMOSPHERIC CLIMATE ──────────────────────────────────────────────

    fn run_climate(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let world_config = self.world_config();
        let climate_config = self.climate_config();
        let progress = watching(sink);
        let heightmap = self.heightmap.clone().expect("act one runs before act two");

        let temperature =
            simulate_temperature(&heightmap, relief::SEA_LEVEL, &climate_config, &progress);
        let winds = simulate_winds(
            world_config.width,
            world_config.height,
            &climate_config,
            &progress,
        );
        let moisture = simulate_moisture(
            &heightmap,
            &temperature,
            &winds,
            relief::SEA_LEVEL,
            &climate_config,
            &progress,
        );

        // The new grids are built beside the world the atlas is painting and
        // published only once they are whole, so no reader ever sees a
        // half-simulated climate.
        let (mean_temp, mean_rain) = climate_means(&temperature, &moisture.precipitation);
        self.temperature = Some(Arc::new(temperature));
        self.precipitation = Some(Arc::new(moisture.precipitation));

        vec![
            format!("Mean surface temperature {mean_temp:.1} °C"),
            format!("Mean annual rainfall {:.0} mm/yr", mean_rain * 1200.0),
        ]
    }

    // ─── 3. HYDROLOGY & RIVERS ───────────────────────────────────────────────

    fn run_hydrology(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let hydro_config = self.hydro_config();
        let progress = watching(sink);
        let heightmap = self.heightmap.clone().expect("act one runs before act three");
        let precipitation = self
            .precipitation
            .clone()
            .expect("act two runs before act three");

        let pit_res = fill_depressions_and_find_lakes(
            &heightmap,
            relief::SEA_LEVEL,
            &hydro_config,
            &progress,
        );
        let flow_res = calculate_flow(
            &pit_res.filled_heightmap,
            &precipitation,
            relief::SEA_LEVEL,
            &hydro_config,
            &progress,
        );

        let lakes = pit_res.is_lake.raw_slice().iter().filter(|&&lake| lake).count();
        let is_lake = Arc::new(pit_res.is_lake);
        self.flow = Some(Arc::new(flow_res.accumulated_flow));
        self.is_lake = Some(is_lake.clone());

        // The cache is written *before* the polar ice is pinned into the
        // temperature grid, exactly as it always has been: the file holds the
        // climate the atmosphere simulated, and the ice belt is resolved again on
        // load. A cached world and a freshly grown one therefore agree to the
        // last bit — which is what makes `--load` a genuine shortcut rather than
        // a second, subtly different world.
        if self.autosave && let Err(why) = self.save_cache() {
            println!("⚠ The world cache could not be written: {why:?}");
        }

        // The belt is permanent arctic waste — no province may settle there, no
        // tundra may grow at its shore — so it is resolved here, at the end of
        // act three, and pinned into the temperature grid every later act reads.
        let polar_ice = polar::permanent_polar_ice(
            &heightmap,
            &is_lake,
            relief::SEA_LEVEL,
            self.seed,
        );
        let mut temperature = (*self
            .temperature
            .clone()
            .expect("act two runs before act three"))
        .clone();
        polar::freeze_polar_ice(&mut temperature, &polar_ice);

        self.temperature = Some(Arc::new(temperature));
        self.polar_ice = Some(Arc::new(polar_ice));

        vec![
            format!("{} lake pixels", thousands_u64(lakes as u64)),
            format!(
                "Rivers routed downhill · {:.0} min river flow",
                hydro_config.min_river_flow
            ),
        ]
    }

    /// Resolve the polar ice belt for a world adopted from disk.
    ///
    /// A loaded world skipped the tail of act three, so the pass is run here —
    /// on the very same grids, in the very same order — which is what makes a
    /// loaded world indistinguishable from one that grew from a seed.
    fn finish_polar_ice(&mut self) {
        let (Some(heightmap), Some(is_lake), Some(temperature)) = (
            self.heightmap.clone(),
            self.is_lake.clone(),
            self.temperature.clone(),
        ) else {
            return;
        };
        let polar_ice =
            polar::permanent_polar_ice(&heightmap, &is_lake, relief::SEA_LEVEL, self.seed);
        let mut frozen = (*temperature).clone();
        polar::freeze_polar_ice(&mut frozen, &polar_ice);

        self.temperature = Some(Arc::new(frozen));
        self.is_lake = Some(is_lake);
        self.polar_ice = Some(Arc::new(polar_ice));
    }
}

impl Genesis {
    // ─── 4. GEOGRAPHY & PROVINCES ────────────────────────────────────────────

    fn run_provinces(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let hydro_config = self.hydro_config();
        let geo_config = self.geography_config();
        let progress = watching(sink);
        let world = self.atlas().expect("act three runs before act four");
        let polar_ice = self.polar_ice.clone().expect("act three resolves the ice");

        let result = generate_provinces(
            &world.heightmap,
            world.temperature.as_deref().expect("act two runs before act four"),
            world.precipitation.as_deref().expect("act two runs before act four"),
            world.flow.as_deref().expect("act three runs before act four"),
            world.is_lake.as_deref().expect("act three runs before act four"),
            &polar_ice,
            // The planet's own vertical datum. The old 0.11 threshold is only the
            // *noise* water line.
            relief::SEA_LEVEL,
            hydro_config.min_river_flow,
            &geo_config,
            self.seed,
            &progress,
        );

        let wasteland = result.is_wasteland.raw_slice().iter().filter(|&&w| w).count();
        let summary = vec![
            format!("{} provinces", thousands_u64(result.num_provinces as u64)),
            format!("{} wasteland pixels", thousands_u64(wasteland as u64)),
        ];
        self.provinces = Some(Arc::new(result));
        summary
    }

    // ─── 5. GEOPOLITICAL HIERARCHY ───────────────────────────────────────────

    fn run_geopolitics(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let progress = watching(sink);
        let world = self.atlas().expect("act four runs before act five");
        let provinces = world.provinces.clone().expect("act four runs before act five");
        let is_lake = world.is_lake.clone().expect("act three runs before act five");

        let extracted = extract_province_graph(
            &provinces.province_ids,
            &world.heightmap,
            &is_lake,
            relief::SEA_LEVEL,
        );
        // The clustering passes never see a pixel, so the pixel grids they are
        // painted from travel with the progress handle.
        let progress = progress.with_map(MapRefs {
            heightmap: &world.heightmap,
            province_ids: &provinces.province_ids,
            ocean_ids: None,
        });
        let hierarchy = build_geopolitical_hierarchy(
            extracted.provinces,
            self.seed,
            world.width as f32,
            &progress,
        );

        let summary = vec![
            format!("{} continents", hierarchy.total_continents),
            format!("{} superregions", hierarchy.total_superregions),
            format!("{} regions", hierarchy.total_regions),
            format!("{} areas", hierarchy.total_areas),
        ];
        self.hierarchy = Some(Arc::new(hierarchy));
        summary
    }

    // ─── 6. HYDRO-POLITICAL HIERARCHY ────────────────────────────────────────

    fn run_hydro_politics(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let ocean_config = self.ocean_config();
        let world = self.atlas().expect("act four runs before act six");
        let is_lake = world.is_lake.clone().expect("act three runs before act six");
        let land = world.provinces.clone().expect("act four runs before act six");

        let result = generate_ocean_provinces(
            &world.heightmap,
            &is_lake,
            relief::SEA_LEVEL,
            &ocean_config,
            self.seed,
            &watching(sink),
        );
        let extracted = extract_ocean_graph(&result.ocean_province_ids);

        // Now that the marine provinces exist, the clustering that follows can be
        // painted: it is a hierarchy of *nodes* over a grid of ocean provinces.
        let progress = watching(sink).with_map(MapRefs {
            heightmap: &world.heightmap,
            province_ids: &land.province_ids,
            ocean_ids: Some(&result.ocean_province_ids),
        });
        let ocean_hierarchy = build_ocean_hierarchy(
            extracted.provinces,
            self.seed,
            world.width as f32,
            &ocean_config,
            &progress,
        );

        println!(
            "         Ocean: {} provinces → {} basins → {} superregions → {} regions → {} areas.",
            result.num_provinces,
            ocean_hierarchy.total_continents,
            ocean_hierarchy.total_superregions,
            ocean_hierarchy.total_regions,
            ocean_hierarchy.total_areas,
        );

        let summary = vec![
            format!("{} marine provinces", thousands_u64(result.num_provinces as u64)),
            format!("{} ocean basins", ocean_hierarchy.total_continents),
            format!("{} oceanic regions", ocean_hierarchy.total_regions),
        ];
        self.ocean = Some(Arc::new(result));
        self.ocean_hierarchy = Some(Arc::new(ocean_hierarchy));
        summary
    }

    // ─── 7. DAWN OF CIVILIZATION ─────────────────────────────────────────────

    fn run_civilization(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let hydro_config = self.hydro_config();
        let climate_config = self.climate_config();
        let civ_config = self.civilization_config();
        let atlas = self.atlas().expect("act six runs before act seven");
        let provinces = atlas.provinces.clone().expect("act four runs before act seven");
        let hierarchy = atlas.hierarchy.clone();
        let is_lake = atlas.is_lake.clone().expect("act three runs before act seven");
        let temperature = atlas.temperature.clone().expect("act two runs before act seven");
        let precipitation = atlas.precipitation.clone().expect("act two runs before act seven");
        let flow = atlas.flow.clone().expect("act three runs before act seven");

        let progress = watching(sink).with_map(MapRefs {
            heightmap: &atlas.heightmap,
            province_ids: &provinces.province_ids,
            ocean_ids: None,
        });
        let world = generate_civilizations(
            &atlas.heightmap,
            &temperature,
            &precipitation,
            &flow,
            &is_lake,
            &provinces.habitability,
            &provinces.province_ids,
            &hierarchy,
            relief::SEA_LEVEL,
            hydro_config.min_river_flow,
            climate_config.mode,
            &civ_config,
            self.seed,
            &progress,
        );

        let summary = vec![
            format!("{} realms", world.civilizations.len()),
            format!("{} civilized provinces", thousands_u64(world.civilized_provinces as u64)),
            format!("world population {}", thousands_u64(world.total_population)),
            format!("atlas date {}", world.map_year_label()),
        ];
        self.civilizations = Some(Arc::new(world));
        summary
    }
}

/// A progress handle for one act: silent when nobody is watching.
fn watching(sink: &Option<Arc<Sink>>) -> Progress<'static> {
    match sink {
        Some(sink) => Progress::watching(sink.clone()),
        None => Progress::silent(),
    }
}

/// The finished world's hypsometric profile, in metres.
///
/// The quickest sanity check that the vertical datum came out sane — how much of
/// the surface ended up as ocean, where the median shore and sea floor sit, how
/// much shelf there is, and what the extremes are. The atlas shows two of these
/// figures the moment act one lands, and the terminal prints all of them, for a
/// world grown from a seed and for one adopted from a cache alike: a world
/// generated under an older datum then announces itself rather than quietly
/// reporting a 400 m-deep ocean.
struct ReliefProfile {
    land_share: f32,
    median_altitude: f32,
    altitude_90: f32,
    peak: f32,
    median_depth: f32,
    shelf_share: f32,
    trench: f32,
}

fn relief_profile(heightmap: &Grid<f32>) -> ReliefProfile {
    const BIN_M: f32 = 100.0;
    const BINS: usize = 200; // the datum spans -11 000 m … +9 000 m

    let mut bins = vec![0u64; BINS];
    let (mut land, mut ocean) = (0u64, 0u64);
    let (mut peak, mut trench) = (f32::MIN, f32::MAX);

    for &elev in heightmap.raw_slice() {
        let metres = relief::to_metres(elev);
        if metres > 0.0 {
            land += 1;
            peak = peak.max(metres);
        } else {
            ocean += 1;
            trench = trench.min(metres);
        }
        let bin = (((metres + relief::DEEPEST_TRENCH_M) / BIN_M) as isize)
            .clamp(0, BINS as isize - 1) as usize;
        bins[bin] += 1;
    }

    let surface = (land + ocean).max(1) as f64;
    let (land_seen, ocean_seen) = (land.max(1) as f64, ocean.max(1) as f64);
    let (mut deep, mut high) = (0f64, 0f64);
    let (mut shelf, mut median_depth) = (0u64, 0f32);
    let (mut median_altitude, mut altitude_90) = (0f32, 0f32);
    let (mut have_depth, mut have_alt, mut have_alt90) = (false, false, false);

    for bin in 0..BINS {
        let metres = bin as f32 * BIN_M - relief::DEEPEST_TRENCH_M + BIN_M * 0.5;
        let count = bins[bin] as f64;
        if metres <= 0.0 {
            deep += count;
            if metres >= -200.0 {
                shelf += bins[bin];
            }
            if !have_depth && deep >= ocean_seen * 0.5 {
                median_depth = metres;
                have_depth = true;
            }
        } else {
            high += count;
            if !have_alt && high >= land_seen * 0.5 {
                median_altitude = metres;
                have_alt = true;
            }
            if !have_alt90 && high >= land_seen * 0.9 {
                altitude_90 = metres;
                have_alt90 = true;
            }
        }
    }

    ReliefProfile {
        land_share: (land as f64 / surface) as f32,
        median_altitude,
        altitude_90,
        peak,
        median_depth,
        shelf_share: (shelf as f64 / ocean_seen) as f32,
        trench,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asking for a world that was never saved is not a crash: it is a sentence
    /// the atlas can show, and the pipeline it was asked of is left untouched —
    /// so the reader can simply grow the world instead.
    #[test]
    fn loading_a_world_that_was_never_saved_says_so() {
        // Nothing is saved for this seed, and nothing ever will be.
        let mut genesis = Genesis::new(u64::MAX);
        match genesis.load_cache() {
            Err(LoadError::Missing(path)) => {
                let path = path.to_string_lossy().to_string();
                assert!(path.starts_with(CACHE_DIR), "{path} lives in the cache directory");
                assert!(path.contains("world_"), "{path} is named after its seed");
            }
            _ => panic!("a world that was never saved must be reported as missing"),
        }
        assert_eq!(
            genesis.ready(),
            Some(Stage::Geology),
            "a load that found nothing leaves the pipeline exactly where it was"
        );
        assert!(genesis.atlas().is_none(), "and there is still no world to paint");
    }

    /// An unfinished world has nothing worth writing: the cache holds the first
    /// three acts, so a world of one act has no cache to save.
    #[test]
    fn a_world_cannot_be_saved_before_it_has_weather_and_water() {
        let genesis = Genesis::new(7);
        assert!(
            genesis.save_cache().is_err(),
            "there is no world yet, so there is nothing to write"
        );
    }

    /// A cache is named after the seed it holds, and lives with its siblings.
    #[test]
    fn a_worlds_cache_is_named_after_its_seed() {
        assert_eq!(
            cache_path(1_980).to_string_lossy(),
            format!("{CACHE_DIR}/world_1980.bin")
        );
        assert_ne!(cache_path(1).to_string_lossy(), cache_path(2).to_string_lossy());
    }

    /// A world adopted from disk is a world of three acts, whatever the seed.
    #[test]
    fn a_world_of_three_acts_offers_the_fourth() {
        let genesis = Genesis::for_test(5, Stage::Hydrology);
        assert_eq!(genesis.ready(), Some(Stage::Provinces));
        assert_eq!(genesis.state(Stage::Hydrology), StageState::Done { secs: 0.0 });
        assert_eq!(genesis.state(Stage::Provinces), StageState::Ready);
        assert_eq!(genesis.state(Stage::Geopolitics), StageState::Locked);
        let world = genesis.atlas().expect("three acts are a world");
        assert_eq!(world.stage, Stage::Hydrology);
        assert!(world.temperature.is_some() && world.is_lake.is_some());
        assert!(world.provinces.is_none(), "act four has not run");
        assert_eq!(world.hierarchy.total_provinces, 0, "and neither has act five");
    }
}

/// Print the finished world's hypsometric profile, in metres.
fn report_relief(profile: &ReliefProfile) {
    println!("── Planetary relief (vertical datum applied) ──────────");
    println!(
        "   Surface : {:.1}% land / {:.1}% ocean",
        100.0 * profile.land_share,
        100.0 * (1.0 - profile.land_share)
    );
    println!(
        "   Land    : median +{:.0} m | 90% below +{:.0} m | highest peak +{:.0} m",
        profile.median_altitude, profile.altitude_90, profile.peak
    );
    println!(
        "   Ocean   : median {:.0} m | {:.1}% continental shelf (<= -200 m) | deepest trench {:.0} m",
        profile.median_depth,
        100.0 * profile.shelf_share,
        profile.trench
    );
}

/// Mean temperature and mean rainfall over the whole world.
fn climate_means(temperature: &Grid<f32>, precipitation: &Grid<f32>) -> (f32, f32) {
    let count = temperature.raw_slice().len().max(1) as f32;
    let temp = temperature.raw_slice().iter().sum::<f32>() / count;
    let rain = precipitation.raw_slice().iter().sum::<f32>() / count;
    (temp, rain)
}
