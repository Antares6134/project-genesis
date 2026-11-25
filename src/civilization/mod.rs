//! History phase — societies & faiths: the civilizations that rise on a finished planet.
//!
//! The physical pipeline decides where there is land, water, rain and soil; the
//! geopolitical pipeline cuts that land into provinces. This module asks the next
//! question a world-builder asks: **where did people first stop being a landscape
//! and start being a story?**
//!
//! The model is deliberately the same shape as history: a small number of
//! *cradles* — a handful of exceptionally fertile, well-watered, defensible spots
//! — each of which grows a civilization outward along fertile ground until it is
//! stopped by wilderness, mountains, or an older neighbour. Nothing is placed at
//! random: every cradle is chosen because the province under it is measurably
//! better than its neighbours, and the dossier reports the kind of country it was
//! born into.
//!
//! ## The calendar
//!
//! The calendar begins where the first civilization does: the founding of the first
//! city is **Year 0**, and every later date is a plain count of years from it —
//! `Year 1 111`, never a BCE/CE pair. There is no separate epoch place to choose and
//! no era before the count: what precedes Year 0 is prehistory, a separate, much
//! larger quantity drawn per world and biased by how kind the planet turned out to
//! be. There is no fixed span between the first people and the first city.
//!
//! The atlas then portrays the world at a single date (see
//! [`CivilizationWorld::map_year`]), at which the world stands in an age, every
//! realm has a government, a culture and a population, and every province keeps a
//! faith of its own. The faith is a seven-level cascade — metaphysical root,
//! domain architecture, cosmology, ethos, praxis, polity and manifestation —
//! grown from the country and society it belongs to and spread across the
//! province graph; see [`faith`] and [`religion`] for the whole model.
//!
//! ## Determinism
//!
//! Everything here is drawn from the world seed through a `ChaCha8Rng` seeded
//! with a fixed salt, and every collection that the RNG walks is a `Vec` sorted
//! by province id or a `BTreeMap`. The same seed therefore always produces the
//! same civilizations, on the same provinces, with the same dates and
//! populations.

pub mod age;
pub mod config;
pub mod epoch;
pub mod faith;
pub mod generator;
pub mod religion;
pub mod report;

pub use age::Age;
pub use config::CivilizationConfig;
#[allow(unused_imports)]
pub use epoch::Timeline;
pub use faith::{Faith, MetaphysicalRoot};
pub use generator::generate_civilizations;
#[allow(unused_imports)]
pub use religion::RealmFaith;
pub use religion::{RealmReligion, Religion, ReligionWorld};
pub use report::{report_civilizations, report_civilizations_all, report_languages};

// ─── WHY A CIVILIZATION ROSE WHERE IT DID ────────────────────────────────────

/// The physical situation that made a cradle worth settling.
///
/// This is the single most important category in the dossier: it drives the
/// economy, the social order, the faith and the architecture that follow, and it
/// is the first line a reader sees when hovering a rising civilization.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CradleKind {
    /// A great river's floodplain: annual silt, irrigation, no rain needed.
    RiverFloodplain,
    /// A great river meeting the sea: silt, salt, fish and a harbour at once.
    RiverDelta,
    /// A lake shore: fish, reeds, fresh water and a navigable surface.
    Lacustrine,
    /// A temperate shore with no great river: boats, fish and salt.
    CoastalMaritime,
    /// A sheltered mountain valley: terraces, springs, and a wall of rock.
    HighlandValley,
    /// An arid land with one reliable spring, river reach or shore.
    DesertOasis,
    /// Dry grassland: herds first, ploughs later.
    SteppeGrassland,
    /// Hot, seasonally wet grass and scrub: grain and herds together.
    SavannaHeartland,
    /// Wet tropical forest broken by rivers: slash, burn, plant, move.
    RainforestClearing,
    /// Middle-latitude forest with rain and soil enough for grain, timber and
    /// forage: the temperate breadbasket.
    WoodlandHeartland,
    /// A shore reached only by water: an island civilization.
    IslandShore,
}

impl CradleKind {
    /// Short name of the cradle, for the atlas legend and the dossier header.
    pub fn label(self) -> &'static str {
        match self {
            CradleKind::RiverFloodplain => "River Floodplain",
            CradleKind::RiverDelta => "River Delta",
            CradleKind::Lacustrine => "Lakeshore",
            CradleKind::CoastalMaritime => "Open Coast",
            CradleKind::HighlandValley => "Highland Valley",
            CradleKind::DesertOasis => "Desert Oasis",
            CradleKind::SteppeGrassland => "Steppe Grassland",
            CradleKind::SavannaHeartland => "Savanna Heartland",
            CradleKind::RainforestClearing => "Rainforest Clearing",
            CradleKind::WoodlandHeartland => "Woodland Heartland",
            CradleKind::IslandShore => "Island Shore",
        }
    }

    /// The adjective a language family is named after: realms that grew out of
    /// the same kind of country tend to be named the same way.
    pub fn tongue(self) -> &'static str {
        match self {
            CradleKind::RiverFloodplain => "Riparian",
            CradleKind::RiverDelta => "Delta",
            CradleKind::Lacustrine => "Lacustrine",
            CradleKind::CoastalMaritime => "Maritime",
            CradleKind::HighlandValley => "Highland",
            CradleKind::DesertOasis => "Oasis",
            CradleKind::SteppeGrassland => "Steppe",
            CradleKind::SavannaHeartland => "Savanna",
            CradleKind::RainforestClearing => "Rainforest",
            CradleKind::WoodlandHeartland => "Woodland",
            CradleKind::IslandShore => "Island",
        }
    }
}

// ─── WHAT COUNTRY A PEOPLE LIVES IN ──────────────────────────────────────────

/// The biome a province's people live in.
///
/// Read off a province's own climate, soil, relief and water — for **every**
/// inhabited province, not only for the handful of seats that grow states. This is
/// the coarse half of the model: the environment decides what a country *can*
/// feed (see [`Subsistence`]), while how far it lies from the nearest state decides
/// how much of that potential has actually been worked (see [`SocietyTier`]).
///
/// Where [`CradleKind`] answers "why did a *state* rise here?", `Environment`
/// answers "what is the country like?" for a forest band or a herd people too.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Environment {
    /// Permanent ice and the frost-shattered ground at its edge.
    IceMargin,
    /// Cold, treeless ground: lichen, moss and the summer herd.
    Tundra,
    /// Cold conifer forest and bog.
    Taiga,
    /// Temperate forest with rain and soil enough for grain, timber and forage.
    Woodland,
    /// Dry grassland: too dry for a plough, rich enough for herds.
    Steppe,
    /// Hot, seasonally wet grass and scrub: grain and herds together.
    Savanna,
    /// Arid ground where rain almost never falls.
    Desert,
    /// Wet tropical forest broken by rivers.
    TropicalForest,
    /// High, thin-aired ground: terraces, springs and a wall of rock.
    Highland,
    /// A shore reached only by water: an island's country.
    IslandShore,
    /// A lake shore: fish, reeds, fresh water and a navigable surface.
    Lacustrine,
    /// A great river's floodplain: annual silt and irrigation.
    Floodplain,
    /// A great river meeting the sea: silt, salt, fish and a harbour at once.
    Delta,
    /// A temperate shore with no great river: boats, fish and salt.
    Coast,
}

impl Environment {
    /// Short name of the country, for the atlas legend and the dossier.
    pub fn label(self) -> &'static str {
        match self {
            Environment::IceMargin => "Ice Margin",
            Environment::Tundra => "Tundra",
            Environment::Taiga => "Taiga",
            Environment::Woodland => "Woodland",
            Environment::Steppe => "Steppe",
            Environment::Savanna => "Savanna",
            Environment::Desert => "Desert",
            Environment::TropicalForest => "Tropical Forest",
            Environment::Highland => "Highland",
            Environment::IslandShore => "Island Shore",
            Environment::Lacustrine => "Lakeshore",
            Environment::Floodplain => "River Floodplain",
            Environment::Delta => "River Delta",
            Environment::Coast => "Open Coast",
        }
    }

    /// The noun a people of this country are named by: `taiga hunter-gatherers`.
    pub fn noun(self) -> &'static str {
        match self {
            Environment::IceMargin => "polar",
            Environment::Tundra => "tundra",
            Environment::Taiga => "taiga",
            Environment::Woodland => "woodland",
            Environment::Steppe => "steppe",
            Environment::Savanna => "savanna",
            Environment::Desert => "desert",
            Environment::TropicalForest => "tropical forest",
            Environment::Highland => "highland",
            Environment::IslandShore => "island",
            Environment::Lacustrine => "lakeside",
            Environment::Floodplain => "floodplain",
            Environment::Delta => "delta",
            Environment::Coast => "coastal",
        }
    }
}

// ─── HOW A CIVILIZATION LIVES ────────────────────────────────────────────────

/// How a civilization feeds itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Subsistence {
    IrrigatedAgriculture,
    DrylandFarming,
    Pastoralism,
    FishingAndSeafaring,
    ShiftingCultivation,
    TerraceFarming,
    /// Hunting, fishing and gathering: no field is planted and no herd is kept.
    Foraging,
}

impl Subsistence {
    pub fn label(self) -> &'static str {
        match self {
            Subsistence::IrrigatedAgriculture => "Irrigated Agriculture",
            Subsistence::DrylandFarming => "Dryland Farming",
            Subsistence::Pastoralism => "Herds and Transhumance",
            Subsistence::FishingAndSeafaring => "Fishing and Seafaring",
            Subsistence::ShiftingCultivation => "Shifting Cultivation",
            Subsistence::TerraceFarming => "Terrace Farming",
            Subsistence::Foraging => "Hunting and Gathering",
        }
    }
}

// ─── HOW ORGANISED A PEOPLE IS ───────────────────────────────────────────────

/// How far a people has organised itself — the five-way scheme the atlas paints.
///
/// This is the coarse band of the same axis [`Government`] walks in detail, and it
/// is what the Civilizations map mode colours by. A world at any date is a *mosaic*
/// of all five: a state with archives and canals is a contemporary of a band of
/// foragers two valleys away, and the map shows both. The tier decides a society's
/// colour, how many people each province feeds, and what the atlas calls it.
///
/// Ordered from least to most organised, so a comparison is also a ranking.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum SocietyTier {
    /// Bands who hunt, fish and gather: no fields, no herds, no state.
    HunterGatherers,
    /// Herders who move with their animals: wealth walks, and so do they.
    NomadicPastoralists,
    /// Villages that farm, with no rank or apparatus above the household.
    SimpleFarmers,
    /// Ranked farming societies under chiefs and lineages.
    ComplexFarmers,
    /// States: offices, an apparatus, and power held rather than shared by all.
    States,
}

/// Hue, saturation and value of an 8-bit colour: hue in degrees, the other two in
/// `0..=1`.
///
/// The atlas's palettes are written as RGB because that is what the painter and the
/// legend both want; a colour *family* is easier to walk in HSV, where a step is a
/// step in hue or in value rather than an arbitrary mix of the three channels.
fn rgb_to_hsv(rgb: [u8; 3]) -> (f32, f32, f32) {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    let hue = if chroma <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * ((g - b) / chroma).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    let saturation = if max <= f32::EPSILON {
        0.0
    } else {
        chroma / max
    };
    (hue, saturation, max)
}

/// The inverse of [`rgb_to_hsv`], with the hue wrapping and the rest clamped.
fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let hue = hue.rem_euclid(360.0);
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);
    let chroma = value * saturation;
    let sector = hue / 60.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    [
        ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// CIE L\*a\*b\* of an 8-bit sRGB colour.
///
/// Contrast is a claim about the eye, so it is measured in perceptual units
/// rather than in channel differences: two fills a hundred RGB steps apart can be
/// the same colour to a reader, and two fills ten steps apart can be different.
fn lab(colour: [u8; 3]) -> [f32; 3] {
    let linear = |channel: u8| {
        let value = channel as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(colour[0]), linear(colour[1]), linear(colour[2]));
    let x = r * 0.4124 + g * 0.3576 + b * 0.1805;
    let y = r * 0.2126 + g * 0.7152 + b * 0.0722;
    let z = r * 0.0193 + g * 0.1192 + b * 0.9505;
    let f = |t: f32| {
        if t > 0.008_856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.950_47), f(y), f(z / 1.088_83));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// ΔE\*ab — about 2.3 is a just-noticeable difference, 20 is a clear one.
fn delta_e(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (a, b) = (lab(a), lab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl SocietyTier {
    /// Every tier, least organised first — the atlas legend's order.
    pub const LADDER: [SocietyTier; 5] = [
        SocietyTier::HunterGatherers,
        SocietyTier::NomadicPastoralists,
        SocietyTier::SimpleFarmers,
        SocietyTier::ComplexFarmers,
        SocietyTier::States,
    ];

    /// The tier's index on the ladder, for the per-province tier table.
    pub fn index(self) -> u8 {
        match self {
            SocietyTier::HunterGatherers => 0,
            SocietyTier::NomadicPastoralists => 1,
            SocietyTier::SimpleFarmers => 2,
            SocietyTier::ComplexFarmers => 3,
            SocietyTier::States => 4,
        }
    }

    /// The tier an index came from; out-of-range indices fall back to the least
    /// organised tier rather than panicking.
    pub fn from_index(index: u8) -> Self {
        match index {
            1 => SocietyTier::NomadicPastoralists,
            2 => SocietyTier::SimpleFarmers,
            3 => SocietyTier::ComplexFarmers,
            4 => SocietyTier::States,
            _ => SocietyTier::HunterGatherers,
        }
    }

    /// The name the atlas writes for this tier.
    pub fn label(self) -> &'static str {
        match self {
            SocietyTier::HunterGatherers => "Hunter-Gatherers",
            SocietyTier::NomadicPastoralists => "Nomadic Pastoralists",
            SocietyTier::SimpleFarmers => "Simple Farming Societies",
            SocietyTier::ComplexFarmers => "Complex Farming Societies",
            SocietyTier::States => "State Societies",
        }
    }

    /// The suffix a society of this tier is named by.
    pub fn noun(self) -> &'static str {
        match self {
            SocietyTier::HunterGatherers => "hunter-gatherers",
            SocietyTier::NomadicPastoralists => "pastoralists",
            SocietyTier::SimpleFarmers => "farmers",
            SocietyTier::ComplexFarmers => "chiefdoms",
            SocietyTier::States => "realm",
        }
    }

    /// The number of shades one tier's colour family carries.
    ///
    /// Nine is what a *planar* map needs: a tier's provinces form a flat graph, so
    /// four shades would in principle suffice; nine leaves room for the greedy walk
    /// to be order-blind and still never repeat a shade next door.
    pub const SHADES: usize = 9;

    /// The base colour of this tier on the Civilizations map mode.
    ///
    /// Five hues, read as a ladder rather than as a set: the palette darkens and
    /// deepens from the bands that hold most of the world to the states that hold
    /// least of it, so the map answers "where are the states?" before the reader has
    /// found the key. Every hue is deliberately far from water — the old state blue
    /// sat within a perceptual step of the shelf colour, which made every realm on a
    /// coast vanish into the sea — and the hunter-gatherers keep enough chroma to
    /// read as country rather than as blank paper, because they own two thirds of
    /// most worlds.
    pub fn legend_color(self) -> [u8; 3] {
        match self {
            SocietyTier::HunterGatherers => [198, 195, 154],
            SocietyTier::NomadicPastoralists => [198, 158, 92],
            SocietyTier::SimpleFarmers => [120, 157, 80],
            SocietyTier::ComplexFarmers => [168, 80, 58],
            SocietyTier::States => [96, 42, 60],
        }
    }

    /// The `index`-th shade of this tier's colour family.
    ///
    /// The family is a real neighbourhood, not a lightness ramp: each step drifts
    /// the hue, the chroma and the value together, so two peoples of one tier read
    /// as two peoples while still belonging to the same hue. Index wraps, and the
    /// walk is deterministic, so the same seed paints the same map.
    pub fn shade(self, index: usize) -> [u8; 3] {
        let (hue, saturation, value) = rgb_to_hsv(self.legend_color());
        let half = (Self::SHADES - 1) as f32 * 0.5;
        let t = (index % Self::SHADES) as f32 / half - 1.0;
        hsv_to_rgb(
            hue + t * 10.0,
            saturation * (1.0 - 0.10 * t),
            value * (1.0 + 0.06 * t),
        )
    }

    /// True for the tiers with a centralised apparatus behind them.
    pub fn is_state(self) -> bool {
        self == SocietyTier::States
    }

    /// How much of a province's carrying capacity this tier actually draws.
    ///
    /// A province's capacity is measured as what a *state* could take from it; every
    /// other tier is a fraction of that. A band of foragers lives off the same
    /// country at less than a hundredth of it, a ranked chiefdom at a half, and a
    /// state's figure is then scaled again by its own measured development. These few
    /// numbers are what makes a world's population look like a world's rather than a
    /// sum of empires.
    pub fn density_factor(self) -> f32 {
        match self {
            SocietyTier::HunterGatherers => 0.007,
            SocietyTier::NomadicPastoralists => 0.045,
            SocietyTier::SimpleFarmers => 0.150,
            SocietyTier::ComplexFarmers => 0.500,
            SocietyTier::States => 1.0,
        }
    }

    /// The singular form the dossier uses for one society: `Organized State`.
    pub fn form(self) -> &'static str {
        match self {
            SocietyTier::HunterGatherers => "Hunter-Gatherers",
            SocietyTier::NomadicPastoralists => "Nomadic Pastoralists",
            SocietyTier::SimpleFarmers => "Simple Farmers",
            SocietyTier::ComplexFarmers => "Complex Farmers",
            SocietyTier::States => "Organized State",
        }
    }

    /// How fixed a society is to its ground, for the dossier's summary line.
    pub fn mobility(self) -> &'static str {
        match self {
            SocietyTier::HunterGatherers => "Mobile",
            SocietyTier::NomadicPastoralists => "Nomadic",
            SocietyTier::SimpleFarmers | SocietyTier::ComplexFarmers | SocietyTier::States => {
                "Sedentary"
            }
        }
    }
}

/// How a civilization organises power — from kinship society up through states.
///
/// A single axis, ordered by complexity. The first three are **pre-state**
/// societies without a centralised apparatus; the rest are **states**,
/// distinguished by how power is held and legitimated. Scope — a city-state
/// against a broad realm — is deliberately *not* here: that is a matter of size,
/// carried by the realm's own area and population, not a form of government.
///
/// Which one a realm gets follows from its own development: a kinship band or a
/// tribe where complexity is low, a chiefdom as it rises, and one of the state
/// regimes once it is high — the *kind* of state then following from how it eats
/// ([`Subsistence`]) and what it values ([`Outlook`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Government {
    /// A kinship band: no ranks and no offices, decisions by consent.
    Band,
    /// A tribal society: councils of elders and lineages, no state above them.
    Tribe,
    /// A chiefdom: a ranked society under a hereditary chief, bound by kinship.
    Chiefdom,
    /// A god-king: the ruler is the state's cult as well as its head.
    DivineMonarchy,
    /// Rule by priests: the temple is the administration.
    TheocraticRepublic,
    /// Rule by a hereditary or landed elite.
    AristocraticRepublic,
    /// Rule by wealth: merchants and landowners hold the offices.
    PlutocraticRepublic,
    /// Rule by a narrow clique that keeps itself closed.
    Oligarchy,
    /// One man ruling against the law, held on to by force.
    Tyranny,
}

impl Government {
    pub fn label(self) -> &'static str {
        match self {
            Government::Band => "Band Society",
            Government::Tribe => "Tribal Society",
            Government::Chiefdom => "Chiefdom",
            Government::DivineMonarchy => "Divine Monarchy",
            Government::TheocraticRepublic => "Theocratic Republic",
            Government::AristocraticRepublic => "Aristocratic Republic",
            Government::PlutocraticRepublic => "Plutocratic Republic",
            Government::Oligarchy => "Oligarchy",
            Government::Tyranny => "Tyranny",
        }
    }

    /// True for a government with a centralised state apparatus behind it, false
    /// for the kinship societies that came before the state.
    #[allow(dead_code)]
    pub fn is_state(self) -> bool {
        !matches!(
            self,
            Government::Band | Government::Tribe | Government::Chiefdom
        )
    }

    /// The density bias a government lends to population: a canal state that
    /// taxes, stores and redistributes grain feeds far more mouths per hectare
    /// than a kinship band can.
    pub fn density_bias(self) -> f32 {
        match self {
            Government::Band => 0.00,
            Government::Tribe => 0.04,
            Government::Chiefdom => 0.08,
            Government::DivineMonarchy => 0.24,
            Government::TheocraticRepublic => 0.26,
            Government::AristocraticRepublic => 0.16,
            Government::PlutocraticRepublic => 0.18,
            Government::Oligarchy => 0.14,
            Government::Tyranny => 0.10,
        }
    }
}

/// How a civilization ranks its people — the shape of its social hierarchy.
///
/// Where [`Government`] answers "who holds power, and how is that power
/// legitimated?", `Hierarchy` answers "how is the society itself stratified?".
/// The two are cousins but not the same: a temple state can be governed by a
/// theocratic council while its people are ranked by a hereditary caste order,
/// and a chiefdom is both a government and a hierarchy at once.
///
/// The forms are listed from the flattest society to the broadest and most
/// stratified one; the first three belong to kinship societies, the rest can only
/// be carried by a state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Hierarchy {
    /// No formal ranks: decisions by consent and no office above the household.
    Egalitarian,
    /// Many overlapping authorities — lineages, councils, age grades — and no
    /// single apex.
    Heterarchical,
    /// Ranked lineages bound by kinship, the social order under a chief.
    RankedKinship,
    /// A military elite holds the ranks and the land behind them.
    WarriorElite,
    /// A narrow, self-renewing circle of families or wealth holds the offices.
    Oligarchic,
    /// Office and examination rather than blood order the society.
    Bureaucratic,
    /// The ruler is sacred, and the priesthood ranks the realm beneath him.
    SacredKingship,
    /// Hereditary, endogamous groups fixed in ritual rank.
    Caste,
    /// A vast, many-peopled realm whose apex is an emperor over subject elites.
    Imperial,
}

impl Hierarchy {
    /// The name the dossier and the atlas write for this hierarchy.
    pub fn label(self) -> &'static str {
        match self {
            Hierarchy::Egalitarian => "Egalitarian",
            Hierarchy::Heterarchical => "Heterarchical",
            Hierarchy::RankedKinship => "Ranked Kinship",
            Hierarchy::WarriorElite => "Warrior-Elite",
            Hierarchy::Oligarchic => "Oligarchic",
            Hierarchy::Bureaucratic => "Bureaucratic",
            Hierarchy::SacredKingship => "Sacred Kingship",
            Hierarchy::Caste => "Caste",
            Hierarchy::Imperial => "Imperial",
        }
    }

    /// True for the hierarchies only a centralised state can carry; false for
    /// the kinship forms that come before the state.
    pub fn is_state(self) -> bool {
        !matches!(
            self,
            Hierarchy::Egalitarian | Hierarchy::Heterarchical | Hierarchy::RankedKinship
        )
    }
}

/// What a civilization values — its public character.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Outlook {
    Cosmopolitan,
    Mercantile,
    Martial,
    Scholarly,
    Ceremonial,
    Artistic,
    Isolationist,
}

impl Outlook {
    pub fn label(self) -> &'static str {
        match self {
            Outlook::Cosmopolitan => "Cosmopolitan",
            Outlook::Mercantile => "Mercantile",
            Outlook::Martial => "Martial",
            Outlook::Scholarly => "Scholarly",
            Outlook::Ceremonial => "Ceremonial",
            Outlook::Artistic => "Artistic",
            Outlook::Isolationist => "Isolationist",
        }
    }

    /// The population bias of an ethos at the moment of the map.
    pub fn density_bias(self) -> f32 {
        match self {
            Outlook::Cosmopolitan => 0.14,
            Outlook::Mercantile => 0.12,
            Outlook::Scholarly => 0.06,
            Outlook::Ceremonial => 0.02,
            Outlook::Artistic => 0.00,
            Outlook::Martial => -0.04,
            Outlook::Isolationist => -0.06,
        }
    }

    /// How much this ethos leans on borrowing from abroad.
    pub fn openness(self) -> f32 {
        match self {
            Outlook::Cosmopolitan => 1.30,
            Outlook::Mercantile => 1.25,
            Outlook::Artistic => 1.15,
            Outlook::Scholarly => 1.10,
            Outlook::Ceremonial => 0.95,
            Outlook::Martial => 0.85,
            Outlook::Isolationist => 0.45,
        }
    }
}

// ─── HOW BIG A REALM IS ──────────────────────────────────────────────────────

/// The scale of a civilization, measured against our own history.
///
/// The class is *derived from the finished territory* rather than assigned in
/// advance: a realm that aimed at empire size and then ran into a mountain wall
/// or an older neighbour reports the size it actually reached.
///
/// Kept internal on purpose: scope is a number the atlas shows as area and
/// population, never as a rank — a small polis and a broad empire are told apart
/// by their figures, not by a tier.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ScaleClass {
    Small,
    Medium,
    Large,
}

impl ScaleClass {
    /// Classify a finished territory by the provinces it holds.
    pub fn from_provinces(provinces: usize) -> Self {
        if provinces >= 55 {
            ScaleClass::Large
        } else if provinces >= 22 {
            ScaleClass::Medium
        } else {
            ScaleClass::Small
        }
    }
}

// ─── THE DOSSIER ─────────────────────────────────────────────────────────────

/// The material and social life of a civilization.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Culture {
    pub subsistence: Subsistence,
    pub government: Government,
    /// How the society itself is ranked, as distinct from how it is governed.
    pub hierarchy: Hierarchy,
    pub outlook: Outlook,
    /// The form its monumental building takes.
    ///
    /// An owned string rather than a `&'static str` so that a finished world can
    /// be written to the cache and read back: bincode can serialise a borrowed
    /// string but cannot deserialise one.
    pub architecture: String,
    /// Two or three arts the realm is known for.
    #[allow(dead_code)]
    pub arts: Vec<String>,
}

/// One civilization, from its founding year to the date the atlas portrays.
///
/// A realm's *id* — the number used by [`CivilizationWorld::name_of`] — is its
/// 1-based position in [`CivilizationWorld::civilizations`], which is the
/// founding order. There is deliberately no separate field for it, so the two
/// can never drift apart.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Civilization {
    /// Display name. Deliberately a placeholder: names come later in the
    /// pipeline, once languages exist.
    pub name: String,
    /// The realm's colour on the Civilizations map mode.
    pub color: [u8; 3],

    /// How organised this people is — the five-way scheme the atlas paints.
    pub tier: SocietyTier,
    /// The country they live in, read off their own provinces.
    pub environment: Environment,

    pub cradle_kind: CradleKind,
    /// The realm's size class, kept internal: scope is shown as area and
    /// population, never as a tier.
    #[allow(dead_code)]
    pub scale: ScaleClass,
    pub culture: Culture,

    /// The province the realm was born in.
    pub seat_province: u32,
    /// The seat's position on the canvas, in pixels.
    pub seat_x: f32,
    pub seat_y: f32,
    /// Latitude of the seat, in degrees (negative = southern).
    #[allow(dead_code)]
    pub seat_latitude_deg: f32,
    /// Longitude of the seat, in degrees east (negative = west of the prime meridian).
    #[allow(dead_code)]
    pub seat_longitude_deg: f32,
    /// Every province the realm holds at the map date, ascending.
    pub provinces: Vec<u32>,
    /// Ground area of the realm, in km², integrated over latitude.
    pub area_km2: f32,
    /// Total population at the map date.
    pub population: u64,

    /// The year the realm was founded, in the world's calendar: the count of years
    /// since the first city, which stands at Year 0. See
    /// [`epoch::year_label`](crate::civilization::epoch::year_label).
    ///
    /// `None` for a people rather than a polity: a band of foragers and a herd
    /// people have no founding year, because nobody founded them — they were
    /// already there. Dating them would repeat the error the dossier's whole
    /// rework exists to undo.
    pub founding_year: Option<i32>,
    /// Years elapsed between founding and the map date. Zero for a people.
    #[allow(dead_code)]
    pub age_years: u32,

    /// The tongue family this realm belongs to, e.g. `Riparian Tongue`.
    pub language_family: String,

    /// Latitude-aware population density of the realm at the map date.
    pub density_per_km2: f32,
}

impl Civilization {
    /// `Year 1 240` — the realm's own calendar at the map date.
    /// The founding date for a polity, or the bare fact of prior occupation for a
    /// people. The dossier supplies the `Founded:` label.
    pub fn origin_label(&self) -> String {
        match self.founding_year {
            Some(year) => epoch::year_label(year),
            None => "Prehistoric".to_string(),
        }
    }
}

// ─── THE WORLD'S DAWN, AS ONE OBJECT ─────────────────────────────────────────

/// Every civilization of one world, plus the two per-province tables the atlas
/// needs to paint them: who owns a province, and how many people live in it.
///
/// The per-province tables are indexed by province id and are therefore the same
/// length as [`GeopoliticalHierarchy::provinces`](crate::geopolitics::GeopoliticalHierarchy::provinces),
/// so the viewer can look a hovered pixel up in one step.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CivilizationWorld {
    /// The seed the whole world was grown from.
    pub seed: u64,
    /// The date the atlas portrays, in the world's calendar: the count of years
    /// since the first city, which stands at Year 0. It is set a few centuries after
    /// the youngest realm began, so every realm on the map exists and none has to be
    /// extrapolated. See [`Timeline`].
    pub map_year: i32,
    /// The world's calendar: what its years are reckoned from, and how deep its
    /// prehistory runs.
    pub timeline: Timeline,
    /// Number of civilizations that rose.
    pub civilizations: Vec<Civilization>,
    /// Province id → owning civilization id (0 = wilderness, water or wasteland).
    pub province_owner: Vec<u16>,
    /// Province id → the tier of whoever holds it, as a [`SocietyTier::index`].
    /// The atlas colours by this, so the map reads as a mosaic of societies even
    /// where the holder itself is too small to see.
    pub province_tier: Vec<u8>,
    /// Province id → population at the map date (0 = not civilized).
    pub province_population: Vec<u64>,
    /// Sum of every civilized province's population.
    pub total_population: u64,
    /// Number of provinces held by *somebody* — every inhabited province on the
    /// world, since the dawn partitions the whole of it between peoples and states.
    pub civilized_provinces: usize,
    /// Total area held by all civilizations, in km².
    pub civilized_area_km2: f32,

    /// Every religion of the world, 1-based by position. A faith belongs to the
    /// world, not to one realm: two realms may keep the same creed here, and one
    /// realm may keep several.
    pub religions: Vec<Religion>,
    /// Province id → the religion it keeps (0 = water, wasteland or unsettled).
    pub province_religion: Vec<u16>,
    /// Civilization id - 1 → what its people actually keep. There is no state
    /// religion in this table yet: the map reports faith, not patronage.
    pub religion_of_realm: Vec<RealmReligion>,
}

impl CivilizationWorld {
    /// A world with no civilizations yet — the empty result the atlas falls back
    /// on when the history stage is not asked for.
    pub fn empty(seed: u64) -> Self {
        Self {
            seed,
            map_year: 0,
            timeline: Timeline::empty(),
            civilizations: Vec::new(),
            province_owner: Vec::new(),
            province_tier: Vec::new(),
            province_population: Vec::new(),
            total_population: 0,
            civilized_provinces: 0,
            civilized_area_km2: 0.0,
            religions: Vec::new(),
            province_religion: Vec::new(),
            religion_of_realm: Vec::new(),
        }
    }

    /// True when nothing has risen on this world.
    pub fn is_empty(&self) -> bool {
        self.civilizations.is_empty()
    }

    /// Id of the civilization holding `province_id`, or 0.
    #[inline(always)]
    pub fn owner_of(&self, province_id: u32) -> u32 {
        self.province_owner
            .get(province_id as usize)
            .copied()
            .unwrap_or(0) as u32
    }

    /// Population of `province_id` at the map date, or 0.
    #[inline(always)]
    pub fn population_of(&self, province_id: u32) -> u64 {
        self.province_population
            .get(province_id as usize)
            .copied()
            .unwrap_or(0)
    }

    /// The tier of `province_id`'s holder, or `None` where nobody lives.
    #[inline(always)]
    pub fn tier_of(&self, province_id: u32) -> Option<SocietyTier> {
        let raw = self.province_tier.get(province_id as usize).copied()?;
        if self.owner_of(province_id) == 0 {
            return None;
        }
        Some(SocietyTier::from_index(raw))
    }

    /// How many societies stand at each tier, in [`SocietyTier::LADDER`] order.
    pub fn counts_by_tier(&self) -> [usize; 5] {
        let mut counts = [0usize; 5];
        for civ in &self.civilizations {
            counts[civ.tier.index() as usize] += 1;
        }
        counts
    }

    /// How much land each tier holds, in km², in [`SocietyTier::LADDER`] order.
    pub fn area_by_tier(&self) -> [f32; 5] {
        let mut areas = [0.0f32; 5];
        for civ in &self.civilizations {
            areas[civ.tier.index() as usize] += civ.area_km2;
        }
        areas
    }

    /// How many people each tier feeds, in [`SocietyTier::LADDER`] order.
    pub fn population_by_tier(&self) -> [u64; 5] {
        let mut totals = [0u64; 5];
        for civ in &self.civilizations {
            totals[civ.tier.index() as usize] += civ.population;
        }
        totals
    }

    /// How many provinces each tier holds, in [`SocietyTier::LADDER`] order.
    pub fn provinces_by_tier(&self) -> [usize; 5] {
        let mut counts = [0usize; 5];
        for civ in &self.civilizations {
            counts[civ.tier.index() as usize] += civ.provinces.len();
        }
        counts
    }

    /// How many religions stand under each metaphysical root, in
    /// [`MetaphysicalRoot::ALL`] order.
    ///
    /// This counts *religions that are still kept somewhere*, not realms: a
    /// religion can span several countries, and a country can keep several
    /// religions.
    pub fn counts_by_root(&self) -> [usize; MetaphysicalRoot::ALL.len()] {
        let mut counts = [0usize; MetaphysicalRoot::ALL.len()];
        for religion in &self.religions {
            counts[religion.faith.root.index()] += 1;
        }
        counts
    }

    /// How much land each metaphysical root holds, in [`MetaphysicalRoot::ALL`]
    /// order.
    pub fn area_by_root(&self) -> [f32; MetaphysicalRoot::ALL.len()] {
        let mut areas = [0.0f32; MetaphysicalRoot::ALL.len()];
        for religion in &self.religions {
            areas[religion.faith.root.index()] += religion.area_km2;
        }
        areas
    }

    /// A religion by its 1-based id.
    #[inline(always)]
    pub fn religion(&self, id: u16) -> Option<&Religion> {
        if id == 0 {
            return None;
        }
        self.religions.get(id as usize - 1)
    }

    /// The religion one province keeps, if any.
    #[inline(always)]
    pub fn religion_of_province(&self, province_id: u32) -> Option<&Religion> {
        let id = self
            .province_religion
            .get(province_id as usize)
            .copied()
            .unwrap_or(0);
        self.religion(id)
    }

    /// The colour one province wears on the Religions map. Wilderness falls back
    /// to neutral grey, exactly as [`color_of`](Self::color_of) does.
    #[inline(always)]
    pub fn religion_color_of_province(&self, province_id: u32) -> [u8; 3] {
        self.religion_of_province(province_id)
            .map_or([128, 128, 128], |religion| religion.color)
    }

    /// A realm's religious census, by 1-based civilization id.
    #[inline(always)]
    pub fn religion_of_realm(&self, civ_id: u32) -> Option<&RealmReligion> {
        if civ_id == 0 {
            return None;
        }
        self.religion_of_realm.get(civ_id as usize - 1)
    }

    /// The faith most of a realm's people keep.
    pub fn dominant_religion_of(&self, civ_id: u32) -> Option<&Religion> {
        let census = self.religion_of_realm(civ_id)?;
        self.religion(census.dominant)
    }

    /// Every religion sorted by how many people keep it, largest first.
    pub fn religions_by_size(&self) -> Vec<&Religion> {
        let mut religions: Vec<&Religion> = self.religions.iter().collect();
        religions.sort_by(|a, b| {
            b.population
                .cmp(&a.population)
                .then(b.provinces.len().cmp(&a.provinces.len()))
                .then(a.origin_realm.cmp(&b.origin_realm))
        });
        religions
    }

    /// A civilization by id (1-based), if it exists.
    pub fn civilization(&self, id: u32) -> Option<&Civilization> {
        if id == 0 {
            return None;
        }
        self.civilizations.get(id as usize - 1)
    }

    /// Display name of a civilization id, or `unclaimed wilderness`.
    pub fn name_of(&self, id: u32) -> &str {
        self.civilization(id)
            .map_or("unclaimed", |c| c.name.as_str())
    }

    /// Colour of a civilization id; wilderness falls back to neutral grey.
    #[inline(always)]
    pub fn color_of(&self, id: u32) -> [u8; 3] {
        match self.civilization(id) {
            Some(civ) => civ.color,
            None => [128, 128, 128],
        }
    }

    /// `Year 1 200` — the date the atlas portrays, in the world's calendar.
    pub fn map_year_label(&self) -> String {
        self.timeline.year(self.map_year)
    }

    /// The age the world stands in at the atlas date.
    pub fn age(&self) -> Age {
        self.timeline.age
    }

    /// `The Age of Copper & Alloys` — the world's age, named.
    pub fn age_label(&self) -> &'static str {
        self.timeline.age.label()
    }

    /// Mean population density across every civilized province, per km².
    pub fn mean_density_per_km2(&self) -> f32 {
        if self.civilized_area_km2 <= 0.0 {
            0.0
        } else {
            self.total_population as f32 / self.civilized_area_km2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The painter's own water colours, which no political fill may resemble: this
    /// is the bug the palette rework exists to fix.
    const SEA: [u8; 3] = [16, 48, 88];
    const SHELF: [u8; 3] = [45, 125, 185];
    const LAKE: [u8; 3] = [42, 125, 185];
    const ICE: [u8; 3] = [227, 236, 247];

    /// Five tiers, five clearly different hues — a reader must be able to tell a
    /// chiefdom from a herd people without consulting the key.
    #[test]
    fn tiers_are_pairwise_distinguishable() {
        for a in 0..SocietyTier::LADDER.len() {
            for b in a + 1..SocietyTier::LADDER.len() {
                let one = SocietyTier::LADDER[a];
                let other = SocietyTier::LADDER[b];
                let distance = delta_e(one.legend_color(), other.legend_color());
                assert!(
                    distance > 24.0,
                    "{} and {} are only {distance:.1} apart",
                    one.label(),
                    other.label()
                );
            }
        }
    }

    /// No shade of any tier can be mistaken for the sea, a shelf, a lake or ice —
    /// the old state blue sat one perceptual step from the shelf colour, so every
    /// coastal realm disappeared into the water.
    #[test]
    fn no_tier_is_confusable_with_water() {
        for tier in SocietyTier::LADDER {
            for shade in 0..SocietyTier::SHADES {
                let colour = tier.shade(shade);
                for (name, water) in [("sea", SEA), ("shelf", SHELF), ("lake", LAKE)] {
                    let distance = delta_e(colour, water);
                    assert!(
                        distance > 25.0,
                        "{} shade {shade} is {distance:.1} from the {name}",
                        tier.label()
                    );
                }
                let distance = delta_e(colour, ICE);
                assert!(
                    distance > 15.0,
                    "{} shade {shade} is {distance:.1} from polar ice",
                    tier.label()
                );
            }
        }
    }

    /// The ladder darkens with organisation, so the map ranks its own societies: the
    /// states are the strongest ink and the bands recede into the ground.
    #[test]
    fn the_ladder_darkens_with_organisation() {
        let mut previous = f32::MAX;
        for tier in SocietyTier::LADDER {
            let lightness = lab(tier.legend_color())[0];
            assert!(
                lightness < previous,
                "{} is not darker than the tier below it",
                tier.label()
            );
            previous = lightness;
        }
    }

    /// Every shade stays inside its tier's family — a farming people never looks
    /// like a state — while differing enough from its siblings to read as a people.
    #[test]
    fn shades_stay_in_their_tier_family() {
        for tier in SocietyTier::LADDER {
            let base = tier.legend_color();
            let (hue, _, value) = rgb_to_hsv(base);
            let mut distinct = Vec::new();
            for shade in 0..SocietyTier::SHADES {
                let colour = tier.shade(shade);
                let (shade_hue, _, shade_value) = rgb_to_hsv(colour);
                let drift = (shade_hue - hue).abs();
                let drift = drift.min(360.0 - drift);
                assert!(
                    drift <= 12.0,
                    "{} shade {shade} left its hue family by {drift:.1}°",
                    tier.label()
                );
                let value_drift = (shade_value - value).abs();
                assert!(
                    value_drift <= 0.08,
                    "{} shade {shade} left its value family by {value_drift:.3}",
                    tier.label()
                );
                if !distinct.contains(&colour) {
                    distinct.push(colour);
                }
            }
            assert_eq!(
                distinct.len(),
                SocietyTier::SHADES,
                "{} repeated a shade",
                tier.label()
            );
        }
    }

    /// Nine metaphysical roots, nine clearly different hue families — a reader
    /// must be able to tell a monotheism from a nontheism without consulting the
    /// key.
    #[test]
    fn the_metaphysical_roots_are_pairwise_distinguishable() {
        for a in 0..MetaphysicalRoot::ALL.len() {
            for b in a + 1..MetaphysicalRoot::ALL.len() {
                let one = MetaphysicalRoot::ALL[a];
                let other = MetaphysicalRoot::ALL[b];
                let distance = delta_e(one.legend_color(), other.legend_color());
                assert!(
                    distance > 24.0,
                    "{} and {} are only {distance:.1} apart",
                    one.label(),
                    other.label()
                );
            }
        }
    }

    /// No shade of any root can be mistaken for the sea, a shelf, a lake or ice:
    /// a religion that vanishes into the water is a religion the map lost.
    #[test]
    fn no_religion_is_confusable_with_water() {
        for root in MetaphysicalRoot::ALL {
            for shade in 0..MetaphysicalRoot::SHADES {
                let colour = root.shade(shade);
                for (name, water) in [("sea", SEA), ("shelf", SHELF), ("lake", LAKE)] {
                    let distance = delta_e(colour, water);
                    assert!(
                        distance > 25.0,
                        "{} shade {shade} is {distance:.1} from the {name}",
                        root.label()
                    );
                }
                let distance = delta_e(colour, ICE);
                assert!(
                    distance > 15.0,
                    "{} shade {shade} is {distance:.1} from polar ice",
                    root.label()
                );
            }
        }
    }

    /// The greedy colouring hands neighbouring religions consecutive shade
    /// indices, so the family is walked contrast-first: the first two shades a
    /// pair of neighbours can receive must be visibly different, not two adjacent
    /// steps of a ramp. A grey root has only lightness to walk, so its steps are
    /// smaller — it is the one family where the first two tones are close.
    #[test]
    fn consecutive_root_shades_are_far_apart() {
        for root in MetaphysicalRoot::ALL {
            let first = root.shade(0);
            let second = root.shade(1);
            let distance = delta_e(first, second);
            let (_, saturation, _) = rgb_to_hsv(root.legend_color());
            let wanted = if saturation < 0.30 { 2.0 } else { 9.0 };
            assert!(
                distance > wanted,
                "{}'s first two shades are only {distance:.1} apart",
                root.label()
            );
        }
    }

    /// Every religion stays inside its root's colour family: nine roots read as
    /// nine kinds, and the shades inside one kind read as its religions. The hue
    /// bound is a little wider than a society tier's, because a religion family
    /// leans twelve degrees away from the water to separate ten tones; a grey
    /// root has no hue to lean at all, so only its value is checked.
    #[test]
    fn religion_shades_stay_in_their_root_family() {
        for root in MetaphysicalRoot::ALL {
            let base = root.legend_color();
            let (hue, _, value) = rgb_to_hsv(base);
            let mut distinct = Vec::new();
            for shade in 0..MetaphysicalRoot::SHADES {
                let colour = root.shade(shade);
                let (shade_hue, _, shade_value) = rgb_to_hsv(colour);
                let drift = (shade_hue - hue).abs();
                let drift = drift.min(360.0 - drift);
                assert!(
                    drift <= 14.0,
                    "{} shade {shade} left its hue family by {drift:.1}°",
                    root.label()
                );
                let value_drift = (shade_value - value).abs();
                assert!(
                    value_drift <= 0.27,
                    "{} shade {shade} left its value family by {value_drift:.3}",
                    root.label()
                );
                if !distinct.contains(&colour) {
                    distinct.push(colour);
                }
            }
            assert_eq!(
                distinct.len(),
                MetaphysicalRoot::SHADES,
                "{} repeated a shade",
                root.label()
            );
        }
    }
}
