use crate::core::polar::{self, PolarConfig};
use crate::core::progress::Progress;
use crate::core::relief::{self, SEA_LEVEL};
use crate::core::Grid;
use crate::export::live;
use crate::terrain::config::WorldConfig;
use noise::{NoiseFn, OpenSimplex};
use rayon::prelude::*; // Required for High-Definition speed

/// How many bands of latitude the tectonic pass is cut into for the atlas.
///
/// The pass is embarrassingly parallel per pixel, so the only reason to cut it
/// at all is to be able to *show* it: eight pictures of the continents
/// assembling, at roughly one every half-second on an 8K canvas.
const TECTONIC_BANDS: usize = 8;

pub struct TectonicsResult {
    pub heightmap: Grid<f32>,
}

/// Hypsometric transfer applied to the raw ocean floor: `(raw depth below sea
/// level, real depth in metres)`.
///
/// The noise field generates a *shallow* bathymetry: measured over several
/// seeds, half of the world ocean sits within 0.04 heightmap units of the water
/// line and the deepest basin only reaches ≈ 0.13. Taken at face value that is a
/// globe-spanning 400 m puddle — which is exactly what the old atlas reported.
///
/// This table re-labels the raw depth axis so that the *areas* of the depth
/// bands match the hypsometric curve of an Earth-like ocean. The left-hand
/// values are the measured cumulative depths of the generated field (the share
/// of the ocean shallower than each value), the right-hand values are the depth
/// at which that same share of Earth's ocean lies:
///
/// | share of ocean | raw depth | real depth | band |
/// | :--- | :--- | :--- | :--- |
/// | ≤ 8 % | 0.0090 | −200 m | continental shelf |
/// | 14 % | 0.0148 | −1 000 m | continental slope |
/// | 26 % | 0.0246 | −3 000 m | foot of the slope |
/// | 46 % | 0.0386 | −4 000 m | abyssal plain |
/// | 71 % | 0.0555 | −5 000 m | abyssal plain |
/// | 92 % | 0.0760 | −6 000 m | deep basin |
/// | 99.3 % | 0.0960 | −8 000 m | trench |
/// | 100 % | 0.1250 | −11 000 m | hadal trench |
///
/// The transfer is strictly monotone and pinned at the water line, so it
/// re-contours the ocean floor without ever moving the coastline: the land/ocean
/// mask of the world is bit-identical to what the raw field produced. Re-measure
/// the left-hand column (e.g. with a coarse re-evaluation of the noise field) if
/// the continental noise parameters are ever retuned.
const BATHYMETRY_TRANSFER: [(f32, f32); 12] = [
    (0.0000, 0.0),
    (0.0090, 200.0),
    (0.0148, 1_000.0),
    (0.0198, 2_000.0),
    (0.0246, 3_000.0),
    (0.0386, 4_000.0),
    (0.0555, 5_000.0),
    (0.0760, 6_000.0),
    (0.0880, 7_000.0),
    (0.0960, 8_000.0),
    (0.1100, 9_000.0),
    (0.1250, 11_000.0),
];

/// Push a raw ocean-floor heightmap value through the hypsometric transfer.
///
/// Continuous and monotone over the whole raw range: depths beyond the last
/// breakpoint simply carry on down the final slope (a freak deep basin of some
/// other seed cannot run off the end of the table), and the caller's clamp keeps
/// the result inside the datum.
#[inline(always)]
fn shape_bathymetry(elev_raw: f32) -> f32 {
    let raw_depth = (SEA_LEVEL - elev_raw).max(0.0);
    let last = BATHYMETRY_TRANSFER.len() - 1;
    let i = BATHYMETRY_TRANSFER
        .partition_point(|&(d, _)| d <= raw_depth)
        .saturating_sub(1)
        .min(last - 1);
    let (d0, m0) = BATHYMETRY_TRANSFER[i];
    let (d1, m1) = BATHYMETRY_TRANSFER[i + 1];
    let metres = m0 + (raw_depth - d0) * (m1 - m0) / (d1 - d0);
    relief::from_metres(-metres)
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

pub fn simulate_tectonics(
    config: &WorldConfig,
    polar: &PolarConfig,
    progress: &Progress,
) -> TectonicsResult {
    let w = config.width;
    let h = config.height;

    // The continental field's own shape, as the panel describes it. Read once
    // into locals: the inner loop is per pixel of an 8K canvas.
    let sea_threshold = config.sea_threshold;
    let continent_freq = config.continental_freq;
    let continent_persistence = config.continental_persistence;
    let warp_amp = config.domain_warp_amp;
    let mountain_lift_max = config.mountain_lift;
    let fault_freq = config.fault_noise_freq;
    let fault_band = config.fault_band;

    let continent_noise = OpenSimplex::new(config.seed as u32);
    let warp_x = OpenSimplex::new(config.seed.wrapping_add(101) as u32);
    let warp_y = OpenSimplex::new(config.seed.wrapping_add(202) as u32);
    let warp_z = OpenSimplex::new(config.seed.wrapping_add(303) as u32);
    let mountain_noise = OpenSimplex::new(config.seed.wrapping_add(404) as u32);
    let fault_noise = OpenSimplex::new(config.seed.wrapping_add(505) as u32);
    let cap_noise = OpenSimplex::new(config.seed.wrapping_add(606) as u32);

    let mut heightmap = Grid::new(w, h);

    progress.sub_step("Continents rising", h as u64);

    // PARALLELIZE THE ENTIRE GLOBE ACROSS ALL CPU CORES — a band of latitudes at
    // a time, so that the atlas can be shown the continents assembling rather
    // than only the finished world. Every pixel is computed from the noise field
    // alone and reads no neighbour, so cutting the pass into bands moves not one
    // value by a single bit: a watched world is the same world.
    let rows_per_band = (h / TECTONIC_BANDS).max(1);
    for band in (0..h).step_by(rows_per_band) {
        let end = (band + rows_per_band).min(h);
        let rows = &mut heightmap.raw_slice_mut()[band * w..end * w];
        rows.par_chunks_exact_mut(w).enumerate().for_each(|(offset, row)| {
        let y = band + offset;
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
            let wx = sample_3d_fbm(&warp_x, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * warp_amp;
            let wy = sample_3d_fbm(&warp_y, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * warp_amp;
            let wz = sample_3d_fbm(&warp_z, px * 1.5, py * 1.5, pz * 1.5, 6, 2.0, 0.5) * warp_amp;

            let sx = px + wx as f64;
            let sy = py + wy as f64;
            let sz = pz + wz as f64;

            // The continental field: its frequency, its persistence and the sea
            // level it is read against are the panel's, so a reader can choose
            // one supercontinent or an archipelago without touching the code.
            // HD UPGRADE: Raised from 6 to 9 octaves for millimeter-level coastal detail!
            let continent_val = sample_3d_fbm(
                &continent_noise,
                sx * continent_freq,
                sy * continent_freq,
                sz * continent_freq,
                9,
                2.1,
                continent_persistence,
            );

            let mut elev_raw: f32;

            if continent_val > sea_threshold {
                let land_t = (continent_val - sea_threshold) / (1.0 - sea_threshold);
                let base_plain = 0.415 + land_t.powf(1.3) * 0.065;

                // CONTINUOUS FAULT LINES (The Andes / Rockies system)
                let fault_val = sample_3d_fbm(
                    &fault_noise,
                    sx * fault_freq,
                    sy * fault_freq,
                    sz * fault_freq,
                    4,
                    2.0,
                    0.5,
                );
                let fault_dist = fault_val.abs();
                
                let mut mountain_lift = 0.0;
                if fault_dist < fault_band && land_t > 0.05 {
                    // HD Mountains: Raised to 9 octaves for extreme craggy details
                    let m_sample = sample_3d_fbm(&mountain_noise, sx * 4.0, sy * 4.0, sz * 4.0, 9, 2.2, 0.48);
                    let ridge = (1.0 - m_sample.abs()).powf(3.0);
                    let fault_intensity = (1.0 - (fault_dist / fault_band)).powf(1.5);
                    mountain_lift = ridge * fault_intensity * mountain_lift_max;
                }

                // Add rolling hills to plains (HD: 6 octaves)
                let hills = sample_3d_fbm(&mountain_noise, sx * 4.5, sy * 4.5, sz * 4.5, 6, 2.0, 0.5) * 0.035;

                elev_raw = base_plain + mountain_lift + hills;
            } else {
                let ocean_t = ((sea_threshold - continent_val) / (1.0 + sea_threshold)).clamp(0.0, 1.0);
                elev_raw = 0.395 - ocean_t.powf(0.7) * 0.26;
                // Submarine ridges — broad, low-frequency swells that read as
                // mid-ocean ridges once the floor is pushed through the
                // hypsometric transfer below.
                let ocean_ridge = (1.0 - (sample_3d_fbm(&mountain_noise, px * 2.5, py * 2.5, pz * 2.5, 5, 2.0, 0.5)).abs()).powf(5.0);
                elev_raw += ocean_ridge * 0.045;
            }

            // THE VERTICAL FIX: the raw field above sea level already spans a
            // believable continental range once read through `relief`, but the
            // raw ocean floor is a shallow pancake. Push it through the
            // hypsometric transfer so the ocean gains real bathymetry — shelf,
            // slope, abyssal plain and trenches — while the water line (and
            // therefore every coastline on the map) stays exactly where it was.
            let base_elev = if elev_raw < SEA_LEVEL { shape_bathymetry(elev_raw) } else { elev_raw };

            // POLAR ICE SHEETS: both poles carry a permanent, glaciated *landmass*
            // so the atlas reads them as land (with an elevation) rather than as
            // open ocean. The cap rises smoothly from a submerged shelf just
            // below the water line, through its coastline, up to a solid, high
            // ice sheet; its edge is wobbled by an independent noise field so it
            // is an organic shore rather than a latitude circle. Only pixels
            // inside the cap are touched, so the open ocean keeps its bathymetry.
            let cap_wobble = sample_3d_fbm(&cap_noise, px * 2.6, py * 2.6, pz * 2.6, 5, 2.0, 0.5);
            let cap_t = polar.cap_blend(lat.abs() + cap_wobble * 0.10);
            let elev = if cap_t > 0.0 {
                let cap_relief = sample_3d_fbm(&mountain_noise, sx * 3.2, sy * 3.2, sz * 3.2, 6, 2.0, 0.5) * 0.045;
                let cap_surface = polar::POLAR_CAP_BASE_ELEV
                    + cap_t * (polar::POLAR_CAP_ELEV - polar::POLAR_CAP_BASE_ELEV)
                    + cap_relief * cap_t;
                base_elev.max(cap_surface)
            } else {
                base_elev
            };

            row[x] = elev.clamp(0.0, 1.0);
        }
    });

        progress.tick(end as u64);
        progress.preview_map("Continents rising", (w, h), |x, y| {
            live::relief_at(&heightmap, x, y)
        });
    }

    TectonicsResult { heightmap }
}