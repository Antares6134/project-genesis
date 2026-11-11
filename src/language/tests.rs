//! Cross-module tests for the language stage.

use super::*;
use crate::civilization::Age;
use crate::language::families::{FamilySeed, build_family};
use crate::language::naming::{
    MAX_NAME_CHARS, MAX_NAME_SYLLABLES, MIN_NAME_SYLLABLES, NameRegistry, PolityKind,
    language_name, realm_name, syllable_count,
};

/// The canon of Act 8, checked through its real entry points: a world's families,
/// their tongues and a full gazetteer of realms all read like words — two to four
/// syllables, at most fourteen characters, strictly title-cased, never two
/// modifiers deep.
#[test]
fn every_generated_name_is_a_short_readable_word() {
    let mut registry = NameRegistry::new();
    for (index, archetype) in SoundArchetype::ALL.iter().cycle().take(18).enumerate() {
        let seed = FamilySeed {
            province: index as u32 + 1,
            x: index as f32 * 3.0,
            y: 0.0,
            continent: (index % 3) as u32,
            score: 1.0,
            archetype: *archetype,
        };
        let family = build_family(index as u16 + 1, &seed, 7_000 + index as u64, Age::LADDER[6]);
        assert_well_shaped(&family.name);
        assert_well_shaped(&family.autonym);
        assert_well_shaped(&family.urheimat.name);

        // A family of tongues: the proto-language and a handful of daughters.
        let mut tongues = vec![language_name(&family.lexicon, &family.name, None)];
        for qualifier in [
            Concept::Upper,
            Concept::Lower,
            Concept::Deep,
            Concept::Great,
            Concept::New,
            Concept::Old,
        ] {
            tongues.push(language_name(&family.lexicon, &family.name, Some(qualifier)));
        }
        for tongue in &tongues {
            assert_well_shaped(tongue);
        }

        // The gazetteer: many realms share one tongue, so every realm name after
        // the first is a collision retry — the place the old generator stuttered.
        for step in 0..60usize {
            let kind = match step % 3 {
                0 => PolityKind::State,
                1 => PolityKind::Chiefdom,
                _ => PolityKind::People,
            };
            let candidate = realm_name(&family.lexicon, &family.autonym, kind);
            let name = registry.unique_rooted(&family.autonym, &candidate, &family.lexicon);
            assert_well_shaped(&name);
        }
    }
}


/// A shape a name must have to be legible as a word on a map.
fn assert_well_shaped(name: &str) {
    let syllables = syllable_count(name);
    assert!(
        (MIN_NAME_SYLLABLES..=MAX_NAME_SYLLABLES).contains(&syllables),
        "{name} is {syllables} syllables"
    );
    assert!(
        name.chars().count() <= MAX_NAME_CHARS,
        "{name} is {} characters",
        name.chars().count()
    );
    let first = name.chars().next().expect("a name is never empty");
    assert!(first.is_uppercase(), "{name} does not open in a capital");
    assert!(
        !name.chars().skip(1).any(char::is_uppercase),
        "{name} carries a mid-word capital"
    );
    let chars: Vec<char> = name.chars().collect();
    for (index, c) in chars.iter().enumerate() {
        if matches!(c, 'ã' | 'õ' | 'æ') && index > 0 {
            assert!(
                !matches!(
                    chars[index - 1],
                    'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'ã' | 'õ' | 'æ'
                ),
                "{name} lets two vowels carry diacritics side by side"
            );
        }
    }
}

/// The two halves of the model agree: a family's lexicon, its phonology and its
/// name all derive from the same seed, and a second build is identical.
#[test]
fn a_family_is_reproducible_from_its_seed() {
    let archetype = SoundArchetype::MelodicMaritime;
    let first = PhonologyDna::build(archetype, 9_001);
    let second = PhonologyDna::build(archetype, 9_001);
    assert_eq!(first, second);
    let first_lex = Lexicon::build(&first);
    let second_lex = Lexicon::build(&second);
    assert_eq!(first_lex, second_lex);
}

/// The atlas falls back to an empty world, never to a half-built one.
#[test]
fn an_empty_world_answers_every_lookup_with_nothing() {
    let world = LanguagesWorld::empty(3);
    assert!(world.is_empty());
    assert!(world.family(1).is_none());
    assert!(world.language(1).is_none());
    assert!(world.family_of_province(1).is_none());
    assert!(world.language_of_civ(1).is_none());
    assert_eq!(world.total_provinces(), 0);
}
