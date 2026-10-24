# Project Genesis

A procedural world generator and interactive desktop atlas viewer written in Rust.

The engine grows an 8K ($8192 \times 4096$) planet from 3D spherical noise, simulated
atmospheric circulation and hydrology, and then partitions **both the landmasses and the
world ocean** into a matching five-tier political hierarchy. Everything below was rendered
from a single cached world (seed `3286592765982`), and a given seed always reproduces
the same planet — every stage is order-stable.

---

## Atlas gallery

### Map scopes

| Scope | What it portrays |
| :--- | :--- |
| **🗺 Physical** | Pure relief: hillshaded hypsometric palette, biomes, rivers and ice. No politics. |
| **⛰ Topographic** | A survey sheet: colour *is* elevation, contoured at every band boundary, with metre readouts on hover. |
| **🏛 Land** | Political subdivision of the landmasses only; the ocean is plain water. |
| **🌊 Water** | The hydro-political subdivision of the world ocean only; the continents are muted. |
| **🌍 Everywhere** | Both hierarchies portrayed at once. |

| 🗺 Physical | ⛰ Topographic | 🏛 Land | 🌊 Water | 🌍 Everywhere |
| :---: | :---: | :---: | :---: | :---: |
| [![Physical](atlas_physical.png)](atlas_physical.png) | [![Topographic](atlas_topographic.png)](atlas_topographic.png) | [![Land](atlas_land_regions.png)](atlas_land_regions.png) | [![Water](atlas_water_regions.png)](atlas_water_regions.png) | [![Everywhere](atlas_everywhere_regions.png)](atlas_everywhere_regions.png) |

### Land tiers

The land hierarchy runs **Province → Area → Region → Superregion → Continent**. Each tier
is a pure reaggregation of the one below it, so switching tiers never moves a border, it
only coarsens the palette.

| Provinces | Areas | Regions | Superregions | Continents |
| :---: | :---: | :---: | :---: | :---: |
| [![Land provinces](atlas_land_provinces.png)](atlas_land_provinces.png) | [![Land areas](atlas_land_areas.png)](atlas_land_areas.png) | [![Land regions](atlas_land_regions.png)](atlas_land_regions.png) | [![Land superregions](atlas_land_superregions.png)](atlas_land_superregions.png) | [![Land continents](atlas_land_continents.png)](atlas_land_continents.png) |

### Ocean tiers

The world ocean — open water **and** the frozen polar seas — mirrors the same five tiers,
scaled up because it is one connected, far larger domain.

| Marine provinces | Oceanic areas | Oceanic regions | Oceanic superregions | Continental basins |
| :---: | :---: | :---: | :---: | :---: |
| [![Marine provinces](atlas_water_provinces.png)](atlas_water_provinces.png) | [![Oceanic areas](atlas_water_areas.png)](atlas_water_areas.png) | [![Oceanic regions](atlas_water_regions.png)](atlas_water_regions.png) | [![Oceanic superregions](atlas_water_superregions.png)](atlas_water_superregions.png) | [![Ocean basins](atlas_water_basins.png)](atlas_water_basins.png) |

---

## This world at a glance

| Metric | Value |
| :--- | :--- |
| Surface | **39.4 % land / 60.6 % ocean** |
| Highest peak | **+7 019 m** |
| Median land elevation | **+750 m (90 % of land below +2 050 m)** |
| Median ocean depth | **−3 950 m** |
| Deepest trench | **−10 725 m** |
| Continental shelf (≤ −200 m) | **9.3 % of the ocean** |

---

## Vertical datum

Terrain maths runs in a normalized heightmap coordinate, but everything the atlas
*reports* is in metres, from one calibrated datum ([`src/core/relief.rs`](src/core/relief.rs)):

| Heightmap | Metres | Reference level |
| :--- | :--- | :--- |
| `1.00` | `+9 000 m` | highest possible peak |
| `0.40` | `0 m` | sea level — the datum |
| `0.00` | `-11 000 m` | deepest possible trench floor |

The scale is piecewise, as on a real hypsometric chart: a heightmap unit is worth ~15 km of
continental relief above the water line and ~27.5 km of abyssal relief below it, so mountains
are not flattened and trenches are not absurd. The raw ocean floor is pushed through an
explicit shelf → slope → abyssal plain → trench transfer pinned at the water line, so it
gains real bathymetry without moving a single coastline.

Both poles are permanently glaciated **land** ([`src/core/polar.rs`](src/core/polar.rs)):
the generator lifts each cap above the water line, so the atlas reports elevation (never
depth) there, and the usual land rules — biomes, wasteland, provinces — apply to it.

---

## Pipeline

1. **Geology & Tectonics** — continental crust and cordilleras evaluated on a 3D unit sphere,
   then hydraulic and thermal erosion passes. Closed off by the permanent polar ice sheets.
2. **Atmosphere & Climate** — ITCZ convection, Hadley-cell downwelling, Coriolis trade winds
   and orographic rain shadows, with an altitude lapse rate read through the datum.
3. **Hydrology** — Barnes Priority-Flood depression resolution, rift lakes, and D8 flow
   accumulation driving dendritic river systems.
4. **Geopolitical Hierarchy (land)** — habitability-weighted province seeding snapping to
   mountain crests, rivers and coasts, clustered into areas → regions → superregions →
   continents. Geography-driven wastelands (desert hearts, high massifs, rainforest interiors,
   polar ice) are impassable, but coastal and river corridors stay settled so no landmass is
   ever severed.
5. **Hydro-Political Hierarchy (ocean)** — marine provinces by Poisson-disc seeding plus a
   relaxed Dijkstra flood fill, with bathymetry as a soft barrier, clustered into oceanic
   areas → regions → superregions → continental basins.
6. **Cartography & Rendering** — multi-angle hillshading, an atlas hypsometric palette, and
   the contoured *Topographic* survey sheet, shared verbatim by the headless exporter and the
   interactive viewer so the two can never disagree.

---

## Usage

```bash
# Generate a new world from a seed and save its cache
cargo run --release -- 3286592765982 --save

# Launch the interactive atlas viewer from cached data
cargo run --release -- 3286592765982 --load --viewer

# Generate headless with a random seed
cargo run --release

# Re-render every README plate (all scopes × tiers) from a cached world
cargo run --release -- 3286592765982 --load --plates
```

Every headless run prints the world's hypsometric profile in metres and exports
`world_physical_map.png`, `world_topographic_map.png` (banded elevation survey) and
`world_political_map.png` (land hierarchy). Passing `--plates` additionally writes the
`atlas_*.png` scope and tier plates used above, straight from the viewer's own painter.