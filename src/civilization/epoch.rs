//! The world's calendar: what its years are counted from, and how deep its
//! prehistory runs.
//!
//! A planet nobody has inhabited yet has no calendar to inherit, so the atlas
//! fixes a convention of its own — and borrows the plainest one there is: years of
//! the **Common Era**, counted from a single important event in the world's
//! history. That event is deliberately left undefined: the atlas never says what
//! happened, only that whatever it was, the world's peoples reckon their years
//! from it.
//!
//! Two decisions place the epoch, and both come out of the seed:
//!
//! * **the place** — one province, chosen from the whole world for the shape a
//!   founding story takes. It is never a cradle and never the seat of a realm: the
//!   epoch of a real calendar is not the capital of whoever was strongest, but a
//!   crossroads, a lone peak, a lake or an island whose story travelled further
//!   than any army could. Jerusalem, not Sumer.
//! * **the year** — placed a few centuries to a couple of millennia *after* the
//!   atlas date, so the world the map portrays stands in the years before its own
//!   calendar begins.
//!
//! The epoch is a *convention*, not a beginning — and, for the early ages this build
//! reaches, it lies in the world's own future. The atlas portrays a world in its
//! dawn, so every date it shows is BCE, exactly as a map of our own 2000 BCE is
//! dated in the years before the Common Era. As later builds carry the ladder past
//! this horizon, worlds will begin to stand after the event and read CE. Either way
//! the timeline has no beginning: it extends as far back as the world's prehistory
//! runs, and that depth is a separate, much larger quantity, drawn per world (see
//! [`Timeline::prehistory_years`]).
//!
//! Nothing about any of this is Earth's own. There is no fixed three hundred
//! thousand years between the first people and the first city, and no assumption
//! that a world must have passed through an ice age: the depth of prehistory is a
//! wide seeded draw biased by how kind the planet turned out to be, and the epoch
//! is only ever a convention placed partway along a timeline without ends.

use crate::civilization::age::Age;
use crate::core::fmt::thousands;
use crate::core::polar;
use rand::{Rng, RngExt};

/// The event the world's calendar is reckoned from.
///
/// Deliberately undefined: what happened is never written down — only that it was
/// important, where it happened, and when.
#[derive(Clone, Debug)]
pub struct Epoch {
    /// The event's year in the world's internal frame, which is counted from the
    /// first city. The atlas never shows this number; see [`Timeline::ce`].
    pub absolute_year: i32,
    /// The province it happened in.
    pub site_province: u32,
    /// The site's position on the canvas, in pixels, so the atlas can point at it.
    pub site_x: f32,
    pub site_y: f32,
    /// A plain description of the place, e.g. `the Green Crossing`.
    pub site_name: String,
    /// The realm holding the site at the map date, if any. A site in the
    /// wilderness belongs to nobody, which is the more usual case: an epoch is
    /// most often a place, not a polity.
    pub site_realm: Option<u32>,
}

/// The whole time axis of one world.
///
/// The internal frame is counted from the world's **first city**, which sits at
/// year zero; a realm founded two centuries later is founded in year 200. The
/// reader never sees it: the atlas reports [`Timeline::ce`]. The two quantities
/// that genuinely differ between worlds are kept apart on purpose —
///
/// * [`Timeline::prehistory_years`] is the **depth of prehistory**: how long the
///   world's peoples lived before the first hearth, so the first people sit at
///   internal year `−prehistory_years`. It is wide-ranged and seeded, because a
///   gentle world may reach its first city in five thousand years and a hard one
///   may take a million.
/// * [`Epoch`] is a **convention placed on that timeline**: for the early ages this
///   build reaches, it sits a few centuries to a couple of millennia *ahead* of the
///   atlas date, so the world the map portrays stands in the years before its own
///   calendar begins.
///
/// Neither is bounded by the other: the epoch is a measuring stick placed partway
/// along a timeline that runs arbitrarily far back, and the atlas may fall on either
/// side of it — BCE before it, CE after.
#[derive(Clone, Debug)]
pub struct Timeline {
    /// The depth of prehistory: years from the first people to the first city.
    pub prehistory_years: i32,
    /// The year the atlas portrays, in the internal frame.
    pub atlas_year: i32,
    pub epoch: Epoch,
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
            epoch: Epoch {
                absolute_year: 0,
                site_province: 0,
                site_x: 0.0,
                site_y: 0.0,
                site_name: "nowhere".to_string(),
                site_realm: None,
            },
        }
    }

    /// `1 800 BCE` / `2 100 CE` — an internal year written in the world's own era.
    pub fn ce(&self, absolute: i32) -> String {
        ce_label(absolute - self.epoch.absolute_year)
    }

    /// `200 CE`, the date the atlas portrays.
    pub fn atlas_label(&self) -> String {
        self.ce(self.atlas_year)
    }

    /// `The Common Era — reckoned from an important event at the Green Crossing`.
    pub fn epoch_label(&self) -> String {
        format!(
            "The Common Era — reckoned from an important event at {}",
            self.epoch.site_name
        )
    }

    /// How far back the first people and the first cities lie, in the world's era.
    ///
    /// This is the one line that carries the depth of prehistory, and it is why a
    /// world is never limited to a few thousand years of history: the epoch is a
    /// recent convention, while the peoples it counts for go back as far as the
    /// world took to get going.
    pub fn prehistory_label(&self) -> String {
        let people_before = (self.epoch.absolute_year + self.prehistory_years).max(0);
        let cities_before = self.epoch.absolute_year.max(0);
        format!(
            "The first people appeared {} years before the Common Era; the first cities rose {} years before it",
            thousands(people_before as f32),
            thousands(cities_before as f32),
        )
    }
}

/// A year in the Common Era, written the way the atlas writes it: `1 800 BCE`,
/// `300 CE`.
///
/// Negative years are BCE. The world keeps a year zero of its own — this is
/// nobody's calendar but its own, and it is not obliged to reproduce the one-year
/// gap between 1 BCE and 1 CE that our own historiography carries.
pub fn ce_label(year_ce: i32) -> String {
    if year_ce < 0 {
        format!("{} BCE", thousands(year_ce.unsigned_abs() as f32))
    } else {
        format!("{} CE", thousands(year_ce as f32))
    }
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

// ─── WHERE THE CALENDAR IS RECKONED FROM ─────────────────────────────────────

/// Everything the epoch-site scorer needs about one province.
#[derive(Clone, Copy, Debug)]
pub struct EpochCandidate {
    pub prov: u32,
    pub area_km2: f32,
    /// Mean habitability (0 .. ~1.4).
    pub hab: f32,
    /// Mean surface temperature, °C — used to keep ice out of the running.
    pub temp: f32,
    /// Mean annual precipitation (0 .. ~1).
    pub precip: f32,
    pub coastal: bool,
    pub lakeshore: bool,
    /// Cut off from the mainland core: an island or a stranded shore.
    pub island: bool,
    /// Highest and mean relief inside the province, in metres.
    pub peak_m: f32,
    pub mean_m: f32,
    /// Greatest river discharge inside the province, m³/s.
    pub flow_max: f32,
    /// Distinct realms holding provinces next door — a frontier is a crossroads.
    pub frontier_realms: usize,
    /// Distinct superregions spanned by the province and its neighbours.
    pub group_span: usize,
    /// How many realms the holder knows — how far its story could travel.
    pub realm_contacts: usize,
    /// A cradle seat is never a valid site: the epoch must not be the capital.
    pub is_seat: bool,
}

// ─── SCORING AND CHOOSING THE SITE ───────────────────────────────────────────

/// How fitting one province is to be the place a calendar is reckoned from.
///
/// Deliberately *not* the cradle score. A cradle is the richest ground in the
/// world; an epoch is a place whose story travelled. The two are almost opposite
/// qualities, which is the whole point — the epoch of our own calendar sits in a
/// marginal province of a foreign empire, not in Sumer.
///
/// Returns [`f32::NEG_INFINITY`] for a province disqualified outright: a cradle
/// seat, or permanent ice.
pub fn site_score(c: &EpochCandidate, best_habitability: f32) -> f32 {
    if c.is_seat || c.temp < polar::PACK_ICE_TEMP {
        return f32::NEG_INFINITY;
    }
    let mut score = 1.0;
    // Crossroads: between realms, between great regions, between river and sea.
    score += 1.6 * (c.frontier_realms.min(3)) as f32;
    score += 0.45 * (c.group_span.saturating_sub(1).min(3)) as f32;
    // A shape a story can be built on.
    if c.island {
        score += 1.2;
    }
    if c.lakeshore {
        score += 0.9;
    }
    if c.coastal {
        score += 0.8;
    }
    let rise = (c.peak_m - c.mean_m).max(0.0);
    score += (rise / 1_500.0).min(1.0);
    if c.flow_max > 0.0 {
        score += 0.3;
    }
    // A story travels with trade, so the reach of whoever held the place matters.
    score += 0.35 * (c.realm_contacts.min(6)) as f32;
    // Humility: it must not be the richest ground on the planet.
    if best_habitability > 0.0 {
        score -= 0.85 * (c.hab / best_habitability).clamp(0.0, 1.0);
    }
    score.max(0.05)
}

/// Pick the province the calendar is reckoned from.
///
/// A seeded weighted draw among the best few candidates, so the choice is never
/// arbitrary and never merely the top score: the same seed on the same geography
/// must always choose the same site, but a different seed must be free to choose a
/// different one. Candidates smaller than `min_area_km2` never enter — an epoch
/// needs a place, not a speck.
pub fn select_epoch_site(
    candidates: &[EpochCandidate],
    min_area_km2: f32,
    top_n: usize,
    rng: &mut impl Rng,
) -> Option<u32> {
    let best_habitability = candidates.iter().map(|c| c.hab).fold(0.0, f32::max);
    let mut scored: Vec<(f32, u32)> = candidates
        .iter()
        .filter(|c| c.area_km2 >= min_area_km2)
        .map(|c| (site_score(c, best_habitability), c.prov))
        .filter(|(score, _)| score.is_finite())
        .collect();
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let pool = &scored[..scored.len().min(top_n.max(1))];
    let weights: Vec<f32> = pool.iter().map(|(s, _)| s.max(1e-3).powf(2.5)).collect();
    let total: f32 = weights.iter().sum();
    let mut pick = rng.random::<f32>() * total;
    for (i, weight) in weights.iter().enumerate() {
        pick -= weight;
        if pick <= 0.0 {
            return Some(pool[i].1);
        }
    }
    Some(pool[pool.len() - 1].1)
}

/// A plain description of the epoch's site, built from its own climate and shape.
///
/// Real place names are proper nouns, and this world has no languages yet, so the
/// atlas describes the place instead of naming it: `the Green Crossing`,
/// `the Sunlit Isle`, `the Bleak Peak`.
pub fn place_name(c: &EpochCandidate) -> String {
    format!("the {} {}", climate_word(c.temp, c.precip), feature_word(c))
}

/// The climate half of a site's description.
fn climate_word(temp: f32, precip: f32) -> &'static str {
    if temp < -5.0 {
        "Frozen"
    } else if temp < 4.0 {
        "Bleak"
    } else if temp < 12.0 {
        if precip < 0.30 { "Cold" } else { "Pine" }
    } else if temp < 22.0 {
        if precip < 0.24 { "Dry" } else { "Green" }
    } else if precip < 0.20 {
        "Dust"
    } else {
        "Sunlit"
    }
}

/// The landform half of a site's description.
fn feature_word(c: &EpochCandidate) -> &'static str {
    let rise = c.peak_m - c.mean_m;
    if c.island {
        "Isle"
    } else if c.lakeshore {
        "Lake"
    } else if rise > 1_200.0 {
        "Peak"
    } else if c.flow_max > 2_000.0 {
        "River"
    } else if c.coastal {
        "Shore"
    } else {
        "Crossing"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn ce_label_counts_backwards_and_forwards() {
        assert_eq!(ce_label(0), "0 CE");
        assert_eq!(ce_label(300), "300 CE");
        assert_eq!(ce_label(2_100), "2 100 CE");
        assert_eq!(ce_label(-1_800), "1 800 BCE");
    }

    #[test]
    fn timeline_reports_internal_years_in_its_own_era() {
        let timeline = Timeline {
            prehistory_years: 40_000,
            atlas_year: 4_000,
            age: Age::CopperAndAlloys,
            epoch: Epoch {
                absolute_year: 2_400,
                site_province: 7,
                site_x: 1.0,
                site_y: 2.0,
                site_name: "the Green Crossing".to_string(),
                site_realm: None,
            },
        };
        // Internal year zero is the first city; the epoch is 2 400 years later.
        assert_eq!(timeline.ce(2_400), "0 CE");
        assert_eq!(timeline.ce(0), "2 400 BCE");
        assert_eq!(timeline.ce(4_000), "1 600 CE");
        assert_eq!(timeline.atlas_label(), "1 600 CE");
        // The depth of prehistory is what keeps the timeline from being bounded.
        assert_eq!(
            timeline.prehistory_label(),
            "The first people appeared 42 400 years before the Common Era; the first cities rose 2 400 years before it"
        );
    }

    /// A world portrayed *before* its calendar's epoch reads BCE: the atlas and every
    /// founding lie in the years before the Common Era, and the first cities are as
    /// deep in that era as the epoch stands ahead of them.
    #[test]
    fn a_world_before_its_epoch_reads_bce() {
        let timeline = Timeline {
            prehistory_years: 40_000,
            atlas_year: 1_700,
            age: Age::CopperAndAlloys,
            epoch: Epoch {
                absolute_year: 3_700, // two thousand years after the atlas
                site_province: 7,
                site_x: 1.0,
                site_y: 2.0,
                site_name: "the Green Crossing".to_string(),
                site_realm: None,
            },
        };
        assert_eq!(timeline.atlas_label(), "2 000 BCE");
        assert_eq!(timeline.ce(0), "3 700 BCE");
        assert_eq!(
            timeline.prehistory_label(),
            "The first people appeared 43 700 years before the Common Era; the first cities rose 3 700 years before it"
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

    /// A cradle seat can never be the epoch site, however fine its shape.
    #[test]
    fn a_seat_is_never_the_epoch() {
        let mut seat = candidate(1, true, true);
        seat.is_seat = true;
        assert!(site_score(&seat, 1.2) < 0.0);
    }

    /// Permanent ice is disqualified too.
    #[test]
    fn ice_is_never_the_epoch() {
        let mut frozen = candidate(2, true, true);
        frozen.temp = -20.0;
        assert!(site_score(&frozen, 1.2) < 0.0);
    }

    /// The chooser is deterministic for a seed, never picks a speck, and prefers
    /// the crossroads on a heavily weighted draw.
    #[test]
    fn selection_is_deterministic_and_weighted() {
        let pool = vec![
            candidate(1, false, false), // plain interior
            candidate(2, true, true),   // island, two frontiers: the strong site
            candidate(3, false, false),
            candidate(4, false, false),
        ];
        let mut first = ChaCha8Rng::seed_from_u64(99);
        let mut second = ChaCha8Rng::seed_from_u64(99);
        let a = select_epoch_site(&pool, 500.0, 4, &mut first);
        let b = select_epoch_site(&pool, 500.0, 4, &mut second);
        assert_eq!(a, b);
        assert!(a.is_some());

        // A speck below the area floor is never chosen, however fine its shape.
        let mut tiny = candidate(9, true, true);
        tiny.area_km2 = 200.0;
        assert_eq!(select_epoch_site(&[tiny], 500.0, 4, &mut first), None);
    }

    /// A much better site should come up far more often than a poor one.
    #[test]
    fn weighted_draw_favours_the_better_site() {
        let pool = vec![candidate(1, false, false), candidate(2, true, true)];
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let mut strong = 0;
        for _ in 0..200 {
            if select_epoch_site(&pool, 500.0, 2, &mut rng) == Some(2) {
                strong += 1;
            }
        }
        assert!(strong > 120, "strong site chosen {strong}/200 times");
    }

    fn candidate(prov: u32, island: bool, frontiers: bool) -> EpochCandidate {
        EpochCandidate {
            prov,
            area_km2: 5_000.0,
            hab: 0.9,
            temp: 15.0,
            precip: 0.4,
            coastal: island,
            lakeshore: false,
            island,
            peak_m: 1_500.0,
            mean_m: 300.0,
            flow_max: 900.0,
            frontier_realms: if frontiers { 2 } else { 0 },
            group_span: if frontiers { 3 } else { 1 },
            realm_contacts: if frontiers { 4 } else { 0 },
            is_seat: false,
        }
    }
}
