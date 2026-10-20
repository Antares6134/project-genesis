use crate::core::Grid;
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

/// Authentic Physical Atlas Color System
pub fn get_atlas_biome_color(temp: f32, precip: f32, altitude: f32) -> [u8; 3] {
    // 1. Extreme Alpine Altitudes
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

    // 2. Earthy Physical Atlas Palettes
    let ice_cap          = [242, 246, 250];
    let tundra           = [158, 165, 142];
    let taiga            = [56, 95, 68];
    let cold_steppe      = [168, 160, 128];
    let prairie          = [148, 168, 105];
    let deciduous_forest = [78, 138, 75];
    let rainforest       = [28, 92, 44];
    let savanna          = [178, 168, 98];
    let desert           = [218, 192, 138];

    // 3. Smooth Temperature & Moisture Transitions
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

    let lx = -0.7071f32;
    let ly = -0.7071f32;

    // High-frequency render noise to fracture ice caps and add satellite grain
    let render_noise = OpenSimplex::new(7777);

    let raw_pixels: &mut [u8] = img.as_mut();
    raw_pixels
        .par_chunks_exact_mut((w * 3) as usize)
        .enumerate()
        .for_each(|(y_idx, row)| {
            let y = y_idx as isize;
            
            // Calculate 3D sphere latitude for render noise
            let v = y_idx as f32 / (h - 1) as f32;
            let lat = (0.5 - v) * std::f32::consts::PI;
            let cos_lat = lat.cos() as f64;
            let sin_lat = lat.sin() as f64;

            for x_idx in 0..w {
                let x = x_idx as isize;
                let pixel_idx = (x_idx * 3) as usize;

                // 3D Unit Sphere coordinate
                let u = x_idx as f32 / w as f32;
                let lon = (u - 0.5) * std::f32::consts::TAU;
                let px = cos_lat * (lon as f64).cos();
                let py = sin_lat;
                let pz = cos_lat * (lon as f64).sin();

                // Generate fractal micro-climate noise (shatters smooth lines!)
                let micro_noise = sample_3d_fbm(&render_noise, px * 28.0, py * 28.0, pz * 28.0, 4, 2.0, 0.5);

                let elev = *heightmap.get(x, y);
                let temp = *temp_grid.get(x, y);
                let precip = *precip_grid.get(x, y);
                let lake = *is_lake.get(x, y);
                let flow = *flow_grid.get(x, y);

                // Perturb climate with noise
                let display_temp = temp + micro_noise * 4.5;
                let display_precip = (precip + micro_noise * 0.08).clamp(0.0, 1.0);

                // --- SWISS SHADED RELIEF ---
                let (gx, gy) = heightmap.gradient(x as f32, y as f32);
                let slope = (gx * gx + gy * gy).sqrt();
                let hillshade = -(gx * lx + gy * ly);

                let mut shade = (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38);

                // Fix 1-pixel boundary cliff shadow at the extreme poles
                if y < 2 || y > (h as isize - 3) {
                    shade = 1.0; 
                }

                // --- WATER VS LAND COLORING ---
                let mut final_color: [f32; 3] = if elev <= sea_level {
                    if display_temp < -8.0 {
                        [245.0, 248.0, 255.0] // Glacial Sea Ice (Now jagged and fractured!)
                    } else if display_temp < -3.0 {
                        [175.0, 210.0, 235.0] // Broken Pack Ice Edge
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
                    let base = get_atlas_biome_color(display_temp, display_precip, altitude);

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
                        .clamp(0.25, 1.0);

                    let river_color = [20.0, 85.0, 160.0]; 
                    let blend = (river_strength * 1.35).clamp(0.0, 1.0);
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