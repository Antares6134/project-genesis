use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*;
use crate::core::{Grid, WorldCache};
use crate::core::polar::{frozen_sea_name, is_frozen_sea, PACK_ICE_TEMP};
use crate::geography::{OceanResult, ProvinceResult};
use crate::geopolitics::GeopoliticalHierarchy;
use crate::export::image::{get_atlas_biome_color, get_biome_name, id_to_territory_color};

/// What political domain the atlas is currently portraying.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum MapScope {
    /// Pure physical relief — no political tinting at all.
    Physical,
    /// Political subdivision of the landmasses only.
    Land,
    /// Hydro-political subdivision of the world ocean only.
    Water,
    /// Both land and ocean political subdivisions at once.
    Everywhere,
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
    scope: MapScope,
    tier: MapTier,
    texture: Option<TextureHandle>,
    zoom: f32,
    offset: egui::Vec2,
    show_borders: bool,
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

impl MapViewer {
    pub fn new(
        cache: WorldCache,
        hierarchy: GeopoliticalHierarchy,
        ocean_hierarchy: GeopoliticalHierarchy,
        prov_res: ProvinceResult,
        ocean_res: OceanResult,
    ) -> Self {
        Self {
            cache,
            hierarchy,
            ocean_hierarchy,
            province_ids: prov_res.province_ids,
            ocean_province_ids: ocean_res.ocean_province_ids,
            is_wasteland: prov_res.is_wasteland,
            scope: MapScope::Physical, // Default to the stunning physical atlas view!
            tier: MapTier::Regions,
            texture: None,
            zoom: 0.16,
            offset: egui::vec2(0.0, 0.0),
            show_borders: true,
        }
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

        let heightmap = &self.cache.heightmap;
        let temp_grid = &self.cache.temp_grid;
        let precip_grid = &self.cache.precip_grid;
        let flow_grid = &self.cache.flow_grid;
        let is_lake = &self.cache.is_lake;

        let sea_level = 0.40f32;
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

            let mut final_color: [f32; 3];

            if scope == MapScope::Physical {
                // Pure physical atlas rendering
                final_color = compute_physical_color();
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

        // ── TOP CONTROLS BAR ──────────────────────────────────────────────────
        egui::Panel::top("top_controls").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🌍 Atlas Viewer");
                ui.separator();

                // ── Map scope: which political domain to portray ──────────────
                if ui.selectable_value(&mut self.scope, MapScope::Physical, "🗺 Physical").clicked() {
                    visual_changed = true;
                }
                if ui.selectable_value(&mut self.scope, MapScope::Land, "🏛 Land").clicked() {
                    visual_changed = true;
                }
                if ui.selectable_value(&mut self.scope, MapScope::Water, "🌊 Water").clicked() {
                    visual_changed = true;
                }
                if ui.selectable_value(&mut self.scope, MapScope::Everywhere, "🌍 Everywhere").clicked() {
                    visual_changed = true;
                }

                ui.separator();

                // ── Tier selector (only meaningful in the political scopes) ───
                ui.add_enabled_ui(self.scope != MapScope::Physical, |ui| {
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

                // Border Toggle (in political scopes)
                if self.scope != MapScope::Physical {
                    if ui.checkbox(&mut self.show_borders, "Borders").clicked() {
                        visual_changed = true;
                    }
                    ui.separator();
                }

                // Zoom controls
                if ui.button("➖").on_hover_text("Zoom Out").clicked() {
                    self.zoom = (self.zoom * 0.75).clamp(0.04, 8.0);
                }
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.button("➕").on_hover_text("Zoom In").clicked() {
                    self.zoom = (self.zoom * 1.33).clamp(0.04, 8.0);
                }
                if ui.button("Fit").on_hover_text("Reset View").clicked() {
                    self.zoom = 0.16;
                    self.offset = egui::vec2(0.0, 0.0);
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
            });
        });

        // Regenerate texture when scope, tier or borders change
        if self.texture.is_none() || visual_changed {
            let img = self.generate_texture();
            self.texture = Some(ui.ctx().load_texture("map_texture", img, TextureOptions::NEAREST));
        }

        // ── INTERACTIVE MAP CANVAS ────────────────────────────────────────────
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        if response.dragged() {
            self.offset += response.drag_delta();
        }

        if ui.rect_contains_pointer(rect) {
            let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old_zoom = self.zoom;
                self.zoom = (self.zoom + scroll * 0.001 * self.zoom * 4.0).clamp(0.04, 8.0);

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
                let sea_level = 0.40f32;
                let altitude = elev - sea_level;

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
                    if self.scope == MapScope::Physical {
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
                            ui.label(format!("Elevation: {:.0} m", altitude * 10000.0));
                        } else {
                            ui.label(format!("Depth: {:.0} m", (sea_level - elev) * 4000.0));
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
                        ui.label(format!("Depth: {:.0} m", (sea_level - elev) * 4000.0));
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
                        ui.label(format!("Elevation: {:.0} m", altitude * 10000.0));
                        ui.label(format!("Temperature: {:.1} °C", temp));
                        ui.label(format!("Precipitation: {:.0} mm/yr", precip * 1200.0));
                        ui.separator();
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else if *self.cache.is_lake.get(map_x, map_y) {
                        ui.heading("💧 Inland Freshwater Lake");
                        ui.separator();
                        ui.label(format!("Elevation: {:.0} m", altitude * 10000.0));
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
                        ui.separator();
                        ui.label(format!("Size: {} px", node.pixel_count));
                        ui.label(format!("Elevation: {:.0} m", altitude * 10000.0));
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    } else {
                        ui.heading("🌊 Ocean");
                        ui.label(format!("Depth: {:.0} m", (sea_level - elev) * 4000.0));
                        ui.label(format!("Coordinates: ({}, {})", map_x, map_y));
                    }
                });
            }
        }
    }
}
