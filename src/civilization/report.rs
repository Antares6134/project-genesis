//! The headless dossier: what `--civs` prints for a finished world.
//!
//! This is the human-readable form of everything the Civilization map mode shows
//! on hover, in one place, so a run can be read — and diffed against the same seed
//! run again — without opening a window. It reports the world's calendar first,
//! then one block per realm in founding order: why it rose where it rose, what it
//! became, who it knows and what it inherited.

use crate::civilization::{Civilization, CivilizationWorld, SocietyTier};
use crate::core::fmt::{thousands, thousands_u64};

/// Print the dawn of history: the calendar, the five-way scheme, then a full
/// dossier for every state and one line for every people.
pub fn report_civilizations(world: &CivilizationWorld) {
    report(world, false);
}

/// Print the dawn of history with a full dossier for **every** society.
///
/// A partitioned world carries hundreds of societies, so this is opt-in
/// (`--civs-all`): four hundred full dossiers is a wall of text, not a report.
pub fn report_civilizations_all(world: &CivilizationWorld) {
    report(world, true);
}

fn report(world: &CivilizationWorld, all: bool) {
    println!("── Dawn of civilization (seed {}) ─────────────────────", world.seed);

    if world.is_empty() {
        println!("   No civilization ever rose on this world.");
        return;
    }

    println!("   {}", world.epoch_label());
    println!(
        "   Epoch site: province #{} ({})",
        world.timeline.epoch.site_province,
        match world.timeline.epoch.site_realm {
            Some(id) => world.name_of(id),
            None => "unclaimed wilderness",
        }
    );
    println!("   {}", world.timeline.prehistory_label());
    println!(
        "   Atlas date: {} | {} societies on {} inhabited provinces | {} km² | population {}",
        world.map_year_label(),
        world.civilizations.len(),
        world.civilized_provinces,
        thousands(world.civilized_area_km2),
        thousands_u64(world.total_population),
    );
    // The five-way scheme, as the map paints it. The shares are the model's single
    // most useful diagnostic: a world of nothing but states is a bug, not a world.
    let counts = world.counts_by_tier();
    let provinces = world.provinces_by_tier();
    let areas = world.area_by_tier();
    let populations = world.population_by_tier();
    let inhabited_area: f32 = areas.iter().sum::<f32>().max(1.0);
    for (index, tier) in SocietyTier::LADDER.iter().enumerate() {
        println!(
            "     {:<26} {:>4} societies · {:>6} provinces · {:>5.1}% of the land · {:>11} people",
            tier.label(),
            counts[index],
            provinces[index],
            100.0 * areas[index] / inhabited_area,
            thousands_u64(populations[index]),
        );
    }
    println!(
        "   World age: {} ({}) — {}.",
        world.age_label(),
        world.age().stratum(),
        world.age().describe()
    );
    println!(
        "   Mean civilized density: {:.1} people/km²",
        world.mean_density_per_km2()
    );
    println!();

    for civ in &world.civilizations {
        if all || civ.tier.is_state() {
            report_one(world, civ);
        } else {
            report_line(civ);
        }
    }
}

/// One line for a people that is not a polity: what it is, where it is, how many
/// it feeds. Enough to audit the map without burying the states.
fn report_line(civ: &Civilization) {
    println!(
        "   · {:<32} {:<44} {:>4} provinces · {:>9} km² · {:>10} people ({:.2}/km²)",
        civ.name,
        format!("{} ({})", civ.tier.label(), civ.environment.label()),
        civ.provinces.len(),
        thousands(civ.area_km2),
        thousands_u64(civ.population),
        civ.density_per_km2,
    );
}

/// Print one realm's dossier.
fn report_one(world: &CivilizationWorld, civ: &Civilization) {
    println!(
        "   🏺 {} — {} · {} · {}",
        civ.name,
        civ.tier.label(),
        civ.cradle_kind.label(),
        civ.culture.government.label()
    );
    println!("      {}", civ.origin_label());
    if civ.founding_year.is_some() {
        println!("      {}", civ.year_label());
    }
    println!(
        "      {}: province #{} · {:.1}° {} {:.1}° {} | {} provinces · {} km² · {} people ({:.1}/km²)",
        if civ.tier.is_state() { "Seat" } else { "Heartland" },
        civ.seat_province,
        civ.seat_latitude_deg.abs(),
        if civ.seat_latitude_deg >= 0.0 { "N" } else { "S" },
        civ.seat_longitude_deg.abs(),
        if civ.seat_longitude_deg >= 0.0 { "E" } else { "W" },
        civ.provinces.len(),
        thousands(civ.area_km2),
        thousands_u64(civ.population),
        civ.density_per_km2,
    );
    println!("      Rise: {}", civ.rise.headline);
    println!("      Told short: {}", civ.rise.highlights.join(" · "));
    for reason in &civ.rise.reasons {
        println!("        • {reason}");
    }
    println!("      {}", civ.rise.seat);
    println!(
        "      Economy: {} · public temper: {}",
        civ.culture.subsistence.label(),
        civ.culture.outlook.label(),
    );
    println!(
        "      Hallmark: {} · Arts: {}",
        civ.culture.architecture,
        civ.culture.arts.join(", ")
    );
    println!("      {}", civ.culture.note);
    println!(
        "      Faith: {} · kept by {}",
        civ.faith.kind.label(),
        civ.faith.clergy
    );
    println!("      {}", civ.faith.note);
    println!(
        "      Observances: {}",
        civ.faith.practices.join(" · ")
    );
    println!(
        "      Polity: {} — {}",
        if civ.culture.government.is_state() {
            "a state"
        } else {
            "a pre-state society"
        },
        civ.culture.government.describe()
    );

    if civ.kin.is_empty() {
        println!("      Ancestry: none shared with another realm — a wholly separate stock.");
    } else {
        println!(
            "      Ancestry: {} family, together with {}",
            civ.language_family,
            name_list(&civ.kin)
        );
    }

    if civ.contacts.is_empty() {
        println!("      Contact: none. It rose with the world to itself.");
    } else {
        println!("      Contact:");
        for contact in &civ.contacts {
            println!(
                "        • {} ({}, ≈{} km) — {}",
                world.name_of(contact.other),
                contact.kind.label(),
                thousands(contact.distance_km),
                contact.relation,
            );
            println!("          {}", contact.note);
        }
    }

    if !civ.influences.is_empty() {
        println!("      Before it existed:");
        for influence in &civ.influences {
            println!(
                "        • took up {} from {}",
                influence.borrowed.join(", "),
                world.name_of(influence.other),
            );
            println!("          {}", influence.note);
        }
    }
    println!();
}

/// `Civilization #1`, `Civilization #2 and #3` — ids rendered as names.
fn name_list(ids: &[u32]) -> String {
    let names: Vec<String> = ids
        .iter()
        .map(|&id| format!("Civilization #{id}"))
        .collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        _ => format!(
            "{} and {}",
            names[..names.len() - 1].join(", "),
            names[names.len() - 1]
        ),
    }
}
