use noise::{NoiseFn, OpenSimplex};
use crate::core::Grid;
use image::{ImageBuffer, Luma, Rgb, RgbImage};
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

/// Authentic Physical Atlas Color System (Natural Earth / National Geographic style)
pub fn get_atlas_biome_color(temp: f32, precip: f32, altitude: f32) -> [u8; 3] {
    // 1. Extreme Alpine Altitudes: Snow & Bare Slate Rock
    // (Only triggers on real mountain peaks where altitude > 0.18!)
    if temp < -3.0 {
        return [242, 246, 250]; // Perennial Glacial Ice
    } else if temp < 0.5 && altitude > 0.16 {
        let t = (0.5 - temp) / 3.5;
        return lerp_color([125, 120, 115], [242, 246, 250], t); // Scree to Snow
    } else if altitude > 0.18 {
        // High Montane Crags (Slate rock)
        let t = ((altitude - 0.18) / 0.14).clamp(0.0, 1.0);
        let base = if precip < 0.35 { [165, 150, 120] } else { [85, 120, 80] };
        return lerp_color(base, [115, 105, 95], t);
    }

    // 2. Earthy Physical Atlas Palettes (Muted, Natural Earth Tones)
    let ice_cap          = [242, 246, 250]; // Glacial White
    let tundra           = [158, 165, 142]; // Lichen Gray-Green
    let taiga            = [56, 95, 68];    // Deep Boreal Conifer
    let cold_steppe      = [168, 160, 128]; // Khaki Dry Steppe
    let prairie          = [148, 168, 105]; // Soft Meadow Green
    let deciduous_forest = [78, 138, 75];   // Lush Temperate Forest
    let rainforest       = [28, 92, 44];    // Deep Rainforest Emerald
    let savanna          = [178, 168, 98];  // Warm Savanna Ochre
    let desert           = [218, 192, 138]; // Muted Sandstone Gold

    // 3. Smooth Temperature & Moisture Transitions for Lowlands
    if temp < 2.0 {
        let t = (temp + 3.0) / 5.0;
        lerp_color(ice_cap, tundra, t)
    } else if temp < 14.0 {
        // Cold Boreal Zone
        let wetness = precip.clamp(0.0, 1.0);
        let cold_base = lerp_color(cold_steppe, taiga, wetness);
        let t = (temp - 2.0) / 12.0;
        lerp_color(tundra, cold_base, t)
    } else if temp < 24.0 {
        // Temperate Zone
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
        // Tropical / Subtropical Zone
        if precip < 0.18 {
            lerp_color(desert, savanna, precip / 0.18)
        } else if precip < 0.46 {
            lerp_color(savanna, deciduous_forest, (precip - 0.18) / 0.28)
        } else {
            lerp_color(deciduous_forest, rainforest, (precip - 0.46) / 0.54)
        }
    }
}

/// Exports a photorealistic High-Resolution Physical Atlas Map (2D Equirectangular)
pub fn save_physical_map<P: AsRef<Path>>(
    heightmap: &Grid<f32>,
    temp_grid: &Grid<f32>,
    precip_grid: &Grid<f32>,
    flow_grid: &Grid<f32>,
    is_lake: &Grid<bool>,
    sea_level: f32,
    min_river_flow: f32,
    major_river_flow: f32,
    path: P,
) -> Result<(), image::ImageError> {
    let w = heightmap.width as u32;
    let h = heightmap.height as u32;
    let mut img = RgbImage::new(w, h);

    // Northwest light direction (315° standard Swiss Cartographic Azimuth)
    let lx = -0.7071f32;
    let ly = -0.7071f32;

    let raw_pixels: &mut [u8] = img.as_mut();
    raw_pixels
        .par_chunks_exact_mut((w * 3) as usize)
        .enumerate()
        .for_each(|(y_idx, row)| {
            let y = y_idx as isize;

            for x_idx in 0..w {
                let x = x_idx as isize;
                let pixel_idx = (x_idx * 3) as usize;

                let elev = *heightmap.get(x, y);
                let temp = *temp_grid.get(x, y);
                let precip = *precip_grid.get(x, y);
                let lake = *is_lake.get(x, y);
                let flow = *flow_grid.get(x, y);

                // --- SWISS SHADED RELIEF (Topography & Cavity Shadows) ---
                let (gx, gy) = heightmap.gradient(x as f32, y as f32);
                let slope = (gx * gx + gy * gy).sqrt();
                let hillshade = -(gx * lx + gy * ly);

                // Contrast tuning: steep mountains cast crisp relief
                let mut shade = (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38);

                // FIX: Erase the 1-pixel boundary cliff shadow at the extreme poles!
                if y < 2 || y > (h as isize - 3) {
                    shade = 1.0; 
                }
                
                // --- WATER VS LAND COLORING ---
                let mut final_color: [f32; 3] = if elev <= sea_level {
                    if temp < -8.0 {
                        [245.0, 248.0, 255.0]
                    } else if temp < -3.0 {
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
                } else if lake {
                    [42.0, 125.0, 185.0]
                } else {
                    let altitude = elev - sea_level;
                    // NO CHECKERBOARD HASHING! Pure, clean physical atlas colors.
                    let base = get_atlas_biome_color(temp, precip, altitude);

                    [
                        (base[0] as f32 * shade).clamp(0.0, 255.0),
                        (base[1] as f32 * shade).clamp(0.0, 255.0),
                        (base[2] as f32 * shade).clamp(0.0, 255.0),
                    ]
                };

                // --- MIGHTY ARTERIAL RIVERS ---
                if elev > sea_level && !lake && flow >= min_river_flow {
                    let river_strength = ((flow - min_river_flow) / major_river_flow)
                        .sqrt()
                        .clamp(0.35, 1.0);

                    let river_color = [20.0, 75.0, 150.0]; 
                    
                    let blend = (river_strength * 1.5).clamp(0.0, 1.0);
                    final_color = [
                        final_color[0] * (1.0 - blend) + river_color[0] * blend,
                        final_color[1] * (1.0 - blend) + river_color[1] * blend,
                        final_color[2] * (1.0 - blend) + river_color[2] * blend,
                    ];
                }

                row[pixel_idx]     = final_color[0].clamp(0.0, 255.0) as u8;
                row[pixel_idx + 1] = final_color[1].clamp(0.0, 255.0) as u8;
                row[pixel_idx + 2] = final_color[2].clamp(0.0, 255.0) as u8;
            }
        });

    img.save(path)
}

/// 16-bit Grayscale heightmap exporter
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