//! Live previews: the atlas, painted while the world is still being made.
//!
//! A finished 8K plate is 8192 × 4096 RGBA — 134 MB — and the full painter takes
//! most of a second. Neither can be produced once a frame, so the atlas cannot be
//! shown the real thing while a stage runs. What it *can* be shown is this: a
//! strided sample of the grid the stage is working on, painted at
//! [`PREVIEW_WIDTH`] and handed over as a few hundred kilobytes.
//!
//! Every function here is a pure function of the grids it is given, and they are
//! called from the preview thread pool while the owning stage waits — which is
//! what makes it safe for a stage to lend out a grid it holds mutably (see
//! [`crate::core::progress`]).
//!
//! The palettes are the *printed* atlas's own wherever one exists:
//! [`hypsometric_color`] for relief and [`id_to_territory_color`] for a
//! territory, so a live plate cannot disagree with the plate that replaces it.
//! The two ramps with no printed equivalent — temperature and rainfall — are
//! defined here, and only ever tint a preview.

use crate::core::{relief, Grid};
use crate::export::image::{hypsometric_color, id_to_territory_color};
use rayon::prelude::*;

/// Width of every live preview. The height follows from the world's own aspect
/// ratio, so a preview is never stretched.
pub const PREVIEW_WIDTH: usize = 1024;

/// Direction the preview hillshade treats as its light source — the same
/// north-west key light the finished plates use.
const LIGHT_X: f32 = -0.7071;
const LIGHT_Y: f32 = -0.7071;

/// Paint a preview of a grid `source` pixels wide and tall.
///
/// `at` is asked for the colour of a *source* pixel; the plate itself is a
/// nearest-neighbour sample of that grid, because a preview is a glance at a grid
/// being written rather than a resampling of it. Returns the plate's own size
/// together with its RGBA bytes.
pub fn paint(
    source: (usize, usize),
    at: impl Fn(usize, usize) -> [u8; 3] + Sync,
) -> (usize, usize, Vec<u8>) {
    let (source_w, source_h) = (source.0.max(1), source.1.max(1));
    let width = PREVIEW_WIDTH.min(source_w).max(1);
    let height = ((source_h * width) / source_w).max(1);

    let mut rgba = vec![0u8; width * height * 4];
    rgba.par_chunks_exact_mut(4).enumerate().for_each(|(i, pixel)| {
        let x = i % width;
        let y = i / width;
        let [r, g, b] = at(x * source_w / width, y * source_h / height);
        pixel.copy_from_slice(&[r, g, b, 255]);
    });

    (width, height, rgba)
}

/// The atlas's territory palette as bytes.
#[inline(always)]
fn territory(id: u32) -> [u8; 3] {
    let [r, g, b] = id_to_territory_color(id);
    [
        (r * 255.0).clamp(0.0, 255.0) as u8,
        (g * 255.0).clamp(0.0, 255.0) as u8,
        (b * 255.0).clamp(0.0, 255.0) as u8,
    ]
}

/// Hillshade multiplier for a pixel: the finished plates' own key light, so a
/// ridge reads as a ridge in a preview too.
#[inline(always)]
fn shade(heightmap: &Grid<f32>, x: usize, y: usize) -> f32 {
    let (gx, gy) = heightmap.gradient(x as f32, y as f32);
    let slope = (gx * gx + gy * gy).sqrt();
    let hillshade = -(gx * LIGHT_X + gy * LIGHT_Y);
    (1.0 + hillshade * 5.5 - slope * 1.5).clamp(0.65, 1.38)
}

#[inline(always)]
fn shaded(color: [u8; 3], shade: f32) -> [u8; 3] {
    [
        (color[0] as f32 * shade).clamp(0.0, 255.0) as u8,
        (color[1] as f32 * shade).clamp(0.0, 255.0) as u8,
        (color[2] as f32 * shade).clamp(0.0, 255.0) as u8,
    ]
}

/// Hypsometric relief of the world at a pixel: what *Geology & Erosion* looks
/// like while it is still carving.
pub fn relief_at(heightmap: &Grid<f32>, x: usize, y: usize) -> [u8; 3] {
    let elev = *heightmap.get(x as isize, y as isize);
    shaded(hypsometric_color(relief::to_metres(elev)), shade(heightmap, x, y))
}

/// The colour of a pixel no political pass has claimed: the world keeps its own
/// relief, so a preview shows the frontier moving *across* the land rather than
/// over a blank plate.
#[inline(always)]
fn unclaimed_at(heightmap: &Grid<f32>, x: usize, y: usize) -> [u8; 3] {
    relief_at(heightmap, x, y)
}

/// A province, painted as the atlas paints territory.
pub fn province_at(heightmap: &Grid<f32>, ids: &Grid<u32>, x: usize, y: usize) -> [u8; 3] {
    let id = *ids.get(x as isize, y as isize);
    if id == 0 {
        return unclaimed_at(heightmap, x, y);
    }
    territory(id)
}

/// A province shaded by the cluster `assignment` puts it in: a hierarchy tier
/// filling in, one pass at a time.
pub fn tier_at(
    heightmap: &Grid<f32>,
    ids: &Grid<u32>,
    assignment: &[u32],
    x: usize,
    y: usize,
) -> [u8; 3] {
    let id = *ids.get(x as isize, y as isize) as usize;
    if id == 0 || id >= assignment.len() || assignment[id] == 0 {
        return unclaimed_at(heightmap, x, y);
    }
    territory(assignment[id])
}

/// A province shaded by the realm that holds it: the dawn of history spreading
/// province by province.
pub fn owner_at(
    heightmap: &Grid<f32>,
    ids: &Grid<u32>,
    owner: &[u16],
    x: usize,
    y: usize,
) -> [u8; 3] {
    let id = *ids.get(x as isize, y as isize) as usize;
    if id == 0 || id >= owner.len() || owner[id] == 0 {
        return unclaimed_at(heightmap, x, y);
    }
    territory(owner[id] as u32)
}

/// Relief with the world's water on it: lakes, and the rivers that reach the sea
/// once flow has accumulated downhill.
///
/// Both layers are optional because the two hydrology passes know different
/// things: the depression flood has found the lakes but not the rivers, and the
/// flow accumulation knows the rivers but has never been told what a lake is.
pub fn water_at(
    heightmap: &Grid<f32>,
    is_lake: Option<&Grid<bool>>,
    flow: Option<&Grid<f32>>,
    min_river_flow: f32,
    x: usize,
    y: usize,
) -> [u8; 3] {
    const LAKE: [u8; 3] = [42, 125, 185];
    const RIVER: [u8; 3] = [28, 84, 148];

    if let Some(is_lake) = is_lake {
        if *is_lake.get(x as isize, y as isize) {
            return LAKE;
        }
    }
    if let Some(flow) = flow {
        if *flow.get(x as isize, y as isize) >= min_river_flow {
            return RIVER;
        }
    }
    relief_at(heightmap, x, y)
}

/// The world with its wilderness left pale and its settled ground in relief:
/// what the corridor carver looks like as it opens a path through the waste.
pub fn wilderness_at(
    heightmap: &Grid<f32>,
    is_wasteland: &Grid<bool>,
    x: usize,
    y: usize,
) -> [u8; 3] {
    const WASTE: [u8; 3] = [96, 92, 86];
    if *is_wasteland.get(x as isize, y as isize) {
        return WASTE;
    }
    relief_at(heightmap, x, y)
}

/// Mean temperature at a pixel, on the atlas's own cold-to-hot ramp.
pub fn temperature_at(temp: &Grid<f32>, x: usize, y: usize) -> [u8; 3] {
    ramp(&RAMP_TEMPERATURE, ((*temp.get(x as isize, y as isize) + 30.0) / 65.0).clamp(0.0, 1.0))
}

/// Annual rainfall at a pixel, on a dry-sand to wet-green ramp.
pub fn precipitation_at(precip: &Grid<f32>, x: usize, y: usize) -> [u8; 3] {
    let value = (*precip.get(x as isize, y as isize)).clamp(0.0, 1.0);
    ramp(&RAMP_RAINFALL, value)
}

/// Deep violet cold to red-hot tropics, matching the Temperature scope's own
/// legend colours closely enough to be read the same way.
const RAMP_TEMPERATURE: [[u8; 3]; 6] = [
    [58, 40, 120],
    [72, 96, 190],
    [96, 176, 196],
    [122, 196, 132],
    [232, 196, 96],
    [208, 84, 64],
];

/// Sand, savanna, steppe, forest, rainforest.
const RAMP_RAINFALL: [[u8; 3]; 5] = [
    [214, 190, 132],
    [190, 196, 118],
    [126, 176, 112],
    [74, 148, 96],
    [38, 96, 84],
];

/// Sample a colour ramp at `t` in `0.0..=1.0`.
#[inline(always)]
fn ramp(stops: &[[u8; 3]], t: f32) -> [u8; 3] {
    let scaled = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let low = scaled.floor() as usize;
    let high = (low + 1).min(stops.len() - 1);
    let mix = scaled - low as f32;
    let (a, b) = (stops[low], stops[high]);
    [
        (a[0] as f32 * (1.0 - mix) + b[0] as f32 * mix) as u8,
        (a[1] as f32 * (1.0 - mix) + b[1] as f32 * mix) as u8,
        (a[2] as f32 * (1.0 - mix) + b[2] as f32 * mix) as u8,
    ]
}
