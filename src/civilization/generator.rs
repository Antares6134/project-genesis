//! Where civilizations rise, how far they spread, and what they turn into.
//!
//! The dawn does not leave a wilderness around a handful of clearings: it
//! **partitions the whole inhabited world**. Every province belongs to somebody —
//! a state, a chiefdom, a farming village, a herd people or a band of foragers —
//! because a world at any date is a mosaic of all five, and the map should say so.
//!
//! The stage runs in ten passes, each of which is a single loop over a different
//! domain — which is what keeps it order-stable and reproducible for a seed:
//!
//! 1. **Census** — every province is measured once: its real area in km² (latitude
//!    aware), mean habitability, peak river discharge, mean climate, relief, how
//!    much of it touches sea or lake, and its carrying capacity in people.
//! 2. **Cradle selection** — every province is scored as a potential cradle, and a
//!    quota drawn on the richest superregions picks one to three of them each,
//!    with a few places reserved for realms that rise alone. Contemporaries
//!    therefore share regions instead of being spread evenly over the planet.
//! 3. **Land use** — every province is read for what its own country can feed:
//!    a field, a herd, or nothing but what can be hunted and gathered. This is the
//!    *environmental* half of the model, and it knows nothing about states.
//! 4. **Influence** — one queue seeded at every cradle at once grows all the
//!    states together and leaves behind a cost field: how far every province lies
//!    from the nearest state, and which state that is. This is the *diffusion*
//!    half, and it is what makes farming spread outward from the people who
//!    invented it rather than appearing wherever rain allows.
//! 5. **Tiers** — the two halves meet: land that can be farmed *and* lies within
//!    reach of a state is farmed, ranked or ruled; land that can be farmed but
//!    lies beyond that reach is still wild; dry grass carries herds; everything
//!    else is hunted. An isolated fertile country may rise on its own account.
//! 6. **Assembly** — contiguous provinces of one tier (and, for farming peoples,
//!    of one state's hinterland) become named societies, split where a belt is too
//!    large to be one people.
//! 7. **Development** — from year zero to the atlas date, every *state* grows
//!    along a single internal axis of development: its own surplus, its neighbours'
//!    progress and whatever its ancestral stock already knew. The leading region's
//!    level at the atlas date *is* the world's [`Age`]. A people that is not a
//!    state sits where its tier puts it and does not climb.
//! 8. **Dossier** — the country decides a society's economy, government and faith.
//! 9. **Contact** — territories that touch, or that lie within a sea-crossing of
//!    each other, are linked; the rest rose alone. Capped, because a partitioned
//!    world has hundreds of societies and a neighbour list is not a census.
//! 10. **Inheritance** — societies close enough to share an ancestral stock get one
//!    tongue family, and a younger realm in contact with an older one may have
//!    taken up its crops, rites or government *before* founding a city of its own.

use crate::civilization::config::CivilizationConfig;
use crate::civilization::epoch::{
    DevelopmentPace, Epoch, EpochCandidate, Timeline, place_name, select_epoch_site,
};
use crate::civilization::{
    Age, Civilization, CivilizationWorld, Contact, ContactKind, CradleKind, Culture, Environment,
    Faith, FaithKind, Government, Influence, Outlook, RiseProfile, ScaleClass, SocietyTier,
    Subsistence,
};
use crate::climate::PlanetaryMode;
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::sphere;
use crate::core::{relief, Grid};
use crate::geopolitics::{GeopoliticalHierarchy, ProvinceNode};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::{BTreeMap, BinaryHeap};

/// Salt mixed into the world seed for the history stage, so the dawn draws from
/// an independent random stream instead of replaying the province generator's.
const DAWN_SALT: u64 = 0x0DA7_0F1F_0000_0007;

// ─── THE DEVELOPMENT AXIS ────────────────────────────────────────────────────
//
// A realm's development is one number in `0..=1`, integrated over the years
// between the first city and the atlas date. It is never displayed: the reader
// sees its consequences — a government form and a population figure.

/// How many years a single development step covers. Fixed, so the integration is
/// reproducible to the last decimal.
const DEVELOPMENT_STEP_YEARS: i32 = 25;

/// A realm's growth on the development axis, per year, at a world pace of ×1,
/// before the cradle's own soil, river, shore and size scale it.
///
/// Calibrated against the atlas date (see [`generate_civilizations`]): the map now
/// spans the world's own dawn — a couple of millennia from the first city, not the
/// seven thousand a chained founding relay used to give — so an ordinary world's
/// leading region nears the copper horizon over those few thousand years, while a
/// small, slow or young world is still in the Irrigation age. That spread is what
/// makes the world's age vary with the seed. The logistic needs `g · t ≈ 8` to carry
/// a leader from its first village to the horizon, and the leader's own soil, river,
/// shore and size multipliers are worth roughly ×1.7 on top of this base.
const DEVELOPMENT_BASE_RATE: f64 = 0.0030;

/// How readily development crosses a shared frontier, per year, for a realm of
/// ordinary openness. A connected region converges; a loner lags.
const DEVELOPMENT_DIFFUSION: f64 = 0.0006;

/// The level a realm starts from when no older stock stands behind it: a first
/// village, not a state.
const DEVELOPMENT_SEED: f64 = 0.02;

/// The share of an older kin's development a daughter realm begins with, because
/// it learned the country's crops and rites before it founded a city of its own.
const DEVELOPMENT_INHERITANCE: f64 = 0.50;

/// Everything the history stage ever needs to know about one province.
///
/// The census is the only pass that touches the 8K grids: after it, every
/// decision is taken on provinces, and nothing has to be re-measured.
#[derive(Clone, Copy, Default)]
struct ProvinceStats {
    present: bool,
    px: u32,
    /// Real ground area, in km², integrated over latitude.
    area_km2: f32,
    hab_sum: f64,
    temp_sum: f64,
    precip_sum: f64,
    /// Highest river discharge found inside the province, in m³/s.
    flow_max: f32,
    elev_sum_m: f64,
    elev_max_m: f32,
    /// Pixel sides that touch the open sea / a lake.
    sea_px: u32,
    lake_px: u32,
    /// Carrying capacity of the province at full development, in people.
    pop_seed: f64,
    /// Which landmass core the province belongs to (its political continent).
    continent_id: u32,
}

impl ProvinceStats {
    fn hab_mean(&self) -> f32 {
        if self.px == 0 { 0.0 } else { (self.hab_sum / self.px as f64) as f32 }
    }
    fn temp_mean(&self) -> f32 {
        if self.px == 0 { 0.0 } else { (self.temp_sum / self.px as f64) as f32 }
    }
    fn precip_mean(&self) -> f32 {
        if self.px == 0 { 0.0 } else { (self.precip_sum / self.px as f64) as f32 }
    }
    fn elev_mean_m(&self) -> f32 {
        if self.px == 0 { 0.0 } else { (self.elev_sum_m / self.px as f64) as f32 }
    }
    fn is_coastal(&self) -> bool {
        self.sea_px > 0
    }
}

/// A province that could host a civilization, with the score it earned.
#[derive(Clone, Copy, Debug)]
struct Cradle {
    prov: u32,
    score: f32,
    continent: u32,
    /// The region and superregion it belongs to — the two scales the cradle
    /// quota is drawn on, so contemporaries can stand shoulder to shoulder.
    region: u32,
    superregion: u32,
    cx: f32,
    cy: f32,
}

/// Grow the dawn civilizations of one world.
///
/// Everything the stage needs comes from the finished planet: the heightmap and
/// its datum, the climate grids, the river network, the province partition and the
/// political hierarchy over it. Nothing is read from disk and nothing is added to
/// the world cache — the whole stage is a deterministic function of those inputs
/// and the seed, which is what lets a cached world and a freshly generated one
/// agree down to the last village.
pub fn generate_civilizations(
    heightmap: &Grid<f32>,
    temp_grid: &Grid<f32>,
    precip_grid: &Grid<f32>,
    flow_grid: &Grid<f32>,
    is_lake: &Grid<bool>,
    habitability: &Grid<f32>,
    province_ids: &Grid<u32>,
    hierarchy: &GeopoliticalHierarchy,
    sea_level: f32,
    min_river_flow: f32,
    planetary_mode: PlanetaryMode,
    config: &CivilizationConfig,
    seed: u64,
) -> CivilizationWorld {
    let w = heightmap.width;
    let h = heightmap.height;
    let n = hierarchy.provinces.len();
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ DAWN_SALT);

    // ─── 1. PROVINCE CENSUS ──────────────────────────────────────────────────
    // One pass over the canvas, accumulating everything any later decision needs.
    // Area is integrated over latitude through the planetary geometry, so a
    // province on the 60th parallel is not credited with the ground of one on the
    // equator; the pixel count is kept only as a shape measure.
    let mut stats = vec![ProvinceStats::default(); n];
    {
        const SHORE: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        for y in 0..h {
            let pixel_area = sphere::pixel_area_km2(y, h, w);
            let yi = y as isize;
            for x in 0..w {
                let xi = x as isize;
                let id = *province_ids.get(xi, yi);
                if id == 0 {
                    continue; // water, lake or wilderness — nobody lives here
                }
                let idx = id as usize;
                if idx >= n {
                    continue;
                }

                let elev = *heightmap.get(xi, yi);
                let hab = *habitability.get(xi, yi);
                let metres = relief::to_metres(elev);

                let (mut sea_px, mut lake_px) = (0u32, 0u32);
                for &(dx, dy) in &SHORE {
                    let nx = heightmap.wrap_x(xi + dx) as isize;
                    let ny = yi + dy;
                    if ny < 0 || ny >= h as isize {
                        continue;
                    }
                    if *is_lake.get(nx, ny) {
                        lake_px += 1;
                    } else if *heightmap.get(nx, ny) <= sea_level {
                        sea_px += 1;
                    }
                }

                // Carrying capacity: people per km² at full habitability, dimmed
                // by thin air and hard ground high up. The realm's own
                // development multiplier is applied once the realm exists.
                let alt_penalty = 1.0 - (metres / 4_000.0).clamp(0.0, 0.55);
                let density = config.max_density_per_km2
                    * hab.clamp(0.0, 1.4).powf(0.85)
                    * alt_penalty;

                let node = &hierarchy.provinces[idx];
                let s = &mut stats[idx];
                s.present = true;
                s.px += 1;
                s.area_km2 += pixel_area;
                s.hab_sum += hab as f64;
                s.temp_sum += *temp_grid.get(xi, yi) as f64;
                s.precip_sum += *precip_grid.get(xi, yi) as f64;
                s.flow_max = s.flow_max.max(*flow_grid.get(xi, yi));
                s.elev_sum_m += metres as f64;
                s.elev_max_m = s.elev_max_m.max(metres);
                s.sea_px += sea_px;
                s.lake_px += lake_px;
                s.pop_seed += density.max(0.0) as f64 * pixel_area as f64;
                s.continent_id = node.continent_id;
            }
        }
    }

    let capacity: f64 = stats.iter().map(|s| s.pop_seed).sum();
    let settled_land_km2: f32 = stats.iter().map(|s| s.area_km2).sum();
    println!(
        "✓ Provincial census: {} inhabited provinces | {:.0} km² of land | carrying capacity {} people",
        stats.iter().filter(|s| s.present).count(),
        settled_land_km2,
        thousands_u64(capacity as u64),
    );

    // ─── 2. CRADLE SELECTION ─────────────────────────────────────────────────
    // A cradle is a province so much better than its neighbours that people stop
    // moving: a river that rebuilds the soil, a shore with fish and salt, a mild
    // climate, gentle ground. Score every candidate, then draw the roster from the
    // richest superregions — several cradles in a region that can carry them, a
    // few loners on far continents — so the map carries both rivals and isolated
    // rises rather than one evenly-spaced realm per landmass.
    let mut cradles: Vec<Cradle> = Vec::new();
    for i in 1..n {
        let s = &stats[i];
        if !s.present || s.px < 40 {
            continue; // too small to hold a city, let alone a civilization
        }
        let hab = s.hab_mean();
        let temp = s.temp_mean();
        if hab < 0.12 || temp < 2.0 {
            continue; // tundra and frozen desert never fed a first city
        }

        let node = &hierarchy.provinces[i];
        let river_mult = if s.flow_max >= config.great_river_flow {
            2.4
        } else if s.flow_max >= min_river_flow {
            1.7
        } else {
            1.0
        };
        let coast_mult = if s.is_coastal() { 1.18 } else { 1.0 };
        let lake_mult = if s.lake_px > 0 { 1.10 } else { 1.0 };
        let size_mult = (s.px as f32 / 300.0).sqrt().clamp(0.6, 1.5);
        let jitter = rng.random_range(0.85..1.15f32);

        cradles.push(Cradle {
            prov: i as u32,
            score: hab.powf(1.6) * river_mult * coast_mult * lake_mult * size_mult * jitter,
            continent: s.continent_id,
            region: node.region_id,
            superregion: node.superregion_id,
            cx: node.center_x,
            cy: node.center_y,
        });
    }

    cradles.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.prov.cmp(&b.prov))
    });

    if cradles.is_empty() {
        println!("⚠ No province on this world is fertile enough to cradle a civilization.");
        return CivilizationWorld::empty(seed);
    }

    // Which superregions and regions can host a cradle, how good their best ground
    // is, and how much land they hold. The quota is drawn per superregion rather
    // than per continent: on a real map the contemporaries of one region stand
    // shoulder to shoulder — Greeks, Thracians, Phrygians and Hittites — while
    // other regions rise far away and alone.
    let mut superregion_land: BTreeMap<u32, f64> = BTreeMap::new();
    for (i, s) in stats.iter().enumerate().filter(|(_, s)| s.present) {
        let superregion = hierarchy.provinces[i].superregion_id;
        *superregion_land.entry(superregion).or_insert(0.0) += s.px as f64;
    }
    let mut superregion_best: BTreeMap<u32, f32> = BTreeMap::new();
    for c in &cradles {
        let entry = superregion_best.entry(c.superregion).or_insert(f32::MIN);
        if c.score > *entry {
            *entry = c.score;
        }
    }
    // Ranked by their best cradle, then by the ground they hold, tie-broken by id,
    // so the quota never depends on map iteration order.
    let mut ranked_superregions: Vec<u32> = superregion_best.keys().copied().collect();
    ranked_superregions.sort_by(|a, b| {
        superregion_best[b]
            .partial_cmp(&superregion_best[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                superregion_land[b]
                    .partial_cmp(&superregion_land[a])
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.cmp(b))
    });

    let wanted = rng.random_range(config.min_civilizations..=config.max_civilizations);
    let distance = |a: &Cradle, b: &Cradle| sphere::wrap_distance_px(a.cx, a.cy, b.cx, b.cy, w);

    // How close two cradles may stand. Inside one region a few provinces' worth of
    // ground is enough — that is a shared valley with two thrones in it — while
    // across regions only the universal floor applies. The old hard minimum is
    // gone: it was what kept every realm evenly far from every other, and no real
    // map looks like that.
    let same_region = config.cradle_separation_frac * w as f32;
    let floor = config.cradle_floor_frac * w as f32;

    let mut picked: Vec<Cradle> = Vec::new();
    let mut per_region: BTreeMap<u32, usize> = BTreeMap::new();
    let mut per_superregion: BTreeMap<u32, usize> = BTreeMap::new();

    // A fifth of the roster is held back for realms that rose alone; the rest goes
    // to the richest superregions, one to three cradles each.
    let lonely_slots = ((wanted as f32 * 0.2).round() as usize).clamp(1, 4);
    let core_target = wanted.saturating_sub(lonely_slots).max(1);
    let quota_for = |rank: usize| if rank < 3 { 3 } else if rank < 8 { 2 } else { 1 };

    for (rank, superregion) in ranked_superregions.iter().enumerate() {
        if picked.len() >= core_target {
            break;
        }
        for _ in 0..quota_for(rank) {
            if picked.len() >= core_target {
                break;
            }
            let fits = |candidate: &Cradle| {
                candidate.superregion == *superregion
                    && !picked.iter().any(|p| p.prov == candidate.prov)
                    && per_region.get(&candidate.region).copied().unwrap_or(0)
                        < config.cradle_max_per_region
                    && per_superregion.get(superregion).copied().unwrap_or(0)
                        < config.cradle_max_per_superregion
                    && picked.iter().all(|p| {
                        if distance(p, candidate) < floor {
                            return false;
                        }
                        p.region != candidate.region || distance(p, candidate) >= same_region
                    })
            };
            // A second or third cradle in this superregion is taken from within
            // reach of one already picked, so the two are genuine rivals who can
            // meet — a shared valley, a strait between them — rather than two
            // thrones at opposite ends of a continent.
            let next = cradles
                .iter()
                .find(|candidate| {
                    fits(candidate)
                        && picked.iter().any(|p| {
                            p.superregion == candidate.superregion
                                && distance(p, candidate) <= config.contact_distance_px
                        })
                })
                .or_else(|| cradles.iter().find(|candidate| fits(candidate)));
            match next {
                Some(candidate) => {
                    picked.push(*candidate);
                    *per_region.entry(candidate.region).or_insert(0) += 1;
                    *per_superregion.entry(*superregion).or_insert(0) += 1;
                }
                None => break, // this superregion's good ground is used up
            }
        }
    }

    // The lonely rises: the best cradle on a continent nobody has reached yet, so
    // the world still shows a civilization that invented everything by itself.
    let mut lonely = 0usize;
    for candidate in &cradles {
        if lonely >= lonely_slots || picked.len() >= wanted {
            break;
        }
        if picked.iter().any(|p| p.continent == candidate.continent) {
            continue; // that landmass already has its own realm
        }
        if picked
            .iter()
            .any(|p| distance(p, candidate) < config.isolated_distance_px)
        {
            continue; // close enough to the others to be a neighbour, not a loner
        }
        picked.push(*candidate);
        *per_region.entry(candidate.region).or_insert(0) += 1;
        *per_superregion.entry(candidate.superregion).or_insert(0) += 1;
        lonely += 1;
    }

    // Fill the roster from the best remaining ground, still respecting the region
    // and superregion caps so the world does not collapse into one crowded corner.
    for candidate in &cradles {
        if picked.len() >= wanted {
            break;
        }
        if picked.iter().any(|p| p.prov == candidate.prov) {
            continue;
        }
        if per_region.get(&candidate.region).copied().unwrap_or(0)
            >= config.cradle_max_per_region
            || per_superregion.get(&candidate.superregion).copied().unwrap_or(0)
                >= config.cradle_max_per_superregion
        {
            continue;
        }
        if picked.iter().any(|p| distance(p, candidate) < floor) {
            continue;
        }
        picked.push(*candidate);
        *per_region.entry(candidate.region).or_insert(0) += 1;
        *per_superregion.entry(candidate.superregion).or_insert(0) += 1;
    }

    picked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.prov.cmp(&b.prov))
    });

    if picked.len() < config.min_civilizations {
        println!(
            "Only {} cradles could be placed on this world (wanted {}).",
            picked.len(),
            wanted
        );
    }

    // ─── 3. FOUNDING DATES, AMBITIONS AND EXPANSION ──────────────────────────
    // Year zero of the internal frame is the founding of the first city, by the
    // first realm to rise. The rest are *contemporaries*, not a relay: every later
    // cradle rises inside one bounded window after it, so the whole set clusters
    // around the first city instead of running away from it.
    //
    // How fast this world develops is read here, because the window below and the
    // eras beyond it are scaled by it. What that pace implies for the depth of
    // prehistory — how long the peoples of this world lived before the first hearth
    // — is drawn much later, once the whole history is known and every earlier draw
    // has been made.
    let pace = {
        let present = stats.iter().filter(|s| s.present).collect::<Vec<_>>();
        let (mean_habitability, frost_share) = if present.is_empty() {
            (0.5, 0.0)
        } else {
            let count = present.len() as f32;
            let hab = present.iter().map(|s| s.hab_mean()).sum::<f32>() / count;
            let frost = present.iter().filter(|s| s.temp_mean() < 0.0).count() as f32 / count;
            (hab, frost)
        };
        DevelopmentPace::from_measurements(
            mean_habitability,
            frost_share,
            matches!(planetary_mode, PlanetaryMode::TidallyLocked),
        )
    };
    println!(
        "✓ Development pace: ×{:.2} — how fast this world reaches its first cities",
        pace.multiplier(),
    );

    // The era of first cities: the nominal years within which every cradle after
    // the first rises, divided by the world's pace so a slow world's dawn lasts
    // longer. The first city stands at internal year zero; the rest are drawn
    // inside the window, sorted, and handed to the ranks in order — so the richest
    // cradle still rises first and the roster stays in founding order, while the
    // set clusters instead of stretching and can never exceed the window however
    // many cradles the world carries.
    let founding_span =
        (config.founding_window_years as f32 / pace.multiplier()).round().max(1.0) as i32;
    let mut founding_years: Vec<i32> = Vec::with_capacity(picked.len());
    founding_years.push(0);
    let nearest_elder: Vec<f32> = (1..picked.len())
        .map(|rank| {
            let cradle = &picked[rank];
            picked[..rank]
                .iter()
                .map(|p| distance(p, cradle))
                .fold(f32::MAX, f32::min)
        })
        .collect();
    founding_years.extend(founding_offsets(
        &nearest_elder,
        founding_span,
        config.isolated_distance_px,
        &mut rng,
    ));

    // Ambition: how many provinces each realm sets out to hold. Riches decide it —
    // the richest cradle is the one that can feed the most people — with the
    // guarantee that a world of three or more realms shows both a giant and a
    // pocket state, because history always did.
    let best_score = picked[0].score.max(f32::EPSILON);
    let mut ambitions: Vec<ScaleClass> = Vec::with_capacity(picked.len());
    for cradle in &picked {
        let share = (cradle.score / best_score).clamp(0.0, 1.0);
        let roll: f32 = rng.random();
        ambitions.push(if roll < 0.22 + 0.38 * share {
            ScaleClass::Large
        } else if roll < 0.68 {
            ScaleClass::Medium
        } else {
            ScaleClass::Small
        });
    }
    if !ambitions.contains(&ScaleClass::Large) {
        let idx = ambitions
            .iter()
            .position(|a| *a == ScaleClass::Medium)
            .unwrap_or(0);
        ambitions[idx] = ScaleClass::Large;
    }
    if !ambitions.contains(&ScaleClass::Small) {
        let idx = ambitions
            .iter()
            .rposition(|a| *a != ScaleClass::Large)
            .unwrap_or(ambitions.len() - 1);
        ambitions[idx] = ScaleClass::Small;
    }

    // ─── 3. LAND USE ─────────────────────────────────────────────────────────
    // What every province's own country can feed, before anybody has decided
    // anything: a field, a herd, or nothing but what can be hunted and gathered.
    // This is the environmental half of the model, and it is the reason a world
    // grows a pastoralist belt between its deserts and its plough country.
    let land: Vec<ProvinceLand> = (0..n)
        .map(|i| {
            if stats[i].present {
                read_land(
                    &stats[i],
                    &hierarchy.provinces[i],
                    min_river_flow,
                    config.great_river_flow,
                    config,
                )
            } else {
                ProvinceLand {
                    environment: Environment::IceMargin,
                    land_use: LandUse::Foraging,
                    farm: 0.0,
                    pasture: 0.0,
                }
            }
        })
        .collect();
    {
        let mut farmable = 0usize;
        let mut grazable = 0usize;
        for (i, l) in land.iter().enumerate() {
            if !stats[i].present {
                continue;
            }
            match l.land_use {
                LandUse::Farming { .. } => farmable += 1,
                LandUse::Pastoralism => grazable += 1,
                LandUse::Foraging => {}
            }
        }
        println!(
            "✓ Land use: {} of {} provinces could carry a field, {} could carry herds — the rest are hunted",
            farmable,
            stats.iter().filter(|s| s.present).count(),
            grazable,
        );
    }

    // ─── 4. INFLUENCE ────────────────────────────────────────────────────────
    // One queue, seeded at every cradle at once. The states grow together instead
    // of in rank order, each province falls to whichever state reaches it most
    // cheaply, and the field they leave behind says how far every province on the
    // planet lies from the nearest state — which is the second half of the rule.
    let seats: Vec<u32> = picked.iter().map(|c| c.prov).collect();
    let targets: Vec<usize> = ambitions
        .iter()
        .map(|ambition| {
            let (lo, hi) = match ambition {
                ScaleClass::Small => config.small_provinces,
                ScaleClass::Medium => config.medium_provinces,
                ScaleClass::Large => config.large_provinces,
            };
            rng.random_range(lo..=hi)
        })
        .collect();

    let mut owner: Vec<u16> = vec![0; n];
    let (influence, _) = grow_states(&seats, &targets, &mut owner, hierarchy, &stats, w, config);

    // Tuning diagnostic: what share of the inhabited world falls inside a given
    // influence cost, so the two thresholds can be set from the numbers rather than
    // guessed. The atlas targets are roughly 19% simple farming and 5% chiefdoms.
    {
        let inhabited = stats.iter().filter(|s| s.present).count().max(1);
        let mut farming: Vec<f32> = (0..n)
            .filter(|&i| stats[i].present && matches!(land[i].land_use, LandUse::Farming { .. }))
            .map(|i| influence.cost[i])
            .filter(|cost| cost.is_finite())
            .collect();
        farming.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut line = String::from("  farmable land inside influence cost:");
        for threshold in [10.0f32, 20.0, 40.0, 80.0, 160.0, 320.0, 640.0] {
            let within = farming.iter().filter(|&&cost| cost <= threshold).count();
            line.push_str(&format!(
                " {threshold:.0}→{:.1}%",
                100.0 * within as f32 / inhabited as f32
            ));
        }
        println!("{line}");
    }

    // ─── 5. TIERS ────────────────────────────────────────────────────────────
    // The independent neolithics: a fertile country nobody reached, where farming
    // was worked out locally rather than brought in — the Amazon, New Guinea, West
    // Africa. Seeded on the best ground beyond any state's reach, and spread only
    // as far as such a people's own neolithic could carry.
    let independent_seeds: Vec<u32> = (0..n)
        .filter(|&i| {
            stats[i].present
                && owner[i] == 0
                && matches!(land[i].land_use, LandUse::Farming { .. })
                && land[i].farm >= config.rise_floor
                && influence.cost[i] >= config.far_cost
        })
        .map(|i| i as u32)
        .collect();
    let independent = diffuse_field(
        &independent_seeds,
        hierarchy,
        &stats,
        w,
        config,
        config.independent_reach,
    );
    let tiers = assign_tiers(
        &owner,
        &influence,
        &independent,
        &land,
        &stats,
        config,
        &mut rng,
    );

    // ─── 6. ASSEMBLY ─────────────────────────────────────────────────────────
    // Contiguous provinces of one tier — and, for farming peoples, of one state's
    // hinterland — become named societies. Nothing is left over: after this every
    // inhabited province on the world belongs to somebody.
    let (territories, society_tiers) = assemble_societies(
        &tiers,
        &owner,
        &influence.nearest,
        &stats,
        hierarchy,
        picked.len(),
        config,
    );
    owner = vec![0u16; n];
    for (rank, provinces) in territories.iter().enumerate() {
        let id = rank as u16 + 1;
        for &prov in provinces {
            owner[prov as usize] = id;
        }
    }

    let mut continents_used: Vec<u32> = picked.iter().map(|c| c.continent).collect();
    continents_used.sort_unstable();
    continents_used.dedup();
    // Which societies share a border, now that the partition has filled `owner`. It
    // is needed twice below — by the development pass, which lets a realm learn
    // from the neighbours it touches, and by the palette — so it is built once.
    let adjacency = frontier_adjacency(&owner, hierarchy);

    println!(
        "✓ Dawn cradles: {} states on {} continents | {} provinces claimed",
        picked.len(),
        continents_used.len(),
        territories
            .iter()
            .take(picked.len())
            .map(|t| t.len())
            .sum::<usize>(),
    );
    {
        let mut counts = [0usize; 5];
        let mut provinces = [0usize; 5];
        for (rank, tier) in society_tiers.iter().enumerate() {
            counts[*tier as usize] += 1;
            provinces[*tier as usize] += territories[rank].len();
        }
        let inhabited = stats.iter().filter(|s| s.present).count().max(1);
        for (index, tier) in SocietyTier::LADDER.iter().enumerate() {
            println!(
                "  {:<26} {:>4} societies · {:>6} provinces ({:>5.1}% of the inhabited world)",
                tier.label(),
                counts[index],
                provinces[index],
                100.0 * provinces[index] as f32 / inhabited as f32,
            );
        }
    }

    // ─── 4. SKELETONS AND ANCESTRAL STOCKS ───────────────────────────────────
    // Everything a realm is before its dossier is written: where it sits, how much
    // it holds, when it began and what kind of country it was born in.
    //
    // The map date is set a few centuries *after* the youngest realm began, so
    // that every civilization on the map has a history rather than only a founding
    // — otherwise the last cradle to rise would always stand at Year 0 of its own
    // calendar, with no cities and no scribes to its name. It is bounded because the
    // founding window is: the map is a snapshot of the world's dawn, not a date ten
    // thousand years on.
    //
    // This date is also the far end of the development pass, so the tail is what
    // decides how mature the world is: it is calibrated together with
    // `DEVELOPMENT_BASE_RATE` so an ordinary world lands at the copper horizon
    // while a young, slow or clustered one is still in the Irrigation age.
    let last_founding = *founding_years.last().unwrap_or(&0);
    let map_year = last_founding + rng.random_range(150..=400);
    let family_radius = config.family_radius_frac * w as f32;

    let mut skeletons: Vec<Skeleton> = Vec::with_capacity(territories.len());
    for (rank, provinces) in territories.iter().enumerate() {
        let tier = SocietyTier::from_index(society_tiers[rank]);
        let is_state = rank < picked.len();
        // The seat: a state's own cradle, or — for a people, who have no cradle —
        // the province of their country that could feed the most of them.
        let seat = if is_state {
            seats[rank]
        } else {
            let mut best = *provinces.first().unwrap_or(&0);
            for &prov in provinces {
                let candidate = stats[prov as usize].pop_seed;
                let incumbent = stats[best as usize].pop_seed;
                if candidate > incumbent || (candidate == incumbent && prov < best) {
                    best = prov;
                }
            }
            best
        };
        let seat_node = &hierarchy.provinces[seat as usize];
        let seat_stats = stats[seat as usize];
        let environment = land[seat as usize].environment;
        let cradle_kind = if is_state {
            classify_cradle(
                &seat_stats,
                hierarchy,
                seat,
                min_river_flow,
                config.great_river_flow,
            )
        } else {
            environment_cradle_kind(environment)
        };
        skeletons.push(Skeleton {
            cradle: Cradle {
                prov: seat,
                // A state's score belongs to the cradle that was *chosen*; a
                // people's is simply how good their own country is.
                score: if is_state {
                    picked[rank].score
                } else {
                    land[seat as usize].farm.max(land[seat as usize].pasture)
                },
                continent: seat_stats.continent_id,
                region: seat_node.region_id,
                superregion: seat_node.superregion_id,
                cx: seat_node.center_x,
                cy: seat_node.center_y,
            },
            cradle_kind,
            environment,
            tier,
            area_km2: provinces.iter().map(|&p| stats[p as usize].area_km2).sum(),
            scale: ScaleClass::from_provinces(provinces.len()),
            // A state's level is filled in by the deep-time development pass below;
            // a people's is set from its tier, because a band does not climb.
            complexity: 0.0,
            age_years: if is_state {
                (map_year - founding_years[rank]).max(0) as u32
            } else {
                map_year.max(0) as u32
            },
            founding_year_internal: if is_state { founding_years[rank] } else { 0 },
            provinces: provinces.clone(),
            seat_stats,
        });
    }

    // Two cradles close together on the same landmass almost always came from the
    // same people, parted *before* either built a city. That shared stock is why
    // some civilizations on a world look like siblings while others could not be
    // more foreign: it fixes their tongue family, and it lets one inherit the
    // other's temple form, rites and craft.
    let mut family_of: Vec<usize> = (0..skeletons.len()).collect();
    for a in 0..skeletons.len() {
        for b in (a + 1)..skeletons.len() {
            if skeletons[a].cradle.continent != skeletons[b].cradle.continent {
                continue;
            }
            if distance(&skeletons[a].cradle, &skeletons[b].cradle) <= family_radius {
                let (ra, rb) = (
                    family_find(&mut family_of, a),
                    family_find(&mut family_of, b),
                );
                if ra != rb {
                    // Always merge towards the lower index, so the family's
                    // representative never depends on iteration order.
                    family_of[ra.max(rb)] = ra.min(rb);
                }
            }
        }
    }

    // The family's name and its oldest member (the realm the rest descend from).
    let mut family_members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..skeletons.len() {
        let root = family_find(&mut family_of, i);
        family_members.entry(root).or_default().push(i);
    }
    let mut family_name: Vec<String> = vec![String::new(); skeletons.len()];
    let mut family_ancestor: Vec<usize> = (0..skeletons.len()).collect();
    let mut kin: Vec<Vec<u32>> = vec![Vec::new(); skeletons.len()];
    for members in family_members.values() {
        let ancestor = *members
            .iter()
            .min_by_key(|&&i| (skeletons[i].founding_year_internal, i))
            .unwrap();
        let name = format!("{} Tongue", skeletons[ancestor].cradle_kind.tongue());
        for &m in members {
            family_name[m] = name.clone();
            family_ancestor[m] = ancestor;
            // A family can hold a hundred peoples on a crowded continent, so kin are
            // the nearest few by the seats they sit in, not the whole clan: a
            // genealogy is not a census.
            let mut siblings: Vec<(f32, u32)> = members
                .iter()
                .filter(|&&other| other != m)
                .map(|&other| {
                    (
                        distance(&skeletons[m].cradle, &skeletons[other].cradle),
                        other as u32 + 1,
                    )
                })
                .collect();
            siblings.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.1.cmp(&b.1))
            });
            siblings.truncate(config.kin_limit);
            kin[m] = siblings.into_iter().map(|(_, id)| id).collect();
        }
    }

    // What the atlas calls each society, decided here because the contact and
    // inheritance notes below have to name the other side of every frontier.
    //
    // A people's name is their country and their kind, and two peoples of the same
    // country and the same kind collide: the second one onward takes a numeral, which
    // is how a real gazetteer disambiguates them too. A state keeps its placeholder
    // number, because a state is exactly the kind of thing that would have a proper
    // name once this world has languages.
    let names: Vec<String> = {
        let mut names: Vec<String> = Vec::with_capacity(skeletons.len());
        let mut used: BTreeMap<String, usize> = BTreeMap::new();
        for (rank, sk) in skeletons.iter().enumerate() {
            if sk.tier.is_state() {
                names.push(civilization_name(rank as u32 + 1));
                continue;
            }
            let base = format!(
                "{} {}",
                capitalize_first(sk.environment.noun()),
                sk.tier.noun()
            );
            let count = used.entry(base.clone()).or_insert(0);
            *count += 1;
            names.push(if *count == 1 {
                base
            } else {
                format!("{base} {count}")
            });
        }
        names
    };

    // ─── 5. DEEP-TIME DEVELOPMENT ────────────────────────────────────────────
    // How developed every realm is at the atlas date is not a function of how old
    // it is: it grows from its own surplus, learns from the neighbours it touches,
    // and starts part-way up if an older stock already held the country. The
    // economy and temper of each realm are drawn first, because how open a people
    // is decides how readily they take a neighbour's lead — and they are drawn in
    // rank order, exactly as the dossier below would have drawn them.
    let mut schemes: Vec<Scheme> = Vec::with_capacity(skeletons.len());
    for rank in 0..skeletons.len() {
        let tier = skeletons[rank].tier;
        let environment = skeletons[rank].environment;
        let subsistence = if tier == SocietyTier::States {
            choose_subsistence(&mut rng, &skeletons[rank])
        } else {
            subsistence_for(tier, environment, &mut rng)
        };
        let outlook = choose_outlook(&mut rng, subsistence);
        // A people that is not a state does not climb the development axis: where it
        // sits on it is decided by what kind of society it is, not by how long it has
        // been climbing. Only a state is measured by the deep-time pass.
        if tier != SocietyTier::States {
            skeletons[rank].complexity = tier_complexity(tier, &mut rng);
        }
        schemes.push(Scheme {
            subsistence,
            outlook,
        });
    }

    let frontier = simulate_development(
        &mut skeletons,
        &schemes,
        &family_ancestor,
        &adjacency,
        &stats,
        min_river_flow,
        config.great_river_flow,
        map_year,
        pace.multiplier(),
        picked.len(),
    );
    let mean_complexity = if skeletons.is_empty() {
        0.0
    } else {
        skeletons.iter().map(|sk| sk.complexity).sum::<f32>() / skeletons.len() as f32
    };
    println!(
        "✓ Deep-time development: frontier {:.2} — the world stands in {} | mean realm development {:.2}",
        frontier,
        Age::from_development(frontier as f32).label(),
        mean_complexity,
    );

    // ─── 6. DOSSIERS ─────────────────────────────────────────────────────────
    // Culture and faith are read off the cradle's own country where geography
    // decides them (a canal state is not born in a rainforest), with a seeded draw
    // for the rest. Kin inherit from the head of their family, which is what makes
    // siblings on a world resemble each other while strangers do not.
    let mut cultures: Vec<Culture> = Vec::with_capacity(skeletons.len());
    let mut faiths: Vec<Faith> = Vec::with_capacity(skeletons.len());
    for rank in 0..skeletons.len() {
        let ancestor = family_ancestor[rank];
        let inherited_culture = if ancestor != rank {
            cultures.get(ancestor).cloned()
        } else {
            None
        };
        let inherited_faith = if ancestor != rank {
            faiths.get(ancestor).cloned()
        } else {
            None
        };
        let culture = build_culture(
            &mut rng,
            &skeletons[rank],
            schemes[rank],
            inherited_culture.as_ref(),
            family_name[rank].as_str(),
        );
        let faith = build_faith(&mut rng, &skeletons[rank], inherited_faith.as_ref());
        cultures.push(culture);
        faiths.push(faith);
    }

    // ─── 7. POPULATION ───────────────────────────────────────────────────────
    // A province's population is its own carrying capacity from the census, times
    // what its holders can actually draw from it. For a state that is its
    // development — an empire with archives, canals and standing armies feeds more
    // people per hectare than a scatter of first villages. For a people it is their
    // tier: a band of foragers lives off the same country at a small fraction of
    // what a state could take from it, and that single number is what makes a
    // world's population look like a world's rather than a sum of empires.
    let mut province_population: Vec<u64> = vec![0; n];
    let mut populations: Vec<u64> = Vec::with_capacity(skeletons.len());
    for rank in 0..skeletons.len() {
        let sk = &skeletons[rank];
        let draw = sk.tier.density_factor()
            * if sk.tier.is_state() {
                development_multiplier(
                    cultures[rank].government,
                    cultures[rank].outlook,
                    sk.complexity,
                )
            } else {
                1.0
            };
        let mut total = 0u64;
        for &p in &sk.provinces {
            let pop = (stats[p as usize].pop_seed * draw as f64).round() as u64;
            province_population[p as usize] = pop;
            total += pop;
        }
        populations.push(total);
    }

    // ─── 8. CONTACT ──────────────────────────────────────────────────────────
    // Two realms are linked when their provinces touch across dry land (a
    // frontier), when only a strait or a short sea crossing lies between them (a
    // sea route), or when they are close enough for travellers to carry word of
    // each other (a rumour). Every other realm rose completely alone.
    let mut reach: BTreeMap<(u32, u32), Reach> = BTreeMap::new();
    for (rank, sk) in skeletons.iter().enumerate() {
        let a = rank as u32 + 1;
        for &p in &sk.provinces {
            let node = &hierarchy.provinces[p as usize];
            for &(nb, _) in &node.neighbors {
                let idx = nb as usize;
                if idx >= n {
                    continue;
                }
                let other = owner[idx] as u32;
                if other == 0 || other == a {
                    continue;
                }
                let other_node = &hierarchy.provinces[idx];
                let candidate = Reach {
                    px: sphere::wrap_distance_px(
                        node.center_x,
                        node.center_y,
                        other_node.center_x,
                        other_node.center_y,
                        w,
                    ),
                    x1: node.center_x,
                    y1: node.center_y,
                    x2: other_node.center_x,
                    y2: other_node.center_y,
                };
                let key = (a.min(other), a.max(other));
                let slot = reach.entry(key).or_insert(candidate);
                if candidate.px < slot.px {
                    *slot = candidate;
                }
            }
        }
    }

    let mut contacts: Vec<Vec<Contact>> = vec![Vec::new(); skeletons.len()];
    for a in 0..skeletons.len() {
        for b in (a + 1)..skeletons.len() {
            let id_a = a as u32 + 1;
            let id_b = b as u32 + 1;

            let found = match reach.get(&(id_a, id_b)) {
                Some(r) => Some(*r),
                None => {
                    // No provinces touch at all: the two only matter to each other
                    // if their hearts are within reach of a traveller.
                    let seat_px = distance(&skeletons[a].cradle, &skeletons[b].cradle);
                    if seat_px > config.rumour_distance_px {
                        continue;
                    }
                    Some(Reach {
                        px: seat_px,
                        x1: skeletons[a].cradle.cx,
                        y1: skeletons[a].cradle.cy,
                        x2: skeletons[b].cradle.cx,
                        y2: skeletons[b].cradle.cy,
                    })
                }
            };
            let Some(found) = found else { continue };

            let kind = if found.px <= config.frontier_distance_px {
                ContactKind::LandFrontier
            } else if found.px <= config.searoute_distance_px {
                ContactKind::SeaRoute
            } else {
                ContactKind::DistantRumour
            };
            let distance_km =
                sphere::map_distance_km(found.x1, found.y1, found.x2, found.y2, w, h);
            let relation = pick_relation(&mut rng, kind, cultures[a].outlook, cultures[b].outlook);

            contacts[a].push(Contact {
                other: id_b,
                kind,
                distance_km,
                relation,
                note: contact_note(kind, relation, distance_km, &names[b]),
            });
            contacts[b].push(Contact {
                other: id_a,
                kind,
                distance_km,
                relation,
                note: contact_note(kind, relation, distance_km, &names[a]),
            });
        }
    }

    // A partitioned world carries hundreds of societies, so a neighbour list is not
    // a census: every society keeps the nearest handful and no more. Sorted by
    // distance first, then by id, so the cut is never arbitrary.
    for list in contacts.iter_mut() {
        list.sort_by(|a, b| {
            a.distance_km
                .partial_cmp(&b.distance_km)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.other.cmp(&b.other))
        });
        list.truncate(config.contact_limit);
        list.sort_by_key(|contact| contact.other);
    }

    // ─── 9. INHERITANCE ──────────────────────────────────────────────────────
    // A younger realm in contact with an older one may have taken up crops, rites
    // or a script centuries before it founded a city of its own — the way the
    // ancestors of Egypt took up farming and pottery from Mesopotamia long before
    // Narmer. This is the half of prehistory that leaves no border on the map.
    let mut influences: Vec<Vec<Influence>> = vec![Vec::new(); skeletons.len()];
    for a in 0..skeletons.len() {
        for b in (a + 1)..skeletons.len() {
            let (older, younger) = if skeletons[a].founding_year_internal
                <= skeletons[b].founding_year_internal
            {
                (a, b)
            } else {
                (b, a)
            };
            let gap = skeletons[younger].founding_year_internal
                - skeletons[older].founding_year_internal;
            if gap < 150 {
                continue; // too close in time for an inheritance to be visible
            }
            let older_id = older as u32 + 1;
            // Only a state takes up a neighbour's crops *before founding a city* —
            // a people never founded one, and their own country already decides how
            // they live. Inheritance in the dossier is a story about cities.
            if !skeletons[older].tier.is_state() || !skeletons[younger].tier.is_state() {
                continue;
            }
            let Some(contact) = contacts[younger].iter().find(|c| c.other == older_id) else {
                continue; // the two never met before the younger one existed
            };
            let same_family =
                family_find(&mut family_of, older) == family_find(&mut family_of, younger);
            let base = match contact.kind {
                ContactKind::LandFrontier => 0.60,
                ContactKind::SeaRoute => 0.45,
                ContactKind::DistantRumour => 0.18,
            };
            let chance = (base
                * cultures[younger].outlook.openness()
                * if same_family { 1.30 } else { 1.0 })
            .min(0.95);
            if rng.random::<f32>() > chance {
                continue;
            }

            let pool = hallmarks(&cultures[older], &faiths[older]);
            let count = rng.random_range(1..=3usize).min(pool.len());
            let mut picks: Vec<&'static str> = Vec::new();
            while picks.len() < count {
                let candidate = *pool.choose(&mut rng).unwrap();
                if !picks.contains(&candidate) {
                    picks.push(candidate);
                }
            }
            let borrowed: Vec<String> = picks.iter().map(|s| lowercase_first(s)).collect();
            let note = format!(
                "{} was already {} years old when {} began to settle: {} had crossed the country — and the frontier between them did not exist yet — generations before the younger realm's first city did.",
                names[older],
                thousands_u64(gap.max(0) as u64),
                names[younger],
                join_list(&borrowed),
            );
            influences[younger].push(Influence {
                other: older_id,
                borrowed,
                note,
            });
        }
    }

    // ─── 10. THE REASONS ──────────────────────────────────────────────────────
    // The dossier's "why here" bullets are written last, once the distances to the
    // other realms are finally known.
    let mut rises: Vec<RiseProfile> = Vec::with_capacity(skeletons.len());
    for rank in 0..skeletons.len() {
        let nearest_km = (0..skeletons.len())
            .filter(|&other| other != rank)
            .map(|other| {
                sphere::map_distance_km(
                    skeletons[rank].cradle.cx,
                    skeletons[rank].cradle.cy,
                    skeletons[other].cradle.cx,
                    skeletons[other].cradle.cy,
                    w,
                    h,
                )
            })
            .fold(f32::MAX, f32::min);
        let richest_rank = cradles
            .iter()
            .position(|c| c.prov == skeletons[rank].cradle.prov)
            .map_or(0, |i| i + 1);
        rises.push(build_rise(
            &skeletons[rank],
            hierarchy,
            richest_rank,
            cradles.len(),
            nearest_km,
            sphere::row_latitude_deg(skeletons[rank].cradle.cy, h),
            min_river_flow,
            config.great_river_flow,
        ));
    }

    // ─── 10b. THE EPOCH AND THE DEPTH OF PREHISTORY ───────────────────────────
    // The world's calendar is reckoned from one place and one year, and both come
    // out of the seed. The place is a province chosen for the shape a founding
    // story takes — a crossroads between realms, a lone peak, a lake, an island —
    // and never a cradle or a throne; the year is a few centuries into the life of
    // whoever held it, so the place and the date belong to one story. What actually
    // happened there is left undefined: the atlas only says it mattered.
    //
    // The depth of prehistory — how long this world's peoples lived before the
    // first city — is drawn here too, at the very end, so that every earlier draw
    // in the stage is untouched and a world generated before this stage existed
    // still produces the same civilizations. It is a wide, seeded span divided by
    // the world's own pace, and there is nothing Earth-like about it: a gentle
    // world may reach its first city in five thousand years and a hard or
    // tidally-locked one may take a million.
    let depth_years = config.species_emergence_years.unwrap_or_else(|| {
        // Log-uniform over five thousand to two million years: the honest
        // distribution for a quantity that spans orders of magnitude.
        let (lo, hi) = (5_000f32.ln(), 2_000_000f32.ln());
        (lo + rng.random::<f32>() * (hi - lo)).exp().round() as i32
    });
    let prehistory_years = ((depth_years as f32 / pace.multiplier()).round() as i32).max(500);
    println!(
        "✓ Prehistory: {} years separate the first people from the first city",
        thousands(prehistory_years as f32),
    );

    let epoch_candidates = build_epoch_candidates(&stats, hierarchy, &owner, &contacts, &seats);
    let epoch_site = select_epoch_site(
        &epoch_candidates,
        config.epoch_site_min_area_km2,
        config.epoch_site_candidates,
        &mut rng,
    );
    let epoch = choose_epoch(
        epoch_site,
        &epoch_candidates,
        &owner,
        config,
        hierarchy,
        map_year,
        &mut rng,
    );
    let timeline = Timeline {
        prehistory_years,
        atlas_year: map_year,
        age: Age::from_development(frontier as f32),
        epoch,
    };
    // Every date the atlas shows is this one shift away from the internal frame.
    let epoch_offset = timeline.epoch.absolute_year;

    // ─── 11. ASSEMBLE ────────────────────────────────────────────────────────
    // Every date the atlas ever shows is written in the world's own era, so the
    // internal frame is converted exactly once, here: a realm's founding and the
    // map date both become years of the Common Era, negative for BCE. Nothing
    // above this point ever saw a CE year, and nothing below it sees anything else.
    let colors = assign_colors(&skeletons);
    let mut civilizations: Vec<Civilization> = Vec::with_capacity(skeletons.len());
    for rank in 0..skeletons.len() {
        let sk = &skeletons[rank];
        let population = populations[rank];
        civilizations.push(Civilization {
            name: names[rank].clone(),
            color: colors[rank],
            tier: sk.tier,
            environment: sk.environment,
            cradle_kind: sk.cradle_kind,
            scale: sk.scale,
            rise: rises[rank].clone(),
            culture: cultures[rank].clone(),
            faith: faiths[rank].clone(),
            seat_province: sk.cradle.prov,
            seat_x: sk.cradle.cx,
            seat_y: sk.cradle.cy,
            seat_latitude_deg: sphere::row_latitude_deg(sk.cradle.cy, h),
            seat_longitude_deg: (sk.cradle.cx / w as f32 - 0.5) * 360.0,
            provinces: sk.provinces.clone(),
            area_km2: sk.area_km2,
            population,
            // A state carries a founding date; a people carry none, because nobody
            // founded them.
            founding_year: if sk.tier.is_state() {
                Some(sk.founding_year_internal - epoch_offset)
            } else {
                None
            },
            age_years: sk.age_years,
            contacts: std::mem::take(&mut contacts[rank]),
            influences: std::mem::take(&mut influences[rank]),
            kin: kin[rank].clone(),
            language_family: family_name[rank].clone(),
            density_per_km2: if sk.area_km2 > 0.0 {
                population as f32 / sk.area_km2
            } else {
                0.0
            },
        });
    }

    let total_population: u64 = civilizations.iter().map(|c| c.population).sum();
    let civilized_provinces: usize = civilizations.iter().map(|c| c.provinces.len()).sum();
    let civilized_area_km2: f32 = civilizations.iter().map(|c| c.area_km2).sum();

    println!(
        "✓ {} | atlas date: {} | {} civilized provinces | {} km² | population {}",
        timeline.epoch_label(),
        timeline.atlas_label(),
        civilized_provinces,
        civilized_area_km2 as u64,
        thousands_u64(total_population),
    );

    CivilizationWorld {
        seed,
        map_year: map_year - epoch_offset,
        timeline,
        civilizations,
        province_owner: owner,
        province_tier: tiers,
        province_population,
        total_population,
        civilized_provinces,
        civilized_area_km2,
    }
}
// ─── INTERNAL RECORDS ────────────────────────────────────────────────────────

/// How a realm's country feeds it and what it values.
///
/// Drawn before the development pass because an open people takes a neighbour's
/// lead more readily than a closed one, and the same values then shape the
/// government the dossier writes.
#[derive(Clone, Copy)]
struct Scheme {
    subsistence: Subsistence,
    outlook: Outlook,
}

/// A cradle that has taken root: everything about a realm that is known before
/// its dossier is written.
struct Skeleton {
    cradle: Cradle,
    cradle_kind: CradleKind,
    /// The country the people live in, for every society and not only for a state.
    environment: Environment,
    /// Which of the five kinds of society this is.
    tier: SocietyTier,
    seat_stats: ProvinceStats,
    provinces: Vec<u32>,
    area_km2: f32,
    scale: ScaleClass,
    /// How developed the realm is at the atlas date, in `0..=1`, measured by the
    /// deep-time development pass for a state, and set from the tier for a people.
    /// Drives its government and its population density; never shown as a tier.
    complexity: f32,
    age_years: u32,
    /// Founding year in the world's internal frame, whose year zero is the first
    /// city. Zero for a people, who were already there.
    founding_year_internal: i32,
}

/// The closest approach between two realms' territories.
#[derive(Clone, Copy)]
struct Reach {
    px: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

/// One frontier step: a province reached at a certain accumulated cost, by a
/// particular society. The society is part of the ordering so that two sources
/// reaching one province at equal cost always resolve the same way.
#[derive(Copy, Clone, PartialEq)]
struct Step {
    cost: f32,
    prov: u32,
    civ: u16,
}

impl Eq for Step {}

impl Ord for Step {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| self.prov.cmp(&other.prov))
            .then_with(|| self.civ.cmp(&other.civ))
    }
}

impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// `Civilization #4` — the placeholder name every realm carries until languages
/// exist on this world.
fn civilization_name(id: u32) -> String {
    format!("Civilization #{id}")
}

/// Gather every province worth considering as the place a calendar is reckoned
/// from.
///
/// Runs after contacts and inheritance, because a crossroads is only a crossroads
/// once somebody is standing on the other side of it. Every field is a measurement
/// the census already took, so the choice can be audited line by line.
fn build_epoch_candidates(
    stats: &[ProvinceStats],
    hierarchy: &GeopoliticalHierarchy,
    owner: &[u16],
    contacts: &[Vec<Contact>],
    seats: &[u32],
) -> Vec<EpochCandidate> {
    let mut out = Vec::new();
    for (i, s) in stats.iter().enumerate() {
        if !s.present {
            continue;
        }
        let node = &hierarchy.provinces[i];
        let prov = i as u32;

        // Crossroads: which realms, and how many great regions, meet here.
        let mut frontier_realm_ids: Vec<u16> = Vec::new();
        let mut superregions: Vec<u32> = vec![node.superregion_id];
        for &(nb, _) in &node.neighbors {
            let nb_idx = nb as usize;
            let other_owner = owner.get(nb_idx).copied().unwrap_or(0);
            if other_owner != 0 && !frontier_realm_ids.contains(&other_owner) {
                frontier_realm_ids.push(other_owner);
            }
            if let Some(superregion) = hierarchy
                .provinces
                .get(nb_idx)
                .map(|n| n.superregion_id)
                .filter(|sr| !superregions.contains(sr))
            {
                superregions.push(superregion);
            }
        }

        // A story travels with trade: how far could the holder's word carry?
        let holder = owner.get(i).copied().unwrap_or(0);
        let realm_contacts = if holder == 0 {
            0
        } else {
            contacts.get(holder as usize - 1).map_or(0, |c| c.len())
        };

        out.push(EpochCandidate {
            prov,
            area_km2: s.area_km2,
            hab: s.hab_mean(),
            temp: s.temp_mean(),
            precip: s.precip_mean(),
            coastal: s.sea_px > 0,
            lakeshore: s.lake_px > 0,
            island: node.anchor_id != node.id,
            peak_m: s.elev_max_m,
            mean_m: s.elev_mean_m(),
            flow_max: s.flow_max,
            frontier_realms: frontier_realm_ids.len(),
            group_span: superregions.len(),
            realm_contacts,
            is_seat: seats.contains(&prov),
        });
    }
    out
}

/// The founding years of every cradle after the first, in the world's internal
/// frame: one bounded draw each, sorted so the roster stays in founding order.
///
/// `nearest_elder[rank]` is how far cradle `rank` lies from the nearest cradle that
/// rose before it. The further it is, the longer the seed took to reach it —
/// crossing an ocean or an empty continent costs generations that a neighbouring
/// floodplain does not — so an isolated cradle's draw is biased towards the late end
/// of the window. The bias is bounded by the window itself: it can delay a cradle,
/// never stretch the era, however many cradles the world carries.
fn founding_offsets(
    nearest_elder: &[f32],
    span: i32,
    isolated_distance_px: f32,
    rng: &mut ChaCha8Rng,
) -> Vec<i32> {
    let span = span.max(1) as f32;
    let mut offsets: Vec<i32> = nearest_elder
        .iter()
        .map(|&nearest| {
            let u = rng.random::<f32>();
            let u = if nearest > isolated_distance_px {
                u.sqrt()
            } else {
                u
            };
            (u * span).round() as i32
        })
        .collect();
    offsets.sort_unstable();
    offsets
}

/// Decide the world's epoch: where the event happened, and when.
///
/// The year is placed *after* the map date, not before it. The atlas portrays the
/// world in its dawn — a few centuries to a couple of millennia before its
/// calendar's own year zero — so every date it shows is BCE, exactly as a map of our
/// own 2000 BCE is dated in the years before the Common Era. The epoch stays a
/// *convention*, not a beginning: as later builds carry the ladder past this horizon,
/// worlds will begin to stand after the event and read CE.
///
/// The place is still chosen for the shape a founding story takes, and is still
/// never a cradle: the epoch of a real calendar is a crossroads whose story travelled
/// further than any army could, not the capital of whoever was strongest.
#[allow(clippy::too_many_arguments)]
fn choose_epoch(
    site: Option<u32>,
    candidates: &[EpochCandidate],
    owner: &[u16],
    config: &CivilizationConfig,
    hierarchy: &GeopoliticalHierarchy,
    atlas_year: i32,
    rng: &mut ChaCha8Rng,
) -> Epoch {
    let site = site.unwrap_or(0);
    let site_realm = owner
        .get(site as usize)
        .copied()
        .filter(|&o| o != 0)
        .map(u32::from);

    // A later convention, so the world it dates stands before it.
    let (epoch_lo, epoch_hi) = config.epoch_after_atlas_years;
    let year = atlas_year + rng.random_range(epoch_lo..=epoch_hi);

    let candidate = candidates.iter().find(|c| c.prov == site);
    let (site_x, site_y) = hierarchy
        .provinces
        .get(site as usize)
        .map_or((0.0, 0.0), |n| (n.center_x, n.center_y));
    let site_name = candidate
        .map(place_name)
        .unwrap_or_else(|| format!("Province #{site}"));

    Epoch {
        absolute_year: year,
        site_province: site,
        site_x,
        site_y,
        site_name,
        site_realm,
    }
}

/// Union-find root of an ancestral-stock family, path-compressed.
fn family_find(parent: &mut Vec<usize>, mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

// ─── THE DEEP-TIME DEVELOPMENT PASS ─────────────────────────────────────────

/// Which realms share a frontier, for the development pass.
///
/// The rich [`Contact`] pass runs after the dossiers and is far too expensive to
/// repeat here; this is only the cheap half of it — two realms are adjacent when
/// one holds a province whose neighbour the other holds. Sorted, so the walk over
/// it is order-stable.
fn frontier_adjacency(owner: &[u16], hierarchy: &GeopoliticalHierarchy) -> BTreeMap<u16, Vec<u16>> {
    let mut adjacency: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for (i, node) in hierarchy.provinces.iter().enumerate() {
        let a = owner.get(i).copied().unwrap_or(0);
        if a == 0 {
            continue;
        }
        for &(neighbour, _) in &node.neighbors {
            let b = owner.get(neighbour as usize).copied().unwrap_or(0);
            if b == 0 || b == a {
                continue;
            }
            adjacency.entry(a).or_default().push(b);
        }
    }
    for neighbours in adjacency.values_mut() {
        neighbours.sort_unstable();
        neighbours.dedup();
    }
    adjacency
}

/// Give every society its colour: the tier carries the hue, the society the shade.
///
/// Graph colouring is hopeless here — a partitioned world carries hundreds of
/// societies and no palette can give them all a distinct tone. What matters instead
/// is that the map says at a glance *what kind of world this is*: the five tiers
/// take five hues, and each society inside a tier steps its own shade so neighbours
/// in one belt still read apart. Order-stable, so the same seed paints the same map.
fn assign_colors(skeletons: &[Skeleton]) -> Vec<[u8; 3]> {
    let mut seen = [0usize; 5];
    skeletons
        .iter()
        .map(|sk| {
            let slot = &mut seen[sk.tier.index() as usize];
            let color = tier_shade(sk.tier, *slot);
            *slot += 1;
            color
        })
        .collect()
}

/// Integrate every *state's* development from the first city to the atlas date.
///
/// Year zero is the first city and `map_year` is the atlas date, stepped in fixed
/// [`DEVELOPMENT_STEP_YEARS`] increments. At each step a state grows from its own
/// surplus — a logistic climb, slow at first, fast once the country is worked, and
/// flattening as it approaches the horizon — and takes a share of any neighbour's
/// lead, in proportion to the gap and to how open its ethos is. A state that was
/// not yet founded does not exist and neither grows nor lends. When it is founded
/// it enters with the part of its elder kin's development it inherited, or from
/// nothing but a first village if it has no elder.
///
/// Only the first `states` skeletons take part. A band of foragers, a herd people
/// and a farming village are not on this axis at all: they are different kinds of
/// society rather than less-developed states, and their place on the government
/// axis is set from their tier. Their neighbours still lend to the states that
/// touch them, so a state among farmers is not treated as though it stood alone.
///
/// Every step reads one snapshot of the previous step's levels, so the rank order
/// of the loops can never bias the result. Returns the frontier: the highest level
/// any state reached at the atlas date.
#[allow(clippy::too_many_arguments)]
fn simulate_development(
    skeletons: &mut [Skeleton],
    schemes: &[Scheme],
    family_ancestor: &[usize],
    adjacency: &BTreeMap<u16, Vec<u16>>,
    stats: &[ProvinceStats],
    min_river_flow: f32,
    great_river_flow: f32,
    map_year: i32,
    pace: f32,
    states: usize,
) -> f64 {
    let n = states.min(skeletons.len());
    if n == 0 {
        return 0.0;
    }

    // How much country each state holds, in people at full development. It is the
    // modest "population" term of the growth rate: a broad, well-peopled realm
    // learns faster than a single valley, but not much faster.
    let capacity: Vec<f64> = skeletons[..n]
        .iter()
        .map(|sk| {
            sk.provinces
                .iter()
                .map(|&p| stats[p as usize].pop_seed)
                .sum()
        })
        .collect();
    let max_capacity = capacity.iter().copied().fold(0.0f64, f64::max).max(1.0);

    // Each realm's own growth rate: how kind its cradle is — soil, river, shore —
    // times how much country it holds, times the world's pace.
    let growth: Vec<f64> = (0..n)
        .map(|rank| {
            let s = &skeletons[rank].seat_stats;
            let soil = (s.hab_mean().max(0.05) as f64).sqrt().clamp(0.30, 1.20);
            let river = if s.flow_max >= great_river_flow {
                1.45
            } else if s.flow_max >= min_river_flow {
                1.18
            } else {
                1.0
            };
            let shore = if s.is_coastal() || s.lake_px > 0 {
                1.06
            } else {
                1.0
            };
            let size = 0.75 + 0.5 * (capacity[rank] / max_capacity).sqrt();
            DEVELOPMENT_BASE_RATE * pace.max(0.05) as f64 * soil * river * shore * size
        })
        .collect();

    let mut level = vec![0.0f64; n];
    let mut active = vec![false; n];

    let mut year = 0i32;
    while year < map_year {
        let dt = (map_year - year).min(DEVELOPMENT_STEP_YEARS) as f64;

        // Founding: a realm enters the axis at the first step at or after its
        // founding year, carrying whatever its elder kin had already built.
        for rank in 0..n {
            if active[rank] || skeletons[rank].founding_year_internal > year {
                continue;
            }
            active[rank] = true;
            let ancestor = family_ancestor[rank];
            // An ancestor that is a people, not a state, is not on this axis.
            let inherited = if ancestor != rank && ancestor < n && active[ancestor] {
                level[ancestor] * DEVELOPMENT_INHERITANCE
            } else {
                0.0
            };
            level[rank] = inherited.max(DEVELOPMENT_SEED);
        }

        // One snapshot for the whole step: growth and diffusion never see a value
        // another realm wrote in the same pass.
        let snapshot = level.clone();
        let mut delta = vec![0.0f64; n];
        for rank in 0..n {
            if !active[rank] {
                continue;
            }
            let current = snapshot[rank];
            let logistic = growth[rank] * current * (1.0 - current);
            let openness = schemes[rank].outlook.openness() as f64;
            let lead = adjacency.get(&(rank as u16 + 1)).map_or(0.0, |neighbours| {
                let mut gap = 0.0;
                let mut count = 0.0;
                for &neighbour in neighbours {
                    // A neighbour that is a people, not a state, is not on this
                    // axis and has no level to lend.
                    if neighbour as usize > n {
                        continue;
                    }
                    let other = snapshot[neighbour as usize - 1];
                    if other > current {
                        gap += other - current;
                        count += 1.0;
                    }
                }
                if count > 0.0 { gap / count } else { 0.0 }
            });
            delta[rank] = (logistic + DEVELOPMENT_DIFFUSION * openness * lead) * dt;
        }
        for rank in 0..n {
            if active[rank] {
                level[rank] = (level[rank] + delta[rank]).clamp(0.0, 1.0);
            }
        }

        year += DEVELOPMENT_STEP_YEARS;
    }

    let mut frontier = 0.0f64;
    for rank in 0..n {
        skeletons[rank].complexity = level[rank] as f32;
        frontier = frontier.max(level[rank]);
    }
    frontier
}

// ─── READING THE COUNTRY ─────────────────────────────────────────────────────

/// Read a cradle out of the country it sits in.
///
/// The order of these tests *is* the model: an island is an island even if a river
/// crosses it, a river mouth is a delta before it is a floodplain, and a shore
/// with fish beats dry grassland. Everything the reader is later told about the
/// realm follows from this one word — its economy, its social order, its faith and
/// its architecture all branch off it.
fn classify_cradle(
    stats: &ProvinceStats,
    hierarchy: &GeopoliticalHierarchy,
    seat: u32,
    min_river_flow: f32,
    great_river_flow: f32,
) -> CradleKind {
    let node = &hierarchy.provinces[seat as usize];
    let on_island = node.anchor_id != 0 && node.anchor_id != seat;
    let temp = stats.temp_mean();
    let precip = stats.precip_mean();
    let has_major_river = stats.flow_max >= great_river_flow;
    let has_river = stats.flow_max >= min_river_flow;

    if on_island && stats.is_coastal() {
        CradleKind::IslandShore
    } else if has_major_river && stats.is_coastal() {
        CradleKind::RiverDelta
    } else if has_major_river {
        CradleKind::RiverFloodplain
    } else if stats.lake_px > 0 && !has_river {
        CradleKind::Lacustrine
    } else if precip < 0.20 && (stats.is_coastal() || stats.lake_px > 0 || has_river) {
        CradleKind::DesertOasis
    } else if stats.elev_mean_m() >= 900.0 {
        CradleKind::HighlandValley
    } else if temp >= 24.0 && precip >= 0.46 {
        CradleKind::RainforestClearing
    } else if temp >= 20.0 && precip >= 0.20 {
        CradleKind::SavannaHeartland
    } else if precip < 0.34 {
        // Temperate to cold dry grass: enough for herds, not for a plough.
        CradleKind::SteppeGrassland
    } else if stats.is_coastal() {
        CradleKind::CoastalMaritime
    } else {
        // Temperate, well-watered, inland and low: the forest clearing that fed
        // the loess civilizations — grain on the field, timber on the ridge.
        CradleKind::WoodlandHeartland
    }
}

/// The cradle kind a *people's* country is closest to.
///
/// A band of foragers has no cradle — nobody founded them — but the rest of the
/// dossier (faith, architecture, the prose) branches off [`CradleKind`], so their
/// country is read into that vocabulary rather than duplicating it.
fn environment_cradle_kind(environment: Environment) -> CradleKind {
    match environment {
        Environment::IceMargin | Environment::Tundra | Environment::Steppe => {
            CradleKind::SteppeGrassland
        }
        Environment::Taiga | Environment::Woodland => CradleKind::WoodlandHeartland,
        Environment::Savanna => CradleKind::SavannaHeartland,
        Environment::Desert => CradleKind::DesertOasis,
        Environment::TropicalForest => CradleKind::RainforestClearing,
        Environment::Highland => CradleKind::HighlandValley,
        Environment::IslandShore => CradleKind::IslandShore,
        Environment::Lacustrine => CradleKind::Lacustrine,
        Environment::Floodplain => CradleKind::RiverFloodplain,
        Environment::Delta => CradleKind::RiverDelta,
        Environment::Coast => CradleKind::CoastalMaritime,
    }
}

/// What a people's country lets them eat, decided by their tier rather than drawn.
///
/// A band of foragers forages, a herd people herds, and a farming society farms —
/// with the *kind* of farming still following the country, which is the one part of
/// this that geography has always decided.
fn subsistence_for(tier: SocietyTier, environment: Environment, rng: &mut ChaCha8Rng) -> Subsistence {
    match tier {
        SocietyTier::HunterGatherers => Subsistence::Foraging,
        SocietyTier::NomadicPastoralists => Subsistence::Pastoralism,
        SocietyTier::SimpleFarmers | SocietyTier::ComplexFarmers => match environment {
            Environment::TropicalForest => Subsistence::ShiftingCultivation,
            Environment::Highland => Subsistence::TerraceFarming,
            Environment::Delta
            | Environment::Floodplain
            | Environment::Desert
            | Environment::Lacustrine => Subsistence::IrrigatedAgriculture,
            Environment::IslandShore | Environment::Coast => {
                if rng.random::<f32>() < 0.5 {
                    Subsistence::FishingAndSeafaring
                } else {
                    Subsistence::DrylandFarming
                }
            }
            _ => {
                if rng.random::<f32>() < 0.75 {
                    Subsistence::DrylandFarming
                } else {
                    Subsistence::Pastoralism
                }
            }
        },
        // A state's economy is drawn from its cradle as it always was; this arm is
        // only reached for a state whose cradle read oddly.
        SocietyTier::States => Subsistence::DrylandFarming,
    }
}

/// Where on the development axis a people sits, from its tier alone.
///
/// A band of foragers and a chiefdom are not *less* developed states on the same
/// climb — they are different kinds of society — but the government axis is shared,
/// so each tier is given the band of the axis its own institutions belong to. A
/// state is not set here: its level is measured by the deep-time pass.
fn tier_complexity(tier: SocietyTier, rng: &mut ChaCha8Rng) -> f32 {
    let (floor, spread) = match tier {
        SocietyTier::HunterGatherers => (0.010, 0.040),
        SocietyTier::NomadicPastoralists => (0.080, 0.050),
        SocietyTier::SimpleFarmers => (0.110, 0.040),
        SocietyTier::ComplexFarmers => (0.190, 0.090),
        SocietyTier::States => (0.300, 0.0),
    };
    floor + rng.random::<f32>() * spread
}

// ─── WHAT THE COUNTRY CAN FEED ───────────────────────────────────────────────

/// What a province's own country can feed, before anybody has decided anything.
///
/// This is the environmental half of the model. It knows nothing about states,
/// distances or history: a floodplain is farmable whether or not anyone has
/// reached it yet, and that distinction is exactly what the tier rule uses.
#[derive(Clone, Copy, PartialEq, Debug)]
enum LandUse {
    /// Neither a field nor a herd: the country is hunted, fished and gathered.
    Foraging,
    /// Dry grass enough for herds, and not enough for a plough.
    Pastoralism,
    /// Ground that will carry a field, with how good it is in `0..=1`.
    Farming { potential: f32 },
}

/// Everything the tier rule needs about one province's country.
#[derive(Clone, Copy)]
struct ProvinceLand {
    environment: Environment,
    land_use: LandUse,
    /// The two potentials themselves, kept so the rule can threshold on them.
    farm: f32,
    pasture: f32,
}

/// A 0 → 1 ramp, so every threshold in this file reads the same way.
fn ramp(value: f32, lo: f32, hi: f32) -> f32 {
    if hi <= lo {
        return if value >= hi { 1.0 } else { 0.0 };
    }
    ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// The biome a province's people live in, read off its own measurements.
///
/// Deliberately a *total* function over every inhabited province. Where
/// [`classify_cradle`] answers "why did a state rise *here*?", this answers "what
/// is the country like?" for a forest band and a herd people too — and those two
/// are most of the world.
fn classify_environment(
    stats: &ProvinceStats,
    node: &ProvinceNode,
    min_river_flow: f32,
    great_river_flow: f32,
) -> Environment {
    let temp = stats.temp_mean();
    let precip = stats.precip_mean();
    let on_island = node.anchor_id != 0 && node.anchor_id != node.id;
    let has_major_river = stats.flow_max >= great_river_flow;
    let has_river = stats.flow_max >= min_river_flow;

    // Water first: a great river outweighs the climate it runs through, which is
    // why a floodplain in the desert is still a floodplain.
    if temp < -2.0 {
        return Environment::IceMargin;
    }
    if has_major_river && stats.is_coastal() {
        return Environment::Delta;
    }
    if has_major_river {
        return Environment::Floodplain;
    }
    if on_island && stats.is_coastal() {
        return Environment::IslandShore;
    }
    if stats.lake_px > 0 && !has_river {
        return Environment::Lacustrine;
    }
    if stats.elev_mean_m() >= 1_200.0 {
        return Environment::Highland;
    }
    if precip < 0.10 {
        return Environment::Desert;
    }
    if temp < 2.0 {
        return Environment::Tundra;
    }
    if temp < 7.0 {
        return Environment::Taiga;
    }
    if precip < 0.28 {
        return if temp >= 20.0 {
            Environment::Savanna
        } else {
            Environment::Steppe
        };
    }
    if temp >= 24.0 && precip >= 0.46 {
        return Environment::TropicalForest;
    }
    if temp >= 20.0 {
        return Environment::Savanna;
    }
    if stats.is_coastal() {
        return Environment::Coast;
    }
    Environment::Woodland
}

/// How well a province would carry a field, in `0..=1`.
///
/// Soil, water, warmth and gentle ground, in that order of importance: a great
/// river can make a desert farmable, and nothing can make a mountain farmable.
fn farm_potential(stats: &ProvinceStats, min_river_flow: f32, great_river_flow: f32) -> f32 {
    let hab = stats.hab_mean().clamp(0.0, 1.4) / 1.4;
    let temp = stats.temp_mean();
    let precip = stats.precip_mean();
    let river = if stats.flow_max >= great_river_flow {
        1.80
    } else if stats.flow_max >= min_river_flow {
        1.35
    } else {
        1.0
    };
    // Rain helps up to a point, then leaches the soil and drowns the seed.
    let rain = ramp(precip, 0.10, 0.45) * (1.0 - 0.35 * ramp(precip, 0.75, 1.10));
    // Warmth helps up to a point, then the ground bakes.
    let warmth = ramp(temp, 2.0, 12.0) * (1.0 - 0.45 * ramp(temp, 30.0, 42.0));
    let relief = 1.0 - (stats.elev_mean_m() / 2_500.0).clamp(0.0, 0.5);
    hab.powf(1.4) * river * rain * warmth * relief
}

/// How well a province would carry herds, in `0..=1`.
///
/// Dry grass is the whole point: too much rain grows a forest a herd cannot graze,
/// and too little grows nothing at all. This is why the pastoralist belt of a world
/// sits between the desert and the plough country, and nowhere else.
fn pasture_potential(stats: &ProvinceStats) -> f32 {
    let temp = stats.temp_mean();
    let precip = stats.precip_mean();
    let dryness = (1.0 - (precip - 0.15).abs() / 0.18).clamp(0.0, 1.0);
    let warmth = ramp(temp, -2.0, 8.0) * (1.0 - 0.5 * ramp(temp, 26.0, 38.0));
    let relief = 1.0 - (stats.elev_mean_m() / 3_000.0).clamp(0.0, 0.6);
    let openness = 1.0 - 0.5 * ramp(precip, 0.30, 0.55);
    (0.35 + 0.65 * openness) * dryness * warmth * relief
}

/// Read one province's whole country: its biome and what it can feed.
fn read_land(
    stats: &ProvinceStats,
    node: &ProvinceNode,
    min_river_flow: f32,
    great_river_flow: f32,
    config: &CivilizationConfig,
) -> ProvinceLand {
    let environment = classify_environment(stats, node, min_river_flow, great_river_flow);
    let farm = farm_potential(stats, min_river_flow, great_river_flow);
    let pasture = pasture_potential(stats);
    let land_use = if farm >= config.farm_floor {
        LandUse::Farming { potential: farm }
    } else if pasture >= config.pasture_floor {
        LandUse::Pastoralism
    } else {
        LandUse::Foraging
    };
    ProvinceLand {
        environment,
        land_use,
        farm,
        pasture,
    }
}

// ─── THE DIFFUSION HALF ──────────────────────────────────────────────────────

/// The cost field a diffusion leaves behind: how far every province lies from the
/// nearest seed, and which seed that was.
///
/// One queue, seeded at every cradle at once, replaces the old race in which the
/// oldest realm drained the world before the second one was considered. All the
/// states now grow together, cheapest ground first, exactly as they did — and the
/// field they leave behind is the second half of the tier rule.
struct InfluenceField {
    /// Cheapest accumulated cost from any state's seat; `f32::MAX` where no route
    /// exists at all (an island nobody reached).
    cost: Vec<f32>,
    /// Which state reached the province first, as a 1-based society id.
    nearest: Vec<u16>,
}

/// What it costs a frontier to step from one province into another.
///
/// Fertile ground is cheap ground: this is what bends a frontier along a river
/// valley instead of along a straight line, and what makes a mountain wall a wall
/// that history did not cross. `None` when the step is over the ceiling.
fn edge_step(
    from: &ProvinceNode,
    to: &ProvinceNode,
    to_stats: &ProvinceStats,
    edge: u32,
    width: usize,
    config: &CivilizationConfig,
) -> Option<f32> {
    let hop = sphere::wrap_distance_px(from.center_x, from.center_y, to.center_x, to.center_y, width);
    let fertility = 0.35 + to_stats.hab_mean().clamp(0.0, 1.2);
    let jump = if hop <= 90.0 {
        1.0
    } else {
        (hop / 90.0).powf(1.6)
    };
    let step = (edge as f32) * 0.35 / fertility * jump;
    if step > config.max_step_cost {
        return None;
    }
    Some(step)
}

/// Grow every state at once and leave the influence field behind.
///
/// Each province is claimed by whichever state reaches it most cheaply, up to that
/// state's own target, so the claims are a cost-Voronoi partition of the world —
/// contiguous by construction, because a claim is the first `target` provinces of
/// one state's own search tree.
fn grow_states(
    seats: &[u32],
    targets: &[usize],
    owner: &mut [u16],
    hierarchy: &GeopoliticalHierarchy,
    stats: &[ProvinceStats],
    width: usize,
    config: &CivilizationConfig,
) -> (InfluenceField, Vec<Vec<u32>>) {
    let n = owner.len();
    let mut field = InfluenceField {
        cost: vec![f32::MAX; n],
        nearest: vec![0; n],
    };
    let mut claimed: Vec<Vec<u32>> = vec![Vec::new(); seats.len()];
    let mut heap: BinaryHeap<Step> = BinaryHeap::new();

    for (rank, &seat) in seats.iter().enumerate() {
        let id = rank as u16 + 1;
        field.cost[seat as usize] = 0.0;
        field.nearest[seat as usize] = id;
        heap.push(Step {
            cost: 0.0,
            prov: seat,
            civ: id,
        });
    }

    while let Some(Step { cost, prov, civ }) = heap.pop() {
        if cost > field.cost[prov as usize] + 1e-3 {
            continue; // a stale heap entry
        }
        let rank = civ as usize - 1;
        if owner[prov as usize] == 0 && claimed[rank].len() < targets[rank] {
            owner[prov as usize] = civ;
            claimed[rank].push(prov);
        }

        let node = &hierarchy.provinces[prov as usize];
        for &(nb, edge) in &node.neighbors {
            let nb_idx = nb as usize;
            if nb_idx >= n {
                continue;
            }
            let Some(step) = edge_step(
                node,
                &hierarchy.provinces[nb_idx],
                &stats[nb_idx],
                edge,
                width,
                config,
            ) else {
                continue;
            };
            let next = cost + step;
            if next < field.cost[nb_idx] {
                field.cost[nb_idx] = next;
                field.nearest[nb_idx] = civ;
                heap.push(Step {
                    cost: next,
                    prov: nb,
                    civ,
                });
            }
        }
    }

    for list in claimed.iter_mut() {
        list.sort_unstable();
    }
    (field, claimed)
}

/// Diffuse a cost field from a set of seeds, with no claims and a budget.
///
/// Used for the independent neolithics: a fertile country nobody reached, where
/// farming was worked out locally rather than brought in. The budget is how far
/// such a people's own neolithic spreads from its best ground.
fn diffuse_field(
    seeds: &[u32],
    hierarchy: &GeopoliticalHierarchy,
    stats: &[ProvinceStats],
    width: usize,
    config: &CivilizationConfig,
    budget: f32,
) -> Vec<f32> {
    let n = hierarchy.provinces.len();
    let mut cost = vec![f32::MAX; n];
    if seeds.is_empty() {
        return cost;
    }
    let mut heap: BinaryHeap<Step> = BinaryHeap::new();
    for &seed in seeds {
        cost[seed as usize] = 0.0;
        heap.push(Step {
            cost: 0.0,
            prov: seed,
            civ: 1,
        });
    }
    while let Some(Step { cost: c, prov, .. }) = heap.pop() {
        if c > cost[prov as usize] + 1e-3 || c > budget {
            continue;
        }
        let node = &hierarchy.provinces[prov as usize];
        for &(nb, edge) in &node.neighbors {
            let nb_idx = nb as usize;
            if nb_idx >= n {
                continue;
            }
            let Some(step) = edge_step(
                node,
                &hierarchy.provinces[nb_idx],
                &stats[nb_idx],
                edge,
                width,
                config,
            ) else {
                continue;
            };
            let next = c + step;
            if next < cost[nb_idx] {
                cost[nb_idx] = next;
                heap.push(Step {
                    cost: next,
                    prov: nb,
                    civ: 1,
                });
            }
        }
    }
    cost
}

// ─── WHERE THE TWO HALVES MEET ───────────────────────────────────────────────

/// Decide which of the five kinds of society holds each province.
///
/// The country says what is *possible*; the influence field says what has
/// *happened*. A province inside a state's own claim is the state; farming ground
/// within reach of a state is farmed, ranked or ruled according to how close it
/// lies; farming ground beyond that reach is still wild unless its own country is
/// good enough to have risen by itself; dry grass carries herds; everything else is
/// hunted.
///
/// The seeded wobble on the two thresholds is what makes a belt a *mosaic* — simple
/// farmers inside a chiefdom country, a chiefdom far out among the villages —
/// rather than a set of concentric rings.
fn assign_tiers(
    owner: &[u16],
    influence: &InfluenceField,
    independent: &[f32],
    land: &[ProvinceLand],
    stats: &[ProvinceStats],
    config: &CivilizationConfig,
    rng: &mut ChaCha8Rng,
) -> Vec<u8> {
    let mut tiers = vec![0u8; owner.len()];
    for i in 0..owner.len() {
        if !stats[i].present {
            continue;
        }
        if owner[i] != 0 {
            tiers[i] = SocietyTier::States.index();
            continue;
        }
        let cost = influence.cost[i];
        let jitter = if cost.is_finite() {
            rng.random_range(0.85..1.15f32)
        } else {
            1.0
        };
        let reach = cost * jitter;
        let tier = match land[i].land_use {
            LandUse::Farming { .. } => {
                if reach <= config.chiefdom_cost {
                    SocietyTier::ComplexFarmers
                } else if reach <= config.farm_cost {
                    SocietyTier::SimpleFarmers
                } else if independent[i] <= config.independent_reach {
                    // An isolated neolithic: its own country was good enough to be
                    // worked without anyone bringing the seed. The best ground of
                    // such a country grows chiefs, the rest only villages.
                    if land[i].farm >= config.rise_floor * 1.5 {
                        SocietyTier::ComplexFarmers
                    } else {
                        SocietyTier::SimpleFarmers
                    }
                } else {
                    // Farmable country nobody has farmed yet — the Amazon, the
                    // Congo, the Siberian river valleys.
                    SocietyTier::HunterGatherers
                }
            }
            LandUse::Pastoralism => SocietyTier::NomadicPastoralists,
            LandUse::Foraging => SocietyTier::HunterGatherers,
        };
        tiers[i] = tier.index();
    }
    tiers
}

/// Which cultural area a province's people belong to.
///
/// Farming peoples belong to the hinterland of the state that is nearest in
/// influence, which is what makes one state's farming country a single cultural
/// area rather than a generic green field. Everybody else belongs to their own
/// landmass: a taiga is one country.
fn group_key(tier: u8, prov: usize, nearest: &[u16], stats: &[ProvinceStats]) -> u32 {
    match SocietyTier::from_index(tier) {
        SocietyTier::SimpleFarmers | SocietyTier::ComplexFarmers => nearest[prov] as u32,
        _ => stats[prov].continent_id,
    }
}

/// How many provinces one society of this tier may span before it is split.
fn province_cap(tier: u8, config: &CivilizationConfig) -> usize {
    match SocietyTier::from_index(tier) {
        SocietyTier::HunterGatherers => config.forager_max_provinces,
        SocietyTier::NomadicPastoralists => config.pastoralist_max_provinces,
        SocietyTier::SimpleFarmers => config.simple_farmer_max_provinces,
        SocietyTier::ComplexFarmers => config.complex_farmer_max_provinces,
        SocietyTier::States => usize::MAX,
    }
}

/// Turn a tier map into the societies that hold it.
///
/// The states come first, in rank order, so a state's id never depends on how many
/// peoples were assembled before it. Then every remaining province is walked in id
/// order and flooded outward through its own tier and cultural area, up to that
/// tier's cap — so a belt too large to be one people is cut into contiguous pieces,
/// deterministically, for a given seed.
fn assemble_societies(
    tiers: &[u8],
    owner: &[u16],
    nearest: &[u16],
    stats: &[ProvinceStats],
    hierarchy: &GeopoliticalHierarchy,
    state_count: usize,
    config: &CivilizationConfig,
) -> (Vec<Vec<u32>>, Vec<u8>) {
    let n = tiers.len();
    let mut societies: Vec<Vec<u32>> = vec![Vec::new(); state_count];
    let mut society_tier: Vec<u8> = vec![SocietyTier::States.index(); state_count];
    let mut taken = vec![false; n];

    for i in 0..n {
        if !stats[i].present {
            continue;
        }
        let state = owner[i] as usize;
        if state > 0 && state <= state_count {
            societies[state - 1].push(i as u32);
            taken[i] = true;
        }
    }

    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    for start in 0..n {
        if taken[start] || !stats[start].present {
            continue;
        }
        let tier = tiers[start];
        let group = group_key(tier, start, nearest, stats);
        let cap = province_cap(tier, config);
        let mut piece: Vec<u32> = Vec::new();
        queue.clear();
        queue.push_back(start as u32);
        while let Some(prov) = queue.pop_front() {
            // Taken is claimed on the pop, never on the push: a piece that stops at
            // its cap must leave the provinces it had queued unclaimed, or they
            // would be dropped from the map entirely.
            if taken[prov as usize] {
                continue;
            }
            taken[prov as usize] = true;
            piece.push(prov);
            if piece.len() >= cap {
                break;
            }
            let node = &hierarchy.provinces[prov as usize];
            for &(nb, _) in &node.neighbors {
                let idx = nb as usize;
                if idx >= n || taken[idx] || !stats[idx].present {
                    continue;
                }
                if tiers[idx] != tier || group_key(tiers[idx], idx, nearest, stats) != group {
                    continue;
                }
                queue.push_back(nb);
            }
        }
        piece.sort_unstable();
        societies.push(piece);
        society_tier.push(tier);
    }

    (societies, society_tier)
}

/// A tier's base hue, stepped for the `index`-th society of that tier.
///
/// Five hues cannot tell four hundred societies apart, so the tier carries the
/// colour and the society carries the shade: neighbours inside one belt still read
/// apart, and the map still says at a glance *what kind of world this is*. The walk
/// alternates lighter and darker and drifts a little further with every step, so a
/// belt of twelve farming peoples never repeats a shade next door.
fn tier_shade(tier: SocietyTier, index: usize) -> [u8; 3] {
    let base = tier.legend_color();
    let sign = if index % 2 == 0 { 1.0 } else { -1.0 };
    let step = (index / 2) as f32;
    let shift = sign * (0.055 * step.min(3.0) + 0.025 * ((index / 8) % 3) as f32);
    let mut out = [0u8; 3];
    for channel in 0..3 {
        let value = base[channel] as f32 / 255.0;
        let value = if shift >= 0.0 {
            value + (1.0 - value) * shift
        } else {
            value * (1.0 + shift)
        };
        out[channel] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out
}

/// How much a realm's development multiplies its provinces' carrying capacity.
///
/// Three things push the same number: the realm's own development (a canal state
/// that taxes, stores and redistributes grain feeds far more mouths per hectare
/// than a scatter of first villages), how it is governed, and what it values.
fn development_multiplier(government: Government, outlook: Outlook, complexity: f32) -> f32 {
    let development = 0.40 + 0.60 * complexity.clamp(0.0, 1.0);
    (development + government.density_bias() + outlook.density_bias()).clamp(0.40, 1.45)
}

// ─── RELATIONS ───────────────────────────────────────────────────────────────

/// Ways two realms that share a land border treat each other.
const FRONTIER_RELATIONS: [&str; 6] = [
    "trade partners",
    "uneasy rivals",
    "rivals for the same river",
    "raiding neighbours",
    "kin who share the frontier",
    "a wary peace along the border",
];

/// Ways two realms separated by water treat each other.
const SEA_RELATIONS: [&str; 5] = [
    "trading partners across the water",
    "rival merchants",
    "colonists of the same sea",
    "fisherfolk who share the fishing grounds",
    "rival captains",
];

/// Ways two realms that know of each other only by report treat each other.
const RUMOUR_RELATIONS: [&str; 4] = [
    "a name in travellers' tales",
    "traders by proxy",
    "a half-believed rumour",
    "a distant market for the same goods",
];

/// Choose how two contacting realms treat each other.
///
/// A martial ethos leans the draw towards rivalry and a mercantile one towards
/// trade, so the relation reads off the two dossiers instead of contradicting
/// them.
fn pick_relation(rng: &mut ChaCha8Rng, kind: ContactKind, a: Outlook, b: Outlook) -> &'static str {
    let mut pool: Vec<&'static str> = match kind {
        ContactKind::LandFrontier => FRONTIER_RELATIONS.to_vec(),
        ContactKind::SeaRoute => SEA_RELATIONS.to_vec(),
        ContactKind::DistantRumour => RUMOUR_RELATIONS.to_vec(),
    };

    let martial = a == Outlook::Martial || b == Outlook::Martial;
    let mercantile = a == Outlook::Mercantile || b == Outlook::Mercantile;
    if martial {
        let rivals: Vec<&'static str> = pool
            .iter()
            .copied()
            .filter(|r| r.contains("rival") || r.contains("raid"))
            .collect();
        pool.extend(rivals);
    }
    if mercantile {
        let traders: Vec<&'static str> = pool
            .iter()
            .copied()
            .filter(|r| r.contains("trade") || r.contains("traders") || r.contains("market"))
            .collect();
        pool.extend(traders);
    }

    *pool.choose(rng).unwrap()
}

/// A sentence describing one side of a contact, from that side's point of view.
fn contact_note(kind: ContactKind, relation: &str, distance_km: f32, other: &str) -> String {
    match kind {
        ContactKind::LandFrontier => format!(
            "Its provinces meet those of {other} — a shared border, and across it the two are {relation}."
        ),
        ContactKind::SeaRoute => format!(
            "It reaches {other} across about {} km of water, and the two are {relation}.",
            thousands(distance_km)
        ),
        ContactKind::DistantRumour => format!(
            "It knows {other} only by report, from roughly {} km away, where the two are {relation}.",
            thousands(distance_km)
        ),
    }
}

// ─── SMALL HELPERS ───────────────────────────────────────────────────────────

/// What a realm is known for abroad — the shop list of prehistory.
///
/// Every entry is drawn from the realm's own dossier, so a borrowed habit is
/// always something the older civilization genuinely had.
fn hallmarks(culture: &Culture, faith: &Faith) -> Vec<&'static str> {
    vec![
        culture.subsistence.hallmark(),
        culture.government.hallmark(),
        faith.kind.hallmark(),
        culture.architecture,
    ]
}

/// `Mud-brick temple platforms` → `mud-brick temple platforms`, so a borrowed
/// hallmark reads inside a sentence.
/// `Riparian Tongue` → `Riparian tongue`, so a borrowed hallmark reads inside a
/// sentence.
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `woodland` → `Woodland`, so a descriptive society name opens like a name.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `[a, b, c]` → `a, b and c`.
fn join_list<T: AsRef<str>>(items: &[T]) -> String {
    match items {
        [] => String::new(),
        [only] => only.as_ref().to_string(),
        [head @ .., last] => format!(
            "{} and {}",
            head.iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(", "),
            last.as_ref()
        ),
    }
}

/// `4` → `4th`, for the dossier's "the 4th richest cradle" line.
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

// ─── THE PROSE POOLS ─────────────────────────────────────────────────────────

/// Monumental building traditions, biased by the country a realm grew up in.
const ARCHITECTURE_POOL: [&str; 8] = [
    "Mud-brick temple platforms",
    "Megalithic stone circles",
    "Corbelled dry-stone tombs",
    "Timber longhouses on stilts",
    "Plastered pit-houses packed in terraces",
    "Terraced stone cities cut into hillsides",
    "Stuccoed plaza pyramids",
    "Rammed-earth walls and gateways",
];

/// Arts a realm can be known for; two or three are drawn per civilization.
const ARTS_POOL: [&str; 10] = [
    "geometric painted pottery",
    "jade and shell figurines",
    "mural painting",
    "bone flutes and skin drums",
    "dyed cloth and weaving",
    "cast metal masks",
    "inlaid shell and mother-of-pearl",
    "hymn cycles and recited epics",
    "tattooing and body paint",
    "calendar stones",
];

/// Observances a visitor would notice, drawn three at a time.
const FAITH_PRACTICES: [&str; 12] = [
    "seasonal flood festivals",
    "offerings of first fruits",
    "sky-watching for the calendar",
    "ancestor masks and funeral wealth",
    "blood offerings at the river ford",
    "sacred groves left uncut",
    "sea burials for the drowned",
    "the mummification of rulers",
    "a sacred marriage of king and river",
    "pilgrimage to the high oracle",
    "sacred beer and communal feasting",
    "star-named months",
];

// ─── DOSSIER BUILDERS ────────────────────────────────────────────────────────

/// Read a realm's whole material and social life off the country it was born in.
///
/// Geography decides the economy, the economy narrows the range of governments, and
/// the realm's own development decides whether it has a state at all — the oldest
/// argument in history writing. The draw (`rng`) only fills in what geography and
/// development leave open, such as which arts a people are known for. A realm that
/// descends from the head of its family echoes its ancestor's temple form and
/// rites, which is what makes siblings on a world resemble each other while
/// strangers do not.
fn build_culture(
    rng: &mut ChaCha8Rng,
    sk: &Skeleton,
    scheme: Scheme,
    inherited: Option<&Culture>,
    family: &str,
) -> Culture {
    let Scheme {
        subsistence,
        outlook,
    } = scheme;

    // Government: how this society organises power. Complexity decides where it
    // sits on the axis — a kinship band or a tribe low down, a chiefdom as it
    // rises, a state once it is high — and how it eats and what it values decide
    // what kind of state it is.
    let government = build_government(rng, subsistence, outlook, sk.scale, sk.complexity);

    let architecture = match inherited {
        Some(ancestor) if rng.random::<f32>() < 0.7 => ancestor.architecture,
        _ => {
            if subsistence == Subsistence::IrrigatedAgriculture
                && matches!(
                    government,
                    Government::DivineMonarchy | Government::TheocraticRepublic
                )
            {
                ARCHITECTURE_POOL[0]
            } else {
                *ARCHITECTURE_POOL.choose(rng).unwrap()
            }
        }
    };

    let art_count = rng.random_range(2..=3usize);
    let mut arts: Vec<&'static str> = Vec::with_capacity(art_count);
    while arts.len() < art_count {
        let art = *ARTS_POOL.choose(rng).unwrap();
        if !arts.contains(&art) {
            arts.push(art);
        }
    }

    let mut note = if sk.tier.is_state() {
        format!(
            "{} Its cradle: {} — {}.",
            subsistence.describe(),
            sk.cradle_kind.label().to_lowercase(),
            sk.cradle_kind.describe()
        )
    } else {
        format!(
            "{} Its country: {} — {}.",
            subsistence.describe(),
            sk.environment.label().to_lowercase(),
            sk.environment.describe()
        )
    };
    let standing = if government.is_state() {
        "a state with an apparatus behind it"
    } else {
        "a society with no state above its lineages"
    };
    note.push_str(&format!(
        " It is organised as {} — {}, and its public temper is {} — it {}.",
        government.label().to_lowercase(),
        standing,
        outlook.label().to_lowercase(),
        outlook.disposition()
    ));
    if inherited.is_some() {
        note.push_str(&format!(
            " It shares its ancestral stock with the rest of the {family}, whose tongue, rites and arts it still echoes."
        ));
    }

    Culture {
        subsistence,
        government,
        outlook,
        architecture,
        arts,
        note,
    }
}

/// What a realm's country lets it eat.
///
/// Geography decides the economy: a canal state is not born in a rainforest, and
/// a herd people does not grow up on a drowned delta. The seeded draw only settles
/// the borderline countries — a lake that could go either way, a steppe that could
/// plough or herd.
fn choose_subsistence(rng: &mut ChaCha8Rng, sk: &Skeleton) -> Subsistence {
    match sk.cradle_kind {
        CradleKind::RiverFloodplain
        | CradleKind::RiverDelta
        | CradleKind::DesertOasis => Subsistence::IrrigatedAgriculture,
        CradleKind::Lacustrine => {
            if rng.random::<f32>() < 0.5 {
                Subsistence::FishingAndSeafaring
            } else {
                Subsistence::IrrigatedAgriculture
            }
        }
        CradleKind::CoastalMaritime | CradleKind::IslandShore => Subsistence::FishingAndSeafaring,
        CradleKind::HighlandValley => Subsistence::TerraceFarming,
        CradleKind::SteppeGrassland => {
            if rng.random::<f32>() < 0.7 {
                Subsistence::Pastoralism
            } else {
                Subsistence::DrylandFarming
            }
        }
        CradleKind::SavannaHeartland => {
            if rng.random::<f32>() < 0.6 {
                Subsistence::DrylandFarming
            } else {
                Subsistence::Pastoralism
            }
        }
        CradleKind::WoodlandHeartland => {
            if rng.random::<f32>() < 0.75 {
                Subsistence::DrylandFarming
            } else {
                Subsistence::Pastoralism
            }
        }
        CradleKind::RainforestClearing => Subsistence::ShiftingCultivation,
    }
}

/// What a realm values — its public temper, and the lens through which it treats
/// its neighbours.
///
/// Weighted by how the realm eats, because a boat people and a herd people do not
/// value the same things.
fn choose_outlook(rng: &mut ChaCha8Rng, subsistence: Subsistence) -> Outlook {
    let pool: &[Outlook] = match subsistence {
        Subsistence::IrrigatedAgriculture => &[
            Outlook::Agrarian,
            Outlook::Ceremonial,
            Outlook::Ceremonial,
            Outlook::Scholarly,
            Outlook::Agrarian,
            Outlook::Artistic,
        ],
        Subsistence::DrylandFarming => &[
            Outlook::Agrarian,
            Outlook::Agrarian,
            Outlook::Isolationist,
            Outlook::Ceremonial,
            Outlook::Artistic,
            Outlook::Scholarly,
        ],
        Subsistence::Pastoralism => &[
            Outlook::Martial,
            Outlook::Martial,
            Outlook::Mercantile,
            Outlook::Isolationist,
            Outlook::Ceremonial,
        ],
        Subsistence::FishingAndSeafaring => &[
            Outlook::Mercantile,
            Outlook::Mercantile,
            Outlook::Artistic,
            Outlook::Scholarly,
            Outlook::Martial,
        ],
        Subsistence::ShiftingCultivation => &[
            Outlook::Isolationist,
            Outlook::Artistic,
            Outlook::Ceremonial,
            Outlook::Agrarian,
        ],
        Subsistence::TerraceFarming => &[
            Outlook::Isolationist,
            Outlook::Agrarian,
            Outlook::Martial,
            Outlook::Ceremonial,
            Outlook::Scholarly,
        ],
        // A country read rather than worked breeds patience and self-reliance: the
        // band must know its ground better than any neighbour ever will.
        Subsistence::Foraging => &[
            Outlook::Isolationist,
            Outlook::Isolationist,
            Outlook::Artistic,
            Outlook::Ceremonial,
            Outlook::Agrarian,
            Outlook::Martial,
        ],
    };
    *pool.choose(rng).unwrap()
}

/// Choose how a realm is governed.
///
/// Complexity decides where the realm sits on the single government axis — a
/// kinship band or a tribe has no state above it, a chiefdom ranks its lineages,
/// and a state holds power through an apparatus. Above the state threshold the
/// *kind* of state follows from how the realm eats and what it values: a river
/// people under priests or a god-king, a boat people under its merchants, a herd
/// people under a strongman, a temperate plough country under its landed elite.
fn build_government(
    rng: &mut ChaCha8Rng,
    subsistence: Subsistence,
    outlook: Outlook,
    scale: ScaleClass,
    complexity: f32,
) -> Government {
    if complexity < 0.06 {
        return Government::Band;
    }
    if complexity < 0.16 {
        return Government::Tribe;
    }
    if complexity < 0.30 {
        return Government::Chiefdom;
    }

    // Above the state threshold the *kind* of state is a weighted choice: the
    // economy fixes the candidates, and ethos and scope tilt the balance between
    // them. Scope is a bias only — never a label.
    let mut weights: Vec<(Government, f32)> = state_options(subsistence)
        .iter()
        .map(|&government| (government, 1.0f32))
        .collect();
    for (government, weight) in weights.iter_mut() {
        // A broad, many-province realm leans on a single throne or a narrow
        // clique; a compact one leans on councils and assemblies.
        *weight *= match scale {
            ScaleClass::Large => match government {
                Government::DivineMonarchy | Government::Tyranny | Government::Oligarchy => 1.9,
                _ => 0.7,
            },
            ScaleClass::Medium => 1.0,
            ScaleClass::Small => match government {
                Government::TheocraticRepublic
                | Government::AristocraticRepublic
                | Government::PlutocraticRepublic => 1.7,
                _ => 0.7,
            },
        };
        // Public temper pulls the same way the economy does, but less hard.
        *weight *= match outlook {
            Outlook::Ceremonial => match government {
                Government::DivineMonarchy | Government::TheocraticRepublic => 1.8,
                _ => 0.8,
            },
            Outlook::Scholarly => match government {
                Government::TheocraticRepublic | Government::AristocraticRepublic => 1.5,
                _ => 0.9,
            },
            Outlook::Mercantile => match government {
                Government::PlutocraticRepublic => 2.0,
                _ => 0.85,
            },
            Outlook::Martial => match government {
                Government::Tyranny | Government::Oligarchy => 1.8,
                _ => 0.8,
            },
            Outlook::Agrarian => match government {
                Government::AristocraticRepublic | Government::DivineMonarchy => 1.4,
                _ => 0.95,
            },
            Outlook::Artistic => match government {
                Government::AristocraticRepublic | Government::PlutocraticRepublic => 1.3,
                _ => 0.95,
            },
            Outlook::Isolationist => match government {
                Government::Oligarchy | Government::TheocraticRepublic => 1.5,
                _ => 0.85,
            },
        };
    }
    weighted_government(rng, &weights)
}

/// The state regimes a given economy can plausibly grow into.
fn state_options(subsistence: Subsistence) -> &'static [Government] {
    match subsistence {
        Subsistence::IrrigatedAgriculture => {
            &[Government::DivineMonarchy, Government::TheocraticRepublic]
        }
        Subsistence::FishingAndSeafaring => &[
            Government::PlutocraticRepublic,
            Government::AristocraticRepublic,
        ],
        Subsistence::Pastoralism => &[Government::Tyranny, Government::Oligarchy],
        Subsistence::DrylandFarming | Subsistence::TerraceFarming => &[
            Government::AristocraticRepublic,
            Government::Oligarchy,
            Government::DivineMonarchy,
        ],
        Subsistence::ShiftingCultivation => {
            &[Government::TheocraticRepublic, Government::Oligarchy]
        }
        // A foraging people never builds a state: no field is taxed, so there is
        // nothing for an apparatus to live on. The arm exists only so the match is
        // total — a band is decided by its development long before this point.
        Subsistence::Foraging => &[Government::Tribe, Government::Chiefdom],
    }
}

/// Pick one government from a weighted list, deterministically.
fn weighted_government(rng: &mut ChaCha8Rng, options: &[(Government, f32)]) -> Government {
    let total: f32 = options.iter().map(|(_, weight)| *weight).sum();
    let mut roll = rng.random::<f32>() * total;
    for &(government, weight) in options {
        if roll < weight {
            return government;
        }
        roll -= weight;
    }
    options
        .last()
        .map_or(Government::Oligarchy, |(government, _)| *government)
}

/// Read a realm's faith off the same country its economy came from.
///
/// The creed follows the water: river peoples worship the flood, islanders the
/// tide, highlanders the storm and the stars, forest peoples the earth. A realm
/// that descends from the head of its family keeps its ancestor's creed more
/// often than not — the second thing a shared stock transmits after the tongue.
fn build_faith(rng: &mut ChaCha8Rng, sk: &Skeleton, inherited: Option<&Faith>) -> Faith {
    let kind = match inherited {
        Some(ancestor) if rng.random::<f32>() < 0.6 => ancestor.kind,
        _ => match sk.cradle_kind {
            CradleKind::RiverFloodplain | CradleKind::RiverDelta | CradleKind::DesertOasis => {
                if rng.random::<f32>() < 0.75 {
                    FaithKind::RiverFertility
                } else {
                    FaithKind::Solar
                }
            }
            CradleKind::Lacustrine => {
                if rng.random::<f32>() < 0.6 {
                    FaithKind::RiverFertility
                } else {
                    FaithKind::EarthMother
                }
            }
            CradleKind::CoastalMaritime | CradleKind::IslandShore => {
                if rng.random::<f32>() < 0.75 {
                    FaithKind::SeaAndTide
                } else {
                    FaithKind::Solar
                }
            }
            CradleKind::HighlandValley => {
                let pool = [
                    FaithKind::SkyAndStorm,
                    FaithKind::StarAndOmen,
                    FaithKind::Ancestral,
                ];
                *pool.choose(rng).unwrap()
            }
            CradleKind::SteppeGrassland => {
                let pool = [
                    FaithKind::SkyAndStorm,
                    FaithKind::StarAndOmen,
                    FaithKind::Solar,
                ];
                *pool.choose(rng).unwrap()
            }
            CradleKind::SavannaHeartland => {
                let pool = [
                    FaithKind::Solar,
                    FaithKind::Solar,
                    FaithKind::EarthMother,
                ];
                *pool.choose(rng).unwrap()
            }
            CradleKind::WoodlandHeartland => {
                let pool = [
                    FaithKind::EarthMother,
                    FaithKind::SkyAndStorm,
                    FaithKind::Ancestral,
                ];
                *pool.choose(rng).unwrap()
            }
            CradleKind::RainforestClearing => {
                let pool = [
                    FaithKind::EarthMother,
                    FaithKind::EarthMother,
                    FaithKind::Ancestral,
                ];
                *pool.choose(rng).unwrap()
            }
        },
    };

    let clergy_pool: &[&'static str] = match kind {
        FaithKind::RiverFertility => &[
            "a temple priesthood that measures the flood",
            "the keepers of the river shrine",
            "a college of diviners who read the silt",
        ],
        FaithKind::Solar => &[
            "a priesthood of the sun calendar",
            "the sky-watchers of the temple roof",
        ],
        FaithKind::SkyAndStorm => &[
            "an oracle college in the high passes",
            "storm-callers who read the clouds",
        ],
        FaithKind::Ancestral => &[
            "the clan elders, who are their own priests",
            "masked lineage-keepers",
        ],
        FaithKind::SeaAndTide => &[
            "shrine-keepers at the landing place",
            "tide-readers who bless each hull",
        ],
        FaithKind::EarthMother => &[
            "the harvest priestesses",
            "village shrine-keepers of the mother",
        ],
        FaithKind::StarAndOmen => &[
            "astronomer-priests on a raised platform",
            "omen-readers who chart the wandering lights",
        ],
    };
    let clergy = *clergy_pool.choose(rng).unwrap();

    // Observances: the common stock of early religion, weighted towards the three
    // that this particular creed would care about most.
    let mut pool: Vec<&'static str> = FAITH_PRACTICES.to_vec();
    let favoured: &[&'static str] = match kind {
        FaithKind::RiverFertility => &[
            "seasonal flood festivals",
            "a sacred marriage of king and river",
            "sacred groves left uncut",
        ],
        FaithKind::Solar => &[
            "star-named months",
            "sky-watching for the calendar",
            "offerings of first fruits",
        ],
        FaithKind::SkyAndStorm => &[
            "pilgrimage to the high oracle",
            "sky-watching for the calendar",
            "offerings of first fruits",
        ],
        FaithKind::Ancestral => &[
            "ancestor masks and funeral wealth",
            "the mummification of rulers",
            "sacred beer and communal feasting",
        ],
        FaithKind::SeaAndTide => &[
            "sea burials for the drowned",
            "blood offerings at the river ford",
            "sacred beer and communal feasting",
        ],
        FaithKind::EarthMother => &[
            "offerings of first fruits",
            "sacred groves left uncut",
            "sacred beer and communal feasting",
        ],
        FaithKind::StarAndOmen => &[
            "star-named months",
            "sky-watching for the calendar",
            "ancestor masks and funeral wealth",
        ],
    };
    pool.extend(favoured.iter().copied());

    let mut practices: Vec<&'static str> = Vec::with_capacity(3);
    while practices.len() < 3 {
        let practice = *pool.choose(rng).unwrap();
        if !practices.contains(&practice) {
            practices.push(practice);
        }
    }

    let note = format!(
        "{} Its rites are kept by {}; a visitor would notice {}.",
        kind.creed(),
        clergy,
        join_list(&practices)
    );

    Faith {
        kind,
        clergy,
        practices,
        note,
    }
}

/// Write the dossier's "why here" answer: the measurements behind the cradle.
///
/// Every bullet is a number the planet actually produced, so a reader can audit
/// the choice instead of taking it on faith — which is the whole point of putting
/// the reasoning in the tooltip rather than a placename.
fn build_rise(
    sk: &Skeleton,
    hierarchy: &GeopoliticalHierarchy,
    richest_rank: usize,
    cradle_count: usize,
    nearest_km: f32,
    seat_latitude_deg: f32,
    min_river_flow: f32,
    great_river_flow: f32,
) -> RiseProfile {
    let s = &sk.seat_stats;
    let node = &hierarchy.provinces[sk.cradle.prov as usize];
    let kind = sk.cradle_kind;
    let mut reasons: Vec<String> = Vec::new();

    // A people are not a cradle story. Nobody chose their site: they were already
    // there, and what the atlas owes the reader is what their country gives them.
    if !sk.tier.is_state() {
        return build_people_rise(sk, hierarchy, seat_latitude_deg);
    }

    if richest_rank == 0 {
        reasons.push(format!(
            "Habitability {:.2} — not a leading cradle in its own right: it is ground nobody had taken yet.",
            s.hab_mean()
        ));
    } else {
        reasons.push(format!(
            "Habitability {:.2} — the {} richest of {} candidate cradles on this planet.",
            s.hab_mean(),
            ordinal(richest_rank),
            thousands_u64(cradle_count as u64),
        ));
    }
    if s.flow_max >= great_river_flow {
        reasons.push(format!(
            "A great river crosses it: peak discharge {} m³/s, several times the {} m³/s that already makes a river worth navigating and worth taxing.",
            thousands(s.flow_max),
            thousands(min_river_flow)
        ));
    } else if s.flow_max >= min_river_flow {
        reasons.push(format!(
            "A navigable river runs through it ({:.0} m³/s peak): water for the fields and a road for the boats.",
            s.flow_max
        ));
    } else {
        reasons.push(format!(
            "No river worth the name — the fields live on {:.0} mm of rain a year, which is enough here.",
            s.precip_mean() * 1200.0
        ));
    }
    if s.is_coastal() {
        reasons.push(format!(
            "{} of its pixels touch the open sea: fish, salt, and a harbour nobody had to dig.",
            thousands_u64(s.sea_px as u64)
        ));
    }
    if s.lake_px > 0 {
        reasons.push(format!(
            "{} pixels of lake shore give fish, reeds and fresh water all year — and a road that carries a canoe.",
            thousands_u64(s.lake_px as u64)
        ));
    }
    reasons.push(format!(
        "Mean {:.1} °C and {:.0} mm/yr of rain: grain country, neither desert nor ice.",
        s.temp_mean(),
        s.precip_mean() * 1200.0
    ));
    if s.elev_mean_m() >= 900.0 {
        reasons.push(format!(
            "High ground — {:.0} m mean elevation, {:.0} m at the highest point — terraced above the flood and hard to attack.",
            s.elev_mean_m(),
            s.elev_max_m
        ));
    } else {
        reasons.push(format!(
            "Gentle ground — {:.0} m mean elevation, {:.0} m at the highest point: ploughable plain rather than crag.",
            s.elev_mean_m(),
            s.elev_max_m
        ));
    }
    if nearest_km.is_finite() {
        reasons.push(format!(
            "Nearest other cradle: about {} km away.",
            thousands(nearest_km)
        ));
    }

    // The same story in a handful of words, for the atlas hover card: no
    // figures, no province ids, just the things a traveller would have noticed.
    let mut highlights: Vec<String> = Vec::new();
    if s.flow_max >= great_river_flow {
        highlights.push("A great river at its door".to_string());
    } else if s.flow_max >= min_river_flow {
        highlights.push("A navigable river through its fields".to_string());
    } else {
        highlights.push("Rain-fed fields, no river needed".to_string());
    }
    if s.is_coastal() {
        highlights.push("A shore for fish and salt".to_string());
    }
    if s.lake_px > 0 {
        highlights.push("A lake for fish, reeds and fresh water".to_string());
    }
    if s.elev_mean_m() >= 900.0 {
        highlights.push("High, defensible ground".to_string());
    }
    if richest_rank == 0 {
        highlights.push("Ground nobody had taken".to_string());
    }

    RiseProfile {
        kind,
        headline: format!("{} — {}.", kind.label(), kind.describe()),
        reasons,
        highlights,
        seat: format!(
            "Seat province #{} · area #{} · region #{} · superregion #{} · continent #{} · {:.1}° {}",
            sk.cradle.prov,
            node.area_id,
            node.region_id,
            node.superregion_id,
            node.continent_id,
            seat_latitude_deg.abs(),
            if seat_latitude_deg >= 0.0 { "N" } else { "S" },
        ),
    }
}

/// Why a *people* live where they live.
///
/// Not a cradle story at all: nobody founded them and nobody chose their site. What
/// the atlas owes the reader instead is their country — what it gives, what it
/// refuses, and why nobody ever turned it into fields.
fn build_people_rise(
    sk: &Skeleton,
    hierarchy: &GeopoliticalHierarchy,
    seat_latitude_deg: f32,
) -> RiseProfile {
    let s = &sk.seat_stats;
    let prov = sk.cradle.prov;
    let node = &hierarchy.provinces[prov as usize];
    let mut reasons: Vec<String> = vec![format!(
        "{} — {}.",
        sk.environment.label(),
        sk.environment.describe()
    )];
    reasons.push(format!(
        "Habitability {:.2}: {}.",
        s.hab_mean(),
        if sk.tier == SocietyTier::HunterGatherers {
            "too poor to plough, which is why nobody ever did"
        } else {
            "enough to feed the people who live here, and no more"
        }
    ));
    if s.flow_max >= 1.0 {
        reasons.push(format!(
            "Its rivers peak at {} m³/s: fishing water, drinking water, and the road a people use where there is no track.",
            thousands(s.flow_max)
        ));
    }
    if s.is_coastal() || s.lake_px > 0 {
        reasons.push(format!(
            "{} pixels of shore — the water is the richest ground these people have.",
            thousands_u64((s.sea_px + s.lake_px) as u64)
        ));
    }
    reasons.push(format!(
        "Mean {:.1} °C and {:.0} mm/yr of rain, over {:.0} m of mean elevation.",
        s.temp_mean(),
        s.precip_mean() * 1200.0,
        s.elev_mean_m()
    ));
    reasons.push(format!(
        "It sits at {:.1}° {}, which fixes the season the whole year turns on.",
        seat_latitude_deg.abs(),
        if seat_latitude_deg >= 0.0 { "N" } else { "S" }
    ));

    let mut highlights: Vec<String> = vec![sk.environment.label().to_string()];
    if s.is_coastal() || s.lake_px > 0 {
        highlights.push("Water for fish and for the road".to_string());
    }
    if s.flow_max >= 1.0 {
        highlights.push("A river through the country".to_string());
    } else {
        highlights.push("Dry country: the water is underground".to_string());
    }
    highlights.push(format!(
        "{} provinces of it",
        thousands_u64(sk.provinces.len() as u64)
    ));

    RiseProfile {
        kind: sk.cradle_kind,
        headline: format!("A country of {}", sk.tier.label().to_lowercase()),
        reasons,
        highlights,
        seat: format!(
            "Heartland province #{} · area #{} · region #{} · superregion #{} · continent #{} · {:.1}° {}",
            prov,
            node.area_id,
            node.region_id,
            node.superregion_id,
            node.continent_id,
            seat_latitude_deg.abs(),
            if seat_latitude_deg >= 0.0 { "N" } else { "S" },
        ),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const ECONOMIES: [Subsistence; 6] = [
        Subsistence::IrrigatedAgriculture,
        Subsistence::DrylandFarming,
        Subsistence::Pastoralism,
        Subsistence::FishingAndSeafaring,
        Subsistence::ShiftingCultivation,
        Subsistence::TerraceFarming,
    ];

    /// A realm above the state threshold is governed by a state, whatever its
    /// economy, temper or shape.
    #[test]
    fn a_developed_realm_is_always_a_state() {
        for subsistence in ECONOMIES {
            for outlook in [
                Outlook::Agrarian,
                Outlook::Mercantile,
                Outlook::Martial,
                Outlook::Scholarly,
                Outlook::Ceremonial,
                Outlook::Artistic,
                Outlook::Isolationist,
            ] {
                for scale in [ScaleClass::Small, ScaleClass::Medium, ScaleClass::Large] {
                    let mut rng = ChaCha8Rng::seed_from_u64(7);
                    for _ in 0..8 {
                        let government =
                            build_government(&mut rng, subsistence, outlook, scale, 0.55);
                        assert!(
                            government.is_state(),
                            "{subsistence:?} / {outlook:?} / {scale:?} produced {government:?}"
                        );
                    }
                }
            }
        }
    }

    /// A realm below the thresholds has no state above its lineages.
    #[test]
    fn an_undeveloped_realm_has_no_state() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        assert_eq!(
            build_government(
                &mut rng,
                Subsistence::IrrigatedAgriculture,
                Outlook::Agrarian,
                ScaleClass::Large,
                0.02,
            ),
            Government::Band
        );
        assert_eq!(
            build_government(
                &mut rng,
                Subsistence::IrrigatedAgriculture,
                Outlook::Agrarian,
                ScaleClass::Large,
                0.20,
            ),
            Government::Chiefdom
        );
    }

    /// The tier carries the hue and the society carries the shade: the first two
    /// societies of a tier wear its base colour, no two consecutive societies of one
    /// tier repeat a shade, and every shade stays inside its tier's hue family — so a
    /// farming people never looks like a state.
    #[test]
    fn the_tier_carries_the_hue_and_the_society_the_shade() {
        for tier in SocietyTier::LADDER {
            let base = tier.legend_color();
            assert_eq!(tier_shade(tier, 0), base);
            assert_eq!(tier_shade(tier, 1), base);
            let mut previous = tier_shade(tier, 1);
            for index in 2..48 {
                let shade = tier_shade(tier, index);
                assert_ne!(
                    shade,
                    previous,
                    "{} repeated a shade at {index}",
                    tier.label()
                );
                previous = shade;
            }
            let lead = (0..3).max_by_key(|&channel| base[channel]).unwrap();
            for index in 0..48 {
                let shade = tier_shade(tier, index);
                assert_eq!(
                    (0..3).max_by_key(|&channel| shade[channel]).unwrap(),
                    lead,
                    "{} left its hue family at {index}",
                    tier.label()
                );
            }
        }
    }

    /// Every tier's development sits in the band of the government axis its own
    /// institutions belong to: a band of foragers is never a state, and a ranked
    /// chiefdom never grows an apparatus.
    #[test]
    fn a_tier_never_claims_the_government_of_a_higher_one() {
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        for _ in 0..200 {
            let band_level = tier_complexity(SocietyTier::HunterGatherers, &mut rng);
            let band = build_government(
                &mut rng,
                Subsistence::Foraging,
                Outlook::Isolationist,
                ScaleClass::Small,
                band_level,
            );
            assert_eq!(band, Government::Band);

            let village_level = tier_complexity(SocietyTier::SimpleFarmers, &mut rng);
            let village = build_government(
                &mut rng,
                Subsistence::DrylandFarming,
                Outlook::Agrarian,
                ScaleClass::Small,
                village_level,
            );
            assert!(
                matches!(village, Government::Tribe | Government::Chiefdom),
                "simple farmers became {village:?}"
            );

            let chiefdom_level = tier_complexity(SocietyTier::ComplexFarmers, &mut rng);
            let chiefdom = build_government(
                &mut rng,
                Subsistence::DrylandFarming,
                Outlook::Agrarian,
                ScaleClass::Small,
                chiefdom_level,
            );
            assert!(
                !chiefdom.is_state(),
                "a chiefdom people became a state: {chiefdom:?}"
            );
        }
    }

    /// The same seed draws the same government — the stage stays a pure function.
    #[test]
    fn government_is_deterministic() {
        let draw = |seed: u64| {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            build_government(
                &mut rng,
                Subsistence::FishingAndSeafaring,
                Outlook::Mercantile,
                ScaleClass::Small,
                0.8,
            )
        };
        assert_eq!(draw(99), draw(99));
    }

    /// Contemporaries, not a relay: however many cradles a world carries, every
    /// founding year after the first stays inside the window, the set is handed out
    /// in founding order, and the same seed draws the same years.
    #[test]
    fn founding_years_are_bounded_sorted_and_deterministic() {
        let draw = |seed: u64, nearest: Vec<f32>| {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            founding_offsets(&nearest, 1_500, 620.0, &mut rng)
        };
        // A hundred cradles — far more than any real world — must not stretch the era.
        let offsets = draw(7, vec![900.0; 99]);
        assert_eq!(offsets, draw(7, vec![900.0; 99]));
        assert_eq!(offsets.len(), 99);
        assert!(offsets.iter().all(|&y| (0..=1_500).contains(&y)));
        assert!(offsets.windows(2).all(|pair| pair[0] <= pair[1]));
        // A real spread of contemporaries, not a single instant.
        assert!(offsets.iter().copied().max().unwrap() > 1_500 / 4);
        // A world with one city founds nothing later.
        assert!(draw(7, Vec::new()).is_empty());
    }
}
