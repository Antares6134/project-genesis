use crate::core::Grid;
use crate::geopolitics::GeopoliticalHierarchy;
use image::{ImageBuffer, Luma, Rgb, RgbImage};
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*;
use std::path::Path;

#[inline(always)]
fn lerp_color(c1: [u8; 3], c2: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (c1[0] as f32 * (1.0 - t) + c2[0] as f32 * t) as u8,
        (c1[1] as f32 * (1.0 - t) + c2[1] as f32 * t) as u8,
        (c1[2] as f32 * (1.0 - t) + c2[2] as f32 * t) as u8,
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

pub fn get_atlas_biome_color(temp: f32, precip: f32, altitude: f32) -> [u8; 3] {
    if temp < -3.0 {
        return [242, 246, 250];
    } else if temp < 0.5 && altitude > 0.16 {
        let t = (0.5 - temp) / 3.5;
        return lerp_color([125, 120, 115], [242, 246, 250], t);
    } else if altitude > 0.18 {
        let t = ((altitude - 0.18) / 0.14).clamp(0.0, 1.0);
        let base = if precip < 0.35 { [165, 150, 120] } else { [85, 120, 80] };
        return lerp_color(base, [115, 105, 95], t);
    }

    let ice_cap          = [242, 246, 250];
    let tundra           = [158, 165, 142];
    let taiga            = [56, 95, 68];
    let cold_steppe      = [168, 160, 128];
    let prairie          = [148, 168, 105];
    let deciduous_forest = [78, 138, 75];
    let rainforest       = [28, 92, 44];
    let savanna          = [178, 168, 98];
    let desert           = [218, 192, 138];

    if temp < 2.0 {
        let t = (temp + 3.0) / 5.0;
        lerp_color(ice_cap, tundra, t)
    } else if temp < 14.0 {
        let wetness = precip.clamp(0.0, 1.0);
        let cold_base = lerp_color(cold_steppe, taiga, wetness);
        let t = (temp - 2.0) / 12.0;
        lerp_color(tundra, cold_base, t)
    } else if temp < 24.0 {
        let temperate_base = if precip < 0.16 {
            lerp_color(desert, prairie, precip / 0.16)
        } else if precip < 0.45 {
            lerp_color(prairie, deciduous_forest, (precip - 0.16) / 0.29)
        } else {
            lerp_color(deciduous_forest, rainforest, (precip - 0.45) / 0.55)
        };
        let t = (temp - 14.0) / 10.0;
        lerp_color(taiga, temperate_base, t)
    } else {
        if precip < 0.18 {
            lerp_color(desert, savanna, precip / 0.18)
        } else if precip < 0.46 {
            lerp_color(savanna, deciduous_forest, (precip - 0.18) / 0.28)
        } else {
            lerp_color(deciduous_forest, rainforest, (precip - 0.46) / 0.54)
        }
    }
}

pub fn get_biome_name(temp: f32, precip: f32, altitude: f32) -> &'static str {
    if temp < -3.0 {
        "Glacial Ice Sheet"
    } else if temp < 0.5 && altitude > 0.16 {
        "Alpine Snow Peak"
    } else if altitude > 0.18 {
        if precip < 0.35 { "Arid Mountain Plateau" } else { "Alpine Mountain Ridge" }
    } else if temp < 2.0 {
        "Arctic Tundra"
    } else if temp < 14.0 {
        if precip < 0.30 { "Cold Steppe" } else { "Boreal Taiga" }
    } else if temp < 24.0 {
        if precip < 0.16 {
            "Temperate Semi-Arid Scrub"
        } else if precip < 0.45 {
            "Temperate Grassland / Prairie"
        } else {
            "Temperate Deciduous Forest"
        }
    } else {
        if precip < 0.18 {
            "Subtropical Arid Desert"
        } else if precip < 0.46 {
            "Tropical Savanna"
        } else {
            "Tropical Rainforest"
        }
    }
}

/// Golden-ratio HSV to RGB converter: Generates distinct, harmonious pastel territory colors.
pub fn id_to_territory_color(id: u32) -> [f32; 3] {
    if id == 0 {
        return [0.0, 0.0, 0.0];
    }
    let golden_ratio = 0.618033988749895f32;
    let h = ((id as f32) * golden_ratio).fract(); // Hue [0.0, 1.0)
    let s = 0.52f32; // Gentle saturation
    let v = 0.88f32; // Brightness

    let h_i = (h * 6.0).floor() as usize;
    let f = h * 6.0 - h_i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);

    let (r, g, b) = match h_i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };

    [r * 255.0, g * 255.0, b * 255.0]
}

pub fn save_physical_map<P: AsRef<Path>>(
    heightmap: &Grid<f32>, temp_grid: &Grid<f32>, precip_grid: &Grid<f32>,
    flow_grid: &Grid<f32>, is_lake: &Grid<bool>, sea_level: f32,
    min_river_flow: f32, major_river_flow: f32, path: P,
) -> Result<(), image::ImageError> {
    let w = heightmap.width as u32;
    let h = heightmap.height as u32;
    let mut img = RgbImage::new(w, h);
    let lx = -0.7071f32;
    let ly = -0.7071f32;
    let render_noise = OpenSimplex::new(777);

    let raw_pixels: &mut [u8] = img.as_mut();
    raw_pixels.par_chunks_exact_mut((w * 3) as usize).enumerate().for_each(|(y_idx, row)| {
        let y = y_idx as isize;
        let v = y_idx as f32 / (h - 1) as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;
        
        for x_idx in 0..w {
            let x = x_idx as isize;
            let u = x_idx as f32 / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;

            let px = (lat.cos() * lon.cos()) as f64;
            let py = lat.sin() as f64;
            let pz = (lat.cos() * lon.sin()) as f64;
            
            // Fractal Micro-Climate: shatters the smooth temperature bounds!
            let micro_noise = sample_3d_fbm(&render_noise, px * 35.0, py * 35.0, pz * 35.0, 4, 2.0, 0.5);

            let elev = *heightmap.get(x, y);
            let temp = *temp_grid.get(x, y);
            let precip = *precip_grid.get(x, y);
            
            let display_temp = temp + micro_noise * 5.5;

            let (gx, gy) = heightmap.gradient(x as f32, y as f32);
            let slope = (gx * gx + gy * gy).sqrt();
            let hillshade = -(gx * lx + gy * ly);
            let mut shade = (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38);
            if y < 2 || y > (h as isize - 3) { shade = 1.0; }

            let mut final_color: [f32; 3] = if elev <= sea_level {
                if display_temp < -8.0 {
                    [245.0, 248.0, 255.0]
                } else if display_temp < -3.0 {
                    [165.0, 205.0, 235.0]
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
            } else if *is_lake.get(x, y) {
                [42.0, 125.0, 185.0]
            } else {
                let altitude = elev - sea_level;
                let base = get_atlas_biome_color(display_temp, precip, altitude);
                [(base[0] as f32 * shade).clamp(0.0, 255.0), (base[1] as f32 * shade).clamp(0.0, 255.0), (base[2] as f32 * shade).clamp(0.0, 255.0)]
            };

            let flow = *flow_grid.get(x, y);
            if elev > sea_level && !*is_lake.get(x, y) && flow >= min_river_flow {
                let river_strength = ((flow - min_river_flow) / major_river_flow).sqrt().clamp(0.35, 1.0);
                let blend = (river_strength * 1.5).clamp(0.0, 1.0);
                final_color = [
                    final_color[0] * (1.0 - blend) + 20.0 * blend,
                    final_color[1] * (1.0 - blend) + 75.0 * blend,
                    final_color[2] * (1.0 - blend) + 150.0 * blend,
                ];
            }

            let pixel_idx = (x_idx * 3) as usize;
            row[pixel_idx]     = final_color[0] as u8;
            row[pixel_idx + 1] = final_color[1] as u8;
            row[pixel_idx + 2] = final_color[2] as u8;
        }
    });

    img.save(path)
}

pub fn save_hierarchical_political_map<P: AsRef<Path>>(
    heightmap: &Grid<f32>, temp_grid: &Grid<f32>, precip_grid: &Grid<f32>,
    flow_grid: &Grid<f32>, is_lake: &Grid<bool>, hierarchy: &GeopoliticalHierarchy,
    province_grid: &Grid<u32>, sea_level: f32, min_river_flow: f32, major_river_flow: f32, path: P,
) -> Result<(), image::ImageError> {
    let w = heightmap.width as u32;
    let h = heightmap.height as u32;
    let mut img = RgbImage::new(w, h);
    let lx = -0.7071f32;
    let ly = -0.7071f32;
    let render_noise = OpenSimplex::new(777);

    // Fast O(1) lookup tables for province tiers
    let mut area_lut = vec![0u32; hierarchy.provinces.len()];
    let mut region_lut = vec![0u32; hierarchy.provinces.len()];
    let mut super_lut = vec![0u32; hierarchy.provinces.len()]; // FIXED: Now defined!
    
    for p in &hierarchy.provinces {
        area_lut[p.id as usize] = p.area_id;
        region_lut[p.id as usize] = p.region_id;
        super_lut[p.id as usize] = p.superregion_id; // FIXED: Now populated!
    }

    let raw_pixels: &mut [u8] = img.as_mut();
    raw_pixels.par_chunks_exact_mut((w * 3) as usize).enumerate().for_each(|(y_idx, row)| {
        let y = y_idx as isize;
        let v = y_idx as f32 / (h - 1) as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;

        for x_idx in 0..w {
            let x = x_idx as isize;
            let u = x_idx as f32 / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;

            let px = (lat.cos() * lon.cos()) as f64;
            let py = lat.sin() as f64;
            let pz = (lat.cos() * lon.sin()) as f64;

            let micro_noise = sample_3d_fbm(&render_noise, px * 35.0, py * 35.0, pz * 35.0, 4, 2.0, 0.5);

            let elev = *heightmap.get(x, y);
            let temp = *temp_grid.get(x, y);
            let precip = *precip_grid.get(x, y);
            let display_temp = temp + micro_noise * 5.5;

            let (gx, gy) = heightmap.gradient(x as f32, y as f32);
            let slope = (gx * gx + gy * gy).sqrt();
            let hillshade = -(gx * lx + gy * ly);
            let mut shade = (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38);
            if y < 2 || y > (h as isize - 3) { shade = 1.0; }

            let mut final_color: [f32; 3];

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
            } else if *is_lake.get(x, y) {
                final_color = [42.0, 125.0, 185.0];
            } else {
                let altitude = elev - sea_level;
                let base = get_atlas_biome_color(display_temp, precip, altitude);
                final_color = [(base[0] as f32 * shade).clamp(0.0, 255.0), (base[1] as f32 * shade).clamp(0.0, 255.0), (base[2] as f32 * shade).clamp(0.0, 255.0)];
            }

            let flow = *flow_grid.get(x, y);
            if elev > sea_level && !*is_lake.get(x, y) && flow >= min_river_flow {
                let river_strength = ((flow - min_river_flow) / major_river_flow).sqrt().clamp(0.25, 1.0);
                let blend = (river_strength * 1.35).clamp(0.0, 1.0);
                final_color = [
                    final_color[0] * (1.0 - blend) + 20.0 * blend,
                    final_color[1] * (1.0 - blend) + 85.0 * blend,
                    final_color[2] * (1.0 - blend) + 160.0 * blend,
                ];
            }

            // --- COHESIVE REGIONAL REALM COLOR BLEND ---
            let my_id = *province_grid.get(x, y);
            if my_id != 0 && elev > sea_level {
                let my_reg = region_lut[my_id as usize];
                let territory_rgb = id_to_territory_color(my_reg);

                final_color = [
                    final_color[0] * 0.70 + territory_rgb[0] * 0.30,
                    final_color[1] * 0.70 + territory_rgb[1] * 0.30,
                    final_color[2] * 0.70 + territory_rgb[2] * 0.30,
                ];
            }

            // --- HIERARCHICAL BORDER RENDERING ---
            if my_id != 0 {
                let mut is_super = false;
                let mut is_region = false;
                let mut is_area = false;
                let mut is_prov = false;

                for dy in -2..=2isize {
                    for dx in -2..=2isize {
                        if dx == 0 && dy == 0 { continue; }
                        
                        let nx = province_grid.wrap_x(x + dx) as isize;
                        let ny = y + dy;
                        if ny < 0 || ny >= h as isize { continue; }

                        let neighbor_id = *province_grid.get(nx, ny);
                        let dist_sq = dx * dx + dy * dy;

                        let target_elev = *heightmap.get(nx, ny);
                        let is_wasteland_neighbor = neighbor_id == 0 && target_elev > sea_level && !*is_lake.get(nx, ny);

                        if is_wasteland_neighbor && dist_sq <= 1 {
                            is_prov = true;
                        } else if neighbor_id != 0 && neighbor_id != my_id {
                            let n_sup = super_lut[neighbor_id as usize];
                            let n_reg = region_lut[neighbor_id as usize];
                            let n_area = area_lut[neighbor_id as usize];

                            if n_sup != super_lut[my_id as usize] { is_super = true; }
                            if n_reg != region_lut[my_id as usize] && dist_sq <= 4 { is_region = true; }
                            if n_area != area_lut[my_id as usize] && dist_sq <= 2 { is_area = true; }
                            if dist_sq <= 1 { is_prov = true; }
                        }
                    }
                }

                if is_super {
                    final_color = [230.0, 235.0, 245.0];
                } else if is_region {
                    final_color = [10.0, 10.0, 12.0];
                } else if is_area {
                    final_color = [50.0, 50.0, 55.0];
                } else if is_prov {
                    final_color = [
                        final_color[0] * 0.55,
                        final_color[1] * 0.55,
                        final_color[2] * 0.55,
                    ];
                }
            }

            let pixel_idx = (x_idx * 3) as usize;
            row[pixel_idx]     = final_color[0] as u8;
            row[pixel_idx + 1] = final_color[1] as u8;
            row[pixel_idx + 2] = final_color[2] as u8;
        }
    });

    img.save(path)
}

pub fn save_raw_heightmap_16bit<P: AsRef<Path>>(
    heightmap: &Grid<f32>,
    path: P,
) -> Result<(), image::ImageError> {
    let mut img: ImageBuffer<Luma<u16>, Vec<u16>> =
        ImageBuffer::new(heightmap.width as u32, heightmap.height as u32);

    for y in 0..heightmap.height {
        for x in 0..heightmap.width {
            let h = *heightmap.get(x as isize, y as isize);
            let val_16 = (h.clamp(0.0, 1.0) * 65535.0) as u16;
            img.put_pixel(x as u32, y as u32, Luma([val_16]));
        }
    }

    img.save(path)
}