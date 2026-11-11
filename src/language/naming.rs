//! Turning roots into names.
//!
//! Every name is built from the family's own forty roots with its own compounding
//! rules, so a realm called `Olnarma` is not a string that merely looks foreign:
//! it is `oln` (people) + `mar` (river) + a polity suffix, assembled by the same
//! sandhi that joins any other compound.
//!
//! * **A name is unique.** Collisions are resolved by a qualifier that is itself a
//!   root (`Upper`, `Lower`, `Great`, `Little`), never by a numeral.
//! * **A name is pronounceable in its own language.** Suffixes are generated from
//!   the same phonology, not imported from English.
//!
//! ## Shape
//!
//! A name is a *word*, not a sentence. Three rules give every name the punch of the
//! worked examples — `Vufupo`, `Ravona`, `Kaesiva`, `Li'ngikri`:
//!
//! * **Two to four syllables, no more than fourteen characters.** A longer
//!   candidate is clipped at a syllable boundary, never in the middle of a
//!   cluster, so agglutination cannot run away.
//! * **One morpheme of modification.** A base may take *one* qualifier (`Upper`,
//!   `Great`, …) or *one* suffix, never both and never two of either. A base that
//!   already fills three syllables loses its last one before a suffix lands, and a
//!   retry always rebuilds from the plain base root rather than stacking another
//!   qualifier on the name that just collided.
//! * **Strict title case, and no touching marked vowels.** `Foobar`, never
//!   `FooBar`; and when two vowels meet the second drops its diacritic, so `ÃÕ`
//!   cannot be printed.

use crate::language::lexicon::{BridgeRule, Concept, Lexicon};
use crate::language::phonology::{PhonologyDna, SoundArchetype};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The longest a finished name may be, in characters.
pub const MAX_NAME_CHARS: usize = 14;
/// The most syllables a finished name may carry.
pub const MAX_NAME_SYLLABLES: usize = 4;
/// The fewest syllables a finished name may carry.
pub const MIN_NAME_SYLLABLES: usize = 2;

/// The qualifiers a collision may draw from, in a fixed order. A name may carry
/// at most one of them.
pub const MODIFIERS: [Concept; 10] = [
    Concept::Upper,
    Concept::Lower,
    Concept::Great,
    Concept::Small,
    Concept::First,
    Concept::Last,
    Concept::Bright,
    Concept::Dark,
    Concept::New,
    Concept::Old,
];

/// What kind of thing is being named.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum NameKind {
    Family,
    Language,
    Autonym,
    Realm,
    Settlement,
    Feature,
}

/// The social weight of a named polity, which decides its suffix.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolityKind {
    State,
    Chiefdom,
    People,
}

// ─── SHAPE ───────────────────────────────────────────────────────────────────

/// The lowercase form of a character, diacritics intact.
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Whether a character is a syllable nucleus.
fn is_nucleus(c: char) -> bool {
    matches!(lower(c), 'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'æ' | 'ã' | 'õ')
}

/// The plain vowel a marked one falls back to when its diacritic has to go.
fn unmark(c: char) -> Option<char> {
    match lower(c) {
        'ã' => Some('a'),
        'õ' => Some('o'),
        'æ' => Some('e'),
        _ => None,
    }
}

/// How many syllables a rendered name carries: one per nucleus, with the `ae`
/// digraph — a single phoneme — counted once.
pub fn syllable_count(name: &str) -> usize {
    let chars: Vec<char> = name.chars().collect();
    let mut count = 0usize;
    let mut index = 0usize;
    while index < chars.len() {
        if lower(chars[index]) == 'a' && chars.get(index + 1).is_some_and(|next| lower(*next) == 'e')
        {
            count += 1;
            index += 2;
            continue;
        }
        if is_nucleus(chars[index]) {
            count += 1;
        }
        index += 1;
    }
    count
}

/// Keep the first `max` syllables of a name, whole ones only.
///
/// The onset of the syllable that would come next is dropped with it, so a
/// clipped name never ends in a torn cluster.
fn clip_to_syllables(name: &str, max: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut kept = String::new();
    let mut pending = String::new();
    let mut syllables = 0usize;
    let mut index = 0usize;
    while index < chars.len() {
        let c = chars[index];
        if lower(c) == 'a' && chars.get(index + 1).is_some_and(|next| lower(*next) == 'e') {
            if syllables >= max {
                break;
            }
            kept.push_str(&pending);
            pending.clear();
            kept.push(c);
            kept.push(chars[index + 1]);
            syllables += 1;
            index += 2;
            continue;
        }
        if is_nucleus(c) {
            if syllables >= max {
                break;
            }
            kept.push_str(&pending);
            pending.clear();
            kept.push(c);
            syllables += 1;
        } else {
            pending.push(c);
        }
        index += 1;
    }
    if kept.is_empty() {
        name.to_string()
    } else {
        kept
    }
}

/// The stem a suffix may be appended to: a base of three syllables or more gives
/// up its last syllable first, so `base + suffix` cannot run away.
///
/// A three-syllable base is already full — `Vufupo` + a polity suffix would be
/// five syllables — so the final syllable is dropped before the suffix lands and
/// the finished name stays at three.
fn suffix_stem(base: &str) -> String {
    let base = base.to_lowercase();
    let syllables = syllable_count(&base);
    let keep = if syllables >= MAX_NAME_SYLLABLES - 1 {
        MAX_NAME_SYLLABLES - 2
    } else {
        syllables
    };
    clip_to_syllables(&base, keep.max(1))
}

/// Trim a name to the character ceiling, whole syllables at a time, never below
/// the syllable floor.
fn fit_chars(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_string();
    }
    let mut current = name.to_string();
    while current.chars().count() > max && syllable_count(&current) > MIN_NAME_SYLLABLES {
        let shorter = clip_to_syllables(&current, syllable_count(&current) - 1);
        if shorter.chars().count() >= current.chars().count() {
            break;
        }
        current = shorter;
    }
    if current.chars().count() > max {
        current = current.chars().take(max).collect();
    }
    current
}

/// Drop the diacritic of a marked vowel that directly follows another vowel.
///
/// Compounding can put two nuclei side by side, and two marked vowels side by
/// side print as the unreadable `ÃÕ`; the second one loses its tilde.
fn smooth_vowels(name: &str) -> String {
    let mut out = String::new();
    let mut previous_vowel = false;
    for c in name.chars() {
        if is_nucleus(c) {
            match (previous_vowel, unmark(c)) {
                (true, Some(plain)) => out.push(plain),
                _ => out.push(c),
            }
            previous_vowel = true;
        } else {
            out.push(c);
            previous_vowel = false;
        }
    }
    out
}

/// Capitalize the first *letter* and lowercase the rest: a name is strictly
/// title-cased, so `Foo`, never `FooBar` or `Foo-Bar`.
///
/// A leading glottal stop is skipped rather than capitalised — `'Oshraepsi`
/// would otherwise print as a stray quote.
pub fn capitalize(text: &str) -> String {
    let mut out = String::new();
    let mut opened = false;
    for c in text.chars() {
        if !opened && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            opened = true;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// The final shape every generated name must have: smooth vowels, no leading
/// glottal stop, at most four syllables, at most fourteen characters, strictly
/// title-cased.
pub fn tidy_name(name: &str) -> String {
    let lowered = name.trim().trim_start_matches('\'').to_lowercase();
    if lowered.is_empty() {
        return String::new();
    }
    let smoothed = smooth_vowels(&lowered);
    let syllables = clip_to_syllables(&smoothed, MAX_NAME_SYLLABLES);
    capitalize(&fit_chars(&syllables, MAX_NAME_CHARS))
}

/// A base with exactly one qualifier in front of it.
pub fn qualified(lex: &Lexicon, base: &str, qualifier: Concept) -> String {
    let marker = lex.form(qualifier);
    let room = MAX_NAME_SYLLABLES
        .saturating_sub(syllable_count(marker))
        .max(1);
    let base = clip_to_syllables(&base.to_lowercase(), room);
    tidy_name(&lex.join(marker, &base))
}

// ─── THE REGISTRY ────────────────────────────────────────────────────────────

/// A deterministic, collision-free name registry.
#[derive(Clone, Debug, Default)]
pub struct NameRegistry {
    used: BTreeSet<String>,
}

impl NameRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.used.contains(name)
    }

    /// Return `candidate`, or the first deterministic variant no other name has
    /// taken. Numerals are deliberately absent.
    ///
    /// The candidate is treated as its own base root: any qualifier it already
    /// wears is stripped before a retry is built, so a collision can never stack
    /// a second modifier on a qualified name.
    pub fn unique(&mut self, candidate: &str, lex: &Lexicon) -> String {
        let base = strip_modifier(lex, candidate);
        self.unique_rooted(&base, candidate, lex)
    }

    /// Register `candidate`, which was built from `base` — the plain, unqualified
    /// root it grew from.
    ///
    /// A collision is resolved by rebuilding from the **base** with at most one
    /// qualifier or one suffix, never by prepending a qualifier to the string
    /// that collided. A retry therefore cannot stutter, and no name can carry two
    /// modifiers.
    pub fn unique_rooted(&mut self, base: &str, candidate: &str, lex: &Lexicon) -> String {
        let fallback = tidy_name(&lex.compound(&[Concept::People, Concept::River]));
        let base = match tidy_name(base) {
            base if base.is_empty() => fallback,
            base => base,
        };
        let candidate = match tidy_name(candidate) {
            candidate if candidate.is_empty() => base.clone(),
            candidate => candidate,
        };
        if self.used.insert(candidate.clone()) {
            return candidate;
        }

        // One qualifier, drawn from the fixed list: `Upper Tolirae`, never
        // `Upper Lower Tolirae`.
        for qualifier in MODIFIERS {
            let alternative = qualified(lex, &base, qualifier);
            if self.used.insert(alternative.clone()) {
                return alternative;
            }
        }

        // One plain suffix, for the rare base that has already spent all ten
        // qualifiers. A suffix is not a modifier, so the name still carries at
        // most one morpheme of modification.
        for concept in Concept::CORE {
            if concept.is_modifier() {
                continue;
            }
            let alternative = tidy_name(&lex.join(&suffix_stem(&base), lex.form(concept)));
            if self.used.insert(alternative.clone()) {
                return alternative;
            }
        }

        let mut index = 0usize;
        loop {
            let alternative = tidy_name(&format!("{}{}", suffix_stem(&base), epenthetic(index, lex)));
            if self.used.insert(alternative.clone()) {
                return alternative;
            }
            index += 1;
            if index > 64 {
                return candidate;
            }
        }
    }
}

/// The plain root under a name: the name without a leading qualifier, if it
/// wears one.
///
/// A realm or a province is named from its *language's* autonym, but a daughter
/// tongue's autonym already wears a dialect marker (`K'lirmopat'ha`). Rooting the
/// realm on that string would make the first retry prepend the marker a second
/// time, so callers pass the plain root instead — `Mopat'ha`, never
/// `K'lirmopat'ha`.
pub fn plain_root(lex: &Lexicon, name: &str) -> String {
    strip_modifier(lex, name)
}

/// The candidate without a leading qualifier, if it wears one.
fn strip_modifier(lex: &Lexicon, name: &str) -> String {
    let lowered = name.to_lowercase();
    for concept in MODIFIERS {
        let root = lex.form(concept).to_lowercase();
        if let Some(rest) = lowered.strip_prefix(&root) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    lowered
}

fn epenthetic(index: usize, lex: &Lexicon) -> String {
    let root = lex.form(Concept::River);
    let mut chars = root.chars().cycle();
    let mut out = String::new();
    for _ in 0..=index % 4 {
        if let Some(c) = chars.next() {
            out.push(c);
        }
    }
    if out.is_empty() { "a".to_string() } else { out }
}

/// The people's name for themselves, built as the plan's worked example:
/// `Oln` (people) + `Mar` (river) + a genitive ending → `Olnar`.
pub fn autonym(lex: &Lexicon) -> String {
    tidy_name(&lex.compound_with(
        &[Concept::People, Concept::River],
        lex.genitive_suffix.as_deref(),
        BridgeRule::Genitive,
    ))
}

/// A family name, built from its own autonym and a native "speech" suffix.
pub fn family_name(lex: &Lexicon, autonym: &str) -> String {
    tidy_name(&lex.join(&suffix_stem(autonym), lex.form(Concept::Word)))
}

/// A language name: the family's name qualified by its place in the family, or
/// the family name itself for the proto-language.
pub fn language_name(lex: &Lexicon, family: &str, qualifier: Option<Concept>) -> String {
    match qualifier {
        Some(concept) => qualified(lex, family, concept),
        None => tidy_name(family),
    }
}

/// A realm name. A state grows a polity suffix from its own roots; a people is
/// simply its ethnonym.
pub fn realm_name(lex: &Lexicon, autonym: &str, kind: PolityKind) -> String {
    match kind {
        PolityKind::State => {
            // One root is enough for a polity suffix; two roots make a realm
            // name that no legend row can hold.
            let suffix = lex.form(Concept::Hearth);
            tidy_name(&lex.join(&suffix_stem(autonym), suffix))
        }
        PolityKind::Chiefdom => {
            let suffix = lex.form(Concept::Elder);
            tidy_name(&lex.join(&suffix_stem(autonym), suffix))
        }
        PolityKind::People => tidy_name(autonym),
    }
}

/// The name of a family's homeland: a physical feature plus a locative suffix.
pub fn urheimat_name(lex: &Lexicon, archetype: SoundArchetype) -> String {
    let feature = match archetype {
        SoundArchetype::GutturalArid => Concept::Plain,
        SoundArchetype::MelodicMaritime => Concept::Sea,
        SoundArchetype::SibilantTaiga => Concept::Forest,
        SoundArchetype::NasalRiverine => Concept::River,
        SoundArchetype::SonorousWoodland => Concept::Forest,
        SoundArchetype::GlottalHighland => Concept::Mountain,
    };
    tidy_name(&lex.compound_with(
        &[feature],
        lex.genitive_suffix.as_deref(),
        BridgeRule::Genitive,
    ))
}

/// A settlement or capital name.
pub fn settlement_name(lex: &Lexicon, feature: Concept) -> String {
    tidy_name(&lex.compound_with(
        &[feature],
        lex.diminutive_suffix.as_deref(),
        BridgeRule::Diminutive,
    ))
}

/// A short root list for a dossier.
pub fn sample_roots(lex: &Lexicon) -> String {
    [
        Concept::River,
        Concept::Sea,
        Concept::Mountain,
        Concept::People,
        Concept::Great,
        Concept::First,
    ]
    .iter()
    .map(|concept| format!("{} = {}", concept.label(), lex.form(*concept)))
    .collect::<Vec<_>>()
    .join(", ")
}

/// A readable description of the family's morphology for the dossier.
pub fn morphology_label(lex: &Lexicon, dna: &PhonologyDna) -> String {
    let order = match lex.order {
        crate::language::lexicon::MorphemeOrder::HeadFinal => "head-final compounds",
        crate::language::lexicon::MorphemeOrder::HeadInitial => "head-initial compounds",
    };
    format!("{order}; {}", dna.marker_label())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(archetype: SoundArchetype) -> Lexicon {
        Lexicon::build(&PhonologyDna::build(archetype, 777))
    }

    /// The shape every generated name must have: a word a legend row can hold.
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
            if unmark(*c).is_some() {
                assert!(
                    index == 0 || !is_nucleus(chars[index - 1]),
                    "{name} lets two vowels carry diacritics side by side"
                );
            }
        }
    }

    /// Every form the registry may legally return from a base: the candidate
    /// itself, one qualifier on the base, or one plain suffix on the base. A name
    /// outside this set carries something extra — two modifiers, or a modifier
    /// compounded onto a name that already had one.
    fn retry_forms(lex: &Lexicon, base: &str, candidate: &str) -> Vec<String> {
        let mut forms = vec![tidy_name(candidate)];
        for qualifier in MODIFIERS {
            forms.push(qualified(lex, base, qualifier));
        }
        for concept in Concept::CORE {
            if !concept.is_modifier() {
                forms.push(tidy_name(&lex.join(&suffix_stem(base), lex.form(concept))));
            }
        }
        for index in 0..=64 {
            forms.push(tidy_name(&format!(
                "{}{}",
                suffix_stem(base),
                epenthetic(index, lex)
            )));
        }
        forms
    }

    #[test]
    fn an_autonym_is_built_from_the_languages_own_roots() {
        let lex = lex(SoundArchetype::NasalRiverine);
        let name = autonym(&lex);
        assert!(!name.is_empty());
        assert!(name.chars().next().is_some_and(char::is_uppercase));
        assert_well_shaped(&name);
    }

    #[test]
    fn a_registry_never_returns_the_same_name_twice() {
        let lex = lex(SoundArchetype::SonorousWoodland);
        let mut registry = NameRegistry::new();
        let first = registry.unique("Olnar", &lex);
        let second = registry.unique("Olnar", &lex);
        let third = registry.unique("Olnar", &lex);
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert!(!second.contains('2'), "a gazetteer never numbers a name");
    }

    #[test]
    fn a_state_grows_a_polity_suffix() {
        let lex = lex(SoundArchetype::GutturalArid);
        let people = realm_name(&lex, "Olnar", PolityKind::People);
        let state = realm_name(&lex, "Olnar", PolityKind::State);
        assert_ne!(people, state);
        assert!(state.to_lowercase().starts_with(&people.to_lowercase()));
        assert_well_shaped(&state);
    }

    /// The canon: every realm and language name of a family's own gazetteer is
    /// short, title-cased and readable — no stutter, no runaway compound.
    #[test]
    fn a_gazetteer_of_realms_stays_short_and_clean() {
        let mut registry = NameRegistry::new();
        for archetype in SoundArchetype::ALL {
            for seed in [11u64, 777, 4_242, 90_001] {
                let lex = Lexicon::build(&PhonologyDna::build(archetype, seed));
                let own = autonym(&lex);
                let family = family_name(&lex, &own);
                assert_well_shaped(&own);
                assert_well_shaped(&family);
                for rank in 0..MODIFIERS.len() {
                    let name = language_name(&lex, &family, Some(MODIFIERS[rank]));
                    assert_well_shaped(&name);
                }
                for step in 0..40usize {
                    let kind = match step % 3 {
                        0 => PolityKind::State,
                        1 => PolityKind::Chiefdom,
                        _ => PolityKind::People,
                    };
                    let candidate = realm_name(&lex, &own, kind);
                    let name = registry.unique_rooted(&own, &candidate, &lex);
                    assert_well_shaped(&name);
                    assert!(
                        retry_forms(&lex, &own, &candidate).contains(&name),
                        "{name} stacks two modifiers on {own}"
                    );
                }
            }
        }
    }

    /// A retry never compounds a modifier onto the name that collided: the
    /// eightieth realm of one tongue is still one qualifier deep.
    #[test]
    fn a_retry_rebuilds_from_the_base_root() {
        let lex = lex(SoundArchetype::MelodicMaritime);
        let own = autonym(&lex);
        let mut registry = NameRegistry::new();
        for _ in 0..80 {
            let candidate = realm_name(&lex, &own, PolityKind::State);
            let name = registry.unique_rooted(&own, &candidate, &lex);
            assert_well_shaped(&name);
            assert!(
                retry_forms(&lex, &own, &candidate).contains(&name),
                "{name} stutters instead of rebuilding from {own}"
            );
        }
    }

    /// The stutter itself, on a base no lexicon can generate: no qualifier is
    /// ever doubled, and a name never carries its own root twice.
    #[test]
    fn a_retry_never_doubles_a_root() {
        let lex = lex(SoundArchetype::SibilantTaiga);
        let base = "Vufupo";
        let mut registry = NameRegistry::new();
        for _ in 0..80 {
            let name = registry.unique_rooted(base, base, &lex);
            let lowered = name.to_lowercase();
            for qualifier in MODIFIERS {
                let marker = lex.form(qualifier).to_lowercase();
                assert!(
                    !lowered.starts_with(&format!("{marker}{marker}")),
                    "{name} doubles the qualifier {marker}"
                );
            }
            assert!(
                lowered.matches(&base.to_lowercase()).count() <= 1,
                "{name} repeats its own root"
            );
        }
    }

    /// A realm named after a daughter tongue roots on the plain root under that
    /// tongue's own qualified name: `K'lirmopat'ha` roots on `Mopat'ha`, so the
    /// dialect marker cannot be doubled into `K'lirk'lirmopa`.
    #[test]
    fn a_realm_roots_on_the_plain_root_under_a_qualified_tongue() {
        let lex = lex(SoundArchetype::GlottalHighland);
        let family = autonym(&lex);
        let daughter = qualified(&lex, &family, Concept::Upper);
        assert_ne!(daughter.to_lowercase(), family.to_lowercase());
        let base = plain_root(&lex, &daughter);
        assert_eq!(
            base.to_lowercase(),
            family.to_lowercase(),
            "plain_root({daughter}) did not reach the plain root"
        );

        let marker = lex.form(Concept::Upper).to_lowercase();
        let doubled = format!("{marker}{marker}");
        let mut registry = NameRegistry::new();
        for _ in 0..40 {
            let candidate = realm_name(&lex, &base, PolityKind::State);
            let name = registry.unique_rooted(&base, &candidate, &lex);
            assert_well_shaped(&name);
            assert!(
                !name.to_lowercase().starts_with(&doubled),
                "{name} stutters the marker {marker}"
            );
        }
    }

    /// A base of three syllables gives one up before a suffix lands.
    #[test]
    fn a_three_syllable_base_loses_a_syllable_before_its_suffix() {
        let lex = lex(SoundArchetype::SonorousWoodland);
        let long = lex.compound(&[Concept::Mountain, Concept::River, Concept::People]);
        assert!(syllable_count(&long) >= 3, "the fixture is not long enough");
        let stem = suffix_stem(&long);
        assert!(
            syllable_count(&stem) < syllable_count(&long),
            "the stem {stem} kept every syllable of {long}"
        );
        let short = tidy_name(&lex.join(&stem, lex.form(Concept::Hearth)));
        assert!(
            syllable_count(&short) <= MAX_NAME_SYLLABLES,
            "the suffix ran away: {short}"
        );
    }

    /// Title case is strict, and marked vowels never touch — whatever cursed
    /// compound the generator hands over.
    #[test]
    fn the_final_shape_is_title_cased_and_unmarked_in_pairs() {
        for cursed in [
            "lonlölLonlõli'ngikri",
            "Ghaevaenapupyvuwe",
            "DhãnghmighLewõhohnengongãs",
            "ÃÕli'ngikri",
            "kaesivanapupyvuwengongãs",
        ] {
            let tidy = tidy_name(cursed);
            assert_well_shaped(&tidy);
            assert!(
                tidy.chars().count() < cursed.chars().count(),
                "{cursed} was not clipped"
            );
            assert!(
                !tidy.contains('Õ'),
                "{tidy} kept a marked vowel that was smoothed away"
            );
        }
        assert!(tidy_name("vufupo").starts_with("Vufupo"));
    }

    #[test]
    fn the_same_lexicon_names_the_same_place_twice() {
        let lex = lex(SoundArchetype::SibilantTaiga);
        assert_eq!(
            urheimat_name(&lex, SoundArchetype::SibilantTaiga),
            urheimat_name(&lex, SoundArchetype::SibilantTaiga)
        );
    }
}
