use crate::core::Grid;
use crate::geopolitics::hierarchy::ProvinceNode;
use std::collections::HashMap;

pub struct ProvinceAccumulator {
    pub count:     usize,
    pub sum_cos_x: f64,
    pub sum_sin_x: f64,
    pub sum_y:     f64,
}

pub struct ExtractedGraph {
    pub provinces: Vec<ProvinceNode>,
    pub max_id:    u32,
}

// ─── Disjoint-Set Union (path-compressed with rank) ─────────────────────────
struct DisjointSet {
    parent: Vec<usize>,
    rank:   Vec<u8>,
}

impl DisjointSet {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank:   vec![0; size],
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, i: usize, j: usize) {
        let ri = self.find(i);
        let rj = self.find(j);
        if ri == rj { return; }
        match self.rank[ri].cmp(&self.rank[rj]) {
            std::cmp::Ordering::Less    => { self.parent[ri] = rj; }
            std::cmp::Ordering::Greater => { self.parent[rj] = ri; }
            std::cmp::Ordering::Equal   => {
                self.parent[rj] = ri;
                self.rank[ri]  += 1;
            }
        }
    }
}

/// Spherical wrap-around distance on the X axis.
fn wrap_dist_sq(x1: f32, y1: f32, x2: f32, y2: f32, w: f32) -> f32 {
    let mut dx = (x1 - x2).abs();
    if dx > w * 0.5 { dx = w - dx; }
    let dy = y1 - y2;
    dx * dx + dy * dy
}

/// Build the province adjacency graph and assign continent IDs.
///
/// # Geopolitical Continents & Natural Island Geography:
/// 1. Major landmasses form Continental Cores.
/// 2. Every island belongs to the continent it is physically closest to (no artificial dumping grounds!).
///    - Britain, Ireland, Iceland -> Europe
///    - Greenland, Newfoundland -> North America
///    - Madagascar -> Africa
///    - Japan, Taiwan, Indonesia -> Asia
/// 3. Each island identifies its Anchor Coast (the mainland coastal province with the absolute minimum distance).
pub fn extract_province_graph(
    province_grid: &Grid<u32>,
    heightmap:     &Grid<f32>,
    is_lake:       &Grid<bool>,
    sea_level:     f32,
) -> ExtractedGraph {
    let w = province_grid.width;
    let h = province_grid.height;
    let wf = w as f32;

    // ── Find maximum province ID ──────────────────────────────────────────────
    let mut max_id = 0u32;
    for &id in province_grid.raw_slice() {
        if id > max_id { max_id = id; }
    }

    let n = max_id as usize;

    // ── Accumulators for province centers & pixel counts ─────────────────────
    let mut accs: Vec<ProvinceAccumulator> = (0..=n).map(|_| ProvinceAccumulator {
        count: 0, sum_cos_x: 0.0, sum_sin_x: 0.0, sum_y: 0.0,
    }).collect();

    for y in 0..h {
        let fy = y as f64;
        for x in 0..w {
            let id = *province_grid.get(x as isize, y as isize);
            if id == 0 { continue; }
            let angle = (x as f64 / w as f64) * std::f64::consts::TAU;
            let acc   = &mut accs[id as usize];
            acc.count    += 1;
            acc.sum_cos_x += angle.cos();
            acc.sum_sin_x += angle.sin();
            acc.sum_y    += fy;
        }
    }

    // ── Edge maps ─────────────────────────────────────────────────────────────
    let mut land_edges:     HashMap<(u32, u32), u32> = HashMap::new();
    let mut maritime_edges: HashMap<(u32, u32), u32> = HashMap::new();

    let add_to = |map: &mut HashMap<(u32, u32), u32>, a: u32, b: u32, cost: u32| {
        let pair = if a < b { (a, b) } else { (b, a) };
        let entry = map.entry(pair).or_insert(u32::MAX);
        *entry = (*entry).min(cost);
    };

    let mut is_coastal_province = vec![false; n + 1];

    // ── Land Adjacency & Coastal Strait Detection ─────────────────────────────
    for y in 0..h {
        for x in 0..w {
            let id = *province_grid.get(x as isize, y as isize);
            if id == 0 { continue; }

            let cur_elev = *heightmap.get(x as isize, y as isize);

            // Right neighbour
            let rx  = province_grid.wrap_x(x as isize + 1) as isize;
            let rid = *province_grid.get(rx, y as isize);
            if rid != 0 && rid != id {
                let target_elev = *heightmap.get(rx, y as isize);
                let diff = ((cur_elev - sea_level) - (target_elev - sea_level)).abs();
                let cost = 10 + (diff * 3500.0) as u32;
                add_to(&mut land_edges, id, rid, cost);
            }

            // Down neighbour
            if y + 1 < h {
                let dy  = y as isize + 1;
                let down_id = *province_grid.get(x as isize, dy);
                if down_id != 0 && down_id != id {
                    let target_elev = *heightmap.get(x as isize, dy);
                    let diff = ((cur_elev - sea_level) - (target_elev - sea_level)).abs();
                    let cost = 10 + (diff * 3500.0) as u32;
                    add_to(&mut land_edges, id, down_id, cost);
                }
            }

            // Coastal detection: A pixel is coastal only if it borders actual WATER
            let is_water_at = |px: isize, py: isize| -> bool {
                *heightmap.get(px, py) <= sea_level || *is_lake.get(px, py)
            };

            let is_coast = is_water_at(rx, y as isize)
                || (y + 1 < h && is_water_at(x as isize, y as isize + 1))
                || (x > 0 && is_water_at(x as isize - 1, y as isize))
                || (y > 0 && is_water_at(x as isize, y as isize - 1));

            if is_coast {
                is_coastal_province[id as usize] = true;

                let p_count = accs[id as usize].count;
                let sample_this_pixel = if p_count < 150 {
                    true
                } else {
                    (x + y * 7) % 3 == 0
                };

                if sample_this_pixel {
                    for i in 0..16u32 {
                        let theta  = (i as f32 / 16.0) * std::f32::consts::TAU;
                        let step_x = theta.cos();
                        let step_y = theta.sin();

                        for d in 2..=90u32 {
                            let nx = province_grid
                                .wrap_x(x as isize + (step_x * d as f32).round() as isize)
                                as isize;
                            let ny = y as isize + (step_y * d as f32).round() as isize;

                            if ny < 0 || ny >= h as isize { break; }

                            // If we hit land:
                            if !is_water_at(nx, ny) {
                                let target_id = *province_grid.get(nx, ny);
                                if target_id != 0 && target_id != id {
                                    let cost = 25 + d * 4;
                                    add_to(&mut maritime_edges, id, target_id, cost);
                                }
                                break; // Land blocks maritime rays!
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Compute province centers ──────────────────────────────────────────────
    let mut centers = vec![(0.0f32, 0.0f32); n + 1];
    for i in 1..=n {
        let acc = &accs[i];
        if acc.count > 0 {
            let avg_angle = acc.sum_sin_x.atan2(acc.sum_cos_x).rem_euclid(std::f64::consts::TAU);
            let cx = ((avg_angle / std::f64::consts::TAU) * w as f64) as f32;
            let cy = (acc.sum_y / acc.count as f64) as f32;
            centers[i] = (cx, cy);
        }
    }

    // ── CONTINENT DETECTION & ACCURATE ISLAND ANCHORING ───────────────────────
    // 1. Identify pure landmasses (via land edges only)
    let mut land_ds = DisjointSet::new(n + 1);
    for &(a, b) in land_edges.keys() {
        land_ds.union(a as usize, b as usize);
    }

    let mut landmass_provinces: HashMap<usize, Vec<u32>> = HashMap::new();
    let mut landmass_pixels: HashMap<usize, usize> = HashMap::new();

    for i in 1..=n {
        if accs[i].count > 0 {
            let root = land_ds.find(i);
            landmass_provinces.entry(root).or_default().push(i as u32);
            *landmass_pixels.entry(root).or_insert(0) += accs[i].count;
        }
    }

    let mut sorted_landmasses: Vec<_> = landmass_provinces.into_iter().collect();
    sorted_landmasses.sort_by(|a, b| {
        let px_a = landmass_pixels.get(&a.0).copied().unwrap_or(0);
        let px_b = landmass_pixels.get(&b.0).copied().unwrap_or(0);
        px_b.cmp(&px_a)
    });

    // 2. Select Major Continental Cores (Top major landmasses)
    let mut continent_assignments = vec![0u32; n + 1];
    let mut is_mainland = vec![false; n + 1];
    let mut total_continents = 0u32;

    for (_root, members) in &sorted_landmasses {
        let is_major = members.len() >= 120
            || (total_continents < 4 && members.len() >= 20);

        if is_major && total_continents < 7 {
            total_continents += 1;
            for &p in members {
                continent_assignments[p as usize] = total_continents;
                is_mainland[p as usize] = true;
            }
        }
    }

    if total_continents == 0 && !sorted_landmasses.is_empty() {
        for &p in &sorted_landmasses[0].1 {
            continent_assignments[p as usize] = 1;
            is_mainland[p as usize] = true;
        }
    }

    // Collect all coastal mainland provinces for exact nearest-neighbor search
    let mainland_coastal_provinces: Vec<u32> = (1..=n)
        .filter(|&i| is_mainland[i] && is_coastal_province[i])
        .map(|i| i as u32)
        .collect();

    // 3. Anchor & Continent Assignment for Islands
    // Every island landmass in the world finds the mainland coastal province with the
    // ABSOLUTE MINIMUM physical distance.
    // - Britain & Iceland find Europe -> assigned to Europe!
    // - Newfoundland & Greenland find North America -> assigned to North America!
    // - Madagascar finds Africa -> assigned to Africa!
    // - Island A between Coast A (nearer) and Coast B (further) finds Coast A -> assigned to Coast A!
    let mut anchor_ids = vec![0u32; n + 1];
    for i in 1..=n {
        if is_mainland[i] {
            anchor_ids[i] = i as u32; // Mainland provinces anchor to themselves
        }
    }

    for (_root, members) in &sorted_landmasses {
        if members.is_empty() || is_mainland[members[0] as usize] {
            continue; // Already a mainland continent
        }

        // Exhaustive exact search across all coastal mainland provinces
        // to guarantee the ABSOLUTE physically nearest coast is chosen!
        let mut best_mainland = 0u32;
        let mut best_island_p = members[0];
        let mut min_dist_sq = f32::MAX;

        for &island_p in members {
            let (ix, iy) = centers[island_p as usize];
            for &mainland_p in &mainland_coastal_provinces {
                let (mx, my) = centers[mainland_p as usize];
                let d2 = wrap_dist_sq(ix, iy, mx, my, wf);
                if d2 < min_dist_sq {
                    min_dist_sq = d2;
                    best_mainland = mainland_p;
                    best_island_p = island_p;
                }
            }
        }

        if best_mainland != 0 {
            let dist = min_dist_sq.sqrt();
            let target_continent = continent_assignments[best_mainland as usize];

            for &island_p in members {
                anchor_ids[island_p as usize] = best_mainland;
                continent_assignments[island_p as usize] = target_continent;
            }

            // Ensure a direct lowest-cost maritime edge between closest island point and anchor
            let cost = 25 + (dist * 4.0) as u32;
            add_to(&mut maritime_edges, best_island_p, best_mainland, cost);
        }
    }

    // ── Guaranteed Graph Connectivity ─────────────────────────────────────────
    let mut full_ds = DisjointSet::new(n + 1);
    for &(a, b) in land_edges.keys() { full_ds.union(a as usize, b as usize); }
    for &(a, b) in maritime_edges.keys() { full_ds.union(a as usize, b as usize); }

    let mut comp_members: HashMap<usize, Vec<u32>> = HashMap::new();
    for i in 1..=n {
        if accs[i].count > 0 {
            let root = full_ds.find(i);
            comp_members.entry(root).or_default().push(i as u32);
        }
    }

    if comp_members.len() > 1 {
        let mut sorted_comps: Vec<_> = comp_members.into_iter().collect();
        sorted_comps.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));

        let main_root = sorted_comps[0].0;
        let main_members = &sorted_comps[0].1;

        for (root, members) in &sorted_comps[1..] {
            if *root == main_root { continue; }
            let mut best_pair = (members[0], main_members[0]);
            let mut min_d2 = f32::MAX;

            for &m in members {
                let (mx, my) = centers[m as usize];
                for &other in main_members {
                    let (ox, oy) = centers[other as usize];
                    let d2 = wrap_dist_sq(mx, my, ox, oy, wf);
                    if d2 < min_d2 {
                        min_d2 = d2;
                        best_pair = (m, other);
                    }
                }
            }

            let dist = min_d2.sqrt();
            let cost = 50 + (dist * 5.0) as u32;
            add_to(&mut maritime_edges, best_pair.0, best_pair.1, cost);
            full_ds.union(best_pair.0 as usize, best_pair.1 as usize);
        }
    }

    // ── Build final neighbor list per province ────────────────────────────────
    let mut neighbor_map: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n + 1];

    let insert_neighbors = |nmap: &mut Vec<Vec<(u32, u32)>>, edges: &HashMap<(u32, u32), u32>| {
        for (&(a, b), &cost) in edges {
            nmap[a as usize].push((b, cost));
            nmap[b as usize].push((a, cost));
        }
    };

    insert_neighbors(&mut neighbor_map, &land_edges);
    insert_neighbors(&mut neighbor_map, &maritime_edges);

    for list in &mut neighbor_map {
        list.sort_by_key(|&(nb, _)| nb);
        list.dedup_by(|a, b| {
            if a.0 == b.0 {
                b.1 = b.1.min(a.1);
                true
            } else {
                false
            }
        });
    }

    // ── Build ProvinceNode list ───────────────────────────────────────────────
    let mut provinces = Vec::with_capacity(n + 1);
    provinces.push(ProvinceNode {
        id: 0, center_x: 0.0, center_y: 0.0, pixel_count: 0,
        neighbors: vec![],
        area_id: 0, region_id: 0, superregion_id: 0, continent_id: 0,
        anchor_id: 0,
    });

    for i in 1..=n {
        let acc = &accs[i];
        if acc.count == 0 {
            provinces.push(ProvinceNode {
                id: i as u32, center_x: 0.0, center_y: 0.0, pixel_count: 0,
                neighbors: vec![],
                area_id: 0, region_id: 0, superregion_id: 0, continent_id: 0,
                anchor_id: 0,
            });
            continue;
        }

        let (center_x, center_y) = centers[i];
        let continent_id = continent_assignments[i].max(1);
        let anchor_id = anchor_ids[i];

        provinces.push(ProvinceNode {
            id: i as u32,
            center_x,
            center_y,
            pixel_count: acc.count,
            neighbors: neighbor_map[i].clone(),
            area_id: 0,
            region_id: 0,
            superregion_id: 0,
            continent_id,
            anchor_id,
        });
    }

    ExtractedGraph { provinces, max_id }
}

/// Build the adjacency graph for the marine provinces.
///
/// Structurally identical to the land graph — province centres, pixel counts
/// and cost-weighted edges — but the ocean is a single connected body, so there
/// are no landmasses to identify here: the top (Continental Basin) tier of the
/// ocean hierarchy is produced later by clustering, not by island geography.
/// Consequently `continent_id` and `anchor_id` are left at `0` for every ocean
/// province.
pub fn extract_ocean_graph(ocean_ids: &Grid<u32>) -> ExtractedGraph {
    let w = ocean_ids.width;
    let h = ocean_ids.height;
    let wf = w as f32;

    // ── Find maximum province ID ──────────────────────────────────────────────
    let mut max_id = 0u32;
    for &id in ocean_ids.raw_slice() {
        if id > max_id { max_id = id; }
    }
    let n = max_id as usize;

    // ── Accumulators for province centres & pixel counts ─────────────────────
    let mut accs: Vec<ProvinceAccumulator> = (0..=n).map(|_| ProvinceAccumulator {
        count: 0, sum_cos_x: 0.0, sum_sin_x: 0.0, sum_y: 0.0,
    }).collect();

    for y in 0..h {
        let fy = y as f64;
        for x in 0..w {
            let id = ocean_ids.raw_slice()[y * w + x];
            if id == 0 { continue; }
            let angle = (x as f64 / w as f64) * std::f64::consts::TAU;
            let acc = &mut accs[id as usize];
            acc.count += 1;
            acc.sum_cos_x += angle.cos();
            acc.sum_sin_x += angle.sin();
            acc.sum_y += fy;
        }
    }

    let mut centers = vec![(0.0f32, 0.0f32); n + 1];
    for i in 1..=n {
        let acc = &accs[i];
        if acc.count == 0 { continue; }
        let avg_cos = acc.sum_cos_x / acc.count as f64;
        let avg_sin = acc.sum_sin_x / acc.count as f64;
        let angle = avg_sin.atan2(avg_cos);
        let cx = (((angle / std::f64::consts::TAU) + 0.5) * w as f64) as f32;
        let cy = (acc.sum_y / acc.count as f64) as f32;
        centers[i] = (cx, cy);
    }

    // ── Edge weight = shared-border strength (shorter borders cost more) ─────
    let mut boundary: HashMap<(u32, u32), u32> = HashMap::new();

    for y in 0..h {
        let dy = y as isize;
        for x in 0..w {
            let id = ocean_ids.raw_slice()[y * w + x];
            if id == 0 { continue; }

            // Right neighbour (wrapping in X)
            let rx = ocean_ids.wrap_x(x as isize + 1) as isize;
            let rid = *ocean_ids.get(rx, dy);
            if rid != 0 && rid != id {
                let pair = if id < rid { (id, rid) } else { (rid, id) };
                *boundary.entry(pair).or_insert(0) += 1;
            }

            // Down neighbour
            if y + 1 < h {
                let down_id = *ocean_ids.get(x as isize, dy + 1);
                if down_id != 0 && down_id != id {
                    let pair = if id < down_id { (id, down_id) } else { (down_id, id) };
                    *boundary.entry(pair).or_insert(0) += 1;
                }
            }
        }
    }

    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for (&(a, b), &shared) in &boundary {
        let cost = (4000.0 / shared.max(1) as f32).max(1.0) as u32;
        edges.insert((a, b), cost);
    }


    // ── Ensure graph connectivity (defensive: the ocean is one body) ─────────
    let mut full_ds = DisjointSet::new(n + 1);
    for &(a, b) in edges.keys() { full_ds.union(a as usize, b as usize); }

    let mut comp_members: HashMap<usize, Vec<u32>> = HashMap::new();
    for i in 1..=n {
        if accs[i].count > 0 {
            let root = full_ds.find(i);
            comp_members.entry(root).or_default().push(i as u32);
        }
    }

    if comp_members.len() > 1 {
        let mut sorted_comps: Vec<_> = comp_members.into_iter().collect();
        sorted_comps.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));

        let main_root = sorted_comps[0].0;
        let main_members = &sorted_comps[0].1;

        for (root, members) in &sorted_comps[1..] {
            if *root == main_root { continue; }
            let mut best_pair = (members[0], main_members[0]);
            let mut min_d2 = f32::MAX;

            for &m in members {
                let (mx, my) = centers[m as usize];
                for &other in main_members {
                    let (ox, oy) = centers[other as usize];
                    let d2 = wrap_dist_sq(mx, my, ox, oy, wf);
                    if d2 < min_d2 {
                        min_d2 = d2;
                        best_pair = (m, other);
                    }
                }
            }

            let dist = min_d2.sqrt();
            let cost = 25 + (dist * 4.0) as u32;
            let (a, b) = if best_pair.0 < best_pair.1 {
                (best_pair.0, best_pair.1)
            } else {
                (best_pair.1, best_pair.0)
            };
            edges.insert((a, b), cost);
            full_ds.union(best_pair.0 as usize, best_pair.1 as usize);
        }
    }

    // ── Build final neighbour list per province ───────────────────────────────
    let mut neighbor_map: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n + 1];
    for (&(a, b), &cost) in &edges {
        neighbor_map[a as usize].push((b, cost));
        neighbor_map[b as usize].push((a, cost));
    }
    for list in &mut neighbor_map {
        list.sort_by_key(|&(nb, _)| nb);
        list.dedup_by(|a, b| {
            if a.0 == b.0 { b.1 = b.1.min(a.1); true } else { false }
        });
    }

    // ── Build ProvinceNode list ───────────────────────────────────────────────
    let mut provinces = Vec::with_capacity(n + 1);
    provinces.push(ProvinceNode {
        id: 0, center_x: 0.0, center_y: 0.0, pixel_count: 0,
        neighbors: vec![],
        area_id: 0, region_id: 0, superregion_id: 0, continent_id: 0,
        anchor_id: 0,
    });

    for i in 1..=n {
        let acc = &accs[i];
        let (center_x, center_y) = centers[i];
        provinces.push(ProvinceNode {
            id: i as u32,
            center_x,
            center_y,
            pixel_count: acc.count,
            neighbors: neighbor_map[i].clone(),
            area_id: 0,
            region_id: 0,
            superregion_id: 0,
            continent_id: 0,
            anchor_id: 0,
        });
    }

    ExtractedGraph { provinces, max_id }
}

