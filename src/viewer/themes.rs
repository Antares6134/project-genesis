//! Palettes and classifications for the atlas's **thematic** scopes.
//!
//! Three of the atlas modes do not portray politics at all: the **Climate** scope
//! paints the world's climate zones, the **Temperature** scope its thermal belts,
//! and the **Terrain** scope its landform classes. Each needs two things the
//! physical painter does not: a classifier that turns the engine's raw grids into
//! a *named* class, and a palette plus legend entries so a reader can decode what
//! is on screen. Both live here, side by side, so a class can never be painted one
//! colour and listed as another in the legend.
//!
//! Every classifier reads only what the pipeline already produced — the
//! heightmap, the mean annual temperature and precipitation grids, lake flags and
//! the local slope. Nothing here invents data; it only *names* it.

use eframe::egui::{self, Color32};

use crate::core::fmt::thousands;
use crate::core::relief;

// ─── SHARED ──────────────────────────────────────────────────────────────────

/// Linear interpolation between two RGB colours; `t = 0` keeps `a`.
#[inline]
fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
    ]
}

/// One legend line: a colour swatch, a name and a short qualifier.
///
/// Every thematic legend is drawn with this helper, which is what keeps the
/// Climate, Temperature and Terrain legends looking like the Topographic one.
pub fn legend_row(ui: &mut egui::Ui, color: [u8; 3], label: &str, note: &str) {
    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(egui::vec2(16.0, 11.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(swatch, 2.0, Color32::from_rgb(color[0], color[1], color[2]));
        ui.label(egui::RichText::new(label).small());
        if !note.is_empty() {
            ui.label(egui::RichText::new(note).small().weak());
        }
    });
}

// ─── CLIMATE ─────────────────────────────────────────────────────────────────

/// A land climate zone, named from mean temperature, annual rainfall and
/// altitude.
///
/// The atlas only carries annual means — no seasons, no monthly curve — so the
/// classes are deliberately described the way the data supports: *cold steppe*,
/// *hot desert*, *temperate rainforest*. No class claims a Mediterranean summer
/// or a monsoon season the grids cannot prove.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClimateZone {
    IceCap,
    Tundra,
    ColdSteppe,
    BorealForest,
    Highland,
    ColdDesert,
    SemiArid,
    TemperateForest,
    TemperateRainforest,
    HotDesert,
    Savanna,
    TropicalRainforest,
}

impl ClimateZone {
    /// Pole-to-equator order, for the legend.
    pub const ALL: [ClimateZone; 12] = [
        ClimateZone::IceCap,
        ClimateZone::Tundra,
        ClimateZone::ColdSteppe,
        ClimateZone::BorealForest,
        ClimateZone::Highland,
        ClimateZone::ColdDesert,
        ClimateZone::SemiArid,
        ClimateZone::TemperateForest,
        ClimateZone::TemperateRainforest,
        ClimateZone::HotDesert,
        ClimateZone::Savanna,
        ClimateZone::TropicalRainforest,
    ];

    /// Classify a land pixel. `altitude` stays in heightmap units, exactly as
    /// the biome palette uses it; `precip` is the engine's normalized rainfall.
    pub fn classify(temp: f32, precip: f32, altitude: f32) -> Self {
        if !temp.is_finite() {
            // The two polar rows of the temperature grid are undefined; on every
            // other scope they read as ice, so this one must agree.
            return ClimateZone::IceCap;
        }
        if temp < -3.0 {
            ClimateZone::IceCap
        } else if altitude > 0.18 {
            // High ground is its own zone whatever the latitude: thin air, hard
            // frost at night and a short growing season.
            ClimateZone::Highland
        } else if temp < 2.0 {
            ClimateZone::Tundra
        } else if temp < 12.0 {
            if precip < 0.28 {
                ClimateZone::ColdSteppe
            } else {
                ClimateZone::BorealForest
            }
        } else if temp < 22.0 {
            if precip < 0.14 {
                ClimateZone::ColdDesert
            } else if precip < 0.34 {
                ClimateZone::SemiArid
            } else if precip < 0.62 {
                ClimateZone::TemperateForest
            } else {
                ClimateZone::TemperateRainforest
            }
        } else if precip < 0.16 {
            ClimateZone::HotDesert
        } else if precip < 0.42 {
            ClimateZone::Savanna
        } else {
            ClimateZone::TropicalRainforest
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ClimateZone::IceCap => "Ice cap",
            ClimateZone::Tundra => "Tundra",
            ClimateZone::ColdSteppe => "Cold steppe",
            ClimateZone::BorealForest => "Boreal forest",
            ClimateZone::Highland => "Highland",
            ClimateZone::ColdDesert => "Temperate desert",
            ClimateZone::SemiArid => "Semi-arid grassland",
            ClimateZone::TemperateForest => "Temperate forest",
            ClimateZone::TemperateRainforest => "Temperate rainforest",
            ClimateZone::HotDesert => "Hot desert",
            ClimateZone::Savanna => "Savanna",
            ClimateZone::TropicalRainforest => "Tropical rainforest",
        }
    }

    /// The climate in one short sentence, for the hover readout.
    pub fn describe(self) -> &'static str {
        match self {
            ClimateZone::IceCap => "permanent ice: nothing thaws long enough to grow",
            ClimateZone::Tundra => "a brief summer over frozen subsoil; moss, lichen and low scrub",
            ClimateZone::ColdSteppe => "cold and dry: short grass over hard ground",
            ClimateZone::BorealForest => "long cold winters under a belt of conifers",
            ClimateZone::Highland => "mountain climate: thin air, hard frost, a short season",
            ClimateZone::ColdDesert => "temperate but arid: bare ground and thorn scrub",
            ClimateZone::SemiArid => "dry grassland at the edge of the desert",
            ClimateZone::TemperateForest => "mild and wet enough for broadleaf woodland",
            ClimateZone::TemperateRainforest => "cool, very wet and densely wooded",
            ClimateZone::HotDesert => "hot and almost rainless: sand, gravel and stone",
            ClimateZone::Savanna => {
                "a hot wet season and a long dry one: grass with scattered trees"
            }
            ClimateZone::TropicalRainforest => "hot and wet all year: closed evergreen canopy",
        }
    }

    pub fn color(self) -> [u8; 3] {
        match self {
            ClimateZone::IceCap => [240, 246, 252],
            ClimateZone::Tundra => [170, 176, 158],
            ClimateZone::ColdSteppe => [196, 190, 150],
            ClimateZone::BorealForest => [64, 110, 84],
            ClimateZone::Highland => [150, 132, 116],
            ClimateZone::ColdDesert => [214, 198, 158],
            ClimateZone::SemiArid => [188, 174, 116],
            ClimateZone::TemperateForest => [92, 148, 84],
            ClimateZone::TemperateRainforest => [48, 118, 76],
            ClimateZone::HotDesert => [226, 198, 132],
            ClimateZone::Savanna => [196, 180, 96],
            ClimateZone::TropicalRainforest => [30, 98, 52],
        }
    }
}

/// Colour of the open sea on the Climate scope: water is classified by its
/// surface temperature rather than by land climate zones.
pub fn ocean_climate_color(temp: f32) -> [u8; 3] {
    if !temp.is_finite() || temp < -3.0 {
        [228, 238, 248] // frozen sea
    } else if temp < 4.0 {
        [86, 142, 184] // polar water
    } else if temp < 14.0 {
        [58, 116, 168] // cold temperate water
    } else if temp < 24.0 {
        [44, 96, 150] // warm temperate water
    } else if temp < 28.0 {
        [34, 78, 134] // tropical water
    } else {
        [24, 62, 116] // equatorial water
    }
}

/// Name of the ocean climate band, for the hover readout and the legend.
pub fn ocean_climate_label(temp: f32) -> &'static str {
    if !temp.is_finite() || temp < -3.0 {
        "Frozen sea"
    } else if temp < 4.0 {
        "Polar sea"
    } else if temp < 14.0 {
        "Cold temperate sea"
    } else if temp < 24.0 {
        "Warm temperate sea"
    } else if temp < 28.0 {
        "Tropical sea"
    } else {
        "Equatorial sea"
    }
}

// ─── TEMPERATURE ─────────────────────────────────────────────────────────────

/// Colour stops of the thermal palette, coldest first.
///
/// The palette is read as a number line, not as scenery: violet is deep cold,
/// blue is frost, pale green is the temperate band, and the oranges and reds are
/// the tropics. [`temperature_color`] interpolates between the stops so the map
/// has no visible banding of its own.
pub const TEMPERATURE_STOPS: [(f32, [u8; 3]); 11] = [
    (-40.0, [40, 24, 92]),
    (-30.0, [52, 44, 128]),
    (-20.0, [54, 82, 168]),
    (-10.0, [72, 132, 202]),
    (0.0, [120, 185, 224]),
    (8.0, [170, 214, 190]),
    (16.0, [226, 224, 146]),
    (22.0, [242, 186, 96]),
    (28.0, [228, 122, 62]),
    (34.0, [186, 48, 38]),
    (40.0, [120, 16, 28]),
];

/// The cold end of the palette.
pub const TEMPERATURE_MIN: f32 = -40.0;
/// The warm end of the palette.
pub const TEMPERATURE_MAX: f32 = 40.0;
/// Width of one isotherm band, in °C.
pub const ISOTHERM_STEP: f32 = 5.0;
/// Number of 5 °C bands the palette spans, from −40 °C to +40 °C inclusive.
pub const ISOTHERM_BANDS: usize = 16;

/// Colour of a mean annual (or sea-surface) temperature, in °C.
pub fn temperature_color(temp: f32) -> [u8; 3] {
    if !temp.is_finite() {
        return TEMPERATURE_STOPS[0].1; // undefined polar rows read as deep cold
    }
    let temp = temp.clamp(TEMPERATURE_MIN, TEMPERATURE_MAX);
    for pair in TEMPERATURE_STOPS.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if temp <= t1 {
            return lerp_rgb(c0, c1, (temp - t0) / (t1 - t0));
        }
    }
    TEMPERATURE_STOPS[TEMPERATURE_STOPS.len() - 1].1
}

/// Index of the 5 °C band a temperature falls in (0 at −40 °C).
///
/// The warm end of the scale lands exactly on a band boundary, so the index is
/// clamped into the palette rather than allowed to run one past its end.
#[inline]
pub fn isotherm_band(temp: f32) -> usize {
    if !temp.is_finite() {
        return 0;
    }
    let band = (((temp.clamp(TEMPERATURE_MIN, TEMPERATURE_MAX) - TEMPERATURE_MIN)
        / ISOTHERM_STEP)
        .floor()) as usize;
    band.min(ISOTHERM_BANDS - 1)
}

/// `10 … 15 °C` — the label of one isotherm band.
pub fn isotherm_band_label(band: usize) -> String {
    let lo = TEMPERATURE_MIN + band as f32 * ISOTHERM_STEP;
    format!("{lo:.0} … {:.0} °C", lo + ISOTHERM_STEP)
}

/// Number of 10 °C rows in the temperature legend.
pub const TEMPERATURE_LEGEND_ROWS: usize = 8;

/// One row of the temperature legend, coloured at its midpoint.
pub fn temperature_legend_color(row: usize) -> [u8; 3] {
    temperature_color(TEMPERATURE_MIN + row as f32 * 10.0 + 5.0)
}

/// `−40 … −30 °C` — the label of one temperature legend row.
pub fn temperature_legend_label(row: usize) -> String {
    let lo = TEMPERATURE_MIN + row as f32 * 10.0;
    format!("{lo:.0} … {:.0} °C", lo + 10.0)
}

// ─── TERRAIN ─────────────────────────────────────────────────────────────────

/// A landform / bathymetry class: what kind of ground a pixel is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerrainClass {
    DeepOcean,
    OceanFloor,
    Shelf,
    Lake,
    Ice,
    CoastalLowland,
    Lowland,
    Hill,
    Upland,
    Mountain,
    HighMountain,
}

impl TerrainClass {
    /// Deep water to high summit, in legend order.
    pub const ALL: [TerrainClass; 11] = [
        TerrainClass::DeepOcean,
        TerrainClass::OceanFloor,
        TerrainClass::Shelf,
        TerrainClass::Lake,
        TerrainClass::Ice,
        TerrainClass::CoastalLowland,
        TerrainClass::Lowland,
        TerrainClass::Hill,
        TerrainClass::Upland,
        TerrainClass::Mountain,
        TerrainClass::HighMountain,
    ];

    /// Classify one pixel. `metres` is the signed elevation from the vertical
    /// datum; `display_temp` is the same jittered temperature the painter uses,
    /// so the ice edge on this map always matches the ice edge on the others.
    pub fn classify(metres: f32, is_lake: bool, display_temp: f32) -> Self {
        if is_lake {
            return TerrainClass::Lake;
        }
        if !display_temp.is_finite() {
            return TerrainClass::Ice; // undefined polar rows are permanent ice
        }
        if metres < 0.0 {
            if display_temp < -3.0 {
                return TerrainClass::Ice;
            }
            return if metres < -3000.0 {
                TerrainClass::DeepOcean
            } else if metres < -200.0 {
                TerrainClass::OceanFloor
            } else {
                TerrainClass::Shelf
            };
        }
        if display_temp < -3.0 {
            return TerrainClass::Ice;
        }
        if metres < 150.0 {
            TerrainClass::CoastalLowland
        } else if metres < 600.0 {
            TerrainClass::Lowland
        } else if metres < 1200.0 {
            TerrainClass::Hill
        } else if metres < 2000.0 {
            TerrainClass::Upland
        } else if metres < 3200.0 {
            TerrainClass::Mountain
        } else {
            TerrainClass::HighMountain
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TerrainClass::DeepOcean => "Abyssal plain",
            TerrainClass::OceanFloor => "Ocean floor",
            TerrainClass::Shelf => "Continental shelf",
            TerrainClass::Lake => "Lake",
            TerrainClass::Ice => "Ice",
            TerrainClass::CoastalLowland => "Coastal lowland",
            TerrainClass::Lowland => "Lowland",
            TerrainClass::Hill => "Hill country",
            TerrainClass::Upland => "Upland",
            TerrainClass::Mountain => "Mountain",
            TerrainClass::HighMountain => "High mountain",
        }
    }

    /// The vertical range this class covers, in metres.
    pub fn range_label(self) -> String {
        match self {
            TerrainClass::DeepOcean => "below −3 000 m".to_string(),
            TerrainClass::OceanFloor => "−3 000 … −200 m".to_string(),
            TerrainClass::Shelf => "−200 … 0 m".to_string(),
            TerrainClass::Lake => "lake surface".to_string(),
            TerrainClass::Ice => "frozen ground or sea".to_string(),
            TerrainClass::CoastalLowland => "0 … 150 m".to_string(),
            TerrainClass::Lowland => format!("150 … {} m", thousands(600.0)),
            TerrainClass::Hill => format!("{} … {} m", thousands(600.0), thousands(1200.0)),
            TerrainClass::Upland => format!("{} … {} m", thousands(1200.0), thousands(2000.0)),
            TerrainClass::Mountain => format!("{} … {} m", thousands(2000.0), thousands(3200.0)),
            TerrainClass::HighMountain => format!("{} m +", thousands(3200.0)),
        }
    }

    pub fn color(self) -> [u8; 3] {
        match self {
            TerrainClass::DeepOcean => [14, 28, 64],
            TerrainClass::OceanFloor => [26, 56, 104],
            TerrainClass::Shelf => [56, 108, 156],
            TerrainClass::Lake => [58, 138, 178],
            TerrainClass::Ice => [240, 246, 252],
            TerrainClass::CoastalLowland => [150, 190, 120],
            TerrainClass::Lowland => [122, 168, 96],
            TerrainClass::Hill => [168, 162, 96],
            TerrainClass::Upland => [180, 144, 98],
            TerrainClass::Mountain => [148, 120, 104],
            TerrainClass::HighMountain => [214, 208, 204],
        }
    }
}

/// How steep a pixel is, in words, from the heightmap gradient magnitude.
///
/// The gradient is in heightmap units per pixel; [`relief::LAND_M_PER_UNIT`]
/// turns it into metres of rise per pixel so the thresholds mean something.
pub fn slope_label(slope_units: f32) -> &'static str {
    let m = slope_units * relief::LAND_M_PER_UNIT;
    if m < 120.0 {
        "gentle"
    } else if m < 400.0 {
        "rolling"
    } else if m < 900.0 {
        "steep"
    } else {
        "cliff-like"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climate_zones_follow_temperature_rainfall_and_altitude() {
        assert_eq!(ClimateZone::classify(-10.0, 0.9, 0.05), ClimateZone::IceCap);
        assert_eq!(
            ClimateZone::classify(18.0, 0.9, 0.30),
            ClimateZone::Highland
        );
        assert_eq!(ClimateZone::classify(5.0, 0.1, 0.05), ClimateZone::ColdSteppe);
        assert_eq!(
            ClimateZone::classify(5.0, 0.5, 0.05),
            ClimateZone::BorealForest
        );
        assert_eq!(ClimateZone::classify(25.0, 0.05, 0.05), ClimateZone::HotDesert);
        assert_eq!(ClimateZone::classify(25.0, 0.30, 0.05), ClimateZone::Savanna);
        assert_eq!(
            ClimateZone::classify(25.0, 0.80, 0.05),
            ClimateZone::TropicalRainforest
        );
    }

    #[test]
    fn every_climate_zone_has_a_colour_and_a_name() {
        for zone in ClimateZone::ALL {
            assert!(!zone.label().is_empty());
            assert!(!zone.describe().is_empty());
            // No zone may be painted pure black, which would read as "no data".
            assert!(zone.color() != [0, 0, 0]);
        }
    }

    #[test]
    fn temperature_palette_spans_the_whole_scale() {
        assert_eq!(temperature_color(-40.0), TEMPERATURE_STOPS[0].1);
        assert_eq!(
            temperature_color(TEMPERATURE_MAX),
            TEMPERATURE_STOPS[TEMPERATURE_STOPS.len() - 1].1
        );
        // Cold must be bluer than hot, hot must be redder than cold.
        let cold = temperature_color(-30.0);
        let hot = temperature_color(35.0);
        assert!(cold[2] > cold[0], "cold end should be blue-dominant");
        assert!(hot[0] > hot[2], "hot end should be red-dominant");
    }

    #[test]
    fn isotherms_step_every_five_degrees() {
        assert_eq!(isotherm_band(-40.0), 0);
        assert_eq!(isotherm_band(-35.0), 1);
        assert_eq!(isotherm_band(-30.1), 1);
        assert_eq!(isotherm_band(-30.0), 2);
        assert_eq!(isotherm_band(40.0), ISOTHERM_BANDS - 1);
        assert_eq!(isotherm_band_label(2), "-30 … -25 °C");
    }

    #[test]
    fn terrain_classes_follow_the_vertical_datum() {
        assert_eq!(
            TerrainClass::classify(-5000.0, false, 12.0),
            TerrainClass::DeepOcean
        );
        assert_eq!(
            TerrainClass::classify(-1000.0, false, 12.0),
            TerrainClass::OceanFloor
        );
        assert_eq!(
            TerrainClass::classify(-100.0, false, 12.0),
            TerrainClass::Shelf
        );
        assert_eq!(
            TerrainClass::classify(-100.0, false, -10.0),
            TerrainClass::Ice
        );
        assert_eq!(
            TerrainClass::classify(10.0, true, 12.0),
            TerrainClass::Lake
        );
        assert_eq!(
            TerrainClass::classify(100.0, false, 12.0),
            TerrainClass::CoastalLowland
        );
        assert_eq!(
            TerrainClass::classify(3300.0, false, 2.0),
            TerrainClass::HighMountain
        );
    }

    #[test]
    fn every_terrain_class_has_a_readable_band() {
        for class in TerrainClass::ALL {
            assert!(!class.label().is_empty());
            assert!(!class.range_label().is_empty());
            assert!(class.color() != [0, 0, 0]);
        }
    }

    #[test]
    fn undefined_polar_temperatures_read_as_cold_ice_not_as_tropical_heat() {
        let nan = f32::NAN;
        assert_eq!(ClimateZone::classify(nan, 0.5, 0.05), ClimateZone::IceCap);
        assert_eq!(TerrainClass::classify(100.0, false, nan), TerrainClass::Ice);
        assert_eq!(temperature_color(nan), TEMPERATURE_STOPS[0].1);
        assert_eq!(isotherm_band(nan), 0);
        assert_eq!(ocean_climate_color(nan), [228, 238, 248]);
        assert_eq!(ocean_climate_label(nan), "Frozen sea");
    }

    #[test]
    fn ocean_climate_bands_get_warmer_but_stay_blue() {
        let colours = [0.0, 9.0, 19.0, 26.0, 30.0].map(ocean_climate_color);
        for c in colours {
            assert!(c[2] > c[0], "sea should stay blue-dominant");
        }
        // The warm bands are darker than the polar band, so the legend reads as a
        // depth of colour rather than a clash with the land palette.
        assert!(colours[4][0] < colours[1][0]);
    }

    #[test]
    fn slope_words_climb_with_the_gradient() {
        assert_eq!(slope_label(0.0), "gentle");
        assert_eq!(slope_label(0.02), "rolling");
        assert_eq!(slope_label(0.04), "steep");
        assert_eq!(slope_label(0.10), "cliff-like");
    }
}
