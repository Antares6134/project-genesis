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
//!   7 History & Languages        realms, faiths, phonologies, lexicons, native names
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
use crate::climate::{
    ClimateConfig, PlanetaryMode, simulate_moisture, simulate_temperature, simulate_winds,
};
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::polar::PolarConfig;
use crate::core::progress::{MapRefs, Progress, Sink};
use crate::core::sphere::{CanvasScale, REFERENCE_CANVAS_HEIGHT, REFERENCE_CANVAS_WIDTH};
use crate::core::{Grid, WorldCache, WorldCacheView, cache_stage, polar, relief};
use crate::geography::{
    GeographyConfig, OceanConfig, OceanResult, ProvinceResult, generate_ocean_provinces,
    generate_provinces,
};
use crate::geopolitics::{
    GeopoliticalHierarchy, GeopoliticsConfig, build_geopolitical_hierarchy, build_ocean_hierarchy,
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

/// The engine's tuned reference canvas: 16K.
///
/// A world's resolution is part of its identity, exactly like its seed, so it
/// travels with the pipeline and the cache rather than living in a global. These
/// constants are the reference every tuned pixel constant is quoted at, the
/// default for a headless run, and the canvas an existing `world_<seed>.bin`
/// cache was written on. The interactive atlas starts on the 8K preset instead
/// (see `main`); `Genesis::new_sized` is how either path chooses its own plate.
pub const WORLD_WIDTH: usize = REFERENCE_CANVAS_WIDTH;
pub const WORLD_HEIGHT: usize = REFERENCE_CANVAS_HEIGHT;

/// Directory a world's cache is written to.
pub const CACHE_DIR: &str = "./saved_seeds";

// ─── THE WORLD'S OWN SETTINGS ────────────────────────────────────────────────

/// Every knob a reader may turn on a world, in the units the panel speaks.
///
/// The engine has no single settings object of its own: each act reads its own
/// config — [`WorldConfig`], [`ClimateConfig`], [`HydrologyConfig`] … — and
/// [`Genesis`] rebuilds those from these values at the moment the act runs. That
/// is what makes a change here apply to the acts **not yet grown**, exactly as
/// the seed already does, and what leaves the world already on the canvas
/// untouched by a setting typed after it.
///
/// The defaults are the engine's own: a world grown with the panel untouched is
/// bit-for-bit the world this program has always drawn. Each value is the one
/// the atlas was tuned at, and the safe bounds are the panel's, not the
/// engine's — a value far outside them is still a legal config, just not a
/// planet anyone has looked at.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGenSettings {
    // ── Planetary & astronomical ─────────────────────────────────────────────
    /// A rotating world with Hadley/Ferrel circulation, or an eyeball world
    /// locked to its sun.
    pub tidally_locked: bool,
    /// Sea-level surface temperature at the equator, in °C.
    pub equator_temp: f32,
    /// Sea-level surface temperature at the poles, in °C.
    pub pole_temp: f32,

    // ── Polar glaciation, in degrees of latitude ─────────────────────────────
    /// Where the permanent land ice cap begins to rise.
    pub polar_cap_start_lat_deg: f32,
    /// Where permanent pack ice begins to form over the ocean.
    pub polar_belt_start_lat_deg: f32,

    // ── Continental layout & coastlines ──────────────────────────────────────
    /// The continental field's water line: more land below, more ocean above.
    pub sea_threshold: f32,
    /// Continental field frequency: supercontinents low, archipelago high.
    ///
    /// The three fBm-shape values are `f64` so that the engine's own constant
    /// reaches the noise field at the precision it was tuned at — see
    /// [`WorldConfig::continental_freq`].
    pub continental_freq: f64,
    /// Fractal persistence of the continental field: smooth shores or craggy.
    pub continental_persistence: f64,
    /// Amplitude of the coordinate warp: fjords and folded coasts.
    pub domain_warp_amp: f32,

    // ── Orogeny & tectonics ──────────────────────────────────────────────────
    /// Maximum uplift along a fault line.
    pub mountain_lift: f32,
    /// Fault-line frequency: how densely the globe is crossed by ranges.
    pub fault_noise_freq: f64,
    /// Width of the orogenic collision zone: spine or plateau.
    pub fault_band: f32,

    // ── Weathering & erosion ─────────────────────────────────────────────────
    /// Simulated raindrops. The single dominant cost of act one.
    pub erosion_droplets: usize,
    /// Talus slumping passes: how far cliffs soften into scree slopes.
    pub thermal_iterations: usize,

    // ── Climate & moisture ───────────────────────────────────────────────────
    /// Global baseline rainfall before belts, shadows and continentality.
    pub base_rainfall: f32,
    /// Depth of the subtropical dry belt (the Hadley descent, ≈ ±26°).
    pub hadley_dry_depth: f32,
    /// Moisture retained in the deepest continental interior.
    pub inland_factor_floor: f32,
    /// Multiplier on mountain-induced precipitation.
    pub orographic_strength: f32,

    // ── Hydrology ────────────────────────────────────────────────────────────
    /// Share of rainfall that becomes surface discharge.
    pub runoff_coefficient: f32,
    /// Smallest discharge, in m³/s, worth drawing as a river.
    pub min_river_flow: f32,

    // ── Geography & wastelands ───────────────────────────────────────────────
    /// Land provinces across the planet — the **mean** the seed draws from.
    ///
    /// Each world draws its own exact target within ±7.5 % of this number, on
    /// whatever canvas it grows on. The province's pixel mass (and therefore the
    /// whole Act 4/5 hierarchy) follows the canvas, but the count does not.
    pub num_provinces: usize,
    /// Marine provinces across the world ocean — the **mean** the seed draws from.
    pub ocean_num_provinces: usize,
    /// Most ice-free land that may become impassable wilderness.
    ///
    /// `f64`, like the wilderness budget the province pass keeps: the share is
    /// multiplied by a pixel count that runs to tens of millions, and the
    /// engine's own `0.18` has to arrive intact for a default world to be the
    /// world it has always been.
    pub max_waste_share: f64,
    /// Hostility above which a province is abandoned to the wild.
    pub waste_cutoff: f32,

    // ── Political hierarchy ──────────────────────────────────────────────────
    /// Ceiling on the major continental cores.
    pub max_continents: usize,
}

impl Default for WorldGenSettings {
    fn default() -> Self {
        // Each default is read off the config it came from, so that the panel's
        // "reset" and the engine's own default can never drift apart.
        let world = WorldConfig::default();
        let climate = ClimateConfig::default();
        let hydro = HydrologyConfig::default();
        let geography = GeographyConfig::default();
        let ocean = OceanConfig::default();
        let geopolitics = GeopoliticsConfig::default();
        let polar = PolarConfig::default();
        Self {
            tidally_locked: !matches!(climate.mode, PlanetaryMode::RotatingStandard { .. }),
            equator_temp: climate.equator_temp,
            pole_temp: climate.pole_temp,
            polar_cap_start_lat_deg: polar.cap_start_lat.to_degrees(),
            polar_belt_start_lat_deg: polar.belt_start_lat.to_degrees(),
            sea_threshold: world.sea_threshold,
            continental_freq: world.continental_freq,
            continental_persistence: world.continental_persistence,
            domain_warp_amp: world.domain_warp_amp,
            mountain_lift: world.mountain_lift,
            fault_noise_freq: world.fault_noise_freq,
            fault_band: world.fault_band,
            // The budget below is the tuned 16K weathering: the crate's own
            // `WorldConfig::default` is a small-world default (120 000 droplets)
            // and is not what a planet is carved with. `Genesis::world_config`
            // scales it by the chosen canvas's area, so a 2K world receives the
            // same rain per square kilometre.
            erosion_droplets: 3_500_000,
            thermal_iterations: world.thermal_iterations,
            base_rainfall: climate.base_rainfall,
            hadley_dry_depth: climate.hadley_dry_depth,
            inland_factor_floor: climate.inland_factor_floor,
            orographic_strength: climate.orographic_strength,
            runoff_coefficient: hydro.runoff_coefficient,
            min_river_flow: hydro.min_river_flow,
            num_provinces: geography.num_provinces,
            ocean_num_provinces: ocean.num_provinces,
            max_waste_share: geography.max_waste_share,
            waste_cutoff: geography.waste_cutoff,
            max_continents: geopolitics.max_continents,
        }
    }
}

/// How far a drawn province target may sit from the typed mean, as a fraction.
///
/// 7.5 % puts a typed 4 000 between 3 700 and 4 300 — enough for the world to
/// feel grown rather than stamped, while the typed number stays the promise.
const DRAWN_COUNT_SPREAD: f64 = 0.075;

/// Salts that keep the land and the sea from drawing the same factor.
const LAND_PROVINCE_SALT: u64 = 0x51ED_270B_9E37_79B9;
const OCEAN_PROVINCE_SALT: u64 = 0x2545_F491_4F6C_DD1D;

/// The count this seed actually grows: the typed mean ±7.5 %, drawn once.
///
/// Deliberately independent of the canvas — the same seed asks for the same
/// number of provinces on every plate — and of the seeder's own RNG, so the
/// world's geography is untouched by the draw.
fn drawn_count(requested: usize, seed: u64, salt: u64) -> usize {
    // splitmix64's finalizer: a cheap, well-mixed 64-bit hash of (seed, salt).
    let mut z = seed.wrapping_add(salt);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let unit = (z >> 11) as f64 / (1u64 << 53) as f64; // [0, 1)
    let factor = 1.0 + (unit * 2.0 - 1.0) * DRAWN_COUNT_SPREAD;
    ((requested as f64 * factor).round() as usize).max(1)
}

impl WorldGenSettings {
    /// The engine's tuned defaults for one canvas.
    ///
    /// The counts that the engine was tuned at are quoted on the 16K reference
    /// plate (30 000 land provinces, 2 500 marine), so a fresh world on a
    /// smaller plate starts at the *same province density* the atlas has always
    /// drawn: 469 on 2K, 7 500 on 8K, 30 000 on 16K. Once the reader types a
    /// number it is absolute and no longer follows the canvas — the plate's
    /// defaults are only what "reset" and a fresh world begin from.
    pub fn for_canvas(width: usize, height: usize) -> Self {
        let scale = CanvasScale::new(width, height);
        let mut settings = Self::default();
        settings.num_provinces = scale.count(settings.num_provinces);
        settings.ocean_num_provinces = scale.count(settings.ocean_num_provinces);
        settings
    }

    /// Move the counts that are still the **old plate's** tuned defaults onto the
    /// new plate's defaults, leaving every value the reader typed alone.
    ///
    /// A world's canvas is part of its identity, and choosing a new one before
    /// act one must not silently ask a 2K plate for thirty thousand provinces.
    /// A value that has been edited is an instruction and is never touched.
    pub fn adopt_plate_defaults(&mut self, from: (usize, usize), to: (usize, usize)) {
        if from == to {
            return;
        }
        let old = Self::for_canvas(from.0, from.1);
        let new = Self::for_canvas(to.0, to.1);
        if self.num_provinces == old.num_provinces {
            self.num_provinces = new.num_provinces;
        }
        if self.ocean_num_provinces == old.ocean_num_provinces {
            self.ocean_num_provinces = new.ocean_num_provinces;
        }
    }

    /// The polar geography these settings describe, in radians.
    ///
    /// The degrees above are what the panel shows; this is the one conversion
    /// the engine needs, and it also enforces the planet's own rule that the sea
    /// freezes before the land is buried (see [`PolarConfig::from_degrees`]).
    pub fn polar_config(&self) -> PolarConfig {
        PolarConfig::from_degrees(self.polar_cap_start_lat_deg, self.polar_belt_start_lat_deg)
    }
}

/// The seven acts of creation, in the only order they can be run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Geology,
    Climate,
    Hydrology,
    Provinces,
    Geopolitics,
    HydroPolitics,
    History,
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
        Stage::History,
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
            Stage::History => 7,
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
            Stage::History => "History & Languages",
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
            Stage::History => {
                "Realms rise with a history, an age and a people; faiths, phonologies, \
                 proto-lexicons and native names follow."
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
/// world seven acts old are both paintable. The fields whose act has not been
/// run yet hold *empty* values — an empty hierarchy, a civilization with nobody
/// in it — never stale or placeholder data, and `stage` tells the painter which
/// is which: it must not read the province grids before act four, or the realms
/// before history.
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
    /// The dawn of history — the history act onward.
    pub civilizations: Arc<CivilizationWorld>,
    /// The world's languages — the history act onward.
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
    /// Act seven's realms, already wearing the names their own tongues gave them.
    civilizations: Option<Arc<CivilizationWorld>>,
    /// Act seven's languages: families, lexicons and the tongues the realms speak.
    languages: Option<Arc<LanguagesWorld>>,
    /// How long each act took, so the atlas can show it done.
    timings: [Option<f32>; 7],
    /// Write the world cache (every act grown so far, up to act six) whenever act
    /// three and again when act six land, overwriting the earlier save.
    autosave: bool,
    /// The world's own constants, as the panel set them. Read by every act when
    /// it runs, exactly like the seed, so a change applies to the acts not yet
    /// grown and never rewrites one already on the canvas.
    settings: WorldGenSettings,
}

impl Genesis {
    /// A world that has not been begun: a seed, and nothing else.
    pub fn new(seed: u64) -> Self {
        Self::new_sized(seed, WORLD_WIDTH, WORLD_HEIGHT)
    }

    /// A world that has not been begun, on a canvas of the reader's choosing.
    ///
    /// The canvas is part of a world's identity — the same seed on an 8K plate
    /// and on a 16K plate are two different worlds — so it is held by the
    /// pipeline alongside the seed, written into the cache, and fixed from the
    /// moment act one runs.
    pub fn new_sized(seed: u64, width: usize, height: usize) -> Self {
        Self {
            seed,
            stage: None,
            width: width.max(1),
            height: height.max(1),
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
            languages: None,
            timings: [None; 7],
            autosave: false,
            // A fresh world starts at the tuned province density for its plate;
            // anything the reader types afterwards is an absolute count.
            settings: WorldGenSettings::for_canvas(width.max(1), height.max(1)),
        }
    }

    /// The canvas this world grows on, in pixels.
    pub fn canvas(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Set the canvas, before act one has fixed it.
    ///
    /// Returns `false` and changes nothing once the world has begun: a heightmap
    /// already exists at the old size, and silently reinterpreting it on another
    /// plate would be a lie. The panel therefore locks the choice behind a reset.
    pub fn set_canvas(&mut self, width: usize, height: usize) -> bool {
        if self.stage.is_some() || self.heightmap.is_some() {
            return false;
        }
        let old = (self.width, self.height);
        self.width = width.max(1);
        self.height = height.max(1);
        // A count still sitting at the old plate's tuned default follows the new
        // plate; a count the reader typed is absolute and stays put.
        self.settings
            .adopt_plate_defaults(old, (self.width, self.height));
        true
    }

    /// The runtime canvas expressed as a scale against the tuned 16K reference.
    pub fn canvas_scale(&self) -> CanvasScale {
        CanvasScale::new(self.width, self.height)
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

    /// The world's constants as the panel has them.
    pub fn settings(&self) -> &WorldGenSettings {
        &self.settings
    }

    /// Set the world's constants.
    ///
    /// Like the seed, these are read by each act *as it runs*, so a change
    /// applies to the acts not yet grown and leaves the acts already done — and
    /// every grid they produced — exactly as they are. Press ↺ on an act to grow
    /// it again under the new settings.
    pub fn set_settings(&mut self, settings: WorldGenSettings) {
        self.settings = settings;
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
    /// One artefact is deliberately kept apart from its act so that a rewind can
    /// restore it honestly: [`Genesis::temperature`] (act two's climate, which
    /// act three would otherwise overwrite with its iced copy). History publishes
    /// its named realms and tongues together, so a rewind drops both.
    pub fn rewind_to(&mut self, stage: Stage) {
        if stage.index() <= 1 {
            // Back to before act one: nothing is left of the world but the
            // choices that outlive a world — its seed, its canvas, its save
            // setting and the constants the reader tuned.
            let (seed, autosave) = (self.seed, self.autosave);
            let (width, height) = (self.width, self.height);
            let settings = self.settings.clone();
            *self = Self::new_sized(seed, width, height);
            self.autosave = autosave;
            self.settings = settings;
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
                Stage::History => {
                    self.civilizations = None;
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
        let scale = self.canvas_scale();
        let mut config = WorldConfig::for_canvas(self.width, self.height);
        config.seed = self.seed;
        config.num_plates = 32;
        config.ocean_ratio = 0.65;
        // Everything the terrain pass actually reads comes from the panel:
        // the water line, the continental field and the orogeny that folds it.
        config.sea_threshold = self.settings.sea_threshold;
        config.continental_freq = self.settings.continental_freq;
        config.continental_persistence = self.settings.continental_persistence;
        config.domain_warp_amp = self.settings.domain_warp_amp;
        config.mountain_lift = self.settings.mountain_lift;
        config.fault_noise_freq = self.settings.fault_noise_freq;
        config.fault_band = self.settings.fault_band;
        // Droplets are a density, not a head count: the tuned budget is 3.5M on
        // the 16K plate, and a quarter of that on 8K carves the same rain per
        // square kilometre.
        config.erosion_droplets = scale.count(self.settings.erosion_droplets);
        config.thermal_iterations = self.settings.thermal_iterations;
        config
    }

    pub fn climate_config(&self) -> ClimateConfig {
        ClimateConfig {
            seed: self.seed,
            mode: if self.settings.tidally_locked {
                PlanetaryMode::TidallyLocked
            } else {
                PlanetaryMode::RotatingStandard {
                    axial_tilt_deg: 23.5,
                }
            },
            equator_temp: self.settings.equator_temp,
            pole_temp: self.settings.pole_temp,
            base_rainfall: self.settings.base_rainfall,
            hadley_dry_depth: self.settings.hadley_dry_depth,
            inland_factor_floor: self.settings.inland_factor_floor,
            orographic_strength: self.settings.orographic_strength,
            ..Default::default()
        }
    }

    pub fn hydro_config(&self) -> HydrologyConfig {
        let mut config = HydrologyConfig::for_canvas(self.width, self.height);
        config.min_lake_depth = 0.012;
        // Only the rivers an atlas would ink: the major ones and their larger
        // branches, not the capillary web.
        config.min_river_flow = self.settings.min_river_flow;
        // The Nile — a continental trunk drawn at full width.
        config.major_river_flow = 3_000.0;
        config.runoff_coefficient = self.settings.runoff_coefficient;
        config
    }

    pub fn geography_config(&self) -> GeographyConfig {
        let mut config = GeographyConfig::default();
        // The panel's number is a mean; this seed draws its own target around it.
        config.num_provinces = drawn_count(
            self.settings.num_provinces.max(1),
            self.seed,
            LAND_PROVINCE_SALT,
        );
        config.max_waste_share = self.settings.max_waste_share;
        config.waste_cutoff = self.settings.waste_cutoff;
        config
    }

    pub fn ocean_config(&self) -> OceanConfig {
        let mut config = OceanConfig::default();
        config.num_provinces = drawn_count(
            self.settings.ocean_num_provinces.max(1),
            self.seed,
            OCEAN_PROVINCE_SALT,
        );
        config
    }

    pub fn geopolitics_config(&self) -> GeopoliticsConfig {
        GeopoliticsConfig {
            max_continents: self.settings.max_continents,
        }
    }

    /// Where this world's polar ice lies, in radians.
    pub fn polar_config(&self) -> PolarConfig {
        self.settings.polar_config()
    }

    pub fn civilization_config(&self) -> CivilizationConfig {
        CivilizationConfig::default()
    }

    pub fn language_config(&self) -> LanguageConfig {
        LanguageConfig::default()
    }

    // ─── THE WORLD CACHE ─────────────────────────────────────────────────────

    /// Where this world's cache lives: `world_<seed>_<width>x<height>.bin`.
    pub fn cache_path(&self) -> PathBuf {
        cache_path(self.seed, self.width, self.height)
    }

    /// Write the acts grown so far to disk, overwriting any earlier save for
    /// this seed **on this canvas**.
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

    /// Adopt the acts already saved for this seed.
    ///
    /// The current canvas is preferred, so an 8K pipeline loads the 8K cache.
    /// If this seed has no cache on that canvas — an older save from before
    /// resolution was part of the filename, for example — the newest cache for
    /// the seed is adopted instead and its own dimensions become the world's.
    ///
    /// The save carries acts one through whichever act the world had reached when
    /// it was written, so the pipeline picks up at the act after that one — act
    /// four for a save made at act three, history for one made at act six. On
    /// success it holds exactly that much of the world, and no more.
    pub fn load_cache(&mut self) -> Result<(), LoadError> {
        let exact = self.cache_path();
        if Path::new(&exact).exists() {
            return self.load_cached_path(exact);
        }
        match cached_world(self.seed) {
            Some(found) => self.load_cached_world(&found),
            None => Err(LoadError::Missing(exact)),
        }
    }

    /// Adopt the acts already saved in one cache found on disk.
    pub fn load_cached_world(&mut self, found: &CachedWorld) -> Result<(), LoadError> {
        self.load_cached_path(found.path.clone())
    }

    /// Read one cache file and adopt every act it carries.
    fn load_cached_path(&mut self, path: PathBuf) -> Result<(), LoadError> {
        let start = Instant::now();
        println!("Loading world from cache '{}'…", path.display());
        let cache = WorldCache::try_load(&path.to_string_lossy())
            .map_err(|why| LoadError::Unreadable(path.clone(), why))?;

        let stage = Stage::ALL[(cache.stage() - 1) as usize];
        let old_canvas = (self.width, self.height);
        self.width = cache.width;
        self.height = cache.height;
        // A count still at the old plate's tuned default follows the plate the
        // cache was written on; a value the reader typed is absolute and stays.
        self.settings
            .adopt_plate_defaults(old_canvas, (self.width, self.height));
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

/// Where a world's cache lives: one file per seed **per canvas**.
pub fn cache_path(seed: u64, width: usize, height: usize) -> PathBuf {
    PathBuf::from(format!("{CACHE_DIR}/world_{seed}_{width}x{height}.bin"))
}

/// One world cache found on disk, as the atlas lists it.
///
/// This is metadata only: the atlas reads a cache's *header* — never its payload —
/// to describe it. The file itself is loaded only when the reader asks to load it.
///
/// A cache is identified by its seed **and its canvas**. An 8K and a 16K world
/// of the same seed are two different planets, and two different files:
/// `world_<seed>_<width>x<height>.bin`. A save from an older build, spelled
/// `world_<seed>.bin`, is recognised as the reference 16K plate.
#[derive(Clone, Debug)]
pub struct CachedWorld {
    /// The seed the cache is named after.
    pub seed: u64,
    /// Canvas width the cache holds.
    pub width: usize,
    /// Canvas height the cache holds.
    pub height: usize,
    /// The file on disk, so a pick loads the very file the list described.
    pub path: PathBuf,
    /// True for a pre-resolution cache named `world_<seed>.bin`.
    pub legacy: bool,
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

/// The newest cache for one seed, on any canvas.
///
/// This scans the cache directory, so the atlas uses its once-per-scan list
/// instead of calling it per frame. It is for `--load`, tests and the headless
/// path, where there is no panel to keep warm.
pub fn cached_world(seed: u64) -> Option<CachedWorld> {
    cached_worlds().into_iter().find(|world| world.seed == seed)
}

/// Parse a cache filename into `(seed, width, height, legacy)`.
///
/// Only canonical spellings are worlds the atlas can load. A stray file, a
/// `.lang.bin` left over from an older build, or `world_0042.bin` names nothing,
/// exactly as before. The one old spelling still accepted is `world_<seed>.bin`:
/// every cache written before the canvas joined the name was a 16K plate.
fn parse_cache_name(name: &str) -> Option<(u64, usize, usize, bool)> {
    let rest = name.strip_prefix("world_")?.strip_suffix(".bin")?;
    if let Some((seed_part, canvas)) = rest.split_once('_') {
        let (width, height) = canvas.split_once('x')?;
        let seed = seed_part.parse::<u64>().ok()?;
        let width = width.parse::<usize>().ok()?;
        let height = height.parse::<usize>().ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        if name != format!("world_{seed}_{width}x{height}.bin") {
            return None;
        }
        return Some((seed, width, height, false));
    }
    let seed = rest.parse::<u64>().ok()?;
    if name != format!("world_{seed}.bin") {
        return None;
    }
    Some((seed, WORLD_WIDTH, WORLD_HEIGHT, true))
}

/// Every world cache in the cache directory, newest first.
pub fn cached_worlds() -> Vec<CachedWorld> {
    cached_worlds_in(Path::new(CACHE_DIR))
}

/// Every world cache in `dir`, newest first.
///
/// A seed may now have a cache on each canvas it was grown on; all of them are
/// listed, because they are genuinely different worlds. A canonical file and a
/// legacy `world_<seed>.bin` for the same seed and canvas are deduplicated in
/// favour of the canonical one, so a resaved world does not appear twice. An
/// unreadable directory is an empty list, never a panic.
pub fn cached_worlds_in(dir: &Path) -> Vec<CachedWorld> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut worlds: Vec<CachedWorld> = Vec::new();
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
        let Some((seed, width, height, legacy)) = parse_cache_name(&name) else {
            continue;
        };
        let path = entry.path();
        let Ok(file) = entry.metadata() else {
            continue;
        };
        // The act is in the header, so the list pays a handful of bytes per file
        // rather than decoding a payload.
        let acts = cache_stage(&path.to_string_lossy());
        let world = CachedWorld {
            seed,
            width,
            height,
            path,
            legacy,
            bytes: file.len(),
            modified: file.modified().ok(),
            acts,
        };
        if let Some(existing) = worlds
            .iter_mut()
            .find(|other| (other.seed, other.width, other.height) == (seed, width, height))
        {
            // Prefer the canonical spelling, then the fresher file.
            let fresher = match (world.modified, existing.modified) {
                (Some(new), Some(old)) => new > old,
                (Some(_), None) => true,
                _ => false,
            };
            if (existing.legacy && !world.legacy) || (existing.legacy == world.legacy && fresher) {
                *existing = world;
            }
        } else {
            worlds.push(world);
        }
    }
    worlds.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then(b.seed.cmp(&a.seed))
            .then(b.width.cmp(&a.width))
    });
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
            Stage::History => self.run_history(&sink),
        };
        let secs = start.elapsed().as_secs_f32();
        self.timings[stage.index() - 1] = Some(secs);
        self.stage = Some(stage);
        println!(
            "✓ [{}/{}] {} completed: {:.2?}",
            stage.index(),
            Stage::ALL.len(),
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
            genesis.frozen_temperature = Some(Arc::new(Grid::filled_with(width, height, -30.0f32)));
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
        if stage.available(Stage::History) {
            genesis.civilizations = Some(Arc::new(CivilizationWorld::empty(seed)));
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
            // History publishes the named realms; a world younger than history
            // has no realms to paint at all.
            civilizations: self
                .civilizations
                .clone()
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
        let polar_config = self.polar_config();
        let progress = watching(sink);

        let mut result = simulate_tectonics(&world_config, &polar_config, &progress);
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
        let polar_ice = polar::permanent_polar_ice(
            &heightmap,
            &is_lake,
            relief::SEA_LEVEL,
            self.seed,
            &self.polar_config(),
        );
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
        let polar_ice = polar::permanent_polar_ice(
            &heightmap,
            &is_lake,
            relief::SEA_LEVEL,
            self.seed,
            &self.polar_config(),
        );
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
        let mut summary = vec![
            format!("{} provinces", thousands_u64(result.num_provinces as u64)),
            format!("{} wasteland pixels", thousands_u64(wasteland as u64)),
        ];
        // The panel's number is a mean, and this seed drew its own target around
        // it. Say both, so a world a few hundred provinces off the typed value is
        // explained rather than mistaken for a shortfall — and still say plainly
        // when the seeder could not fit even the drawn target on this planet.
        if geo_config.num_provinces != self.settings.num_provinces {
            summary.push(format!(
                "drawn from {} requested",
                thousands_u64(self.settings.num_provinces as u64)
            ));
        }
        if result.num_provinces < geo_config.num_provinces {
            summary.push(format!(
                "{} target · {} fit on this planet",
                thousands_u64(geo_config.num_provinces as u64),
                thousands_u64(result.num_provinces as u64)
            ));
        }
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
            // The ceiling on continental cores is the panel's; the clustering
            // below turns whatever cores it is given into a hierarchy.
            self.geopolitics_config().max_continents,
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

    // ─── 7. HISTORY & LANGUAGES ──────────────────────────────────────────────

    /// The history act: societies, faiths, languages and the names they give
    /// one another. The two models share one act because the naming pass needs
    /// both worlds at once; the dependency is one-way — societies first, tongues
    /// second, names last.
    fn run_history(&mut self, sink: &Option<Arc<Sink>>) -> Vec<String> {
        let hydro_config = self.hydro_config();
        let climate_config = self.climate_config();
        let civ_config = self.civilization_config();
        let lang_config = self.language_config();
        let atlas = self.atlas().expect("act six runs before history");
        let provinces = atlas
            .provinces
            .clone()
            .expect("act four runs before history");
        let hierarchy = atlas.hierarchy.clone();
        let is_lake = atlas
            .is_lake
            .clone()
            .expect("act three runs before history");
        let temperature = atlas
            .temperature
            .clone()
            .expect("act two runs before history");
        let precipitation = atlas
            .precipitation
            .clone()
            .expect("act two runs before history");
        let flow = atlas.flow.clone().expect("act three runs before history");

        let progress = watching(sink).with_map(MapRefs {
            heightmap: &atlas.heightmap,
            province_ids: &provinces.province_ids,
            ocean_ids: None,
        });
        let societies = generate_civilizations(
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

        let (languages, civilizations) =
            generate_languages(&atlas, &societies, &lang_config, self.seed, &progress);

        let summary = vec![
            format!("{} realms", civilizations.civilizations.len()),
            format!(
                "{} civilized provinces",
                thousands_u64(civilizations.civilized_provinces as u64)
            ),
            format!(
                "world population {}",
                thousands_u64(civilizations.total_population)
            ),
            format!("atlas date {}", civilizations.map_year_label()),
            format!("{} language families", languages.families.len()),
            format!(
                "{} languages across {} provinces",
                languages.languages.len(),
                languages.total_provinces()
            ),
            format!("{} realms named", civilizations.civilizations.len()),
        ];
        self.civilizations = Some(Arc::new(civilizations));
        self.languages = Some(Arc::new(languages));
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

    // ─── THE WORLD'S OWN SETTINGS ────────────────────────────────────────────

    /// A world grown with the panel untouched must be the world this program has
    /// always drawn: every default is the constant the engine was tuned at, and
    /// each one lands in the config of the act that reads it.
    #[test]
    fn the_default_settings_rebuild_the_engine_s_own_configs() {
        let genesis = Genesis::new(42);
        assert_eq!(
            *genesis.settings(),
            WorldGenSettings::default(),
            "a fresh world starts at the engine's defaults"
        );

        let world = genesis.world_config();
        assert_eq!(world.sea_threshold, 0.11);
        assert_eq!(world.continental_freq, 1.05);
        assert_eq!(world.continental_persistence, 0.52);
        assert_eq!(world.domain_warp_amp, 0.32);
        assert_eq!(world.mountain_lift, 0.45);
        assert_eq!(world.fault_noise_freq, 1.60);
        assert_eq!(world.fault_band, 0.10);
        assert_eq!(world.erosion_droplets, 3_500_000);
        assert_eq!(world.thermal_iterations, 4);

        let climate = genesis.climate_config();
        assert_eq!(
            climate.mode,
            PlanetaryMode::RotatingStandard {
                axial_tilt_deg: 23.5
            }
        );
        assert_eq!(climate.equator_temp, 32.0);
        assert_eq!(climate.pole_temp, -28.0);
        assert_eq!(climate.base_rainfall, 0.45);
        assert_eq!(climate.hadley_dry_depth, 0.48);
        assert_eq!(climate.inland_factor_floor, 0.40);
        assert_eq!(climate.orographic_strength, 15.0);

        let hydro = genesis.hydro_config();
        assert_eq!(hydro.runoff_coefficient, 0.35);
        assert_eq!(hydro.min_river_flow, 100.0);

        let geography = genesis.geography_config();
        assert_eq!(
            geography.num_provinces,
            drawn_count(30_000, genesis.seed(), LAND_PROVINCE_SALT),
            "the typed mean is drawn into the seed's own target"
        );
        assert_eq!(geography.max_waste_share, 0.18);
        assert_eq!(geography.waste_cutoff, 0.68);

        assert_eq!(
            genesis.ocean_config().num_provinces,
            drawn_count(2_500, genesis.seed(), OCEAN_PROVINCE_SALT)
        );
        assert_eq!(genesis.geopolitics_config().max_continents, 7);
        assert_eq!(
            genesis.polar_config(),
            crate::core::polar::PolarConfig::default()
        );
    }

    /// A constant a reader moves reaches the act that reads it — and only that
    /// act's config: the settings are rebuilt into each config as the act runs,
    /// which is what makes a change apply to the acts not yet grown.
    #[test]
    fn a_setting_reaches_the_config_of_the_act_that_reads_it() {
        let mut genesis = Genesis::new(42);
        let settings = WorldGenSettings {
            tidally_locked: true,
            equator_temp: 48.0,
            polar_cap_start_lat_deg: 50.0,
            polar_belt_start_lat_deg: 45.0,
            sea_threshold: 0.05,
            continental_freq: 3.0,
            mountain_lift: 0.80,
            erosion_droplets: 500_000,
            base_rainfall: 0.80,
            orographic_strength: 4.0,
            runoff_coefficient: 0.60,
            min_river_flow: 250.0,
            num_provinces: 12_000,
            ocean_num_provinces: 1_000,
            max_waste_share: 0.0,
            waste_cutoff: 0.45,
            max_continents: 3,
            ..WorldGenSettings::default()
        };
        genesis.set_settings(settings);

        let world = genesis.world_config();
        assert_eq!(world.sea_threshold, 0.05);
        assert_eq!(world.continental_freq, 3.0);
        assert_eq!(world.mountain_lift, 0.80);
        assert_eq!(world.erosion_droplets, 500_000);

        let climate = genesis.climate_config();
        assert_eq!(climate.mode, PlanetaryMode::TidallyLocked);
        assert_eq!(climate.equator_temp, 48.0);
        assert_eq!(climate.base_rainfall, 0.80);
        assert_eq!(climate.orographic_strength, 4.0);

        assert_eq!(genesis.hydro_config().runoff_coefficient, 0.60);
        assert_eq!(genesis.hydro_config().min_river_flow, 250.0);

        let geography = genesis.geography_config();
        assert_eq!(
            geography.num_provinces,
            drawn_count(12_000, genesis.seed(), LAND_PROVINCE_SALT)
        );
        assert_eq!(geography.max_waste_share, 0.0);
        assert_eq!(geography.waste_cutoff, 0.45);

        assert_eq!(
            genesis.ocean_config().num_provinces,
            drawn_count(1_000, genesis.seed(), OCEAN_PROVINCE_SALT)
        );
        assert_eq!(genesis.geopolitics_config().max_continents, 3);

        let polar = genesis.polar_config();
        assert!((polar.cap_start_lat.to_degrees() - 50.0).abs() < 1.0e-3);
        assert!((polar.belt_start_lat.to_degrees() - 45.0).abs() < 1.0e-3);
    }

    /// The settings are read by each act as it runs, so moving one leaves every
    /// act already grown — and the world they produced — exactly as it was.
    #[test]
    fn changing_a_setting_leaves_the_acts_already_grown_alone() {
        let mut genesis = Genesis::for_test(42, Stage::Geology);
        let grown = genesis.atlas().expect("act one has run");
        let before = grown.heightmap.clone();

        genesis.set_settings(WorldGenSettings {
            sea_threshold: 0.20,
            ..WorldGenSettings::default()
        });

        assert_eq!(
            genesis.stage(),
            Some(Stage::Geology),
            "no act is un-done by a setting"
        );
        let after = genesis.atlas().expect("the world is still there");
        assert!(
            Arc::ptr_eq(&before, &after.heightmap),
            "and the heightmap act one made is not rebuilt"
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
    /// the pipeline picks up at history rather than re-running acts one to six.
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

        assert_eq!(adopted.ready(), Some(Stage::History));
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
        let path = cache_path(seed, 4, 2);
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

    /// A cache is named after the seed it holds and the canvas it was grown on.
    #[test]
    fn a_worlds_cache_is_named_after_its_seed_and_canvas() {
        assert_eq!(
            cache_path(1_980, 8_192, 4_096).to_string_lossy(),
            format!("{CACHE_DIR}/world_1980_8192x4096.bin")
        );
        assert_ne!(
            cache_path(1, 8_192, 4_096).to_string_lossy(),
            cache_path(2, 8_192, 4_096).to_string_lossy()
        );
        assert_ne!(
            cache_path(42, 8_192, 4_096).to_string_lossy(),
            cache_path(42, 16_384, 8_192).to_string_lossy(),
            "the same seed on two canvases is two worlds"
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
        let path = cache_path(seed, 4, 2);
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
    /// offers next is history.
    #[test]
    fn a_world_of_six_acts_offers_the_seventh() {
        let genesis = Genesis::for_test(5, Stage::HydroPolitics);
        assert_eq!(genesis.ready(), Some(Stage::History));
        assert_eq!(
            genesis.state(Stage::HydroPolitics),
            StageState::Done { secs: 0.0 }
        );
        assert_eq!(genesis.state(Stage::History), StageState::Ready);
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
        assert_eq!(genesis.ready(), Some(Stage::History));

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

    /// Rewinding to history drops its realms and tongues, but leaves every
    /// physical act before it untouched.
    #[test]
    fn rewinding_to_history_drops_the_realms_and_the_tongues() {
        let mut genesis = Genesis::for_test(1_980, Stage::History);
        assert_eq!(genesis.ready(), None, "a world of seven acts is finished");

        genesis.rewind_to(Stage::History);

        assert_eq!(genesis.ready(), Some(Stage::History));
        assert_eq!(
            genesis.state(Stage::HydroPolitics),
            StageState::Done { secs: 0.0 },
            "act six stands"
        );
        let world = genesis.atlas().expect("six acts are still a world");
        assert!(world.languages.is_empty(), "history's tongues are gone");
        assert!(
            world.civilizations.is_empty(),
            "history's realms are gone too"
        );
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

    // ─── THE RUNTIME CANVAS ──────────────────────────────────────────────────

    /// A drawn target sits within 7.5 % of the typed mean.
    fn assert_drawn_near(drawn: usize, mean: usize) {
        let limit = mean as f64 * 0.075 + 0.5;
        assert!(
            (drawn as f64 - mean as f64).abs() <= limit,
            "drawn {drawn} is not within 7.5 % of {mean}"
        );
    }

    /// A sized pipeline grows the canvas it was given, and its terrain config
    /// reads that canvas rather than the global reference.
    #[test]
    fn a_sized_world_uses_its_own_canvas() {
        let genesis = Genesis::new_sized(42, 2_048, 1_024);
        assert_eq!(genesis.canvas(), (2_048, 1_024));
        let terrain = genesis.world_config();
        assert_eq!((terrain.width, terrain.height), (2_048, 1_024));
        assert_eq!(terrain.strait_width, 1);
        assert_eq!(terrain.strait_max_span, 150);
        assert_eq!(terrain.max_droplet_lifetime, 5);
        assert_eq!(terrain.erosion_droplets, 54_688);
        assert_eq!(
            genesis.settings().num_provinces,
            469,
            "the plate's default mean"
        );
        assert_drawn_near(genesis.geography_config().num_provinces, 469);
        assert_eq!(genesis.hydro_config().min_lake_cells, 1);
    }

    /// The per-resolution budgets follow the area and linear scales, and the 16K
    /// reference is exactly the world the engine has always grown. The province
    /// *default* is part of that density: a fresh 2K world starts at 469 and a
    /// fresh 8K at 7 500, so the plate still draws the world it always did.
    #[test]
    fn the_resolution_budgets_scale_with_the_canvas() {
        let cases = [
            (2_048, 1_024, 469, 54_688, 1, 150),
            (4_096, 2_048, 30_000 / 16, 3_500_000 / 16, 1, 300),
            (8_192, 4_096, 30_000 / 4, 3_500_000 / 4, 3, 600),
            (12_288, 6_144, 16_875, 1_968_750, 7, 900),
            (16_384, 8_192, 30_000, 3_500_000, 12, 1_200),
        ];
        for (width, height, provinces, droplets, lake_cells, strait_span) in cases {
            let genesis = Genesis::new_sized(42, width, height);
            let terrain = genesis.world_config();
            assert_eq!(
                genesis.settings().num_provinces,
                provinces,
                "default provinces at {width}×{height}"
            );
            assert_drawn_near(genesis.geography_config().num_provinces, provinces);
            assert_eq!(
                terrain.erosion_droplets, droplets,
                "droplets at {width}×{height}"
            );
            assert_eq!(
                genesis.hydro_config().min_lake_cells,
                lake_cells,
                "lake cells at {width}×{height}"
            );
            assert_eq!(
                terrain.strait_max_span, strait_span,
                "strait span at {width}×{height}"
            );
        }

        let reference = Genesis::new_sized(42, 16_384, 8_192);
        assert_eq!(reference.settings().num_provinces, 30_000);
        assert_drawn_near(reference.geography_config().num_provinces, 30_000);
        assert_eq!(reference.world_config().erosion_droplets, 3_500_000);
        assert_eq!(reference.world_config().strait_width, 8);
        assert_eq!(reference.hydro_config().min_lake_cells, 12);
    }

    /// The drawn target stays inside its band, varies with the seed, and is the
    /// same draw on every canvas.
    #[test]
    fn the_drawn_province_target_follows_the_seed_and_not_the_canvas() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..64u64 {
            let drawn = drawn_count(4_000, seed, LAND_PROVINCE_SALT);
            assert!(
                (3_700..=4_300).contains(&drawn),
                "seed {seed} drew {drawn}, outside 3 700–4 300"
            );
            seen.insert(drawn);
        }
        assert!(seen.len() > 8, "the draw must vary from seed to seed");

        let mut settings = WorldGenSettings::default();
        settings.num_provinces = 4_000;
        settings.ocean_num_provinces = 500;
        let mut land = None;
        let mut sea = None;
        for (width, height) in [
            (2_048usize, 1_024usize),
            (4_096, 2_048),
            (8_192, 4_096),
            (12_288, 6_144),
            (16_384, 8_192),
        ] {
            let mut genesis = Genesis::new_sized(42, width, height);
            genesis.set_settings(settings.clone());
            let drawn = genesis.geography_config().num_provinces;
            assert_drawn_near(drawn, 4_000);
            land.get_or_insert(drawn);
            assert_eq!(
                land,
                Some(drawn),
                "the land draw must not follow the canvas"
            );
            let drawn_sea = genesis.ocean_config().num_provinces;
            assert_drawn_near(drawn_sea, 500);
            sea.get_or_insert(drawn_sea);
            assert_eq!(
                sea,
                Some(drawn_sea),
                "the marine draw must not follow the canvas"
            );
        }
    }

    /// A province count the reader typed is absolute in its *mean*: the same
    /// number asks for the same drawn provinces on every plate.
    #[test]
    fn a_typed_province_count_does_not_follow_the_canvas() {
        let mut settings = WorldGenSettings::default();
        settings.num_provinces = 4_000;
        settings.ocean_num_provinces = 500;
        for (width, height) in [
            (2_048usize, 1_024usize),
            (4_096, 2_048),
            (8_192, 4_096),
            (12_288, 6_144),
            (16_384, 8_192),
        ] {
            let mut genesis = Genesis::new_sized(42, width, height);
            genesis.set_settings(settings.clone());
            assert_eq!(
                genesis.geography_config().num_provinces,
                drawn_count(4_000, 42, LAND_PROVINCE_SALT),
                "land provinces at {width}×{height}"
            );
            assert_eq!(
                genesis.ocean_config().num_provinces,
                drawn_count(500, 42, OCEAN_PROVINCE_SALT),
                "marine provinces at {width}×{height}"
            );
        }
    }

    /// A fresh world starts at its plate's tuned density; a value still at that
    /// density follows a canvas change, and a value the reader typed does not.
    #[test]
    fn the_plate_sets_the_default_density_and_typed_values_outlive_it() {
        assert_eq!(
            Genesis::new_sized(42, 2_048, 1_024)
                .settings()
                .num_provinces,
            469
        );
        assert_eq!(
            Genesis::new_sized(42, 8_192, 4_096)
                .settings()
                .num_provinces,
            7_500
        );
        assert_eq!(
            Genesis::new_sized(42, 16_384, 8_192)
                .settings()
                .num_provinces,
            30_000
        );

        let mut genesis = Genesis::new_sized(42, 8_192, 4_096);
        assert!(genesis.set_canvas(16_384, 8_192));
        assert_eq!(
            genesis.settings().num_provinces,
            30_000,
            "an untouched default follows the new plate"
        );
        assert!(genesis.set_canvas(8_192, 4_096));
        assert_eq!(genesis.settings().num_provinces, 7_500);

        let mut typed = genesis.settings().clone();
        typed.num_provinces = 4_000;
        genesis.set_settings(typed);
        assert!(genesis.set_canvas(2_048, 1_024));
        assert_eq!(
            genesis.settings().num_provinces,
            4_000,
            "a typed count is absolute and survives a canvas change"
        );
    }

    /// The canvas may be chosen before act one and never after it.
    #[test]
    fn the_canvas_is_fixed_once_act_one_has_run() {
        let mut blank = Genesis::new_sized(42, 2_048, 1_024);
        assert!(blank.set_canvas(4_096, 2_048));
        assert_eq!(blank.canvas(), (4_096, 2_048));

        let mut grown = Genesis::for_test(42, Stage::Geology);
        assert!(!grown.set_canvas(2_048, 1_024));
        assert_eq!(grown.canvas(), (4, 2), "the grown world keeps its plate");
        assert_eq!(grown.world_config().strait_width, 1);
    }

    /// A rewind to before act one keeps the canvas and the reader's constants.
    #[test]
    fn rewinding_to_geology_keeps_the_canvas_and_settings() {
        let mut genesis = Genesis::for_test(42, Stage::Hydrology);
        genesis.width = 8_192;
        genesis.height = 4_096;
        genesis.set_settings(WorldGenSettings {
            sea_threshold: 0.05,
            ..WorldGenSettings::default()
        });

        genesis.rewind_to(Stage::Geology);

        assert_eq!(genesis.canvas(), (8_192, 4_096));
        assert_eq!(genesis.settings().sea_threshold, 0.05);
        assert_eq!(genesis.world_config().strait_width, 4);
    }

    /// Cache names carry the canvas, and only canonical spellings are listed.
    #[test]
    fn a_cache_name_carries_seed_and_canvas() {
        assert_eq!(
            parse_cache_name("world_42_2048x1024.bin"),
            Some((42, 2_048, 1_024, false))
        );
        assert_eq!(
            parse_cache_name("world_42.bin"),
            Some((42, WORLD_WIDTH, WORLD_HEIGHT, true)),
            "an old save is the reference 16K plate"
        );
        assert_eq!(parse_cache_name("world_0042_2048x1024.bin"), None);
        assert_eq!(parse_cache_name("world_42_2048X1024.bin"), None);
        assert_eq!(parse_cache_name("world_42_0x1024.bin"), None);
        assert_eq!(parse_cache_name("world_42.full.bin"), None);
    }

    /// A load with no cache on the current canvas falls back to the seed's cache
    /// and adopts the canvas the cache actually holds.
    #[test]
    fn a_load_adopts_the_canvas_of_the_cache_it_finds() {
        let seed = 0x0BAD_C0D2;
        let path = Genesis::for_test(seed, Stage::Hydrology).cache_path();
        let _ = std::fs::remove_file(&path);
        Genesis::for_test(seed, Stage::Hydrology)
            .save_cache()
            .expect("write the world");

        let mut adopted = Genesis::new(seed);
        assert_eq!(adopted.canvas(), (WORLD_WIDTH, WORLD_HEIGHT));
        adopted.load_cache().expect("read the world back");
        assert_eq!(adopted.canvas(), (4, 2), "the file's canvas is the world's");

        let _ = std::fs::remove_file(&path);
    }

    /// The same seed on two canvases is two entries, and a canonical file is
    /// preferred to a legacy file for the same plate.
    #[test]
    fn a_cache_directory_lists_two_canvases_of_one_seed() {
        let dir = std::env::temp_dir().join(format!(
            "project-genesis-cache-canvases-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch cache directory");
        for name in [
            "world_42_8192x4096.bin",
            "world_42_16384x8192.bin",
            "world_42.bin",
        ] {
            std::fs::write(dir.join(name), b"cache").expect("a scratch cache file");
        }

        let worlds = cached_worlds_in(&dir);
        let forty_two: Vec<&CachedWorld> = worlds.iter().filter(|world| world.seed == 42).collect();
        assert_eq!(
            forty_two.len(),
            2,
            "one legacy file folds into its 16K twin"
        );
        assert!(
            forty_two
                .iter()
                .any(|world| (world.width, world.height) == (8_192, 4_096))
        );
        assert!(
            forty_two
                .iter()
                .any(|world| !world.legacy && world.width == 16_384)
        );

        let _ = std::fs::remove_dir_all(&dir);
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
