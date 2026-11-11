//! Stage 8 — Languages & Phonetics: the orchestration of a world's tongues.
//!
//! The passes run in a fixed order so a seed always produces the same languages:
//!
//! 1. measure every province (relief, river, climate, population);
//! 2. choose 10–18 Urheimats, preferring the seats of the civilizations that
//!    already exist so a language's homeland and a polity's cradle coincide;
//! 3. build each family's phonology and forty-root lexicon;
//! 4. diffuse the families across the province graph;
//! 5. split dense families into daughter languages and dialects;
//! 6. assign a primary language to every society;
//! 7. name realms, capitals and homelands from the roots;
//! 8. republish a renamed [`CivilizationWorld`] so every existing atlas panel
//!    reads native names without a special case.
//!
//! The stage never touches the RNG stream of the dawn of history: it draws from
//! `seed ^ LANGUAGE_SALT`, and each family from its own sub-stream. Stage 7's
//! measurements are therefore identical whether or not this act ever runs; only
//! the display names change.

use crate::civilization::{Age, Civilization, CivilizationWorld, SocietyTier};
use crate::core::progress::Progress;
use crate::core::{relief, sphere};
use crate::language::config::LanguageConfig;
use crate::language::diffusion::{build_profiles, diffuse_families, diffuse_languages};
use crate::language::families::{
    assign_family_colors, build_family, choose_urheimats, language_color,
};
use crate::language::lexicon::Concept;
use crate::language::naming::{
    NameRegistry, PolityKind, language_name, plain_root, qualified, realm_name, settlement_name,
};
use crate::language::phonology::SoundLaw;
use crate::language::{
    Language, LanguageContact, LanguageContactKind, LanguageFamily, LanguagesWorld,
};
use crate::pipeline::AtlasWorld;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::{BTreeMap, BTreeSet};

/// Salt mixed into the world seed for Act 8, deliberately independent of the
/// dawn's own salt so the two stages never share a random stream.
pub const LANGUAGE_SALT: u64 = 0x1A26_0F1C_0000_0008;

/// Grow the languages of one world and republish its civilizations with native
/// names.
pub fn generate_languages(
    atlas: &AtlasWorld,
    config: &LanguageConfig,
    seed: u64,
    progress: &Progress,
) -> (LanguagesWorld, CivilizationWorld) {
    let world = atlas.civilizations.as_ref();
    let renamed = world.clone();
    let Some(provinces) = atlas.provinces.as_ref() else {
        return (LanguagesWorld::empty(seed), renamed);
    };
    let hierarchy = atlas.hierarchy.as_ref();
    if hierarchy.provinces.is_empty() || provinces.num_provinces == 0 {
        return (LanguagesWorld::empty(seed), renamed);
    }
    let (Some(temperature), Some(precipitation), Some(flow), Some(is_lake)) = (
        atlas.temperature.as_ref(),
        atlas.precipitation.as_ref(),
        atlas.flow.as_ref(),
        atlas.is_lake.as_ref(),
    ) else {
        return (LanguagesWorld::empty(seed), renamed);
    };

    let width = atlas.width;
    let n = hierarchy.provinces.len();

    // ─── 1. PROVINCE SURVEY ──────────────────────────────────────────────────
    progress.sub_step("Surveying the province tongues", 0);
    let profiles = build_profiles(
        &atlas.heightmap,
        temperature,
        precipitation,
        flow,
        is_lake,
        &provinces.province_ids,
        hierarchy,
        &world.province_owner,
        &world.province_population,
        relief::SEA_LEVEL,
    );
    let inhabited = profiles.iter().filter(|profile| profile.present).count();

    // ─── 2. URHEIMATS ────────────────────────────────────────────────────────
    progress.sub_step("Choosing the Urheimats", 0);
    let preferred: BTreeSet<u32> = world
        .civilizations
        .iter()
        .filter(|civ| civ.tier == SocietyTier::States)
        .map(|civ| civ.seat_province)
        .collect();
    let seeds = choose_urheimats(&profiles, hierarchy, width, config, &preferred);
    if seeds.is_empty() {
        return (LanguagesWorld::empty(seed), renamed);
    }

    let age = world.age();
    let allow_sea = age >= Age::LADDER[(config.seafaring_age as usize).min(Age::LADDER.len() - 1)];
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ LANGUAGE_SALT);

    // ─── 3. FAMILIES AND THEIR PHONOLOGIES ───────────────────────────────────
    progress.sub_step("Giving the families their phonologies", 0);
    let mut families: Vec<LanguageFamily> = Vec::with_capacity(seeds.len());
    let mut family_seeds: Vec<(u32, u16)> = Vec::with_capacity(seeds.len());
    let mut family_registry = NameRegistry::new();
    for (index, seed) in seeds.iter().enumerate() {
        let id = index as u16 + 1;
        let family_stream = rng.random::<u64>();
        let mut family = build_family(id, seed, family_stream, age);
        let autonym = family_registry.unique_rooted(
            &family.autonym,
            &family.autonym.clone(),
            &family.lexicon,
        );
        let name =
            family_registry.unique_rooted(&autonym, &family.name.clone(), &family.lexicon);
        family.autonym = autonym;
        family.name = name;
        families.push(family);
        family_seeds.push((seed.province, id));
    }

    // ─── 4. MACRO-FAMILY DIFFUSION ───────────────────────────────────────────
    progress.sub_step("Spreading the families", 0);
    let cap = ((inhabited as f32) * config.family_cap_share).ceil() as usize;
    let caps = vec![cap.max(1); families.len() + 1];
    let province_family = diffuse_families(
        &family_seeds,
        &profiles,
        hierarchy,
        width,
        config,
        allow_sea,
        &caps,
    );

    let colors = assign_family_colors(&province_family, hierarchy, families.len());
    for family in &mut families {
        family.color = colors[family.id as usize];
    }
    progress.tick(family_seeds.len() as u64);

    // ─── 5. DAUGHTER LANGUAGES ───────────────────────────────────────────────
    progress.sub_step("Splitting the families into tongues", 0);
    let mut languages: Vec<Language> = Vec::new();
    let mut language_family: Vec<u16> = Vec::new();
    let mut language_seeds: Vec<(u32, u16)> = Vec::new();
    let mut family_languages: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    let mut language_registry = NameRegistry::new();

    for family in &families {
        if languages.len() >= config.max_languages {
            break;
        }
        let fid = family.id;
        let mut family_provinces: Vec<u32> = (1..n)
            .filter(|p| province_family.get(*p).copied().unwrap_or(0) == fid)
            .map(|p| p as u32)
            .collect();
        if family_provinces.is_empty() {
            continue;
        }
        family_provinces.sort_unstable();

        let proto_province = if province_family
            .get(family.urheimat.province as usize)
            .copied()
            .unwrap_or(0)
            == fid
        {
            family.urheimat.province
        } else {
            family_provinces[0]
        };

        let urban = family_provinces
            .iter()
            .filter(|p| {
                profiles
                    .get(**p as usize)
                    .is_some_and(|profile| profile.density_per_km2 >= config.urban_density_per_km2)
            })
            .count();
        let mut wanted = if urban >= config.dense_family_urban_provinces {
            config.languages_per_family_dense_min
        } else {
            1
        };
        wanted = wanted
            .max(1 + family_provinces.len() / 1_200)
            .min(config.languages_per_family_max)
            .min(family_provinces.len())
            .max(1);
        wanted = wanted.min(config.max_languages.saturating_sub(languages.len()).max(1));

        let mut chosen: Vec<u32> = vec![proto_province];
        let mut candidates = family_provinces.clone();
        candidates.sort_by(|a, b| {
            let pa = profiles.get(*a as usize).map_or(0, |p| p.population);
            let pb = profiles.get(*b as usize).map_or(0, |p| p.population);
            pb.cmp(&pa).then(a.cmp(b))
        });
        for candidate in &candidates {
            if chosen.len() >= wanted {
                break;
            }
            if chosen.contains(candidate) {
                continue;
            }
            let min_separation = width as f32 * 0.012;
            let far_enough = chosen.iter().all(|existing| {
                let a = &profiles[*existing as usize];
                let b = &profiles[*candidate as usize];
                sphere::wrap_distance_px(a.center_x, a.center_y, b.center_x, b.center_y, width)
                    >= min_separation
            });
            if far_enough {
                chosen.push(*candidate);
            }
        }
        for candidate in &family_provinces {
            if chosen.len() >= wanted {
                break;
            }
            if !chosen.contains(candidate) {
                chosen.push(*candidate);
            }
        }

        // The proto-language: the family's own voice, at its Urheimat.
        let proto_id = languages.len() as u16 + 1;
        let proto_name = language_registry.unique_rooted(
            &family.name,
            &family.name.clone(),
            &family.lexicon,
        );
        let proto_autonym = language_registry.unique_rooted(
            &family.autonym,
            &family.autonym.clone(),
            &family.lexicon,
        );
        languages.push(Language {
            id: proto_id,
            family: fid,
            name: proto_name,
            autonym: proto_autonym,
            color: family.color,
            phonology: family.phonology.clone(),
            lexicon: family.lexicon.clone(),
            parent: None,
            depth: 0,
            sound_laws: Vec::new(),
            urheimat: proto_province,
            province_count: 0,
            population: 0,
        });
        language_family.push(fid);
        language_seeds.push((proto_province, proto_id));
        family_languages.entry(fid).or_default().push(proto_id);

        // Daughters: one sound law each, one qualifier each, one province each.
        for (rank, province) in chosen.iter().enumerate().skip(1) {
            let law = SoundLaw::ALL[(rng.random::<u32>() as usize) % SoundLaw::ALL.len()];
            let daughter_stream = rng.random::<u64>();
            let dna = family.phonology.daughter(daughter_stream, law);
            let mut lexicon = family.lexicon.clone();
            lexicon.apply_law(law);
            let qualifier = language_qualifier(rank);
            let id = languages.len() as u16 + 1;
            let daughter_name = language_registry.unique_rooted(
                &family.name,
                &language_name(&lexicon, &family.name, Some(qualifier)),
                &lexicon,
            );
            let daughter_autonym = language_registry.unique_rooted(
                &family.autonym,
                &qualified(&lexicon, &family.autonym, qualifier),
                &lexicon,
            );
            languages.push(Language {
                id,
                family: fid,
                name: daughter_name,
                autonym: daughter_autonym,
                color: language_color(family.color, 1),
                phonology: dna,
                lexicon,
                parent: Some(proto_id),
                depth: 1,
                sound_laws: vec![law],
                urheimat: *province,
                province_count: 0,
                population: 0,
            });
            language_family.push(fid);
            language_seeds.push((*province, id));
            family_languages.entry(fid).or_default().push(id);
        }
    }

    // ─── 6. LANGUAGE DIFFUSION ───────────────────────────────────────────────
    progress.sub_step("Settling the dialects", 0);
    let province_language = diffuse_languages(
        &language_seeds,
        &language_family,
        &province_family,
        &profiles,
        hierarchy,
        width,
        config,
        allow_sea,
    );

    let mut language_provinces = vec![0usize; languages.len()];
    let mut language_population = vec![0u64; languages.len()];
    let mut fam_provinces = vec![0usize; families.len()];
    let mut fam_population = vec![0u64; families.len()];
    for province in 1..n {
        if !profiles[province].present {
            continue;
        }
        let language_id = province_language.get(province).copied().unwrap_or(0);
        if language_id != 0 {
            let index = language_id as usize - 1;
            if index < languages.len() {
                language_provinces[index] += 1;
                language_population[index] += profiles[province].population;
            }
        }
        let family_id = province_family.get(province).copied().unwrap_or(0);
        if family_id != 0 {
            let index = family_id as usize - 1;
            if index < families.len() {
                fam_provinces[index] += 1;
                fam_population[index] += profiles[province].population;
            }
        }
    }
    for (index, language) in languages.iter_mut().enumerate() {
        language.province_count = language_provinces[index];
        language.population = language_population[index];
    }
    for (index, family) in families.iter_mut().enumerate() {
        family.province_count = fam_provinces[index];
        family.population = fam_population[index];
        family.languages = family_languages
            .get(&family.id)
            .cloned()
            .unwrap_or_default();
    }

    // ─── 7. EVERY SOCIETY GETS A PRIMARY TONGUE ──────────────────────────────
    progress.sub_step("Teaching the societies to speak", 0);
    let mut language_of_realm = vec![0u16; world.civilizations.len()];
    let mut family_of_realm = vec![0u16; world.civilizations.len()];
    for (index, civ) in world.civilizations.iter().enumerate() {
        let language = primary_language(civ, &province_language, &world.province_population);
        language_of_realm[index] = language;
        family_of_realm[index] = language_family
            .get(language as usize - 1)
            .copied()
            .unwrap_or(0);
    }

    // ─── 8. NAMES ────────────────────────────────────────────────────────────
    progress.sub_step("Naming the realms", 0);
    let mut registry = NameRegistry::new();
    let mut realm_names = vec![String::new(); world.civilizations.len()];
    for (index, civ) in world.civilizations.iter().enumerate() {
        let language = language_of_realm[index]
            .checked_sub(1)
            .and_then(|i| languages.get(i as usize));
        let Some(language) = language else {
            realm_names[index] = civ.name.clone();
            continue;
        };
        // The plain root, not the tongue's own name: a daughter's autonym already
        // wears its dialect marker, and a realm retried from that string would
        // stutter the marker (`K'lirk'lirmopa`).
        let base = plain_root(&language.lexicon, &language.autonym);
        let candidate = realm_name(&language.lexicon, &base, polity_kind(civ.tier));
        let chosen = if candidate.is_empty() {
            base.clone()
        } else {
            candidate
        };
        realm_names[index] = registry.unique_rooted(&base, &chosen, &language.lexicon);
    }

    let mut province_names = vec![String::new(); n];
    for family in &families {
        let index = family.urheimat.province as usize;
        if index < n && province_names[index].is_empty() {
            province_names[index] = registry.unique_rooted(
                &family.urheimat.name,
                &family.urheimat.name.clone(),
                &family.lexicon,
            );
        }
    }
    for (index, civ) in world.civilizations.iter().enumerate() {
        let province = civ.seat_province as usize;
        if province >= n {
            continue;
        }
        let language = language_of_realm[index]
            .checked_sub(1)
            .and_then(|i| languages.get(i as usize));
        let Some(language) = language else {
            continue;
        };
        if province_names[province].is_empty() {
            province_names[province] = registry.unique(
                &settlement_name(&language.lexicon, Concept::Hearth),
                &language.lexicon,
            );
        }
    }

    // ─── 9. CONTACT AND BORROWING ────────────────────────────────────────────
    progress.sub_step("Listening across the frontiers", 0);
    let mut language_pairs: BTreeSet<(u16, u16)> = BTreeSet::new();
    for node in &hierarchy.provinces {
        let id = node.id as usize;
        if id == 0 || id >= n {
            continue;
        }
        let a = province_language.get(id).copied().unwrap_or(0);
        if a == 0 {
            continue;
        }
        for &(neighbor, _) in &node.neighbors {
            let nb = neighbor as usize;
            if nb >= n {
                continue;
            }
            let b = province_language.get(nb).copied().unwrap_or(0);
            if b == 0 || b == a {
                continue;
            }
            language_pairs.insert((a.min(b), a.max(b)));
        }
    }
    let mut contacts = Vec::new();
    for (a, b) in language_pairs {
        if contacts.len() >= 4_000 {
            break;
        }
        let family_a = language_family.get(a as usize - 1).copied().unwrap_or(0);
        let family_b = language_family.get(b as usize - 1).copied().unwrap_or(0);
        let kind = if family_a == family_b {
            LanguageContactKind::Frontier
        } else {
            LanguageContactKind::Trade
        };
        let mut borrowed = Vec::new();
        let donor = languages.get(b as usize - 1).cloned();
        if let (Some(donor), Some(recipient)) = (donor, languages.get_mut(a as usize - 1)) {
            for step in 0..config.max_borrowings.min(Concept::CORE.len()) {
                let concept = Concept::CORE[(a as usize + b as usize + step) % Concept::CORE.len()];
                let form = donor.lexicon.form(concept).to_string();
                recipient.lexicon.borrow(concept, &form);
                borrowed.push(concept);
            }
        }
        contacts.push(LanguageContact {
            a,
            b,
            kind,
            borrowed,
        });
    }

    // ─── 10. REPUBLISH THE CIVILIZATIONS WITH THEIR OWN NAMES ────────────────
    progress.sub_step("Writing the gazetteer", 0);
    let languages_world = LanguagesWorld {
        seed,
        families,
        languages,
        province_language,
        province_family,
        language_of_realm,
        family_of_realm,
        realm_names,
        province_names,
        contacts,
    };
    let renamed = apply_language_names(world, &languages_world, width, config);
    (languages_world, renamed)
}

/// Republish a civilization world with the names and kin its languages gave it.
///
/// Stage 8 never mutates the dawn's published world: it clones it and replaces
/// only the display strings, the family label and the kin list. Every other
/// measurement — seat, provinces, population, contacts, age — is untouched.
pub fn apply_language_names(
    civilizations: &CivilizationWorld,
    languages: &LanguagesWorld,
    width: usize,
    config: &LanguageConfig,
) -> CivilizationWorld {
    let mut renamed = civilizations.clone();
    let seats: Vec<(f32, f32, SocietyTier)> = renamed
        .civilizations
        .iter()
        .map(|civ| (civ.seat_x, civ.seat_y, civ.tier))
        .collect();
    let kin_of: Vec<Vec<u32>> = (0..seats.len())
        .map(|index| {
            let family_id = languages.family_of_realm.get(index).copied().unwrap_or(0);
            let mut kin: Vec<(f32, u32)> = seats
                .iter()
                .enumerate()
                .filter(|(other, candidate)| {
                    *other != index
                        && family_id != 0
                        && languages.family_of_realm.get(*other).copied().unwrap_or(0) == family_id
                        && candidate.2 == seats[index].2
                })
                .map(|(other, candidate)| {
                    (
                        sphere::wrap_distance_px(
                            seats[index].0,
                            seats[index].1,
                            candidate.0,
                            candidate.1,
                            width,
                        ),
                        other as u32 + 1,
                    )
                })
                .collect();
            kin.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.1.cmp(&b.1))
            });
            kin.truncate(config.contact_limit);
            kin.into_iter().map(|(_, id)| id).collect()
        })
        .collect();
    for (index, civ) in renamed.civilizations.iter_mut().enumerate() {
        if let Some(name) = languages.realm_names.get(index) {
            civ.name = name.clone();
        }
        let family_id = languages.family_of_realm.get(index).copied().unwrap_or(0);
        if let Some(family) = languages.family(family_id) {
            civ.language_family = family.name.clone();
        }
        civ.kin = kin_of[index].clone();
    }
    renamed
}

/// The polity suffix a tier earns.
fn polity_kind(tier: SocietyTier) -> PolityKind {
    match tier {
        SocietyTier::States => PolityKind::State,
        SocietyTier::ComplexFarmers => PolityKind::Chiefdom,
        _ => PolityKind::People,
    }
}

/// Which of a society's languages most of its people actually speak.
fn primary_language(
    civ: &Civilization,
    province_language: &[u16],
    province_population: &[u64],
) -> u16 {
    let mut tally: BTreeMap<u16, u64> = BTreeMap::new();
    for &province in &civ.provinces {
        let index = province as usize;
        let language = province_language.get(index).copied().unwrap_or(0);
        if language == 0 {
            continue;
        }
        let population = province_population.get(index).copied().unwrap_or(0).max(1);
        *tally.entry(language).or_insert(0) += population;
    }
    if let Some((language, _)) = tally.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) {
        return *language;
    }
    // A society with no counted provinces falls back to its seat.
    province_language
        .get(civ.seat_province as usize)
        .copied()
        .unwrap_or(0)
}

/// The roots a daughter language may use as a qualifier, in a fixed order.
fn language_qualifier(rank: usize) -> Concept {
    const QUALIFIERS: [Concept; 6] = [
        Concept::Upper,
        Concept::Lower,
        Concept::Deep,
        Concept::Great,
        Concept::New,
        Concept::Old,
    ];
    QUALIFIERS[rank % QUALIFIERS.len()]
}
