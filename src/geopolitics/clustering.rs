use crate::core::progress::Progress;
use crate::core::sphere::CanvasScale;
use crate::geography::config::OceanConfig;
use crate::geopolitics::hierarchy::{GeopoliticalHierarchy, ProvinceNode};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashSet};

// ─── Priority-queue state for Dijkstra ───────────────────────────────────────
#[derive(Copy, Clone, Eq, PartialEq)]
struct DijkState {
    cost: u32,
    id: u32,
    cid: u32,
}

impl Ord for DijkState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for DijkState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// How a clustering tier measures the size of one of its clusters.
///
/// The upper tiers are cut by head count: a Region is a run of ~45 provinces
/// whether they are desert or delta. The land Areas cannot be, because the map
/// is read by the eye rather than counted: a desert province and a coastal speck
/// must not buy the same amount of drawn map. The Areas are therefore budgeted
/// in *pixels*, which is what makes the ocean's own Area plate read evenly —
/// its provinces are already of one size.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ClusterWeight {
    /// One per province — the historical measure.
    Count,
    /// The province's own pixels, floored at one so a speck still counts.
    Pixels,
}

impl ClusterWeight {
    #[inline]
    fn of(self, province: &ProvinceNode) -> usize {
        match self {
            ClusterWeight::Count => 1,
            ClusterWeight::Pixels => province.pixel_count.max(1),
        }
    }

    #[inline]
    fn of_ids(self, provinces: &[ProvinceNode], ids: &[u32]) -> usize {
        ids.iter().map(|&id| self.of(&provinces[id as usize])).sum()
    }
}

/// Spherical wrap-around distance on the X axis
fn wrap_dist_sq(x1: f32, y1: f32, x2: f32, y2: f32, w: f32) -> f32 {
    let mut dx = (x1 - x2).abs();
    if dx > w * 0.5 {
        dx = w - dx;
    }
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
    members: &[u32],
    member_set: &HashSet<u32>,
    provinces: &[ProvinceNode],
    k: usize,
    first_cid: u32,
    rng: &mut ChaCha8Rng,
    map_width: f32,
    weight: ClusterWeight,
) -> BTreeMap<u32, u32> {
    let mut assignments: BTreeMap<u32, u32> = BTreeMap::new();

    if k == 1 {
        for &m in members {
            assignments.insert(m, first_cid);
        }
        return assignments;
    }

    let total_weight = weight.of_ids(provinces, members);
    let target_size = (total_weight as f32 / k as f32).max(1.0);

    // ── Step 1: K-Centers Farthest-point seeding ────────────────────────────
    let mut seeds: Vec<u32> = Vec::with_capacity(k);
    seeds.push(*members.choose(rng).unwrap());

    for _ in 1..k {
        let mut dist: BTreeMap<u32, u32> = BTreeMap::new();
        let mut heap: BinaryHeap<DijkState> = BinaryHeap::new();

        for &s in &seeds {
            dist.insert(s, 0);
            heap.push(DijkState {
                cost: 0,
                id: s,
                cid: s,
            });
        }

        let mut farthest = seeds[0];
        let mut farthest_d = 0u32;

        while let Some(state) = heap.pop() {
            let best = *dist.get(&state.id).unwrap_or(&u32::MAX);
            if state.cost > best {
                continue;
            }

            if state.cost > farthest_d {
                farthest_d = state.cost;
                farthest = state.id;
            }

            for &(nb, edge_cost) in &provinces[state.id as usize].neighbors {
                if !member_set.contains(&nb) {
                    continue;
                }
                let new_cost = state.cost.saturating_add(edge_cost);
                let prev = *dist.get(&nb).unwrap_or(&u32::MAX);
                if new_cost < prev {
                    dist.insert(nb, new_cost);
                    heap.push(DijkState {
                        cost: new_cost,
                        id: nb,
                        cid: state.cid,
                    });
                }
            }
        }

        // Fallback for unreachable / disconnected nodes
        if farthest_d == 0 || seeds.contains(&farthest) {
            let mut max_euc_d = -1.0f32;
            let mut best_m = members[0];
            for &m in members {
                if seeds.contains(&m) {
                    continue;
                }
                let (mx, my) = (
                    provinces[m as usize].center_x,
                    provinces[m as usize].center_y,
                );
                let mut min_to_seed = f32::MAX;
                for &s in &seeds {
                    let (sx, sy) = (
                        provinces[s as usize].center_x,
                        provinces[s as usize].center_y,
                    );
                    let d2 = wrap_dist_sq(mx, my, sx, sy, map_width);
                    if d2 < min_to_seed {
                        min_to_seed = d2;
                    }
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
            cluster_claimed[idx] += weight.of(&provinces[s as usize]);
            heap.push(DijkState {
                cost: 0,
                id: s,
                cid,
            });
        }

        while let Some(state) = heap.pop() {
            let prev = *best.get(&state.id).unwrap_or(&u32::MAX);
            if state.cost > prev {
                continue;
            }

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
                if !member_set.contains(&nb) {
                    continue;
                }
                let weighted_cost = (edge_cost as f32 * penalty) as u32;
                let new_cost = state.cost.saturating_add(weighted_cost);
                let prev_nb = *best.get(&nb).unwrap_or(&u32::MAX);

                if new_cost < prev_nb {
                    best.insert(nb, new_cost);
                    if !assignments.contains_key(&nb) && c_idx < k {
                        cluster_claimed[c_idx] += weight.of(&provinces[nb as usize]);
                    }
                    assignments.insert(nb, state.cid);
                    heap.push(DijkState {
                        cost: new_cost,
                        id: nb,
                        cid: state.cid,
                    });
                }
            }
        }

        // Unreached members fallback to nearest seed
        for &m in members {
            if !assignments.contains_key(&m) {
                let (mx, my) = (
                    provinces[m as usize].center_x,
                    provinces[m as usize].center_y,
                );
                let mut closest_cid = first_cid;
                let mut min_d2 = f32::MAX;
                for (idx, &s) in seeds.iter().enumerate() {
                    let (sx, sy) = (
                        provinces[s as usize].center_x,
                        provinces[s as usize].center_y,
                    );
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
            // A province centre is a longitude, and a longitude wraps. Averaging
            // `x` arithmetically would put a group that straddles the antimeridian
            // in the middle of the map and drag its medoid there — one bogus seed,
            // one malformed cluster at the seam. The mean is taken on the circle
            // and only then turned back into map pixels.
            let mut sum_cos_x = vec![0.0f64; k];
            let mut sum_sin_x = vec![0.0f64; k];
            let mut sum_y = vec![0.0f32; k];
            let mut count = vec![0usize; k];

            for &m in members {
                let cid = assignments[&m];
                let idx = (cid - first_cid) as usize;
                if idx < k {
                    let centre = &provinces[m as usize];
                    let angle = (centre.center_x / map_width) as f64 * std::f64::consts::TAU;
                    sum_cos_x[idx] += angle.cos();
                    sum_sin_x[idx] += angle.sin();
                    sum_y[idx] += centre.center_y;
                    count[idx] += 1;
                }
            }

            for idx in 0..k {
                if count[idx] > 0 {
                    let mean_angle = sum_sin_x[idx]
                        .atan2(sum_cos_x[idx])
                        .rem_euclid(std::f64::consts::TAU);
                    let avg_x = ((mean_angle / std::f64::consts::TAU) * map_width as f64) as f32;
                    let avg_y = sum_y[idx] / count[idx] as f32;

                    let mut best_m = seeds[idx];
                    let mut min_d2 = f32::MAX;
                    for &m in members {
                        if assignments.get(&m) == Some(&(first_cid + idx as u32)) {
                            let (px, py) = (
                                provinces[m as usize].center_x,
                                provinces[m as usize].center_y,
                            );
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
/// Show the atlas a clustering pass's work so far.
///
/// A clustering pass never sees a pixel — it works on province *nodes* — so the
/// picture is painted through the pixel grids the stage registered on its
/// [`Progress`] handle. The land and the ocean hierarchies are both built this
/// way, and each has its own registered grid of provinces.
fn preview_tier_of(progress: &Progress, label: &'static str, assignment: &[u32], ocean: bool) {
    if ocean {
        progress.preview_ocean_tier(label, assignment);
    } else {
        progress.preview_tier(label, assignment);
    }
}

fn cluster_tier(
    provinces: &[ProvinceNode],
    parent_ids: &[u32],
    target_cluster_size: usize,
    max_clusters_per_parent: usize,
    seed: u64,
    map_width: f32,
    progress: &Progress,
    label: &'static str,
    ocean: bool,
    weight: ClusterWeight,
) -> (Vec<u32>, usize) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let n = provinces.len();
    let mut assignments = vec![0u32; n];

    let mut parent_groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for p in provinces.iter() {
        if p.id == 0 || p.pixel_count == 0 {
            continue;
        }
        if (p.id as usize) >= parent_ids.len() {
            continue;
        }
        let parent = parent_ids[p.id as usize];
        if parent == 0 {
            continue;
        }
        parent_groups.entry(parent).or_default().push(p.id);
    }

    // One checkpoint per parent group: the tier is built kingdom by kingdom, so
    // the atlas can watch the world fill in rather than wait for the whole tier.
    progress.sub_step(label, parent_groups.len() as u64);

    let mut next_cid = 1u32;

    for (group, (_parent, mut members)) in parent_groups.into_iter().enumerate() {
        progress.tick(group as u64);
        preview_tier_of(progress, label, &assignments, ocean);
        let count = members.len();
        if count == 0 {
            continue;
        }

        // A tier budgeted in pixels picks its quota from the member's mass, so
        // a continent of tiny coastal provinces is cut into fewer, larger areas
        // than the same head count of interior ones would be.
        let member_weight = weight.of_ids(provinces, &members);
        let k = ((member_weight as f32 / target_cluster_size as f32).round() as usize)
            .clamp(1, max_clusters_per_parent.min(count));

        let member_set: HashSet<u32> = members.iter().copied().collect();
        members.shuffle(&mut rng);

        let group_assignments = cluster_group(
            &members,
            &member_set,
            provinces,
            k,
            next_cid,
            &mut rng,
            map_width,
            weight,
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
///
/// A cluster of a few provinces is dissolved **province by province** into the
/// neighbours each province actually touches; only a lone province (or a pair)
/// is moved whole. Swallowing every undersized cluster into a single neighbour
/// is what used to leave a long annex hanging off the side of an otherwise
/// round area.
fn absorb_small_clusters(
    provinces: &[ProvinceNode],
    cluster_ids: &mut [u32],
    parent_ids: &[u32],
    min_size: usize,
    weight: ClusterWeight,
    map_width: f32,
    progress: &Progress,
    label: &'static str,
    ocean: bool,
) -> usize {
    let n = provinces.len();

    // The absorber runs until nothing is left below the floor, so its length is
    // not known in advance: a count, and a picture per sweep.
    progress.sub_step(label, 0);
    let mut sweeps = 0u64;

    loop {
        progress.tick(sweeps);
        preview_tier_of(progress, label, cluster_ids, ocean);
        sweeps += 1;

        let mut cluster_sizes: BTreeMap<u32, usize> = BTreeMap::new();
        let mut cluster_parent: BTreeMap<u32, u32> = BTreeMap::new();
        let mut parent_cluster_count: BTreeMap<u32, usize> = BTreeMap::new();

        for p in provinces.iter() {
            if p.id == 0 || p.pixel_count == 0 {
                continue;
            }
            let cid = cluster_ids[p.id as usize];
            if cid != 0 {
                *cluster_sizes.entry(cid).or_insert(0) += weight.of(p);
                let parent = parent_ids[p.id as usize];
                cluster_parent.insert(cid, parent);
            }
        }

        for (&_cid, &parent) in &cluster_parent {
            *parent_cluster_count.entry(parent).or_insert(0) += 1;
        }

        let mut small_clusters: Vec<u32> = cluster_sizes
            .iter()
            .filter(|&(&cid, &mass)| {
                mass < min_size
                    && parent_cluster_count
                        .get(&cluster_parent[&cid])
                        .copied()
                        .unwrap_or(0)
                        > 1
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

            if members.is_empty() {
                continue;
            }
            let current_parent = parent_ids[members[0] as usize];

            if parent_cluster_count
                .get(&current_parent)
                .copied()
                .unwrap_or(0)
                <= 1
            {
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

            let target_cid = if let Some((&best_cid, _)) =
                neighbor_scores.iter().max_by_key(|&(_, score)| score)
            {
                Some(best_cid)
            } else {
                let (mx, my) = (
                    provinces[members[0] as usize].center_x,
                    provinces[members[0] as usize].center_y,
                );
                let mut closest_cid = None;
                let mut min_d2 = f32::MAX;

                for p in provinces.iter() {
                    if p.id == 0 || p.pixel_count == 0 {
                        continue;
                    }
                    let other_cid = cluster_ids[p.id as usize];
                    if other_cid != 0
                        && other_cid != small_cid
                        && parent_ids[p.id as usize] == current_parent
                    {
                        let d2 = wrap_dist_sq(mx, my, p.center_x, p.center_y, map_width);
                        if d2 < min_d2 {
                            min_d2 = d2;
                            closest_cid = Some(other_cid);
                        }
                    }
                }
                closest_cid
            };

            // A larger undersized area is a shape, not a token: hand each of its
            // provinces to the neighbour it shares the most frontier with. The
            // relaxation pass that follows then rounds whatever seams that leaves.
            let dissolve = members.len() >= 3 && neighbor_scores.len() >= 2;

            let mut moved_any = false;
            for &m in &members {
                let destination = if dissolve {
                    let mut per_province: BTreeMap<u32, u32> = BTreeMap::new();
                    for &(nb, cost) in &provinces[m as usize].neighbors {
                        let nb_cid = cluster_ids[nb as usize];
                        if nb_cid != 0 && nb_cid != small_cid {
                            if parent_ids[nb as usize] == current_parent {
                                let score = 5000u32.saturating_sub(cost.min(4900));
                                *per_province.entry(nb_cid).or_insert(0) += score;
                            }
                        }
                    }
                    per_province
                        .iter()
                        .max_by_key(|&(_, score)| score)
                        .map(|(&cid, _)| cid)
                        .or(target_cid)
                } else {
                    target_cid
                };

                if let Some(dest_cid) = destination {
                    if dest_cid != small_cid {
                        cluster_ids[m as usize] = dest_cid;
                        moved_any = true;
                    }
                }
            }
            any_merged |= moved_any;
        }

        if !any_merged {
            break;
        }
    }

    // Re-index clusters to continuous 1..=total IDs
    reindex_clusters(cluster_ids)
}

/// Renumber every non-zero cluster id to a contiguous `1..=total` run.
///
/// Every pass that can delete a cluster (the absorber, the splitter, the island
/// unifier) ends here, so an id stays both a stable key inside a pass and a
/// compact index for the palette that reads it.
fn reindex_clusters(cluster_ids: &mut [u32]) -> usize {
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

/// How large a land Area should read on the plate, measured in province pixels.
///
/// The Area tier is the only one budgeted by drawn mass instead of head count:
/// a desert province and a coastal speck must not buy the same amount of map.
/// That irregular speckle is exactly what made the old head-counted Areas read
/// as confetti. A land province is ≈350 px today, so these come out at roughly
/// 13, 6 and 20 provinces respectively.
const LAND_AREA_TARGET_PIXELS: usize = 4_500;

/// Smallest land Area that may survive the absorb sweep.
const LAND_AREA_MIN_PIXELS: usize = 2_000;

/// Largest land Area the splitter will tolerate.
const LAND_AREA_MAX_PIXELS: usize = 7_000;

/// A second, province-count ceiling: a raft of tiny island specks must not pile
/// into one Area just because it happens to weigh as much as a desert.
const LAND_AREA_MAX_PROVINCES: usize = 24;

/// Most Areas a single Region may be cut into — a cluster cap, not a province cap.
const MAX_AREAS_PER_REGION: usize = 8;

/// How many tightening sweeps the shape pass may take. Three is ample: the first
/// does almost all of the work, the rest only settle the seams it creates.
const AREA_RELAX_SWEEPS: usize = 3;

/// The oceanic mirror of the old land ceiling.
///
/// An ocean province covers roughly 22× the area of a land province, so the
/// oceanic Area ceiling is scaled up by the same factor. Ocean provinces are
/// already of one size, which is why the marine Area plate never needed a
/// pixel budget.
const MAX_OCEAN_AREA_PROVINCES: usize = 25;

/// Bisect one cluster into two connected halves of nearly equal size.
///
/// The seeds are the farthest-apart pair of members, so the cut follows the
/// cluster's longest internal axis; each seed then claims neighbouring members
/// with a capacity-limited Dijkstra. The capacities are `len / 2` and
/// `len - len / 2` — they sum to the member count, so a connected cluster is
/// always fully claimed and neither half can swallow the other whole.
///
/// "Size" is measured by `weight`: in provinces for the upper tiers, in pixels
/// for the land Areas, where the cut must balance drawn map rather than heads.
///
/// Deliberately deterministic: no random numbers are drawn, ties in the
/// farthest-pair scan keep the first (id-ordered) pair, and the heap orders
/// equal-cost states by province id.
fn bisect_cluster(
    provinces: &[ProvinceNode],
    group: &[u32],
    map_width: f32,
    weight: ClusterWeight,
) -> (Vec<u32>, Vec<u32>) {
    let len = group.len();
    if len < 2 {
        return (group.to_vec(), Vec::new());
    }

    let mut seed_a = group[0];
    let mut seed_b = group[0];
    let mut farthest = -1.0f32;

    for (index, &a) in group.iter().enumerate() {
        let (ax, ay) = (
            provinces[a as usize].center_x,
            provinces[a as usize].center_y,
        );
        for &b in &group[index + 1..] {
            let (bx, by) = (
                provinces[b as usize].center_x,
                provinces[b as usize].center_y,
            );
            let d = wrap_dist_sq(ax, ay, bx, by, map_width);
            if d > farthest {
                farthest = d;
                seed_a = a;
                seed_b = b;
            }
        }
    }

    // Every centre identical (a synthetic test, or a degenerate island): the
    // farthest-pair scan found no pair at all. Keep the halves distinct anyway.
    if seed_a == seed_b {
        seed_b = group[1];
    }

    let member_set: HashSet<u32> = group.iter().copied().collect();
    let total_weight = weight.of_ids(provinces, group).max(2);
    let caps = [
        (total_weight / 2).max(1),
        (total_weight - total_weight / 2).max(1),
    ];

    let mut side_of: BTreeMap<u32, usize> = BTreeMap::new();
    let mut side_len = [0usize; 2];
    let mut heap: BinaryHeap<DijkState> = BinaryHeap::new();

    for (side, &seed) in [seed_a, seed_b].iter().enumerate() {
        side_of.insert(seed, side);
        side_len[side] += weight.of(&provinces[seed as usize]);
        heap.push(DijkState {
            cost: 0,
            id: seed,
            cid: side as u32,
        });
    }

    while let Some(state) = heap.pop() {
        let side = state.cid as usize;

        // Only expand from our own frontier, and never past this half's capacity.
        if side_of.get(&state.id) != Some(&side) || side_len[side] >= caps[side] {
            continue;
        }

        for &(nb, edge_cost) in &provinces[state.id as usize].neighbors {
            if !member_set.contains(&nb) || side_of.contains_key(&nb) {
                continue;
            }
            side_of.insert(nb, side);
            side_len[side] += weight.of(&provinces[nb as usize]);
            heap.push(DijkState {
                cost: state.cost.saturating_add(edge_cost),
                id: nb,
                cid: side as u32,
            });
            if side_len[side] >= caps[side] {
                break;
            }
        }
    }

    // A disconnected cluster can leave members unreached by either seed: park
    // them on whichever half still has room and lies closer.
    for &m in group {
        if side_of.contains_key(&m) {
            continue;
        }
        let (mx, my) = (
            provinces[m as usize].center_x,
            provinces[m as usize].center_y,
        );
        let mut best_side = if side_len[0] <= side_len[1] { 0 } else { 1 };
        let mut best_d = f32::MAX;

        for side in 0..2 {
            if side_len[side] >= caps[side] {
                continue;
            }
            let mut d_min = f32::MAX;
            for (&other, &other_side) in &side_of {
                if other_side != side {
                    continue;
                }
                let (ox, oy) = (
                    provinces[other as usize].center_x,
                    provinces[other as usize].center_y,
                );
                let d = wrap_dist_sq(mx, my, ox, oy, map_width);
                if d < d_min {
                    d_min = d;
                }
            }
            if d_min < best_d {
                best_d = d_min;
                best_side = side;
            }
        }

        side_of.insert(m, best_side);
        side_len[best_side] += weight.of(&provinces[m as usize]);
    }

    let mut first = Vec::with_capacity(len);
    let mut second = Vec::with_capacity(len);
    for &m in group {
        match side_of.get(&m).copied().unwrap_or(0usize) {
            0 => first.push(m),
            _ => second.push(m),
        }
    }

    (first, second)
}

/// Split any cluster larger than `max_size` into balanced, connected pieces.
///
/// The clustering pass only bounds how many clusters a parent holds, and every
/// other post-pass adds provinces to clusters rather than taking them away, so
/// nothing else enforces a ceiling. This pass restores it: an over-full cluster
/// is bisected ([`bisect_cluster`]) and the halves are split again until every
/// cluster fits.
///
/// A bisection always yields halves of `len / 2` or more, so splitting can never
/// produce the single-province clusters the absorber exists to remove, and both
/// halves stay inside the parent they already shared. The pass is deterministic —
/// it draws no random numbers and iterates a `BTreeMap` with id tie-breaks — so a
/// given world splits identically on every run.
///
/// Returns the number of clusters in the tier. When nothing is over the ceiling
/// the assignments are left exactly as the clustering made them, ids included.
fn split_oversized_clusters(
    provinces: &[ProvinceNode],
    cluster_ids: &mut [u32],
    parent_ids: &[u32],
    max_size: usize,
    weight: ClusterWeight,
    map_width: f32,
    progress: &Progress,
    label: &'static str,
    ocean: bool,
) -> usize {
    // A ceiling below the one-province floor is unsatisfiable.
    if max_size < 2 {
        return cluster_ids
            .iter()
            .filter(|&&id| id != 0)
            .copied()
            .collect::<HashSet<_>>()
            .len();
    }

    // Is there anything to do? If not, leave the tier untouched — reassigning
    // ids would churn the atlas for no gain.
    let mut sizes: BTreeMap<u32, usize> = BTreeMap::new();
    for p in provinces.iter() {
        if p.id == 0 || p.pixel_count == 0 {
            continue;
        }
        let index = p.id as usize;
        if index >= cluster_ids.len() || index >= parent_ids.len() {
            continue;
        }
        let cid = cluster_ids[index];
        if cid == 0 || parent_ids[index] == 0 {
            continue;
        }
        *sizes.entry(cid).or_insert(0) += weight.of(p);
    }
    if !sizes.values().any(|&size| size > max_size) {
        return sizes.len();
    }

    // The splitter runs until nothing is left over the ceiling, so its length is
    // not known in advance: a count, and a picture per sweep.
    progress.sub_step(label, 0);
    let mut sweeps = 0u64;
    let mut next_cid = cluster_ids.iter().copied().max().unwrap_or(0) + 1;

    loop {
        progress.tick(sweeps);
        preview_tier_of(progress, label, cluster_ids, ocean);
        sweeps += 1;

        let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for p in provinces.iter() {
            if p.id == 0 || p.pixel_count == 0 {
                continue;
            }
            let index = p.id as usize;
            if index >= cluster_ids.len() || index >= parent_ids.len() {
                continue;
            }
            let cid = cluster_ids[index];
            if cid == 0 || parent_ids[index] == 0 {
                continue;
            }
            members.entry(cid).or_default().push(p.id);
        }

        let mut oversized: Option<Vec<u32>> = None;
        for group in members.values() {
            if weight.of_ids(provinces, group) > max_size {
                oversized = Some(group.clone());
                break;
            }
        }
        let Some(group) = oversized else {
            break;
        };

        // Both halves keep the parent the cluster already had, so a split can
        // never cross the Region boundary.
        let (keep, split) = bisect_cluster(provinces, &group, map_width, weight);
        if split.is_empty() {
            break;
        }
        debug_assert!(
            !keep.is_empty(),
            "bisecting {} members kept nothing",
            group.len()
        );

        for member in split {
            cluster_ids[member as usize] = next_cid;
        }
        next_cid += 1;
    }

    // Re-index clusters to continuous 1..=total IDs, exactly as the absorber does.
    reindex_clusters(cluster_ids)
}

/// Give every small island landmass its own single Area.
///
/// The province graph already anchors each island landmass to one mainland
/// coastal province, and every province of that landmass carries the same
/// `anchor_id` (`graph.rs`). That is the island key used here: a landmass whose
/// whole mass fits inside one Area must not be cut across several of them, or a
/// handful of rocks shows up as a handful of unrelated colours and an
/// archipelago reads as confetti.
///
/// Returns one flag per province: the provinces this pass moved are frozen, so
/// the relaxation that follows can round the areas around an island without
/// pulling the island apart again.
fn unify_island_areas(
    provinces: &[ProvinceNode],
    cluster_ids: &mut [u32],
    parent_ids: &[u32],
    weight: ClusterWeight,
    max_size: usize,
) -> Vec<bool> {
    let n = provinces.len();
    let mut locked = vec![false; n];

    // (anchor, parent) -> the whole landmass's provinces. Grouping by parent as
    // well as anchor keeps every unification inside the Region that holds it.
    let mut islands: BTreeMap<(u32, u32), Vec<u32>> = BTreeMap::new();
    for p in provinces.iter() {
        if p.id == 0 || p.pixel_count == 0 {
            continue;
        }
        let index = p.id as usize;
        if index >= cluster_ids.len() || index >= parent_ids.len() {
            continue;
        }
        if p.anchor_id == 0 || p.anchor_id == p.id {
            continue;
        } // mainland province
        if cluster_ids[index] == 0 || parent_ids[index] == 0 {
            continue;
        }
        islands
            .entry((p.anchor_id, parent_ids[index]))
            .or_default()
            .push(p.id);
    }

    for (_island, members) in islands {
        let mass = weight.of_ids(provinces, &members);
        if mass == 0 || mass > max_size {
            continue; // a landmass that legitimately holds several Areas
        }

        // The area holding the most of the island wins; the lowest id breaks a tie.
        let mut held: BTreeMap<u32, usize> = BTreeMap::new();
        for &m in &members {
            *held.entry(cluster_ids[m as usize]).or_insert(0) += weight.of(&provinces[m as usize]);
        }
        let Some((&home, _)) = held
            .iter()
            .max_by_key(|&(&cid, &mass)| (mass, std::cmp::Reverse(cid)))
        else {
            continue;
        };

        for &m in &members {
            cluster_ids[m as usize] = home;
            locked[m as usize] = true;
        }
    }

    locked
}

/// Round the Areas by moving boundary provinces between neighbours.
///
/// The clustering balances mass; it does not care about shape. This pass does.
/// A boundary province is handed to an adjacent Area whenever that lowers the
/// tier's total `perimeter² / mass` — the same quantity a cartographer means by
/// "a compact shape" — which is what removes the lobes, necks and dangling
/// single-province protrusions a capacity-balanced flood fill leaves behind.
///
/// A province may only move to an Area that touches it and shares its Region,
/// both Areas must stay inside their size bounds, the donor must remain one
/// piece, and anything an island was unified into stays frozen. Deterministic:
/// sweeps run in province-id order, each province takes the best improvement it
/// can see, and ties keep the lowest destination id.
fn relax_area_boundaries(
    provinces: &[ProvinceNode],
    cluster_ids: &mut [u32],
    parent_ids: &[u32],
    weight: ClusterWeight,
    min_size: usize,
    max_size: usize,
    locked: &[bool],
    max_sweeps: usize,
) -> usize {
    let n = provinces.len();
    if n < 2 {
        return 0;
    }

    let mut mass: BTreeMap<u32, usize> = BTreeMap::new();
    let mut members: BTreeMap<u32, usize> = BTreeMap::new();
    let mut perimeter: BTreeMap<u32, usize> = BTreeMap::new();

    for p in provinces.iter() {
        if p.id == 0 || p.pixel_count == 0 {
            continue;
        }
        let index = p.id as usize;
        if index >= cluster_ids.len() {
            continue;
        }
        let cid = cluster_ids[index];
        if cid == 0 {
            continue;
        }
        *mass.entry(cid).or_insert(0) += weight.of(p);
        *members.entry(cid).or_insert(0) += 1;
    }

    // One frontier edge per adjacent province pair whose Areas differ. The pair
    // is counted once (`nb > id`) and charged to both sides, because it is one
    // unit of boundary in each area's own perimeter.
    for p in provinces.iter() {
        if p.id == 0 || p.pixel_count == 0 {
            continue;
        }
        let index = p.id as usize;
        if index >= cluster_ids.len() {
            continue;
        }
        let cid = cluster_ids[index];
        if cid == 0 {
            continue;
        }
        for &(nb, _) in &p.neighbors {
            if nb <= p.id || (nb as usize) >= cluster_ids.len() {
                continue;
            }
            let n_cid = cluster_ids[nb as usize];
            if n_cid == 0 || n_cid == cid {
                continue;
            }
            *perimeter.entry(cid).or_insert(0) += 1;
            *perimeter.entry(n_cid).or_insert(0) += 1;
        }
    }

    let order: Vec<u32> = provinces
        .iter()
        .filter(|p| p.id != 0 && p.pixel_count > 0)
        .map(|p| p.id)
        .collect();

    for _ in 0..max_sweeps {
        let mut moved = false;

        for &pid in &order {
            let index = pid as usize;
            if index >= cluster_ids.len() || locked.get(index).copied().unwrap_or(false) {
                continue;
            }
            let own = cluster_ids[index];
            if own == 0 {
                continue;
            }
            let own_parent = parent_ids.get(index).copied().unwrap_or(0);
            if own_parent == 0 {
                continue;
            }

            // How much frontier this province shares with each neighbouring Area.
            let mut touch: BTreeMap<u32, usize> = BTreeMap::new();
            let mut deg_own = 0usize;
            let mut deg_total = 0usize;
            for &(nb, _) in &provinces[index].neighbors {
                let n_index = nb as usize;
                if n_index >= cluster_ids.len()
                    || parent_ids.get(n_index).copied().unwrap_or(0) != own_parent
                {
                    continue;
                }
                let n_cid = cluster_ids[n_index];
                if n_cid == 0 {
                    continue;
                }
                if n_cid == own {
                    deg_own += 1;
                } else {
                    *touch.entry(n_cid).or_insert(0) += 1;
                }
                deg_total += 1;
            }
            if touch.is_empty() || deg_total == 0 {
                continue; // interior province, or nothing to move to
            }

            let w = weight.of(&provinces[index]);
            let mass_own = mass.get(&own).copied().unwrap_or(0);
            if mass_own < min_size + w {
                continue; // the donor would fall under the floor
            }

            let p_own = perimeter.get(&own).copied().unwrap_or(0) as f64;
            let energy_own = p_own * p_own / mass_own as f64;

            let mut best: Option<(u32, f64)> = None;
            for (&dest, &deg_dest) in &touch {
                let mass_dest = mass.get(&dest).copied().unwrap_or(0);
                if mass_dest + w > max_size {
                    continue;
                }
                let p_dest = perimeter.get(&dest).copied().unwrap_or(0) as f64;

                let delta_own = 2.0 * deg_own as f64 - deg_total as f64;
                let delta_dest = deg_total as f64 - 2.0 * deg_dest as f64;
                let energy_after_own = (p_own + delta_own).max(0.0).powi(2) / (mass_own - w) as f64;
                let energy_after_dest =
                    (p_dest + delta_dest).max(0.0).powi(2) / (mass_dest + w) as f64;
                let energy_before_dest = p_dest * p_dest / mass_dest.max(1) as f64;

                let improvement =
                    energy_own + energy_before_dest - energy_after_own - energy_after_dest;
                if improvement <= 1e-9 {
                    continue;
                }
                // Taking a province out of the middle of an area can cut it in
                // two; a leaf cannot. Only the former needs the walk.
                if deg_own >= 2
                    && !donor_stays_connected(
                        provinces,
                        cluster_ids,
                        own,
                        pid,
                        members.get(&own).copied().unwrap_or(0),
                    )
                {
                    continue;
                }

                let take = match best {
                    None => true,
                    Some((best_cid, best_gain)) => {
                        improvement > best_gain + 1e-9
                            || ((improvement - best_gain).abs() <= 1e-9 && dest < best_cid)
                    }
                };
                if take {
                    best = Some((dest, improvement));
                }
            }

            let Some((dest, _)) = best else { continue };

            let deg_dest = touch[&dest] as isize;
            let p_own = perimeter.get(&own).copied().unwrap_or(0) as isize;
            let p_dest = perimeter.get(&dest).copied().unwrap_or(0) as isize;
            let delta_own = 2 * deg_own as isize - deg_total as isize;
            let delta_dest = deg_total as isize - 2 * deg_dest;

            perimeter.insert(own, (p_own + delta_own).max(0) as usize);
            perimeter.insert(dest, (p_dest + delta_dest).max(0) as usize);
            mass.insert(own, mass_own - w);
            *mass.entry(dest).or_insert(0) += w;
            *members.entry(dest).or_insert(0) += 1;

            let donor_members = members.get(&own).copied().unwrap_or(1).saturating_sub(1);
            members.insert(own, donor_members);
            if donor_members == 0 {
                members.remove(&own);
                mass.remove(&own);
                perimeter.remove(&own);
            }

            cluster_ids[index] = dest;
            moved = true;
        }

        if !moved {
            break;
        }
    }

    members.len()
}

/// Would the donor Area still be one piece without `removed`?
///
/// Only called for a province with at least two neighbours inside its own Area —
/// a leaf cannot disconnect anything. A bounded flood fill over a cluster of a
/// few provinces is cheaper than any cleverer test.
fn donor_stays_connected(
    provinces: &[ProvinceNode],
    cluster_ids: &[u32],
    own: u32,
    removed: u32,
    member_count: usize,
) -> bool {
    let start = provinces[removed as usize]
        .neighbors
        .iter()
        .map(|&(nb, _)| nb)
        .find(|&nb| {
            nb != removed && (nb as usize) < cluster_ids.len() && cluster_ids[nb as usize] == own
        });
    let Some(start) = start else {
        return member_count <= 1;
    };

    let mut seen: HashSet<u32> = HashSet::new();
    let mut stack = vec![start];
    seen.insert(start);
    while let Some(id) = stack.pop() {
        for &(nb, _) in &provinces[id as usize].neighbors {
            if nb == removed {
                continue;
            }
            let index = nb as usize;
            if index >= cluster_ids.len() || cluster_ids[index] != own {
                continue;
            }
            if seen.insert(nb) {
                stack.push(nb);
            }
        }
    }

    // Everyone but the province about to leave must still be reachable.
    seen.len() + 1 >= member_count
}

/// Mean number of frontier edges an area of the tier carries.
///
/// A pair of neighbouring provinces in two different areas is one frontier edge,
/// charged to each side — the simple graph measure of how ragged the tier is.
/// The shape pass exists to push this number down without shrinking any area.
fn mean_frontier_edges(provinces: &[ProvinceNode], cluster_ids: &[u32]) -> f64 {
    let mut frontier = 0usize;
    let mut areas: BTreeSet<u32> = BTreeSet::new();
    for p in provinces.iter().skip(1) {
        let id = p.id as usize;
        if id >= cluster_ids.len() || p.pixel_count == 0 {
            continue;
        }
        let cid = cluster_ids[id];
        if cid == 0 {
            continue;
        }
        areas.insert(cid);
        for &(nb, _) in &p.neighbors {
            let n = nb as usize;
            if n <= id || n >= cluster_ids.len() {
                continue;
            }
            let n_cid = cluster_ids[n];
            if n_cid != 0 && n_cid != cid {
                frontier += 1;
            }
        }
    }
    if areas.is_empty() {
        0.0
    } else {
        2.0 * frontier as f64 / areas.len() as f64
    }
}

/// Build the complete four-level geopolitical hierarchy:
///
/// ```
/// Continent  (Major landmass cores + strictly anchored islands)
///   └─ Superregion  (~200 provinces at 16K, scaled by canvas area)
///        └─ Region  (~45 provinces at 16K, scaled with it)
///             └─ Area  (budgeted by drawn pixel mass, island chains united)
/// ```
///
/// The superregion and region targets are quoted at the 16K reference plate and
/// scaled with the province count, so a 2K world's political map keeps the same
/// tier structure at a coarser grain.
pub fn build_geopolitical_hierarchy(
    mut provinces: Vec<ProvinceNode>,
    seed: u64,
    map_width: f32,
    scale: CanvasScale,
    progress: &Progress,
) -> GeopoliticalHierarchy {
    let n = provinces.len();
    // The tier targets are province *counts*, tuned on the 16K reference plate.
    // A canvas's province count is scaled by its area (see `GeographyConfig`), so
    // the same factor carries these targets: a two-hundred-province superregion
    // at 16K covers the same share of the world as an eight-province one at 2K.
    // The floors keep every tier present on the smallest plate.
    let superregion_target = scale.count_at_least(200, 8);
    let superregion_absorb = scale.count_at_least(95, 2);
    let region_target = scale.count_at_least(45, 4);
    let region_absorb = scale.count_at_least(16, 1);

    // ── 1. Continents ─────────────────────────────────────────────────────────
    progress.sub_step("Anchoring the continents", 0);
    let continent_ids: Vec<u32> = provinces.iter().map(|p| p.continent_id).collect();
    let continent_count = continent_ids
        .iter()
        .filter(|&&c| c != 0)
        .copied()
        .collect::<HashSet<_>>()
        .len();
    println!(
        "✓ Identified {} Continent(s) (All islands anchored to their nearest continent).",
        continent_count
    );

    // ── 2. Superregions (Balanced realms: ~200 provinces) ─────────────────────
    println!("  Clustering Superregions (Strictly inside Continents)…");
    let (mut super_ids, _) = cluster_tier(
        &provinces,
        &continent_ids,
        superregion_target,
        50, // At most 50 superregions per large continent
        seed.wrapping_add(3),
        map_width,
        progress,
        "Clustering superregions",
        false,
        ClusterWeight::Count,
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
        superregion_absorb,
        ClusterWeight::Count,
        map_width,
        progress,
        "Absorbing small superregions",
        false,
    );

    for i in 1..n {
        provinces[i].superregion_id = super_ids[i];
    }
    println!(
        "  ✓ Generated {} Superregion(s) worldwide (undersized absorbed).",
        total_superregions
    );

    // ── 3. Regions (Kingdom level: ~35 provinces) ─────────────────────────────
    println!("  Clustering Regions (Strictly inside Superregions)…");
    let (mut region_ids, _) = cluster_tier(
        &provinces,
        &super_ids,
        region_target,
        10, // Up to 10 regions per superregion
        seed.wrapping_add(2),
        map_width,
        progress,
        "Clustering regions",
        false,
        ClusterWeight::Count,
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
        region_absorb,
        ClusterWeight::Count,
        map_width,
        progress,
        "Absorbing small regions",
        false,
    );

    for i in 1..n {
        provinces[i].region_id = region_ids[i];
    }
    println!(
        "  ✓ Generated {} Region(s) worldwide (undersized absorbed).",
        total_regions
    );

    // ── 4. Areas (the fine political grain, budgeted by drawn mass) ───────────
    println!("  Clustering Areas (Strictly inside Regions)…");
    let (mut area_ids, _) = cluster_tier(
        &provinces,
        &region_ids,
        LAND_AREA_TARGET_PIXELS,
        MAX_AREAS_PER_REGION,
        seed.wrapping_add(1),
        map_width,
        progress,
        "Clustering areas",
        false,
        ClusterWeight::Pixels,
    );

    // Absorb areas whose drawn mass is under one readable unit. Provinces are
    // handed to the neighbours they touch, so no neighbour grows an annex.
    let _ = absorb_small_clusters(
        &provinces,
        &mut area_ids,
        &region_ids,
        LAND_AREA_MIN_PIXELS,
        ClusterWeight::Pixels,
        map_width,
        progress,
        "Absorbing small areas",
        false,
    );

    // Enforce both ceilings. Clustering only bounds how many areas a Region
    // holds, and the absorb sweep grows existing areas, so an over-full Area
    // must be split: first by mass, then by head count for the island rafts.
    let _ = split_oversized_clusters(
        &provinces,
        &mut area_ids,
        &region_ids,
        LAND_AREA_MAX_PIXELS,
        ClusterWeight::Pixels,
        map_width,
        progress,
        "Splitting oversized areas",
        false,
    );
    let _ = split_oversized_clusters(
        &provinces,
        &mut area_ids,
        &region_ids,
        LAND_AREA_MAX_PROVINCES,
        ClusterWeight::Count,
        map_width,
        progress,
        "Splitting crowded areas",
        false,
    );

    // A small island is one place, so it is one Area; then round whatever the
    // clustering left lopsided. Island provinces are frozen against the relax.
    let locked = unify_island_areas(
        &provinces,
        &mut area_ids,
        &region_ids,
        ClusterWeight::Pixels,
        LAND_AREA_MAX_PIXELS,
    );
    let frontier_before = mean_frontier_edges(&provinces, &area_ids);
    relax_area_boundaries(
        &provinces,
        &mut area_ids,
        &region_ids,
        ClusterWeight::Pixels,
        LAND_AREA_MIN_PIXELS,
        LAND_AREA_MAX_PIXELS,
        &locked,
        AREA_RELAX_SWEEPS,
    );
    let frontier_after = mean_frontier_edges(&provinces, &area_ids);

    // The island unifier can delete a cluster id; the shape pass cannot.
    let total_areas = reindex_clusters(&mut area_ids);

    for i in 1..n {
        provinces[i].area_id = area_ids[i];
    }

    // A one-line census of the tier, so its shape can be judged from the console
    // the same way relief and climate already are.
    let mut area_members: BTreeMap<u32, usize> = BTreeMap::new();
    for i in 1..n {
        if area_ids[i] != 0 {
            *area_members.entry(area_ids[i]).or_insert(0) += 1;
        }
    }
    let mut sizes: Vec<usize> = area_members.values().copied().collect();
    sizes.sort_unstable();
    let smallest = sizes.first().copied().unwrap_or(0);
    let largest = sizes.last().copied().unwrap_or(0);
    let median = sizes.get(sizes.len() / 2).copied().unwrap_or(0);
    println!(
        "  ✓ Generated {} Area(s) worldwide — {}–{} provinces (median {}), frontier edges each {:.1} → {:.1}.",
        total_areas, smallest, largest, median, frontier_before, frontier_after
    );

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
    seed: u64,
    map_width: f32,
    config: &OceanConfig,
    progress: &Progress,
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
    let (continent_ids, continent_count) = cluster_tier(
        &provinces,
        &roots,
        config.continent_target,
        8, // at most 8 ocean basins
        seed.wrapping_add(4),
        map_width,
        progress,
        "Clustering ocean basins",
        true,
        ClusterWeight::Count,
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
        progress,
        "Clustering oceanic superregions",
        true,
        ClusterWeight::Count,
    );
    // Undersized superregions are absorbed so no tiny sliver survives between
    // two large ocean realms.
    let total_superregions = absorb_small_clusters(
        &provinces,
        &mut super_ids,
        &continent_ids,
        (config.superregion_target / 5).max(6),
        ClusterWeight::Count,
        map_width,
        progress,
        "Absorbing small ocean superregions",
        true,
    );
    for i in 1..n {
        provinces[i].superregion_id = super_ids[i];
    }
    println!(
        "  ✓ Clustered {} Oceanic Superregion(s) (undersized absorbed).",
        total_superregions
    );

    // ── 3. Oceanic Regions (Strictly inside Superregions) ─────────────────────
    let (mut region_ids, _) = cluster_tier(
        &provinces,
        &super_ids,
        config.region_target,
        40,
        seed.wrapping_add(2),
        map_width,
        progress,
        "Clustering oceanic regions",
        true,
        ClusterWeight::Count,
    );
    let total_regions = absorb_small_clusters(
        &provinces,
        &mut region_ids,
        &super_ids,
        (config.region_target / 5).max(4),
        ClusterWeight::Count,
        map_width,
        progress,
        "Absorbing small ocean regions",
        true,
    );
    for i in 1..n {
        provinces[i].region_id = region_ids[i];
    }
    println!(
        "  ✓ Clustered {} Oceanic Region(s) (undersized absorbed).",
        total_regions
    );

    // ── 4. Oceanic Areas (Strictly inside Regions) ────────────────────────────
    let (mut area_ids, _) = cluster_tier(
        &provinces,
        &region_ids,
        config.area_target,
        25, // at most 25 oceanic areas per region (a cluster cap)
        seed.wrapping_add(1),
        map_width,
        progress,
        "Clustering oceanic areas",
        true,
        ClusterWeight::Count,
    );
    let _ = absorb_small_clusters(
        &provinces,
        &mut area_ids,
        &region_ids,
        2,
        ClusterWeight::Count,
        map_width,
        progress,
        "Absorbing small ocean areas",
        true,
    );
    let total_areas = split_oversized_clusters(
        &provinces,
        &mut area_ids,
        &region_ids,
        MAX_OCEAN_AREA_PROVINCES,
        ClusterWeight::Count,
        map_width,
        progress,
        "Splitting oversized ocean areas",
        true,
    );
    for i in 1..n {
        provinces[i].area_id = area_ids[i];
    }
    println!(
        "  ✓ Clustered {} Oceanic Area(s) (undersized absorbed).",
        total_areas
    );

    GeopoliticalHierarchy {
        total_provinces: provinces.iter().filter(|p| p.pixel_count > 0).count(),
        total_areas,
        total_regions,
        total_superregions,
        total_continents: continent_count,
        provinces,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A chain of `count` provinces, all in one region and one area, each
    /// adjacent to its neighbours — the shape a single oversized area takes.
    /// Index 0 is the empty sentinel the graph extractors always keep.
    fn chain(count: u32) -> Vec<ProvinceNode> {
        let mut provinces = vec![ProvinceNode {
            id: 0,
            center_x: 0.0,
            center_y: 0.0,
            pixel_count: 0,
            neighbors: Vec::new(),
            area_id: 0,
            region_id: 0,
            superregion_id: 0,
            continent_id: 0,
            anchor_id: 0,
        }];
        for id in 1..=count {
            let mut neighbors = Vec::new();
            if id > 1 {
                neighbors.push((id - 1, 1));
            }
            if id < count {
                neighbors.push((id + 1, 1));
            }
            provinces.push(ProvinceNode {
                id,
                center_x: id as f32,
                center_y: 0.0,
                pixel_count: 10,
                neighbors,
                area_id: 1,
                region_id: 1,
                superregion_id: 1,
                continent_id: 1,
                anchor_id: 1,
            });
        }
        provinces
    }

    fn parents_of(provinces: &[ProvinceNode]) -> Vec<u32> {
        (0..provinces.len()).map(|i| u32::from(i != 0)).collect()
    }

    fn areas_of(ids: &[u32]) -> BTreeMap<u32, Vec<u32>> {
        let mut areas: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (index, &id) in ids.iter().enumerate().skip(1) {
            areas.entry(id).or_default().push(index as u32);
        }
        areas
    }

    /// The splitter exists because the clustering caps areas per region, not
    /// provinces per area: an over-full area must be bisected until every piece
    /// fits, and every piece must stay a connected run of the province chain.
    #[test]
    fn an_oversized_area_is_split_within_the_ceiling() {
        let provinces = chain(12);
        let parents = parents_of(&provinces);
        let mut ids = vec![0u32; provinces.len()];
        for p in provinces.iter().skip(1) {
            ids[p.id as usize] = 1;
        }

        // A ceiling of five cannot hold twelve provinces in two pieces.
        let ceiling = 5;
        let total = split_oversized_clusters(
            &provinces,
            &mut ids,
            &parents,
            ceiling,
            ClusterWeight::Count,
            1_000.0,
            &Progress::silent(),
            "Splitting oversized areas",
            false,
        );

        assert!(
            total >= 3,
            "12 provinces cannot fit in fewer than 3 areas of {ceiling}"
        );

        let areas = areas_of(&ids);
        assert_eq!(
            areas.len(),
            total,
            "the returned count matches the distinct area ids"
        );

        for (&area, members) in &areas {
            assert!(
                members.len() <= ceiling,
                "area {area} holds {} provinces, over the ceiling",
                members.len()
            );
            assert!(
                members.len() >= 2,
                "area {area} is a single province — a split must never strand one"
            );

            let mut sorted = members.clone();
            sorted.sort_unstable();
            for pair in sorted.windows(2) {
                assert_eq!(
                    pair[1],
                    pair[0] + 1,
                    "area {area} fell into disconnected pieces"
                );
            }
        }

        let mut seen: Vec<u32> = ids.iter().copied().filter(|&id| id != 0).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen,
            (1..=total as u32).collect::<Vec<_>>(),
            "area ids stay contiguous from one"
        );
    }

    /// A tier already inside the ceiling is left exactly as the clustering made
    /// it — not even re-numbered.
    #[test]
    fn an_area_within_the_ceiling_is_left_untouched() {
        let provinces = chain(4);
        let parents = parents_of(&provinces);
        let mut ids = vec![0u32; provinces.len()];
        for p in provinces.iter().skip(1) {
            ids[p.id as usize] = 7;
        }

        let total = split_oversized_clusters(
            &provinces,
            &mut ids,
            &parents,
            8,
            ClusterWeight::Count,
            1_000.0,
            &Progress::silent(),
            "Splitting oversized areas",
            false,
        );

        assert_eq!(total, 1);
        assert!(
            ids.iter().skip(1).all(|&id| id == 7),
            "a compliant tier keeps its original ids"
        );
    }

    /// Splitting is a pure function of the cluster shape: same world in, same
    /// areas out, run after run.
    #[test]
    fn splitting_is_deterministic() {
        let provinces = chain(17);
        let parents = parents_of(&provinces);
        let mut first = vec![0u32; provinces.len()];
        let mut second = vec![0u32; provinces.len()];
        for p in provinces.iter().skip(1) {
            first[p.id as usize] = 1;
            second[p.id as usize] = 1;
        }

        let a = split_oversized_clusters(
            &provinces,
            &mut first,
            &parents,
            8,
            ClusterWeight::Count,
            10_000.0,
            &Progress::silent(),
            "Splitting oversized areas",
            false,
        );
        let b = split_oversized_clusters(
            &provinces,
            &mut second,
            &parents,
            8,
            ClusterWeight::Count,
            10_000.0,
            &Progress::silent(),
            "Splitting oversized areas",
            false,
        );

        assert_eq!(a, b);
        assert_eq!(first, second);
    }

    /// A square continent, every province adjacent to its four neighbours and
    /// anchored to itself — enough for the real builder to cut continents,
    /// superregions, regions and areas without any islands to anchor.
    fn continent_grid(width: u32, height: u32) -> Vec<ProvinceNode> {
        let mut provinces = vec![ProvinceNode {
            id: 0,
            center_x: 0.0,
            center_y: 0.0,
            pixel_count: 0,
            neighbors: Vec::new(),
            area_id: 0,
            region_id: 0,
            superregion_id: 0,
            continent_id: 0,
            anchor_id: 0,
        }];

        for row in 0..height {
            for col in 0..width {
                let id = row * width + col + 1;
                let mut neighbors = Vec::new();
                if col > 0 {
                    neighbors.push((id - 1, 1));
                }
                if col + 1 < width {
                    neighbors.push((id + 1, 1));
                }
                if row > 0 {
                    neighbors.push((id - width, 1));
                }
                if row + 1 < height {
                    neighbors.push((id + width, 1));
                }

                provinces.push(ProvinceNode {
                    id,
                    center_x: col as f32,
                    center_y: row as f32,
                    pixel_count: 300,
                    neighbors,
                    area_id: 0,
                    region_id: 0,
                    superregion_id: 0,
                    continent_id: 1,
                    anchor_id: id,
                });
            }
        }

        provinces
    }

    /// The whole land builder — clustering, absorb sweep, split, island
    /// unification and shape relaxation — must leave nothing over the Area
    /// ceiling, and nothing split across two Regions.
    #[test]
    fn the_built_hierarchy_never_exceeds_the_area_ceiling() {
        let hierarchy = build_geopolitical_hierarchy(
            continent_grid(20, 20),
            42,
            100.0,
            CanvasScale::reference(),
            &Progress::silent(),
        );

        let mut areas: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for p in hierarchy.provinces.iter().filter(|p| p.id != 0) {
            areas.entry(p.area_id).or_default().push(p.id);
        }

        assert!(!areas.is_empty(), "the builder must produce areas");
        assert_eq!(
            areas.len(),
            hierarchy.total_areas,
            "the reported count matches the ids"
        );

        for (&area, members) in &areas {
            assert_ne!(area, 0, "every province got an area");
            // The grid's provinces are all mainland, so nothing here is exempted
            // from the ceilings by the island unifier.
            assert!(
                members.len() <= LAND_AREA_MAX_PROVINCES,
                "area {area} holds {} provinces, over the ceiling",
                members.len()
            );

            let mass: usize = members
                .iter()
                .map(|&m| hierarchy.provinces[m as usize].pixel_count)
                .sum();
            assert!(
                mass <= LAND_AREA_MAX_PIXELS,
                "area {area} weighs {mass} px, over the ceiling"
            );

            let region = hierarchy.provinces[members[0] as usize].region_id;
            assert!(
                members
                    .iter()
                    .all(|&m| hierarchy.provinces[m as usize].region_id == region),
                "area {area} straddles two regions"
            );
        }
    }

    /// Total frontier edges across a tier, charged once to each side of every
    /// adjacent province pair — the perimeter the shape pass reasons about.
    fn frontier_edges(provinces: &[ProvinceNode], ids: &[u32]) -> usize {
        let mut total = 0;
        for p in provinces.iter().skip(1) {
            let cid = ids[p.id as usize];
            if cid == 0 {
                continue;
            }
            for &(nb, _) in &p.neighbors {
                let n = nb as usize;
                if n > p.id as usize && n < ids.len() && ids[n] != 0 && ids[n] != cid {
                    total += 1;
                }
            }
        }
        total
    }

    /// `Σ perimeter² / mass`, the compactness energy the shape pass minimises.
    fn area_objective(provinces: &[ProvinceNode], ids: &[u32], weight: ClusterWeight) -> f64 {
        let mut mass: BTreeMap<u32, usize> = BTreeMap::new();
        let mut perimeter: BTreeMap<u32, usize> = BTreeMap::new();
        for p in provinces.iter().skip(1) {
            let cid = ids[p.id as usize];
            if cid == 0 {
                continue;
            }
            *mass.entry(cid).or_insert(0) += weight.of(p);
            for &(nb, _) in &p.neighbors {
                let n = nb as usize;
                if n <= p.id as usize || n >= ids.len() {
                    continue;
                }
                let n_cid = ids[n];
                if n_cid != 0 && n_cid != cid {
                    *perimeter.entry(cid).or_insert(0) += 1;
                    *perimeter.entry(n_cid).or_insert(0) += 1;
                }
            }
        }
        mass.iter()
            .map(|(&cid, &m)| {
                let p = perimeter.get(&cid).copied().unwrap_or(0) as f64;
                p * p / m as f64
            })
            .sum()
    }

    fn area_is_connected(provinces: &[ProvinceNode], ids: &[u32], cid: u32) -> bool {
        let Some(start) = provinces
            .iter()
            .skip(1)
            .find(|p| ids[p.id as usize] == cid)
            .map(|p| p.id)
        else {
            return true;
        };
        let mut seen: HashSet<u32> = HashSet::new();
        let mut stack = vec![start];
        seen.insert(start);
        while let Some(id) = stack.pop() {
            for &(nb, _) in &provinces[id as usize].neighbors {
                let n = nb as usize;
                if n < ids.len() && ids[n] == cid && seen.insert(nb) {
                    stack.push(nb);
                }
            }
        }
        provinces
            .iter()
            .skip(1)
            .filter(|p| ids[p.id as usize] == cid)
            .all(|p| seen.contains(&p.id))
    }

    /// The shape pass pulls a lone protrusion back into the area it belongs to.
    #[test]
    fn relaxing_pulls_a_protrusion_into_shape() {
        let provinces = continent_grid(5, 5);
        let parents = parents_of(&provinces);
        let mut ids = vec![0u32; provinces.len()];
        for p in provinces.iter().skip(1) {
            let row = (p.id - 1) / 5;
            let col = (p.id - 1) % 5;
            ids[p.id as usize] = if (row < 3 && col < 3) || p.id == 9 {
                1
            } else {
                2
            };
        }

        let before = area_objective(&provinces, &ids, ClusterWeight::Pixels);
        let before_frontier = frontier_edges(&provinces, &ids);
        let locked = vec![false; provinces.len()];
        let total = relax_area_boundaries(
            &provinces,
            &mut ids,
            &parents,
            ClusterWeight::Pixels,
            300,
            1_000_000,
            &locked,
            AREA_RELAX_SWEEPS,
        );

        assert_eq!(total, 2, "relaxation never invents or removes an area");
        let after = area_objective(&provinces, &ids, ClusterWeight::Pixels);
        assert!(
            after < before,
            "the protrusion must round away: {before} -> {after}"
        );
        assert!(
            frontier_edges(&provinces, &ids) < before_frontier,
            "rounding an area must shorten its frontier"
        );
        assert!(area_is_connected(&provinces, &ids, 1));
        assert!(area_is_connected(&provinces, &ids, 2));
    }

    /// Relaxation is a pure function of the tier it is handed.
    #[test]
    fn relaxation_is_deterministic() {
        let provinces = continent_grid(6, 6);
        let parents = parents_of(&provinces);
        let mut first = vec![0u32; provinces.len()];
        let mut second = vec![0u32; provinces.len()];
        for p in provinces.iter().skip(1) {
            let col = (p.id - 1) % 6;
            let id = match col {
                0..=1 => 1,
                2..=3 => 2,
                _ => 3,
            };
            first[p.id as usize] = id;
            second[p.id as usize] = id;
        }

        let locked = vec![false; provinces.len()];
        relax_area_boundaries(
            &provinces,
            &mut first,
            &parents,
            ClusterWeight::Pixels,
            300,
            100_000,
            &locked,
            5,
        );
        relax_area_boundaries(
            &provinces,
            &mut second,
            &parents,
            ClusterWeight::Pixels,
            300,
            100_000,
            &locked,
            5,
        );

        assert_eq!(first, second, "two runs of the same tier must agree");
    }

    /// A small island is one place: every province of it ends up in one area, and
    /// the shape pass is not allowed to pull it apart again.
    #[test]
    fn a_small_island_becomes_one_area() {
        let mut provinces = continent_grid(5, 5);
        // A three-province island, all anchored to the same mainland coast.
        for id in [3u32, 8, 9] {
            provinces[id as usize].anchor_id = 2;
            provinces[id as usize].pixel_count = 200;
        }
        let parents = parents_of(&provinces);
        let mut ids = vec![0u32; provinces.len()];
        for p in provinces.iter().skip(1) {
            ids[p.id as usize] = if p.id <= 4 { 1 } else { 2 };
        }

        let locked = unify_island_areas(
            &provinces,
            &mut ids,
            &parents,
            ClusterWeight::Pixels,
            LAND_AREA_MAX_PIXELS,
        );

        let home = ids[3];
        assert!(home == 1 || home == 2, "the island kept a real area");
        for id in [3u32, 8, 9] {
            assert_eq!(ids[id as usize], home, "province {id} was left behind");
            assert!(locked[id as usize], "unified province {id} must be frozen");
        }
        assert!(!locked[1], "only the island is frozen");
    }
}
