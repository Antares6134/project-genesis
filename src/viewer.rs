mod themes;

use crate::civilization::SocietyTier;
use crate::civilization::{Civilization, CivilizationWorld};
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::polar::{ICE_SHELF_TEMP, PACK_ICE_TEMP, frozen_sea_name, is_frozen_sea};
use crate::core::progress::{ProgressEvent, Sink};
use crate::core::{Grid, relief};
use crate::export::image::{
    HYPSOMETRIC_COLORS, HYPSOMETRIC_NAMES, get_atlas_biome_color, get_biome_name, hypsometric_band,
    hypsometric_color, hypsometric_range_label, id_to_territory_color,
};
use crate::geopolitics::GeopoliticalHierarchy;
use crate::language::LanguagesWorld;
use crate::pipeline::{
    AtlasWorld, CachedWorld, Genesis, LoadError, Stage, StageReport, StageState, cache_path,
    cached_world, cached_worlds,
};
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontFamily, FontId, Stroke, TextStyle, TextureHandle,
    TextureOptions,
};
use noise::{NoiseFn, OpenSimplex};
use rand::RngExt;
use rayon::prelude::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Instant, SystemTime};
use themes::{ClimateZone, TEMPERATURE_LEGEND_ROWS, TerrainClass};

/// How far the atlas can be zoomed out and in.
const MIN_ZOOM: f32 = 0.03;
const MAX_ZOOM: f32 = 8.0;
/// Zoom used when the reader asks the atlas to fly to a civilization's cradle.
/// A realm is a country, not a planet: the whole world stays in view, just
/// closer than the fit that shows everything.
const CIV_FOCUS_ZOOM: f32 = 0.55;
/// Zoom used when flying to one of the world's relief extremes.
const RELIEF_FOCUS_ZOOM: f32 = 0.55;

// ─── TYPOGRAPHY ──────────────────────────────────────────────────────────────
//
// egui ships a type scale tuned for a 13 pt interface, with `TextStyle::Small`
// at 9 pt. The atlas marked every legend note, every caption and every secondary
// line of a hover card `.small()`, so the most informative text on screen was
// also the least readable — 9 pt is a size to lose in, not to read.
//
// The atlas therefore installs a scale of its own: one step up throughout, with
// a hard floor under the smallest size, plus the padding, hit-targets and tooltip
// metrics that larger type needs. Every piece of text in this file names one of
// the four sizes below, so the hierarchy can be re-tuned in a single place and
// can never quietly fall back to egui's defaults.

/// Captions: provenance, a qualifier, the note beside a legend swatch. The
/// atlas's smallest size — and a size meant to be read, not squinted at.
const TEXT_SMALL: f32 = 13.0;
/// The atlas's reading size: readouts, legend rows, every line of a hover card.
const TEXT_BODY: f32 = 15.0;
/// The title of a hover card. Smaller than a panel heading, so a card reads as a
/// card and never as a second panel lying over the map.
const TEXT_CARD_TITLE: f32 = 17.0;
/// The title of a panel, and of the atlas window itself.
const TEXT_HEADING: f32 = 20.0;
/// The width a hover card may claim: wide enough for a full sentence at
/// [`TEXT_BODY`], narrow enough to keep the map underneath in view.
const CARD_WIDTH: f32 = 430.0;

/// The width of a legend panel. The five thematic legends are all keys, so they
/// share one width; the dawn of history is the only one that lists societies by
/// name, and gets a little more room for it.
const LEGEND_WIDTH: f32 = 330.0;
const CIV_LEGEND_WIDTH: f32 = 350.0;

/// Install the atlas's type scale, spacing and metrics on one [`egui::Style`].
///
/// This is pushed to *both* themes (see `MapViewer::install_style`), because
/// egui builds tooltips and popups from the context's style rather than from the
/// ui that opened them: a hover card has to come out at exactly these sizes
/// whether it belongs to a panel or to the canvas.
pub fn configure_style(style: &mut egui::Style) {
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(TEXT_SMALL, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(TEXT_BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(TEXT_BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(TEXT_SMALL + 1.0, FontFamily::Monospace),
        ),
        (
            TextStyle::Heading,
            FontId::new(TEXT_HEADING, FontFamily::Proportional),
        ),
    ]
    .into();

    let spacing = &mut style.spacing;
    spacing.item_spacing = egui::vec2(9.0, 6.0);
    spacing.button_padding = egui::vec2(10.0, 5.0);
    spacing.interact_size = egui::vec2(52.0, 24.0);
    spacing.indent = 22.0;
    spacing.slider_width = 120.0;
    // A tooltip is read, not skimmed, and it carries whole sentences: give the
    // text room inside its frame, and a width a sentence can breathe in.
    spacing.window_margin = egui::Margin::same(10);
    spacing.menu_margin = egui::Margin::same(10);
    spacing.tooltip_width = CARD_WIDTH + 40.0;
}

/// The atlas's panel frame: egui's stock panel frame with the inner margin the
/// larger type needs (the stock one is sized for a 13 pt interface).
fn atlas_panel_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin::symmetric(12, 5))
}

/// A panel title: the name of a legend panel, or of the atlas window itself.
fn panel_title(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).size(TEXT_HEADING).strong());
}

/// A card title: the name of whatever a hover card, or a legend entry, describes.
fn card_title(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(
        egui::RichText::new(text.into())
            .size(TEXT_CARD_TITLE)
            .strong(),
    );
}

/// A caption: provenance, a qualifier, a sentence of context.
fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).small().weak());
}

/// One line of the status footer.
///
/// Wraps rather than running off the edge: a status bar that hides the last
/// figure of a census is worse than one that takes two lines to show it.
fn status_line(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text.into()).small()).wrap());
}

/// What political domain the atlas is currently portraying.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum MapScope {
    /// Pure physical relief — no political tinting at all.
    Physical,
    /// Hypsometric survey: terrain colour *is* elevation, above and below water.
    Topographic,
    /// Climate zones: land classified by temperature and rainfall, sea by its
    /// surface temperature.
    Climate,
    /// Thermal belts: mean annual temperature, banded by isotherms.
    Temperature,
    /// Landform classes: coastal lowland to high mountain, shelf to abyssal plain.
    Terrain,
    /// The dawn of history: the physical world with the civilizations on it.
    Civilizations,
    /// The world's language families and their languages.
    Languages,
    /// Political subdivision of the landmasses only.
    Land,
    /// Hydro-political subdivision of the world ocean only.
    Water,
    /// Both land and ocean political subdivisions at once.
    Everywhere,
}

impl MapScope {
    /// True for the scopes that portray a political hierarchy, which are the only
    /// ones the tier selector and the border toggle apply to.
    fn is_political(self) -> bool {
        matches!(
            self,
            MapScope::Land | MapScope::Water | MapScope::Everywhere
        )
    }

    /// True for the scope whose subject *is* the dawn civilizations.
    fn is_civilizations(self) -> bool {
        matches!(self, MapScope::Civilizations)
    }

    /// True for the scope whose subject is the world's languages.
    fn is_languages(self) -> bool {
        matches!(self, MapScope::Languages)
    }

    /// The act a scope needs before it has anything to portray.
    ///
    /// This is what keeps the atlas honest while the world is still being made:
    /// a scope is offered exactly when its subject exists, so a reader can never
    /// switch to a map of provinces that have not been drawn yet. Relief needs
    /// only the land; biomes and climate need the atmosphere; rivers need the
    /// water; the hierarchies need their own acts.
    fn min_stage(self) -> Stage {
        match self {
            // Pure elevation and bathymetry: the hypsometric scale and the
            // landform classes read nothing but the heightmap.
            MapScope::Topographic | MapScope::Terrain => Stage::Geology,
            // Biomes, ice and the climate zones need the atmosphere.
            MapScope::Physical | MapScope::Climate | MapScope::Temperature => Stage::Climate,
            MapScope::Land => Stage::Provinces,
            MapScope::Water | MapScope::Everywhere => Stage::HydroPolitics,
            MapScope::Civilizations => Stage::Civilization,
            MapScope::Languages => Stage::Languages,
        }
    }

    /// True once the world is old enough for this scope to be painted.
    fn available(self, world: Stage) -> bool {
        world.index() >= self.min_stage().index()
    }
}

/// Which tier of the (land or ocean) hierarchy is being shaded.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum MapTier {
    Provinces,
    Areas,
    Regions,
    Superregions,
    Continents,
}

pub struct MapViewer {
    /// The world being built, and the acts of creation not yet run.
    ///
    /// The atlas owns the pipeline: it is the thing that presses the buttons, so
    /// it is also the thing that holds the seed, the finished acts and the cache.
    genesis: Genesis,
    /// The world as it may be painted right now. `None` until act one lands,
    /// which is exactly what "zero data yet" means.
    world: Option<AtlasWorld>,
    /// The act running on its own thread, if one is.
    job: Option<StageJob>,
    /// The save running on its own thread, if one is.
    save_job: Option<SaveJob>,
    /// The seed as the reader typed it, which is not always a valid world.
    seed_text: String,
    /// A sentence about the last cache attempt, when there is one worth saying.
    cache_notice: Option<String>,
    /// Every world cache on disk as of the last scan.
    cached: Vec<CachedWorld>,
    /// True once the cache directory has been read at least once.
    cached_scanned: bool,
    /// Set when an act may have written a cache, so the list is read again.
    cached_dirty: bool,
    /// What each finished act produced, newest first.
    log: Vec<String>,
    /// Keep running the acts, one after another, until the world is finished.
    run_queue: bool,
    /// The low-resolution plate of the act in flight, painted from the grid that
    /// act is working on right now.
    live: Option<LivePlate>,
    scope: MapScope,
    tier: MapTier,
    texture: Option<TextureHandle>,
    zoom: f32,
    offset: egui::Vec2,
    show_borders: bool,
    /// Tint the civilizations of the dawn over *any* scope, not just their own.
    show_civilizations: bool,
    /// World-wide relief extremes, measured once whenever the heightmap changes
    /// so the topographic legend never has to re-scan the 8K heightmap every
    /// frame. The positions travel with the measurements so the legend can fly
    /// the reader there. `None` until there is a heightmap to measure.
    relief: Option<ReliefExtremes>,
    /// Set until the map has been fitted to the canvas for the first time, and
    /// again whenever the reader presses **Fit**.
    pending_fit: bool,
    /// A place the reader asked to be flown to (map pixel x, y and target zoom),
    /// applied once the canvas rectangle for the frame is known.
    pending_focus: Option<(f32, f32, f32)>,
    /// The last place the atlas flew to, ringed on the map so the reader can see
    /// where they landed.
    marker: Option<(f32, f32)>,
    /// The realm whose entry in the legend was last clicked.
    selected_civ: Option<u32>,
    /// The language family whose entry in the legend was last clicked.
    selected_family: Option<u16>,
    /// Whether the atlas's type scale has been installed on this frame's style.
    /// A style change makes egui rebuild its font atlas, so it is done once.
    style_installed: bool,
}

/// The live plate of an act in flight: its label, and the texture it is painted
/// into, which is replaced as the act reports new ground.
struct LivePlate {
    label: String,
    texture: TextureHandle,
}

/// Width of the genesis panel: wide enough for a seed, a stage title and a
/// percentage on one line, narrow enough to leave the world the window.
const GENESIS_PANEL_WIDTH: f32 = 372.0;

/// A hard ceiling on the genesis panel: whatever its content asks for, the map
/// keeps the rest of the window.
const GENESIS_PANEL_MAX: f32 = 520.0;

/// What a running act is doing, in the few words a caption has room for.
///
/// One spelling of it, used by the genesis panel, by the strip over the live
/// plate and by the plate's own caption — so the reader can never be told three
/// slightly different things about the same act.
fn stage_progress(job: Option<&StageJob>) -> String {
    match job {
        Some(job) if job.total > 0 => format!(
            "{} — {:.0}%",
            job.sub,
            100.0 * job.done as f32 / job.total as f32
        ),
        // A flood fill that pops until its heap is empty has no scale to be a
        // percentage of: it reports what it has drained so far instead.
        Some(job) => format!("{} — {}", job.sub, thousands(job.done as f32)),
        None => "working".to_string(),
    }
}

/// A cache's size in the unit a reader thinks in.
///
/// A world cache is hundreds of megabytes, so bytes would be a row of digits
/// that says less than "544 MB" does.
fn cache_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let megabytes = bytes as f64 / MB;
    if megabytes >= 1024.0 {
        format!("{:.1} GB", megabytes / 1024.0)
    } else {
        format!("{megabytes:.0} MB")
    }
}

/// How long ago a cache was written, in the few words a row has room for.
fn cache_age(modified: Option<SystemTime>) -> String {
    let Some(modified) = modified else {
        return "at an unknown time".to_string();
    };
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        // A clock that disagrees with the file's timestamp is not worth a wrong
        // age: say only that it is there.
        return "just now".to_string();
    };
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{} min ago", secs / 60),
        3_600..=86_399 => format!("{} h ago", secs / 3_600),
        86_400..=2_591_999 => format!("{} d ago", secs / 86_400),
        _ => format!("{} mo ago", secs / 2_592_000),
    }
}

/// Which acts a save holds, in the few words a row has room for: `acts 1–5`, or
/// `a saved world` when the file predates the header that names them — the acts
/// are then knowable only by decoding the payload, which the list does not do.
fn acts_saved(acts: Option<u8>) -> String {
    match acts {
        Some(acts) => format!("acts 1–{acts}"),
        None => "a saved world".to_string(),
    }
}

/// The scope the atlas opens on for a world of a given age.
///
/// A world is only ever shown as what it currently *is*. One act old, it is
/// relief and nothing else, and its survey sheet is the honest picture of it.
/// Two acts old it has weather, so the physical atlas — the atlas's own subject
/// — takes over. Four acts old it has provinces, which is what the reader has
/// just watched being drawn.
fn default_scope(stage: Stage) -> MapScope {
    let land = MapScope::Topographic;
    let physical = MapScope::Physical;
    let provinces = MapScope::Land;
    match stage {
        Stage::Geology | Stage::Climate | Stage::Hydrology => {
            if stage.available(Stage::Climate) {
                physical
            } else {
                land
            }
        }
        Stage::Provinces | Stage::Geopolitics | Stage::HydroPolitics => provinces,
        Stage::Civilization => MapScope::Civilizations,
        Stage::Languages => MapScope::Languages,
    }
}

/// An act running on its own thread, and everything the panel needs to show it.
struct StageJob {
    stage: Stage,
    /// Fractions and pictures from the act.
    progress: Receiver<ProgressEvent>,
    /// The pipeline, handed back when the act is done.
    finished: Receiver<(Box<Genesis>, StageReport)>,
    /// The world as it stood before this act started.
    ///
    /// An act runs on a worker that owns the pipeline, so if that worker dies —
    /// a bug, a panic, a failed allocation mid-stage — the world it was given
    /// dies with it. This is the way back: the atlas puts the *previous* world
    /// back, so a failed act leaves the pipeline exactly where it was rather
    /// than losing everything that had been grown so far. It costs a handful of
    /// `Arc` increments, because that is all a pipeline is.
    fallback: Genesis,
    started: Instant,
    /// The sub-step being reported, and how far into it the act has got.
    sub: &'static str,
    done: u64,
    total: u64,
}

/// A save running on its own thread: the seed and act count it was taken at, and
/// where it wrote or why it could not.
type SaveJob = Receiver<(u64, usize, Result<std::path::PathBuf, LoadError>)>;

/// The world's relief extremes, in metres, together with where they are.
#[derive(Clone, Copy, Debug)]
struct ReliefExtremes {
    peak_metres: f32,
    peak_x: f32,
    peak_y: f32,
    trench_metres: f32,
    trench_x: f32,
    trench_y: f32,
    ocean_share: f32,
}

/// Partial result of one lane of the relief scan.
#[derive(Clone, Copy)]
struct ReliefScan {
    peak_metres: f32,
    peak_index: usize,
    trench_metres: f32,
    trench_index: usize,
    ocean_px: usize,
}

impl ReliefScan {
    const START: Self = Self {
        peak_metres: f32::MIN,
        peak_index: usize::MAX,
        trench_metres: f32::MAX,
        trench_index: usize::MAX,
        ocean_px: 0,
    };

    /// Merge two lanes. Ties are broken by the lowest pixel index so the answer
    /// never depends on how the work happened to be split between threads.
    fn merge(self, other: Self) -> Self {
        let (peak_metres, peak_index) = if other.peak_metres > self.peak_metres
            || (other.peak_metres == self.peak_metres && other.peak_index < self.peak_index)
        {
            (other.peak_metres, other.peak_index)
        } else {
            (self.peak_metres, self.peak_index)
        };
        let (trench_metres, trench_index) = if other.trench_metres < self.trench_metres
            || (other.trench_metres == self.trench_metres && other.trench_index < self.trench_index)
        {
            (other.trench_metres, other.trench_index)
        } else {
            (self.trench_metres, self.trench_index)
        };
        Self {
            peak_metres,
            peak_index,
            trench_metres,
            trench_index,
            ocean_px: self.ocean_px + other.ocean_px,
        }
    }
}

/// Highest peak, deepest trench and ocean share of a heightmap, in metres, with
/// the pixel each extreme sits on.
fn measure_relief(heightmap: &Grid<f32>) -> ReliefExtremes {
    let scanned = heightmap
        .raw_slice()
        .par_iter()
        .enumerate()
        .fold(
            || ReliefScan::START,
            |acc, (index, &e)| {
                let m = relief::to_metres(e);
                ReliefScan {
                    peak_metres: acc.peak_metres.max(m),
                    peak_index: if m > acc.peak_metres {
                        index
                    } else {
                        acc.peak_index
                    },
                    trench_metres: acc.trench_metres.min(m),
                    trench_index: if m < acc.trench_metres {
                        index
                    } else {
                        acc.trench_index
                    },
                    ocean_px: acc.ocean_px + usize::from(e <= relief::SEA_LEVEL),
                }
            },
        )
        .reduce(|| ReliefScan::START, ReliefScan::merge);

    let total = heightmap.raw_slice().len().max(1);
    let width = heightmap.width.max(1);
    let at = |index: usize| -> (f32, f32) {
        let index = index.min(total - 1);
        ((index % width) as f32, (index / width) as f32)
    };
    let (peak_x, peak_y) = at(scanned.peak_index);
    let (trench_x, trench_y) = at(scanned.trench_index);

    ReliefExtremes {
        peak_metres: scanned.peak_metres,
        peak_x,
        peak_y,
        trench_metres: scanned.trench_metres,
        trench_x,
        trench_y,
        ocean_share: scanned.ocean_px as f32 / total as f32,
    }
}

/// `52.3° N · 14.8° E` — a map position, written the way an atlas writes it.
fn lat_lon_label(lat: f32, lon: f32) -> String {
    format!(
        "{:.1}° {} · {:.1}° {}",
        lat.abs(),
        if lat >= 0.0 { "N" } else { "S" },
        lon.abs(),
        if lon >= 0.0 { "E" } else { "W" },
    )
}

/// The ink a political plate is drawn in: a warm near-black, so a frontier reads as
/// a pen line rather than as a darker shade of the fill.
const CIV_INK: [f32; 3] = [54.0, 46.0, 40.0];

/// The sea of the Civilizations plate: flattened and pulled towards a pale slate.
///
/// The physical sea is nearly black in its deeps, which on every other scope is
/// exactly right — those plates are *about* the ocean. Here it is context, and left
/// alone it out-shouts the land it is supposed to frame.
#[inline(always)]
fn muted_sea(sea: [f32; 3]) -> [f32; 3] {
    const SLATE: [f32; 3] = [74.0, 104.0, 130.0];
    let mean = (sea[0] + sea[1] + sea[2]) / 3.0;
    // Flatten first: the bathymetric shading is texture the political plate does not
    // need, and it is what made the water look inked-in. What little survives keeps
    // a shelf faintly visible without turning the ocean into a second map.
    let flat = [
        mean + (sea[0] - mean) * 0.35,
        mean + (sea[1] - mean) * 0.35,
        mean + (sea[2] - mean) * 0.35,
    ];
    blend_rgb(flat, SLATE, 0.65)
}

/// Linear blend of two float colours, `t = 0` keeping `base`.
#[inline(always)]
fn blend_rgb(base: [f32; 3], over: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        base[0] * (1.0 - t) + over[0] * t,
        base[1] * (1.0 - t) + over[1] * t,
        base[2] * (1.0 - t) + over[2] * t,
    ]
}

#[inline(always)]
fn sample_3d_fbm(
    noise: &OpenSimplex,
    x: f64,
    y: f64,
    z: f64,
    octaves: usize,
    lacunarity: f64,
    persistence: f64,
) -> f32 {
    let mut total = 0.0f64;
    let mut amplitude = 1.0f64;
    let mut frequency = 1.0f64;
    let mut max_val = 0.0f64;
    for _ in 0..octaves {
        total += noise.get([x * frequency, y * frequency, z * frequency]) * amplitude;
        max_val += amplitude;
        amplitude *= persistence;
        frequency *= lacunarity;
    }
    (total / max_val) as f32
}

/// Resolve the id that a land or ocean province should be coloured by at the
/// requested tier. Both hierarchies share the same `ProvinceNode` shape, so a
/// single helper serves them.
#[inline(always)]
fn tier_color_id(hierarchy: &GeopoliticalHierarchy, id: u32, tier: MapTier) -> u32 {
    let node = &hierarchy.provinces[id as usize];
    match tier {
        MapTier::Provinces => node.id,
        MapTier::Areas => node.area_id,
        MapTier::Regions => node.region_id,
        MapTier::Superregions => node.superregion_id,
        MapTier::Continents => node.continent_id,
    }
}

/// Darken a land pixel that sits on a border of the currently displayed tier.
///
/// Only the right and down neighbours are inspected, which yields a clean
/// one-pixel frontier line; `id == 0` (coast or wilderness) always counts as a
/// border, exactly as before.
#[inline]
fn land_border_shade(
    mut color: [f32; 3],
    province_ids: Option<&Grid<u32>>,
    hierarchy: &GeopoliticalHierarchy,
    id: u32,
    color_id: u32,
    x: usize,
    y: usize,
    h: usize,
    tier: MapTier,
) -> [f32; 3] {
    // No province has been drawn yet, so there is no border to darken.
    let Some(province_ids) = province_ids else {
        return color;
    };
    let rx = province_ids.wrap_x(x as isize + 1) as isize;
    let right_id = *province_ids.get(rx, y as isize);
    let down_id = if y + 1 < h {
        *province_ids.get(x as isize, y as isize + 1)
    } else {
        id
    };

    let differs = |nid: u32| -> bool {
        if nid == id {
            return false;
        }
        if nid == 0 {
            return true; // Coast or wasteland
        }
        if (nid as usize) >= hierarchy.provinces.len() {
            return false;
        }
        tier_color_id(hierarchy, nid, tier) != color_id
    };

    if differs(right_id) || differs(down_id) {
        color[0] *= 0.55;
        color[1] *= 0.55;
        color[2] *= 0.55;
    }
    color
}

/// Darken an ocean pixel that sits on a border between two marine realms of the
/// currently displayed tier.
///
/// Coastlines are intentionally *not* drawn: the shore is already unmistakable,
/// and drawing it would trace every continent with a heavy outline. Only
/// province-to-province transitions within the ocean produce a frontier line.
#[inline]
fn ocean_border_shade(
    mut color: [f32; 3],
    ocean_province_ids: Option<&Grid<u32>>,
    ocean_hierarchy: &GeopoliticalHierarchy,
    ocean_id: u32,
    color_id: u32,
    x: usize,
    y: usize,
    h: usize,
    tier: MapTier,
) -> [f32; 3] {
    // No marine province has been drawn yet, so there is no border to darken.
    let Some(ocean_province_ids) = ocean_province_ids else {
        return color;
    };
    let rx = ocean_province_ids.wrap_x(x as isize + 1) as isize;
    let right_id = *ocean_province_ids.get(rx, y as isize);
    let down_id = if y + 1 < h {
        *ocean_province_ids.get(x as isize, y as isize + 1)
    } else {
        ocean_id
    };

    let differs = |nid: u32| -> bool {
        if nid == 0 || nid == ocean_id {
            return false;
        }
        if (nid as usize) >= ocean_hierarchy.provinces.len() {
            return false;
        }
        tier_color_id(ocean_hierarchy, nid, tier) != color_id
    };

    if differs(right_id) || differs(down_id) {
        color[0] *= 0.55;
        color[1] *= 0.55;
        color[2] *= 0.55;
    }
    color
}

/// Darken a pixel standing on a civilization's edge, or on an internal boundary
/// of the currently displayed tier.
///
/// Two weights, because the two lines mean different things. A realm's outer edge
/// is drawn hard — it is the one frontier history actually argued about — while the
/// subdivisions *inside* a realm are drawn faintly, exactly where the tier
/// selector says they belong. The overlay on other scopes draws the frontier only,
/// since those scopes already draw their own internal lines.
#[inline]
fn civ_border_shade(
    mut color: [f32; 3],
    province_ids: Option<&Grid<u32>>,
    hierarchy: &GeopoliticalHierarchy,
    civilizations: &CivilizationWorld,
    id: u32,
    x: usize,
    y: usize,
    h: usize,
    tier: MapTier,
    internal: bool,
) -> [f32; 3] {
    // No province has been drawn yet, so there is no frontier to draw either.
    let Some(province_ids) = province_ids else {
        return color;
    };
    let owner = civilizations.owner_of(id);
    let color_id = tier_color_id(hierarchy, id, tier);

    let rx = province_ids.wrap_x(x as isize + 1) as isize;
    let right_id = *province_ids.get(rx, y as isize);
    let down_id = if y + 1 < h {
        *province_ids.get(x as isize, y as isize + 1)
    } else {
        id
    };

    // Coast or wilderness always counts as the edge of the realm.
    let frontier = |nid: u32| -> bool { nid == 0 || civilizations.owner_of(nid) != owner };
    let internal_edge = |nid: u32| -> bool {
        if nid == 0 || nid == id || civilizations.owner_of(nid) != owner {
            return false;
        }
        if (nid as usize) >= hierarchy.provinces.len() {
            return false;
        }
        tier_color_id(hierarchy, nid, tier) != color_id
    };

    if frontier(right_id) || frontier(down_id) {
        // Ink, not a darkened fill: a frontier drawn by multiplying a pastel gives a
        // muddy grey line, and a frontier drawn in warm near-black reads as a border.
        // Two realms of one tier wear near-identical pastels, so the line has to be
        // dark enough to be read *between* them, not merely against the parchment.
        color = blend_rgb(color, CIV_INK, 0.75);
    } else if internal && (internal_edge(right_id) || internal_edge(down_id)) {
        // The subdivisions inside a realm stay faint: they are structure, not a
        // frontier, and the tier selector is what asks for them.
        color = blend_rgb(color, CIV_INK, 0.30);
    }
    color
}

/// Darken a pixel on a language or family frontier.
///
/// Two weights, because the two lines mean different things: a family frontier is
/// the hard edge between two unrelated tongues, while a language inside one family
/// is a softer dialect boundary. The coast is drawn lightly, so an island's speech
/// still reads as its own without a heavy outline around every shore.
#[inline]
fn language_border_shade(
    mut color: [f32; 3],
    province_ids: Option<&Grid<u32>>,
    languages: &LanguagesWorld,
    id: u32,
    x: usize,
    y: usize,
    h: usize,
) -> [f32; 3] {
    let Some(province_ids) = province_ids else {
        return color;
    };
    let right_id = *province_ids.get(province_ids.wrap_x(x as isize + 1) as isize, y as isize);
    let down_id = if y + 1 < h {
        *province_ids.get(x as isize, y as isize + 1)
    } else {
        id
    };
    let language = languages
        .province_language
        .get(id as usize)
        .copied()
        .unwrap_or(0);
    let family = languages
        .province_family
        .get(id as usize)
        .copied()
        .unwrap_or(0);

    let mut hard = false;
    let mut soft = false;
    for other in [right_id, down_id] {
        if other == id {
            continue;
        }
        if other == 0 {
            soft = true;
            continue;
        }
        let other_family = languages
            .province_family
            .get(other as usize)
            .copied()
            .unwrap_or(0);
        let other_language = languages
            .province_language
            .get(other as usize)
            .copied()
            .unwrap_or(0);
        if other_family != family {
            hard = true;
        } else if other_language != language {
            soft = true;
        }
    }

    if hard {
        color = blend_rgb(color, CIV_INK, 0.72);
    } else if soft {
        color = blend_rgb(color, CIV_INK, 0.28);
    }
    color
}

/// A partitioned world carries hundreds of societies, and building a widget for
/// every one of them every frame would cost more than the map it describes: the
/// legend lists the first few of each tier, in the order the atlas assembled them,
/// and says how many it left out.
const LISTED_PER_TIER: usize = 24;

/// A short dossier for one civilization, as the Civilizations map mode shows it
/// on hover: where it rose and why, how it lives, what it believes, and how many
/// people it holds. Deliberately a card, not a monograph — the full measurements
/// stay in the console report (`--civs`), where they belong.
fn civilization_dossier(
    ui: &mut egui::Ui,
    civ_id: u32,
    civ: &Civilization,
    world: &CivilizationWorld,
    languages: &LanguagesWorld,
) {
    // A hover card has to stay a card: at 430 px it never swallows the map.
    ui.set_max_width(CARD_WIDTH);

    ui.horizontal(|ui| {
        themes::swatch(ui, civ.color);
        card_title(ui, civ.name.as_str());
    });
    caption(
        ui,
        format!(
            "{} · {} — {}",
            civ.tier.label(),
            civ.rise.kind.label(),
            civ.culture.government.label()
        ),
    );
    caption(ui, civ.origin_label());
    if civ.founding_year.is_some() {
        caption(ui, civ.year_label());
    }

    ui.separator();
    caption(ui, civ.cradle_kind.describe());
    for highlight in civ.rise.highlights.iter().take(3) {
        ui.label(format!("• {highlight}"));
    }

    ui.separator();
    ui.label(
        egui::RichText::new(format!(
            "{} · {}",
            civ.culture.subsistence.label(),
            civ.culture.outlook.label()
        ))
        .strong(),
    );
    caption(
        ui,
        if civ.culture.government.is_state() {
            "A state"
        } else {
            "A pre-state society"
        },
    );

    ui.add_space(4.0);
    ui.label(egui::RichText::new(civ.faith.kind.label()).strong());
    caption(ui, civ.faith.kind.creed());

    if let (Some(language), Some(family)) = (
        languages.language_of_civ(civ_id),
        languages.family_of_civ(civ_id),
    ) {
        ui.separator();
        ui.horizontal(|ui| {
            themes::swatch(ui, family.color);
            ui.label(
                egui::RichText::new(format!(
                    "{} · {}",
                    family.name,
                    family.urheimat.archetype.label()
                ))
                .strong(),
            );
        });
        ui.label(format!("Language: {}", language.name));
        ui.label(format!("Autonym: {}", language.autonym));
        caption(
            ui,
            format!("Urheimat: {}", languages.urheimat_label(family.id)),
        );
    }

    ui.separator();
    ui.label(format!(
        "{} people · {} provinces · {} km²",
        thousands_u64(civ.population),
        civ.provinces.len(),
        thousands(civ.area_km2)
    ));
    if civ.contacts.is_empty() {
        caption(ui, "No neighbour within reach.");
    } else {
        let neighbours: Vec<&str> = civ
            .contacts
            .iter()
            .map(|contact| world.name_of(contact.other))
            .collect();
        caption(ui, format!("Neighbours: {}", neighbours.join(", ")));
    }
}

/// One row of the five-way key: a tier's colour, its name, and how much of the
/// world's inhabited land the tier holds.
///
/// The line is **wrapped, never widened**. A legend panel is painted *before* the
/// map canvas and is only ever clipped to itself, so a row wider than the panel
/// widens the panel's own frame: the map, painted afterwards, then covers the
/// overhang — and the reader loses the left end of every over-long row behind
/// the map. Keep every row of a legend inside the width it was given.
fn society_key_row(ui: &mut egui::Ui, tier: SocietyTier, count: usize, share: f32) {
    ui.horizontal(|ui| {
        themes::swatch(ui, tier.legend_color());
        // `.wrap()` is the whole point: egui lets a label in a horizontal row
        // extend past the row (a vertical layout wraps, a horizontal one does
        // not), and a wrapped line costs the row a second line instead of the
        // panel its left edge.
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "{} — {count} societies · {share:.1}% of the land",
                    tier.label(),
                ))
                .small(),
            )
            .wrap(),
        );
    })
    .response
    .on_hover_text(tier.describe());
}

/// One society's entry in the dawn legend: its colour and its name, as a button
/// that flies the map to the society's heartland. Returns `true` when the reader
/// clicked it.
///
/// A society's name is *data* — the atlas did not choose it — so the button
/// truncates rather than widen the row. Widening a row widens the panel's frame
/// (see [`society_key_row`]), and the map then covers the panel's left edge.
fn society_entry(ui: &mut egui::Ui, color: [u8; 3], name: &str, selected: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        themes::swatch(ui, color);
        clicked = ui
            .add(egui::Button::selectable(selected, egui::RichText::new(name).strong()).truncate())
            .on_hover_text("Click to fly to this society's heartland")
            .clicked();
    });
    clicked
}

impl MapViewer {
    /// The atlas as an application: it owns the pipeline and drives it.
    ///
    /// Whatever the pipeline already holds is shown at once — a world adopted
    /// with `--load`, or nothing at all — and every act still to be run is a
    /// button in the genesis panel.
    pub fn new(genesis: Genesis) -> Self {
        let world = genesis.atlas();
        Self::with_world(genesis, world)
    }

    /// An atlas over a world that is already built.
    ///
    /// This is what the headless exports use: the plates and the civilizations
    /// map are the very same painter the window uses, over a finished world and
    /// with no pipeline driving it.
    pub fn from_world(world: AtlasWorld) -> Self {
        let genesis = Genesis::new(0);
        Self::with_world(genesis, Some(world))
    }

    fn with_world(mut genesis: Genesis, world: Option<AtlasWorld>) -> Self {
        let relief = world.as_ref().map(|world| measure_relief(&world.heightmap));
        let seed_text = genesis.seed().to_string();
        // The panel opens ready to save, and the pipeline is told so at once: what
        // the checkbox says will happen and what act 3 will do are the same fact,
        // never two that happen to agree until someone touches the checkbox.
        genesis.set_autosave(true);
        // The first scope a world of this age is old enough to portray: relief
        // while the land is all there is, the physical atlas once the atmosphere
        // exists, and the provinces themselves once they have been drawn.
        let scope = default_scope(world.as_ref().map_or(Stage::Geology, |world| world.stage));
        Self {
            genesis,
            world,
            job: None,
            save_job: None,
            seed_text,
            cache_notice: None,
            cached: Vec::new(),
            cached_scanned: false,
            cached_dirty: true,
            log: Vec::new(),
            run_queue: false,
            live: None,
            scope,
            tier: MapTier::Regions,
            texture: None,
            // Replaced by a real fit on the first painted frame, once the window
            // (and therefore the canvas) has a size.
            zoom: 0.16,
            offset: egui::vec2(0.0, 0.0),
            show_borders: true,
            // The overlay is opt-in: the Physical scope stays pure relief until the
            // reader asks for the dawn on top of it. The Civilizations scope itself
            // always shows the realms, overlay or not.
            show_civilizations: false,
            relief,
            pending_fit: true,
            pending_focus: None,
            marker: None,
            selected_civ: None,
            selected_family: None,
            style_installed: false,
        }
    }

    // ─── RUNNING AN ACT ──────────────────────────────────────────────────────

    /// Start an act on its own thread.
    ///
    /// The window has to stay alive while the world is grown: the long acts take
    /// eight seconds to half a minute each, and a window frozen for half a minute
    /// is not an atlas. So the pipeline is handed to a worker thread, which
    /// reports its fractions and its pictures back down two channels while the
    /// atlas keeps painting whatever it already has.
    fn start_stage(&mut self, stage: Stage, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        // The pipeline moves to the worker. The atlas keeps its own `Arc` clones
        // of the world as it stands, which is what lets it go on painting a world
        // whose own copy is being mutated on another thread.
        let autosave = self.genesis.autosave();
        let mut genesis = Genesis::new(self.genesis.seed());
        std::mem::swap(&mut genesis, &mut self.genesis);
        // The pipeline left behind is a placeholder, but the panel still asks it
        // what it would do: without this the Save choice would flicker off for the
        // length of every act.
        self.genesis.set_autosave(autosave);
        let fallback = genesis.clone();

        let (progress_tx, progress_rx) = mpsc::channel::<ProgressEvent>();
        let (done_tx, done_rx) = mpsc::channel::<(Box<Genesis>, StageReport)>();

        let worker_ctx = ctx.clone();
        std::thread::spawn(move || {
            let wake = {
                let ctx = worker_ctx.clone();
                move || ctx.request_repaint()
            };
            let sink = Sink::new(progress_tx, wake);
            let report = genesis
                .run_stage(stage, Some(sink))
                .expect("the panel only offers acts that can be run");
            let _ = done_tx.send((Box::new(genesis), report));
            worker_ctx.request_repaint();
        });

        self.log.insert(
            0,
            format!("⏳ {}. {} — working…", stage.index(), stage.title()),
        );
        self.job = Some(StageJob {
            stage,
            progress: progress_rx,
            finished: done_rx,
            fallback,
            started: Instant::now(),
            sub: "starting up",
            done: 0,
            total: 0,
        });
    }

    /// Drain whatever the running act has reported since the last frame, and
    /// adopt it the moment it is finished.
    fn poll_worker(&mut self, ctx: &egui::Context) {
        self.poll_save();
        let Some(mut job) = self.job.take() else {
            return;
        };

        loop {
            match job.progress.try_recv() {
                Ok(ProgressEvent::SubStep { label, total }) => {
                    job.sub = label;
                    job.total = total;
                    job.done = 0;
                }
                Ok(ProgressEvent::Tick { done, total }) => {
                    job.done = done;
                    job.total = total;
                }
                Ok(ProgressEvent::Preview {
                    label,
                    width,
                    height,
                    rgba,
                }) => {
                    // A preview is a few hundred kilobytes, so it costs one
                    // texture upload — not one per pixel of an 8K world.
                    let image = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    let texture = ctx.load_texture("live_preview", image, TextureOptions::NEAREST);
                    self.live = Some(LivePlate {
                        label: label.to_string(),
                        texture,
                    });
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        match job.finished.try_recv() {
            Ok((genesis, report)) => {
                self.genesis = *genesis;
                self.finish_stage(&report);
                // "Run all remaining" keeps its promise: the next act starts now.
                if self.run_queue {
                    match self.genesis.ready() {
                        Some(next) => self.start_stage(next, ctx),
                        None => self.run_queue = false,
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                // Still working: ask for the next frame, so the bar moves and the
                // clock ticks even while the reader touches nothing.
                ctx.request_repaint();
                self.job = Some(job);
            }
            Err(TryRecvError::Disconnected) => {
                // The worker died before it could hand the world back, and took
                // its copy of the pipeline with it. The world it started from is
                // put back exactly as it was — nothing half-grown, nothing lost —
                // and the act can be pressed again.
                let stage = job.stage;
                self.genesis = job.fallback;
                self.world = self.genesis.atlas();
                self.live = None;
                self.texture = None;
                self.log.insert(
                    0,
                    format!(
                        "⚠ {}. {} did not finish — the world is as it was before it started",
                        stage.index(),
                        stage.title()
                    ),
                );
            }
        }
    }

    /// Adopt a finished act: its world, its figures, and the next act's turn.
    fn finish_stage(&mut self, report: &StageReport) {
        self.world = self.genesis.atlas();
        // Act three and act six may have written the world cache, so the list of
        // saved worlds can have changed, and the panel reads it again next frame.
        if self.genesis.autosave() {
            self.cached_dirty = true;
        }
        self.relief = self
            .world
            .as_ref()
            .map(|world| measure_relief(&world.heightmap));
        self.texture = None;
        self.live = None;
        self.selected_civ = None;
        self.selected_family = None;
        self.log.insert(
            0,
            format!(
                "✔ {}. {} — {:.1}s",
                report.stage.index(),
                report.stage.title(),
                report.secs
            ),
        );
        for line in &report.summary {
            self.log.insert(1, format!("      {line}"));
        }
        // A scope the act has just made possible becomes the atlas's subject:
        // the reader watched provinces being drawn, so provinces are what they
        // now see.
        if let Some(world) = &self.world {
            let wanted = default_scope(world.stage);
            if wanted != self.scope && self.scope.min_stage().index() <= world.stage.index() {
                self.scope = wanted;
            }
        }
    }

    // ─── THE WORLD'S SEED, AND ITS CACHE ─────────────────────────────────────

    /// Adopt the seed the reader typed.
    ///
    /// A seed is read by each act as it runs, so it is not part of an act already
    /// grown: changing it keeps the world that is on the canvas — its acts, its
    /// zoom, its scope and its log — and re-points only the acts still to come.
    /// Before act one there is nothing to keep, so a fresh seed simply starts a
    /// fresh world.
    fn apply_seed(&mut self) {
        let Ok(seed) = self.seed_text.trim().parse::<u64>() else {
            return;
        };
        if seed == self.genesis.seed() {
            return;
        }
        self.genesis.set_seed(seed);
        // A notice about the last seed's cache says nothing about this one.
        self.cache_notice = None;
        if self.world.is_none() {
            // Nothing grown yet: the plate is already blank, so a clean slate
            // costs nothing and is exactly what "a new world" means.
            self.world = self.genesis.atlas();
            self.forget_world_view();
            self.log.clear();
            self.log.insert(
                0,
                format!("🌱 A new world: seed {seed}. Press “1. Geology & Erosion” to grow it."),
            );
        } else {
            // The acts already grown stay exactly as they are; only the acts ahead
            // will grow from the new seed.
            self.log.insert(
                0,
                format!(
                    "🔀 Seed is now {seed} — the acts already grown stay; the next act grows from {seed}."
                ),
            );
        }
    }

    /// Draw a seed the world has never heard of.
    fn random_seed(&mut self) {
        let seed = rand::rng().random::<u64>() % 10_000_000_000;
        self.seed_text = seed.to_string();
        self.apply_seed();
    }

    /// Adopt the acts already saved for this seed.
    ///
    /// A load that finds nothing is not an error the reader can do anything about
    /// except be told: the panel says so, in a sentence that names the file and
    /// the one thing that would put one there.
    fn load_world(&mut self) {
        match self.genesis.load_cache() {
            Ok(()) => {
                self.world = self.genesis.atlas();
                self.relief = self
                    .world
                    .as_ref()
                    .map(|world| measure_relief(&world.heightmap));
                // A loaded world is past whichever act it was saved at, so the
                // atlas opens on it rather than on a blank plate.
                self.forget_world_view();
                let stage = self.genesis.stage().unwrap_or(Stage::Geology);
                self.scope = default_scope(stage);
                self.cache_notice = None;
                self.log.clear();
                // The save that was on disk is the one just adopted, and it says
                // how much of the world is already done.
                let (acts, next) = (stage.index(), stage.index() + 1);
                self.log.insert(
                    0,
                    format!(
                        "✔ Adopted the cached world for seed {} — acts 1–{acts} are already done; \
                         act {next} is next",
                        self.genesis.seed()
                    ),
                );
            }
            Err(LoadError::Missing(path)) => {
                self.cache_notice = Some(format!(
                    "No saved world for seed {} — '{}' is not there. Press Save below, \
                     or pick a world from Saved worlds below.",
                    self.genesis.seed(),
                    path.display()
                ));
            }
            Err(LoadError::Unreadable(path, why)) => {
                self.cache_notice = Some(format!(
                    "'{}' could not be read, so there is nothing to load ({why}).",
                    path.display()
                ));
            }
        }
    }

    /// Write the world as it stands, up to act six, without freezing the atlas.
    ///
    /// The pipeline is moved onto a worker — every grid is behind an `Arc`, so the
    /// clone copies no pixels — and the result is polled back on a later frame.
    /// Acts seven and eight are never written: a save taken after them still holds
    /// acts one to six.
    fn save_world(&mut self) {
        if self.save_job.is_some() {
            return;
        }
        let seed = self.genesis.seed();
        let acts = self.genesis.stage().map_or(0, |stage| stage.index());
        let genesis = self.genesis.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((seed, acts, genesis.save_cache()));
        });
        self.save_job = Some(rx);
    }

    /// Adopt a finished save: say what was written, or why it could not be.
    fn poll_save(&mut self) {
        let Some(rx) = self.save_job.as_ref() else {
            return;
        };
        let (seed, acts, outcome) = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.save_job = None;
                return;
            }
        };
        self.save_job = None;
        match outcome {
            Ok(path) => {
                self.cache_notice = None;
                self.log.insert(
                    0,
                    format!("✔ Saved acts 1–{acts} for seed {seed} to '{}'", path.display()),
                );
                self.cached_dirty = true;
            }
            Err(why) => {
                self.cache_notice = Some(format!("The world could not be saved ({why:?})."));
            }
        }
    }

    /// Re-read the cache directory, so the panel lists what is on disk now.
    fn refresh_cached(&mut self) {
        self.cached = cached_worlds();
        self.cached_scanned = true;
        self.cached_dirty = false;
    }

    /// Adopt a world the reader picked from the saved list.
    ///
    /// Picking *is* loading: the seed becomes the picked one and its cache is
    /// adopted at once, so there is no second press to discover. A file that
    /// vanished or cannot be read between the scan and the click still leaves
    /// the panel saying why.
    fn pick_cached_world(&mut self, seed: u64) {
        self.seed_text = seed.to_string();
        self.genesis.set_seed(seed);
        self.load_world();
    }

    /// Throw the world away and start again on the same seed.
    fn reset_world(&mut self) {
        let seed = self.genesis.seed();
        let autosave = self.genesis.autosave();
        self.genesis = Genesis::new(seed);
        // What the reader chose about the cache outlives the world it was chosen
        // for: a reset world is still a world that will be saved as it is made.
        self.genesis.set_autosave(autosave);
        self.world = None;
        self.forget_world_view();
        self.log.clear();
        self.cache_notice = None;
        self.log.insert(
            0,
            format!("↺ Seed {seed} cleared — press “1. Geology & Erosion” to grow it again."),
        );
    }

    /// Un-do an act and every act after it, so they may be grown again.
    ///
    /// The world is thrown back to the act before `stage`, which becomes the next
    /// one the panel offers. Nothing is regenerated here: the act is simply
    /// dropped, and pressing it again — optionally after typing a new seed into
    /// the field above — grows it afresh, along with everything that had followed
    /// it. The acts before it, and their artefacts, are left exactly as they were.
    fn rewind_world(&mut self, stage: Stage) {
        self.genesis.rewind_to(stage);
        self.world = self.genesis.atlas();
        self.relief = self
            .world
            .as_ref()
            .map(|world| measure_relief(&world.heightmap));
        self.forget_world_view();
        self.cache_notice = None;
        self.log.insert(
            0,
            format!(
                "↺ Un-done act {} and everything after it — press “{}. {}” to grow it again.",
                stage.index(),
                stage.index(),
                stage.title()
            ),
        );
    }

    /// Forget everything that belonged to the world just replaced.
    fn forget_world_view(&mut self) {
        self.texture = None;
        self.live = None;
        self.selected_civ = None;
        self.selected_family = None;
        self.marker = None;
        self.scope = default_scope(
            self.world
                .as_ref()
                .map_or(Stage::Geology, |world| world.stage),
        );
        self.pending_fit = true;
        self.run_queue = false;
        if self.world.is_none() {
            self.relief = None;
        }
    }

    /// The plate that stands where a world has not been grown yet.
    ///
    /// Zero data is a state the atlas has to be able to draw: a dark, empty
    /// planet with the one instruction that matters on it — and, while act one is
    /// running, the live plate of the continents assembling behind it.
    fn cold_start(&mut self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(9, 12, 22));

        // The live plate, if act one is part-way through: the world arriving.
        if let Some(live) = &self.live {
            let texture_size = live.texture.size_vec2();
            let scale =
                (rect.width() * 0.6 / texture_size.x).min(rect.height() * 0.6 / texture_size.y);
            let size = texture_size * scale;
            let at = egui::Rect::from_center_size(rect.center(), size);
            let mut mesh = egui::Mesh::with_texture(live.texture.id());
            mesh.add_rect_with_uv(
                at,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::from_rgb(210, 214, 224),
            );
            painter.add(egui::Shape::mesh(mesh));
            // The plate is the act's own picture, so it is captioned with what
            // that act is doing — and with the fact that it is still doing it,
            // because a growing world must never read as a finished one.
            painter.text(
                egui::pos2(rect.center().x, at.max.y + 18.0),
                Align2::CENTER_CENTER,
                format!("🔴 {} — {}", live.label, stage_progress(self.job.as_ref())),
                FontId::proportional(TEXT_BODY),
                Color32::from_rgb(196, 204, 224),
            );
            painter.text(
                egui::pos2(rect.center().x, at.max.y + 40.0),
                Align2::CENTER_CENTER,
                format!("Growing a world — seed {}", self.genesis.seed()),
                FontId::proportional(TEXT_BODY),
                Color32::from_rgb(150, 164, 190),
            );
            return;
        }

        let title = if self.job.is_some() {
            "Growing a world…"
        } else {
            "No world yet"
        };
        painter.text(
            rect.center() - egui::vec2(0.0, 18.0),
            Align2::CENTER_CENTER,
            title,
            FontId::proportional(TEXT_HEADING + 8.0),
            Color32::from_rgb(214, 224, 242),
        );
        painter.text(
            rect.center() + egui::vec2(0.0, 20.0),
            Align2::CENTER_CENTER,
            format!(
                "Seed {}. Every act of creation is a button in the Genesis panel: \
                 begin with 1. Geology & Erosion.",
                self.genesis.seed()
            ),
            FontId::proportional(TEXT_BODY),
            Color32::from_rgb(150, 164, 190),
        );
    }

    // ─── THE GENESIS PANEL ───────────────────────────────────────────────────
    //
    // Every act of creation is a button here, in the only order they can be run,
    // and the world is grown in front of the reader: a seed, then a load or a
    // save, then seven presses. Nothing is generated before it is asked for, and
    // nothing waits to be looked at.

    fn genesis_panel(&mut self, ui: &mut egui::Ui, frame: &egui::Frame) {
        let busy = self.job.is_some() || self.save_job.is_some();
        egui::Panel::left("genesis")
            .frame(frame.clone())
            .resizable(true)
            .default_size(GENESIS_PANEL_WIDTH)
            .min_size(320.0)
            .max_size(GENESIS_PANEL_MAX)
            .show(ui, |ui| {
                // The panel can hold a seed, three cache controls, eight acts and
                // the notebook; on a short window it has to scroll rather than
                // hide the last act behind the bottom edge.
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        panel_title(ui, "🧬 Genesis");
                        caption(
                            ui,
                            "A world is grown one act at a time. Watch each one as it happens.",
                        );
                        ui.separator();
                        self.seed_row(ui, busy);
                        ui.add_space(4.0);
                        self.world_data_rows(ui, busy);
                        ui.add_space(4.0);
                        ui.separator();
                        self.act_rows(ui, busy);
                        ui.separator();
                        self.act_controls(ui, busy);
                        ui.add_space(4.0);
                        self.notebook(ui);
                    });
            });
    }

    /// The seed: typed, drawn at random, and never editable while a world is
    /// being grown from it.
    fn seed_row(&mut self, ui: &mut egui::Ui, busy: bool) {
        ui.horizontal(|ui| {
            ui.label("Seed");
            let typed = ui
                .add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.seed_text).desired_width(140.0),
                )
                .changed();
            if ui
                .add_enabled(!busy, egui::Button::new("🎲"))
                .on_hover_text("Draw a seed the world has never seen")
                .clicked()
            {
                self.random_seed();
            } else if typed {
                self.apply_seed();
            }
        });

        // A seed is any whole number, and the only way to enter something that is
        // not one is to type it.
        if self.seed_text.trim().parse::<u64>().is_err() {
            caption(ui, "A seed is a whole number — try 42, or press 🎲.");
            return;
        }
        let config = self.genesis.world_config();
        caption(
            ui,
            format!(
                "Tectonics: {} plates · {:.0}% ocean · {} erosion droplets",
                config.num_plates,
                config.ocean_ratio * 100.0,
                thousands(config.erosion_droplets as f32)
            ),
        );
    }

    /// World data: loading and saving are commands, auto-save is a setting, and
    /// the list is what is actually on disk.
    ///
    /// Load and Save are buttons that do their one thing at once — Load adopts
    /// the seed's saved world, Save writes the world as it stands. The checkbox
    /// below them arms the automatic saves at act 3 and act 6, and every cache on
    /// disk is listed at the bottom.
    fn world_data_rows(&mut self, ui: &mut egui::Ui, busy: bool) {
        caption(ui, "World data");

        // Read the directory once, and again whenever an act may have written
        // to it: one `read_dir` per act, not one per frame.
        if self.cached_dirty || !self.cached_scanned {
            self.refresh_cached();
        }

        self.load_row(ui, busy);
        ui.add_space(6.0);
        self.save_row(ui, busy);
        ui.add_space(4.0);
        self.saved_worlds_list(ui, busy);
    }

    /// The load and save commands: adopt the cache belonging to the seed above, or
    /// write the world as it stands now.
    fn load_row(&mut self, ui: &mut egui::Ui, busy: bool) {
        // The field is the truth here, not the last seed the pipeline accepted:
        // an empty or half-typed seed must not look loadable.
        let typed = self.seed_text.trim().parse::<u64>().ok();
        let cache = typed.and_then(cached_world);
        let load_hint = if busy {
            "Wait for the act in flight to finish"
        } else if typed.is_none() {
            "A seed is a whole number — Load needs one"
        } else {
            "There is no cache for this seed yet"
        };

        // A save holds every act grown so far, capped at six, so it is offered
        // from act one to act six and withdrawn once the dawn is on the world.
        let stage = self.genesis.stage();
        let saveable = stage.is_some_and(|stage| stage.index() <= Stage::HydroPolitics.index());
        let save_hint = if busy {
            "Wait for the act in flight to finish"
        } else {
            "Grow the world first — there is nothing to save yet"
        };

        ui.horizontal(|ui| {
            let load = ui
                .add_enabled(cache.is_some() && !busy, egui::Button::new("📂 Load"))
                .on_hover_text(match &cache {
                    Some(cache) => format!(
                        "Read {} and pick the panel up where it left off",
                        cache_path(cache.seed).display()
                    ),
                    None => "Read this seed's saved world".to_string(),
                })
                .on_disabled_hover_text(load_hint);
            if load.clicked()
                && let Some(seed) = typed
            {
                self.genesis.set_seed(seed);
                self.load_world();
            }

            let save = ui
                .add_enabled(saveable && !busy, egui::Button::new("💾 Save"))
                .on_hover_text(
                    "Write every act grown so far, up to act 6, to \
                     saved_seeds/world_<seed>.bin",
                )
                .on_disabled_hover_text(save_hint);
            if save.clicked() {
                self.save_world();
            }
        });

        match (&cache, typed) {
            (Some(cache), _) => caption(
                ui,
                format!(
                    "seed {} · {} · saved {} · Load picks up where it left off",
                    cache.seed,
                    cache_size(cache.bytes),
                    cache_age(cache.modified)
                ),
            ),
            (None, Some(seed)) => caption(
                ui,
                format!("No saved world for seed {seed} yet — press Save and grow one."),
            ),
            (None, None) => caption(ui, "A seed is a whole number — Load needs one."),
        }

        if let Some(notice) = &self.cache_notice {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(notice)
                    .size(TEXT_SMALL)
                    .color(Color32::from_rgb(224, 156, 96)),
            );
        }
    }

    /// The save setting: whether act 3 and act 6 write the world cache.
    fn save_row(&mut self, ui: &mut egui::Ui, busy: bool) {
        let mut autosave = self.genesis.autosave();
        if ui
            .add_enabled(
                !busy,
                egui::Checkbox::new(
                    &mut autosave,
                    "💾 Save automatically when act 3 and act 6 land",
                ),
            )
            .on_hover_text(
                "Write every act grown so far — up to act 6 — to saved_seeds/world_<seed>.bin",
            )
            .changed()
        {
            self.genesis.set_autosave(autosave);
            self.cache_notice = None;
        }

        if !autosave {
            caption(ui, "Auto-save — off: the world is written only when you press Save.");
        } else {
            caption(
                ui,
                "Auto-save — on: written the moment act 3 lands, and again at act 6, \
                 overwriting the earlier save.",
            );
        }
    }

    /// Every world cache on disk, newest first, as a list to pick from.
    ///
    /// A click is a load: the seed field follows the pick and the cache is
    /// adopted at once. The list refreshes by itself after any act that may
    /// have written a cache, and on demand with **⟳ Rescan**.
    fn saved_worlds_list(&mut self, ui: &mut egui::Ui, busy: bool) {
        let mut refresh = false;
        let mut picked = None;
        let current = self.genesis.seed();

        let response =
            egui::CollapsingHeader::new(format!("📂 Saved worlds ({})", self.cached.len()))
                // The count is in the label, so the id must not be: otherwise a
                // newly saved world would change the id and snap the list shut.
                .id_salt("saved_worlds")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button("⟳ Rescan")
                            .on_hover_text("Read saved_seeds again")
                            .clicked()
                        {
                            refresh = true;
                        }
                        caption(ui, "Click a world to load it");
                    });

                    if self.cached.is_empty() {
                        caption(
                            ui,
                            "Nothing saved yet. Tick Save above and grow a world, and it will \
                             appear here.",
                        );
                        return;
                    }

                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for cache in &self.cached {
                                // A save holds whichever acts the world had grown
                                // when it was written, and its header names the
                                // deepest of them, so the row says `acts 1–5`.
                                let acts = acts_saved(cache.acts);
                                let line = format!(
                                    "{}   {} · {}  · {}",
                                    cache.seed,
                                    cache_size(cache.bytes),
                                    cache_age(cache.modified),
                                    acts
                                );
                                let hover = format!(
                                    "Load seed {} — {} · {} · {}\nPicks the panel up where the \
                                     save left off.",
                                    cache.seed,
                                    cache_size(cache.bytes),
                                    cache_age(cache.modified),
                                    acts
                                );
                                let button = egui::Button::selectable(cache.seed == current, line)
                                    .truncate();
                                if ui
                                    .add_enabled(!busy, button)
                                    .on_hover_text(hover)
                                    .on_disabled_hover_text("Wait for the act in flight to finish")
                                    .clicked()
                                {
                                    picked = Some(cache.seed);
                                }
                            }
                        });
                });

        // The header is also a question — "what is on disk now?" — so opening
        // or closing it rescans, and the next frame shows the answer.
        if response.header_response.clicked() {
            refresh = true;
        }

        if refresh {
            self.refresh_cached();
        }
        if let Some(seed) = picked {
            self.pick_cached_world(seed);
        }
    }

    /// The eight acts, in order: the one that can be run next is the only button
    /// that is pressable, and the one running wears its own progress bar.
    fn act_rows(&mut self, ui: &mut egui::Ui, busy: bool) {
        let running = self.job.as_ref().map(|job| job.stage);
        let mut pressed: Option<Stage> = None;
        let mut rewind: Option<Stage> = None;

        for stage in Stage::ALL {
            let state = self.genesis.state(stage);
            let is_running = running == Some(stage);
            // A world cannot be run past its own frontier: the acts after the next
            // one are locked, and greying them out is how the panel says so.
            let enabled = match state {
                StageState::Ready => !busy,
                StageState::Done { .. } | StageState::Locked => false,
            };

            ui.horizontal(|ui| {
                let mark = if is_running {
                    "⟳"
                } else {
                    match state {
                        StageState::Done { .. } => "✔",
                        StageState::Ready => "▶",
                        StageState::Locked => "▢",
                    }
                };
                let label = format!("{mark} {}. {}", stage.index(), stage.title());
                let button = egui::Button::new(label).frame(false);
                if ui
                    .add_enabled(enabled, button)
                    .on_hover_text(stage.blurb())
                    .clicked()
                {
                    pressed = Some(stage);
                }

                match state {
                    StageState::Done { secs } => {
                        status_line(
                            ui,
                            if secs > 0.0 {
                                format!("· {secs:.1}s")
                            } else {
                                // A stage adopted from a cache was not timed: it
                                // happened in another session.
                                "· loaded".to_string()
                            },
                        );
                        // A finished act can be un-done: this drops it and every
                        // act after it, so they may be grown again — optionally
                        // from a new seed typed into the field above.
                        if ui
                            .add_enabled(!busy, egui::Button::new("↺").frame(false))
                            .on_hover_text(format!(
                                "Redo from act {} — un-do it and everything after it",
                                stage.index()
                            ))
                            .on_disabled_hover_text("Wait for the act in flight to finish")
                            .clicked()
                        {
                            rewind = Some(stage);
                        }
                    }
                    StageState::Ready if !busy => {
                        status_line(ui, "· next");
                    }
                    _ => {}
                }
            });

            if is_running {
                self.act_progress(ui);
            }
        }

        if let Some(stage) = rewind {
            self.rewind_world(stage);
        }
        if let Some(stage) = pressed {
            let ctx = ui.ctx().clone();
            self.start_stage(stage, &ctx);
        }
    }

    /// The bar, the sub-step and the clock of the act being run.
    fn act_progress(&self, ui: &mut egui::Ui) {
        let Some(job) = self.job.as_ref() else { return };
        let elapsed = job.started.elapsed().as_secs_f32();

        // A bar fills the panel; it must never widen it. An infinite desired
        // width makes egui's Panel grow its frame — and the size it persists —
        // to the whole window, which collapses the map canvas to zero width.
        let bar_width = ui.available_width().max(96.0);
        let bar = if job.total > 0 {
            egui::ProgressBar::new(job.done as f32 / job.total as f32)
                .desired_width(bar_width)
                .text(format!(
                    "{:.0}%",
                    100.0 * job.done as f32 / job.total as f32
                ))
        } else {
            egui::ProgressBar::new(0.0)
                .desired_width(bar_width)
                .animate(true)
                .text(thousands(job.done as f32))
        };

        ui.add(bar);
        caption(
            ui,
            format!("{} · {elapsed:.0} s", stage_progress(self.job.as_ref())),
        );

        // A picture of the act, when it has one to show: the live plate is
        // painted from the very grid the act is working on.
        if let Some(live) = &self.live {
            caption(
                ui,
                format!("🔴 live: {} · {}", live.label, job.stage.title()),
            );
        }
    }

    /// Run the remaining acts in order, or throw the world away.
    fn act_controls(&mut self, ui: &mut egui::Ui, busy: bool) {
        let remaining = self.genesis.ready().is_some();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !busy && remaining,
                    egui::Button::new("▶▶ Run all remaining"),
                )
                .on_hover_text("Grow the whole world, act after act, without stopping")
                .clicked()
            {
                self.run_queue = true;
                let ctx = ui.ctx().clone();
                if let Some(next) = self.genesis.ready() {
                    self.start_stage(next, &ctx);
                }
            }

            if busy {
                if self.run_queue {
                    if ui
                        .button("⏹ Stop after this act")
                        .on_hover_text("The queue stops here; this act finishes")
                        .clicked()
                    {
                        self.run_queue = false;
                    }
                }
            } else if ui
                .add_enabled(self.world.is_some(), egui::Button::new("↺ Reset world"))
                .on_hover_text("Throw this world away and begin again on the same seed")
                .clicked()
            {
                self.reset_world();
            }
        });
    }

    /// What the acts have produced so far, newest first.
    fn notebook(&mut self, ui: &mut egui::Ui) {
        if self.log.is_empty() {
            caption(
                ui,
                "Nothing has been made yet. Press ▶ 1. Geology & Erosion to begin.",
            );
            return;
        }
        caption(ui, "What has been made");
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for line in &self.log {
                    caption(ui, line);
                }
            });
    }

    /// Install the atlas's type scale, once, on the context's styles — and on
    /// this frame's root ui, which egui built from the old style before we were
    /// called.
    ///
    /// Both are needed: egui builds tooltips, menus and every other popup from
    /// the *context's* style rather than from the ui that opened them, while the
    /// panels of this frame inherit the style of the root ui. Without the second
    /// half, the atlas's first frame — and every hover card, forever — would come
    /// out at egui's defaults while the panels came out at ours.
    fn install_style(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ctx.all_styles_mut(configure_style);
        let style = ctx.global_style();
        *ui.style_mut() = style.as_ref().clone();
        self.style_installed = true;
    }

    /// Fit the whole world into `rect`, centred, with a small margin.
    ///
    /// This is what the **Fit** button does — and what the viewer does once, on
    /// its first frame, so the atlas opens showing the entire planet whatever
    /// size the window happens to be.
    ///
    /// The world's pixel size is handed in rather than read from `self.world`:
    /// the frame borrows the world *out* of `self` (`frame_ui` takes it, so the
    /// painter can hold `&self` and `&AtlasWorld` at once), so `self.world` is
    /// `None` for the very frame in which a fit is applied.
    fn fit_to(&mut self, rect: egui::Rect, map_w: f32, map_h: f32) {
        let available = rect.size();
        if available.x < 64.0 || available.y < 64.0 {
            return; // the panel layout has not settled yet; try next frame
        }
        let margin = 12.0;
        let zoom_x = (available.x - margin * 2.0) / map_w;
        let zoom_y = (available.y - margin * 2.0) / map_h;
        self.zoom = zoom_x.min(zoom_y).clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = egui::vec2(
            (available.x - map_w * self.zoom) * 0.5,
            (available.y - map_h * self.zoom) * 0.5,
        );
    }

    /// Bring a map pixel to the centre of the canvas at `zoom`.
    fn focus_on(&mut self, rect: egui::Rect, x: f32, y: f32, zoom: f32) {
        if rect.width() < 64.0 || rect.height() < 64.0 {
            // Not laid out yet: remember the request and apply it next frame.
            self.pending_focus = Some((x, y, zoom));
            return;
        }
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = egui::vec2(
            rect.width() * 0.5 - x * self.zoom,
            rect.height() * 0.5 - y * self.zoom,
        );
        self.marker = Some((x, y));
    }

    /// Latitude and longitude, in degrees, of a map pixel.
    fn pixel_lat_lon(&self, x: f32, y: f32) -> (f32, f32) {
        let (width, height) = self
            .world
            .as_ref()
            .map_or((8192, 4096), |world| (world.width, world.height));
        let v = (y / (height.saturating_sub(1)).max(1) as f32).clamp(0.0, 1.0);
        let u = (x / width.max(1) as f32).clamp(0.0, 1.0);
        ((0.5 - v) * 180.0, (u - 0.5) * 360.0)
    }

    /// Select the scope, tier and border style the atlas portrays.
    ///
    /// Lets a headless caller render any scope × tier combination without ever
    /// opening the interactive window.
    pub fn set_view(&mut self, scope: MapScope, tier: MapTier, show_borders: bool) {
        self.scope = scope;
        self.tier = tier;
        self.show_borders = show_borders;
    }

    /// Render the current scope/tier straight to a PNG, headlessly.
    ///
    /// This is the very same painter the interactive window uses, so a plate
    /// written here is pixel-identical to what the viewer puts on screen. The
    /// plate is downscaled by `scale` (e.g. `0.25` for a 2K plate) so exported
    /// images stay a usable size.
    pub fn save_png(&self, path: &str, scale: f32) -> Result<(), image::ImageError> {
        let world = self
            .world
            .as_ref()
            .expect("a headless plate needs a world to paint");
        let img = self.generate_texture(world);
        let (w, h) = (img.size[0] as u32, img.size[1] as u32);
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for px in &img.pixels {
            buf.extend_from_slice(&px.to_array());
        }
        let full =
            image::RgbaImage::from_raw(w, h, buf).expect("RGBA buffer sized to the atlas texture");
        let plate = if (scale - 1.0).abs() < f32::EPSILON {
            full
        } else {
            let out_w = ((w as f32 * scale).round() as u32).max(1);
            let out_h = ((h as f32 * scale).round() as u32).max(1);
            image::imageops::resize(&full, out_w, out_h, image::imageops::FilterType::Lanczos3)
        };
        plate.save(path)
    }

    fn generate_texture(&self, world: &AtlasWorld) -> ColorImage {
        let w = world.width;
        let h = world.height;
        let scope = self.scope;
        let tier = self.tier;
        let show_borders = self.show_borders;
        let hierarchy = &world.hierarchy;
        let ocean_hierarchy = &world.ocean_hierarchy;
        let province_ids = world.provinces.as_ref().map(|p| &p.province_ids);
        let ocean_province_ids = world.ocean.as_ref().map(|o| &o.ocean_province_ids);
        let is_wasteland = world.provinces.as_ref().map(|p| &p.is_wasteland);

        let land_political = matches!(scope, MapScope::Land | MapScope::Everywhere);
        let water_political = matches!(scope, MapScope::Water | MapScope::Everywhere);
        let civ_scope = scope.is_civilizations();
        let language_scope = scope.is_languages();
        // The civilizations' own scope always shows them; every other scope shows
        // them only when the reader asks for the overlay.
        let show_civs = (self.show_civilizations && !language_scope) || civ_scope;
        let civilizations = &world.civilizations;
        let languages = &world.languages;

        let heightmap = &world.heightmap;
        // Which of the world's other grids exist depends on how old it is: the
        // atmosphere lands at act two and the water at act three, and until then
        // the scopes that would read them are not offered at all (see
        // `MapScope::min_stage`). A world of one act is painted as the land it is.
        let climate_simulated = world.stage.available(Stage::Climate);
        let temp_grid = world.temperature.as_deref();
        let precip_grid = world.precipitation.as_deref();
        let flow_grid = world.flow.as_deref();
        let is_lake = world.is_lake.as_deref();

        let sea_level = relief::SEA_LEVEL;
        let min_river_flow = 1800.0f32;
        let major_river_flow = 15000.0f32;
        let lx = -0.7071f32;
        let ly = -0.7071f32;
        let render_noise = OpenSimplex::new(777);

        let mut pixels = vec![0u8; w * h * 4];

        pixels
            .par_chunks_exact_mut(4)
            .enumerate()
            .for_each(|(i, pixel)| {
                let x = i % w;
                let y = i / w;
                let elev = *heightmap.get(x as isize, y as isize);
                // A grid the world is too young to have yet reads as its own absence:
                // no province claims a pixel before act four, no river runs before act
                // three, no rain falls before act two. The scopes that would paint
                // those are not offered until they exist (`MapScope::min_stage`), so
                // these defaults are never something a reader can see.
                let id = province_ids.map_or(0, |ids| *ids.get(x as isize, y as isize));
                let ocean_id = ocean_province_ids.map_or(0, |ids| *ids.get(x as isize, y as isize));
                let lake_here = is_lake.is_some_and(|lake| *lake.get(x as isize, y as isize));
                let waste_here = is_wasteland.is_some_and(|w| *w.get(x as isize, y as isize));

                // Coordinates on 3D sphere for seamless spherical noise
                let v = y as f32 / (h - 1) as f32;
                let lat = (0.5 - v) * std::f32::consts::PI;
                let u = x as f32 / w as f32;
                let lon = (u - 0.5) * std::f32::consts::TAU;
                let px = (lat.cos() * lon.cos()) as f64;
                let py = lat.sin() as f64;
                let pz = (lat.cos() * lon.sin()) as f64;

                let micro_noise =
                    sample_3d_fbm(&render_noise, px * 35.0, py * 35.0, pz * 35.0, 4, 2.0, 0.5);
                let temp = temp_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize));
                let precip = precip_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize));
                let display_temp = temp + micro_noise * 5.5;

                let (gx, gy) = heightmap.gradient(x as f32, y as f32);
                let slope = (gx * gx + gy * gy).sqrt();
                let hillshade = -(gx * lx + gy * ly);
                let mut shade = (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38);
                if y < 2 || y > h.saturating_sub(3) {
                    shade = 1.0;
                }

                // ── Physical pixel computation ────────────────────────────────
                let compute_physical_color = || -> [f32; 3] {
                    if elev <= sea_level {
                        if display_temp < -8.0 {
                            [245.0, 248.0, 255.0] // Glacial ice shelf
                        } else if display_temp < -3.0 {
                            [165.0, 205.0, 235.0] // Polar sea ice
                        } else {
                            let depth = (elev / sea_level).clamp(0.0, 1.0);
                            let shelf = depth.powf(2.8);
                            let sea_shade = (1.0 + hillshade * 2.2).clamp(0.85, 1.15);
                            [
                                (12.0 * (1.0 - shelf) + 45.0 * shelf) * sea_shade,
                                (35.0 * (1.0 - shelf) + 125.0 * shelf) * sea_shade,
                                (75.0 * (1.0 - shelf) + 185.0 * shelf) * sea_shade,
                            ]
                        }
                    } else if !climate_simulated {
                        // Act one: the land exists and nothing else does. It is
                        // painted as the bare relief it is — the hypsometric tints
                        // under the world's own key light — rather than through a
                        // biome palette that would have to be told the temperature
                        // and rainfall of a world that has no weather yet.
                        let band = hypsometric_color(relief::to_metres(elev));
                        [
                            band[0] as f32 * shade,
                            band[1] as f32 * shade,
                            band[2] as f32 * shade,
                        ]
                    } else if lake_here {
                        [42.0, 125.0, 185.0]
                    } else {
                        let altitude = elev - sea_level;
                        let base = get_atlas_biome_color(display_temp, precip, altitude);
                        let mut color = [
                            (base[0] as f32 * shade).clamp(0.0, 255.0),
                            (base[1] as f32 * shade).clamp(0.0, 255.0),
                            (base[2] as f32 * shade).clamp(0.0, 255.0),
                        ];

                        let flow = flow_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize));
                        if flow >= min_river_flow {
                            let river_strength = ((flow - min_river_flow) / major_river_flow)
                                .sqrt()
                                .clamp(0.30, 1.0);
                            let blend = (river_strength * 1.4).clamp(0.0, 1.0);
                            color = [
                                color[0] * (1.0 - blend) + 20.0 * blend,
                                color[1] * (1.0 - blend) + 75.0 * blend,
                                color[2] * (1.0 - blend) + 155.0 * blend,
                            ];
                        }
                        color
                    }
                };

                // ── Topographic (hypsometric) pixel computation ───────────────
                let compute_topographic_color = || -> [f32; 3] {
                    let metres = relief::to_metres(elev);
                    let band = hypsometric_color(metres);
                    let mut color = [band[0] as f32, band[1] as f32, band[2] as f32];

                    if lake_here && elev > sea_level {
                        color = [42.0, 125.0, 185.0];
                    } else if elev <= sea_level {
                        // Frozen seas read as "polar land": drifting pack ice over
                        // the shelf, solid ice shelf deeper towards the pole. The
                        // bathymetry stays faintly visible underneath.
                        if display_temp < ICE_SHELF_TEMP {
                            color = blend_rgb(color, [246.0, 249.0, 255.0], 0.90);
                        } else if display_temp < PACK_ICE_TEMP {
                            color = blend_rgb(color, [222.0, 236.0, 250.0], 0.70);
                        }
                    } else {
                        color = [color[0] * shade, color[1] * shade, color[2] * shade];
                        // Ice-capped land keeps its relief readable under a
                        // translucent veneer instead of blanking the survey out.
                        if display_temp < -3.0 {
                            color = blend_rgb(color, [238.0, 244.0, 252.0], 0.75);
                        }
                        let flow = flow_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize));
                        if flow >= min_river_flow {
                            let river_strength = ((flow - min_river_flow) / major_river_flow)
                                .sqrt()
                                .clamp(0.30, 1.0);
                            let blend = (river_strength * 1.4).clamp(0.0, 1.0);
                            color = blend_rgb(color, [20.0, 75.0, 155.0], blend);
                        }
                    }

                    // A contour line along every hypsometric band boundary, the way
                    // a topographic sheet steps between its elevation tints.
                    let band_idx = hypsometric_band(metres);
                    let right = hypsometric_band(relief::to_metres(
                        *heightmap.get(x as isize + 1, y as isize),
                    ));
                    let down = if y + 1 < h {
                        hypsometric_band(relief::to_metres(
                            *heightmap.get(x as isize, y as isize + 1),
                        ))
                    } else {
                        band_idx
                    };
                    if right != band_idx || down != band_idx {
                        color = [color[0] * 0.62, color[1] * 0.62, color[2] * 0.62];
                    }
                    color
                };

                let mut final_color: [f32; 3];

                if scope == MapScope::Physical {
                    // Pure physical atlas rendering
                    final_color = compute_physical_color();
                } else if scope == MapScope::Topographic {
                    // Hypsometric survey: colour is elevation, above and below water
                    final_color = compute_topographic_color();
                } else if scope == MapScope::Climate {
                    // ── Climate zones ────────────────────────────────────────
                    // Land is classified by mean temperature, rainfall and altitude;
                    // the sea by its surface temperature. A little of the hillshade is
                    // kept so the reader can still see where the mountains are.
                    let lake = lake_here;
                    let base: [u8; 3] = if lake {
                        [58, 138, 178]
                    } else if elev <= sea_level {
                        themes::ocean_climate_color(temp)
                    } else {
                        ClimateZone::classify(display_temp, precip, elev - sea_level).color()
                    };
                    let relief_tint = if elev > sea_level && !lake {
                        (0.88 + 0.14 * shade).clamp(0.82, 1.08)
                    } else {
                        1.0
                    };
                    final_color = [
                        (base[0] as f32 * relief_tint).clamp(0.0, 255.0),
                        (base[1] as f32 * relief_tint).clamp(0.0, 255.0),
                        (base[2] as f32 * relief_tint).clamp(0.0, 255.0),
                    ];
                } else if scope == MapScope::Temperature {
                    // ── Thermal belts ────────────────────────────────────────
                    // The mean annual temperature as a number line: violet cold, red
                    // hot, with a thin dark isotherm wherever the 5 °C band changes.
                    let band = themes::isotherm_band(temp);
                    let base = themes::temperature_color(temp);
                    let right_band = themes::isotherm_band(
                        temp_grid.map_or(0.0, |grid| *grid.get(x as isize + 1, y as isize)),
                    );
                    let down_band = if y + 1 < h {
                        themes::isotherm_band(
                            temp_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize + 1)),
                        )
                    } else {
                        band
                    };
                    let on_isotherm = right_band != band || down_band != band;
                    let tint = if on_isotherm { 0.66 } else { 1.0 };
                    let mut color = [
                        (base[0] as f32 * tint).clamp(0.0, 255.0),
                        (base[1] as f32 * tint).clamp(0.0, 255.0),
                        (base[2] as f32 * tint).clamp(0.0, 255.0),
                    ];

                    // Temperature is continuous, so the plate would be an unreadable
                    // gradient without a shoreline: draw it as a thin dark line.
                    let here_land = elev > sea_level;
                    let right_land = *heightmap.get(x as isize + 1, y as isize) > sea_level;
                    let down_land = if y + 1 < h {
                        *heightmap.get(x as isize, y as isize + 1) > sea_level
                    } else {
                        here_land
                    };
                    if here_land != right_land || here_land != down_land {
                        color = blend_rgb(color, [46.0, 40.0, 54.0], 0.55);
                    }
                    final_color = color;
                } else if scope == MapScope::Terrain {
                    // ── Landform classes ─────────────────────────────────────
                    // One flat colour per landform band, shaded enough to keep the
                    // ridges readable, with the rivers drawn as on the physical atlas.
                    let lake = lake_here;
                    let metres = relief::to_metres(elev);
                    let class = TerrainClass::classify(metres, lake, display_temp);
                    let base = class.color();
                    let mut color = [base[0] as f32, base[1] as f32, base[2] as f32];
                    if elev > sea_level && !lake && class != TerrainClass::Ice {
                        let relief_tint = (0.86 + 0.16 * shade).clamp(0.80, 1.10);
                        color = [
                            color[0] * relief_tint,
                            color[1] * relief_tint,
                            color[2] * relief_tint,
                        ];
                        let flow = flow_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize));
                        if flow >= min_river_flow {
                            let river_strength = ((flow - min_river_flow) / major_river_flow)
                                .sqrt()
                                .clamp(0.30, 1.0);
                            color = blend_rgb(color, [20.0, 75.0, 155.0], river_strength);
                        }
                    }
                    final_color = color;
                } else if civ_scope {
                    // ── Civilizations scope ────────────────────────────────
                    // The land is the subject, so the plate is built the way a political
                    // atlas is printed: a small ordered palette of flat fills, ink on the
                    // frontiers and the coast, the sea pushed back to a pale slate, and
                    // the relief carried *inside* each fill rather than mixed into it, so
                    // a realm keeps one hue from its coast to its mountains.
                    let lake = lake_here;
                    let is_ocean = elev <= sea_level && !lake;
                    let is_waste = waste_here || (id == 0 && elev > sea_level && !lake);
                    let owner = civilizations.owner_of(id);
                    let has_land_node = id != 0 && (id as usize) < hierarchy.provinces.len();

                    // Nobody's ground: a flat parchment, colder where the ice lies.
                    let parchment = |relief: f32| -> [f32; 3] {
                        let phys = compute_physical_color();
                        if display_temp < ICE_SHELF_TEMP {
                            return blend_rgb(phys, [227.0, 236.0, 247.0], 0.55);
                        }
                        let grey = (phys[0] + phys[1] + phys[2]) / 3.0;
                        [
                            (grey * 0.45 + 214.0 * 0.55) * relief,
                            (grey * 0.45 + 207.0 * 0.55) * relief,
                            (grey * 0.45 + 188.0 * 0.55) * relief,
                        ]
                    };

                    if is_ocean {
                        final_color = muted_sea(compute_physical_color());
                    } else if lake {
                        final_color = [92.0, 150.0, 186.0];
                    } else if owner != 0 && has_land_node {
                        let civ_rgb = civilizations.color_of(owner);
                        // Relief multiplies the fill instead of diluting it: the hue stays
                        // exactly the realm's own across the whole of its country.
                        let relief = (0.90 + 0.20 * (shade - 1.0)).clamp(0.82, 1.10);
                        let mut color = [
                            civ_rgb[0] as f32 * relief,
                            civ_rgb[1] as f32 * relief,
                            civ_rgb[2] as f32 * relief,
                        ];
                        // Ice-capped country keeps its political colour under a thin
                        // veneer rather than blanking the survey out.
                        if display_temp < ICE_SHELF_TEMP {
                            color = blend_rgb(color, [227.0, 236.0, 247.0], 0.45);
                        }
                        final_color = color;

                        // The cradle itself: a small dark dot with a pale halo, so it
                        // reads on a hunter-gatherer khaki as well as on an oxblood state.
                        if let Some(civ) = civilizations.civilization(owner) {
                            let dx = {
                                let mut d = (x as f32 - civ.seat_x).abs();
                                let width = w as f32;
                                if d > width * 0.5 {
                                    d = width - d;
                                }
                                d
                            };
                            let dy = (y as f32 - civ.seat_y).abs();
                            if dx.max(dy) <= 1.0 {
                                final_color = blend_rgb(final_color, [26.0, 20.0, 16.0], 0.85);
                            } else if dx.max(dy) <= 2.0 {
                                final_color = blend_rgb(final_color, [244.0, 240.0, 230.0], 0.30);
                            }
                        }

                        // Rivers are a watermark here, not a subject: enough to tie the
                        // political patches to the land, faint enough not to muddy them.
                        let flow = flow_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize));
                        if flow >= min_river_flow {
                            let river_strength = ((flow - min_river_flow) / major_river_flow)
                                .sqrt()
                                .clamp(0.30, 1.0);
                            let blend = (river_strength * 0.55).clamp(0.0, 0.55);
                            final_color = blend_rgb(final_color, [36.0, 92.0, 150.0], blend);
                        }

                        if show_borders {
                            // Draw the shoreline itself: a plate whose polities run
                            // straight into the sea reads as a diagram, not a map.
                            let touching_sea = |nx: isize, ny: isize| {
                                *heightmap.get(nx, ny) <= sea_level
                                    && !is_lake.is_some_and(|lake| *lake.get(nx, ny))
                            };
                            if touching_sea(x as isize + 1, y as isize)
                                || touching_sea(x as isize - 1, y as isize)
                                || touching_sea(x as isize, y as isize + 1)
                                || touching_sea(x as isize, y as isize - 1)
                            {
                                final_color = blend_rgb(final_color, CIV_INK, 0.32);
                            }
                            // Realm frontiers, and the subdivisions *inside* a realm at
                            // the tier the selector names — drawn as line-work only. The
                            // fill stays the realm's own colour, so a country never
                            // shatters into a patchwork of shades.
                            final_color = civ_border_shade(
                                final_color,
                                province_ids,
                                hierarchy,
                                civilizations,
                                id,
                                x,
                                y,
                                h,
                                tier,
                                true,
                            );
                        }
                    } else {
                        // Wilderness and wasteland: one parchment, so the ink of the
                        // polities is the only thing that draws the eye.
                        final_color = parchment((0.96 + 0.10 * (shade - 1.0)).clamp(0.90, 1.04));

                        // A dark rim on the wilderness side of every frontier, so even a
                        // single-province realm reads as a place rather than a smudge.
                        if show_borders && !is_waste {
                            let holds = |nid: u32| nid != 0 && civilizations.owner_of(nid) != 0;
                            let right = province_ids.map_or(0, |ids| {
                                *ids.get(ids.wrap_x(x as isize + 1) as isize, y as isize)
                            });
                            let down = if y + 1 < h {
                                province_ids.map_or(0, |ids| *ids.get(x as isize, y as isize + 1))
                            } else {
                                0
                            };
                            if holds(right) || holds(down) {
                                final_color = blend_rgb(final_color, CIV_INK, 0.42);
                            }
                        }
                    }
                } else if language_scope {
                    // ── Languages scope ────────────────────────────────────
                    // One flat colour per family, a shade per language, relief
                    // carried inside the fill so a tongue keeps its hue from coast
                    // to mountain.
                    let lake = lake_here;
                    let is_ocean = elev <= sea_level && !lake;
                    if is_ocean {
                        final_color = muted_sea(compute_physical_color());
                    } else if lake {
                        final_color = [92.0, 150.0, 186.0];
                    } else if id != 0 {
                        let language_id = languages
                            .province_language
                            .get(id as usize)
                            .copied()
                            .unwrap_or(0);
                        if language_id != 0 {
                            let language = languages.color_of_language(language_id);
                            let relief = (0.90 + 0.20 * (shade - 1.0)).clamp(0.82, 1.10);
                            final_color = [
                                language[0] as f32 * relief,
                                language[1] as f32 * relief,
                                language[2] as f32 * relief,
                            ];
                            if show_borders {
                                final_color = language_border_shade(
                                    final_color,
                                    province_ids,
                                    languages,
                                    id,
                                    x,
                                    y,
                                    h,
                                );
                            }
                        } else {
                            let phys = compute_physical_color();
                            let grey = (phys[0] + phys[1] + phys[2]) / 3.0;
                            final_color = [
                                grey * 0.45 + 214.0 * 0.55,
                                grey * 0.45 + 207.0 * 0.55,
                                grey * 0.45 + 188.0 * 0.55,
                            ];
                        }
                    } else {
                        let phys = compute_physical_color();
                        let grey = (phys[0] + phys[1] + phys[2]) / 3.0;
                        final_color = [
                            grey * 0.45 + 214.0 * 0.55,
                            grey * 0.45 + 207.0 * 0.55,
                            grey * 0.45 + 188.0 * 0.55,
                        ];
                    }
                } else {
                    // ── Political map modes ────────────────────────────────
                    let is_ocean = elev <= sea_level && !lake_here;
                    let is_waste = waste_here || (id == 0 && elev > sea_level && !lake_here);
                    let has_ocean_node =
                        ocean_id != 0 && (ocean_id as usize) < ocean_hierarchy.provinces.len();
                    let has_land_node = id != 0 && (id as usize) < hierarchy.provinces.len();

                    if is_ocean && water_political && has_ocean_node {
                        // ── Hydro-political Ocean Territory ────────────────
                        // Territory tint over the shaded bathymetry. The frozen
                        // polar seas are still ocean, so they are tinted too and
                        // never break the continuity of a marine realm's borders.
                        let ocean_cid = tier_color_id(ocean_hierarchy, ocean_id, tier);
                        let territory_rgb = id_to_territory_color(ocean_cid);
                        let phys = compute_physical_color();
                        final_color = [
                            territory_rgb[0] * 0.60 + phys[0] * 0.40,
                            territory_rgb[1] * 0.60 + phys[1] * 0.40,
                            territory_rgb[2] * 0.60 + phys[2] * 0.40,
                        ];
                        if show_borders {
                            final_color = ocean_border_shade(
                                final_color,
                                ocean_province_ids,
                                ocean_hierarchy,
                                ocean_id,
                                ocean_cid,
                                x,
                                y,
                                h,
                                tier,
                            );
                        }
                    } else if is_ocean {
                        // Water outside the current political scope: plain atlas sea.
                        final_color = compute_physical_color();
                    } else if lake_here {
                        final_color = [42.0, 125.0, 185.0];
                    } else if is_waste || !land_political || !has_land_node {
                        // Wasteland, wilderness, or land that the current scope does
                        // not portray. In the Water scope the continents are muted
                        // so that the ocean politics stay legible.
                        let phys = compute_physical_color();
                        if is_waste {
                            final_color = [
                                phys[0] * 0.78 + 35.0,
                                phys[1] * 0.78 + 35.0,
                                phys[2] * 0.78 + 40.0,
                            ];
                        } else if scope == MapScope::Water {
                            let grey = (phys[0] + phys[1] + phys[2]) / 3.0;
                            final_color = [
                                phys[0] * 0.40 + grey * 0.60,
                                phys[1] * 0.40 + grey * 0.60,
                                phys[2] * 0.40 + grey * 0.60,
                            ];
                        } else {
                            final_color = phys;
                        }
                    } else {
                        // Civilized Populated Province: territory tint over subtle terrain relief
                        let color_id = tier_color_id(hierarchy, id, tier);
                        let territory_rgb = id_to_territory_color(color_id);
                        let altitude = elev - sea_level;
                        let base = get_atlas_biome_color(display_temp, precip, altitude);

                        // Blend 65% territory color + 35% shaded relief
                        final_color = [
                            territory_rgb[0] * 0.65 + base[0] as f32 * shade * 0.35,
                            territory_rgb[1] * 0.65 + base[1] as f32 * shade * 0.35,
                            territory_rgb[2] * 0.65 + base[2] as f32 * shade * 0.35,
                        ];

                        let flow = flow_grid.map_or(0.0, |grid| *grid.get(x as isize, y as isize));
                        if flow >= min_river_flow {
                            let river_strength = ((flow - min_river_flow) / major_river_flow)
                                .sqrt()
                                .clamp(0.30, 1.0);
                            let blend = (river_strength * 1.3).clamp(0.0, 1.0);
                            final_color = [
                                final_color[0] * (1.0 - blend) + 20.0 * blend,
                                final_color[1] * (1.0 - blend) + 75.0 * blend,
                                final_color[2] * (1.0 - blend) + 155.0 * blend,
                            ];
                        }

                        if show_borders {
                            final_color = land_border_shade(
                                final_color,
                                province_ids,
                                hierarchy,
                                id,
                                color_id,
                                x,
                                y,
                                h,
                                tier,
                            );
                        }
                    }
                }

                // ── Civilizations overlay on every other scope ────────────────
                // The dawn can be laid over the physical relief, the survey sheet or
                // either political hierarchy: the realm tint never replaces the layer
                // underneath, it just claims the ground its provinces stand on. On the
                // political scopes the blend is lighter, so both layers stay readable.
                if show_civs && !civ_scope {
                    let lake = lake_here;
                    let is_ocean = elev <= sea_level && !lake;
                    let owner = civilizations.owner_of(id);
                    if !is_ocean && !lake && owner != 0 && id != 0 {
                        let civ_rgb = civilizations.color_of(owner);
                        let weight = if land_political || water_political {
                            0.40
                        } else {
                            0.62
                        };
                        final_color = blend_rgb(
                            final_color,
                            [civ_rgb[0] as f32, civ_rgb[1] as f32, civ_rgb[2] as f32],
                            weight,
                        );
                        if show_borders {
                            final_color = civ_border_shade(
                                final_color,
                                province_ids,
                                hierarchy,
                                civilizations,
                                id,
                                x,
                                y,
                                h,
                                tier,
                                false,
                            );
                        }
                    }
                }

                pixel[0] = final_color[0].clamp(0.0, 255.0) as u8;
                pixel[1] = final_color[1].clamp(0.0, 255.0) as u8;
                pixel[2] = final_color[2].clamp(0.0, 255.0) as u8;
                pixel[3] = 255;
            });

        ColorImage::from_rgba_unmultiplied([w, h], &pixels)
    }
}

impl eframe::App for MapViewer {
    /// One frame of the atlas.
    ///
    /// The order is the whole design: collect whatever the running act has
    /// reported, lay out the genesis panel (which is how a world is made), and
    /// then paint the world — or the cold, empty plate that stands where a world
    /// has not been grown yet.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame_ui(ui);
    }
}

impl MapViewer {
    /// One frame of the atlas, free of `eframe`.
    ///
    /// `eframe::Frame` cannot be built outside a real window, and every decision
    /// this frame makes is about egui and the pipeline — so the whole of it lives
    /// here, where a test can drive it, and the `App` impl above is the thin
    /// adapter that a running application needs.
    fn frame_ui(&mut self, ui: &mut egui::Ui) {
        if !self.style_installed {
            self.install_style(ui);
        }
        let ctx = ui.ctx().clone();
        self.poll_worker(&ctx);

        // Every panel wears the atlas's own frame: the stock one is padded for a
        // 13 pt interface, and this atlas reads at 15 pt.
        let panel_frame = atlas_panel_frame(ui.style());
        self.genesis_panel(ui, &panel_frame);

        // The world is taken for the length of the frame rather than borrowed:
        // painting it needs `&self` for the view state and `&AtlasWorld` for the
        // data, and a painter that had to be handed both would have to be handed
        // the data twice.
        match self.world.take() {
            Some(world) => {
                self.atlas_ui(ui, &world);
                self.world = Some(world);
            }
            None => self.cold_start(ui),
        }
    }
}

impl MapViewer {
    /// Paint the world, and the atlas that describes it.
    fn atlas_ui(&mut self, ui: &mut egui::Ui, world: &AtlasWorld) {
        if !self.style_installed {
            self.install_style(ui);
        }
        // Every panel wears the atlas's own frame: the stock one is padded for a
        // 13 pt interface, and this atlas reads at 15 pt.
        let panel_frame = atlas_panel_frame(ui.style());

        // The province bundles, when the acts that made them have run. A world
        // that has not reached act four has no provinces, and none of the
        // scopes that would read them are offered (see `MapScope::min_stage`).
        let province_ids = world.provinces.as_ref().map(|p| &p.province_ids);
        let ocean_province_ids = world.ocean.as_ref().map(|o| &o.ocean_province_ids);
        let is_wasteland = world.provinces.as_ref().map(|p| &p.is_wasteland);

        let mut visual_changed = false;
        // Panel clicks cannot move the map directly: the canvas rectangle for
        // this frame is only known once the panels have been laid out. Requests
        // are collected here and applied right after the canvas is allocated.
        let mut fit_request = false;
        let mut focus_request: Option<(f32, f32, f32)> = None;
        let mut clicked_civ: Option<u32> = None;
        let mut clicked_family: Option<u16> = None;

        // ── TOP CONTROLS BAR ──────────────────────────────────────────────────
        // Two rows, because the atlas now has ten map modes: what it shows, then
        // how it shows it. Both rows wrap, so a narrow window never clips a button.
        egui::Panel::top("top_controls")
            .frame(panel_frame.clone())
            .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                panel_title(ui, "🌍 Atlas Viewer");
                ui.separator();

                // ── Map scope: what the atlas portrays ────────────────────────
                // A scope is offered exactly when the world is old enough to have
                // what it portrays: relief needs only the land, biomes and climate
                // need the atmosphere, the provinces and the two hierarchies each
                // need their own act. Nothing is ever a map of something that has
                // not been made yet.
                for (scope, label) in [
                    (MapScope::Physical, "🗺 Physical"),
                    (MapScope::Topographic, "⛰ Topographic"),
                    (MapScope::Climate, "🌦 Climate"),
                    (MapScope::Temperature, "🌡 Temperature"),
                    (MapScope::Terrain, "🪨 Terrain"),
                    (MapScope::Civilizations, "🏺 Civilizations"),
                    (MapScope::Languages, "🗣 Languages"),
                    (MapScope::Land, "🏛 Land"),
                    (MapScope::Water, "🌊 Water"),
                    (MapScope::Everywhere, "🌍 Everywhere"),
                ] {
                    let ready = scope.available(world.stage);
                    let response = ui.add_enabled_ui(ready, |ui| {
                        ui.selectable_value(&mut self.scope, scope, label)
                    });
                    let response = response.inner;
                    if !ready {
                        response.on_hover_text(format!(
                            "Needs “{}. {}” — {}.",
                            scope.min_stage().index(),
                            scope.min_stage().title(),
                            scope.min_stage().blurb()
                        ));
                    } else if response.clicked() {
                        // Repaint in the new scope, but keep the reader's zoom and
                        // pan: a map mode is a change of what is shown, not of where
                        // the reader is looking. **Fit** is there when the whole
                        // world is wanted again.
                        visual_changed = true;
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                // Tier selector: the political tiers when a political scope is on,
                // and the internal subdivision of the realms in the Civilizations
                // scope (a realm's own provinces, areas, regions…).
                ui.add_enabled_ui(self.scope.is_political() || self.scope.is_civilizations(), |ui| {
                    ui.label("Tier:");
                    for (tier, label) in [
                        (MapTier::Provinces, "Provinces"),
                        (MapTier::Areas, "Areas"),
                        (MapTier::Regions, "Regions"),
                        (MapTier::Superregions, "Superregions"),
                        (MapTier::Continents, "Continents"),
                    ] {
                        if ui.selectable_value(&mut self.tier, tier, label).clicked() {
                            visual_changed = true;
                        }
                    }
                });

                ui.separator();

                // Border Toggle (in political and civilization scopes)
                if self.scope.is_political() || self.scope.is_civilizations() {
                    if ui.checkbox(&mut self.show_borders, "Borders").clicked() {
                        visual_changed = true;
                    }
                    ui.separator();
                }

                // ── Civilizations overlay ─────────────────────────────────────
                // The dawn is viewable on the physical atlas, the survey sheet and
                // both political hierarchies, not only on its own map mode.
                if !self.scope.is_civilizations() && !world.civilizations.is_empty() {
                    let world = &world.civilizations;
                    let label = format!("🏺 Civilizations ({})", world.civilizations.len());
                    if ui
                        .checkbox(&mut self.show_civilizations, label)
                        .on_hover_text(format!(
                            "{}\nAtlas date: {}\nThe overlay tints the provinces each realm holds, on top of whatever this scope already paints.",
                            world.epoch_label(),
                            world.map_year_label()
                        ))
                        .clicked()
                    {
                        visual_changed = true;
                    }
                    ui.separator();
                }

                // Zoom controls. **Fit** measures the canvas and shows the whole
                // planet; it is applied after the panels settle, further down.
                if ui
                    .button("➖")
                    .on_hover_text("Zoom out")
                    .clicked()
                {
                    self.zoom = (self.zoom * 0.75).clamp(MIN_ZOOM, MAX_ZOOM);
                }
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.button("➕").on_hover_text("Zoom in").clicked() {
                    self.zoom = (self.zoom * 1.33).clamp(MIN_ZOOM, MAX_ZOOM);
                }
                if ui
                    .button("⤢ Fit")
                    .on_hover_text("Fit the whole world in the window")
                    .clicked()
                {
                    fit_request = true;
                }
            });
        });

        // ── BOTTOM STATUS BAR ─────────────────────────────────────────────────
        egui::Panel::bottom("bottom_status")
            .frame(panel_frame.clone())
            .show(ui, |ui| {
                // Three lines of plain figures, each one allowed to wrap: the
                // footer is a status bar, not a place where a narrow window
                // quietly loses the right-hand half of the world's census. A
                // hierarchy that has not been built yet is not printed as a row
                // of zeroes either: it is simply not there to count.
                if world.hierarchy.total_provinces > 0 {
                    status_line(
                        ui,
                        format!(
                            "🏛 LAND — Continents: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}",
                            world.hierarchy.total_continents,
                            world.hierarchy.total_superregions,
                            world.hierarchy.total_regions,
                            world.hierarchy.total_areas,
                            world.hierarchy.total_provinces
                        ),
                    );
                }
                if world.ocean_hierarchy.total_provinces > 0 {
                    status_line(
                        ui,
                            format!(
                                "🌊 OCEAN — Basins: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}",
                                world.ocean_hierarchy.total_continents,
                                world.ocean_hierarchy.total_superregions,
                                world.ocean_hierarchy.total_regions,
                                world.ocean_hierarchy.total_areas,
                                world.ocean_hierarchy.total_provinces
                            ),
                        );
                }
                // ── The dawn of history ───────────────────────────────────────
                if !world.civilizations.is_empty() {
                    let dawn = &world.civilizations;
                    status_line(
                        ui,
                        format!(
                            "🏺 CIVILIZATIONS — {} · Atlas date: {} · {} · {} realms · {} civilized provinces · population {} · {:.1} people/km²",
                            dawn.epoch_label(),
                            dawn.map_year_label(),
                            dawn.age_label(),
                            dawn.civilizations.len(),
                            dawn.civilized_provinces,
                            thousands_u64(dawn.total_population),
                            dawn.mean_density_per_km2(),
                        ),
                    );
                }
            });

        // ── HYPSOMETRIC LEGEND (Topographic scope only) ───────────────────────
        // The survey sheet is only readable with its scale at hand, so the
        // Topographic scope carries the palette — every band, its metre range and
        // its name — plus the world's own relief extremes. Both extremes are
        // clickable: a figure the reader cannot find is a figure worth nothing.
        if self.scope == MapScope::Topographic
            && let Some(relief) = self.relief
        {
            // The survey sheet is only offered once there is a heightmap to
            // measure, so its extremes are always here to be shown.
            let (peak_lat, peak_lon) = self.pixel_lat_lon(relief.peak_x, relief.peak_y);
            let (trench_lat, trench_lon) = self.pixel_lat_lon(relief.trench_x, relief.trench_y);
            let peak_where = lat_lon_label(peak_lat, peak_lon);
            let trench_where = lat_lon_label(trench_lat, trench_lon);

            egui::Panel::right("hypsometric_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, "⛰ Topographic Scale");
                    caption(ui, "Tint = elevation above / depth below the sea surface.");
                    ui.separator();

                    let peak_label = format!("🗻 Highest peak   +{:.0} m", relief.peak_metres);
                    if ui
                        .add(egui::Button::new(peak_label).frame(false))
                        .on_hover_text(format!("{peak_where}\nClick to fly to the summit"))
                        .clicked()
                    {
                        // Never zoom *out* to fly somewhere: a reader who is already
                        // closer than the default keeps their scale.
                        let zoom = RELIEF_FOCUS_ZOOM.max(self.zoom);
                        focus_request = Some((relief.peak_x, relief.peak_y, zoom));
                    }
                    caption(ui, peak_where);

                    let trench_label = format!("🌊 Deepest trench  {:.0} m", relief.trench_metres);
                    if ui
                        .add(egui::Button::new(trench_label).frame(false))
                        .on_hover_text(format!("{trench_where}\nClick to fly to the deepest point"))
                        .clicked()
                    {
                        let zoom = RELIEF_FOCUS_ZOOM.max(self.zoom);
                        focus_request = Some((relief.trench_x, relief.trench_y, zoom));
                    }
                    caption(ui, trench_where);

                    ui.label(format!(
                        "🌍 Surface: {:.0}% ocean",
                        relief.ocean_share * 100.0
                    ));
                    caption(ui, "Click a figure above to centre the map on it.");
                    ui.separator();

                    // The band list is the only part that can overflow a short
                    // window, so it is the part that scrolls. It wears the same
                    // row as every other legend: swatch, band name, metre range.
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for band in (0..HYPSOMETRIC_COLORS.len()).rev() {
                            themes::legend_row(
                                ui,
                                HYPSOMETRIC_COLORS[band],
                                HYPSOMETRIC_NAMES[band],
                                &hypsometric_range_label(band),
                            );
                        }
                    });
                });
        }

        // ── CLIMATE LEGEND (Climate scope only) ───────────────────────────────
        if self.scope == MapScope::Climate {
            egui::Panel::right("climate_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, "🌦 Climate Zones");
                    caption(
                        ui,
                        "Land is classified by mean temperature, annual rainfall and altitude; the sea by its surface temperature.",
                    );
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for zone in ClimateZone::ALL {
                            themes::legend_row(ui, zone.color(), zone.label(), "");
                        }
                        ui.separator();
                        for (label, sample_temp) in [
                            ("Frozen sea", -8.0),
                            ("Polar sea", 0.0),
                            ("Cold temperate sea", 9.0),
                            ("Warm temperate sea", 19.0),
                            ("Tropical sea", 26.0),
                            ("Equatorial sea", 30.0),
                        ] {
                            themes::legend_row(
                                ui,
                                themes::ocean_climate_color(sample_temp),
                                label,
                                "",
                            );
                        }
                    });
                });
        }

        // ── TEMPERATURE LEGEND (Temperature scope only) ───────────────────────
        if self.scope == MapScope::Temperature {
            egui::Panel::right("temperature_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, "🌡 Temperature");
                    caption(
                        ui,
                        "Mean annual surface temperature: land station means, sea-surface means over the ocean.",
                    );
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for row in 0..TEMPERATURE_LEGEND_ROWS {
                            themes::legend_row(
                                ui,
                                themes::temperature_legend_color(row),
                                &themes::temperature_legend_label(row),
                                "",
                            );
                        }
                    });
                    ui.separator();
                    caption(ui, "A thin dark line marks every 5 °C isotherm.");
                });
        }

        // ── TERRAIN LEGEND (Terrain scope only) ───────────────────────────────
        if self.scope == MapScope::Terrain {
            egui::Panel::right("terrain_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, "🪨 Terrain Types");
                    caption(
                        ui,
                        "Landform bands from the vertical datum; the ocean floor is classified by depth.",
                    );
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for class in TerrainClass::ALL {
                            themes::legend_row(
                                ui,
                                class.color(),
                                class.label(),
                                &class.range_label(),
                            );
                        }
                    });
                });
        }

        // ── CIVILIZATIONS LEGEND (Civilizations scope only) ───────────────────
        // The subject of this map mode is a handful of realms, so the panel can
        // carry the whole history at once: the world's own calendar, then every
        // civilization with its colour, cradle, era and population. A realm's
        // entry is a button: clicking it flies the map to its cradle.
        if self.scope.is_civilizations() {
            egui::Panel::right("civilizations_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(CIV_LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, "🏺 The Dawn of Civilization");
                    let world = &world.civilizations;
                    if world.is_empty() {
                        ui.label("No civilization rose on this world.");
                        caption(
                            ui,
                            "The history stage only runs when it is asked for: pass --civs (it is on automatically with --viewer and --plates).",
                        );
                        return;
                    }

                    ui.label(egui::RichText::new(world.epoch_label()).small());
                    caption(ui, world.timeline.prehistory_label());
                    ui.label(
                        egui::RichText::new(format!(
                            "Atlas date: {} · {}",
                            world.map_year_label(),
                            world.age_label()
                        ))
                        .small()
                        .weak(),
                    )
                    .on_hover_text(format!(
                        "{} — {}",
                        world.age().stratum(),
                        world.age().describe()
                    ));
                    if ui
                        .button("🏛 Fly to the Common Era site")
                        .on_hover_text("The province the world's calendar is reckoned from")
                        .clicked()
                    {
                        focus_request = Some((
                            world.timeline.epoch.site_x,
                            world.timeline.epoch.site_y,
                            CIV_FOCUS_ZOOM,
                        ));
                    }
                    ui.separator();
                    ui.label(format!(
                        "{} societies on {} inhabited provinces",
                        world.civilizations.len(),
                        thousands(world.civilized_provinces as f32),
                    ));
                    ui.label(format!(
                        "{} km² inhabited · world population {}",
                        thousands(world.civilized_area_km2),
                        thousands_u64(world.total_population),
                    ));
                    ui.label(format!(
                        "Mean density {:.2} people/km²",
                        world.mean_density_per_km2()
                    ));
                    ui.separator();

                    // ── THE FIVE-WAY KEY ──────────────────────────────────────
                    // The map is a mosaic of five kinds of society, so the legend
                    // leads with what the colours *mean* and only then lists who holds
                    // what: a reader should be able to read the world's shape off the
                    // key alone.
                    let counts = world.counts_by_tier();
                    let areas = world.area_by_tier();
                    let inhabited_area: f32 = areas.iter().sum::<f32>().max(1.0);
                    caption(ui, "How the world is organised");
                    for (index, tier) in SocietyTier::LADDER.iter().enumerate() {
                        society_key_row(
                            ui,
                            *tier,
                            counts[index],
                            100.0 * areas[index] / inhabited_area,
                        );
                    }
                    ui.separator();
                    caption(ui, "Click a society to fly to its heartland.");

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for tier in SocietyTier::LADDER {
                            let shown = world
                                .civilizations
                                .iter()
                                .filter(|civ| civ.tier == tier)
                                .count();
                            if shown == 0 {
                                continue;
                            }
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} ({shown})",
                                    tier.label()
                                ))
                                .strong(),
                            );
                            let mut listed = 0usize;
                            for (index, civ) in world.civilizations.iter().enumerate() {
                                if civ.tier != tier || listed >= LISTED_PER_TIER {
                                    continue;
                                }
                                listed += 1;
                                let id = index as u32 + 1;
                                let selected = self.selected_civ == Some(id);
                                if society_entry(ui, civ.color, civ.name.as_str(), selected) {
                                    clicked_civ = Some(id);
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} · {}",
                                        civ.environment.label(),
                                        civ.culture.government.label(),
                                    ))
                                    .small(),
                                );
                                caption(
                                    ui,
                                    format!(
                                        "{} provinces · {} people",
                                        civ.provinces.len(),
                                        thousands_u64(civ.population),
                                    ),
                                );
                            }
                            if shown > listed {
                                caption(
                                    ui,
                                    format!("… and {} more, all on the map.", shown - listed),
                                );
                            }
                        }
                    });
                });
        }

        // ── LANGUAGES LEGEND (Languages scope only) ───────────────────────────
        if self.scope == MapScope::Languages {
            egui::Panel::right("languages_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(CIV_LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, "🗣 Languages of the World");
                    caption(
                        ui,
                        "Each family is a colour, each language a shade of it. A family holds the provinces its speakers settled, from its Urheimat outward.",
                    );
                    ui.separator();
                    if world.languages.is_empty() {
                        caption(ui, "No tongue has been given to this world yet.");
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for family in world.languages.families_by_size() {
                                let selected = self.selected_family == Some(family.id);
                                let response = ui
                                    .horizontal(|ui| {
                                        themes::swatch(ui, family.color);
                                        ui.selectable_label(
                                            selected,
                                            egui::RichText::new(&family.name).strong(),
                                        )
                                    })
                                    .inner;
                                if response.clicked() {
                                    clicked_family = Some(family.id);
                                }
                                caption(
                                    ui,
                                    format!(
                                        "{} · {}",
                                        family.autonym,
                                        family.urheimat.archetype.label()
                                    ),
                                );
                                caption(
                                    ui,
                                    format!(
                                        "{} languages · {} provinces · {} people",
                                        family.languages.len(),
                                        family.province_count,
                                        thousands_u64(family.population),
                                    ),
                                );
                                caption(
                                    ui,
                                    format!("Urheimat: {}", world.languages.urheimat_label(family.id)),
                                );
                                for language in world.languages.languages_of(family.id).iter().take(6) {
                                    ui.horizontal(|ui| {
                                        themes::swatch(ui, language.color);
                                        ui.label(egui::RichText::new(&language.name).small());
                                        caption(
                                            ui,
                                            format!("{} provinces", language.province_count),
                                        );
                                    });
                                }
                                ui.separator();
                            }
                        });
                    }
                });
        }

        // A click on a family's legend entry flies the map to its Urheimat.
        if let Some(id) = clicked_family {
            self.selected_family = Some(id);
            if let Some(family) = world.languages.family(id) {
                let zoom = CIV_FOCUS_ZOOM.max(self.zoom);
                focus_request = Some((family.urheimat.x, family.urheimat.y, zoom));
            }
        }

        // A click on a realm's legend entry flies the map to that realm.
        if let Some(id) = clicked_civ {
            self.selected_civ = Some(id);
            if let Some(civ) = world.civilizations.civilization(id) {
                let zoom = CIV_FOCUS_ZOOM.max(self.zoom);
                focus_request = Some((civ.seat_x, civ.seat_y, zoom));
            }
        }

        // The 8K plate is only regenerated when the world or the scope has
        // changed — never per frame — and never while an act is running, because
        // while an act is running the *live* plate is what the canvas shows.
        if self.job.is_none() && (self.texture.is_none() || visual_changed) {
            let img = self.generate_texture(world);
            self.texture = Some(
                ui.ctx()
                    .load_texture("map_texture", img, TextureOptions::NEAREST),
            );
        }

        // ── INTERACTIVE MAP CANVAS ────────────────────────────────────────────
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        // The canvas is the only thing that knows how big the map area really is,
        // so both "fit" and "fly to" are resolved here, in the same frame the
        // click happened.
        if fit_request || self.pending_fit {
            if rect.width() >= 64.0 && rect.height() >= 64.0 {
                self.fit_to(rect, world.width as f32, world.height as f32);
                self.pending_fit = false;
            }
        }
        // A fly-to request from this frame wins over one left over from a frame
        // whose canvas had not been laid out yet; either way it is applied here,
        // and `focus_on` re-queues it if the canvas is still too small.
        if let Some((x, y, zoom)) = focus_request.take().or(self.pending_focus.take()) {
            self.focus_on(rect, x, y, zoom);
        }

        if response.dragged() {
            self.offset += response.drag_delta();
        }

        if ui.rect_contains_pointer(rect) {
            let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old_zoom = self.zoom;
                self.zoom =
                    (self.zoom + scroll * 0.001 * self.zoom * 4.0).clamp(MIN_ZOOM, MAX_ZOOM);

                if let Some(mouse_pos) = ui.ctx().pointer_hover_pos() {
                    let zoom_ratio = self.zoom / old_zoom;
                    self.offset =
                        mouse_pos.to_vec2() - (mouse_pos.to_vec2() - self.offset) * zoom_ratio;
                }
            }
        }

        // Paint Map Mesh
        //
        // While an act is running, the plate on the canvas is the *live* one: the
        // low-resolution picture that act is pushing up from the very grid it is
        // working on. It is drawn in the same rectangle as the finished map, at
        // the same zoom and offset, so the reader can pan the world as it grows —
        // and when the act lands, the 8K plate replaces it in place.
        let live = if self.job.is_some() {
            self.live.as_ref()
        } else {
            None
        };
        let plate = live
            .map(|live| (&live.texture, live.label.as_str()))
            .or_else(|| self.texture.as_ref().map(|texture| (texture, "")));
        if let Some((texture, live_label)) = plate {
            let map_size = egui::vec2(
                world.width as f32 * self.zoom,
                world.height as f32 * self.zoom,
            );
            let map_rect = egui::Rect::from_min_size(rect.min + self.offset, map_size);

            let mut mesh = egui::Mesh::with_texture(texture.id());
            mesh.add_rect_with_uv(
                map_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );

            ui.painter()
                .with_clip_rect(rect)
                .add(egui::Shape::mesh(mesh));

            // The place the atlas was last flown to, ringed so the reader can see
            // what the "Highest peak" or a clicked realm actually points at.
            if let Some((mx, my)) = self.marker {
                let at = map_rect.min + egui::vec2(mx * self.zoom, my * self.zoom);
                if rect.expand(16.0).contains(at) {
                    let painter = ui.painter().with_clip_rect(rect);
                    painter.circle_stroke(
                        at,
                        14.0,
                        Stroke::new(2.0, Color32::from_rgb(255, 236, 170)),
                    );
                    painter.circle_stroke(at, 5.0, Stroke::new(1.5, Color32::from_rgb(60, 46, 24)));
                }
            }

            // The strip that says what is being watched: the plate underneath is
            // the act in flight, not a finished world, and it must never be
            // mistaken for one.
            if !live_label.is_empty() {
                let strip = egui::Rect::from_min_size(
                    rect.min + egui::vec2(0.0, 8.0),
                    egui::vec2(rect.width().min(520.0), 34.0),
                );
                let painter = ui.painter().with_clip_rect(rect);
                painter.rect_filled(strip, 4.0, Color32::from_rgba_unmultiplied(12, 16, 28, 224));
                let text = format!("🔴 {} — {}", live_label, stage_progress(self.job.as_ref()));
                painter.text(
                    strip.left_center() + egui::vec2(12.0, 0.0),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::proportional(TEXT_BODY),
                    Color32::from_rgb(240, 214, 180),
                );
            }
        }

        // ── HOVER INSPECTION TOOLTIP ──────────────────────────────────────────
        if let Some(mouse_pos) = response.hover_pos() {
            let map_x = ((mouse_pos.x - rect.min.x - self.offset.x) / self.zoom).floor() as isize;
            let map_y = ((mouse_pos.y - rect.min.y - self.offset.y) / self.zoom).floor() as isize;

            if map_x >= 0
                && map_x < world.width as isize
                && map_y >= 0
                && map_y < world.height as isize
            {
                let elev = *world.heightmap.get(map_x, map_y);
                // A grid the world does not have yet reads as its own absence:
                // there is no temperature in a world with no atmosphere.
                let temp = world
                    .temperature
                    .as_ref()
                    .map_or(0.0, |grid| *grid.get(map_x, map_y));
                let precip = world
                    .precipitation
                    .as_ref()
                    .map_or(0.0, |grid| *grid.get(map_x, map_y));
                let flow = world
                    .flow
                    .as_ref()
                    .map_or(0.0, |grid| *grid.get(map_x, map_y));
                let sea_level = relief::SEA_LEVEL;
                // `altitude` stays in heightmap units: the biome palette's own
                // thresholds are expressed in those units. Everything that is
                // *reported to the user* goes through the vertical datum.
                let altitude = elev - sea_level;
                let metres = relief::to_metres(elev);

                // Reproduce the exact "display temperature" (station temperature
                // plus the fractal micro-climate jitter) used while painting, so
                // the tooltip can never disagree with the pixels on screen about
                // whether a spot is frozen or not.
                let v = map_y as f32 / (world.height - 1) as f32;
                let lat = (0.5 - v) * std::f32::consts::PI;
                let u = map_x as f32 / world.width as f32;
                let lon = (u - 0.5) * std::f32::consts::TAU;
                let hx = (lat.cos() * lon.cos()) as f64;
                let hy = lat.sin() as f64;
                let hz = (lat.cos() * lon.sin()) as f64;
                let micro_noise = sample_3d_fbm(
                    &OpenSimplex::new(777),
                    hx * 35.0,
                    hy * 35.0,
                    hz * 35.0,
                    4,
                    2.0,
                    0.5,
                );
                let display_temp = temp + micro_noise * 5.5;

                let id = province_ids.map_or(0, |ids| *ids.get(map_x, map_y));
                let ocean_id = ocean_province_ids.map_or(0, |ids| *ids.get(map_x, map_y));
                // A frozen polar sea is "polar land": physically the very same
                // impassable wasteland as the ice-capped polar continents, but
                // politically it still belongs to a marine realm.
                let is_ocean = elev <= sea_level
                    && !world
                        .is_lake
                        .as_ref()
                        .is_some_and(|lake| *lake.get(map_x, map_y));
                let frozen_sea = is_frozen_sea(display_temp, elev, sea_level);
                let is_waste = is_wasteland.is_some_and(|w| *w.get(map_x, map_y))
                    || (id == 0
                        && elev > sea_level
                        && !world
                            .is_lake
                            .as_ref()
                            .is_some_and(|lake| *lake.get(map_x, map_y)));

                response.on_hover_ui_at_pointer(|ui| {
                    if self.scope == MapScope::Civilizations {
                        // ── The dawn of history readout ───────────────────────
                        // The whole point of this map mode: hovering a rising
                        // civilization explains why *this* spot became one.
                        let owner = world.civilizations.owner_of(id);
                        let lake = world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y));
                        if owner != 0 && !is_ocean && !lake {
                            let civ = world
                                .civilizations
                                .civilization(owner)
                                .expect("owner id always indexes a civilization");
                            civilization_dossier(
                                ui,
                                owner,
                                civ,
                                &world.civilizations,
                                &world.languages,
                            );
                        } else if lake {
                            card_title(ui, "💧 Inland Freshwater Lake");
                            ui.label("No realm claims the water itself.");
                            ui.separator();
                            ui.label(format!("Lake Surface: {:.0} m", relief::altitude_metres(elev)));
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else if is_ocean {
                            card_title(ui, "🌊 Uncivilized Sea");
                            ui.label(format!("Depth: {:.0} m", relief::depth_metres(elev)));
                            ui.separator();
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else if is_waste {
                            card_title(ui, "🏔️ Wasteland Beyond History");
                            ui.strong("Impassable or unclaimed wilderness");
                            ui.separator();
                            ui.label(format!("Environment: {}", get_biome_name(display_temp, precip, altitude)));
                            ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else {
                            card_title(ui, "🌾 Unclaimed Land");
                            ui.strong("No civilization has risen here");
                            caption(
                                ui,
                                "Fertile enough to live on, but never the best ground in reach: nothing here outperformed the cradles.",
                            );
                            ui.separator();
                            ui.label(format!("Environment: {}", get_biome_name(display_temp, precip, altitude)));
                            ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                            ui.label(format!("Temperature: {:.1} °C", temp));
                            ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                            ui.separator();
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        }
                    } else if self.scope == MapScope::Languages {
                        // ── The language readout ──────────────────────────────
                        let lake = world
                            .is_lake
                            .as_ref()
                            .is_some_and(|lake| *lake.get(map_x, map_y));
                        if is_ocean {
                            card_title(ui, "🌊 Open Water");
                            ui.label("No land, and so no tongue of its own.");
                            ui.separator();
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else if lake {
                            card_title(ui, "💧 Inland Freshwater Lake");
                            ui.label("The water itself belongs to no language.");
                            ui.separator();
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else if let (Some(language), Some(family)) = (
                            world.languages.language_of_province(id),
                            world.languages.family_of_province(id),
                        ) {
                            ui.horizontal(|ui| {
                                themes::swatch(ui, family.color);
                                card_title(ui, family.name.as_str());
                            });
                            ui.strong(format!(
                                "{} · {}",
                                language.name,
                                family.urheimat.archetype.label()
                            ));
                            caption(ui, family.urheimat.archetype.describe());
                            ui.separator();
                            ui.label(format!("Autonym: {}", language.autonym));
                            ui.label(format!(
                                "Urheimat: {}",
                                world.languages.urheimat_label(family.id)
                            ));
                            if let Some(name) = world
                                .languages
                                .province_names
                                .get(id as usize)
                                .filter(|name| !name.is_empty())
                            {
                                ui.label(format!("Province: {name}"));
                            }
                            let owner = world.civilizations.owner_of(id);
                            if owner != 0 {
                                ui.separator();
                                ui.label(format!(
                                    "Society: {}",
                                    world.civilizations.name_of(owner)
                                ));
                                if let Some(tier) = world.civilizations.tier_of(id) {
                                    ui.label(egui::RichText::new(tier.label()).small());
                                }
                            }
                            ui.separator();
                            ui.label(format!("Size: {} px", {
                                world
                                    .hierarchy
                                    .provinces
                                    .get(id as usize)
                                    .map_or(0, |node| node.pixel_count)
                            }));
                            ui.label(format!(
                                "Elevation: {:.0} m",
                                relief::altitude_metres(elev)
                            ));
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else {
                            card_title(ui, "🏔️ Beyond the Tongues");
                            ui.strong("No language has settled this ground");
                            ui.separator();
                            ui.label(format!(
                                "Environment: {}",
                                get_biome_name(display_temp, precip, altitude)
                            ));
                            ui.label(format!(
                                "Elevation: {:.0} m",
                                relief::altitude_metres(elev)
                            ));
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        }
                    } else if self.scope == MapScope::Topographic {
                        // ── Hypsometric survey readout ────────────────────────
                        let band = hypsometric_band(metres);
                        let surface = if is_ocean {
                            if frozen_sea { frozen_sea_name(display_temp) } else { "Open Ocean" }
                        } else if world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y)) {
                            "Inland Freshwater Lake"
                        } else if display_temp < -3.0 {
                            "Ice-capped Land"
                        } else {
                            get_biome_name(display_temp, precip, altitude)
                        };

                        card_title(ui, format!("⛰ {}", surface));
                        ui.separator();
                        if elev > sea_level {
                            ui.label(format!("Elevation: {:.0} m above sea level", relief::altitude_metres(elev)));
                        } else {
                            ui.label(format!("Depth: {:.0} m below sea level", relief::depth_metres(elev)));
                        }
                        ui.label(format!(
                            "Relief band: {}  ({})",
                            HYPSOMETRIC_NAMES[band],
                            hypsometric_range_label(band)
                        ));
                        // Every surface reports a temperature — the land its
                        // station temperature, the sea its surface temperature.
                        if is_ocean {
                            ui.label(format!("Sea-Surface Temperature: {:.1} °C", temp));
                        } else {
                            ui.label(format!("Temperature: {:.1} °C", temp));
                            ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        }
                        if !is_ocean && flow >= 1800.0 {
                            ui.label(format!("River Flow: {:.0} m³/s", flow));
                        }
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if self.scope == MapScope::Climate {
                        // ── Climate readout ────────────────────────────
                        let lake = world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y));
                        if lake {
                            card_title(ui, "🌦 Inland Lake");
                            ui.label("Fresh water, read with the climate of its shores.");
                        } else if is_ocean {
                            card_title(ui, format!("🌦 {}", themes::ocean_climate_label(temp)));
                            ui.separator();
                            ui.label(format!("Sea-surface temperature: {:.1} °C", temp));
                            if frozen_sea {
                                ui.label("Perennial sea ice.");
                            }
                        } else {
                            let zone = ClimateZone::classify(display_temp, precip, altitude);
                            card_title(ui, format!("🌦 {}", zone.label()));
                            caption(ui, zone.describe());
                            ui.separator();
                            ui.label(format!("Mean temperature: {:.1} °C", temp));
                            ui.label(format!(
                                "Annual rainfall: {:.0} mm/yr",
                                precip * 1200.0
                            ));
                            ui.label(format!(
                                "Elevation: {:.0} m",
                                relief::altitude_metres(elev)
                            ));
                        }
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if self.scope == MapScope::Temperature {
                        // ── Thermal readout ────────────────────────────
                        let band = themes::isotherm_band(temp);
                        let lake = world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y));
                        let surface = if is_ocean {
                            if frozen_sea {
                                "Sea ice"
                            } else {
                                "Sea surface"
                            }
                        } else if lake {
                            "Lake surface"
                        } else if display_temp < -3.0 {
                            "Ice-capped land"
                        } else {
                            "Land surface"
                        };
                        card_title(ui, format!("🌡 {:.1} °C", temp));
                        caption(ui, surface);
                        ui.separator();
                        ui.label(format!(
                            "Isotherm band: {}",
                            themes::isotherm_band_label(band)
                        ));
                        if elev > sea_level {
                            ui.label(format!(
                                "Elevation: {:.0} m",
                                relief::altitude_metres(elev)
                            ));
                        }
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if self.scope == MapScope::Terrain {
                        // ── Terrain readout ────────────────────────────
                        let lake = world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y));
                        let metres = relief::to_metres(elev);
                        let class = TerrainClass::classify(metres, lake, display_temp);
                        let (gx, gy) =
                            world.heightmap.gradient(map_x as f32, map_y as f32);
                        let slope = (gx * gx + gy * gy).sqrt();
                        card_title(ui, format!("🪨 {}", class.label()));
                        ui.separator();
                        if elev > sea_level {
                            ui.label(format!(
                                "Elevation: {:.0} m",
                                relief::altitude_metres(elev)
                            ));
                        } else {
                            ui.label(format!(
                                "Depth: {:.0} m",
                                relief::depth_metres(elev)
                            ));
                        }
                        ui.label(format!("Band: {}", class.range_label()));
                        if !is_ocean && !lake && class != TerrainClass::Ice {
                            ui.label(format!(
                                "Ground: {} ({:.0} m rise per pixel)",
                                themes::slope_label(slope),
                                slope * relief::LAND_M_PER_UNIT
                            ));
                        }
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if self.scope == MapScope::Physical {
                        // ── Physical atlas readout ─────────────────────
                        let biome_title = if is_ocean {
                            if frozen_sea { frozen_sea_name(display_temp) } else { "Ocean" }
                        } else if world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y)) {
                            "Inland Freshwater Lake"
                        } else {
                            get_biome_name(display_temp, precip, altitude)
                        };

                        card_title(ui, format!("🌍 {}", biome_title));
                        ui.separator();
                        if elev > sea_level {
                            ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                        } else {
                            ui.label(format!("Depth: {:.0} m", relief::depth_metres(elev)));
                        }
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        if flow >= 1800.0 {
                            ui.label(format!("River Flow: {:.0} m³/s", flow));
                        }
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if is_ocean
                        && ocean_id != 0
                        && (ocean_id as usize) < world.ocean_hierarchy.provinces.len()
                    {
                        // ── Hydro-political marine realm ───────────────
                        let node = &world.ocean_hierarchy.provinces[ocean_id as usize];
                        if frozen_sea {
                            card_title(ui, format!("🧊 Ocean Province #{}", ocean_id));
                            ui.strong(format!("{} · Impassable Polar Wasteland", frozen_sea_name(display_temp)));
                        } else {
                            card_title(ui, format!("🌊 Ocean Province #{}", ocean_id));
                        }
                        ui.separator();
                        ui.horizontal(|ui| { ui.label("Area:");        ui.strong(format!("#{}", node.area_id)); });
                        ui.horizontal(|ui| { ui.label("Region:");      ui.strong(format!("#{}", node.region_id)); });
                        ui.horizontal(|ui| { ui.label("Superregion:"); ui.strong(format!("#{}", node.superregion_id)); });
                        ui.horizontal(|ui| { ui.label("Ocean Basin:"); ui.strong(format!("#{}", node.continent_id)); });
                        ui.separator();
                        ui.label(format!("Size: {} px", node.pixel_count));
                        ui.label(format!("Sea Floor Depth: {:.0} m", relief::depth_metres(elev)));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if is_waste {
                        if display_temp < PACK_ICE_TEMP {
                            card_title(ui, "🧊 Polar Ice Wasteland");
                            ui.strong("Permanent Ice Sheet / Impassable Wilderness");
                        } else {
                            card_title(ui, "🏔️ Wasteland");
                            ui.strong("Impassable / Uninhabited Wilderness");
                        }
                        ui.separator();
                        ui.label(format!("Environment: {}", get_biome_name(display_temp, precip, altitude)));
                        ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y)) {
                        card_title(ui, "💧 Inland Freshwater Lake");
                        ui.separator();
                        ui.label(format!("Lake Surface: {:.0} m", relief::altitude_metres(elev)));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if id != 0 && (id as usize) < world.hierarchy.provinces.len() {
                        let node = &world.hierarchy.provinces[id as usize];

                        card_title(ui, format!("📍 Province #{}", id));
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Area:");
                            ui.strong(format!("#{}", node.area_id));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Region:");
                            ui.strong(format!("#{}", node.region_id));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Superregion:");
                            ui.strong(format!("#{}", node.superregion_id));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Continent:");
                            ui.strong(format!("#{}", node.continent_id));
                        });
                        if node.anchor_id != 0 && node.anchor_id != node.id {
                            ui.horizontal(|ui| {
                                ui.label("Anchor Coast:");
                                ui.strong(format!("Province #{}", node.anchor_id));
                            });
                        }
                        // ── The society this province belongs to ──────────────
                        // Shown in every scope, because the province's population
                        // and its holders are facts about the province, not about the
                        // map mode currently on screen.
                        let owner = world.civilizations.owner_of(id);
                        if owner != 0 {
                            ui.horizontal(|ui| {
                                ui.label("Society:");
                                ui.strong(world.civilizations.name_of(owner));
                            });
                            if let Some(tier) = world.civilizations.tier_of(id) {
                                let environment = world
                                    .civilizations
                                    .civilization(owner)
                                    .map_or("", |civ| civ.environment.label());
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} · {environment}",
                                        tier.label()
                                    ))
                                    .small(),
                                );
                            }
                        }
                        ui.separator();
                        ui.label(format!("Size: {} px", node.pixel_count));
                        if owner != 0 {
                            ui.label(format!(
                                "Population: {} people ({:.1}/km²)",
                                thousands_u64(world.civilizations.population_of(id)),
                                world.civilizations.population_of(id) as f32
                                    / (node.pixel_count as f32
                                        * crate::core::sphere::pixel_area_km2(
                                            map_y as usize,
                                            world.height,
                                            world.width
                                        ))
                                        .max(1.0),
                            ));
                            ui.label(format!(
                                "Society population: {} people",
                                thousands_u64(
                                    world.civilizations
                                        .civilization(owner)
                                        .map_or(0, |civ| civ.population)
                                )
                            ));
                        }
                        ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else {
                        card_title(ui, "🌊 Ocean");
                        ui.label(format!("Depth: {:.0} m", relief::depth_metres(elev)));
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── THE GENESIS FLOW ────────────────────────────────────────────────────

    /// A scope's subject has to exist before the scope is offered: relief needs
    /// only the land, biomes and climate need the atmosphere, and the provinces,
    /// the two hierarchies and the dawn each need their own act.
    #[test]
    fn a_scope_is_offered_exactly_when_its_subject_exists() {
        // A world of one act can portray its elevation and its landforms, and
        // nothing else — there is nothing else to portray.
        assert!(MapScope::Topographic.available(Stage::Geology));
        assert!(MapScope::Terrain.available(Stage::Geology));
        for scope in [
            MapScope::Physical,
            MapScope::Climate,
            MapScope::Temperature,
            MapScope::Land,
            MapScope::Water,
            MapScope::Everywhere,
            MapScope::Civilizations,
            MapScope::Languages,
        ] {
            assert!(
                !scope.available(Stage::Geology),
                "{:?} must not be offered by a world of one act",
                scope.min_stage()
            );
        }

        // Every scope's requirement is monotone: an act that satisfies one scope
        // satisfies every scope before it, so the atlas can never offer a hole in
        // the sequence.
        for scope in [
            MapScope::Physical,
            MapScope::Topographic,
            MapScope::Climate,
            MapScope::Temperature,
            MapScope::Terrain,
            MapScope::Civilizations,
            MapScope::Languages,
            MapScope::Land,
            MapScope::Water,
            MapScope::Everywhere,
        ] {
            let first = Stage::ALL
                .iter()
                .position(|stage| scope.available(*stage))
                .unwrap_or_else(|| panic!("{:?} is never offered at all", scope.min_stage()));
            for (index, stage) in Stage::ALL.iter().enumerate() {
                assert_eq!(
                    scope.available(*stage),
                    index >= first,
                    "{:?} must be offered from {} onward",
                    scope.min_stage(),
                    scope.min_stage().title()
                );
            }
        }

        // And what the atlas opens on is always something it can paint.
        for stage in Stage::ALL {
            assert!(
                default_scope(stage).available(stage),
                "the scope the atlas opens on for {} must itself be offered",
                stage.title()
            );
        }
    }

    /// The panel is the only way a world is made, so it has to be readable before
    /// there is one: the first act is the only pressable act, the rest are marked
    /// locked, and the plate where the world will be says there is no world yet.
    #[test]
    fn the_genesis_panel_offers_only_the_first_act_before_there_is_a_world() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        let says = |needle: &str| {
            assert!(
                text.iter().any(|drawn| drawn.contains(needle)),
                "the atlas never painted {needle:?}"
            );
        };

        says("🧬 Genesis");
        // The world data controls say what they are: an action, a setting, and
        // the list of what is on disk.
        says("World data");
        says("📂 Load");
        says("💾 Save");
        says("Save automatically when act 3 and act 6 land");
        says("Saved worlds");
        // The first act is offered, and marked as the one to run next.
        says("▶ 1. Geology & Erosion");
        // Nothing after it is offered at all.
        for stage in &Stage::ALL[1..] {
            says(&format!("▢ {}. {}", stage.index(), stage.title()));
        }
        // And the plate where the world will be is honest about being empty.
        says("No world yet");
        says("Seed 42");
    }

    /// A world that is already physical — the state `--load` leaves behind —
    /// opens with its three acts done, its next act waiting, and the rest locked.
    #[test]
    fn a_world_of_three_acts_opens_at_the_fourth() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::for_test(1_980, Stage::Hydrology));
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        let says = |needle: &str| {
            assert!(
                text.iter().any(|drawn| drawn.contains(needle)),
                "the atlas never painted {needle:?}"
            );
        };

        says("✔ 3. Hydrology & Rivers");
        // Nothing in this session timed those acts: they came from the cache.
        says("· loaded");
        says("▶ 4. Geography & Provinces");
        says("▢ 5. Geopolitical Hierarchy");
        // Its climate exists, so the atlas opens on the physical world itself.
        assert_eq!(viewer.scope, MapScope::Physical);
    }

    /// A world adopted from the full cache opens at act seven, with acts one to
    /// six done and the rest still locked.
    #[test]
    fn a_world_of_six_acts_opens_at_the_seventh() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::for_test(1_980, Stage::HydroPolitics));
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        let says = |needle: &str| {
            assert!(
                text.iter().any(|drawn| drawn.contains(needle)),
                "the atlas never painted {needle:?}"
            );
        };

        says("✔ 6. Hydro-Political Hierarchy");
        says("▶ 7. Dawn of Civilization");
        says("▢ 8. Languages & Phonetics");
    }

    /// A finished act can be un-done from the panel: rewinding re-offers that act
    /// and locks the ones that had followed it, while the acts before it stand.
    #[test]
    fn a_finished_act_can_be_un_done_from_the_panel() {
        let mut viewer = MapViewer::new(Genesis::for_test(1_980, Stage::HydroPolitics));
        viewer.rewind_world(Stage::Provinces);

        assert_eq!(viewer.genesis.ready(), Some(Stage::Provinces));
        assert_eq!(
            viewer.genesis.state(Stage::Hydrology),
            StageState::Done { secs: 0.0 },
            "act three stands untouched"
        );
        assert_eq!(
            viewer.genesis.state(Stage::Geopolitics),
            StageState::Locked,
            "and act five is locked again"
        );
    }

    /// An act in flight is shown as it happens: the sub-step it is on, how far
    /// into that sub-step it has got, and the live plate it is pushing up.
    #[test]
    fn a_running_act_shows_its_sub_step_its_fraction_and_its_plate() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));

        // An act of the shape `start_stage` builds: a pipeline handed over, and
        // the two channels back.
        let (progress_tx, progress_rx) = mpsc::channel();
        let (_done_tx, done_rx) = mpsc::channel();
        viewer.job = Some(StageJob {
            stage: Stage::Geology,
            progress: progress_rx,
            finished: done_rx,
            fallback: Genesis::new(42),
            started: Instant::now(),
            sub: "starting up",
            done: 0,
            total: 0,
        });

        progress_tx
            .send(ProgressEvent::SubStep {
                label: "Valley carving",
                total: 100,
            })
            .expect("the atlas is listening");
        progress_tx
            .send(ProgressEvent::Tick {
                done: 42,
                total: 100,
            })
            .expect("and taking ticks");
        // A plate of a 3x2 world, as an act would send it.
        progress_tx
            .send(ProgressEvent::Preview {
                label: "Continents rising",
                width: 3,
                height: 2,
                rgba: vec![0u8; 3 * 2 * 4],
            })
            .expect("and pictures");

        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        let says = |needle: &str| {
            assert!(
                text.iter().any(|drawn| drawn.contains(needle)),
                "the atlas never painted {needle:?}"
            );
        };

        says("⟳ 1. Geology & Erosion");
        says("Valley carving");
        says("42%");
        says("🔴 live: Continents rising");
        // The act has not finished, so the world is not there yet — and the plate
        // on the canvas is captioned as a world *being* grown.
        says("Growing a world");
        assert!(viewer.live.is_some(), "the plate became a texture");
    }

    /// What the panel says about the world cache and what the pipeline would do
    /// with it are one fact, not two: the Save checkbox *is* a pipeline setting,
    /// and it survives a world being begun again.
    #[test]
    fn the_panel_and_the_pipeline_agree_about_the_world_cache() {
        // The atlas opens ready to save the world as it grows, and the pipeline is
        // armed to match.
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));
        let _ = draw_frames(&ctx, &mut viewer);
        assert!(
            viewer.genesis.autosave(),
            "the Save checkbox means act 3 and act 6 write the cache"
        );

        // Turning it off is a setting on the pipeline, not a mode in the panel.
        viewer.genesis.set_autosave(false);
        // ... and a world begun again keeps the reader's choice.
        viewer.reset_world();
        assert!(
            !viewer.genesis.autosave(),
            "a reset world keeps the cache choice it was grown under"
        );
    }

    /// Picking a world from the saved list adopts its seed and loads it at once:
    /// there is no second press between picking and loading, and a cache that is
    /// not there is a sentence rather than a crash.
    #[test]
    fn picking_a_saved_seed_adopts_it_and_says_why_when_there_is_nothing_to_load() {
        let mut viewer = MapViewer::new(Genesis::new(42));
        // No cache will ever be written under this seed in a test.
        viewer.pick_cached_world(u64::MAX);

        assert_eq!(
            viewer.seed_text,
            u64::MAX.to_string(),
            "the seed field follows the pick"
        );
        assert_eq!(viewer.genesis.seed(), u64::MAX, "and so does the pipeline");
        let notice = viewer.cache_notice.as_deref().unwrap_or_default();
        assert!(
            notice.contains("No saved world"),
            "a missing cache is said in words: {notice:?}"
        );
    }

    /// The metadata under a saved world is written in the units a reader thinks
    /// in, not in bytes and nanoseconds.
    #[test]
    fn a_cache_is_described_in_units_a_reader_thinks_in() {
        assert_eq!(cache_size(570_425_480), "544 MB");
        assert_eq!(cache_size(2 * 1024 * 1024 * 1024), "2.0 GB");

        let now = SystemTime::now();
        assert_eq!(
            cache_age(Some(now - std::time::Duration::from_secs(90))),
            "1 min ago"
        );
        assert_eq!(
            cache_age(Some(now - std::time::Duration::from_secs(3 * 86_400))),
            "3 d ago"
        );
        assert_eq!(cache_age(None), "at an unknown time");
    }

    /// The row for a saved world says which acts it holds, when the file's header
    /// names them; an older save with no header says only that a world is there.
    #[test]
    fn a_saved_world_says_which_acts_it_holds() {
        assert_eq!(acts_saved(Some(5)), "acts 1–5");
        assert_eq!(acts_saved(Some(6)), "acts 1–6");
        assert_eq!(acts_saved(None), "a saved world");
    }

    /// Changing the seed mid-way does not throw the world away: the acts already
    /// grown stay on the canvas, and the seed re-points only the acts still to run.
    #[test]
    fn changing_the_seed_keeps_the_world_that_is_already_grown() {
        let mut viewer = MapViewer::new(Genesis::for_test(1_980, Stage::Geology));
        assert!(
            viewer.world.is_some(),
            "the atlas opens on the world act one grew"
        );
        viewer
            .log
            .insert(0, "✔ 1. Geology & Erosion — 8.0s".to_string());

        viewer.seed_text = "4242".to_string();
        viewer.apply_seed();

        assert_eq!(viewer.genesis.seed(), 4_242, "the new seed is adopted");
        assert!(
            viewer.world.is_some(),
            "the world is still on the canvas, not thrown away"
        );
        assert_eq!(
            viewer.genesis.ready(),
            Some(Stage::Climate),
            "only the acts still to run follow the new seed"
        );
        assert!(
            viewer.log.iter().any(|line| line.contains("Geology")),
            "the log of what has been grown survives the change"
        );
        assert!(
            viewer.log.iter().any(|line| line.contains("4242")),
            "and the panel says the seed changed"
        );
    }

    /// The acts are a strict chain, and the chain lives in the pipeline rather
    /// than in the panel: a virgin world offers exactly one act, and every act
    /// after it waits.
    #[test]
    fn the_acts_are_a_strict_chain() {
        let genesis = Genesis::new(7);
        assert_eq!(genesis.ready(), Some(Stage::Geology));
        assert_eq!(genesis.state(Stage::Geology), StageState::Ready);
        assert_eq!(genesis.state(Stage::Climate), StageState::Locked);
        for (position, stage) in Stage::ALL.iter().enumerate() {
            assert_eq!(
                stage.index(),
                position + 1,
                "the acts are numbered in order"
            );
            assert!(!stage.title().is_empty(), "every act is named");
            assert!(
                !stage.blurb().is_empty(),
                "and every act says what it makes"
            );
        }
    }

    /// Drive a few frames of the atlas headlessly and collect all it painted.
    fn draw_frames(ctx: &egui::Context, viewer: &mut MapViewer) -> Vec<(String, f32)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));
        let mut painted = Vec::new();
        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| viewer.frame_ui(ui));
            painted = drawn_text(&output);
            // A test paints nothing, so the texture deltas are ours to discard.
            output.textures_delta.clear();
        }
        painted
    }

    /// Everything one frame painted, as a single bounding rectangle.
    fn painted_bounds(output: &egui::FullOutput) -> egui::Rect {
        output
            .shapes
            .iter()
            .fold(egui::Rect::NOTHING, |bounds, clipped| {
                // The shape's own extent, deliberately not its clip: whether a
                // shape is *visible* is exactly what is in question here.
                bounds.union(clipped.shape.visual_bounding_rect())
            })
    }

    /// A legend is painted *before* the map canvas, and the canvas is clipped only
    /// to itself: anything the legend paints past the space it reserved is
    /// covered by the map. A legend row is the one piece of a panel that can
    /// grow, and a row wider than its panel widens the panel's own *frame*, so
    /// the panel's background — and with it the left end of every over-long row —
    /// used to disappear behind the map.
    ///
    /// Only the legend is shown, so every shape in the output belongs to it.
    #[test]
    fn a_legend_never_paints_under_the_map_canvas() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));
        let mut canvas = egui::Rect::NOTHING;
        let mut legend_ink = egui::Rect::NOTHING;
        let mut drawn = Vec::new();

        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));

            let mut output = ctx.run_ui(input, |ui| {
                egui::Panel::right("civilizations_legend")
                    .frame(atlas_panel_frame(ui.style()))
                    .resizable(false)
                    .exact_size(CIV_LEGEND_WIDTH)
                    .show(ui, |ui| {
                        // The legend as the atlas lays it out, with the widest
                        // rows a world can put in it.
                        panel_title(ui, "🏺 The Dawn of Civilization");
                        ui.separator();
                        caption(ui, "How the world is organised");
                        for (index, tier) in SocietyTier::LADDER.iter().enumerate() {
                            society_key_row(ui, *tier, 9_999 + index, 100.0 / 5.0);
                        }
                        ui.separator();
                        // A society's name comes from the world, not from this
                        // file: the longest a name can get must still fit.
                        society_entry(
                            ui,
                            SocietyTier::States.legend_color(),
                            "The Sunlit Isle Confederacy of the Common Era",
                            false,
                        );
                    });

                let (rect, _) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                canvas = rect;
            });
            legend_ink = painted_bounds(&output);
            drawn = drawn_text(&output);
            // A test paints nothing, so the texture deltas are ours to discard.
            output.textures_delta.clear();
        }

        assert!(
            legend_ink.min.x >= canvas.max.x - 1.0,
            "the legend paints from x = {:.1} but the map canvas ends at x = {:.1}: \
             the map would be drawn over the legend's left edge",
            legend_ink.min.x,
            canvas.max.x,
        );
        // ...and it was moved, not lost: every tier of the key is still painted.
        let key_rows = drawn
            .iter()
            .filter(|(text, _)| text.contains("% of the land"))
            .count();
        assert_eq!(
            key_rows,
            SocietyTier::LADDER.len(),
            "a key row must wrap inside the panel, never be dropped from it"
        );
    }

    /// The survey sheet's own scale belongs to the survey sheet.
    ///
    /// Every scope carries its own legend, and a right panel that does not belong
    /// to the current scope is not merely the wrong key: egui lays right panels
    /// out from the window's edge *inward*, in creation order, so a stray panel
    /// also shoves the scope's real legend into the middle of the window. The
    /// Topographic Scale showing in every mode, and the dawn's society list
    /// landing mid-window instead of at the right edge, were that one bug.
    #[test]
    fn the_topographic_scale_is_shown_only_on_the_survey_sheet() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);

        // A world old enough to offer every scope, so only the scope gate — not
        // whether the data exists — decides what is painted.
        let world = Genesis::for_test(1, Stage::Civilization)
            .atlas()
            .expect("a world of seven acts");

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));

        for scope in [
            MapScope::Physical,
            MapScope::Topographic,
            MapScope::Climate,
            MapScope::Temperature,
            MapScope::Terrain,
            MapScope::Civilizations,
            MapScope::Languages,
        ] {
            let mut viewer = MapViewer::from_world(world.clone());
            viewer.scope = scope;

            // A few passes: a brand-new area measures itself before it paints.
            let mut painted = Vec::new();
            for frame in 0..3 {
                let mut input = egui::RawInput::default();
                input.screen_rect = Some(screen);
                input.time = Some(1.0 + f64::from(frame));
                let mut output = ctx.run_ui(input, |ui| viewer.frame_ui(ui));
                painted = drawn_text_rects(&output);
                output.textures_delta.clear();
            }

            let scale = painted
                .iter()
                .find(|(text, _)| text.contains("Topographic Scale"));
            assert_eq!(
                scale.is_some(),
                scope == MapScope::Topographic,
                "the topographic scale must be painted on the survey sheet alone, not in {scope:?}"
            );

            // The dawn's legend is the rightmost panel when nothing intrudes on
            // it, so its title must begin inside the rightmost panel's width:
            // a panel to its right would push the whole list inward.
            if scope == MapScope::Civilizations {
                let dawn = painted
                    .iter()
                    .find(|(text, _)| text.contains("The Dawn of Civilization"))
                    .expect("the dawn's legend is always painted in its own scope");
                assert!(
                    dawn.1.min.x >= screen.max.x - CIV_LEGEND_WIDTH,
                    "the dawn's legend must sit at the window's right edge, but it starts at \
                     x = {:.1} while the edge is at {:.1}",
                    dawn.1.min.x,
                    screen.max.x,
                );
            }
        }
    }

    /// The size the atlas draws a given style in.
    fn size_of(style: &egui::Style, text_style: TextStyle) -> f32 {
        style
            .text_styles
            .get(&text_style)
            .unwrap_or_else(|| panic!("{text_style:?} must be defined in the atlas type scale"))
            .size
    }

    #[test]
    fn the_type_scale_is_legible_and_ordered() {
        let mut style = egui::Style::default();
        configure_style(&mut style);

        let small = size_of(&style, TextStyle::Small);
        let body = size_of(&style, TextStyle::Body);
        let button = size_of(&style, TextStyle::Button);
        let heading = size_of(&style, TextStyle::Heading);

        // egui's stock `Small` is 9 pt, which is what made legend notes and the
        // secondary lines of every hover card unreadable. Anything at or below
        // 12 pt is a regression, whatever else changes.
        assert!(
            small >= 12.0,
            "Small is the atlas's caption size and must stay readable, got {small}"
        );
        // Body is the size everything is actually read in.
        assert!(body >= 15.0, "Body must be a reading size, got {body}");
        // A button's label is read as often as body text, never smaller.
        assert!(
            button >= body,
            "Button labels must not shrink below body text"
        );
        // The hierarchy has to stay a hierarchy.
        assert!(small < body, "captions must be smaller than body text");
        assert!(body < heading, "panel titles must stand out from body text");
    }

    #[test]
    fn the_hover_card_metrics_leave_room_for_the_text() {
        let mut style = egui::Style::default();
        configure_style(&mut style);

        // A card must be able to hold a sentence of body text, and the tooltip
        // that carries it must be wider than the card it draws.
        assert!(CARD_WIDTH >= 400.0, "a hover card must hold a sentence");
        assert!(
            style.spacing.tooltip_width > CARD_WIDTH,
            "the tooltip frame must be wider than the card inside it"
        );
        // Every control the atlas offers has to be a real target for a pointer.
        assert!(style.spacing.interact_size.y >= 22.0);
        assert!(style.spacing.button_padding.x >= 8.0);
    }

    /// Every run of text egui was asked to paint, with the size it was drawn at.
    fn drawn_text(output: &egui::FullOutput) -> Vec<(String, f32)> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(String, f32)>) {
            match shape {
                egui::epaint::Shape::Text(text) => {
                    let job = &text.galley.job;
                    for section in &job.sections {
                        let range = section.byte_range.clone();
                        out.push((
                            job.text[range.start.0..range.end.0].to_owned(),
                            section.format.font_id.size,
                        ));
                    }
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }

        let mut drawn = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut drawn);
        }
        drawn
    }

    /// Every run of text egui was asked to paint, with the rectangle it was
    /// painted in. Where [`drawn_text`] answers *what* a frame said, this answers
    /// *where* — which is how a panel's own edge is told from its neighbour's.
    fn drawn_text_rects(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(text) => {
                    let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                    let job = &text.galley.job;
                    for section in &job.sections {
                        let range = section.byte_range.clone();
                        out.push((job.text[range.start.0..range.end.0].to_owned(), rect));
                    }
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }

        let mut drawn = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut drawn);
        }
        drawn
    }

    /// The sizes have to survive the whole journey to the screen: context style →
    /// panel (or tooltip) → label → laid-out galley. This is the mechanism the
    /// viewer's unreadable legends and hover cards were lost in.
    #[test]
    fn every_panel_and_hover_card_is_painted_in_the_atlas_type_scale() {
        let ctx = egui::Context::default();
        // Exactly what `MapViewer::install_style` does on the viewer's first frame.
        ctx.all_styles_mut(configure_style);

        // The canvas the atlas would paint on, and a probe the pointer can hover
        // the way it hovers the map.
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let probe = egui::Rect::from_min_size(egui::pos2(40.0, 400.0), egui::vec2(300.0, 120.0));

        // Several passes: a brand-new area measures itself before it paints, and
        // a tooltip waits out its delay before it opens. The last pass is the one
        // that shows the atlas at rest, with everything on it.
        let mut drawn = Vec::new();
        for frame in 0..4 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame) * 0.75);
            // The pointer arrives over the map on the second pass and then rests
            // there: egui opens a tooltip only once the pointer has been still
            // for `tooltip_delay`, which is exactly what a reader experiences.
            if frame == 1 {
                input.events = vec![egui::Event::PointerMoved(probe.center())];
            }

            let mut output = ctx.run_ui(input, |ui| {
                egui::Panel::right("legend")
                    .frame(atlas_panel_frame(ui.style()))
                    .exact_size(330.0)
                    .show(ui, |ui| {
                        panel_title(ui, "A legend");
                        ui.label("A readout");
                        caption(ui, "A caption");
                    });
                // A hover card, opened exactly as the canvas opens one.
                ui.interact(probe, egui::Id::unique("probe"), egui::Sense::hover())
                    .on_hover_ui(|ui| {
                        card_title(ui, "A hover card");
                        caption(ui, "A caption inside the card");
                        ui.label("A body line");
                    });
            });
            drawn = drawn_text(&output);
            // A test paints nothing, so the texture deltas are ours to discard.
            output.textures_delta.clear();
        }

        let size_of = |text: &str| {
            drawn
                .iter()
                .find(|(drawn, _)| drawn == text)
                .map(|(_, size)| *size)
                .unwrap_or_else(|| panic!("{text:?} was never painted; drew {drawn:?}"))
        };

        assert_eq!(size_of("A legend"), TEXT_HEADING);
        assert_eq!(size_of("A readout"), TEXT_BODY);
        assert_eq!(size_of("A caption"), TEXT_SMALL);
        // The hover card, opened as an egui tooltip, is drawn from the context's
        // style like the panels are — never smaller.
        assert_eq!(size_of("A hover card"), TEXT_CARD_TITLE);
        assert_eq!(size_of("A caption inside the card"), TEXT_SMALL);
        assert_eq!(size_of("A body line"), TEXT_BODY);
        // And nothing anywhere falls back to egui's 9 pt default.
        for (text, size) in &drawn {
            assert!(
                *size >= TEXT_SMALL,
                "{text:?} was painted at {size} pt, below the atlas's floor"
            );
        }
    }
}
