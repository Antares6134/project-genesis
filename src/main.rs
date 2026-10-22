mod climate;
mod core;
mod export;
mod geography;
mod hydrology;
mod terrain;
mod geopolitics;
mod viewer;

use climate::{simulate_moisture, simulate_temperature, simulate_winds, ClimateConfig};
use geopolitics::{build_geopolitical_hierarchy, extract_province_graph};
use core::WorldCache;
use geography::{generate_provinces, GeographyConfig};
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

    for arg in env::args().skip(1) {
        if arg == "--save" { save_cache = true; } 
        else if arg == "--load" { load_cache = true; }
        else if arg == "--viewer" { run_viewer = true; } // NEW!
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

    println!("Active Seed: {} | Canvas: 8192x4096", seed);
    let cache_path = format!("world_{}.bin", seed);

    let total_start = Instant::now();

    // --- CACHE ARCHITECTURE: STAGES 1 TO 3 ---
    let cache = if load_cache && Path::new(&cache_path).exists() {
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
        let temp_grid = simulate_temperature(&result.heightmap, 0.40, &climate_config);
        let winds = simulate_winds(world_config.width, world_config.height, &climate_config);
        let moisture = simulate_moisture(&result.heightmap, &temp_grid, &winds, 0.40, &climate_config);
        println!("✓ [2/3] Atmospheric Climate completed:     {:.2?}", c_start.elapsed());

        let h_start = Instant::now();
        let pit_res = fill_depressions_and_find_lakes(&result.heightmap, 0.40, &hydro_config);
        let flow_res = calculate_flow(&pit_res.filled_heightmap, &moisture.precipitation, 0.40, &hydro_config);
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

    // --- STAGE 4: POLITICAL GEOGRAPHY (PROVINCES) ---
    let p_start = Instant::now();
    let prov_res = generate_provinces(
        &cache.heightmap,
        &cache.temp_grid,
        &cache.precip_grid,
        &cache.flow_grid,
        &cache.is_lake,
        0.40, // THE BUG FIX: Physical sea level is 0.40, NOT 0.11!!!
        hydro_config.min_river_flow,
        &geo_config,
        seed,
    );
    println!("✓ [4/4] Geography & Provinces done:        {:.2?}", p_start.elapsed());

    // --- STAGE 5: GEOPOLITICAL HIERARCHY (AREAS, REGIONS, CONTINENTS) ---
    let g_start = Instant::now();
    let extracted = extract_province_graph(&prov_res.province_ids, &cache.heightmap, &cache.is_lake, 0.40);
    let hierarchy = build_geopolitical_hierarchy(extracted.provinces, seed, cache.width as f32);
    println!("✓ [5/5] Geopolitical Hierarchy completed:   {:.2?}", g_start.elapsed());

    println!("-------------------------------------------------------");
    println!("Total Generation Time:                     {:.2?}", total_start.elapsed());
    println!("-------------------------------------------------------");

    // --- STAGE 6: LAUNCH INTERACTIVE MAP VIEWER ---
    if run_viewer {
        println!("Launching Interactive 2D Viewer...");
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
            ..Default::default()
        };
        
        eframe::run_native(
            "Project Genesis - Atlas Viewer",
            options,
            Box::new(|_cc| Ok(Box::new(viewer::MapViewer::new(
                cache.clone(), 
                hierarchy.clone(), 
                prov_res.clone()
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
        0.40,
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
        0.40,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        "world_physical_map.png",
    ).expect("Failed to export physical map");

    println!("✓ Rendered Physical Relief Map in:         {:.2?}", phys_start.elapsed());
    println!("=======================================================");
}