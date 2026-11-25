//! The **Advanced Configuration** section of the genesis panel.
//!
//! Every constant the world generator reads, in one place, in the order the acts
//! apply it: the planet's spin and its sea-level temperatures, the polar ice, the
//! continental field and its coastlines, the orogeny that folds them, the rains
//! that fall on them, the rivers that drain them, the provinces they are cut into,
//! the continents those are gathered into, and the dawn of history — how civilized
//! the world grows, how strongly societies and creeds consolidate, how fast realms
//! develop, and how many people they hold.
//!
//! Two rules shape this panel, and both are honest rather than convenient:
//!
//! * **A setting is read by an act when it runs.** The pipeline rebuilds each
//!   act's config from [`WorldGenSettings`] at the moment the act starts, exactly
//!   as it reads the seed, so a value typed here applies to the acts *not yet
//!   grown* and never rewrites a world already on the canvas. The section says so,
//!   and every act carries the ↺ that grows it again.
//! * **A default is the engine's own.** The ↺ beside each parameter — and the
//!   reset on each group — restores the constants the atlas was tuned at, so the
//!   panel always has a way back to the world it has always drawn. A group whose
//!   values have been moved off those constants says `edited` in its header, so a
//!   reader can see at a glance where the planet was changed.
//!
//! The section, and every act inside it, is collapsible: the panel is 372 px
//! wide and holds thirty sliders plus the acts, and a reader who is growing a
//! world wants the acts in front of them, not the weathering model. The sliders
//! are gathered under the act that reads them, so a reader changing the dawn of
//! history is not scrolling past the orogeny to find it.

use crate::core::fmt::thousands;
use crate::pipeline::{Stage, WorldGenSettings};
use eframe::egui::{self, RichText, SliderClamping};
use std::ops::RangeInclusive;

use super::i18n::Language;
use super::{TEXT_BODY, caption, scroll_bar_gutter};

/// The width a combo box gets in this panel: the panel is narrow, and the mode
/// has to stay readable without widening it.
const SETTING_SLIDER_WIDTH: f32 = 190.0;

/// Draw the whole Advanced Configuration section.
///
/// `defaults` is the tuned density of **this world's plate** — a fresh 2K world
/// starts at 469 provinces, an 8K at 7 500, a 16K at 30 000 — so the ↺ buttons
/// restore the world the reader's canvas has always drawn, not a number quoted
/// for a plate they are not on.
///
/// Returns `true` when the reader changed something, so the caller can hand the
/// new settings to the pipeline — which is the only place they are stored.
pub fn advanced_settings(
    ui: &mut egui::Ui,
    settings: &mut WorldGenSettings,
    defaults: &WorldGenSettings,
    busy: bool,
    language: Language,
) -> bool {
    draw_section(ui, settings, defaults, busy, None, language)
}

/// The section itself, with the fold-outs optionally forced open.
///
/// `force_open` exists for the tests: a headless frame has no pointer to click a
/// header with, and every slider in the panel should still be drawn — and
/// exercised — by one. The panel itself passes `None`, which is "remember what
/// the reader left".
fn draw_section(
    ui: &mut egui::Ui,
    settings: &mut WorldGenSettings,
    defaults: &WorldGenSettings,
    busy: bool,
    force_open: Option<bool>,
    language: Language,
) -> bool {
    let mut changed = false;
    let edited = *settings != *defaults;

    egui::CollapsingHeader::new(section_title(edited, language))
        // The badge is part of the label, so the id must not be: a group
        // becoming `edited` must not snap the section shut (the same rule the
        // saved-worlds list follows).
        .id_salt("advanced_configuration")
        .open(force_open)
        .default_open(false)
        .show(ui, |ui| {
            caption(
                ui,
                language.text(
                    "Every constant of the engine, in the order the acts apply it. A value applies to the acts not yet grown — press ↺ on an act to grow it again.",
                ),
            );
            caption(
                ui,
                language.text(
                    "These constants travel with the world: Save records them, and Load restores them.",
                ),
            );

            ui.horizontal(|ui| {
                let reset = ui
                    .add_enabled(
                        edited && !busy,
                        egui::Button::new(language.text("↺ Reset all")).small(),
                    )
                    .on_hover_text(language.text("Every constant back to the engine's default"))
                    .on_disabled_hover_text(if busy {
                        language.text("Wait for the act in flight to finish")
                    } else {
                        language.text("Everything is already at the engine's defaults")
                    });
                if reset.clicked() {
                    *settings = defaults.clone();
                    changed = true;
                }
                if busy {
                    caption(ui, language.text("· an act is running"));
                }
            });

            // An act running holds the pipeline on its own thread, so a drag here
            // would be thrown away when the world is handed back. The groups stay
            // readable — greyed — rather than lying about what they did.
            ui.add_enabled_ui(!busy, |ui| {
                for act in ActSection::ALL {
                    draw_act(
                        ui, act, settings, defaults, &mut changed, force_open, language,
                    );
                }
            });
        });

    changed
}

/// The section's header, with its badge when the planet has been changed.
fn section_title(edited: bool, language: Language) -> String {
    if edited {
        language
            .text("⚙ Advanced configuration  ·  edited")
            .to_string()
    } else {
        language.text("⚙ Advanced configuration").to_string()
    }
}

/// One act of creation, and the groups of constants it reads.
///
/// This is the panel's top level: a reader who wants the dawn of history opens
/// Act 7, not a flat list that begins with the planet's spin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ActSection {
    Geology,
    Climate,
    Hydrology,
    Provinces,
    Geopolitics,
    HydroPolitics,
    History,
}

impl ActSection {
    /// Every act, in the order the pipeline runs them.
    const ALL: [ActSection; 7] = [
        ActSection::Geology,
        ActSection::Climate,
        ActSection::Hydrology,
        ActSection::Provinces,
        ActSection::Geopolitics,
        ActSection::HydroPolitics,
        ActSection::History,
    ];

    /// The act whose title and reading order this section follows.
    fn stage(self) -> Stage {
        match self {
            ActSection::Geology => Stage::Geology,
            ActSection::Climate => Stage::Climate,
            ActSection::Hydrology => Stage::Hydrology,
            ActSection::Provinces => Stage::Provinces,
            ActSection::Geopolitics => Stage::Geopolitics,
            ActSection::HydroPolitics => Stage::HydroPolitics,
            ActSection::History => Stage::History,
        }
    }

    /// A stable id: the header carries a badge, so it cannot be the id.
    fn id(self) -> &'static str {
        match self {
            ActSection::Geology => "settings_act_1",
            ActSection::Climate => "settings_act_2",
            ActSection::Hydrology => "settings_act_3",
            ActSection::Provinces => "settings_act_4",
            ActSection::Geopolitics => "settings_act_5",
            ActSection::HydroPolitics => "settings_act_6",
            ActSection::History => "settings_act_7",
        }
    }

    /// The groups this act reads, in the order the specification lists them.
    fn groups(self) -> &'static [Group] {
        match self {
            ActSection::Geology => &[Group::Coastlines, Group::Orogeny, Group::Weathering],
            ActSection::Climate => &[Group::Planetary, Group::Climate],
            ActSection::Hydrology => &[Group::Polar, Group::Hydrology],
            ActSection::Provinces => &[Group::LandProvinces],
            ActSection::Geopolitics => &[Group::Geopolitics],
            ActSection::HydroPolitics => &[Group::OceanProvinces],
            ActSection::History => &[
                Group::HistorySocieties,
                Group::HistoryReligion,
                Group::HistoryDevelopment,
            ],
        }
    }

    /// True when any constant under this act has left the engine's default.
    fn edited(self, settings: &WorldGenSettings, defaults: &WorldGenSettings) -> bool {
        self.groups()
            .iter()
            .any(|group| group.edited(settings, defaults))
    }

    /// Put every group under this act back to the engine's defaults.
    fn reset(self, settings: &mut WorldGenSettings, defaults: &WorldGenSettings) {
        for group in self.groups() {
            group.reset(settings, defaults);
        }
    }
}

/// One group of the specification: a handful of constants an act reads together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    Coastlines,
    Orogeny,
    Weathering,
    Planetary,
    Climate,
    Polar,
    Hydrology,
    LandProvinces,
    Geopolitics,
    OceanProvinces,
    HistorySocieties,
    HistoryReligion,
    HistoryDevelopment,
}

impl Group {
    /// Every group, in the order the acts apply them.
    ///
    /// The panel walks the act sections themselves; this list is the test's
    /// guarantee that every group lives under exactly one of them.
    #[cfg(test)]
    const ALL: [Group; 13] = [
        Group::Coastlines,
        Group::Orogeny,
        Group::Weathering,
        Group::Planetary,
        Group::Climate,
        Group::Polar,
        Group::Hydrology,
        Group::LandProvinces,
        Group::Geopolitics,
        Group::OceanProvinces,
        Group::HistorySocieties,
        Group::HistoryReligion,
        Group::HistoryDevelopment,
    ];

    /// A stable id: the header carries a badge, so it cannot be the id.
    fn id(self) -> &'static str {
        match self {
            Group::Coastlines => "settings_coastlines",
            Group::Orogeny => "settings_orogeny",
            Group::Weathering => "settings_weathering",
            Group::Planetary => "settings_planetary",
            Group::Climate => "settings_climate",
            Group::Polar => "settings_polar",
            Group::Hydrology => "settings_hydrology",
            Group::LandProvinces => "settings_land_provinces",
            Group::Geopolitics => "settings_geopolitics",
            Group::OceanProvinces => "settings_ocean_provinces",
            Group::HistorySocieties => "settings_history_societies",
            Group::HistoryReligion => "settings_history_religion",
            Group::HistoryDevelopment => "settings_history_development",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Group::Coastlines => "🗺 Continental layout & coastlines",
            Group::Orogeny => "⛰ Orogeny & tectonics",
            Group::Weathering => "💧 Weathering & erosion",
            Group::Planetary => "🪐 Planetary & astronomical",
            Group::Climate => "🌧 Climate & atmospheric moisture",
            Group::Polar => "🧊 Polar glaciation",
            Group::Hydrology => "🏞 Hydrology & rivers",
            Group::LandProvinces => "🧭 Land provinces & wastelands",
            Group::Geopolitics => "👑 Geopolitical hierarchy",
            Group::OceanProvinces => "🌊 Marine provinces",
            Group::HistorySocieties => "🏺 Societies & states",
            Group::HistoryReligion => "🛐 Religion & faiths",
            Group::HistoryDevelopment => "🌱 Development & population",
        }
    }

    /// Which act reads this group, so a reader can see what they are about to
    /// change — and what they would have to grow again for it to show.
    fn blurb(self) -> &'static str {
        match self {
            Group::Coastlines => "Act 1 — the continental field and its water line",
            Group::Orogeny => "Act 1 — the faults that raise the ranges",
            Group::Weathering => "Act 1 — how the land is worn down after it rises",
            Group::Planetary => "Act 2 — the planet's spin and its sea-level temperatures",
            Group::Climate => "Act 2 — the moist atmosphere",
            Group::Polar => {
                "Act 3 — where the permanent ice lies, in degrees of latitude; act 1 reads \
                 the same belts when it cuts the coastlines."
            }
            Group::Hydrology => "Act 3 — what reaches the sea",
            Group::LandProvinces => {
                "Act 4 — the land cut into provinces, and the wilderness left between them"
            }
            Group::Geopolitics => "Act 5 — how many continents the world is allowed",
            Group::OceanProvinces => "Act 6 — the world ocean cut into marine provinces",
            Group::HistorySocieties => {
                "Act 7 — how many cradles rise, and how readily farming and chiefdoms spread"
            }
            Group::HistoryReligion => "Act 7 — how far local creeds fold into broader traditions",
            Group::HistoryDevelopment => {
                "Act 7 — how fast realms develop, and how many people they hold"
            }
        }
    }

    /// Put this group's constants back to the engine's defaults for the plate.
    fn reset(self, settings: &mut WorldGenSettings, defaults: &WorldGenSettings) {
        let d = defaults;
        match self {
            Group::Coastlines => {
                settings.sea_threshold = d.sea_threshold;
                settings.continental_freq = d.continental_freq;
                settings.continental_persistence = d.continental_persistence;
                settings.domain_warp_amp = d.domain_warp_amp;
            }
            Group::Orogeny => {
                settings.mountain_lift = d.mountain_lift;
                settings.fault_noise_freq = d.fault_noise_freq;
                settings.fault_band = d.fault_band;
            }
            Group::Weathering => {
                settings.erosion_droplets = d.erosion_droplets;
                settings.thermal_iterations = d.thermal_iterations;
            }
            Group::Planetary => {
                settings.tidally_locked = d.tidally_locked;
                settings.equator_temp = d.equator_temp;
                settings.pole_temp = d.pole_temp;
            }
            Group::Climate => {
                settings.base_rainfall = d.base_rainfall;
                settings.hadley_dry_depth = d.hadley_dry_depth;
                settings.inland_factor_floor = d.inland_factor_floor;
                settings.orographic_strength = d.orographic_strength;
            }
            Group::Polar => {
                settings.polar_cap_start_lat_deg = d.polar_cap_start_lat_deg;
                settings.polar_belt_start_lat_deg = d.polar_belt_start_lat_deg;
            }
            Group::Hydrology => {
                settings.runoff_coefficient = d.runoff_coefficient;
                settings.min_river_flow = d.min_river_flow;
            }
            Group::LandProvinces => {
                settings.num_provinces = d.num_provinces;
                settings.max_waste_share = d.max_waste_share;
                settings.waste_cutoff = d.waste_cutoff;
            }
            Group::Geopolitics => {
                settings.max_continents = d.max_continents;
            }
            Group::OceanProvinces => {
                settings.ocean_num_provinces = d.ocean_num_provinces;
            }
            Group::HistorySocieties => {
                settings.civilization_level = d.civilization_level;
                settings.society_consolidation = d.society_consolidation;
            }
            Group::HistoryReligion => {
                settings.religion_consolidation = d.religion_consolidation;
            }
            Group::HistoryDevelopment => {
                settings.development_pace = d.development_pace;
                settings.population_multiplier = d.population_multiplier;
            }
        }
    }

    /// True when any constant in this group has left the engine's default.
    ///
    /// Asked by putting the group back to its defaults on a *copy*: one
    /// comparison answers for the group, and there is no second list of fields
    /// to keep in step with `reset`.
    fn edited(self, settings: &WorldGenSettings, defaults: &WorldGenSettings) -> bool {
        let mut probe = settings.clone();
        self.reset(&mut probe, defaults);
        *settings != probe
    }
}

/// One act: its header, its groups, and the way back to the defaults.
fn draw_act(
    ui: &mut egui::Ui,
    act: ActSection,
    settings: &mut WorldGenSettings,
    defaults: &WorldGenSettings,
    changed: &mut bool,
    force_open: Option<bool>,
    language: Language,
) {
    let stage = act.stage();
    let title = format!("{}. {}", stage.index(), language.text(stage.title()));
    let header = if act.edited(settings, defaults) {
        language.fill("{}  ·  edited", &[&title])
    } else {
        title
    };

    egui::CollapsingHeader::new(header)
        .id_salt(act.id())
        .open(force_open)
        .default_open(false)
        .show(ui, |ui| {
            for group in act.groups() {
                draw_group(
                    ui, *group, settings, defaults, changed, force_open, language,
                );
            }

            if act.edited(settings, defaults) {
                ui.add_space(2.0);
                if ui
                    .add(egui::Button::new(language.text("↺ Reset this act's constants")).small())
                    .on_hover_text(
                        language.text("Every constant of this act back to the engine's default"),
                    )
                    .clicked()
                {
                    act.reset(settings, defaults);
                    *changed = true;
                }
            }
        });
}

/// One group: its header, its constants, and the way back to the defaults.
fn draw_group(
    ui: &mut egui::Ui,
    group: Group,
    settings: &mut WorldGenSettings,
    defaults: &WorldGenSettings,
    changed: &mut bool,
    force_open: Option<bool>,
    language: Language,
) {
    let edited = group.edited(settings, defaults);
    let title = language.text(group.title());
    let header = if edited {
        language.fill("{}  ·  edited", &[&title])
    } else {
        title.to_string()
    };

    egui::CollapsingHeader::new(header)
        .id_salt(group.id())
        .open(force_open)
        .default_open(false)
        .show(ui, |ui| {
            caption(ui, language.text(group.blurb()));
            draw_constants(ui, group, settings, changed, defaults, language);

            // The way back, offered only where there is something to go back
            // from — a reset button on an untouched group is a dead control.
            if group.edited(settings, defaults) {
                ui.add_space(2.0);
                if ui
                    .add(
                        egui::Button::new(language.text("↺ Reset these to the engine's defaults"))
                            .small(),
                    )
                    .clicked()
                {
                    group.reset(settings, defaults);
                    *changed = true;
                }
            }
        });
}

/// The constants of one group, drawn in the order the spec lists them.
fn draw_constants(
    ui: &mut egui::Ui,
    group: Group,
    settings: &mut WorldGenSettings,
    changed: &mut bool,
    d: &WorldGenSettings,
    language: Language,
) {
    match group {
        Group::Planetary => {
            mode_setting(ui, changed, settings, language);
            float_setting(
                ui,
                changed,
                language.text("Equatorial temperature"),
                language.text("Sea-level surface temperature at the equator. Higher values expand the tropical belt; lower values begin a global cooling."),
                &format!("{:.0} °C", d.equator_temp),
                &mut settings.equator_temp,
                d.equator_temp,
                10.0..=55.0,
                1.0,
                " °C",
                0,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Polar temperature"),
                language.text("Sea-level surface temperature at the poles. Above 0 °C the world is a greenhouse without permanent ice."),
                &format!("{:.0} °C", d.pole_temp),
                &mut settings.pole_temp,
                d.pole_temp,
                -60.0..=15.0,
                1.0,
                " °C",
                0,
                language,
            );
        }

        Group::Polar => {
            float_setting(
                ui,
                changed,
                language.text("Land ice cap edge"),
                language.text("The latitude where the thick continental ice caps begin to form. Lower it for an ice age; raise it to keep the glaciers at the poles. The sheet reaches solid ice 0.18 rad (≈ 10°) further poleward."),
                &format!("{:.1}°", d.polar_cap_start_lat_deg),
                &mut settings.polar_cap_start_lat_deg,
                d.polar_cap_start_lat_deg,
                40.0..=88.0,
                0.5,
                "°",
                1,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Permanent sea ice edge"),
                language.text("The latitude where permanent pack ice forms over the ocean. The sea freezes slightly equatorward of the land ice, so this edge is held at or below the ice cap's."),
                &format!("{:.1}°", d.polar_belt_start_lat_deg),
                &mut settings.polar_belt_start_lat_deg,
                d.polar_belt_start_lat_deg,
                35.0..=85.0,
                0.5,
                "°",
                1,
                language,
            );
            if settings.polar_belt_start_lat_deg > settings.polar_cap_start_lat_deg {
                warning(
                    ui,
                    language.text("The sea-ice edge is poleward of the ice cap: the sea freezes first, so the belt is pulled back to the cap when the act runs."),
                );
            }
        }

        Group::Coastlines => {
            float_setting(
                ui,
                changed,
                language.text("Sea level / ocean balance"),
                language.text("Shifts the continental datum against the water line: 0.02 drains the ocean basins to reveal land bridges and inland seas, 0.11 is Earth, 0.22 drowns the continents into archipelagos."),
                &format!("{:.3}", d.sea_threshold),
                &mut settings.sea_threshold,
                d.sea_threshold,
                0.02..=0.22,
                0.005,
                "",
                3,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Landmass scale"),
                language.text("Frequency of the continental field: 0.40 aggregates the crust into one or two supercontinents, 1.05 is Earth's continents, 4.50 shatters the land into hundreds of islands."),
                &format!("{:.2}", d.continental_freq),
                &mut settings.continental_freq,
                d.continental_freq,
                0.40..=4.50,
                0.05,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Coastline roughness"),
                language.text("Fractal persistence across the continental field's octaves. Low values give smooth, gentle shorelines; high values craggy, rugged coasts with abundant micro-islands."),
                &format!("{:.2}", d.continental_persistence),
                &mut settings.continental_persistence,
                d.continental_persistence,
                0.30..=0.70,
                0.01,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Coastline distortion / fjords"),
                language.text("Strength of the coordinate warp on the crust. At 0.00 the landmasses are regular and rounded; at high values coastlines fold into deep fjords, sweeping bays and chaotic peninsulas."),
                &format!("{:.2}", d.domain_warp_amp),
                &mut settings.domain_warp_amp,
                d.domain_warp_amp,
                0.00..=0.75,
                0.01,
                "",
                2,
                language,
            );
        }

        Group::Orogeny => {
            float_setting(
                ui,
                changed,
                language.text("Mountain uplift height"),
                language.text("Maximum vertical uplift along a fault line. Low values leave weathered, low-relief shield plains; high values thrust up towering, impassable Alpine ridges."),
                &format!("{:.2}", d.mountain_lift),
                &mut settings.mountain_lift,
                d.mountain_lift,
                0.10..=0.85,
                0.01,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Mountain chain spacing"),
                language.text("Density of the tectonic fault lines across the globe. Low values yield a few isolated mountain belts; high values a dense, criss-crossed web of ranges."),
                &format!("{:.2}", d.fault_noise_freq),
                &mut settings.fault_noise_freq,
                d.fault_noise_freq,
                0.60..=3.50,
                0.05,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Mountain belt width"),
                language.text("Thickness of the orogenic collision zone. Small values draw sharp, narrow mountain spines; large values build broad, sprawling highland plateaus."),
                &format!("{:.3}", d.fault_band),
                &mut settings.fault_band,
                d.fault_band,
                0.03..=0.25,
                0.005,
                "",
                3,
                language,
            );
        }

        Group::Weathering => {
            count_setting(
                ui,
                changed,
                language.text("Hydraulic erosion droplets"),
                language.text("Number of simulated raindrops running downhill. Low values leave freshly uplifted, jagged crags; high values carve deep canyons, sediment basins and mature dendritic valleys. This is the dominant cost of act 1."),
                &thousands(d.erosion_droplets as f32),
                &mut settings.erosion_droplets,
                d.erosion_droplets,
                50_000..=7_000_000,
                50_000,
                true,
                language,
            );
            count_setting(
                ui,
                changed,
                language.text("Thermal weathering passes"),
                language.text("Iterations of talus slumping downhill. At 0 the cliffs stay near-vertical and jagged; higher values soften them into stable, natural scree slopes."),
                &d.thermal_iterations.to_string(),
                &mut settings.thermal_iterations,
                d.thermal_iterations,
                0..=10,
                1,
                false,
                language,
            );
        }

        Group::Climate => {
            float_setting(
                ui,
                changed,
                language.text("Global baseline rainfall"),
                language.text("Baseline precipitation before latitude belts, rain shadows and distance from the sea are accounted for. Sets how lush or dry the planet is."),
                &format!("{:.2}", d.base_rainfall),
                &mut settings.base_rainfall,
                d.base_rainfall,
                0.10..=0.90,
                0.01,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Subtropical desert severity"),
                language.text("How deeply the descending limb of the Hadley cell dries the subtropics (around ±26°). At 0.00 subtropical deserts disappear; high values girdle the globe in Sahara-like belts."),
                &format!("{:.2}", d.hadley_dry_depth),
                &mut settings.hadley_dry_depth,
                d.hadley_dry_depth,
                0.00..=0.85,
                0.02,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Interior aridity floor"),
                language.text("Share of oceanic moisture retained in the deepest continental interior. Low values dry out supercontinent hearts into steppes and salt flats; high values let the sea's rain reach far inland."),
                &format!("{:.2}", d.inland_factor_floor),
                &mut settings.inland_factor_floor,
                d.inland_factor_floor,
                0.05..=0.85,
                0.01,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Rain shadow intensity"),
                language.text("Multiplier on mountain-induced precipitation. High values create heavy deluges on the windward slopes and severe rain-shadow deserts on the lee."),
                &format!("{:.1}×", d.orographic_strength),
                &mut settings.orographic_strength,
                d.orographic_strength,
                0.0..=30.0,
                0.5,
                "×",
                1,
                language,
            );
        }

        Group::Hydrology => {
            float_setting(
                ui,
                changed,
                language.text("Surface runoff fraction"),
                language.text("Share of rainfall that becomes surface river discharge rather than soaking away or evaporating. Higher values mean fuller, wider rivers."),
                &format!("{:.2}", d.runoff_coefficient),
                &mut settings.runoff_coefficient,
                d.runoff_coefficient,
                0.05..=0.75,
                0.01,
                "",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("River network cutoff"),
                language.text("Smallest discharge, in m³/s, worth drawing as a river. Lower values reveal fine capillary tributaries; higher values display only the great arterial rivers."),
                &format!("{:.0} m³/s", d.min_river_flow),
                &mut settings.min_river_flow,
                d.min_river_flow,
                10.0..=1_500.0,
                10.0,
                " m³/s",
                0,
                language,
            );
        }

        Group::LandProvinces => {
            count_setting(
                ui,
                changed,
                language.text("Land provinces"),
                language.text("Land provinces across the planet. Each world draws its own exact count from this mean (within 7.5 %); the area, region and superregion tiers follow the drawn count (about one area per five provinces, one region per fifty and one superregion per two hundred)."),
                &thousands(d.num_provinces as f32),
                &mut settings.num_provinces,
                d.num_provinces,
                250..=60_000,
                250,
                false,
                language,
            );
            share_setting(
                ui,
                changed,
                language.text("Impassable wilderness cap"),
                language.text("Most of the ice-free land that may become impassable wasteland — the deep desert hearts, the glaciated crags, the interior jungle. Set it to 0% for a completely settled planet. Polar ice is not counted: it is geography, not a choice."),
                &mut settings.max_waste_share,
                d.max_waste_share,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Wasteland hostility cutoff"),
                language.text("Environmental hostility above which a province is abandoned to the wild. Lowering it makes the planet harsher and harder to colonise."),
                &format!("{:.2}", d.waste_cutoff),
                &mut settings.waste_cutoff,
                d.waste_cutoff,
                0.40..=0.90,
                0.01,
                "",
                2,
                language,
            );
        }

        Group::OceanProvinces => {
            count_setting(
                ui,
                changed,
                language.text("Marine provinces"),
                language.text(
                    "Marine provinces across the world ocean. Each world draws its own exact count from this mean (within 7.5 %); the ocean's basins, superregions, regions and areas follow it.",
                ),
                &thousands(d.ocean_num_provinces as f32),
                &mut settings.ocean_num_provinces,
                d.ocean_num_provinces,
                25..=8_000,
                25,
                false,
                language,
            );
        }

        Group::Geopolitics => {
            count_setting(
                ui,
                changed,
                language.text("Max continents"),
                language.text("Ceiling on the major continental cores found by the topological clustering. 1 gives a single Pangaean hierarchy, 7 an Earth-like fractured world. Landmasses beyond the ceiling are anchored to the nearest core, exactly as an island is."),
                &d.max_continents.to_string(),
                &mut settings.max_continents,
                d.max_continents,
                1..=16,
                1,
                false,
                language,
            );
        }

        Group::HistorySocieties => {
            float_setting(
                ui,
                changed,
                language.text("Civilization level"),
                language.text("Scales the civilization the world grows: higher values draw more cradles and let farming and chiefdoms reach farther from them; lower values leave more of the map to foragers and herders."),
                &format!("×{:.2}", d.civilization_level),
                &mut settings.civilization_level,
                d.civilization_level,
                0.25..=3.00,
                0.05,
                "×",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Society consolidation"),
                language.text("How strongly neighbouring peoples of one kind merge into a single society. Higher values leave fewer, larger forager bands, herding peoples, farming villages and chiefdoms; lower values keep more small societies standing. States are never merged."),
                &format!("×{:.2}", d.society_consolidation),
                &mut settings.society_consolidation,
                d.society_consolidation,
                0.25..=3.00,
                0.05,
                "×",
                2,
                language,
            );
        }

        Group::HistoryReligion => {
            float_setting(
                ui,
                changed,
                language.text("Religion consolidation"),
                language.text("How strongly local creeds fold into broader traditions. Higher values lower every society's and superregion's faith ceiling, raise the bar for a schism, make new hearths, diasporas and residual faiths rarer, and dissolve the smallest traditions into the neighbour they share the longest border with — fewer, larger religions, each one unbroken ground; lower values do the opposite."),
                &format!("×{:.2}", d.religion_consolidation),
                &mut settings.religion_consolidation,
                d.religion_consolidation,
                0.50..=10.00,
                0.10,
                "×",
                2,
                language,
            );
        }

        Group::HistoryDevelopment => {
            float_setting(
                ui,
                changed,
                language.text("Development pace"),
                language.text("Multiplies how fast every state climbs the development axis after its founding. Higher values reach a later age with more developed governments and denser populations; lower values keep the world nearer its first cities."),
                &format!("×{:.2}", d.development_pace),
                &mut settings.development_pace,
                d.development_pace,
                0.50..=2.00,
                0.05,
                "×",
                2,
                language,
            );
            float_setting(
                ui,
                changed,
                language.text("Population multiplier"),
                language.text("Scales every province's carrying capacity and the density at which a province counts as urban, so the world's peoples grow or shrink together without changing which provinces are cities. ×1.00 is the engine's own."),
                &format!("×{:.2}", d.population_multiplier),
                &mut settings.population_multiplier,
                d.population_multiplier,
                0.50..=2.00,
                0.05,
                "×",
                2,
                language,
            );
        }
    }
}

// ─── THE WIDGETS ─────────────────────────────────────────────────────────────
//
// Every parameter is drawn the same way, because every parameter is read the
// same way: a name with its way back to the default on the right, then the
// widget itself, then the sentence the spec gives for it as a hover card. The
// one thing that differs is the shape of the value — a fraction, a count, a
// latitude, a rate — and that is what the three helpers below choose between.
//
// Every slider is clamped on *edits* rather than on every frame. A slider's
// default track snaps its value onto the step grid as soon as the widget is
// drawn, and the engine's defaults are not all on the panel's grid: the land ice
// cap, for one, begins at 69.9008°, which a 0.5° slider would quietly round to
// 70° — a world changed by being looked at, and a `edited` badge on a panel
// nobody had touched. Clamping on the edit keeps the default exact, and a drag
// still lands on the grid.

/// One floating-point parameter: a slider between the spec's safe bounds.
///
/// Generic over the value's precision, because the panel's constants are not all
/// `f32`: the fBm shape of the continental field is sampled in `f64`, and its
/// default has to arrive at the noise field exactly as the engine wrote it.
#[allow(clippy::too_many_arguments)]
fn float_setting<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    changed: &mut bool,
    name: &str,
    hover: &str,
    default_text: &str,
    value: &mut Num,
    default: Num,
    range: RangeInclusive<Num>,
    step: f64,
    suffix: &str,
    decimals: usize,
    language: Language,
) {
    if parameter_name(ui, name, *value == default, default_text, language).clicked() {
        *value = default;
        *changed = true;
    }
    let response = ui
        .add(
            egui::Slider::new(value, range)
                .step_by(step)
                .max_decimals(decimals)
                .min_decimals(0)
                .suffix(suffix)
                .trailing_fill(true)
                .clamping(SliderClamping::Edits),
        )
        .on_hover_text(hover_card(hover, default_text, language));
    if response.changed() {
        *changed = true;
    }
    ui.add_space(6.0);
}

/// One integer parameter: a slider that counts, with its thousands marked.
///
/// The erosion budget spans two orders of magnitude, so it is offered on a
/// logarithmic track: the difference between 50 000 and 500 000 droplets matters
/// as much as the difference between 5 million and 7 million, and a linear
/// slider would spend almost all of its length in the last of those.
#[allow(clippy::too_many_arguments)]
fn count_setting(
    ui: &mut egui::Ui,
    changed: &mut bool,
    name: &str,
    hover: &str,
    default_text: &str,
    value: &mut usize,
    default: usize,
    range: RangeInclusive<usize>,
    step: usize,
    logarithmic: bool,
    language: Language,
) {
    if parameter_name(ui, name, *value == default, default_text, language).clicked() {
        *value = default;
        *changed = true;
    }
    let response = ui
        .add(
            egui::Slider::new(value, range)
                .step_by(step as f64)
                .logarithmic(logarithmic)
                .clamping(SliderClamping::Edits)
                .custom_formatter(|n, _| thousands(n as f32)),
        )
        .on_hover_text(hover_card(hover, default_text, language));
    if response.changed() {
        *changed = true;
    }
    ui.add_space(6.0);
}

/// One share of the land, written the way a reader thinks of it: `18%`, not
/// `0.18`.
fn share_setting(
    ui: &mut egui::Ui,
    changed: &mut bool,
    name: &str,
    hover: &str,
    value: &mut f64,
    default: f64,
    language: Language,
) {
    let default_text = format!("{:.0}%", default * 100.0);
    if parameter_name(ui, name, *value == default, &default_text, language).clicked() {
        *value = default;
        *changed = true;
    }
    let response = ui
        .add(
            egui::Slider::new(value, 0.0..=0.50)
                .step_by(0.01)
                .clamping(SliderClamping::Edits)
                .custom_formatter(|n, _| format!("{:.0}%", n * 100.0)),
        )
        .on_hover_text(hover_card(hover, &default_text, language));
    if response.changed() {
        *changed = true;
    }
    ui.add_space(6.0);
}

/// The planetary mode: a rotating globe, or an eyeball world locked to its sun.
fn mode_setting(
    ui: &mut egui::Ui,
    changed: &mut bool,
    settings: &mut WorldGenSettings,
    language: Language,
) {
    let default_text = language.text("Rotating");
    if parameter_name(
        ui,
        language.text("Planetary rotation mode"),
        !settings.tidally_locked,
        default_text,
        language,
    )
    .clicked()
    {
        settings.tidally_locked = false;
        *changed = true;
    }
    let hover = hover_card(
        language.text("Controls the planetary dynamic: a conventional rotating globe with Hadley/Ferrel circulation, or an eyeball world tidally locked to its sun, with a permanent day face and a permanent night face."),
        default_text,
        language,
    );
    let selected = if settings.tidally_locked {
        language.text("Tidally locked (eyeball)")
    } else {
        language.text("Rotating (day & night)")
    };
    egui::ComboBox::from_id_salt("planetary_mode")
        .selected_text(selected)
        .width(SETTING_SLIDER_WIDTH)
        .show_ui(ui, |ui| {
            if ui
                .selectable_value(
                    &mut settings.tidally_locked,
                    false,
                    language.text("Rotating (day & night)"),
                )
                .changed()
            {
                *changed = true;
            }
            if ui
                .selectable_value(
                    &mut settings.tidally_locked,
                    true,
                    language.text("Tidally locked (eyeball)"),
                )
                .changed()
            {
                *changed = true;
            }
        })
        .response
        .on_hover_text(hover);
    ui.add_space(6.0);
}

/// The name of a parameter, with the ↺ that puts it back — on the right, where
/// the eye already goes for a control that belongs to the row.
///
/// Returns the reset button's response, so the caller can tell whether the
/// reader asked for the default back — and so a test can see where the way back
/// was actually drawn. A parameter that *is* the default keeps its arrow,
/// greyed: a control that vanishes is a control a reader has to hunt for.
fn parameter_name(
    ui: &mut egui::Ui,
    name: &str,
    at_default: bool,
    default_text: &str,
    language: Language,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(TEXT_BODY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // In a right-to-left layout the first item added is the rightmost,
            // so this holds the band egui's floating scroll bar paints over the
            // panel's edge clear of the arrow: a ↺ flush with that edge would
            // lie under the bar and never see a click — the bar would take the
            // press and page the panel instead.
            ui.add_space(scroll_bar_gutter(ui));
            ui.add_enabled(!at_default, egui::Button::new("↺").frame(false))
                .on_hover_text(language.fill("Back to the engine's default ({})", &[&default_text]))
                .on_disabled_hover_text(
                    language.fill("Already the engine's default ({})", &[&default_text]),
                )
        })
        .inner
    })
    .inner
}

/// A parameter's hover card: the spec's sentence, and the default it returns to.
fn hover_card(hover: &str, default_text: &str, language: Language) -> String {
    language.fill("{}\n\nEngine default: {}", &[&hover, &default_text])
}

/// A caution under the parameter that raised it, in the same amber the panel
/// uses for a world cache that could not be read.
fn warning(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(format!("⚠ {text}"))
            .size(super::TEXT_SMALL)
            .color(egui::Color32::from_rgb(224, 156, 96)),
    );
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every constant, paired with the group that owns it — one nudge each.
    ///
    /// This is the guard that keeps the lists in this file honest: the group that
    /// `reset`s a constant must also `edited`-notice it, and a constant added to
    /// `WorldGenSettings` but forgotten here is a constant the reader could
    /// change and never reset.
    type Nudge = Box<dyn Fn(&mut WorldGenSettings)>;
    fn nudges() -> Vec<(Group, &'static str, Nudge)> {
        vec![
            (
                Group::Planetary,
                "tidally_locked",
                Box::new(|s| s.tidally_locked = true),
            ),
            (
                Group::Planetary,
                "equator_temp",
                Box::new(|s| s.equator_temp = 10.0),
            ),
            (
                Group::Planetary,
                "pole_temp",
                Box::new(|s| s.pole_temp = 15.0),
            ),
            (
                Group::Polar,
                "polar_cap_start_lat_deg",
                Box::new(|s| s.polar_cap_start_lat_deg = 40.0),
            ),
            (
                Group::Polar,
                "polar_belt_start_lat_deg",
                Box::new(|s| s.polar_belt_start_lat_deg = 35.0),
            ),
            (
                Group::Coastlines,
                "sea_threshold",
                Box::new(|s| s.sea_threshold = 0.22),
            ),
            (
                Group::Coastlines,
                "continental_freq",
                Box::new(|s| s.continental_freq = 4.50),
            ),
            (
                Group::Coastlines,
                "continental_persistence",
                Box::new(|s| s.continental_persistence = 0.70),
            ),
            (
                Group::Coastlines,
                "domain_warp_amp",
                Box::new(|s| s.domain_warp_amp = 0.75),
            ),
            (
                Group::Orogeny,
                "mountain_lift",
                Box::new(|s| s.mountain_lift = 0.85),
            ),
            (
                Group::Orogeny,
                "fault_noise_freq",
                Box::new(|s| s.fault_noise_freq = 3.50),
            ),
            (
                Group::Orogeny,
                "fault_band",
                Box::new(|s| s.fault_band = 0.25),
            ),
            (
                Group::Weathering,
                "erosion_droplets",
                Box::new(|s| s.erosion_droplets = 50_000),
            ),
            (
                Group::Weathering,
                "thermal_iterations",
                Box::new(|s| s.thermal_iterations = 10),
            ),
            (
                Group::Climate,
                "base_rainfall",
                Box::new(|s| s.base_rainfall = 0.90),
            ),
            (
                Group::Climate,
                "hadley_dry_depth",
                Box::new(|s| s.hadley_dry_depth = 0.0),
            ),
            (
                Group::Climate,
                "inland_factor_floor",
                Box::new(|s| s.inland_factor_floor = 0.85),
            ),
            (
                Group::Climate,
                "orographic_strength",
                Box::new(|s| s.orographic_strength = 30.0),
            ),
            (
                Group::Hydrology,
                "runoff_coefficient",
                Box::new(|s| s.runoff_coefficient = 0.75),
            ),
            (
                Group::Hydrology,
                "min_river_flow",
                Box::new(|s| s.min_river_flow = 1_500.0),
            ),
            (
                Group::LandProvinces,
                "num_provinces",
                Box::new(|s| s.num_provinces = 5_000),
            ),
            (
                Group::OceanProvinces,
                "ocean_num_provinces",
                Box::new(|s| s.ocean_num_provinces = 8_000),
            ),
            (
                Group::LandProvinces,
                "max_waste_share",
                Box::new(|s| s.max_waste_share = 0.0),
            ),
            (
                Group::LandProvinces,
                "waste_cutoff",
                Box::new(|s| s.waste_cutoff = 0.90),
            ),
            (
                Group::Geopolitics,
                "max_continents",
                Box::new(|s| s.max_continents = 1),
            ),
            (
                Group::HistorySocieties,
                "civilization_level",
                Box::new(|s| s.civilization_level = 2.0),
            ),
            (
                Group::HistorySocieties,
                "society_consolidation",
                Box::new(|s| s.society_consolidation = 2.0),
            ),
            (
                Group::HistoryReligion,
                "religion_consolidation",
                Box::new(|s| s.religion_consolidation = 2.0),
            ),
            (
                Group::HistoryDevelopment,
                "development_pace",
                Box::new(|s| s.development_pace = 2.0),
            ),
            (
                Group::HistoryDevelopment,
                "population_multiplier",
                Box::new(|s| s.population_multiplier = 1.05),
            ),
        ]
    }

    /// A group notices the constant it owns, and its reset is what puts it back:
    /// every constant belongs to exactly one group, and the way home is exact.
    #[test]
    fn every_constant_belongs_to_a_group_that_notices_and_resets_it() {
        let default = WorldGenSettings::default();
        for (group, name, nudge) in nudges() {
            let mut settings = default.clone();
            nudge(&mut settings);
            assert_ne!(settings, default, "{name} was not actually moved");
            assert!(
                group.edited(&settings, &default),
                "{} must notice {name}",
                group.title()
            );

            // The group's own reset must restore it — and restore *only* what it
            // owns, so no constant is left to another group's button.
            group.reset(&mut settings, &default);
            assert_eq!(settings, default, "{} must reset {name}", group.title());
        }
    }

    /// The panel walks thirteen groups, and between them they hold the
    /// specification's thirty constants — each of them once.
    #[test]
    fn the_panel_walks_every_group_and_every_constant() {
        assert_eq!(Group::ALL.len(), 13);
        let mut counted: Vec<&str> = Vec::new();
        for group in Group::ALL {
            let owned: Vec<&str> = nudges()
                .into_iter()
                .filter(|(owner, _, _)| *owner == group)
                .map(|(_, name, _)| name)
                .collect();
            assert!(!owned.is_empty(), "{} owns no constant", group.title());
            counted.extend(owned);
        }
        assert_eq!(
            counted.len(),
            30,
            "the specification lists thirty constants"
        );
    }

    /// Every group lives under exactly one act, in the same order the panel
    /// walks them. A group that fell out of an act would be a control a reader
    /// could not reach.
    #[test]
    fn every_group_lives_under_exactly_one_act() {
        let mut seen: Vec<Group> = Vec::new();
        for act in ActSection::ALL {
            assert!(!act.groups().is_empty(), "{act:?} has no groups");
            seen.extend_from_slice(act.groups());
        }
        assert_eq!(seen, Group::ALL.to_vec());
    }

    /// The whole section must draw in a frame — every header, every slider, every
    /// hover card — with no window and no pointer, and leave the settings it was
    /// given exactly as it found them.
    #[test]
    fn the_whole_section_draws_headlessly_without_touching_the_settings() {
        let mut settings = WorldGenSettings::default();
        let changed = draw_frames(&mut settings, false);
        assert!(!changed, "drawing an untouched panel changes nothing");
        assert_eq!(
            settings,
            WorldGenSettings::default(),
            "and leaves every constant at its default"
        );
    }

    /// While an act is in flight the constants are read-only: the worker holds the
    /// pipeline, so a drag here could not reach it.
    #[test]
    fn a_running_act_leaves_the_constants_read_only() {
        let mut settings = WorldGenSettings::default();
        let changed = draw_frames(&mut settings, true);
        assert!(!changed, "a disabled panel cannot change a constant");
        assert_eq!(settings, WorldGenSettings::default());
    }

    /// A world grown from the panel's untouched defaults must be the world the
    /// engine has always drawn: the settings' own polar geography is the engine's
    /// constants, not a degree round-trip of them.
    #[test]
    fn the_untouched_panel_paints_the_engine_s_world() {
        let polar = WorldGenSettings::default().polar_config();
        assert_eq!(
            (polar.cap_start_lat, polar.cap_full_lat),
            (
                crate::core::polar::POLAR_CAP_START_LAT,
                crate::core::polar::POLAR_CAP_FULL_LAT
            ),
            "the land ice cap must not move by being put in a slider"
        );
        assert_eq!(
            (polar.belt_start_lat, polar.belt_full_lat),
            (
                crate::core::polar::POLAR_BELT_START_LAT,
                crate::core::polar::POLAR_BELT_FULL_LAT
            ),
            "and neither must the belt of sea ice"
        );
    }

    /// Draw the open section for a few frames headlessly, and report whether the
    /// *last* of them reported a change.
    ///
    /// The first frame of a fresh context is the one that builds the font atlas
    /// and every widget's id, so it is not the frame a reader sees; the answer is
    /// taken from the last frame, which is.
    fn draw_frames(settings: &mut WorldGenSettings, busy: bool) -> bool {
        let ctx = egui::Context::default();
        let defaults = WorldGenSettings::default();
        let mut changed = false;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                changed =
                    draw_section(ui, settings, &defaults, busy, Some(true), Language::English);
            });
            // Nothing is painting a headless frame, so the font atlas it built has
            // nowhere to go — but egui insists it be acknowledged rather than
            // dropped.
            output.textures_delta.clear();
        }
        changed
    }

    /// An open section must never paint past the genesis panel.
    ///
    /// The panel is `GENESIS_PANEL_WIDTH` wide and the map keeps everything else:
    /// a widget that asks for more than the panel offers widens the panel's frame,
    /// and a panel that grows takes the canvas with it. Twenty-five sliders and a
    /// combo box are exactly the kind of content that does it — and a translation
    /// is longer than its English original, so every language is measured.
    #[test]
    fn an_open_section_never_paints_past_its_panel() {
        for language in Language::ALL {
            let (ink, panel) = section_bounds(language);
            assert!(
                ink.max.x <= panel.max.x + 1.0,
                "the {language:?} section paints to x = {:.1} but the panel ends at \
                 x = {:.1}: the map would lose the width it was promised",
                ink.max.x,
                panel.max.x,
            );
            assert!(ink.width() > 0.0, "{language:?} painted nothing at all");
        }
    }

    /// The ink and the panel rectangle of one language's open section, taken from
    /// the last of three headless frames.
    fn section_bounds(language: Language) -> (egui::Rect, egui::Rect) {
        use super::super::{GENESIS_PANEL_WIDTH, atlas_panel_frame, configure_style};

        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));
        let mut panel_rect = egui::Rect::NOTHING;
        let mut ink = egui::Rect::NOTHING;

        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut settings = WorldGenSettings::default();
            let defaults = WorldGenSettings::default();

            let mut output = ctx.run_ui(input, |ui| {
                let panel = egui::Panel::left("settings_width_probe")
                    .frame(atlas_panel_frame(ui.style()))
                    .resizable(false)
                    .exact_size(GENESIS_PANEL_WIDTH)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            draw_section(ui, &mut settings, &defaults, false, Some(true), language);
                        });
                    });
                panel_rect = panel.response.rect;
            });

            // The shape's own extent, deliberately not its clip: overflowing is
            // exactly what is in question.
            ink = output
                .shapes
                .iter()
                .fold(egui::Rect::NOTHING, |bounds, clipped| {
                    bounds.union(clipped.shape.visual_bounding_rect())
                });
            output.textures_delta.clear();
        }

        (ink, panel_rect)
    }

    /// The ↺ that puts a constant back must never lie under the panel's scroll
    /// bar.
    ///
    /// egui's floating scroll bar is painted *over* the right edge of a scroll
    /// area and senses every press in its `bar_width` band: a way back drawn
    /// flush with that edge was dead — the bar took the press, expanded under
    /// the pointer and paged the panel, and the default was never restored. The
    /// row holds that band clear of the arrow, so the test draws one row in a
    /// scroll area that reserves nothing itself, and asks where the arrow landed.
    #[test]
    fn the_way_back_never_lies_under_the_scroll_bar() {
        use super::super::{GENESIS_PANEL_WIDTH, atlas_panel_frame, configure_style};

        let ctx = egui::Context::default();
        ctx.all_styles_mut(configure_style);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1720.0, 1040.0));

        let mut reset = egui::Rect::NOTHING;
        let mut content_right = 0.0;
        let mut bar_width = 0.0;

        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.time = Some(1.0 + f64::from(frame));
            let mut output = ctx.run_ui(input, |ui| {
                egui::Panel::left("settings_reset_probe")
                    .frame(atlas_panel_frame(ui.style()))
                    .resizable(false)
                    .exact_size(GENESIS_PANEL_WIDTH)
                    .show(ui, |ui| {
                        bar_width = ui.spacing().scroll.bar_width;
                        // Deliberately no content margin: this is the row's own
                        // clearance being measured, not the panel's.
                        egui::ScrollArea::vertical()
                            .id_salt("settings_reset_scroll")
                            .show(ui, |ui| {
                                content_right = ui.max_rect().right();
                                reset = parameter_name(
                                    ui,
                                    "Equatorial temperature",
                                    false,
                                    "32 °C",
                                    Language::English,
                                )
                                .rect;
                            });
                    });
            });
            output.textures_delta.clear();
        }

        assert!(reset.width() > 0.0, "the way back was never laid out");
        assert!(
            reset.right() <= content_right - bar_width,
            "the way back ends at x = {:.1}, inside the {bar_width:.0} px the scroll bar \
             claims at the right edge of the content (x = {:.1}..{content_right:.1})",
            reset.right(),
            content_right - bar_width,
        );
    }
}
