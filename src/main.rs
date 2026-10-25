mod climate;
mod core;
mod export;
mod geography;
mod hydrology;
mod terrain;
mod geopolitics;
mod civilization;
mod viewer;

use climate::{simulate_moisture, simulate_temperature, simulate_winds, ClimateConfig};
use civilization::{
    generate_civilizations, report_civilizations, CivilizationConfig, CivilizationWorld,
};
use geopolitics::{
    build_geopolitical_hierarchy, build_ocean_hierarchy, extract_ocean_graph, extract_province_graph,
};
use core::{polar, relief, Grid, WorldCache};
use geography::{generate_ocean_provinces, generate_provinces, GeographyConfig, OceanConfig};
use hydrology::{
    calculate_flow, extract_rivers_and_basins, fill_depressions_and_find_lakes, HydrologyConfig,
};
use rand::RngExt;
use std::env;
use std::path::Path;
use std::time::Instant;
use terrain::{
    apply_fractal_detail, simulate_hydraulic_erosion, simulate_tectonics, simulate_thermal_erosion,
    WorldConfig,
};

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

    for arg in env::args().skip(1) {
        if arg == "--save" { save_cache = true; } 
        else if arg == "--load" { load_cache = true; }
        else if arg == "--viewer" { run_viewer = true; }
        else if arg == "--plates" { run_plates = true; }
        else if arg == "--civs" { run_civilizations = true; }
        else if let Ok(parsed_seed) = arg.parse::<u64>() { seed = parsed_seed; }
    }

    let world_config = WorldConfig {
        width: 8192,
        height: 4096,
        seed,
        num_plates: 32,
        ocean_ratio: 0.65,
        erosion_droplets: 3_500_000,
        ..Default::default()
    };
    let climate_config = ClimateConfig { seed, ..Default::default() };
    let hydro_config = HydrologyConfig { min_lake_depth: 0.015, min_river_flow: 1800.0, major_river_flow: 15000.0, ..Default::default() };
    let geo_config = GeographyConfig::default();
    let ocean_config = OceanConfig::default();
    let civ_config = CivilizationConfig::default();

    println!("Active Seed: {} | Canvas: 8192x4096", seed);
    let cache_path = format!("world_{}.bin", seed);

    let total_start = Instant::now();

    // --- CACHE ARCHITECTURE: STAGES 1 TO 3 ---
    let mut cache = if load_cache && Path::new(&cache_path).exists() {
        let load_start = Instant::now();
        println!("Loading world from cache '{}'...", cache_path);
        let loaded = WorldCache::load(&cache_path);
        println!("✓ Loaded cached world data in:             {:.2?}", load_start.elapsed());
        loaded
    } else {
        let t_start = Instant::now();
        let mut result = simulate_tectonics(&world_config);
        apply_fractal_detail(&mut result.heightmap, &world_config);
        simulate_hydraulic_erosion(&mut result.heightmap, &world_config);
        simulate_thermal_erosion(&mut result.heightmap, &world_config);
        println!("✓ [1/3] Geology & Erosion completed:       {:.2?}", t_start.elapsed());

        let c_start = Instant::now();
        let temp_grid = simulate_temperature(&result.heightmap, relief::SEA_LEVEL, &climate_config);
        let winds = simulate_winds(world_config.width, world_config.height, &climate_config);
        let moisture = simulate_moisture(&result.heightmap, &temp_grid, &winds, relief::SEA_LEVEL, &climate_config);
        println!("✓ [2/3] Atmospheric Climate completed:     {:.2?}", c_start.elapsed());

        let h_start = Instant::now();
        let pit_res = fill_depressions_and_find_lakes(&result.heightmap, relief::SEA_LEVEL, &hydro_config);
        let flow_res = calculate_flow(&pit_res.filled_heightmap, &moisture.precipitation, relief::SEA_LEVEL, &hydro_config);
        // Rivers extracted here if needed later
        println!("✓ [3/3] Hydrology & River routing done:    {:.2?}", h_start.elapsed());

        let new_cache = WorldCache {
            width: world_config.width,
            height: world_config.height,
            heightmap: result.heightmap,
            temp_grid,
            precip_grid: moisture.precipitation,
            flow_grid: flow_res.accumulated_flow,
            is_lake: pit_res.is_lake,
        };

        if save_cache {
            println!("Saving world state to cache '{}'...", cache_path);
            new_cache.save(&cache_path);
        }
        new_cache
    };

    // --- POLAR ICE: the belt is permanent arctic waste ----------------------
    // The polar belt — and every landmass cut off from a pole — is permanent ice:
    // no province may settle there and no tundra may grow at its shore. Pinning
    // its surface temperature into the ice band is what lets every downstream
    // pass — the province generator, the atlas painter and the hover telemetry —
    // read it as the same arctic waste. It runs after the cache is built so
    // worlds saved under the old rules still come out right.
    let polar_ice =
        polar::permanent_polar_ice(&cache.heightmap, &cache.is_lake, relief::SEA_LEVEL, seed);
    polar::freeze_polar_ice(&mut cache.temp_grid, &polar_ice);

    // --- RELIEF REPORT: the finished world's hypsometric profile, in metres ---
    report_relief(&cache.heightmap);

    // --- STAGE 4: POLITICAL GEOGRAPHY (PROVINCES) ---
    let p_start = Instant::now();
    let prov_res = generate_provinces(
        &cache.heightmap,
        &cache.temp_grid,
        &cache.precip_grid,
        &cache.flow_grid,
        &cache.is_lake,
        &polar_ice,
        relief::SEA_LEVEL, // The planet's own vertical datum (the old 0.11 threshold is only the *noise* water line)
        hydro_config.min_river_flow,
        &geo_config,
        seed,
    );
    println!("✓ [4/7] Geography & Provinces done:        {:.2?}", p_start.elapsed());

    // --- STAGE 5: GEOPOLITICAL HIERARCHY (AREAS, REGIONS, CONTINENTS) ---
    let g_start = Instant::now();
    let extracted = extract_province_graph(&prov_res.province_ids, &cache.heightmap, &cache.is_lake, relief::SEA_LEVEL);
    let hierarchy = build_geopolitical_hierarchy(extracted.provinces, seed, cache.width as f32);
    println!("✓ [5/7] Geopolitical Hierarchy completed:   {:.2?}", g_start.elapsed());

    // --- STAGE 6: HYDRO-POLITICAL HIERARCHY (THE WORLD OCEAN) ---
    let o_start = Instant::now();
    let ocean_res = generate_ocean_provinces(
        &cache.heightmap,
        &cache.is_lake,
        relief::SEA_LEVEL,
        &ocean_config,
        seed,
    );
    let ocean_extracted = extract_ocean_graph(&ocean_res.ocean_province_ids);
    let ocean_hierarchy = build_ocean_hierarchy(
        ocean_extracted.provinces,
        seed,
        cache.width as f32,
        &ocean_config,
    );
    println!(
        "✓ [6/7] Hydro-Political Hierarchy completed:{:.2?}",
        o_start.elapsed()
    );
    println!(
        "         Ocean: {} provinces → {} basins → {} superregions → {} regions → {} areas.",
        ocean_res.num_provinces,
        ocean_hierarchy.total_continents,
        ocean_hierarchy.total_superregions,
        ocean_hierarchy.total_regions,
        ocean_hierarchy.total_areas,
    );

    // --- STAGE 7: THE DAWN OF HISTORY (CIVILIZATIONS) ---
    // Where people stop being a landscape and start being a story. The stage is
    // only run when it is wanted (`--civs`, `--viewer` or `--plates`), because a
    // world without a history is still a finished world.
    let want_civilizations = run_civilizations || run_viewer || run_plates;
    let civ_start = Instant::now();
    let civilizations = if want_civilizations {
        let world = generate_civilizations(
            &cache.heightmap,
            &cache.temp_grid,
            &cache.precip_grid,
            &cache.flow_grid,
            &cache.is_lake,
            &prov_res.habitability,
            &prov_res.province_ids,
            &hierarchy,
            relief::SEA_LEVEL,
            hydro_config.min_river_flow,
            climate_config.mode,
            &civ_config,
            seed,
        );
        println!(
            "✓ [7/7] Dawn of Civilization completed:     {:.2?}",
            civ_start.elapsed()
        );
        if run_civilizations {
            println!();
            report_civilizations(&world);
        }
        world
    } else {
        println!("· [7/7] Dawn of Civilization skipped (pass --civs to wake the world up).");
        CivilizationWorld::empty(seed)
    };


    println!("-------------------------------------------------------");
    println!("Total Generation Time:                     {:.2?}", total_start.elapsed());
    println!("-------------------------------------------------------");

    // --- STAGE 8: LAUNCH INTERACTIVE MAP VIEWER ---
    if run_viewer {
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
            Box::new(|_cc| Ok(Box::new(viewer::MapViewer::new(
                cache.clone(),
                hierarchy.clone(),
                ocean_hierarchy.clone(),
                prov_res.clone(),
                ocean_res.clone(),
                civilizations.clone(),
            )))),
        ).unwrap();
    }

    let exp_start = Instant::now();
    export::save_hierarchical_political_map(
        &cache.heightmap,
        &cache.temp_grid,
        &cache.precip_grid,
        &cache.flow_grid,
        &cache.is_lake,
        &hierarchy,
        &prov_res.province_ids,
        relief::SEA_LEVEL,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_political_map.png",
    ).expect("Failed to export political map");

    println!("✓ Rendered Hierarchical Political Map in:   {:.2?}", exp_start.elapsed());

    let phys_start = Instant::now();
    export::save_physical_map(
        &cache.heightmap,
        &cache.temp_grid,
        &cache.precip_grid,
        &cache.flow_grid,
        &cache.is_lake,
        relief::SEA_LEVEL,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_physical_map.png",
    ).expect("Failed to export physical map");

    println!("✓ Rendered Physical Relief Map in:         {:.2?}", phys_start.elapsed());

    let topo_start = Instant::now();
    export::save_topographic_map(
        &cache.heightmap,
        &cache.temp_grid,
        &cache.flow_grid,
        &cache.is_lake,
        relief::SEA_LEVEL,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_topographic_map.png",
    ).expect("Failed to export topographic map");

    println!("✓ Rendered Topographic Survey Map in:      {:.2?}", topo_start.elapsed());

    // --- STAGE 9: CIVILIZATIONS MAP (optional) ---
    // The very same painter the viewer's Civilizations scope uses, exported at
    // full 8K so it sits alongside the other `world_*.png` plates.
    if want_civilizations && !civilizations.is_empty() {
        let civ_start = Instant::now();
        let mut civ_viewer = viewer::MapViewer::new(
            cache.clone(),
            hierarchy.clone(),
            ocean_hierarchy.clone(),
            prov_res.clone(),
            ocean_res.clone(),
            civilizations.clone(),
        );
        civ_viewer.set_view(viewer::MapScope::Civilizations, viewer::MapTier::Regions, true);
        civ_viewer
            .save_png("world_civilizations_map.png", 1.0)
            .unwrap_or_else(|e| panic!("Failed to render civilizations map: {e}"));
        println!(
            "✓ Rendered Civilizations Map in:           {:.2?}",
            civ_start.elapsed()
        );
    }

    // --- STAGE 10: HEADLESS ATLAS PLATES (optional) ---
    // The very same painter the interactive window uses, exported straight to
    // 2K PNGs. Handy for regenerating the README's scope and tier tables in one
    // shot: `cargo run --release -- <seed> --load --plates`.
    if run_plates {
        let plate_start = Instant::now();
        let plates: [(&str, viewer::MapScope, viewer::MapTier); 17] = [
            ("atlas_physical.png",            viewer::MapScope::Physical,      viewer::MapTier::Regions),
            ("atlas_topographic.png",         viewer::MapScope::Topographic,   viewer::MapTier::Regions),
            ("atlas_climate.png",             viewer::MapScope::Climate,       viewer::MapTier::Regions),
            ("atlas_temperature.png",         viewer::MapScope::Temperature,   viewer::MapTier::Regions),
            ("atlas_terrain.png",             viewer::MapScope::Terrain,       viewer::MapTier::Regions),
            ("atlas_civilizations.png",       viewer::MapScope::Civilizations, viewer::MapTier::Regions),
            ("atlas_land_provinces.png",      viewer::MapScope::Land,          viewer::MapTier::Provinces),
            ("atlas_land_areas.png",          viewer::MapScope::Land,          viewer::MapTier::Areas),
            ("atlas_land_regions.png",        viewer::MapScope::Land,          viewer::MapTier::Regions),
            ("atlas_land_superregions.png",   viewer::MapScope::Land,          viewer::MapTier::Superregions),
            ("atlas_land_continents.png",     viewer::MapScope::Land,          viewer::MapTier::Continents),
            ("atlas_water_provinces.png",     viewer::MapScope::Water,         viewer::MapTier::Provinces),
            ("atlas_water_areas.png",         viewer::MapScope::Water,         viewer::MapTier::Areas),
            ("atlas_water_regions.png",       viewer::MapScope::Water,         viewer::MapTier::Regions),
            ("atlas_water_superregions.png",  viewer::MapScope::Water,         viewer::MapTier::Superregions),
            ("atlas_water_basins.png",        viewer::MapScope::Water,         viewer::MapTier::Continents),
            ("atlas_everywhere_regions.png",  viewer::MapScope::Everywhere,    viewer::MapTier::Regions),
        ];

        let mut plate_viewer = viewer::MapViewer::new(
            cache.clone(),
            hierarchy.clone(),
            ocean_hierarchy.clone(),
            prov_res.clone(),
            ocean_res.clone(),
            civilizations.clone(),
        );
        for (path, scope, tier) in plates {
            plate_viewer.set_view(scope, tier, true);
            plate_viewer
                .save_png(path, 0.25)
                .unwrap_or_else(|e| panic!("Failed to render atlas plate {path}: {e}"));
            println!("✓ Rendered atlas plate:                    {path}");
        }
        println!("✓ Rendered {} atlas plates in:             {:.2?}", plates.len(), plate_start.elapsed());
    }

    println!("=======================================================");
}

/// Print the finished world's hypsometric profile in metres.
///
/// This is the quickest sanity check that the vertical datum came out sane —
/// how much of the surface ended up as ocean, where the median shore and sea
/// floor sit, how much shelf there is, and what the extremes are. It is printed
/// for cached worlds too, so a world generated under an older datum announces
/// itself instead of quietly reporting a 400 m-deep ocean.
fn report_relief(heightmap: &Grid<f32>) {
    const BIN_M: f32 = 100.0;
    const BINS: usize = 200; // the datum spans -11 000 m … +9 000 m

    let mut bins = vec![0u64; BINS];
    let (mut land, mut ocean) = (0u64, 0u64);
    let (mut peak, mut trench) = (f32::MIN, f32::MAX);

    for &elev in heightmap.raw_slice() {
        let metres = relief::to_metres(elev);
        if metres > 0.0 {
            land += 1;
            peak = peak.max(metres);
        } else {
            ocean += 1;
            trench = trench.min(metres);
        }
        let bin = (((metres + relief::DEEPEST_TRENCH_M) / BIN_M) as isize)
            .clamp(0, BINS as isize - 1) as usize;
        bins[bin] += 1;
    }

    let surface = (land + ocean).max(1) as f64;
    let (land_seen, ocean_seen) = (land.max(1) as f64, ocean.max(1) as f64);
    let (mut deep, mut high) = (0f64, 0f64);
    let (mut shelf, mut median_depth) = (0u64, 0f32);
    let (mut median_altitude, mut altitude_90) = (0f32, 0f32);
    let (mut have_depth, mut have_alt, mut have_alt90) = (false, false, false);

    for bin in 0..BINS {
        let metres = bin as f32 * BIN_M - relief::DEEPEST_TRENCH_M + BIN_M * 0.5;
        let count = bins[bin] as f64;
        if metres <= 0.0 {
            deep += count;
            if metres >= -200.0 {
                shelf += bins[bin];
            }
            if !have_depth && deep >= ocean_seen * 0.5 {
                median_depth = metres;
                have_depth = true;
            }
        } else {
            high += count;
            if !have_alt && high >= land_seen * 0.5 {
                median_altitude = metres;
                have_alt = true;
            }
            if !have_alt90 && high >= land_seen * 0.9 {
                altitude_90 = metres;
                have_alt90 = true;
            }
        }
    }

    println!("── Planetary relief (vertical datum applied) ──────────");
    println!(
        "   Surface : {:.1}% land / {:.1}% ocean",
        100.0 * land as f64 / surface,
        100.0 * ocean as f64 / surface
    );
    println!(
        "   Land    : median +{:.0} m | 90% below +{:.0} m | highest peak +{:.0} m",
        median_altitude, altitude_90, peak
    );
    println!(
        "   Ocean   : median {:.0} m | {:.1}% continental shelf (<= -200 m) | deepest trench {:.0} m",
        median_depth,
        100.0 * shelf as f64 / ocean_seen,
        trench
    );
}