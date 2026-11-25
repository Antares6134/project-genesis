//! The world's calendar: Year 0 and the depth of prehistory.
//!
//! The first civilization the simulator raises fixes the epoch. The richest
//! cradle's first city is **Year 0**, and every later date is a plain count of
//! years from it — `Year 1 111`, never a BCE/CE pair and never a separately chosen
//! place or year. There is no "before" era for the atlas to show: what precedes
//! Year 0 is prehistory, reported as a depth of years rather than as dates.
//!
//! Two quantities are kept apart on purpose:
//!
//! * [`Timeline::prehistory_years`] — how long the world's peoples lived before the
//!   first city. It is wide-ranged and seeded, because a gentle world may reach its
//!   first city in five thousand years and a hard one may take a million.
//! * [`Timeline::atlas_year`] — how far the map's own date stands into the count.
//!
//! Nothing about the depth is Earth's own. There is no fixed three hundred thousand
//! years between the first people and the first city, and no assumption that a world
//! must have passed through an ice age: the depth of prehistory is a wide seeded
//! draw biased by how kind the planet turned out to be.
//!
//! [`Timeline::year`] and [`year_label`] are the single place an internal year is
//! turned into text. A later build that adopts a new epoch — a hegemon, a
//! federation, a faith — can anchor that conversion there without changing a call
//! site.

use crate::civilization::age::Age;
use crate::core::fmt::thousands;

/// The whole time axis of one world.
///
/// The internal frame is the world's calendar: the first city is Year 0, and a
/// realm founded two centuries later is founded in Year 200. The atlas reports
/// [`Timeline::atlas_label`]; nothing above or below the dawn stage ever sees a
/// BCE/CE pair.
///
/// The two quantities that genuinely differ between worlds are kept apart:
///
/// * [`Timeline::prehistory_years`] is the **depth of prehistory**: how long the
///   world's peoples lived before the first city, so the first people sit at
///   internal year `−prehistory_years`. It is wide-ranged and seeded, because a
///   gentle world may reach its first city in five thousand years and a hard one
///   may take a million.
/// * [`Timeline::atlas_year`] is where the map stands in that count — a few
///   centuries to a couple of millennia after the first city, never before it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Timeline {
    /// The depth of prehistory: years from the first people to the first city.
    pub prehistory_years: i32,
    /// The year the atlas portrays, in the world's calendar.
    pub atlas_year: i32,
    /// The age the world stands in at [`Timeline::atlas_year`]. A property of the
    /// world and its date, never of a single realm — two contemporaries, a river
    /// monarchy and a forest chiefdom, share it. See
    /// [`crate::civilization::age`].
    pub age: Age,
}

impl Timeline {
    /// A world where nothing was ever dated — the placeholder the atlas falls back
    /// on before the history stage has run.
    pub fn empty() -> Self {
        Self {
            prehistory_years: 0,
            atlas_year: 0,
            age: Age::Cultivation,
        }
    }

    /// `Year 1 800` — an internal year written in the world's own calendar.
    pub fn year(&self, absolute: i32) -> String {
        year_label(absolute)
    }

    /// `Year 300`, the date the atlas portrays.
    pub fn atlas_label(&self) -> String {
        self.year(self.atlas_year)
    }

    /// How far back the first people lie, in one line.
    ///
    /// This is the line that carries the depth of prehistory, and it is why a world
    /// is never limited to a few thousand years of history: the calendar begins at
    /// the first city, while the peoples it counts for go back as far as the world
    /// took to get going.
    pub fn prehistory_label(&self) -> String {
        format!(
            "The first people appeared {} years before the first city, which rose in Year 0",
            thousands(self.prehistory_years as f32),
        )
    }
}

/// A year of the world's calendar, written the way the atlas writes it: `Year 0`,
/// `Year 300`, `Year 1 111`.
///
/// Year 0 is the founding of the first city. There is no BCE/CE: nothing the atlas
/// displays predates the epoch, because what came before it is prehistory.
pub fn year_label(year: i32) -> String {
    format!("Year {}", thousands(year as f32))
}

// ─── HOW FAST THIS WORLD DEVELOPED ───────────────────────────────────────────

/// How quickly this world's peoples moved from the first hearth to a civilization.
///
/// A multiplier on the pace of development: `1.0` is an ordinary temperate world,
/// above it develops faster, below it slower. It is read from what the census
/// already measured — how kind the ground is, how much of the world freezes — so a
/// harsh or tidally-locked world genuinely takes longer without anything being
/// invented for the purpose.
#[derive(Clone, Copy, Debug)]
pub struct DevelopmentPace(f32);

impl DevelopmentPace {
    /// The multiplier itself; `> 1` develops faster.
    pub fn multiplier(self) -> f32 {
        self.0
    }

    /// Read the pace off the finished world.
    ///
    /// `mean_habitability` is the mean habitability of every inhabited province,
    /// `frost_share` the share of them whose mean temperature is below freezing,
    /// and `tidally_locked` whether the planet has no rotation — no day, no
    /// seasons, and one half frozen forever, which is slower ground to farm.
    pub fn from_measurements(
        mean_habitability: f32,
        frost_share: f32,
        tidally_locked: bool,
    ) -> Self {
        let mut pace = 0.55 + 1.15 * mean_habitability.clamp(0.0, 1.0);
        pace *= 1.0 - 0.35 * frost_share.clamp(0.0, 1.0);
        if tidally_locked {
            pace *= 0.70;
        }
        Self(pace.clamp(0.25, 2.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_labels_count_from_the_first_city() {
        assert_eq!(year_label(0), "Year 0");
        assert_eq!(year_label(300), "Year 300");
        assert_eq!(year_label(1_111), "Year 1 111");
        assert_eq!(year_label(2_100), "Year 2 100");
        // The same label a Timeline writes for an arbitrary internal year.
        assert_eq!(Timeline::empty().year(300), "Year 300");
    }

    #[test]
    fn timeline_reports_years_and_the_depth_of_prehistory() {
        let timeline = Timeline {
            prehistory_years: 40_000,
            atlas_year: 4_000,
            age: Age::CopperAndAlloys,
        };
        assert_eq!(timeline.atlas_label(), "Year 4 000");
        // The depth of prehistory is what keeps the timeline from being bounded.
        assert_eq!(
            timeline.prehistory_label(),
            "The first people appeared 40 000 years before the first city, which rose in Year 0"
        );
    }

    /// The pace rises as the ground gets kinder, and falls when it is locked.
    #[test]
    fn pace_reads_the_world() {
        let kind = DevelopmentPace::from_measurements(1.0, 0.0, false).multiplier();
        let harsh = DevelopmentPace::from_measurements(0.2, 0.6, false).multiplier();
        let locked = DevelopmentPace::from_measurements(1.0, 0.0, true).multiplier();
        assert!(kind > harsh);
        assert!(kind > locked);
        assert!(kind <= 2.0 && harsh >= 0.25);
    }
}
