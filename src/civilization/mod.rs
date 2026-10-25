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
//! a year a few centuries into the life of whoever held it.
//!
//! The timeline has no beginning. Years before the epoch are counted backwards as
//! BCE, and the depth of prehistory before the first city is a separate, much
//! larger quantity drawn per world and biased by how kind the planet turned out to
//! be — there is no fixed span between the first people and the first hearth. The
//! atlas then portrays the world at a single date (see [`CivilizationWorld::map_year`]),
//! at which every realm has an age, an era, a culture, a faith and a population. See
//! [`epoch`] for the whole model.
//!
//! ## Determinism
//!
//! Everything here is drawn from the world seed through a `ChaCha8Rng` seeded
//! with a fixed salt, and every collection that the RNG walks is a `Vec` sorted
//! by province id or a `BTreeMap`. The same seed therefore always produces the
//! same civilizations, on the same provinces, with the same dates and
//! populations.

pub mod config;
pub mod epoch;
pub mod generator;
pub mod report;

pub use config::CivilizationConfig;
#[allow(unused_imports)]
pub use epoch::{Epoch, Timeline};
pub use generator::generate_civilizations;
pub use report::report_civilizations;

/// Palette of the dawn civilizations.
///
/// Unlike the geopolitical tiers — which are generated from a golden-ratio hue
/// walk because there are thirty thousand of them — there are at most ten
/// civilizations, and they are the *subject* of their map mode. They therefore
/// get ten hand-picked, mutually distinguishable earth tones (ochre, lapis, jade,
/// terracotta…) so that no two realms can ever be confused on a small plate.
pub const CIVILIZATION_COLORS: [[u8; 3]; 10] = [
    [188, 74, 58],   // terracotta — fired brick and flood silt
    [66, 108, 168],  // lapis — river-borne trade and deep water
    [186, 144, 58],  // ochre — dry grain country
    [88, 148, 96],   // jade — forest and terrace
    [150, 100, 168], // amethyst — highland oracle country
    [206, 132, 92],  // copper — smelted metal and desert light
    [70, 152, 150],  // turquoise — island and strait
    [166, 166, 92],  // olive — steppe and thorn scrub
    [196, 100, 140], // rose — savanna blossom
    [120, 122, 176], // slate violet — cold northern shore
];

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
        }
    }
}

/// How a civilization organises power between people.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kinship {
    Egalitarian,
    ClanChiefdoms,
    TempleBureaucracy,
    MerchantOligarchy,
    WarriorAristocracy,
    DivineMonarchy,
}

impl Kinship {
    pub fn label(self) -> &'static str {
        match self {
            Kinship::Egalitarian => "Egalitarian villages",
            Kinship::ClanChiefdoms => "Clan chiefdoms",
            Kinship::TempleBureaucracy => "Temple bureaucracy",
            Kinship::MerchantOligarchy => "Merchant oligarchy",
            Kinship::WarriorAristocracy => "Warrior aristocracy",
            Kinship::DivineMonarchy => "Divine monarchy",
        }
    }

    /// The density bias a social order lends to population: a canal state can
    /// feed far more mouths per hectare than a village council can.
    pub fn density_bias(self) -> f32 {
        match self {
            Kinship::Egalitarian => 0.00,
            Kinship::ClanChiefdoms => 0.04,
            Kinship::TempleBureaucracy => 0.26,
            Kinship::MerchantOligarchy => 0.18,
            Kinship::WarriorAristocracy => 0.10,
            Kinship::DivineMonarchy => 0.22,
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

// ─── WHAT A CIVILIZATION MAKES, WRITES AND BELIEVES ──────────────────────────

/// Material culture — what a civilization makes its tools and weapons from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CraftTier {
    StoneAndBone,
    NativeCopper,
    CopperAlloys,
    SmeltedIron,
}

impl CraftTier {
    pub fn label(self) -> &'static str {
        match self {
            CraftTier::StoneAndBone => "Chipped stone and bone",
            CraftTier::NativeCopper => "Native copper and gold",
            CraftTier::CopperAlloys => "Copper alloys",
            CraftTier::SmeltedIron => "Smelted iron",
        }
    }

    pub fn hallmark(self) -> &'static str {
        match self {
            CraftTier::StoneAndBone => "knapped stone blades",
            CraftTier::NativeCopper => "cold-hammered copper",
            CraftTier::CopperAlloys => "cast copper-alloy work",
            CraftTier::SmeltedIron => "the bloomery furnace",
        }
    }
}

/// How a civilization keeps records.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Literacy {
    OralTradition,
    GlyphTallies,
    LogographicScript,
    AlphabeticScript,
}

impl Literacy {
    pub fn label(self) -> &'static str {
        match self {
            Literacy::OralTradition => "Oral tradition",
            Literacy::GlyphTallies => "Glyphs and tally marks",
            Literacy::LogographicScript => "Logographic script",
            Literacy::AlphabeticScript => "Alphabetic script",
        }
    }

    pub fn hallmark(self) -> &'static str {
        match self {
            Literacy::OralTradition => "the memorised lineage chant",
            Literacy::GlyphTallies => "the tally mark",
            Literacy::LogographicScript => "the glyph script",
            Literacy::AlphabeticScript => "the alphabet",
        }
    }
}

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

/// How old a civilization is at the date the atlas portrays.
///
/// Ages are counted from a realm's own founding year, so two realms alive on the
/// same map can be centuries apart in development — exactly as Sumer and the
/// Olmec were.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Era {
    FirstSettlements,
    FirstCities,
    CityStates,
    Empires,
}

impl Era {
    /// The era reached by a realm `age_years` old.
    pub fn from_age(age_years: u32) -> Self {
        match age_years {
            0..=199 => Era::FirstSettlements,
            200..=799 => Era::FirstCities,
            800..=1_699 => Era::CityStates,
            _ => Era::Empires,
        }
    }

    /// The era reached by a realm `age_years` old on a world developing at `pace`.
    ///
    /// The thresholds above are the ordinary case; a world that developed quickly
    /// (`pace > 1`) reaches its later ages sooner, and a slow one later. Without
    /// this a very slow world would still be building its first granaries at the
    /// map date, and a very fast one would leap straight to empire.
    pub fn from_age_scaled(age_years: u32, pace: f32) -> Self {
        Self::from_age((age_years as f32 / pace.max(0.05)) as u32)
    }

    pub fn label(self) -> &'static str {
        match self {
            Era::FirstSettlements => "Age of First Settlements",
            Era::FirstCities => "Age of First Cities",
            Era::CityStates => "Age of City-States",
            Era::Empires => "Age of Empires",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Era::FirstSettlements => "permanent villages, the first granaries, no writing yet",
            Era::FirstCities => "walled towns, temple workshops, the first scribes",
            Era::CityStates => "competing city-states, standing temples, long-distance trade",
            Era::Empires => "one capital ruling many cities, professional armies, archives",
        }
    }

    /// Development multiplier a realm of this era lends to its population.
    pub fn density_bias(self) -> f32 {
        match self {
            Era::FirstSettlements => 0.00,
            Era::FirstCities => 0.10,
            Era::CityStates => 0.20,
            Era::Empires => 0.32,
        }
    }
}

/// Whether a civilization rose on its own, beside a rival, or inside a crowd.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Isolation {
    /// No realm within reach: the whole culture is its own invention.
    IsolatedCradle,
    /// Known to others, but separated by sea, desert or simple distance.
    NeighbouringCradle,
    /// Shares a land frontier with at least one other realm.
    IntegratedHeartland,
}

impl Isolation {
    pub fn label(self) -> &'static str {
        match self {
            Isolation::IsolatedCradle => "Isolated rise",
            Isolation::NeighbouringCradle => "Distant neighbours",
            Isolation::IntegratedHeartland => "Contested heartland",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Isolation::IsolatedCradle => {
                "Rose entirely alone: from the seed grain to the temple plan, everything had to be invented on the spot."
            }
            Isolation::NeighbouringCradle => {
                "Rose in sight of another world but across water or dry land, so ideas arrived second-hand and late."
            }
            Isolation::IntegratedHeartland => {
                "Rose in a crowded region: it shared a border, a river and a border war with an older realm from the very beginning."
            }
        }
    }
}

/// The scale of a civilization, measured against our own history.
///
/// The class is *derived from the finished territory* rather than assigned in
/// advance: a realm that aimed at empire size and then ran into a mountain wall
/// or an older neighbour reports the size it actually reached.
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

    pub fn label(self) -> &'static str {
        match self {
            ScaleClass::Small => "Small realm",
            ScaleClass::Medium => "Middling realm",
            ScaleClass::Large => "Large realm",
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
    pub kinship: Kinship,
    pub outlook: Outlook,
    pub craft: CraftTier,
    pub literacy: Literacy,
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

    pub cradle_kind: CradleKind,
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
    pub founding_year: i32,
    /// Years elapsed between founding and the map date.
    pub age_years: u32,
    /// The era that age puts the realm in.
    pub era: Era,

    pub isolation: Isolation,
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
    /// Province id → population at the map date (0 = not civilized).
    pub province_population: Vec<u64>,
    /// Sum of every civilized province's population.
    pub total_population: u64,
    /// Number of provinces held by a civilization.
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

    /// Mean population density across every civilized province, per km².
    pub fn mean_density_per_km2(&self) -> f32 {
        if self.civilized_area_km2 <= 0.0 {
            0.0
        } else {
            self.total_population as f32 / self.civilized_area_km2
        }
    }
}
