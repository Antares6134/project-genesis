use crate::core::polar::{ICE_SHELF_TEMP, PACK_ICE_TEMP};
use crate::core::{relief, Grid};
use crate::geopolitics::GeopoliticalHierarchy;
use image::{ImageBuffer, Luma, RgbImage};
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

/// Elevation boundaries of the hypsometric tint scale, in metres, ascending from
/// the floor of the deepest trench to the summit of the highest peak.
///
/// This table — together with [`HYPSOMETRIC_COLORS`] and [`HYPSOMETRIC_NAMES`] —
/// is the single definition of the Topographic palette: the Atlas Viewer's
/// *Topographic* scope, its legend panel and the headless topographic export all
/// paint a pixel of a given elevation in exactly the same colour.
pub const HYPSOMETRIC_EDGES: [f32; 21] = [
    -11_000.0, -9_000.0, -8_000.0, -7_000.0, -6_000.0, -5_000.0, -4_000.0,
    -3_000.0, -2_000.0, -1_000.0, -200.0, 0.0, 200.0, 500.0, 1_000.0, 2_000.0,
    3_000.0, 4_500.0, 6_000.0, 8_000.0, 9_000.0,
];

/// Colour of each of the twenty bands between consecutive [`HYPSOMETRIC_EDGES`]:
/// eleven bathymetric tints below the water line, nine land tints above it.
pub const HYPSOMETRIC_COLORS: [[u8; 3]; 20] = [
    [12, 10, 34],    // -11 000 … -9 000  hadal trench floor
    [16, 20, 56],    //  -9 000 … -8 000  trench
    [20, 32, 82],    //  -8 000 … -7 000  trench
    [26, 48, 110],   //  -7 000 … -6 000  deep basin
    [30, 64, 134],   //  -6 000 … -5 000  deep basin
    [38, 84, 152],   //  -5 000 … -4 000  abyssal plain
    [48, 106, 170],  //  -4 000 … -3 000  abyssal plain
    [70, 134, 190],  //  -3 000 … -2 000  continental rise
    [96, 160, 206],  //  -2 000 … -1 000  continental slope
    [126, 186, 220], //  -1 000 …   -200  continental slope
    [158, 208, 232], //   -200 …      0  continental shelf
    [156, 194, 128], //      0 …    200  coastal plain
    [180, 202, 118], //    200 …    500  lowland
    [208, 206, 122], //    500 …  1 000  upland
    [226, 200, 122], //  1 000 …  2 000  highland
    [222, 174, 106], //  2 000 …  3 000  plateau
    [198, 146, 100], //  3 000 …  4 500  mountain range
    [172, 124, 96],  //  4 500 …  6 000  high mountain range
    [206, 200, 198], //  6 000 …  8 000  snow line
    [250, 250, 252], //  8 000 …  9 000  nival peak
];

/// Human-readable name of each hypsometric band, in the same order.
pub const HYPSOMETRIC_NAMES: [&str; 20] = [
    "Hadal Trench Floor",
    "Oceanic Trench",
    "Oceanic Trench",
    "Deep Ocean Basin",
    "Deep Ocean Basin",
    "Abyssal Plain",
    "Abyssal Plain",
    "Continental Rise",
    "Continental Slope",
    "Continental Slope",
    "Continental Shelf",
    "Coastal Plain",
    "Lowland",
    "Upland",
    "Highland",
    "Plateau",
    "Mountain Range",
    "High Mountain Range",
    "Snow Line",
    "Nival Peak",
];

/// Index of the hypsometric band an elevation in metres falls into.
#[inline(always)]
pub fn hypsometric_band(metres: f32) -> usize {
    HYPSOMETRIC_EDGES
        .partition_point(|&edge| edge <= metres)
        .saturating_sub(1)
        .min(HYPSOMETRIC_COLORS.len() - 1)
}

/// Hypsometric tint of an elevation in metres (negative = below the sea surface).
#[inline(always)]
pub fn hypsometric_color(metres: f32) -> [u8; 3] {
    HYPSOMETRIC_COLORS[hypsometric_band(metres)]
}

/// Label of a hypsometric band, e.g. `-5 000 … -4 000 m`.
pub fn hypsometric_range_label(band: usize) -> String {
    let band = band.min(HYPSOMETRIC_COLORS.len() - 1);
    let lo = HYPSOMETRIC_EDGES[band];
    let hi = HYPSOMETRIC_EDGES[band + 1];
    if band + 1 == HYPSOMETRIC_COLORS.len() {
        format!("{} m +", thousands(lo))
    } else {
        format!("{} … {} m", thousands(lo), thousands(hi))
    }
}

/// `-5000.0` rendered as `-5 000`, so a legend column never breaks its alignment.
fn thousands(metres: f32) -> String {
    let v = metres.round() as i64;
    let s = v.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    if v < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hypsometric_bands_span_the_whole_datum_without_gaps() {
        // Every band index must be reachable, from the trench floor to the peak,
        // and the water line must fall in the shelf band (index 10).
        assert_eq!(hypsometric_band(-11_000.0), 0);
        assert_eq!(hypsometric_band(-50_000.0), 0);
        assert_eq!(hypsometric_band(-1.0), 10);
        assert_eq!(hypsometric_band(0.0), 11);
        assert_eq!(hypsometric_band(1.0), 11);
        assert_eq!(hypsometric_band(9_000.0), HYPSOMETRIC_COLORS.len() - 1);
        assert_eq!(hypsometric_band(50_000.0), HYPSOMETRIC_COLORS.len() - 1);
        for band in 0..HYPSOMETRIC_COLORS.len() {
            let midpoint = (HYPSOMETRIC_EDGES[band] + HYPSOMETRIC_EDGES[band + 1]) * 0.5;
            assert_eq!(hypsometric_band(midpoint), band, "band {band} is unreachable");
        }
    }

    #[test]
    fn band_labels_are_readable_and_terminated() {
        assert_eq!(hypsometric_range_label(5), "-5 000 … -4 000 m");
        assert_eq!(hypsometric_range_label(10), "-200 … 0 m");
        assert_eq!(
            hypsometric_range_label(HYPSOMETRIC_COLORS.len() - 1),
            "8 000 m +"
        );
    }

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(-5000.0), "-5 000");
        assert_eq!(thousands(200.0), "200");
        assert_eq!(thousands(0.0), "0");
        assert_eq!(thousands(-11_000.0), "-11 000");
    }
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

/// Render the hypsometric sheet of the world — the headless twin of the Atlas
/// Viewer's *Topographic* scope.
///
/// Colour is elevation alone: the eleven bathymetric tints below the water line
/// and the nine land tints above it, defined once in [`HYPSOMETRIC_EDGES`] and
/// [`HYPSOMETRIC_COLORS`], laid under the same Swiss-style multi-angle hillshading
/// the physical map uses. Every band boundary is overdrawn as a contour line, so
/// the sheet reads like a survey chart: a pale shelf rim, a steep continental
/// slope, broad abyssal plains, and trench floors dropping to −11 km. Rivers,
/// lakes and the polar ice are drawn on top.
pub fn save_topographic_map<P: AsRef<Path>>(
    heightmap: &Grid<f32>, temp_grid: &Grid<f32>, flow_grid: &Grid<f32>,
    is_lake: &Grid<bool>, sea_level: f32,
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
            let micro_noise = sample_3d_fbm(&render_noise, px * 35.0, py * 35.0, pz * 35.0, 4, 2.0, 0.5);

            let elev = *heightmap.get(x, y);
            let display_temp = *temp_grid.get(x, y) + micro_noise * 5.5;
            let metres = relief::to_metres(elev);

            let (gx, gy) = heightmap.gradient(x as f32, y as f32);
            let slope = (gx * gx + gy * gy).sqrt();
            let hillshade = -(gx * lx + gy * ly);
            let mut shade = (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38);
            if y < 2 || y > (h as isize - 3) { shade = 1.0; }

            let is_lake_px = *is_lake.get(x, y);
            let mut final_color: [f32; 3];

            if is_lake_px && elev > sea_level {
                final_color = [42.0, 125.0, 185.0];
            } else if elev <= sea_level {
                let band = hypsometric_color(metres);
                final_color = [band[0] as f32, band[1] as f32, band[2] as f32];
                // Frozen seas read as "polar land": a drifting pack-ice veneer
                // for the seasonal fringe, a solid ice shelf deeper inside.
                if display_temp < ICE_SHELF_TEMP {
                    final_color = blend(final_color, [246.0, 249.0, 255.0], 0.90);
                } else if display_temp < PACK_ICE_TEMP {
                    final_color = blend(final_color, [222.0, 236.0, 250.0], 0.70);
                }
            } else {
                let band = hypsometric_color(metres);
                final_color = [
                    band[0] as f32 * shade,
                    band[1] as f32 * shade,
                    band[2] as f32 * shade,
                ];
                // Capped with permanent ice — keep the terrain readable
                // underneath a translucent ice veneer instead of blanking it out.
                if display_temp < -3.0 {
                    final_color = blend(final_color, [238.0, 244.0, 252.0], 0.75);
                }
                let flow = *flow_grid.get(x, y);
                if flow >= min_river_flow {
                    let river_strength = ((flow - min_river_flow) / major_river_flow).sqrt().clamp(0.25, 1.0);
                    let blend_t = (river_strength * 1.35).clamp(0.0, 1.0);
                    final_color = blend(final_color, [20.0, 85.0, 160.0], blend_t);
                }
            }

            // A contour line along every hypsometric band boundary.
            let band = hypsometric_band(metres);
            let right = hypsometric_band(relief::to_metres(*heightmap.get(x + 1, y)));
            let down = if y + 1 < h as isize {
                hypsometric_band(relief::to_metres(*heightmap.get(x, y + 1)))
            } else {
                band
            };
            if right != band || down != band {
                final_color = [final_color[0] * 0.62, final_color[1] * 0.62, final_color[2] * 0.62];
            }

            let pixel_idx = (x_idx * 3) as usize;
            row[pixel_idx]     = final_color[0].clamp(0.0, 255.0) as u8;
            row[pixel_idx + 1] = final_color[1].clamp(0.0, 255.0) as u8;
            row[pixel_idx + 2] = final_color[2].clamp(0.0, 255.0) as u8;
        }
    });

    img.save(path)
}

/// Linear blend of two float colours, `t = 0` keeping `base`.
#[inline(always)]
fn blend(base: [f32; 3], over: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        base[0] * (1.0 - t) + over[0] * t,
        base[1] * (1.0 - t) + over[1] * t,
        base[2] * (1.0 - t) + over[2] * t,
    ]
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

