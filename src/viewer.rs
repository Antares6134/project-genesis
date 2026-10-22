use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*;
use crate::core::{Grid, WorldCache};
use crate::geography::ProvinceResult;
use crate::geopolitics::GeopoliticalHierarchy;
use crate::export::image::{get_atlas_biome_color, get_biome_name, id_to_territory_color};

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum MapMode {
    Physical,
    Provinces,
    Areas,
    Regions,
    Superregions,
    Continents,
}

pub struct MapViewer {
    cache: WorldCache,
    hierarchy: GeopoliticalHierarchy,
    province_ids: Grid<u32>,
    is_wasteland: Grid<bool>,
    mode: MapMode,
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

impl MapViewer {
    pub fn new(cache: WorldCache, hierarchy: GeopoliticalHierarchy, prov_res: ProvinceResult) -> Self {
        Self {
            cache,
            hierarchy,
            province_ids: prov_res.province_ids,
            is_wasteland: prov_res.is_wasteland,
            mode: MapMode::Physical, // Default to stunning physical atlas view!
            texture: None,
            zoom: 0.16,
            offset: egui::vec2(0.0, 0.0),
            show_borders: true,
        }
    }

    fn generate_texture(&self) -> ColorImage {
        let w = self.cache.width;
        let h = self.cache.height;
        let mode = self.mode;
        let show_borders = self.show_borders;
        let hierarchy = &self.hierarchy;
        let province_ids = &self.province_ids;
        let is_wasteland = &self.is_wasteland;

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

            if mode == MapMode::Physical {
                // Pure physical atlas rendering
                final_color = compute_physical_color();
            } else {
                // ── Political map modes with authentic Wastelands ─────────
                let is_waste = *is_wasteland.get(x as isize, y as isize)
                    || (id == 0 && elev > sea_level && !*is_lake.get(x as isize, y as isize));

                if elev <= sea_level {
                    if display_temp < -8.0 {
                        final_color = [245.0, 248.0, 255.0];
                    } else if display_temp < -3.0 {
                        final_color = [175.0, 210.0, 235.0];
                    } else {
                        let depth = (elev / sea_level).clamp(0.0, 1.0);
                        let shelf = depth.powf(2.8);
                        let sea_shade = (1.0 + hillshade * 2.2).clamp(0.85, 1.15);
                        final_color = [
                            (12.0 * (1.0 - shelf) + 45.0 * shelf) * sea_shade,
                            (35.0 * (1.0 - shelf) + 125.0 * shelf) * sea_shade,
                            (75.0 * (1.0 - shelf) + 185.0 * shelf) * sea_shade,
                        ];
                    }
                } else if *is_lake.get(x as isize, y as isize) {
                    final_color = [42.0, 125.0, 185.0];
                } else if is_waste {
                    // Impassable Wasteland: physical terrain relief with subtle neutral shading
                    let phys = compute_physical_color();
                    final_color = [
                        phys[0] * 0.78 + 35.0,
                        phys[1] * 0.78 + 35.0,
                        phys[2] * 0.78 + 40.0,
                    ];
                } else {
                    // Civilized Populated Province: territory tint over subtle terrain relief
                    let node = &hierarchy.provinces[id as usize];
                    let color_id = match mode {
                        MapMode::Physical => unreachable!(),
                        MapMode::Provinces => node.id,
                        MapMode::Areas => node.area_id,
                        MapMode::Regions => node.region_id,
                        MapMode::Superregions => node.superregion_id,
                        MapMode::Continents => node.continent_id,
                    };

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

                    // Border darkening
                    if show_borders {
                        let rx = province_ids.wrap_x(x as isize + 1) as isize;
                        let right_id = *province_ids.get(rx, y as isize);
                        let down_id = if y + 1 < h {
                            *province_ids.get(x as isize, y as isize + 1)
                        } else {
                            id
                        };

                        let is_border = if right_id != id {
                            if right_id == 0 {
                                true // Coast or Wasteland border
                            } else {
                                let r_node = &hierarchy.provinces[right_id as usize];
                                let r_cid = match mode {
                                    MapMode::Physical => unreachable!(),
                                    MapMode::Provinces => r_node.id,
                                    MapMode::Areas => r_node.area_id,
                                    MapMode::Regions => r_node.region_id,
                                    MapMode::Superregions => r_node.superregion_id,
                                    MapMode::Continents => r_node.continent_id,
                                };
                                r_cid != color_id
                            }
                        } else if down_id != id {
                            if down_id == 0 {
                                true
                            } else {
                                let d_node = &hierarchy.provinces[down_id as usize];
                                let d_cid = match mode {
                                    MapMode::Physical => unreachable!(),
                                    MapMode::Provinces => d_node.id,
                                    MapMode::Areas => d_node.area_id,
                                    MapMode::Regions => d_node.region_id,
                                    MapMode::Superregions => d_node.superregion_id,
                                    MapMode::Continents => d_node.continent_id,
                                };
                                d_cid != color_id
                            }
                        } else {
                            false
                        };

                        if is_border {
                            final_color[0] *= 0.55;
                            final_color[1] *= 0.55;
                            final_color[2] *= 0.55;
                        }
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

                // Map Mode Selector Tabs
                if ui.selectable_value(&mut self.mode, MapMode::Physical, "Physical").clicked() {
                    visual_changed = true;
                }
                ui.separator();

                if ui.selectable_value(&mut self.mode, MapMode::Provinces, "Provinces").clicked() {
                    visual_changed = true;
                }
                if ui.selectable_value(&mut self.mode, MapMode::Areas, "Areas").clicked() {
                    visual_changed = true;
                }
                if ui.selectable_value(&mut self.mode, MapMode::Regions, "Regions").clicked() {
                    visual_changed = true;
                }
                if ui.selectable_value(&mut self.mode, MapMode::Superregions, "Superregions").clicked() {
                    visual_changed = true;
                }
                if ui.selectable_value(&mut self.mode, MapMode::Continents, "Continents").clicked() {
                    visual_changed = true;
                }

                ui.separator();

                // Border Toggle (in political modes)
                if self.mode != MapMode::Physical {
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
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Continents: {} | Superregions: {} | Regions: {} | Areas: {} | Provinces: {}",
                    self.hierarchy.total_continents,
                    self.hierarchy.total_superregions,
                    self.hierarchy.total_regions,
                    self.hierarchy.total_areas,
                    self.hierarchy.total_provinces
                ));
            });
        });

        // Regenerate texture when mode or borders change
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

                let id = *self.province_ids.get(map_x, map_y);
                let is_waste = *self.is_wasteland.get(map_x, map_y)
                    || (id == 0 && elev > sea_level && !*self.cache.is_lake.get(map_x, map_y));

                response.on_hover_ui_at_pointer(|ui| {
                    if self.mode == MapMode::Physical {
                        let biome_title = if elev <= sea_level {
                            if temp < -8.0 {
                                "Polar Glacial Ice Shelf"
                            } else if temp < -3.0 {
                                "Polar Pack Sea Ice"
                            } else {
                                "Ocean"
                            }
                        } else if *self.cache.is_lake.get(map_x, map_y) {
                            "Inland Freshwater Lake"
                        } else {
                            get_biome_name(temp, precip, altitude)
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
                    } else if is_waste {
                        ui.heading("🏔️ Wasteland");
                        ui.strong("Impassable / Uninhabited Wilderness");
                        ui.separator();
                        ui.label(format!("Environment: {}", get_biome_name(temp, precip, altitude)));
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
