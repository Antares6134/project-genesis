use crate::geography::config::OceanConfig;
use crate::geopolitics::hierarchy::{GeopoliticalHierarchy, ProvinceNode};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::{BTreeMap, BinaryHeap, HashSet};

// ─── Priority-queue state for Dijkstra ───────────────────────────────────────
#[derive(Copy, Clone, Eq, PartialEq)]
struct DijkState {
    cost: u32,
    id:   u32,
    cid:  u32,
}

impl Ord for DijkState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost).then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for DijkState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Spherical wrap-around distance on the X axis
fn wrap_dist_sq(x1: f32, y1: f32, x2: f32, y2: f32, w: f32) -> f32 {
    let mut dx = (x1 - x2).abs();
    if dx > w * 0.5 { dx = w - dx; }
    let dy = y1 - y2;
    dx * dx + dy * dy
}

/// Cluster a set of provinces into `k` balanced groups using capacity-aware
/// Dijkstra with Lloyd centroid relaxation.
///
/// Every map below is a `BTreeMap`: the clustering consumes a seeded RNG while
/// iterating parent groups, so a stable iteration order is what makes the whole
/// hierarchy reproducible for a given seed.
fn cluster_group(
    members:      &[u32],
    member_set:   &HashSet<u32>,
    provinces:    &[ProvinceNode],
    k:            usize,
    first_cid:    u32,
    rng:          &mut ChaCha8Rng,
    map_width:    f32,
) -> BTreeMap<u32, u32> {
    let mut assignments: BTreeMap<u32, u32> = BTreeMap::new();

    if k == 1 {
        for &m in members { assignments.insert(m, first_cid); }
        return assignments;
    }

    let target_size = (members.len() as f32 / k as f32).max(1.0);

    // ── Step 1: K-Centers Farthest-point seeding ────────────────────────────
    let mut seeds: Vec<u32> = Vec::with_capacity(k);
    seeds.push(*members.choose(rng).unwrap());

    for _ in 1..k {
        let mut dist: BTreeMap<u32, u32> = BTreeMap::new();
        let mut heap: BinaryHeap<DijkState> = BinaryHeap::new();

        for &s in &seeds {
            dist.insert(s, 0);
            heap.push(DijkState { cost: 0, id: s, cid: s });
        }

        let mut farthest   = seeds[0];
        let mut farthest_d = 0u32;

        while let Some(state) = heap.pop() {
            let best = *dist.get(&state.id).unwrap_or(&u32::MAX);
            if state.cost > best { continue; }

            if state.cost > farthest_d {
                farthest_d = state.cost;
                farthest   = state.id;
            }

            for &(nb, edge_cost) in &provinces[state.id as usize].neighbors {
                if !member_set.contains(&nb) { continue; }
                let new_cost = state.cost.saturating_add(edge_cost);
                let prev     = *dist.get(&nb).unwrap_or(&u32::MAX);
                if new_cost < prev {
                    dist.insert(nb, new_cost);
                    heap.push(DijkState { cost: new_cost, id: nb, cid: state.cid });
                }
            }
        }

        // Fallback for unreachable / disconnected nodes
        if farthest_d == 0 || seeds.contains(&farthest) {
            let mut max_euc_d = -1.0f32;
            let mut best_m = members[0];
            for &m in members {
                if seeds.contains(&m) { continue; }
                let (mx, my) = (provinces[m as usize].center_x, provinces[m as usize].center_y);
                let mut min_to_seed = f32::MAX;
                for &s in &seeds {
                    let (sx, sy) = (provinces[s as usize].center_x, provinces[s as usize].center_y);
                    let d2 = wrap_dist_sq(mx, my, sx, sy, map_width);
                    if d2 < min_to_seed { min_to_seed = d2; }
                }
                if min_to_seed > max_euc_d {
                    max_euc_d = min_to_seed;
                    best_m = m;
                }
            }
            farthest = best_m;
        }

        seeds.push(farthest);
    }

    // ── Step 2: Lloyd Relaxation (2 Passes) with Capacity-Weighted Dijkstra ──
    for pass in 0..2 {
        let is_final = pass == 1;
        assignments.clear();

        let mut best: BTreeMap<u32, u32> = BTreeMap::new();
        let mut heap: BinaryHeap<DijkState> = BinaryHeap::new();
        let mut cluster_claimed = vec![0usize; k];

        for (idx, &s) in seeds.iter().enumerate() {
            let cid = first_cid + idx as u32;
            best.insert(s, 0);
            assignments.insert(s, cid);
            cluster_claimed[idx] += 1;
            heap.push(DijkState { cost: 0, id: s, cid });
        }

        while let Some(state) = heap.pop() {
            let prev = *best.get(&state.id).unwrap_or(&u32::MAX);
            if state.cost > prev { continue; }

            let c_idx = (state.cid - first_cid) as usize;
            let claimed_count = if c_idx < k { cluster_claimed[c_idx] } else { 0 };

            // Capacity-Aware Weighting:
            // When a cluster exceeds its fair share (target_size), expanding further becomes
            // progressively more expensive, giving smaller clusters room to expand equally.
            let penalty = if claimed_count as f32 > target_size {
                1.0 + ((claimed_count as f32 - target_size) / target_size) * 3.5
            } else {
                1.0
            };

            for &(nb, edge_cost) in &provinces[state.id as usize].neighbors {
                if !member_set.contains(&nb) { continue; }
                let weighted_cost = (edge_cost as f32 * penalty) as u32;
                let new_cost = state.cost.saturating_add(weighted_cost);
                let prev_nb  = *best.get(&nb).unwrap_or(&u32::MAX);

                if new_cost < prev_nb {
                    best.insert(nb, new_cost);
                    if !assignments.contains_key(&nb) && c_idx < k {
                        cluster_claimed[c_idx] += 1;
                    }
                    assignments.insert(nb, state.cid);
                    heap.push(DijkState { cost: new_cost, id: nb, cid: state.cid });
                }
            }
        }

        // Unreached members fallback to nearest seed
        for &m in members {
            if !assignments.contains_key(&m) {
                let (mx, my) = (provinces[m as usize].center_x, provinces[m as usize].center_y);
                let mut closest_cid = first_cid;
                let mut min_d2 = f32::MAX;
                for (idx, &s) in seeds.iter().enumerate() {
                    let (sx, sy) = (provinces[s as usize].center_x, provinces[s as usize].center_y);
                    let d2 = wrap_dist_sq(mx, my, sx, sy, map_width);
                    if d2 < min_d2 {
                        min_d2 = d2;
                        closest_cid = first_cid + idx as u32;
                    }
                }
                assignments.insert(m, closest_cid);
            }
        }

        // Centroid Relocation for next pass
        if !is_final {
            let mut sum_x = vec![0.0f32; k];
            let mut sum_y = vec![0.0f32; k];
            let mut count = vec![0usize; k];

            for &m in members {
                let cid = assignments[&m];
                let idx = (cid - first_cid) as usize;
                if idx < k {
                    sum_x[idx] += provinces[m as usize].center_x;
                    sum_y[idx] += provinces[m as usize].center_y;
                    count[idx] += 1;
                }
            }

            for idx in 0..k {
                if count[idx] > 0 {
                    let avg_x = sum_x[idx] / count[idx] as f32;
                    let avg_y = sum_y[idx] / count[idx] as f32;

                    let mut best_m = seeds[idx];
                    let mut min_d2 = f32::MAX;
                    for &m in members {
                        if assignments.get(&m) == Some(&(first_cid + idx as u32)) {
                            let (px, py) = (provinces[m as usize].center_x, provinces[m as usize].center_y);
                            let d2 = wrap_dist_sq(px, py, avg_x, avg_y, map_width);
                            if d2 < min_d2 {
                                min_d2 = d2;
                                best_m = m;
                            }
                        }
                    }
                    seeds[idx] = best_m;
                }
            }
        }
    }

    assignments
}

/// Cluster provinces into a single tier.
fn cluster_tier(
    provinces:               &[ProvinceNode],
    parent_ids:              &[u32],
    target_cluster_size:     usize,
    max_clusters_per_parent: usize,
    seed:                    u64,
    map_width:               f32,
) -> (Vec<u32>, usize) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let n = provinces.len();
    let mut assignments = vec![0u32; n];

    let mut parent_groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for p in provinces.iter() {
        if p.id == 0 || p.pixel_count == 0 { continue; }
        if (p.id as usize) >= parent_ids.len() { continue; }
        let parent = parent_ids[p.id as usize];
        if parent == 0 { continue; }
        parent_groups.entry(parent).or_default().push(p.id);
    }

    let mut next_cid = 1u32;

    for (_parent, mut members) in parent_groups {
        let count = members.len();
        if count == 0 { continue; }

        let k = ((count as f32 / target_cluster_size as f32).round() as usize)
            .clamp(1, max_clusters_per_parent.min(count));

        let member_set: HashSet<u32> = members.iter().copied().collect();
        members.shuffle(&mut rng);

        let group_assignments = cluster_group(
            &members, &member_set, provinces, k, next_cid, &mut rng, map_width,
        );

        for (pid, cid) in group_assignments {
            assignments[pid as usize] = cid;
        }

        next_cid += k as u32;
    }

    let total = (next_cid - 1) as usize;
    (assignments, total)
}

/// Absorb small clusters below `min_size` into their strongest neighboring cluster.
/// Strictly enforces that clusters ONLY merge into a neighbor within the SAME parent.
fn absorb_small_clusters(
    provinces:   &[ProvinceNode],
    cluster_ids: &mut [u32],
    parent_ids:  &[u32],
    min_size:    usize,
    map_width:   f32,
) -> usize {
    let n = provinces.len();

    loop {
        let mut cluster_sizes: BTreeMap<u32, usize> = BTreeMap::new();
        let mut cluster_parent: BTreeMap<u32, u32> = BTreeMap::new();
        let mut parent_cluster_count: BTreeMap<u32, usize> = BTreeMap::new();

        for p in provinces.iter() {
            if p.id == 0 || p.pixel_count == 0 { continue; }
            let cid = cluster_ids[p.id as usize];
            if cid != 0 {
                *cluster_sizes.entry(cid).or_insert(0) += 1;
                let parent = parent_ids[p.id as usize];
                cluster_parent.insert(cid, parent);
            }
        }

        for (&_cid, &parent) in &cluster_parent {
            *parent_cluster_count.entry(parent).or_insert(0) += 1;
        }

        let mut small_clusters: Vec<u32> = cluster_sizes
            .iter()
            .filter(|&(&cid, &count)| {
                count < min_size && parent_cluster_count.get(&cluster_parent[&cid]).copied().unwrap_or(0) > 1
            })
            .map(|(&cid, _)| cid)
            .collect();

        if small_clusters.is_empty() {
            break;
        }

        small_clusters.sort_by_key(|cid| cluster_sizes[cid]);

        let mut any_merged = false;

        for small_cid in small_clusters {
            let members: Vec<u32> = (1..n)
                .filter(|&i| cluster_ids[i] == small_cid && provinces[i].pixel_count > 0)
                .map(|i| i as u32)
                .collect();

            if members.is_empty() { continue; }
            let current_parent = parent_ids[members[0] as usize];

            if parent_cluster_count.get(&current_parent).copied().unwrap_or(0) <= 1 {
                continue;
            }

            let mut neighbor_scores: BTreeMap<u32, u32> = BTreeMap::new();

            for &m in &members {
                for &(nb, cost) in &provinces[m as usize].neighbors {
                    let nb_cid = cluster_ids[nb as usize];
                    if nb_cid != 0 && nb_cid != small_cid {
                        let nb_parent = parent_ids[nb as usize];
                        if nb_parent == current_parent {
                            let score = 5000u32.saturating_sub(cost.min(4900));
                            *neighbor_scores.entry(nb_cid).or_insert(0) += score;
                        }
                    }
                }
            }

            let target_cid = if let Some((&best_cid, _)) = neighbor_scores.iter().max_by_key(|&(_, score)| score) {
                Some(best_cid)
            } else {
                let (mx, my) = (provinces[members[0] as usize].center_x, provinces[members[0] as usize].center_y);
                let mut closest_cid = None;
                let mut min_d2 = f32::MAX;

                for p in provinces.iter() {
                    if p.id == 0 || p.pixel_count == 0 { continue; }
                    let other_cid = cluster_ids[p.id as usize];
                    if other_cid != 0 && other_cid != small_cid && parent_ids[p.id as usize] == current_parent {
                        let d2 = wrap_dist_sq(mx, my, p.center_x, p.center_y, map_width);
                        if d2 < min_d2 {
                            min_d2 = d2;
                            closest_cid = Some(other_cid);
                        }
                    }
                }
                closest_cid
            };

            if let Some(dest_cid) = target_cid {
                for &m in &members {
                    cluster_ids[m as usize] = dest_cid;
                }
                any_merged = true;
            }
        }

        if !any_merged {
            break;
        }
    }

    // Re-index clusters to continuous 1..=total IDs
    let mut unique_ids: Vec<u32> = cluster_ids.iter().copied().filter(|&id| id != 0).collect();
    unique_ids.sort_unstable();
    unique_ids.dedup();

    let remap: BTreeMap<u32, u32> = unique_ids
        .iter()
        .enumerate()
        .map(|(idx, &old)| (old, (idx + 1) as u32))
        .collect();

    for id in cluster_ids.iter_mut() {
        if *id != 0 {
            *id = *remap.get(id).unwrap();
        }
    }

    remap.len()
}

/// Build the complete four-level geopolitical hierarchy:
///
/// ```
/// Continent  (Major landmass cores + strictly anchored islands)
///   └─ Superregion  (~200 provinces, balanced, undersized absorbed)
///        └─ Region  (~35 provinces, kingdom scale, undersized absorbed)
///             └─ Area  (~6 provinces, island chains united, min size 3)
/// ```
pub fn build_geopolitical_hierarchy(
    mut provinces: Vec<ProvinceNode>,
    seed:          u64,
    map_width:     f32,
) -> GeopoliticalHierarchy {
    let n = provinces.len();

    // ── 1. Continents ─────────────────────────────────────────────────────────
    let continent_ids: Vec<u32> = provinces.iter().map(|p| p.continent_id).collect();
    let continent_count = continent_ids.iter().filter(|&&c| c != 0).copied()
        .collect::<HashSet<_>>().len();
    println!("✓ Identified {} Continent(s) (All islands anchored to their nearest continent).", continent_count);

    // ── 2. Superregions (Balanced realms: ~200 provinces) ─────────────────────
    println!("  Clustering Superregions (Strictly inside Continents)…");
    let (mut super_ids, _) = cluster_tier(
        &provinces,
        &continent_ids,
        200,   // ~200 provinces per superregion
        50,    // At most 50 superregions per large continent
        seed.wrapping_add(3),
        map_width,
    );

    // COASTAL ISLAND ANCHORING: Every island strictly inherits its superregion from its anchor coast!
    // - Britain, Ireland, Iceland -> Western Europe Superregion!
    // - Newfoundland, Greenland -> North America Superregion!
    // - Island A between Coast A and Coast B -> Coast A's Superregion!
    for i in 1..n {
        let anchor = provinces[i].anchor_id;
        if anchor != 0 && anchor != i as u32 {
            super_ids[i] = super_ids[anchor as usize];
        }
    }

    // Absorb any superregion with < 95 provinces (guarantees no tiny sliver exists between large ones)
    let total_superregions = absorb_small_clusters(
        &provinces,
        &mut super_ids,
        &continent_ids,
        95,
        map_width,
    );

    for i in 1..n {
        provinces[i].superregion_id = super_ids[i];
    }
    println!("  ✓ Generated {} Superregion(s) worldwide (undersized absorbed).", total_superregions);

    // ── 3. Regions (Kingdom level: ~35 provinces) ─────────────────────────────
    println!("  Clustering Regions (Strictly inside Superregions)…");
    let (mut region_ids, _) = cluster_tier(
        &provinces,
        &super_ids,
        35,    // ~35 provinces per region
        25,    // Up to 25 regions per superregion
        seed.wrapping_add(2),
        map_width,
    );

    // For islands with < 25 provinces, strictly inherit Region from anchor coast
    // (Large islands with >= 25 provinces form their own Region within the Superregion)
    for i in 1..n {
        let anchor = provinces[i].anchor_id;
        if anchor != 0 && anchor != i as u32 {
            // Count size of this island's landmass
            let anchor_p = &provinces[anchor as usize];
            if provinces[i].pixel_count < 25 * 300 {
                region_ids[i] = region_ids[anchor_p.id as usize];
            }
        }
    }

    // Absorb small regions (< 16 provinces) into their best neighbor region
    let total_regions = absorb_small_clusters(
        &provinces,
        &mut region_ids,
        &super_ids,
        16,
        map_width,
    );

    for i in 1..n {
        provinces[i].region_id = region_ids[i];
    }
    println!("  ✓ Generated {} Region(s) worldwide (undersized absorbed).", total_regions);

    // ── 4. Areas (State level: ~6 provinces; small islands united) ────────────
    println!("  Clustering Areas (Strictly inside Regions)…");
    let (mut area_ids, _) = cluster_tier(
        &provinces,
        &region_ids,
        3,     // ~3 min provinces per area
        6,    // 6 max provinces per area
        seed.wrapping_add(1),
        map_width,
    );

    // Small 1-2 province coastal rocks merge directly into their anchor coast's Area
    for i in 1..n {
        let anchor = provinces[i].anchor_id;
        if anchor != 0 && anchor != i as u32 && provinces[i].pixel_count < 200 {
            area_ids[i] = area_ids[anchor as usize];
        }
    }

    // Absorb small areas (< 3 provinces) so multiple small islands become a single area
    let total_areas = absorb_small_clusters(
        &provinces,
        &mut area_ids,
        &region_ids,
        3,
        map_width,
    );

    for i in 1..n {
        provinces[i].area_id = area_ids[i];
    }
    println!("  ✓ Generated {} Area(s) worldwide (island areas unified).", total_areas);

    GeopoliticalHierarchy {
        total_provinces: provinces.len().saturating_sub(1),
        total_areas,
        total_regions,
        total_superregions,
        total_continents: continent_count,
        provinces,
    }
}

/// Build the five-tier hydro-political hierarchy for the world ocean.
///
/// Mirrors the land hierarchy (`Province → Area → Region → Superregion →
/// Continent`) but every tier is scaled up, as the ocean is the larger domain.
/// Unlike the land there are no natural landmasses to seed continents from — the
/// ocean is one connected body — so the top tier is produced by clustering the
/// marine provinces directly into the great oceanic basins, and each lower tier
/// is then clustered strictly inside its parent.
pub fn build_ocean_hierarchy(
    mut provinces: Vec<ProvinceNode>,
    seed:      u64,
    map_width: f32,
    config:    &OceanConfig,
) -> GeopoliticalHierarchy {
    let n = provinces.len();
    if n <= 1 {
        return GeopoliticalHierarchy {
            total_provinces: 0,
            total_areas: 0,
            total_regions: 0,
            total_superregions: 0,
            total_continents: 0,
            provinces,
        };
    }

    // ── 1. Ocean Continents (the great basins) ────────────────────────────────
    // The whole ocean is one parent group, so this is a global partition.
    let roots: Vec<u32> = vec![1; n];
    let (mut continent_ids, continent_count) = cluster_tier(
        &provinces,
        &roots,
        config.continent_target,
        8, // at most 8 ocean basins
        seed.wrapping_add(4),
        map_width,
    );
    for i in 1..n {
        provinces[i].continent_id = continent_ids[i];
    }
    println!("  ✓ Clustered {} Ocean Basin(s).", continent_count);

    // ── 2. Oceanic Superregions (Strictly inside Basins) ──────────────────────
    let (mut super_ids, _) = cluster_tier(
        &provinces,
        &continent_ids,
        config.superregion_target,
        50,
        seed.wrapping_add(3),
        map_width,
    );
    // Undersized superregions are absorbed so no tiny sliver survives between
    // two large ocean realms.
    let total_superregions = absorb_small_clusters(
        &provinces,
        &mut super_ids,
        &continent_ids,
        (config.superregion_target / 5).max(6),
        map_width,
    );
    for i in 1..n {
        provinces[i].superregion_id = super_ids[i];
    }
    println!("  ✓ Clustered {} Oceanic Superregion(s) (undersized absorbed).", total_superregions);

    // ── 3. Oceanic Regions (Strictly inside Superregions) ─────────────────────
    let (mut region_ids, _) = cluster_tier(
        &provinces,
        &super_ids,
        config.region_target,
        40,
        seed.wrapping_add(2),
        map_width,
    );
    let total_regions = absorb_small_clusters(
        &provinces,
        &mut region_ids,
        &super_ids,
        (config.region_target / 5).max(4),
        map_width,
    );
    for i in 1..n {
        provinces[i].region_id = region_ids[i];
    }
    println!("  ✓ Clustered {} Oceanic Region(s) (undersized absorbed).", total_regions);

    // ── 4. Oceanic Areas (Strictly inside Regions) ────────────────────────────
    let (mut area_ids, _) = cluster_tier(
        &provinces,
        &region_ids,
        config.area_target,
        25,
        seed.wrapping_add(1),
        map_width,
    );
    let total_areas = absorb_small_clusters(
        &provinces,
        &mut area_ids,
        &region_ids,
        2,
        map_width,
    );
    for i in 1..n {
        provinces[i].area_id = area_ids[i];
    }
    println!("  ✓ Clustered {} Oceanic Area(s) (undersized absorbed).", total_areas);

    GeopoliticalHierarchy {
        total_provinces: provinces.iter().filter(|p| p.pixel_count > 0).count(),
        total_areas,
        total_regions,
        total_superregions,
        total_continents: continent_count,
        provinces,
    }
}