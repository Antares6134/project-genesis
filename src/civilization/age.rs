//! The world's age: the technological stage the whole planet stands in at the
//! date the atlas portrays.
//!
//! An Age is a property of the **world and its date**, never of a single realm:
//! two contemporaries — a river monarchy and a forest chiefdom — live in the same
//! age, however differently they are governed. This mirrors the split the calendar
//! already makes (see [`crate::civilization::epoch`]): the epoch is the world's
//! measuring stick, and the age is where the world currently stands on it.
//!
//! The ladder ran on our own world as
//!
//! ```text
//!   Agrarian       Cultivation      Irrigation
//!   Metallurgical  Copper & Alloys  Refinement      Architecture
//!   Mechanical     Automated Mills  Oceanic Currents
//!   Industrial
//!   Information
//!   Molecular
//!   Planetary      (a Kardashev Type I civilization)
//! ```
//!
//! with the earliest ages given sub-ages because they lasted so much longer than
//! the later ones. The spans are *nominal* — the shape of the ladder, not a law:
//! a world's own pace of development (see
//! [`DevelopmentPace`](crate::civilization::epoch::DevelopmentPace)) stretches or
//! compresses them, so a gentle planet may cross an age in a few centuries and a
//! harsh one in a few thousand.
//!
//! This build simulates to the **Copper & Alloys** age and no further
//! ([`Age::HORIZON`]): the later ages are defined so the ladder is complete and
//! the code is ready for them, but no world reaches them yet.

/// The technological age a world stands in.
///
/// Ordered from oldest to newest, so `Age::min(Age::HORIZON)` caps a world at the
/// deepest age this build models.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Age {
    // ── The Agrarian age ─────────────────────────────────────────────────────
    /// The first fields: people plant and stay, but build no towns.
    Cultivation,
    /// Canals and works: the river is led to the fields and towns grow round it.
    Irrigation,
    // ── The Metallurgical age ────────────────────────────────────────────────
    /// Metal is cast, not merely hammered: copper alloyed with tin or arsenic.
    CopperAndAlloys,
    /// Alloys are mastered and worked to a finish.
    Refinement,
    /// Monumental building: the age of the great works.
    Architecture,
    // ── The Mechanical age ───────────────────────────────────────────────────
    /// Mills and gearing: labour begins to be taken from wind and water.
    AutomatedMills,
    /// Deep-water sailing: the world's oceans become roads.
    OceanicCurrents,
    // ── And on, to the horizon and beyond ────────────────────────────────────
    /// The industrial age: coal, steam and the factory.
    Industrial,
    /// The information age: computation and instant, global communication.
    Information,
    /// The molecular age: matter itself is engineered to order.
    Molecular,
    /// The planetary age: a world-spanning civilization — Kardashev Type I.
    Planetary,
}

impl Age {
    /// The deepest age this build simulates to. Worlds are capped here; the ages
    /// above it exist so the ladder — and the code that walks it — is complete.
    pub const HORIZON: Age = Age::CopperAndAlloys;

    /// The whole ladder, oldest first.
    pub const LADDER: [Age; 11] = [
        Age::Cultivation,
        Age::Irrigation,
        Age::CopperAndAlloys,
        Age::Refinement,
        Age::Architecture,
        Age::AutomatedMills,
        Age::OceanicCurrents,
        Age::Industrial,
        Age::Information,
        Age::Molecular,
        Age::Planetary,
    ];

    /// The age a world has reached, from how far its leading region has developed.
    ///
    /// `progress` is the frontier of the deep-time development pass — the most
    /// developed realm's level at the atlas date, normalised to `0..=1` (see
    /// `simulate_development` in [`crate::civilization::generator`]). The ladder's
    /// nominal [`span`](Self::span)s are read as a *share* of that progress, in
    /// the same order [`Age::LADDER`] keeps them, so a world at `1.0` stands at
    /// [`Age::HORIZON`] and a world at `0.0` has barely begun to cultivate.
    ///
    /// No world passes the horizon this build models, however developed it is.
    pub fn from_development(progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let mut boundary = 0.0f32;
        for age in Self::LADDER {
            if age > Self::HORIZON {
                break; // the ages above the horizon are defined, never reached
            }
            boundary += age.span();
            if progress * Self::reach() < boundary {
                return age;
            }
        }
        Self::HORIZON
    }

    /// The nominal years a world spends from its first field to the horizon.
    ///
    /// Only the ages this build simulates contribute: the ages above
    /// [`Age::HORIZON`] are not part of the span a frontier can measure.
    fn reach() -> f32 {
        Self::LADDER
            .iter()
            .take_while(|age| **age <= Self::HORIZON)
            .map(|age| age.span())
            .sum()
    }

    /// The nominal span of this age, in years of our own world, before a world's
    /// pace stretches or compresses it. `Planetary` is open-ended — it never ends.
    fn span(self) -> f32 {
        match self {
            Age::Cultivation => 1_400.0,
            Age::Irrigation => 2_400.0,
            Age::CopperAndAlloys => 2_000.0,
            Age::Refinement => 700.0,
            Age::Architecture => 1_600.0,
            Age::AutomatedMills => 350.0,
            Age::OceanicCurrents => 310.0,
            Age::Industrial => 190.0,
            Age::Information => 70.0,
            Age::Molecular => 130.0,
            Age::Planetary => f32::INFINITY,
        }
    }

    /// The parent age this one belongs to — `Cultivation` and `Irrigation` are
    /// both of the Agrarian age, and so on.
    pub fn stratum(self) -> &'static str {
        match self {
            Age::Cultivation | Age::Irrigation => "the Agrarian age",
            Age::CopperAndAlloys | Age::Refinement | Age::Architecture => "the Metallurgical age",
            Age::AutomatedMills | Age::OceanicCurrents => "the Mechanical age",
            Age::Industrial => "the Industrial age",
            Age::Information => "the Information age",
            Age::Molecular => "the Molecular age",
            Age::Planetary => "the Planetary age",
        }
    }

    /// The age, named the way the atlas writes it.
    pub fn label(self) -> &'static str {
        match self {
            Age::Cultivation => "The Age of Cultivation",
            Age::Irrigation => "The Age of Irrigation",
            Age::CopperAndAlloys => "The Age of Copper & Alloys",
            Age::Refinement => "The Age of Refinement",
            Age::Architecture => "The Age of Architecture",
            Age::AutomatedMills => "The Age of Automated Mills",
            Age::OceanicCurrents => "The Age of Oceanic Currents",
            Age::Industrial => "The Industrial Age",
            Age::Information => "The Information Age",
            Age::Molecular => "The Molecular Age",
            Age::Planetary => "The Planetary Age",
        }
    }

    /// One sentence on what the world is like in this age.
    pub fn describe(self) -> &'static str {
        match self {
            Age::Cultivation => {
                "the first fields are planted and people stop following the herds, though they build no towns yet"
            }
            Age::Irrigation => {
                "canals carry the river to the fields, and the first walled towns grow up around the works"
            }
            Age::CopperAndAlloys => {
                "metal is cast rather than merely hammered — copper alloyed with tin or arsenic — and the trade in it runs far"
            }
            Age::Refinement => {
                "alloys are mastered and worked to a finish, and the smith's craft becomes an art of the state"
            }
            Age::Architecture => "monumental building: the age of the great works in stone and brick",
            Age::AutomatedMills => "mills and gearing take the grinding of grain from wind and water",
            Age::OceanicCurrents => "deep-water sailing turns the world's oceans into roads",
            Age::Industrial => "coal, steam and the factory remake how everything is made",
            Age::Information => "computation and instant communication bind the world together",
            Age::Molecular => "matter itself is engineered to order",
            Age::Planetary => {
                "the whole planet is one civilization's country — a Kardashev Type I world"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world whose leading region is fully developed stands at the copper
    /// horizon, while one that has barely begun to cultivate is still agrarian.
    #[test]
    fn development_maps_onto_the_ladder() {
        assert_eq!(Age::from_development(1.0), Age::CopperAndAlloys);
        assert_eq!(Age::from_development(0.0), Age::Cultivation);
        assert_eq!(Age::from_development(0.10), Age::Cultivation);
        assert_eq!(Age::from_development(0.40), Age::Irrigation);
        assert_eq!(Age::from_development(0.80), Age::CopperAndAlloys);
        assert!(Age::from_development(0.10) < Age::from_development(0.40));
    }

    /// The frontier is monotone: a more developed world is never behind a less
    /// developed one, and every step of progress is some age on the ladder.
    #[test]
    fn a_more_developed_world_is_never_behind() {
        let mut previous = Age::from_development(0.0);
        for step in 0..=100 {
            let age = Age::from_development(step as f32 / 100.0);
            assert!(age >= previous, "age went backwards at {step}%");
            assert!(Age::LADDER.contains(&age));
            previous = age;
        }
    }

    /// However developed a world grows, it never passes the horizon this build
    /// models — and out-of-range frontiers are clamped, not trusted.
    #[test]
    fn no_world_passes_the_horizon() {
        assert_eq!(Age::from_development(7.5), Age::HORIZON);
        assert_eq!(Age::from_development(-3.0), Age::Cultivation);
        assert_eq!(Age::from_development(f32::INFINITY), Age::HORIZON);
    }

    /// The frontier never reaches the ages above the horizon.
    #[test]
    fn the_horizon_is_the_ceiling_of_the_development_ladder() {
        for age in Age::LADDER {
            if age > Age::HORIZON {
                assert!(Age::from_development(1.0) < age);
            }
        }
    }

    /// The ladder runs oldest to newest with no repeats.
    #[test]
    fn the_ladder_is_ordered_oldest_first() {
        for pair in Age::LADDER.windows(2) {
            assert!(pair[0] < pair[1], "ladder out of order at {:?}", pair[0]);
        }
    }
}
