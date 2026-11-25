//! The seven levels of a civilization's religion.
//!
//! The old dossier answered "what does this realm believe?" with a single cult
//! label drawn from its cradle. This module answers the same question the way a
//! real comparative religion does: as a **cascade**, where each level narrows the
//! next.
//!
//! 1. **Metaphysical root** — what kind of divine order there is at all
//!    (monotheism, dualism, animism, nontheism, …).
//! 2. **Domain architecture** — the object of reverence (gods, goddesses,
//!    spirits, ancestors, souls, a principle or a teacher), how many there are,
//!    and what each holds.
//! 3. **Cosmology** — where the world came from, where time goes, how it ends,
//!    and what happens to the dead.
//! 4. **Ethos** — what the faith praises, forbids, promises and makes of
//!    outsiders.
//! 5. **Praxis** — the rites, the transmission of the tradition and the calendar
//!    that keeps it.
//! 6. **Polity** — who speaks for the faith, and how it stands to the state.
//! 7. **Manifestation** — its sacred place, its orientation, its emblem and the
//!    pattern its name follows.
//!
//! The draw is not a uniform walk through 200-odd terms. Every level is read off
//! the realm's own country and society — a river floodplain, a cold forest, a
//! mercantile coast, a militarised frontier — and the higher levels are filtered
//! through the lower ones. Hard constraints keep the impossible out: an animist
//! people never grows an ecclesiarchy, a nontheist creed never keeps a living
//! oracle, a polycentric pantheon never sends missionaries.
//!
//! ## Determinism
//!
//! The stage uses its own RNG, seeded from `seed ^ FAITH_SALT` by the generator,
//! so the dawn's other draws — cradles, cultures, populations, prehistory — are
//! untouched by this module. Every collection walked here is a fixed slice or a
//! `BTreeMap`, and every term has a stated fallback. The same seed therefore
//! produces the same faiths, in the same order, forever.
//!
//! ## What is approximate
//!
//! The history act has no tectonic, volcanic or seasonal-rainfall grid. "Tectonic/volcanic"
//! is read as high, broken ground, and "monsoon/ITCZ" as wet tropical and savanna
//! country. Those approximations are marked where they are made.

use super::{CradleKind, Environment, Government, Hierarchy, Outlook, SocietyTier, Subsistence};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::BTreeMap;

/// Salt mixed into the world seed for the faith draw. Deliberately independent
/// of the dawn's own stream, so remaking religion cannot move a cradle.
pub const FAITH_SALT: u64 = 0x0F41_7A11_0000_0007;

/// How many domains a faith may hold at each end of a cardinality.
pub const MIN_POLYCENTRIC_DOMAINS: usize = 3;
pub const MAX_POLYCENTRIC_DOMAINS: usize = 6;
pub const MAX_HEGEMONIC_SECONDARIES: usize = 4;
pub const MAX_DYADIC_SECONDARIES: usize = 2;

// ─── LEVEL 1: THE METAPHYSICAL ROOT ─────────────────────────────────────────

/// What kind of divine order a faith holds to exist.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum MetaphysicalRoot {
    /// One god, and no other.
    Monotheism,
    /// Two co-eternal powers, in tension or war.
    Dualism,
    /// One god above a lesser host, worshipped one at a time.
    Henotheism,
    /// Many gods, none absolute.
    Polytheism,
    /// A world full of spirits, places and kin rather than gods.
    Animism,
    /// The divine is the world itself.
    Pantheism,
    /// The world is within the divine, and the divine within it.
    Panentheism,
    /// A creator inferred from the order of things, who does not intervene.
    Deism,
    /// No god is posited: the tradition is a path, not a worship.
    Nontheism,
}

impl MetaphysicalRoot {
    pub const ALL: [MetaphysicalRoot; 9] = [
        MetaphysicalRoot::Monotheism,
        MetaphysicalRoot::Dualism,
        MetaphysicalRoot::Henotheism,
        MetaphysicalRoot::Polytheism,
        MetaphysicalRoot::Animism,
        MetaphysicalRoot::Pantheism,
        MetaphysicalRoot::Panentheism,
        MetaphysicalRoot::Deism,
        MetaphysicalRoot::Nontheism,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MetaphysicalRoot::Monotheism => "Monotheism",
            MetaphysicalRoot::Dualism => "Dualism",
            MetaphysicalRoot::Henotheism => "Henotheism",
            MetaphysicalRoot::Polytheism => "Polytheism",
            MetaphysicalRoot::Animism => "Animism",
            MetaphysicalRoot::Pantheism => "Pantheism",
            MetaphysicalRoot::Panentheism => "Panentheism",
            MetaphysicalRoot::Deism => "Deism",
            MetaphysicalRoot::Nontheism => "Nontheism",
        }
    }

    /// The noun a name may be built from: `the Polytheist Covenant`.
    pub fn noun(self) -> &'static str {
        match self {
            MetaphysicalRoot::Monotheism => "Monotheist",
            MetaphysicalRoot::Dualism => "Dualist",
            MetaphysicalRoot::Henotheism => "Henotheist",
            MetaphysicalRoot::Polytheism => "Polytheist",
            MetaphysicalRoot::Animism => "Animist",
            MetaphysicalRoot::Pantheism => "Pantheist",
            MetaphysicalRoot::Panentheism => "Panentheist",
            MetaphysicalRoot::Deism => "Deist",
            MetaphysicalRoot::Nontheism => "Nontheist",
        }
    }

    /// True for the two roots whose domain architecture must stay philosophical.
    pub fn is_philosophical(self) -> bool {
        matches!(self, MetaphysicalRoot::Deism | MetaphysicalRoot::Nontheism)
    }

    /// This root's index in [`MetaphysicalRoot::ALL`], for the per-root tables
    /// the Religions map mode reads.
    pub fn index(self) -> usize {
        match self {
            MetaphysicalRoot::Monotheism => 0,
            MetaphysicalRoot::Dualism => 1,
            MetaphysicalRoot::Henotheism => 2,
            MetaphysicalRoot::Polytheism => 3,
            MetaphysicalRoot::Animism => 4,
            MetaphysicalRoot::Pantheism => 5,
            MetaphysicalRoot::Panentheism => 6,
            MetaphysicalRoot::Deism => 7,
            MetaphysicalRoot::Nontheism => 8,
        }
    }

    /// How many shades one metaphysical root's colour family carries.
    ///
    /// Ten tones: five lightness bands and, for a root with chroma to spare, two
    /// hue sides. Nine was enough for a planar map's greedy walk, but a Religions
    /// map puts a dozen faiths of one root beside each other — Polytheism alone
    /// can hold a third of a world's creeds — and ten is the most a family can
    /// carry while every tone stays clear of the painter's sea.
    pub const SHADES: usize = 10;

    /// How many lightness bands a colour family walks, from ink to paper.
    const BANDS: usize = 5;

    /// The main colour of this metaphysical root on the Religions map mode.
    ///
    /// Nine hues spread around the wheel, each a *family* rather than a single
    /// fill: a faith of this root wears one of its [`shade`](Self::shade)s, so
    /// two religions of one kind read as relatives while remaining distinct.
    /// Every colour is kept away from the painter's water and ice (see the
    /// palette tests in [`super`]), because a religion that fades into the sea
    /// is no religion a reader can find.
    pub fn legend_color(self) -> [u8; 3] {
        match self {
            MetaphysicalRoot::Monotheism => [200, 160, 60],
            MetaphysicalRoot::Dualism => [166, 54, 62],
            MetaphysicalRoot::Henotheism => [124, 66, 148],
            MetaphysicalRoot::Polytheism => [192, 72, 142],
            MetaphysicalRoot::Animism => [62, 120, 66],
            MetaphysicalRoot::Pantheism => [28, 140, 128],
            MetaphysicalRoot::Panentheism => [128, 138, 58],
            MetaphysicalRoot::Deism => [160, 154, 146],
            MetaphysicalRoot::Nontheism => [92, 92, 96],
        }
    }

    /// The `index`-th shade of this root's colour family.
    ///
    /// The family is a real neighbourhood, not a categorical palette: a shade is
    /// the root's own colour walked from ink to paper and — where the root has
    /// chroma to spare — leaned a little in hue. Two religions of one root
    /// therefore read as relatives while remaining clearly distinct fills.
    ///
    /// The lean is always *away* from the painter's water hues, never toward
    /// them: a violet family drifting into the navy shallows, or a teal family
    /// drifting onto the shelf, is a religion the sea has swallowed. A grey root
    /// (deism, nontheism) has no hue to lean, so it spends all ten tones on
    /// lightness instead.
    ///
    /// The walk visits the family *contrast-first* — the root's own colour, then
    /// the darkest, then the lightest, then inwards — because the greedy colouring
    /// hands neighbours consecutive indices; a plain ramp would give two
    /// neighbouring faiths the two most similar shades in the family.
    /// Deterministic, and the index wraps.
    pub fn shade(self, index: usize) -> [u8; 3] {
        /// The walk over the five bands and the two hue sides: the root's own
        /// tone first, then the darkest and the lightest, then the pairs in
        /// between. Every `(band, side)` appears exactly once.
        const WALK: [(usize, bool); MetaphysicalRoot::SHADES] = [
            (2, false),
            (0, false),
            (4, true),
            (1, true),
            (3, false),
            (1, false),
            (3, true),
            (0, true),
            (4, false),
            (2, true),
        ];
        /// Ten plain lightness bands, for a root with no hue to lean.
        const GREY_WALK: [usize; MetaphysicalRoot::SHADES] = [0, 9, 1, 8, 2, 7, 3, 6, 4, 5];
        /// How far the darkest and lightest tones travel, as a share of the
        /// root's own value. Wide enough that the ten tones are ten fills a reader
        /// can tell apart, still inside the family's own band.
        const VALUE_REACH: f32 = 0.30;
        /// How far the far hue side leans, in degrees — inside the twelve the
        /// family test allows, and always away from the sea.
        const HUE_REACH: f32 = 12.0;
        /// Below this saturation a root is a grey: it has no hue worth leaning.
        const GREY: f32 = 0.30;
        /// The hue of the painter's water — sea, shelf and lake all sit in
        /// `205..240°`, so that is the band a family leans away from.
        const WATER_HUE: f32 = 210.0;

        let (hue, saturation, value) = super::rgb_to_hsv(self.legend_color());
        if saturation < GREY {
            let band =
                GREY_WALK[index % Self::SHADES] as f32 / (Self::SHADES - 1) as f32 * 2.0 - 1.0;
            return super::hsv_to_rgb(hue, saturation, value * (1.0 + VALUE_REACH * band));
        }
        let (band, far_side) = WALK[index % Self::SHADES];
        let value_step = band as f32 / (Self::BANDS - 1) as f32 * 2.0 - 1.0;
        let lean = if hue < WATER_HUE {
            -HUE_REACH
        } else {
            HUE_REACH
        };
        let tone_hue = if far_side { hue + lean } else { hue };
        super::hsv_to_rgb(
            tone_hue,
            saturation,
            value * (1.0 + VALUE_REACH * value_step),
        )
    }
}

// ─── LEVEL 2A: CARDINALITY ─────────────────────────────────────────────────

/// How many primary powers a faith has, and how many serve beneath them.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum Cardinality {
    /// One primary, no secondaries.
    Singular,
    /// Two co-primaries, and up to two secondaries.
    Dyadic,
    /// One primary, with one to four secondaries.
    Hegemonic,
    /// Three to six equal powers, with no primary.
    Polycentric,
    /// No powers at all: the root itself is the whole creed.
    Abstract,
}

impl Cardinality {
    #[allow(dead_code)]
    pub const ALL: [Cardinality; 5] = [
        Cardinality::Singular,
        Cardinality::Dyadic,
        Cardinality::Hegemonic,
        Cardinality::Polycentric,
        Cardinality::Abstract,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Cardinality::Singular => "Singular",
            Cardinality::Dyadic => "Dyadic",
            Cardinality::Hegemonic => "Hegemonic",
            Cardinality::Polycentric => "Polycentric",
            Cardinality::Abstract => "Abstract",
        }
    }

    /// How many leading domains are primary or co-primary. Polycentric faiths
    /// deliberately report none: their powers are equals, not a ranked host.
    pub fn primary_count(self) -> usize {
        match self {
            Cardinality::Singular | Cardinality::Hegemonic => 1,
            Cardinality::Dyadic => 2,
            Cardinality::Polycentric | Cardinality::Abstract => 0,
        }
    }

    /// How many secondary domains this cardinality can carry.
    pub fn secondary_capacity(self) -> usize {
        match self {
            Cardinality::Singular | Cardinality::Abstract => 0,
            Cardinality::Dyadic => MAX_DYADIC_SECONDARIES,
            Cardinality::Hegemonic => MAX_HEGEMONIC_SECONDARIES,
            Cardinality::Polycentric => 0,
        }
    }
}

// ─── LEVEL 2B: THE OBJECT OF REVERENCE ──────────────────────────────────────

/// The gender a theistic faith ascribes to its god or gods.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum DivineGender {
    Masculine,
    Feminine,
}

/// What an animist faith reveres.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum SpiritKind {
    Spirits,
    Totems,
    Ancestors,
    Souls,
}

/// The principle at the heart of a nontheist faith.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum NontheistConcept {
    Path,
    Way,
    Truth,
    Harmony,
}

/// The human figure at the heart of a nontheist faith.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum NontheistPerson {
    Teacher,
    Sage,
    Ascended,
    Enlightened,
}

/// Whether a nontheist faith reveres a principle or a person.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum NontheistFocus {
    Concept(NontheistConcept),
    Person(NontheistPerson),
}

/// The kind of being or principle a faith reveres.
///
/// This is the umbrella the dossier never prints: its heading is the concrete
/// title ([`Divinity::title`]), so a faith of teachers is never called a
/// "Deity" and a faith of principles is never called a "God".
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum Divinity {
    Theistic(DivineGender),
    Animistic(SpiritKind),
    Nontheist(NontheistFocus),
}

impl Divinity {
    #[allow(dead_code)]
    pub const ALL: [Divinity; 14] = [
        Divinity::Theistic(DivineGender::Masculine),
        Divinity::Theistic(DivineGender::Feminine),
        Divinity::Animistic(SpiritKind::Spirits),
        Divinity::Animistic(SpiritKind::Totems),
        Divinity::Animistic(SpiritKind::Ancestors),
        Divinity::Animistic(SpiritKind::Souls),
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Path)),
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Way)),
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Truth)),
        Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Harmony)),
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Teacher)),
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Sage)),
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Ascended)),
        Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Enlightened)),
    ];

    /// The English heading a dossier uses. `plural` only affects gods and
    /// goddesses; every other class has its own grammatical number.
    pub fn title(self, plural: bool) -> &'static str {
        match self {
            Divinity::Theistic(DivineGender::Masculine) => {
                if plural {
                    "Gods"
                } else {
                    "God"
                }
            }
            Divinity::Theistic(DivineGender::Feminine) => {
                if plural {
                    "Goddesses"
                } else {
                    "Goddess"
                }
            }
            Divinity::Animistic(SpiritKind::Spirits) => "Spirits",
            Divinity::Animistic(SpiritKind::Totems) => "Totems",
            Divinity::Animistic(SpiritKind::Ancestors) => "Ancestors",
            Divinity::Animistic(SpiritKind::Souls) => "Souls",
            Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Path)) => "Path",
            Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Way)) => "Way",
            Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Truth)) => "Truth",
            Divinity::Nontheist(NontheistFocus::Concept(NontheistConcept::Harmony)) => "Harmony",
            Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Teacher)) => "Teacher",
            Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Sage)) => "Sage",
            Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Ascended)) => "Ascended",
            Divinity::Nontheist(NontheistFocus::Person(NontheistPerson::Enlightened)) => {
                "Enlightened"
            }
        }
    }
}

// ─── LEVEL 2C: DOMAINS ──────────────────────────────────────────────────────

/// The four families of domains, used for grouping and for the philosophical
/// roots' domain bias.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum DomainCategory {
    Celestial,
    Elemental,
    Civic,
    Metaphysical,
}

impl DomainCategory {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            DomainCategory::Celestial => "Celestial",
            DomainCategory::Elemental => "Elemental",
            DomainCategory::Civic => "Civic",
            DomainCategory::Metaphysical => "Metaphysical",
        }
    }
}

/// One power, principle or province of a faith.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Domain {
    Sky,
    Solar,
    Lunar,
    Astral,
    Storm,
    Light,
    Shadow,
    Eclipse,
    Earth,
    Water,
    Ocean,
    Fire,
    Wind,
    Frost,
    Volcano,
    Flora,
    Fauna,
    Decay,
    Ancestor,
    War,
    Sovereignty,
    Fertility,
    Underworld,
    Death,
    Hunt,
    Craft,
    Trade,
    Law,
    Wisdom,
    Healing,
    Hearth,
    CosmicOrder,
    Transcendence,
    Fate,
    Chaos,
    Harmony,
    Cycle,
    Entropy,
    Truth,
}

impl Domain {
    pub const ALL: &'static [Domain] = &[
        Domain::Sky,
        Domain::Solar,
        Domain::Lunar,
        Domain::Astral,
        Domain::Storm,
        Domain::Light,
        Domain::Shadow,
        Domain::Eclipse,
        Domain::Earth,
        Domain::Water,
        Domain::Ocean,
        Domain::Fire,
        Domain::Wind,
        Domain::Frost,
        Domain::Volcano,
        Domain::Flora,
        Domain::Fauna,
        Domain::Decay,
        Domain::Ancestor,
        Domain::War,
        Domain::Sovereignty,
        Domain::Fertility,
        Domain::Underworld,
        Domain::Death,
        Domain::Hunt,
        Domain::Craft,
        Domain::Trade,
        Domain::Law,
        Domain::Wisdom,
        Domain::Healing,
        Domain::Hearth,
        Domain::CosmicOrder,
        Domain::Transcendence,
        Domain::Fate,
        Domain::Chaos,
        Domain::Harmony,
        Domain::Cycle,
        Domain::Entropy,
        Domain::Truth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Domain::Sky => "Sky",
            Domain::Solar => "Solar",
            Domain::Lunar => "Lunar",
            Domain::Astral => "Astral",
            Domain::Storm => "Storm",
            Domain::Light => "Light",
            Domain::Shadow => "Shadow",
            Domain::Eclipse => "Eclipse",
            Domain::Earth => "Earth",
            Domain::Water => "Water",
            Domain::Ocean => "Ocean",
            Domain::Fire => "Fire",
            Domain::Wind => "Wind",
            Domain::Frost => "Frost",
            Domain::Volcano => "Volcano",
            Domain::Flora => "Flora",
            Domain::Fauna => "Fauna",
            Domain::Decay => "Decay",
            Domain::Ancestor => "Ancestor",
            Domain::War => "War",
            Domain::Sovereignty => "Sovereignty",
            Domain::Fertility => "Fertility",
            Domain::Underworld => "Underworld",
            Domain::Death => "Death",
            Domain::Hunt => "Hunt",
            Domain::Craft => "Craft",
            Domain::Trade => "Trade",
            Domain::Law => "Law",
            Domain::Wisdom => "Wisdom",
            Domain::Healing => "Healing",
            Domain::Hearth => "Hearth",
            Domain::CosmicOrder => "Cosmic Order",
            Domain::Transcendence => "Transcendence",
            Domain::Fate => "Fate",
            Domain::Chaos => "Chaos",
            Domain::Harmony => "Harmony",
            Domain::Cycle => "Cycle",
            Domain::Entropy => "Entropy",
            Domain::Truth => "Truth",
        }
    }
}

impl Domain {
    /// A name-friendly form: `Sovereignty` becomes `Crown`, so a faith can be
    /// the Order of the Crown rather than the Order of the Sovereignty.
    pub fn noun(self) -> &'static str {
        match self {
            Domain::Sky => "Sky",
            Domain::Solar => "Sun",
            Domain::Lunar => "Moon",
            Domain::Astral => "Stars",
            Domain::Storm => "Storm",
            Domain::Light => "Light",
            Domain::Shadow => "Shadow",
            Domain::Eclipse => "Eclipse",
            Domain::Earth => "Earth",
            Domain::Water => "Water",
            Domain::Ocean => "Sea",
            Domain::Fire => "Flame",
            Domain::Wind => "Wind",
            Domain::Frost => "Frost",
            Domain::Volcano => "Volcano",
            Domain::Flora => "Green",
            Domain::Fauna => "Beast",
            Domain::Decay => "Ash",
            Domain::Ancestor => "Ancestors",
            Domain::War => "War",
            Domain::Sovereignty => "Crown",
            Domain::Fertility => "Harvest",
            Domain::Underworld => "Deep",
            Domain::Death => "Death",
            Domain::Hunt => "Hunt",
            Domain::Craft => "Forge",
            Domain::Trade => "Road",
            Domain::Law => "Law",
            Domain::Wisdom => "Wisdom",
            Domain::Healing => "Healing",
            Domain::Hearth => "Hearth",
            Domain::CosmicOrder => "Order",
            Domain::Transcendence => "Beyond",
            Domain::Fate => "Fate",
            Domain::Chaos => "Chaos",
            Domain::Harmony => "Harmony",
            Domain::Cycle => "Wheel",
            Domain::Entropy => "Ruin",
            Domain::Truth => "Truth",
        }
    }
}

impl Domain {
    pub fn category(self) -> DomainCategory {
        match self {
            Domain::Sky
            | Domain::Solar
            | Domain::Lunar
            | Domain::Astral
            | Domain::Storm
            | Domain::Light
            | Domain::Shadow
            | Domain::Eclipse => DomainCategory::Celestial,
            Domain::Earth
            | Domain::Water
            | Domain::Ocean
            | Domain::Fire
            | Domain::Wind
            | Domain::Frost
            | Domain::Volcano
            | Domain::Flora
            | Domain::Fauna
            | Domain::Decay => DomainCategory::Elemental,
            Domain::Ancestor
            | Domain::War
            | Domain::Sovereignty
            | Domain::Fertility
            | Domain::Underworld
            | Domain::Death
            | Domain::Hunt
            | Domain::Craft
            | Domain::Trade
            | Domain::Law
            | Domain::Wisdom
            | Domain::Healing
            | Domain::Hearth => DomainCategory::Civic,
            Domain::CosmicOrder
            | Domain::Transcendence
            | Domain::Fate
            | Domain::Chaos
            | Domain::Harmony
            | Domain::Cycle
            | Domain::Entropy
            | Domain::Truth => DomainCategory::Metaphysical,
        }
    }
}

// ─── LEVEL 3: COSMOLOGY AND ESCHATOLOGY ────────────────────────────────────

/// Where the world came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Cosmogony {
    ExNihilo,
    CosmicOrdering,
    WorldParent,
    EarthDiver,
    CosmicEgg,
    Emanation,
    DivineConflict,
    Eternal,
}

impl Cosmogony {
    pub const ALL: [Cosmogony; 8] = [
        Cosmogony::ExNihilo,
        Cosmogony::CosmicOrdering,
        Cosmogony::WorldParent,
        Cosmogony::EarthDiver,
        Cosmogony::CosmicEgg,
        Cosmogony::Emanation,
        Cosmogony::DivineConflict,
        Cosmogony::Eternal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Cosmogony::ExNihilo => "Ex Nihilo",
            Cosmogony::CosmicOrdering => "Cosmic Ordering",
            Cosmogony::WorldParent => "World-Parent",
            Cosmogony::EarthDiver => "Earth-Diver",
            Cosmogony::CosmicEgg => "Cosmic Egg",
            Cosmogony::Emanation => "Emanation",
            Cosmogony::DivineConflict => "Divine Conflict",
            Cosmogony::Eternal => "Eternal",
        }
    }
}

/// The shape of time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Temporality {
    Linear,
    Cyclical,
    Spiral,
    Degenerative,
    Progressive,
    Timeless,
}

impl Temporality {
    pub const ALL: [Temporality; 6] = [
        Temporality::Linear,
        Temporality::Cyclical,
        Temporality::Spiral,
        Temporality::Degenerative,
        Temporality::Progressive,
        Temporality::Timeless,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Temporality::Linear => "Linear",
            Temporality::Cyclical => "Cyclical",
            Temporality::Spiral => "Spiral",
            Temporality::Degenerative => "Degenerative",
            Temporality::Progressive => "Progressive",
            Temporality::Timeless => "Timeless",
        }
    }
}

/// How the world ends — or whether it does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Eschaton {
    FinalJudgment,
    CosmicRenewal,
    EndlessContinuity,
    GreatReturn,
    Dissolution,
    Apotheosis,
}

impl Eschaton {
    pub const ALL: [Eschaton; 6] = [
        Eschaton::FinalJudgment,
        Eschaton::CosmicRenewal,
        Eschaton::EndlessContinuity,
        Eschaton::GreatReturn,
        Eschaton::Dissolution,
        Eschaton::Apotheosis,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Eschaton::FinalJudgment => "Final Judgment",
            Eschaton::CosmicRenewal => "Cosmic Renewal",
            Eschaton::EndlessContinuity => "Endless Continuity",
            Eschaton::GreatReturn => "Great Return",
            Eschaton::Dissolution => "Dissolution",
            Eschaton::Apotheosis => "Apotheosis",
        }
    }
}

/// What happens to the dead.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Afterlife {
    DualRealms,
    Reincarnation,
    ShadowUnderworld,
    AncestralPlane,
    HeroicHall,
    Absorption,
    AstralAscent,
    Oblivion,
}

impl Afterlife {
    pub const ALL: [Afterlife; 8] = [
        Afterlife::DualRealms,
        Afterlife::Reincarnation,
        Afterlife::ShadowUnderworld,
        Afterlife::AncestralPlane,
        Afterlife::HeroicHall,
        Afterlife::Absorption,
        Afterlife::AstralAscent,
        Afterlife::Oblivion,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Afterlife::DualRealms => "Dual Realms",
            Afterlife::Reincarnation => "Reincarnation",
            Afterlife::ShadowUnderworld => "Shadow Underworld",
            Afterlife::AncestralPlane => "Ancestral Plane",
            Afterlife::HeroicHall => "Heroic Hall",
            Afterlife::Absorption => "Absorption",
            Afterlife::AstralAscent => "Astral Ascent",
            Afterlife::Oblivion => "Oblivion",
        }
    }
}

// ─── LEVEL 4: ETHOS AND SOTERIOLOGY ────────────────────────────────────────

/// What the faith praises.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Virtue {
    Obedience,
    Valor,
    Harmony,
    Compassion,
    Asceticism,
    Purity,
    Stewardship,
    Wisdom,
    Hospitality,
    Fecundity,
    Honor,
    Detachment,
}

impl Virtue {
    pub const ALL: [Virtue; 12] = [
        Virtue::Obedience,
        Virtue::Valor,
        Virtue::Harmony,
        Virtue::Compassion,
        Virtue::Asceticism,
        Virtue::Purity,
        Virtue::Stewardship,
        Virtue::Wisdom,
        Virtue::Hospitality,
        Virtue::Fecundity,
        Virtue::Honor,
        Virtue::Detachment,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Virtue::Obedience => "Obedience",
            Virtue::Valor => "Valor",
            Virtue::Harmony => "Harmony",
            Virtue::Compassion => "Compassion",
            Virtue::Asceticism => "Asceticism",
            Virtue::Purity => "Purity",
            Virtue::Stewardship => "Stewardship",
            Virtue::Wisdom => "Wisdom",
            Virtue::Hospitality => "Hospitality",
            Virtue::Fecundity => "Fecundity",
            Virtue::Honor => "Honor",
            Virtue::Detachment => "Detachment",
        }
    }
}

/// The one thing the faith cannot forgive.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Taboo {
    Idolatry,
    Apostasy,
    KinSlaying,
    OathBreaking,
    Pollution,
    Hubris,
    Desecration,
    Cowardice,
    Hoarding,
    Stagnation,
}

impl Taboo {
    pub const ALL: [Taboo; 10] = [
        Taboo::Idolatry,
        Taboo::Apostasy,
        Taboo::KinSlaying,
        Taboo::OathBreaking,
        Taboo::Pollution,
        Taboo::Hubris,
        Taboo::Desecration,
        Taboo::Cowardice,
        Taboo::Hoarding,
        Taboo::Stagnation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Taboo::Idolatry => "Idolatry",
            Taboo::Apostasy => "Apostasy",
            Taboo::KinSlaying => "Kin-Slaying",
            Taboo::OathBreaking => "Oath-Breaking",
            Taboo::Pollution => "Pollution",
            Taboo::Hubris => "Hubris",
            Taboo::Desecration => "Desecration",
            Taboo::Cowardice => "Cowardice",
            Taboo::Hoarding => "Hoarding",
            Taboo::Stagnation => "Stagnation",
        }
    }
}

/// How salvation, liberation or the good life is reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Soteriology {
    Grace,
    Works,
    Gnosis,
    Liberation,
    RitualExactness,
    Glory,
    Lineage,
    Submission,
}

impl Soteriology {
    pub const ALL: [Soteriology; 8] = [
        Soteriology::Grace,
        Soteriology::Works,
        Soteriology::Gnosis,
        Soteriology::Liberation,
        Soteriology::RitualExactness,
        Soteriology::Glory,
        Soteriology::Lineage,
        Soteriology::Submission,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Soteriology::Grace => "Grace",
            Soteriology::Works => "Works",
            Soteriology::Gnosis => "Gnosis",
            Soteriology::Liberation => "Liberation",
            Soteriology::RitualExactness => "Ritual Exactness",
            Soteriology::Glory => "Glory",
            Soteriology::Lineage => "Lineage",
            Soteriology::Submission => "Submission",
        }
    }
}

/// What the faith makes of outsiders.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum OutgroupStance {
    Missionary,
    Exclusive,
    Syncretic,
    Pluralistic,
    Secretive,
}

impl OutgroupStance {
    pub const ALL: [OutgroupStance; 5] = [
        OutgroupStance::Missionary,
        OutgroupStance::Exclusive,
        OutgroupStance::Syncretic,
        OutgroupStance::Pluralistic,
        OutgroupStance::Secretive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OutgroupStance::Missionary => "Missionary",
            OutgroupStance::Exclusive => "Exclusive",
            OutgroupStance::Syncretic => "Syncretic",
            OutgroupStance::Pluralistic => "Pluralistic",
            OutgroupStance::Secretive => "Secretive",
        }
    }
}

// ─── LEVEL 5: PRAXIS AND RITUAL ─────────────────────────────────────────────

/// The central act of worship.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Rite {
    AnimalSacrifice,
    BloodOffering,
    BurntOffering,
    Libation,
    Prayer,
    Meditation,
    EcstaticTrance,
    Procession,
    Fasting,
    Pilgrimage,
    SacredDebate,
    Monumentalism,
}

impl Rite {
    pub const ALL: [Rite; 12] = [
        Rite::AnimalSacrifice,
        Rite::BloodOffering,
        Rite::BurntOffering,
        Rite::Libation,
        Rite::Prayer,
        Rite::Meditation,
        Rite::EcstaticTrance,
        Rite::Procession,
        Rite::Fasting,
        Rite::Pilgrimage,
        Rite::SacredDebate,
        Rite::Monumentalism,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Rite::AnimalSacrifice => "Animal Sacrifice",
            Rite::BloodOffering => "Blood Offering",
            Rite::BurntOffering => "Burnt Offering",
            Rite::Libation => "Libation",
            Rite::Prayer => "Prayer",
            Rite::Meditation => "Meditation",
            Rite::EcstaticTrance => "Ecstatic Trance",
            Rite::Procession => "Procession",
            Rite::Fasting => "Fasting",
            Rite::Pilgrimage => "Pilgrimage",
            Rite::SacredDebate => "Sacred Debate",
            Rite::Monumentalism => "Monumentalism",
        }
    }
}

/// How the tradition is carried.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Transmission {
    WrittenCanon,
    OralEpic,
    Augury,
    Astrology,
    LivingOracle,
    Iconography,
    MysteryInitiation,
}

impl Transmission {
    pub const ALL: [Transmission; 7] = [
        Transmission::WrittenCanon,
        Transmission::OralEpic,
        Transmission::Augury,
        Transmission::Astrology,
        Transmission::LivingOracle,
        Transmission::Iconography,
        Transmission::MysteryInitiation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Transmission::WrittenCanon => "Written Canon",
            Transmission::OralEpic => "Oral Epic",
            Transmission::Augury => "Augury",
            Transmission::Astrology => "Astrology",
            Transmission::LivingOracle => "Living Oracle",
            Transmission::Iconography => "Iconography",
            Transmission::MysteryInitiation => "Mystery Initiation",
        }
    }
}

/// The event the sacred year hangs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CalendarAnchor {
    Solstice,
    Equinox,
    LunarPhase,
    FloodCycle,
    Monsoon,
    Harvest,
    Eclipse,
    FoundingEra,
}

impl CalendarAnchor {
    pub const ALL: [CalendarAnchor; 8] = [
        CalendarAnchor::Solstice,
        CalendarAnchor::Equinox,
        CalendarAnchor::LunarPhase,
        CalendarAnchor::FloodCycle,
        CalendarAnchor::Monsoon,
        CalendarAnchor::Harvest,
        CalendarAnchor::Eclipse,
        CalendarAnchor::FoundingEra,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CalendarAnchor::Solstice => "Solstice",
            CalendarAnchor::Equinox => "Equinox",
            CalendarAnchor::LunarPhase => "Lunar Phase",
            CalendarAnchor::FloodCycle => "Flood Cycle",
            CalendarAnchor::Monsoon => "Monsoon",
            CalendarAnchor::Harvest => "Harvest",
            CalendarAnchor::Eclipse => "Eclipse",
            CalendarAnchor::FoundingEra => "Founding Era",
        }
    }
}

// ─── LEVEL 6: ORGANIZATION AND POLITY ──────────────────────────────────────

/// Who performs the rites and speaks for the faith.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Clergy {
    Ecclesiarchy,
    SacredKingship,
    CivicMagistracy,
    Monasticism,
    Shamanism,
    MysteryCult,
    ClanElders,
    PropheticGuild,
    None,
}

impl Clergy {
    pub const ALL: [Clergy; 9] = [
        Clergy::Ecclesiarchy,
        Clergy::SacredKingship,
        Clergy::CivicMagistracy,
        Clergy::Monasticism,
        Clergy::Shamanism,
        Clergy::MysteryCult,
        Clergy::ClanElders,
        Clergy::PropheticGuild,
        Clergy::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Clergy::Ecclesiarchy => "Ecclesiarchy",
            Clergy::SacredKingship => "Sacred Kingship",
            Clergy::CivicMagistracy => "Civic Magistracy",
            Clergy::Monasticism => "Monasticism",
            Clergy::Shamanism => "Shamanism",
            Clergy::MysteryCult => "Mystery Cult",
            Clergy::ClanElders => "Clan Elders",
            Clergy::PropheticGuild => "Prophetic Guild",
            Clergy::None => "None",
        }
    }
}

/// How the faith stands to the state.
///
/// There is deliberately no `Outlawed` term: a persecuted cult is represented by
/// its `Secretive` outgroup stance and by the minority cults it plants abroad, not
/// by a second way of saying the same thing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum StateCoupling {
    Theocracy,
    StatePatronage,
    DualPower,
    SubordinateCult,
    Autonomous,
    FolkDecentralized,
}

impl StateCoupling {
    pub const ALL: [StateCoupling; 6] = [
        StateCoupling::Theocracy,
        StateCoupling::StatePatronage,
        StateCoupling::DualPower,
        StateCoupling::SubordinateCult,
        StateCoupling::Autonomous,
        StateCoupling::FolkDecentralized,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StateCoupling::Theocracy => "Theocracy",
            StateCoupling::StatePatronage => "State Patronage",
            StateCoupling::DualPower => "Dual Power",
            StateCoupling::SubordinateCult => "Subordinate Cult",
            StateCoupling::Autonomous => "Autonomous",
            StateCoupling::FolkDecentralized => "Folk Decentralized",
        }
    }
}

// ─── LEVEL 7: MANIFESTATION ────────────────────────────────────────────────

/// The place the faith holds sacred.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SacredLocus {
    Peak,
    RiverConfluence,
    Spring,
    Caldera,
    Oasis,
    Strait,
    Crater,
    AbyssalCoast,
    Monolith,
    Grove,
    Necropolis,
    Observatory,
}

impl SacredLocus {
    pub const ALL: [SacredLocus; 12] = [
        SacredLocus::Peak,
        SacredLocus::RiverConfluence,
        SacredLocus::Spring,
        SacredLocus::Caldera,
        SacredLocus::Oasis,
        SacredLocus::Strait,
        SacredLocus::Crater,
        SacredLocus::AbyssalCoast,
        SacredLocus::Monolith,
        SacredLocus::Grove,
        SacredLocus::Necropolis,
        SacredLocus::Observatory,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SacredLocus::Peak => "Peak",
            SacredLocus::RiverConfluence => "River Confluence",
            SacredLocus::Spring => "Spring",
            SacredLocus::Caldera => "Caldera",
            SacredLocus::Oasis => "Oasis",
            SacredLocus::Strait => "Strait",
            SacredLocus::Crater => "Crater",
            SacredLocus::AbyssalCoast => "Abyssal Coast",
            SacredLocus::Monolith => "Monolith",
            SacredLocus::Grove => "Grove",
            SacredLocus::Necropolis => "Necropolis",
            SacredLocus::Observatory => "Observatory",
        }
    }
}

/// The direction the sacred place and its rites face.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum OrientationAxis {
    Sunrise,
    Sunset,
    Zenith,
    North,
    South,
    Upstream,
    Downstream,
    Peakward,
    Seaward,
    Omnidirectional,
}

impl OrientationAxis {
    pub const ALL: [OrientationAxis; 10] = [
        OrientationAxis::Sunrise,
        OrientationAxis::Sunset,
        OrientationAxis::Zenith,
        OrientationAxis::North,
        OrientationAxis::South,
        OrientationAxis::Upstream,
        OrientationAxis::Downstream,
        OrientationAxis::Peakward,
        OrientationAxis::Seaward,
        OrientationAxis::Omnidirectional,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OrientationAxis::Sunrise => "Sunrise",
            OrientationAxis::Sunset => "Sunset",
            OrientationAxis::Zenith => "Zenith",
            OrientationAxis::North => "North",
            OrientationAxis::South => "South",
            OrientationAxis::Upstream => "Upstream",
            OrientationAxis::Downstream => "Downstream",
            OrientationAxis::Peakward => "Peakward",
            OrientationAxis::Seaward => "Seaward",
            OrientationAxis::Omnidirectional => "Omnidirectional",
        }
    }
}

/// The mark the faith is known by.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Emblem {
    Disk,
    Crescent,
    Star,
    Spiral,
    Wave,
    Flame,
    Mountain,
    Tree,
    Serpent,
    Sword,
    Eye,
    Wheel,
    Void,
}

impl Emblem {
    pub const ALL: [Emblem; 13] = [
        Emblem::Disk,
        Emblem::Crescent,
        Emblem::Star,
        Emblem::Spiral,
        Emblem::Wave,
        Emblem::Flame,
        Emblem::Mountain,
        Emblem::Tree,
        Emblem::Serpent,
        Emblem::Sword,
        Emblem::Eye,
        Emblem::Wheel,
        Emblem::Void,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Emblem::Disk => "Disk",
            Emblem::Crescent => "Crescent",
            Emblem::Star => "Star",
            Emblem::Spiral => "Spiral",
            Emblem::Wave => "Wave",
            Emblem::Flame => "Flame",
            Emblem::Mountain => "Mountain",
            Emblem::Tree => "Tree",
            Emblem::Serpent => "Serpent",
            Emblem::Sword => "Sword",
            Emblem::Eye => "Eye",
            Emblem::Wheel => "Wheel",
            Emblem::Void => "Void",
        }
    }
}

/// The grammatical pattern a faith's name follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum NamingPattern {
    RootEpithet,
    DomainOrder,
    FounderPath,
    LocusCovenant,
    DualPolarity,
}

impl NamingPattern {
    #[allow(dead_code)]
    pub const ALL: [NamingPattern; 5] = [
        NamingPattern::RootEpithet,
        NamingPattern::DomainOrder,
        NamingPattern::FounderPath,
        NamingPattern::LocusCovenant,
        NamingPattern::DualPolarity,
    ];

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            NamingPattern::RootEpithet => "Root + Epithet",
            NamingPattern::DomainOrder => "Domain + Order",
            NamingPattern::FounderPath => "Founder + Path",
            NamingPattern::LocusCovenant => "Locus + Covenant",
            NamingPattern::DualPolarity => "Dual Polarity",
        }
    }
}

// ─── THE FAITH ──────────────────────────────────────────────────────────────

/// The mark a daughter faith carries from its ancestor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum FaithRecension {
    Reformed,
    Old,
    High,
    Low,
    True,
}

impl FaithRecension {
    pub const ALL: [FaithRecension; 5] = [
        FaithRecension::Reformed,
        FaithRecension::Old,
        FaithRecension::High,
        FaithRecension::Low,
        FaithRecension::True,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FaithRecension::Reformed => "Reformed",
            FaithRecension::Old => "Old",
            FaithRecension::High => "High",
            FaithRecension::Low => "Low",
            FaithRecension::True => "True",
        }
    }
}

/// The native names of a faith's named parts. Empty until the history act's
/// naming phase runs; every reader falls back to the English label.
///
/// Only the faith name, its deities and the words for their domains, the core
/// of an abstract faith, and its emblem are named. The rest of the cascade —
/// creed, cosmology, ethos, path, rites, temple, manifestation — stays English.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FaithNames {
    /// Parallel to [`Faith::domains`]: the native word for each domain.
    pub domain_nouns: Vec<String>,
    /// Parallel to [`Faith::domains`]: the native name of each deity, spirit,
    /// ancestor, principle or teacher associated with that domain.
    pub deities: Vec<String>,
    /// The native name of the core concept, teacher or god of an abstract faith.
    pub core: String,
    /// The named emblem itself.
    pub emblem: String,
    /// The name of the liturgical tongue, when the faith keeps one.
    pub liturgy: String,
}

/// A minority tradition planted in a realm by a secretive neighbour.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cult {
    /// Native after the naming phase; the descriptive mystery before it.
    pub name: String,
    /// The English working name. Hidden once a native name exists.
    pub gloss: String,
    /// Native name of the power the cult venerates.
    pub power_name: String,
    pub root: MetaphysicalRoot,
    pub domain: Domain,
}

/// One holy place of a faith, named in its own tongue.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SacredSite {
    /// Native proper name of the site.
    pub name: String,
    /// The province it lies in.
    pub province: u32,
    /// The power it honours, when it is a power's own site.
    pub domain: Option<Domain>,
    /// True for the faith's chief holy place.
    pub primary: bool,
}

/// One civilization's religion, read as a cascade from root to manifestation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Faith {
    /// Native after the naming phase; the English name before it.
    pub name: String,
    /// The descriptive English working name. Hidden once a native name exists.
    pub gloss: String,
    /// Set when a daughter realm inherited this faith.
    pub recension: Option<FaithRecension>,
    pub root: MetaphysicalRoot,
    pub cardinality: Cardinality,
    /// Primaries first, then secondaries. A polycentric faith stores all its
    /// equals here; an abstract faith stores nothing.
    pub domains: Vec<Domain>,

    pub cosmogony: Cosmogony,
    pub temporality: Temporality,
    pub eschaton: Eschaton,
    pub afterlife: Afterlife,

    /// One or two things the faith praises.
    pub virtues: Vec<Virtue>,
    pub taboo: Taboo,
    pub soteriology: Soteriology,
    pub stance: OutgroupStance,

    /// One or two central rites.
    pub rites: Vec<Rite>,
    /// One or two ways the tradition is carried.
    pub transmissions: Vec<Transmission>,
    pub calendar: CalendarAnchor,

    pub clergy: Clergy,
    /// How the creed's own ideal polity stands to a state.
    ///
    /// A faith is a world entity, not a possession of one realm, so this is a
    /// property of the creed rather than of any country. The atlas does not yet
    /// assign a state religion: it reports what people keep, not what a throne
    /// patronises.
    pub state_coupling: StateCoupling,

    pub locus: SacredLocus,
    pub orientation: OrientationAxis,
    pub emblem: Emblem,
    pub naming: NamingPattern,

    /// Native names for the named pass: faith, deities, domain words, emblem.
    /// Filled by the history act.
    pub names: FaithNames,
    /// The proto-language used for the sacred vocabulary, when the faith keeps one.
    pub liturgical_language: Option<u16>,
    /// The faith's holy places, best first. Filled by the naming phase.
    pub sacred_sites: Vec<SacredSite>,

    pub cults: Vec<Cult>,
}

impl Faith {
    /// The leading domains: primary for a singular or hegemonic faith,
    /// co-primary for a dyadic one, and none for a polycentric or abstract one.
    pub fn primary_domains(&self) -> &[Domain] {
        let count = self.cardinality.primary_count();
        &self.domains[..count.min(self.domains.len())]
    }

    /// The domains beneath the primaries. A polycentric faith has equals, not
    /// secondaries; an abstract faith has none.
    pub fn secondary_domains(&self) -> &[Domain] {
        if matches!(
            self.cardinality,
            Cardinality::Polycentric | Cardinality::Abstract
        ) {
            return &[];
        }
        let start = self.primary_domains().len();
        &self.domains[start..]
    }

    /// The kind of being or principle this faith reveres.
    pub fn divinity(&self) -> Divinity {
        derive_divinity(self)
    }

    /// Whether the faith speaks of its divinity in the plural.
    fn divine_plural(&self) -> bool {
        match self.divinity() {
            Divinity::Theistic(_) => match self.root {
                MetaphysicalRoot::Monotheism
                | MetaphysicalRoot::Pantheism
                | MetaphysicalRoot::Panentheism => false,
                MetaphysicalRoot::Deism => !matches!(
                    self.cardinality,
                    Cardinality::Abstract | Cardinality::Singular
                ),
                MetaphysicalRoot::Dualism | MetaphysicalRoot::Polytheism => true,
                MetaphysicalRoot::Henotheism => self.cardinality != Cardinality::Singular,
                MetaphysicalRoot::Animism | MetaphysicalRoot::Nontheism => false,
            },
            Divinity::Animistic(_) => true,
            Divinity::Nontheist(_) => false,
        }
    }

    /// The dossier heading for the faith's powers: `God`, `Goddesses`,
    /// `Ancestors`, `Way`, `Teacher`, and so on.
    pub fn deity_title(&self) -> &'static str {
        self.divinity().title(self.divine_plural())
    }

    /// One line per power, or one line for the core of an abstract faith.
    ///
    /// `Lunar — Ler (Moon) — Lertow'etbo`, or `T'asag'idi (the Creator)`.
    pub fn deity_lines(&self) -> Vec<String> {
        if self.domains.is_empty() {
            let core = self.names.core.trim();
            return if core.is_empty() {
                vec![self.deity_title().to_string()]
            } else {
                vec![named(core, &self.core_gloss())]
            };
        }
        self.domains
            .iter()
            .enumerate()
            .map(|(index, domain)| {
                let native = self
                    .names
                    .domain_nouns
                    .get(index)
                    .map_or("", String::as_str);
                let deity = self.names.deities.get(index).map_or("", String::as_str);
                let noun = named(native, domain.noun());
                if deity.is_empty() || deity == native {
                    format!("{} — {noun}", domain.label())
                } else {
                    format!("{} — {noun} — {deity}", domain.label())
                }
            })
            .collect()
    }

    /// The English gloss of an abstract faith's core object.
    pub fn core_gloss(&self) -> String {
        match self.divinity() {
            Divinity::Theistic(_) => match self.root {
                MetaphysicalRoot::Monotheism | MetaphysicalRoot::Deism => "the Creator".to_string(),
                _ => self.deity_title().to_string(),
            },
            Divinity::Animistic(_) => self.deity_title().to_string(),
            Divinity::Nontheist(_) => format!("the {}", self.deity_title()),
        }
    }

    fn join_virtues(&self) -> String {
        if self.virtues.is_empty() {
            return "—".to_string();
        }
        self.virtues
            .iter()
            .map(|virtue| virtue.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn join_rites(&self) -> String {
        if self.rites.is_empty() {
            return "—".to_string();
        }
        self.rites
            .iter()
            .map(|rite| rite.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn join_transmissions(&self) -> String {
        if self.transmissions.is_empty() {
            return "—".to_string();
        }
        self.transmissions
            .iter()
            .map(|transmission| transmission.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `Polytheism · Hegemonic` — the shape of the creed, without its domains.
    pub fn creed(&self) -> String {
        format!("{} · {}", self.root.label(), self.cardinality.label())
    }

    /// `Cosmic Ordering · Cyclical · Endless Continuity · Shadow Underworld`.
    pub fn cosmos(&self) -> String {
        [
            self.cosmogony.label(),
            self.temporality.label(),
            self.eschaton.label(),
            self.afterlife.label(),
        ]
        .join(" · ")
    }

    /// `Obedience, Purity` — the virtues alone; the taboo has its own row.
    pub fn ethos(&self) -> String {
        self.join_virtues()
    }

    /// `Desecration` — the one thing the faith cannot forgive.
    pub fn taboo(&self) -> String {
        self.taboo.label().to_string()
    }
}

impl Faith {
    /// `Ritual Exactness · Syncretic`.
    pub fn path(&self) -> String {
        format!("{} · {}", self.soteriology.label(), self.stance.label())
    }

    /// `Animal Sacrifice, Monumentalism · Written Canon, Augury · Equinox`.
    pub fn praxis(&self) -> String {
        format!(
            "{} · {} · {}",
            self.join_rites(),
            self.join_transmissions(),
            self.calendar.label()
        )
    }

    /// `Ecclesiarchy · Dual Power`.
    pub fn polity(&self) -> String {
        format!("{} · {}", self.clergy.label(), self.state_coupling.label())
    }

    /// `River Confluence · Sunrise`. The emblem has its own row.
    pub fn manifestation(&self) -> String {
        format!("{} · {}", self.locus.label(), self.orientation.label())
    }

    /// `Suhu (Eye)` — the native emblem with its English gloss.
    pub fn emblem_name(&self) -> String {
        named(self.names.emblem.as_str(), self.emblem.label())
    }

    /// `Sacred Site` or `Sacred Sites`, by how many the faith holds.
    pub fn sacred_site_title(&self) -> &'static str {
        if self.sacred_sites.len() <= 1 {
            "Sacred Site"
        } else {
            "Sacred Sites"
        }
    }

    /// One line per holy place: `Kinõlawu — province #2493`, with the primary
    /// place and any power's own site marked once the faith has more than one.
    pub fn sacred_site_lines(&self) -> Vec<String> {
        let many = self.sacred_sites.len() > 1;
        let mut seen: Vec<Domain> = Vec::new();
        self.sacred_sites
            .iter()
            .map(|site| {
                let mut line = format!("{} — province #{}", site.name, site.province);
                if many {
                    if site.primary {
                        line.push_str(" — primary");
                    } else if let Some(domain) = site.domain {
                        if !seen.contains(&domain) {
                            seen.push(domain);
                            line.push_str(&format!(" — {}", domain.label()));
                        }
                    }
                }
                line
            })
            .collect()
    }

    /// The faith's display name: its own proper name. The descriptive English
    /// working name is never shown once a native name exists.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.gloss.clone()
        } else {
            self.name.clone()
        }
    }

    /// The names of the minority cults, if any.
    pub fn cult_names(&self) -> String {
        self.cults
            .iter()
            .map(|cult| {
                let mut label = cult.display_name();
                if !cult.power_name.is_empty() {
                    label.push_str(&format!(" · power {}", cult.power_name));
                }
                label
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Cult {
    /// The cult's display name: its own proper name, once named.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.gloss.clone()
        } else {
            self.name.clone()
        }
    }
}

/// A native name with its English gloss, or the gloss alone when no native
/// name has been assigned yet.
fn named(native: &str, gloss: &str) -> String {
    if native.is_empty() || native == gloss {
        gloss.to_string()
    } else {
        format!("{native} ({gloss})")
    }
}

// ─── THE COUNTRY A FAITH GROWS IN ───────────────────────────────────────────

/// Everything the faith draw is allowed to know about a realm. Built by the
/// generator from the realm's own cradle, economy and government, so this module
/// never reaches back into the stage's private bookkeeping.
#[derive(Clone, Copy, Debug)]
pub struct FaithContext {
    pub environment: Environment,
    pub cradle_kind: CradleKind,
    pub tier: SocietyTier,
    /// Development in `0..=1`, the same axis the government draw reads.
    pub complexity: f32,
    pub subsistence: Subsistence,
    pub outlook: Outlook,
    pub government: Government,
    pub hierarchy: Hierarchy,
    pub mean_temp_c: f32,
    pub mean_precip: f32,
    pub mean_elevation_m: f32,
    pub sea_px: u32,
    pub lake_px: u32,
    /// The seat sits on a river large enough for the stage's ordinary river test.
    pub river: bool,
    /// The seat sits on one of the world's great rivers.
    pub great_river: bool,
    pub latitude_deg: f32,
}

impl FaithContext {
    fn on_sea(&self) -> bool {
        self.sea_px > 0
    }

    fn on_lake(&self) -> bool {
        self.lake_px > 0
    }
}

/// The environmental and social signals every domain and virtue is scored by.
#[derive(Clone, Copy, Debug, Default)]
struct Signals {
    arid: f32,
    riverine: f32,
    coastal: f32,
    island: f32,
    highland: f32,
    cold: f32,
    forest: f32,
    tropical: f32,
    nomadic: f32,
    agrarian: f32,
    urban: f32,
    martial: f32,
    scholarly: f32,
    small_scale: f32,
    complexity: f32,
}

impl Signals {
    fn from(ctx: &FaithContext) -> Self {
        let arid = match ctx.environment {
            Environment::Desert => 1.0,
            _ if ctx.mean_precip < 0.20 => 0.6,
            _ if ctx.mean_precip < 0.30 => 0.3,
            _ => 0.0,
        };
        let riverine = if matches!(
            ctx.cradle_kind,
            CradleKind::RiverFloodplain | CradleKind::RiverDelta | CradleKind::Lacustrine
        ) || ctx.great_river
        {
            1.0
        } else if ctx.river || ctx.on_lake() {
            0.6
        } else {
            0.0
        };
        let coastal = if ctx.on_sea() { 1.0 } else { 0.0 };
        let island = if matches!(ctx.environment, Environment::IslandShore)
            || matches!(ctx.cradle_kind, CradleKind::IslandShore)
        {
            1.0
        } else {
            0.0
        };
        let highland = match ctx.environment {
            Environment::Highland => 1.0,
            _ if ctx.mean_elevation_m > 1_200.0 => 0.8,
            _ if ctx.mean_elevation_m > 600.0 => 0.4,
            _ => 0.0,
        };
        let cold = match ctx.environment {
            Environment::IceMargin | Environment::Tundra => 1.0,
            Environment::Taiga => 0.8,
            _ if ctx.mean_temp_c < 0.0 => 1.0,
            _ if ctx.mean_temp_c < 8.0 => 0.5,
            _ => 0.0,
        };
        let forest = match ctx.environment {
            Environment::Woodland => 0.8f32,
            Environment::Taiga | Environment::TropicalForest => 1.0,
            _ => 0.0,
        }
        .max(match ctx.cradle_kind {
            CradleKind::WoodlandHeartland | CradleKind::RainforestClearing => 1.0,
            _ => 0.0,
        });
        let tropical = match ctx.environment {
            Environment::TropicalForest => 1.0,
            Environment::Savanna => 0.8,
            _ if ctx.mean_temp_c > 24.0 => 0.6,
            _ => 0.0,
        };
        let nomadic = match ctx.tier {
            SocietyTier::NomadicPastoralists => 1.0,
            _ => match ctx.subsistence {
                Subsistence::Pastoralism => 1.0,
                Subsistence::Foraging => 0.6,
                _ => 0.0,
            },
        };
        let agrarian = match ctx.subsistence {
            Subsistence::IrrigatedAgriculture
            | Subsistence::DrylandFarming
            | Subsistence::TerraceFarming
            | Subsistence::ShiftingCultivation => 1.0,
            _ => 0.0,
        };
        let urban = if ctx.tier.is_state() {
            (0.5 + 0.5 * ctx.complexity
                + match ctx.outlook {
                    Outlook::Cosmopolitan | Outlook::Mercantile => 0.3,
                    _ => 0.0,
                })
            .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let martial = (match ctx.outlook {
            Outlook::Martial => 1.0f32,
            _ => 0.0,
        } + match ctx.subsistence {
            Subsistence::Pastoralism => 0.4,
            _ => 0.0,
        } + highland * 0.3)
            .clamp(0.0, 1.0);
        let scholarly = (match ctx.outlook {
            Outlook::Scholarly => 1.0,
            Outlook::Cosmopolitan => 0.4,
            Outlook::Artistic => 0.3,
            _ => 0.0,
        } + ctx.complexity * 0.3)
            .clamp(0.0, 1.0);
        let small_scale = match ctx.tier {
            SocietyTier::HunterGatherers => 1.0,
            SocietyTier::NomadicPastoralists => 0.75,
            SocietyTier::SimpleFarmers => 0.55,
            SocietyTier::ComplexFarmers => 0.30,
            SocietyTier::States => 0.0,
        };
        Self {
            arid,
            riverine,
            coastal,
            island,
            highland,
            cold,
            forest,
            tropical,
            nomadic,
            agrarian,
            urban,
            martial,
            scholarly,
            small_scale,
            complexity: ctx.complexity.clamp(0.0, 1.0),
        }
    }
}

// ─── WEIGHTED DRAWS ─────────────────────────────────────────────────────────

/// The one weighted-draw routine the dossier builders share: one uniform roll
/// against the summed weights, with a stated fallback if floating-point rounding
/// ever leaves the roll past the last entry, or if every weight was filtered out.
pub(crate) fn weighted_pick<T: Copy>(rng: &mut ChaCha8Rng, options: &[(T, f32)], fallback: T) -> T {
    let total: f32 = options.iter().map(|(_, weight)| weight.max(0.0)).sum();
    if total <= 0.0 {
        return fallback;
    }
    let mut roll = rng.random::<f32>() * total;
    for &(value, weight) in options {
        let weight = weight.max(0.0);
        if roll < weight {
            return value;
        }
        roll -= weight;
    }
    options
        .iter()
        .rev()
        .find(|(_, weight)| *weight > 0.0)
        .map_or(fallback, |(value, _)| *value)
}

/// Draw one term from a fixed slice by a weight function.
fn choose<T: Copy>(
    rng: &mut ChaCha8Rng,
    items: &[T],
    mut weight: impl FnMut(T) -> f32,
    fallback: T,
) -> T {
    let options: Vec<(T, f32)> = items
        .iter()
        .copied()
        .map(|value| (value, weight(value).max(0.0)))
        .collect();
    weighted_pick(rng, &options, fallback)
}

/// Draw `count` distinct terms without replacement. If fewer than `count` terms
/// carry any weight, the draw stops rather than repeating one.
fn sample_unique<T: Copy + PartialEq>(
    rng: &mut ChaCha8Rng,
    items: &[T],
    mut weight: impl FnMut(T) -> f32,
    count: usize,
) -> Vec<T> {
    let mut pool: Vec<(T, f32)> = items
        .iter()
        .copied()
        .map(|value| (value, weight(value).max(0.0)))
        .collect();
    let mut chosen = Vec::with_capacity(count);
    while chosen.len() < count && !pool.is_empty() {
        let fallback = pool[0].0;
        let value = weighted_pick(rng, &pool, fallback);
        chosen.push(value);
        pool.retain(|(candidate, _)| *candidate != value);
    }
    chosen
}

/// Whether a faith already holds a domain.
fn has_domain(domains: &[Domain], domain: Domain) -> bool {
    domains.contains(&domain)
}

/// The domain a dyadic faith most wants beside `domain`: its mythic opposite.
fn opposite(domain: Domain) -> Option<Domain> {
    match domain {
        Domain::Light => Some(Domain::Shadow),
        Domain::Shadow => Some(Domain::Light),
        Domain::Sky => Some(Domain::Earth),
        Domain::Earth => Some(Domain::Sky),
        Domain::Solar => Some(Domain::Lunar),
        Domain::Lunar => Some(Domain::Solar),
        Domain::Fire => Some(Domain::Frost),
        Domain::Frost => Some(Domain::Fire),
        Domain::Fertility => Some(Domain::Decay),
        Domain::Decay => Some(Domain::Fertility),
        Domain::War => Some(Domain::Healing),
        Domain::Healing => Some(Domain::War),
        Domain::Chaos => Some(Domain::Harmony),
        Domain::Harmony => Some(Domain::Chaos),
        Domain::Cycle => Some(Domain::Entropy),
        Domain::Entropy => Some(Domain::Cycle),
        _ => None,
    }
}

// ─── LEVEL 1: DRAW THE ROOT ────────────────────────────────────────────────

fn draw_root(rng: &mut ChaCha8Rng, s: &Signals, ctx: &FaithContext) -> MetaphysicalRoot {
    let deism_open = ctx.complexity >= 0.50 && s.scholarly > 0.50;
    let nontheism_open = ctx.complexity >= 0.60 && s.scholarly > 0.60;
    let options: Vec<(MetaphysicalRoot, f32)> = MetaphysicalRoot::ALL
        .iter()
        .copied()
        .map(|root| {
            let weight = match root {
                MetaphysicalRoot::Animism => {
                    2.5 + 5.0 * s.small_scale + 2.0 * s.forest + 1.5 * s.cold + 1.0 * s.nomadic
                }
                MetaphysicalRoot::Polytheism => {
                    2.0 + 4.0 * s.complexity + 2.0 * s.agrarian + 1.5 * s.urban
                }
                MetaphysicalRoot::Henotheism => {
                    1.0 + 3.0 * s.nomadic + 1.5 * s.highland + 1.0 * s.martial
                }
                MetaphysicalRoot::Dualism => {
                    0.6 + 2.0 * s.martial + 1.2 * s.arid + 1.0 * s.cold + 1.0 * s.highland
                }
                MetaphysicalRoot::Monotheism => {
                    0.3 + 2.5 * s.complexity + 1.2 * s.agrarian + 0.8 * s.urban
                }
                MetaphysicalRoot::Pantheism => {
                    0.4 + 1.5 * s.scholarly + 1.2 * s.forest + 0.8 * s.tropical
                }
                MetaphysicalRoot::Panentheism => 0.4 + 1.6 * s.scholarly + 1.0 * s.highland,
                MetaphysicalRoot::Deism => {
                    if deism_open {
                        0.2 + 2.0 * s.scholarly + 1.0 * s.urban
                    } else {
                        0.0
                    }
                }
                MetaphysicalRoot::Nontheism => {
                    if nontheism_open {
                        0.2 + 2.2 * s.scholarly + 1.2 * s.urban
                    } else {
                        0.0
                    }
                }
            };
            (root, weight)
        })
        .collect();
    weighted_pick(rng, &options, MetaphysicalRoot::Animism)
}

// ─── LEVEL 2A: DRAW THE CARDINALITY ────────────────────────────────────────

fn draw_cardinality(rng: &mut ChaCha8Rng, root: MetaphysicalRoot) -> Cardinality {
    let options: &[(Cardinality, f32)] = match root {
        MetaphysicalRoot::Monotheism => {
            &[(Cardinality::Singular, 8.0), (Cardinality::Hegemonic, 2.0)]
        }
        MetaphysicalRoot::Dualism => &[(Cardinality::Dyadic, 10.0)],
        MetaphysicalRoot::Henotheism => {
            &[(Cardinality::Hegemonic, 8.0), (Cardinality::Singular, 2.0)]
        }
        MetaphysicalRoot::Polytheism => &[
            (Cardinality::Hegemonic, 4.0),
            (Cardinality::Polycentric, 7.0),
            (Cardinality::Dyadic, 2.0),
        ],
        MetaphysicalRoot::Animism => &[
            (Cardinality::Polycentric, 8.0),
            (Cardinality::Hegemonic, 2.0),
        ],
        MetaphysicalRoot::Pantheism => &[
            (Cardinality::Abstract, 4.0),
            (Cardinality::Singular, 4.0),
            (Cardinality::Polycentric, 2.0),
        ],
        MetaphysicalRoot::Panentheism => &[
            (Cardinality::Singular, 4.0),
            (Cardinality::Hegemonic, 4.0),
            (Cardinality::Abstract, 2.0),
        ],
        MetaphysicalRoot::Deism => &[(Cardinality::Abstract, 8.0), (Cardinality::Singular, 4.0)],
        MetaphysicalRoot::Nontheism => &[
            (Cardinality::Abstract, 8.0),
            (Cardinality::Singular, 4.0),
            (Cardinality::Polycentric, 2.0),
        ],
    };
    weighted_pick(rng, options, Cardinality::Singular)
}

// ─── LEVEL 2B: SCORE AND DRAW DOMAINS ───────────────────────────────────────

/// The environmental weight of a domain before the root's own character is
/// applied. The environmental triggers are the spec's table, split by the data
/// The history act actually has:
///
/// * arid country lifts Solar, Astral and Lunar;
/// * floodplains lift Water, Fertility and Sovereignty;
/// * coasts and islands lift Ocean, Wind and Trade;
/// * high ground stands in for tectonic/volcanic country, lifting Earth, Fire,
///   Volcano and Storm;
/// * wet tropical country stands in for the monsoon, lifting Storm, Water and
///   Fertility;
/// * cold country lifts Frost, Fire, Hearth, Ancestor and Hunt;
/// * forest lifts Flora, Fauna, Shadow and Ancestor;
/// * pastoral and foraging peoples lift Sky, Wind, Hunt, Ancestor and Fauna;
/// * agrarian states lift Fertility, Solar, Water, Sovereignty and Law;
/// * mercantile cities lift Craft, Trade, Law and Wisdom;
/// * a martial frontier lifts War, Death, Hunt, Storm and Sovereignty — while
///   the *virtues* Honor and Valor are biased separately in [`virtue_weight`].
fn domain_base(domain: Domain, s: &Signals) -> f32 {
    match domain {
        Domain::Sky => 1.0 + s.highland * 1.8 + s.nomadic * 1.5 + s.cold * 0.4,
        Domain::Solar => 1.0 + s.arid * 1.8 + s.agrarian * 1.5 + s.small_scale * 0.3,
        Domain::Lunar => 1.0 + s.arid * 1.2 + s.coastal * 0.8 + s.small_scale * 0.8,
        Domain::Astral => 1.0 + s.arid * 1.5 + s.highland * 0.8 + s.scholarly * 1.2,
        Domain::Storm => {
            1.0 + s.highland * 1.5 + s.tropical * 1.2 + s.martial * 0.8 + s.coastal * 0.5
        }
        Domain::Light => 1.0 + s.arid * 0.6 + s.scholarly * 0.8,
        Domain::Shadow => 1.0 + s.forest * 1.4 + s.scholarly * 0.4,
        Domain::Eclipse => 0.6 + s.arid * 0.8 + s.scholarly * 0.6,
        Domain::Earth => 1.0 + s.highland * 1.5 + s.agrarian * 0.8 + s.forest * 0.5,
        Domain::Water => 1.0 + s.riverine * 8.0 + s.arid * 0.6 + s.agrarian * 0.8,
        Domain::Ocean => 1.0 + s.coastal * 2.0 + s.island * 2.5,
        Domain::Fire => 1.0 + s.cold * 1.8 + s.highland * 0.8 + s.urban * 0.8,
        Domain::Wind => 1.0 + s.coastal * 1.2 + s.nomadic * 1.2 + s.highland * 0.8,
        Domain::Frost => 1.0 + s.cold * 2.5,
        Domain::Volcano => 0.5 + s.highland * 1.5 + s.arid * 0.4,
        Domain::Flora => 1.0 + s.forest * 2.0 + s.tropical * 1.2 + s.agrarian * 0.8,
        Domain::Fauna => 1.0 + s.forest * 1.5 + s.nomadic * 1.5 + s.tropical * 0.8,
        Domain::Decay => 0.8 + s.forest * 0.8 + s.small_scale * 0.5,
        _ => domain_base_civic(domain, s),
    }
}

fn domain_base_civic(domain: Domain, s: &Signals) -> f32 {
    match domain {
        Domain::Ancestor => {
            1.0 + s.small_scale * 1.5 + s.nomadic * 1.5 + s.forest * 0.8 + s.cold * 0.5
        }
        Domain::War => 1.0 + s.martial * 2.5 + s.nomadic * 0.8 + s.urban * 0.4,
        Domain::Sovereignty => {
            1.0 + s.agrarian * 1.5 + s.urban * 1.5 + s.complexity * 1.5 + s.riverine * 2.0
        }
        Domain::Fertility => 1.0 + s.agrarian * 2.2 + s.riverine * 6.0 + s.tropical * 0.8,
        Domain::Underworld => 1.0 + s.cold * 0.8 + s.arid * 0.8 + s.small_scale * 0.5,
        Domain::Death => 1.0 + s.martial * 0.8 + s.cold * 0.8 + s.small_scale * 0.5,
        Domain::Hunt => 1.0 + s.nomadic * 2.0 + s.forest * 1.2 + s.cold * 1.2,
        Domain::Craft => 1.0 + s.urban * 1.8 + s.highland * 0.8,
        Domain::Trade => 1.0 + s.coastal * 1.5 + s.urban * 1.8 + s.island * 1.0,
        Domain::Law => 1.0 + s.agrarian * 1.2 + s.urban * 1.8 + s.complexity * 1.5,
        Domain::Wisdom => 1.0 + s.scholarly * 2.0 + s.urban * 0.8,
        Domain::Healing => 1.0 + s.agrarian * 0.5 + s.urban * 0.5 + s.forest * 0.5,
        Domain::Hearth => 1.0 + s.cold * 1.5 + s.agrarian * 0.8 + s.small_scale * 0.8,
        Domain::CosmicOrder => 1.0 + s.complexity * 2.0 + s.scholarly * 1.0 + s.agrarian * 0.8,
        Domain::Transcendence => 1.0 + s.scholarly * 1.5 + s.highland * 1.0,
        Domain::Fate => 1.0 + s.arid * 0.8 + s.scholarly * 0.8 + s.small_scale * 0.5,
        Domain::Chaos => 0.8 + s.martial * 1.0 + s.tropical * 0.8,
        Domain::Harmony => 1.0 + s.agrarian * 1.0 + s.forest * 0.8 + s.scholarly * 0.5,
        Domain::Cycle => 1.0 + s.agrarian * 1.5 + s.tropical * 1.0 + s.forest * 0.8,
        Domain::Entropy => 0.8 + s.cold * 0.8 + s.arid * 0.8 + s.scholarly * 0.4,
        Domain::Truth => 1.0 + s.scholarly * 2.0 + s.urban * 0.8,
        _ => 1.0,
    }
}

/// The root's own taste in domains, applied on top of the country.
fn root_domain_weight(root: MetaphysicalRoot, domain: Domain, base: f32) -> f32 {
    match root {
        MetaphysicalRoot::Monotheism => match domain {
            Domain::Law
            | Domain::Truth
            | Domain::CosmicOrder
            | Domain::Sovereignty
            | Domain::Light => base * 2.5,
            Domain::Water => base * 2.0,
            Domain::Fertility | Domain::Hunt | Domain::Storm | Domain::Volcano | Domain::Chaos => {
                base * 0.3
            }
            _ => base,
        },
        MetaphysicalRoot::Dualism => match domain {
            Domain::Light | Domain::Shadow | Domain::Fire | Domain::Truth | Domain::Chaos => {
                base * 3.0
            }
            _ => base * 0.6,
        },
        MetaphysicalRoot::Henotheism => match domain {
            Domain::Sky | Domain::Storm | Domain::Sovereignty | Domain::War | Domain::Ancestor => {
                base * 2.0
            }
            _ => base,
        },
        MetaphysicalRoot::Polytheism => base * 1.1,
        MetaphysicalRoot::Animism => match domain {
            Domain::Flora
            | Domain::Fauna
            | Domain::Earth
            | Domain::Ancestor
            | Domain::Hunt
            | Domain::Sky
            | Domain::Water
            | Domain::Cycle => base * 2.2,
            Domain::Law | Domain::Sovereignty | Domain::Trade | Domain::Craft | Domain::Truth => {
                base * 0.3
            }
            _ => base,
        },
        MetaphysicalRoot::Pantheism => match domain {
            Domain::CosmicOrder
            | Domain::Cycle
            | Domain::Harmony
            | Domain::Flora
            | Domain::Transcendence
            | Domain::Entropy => base * 2.5,
            Domain::War | Domain::Hunt | Domain::Sovereignty => base * 0.3,
            _ => base,
        },
        MetaphysicalRoot::Panentheism => match domain {
            Domain::Transcendence
            | Domain::Truth
            | Domain::CosmicOrder
            | Domain::Cycle
            | Domain::Light => base * 2.2,
            _ => base,
        },
        MetaphysicalRoot::Deism | MetaphysicalRoot::Nontheism => {
            // A philosophical creed may hold domains, but not a pantheon of
            // anthropomorphic nature and war gods. The preferred set is the whole
            // Metaphysical family plus the three civic domains that can be read
            // as principles rather than persons.
            let preferred = domain.category() == DomainCategory::Metaphysical
                || matches!(domain, Domain::Wisdom | Domain::Ancestor | Domain::Law);
            let moderate = matches!(
                domain,
                Domain::Astral
                    | Domain::Light
                    | Domain::Shadow
                    | Domain::Eclipse
                    | Domain::Healing
                    | Domain::Hearth
                    | Domain::Craft
                    | Domain::Trade
            );
            if preferred {
                base * 5.0
            } else if moderate {
                base * 0.5
            } else {
                0.0
            }
        }
    }
}

/// Draw the domain architecture. The cardinality decides the shape; the country
/// and root decide which powers fill it.
fn draw_domains(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    cardinality: Cardinality,
    s: &Signals,
) -> Vec<Domain> {
    if matches!(cardinality, Cardinality::Abstract) {
        return Vec::new();
    }
    let weight = |domain: Domain| root_domain_weight(root, domain, domain_base(domain, s));
    let primaries = cardinality.primary_count();
    let mut domains = Vec::new();

    if primaries > 0 {
        let first = choose(rng, Domain::ALL, weight, Domain::Sky);
        domains.push(first);
        if primaries == 2 {
            // A dyad wants tension: the mythic opposite of the first power is
            // strongly preferred, and the rest of the field fills the gap if the
            // faith has no opposite for it.
            let wanted = opposite(first);
            let second = choose(
                rng,
                Domain::ALL,
                |domain| {
                    if domain == first {
                        0.0
                    } else if Some(domain) == wanted {
                        weight(domain) * 3.0
                    } else {
                        weight(domain)
                    }
                },
                Domain::Shadow,
            );
            domains.push(second);
        }
    }

    let secondary_count = match cardinality {
        Cardinality::Singular => 0,
        Cardinality::Dyadic => rng.random_range(0..=MAX_DYADIC_SECONDARIES),
        Cardinality::Hegemonic => rng.random_range(1..=cardinality.secondary_capacity()),
        Cardinality::Polycentric => {
            rng.random_range(MIN_POLYCENTRIC_DOMAINS..=MAX_POLYCENTRIC_DOMAINS)
        }
        Cardinality::Abstract => 0,
    };
    if matches!(cardinality, Cardinality::Polycentric) {
        // A polycentric faith has no primaries at all: every power is an equal.
        domains.clear();
    }
    let extra = sample_unique(
        rng,
        Domain::ALL,
        |domain| {
            if domains.contains(&domain) {
                0.0
            } else {
                weight(domain)
            }
        },
        secondary_count,
    );
    domains.extend(extra);
    domains.truncate(match cardinality {
        Cardinality::Polycentric => MAX_POLYCENTRIC_DOMAINS,
        _ => 1 + MAX_HEGEMONIC_SECONDARIES,
    });
    domains
}

// ─── LEVEL 3: COSMOLOGY ─────────────────────────────────────────────────────

fn domain_any(domains: &[Domain], wanted: &[Domain]) -> bool {
    wanted.iter().any(|domain| has_domain(domains, *domain))
}

/// Base weights in [`Cosmogony::ALL`] order.
fn cosmogony_base(root: MetaphysicalRoot) -> [f32; 8] {
    match root {
        MetaphysicalRoot::Monotheism => [4.0, 2.0, 0.3, 0.3, 0.3, 1.5, 0.5, 1.0],
        MetaphysicalRoot::Dualism => [0.4, 1.5, 0.4, 0.4, 0.3, 1.0, 4.0, 1.5],
        MetaphysicalRoot::Henotheism => [0.3, 3.0, 2.0, 1.0, 1.0, 1.0, 1.5, 2.0],
        MetaphysicalRoot::Polytheism => [0.2, 2.0, 2.0, 1.5, 3.0, 1.5, 2.0, 1.0],
        MetaphysicalRoot::Animism => [0.1, 1.0, 3.0, 3.0, 1.0, 1.0, 1.0, 3.0],
        MetaphysicalRoot::Pantheism => [0.2, 1.5, 1.0, 1.0, 1.0, 4.0, 0.8, 3.0],
        MetaphysicalRoot::Panentheism => [0.5, 2.0, 1.0, 0.8, 0.8, 4.0, 1.0, 2.0],
        MetaphysicalRoot::Deism => [1.0, 4.0, 0.2, 0.2, 0.2, 1.0, 0.5, 2.0],
        MetaphysicalRoot::Nontheism => [0.3, 1.0, 0.5, 0.5, 0.5, 1.0, 0.3, 5.0],
    }
}

fn draw_cosmogony(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    s: &Signals,
) -> Cosmogony {
    let base = cosmogony_base(root);
    choose(
        rng,
        &Cosmogony::ALL,
        |cosmogony| {
            let index = Cosmogony::ALL
                .iter()
                .position(|candidate| *candidate == cosmogony)
                .unwrap_or(0);
            let domain_bias = match cosmogony {
                Cosmogony::EarthDiver
                    if domain_any(domains, &[Domain::Water, Domain::Ocean, Domain::Earth]) =>
                {
                    1.6
                }
                Cosmogony::WorldParent
                    if domain_any(domains, &[Domain::Fertility, Domain::Flora, Domain::Earth]) =>
                {
                    1.5
                }
                Cosmogony::CosmicEgg
                    if domain_any(
                        domains,
                        &[Domain::CosmicOrder, Domain::Fertility, Domain::Cycle],
                    ) =>
                {
                    1.4
                }
                Cosmogony::DivineConflict
                    if domain_any(
                        domains,
                        &[Domain::War, Domain::Chaos, Domain::Shadow, Domain::Light],
                    ) =>
                {
                    1.7
                }
                Cosmogony::Emanation
                    if domain_any(
                        domains,
                        &[Domain::Transcendence, Domain::Truth, Domain::Light],
                    ) =>
                {
                    1.5
                }
                Cosmogony::Eternal if domain_any(domains, &[Domain::Cycle, Domain::Entropy]) => 1.3,
                Cosmogony::CosmicOrdering
                    if domain_any(domains, &[Domain::CosmicOrder, Domain::Law]) =>
                {
                    1.4
                }
                Cosmogony::ExNihilo if domain_any(domains, &[Domain::Law, Domain::Truth]) => 1.4,
                _ => 1.0,
            };
            let country_bias = if s.arid > 0.5 && cosmogony == Cosmogony::DivineConflict {
                1.2
            } else {
                1.0
            };
            base[index] * domain_bias * country_bias
        },
        Cosmogony::CosmicOrdering,
    )
}

/// Base weights in [`Temporality::ALL`] order.
fn temporality_base(root: MetaphysicalRoot) -> [f32; 6] {
    match root {
        MetaphysicalRoot::Monotheism => [4.0, 1.0, 0.5, 1.0, 2.0, 0.5],
        MetaphysicalRoot::Dualism => [4.0, 1.0, 1.0, 1.5, 1.0, 0.5],
        MetaphysicalRoot::Henotheism => [1.0, 3.0, 2.0, 1.0, 0.5, 1.0],
        MetaphysicalRoot::Polytheism => [1.0, 3.0, 2.0, 2.0, 0.5, 0.8],
        MetaphysicalRoot::Animism => [0.5, 4.0, 1.0, 1.0, 0.3, 2.0],
        MetaphysicalRoot::Pantheism => [0.5, 3.0, 1.0, 1.0, 1.0, 2.0],
        MetaphysicalRoot::Panentheism => [1.0, 2.0, 3.0, 0.5, 2.0, 1.0],
        MetaphysicalRoot::Deism => [3.0, 0.8, 1.0, 0.6, 2.0, 1.0],
        MetaphysicalRoot::Nontheism => [0.5, 3.0, 1.0, 0.8, 1.0, 2.0],
    }
}

fn draw_temporality(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
) -> Temporality {
    let base = temporality_base(root);
    choose(
        rng,
        &Temporality::ALL,
        |temporality| {
            let index = temporality_index(temporality);
            let domain_bias = match temporality {
                Temporality::Cyclical if has_domain(domains, Domain::Cycle) => 1.5,
                Temporality::Spiral if has_domain(domains, Domain::Cycle) => 1.5,
                Temporality::Degenerative if has_domain(domains, Domain::Entropy) => 1.5,
                Temporality::Progressive
                    if domain_any(domains, &[Domain::Transcendence, Domain::Truth]) =>
                {
                    1.2
                }
                Temporality::Linear
                    if domain_any(domains, &[Domain::Fate, Domain::CosmicOrder]) =>
                {
                    1.2
                }
                _ => 1.0,
            };
            base[index] * domain_bias
        },
        Temporality::Cyclical,
    )
}

fn temporality_index(temporality: Temporality) -> usize {
    Temporality::ALL
        .iter()
        .position(|candidate| *candidate == temporality)
        .unwrap_or(0)
}

/// Base weights in [`Eschaton::ALL`] order.
fn eschaton_base(root: MetaphysicalRoot) -> [f32; 6] {
    match root {
        MetaphysicalRoot::Monotheism => [4.0, 2.0, 0.8, 0.5, 0.5, 1.0],
        MetaphysicalRoot::Dualism => [3.0, 3.0, 0.8, 0.5, 1.0, 0.7],
        MetaphysicalRoot::Henotheism => [1.0, 2.0, 3.0, 1.0, 0.8, 0.5],
        MetaphysicalRoot::Polytheism => [0.8, 2.0, 3.0, 1.0, 0.8, 0.5],
        MetaphysicalRoot::Animism => [0.2, 1.5, 4.0, 2.0, 1.0, 0.3],
        MetaphysicalRoot::Pantheism => [0.2, 1.5, 2.0, 1.5, 3.0, 1.0],
        MetaphysicalRoot::Panentheism => [0.5, 3.0, 1.0, 1.0, 1.5, 2.0],
        MetaphysicalRoot::Deism => [0.5, 1.0, 3.0, 0.5, 2.0, 0.5],
        MetaphysicalRoot::Nontheism => [0.2, 1.0, 3.0, 0.8, 3.0, 0.5],
    }
}

fn draw_eschaton(rng: &mut ChaCha8Rng, root: MetaphysicalRoot, domains: &[Domain]) -> Eschaton {
    let base = eschaton_base(root);
    choose(
        rng,
        &Eschaton::ALL,
        |eschaton| {
            let index = Eschaton::ALL
                .iter()
                .position(|candidate| *candidate == eschaton)
                .unwrap_or(0);
            let bias = match eschaton {
                Eschaton::CosmicRenewal if has_domain(domains, Domain::CosmicOrder) => 1.4,
                Eschaton::GreatReturn if has_domain(domains, Domain::Cycle) => 1.4,
                Eschaton::Dissolution if domain_any(domains, &[Domain::Entropy, Domain::Decay]) => {
                    1.5
                }
                Eschaton::Apotheosis if has_domain(domains, Domain::Transcendence) => 1.5,
                Eschaton::FinalJudgment if domain_any(domains, &[Domain::Law, Domain::Truth]) => {
                    1.3
                }
                _ => 1.0,
            };
            base[index] * bias
        },
        Eschaton::EndlessContinuity,
    )
}

/// Base weights in [`Afterlife::ALL`] order.
fn afterlife_base(root: MetaphysicalRoot) -> [f32; 8] {
    match root {
        MetaphysicalRoot::Monotheism => [4.0, 0.5, 0.5, 0.5, 0.3, 0.5, 1.0, 0.5],
        MetaphysicalRoot::Dualism => [4.0, 0.5, 1.0, 0.5, 0.5, 0.5, 1.0, 0.5],
        MetaphysicalRoot::Henotheism => [0.5, 1.0, 2.0, 3.0, 2.0, 0.5, 0.5, 0.5],
        MetaphysicalRoot::Polytheism => [1.0, 1.0, 3.0, 1.5, 2.0, 0.5, 0.5, 0.5],
        MetaphysicalRoot::Animism => [0.2, 2.0, 1.0, 4.0, 0.5, 1.0, 0.3, 0.5],
        MetaphysicalRoot::Pantheism => [0.3, 2.0, 0.5, 1.0, 0.3, 3.0, 1.0, 1.0],
        MetaphysicalRoot::Panentheism => [0.5, 2.0, 0.5, 0.8, 0.3, 2.0, 2.0, 0.5],
        MetaphysicalRoot::Deism => [0.5, 0.5, 0.5, 0.5, 0.3, 0.5, 1.0, 3.0],
        MetaphysicalRoot::Nontheism => [0.3, 3.0, 0.5, 0.8, 0.3, 2.0, 0.8, 2.0],
    }
}

fn draw_afterlife(rng: &mut ChaCha8Rng, root: MetaphysicalRoot, domains: &[Domain]) -> Afterlife {
    let base = afterlife_base(root);
    choose(
        rng,
        &Afterlife::ALL,
        |afterlife| {
            let index = Afterlife::ALL
                .iter()
                .position(|candidate| *candidate == afterlife)
                .unwrap_or(0);
            let bias = match afterlife {
                Afterlife::AncestralPlane if has_domain(domains, Domain::Ancestor) => 1.5,
                Afterlife::HeroicHall if has_domain(domains, Domain::War) => 1.5,
                Afterlife::ShadowUnderworld
                    if domain_any(
                        domains,
                        &[Domain::Underworld, Domain::Death, Domain::Shadow],
                    ) =>
                {
                    1.5
                }
                Afterlife::AstralAscent
                    if domain_any(domains, &[Domain::Astral, Domain::Truth]) =>
                {
                    1.4
                }
                Afterlife::Reincarnation if has_domain(domains, Domain::Cycle) => 1.4,
                Afterlife::Oblivion if has_domain(domains, Domain::Entropy) => 1.4,
                Afterlife::Absorption if has_domain(domains, Domain::Transcendence) => 1.3,
                _ => 1.0,
            };
            base[index] * bias
        },
        Afterlife::ShadowUnderworld,
    )
}

// ─── LEVEL 4: ETHOS ────────────────────────────────────────────────────────

fn virtue_weight(virtue: Virtue, root: MetaphysicalRoot, domains: &[Domain], s: &Signals) -> f32 {
    let base = match root {
        MetaphysicalRoot::Monotheism => match virtue {
            Virtue::Obedience | Virtue::Purity => 2.5,
            Virtue::Compassion => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Dualism => match virtue {
            Virtue::Purity | Virtue::Wisdom => 2.0,
            Virtue::Valor => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Henotheism => match virtue {
            Virtue::Honor | Virtue::Hospitality => 2.0,
            Virtue::Valor => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Polytheism => match virtue {
            Virtue::Honor | Virtue::Hospitality => 2.0,
            Virtue::Valor | Virtue::Fecundity => 1.3,
            _ => 1.0,
        },
        MetaphysicalRoot::Animism => match virtue {
            Virtue::Stewardship | Virtue::Harmony => 2.5,
            Virtue::Hospitality => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Pantheism => match virtue {
            Virtue::Detachment | Virtue::Harmony => 2.5,
            Virtue::Compassion => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Panentheism => match virtue {
            Virtue::Compassion | Virtue::Wisdom => 2.0,
            Virtue::Detachment => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Deism => match virtue {
            Virtue::Wisdom | Virtue::Detachment => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Nontheism => match virtue {
            Virtue::Compassion | Virtue::Detachment => 2.5,
            Virtue::Wisdom => 1.5,
            _ => 1.0,
        },
    };
    let domain_bias = match virtue {
        Virtue::Obedience
            if domain_any(domains, &[Domain::Law, Domain::Fate, Domain::Sovereignty]) =>
        {
            1.6
        }
        Virtue::Valor if domain_any(domains, &[Domain::War, Domain::Hunt, Domain::Storm]) => 1.8,
        Virtue::Harmony
            if domain_any(
                domains,
                &[Domain::CosmicOrder, Domain::Harmony, Domain::Cycle],
            ) =>
        {
            1.5
        }
        Virtue::Compassion
            if domain_any(
                domains,
                &[Domain::Healing, Domain::Fertility, Domain::Hearth],
            ) =>
        {
            1.5
        }
        Virtue::Asceticism
            if domain_any(
                domains,
                &[Domain::Transcendence, Domain::Truth, Domain::Frost],
            ) =>
        {
            1.5
        }
        Virtue::Purity
            if domain_any(
                domains,
                &[Domain::Water, Domain::Light, Domain::Fire, Domain::Law],
            ) =>
        {
            1.5
        }
        Virtue::Stewardship
            if domain_any(
                domains,
                &[
                    Domain::Earth,
                    Domain::Flora,
                    Domain::Fauna,
                    Domain::Fertility,
                ],
            ) =>
        {
            1.6
        }
        Virtue::Wisdom
            if domain_any(
                domains,
                &[Domain::Wisdom, Domain::Truth, Domain::Craft, Domain::Astral],
            ) =>
        {
            1.6
        }
        Virtue::Hospitality
            if domain_any(domains, &[Domain::Hearth, Domain::Trade, Domain::Ancestor]) =>
        {
            1.5
        }
        Virtue::Fecundity
            if domain_any(
                domains,
                &[
                    Domain::Fertility,
                    Domain::Flora,
                    Domain::Fauna,
                    Domain::Water,
                ],
            ) =>
        {
            1.6
        }
        Virtue::Honor
            if domain_any(
                domains,
                &[Domain::War, Domain::Sovereignty, Domain::Ancestor],
            ) =>
        {
            1.7
        }
        Virtue::Detachment if domain_any(domains, &[Domain::Transcendence, Domain::Cycle]) => 1.4,
        _ => 1.0,
    };
    let signal_bias = match virtue {
        Virtue::Honor | Virtue::Valor => 1.0 + s.martial * 2.5,
        Virtue::Fecundity | Virtue::Stewardship => 1.0 + s.agrarian * 0.8,
        Virtue::Wisdom | Virtue::Detachment => 1.0 + s.scholarly * 1.2,
        Virtue::Hospitality => 1.0 + s.coastal * 0.5,
        _ => 1.0,
    };
    base * domain_bias * signal_bias
}

fn draw_virtues(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    s: &Signals,
) -> Vec<Virtue> {
    let count = if rng.random::<f32>() < 0.55 { 2 } else { 1 };
    sample_unique(
        rng,
        &Virtue::ALL,
        |virtue| virtue_weight(virtue, root, domains, s),
        count,
    )
}

fn taboo_weight(taboo: Taboo, root: MetaphysicalRoot, domains: &[Domain]) -> f32 {
    let base = match root {
        MetaphysicalRoot::Monotheism => match taboo {
            Taboo::Idolatry => 4.0,
            Taboo::Apostasy => 3.0,
            Taboo::OathBreaking => 2.0,
            Taboo::Desecration => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Dualism => match taboo {
            Taboo::Pollution => 3.0,
            Taboo::Apostasy => 2.0,
            Taboo::Idolatry => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Henotheism => match taboo {
            Taboo::KinSlaying => 2.0,
            Taboo::Hubris => 2.0,
            Taboo::Cowardice => 2.0,
            Taboo::Desecration => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Polytheism => match taboo {
            Taboo::Hubris => 3.0,
            Taboo::Desecration => 2.5,
            Taboo::Pollution | Taboo::OathBreaking => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Animism => match taboo {
            Taboo::KinSlaying => 2.0,
            Taboo::Pollution => 1.5,
            Taboo::Cowardice => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Pantheism => match taboo {
            Taboo::Hoarding => 2.0,
            Taboo::Hubris => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Panentheism => match taboo {
            Taboo::Hoarding => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Deism => match taboo {
            Taboo::Stagnation => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Nontheism => match taboo {
            Taboo::Hoarding => 2.5,
            Taboo::Stagnation => 1.5,
            _ => 1.0,
        },
    };
    let domain_bias = match taboo {
        Taboo::OathBreaking if domain_any(domains, &[Domain::Law, Domain::Trade]) => 1.6,
        Taboo::Apostasy if domain_any(domains, &[Domain::Truth, Domain::Law]) => 1.5,
        Taboo::Cowardice if domain_any(domains, &[Domain::War, Domain::Hunt]) => 1.6,
        Taboo::Pollution
            if domain_any(domains, &[Domain::Water, Domain::Fire, Domain::Underworld]) =>
        {
            1.5
        }
        Taboo::Desecration if domain_any(domains, &[Domain::Sovereignty, Domain::Ancestor]) => 1.5,
        Taboo::Hoarding if domain_any(domains, &[Domain::Trade, Domain::Hearth]) => 1.4,
        Taboo::Stagnation if domain_any(domains, &[Domain::Craft, Domain::Cycle]) => 1.5,
        Taboo::Hubris if has_domain(domains, Domain::Chaos) => 1.4,
        Taboo::KinSlaying if has_domain(domains, Domain::Ancestor) => 1.5,
        _ => 1.0,
    };
    base * domain_bias
}

fn draw_taboo(rng: &mut ChaCha8Rng, root: MetaphysicalRoot, domains: &[Domain]) -> Taboo {
    choose(
        rng,
        &Taboo::ALL,
        |taboo| taboo_weight(taboo, root, domains),
        Taboo::Hubris,
    )
}

fn soteriology_weight(
    soteriology: Soteriology,
    root: MetaphysicalRoot,
    domains: &[Domain],
    s: &Signals,
) -> f32 {
    let base = match root {
        MetaphysicalRoot::Monotheism => match soteriology {
            Soteriology::Grace => 4.0,
            Soteriology::Submission => 3.0,
            Soteriology::Works => 1.5,
            Soteriology::RitualExactness => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Dualism => match soteriology {
            Soteriology::Works => 3.0,
            Soteriology::Submission => 2.0,
            Soteriology::RitualExactness => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Henotheism => match soteriology {
            Soteriology::Works => 2.0,
            Soteriology::RitualExactness => 2.0,
            Soteriology::Glory => 2.0,
            Soteriology::Lineage => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Polytheism => match soteriology {
            Soteriology::Works => 2.0,
            Soteriology::RitualExactness => 3.0,
            Soteriology::Glory => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Animism => match soteriology {
            Soteriology::Lineage => 4.0,
            Soteriology::Works => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Pantheism => match soteriology {
            Soteriology::Gnosis => 3.0,
            Soteriology::Liberation => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Panentheism => match soteriology {
            Soteriology::Gnosis => 3.0,
            Soteriology::Grace => 2.0,
            _ => 1.0,
        },
        MetaphysicalRoot::Deism => match soteriology {
            Soteriology::Gnosis => 2.0,
            Soteriology::Works => 1.5,
            _ => 1.0,
        },
        MetaphysicalRoot::Nontheism => match soteriology {
            Soteriology::Liberation => 4.0,
            Soteriology::Gnosis => 2.0,
            _ => 1.0,
        },
    };
    let domain_bias = match soteriology {
        Soteriology::Works if domain_any(domains, &[Domain::Fertility, Domain::Law]) => 1.4,
        Soteriology::Gnosis if domain_any(domains, &[Domain::Wisdom, Domain::Truth]) => 1.5,
        Soteriology::Liberation if has_domain(domains, Domain::Transcendence) => 1.5,
        Soteriology::RitualExactness
            if domain_any(domains, &[Domain::Law, Domain::CosmicOrder]) =>
        {
            1.4
        }
        Soteriology::Glory if has_domain(domains, Domain::War) => 1.4,
        Soteriology::Lineage if has_domain(domains, Domain::Ancestor) => 1.6,
        Soteriology::Submission if has_domain(domains, Domain::Sovereignty) => 1.4,
        Soteriology::Grace if has_domain(domains, Domain::Light) => 1.3,
        _ => 1.0,
    };
    let signal_bias = match soteriology {
        Soteriology::Gnosis => 1.0 + s.scholarly * 0.8,
        Soteriology::Works => 1.0 + s.agrarian * 0.5,
        Soteriology::Lineage => 1.0 + s.small_scale * 0.8,
        _ => 1.0,
    };
    base * domain_bias * signal_bias
}

fn draw_soteriology(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    s: &Signals,
) -> Soteriology {
    choose(
        rng,
        &Soteriology::ALL,
        |soteriology| soteriology_weight(soteriology, root, domains, s),
        Soteriology::Works,
    )
}

fn stance_weight(
    stance: OutgroupStance,
    root: MetaphysicalRoot,
    cardinality: Cardinality,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    if stance == OutgroupStance::Missionary && cardinality == Cardinality::Polycentric {
        return 0.0;
    }
    if stance == OutgroupStance::Syncretic
        && root == MetaphysicalRoot::Monotheism
        && cardinality == Cardinality::Singular
    {
        return 0.0;
    }
    let base = match root {
        MetaphysicalRoot::Monotheism => match stance {
            OutgroupStance::Missionary => 3.0,
            OutgroupStance::Exclusive => 3.0,
            OutgroupStance::Pluralistic => 0.8,
            OutgroupStance::Syncretic => 0.5,
            OutgroupStance::Secretive => 0.6,
        },
        MetaphysicalRoot::Dualism => match stance {
            OutgroupStance::Exclusive => 2.5,
            OutgroupStance::Missionary => 1.0,
            OutgroupStance::Pluralistic => 0.8,
            OutgroupStance::Syncretic => 0.8,
            OutgroupStance::Secretive => 0.8,
        },
        MetaphysicalRoot::Henotheism => match stance {
            OutgroupStance::Syncretic => 2.5,
            OutgroupStance::Pluralistic => 2.0,
            OutgroupStance::Exclusive => 1.0,
            OutgroupStance::Missionary => 0.5,
            OutgroupStance::Secretive => 0.8,
        },
        MetaphysicalRoot::Polytheism => match stance {
            OutgroupStance::Syncretic => 3.0,
            OutgroupStance::Pluralistic => 3.0,
            OutgroupStance::Exclusive => 0.8,
            OutgroupStance::Missionary => 0.5,
            OutgroupStance::Secretive => 0.8,
        },
        MetaphysicalRoot::Animism => match stance {
            OutgroupStance::Pluralistic => 4.0,
            OutgroupStance::Secretive => 1.2,
            OutgroupStance::Syncretic => 1.5,
            OutgroupStance::Exclusive => 0.5,
            OutgroupStance::Missionary => 0.2,
        },
        MetaphysicalRoot::Pantheism => match stance {
            OutgroupStance::Pluralistic => 2.0,
            OutgroupStance::Secretive => 1.0,
            OutgroupStance::Syncretic => 2.0,
            OutgroupStance::Exclusive => 0.5,
            OutgroupStance::Missionary => 0.3,
        },
        MetaphysicalRoot::Panentheism => match stance {
            OutgroupStance::Pluralistic => 2.0,
            OutgroupStance::Syncretic => 2.0,
            OutgroupStance::Secretive => 1.0,
            OutgroupStance::Missionary => 0.5,
            OutgroupStance::Exclusive => 0.5,
        },
        MetaphysicalRoot::Deism => match stance {
            OutgroupStance::Pluralistic => 2.0,
            OutgroupStance::Exclusive => 1.0,
            OutgroupStance::Secretive => 1.0,
            OutgroupStance::Syncretic => 1.0,
            OutgroupStance::Missionary => 0.2,
        },
        MetaphysicalRoot::Nontheism => match stance {
            OutgroupStance::Missionary => 1.5,
            OutgroupStance::Pluralistic => 2.0,
            OutgroupStance::Secretive => 1.0,
            OutgroupStance::Syncretic => 1.0,
            OutgroupStance::Exclusive => 0.8,
        },
    };
    let outlook_bias = match ctx.outlook {
        Outlook::Isolationist => match stance {
            OutgroupStance::Exclusive | OutgroupStance::Secretive => 1.6,
            _ => 0.9,
        },
        Outlook::Cosmopolitan | Outlook::Mercantile => match stance {
            OutgroupStance::Syncretic | OutgroupStance::Pluralistic => 1.5,
            _ => 0.9,
        },
        Outlook::Martial => match stance {
            OutgroupStance::Exclusive => 1.5,
            _ => 1.0,
        },
        Outlook::Scholarly => match stance {
            OutgroupStance::Pluralistic | OutgroupStance::Syncretic => 1.3,
            _ => 1.0,
        },
        Outlook::Artistic | Outlook::Ceremonial => match stance {
            OutgroupStance::Pluralistic => 1.2,
            _ => 1.0,
        },
    };
    let domain_bias = match stance {
        OutgroupStance::Secretive
            if domain_any(domains, &[Domain::Shadow, Domain::Underworld, Domain::Fate]) =>
        {
            2.0
        }
        OutgroupStance::Exclusive
            if domain_any(domains, &[Domain::Law, Domain::Sovereignty, Domain::Truth]) =>
        {
            1.4
        }
        OutgroupStance::Syncretic if has_domain(domains, Domain::Trade) => 1.4,
        OutgroupStance::Pluralistic if has_domain(domains, Domain::Hearth) => 1.3,
        OutgroupStance::Missionary if has_domain(domains, Domain::Truth) => 1.4,
        _ => 1.0,
    };
    let scale_bias = if stance == OutgroupStance::Pluralistic {
        1.0 + s.small_scale * 0.6
    } else {
        1.0
    };
    base * outlook_bias * domain_bias * scale_bias
}

fn draw_stance(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    cardinality: Cardinality,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> OutgroupStance {
    choose(
        rng,
        &OutgroupStance::ALL,
        |stance| stance_weight(stance, root, cardinality, domains, ctx, s),
        OutgroupStance::Pluralistic,
    )
}

// ─── LEVEL 5: PRAXIS ───────────────────────────────────────────────────────

fn rite_weight(
    rite: Rite,
    root: MetaphysicalRoot,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    if root.is_philosophical() && matches!(rite, Rite::AnimalSacrifice | Rite::BloodOffering) {
        return 0.0;
    }
    let base = match rite {
        Rite::AnimalSacrifice => match root {
            MetaphysicalRoot::Polytheism | MetaphysicalRoot::Henotheism => 3.0,
            MetaphysicalRoot::Animism => 2.0,
            _ => 1.0,
        },
        Rite::BloodOffering => match root {
            MetaphysicalRoot::Dualism | MetaphysicalRoot::Polytheism => 2.0,
            _ => 1.0,
        },
        Rite::BurntOffering => match root {
            MetaphysicalRoot::Monotheism => 2.0,
            _ => 1.0,
        },
        Rite::Libation => 1.2,
        Rite::Prayer => match root {
            MetaphysicalRoot::Monotheism => 3.0,
            MetaphysicalRoot::Dualism => 2.0,
            MetaphysicalRoot::Deism => 1.5,
            _ => 1.0,
        },
        Rite::Meditation => match root {
            MetaphysicalRoot::Nontheism => 3.0,
            MetaphysicalRoot::Pantheism | MetaphysicalRoot::Panentheism => 2.0,
            _ => 0.8,
        },
        Rite::EcstaticTrance => match root {
            MetaphysicalRoot::Animism => 3.0,
            MetaphysicalRoot::Henotheism => 2.0,
            _ => 1.0,
        },
        Rite::Procession => 1.1,
        Rite::Fasting => match root {
            MetaphysicalRoot::Monotheism | MetaphysicalRoot::Dualism => 2.0,
            _ => 1.0,
        },
        Rite::Pilgrimage => match root {
            MetaphysicalRoot::Monotheism | MetaphysicalRoot::Polytheism => 2.0,
            _ => 1.0,
        },
        Rite::SacredDebate => match root {
            MetaphysicalRoot::Nontheism => 2.5,
            MetaphysicalRoot::Deism => 2.0,
            _ => 0.8,
        },
        Rite::Monumentalism => 1.0,
    };
    let domain_bias = match rite {
        Rite::AnimalSacrifice | Rite::BloodOffering
            if domain_any(
                domains,
                &[Domain::Fertility, Domain::War, Domain::Underworld],
            ) =>
        {
            1.6
        }
        Rite::BurntOffering if domain_any(domains, &[Domain::Solar, Domain::Fire]) => 1.7,
        Rite::Libation
            if domain_any(
                domains,
                &[
                    Domain::Water,
                    Domain::Fertility,
                    Domain::Ancestor,
                    Domain::Ocean,
                ],
            ) =>
        {
            1.6
        }
        Rite::Meditation if domain_any(domains, &[Domain::Transcendence, Domain::Truth]) => 1.6,
        Rite::EcstaticTrance
            if domain_any(domains, &[Domain::Storm, Domain::Shadow, Domain::Fauna]) =>
        {
            1.5
        }
        Rite::Procession if domain_any(domains, &[Domain::Solar, Domain::Sovereignty]) => 1.5,
        Rite::Fasting if domain_any(domains, &[Domain::Frost, Domain::Entropy]) => 1.4,
        Rite::Pilgrimage if domain_any(domains, &[Domain::Sky, Domain::Solar, Domain::Fate]) => 1.5,
        Rite::SacredDebate if domain_any(domains, &[Domain::Wisdom, Domain::Truth]) => 1.6,
        Rite::Monumentalism
            if domain_any(
                domains,
                &[Domain::Sovereignty, Domain::Solar, Domain::Earth],
            ) =>
        {
            1.7
        }
        _ => 1.0,
    };
    let country_bias = match rite {
        Rite::Pilgrimage => 1.0 + s.highland * 0.6,
        Rite::Fasting => 1.0 + s.arid * 0.5,
        Rite::Procession | Rite::Monumentalism => 1.0 + s.urban * 0.6,
        Rite::EcstaticTrance => 1.0 + s.small_scale * 0.5,
        Rite::SacredDebate => 1.0 + s.scholarly * 0.8,
        _ => 1.0,
    };
    let gate = if rite == Rite::Monumentalism && ctx.complexity < 0.20 {
        0.2
    } else {
        1.0
    };
    base * domain_bias * country_bias * gate
}

fn draw_rites(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> Vec<Rite> {
    let count = if rng.random::<f32>() < 0.55 { 2 } else { 1 };
    sample_unique(
        rng,
        &Rite::ALL,
        |rite| rite_weight(rite, root, domains, ctx, s),
        count,
    )
}

fn transmission_weight(
    transmission: Transmission,
    root: MetaphysicalRoot,
    domains: &[Domain],
    stance: OutgroupStance,
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    let base = match transmission {
        Transmission::WrittenCanon => match root {
            MetaphysicalRoot::Monotheism => 4.0,
            MetaphysicalRoot::Deism | MetaphysicalRoot::Nontheism => 2.0,
            MetaphysicalRoot::Dualism => 1.5,
            _ => 1.0,
        },
        Transmission::OralEpic => match root {
            MetaphysicalRoot::Polytheism | MetaphysicalRoot::Henotheism => 2.0,
            _ => 1.5,
        },
        Transmission::Augury => match root {
            MetaphysicalRoot::Polytheism | MetaphysicalRoot::Henotheism => 2.0,
            _ => 1.0,
        },
        Transmission::Astrology => match root {
            MetaphysicalRoot::Polytheism => 1.5,
            MetaphysicalRoot::Deism | MetaphysicalRoot::Nontheism => 1.5,
            _ => 1.0,
        },
        Transmission::LivingOracle => match root {
            MetaphysicalRoot::Henotheism => 3.0,
            MetaphysicalRoot::Polytheism => 2.0,
            MetaphysicalRoot::Animism => 1.5,
            _ => 1.0,
        },
        Transmission::Iconography => match root {
            MetaphysicalRoot::Polytheism => 2.5,
            MetaphysicalRoot::Monotheism => 0.5,
            _ => 1.0,
        },
        Transmission::MysteryInitiation => 1.0,
    };
    if root.is_philosophical() && transmission == Transmission::LivingOracle {
        return 0.0;
    }
    if root == MetaphysicalRoot::Animism && transmission == Transmission::WrittenCanon {
        return 0.0;
    }
    let domain_bias = match transmission {
        Transmission::WrittenCanon
            if domain_any(domains, &[Domain::Law, Domain::Truth, Domain::CosmicOrder]) =>
        {
            1.6
        }
        Transmission::OralEpic if has_domain(domains, Domain::War) => 1.5,
        Transmission::Augury if domain_any(domains, &[Domain::Storm, Domain::Sky]) => 1.8,
        Transmission::Astrology
            if domain_any(domains, &[Domain::Astral, Domain::Fate, Domain::Eclipse]) =>
        {
            2.0
        }
        Transmission::LivingOracle if domain_any(domains, &[Domain::Sky, Domain::Fate]) => 1.5,
        Transmission::Iconography
            if domain_any(domains, &[Domain::Craft, Domain::Solar, Domain::Ancestor]) =>
        {
            1.5
        }
        Transmission::MysteryInitiation
            if domain_any(
                domains,
                &[Domain::Shadow, Domain::Underworld, Domain::Death],
            ) =>
        {
            1.6
        }
        _ => 1.0,
    };
    let stance_bias =
        if stance == OutgroupStance::Secretive && transmission == Transmission::MysteryInitiation {
            2.5
        } else {
            1.0
        };
    let signal_bias = match transmission {
        Transmission::OralEpic => 1.0 + s.nomadic * 0.8,
        Transmission::Astrology => 1.0 + s.scholarly * 0.8,
        Transmission::Augury => 1.0 + s.highland * 0.6,
        Transmission::Iconography => 1.0 + s.urban * 0.6,
        _ => 1.0,
    };
    let gate = match transmission {
        Transmission::WrittenCanon if ctx.complexity < 0.45 || !ctx.tier.is_state() => 0.0,
        Transmission::Astrology if ctx.complexity < 0.20 => 0.0,
        _ => 1.0,
    };
    base * domain_bias * stance_bias * signal_bias * gate
}

fn draw_transmissions(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    stance: OutgroupStance,
    ctx: &FaithContext,
    s: &Signals,
) -> Vec<Transmission> {
    let count = if rng.random::<f32>() < 0.45 { 2 } else { 1 };
    sample_unique(
        rng,
        &Transmission::ALL,
        |transmission| transmission_weight(transmission, root, domains, stance, ctx, s),
        count,
    )
}

fn calendar_weight(
    calendar: CalendarAnchor,
    root: MetaphysicalRoot,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    let base = match calendar {
        CalendarAnchor::Solstice => 1.0,
        CalendarAnchor::Equinox => 1.0,
        CalendarAnchor::LunarPhase => 1.0,
        CalendarAnchor::FloodCycle => 1.0,
        CalendarAnchor::Monsoon => 1.0,
        CalendarAnchor::Harvest => 1.0,
        CalendarAnchor::Eclipse => 0.6,
        CalendarAnchor::FoundingEra => match root {
            MetaphysicalRoot::Monotheism
            | MetaphysicalRoot::Deism
            | MetaphysicalRoot::Nontheism => 1.5,
            _ => 0.8,
        },
    };
    let domain_bias = match calendar {
        CalendarAnchor::Solstice if domain_any(domains, &[Domain::Solar, Domain::Light]) => 2.0,
        CalendarAnchor::Equinox
            if domain_any(domains, &[Domain::CosmicOrder, Domain::Fertility]) =>
        {
            1.5
        }
        CalendarAnchor::LunarPhase if has_domain(domains, Domain::Lunar) => 3.0,
        CalendarAnchor::FloodCycle if has_domain(domains, Domain::Water) => 1.8,
        CalendarAnchor::Monsoon
            if domain_any(domains, &[Domain::Storm, Domain::Water, Domain::Fertility]) =>
        {
            1.6
        }
        CalendarAnchor::Harvest if domain_any(domains, &[Domain::Fertility, Domain::Hearth]) => 1.8,
        CalendarAnchor::Eclipse if has_domain(domains, Domain::Eclipse) => 4.0,
        CalendarAnchor::FoundingEra if domain_any(domains, &[Domain::Law, Domain::Sovereignty]) => {
            1.5
        }
        _ => 1.0,
    };
    let country_bias = match calendar {
        CalendarAnchor::Solstice => {
            1.0 + s.cold * 1.2
                + if ctx.latitude_deg.abs() > 45.0 {
                    0.8
                } else {
                    0.0
                }
        }
        CalendarAnchor::Equinox => 1.0 + s.agrarian * 0.5,
        CalendarAnchor::LunarPhase => 1.0 + s.arid * 0.5 + s.small_scale * 0.5,
        CalendarAnchor::FloodCycle => 1.0 + s.riverine * 2.0,
        CalendarAnchor::Monsoon => 1.0 + s.tropical * 2.0,
        CalendarAnchor::Harvest => 1.0 + s.agrarian * 1.5,
        CalendarAnchor::Eclipse => 1.0,
        CalendarAnchor::FoundingEra => 1.0 + s.complexity,
    };
    base * domain_bias * country_bias
}

fn draw_calendar(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> CalendarAnchor {
    choose(
        rng,
        &CalendarAnchor::ALL,
        |calendar| calendar_weight(calendar, root, domains, ctx, s),
        CalendarAnchor::Harvest,
    )
}

// ─── LEVEL 6: ORGANIZATION AND POLITY ──────────────────────────────────────

fn clergy_weight(
    clergy: Clergy,
    root: MetaphysicalRoot,
    domains: &[Domain],
    cardinality: Cardinality,
    stance: OutgroupStance,
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    let base = match clergy {
        Clergy::Ecclesiarchy => match root {
            MetaphysicalRoot::Polytheism => 3.0,
            MetaphysicalRoot::Monotheism => 3.0,
            MetaphysicalRoot::Dualism => 1.5,
            MetaphysicalRoot::Henotheism => 1.5,
            MetaphysicalRoot::Deism | MetaphysicalRoot::Nontheism => 0.3,
            _ => 1.0,
        },
        Clergy::SacredKingship => match root {
            MetaphysicalRoot::Monotheism | MetaphysicalRoot::Polytheism => 2.0,
            MetaphysicalRoot::Henotheism => 1.5,
            _ => 1.0,
        },
        Clergy::CivicMagistracy => match root {
            MetaphysicalRoot::Polytheism => 2.0,
            MetaphysicalRoot::Monotheism => 1.0,
            _ => 1.0,
        },
        Clergy::Monasticism => match root {
            MetaphysicalRoot::Nontheism => 3.0,
            MetaphysicalRoot::Pantheism | MetaphysicalRoot::Panentheism => 2.5,
            MetaphysicalRoot::Deism => 2.0,
            _ => 0.8,
        },
        Clergy::Shamanism => match root {
            MetaphysicalRoot::Animism => 4.0,
            MetaphysicalRoot::Henotheism => 2.0,
            _ => 0.8,
        },
        Clergy::MysteryCult => 1.0,
        Clergy::ClanElders => match root {
            MetaphysicalRoot::Animism => 3.0,
            MetaphysicalRoot::Henotheism => 1.5,
            _ => 0.8,
        },
        Clergy::PropheticGuild => match root {
            MetaphysicalRoot::Monotheism | MetaphysicalRoot::Dualism => 2.0,
            MetaphysicalRoot::Henotheism => 1.5,
            _ => 1.0,
        },
        Clergy::None => match root {
            MetaphysicalRoot::Nontheism => 2.0,
            MetaphysicalRoot::Deism => 1.5,
            _ => 0.5,
        },
    };
    let domain_bias = match clergy {
        Clergy::Ecclesiarchy
            if domain_any(
                domains,
                &[Domain::Law, Domain::CosmicOrder, Domain::Sovereignty],
            ) =>
        {
            1.6
        }
        Clergy::SacredKingship
            if domain_any(domains, &[Domain::Sovereignty, Domain::Solar, Domain::War]) =>
        {
            1.8
        }
        Clergy::CivicMagistracy
            if domain_any(domains, &[Domain::Law, Domain::Trade, Domain::Wisdom]) =>
        {
            1.5
        }
        Clergy::Monasticism
            if domain_any(
                domains,
                &[Domain::Transcendence, Domain::Truth, Domain::Frost],
            ) =>
        {
            1.6
        }
        Clergy::Shamanism
            if domain_any(
                domains,
                &[Domain::Flora, Domain::Fauna, Domain::Storm, Domain::Shadow],
            ) =>
        {
            1.7
        }
        Clergy::MysteryCult
            if domain_any(
                domains,
                &[Domain::Shadow, Domain::Underworld, Domain::Death],
            ) =>
        {
            1.8
        }
        Clergy::ClanElders if has_domain(domains, Domain::Ancestor) => 1.8,
        Clergy::PropheticGuild
            if domain_any(domains, &[Domain::Fate, Domain::Truth, Domain::Sky]) =>
        {
            1.6
        }
        _ => 1.0,
    };
    let stance_bias = match (stance, clergy) {
        (OutgroupStance::Secretive, Clergy::MysteryCult) => 3.0,
        (OutgroupStance::Secretive, _) => 0.9,
        (OutgroupStance::Exclusive, Clergy::Ecclesiarchy | Clergy::PropheticGuild) => 1.4,
        (OutgroupStance::Pluralistic, Clergy::ClanElders | Clergy::Shamanism) => 1.3,
        _ => 1.0,
    };
    let signal_bias = match clergy {
        Clergy::Shamanism | Clergy::ClanElders => 1.0 + s.small_scale * 1.2,
        Clergy::Ecclesiarchy | Clergy::CivicMagistracy => 1.0 + s.urban * 0.8,
        Clergy::Monasticism => 1.0 + s.scholarly * 0.8,
        _ => 1.0,
    };
    let institution_bias = match clergy {
        Clergy::SacredKingship if ctx.government == Government::DivineMonarchy => 2.5,
        Clergy::Ecclesiarchy if ctx.government == Government::TheocraticRepublic => 2.2,
        Clergy::CivicMagistracy
            if matches!(
                ctx.government,
                Government::AristocraticRepublic | Government::PlutocraticRepublic
            ) =>
        {
            1.8
        }
        Clergy::Ecclesiarchy if ctx.hierarchy == Hierarchy::SacredKingship => 1.4,
        Clergy::CivicMagistracy if ctx.hierarchy == Hierarchy::Bureaucratic => 1.4,
        _ => 1.0,
    };
    let gate = match clergy {
        Clergy::Ecclesiarchy | Clergy::SacredKingship | Clergy::CivicMagistracy
            if !ctx.tier.is_state() =>
        {
            0.0
        }
        Clergy::Monasticism | Clergy::PropheticGuild if ctx.complexity < 0.30 => 0.0,
        Clergy::MysteryCult if ctx.complexity < 0.20 => 0.0,
        Clergy::Ecclesiarchy if root == MetaphysicalRoot::Animism => 0.0,
        Clergy::SacredKingship if root.is_philosophical() => 0.0,
        _ => 1.0,
    };
    // A polycentric pantheon has no single hierarchy to head it.
    let cardinality_bias = if cardinality == Cardinality::Polycentric
        && matches!(clergy, Clergy::Ecclesiarchy | Clergy::SacredKingship)
    {
        0.4
    } else {
        1.0
    };
    base * domain_bias * stance_bias * signal_bias * institution_bias * gate * cardinality_bias
}

fn draw_clergy(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    cardinality: Cardinality,
    stance: OutgroupStance,
    ctx: &FaithContext,
    s: &Signals,
) -> Clergy {
    choose(
        rng,
        &Clergy::ALL,
        |clergy| clergy_weight(clergy, root, domains, cardinality, stance, ctx, s),
        Clergy::ClanElders,
    )
}

fn coupling_weight(
    coupling: StateCoupling,
    root: MetaphysicalRoot,
    cardinality: Cardinality,
    clergy: Clergy,
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    let base = match coupling {
        StateCoupling::Theocracy => match clergy {
            Clergy::Ecclesiarchy | Clergy::SacredKingship => 4.0,
            _ => 0.8,
        },
        StateCoupling::StatePatronage => match clergy {
            Clergy::CivicMagistracy | Clergy::ClanElders | Clergy::PropheticGuild => 3.0,
            _ => 1.0,
        },
        StateCoupling::DualPower => match clergy {
            Clergy::Ecclesiarchy | Clergy::SacredKingship => 3.0,
            _ => 0.6,
        },
        StateCoupling::SubordinateCult => 1.0,
        StateCoupling::Autonomous => match clergy {
            Clergy::Monasticism | Clergy::None => 2.5,
            _ => 1.0,
        },
        StateCoupling::FolkDecentralized => match clergy {
            Clergy::Shamanism | Clergy::ClanElders => 4.0,
            _ => 1.0,
        },
    };
    let state_gate = match coupling {
        StateCoupling::Theocracy
        | StateCoupling::StatePatronage
        | StateCoupling::DualPower
        | StateCoupling::SubordinateCult
            if !ctx.tier.is_state() =>
        {
            0.0
        }
        StateCoupling::FolkDecentralized if ctx.tier.is_state() => 0.0,
        _ => 1.0,
    };
    let clergy_gate = match coupling {
        StateCoupling::Theocracy | StateCoupling::DualPower if clergy == Clergy::None => 0.0,
        _ => 1.0,
    };
    let root_gate = if root == MetaphysicalRoot::Animism
        && matches!(
            coupling,
            StateCoupling::Theocracy | StateCoupling::DualPower
        ) {
        0.0
    } else {
        1.0
    };
    let cardinality_gate =
        if cardinality == Cardinality::Polycentric && coupling == StateCoupling::Theocracy {
            0.0
        } else {
            1.0
        };
    let scale_bias = match coupling {
        StateCoupling::FolkDecentralized => 1.0 + s.small_scale * 1.5,
        StateCoupling::Theocracy | StateCoupling::StatePatronage => {
            1.0 + s.urban * 0.6 + s.complexity * 0.5
        }
        StateCoupling::Autonomous => 1.0 + s.scholarly * 0.5,
        _ => 1.0,
    };
    let government_bias = match coupling {
        StateCoupling::Theocracy if ctx.government == Government::TheocraticRepublic => 2.5,
        StateCoupling::StatePatronage
            if matches!(
                ctx.government,
                Government::AristocraticRepublic | Government::PlutocraticRepublic
            ) =>
        {
            1.8
        }
        StateCoupling::DualPower if ctx.government == Government::DivineMonarchy => 1.8,
        StateCoupling::SubordinateCult if ctx.government == Government::Tyranny => 1.3,
        StateCoupling::Autonomous
            if matches!(ctx.government, Government::Band | Government::Tribe) =>
        {
            1.3
        }
        StateCoupling::FolkDecentralized
            if matches!(
                ctx.government,
                Government::Band | Government::Tribe | Government::Chiefdom
            ) =>
        {
            1.5
        }
        _ => 1.0,
    };
    base * state_gate * clergy_gate * root_gate * cardinality_gate * scale_bias * government_bias
}

fn draw_coupling(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    cardinality: Cardinality,
    clergy: Clergy,
    ctx: &FaithContext,
    s: &Signals,
) -> StateCoupling {
    choose(
        rng,
        &StateCoupling::ALL,
        |coupling| coupling_weight(coupling, root, cardinality, clergy, ctx, s),
        StateCoupling::Autonomous,
    )
}

// ─── LEVEL 7: MANIFESTATION ────────────────────────────────────────────────

fn locus_weight(
    locus: SacredLocus,
    root: MetaphysicalRoot,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    let base = match locus {
        SacredLocus::Peak => 0.5 + s.highland * 3.0,
        SacredLocus::RiverConfluence => 0.4 + s.riverine * 3.0,
        SacredLocus::Spring => 0.4 + s.arid * 2.0,
        SacredLocus::Caldera => 0.3 + s.highland * 1.5,
        SacredLocus::Oasis => 0.3 + s.arid * 3.0,
        SacredLocus::Strait => 0.3 + s.coastal * 2.0,
        SacredLocus::Crater => 0.3 + s.highland * 1.0,
        SacredLocus::AbyssalCoast => 0.2 + s.coastal * 1.5,
        SacredLocus::Monolith => 0.5 + s.highland * 1.5,
        SacredLocus::Grove => 0.4 + s.forest * 3.0,
        SacredLocus::Necropolis => 0.5,
        SacredLocus::Observatory => 0.3 + s.highland * 1.5 + s.arid * 1.0,
    };
    let domain_bias = match locus {
        SacredLocus::Peak if domain_any(domains, &[Domain::Sky, Domain::Storm]) => 1.5,
        SacredLocus::RiverConfluence if has_domain(domains, Domain::Water) => 1.5,
        SacredLocus::Spring if domain_any(domains, &[Domain::Water, Domain::Healing]) => 1.4,
        SacredLocus::Caldera if domain_any(domains, &[Domain::Fire, Domain::Volcano]) => 2.0,
        SacredLocus::Oasis if domain_any(domains, &[Domain::Water, Domain::Fertility]) => 1.5,
        SacredLocus::Strait if domain_any(domains, &[Domain::Ocean, Domain::Trade]) => 1.5,
        SacredLocus::Crater if domain_any(domains, &[Domain::Astral, Domain::Eclipse]) => 1.6,
        SacredLocus::AbyssalCoast
            if domain_any(
                domains,
                &[Domain::Shadow, Domain::Underworld, Domain::Death],
            ) =>
        {
            1.6
        }
        SacredLocus::Monolith if domain_any(domains, &[Domain::Sovereignty, Domain::Solar]) => 1.5,
        SacredLocus::Grove if domain_any(domains, &[Domain::Flora, Domain::Fauna]) => 1.6,
        SacredLocus::Necropolis
            if domain_any(
                domains,
                &[Domain::Ancestor, Domain::Death, Domain::Underworld],
            ) =>
        {
            2.2
        }
        SacredLocus::Observatory
            if domain_any(domains, &[Domain::Astral, Domain::Fate, Domain::Truth]) =>
        {
            2.0
        }
        _ => 1.0,
    };
    let root_bias = match root {
        MetaphysicalRoot::Animism => match locus {
            SacredLocus::Grove | SacredLocus::Spring | SacredLocus::Peak => 1.4,
            _ => 1.0,
        },
        MetaphysicalRoot::Nontheism | MetaphysicalRoot::Deism => match locus {
            SacredLocus::Observatory | SacredLocus::Grove | SacredLocus::Monolith => 1.3,
            _ => 1.0,
        },
        _ => 1.0,
    };
    let latitude = ctx.latitude_deg.abs();
    let latitude_bias = match locus {
        SacredLocus::Observatory if latitude > 30.0 => 1.2,
        SacredLocus::Spring | SacredLocus::Oasis if latitude < 35.0 => 1.2,
        _ => 1.0,
    };
    base * domain_bias * root_bias * latitude_bias
}

fn draw_locus(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    domains: &[Domain],
    ctx: &FaithContext,
    s: &Signals,
) -> SacredLocus {
    choose(
        rng,
        &SacredLocus::ALL,
        |locus| locus_weight(locus, root, domains, ctx, s),
        SacredLocus::Peak,
    )
}

fn orientation_weight(
    orientation: OrientationAxis,
    root: MetaphysicalRoot,
    cardinality: Cardinality,
    domains: &[Domain],
    locus: SacredLocus,
    ctx: &FaithContext,
    s: &Signals,
) -> f32 {
    let base = match orientation {
        OrientationAxis::Sunrise => 1.0,
        OrientationAxis::Sunset => 1.0,
        OrientationAxis::Zenith => 1.0,
        OrientationAxis::North => 1.0,
        OrientationAxis::South => 1.0,
        OrientationAxis::Upstream => 1.0,
        OrientationAxis::Downstream => 1.0,
        OrientationAxis::Peakward => 1.0,
        OrientationAxis::Seaward => 1.0,
        OrientationAxis::Omnidirectional => 0.6,
    };
    let domain_bias = match orientation {
        OrientationAxis::Sunrise if domain_any(domains, &[Domain::Solar, Domain::Light]) => 2.0,
        OrientationAxis::Sunset
            if domain_any(
                domains,
                &[Domain::Shadow, Domain::Death, Domain::Underworld],
            ) =>
        {
            1.8
        }
        OrientationAxis::Zenith if domain_any(domains, &[Domain::Sky, Domain::Storm]) => 2.0,
        OrientationAxis::North if domain_any(domains, &[Domain::Frost, Domain::Wind]) => 1.8,
        OrientationAxis::South if domain_any(domains, &[Domain::Fertility, Domain::Solar]) => 1.4,
        OrientationAxis::Upstream if has_domain(domains, Domain::Water) => 1.6,
        OrientationAxis::Downstream if domain_any(domains, &[Domain::Water, Domain::Ocean]) => 1.6,
        OrientationAxis::Peakward if has_domain(domains, Domain::Earth) => 1.5,
        OrientationAxis::Seaward if domain_any(domains, &[Domain::Ocean, Domain::Trade]) => 1.6,
        OrientationAxis::Omnidirectional
            if root.is_philosophical()
                || domain_any(domains, &[Domain::CosmicOrder, Domain::Truth]) =>
        {
            2.5
        }
        _ => 1.0,
    };
    let locus_bias = match orientation {
        OrientationAxis::Peakward
            if matches!(
                locus,
                SacredLocus::Peak | SacredLocus::Caldera | SacredLocus::Crater
            ) =>
        {
            2.0
        }
        OrientationAxis::Seaward
            if matches!(locus, SacredLocus::Strait | SacredLocus::AbyssalCoast) =>
        {
            2.0
        }
        OrientationAxis::Upstream if locus == SacredLocus::RiverConfluence => 1.5,
        OrientationAxis::Downstream if locus == SacredLocus::RiverConfluence => 1.5,
        _ => 1.0,
    };
    let country_bias = match orientation {
        OrientationAxis::North => {
            1.0 + s.cold * 1.2 + if ctx.latitude_deg > 30.0 { 0.6 } else { 0.0 }
        }
        OrientationAxis::South => {
            1.0 + s.tropical * 0.8 + if ctx.latitude_deg < -25.0 { 0.8 } else { 0.0 }
        }
        OrientationAxis::Zenith => 1.0 + s.highland * 0.6,
        _ => 1.0,
    };
    let cardinality_bias = if cardinality == Cardinality::Abstract
        && orientation == OrientationAxis::Omnidirectional
    {
        2.0
    } else {
        1.0
    };
    base * domain_bias * locus_bias * country_bias * cardinality_bias
}

fn draw_orientation(
    rng: &mut ChaCha8Rng,
    root: MetaphysicalRoot,
    cardinality: Cardinality,
    domains: &[Domain],
    locus: SacredLocus,
    ctx: &FaithContext,
    s: &Signals,
) -> OrientationAxis {
    choose(
        rng,
        &OrientationAxis::ALL,
        |orientation| orientation_weight(orientation, root, cardinality, domains, locus, ctx, s),
        OrientationAxis::Sunrise,
    )
}

fn emblem_weight(emblem: Emblem, root: MetaphysicalRoot, domains: &[Domain]) -> f32 {
    let base = match emblem {
        Emblem::Disk => 1.0,
        Emblem::Crescent => 1.0,
        Emblem::Star => 1.0,
        Emblem::Spiral => 1.0,
        Emblem::Wave => 1.0,
        Emblem::Flame => 1.0,
        Emblem::Mountain => 1.0,
        Emblem::Tree => 1.0,
        Emblem::Serpent => 1.0,
        Emblem::Sword => 1.0,
        Emblem::Eye => 1.0,
        Emblem::Wheel => 1.0,
        Emblem::Void => 1.0,
    };
    let domain_bias = match emblem {
        Emblem::Disk if has_domain(domains, Domain::Solar) => 3.0,
        Emblem::Crescent if has_domain(domains, Domain::Lunar) => 3.0,
        Emblem::Star if has_domain(domains, Domain::Astral) => 3.0,
        Emblem::Spiral if domain_any(domains, &[Domain::Cycle, Domain::Transcendence]) => 2.5,
        Emblem::Wave if domain_any(domains, &[Domain::Water, Domain::Ocean]) => 3.0,
        Emblem::Flame if domain_any(domains, &[Domain::Fire, Domain::Solar]) => 2.5,
        Emblem::Mountain if domain_any(domains, &[Domain::Earth, Domain::Sovereignty]) => 2.5,
        Emblem::Tree if domain_any(domains, &[Domain::Flora, Domain::Fertility]) => 3.0,
        Emblem::Serpent
            if domain_any(
                domains,
                &[Domain::Fauna, Domain::Underworld, Domain::Wisdom],
            ) =>
        {
            2.0
        }
        Emblem::Sword if domain_any(domains, &[Domain::War, Domain::Sovereignty]) => 3.0,
        Emblem::Eye if domain_any(domains, &[Domain::Truth, Domain::Wisdom, Domain::Astral]) => 2.5,
        Emblem::Wheel if domain_any(domains, &[Domain::Cycle, Domain::CosmicOrder]) => 2.5,
        Emblem::Void
            if root.is_philosophical()
                || domain_any(domains, &[Domain::Entropy, Domain::Transcendence]) =>
        {
            2.5
        }
        _ => 1.0,
    };
    base * domain_bias
}

fn draw_emblem(rng: &mut ChaCha8Rng, root: MetaphysicalRoot, domains: &[Domain]) -> Emblem {
    choose(
        rng,
        &Emblem::ALL,
        |emblem| emblem_weight(emblem, root, domains),
        Emblem::Eye,
    )
}

/// Which naming patterns a faith can use. `Root + Epithet` and `Domain + Order`
/// need a primary domain; `Dual Polarity` needs two co-primaries. A polycentric
/// or abstract faith therefore draws only the root, founder or locus patterns,
/// which keeps every name strictly faithful to what was rolled.
fn naming_pattern_weight(
    pattern: NamingPattern,
    primary: &[Domain],
    cardinality: Cardinality,
) -> f32 {
    match pattern {
        NamingPattern::RootEpithet if primary.is_empty() => 1.5,
        NamingPattern::RootEpithet => 3.0,
        NamingPattern::DomainOrder if !primary.is_empty() => 3.0,
        NamingPattern::FounderPath => 2.0,
        NamingPattern::LocusCovenant => 2.0,
        NamingPattern::DualPolarity if cardinality == Cardinality::Dyadic && primary.len() == 2 => {
            5.0
        }
        _ => 0.0,
    }
}

fn draw_naming(
    rng: &mut ChaCha8Rng,
    domains: &[Domain],
    cardinality: Cardinality,
) -> NamingPattern {
    let primary_count = cardinality.primary_count().min(domains.len());
    let primary = &domains[..primary_count];
    choose(
        rng,
        &NamingPattern::ALL,
        |pattern| naming_pattern_weight(pattern, primary, cardinality),
        NamingPattern::LocusCovenant,
    )
}

// ─── THE OBJECT OF REVERENCE ───────────────────────────────────────────────

/// Derive the kind of being or principle a faith reveres from the faith it has
/// already drawn. Deliberately not a new RNG draw: the object of reverence is
/// read off the domains, emblem, naming pattern and eschatology, so adding it
/// cannot move a cradle, a culture or a language.
fn derive_divinity(faith: &Faith) -> Divinity {
    match faith.root {
        MetaphysicalRoot::Animism => Divinity::Animistic(spirit_kind(faith)),
        MetaphysicalRoot::Nontheism => Divinity::Nontheist(nontheist_focus(faith)),
        _ => Divinity::Theistic(divine_gender(faith)),
    }
}

fn divine_gender(faith: &Faith) -> DivineGender {
    let domains = if faith.primary_domains().is_empty() {
        faith.domains.as_slice()
    } else {
        faith.primary_domains()
    };
    let mut feminine = 0.0f32;
    let mut masculine = 0.0f32;
    for domain in domains {
        match domain {
            Domain::Lunar
            | Domain::Water
            | Domain::Earth
            | Domain::Fertility
            | Domain::Flora
            | Domain::Hearth
            | Domain::Healing
            | Domain::Harmony
            | Domain::Shadow => feminine += 1.0,
            Domain::Solar
            | Domain::War
            | Domain::Storm
            | Domain::Fire
            | Domain::Volcano
            | Domain::Sovereignty
            | Domain::Hunt
            | Domain::Law => masculine += 1.0,
            _ => {}
        }
    }
    match faith.emblem {
        Emblem::Crescent | Emblem::Spiral | Emblem::Wave | Emblem::Tree => feminine += 1.5,
        Emblem::Disk | Emblem::Sword | Emblem::Flame | Emblem::Mountain | Emblem::Serpent => {
            masculine += 1.5
        }
        Emblem::Star | Emblem::Eye | Emblem::Wheel | Emblem::Void => {}
    }
    if feminine > masculine {
        DivineGender::Feminine
    } else {
        DivineGender::Masculine
    }
}

fn spirit_kind(faith: &Faith) -> SpiritKind {
    let mut spirits = 1.0f32;
    let mut totems = 0.0f32;
    let mut ancestors = 0.0f32;
    let mut souls = 0.0f32;
    for domain in &faith.domains {
        match domain {
            Domain::Ancestor => ancestors += 3.0,
            Domain::Fauna | Domain::Flora | Domain::Hunt => totems += 2.0,
            Domain::Death | Domain::Underworld | Domain::Decay => souls += 2.0,
            Domain::Sky | Domain::Water | Domain::Earth => spirits += 0.5,
            _ => {}
        }
    }
    match faith.afterlife {
        Afterlife::Reincarnation | Afterlife::Absorption => souls += 1.0,
        Afterlife::AncestralPlane => ancestors += 1.0,
        Afterlife::AstralAscent => spirits += 1.0,
        _ => {}
    }
    [
        (SpiritKind::Spirits, spirits),
        (SpiritKind::Totems, totems),
        (SpiritKind::Ancestors, ancestors),
        (SpiritKind::Souls, souls),
    ]
    .into_iter()
    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    .map_or(SpiritKind::Spirits, |(kind, _)| kind)
}

fn nontheist_focus(faith: &Faith) -> NontheistFocus {
    let person = faith.naming == NamingPattern::FounderPath
        || faith
            .domains
            .iter()
            .any(|domain| matches!(domain, Domain::Wisdom | Domain::Ancestor))
        || matches!(faith.eschaton, Eschaton::Apotheosis);
    if !person {
        let concept = if faith.domains.contains(&Domain::Harmony) {
            NontheistConcept::Harmony
        } else if faith.domains.contains(&Domain::Truth) {
            NontheistConcept::Truth
        } else if faith.domains.iter().any(|domain| {
            matches!(
                domain,
                Domain::CosmicOrder | Domain::Law | Domain::Fate | Domain::Transcendence
            )
        }) {
            NontheistConcept::Way
        } else {
            NontheistConcept::Path
        };
        return NontheistFocus::Concept(concept);
    }
    let figure = if faith.domains.contains(&Domain::Wisdom) {
        NontheistPerson::Sage
    } else if matches!(faith.eschaton, Eschaton::Apotheosis)
        || faith.domains.contains(&Domain::Transcendence)
    {
        NontheistPerson::Ascended
    } else if faith.domains.contains(&Domain::Truth) || faith.domains.contains(&Domain::Light) {
        NontheistPerson::Enlightened
    } else {
        NontheistPerson::Teacher
    };
    NontheistFocus::Person(figure)
}

/// Compose a faith's name from **only** its primary domains, its root and its
/// sacred locus. Secondary domains are never read here.
fn build_faith_name(rng: &mut ChaCha8Rng, faith: &Faith) -> String {
    let primary = faith.primary_domains();
    let pattern = faith.naming;
    match pattern {
        NamingPattern::DomainOrder => {
            let domain = primary.first().copied().unwrap_or(Domain::Sky);
            format!("Order of the {}", domain.noun())
        }
        NamingPattern::RootEpithet => {
            let epithet = *["Covenant", "Order", "Way", "Path", "Rite", "Creed"]
                .choose(rng)
                .unwrap();
            match primary.first() {
                Some(domain) => format!("{} {}", domain.noun(), epithet),
                None => format!("{} {}", faith.root.noun(), epithet),
            }
        }
        NamingPattern::FounderPath => match primary.first() {
            Some(domain) => format!("Way of the {}", domain.noun()),
            None => format!("Way of the {}", faith.root.noun()),
        },
        NamingPattern::LocusCovenant => {
            format!("Covenant of the {}", faith.locus.label())
        }
        NamingPattern::DualPolarity => {
            let first = primary.first().copied().unwrap_or(Domain::Light);
            let second = primary.get(1).copied().unwrap_or(Domain::Shadow);
            format!("{} and {}", first.noun(), second.noun())
        }
    }
}

// ─── THE CASCADE ────────────────────────────────────────────────────────────

/// Build one faith from the country it grew in.
pub fn generate_faith(rng: &mut ChaCha8Rng, ctx: &FaithContext) -> Faith {
    let signals = Signals::from(ctx);
    let root = draw_root(rng, &signals, ctx);
    let cardinality = draw_cardinality(rng, root);
    let domains = draw_domains(rng, root, cardinality, &signals);
    let cosmogony = draw_cosmogony(rng, root, &domains, &signals);
    let temporality = draw_temporality(rng, root, &domains);
    let eschaton = draw_eschaton(rng, root, &domains);
    let afterlife = draw_afterlife(rng, root, &domains);
    let virtues = draw_virtues(rng, root, &domains, &signals);
    let taboo = draw_taboo(rng, root, &domains);
    let soteriology = draw_soteriology(rng, root, &domains, &signals);
    let stance = draw_stance(rng, root, cardinality, &domains, ctx, &signals);
    let rites = draw_rites(rng, root, &domains, ctx, &signals);
    let transmissions = draw_transmissions(rng, root, &domains, stance, ctx, &signals);
    let calendar = draw_calendar(rng, root, &domains, ctx, &signals);
    let clergy = draw_clergy(rng, root, &domains, cardinality, stance, ctx, &signals);
    let state_coupling = draw_coupling(rng, root, cardinality, clergy, ctx, &signals);
    let locus = draw_locus(rng, root, &domains, ctx, &signals);
    let orientation = draw_orientation(rng, root, cardinality, &domains, locus, ctx, &signals);
    let emblem = draw_emblem(rng, root, &domains);
    let naming = draw_naming(rng, &domains, cardinality);

    let mut faith = Faith {
        name: String::new(),
        gloss: String::new(),
        recension: None,
        names: FaithNames::default(),
        liturgical_language: None,
        sacred_sites: Vec::new(),
        root,
        cardinality,
        domains,
        cosmogony,
        temporality,
        eschaton,
        afterlife,
        virtues,
        taboo,
        soteriology,
        stance,
        rites,
        transmissions,
        calendar,
        clergy,
        state_coupling,
        locus,
        orientation,
        emblem,
        naming,
        cults: Vec::new(),
    };
    faith.gloss = build_faith_name(rng, &faith);
    faith.name = faith.gloss.clone();
    faith
}

// ─── INHERITANCE ───────────────────────────────────────────────────────────

/// The strongest domain this country would give the root, without drawing.
fn top_local_domain(root: MetaphysicalRoot, s: &Signals) -> Domain {
    Domain::ALL
        .iter()
        .copied()
        .max_by(|a, b| {
            let wa = root_domain_weight(root, *a, domain_base(*a, s));
            let wb = root_domain_weight(root, *b, domain_base(*b, s));
            wa.partial_cmp(&wb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(Domain::Sky)
}

/// The least fitting domain in `start..`, by the root's own weights.
fn lowest_affinity_index(
    domains: &[Domain],
    start: usize,
    root: MetaphysicalRoot,
    s: &Signals,
) -> Option<usize> {
    (start..domains.len()).min_by(|&a, &b| {
        let wa = root_domain_weight(root, domains[a], domain_base(domains[a], s));
        let wb = root_domain_weight(root, domains[b], domain_base(domains[b], s));
        wa.partial_cmp(&wb).unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// A daughter realm keeps its ancestor's root and cosmology, shifts its
/// secondaries towards its own country, and only rarely breaks away wholesale.
///
/// A singular faith never loses its primary domain here: the worst that happens
/// is that it gains a secondary and becomes hegemonic, or that a schism rewrites
/// its ethos and polity.
pub fn inherit_faith(rng: &mut ChaCha8Rng, parent: &Faith, ctx: &FaithContext) -> Faith {
    let signals = Signals::from(ctx);
    let mut faith = parent.clone();
    // A daughter is named afresh for its own country; it cannot inherit the
    // parent's onomasticon or its holy places.
    faith.names = FaithNames::default();
    faith.liturgical_language = None;
    faith.sacred_sites.clear();
    let local = top_local_domain(faith.root, &signals);
    let mut schism = false;

    match faith.cardinality {
        Cardinality::Hegemonic => {
            if rng.random::<f32>() < 0.60 && !has_domain(&faith.domains, local) {
                if let Some(index) = lowest_affinity_index(&faith.domains, 1, faith.root, &signals)
                {
                    faith.domains[index] = local;
                }
            }
        }
        Cardinality::Dyadic => {
            if faith.secondary_domains().is_empty() {
                if rng.random::<f32>() < 0.20 && !has_domain(&faith.domains, local) {
                    faith.domains.push(local);
                }
            } else if rng.random::<f32>() < 0.60 && !has_domain(&faith.domains, local) {
                if let Some(index) = lowest_affinity_index(&faith.domains, 2, faith.root, &signals)
                {
                    faith.domains[index] = local;
                }
            }
        }
        Cardinality::Polycentric => {
            if rng.random::<f32>() < 0.50 && !has_domain(&faith.domains, local) {
                if let Some(index) = lowest_affinity_index(&faith.domains, 0, faith.root, &signals)
                {
                    faith.domains[index] = local;
                }
            }
        }
        Cardinality::Singular => {
            let roll = rng.random::<f32>();
            if roll < 0.20 && !has_domain(&faith.domains, local) {
                // A new power is admitted beneath the one: still faithful to the
                // ancestor's primary, but no longer singular.
                faith.domains.push(local);
                faith.cardinality = Cardinality::Hegemonic;
            } else if roll < 0.30 {
                schism = true;
            }
        }
        Cardinality::Abstract => {
            if rng.random::<f32>() < 0.20 {
                schism = true;
            }
        }
    }

    if !schism {
        schism = match faith.cardinality {
            Cardinality::Singular | Cardinality::Abstract => rng.random::<f32>() < 0.10,
            _ => rng.random::<f32>() < 0.20,
        };
    }

    if schism {
        faith.virtues = draw_virtues(rng, faith.root, &faith.domains, &signals);
        faith.taboo = draw_taboo(rng, faith.root, &faith.domains);
        faith.soteriology = draw_soteriology(rng, faith.root, &faith.domains, &signals);
        faith.stance = draw_stance(
            rng,
            faith.root,
            faith.cardinality,
            &faith.domains,
            ctx,
            &signals,
        );
        faith.clergy = draw_clergy(
            rng,
            faith.root,
            &faith.domains,
            faith.cardinality,
            faith.stance,
            ctx,
            &signals,
        );
        faith.state_coupling = draw_coupling(
            rng,
            faith.root,
            faith.cardinality,
            faith.clergy,
            ctx,
            &signals,
        );
    }

    // The calendar and the sacred place are local by nature: a daughter on a
    // different coast does not keep a flood festival it cannot hold.
    faith.calendar = draw_calendar(rng, faith.root, &faith.domains, ctx, &signals);
    faith.locus = draw_locus(rng, faith.root, &faith.domains, ctx, &signals);
    faith.orientation = draw_orientation(
        rng,
        faith.root,
        faith.cardinality,
        &faith.domains,
        faith.locus,
        ctx,
        &signals,
    );

    let recension = *FaithRecension::ALL.choose(rng).unwrap();
    faith.recension = Some(recension);
    faith.gloss = format!("{} {}", recension.label(), build_faith_name(rng, &faith));
    faith.name = faith.gloss.clone();
    faith
}

// ─── CONTACT ───────────────────────────────────────────────────────────────

/// One change a neighbour proposes. The pass collects these against a snapshot
/// of the faiths, then applies them once, so no conversion can travel two
/// frontiers in one pass.
#[derive(Clone, Debug)]
enum ContactChange {
    Convert { source: usize },
    Merge { source: usize, domain: Domain },
    PlantCult { source: usize, cult: Cult },
}

impl ContactChange {
    fn priority(&self) -> u8 {
        match self {
            ContactChange::Convert { .. } => 3,
            ContactChange::Merge { .. } => 2,
            ContactChange::PlantCult { .. } => 1,
        }
    }

    fn source(&self) -> usize {
        match self {
            ContactChange::Convert { source }
            | ContactChange::Merge { source, .. }
            | ContactChange::PlantCult { source, .. } => *source,
        }
    }
}

fn stronger(a: usize, b: usize, contexts: &[FaithContext]) -> bool {
    let ca = contexts[a].complexity;
    let cb = contexts[b].complexity;
    ca > cb + 0.001 || ((ca - cb).abs() <= 0.001 && a < b)
}

fn propose(slot: &mut Option<ContactChange>, change: ContactChange, contexts: &[FaithContext]) {
    let replace = match slot {
        None => true,
        Some(existing) => {
            change.priority() > existing.priority()
                || (change.priority() == existing.priority()
                    && stronger(change.source(), existing.source(), contexts))
        }
    };
    if replace {
        *slot = Some(change);
    }
}

/// Let neighbours influence one another: a missionary creed converts an
/// exclusive or polycentric neighbour, a syncretic one borrows its strongest
/// power, and a secretive one plants a minority cult.
///
/// `religions` is the world roster, one creed per realm in founding order;
/// `founding[realm]` is the religion that realm follows. A conversion makes the
/// target *join the source's religion* rather than clone its text, which is what
/// lets one faith exist in several countries. The function returns the faith each
/// converted realm displaced, so the province pass can keep it alive in the
/// countryside.
///
/// `adjacency` is the stage's own frontier map, keyed by 1-based realm id.
pub fn apply_contact(
    rng: &mut ChaCha8Rng,
    religions: &mut Vec<Faith>,
    founding: &mut [u16],
    contexts: &[FaithContext],
    adjacency: &BTreeMap<u16, Vec<u16>>,
) -> Vec<Option<u16>> {
    let realms = founding.len();
    let mut displaced = vec![None; realms];
    if realms < 2 || contexts.len() < realms || religions.is_empty() {
        return displaced;
    }
    let before = religions.clone();
    let before_founding = founding.to_vec();
    let mut proposals: Vec<Option<ContactChange>> = vec![None; realms];

    for (&a, neighbours) in adjacency {
        for &b in neighbours {
            if b <= a {
                continue;
            }
            let (Some(ai), Some(bi)) = (a.checked_sub(1), b.checked_sub(1)) else {
                continue;
            };
            let (ai, bi) = (ai as usize, bi as usize);
            if ai >= realms || bi >= realms {
                continue;
            }
            let (source, target) = if stronger(ai, bi, contexts) {
                (ai, bi)
            } else {
                (bi, ai)
            };
            // A society without a hearth has no creed to argue with yet; its
            // provinces will take a neighbour's faith in the province pass.
            if before_founding[source] == 0 || before_founding[target] == 0 {
                continue;
            }
            let Some(source_faith) = before.get(before_founding[source] as usize - 1) else {
                continue;
            };
            match source_faith.stance {
                OutgroupStance::Missionary => {
                    let Some(target_faith) = before.get(before_founding[target] as usize - 1)
                    else {
                        continue;
                    };
                    let convertible = target_faith.stance == OutgroupStance::Exclusive
                        || target_faith.cardinality == Cardinality::Polycentric;
                    if convertible
                        && contexts[source].complexity > contexts[target].complexity + 0.05
                    {
                        propose(
                            &mut proposals[target],
                            ContactChange::Convert { source },
                            contexts,
                        );
                    }
                }
                OutgroupStance::Syncretic => {
                    let domain = source_faith
                        .primary_domains()
                        .first()
                        .copied()
                        .unwrap_or_else(|| {
                            if source_faith.root.is_philosophical() {
                                Domain::Truth
                            } else {
                                Domain::Shadow
                            }
                        });
                    propose(
                        &mut proposals[target],
                        ContactChange::Merge { source, domain },
                        contexts,
                    );
                }
                OutgroupStance::Secretive => {
                    let domain = source_faith
                        .primary_domains()
                        .first()
                        .copied()
                        .unwrap_or_else(|| {
                            if source_faith.root.is_philosophical() {
                                Domain::Truth
                            } else {
                                Domain::Shadow
                            }
                        });
                    let gloss = format!("Mystery of the {}", domain.noun());
                    let cult = Cult {
                        name: gloss.clone(),
                        gloss,
                        power_name: String::new(),
                        root: source_faith.root,
                        domain,
                    };
                    propose(
                        &mut proposals[target],
                        ContactChange::PlantCult { source, cult },
                        contexts,
                    );
                }
                OutgroupStance::Exclusive | OutgroupStance::Pluralistic => {}
            }
        }
    }

    for (target, change) in proposals.into_iter().enumerate() {
        let Some(change) = change else {
            continue;
        };
        let target_religion = before_founding[target];
        match change {
            ContactChange::Convert { source } => {
                let source_religion = before_founding[source];
                if source_religion == 0 || source_religion == target_religion {
                    continue;
                }
                // The realm now keeps the source creed itself. Its own faith is
                // not destroyed: it becomes a minority the province pass can seed
                // back into the countryside.
                displaced[target] = Some(target_religion);
                founding[target] = source_religion;
            }
            ContactChange::Merge { domain, .. } => {
                if let Some(faith) = religions.get_mut(target_religion as usize - 1) {
                    // The recipient now shares the source's religion identity, so
                    // the borrowed power belongs to every realm that keeps it.
                    merge_domain(rng, faith, domain, contexts[target]);
                }
            }
            ContactChange::PlantCult { cult, .. } => {
                if let Some(faith) = religions.get_mut(target_religion as usize - 1)
                    && faith.cults.len() < 2
                    && !faith
                        .cults
                        .iter()
                        .any(|existing| existing.name == cult.name)
                {
                    faith.cults.push(cult);
                }
            }
        }
    }
    displaced
}

fn merge_domain(rng: &mut ChaCha8Rng, faith: &mut Faith, domain: Domain, ctx: FaithContext) {
    if has_domain(&faith.domains, domain) {
        return;
    }
    let signals = Signals::from(&ctx);
    match faith.cardinality {
        Cardinality::Abstract => {
            faith.stance = OutgroupStance::Syncretic;
        }
        Cardinality::Singular => {
            if rng.random::<f32>() < 0.30 {
                faith.domains.push(domain);
                faith.cardinality = Cardinality::Hegemonic;
                faith.stance = OutgroupStance::Syncretic;
            }
        }
        Cardinality::Dyadic => {
            if faith.secondary_domains().len() < MAX_DYADIC_SECONDARIES {
                faith.domains.push(domain);
            } else if let Some(index) =
                lowest_affinity_index(&faith.domains, 2, faith.root, &signals)
            {
                faith.domains[index] = domain;
            }
        }
        Cardinality::Hegemonic => {
            if faith.secondary_domains().len() < MAX_HEGEMONIC_SECONDARIES {
                faith.domains.push(domain);
            } else if let Some(index) =
                lowest_affinity_index(&faith.domains, 1, faith.root, &signals)
            {
                faith.domains[index] = domain;
            }
        }
        Cardinality::Polycentric => {
            if faith.domains.len() < MAX_POLYCENTRIC_DOMAINS {
                faith.domains.push(domain);
            } else if let Some(index) =
                lowest_affinity_index(&faith.domains, 0, faith.root, &signals)
            {
                faith.domains[index] = domain;
            }
        }
    }
    // A syncretic neighbour changes the recipient's stance when its own
    // constitution allows it. A monotheism that is still singular is never
    // labelled syncretic; only the hegemonic shift above can admit that.
    if !(faith.root == MetaphysicalRoot::Monotheism && faith.cardinality == Cardinality::Singular) {
        faith.stance = OutgroupStance::Syncretic;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn context(
        environment: Environment,
        cradle_kind: CradleKind,
        tier: SocietyTier,
        complexity: f32,
        subsistence: Subsistence,
        outlook: Outlook,
        government: Government,
        hierarchy: Hierarchy,
    ) -> FaithContext {
        FaithContext {
            environment,
            cradle_kind,
            tier,
            complexity,
            subsistence,
            outlook,
            government,
            hierarchy,
            mean_temp_c: 18.0,
            mean_precip: 0.45,
            mean_elevation_m: 120.0,
            sea_px: if matches!(
                environment,
                Environment::Coast | Environment::IslandShore | Environment::Delta
            ) {
                8
            } else {
                0
            },
            lake_px: 0,
            river: matches!(
                cradle_kind,
                CradleKind::RiverFloodplain | CradleKind::RiverDelta
            ),
            great_river: matches!(cradle_kind, CradleKind::RiverFloodplain),
            latitude_deg: 22.0,
        }
    }

    fn river_state() -> FaithContext {
        context(
            Environment::Floodplain,
            CradleKind::RiverFloodplain,
            SocietyTier::States,
            0.70,
            Subsistence::IrrigatedAgriculture,
            Outlook::Ceremonial,
            Government::TheocraticRepublic,
            Hierarchy::SacredKingship,
        )
    }

    fn desert_band() -> FaithContext {
        context(
            Environment::Desert,
            CradleKind::DesertOasis,
            SocietyTier::HunterGatherers,
            0.03,
            Subsistence::Foraging,
            Outlook::Isolationist,
            Government::Band,
            Hierarchy::Egalitarian,
        )
    }

    fn forest_tribe() -> FaithContext {
        context(
            Environment::TropicalForest,
            CradleKind::RainforestClearing,
            SocietyTier::SimpleFarmers,
            0.20,
            Subsistence::ShiftingCultivation,
            Outlook::Artistic,
            Government::Tribe,
            Hierarchy::Heterarchical,
        )
    }

    fn island_merchants() -> FaithContext {
        context(
            Environment::IslandShore,
            CradleKind::IslandShore,
            SocietyTier::States,
            0.55,
            Subsistence::FishingAndSeafaring,
            Outlook::Mercantile,
            Government::PlutocraticRepublic,
            Hierarchy::Oligarchic,
        )
    }

    fn scholarly_state() -> FaithContext {
        context(
            Environment::Woodland,
            CradleKind::WoodlandHeartland,
            SocietyTier::States,
            0.90,
            Subsistence::DrylandFarming,
            Outlook::Scholarly,
            Government::AristocraticRepublic,
            Hierarchy::Bureaucratic,
        )
    }

    fn steppe_herders() -> FaithContext {
        context(
            Environment::Steppe,
            CradleKind::SteppeGrassland,
            SocietyTier::NomadicPastoralists,
            0.15,
            Subsistence::Pastoralism,
            Outlook::Martial,
            Government::Chiefdom,
            Hierarchy::RankedKinship,
        )
    }

    #[test]
    fn the_same_seed_draws_the_same_faith() {
        for context in [river_state(), desert_band(), scholarly_state()] {
            let mut a = ChaCha8Rng::seed_from_u64(7);
            let mut b = ChaCha8Rng::seed_from_u64(7);
            assert_eq!(
                generate_faith(&mut a, &context),
                generate_faith(&mut b, &context)
            );
        }
    }

    #[test]
    fn every_cardinality_has_its_own_shape() {
        let contexts = [
            river_state(),
            desert_band(),
            forest_tribe(),
            island_merchants(),
            scholarly_state(),
            steppe_herders(),
        ];
        for context in contexts {
            for seed in 0..200u64 {
                let mut rng = ChaCha8Rng::seed_from_u64(seed);
                let faith = generate_faith(&mut rng, &context);
                match faith.cardinality {
                    Cardinality::Abstract => {
                        assert!(faith.domains.is_empty(), "abstract faith held domains");
                        assert!(faith.primary_domains().is_empty());
                        assert!(!matches!(
                            faith.naming,
                            NamingPattern::DomainOrder | NamingPattern::DualPolarity
                        ));
                    }
                    Cardinality::Singular => {
                        assert_eq!(faith.domains.len(), 1, "singular faith is not singular");
                        assert_eq!(faith.primary_domains().len(), 1);
                        assert!(faith.secondary_domains().is_empty());
                    }
                    Cardinality::Dyadic => {
                        assert_eq!(faith.primary_domains().len(), 2);
                        assert!(faith.domains.len() >= 2);
                        assert!(faith.domains.len() <= 2 + MAX_DYADIC_SECONDARIES);
                    }
                    Cardinality::Hegemonic => {
                        assert_eq!(faith.primary_domains().len(), 1);
                        assert!(!faith.secondary_domains().is_empty());
                        assert!(faith.secondary_domains().len() <= MAX_HEGEMONIC_SECONDARIES);
                    }
                    Cardinality::Polycentric => {
                        assert!(
                            faith.domains.len() >= MIN_POLYCENTRIC_DOMAINS
                                && faith.domains.len() <= MAX_POLYCENTRIC_DOMAINS
                        );
                        assert!(faith.primary_domains().is_empty());
                        assert!(faith.secondary_domains().is_empty());
                        assert!(!matches!(
                            faith.naming,
                            NamingPattern::DomainOrder | NamingPattern::DualPolarity
                        ));
                    }
                }
                assert!(
                    (1..=2).contains(&faith.virtues.len()),
                    "virtues must be one or two"
                );
                assert!(
                    (1..=2).contains(&faith.rites.len()),
                    "rites must be one or two"
                );
                assert!(
                    (1..=2).contains(&faith.transmissions.len()),
                    "transmissions must be one or two"
                );
            }
        }
    }

    #[test]
    fn hard_constraints_hold_across_the_whole_field() {
        let contexts = [
            river_state(),
            desert_band(),
            forest_tribe(),
            island_merchants(),
            scholarly_state(),
            steppe_herders(),
        ];
        for context in contexts {
            for seed in 0..250u64 {
                let mut rng = ChaCha8Rng::seed_from_u64(seed);
                let faith = generate_faith(&mut rng, &context);
                if faith.root == MetaphysicalRoot::Animism {
                    assert_ne!(faith.clergy, Clergy::Ecclesiarchy);
                    assert_ne!(faith.state_coupling, StateCoupling::Theocracy);
                    assert!(!faith.transmissions.contains(&Transmission::WrittenCanon));
                }
                if faith.root.is_philosophical() {
                    assert_ne!(faith.clergy, Clergy::SacredKingship);
                    assert!(!faith.rites.contains(&Rite::AnimalSacrifice));
                    assert!(!faith.rites.contains(&Rite::BloodOffering));
                    assert!(!faith.transmissions.contains(&Transmission::LivingOracle));
                }
                if faith.cardinality == Cardinality::Polycentric {
                    assert_ne!(faith.stance, OutgroupStance::Missionary);
                    assert_ne!(faith.state_coupling, StateCoupling::Theocracy);
                }
                if faith.root == MetaphysicalRoot::Monotheism
                    && faith.cardinality == Cardinality::Singular
                {
                    assert_ne!(faith.stance, OutgroupStance::Syncretic);
                }
                if faith.clergy == Clergy::None {
                    assert_ne!(faith.state_coupling, StateCoupling::Theocracy);
                    assert_ne!(faith.state_coupling, StateCoupling::DualPower);
                }
                if !context.tier.is_state() {
                    assert!(matches!(
                        faith.state_coupling,
                        StateCoupling::Autonomous | StateCoupling::FolkDecentralized
                    ));
                }
            }
        }
    }

    #[test]
    fn environments_lean_towards_their_own_powers() {
        let presence = |context: &FaithContext, wanted: &[Domain]| {
            let mut hits = 0usize;
            for seed in 0..300u64 {
                let mut rng = ChaCha8Rng::seed_from_u64(seed);
                let faith = generate_faith(&mut rng, context);
                if faith.domains.iter().any(|domain| wanted.contains(domain)) {
                    hits += 1;
                }
            }
            hits as f32 / 300.0
        };
        let river = presence(
            &river_state(),
            &[Domain::Water, Domain::Fertility, Domain::Sovereignty],
        );
        assert!(
            river > 0.55,
            "a river people did not look to the water: {river:.2}"
        );
        let desert = presence(
            &desert_band(),
            &[Domain::Solar, Domain::Astral, Domain::Lunar, Domain::Water],
        );
        assert!(
            desert > 0.45,
            "a desert people did not look to the sky: {desert:.2}"
        );
        let forest = presence(
            &forest_tribe(),
            &[Domain::Flora, Domain::Fauna, Domain::Earth],
        );
        assert!(
            forest > 0.45,
            "a forest people did not look to the green: {forest:.2}"
        );
        let island = presence(
            &island_merchants(),
            &[Domain::Ocean, Domain::Wind, Domain::Trade],
        );
        assert!(
            island > 0.45,
            "an island people did not look to the sea: {island:.2}"
        );
    }

    #[test]
    fn philosophical_roots_keep_philosophical_domains() {
        let preferred = |domain: Domain| {
            domain.category() == DomainCategory::Metaphysical
                || matches!(
                    domain,
                    Domain::Wisdom
                        | Domain::Ancestor
                        | Domain::Law
                        | Domain::Astral
                        | Domain::Light
                        | Domain::Shadow
                        | Domain::Eclipse
                        | Domain::Healing
                        | Domain::Hearth
                        | Domain::Craft
                        | Domain::Trade
                )
        };
        let mut seen = 0usize;
        let mut total = 0usize;
        let mut primary_violations = 0usize;
        for seed in 0..600u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let faith = generate_faith(&mut rng, &scholarly_state());
            if !faith.root.is_philosophical() {
                continue;
            }
            seen += 1;
            if faith.domains.is_empty() {
                continue;
            }
            total += faith.domains.len();
            for &primary in faith.primary_domains() {
                if matches!(
                    primary,
                    Domain::War | Domain::Fertility | Domain::Hunt | Domain::Storm | Domain::Solar
                ) {
                    primary_violations += 1;
                }
            }
            let preferred_count = faith.domains.iter().filter(|d| preferred(**d)).count();
            assert!(
                preferred_count as f32 / faith.domains.len().max(1) as f32 > 0.75,
                "a philosophical creed rolled a pantheon of nature gods: {:?}",
                faith.domains
            );
        }
        assert!(seen > 0, "no philosophical root was ever drawn");
        assert_eq!(primary_violations, 0);
        assert!(total > 0);
    }

    #[test]
    fn a_martial_country_praises_honor_and_valor_and_holds_war_and_death() {
        let mut honor = 0usize;
        let mut war = 0usize;
        for seed in 0..300u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let faith = generate_faith(&mut rng, &steppe_herders());
            if faith
                .virtues
                .iter()
                .any(|v| matches!(v, Virtue::Honor | Virtue::Valor))
            {
                honor += 1;
            }
            if faith
                .domains
                .iter()
                .any(|d| matches!(d, Domain::War | Domain::Death))
            {
                war += 1;
            }
        }
        assert!(
            honor > 60,
            "a martial country rarely praised honor or valor"
        );
        assert!(war > 60, "a martial country rarely held war or death");
    }

    fn rewire(
        mut faith: Faith,
        root: MetaphysicalRoot,
        stance: OutgroupStance,
        cardinality: Cardinality,
        domains: Vec<Domain>,
    ) -> Faith {
        faith.root = root;
        faith.stance = stance;
        faith.cardinality = cardinality;
        faith.domains = domains;
        faith
    }

    #[test]
    fn inheritance_keeps_root_and_cosmology() {
        let context = river_state();
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let parent = generate_faith(&mut rng, &context);
        for seed in 0..200u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let child = inherit_faith(&mut rng, &parent, &context);
            assert_eq!(child.root, parent.root);
            assert_eq!(child.cosmogony, parent.cosmogony);
            assert_eq!(child.temporality, parent.temporality);
            assert_eq!(child.eschaton, parent.eschaton);
            assert_eq!(child.afterlife, parent.afterlife);
            assert_eq!(
                child.primary_domains(),
                parent.primary_domains(),
                "inheritance replaced a primary domain"
            );
        }
    }

    #[test]
    fn a_singular_faith_keeps_its_one_primary() {
        let context = island_merchants();
        let mut found = None;
        for seed in 0..300u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let faith = generate_faith(&mut rng, &context);
            if faith.cardinality == Cardinality::Singular {
                found = Some(faith);
                break;
            }
        }
        let parent = found.expect("no singular faith was ever drawn");
        let primary = parent.domains[0];
        for seed in 0..500u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let child = inherit_faith(&mut rng, &parent, &context);
            assert_eq!(child.domains[0], primary);
            assert_eq!(child.primary_domains(), &[primary]);
        }
    }

    #[test]
    fn contact_is_single_hop_and_shares_identity() {
        let mut source_ctx = scholarly_state();
        source_ctx.complexity = 0.90;
        let target_ctx = context(
            Environment::Desert,
            CradleKind::DesertOasis,
            SocietyTier::States,
            0.50,
            Subsistence::IrrigatedAgriculture,
            Outlook::Ceremonial,
            Government::Oligarchy,
            Hierarchy::Oligarchic,
        );
        let third_ctx = context(
            Environment::TropicalForest,
            CradleKind::RainforestClearing,
            SocietyTier::SimpleFarmers,
            0.35,
            Subsistence::ShiftingCultivation,
            Outlook::Ceremonial,
            Government::Tribe,
            Hierarchy::Heterarchical,
        );
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let mut a = rewire(
            generate_faith(&mut rng, &source_ctx),
            MetaphysicalRoot::Monotheism,
            OutgroupStance::Missionary,
            Cardinality::Singular,
            vec![Domain::Law],
        );
        a.locus = SacredLocus::Peak;
        let b = rewire(
            generate_faith(&mut rng, &target_ctx),
            MetaphysicalRoot::Polytheism,
            OutgroupStance::Exclusive,
            Cardinality::Hegemonic,
            vec![Domain::Storm, Domain::Water, Domain::War],
        );
        let c = rewire(
            generate_faith(&mut rng, &third_ctx),
            MetaphysicalRoot::Animism,
            OutgroupStance::Exclusive,
            Cardinality::Polycentric,
            vec![Domain::Flora, Domain::Fauna, Domain::Earth],
        );
        let mut faiths = vec![a, b, c];
        let mut founding = vec![1u16, 2u16, 3u16];
        let mut adjacency = BTreeMap::new();
        adjacency.insert(1u16, vec![2u16]);
        adjacency.insert(2u16, vec![3u16]);
        let displaced = apply_contact(
            &mut ChaCha8Rng::seed_from_u64(99),
            &mut faiths,
            &mut founding,
            &[source_ctx, target_ctx, third_ctx],
            &adjacency,
        );
        // Realm 2 joins the monotheism; realm 3 is untouched, because the pass reads
        // one snapshot and a conversion crosses one frontier.
        assert_eq!(founding, vec![1, 1, 3]);
        assert_eq!(displaced, vec![None, Some(2), None]);
        assert_eq!(faiths[0].root, MetaphysicalRoot::Monotheism);
        assert_eq!(faiths[1].root, MetaphysicalRoot::Polytheism);
        assert_eq!(faiths[2].root, MetaphysicalRoot::Animism);
    }

    #[test]
    fn a_secretive_neighbour_plants_a_minority_cult() {
        let mut source_ctx = island_merchants();
        source_ctx.complexity = 0.65;
        let mut target_ctx = forest_tribe();
        target_ctx.complexity = 0.20;
        let mut rng = ChaCha8Rng::seed_from_u64(17);
        let source = rewire(
            generate_faith(&mut rng, &source_ctx),
            MetaphysicalRoot::Polytheism,
            OutgroupStance::Secretive,
            Cardinality::Hegemonic,
            vec![Domain::Shadow, Domain::Water],
        );
        let target = rewire(
            generate_faith(&mut rng, &target_ctx),
            MetaphysicalRoot::Animism,
            OutgroupStance::Pluralistic,
            Cardinality::Polycentric,
            vec![Domain::Flora, Domain::Fauna, Domain::Earth],
        );
        let mut faiths = vec![source, target];
        let mut founding = vec![1u16, 2u16];
        let mut adjacency = BTreeMap::new();
        adjacency.insert(1u16, vec![2u16]);
        apply_contact(
            &mut ChaCha8Rng::seed_from_u64(5),
            &mut faiths,
            &mut founding,
            &[source_ctx, target_ctx],
            &adjacency,
        );
        assert_eq!(faiths[1].cults.len(), 1);
        assert!(faiths[1].cults[0].name.contains("Shadow"));
    }

    #[test]
    fn the_spec_terms_are_spelled_exactly() {
        assert_eq!(
            MetaphysicalRoot::ALL.map(MetaphysicalRoot::label),
            [
                "Monotheism",
                "Dualism",
                "Henotheism",
                "Polytheism",
                "Animism",
                "Pantheism",
                "Panentheism",
                "Deism",
                "Nontheism",
            ]
        );
        assert_eq!(
            Cardinality::ALL.map(Cardinality::label),
            ["Singular", "Dyadic", "Hegemonic", "Polycentric", "Abstract"]
        );
        assert_eq!(
            Cosmogony::ALL.map(Cosmogony::label),
            [
                "Ex Nihilo",
                "Cosmic Ordering",
                "World-Parent",
                "Earth-Diver",
                "Cosmic Egg",
                "Emanation",
                "Divine Conflict",
                "Eternal",
            ]
        );
        assert_eq!(
            Temporality::ALL.map(Temporality::label),
            [
                "Linear",
                "Cyclical",
                "Spiral",
                "Degenerative",
                "Progressive",
                "Timeless",
            ]
        );
        assert_eq!(
            Eschaton::ALL.map(Eschaton::label),
            [
                "Final Judgment",
                "Cosmic Renewal",
                "Endless Continuity",
                "Great Return",
                "Dissolution",
                "Apotheosis",
            ]
        );
        assert_eq!(
            Afterlife::ALL.map(Afterlife::label),
            [
                "Dual Realms",
                "Reincarnation",
                "Shadow Underworld",
                "Ancestral Plane",
                "Heroic Hall",
                "Absorption",
                "Astral Ascent",
                "Oblivion",
            ]
        );
        assert_eq!(Domain::ALL.len(), 39);
        assert_eq!(Domain::CosmicOrder.label(), "Cosmic Order");
        assert_eq!(Domain::Sovereignty.label(), "Sovereignty");
        assert_eq!(Domain::Ancestor.label(), "Ancestor");
        assert_eq!(DomainCategory::Metaphysical.label(), "Metaphysical");
    }

    #[test]
    fn every_root_and_cardinality_is_reachable() {
        let contexts = [
            scholarly_state(),
            river_state(),
            forest_tribe(),
            steppe_herders(),
        ];
        let mut roots = BTreeSet::new();
        let mut cardinalities = BTreeSet::new();
        for seed in 0..400u64 {
            for context in contexts {
                let mut rng = ChaCha8Rng::seed_from_u64(seed);
                let faith = generate_faith(&mut rng, &context);
                roots.insert(faith.root);
                cardinalities.insert(faith.cardinality);
            }
        }
        assert_eq!(roots.len(), MetaphysicalRoot::ALL.len());
        assert_eq!(cardinalities.len(), Cardinality::ALL.len());
    }

    #[test]
    fn native_names_are_native_only_in_the_dossier() {
        let mut faith = generate_faith(&mut ChaCha8Rng::seed_from_u64(31), &river_state());
        assert_eq!(faith.name, faith.gloss);
        assert!(faith.names.deities.is_empty());
        assert!(faith.names.domain_nouns.is_empty());

        faith.root = MetaphysicalRoot::Monotheism;
        faith.cardinality = Cardinality::Hegemonic;
        faith.domains = vec![Domain::Law, Domain::Water];
        faith.emblem = Emblem::Eye;
        faith.name = "Nativafaith".to_string();
        faith.names.domain_nouns = vec!["Nativenoun".to_string(); faith.domains.len()];
        faith.names.deities = vec!["Nativegod".to_string(); faith.domains.len()];
        faith.names.emblem = "Nativeemblem".to_string();

        assert_eq!(faith.display_name(), "Nativafaith");
        assert!(!faith.creed().contains("Native"));
        assert!(faith.deity_lines()[0].contains("Nativegod"));
        assert!(faith.deity_lines()[0].contains("Nativenoun"));
        assert!(faith.emblem_name().contains("Nativeemblem"));
    }

    #[test]
    fn deity_titles_follow_the_object_of_reverence() {
        let mut faith = generate_faith(&mut ChaCha8Rng::seed_from_u64(23), &river_state());
        faith.emblem = Emblem::Star;

        faith.root = MetaphysicalRoot::Monotheism;
        faith.cardinality = Cardinality::Singular;
        faith.domains = vec![Domain::Law];
        assert_eq!(faith.deity_title(), "God");

        faith.root = MetaphysicalRoot::Polytheism;
        faith.cardinality = Cardinality::Polycentric;
        faith.domains = vec![Domain::Lunar, Domain::Solar, Domain::War];
        assert_eq!(faith.deity_title(), "Gods");

        faith.root = MetaphysicalRoot::Animism;
        faith.domains = vec![Domain::Flora, Domain::Fauna, Domain::Hunt];
        assert_eq!(faith.deity_title(), "Totems");

        faith.root = MetaphysicalRoot::Nontheism;
        faith.cardinality = Cardinality::Abstract;
        faith.domains.clear();
        faith.naming = NamingPattern::FounderPath;
        assert_eq!(faith.deity_title(), "Teacher");
    }

    #[test]
    fn deity_lines_show_domain_word_and_name() {
        let mut faith = generate_faith(&mut ChaCha8Rng::seed_from_u64(23), &river_state());
        faith.root = MetaphysicalRoot::Polytheism;
        faith.cardinality = Cardinality::Polycentric;
        faith.domains = vec![Domain::Lunar, Domain::Solar, Domain::War];
        faith.emblem = Emblem::Star;
        faith.names.domain_nouns = vec!["Lun".into(), "Sol".into(), "War".into()];
        faith.names.deities = vec!["Lungod".into(), "Solgod".into(), "Wargod".into()];

        let lines = faith.deity_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Lunar — Lun (Moon) — Lungod");
        assert_eq!(lines[1], "Solar — Sol (Sun) — Solgod");
        assert_eq!(lines[2], "War — War — Wargod");
    }

    #[test]
    fn names_use_only_primary_domains() {
        let context = scholarly_state();
        let mut rng = ChaCha8Rng::seed_from_u64(21);
        let base = generate_faith(&mut rng, &context);
        let mut faith = base.clone();
        faith.root = MetaphysicalRoot::Monotheism;
        faith.cardinality = Cardinality::Hegemonic;
        faith.domains = vec![Domain::Law, Domain::Volcano, Domain::Fertility];
        faith.locus = SacredLocus::Grove;

        for pattern in [
            NamingPattern::RootEpithet,
            NamingPattern::DomainOrder,
            NamingPattern::FounderPath,
        ] {
            faith.naming = pattern;
            let name = build_faith_name(&mut ChaCha8Rng::seed_from_u64(2), &faith);
            assert!(name.contains("Law"), "{pattern:?} lost the primary: {name}");
            assert!(
                !name.contains("Volcano"),
                "{pattern:?} used a secondary: {name}"
            );
            assert!(
                !name.contains("Harvest"),
                "{pattern:?} used a secondary: {name}"
            );
        }

        faith.naming = NamingPattern::LocusCovenant;
        let name = build_faith_name(&mut ChaCha8Rng::seed_from_u64(2), &faith);
        assert!(name.contains("Grove"));
        assert!(!name.contains("Volcano"));

        faith.cardinality = Cardinality::Dyadic;
        faith.domains = vec![Domain::Light, Domain::Shadow, Domain::Volcano];
        faith.naming = NamingPattern::DualPolarity;
        let name = build_faith_name(&mut ChaCha8Rng::seed_from_u64(2), &faith);
        assert_eq!(name, "Light and Shadow");
    }

    #[test]
    fn an_abstract_faith_shows_one_core_line() {
        let mut faith = generate_faith(&mut ChaCha8Rng::seed_from_u64(29), &river_state());
        faith.root = MetaphysicalRoot::Nontheism;
        faith.cardinality = Cardinality::Abstract;
        faith.domains.clear();
        faith.naming = NamingPattern::FounderPath;
        faith.names.core = "Suhu".to_string();

        let lines = faith.deity_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "Suhu (the Teacher)");
        assert_eq!(faith.deity_title(), "Teacher");
    }

    #[test]
    fn sacred_sites_list_their_provinces_and_roles() {
        let mut faith = generate_faith(&mut ChaCha8Rng::seed_from_u64(37), &river_state());
        faith.sacred_sites = vec![
            SacredSite {
                name: "Kinõlawu".to_string(),
                province: 2493,
                domain: None,
                primary: true,
            },
            SacredSite {
                name: "Lertow'õka".to_string(),
                province: 1181,
                domain: Some(Domain::Lunar),
                primary: false,
            },
        ];
        assert_eq!(faith.sacred_site_title(), "Sacred Sites");
        let lines = faith.sacred_site_lines();
        assert_eq!(lines[0], "Kinõlawu — province #2493 — primary");
        assert_eq!(lines[1], "Lertow'õka — province #1181 — Lunar");

        faith.sacred_sites.truncate(1);
        assert_eq!(faith.sacred_site_title(), "Sacred Site");
        assert_eq!(faith.sacred_site_lines()[0], "Kinõlawu — province #2493");
    }
}
