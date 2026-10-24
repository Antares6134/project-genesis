mod themes;

use eframe::egui::{self, Color32, ColorImage, Stroke, TextureHandle, TextureOptions};
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*;
use crate::civilization::{Civilization, CivilizationWorld};
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::polar::{frozen_sea_name, is_frozen_sea, ICE_SHELF_TEMP, PACK_ICE_TEMP};
use crate::core::{relief, Grid, WorldCache};
use crate::geography::{OceanResult, ProvinceResult};
use crate::geopolitics::GeopoliticalHierarchy;
use crate::export::image::{
    get_atlas_biome_color, get_biome_name, hypsometric_band, hypsometric_color,
    hypsometric_range_label, id_to_territory_color, HYPSOMETRIC_COLORS, HYPSOMETRIC_NAMES,
};
use themes::{ClimateZone, TerrainClass, TEMPERATURE_LEGEND_ROWS};

/// How far the atlas can be zoomed out and in.
const MIN_ZOOM: f32 = 0.03;
const MAX_ZOOM: f32 = 8.0;
/// Zoom used when the reader asks the atlas to fly to a civilization's cradle.
/// A realm is a country, not a planet: the whole world stays in view, just
/// closer than the fit that shows everything.
const CIV_FOCUS_ZOOM: f32 = 0.55;
/// Zoom used when flying to one of the world's relief extremes.
const RELIEF_FOCUS_ZOOM: f32 = 0.55;

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
        matches!(self, MapScope::Land | MapScope::Water | MapScope::Everywhere)
    }

    /// True for the scope whose subject *is* the dawn civilizations.
    fn is_civilizations(self) -> bool {
        matches!(self, MapScope::Civilizations)
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
    cache: WorldCache,
    hierarchy: GeopoliticalHierarchy,
    ocean_hierarchy: GeopoliticalHierarchy,
    province_ids: Grid<u32>,
    ocean_province_ids: Grid<u32>,
    is_wasteland: Grid<bool>,
    /// The world's dawn: its civilizations, and who holds which province.
    civilizations: CivilizationWorld,
    scope: MapScope,
    tier: MapTier,
    texture: Option<TextureHandle>,
    zoom: f32,
    offset: egui::Vec2,
    show_borders: bool,
    /// Tint the civilizations of the dawn over *any* scope, not just their own.
    show_civilizations: bool,
    /// World-wide relief extremes, measured once at startup so the topographic
    /// legend never has to re-scan the 8K heightmap every frame. The positions
    /// travel with the measurements so the legend can fly the reader there.
    relief: ReliefExtremes,
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
}

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
                    peak_index: if m > acc.peak_metres { index } else { acc.peak_index },
                    trench_metres: acc.trench_metres.min(m),
                    trench_index: if m < acc.trench_metres { index } else { acc.trench_index },
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
fn sample_3d_fbm(noise: &OpenSimplex, x: f64, y: f64, z: f64, octaves: usize, lacunarity: f64, persistence: f64) -> f32 {
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
    province_ids: &Grid<u32>,
    hierarchy: &GeopoliticalHierarchy,
    id: u32,
    color_id: u32,
    x: usize,
    y: usize,
    h: usize,
    tier: MapTier,
) -> [f32; 3] {
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
    ocean_province_ids: &Grid<u32>,
    ocean_hierarchy: &GeopoliticalHierarchy,
    ocean_id: u32,
    color_id: u32,
    x: usize,
    y: usize,
    h: usize,
    tier: MapTier,
) -> [f32; 3] {
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
    province_ids: &Grid<u32>,
    hierarchy: &GeopoliticalHierarchy,
    civilizations: &CivilizationWorld,
    id: u32,
    x: usize,
    y: usize,
    h: usize,
    tier: MapTier,
    internal: bool,
) -> [f32; 3] {
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
    let frontier = |nid: u32| -> bool {
        nid == 0 || civilizations.owner_of(nid) != owner
    };
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
        color = [color[0] * 0.50, color[1] * 0.50, color[2] * 0.50];
    } else if internal && (internal_edge(right_id) || internal_edge(down_id)) {
        color = [color[0] * 0.84, color[1] * 0.84, color[2] * 0.84];
    }
    color
}

/// A short dossier for one civilization, as the Civilizations map mode shows it
/// on hover: where it rose and why, how it lives, what it believes, and how many
/// people it holds. Deliberately a card, not a monograph — the full measurements
/// stay in the console report (`--civs`), where they belong.
fn civilization_dossier(ui: &mut egui::Ui, civ: &Civilization, world: &CivilizationWorld) {
    // A hover card has to stay a card: at 360 px wide it never swallows the map.
    ui.set_max_width(360.0);

    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(
            swatch,
            2.0,
            Color32::from_rgb(civ.color[0], civ.color[1], civ.color[2]),
        );
        ui.heading(civ.name.as_str());
    });
    ui.label(
        egui::RichText::new(format!(
            "{} · founded Year {} · {}",
            civ.rise.kind.label(),
            civ.founding_year,
            civ.era.label()
        ))
        .small()
        .weak(),
    );
    ui.label(
        egui::RichText::new(format!("{} · {}", civ.year_label(), civ.isolation.label()))
            .small()
            .weak(),
    );

    ui.separator();
    ui.label(egui::RichText::new(civ.cradle_kind.describe()).small());
    for highlight in civ.rise.highlights.iter().take(3) {
        ui.label(format!("• {highlight}"));
    }

    ui.separator();
    ui.label(
        egui::RichText::new(format!(
            "{} · {}",
            civ.culture.subsistence.label(),
            civ.culture.kinship.label()
        ))
        .strong(),
    );
    ui.label(
        egui::RichText::new(format!(
            "{} · {} · {}",
            civ.culture.outlook.label(),
            civ.culture.craft.label(),
            civ.culture.literacy.label()
        ))
        .small(),
    );

    ui.add_space(4.0);
    ui.label(egui::RichText::new(civ.faith.kind.label()).strong());
    ui.label(egui::RichText::new(civ.faith.kind.creed()).small());

    ui.separator();
    ui.label(format!(
        "{} people · {} provinces · {} km²",
        thousands_u64(civ.population),
        civ.provinces.len(),
        thousands(civ.area_km2)
    ));
    if civ.contacts.is_empty() {
        ui.label(
            egui::RichText::new("No neighbour within reach.")
                .small()
                .weak(),
        );
    } else {
        let neighbours: Vec<&str> = civ
            .contacts
            .iter()
            .map(|contact| world.name_of(contact.other))
            .collect();
        ui.label(
            egui::RichText::new(format!("Neighbours: {}", neighbours.join(", ")))
                .small()
                .weak(),
        );
    }
}

impl MapViewer {
    pub fn new(
        cache: WorldCache,
        hierarchy: GeopoliticalHierarchy,
        ocean_hierarchy: GeopoliticalHierarchy,
        prov_res: ProvinceResult,
        ocean_res: OceanResult,
        civilizations: CivilizationWorld,
    ) -> Self {
        let relief = measure_relief(&cache.heightmap);
        Self {
            cache,
            hierarchy,
            ocean_hierarchy,
            province_ids: prov_res.province_ids,
            ocean_province_ids: ocean_res.ocean_province_ids,
            is_wasteland: prov_res.is_wasteland,
            civilizations,
            scope: MapScope::Physical, // Default to the stunning physical atlas view!
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
        }
    }

    /// Fit the whole world into `rect`, centred, with a small margin.
    ///
    /// This is what the **Fit** button does — and what the viewer does once, on
    /// its first frame, so the atlas opens showing the entire planet whatever
    /// size the window happens to be.
    fn fit_to(&mut self, rect: egui::Rect) {
        let available = rect.size();
        if available.x < 64.0 || available.y < 64.0 {
            return; // the panel layout has not settled yet; try next frame
        }
        let margin = 12.0;
        let map_w = self.cache.width as f32;
        let map_h = self.cache.height as f32;
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
        let v = (y / (self.cache.height.saturating_sub(1)).max(1) as f32).clamp(0.0, 1.0);
        let u = (x / self.cache.width.max(1) as f32).clamp(0.0, 1.0);
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
        let img = self.generate_texture();
        let (w, h) = (img.size[0] as u32, img.size[1] as u32);
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for px in &img.pixels {
            buf.extend_from_slice(&px.to_array());
        }
        let full = image::RgbaImage::from_raw(w, h, buf)
            .expect("RGBA buffer sized to the atlas texture");
        let plate = if (scale - 1.0).abs() < f32::EPSILON {
            full
        } else {
            let out_w = ((w as f32 * scale).round() as u32).max(1);
            let out_h = ((h as f32 * scale).round() as u32).max(1);
            image::imageops::resize(&full, out_w, out_h, image::imageops::FilterType::Lanczos3)
        };
        plate.save(path)
    }

    fn generate_texture(&self) -> ColorImage {
        let w = self.cache.width;
        let h = self.cache.height;
        let scope = self.scope;
        let tier = self.tier;
        let show_borders = self.show_borders;
        let hierarchy = &self.hierarchy;
        let ocean_hierarchy = &self.ocean_hierarchy;
        let province_ids = &self.province_ids;
        let ocean_province_ids = &self.ocean_province_ids;
        let is_wasteland = &self.is_wasteland;

        let land_political = matches!(scope, MapScope::Land | MapScope::Everywhere);
        let water_political = matches!(scope, MapScope::Water | MapScope::Everywhere);
        let civ_scope = scope.is_civilizations();
        // The civilizations' own scope always shows them; every other scope shows
        // them only when the reader asks for the overlay.
        let show_civs = self.show_civilizations || civ_scope;
        let civilizations = &self.civilizations;

        let heightmap = &self.cache.heightmap;
        let temp_grid = &self.cache.temp_grid;
        let precip_grid = &self.cache.precip_grid;
        let flow_grid = &self.cache.flow_grid;
        let is_lake = &self.cache.is_lake;

        let sea_level = relief::SEA_LEVEL;
        let min_river_flow = 1800.0f32;
        let major_river_flow = 15000.0f32;
        let lx = -0.7071f32;
        let ly = -0.7071f32;
        let render_noise = OpenSimplex::new(777);

        let mut pixels = vec![0u8; w * h * 4];

        pixels.par_chunks_exact_mut(4).enumerate().for_each(|(i, pixel)| {
            let x = i % w;
            let y = i / w;
            let elev = *heightmap.get(x as isize, y as isize);
            let id = *province_ids.get(x as isize, y as isize);
            let ocean_id = *ocean_province_ids.get(x as isize, y as isize);

            // Coordinates on 3D sphere for seamless spherical noise
            let v = y as f32 / (h - 1) as f32;
            let lat = (0.5 - v) * std::f32::consts::PI;
            let u = x as f32 / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;
            let px = (lat.cos() * lon.cos()) as f64;
            let py = lat.sin() as f64;
            let pz = (lat.cos() * lon.sin()) as f64;

            let micro_noise = sample_3d_fbm(&render_noise, px * 35.0, py * 35.0, pz * 35.0, 4, 2.0, 0.5);
            let temp = *temp_grid.get(x as isize, y as isize);
            let precip = *precip_grid.get(x as isize, y as isize);
            let display_temp = temp + micro_noise * 5.5;

            let (gx, gy) = heightmap.gradient(x as f32, y as f32);
            let slope = (gx * gx + gy * gy).sqrt();
            let hillshade = -(gx * lx + gy * ly);
            let mut shade = (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38);
            if y < 2 || y > h.saturating_sub(3) { shade = 1.0; }

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
                } else if *is_lake.get(x as isize, y as isize) {
                    [42.0, 125.0, 185.0]
                } else {
                    let altitude = elev - sea_level;
                    let base = get_atlas_biome_color(display_temp, precip, altitude);
                    let mut color = [
                        (base[0] as f32 * shade).clamp(0.0, 255.0),
                        (base[1] as f32 * shade).clamp(0.0, 255.0),
                        (base[2] as f32 * shade).clamp(0.0, 255.0),
                    ];

                    let flow = *flow_grid.get(x as isize, y as isize);
                    if flow >= min_river_flow {
                        let river_strength = ((flow - min_river_flow) / major_river_flow).sqrt().clamp(0.30, 1.0);
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

                if *is_lake.get(x as isize, y as isize) && elev > sea_level {
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
                    color = [
                        color[0] * shade,
                        color[1] * shade,
                        color[2] * shade,
                    ];
                    // Ice-capped land keeps its relief readable under a
                    // translucent veneer instead of blanking the survey out.
                    if display_temp < -3.0 {
                        color = blend_rgb(color, [238.0, 244.0, 252.0], 0.75);
                    }
                    let flow = *flow_grid.get(x as isize, y as isize);
                    if flow >= min_river_flow {
                        let river_strength = ((flow - min_river_flow) / major_river_flow).sqrt().clamp(0.30, 1.0);
                        let blend = (river_strength * 1.4).clamp(0.0, 1.0);
                        color = blend_rgb(color, [20.0, 75.0, 155.0], blend);
                    }
                }

                // A contour line along every hypsometric band boundary, the way
                // a topographic sheet steps between its elevation tints.
                let band_idx = hypsometric_band(metres);
                let right = hypsometric_band(relief::to_metres(*heightmap.get(x as isize + 1, y as isize)));
                let down = if y + 1 < h {
                    hypsometric_band(relief::to_metres(*heightmap.get(x as isize, y as isize + 1)))
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
                let lake = *is_lake.get(x as isize, y as isize);
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
                let right_band =
                    themes::isotherm_band(*temp_grid.get(x as isize + 1, y as isize));
                let down_band = if y + 1 < h {
                    themes::isotherm_band(*temp_grid.get(x as isize, y as isize + 1))
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
                let lake = *is_lake.get(x as isize, y as isize);
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
                    let flow = *flow_grid.get(x as isize, y as isize);
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
                // The physical world wearing its history: ocean and ice exactly as
                // the atlas paints them, unclaimed land muted to grey-green so the
                // cradles are the only colour on the plate, and each realm in its
                // own tint over subtle relief.
                let lake = *is_lake.get(x as isize, y as isize);
                let is_ocean = elev <= sea_level && !lake;
                let is_waste = *is_wasteland.get(x as isize, y as isize)
                    || (id == 0 && elev > sea_level && !lake);
                let owner = civilizations.owner_of(id);
                let has_land_node = id != 0 && (id as usize) < hierarchy.provinces.len();

                if is_ocean {
                    final_color = compute_physical_color();
                } else if lake {
                    final_color = [42.0, 125.0, 185.0];
                } else if owner != 0 && has_land_node {
                    let civ_rgb = civilizations.color_of(owner);
                    let phys = compute_physical_color();
                    // Flat enough to read as one political patch, tinted enough to
                    // keep the rivers, coasts and hills underneath legible.
                    final_color = [
                        civ_rgb[0] as f32 * 0.72 + phys[0] * 0.28,
                        civ_rgb[1] as f32 * 0.72 + phys[1] * 0.28,
                        civ_rgb[2] as f32 * 0.72 + phys[2] * 0.28,
                    ];

                    // The cradle itself: a small dark dot on the seat province,
                    // the one spot on the map where this realm actually began.
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
                            final_color = blend_rgb(final_color, [28.0, 22.0, 18.0], 0.8);
                        }
                    }

                    let flow = *flow_grid.get(x as isize, y as isize);
                    if flow >= min_river_flow {
                        let river_strength = ((flow - min_river_flow) / major_river_flow)
                            .sqrt()
                            .clamp(0.30, 1.0);
                        let blend = (river_strength * 1.3).clamp(0.0, 1.0);
                        final_color = blend_rgb(final_color, [20.0, 75.0, 155.0], blend);
                    }

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
                            true,
                        );
                    }
                } else if is_waste {
                    let phys = compute_physical_color();
                    final_color = [
                        phys[0] * 0.78 + 35.0,
                        phys[1] * 0.78 + 35.0,
                        phys[2] * 0.78 + 40.0,
                    ];
                } else {
                    // Unclaimed land: the same relief, drained of colour, so the
                    // cradles are the only politics on the plate.
                    let phys = compute_physical_color();
                    let grey = (phys[0] + phys[1] + phys[2]) / 3.0;
                    final_color = [
                        (phys[0] * 0.30 + grey * 0.70) * 0.92,
                        (phys[1] * 0.30 + grey * 0.70) * 0.92,
                        (phys[2] * 0.30 + grey * 0.70) * 0.92,
                    ];

                    // A dark ring on the wilderness side of every frontier, so that
                    // even a single-province realm reads as a place on a world map
                    // instead of a smudge of colour.
                    if show_borders {
                        let wraps = |nid: u32| nid != 0 && civilizations.owner_of(nid) != 0;
                        let right = *province_ids
                            .get(province_ids.wrap_x(x as isize + 1) as isize, y as isize);
                        let down = if y + 1 < h {
                            *province_ids.get(x as isize, y as isize + 1)
                        } else {
                            0
                        };
                        if wraps(right) || wraps(down) {
                            final_color = [
                                final_color[0] * 0.55,
                                final_color[1] * 0.55,
                                final_color[2] * 0.55,
                            ];
                        }
                    }
                }
            } else {
                // ── Political map modes ────────────────────────────────
                let is_ocean = elev <= sea_level && !*is_lake.get(x as isize, y as isize);
                let is_waste = *is_wasteland.get(x as isize, y as isize)
                    || (id == 0 && elev > sea_level && !*is_lake.get(x as isize, y as isize));
                let has_ocean_node = ocean_id != 0
                    && (ocean_id as usize) < ocean_hierarchy.provinces.len();
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
                            final_color, ocean_province_ids, ocean_hierarchy,
                            ocean_id, ocean_cid, x, y, h, tier,
                        );
                    }
                } else if is_ocean {
                    // Water outside the current political scope: plain atlas sea.
                    final_color = compute_physical_color();
                } else if *is_lake.get(x as isize, y as isize) {
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

                    let flow = *flow_grid.get(x as isize, y as isize);
                    if flow >= min_river_flow {
                        let river_strength = ((flow - min_river_flow) / major_river_flow).sqrt().clamp(0.30, 1.0);
                        let blend = (river_strength * 1.3).clamp(0.0, 1.0);
                        final_color = [
                            final_color[0] * (1.0 - blend) + 20.0 * blend,
                            final_color[1] * (1.0 - blend) + 75.0 * blend,
                            final_color[2] * (1.0 - blend) + 155.0 * blend,
                        ];
                    }

                    if show_borders {
                        final_color = land_border_shade(
                            final_color, province_ids, hierarchy,
                            id, color_id, x, y, h, tier,
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
                let lake = *is_lake.get(x as isize, y as isize);
                let is_ocean = elev <= sea_level && !lake;
                let owner = civilizations.owner_of(id);
                if !is_ocean && !lake && owner != 0 && id != 0 {
                    let civ_rgb = civilizations.color_of(owner);
                    let weight = if land_political || water_political { 0.45 } else { 0.62 };
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut visual_changed = false;
        // Panel clicks cannot move the map directly: the canvas rectangle for
        // this frame is only known once the panels have been laid out. Requests
        // are collected here and applied right after the canvas is allocated.
        let mut fit_request = false;
        let mut focus_request: Option<(f32, f32, f32)> = None;
        let mut clicked_civ: Option<u32> = None;

        // ── TOP CONTROLS BAR ──────────────────────────────────────────────────
        // Two rows, because the atlas now has nine map modes: what it shows, then
        // how it shows it. Both rows wrap, so a narrow window never clips a button.
        egui::Panel::top("top_controls").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("🌍 Atlas Viewer");
                ui.separator();

                // ── Map scope: what the atlas portrays ────────────────────────
                for (scope, label) in [
                    (MapScope::Physical, "🗺 Physical"),
                    (MapScope::Topographic, "⛰ Topographic"),
                    (MapScope::Climate, "🌦 Climate"),
                    (MapScope::Temperature, "🌡 Temperature"),
                    (MapScope::Terrain, "🪨 Terrain"),
                    (MapScope::Civilizations, "🏺 Civilizations"),
                    (MapScope::Land, "🏛 Land"),
                    (MapScope::Water, "🌊 Water"),
                    (MapScope::Everywhere, "🌍 Everywhere"),
                ] {
                    if ui.selectable_value(&mut self.scope, scope, label).clicked() {
                        visual_changed = true;
                        // Every scope carries its own legend (and so its own canvas
                        // width), so switching opens the new map whole.
                        fit_request = true;
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
                if !self.scope.is_civilizations() && !self.civilizations.is_empty() {
                    let world = &self.civilizations;
                    let label = format!("🏺 Civilizations ({})", world.civilizations.len());
                    if ui
                        .checkbox(&mut self.show_civilizations, label)
                        .on_hover_text(format!(
                            "{}\nThe overlay tints the provinces each realm holds, on top of whatever this scope already paints.",
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
        egui::Panel::bottom("bottom_status").show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "🏛 LAND  — Continents: {} | Superregions: {} | Regions: {} | Areas: {} | Provinces: {}",
                        self.hierarchy.total_continents,
                        self.hierarchy.total_superregions,
                        self.hierarchy.total_regions,
                        self.hierarchy.total_areas,
                        self.hierarchy.total_provinces
                    ));
                });
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "🌊 OCEAN — Basins: {} | Superregions: {} | Regions: {} | Areas: {} | Provinces: {}",
                        self.ocean_hierarchy.total_continents,
                        self.ocean_hierarchy.total_superregions,
                        self.ocean_hierarchy.total_regions,
                        self.ocean_hierarchy.total_areas,
                        self.ocean_hierarchy.total_provinces
                    ));
                });
                // ── The dawn of history ───────────────────────────────────────
                if !self.civilizations.is_empty() {
                    let world = &self.civilizations;
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "🏺 CIVILIZATIONS — Year Zero: {} | Map date: {} | {} realms | {} civilized provinces | population {} | {:.1} people/km²",
                            world.year_zero_label(),
                            world.map_year_label(),
                            world.civilizations.len(),
                            world.civilized_provinces,
                            thousands_u64(world.total_population),
                            world.mean_density_per_km2(),
                        ));
                    });
                }
            });
        });

        // ── HYPSOMETRIC LEGEND (Topographic scope only) ───────────────────────
        // The survey sheet is only readable with its scale at hand, so the
        // Topographic scope carries the palette — every band, its metre range and
        // its name — plus the world's own relief extremes. Both extremes are
        // clickable: a figure the reader cannot find is a figure worth nothing.
        if self.scope == MapScope::Topographic {
            let relief = self.relief;
            let (peak_lat, peak_lon) = self.pixel_lat_lon(relief.peak_x, relief.peak_y);
            let (trench_lat, trench_lon) = self.pixel_lat_lon(relief.trench_x, relief.trench_y);
            let peak_where = lat_lon_label(peak_lat, peak_lon);
            let trench_where = lat_lon_label(trench_lat, trench_lon);

            egui::Panel::right("hypsometric_legend")
                .resizable(false)
                .exact_size(286.0)
                .show(ui, |ui| {
                    ui.heading("⛰ Topographic Scale");
                    ui.label(
                        egui::RichText::new("Tint = elevation above / depth below the sea surface.")
                            .small()
                            .weak(),
                    );
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
                    ui.label(egui::RichText::new(peak_where).small().weak());

                    let trench_label =
                        format!("🌊 Deepest trench  {:.0} m", relief.trench_metres);
                    if ui
                        .add(egui::Button::new(trench_label).frame(false))
                        .on_hover_text(format!(
                            "{trench_where}\nClick to fly to the deepest point"
                        ))
                        .clicked()
                    {
                        let zoom = RELIEF_FOCUS_ZOOM.max(self.zoom);
                        focus_request = Some((relief.trench_x, relief.trench_y, zoom));
                    }
                    ui.label(egui::RichText::new(trench_where).small().weak());

                    ui.label(format!(
                        "🌍 Surface: {:.0}% ocean",
                        relief.ocean_share * 100.0
                    ));
                    ui.label(
                        egui::RichText::new("Click a figure above to centre the map on it.")
                            .small()
                            .weak(),
                    );
                    ui.separator();

                    // The band list is the only part that can overflow a short
                    // window, so it is the part that scrolls.
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for band in (0..HYPSOMETRIC_COLORS.len()).rev() {
                            let rgb = HYPSOMETRIC_COLORS[band];
                            ui.horizontal(|ui| {
                                let (swatch, _) = ui.allocate_exact_size(
                                    egui::vec2(16.0, 11.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    swatch,
                                    2.0,
                                    Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                                );
                                ui.label(
                                    egui::RichText::new(hypsometric_range_label(band)).small(),
                                );
                                ui.label(
                                    egui::RichText::new(HYPSOMETRIC_NAMES[band]).small().weak(),
                                );
                            });
                        }
                    });
                });
        }

        // ── CLIMATE LEGEND (Climate scope only) ───────────────────────────────
        if self.scope == MapScope::Climate {
            egui::Panel::right("climate_legend")
                .resizable(false)
                .exact_size(300.0)
                .show(ui, |ui| {
                    ui.heading("🌦 Climate Zones");
                    ui.label(
                        egui::RichText::new(
                            "Land is classified by mean temperature, annual rainfall and altitude; the sea by its surface temperature.",
                        )
                        .small()
                        .weak(),
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
                .resizable(false)
                .exact_size(300.0)
                .show(ui, |ui| {
                    ui.heading("🌡 Temperature");
                    ui.label(
                        egui::RichText::new(
                            "Mean annual surface temperature: land station means, sea-surface means over the ocean.",
                        )
                        .small()
                        .weak(),
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
                    ui.label(
                        egui::RichText::new("A thin dark line marks every 5 °C isotherm.")
                            .small()
                            .weak(),
                    );
                });
        }

        // ── TERRAIN LEGEND (Terrain scope only) ───────────────────────────────
        if self.scope == MapScope::Terrain {
            egui::Panel::right("terrain_legend")
                .resizable(false)
                .exact_size(300.0)
                .show(ui, |ui| {
                    ui.heading("🪨 Terrain Types");
                    ui.label(
                        egui::RichText::new(
                            "Landform bands from the vertical datum; the ocean floor is classified by depth.",
                        )
                        .small()
                        .weak(),
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
                .resizable(false)
                .exact_size(300.0)
                .show(ui, |ui| {
                    ui.heading("🏺 The Dawn of Civilization");
                    let world = &self.civilizations;
                    if world.is_empty() {
                        ui.label("No civilization rose on this world.");
                        ui.label(
                            egui::RichText::new(
                                "The history stage only runs when it is asked for: pass --civs (it is on automatically with --viewer and --plates).",
                            )
                            .small()
                            .weak(),
                        );
                        return;
                    }

                    ui.label(egui::RichText::new(world.year_zero_label()).small());
                    ui.label(egui::RichText::new(world.map_year_label()).small().weak());
                    ui.separator();
                    ui.label(format!(
                        "{} realms on {} civilized provinces",
                        world.civilizations.len(),
                        thousands(world.civilized_provinces as f32),
                    ));
                    ui.label(format!(
                        "{} km² held · world population {}",
                        thousands(world.civilized_area_km2),
                        thousands_u64(world.total_population),
                    ));
                    ui.label(format!(
                        "Mean density {:.1} people/km²",
                        world.mean_density_per_km2()
                    ));
                    ui.label(
                        egui::RichText::new("Click a realm to fly to its cradle.")
                            .small()
                            .weak(),
                    );
                    ui.separator();

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (index, civ) in world.civilizations.iter().enumerate() {
                            let id = index as u32 + 1;
                            ui.horizontal(|ui| {
                                let (swatch, _) = ui.allocate_exact_size(
                                    egui::vec2(16.0, 12.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    swatch,
                                    2.0,
                                    Color32::from_rgb(civ.color[0], civ.color[1], civ.color[2]),
                                );
                                let selected = self.selected_civ == Some(id);
                                if ui
                                    .selectable_label(
                                        selected,
                                        egui::RichText::new(civ.name.as_str()).strong(),
                                    )
                                    .on_hover_text("Click to fly to this realm's cradle")
                                    .clicked()
                                {
                                    clicked_civ = Some(id);
                                }
                            });
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} · founded Year {} · {}",
                                    civ.cradle_kind.label(),
                                    civ.founding_year,
                                    civ.era.label(),
                                ))
                                .small(),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} provinces · {} people",
                                    civ.provinces.len(),
                                    thousands_u64(civ.population),
                                ))
                                .small()
                                .weak(),
                            );
                            ui.separator();
                        }
                    });
                });
        }

        // A click on a realm's legend entry flies the map to that realm.
        if let Some(id) = clicked_civ {
            self.selected_civ = Some(id);
            if let Some(civ) = self.civilizations.civilization(id) {
                let zoom = CIV_FOCUS_ZOOM.max(self.zoom);
                focus_request = Some((civ.seat_x, civ.seat_y, zoom));
            }
        }

        if self.texture.is_none() || visual_changed {
            let img = self.generate_texture();
            self.texture = Some(ui.ctx().load_texture("map_texture", img, TextureOptions::NEAREST));
        }

        // ── INTERACTIVE MAP CANVAS ────────────────────────────────────────────
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        // The canvas is the only thing that knows how big the map area really is,
        // so both "fit" and "fly to" are resolved here, in the same frame the
        // click happened.
        if fit_request || self.pending_fit {
            if rect.width() >= 64.0 && rect.height() >= 64.0 {
                self.fit_to(rect);
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
                    self.offset = mouse_pos.to_vec2() - (mouse_pos.to_vec2() - self.offset) * zoom_ratio;
                }
            }
        }

        // Paint Map Mesh
        if let Some(texture) = &self.texture {
            let map_size = egui::vec2(self.cache.width as f32 * self.zoom, self.cache.height as f32 * self.zoom);
            let map_rect = egui::Rect::from_min_size(rect.min + self.offset, map_size);

            let mut mesh = egui::Mesh::with_texture(texture.id());
            mesh.add_rect_with_uv(
                map_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );

            ui.painter().with_clip_rect(rect).add(egui::Shape::mesh(mesh));

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
                    painter.circle_stroke(
                        at,
                        5.0,
                        Stroke::new(1.5, Color32::from_rgb(60, 46, 24)),
                    );
                }
            }
        }

        // ── HOVER INSPECTION TOOLTIP ──────────────────────────────────────────
        if let Some(mouse_pos) = response.hover_pos() {
            let map_x = ((mouse_pos.x - rect.min.x - self.offset.x) / self.zoom).floor() as isize;
            let map_y = ((mouse_pos.y - rect.min.y - self.offset.y) / self.zoom).floor() as isize;

            if map_x >= 0 && map_x < self.cache.width as isize && map_y >= 0 && map_y < self.cache.height as isize {
                let elev = *self.cache.heightmap.get(map_x, map_y);
                let temp = *self.cache.temp_grid.get(map_x, map_y);
                let precip = *self.cache.precip_grid.get(map_x, map_y);
                let flow = *self.cache.flow_grid.get(map_x, map_y);
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
                let v = map_y as f32 / (self.cache.height - 1) as f32;
                let lat = (0.5 - v) * std::f32::consts::PI;
                let u = map_x as f32 / self.cache.width as f32;
                let lon = (u - 0.5) * std::f32::consts::TAU;
                let hx = (lat.cos() * lon.cos()) as f64;
                let hy = lat.sin() as f64;
                let hz = (lat.cos() * lon.sin()) as f64;
                let micro_noise = sample_3d_fbm(&OpenSimplex::new(777), hx * 35.0, hy * 35.0, hz * 35.0, 4, 2.0, 0.5);
                let display_temp = temp + micro_noise * 5.5;

                let id = *self.province_ids.get(map_x, map_y);
                let ocean_id = *self.ocean_province_ids.get(map_x, map_y);
                // A frozen polar sea is "polar land": physically the very same
                // impassable wasteland as the ice-capped polar continents, but
                // politically it still belongs to a marine realm.
                let is_ocean = elev <= sea_level && !*self.cache.is_lake.get(map_x, map_y);
                let frozen_sea = is_frozen_sea(display_temp, elev, sea_level);
                let is_waste = *self.is_wasteland.get(map_x, map_y)
                    || (id == 0 && elev > sea_level && !*self.cache.is_lake.get(map_x, map_y));

                response.on_hover_ui_at_pointer(|ui| {
                    if self.scope == MapScope::Civilizations {
                        // ── The dawn of history readout ───────────────────────
                        // The whole point of this map mode: hovering a rising
                        // civilization explains why *this* spot became one.
                        let owner = self.civilizations.owner_of(id);
                        let lake = *self.cache.is_lake.get(map_x, map_y);
                        if owner != 0 && !is_ocean && !lake {
                            let civ = self
                                .civilizations
                                .civilization(owner)
                                .expect("owner id always indexes a civilization");
                            civilization_dossier(ui, civ, &self.civilizations);
                        } else if lake {
                            ui.heading("💧 Inland Freshwater Lake");
                            ui.label("No realm claims the water itself.");
                            ui.separator();
                            ui.label(format!("Lake Surface: {:.0} m", relief::altitude_metres(elev)));
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else if is_ocean {
                            ui.heading("🌊 Uncivilized Sea");
                            ui.label(format!("Depth: {:.0} m", relief::depth_metres(elev)));
                            ui.separator();
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else if is_waste {
                            ui.heading("🏔️ Wasteland Beyond History");
                            ui.strong("Impassable or unclaimed wilderness");
                            ui.separator();
                            ui.label(format!("Environment: {}", get_biome_name(display_temp, precip, altitude)));
                            ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        } else {
                            ui.heading("🌾 Unclaimed Land");
                            ui.strong("No civilization has risen here");
                            ui.label(
                                egui::RichText::new(
                                    "Fertile enough to live on, but never the best ground in reach: nothing here outperformed the cradles.",
                                )
                                .small()
                                .weak(),
                            );
                            ui.separator();
                            ui.label(format!("Environment: {}", get_biome_name(display_temp, precip, altitude)));
                            ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                            ui.label(format!("Temperature: {:.1} °C", temp));
                            ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                            ui.separator();
                            ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                        }
                    } else if self.scope == MapScope::Topographic {
                        // ── Hypsometric survey readout ────────────────────────
                        let band = hypsometric_band(metres);
                        let surface = if is_ocean {
                            if frozen_sea { frozen_sea_name(display_temp) } else { "Open Ocean" }
                        } else if *self.cache.is_lake.get(map_x, map_y) {
                            "Inland Freshwater Lake"
                        } else if display_temp < -3.0 {
                            "Ice-capped Land"
                        } else {
                            get_biome_name(display_temp, precip, altitude)
                        };

                        ui.heading(format!("⛰ {}", surface));
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
                        let lake = *self.cache.is_lake.get(map_x, map_y);
                        if lake {
                            ui.heading("🌦 Inland Lake");
                            ui.label("Fresh water, read with the climate of its shores.");
                        } else if is_ocean {
                            ui.heading(format!("🌦 {}", themes::ocean_climate_label(temp)));
                            ui.separator();
                            ui.label(format!("Sea-surface temperature: {:.1} °C", temp));
                            if frozen_sea {
                                ui.label("Perennial sea ice.");
                            }
                        } else {
                            let zone = ClimateZone::classify(display_temp, precip, altitude);
                            ui.heading(format!("🌦 {}", zone.label()));
                            ui.label(egui::RichText::new(zone.describe()).small().weak());
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
                        let lake = *self.cache.is_lake.get(map_x, map_y);
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
                        ui.heading(format!("🌡 {:.1} °C", temp));
                        ui.label(egui::RichText::new(surface).small().weak());
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
                        let lake = *self.cache.is_lake.get(map_x, map_y);
                        let metres = relief::to_metres(elev);
                        let class = TerrainClass::classify(metres, lake, display_temp);
                        let (gx, gy) =
                            self.cache.heightmap.gradient(map_x as f32, map_y as f32);
                        let slope = (gx * gx + gy * gy).sqrt();
                        ui.heading(format!("🪨 {}", class.label()));
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
                        } else if *self.cache.is_lake.get(map_x, map_y) {
                            "Inland Freshwater Lake"
                        } else {
                            get_biome_name(display_temp, precip, altitude)
                        };

                        ui.heading(format!("🌍 {}", biome_title));
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
                        && (ocean_id as usize) < self.ocean_hierarchy.provinces.len()
                    {
                        // ── Hydro-political marine realm ───────────────
                        let node = &self.ocean_hierarchy.provinces[ocean_id as usize];
                        if frozen_sea {
                            ui.heading(format!("🧊 Ocean Province #{}", ocean_id));
                            ui.strong(format!("{} · Impassable Polar Wasteland", frozen_sea_name(display_temp)));
                        } else {
                            ui.heading(format!("🌊 Ocean Province #{}", ocean_id));
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
                            ui.heading("🧊 Polar Ice Wasteland");
                            ui.strong("Permanent Ice Sheet / Impassable Wilderness");
                        } else {
                            ui.heading("🏔️ Wasteland");
                            ui.strong("Impassable / Uninhabited Wilderness");
                        }
                        ui.separator();
                        ui.label(format!("Environment: {}", get_biome_name(display_temp, precip, altitude)));
                        ui.label(format!("Elevation: {:.0} m", relief::altitude_metres(elev)));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if *self.cache.is_lake.get(map_x, map_y) {
                        ui.heading("💧 Inland Freshwater Lake");
                        ui.separator();
                        ui.label(format!("Lake Surface: {:.0} m", relief::altitude_metres(elev)));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if id != 0 && (id as usize) < self.hierarchy.provinces.len() {
                        let node = &self.hierarchy.provinces[id as usize];

                        ui.heading(format!("📍 Province #{}", id));
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
                        // ── The realm this province belongs to ────────────────
                        // Shown in every scope, because the province's population
                        // and its civilization are facts about the province, not
                        // about the map mode currently on screen.
                        let owner = self.civilizations.owner_of(id);
                        if owner != 0 {
                            ui.horizontal(|ui| {
                                ui.label("Civilization:");
                                ui.strong(self.civilizations.name_of(owner));
                            });
                        }
                        ui.separator();
                        ui.label(format!("Size: {} px", node.pixel_count));
                        if owner != 0 {
                            ui.label(format!(
                                "Population: {} people ({:.1}/km²)",
                                thousands_u64(self.civilizations.population_of(id)),
                                self.civilizations.population_of(id) as f32
                                    / (node.pixel_count as f32
                                        * crate::core::sphere::pixel_area_km2(
                                            map_y as usize,
                                            self.cache.height,
                                            self.cache.width
                                        ))
                                        .max(1.0),
                            ));
                            ui.label(format!(
                                "Realm population: {} people",
                                thousands_u64(
                                    self.civilizations
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
                        ui.heading("🌊 Ocean");
                        ui.label(format!("Depth: {:.0} m", relief::depth_metres(elev)));
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    }
                });
            }
        }
    }
}
