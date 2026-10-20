mod climate;
mod core;
mod export;
mod hydrology;
mod terrain;

use climate::{simulate_moisture, simulate_temperature, simulate_winds, ClimateConfig};
use hydrology::{
    calculate_flow, extract_rivers_and_basins, fill_depressions_and_find_lakes, HydrologyConfig,
};
use rand::RngExt;
use std::env;
use std::time::Instant;
use terrain::{
    apply_fractal_detail, simulate_hydraulic_erosion, simulate_tectonics,
    simulate_thermal_erosion, WorldConfig,
};

fn main() {
    println!("=======================================================");
    println!("   Project Genesis: High-Definition Physical World Engine  ");
    println!("=======================================================");

    let args: Vec<String> = env::args().collect();
    let seed: u64 = if args.len() > 1 {
        args[1].parse().unwrap_or_else(|_| {
            println!("Invalid seed argument, picking random seed.");
            rand::rng().random_range(1..1_000_000)
        })
    } else {
        rand::rng().random_range(1..1_000_000)
    };

    // EXTREME HIGH RESOLUTION: 8192 x 4096 (33.5 Million Data Points!)
    // Bigger than Europa Universalis 4's entire planetary map.
    let world_config = WorldConfig {
        width: 8192,
        height: 4096,
        seed,
        num_plates: 32,
        ocean_ratio: 0.65,
        erosion_droplets: 3_500_000, // 3.5 Million droplets for ultra-fine HD river carving!
        ..Default::default()
    };

    let climate_config = ClimateConfig {
        seed,
        ..Default::default()
    };

    // Scaled thresholds for HD rivers
    let hydro_config = HydrologyConfig {
        min_lake_depth: 0.015,
        min_river_flow: 1800.0,    // Flow requires much more accumulation at this scale
        major_river_flow: 15000.0, // Massive Amazon-style arteries
        ..Default::default()
    };

    println!(
        "Active Seed: {} | Canvas: {}x{} | Tectonic Plates: {}",
        world_config.seed, world_config.width, world_config.height, world_config.num_plates
    );

    let total_start = Instant::now();

    // 1. Geology
    let t_start = Instant::now();
    let mut result = simulate_tectonics(&world_config);
    apply_fractal_detail(&mut result.heightmap, &world_config);
    simulate_hydraulic_erosion(&mut result.heightmap, &world_config);
    simulate_thermal_erosion(&mut result.heightmap, &world_config);
    println!("✓ [1/3] Geology & Erosion completed:       {:.2?}", t_start.elapsed());

    // 2. Climate & Biomes
    let c_start = Instant::now();
    let temp_grid = simulate_temperature(&result.heightmap, 0.40, &climate_config);
    let winds = simulate_winds(world_config.width, world_config.height, &climate_config);
    let moisture = simulate_moisture(
        &result.heightmap,
        &temp_grid,
        &winds,
        0.40,
        &climate_config,
    );
    println!("✓ [2/3] Atmospheric Climate completed:     {:.2?}", c_start.elapsed());

    // 3. Hydrology
    let h_start = Instant::now();
    let pit_res = fill_depressions_and_find_lakes(&result.heightmap, 0.40, &hydro_config);
    let flow_res = calculate_flow(
        &pit_res.filled_heightmap,
        &moisture.precipitation,
        0.40,
        &hydro_config,
    );
    let _river_res = extract_rivers_and_basins(
        &pit_res.filled_heightmap,
        &flow_res,
        0.40,
        &hydro_config,
    );
    println!("✓ [3/3] Hydrology & River routing done:    {:.2?}", h_start.elapsed());

    println!("-------------------------------------------------------");
    println!("Total Generation Time:                     {:.2?}", total_start.elapsed());
    println!("-------------------------------------------------------");

    // Export High-Resolution Physical Atlas Map
    let output_file = "world_physical_map.png";
    let exp_start = Instant::now();

    export::save_physical_map(
        &result.heightmap,
        &temp_grid,
        &moisture.precipitation,
        &flow_res.accumulated_flow,
        &pit_res.is_lake,
        0.40,
        hydro_config.min_river_flow,
        hydro_config.major_river_flow,
        output_file,
    )
    .expect("Failed to export physical map");

    println!("✓ Rendered High-Definition 2D Map in:      {:.2?}", exp_start.elapsed());
    println!("  -> Exported to '{}'", output_file);
    println!("=======================================================");
}