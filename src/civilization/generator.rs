//! Where civilizations rise, how far they spread, and what they turn into.
//!
//! The stage runs in six passes, each of which is a single loop over a different
//! domain — which is what keeps it order-stable and reproducible for a seed:
//!
//! 1. **Census** — every province is measured once: its real area in km² (latitude
//!    aware), mean habitability, peak river discharge, mean climate, relief, how
//!    much of it touches sea or lake, and its carrying capacity in people.
//! 2. **Cradle selection** — the richest handful of provinces in the world become
//!    candidate cradles, and a continent-aware quota picks a well-separated few of
//!    them, so the world never fills up with civilizations.
//! 3. **Expansion** — each cradle floods outward along fertile, cheap ground until
//!    it has taken its share, is walled in by wilderness, or runs into an older
//!    realm. Older realms pick first, exactly as history did.
//! 4. **Dossier** — the cradle's own geography decides its economy, social order,
//!    crafts, script and faith.
//! 5. **Contact** — territories that touch, or that lie within a sea-crossing of
//!    each other, are linked; the rest rose alone.
//! 6. **Inheritance** — realms close enough to share an ancestral stock get one
//!    tongue family, and a younger realm in contact with an older one may have
//!    taken up its crops, rites or script *before* founding a city of its own.

use crate::civilization::config::CivilizationConfig;
use crate::civilization::epoch::{
    DevelopmentPace, Epoch, EpochCandidate, Timeline, place_name, select_epoch_site,
};
use crate::civilization::{
    CIVILIZATION_COLORS, Civilization, CivilizationWorld, Contact, ContactKind, CraftTier,
    CradleKind, Culture, Era, Faith, FaithKind, Influence, Isolation, Kinship, Literacy, Outlook,
    RiseProfile, ScaleClass, Subsistence,
};
use crate::climate::PlanetaryMode;
use crate::core::fmt::{thousands, thousands_u64};
use crate::core::sphere;
use crate::core::{relief, Grid};
use crate::geopolitics::GeopoliticalHierarchy;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::{BTreeMap, BinaryHeap};

/// Salt mixed into the world seed for the history stage, so the dawn draws from
/// an independent random stream instead of replaying the province generator's.
const DAWN_SALT: u64 = 0x0DA7_0F1F_0000_0007;

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
    // climate, gentle ground. Score every candidate, then take a well-spread
    // handful — and only a handful, so the world stays a wilderness with a few
    // bright corners rather than a patchwork of states.
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

    // Which continents can host a cradle, and how much land each one holds. The
    // quota below deliberately gives every continent with good ground one cradle
    // before any continent gets a second: that is what leaves a few civilizations
    // out of reach of each other on any world with more than one landmass.
    let mut continent_land: BTreeMap<u32, f64> = BTreeMap::new();
    for s in stats.iter().filter(|s| s.present) {
        *continent_land.entry(s.continent_id).or_insert(0.0) += s.px as f64;
    }
    let mut continent_best: BTreeMap<u32, f32> = BTreeMap::new();
    for c in &cradles {
        let entry = continent_best.entry(c.continent).or_insert(f32::MIN);
        if c.score > *entry {
            *entry = c.score;
        }
    }

    let wanted = rng.random_range(config.min_civilizations..=config.max_civilizations);

    let mut ranked_continents: Vec<u32> = continent_best.keys().copied().collect();
    ranked_continents.sort_by(|a, b| {
        continent_best[b]
            .partial_cmp(&continent_best[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(b))
    });

    let mut quotas: BTreeMap<u32, usize> = BTreeMap::new();
    // How many landmasses actually get a civilization of their own. Deliberately
    // only about three fifths of the realms: on a world of many continents that
    // leaves the rest to rise as *second* cradles on the richest continents, which
    // is what puts neighbours, rivals and cultural cousins on the map instead of
    // one lonely realm per landmass.
    let continent_slots = ((wanted as f32 * 0.6).ceil() as usize)
        .clamp(1, ranked_continents.len());

    if wanted <= continent_slots {
        for c in ranked_continents.iter().take(wanted) {
            quotas.insert(*c, 1);
        }
    } else {
        for c in ranked_continents.iter().take(continent_slots) {
            quotas.insert(*c, 1);
        }
        let mut by_land = ranked_continents.clone();
        by_land.sort_by(|a, b| {
            continent_land[b]
                .partial_cmp(&continent_land[a])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(b))
        });
        let mut left = wanted - continent_slots;
        let mut turn = 0usize;
        while left > 0 {
            *quotas.get_mut(&by_land[turn % by_land.len()]).unwrap() += 1;
            left -= 1;
            turn += 1;
        }
    }

    let distance = |a: &Cradle, b: &Cradle| sphere::wrap_distance_px(a.cx, a.cy, b.cx, b.cy, w);

    let mut picked: Vec<Cradle> = Vec::new();
    let mut separation = config.cradle_separation_frac * w as f32;
    for _round in 0..3 {
        for (&continent, &quota) in &quotas {
            loop {
                if picked.iter().filter(|p| p.continent == continent).count() >= quota {
                    break;
                }
                let next = cradles.iter().find(|cand| {
                    cand.continent == continent
                        && !picked.iter().any(|p| p.prov == cand.prov)
                        && picked.iter().all(|p| distance(p, cand) >= separation)
                });
                match next {
                    Some(cand) => picked.push(*cand),
                    None => break, // this continent has no room left at this spacing
                }
            }
        }
        if picked.len() >= wanted {
            break;
        }
        // The world could not fit them this far apart: let the cradles crowd, and
        // say so, rather than place fewer than the world was asked for.
        separation *= 0.6;
    }

    picked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.prov.cmp(&b.prov))
    });

    // Every world should show both halves of the story: realms that grew up in
    // another realm's shadow, and realms that had to invent everything alone. If
    // the planet only offered one of the two, the weakest cradle is swapped for a
    // candidate that restores the other — if such a candidate exists at all.
    if picked.len() >= 3 {
        let mut all_crowded = true;
        let mut all_lonely = true;
        for i in 0..picked.len() {
            for j in (i + 1)..picked.len() {
                let d = distance(&picked[i], &picked[j]);
                if d >= config.contact_distance_px {
                    all_crowded = false;
                }
                if d <= config.isolated_distance_px {
                    all_lonely = false;
                }
            }
        }

        if all_crowded || all_lonely {
            let weak = picked.pop().unwrap();
            let want_lonely = all_crowded;
            let replacement = cradles.iter().find(|cand| {
                if picked.iter().any(|p| p.prov == cand.prov) {
                    return false;
                }
                if want_lonely {
                    picked
                        .iter()
                        .all(|p| distance(p, cand) >= config.isolated_distance_px)
                } else {
                    picked
                        .iter()
                        .any(|p| distance(p, cand) <= config.contact_distance_px)
                }
            });
            picked.push(replacement.copied().unwrap_or(weak));
            picked.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.prov.cmp(&b.prov))
            });
        }
    }

    if picked.len() < config.min_civilizations {
        println!(
            "⚠ Only {} cradles could be placed on this world (wanted {}).",
            picked.len(),
            wanted
        );
    }

    // ─── 3. FOUNDING DATES, AMBITIONS AND EXPANSION ──────────────────────────
    // Year zero of the internal frame is the founding of the first city, by the
    // first realm to rise. Every later cradle waits for the ground it wants, and
    // the further it is from anyone who already exists, the longer it waits:
    // crossing an ocean or an empty continent costs generations that a
    // neighbouring floodplain does not.
    //
    // How fast this world develops is read here, because the eras below are scaled
    // by it. What that pace implies for the depth of prehistory — how long the
    // peoples of this world lived before the first hearth — is drawn much later,
    // once the whole history is known and every earlier draw has been made.
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

    let mut founding_years: Vec<i32> = Vec::with_capacity(picked.len());
    for (rank, cradle) in picked.iter().enumerate() {
        if rank == 0 {
            founding_years.push(0);
            continue;
        }
        let nearest_earlier = picked[..rank]
            .iter()
            .map(|p| distance(p, cradle))
            .fold(f32::MAX, f32::min);
        let base = rng.random_range(60..=150) as f32;
        let spread = if nearest_earlier > config.isolated_distance_px {
            rng.random_range(280..=620) as f32
        } else if nearest_earlier > config.contact_distance_px {
            rng.random_range(120..=320) as f32
        } else {
            rng.random_range(0..=90) as f32
        };
        let year = founding_years[rank - 1] + (base + spread).round() as i32;
        founding_years.push(year);
    }

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

    // The land is claimed in founding order, so the oldest realm always gets first
    // pick of the floodplain and the youngest grows up in whatever valley is left.
    let mut owner: Vec<u16> = vec![0; n];
    let mut territories: Vec<Vec<u32>> = Vec::with_capacity(picked.len());
    let mut seats: Vec<u32> = Vec::with_capacity(picked.len());
    let mut displaced: Vec<Option<u32>> = Vec::with_capacity(picked.len());

    for (rank, cradle) in picked.iter().enumerate() {
        let civ_id = (rank + 1) as u16;
        let (lo, hi) = match ambitions[rank] {
            ScaleClass::Small => config.small_provinces,
            ScaleClass::Medium => config.medium_provinces,
            ScaleClass::Large => config.large_provinces,
        };
        let target = rng.random_range(lo..=hi);

        // An older realm may already hold this cradle's heartland — a rich valley
        // three hundred kilometres from an empire's capital is *inside* the
        // empire. The latecomer then does what latecomers did: it settles the
        // nearest ground still free, and its story says so.
        let mut seat = cradle.prov;
        let mut moved_from = None;
        if owner[seat as usize] != 0 {
            if let Some(free) = nearest_free_province(seat, &owner, hierarchy, &stats, w) {
                moved_from = Some(seat);
                seat = free;
            }
        }
        seats.push(seat);
        displaced.push(moved_from);

        territories.push(expand_cradle(
            seat,
            target,
            civ_id,
            &mut owner,
            hierarchy,
            &stats,
            w,
            config,
        ));
    }

    println!(
        "✓ Dawn cradles: {} civilizations on {} continents | {} provinces claimed",
        picked.len(),
        ranked_continents.len(),
        territories.iter().map(|t| t.len()).sum::<usize>(),
    );

    // ─── 4. SKELETONS AND ANCESTRAL STOCKS ───────────────────────────────────
    // Everything a realm is before its dossier is written: where it sits, how much
    // it holds, when it began and what kind of country it was born in.
    //
    // The map date is set a few centuries *after* the youngest realm began, so
    // that every civilization on the map has a history rather than only a founding
    // — otherwise the last cradle to rise would always stand at Year 0 of its own
    // calendar, with no cities and no scribes to its name.
    let last_founding = *founding_years.last().unwrap_or(&0);
    let map_year = last_founding + ((last_founding as f32 / 6.0) as i32).clamp(150, 500);
    let family_radius = config.family_radius_frac * w as f32;

    let mut skeletons: Vec<Skeleton> = Vec::with_capacity(picked.len());
    for rank in 0..picked.len() {
        let seat = seats[rank];
        let seat_node = &hierarchy.provinces[seat as usize];
        let seat_stats = stats[seat as usize];
        let provinces = territories[rank].clone();
        let age_years = (map_year - founding_years[rank]).max(0) as u32;
        skeletons.push(Skeleton {
            cradle: Cradle {
                prov: seat,
                // The score belongs to the cradle that was *chosen*; once a realm
                // has settled somewhere else, what matters is where it actually is.
                score: picked[rank].score,
                continent: seat_stats.continent_id,
                cx: seat_node.center_x,
                cy: seat_node.center_y,
            },
            cradle_kind: classify_cradle(
                &seat_stats,
                hierarchy,
                seat,
                min_river_flow,
                config.great_river_flow,
            ),
            area_km2: provinces.iter().map(|&p| stats[p as usize].area_km2).sum(),
            scale: ScaleClass::from_provinces(provinces.len()),
            era: Era::from_age_scaled(age_years, pace.multiplier()),
            age_years,
            founding_year_internal: founding_years[rank],
            provinces,
            seat_stats,
            displaced_from: displaced[rank],
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
            kin[m] = members
                .iter()
                .filter(|&&other| other != m)
                .map(|&other| other as u32 + 1)
                .collect();
        }
    }

    // ─── 5. DOSSIERS ─────────────────────────────────────────────────────────
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
            inherited_culture.as_ref(),
            family_name[rank].as_str(),
        );
        let faith = build_faith(&mut rng, &skeletons[rank], inherited_faith.as_ref());
        cultures.push(culture);
        faiths.push(faith);
    }

    // ─── 6. POPULATION ───────────────────────────────────────────────────────
    // A province's population is its own carrying capacity from the census, times
    // the realm's development: an empire with archives, canals and standing armies
    // feeds more people per hectare than a scatter of first villages. Every
    // civilized province therefore carries a population figure, and every realm
    // the sum of its provinces.
    let mut province_population: Vec<u64> = vec![0; n];
    let mut populations: Vec<u64> = Vec::with_capacity(skeletons.len());
    for rank in 0..skeletons.len() {
        let development = development_multiplier(
            skeletons[rank].era,
            cultures[rank].kinship,
            cultures[rank].outlook,
        );
        let mut total = 0u64;
        for &p in &skeletons[rank].provinces {
            let pop = (stats[p as usize].pop_seed * development as f64).round() as u64;
            province_population[p as usize] = pop;
            total += pop;
        }
        populations.push(total);
    }

    // ─── 7. CONTACT ──────────────────────────────────────────────────────────
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
                note: contact_note(kind, relation, distance_km, id_b),
            });
            contacts[b].push(Contact {
                other: id_a,
                kind,
                distance_km,
                relation,
                note: contact_note(kind, relation, distance_km, id_a),
            });
        }
    }

    // ─── 8. INHERITANCE ──────────────────────────────────────────────────────
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
                civilization_name(older_id),
                thousands_u64(gap.max(0) as u64),
                civilization_name(younger as u32 + 1),
                join_list(&borrowed),
            );
            influences[younger].push(Influence {
                other: older_id,
                borrowed,
                note,
            });
        }
    }

    // ─── 9. THE REASONS ──────────────────────────────────────────────────────
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

    // ─── 9b. THE EPOCH AND THE DEPTH OF PREHISTORY ───────────────────────────
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
        &skeletons,
        hierarchy,
        map_year,
        &mut rng,
    );
    let timeline = Timeline {
        prehistory_years,
        atlas_year: map_year,
        epoch,
    };
    // Every date the atlas shows is this one shift away from the internal frame.
    let epoch_offset = timeline.epoch.absolute_year;

    // ─── 10. ASSEMBLE ────────────────────────────────────────────────────────
    // Every date the atlas ever shows is written in the world's own era, so the
    // internal frame is converted exactly once, here: a realm's founding and the
    // map date both become years of the Common Era, negative for BCE. Nothing
    // above this point ever saw a CE year, and nothing below it sees anything else.
    let mut civilizations: Vec<Civilization> = Vec::with_capacity(skeletons.len());
    for rank in 0..skeletons.len() {
        let sk = &skeletons[rank];
        let isolation = if contacts[rank].is_empty() {
            Isolation::IsolatedCradle
        } else if contacts[rank]
            .iter()
            .any(|c| c.kind == ContactKind::LandFrontier)
        {
            Isolation::IntegratedHeartland
        } else {
            Isolation::NeighbouringCradle
        };

        let population = populations[rank];
        civilizations.push(Civilization {
            name: civilization_name(rank as u32 + 1),
            color: CIVILIZATION_COLORS[rank % CIVILIZATION_COLORS.len()],
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
            founding_year: sk.founding_year_internal - epoch_offset,
            age_years: sk.age_years,
            era: sk.era,
            isolation,
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
        province_population,
        total_population,
        civilized_provinces,
        civilized_area_km2,
    }
}
// ─── INTERNAL RECORDS ────────────────────────────────────────────────────────

/// A cradle that has taken root: everything about a realm that is known before
/// its dossier is written.
struct Skeleton {
    cradle: Cradle,
    cradle_kind: CradleKind,
    seat_stats: ProvinceStats,
    provinces: Vec<u32>,
    area_km2: f32,
    scale: ScaleClass,
    era: Era,
    age_years: u32,
    /// Founding year in the world's internal frame, whose year zero is the first
    /// city. The atlas never shows this; [`Civilization::founding_year`] carries the
    /// same date written in the world's own era.
    founding_year_internal: i32,
    /// The cradle province a latecomer was pushed off, if it ever was.
    displaced_from: Option<u32>,
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

/// One frontier step: a province reached at a certain accumulated cost.
#[derive(Copy, Clone, PartialEq)]
struct Step {
    cost: f32,
    prov: u32,
}

impl Eq for Step {}

impl Ord for Step {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| self.prov.cmp(&other.prov))
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

/// Decide the world's epoch: where the event happened, and when.
///
/// The year follows the place — a few centuries into the life of whoever held it,
/// so the place and the date are one story. A wilderness site, which is the more
/// usual case for a shrine, takes the oldest realm in the world as its anchor. The
/// result is always strictly inside the civilized era, so no realm is ever dated
/// from an event that has not happened yet.
#[allow(clippy::too_many_arguments)]
fn choose_epoch(
    site: Option<u32>,
    candidates: &[EpochCandidate],
    owner: &[u16],
    skeletons: &[Skeleton],
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

    let anchor_founding = site_realm
        .and_then(|id| skeletons.get(id as usize - 1).map(|sk| sk.founding_year_internal))
        .or_else(|| skeletons.iter().map(|sk| sk.founding_year_internal).min())
        .unwrap_or(0);
    // Strictly inside the civilized era: after the first hearth, before the atlas.
    let lo = 1;
    let hi = (atlas_year - 1).max(lo);
    let year = (anchor_founding + rng.random_range(300..=1_200)).clamp(lo, hi);

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

// ─── EXPANSION ───────────────────────────────────────────────────────────────

/// Flood a cradle's territory outward through the province graph.
///
/// The frontier expands cheapest-first (Dijkstra): a step costs what the terrain
/// between two provinces costs, discounted by how fertile the target is, and
/// multiplied hard by the length of any jump. That single rule produces the whole
/// historical geography at once — a realm creeps along a river valley, stalls on
/// a mountain wall, hops a strait to the next island, and stops dead at the
/// border of an older neighbour, because that land is already claimed.
fn expand_cradle(
    seat: u32,
    target: usize,
    civ_id: u16,
    owner: &mut [u16],
    hierarchy: &GeopoliticalHierarchy,
    stats: &[ProvinceStats],
    width: usize,
    config: &CivilizationConfig,
) -> Vec<u32> {
    let n = owner.len();
    let mut dist = vec![f32::MAX; n];
    let mut heap: BinaryHeap<Step> = BinaryHeap::new();
    let mut taken: Vec<u32> = Vec::with_capacity(target);

    dist[seat as usize] = 0.0;
    heap.push(Step { cost: 0.0, prov: seat });

    while let Some(Step { cost, prov }) = heap.pop() {
        if cost > dist[prov as usize] + 1e-3 || owner[prov as usize] != 0 {
            continue; // stale heap entry, or land an older realm already holds
        }
        owner[prov as usize] = civ_id;
        taken.push(prov);
        if taken.len() >= target {
            break;
        }

        let node = &hierarchy.provinces[prov as usize];
        for &(nb, edge) in &node.neighbors {
            let nb_idx = nb as usize;
            if nb_idx >= n || owner[nb_idx] != 0 {
                continue;
            }
            let other = &hierarchy.provinces[nb_idx];
            let hop = sphere::wrap_distance_px(
                node.center_x,
                node.center_y,
                other.center_x,
                other.center_y,
                width,
            );
            // Fertile ground is cheap ground: this is what bends a frontier along
            // a river valley instead of along a straight line.
            let fertility = 0.35 + stats[nb_idx].hab_mean().clamp(0.0, 1.2);
            let jump = if hop <= 90.0 { 1.0 } else { (hop / 90.0).powf(1.6) };
            let step = (edge as f32) * 0.35 / fertility * jump;
            if step > config.max_step_cost {
                continue; // a wall that history did not cross
            }
            let next = cost + step;
            if next < dist[nb_idx] {
                dist[nb_idx] = next;
                heap.push(Step { cost: next, prov: nb });
            }
        }
    }

    taken.sort_unstable();
    taken
}

/// The nearest province nobody holds yet — where a latecomer settles.
///
/// It walks outward through the province graph first, so the realm stays in the
/// country it was actually born in, and only then falls back to a plain
/// nearest-neighbour scan of the whole planet. Every candidate must be a real,
/// inhabited province, never one of the id slots the partition left empty.
fn nearest_free_province(
    from: u32,
    owner: &[u16],
    hierarchy: &GeopoliticalHierarchy,
    stats: &[ProvinceStats],
    width: usize,
) -> Option<u32> {
    let count = hierarchy.provinces.len();
    let mut visited = vec![false; count];
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    visited[from as usize] = true;
    queue.push_back(from);

    while let Some(prov) = queue.pop_front() {
        if owner[prov as usize] == 0 && stats[prov as usize].present {
            return Some(prov);
        }
        for &(nb, _) in &hierarchy.provinces[prov as usize].neighbors {
            let idx = nb as usize;
            if idx < count && !visited[idx] {
                visited[idx] = true;
                queue.push_back(nb);
            }
        }
    }

    let node = &hierarchy.provinces[from as usize];
    (1..count as u32)
        .filter(|&p| owner[p as usize] == 0 && stats[p as usize].present)
        .min_by(|&a, &b| {
            let da = sphere::wrap_distance_px(
                node.center_x,
                node.center_y,
                hierarchy.provinces[a as usize].center_x,
                hierarchy.provinces[a as usize].center_y,
                width,
            );
            let db = sphere::wrap_distance_px(
                node.center_x,
                node.center_y,
                hierarchy.provinces[b as usize].center_x,
                hierarchy.provinces[b as usize].center_y,
                width,
            );
            da.partial_cmp(&db)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        })
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

/// How much a realm's development multiplies its provinces' carrying capacity.
///
/// Age, social order and ethos all push the same number: an empire with archives
/// and canals feeds more people per hectare than a scatter of first villages, and
/// a martial or isolationist realm feeds marginally fewer than an agrarian one.
fn development_multiplier(era: Era, kinship: Kinship, outlook: Outlook) -> f32 {
    (0.60 + era.density_bias() + kinship.density_bias() + outlook.density_bias())
        .clamp(0.45, 1.45)
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
fn contact_note(kind: ContactKind, relation: &str, distance_km: f32, other: u32) -> String {
    let other = civilization_name(other);
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
        culture.craft.hallmark(),
        culture.literacy.hallmark(),
        faith.kind.hallmark(),
        culture.architecture,
    ]
}

/// `Mud-brick temple platforms` → `mud-brick temple platforms`, so a borrowed
/// hallmark reads inside a sentence.
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
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
/// Geography decides the economy, and the economy decides the social order — the
/// oldest argument in history writing. The draw (`rng`) only fills in what
/// geography leaves open, such as which arts a people are known for. A realm that
/// descends from the head of its family echoes its ancestor's craft, temple form
/// and rites, which is what makes siblings on a world resemble each other while
/// strangers do not.
fn build_culture(
    rng: &mut ChaCha8Rng,
    sk: &Skeleton,
    inherited: Option<&Culture>,
    family: &str,
) -> Culture {
    let subsistence = match sk.cradle_kind {
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
    };

    let kinship = match subsistence {
        Subsistence::IrrigatedAgriculture => {
            if rng.random::<f32>() < 0.55 {
                Kinship::TempleBureaucracy
            } else {
                Kinship::DivineMonarchy
            }
        }
        Subsistence::TerraceFarming => {
            if rng.random::<f32>() < 0.5 {
                Kinship::ClanChiefdoms
            } else {
                Kinship::TempleBureaucracy
            }
        }
        Subsistence::FishingAndSeafaring => {
            if rng.random::<f32>() < 0.6 {
                Kinship::MerchantOligarchy
            } else {
                Kinship::ClanChiefdoms
            }
        }
        Subsistence::Pastoralism => {
            if rng.random::<f32>() < 0.55 {
                Kinship::ClanChiefdoms
            } else {
                Kinship::WarriorAristocracy
            }
        }
        Subsistence::DrylandFarming => {
            if rng.random::<f32>() < 0.5 {
                Kinship::ClanChiefdoms
            } else {
                Kinship::Egalitarian
            }
        }
        Subsistence::ShiftingCultivation => {
            if rng.random::<f32>() < 0.6 {
                Kinship::Egalitarian
            } else {
                Kinship::ClanChiefdoms
            }
        }
    };

    // Ethos: weighted by how the realm eats, because a boat people and a herd
    // people do not value the same things.
    let outlook = {
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
        };
        *pool.choose(rng).unwrap()
    };

    // Metal is a matter of what the ground offers: a realm whose province climbs
    // into rock has ore within a week's walk and turns it up earlier.
    let ore_in_reach = sk.seat_stats.elev_max_m >= 1_200.0;
    let craft = match inherited {
        Some(ancestor) if rng.random::<f32>() < 0.6 => ancestor.craft,
        _ => match sk.era {
            Era::FirstSettlements => {
                if ore_in_reach && rng.random::<f32>() < 0.25 {
                    CraftTier::NativeCopper
                } else {
                    CraftTier::StoneAndBone
                }
            }
            Era::FirstCities => {
                if ore_in_reach && rng.random::<f32>() < 0.5 {
                    CraftTier::NativeCopper
                } else {
                    CraftTier::StoneAndBone
                }
            }
            Era::CityStates => {
                if ore_in_reach || rng.random::<f32>() < 0.5 {
                    CraftTier::CopperAlloys
                } else {
                    CraftTier::NativeCopper
                }
            }
            Era::Empires => {
                if ore_in_reach && rng.random::<f32>() < 0.6 {
                    CraftTier::SmeltedIron
                } else {
                    CraftTier::CopperAlloys
                }
            }
        },
    };

    let literacy = match sk.era {
        Era::FirstSettlements => Literacy::OralTradition,
        Era::FirstCities => {
            if rng.random::<f32>() < 0.55 {
                Literacy::GlyphTallies
            } else {
                Literacy::OralTradition
            }
        }
        Era::CityStates => {
            if rng.random::<f32>() < 0.7 {
                Literacy::LogographicScript
            } else {
                Literacy::GlyphTallies
            }
        }
        Era::Empires => {
            if rng.random::<f32>() < 0.35 {
                Literacy::AlphabeticScript
            } else {
                Literacy::LogographicScript
            }
        }
    };

    let architecture = match inherited {
        Some(ancestor) if rng.random::<f32>() < 0.7 => ancestor.architecture,
        _ => {
            if subsistence == Subsistence::IrrigatedAgriculture
                && kinship == Kinship::TempleBureaucracy
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

    let mut note = format!(
        "{} Its cradle: {} — {}.",
        subsistence.describe(),
        sk.cradle_kind.label().to_lowercase(),
        sk.cradle_kind.describe()
    );
    note.push_str(&format!(
        " It is organised as {}, and its public temper is {} — it {}.",
        kinship.label().to_lowercase(),
        outlook.label().to_lowercase(),
        outlook.disposition()
    ));
    if inherited.is_some() {
        note.push_str(&format!(
            " It shares its ancestral stock with the rest of the {family}, whose tongue, rites and craft it still echoes."
        ));
    }

    Culture {
        subsistence,
        kinship,
        outlook,
        craft,
        literacy,
        architecture,
        arts,
        note,
    }
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

    if let Some(original) = sk.displaced_from {
        reasons.push(format!(
            "Pushed off its own cradle: province #{original} — the best ground inside its reach — was already held by an older realm, so it settled the nearest free country instead.",
        ));
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
    if sk.displaced_from.is_some() {
        highlights.push("Settled the best ground still free".to_string());
    }
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

