//! Province-level religion: which creed each province keeps.
//!
//! The creed itself lives in [`faith`](crate::civilization::faith). This module
//! takes the faiths a world's societies generated, gives each one a stable
//! identity, and spreads them across the province graph. A religion is therefore
//! a world entity, not a possession of one realm: several realms may keep the
//! same creed, and one realm may keep several.
//!
//! The pass is a frontier walk rather than a Dijkstra flood. Every inhabited
//! province already starts with its owner's faith, so the question is not which
//! faith is nearest but which faith the province *turns to*: terrain, rivers,
//! cities, the travelling habits of the creed and the cost of crossing a realm
//! frontier all shape the answer. A final cleanup removes the rural
//! one-province speckle that no history could explain, while leaving isolated
//! urban, coastal and displaced communities alone — a faith may live in a single
//! province, if that province is a town, a port or a community the map can
//! believe in.
//!
//! ## Consolidation
//!
//! A province-level map is not a census of every local recension. Two ceilings
//! keep the roster honest, and both are read from [`SocietyTier`]:
//!
//! * [`ReligionConfig::faiths_per_tier`] caps the faiths one **society** keeps.
//!   A forager people that spans a continent is one tradition with two or three
//!   local recensions, not forty near-identical creeds; a state is left alone,
//!   because a state really is a mosaic of local cults.
//! * [`ReligionConfig::faiths_per_superregion`] caps the broad traditions of the
//!   two **mobile** tiers across a superregion. This is the Tengrism of the
//!   steppe: many peoples, one family of creeds, with a local recension still
//!   surviving for each region inside the family.
//!
//! The planner groups faiths by *tradition kind* — the metaphysical root and,
//! where it matters, the object of reverence — so a totem cult and an ancestor
//! cult are different traditions even when both are animist. The leading kind's
//! largest faith becomes the umbrella; one keeper is kept for every other kind
//! that holds a real share of the ground, then one for each geographic group,
//! and every other faith falls to the keeper of its own kind. Only when a cell
//! has more kinds than its ceiling does a faith cross to another kind. The same
//! ceiling also caps how many daughters one schism may grow, so the map never
//! has to undo an explosion it could have prevented.
//!
//! [`SocietyTier`]: crate::civilization::SocietyTier
//! [`ReligionConfig::faiths_per_tier`]: crate::civilization::config::ReligionConfig::faiths_per_tier
//! [`ReligionConfig::faiths_per_superregion`]: crate::civilization::config::ReligionConfig::faiths_per_superregion

use crate::civilization::config::ReligionConfig;
use crate::civilization::faith::{
    Divinity, Faith, FaithContext, MetaphysicalRoot, NontheistFocus, OutgroupStance, SacredLocus,
    SpiritKind, Transmission, generate_faith, inherit_faith,
};
use crate::core::sphere;
use crate::geopolitics::{GeopoliticalHierarchy, ProvinceNode};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use super::SocietyTier;

/// Salt mixed into the world seed for the province religion pass. It is distinct
/// from the creed stream ([`FAITH_SALT`](crate::civilization::faith::FAITH_SALT))
/// and from the naming stream, so distributing a faith can never move a creed or
/// a name.
pub const RELIGION_SALT: u64 = 0x0F41_7A11_0000_000A;

/// Everything this pass needs to know about one province's country.
///
/// A trimmed view of the generator's own census: relief, climate, discharge and
/// coast, with none of the political bookkeeping. It exists so this module never
/// reaches back into the stage's private records.
#[derive(Clone, Copy, Default, Debug)]
pub struct ProvinceLandscape {
    pub present: bool,
    pub area_km2: f32,
    pub mean_elevation_m: f32,
    pub mean_temp_c: f32,
    pub mean_precip: f32,
    pub max_flow: f32,
    pub coastal: bool,
    pub lakeshore: bool,
}

/// One religion of the world: a creed and every place that keeps it.
#[derive(Clone, Debug)]
pub struct Religion {
    /// The seven-level cascade, exactly as the creed generator drew it.
    pub faith: Faith,
    /// The realm the religion rose in. Its tongue names the faith and its holy
    /// places, and its seat is where a person-founder's homeland lies.
    pub origin_realm: u16,
    /// The province the religion rose in, usually the origin realm's seat.
    pub origin_province: u32,
    /// The shade this religion wears on the Religions map.
    pub color: [u8; 3],
    /// Every province that keeps it, ascending.
    pub provinces: Vec<u32>,
    /// How many people keep it at the atlas date.
    pub population: u64,
    /// The ground those people stand on, in km².
    pub area_km2: f32,
    /// Every realm with at least one of its provinces, ascending.
    pub realms: Vec<u16>,
}

impl Religion {
    /// `3 provinces` or `1 province`.
    pub fn province_count_label(&self) -> &'static str {
        if self.provinces.len() == 1 {
            "province"
        } else {
            "provinces"
        }
    }
}

/// One faith as it stands inside one realm's borders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealmFaith {
    pub religion: u16,
    pub provinces: u32,
    pub population: u64,
}

/// A realm's religious census. There is deliberately no state religion here yet:
/// the atlas records what a realm's people keep, not what a throne patronises.
#[derive(Clone, Debug, Default)]
pub struct RealmReligion {
    /// The faith most of the realm's people keep. Zero only for an empty realm.
    pub dominant: u16,
    /// Every faith present in its provinces, largest first.
    pub faiths: Vec<RealmFaith>,
}

/// The whole province-level religion layer.
#[derive(Clone, Debug)]
pub struct ReligionWorld {
    pub religions: Vec<Religion>,
    /// Province id → religion id (0 = water, wasteland or unsettled).
    pub province_religion: Vec<u16>,
    /// Civilization id - 1 → its religious census.
    pub religion_of_realm: Vec<RealmReligion>,
}

impl ReligionWorld {
    /// A world with no religions, but the right number of empty provinces.
    pub fn empty(provinces: usize) -> Self {
        Self {
            religions: Vec::new(),
            province_religion: vec![0; provinces],
            religion_of_realm: Vec::new(),
        }
    }

    /// A religion by its 1-based id.
    pub fn religion(&self, id: u16) -> Option<&Religion> {
        if id == 0 {
            return None;
        }
        self.religions.get(id as usize - 1)
    }
}
// ─── THE DISTRIBUTION ───────────────────────────────────────────────────────

/// Spread the world's faiths across its provinces and census what each realm
/// keeps.
///
/// `faiths` is the roster drawn by the history act, one creed per society.
/// `origins[i]` is the `(origin realm, origin province)` of `faiths[i]`;
/// `founding[realm]` is the religion id that realm follows after contact, and
/// `displaced[realm]` is the faith a conversion displaced there, if any.
///
/// Every inhabited province starts with its owner's founding faith. A realm that
/// converted keeps the displaced faith in a share of its rural provinces, which
/// is how a faith can survive without a throne. The frontier passes then let
/// neighbours argue; the cleanup leaves believable one-province communities alone.
/// Two consolidation passes — one before the frontier and one after it — keep
/// each society and each mobile superregion at the handful of traditions it can
/// really carry.
#[allow(clippy::too_many_arguments)]
pub fn distribute_religions(
    mut faiths: Vec<Faith>,
    origins: &mut Vec<(u16, u32)>,
    founding: &[u16],
    displaced: &[Option<u16>],
    seats: &[u32],
    realm_provinces: &[Vec<u32>],
    owner: &[u16],
    realm_tiers: &[SocietyTier],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    hierarchy: &GeopoliticalHierarchy,
    contexts: &[FaithContext],
    width: usize,
    config: &ReligionConfig,
    rng: &mut ChaCha8Rng,
) -> ReligionWorld {
    let n = owner.len();
    if faiths.is_empty() || n == 0 {
        return ReligionWorld::empty(n);
    }

    // ── 1. The ground each realm's faith already holds ──────────────────────
    let mut assignment = vec![0u16; n];
    let mut protected: BTreeSet<u32> = seats.iter().copied().collect();
    let displaced_faiths: BTreeSet<u16> = displaced.iter().flatten().copied().collect();

    for (realm, provinces) in realm_provinces.iter().enumerate() {
        let own = founding.get(realm).copied().unwrap_or(0);
        if own == 0 {
            continue;
        }
        let old = displaced.get(realm).copied().flatten();
        let seat = seats.get(realm).copied().unwrap_or(0);
        for &province in provinces {
            let index = province as usize;
            if index >= n {
                continue;
            }
            let mut faith = own;
            if let Some(old) = old
                && old != own
                && province != seat
            {
                let rural = rurality(
                    province, seat, population, landscapes, hierarchy, width, config,
                );
                if rng.random::<f32>() < config.residual_share * rural {
                    faith = old;
                }
            }
            assignment[index] = faith;
        }
    }
    // ── 2. Small societies adopt ────────────────────────────────────────────
    // A province whose society never seeded a faith is filled from the nearest
    // hearth on the province graph. A small realm lying between two hearths can
    // therefore take one faith on its western ground and another on its east —
    // the first way a realm comes to keep several religions.
    fill_unclaimed(
        &mut assignment,
        owner,
        population,
        landscapes,
        hierarchy,
        config,
        width,
    );

    // ── 3. Regional traditions ──────────────────────────────────────────────
    // A creed covering a continent is not one parish. Its larger regions grow
    // daughter traditions of their own, which is how a faith that spans several
    // countries also comes to hold several faiths inside one.
    schism_large_faiths(
        &mut faiths,
        origins,
        realm_tiers,
        &mut assignment,
        owner,
        population,
        landscapes,
        hierarchy,
        contexts,
        config,
        rng,
    );

    // ── 4. Diasporas ────────────────────────────────────────────────────────
    // A town, a port or a lake shore may keep a foreign faith as a minority
    // community. These are protected from the frontier walk: a city congregation
    // does not convert the countryside around it.
    protected.extend(seed_diasporas(
        &mut assignment,
        &mut faiths,
        origins,
        owner,
        population,
        landscapes,
        contexts,
        config,
        rng,
    ));

    // ── 5. Consolidation before the frontier ────────────────────────────────
    // A province-level map is not a census of every local recension. These two
    // passes first give every society its tier's handful of faiths, then let
    // the mobile tiers share a small family of traditions across a superregion.
    // Running them here means the frontier argues between traditions rather
    // than between forty near-identical daughters.
    consolidate_realms(
        &mut assignment,
        &faiths,
        owner,
        realm_tiers,
        population,
        hierarchy,
        config,
    );
    consolidate_superregions(
        &mut assignment,
        &faiths,
        owner,
        realm_tiers,
        population,
        hierarchy,
        config,
    );

    // ── 6. Frontier passes ──────────────────────────────────────────────────
    for _ in 0..config.rounds {
        let snapshot = assignment.clone();
        let mut next = snapshot.clone();
        for index in 1..n {
            if owner[index] == 0
                || !landscapes[index].present
                || protected.contains(&(index as u32))
            {
                continue;
            }
            let incumbent = snapshot[index];
            if incumbent == 0 {
                continue;
            }
            let node = &hierarchy.provinces[index];
            let mut scores: BTreeMap<u16, f32> = BTreeMap::new();
            let mut total = 0.0f32;
            for &(neighbour, edge_cost) in &node.neighbors {
                let nb = neighbour as usize;
                if nb >= n || owner[nb] == 0 || !landscapes[nb].present {
                    continue;
                }
                let faith = snapshot[nb];
                if faith == 0 || faith as usize > faiths.len() {
                    continue;
                }
                let weight = neighbour_weight(
                    &faiths[faith as usize - 1],
                    faith,
                    index,
                    nb,
                    owner,
                    population,
                    landscapes,
                    config,
                    edge_cost,
                );
                *scores.entry(faith).or_insert(0.0) += weight;
                total += weight;
            }
            if total <= 0.0 {
                continue;
            }
            let incumbent_score =
                scores.get(&incumbent).copied().unwrap_or(0.0) + config.incumbent_resistance;
            // Highest neighbour score wins; on a tie the lower faith id wins, so
            // the walk never depends on map iteration order.
            let Some((&best, &best_score)) = scores.iter().max_by(|(id_a, a), (id_b, b)| {
                a.partial_cmp(b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| id_b.cmp(id_a))
            }) else {
                continue;
            };
            if best == incumbent || best_score <= incumbent_score * config.flip_margin {
                continue;
            }
            let support = node
                .neighbors
                .iter()
                .filter(|&&(neighbour, _)| {
                    (neighbour as usize) < n && snapshot[neighbour as usize] == best
                })
                .count();
            if support >= 2 || best_score > total * 0.60 {
                next[index] = best;
            }
        }
        assignment = next;
    }

    // ── 7. Consolidation after the frontier ─────────────────────────────────
    // A faith may have crossed a border or been planted as a diaspora since the
    // first pass. The same two ceilings are applied again, so the final map
    // never keeps more traditions than its society or its superregion allows.
    consolidate_realms(
        &mut assignment,
        &faiths,
        owner,
        realm_tiers,
        population,
        hierarchy,
        config,
    );
    consolidate_superregions(
        &mut assignment,
        &faiths,
        owner,
        realm_tiers,
        population,
        hierarchy,
        config,
    );

    // ── 8. One-province communities ─────────────────────────────────────────
    // A faith may live in a single province — a city congregation, a port
    // quarter, a displaced people — but a lone rural patch that no history
    // explains is speckle. Only the latter is swept into its neighbours.
    clean_rural_speckle(
        &mut assignment,
        owner,
        population,
        landscapes,
        hierarchy,
        &protected,
        &displaced_faiths,
        config,
    );

    finish_religions(
        faiths, origins, assignment, owner, population, landscapes, hierarchy,
    )
}
// ─── FILLING THE GROUND ─────────────────────────────────────────────────────

/// One province reached by the fill walk, cheapest first.
#[derive(Clone, Copy, PartialEq)]
struct Step {
    cost: f32,
    province: u32,
    faith: u16,
}

impl Eq for Step {}

impl Ord for Step {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.province.cmp(&other.province))
            .then_with(|| self.faith.cmp(&other.faith))
    }
}

impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Fill every inhabited province that has no faith yet from the nearest seeded
/// province, walking the same province graph the frontier pass uses.
///
/// This is how the small societies adopt: a folk tradition from a hearth spreads
/// across the ground of the villages around it. A province with no land path to
/// any hearth — a lone island before seafaring — takes the faith of the nearest
/// seeded province by geography rather than being left blank.
fn fill_unclaimed(
    assignment: &mut [u16],
    owner: &[u16],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    hierarchy: &GeopoliticalHierarchy,
    config: &ReligionConfig,
    width: usize,
) {
    let n = assignment.len();
    let mut best = vec![f32::MAX; n];
    let mut heap: BinaryHeap<Step> = BinaryHeap::new();
    for index in 1..n {
        if owner[index] == 0 || !landscapes[index].present || assignment[index] == 0 {
            continue;
        }
        best[index] = 0.0;
        heap.push(Step {
            cost: 0.0,
            province: index as u32,
            faith: assignment[index],
        });
    }
    if heap.is_empty() {
        return;
    }

    while let Some(step) = heap.pop() {
        let index = step.province as usize;
        if index >= n || step.cost > best[index] + 1e-3 {
            continue;
        }
        if assignment[index] == 0 {
            assignment[index] = step.faith;
        } else if assignment[index] != step.faith {
            // The province was already finalised by another faith at an equal
            // cost; it does not spread a creed it does not keep.
            continue;
        }
        let node = &hierarchy.provinces[index];
        for &(neighbour, edge_cost) in &node.neighbors {
            let nb = neighbour as usize;
            if nb >= n || owner[nb] == 0 || !landscapes[nb].present {
                continue;
            }
            let cost = fill_step_cost(nb, index, owner, population, landscapes, config, edge_cost);
            let next = step.cost + cost;
            if next + 1e-3 < best[nb] {
                best[nb] = next;
                heap.push(Step {
                    cost: next,
                    province: neighbour,
                    faith: step.faith,
                });
            }
        }
    }

    // No land path at all: give the province the faith of the nearest seeded
    // province across the water. There are only a handful of these on a world,
    // so the quadratic scan is honest rather than costly.
    let leftovers: Vec<usize> = (1..n)
        .filter(|&index| owner[index] != 0 && assignment[index] == 0)
        .collect();
    if leftovers.is_empty() {
        return;
    }
    let seeds: Vec<(usize, u16)> = (1..n)
        .filter(|&index| owner[index] != 0 && assignment[index] != 0)
        .map(|index| (index, assignment[index]))
        .collect();
    for index in leftovers {
        let node = &hierarchy.provinces[index];
        let mut nearest: Option<(f32, u16)> = None;
        for &(seed, faith) in &seeds {
            let distance = sphere::wrap_distance_px(
                node.center_x,
                node.center_y,
                hierarchy.provinces[seed].center_x,
                hierarchy.provinces[seed].center_y,
                width,
            );
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, faith));
            }
        }
        if let Some((_, faith)) = nearest {
            assignment[index] = faith;
        }
    }
}

/// What it costs to enter one province during the fill walk.
fn fill_step_cost(
    destination: usize,
    source: usize,
    owner: &[u16],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    config: &ReligionConfig,
    edge_cost: u32,
) -> f32 {
    let friction = terrain_friction(destination, population, landscapes, config);
    let border = if owner[destination] != owner[source] {
        config.border_friction
    } else {
        1.0
    };
    edge_cost.max(1) as f32 * friction * border
}
// ─── REGIONAL TRADITIONS ────────────────────────────────────────────────────

/// Split a faith that covers a continent into the regional traditions it would
/// really have become.
///
/// The parent keeps the region its origin province lies in; every other region
/// that holds enough of its provinces receives a daughter faith through
/// [`inherit_faith`], so the daughter keeps the parent's root, cosmology and
/// primary powers while its secondaries, ethos and calendar lean toward its own
/// ground. This is how a world gets regional churches inside one country and one
/// creed inside several countries at the same time.
///
/// The scale is read from the faith's origin tier. A forager or herd people's
/// tradition is a country-wide thing, so its daughters are grouped by
/// `superregion_id`; a settled people's faith is grouped by the finer
/// `region_id`, because a state or a chiefdom really does keep regional
/// churches. The tier's own ceiling also caps the daughters: a people that may
/// keep three faiths cannot grow ten from one schism.
#[allow(clippy::too_many_arguments)]
fn schism_large_faiths(
    faiths: &mut Vec<Faith>,
    origins: &mut Vec<(u16, u32)>,
    realm_tiers: &[SocietyTier],
    assignment: &mut [u16],
    owner: &[u16],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    hierarchy: &GeopoliticalHierarchy,
    contexts: &[FaithContext],
    config: &ReligionConfig,
    rng: &mut ChaCha8Rng,
) {
    let original = faiths.len();
    for faith_id in 1..=original as u16 {
        let provinces: Vec<u32> = assignment
            .iter()
            .enumerate()
            .filter(|(_, faith)| **faith == faith_id)
            .map(|(index, _)| index as u32)
            .filter(|province| landscapes[*province as usize].present)
            .collect();
        if provinces.len() < config.schism_min_provinces {
            continue;
        }
        let origin = origins
            .get(faith_id as usize - 1)
            .map_or(0, |(_, province)| *province);
        // A faith's origin tier decides the scale of its family: a forager
        // tradition varies across superregions, a state's across regions.
        let tier = origins
            .get(faith_id as usize - 1)
            .and_then(|(realm, _)| realm_tiers.get(*realm as usize - 1))
            .copied()
            .unwrap_or(SocietyTier::States);
        let max_daughters = config.faiths_per_tier[tier.index() as usize].saturating_sub(1);
        if max_daughters == 0 {
            continue;
        }

        let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &province in &provinces {
            let group = hierarchy
                .provinces
                .get(province as usize)
                .map_or(0, |node| tradition_group(tier, node));
            groups.entry(group).or_default().push(province);
        }
        if groups.len() < 2 {
            continue;
        }
        let parent_group = groups
            .iter()
            .find(|(_, members)| members.contains(&origin))
            .map(|(group, _)| *group);
        let parent = faiths[faith_id as usize - 1].clone();

        // Largest groups first, so the ceiling spends its daughters on the
        // regions that really carry the faith. Groups beyond the ceiling stay
        // with the parent, which is the whole point of consolidation.
        let mut candidates: Vec<(u32, Vec<u32>)> = groups
            .into_iter()
            .filter(|(group, members)| {
                Some(*group) != parent_group && members.len() >= config.schism_min_group
            })
            .collect();
        candidates.sort_by(|(group_a, members_a), (group_b, members_b)| {
            members_b
                .len()
                .cmp(&members_a.len())
                .then_with(|| group_a.cmp(group_b))
        });

        for (_group, members) in candidates.into_iter().take(max_daughters) {
            // The daughter speaks for the realm that feeds most of its ground.
            let realm = members
                .iter()
                .map(|province| owner[*province as usize])
                .filter(|realm| *realm != 0)
                .max_by_key(|realm| {
                    members
                        .iter()
                        .filter(|province| owner[**province as usize] == *realm)
                        .map(|province| population[*province as usize])
                        .sum::<u64>()
                })
                .unwrap_or(0);
            if realm == 0 {
                continue;
            }
            let Some(context) = contexts.get(realm as usize - 1).copied() else {
                continue;
            };
            let daughter = inherit_faith(rng, &parent, &context);
            let new_id = faiths.len() as u16 + 1;
            faiths.push(daughter);
            let seat = *members
                .iter()
                .max_by_key(|province| population[**province as usize])
                .unwrap_or(&members[0]);
            origins.push((realm, seat));
            for province in members {
                assignment[province as usize] = new_id;
            }
        }
    }
}
// ─── CONSOLIDATION ──────────────────────────────────────────────────────────

/// The kind of tradition consolidation reads: the metaphysical root and, where
/// it matters, the object of reverence.
///
/// A god-pantheon and a totem cult are not the same tradition even when both
/// answer to a root of animism; a sun god and a moon goddess are. That is the
/// granularity the map needs: Tengrism and Totemism are different families, but
/// two local forms of either are the same family.
type TraditionKind = (MetaphysicalRoot, u8);

/// The tradition kind of one faith.
fn tradition_kind(faith: &Faith) -> TraditionKind {
    let object = match faith.divinity() {
        Divinity::Theistic(_) => 0,
        Divinity::Animistic(SpiritKind::Spirits) => 1,
        Divinity::Animistic(SpiritKind::Totems) => 2,
        Divinity::Animistic(SpiritKind::Ancestors) => 3,
        Divinity::Animistic(SpiritKind::Souls) => 4,
        Divinity::Nontheist(NontheistFocus::Concept(_)) => 5,
        Divinity::Nontheist(NontheistFocus::Person(_)) => 6,
    };
    (faith.root, object)
}

/// The geographic cell in which a faith may vary for a tier.
///
/// A forager or herd people's tradition is a country-wide thing, so its local
/// recensions are read across superregions. A settled people keeps the finer
/// political region, because a state or a chiefdom really does grow regional
/// churches.
fn tradition_group(tier: SocietyTier, node: &ProvinceNode) -> u32 {
    match tier {
        SocietyTier::HunterGatherers | SocietyTier::NomadicPastoralists => node.superregion_id,
        _ => node.region_id,
    }
}

/// What one faith holds inside one consolidation cell.
#[derive(Clone, Debug, Default)]
struct FaithCellStats {
    provinces: u32,
    population: u64,
    /// Geographic group → provinces in it.
    groups: BTreeMap<u32, u32>,
}

impl FaithCellStats {
    /// The geographic group in which this faith is strongest.
    fn dominant_group(&self) -> u32 {
        self.groups
            .iter()
            .max_by(|(group_a, count_a), (group_b, count_b)| {
                count_a.cmp(count_b).then_with(|| group_b.cmp(group_a))
            })
            .map_or(0, |(group, _)| *group)
    }
}

/// Fold every society's faiths into the handful its tier allows.
///
/// The ceiling is the whole point: a hunter-gatherer people that spans a
/// continent is one tradition with two or three local recensions, not forty
/// near-identical creeds. A state is left alone, because its local cults are
/// what a state really looks like.
fn consolidate_realms(
    assignment: &mut [u16],
    faiths: &[Faith],
    owner: &[u16],
    realm_tiers: &[SocietyTier],
    population: &[u64],
    hierarchy: &GeopoliticalHierarchy,
    config: &ReligionConfig,
) {
    let n = assignment.len();
    let realm_count = owner.iter().copied().max().unwrap_or(0) as usize;
    if faiths.is_empty() || realm_count == 0 || n == 0 {
        return;
    }
    let kinds: Vec<TraditionKind> = faiths.iter().map(tradition_kind).collect();

    // What each faith holds inside each realm, and which of its geographic
    // groups (superregion for the mobile tiers, region for the settled ones)
    // carries it.
    let mut realms: Vec<BTreeMap<u16, FaithCellStats>> = vec![BTreeMap::new(); realm_count];
    for index in 1..n {
        let realm = owner[index];
        let faith = assignment[index];
        if realm == 0 || faith == 0 || faith as usize > faiths.len() {
            continue;
        }
        let tier = realm_tiers
            .get(realm as usize - 1)
            .copied()
            .unwrap_or(SocietyTier::States);
        let group = hierarchy
            .provinces
            .get(index)
            .map_or(0, |node| tradition_group(tier, node));
        let stats = realms[realm as usize - 1].entry(faith).or_default();
        stats.provinces += 1;
        stats.population += population[index];
        *stats.groups.entry(group).or_insert(0) += 1;
    }

    let mut plans: Vec<Option<BTreeMap<u16, u16>>> = vec![None; realm_count];
    for (realm, stats) in realms.iter().enumerate() {
        let tier = realm_tiers
            .get(realm)
            .copied()
            .unwrap_or(SocietyTier::States);
        let cap = config.faiths_per_tier[tier.index() as usize];
        if cap == usize::MAX || stats.len() <= cap {
            continue;
        }
        let plan = plan_consolidation(stats, &kinds, cap);
        if !plan.is_empty() {
            plans[realm] = Some(plan);
        }
    }
    if plans.iter().all(Option::is_none) {
        return;
    }

    for index in 1..n {
        let realm = owner[index];
        if realm == 0 {
            continue;
        }
        let Some(realm_plan) = plans.get(realm as usize - 1).and_then(Option::as_ref) else {
            continue;
        };
        if let Some(&keeper) = realm_plan.get(&assignment[index]) {
            assignment[index] = keeper;
        }
    }
}

/// Fold a superregion's mobile peoples into a small family of traditions.
///
/// This is the Tengrism of the steppe: many peoples, one family of creeds. The
/// same planner runs on the whole superregion, but only for the two mobile
/// tiers; a settled region's mosaic is left alone. A local recension still
/// survives because the planner keeps one faith per region inside the family.
fn consolidate_superregions(
    assignment: &mut [u16],
    faiths: &[Faith],
    owner: &[u16],
    realm_tiers: &[SocietyTier],
    population: &[u64],
    hierarchy: &GeopoliticalHierarchy,
    config: &ReligionConfig,
) {
    let n = assignment.len();
    if faiths.is_empty() || n == 0 {
        return;
    }
    let kinds: Vec<TraditionKind> = faiths.iter().map(tradition_kind).collect();

    // A cell is one superregion's ground held by one mobile tier. Only the
    // provinces in that cell are reassigned, so a faith that also lives in a
    // settled realm is untouched there.
    let mut cells: BTreeMap<(u32, usize), BTreeMap<u16, FaithCellStats>> = BTreeMap::new();
    let mut cell_of: Vec<Option<(u32, usize)>> = vec![None; n];
    for index in 1..n {
        let realm = owner[index];
        let faith = assignment[index];
        if realm == 0 || faith == 0 || faith as usize > faiths.len() {
            continue;
        }
        let tier = realm_tiers
            .get(realm as usize - 1)
            .copied()
            .unwrap_or(SocietyTier::States);
        let tier_index = tier.index() as usize;
        if config.faiths_per_superregion[tier_index] == usize::MAX {
            continue;
        }
        let Some(node) = hierarchy.provinces.get(index) else {
            continue;
        };
        let key = (node.superregion_id, tier_index);
        let stats = cells.entry(key).or_default().entry(faith).or_default();
        stats.provinces += 1;
        stats.population += population[index];
        // Inside a superregion the local recension is a region.
        *stats.groups.entry(node.region_id).or_insert(0) += 1;
        cell_of[index] = Some(key);
    }

    let mut plans: BTreeMap<(u32, usize), BTreeMap<u16, u16>> = BTreeMap::new();
    for (key, stats) in &cells {
        let cap = config.faiths_per_superregion[key.1];
        if cap == usize::MAX || stats.len() <= cap {
            continue;
        }
        let plan = plan_consolidation(stats, &kinds, cap);
        if !plan.is_empty() {
            plans.insert(*key, plan);
        }
    }
    if plans.is_empty() {
        return;
    }

    for index in 1..n {
        let Some(key) = cell_of[index] else {
            continue;
        };
        let Some(cell_plan) = plans.get(&key) else {
            continue;
        };
        if let Some(&keeper) = cell_plan.get(&assignment[index]) {
            assignment[index] = keeper;
        }
    }
}

/// Choose the keepers of one consolidation cell and fold every other faith into
/// one of them.
///
/// The leading tradition is the kind that holds the most ground. Its largest
/// faith becomes the umbrella; one keeper is kept for every other kind with a
/// real share of the ground, then one for each geographic group, then the
/// largest of what remains. Every non-keeper falls to the keeper of its own kind
/// (and within that kind to the largest, which keeps one tradition dominant),
/// or — only when a cell has more kinds than its ceiling — to the keeper sharing
/// its ground.
///
/// Deterministic throughout: every walk is over a sorted `BTreeMap` or a list
/// with a total order.
fn plan_consolidation(
    stats: &BTreeMap<u16, FaithCellStats>,
    kinds: &[TraditionKind],
    cap: usize,
) -> BTreeMap<u16, u16> {
    let cap = cap.max(1);
    if stats.len() <= cap {
        return BTreeMap::new();
    }

    let mut ranked: Vec<u16> = stats.keys().copied().collect();
    ranked.sort_by(|&a, &b| {
        let a_stats = &stats[&a];
        let b_stats = &stats[&b];
        b_stats
            .provinces
            .cmp(&a_stats.provinces)
            .then_with(|| b_stats.population.cmp(&a_stats.population))
            .then_with(|| a.cmp(&b))
    });
    let Some(&first) = ranked.first() else {
        return BTreeMap::new();
    };

    // Which tradition kind holds the most ground? That is the umbrella family,
    // not whichever single faith happens to be largest.
    let mut kind_totals: BTreeMap<TraditionKind, (u32, u64)> = BTreeMap::new();
    for &id in &ranked {
        let Some(kind) = kinds.get(id as usize - 1).copied() else {
            continue;
        };
        let faith = &stats[&id];
        let entry = kind_totals.entry(kind).or_insert((0, 0));
        entry.0 += faith.provinces;
        entry.1 += faith.population;
    }
    let mut kind_order: Vec<(TraditionKind, (u32, u64))> = kind_totals.into_iter().collect();
    kind_order.sort_by(
        |(kind_a, (provinces_a, population_a)), (kind_b, (provinces_b, population_b))| {
            provinces_b
                .cmp(provinces_a)
                .then_with(|| population_b.cmp(population_a))
                .then_with(|| kind_a.cmp(kind_b))
        },
    );
    let Some(&(primary_kind, _)) = kind_order.first() else {
        return BTreeMap::new();
    };
    let primary_faith = ranked
        .iter()
        .copied()
        .find(|id| kinds.get(*id as usize - 1) == Some(&primary_kind))
        .unwrap_or(first);

    let mut keepers = vec![primary_faith];
    let mut kept: BTreeSet<u16> = BTreeSet::new();
    kept.insert(primary_faith);

    // One keeper for every other kind, largest kind first. Genuine differences
    // are more important than another recension of the leading family — but
    // only while the difference is more than a speck. A one-province exotic
    // cult does not deserve a keeper slot that a region-sized recension could
    // use; it is folded into the leading tradition like any other minor faith.
    const MIN_TRADITION_KIND_SHARE: f32 = 0.05;
    let total_provinces: u32 = kind_order
        .iter()
        .map(|(_, (provinces, _))| *provinces)
        .sum();
    for &(kind, (provinces, _)) in kind_order.iter().skip(1) {
        if keepers.len() >= cap {
            break;
        }
        if (provinces as f32) < total_provinces as f32 * MIN_TRADITION_KIND_SHARE {
            continue;
        }
        if let Some(id) = ranked
            .iter()
            .copied()
            .find(|id| !kept.contains(id) && kinds.get(*id as usize - 1) == Some(&kind))
        {
            keepers.push(id);
            kept.insert(id);
        }
    }

    // One regional representative per geographic group, largest group first.
    // This is where the local recensions that survive are chosen.
    let mut group_totals: BTreeMap<u32, u32> = BTreeMap::new();
    for &id in &ranked {
        for (&group, &count) in &stats[&id].groups {
            *group_totals.entry(group).or_insert(0) += count;
        }
    }
    let mut group_order: Vec<(u32, u32)> = group_totals.into_iter().collect();
    group_order.sort_by(|(group_a, count_a), (group_b, count_b)| {
        count_b.cmp(count_a).then_with(|| group_a.cmp(group_b))
    });
    for (group, _) in group_order {
        if keepers.len() >= cap {
            break;
        }
        if let Some(id) = ranked.iter().copied().find(|id| {
            !kept.contains(id)
                && stats
                    .get(id)
                    .is_some_and(|faith| faith.groups.contains_key(&group))
        }) {
            keepers.push(id);
            kept.insert(id);
        }
    }

    // Any room left goes to the largest remaining faiths. A cell with fewer
    // kinds and groups than its ceiling still gets its full handful.
    for &id in &ranked {
        if keepers.len() >= cap {
            break;
        }
        if kept.insert(id) {
            keepers.push(id);
        }
    }

    let mut plan = BTreeMap::new();
    for &id in &ranked {
        if kept.contains(&id) {
            continue;
        }
        let kind = kinds.get(id as usize - 1).copied();
        let group = stats[&id].dominant_group();
        let target = keepers
            .iter()
            .copied()
            .max_by(|&a, &b| {
                let score = |keeper: u16| {
                    let same_kind = kind
                        .is_some_and(|kind| kinds.get(keeper as usize - 1).copied() == Some(kind));
                    let same_group = stats
                        .get(&keeper)
                        .is_some_and(|faith| faith.groups.contains_key(&group));
                    (same_kind, same_group, stats[&keeper].provinces, keeper)
                };
                score(a).cmp(&score(b))
            })
            .unwrap_or(primary_faith);
        if target != id {
            plan.insert(id, target);
        }
    }
    plan
}

// ─── DIASPORAS ──────────────────────────────────────────────────────────────

/// Plant minority communities in towns, ports and lake shores.
///
/// This is the one-province faith the map must be able to show: a merchant
/// quarter, a congregation of exiles, a shrine kept by a foreign quarter of a
/// city. The chosen province is returned as protected, because such a community
/// persists beside the majority instead of converting the countryside around it.
#[allow(clippy::too_many_arguments)]
fn seed_diasporas(
    assignment: &mut [u16],
    faiths: &mut Vec<Faith>,
    origins: &mut Vec<(u16, u32)>,
    owner: &[u16],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    contexts: &[FaithContext],
    config: &ReligionConfig,
    rng: &mut ChaCha8Rng,
) -> BTreeSet<u32> {
    let mut planted = BTreeSet::new();
    if faiths.len() < 2 || config.diaspora_chance <= 0.0 {
        return planted;
    }
    let mut realm_size: BTreeMap<u16, usize> = BTreeMap::new();
    for &realm in owner {
        if realm != 0 {
            *realm_size.entry(realm).or_insert(0) += 1;
        }
    }
    let mut used: BTreeMap<u16, usize> = BTreeMap::new();
    for index in 1..assignment.len() {
        let realm = owner[index];
        if realm == 0 || !landscapes[index].present {
            continue;
        }
        let land = &landscapes[index];
        let density = density_of(index, population, landscapes);
        let cosmopolitan =
            density >= config.urban_density_per_km2 || land.coastal || land.lakeshore;
        if !cosmopolitan {
            continue;
        }
        let ceiling = ((realm_size.get(&realm).copied().unwrap_or(1) as f32
            * config.diaspora_max_share) as usize)
            .clamp(1, config.diaspora_max_per_realm);
        if used.get(&realm).copied().unwrap_or(0) >= ceiling {
            continue;
        }
        if rng.random::<f32>() >= config.diaspora_chance {
            continue;
        }
        // A rare community grows a tradition of its own instead of keeping a
        // neighbour's. It begins in one town and may stay there: exactly the
        // one-province faith the map must be able to show.
        if rng.random::<f32>() < config.diaspora_new_faith_chance
            && let Some(context) = contexts.get(realm as usize - 1).copied()
        {
            let faith = generate_faith(rng, &context);
            let id = faiths.len() as u16 + 1;
            faiths.push(faith);
            origins.push((realm, index as u32));
            assignment[index] = id;
            *used.entry(realm).or_insert(0) += 1;
            planted.insert(index as u32);
            continue;
        }
        let current = assignment[index];
        // A travelling creed is the likelier guest. Draw a few candidates and
        // keep the one with the widest reach; the faith id breaks a tie.
        let mut guest = (0u16, 0.0f32);
        for _ in 0..6 {
            let candidate = rng.random_range(1..=faiths.len() as u16);
            if candidate == current {
                continue;
            }
            let reach = faith_reach(&faiths[candidate as usize - 1]);
            if reach > guest.1 || (reach == guest.1 && candidate < guest.0) {
                guest = (candidate, reach);
            }
        }
        if guest.0 == 0 {
            continue;
        }
        assignment[index] = guest.0;
        *used.entry(realm).or_insert(0) += 1;
        planted.insert(index as u32);
    }
    planted
}
// ─── FINISHING ──────────────────────────────────────────────────────────────

/// Drop faiths nobody kept, give every survivor its province list and census,
/// and paint it in its root's colour family.
#[allow(clippy::too_many_arguments)]
fn finish_religions(
    faiths: Vec<Faith>,
    origins: &[(u16, u32)],
    mut assignment: Vec<u16>,
    owner: &[u16],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    hierarchy: &GeopoliticalHierarchy,
) -> ReligionWorld {
    let n = assignment.len();

    // A faith converted out of every province it once held is history, not a
    // religion: the roster keeps only what somebody still keeps.
    let mut kept = vec![false; faiths.len() + 1];
    for &faith in &assignment {
        if faith != 0 && (faith as usize) < kept.len() {
            kept[faith as usize] = true;
        }
    }
    let mut remap = vec![0u16; faiths.len() + 1];
    let mut religions: Vec<Religion> = Vec::new();
    for (index, faith) in faiths.into_iter().enumerate() {
        let old = index as u16 + 1;
        if !kept[old as usize] {
            continue;
        }
        let (origin_realm, origin_province) = origins.get(index).copied().unwrap_or((old, 0));
        remap[old as usize] = religions.len() as u16 + 1;
        religions.push(Religion {
            faith,
            origin_realm: origin_realm.max(1),
            origin_province,
            color: [0; 3],
            provinces: Vec::new(),
            population: 0,
            area_km2: 0.0,
            realms: Vec::new(),
        });
    }
    for faith in &mut assignment {
        *faith = remap[*faith as usize];
    }

    // ── The census ──────────────────────────────────────────────────────────
    let realm_count = owner.iter().copied().max().unwrap_or(0) as usize;
    let mut realm_maps: Vec<BTreeMap<u16, (u32, u64)>> = vec![BTreeMap::new(); realm_count];
    for index in 0..n {
        let realm = owner[index];
        let faith = assignment[index];
        if realm == 0 || faith == 0 || faith as usize > religions.len() {
            continue;
        }
        let entry = realm_maps[realm as usize - 1]
            .entry(faith)
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += population[index];
        let religion = &mut religions[faith as usize - 1];
        religion.provinces.push(index as u32);
        religion.population += population[index];
        religion.area_km2 += landscapes[index].area_km2;
    }

    let mut religion_of_realm = Vec::with_capacity(realm_count);
    for map in realm_maps {
        let mut faiths: Vec<RealmFaith> = map
            .into_iter()
            .map(|(religion, (provinces, population))| RealmFaith {
                religion,
                provinces,
                population,
            })
            .collect();
        // A census reads largest first; ties go by ground held, then founding
        // order, so the order never depends on the map walk.
        faiths.sort_by(|a, b| {
            b.population
                .cmp(&a.population)
                .then(b.provinces.cmp(&a.provinces))
                .then(a.religion.cmp(&b.religion))
        });
        let dominant = faiths.first().map_or(0, |faith| faith.religion);
        religion_of_realm.push(RealmReligion { dominant, faiths });
    }
    for (index, census) in religion_of_realm.iter().enumerate() {
        let realm = index as u16 + 1;
        for faith in &census.faiths {
            religions[faith.religion as usize - 1].realms.push(realm);
        }
    }

    // ── The palette ─────────────────────────────────────────────────────────
    let roots: Vec<MetaphysicalRoot> = religions
        .iter()
        .map(|religion| religion.faith.root)
        .collect();
    let adjacency = religion_adjacency(&assignment, owner, landscapes, hierarchy);
    let colors = assign_religion_colors(&roots, &adjacency);
    for (religion, color) in religions.iter_mut().zip(colors) {
        religion.color = color;
    }

    ReligionWorld {
        religions,
        province_religion: assignment,
        religion_of_realm,
    }
}
// ─── THE CLEANUP ────────────────────────────────────────────────────────────

/// Sweep rural one-province faiths into a clear local majority.
///
/// A single-province faith is not automatically wrong: a city congregation, a
/// port quarter, a highland valley and a displaced people all produce exactly
/// that, and the Religions map must be able to show them. What this pass removes
/// is the lone rural patch with no plausible cause — the confetti a frontier walk
/// can leave behind. It only acts when a neighbour holds a two-thirds majority,
/// so an ambiguous border stays as it is.
#[allow(clippy::too_many_arguments)]
fn clean_rural_speckle(
    assignment: &mut [u16],
    owner: &[u16],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    hierarchy: &GeopoliticalHierarchy,
    protected: &BTreeSet<u32>,
    displaced: &BTreeSet<u16>,
    config: &ReligionConfig,
) {
    let n = assignment.len();
    let mut counts: BTreeMap<u16, u32> = BTreeMap::new();
    for &faith in assignment.iter() {
        if faith != 0 {
            *counts.entry(faith).or_insert(0) += 1;
        }
    }
    let snapshot = assignment.to_vec();
    for index in 1..n {
        if owner[index] == 0 || protected.contains(&(index as u32)) {
            continue;
        }
        let faith = snapshot[index];
        if faith == 0 || counts.get(&faith).copied().unwrap_or(0) != 1 {
            continue;
        }
        let land = &landscapes[index];
        if !land.present {
            continue;
        }
        let density = density_of(index, population, landscapes);
        let enclave = land.coastal
            || land.lakeshore
            || density >= config.urban_density_per_km2
            || land.mean_elevation_m >= config.hill_start_m
            || displaced.contains(&faith);
        if enclave {
            continue;
        }
        let mut votes: BTreeMap<u16, u32> = BTreeMap::new();
        for &(neighbour, _) in &hierarchy.provinces[index].neighbors {
            let nb = neighbour as usize;
            if nb >= n || owner[nb] == 0 {
                continue;
            }
            let other = snapshot[nb];
            if other == 0 || other == faith {
                continue;
            }
            *votes.entry(other).or_insert(0) += 1;
        }
        let total: u32 = votes.values().sum();
        if total < 2 {
            continue;
        }
        let Some((&best, &support)) = votes
            .iter()
            .max_by(|(id_a, a), (id_b, b)| a.cmp(b).then_with(|| id_b.cmp(id_a)))
        else {
            continue;
        };
        if support >= 2 && support * 3 >= total * 2 {
            assignment[index] = best;
        }
    }
}

// ─── FRONTIER WEIGHTING ─────────────────────────────────────────────────────

/// How much a neighbour's faith argues for itself in one province.
#[allow(clippy::too_many_arguments)]
fn neighbour_weight(
    faith: &Faith,
    _faith_id: u16,
    destination: usize,
    source: usize,
    owner: &[u16],
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    config: &ReligionConfig,
    edge_cost: u32,
) -> f32 {
    let reach = faith_reach(faith);
    let affinity = faith_affinity(faith, &landscapes[destination]);
    let friction = terrain_friction(destination, population, landscapes, config);
    let border = if owner[destination] != owner[source] {
        config.border_friction
    } else {
        1.0
    };
    reach * affinity / (edge_cost.max(1) as f32 * friction * border)
}

/// People per km² in one province.
fn density_of(index: usize, population: &[u64], landscapes: &[ProvinceLandscape]) -> f32 {
    let area = landscapes[index].area_km2;
    if area > 0.0 {
        population[index] as f32 / area
    } else {
        0.0
    }
}

/// How much of a province is countryside rather than town: the weight the
/// displaced faith gets when a realm converts. The seat and its cities take the
/// new creed; the far fields often do not.
fn rurality(
    province: u32,
    seat: u32,
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    hierarchy: &GeopoliticalHierarchy,
    width: usize,
    config: &ReligionConfig,
) -> f32 {
    let index = province as usize;
    if index >= landscapes.len() || seat as usize >= hierarchy.provinces.len() {
        return 0.0;
    }
    let density = density_of(index, population, landscapes);
    let urban = (density / config.urban_density_per_km2.max(0.001)).clamp(0.0, 1.0);
    let node = &hierarchy.provinces[index];
    let seat_node = &hierarchy.provinces[seat as usize];
    let distance = sphere::wrap_distance_px(
        node.center_x,
        node.center_y,
        seat_node.center_x,
        seat_node.center_y,
        width,
    );
    let far = (distance / (width as f32 * 0.06).max(1.0)).clamp(0.0, 1.0);
    (0.65 * (1.0 - urban) + 0.35 * far).clamp(0.0, 1.0)
}
/// How expensive it is for a creed to enter one province.
fn terrain_friction(
    index: usize,
    population: &[u64],
    landscapes: &[ProvinceLandscape],
    config: &ReligionConfig,
) -> f32 {
    let land = &landscapes[index];
    let mut friction = 1.0f32;

    // Relief: plains are cheap, hills cost more, mountains are a wall.
    if land.mean_elevation_m >= config.mountain_wall_m {
        friction *= config.mountain_friction;
    } else if land.mean_elevation_m >= config.hill_start_m {
        friction *= config.hill_friction;
    }

    // Climate and vegetation.
    if land.mean_temp_c < 2.0 {
        friction *= config.deep_forest_friction;
    } else if land.mean_precip < 0.10 {
        friction *= config.arid_friction;
    } else if land.mean_temp_c < 7.0 || (land.mean_temp_c >= 24.0 && land.mean_precip >= 0.46) {
        friction *= config.deep_forest_friction;
    } else if land.mean_precip < 0.28 {
        friction *= config.grassland_friction;
    } else {
        friction *= config.woodland_friction;
    }

    // Rivers are roads: a faith follows the valley, not the ridge.
    if land.max_flow >= 200.0 {
        friction *= config.river_discount;
    }

    // Cities resist absorption; empty country is cheap to sweep through.
    let density = density_of(index, population, landscapes);
    if density >= config.urban_density_per_km2 {
        friction *= config.urban_resistance;
    } else if density < 0.15 {
        friction *= config.wilderness_discount;
    }

    friction
}

/// How far a creed is willing to travel, from its own constitution.
///
/// A missionary monotheism with a written canon is the great traveller; a
/// secretive mystery cult stays where its initiates are. This is a multiplier on
/// a neighbour's argument, never a guarantee: the map and the incumbent still
/// decide.
fn faith_reach(faith: &Faith) -> f32 {
    let stance = match faith.stance {
        OutgroupStance::Missionary => 1.30,
        OutgroupStance::Syncretic => 1.15,
        OutgroupStance::Pluralistic => 1.05,
        OutgroupStance::Exclusive => 0.92,
        OutgroupStance::Secretive => 0.70,
    };
    // The strongest carrier the creed has: a written canon travels further than
    // an oral epic, but one good transmission is enough.
    let transmission = faith
        .transmissions
        .iter()
        .map(|transmission| match transmission {
            Transmission::WrittenCanon => 1.18,
            Transmission::Iconography => 1.06,
            Transmission::LivingOracle => 1.04,
            Transmission::OralEpic => 0.94,
            Transmission::MysteryInitiation => 0.90,
            Transmission::Astrology | Transmission::Augury => 0.98,
        })
        .fold(1.0f32, f32::max);
    let root = match faith.root {
        MetaphysicalRoot::Monotheism => 1.08,
        MetaphysicalRoot::Dualism => 1.04,
        MetaphysicalRoot::Animism => 0.92,
        MetaphysicalRoot::Pantheism => 0.96,
        MetaphysicalRoot::Panentheism => 0.98,
        _ => 1.0,
    };
    (stance * transmission * root).clamp(0.55, 1.60)
}

/// How at home a creed is in one country.
///
/// Only the sacred place is read here, not the whole creed: a river cult travels
/// down a valley, a mountain faith stays in the hills, a coast faith follows the
/// shore. The domains already shaped the creed; this is where they shape its
/// map.
fn faith_affinity(faith: &Faith, land: &ProvinceLandscape) -> f32 {
    let mut affinity = 1.0f32;
    match faith.locus {
        SacredLocus::Peak
        | SacredLocus::Observatory
        | SacredLocus::Crater
        | SacredLocus::Monolith
        | SacredLocus::Necropolis => {
            affinity += (land.mean_elevation_m / 3_000.0).clamp(0.0, 0.30);
        }
        SacredLocus::RiverConfluence | SacredLocus::Spring => {
            affinity += (land.max_flow / 2_000.0).clamp(0.0, 0.25);
            if land.lakeshore {
                affinity += 0.05;
            }
        }
        SacredLocus::Oasis => {
            affinity += (1.0 - land.mean_precip).clamp(0.0, 0.30);
        }
        SacredLocus::Strait | SacredLocus::AbyssalCoast => {
            if land.coastal {
                affinity += 0.22;
            }
        }
        SacredLocus::Grove => {
            affinity += (land.mean_precip * 0.4).clamp(0.0, 0.20);
            if (2.0..=30.0).contains(&land.mean_temp_c) {
                affinity += 0.05;
            }
        }
        SacredLocus::Caldera => {
            affinity += (land.mean_elevation_m / 4_000.0).clamp(0.0, 0.20);
            affinity += (land.max_flow / 4_000.0).clamp(0.0, 0.10);
        }
    }
    affinity.clamp(0.85, 1.30)
}
// ─── PALETTE ────────────────────────────────────────────────────────────────

/// Which religions share a frontier, condensed from the province graph.
///
/// Two religions are neighbours when any two provinces keeping them touch. The
/// result is symmetric and sorted, so the greedy colour walk is deterministic.
fn religion_adjacency(
    assignment: &[u16],
    owner: &[u16],
    landscapes: &[ProvinceLandscape],
    hierarchy: &GeopoliticalHierarchy,
) -> BTreeMap<u16, Vec<u16>> {
    let n = assignment.len();
    let mut edges: BTreeMap<u16, BTreeSet<u16>> = BTreeMap::new();
    for index in 1..n {
        if owner[index] == 0 || !landscapes[index].present {
            continue;
        }
        let a = assignment[index];
        if a == 0 {
            continue;
        }
        for &(neighbour, _) in &hierarchy.provinces[index].neighbors {
            let nb = neighbour as usize;
            if nb >= n || owner[nb] == 0 || !landscapes[nb].present {
                continue;
            }
            let b = assignment[nb];
            if b == 0 || b == a {
                continue;
            }
            edges.entry(a).or_default().insert(b);
            edges.entry(b).or_default().insert(a);
        }
    }
    edges
        .into_iter()
        .map(|(religion, neighbours)| (religion, neighbours.into_iter().collect()))
        .collect()
}

/// Give every religion its colour: the metaphysical root carries the hue, the
/// religion the shade.
///
/// The same greedy walk as the society palette, run inside each root's family: a
/// religion takes the first shade none of its *same-root* neighbours is already
/// wearing. Religions of different roots never compete, because their hue
/// families are already distinct.
pub fn assign_religion_colors(
    roots: &[MetaphysicalRoot],
    adjacency: &BTreeMap<u16, Vec<u16>>,
) -> Vec<[u8; 3]> {
    let mut chosen = vec![usize::MAX; roots.len()];
    let mut colors = vec![[0u8; 3]; roots.len()];
    for (rank, &root) in roots.iter().enumerate() {
        let mut taken = [false; MetaphysicalRoot::SHADES];
        if let Some(neighbours) = adjacency.get(&(rank as u16 + 1)) {
            for &neighbour in neighbours {
                if neighbour == 0 {
                    continue;
                }
                let other = neighbour as usize - 1;
                if other == rank || other >= roots.len() || roots[other] != root {
                    continue;
                }
                let shade = chosen[other];
                if shade < MetaphysicalRoot::SHADES {
                    taken[shade] = true;
                }
            }
        }
        let pick = (0..MetaphysicalRoot::SHADES)
            .find(|&shade| !taken[shade])
            .unwrap_or(0);
        chosen[rank] = pick;
        colors[rank] = root.shade(pick);
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civilization::faith::{Cardinality, Domain, generate_faith};
    use crate::civilization::{
        CradleKind, Environment, Government, Hierarchy, Outlook, SocietyTier, Subsistence,
    };
    use crate::geopolitics::ProvinceNode;

    fn context() -> FaithContext {
        FaithContext {
            environment: Environment::Floodplain,
            cradle_kind: CradleKind::RiverFloodplain,
            tier: SocietyTier::States,
            complexity: 0.60,
            subsistence: Subsistence::IrrigatedAgriculture,
            outlook: Outlook::Ceremonial,
            government: Government::Oligarchy,
            hierarchy: Hierarchy::Oligarchic,
            mean_temp_c: 18.0,
            mean_precip: 0.45,
            mean_elevation_m: 100.0,
            sea_px: 0,
            lake_px: 0,
            river: true,
            great_river: true,
            latitude_deg: 20.0,
        }
    }

    fn faiths(count: usize) -> Vec<Faith> {
        let context = context();
        let mut rng = ChaCha8Rng::seed_from_u64(17);
        (0..count)
            .map(|_| generate_faith(&mut rng, &context))
            .collect()
    }

    fn landscape() -> ProvinceLandscape {
        ProvinceLandscape {
            present: true,
            area_km2: 1.0,
            mean_elevation_m: 100.0,
            mean_temp_c: 15.0,
            mean_precip: 0.45,
            max_flow: 0.0,
            coastal: false,
            lakeshore: false,
        }
    }

    /// A chain of provinces `1..=nodes`, each touching its neighbours.
    fn chain(nodes: usize) -> GeopoliticalHierarchy {
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
        for id in 1..=nodes {
            let mut neighbors = Vec::new();
            if id > 1 {
                neighbors.push((id as u32 - 1, 1));
            }
            if id < nodes {
                neighbors.push((id as u32 + 1, 1));
            }
            provinces.push(ProvinceNode {
                id: id as u32,
                center_x: id as f32 * 10.0,
                center_y: 0.0,
                pixel_count: 1,
                neighbors,
                area_id: 1,
                region_id: 1,
                superregion_id: 1,
                continent_id: 1,
                anchor_id: 0,
            });
        }
        GeopoliticalHierarchy {
            total_provinces: nodes,
            total_areas: 1,
            total_regions: 1,
            total_superregions: 1,
            total_continents: 1,
            provinces,
        }
    }
    /// A chain of provinces `1..=nodes`, each touching its neighbours, with
    /// region and superregion ids assigned in blocks.
    fn chain_grouped(
        nodes: usize,
        region_size: usize,
        superregion_size: usize,
    ) -> GeopoliticalHierarchy {
        let mut hierarchy = chain(nodes);
        for id in 1..=nodes {
            let node = &mut hierarchy.provinces[id];
            node.region_id = ((id - 1) / region_size + 1) as u32;
            node.superregion_id = ((id - 1) / superregion_size + 1) as u32;
        }
        hierarchy.total_regions = nodes.div_ceil(region_size);
        hierarchy.total_superregions = nodes.div_ceil(superregion_size);
        hierarchy
    }

    /// Faiths forced into one tradition kind: animist totemists, all alike.
    fn same_kind_faiths(count: usize) -> Vec<Faith> {
        let mut roster = faiths(count);
        for faith in &mut roster {
            faith.root = MetaphysicalRoot::Animism;
            faith.cardinality = Cardinality::Polycentric;
            faith.domains = vec![Domain::Fauna, Domain::Hunt];
        }
        roster
    }

    /// One realm of `sizes.len()` faiths on a chain, consolidated once.
    fn consolidated_world(tier: SocietyTier, sizes: &[u32]) -> Vec<u16> {
        let nodes: usize = sizes.iter().map(|&size| size as usize).sum();
        let hierarchy = chain(nodes);
        let mut owner = vec![0u16; nodes + 1];
        let mut population = vec![0u64; nodes + 1];
        for index in 1..=nodes {
            owner[index] = 1;
            population[index] = 100;
        }
        let roster = same_kind_faiths(sizes.len());
        let mut assignment = vec![0u16; nodes + 1];
        let mut next = 1usize;
        for (index, &size) in sizes.iter().enumerate() {
            for _ in 0..size {
                assignment[next] = index as u16 + 1;
                next += 1;
            }
        }
        consolidate_realms(
            &mut assignment,
            &roster,
            &owner,
            &[tier],
            &population,
            &hierarchy,
            &ReligionConfig::default(),
        );
        assignment
    }

    fn faith_counts(assignment: &[u16]) -> BTreeMap<u16, u32> {
        let mut counts = BTreeMap::new();
        for &faith in assignment.iter().skip(1) {
            if faith != 0 {
                *counts.entry(faith).or_insert(0) += 1;
            }
        }
        counts
    }

    /// A world of two realms on a four-province chain, both seeded.
    fn two_realm_world(config: ReligionConfig) -> ReligionWorld {
        let hierarchy = chain(4);
        let owner = vec![0u16, 1, 1, 2, 2];
        let population = vec![0u64, 1_000, 1_000, 1_000, 1_000];
        let landscapes = vec![landscape(); 5];
        let founding = vec![1u16, 2u16];
        let seats = vec![1u32, 3u32];
        let realm_provinces = vec![vec![1u32, 2u32], vec![3u32, 4u32]];
        let contexts = vec![context(), context()];
        let mut origins = vec![(1u16, 1u32), (2u16, 3u32)];
        let mut rng = ChaCha8Rng::seed_from_u64(29);
        distribute_religions(
            faiths(2),
            &mut origins,
            &founding,
            &[None, None],
            &seats,
            &realm_provinces,
            &owner,
            &[SocietyTier::States, SocietyTier::States],
            &population,
            &landscapes,
            &hierarchy,
            &contexts,
            100,
            &config,
            &mut rng,
        )
    }

    #[test]
    fn every_inhabited_province_gets_a_religion() {
        // Realm 2 carries no hearth: its ground is filled from realm 1's faith.
        let hierarchy = chain(4);
        let owner = vec![0u16, 1, 1, 2, 2];
        let population = vec![0u64, 100, 100, 100, 100];
        let landscapes = vec![landscape(); 5];
        let founding = vec![1u16, 0u16];
        let seats = vec![1u32, 3u32];
        let realm_provinces = vec![vec![1u32, 2u32], vec![3u32, 4u32]];
        let contexts = vec![context(), context()];
        let mut origins = vec![(1u16, 1u32)];
        let mut rng = ChaCha8Rng::seed_from_u64(31);
        let world = distribute_religions(
            faiths(1),
            &mut origins,
            &founding,
            &[None, None],
            &seats,
            &realm_provinces,
            &owner,
            &[SocietyTier::States, SocietyTier::States],
            &population,
            &landscapes,
            &hierarchy,
            &contexts,
            100,
            &ReligionConfig::default(),
            &mut rng,
        );
        for province in 1..=4 {
            assert!(
                world.province_religion[province] != 0,
                "province {province} was left without a faith"
            );
        }
        assert_eq!(world.religion_of_realm.len(), 2);
        assert_eq!(world.religion_of_realm[1].dominant, 1);
    }

    #[test]
    fn a_realm_can_keep_two_faiths_and_a_faith_can_span_two_realms() {
        let mut config = ReligionConfig::default();
        config.rounds = 0;
        config.diaspora_chance = 1.0;
        config.diaspora_new_faith_chance = 0.0;
        config.diaspora_max_share = 1.0;
        config.diaspora_max_per_realm = 1;
        let world = two_realm_world(config);
        let first = &world.religion_of_realm[0];
        assert!(
            first.faiths.len() >= 2,
            "a cosmopolitan realm should keep more than one faith"
        );
        assert!(
            world
                .religion(2)
                .is_some_and(|religion| religion.realms.len() >= 2),
            "a diaspora should make one faith live in two realms"
        );
    }

    #[test]
    fn a_one_province_faith_can_survive_in_a_town() {
        let mut config = ReligionConfig::default();
        config.rounds = 0;
        config.diaspora_chance = 1.0;
        config.diaspora_new_faith_chance = 1.0;
        config.diaspora_max_share = 1.0;
        config.diaspora_max_per_realm = 1;
        let world = two_realm_world(config);
        assert!(
            world
                .religions
                .iter()
                .any(|religion| religion.provinces.len() == 1),
            "a town congregation should be allowed to keep a one-province faith"
        );
    }
    #[test]
    fn faiths_nobody_kept_are_pruned() {
        let hierarchy = chain(4);
        let owner = vec![0u16, 1, 1, 2, 2];
        let population = vec![0u64, 1_000, 1_000, 1_000, 1_000];
        let landscapes = vec![landscape(); 5];
        let founding = vec![1u16, 2u16];
        let seats = vec![1u32, 3u32];
        let realm_provinces = vec![vec![1u32, 2u32], vec![3u32, 4u32]];
        let contexts = vec![context(), context()];
        let mut origins = vec![(1u16, 1u32), (2u16, 3u32), (2u16, 4u32)];
        let mut rng = ChaCha8Rng::seed_from_u64(37);
        let mut config = ReligionConfig::default();
        config.rounds = 0;
        config.diaspora_chance = 0.0;
        let world = distribute_religions(
            faiths(3),
            &mut origins,
            &founding,
            &[None, None],
            &seats,
            &realm_provinces,
            &owner,
            &[SocietyTier::States, SocietyTier::States],
            &population,
            &landscapes,
            &hierarchy,
            &contexts,
            100,
            &config,
            &mut rng,
        );
        assert_eq!(world.religions.len(), 2);
    }

    #[test]
    fn consolidation_caps_a_forager_people() {
        // Sizes deliberately sum to 40 and leave one clear primary: after the
        // ceiling the three keepers are 20, 5 and 5, and every other same-kind
        // faith folds into the primary.
        let sizes = [20u32, 5, 5, 5, 2, 1, 1, 1];
        let assignment = consolidated_world(SocietyTier::HunterGatherers, &sizes);
        let counts = faith_counts(&assignment);
        assert_eq!(
            counts.len(),
            3,
            "a forager people should keep three faiths, not {}",
            counts.len()
        );
        assert_eq!(counts.get(&1).copied().unwrap_or(0), 30);
        assert_eq!(counts.get(&2).copied().unwrap_or(0), 5);
        assert_eq!(counts.get(&3).copied().unwrap_or(0), 5);
    }

    #[test]
    fn consolidation_keeps_a_dominant_faith() {
        let sizes = [20u32, 5, 5, 5, 2, 1, 1, 1];
        let assignment = consolidated_world(SocietyTier::HunterGatherers, &sizes);
        let counts = faith_counts(&assignment);
        let total: u32 = counts.values().sum();
        let dominant = counts.get(&1).copied().unwrap_or(0);
        assert_eq!(total, 40);
        assert!(
            dominant * 4 >= total * 3,
            "the leading tradition held {dominant}/{total}, not at least 75%"
        );
    }

    #[test]
    fn consolidation_leaves_a_state_mosaic_alone() {
        let sizes = [20u32, 5, 5, 5, 2, 1, 1, 1];
        let assignment = consolidated_world(SocietyTier::States, &sizes);
        assert_eq!(
            faith_counts(&assignment).len(),
            8,
            "a state should keep its local faiths"
        );
    }

    #[test]
    fn consolidation_preserves_distinct_tradition_kinds() {
        // Five totemists and three ancestor cults: the two kinds must survive
        // the cap, even though the totemists are larger.
        let mut roster = faiths(8);
        for faith in roster.iter_mut().take(5) {
            faith.root = MetaphysicalRoot::Animism;
            faith.cardinality = Cardinality::Polycentric;
            faith.domains = vec![Domain::Fauna, Domain::Hunt];
        }
        for faith in roster.iter_mut().skip(5) {
            faith.root = MetaphysicalRoot::Animism;
            faith.cardinality = Cardinality::Polycentric;
            faith.domains = vec![Domain::Ancestor];
        }
        let sizes = [20u32, 5, 5, 2, 1, 5, 1, 1];
        let nodes: usize = sizes.iter().map(|&size| size as usize).sum();
        let hierarchy = chain(nodes);
        let mut owner = vec![0u16; nodes + 1];
        let mut population = vec![0u64; nodes + 1];
        for index in 1..=nodes {
            owner[index] = 1;
            population[index] = 100;
        }
        let mut assignment = vec![0u16; nodes + 1];
        let mut next = 1usize;
        for (index, &size) in sizes.iter().enumerate() {
            for _ in 0..size {
                assignment[next] = index as u16 + 1;
                next += 1;
            }
        }
        consolidate_realms(
            &mut assignment,
            &roster,
            &owner,
            &[SocietyTier::HunterGatherers],
            &population,
            &hierarchy,
            &ReligionConfig::default(),
        );
        let counts = faith_counts(&assignment);
        assert_eq!(counts.len(), 3);
        assert!(counts.contains_key(&1), "the totemist primary was lost");
        assert!(
            counts.contains_key(&6),
            "the ancestor keeper was folded into the totemists"
        );
    }

    #[test]
    fn consolidation_is_deterministic() {
        let sizes = [20u32, 5, 5, 5, 2, 1, 1, 1];
        let first = consolidated_world(SocietyTier::HunterGatherers, &sizes);
        let second = consolidated_world(SocietyTier::HunterGatherers, &sizes);
        assert_eq!(first, second);
    }

    #[test]
    fn neighbouring_forager_societies_share_a_few_traditions() {
        let nodes = 40;
        let hierarchy = chain_grouped(nodes, 10, 40);
        let mut owner = vec![0u16; nodes + 1];
        let mut assignment = vec![0u16; nodes + 1];
        let mut population = vec![0u64; nodes + 1];
        for index in 1..=nodes {
            let realm = ((index - 1) / 10 + 1) as u16;
            owner[index] = realm;
            assignment[index] = realm;
            population[index] = 100;
        }
        let roster = same_kind_faiths(4);
        let mut config = ReligionConfig::default();
        config.faiths_per_superregion[SocietyTier::HunterGatherers.index() as usize] = 2;
        consolidate_superregions(
            &mut assignment,
            &roster,
            &owner,
            &[SocietyTier::HunterGatherers; 4],
            &population,
            &hierarchy,
            &config,
        );
        let counts = faith_counts(&assignment);
        assert_eq!(
            counts.len(),
            2,
            "four neighbouring forager peoples should share two traditions, not {}",
            counts.len()
        );
    }

    #[test]
    fn superregion_consolidation_skips_settled_tiers() {
        let nodes = 40;
        let hierarchy = chain_grouped(nodes, 10, 40);
        let mut owner = vec![0u16; nodes + 1];
        let mut assignment = vec![0u16; nodes + 1];
        let mut population = vec![0u64; nodes + 1];
        for index in 1..=nodes {
            let realm = ((index - 1) / 10 + 1) as u16;
            owner[index] = realm;
            assignment[index] = realm;
            population[index] = 100;
        }
        let roster = same_kind_faiths(4);
        consolidate_superregions(
            &mut assignment,
            &roster,
            &owner,
            &[SocietyTier::States; 4],
            &population,
            &hierarchy,
            &ReligionConfig::default(),
        );
        assert_eq!(
            faith_counts(&assignment).len(),
            4,
            "a settled superregion should keep its regional faiths"
        );
    }

    /// Run the schism pass on one forager faith and report how many faiths the
    /// roster holds afterwards.
    fn schism_forager(hierarchy: GeopoliticalHierarchy, cap: usize) -> usize {
        let nodes = hierarchy.provinces.len() - 1;
        let mut assignment = vec![0u16; nodes + 1];
        let mut owner = vec![0u16; nodes + 1];
        let mut population = vec![0u64; nodes + 1];
        for index in 1..=nodes {
            owner[index] = 1;
            assignment[index] = 1;
            population[index] = 100;
        }
        let landscapes = vec![landscape(); nodes + 1];
        let contexts = vec![context()];
        let mut roster = faiths(1);
        let mut origins = vec![(1u16, 1u32)];
        let mut config = ReligionConfig::default();
        config.schism_min_provinces = 10;
        config.schism_min_group = 5;
        config.faiths_per_tier[SocietyTier::HunterGatherers.index() as usize] = cap;
        let mut rng = ChaCha8Rng::seed_from_u64(41);
        schism_large_faiths(
            &mut roster,
            &mut origins,
            &[SocietyTier::HunterGatherers],
            &mut assignment,
            &owner,
            &population,
            &landscapes,
            &hierarchy,
            &contexts,
            &config,
            &mut rng,
        );
        roster.len()
    }

    #[test]
    fn schism_keeps_a_forager_faith_at_superregion_scale() {
        // Six regions inside three superregions: the family should follow the
        // superregions, so the parent and two daughters — not six faiths.
        let hierarchy = chain_grouped(60, 10, 20);
        assert_eq!(schism_forager(hierarchy, usize::MAX), 3);
    }

    #[test]
    fn schism_caps_daughters_to_the_tier_ceiling() {
        // Six superregions, but a forager people may keep only three faiths.
        let hierarchy = chain_grouped(60, 5, 10);
        assert_eq!(schism_forager(hierarchy, 3), 3);
    }

    fn ring(n: u16) -> BTreeMap<u16, Vec<u16>> {
        let mut adjacency: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
        for rank in 0..n {
            let next = (rank + 1) % n + 1;
            let previous = (rank + n - 1) % n + 1;
            adjacency.entry(rank + 1).or_default().push(next);
            adjacency.entry(rank + 1).or_default().push(previous);
        }
        adjacency
    }

    #[test]
    fn neighbouring_religions_of_one_root_never_share_a_shade() {
        for root in MetaphysicalRoot::ALL {
            let roots = vec![root; MetaphysicalRoot::SHADES];
            let adjacency = ring(MetaphysicalRoot::SHADES as u16);
            let colors = assign_religion_colors(&roots, &adjacency);
            for rank in 0..MetaphysicalRoot::SHADES as u16 {
                for &neighbour in &adjacency[&(rank + 1)] {
                    assert_ne!(
                        colors[rank as usize],
                        colors[neighbour as usize - 1],
                        "{} repeated a shade across a frontier",
                        root.label()
                    );
                }
            }
            assert_eq!(colors, assign_religion_colors(&roots, &adjacency));
        }
    }

    #[test]
    fn a_root_shade_is_always_free() {
        let n = MetaphysicalRoot::SHADES as u16;
        let mut adjacency: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
        for rank in 0..n {
            for other in 0..n {
                if other != rank {
                    adjacency.entry(rank + 1).or_default().push(other + 1);
                }
            }
        }
        let roots = vec![MetaphysicalRoot::Polytheism; MetaphysicalRoot::SHADES];
        let colors = assign_religion_colors(&roots, &adjacency);
        let mut seen = colors.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), MetaphysicalRoot::SHADES);
    }

    #[test]
    fn roots_keep_their_own_shades_beside_other_roots() {
        let roots = [
            MetaphysicalRoot::Monotheism,
            MetaphysicalRoot::Dualism,
            MetaphysicalRoot::Monotheism,
        ];
        let adjacency = BTreeMap::from([
            (1u16, vec![2u16, 3u16]),
            (2u16, vec![1u16, 3u16]),
            (3u16, vec![1u16, 2u16]),
        ]);
        let colors = assign_religion_colors(&roots, &adjacency);
        assert_ne!(
            colors[0], colors[2],
            "two neighbouring monotheisms shared a shade"
        );
        assert_eq!(colors[0], MetaphysicalRoot::Monotheism.shade(0));
        assert_eq!(colors[1], MetaphysicalRoot::Dualism.shade(0));
    }
}
