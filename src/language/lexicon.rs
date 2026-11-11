//! The semantic core of a language: forty roots, and the rules that join them.
//!
//! A language is not a list of names. It is a small set of meanings — river,
//! mountain, people, great, first — each of which has a *root*, and a grammar for
//! compounding those roots into the names a place actually carries. The roots are
//! generated from the family's [`PhonologyDna`], so a Guttural family and a
//! Melodic family do not merely sound different: they name the same world through
//! different sounds.
//!
//! The compounder is deliberately phonetic. Joining two roots can produce a
//! cluster the family does not allow, and the repair is part of the language's
//! identity: a nasal assimilates, a vowel is inserted, a geminate simplifies. The
//! plan's worked example — `Oln` (people) + `Mar` (river) → `Olnar` — is exactly
//! this rule, not a special case.

use crate::language::phonology::{PhonologyDna, SoundLaw};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The forty meanings every proto-lexicon carries.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum Concept {
    River,
    Sea,
    Mountain,
    Forest,
    Sun,
    Sky,
    Earth,
    Plain,
    Stone,
    Wind,
    Fire,
    Ice,
    People,
    Stranger,
    Elder,
    Sacred,
    Hearth,
    Kin,
    Child,
    War,
    Word,
    Dead,
    Great,
    Small,
    Upper,
    Lower,
    Deep,
    First,
    Last,
    New,
    Old,
    Bright,
    Dark,
    Cold,
    Warm,
    North,
    South,
    East,
    West,
    Middle,
}

impl Concept {
    pub const CORE: [Concept; 40] = [
        Concept::River,
        Concept::Sea,
        Concept::Mountain,
        Concept::Forest,
        Concept::Sun,
        Concept::Sky,
        Concept::Earth,
        Concept::Plain,
        Concept::Stone,
        Concept::Wind,
        Concept::Fire,
        Concept::Ice,
        Concept::People,
        Concept::Stranger,
        Concept::Elder,
        Concept::Sacred,
        Concept::Hearth,
        Concept::Kin,
        Concept::Child,
        Concept::War,
        Concept::Word,
        Concept::Dead,
        Concept::Great,
        Concept::Small,
        Concept::Upper,
        Concept::Lower,
        Concept::Deep,
        Concept::First,
        Concept::Last,
        Concept::New,
        Concept::Old,
        Concept::Bright,
        Concept::Dark,
        Concept::Cold,
        Concept::Warm,
        Concept::North,
        Concept::South,
        Concept::East,
        Concept::West,
        Concept::Middle,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Concept::River => "river",
            Concept::Sea => "sea",
            Concept::Mountain => "mountain",
            Concept::Forest => "forest",
            Concept::Sun => "sun",
            Concept::Sky => "sky",
            Concept::Earth => "earth",
            Concept::Plain => "plain",
            Concept::Stone => "stone",
            Concept::Wind => "wind",
            Concept::Fire => "fire",
            Concept::Ice => "ice",
            Concept::People => "people",
            Concept::Stranger => "stranger",
            Concept::Elder => "elder",
            Concept::Sacred => "sacred",
            Concept::Hearth => "hearth",
            Concept::Kin => "kin",
            Concept::Child => "child",
            Concept::War => "war",
            Concept::Word => "word",
            Concept::Dead => "dead",
            Concept::Great => "great",
            Concept::Small => "small",
            Concept::Upper => "upper",
            Concept::Lower => "lower",
            Concept::Deep => "deep",
            Concept::First => "first",
            Concept::Last => "last",
            Concept::New => "new",
            Concept::Old => "old",
            Concept::Bright => "bright",
            Concept::Dark => "dark",
            Concept::Cold => "cold",
            Concept::Warm => "warm",
            Concept::North => "north",
            Concept::South => "south",
            Concept::East => "east",
            Concept::West => "west",
            Concept::Middle => "middle",
        }
    }

    pub const fn is_modifier(self) -> bool {
        matches!(
            self,
            Concept::Great
                | Concept::Small
                | Concept::Upper
                | Concept::Lower
                | Concept::Deep
                | Concept::First
                | Concept::Last
                | Concept::New
                | Concept::Old
                | Concept::Bright
                | Concept::Dark
                | Concept::Cold
                | Concept::Warm
        )
    }
}

/// A generated root: the meaning and the sound that carries it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Root {
    pub concept: Concept,
    pub form: String,
    pub syllables: Vec<crate::language::phonology::Syllable>,
}

/// Where the head of a compound sits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MorphemeOrder {
    HeadFinal,
    HeadInitial,
}

/// What joins two roots when their sounds collide.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BridgeRule {
    Direct,
    Genitive,
    Epenthetic,
    Diminutive,
}

/// The forty-root lexicon of one language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lexicon {
    pub roots: Vec<Root>,
    pub order: MorphemeOrder,
    pub genitive_suffix: Option<String>,
    pub plural_suffix: Option<String>,
    pub diminutive_suffix: Option<String>,
    pub augmentative_suffix: Option<String>,
}

impl Lexicon {
    /// Build a lexicon from a family's phonology.
    pub fn build(dna: &PhonologyDna) -> Self {
        let order = if dna.seed & 1 == 0 {
            MorphemeOrder::HeadFinal
        } else {
            MorphemeOrder::HeadInitial
        };

        let mut suffix_rng = ChaCha8Rng::seed_from_u64(dna.seed ^ 0x5A17_1EAF);
        let genitive_suffix = Some(dna.short_word(&mut suffix_rng));
        let plural_suffix = Some(dna.short_word(&mut suffix_rng));
        let diminutive_suffix = Some(dna.short_word(&mut suffix_rng));
        let augmentative_suffix = Some(dna.short_word(&mut suffix_rng));

        let mut used: BTreeSet<String> = BTreeSet::new();
        let mut roots = Vec::with_capacity(Concept::CORE.len());
        for (index, concept) in Concept::CORE.iter().copied().enumerate() {
            let mut rng = ChaCha8Rng::seed_from_u64(
                dna.seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            );
            // A root is a word, not a phrase: one or two syllables. Compounding
            // supplies the length; a root that already fills a line leaves no
            // room for the grammar around it.
            let max_syllables = dna.max_root_syllables.min(2) as usize;
            let mut syllables = dna.word(&mut rng, 1 + (index % max_syllables));
            let mut form = dna.render(&syllables);
            let mut tries = 0;
            while used.contains(&form) && tries < 12 {
                tries += 1;
                syllables = dna.word(&mut rng, 1 + (tries % max_syllables));
                form = dna.render(&syllables);
            }
            if used.contains(&form) {
                form.push(
                    dna.rules
                        .epenthetic_vowel
                        .symbol()
                        .chars()
                        .next()
                        .unwrap_or('a'),
                );
            }
            used.insert(form.clone());
            roots.push(Root {
                concept,
                form,
                syllables,
            });
        }

        Self {
            roots,
            order,
            genitive_suffix,
            plural_suffix,
            diminutive_suffix,
            augmentative_suffix,
        }
    }

    pub fn root(&self, concept: Concept) -> &Root {
        self.roots
            .iter()
            .find(|root| root.concept == concept)
            .unwrap_or_else(|| &self.roots[0])
    }

    pub fn form(&self, concept: Concept) -> &str {
        self.root(concept).form.as_str()
    }

    /// Join two roots with the family's sandhi.
    ///
    /// The one rule that is orthographic rather than phonetic: a marked vowel
    /// never touches another vowel. When two vowels meet and the second is
    /// nasalised, the second loses its tilde before the bridge is chosen, so
    /// `ã` followed by `õ` prints as `ano`, never as the unreadable `ãõ`.
    pub fn join(&self, first: &str, second: &str) -> String {
        let mut out = first.to_string();
        let Some(last) = first.chars().last() else {
            return second.to_string();
        };
        if second.is_empty() {
            return out;
        }
        let mut second = second.to_string();
        let head = second.chars().next().unwrap_or('a');
        if is_vowel(last) && is_vowel(head) {
            if let Some(plain) = unmarked_vowel(head) {
                second = format!("{plain}{}", &second[head.len_utf8()..]);
            }
        }
        let Some(head) = second.chars().next() else {
            return out;
        };
        if last == head {
            out.push_str(&second[head.len_utf8()..]);
        } else if is_vowel(last) && is_vowel(head) {
            if last == 'i' || head == 'i' {
                out.push('y');
            } else if last == 'u' || head == 'u' {
                out.push('w');
            } else {
                out.push('n');
            }
            out.push_str(&second);
        } else if is_nasal(last) && is_nasal(head) {
            // Nasal + nasal: the second assimilates away. This is the rule that
            // turns `oln` + `mar` into `olnar`, not `olnmar`.
            out.push_str(&second[head.len_utf8()..]);
        } else {
            out.push_str(&second);
        }
        out
    }
}

impl Lexicon {
    /// Assemble a name from roots, honoring the family's head order.
    pub fn compound(&self, parts: &[Concept]) -> String {
        if parts.is_empty() {
            return String::new();
        }
        let ordered: Vec<Concept> = match self.order {
            MorphemeOrder::HeadFinal => parts.to_vec(),
            MorphemeOrder::HeadInitial => parts.iter().rev().copied().collect(),
        };
        let mut out = String::new();
        for (index, concept) in ordered.iter().enumerate() {
            let form = self.form(*concept);
            out = if index == 0 {
                form.to_string()
            } else {
                self.join(&out, form)
            };
        }
        out
    }

    /// Assemble a name and append a specified suffix.
    pub fn compound_with(
        &self,
        parts: &[Concept],
        suffix: Option<&str>,
        bridge: BridgeRule,
    ) -> String {
        let mut out = self.compound(parts);
        let suffix = suffix.or(match bridge {
            BridgeRule::Direct | BridgeRule::Epenthetic => None,
            BridgeRule::Genitive => self.genitive_suffix.as_deref(),
            BridgeRule::Diminutive => self.diminutive_suffix.as_deref(),
        });
        if let Some(suffix) = suffix {
            if bridge == BridgeRule::Epenthetic && !is_vowel(out.chars().last().unwrap_or('a')) {
                let vowel = self.form(Concept::River).chars().next().unwrap_or('a');
                out.push(vowel);
            }
            out = self.join(&out, suffix);
        }
        out
    }

    /// Borrow a root from a neighbour, adapted to this language's own sounds.
    pub fn borrow(&mut self, concept: Concept, donor_form: &str) {
        let mut form = donor_form.to_string();
        if self.roots.iter().any(|root| root.form == form) {
            form.push('a');
        }
        let Some(slot) = self.roots.iter_mut().find(|root| root.concept == concept) else {
            return;
        };
        slot.form = form;
        slot.syllables.clear();
    }

    /// Apply a historical sound change to every root.
    pub fn apply_law(&mut self, law: SoundLaw) {
        for root in &mut self.roots {
            root.form = transform(&root.form, law);
            root.syllables.clear();
        }
    }
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'ã' | 'õ' | 'æ')
}

/// The plain vowel a marked one falls back to when its diacritic has to go.
fn unmarked_vowel(c: char) -> Option<char> {
    match c {
        'ã' => Some('a'),
        'õ' => Some('o'),
        'æ' => Some('e'),
        _ => None,
    }
}

fn is_nasal(c: char) -> bool {
    matches!(c, 'm' | 'n')
}

/// The string-level form of a sound law.
fn transform(form: &str, law: SoundLaw) -> String {
    let mut out = form.to_string();
    match law {
        SoundLaw::Lenition => {
            for (from, to) in [
                ("p", "b"),
                ("t", "d"),
                ("k", "g"),
                ("b", "v"),
                ("d", "dh"),
                ("g", "gh"),
            ] {
                out = out.replace(from, to);
            }
        }
        SoundLaw::Fortition => {
            for (from, to) in [("b", "p"), ("d", "t"), ("g", "k"), ("v", "f"), ("z", "s")] {
                out = out.replace(from, to);
            }
        }
        SoundLaw::Palatalization => {
            out = out
                .replace("ki", "chi")
                .replace("ke", "che")
                .replace("gi", "ji");
        }
        SoundLaw::VowelShift => {
            out = out.replace('a', "e").replace('e', "i").replace('o', "u");
        }
        SoundLaw::FinalVowelLoss => {
            if out.chars().last().is_some_and(is_vowel) {
                out.pop();
            }
        }
        SoundLaw::Nasalization => {
            out = out.replace('a', "ã").replace('o', "õ");
        }
        SoundLaw::Metathesis => {
            let mut chars: Vec<char> = out.chars().collect();
            if chars.len() >= 3 {
                let last = chars.len() - 1;
                chars.swap(last - 1, last);
                out = chars.into_iter().collect();
            }
        }
        SoundLaw::ClusterSimplification => {
            for (from, to) in [
                ("nm", "n"),
                ("mn", "m"),
                ("tk", "k"),
                ("kt", "t"),
                ("dl", "l"),
                ("tl", "l"),
            ] {
                out = out.replace(from, to);
            }
        }
        SoundLaw::GlottalInsertion => {
            out = out
                .replace("aa", "a'a")
                .replace("ee", "e'e")
                .replace("ii", "i'i");
        }
        SoundLaw::Umlaut => {
            out = out
                .replace("ai", "ei")
                .replace("au", "ou")
                .replace("ia", "ie");
        }
    }
    if out.is_empty() {
        form.to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::phonology::SoundArchetype;

    fn lexicon(archetype: SoundArchetype) -> Lexicon {
        let dna = PhonologyDna::build(archetype, 4_242);
        Lexicon::build(&dna)
    }

    #[test]
    fn every_concept_has_a_root_and_no_two_roots_collide() {
        for archetype in SoundArchetype::ALL {
            let lex = lexicon(archetype);
            assert_eq!(lex.roots.len(), Concept::CORE.len());
            let forms: BTreeSet<&str> = lex.roots.iter().map(|root| root.form.as_str()).collect();
            assert_eq!(
                forms.len(),
                lex.roots.len(),
                "{archetype:?} repeated a root"
            );
            for concept in Concept::CORE {
                assert!(!lex.form(concept).is_empty());
            }
        }
    }

    #[test]
    fn a_lexicon_is_reproducible_from_its_phonology_seed() {
        let first = lexicon(SoundArchetype::NasalRiverine);
        let second = lexicon(SoundArchetype::NasalRiverine);
        assert_eq!(first, second);
    }

    #[test]
    fn nasal_plus_nasal_simplifies_to_the_river_people() {
        let lex = lexicon(SoundArchetype::NasalRiverine);
        assert_eq!(lex.join("oln", "mar"), "olnar");
    }

    #[test]
    fn two_vowels_are_bridged() {
        let lex = lexicon(SoundArchetype::MelodicMaritime);
        assert_eq!(lex.join("mara", "ol"), "maranol");
        assert_eq!(lex.join("mari", "ol"), "mariyol");
    }

    #[test]
    fn a_compound_uses_the_family_head_order() {
        let lex = lexicon(SoundArchetype::SonorousWoodland);
        let river = lex.form(Concept::River);
        let people = lex.form(Concept::People);
        let compound = lex.compound(&[Concept::River, Concept::People]);
        match lex.order {
            MorphemeOrder::HeadFinal => assert_eq!(compound, lex.join(river, people)),
            MorphemeOrder::HeadInitial => assert_eq!(compound, lex.join(people, river)),
        }
    }

    #[test]
    fn a_sound_law_changes_the_lexicon_without_emptying_it() {
        let mut lex = lexicon(SoundArchetype::SibilantTaiga);
        let before = lex.roots[0].form.clone();
        // Metathesis swaps the final two sounds of any root long enough to have
        // them, so the test does not depend on which archetype the seed drew.
        lex.apply_law(SoundLaw::Metathesis);
        assert_ne!(lex.roots[0].form, before);
        assert!(lex.roots.iter().all(|root| !root.form.is_empty()));
    }
}
