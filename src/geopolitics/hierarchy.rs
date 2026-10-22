use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvinceNode {
    pub id: u32,
    pub center_x: f32,
    pub center_y: f32,
    pub pixel_count: usize,
    pub neighbors: Vec<(u32, u32)>, // NEW: (Neighbor ID, Edge Cost)

    pub area_id: u32,
    pub region_id: u32,
    pub superregion_id: u32,
    pub continent_id: u32,
    pub anchor_id: u32, // ID of closest mainland coastal province (or self if mainland, 0 if remote ocean)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeopoliticalHierarchy {
    pub total_provinces: usize,
    pub total_areas: usize,
    pub total_regions: usize,
    pub total_superregions: usize,
    pub total_continents: usize,
    pub provinces: Vec<ProvinceNode>,
}