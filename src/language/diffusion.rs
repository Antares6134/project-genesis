//! Spatial diffusion: how a tongue crosses a world.
//!
//! The province graph is the substrate. Each province carries a compact profile
//! (relief, river discharge, climate, coast, population), and every edge has a
//! friction derived from the country it leads into: rivers are roads, plains are
//! cheap, hills cost more, mountain spines and deep wilderness cost most, and the
//! open sea is a wall until a world has learned to sail. A multi-source Dijkstra
//! seeded at every Urheimat then fills the map cheapest-first, so the families
//! that meet in the middle leave a frontier exactly where the land itself makes
//! one.
//!
//! A second pass runs *inside* each family. Dense cradles split into several
//! daughter languages; a pastoral or wilderness family keeps one language across
//! a continent. That is what gives the Languages map its two textures at once:
//! sweeping macro-family blobs and tight little islands of speech around cities.

use crate::core::{Grid, relief, sphere};
use crate::geopolitics::{GeopoliticalHierarchy, ProvinceNode};
use crate::language::config::LanguageConfig;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Everything the language stage needs to know about one province.
#[derive(Clone, Copy, Default, Debug)]
pub struct ProvinceProfile {
    pub present: bool,
    pub center_x: f32,
    pub center_y: f32,
    pub continent: u32,
    pub area_km2: f32,
    pub mean_elevation_m: f32,
    pub max_flow: f32,
    pub coastal: bool,
    pub lakeshore: bool,
    pub mean_temp_c: f32,
    pub mean_precip: f32,
    pub population: u64,
    pub density_per_km2: f32,
    pub owner: u16,
}

/// One frontier step. `source` is a family id in the macro pass and a language id
/// in the daughter pass; ties always resolve by cost, then province, then source.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Step {
    cost: f32,
    province: u32,
    source: u16,
}

impl Eq for Step {}

impl Ord for Step {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.province.cmp(&other.province))
            .then_with(|| self.source.cmp(&other.source))
    }
}

impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Measure every province once, from the same grids the painter and the other
/// stages read.
#[allow(clippy::too_many_arguments)]
pub fn build_profiles(
    heightmap: &Grid<f32>,
    temperature: &Grid<f32>,
    precipitation: &Grid<f32>,
    flow: &Grid<f32>,
    is_lake: &Grid<bool>,
    province_ids: &Grid<u32>,
    hierarchy: &GeopoliticalHierarchy,
    owner: &[u16],
    population: &[u64],
    sea_level: f32,
) -> Vec<ProvinceProfile> {
    let w = heightmap.width;
    let h = heightmap.height;
    let n = hierarchy.provinces.len();
    let mut profiles = vec![ProvinceProfile::default(); n];

    // Accumulators are kept beside the public profile so the hot loop never has
    // to divide by a running pixel count.
    let mut px = vec![0u32; n];
    let mut elev_sum = vec![0f64; n];
    let mut temp_sum = vec![0f64; n];
    let mut precip_sum = vec![0f64; n];

    for y in 0..h {
        let area = sphere::pixel_area_km2(y, h, w);
        for x in 0..w {
            let id = *province_ids.get(x as isize, y as isize);
            if id == 0 {
                continue;
            }
            let idx = id as usize;
            if idx >= n {
                continue;
            }
            let elev = *heightmap.get(x as isize, y as isize);
            let profile = &mut profiles[idx];
            profile.present = true;
            profile.area_km2 += area;
            px[idx] += 1;
            elev_sum[idx] += relief::to_metres(elev) as f64;
            temp_sum[idx] += *temperature.get(x as isize, y as isize) as f64;
            precip_sum[idx] += *precipitation.get(x as isize, y as isize) as f64;
            profile.max_flow = profile.max_flow.max(*flow.get(x as isize, y as isize));

            // A province is coastal when one of its pixels touches open sea, and
            // a lakeshore when one touches an inland lake.
            for (dx, dy) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let nx = heightmap.wrap_x(x as isize + dx) as isize;
                let ny = dy + y as isize;
                if ny < 0 || ny >= h as isize {
                    continue;
                }
                if *is_lake.get(nx, ny) {
                    profile.lakeshore = true;
                } else if *heightmap.get(nx, ny) <= sea_level {
                    profile.coastal = true;
                }
            }
        }
    }

    for idx in 1..n {
        let node = &hierarchy.provinces[idx];
        let count = px[idx].max(1) as f64;
        let profile = &mut profiles[idx];
        profile.center_x = node.center_x;
        profile.center_y = node.center_y;
        profile.continent = node.continent_id;
        profile.mean_elevation_m = (elev_sum[idx] / count) as f32;
        profile.mean_temp_c = (temp_sum[idx] / count) as f32;
        profile.mean_precip = (precip_sum[idx] / count) as f32;
        profile.population = population.get(idx).copied().unwrap_or(0);
        profile.owner = owner.get(idx).copied().unwrap_or(0);
        profile.density_per_km2 = if profile.area_km2 > 0.0 {
            profile.population as f32 / profile.area_km2
        } else {
            0.0
        };
    }
    profiles
}

/// How expensive it is to enter one province.
fn terrain_friction(profile: &ProvinceProfile, config: &LanguageConfig) -> f32 {
    let mut friction = 1.0;

    // Relief: plains are cheap, hills cost more, mountains are a wall.
    if profile.mean_elevation_m >= config.mountain_wall_m {
        friction *= config.mountain_friction;
    } else if profile.mean_elevation_m >= config.hill_start_m {
        friction *= config.hill_friction;
    }

    // Climate and vegetation.
    if profile.mean_temp_c < 2.0 {
        friction *= config.deep_forest_friction;
    } else if profile.mean_precip < 0.10 {
        friction *= config.arid_friction;
    } else if profile.mean_temp_c < 7.0 {
        friction *= config.deep_forest_friction;
    } else if profile.mean_temp_c >= 24.0 && profile.mean_precip >= 0.46 {
        friction *= config.deep_forest_friction;
    } else if profile.mean_precip < 0.28 {
        friction *= config.grassland_friction;
    } else {
        friction *= config.woodland_friction;
    }

    // Rivers are roads; a step between two well-watered provinces follows the
    // valley rather than crossing it. Discharge is in m³/s now, so the bar is a
    // river worth following: a couple of hundred cubic metres a second — the
    // Thames is ~65, the Rhine ~2 900.
    if profile.max_flow >= 200.0 {
        friction *= config.river_discount;
    }

    // Cities resist absorption; empty country is cheap to sweep through.
    if profile.density_per_km2 >= config.urban_density_per_km2 {
        friction *= config.urban_resistance;
    } else if profile.density_per_km2 < 0.15 {
        friction *= config.wilderness_discount;
    }

    friction
}

/// What it costs to step from one province to another.
fn edge_step(
    from: &ProvinceNode,
    to: &ProvinceNode,
    to_profile: &ProvinceProfile,
    edge_cost: u32,
    width: usize,
    config: &LanguageConfig,
) -> Option<f32> {
    let hop = sphere::wrap_distance_px(
        from.center_x,
        from.center_y,
        to.center_x,
        to.center_y,
        width,
    );
    let jump = if hop <= config.hop_divisor_px {
        1.0
    } else {
        (hop / config.hop_divisor_px).powf(config.hop_exponent)
    };
    let step = (edge_cost.max(1) as f32) * 0.35 * jump * terrain_friction(to_profile, config);
    if step > config.max_step_cost {
        None
    } else {
        Some(step)
    }
}

/// Extra sea links, one per island province and its nearest mainland coast.
///
/// The province graph is a *land* graph: an island's neighbours are the other
/// parts of the island, not the sea. The hierarchy already records the closest
/// mainland coastal province (`anchor_id`), so that is the crossing a language
/// takes when a world has learned to sail.
fn sea_edges(
    profiles: &[ProvinceProfile],
    hierarchy: &GeopoliticalHierarchy,
    width: usize,
    config: &LanguageConfig,
    allow_sea: bool,
) -> Vec<Vec<(u32, u32)>> {
    let n = profiles.len();
    let mut extra = vec![Vec::new(); n];
    if !allow_sea || config.sea_crossing_max_px <= 0.0 {
        return extra;
    }
    for node in &hierarchy.provinces {
        let id = node.id as usize;
        if id == 0 || id >= n || node.anchor_id == 0 || node.anchor_id == node.id {
            continue;
        }
        let anchor = node.anchor_id as usize;
        if anchor >= n || !profiles[anchor].present {
            continue;
        }
        let distance = sphere::wrap_distance_px(
            node.center_x,
            node.center_y,
            hierarchy.provinces[anchor].center_x,
            hierarchy.provinces[anchor].center_y,
            width,
        );
        if distance > config.sea_crossing_max_px {
            continue;
        }
        let cost = (distance.max(1.0) * config.sea_crossing_cost).max(1.0) as u32;
        extra[id].push((node.anchor_id, cost));
        extra[anchor].push((node.id, cost));
    }
    extra
}

/// Grow every family at once across the province graph.
///
/// Returns province id → family id. Provinces no family could reach — a lone
/// island before seafaring, a valley behind a wall — are filled from the nearest
/// Urheimat afterwards, so the language map has no silent holes.
#[allow(clippy::too_many_arguments)]
pub fn diffuse_families(
    seeds: &[(u32, u16)],
    profiles: &[ProvinceProfile],
    hierarchy: &GeopoliticalHierarchy,
    width: usize,
    config: &LanguageConfig,
    allow_sea: bool,
    caps: &[usize],
) -> Vec<u16> {
    let n = profiles.len();
    let mut owner = vec![0u16; n];
    if seeds.is_empty() || n == 0 {
        return owner;
    }
    let max_family = seeds
        .iter()
        .map(|(_, family)| *family as usize)
        .max()
        .unwrap_or(0);
    let mut best = vec![f32::MAX; (max_family + 1) * n];
    let mut counts = vec![0usize; max_family + 1];
    let extra = sea_edges(profiles, hierarchy, width, config, allow_sea);
    let mut heap: BinaryHeap<Step> = BinaryHeap::new();

    let mut ordered = seeds.to_vec();
    ordered.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    for (province, family) in ordered {
        let index = province as usize;
        if index >= n || family == 0 {
            continue;
        }
        best[family as usize * n + index] = 0.0;
        heap.push(Step {
            cost: 0.0,
            province,
            source: family,
        });
    }

    while let Some(step) = heap.pop() {
        let province = step.province as usize;
        let family = step.source as usize;
        if province >= n || family == 0 || family > max_family {
            continue;
        }
        if step.cost > best[family * n + province] + 1e-3 {
            continue;
        }
        if owner[province] != 0 {
            continue;
        }
        let cap = caps.get(family).copied().unwrap_or(usize::MAX);
        if counts[family] >= cap {
            continue;
        }
        owner[province] = family as u16;
        counts[family] += 1;

        let node = &hierarchy.provinces[province];
        for &(neighbor, edge_cost) in node.neighbors.iter().chain(extra[province].iter()) {
            let nb = neighbor as usize;
            if nb >= n || !profiles[nb].present || owner[nb] != 0 {
                continue;
            }
            let Some(cost) = edge_step(
                node,
                &hierarchy.provinces[nb],
                &profiles[nb],
                edge_cost,
                width,
                config,
            ) else {
                continue;
            };
            let next = step.cost + cost;
            let slot = family * n + nb;
            if next + 1e-3 < best[slot] {
                best[slot] = next;
                heap.push(Step {
                    cost: next,
                    province: neighbor,
                    source: family as u16,
                });
            }
        }
    }

    for province in 1..n {
        if !profiles[province].present || owner[province] != 0 {
            continue;
        }
        let mut nearest: Option<(f32, u16)> = None;
        for (seed, family) in seeds {
            let seed = *seed as usize;
            if seed >= n {
                continue;
            }
            let distance = sphere::wrap_distance_px(
                profiles[province].center_x,
                profiles[province].center_y,
                profiles[seed].center_x,
                profiles[seed].center_y,
                width,
            );
            let better = match nearest {
                None => true,
                Some((best, best_family)) => {
                    distance < best - 1e-3
                        || ((distance - best).abs() <= 1e-3 && *family < best_family)
                }
            };
            if better {
                nearest = Some((distance, *family));
            }
        }
        if let Some((_, family)) = nearest {
            owner[province] = family;
        }
    }
    owner
}

/// Split one family into its daughter languages.
///
/// The same Dijkstra, restricted to the family's own provinces and seeded at the
/// family's proto-language plus any daughter seats the caller chose.
#[allow(clippy::too_many_arguments)]
pub fn diffuse_languages(
    seeds: &[(u32, u16)],
    language_family: &[u16],
    province_family: &[u16],
    profiles: &[ProvinceProfile],
    hierarchy: &GeopoliticalHierarchy,
    width: usize,
    config: &LanguageConfig,
    allow_sea: bool,
) -> Vec<u16> {
    let n = profiles.len();
    let mut owner = vec![0u16; n];
    if seeds.is_empty() || n == 0 {
        return owner;
    }
    let extra = sea_edges(profiles, hierarchy, width, config, allow_sea);
    let mut best = vec![f32::MAX; n];
    let mut heap: BinaryHeap<Step> = BinaryHeap::new();

    let mut ordered = seeds.to_vec();
    ordered.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    for (province, language) in ordered {
        let index = province as usize;
        if index >= n || language == 0 {
            continue;
        }
        best[index] = 0.0;
        heap.push(Step {
            cost: 0.0,
            province,
            source: language,
        });
    }

    while let Some(step) = heap.pop() {
        let province = step.province as usize;
        let language = step.source;
        if province >= n || language == 0 || step.cost > best[province] + 1e-3 {
            continue;
        }
        if owner[province] != 0 {
            continue;
        }
        let family = language_family
            .get(language as usize - 1)
            .copied()
            .unwrap_or(0);
        if family == 0 || province_family.get(province).copied().unwrap_or(0) != family {
            continue;
        }
        owner[province] = language;

        let node = &hierarchy.provinces[province];
        for &(neighbor, edge_cost) in node.neighbors.iter().chain(extra[province].iter()) {
            let nb = neighbor as usize;
            if nb >= n
                || !profiles[nb].present
                || owner[nb] != 0
                || province_family.get(nb).copied().unwrap_or(0) != family
            {
                continue;
            }
            let Some(cost) = edge_step(
                node,
                &hierarchy.provinces[nb],
                &profiles[nb],
                edge_cost,
                width,
                config,
            ) else {
                continue;
            };
            let next = step.cost + cost;
            if next + 1e-3 < best[nb] {
                best[nb] = next;
                heap.push(Step {
                    cost: next,
                    province: neighbor,
                    source: language,
                });
            }
        }
    }

    for province in 1..n {
        if !profiles[province].present || owner[province] != 0 {
            continue;
        }
        let family = province_family.get(province).copied().unwrap_or(0);
        if family == 0 {
            continue;
        }
        let mut nearest: Option<(f32, u16)> = None;
        for (seed, language) in seeds {
            let seed = *seed as usize;
            if seed >= n
                || language_family
                    .get(*language as usize - 1)
                    .copied()
                    .unwrap_or(0)
                    != family
            {
                continue;
            }
            let distance = sphere::wrap_distance_px(
                profiles[province].center_x,
                profiles[province].center_y,
                profiles[seed].center_x,
                profiles[seed].center_y,
                width,
            );
            let better = match nearest {
                None => true,
                Some((best, best_language)) => {
                    distance < best - 1e-3
                        || ((distance - best).abs() <= 1e-3 && *language < best_language)
                }
            };
            if better {
                nearest = Some((distance, *language));
            }
        }
        if let Some((_, language)) = nearest {
            owner[province] = language;
        }
    }
    owner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::config::LanguageConfig;

    fn line_world(count: usize) -> (Vec<ProvinceProfile>, GeopoliticalHierarchy) {
        let mut profiles = vec![ProvinceProfile::default(); count];
        let mut provinces = Vec::new();
        for index in 0..count {
            profiles[index] = ProvinceProfile {
                present: true,
                center_x: index as f32 * 20.0,
                center_y: 40.0,
                continent: 1,
                area_km2: 500.0,
                mean_elevation_m: 100.0,
                max_flow: if index % 2 == 0 { 3_000.0 } else { 100.0 },
                coastal: false,
                lakeshore: false,
                mean_temp_c: 15.0,
                mean_precip: 0.35,
                population: 10_000,
                density_per_km2: 2.0,
                owner: 0,
            };
            let mut neighbors = Vec::new();
            if index > 0 {
                neighbors.push((index as u32 - 1, 20));
            }
            if index + 1 < count {
                neighbors.push((index as u32 + 1, 20));
            }
            provinces.push(ProvinceNode {
                id: index as u32,
                center_x: index as f32 * 20.0,
                center_y: 40.0,
                pixel_count: 100,
                neighbors,
                area_id: 0,
                region_id: 0,
                superregion_id: 0,
                continent_id: 1,
                anchor_id: 0,
            });
        }
        // Province id 0 is unused, matching the real hierarchy.
        (
            profiles,
            GeopoliticalHierarchy {
                total_provinces: count,
                total_areas: 1,
                total_regions: 1,
                total_superregions: 1,
                total_continents: 1,
                provinces,
            },
        )
    }

    #[test]
    fn a_family_reaches_every_province_it_can_walk_to() {
        let (profiles, hierarchy) = line_world(6);
        let config = LanguageConfig::default();
        let caps = [0, usize::MAX, usize::MAX];
        let owner = diffuse_families(
            &[(1, 1), (4, 2)],
            &profiles,
            &hierarchy,
            200,
            &config,
            false,
            &caps,
        );
        for province in 1..profiles.len() {
            assert_ne!(owner[province], 0, "province {province} was left silent");
        }
        assert_eq!(owner[1], 1, "the western seed keeps its own ground");
        assert_eq!(owner[4], 2, "the eastern seed keeps its own ground");
    }

    #[test]
    fn diffusion_is_reproducible() {
        let (profiles, hierarchy) = line_world(8);
        let config = LanguageConfig::default();
        let caps = [0, usize::MAX, usize::MAX, usize::MAX];
        let first = diffuse_families(
            &[(1, 1), (4, 2), (7, 3)],
            &profiles,
            &hierarchy,
            300,
            &config,
            false,
            &caps,
        );
        let second = diffuse_families(
            &[(1, 1), (4, 2), (7, 3)],
            &profiles,
            &hierarchy,
            300,
            &config,
            false,
            &caps,
        );
        assert_eq!(first, second);
    }

    #[test]
    fn a_daughter_pass_stays_inside_its_family() {
        let (profiles, hierarchy) = line_world(6);
        let config = LanguageConfig::default();
        let family_caps = [0, usize::MAX, usize::MAX];
        let province_family = diffuse_families(
            &[(1, 1), (4, 2)],
            &profiles,
            &hierarchy,
            200,
            &config,
            false,
            &family_caps,
        );
        let language_family = [1u16, 2u16];
        let owner = diffuse_languages(
            &[(1, 1), (4, 2)],
            &language_family,
            &province_family,
            &profiles,
            &hierarchy,
            200,
            &config,
            false,
        );
        for province in 1..profiles.len() {
            let family = province_family[province];
            let language = owner[province];
            if family == 1 {
                assert_eq!(language, 1, "province {province} left its family");
            } else if family == 2 {
                assert_eq!(language, 2, "province {province} left its family");
            }
        }
    }
}
