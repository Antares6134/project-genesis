//! Stage 7 — the dawn of history: the civilizations that rise on a finished planet.
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
//! better than its neighbours, and the reason is reported to the reader verbatim
//! (see [`RiseProfile::reasons`]).
//!
//! ## The calendar
//!
//! There is no calendar to inherit on a planet nobody has inhabited yet, so this
//! world fixes a convention of its own — the plainest one there is: years of the
//! **Common Era**, counted from a single important event in its history. The event
//! is deliberately undefined, and it is never a civilization's founding — a calendar
//! cannot take its epoch from a polity that only exists because somebody later
//! decided to count years. Instead the epoch is a *place* and a *year*, both drawn
//! from the seed: a province chosen for the shape a founding story takes (a
//! crossroads, a lone peak, a lake, an island — never a cradle, never a throne), and
//! a year placed a few centuries to a couple of millennia *after* the atlas date.
//!
//! The timeline has no beginning. Years before the epoch are counted backwards as
//! BCE — and because the atlas portrays a world in its dawn, that is every date the
//! map shows, exactly as a map of our own 2000 BCE is dated in the years before the
//! Common Era. The depth of prehistory before the first city is a separate, much
//! larger quantity drawn per world and biased by how kind the planet turned out to
//! be — there is no fixed span between the first people and the first hearth. The
//! atlas then portrays the world at a single date (see [`CivilizationWorld::map_year`]),
//! at which the world stands in an age and every realm has a government, a culture,
//! a faith and a population. See [`epoch`] for the whole model.
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
pub mod generator;
pub mod report;

pub use age::Age;
pub use config::CivilizationConfig;
#[allow(unused_imports)]
pub use epoch::{Epoch, Timeline};
pub use generator::generate_civilizations;
pub use report::{report_civilizations, report_civilizations_all};

// ─── WHY A CIVILIZATION ROSE WHERE IT DID ────────────────────────────────────

/// The physical situation that made a cradle worth settling.
///
/// This is the single most important category in the dossier: it drives the
/// economy, the social order, the faith and the architecture that follow, and it
/// is the first line a reader sees when hovering a rising civilization.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
            CradleKind::RiverFloodplain => "River floodplain",
            CradleKind::RiverDelta => "River delta",
            CradleKind::Lacustrine => "Lakeshore",
            CradleKind::CoastalMaritime => "Open coast",
            CradleKind::HighlandValley => "Highland valley",
            CradleKind::DesertOasis => "Desert oasis",
            CradleKind::SteppeGrassland => "Steppe grassland",
            CradleKind::SavannaHeartland => "Savanna heartland",
            CradleKind::RainforestClearing => "Rainforest clearing",
            CradleKind::WoodlandHeartland => "Woodland heartland",
            CradleKind::IslandShore => "Island shore",
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

    /// One sentence on how such a civilization makes its living.
    pub fn describe(self) -> &'static str {
        match self {
            CradleKind::RiverFloodplain => "the river rebuilds the soil every year, so nobody has to move and nobody can leave",
            CradleKind::RiverDelta => "silt, fish and salt water meet in one place, so farming and shipping grow up in the same village",
            CradleKind::Lacustrine => "the lake gives fish, reeds, fresh water and a road that carries a canoe in every direction",
            CradleKind::CoastalMaritime => "there is no great river, but the sea feeds a shore that can sail rather than plough",
            CradleKind::HighlandValley => "rock walls keep the neighbours out and springs keep the valley green",
            CradleKind::DesertOasis => "one reliable spring, river reach or shore is the whole world, and whoever holds it holds the roads",
            CradleKind::SteppeGrassland => "too dry for a plough, rich enough for herds: wealth walks, and so does the people who own it",
            CradleKind::SavannaHeartland => "rain comes in one season, so grain, herds and game can all be husbanded in one country",
            CradleKind::RainforestClearing => "the forest gives everything except open ground, so gardens are cut and abandoned as the soil runs out",
            CradleKind::WoodlandHeartland => "rain, timber and soil all arrive together, so a village can clear a field, build a house and hunt the same woodlot",
            CradleKind::IslandShore => "water is the only road, so every household is a boat and every coast is a frontier",
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
            Environment::IceMargin => "Ice margin",
            Environment::Tundra => "Tundra",
            Environment::Taiga => "Taiga",
            Environment::Woodland => "Woodland",
            Environment::Steppe => "Steppe",
            Environment::Savanna => "Savanna",
            Environment::Desert => "Desert",
            Environment::TropicalForest => "Tropical forest",
            Environment::Highland => "Highland",
            Environment::IslandShore => "Island shore",
            Environment::Lacustrine => "Lakeshore",
            Environment::Floodplain => "River floodplain",
            Environment::Delta => "River delta",
            Environment::Coast => "Open coast",
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

    /// One sentence on what living here is like.
    pub fn describe(self) -> &'static str {
        match self {
            Environment::IceMargin => "ice lies on the ground all year and only the sea's edge gives a living",
            Environment::Tundra => "nothing grows tall enough to hide a man, so the summer herd is the whole harvest",
            Environment::Taiga => "cold conifer forest and bog, where the rivers are the only roads",
            Environment::Woodland => "rain, timber and soil arrive together, so a household can hunt, forage and clear a field in one country",
            Environment::Steppe => "too dry for a plough, rich enough for herds: wealth walks, and so does the people who own it",
            Environment::Savanna => "rain comes in one season, so grain, herds and game can all be husbanded in one country",
            Environment::Desert => "rain almost never falls, so every crossing is measured in wells",
            Environment::TropicalForest => "the forest gives everything except open ground, so gardens are cut and abandoned as the soil runs out",
            Environment::Highland => "rock walls keep the neighbours out and springs keep the valley green",
            Environment::IslandShore => "water is the only road, so every household is a boat and every coast is a frontier",
            Environment::Lacustrine => "the lake gives fish, reeds, fresh water and a road that carries a canoe in every direction",
            Environment::Floodplain => "the river rebuilds the soil every year, so nobody has to move and nobody can leave",
            Environment::Delta => "silt, fish and salt water meet in one place, so farming and shipping grow up in the same village",
            Environment::Coast => "there is no great river, but the sea feeds a shore that can sail rather than plough",
        }
    }
}

// ─── HOW A CIVILIZATION LIVES ────────────────────────────────────────────────

/// How a civilization feeds itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
            Subsistence::IrrigatedAgriculture => "Irrigated agriculture",
            Subsistence::DrylandFarming => "Dryland farming",
            Subsistence::Pastoralism => "Herds and transhumance",
            Subsistence::FishingAndSeafaring => "Fishing and seafaring",
            Subsistence::ShiftingCultivation => "Shifting cultivation",
            Subsistence::TerraceFarming => "Terrace farming",
            Subsistence::Foraging => "Hunting and gathering",
        }
    }

    /// Short noun used when one civilization borrows another's way of life.
    pub fn hallmark(self) -> &'static str {
        match self {
            Subsistence::IrrigatedAgriculture => "flood-plain irrigation",
            Subsistence::DrylandFarming => "rain-fed field farming",
            Subsistence::Pastoralism => "herd transhumance",
            Subsistence::FishingAndSeafaring => "the fishing boat",
            Subsistence::ShiftingCultivation => "the forest garden plot",
            Subsistence::TerraceFarming => "the terraced slope",
            Subsistence::Foraging => "the hunting ground",
        }
    }

    /// One sentence on the economy, quoted in the dossier.
    pub fn describe(self) -> &'static str {
        match self {
            Subsistence::IrrigatedAgriculture => "Canals carry the river into fields that need no rain, so the harvest is a public work and the state is its foreman.",
            Subsistence::DrylandFarming => "Fields drink only what the sky gives them, so villages hedge their luck across several soils and several gods.",
            Subsistence::Pastoralism => "Wealth is counted in animals and measured in pasture, which makes the people mobile and the horizon a country.",
            Subsistence::FishingAndSeafaring => "The catch is a daily tax on the water and the boat is the family's capital, so skill outranks land.",
            Subsistence::ShiftingCultivation => "Forest soil gives two or three harvests and then rests for a generation, so the village moves on a slow, deliberate cycle.",
            Subsistence::TerraceFarming => "Every field is a wall of stone built up the valley side, and every wall belongs to somebody's grandfather.",
            Subsistence::Foraging => "Nothing is planted and nothing is herded: the country is read rather than worked, and a band knows one valley the way a farmer knows one field.",
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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
    let saturation = if max <= f32::EPSILON { 0.0 } else { chroma / max };
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
            SocietyTier::HunterGatherers => "Hunter-gatherers",
            SocietyTier::NomadicPastoralists => "Nomadic pastoralists",
            SocietyTier::SimpleFarmers => "Simple farming societies",
            SocietyTier::ComplexFarmers => "Complex farming societies",
            SocietyTier::States => "State societies",
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

    /// One sentence on how such a people lives.
    pub fn describe(self) -> &'static str {
        match self {
            SocietyTier::HunterGatherers => {
                "A country of bands: a few families read one valley, move with the season, and answer to nobody but their own elders."
            }
            SocietyTier::NomadicPastoralists => {
                "A country of herders: wealth is counted in animals and measured in pasture, so the people move and the horizon is their border."
            }
            SocietyTier::SimpleFarmers => {
                "A country of villages: fields are cleared and kept, the household is the unit of work, and no rank stands above it."
            }
            SocietyTier::ComplexFarmers => {
                "A ranked farming country: chiefs and lineages hold the land, tribute flows upward, and a great man is buried with more than he could use."
            }
            SocietyTier::States => {
                "A state: offices, archives and a standing apparatus, which can tax a harvest it never touched and raise a wall it will not live to see finished."
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
            Government::Band => "Bands",
            Government::Tribe => "Tribal councils",
            Government::Chiefdom => "Chiefdom",
            Government::DivineMonarchy => "Divine monarchy",
            Government::TheocraticRepublic => "Theocratic republic",
            Government::AristocraticRepublic => "Aristocratic republic",
            Government::PlutocraticRepublic => "Plutocratic republic",
            Government::Oligarchy => "Oligarchy",
            Government::Tyranny => "Tyranny",
        }
    }

    /// True for a government with a centralised state apparatus behind it, false
    /// for the kinship societies that came before the state.
    pub fn is_state(self) -> bool {
        !matches!(
            self,
            Government::Band | Government::Tribe | Government::Chiefdom
        )
    }

    /// One sentence on how this society is governed.
    pub fn describe(self) -> &'static str {
        match self {
            Government::Band => {
                "A kinship band: no ranks and no offices, every decision taken by consent around a fire."
            }
            Government::Tribe => {
                "A tribal society: councils of elders and lineages settle its matters, and no state stands above them."
            }
            Government::Chiefdom => {
                "A chiefdom: a ranked society under a hereditary chief, still bound together by kinship."
            }
            Government::DivineMonarchy => {
                "A divine monarchy: the king is the state's cult as well as its head, and law descends from his person."
            }
            Government::TheocraticRepublic => {
                "A theocratic republic: priests hold the offices and the temple is the administration."
            }
            Government::AristocraticRepublic => {
                "An aristocratic republic: a landed elite governs through councils and magistrates it fills itself."
            }
            Government::PlutocraticRepublic => {
                "A plutocratic republic: wealth, not birth, buys the offices, and the merchants hold the state."
            }
            Government::Oligarchy => {
                "An oligarchy: a narrow clique holds power and keeps the door shut behind it."
            }
            Government::Tyranny => {
                "A tyranny: one man rules against the law, and holds on to the state by force."
            }
        }
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

    /// Short noun used when one civilization borrows another's way of governing.
    pub fn hallmark(self) -> &'static str {
        match self {
            Government::Band => "the kinship band",
            Government::Tribe => "the council of elders",
            Government::Chiefdom => "the chief's court",
            Government::DivineMonarchy => "the divine kingship",
            Government::TheocraticRepublic => "the temple rule",
            Government::AristocraticRepublic => "the aristocratic council",
            Government::PlutocraticRepublic => "the merchants' assembly",
            Government::Oligarchy => "the ruling clique",
            Government::Tyranny => "the tyrant's court",
        }
    }
}

/// What a civilization values — its public character, and the lens through which
/// it treats its neighbours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outlook {
    Agrarian,
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
            Outlook::Agrarian => "Agrarian",
            Outlook::Mercantile => "Mercantile",
            Outlook::Martial => "Martial",
            Outlook::Scholarly => "Scholarly",
            Outlook::Ceremonial => "Ceremonial",
            Outlook::Artistic => "Artistic",
            Outlook::Isolationist => "Isolationist",
        }
    }

    /// How the realm is likely to treat a neighbour it can reach.
    pub fn disposition(self) -> &'static str {
        match self {
            Outlook::Agrarian => "prefers a quiet border and a full granary",
            Outlook::Mercantile => "wants the road open and the tariff agreed",
            Outlook::Martial => "counts the neighbour's cattle as its own",
            Outlook::Scholarly => "sends scribes before soldiers",
            Outlook::Ceremonial => "cares who holds the holy place, not where the border runs",
            Outlook::Artistic => "trades styles and songs more readily than land",
            Outlook::Isolationist => "would rather the neighbour stayed a rumour",
        }
    }

    /// The population bias of an ethos at the moment of the map.
    pub fn density_bias(self) -> f32 {
        match self {
            Outlook::Agrarian => 0.06,
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
            Outlook::Mercantile => 1.25,
            Outlook::Artistic => 1.15,
            Outlook::Scholarly => 1.10,
            Outlook::Agrarian => 1.00,
            Outlook::Ceremonial => 0.95,
            Outlook::Martial => 0.85,
            Outlook::Isolationist => 0.45,
        }
    }
}

// ─── WHAT A CIVILIZATION BELIEVES ────────────────────────────────────────────

/// What a civilization believes, and who speaks for its gods.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaithKind {
    RiverFertility,
    Solar,
    SkyAndStorm,
    Ancestral,
    SeaAndTide,
    EarthMother,
    StarAndOmen,
}

impl FaithKind {
    pub fn label(self) -> &'static str {
        match self {
            FaithKind::RiverFertility => "River and fertility cult",
            FaithKind::Solar => "Sun cult",
            FaithKind::SkyAndStorm => "Sky and storm cult",
            FaithKind::Ancestral => "Ancestor veneration",
            FaithKind::SeaAndTide => "Sea and tide cult",
            FaithKind::EarthMother => "Earth-mother cult",
            FaithKind::StarAndOmen => "Star and omen lore",
        }
    }

    pub fn hallmark(self) -> &'static str {
        match self {
            FaithKind::RiverFertility => "the river rite",
            FaithKind::Solar => "the sun calendar",
            FaithKind::SkyAndStorm => "the storm offering",
            FaithKind::Ancestral => "the ancestor mask",
            FaithKind::SeaAndTide => "the tide blessing",
            FaithKind::EarthMother => "the harvest mother",
            FaithKind::StarAndOmen => "the omen reading",
        }
    }

    /// The heart of the creed, in one sentence.
    pub fn creed(self) -> &'static str {
        match self {
            FaithKind::RiverFertility => {
                "The river is the god: it dies every year and is born again, and so are the grain, the king and the people."
            }
            FaithKind::Solar => {
                "The sun is the only power worth counting on, and every calendar is a prayer addressed to it."
            }
            FaithKind::SkyAndStorm => {
                "The sky decides: rain is mercy, hail is judgement, and the mountain is where it speaks."
            }
            FaithKind::Ancestral => {
                "The dead are the true citizens; the living only hold their land in trust for them."
            }
            FaithKind::SeaAndTide => {
                "The tide is a debt that has to be settled every night, and no hull sails until it has been paid."
            }
            FaithKind::EarthMother => {
                "The earth is a mother who must be fed before she will feed anyone else."
            }
            FaithKind::StarAndOmen => {
                "Everything that will happen is already written in the sky, and the skilled can read it there."
            }
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// How two realms know about each other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContactKind {
    /// Their provinces touch across dry land.
    LandFrontier,
    /// Only water lies between their provinces.
    SeaRoute,
    /// Within reach of travellers, but not of armies.
    DistantRumour,
}

impl ContactKind {
    pub fn label(self) -> &'static str {
        match self {
            ContactKind::LandFrontier => "Land frontier",
            ContactKind::SeaRoute => "Sea route",
            ContactKind::DistantRumour => "Distant rumour",
        }
    }
}

// ─── THE DOSSIER ─────────────────────────────────────────────────────────────

/// Why this civilization rose *here*, in the reader's own words.
///
/// [`RiseProfile::reasons`] is the answer the hover tooltip gives: every bullet
/// is a real measurement of the seat province (habitability, river discharge,
/// mean temperature, annual rainfall, elevation, distance to the nearest other
/// cradle), so a reader can always audit why the world decided this spot was
/// worth a civilization.
#[derive(Clone, Debug)]
pub struct RiseProfile {
    pub kind: CradleKind,
    /// One line: the country the civilization was born in.
    pub headline: String,
    /// Bullet points: the measurements behind the choice.
    pub reasons: Vec<String>,
    /// Two to four *short* phrases a hover card can carry without covering the
    /// map: "A great river at its door", "A shore for fish and salt". These say
    /// the same thing as [`RiseProfile::reasons`] without the figures.
    pub highlights: Vec<String>,
    /// Where the seat sits in the political hierarchy.
    pub seat: String,
}

/// The material and social life of a civilization.
#[derive(Clone, Debug)]
pub struct Culture {
    pub subsistence: Subsistence,
    pub government: Government,
    pub outlook: Outlook,
    /// The form its monumental building takes.
    pub architecture: &'static str,
    /// Two or three arts the realm is known for.
    pub arts: Vec<&'static str>,
    /// A sentence or two of prose built from the fields above.
    pub note: String,
}

/// The faith of a civilization.
#[derive(Clone, Debug)]
pub struct Faith {
    pub kind: FaithKind,
    /// Who performs the rites.
    pub clergy: &'static str,
    /// Three observances a visitor would notice.
    pub practices: Vec<&'static str>,
    /// A sentence of prose built from the fields above.
    pub note: String,
}

/// A realm this civilization can reach.
#[derive(Clone, Debug)]
pub struct Contact {
    /// The other civilization's id.
    pub other: u32,
    pub kind: ContactKind,
    /// Nearest surface distance between the two territories, in kilometres.
    pub distance_km: f32,
    /// How they treat each other — trade, rivalry, tribute, rumour.
    pub relation: &'static str,
    /// A sentence putting the relationship in context.
    pub note: String,
}

/// Something a younger civilization had already taken up **before** it founded
/// its own first city.
///
/// This is the quiet half of prehistory: a culture can inherit a crop, a rite or
/// a script from an older neighbour centuries before it becomes a civilization in
/// its own right — the way the ancestors of Egypt and the Levant took up farming
/// and pottery from Mesopotamia long before Narmer.
#[derive(Clone, Debug)]
pub struct Influence {
    /// The older civilization that gave.
    pub other: u32,
    /// What crossed the frontier before the borrower existed, in the older
    /// realm's own terms (lower-cased so it reads inside a sentence).
    pub borrowed: Vec<String>,
    /// A sentence of prose built from the fields above.
    pub note: String,
}

/// One civilization, from its founding year to the date the atlas portrays.
///
/// A realm's *id* — the number used by [`Contact`], [`Influence`] and
/// [`CivilizationWorld::name_of`] — is its 1-based position in
/// [`CivilizationWorld::civilizations`], which is the founding order. There is
/// deliberately no separate field for it, so the two can never drift apart.
#[derive(Clone, Debug)]
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
    pub rise: RiseProfile,
    pub culture: Culture,
    pub faith: Faith,

    /// The province the realm was born in.
    pub seat_province: u32,
    /// The seat's position on the canvas, in pixels.
    pub seat_x: f32,
    pub seat_y: f32,
    /// Latitude of the seat, in degrees (negative = southern).
    pub seat_latitude_deg: f32,
    /// Longitude of the seat, in degrees east (negative = west of the prime meridian).
    pub seat_longitude_deg: f32,
    /// Every province the realm holds at the map date, ascending.
    pub provinces: Vec<u32>,
    /// Ground area of the realm, in km², integrated over latitude.
    pub area_km2: f32,
    /// Total population at the map date.
    pub population: u64,

    /// The year the realm was founded, written in the world's own era — negative
    /// for BCE, so a realm older than the epoch carries a BCE date. See
    /// [`epoch::ce_label`](crate::civilization::epoch::ce_label).
    ///
    /// `None` for a people rather than a polity: a band of foragers and a herd
    /// people have no founding year, because nobody founded them — they were
    /// already there. Dating them would repeat the error the dossier's whole
    /// rework exists to undo.
    pub founding_year: Option<i32>,
    /// Years elapsed between founding and the map date. Zero for a people.
    pub age_years: u32,

    /// Every realm it can reach, ascending by id.
    pub contacts: Vec<Contact>,
    /// Inheritances from older realms, ascending by id.
    pub influences: Vec<Influence>,
    /// Realms that share this one's ancestral stock (see
    /// [`Civilization::language_family`]), ascending by id.
    pub kin: Vec<u32>,
    /// The tongue family two kin realms share, e.g. `Riparian Tongue`.
    pub language_family: String,

    /// Latitude-aware population density of the realm at the map date.
    pub density_per_km2: f32,
}

impl Civilization {
    /// `Year 1 240` — the realm's own calendar at the map date.
    pub fn year_label(&self) -> String {
        format!("Year {}", crate::core::fmt::thousands(self.age_years as f32))
    }

    /// How the atlas opens a society's story: a founding date for a polity, a
    /// statement of prior occupation for a people.
    pub fn origin_label(&self) -> String {
        match self.founding_year {
            Some(year) => format!("Founded {}", epoch::ce_label(year)),
            None => "Older than any city: a people, not a polity".to_string(),
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
#[derive(Clone, Debug)]
pub struct CivilizationWorld {
    /// The seed the whole world was grown from.
    pub seed: u64,
    /// The date the atlas portrays, in the world's own era (negative = BCE). It is
    /// set a few centuries after the youngest realm began, so every realm on the
    /// map exists and none has to be extrapolated. See [`Timeline`].
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

    /// A civilization by id (1-based), if it exists.
    pub fn civilization(&self, id: u32) -> Option<&Civilization> {
        if id == 0 {
            return None;
        }
        self.civilizations.get(id as usize - 1)
    }

    /// Display name of a civilization id, or `unclaimed wilderness`.
    pub fn name_of(&self, id: u32) -> &str {
        self.civilization(id).map_or("unclaimed", |c| c.name.as_str())
    }

    /// Colour of a civilization id; wilderness falls back to neutral grey.
    #[inline(always)]
    pub fn color_of(&self, id: u32) -> [u8; 3] {
        match self.civilization(id) {
            Some(civ) => civ.color,
            None => [128, 128, 128],
        }
    }

    /// `The Common Era — reckoned from an important event at the Green Crossing`.
    ///
    /// The world's own calendar, in one line: the epoch is a convention counted
    /// from a single important event, never from a civilization's founding. See
    /// [`crate::civilization::epoch`].
    pub fn epoch_label(&self) -> String {
        self.timeline.epoch_label()
    }

    /// `300 CE` — the date the atlas portrays, in the world's own era.
    pub fn map_year_label(&self) -> String {
        epoch::ce_label(self.map_year)
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

    /// CIE L\*a\*b\* of an 8-bit sRGB colour, so contrast can be asserted in
    /// perceptual units rather than in channel differences.
    fn lab(colour: [u8; 3]) -> [f32; 3] {
        let linear = |channel: u8| {
            let value = channel as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (
            linear(colour[0]),
            linear(colour[1]),
            linear(colour[2]),
        );
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
}
