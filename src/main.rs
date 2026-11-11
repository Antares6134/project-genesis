mod civilization;
mod climate;
mod core;
mod export;
mod geography;
mod geopolitics;
mod hydrology;
mod language;
mod pipeline;
mod terrain;
mod viewer;

use crate::civilization::{report_civilizations, report_civilizations_all, report_languages};
use crate::core::relief;
use pipeline::{Genesis, LoadError, Stage};
use std::env;
use std::time::Instant;

fn main() {
    println!("=======================================================");
    println!("   Project Genesis: High-Definition Physical World Engine  ");
    println!("=======================================================");

    // Parse CLI arguments: `cargo run --release -- 1980 --save`
    let mut seed = 42;
    let mut save_cache = false;
    let mut load_cache = false;

    let mut run_viewer = false;
    let mut run_plates = false;
    let mut run_civilizations = false;
    let mut report_all_societies = false;
    let mut run_languages = false;

    for arg in env::args().skip(1) {
        if arg == "--save" {
            save_cache = true;
        } else if arg == "--load" {
            load_cache = true;
        } else if arg == "--viewer" {
            run_viewer = true;
        } else if arg == "--plates" {
            run_plates = true;
        } else if arg == "--civs" {
            run_civilizations = true;
        } else if arg == "--civs-all" {
            run_civilizations = true;
            report_all_societies = true;
        } else if arg == "--langs" {
            run_languages = true;
        } else if let Ok(parsed_seed) = arg.parse::<u64>() {
            seed = parsed_seed;
        }
    }

    println!(
        "Active Seed: {} | Canvas: {}x{}",
        seed,
        pipeline::WORLD_WIDTH,
        pipeline::WORLD_HEIGHT
    );

    let total_start = Instant::now();

    // ── THE ATLAS: THE WINDOW COMES FIRST, THE WORLD SECOND ───────────────────
    //
    // Every act of creation is now run from the viewer's own panel, one press at
    // a time, and watched while it runs. The command line therefore no longer
    // decides *what* gets generated before the window opens — only where the
    // panel starts: a seed to begin from, whether act three and act six should
    // write the world cache (`--save`), and whether to adopt the world already on
    // disk (`--load`) instead of growing one.
    if run_viewer {
        let mut genesis = Genesis::new(seed);
        genesis.set_autosave(save_cache);
        if load_cache {
            // `--load --viewer` adopts the saved world for the seed in one read:
            // the acts the file carries are taken as done, and the act after them
            // is offered next. It is instant: no stage is re-run, only the file is
            // read.
            if let Err(why) = genesis.load_cache() {
                report_load_failure(&why);
            }
        }

        println!("Launching Interactive 2D Viewer...");
        // An atlas is a window application, not a pop-up: open at a proper size
        // for a modern desktop and start maximised, while keeping a sane
        // restore size and a floor below which the toolbar would have to wrap.
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1720.0, 1040.0])
                .with_min_inner_size([1100.0, 700.0])
                .with_maximized(true),
            ..Default::default()
        };

        eframe::run_native(
            "Project Genesis - Atlas Viewer",
            options,
            Box::new(|_cc| Ok(Box::new(viewer::MapViewer::new(genesis)))),
        )
        .unwrap();
        return;
    }

    // ── HEADLESS: GROW THE WORLD, PRINT ITS FIGURES, WRITE ITS PLATES ─────────
    //
    // `--save`, `--civs` and `--plates` are for scripts and for regenerating the
    // README, so they want exactly what they always wanted: a finished world, a
    // terminal full of figures, and PNGs on disk. They run the same pipeline the
    // atlas does, silently.
    let mut genesis = Genesis::new(seed);
    genesis.set_autosave(save_cache);
    if load_cache {
        if let Err(why) = genesis.load_cache() {
            report_load_failure(&why);
        }
    }

    let want_civilizations = run_civilizations || run_plates || run_languages;
    let want_languages = run_languages || run_civilizations || run_plates;
    genesis.run_through(Stage::HydroPolitics, None);

    // --- STAGE 7: THE DAWN OF HISTORY (CIVILIZATIONS) ---
    // Where people stop being a landscape and start being a story. The stage is
    // only run when it is wanted (`--civs` or `--plates`), because a world
    // without a history is still a finished world.
    if want_civilizations {
        genesis.run_stage(Stage::Civilization, None);
        if run_civilizations {
            println!();
            let world = genesis
                .atlas()
                .expect("act seven just finished")
                .civilizations;
            if report_all_societies {
                report_civilizations_all(&world);
            } else {
                report_civilizations(&world);
            }
        }
    } else {
        println!("· [7/8] Dawn of Civilization skipped (pass --civs to wake the world up).");
    }

    // --- STAGE 8: LANGUAGES & PHONETICS ---
    // Names are the point: the realms Stage 7 generated are republished with
    // their own autonyms, and every province receives a language and family.
    if want_languages && want_civilizations {
        // Languages are cheap — a fraction of a second — so there is no companion
        // cache to adopt: the stage simply runs, and a loaded world grows its
        // tongues afresh rather than reading them back.
        genesis.run_stage(Stage::Languages, None);
        if run_languages {
            println!();
            let world = genesis.atlas().expect("act eight just finished").languages;
            report_languages(&world);
        }
    } else {
        println!("· [8/8] Languages & Phonetics skipped (pass --langs to give the world a voice).");
    }

    println!("-------------------------------------------------------");
    println!(
        "Total Generation Time:                     {:.2?}",
        total_start.elapsed()
    );
    println!("-------------------------------------------------------");

    let world = genesis.atlas().expect("a finished world");
    let hydro_config = genesis.hydro_config();
    let temperature = world.temperature.clone().expect("act two");
    let precipitation = world.precipitation.clone().expect("act two");
    let flow = world.flow.clone().expect("act three");
    let is_lake = world.is_lake.clone().expect("act three");
    let provinces = world.provinces.clone().expect("act four");

    let exp_start = Instant::now();
    export::save_hierarchical_political_map(
        &world.heightmap,
        &temperature,
        &precipitation,
        &flow,
        &is_lake,
        &world.hierarchy,
        &provinces.province_ids,
        relief::SEA_LEVEL,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_political_map.png",
    )
    .expect("Failed to export political map");

    println!(
        "✓ Rendered Hierarchical Political Map in:   {:.2?}",
        exp_start.elapsed()
    );

    let phys_start = Instant::now();
    export::save_physical_map(
        &world.heightmap,
        &temperature,
        &precipitation,
        &flow,
        &is_lake,
        relief::SEA_LEVEL,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_physical_map.png",
    )
    .expect("Failed to export physical map");

    println!(
        "✓ Rendered Physical Relief Map in:         {:.2?}",
        phys_start.elapsed()
    );

    let topo_start = Instant::now();
    export::save_topographic_map(
        &world.heightmap,
        &temperature,
        &flow,
        &is_lake,
        relief::SEA_LEVEL,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_topographic_map.png",
    )
    .expect("Failed to export topographic map");

    println!(
        "✓ Rendered Topographic Survey Map in:      {:.2?}",
        topo_start.elapsed()
    );

    // --- STAGE 9: CIVILIZATIONS MAP (optional) ---
    // The very same painter the viewer's Civilizations scope uses, exported at
    // full 8K so it sits alongside the other `world_*.png` plates.
    if want_civilizations && !world.civilizations.is_empty() {
        let civ_start = Instant::now();
        let mut civ_viewer = viewer::MapViewer::from_world(world.clone());
        civ_viewer.set_view(
            viewer::MapScope::Civilizations,
            viewer::MapTier::Regions,
            true,
        );
        civ_viewer
            .save_png("world_civilizations_map.png", 1.0)
            .unwrap_or_else(|e| panic!("Failed to render civilizations map: {e}"));
        println!(
            "✓ Rendered Civilizations Map in:           {:.2?}",
            civ_start.elapsed()
        );
    }

    // --- STAGE 9b: LANGUAGES MAP (optional) ---
    // The tongue each province speaks, in the same painter the atlas uses.
    if want_languages && !world.languages.is_empty() {
        let tongue_start = Instant::now();
        let mut language_viewer = viewer::MapViewer::from_world(world.clone());
        language_viewer.set_view(
            viewer::MapScope::Languages,
            viewer::MapTier::Provinces,
            true,
        );
        language_viewer
            .save_png("world_languages_map.png", 1.0)
            .unwrap_or_else(|e| panic!("Failed to render languages map: {e}"));
        println!(
            "✓ Rendered Languages Map in:              {:.2?}",
            tongue_start.elapsed()
        );
    }

    // --- STAGE 10: HEADLESS ATLAS PLATES (optional) ---
    // The very same painter the interactive window uses, exported straight to
    // 2K PNGs. Handy for regenerating the README's scope and tier tables in one
    // shot: `cargo run --release -- <seed> --load --plates`.
    if run_plates {
        let plate_start = Instant::now();
        let plates: [(&str, viewer::MapScope, viewer::MapTier); 18] = [
            (
                "./exports/atlas_physical.png",
                viewer::MapScope::Physical,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_topographic.png",
                viewer::MapScope::Topographic,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_climate.png",
                viewer::MapScope::Climate,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_temperature.png",
                viewer::MapScope::Temperature,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_terrain.png",
                viewer::MapScope::Terrain,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_civilizations.png",
                viewer::MapScope::Civilizations,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_languages.png",
                viewer::MapScope::Languages,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_land_provinces.png",
                viewer::MapScope::Land,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_land_areas.png",
                viewer::MapScope::Land,
                viewer::MapTier::Areas,
            ),
            (
                "./exports/atlas_land_regions.png",
                viewer::MapScope::Land,
                viewer::MapTier::Regions,
            ),
            (
                "./exports/atlas_land_superregions.png",
                viewer::MapScope::Land,
                viewer::MapTier::Superregions,
            ),
            (
                "./exports/atlas_land_continents.png",
                viewer::MapScope::Land,
                viewer::MapTier::Continents,
            ),
            (
                "./exports/atlas_water_provinces.png",
                viewer::MapScope::Water,
                viewer::MapTier::Provinces,
            ),
            (
                "./exports/atlas_water_areas.png",
                viewer::MapScope::Water,
                viewer::MapTier::Areas,
            ),
            (
                "./exports/atlas_water_regions.png",
                viewer::MapScope::Water,
                viewer::MapTier::Regions,
            ),
            (
                "./exports/atlas_water_superregions.png",
                viewer::MapScope::Water,
                viewer::MapTier::Superregions,
            ),
            (
                "./exports/atlas_water_basins.png",
                viewer::MapScope::Water,
                viewer::MapTier::Continents,
            ),
            (
                "./exports/atlas_everywhere_regions.png",
                viewer::MapScope::Everywhere,
                viewer::MapTier::Regions,
            ),
        ];

        let mut plate_viewer = viewer::MapViewer::from_world(world.clone());
        for (path, scope, tier) in plates {
            plate_viewer.set_view(scope, tier, true);
            plate_viewer
                .save_png(path, 0.25)
                .unwrap_or_else(|e| panic!("Failed to render atlas plate {path}: {e}"));
            println!("✓ Rendered atlas plate:                    {path}");
        }
        println!(
            "✓ Rendered {} atlas plates in:             {:.2?}",
            plates.len(),
            plate_start.elapsed()
        );
    }

    println!("=======================================================");
}

/// Say why a world could not be adopted from disk, in one sentence a reader can
/// act on — and carry on growing a fresh world instead of falling over.
fn report_load_failure(why: &LoadError) {
    match why {
        LoadError::Missing(path) => println!(
            "· No cached world at '{}' — nothing to load. Grow one, and save it as it is made.",
            path.display()
        ),
        LoadError::Unreadable(path, reason) => println!(
            "· Cached world '{}' could not be read ({reason}) — nothing to load.",
            path.display()
        ),
    }
}
