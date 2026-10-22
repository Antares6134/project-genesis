

# File: src/climate/config.rs

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlanetaryMode {
    RotatingStandard { axial_tilt_deg: f32 }, // Earth-like rotating planet
    TidallyLocked,                            // Eyeball world with permanent day/night sides
}

#[derive(Clone, Debug)]
pub struct ClimateConfig {
    pub seed: u64,
    pub mode: PlanetaryMode,

    // Temperatures in Celsius
    pub equator_temp: f32, // e.g. 30.0 °C
    pub pole_temp: f32,    // e.g. -25.0 °C
    pub lapse_rate: f32,   // Temperature drop per elevation unit (e.g. 50.0 °C)

    // Moisture & Precipitation
    pub evaporation_rate: f32,    // How fast oceans vaporize moisture into winds
    pub rain_shadow_decay: f32,   // How quickly clouds deplete over mountains
    pub wind_passes: usize,       // Steps for moisture advection across the globe
}

impl Default for ClimateConfig {
    fn default() -> Self {
        Self {
            mode: PlanetaryMode::RotatingStandard { axial_tilt_deg: 23.5 },
            equator_temp: 32.0,
            pole_temp: -28.0,
            lapse_rate: 45.0,
            evaporation_rate: 0.12,
            rain_shadow_decay: 0.08,
            wind_passes: 45,
            seed: 42,
        }
    }
}
```


# File: src/climate/mod.rs

```rust
pub mod config;
pub mod moisture;
pub mod temperature;
pub mod wind;

pub use config::{ClimateConfig, PlanetaryMode};
pub use moisture::simulate_moisture;
pub use temperature::simulate_temperature;
pub use wind::simulate_winds;
```


# File: src/climate/moisture.rs

```rust
use crate::core::Grid;
use crate::climate::config::ClimateConfig;
use crate::climate::wind::WindVector;
use noise::{NoiseFn, OpenSimplex};

pub struct MoistureResult {
    pub precipitation: Grid<f32>,
}

pub fn simulate_moisture(
    heightmap: &Grid<f32>,
    _temperature: &Grid<f32>,
    winds: &Grid<WindVector>,
    sea_level: f32,
    config: &ClimateConfig,
) -> MoistureResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut precip_grid = Grid::new(w, h);
    let gyre_noise = OpenSimplex::new(config.seed as u32 + 777);
    let detail_noise = OpenSimplex::new(config.seed as u32 + 888);

    // 1. Calculate approximate distance to ocean (Continentality)
    let mut ocean_dist = Grid::filled_with(w, h, 25.0f32);
    for y in 0..h {
        for x in 0..w {
            if *heightmap.get(x as isize, y as isize) <= sea_level {
                ocean_dist.set(x as isize, y as isize, 0.0);
            }
        }
    }

    for _ in 0..4 {
        for y in 1..(h as isize - 1) {
            for x in 0..(w as isize) {
                let current = *ocean_dist.get(x, y);
                if current > 0.0 {
                    let min_n = (*ocean_dist.get(x - 1, y))
                        .min(*ocean_dist.get(x + 1, y))
                        .min(*ocean_dist.get(x, y - 1))
                        .min(*ocean_dist.get(x, y + 1));
                    ocean_dist.set(x, y, current.min(min_n + 1.0));
                }
            }
        }
    }

    // 2. Synthesize physically authentic planetary precipitation
    for y in 0..h {
        let v = y as f32 / (h as f32 - 1.0);
        let base_lat = ((0.5 - v) * 180.0).abs();

        for x in 0..w {
            let fx = x as f32;
            let u = fx / w as f32;
            let elev = *heightmap.get(x as isize, y as isize);

            if elev <= sea_level {
                precip_grid.set(x as isize, y as isize, 0.8);
                continue;
            }

            // A. Planetary Atmospheric Circulation Bands
            let angle = u * std::f32::consts::TAU;
            let gyre = gyre_noise.get([
                angle.cos() as f64 * 1.8,
                angle.sin() as f64 * 1.8,
                v as f64 * 3.0,
            ]) as f32;

            let organic_lat = (base_lat + gyre * 12.0).clamp(0.0, 90.0);

            let itcz = (-((organic_lat / 12.0).powf(2.0))).exp() * 0.70;
            let hadley_dist = (organic_lat - 26.0) / 7.0;
            let hadley_dry = (-hadley_dist.powf(2.0)).exp();
            let temperate_dist = (organic_lat - 48.0) / 10.0;
            let temperate = (-temperate_dist.powf(2.0)).exp() * 0.55;
            let polar_dry = ((organic_lat - 65.0).max(0.0) / 25.0).clamp(0.0, 0.7);

            let mut base_rain = 0.45 + itcz + temperate - hadley_dry * 0.48 - polar_dry;
            base_rain = base_rain.clamp(0.02, 1.2);

            // B. Continentality (Distance to Ocean)
            let dist = *ocean_dist.get(x as isize, y as isize);
            let continentality = (-dist / 14.0).exp(); 
            let inland_factor = 0.40 + 0.60 * continentality;

            // C. Orographic Rain & Rain Shadows
            let wind = *winds.get(x as isize, y as isize);
            let (gx, gy) = heightmap.gradient(fx, y as f32);
            let slope_upwind = -(wind.dx * gx + wind.dy * gy);
            let orographic = (slope_upwind * 15.0).clamp(-0.35, 0.60);

            // D. Local High-Frequency Detail Noise
            let local_noise = detail_noise.get([
                angle.cos() as f64 * 4.0,
                angle.sin() as f64 * 4.0,
                v as f64 * 6.0,
            ]) as f32 * 0.12;

            let final_precip = (base_rain * inland_factor + orographic + local_noise).clamp(0.01, 1.0);
            precip_grid.set(x as isize, y as isize, final_precip);
        }
    }

    // 3. Smooth blur pass
    let mut smoothed = precip_grid.clone();
    for y in 1..(h as isize - 1) {
        for x in 0..(w as isize) {
            let mut sum = *precip_grid.get(x, y) * 4.0;
            sum += *precip_grid.get(x - 1, y) * 2.0; sum += *precip_grid.get(x + 1, y) * 2.0;
            sum += *precip_grid.get(x, y - 1) * 2.0; sum += *precip_grid.get(x, y + 1) * 2.0;
            sum += *precip_grid.get(x - 1, y - 1);   sum += *precip_grid.get(x + 1, y - 1);
            sum += *precip_grid.get(x - 1, y + 1);   sum += *precip_grid.get(x + 1, y + 1);
            *smoothed.get_mut(x, y) = sum / 16.0;
        }
    }

    MoistureResult { precipitation: smoothed }
}
```


# File: src/climate/temperature.rs

```rust
use crate::core::Grid;
use crate::climate::config::{ClimateConfig, PlanetaryMode};
use noise::{NoiseFn, OpenSimplex};

pub fn simulate_temperature(
    heightmap: &Grid<f32>,
    sea_level: f32,
    config: &ClimateConfig,
) -> Grid<f32> {
    let w = heightmap.width;
    let h = heightmap.height;
    let mut temp_grid = Grid::new(w, h);
    let noise = OpenSimplex::new(config.seed as u32);
    
    for y in 0..h {
        let fy = y as f32;
        let v = fy / (h as f32 - 1.0); // 0.0 (North Pole) to 1.0 (South Pole)

        for x in 0..w {
            let fx = x as f32;
            let u = fx / w as f32;

            // Insolation calculation
            let insolation = match config.mode {
                PlanetaryMode::RotatingStandard { .. } => {
                    let lat_rad = (v - 0.5) * std::f32::consts::PI;
                    lat_rad.cos()
                }
                PlanetaryMode::TidallyLocked => {
                    let dx = (u - 0.5) * 2.0;
                    let dy = (v - 0.5) * 2.0;
                    let dist_sq = dx * dx + dy * dy;
                    (1.0 - dist_sq.sqrt()).max(0.0)
                }
            };

            // Steeper cosine curve: Keeps mid-latitudes temperate, freezing ONLY at extreme poles (75°+)
            let polar_taper = (insolation * 1.15).min(1.0).powf(0.65);
            let mut base_temp = config.pole_temp + polar_taper * (config.equator_temp - config.pole_temp);

            // Organic Atmospheric Waves (bends horizontal climate bands organically!)
            let angle = u * std::f32::consts::TAU;
            let atmospheric_wave = noise.get([
                angle.cos() as f64 * 1.8,
                angle.sin() as f64 * 1.8,
                v as f64 * 3.5,
            ]) as f32;
            base_temp += atmospheric_wave * 6.5;

            // 3. Altitude Lapse Rate (higher altitude cools significantly)
            let elev = *heightmap.get(x as isize, y as isize);
            let altitude_above_sea = (elev - sea_level).max(0.0);
            let temp = base_temp - altitude_above_sea * config.lapse_rate;

            temp_grid.set(x as isize, y as isize, temp);
        }
    }

    temp_grid
}
```


# File: src/climate/wind.rs

```rust
use crate::core::Grid;
use crate::climate::config::{ClimateConfig, PlanetaryMode};

#[derive(Clone, Copy, Debug, Default)]
pub struct WindVector {
    pub dx: f32,
    pub dy: f32,
}

/// Generates the global atmospheric circulation wind field.
pub fn simulate_winds(width: usize, height: usize, config: &ClimateConfig) -> Grid<WindVector> {
    let mut wind_grid = Grid::filled_with(width, height, WindVector { dx: 1.0, dy: 0.0 });

    for y in 0..height {
        let v = y as f32 / (height as f32 - 1.0); // 0.0 (North Pole) to 1.0 (South Pole)
        let lat_deg = (0.5 - v) * 180.0;          // +90 (North) to -90 (South)

        // Earth-like 3-cell atmospheric circulation
        let (base_dx, base_dy) = match config.mode {
            PlanetaryMode::RotatingStandard { .. } => {
                let abs_lat = lat_deg.abs();
                if abs_lat < 30.0 {
                    // Hadley Cell: Trade Winds blow East -> West (-dx), heading towards equator
                    let equator_pull = if lat_deg > 0.0 { 0.25 } else { -0.25 };
                    (-1.0, equator_pull)
                } else if abs_lat < 60.0 {
                    // Ferrel Cell: Westerlies blow West -> East (+dx), heading towards poles
                    let pole_pull = if lat_deg > 0.0 { -0.2 } else { 0.2 };
                    (1.2, pole_pull)
                } else {
                    // Polar Cell: Polar Easterlies blow East -> West (-dx)
                    (-0.8, 0.0)
                }
            }
            PlanetaryMode::TidallyLocked => {
                // Wind rushes violently from cold night side to hot day side center
                (0.8, 0.0)
            }
        };

        for x in 0..width {
            wind_grid.set(x as isize, y as isize, WindVector { dx: base_dx, dy: base_dy });
        }
    }

    wind_grid
}
```


# File: src/core/cache.rs

```rust
use crate::core::Grid;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};

#[derive(Clone, Serialize, Deserialize)]
pub struct WorldCache {
    pub width: usize,
    pub height: usize,
    pub heightmap: Grid<f32>,
    pub temp_grid: Grid<f32>,
    pub precip_grid: Grid<f32>,
    pub flow_grid: Grid<f32>,
    pub is_lake: Grid<bool>,
}

impl WorldCache {
    /// Dumps the entire world data to a highly compressed binary file
    pub fn save(&self, path: &str) {
        let file = File::create(path).expect("Failed to create cache file");
        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, self).expect("Failed to serialize world data");
    }

    /// Loads the world data back into RAM instantly
    pub fn load(path: &str) -> Self {
        let file = File::open(path).expect("Failed to open cache file");
        let reader = BufReader::new(file);
        bincode::deserialize_from(reader).expect("Failed to deserialize world data")
    }
}
```


# File: src/core/grid.rs

```rust
use serde::{Deserialize, Serialize};

/// A 2D continuous grid that wraps horizontally (cylindrically) along the X-axis
/// and clamps at the poles along the Y-axis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grid<T> {
    pub width: usize,
    pub height: usize,
    data: Vec<T>,
}

impl<T: Clone + Default> Grid<T> {
    /// Creates a new grid initialized with default values.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![T::default(); width * height],
        }
    }

    /// Creates a new grid filled with a specific value.
    pub fn filled_with(width: usize, height: usize, value: T) -> Self {
        Self {
            width,
            height,
            data: vec![value; width * height],
        }
    }
}

impl<T> Grid<T> {
    #[inline(always)]
    pub fn width(&self) -> usize {
        self.width
    }

    #[inline(always)]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Wraps X horizontally around the cylindrical boundary.
    #[inline(always)]
    pub fn wrap_x(&self, x: isize) -> usize {
        let w = self.width as isize;
        ((x % w + w) % w) as usize
    }

    /// Clamps Y to the north and south poles.
    #[inline(always)]
    pub fn clamp_y(&self, y: isize) -> usize {
        y.clamp(0, (self.height - 1) as isize) as usize
    }

    /// Maps 2D coordinate to 1D index with cylindrical X wrapping and polar Y clamping.
    #[inline(always)]
    pub fn coords_to_index(&self, x: isize, y: isize) -> usize {
        let wx = self.wrap_x(x);
        let cy = self.clamp_y(y);
        cy * self.width + wx
    }

    #[inline(always)]
    pub fn get(&self, x: isize, y: isize) -> &T {
        let idx = self.coords_to_index(x, y);
        &self.data[idx]
    }

    #[inline(always)]
    pub fn get_mut(&mut self, x: isize, y: isize) -> &mut T {
        let idx = self.coords_to_index(x, y);
        &mut self.data[idx]
    }

    #[inline(always)]
    pub fn set(&mut self, x: isize, y: isize, val: T) {
        let idx = self.coords_to_index(x, y);
        self.data[idx] = val;
    }

    pub fn raw_slice(&self) -> &[T] {
        &self.data
    }

    pub fn raw_slice_mut(&mut self) -> &mut [T] {
        &mut self.data
    }
}

// Float-specific mathematical operations (Heightmaps)
impl Grid<f32> {
    /// Samples the height at continuous coordinates (fx, fy) using bilinear interpolation.
    /// This is crucial for droplet erosion to calculate sub-pixel movements smoothly.
    pub fn sample_bilinear(&self, fx: f32, fy: f32) -> f32 {
        let x0 = fx.floor() as isize;
        let y0 = fy.floor() as isize;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let tx = fx - fx.floor();
        let ty = fy - fy.floor();

        let h00 = *self.get(x0, y0);
        let h10 = *self.get(x1, y0);
        let h01 = *self.get(x0, y1);
        let h11 = *self.get(x1, y1);

        // Linear interpolation along X, then Y
        let top = h00 * (1.0 - tx) + h10 * tx;
        let bottom = h01 * (1.0 - tx) + h11 * tx;

        top * (1.0 - ty) + bottom * ty
    }

    /// Computes the surface gradient / slope vector (-dh/dx, -dh/dy) at continuous coordinates.
    pub fn gradient(&self, fx: f32, fy: f32) -> (f32, f32) {
        let x0 = fx.floor() as isize;
        let y0 = fy.floor() as isize;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let tx = fx - fx.floor();
        let ty = fy - fy.floor();

        let h00 = *self.get(x0, y0);
        let h10 = *self.get(x1, y0);
        let h01 = *self.get(x0, y1);
        let h11 = *self.get(x1, y1);

        // Slope along X and Y
        let dx = (h10 - h00) * (1.0 - ty) + (h11 - h01) * ty;
        let dy = (h01 - h00) * (1.0 - tx) + (h11 - h10) * tx;

        (dx, dy)
    }

    /// Normalizes all values in the grid between 0.0 and 1.0.
    pub fn normalize(&mut self) {
        let mut min_val = f32::MAX;
        let mut max_val = f32::MIN;

        for &val in &self.data {
            if val < min_val { min_val = val; }
            if val > max_val { max_val = val; }
        }

        let range = max_val - min_val;
        if range > 1e-6 {
            for val in &mut self.data {
                *val = (*val - min_val) / range;
            }
        }
    }
}
```


# File: src/core/mod.rs

```rust
pub mod cache;
pub mod grid;

pub use cache::WorldCache;
pub use grid::Grid;
```


# File: src/export/image.rs

```rust
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
                        if neighbor_id != 0 && neighbor_id != my_id {
                            let dist_sq = dx * dx + dy * dy;

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
```


# File: src/export/mod.rs

```rust
pub mod image;
#[allow(unused_imports)]
pub use image::{save_hierarchical_political_map, save_physical_map, save_raw_heightmap_16bit};
```


# File: src/geography/config.rs

```rust
#[derive(Clone, Debug)]
pub struct GeographyConfig {
    pub num_provinces: usize,
}

impl Default for GeographyConfig {
    fn default() -> Self {
        Self {
            num_provinces: 30_000, // Grand Strategy Location Scale!
        }
    }
}
```


# File: src/geography/mod.rs

```rust
pub mod config;
pub mod provinces;

pub use config::GeographyConfig;
pub use provinces::{generate_provinces, ProvinceResult};
```


# File: src/geography/provinces.rs

```rust
use crate::core::Grid;
use crate::geography::config::GeographyConfig;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::BinaryHeap;

#[derive(Clone)]
pub struct ProvinceResult {
    pub province_ids: Grid<u32>,
    pub habitability: Grid<f32>,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct ExpandState {
    cost: u32,
    x: isize,
    y: isize,
    id: u32,
}

impl Ord for ExpandState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost)
            .then_with(|| self.x.cmp(&other.x))
            .then_with(|| self.y.cmp(&other.y))
    }
}

impl PartialOrd for ExpandState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub fn generate_provinces(
    heightmap: &Grid<f32>,
    temp_grid: &Grid<f32>,
    precip_grid: &Grid<f32>,
    flow_grid: &Grid<f32>,
    is_lake: &Grid<bool>,
    sea_level: f32,
    min_river_flow: f32,
    config: &GeographyConfig,
    seed: u64,
) -> ProvinceResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut hab_grid = Grid::filled_with(w, h, 0.0f32);
    let mut province_ids = Grid::filled_with(w, h, 0u32);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // 1. Habitability Score
    for y in 0..h {
        for x in 0..w {
            let elev = *heightmap.get(x as isize, y as isize);
            // FIX: Treat lakes just like ocean!
            if elev <= sea_level || *is_lake.get(x as isize, y as isize) {
                continue;
            }

            let temp = *temp_grid.get(x as isize, y as isize);
            let precip = *precip_grid.get(x as isize, y as isize);
            let flow = *flow_grid.get(x as isize, y as isize);

            let temp_score = (-((temp - 18.0) / 12.0).powf(2.0)).exp();
            let precip_score = if precip < 0.15 { 0.1 } else { precip.clamp(0.0, 1.0).sqrt() };
            let (gx, gy) = heightmap.gradient(x as f32, y as f32);
            let slope = (gx * gx + gy * gy).sqrt();
            let flat_score = (1.0 - slope * 12.0).clamp(0.08, 1.0);
            let river_bonus = if flow >= min_river_flow { 1.4 } else { 1.0 };

            hab_grid.set(x as isize, y as isize, temp_score * precip_score * flat_score * river_bonus);
        }
    }

    // 2. Strategic Seeding
    let mut seed_candidates = Vec::new();
    let stride = 3;
    for y in (0..h).step_by(stride) {
        for x in (0..w).step_by(stride) {
            let hab = *hab_grid.get(x as isize, y as isize);
            if hab > 0.05 {
                let r_score = hab * rng.random_range(0.4..1.0);
                seed_candidates.push((r_score, x as isize, y as isize));
            }
        }
    }

    seed_candidates.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    let mut seeds: Vec<(isize, isize)> = Vec::with_capacity(config.num_provinces);
    let mut current_id = 1u32;

    for &(_, cx, cy) in &seed_candidates {
        if seeds.len() >= config.num_provinces { break; }

        let local_hab = *hab_grid.get(cx, cy);
        let min_dist_sq: isize = ((14.0 / (local_hab + 0.18)).powf(2.0)) as isize;

        let mut too_close = false;
        for &(ax, ay) in &seeds {
            let mut dx = (cx - ax).abs();
            if dx > (w as isize) / 2 { dx = w as isize - dx; }
            let dy = cy - ay;
            if dx * dx + dy * dy < min_dist_sq {
                too_close = true;
                break;
            }
        }

        if !too_close {
            seeds.push((cx, cy));
            current_id += 1;
        }
    }

    let num_seeds = seeds.len();
    println!("✓ Seeded {} province centers.", num_seeds);

    let neighbors: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
        (-1, -1, 1.414), (1, -1, 1.414), (-1, 1, 1.414), (1, 1, 1.414),
    ];

    // 3. TWO-PASS RELAXED FLOOD FILL (Initial pass -> Centroid Relaxation -> Final pass)
    // This turns ugly jagged tentacles into compact, natural rounded provinces!
    for pass in 0..2 {
        let is_last_pass = pass == 1;
        province_ids = Grid::filled_with(w, h, 0u32);
        let mut heap = BinaryHeap::new();

        for (idx, &(sx, sy)) in seeds.iter().enumerate() {
            let pid = (idx + 1) as u32;
            province_ids.set(sx, sy, pid);
            heap.push(ExpandState { cost: 0, x: sx, y: sy, id: pid });
        }

        while let Some(state) = heap.pop() {
            if *province_ids.get(state.x, state.y) != state.id { continue; }
            let current_elev = *heightmap.get(state.x, state.y);

            for &(dx, dy, dist) in &neighbors {
                let nx = heightmap.wrap_x(state.x + dx) as isize;
                let ny = state.y + dy;
                if ny < 0 || ny >= h as isize { continue; }

                let target_elev = *heightmap.get(nx, ny);
                // FIX: Borders stop dead at lakes!
                if target_elev <= sea_level || *is_lake.get(nx, ny) { continue; }

                if *province_ids.get(nx, ny) == 0 {
                    let elev_diff = (target_elev - current_elev).abs();

                    // BALANCED COST: Distance dominates (keeping shapes rounded),
                    // while mountain crests and rivers act as natural snap boundaries!
                    let mountain_penalty = (elev_diff * 350.0).min(45.0);
                    let river_flow = *flow_grid.get(nx, ny);
                    let river_penalty = if river_flow >= min_river_flow { 28.0 } else { 0.0 };

                    let move_cost = dist * 12.0 + mountain_penalty + river_penalty;
                    let new_cost = state.cost + move_cost as u32;

                    province_ids.set(nx, ny, state.id);
                    heap.push(ExpandState { cost: new_cost, x: nx, y: ny, id: state.id });
                }
            }
        }

        // LLOYD RELAXATION: Move seeds to the exact center-of-mass of each province!
        if !is_last_pass {
            let mut sum_x = vec![0.0f64; num_seeds + 1];
            let mut sum_y = vec![0.0f64; num_seeds + 1];
            let mut counts = vec![0usize; num_seeds + 1];

            for y in 0..h {
                for x in 0..w {
                    let id = *province_ids.get(x as isize, y as isize) as usize;
                    if id != 0 && id <= num_seeds {
                        sum_x[id] += x as f64;
                        sum_y[id] += y as f64;
                        counts[id] += 1;
                    }
                }
            }

            for idx in 0..num_seeds {
                let id = idx + 1;
                if counts[id] > 0 {
                    let mut avg_x = (sum_x[id] / counts[id] as f64).round() as isize;
                    let mut avg_y = (sum_y[id] / counts[id] as f64).round() as isize;
                    avg_x = heightmap.wrap_x(avg_x) as isize;
                    avg_y = avg_y.clamp(0, h as isize - 1);

                    // Ensure seed lands on valid land
                    if *heightmap.get(avg_x, avg_y) > sea_level {
                        seeds[idx] = (avg_x, avg_y);
                    }
                }
            }
        }
    }

    ProvinceResult {
        province_ids,
        habitability: hab_grid,
    }
}
```


# File: src/geopolitics/clustering.rs

```rust
use crate::geopolitics::hierarchy::{GeopoliticalHierarchy, ProvinceNode};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Copy, Clone, Eq, PartialEq)]
struct DijkstraState {
    cost: u32,
    id: u32,
    cid: u32,
}

impl Ord for DijkstraState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost).then_with(|| self.id.cmp(&other.id))
    }
}
impl PartialOrd for DijkstraState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn cluster_tier(
    provinces: &[ProvinceNode],
    parent_ids: &[u32],
    target_cluster_size: usize,
    max_clusters_per_parent: usize,
    seed: u64,
) -> (Vec<u32>, usize) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let n = provinces.len();
    let mut assignments = vec![0u32; n];

    let mut parent_groups: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in provinces.iter() {
        if p.id == 0 || p.pixel_count == 0 { continue; }
        let parent = parent_ids[p.id as usize];
        parent_groups.entry(parent).or_default().push(p.id);
    }

    let mut global_counter = 1u32;

    for (_parent, members) in parent_groups {
        let count = members.len();
        if count == 0 { continue; }

        let k = ((count as f32 / target_cluster_size as f32).round() as usize)
            .clamp(1, max_clusters_per_parent);

        if k == 1 {
            for &m in &members { assignments[m as usize] = global_counter; }
            global_counter += 1;
        } else {
            let mut seeds = Vec::with_capacity(k);
            seeds.push(*members.choose(&mut rng).unwrap());
            let member_set: HashSet<u32> = members.iter().copied().collect();

            // K-Centers Farthest Point
            for _ in 1..k {
                let mut heap = BinaryHeap::new();
                let mut min_costs = HashMap::new();
                for &s in &seeds {
                    heap.push(DijkstraState { cost: 0, id: s, cid: s });
                    min_costs.insert(s, 0);
                }

                let mut farthest = seeds[0];
                let mut max_cost = 0;

                while let Some(state) = heap.pop() {
                    if state.cost > min_costs[&state.id] { continue; }
                    if state.cost > max_cost {
                        max_cost = state.cost;
                        farthest = state.id;
                    }
                    for &(nb, cost) in &provinces[state.id as usize].neighbors {
                        if !member_set.contains(&nb) { continue; }
                        let new_cost = state.cost + cost;
                        if new_cost < *min_costs.get(&nb).unwrap_or(&u32::MAX) {
                            min_costs.insert(nb, new_cost);
                            heap.push(DijkstraState { cost: new_cost, id: nb, cid: state.cid });
                        }
                    }
                }
                seeds.push(farthest);
            }

            // Multi-source Dijkstra Expansion
            let mut heap = BinaryHeap::new();
            let mut best_cost = HashMap::new();
            for (idx, &s) in seeds.iter().enumerate() {
                let cid = global_counter + idx as u32;
                assignments[s as usize] = cid;
                best_cost.insert(s, 0);
                heap.push(DijkstraState { cost: 0, id: s, cid });
            }

            while let Some(state) = heap.pop() {
                if best_cost.get(&state.id).copied().unwrap_or(u32::MAX) < state.cost { continue; }
                for &(nb, cost) in &provinces[state.id as usize].neighbors {
                    if !member_set.contains(&nb) { continue; }
                    let new_cost = state.cost + cost;
                    if new_cost < *best_cost.get(&nb).unwrap_or(&u32::MAX) {
                        best_cost.insert(nb, new_cost);
                        assignments[nb as usize] = state.cid;
                        heap.push(DijkstraState { cost: new_cost, id: nb, cid: state.cid });
                    }
                }
            }

            global_counter += k as u32;
        }
    }

    (assignments, (global_counter - 1) as usize)
}

pub fn build_geopolitical_hierarchy(
    mut provinces: Vec<ProvinceNode>,
    seed: u64,
) -> GeopoliticalHierarchy {
    let n = provinces.len();

    // 1. Continents are already solved in graph.rs!
    let continent_ids: Vec<u32> = provinces.iter().map(|p| p.continent_id).collect();
    let mut continent_set = HashSet::new();
    for &c in &continent_ids { if c != 0 { continent_set.insert(c); } }
    let total_continents = continent_set.len();
    println!("✓ Identified {} true Continents (including Oceania & adopted coastal islands).", total_continents);

    // 2. Superregions (5 to 8 per large continent)
    println!("Clustering Superregions (Strictly inside Continents)...");
    let (super_ids, total_superregions) = cluster_tier(
        &provinces,
        &continent_ids,
        900, // ~900 provinces per superregion
        7,   // Max 7 superregions per continent!
        seed.wrapping_add(3),
    );
    for i in 1..n { provinces[i].superregion_id = super_ids[i]; }
    println!("✓ Generated {} Superregions worldwide.", total_superregions);

    // 3. Regions (Kingdom level: ~35 provinces per region)
    println!("Clustering Regions (Strictly inside Superregions)...");
    let (region_ids, total_regions) = cluster_tier(
        &provinces,
        &super_ids,
        35,
        30,
        seed.wrapping_add(2),
    );
    for i in 1..n { provinces[i].region_id = region_ids[i]; }
    println!("✓ Generated {} Regions worldwide.", total_regions);

    // 4. Areas (State level: ~6 provinces per area)
    println!("Clustering Areas (Strictly inside Regions)...");
    let (area_ids, total_areas) = cluster_tier(
        &provinces,
        &region_ids,
        6,
        15,
        seed.wrapping_add(1),
    );
    for i in 1..n { provinces[i].area_id = area_ids[i]; }
    println!("✓ Generated {} Areas worldwide.", total_areas);

    GeopoliticalHierarchy {
        total_provinces: provinces.len() - 1,
        total_areas,
        total_regions,
        total_superregions,
        total_continents,
        provinces,
    }
}
```


# File: src/geopolitics/graph.rs

```rust
use crate::core::Grid;
use crate::geopolitics::hierarchy::ProvinceNode;
use std::collections::HashMap;

pub struct ProvinceAccumulator {
    pub count: usize,
    pub sum_cos_x: f64,
    pub sum_sin_x: f64,
    pub sum_y: f64,
}

pub struct ExtractedGraph {
    pub provinces: Vec<ProvinceNode>,
    pub max_id: u32,
}

struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(size: usize) -> Self { Self { parent: (0..size).collect() } }
    fn find(&mut self, i: usize) -> usize {
        if self.parent[i] == i { i } else {
            let p = self.parent[i];
            self.parent[i] = self.find(p);
            self.parent[i]
        }
    }
    fn union(&mut self, i: usize, j: usize) {
        let root_i = self.find(i);
        let root_j = self.find(j);
        if root_i != root_j { self.parent[root_i] = root_j; }
    }
}

pub fn extract_province_graph(province_grid: &Grid<u32>, heightmap: &Grid<f32>) -> ExtractedGraph {
    let w = province_grid.width;
    let h = province_grid.height;

    let mut max_id = 0u32;
    for &id in province_grid.raw_slice() {
        if id > max_id { max_id = id; }
    }

    let n = max_id as usize;
    let mut accumulators = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        accumulators.push(ProvinceAccumulator { count: 0, sum_cos_x: 0.0, sum_sin_x: 0.0, sum_y: 0.0 });
    }

    // HashMap stores the MINIMUM cost between two provinces (e.g. finding the lowest mountain pass!)
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();

    let mut add_edge = |edges: &mut HashMap<(u32, u32), u32>, a: u32, b: u32, cost: u32| {
        let pair = if a < b { (a, b) } else { (b, a) };
        let entry = edges.entry(pair).or_insert(u32::MAX);
        *entry = (*entry).min(cost);
    };

    for y in 0..h {
        let fy = y as f64;
        for x in 0..w {
            let id = *province_grid.get(x as isize, y as isize);
            if id == 0 { continue; }

            let angle = (x as f64 / w as f64) * std::f64::consts::TAU;
            let acc = &mut accumulators[id as usize];
            acc.count += 1;
            acc.sum_cos_x += angle.cos();
            acc.sum_sin_x += angle.sin();
            acc.sum_y += fy;

            let current_elev = *heightmap.get(x as isize, y as isize);

            // Right neighbor
            let nx_right = province_grid.wrap_x(x as isize + 1) as isize;
            let right_id = *province_grid.get(nx_right, y as isize);
            if right_id != 0 && right_id != id {
                let target_elev = *heightmap.get(nx_right, y as isize);
                let cost = 10 + ((current_elev - target_elev).abs() * 4000.0) as u32; // Mountain Penalty!
                add_edge(&mut edges, id, right_id, cost);
            }

            // Down neighbor
            let mut down_id = 0;
            if y + 1 < h {
                down_id = *province_grid.get(x as isize, y as isize + 1);
                if down_id != 0 && down_id != id {
                    let target_elev = *heightmap.get(x as isize, y as isize + 1);
                    let cost = 10 + ((current_elev - target_elev).abs() * 4000.0) as u32; // Mountain Penalty!
                    add_edge(&mut edges, id, down_id, cost);
                }
            }

            // Maritime Straits (Raycasting)
            let is_coast = right_id == 0 || down_id == 0 
                || (x > 0 && *province_grid.get(x as isize - 1, y as isize) == 0)
                || (y > 0 && *province_grid.get(x as isize, y as isize - 1) == 0);

            if is_coast {
                for i in 0..16 {
                    let theta = (i as f32 / 16.0) * std::f32::consts::TAU;
                    let step_x = theta.cos();
                    let step_y = theta.sin();
                    
                    for d in 2..=80 {
                        let nx = province_grid.wrap_x(x as isize + (step_x * d as f32).round() as isize) as isize;
                        let ny = y as isize + (step_y * d as f32).round() as isize;
                        
                        if ny >= 0 && ny < h as isize {
                            let target_id = *province_grid.get(nx, ny);
                            if target_id != 0 {
                                if target_id != id {
                                    // MASSIVE PENALTY: 4000. Prevents mainland regions from 
                                    // randomly stealing 1 province on an island!
                                    add_edge(&mut edges, id, target_id, 4000 + d as u32 * 20);
                                }
                                break; 
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }

    let mut ds = DisjointSet::new(n + 1);
    for &(a, b) in edges.keys() {
        ds.union(a as usize, b as usize);
    }

    let mut root_sizes = HashMap::new();
    for i in 1..=n {
        if accumulators[i].count > 0 {
            let root = ds.find(i);
            *root_sizes.entry(root).or_insert(0usize) += accumulators[i].count;
        }
    }

    let mut sorted_roots: Vec<_> = root_sizes.into_iter().collect();
    sorted_roots.sort_by(|a, b| b.1.cmp(&a.1));

    let mut root_to_continent_id = HashMap::new();
    for (idx, (root, _)) in sorted_roots.into_iter().enumerate() {
        root_to_continent_id.insert(root, (idx + 1) as u32);
    }

    let mut neighbor_map = vec![Vec::new(); n + 1];
    for (&(a, b), &cost) in &edges {
        neighbor_map[a as usize].push((b, cost));
        neighbor_map[b as usize].push((a, cost));
    }

    let mut provinces = Vec::with_capacity(n + 1);
    provinces.push(ProvinceNode { id: 0, center_x: 0.0, center_y: 0.0, pixel_count: 0, neighbors: vec![], area_id: 0, region_id: 0, superregion_id: 0, continent_id: 0 });

    for i in 1..=n {
        let acc = &accumulators[i];
        if acc.count == 0 {
            provinces.push(ProvinceNode { id: i as u32, center_x: 0.0, center_y: 0.0, pixel_count: 0, neighbors: vec![], area_id: 0, region_id: 0, superregion_id: 0, continent_id: 0 });
            continue;
        }

        let avg_angle = acc.sum_sin_x.atan2(acc.sum_cos_x).rem_euclid(std::f64::consts::TAU);
        let center_x = ((avg_angle / std::f64::consts::TAU) * w as f64) as f32;
        let center_y = (acc.sum_y / acc.count as f64) as f32;
        let root = ds.find(i);
        let continent_id = *root_to_continent_id.get(&root).unwrap_or(&1);

        provinces.push(ProvinceNode { id: i as u32, center_x, center_y, pixel_count: acc.count, neighbors: neighbor_map[i].clone(), area_id: 0, region_id: 0, superregion_id: 0, continent_id });
    }

    ExtractedGraph { provinces, max_id }
}
```


# File: src/geopolitics/hierarchy.rs

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvinceNode {
    pub id: u32,
    pub center_x: f32,
    pub center_y: f32,
    pub pixel_count: usize,
    pub neighbors: Vec<(u32, u32)>, // NEW: (Neighbor ID, Edge Cost)

    pub area_id: u32,
    pub region_id: u32,
    pub superregion_id: u32,
    pub continent_id: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeopoliticalHierarchy {
    pub total_provinces: usize,
    pub total_areas: usize,
    pub total_regions: usize,
    pub total_superregions: usize,
    pub total_continents: usize,
    pub provinces: Vec<ProvinceNode>,
}
```


# File: src/geopolitics/mod.rs

```rust
pub mod clustering;
pub mod graph;
pub mod hierarchy;

pub use clustering::build_geopolitical_hierarchy;
pub use graph::extract_province_graph;
pub use hierarchy::{GeopoliticalHierarchy, ProvinceNode};
```


# File: src/hydrology/config.rs

```rust
#[derive(Clone, Debug)]
pub struct HydrologyConfig {
    pub min_lake_depth: f32,
    pub min_river_flow: f32,       // Raised from 35.0 -> 160.0 to eliminate the parallel "rake" scratches!
    pub major_river_flow: f32,
    pub runoff_multiplier: f32,
}

impl Default for HydrologyConfig {
    fn default() -> Self {
        Self {
            min_lake_depth: 0.015,     // Only genuine deep tectonic rift depressions become lakes!
            min_river_flow: 180.0,
            major_river_flow: 750.0,
            runoff_multiplier: 1.0,
        }
    }
}
```


# File: src/hydrology/flow.rs

```rust
use crate::core::Grid;
use crate::hydrology::config::HydrologyConfig;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum FlowDir {
    #[default]
    None,
    Target(isize, isize),
}

pub struct FlowResult {
    pub flow_direction: Grid<FlowDir>,
    pub accumulated_flow: Grid<f32>,
}

/// Calculates D8 steepest downhill flow routing and accumulates precipitation downstream.
pub fn calculate_flow(
    filled_heights: &Grid<f32>,
    precipitation: &Grid<f32>,
    sea_level: f32,
    config: &HydrologyConfig,
) -> FlowResult {
    let w = filled_heights.width;
    let h = filled_heights.height;

    let mut flow_dir = Grid::filled_with(w, h, FlowDir::None);
    let mut in_degrees = Grid::filled_with(w, h, 0usize);
    let mut flow = Grid::filled_with(w, h, 0.0f32);

    let neighbors: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
        (-1, -1, std::f32::consts::SQRT_2), (1, -1, std::f32::consts::SQRT_2),
        (-1, 1, std::f32::consts::SQRT_2), (1, 1, std::f32::consts::SQRT_2),
    ];

    // 1. Initial surface runoff from precipitation
    for y in 0..h {
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level {
                let rain = *precipitation.get(x as isize, y as isize);
                flow.set(x as isize, y as isize, (rain * config.runoff_multiplier).max(0.05));
            }
        }
    }

    // 2. Determine D8 steepest downhill neighbor
    for y in 1..(h as isize - 1) {
        for x in 0..(w as isize) {
            let current_h = *filled_heights.get(x, y);
            if current_h <= sea_level {
                continue;
            }

            let mut best_slope = 0.0f32;
            let mut best_target = None;

            for &(dx, dy, dist) in &neighbors {
                let ny = y + dy;
                let nx = filled_heights.wrap_x(x + dx) as isize;

                let neighbor_h = *filled_heights.get(nx, ny);
                let slope = (current_h - neighbor_h) / dist;

                if slope > best_slope {
                    best_slope = slope;
                    best_target = Some((nx, ny));
                }
            }

            if let Some((tx, ty)) = best_target {
                flow_dir.set(x, y, FlowDir::Target(tx, ty));
                *in_degrees.get_mut(tx, ty) += 1;
            }
        }
    }

    // 3. Topological sort flow accumulation (Kahn's Algorithm: leaves to roots)
    let mut queue = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level && *in_degrees.get(x as isize, y as isize) == 0 {
                queue.push((x as isize, y as isize));
            }
        }
    }

    let mut head = 0;
    while head < queue.len() {
        let (cx, cy) = queue[head];
        head += 1;

        if let FlowDir::Target(tx, ty) = *flow_dir.get(cx, cy) {
            let current_water = *flow.get(cx, cy);
            *flow.get_mut(tx, ty) += current_water;

            let in_deg = in_degrees.get_mut(tx, ty);
            *in_deg -= 1;
            if *in_deg == 0 && *filled_heights.get(tx, ty) > sea_level {
                queue.push((tx, ty));
            }
        }
    }

    FlowResult {
        flow_direction: flow_dir,
        accumulated_flow: flow,
    }
}
```


# File: src/hydrology/mod.rs

```rust
pub mod config;
pub mod flow;
pub mod pit_fill;
pub mod rivers;

pub use config::HydrologyConfig;
pub use flow::calculate_flow;
pub use pit_fill::fill_depressions_and_find_lakes;
pub use rivers::extract_rivers_and_basins;
```


# File: src/hydrology/pit_fill.rs

```rust
use crate::core::Grid;
use crate::hydrology::config::HydrologyConfig;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, PartialEq)]
struct QueueItem {
    height: f32,
    x: isize,
    y: isize,
}

impl Eq for QueueItem {}

// Reverse ordering to make BinaryHeap act as a Min-Heap
impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .height
            .partial_cmp(&self.height)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct PitFillResult {
    pub filled_heightmap: Grid<f32>,
    pub is_lake: Grid<bool>,
    pub lake_depth: Grid<f32>,
}

/// Simulates water filling land depressions using the Barnes Priority-Flood algorithm.
/// Guarantees that all terrain slopes downhill to an ocean pour point.
pub fn fill_depressions_and_find_lakes(
    heightmap: &Grid<f32>,
    sea_level: f32,
    config: &HydrologyConfig,
) -> PitFillResult {
    let w = heightmap.width;
    let h = heightmap.height;

    let mut filled = heightmap.clone();
    let mut visited = Grid::filled_with(w, h, false);
    let mut is_lake = Grid::filled_with(w, h, false);
    let mut lake_depth = Grid::filled_with(w, h, 0.0f32);
    let mut heap = BinaryHeap::new();

    // 1. Initialize heap with all ocean cells and polar boundaries
    for y in 0..h {
        for x in 0..w {
            let elev = *heightmap.get(x as isize, y as isize);
            if elev <= sea_level || y == 0 || y == h - 1 {
                visited.set(x as isize, y as isize, true);
                heap.push(QueueItem {
                    height: elev,
                    x: x as isize,
                    y: y as isize,
                });
            }
        }
    }

    let neighbors: [(isize, isize); 8] = [
        (-1, 0), (1, 0), (0, -1), (0, 1),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];

    // 2. Priority flood across terrain
    while let Some(current) = heap.pop() {
        for &(dx, dy) in &neighbors {
            let ny = current.y + dy;
            if ny < 0 || ny >= h as isize {
                continue;
            }
            let nx = heightmap.wrap_x(current.x + dx) as isize;

            if !*visited.get(nx, ny) {
                visited.set(nx, ny, true);

                let original_h = *heightmap.get(nx, ny);
                let filled_h = original_h.max(current.height);
                filled.set(nx, ny, filled_h);

                // Detect inland lake (ONLY deep, major geological depressions become lakes!)
                let depth = filled_h - original_h;
                let ny_norm = ny as f32 / h as f32;
                let is_polar = ny_norm < 0.12 || ny_norm > 0.88;

                if depth > 0.012 && original_h > sea_level && !is_polar {
                    is_lake.set(nx, ny, true);
                    lake_depth.set(nx, ny, depth);
                }

                heap.push(QueueItem {
                    height: filled_h,
                    x: nx,
                    y: ny,
                });
            }
        }
    }

    PitFillResult {
        filled_heightmap: filled,
        is_lake,
        lake_depth,
    }
}
```


# File: src/hydrology/rivers.rs

```rust
use crate::core::Grid;
use crate::hydrology::config::HydrologyConfig;
use crate::hydrology::flow::{FlowDir, FlowResult};

pub struct RiverResult {
    pub is_river: Grid<bool>,
    pub river_width: Grid<f32>,
    pub basin_id: Grid<usize>,
}

/// Extracts river channels with dynamic widths and delineates continental drainage basins.
pub fn extract_rivers_and_basins(
    filled_heights: &Grid<f32>,
    flow_res: &FlowResult,
    sea_level: f32,
    config: &HydrologyConfig,
) -> RiverResult {
    let w = filled_heights.width;
    let h = filled_heights.height;

    let mut is_river = Grid::filled_with(w, h, false);
    let mut river_width = Grid::filled_with(w, h, 0.0f32);
    let mut basin_id = Grid::filled_with(w, h, 0usize);

    // 1. Identify river channels based on accumulated flow
    for y in 0..h {
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level {
                let water = *flow_res.accumulated_flow.get(x as isize, y as isize);
                if water >= config.min_river_flow {
                    is_river.set(x as isize, y as isize, true);
                    // River width scales with square root of flow volume
                    let normalized_width = ((water - config.min_river_flow) / config.major_river_flow).sqrt().clamp(0.2, 3.5);
                    river_width.set(x as isize, y as isize, normalized_width);
                }
            }
        }
    }

    // 2. Identify Ocean Pour Points (River Mouths) & Assign Drainage Basins
    let mut current_basin = 1usize;
    let mut ocean_mouths = Vec::new();

    for y in 0..h {
        for x in 0..w {
            let elev = *filled_heights.get(x as isize, y as isize);
            if elev > sea_level {
                if let FlowDir::Target(tx, ty) = *flow_res.flow_direction.get(x as isize, y as isize) {
                    if *filled_heights.get(tx, ty) <= sea_level {
                        // This land tile directly pours into the ocean!
                        basin_id.set(x as isize, y as isize, current_basin);
                        ocean_mouths.push((x as isize, y as isize, current_basin));
                        current_basin += 1;
                    }
                }
            }
        }
    }

    RiverResult {
        is_river,
        river_width,
        basin_id,
    }
}
```


# File: src/main.rs

```rust
mod climate;
mod core;
mod export;
mod geography;
mod hydrology;
mod terrain;
mod geopolitics;
mod viewer;

use climate::{simulate_moisture, simulate_temperature, simulate_winds, ClimateConfig};
use geopolitics::{build_geopolitical_hierarchy, extract_province_graph};
use core::WorldCache;
use geography::{generate_provinces, GeographyConfig};
use hydrology::{
    calculate_flow, extract_rivers_and_basins, fill_depressions_and_find_lakes, HydrologyConfig,
};
use rand::RngExt;
use std::env;
use std::path::Path;
use std::time::Instant;
use terrain::{
    apply_fractal_detail, simulate_hydraulic_erosion, simulate_tectonics, simulate_thermal_erosion,
    WorldConfig,
};

fn main() {
    println!("=======================================================");
    println!("   Project Genesis: High-Definition Physical World Engine  ");
    println!("=======================================================");

    // Parse CLI arguments: `cargo run --release -- 1980 --save`
    let mut seed = 42;
    let mut save_cache = false;
    let mut load_cache = false;

    let mut run_viewer = false;

    for arg in env::args().skip(1) {
        if arg == "--save" { save_cache = true; } 
        else if arg == "--load" { load_cache = true; }
        else if arg == "--viewer" { run_viewer = true; } // NEW!
        else if let Ok(parsed_seed) = arg.parse::<u64>() { seed = parsed_seed; }
    }

    let world_config = WorldConfig {
        width: 8192,
        height: 4096,
        seed,
        num_plates: 32,
        ocean_ratio: 0.65,
        erosion_droplets: 3_500_000,
        ..Default::default()
    };
    let climate_config = ClimateConfig { seed, ..Default::default() };
    let hydro_config = HydrologyConfig { min_lake_depth: 0.015, min_river_flow: 1800.0, major_river_flow: 15000.0, ..Default::default() };
    let geo_config = GeographyConfig::default();

    println!("Active Seed: {} | Canvas: 8192x4096", seed);
    let cache_path = format!("world_{}.bin", seed);

    let total_start = Instant::now();

    // --- CACHE ARCHITECTURE: STAGES 1 TO 3 ---
    let cache = if load_cache && Path::new(&cache_path).exists() {
        let load_start = Instant::now();
        println!("Loading world from cache '{}'...", cache_path);
        let loaded = WorldCache::load(&cache_path);
        println!("✓ Loaded cached world data in:             {:.2?}", load_start.elapsed());
        loaded
    } else {
        let t_start = Instant::now();
        let mut result = simulate_tectonics(&world_config);
        apply_fractal_detail(&mut result.heightmap, &world_config);
        simulate_hydraulic_erosion(&mut result.heightmap, &world_config);
        simulate_thermal_erosion(&mut result.heightmap, &world_config);
        println!("✓ [1/3] Geology & Erosion completed:       {:.2?}", t_start.elapsed());

        let c_start = Instant::now();
        let temp_grid = simulate_temperature(&result.heightmap, 0.40, &climate_config);
        let winds = simulate_winds(world_config.width, world_config.height, &climate_config);
        let moisture = simulate_moisture(&result.heightmap, &temp_grid, &winds, 0.40, &climate_config);
        println!("✓ [2/3] Atmospheric Climate completed:     {:.2?}", c_start.elapsed());

        let h_start = Instant::now();
        let pit_res = fill_depressions_and_find_lakes(&result.heightmap, 0.40, &hydro_config);
        let flow_res = calculate_flow(&pit_res.filled_heightmap, &moisture.precipitation, 0.40, &hydro_config);
        // Rivers extracted here if needed later
        println!("✓ [3/3] Hydrology & River routing done:    {:.2?}", h_start.elapsed());

        let new_cache = WorldCache {
            width: world_config.width,
            height: world_config.height,
            heightmap: result.heightmap,
            temp_grid,
            precip_grid: moisture.precipitation,
            flow_grid: flow_res.accumulated_flow,
            is_lake: pit_res.is_lake,
        };

        if save_cache {
            println!("Saving world state to cache '{}'...", cache_path);
            new_cache.save(&cache_path);
        }
        new_cache
    };

    // --- STAGE 4: POLITICAL GEOGRAPHY (PROVINCES) ---
    let p_start = Instant::now();
    let prov_res = generate_provinces(
        &cache.heightmap,
        &cache.temp_grid,
        &cache.precip_grid,
        &cache.flow_grid,
        &cache.is_lake,
        0.40, // THE BUG FIX: Physical sea level is 0.40, NOT 0.11!!!
        hydro_config.min_river_flow,
        &geo_config,
        seed,
    );
    println!("✓ [4/4] Geography & Provinces done:        {:.2?}", p_start.elapsed());

    // --- STAGE 5: GEOPOLITICAL HIERARCHY (AREAS, REGIONS, CONTINENTS) ---
    let g_start = Instant::now();
    let extracted = extract_province_graph(&prov_res.province_ids, &cache.heightmap);
    let hierarchy = build_geopolitical_hierarchy(extracted.provinces, seed);
    println!("✓ [5/5] Geopolitical Hierarchy completed:   {:.2?}", g_start.elapsed());

    println!("-------------------------------------------------------");
    println!("Total Generation Time:                     {:.2?}", total_start.elapsed());
    println!("-------------------------------------------------------");

    // --- STAGE 6: LAUNCH INTERACTIVE MAP VIEWER ---
    if run_viewer {
        println!("Launching Interactive 2D Viewer...");
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
            ..Default::default()
        };
        
        eframe::run_native(
            "Project Genesis - Atlas Viewer",
            options,
            Box::new(|_cc| Ok(Box::new(viewer::MapViewer::new(
                cache.clone(), 
                hierarchy.clone(), 
                prov_res.clone()
            )))),
        ).unwrap();
    }

    let exp_start = Instant::now();
    export::save_hierarchical_political_map(
        &cache.heightmap,
        &cache.temp_grid,
        &cache.precip_grid,
        &cache.flow_grid,
        &cache.is_lake,
        &hierarchy,
        &prov_res.province_ids,
        0.40,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_political_map.png",
    ).expect("Failed to export political map");

    println!("✓ Rendered Hierarchical Political Map in:   {:.2?}", exp_start.elapsed());
    println!("=======================================================");
}
```


# File: src/terrain/config.rs

```rust
#[derive(Clone, Debug)]
pub struct WorldConfig {
    pub width: usize,
    pub height: usize,
    pub seed: u64,

    // Tectonic plate parameters
    pub num_plates: usize,
    pub ocean_ratio: f32,
    pub continental_base_elev: f32,
    pub oceanic_base_elev: f32,
    pub mountain_height: f32,
    pub trench_depth: f32,
    pub boundary_spread: f32,

    // Hydraulic Erosion parameters (NEW)
    pub erosion_droplets: usize, // e.g. 150_000 droplets
    pub erosion_inertia: f32,    // 0.0 (no inertia) to 1.0 (droplet moves in straight line)
    pub sediment_capacity_factor: f32, // Higher = droplets carry and carve more rock
    pub min_sediment_capacity: f32,
    pub erode_speed: f32,        // Speed of picking up soil
    pub deposit_speed: f32,      // Speed of depositing sediment in valleys
    pub evaporation_rate: f32,   // Water evaporation per step
    pub max_droplet_lifetime: usize, // Max movement steps per droplet

    // Thermal weathering parameters (NEW)
    pub thermal_iterations: usize, // e.g. 4 passes across the map
    pub talus_threshold: f32,      // Max slope before rock slips downhill (e.g. 0.025)
    pub thermal_rate: f32,         // Fraction of excess material that slides down (e.g. 0.15)
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 512,
            seed: 42,
            num_plates: 28,
            ocean_ratio: 0.65,
            continental_base_elev: 0.58,
            oceanic_base_elev: 0.22,
            mountain_height: 0.35,
            trench_depth: 0.18,
            boundary_spread: 28.0,

            // Default erosion values for crisp geological weathering
            erosion_droplets: 120_000,
            erosion_inertia: 0.15,
            sediment_capacity_factor: 4.0,
            min_sediment_capacity: 0.01,
            erode_speed: 0.3,
            deposit_speed: 0.3,
            evaporation_rate: 0.02,
            max_droplet_lifetime: 40,

            // Thermal defaults
            thermal_iterations: 4,
            talus_threshold: 0.025,
            thermal_rate: 0.15,
        }
    }
}
```


# File: src/terrain/erosion.rs

```rust
use crate::core::Grid;
use crate::terrain::config::WorldConfig;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

/// Distributes an erosion/deposition delta across the 4 neighboring grid vertices
/// using bilinear weights so terrain changes are smooth and crack-free.
#[inline(always)]
fn distribute_delta(heightmap: &mut Grid<f32>, fx: f32, fy: f32, delta: f32) {
    let x0 = fx.floor() as isize;
    let y0 = fy.floor() as isize;
    let x1 = x0 + 1;
    let y1 = y0 + 1;

    let tx = fx - fx.floor();
    let ty = fy - fy.floor();

    let w00 = (1.0 - tx) * (1.0 - ty);
    let w10 = tx * (1.0 - ty);
    let w01 = (1.0 - tx) * ty;
    let w11 = tx * ty;

    let h00 = *heightmap.get(x0, y0);
    let h10 = *heightmap.get(x1, y0);
    let h01 = *heightmap.get(x0, y1);
    let h11 = *heightmap.get(x1, y1);

    heightmap.set(x0, y0, h00 + delta * w00);
    heightmap.set(x1, y0, h10 + delta * w10);
    heightmap.set(x0, y1, h01 + delta * w01);
    heightmap.set(x1, y1, h11 + delta * w11);
}

pub fn simulate_hydraulic_erosion(heightmap: &mut Grid<f32>, config: &WorldConfig) {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed.wrapping_add(9999));
    let w = config.width as f32;
    let h = config.height as f32;

    for _ in 0..config.erosion_droplets {
        // Spawn droplet randomly across the world
        let mut px = rng.random_range(0.0..w);
        let mut py = rng.random_range(1.0..h - 2.0);

        // Optimization: don't waste time on deep underwater ocean abysses
        if heightmap.sample_bilinear(px, py) < 0.38 {
            continue;
        }

        let mut dir_x = 0.0;
        let mut dir_y = 0.0;
        let mut speed = 1.0;
        let mut water = 1.0;
        let mut sediment = 0.0;

        for _ in 0..config.max_droplet_lifetime {
            let (gx, gy) = heightmap.gradient(px, py);

            // Compute new flow direction with inertia: inertia * old_dir + (1 - inertia) * downhill_gradient
            dir_x = dir_x * config.erosion_inertia - gx * (1.0 - config.erosion_inertia);
            dir_y = dir_y * config.erosion_inertia - gy * (1.0 - config.erosion_inertia);

            let len = (dir_x * dir_x + dir_y * dir_y).sqrt();
            if len < 1e-5 {
                break; // Flat depression or sinkhole
            }
            dir_x /= len;
            dir_y /= len;

            let old_px = px;
            let old_py = py;

            // Move droplet
            px += dir_x;
            py += dir_y;

            // Cylindrical wrapping on X, pole check on Y
            if px < 0.0 { px += w; }
            if px >= w { px -= w; }
            if py < 1.0 || py >= h - 2.0 {
                break;
            }

            let old_height = heightmap.sample_bilinear(old_px, old_py);
            let new_height = heightmap.sample_bilinear(px, py);
            let height_diff = new_height - old_height;

            // Calculate sediment carrying capacity
            // Faster water on steeper slopes carries dramatically more soil
            let capacity = ((-height_diff).max(0.0) * speed * water * config.sediment_capacity_factor)
                .max(config.min_sediment_capacity);

            if sediment > capacity || height_diff > 0.0 {
                // Moving uphill or carrying too much sediment: DEPOSIT
                let deposit_amount = if height_diff > 0.0 {
                    // Fill sinkholes
                    sediment.min(height_diff)
                } else {
                    (sediment - capacity) * config.deposit_speed
                };
                sediment -= deposit_amount;
                distribute_delta(heightmap, old_px, old_py, deposit_amount);
            } else {
                // ERODE rock from current position
                let erode_amount = ((capacity - sediment) * config.erode_speed).min(-height_diff);
                sediment += erode_amount;
                distribute_delta(heightmap, old_px, old_py, -erode_amount);
            }

            // Update droplet physics
            speed = (speed * speed + height_diff.abs() * 4.0).sqrt();
            water *= 1.0 - config.evaporation_rate;

            if water < 0.01 {
                break;
            }
        }
    }
}
```


# File: src/terrain/fractal.rs

```rust
use crate::core::Grid;
use crate::terrain::config::WorldConfig;

pub struct FractalDetailer;

impl FractalDetailer {
    pub fn new(_seed: u32) -> Self {
        Self
    }
}

/// Applies gentle micro-topography without modifying macro continental shapes.
pub fn apply_fractal_detail(
    _heightmap: &mut Grid<f32>,
    _config: &WorldConfig,
) {
    // Topography is generated in full fidelity directly by the 3D spherical engine!
}
```


# File: src/terrain/mod.rs

```rust
pub mod config;
pub mod erosion;
pub mod fractal;
pub mod tectonics;
pub mod thermal;

pub use config::WorldConfig;
pub use erosion::simulate_hydraulic_erosion;
pub use fractal::apply_fractal_detail;
pub use tectonics::simulate_tectonics;
pub use thermal::simulate_thermal_erosion;
```


# File: src/terrain/tectonics.rs

```rust
use crate::core::Grid;
use crate::terrain::config::WorldConfig;
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*; // Required for High-Definition speed

pub struct TectonicsResult {
    pub heightmap: Grid<f32>,
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

pub fn simulate_tectonics(config: &WorldConfig) -> TectonicsResult {
    let w = config.width;
    let h = config.height;

    let continent_noise = OpenSimplex::new(config.seed as u32);
    let warp_x = OpenSimplex::new(config.seed.wrapping_add(101) as u32);
    let warp_y = OpenSimplex::new(config.seed.wrapping_add(202) as u32);
    let warp_z = OpenSimplex::new(config.seed.wrapping_add(303) as u32);
    let mountain_noise = OpenSimplex::new(config.seed.wrapping_add(404) as u32);
    let fault_noise = OpenSimplex::new(config.seed.wrapping_add(505) as u32);

    let mut heightmap = Grid::new(w, h);

    // PARALLELIZE THE ENTIRE GLOBE ACROSS ALL CPU CORES
    heightmap.raw_slice_mut().par_chunks_exact_mut(w).enumerate().for_each(|(y, row)| {
        let fy = y as f32;
        let v = fy / (h - 1) as f32;
        let lat = (0.5 - v) * std::f32::consts::PI;
        let cos_lat = lat.cos() as f64;
        let sin_lat = lat.sin() as f64;

        for x in 0..w {
            let fx = x as f32;
            let u = fx / w as f32;
            let lon = (u - 0.5) * std::f32::consts::TAU;

            let px = cos_lat * (lon as f64).cos();
            let py = sin_lat;
            let pz = cos_lat * (lon as f64).sin();

            // Slightly stronger warp for jagged, realistic coastlines (HD: 6 octaves)
            let wx = sample_3d_fbm(&warp_x, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * 0.32;
            let wy = sample_3d_fbm(&warp_y, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * 0.32;
            let wz = sample_3d_fbm(&warp_z, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * 0.32;

            let sx = px + wx as f64;
            let sy = py + wy as f64;
            let sz = pz + wz as f64;

            // The exact frequency and threshold that generated the perfect continents!
            // HD UPGRADE: Raised from 6 to 9 octaves for millimeter-level coastal detail!
            let continent_val = sample_3d_fbm(&continent_noise, sx * 1.05, sy * 1.05, sz * 1.05, 9, 2.1, 0.52);
            let sea_threshold = 0.11f32; 
            
            let mut elev: f32;

            if continent_val > sea_threshold {
                let land_t = (continent_val - sea_threshold) / (1.0 - sea_threshold);
                let base_plain = 0.415 + land_t.powf(1.3) * 0.065;

                // CONTINUOUS FAULT LINES (The Andes / Rockies system)
                let fault_val = sample_3d_fbm(&fault_noise, sx * 1.6, sy * 1.6, sz * 1.6, 4, 2.0, 0.5);
                let fault_dist = fault_val.abs();
                
                let mut mountain_lift = 0.0;
                if fault_dist < 0.10 && land_t > 0.05 {
                    // HD Mountains: Raised to 9 octaves for extreme craggy details
                    let m_sample = sample_3d_fbm(&mountain_noise, sx * 4.0, sy * 4.0, sz * 4.0, 9, 2.2, 0.48);
                    let ridge = (1.0 - m_sample.abs()).powf(3.0);
                    let fault_intensity = (1.0 - (fault_dist / 0.10)).powf(1.5);
                    mountain_lift = ridge * fault_intensity * 0.45;
                }

                // Add rolling hills to plains (HD: 6 octaves)
                let hills = sample_3d_fbm(&mountain_noise, sx * 4.5, sy * 4.5, sz * 4.5, 6, 2.0, 0.5) * 0.035;

                elev = base_plain + mountain_lift + hills;
            } else {
                let ocean_t = ((sea_threshold - continent_val) / (1.0 + sea_threshold)).clamp(0.0, 1.0);
                elev = 0.395 - ocean_t.powf(0.7) * 0.26;
                // Submarine ridges
                let ocean_ridge = (1.0 - (sample_3d_fbm(&mountain_noise, px * 2.5, py * 2.5, pz * 2.5, 5, 2.0, 0.5)).abs()).powf(5.0);
                elev += ocean_ridge * 0.045;
            }

            row[x] = elev.clamp(0.04, 1.0);
        }
    });

    TectonicsResult { heightmap }
}
```


# File: src/terrain/thermal.rs

```rust
use crate::core::Grid;
use crate::terrain::config::WorldConfig;

/// Simulates thermal weathering and talus slope relaxation.
/// Whenever the slope between neighboring cells exceeds the critical angle of repose,
/// rock material slumps downhill to form natural scree slopes.
pub fn simulate_thermal_erosion(heightmap: &mut Grid<f32>, config: &WorldConfig) {
    let w = config.width;
    let h = config.height;

    // 8-directional neighbor offsets (dx, dy)
    let neighbors: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0),
        (1, 0, 1.0),
        (0, -1, 1.0),
        (0, 1, 1.0),
        (-1, -1, std::f32::consts::SQRT_2),
        (1, -1, std::f32::consts::SQRT_2),
        (-1, 1, std::f32::consts::SQRT_2),
        (1, 1, std::f32::consts::SQRT_2),
    ];

    for _ in 0..config.thermal_iterations {
        let mut delta_grid = Grid::filled_with(w, h, 0.0f32);

        for y in 1..(h as isize - 1) {
            for x in 0..(w as isize) {
                let current_h = *heightmap.get(x, y);

                let mut max_slope_diff = 0.0f32;
                let mut total_excess = 0.0f32;
                let mut target_count = 0;

                for &(dx, dy, dist) in &neighbors {
                    let nx = x + dx;
                    let ny = y + dy;
                    let neighbor_h = *heightmap.get(nx, ny);

                    let diff = current_h - neighbor_h;
                    let slope = diff / dist;

                    if slope > config.talus_threshold {
                        let excess = (slope - config.talus_threshold) * dist;
                        total_excess += excess;
                        target_count += 1;
                        if diff > max_slope_diff {
                            max_slope_diff = diff;
                        }
                    }
                }

                // If unstable, distribute material to lower neighbors
                if target_count > 0 && max_slope_diff > 0.0 {
                    let move_amount = (total_excess / target_count as f32) * config.thermal_rate;
                    let move_per_neighbor = move_amount / target_count as f32;

                    *delta_grid.get_mut(x, y) -= move_amount;

                    for &(dx, dy, dist) in &neighbors {
                        let nx = x + dx;
                        let ny = y + dy;
                        let neighbor_h = *heightmap.get(nx, ny);
                        let slope = (current_h - neighbor_h) / dist;

                        if slope > config.talus_threshold {
                            *delta_grid.get_mut(nx, ny) += move_per_neighbor;
                        }
                    }
                }
            }
        }

        // Apply deltas back to heightmap
        for y in 0..h {
            for x in 0..w {
                let d = *delta_grid.get(x as isize, y as isize);
                let current = heightmap.get_mut(x as isize, y as isize);
                *current = (*current + d).clamp(0.01, 1.0);
            }
        }
    }
}
```


# File: src/viewer.rs

```rust
use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use rayon::prelude::*;
use crate::core::{Grid, WorldCache};
use crate::geography::ProvinceResult;
use crate::geopolitics::GeopoliticalHierarchy;
use crate::export::image::id_to_territory_color;

#[derive(PartialEq, Clone, Copy)]
enum MapMode {
    Areas,
    Regions,
    Superregions,
    Continents,
}

pub struct MapViewer {
    cache: WorldCache,
    hierarchy: GeopoliticalHierarchy,
    province_ids: Grid<u32>,
    mode: MapMode,
    texture: Option<TextureHandle>,
    zoom: f32,
    offset: egui::Vec2,
}

impl MapViewer {
    pub fn new(cache: WorldCache, hierarchy: GeopoliticalHierarchy, prov_res: ProvinceResult) -> Self {
        Self {
            cache,
            hierarchy,
            province_ids: prov_res.province_ids,
            mode: MapMode::Regions,
            texture: None,
            zoom: 0.15,
            offset: egui::vec2(0.0, 0.0),
        }
    }

    fn generate_texture(&self) -> ColorImage {
        let w = self.cache.width;
        let h = self.cache.height;
        
        let mut pixels = vec![0u8; w * h * 4];

        pixels.par_chunks_exact_mut(4).enumerate().for_each(|(i, pixel)| {
            let x = i % w;
            let y = i / w;
            let id = *self.province_ids.get(x as isize, y as isize);
            
            if id == 0 {
                // Ocean
                pixel[0] = 25; pixel[1] = 60; pixel[2] = 110; pixel[3] = 255;
            } else {
                let node = &self.hierarchy.provinces[id as usize];
                let color_id = match self.mode {
                    MapMode::Areas => node.area_id,
                    MapMode::Regions => node.region_id,
                    MapMode::Superregions => node.superregion_id,
                    MapMode::Continents => node.continent_id,
                };
                let rgb = id_to_territory_color(color_id);
                pixel[0] = rgb[0] as u8; 
                pixel[1] = rgb[1] as u8; 
                pixel[2] = rgb[2] as u8; 
                pixel[3] = 255;
            }
        });

        ColorImage::from_rgba_unmultiplied([w, h], &pixels)
    }
}

impl eframe::App for MapViewer {
    // FIXED: The new 0.36+ App trait signature!
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut mode_changed = false;

        // FIXED: New Panel API
        egui::Panel::top("controls").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🌍 Project Genesis Viewer");
                ui.separator();
                if ui.selectable_value(&mut self.mode, MapMode::Areas, "Areas (States)").clicked() { mode_changed = true; }
                if ui.selectable_value(&mut self.mode, MapMode::Regions, "Regions (Kingdoms)").clicked() { mode_changed = true; }
                if ui.selectable_value(&mut self.mode, MapMode::Superregions, "Superregions").clicked() { mode_changed = true; }
                if ui.selectable_value(&mut self.mode, MapMode::Continents, "Continents").clicked() { mode_changed = true; }
                ui.separator();
                ui.label(format!("Zoom: {:.1}x", self.zoom));
            });
        });

        if self.texture.is_none() || mode_changed {
            let img = self.generate_texture();
            self.texture = Some(ui.ctx().load_texture("map", img, TextureOptions::LINEAR));
        }

        // FIXED: No CentralPanel needed anymore! We just allocate the remaining UI space directly!
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        if response.dragged() {
            self.offset += response.drag_delta();
        }

        if ui.rect_contains_pointer(rect) {
            let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old_zoom = self.zoom;
                self.zoom = (self.zoom + scroll * 0.001).clamp(0.05, 5.0);
                
                if let Some(mouse_pos) = ui.ctx().pointer_hover_pos() {
                    let zoom_ratio = self.zoom / old_zoom;
                    self.offset = mouse_pos.to_vec2() - (mouse_pos.to_vec2() - self.offset) * zoom_ratio;
                }
            }
        }

        if let Some(texture) = &self.texture {
            let map_size = egui::vec2(self.cache.width as f32 * self.zoom, self.cache.height as f32 * self.zoom);
            let map_rect = egui::Rect::from_min_size(rect.min + self.offset, map_size);
            
            let mut mesh = egui::Mesh::with_texture(texture.id());
            mesh.add_rect_with_uv(map_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            if let Some(texture) = &self.texture {
                let map_size = egui::vec2(self.cache.width as f32 * self.zoom, self.cache.height as f32 * self.zoom);
                let map_rect = egui::Rect::from_min_size(rect.min + self.offset, map_size);
                
                let mut mesh = egui::Mesh::with_texture(texture.id());
                mesh.add_rect_with_uv(map_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                
                // FIX: Clip the painter so the map never overlaps the top UI bar!
                ui.painter().with_clip_rect(rect).add(egui::Shape::mesh(mesh));
            }
        }

        // FIXED: New Tooltip API (called directly on the hover response)
        if let Some(mouse_pos) = response.hover_pos() {
            let map_x = ((mouse_pos.x - rect.min.x - self.offset.x) / self.zoom).floor() as isize;
            let map_y = ((mouse_pos.y - rect.min.y - self.offset.y) / self.zoom).floor() as isize;

            if map_x >= 0 && map_x < self.cache.width as isize && map_y >= 0 && map_y < self.cache.height as isize {
                let id = *self.province_ids.get(map_x, map_y);
                if id != 0 {
                    let node = &self.hierarchy.provinces[id as usize];
                    
                    response.on_hover_ui_at_pointer(|ui| {
                        ui.heading(format!("Province #{}", id));
                        ui.separator();
                        ui.label(format!("Area ID: {}", node.area_id));
                        ui.label(format!("Region ID: {}", node.region_id));
                        ui.label(format!("Superregion ID: {}", node.superregion_id));
                        ui.label(format!("Continent ID: {}", node.continent_id));
                        ui.separator();
                        ui.label(format!("Pixel Size: {}", node.pixel_count));
                    });
                }
            }
        }
    }
}
```
