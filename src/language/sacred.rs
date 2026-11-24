//! The sacred onomasticon: the words a faith, its deities and its emblem speak.
//!
//! Act seven writes a faith as a cascade of enums. This module reads that
//! cascade through a language's own roots and compounds, so a realm's religion
//! is a set of words its people could actually say. The vocabulary is
//! compositional: a domain, a spirit and an emblem are short recipes of
//! [`Concept`]s, joined by the same sandhi as a realm name.
//!
//! Only the faith's own name, the native word for each domain, the name of each
//! deity/spirit/principle/teacher, the core of an abstract faith and the emblem
//! are named. Creed, cosmology, ethos, path, rites, temple and manifestation
//! stay English.
//!
//! Nothing here draws from the history act's main stream. A per-realm stream
//! seeded from `seed ^ FAITH_NAME_SALT` picks the few optional epithets, and the
//! shared [`NameRegistry`] makes faith, cult and sacred-site names unique.

use crate::civilization::faith::{
    Clergy, DivineGender, Divinity, Domain, DomainCategory, Emblem, Faith, FaithNames,
    FaithRecension, MetaphysicalRoot, NontheistConcept, NontheistFocus, NontheistPerson,
    SacredLocus, SacredSite, SpiritKind, StateCoupling, Transmission,
};
use crate::civilization::{CivilizationWorld, SocietyTier};
use crate::language::LanguagesWorld;
use crate::language::diffusion::ProvinceProfile;
use crate::language::lexicon::{BridgeRule, Concept, Lexicon};
use crate::language::naming::{NameRegistry, tidy_name};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

/// Salt mixed into the world seed for the faith-naming stream.
pub const FAITH_NAME_SALT: u64 = 0x0FA1_7A11_0000_0009;

// ─── RECIPES ────────────────────────────────────────────────────────────────

fn domain_recipe(domain: Domain) -> &'static [Concept] {
    use Domain::*;
    match domain {
        Sky => &[Concept::Sky],
        Solar => &[Concept::Sun],
        Lunar => &[Concept::Moon],
        Astral => &[Concept::Star],
        Storm => &[Concept::Storm],
        Light => &[Concept::Bright],
        Shadow => &[Concept::Dark],
        Eclipse => &[Concept::Sun, Concept::Night],
        Earth => &[Concept::Earth],
        Water => &[Concept::River],
        Ocean => &[Concept::Sea],
        Fire => &[Concept::Fire],
        Wind => &[Concept::Wind],
        Frost => &[Concept::Ice],
        Volcano => &[Concept::Mountain, Concept::Fire],
        Flora => &[Concept::Forest],
        Fauna => &[Concept::Beast],
        Decay => &[Concept::Ash],
        Ancestor => &[Concept::Elder],
        War => &[Concept::War],
        Sovereignty => &[Concept::Crown],
        Fertility => &[Concept::Harvest],
        Underworld => &[Concept::Deep],
        Death => &[Concept::Dead],
        Hunt => &[Concept::Hunt],
        Craft => &[Concept::Craft],
        Trade => &[Concept::Road],
        Law => &[Concept::Law],
        Wisdom => &[Concept::Wisdom],
        Healing => &[Concept::Healing],
        Hearth => &[Concept::Hearth],
        CosmicOrder => &[Concept::Order],
        Transcendence => &[Concept::Beyond],
        Fate => &[Concept::Fate],
        Chaos => &[Concept::Chaos],
        Harmony => &[Concept::Harmony],
        Cycle => &[Concept::Wheel],
        Entropy => &[Concept::Ruin],
        Truth => &[Concept::Truth],
    }
}

fn root_recipe(root: MetaphysicalRoot) -> &'static [Concept] {
    use MetaphysicalRoot::*;
    match root {
        Monotheism => &[Concept::One],
        Dualism => &[Concept::Twin],
        Henotheism => &[Concept::Upper],
        Polytheism => &[Concept::Many],
        Animism => &[Concept::Soul],
        Pantheism => &[Concept::Earth],
        Panentheism => &[Concept::Deep],
        Deism => &[Concept::Sky],
        Nontheism => &[Concept::Truth],
    }
}

fn recension_concept(recension: FaithRecension) -> Concept {
    match recension {
        FaithRecension::Reformed => Concept::New,
        FaithRecension::Old => Concept::Old,
        FaithRecension::High => Concept::Upper,
        FaithRecension::Low => Concept::Lower,
        FaithRecension::True => Concept::First,
    }
}

fn locus_recipe(locus: SacredLocus) -> &'static [Concept] {
    use SacredLocus::*;
    match locus {
        Peak => &[Concept::Mountain],
        RiverConfluence => &[Concept::River, Concept::Middle],
        Spring => &[Concept::River, Concept::Small],
        Caldera => &[Concept::Mountain, Concept::Fire],
        Oasis => &[Concept::River, Concept::Plain],
        Strait => &[Concept::Sea, Concept::Small],
        Crater => &[Concept::Stone, Concept::Deep],
        AbyssalCoast => &[Concept::Sea, Concept::Deep],
        Monolith => &[Concept::Stone, Concept::Great],
        Grove => &[Concept::Forest],
        Necropolis => &[Concept::Dead, Concept::Stone],
        Observatory => &[Concept::Star, Concept::Eye],
    }
}

fn emblem_recipe(emblem: Emblem) -> &'static [Concept] {
    use Emblem::*;
    match emblem {
        Disk => &[Concept::Sun],
        Crescent => &[Concept::Moon],
        Star => &[Concept::Star],
        Spiral => &[Concept::Wheel],
        Wave => &[Concept::Sea],
        Flame => &[Concept::Fire],
        Mountain => &[Concept::Mountain],
        Tree => &[Concept::Forest],
        Serpent => &[Concept::Serpent],
        Sword => &[Concept::War],
        Eye => &[Concept::Eye],
        Wheel => &[Concept::Wheel],
        Void => &[Concept::Void],
    }
}

// ─── NAMING ──────────────────────────────────────────────────────────────────

/// Join a recipe with the language's own sandhi and shape it like a name.
fn say(lex: &Lexicon, parts: &[Concept], bridge: BridgeRule, suffix: Option<&str>) -> String {
    if parts.is_empty() {
        return String::new();
    }
    let candidate = lex.compound_with(parts, suffix, bridge);
    let tidy = tidy_name(&candidate);
    if tidy.is_empty() {
        tidy_name(lex.form(Concept::Sacred))
    } else {
        tidy
    }
}

fn say_plural(lex: &Lexicon, parts: &[Concept]) -> String {
    let suffix = match lex.style {
        crate::language::lexicon::NamingStyle::Genitive => lex.genitive_suffix.as_deref(),
        crate::language::lexicon::NamingStyle::Augmentative => lex.augmentative_suffix.as_deref(),
        _ => lex.plural_suffix.as_deref(),
    };
    say(lex, parts, BridgeRule::Direct, suffix)
}

fn say_phrase(lex: &Lexicon, parts: &[Concept]) -> String {
    say(lex, parts, BridgeRule::Direct, None)
}

fn subject_parts(faith: &Faith) -> Vec<Concept> {
    match faith.divinity() {
        // A nontheist tradition is named for its teacher or its principle, not
        // for a domain it does not hold as a god.
        Divinity::Nontheist(_) => divine_head(faith.divinity()).to_vec(),
        _ => match faith.primary_domains().first().copied() {
            Some(domain) => domain_recipe(domain).to_vec(),
            None => root_recipe(faith.root).to_vec(),
        },
    }
}

fn push_unique(parts: &mut Vec<Concept>, concept: Concept) {
    if !parts.contains(&concept) {
        parts.push(concept);
    }
}

/// The native name of a faith and the plain base a retry rebuilds from.
fn faith_name(lex: &Lexicon, faith: &Faith, rng: &mut ChaCha8Rng) -> (String, String) {
    use crate::civilization::faith::NamingPattern::*;
    let subject = subject_parts(faith);
    let mut parts: Vec<Concept> = match faith.naming {
        DomainOrder => {
            let mut parts = subject.clone();
            push_unique(&mut parts, Concept::Order);
            parts
        }
        RootEpithet => {
            const EPITHETS: [Concept; 6] = [
                Concept::Oath,
                Concept::Order,
                Concept::Path,
                Concept::Word,
                Concept::Law,
                Concept::Song,
            ];
            let pick = EPITHETS[(rng.random::<u32>() as usize) % EPITHETS.len()];
            let mut parts = subject.clone();
            push_unique(&mut parts, pick);
            parts
        }
        FounderPath => {
            let mut parts = subject.clone();
            push_unique(&mut parts, Concept::Path);
            parts
        }
        LocusCovenant => {
            let mut parts = locus_recipe(faith.locus).to_vec();
            push_unique(&mut parts, Concept::Oath);
            parts
        }
        DualPolarity => {
            let first = faith
                .primary_domains()
                .first()
                .copied()
                .unwrap_or(Domain::Light);
            let second = faith
                .primary_domains()
                .get(1)
                .copied()
                .unwrap_or(Domain::Shadow);
            let mut parts = domain_recipe(first).to_vec();
            for concept in domain_recipe(second) {
                push_unique(&mut parts, *concept);
            }
            parts
        }
    };
    if let Some(recension) = faith.recension {
        parts.insert(0, recension_concept(recension));
    }

    let bridge = match faith.naming {
        FounderPath | LocusCovenant => BridgeRule::Genitive,
        _ => BridgeRule::Direct,
    };
    let suffix = if bridge == BridgeRule::Genitive {
        lex.genitive_suffix.as_deref()
    } else {
        None
    };
    let candidate = say(lex, &parts, bridge, suffix);

    let mut base_parts = subject.clone();
    push_unique(&mut base_parts, Concept::Order);
    let base = say(lex, &base_parts, BridgeRule::Direct, None);
    (candidate, base)
}

/// The concepts that stand for one class of object of reverence.
fn divine_head(divinity: Divinity) -> &'static [Concept] {
    match divinity {
        Divinity::Theistic(_) => &[Concept::Deity],
        Divinity::Animistic(SpiritKind::Spirits) => &[Concept::Spirit],
        Divinity::Animistic(SpiritKind::Totems) => &[Concept::Beast],
        Divinity::Animistic(SpiritKind::Ancestors) => &[Concept::Elder],
        Divinity::Animistic(SpiritKind::Souls) => &[Concept::Soul],
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Path)) => &[Concept::Path],
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Way)) => &[Concept::Road],
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Truth)) => &[Concept::Truth],
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Harmony)) => {
            &[Concept::Harmony]
        }
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Teacher)) => {
            &[Concept::Elder, Concept::Word]
        }
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Sage)) => &[Concept::Wisdom],
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Ascended)) => {
            &[Concept::Upper, Concept::Beyond]
        }
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Enlightened)) => {
            &[Concept::Bright, Concept::Truth]
        }
    }
}

fn divine_honorific(domain: Domain, gender: Option<DivineGender>) -> Concept {
    match domain.category() {
        DomainCategory::Celestial => Concept::Upper,
        DomainCategory::Elemental => Concept::Great,
        DomainCategory::Civic => match gender {
            Some(DivineGender::Feminine) => Concept::Lady,
            _ => Concept::Lord,
        },
        DomainCategory::Metaphysical => Concept::Spirit,
    }
}

/// The recipe for one power's proper name: its domain, an honorific and the
/// head of its class of reverence.
fn divine_parts(domain: Domain, divinity: Divinity) -> Vec<Concept> {
    let mut parts = domain_recipe(domain).to_vec();
    match divinity {
        Divinity::Theistic(gender) => {
            parts.push(divine_honorific(domain, Some(gender)));
            parts.extend_from_slice(divine_head(divinity));
        }
        Divinity::Animistic(_) => {
            let honorific = match domain.category() {
                DomainCategory::Civic => Concept::Elder,
                _ => divine_honorific(domain, None),
            };
            parts.push(honorific);
            parts.extend_from_slice(divine_head(divinity));
        }
        Divinity::Nontheist(_) => {
            // A principle or a teacher is not a lord: keep the name a plain
            // compound, and never repeat a root the domain already carries.
            for concept in divine_head(divinity) {
                push_unique(&mut parts, *concept);
            }
        }
    }
    parts
}

/// The recipe for the single core of an abstract faith.
fn core_parts(faith: &Faith) -> Vec<Concept> {
    match faith.divinity() {
        Divinity::Theistic(_) => {
            let mut parts = root_recipe(faith.root).to_vec();
            parts.extend_from_slice(divine_head(faith.divinity()));
            parts
        }
        Divinity::Animistic(_) | Divinity::Nontheist(_) => divine_head(faith.divinity()).to_vec(),
    }
}

fn liturgical_language(faith: &Faith, primary: u16, languages: &LanguagesWorld) -> Option<u16> {
    let wants_archaic = matches!(
        faith.clergy,
        Clergy::Ecclesiarchy | Clergy::Monasticism | Clergy::PropheticGuild
    ) || faith.transmissions.contains(&Transmission::WrittenCanon);
    if !wants_archaic {
        return None;
    }
    let mut id = primary;
    for _ in 0..8 {
        let Some(parent) = languages.language(id).and_then(|language| language.parent) else {
            break;
        };
        if parent == id {
            break;
        }
        id = parent;
    }
    (id != primary).then_some(id)
}

fn locus_score(locus: SacredLocus, profile: &ProvinceProfile) -> f32 {
    match locus {
        SacredLocus::Peak
        | SacredLocus::Observatory
        | SacredLocus::Crater
        | SacredLocus::Monolith => profile.mean_elevation_m,
        SacredLocus::RiverConfluence | SacredLocus::Spring => {
            profile.max_flow + if profile.lakeshore { 500.0 } else { 0.0 }
        }
        SacredLocus::Caldera => profile.mean_elevation_m + profile.max_flow * 0.1,
        SacredLocus::Oasis => (1.0 - profile.mean_precip) * 1000.0 + profile.max_flow,
        SacredLocus::Strait | SacredLocus::AbyssalCoast => {
            (if profile.coastal { 1000.0 } else { 0.0 }) + profile.max_flow * 0.05
        }
        SacredLocus::Grove => {
            profile.mean_precip * 1000.0
                + if (2.0..=30.0).contains(&profile.mean_temp_c) {
                    500.0
                } else {
                    0.0
                }
        }
        SacredLocus::Necropolis => profile.population as f32,
    }
}

/// The number of holy places a faith holds.
///
/// Every faith has one faith-wide primary site and one site for each power; a
/// state monotheism also grows branch sees from its clergy. Capped so a dossier
/// stays readable.
const MAX_SACRED_SITES: usize = 8;

fn sacred_site_count(faith: &Faith, tier: SocietyTier) -> usize {
    let base = 1 + faith.domains.len();
    let count = if faith.root == MetaphysicalRoot::Monotheism && tier.is_state() {
        let sees = match faith.clergy {
            Clergy::Ecclesiarchy => 5,
            Clergy::SacredKingship => 4,
            Clergy::Monasticism | Clergy::PropheticGuild => 3,
            Clergy::MysteryCult | Clergy::CivicMagistracy => 2,
            Clergy::Shamanism | Clergy::ClanElders | Clergy::None => 1,
        } + if matches!(faith.state_coupling, StateCoupling::Theocracy) {
            1
        } else {
            0
        };
        base.max(sees)
    } else {
        base
    };
    count.min(MAX_SACRED_SITES)
}

/// Which powers (if any) each holy place belongs to. The first slot is always
/// the faith's own primary site.
fn sacred_site_plan(faith: &Faith, tier: SocietyTier) -> Vec<Option<Domain>> {
    let count = sacred_site_count(faith, tier);
    let mut plan = Vec::with_capacity(count);
    plan.push(None);
    plan.extend(faith.domains.iter().copied().map(Some));
    while plan.len() < count {
        plan.push(None);
    }
    plan.truncate(count);
    plan
}

/// The best `count` distinct provinces for a faith's holy places. A
/// person-focused nontheist tradition puts the realm's seat first: the homeland
/// of the teacher.
fn sacred_site_provinces(
    faith: &Faith,
    provinces: &[u32],
    profiles: &[ProvinceProfile],
    count: usize,
    seat: u32,
) -> Vec<u32> {
    let mut candidates: Vec<u32> = provinces
        .iter()
        .copied()
        .filter(|province| {
            profiles
                .get(*province as usize)
                .is_some_and(|profile| profile.present)
        })
        .collect();
    candidates.sort_by(|a, b| {
        let pa = &profiles[*a as usize];
        let pb = &profiles[*b as usize];
        locus_score(faith.locus, pb)
            .partial_cmp(&locus_score(faith.locus, pa))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    if matches!(
        faith.divinity(),
        Divinity::Nontheist(NontheistFocus::Person(_))
    ) {
        if let Some(index) = candidates.iter().position(|province| *province == seat) {
            let home = candidates.remove(index);
            candidates.insert(0, home);
        }
    }
    candidates.truncate(count);
    candidates
}

/// Name every named part of one faith. `local` keeps deities distinct inside
/// the creed without qualifiers leaking between unrelated faiths.
fn faith_term_names(lex: &Lexicon, faith: &Faith, local: &mut NameRegistry) -> FaithNames {
    let domain_nouns: Vec<String> = faith
        .domains
        .iter()
        .map(|domain| say_phrase(lex, domain_recipe(*domain)))
        .collect();

    let deities = faith
        .domains
        .iter()
        .enumerate()
        .map(|(index, domain)| {
            let base = domain_nouns[index].as_str();
            let candidate = say_phrase(lex, &divine_parts(*domain, faith.divinity()));
            local.unique_rooted(base, &candidate, lex)
        })
        .collect();

    let core = if faith.domains.is_empty() {
        let base = say_phrase(lex, &core_parts(faith));
        local.unique_rooted(&base, &base, lex)
    } else {
        String::new()
    };

    FaithNames {
        domain_nouns,
        deities,
        core,
        emblem: say_phrase(lex, emblem_recipe(faith.emblem)),
        liturgy: String::new(),
    }
}

// ─── THE PASS ───────────────────────────────────────────────────────────────

/// Name every religion, its deities and domain words, its emblem, its cults and
/// its sacred sites in the world.
///
/// A religion is named in the tongue of the realm it rose in, and its holy
/// places are chosen from every province that keeps it — not from one realm's
/// borders. `registry` is shared with realm, family and province names, so a
/// faith can never take a name the gazetteer already gave a place. Names inside
/// one faith use their own registry: two deities may share a word across
/// unrelated creeds.
pub fn name_religions(
    civilizations: &mut CivilizationWorld,
    languages: &mut LanguagesWorld,
    profiles: &[ProvinceProfile],
    seed: u64,
    registry: &mut NameRegistry,
) {
    for (index, religion) in civilizations.religions.iter_mut().enumerate() {
        let origin = religion.origin_realm.saturating_sub(1) as usize;
        let mut primary = languages
            .language_of_realm
            .get(origin)
            .copied()
            .unwrap_or(0);
        // A religion whose origin realm has no surviving tongue is named by the
        // first realm that keeps it instead: no faith is left nameless.
        if primary == 0 {
            primary = religion
                .realms
                .iter()
                .filter_map(|realm| {
                    languages
                        .language_of_realm
                        .get(*realm as usize - 1)
                        .copied()
                })
                .find(|id| *id != 0)
                .unwrap_or(0);
        }
        let Some(primary_language) = languages.language(primary) else {
            continue;
        };
        if primary_language.lexicon.roots.is_empty() {
            continue;
        }

        let liturgy = liturgical_language(&religion.faith, primary, languages);
        religion.faith.liturgical_language = liturgy;
        let naming_id = liturgy.unwrap_or(primary);
        let Some(language) = languages.language(naming_id).cloned() else {
            continue;
        };
        let lexicon = &language.lexicon;
        let mut rng = ChaCha8Rng::seed_from_u64(
            seed ^ FAITH_NAME_SALT ^ (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        );

        let (candidate, base) = faith_name(lexicon, &religion.faith, &mut rng);
        religion.faith.name = registry.unique_rooted(&base, &candidate, lexicon);
        // The dossier shows the proper name alone; the descriptive working name
        // is kept out of the reader's way.
        religion.faith.gloss = religion.faith.name.clone();
        let mut local = NameRegistry::new();
        local.unique_rooted(&base, &religion.faith.name, lexicon);

        let mut names = faith_term_names(lexicon, &religion.faith, &mut local);
        names.liturgy = if liturgy.is_some() {
            language.name.clone()
        } else {
            String::new()
        };
        religion.faith.names = names;

        let divinity = religion.faith.divinity();
        for cult in &mut religion.faith.cults {
            let mut parts = domain_recipe(cult.domain).to_vec();
            parts.push(Concept::Mystery);
            let base = say_phrase(lexicon, domain_recipe(cult.domain));
            let candidate = say_plural(lexicon, &parts);
            cult.name = local.unique_rooted(&base, &candidate, lexicon);
            cult.gloss = cult.name.clone();
            let power_base = say_phrase(lexicon, domain_recipe(cult.domain));
            let power_candidate = say_phrase(lexicon, &divine_parts(cult.domain, divinity));
            cult.power_name = local.unique_rooted(&power_base, &power_candidate, lexicon);
        }

        // A faith's holy places are chosen from everywhere it is kept. The site
        // count follows the most organised realm that keeps it, so a folk creed
        // that became a state church grows more sees.
        let origin_tier = civilizations
            .civilizations
            .get(religion.origin_realm as usize - 1)
            .map_or(SocietyTier::States, |civ| civ.tier);
        let site_tier = religion
            .realms
            .iter()
            .filter_map(|realm| {
                civilizations
                    .civilizations
                    .get(*realm as usize - 1)
                    .map(|civ| civ.tier)
            })
            .fold(origin_tier, std::cmp::max);
        let plan = sacred_site_plan(&religion.faith, site_tier);
        let sites = sacred_site_provinces(
            &religion.faith,
            &religion.provinces,
            profiles,
            plan.len(),
            religion.origin_province,
        );
        let mut sacred_sites = Vec::with_capacity(sites.len());
        for (site_index, (province, domain)) in sites.iter().zip(plan.iter()).enumerate() {
            // The first site is the faith's own holy place; the rest are named
            // for the power they honour.
            let recipe = if site_index == 0 {
                locus_recipe(religion.faith.locus).to_vec()
            } else if let Some(domain) = domain {
                domain_recipe(*domain).to_vec()
            } else {
                locus_recipe(religion.faith.locus).to_vec()
            };
            let base = say_phrase(lexicon, &recipe);
            let mut parts = recipe;
            parts.push(Concept::Hearth);
            let candidate = say(
                lexicon,
                &parts,
                BridgeRule::Genitive,
                lexicon.genitive_suffix.as_deref(),
            );
            let name = registry.unique_rooted(&base, &candidate, lexicon);
            if languages
                .province_names
                .get(*province as usize)
                .is_some_and(|name| name.is_empty())
            {
                languages.province_names[*province as usize] = name.clone();
            }
            sacred_sites.push(SacredSite {
                name,
                province: *province,
                domain: *domain,
                primary: site_index == 0,
            });
        }
        religion.faith.sacred_sites = sacred_sites;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civilization::faith::{
        Cardinality, FaithContext, FaithRecension, NamingPattern, generate_faith,
    };
    use crate::civilization::{
        CradleKind, Environment, Government, Hierarchy, Outlook, Subsistence,
    };
    use crate::language::phonology::{PhonologyDna, SoundArchetype};

    fn lexicon(archetype: SoundArchetype, seed: u64) -> Lexicon {
        Lexicon::build(&PhonologyDna::build(archetype, seed))
    }

    fn river_context() -> FaithContext {
        FaithContext {
            environment: Environment::Floodplain,
            cradle_kind: CradleKind::RiverFloodplain,
            tier: SocietyTier::States,
            complexity: 0.70,
            subsistence: Subsistence::IrrigatedAgriculture,
            outlook: Outlook::Ceremonial,
            government: Government::TheocraticRepublic,
            hierarchy: Hierarchy::SacredKingship,
            mean_temp_c: 18.0,
            mean_precip: 0.45,
            mean_elevation_m: 120.0,
            sea_px: 0,
            lake_px: 0,
            river: true,
            great_river: true,
            latitude_deg: 22.0,
        }
    }

    fn sample_faith() -> Faith {
        generate_faith(&mut ChaCha8Rng::seed_from_u64(7), &river_context())
    }

    fn profile(elevation: f32) -> ProvinceProfile {
        ProvinceProfile {
            present: true,
            center_x: 0.0,
            center_y: 0.0,
            continent: 0,
            area_km2: 1.0,
            mean_elevation_m: elevation,
            max_flow: 0.0,
            coastal: false,
            lakeshore: false,
            mean_temp_c: 0.0,
            mean_precip: 0.0,
            population: 0,
            density_per_km2: 0.0,
            owner: 0,
        }
    }

    #[test]
    fn an_empty_world_is_named_without_panicking() {
        let mut civilizations = CivilizationWorld::empty(1);
        let mut languages = LanguagesWorld::empty(1);
        let mut registry = NameRegistry::new();
        name_religions(&mut civilizations, &mut languages, &[], 1, &mut registry);
        assert!(civilizations.civilizations.is_empty());
    }

    #[test]
    fn every_named_term_has_a_recipe() {
        for domain in Domain::ALL {
            assert!(!domain_recipe(*domain).is_empty(), "{domain:?}");
            let _ = domain.category();
        }
        for divinity in Divinity::ALL {
            assert!(!divine_head(divinity).is_empty(), "{divinity:?}");
            assert!(!divinity.title(false).is_empty(), "{divinity:?}");
            for domain in Domain::ALL {
                assert!(
                    !divine_parts(*domain, divinity).is_empty(),
                    "{divinity:?} {domain:?}"
                );
            }
        }
        for root in MetaphysicalRoot::ALL {
            assert!(!root_recipe(root).is_empty(), "{root:?}");
        }
        for locus in SacredLocus::ALL {
            assert!(!locus_recipe(locus).is_empty(), "{locus:?}");
        }
        for emblem in Emblem::ALL {
            assert!(!emblem_recipe(emblem).is_empty(), "{emblem:?}");
        }
        for recension in FaithRecension::ALL {
            let _ = recension_concept(recension);
        }
    }

    #[test]
    fn a_recipe_becomes_a_readable_word() {
        for archetype in SoundArchetype::ALL {
            let lex = lexicon(archetype, 4_242);
            for domain in Domain::ALL {
                let name = say_phrase(&lex, domain_recipe(*domain));
                assert!(!name.is_empty(), "{archetype:?} {domain:?}");
                assert!(
                    name.chars().count() <= crate::language::naming::MAX_NAME_CHARS,
                    "{archetype:?} {domain:?} produced {name}"
                );
                assert!(
                    name.chars().next().is_some_and(char::is_uppercase),
                    "{archetype:?} {domain:?} produced {name}"
                );
            }
            for divinity in Divinity::ALL {
                let name = say_phrase(&lex, &divine_parts(Domain::Lunar, divinity));
                assert!(!name.is_empty(), "{archetype:?} {divinity:?}");
                assert!(
                    name.chars().count() <= crate::language::naming::MAX_NAME_CHARS,
                    "{archetype:?} {divinity:?} produced {name}"
                );
            }
        }
        let lex = lexicon(SoundArchetype::NasalRiverine, 4_242);
        for pattern in [
            NamingPattern::RootEpithet,
            NamingPattern::DomainOrder,
            NamingPattern::FounderPath,
            NamingPattern::LocusCovenant,
            NamingPattern::DualPolarity,
        ] {
            let mut parts = vec![Concept::Sky, Concept::Order];
            if pattern == NamingPattern::DualPolarity {
                parts.push(Concept::Night);
            }
            let name = say_phrase(&lex, &parts);
            assert!(!name.is_empty(), "{pattern:?}");
        }
    }

    #[test]
    fn faith_names_are_parallel_to_its_domains() {
        let mut faith = sample_faith();
        faith.root = MetaphysicalRoot::Polytheism;
        faith.cardinality = Cardinality::Polycentric;
        faith.domains = vec![Domain::Lunar, Domain::Solar, Domain::War];
        faith.emblem = Emblem::Star;
        let lex = lexicon(SoundArchetype::NasalRiverine, 4_242);
        let mut local = NameRegistry::new();
        let names = faith_term_names(&lex, &faith, &mut local);

        assert_eq!(names.domain_nouns.len(), faith.domains.len());
        assert_eq!(names.deities.len(), faith.domains.len());
        assert!(names.domain_nouns.iter().all(|name| !name.is_empty()));
        assert!(names.deities.iter().all(|name| !name.is_empty()));
        assert!(names.core.is_empty());
        assert!(!names.emblem.is_empty());
    }

    #[test]
    fn an_abstract_faith_names_its_core() {
        let mut faith = sample_faith();
        faith.root = MetaphysicalRoot::Nontheism;
        faith.cardinality = Cardinality::Abstract;
        faith.domains.clear();
        faith.naming = NamingPattern::FounderPath;
        let lex = lexicon(SoundArchetype::NasalRiverine, 4_242);
        let mut local = NameRegistry::new();
        let names = faith_term_names(&lex, &faith, &mut local);

        assert!(names.domain_nouns.is_empty());
        assert!(names.deities.is_empty());
        assert!(!names.core.is_empty());
    }

    #[test]
    fn a_monotheist_state_grows_several_sees() {
        let mut faith = sample_faith();
        faith.root = MetaphysicalRoot::Monotheism;
        faith.cardinality = Cardinality::Singular;
        faith.domains = vec![Domain::Law];
        faith.clergy = Clergy::Ecclesiarchy;
        faith.state_coupling = StateCoupling::Theocracy;

        let plan = sacred_site_plan(&faith, SocietyTier::States);
        assert_eq!(plan.len(), 6);
        assert_eq!(plan[0], None);
        assert_eq!(plan[1], Some(Domain::Law));
        assert!(plan[2..].iter().all(Option::is_none));
    }

    #[test]
    fn a_polytheist_faith_has_one_site_per_power() {
        let mut faith = sample_faith();
        faith.root = MetaphysicalRoot::Polytheism;
        faith.cardinality = Cardinality::Polycentric;
        faith.domains = vec![Domain::Lunar, Domain::Solar, Domain::War, Domain::Water];

        let plan = sacred_site_plan(&faith, SocietyTier::States);
        assert_eq!(plan.len(), 5);
        assert_eq!(plan[0], None);
        assert_eq!(plan[1], Some(Domain::Lunar));
        assert_eq!(plan[4], Some(Domain::Water));
    }

    #[test]
    fn a_person_faith_puts_its_home_province_first() {
        let mut faith = sample_faith();
        faith.root = MetaphysicalRoot::Nontheism;
        faith.cardinality = Cardinality::Abstract;
        faith.domains.clear();
        faith.naming = NamingPattern::FounderPath;
        faith.locus = SacredLocus::Peak;

        let profiles = [profile(100.0), profile(500.0), profile(50.0)];
        let picked = sacred_site_provinces(&faith, &[0, 1, 2], &profiles, 2, 2);
        assert_eq!(picked, vec![2, 1]);
    }
}
