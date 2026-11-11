//! Phonology DNA: the sound inventory a language family is born with.
//!
//! A family's phonology is a small, self-contained *genome*: the consonants and
//! vowels it distinguishes, the syllable shapes it allows, the clusters its
//! speakers tolerate, and the marked articulations — glottal stops, ejectives,
//! nasal vowels — that make an alien tongue sound alien. Everything a language
//! ever says is generated from this genome plus its lexicon, never from a table
//! of hand-written names.
//!
//! The rules are deliberately phonetic rather than typographic. A syllable is
//! built out of phonemes, validated against sonority sequencing and the family's
//! own cluster limits, and only then rendered to ASCII. This is what stops the
//! generator from producing the `tkmt` that a naive consonant sampler loves and
//! what lets a daughter language inherit its parent's phonotactics while changing
//! its sounds through [`SoundLaw`]s.

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

// ─── PHONEMES ────────────────────────────────────────────────────────────────

/// One distinguishable sound.
///
/// Vowels live in the same alphabet as consonants because a syllable is one
/// sequence; [`Phoneme::class`] tells them apart. Every variant renders to a
/// plain ASCII (or single precomposed character) string, so an atlas card never
/// depends on an IPA font being installed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Phoneme {
    P,
    T,
    K,
    Q,
    B,
    D,
    G,
    M,
    N,
    Ng,
    Ny,
    F,
    V,
    S,
    Z,
    Sh,
    Zh,
    Kh,
    Gh,
    H,
    Hh,
    Ts,
    Tsh,
    L,
    R,
    W,
    J,
    GlottalStop,
    EjectiveK,
    EjectiveT,
    Pharyngeal,
    VowelA,
    VowelE,
    VowelI,
    VowelO,
    VowelU,
    VowelAe,
    VowelY,
    VowelNasalA,
    VowelNasalO,
}

/// The phonetic class of a [`Phoneme`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PhonemeClass {
    Plosive,
    Nasal,
    Fricative,
    Affricate,
    Approximant,
    Trill,
    Lateral,
    Glottal,
    Vowel,
}

impl Phoneme {
    /// The orthographic form of the sound, as the atlas prints it.
    pub const fn symbol(self) -> &'static str {
        match self {
            Phoneme::P => "p",
            Phoneme::T => "t",
            Phoneme::K => "k",
            Phoneme::Q => "q",
            Phoneme::B => "b",
            Phoneme::D => "d",
            Phoneme::G => "g",
            Phoneme::M => "m",
            Phoneme::N => "n",
            Phoneme::Ng => "ng",
            Phoneme::Ny => "ny",
            Phoneme::F => "f",
            Phoneme::V => "v",
            Phoneme::S => "s",
            Phoneme::Z => "z",
            Phoneme::Sh => "sh",
            Phoneme::Zh => "zh",
            Phoneme::Kh => "kh",
            Phoneme::Gh => "gh",
            Phoneme::H => "h",
            Phoneme::Hh => "hh",
            Phoneme::Ts => "ts",
            Phoneme::Tsh => "ch",
            Phoneme::L => "l",
            Phoneme::R => "r",
            Phoneme::W => "w",
            Phoneme::J => "y",
            Phoneme::GlottalStop => "'",
            Phoneme::EjectiveK => "k'",
            Phoneme::EjectiveT => "t'",
            Phoneme::Pharyngeal => "ḥ",
            Phoneme::VowelA => "a",
            Phoneme::VowelE => "e",
            Phoneme::VowelI => "i",
            Phoneme::VowelO => "o",
            Phoneme::VowelU => "u",
            Phoneme::VowelAe => "ae",
            Phoneme::VowelY => "y",
            Phoneme::VowelNasalA => "ã",
            Phoneme::VowelNasalO => "õ",
        }
    }

    pub const fn class(self) -> PhonemeClass {
        match self {
            Phoneme::P
            | Phoneme::T
            | Phoneme::K
            | Phoneme::Q
            | Phoneme::B
            | Phoneme::D
            | Phoneme::G => PhonemeClass::Plosive,
            Phoneme::M | Phoneme::N | Phoneme::Ng | Phoneme::Ny => PhonemeClass::Nasal,
            Phoneme::F
            | Phoneme::V
            | Phoneme::S
            | Phoneme::Z
            | Phoneme::Sh
            | Phoneme::Zh
            | Phoneme::Kh
            | Phoneme::Gh
            | Phoneme::H
            | Phoneme::Hh => PhonemeClass::Fricative,
            Phoneme::Ts | Phoneme::Tsh => PhonemeClass::Affricate,
            Phoneme::L => PhonemeClass::Lateral,
            Phoneme::R => PhonemeClass::Trill,
            Phoneme::W | Phoneme::J => PhonemeClass::Approximant,
            Phoneme::GlottalStop | Phoneme::Pharyngeal => PhonemeClass::Glottal,
            Phoneme::EjectiveK | Phoneme::EjectiveT => PhonemeClass::Plosive,
            Phoneme::VowelA
            | Phoneme::VowelE
            | Phoneme::VowelI
            | Phoneme::VowelO
            | Phoneme::VowelU
            | Phoneme::VowelAe
            | Phoneme::VowelY
            | Phoneme::VowelNasalA
            | Phoneme::VowelNasalO => PhonemeClass::Vowel,
        }
    }

    /// Sonority, from 1 (least sonorous) to 8 (a vowel).
    ///
    /// The single number that decides whether a consonant cluster is pronounceable:
    /// an onset must rise towards its nucleus, a coda must fall away from it.
    pub const fn sonority(self) -> u8 {
        match self.class() {
            PhonemeClass::Plosive => 1,
            PhonemeClass::Affricate => 2,
            PhonemeClass::Fricative => 3,
            PhonemeClass::Nasal => 4,
            PhonemeClass::Lateral => 5,
            PhonemeClass::Trill => 6,
            PhonemeClass::Approximant => 7,
            PhonemeClass::Glottal => 2,
            PhonemeClass::Vowel => 8,
        }
    }

    pub const fn is_vowel(self) -> bool {
        matches!(self.class(), PhonemeClass::Vowel)
    }
}

// ─── SOUND ARCHETYPES ────────────────────────────────────────────────────────

/// The broad sound of a family, chosen by the country its Urheimat sits in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SoundArchetype {
    /// Hot and dry: uvulars, pharyngeals, few vowels, heavy closed syllables.
    GutturalArid,
    /// Temperate coast: liquids and nasals, open syllables, a full vowel set.
    MelodicMaritime,
    /// Cold forest: sibilants and affricates, s-clusters, front vowels.
    SibilantTaiga,
    /// River and lake: nasals and prenasalized clusters, steady CV/CVC rhythm.
    NasalRiverine,
    /// Temperate woodland and savanna: sonorants, balanced syllables.
    SonorousWoodland,
    /// High valleys and islands: ejectives, glottal stops, consonant clusters.
    GlottalHighland,
}

impl SoundArchetype {
    pub const ALL: [SoundArchetype; 6] = [
        SoundArchetype::GutturalArid,
        SoundArchetype::MelodicMaritime,
        SoundArchetype::SibilantTaiga,
        SoundArchetype::NasalRiverine,
        SoundArchetype::SonorousWoodland,
        SoundArchetype::GlottalHighland,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            SoundArchetype::GutturalArid => "Guttural",
            SoundArchetype::MelodicMaritime => "Melodic",
            SoundArchetype::SibilantTaiga => "Sibilant",
            SoundArchetype::NasalRiverine => "Nasal",
            SoundArchetype::SonorousWoodland => "Sonorous",
            SoundArchetype::GlottalHighland => "Glottal",
        }
    }

    pub const fn describe(self) -> &'static str {
        match self {
            SoundArchetype::GutturalArid => {
                "throaty stops and long vowels, the speech of a land that measures water in wells"
            }
            SoundArchetype::MelodicMaritime => {
                "open syllables and flowing liquids, a tongue that carries across water"
            }
            SoundArchetype::SibilantTaiga => {
                "hissing fricatives and tight clusters, a speech made for a cold, quiet forest"
            }
            SoundArchetype::NasalRiverine => {
                "nasal cadences and steady beats, a tongue that follows the river's own rhythm"
            }
            SoundArchetype::SonorousWoodland => {
                "warm sonorants and balanced syllables, the everyday speech of field and wood"
            }
            SoundArchetype::GlottalHighland => {
                "glottal stops and ejectives, a hard speech thrown across a high valley"
            }
        }
    }

    pub const fn max_onset(self) -> u8 {
        match self {
            SoundArchetype::GutturalArid => 2,
            SoundArchetype::MelodicMaritime => 1,
            SoundArchetype::SibilantTaiga => 2,
            SoundArchetype::NasalRiverine => 2,
            SoundArchetype::SonorousWoodland => 1,
            SoundArchetype::GlottalHighland => 2,
        }
    }

    pub const fn max_coda(self) -> u8 {
        match self {
            SoundArchetype::GutturalArid => 2,
            SoundArchetype::MelodicMaritime => 0,
            SoundArchetype::SibilantTaiga => 2,
            SoundArchetype::NasalRiverine => 1,
            SoundArchetype::SonorousWoodland => 1,
            SoundArchetype::GlottalHighland => 1,
        }
    }

    pub const fn max_root_syllables(self) -> u8 {
        match self {
            SoundArchetype::MelodicMaritime | SoundArchetype::NasalRiverine => 3,
            _ => 2,
        }
    }

    pub const fn consonants(self) -> &'static [Phoneme] {
        match self {
            SoundArchetype::GutturalArid => &[
                Phoneme::K,
                Phoneme::Q,
                Phoneme::G,
                Phoneme::T,
                Phoneme::D,
                Phoneme::B,
                Phoneme::Hh,
                Phoneme::Kh,
                Phoneme::Gh,
                Phoneme::Pharyngeal,
                Phoneme::GlottalStop,
                Phoneme::M,
                Phoneme::N,
                Phoneme::R,
                Phoneme::L,
                Phoneme::S,
            ],
            SoundArchetype::MelodicMaritime => &[
                Phoneme::M,
                Phoneme::N,
                Phoneme::L,
                Phoneme::R,
                Phoneme::V,
                Phoneme::S,
                Phoneme::F,
                Phoneme::H,
                Phoneme::K,
                Phoneme::T,
                Phoneme::P,
                Phoneme::W,
                Phoneme::J,
                Phoneme::Ng,
            ],
            SoundArchetype::SibilantTaiga => &[
                Phoneme::S,
                Phoneme::Sh,
                Phoneme::Z,
                Phoneme::Zh,
                Phoneme::Ts,
                Phoneme::Tsh,
                Phoneme::K,
                Phoneme::T,
                Phoneme::P,
                Phoneme::M,
                Phoneme::N,
                Phoneme::L,
                Phoneme::R,
                Phoneme::H,
                Phoneme::Kh,
            ],
            SoundArchetype::NasalRiverine => &[
                Phoneme::M,
                Phoneme::N,
                Phoneme::Ng,
                Phoneme::Ny,
                Phoneme::B,
                Phoneme::D,
                Phoneme::G,
                Phoneme::W,
                Phoneme::L,
                Phoneme::R,
                Phoneme::S,
                Phoneme::H,
                Phoneme::T,
                Phoneme::K,
                Phoneme::GlottalStop,
            ],
            SoundArchetype::SonorousWoodland => &[
                Phoneme::L,
                Phoneme::R,
                Phoneme::M,
                Phoneme::N,
                Phoneme::W,
                Phoneme::J,
                Phoneme::V,
                Phoneme::S,
                Phoneme::T,
                Phoneme::D,
                Phoneme::K,
                Phoneme::G,
                Phoneme::P,
                Phoneme::B,
                Phoneme::H,
            ],
            SoundArchetype::GlottalHighland => &[
                Phoneme::K,
                Phoneme::T,
                Phoneme::P,
                Phoneme::Q,
                Phoneme::EjectiveK,
                Phoneme::EjectiveT,
                Phoneme::GlottalStop,
                Phoneme::S,
                Phoneme::Sh,
                Phoneme::Kh,
                Phoneme::H,
                Phoneme::M,
                Phoneme::N,
                Phoneme::L,
                Phoneme::R,
            ],
        }
    }

    pub const fn vowels(self) -> &'static [Phoneme] {
        match self {
            SoundArchetype::GutturalArid => &[
                Phoneme::VowelA,
                Phoneme::VowelI,
                Phoneme::VowelU,
                Phoneme::VowelAe,
            ],
            SoundArchetype::MelodicMaritime => &[
                Phoneme::VowelA,
                Phoneme::VowelE,
                Phoneme::VowelI,
                Phoneme::VowelO,
                Phoneme::VowelU,
                Phoneme::VowelAe,
                Phoneme::VowelY,
            ],
            SoundArchetype::SibilantTaiga => &[
                Phoneme::VowelA,
                Phoneme::VowelE,
                Phoneme::VowelI,
                Phoneme::VowelO,
                Phoneme::VowelU,
                Phoneme::VowelY,
            ],
            SoundArchetype::NasalRiverine => &[
                Phoneme::VowelA,
                Phoneme::VowelE,
                Phoneme::VowelI,
                Phoneme::VowelO,
                Phoneme::VowelU,
                Phoneme::VowelNasalA,
                Phoneme::VowelNasalO,
            ],
            SoundArchetype::SonorousWoodland => &[
                Phoneme::VowelA,
                Phoneme::VowelE,
                Phoneme::VowelI,
                Phoneme::VowelO,
                Phoneme::VowelU,
            ],
            SoundArchetype::GlottalHighland => &[
                Phoneme::VowelA,
                Phoneme::VowelE,
                Phoneme::VowelI,
                Phoneme::VowelO,
                Phoneme::VowelU,
                Phoneme::VowelAe,
            ],
        }
    }

    pub fn template_pool(self) -> Vec<SyllableTemplate> {
        use SyllableTemplate::*;
        match self {
            SoundArchetype::GutturalArid => vec![CVC, CVC, CVC, CV, CV, CVCC, VC],
            SoundArchetype::MelodicMaritime => vec![CV, CV, CV, CV, CV, V, CVC],
            SoundArchetype::SibilantTaiga => vec![CVC, CVC, CV, CV, CCVC, CCV, CVCC],
            SoundArchetype::NasalRiverine => vec![CV, CV, CVC, CVC, V, CCV],
            SoundArchetype::SonorousWoodland => vec![CV, CV, CV, CVC, CVC, V],
            SoundArchetype::GlottalHighland => vec![CVC, CVC, CCVC, CCV, CV, CVCC],
        }
    }

    pub fn markers(self) -> Vec<ArticulationMarker> {
        match self {
            SoundArchetype::GutturalArid => {
                vec![
                    ArticulationMarker::GlottalStop,
                    ArticulationMarker::Ejective,
                ]
            }
            SoundArchetype::MelodicMaritime => vec![],
            SoundArchetype::SibilantTaiga => vec![],
            SoundArchetype::NasalRiverine => vec![ArticulationMarker::NasalVowels],
            SoundArchetype::SonorousWoodland => vec![],
            SoundArchetype::GlottalHighland => {
                vec![
                    ArticulationMarker::GlottalStop,
                    ArticulationMarker::Ejective,
                ]
            }
        }
    }
}

// ─── SYLLABLES ───────────────────────────────────────────────────────────────

/// The shape of one syllable: `CV`, `CVC`, `CCVC`, …
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SyllableTemplate {
    V,
    CV,
    CVC,
    CCV,
    CCVC,
    CVCC,
    VC,
}

impl SyllableTemplate {
    /// Onset, nucleus and coda lengths.
    pub const fn shape(self) -> (u8, u8, u8) {
        match self {
            SyllableTemplate::V => (0, 1, 0),
            SyllableTemplate::CV => (1, 1, 0),
            SyllableTemplate::CVC => (1, 1, 1),
            SyllableTemplate::CCV => (2, 1, 0),
            SyllableTemplate::CCVC => (2, 1, 1),
            SyllableTemplate::CVCC => (1, 1, 2),
            SyllableTemplate::VC => (0, 1, 1),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            SyllableTemplate::V => "V",
            SyllableTemplate::CV => "CV",
            SyllableTemplate::CVC => "CVC",
            SyllableTemplate::CCV => "CCV",
            SyllableTemplate::CCVC => "CCVC",
            SyllableTemplate::CVCC => "CVCC",
            SyllableTemplate::VC => "VC",
        }
    }
}

/// An articulation a family may mark, beyond its plain consonant and vowel set.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ArticulationMarker {
    /// A glottal stop, printed as an ASCII apostrophe.
    GlottalStop,
    /// An ejective series, printed with a trailing apostrophe.
    Ejective,
    /// Nasalized vowels, printed with a tilde.
    NasalVowels,
    /// A tonal distinction, retained as a semantic marker.
    Tone,
}

impl ArticulationMarker {
    pub const fn label(self) -> &'static str {
        match self {
            ArticulationMarker::GlottalStop => "glottal stop",
            ArticulationMarker::Ejective => "ejectives",
            ArticulationMarker::NasalVowels => "nasal vowels",
            ArticulationMarker::Tone => "tone",
        }
    }
}

/// The phonotactic rules one family lives by.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyllableRules {
    pub max_onset: u8,
    pub max_coda: u8,
    pub allow_geminates: bool,
    pub allow_final_stops: bool,
    pub require_sonority_rise: bool,
    pub require_sonority_fall: bool,
    pub forbidden_pairs: Vec<(Phoneme, Phoneme)>,
    pub epenthetic_vowel: Phoneme,
    pub bridge_vowel: Option<Phoneme>,
}

/// One built syllable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Syllable {
    pub onset: Vec<Phoneme>,
    pub nucleus: Phoneme,
    pub coda: Vec<Phoneme>,
}

/// The full phonological genome of one proto-language or daughter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhonologyDna {
    /// Family-local stream seed: every word in this tongue derives from this
    /// number, never from a global draw order.
    pub seed: u64,
    pub archetype: SoundArchetype,
    pub consonants: Vec<Phoneme>,
    pub vowels: Vec<Phoneme>,
    /// Weighted syllable shapes: a template present twice is twice as likely.
    pub templates: Vec<SyllableTemplate>,
    pub rules: SyllableRules,
    pub markers: Vec<ArticulationMarker>,
    pub max_root_syllables: u8,
}

impl PhonologyDna {
    /// Build a genome for one family from its archetype and its own seed.
    pub fn build(archetype: SoundArchetype, seed: u64) -> Self {
        let rules = SyllableRules {
            max_onset: archetype.max_onset(),
            max_coda: archetype.max_coda(),
            allow_geminates: matches!(
                archetype,
                SoundArchetype::GutturalArid | SoundArchetype::GlottalHighland
            ),
            allow_final_stops: !matches!(archetype, SoundArchetype::MelodicMaritime),
            require_sonority_rise: true,
            require_sonority_fall: true,
            forbidden_pairs: vec![
                (Phoneme::S, Phoneme::Sh),
                (Phoneme::Sh, Phoneme::S),
                (Phoneme::Ts, Phoneme::S),
                (Phoneme::T, Phoneme::Ts),
                (Phoneme::K, Phoneme::Q),
                (Phoneme::GlottalStop, Phoneme::GlottalStop),
            ],
            epenthetic_vowel: archetype.vowels()[0],
            bridge_vowel: if matches!(archetype, SoundArchetype::MelodicMaritime) {
                Some(Phoneme::VowelA)
            } else {
                None
            },
        };

        Self {
            seed,
            archetype,
            consonants: archetype.consonants().to_vec(),
            vowels: archetype.vowels().to_vec(),
            templates: archetype.template_pool(),
            rules,
            markers: archetype.markers(),
            max_root_syllables: archetype.max_root_syllables(),
        }
    }

    /// A daughter genome: the same family, a new stream, and the freedom to lose
    /// a cluster or a vowel.
    pub fn daughter(&self, seed: u64, law: SoundLaw) -> Self {
        let mut next = self.clone();
        next.seed = seed;
        match law {
            SoundLaw::FinalVowelLoss => {
                next.max_root_syllables = next.max_root_syllables.max(2);
            }
            SoundLaw::ClusterSimplification | SoundLaw::Lenition => {
                next.rules.max_onset = 1;
                next.rules.max_coda = next.rules.max_coda.min(1);
            }
            SoundLaw::GlottalInsertion => {
                if !next.markers.contains(&ArticulationMarker::GlottalStop) {
                    next.markers.push(ArticulationMarker::GlottalStop);
                    next.consonants.push(Phoneme::GlottalStop);
                }
            }
            SoundLaw::Nasalization => {
                if !next.markers.contains(&ArticulationMarker::NasalVowels) {
                    next.markers.push(ArticulationMarker::NasalVowels);
                    if !next.vowels.contains(&Phoneme::VowelNasalA) {
                        next.vowels.push(Phoneme::VowelNasalA);
                    }
                }
            }
            SoundLaw::Palatalization => {
                if !next.consonants.contains(&Phoneme::Tsh) {
                    next.consonants.push(Phoneme::Tsh);
                }
            }
            SoundLaw::VowelShift
            | SoundLaw::Fortition
            | SoundLaw::Metathesis
            | SoundLaw::Umlaut => {}
        }
        next
    }

    /// A complete word of `syllables` syllables, with cross-syllable repairs.
    pub fn word(&self, rng: &mut ChaCha8Rng, syllables: usize) -> Vec<Syllable> {
        let mut out: Vec<Syllable> = Vec::with_capacity(syllables.max(1));
        for _ in 0..syllables.max(1) {
            let mut syllable = self.syllable(rng);
            if syllable.onset.is_empty() {
                // A vowel-initial syllable after a vowel gets a glide or a
                // glottal onset, so the word does not run two vowels together.
                if let Some(previous) = out.last() {
                    if previous.coda.is_empty() && previous.nucleus.is_vowel() {
                        let bridge = if self.markers.contains(&ArticulationMarker::GlottalStop) {
                            Phoneme::GlottalStop
                        } else if self.consonants.contains(&Phoneme::J) {
                            Phoneme::J
                        } else {
                            Phoneme::W
                        };
                        syllable.onset.push(bridge);
                    }
                }
            }
            // Two identical consonants at a syllable seam simplify to one, unless
            // the family allows geminates.
            if !self.rules.allow_geminates {
                if let (Some(previous), Some(next)) = (out.last(), syllable.onset.first()) {
                    if previous.coda.last() == Some(next) {
                        syllable.onset.remove(0);
                    }
                }
            }
            out.push(syllable);
        }
        out
    }

    /// One validated syllable.
    pub fn syllable(&self, rng: &mut ChaCha8Rng) -> Syllable {
        for _ in 0..32 {
            if let Some(template) = self.templates.choose(rng).copied() {
                let candidate = self.syllable_of(template, rng);
                if self.is_valid(&candidate) {
                    return candidate;
                }
            }
        }
        // A CV syllable from the first consonants and vowels: always legal.
        Syllable {
            onset: vec![self.consonants[0]],
            nucleus: self.vowels[0],
            coda: Vec::new(),
        }
    }

    fn syllable_of(&self, template: SyllableTemplate, rng: &mut ChaCha8Rng) -> Syllable {
        let (onset_len, _, coda_len) = template.shape();
        let mut onset = Vec::with_capacity(onset_len as usize);
        for _ in 0..onset_len {
            onset.push(self.pick_consonant(rng));
        }
        let nucleus = *self.vowels.choose(rng).unwrap_or(&Phoneme::VowelA);
        let mut coda = Vec::with_capacity(coda_len as usize);
        for _ in 0..coda_len {
            coda.push(self.pick_consonant(rng));
        }
        Syllable {
            onset,
            nucleus,
            coda,
        }
    }

    fn pick_consonant(&self, rng: &mut ChaCha8Rng) -> Phoneme {
        *self.consonants.choose(rng).unwrap_or(&Phoneme::T)
    }

    /// Whether a syllable obeys the family's own phonotactics.
    pub fn is_valid(&self, syllable: &Syllable) -> bool {
        if !syllable.nucleus.is_vowel() {
            return false;
        }
        if syllable.onset.len() > self.rules.max_onset as usize
            || syllable.coda.len() > self.rules.max_coda as usize
        {
            return false;
        }
        if !self.valid_cluster(&syllable.onset, true) || !self.valid_cluster(&syllable.coda, false)
        {
            return false;
        }
        if !self.rules.allow_final_stops
            && matches!(
                syllable.coda.last().map(|p| p.class()),
                Some(PhonemeClass::Plosive)
            )
        {
            return false;
        }
        true
    }

    fn valid_cluster(&self, cluster: &[Phoneme], onset: bool) -> bool {
        if cluster.is_empty() {
            return true;
        }
        for pair in cluster.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a == b && !self.rules.allow_geminates {
                return false;
            }
            if self.rules.forbidden_pairs.contains(&(a, b)) {
                return false;
            }
            if onset && self.rules.require_sonority_rise && a.sonority() >= b.sonority() {
                return false;
            }
            if !onset && self.rules.require_sonority_fall && a.sonority() <= b.sonority() {
                return false;
            }
        }
        true
    }

    /// Render a word in the family's own orthography.
    pub fn render(&self, syllables: &[Syllable]) -> String {
        let mut out = String::new();
        for syllable in syllables {
            for phoneme in &syllable.onset {
                out.push_str(phoneme.symbol());
            }
            out.push_str(syllable.nucleus.symbol());
            for phoneme in &syllable.coda {
                out.push_str(phoneme.symbol());
            }
        }
        out
    }

    /// A one-syllable word, for suffixes and short roots.
    pub fn short_word(&self, rng: &mut ChaCha8Rng) -> String {
        self.render(&self.word(rng, 1))
    }

    /// True when the family marks a glottal stop in its orthography.
    pub fn has_glottal_stop(&self) -> bool {
        self.markers.contains(&ArticulationMarker::GlottalStop)
            || self.consonants.contains(&Phoneme::GlottalStop)
    }

    /// A readable list of the markers, for a dossier line.
    pub fn marker_label(&self) -> String {
        if self.markers.is_empty() {
            "no marked articulations".to_string()
        } else {
            self.markers
                .iter()
                .map(|marker| marker.label())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

// ─── SOUND LAWS ──────────────────────────────────────────────────────────────

/// A historical change that separates a daughter tongue from its parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SoundLaw {
    Lenition,
    Fortition,
    Palatalization,
    VowelShift,
    FinalVowelLoss,
    Nasalization,
    Metathesis,
    ClusterSimplification,
    GlottalInsertion,
    Umlaut,
}

impl SoundLaw {
    pub const ALL: [SoundLaw; 10] = [
        SoundLaw::Lenition,
        SoundLaw::Fortition,
        SoundLaw::Palatalization,
        SoundLaw::VowelShift,
        SoundLaw::FinalVowelLoss,
        SoundLaw::Nasalization,
        SoundLaw::Metathesis,
        SoundLaw::ClusterSimplification,
        SoundLaw::GlottalInsertion,
        SoundLaw::Umlaut,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            SoundLaw::Lenition => "lenition",
            SoundLaw::Fortition => "fortition",
            SoundLaw::Palatalization => "palatalization",
            SoundLaw::VowelShift => "vowel shift",
            SoundLaw::FinalVowelLoss => "final-vowel loss",
            SoundLaw::Nasalization => "nasalization",
            SoundLaw::Metathesis => "metathesis",
            SoundLaw::ClusterSimplification => "cluster simplification",
            SoundLaw::GlottalInsertion => "glottal insertion",
            SoundLaw::Umlaut => "umlaut",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dna(archetype: SoundArchetype) -> PhonologyDna {
        PhonologyDna::build(archetype, 12_345)
    }

    #[test]
    fn every_archetype_builds_a_usable_inventory() {
        for archetype in SoundArchetype::ALL {
            let dna = dna(archetype);
            assert!(dna.consonants.len() >= 12, "{archetype:?}");
            assert!(dna.vowels.len() >= 3, "{archetype:?}");
            assert!(!dna.templates.is_empty());
        }
    }

    #[test]
    fn every_generated_syllable_is_phonotactically_valid() {
        for archetype in SoundArchetype::ALL {
            let dna = dna(archetype);
            let mut rng = ChaCha8Rng::seed_from_u64(99);
            for syllables in 1..=4 {
                for _ in 0..200 {
                    for syllable in dna.word(&mut rng, syllables) {
                        assert!(
                            dna.is_valid(&syllable),
                            "{archetype:?} produced an invalid syllable: {syllable:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_word_is_reproducible_from_its_seed() {
        let dna = dna(SoundArchetype::MelodicMaritime);
        let mut first = ChaCha8Rng::seed_from_u64(7);
        let mut second = ChaCha8Rng::seed_from_u64(7);
        assert_eq!(dna.word(&mut first, 3), dna.word(&mut second, 3));
    }

    #[test]
    fn a_guttural_family_marks_its_glottal_stops() {
        let dna = dna(SoundArchetype::GutturalArid);
        assert!(dna.has_glottal_stop());
        assert!(dna.marker_label().contains("glottal stop"));
        assert_eq!(Phoneme::GlottalStop.symbol(), "'");
    }

    #[test]
    fn an_open_family_never_builds_a_closed_syllable() {
        let dna = dna(SoundArchetype::MelodicMaritime);
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        for _ in 0..500 {
            let syllable = dna.syllable(&mut rng);
            assert!(syllable.coda.is_empty(), "{syllable:?}");
        }
    }

    #[test]
    fn sonority_makes_a_stop_plus_nasal_coda_illegal() {
        let dna = dna(SoundArchetype::SonorousWoodland);
        let bad = Syllable {
            onset: vec![Phoneme::T],
            nucleus: Phoneme::VowelA,
            coda: vec![Phoneme::T, Phoneme::M],
        };
        assert!(!dna.is_valid(&bad));
    }
}
