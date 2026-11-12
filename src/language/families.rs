//! Urheimats: where a family was born, and what it sounds like.
//!
//! The placement is a geography problem, not a lottery. A candidate province is
//! scored for the things that actually cradle a language — a reliable river, a
//! coast or lake, a climate people can live in, a population to carry the speech,
//! and ground low enough to cross — and the roster is drawn from the best of
//! them, spaced across the world so no two Urheimats sit on top of each other.
//!
//! The sound archetype follows the country: a dry plain produces a Guttural
//! family, a coast a Melodic one, a cold forest a Sibilant one, a river valley a
//! Nasal one, a high valley a Glottal one.

use crate::geopolitics::GeopoliticalHierarchy;
use crate::language::diffusion::ProvinceProfile;
use crate::language::lexicon::Lexicon;
use crate::language::naming::{autonym, family_name, urheimat_name};
use crate::language::phonology::{PhonologyDna, SoundArchetype};
use crate::language::{LanguageFamily, Urheimat};
use std::collections::BTreeSet;

/// A province chosen as a family's homeland, with the score that won it.
#[derive(Clone, Copy, Debug)]
pub struct FamilySeed {
    pub province: u32,
    pub x: f32,
    pub y: f32,
    pub continent: u32,
    pub score: f32,
    pub archetype: SoundArchetype,
}

/// Pick the world's Urheimats.
pub fn choose_urheimats(
    profiles: &[ProvinceProfile],
    hierarchy: &GeopoliticalHierarchy,
    width: usize,
    config: &crate::language::LanguageConfig,
    preferred: &BTreeSet<u32>,
) -> Vec<FamilySeed> {
    let mut candidates: Vec<FamilySeed> = Vec::new();
    for (index, profile) in profiles.iter().enumerate() {
        if index == 0 || !profile.present || profile.area_km2 < 250.0 {
            continue;
        }
        if profile.mean_temp_c < -2.0 {
            continue;
        }
        let node = &hierarchy.provinces[index];
        let climate = climate_score(profile);
        // A province threaded by a great river is a likely cradle: Nile-class
        // discharge (~3 000 m³/s) fills the score.
        let river = (profile.max_flow / 3_000.0).clamp(0.0, 1.0);
        let water = if profile.coastal || profile.lakeshore {
            1.0
        } else {
            0.0
        };
        let population = (profile.density_per_km2 / 3.0).clamp(0.0, 1.0);
        let relief = (1.0 - profile.mean_elevation_m / 3_500.0).clamp(0.0, 1.0);
        let preferred_bonus = if preferred.contains(&(index as u32)) {
            0.22
        } else {
            0.0
        };
        let score = 0.30 * climate
            + 0.24 * river
            + 0.14 * water
            + 0.14 * population
            + 0.10 * relief
            + preferred_bonus;
        candidates.push(FamilySeed {
            province: index as u32,
            x: node.center_x,
            y: node.center_y,
            continent: node.continent_id,
            score,
            archetype: archetype_for(profile),
        });
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.province.cmp(&b.province))
    });

    let wanted = config
        .max_families
        .min(candidates.len())
        .max(config.min_families.min(candidates.len()));
    let mut picked: Vec<FamilySeed> = Vec::with_capacity(wanted);
    let mut per_continent: std::collections::BTreeMap<u32, usize> =
        std::collections::BTreeMap::new();

    // Three passes, each with a looser spacing floor: a crowded world still gets
    // its minimum number of families rather than a single crowded one.
    for spacing in [
        config.family_separation_frac,
        config.family_separation_frac * 0.5,
        config.family_floor_frac,
    ] {
        if picked.len() >= wanted {
            break;
        }
        for candidate in &candidates {
            if picked.len() >= wanted {
                break;
            }
            if picked.iter().any(|p| p.province == candidate.province) {
                continue;
            }
            if per_continent
                .get(&candidate.continent)
                .copied()
                .unwrap_or(0)
                >= config.max_families_per_continent
            {
                continue;
            }
            let too_close = picked.iter().any(|p| {
                crate::core::sphere::wrap_distance_px(p.x, p.y, candidate.x, candidate.y, width)
                    < spacing * width as f32
            });
            if too_close {
                continue;
            }
            picked.push(*candidate);
            *per_continent.entry(candidate.continent).or_insert(0) += 1;
        }
    }

    picked.sort_by(|a, b| a.province.cmp(&b.province));
    picked.truncate(config.max_families);
    picked
}

fn climate_score(profile: &ProvinceProfile) -> f32 {
    let warmth = if profile.mean_temp_c < 2.0 {
        0.25
    } else if profile.mean_temp_c > 30.0 {
        0.55
    } else {
        1.0
    };
    let rain = if profile.mean_precip < 0.07 {
        0.25
    } else if profile.mean_precip < 0.14 {
        0.60
    } else {
        1.0
    };
    warmth * rain
}

/// The sound a country gives a family.
pub fn archetype_for(profile: &ProvinceProfile) -> SoundArchetype {
    if profile.mean_elevation_m >= 1_400.0 {
        return SoundArchetype::GlottalHighland;
    }
    // A major river (m³/s) gives a family a riverine voice.
    if profile.max_flow >= 1_500.0 {
        return SoundArchetype::NasalRiverine;
    }
    if profile.mean_temp_c < 7.0 && profile.mean_precip >= 0.20 {
        return SoundArchetype::SibilantTaiga;
    }
    if profile.mean_precip < 0.14 || profile.mean_temp_c < 2.0 {
        return SoundArchetype::GutturalArid;
    }
    if profile.coastal || profile.lakeshore {
        return SoundArchetype::MelodicMaritime;
    }
    SoundArchetype::SonorousWoodland
}

/// Build the proto-language of one family.
pub fn build_family(
    id: u16,
    seed: &FamilySeed,
    family_seed: u64,
    age: crate::civilization::Age,
) -> LanguageFamily {
    let dna = PhonologyDna::build(seed.archetype, family_seed);
    let lexicon = Lexicon::build(&dna);
    let autonym = autonym(&lexicon);
    let name = family_name(&lexicon, &autonym);
    let homeland = urheimat_name(&lexicon, seed.archetype);
    LanguageFamily {
        id,
        name,
        autonym,
        color: [128, 128, 128], // assigned after diffusion
        phonology: dna,
        lexicon,
        urheimat: Urheimat {
            province: seed.province,
            x: seed.x,
            y: seed.y,
            name: homeland,
            archetype: seed.archetype,
        },
        parent: None,
        age,
        languages: Vec::new(),
        province_count: 0,
        population: 0,
    }
}

/// A curated palette of family hues: twenty-four categorical colours spread
/// around the whole wheel.
///
/// The palette is deliberately *categorical* rather than terrain-safe. A family's
/// ground is drawn on land, and land is already separated from the water by its
/// coastline, its relief shading and its hillshade, so a saturated green, teal or
/// violet reads as a country, not as a sea. The colours are far enough apart in
/// RGB that no two families read as the same hue, and the first colour a family
/// may wear is the one the fewest families — itself never a neighbour — already
/// wear, so a world of a dozen families shows a dozen different hues rather than a
/// dozen warm ones.
pub const FAMILY_PALETTE: [[u8; 3]; 24] = [
    [17, 158, 102],  // emerald green
    [28, 96, 52],    // forest green
    [104, 40, 148],  // royal purple
    [150, 88, 214],  // violet
    [14, 150, 148],  // teal
    [62, 196, 190],  // turquoise
    [204, 40, 132],  // magenta
    [240, 106, 82],  // coral
    [96, 122, 150],  // slate
    [235, 162, 30],  // amber
    [176, 118, 32],  // ochre
    [186, 30, 54],   // crimson
    [226, 140, 162], // rose
    [108, 120, 40],  // olive
    [56, 58, 152],   // indigo
    [226, 194, 78],  // gold
    [44, 148, 196],  // azure
    [126, 42, 96],   // plum
    [40, 176, 130],  // jade
    [186, 84, 52],   // terracotta
    [242, 176, 120], // apricot
    [186, 150, 210], // mauve
    [92, 134, 70],   // moss
    [94, 60, 108],   // raisin
];

/// Give every family a colour, adjacent families never the same one.
///
/// The greedy pass walks the families in id order and, for each, excludes the
/// hues its already-coloured neighbours wear, then takes the *least used* of what
/// is left. Excluding neighbours is the graph-colouring guarantee — no shared
/// border ever divides two patches of one hue; taking the least used of the rest
/// is what makes the world categorical rather than warm, because ten unrelated
/// families no longer all reach for the first colour in the list.
pub fn assign_family_colors(
    province_family: &[u16],
    hierarchy: &GeopoliticalHierarchy,
    family_count: usize,
) -> Vec<[u8; 3]> {
    let mut adjacency: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); family_count + 1];
    for node in &hierarchy.provinces {
        let id = node.id as usize;
        if id == 0 || id >= province_family.len() {
            continue;
        }
        let family = province_family[id];
        if family == 0 {
            continue;
        }
        for &(neighbor, _) in &node.neighbors {
            let nb = neighbor as usize;
            if nb >= province_family.len() {
                continue;
            }
            let other = province_family[nb];
            if other != 0 && other != family {
                adjacency[family as usize].insert(other);
                adjacency[other as usize].insert(family);
            }
        }
    }

    let mut colors = vec![[128u8, 128u8, 128u8]; family_count + 1];
    let mut usage = vec![0usize; FAMILY_PALETTE.len()];
    for family in 1..=family_count {
        let mut taken: BTreeSet<[u8; 3]> = BTreeSet::new();
        for neighbor in &adjacency[family] {
            if *neighbor < family as u16 {
                taken.insert(colors[*neighbor as usize]);
            }
        }
        let pick = (0..FAMILY_PALETTE.len())
            .filter(|index| !taken.contains(&FAMILY_PALETTE[*index]))
            .min_by_key(|index| (usage[*index], *index))
            .unwrap_or((family - 1) % FAMILY_PALETTE.len());
        colors[family] = FAMILY_PALETTE[pick];
        usage[pick] += 1;
    }
    colors
}

/// A daughter language wears its family's hue, a shade darker per generation.
pub fn language_color(family: [u8; 3], depth: u8) -> [u8; 3] {
    let factor = (1.0 - 0.075 * depth as f32).clamp(0.62, 1.0);
    [
        (family[0] as f32 * factor).clamp(0.0, 255.0) as u8,
        (family[1] as f32 * factor).clamp(0.0, 255.0) as u8,
        (family[2] as f32 * factor).clamp(0.0, 255.0) as u8,
    ]
}

/// Perceptual-ish RGB distance, used to prove the curated palette's colours are
/// far enough apart. Exact CIE is unnecessary here: the palette is hand-picked,
/// and this catches a duplicate at a glance.
#[cfg(test)]
pub fn rgb_distance(a: [u8; 3], b: [u8; 3]) -> f32 {
    let dr = a[0] as f32 - b[0] as f32;
    let dg = a[1] as f32 - b[1] as f32;
    let db = a[2] as f32 - b[2] as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geopolitics::{GeopoliticalHierarchy, ProvinceNode};

    /// Hue in degrees, for the wheel-coverage test. The palette is a categorical
    /// one, so what matters is that it visits every sector, not that the numbers
    /// are evenly spaced.
    fn hue(colour: [u8; 3]) -> f32 {
        let (r, g, b) = (
            colour[0] as f32 / 255.0,
            colour[1] as f32 / 255.0,
            colour[2] as f32 / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let chroma = max - min;
        if chroma <= f32::EPSILON {
            0.0
        } else if max == r {
            60.0 * ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / chroma + 2.0)
        } else {
            60.0 * ((r - g) / chroma + 4.0)
        }
    }

    /// A hierarchy of `count` provinces, each its own node, wired into a ring so
    /// every province has two neighbours.
    fn ring(count: u32) -> GeopoliticalHierarchy {
        let provinces = (0..=count)
            .map(|id| ProvinceNode {
                id,
                center_x: id as f32,
                center_y: 0.0,
                pixel_count: 1,
                neighbors: if id == 0 {
                    Vec::new()
                } else {
                    let left = if id == 1 { count } else { id - 1 };
                    let right = if id == count { 1 } else { id + 1 };
                    vec![(left, 1), (right, 1)]
                },
                area_id: 1,
                region_id: 1,
                superregion_id: 1,
                continent_id: 1,
                anchor_id: 1,
            })
            .collect();
        GeopoliticalHierarchy {
            total_provinces: count as usize,
            total_areas: 1,
            total_regions: 1,
            total_superregions: 1,
            total_continents: 1,
            provinces,
        }
    }

    #[test]
    fn every_palette_colour_is_visibly_distinct_from_its_siblings() {
        for (a, first) in FAMILY_PALETTE.iter().enumerate() {
            for (b, second) in FAMILY_PALETTE.iter().enumerate().skip(a + 1) {
                assert!(
                    rgb_distance(*first, *second) > 30.0,
                    "palette colours {a} and {b} are too close"
                );
            }
        }
    }

    /// The palette spans the wheel: every sixty-degree sector has a colour, so a
    /// map cannot come out in shades of one warm arc.
    #[test]
    fn the_palette_covers_every_sector_of_the_wheel() {
        let mut sectors = [0usize; 6];
        for colour in FAMILY_PALETTE {
            sectors[(hue(colour) / 60.0) as usize % 6] += 1;
        }
        for (sector, count) in sectors.iter().enumerate() {
            assert!(
                *count > 0,
                "the {}°–{}° sector of the wheel has no colour",
                sector * 60,
                sector * 60 + 60
            );
        }
    }

    /// A family never wears a neighbour's hue, and unrelated families are spread
    /// across the palette rather than piled onto its first colour.
    #[test]
    fn neighbouring_families_never_share_a_hue() {
        let hierarchy = ring(14);
        let province_family: Vec<u16> = (0..=14).map(|id| if id == 0 { 0 } else { id }).collect();
        let colors = assign_family_colors(&province_family, &hierarchy, 14);
        for node in &hierarchy.provinces {
            if node.id == 0 {
                continue;
            }
            for (neighbor, _) in &node.neighbors {
                assert_ne!(
                    colors[node.id as usize],
                    colors[*neighbor as usize],
                    "province {} and its neighbour {neighbor} share a hue",
                    node.id
                );
            }
        }
        let distinct: BTreeSet<[u8; 3]> = colors[1..].iter().copied().collect();
        assert_eq!(
            distinct.len(),
            14,
            "a ring of fourteen families should show fourteen hues"
        );
    }

    #[test]
    fn a_daughter_is_a_shade_of_its_family() {
        let base = FAMILY_PALETTE[0];
        let deep = language_color(base, 4);
        assert!(deep[0] <= base[0] && deep[1] <= base[1] && deep[2] <= base[2]);
        assert!(rgb_distance(base, deep) < 120.0);
    }
}
