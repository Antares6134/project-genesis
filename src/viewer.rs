mod i18n;
mod settings;
mod themes;

pub use i18n::Language;

use crate::civilization::SocietyTier;
use crate::civilization::{Civilization, CivilizationWorld, Faith, MetaphysicalRoot, Religion};
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::polar::{
    GLACIAL_LAND_TEMP, ICE_SHELF_TEMP, PACK_ICE_TEMP, frozen_sea_name, is_frozen_sea,
};
use crate::core::progress::{ProgressEvent, Sink};
use crate::core::{Grid, relief};
use crate::export::image::{
    HYPSOMETRIC_COLORS, HYPSOMETRIC_NAMES, get_atlas_biome_color, get_biome_name, hypsometric_band,
    hypsometric_color, hypsometric_range_label, id_to_territory_color,
};
use crate::geopolitics::{GeopoliticalHierarchy, ProvinceNode};
use crate::language::LanguagesWorld;
use crate::pipeline::{
    AtlasWorld, CachedWorld, Genesis, LoadError, Stage, StageReport, StageState, WorldGenSettings,
    cached_worlds,
};
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontFamily, FontId, Stroke, TextStyle, TextureHandle,
    TextureOptions,
};
use noise::{NoiseFn, OpenSimplex};
use rand::RngExt;
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
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

// ─── THE CHROME ──────────────────────────────────────────────────────────────
//
// The window and the empty planet are one material. The plate a world has not
// been grown on is painted in [`CHROME_INK`], and every panel, chip and
// separator is a step up from that same ink: the atlas reads as one dark
// instrument, not as a dark map inside a grey dialog. One accent is spent, and
// only on the one thing the reader chose — the active scope, the active tier,
// the selected text.

/// The deepest colour in the atlas: the cold-start plate and the canvas's ink.
const CHROME_INK: Color32 = Color32::from_rgb(9, 12, 22);
/// Panels and the top bar: one step up from the plate.
const CHROME_PANEL: Color32 = Color32::from_rgb(18, 23, 36);
/// A chip's fill — the seed and the world's status.
const CHROME_CHIP: Color32 = Color32::from_rgb(27, 34, 52);
/// Edges: chip borders, separators, disabled controls.
const CHROME_EDGE: Color32 = Color32::from_rgb(46, 58, 84);
/// The one accent: what is selected, and nothing else.
const CHROME_ACCENT: Color32 = Color32::from_rgb(88, 130, 200);
/// Text on a chip: bright enough to read against [`CHROME_CHIP`].
const CHROME_CHIP_TEXT: Color32 = Color32::from_rgb(214, 224, 242);
/// The ink, thinned: a strip laid over the map without hiding it.
const CHROME_VEIL: Color32 = Color32::from_rgba_unmultiplied_const(9, 12, 22, 224);
/// The panel, thinned: the zoom pill floating over the map.
const CHROME_PANEL_VEIL: Color32 = Color32::from_rgba_unmultiplied_const(18, 23, 36, 235);

/// Width of the label column in a civilization dossier, so values line up.
const DOSSIER_LABEL_WIDTH: f32 = 92.0;

/// The width of a legend panel. The five thematic legends are all keys, so they
/// share one width; the dawn of history is the only one that lists societies by
/// name, and gets a little more room for it.
const LEGEND_WIDTH: f32 = 330.0;
const CIV_LEGEND_WIDTH: f32 = 350.0;

/// The zoom pill that floats over the map's bottom-right corner. It sits on
/// the canvas rather than in the bar because the camera belongs to the map, and
/// because a bar that carries everything ends up two rows tall on a small
/// window. Its size is fixed so the pill never resizes as the percentage goes
/// from 3% to 800%.
const ZOOM_PILL_WIDTH: f32 = 274.0;
const ZOOM_PILL_HEIGHT: f32 = 38.0;
/// How far the pill floats from the canvas's corner.
const ZOOM_PILL_MARGIN: f32 = 12.0;

/// The atlas's icons come from two faces, both bundled and both subset to
/// exactly the characters the interface uses.
///
/// [`ICON_FONT`] is the **picture** face. The interface's pictograms are emoji
/// codepoints written straight into its lines — and into all six translations of
/// them — so a font, not a call site, decides how they look. OpenMoji gives every
/// one of them the same flat, outlined drawing on every platform, instead of
/// whatever the desktop's font fallback happens to pick (see
/// `assets/fonts/README.md`).
///
/// [`SYMBOL_FONT`] is the **mark** face. The three typographic symbols of the
/// chrome — `☰` the panel toggle, `↺` reset, `→` "to" in a tooltip — are not
/// pictures and must not be painted as though they were; but they must not be
/// borrowed from the desktop either, or the atlas would look different on every
/// machine. A monochrome face with the same clean stroke keeps them consistent
/// with the type and tinted by the theme.
const ICON_FONT: &str = "openmoji";
const SYMBOL_FONT: &str = "atlas-marks";
/// OpenMoji draws in a 1.29 em advance; this brings an icon back to about one
/// em, so `🌊 Water` stays a row of text and not a banner. egui shrinks its own
/// default emoji the same way (0.81 for Noto Emoji).
const ICON_SCALE: f32 = 0.85;

/// Install the atlas's two faces once: the picture face and the mark face,
/// appended to both families as their last *configured* fallbacks.
///
/// Both deliberately go at [`egui::FontPriority::Lowest`], not `Highest`: egui
/// measures a row, and places its baseline, with the family's **primary** face,
/// so a font at the front would re-metric every line of the atlas. Being the last
/// configured fallbacks keeps Ubuntu-Light primary and still beats the desktop:
/// egui tries every installed font before it asks the system's font provider for
/// one, so a pictogram or a mark is answered by one of these faces rather than by
/// whatever fonts the machine happens to have.
///
/// The picture face is a COLRv0/CPAL colour font, which is why `Cargo.toml`
/// enables epaint's `color_fonts` feature: without it the layers would not be
/// painted. The mark face is an ordinary monochrome face and is tinted by the
/// text colour like any other glyph.
fn install_icon_fonts(ctx: &egui::Context) {
    let lowest = |family: egui::FontFamily| egui::InsertFontFamily {
        family,
        priority: egui::FontPriority::Lowest,
    };
    ctx.add_font(egui::FontInsert::new(
        ICON_FONT,
        egui::FontData::from_static(include_bytes!("../assets/fonts/OpenMoji-color-subset.ttf"))
            .tweak(egui::FontTweak {
                scale: ICON_SCALE,
                ..Default::default()
            }),
        vec![
            lowest(egui::FontFamily::Proportional),
            lowest(egui::FontFamily::Monospace),
        ],
    ));
    ctx.add_font(egui::FontInsert::new(
        SYMBOL_FONT,
        egui::FontData::from_static(include_bytes!("../assets/fonts/AtlasMarks-subset.ttf")),
        vec![
            lowest(egui::FontFamily::Proportional),
            lowest(egui::FontFamily::Monospace),
        ],
    ));
}

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

    // The colours last, because a palette never changes a measurement: the
    // type scale above is what the atlas is read at, and it stays exactly as
    // it was. What changes is the material the chrome is cut from.
    style.visuals = atlas_visuals();
}

/// The atlas's own visuals: the navy chrome the panels and the empty plate
/// share, in place of egui's default grey.
///
/// This is a palette, not a restyle. [`configure_style`] owns the type scale,
/// the spacing and the hit targets; this owns only the colours, so a theme
/// change can never quietly change how a row of text is measured. Every state
/// of a widget gets the same corner and the same hairline, so a button is a
/// quiet chip of chrome until the pointer arrives and the accent answers it.
fn atlas_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = CHROME_PANEL;
    visuals.window_fill = CHROME_PANEL;
    visuals.extreme_bg_color = CHROME_INK;
    visuals.faint_bg_color = CHROME_CHIP;
    visuals.code_bg_color = CHROME_CHIP;
    visuals.window_stroke = Stroke::new(1.0, CHROME_EDGE);
    visuals.hyperlink_color = CHROME_ACCENT;
    visuals.selection.bg_fill = CHROME_ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.window_corner_radius = egui::CornerRadius::same(6);
    visuals.menu_corner_radius = egui::CornerRadius::same(6);

    let widgets = &mut visuals.widgets;
    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.corner_radius = egui::CornerRadius::same(4);
        state.bg_stroke = Stroke::new(1.0, CHROME_EDGE);
    }
    widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.weak_bg_fill = CHROME_CHIP;
    widgets.hovered.weak_bg_fill = Color32::from_rgb(40, 52, 78);
    widgets.active.weak_bg_fill = CHROME_ACCENT;
    widgets.open.weak_bg_fill = CHROME_CHIP;
    visuals
}

/// The atlas's panel frame: egui's stock panel frame with the inner margin the
/// larger type needs (the stock one is sized for a 13 pt interface).
fn atlas_panel_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin::symmetric(12, 5))
}

/// The room egui's floating scroll bar needs at a panel's right edge, in points.
///
/// The bar is painted *on top of* the content — the default
/// [`egui::style::ScrollStyle::floating`] allocates no width for it — and it
/// senses presses across its full `bar_width` whether or not it is expanded, so
/// anything flush with that edge sits under it. A control there never sees the
/// press: the bar takes it, expands under the pointer and pages the panel
/// instead, which is how the ↺ that ends a settings row became unclickable.
fn scroll_bar_gutter(ui: &egui::Ui) -> f32 {
    let scroll = ui.spacing().scroll;
    if scroll.floating {
        // The band itself, plus a little air so a control's edge and the bar's
        // never touch.
        scroll.bar_width + 4.0
    } else {
        // A solid bar allocates its own column, so the content is already clear.
        0.0
    }
}

/// The content margin that keeps a scroll area's contents out of the band the
/// floating scroll bar claims for itself (see [`scroll_bar_gutter`]).
fn clear_of_scroll_bar(ui: &egui::Ui) -> egui::Margin {
    egui::Margin {
        right: scroll_bar_gutter(ui).round() as i8,
        ..egui::Margin::ZERO
    }
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

/// One chip in the top bar: a short, atomic figure — the seed, the world's
/// status — set on its own plate so it reads as a fact and not as a control.
///
/// The label is deliberately *extended* rather than wrapped. A chip is one
/// line by definition: a long seed must not be split across two rows, and a
/// phrase must not be broken down the middle. That is what keeps the bar's
/// height a constant of the window rather than a function of the seed — the
/// canvas below it must never move because a number got longer.
fn chip(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    egui::Frame::new()
        .fill(CHROME_CHIP)
        .stroke(Stroke::new(1.0, CHROME_EDGE))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text.into())
                        .size(TEXT_BODY)
                        .color(CHROME_CHIP_TEXT),
                )
                .extend(),
            )
        })
        .response
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
    /// The world's religions: one colour family per metaphysical root, one shade
    /// per faith.
    Religions,
    /// The world's language families and their languages.
    Languages,
    /// Political subdivision of the landmasses only.
    Land,
    /// Hydro-political subdivision of the world ocean only.
    Water,
    /// Both land and ocean political subdivisions at once.
    Everywhere,
}

/// How many scopes at the head of [`MapScope::ALL`] are the physical survey.
/// The scope menu draws its hairline here: the land as it is, then the people
/// who live on it.
const PHYSICAL_SCOPES: usize = 5;

impl MapScope {
    /// Every scope, in the order the atlas offers them: the physical survey
    /// first — relief, the survey sheet, climate, temperature, landforms — and
    /// then the people the world grew, from the dawn to its tongues.
    pub const ALL: [MapScope; 11] = [
        MapScope::Physical,
        MapScope::Topographic,
        MapScope::Climate,
        MapScope::Temperature,
        MapScope::Terrain,
        MapScope::Civilizations,
        MapScope::Religions,
        MapScope::Languages,
        MapScope::Land,
        MapScope::Water,
        MapScope::Everywhere,
    ];

    /// The scope's name, as the bar and its menu write it. One spelling for
    /// every place a scope is named, so the button and the menu can never drift.
    fn label(self) -> &'static str {
        match self {
            MapScope::Physical => "🗺 Physical",
            MapScope::Topographic => "⛰ Topographic",
            MapScope::Climate => "🌦 Climate",
            MapScope::Temperature => "🌡 Temperature",
            MapScope::Terrain => "🪨 Terrain",
            MapScope::Civilizations => "🏺 Civilizations",
            MapScope::Religions => "🛐 Religions",
            MapScope::Languages => "🗣 Languages",
            MapScope::Land => "🏛 Land",
            MapScope::Water => "🌊 Water",
            MapScope::Everywhere => "🌍 Everywhere",
        }
    }

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

    /// True for the scope whose subject is the world's religions.
    fn is_religions(self) -> bool {
        matches!(self, MapScope::Religions)
    }

    /// True for the scope whose subject is the world's languages.
    fn is_languages(self) -> bool {
        matches!(self, MapScope::Languages)
    }

    /// True for the scopes the Civilizations overlay may tint. The scopes whose
    /// *subject* is a people — their realms, their faiths, their tongues — never
    /// take it: the tint would repeat what is already painted.
    fn accepts_civ_overlay(self) -> bool {
        !self.is_civilizations() && !self.is_religions() && !self.is_languages()
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
            MapScope::Civilizations | MapScope::Religions | MapScope::Languages => Stage::History,
        }
    }

    /// True once the world is old enough for this scope to be painted.
    fn available(self, world: Stage) -> bool {
        world.index() >= self.min_stage().index()
    }

    /// The scope's name as a file name: the same words the headless plates use
    /// (`atlas_land_provinces.png`), so an export and a printed plate of the
    /// same map mode are named alike.
    fn slug(self) -> &'static str {
        match self {
            MapScope::Physical => "physical",
            MapScope::Topographic => "topographic",
            MapScope::Climate => "climate",
            MapScope::Temperature => "temperature",
            MapScope::Terrain => "terrain",
            MapScope::Civilizations => "civilizations",
            MapScope::Religions => "religions",
            MapScope::Languages => "languages",
            MapScope::Land => "land",
            MapScope::Water => "water",
            MapScope::Everywhere => "everywhere",
        }
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

impl MapTier {
    /// Every tier, from the finest to the coarsest, in the order the picker
    /// offers them.
    pub const ALL: [MapTier; 5] = [
        MapTier::Provinces,
        MapTier::Areas,
        MapTier::Regions,
        MapTier::Superregions,
        MapTier::Continents,
    ];

    /// The tier's name as the bar and its picker write it: the same words the
    /// legend and the census use for the same rung of the hierarchy.
    fn label(self) -> &'static str {
        match self {
            MapTier::Provinces => "Provinces",
            MapTier::Areas => "Areas",
            MapTier::Regions => "Regions",
            MapTier::Superregions => "Superregions",
            MapTier::Continents => "Continents",
        }
    }

    /// The tier's name as a file name. The ocean hierarchy's top tier is its
    /// *basins*, which is what the headless water plate calls it.
    fn slug(self) -> &'static str {
        match self {
            MapTier::Provinces => "provinces",
            MapTier::Areas => "areas",
            MapTier::Regions => "regions",
            MapTier::Superregions => "superregions",
            MapTier::Continents => "continents",
        }
    }
}

/// The map resolutions the atlas offers, from the smallest plate to the tuned
/// 16K one.
///
/// A resolution is part of a world's identity, exactly like its seed: the same
/// seed grown at 8K and at 16K are two different planets. The panel therefore
/// chooses the canvas before act one, and a world already grown must be reset
/// before its canvas can change.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum MapResolution {
    K2,
    K4,
    K8,
    K12,
    K16,
    /// A canvas supplied on the command line that is not one of the presets.
    Custom(usize, usize),
}

impl MapResolution {
    /// Every preset, in the order the picker offers them.
    pub const ALL: [MapResolution; 5] = [
        MapResolution::K2,
        MapResolution::K4,
        MapResolution::K8,
        MapResolution::K12,
        MapResolution::K16,
    ];

    /// The interactive atlas starts on the 8K plate: detailed enough for the
    /// whole hierarchy to breathe, small enough to grow in a few seconds.
    pub const DEFAULT: MapResolution = MapResolution::K8;

    /// The canvas size this choice means, in pixels.
    pub fn size(self) -> (usize, usize) {
        match self {
            MapResolution::K2 => (2048, 1024),
            MapResolution::K4 => (4096, 2048),
            MapResolution::K8 => (8192, 4096),
            MapResolution::K12 => (12288, 6144),
            MapResolution::K16 => (16384, 8192),
            MapResolution::Custom(width, height) => (width, height),
        }
    }

    /// The label the picker and the saved-world rows show.
    pub fn label(self) -> String {
        let (width, height) = self.size();
        match self {
            MapResolution::K2 => format!("2K · {width} × {height}"),
            MapResolution::K4 => format!("4K · {width} × {height}"),
            MapResolution::K8 => format!("8K · {width} × {height}"),
            MapResolution::K12 => format!("12K · {width} × {height}"),
            MapResolution::K16 => format!("16K · {width} × {height}"),
            MapResolution::Custom(..) => format!("{width} × {height}"),
        }
    }

    /// Read a canvas back as a preset, or as a custom size when it is not one.
    pub fn from_canvas(width: usize, height: usize) -> Self {
        Self::ALL
            .into_iter()
            .find(|preset| preset.size() == (width, height))
            .unwrap_or(MapResolution::Custom(width, height))
    }
}

/// Where an export is written: the project's existing gallery directory, beside
/// the headless `atlas_*.png` plates.
const EXPORTS_DIR: &str = "./exports";

/// The sizes the export menu offers: the two plate sizes the atlas has always
/// printed, plus the world's own size when it is neither of them.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum ExportChoice {
    K2,
    K8,
    Native,
}

impl ExportChoice {
    /// Every choice, in the order the menu offers them.
    const ALL: [ExportChoice; 3] = [ExportChoice::K2, ExportChoice::K8, ExportChoice::Native];

    /// The size this choice means for a world of `native` pixels.
    fn size(self, native: (usize, usize)) -> (usize, usize) {
        match self {
            ExportChoice::K2 => MapResolution::K2.size(),
            ExportChoice::K8 => MapResolution::K8.size(),
            ExportChoice::Native => native,
        }
    }

    /// True when this export neither enlarges the world nor stretches it.
    ///
    /// A 16K world may be downsampled to 8K or 2K; a 2K world may not be
    /// enlarged to 8K. A canvas whose aspect ratio is not the plate's own is
    /// only ever exportable at its native size, because the preset sizes would
    /// have to stretch it.
    fn allowed(self, native: (usize, usize)) -> bool {
        let (width, height) = self.size(native);
        width <= native.0 && height <= native.1 && width * native.1 == height * native.0
    }

    /// True when the menu should list this choice at all. *Native* earns a row
    /// only when it is not one of the two plate sizes above — an 8K world does
    /// not need two identical 8K entries.
    fn offered(self, native: (usize, usize)) -> bool {
        match self {
            ExportChoice::Native => {
                native != MapResolution::K2.size() && native != MapResolution::K8.size()
            }
            _ => true,
        }
    }

    /// The label the menu shows, in the reader's language.
    fn label(self, native: (usize, usize), language: Language) -> String {
        match self {
            ExportChoice::K2 => MapResolution::K2.label(),
            ExportChoice::K8 => MapResolution::K8.label(),
            ExportChoice::Native => language.fill("Native · {} × {}", &[&native.0, &native.1]),
        }
    }
}

/// Everything about *how* a plate is painted, copied out of the window so an
/// export can paint the very same plate on its own thread.
#[derive(Clone, Copy)]
struct PlateView {
    scope: MapScope,
    tier: MapTier,
    show_borders: bool,
    show_civilizations: bool,
    min_river_flow: f32,
    major_river_flow: f32,
}

/// One export's file name: the seed, the map mode on screen (the scope, and the
/// tier only where the tier is part of the map mode), the civilizations overlay
/// when it is tinting a scope that is not about them, and the plate's size.
fn export_file_name(
    seed: u64,
    scope: MapScope,
    tier: MapTier,
    civilizations_overlay: bool,
    size: (usize, usize),
) -> String {
    let mut name = format!("world_{seed}_{}", scope.slug());
    if scope.is_political() || scope.is_civilizations() {
        let tier = if scope == MapScope::Water && tier == MapTier::Continents {
            "basins"
        } else {
            tier.slug()
        };
        name.push('_');
        name.push_str(tier);
    }
    if civilizations_overlay && scope.accepts_civ_overlay() {
        name.push_str("_civs");
    }
    format!("{name}_{}x{}.png", size.0, size.1)
}

/// The path an export should be written to, never overwriting an earlier one: a
/// second export of the same map mode and size is `…-2.png`, then `…-3.png`.
fn free_export_path(name: &str) -> std::path::PathBuf {
    free_export_path_in(std::path::Path::new(EXPORTS_DIR), name)
}

/// [`free_export_path`], against a directory a test can choose.
fn free_export_path_in(directory: &std::path::Path, name: &str) -> std::path::PathBuf {
    let stem = name.strip_suffix(".png").unwrap_or(name);
    let mut path = directory.join(name);
    let mut copy = 2;
    while path.exists() {
        path = directory.join(format!("{stem}-{copy}.png"));
        copy += 1;
    }
    path
}

/// A finished export, handed back from its worker.
struct ExportOutcome {
    path: std::path::PathBuf,
    size: (usize, usize),
    outcome: Result<(), String>,
}

/// Paint, resize and encode one export.
///
/// Runs on a worker thread: a 16K plate is several seconds of painting and a
/// good many more of PNG compression, neither of which the window can afford to
/// wait for. The plate is painted at the world's own size — the atlas's one
/// painter — and only then downsampled, with the same Lanczos3 filter the
/// headless plates use, so an export and a printed plate resampled to the same
/// size agree.
fn write_export(
    world: &AtlasWorld,
    view: PlateView,
    target: (usize, usize),
    path: &std::path::Path,
) -> Result<(), String> {
    let plate = MapViewer::generate_texture(world, view);
    let full = rgba_plate(&plate);
    let plate = if (full.width() as usize, full.height() as usize) == target {
        full
    } else {
        image::imageops::resize(
            &full,
            target.0 as u32,
            target.1 as u32,
            image::imageops::FilterType::Lanczos3,
        )
    };
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|why| why.to_string())?;
    }
    plate
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|why| why.to_string())
}

/// The plate as four-byte RGBA, which is what a PNG wants. `ColorImage` holds
/// `Color32`s; this is the one conversion, shared by the headless plates and the
/// interactive export.
fn rgba_plate(image: &ColorImage) -> image::RgbaImage {
    let (width, height) = (image.size[0] as u32, image.size[1] as u32);
    let mut buffer = Vec::with_capacity((width * height * 4) as usize);
    for pixel in &image.pixels {
        buffer.extend_from_slice(&pixel.to_array());
    }
    image::RgbaImage::from_raw(width, height, buffer)
        .expect("RGBA buffer sized to the atlas texture")
}

/// Edge of the largest texture the atlas must be able to upload: a finished 16K
/// plate is one `WORLD_WIDTH × WORLD_HEIGHT` RGBA texture (16384 × 8192).
///
/// egui-wgpu asks the device for only `8192` — enough for any 4K display, but a
/// world plate is not a screen, and wgpu rejects the upload as
/// `egui_texid_Managed` before the atlas has drawn a frame.
const ATLAS_TEXTURE_DIMENSION: u32 = 16384;

/// Bytes the atlas must be able to stage at once: one 16K RGBA plate is 512 MiB,
/// and wgpu's default device asks for only 256 MiB.
const ATLAS_BUFFER_BYTES: u64 = 512 << 20;

/// Raise the two device limits a 16K plate needs, up to what the adapter can do.
///
/// They are *requests*, so each is clamped to `adapter`: a GPU that cannot hold
/// a 16K texture still gets a device (and would still refuse the plate, which is
/// the honest outcome — there is nothing to show it with).
fn atlas_device_limits(
    mut limits: eframe::wgpu::Limits,
    adapter: &eframe::wgpu::Limits,
) -> eframe::wgpu::Limits {
    limits.max_texture_dimension_2d = ATLAS_TEXTURE_DIMENSION.min(adapter.max_texture_dimension_2d);
    limits.max_buffer_size = ATLAS_BUFFER_BYTES.min(adapter.max_buffer_size);
    limits
}

/// The wgpu configuration the atlas opens with.
///
/// Everything egui-wgpu chose is kept — instance backends, power preference, the
/// backend-aware base limits and the device label — and only the requested
/// limits are overridden, through the very descriptor egui-wgpu would have used.
pub fn wgpu_configuration() -> eframe::WgpuConfiguration {
    let mut configuration = eframe::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut configuration.wgpu_setup {
        let egui_descriptor = Arc::clone(&setup.device_descriptor);
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = egui_descriptor(adapter);
            descriptor.required_limits =
                atlas_device_limits(descriptor.required_limits, &adapter.limits());
            descriptor
        });
    }
    configuration
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
    /// The PNG export painting and encoding on its own thread, if one is.
    export_job: Option<Receiver<ExportOutcome>>,
    /// What the last export said: what it is doing, where it went, or why it
    /// could not be written.
    export_notice: Option<String>,
    /// Set when the plate on screen went out of date while an export was
    /// painting. The repaint waits until the export finishes, so two full
    /// plates — a 16K one is half a gigabyte — are never painted at once.
    plate_stale: bool,
    /// The seed as the reader typed it, which is not always a valid world.
    seed_text: String,
    /// The canvas chosen for the next world. It is applied at once before act
    /// one, and only through a reset once a world exists.
    resolution: MapResolution,
    /// A sentence about the last cache attempt, when there is one worth saying.
    cache_notice: Option<String>,
    /// Every world cache on disk as of the last scan.
    cached: Vec<CachedWorld>,
    /// True once the cache directory has been read at least once.
    cached_scanned: bool,
    /// Set when an act may have written a cache, so the list is read again.
    cached_dirty: bool,
    /// The last thing worth saying about the world, in one line: what an act
    /// produced, what a save wrote, why an act did not finish. The panel keeps
    /// the acts themselves — which are done, how long each took — so this is a
    /// note beside them, not a log to read back.
    notice: Option<String>,
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
    /// so the topographic legend never has to re-scan the 16K heightmap every
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
    /// The religion selected in the Religions legend, by 1-based id.
    selected_religion: Option<u16>,
    /// The language family whose entry in the legend was last clicked.
    selected_family: Option<u16>,
    /// Whether the atlas's type scale has been installed on this frame's style.
    /// A style change makes egui rebuild its font atlas, so it is done once.
    style_installed: bool,
    /// The language the interface is read in. English until the reader — or the
    /// desktop's locale — says otherwise.
    language: Language,
    /// Whether the genesis panel is on screen. The map may claim the whole window
    /// once a world exists; the top bar stays either way, so the toggle is always
    /// within reach.
    show_genesis_panel: bool,
}

/// The live plate of an act in flight: its label, and the texture it is painted
/// into, which is replaced as the act reports new ground.
struct LivePlate {
    /// The act's own sub-step label, kept in English so it can be translated
    /// afresh when the reader changes language mid-run.
    label: &'static str,
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
fn stage_progress(job: Option<&StageJob>, language: Language) -> String {
    match job {
        Some(job) if job.total > 0 => format!(
            "{} — {:.0}%",
            language.text(job.sub),
            100.0 * job.done as f32 / job.total as f32
        ),
        // A flood fill that pops until its heap is empty has no scale to be a
        // percentage of: it reports what it has drained so far instead.
        Some(job) => format!(
            "{} — {}",
            language.text(job.sub),
            thousands(job.done as f32)
        ),
        None => language.text("working").to_string(),
    }
}

/// A cache's size in the unit a reader thinks in.
///
/// A world cache is hundreds of megabytes, so bytes would be a row of digits
/// that says less than "544 MB" does.
fn cache_size(bytes: u64, language: Language) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let megabytes = bytes as f64 / MB;
    if megabytes >= 1024.0 {
        language.fill("{} GB", &[&format!("{:.1}", megabytes / 1024.0)])
    } else {
        language.fill("{} MB", &[&format!("{megabytes:.0}")])
    }
}

/// How long ago a cache was written, in the few words a row has room for.
fn cache_age(modified: Option<SystemTime>, language: Language) -> String {
    let Some(modified) = modified else {
        return language.text("at an unknown time").to_string();
    };
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        // A clock that disagrees with the file's timestamp is not worth a wrong
        // age: say only that it is there.
        return language.text("just now").to_string();
    };
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => language.text("just now").to_string(),
        60..=3_599 => language.fill("{} min ago", &[&(secs / 60)]),
        3_600..=86_399 => language.fill("{} h ago", &[&(secs / 3_600)]),
        86_400..=2_591_999 => language.fill("{} d ago", &[&(secs / 86_400)]),
        _ => language.fill("{} mo ago", &[&(secs / 2_592_000)]),
    }
}

/// Which acts a save holds, in the few words a row has room for: `acts 1–5`, or
/// `a saved world` when the file predates the header that names them — the acts
/// are then knowable only by decoding the payload, which the list does not do.
fn acts_saved(acts: Option<u8>, language: Language) -> String {
    match acts {
        Some(acts) => language.fill("acts 1–{}", &[&acts]),
        None => language.text("a saved world").to_string(),
    }
}

/// Whether the Save command has anything to write.
///
/// One act is enough — there is a heightmap — and nothing withdraws the command
/// afterwards: a finished world, dawn and all, is still a world worth keeping.
fn can_save(stage: Option<Stage>) -> bool {
    stage.is_some()
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
        Stage::History => MapScope::Civilizations,
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
fn lat_lon_label(lat: f32, lon: f32, language: Language) -> String {
    format!(
        "{:.1}° {} · {:.1}° {}",
        lat.abs(),
        language.text(if lat >= 0.0 { "N" } else { "S" }),
        lon.abs(),
        language.text(if lon >= 0.0 { "E" } else { "W" }),
    )
}

/// `1. Geology & Erosion` — an act's number and its name, in the reader's tongue.
///
/// The name itself lives on [`Stage`], in English, because the console reports
/// are English; this is the one place the viewer puts the two together.
fn act_label(language: Language, stage: Stage) -> String {
    format!("{}. {}", stage.index(), language.text(stage.title()))
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
    ancestor_id(&hierarchy.provinces[id as usize], tier)
}

/// The id one node of the hierarchy carries at `tier`.
#[inline(always)]
fn ancestor_id(node: &ProvinceNode, tier: MapTier) -> u32 {
    match tier {
        MapTier::Provinces => node.id,
        MapTier::Areas => node.area_id,
        MapTier::Regions => node.region_id,
        MapTier::Superregions => node.superregion_id,
        MapTier::Continents => node.continent_id,
    }
}

/// The tier a plate draws as its *frontier*, if it has one.
///
/// A plate shows its own fills and the frontier of the tier above it: the Areas
/// plate draws the Region skeleton hard and its own subdivisions faintly, as an
/// atlas draws departments inside a province. The Province plate is the base
/// grain and has no frontier above it in this sense.
fn parent_tier(tier: MapTier) -> Option<MapTier> {
    match tier {
        MapTier::Provinces | MapTier::Continents => None,
        MapTier::Areas => Some(MapTier::Regions),
        MapTier::Regions => Some(MapTier::Superregions),
        MapTier::Superregions => Some(MapTier::Continents),
    }
}

/// How hard a tier's own subdivision line is inked.
///
/// The ramp rises with coarseness — an area boundary is finer than a region's —
/// except that the Province plate is the base grain and must stay readable on
/// its own, with no parent skeleton to lean on.
fn tier_border_weight(tier: MapTier) -> f32 {
    match tier {
        MapTier::Provinces => 0.44,
        MapTier::Areas => 0.34,
        MapTier::Regions => 0.42,
        MapTier::Superregions => 0.50,
        MapTier::Continents => 0.58,
    }
}

/// The id count a hierarchy *claims* for a tier, used to size a palette even on
/// a world whose province list is shorter than its census.
fn declared_total(hierarchy: &GeopoliticalHierarchy, tier: MapTier) -> u32 {
    match tier {
        MapTier::Provinces => hierarchy.total_provinces as u32,
        MapTier::Areas => hierarchy.total_areas as u32,
        MapTier::Regions => hierarchy.total_regions as u32,
        MapTier::Superregions => hierarchy.total_superregions as u32,
        MapTier::Continents => hierarchy.total_continents as u32,
    }
}

/// How many sibling shades a parent's children may be dealt.
const SIBLING_SLOTS: usize = 6;

/// Sibling offsets applied to the parent's own hue, saturation and value. One
/// slot of each kind of shift, so even a pair of neighbours that happen to land
/// on neighbouring hues still reads as two areas.
const SIBLING_HUE: [f32; SIBLING_SLOTS] = [0.0, 0.085, -0.085, 0.042, -0.042, 0.125];
const SIBLING_SAT: [f32; SIBLING_SLOTS] = [1.0, 0.90, 1.08, 0.82, 1.0, 0.94];
const SIBLING_VAL: [f32; SIBLING_SLOTS] = [1.0, 0.89, 0.95, 0.81, 1.05, 0.90];

fn rgb_to_hsv(rgb: [f32; 3]) -> (f32, f32, f32) {
    let r = (rgb[0] / 255.0).clamp(0.0, 1.0);
    let g = (rgb[1] / 255.0).clamp(0.0, 1.0);
    let b = (rgb[2] / 255.0).clamp(0.0, 1.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        (((b - r) / delta) + 2.0) / 6.0
    } else {
        (((r - g) / delta) + 4.0) / 6.0
    };
    let saturation = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    (hue, saturation, max)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let h = hue.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - f * saturation);
    let t = value * (1.0 - (1.0 - f) * saturation);
    let (r, g, b) = match sector as i32 % 6 {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    [r * 255.0, g * 255.0, b * 255.0]
}

/// One child's shade of the family colour its parent wears.
fn sibling_color(parent: [f32; 3], slot: usize) -> [f32; 3] {
    let slot = slot % SIBLING_SLOTS;
    let (hue, saturation, value) = rgb_to_hsv(parent);
    hsv_to_rgb(
        hue + SIBLING_HUE[slot],
        (saturation * SIBLING_SAT[slot]).clamp(0.26, 0.80),
        (value * SIBLING_VAL[slot]).clamp(0.60, 0.99),
    )
}

/// One colour per id of `tier`, computed once per plate.
///
/// Regions and Areas are coloured as *families*: a Region takes its
/// Superregion's hue with one of a handful of sibling shades, and an Area takes
/// its Region's. Adjacent siblings are never dealt the same shade (a greedy
/// colouring of the sibling graph), so the plate shows both the structure and
/// the grouping — which is exactly the difference between an atlas and a
/// confetti scatter. Provinces, Superregions and Continents keep the
/// golden-ratio wheel they have always worn; only the two middle tiers nest.
fn territory_palette(hierarchy: &GeopoliticalHierarchy, tier: MapTier) -> Vec<[f32; 3]> {
    let max_id = hierarchy
        .provinces
        .iter()
        .filter(|p| p.id != 0)
        .map(|p| ancestor_id(p, tier))
        .max()
        .unwrap_or(0)
        .max(declared_total(hierarchy, tier));

    let mut colors: Vec<[f32; 3]> = (0..=max_id).map(|id| id_to_territory_color(id)).collect();
    if colors.is_empty() {
        return colors;
    }
    colors[0] = [0.0, 0.0, 0.0];

    // Only Regions and Areas nest; their parent's palette is already resolved.
    if !matches!(tier, MapTier::Regions | MapTier::Areas) {
        return colors;
    }
    let Some(parent) = parent_tier(tier) else {
        return colors;
    };
    let base = territory_palette(hierarchy, parent);

    // Sibling ids per parent, and who among them actually touch.
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut siblings: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for p in hierarchy.provinces.iter() {
        if p.id == 0 {
            continue;
        }
        let cid = ancestor_id(p, tier);
        let pid = ancestor_id(p, parent);
        if cid != 0 && pid != 0 {
            children.entry(pid).or_default().push(cid);
        }
    }
    for p in hierarchy.provinces.iter() {
        if p.id == 0 {
            continue;
        }
        let cid = ancestor_id(p, tier);
        if cid == 0 {
            continue;
        }
        for &(nb, _) in &p.neighbors {
            let Some(q) = hierarchy.provinces.get(nb as usize) else {
                continue;
            };
            if q.id == 0 {
                continue;
            }
            let ncid = ancestor_id(q, tier);
            if ncid == 0 || ncid == cid {
                continue;
            }
            // Cousins inherit different parent hues, so only siblings compete
            // for a shade.
            if ancestor_id(q, parent) != ancestor_id(p, parent) {
                continue;
            }
            siblings.entry(cid).or_default().insert(ncid);
        }
    }

    // Greedy colouring, in id order: the first shade none of its neighbours wear.
    let mut slot_of: BTreeMap<u32, usize> = BTreeMap::new();
    for (_parent, kids) in children {
        let mut kids = kids;
        kids.sort_unstable();
        kids.dedup();
        for cid in kids {
            let mut used = [false; SIBLING_SLOTS];
            if let Some(neighbours) = siblings.get(&cid) {
                for neighbour in neighbours {
                    if let Some(&slot) = slot_of.get(neighbour) {
                        used[slot] = true;
                    }
                }
            }
            let slot = (0..SIBLING_SLOTS).find(|&slot| !used[slot]).unwrap_or(0);
            slot_of.insert(cid, slot);
        }
    }

    for p in hierarchy.provinces.iter() {
        if p.id == 0 {
            continue;
        }
        let cid = ancestor_id(p, tier);
        if cid == 0 || cid as usize >= colors.len() {
            continue;
        }
        let pid = ancestor_id(p, parent);
        let parent_color = base
            .get(pid as usize)
            .copied()
            .unwrap_or([214.0, 207.0, 188.0]);
        let slot = slot_of.get(&cid).copied().unwrap_or(0);
        colors[cid as usize] = sibling_color(parent_color, slot);
    }

    colors
}

/// Ink a land pixel that sits on the frontier of the currently displayed tier.
///
/// A plate draws the tier above as its *frontier*: on the Areas plate the Region
/// skeleton is inked hard (two pixels, so it survives the downscale to a 2K
/// plate) while the Area subdivisions inside a Region stay faint. The coast sits
/// between the two — real, but never louder than a realm's edge.
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
    let right_id = *province_ids.get(province_ids.wrap_x(x as isize + 1) as isize, y as isize);
    let left_id = *province_ids.get(province_ids.wrap_x(x as isize - 1) as isize, y as isize);
    let down_id = if y + 1 < h {
        *province_ids.get(x as isize, y as isize + 1)
    } else {
        id
    };
    let up_id = if y > 0 {
        *province_ids.get(x as isize, y as isize - 1)
    } else {
        id
    };

    let own = tier_border_weight(tier);

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

    // The frontier of the tier above, when there is one: the Region skeleton on
    // the Areas plate, the Superregion skeleton on the Regions plate, …
    let parent = parent_tier(tier);
    let parent_here = parent.map(|parent| tier_color_id(hierarchy, id, parent));
    let crosses_parent = |nid: u32| -> bool {
        let (Some(parent), Some(here)) = (parent, parent_here) else {
            return false;
        };
        if nid == 0 || nid == id || (nid as usize) >= hierarchy.provinces.len() {
            return false;
        }
        let there = tier_color_id(hierarchy, nid, parent);
        there != 0 && here != 0 && there != here
    };

    if crosses_parent(right_id)
        || crosses_parent(down_id)
        || crosses_parent(left_id)
        || crosses_parent(up_id)
        || (parent.is_none() && (differs(right_id) || differs(down_id)))
    {
        return blend_rgb(color, CIV_INK, (own + 0.22).min(0.62));
    }

    if differs(right_id) || differs(down_id) {
        if right_id == 0 || down_id == 0 {
            // A shore is a real edge, but it belongs to the coast, not to a realm.
            color = blend_rgb(color, CIV_INK, (own + 0.10).min(0.60));
        } else {
            color = blend_rgb(color, CIV_INK, own);
        }
    }
    color
}

/// Ink an ocean pixel that sits on a border between two marine realms.
///
/// Coastlines are intentionally *not* drawn: the shore is already unmistakable,
/// and drawing it would trace every continent with a heavy outline. Only
/// province-to-province transitions within the ocean produce a frontier line,
/// weighted exactly as the land's are: the tier above hard, the tier itself faint.
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
    let right_id = *ocean_province_ids.get(
        ocean_province_ids.wrap_x(x as isize + 1) as isize,
        y as isize,
    );
    let left_id = *ocean_province_ids.get(
        ocean_province_ids.wrap_x(x as isize - 1) as isize,
        y as isize,
    );
    let down_id = if y + 1 < h {
        *ocean_province_ids.get(x as isize, y as isize + 1)
    } else {
        ocean_id
    };
    let up_id = if y > 0 {
        *ocean_province_ids.get(x as isize, y as isize - 1)
    } else {
        ocean_id
    };

    let own = tier_border_weight(tier);

    let differs = |nid: u32| -> bool {
        if nid == 0 || nid == ocean_id {
            return false;
        }
        if (nid as usize) >= ocean_hierarchy.provinces.len() {
            return false;
        }
        tier_color_id(ocean_hierarchy, nid, tier) != color_id
    };

    let parent = parent_tier(tier);
    let parent_here = parent.map(|parent| tier_color_id(ocean_hierarchy, ocean_id, parent));
    let crosses_parent = |nid: u32| -> bool {
        let (Some(parent), Some(here)) = (parent, parent_here) else {
            return false;
        };
        if nid == 0 || nid == ocean_id || (nid as usize) >= ocean_hierarchy.provinces.len() {
            return false;
        }
        let there = tier_color_id(ocean_hierarchy, nid, parent);
        there != 0 && here != 0 && there != here
    };

    if crosses_parent(right_id)
        || crosses_parent(down_id)
        || crosses_parent(left_id)
        || crosses_parent(up_id)
        || (parent.is_none() && (differs(right_id) || differs(down_id)))
    {
        return blend_rgb(color, CIV_INK, (own + 0.22).min(0.62));
    }

    if differs(right_id) || differs(down_id) {
        color = blend_rgb(color, CIV_INK, own);
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

/// Darken a pixel on a religion frontier.
///
/// Two weights, for the same reason the language map has two: the hard edge is
/// between two different metaphysical roots, the softer one between two religions
/// of the same family. Comparing the roots rather than the shades matters,
/// because two religions of one root can share a shade when they are far apart —
/// the frontier between them is still a real frontier. The soft weight is heavy
/// enough to carry a boundary even where two relatives of one family were given
/// two close tones: a reader must be able to see where one creed ends without
/// consulting the key.
#[inline]
fn religion_border_shade(
    mut color: [f32; 3],
    province_ids: Option<&Grid<u32>>,
    civilizations: &CivilizationWorld,
    id: u32,
    x: usize,
    y: usize,
    h: usize,
) -> [f32; 3] {
    let Some(province_ids) = province_ids else {
        return color;
    };
    let here = civilizations
        .province_religion
        .get(id as usize)
        .copied()
        .unwrap_or(0);
    if here == 0 {
        return color;
    }
    let right_id = *province_ids.get(province_ids.wrap_x(x as isize + 1) as isize, y as isize);
    let down_id = if y + 1 < h {
        *province_ids.get(x as isize, y as isize + 1)
    } else {
        id
    };

    let mut hard = false;
    let mut soft = false;
    for other_id in [right_id, down_id] {
        if other_id == 0 || other_id == id {
            continue;
        }
        let other = civilizations
            .province_religion
            .get(other_id as usize)
            .copied()
            .unwrap_or(0);
        if other == 0 || other == here {
            continue;
        }
        let (Some(one), Some(two)) = (civilizations.religion(here), civilizations.religion(other))
        else {
            continue;
        };
        if one.faith.root != two.faith.root {
            hard = true;
        } else {
            soft = true;
        }
    }
    if hard {
        color = blend_rgb(color, CIV_INK, 0.55);
    } else if soft {
        color = blend_rgb(color, CIV_INK, 0.40);
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
/// on hover: a compact record of what it is, where it lives and how many people
/// it holds. Deliberately a card, not a monograph.
fn civilization_dossier(
    ui: &mut egui::Ui,
    civ_id: u32,
    civ: &Civilization,
    civilizations: &CivilizationWorld,
    languages: &LanguagesWorld,
    language: Language,
) {
    // A hover card has to stay a card: at 430 px it never swallows the map.
    ui.set_max_width(CARD_WIDTH);

    ui.horizontal(|ui| {
        themes::swatch(ui, civ.color);
        card_title(ui, civ.name.as_str());
    });
    caption(
        ui,
        format!("{} · {}", civ.culture.outlook.label(), civ.tier.mobility()),
    );

    ui.separator();
    dossier_row(ui, "Society:", civ.tier.form());
    dossier_row(
        ui,
        if civ.tier.is_state() {
            "Cradle:"
        } else {
            "Terrain:"
        },
        if civ.tier.is_state() {
            civ.cradle_kind.label()
        } else {
            civ.environment.label()
        },
    );
    dossier_row(ui, "Economy:", civ.culture.subsistence.label());
    if let Some(religion) = civilizations.dominant_religion_of(civ_id) {
        faith_summary(ui, &religion.faith);
        if let Some(census) = civilizations.religion_of_realm(civ_id)
            && census.faiths.len() > 1
        {
            dossier_row(ui, "Faiths kept:", &census.faiths.len().to_string());
        }
    } else {
        dossier_row(ui, "Faith:", "None recorded");
    }
    caption(
        ui,
        language.text(
            "Liturgy, powers, cosmos, ethos, rites and holy places are on the Religions map.",
        ),
    );
    dossier_row(ui, "Polity:", civ.culture.government.label());
    dossier_row(ui, "Hierarchy:", civ.culture.hierarchy.label());

    ui.separator();
    dossier_row(
        ui,
        if civ.tier.is_state() {
            "Capital:"
        } else {
            "Heartland:"
        },
        &format!("Province #{}", civ.seat_province),
    );
    dossier_row(ui, "Area:", &format!("{} km²", thousands(civ.area_km2)));
    dossier_row(
        ui,
        "Population:",
        &format!(
            "{} ({:.1}/km²)",
            thousands_u64(civ.population),
            civ.density_per_km2
        ),
    );

    ui.separator();
    dossier_row(ui, "Culture:", "Later");
    if let Some(language) = languages.language_of_civ(civ_id) {
        dossier_row(ui, "Language:", &language.name);
        dossier_row(ui, "Autonym:", &language.autonym);
    }

    ui.separator();
    dossier_row(ui, "Founded:", &civ.origin_label());
    lineage_row(ui, "Predecessor:", "Predecessors:", &[]);
    lineage_row(ui, "Successor:", "Successors:", &[]);
}

/// The two lines every map mode keeps about a religion: its own name and the
/// shape of its creed. The Civilizations card stops here; the Religions card
/// continues with [`faith_details`].
fn faith_summary(ui: &mut egui::Ui, faith: &Faith) {
    dossier_row(ui, "Faith:", &faith.display_name());
    dossier_row(ui, "Creed:", &faith.creed());
}

/// Everything a faith holds beyond its name and creed: the liturgy, the powers
/// it reveres, the cosmos it tells, the ethos it asks for, and the rites, polity,
/// emblem and holy places that carry it.
fn faith_details(ui: &mut egui::Ui, faith: &Faith) {
    if !faith.names.liturgy.is_empty() {
        dossier_row(ui, "Liturgy:", &faith.names.liturgy);
    }
    dossier_block(
        ui,
        &format!("{}:", faith.deity_title()),
        &faith.deity_lines(),
    );
    dossier_row(ui, "Cosmos:", &faith.cosmos());
    dossier_row(ui, "Ethos:", &faith.ethos());
    dossier_row(ui, "Taboo:", &faith.taboo());
    dossier_row(ui, "Path:", &faith.path());
    dossier_row(ui, "Rites:", &faith.praxis());
    dossier_row(ui, "Temple:", &faith.polity());
    dossier_row(ui, "Manifest:", &faith.manifestation());
    dossier_row(ui, "Emblem:", &faith.emblem_name());
    if !faith.sacred_sites.is_empty() {
        let title = format!("{}:", faith.sacred_site_title());
        let lines = faith.sacred_site_lines();
        if lines.len() == 1 {
            dossier_row(ui, &title, &lines[0]);
        } else {
            dossier_block(ui, &title, &lines);
        }
    }
    if !faith.cults.is_empty() {
        dossier_row(ui, "Cults:", &faith.cult_names());
    }
}

/// The full religion card the Religions map mode shows on hover: the faith, its
/// root colour family, every realm and province that keeps it, and the whole
/// cascade beneath the name.
fn religion_dossier(
    ui: &mut egui::Ui,
    religion: &Religion,
    province: u32,
    owner: &Civilization,
    world: &AtlasWorld,
    language: Language,
) {
    // A hover card has to stay a card: at 430 px it never swallows the map.
    ui.set_max_width(CARD_WIDTH);
    let faith = &religion.faith;

    ui.horizontal(|ui| {
        themes::swatch(ui, religion.color);
        card_title(ui, faith.display_name());
    });
    dossier_row(ui, "Creed:", &faith.creed());
    caption(ui, language.text("Metaphysical root"));
    ui.horizontal(|ui| {
        themes::swatch(ui, faith.root.legend_color());
        ui.add(
            egui::Label::new(egui::RichText::new(language.text(faith.root.label())).small()).wrap(),
        );
    });

    ui.separator();
    let realms = if religion.realms.len() == 1 {
        "1 realm".to_string()
    } else {
        format!("{} realms", religion.realms.len())
    };
    dossier_row(
        ui,
        "Kept by:",
        &format!(
            "{realms} · {} {}",
            religion.provinces.len(),
            religion.province_count_label()
        ),
    );
    let followers_share = if world.civilizations.total_population > 0 {
        100.0 * religion.population as f32 / world.civilizations.total_population as f32
    } else {
        0.0
    };
    dossier_row(
        ui,
        "Followers:",
        &format!(
            "{} ({followers_share:.1}% of the world)",
            thousands_u64(religion.population)
        ),
    );
    dossier_row(
        ui,
        "Provinces:",
        &format!(
            "{} · {} km²",
            religion.provinces.len(),
            thousands(religion.area_km2)
        ),
    );
    dossier_row(
        ui,
        "Here:",
        &format!("{} — province #{province}", owner.name),
    );
    // A province can keep a faith the realm's majority does not. Say so, because
    // that difference is the whole point of a province-level map.
    let province_faith = world
        .civilizations
        .province_religion
        .get(province as usize)
        .copied()
        .unwrap_or(0);
    if let Some(census) = world
        .civilizations
        .religion_of_realm(world.civilizations.owner_of(province))
        && census.dominant != province_faith
        && let Some(dominant) = world.civilizations.religion(census.dominant)
    {
        dossier_row(ui, "Realm's faith:", &dominant.faith.display_name());
    }
    if let Some(site) = faith.sacred_sites.first() {
        dossier_row(
            ui,
            "Holy city:",
            &format!("{} — province #{}", site.name, site.province),
        );
    }

    ui.separator();
    faith_details(ui, faith);
}

/// One aligned row of a civilization dossier: a fixed-width label and a
/// wrapped value. The faith block carries long creed lines, and a horizontal
/// layout will otherwise let them run past the card's width.
fn dossier_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        ui.add_sized(
            [DOSSIER_LABEL_WIDTH, height],
            egui::Label::new(egui::RichText::new(label)),
        );
        ui.add(egui::Label::new(value).wrap());
    });
}

/// A labelled block whose values are one indented line each: the deity list and
/// the sacred-site list. The label is printed once, then the lines sit under it.
fn dossier_block(ui: &mut egui::Ui, label: &str, lines: &[String]) {
    dossier_row(ui, label, "");
    for line in lines {
        ui.horizontal(|ui| {
            ui.add_space(DOSSIER_LABEL_WIDTH);
            ui.add(egui::Label::new(line).wrap());
        });
    }
}

/// A lineage row. The label switches to its plural as soon as a realm has more
/// than one predecessor or successor; until lineage data exists the value is
/// `None`.
fn lineage_row(ui: &mut egui::Ui, singular: &str, plural: &str, names: &[&str]) {
    let label = if names.len() >= 2 { plural } else { singular };
    let value = if names.is_empty() {
        "None".to_string()
    } else {
        names.join(", ")
    };
    dossier_row(ui, label, &value);
}

/// One row of the five-way key: a tier's colour, its name, and how much of the
/// world's inhabited land the tier holds.
///
/// The line is **wrapped, never widened**. A legend panel is painted *before* the
/// map canvas and is only ever clipped to itself, so a row wider than the panel
/// widens the panel's own frame: the map, painted afterwards, then covers the
/// overhang — and the reader loses the left end of every over-long row behind
/// the map. Keep every row of a legend inside the width it was given.
fn society_key_row(
    ui: &mut egui::Ui,
    tier: SocietyTier,
    count: usize,
    share: f32,
    language: Language,
) {
    ui.horizontal(|ui| {
        themes::swatch(ui, tier.legend_color());
        // `.wrap()` is the whole point: egui lets a label in a horizontal row
        // extend past the row (a vertical layout wraps, a horizontal one does
        // not), and a wrapped line costs the row a second line instead of the
        // panel its left edge.
        ui.add(
            egui::Label::new(
                egui::RichText::new(language.fill(
                    "{} — {} societies · {}% of the land",
                    &[&language.text(tier.label()), &count, &format!("{share:.1}")],
                ))
                .small(),
            )
            .wrap(),
        );
    });
}

/// One row of the nine-family key: a root's main colour, its name, and how much
/// of the world's inhabited land its religions hold. Wrapped for the same reason
/// as [`society_key_row`]: a legend row may take a second line, never a wider
/// panel.
fn religion_key_row(
    ui: &mut egui::Ui,
    root: MetaphysicalRoot,
    count: usize,
    share: f32,
    language: Language,
) {
    ui.horizontal(|ui| {
        themes::swatch(ui, root.legend_color());
        ui.add(
            egui::Label::new(
                egui::RichText::new(language.fill(
                    "{} — {} religions · {}% of the land",
                    &[&language.text(root.label()), &count, &format!("{share:.1}")],
                ))
                .small(),
            )
            .wrap(),
        );
    });
}

/// One society's entry in the dawn legend: its colour, its name, and the two
/// lines that place it — as a block that spans the legend's full width.
///
/// The name is a button that flies the map to the society's heartland, and it
/// is given every pixel the row has left: what the reader hovers — and what is
/// highlighted when the realm is selected — is the whole row, not just the
/// word. Its label stays flush with the row's edge. egui's `left_to_right`
/// layout centres a button's label within it (see [`egui::Layout::left_to_right`]),
/// which would indent every name by a different amount, so the row's layout is
/// overridden to start at the left.
///
/// The two caption lines are indented to the name's column rather than left on
/// the panel's edge: an entry then reads as one block — swatch and name, then
/// what the society is and how many people it holds.
///
/// A society's name is *data* — the atlas did not choose it — so the button
/// truncates rather than widen the row. Widening a row widens the panel's frame
/// (see [`society_key_row`]), and the map then covers the panel's left edge.
///
/// Returns the name button's response; its rectangle is the row's full width.
fn society_entry(
    ui: &mut egui::Ui,
    color: [u8; 3],
    name: &str,
    detail: &str,
    census: &str,
    tooltip: &str,
    selected: bool,
) -> egui::Response {
    let mut response = None;
    ui.horizontal(|ui| {
        themes::swatch(ui, color);
        ui.with_layout(
            // A horizontal layout aligns a label to its *centre* by default
            // ("looks best to e.g. center text within a button"); this row's
            // label has to start at the row's own left edge.
            egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
            |ui| {
                // The frame begins where the name does, so the lines under it
                // line up with the label rather than with the frame's padding.
                ui.spacing_mut().button_padding.x = 0.0;
                let width = ui.available_width();
                let height = ui.spacing().interact_size.y;
                response = Some(
                    ui.add(
                        egui::Button::selectable(selected, egui::RichText::new(name).strong())
                            .min_size(egui::vec2(width, height))
                            .truncate(),
                    )
                    .on_hover_text(tooltip),
                );
            },
        );
    });

    // The name's column: the swatch and the row's gap, and nothing else.
    let indent = themes::swatch_width(ui) + ui.spacing().item_spacing.x;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            caption(ui, detail);
            caption(ui, census);
        });
    });

    response.expect("a society entry always draws its name")
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

    /// The atlas in the reader's own language, chosen once when the window opens.
    pub fn with_language(mut self, language: Language) -> Self {
        self.language = language;
        self
    }

    /// One interface line in the language the atlas is being read in.
    ///
    /// English is the key: the line is written at the call site, and the
    /// translation table holds the other tongues (see [`i18n`]).
    fn t(&self, english: &'static str) -> &'static str {
        self.language.text(english)
    }

    /// A line with `{}` placeholders, filled in order in the current language.
    fn tf(&self, template: &'static str, args: &[&dyn std::fmt::Display]) -> String {
        self.language.fill(template, args)
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
        let (canvas_width, canvas_height) = world
            .as_ref()
            .map_or_else(|| genesis.canvas(), |world| (world.width, world.height));
        let resolution = MapResolution::from_canvas(canvas_width, canvas_height);
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
            export_job: None,
            export_notice: None,
            plate_stale: false,
            seed_text,
            resolution,
            cache_notice: None,
            cached: Vec::new(),
            cached_scanned: false,
            cached_dirty: true,
            notice: None,
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
            selected_religion: None,
            selected_family: None,
            style_installed: false,
            // Deterministic on purpose: the tests and the headless plates read
            // English, and `main` decides what the desktop's reader asked for.
            language: Language::English,
            show_genesis_panel: true,
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
        let settings = self.genesis.settings().clone();
        // Before act one the reader's resolution is authoritative; once a world
        // exists it is already fixed in the pipeline and this is a no-op.
        if self.genesis.stage().is_none() {
            let (chosen_width, chosen_height) = self.resolution.size();
            self.genesis.set_canvas(chosen_width, chosen_height);
        }
        let (width, height) = self.genesis.canvas();
        let mut genesis = Genesis::new_sized(self.genesis.seed(), width, height);
        std::mem::swap(&mut genesis, &mut self.genesis);
        // The pipeline left behind is a placeholder, but the panel still asks it
        // what it would do: without this the Save choice would flicker off for the
        // length of every act, and the settings would read as defaults.
        self.genesis.set_autosave(autosave);
        self.genesis.set_settings(settings);
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

        self.notice = Some(self.tf(
            "⏳ {}. {} — working…",
            &[&stage.index(), &self.t(stage.title())],
        ));
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
        self.poll_export();
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
                    // texture upload — not one per pixel of a 16K world.
                    let image = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    let texture = ctx.load_texture("live_preview", image, TextureOptions::NEAREST);
                    self.live = Some(LivePlate { label, texture });
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
                self.invalidate_plate();
                self.notice = Some(self.tf(
                    "⚠ {}. {} did not finish — the world is as it was before it started",
                    &[&stage.index(), &self.t(stage.title())],
                ));
            }
        }
    }

    /// Adopt a finished act: its world, its figures, and the next act's turn.
    fn finish_stage(&mut self, report: &StageReport) {
        self.world = self.genesis.atlas();
        // Act three, act six and act seven may have written the world cache, so the
        // list of saved worlds can have changed, and the panel reads it again next
        // frame.
        if self.genesis.autosave() {
            self.cached_dirty = true;
        }
        self.relief = self
            .world
            .as_ref()
            .map(|world| measure_relief(&world.heightmap));
        self.invalidate_plate();
        self.live = None;
        self.selected_civ = None;
        self.selected_religion = None;
        self.selected_family = None;
        // What the act made, in one line: its figures are the panel's own report
        // of a finished act, and they belong beside the acts, not in a log.
        let mut line = self.tf(
            "✔ {}. {} — {}s",
            &[
                &report.stage.index(),
                &self.t(report.stage.title()),
                &format!("{:.1}", report.secs),
            ],
        );
        if !report.summary.is_empty() {
            line.push_str(" · ");
            line.push_str(&report.summary.join(" · "));
        }
        self.notice = Some(line);
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
            self.notice = Some(self.tf(
                "🌱 A new world: seed {}. Press “{}” to grow it.",
                &[&seed, &act_label(self.language, Stage::Geology)],
            ));
        } else {
            // The acts already grown stay exactly as they are; only the acts ahead
            // will grow from the new seed.
            self.notice = Some(self.tf(
                "🔀 Seed is now {} — the acts already grown stay; the next act grows from {}.",
                &[&seed, &seed],
            ));
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
    #[cfg(test)]
    fn load_world(&mut self) {
        self.adopt_cache(None);
    }

    /// Adopt one cache the reader picked from the saved-world list.
    fn load_world_from(&mut self, cache: &CachedWorld) {
        self.adopt_cache(Some(cache));
    }

    fn adopt_cache(&mut self, cache: Option<&CachedWorld>) {
        let outcome = match cache {
            Some(cache) => self.genesis.load_cached_world(cache),
            None => self.genesis.load_cache(),
        };
        match outcome {
            Ok(()) => {
                self.world = self.genesis.atlas();
                self.relief = self
                    .world
                    .as_ref()
                    .map(|world| measure_relief(&world.heightmap));
                // A loaded world is past whichever act it was saved at, so the
                // atlas opens on it rather than on a blank plate.
                self.forget_world_view();
                // The cache's own canvas is part of what was adopted: an 8K
                // selector must not keep claiming a 16K world.
                let (width, height) = self.genesis.canvas();
                self.resolution = MapResolution::from_canvas(width, height);
                let stage = self.genesis.stage().unwrap_or(Stage::Geology);
                self.scope = default_scope(stage);
                self.cache_notice = None;
                // The save that was on disk is the one just adopted, and it says
                // how much of the world is already done.
                let (acts, next) = (stage.index(), stage.index() + 1);
                self.notice = Some(self.tf(
                    "✔ Adopted the cached world for seed {} — acts 1–{} are already done; act {} is next",
                    &[&self.genesis.seed(), &acts, &next],
                ));
            }
            Err(LoadError::Missing(path)) => {
                self.cache_notice = Some(self.tf(
                    "No saved world for seed {} — '{}' is not there. Press Save below, or pick a world from Saved worlds below.",
                    &[&self.genesis.seed(), &path.display().to_string()],
                ));
            }
            Err(LoadError::Unreadable(path, why)) => {
                self.cache_notice = Some(self.tf(
                    "'{}' could not be read, so there is nothing to load ({}).",
                    &[&path.display().to_string(), &why],
                ));
            }
        }
    }

    /// Write the world as it stands — every act through the dawn, and the advanced
    /// configuration with it — without freezing the atlas.
    ///
    /// The pipeline is moved onto a worker — every grid is behind an `Arc`, so the
    /// clone copies no pixels — and the result is polled back on a later frame.
    /// The history act is written like every other act: a save made after it holds
    /// all seven, and a load of that save finds nothing left to grow.
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
                self.notice = Some(self.tf(
                    "✔ Saved acts 1–{} for seed {} to '{}'",
                    &[&acts, &seed, &path.display().to_string()],
                ));
                self.cached_dirty = true;
            }
            Err(why) => {
                self.cache_notice =
                    Some(self.tf("The world could not be saved ({}).", &[&format!("{why:?}")]));
            }
        }
    }

    /// Export the map mode on screen as a PNG, without freezing the atlas.
    ///
    /// The world moves to the worker as a clone — every grid is behind an `Arc`,
    /// so that copies pointers, not a planet — together with the handful of
    /// settings that decide how a plate is painted. The worker paints at the
    /// world's own size, downsamples it to the chosen plate and writes it, then
    /// wakes the window so [`MapViewer::poll_export`] can say where it went.
    fn start_export(&mut self, ctx: &egui::Context, choice: ExportChoice) {
        if self.export_job.is_some() {
            return;
        }
        let Some(world) = self.world.as_ref() else {
            return;
        };
        let native = (world.width, world.height);
        if !choice.allowed(native) {
            return;
        }
        let target = choice.size(native);
        let overlay = self.show_civilizations && !self.scope.is_civilizations();
        let name = export_file_name(self.genesis.seed(), self.scope, self.tier, overlay, target);
        let path = free_export_path(&name);
        let world = world.clone();
        let view = self.plate_view();
        let (tx, rx) = mpsc::channel();
        let worker_ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = write_export(&world, view, target, &path);
            let _ = tx.send(ExportOutcome {
                path,
                size: target,
                outcome,
            });
            worker_ctx.request_repaint();
        });
        self.export_job = Some(rx);
        self.export_notice = Some(self.tf("⏳ Exporting {} × {}…", &[&target.0, &target.1]));
    }

    /// Adopt a finished export: the file it wrote, or why it could not.
    fn poll_export(&mut self) {
        let Some(rx) = self.export_job.as_ref() else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.export_job = None;
                self.export_notice = Some(self.t("Export failed — the worker stopped").to_string());
                return;
            }
        };
        self.export_job = None;
        self.export_notice = Some(match result.outcome {
            Ok(()) => self.tf(
                "✔ Exported {} × {} PNG to '{}'",
                &[
                    &result.size.0,
                    &result.size.1,
                    &result.path.display().to_string(),
                ],
            ),
            Err(why) => self.tf("Export failed — {}", &[&why]),
        });
        // A plate invalidated while the export was painting was held back so
        // that two full plates were never painted at once. Now it can be.
        if std::mem::take(&mut self.plate_stale) {
            self.texture = None;
        }
    }

    /// Mark the plate on screen out of date.
    ///
    /// While an export is painting the same world on its worker, the repaint
    /// waits: the old plate stays on screen, and the moment the export finishes
    /// the canvas is refreshed from the current map mode.
    fn invalidate_plate(&mut self) {
        if self.export_job.is_some() {
            self.plate_stale = true;
        } else {
            self.texture = None;
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
    #[cfg(test)]
    fn pick_cached_world(&mut self, seed: u64) {
        self.seed_text = seed.to_string();
        self.genesis.set_seed(seed);
        self.load_world();
    }

    /// Adopt one cache the reader picked from the saved-world list, on the
    /// canvas that cache was written at.
    fn pick_cached_world_from(&mut self, cache: CachedWorld) {
        self.seed_text = cache.seed.to_string();
        self.genesis.set_seed(cache.seed);
        self.load_world_from(&cache);
    }

    /// Throw the world away and start again on the same seed.
    ///
    /// The resolution selector is the authority on the new canvas: this is how a
    /// reader applies a resolution they chose after a world was already grown.
    fn reset_world(&mut self) {
        let seed = self.genesis.seed();
        let autosave = self.genesis.autosave();
        // The world's constants are the reader's, not the world's: they outlive a
        // reset exactly as the cache choice does, so the next world grows under
        // the settings they have chosen.
        let (old_width, old_height) = self.genesis.canvas();
        let mut settings = self.genesis.settings().clone();
        let (width, height) = self.resolution.size();
        // A value still sitting at the old plate's tuned default follows the new
        // plate; anything the reader typed is absolute and stays put.
        settings.adopt_plate_defaults((old_width, old_height), (width, height));
        self.genesis = Genesis::new_sized(seed, width, height);
        self.genesis.set_autosave(autosave);
        self.genesis.set_settings(settings);
        self.world = None;
        self.forget_world_view();
        self.cache_notice = None;
        self.notice = Some(self.tf(
            "↺ Seed {} cleared on a {} × {} canvas — press “{}” to grow it again.",
            &[
                &seed,
                &width,
                &height,
                &act_label(self.language, Stage::Geology),
            ],
        ));
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
        let (width, height) = self.genesis.canvas();
        self.resolution = MapResolution::from_canvas(width, height);
        self.world = self.genesis.atlas();
        self.relief = self
            .world
            .as_ref()
            .map(|world| measure_relief(&world.heightmap));
        self.forget_world_view();
        self.cache_notice = None;
        self.notice = Some(self.tf(
            "↺ Un-done act {} and everything after it — press “{}. {}” to grow it again.",
            &[&stage.index(), &stage.index(), &self.t(stage.title())],
        ));
    }

    /// Forget everything that belonged to the world just replaced.
    fn forget_world_view(&mut self) {
        self.invalidate_plate();
        self.live = None;
        self.selected_civ = None;
        self.selected_religion = None;
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
        painter.rect_filled(rect, 0.0, CHROME_INK);

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
                format!(
                    "🔴 {} — {}",
                    self.t(live.label),
                    stage_progress(self.job.as_ref(), self.language)
                ),
                FontId::proportional(TEXT_BODY),
                Color32::from_rgb(196, 204, 224),
            );
            painter.text(
                egui::pos2(rect.center().x, at.max.y + 40.0),
                Align2::CENTER_CENTER,
                self.tf("Seed {}", &[&self.genesis.seed()]),
                FontId::proportional(TEXT_BODY),
                Color32::from_rgb(150, 164, 190),
            );
            return;
        }

        let title = if self.job.is_some() {
            self.t("Growing a world…")
        } else {
            self.t("No world yet")
        };
        painter.text(
            rect.center() - egui::vec2(0.0, 64.0),
            Align2::CENTER_CENTER,
            title,
            FontId::proportional(TEXT_HEADING + 8.0),
            Color32::from_rgb(214, 224, 242),
        );
        painter.text(
            rect.center() - egui::vec2(0.0, 26.0),
            Align2::CENTER_CENTER,
            self.tf("Seed {}", &[&self.genesis.seed()]),
            FontId::proportional(TEXT_BODY),
            Color32::from_rgb(150, 164, 190),
        );

        // One press is all the first act needs, and it is the only thing a
        // reader can want from an empty world. It is offered on the plate
        // itself, so the first act is never hunted for in the panel.
        if self.job.is_none() {
            let label = self.tf("▶ Begin — {}", &[&act_label(self.language, Stage::Geology)]);
            let button_rect = egui::Rect::from_center_size(
                rect.center() + egui::vec2(0.0, 40.0),
                egui::vec2(360.0, 40.0),
            );
            if ui
                .put(
                    button_rect,
                    egui::Button::new(egui::RichText::new(label).size(TEXT_BODY).strong()),
                )
                .clicked()
            {
                self.start_stage(Stage::Geology, ui.ctx());
            }
        }
    }

    // ─── THE GENESIS PANEL ───────────────────────────────────────────────────
    //
    // Every act of creation is a button here, in the only order they can be run,
    // and the world is grown in front of the reader: a seed, then the world's
    // data, then seven presses — and, folded away underneath, every constant the
    // engine reads. Nothing is generated before it is asked for, and nothing
    // waits to be looked at.
    //
    // Each part is a fold-out of its own, so the panel can be read at the size of
    // the thing the reader is doing: a seed and the acts while a world is being
    // grown, the settings when the planet is being shaped, the saved worlds when
    // one is being hunted for.

    fn genesis_panel(&mut self, ui: &mut egui::Ui, frame: &egui::Frame) {
        let busy = self.job.is_some() || self.save_job.is_some();
        egui::Panel::left("genesis")
            .frame(frame.clone())
            .resizable(true)
            .default_size(GENESIS_PANEL_WIDTH)
            .min_size(320.0)
            .max_size(GENESIS_PANEL_MAX)
            .show(ui, |ui| {
                // The panel can hold a seed, three cache controls, seven acts and
                // thirty constants; on a short window it has to scroll rather
                // than hide the last act behind the bottom edge. Its content is
                // held clear of the floating scroll bar, which is painted over
                // the right edge and would take any press aimed at a control
                // there (see `clear_of_scroll_bar`).
                egui::ScrollArea::vertical()
                    .id_salt("genesis_scroll")
                    .auto_shrink([false, false])
                    .content_margin(clear_of_scroll_bar(ui))
                    .show(ui, |ui| {
                        panel_title(ui, self.t("🧬 Genesis"));
                        ui.separator();
                        self.seed_row(ui, busy);
                        self.resolution_row(ui, busy);
                        ui.add_space(2.0);
                        self.world_data_section(ui, busy);
                        self.act_section(ui, busy);
                        self.settings_section(ui, busy);
                    });
            });
    }

    /// World data: loading and saving are commands, auto-save is a setting, and
    /// the list is what is actually on disk.
    ///
    /// Folded away until it is wanted — a reader growing a world presses the acts,
    /// not the cache — but open it and everything about the save is there.
    fn world_data_section(&mut self, ui: &mut egui::Ui, busy: bool) {
        egui::CollapsingHeader::new(self.t("💾 World data"))
            .id_salt("world_data")
            .default_open(true)
            .show(ui, |ui| {
                self.world_data_rows(ui, busy);
            });
    }

    /// The seven acts, the controls that run them, and the one line about the
    /// last one that ran.
    fn act_section(&mut self, ui: &mut egui::Ui, busy: bool) {
        let grown = Stage::ALL
            .iter()
            .filter(|stage| matches!(self.genesis.state(**stage), StageState::Done { .. }))
            .count();
        let total = Stage::ALL.len();
        egui::CollapsingHeader::new(self.tf("🪜 The acts  ·  {} of {} grown", &[&grown, &total]))
            // The count is in the label, so the id must not be: an act finishing
            // must not snap the list shut.
            .id_salt("acts")
            .default_open(true)
            .show(ui, |ui| {
                self.act_rows(ui, busy);
                ui.separator();
                self.act_controls(ui, busy);
                // What the last act made, or why it did not finish: one line, kept
                // beside the acts it is about.
                if let Some(notice) = self.notice.as_deref() {
                    ui.add_space(2.0);
                    status_line(ui, notice);
                }
            });
    }

    /// Advanced configuration: every constant of the engine, gathered under the
    /// seven acts that read it. The section does its own drawing and its own
    /// resets; all this has to do is hand the pipeline whatever the reader left
    /// behind.
    fn settings_section(&mut self, ui: &mut egui::Ui, busy: bool) {
        let mut settings = self.genesis.settings().clone();
        // The ↺ buttons restore the tuned density of the plate this world is on.
        let (width, height) = self.genesis.canvas();
        let defaults = WorldGenSettings::for_canvas(width, height);
        if settings::advanced_settings(ui, &mut settings, &defaults, busy, self.language) {
            self.genesis.set_settings(settings);
        }
    }

    /// The seed: typed, drawn at random, and never editable while a world is
    /// being grown from it.
    fn seed_row(&mut self, ui: &mut egui::Ui, busy: bool) {
        ui.horizontal(|ui| {
            ui.label(self.t("Seed"));
            let typed = ui
                .add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.seed_text).desired_width(140.0),
                )
                .changed();
            if ui
                .add_enabled(!busy, egui::Button::new("🎲"))
                .on_hover_text(self.t("Draw a seed the world has never seen"))
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
            caption(
                ui,
                self.t("A seed is a whole number — try 42, or press 🎲."),
            );
        }
    }

    /// The canvas the next world will grow on.
    ///
    /// Before act one the choice is applied at once. Once a world exists the
    /// canvas is part of that world's identity, so the selector may still be
    /// changed but it only takes effect through an explicit reset — which the
    /// row says, and offers as a button.
    fn resolution_row(&mut self, ui: &mut egui::Ui, busy: bool) {
        let before = self.resolution;
        let has_world = self.genesis.stage().is_some() || self.world.is_some();

        ui.horizontal(|ui| {
            ui.label(self.t("Map resolution"));
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_id_salt("map_resolution")
                    .selected_text(self.resolution.label())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for preset in MapResolution::ALL {
                            ui.selectable_value(&mut self.resolution, preset, preset.label());
                        }
                        if let MapResolution::Custom(width, height) = self.resolution {
                            ui.selectable_value(
                                &mut self.resolution,
                                MapResolution::Custom(width, height),
                                MapResolution::Custom(width, height).label(),
                            );
                        }
                    });
            });
        });

        if self.resolution != before && !busy && !has_world {
            self.apply_chosen_resolution();
        }

        let (width, height) = self.genesis.canvas();
        if has_world && self.resolution.size() != (width, height) {
            caption(
                ui,
                self.t("Changing the resolution resets this world — press ↺ Reset world to apply."),
            );
            if ui
                .add_enabled(!busy, egui::Button::new(self.t("↺ Reset to apply")).small())
                .clicked()
            {
                self.reset_world();
            }
        } else {
            caption(
                ui,
                self.tf(
                    "Canvas {} × {} · resolution is fixed once act 1 runs.",
                    &[&width, &height],
                ),
            );
        }
        caption(
            ui,
            self.t("Erosion and lake budgets scale with the canvas; province counts are absolute, and the ↺ defaults follow the plate."),
        );
    }

    /// Put the chosen resolution into a world that has not begun yet.
    ///
    /// Returns `false` when a world already exists: its canvas is part of its
    /// identity, and only [`MapViewer::reset_world`] may change it.
    fn apply_chosen_resolution(&mut self) -> bool {
        if self.genesis.stage().is_some() || self.world.is_some() {
            return false;
        }
        let (width, height) = self.resolution.size();
        if !self.genesis.set_canvas(width, height) {
            return false;
        }
        self.invalidate_plate();
        self.pending_fit = true;
        self.notice = Some(self.tf(
            "🌱 Canvas {} × {} — press “{}” to grow it.",
            &[&width, &height, &act_label(self.language, Stage::Geology)],
        ));
        true
    }

    /// World data: loading and saving are commands, auto-save is a setting, and
    /// the list is what is actually on disk.
    ///
    /// Load and Save are buttons that do their one thing at once — Load adopts
    /// the seed's saved world, Save writes the world as it stands. The checkbox
    /// below them arms the automatic saves at act 3, act 6 and act 7, and every
    /// cache on disk is listed at the bottom.
    fn world_data_rows(&mut self, ui: &mut egui::Ui, busy: bool) {
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
        // The list has already been scanned, so this costs no filesystem call per
        // frame. Prefer the cache on the canvas the selector is showing; if the
        // seed only exists on another canvas, that cache is still loadable and
        // will move the selector to its own resolution.
        let cache = typed.and_then(|seed| {
            let (width, height) = self.resolution.size();
            self.cached
                .iter()
                .filter(|cache| cache.seed == seed)
                .find(|cache| (cache.width, cache.height) == (width, height))
                .or_else(|| self.cached.iter().find(|cache| cache.seed == seed))
                .cloned()
        });
        let load_hint = if busy {
            self.t("Wait for the act in flight to finish")
        } else if typed.is_none() {
            self.t("A seed is a whole number — Load needs one")
        } else {
            self.t("There is no cache for this seed yet")
        };

        // A save holds every act grown so far, all seven once the dawn is on the
        // world, so it is offered from the moment act one lands and stays offered
        // to the end.
        let saveable = can_save(self.genesis.stage());
        let save_hint = if busy {
            self.t("Wait for the act in flight to finish")
        } else {
            self.t("Grow the world first — there is nothing to save yet")
        };

        ui.horizontal(|ui| {
            let load = ui
                .add_enabled(
                    cache.is_some() && !busy,
                    egui::Button::new(self.t("📂 Load")),
                )
                .on_hover_text(match &cache {
                    Some(cache) => self.tf(
                        "Read {} and pick the panel up where it left off",
                        &[&cache.path.display().to_string()],
                    ),
                    None => self.t("Read this seed's saved world").to_string(),
                })
                .on_disabled_hover_text(load_hint);
            if load.clicked()
                && let (Some(seed), Some(cache)) = (typed, cache.clone())
            {
                self.genesis.set_seed(seed);
                self.load_world_from(&cache);
            }

            let save = ui
                .add_enabled(
                    saveable && !busy,
                    egui::Button::new(self.t("💾 Save")),
                )
                .on_hover_text(self.t(
                    "Write every act grown so far, up to act 7 — the advanced configuration included — to saved_seeds/world_<seed>_<width>x<height>.bin",
                ))
                .on_disabled_hover_text(save_hint);
            if save.clicked() {
                self.save_world();
            }
        });

        match (&cache, typed) {
            (Some(cache), _) => caption(
                ui,
                self.tf(
                    "seed {} · {} × {} · {} · saved {}",
                    &[
                        &cache.seed,
                        &cache.width,
                        &cache.height,
                        &cache_size(cache.bytes, self.language),
                        &cache_age(cache.modified, self.language),
                    ],
                ),
            ),
            (None, Some(seed)) => caption(ui, self.tf("No saved world for seed {}.", &[&seed])),
            (None, None) => caption(ui, self.t("A seed is a whole number — Load needs one.")),
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

    /// The save setting: whether act 3, act 6 and act 7 write the world cache.
    fn save_row(&mut self, ui: &mut egui::Ui, busy: bool) {
        let mut autosave = self.genesis.autosave();
        if ui
            .add_enabled(
                !busy,
                egui::Checkbox::new(
                    &mut autosave,
                    self.t("💾 Save automatically when act 3, act 6 and act 7 land"),
                ),
            )
            .on_hover_text(self.t(
                "Write every act grown so far, up to act 7 — the advanced configuration included — to saved_seeds/world_<seed>_<width>x<height>.bin",
            ))
            .changed()
        {
            self.genesis.set_autosave(autosave);
            self.cache_notice = None;
        }
    }

    /// Every world cache on disk, newest first, as a list to pick from.
    ///
    /// A click is a load: the seed field follows the pick and the cache is
    /// adopted at once. The list refreshes by itself after any act that may
    /// have written a cache, and on demand with **🔄 Rescan**.
    fn saved_worlds_list(&mut self, ui: &mut egui::Ui, busy: bool) {
        let mut refresh = false;
        let mut picked = None;
        let current = self.genesis.seed();

        let response =
            egui::CollapsingHeader::new(self.tf("📂 Saved worlds ({})", &[&self.cached.len()]))
                // The count is in the label, so the id must not be: otherwise a
                // newly saved world would change the id and snap the list shut.
                .id_salt("saved_worlds")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(self.t("🔄 Rescan"))
                            .on_hover_text(self.t("Read saved_seeds again"))
                            .clicked()
                        {
                            refresh = true;
                        }
                    });

                    if self.cached.is_empty() {
                        caption(ui, self.t("No saved worlds."));
                        return;
                    }

                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .auto_shrink([false, true])
                        // A row is a full-width button, and the list's own bar
                        // floats over its right end: without this margin a press
                        // there would page the list instead of loading the world.
                        .content_margin(clear_of_scroll_bar(ui))
                        .show(ui, |ui| {
                            for cache in &self.cached {
                                // A save holds whichever acts the world had grown
                                // when it was written, and its header names the
                                // deepest of them, so the row says `acts 1–5`.
                                let acts = acts_saved(cache.acts, self.language);
                                let size = cache_size(cache.bytes, self.language);
                                let age = cache_age(cache.modified, self.language);
                                let resolution =
                                    MapResolution::from_canvas(cache.width, cache.height).label();
                                let line = self.tf(
                                    "{} · {}   {} · {}  · {}",
                                    &[&cache.seed, &resolution, &size, &age, &acts],
                                );
                                let hover = self.tf(
                                    "Load seed {} on {} — {} · {} · {}\nPicks the panel up where the save left off.",
                                    &[&cache.seed, &resolution, &size, &age, &acts],
                                );
                                let selected = cache.seed == current
                                    && (cache.width, cache.height) == self.genesis.canvas();
                                let button = egui::Button::selectable(selected, line).truncate();
                                if ui
                                    .add_enabled(!busy, button)
                                    .on_hover_text(hover)
                                    .on_disabled_hover_text(
                                        self.t("Wait for the act in flight to finish"),
                                    )
                                    .clicked()
                                {
                                    picked = Some(cache.clone());
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
        if let Some(cache) = picked {
            self.pick_cached_world_from(cache);
        }
    }

    /// The seven acts, in order: the one that can be run next is the only button
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
                    "🔄"
                } else {
                    match state {
                        StageState::Done { .. } => "✔",
                        StageState::Ready => "▶",
                        StageState::Locked => "🔒",
                    }
                };
                let label = format!("{mark} {}", act_label(self.language, stage));
                let button = egui::Button::new(label).frame(false);
                if ui
                    .add_enabled(enabled, button)
                    .on_hover_text(self.t(stage.blurb()))
                    .clicked()
                {
                    pressed = Some(stage);
                }

                match state {
                    StageState::Done { secs } => {
                        status_line(
                            ui,
                            if secs > 0.0 {
                                self.tf("· {}s", &[&format!("{:.1}", secs)])
                            } else {
                                // A stage adopted from a cache was not timed: it
                                // happened in another session.
                                self.t("· loaded").to_string()
                            },
                        );
                        // A finished act can be un-done: this drops it and every
                        // act after it, so they may be grown again — optionally
                        // from a new seed typed into the field above.
                        if ui
                            .add_enabled(!busy, egui::Button::new("↺").frame(false))
                            .on_hover_text(self.tf(
                                "Redo from act {} — un-do it and everything after it",
                                &[&stage.index()],
                            ))
                            .on_disabled_hover_text(self.t("Wait for the act in flight to finish"))
                            .clicked()
                        {
                            rewind = Some(stage);
                        }
                    }
                    StageState::Ready if !busy => {
                        status_line(ui, self.t("· next"));
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
            self.tf(
                "{} · {} s",
                &[
                    &stage_progress(self.job.as_ref(), self.language),
                    &format!("{elapsed:.0}"),
                ],
            ),
        );

        // A picture of the act, when it has one to show: the live plate is
        // painted from the very grid the act is working on.
        if let Some(live) = &self.live {
            caption(
                ui,
                self.tf(
                    "🔴 live: {} · {}",
                    &[&live.label, &self.t(job.stage.title())],
                ),
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
                    egui::Button::new(self.t("▶▶ Run all remaining")),
                )
                .on_hover_text(self.t("Grow the whole world, act after act, without stopping"))
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
                        .button(self.t("⏹ Stop after this act"))
                        .on_hover_text(self.t("The queue stops here; this act finishes"))
                        .clicked()
                    {
                        self.run_queue = false;
                    }
                }
            } else if ui
                .add_enabled(
                    self.world.is_some(),
                    egui::Button::new(self.t("↺ Reset world")),
                )
                .on_hover_text(self.t("Throw this world away and begin again on the same seed"))
                .clicked()
            {
                self.reset_world();
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
        // The icon faces are part of the style in everything but name: they are
        // what the pictograms and marks in these sizes, panels and cards are
        // drawn with, and `add_font` is idempotent, so this installs them once.
        install_icon_fonts(&ctx);
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
        let (width, height) = self.world.as_ref().map_or_else(
            || self.genesis.canvas(),
            |world| (world.width, world.height),
        );
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
        let img = Self::generate_texture(world, self.plate_view());
        let full = rgba_plate(&img);
        let plate = if (scale - 1.0).abs() < f32::EPSILON {
            full
        } else {
            let out_w = ((full.width() as f32 * scale).round() as u32).max(1);
            let out_h = ((full.height() as f32 * scale).round() as u32).max(1);
            image::imageops::resize(&full, out_w, out_h, image::imageops::FilterType::Lanczos3)
        };
        plate.save(path)
    }

    /// The painter's settings, copied out of the window so an export can paint
    /// the same plate without borrowing the atlas.
    fn plate_view(&self) -> PlateView {
        let hydro = self.genesis.hydro_config();
        PlateView {
            scope: self.scope,
            tier: self.tier,
            show_borders: self.show_borders,
            show_civilizations: self.show_civilizations,
            min_river_flow: hydro.min_river_flow,
            major_river_flow: hydro.major_river_flow,
        }
    }

    /// Paint a plate of `world` as `view` describes it.
    ///
    /// This is the atlas's one painter: the interactive canvas, the headless
    /// plates and the PNG export all call it, so none of them can disagree about
    /// what a map mode looks like.
    fn generate_texture(world: &AtlasWorld, view: PlateView) -> ColorImage {
        let w = world.width;
        let h = world.height;
        let scope = view.scope;
        let tier = view.tier;
        let show_borders = view.show_borders;
        let hierarchy = &world.hierarchy;
        let ocean_hierarchy = &world.ocean_hierarchy;
        let province_ids = world.provinces.as_ref().map(|p| &p.province_ids);
        let ocean_province_ids = world.ocean.as_ref().map(|o| &o.ocean_province_ids);
        let is_wasteland = world.provinces.as_ref().map(|p| &p.is_wasteland);

        let land_political = matches!(scope, MapScope::Land | MapScope::Everywhere);
        let water_political = matches!(scope, MapScope::Water | MapScope::Everywhere);
        let civ_scope = scope.is_civilizations();
        let religion_scope = scope.is_religions();
        let language_scope = scope.is_languages();
        // The civilizations' own scope always shows them; every other scope shows
        // them only when the reader asks for the overlay, and the scopes that
        // portray a people by another name never take it.
        let show_civs = (view.show_civilizations && scope.accepts_civ_overlay()) || civ_scope;
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
        // The river thresholds are the pipeline's own, not a private copy: the
        // live plate and the printed plates must agree on what counts as a river.
        // They travel in `PlateView`, so an export paints rivers exactly as the
        // canvas does.
        let min_river_flow = view.min_river_flow;
        let major_river_flow = view.major_river_flow;
        let lx = -0.7071f32;
        let ly = -0.7071f32;
        let render_noise = OpenSimplex::new(777);

        let mut pixels = vec![0u8; w * h * 4];

        // The palette is a per-tier lookup table, built once for the whole plate.
        // Regions and Areas are shaded as families of their parent (see
        // `territory_palette`), so the pixel loop is a table read rather than 33
        // million hue computations.
        let land_palette = territory_palette(hierarchy, tier);
        let ocean_palette = territory_palette(ocean_hierarchy, tier);

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
                    // Lake first: a landlocked basin can hold its surface below the
                    // datum, and it must read as fresh water rather than as sea.
                    if lake_here {
                        [42.0, 125.0, 185.0]
                    } else if elev <= sea_level {
                        if display_temp < ICE_SHELF_TEMP {
                            [245.0, 248.0, 255.0] // Glacial ice shelf
                        } else if display_temp < PACK_ICE_TEMP {
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

                    if lake_here {
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
                        if display_temp < GLACIAL_LAND_TEMP {
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
                } else if civ_scope || religion_scope {
                    // ── Civilizations and Religions scopes ─────────────────
                    // The land is the subject, so the plate is built the way a political
                    // atlas is printed: a small ordered palette of flat fills, ink on the
                    // frontiers and the coast, the sea pushed back to a pale slate, and
                    // the relief carried *inside* each fill rather than mixed into it, so
                    // a country keeps one hue from its coast to its mountains. The two
                    // modes differ only in whose palette fills the ground: each realm's
                    // own colour, or the shade its faith wears under its root.
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
                        let civ_rgb = if religion_scope {
                            civilizations.religion_color_of_province(id)
                        } else {
                            civilizations.color_of(owner)
                        };
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

                        // A landmark dot with a pale halo, so it reads on a
                        // hunter-gatherer khaki as well as on an oxblood state: the
                        // cradle on the Civilizations map, and that faith's primary
                        // holy place on the Religions map, where the holiest city is
                        // the thing worth finding.
                        let marker = if religion_scope {
                            civilizations
                                .religion_of_province(id)
                                .and_then(|religion| religion.faith.sacred_sites.first())
                                .and_then(|site| hierarchy.provinces.get(site.province as usize))
                                .map(|node| (node.center_x, node.center_y))
                        } else {
                            civilizations
                                .civilization(owner)
                                .map(|civ| (civ.seat_x, civ.seat_y))
                        };
                        if let Some((marker_x, marker_y)) = marker {
                            let dx = {
                                let mut d = (x as f32 - marker_x).abs();
                                let width = w as f32;
                                if d > width * 0.5 {
                                    d = width - d;
                                }
                                d
                            };
                            let dy = (y as f32 - marker_y).abs();
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
                            // The Religions map draws faith frontiers, not political ones: its
                            // subject is where one creed ends. The Civilizations map draws
                            // realm frontiers and, when the tier selector asks, the
                            // subdivisions inside them.
                            if religion_scope {
                                final_color = religion_border_shade(
                                    final_color,
                                    province_ids,
                                    civilizations,
                                    id,
                                    x,
                                    y,
                                    h,
                                );
                            } else {
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
                                    !religion_scope,
                                );
                            }
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
                        let territory_rgb = ocean_palette
                            .get(ocean_cid as usize)
                            .copied()
                            .unwrap_or([0.0, 0.0, 0.0]);
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
                        let territory_rgb = land_palette
                            .get(color_id as usize)
                            .copied()
                            .unwrap_or([0.0, 0.0, 0.0]);
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

        // The chrome comes first and is always there: the top bar spans the whole
        // window — before a world exists as well as after — so the language can
        // be chosen, the scopes can be read, and the world can be watched without
        // the frame around it ever going missing.
        let panel_frame = atlas_panel_frame(ui.style());
        self.top_bar(ui, &panel_frame);
        // The census is a footer, not a section of the map: it spans the window
        // like the top bar does, and the genesis panel sits between the two.
        self.bottom_census(ui, &panel_frame);
        if self.show_genesis_panel {
            self.genesis_panel(ui, &panel_frame);
        }

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

    /// The world's age in one chip's worth of words: the act in flight, the
    /// world's absence, the finished world, or how far it has climbed.
    ///
    /// One decision, made once per frame, rather than a condition repeated in
    /// the bar: the same sentence can then be read by a test as by the reader.
    fn world_status(&self) -> String {
        if let Some(job) = self.job.as_ref() {
            // An act in flight is named, not merely counted: the bar says what
            // is being grown, and how far into it the pipeline has got.
            return self.tf("⏳ {}", &[&stage_progress(Some(job), self.language)]);
        }
        // The acts are a strict chain, so the deepest act the pipeline holds is
        // also the number of acts grown — whether the world was grown here or
        // adopted from a cache with `--load`.
        let grown = self.genesis.stage().map_or(0, |stage| stage.index());
        let total = Stage::ALL.len();
        if grown == 0 {
            self.t("🌱 No world yet").to_string()
        } else if grown >= total {
            self.t("✔ World complete").to_string()
        } else {
            self.tf("🌍 {} of {} acts grown", &[&grown, &total])
        }
    }

    /// The atlas's chrome: who it is, which world it is showing, what the map
    /// is portraying, and the controls that dress it.
    ///
    /// Two rows, always drawn, whatever the world's age or the scope on screen.
    /// A control is disabled when it cannot apply — not hidden — so the atlas
    /// never changes shape under the reader's hand and nothing it offers is a
    /// surprise.
    ///
    /// The bar is built to keep its height constant. Every item in it is atomic
    /// — a chip, a menu button, a checkbox — so a long seed or a long language
    /// name makes a row wider, never taller; and the widest group in it, the
    /// eleven scopes, is a menu rather than eleven buttons. A bar whose height
    /// grew with its contents would move the map under the reader's cursor,
    /// which is exactly what a crowded bar used to do.
    fn top_bar(&mut self, ui: &mut egui::Ui, frame: &egui::Frame) {
        let stage = self.world.as_ref().map(|world| world.stage);
        let has_world = self.world.is_some();
        let has_civs = self
            .world
            .as_ref()
            .is_some_and(|world| !world.civilizations.is_empty());
        let civ_count = self
            .world
            .as_ref()
            .map_or(0, |world| world.civilizations.civilizations.len());
        let seed = self.genesis.seed();
        let native = self.world.as_ref().map(|world| (world.width, world.height));
        // A click in the Export menu is resolved after the bar has been laid out,
        // so the menu's own borrow of the atlas has ended by the time the worker
        // is handed the world.
        let mut export_choice = None;
        let ctx = ui.ctx().clone();
        // The chip's words are read off the pipeline before the bar is laid out:
        // one status, computed once, rather than a condition repeated in a row.
        let status = self.world_status();
        // A scope is chosen in the menu and applied after the bar has closed, so
        // the menu's borrow of the atlas has ended and the plate is rebuilt once.
        let mut chosen_scope = self.scope;

        egui::Panel::top("top_bar")
            .frame(frame.clone())
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    panel_title(ui, self.t("🌍 Atlas Viewer"));
                    ui.separator();

                    // ── The seed and the world's status ───────────────────────
                    // Two chips, and both are atomic: neither the seed nor the
                    // status can wrap. Clicking the seed copies it, so a figure
                    // the reader wants to keep is one press away.
                    let seed_chip = chip(ui, self.tf("Seed {}", &[&seed]));
                    let seed_chip = seed_chip
                        .interact(egui::Sense::click())
                        .on_hover_text(self.t("Click to copy"));
                    if seed_chip.clicked() {
                        ctx.copy_text(seed.to_string());
                    }
                    chip(ui, &status);
                    ui.separator();

                    // ── Map scope: what the atlas portrays ────────────────────
                    // A scope is offered exactly when the world is old enough to
                    // have what it portrays: relief needs only the land, biomes
                    // and climate need the atmosphere, the provinces and the two
                    // hierarchies each need their own act. Before a world exists
                    // every scope is disabled, and the menu says why.
                    //
                    // The active scope stays on the bar, so the reader never has
                    // to open the menu to know where they are; the menu holds the
                    // eleven choices and the hairline between the physical
                    // survey and the world's people.
                    let active_scope = self.t(self.scope.label());
                    let scope_menu = ui.menu_button(active_scope, |ui| {
                        for (index, scope) in MapScope::ALL.into_iter().enumerate() {
                            if index == PHYSICAL_SCOPES {
                                ui.separator();
                            }
                            let ready = stage.is_some_and(|stage| scope.available(stage));
                            let label = self.t(scope.label());
                            let response = ui
                                .add_enabled_ui(ready, |ui| {
                                    ui.selectable_value(&mut chosen_scope, scope, label)
                                })
                                .inner;
                            if !ready {
                                let hint = match stage {
                                    None => self.t("Grow a world first").to_string(),
                                    Some(_) => {
                                        let need = scope.min_stage();
                                        self.tf(
                                            "Needs “{}. {}” — {}.",
                                            &[
                                                &need.index(),
                                                &self.t(need.title()),
                                                &self.t(need.blurb()),
                                            ],
                                        )
                                    }
                                };
                                response.on_disabled_hover_text(hint);
                            } else if response.clicked() {
                                ui.close();
                            }
                        }
                    });
                    scope_menu.response.on_hover_text(self.t("Map scope"));

                    // ── The right-hand commands ───────────────────────────────
                    // Laid out from the right edge, so the three controls that
                    // act on the atlas itself — the panel toggle, the export and
                    // the language — keep their places whatever the seed's
                    // length does to the left half of the row. In a right-to-left
                    // layout the first control added is the rightmost one.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // ☰ folds the genesis panel away: the window's own
                        // furniture, in the corner the eye expects it.
                        if ui
                            .button("☰")
                            .on_hover_text(self.t("Show/hide the Genesis panel"))
                            .clicked()
                        {
                            self.show_genesis_panel = !self.show_genesis_panel;
                        }
                        ui.separator();

                        // ── The export ────────────────────────────────────────
                        // The map mode on screen, printed as a PNG next to the
                        // language: a 2K or 8K plate, or the world's own size when
                        // it is neither. Nothing is ever enlarged — an export only
                        // ever downsamples — so a size the world is too small for
                        // stays listed but disabled, with the reason on hover.
                        let export_menu = ui
                            .add_enabled_ui(has_world && self.export_job.is_none(), |ui| {
                                ui.menu_button(self.t("💾 Export"), |ui| {
                                    let Some(native) = native else {
                                        return;
                                    };
                                    for choice in ExportChoice::ALL
                                        .into_iter()
                                        .filter(|choice| choice.offered(native))
                                    {
                                        let response = ui
                                            .add_enabled(
                                                choice.allowed(native),
                                                egui::Button::new(
                                                    choice.label(native, self.language),
                                                ),
                                            )
                                            .on_disabled_hover_text(self.tf(
                                                "This world is {} × {} — choose a size no larger than that",
                                                &[&native.0, &native.1],
                                            ));
                                        if response.clicked() {
                                            ui.close();
                                            export_choice = Some(choice);
                                        }
                                    }
                                })
                            })
                            .inner
                            .response;
                        let _ = if !has_world {
                            export_menu.on_disabled_hover_text(self.t("Grow a world first"))
                        } else if self.export_job.is_some() {
                            export_menu
                                .on_disabled_hover_text(self.t("An export is already running"))
                        } else {
                            export_menu.on_hover_text(self.t("Export the current map as a PNG"))
                        };
                        ui.separator();

                        // ── The language ──────────────────────────────────────
                        // A property of the reader, not of the world: it is
                        // offered before the first act as well as after the
                        // last, and no act can ever disable it.
                        if let Some(chosen) = i18n::language_picker(ui, self.language)
                            && chosen != self.language
                        {
                            self.language = chosen;
                            chosen.save_preference(std::path::Path::new(
                                crate::pipeline::CACHE_DIR,
                            ));
                        }
                    });
                });

                ui.horizontal_wrapped(|ui| {
                    // ── Tier: which rung of the hierarchy is shaded ───────────
                    // The political tiers when a political scope is on, and the
                    // internal subdivision of the realms in the Civilizations
                    // scope (a realm's own provinces, areas, regions…). A picker
                    // rather than five buttons: the active tier stays readable and
                    // the row stays short, which is what keeps the bar two rows
                    // tall at every window size.
                    let tier_ready = has_world
                        && (self.scope.is_political() || self.scope.is_civilizations());
                    let tier_heading = self.t("Tier:");
                    let mut chosen_tier = self.tier;
                    ui.add_enabled_ui(tier_ready, |ui| {
                        ui.label(tier_heading);
                        egui::ComboBox::from_id_salt("map_tier")
                            .selected_text(self.t(self.tier.label()))
                            .width(170.0)
                            .show_ui(ui, |ui| {
                                for tier in MapTier::ALL {
                                    ui.selectable_value(
                                        &mut chosen_tier,
                                        tier,
                                        self.t(tier.label()),
                                    );
                                }
                            });
                    });
                    if chosen_tier != self.tier {
                        self.tier = chosen_tier;
                        self.invalidate_plate();
                    }

                    ui.separator();

                    // Border Toggle (in political and civilization scopes)
                    let borders_label = self.t("Borders");
                    let borders_ready = has_world
                        && (self.scope.is_political()
                            || self.scope.is_civilizations()
                            || self.scope.is_religions());
                    if ui
                        .add_enabled_ui(borders_ready, |ui| {
                            ui.checkbox(&mut self.show_borders, borders_label)
                        })
                        .inner
                        .changed()
                    {
                        self.invalidate_plate();
                    }
                    ui.separator();

                    // ── Civilizations overlay ─────────────────────────────────
                    // The dawn is viewable on the physical atlas, the survey sheet
                    // and both political hierarchies, not only on its own map mode.
                    // It is on none of the scopes that portray people by another
                    // name, so there it is shown disabled rather than taken away.
                    let civ_label = self.tf("🏺 Civilizations ({})", &[&civ_count]);
                    let overlay_ready = has_civs && self.scope.accepts_civ_overlay();
                    let mut response = ui
                        .add_enabled_ui(overlay_ready, |ui| {
                            ui.checkbox(&mut self.show_civilizations, civ_label)
                        })
                        .inner;
                    if !has_civs {
                        response = response.on_disabled_hover_text(self.t("No civilizations yet"));
                    } else if !overlay_ready {
                        response =
                            response.on_disabled_hover_text(self.t("Not shown in this scope"));
                    }
                    if response.changed() {
                        self.invalidate_plate();
                    }
                });
            });

        // A scope chosen in the menu is applied once the bar has closed: the
        // menu's borrow of the atlas has ended, and the plate is rebuilt once.
        // The reader's zoom and pan are kept — a map mode is a change of what is
        // shown, not of where the reader is looking; **Fit** on the canvas is
        // there when the whole world is wanted again.
        if chosen_scope != self.scope {
            self.scope = chosen_scope;
            self.invalidate_plate();
        }

        if let Some(choice) = export_choice {
            self.start_export(&ctx, choice);
        }
    }

    /// The world's census, in the three lines it has always been: the land
    /// hierarchy, the ocean hierarchy and the dawn. A hierarchy that has not been
    /// built yet is not printed as a row of zeroes — it is simply not there to
    /// count.
    ///
    /// The footer is also where the export's notice lands. It belongs to the
    /// world's own figures rather than to the bar: an export's output path is a
    /// sentence, and a sentence in the bar would widen the row that is supposed
    /// to keep its shape. The footer may grow a line when a notice arrives; the
    /// bar never does.
    fn bottom_census(&self, ui: &mut egui::Ui, frame: &egui::Frame) {
        let notice = self.export_notice.as_deref();
        let census = self.world.as_ref().filter(|world| {
            world.hierarchy.total_provinces > 0
                || world.ocean_hierarchy.total_provinces > 0
                || !world.civilizations.is_empty()
        });
        if census.is_none() && notice.is_none() {
            return;
        }

        egui::Panel::bottom("bottom_census")
            .frame(frame.clone())
            .show(ui, |ui| {
                let Some(world) = census else {
                    if let Some(notice) = notice {
                        status_line(ui, notice);
                    }
                    return;
                };
                if world.hierarchy.total_provinces > 0 {
                    status_line(
                        ui,
                        self.tf(
                            "🏛 LAND — Continents: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}",
                            &[
                                &world.hierarchy.total_continents,
                                &world.hierarchy.total_superregions,
                                &world.hierarchy.total_regions,
                                &world.hierarchy.total_areas,
                                &world.hierarchy.total_provinces,
                            ],
                        ),
                    );
                }
                if world.ocean_hierarchy.total_provinces > 0 {
                    status_line(
                        ui,
                        self.tf(
                            "🌊 OCEAN — Basins: {} · Superregions: {} · Regions: {} · Areas: {} · Provinces: {}",
                            &[
                                &world.ocean_hierarchy.total_continents,
                                &world.ocean_hierarchy.total_superregions,
                                &world.ocean_hierarchy.total_regions,
                                &world.ocean_hierarchy.total_areas,
                                &world.ocean_hierarchy.total_provinces,
                            ],
                        ),
                    );
                }
                if !world.civilizations.is_empty() {
                    let dawn = &world.civilizations;
                    status_line(
                        ui,
                        self.tf(
                            "🏺 CIVILIZATIONS — Atlas date: {} · {} · {} realms · {} civilized provinces · population {} · {} people/km²",
                            &[
                                &dawn.map_year_label(),
                                &dawn.age_label(),
                                &dawn.civilizations.len(),
                                &dawn.civilized_provinces,
                                &thousands_u64(dawn.total_population),
                                &format!("{:.1}", dawn.mean_density_per_km2()),
                            ],
                        ),
                    );
                }
                // What the last export said, when it has something to say: the
                // path it wrote, or why it could not write. It sits under the
                // census, in the same plain figures, because it is a fact about
                // the world rather than a command on the bar.
                if let Some(notice) = notice {
                    status_line(ui, notice);
                }
            });
    }

    /// Where the zoom pill sits, given the canvas rectangle: the bottom-right
    /// corner, floating a margin in from the edge.
    fn zoom_pill_rect(canvas: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_size(
            canvas.max
                - egui::vec2(
                    ZOOM_PILL_MARGIN + ZOOM_PILL_WIDTH,
                    ZOOM_PILL_MARGIN + ZOOM_PILL_HEIGHT,
                ),
            egui::vec2(ZOOM_PILL_WIDTH, ZOOM_PILL_HEIGHT),
        )
    }

    /// Draw the zoom pill over the canvas and apply its presses.
    ///
    /// The camera belongs to the map, so its controls sit on the map: zoom out,
    /// the current percentage, zoom in, and **Fit**, floating in the corner the
    /// reader's eye returns to. **Fit** measures the canvas and shows the whole
    /// planet; it is applied after the panels settle, in the canvas's own frame.
    ///
    /// Returns true when the pointer is over the pill, so the canvas can leave
    /// the press, the drag and the scroll to the controls sitting on top of it.
    fn zoom_pill(&mut self, ui: &mut egui::Ui, canvas: egui::Rect) -> bool {
        let pill = Self::zoom_pill_rect(canvas);
        // A canvas too small for the pill shows no pill rather than a clipped
        // one: at the minimum window there is always room, but a legend can
        // still squeeze the map, and a half-drawn control is worse than none.
        if !canvas.contains_rect(pill) {
            return false;
        }

        let painter = ui.painter().with_clip_rect(canvas);
        painter.rect_filled(pill, egui::CornerRadius::same(6), CHROME_PANEL_VEIL);
        painter.rect_stroke(
            pill,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, CHROME_EDGE),
            egui::StrokeKind::Inside,
        );

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(pill.shrink2(egui::vec2(8.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                if ui.button("➖").on_hover_text(self.t("Zoom out")).clicked() {
                    self.zoom = (self.zoom * 0.75).clamp(MIN_ZOOM, MAX_ZOOM);
                }
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.button("➕").on_hover_text(self.t("Zoom in")).clicked() {
                    self.zoom = (self.zoom * 1.33).clamp(MIN_ZOOM, MAX_ZOOM);
                }
                if ui
                    .button(self.t("⤢ Fit"))
                    .on_hover_text(self.t("Fit the whole world in the window"))
                    .clicked()
                {
                    self.pending_fit = true;
                }
            },
        );

        ui.rect_contains_pointer(pill)
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

        // Legend clicks cannot move the map directly: the canvas rectangle for
        // this frame is only known once the panels have been laid out. Requests
        // are collected here and applied right after the canvas is allocated.
        let mut focus_request: Option<(f32, f32, f32)> = None;
        let mut clicked_civ: Option<u32> = None;
        let mut clicked_religion: Option<u16> = None;
        let mut clicked_family: Option<u16> = None;

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
            let peak_where = lat_lon_label(peak_lat, peak_lon, self.language);
            let trench_where = lat_lon_label(trench_lat, trench_lon, self.language);

            egui::Panel::right("hypsometric_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, self.t("⛰ Topographic Scale"));
                    ui.separator();

                    let peak_label = self.tf(
                        "🗻 Highest peak   +{} m",
                        &[&format!("{:.0}", relief.peak_metres)],
                    );
                    if ui
                        .add(egui::Button::new(peak_label).frame(false))
                        .on_hover_text(self.tf("{}\nClick to fly to the summit", &[&peak_where]))
                        .clicked()
                    {
                        // Never zoom *out* to fly somewhere: a reader who is already
                        // closer than the default keeps their scale.
                        let zoom = RELIEF_FOCUS_ZOOM.max(self.zoom);
                        focus_request = Some((relief.peak_x, relief.peak_y, zoom));
                    }
                    caption(ui, peak_where);

                    let trench_label = self.tf(
                        "🌊 Deepest trench  {} m",
                        &[&format!("{:.0}", relief.trench_metres)],
                    );
                    if ui
                        .add(egui::Button::new(trench_label).frame(false))
                        .on_hover_text(
                            self.tf("{}\nClick to fly to the deepest point", &[&trench_where]),
                        )
                        .clicked()
                    {
                        let zoom = RELIEF_FOCUS_ZOOM.max(self.zoom);
                        focus_request = Some((relief.trench_x, relief.trench_y, zoom));
                    }
                    caption(ui, trench_where);

                    ui.label(self.tf(
                        "🌍 Surface: {}% ocean",
                        &[&format!("{:.0}", relief.ocean_share * 100.0)],
                    ));
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
                    panel_title(ui, self.t("🌦 Climate Zones"));
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for zone in ClimateZone::ALL {
                            themes::legend_row(ui, zone.color(), self.t(zone.label()), "");
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
                                self.t(label),
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
                    panel_title(ui, self.t("🌡 Temperature"));
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
                });
        }

        // ── TERRAIN LEGEND (Terrain scope only) ───────────────────────────────
        if self.scope == MapScope::Terrain {
            egui::Panel::right("terrain_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, self.t("🪨 Terrain Types"));
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for class in TerrainClass::ALL {
                            themes::legend_row(
                                ui,
                                class.color(),
                                self.t(class.label()),
                                &class.range_label(self.language),
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
                    panel_title(ui, self.t("🏺 The Dawn of Civilization"));
                    let world = &world.civilizations;
                    if world.is_empty() {
                        ui.label(self.t("No civilization rose on this world."));
                        return;
                    }

                    caption(ui, world.timeline.prehistory_label());
                    ui.label(
                        egui::RichText::new(self.tf(
                            "Atlas date: {} · {}",
                            &[&world.map_year_label(), &world.age_label()],
                        ))
                        .small()
                        .weak(),
                    )
                    .on_hover_text(format!(
                        "{} — {}",
                        world.age().stratum(),
                        world.age().describe()
                    ));
                    ui.separator();
                    ui.label(self.tf(
                        "{} societies on {} inhabited provinces",
                        &[
                            &world.civilizations.len(),
                            &thousands(world.civilized_provinces as f32),
                        ],
                    ));
                    ui.label(self.tf(
                        "{} km² inhabited · world population {}",
                        &[
                            &thousands(world.civilized_area_km2),
                            &thousands_u64(world.total_population),
                        ],
                    ));
                    ui.label(self.tf(
                        "Mean density {} people/km²",
                        &[&format!("{:.2}", world.mean_density_per_km2())],
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
                    for (index, tier) in SocietyTier::LADDER.iter().enumerate() {
                        society_key_row(
                            ui,
                            *tier,
                            counts[index],
                            100.0 * areas[index] / inhabited_area,
                            self.language,
                        );
                    }
                    ui.separator();

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
                                egui::RichText::new(
                                    self.tf("{} ({})", &[&self.t(tier.label()), &shown]),
                                )
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
                                let detail = format!(
                                    "{} · {}",
                                    civ.environment.label(),
                                    civ.culture.government.label(),
                                );
                                let census = self.tf(
                                    "{} provinces · {} people",
                                    &[&civ.provinces.len(), &thousands_u64(civ.population)],
                                );
                                if society_entry(
                                    ui,
                                    civ.color,
                                    civ.name.as_str(),
                                    &detail,
                                    &census,
                                    self.t("Click to fly to this society's heartland"),
                                    selected,
                                )
                                .clicked()
                                {
                                    clicked_civ = Some(id);
                                }
                            }
                            if shown > listed {
                                caption(
                                    ui,
                                    self.tf("… and {} more, all on the map.", &[&(shown - listed)]),
                                );
                            }
                        }
                    });
                });
        }

        // ── RELIGIONS LEGEND (Religions scope only) ───────────────────────────
        // The subject is the world's faiths, so the key leads with the nine
        // metaphysical families and the list beneath it names every religion that
        // actually rose, grouped under the family whose hue it wears.
        if self.scope.is_religions() {
            egui::Panel::right("religions_legend")
                .frame(panel_frame.clone())
                .resizable(false)
                .exact_size(CIV_LEGEND_WIDTH)
                .show(ui, |ui| {
                    panel_title(ui, self.t("🛐 Religions of the World"));
                    let dawn = &world.civilizations;
                    if dawn.is_empty() {
                        caption(ui, self.t("No religion has taken root on this world."));
                        return;
                    }

                    caption(ui, dawn.timeline.prehistory_label());
                    ui.label(
                        egui::RichText::new(self.tf(
                            "Atlas date: {} · {}",
                            &[&dawn.map_year_label(), &dawn.age_label()],
                        ))
                        .small()
                        .weak(),
                    )
                    .on_hover_text(format!(
                        "{} — {}",
                        dawn.age().stratum(),
                        dawn.age().describe()
                    ));
                    ui.separator();
                    ui.label(self.tf(
                        "{} religions on {} inhabited provinces",
                        &[
                            &dawn.religions.len(),
                            &thousands(dawn.civilized_provinces as f32),
                        ],
                    ));
                    ui.label(self.tf(
                        "{} religions · {} followers",
                        &[&dawn.religions.len(), &thousands_u64(dawn.total_population)],
                    ));
                    let multi_realm = dawn
                        .religions
                        .iter()
                        .filter(|religion| religion.realms.len() > 1)
                        .count();
                    caption(
                        ui,
                        self.tf(
                            "{} of them are kept in more than one realm",
                            &[&multi_realm],
                        ),
                    );
                    ui.separator();

                    // ── THE NINE-FAMILY KEY ───────────────────────────────
                    // A root with no religion on this world is not on the map,
                    // so it buys no row; the families that did rise lead with
                    // what their hue means.
                    let counts = dawn.counts_by_root();
                    let areas = dawn.area_by_root();
                    let inhabited_area: f32 = areas.iter().sum::<f32>().max(1.0);
                    for (index, root) in MetaphysicalRoot::ALL.iter().enumerate() {
                        if counts[index] == 0 {
                            continue;
                        }
                        religion_key_row(
                            ui,
                            *root,
                            counts[index],
                            100.0 * areas[index] / inhabited_area,
                            self.language,
                        );
                    }
                    ui.separator();

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for root in MetaphysicalRoot::ALL {
                            let mut faiths: Vec<(u16, &Religion)> = dawn
                                .religions
                                .iter()
                                .enumerate()
                                .map(|(index, religion)| (index as u16 + 1, religion))
                                .filter(|(_, religion)| religion.faith.root == root)
                                .collect();
                            if faiths.is_empty() {
                                continue;
                            }
                            // A legend reads like a census: the faiths that hold
                            // most people come first, founding order breaking ties.
                            faiths.sort_by(|a, b| {
                                b.1.population.cmp(&a.1.population).then(a.0.cmp(&b.0))
                            });

                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    self.tf("{} ({})", &[&self.t(root.label()), &faiths.len()]),
                                )
                                .strong(),
                            );
                            let mut listed = 0usize;
                            for (id, religion) in faiths.iter() {
                                if listed >= LISTED_PER_TIER {
                                    break;
                                }
                                listed += 1;
                                let selected = self.selected_religion == Some(*id);
                                let detail = religion.faith.creed();
                                let census = self.tf(
                                    "{} provinces · {} followers · {} realms",
                                    &[
                                        &religion.provinces.len(),
                                        &thousands_u64(religion.population),
                                        &religion.realms.len(),
                                    ],
                                );
                                let name = religion.faith.display_name();
                                if society_entry(
                                    ui,
                                    religion.color,
                                    name.as_str(),
                                    &detail,
                                    &census,
                                    self.t("Click to fly to this faith's holiest site"),
                                    selected,
                                )
                                .clicked()
                                {
                                    clicked_religion = Some(*id);
                                }
                            }
                            if faiths.len() > listed {
                                caption(
                                    ui,
                                    self.tf(
                                        "… and {} more, all on the map.",
                                        &[&(faiths.len() - listed)],
                                    ),
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
                    panel_title(ui, self.t("🗣 Languages of the World"));
                    ui.separator();
                    if world.languages.is_empty() {
                        caption(ui, self.t("No tongue has been given to this world yet."));
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
                                    self.tf(
                                        "{} languages · {} provinces · {} people",
                                        &[
                                            &family.languages.len(),
                                            &family.province_count,
                                            &thousands_u64(family.population),
                                        ],
                                    ),
                                );
                                caption(
                                    ui,
                                    self.tf(
                                        "Urheimat: {}",
                                        &[&world.languages.urheimat_label(family.id)],
                                    ),
                                );
                                for language in
                                    world.languages.languages_of(family.id).iter().take(6)
                                {
                                    ui.horizontal(|ui| {
                                        themes::swatch(ui, language.color);
                                        ui.label(egui::RichText::new(&language.name).small());
                                        caption(
                                            ui,
                                            self.tf("{} provinces", &[&language.province_count]),
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

        // A click on a religion's legend entry flies to the faith's holiest
        // place, falling back to the province it rose in when nothing is named.
        if let Some(id) = clicked_religion {
            self.selected_religion = Some(id);
            if let Some(religion) = world.civilizations.religion(id) {
                let focus_province = religion
                    .faith
                    .sacred_sites
                    .first()
                    .map_or(religion.origin_province, |site| site.province);
                if let Some(node) = world.hierarchy.provinces.get(focus_province as usize) {
                    let zoom = CIV_FOCUS_ZOOM.max(self.zoom);
                    focus_request = Some((node.center_x, node.center_y, zoom));
                }
            }
        }

        // The 16K plate is only regenerated when the world or the scope has
        // changed — never per frame — and never while an act is running, because
        // while an act is running the *live* plate is what the canvas shows. Nor
        // while an export is painting the same world on its worker: the old
        // plate stays on screen, and the invalidation is honoured the moment the
        // export finishes (see `invalidate_plate`).
        if self.job.is_none() && self.export_job.is_none() && self.texture.is_none() {
            let img = Self::generate_texture(world, self.plate_view());
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
        if self.pending_fit {
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

        // The zoom pill sits on the canvas, so the canvas must leave the press,
        // the drag and the scroll to it: a press on the pill is a press on the
        // camera, not a pan of the map.
        let pill = Self::zoom_pill_rect(rect);
        let over_pill = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pointer| pill.contains(pointer));

        if response.dragged() && !over_pill {
            self.offset += response.drag_delta();
        }

        if !over_pill && ui.rect_contains_pointer(rect) {
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
        // and when the act lands, the 16K plate replaces it in place.
        let live = if self.job.is_some() {
            self.live.as_ref()
        } else {
            None
        };
        let plate = live
            .map(|live| (&live.texture, live.label))
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
                painter.rect_filled(strip, 4.0, CHROME_VEIL);
                let text = format!(
                    "🔴 {} — {}",
                    self.t(live_label),
                    stage_progress(self.job.as_ref(), self.language)
                );
                painter.text(
                    strip.left_center() + egui::vec2(12.0, 0.0),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::proportional(TEXT_BODY),
                    Color32::from_rgb(240, 214, 180),
                );
            }
        }

        // ── ZOOM PILL ─────────────────────────────────────────────────────────
        // Drawn after the map, so it floats over it, and before the hover card,
        // so a card opened over the pill is never buried under it. The pointer
        // over the pill is not a pointer over the map: the guard above keeps the
        // canvas from panning or zooming beneath it.
        self.zoom_pill(ui, rect);

        // ── HOVER INSPECTION TOOLTIP ──────────────────────────────────────────
        if let Some(mouse_pos) = response.hover_pos().filter(|_| !over_pill) {
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
                                self.language,
                            );
                        } else if lake {
                            card_title(ui, self.t("💧 Inland Freshwater Lake"));
                            ui.label(self.t("No realm claims the water itself."));
                            ui.separator();
                            ui.label(self.tf("Lake Surface: {} m", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else if is_ocean {
                            card_title(ui, self.t("🌊 Uncivilized Sea"));
                            ui.label(self.tf("Depth: {} m", &[&format!("{:.0}", relief::depth_metres(elev))]));
                            ui.separator();
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else if is_waste {
                            card_title(ui, self.t("🏔️ Wasteland Beyond History"));
                            ui.strong(self.t("Impassable or unclaimed wilderness"));
                            ui.separator();
                            ui.label(self.tf("Environment: {}", &[&get_biome_name(display_temp, precip, altitude)]));
                            ui.label(self.tf("Elevation: {} m", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else {
                            card_title(ui, self.t("🌾 Unclaimed Land"));
                            ui.strong(self.t("No civilization has risen here"));
                            caption(ui, self.t("Fertile enough to live on, but never the best ground in reach: nothing here outperformed the cradles."));
                            ui.separator();
                            ui.label(self.tf("Environment: {}", &[&get_biome_name(display_temp, precip, altitude)]));
                            ui.label(self.tf("Elevation: {} m", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                            ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                            ui.label(self.tf("Precipitation: {} mm/yr", &[&format!("{:.0}", precip * 1200.0)]));
                            ui.separator();
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        }
                    } else if self.scope == MapScope::Languages {
                        // ── The language readout ──────────────────────────────
                        let lake = world
                            .is_lake
                            .as_ref()
                            .is_some_and(|lake| *lake.get(map_x, map_y));
                        if is_ocean {
                            card_title(ui, self.t("🌊 Open Water"));
                            ui.label(self.t("No land, and so no tongue of its own."));
                            ui.separator();
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else if lake {
                            card_title(ui, self.t("💧 Inland Freshwater Lake"));
                            ui.label(self.t("The water itself belongs to no language."));
                            ui.separator();
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
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
                            ui.label(self.tf("Autonym: {}", &[&language.autonym]));
                            ui.label(self.tf("Urheimat: {}", &[&world.languages.urheimat_label(family.id)]));
                            if let Some(name) = world
                                .languages
                                .province_names
                                .get(id as usize)
                                .filter(|name| !name.is_empty())
                            {
                                ui.label(self.tf("Province: {}", &[&name]));
                            }
                            let owner = world.civilizations.owner_of(id);
                            if owner != 0 {
                                ui.separator();
                            ui.label(self.tf(
                                "Society: {}",
                                &[&world.civilizations.name_of(owner)],
                            ));
                                if let Some(tier) = world.civilizations.tier_of(id) {
                                    ui.label(egui::RichText::new(tier.label()).small());
                                }
                            }
                            ui.separator();
                            let size = world
                                .hierarchy
                                .provinces
                                .get(id as usize)
                                .map_or(0, |node| node.pixel_count);
                            ui.label(self.tf("Size: {} px", &[&size]));
                            ui.label(self.tf(
                                "Elevation: {} m",
                                &[&format!("{:.0}", relief::altitude_metres(elev))],
                            ));
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else {
                            card_title(ui, self.t("🏔️ Beyond the Tongues"));
                            ui.strong(self.t("No language has settled this ground"));
                            ui.separator();
                            ui.label(self.tf(
                                "Environment: {}",
                                &[&get_biome_name(display_temp, precip, altitude)],
                            ));
                            ui.label(self.tf(
                                "Elevation: {} m",
                                &[&format!("{:.0}", relief::altitude_metres(elev))],
                            ));
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        }
                    } else if self.scope == MapScope::Religions {
                        // ── The religion readout ──────────────────────────────
                        // A faith belongs to the society that keeps it, so one
                        // hover answers both halves of the question: what is
                        // believed here, and who believes it.
                        let lake = world
                            .is_lake
                            .as_ref()
                            .is_some_and(|lake| *lake.get(map_x, map_y));
                        let owner = world.civilizations.owner_of(id);
                        if owner != 0 && !is_ocean && !lake {
                            let civ = world
                                .civilizations
                                .civilization(owner)
                                .expect("owner id always indexes a civilization");
                            match world.civilizations.religion_of_province(id) {
                                Some(religion) => {
                                    religion_dossier(ui, religion, id, civ, world, self.language)
                                }
                                None => {
                                    card_title(ui, self.t("🌾 Unclaimed Land"));
                                    ui.strong(self.t("No faith is kept here"));
                                }
                            }
                            ui.separator();
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else if lake {
                            card_title(ui, self.t("💧 Inland Freshwater Lake"));
                            ui.label(self.t("The water itself keeps no faith."));
                            ui.separator();
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else if is_ocean {
                            card_title(ui, self.t("🌊 Open Water"));
                            ui.label(self.t("No faith is kept on the open sea."));
                            ui.separator();
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else if is_waste {
                            card_title(ui, self.t("🏔️ Beyond the Faiths"));
                            ui.strong(self.t("No religion has settled this ground"));
                            ui.separator();
                            ui.label(self.tf(
                                "Environment: {}",
                                &[&get_biome_name(display_temp, precip, altitude)],
                            ));
                            ui.label(self.tf(
                                "Elevation: {} m",
                                &[&format!("{:.0}", relief::altitude_metres(elev))],
                            ));
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        } else {
                            card_title(ui, self.t("🌾 Unclaimed Land"));
                            ui.strong(self.t("No faith is kept here"));
                            ui.separator();
                            ui.label(self.tf(
                                "Environment: {}",
                                &[&get_biome_name(display_temp, precip, altitude)],
                            ));
                            ui.label(self.tf(
                                "Elevation: {} m",
                                &[&format!("{:.0}", relief::altitude_metres(elev))],
                            ));
                            ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                            ui.label(self.tf(
                                "Precipitation: {} mm/yr",
                                &[&format!("{:.0}", precip * 1200.0)],
                            ));
                            ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                        }
                    } else if self.scope == MapScope::Topographic {
                        // ── Hypsometric survey readout ────────────────────────
                        let band = hypsometric_band(metres);
                        let surface = if is_ocean {
                            if frozen_sea { frozen_sea_name(display_temp) } else { "Open Ocean" }
                        } else if world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y)) {
                            "Inland Freshwater Lake"
                        } else if display_temp < GLACIAL_LAND_TEMP {
                            "Ice-capped Land"
                        } else {
                            get_biome_name(display_temp, precip, altitude)
                        };

                        card_title(ui, format!("⛰ {}", surface));
                        ui.separator();
                        if elev > sea_level {
                            ui.label(self.tf("Elevation: {} m above sea level", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                        } else {
                            ui.label(self.tf("Depth: {} m below sea level", &[&format!("{:.0}", relief::depth_metres(elev))]));
                        }
                        ui.label(self.tf(
                            "Relief band: {}  ({})",
                            &[&HYPSOMETRIC_NAMES[band], &hypsometric_range_label(band)],
                        ));
                        // Every surface reports a temperature — the land its
                        // station temperature, the sea its surface temperature.
                        if is_ocean {
                            ui.label(self.tf("Sea-Surface Temperature: {} °C", &[&format!("{:.1}", temp)]));
                        } else {
                            ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                            ui.label(self.tf("Precipitation: {} mm/yr", &[&format!("{:.0}", precip * 1200.0)]));
                        }
                        if !is_ocean && flow >= self.genesis.hydro_config().min_river_flow {
                            ui.label(self.tf("River Flow: {} m³/s", &[&format!("{:.0}", flow)]));
                        }
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else if self.scope == MapScope::Climate {
                        // ── Climate readout ────────────────────────────
                        let lake = world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y));
                        if lake {
                            card_title(ui, self.t("🌦 Inland Lake"));
                            ui.label(self.t("Fresh water, read with the climate of its shores."));
                        } else if is_ocean {
                            card_title(ui, format!("🌦 {}", self.t(themes::ocean_climate_label(temp))));
                            ui.separator();
                            ui.label(self.tf("Sea-surface temperature: {} °C", &[&format!("{:.1}", temp)]));
                            if frozen_sea {
                                ui.label(self.t("Perennial sea ice."));
                            }
                        } else {
                            let zone = ClimateZone::classify(display_temp, precip, altitude);
                            card_title(ui, format!("🌦 {}", self.t(zone.label())));
                            caption(ui, self.t(zone.describe()));
                            ui.separator();
                            ui.label(self.tf("Mean temperature: {} °C", &[&format!("{:.1}", temp)]));
                            ui.label(self.tf("Annual rainfall: {} mm/yr", &[&format!("{:.0}", precip * 1200.0)]));
                            ui.label(format!(
                                "Elevation: {:.0} m",
                                relief::altitude_metres(elev)
                            ));
                        }
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else if self.scope == MapScope::Temperature {
                        // ── Thermal readout ────────────────────────────
                        let band = themes::isotherm_band(temp);
                        let lake = world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y));
                        let surface = if is_ocean {
                            if frozen_sea {
                                self.t("Sea ice")
                            } else {
                                self.t("Sea surface")
                            }
                        } else if lake {
                            self.t("Lake surface")
                        } else if display_temp < GLACIAL_LAND_TEMP {
                            self.t("Ice-capped land")
                        } else {
                            self.t("Land surface")
                        };
                        card_title(ui, format!("🌡 {:.1} °C", temp));
                        caption(ui, surface);
                        ui.separator();
                        ui.label(self.tf(
                            "Isotherm band: {}",
                            &[&themes::isotherm_band_label(band)],
                        ));
                        if elev > sea_level {
                            ui.label(self.tf(
                                "Elevation: {} m",
                                &[&format!("{:.0}", relief::altitude_metres(elev))],
                            ));
                        }
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else if self.scope == MapScope::Terrain {
                        // ── Terrain readout ────────────────────────────
                        let lake = world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y));
                        let metres = relief::to_metres(elev);
                        let class = TerrainClass::classify(metres, lake, display_temp);
                        let (gx, gy) =
                            world.heightmap.gradient(map_x as f32, map_y as f32);
                        let slope = (gx * gx + gy * gy).sqrt();
                        card_title(ui, format!("🪨 {}", self.t(class.label())));
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
                        ui.label(self.tf("Band: {}", &[&class.range_label(self.language)]));
                        if !is_ocean && !lake && class != TerrainClass::Ice {
                            ui.label(self.tf("Ground: {} ({} m rise per pixel)", &[&self.t(themes::slope_label(slope)), &format!("{:.0}", slope * relief::LAND_M_PER_UNIT)]));
                        }
                        ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
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
                            ui.label(self.tf("Elevation: {} m", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                        } else {
                            ui.label(self.tf("Depth: {} m", &[&format!("{:.0}", relief::depth_metres(elev))]));
                        }
                        ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                        ui.label(self.tf("Precipitation: {} mm/yr", &[&format!("{:.0}", precip * 1200.0)]));
                        if flow >= self.genesis.hydro_config().min_river_flow {
                            ui.label(self.tf("River Flow: {} m³/s", &[&format!("{:.0}", flow)]));
                        }
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else if is_ocean
                        && ocean_id != 0
                        && (ocean_id as usize) < world.ocean_hierarchy.provinces.len()
                    {
                        // ── Hydro-political marine realm ───────────────
                        let node = &world.ocean_hierarchy.provinces[ocean_id as usize];
                        if frozen_sea {
                            card_title(ui, self.tf("🧊 Ocean Province #{}", &[&ocean_id]));
                            ui.strong(self.tf(
                                "{} · Impassable Polar Wasteland",
                                &[&frozen_sea_name(display_temp)],
                            ));
                        } else {
                            card_title(ui, self.tf("🌊 Ocean Province #{}", &[&ocean_id]));
                        }
                        ui.separator();
                        ui.horizontal(|ui| { ui.label(self.t("Area:"));        ui.strong(format!("#{}", node.area_id)); });
                        ui.horizontal(|ui| { ui.label(self.t("Region:"));      ui.strong(format!("#{}", node.region_id)); });
                        ui.horizontal(|ui| { ui.label(self.t("Superregion:")); ui.strong(format!("#{}", node.superregion_id)); });
                        ui.horizontal(|ui| { ui.label(self.t("Ocean Basin:")); ui.strong(format!("#{}", node.continent_id)); });
                        ui.separator();
                        ui.label(self.tf("Size: {} px", &[&node.pixel_count]));
                        ui.label(self.tf(
                            "Sea Floor Depth: {} m",
                            &[&format!("{:.0}", relief::depth_metres(elev))],
                        ));
                        ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else if is_waste {
                        if display_temp < PACK_ICE_TEMP {
                            card_title(ui, self.t("🧊 Polar Ice Wasteland"));
                            ui.strong(self.t("Permanent Ice Sheet / Impassable Wilderness"));
                        } else {
                            card_title(ui, self.t("🏔️ Wasteland"));
                            ui.strong(self.t("Impassable / Uninhabited Wilderness"));
                        }
                        ui.separator();
                        ui.label(self.tf("Environment: {}", &[&get_biome_name(display_temp, precip, altitude)]));
                        ui.label(self.tf("Elevation: {} m", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                        ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                        ui.label(self.tf("Precipitation: {} mm/yr", &[&format!("{:.0}", precip * 1200.0)]));
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else if world.is_lake.as_ref().is_some_and(|lake| *lake.get(map_x, map_y)) {
                        card_title(ui, self.t("💧 Inland Freshwater Lake"));
                        ui.separator();
                        ui.label(self.tf("Lake Surface: {} m", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                        ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                        ui.label(self.tf("Precipitation: {} mm/yr", &[&format!("{:.0}", precip * 1200.0)]));
                        ui.separator();
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else if id != 0 && (id as usize) < world.hierarchy.provinces.len() {
                        let node = &world.hierarchy.provinces[id as usize];

                        card_title(ui, self.tf("📍 Province #{}", &[&id]));
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(self.t("Area:"));
                            ui.strong(format!("#{}", node.area_id));
                        });
                        ui.horizontal(|ui| {
                            ui.label(self.t("Region:"));
                            ui.strong(format!("#{}", node.region_id));
                        });
                        ui.horizontal(|ui| {
                            ui.label(self.t("Superregion:"));
                            ui.strong(format!("#{}", node.superregion_id));
                        });
                        ui.horizontal(|ui| {
                            ui.label(self.t("Continent:"));
                            ui.strong(format!("#{}", node.continent_id));
                        });
                        if node.anchor_id != 0 && node.anchor_id != node.id {
                            ui.horizontal(|ui| {
                                ui.label(self.t("Anchor Coast:"));
                                ui.strong(self.tf("Province #{}", &[&node.anchor_id]));
                            });
                        }
                        // ── The society this province belongs to ──────────────
                        // Shown in every scope, because the province's population
                        // and its holders are facts about the province, not about the
                        // map mode currently on screen.
                        let owner = world.civilizations.owner_of(id);
                        if owner != 0 {
                            ui.horizontal(|ui| {
                                ui.label(self.t("Society:"));
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
                        ui.label(self.tf("Size: {} px", &[&node.pixel_count]));
                        if owner != 0 {
                            let population = world.civilizations.population_of(id);
                            let density = population as f32
                                / (node.pixel_count as f32
                                    * crate::core::sphere::pixel_area_km2(
                                        map_y as usize,
                                        world.height,
                                        world.width,
                                    ))
                                .max(1.0);
                            ui.label(self.tf(
                                "Population: {} people ({}/km²)",
                                &[&thousands_u64(population), &format!("{:.1}", density)],
                            ));
                            ui.label(self.tf(
                                "Society population: {} people",
                                &[&thousands_u64(
                                    world
                                        .civilizations
                                        .civilization(owner)
                                        .map_or(0, |civ| civ.population),
                                )],
                            ));
                        }
                        ui.label(self.tf("Elevation: {} m", &[&format!("{:.0}", relief::altitude_metres(elev))]));
                        ui.label(self.tf("Temperature: {} °C", &[&format!("{:.1}", temp)]));
                        ui.label(self.tf("Precipitation: {} mm/yr", &[&format!("{:.0}", precip * 1200.0)]));
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    } else {
                        card_title(ui, self.t("🌊 Ocean"));
                        ui.label(self.tf("Depth: {} m", &[&format!("{:.0}", relief::depth_metres(elev))]));
                        ui.label(self.tf("Coordinates: ({}, {})", &[&map_x, &map_y]));
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── THE WINDOW'S DEVICE ─────────────────────────────────────────────────

    /// A 16K plate must fit in one texture and one staging buffer, and the atlas
    /// must never ask for more than the adapter reports — the request would fail
    /// outright on a modest GPU.
    #[test]
    fn the_atlas_asks_for_the_limits_a_16k_plate_needs() {
        let generous = eframe::wgpu::Limits {
            max_texture_dimension_2d: 16384,
            max_buffer_size: 2 << 30,
            ..Default::default()
        };
        let limits = atlas_device_limits(eframe::wgpu::Limits::default(), &generous);
        assert_eq!(limits.max_texture_dimension_2d, 16384);
        assert_eq!(limits.max_buffer_size, 512 << 20);

        let modest = eframe::wgpu::Limits {
            max_texture_dimension_2d: 8192,
            max_buffer_size: 256 << 20,
            ..Default::default()
        };
        let limits = atlas_device_limits(eframe::wgpu::Limits::default(), &modest);
        assert_eq!(
            limits.max_texture_dimension_2d, 8192,
            "never above what the adapter reports"
        );
        assert_eq!(limits.max_buffer_size, 256 << 20);
    }

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
            MapScope::Religions,
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
            MapScope::Religions,
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

    /// A scope whose subject is a people — their realms, their religions, their
    /// tongues — never takes the Civilizations overlay: the tint would repeat the
    /// layer already painted, and an export would be named for a picture it does
    /// not contain.
    #[test]
    fn the_peoples_scopes_never_take_the_civilizations_overlay() {
        for scope in [
            MapScope::Civilizations,
            MapScope::Religions,
            MapScope::Languages,
        ] {
            assert!(
                !scope.accepts_civ_overlay(),
                "{scope:?} must not tint the civilizations over its own subject"
            );
        }
        for scope in [
            MapScope::Physical,
            MapScope::Topographic,
            MapScope::Climate,
            MapScope::Temperature,
            MapScope::Terrain,
            MapScope::Land,
            MapScope::Water,
            MapScope::Everywhere,
        ] {
            assert!(
                scope.accepts_civ_overlay(),
                "{scope:?} must offer the civilizations overlay"
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
        says("Save automatically when act 3, act 6 and act 7 land");
        says("Saved worlds");
        // The acts are a section of their own, counted, and the constants the
        // engine reads are folded away beneath them.
        says("The acts");
        says("Advanced configuration");
        // The first act is offered, and marked as the one to run next.
        says("▶ 1. Geology & Erosion");
        // Nothing after it is offered at all.
        for stage in &Stage::ALL[1..] {
            says(&format!("🔒 {}. {}", stage.index(), stage.title()));
        }
        // And the plate where the world will be is honest about being empty.
        says("No world yet");
        says("Seed 42");
        // Nothing has been made, so nothing claims to have been: the panel is a
        // list of acts, not a log of them.
        assert!(
            !text
                .iter()
                .any(|drawn| drawn.contains("What has been made")),
            "the panel no longer keeps a notebook of the acts it has run"
        );
    }

    /// The language is a property of the reader, not of the world: it must be
    /// choosable before the first act has been run. The picker lives in the top
    /// bar, so its rectangle is near the top of the window even on the cold
    /// start.
    #[test]
    fn the_language_can_be_chosen_before_a_world_exists() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));
        let mut picker = None;

        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| viewer.frame_ui(ui));
            for (text, rect) in drawn_text_rects(&output) {
                if text.contains("🌐 English") {
                    picker = Some(rect);
                }
            }
            output.textures_delta.clear();
        }

        let rect = picker.expect("the cold-start atlas never painted the language picker");
        assert!(
            rect.min.y < 160.0,
            "the picker belongs to the top bar, not the empty map: {rect:?}"
        );
    }

    /// The chrome is painted before a world exists as well as after: the brand,
    /// both rows of controls, and the one press that begins the world.
    #[test]
    fn the_top_bar_and_the_first_act_are_there_before_there_is_a_world() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        let says = |needle: &str| {
            assert!(
                text.iter().any(|drawn| drawn.contains(needle)),
                "the cold-start atlas never painted {needle:?}"
            );
        };

        says("🌍 Atlas Viewer");
        says("No world yet");
        says("Tier:");
        // The export is in the bar before a world exists — disabled, but never
        // hidden, so the control does not appear out of nowhere once it works.
        says("💾 Export");
        says("▶ Begin — 1. Geology & Erosion");
    }

    /// A long seed used to split the bar in two: the seed label wrapped at the
    /// space between "Seed" and the number, so the bar grew a row and the map
    /// underneath moved. The seed is a chip now, and a chip never wraps.
    #[test]
    fn a_long_seed_never_wraps_the_top_bar() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let seed = u64::MAX;
        let mut viewer = MapViewer::new(Genesis::new(seed));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));

        // The seed is painted twice — once in the bar's chip and once on the
        // cold-start plate — so the test keeps the topmost spelling.
        let mut seed_rect: Option<egui::Rect> = None;
        let mut title_rect: Option<egui::Rect> = None;
        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| viewer.frame_ui(ui));
            for (text, rect) in drawn_text_rects(&output) {
                if text.contains("Seed") && text.contains(&seed.to_string()) {
                    seed_rect = Some(match seed_rect {
                        Some(best) if best.min.y <= rect.min.y => best,
                        _ => rect,
                    });
                }
                if text.contains("🌍 Atlas Viewer") {
                    title_rect = Some(rect);
                }
            }
            output.textures_delta.clear();
        }

        let seed_rect = seed_rect.expect("the bar never painted the seed");
        let title_rect = title_rect.expect("the bar never painted the brand");

        // One line tall: a wrapped pair at this type size would be over 30 px.
        assert!(
            seed_rect.height() < 26.0,
            "the seed chip wrapped onto a second line: {seed_rect:?}"
        );
        // And it sits on the brand's row, not on a row of its own.
        assert!(
            (seed_rect.center().y - title_rect.center().y).abs() < 12.0,
            "the seed is not on the brand's row: seed {seed_rect:?}, brand {title_rect:?}"
        );
        // The bar's first row is at the top of the window, so the chip's row has
        // not been pushed down by anything above it.
        assert!(
            seed_rect.min.y < 60.0,
            "the seed chip is not in the top bar: {seed_rect:?}"
        );
    }

    /// The atlas wears its own chrome: the navy the cold-start plate is painted
    /// in, not egui's default grey. The palette is a *palette* — it must not move
    /// a single measurement, so the type scale is checked beside it.
    #[test]
    fn the_atlas_wears_its_own_navy_chrome() {
        let mut style = egui::Style::default();
        configure_style(&mut style);

        assert!(style.visuals.dark_mode, "the atlas is a dark room");
        assert_eq!(style.visuals.panel_fill, CHROME_PANEL);
        assert_eq!(style.visuals.extreme_bg_color, CHROME_INK);
        assert_eq!(style.visuals.faint_bg_color, CHROME_CHIP);
        assert_eq!(style.visuals.selection.bg_fill, CHROME_ACCENT);

        // One family of navies: the plate is darkest, the panel next, the chip
        // lightest — and the chip's text is legible against the chip.
        assert!(CHROME_PANEL.r() > CHROME_INK.r());
        assert!(CHROME_CHIP.r() > CHROME_PANEL.r());
        assert!(CHROME_CHIP_TEXT.r() > CHROME_CHIP.r() + 100);

        // The palette never moved the type scale.
        let body = style
            .text_styles
            .get(&egui::TextStyle::Body)
            .expect("the body size is still installed");
        assert_eq!(body.size, TEXT_BODY);
    }

    /// The zoom controls belong to the map's camera, so they float on the canvas
    /// rather than sitting in the bar: a bar that carries the camera as well as
    /// the scopes is a bar that wraps.
    #[test]
    fn the_zoom_pill_lives_on_the_canvas() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        // A world of six acts is enough to paint the map and its pill.
        let genesis = Genesis::for_test(7, Stage::HydroPolitics);
        let mut viewer = MapViewer::new(genesis);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));

        let mut fit_rect: Option<egui::Rect> = None;
        let mut tier_rect: Option<egui::Rect> = None;
        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| viewer.frame_ui(ui));
            for (text, rect) in drawn_text_rects(&output) {
                if text.contains("⤢ Fit") {
                    fit_rect = Some(rect);
                }
                if text.contains("Tier:") {
                    tier_rect = Some(rect);
                }
            }
            output.textures_delta.clear();
        }

        let fit_rect = fit_rect.expect("the canvas never painted the zoom pill");
        let tier_rect = tier_rect.expect("the bar never painted the tier row");

        // The camera is in the lower-right quadrant of the window, and the tier
        // row is still in the bar: the camera is not in the bar any more.
        assert!(
            fit_rect.center().y > screen.height() * 0.5,
            "the zoom pill is not on the canvas: {fit_rect:?}"
        );
        assert!(
            fit_rect.center().x > screen.width() * 0.55,
            "the zoom pill is not in the corner: {fit_rect:?}"
        );
        assert!(
            tier_rect.center().y < 120.0,
            "the tier row is not in the top bar: {tier_rect:?}"
        );
    }

    /// The bar has to hold its shape at the smallest window the application
    /// opens (1100 × 700): two rows, and every control on them. A crowded bar
    /// used to wrap here first, which is where the seed's wrap showed up.
    #[test]
    fn the_bar_holds_its_shape_at_the_minimum_window() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(u64::MAX));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0));

        let mut painted = Vec::new();
        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| viewer.frame_ui(ui));
            painted = drawn_text_rects(&output);
            output.textures_delta.clear();
        }

        // Every control the bar carries is painted, and all of them are in the
        // bar's two rows: the chrome never grows to a third.
        for needle in [
            "🌍 Atlas Viewer",
            "Seed 18446744073709551615",
            "🌱 No world yet",
            "🌐 English",
            "💾 Export",
            "☰",
            "Tier:",
            "Borders",
        ] {
            let rect = painted
                .iter()
                .filter(|(text, _)| text.contains(needle))
                .map(|(_, rect)| *rect)
                .min_by(|a, b| a.min.y.partial_cmp(&b.min.y).unwrap())
                .unwrap_or_else(|| panic!("the bar never painted {needle:?}"));
            assert!(
                rect.max.y < 80.0,
                "{needle:?} is not in the bar's two rows: {rect:?}"
            );
        }
    }

    /// The engine's own constants belong in the configuration panel, not in a
    /// caption beside the seed: a reader who wants them opens the fold-out.
    #[test]
    fn the_engine_constants_are_not_printed_beside_the_seed() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));
        let painted = draw_frames(&ctx, &mut viewer);
        for (drawn, _) in &painted {
            assert!(
                !drawn.contains("Tectonics") && !drawn.contains("erosion droplets"),
                "the panel still prints the engine's constants beside the seed: {drawn:?}"
            );
        }
    }

    /// The genesis panel can be folded away so the map claims the window; the
    /// chrome stays, which is what keeps the language and the scopes in reach.
    #[test]
    fn the_genesis_panel_can_be_folded_away_for_the_map() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));
        viewer.show_genesis_panel = false;
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();

        assert!(
            text.iter().any(|drawn| drawn.contains("🌍 Atlas Viewer")),
            "the top bar must outlive the panel it toggles"
        );
        assert!(
            !text.iter().any(|drawn| drawn.contains("🧬 Genesis")),
            "the folded panel is still being painted"
        );
    }
    /// The genesis panel's content is held clear of egui's floating scroll bar.
    ///
    /// The bar is painted *over* the right edge of a scroll area and senses
    /// every press in its `bar_width` band, so a control flush with that edge —
    /// the ↺ that ends a settings row, a full-width saved-world row — would
    /// never see a click. The panel reserves the band with a content margin
    /// measured from the style itself; this draws a real panel and reads the two
    /// back.
    #[test]
    fn the_panel_holds_its_content_clear_of_the_scroll_bar() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));

        let mut content_right = 0.0;
        let mut inner_right = 0.0;
        let mut bar_width = 0.0;

        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| {
                egui::Panel::left("genesis_gutter_probe")
                    .frame(atlas_panel_frame(ui.style()))
                    .resizable(false)
                    .exact_size(GENESIS_PANEL_WIDTH)
                    .show(ui, |ui| {
                        bar_width = ui.spacing().scroll.bar_width;
                        let scroll = egui::ScrollArea::vertical()
                            .id_salt("genesis_gutter_scroll")
                            .auto_shrink([false, false])
                            .content_margin(clear_of_scroll_bar(ui))
                            .show(ui, |ui| {
                                content_right = ui.max_rect().right();
                            });
                        inner_right = scroll.inner_rect.right();
                    });
            });
            output.textures_delta.clear();
        }

        assert!(
            content_right <= inner_right - bar_width,
            "the panel's content reaches x = {content_right:.1}, inside the {bar_width:.0} px \
             the scroll bar claims at x = {:.1}..{inner_right:.1}",
            inner_right - bar_width,
        );
    }

    /// The census is data, not prose: it survives the redesign, in the footer,
    /// with its three lines intact.
    #[test]
    fn the_bottom_census_survives_the_redesign() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        // The test pipeline's own hierarchies are empty; the footer is about a
        // world that has actually been cut into provinces, so give it some.
        let genesis = Genesis::for_test(42, Stage::HydroPolitics);
        let mut world = genesis.atlas().expect("a world of six acts");
        world.hierarchy = std::sync::Arc::new(GeopoliticalHierarchy {
            total_provinces: 3,
            total_areas: 2,
            total_regions: 2,
            total_superregions: 1,
            total_continents: 1,
            ..GeopoliticalHierarchy::empty()
        });
        world.ocean_hierarchy = std::sync::Arc::new(GeopoliticalHierarchy {
            total_provinces: 2,
            total_areas: 2,
            total_regions: 1,
            total_superregions: 1,
            total_continents: 1,
            ..GeopoliticalHierarchy::empty()
        });
        let mut viewer = MapViewer::with_world(genesis, Some(world));
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        let says = |needle: &str| {
            assert!(
                text.iter().any(|drawn| drawn.contains(needle)),
                "the footer never painted {needle:?}"
            );
        };

        says("🏛 LAND — Continents:");
        says("🌊 OCEAN — Basins:");
    }

    /// An export's notice belongs to the world's figures, not to the bar: a long
    /// output path in the bar would widen the row that has to keep its shape. It
    /// lands in the footer, under the census, even before there is a census to
    /// sit under.
    #[test]
    fn an_export_notice_lands_in_the_footer_not_the_bar() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new(42));
        let notice = "Exported exports/world_42_physical_16384x8192.png";
        viewer.export_notice = Some(notice.to_string());

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));
        let mut painted = Vec::new();
        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| viewer.frame_ui(ui));
            painted = drawn_text_rects(&output);
            output.textures_delta.clear();
        }

        let rect = painted
            .iter()
            .filter(|(text, _)| text.contains(notice))
            .map(|(_, rect)| *rect)
            .min_by(|a, b| a.min.y.partial_cmp(&b.min.y).unwrap())
            .unwrap_or_else(|| panic!("the atlas never painted the export's notice"));

        // The bar is the top two rows; the footer is the bottom of the window.
        // A notice in the bar is exactly what used to make the bar grow.
        assert!(
            rect.min.y > screen.height() * 0.5,
            "the export notice is still in the bar: {rect:?}"
        );
        assert!(
            rect.max.y <= screen.height() + 1.0,
            "the export notice is not inside the window: {rect:?}"
        );
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
        says("🔒 5. Geopolitical Hierarchy");
        // Its climate exists, so the atlas opens on the physical world itself.
        assert_eq!(viewer.scope, MapScope::Physical);
    }

    /// A world adopted from the full cache opens at history, with acts one to
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
        says("▶ 7. History & Languages");
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

        says("🔄 1. Geology & Erosion");
        says("Valley carving");
        says("42%");
        says("🔴 live: Continents rising");
        // The act has not finished, so the world is not there yet — and the plate
        // on the canvas is captioned with the seed it is growing.
        says("Seed 42");
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
            "the Save checkbox means act 3, act 6 and act 7 write the cache"
        );

        // Turning it off is a setting on the pipeline, not a mode in the panel.
        viewer.genesis.set_autosave(false);
        // The world's own constants are the reader's too, and outlive a reset the
        // same way: a new world on the same seed grows under the settings chosen.
        viewer
            .genesis
            .set_settings(crate::pipeline::WorldGenSettings {
                sea_threshold: 0.05,
                ..Default::default()
            });
        // ... and a world begun again keeps both choices.
        viewer.reset_world();
        assert!(
            !viewer.genesis.autosave(),
            "a reset world keeps the cache choice it was grown under"
        );
        assert_eq!(
            viewer.genesis.world_config().sea_threshold,
            0.05,
            "and the constants the reader set"
        );
    }

    /// The picker offers exactly the five documented presets, and reads a canvas
    /// back to the preset that names it.
    #[test]
    fn the_resolution_presets_are_the_documented_sizes() {
        assert_eq!(
            MapResolution::ALL.map(MapResolution::size),
            [
                (2_048, 1_024),
                (4_096, 2_048),
                (8_192, 4_096),
                (12_288, 6_144),
                (16_384, 8_192),
            ]
        );
        assert_eq!(MapResolution::DEFAULT.size(), (8_192, 4_096));
        assert_eq!(MapResolution::from_canvas(8_192, 4_096), MapResolution::K8);
        assert_eq!(
            MapResolution::from_canvas(3_000, 1_500),
            MapResolution::Custom(3_000, 1_500)
        );
    }

    // ─── THE PNG EXPORT ──────────────────────────────────────────────────────

    /// An export may shrink a world, never enlarge it: the 2K plate is available
    /// wherever the world is at least 2K, the 8K plate only from 8K up, and the
    /// world's own size whenever it is neither of them.
    #[test]
    fn an_export_never_upscales_the_world() {
        let available = |native: (usize, usize)| {
            ExportChoice::ALL
                .into_iter()
                .filter(|choice| choice.offered(native) && choice.allowed(native))
                .collect::<Vec<_>>()
        };
        assert_eq!(available((2_048, 1_024)), [ExportChoice::K2]);
        assert_eq!(
            available((4_096, 2_048)),
            [ExportChoice::K2, ExportChoice::Native]
        );
        assert_eq!(
            available((8_192, 4_096)),
            [ExportChoice::K2, ExportChoice::K8]
        );
        assert_eq!(
            available((16_384, 8_192)),
            [ExportChoice::K2, ExportChoice::K8, ExportChoice::Native]
        );

        // A size the world is too small for stays listed — the menu never
        // changes shape — but it cannot be chosen.
        assert!(!ExportChoice::K8.allowed((2_048, 1_024)));
        // An 8K world does not need two identical 8K entries.
        assert!(!ExportChoice::Native.offered((8_192, 4_096)));

        // A canvas that is not the plate's own aspect ratio is only ever
        // exported at its native size: a preset would have to stretch it.
        assert!(!ExportChoice::K2.allowed((3_000, 2_000)));
        assert!(ExportChoice::Native.allowed((3_000, 2_000)));
        assert!(ExportChoice::K2.allowed((3_000, 1_500)));
    }

    /// An export is named for the world it came from and the map mode on screen.
    #[test]
    fn an_export_is_named_for_its_seed_map_mode_and_size() {
        assert_eq!(
            export_file_name(
                42,
                MapScope::Physical,
                MapTier::Regions,
                false,
                (8_192, 4_096)
            ),
            "world_42_physical_8192x4096.png"
        );
        assert_eq!(
            export_file_name(
                42,
                MapScope::Land,
                MapTier::Superregions,
                false,
                (2_048, 1_024)
            ),
            "world_42_land_superregions_2048x1024.png"
        );
        // The ocean's top tier is its basins, as the printed plate says.
        assert_eq!(
            export_file_name(
                7,
                MapScope::Water,
                MapTier::Continents,
                false,
                (16_384, 8_192)
            ),
            "world_7_water_basins_16384x8192.png"
        );
        // The civilizations overlay is part of the picture, so it is part of
        // the name — but only on a scope that is not already about them.
        assert_eq!(
            export_file_name(
                7,
                MapScope::Topographic,
                MapTier::Regions,
                true,
                (2_048, 1_024)
            ),
            "world_7_topographic_civs_2048x1024.png"
        );
        assert_eq!(
            export_file_name(
                7,
                MapScope::Civilizations,
                MapTier::Regions,
                true,
                (2_048, 1_024)
            ),
            "world_7_civilizations_regions_2048x1024.png"
        );
        // The Languages map has no tier, so it never gains a tier segment.
        assert_eq!(
            export_file_name(
                9,
                MapScope::Languages,
                MapTier::Regions,
                false,
                (8_192, 4_096)
            ),
            "world_9_languages_8192x4096.png"
        );
        // The Religions map is its own subject: no tier, and no overlay suffix
        // even when the checkbox was left on in another scope.
        assert_eq!(
            export_file_name(
                9,
                MapScope::Religions,
                MapTier::Regions,
                false,
                (8_192, 4_096)
            ),
            "world_9_religions_8192x4096.png"
        );
        assert_eq!(
            export_file_name(
                9,
                MapScope::Religions,
                MapTier::Regions,
                true,
                (8_192, 4_096)
            ),
            "world_9_religions_8192x4096.png"
        );
        // The same holds for Languages: the overlay is not painted there, so it
        // is not in the name either.
        assert_eq!(
            export_file_name(
                9,
                MapScope::Languages,
                MapTier::Regions,
                true,
                (8_192, 4_096)
            ),
            "world_9_languages_8192x4096.png"
        );
    }

    /// An export never quietly overwrites the one before it.
    #[test]
    fn an_export_never_overwrites_an_earlier_one() {
        let directory = std::env::temp_dir().join(format!(
            "genesis_export_name_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |t| t.subsec_nanos())
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let name = "world_1_physical_2048x1024.png";
        assert_eq!(free_export_path_in(&directory, name), directory.join(name));

        std::fs::write(directory.join(name), b"first").unwrap();
        assert_eq!(
            free_export_path_in(&directory, name),
            directory.join("world_1_physical_2048x1024-2.png")
        );

        std::fs::write(
            directory.join("world_1_physical_2048x1024-2.png"),
            b"second",
        )
        .unwrap();
        assert_eq!(
            free_export_path_in(&directory, name),
            directory.join("world_1_physical_2048x1024-3.png")
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The export is the atlas's own painter, written at the size the reader
    /// chose: the world's own size unchanged, and a clean downsample when the
    /// plate is smaller than the world.
    #[test]
    fn an_export_writes_a_png_of_the_requested_size() {
        let viewer = MapViewer::new(Genesis::for_test(42, Stage::Geology));
        let world = viewer.world.as_ref().expect("a test world");
        let view = viewer.plate_view();
        let directory = std::env::temp_dir().join(format!(
            "genesis_export_png_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |t| t.subsec_nanos())
        ));
        let _ = std::fs::remove_dir_all(&directory);

        let native = directory.join("native.png");
        write_export(world, view, (world.width, world.height), &native).unwrap();
        let image = image::open(&native).unwrap();
        assert_eq!((image.width(), image.height()), (4, 2));

        let small = directory.join("small.png");
        write_export(world, view, (2, 1), &small).unwrap();
        let image = image::open(&small).unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The interactive export hands the plate to a worker, then says where it
    /// went. The worker is joined the way the window joins it: by polling.
    #[test]
    fn an_export_runs_on_its_worker_and_says_where_it_went() {
        let ctx = egui::Context::default();
        let mut viewer = MapViewer::new(Genesis::for_test(1_980, Stage::Geology));

        // Only the files this test adds are removed afterwards.
        let exports = std::path::Path::new(EXPORTS_DIR);
        let matching = || {
            std::fs::read_dir(exports)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .filter(|path| {
                            path.file_name()
                                .and_then(|name| name.to_str())
                                .is_some_and(|name| name.starts_with("world_1980_topographic_4x2"))
                        })
                        .collect::<std::collections::BTreeSet<_>>()
                })
                .unwrap_or_default()
        };
        let before = matching();

        viewer.start_export(&ctx, ExportChoice::Native);
        assert!(viewer.export_job.is_some(), "the export did not start");

        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while viewer.export_job.is_some() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            viewer.poll_export();
        }
        assert!(viewer.export_job.is_none(), "the export did not finish");

        let notice = viewer
            .export_notice
            .as_deref()
            .unwrap_or_default()
            .to_string();
        assert!(
            notice.contains("Exported"),
            "the atlas did not say what it wrote: {notice:?}"
        );

        let written = matching().difference(&before).cloned().collect::<Vec<_>>();
        assert_eq!(
            written.len(),
            1,
            "expected one new export, found {written:?}"
        );
        let image = image::open(&written[0]).unwrap();
        assert_eq!((image.width(), image.height()), (4, 2));
        let _ = std::fs::remove_file(&written[0]);
    }

    /// Before act one the selector's choice becomes the pipeline's canvas at once.
    #[test]
    fn a_resolution_can_be_chosen_before_the_first_act() {
        let mut viewer = MapViewer::new(Genesis::new_sized(42, 8_192, 4_096));
        viewer.resolution = MapResolution::K2;
        assert!(viewer.apply_chosen_resolution());
        assert_eq!(viewer.genesis.canvas(), (2_048, 1_024));
    }

    /// Once a world exists, a new resolution waits for the reset it asks for.
    #[test]
    fn a_resolution_chosen_after_a_world_starts_waits_for_a_reset() {
        let mut viewer = MapViewer::new(Genesis::for_test(42, Stage::Geology));
        viewer.resolution = MapResolution::K2;

        assert!(!viewer.apply_chosen_resolution());
        assert_eq!(
            viewer.genesis.canvas(),
            (4, 2),
            "the grown world keeps its own plate"
        );

        viewer.reset_world();
        assert_eq!(viewer.genesis.canvas(), (2_048, 1_024));
        assert!(viewer.world.is_none());
    }

    /// The resolution row is part of the panel before a world exists, and names
    /// the canvas the atlas will grow.
    #[test]
    fn the_panel_paints_the_resolution_row() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut viewer = MapViewer::new(Genesis::new_sized(42, 8_192, 4_096));
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        assert!(
            text.iter().any(|drawn| drawn.contains("Map resolution")),
            "the selector is part of the panel"
        );
        assert!(
            text.iter().any(|drawn| drawn.contains("8K · 8192 × 4096")),
            "the startup preset is named"
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
        assert_eq!(cache_size(570_425_480, Language::English), "544 MB");
        assert_eq!(
            cache_size(2 * 1024 * 1024 * 1024, Language::English),
            "2.0 GB"
        );

        let now = SystemTime::now();
        assert_eq!(
            cache_age(
                Some(now - std::time::Duration::from_secs(90)),
                Language::English
            ),
            "1 min ago"
        );
        assert_eq!(
            cache_age(
                Some(now - std::time::Duration::from_secs(3 * 86_400)),
                Language::English
            ),
            "3 d ago"
        );
        assert_eq!(cache_age(None, Language::English), "at an unknown time");
    }

    /// The row for a saved world says which acts it holds, when the file's header
    /// names them; an older save with no header says only that a world is there.
    #[test]
    fn a_saved_world_says_which_acts_it_holds() {
        assert_eq!(acts_saved(Some(5), Language::English), "acts 1–5");
        assert_eq!(acts_saved(Some(6), Language::English), "acts 1–6");
        assert_eq!(acts_saved(Some(7), Language::English), "acts 1–7");
        assert_eq!(acts_saved(None, Language::English), "a saved world");
    }

    /// Save is offered from the moment there is a world and withdrawn only when
    /// there is none — the dawn does not take the command away.
    #[test]
    fn save_is_offered_for_every_act_including_the_last() {
        assert!(!can_save(None), "there is nothing to write before act one");
        for stage in Stage::ALL {
            assert!(
                can_save(Some(stage)),
                "act {} is a world worth keeping",
                stage.index()
            );
        }
    }

    /// Loading a saved world brings its advanced configuration and its canvas into
    /// the panel: the sliders show the constants the world was grown with, and the
    /// resolution selector moves to the plate the file was written on.
    #[test]
    fn loading_a_saved_world_restores_its_settings_and_canvas() {
        let seed = 0x0BAD_C0D4;
        let path = crate::pipeline::cache_path(seed, 4, 2);
        let _ = std::fs::remove_file(&path);

        let mut grown = Genesis::for_test(seed, Stage::Hydrology);
        let mut settings = grown.settings().clone();
        settings.sea_threshold = 0.03;
        settings.num_provinces = 9_876;
        settings.development_pace = 1.4;
        grown.set_settings(settings.clone());
        grown.save_cache().expect("write the world");

        // A fresh atlas on the same seed, on its own plate and with the engine's
        // defaults: picking the cache must adopt both the file's canvas and its
        // constants.
        let mut viewer = MapViewer::new(Genesis::new(seed));
        assert_ne!(viewer.genesis.settings(), &settings);
        viewer.pick_cached_world(seed);
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            viewer.genesis.settings(),
            &settings,
            "the panel now holds the constants the save records"
        );
        assert_eq!(
            viewer.resolution,
            MapResolution::from_canvas(4, 2),
            "and the selector shows the cache's own plate"
        );
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
        viewer.notice = Some("✔ 1. Geology & Erosion — 8.0s".to_string());

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
        let notice = viewer.notice.as_deref().unwrap_or_default();
        assert!(
            notice.contains("4242"),
            "and the panel says the seed changed: {notice:?}"
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
                            society_key_row(
                                ui,
                                *tier,
                                9_999 + index,
                                100.0 / 5.0,
                                Language::English,
                            );
                        }
                        ui.separator();
                        // A society's name comes from the world, not from this
                        // file: the longest a name can get must still fit. So
                        // must the two lines under it.
                        society_entry(
                            ui,
                            SocietyTier::States.legend_color(),
                            "The Sunlit Isle Confederacy of the Green Shore",
                            "Open Coast · A Government Whose Name Runs On and On",
                            "12 345 provinces · 5 678 901 people",
                            "Click to fly to this society's heartland",
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
    /// A society's entry is a *row*, not a name: the button that flies the map
    /// to its heartland claims every pixel the panel has left, and the two lines
    /// that describe it sit under the name's column.
    ///
    /// It used to be a shrink-to-fit button pushed in by the swatch and by its
    /// own padding, with the captions left on the panel's edge: a narrow,
    /// indented column that reached neither the panel's edge nor its own
    /// metadata. The label must also stay flush with the row: egui's horizontal
    /// layout centres a button's label, and a full-width button would otherwise
    /// re-indent every name by its own width.
    #[test]
    fn a_society_entry_fills_its_legend_panel() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));
        let mut row = egui::Rect::NOTHING;
        let mut painted = Vec::new();

        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| {
                egui::Panel::right("society_entry_panel")
                    .frame(atlas_panel_frame(ui.style()))
                    .resizable(false)
                    .exact_size(CIV_LEGEND_WIDTH)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            row = society_entry(
                                ui,
                                SocietyTier::States.legend_color(),
                                "Orami",
                                "River Floodplain · Band Society",
                                "600 provinces · 212 705 people",
                                "Click to fly to this society's heartland",
                                false,
                            )
                            .rect;
                        });
                    });
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            });
            painted = drawn_text_rects(&output);
            // A test paints nothing, so the texture deltas are ours to discard.
            output.textures_delta.clear();
        }

        // The panel's own frame margin is all the room the row leaves unused.
        let panel_edge = screen.max.x - 12.0;
        assert!(
            (row.right() - panel_edge).abs() <= 1.0,
            "a society's row must fill its legend, but it ends at x = {:.1} \
             while the panel's content ends at x = {:.1}",
            row.right(),
            panel_edge,
        );

        let text = |needle: &str| -> egui::Rect {
            painted
                .iter()
                .find(|(text, _)| text == needle)
                .unwrap_or_else(|| panic!("{needle:?} must be painted in the legend"))
                .1
        };
        let name = text("Orami");
        let detail = text("River Floodplain · Band Society");
        let census = text("600 provinces · 212 705 people");

        // The name and its two lines share one left edge...
        for (label, line) in [("environment", detail), ("census", census)] {
            assert!(
                (line.min.x - name.min.x).abs() <= 0.5,
                "the {label} line must line up with the name, but it starts at \
                 x = {:.1} while the name starts at x = {:.1}",
                line.min.x,
                name.min.x,
            );
        }
        // ...and the name starts at the row's edge, never centred in it.
        assert!(
            name.min.x - row.min.x <= 0.5,
            "the name must start at the row's left edge, but the row starts at \
             x = {:.1} while the name starts at x = {:.1}",
            row.min.x,
            name.min.x,
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
        let world = Genesis::for_test(1, Stage::History)
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
            MapScope::Religions,
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

    /// The two religion cards split the cascade: the Civilizations card keeps the
    /// faith's name and creed, and the Religions card carries the rest. This is
    /// the contract that keeps the dawn's hover readable.
    #[test]
    fn the_faith_cards_split_name_and_creed_from_the_rest() {
        use crate::civilization::faith::{FaithContext, generate_faith};
        use crate::civilization::{
            CradleKind, Environment, Government, Hierarchy, Outlook, Subsistence,
        };
        use rand::SeedableRng;
        use rand_chacha::ChaCha8Rng;

        let context = FaithContext {
            environment: Environment::Floodplain,
            cradle_kind: CradleKind::RiverFloodplain,
            tier: SocietyTier::States,
            complexity: 0.70,
            subsistence: Subsistence::IrrigatedAgriculture,
            outlook: Outlook::Ceremonial,
            government: Government::TheocraticRepublic,
            hierarchy: Hierarchy::SacredKingship,
            mean_temp_c: 18.0,
            mean_precip: 0.45,
            mean_elevation_m: 120.0,
            sea_px: 0,
            lake_px: 0,
            river: true,
            great_river: true,
            latitude_deg: 22.0,
        };
        let faith = generate_faith(&mut ChaCha8Rng::seed_from_u64(7), &context);

        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let read = |paint: &dyn Fn(&mut egui::Ui)| -> Vec<String> {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| paint(ui));
            let text = drawn_text(&output)
                .into_iter()
                .map(|(line, _)| line)
                .collect();
            output.textures_delta.clear();
            text
        };

        let summary = read(&|ui| faith_summary(ui, &faith));
        let says = |lines: &[String], needle: &str| lines.iter().any(|line| line.contains(needle));
        assert!(
            says(&summary, "Faith:"),
            "the summary lost the faith's name"
        );
        assert!(says(&summary, "Creed:"), "the summary lost the creed");
        for moved in ["Liturgy:", "Cosmos:", "Ethos:", "Taboo:", "Rites:"] {
            assert!(
                !says(&summary, moved),
                "the Civilizations card must leave {moved} to the Religions card"
            );
        }

        let details = read(&|ui| faith_details(ui, &faith));
        for present in ["Cosmos:", "Ethos:", "Taboo:", "Path:", "Rites:"] {
            assert!(
                says(&details, present),
                "the full religion card lost {present}"
            );
        }
    }

    // ─── THE ATLAS IN OTHER TONGUES ──────────────────────────────────────────

    /// The whole shell of the atlas — its toolbar, its genesis panel, its world
    /// data, the legend of the scope it opens on and the picker itself — is read
    /// in the language the reader chose.
    #[test]
    fn the_atlas_speaks_the_readers_language() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        // A world old enough to have a dawn, so the atlas opens with its toolbar
        // and its Civilizations legend on screen.
        let genesis = Genesis::for_test(42, Stage::History);
        let world = genesis.atlas().expect("a world of seven acts");
        let mut viewer =
            MapViewer::with_world(genesis, Some(world)).with_language(Language::German);
        let painted = draw_frames(&ctx, &mut viewer);
        let text: Vec<&str> = painted.iter().map(|(drawn, _)| drawn.as_str()).collect();
        let says = |needle: &str| {
            assert!(
                text.iter().any(|drawn| drawn.contains(needle)),
                "the German atlas never painted {needle:?}; it painted {text:?}"
            );
        };

        says("🌍 Atlas-Betrachter");
        says("🌐 Deutsch");
        says("Stufe:");
        says("🧬 Genesis");
        says("💾 Weltdaten");
        says("📂 Gespeicherte Welten");
        says("Startwert");
        says("📂 Laden");
        says("💾 Speichern");
        says("🏺 Die Morgenröte der Zivilisation");
        says("Auf dieser Welt ist keine Zivilisation entstanden.");
        says("↺ Welt zurücksetzen");
        // And nothing of the English shell is left standing beside it.
        for english in [
            "World data",
            "Saved worlds",
            "Dawn of Civilization",
            "Borders",
        ] {
            assert!(
                !text.iter().any(|drawn| drawn.contains(english)),
                "the German atlas still painted {english:?}"
            );
        }
    }

    /// The picker offers every language in its own name, with the current one
    /// shown on the button itself.
    #[test]
    fn the_language_picker_names_each_language_in_itself() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            i18n::language_picker(ui, Language::Portuguese);
        });
        let drawn = drawn_text(&output);
        output.textures_delta.clear();

        assert!(
            drawn.iter().any(|(text, _)| text.contains("🌐 Português")),
            "the picker never painted its current language: {drawn:?}"
        );
    }

    /// Every act's name and its hover sentence are translated, not merely the
    /// panel around them. A line whose key drifts from its call site would fall
    /// back to English, so this pins them together.
    #[test]
    fn every_act_is_named_and_described_in_every_language() {
        for language in Language::ALL {
            for stage in Stage::ALL {
                let title = language.text(stage.title());
                let blurb = language.text(stage.blurb());
                assert!(!title.is_empty(), "{language:?} has no name for {stage:?}");
                assert!(!blurb.is_empty(), "{language:?} has no blurb for {stage:?}");
                if language != Language::English {
                    assert_ne!(
                        title,
                        stage.title(),
                        "{language:?} left {:?} in English",
                        stage.title()
                    );
                    assert_ne!(
                        blurb,
                        stage.blurb(),
                        "{language:?} left the sentence of {stage:?} in English"
                    );
                }
            }
        }
    }

    /// The five-way society key is vocabulary too: its names and its sentences
    /// are translated, not left as English data in a localised legend.
    #[test]
    fn the_society_key_is_translated_in_every_language() {
        for language in Language::ALL {
            if language == Language::English {
                continue;
            }
            for tier in SocietyTier::LADDER {
                assert_ne!(
                    language.text(tier.label()),
                    tier.label(),
                    "{language:?} left the society tier {:?} in English",
                    tier.label()
                );
            }
        }
    }

    /// Every language the picker offers can name the atlas, and none of them
    /// simply repeats the English title except English itself.
    #[test]
    fn every_language_has_its_own_words_for_the_shell() {
        let english = Language::English.text("🌍 Atlas Viewer");
        for language in Language::ALL {
            if language == Language::English {
                continue;
            }
            let title = language.text("🌍 Atlas Viewer");
            assert_ne!(title, english, "{language:?} left the title in English");
            assert!(!title.trim().is_empty());

            let filled = language.fill("Seed {}", &[&42]);
            assert!(
                !filled.contains("Seed"),
                "{language:?} left a templated caption in English"
            );
        }
    }

    // ─── ONE ICON STYLE ──────────────────────────────────────────────────────

    /// The icon face is a fallback, never the primary face.
    ///
    /// egui measures a row of text — its height and its baseline — from the
    /// family's first font, so making the icon face primary would quietly
    /// re-metric every panel, card and toolbar in the atlas even though it holds
    /// no Latin glyph. This is the guard on that: the four type sizes must come
    /// out exactly as tall with the icon face installed as without it.
    #[test]
    fn the_icon_face_does_not_re_metric_a_row() {
        fn row_heights(ctx: &egui::Context) -> Vec<(f32, f32)> {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label("🌊 The atlas");
            });
            // A delta nobody applies panics when dropped; a test must hand it back.
            output.textures_delta.clear();
            [TEXT_SMALL, TEXT_BODY, TEXT_CARD_TITLE, TEXT_HEADING]
                .into_iter()
                .map(|size| {
                    let proportional = ctx.fonts_mut(|fonts| {
                        fonts.row_height(&egui::FontId::new(size, egui::FontFamily::Proportional))
                    });
                    let monospace = ctx.fonts_mut(|fonts| {
                        fonts.row_height(&egui::FontId::new(size, egui::FontFamily::Monospace))
                    });
                    (proportional, monospace)
                })
                .collect()
        }

        let plain = egui::Context::default();
        let with_icons = egui::Context::default();
        install_icon_fonts(&with_icons);

        assert_eq!(
            row_heights(&with_icons),
            row_heights(&plain),
            "the icon face moved the type scale's baseline: it must stay a fallback"
        );
    }

    /// Every pictogram the atlas paints comes from the bundled picture face, and
    /// every typographic mark from the bundled mark face.
    ///
    /// The icons are emoji codepoints written into the lines themselves — and
    /// into all six translations of them — so a font, not a call site, decides
    /// how they look. This test reads the module's own sources and asserts that
    /// each pictogram the atlas can paint is resolved by [`ICON_FONT`] first, and
    /// each of the three chrome marks by [`SYMBOL_FONT`]: add a glyph without
    /// regenerating the subsets (see `assets/fonts/README.md`) and this fails,
    /// rather than the icon quietly falling back to the desktop's own fonts.
    #[test]
    fn every_icon_comes_from_a_bundled_face() {
        // The viewer's own sources: every line the atlas can translate lives in
        // one of them.
        const SOURCES: &[&str] = &[
            include_str!("viewer.rs"),
            include_str!("viewer/i18n.rs"),
            include_str!("viewer/settings.rs"),
            include_str!("viewer/themes.rs"),
        ];
        // The typographic marks: not pictures, and served by the mark face.
        const TEXT_MARKS: &str = "☰↺→";
        let is_pictogram = |c: char| {
            let cp = c as u32;
            (0x1F000..=0x1FAFF).contains(&cp)
                || (0x2600..=0x27BF).contains(&cp)
                || (0x2B00..=0x2BFF).contains(&cp)
                || (0x2300..=0x23FF).contains(&cp)
                || (0x25A0..=0x25FF).contains(&cp)
        };

        let ctx = egui::Context::default();
        let mut viewer = MapViewer::new(Genesis::new(42));
        draw_frames(&ctx, &mut viewer);

        ctx.fonts_mut(|fonts| {
            let characters = fonts.characters(&egui::FontFamily::Proportional);
            for source in SOURCES {
                for icon in source
                    .chars()
                    .filter(|&c| is_pictogram(c) && !TEXT_MARKS.contains(c))
                {
                    let faces = characters.get(&icon).map(Vec::as_slice).unwrap_or_default();
                    assert_eq!(
                        faces.first().map(String::as_str),
                        Some(ICON_FONT),
                        "U+{:04X} ({icon}) is not drawn by {ICON_FONT}: {faces:?}",
                        icon as u32
                    );
                }
            }
            for mark in TEXT_MARKS.chars() {
                let faces = characters.get(&mark).map(Vec::as_slice).unwrap_or_default();
                assert_eq!(
                    faces.first().map(String::as_str),
                    Some(SYMBOL_FONT),
                    "U+{:04X} ({mark}) is not drawn by {SYMBOL_FONT}: {faces:?}",
                    mark as u32
                );
            }
        });
    }

    /// The bundled face is a colour font, and this build really paints its
    /// layers.
    ///
    /// egui stores a monochrome glyph as white coverage and tints it with the
    /// text colour; a colour glyph keeps its own RGBA. If `epaint/color_fonts`
    /// were off, the wave below would come out blank — so a single pixel whose
    /// channels differ proves OpenMoji's palette reached the atlas.
    #[test]
    fn the_icon_face_paints_in_colour() {
        let ctx = egui::Context::default();
        install_icon_fonts(&ctx);

        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 120.0),
        ));
        let mut output = ctx.run_ui(input, |ui| {
            ui.label(egui::RichText::new("🌊").size(TEXT_HEADING));
        });

        let mut coloured = 0;
        for deltas in output.textures_delta.set.values() {
            for delta in deltas {
                let egui::ImageData::Color(image) = &delta.image;
                coloured += image
                    .pixels
                    .iter()
                    .filter(|pixel| {
                        let [r, g, b, a] = pixel.to_array();
                        a > 0 && (r != g || g != b)
                    })
                    .count();
            }
        }
        // A delta that is never applied (here: never uploaded to a GPU) panics
        // when it is dropped, so a test that reads it must hand it back.
        output.textures_delta.clear();
        assert!(
            coloured > 0,
            "the 🌊 glyph is monochrome: is epaint's `color_fonts` feature enabled?"
        );
    }

    /// One continent, one superregion, two regions and three single-province
    /// areas inside each — enough to see the palette's nesting.
    fn tiered_hierarchy() -> GeopoliticalHierarchy {
        let mut provinces = vec![ProvinceNode {
            id: 0,
            center_x: 0.0,
            center_y: 0.0,
            pixel_count: 0,
            neighbors: Vec::new(),
            area_id: 0,
            region_id: 0,
            superregion_id: 0,
            continent_id: 0,
            anchor_id: 0,
        }];
        for id in 1..=6u32 {
            let mut neighbors = Vec::new();
            if id > 1 {
                neighbors.push((id - 1, 1));
            }
            if id < 6 {
                neighbors.push((id + 1, 1));
            }
            provinces.push(ProvinceNode {
                id,
                center_x: id as f32,
                center_y: 0.0,
                pixel_count: 100,
                neighbors,
                area_id: id,
                region_id: if id <= 3 { 1 } else { 2 },
                superregion_id: 1,
                continent_id: 1,
                anchor_id: id,
            });
        }
        GeopoliticalHierarchy {
            total_provinces: 6,
            total_areas: 6,
            total_regions: 2,
            total_superregions: 1,
            total_continents: 1,
            provinces,
        }
    }

    /// Areas are shades of the Region they belong to, and no two adjacent areas
    /// of one Region are dealt the same shade — the map shows grouping *and*
    /// structure instead of confetti.
    #[test]
    fn the_area_palette_nests_inside_its_region() {
        let hierarchy = tiered_hierarchy();
        let areas = territory_palette(&hierarchy, MapTier::Areas);
        let regions = territory_palette(&hierarchy, MapTier::Regions);

        assert_ne!(areas[1], areas[2]);
        assert_ne!(areas[2], areas[3]);
        assert_ne!(areas[4], areas[5]);
        assert_ne!(areas[5], areas[6]);
        assert_ne!(regions[1], regions[2], "adjacent regions must differ");

        let hue = |color: [f32; 3]| rgb_to_hsv(color).0;
        let circular = |a: f32, b: f32| {
            let d = (a - b).abs();
            d.min(1.0 - d)
        };
        let region_hue = hue(regions[1]);
        for area in 1..=3u32 {
            assert!(
                circular(hue(areas[area as usize]), region_hue) < 0.13,
                "area {area} left its region's hue family"
            );
        }
    }

    /// Provinces, Superregions and Continents keep the golden-ratio wheel; only
    /// the two middle tiers nest.
    #[test]
    fn the_upper_tiers_keep_the_golden_ratio_palette() {
        let hierarchy = tiered_hierarchy();
        for tier in [
            MapTier::Provinces,
            MapTier::Superregions,
            MapTier::Continents,
        ] {
            let palette = territory_palette(&hierarchy, tier);
            for id in 1..palette.len() as u32 {
                assert_eq!(
                    palette[id as usize],
                    id_to_territory_color(id),
                    "{tier:?} id {id} changed colour"
                );
            }
        }
    }

    /// The finer the tier, the fainter its own subdivisions — the frontier above
    /// it always outranks it. The Province plate is the base grain, so its own
    /// line stays strong even though Areas are drawn fainter than it.
    #[test]
    fn borders_get_lighter_as_the_tier_gets_finer() {
        let order = [
            MapTier::Continents,
            MapTier::Superregions,
            MapTier::Regions,
            MapTier::Areas,
        ];
        for pair in order.windows(2) {
            assert!(
                tier_border_weight(pair[0]) > tier_border_weight(pair[1]),
                "{:?} must be inked harder than {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(tier_border_weight(MapTier::Provinces) > tier_border_weight(MapTier::Areas));
        assert_eq!(parent_tier(MapTier::Areas), Some(MapTier::Regions));
        assert_eq!(parent_tier(MapTier::Provinces), None);
    }
}
