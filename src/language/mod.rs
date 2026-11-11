//! Stage 8 — Languages & Phonetics: the tongues the world's peoples speak.
//!
//! The dawn of history gives a world its societies; this act gives them a voice.
//! It is deliberately *not* a name generator bolted onto the civilizations. It
//! builds a small linguistic prehistory first:
//!
//! 1. **Urheimats** — ten to eighteen ancestral families are placed in the
//!    provinces that could plausibly cradle a language: river basins, coasts,
//!    lakeshores, temperate woodlands, isolates in high valleys.
//! 2. **Phonology DNA** — each family draws a sound archetype from its country
//!    and a genome from the world seed: consonants, vowels, syllable templates,
//!    phonotactic rules, and marked articulations such as glottal stops.
//! 3. **A forty-root proto-lexicon** — every family names the same forty core
//!    meanings (river, sea, mountain, people, great, first…) in its own sounds.
//! 4. **Diffusion** — the families spread across the province graph under
//!    friction: cheap along rivers and open plains, expensive over mountain
//!    spines and deep wilderness, impossible across open ocean until seafaring.
//!    Dense urban cradles then split into local daughter languages and isolates,
//!    while pastoral and wilderness zones form sweeping macro-family blobs.
//! 5. **Naming** — languages, families, realms, capitals and Urheimats are
//!    assembled from the roots with the family's own compounding rules.
//!
//! The result is a [`LanguagesWorld`]: every inhabited province has a language,
//! every society has a primary language and family, and every name on the map is
//! something the family could actually have said. The stage republishes a renamed
//! [`CivilizationWorld`] too, so every existing atlas panel — the Civilizations
//! legend, the hover card, the printed dossier — reads the native names without a
//! single special case.
//!
//! ## Determinism
//!
//! One `ChaCha8Rng` seeded from `seed ^ LANGUAGE_SALT` drives the stage, and each
//! family then draws from its own sub-stream. Every collection is a `Vec` sorted
//! by province id or a `BTreeMap`; every diffusion tie is broken by cost, then
//! province, then family. The same seed always produces the same families, the
//! same languages, and the same names.

pub mod config;
pub mod diffusion;
pub mod families;
pub mod generator;
pub mod lexicon;
pub mod naming;
pub mod phonology;

use crate::civilization::Age;
use serde::{Deserialize, Serialize};

pub use config::LanguageConfig;
pub use generator::generate_languages;
pub use lexicon::{Concept, Lexicon};
pub use phonology::{PhonologyDna, SoundArchetype, SoundLaw};

#[cfg(test)]
mod tests;

/// The province a family was born in, and the name its own language gives it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Urheimat {
    pub province: u32,
    pub x: f32,
    pub y: f32,
    /// A name assembled in the family's own lexicon, e.g. "Mar-Oln".
    pub name: String,
    pub archetype: SoundArchetype,
}

/// One ancestral family: a phonology, a proto-lexicon, and a homeland.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanguageFamily {
    /// 1-based, and stable for the life of the world.
    pub id: u16,
    pub name: String,
    /// The family's name for itself, built from its own roots.
    pub autonym: String,
    pub color: [u8; 3],
    pub phonology: PhonologyDna,
    pub lexicon: Lexicon,
    pub urheimat: Urheimat,
    pub parent: Option<u16>,
    pub age: Age,
    pub languages: Vec<u16>,
    pub province_count: usize,
    pub population: u64,
}

/// One language, dialect or daughter tongue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Language {
    /// 1-based, and stable for the life of the world.
    pub id: u16,
    pub family: u16,
    pub name: String,
    /// The language's name for itself.
    pub autonym: String,
    /// The family hue, shaded by this language's depth.
    pub color: [u8; 3],
    pub phonology: PhonologyDna,
    pub lexicon: Lexicon,
    pub parent: Option<u16>,
    /// 0 for the proto-language, 1+ for daughters and dialects.
    pub depth: u8,
    pub sound_laws: Vec<SoundLaw>,
    pub urheimat: u32,
    pub province_count: usize,
    pub population: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LanguageContactKind {
    Frontier,
    Trade,
    SeaRoute,
    Isolate,
}

impl LanguageContactKind {
    pub const fn label(self) -> &'static str {
        match self {
            LanguageContactKind::Frontier => "a shared frontier",
            LanguageContactKind::Trade => "a trade road",
            LanguageContactKind::SeaRoute => "a short sea crossing",
            LanguageContactKind::Isolate => "a mountain or island isolate",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanguageContact {
    pub a: u16,
    pub b: u16,
    pub kind: LanguageContactKind,
    pub borrowed: Vec<Concept>,
}

/// Every language of one world, and the per-province tables the atlas paints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanguagesWorld {
    pub seed: u64,
    pub families: Vec<LanguageFamily>,
    pub languages: Vec<Language>,

    /// Province id → language id (0 = unsettled, water or wasteland).
    pub province_language: Vec<u16>,
    /// Province id → family id (0 = none).
    pub province_family: Vec<u16>,

    /// Civilization id - 1 → language id.
    pub language_of_realm: Vec<u16>,
    /// Civilization id - 1 → family id.
    pub family_of_realm: Vec<u16>,
    /// Civilization id - 1 → the realm's native name.
    pub realm_names: Vec<String>,

    /// Province id → a name for the province, where one could be made.
    pub province_names: Vec<String>,

    pub contacts: Vec<LanguageContact>,
}

impl LanguagesWorld {
    /// A world with no languages yet — what the atlas holds before Act 8.
    pub fn empty(seed: u64) -> Self {
        Self {
            seed,
            families: Vec::new(),
            languages: Vec::new(),
            province_language: Vec::new(),
            province_family: Vec::new(),
            language_of_realm: Vec::new(),
            family_of_realm: Vec::new(),
            realm_names: Vec::new(),
            province_names: Vec::new(),
            contacts: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.families.is_empty()
    }

    /// A family by 1-based id.
    pub fn family(&self, id: u16) -> Option<&LanguageFamily> {
        if id == 0 {
            return None;
        }
        self.families.get(id as usize - 1)
    }

    /// A language by 1-based id.
    pub fn language(&self, id: u16) -> Option<&Language> {
        if id == 0 {
            return None;
        }
        self.languages.get(id as usize - 1)
    }

    pub fn family_of_province(&self, province: u32) -> Option<&LanguageFamily> {
        let id = self
            .province_family
            .get(province as usize)
            .copied()
            .unwrap_or(0);
        self.family(id)
    }

    pub fn language_of_province(&self, province: u32) -> Option<&Language> {
        let id = self
            .province_language
            .get(province as usize)
            .copied()
            .unwrap_or(0);
        self.language(id)
    }

    pub fn family_of_civ(&self, civ_id: u32) -> Option<&LanguageFamily> {
        if civ_id == 0 {
            return None;
        }
        let id = self
            .family_of_realm
            .get(civ_id as usize - 1)
            .copied()
            .unwrap_or(0);
        self.family(id)
    }

    pub fn language_of_civ(&self, civ_id: u32) -> Option<&Language> {
        if civ_id == 0 {
            return None;
        }
        let id = self
            .language_of_realm
            .get(civ_id as usize - 1)
            .copied()
            .unwrap_or(0);
        self.language(id)
    }

    pub fn realm_name(&self, civ_id: u32) -> Option<&str> {
        if civ_id == 0 {
            return None;
        }
        self.realm_names
            .get(civ_id as usize - 1)
            .map(String::as_str)
    }

    pub fn name_of_family(&self, id: u16) -> &str {
        self.family(id)
            .map_or("an unknown tongue", |f| f.name.as_str())
    }

    pub fn name_of_language(&self, id: u16) -> &str {
        self.language(id)
            .map_or("an unknown tongue", |l| l.name.as_str())
    }

    pub fn color_of_family(&self, id: u16) -> [u8; 3] {
        self.family(id).map_or([128, 128, 128], |f| f.color)
    }

    pub fn color_of_language(&self, id: u16) -> [u8; 3] {
        self.language(id).map_or([128, 128, 128], |l| l.color)
    }

    /// A hover-card line naming the Urheimat.
    pub fn urheimat_label(&self, family_id: u16) -> String {
        match self.family(family_id) {
            Some(family) => format!(
                "province #{} — {}",
                family.urheimat.province, family.urheimat.name
            ),
            None => "nowhere remembered".to_string(),
        }
    }

    pub fn total_provinces(&self) -> usize {
        self.province_language.iter().filter(|id| **id != 0).count()
    }

    pub fn total_population(&self) -> u64 {
        self.languages
            .iter()
            .map(|language| language.population)
            .sum()
    }

    /// Families ordered by the ground they hold, largest first. A legend reads
    /// like a census, not like a draw order.
    pub fn families_by_size(&self) -> Vec<&LanguageFamily> {
        let mut families: Vec<&LanguageFamily> = self.families.iter().collect();
        families.sort_by(|a, b| {
            b.province_count
                .cmp(&a.province_count)
                .then(a.id.cmp(&b.id))
        });
        families
    }

    /// Languages of one family, shallowest first.
    pub fn languages_of(&self, family_id: u16) -> Vec<&Language> {
        let mut languages: Vec<&Language> = self
            .languages
            .iter()
            .filter(|language| language.family == family_id)
            .collect();
        languages.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.id.cmp(&b.id)));
        languages
    }

    /// Contacts of one language, capped and ordered by id.
    pub fn contacts_of(&self, language_id: u16) -> Vec<&LanguageContact> {
        self.contacts
            .iter()
            .filter(|contact| contact.a == language_id || contact.b == language_id)
            .take(8)
            .collect()
    }
}
