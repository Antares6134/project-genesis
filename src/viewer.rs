mod themes;

use eframe::egui::{
    self, Color32, ColorImage, FontFamily, FontId, Stroke, TextStyle, TextureHandle, TextureOptions,
};
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*;
use crate::civilization::SocietyTier;
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
    /// Whether the atlas's type scale has been installed on this frame's style.
    /// A style change makes egui rebuild its font atlas, so it is done once.
    style_installed: bool,
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

/// A partitioned world carries hundreds of societies, and building a widget for
/// every one of them every frame would cost more than the map it describes: the
/// legend lists the first few of each tier, in the order the atlas assembled them,
/// and says how many it left out.
const LISTED_PER_TIER: usize = 24;

/// A short dossier for one civilization, as the Civilizations map mode shows it
/// on hover: where it rose and why, how it lives, what it believes, and how many
/// people it holds. Deliberately a card, not a monograph — the full measurements
/// stay in the console report (`--civs`), where they belong.
fn civilization_dossier(ui: &mut egui::Ui, civ: &Civilization, world: &CivilizationWorld) {
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
            style_installed: false,
        }
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
        if !self.style_installed {
            self.install_style(ui);
        }
        // Every panel wears the atlas's own frame: the stock one is padded for a
        // 13 pt interface, and this atlas reads at 15 pt.
        let panel_frame = atlas_panel_frame(ui.style());

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
        egui::Panel::top("top_controls")
            .frame(panel_frame.clone())
            .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                panel_title(ui, "🌍 Atlas Viewer");
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
                // quietly loses the right-hand half of the world's census.
                status_line(
                    ui,
                    format!(
                        "🏛 LAND — Continents: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}",
                        self.hierarchy.total_continents,
                        self.hierarchy.total_superregions,
                        self.hierarchy.total_regions,
                        self.hierarchy.total_areas,
                        self.hierarchy.total_provinces
                    ),
                );
                status_line(
                    ui,
                    format!(
                        "🌊 OCEAN — Basins: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}",
                        self.ocean_hierarchy.total_continents,
                        self.ocean_hierarchy.total_superregions,
                        self.ocean_hierarchy.total_regions,
                        self.ocean_hierarchy.total_areas,
                        self.ocean_hierarchy.total_provinces
                    ),
                );
                // ── The dawn of history ───────────────────────────────────────
                if !self.civilizations.is_empty() {
                    let world = &self.civilizations;
                    status_line(
                        ui,
                        format!(
                            "🏺 CIVILIZATIONS — {} · Atlas date: {} · {} · {} realms · {} civilized provinces · population {} · {:.1} people/km²",
                            world.epoch_label(),
                            world.map_year_label(),
                            world.age_label(),
                            world.civilizations.len(),
                            world.civilized_provinces,
                            thousands_u64(world.total_population),
                            world.mean_density_per_km2(),
                        ),
                    );
                }
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
                    let world = &self.civilizations;
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
                        let lake = *self.cache.is_lake.get(map_x, map_y);
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
                        let lake = *self.cache.is_lake.get(map_x, map_y);
                        let metres = relief::to_metres(elev);
                        let class = TerrainClass::classify(metres, lake, display_temp);
                        let (gx, gy) =
                            self.cache.heightmap.gradient(map_x as f32, map_y as f32);
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
                        } else if *self.cache.is_lake.get(map_x, map_y) {
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
                        && (ocean_id as usize) < self.ocean_hierarchy.provinces.len()
                    {
                        // ── Hydro-political marine realm ───────────────
                        let node = &self.ocean_hierarchy.provinces[ocean_id as usize];
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
                    } else if *self.cache.is_lake.get(map_x, map_y) {
                        card_title(ui, "💧 Inland Freshwater Lake");
                        ui.separator();
                        ui.label(format!("Lake Surface: {:.0} m", relief::altitude_metres(elev)));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if id != 0 && (id as usize) < self.hierarchy.provinces.len() {
                        let node = &self.hierarchy.provinces[id as usize];

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
                        let owner = self.civilizations.owner_of(id);
                        if owner != 0 {
                            ui.horizontal(|ui| {
                                ui.label("Society:");
                                ui.strong(self.civilizations.name_of(owner));
                            });
                            if let Some(tier) = self.civilizations.tier_of(id) {
                                let environment = self
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
                                "Society population: {} people",
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
