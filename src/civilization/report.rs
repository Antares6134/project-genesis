//! The headless dossier: what `--civs` prints for a finished world.
//!
//! This is the human-readable form of everything the Civilization map mode shows
//! on hover, in one place, so a run can be read — and diffed against the same seed
//! run again — without opening a window. It reports the world's calendar first,
//! then one block per realm in founding order: why it rose where it rose, what it
//! became, who it knows and what it inherited.

use crate::civilization::epoch::ce_label;
use crate::civilization::{Civilization, CivilizationWorld};
use crate::core::fmt::{thousands, thousands_u64};

/// Print the dawn of history: the calendar, then one dossier per civilization.
pub fn report_civilizations(world: &CivilizationWorld) {
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
        "   Atlas date: {} | {} realms | {} civilized provinces | {} km² | population {}",
        world.map_year_label(),
        world.civilizations.len(),
        world.civilized_provinces,
        thousands(world.civilized_area_km2),
        thousands_u64(world.total_population),
    );
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
        report_one(world, civ);
    }
}

/// Print one realm's dossier.
fn report_one(world: &CivilizationWorld, civ: &Civilization) {
    println!(
        "   🏺 {} — {} · {}",
        civ.name,
        civ.cradle_kind.label(),
        civ.culture.government.label()
    );
    println!(
        "      Founded {} (age {})",
        ce_label(civ.founding_year),
        thousands(civ.age_years as f32),
    );
    println!(
        "      Seat: province #{} · {:.1}° {} {:.1}° {} | {} provinces · {} km² · {} people ({:.1}/km²)",
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
