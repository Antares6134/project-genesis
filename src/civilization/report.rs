//! The headless dossier: what `--civs` prints for a finished world.
//!
//! This is the human-readable form of everything the Civilization map mode shows
//! on hover, in one place, so a run can be read — and diffed against the same seed
//! run again — without opening a window. It reports the world's calendar first,
//! then one block per realm in founding order: what it is, where it lives and how
//! many people it holds.

use crate::civilization::{Civilization, CivilizationWorld, SocietyTier};
use crate::core::fmt::{thousands, thousands_u64};
use crate::language::LanguagesWorld;
use crate::language::naming::sample_roots;

/// Print the dawn of history: the calendar, the five-way scheme, then a full
/// dossier for every state and one line for every people.
pub fn report_civilizations(world: &CivilizationWorld) {
    report(world, false);
}

/// Print the dawn of history with a full dossier for **every** society.
///
/// A partitioned world carries hundreds of societies, so this is opt-in
/// (`--civs-all`): hundreds of dossiers is a wall of text, not a report.
pub fn report_civilizations_all(world: &CivilizationWorld) {
    report(world, true);
}

fn report(world: &CivilizationWorld, all: bool) {
    println!(
        "── Dawn of civilization (seed {}) ─────────────────────",
        world.seed
    );

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
            report_one(civ);
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
        format!("{} ({})", civ.tier.form(), civ.environment.label()),
        civ.provinces.len(),
        thousands(civ.area_km2),
        thousands_u64(civ.population),
        civ.density_per_km2,
    );
}

/// Print one realm's dossier.
fn report_one(civ: &Civilization) {
    println!("   🏺 {}", civ.name);
    println!(
        "      {} · {}",
        civ.culture.outlook.label(),
        civ.tier.mobility()
    );
    println!("      {:<13}{}", "Society:", civ.tier.form());
    println!(
        "      {:<13}{}",
        if civ.tier.is_state() {
            "Cradle:"
        } else {
            "Terrain:"
        },
        if civ.tier.is_state() {
            civ.cradle_kind.label()
        } else {
            civ.environment.label()
        }
    );
    println!(
        "      {:<13}{}",
        "Economy:",
        civ.culture.subsistence.label()
    );
    println!("      {:<13}{}", "Faith:", civ.faith.kind.label());
    println!("      {:<13}{}", "Polity:", civ.culture.government.label());
    println!("      {:<13}{}", "Hierarchy:", "Later");
    println!(
        "      {:<13}{}",
        if civ.tier.is_state() {
            "Capital:"
        } else {
            "Heartland:"
        },
        &format!("Province #{}", civ.seat_province)
    );
    println!(
        "      {:<13}{}",
        "Area:",
        &format!("{} km²", thousands(civ.area_km2))
    );
    println!(
        "      {:<13}{}",
        "Population:",
        &format!(
            "{} ({:.1}/km²)",
            thousands_u64(civ.population),
            civ.density_per_km2
        )
    );
    println!("      {:<13}{}", "Culture:", "Later");
    println!("      {:<13}{}", "Founded:", civ.origin_label());
    lineage_line("Predecessor:", "Predecessors:", &[]);
    lineage_line("Successor:", "Successors:", &[]);
    println!();
}

/// A lineage line. The label pluralizes as soon as a realm has more than one
/// predecessor or successor; until lineage data exists the value is `None`.
fn lineage_line(singular: &str, plural: &str, names: &[&str]) {
    let label = if names.len() >= 2 { plural } else { singular };
    let value = if names.is_empty() {
        "None".to_string()
    } else {
        names.join(", ")
    };
    println!("      {:<13}{}", label, value);
}

/// Print the world's languages: every family, its phonology, its homeland, its
/// lexicon in miniature, and the languages that descend from it.
pub fn report_languages(world: &LanguagesWorld) {
    println!(
        "── Languages & Phonetics (seed {}) ───────────────────",
        world.seed
    );
    if world.is_empty() {
        println!("   No language ever took root on this world.");
        return;
    }
    println!(
        "   {} families · {} languages · {} inhabited provinces · {} people",
        world.families.len(),
        world.languages.len(),
        world.total_provinces(),
        thousands_u64(world.total_population()),
    );
    println!();
    for family in world.families_by_size() {
        println!(
            "   🗣 {} ({}) — {} · {}",
            family.name,
            family.urheimat.archetype.label(),
            family.autonym,
            family.urheimat.archetype.describe(),
        );
        println!(
            "      Urheimat: {} | {} languages | {} provinces | {} people",
            world.urheimat_label(family.id),
            family.languages.len(),
            family.province_count,
            thousands_u64(family.population),
        );
        println!("      Roots: {}", sample_roots(&family.lexicon));
        for language in world.languages_of(family.id) {
            println!(
                "      · {:<32} depth {} · {} provinces · {} people",
                language.name,
                language.depth,
                language.province_count,
                thousands_u64(language.population),
            );
        }
        println!();
    }

    println!("   Named realms (the first twenty) —");
    for (index, name) in world.realm_names.iter().take(20).enumerate() {
        let family = world
            .family(world.family_of_realm.get(index).copied().unwrap_or(0))
            .map_or("an unknown tongue", |family| family.name.as_str());
        let language = world
            .language(world.language_of_realm.get(index).copied().unwrap_or(0))
            .map_or("an unknown tongue", |language| language.name.as_str());
        println!("      {:<34} {:<28} {language}", name, family);
    }
}
