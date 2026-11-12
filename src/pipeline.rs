//! The eight acts of creation, as a state the atlas can drive.
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
//!   8 Languages & Phonetics      phonologies, lexicons, Urheimats, native names
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
//!   *when* each act runs, never *what* it computes. The seed is read by each act
//!   as it runs, so it may be re-pointed between acts without disturbing the acts
//!   already grown; a world grown from one seed throughout is still exactly that
//!   seed's planet.

use crate::civilization::{CivilizationConfig, CivilizationWorld, generate_civilizations};
use crate::climate::{ClimateConfig, simulate_moisture, simulate_temperature, simulate_winds};
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::progress::{MapRefs, Progress, Sink};
use crate::core::{Grid, WorldCache, WorldCacheView, cache_stage, polar, relief};
use crate::geography::{
    GeographyConfig, OceanConfig, OceanResult, ProvinceResult, generate_ocean_provinces,
    generate_provinces,
};
use crate::geopolitics::{
    GeopoliticalHierarchy, build_geopolitical_hierarchy, build_ocean_hierarchy,
    extract_ocean_graph, extract_province_graph,
};
use crate::hydrology::{HydrologyConfig, calculate_flow, fill_depressions_and_find_lakes};
use crate::language::{LanguageConfig, LanguagesWorld, generate_languages};
use crate::terrain::{
    WorldConfig, apply_fractal_detail, form_sea_straits, simulate_hydraulic_erosion,
    simulate_tectonics, simulate_thermal_erosion,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

/// Canvas the engine grows: 8K. Fixed, because a world's resolution is part of
/// its identity — a seed must always mean exactly one planet.
pub const WORLD_WIDTH: usize = 8192;
pub const WORLD_HEIGHT: usize = 4096;

/// Directory a world's cache is written to.
pub const CACHE_DIR: &str = "./saved_seeds";

/// The eight acts of creation, in the only order they can be run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Geology,
    Climate,
    Hydrology,
    Provinces,
    Geopolitics,
    HydroPolitics,
    Civilization,
    Languages,
}

impl Stage {
    /// Every act, in order. The atlas lays its buttons out from this.
    pub const ALL: [Stage; 8] = [
        Stage::Geology,
        Stage::Climate,
        Stage::Hydrology,
        Stage::Provinces,
        Stage::Geopolitics,
        Stage::HydroPolitics,
        Stage::Civilization,
        Stage::Languages,
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
            Stage::Languages => 8,
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
            Stage::Languages => "Languages & Phonetics",
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
            Stage::Geopolitics => {
                "Provinces gathered into areas, regions, superregions and continents."
            }
            Stage::HydroPolitics => "The world ocean partitioned into its own five matching tiers.",
            Stage::Civilization => {
                "Realms rise, spread and are given a history, an age and a people."
            }
            Stage::Languages => {
                "Phonologies, proto-lexicons, Urheimats and the tongues that spread from them."
            }
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
/// world eight acts old are both paintable. The fields whose act has not been
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
    /// The world's languages — act eight onward.
    pub languages: Arc<LanguagesWorld>,
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
    /// Act two's climate, and the artefact that is never mutated: the belt of
    /// polar ice is pinned into a *copy* of it (see [`Genesis::frozen_temperature`])
    /// so that rewinding to act three can start again from the honest climate.
    temperature: Option<Arc<Grid<f32>>>,
    /// Act three's temperature: [`Genesis::temperature`] with the permanent polar
    /// ice pinned in. Acts four onward read this one, because they decide where
    /// ice begins by reading temperature.
    frozen_temperature: Option<Arc<Grid<f32>>>,
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
    /// Act seven's realms, before act eight gives them native names.
    civilizations: Option<Arc<CivilizationWorld>>,
    /// Act eight's realms: the same societies, republished under the names their
    /// own tongues gave them. Kept apart from [`Genesis::civilizations`] so that
    /// rewinding to act eight restores the unnamed dawn rather than losing it.
    named_civilizations: Option<Arc<CivilizationWorld>>,
    languages: Option<Arc<LanguagesWorld>>,
    /// How long each act took, so the atlas can show it done.
    timings: [Option<f32>; 8],
    /// Write the world cache (every act grown so far, up to act six) whenever act
    /// three and again when act six land, overwriting the earlier save.
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
            frozen_temperature: None,
            precipitation: None,
            flow: None,
            is_lake: None,
            polar_ice: None,
            provinces: None,
            hierarchy: None,
            ocean: None,
            ocean_hierarchy: None,
            civilizations: None,
            named_civilizations: None,
            languages: None,
            timings: [None; 8],
            autosave: false,
        }
    }

    /// The seed this world grows from.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The deepest act behind this world, or `None` before act one.
    pub fn stage(&self) -> Option<Stage> {
        self.stage
    }

    /// Re-point the acts still to run at a new seed, keeping every act already grown.
    ///
    /// The seed is not part of a finished act: it is read *when* an act runs —
    /// `world_config`, `climate_config`, the hierarchy clustering and the
    /// civilization call all take it — so changing it leaves the acts behind the
    /// change exactly as they are, and only the acts ahead grow from the new seed.
    /// A world grown from one seed without changing it in between is still exactly
    /// that seed's planet.
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    /// Whether act three and act six write the world cache when they finish.
    pub fn set_autosave(&mut self, on: bool) {
        self.autosave = on;
    }

    /// Whether act three or act six would write the world cache right now.
    pub fn autosave(&self) -> bool {
        self.autosave
    }

    /// Throw away every act from `stage` onward, so that it — and all the acts
    /// that followed it — may be grown again.
    ///
    /// This is cheap: each act's work is an `Arc`, and dropping one is a refcount
    /// decrement, not a recomputation. Nothing is regenerated here; the acts are
    /// simply un-done, and the act named becomes the next one the panel offers.
    ///
    /// The seed is left alone. Because it is read by each act as it runs, a seed
    /// typed *after* a rewind re-points only the acts from the rewind point on —
    /// exactly the rule that governs changing it between acts at any other time.
    ///
    /// Two artefacts are deliberately kept apart from their act so that a rewind
    /// can restore them honestly: [`Genesis::temperature`] (act two's climate,
    /// which act three would otherwise overwrite with its iced copy) and
    /// [`Genesis::civilizations`] (act seven's unnamed realms, which act eight
    /// would otherwise overwrite with their named copy).
    pub fn rewind_to(&mut self, stage: Stage) {
        if stage.index() <= 1 {
            // Back to before act one: nothing is left of the world but its seed
            // and what the reader chose about saving it.
            let (seed, autosave) = (self.seed, self.autosave);
            *self = Self::new(seed);
            self.autosave = autosave;
            return;
        }

        // Drop exactly the acts at or after `stage`, and mark them un-run.
        for act in Stage::ALL {
            if act.index() < stage.index() {
                continue;
            }
            match act {
                Stage::Geology => {}
                Stage::Climate => {
                    self.temperature = None;
                    self.frozen_temperature = None;
                    self.precipitation = None;
                }
                Stage::Hydrology => {
                    self.frozen_temperature = None;
                    self.flow = None;
                    self.is_lake = None;
                    self.polar_ice = None;
                }
                Stage::Provinces => self.provinces = None,
                Stage::Geopolitics => self.hierarchy = None,
                Stage::HydroPolitics => {
                    self.ocean = None;
                    self.ocean_hierarchy = None;
                }
                Stage::Civilization => self.civilizations = None,
                Stage::Languages => {
                    self.named_civilizations = None;
                    self.languages = None;
                }
            }
            self.timings[act.index() - 1] = None;
        }
        self.stage = Stage::ALL.get(stage.index() - 2).copied();
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
        ClimateConfig {
            seed: self.seed,
            ..Default::default()
        }
    }

    pub fn hydro_config(&self) -> HydrologyConfig {
        HydrologyConfig {
            min_lake_depth: 0.012,
            min_river_flow: 100.0,      // major rivers and their larger branches — not the capillary web
            major_river_flow: 3_000.0,  // the Nile: a continental trunk drawn at full width
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

    pub fn language_config(&self) -> LanguageConfig {
        LanguageConfig::default()
    }

    // ─── THE WORLD CACHE ─────────────────────────────────────────────────────

    /// Where this world's cache lives.
    pub fn cache_path(&self) -> PathBuf {
        cache_path(self.seed)
    }

    /// Write the acts grown so far to disk, as `world_<seed>.bin`, overwriting any
    /// earlier save for this seed.
    ///
    /// The save holds acts one through whichever act the world has reached, capped
    /// at act six: the bare land, the climate, the water, and as far up the two
    /// hierarchies as the world has climbed. Acts seven and eight are never
    /// written, so a saved world always resumes at the dawn or before it. A world
    /// with no act behind it has nothing worth keeping and is not written.
    pub fn save_cache(&self) -> Result<PathBuf, LoadError> {
        let path = self.cache_path();
        let Some(heightmap) = &self.heightmap else {
            // No act has run, so there is no world yet.
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
                temp_grid: self.temperature.as_deref(),
                precip_grid: self.precipitation.as_deref(),
                flow_grid: self.flow.as_deref(),
                is_lake: self.is_lake.as_deref(),
                provinces: self.provinces.as_deref(),
                hierarchy: self.hierarchy.as_deref(),
                ocean: self.ocean.as_deref(),
                ocean_hierarchy: self.ocean_hierarchy.as_deref(),
            },
            &path.to_string_lossy(),
        )
        .map_err(|why| LoadError::Unreadable(path.clone(), why))?;
        Ok(path)
    }

    /// Adopt the acts already saved for this seed, from `world_<seed>.bin`.
    ///
    /// The save carries acts one through whichever act the world had reached when
    /// it was written, so the pipeline picks up at the act after that one — act
    /// four for a save made at act three, act seven for one made at act six. On
    /// success it holds exactly that much of the world, and no more.
    pub fn load_cache(&mut self) -> Result<(), LoadError> {
        let path = self.cache_path();
        if !Path::new(&path).exists() {
            return Err(LoadError::Missing(path));
        }
        let start = Instant::now();
        println!("Loading world from cache '{}'…", path.display());
        let cache = WorldCache::try_load(&path.to_string_lossy())
            .map_err(|why| LoadError::Unreadable(path.clone(), why))?;

        let stage = Stage::ALL[(cache.stage() - 1) as usize];
        self.width = cache.width;
        self.height = cache.height;
        self.heightmap = Some(Arc::new(cache.heightmap));
        self.temperature = cache.temp_grid.map(Arc::new);
        self.precipitation = cache.precip_grid.map(Arc::new);
        self.flow = cache.flow_grid.map(Arc::new);
        self.is_lake = cache.is_lake.map(Arc::new);
        self.provinces = cache.provinces.map(Arc::new);
        self.hierarchy = cache.hierarchy.map(Arc::new);
        self.ocean = cache.ocean.map(Arc::new);
        self.ocean_hierarchy = cache.ocean_hierarchy.map(Arc::new);
        // The ice belt is not stored: it is resolved again here, from the same
        // grids and the same seed, exactly as act three resolves it. It is a no-op
        // for a save made before act three, whose lakes are not there yet.
        self.finish_polar_ice();
        self.stage = Some(stage);
        // Every act the save carries happened in another session, so each reports
        // `Done` with the zero that marks an act adopted from disk rather than run
        // just now.
        for act in Stage::ALL {
            self.timings[act.index() - 1] = if act.index() <= stage.index() {
                Some(0.0)
            } else {
                None
            };
        }
        println!(
            "✓ Loaded cached world data in:             {:.2?}",
            start.elapsed()
        );
        Ok(())
    }
}

/// Where a world's cache lives.
pub fn cache_path(seed: u64) -> PathBuf {
    PathBuf::from(format!("{CACHE_DIR}/world_{seed}.bin"))
}

/// One world cache found on disk, as the atlas lists it.
///
/// This is metadata only: the atlas reads a cache's *header* — never its payload —
/// to describe it. The file itself is loaded only when the reader asks to load it.
///
/// A seed carries a single cache: `world_<seed>.bin`, holding every act it has
/// grown. The `bytes` and `modified` describe that one file, which is what a Load
/// would adopt.
#[derive(Clone, Debug)]
pub struct CachedWorld {
    /// The seed the cache is named after.
    pub seed: u64,
    /// Size of the cache, in bytes.
    pub bytes: u64,
    /// When the cache was last written, when the platform says.
    pub modified: Option<SystemTime>,
    /// The deepest act the cache holds, when its header says. `None` for a cache
    /// written before the header existed, or one that will not open — the act
    /// would then be knowable only by decoding the payload, which the list does
    /// not pay for.
    pub acts: Option<u8>,
}

/// The cache for one seed, if any checkpoint for it is on disk.
///
/// A handful of metadata calls, so the panel may ask on every frame whether the
/// seed it is showing can be loaded.
pub fn cached_world(seed: u64) -> Option<CachedWorld> {
    describe_cached_world(Path::new(CACHE_DIR), seed)
}

/// Describe one seed's cache as it sits in `dir`, or `None` if it is not there.
///
/// Metadata only, so the per-frame [`cached_world`] costs nothing: the act the
/// save holds is left `None` here and filled by [`cached_worlds_in`], which reads
/// a cache's header once per scan rather than once per frame.
fn describe_cached_world(dir: &Path, seed: u64) -> Option<CachedWorld> {
    let file = std::fs::metadata(dir.join(format!("world_{seed}.bin")))
        .ok()
        .filter(std::fs::Metadata::is_file)?;
    Some(CachedWorld {
        seed,
        bytes: file.len(),
        modified: file.modified().ok(),
        acts: None,
    })
}

/// Every world cache in the cache directory, newest first.
pub fn cached_worlds() -> Vec<CachedWorld> {
    cached_worlds_in(Path::new(CACHE_DIR))
}

/// Every world cache in `dir`, newest first.
///
/// A world is named by one canonical spelling: `world_<seed>.bin`. A stray file, a
/// `.lang.bin` left over from an older build, or a cache spelled `world_0042.bin`
/// names nothing the atlas can load, so it is not listed. An unreadable directory
/// is an empty list, never a panic.
pub fn cached_worlds_in(dir: &Path) -> Vec<CachedWorld> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut seeds: Vec<u64> = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(rest) = name.strip_prefix("world_") else {
            continue;
        };
        let Some(digits) = rest.strip_suffix(".bin") else {
            continue;
        };
        let Ok(seed) = digits.parse::<u64>() else {
            continue;
        };
        if name != format!("world_{seed}.bin") {
            continue;
        }
        if !seeds.contains(&seed) {
            seeds.push(seed);
        }
    }
    let mut worlds: Vec<CachedWorld> = seeds
        .into_iter()
        .filter_map(|seed| describe_cached_world(dir, seed))
        .collect();
    // The act cannot be told from metadata alone, so each listed file's header is
    // read here for it. This runs once per scan, not once per frame, so a list of
    // many worlds costs a handful of bytes each — never a decoded payload.
    for world in &mut worlds {
        let path = dir.join(format!("world_{}.bin", world.seed));
        world.acts = cache_stage(&path.to_string_lossy());
    }
    worlds.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.seed.cmp(&a.seed)));
    worlds
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
            Stage::Languages => self.run_languages(&sink),
        };
        let secs = start.elapsed().as_secs_f32();
        self.timings[stage.index() - 1] = Some(secs);
        self.stage = Some(stage);
        println!(
            "✓ [{}/8] {} completed: {:.2?}",
            stage.index(),
            stage.title(),
            start.elapsed()
        );
        Some(StageReport {
            stage,
            secs,
            summary,
        })
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
            // Act three's iced copy of the climate, so a test can watch a rewind
            // drop it while act two's honest grid stays.
            genesis.frozen_temperature =
                Some(Arc::new(Grid::filled_with(width, height, -30.0f32)));
        }
        if stage.available(Stage::Provinces) {
            genesis.provinces = Some(Arc::new(ProvinceResult {
                province_ids: Grid::filled_with(width, height, 1u32),
                habitability: Grid::filled_with(width, height, 0.5f32),
                is_wasteland: Grid::filled_with(width, height, false),
                num_provinces: 1,
            }));
        }
        if stage.available(Stage::Geopolitics) {
            genesis.hierarchy = Some(Arc::new(GeopoliticalHierarchy::empty()));
        }
        if stage.available(Stage::HydroPolitics) {
            genesis.ocean = Some(Arc::new(OceanResult {
                ocean_province_ids: Grid::filled_with(width, height, 0u32),
                num_provinces: 0,
            }));
            genesis.ocean_hierarchy = Some(Arc::new(GeopoliticalHierarchy::empty()));
        }
        if stage.available(Stage::Civilization) {
            genesis.civilizations = Some(Arc::new(CivilizationWorld::empty(seed)));
        }
        if stage.available(Stage::Languages) {
            genesis.named_civilizations = Some(Arc::new(CivilizationWorld::empty(seed)));
            genesis.languages = Some(Arc::new(LanguagesWorld::empty(seed)));
        }
        for act in Stage::ALL {
            // Acts already behind a world adopted from disk were not timed here,
            // which is what the panel reports as `loaded`.
            genesis.timings[act.index() - 1] = if act.index() <= stage.index() {
                Some(0.0)
            } else {
                None
            };
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
            // Acts four onward read the iced climate; a world younger than act
            // three has only act two's, which is the honest thing to paint.
            temperature: self
                .frozen_temperature
                .clone()
                .or_else(|| self.temperature.clone()),
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
            // Once act eight has run, the named realms are the ones to paint; a
            // world that has only reached act seven shows them unnamed.
            civilizations: self
                .named_civilizations
                .clone()
                .or_else(|| self.civilizations.clone())
                .unwrap_or_else(|| Arc::new(CivilizationWorld::empty(self.seed))),
            languages: self
                .languages
                .clone()
                .unwrap_or_else(|| Arc::new(LanguagesWorld::empty(self.seed))),
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

        // The terrain is finished. Any basin walled off from the world ocean
        // that is too large to be a lake is now given its Gibraltar, while the
        // surface is still act one's to change: act two's climate then reads the
        // sea, and act three's flood finds it already joined to the ocean.
        let straits = form_sea_straits(
            &mut result.heightmap,
            relief::SEA_LEVEL,
            &world_config,
            &progress,
        );

        let profile = relief_profile(&result.heightmap);
        report_relief(&profile);
        self.heightmap = Some(Arc::new(result.heightmap));

        let mut summary = vec![
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
        ];
        if !straits.straits.is_empty() {
            let channel: usize = straits
                .straits
                .iter()
                .map(|strait| strait.channel_cells)
                .sum();
            summary.push(format!(
                "Opened {} landlocked sea{} · {} px of strait",
                straits.straits.len(),
                if straits.straits.len() == 1 { "" } else { "s" },
                thousands_u64(channel as u64),
            ));
        }
        summary
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
        let heightmap = self
            .heightmap
            .clone()
            .expect("act one runs before act three");
        let precipitation = self
            .precipitation
            .clone()
            .expect("act two runs before act three");
        let temperature = self
            .temperature
            .clone()
            .expect("act two runs before act three");

        let pit_res = fill_depressions_and_find_lakes(
            &heightmap,
            relief::SEA_LEVEL,
            &hydro_config,
            &progress,
        );
        let flow_res = calculate_flow(
            &pit_res.routing_heightmap,
            &precipitation,
            &temperature,
            relief::SEA_LEVEL,
            &hydro_config,
            &progress,
        );

        let lakes = pit_res
            .is_lake
            .raw_slice()
            .iter()
            .filter(|&&lake| lake)
            .count();
        let is_lake = Arc::new(pit_res.is_lake);
        self.flow = Some(Arc::new(flow_res.accumulated_flow));
        self.is_lake = Some(is_lake.clone());

        // The cache is written *before* the polar ice is pinned into the
        // temperature grid, exactly as it always has been: the file holds the
        // climate the atmosphere simulated, and the ice belt is resolved again on
        // load. A cached world and a freshly grown one therefore agree to the
        // last bit — which is what makes `--load` a genuine shortcut rather than
        // a second, subtly different world.
        if self.autosave
            && let Err(why) = self.save_cache()
        {
            println!("⚠ The world cache could not be written: {why:?}");
        }

        // The belt is permanent arctic waste — no province may settle there, no
        // tundra may grow at its shore — so it is resolved here, at the end of
        // act three, and pinned into the temperature grid every later act reads.
        //
        // It is pinned into a *copy*: act two's climate stays untouched, so a
        // rewind to act three re-freezes from the same honest grid rather than
        // layering a second belt over the first.
        let polar_ice =
            polar::permanent_polar_ice(&heightmap, &is_lake, relief::SEA_LEVEL, self.seed);
        let mut frozen = (*self
            .temperature
            .clone()
            .expect("act two runs before act three"))
        .clone();
        polar::freeze_polar_ice(&mut frozen, &polar_ice);

        self.frozen_temperature = Some(Arc::new(frozen));
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

        // The iced copy is act three's own artefact. Act two's climate is left
        // exactly as the atmosphere simulated it, so that rewinding to act three
        // can freeze the belt again from the honest grid.
        self.frozen_temperature = Some(Arc::new(frozen));
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
            world
                .temperature
                .as_deref()
                .expect("act two runs before act four"),
            world
                .precipitation
                .as_deref()
                .expect("act two runs before act four"),
            world
                .flow
                .as_deref()
                .expect("act three runs before act four"),
            world
                .is_lake
                .as_deref()
                .expect("act three runs before act four"),
            &polar_ice,
            // The planet's own vertical datum. The old 0.11 threshold is only the
            // *noise* water line.
            relief::SEA_LEVEL,
            hydro_config.min_river_flow,
            &geo_config,
            self.seed,
            &progress,
        );

        let wasteland = result
            .is_wasteland
            .raw_slice()
            .iter()
            .filter(|&&w| w)
            .count();
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
        let provinces = world
            .provinces
            .clone()
            .expect("act four runs before act five");
        let is_lake = world
            .is_lake
            .clone()
            .expect("act three runs before act five");

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
        let is_lake = world
            .is_lake
            .clone()
            .expect("act three runs before act six");
        let land = world
            .provinces
            .clone()
            .expect("act four runs before act six");

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
            format!(
                "{} marine provinces",
                thousands_u64(result.num_provinces as u64)
            ),
            format!("{} ocean basins", ocean_hierarchy.total_continents),
            format!("{} oceanic regions", ocean_hierarchy.total_regions),
        ];
        self.ocean = Some(Arc::new(result));
        self.ocean_hierarchy = Some(Arc::new(ocean_hierarchy));

        // Everything the world is before history is added to it, written the
        // moment act six lands — overwriting the save act three made, so the file
        // now holds acts one to six.
        if self.autosave
            && let Err(why) = self.save_cache()
        {
            println!("⚠ The world cache could not be written: {why:?}");
        }
        summary
    }

    // ─── 7. DAWN OF CIVILIZATION ─────────────────────────────────────────────

    fn run_civilization(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let hydro_config = self.hydro_config();
        let climate_config = self.climate_config();
        let civ_config = self.civilization_config();
        let atlas = self.atlas().expect("act six runs before act seven");
        let provinces = atlas
            .provinces
            .clone()
            .expect("act four runs before act seven");
        let hierarchy = atlas.hierarchy.clone();
        let is_lake = atlas
            .is_lake
            .clone()
            .expect("act three runs before act seven");
        let temperature = atlas
            .temperature
            .clone()
            .expect("act two runs before act seven");
        let precipitation = atlas
            .precipitation
            .clone()
            .expect("act two runs before act seven");
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
            format!(
                "{} civilized provinces",
                thousands_u64(world.civilized_provinces as u64)
            ),
            format!("world population {}", thousands_u64(world.total_population)),
            format!("atlas date {}", world.map_year_label()),
        ];
        self.civilizations = Some(Arc::new(world));
        summary
    }

    // ─── 8. LANGUAGES & PHONETICS ────────────────────────────────────────────

    fn run_languages(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let config = self.language_config();
        let atlas = self.atlas().expect("act seven runs before act eight");
        let provinces = atlas
            .provinces
            .clone()
            .expect("act four runs before act eight");

        let progress = watching(sink).with_map(MapRefs {
            heightmap: &atlas.heightmap,
            province_ids: &provinces.province_ids,
            ocean_ids: None,
        });
        let (world, named_civilizations) =
            generate_languages(&atlas, &config, self.seed, &progress);

        let summary = vec![
            format!("{} language families", world.families.len()),
            format!(
                "{} languages across {} provinces",
                world.languages.len(),
                world.total_provinces()
            ),
            format!("{} realms named", named_civilizations.civilizations.len()),
        ];
        // The named realms are act eight's own artefact, kept beside act seven's
        // unnamed ones so that a rewind to act eight restores the dawn rather
        // than a world that is already wearing its names.
        self.named_civilizations = Some(Arc::new(named_civilizations));
        self.languages = Some(Arc::new(world));
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
                assert!(
                    path.starts_with(CACHE_DIR),
                    "{path} lives in the cache directory"
                );
                assert!(path.contains("world_"), "{path} is named after its seed");
            }
            _ => panic!("a world that was never saved must be reported as missing"),
        }
        assert_eq!(
            genesis.ready(),
            Some(Stage::Geology),
            "a load that found nothing leaves the pipeline exactly where it was"
        );
        assert!(
            genesis.atlas().is_none(),
            "and there is still no world to paint"
        );
    }

    /// A world with no act behind it has nothing worth writing.
    #[test]
    fn a_world_cannot_be_saved_before_any_act_has_run() {
        let genesis = Genesis::new(7);
        assert!(
            genesis.save_cache().is_err(),
            "there is no world yet, so there is nothing to write"
        );
    }

    /// A save holds every act the world has grown, capped at six, so a world of
    /// three acts writes acts one to three and resumes at act four.
    #[test]
    fn a_partial_world_round_trips_through_its_cache() {
        // A synthetic seed that no real cache will ever be written for, so the
        // test leaves the cache directory as it found it. Not `u64::MAX`: the
        // polar-ice pass salts the seed into a `u32`, and the all-ones low word
        // would overflow that addition.
        let seed = 0x0BAD_C0DF;
        let genesis = Genesis::for_test(seed, Stage::Hydrology);
        let path = genesis.cache_path();
        let _ = std::fs::remove_file(&path);
        genesis.save_cache().expect("write the world");

        let mut adopted = Genesis::new(seed);
        adopted.load_cache().expect("read the world back");
        let _ = std::fs::remove_file(&path);

        assert_eq!(adopted.ready(), Some(Stage::Provinces));
        assert_eq!(
            adopted.state(Stage::Hydrology),
            StageState::Done { secs: 0.0 },
            "acts one to three are adopted from disk"
        );
        let world = adopted.atlas().expect("three acts are a world");
        assert_eq!(world.stage, Stage::Hydrology);
        assert!(world.flow.is_some() && world.is_lake.is_some());
        assert!(
            world.provinces.is_none() && world.ocean.is_none(),
            "acts four to six were never saved, so they are not there"
        );
    }

    /// A full world written to disk is the same world when it is read back, and
    /// the pipeline picks up at act seven rather than re-running acts one to six.
    #[test]
    fn a_full_world_round_trips_through_its_cache() {
        let seed = 0x0BAD_C0DE;
        let genesis = Genesis::for_test(seed, Stage::HydroPolitics);
        let path = genesis.cache_path();
        let _ = std::fs::remove_file(&path);
        genesis.save_cache().expect("write the world");

        let mut adopted = Genesis::new(seed);
        adopted.load_cache().expect("read the world back");
        let _ = std::fs::remove_file(&path);

        assert_eq!(adopted.ready(), Some(Stage::Civilization));
        assert_eq!(
            adopted.state(Stage::HydroPolitics),
            StageState::Done { secs: 0.0 },
            "acts one to six are adopted from disk"
        );
        let world = adopted.atlas().expect("six acts are a world");
        assert_eq!(world.stage, Stage::HydroPolitics);
        assert!(world.flow.is_some() && world.is_lake.is_some());
        assert!(
            world.provinces.is_some() && world.ocean.is_some(),
            "the provinces and the ocean came back with it"
        );
    }

    /// A save holds exactly the acts the world had grown, and a load resumes at
    /// the act after the last of them — for every act from one to six.
    #[test]
    fn a_save_resumes_at_the_act_after_the_one_it_holds() {
        let seed = 0x0BAD_C0D1;
        let path = cache_path(seed);
        let _ = std::fs::remove_file(&path);
        for (index, stage) in Stage::ALL[..6].iter().enumerate() {
            let genesis = Genesis::for_test(seed, *stage);
            genesis.save_cache().expect("write the world");

            let mut adopted = Genesis::new(seed);
            adopted.load_cache().expect("read the world back");

            let next = Stage::ALL.get(index + 1).copied();
            assert_eq!(
                adopted.ready(),
                next,
                "a save at {stage:?} resumes at {next:?}"
            );
            assert_eq!(adopted.stage(), Some(*stage), "and holds exactly {stage:?}");
        }
        let _ = std::fs::remove_file(&path);
    }

    /// A cache is named after the seed it holds, and lives with its siblings.
    #[test]
    fn a_worlds_cache_is_named_after_its_seed() {
        assert_eq!(
            cache_path(1_980).to_string_lossy(),
            format!("{CACHE_DIR}/world_1980.bin")
        );
        assert_ne!(
            cache_path(1).to_string_lossy(),
            cache_path(2).to_string_lossy()
        );
    }

    /// The atlas lists what is actually on disk: one entry per seed that has a
    /// cache, newest first.
    #[test]
    fn a_cache_directory_lists_its_worlds_newest_first() {
        let dir =
            std::env::temp_dir().join(format!("project-genesis-cache-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch cache directory");

        let write = |name: &str, bytes: &[u8]| {
            std::fs::write(dir.join(name), bytes).expect("a scratch cache file");
        };
        // Two worlds, and a retired `world_<seed>.full.bin` beside one of them.
        write("world_42.bin", b"cache");
        write("world_42.full.bin", b"retired");
        write("world_7.bin", b"fuller");
        // Not worlds: a stray file, a `.lang.bin` from an older build, and a
        // non-canonical spelling of a real seed.
        write("notes.txt", b"x");
        write("world_9.lang.bin", b"x");
        write("world_0042.bin", b"x");

        // Pin the timestamps, so "newest first" is a fact of the test rather
        // than of how fast the files happened to be written.
        let stamp = |name: &str, secs: u64| {
            let file = std::fs::File::options()
                .write(true)
                .open(dir.join(name))
                .expect("the scratch file");
            file.set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)),
            )
            .expect("a writable timestamp");
        };
        stamp("world_42.bin", 1_000);
        stamp("world_7.bin", 2_000);

        let worlds = cached_worlds_in(&dir);
        let seeds: Vec<u64> = worlds.iter().map(|world| world.seed).collect();
        assert_eq!(seeds, vec![7, 42], "the newest cache is listed first");

        let seven = worlds
            .iter()
            .find(|world| world.seed == 7)
            .expect("seed 7 is listed");
        assert_eq!(seven.bytes, 6, "the size is the file's own");

        let forty_two = worlds
            .iter()
            .find(|world| world.seed == 42)
            .expect("seed 42 is listed");
        assert_eq!(
            forty_two.bytes, 5,
            "a retired `.full.bin` beside it is not the world's size"
        );
        assert_eq!(
            forty_two.modified,
            Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
        );
        assert_eq!(
            (seven.acts, forty_two.acts),
            (None, None),
            "a scratch file with no acts header names no acts"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The list says how far each save was grown, read from the cache's header —
    /// one small read per file, never a decoded payload.
    #[test]
    fn a_listed_world_says_which_acts_it_holds() {
        // A synthetic seed no real cache is ever written for.
        let seed = 0x0BAD_C0F0;
        let path = cache_path(seed);
        let _ = std::fs::remove_file(&path);
        Genesis::for_test(seed, Stage::Geopolitics)
            .save_cache()
            .expect("write the world");

        let worlds = cached_worlds_in(Path::new(CACHE_DIR));
        let world = worlds
            .iter()
            .find(|world| world.seed == seed)
            .expect("the save is listed");
        assert_eq!(
            world.acts,
            Some(5),
            "a world grown to act five holds acts one to five"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// A world adopted from disk is a world of three acts, whatever the seed.
    #[test]
    fn a_world_of_three_acts_offers_the_fourth() {
        let genesis = Genesis::for_test(5, Stage::Hydrology);
        assert_eq!(genesis.ready(), Some(Stage::Provinces));
        assert_eq!(
            genesis.state(Stage::Hydrology),
            StageState::Done { secs: 0.0 }
        );
        assert_eq!(genesis.state(Stage::Provinces), StageState::Ready);
        assert_eq!(genesis.state(Stage::Geopolitics), StageState::Locked);
        let world = genesis.atlas().expect("three acts are a world");
        assert_eq!(world.stage, Stage::Hydrology);
        assert!(world.temperature.is_some() && world.is_lake.is_some());
        assert!(world.provinces.is_none(), "act four has not run");
        assert_eq!(
            world.hierarchy.total_provinces, 0,
            "and neither has act five"
        );
    }

    /// A world adopted from the full cache is a world of six acts, and the act it
    /// offers next is the dawn of history.
    #[test]
    fn a_world_of_six_acts_offers_the_seventh() {
        let genesis = Genesis::for_test(5, Stage::HydroPolitics);
        assert_eq!(genesis.ready(), Some(Stage::Civilization));
        assert_eq!(
            genesis.state(Stage::HydroPolitics),
            StageState::Done { secs: 0.0 }
        );
        assert_eq!(genesis.state(Stage::Civilization), StageState::Ready);
        assert_eq!(genesis.state(Stage::Languages), StageState::Locked);
        assert_eq!(
            genesis.state(Stage::Geology),
            StageState::Done { secs: 0.0 },
            "every act behind act six is done"
        );
    }

    /// A rewind un-does exactly the acts named and everything after them, and
    /// leaves the acts before them — and their artefacts — untouched.
    #[test]
    fn rewinding_drops_the_acts_at_and_after_the_one_named() {
        let mut genesis = Genesis::for_test(5, Stage::HydroPolitics);
        assert_eq!(genesis.ready(), Some(Stage::Civilization));

        genesis.rewind_to(Stage::Hydrology);

        assert_eq!(
            genesis.ready(),
            Some(Stage::Hydrology),
            "act three is the act that waits again"
        );
        assert_eq!(
            genesis.state(Stage::Climate),
            StageState::Done { secs: 0.0 },
            "act two is untouched"
        );
        assert_eq!(genesis.state(Stage::Hydrology), StageState::Ready);
        assert_eq!(genesis.state(Stage::Provinces), StageState::Locked);
        assert_eq!(genesis.state(Stage::HydroPolitics), StageState::Locked);

        // Act two's honest climate survives; act three's iced copy and its water
        // are gone, so act three can freeze the belt again from the same grid.
        let world = genesis.atlas().expect("two acts are still a world");
        assert_eq!(world.stage, Stage::Climate);
        assert!(
            world.temperature.is_some(),
            "act two's climate is still there to paint"
        );
        assert!(
            world.flow.is_none() && world.is_lake.is_none(),
            "act three's water is gone"
        );
    }

    /// Rewinding to the very beginning leaves nothing but the seed and the
    /// reader's choice about saving.
    #[test]
    fn rewinding_to_geology_empties_the_world_but_keeps_the_seed() {
        let mut genesis = Genesis::for_test(1_980, Stage::HydroPolitics);
        genesis.set_autosave(true);

        genesis.rewind_to(Stage::Geology);

        assert_eq!(genesis.ready(), Some(Stage::Geology));
        assert_eq!(genesis.seed(), 1_980, "the seed is kept");
        assert!(genesis.autosave(), "and so is the save choice");
        assert!(genesis.atlas().is_none(), "there is no world left to paint");
    }

    /// Rewinding to act eight drops the tongues and the names it gave the realms,
    /// but leaves the dawn act seven grew.
    #[test]
    fn rewinding_to_languages_keeps_the_dawn_but_drops_the_tongues() {
        let mut genesis = Genesis::for_test(1_980, Stage::Languages);
        assert_eq!(genesis.ready(), None, "a world of eight acts is finished");

        genesis.rewind_to(Stage::Languages);

        assert_eq!(genesis.ready(), Some(Stage::Languages));
        assert_eq!(
            genesis.state(Stage::Civilization),
            StageState::Done { secs: 0.0 },
            "act seven stands"
        );
        let world = genesis.atlas().expect("seven acts are still a world");
        assert!(world.languages.is_empty(), "act eight's tongues are gone");
    }

    /// A seed is read by each act as it runs, so changing it keeps every act
    /// already grown and re-points only the acts still to come.
    #[test]
    fn changing_the_seed_keeps_the_acts_already_grown() {
        let mut genesis = Genesis::for_test(1_980, Stage::Geology);
        assert_eq!(genesis.ready(), Some(Stage::Climate), "one act is grown");

        genesis.set_seed(4_242);

        assert_eq!(
            genesis.seed(),
            4_242,
            "the world now grows from the new seed"
        );
        assert_eq!(
            genesis.ready(),
            Some(Stage::Climate),
            "the act that was waiting is still the act that waits"
        );
        assert_eq!(
            genesis.state(Stage::Geology),
            StageState::Done { secs: 0.0 },
            "and the act already grown is untouched"
        );
        assert!(
            genesis.atlas().is_some(),
            "the heightmap act one grew is still there to paint"
        );
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
